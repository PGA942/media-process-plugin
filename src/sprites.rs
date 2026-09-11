//! 手続き的に描く 3 つのスタンプ(スマイル / ハート / スター)。フォント無し、数式だけで 96×96 RGBA を作る。
//! 3×3 のスーパーサンプリングで縁を滑らかにする。

use image::{Rgba, RgbaImage};
use std::sync::OnceLock;

pub const SPRITE_SIZE: u32 = 96;
pub const SPRITE_COUNT: usize = 3;

/// スタンプの表示名(FOW のラベル)。
pub fn label(i: usize) -> &'static str {
    match i {
        0 => "スマイル",
        1 => "ハート",
        _ => "スター",
    }
}

/// `i` 番目のスプライト(キャッシュ)。範囲外は最後のもの。
pub fn sprite(i: usize) -> &'static RgbaImage {
    static CACHE: OnceLock<Vec<RgbaImage>> = OnceLock::new();
    let all = CACHE.get_or_init(|| (0..SPRITE_COUNT).map(render).collect());
    &all[i.min(SPRITE_COUNT - 1)]
}

/// 色 + 不透明度(0–1)。座標は中心原点、半径 1 = 半辺。
type Shade = fn(f32, f32) -> Option<([u8; 3], f32)>;

fn render(i: usize) -> RgbaImage {
    let shade: Shade = match i {
        0 => smiley,
        1 => heart,
        _ => star,
    };
    let n = SPRITE_SIZE;
    let mut img = RgbaImage::new(n, n);
    const SS: u32 = 3;
    for y in 0..n {
        for x in 0..n {
            let mut acc = [0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let fx = (x as f32 + (sx as f32 + 0.5) / SS as f32) / n as f32 * 2.0 - 1.0;
                    let fy = (y as f32 + (sy as f32 + 0.5) / SS as f32) / n as f32 * 2.0 - 1.0;
                    if let Some((rgb, a)) = shade(fx, fy) {
                        acc[0] += rgb[0] as f32 * a;
                        acc[1] += rgb[1] as f32 * a;
                        acc[2] += rgb[2] as f32 * a;
                        acc[3] += a;
                    }
                }
            }
            let samples = (SS * SS) as f32;
            let a = acc[3] / samples;
            let px = if acc[3] > 0.0 {
                Rgba([(acc[0] / acc[3]).round() as u8, (acc[1] / acc[3]).round() as u8, (acc[2] / acc[3]).round() as u8, (a * 255.0).round() as u8])
            } else {
                Rgba([0, 0, 0, 0])
            };
            img.put_pixel(x, y, px);
        }
    }
    img
}

const OUTLINE: [u8; 3] = [40, 30, 30];

/// スマイル: 黄色い円 + 目 2 つ + 口の弧。
fn smiley(x: f32, y: f32) -> Option<([u8; 3], f32)> {
    let r = (x * x + y * y).sqrt();
    if r > 0.96 {
        return None;
    }
    if r > 0.88 {
        return Some((OUTLINE, 1.0));
    }
    // 目
    for ex in [-0.34f32, 0.34] {
        let d = ((x - ex).powi(2) + (y + 0.28).powi(2)).sqrt();
        if d < 0.11 {
            return Some((OUTLINE, 1.0));
        }
    }
    // 口: 中心 (0, 0.05)、半径 0.5 の弧の下半分、太さ 0.09
    let md = (x * x + (y - 0.05).powi(2)).sqrt();
    if (md - 0.5).abs() < 0.045 && y > 0.25 {
        return Some((OUTLINE, 1.0));
    }
    Some(([255, 205, 40], 1.0))
}

/// ハート: 陰関数 (x² + y² − 1)³ − x² y³ ≤ 0(y は上向き正なので反転)。
fn heart(x: f32, y: f32) -> Option<([u8; 3], f32)> {
    let hx = x * 1.25;
    let hy = -y * 1.25 + 0.15;
    let f = |s: f32| {
        let (a, b) = (hx * s, hy * s);
        (a * a + b * b - 1.0).powi(3) - a * a * b.powi(3)
    };
    if f(1.0) > 0.0 {
        return None;
    }
    // 少し縮めた形の外側なら縁
    if f(1.12) > 0.0 {
        return Some((OUTLINE, 1.0));
    }
    Some(([235, 60, 80], 1.0))
}

/// スター: 5 角星(外径 0.95 / 内径 0.42)、偶奇規則で内外判定。
fn star(x: f32, y: f32) -> Option<([u8; 3], f32)> {
    let inside = |scale: f32| {
        let pts: Vec<(f32, f32)> = (0..10)
            .map(|k| {
                let ang = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
                let rad = if k % 2 == 0 { 0.95 } else { 0.42 } * scale;
                (ang.cos() * rad, ang.sin() * rad + 0.08)
            })
            .collect();
        let mut hit = false;
        for i in 0..pts.len() {
            let (x1, y1) = pts[i];
            let (x2, y2) = pts[(i + 1) % pts.len()];
            if (y1 > y) != (y2 > y) {
                let t = (y - y1) / (y2 - y1);
                if x < x1 + t * (x2 - x1) {
                    hit = !hit;
                }
            }
        }
        hit
    };
    if !inside(1.0) {
        return None;
    }
    if !inside(0.86) {
        return Some((OUTLINE, 1.0));
    }
    Some(([255, 190, 30], 1.0))
}
