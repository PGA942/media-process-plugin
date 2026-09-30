//! 顔検知。SeetaFace の FuSt カスケード(`rustface` = 純 Rust 移植)を **wasm の中で**動かす。
//!
//! ホストに ONNX ランタイムが無いので YuNet は使えない。モデル(`models/seeta_fd_frontal_v1.0.bin`、約 1.2 MB、正面顔)を
//! `include_bytes!` で埋め込み、初回だけ [`rustface::read_model`] でパースして検知器を保持する。
//! 入力は長辺 ≤ [`DETECT_MAX_SIDE`] のグレースケール(呼び出し側が縮める)。

use crate::ops::{self, RectF};
use image::{GrayImage, RgbaImage};
use rustface::{Detector, ImageData};
use std::cell::RefCell;

/// 検知に使う縮小画像の長辺(px)。wasm はインタープリタで動くので小さめに(自撮り / ポートレートはこれで十分)。
pub const DETECT_MAX_SIDE: u32 = 480;
/// 1 回目で顔が無かったときだけ、この長辺で再検知する(集合写真の小さい顔。カスケードの最小窓が 20px なので、
/// 480px では 1/24 より小さい顔は原理的に拾えない)。処理量は約 4 倍。
pub const DETECT_RETRY_SIDE: u32 = 960;
/// 検知する最小の顔(検知画像 px)。
const MIN_FACE_SIZE: u32 = 20;
/// スコア閾値(小さいほど拾いやすい。rustface の例は 2.0、既定 3.85)。取りこぼしより誤検知の方が
/// トグルで外せるので低めにする。
const SCORE_THRESH: f64 = 2.0;

static MODEL_BYTES: &[u8] = include_bytes!("../../../models/seeta_fd_frontal_v1.0.bin");

thread_local! {
    static DETECTOR: RefCell<Option<Box<dyn Detector>>> = RefCell::new(None);
}

fn build_detector() -> Result<Box<dyn Detector>, String> {
    let model = rustface::read_model(MODEL_BYTES).map_err(|e| format!("model: {e}"))?;
    let mut d = rustface::create_detector_with_model(model);
    d.set_min_face_size(MIN_FACE_SIZE);
    d.set_score_thresh(SCORE_THRESH);
    d.set_pyramid_scale_factor(0.8);
    d.set_slide_window_step(4, 4);
    Ok(d)
}

/// グレースケール画像の顔をすべて返す(検知画像 px の矩形)。空画像は空。モデルが壊れていれば `Err`。
pub fn detect(gray: &GrayImage) -> Result<Vec<RectF>, String> {
    let (w, h) = gray.dimensions();
    if w < MIN_FACE_SIZE || h < MIN_FACE_SIZE {
        return Ok(Vec::new());
    }
    DETECTOR.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(build_detector()?);
        }
        let det = slot.as_mut().expect("detector");
        let data = ImageData::new(gray.as_raw(), w, h);
        let mut faces: Vec<RectF> = det
            .detect(&data)
            .iter()
            .map(|f| {
                let b = f.bbox();
                RectF::new(b.x() as f32, b.y() as f32, b.width() as f32, b.height() as f32)
            })
            .collect();
        // 左上から順に安定した番号を振れるように並べる
        faces.sort_by(|a, b| (a.y, a.x).partial_cmp(&(b.y, b.x)).unwrap_or(std::cmp::Ordering::Equal));
        Ok(faces)
    })
}

/// フル画像の顔を検知して**フル画像 px** の矩形で返す。
/// 長辺 ≤ [`DETECT_MAX_SIDE`] のグレースケールで検知し、無ければ長辺 ≤ [`DETECT_RETRY_SIDE`] で 1 回だけやり直す。
pub fn detect_in_image(full: &RgbaImage) -> Result<Vec<RectF>, String> {
    let full_dims = full.dimensions();
    let longest = full_dims.0.max(full_dims.1);
    let mut sides = vec![DETECT_MAX_SIDE];
    if longest > DETECT_MAX_SIDE {
        sides.push(DETECT_RETRY_SIDE.min(longest));
    }
    for side in sides {
        let small = ops::downscale(full, side);
        let found = detect(&ops::to_gray(&small))?;
        if !found.is_empty() {
            return Ok(found.into_iter().map(|r| r.map(small.dimensions(), full_dims)).collect());
        }
    }
    Ok(Vec::new())
}
