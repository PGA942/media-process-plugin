//! media-process-plugin — スロット `composer.media`(CONTRACT.md §2.2、v0.5)の「加工」サンプル。
//!
//! - **顔ぼかし**: wasm の中で顔を検知(SeetaFace FuSt カスケード = `rustface`、[`faces`])し、顔ごとにボックスぼかし。
//!   検知した顔は重ね(`composer.media.overlay`)に枠 `Box` で出し、タップで顔ごとにぼかしの on / off。
//! - **スタンプ**: 下付き FOW(`sticker-pick`)から 3 つの手続き的スプライト([`sprites`])を**オブジェクト**として貼る。
//!   重ねの枠をドラッグで移動、選択中はハンドルで拡縮(縦横比固定、最小 16px)/ 回転、ボタンで削除 / 複製。
//! - **リセット**: 顔のトグルとオブジェクトを捨てて元のプレビューに戻す。
//!
//! | 入口 | 中身 |
//! |---|---|
//! | `plugin.manifest` | [`rasq_plugin_kit::manifest`] |
//! | `media.catalog {}` | 静的な一覧 [`catalog`](ジャンル `process`: `faces` / `sticker` / `reset`)。セッション不要 |
//! | `media.edit {token, index, name, mimeType, bytesB64}` | デコード → 保持(長辺 ≤ 2048)→ 初期プレビューを push(`width`/`height` 付き)→ 戻り `{genres}` |
//! | `media.action {token, genre:"process", action}` | `faces`: 検知 → ぼかし → プレビュー + 重ね(無ければ toast)。`sticker`: FOW を開く。`reset`: 初期化。戻り `{}` |
//! | `face.<i>`(重ねの `tap`) | 顔 `i` のぼかしをトグル → プレビュー + 重ねを出し直す |
//! | `sticker.add.<i>`(FOW の `tap`) | スプライト `i` をプレビュー中央に置いて選択 → プレビュー + 重ね |
//! | `obj.<i>.move` / `obj.<i>.scale` / `obj.<i>.rotate`(重ねの `drag`) | オブジェクト `i` を移動 / 拡縮 / 回転(payload `{phase, x, y, dx, dy}`) |
//! | `obj.<i>.move`(`tap`)/ `obj.<i>.delete` / `obj.<i>.dup`(`tap`) | 選択 / 削除 / 複製(+16px ずらす) |
//! | `closeSticker`(`tap`)/ `stickerClosed` | FOW を閉じる / ホスト側で閉じられた |
//! | `media.commit {token}` | フル解像度で ぼかし(有効な顔)→ スタンプ合成 → PNG → **戻り値で** `{bytesB64, mimeType, name}`。重ねを消し FOW を閉じ、状態を捨てる |
//! | `media.cancel {token}` | 重ねを消し FOW を閉じ、状態を捨てる |
//!
//! 座標系: 顔の矩形はフル画像 px で持ち、プレビュー / 検知画像へは [`ops::RectF::map`] で写す。オブジェクトは
//! プレビュー px で持ち(重ねの座標系と同じ)、適用時に [`ops::Sticker::scaled`] でフル画像へ写す。

pub mod faces;
pub mod ops;
pub mod sprites;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ImageEncoder, ImageFormat, RgbaImage};
use ops::{RectF, Sticker};
use rasq_plugin_kit::serde_json::{json, Value};
use rasq_plugin_kit::{err, fow_spec, host, manifest, rasq_plugin, rrl, unsupported, Surface};
use std::sync::{Mutex, OnceLock};

const PLUGIN_ID: &str = "sample.media-process";
const VERSION: &str = "0.1.0";
const PLUGIN_NAME: &str = "加工(顔ぼかし / スタンプ)(サンプル)";
const DESCRIPTION: &str = "顔を検知してぼかす、スタンプをオブジェクトとして貼る(移動 / 拡縮 / 回転 / 削除 / 複製)";

/// プレビューの長辺(px)。重ねの座標系もこれ。
pub const PREVIEW_MAX_SIDE: u32 = 360;
/// 適用時の長辺上限(px)。これを超える画像は先に縮小する。
pub const APPLY_MAX_SIDE: u32 = 2048;
/// オブジェクトの最小の一辺(プレビュー px)。
pub const MIN_OBJECT_SIZE: f32 = 16.0;
/// 複製のずらし幅(プレビュー px)。
pub const DUP_OFFSET: f32 = 16.0;
/// 検知した顔の矩形をこの割合だけ広げてぼかす(髪 / 顎まで)。
const FACE_EXPAND: f32 = 0.25;
/// ぼかしのパス数(2 で近似ガウス)。
const BLUR_PASSES: u32 = 2;

/// 重ねのハンドル / ボタンの大きさ(プレビュー px)。
const HANDLE: f64 = 20.0;
const BUTTON: f64 = 26.0;
/// 回転ハンドルの、オブジェクト上辺からの距離。
const ROTATE_GAP: f32 = 28.0;

/// スタンプ選択 FOW の id。
pub const STICKER_FOW_ID: &str = "sticker-pick";
static STICKER_FOW: Surface = Surface::new(STICKER_FOW_ID);

pub const GENRE_PROCESS: &str = "process";

/// `media.catalog` / `media.edit` の戻り `{genres: [{id, actions: [{id, label, icon}]}]}`。
/// 既定ジャンル `process` なので `label` / `icon` はホストに任せる。
pub fn catalog() -> Value {
    json!({
        "genres": [
            {"id": GENRE_PROCESS, "actions": [
                // 顔ぼかし(`faces`)は実機で検知できなかったので一覧から外した(呼べば動く。ONNX の `ml.infer` が
                // つながったら YuNet で戻す予定)。
                {"id": "sticker", "label": "スタンプ", "icon": "emoji_emotions"},
                {"id": "reset", "label": "リセット", "icon": "restart_alt"},
            ]},
        ]
    })
}

// ---------------------------------------------------------------------------
// セッション
// ---------------------------------------------------------------------------

/// 検知した顔(フル画像 px)と、ぼかしが有効か。
#[derive(Clone, Copy, Debug)]
struct Face {
    rect: RectF,
    enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragKind {
    Move,
    Scale,
    Rotate,
}

/// 進行中のドラッグ(拡縮 / 回転はハンドルの位置を積算する)。
#[derive(Clone, Copy, Debug)]
struct Drag {
    kind: DragKind,
    index: usize,
    /// ハンドルの現在位置(プレビュー px、start から dx/dy を積算)
    point: (f32, f32),
    /// 拡縮: 開始時の 中心-ハンドル 距離と一辺、ハンドル方向の単位ベクトル(射影して縮める)
    start_dist: f32,
    start_size: f32,
    dir: (f32, f32),
}

struct Session {
    token: String,
    name: String,
    /// フル解像度(長辺 ≤ APPLY_MAX_SIDE に丸め済み)
    full: RgbaImage,
    /// 縮小した元画像(プレビュー用。加工前)
    preview_src: RgbaImage,
    faces: Vec<Face>,
    /// プレビュー px のオブジェクト
    objects: Vec<Sticker>,
    selected: Option<usize>,
    drag: Option<Drag>,
    sticker_fow: bool,
    /// 重ねを出しているか(消すときだけ `root: null` を送る)
    overlay_shown: bool,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

type SessionGuard = std::sync::MutexGuard<'static, Option<Session>>;

fn lock_session() -> Result<SessionGuard, Value> {
    SESSION.lock().map_err(|_| err("D01", "state poisoned"))
}

/// セッションがあり、`token` が一致すること(`token` 無しの内部イベントは `None` で素通し)。
fn session_mut<'a>(guard: &'a mut SessionGuard, token: Option<&str>) -> Result<&'a mut Session, Value> {
    let s = guard.as_mut().ok_or_else(|| err("D01", "no image (call media.edit first)"))?;
    if let Some(t) = token {
        if !t.is_empty() && t != s.token {
            return Err(err("D01", format!("unknown token: {t}")));
        }
    }
    Ok(s)
}

impl Session {
    fn preview_size(&self) -> (u32, u32) {
        self.preview_src.dimensions()
    }


    /// ぼかし(有効な顔)→ スタンプ合成 を `target`(フル画像 or プレビュー)に描く。
    fn render_into(&self, target: &RgbaImage) -> RgbaImage {
        let mut img = target.clone();
        let full = self.full.dimensions();
        let dims = img.dimensions();
        for f in self.faces.iter().filter(|f| f.enabled) {
            let r = f.rect.expanded(FACE_EXPAND).map(full, dims);
            if let Some(ir) = r.clamp_to(dims.0, dims.1) {
                ops::blur_rect(&mut img, ir, ops::blur_radius_for(ir.2 as f32, ir.3 as f32), BLUR_PASSES);
            }
        }
        let (pw, ph) = self.preview_size();
        let sx = dims.0 as f32 / pw as f32;
        let sy = dims.1 as f32 / ph as f32;
        for o in &self.objects {
            ops::composite_sprite(&mut img, sprites::sprite(o.sprite), &o.scaled(sx, sy));
        }
        img
    }

    fn render_preview(&self) -> RgbaImage {
        self.render_into(&self.preview_src)
    }

    fn render_full(&self) -> RgbaImage {
        self.render_into(&self.full)
    }
}

// ---------------------------------------------------------------------------
// 画像 ⇄ bytes
// ---------------------------------------------------------------------------

fn decode_image(mime: &str, b64: &str) -> Result<RgbaImage, Value> {
    let format = match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" | "image/jpg" => ImageFormat::Jpeg,
        other => return Err(err("C01", format!("unsupported mimeType: {other}"))),
    };
    let bytes = B64.decode(b64.trim()).map_err(|e| err("D01", format!("bytesB64: {e}")))?;
    let img = image::load_from_memory_with_format(&bytes, format).map_err(|e| err("D01", format!("decode: {e}")))?;
    let rgba = img.to_rgba8();
    if rgba.width() == 0 || rgba.height() == 0 {
        return Err(err("D01", "empty image"));
    }
    Ok(rgba)
}

/// PNG(速度優先の圧縮)にエンコードする。
pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, Value> {
    let mut out = Vec::with_capacity((img.width() * img.height()) as usize);
    let enc = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::NoFilter);
    enc.write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8)
        .map_err(|e| err("D01", format!("encode: {e}")))?;
    Ok(out)
}

/// PNG の生 base64(`data:` URI ではない)。
fn png_b64(img: &RgbaImage) -> Result<String, Value> {
    Ok(B64.encode(encode_png(img)?))
}

/// `foo.jpg` → `foo.png`(拡張子が無ければそのまま `.png` を足す)。
fn png_name(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{}.png", &name[..i]),
        _ => format!("{name}.png"),
    }
}

/// スプライトの `data:image/png;base64,…`(FOW の `Image.src`)。
fn sprite_data_uri(i: usize) -> &'static str {
    static URIS: OnceLock<Vec<String>> = OnceLock::new();
    let all = URIS.get_or_init(|| {
        (0..sprites::SPRITE_COUNT)
            .map(|i| format!("data:image/png;base64,{}", B64.encode(encode_png(sprites::sprite(i)).unwrap_or_default())))
            .collect()
    });
    &all[i.min(sprites::SPRITE_COUNT - 1)]
}

// ---------------------------------------------------------------------------
// プレビュー / 重ね / FOW
// ---------------------------------------------------------------------------

fn push_preview(s: &Session) -> Result<(), Value> {
    let preview = s.render_preview();
    let b64 = png_b64(&preview)?;
    host::composer_media_preview(&s.token, &b64, "image/png", Some(preview.dimensions()));
    Ok(())
}

fn sized_box(key: &str, x: f32, y: f32, w: f32, h: f32, props: Value, children: Vec<Value>) -> Value {
    let mut p = props;
    p["width"] = json!(w.max(0.0) as f64);
    p["height"] = json!(h.max(0.0) as f64);
    rrl::positioned(rrl::boxed(key, p, children), x as f64, y as f64)
}

/// 重ねの RRL(プレビュー px の `Stack`)。顔の枠 + オブジェクトの枠 + 選択中のハンドル / ボタン。
fn overlay_root(s: &Session) -> Value {
    let (pw, ph) = s.preview_size();
    let full = s.full.dimensions();
    let mut children = Vec::new();

    for (i, f) in s.faces.iter().enumerate() {
        let r = f.rect.expanded(FACE_EXPAND).map(full, (pw, ph));
        let (color, border) = if f.enabled { ("#34C759", 2.0) } else { ("#FFFFFF99", 1.5) };
        let node = sized_box(&format!("face.{i}"), r.x, r.y, r.w, r.h, json!({"color": "#00000000", "borderColor": color, "borderWidth": border, "radius": 6}), vec![]);
        children.push(rrl::with_on(node, "tap", &format!("face.{i}")));
    }

    for (i, o) in s.objects.iter().enumerate() {
        let selected = s.selected == Some(i);
        let b = o.bbox();
        let (color, border) = if selected { ("#FF9500", 2.0) } else { ("#FFFFFFB0", 1.0) };
        let node = sized_box(&format!("obj.{i}"), b.x, b.y, b.w, b.h, json!({"color": "#00000000", "borderColor": color, "borderWidth": border, "radius": 4}), vec![]);
        let node = rrl::with_on(node, "drag", &format!("obj.{i}.move"));
        children.push(rrl::with_on(node, "tap", &format!("obj.{i}.move")));
    }

    if let Some(i) = s.selected.filter(|&i| i < s.objects.len()) {
        let o = &s.objects[i];
        let b = o.bbox();
        let handle_props = json!({"color": "#FF9500", "borderColor": "#FFFFFF", "borderWidth": 2, "radius": HANDLE / 2.0});
        // 拡縮: 右下の角
        let (hx, hy) = scale_handle_center(o);
        children.push(rrl::with_on(
            sized_box(&format!("obj.{i}.scale"), hx - HANDLE as f32 / 2.0, hy - HANDLE as f32 / 2.0, HANDLE as f32, HANDLE as f32, handle_props.clone(), vec![]),
            "drag",
            &format!("obj.{i}.scale"),
        ));
        // 回転: オブジェクトの「上」(回転に追従)
        let (rx, ry) = rotate_handle_center(o);
        children.push(rrl::with_on(
            sized_box(
                &format!("obj.{i}.rotate"),
                rx - HANDLE as f32 / 2.0,
                ry - HANDLE as f32 / 2.0,
                HANDLE as f32,
                HANDLE as f32,
                json!({"color": "#007AFF", "borderColor": "#FFFFFF", "borderWidth": 2, "radius": HANDLE / 2.0}),
                vec![],
            ),
            "drag",
            &format!("obj.{i}.rotate"),
        ));
        // ボタン: 枠の下に 削除 / 複製
        let bx = b.x;
        let by = (b.y + b.h + 6.0).min(ph as f32 - BUTTON as f32);
        let button_props = json!({"color": "#000000B0", "borderColor": "#FFFFFF", "borderWidth": 1, "radius": BUTTON / 2.0});
        children.push(rrl::with_on(
            sized_box(&format!("obj.{i}.delete"), bx, by, BUTTON as f32, BUTTON as f32, button_props.clone(), vec![rrl::node("Icon", &format!("obj.{i}.delete.icon"), json!({"icon": "delete", "color": "#FFFFFF"}), vec![])]),
            "tap",
            &format!("obj.{i}.delete"),
        ));
        children.push(rrl::with_on(
            sized_box(
                &format!("obj.{i}.dup"),
                bx + BUTTON as f32 + 6.0,
                by,
                BUTTON as f32,
                BUTTON as f32,
                button_props,
                vec![rrl::node("Icon", &format!("obj.{i}.dup.icon"), json!({"icon": "content_copy", "color": "#FFFFFF"}), vec![])],
            ),
            "tap",
            &format!("obj.{i}.dup"),
        ));
    }

    rrl::sized_stack("overlay", pw as f64, ph as f64, children)
}

/// 拡縮ハンドルの中心 = バウンディングボックスの右下。
fn scale_handle_center(o: &Sticker) -> (f32, f32) {
    let b = o.bbox();
    (b.x + b.w, b.y + b.h)
}

/// 回転ハンドルの中心 = オブジェクトの上辺の外(回転に追従)。
fn rotate_handle_center(o: &Sticker) -> (f32, f32) {
    o.local_to_world(0.0, -(o.size / 2.0 + ROTATE_GAP))
}

/// 重ねを出し直す(顔もオブジェクトも無ければ消す)。
fn sync_overlay(s: &mut Session) {
    if s.faces.is_empty() && s.objects.is_empty() {
        if s.overlay_shown {
            host::composer_media_overlay(&s.token, Value::Null);
            s.overlay_shown = false;
        }
        return;
    }
    host::composer_media_overlay(&s.token, overlay_root(s));
    s.overlay_shown = true;
}

fn clear_overlay(s: &mut Session) {
    if s.overlay_shown {
        host::composer_media_overlay(&s.token, Value::Null);
        s.overlay_shown = false;
    }
}

/// プレビューと重ねを両方出し直す。
fn refresh(s: &mut Session) -> Result<(), Value> {
    push_preview(s)?;
    sync_overlay(s);
    Ok(())
}

fn sticker_fow_root() -> Value {
    let tiles = (0..sprites::SPRITE_COUNT)
        .map(|i| {
            let img = rrl::with_on(rrl::image_sized(&format!("sticker.{i}"), sprite_data_uri(i), 64.0, 64.0), "tap", &format!("sticker.add.{i}"));
            rrl::column(&format!("sticker.{i}.col"), vec![img, rrl::text(&format!("sticker.{i}.label"), sprites::label(i))])
        })
        .collect();
    rrl::column("root", vec![rrl::row("stickers", tiles), rrl::spacer("sp", 8.0), rrl::button("close", "閉じる", "closeSticker")])
}

fn open_sticker_fow(s: &mut Session) {
    if s.sticker_fow {
        return;
    }
    s.sticker_fow = STICKER_FOW.open(fow_spec("スタンプ", 0.28), sticker_fow_root(), Some("stickerClosed"), json!({}));
}

fn close_sticker_fow(s: &mut Session) {
    if std::mem::take(&mut s.sticker_fow) {
        STICKER_FOW.close(json!({}));
    }
}

/// セッションを畳む(重ねを消し、FOW を閉じる)。
fn teardown(s: &mut Session) {
    clear_overlay(s);
    close_sticker_fow(s);
}

// ---------------------------------------------------------------------------
// call ハンドラ
// ---------------------------------------------------------------------------

fn media_edit(args: Value) -> Result<Value, Value> {
    let token = args["token"].as_str().unwrap_or("").to_string();
    let name = args["name"].as_str().unwrap_or("image").to_string();
    let mime = args["mimeType"].as_str().ok_or_else(|| err("D01", "mimeType missing"))?;
    let b64 = args["bytesB64"].as_str().ok_or_else(|| err("D01", "bytesB64 missing"))?;

    let decoded = decode_image(mime, b64)?;
    let full = ops::downscale(&decoded, APPLY_MAX_SIDE);
    drop(decoded);
    let preview_src = ops::downscale(&full, PREVIEW_MAX_SIDE);

    let mut guard = lock_session()?;
    if let Some(old) = guard.as_mut() {
        teardown(old);
    }
    let session = Session {
        token,
        name,
        full,
        preview_src,
        faces: Vec::new(),
        objects: Vec::new(),
        selected: None,
        drag: None,
        sticker_fow: false,
        overlay_shown: false,
    };
    push_preview(&session)?;
    *guard = Some(session);
    Ok(catalog())
}

/// `process/faces`: 縮小グレースケールで検知(無ければ一段大きい縮小で再検知)→ フル画像 px の矩形 → ぼかして出し直す。
fn action_faces(s: &mut Session) -> Result<Value, Value> {
    let found = faces::detect_in_image(&s.full).map_err(|e| err("D01", e))?;
    s.faces = found.into_iter().map(|r| Face { rect: r, enabled: true }).collect();
    if s.faces.is_empty() {
        host::toast("顔が見つかりませんでした");
        // 前回の顔枠も無くなったので重ねだけ整える(オブジェクトが無ければ消える)
        sync_overlay(s);
        return Ok(json!({}));
    }
    refresh(s)?;
    Ok(json!({}))
}

fn action_reset(s: &mut Session) -> Result<Value, Value> {
    s.faces.clear();
    s.objects.clear();
    s.selected = None;
    s.drag = None;
    clear_overlay(s);
    push_preview(s)?;
    Ok(json!({}))
}

fn media_action(args: Value) -> Result<Value, Value> {
    let token = args["token"].as_str().unwrap_or("");
    let genre = args["genre"].as_str().unwrap_or("");
    let action = args["action"].as_str().ok_or_else(|| err("D01", "action missing"))?;
    let mut guard = lock_session()?;
    let s = session_mut(&mut guard, Some(token))?;
    match (genre, action) {
        (GENRE_PROCESS, "faces") => action_faces(s),
        (GENRE_PROCESS, "sticker") => {
            open_sticker_fow(s);
            Ok(json!({}))
        }
        (GENRE_PROCESS, "reset") => action_reset(s),
        _ => Err(err("D01", format!("unknown action: {genre}/{action}"))),
    }
}

fn media_commit(args: Value) -> Result<Value, Value> {
    let token = args["token"].as_str().unwrap_or("");
    let mut guard = lock_session()?;
    let s = session_mut(&mut guard, Some(token))?;
    let out = s.render_full();
    let b64 = png_b64(&out)?;
    let name = png_name(&s.name);
    teardown(s);
    *guard = None;
    Ok(json!({"bytesB64": b64, "mimeType": "image/png", "name": name}))
}

fn media_cancel(args: Value) -> Result<Value, Value> {
    let token = args["token"].as_str().unwrap_or("");
    let mut guard = lock_session()?;
    if let Some(s) = guard.as_mut() {
        if !token.is_empty() && token != s.token {
            return Err(err("D01", format!("unknown token: {token}")));
        }
        teardown(s);
    }
    *guard = None;
    Ok(json!({}))
}

/// `face.<i>`(重ねのタップ): ぼかしのトグル。
fn face_toggle(index: &str, args: &Value) -> Result<Value, Value> {
    let i: usize = index.parse().map_err(|_| err("D01", format!("bad face index: {index}")))?;
    let mut guard = lock_session()?;
    let s = session_mut(&mut guard, args["token"].as_str())?;
    let f = s.faces.get_mut(i).ok_or_else(|| err("D01", format!("no face {i}")))?;
    f.enabled = !f.enabled;
    refresh(s)?;
    Ok(json!({}))
}

/// `sticker.add.<i>`(FOW のタップ): プレビュー中央に置いて選択。
fn sticker_add(index: &str, args: &Value) -> Result<Value, Value> {
    let i: usize = index.parse().map_err(|_| err("D01", format!("bad sticker index: {index}")))?;
    if i >= sprites::SPRITE_COUNT {
        return Err(err("D01", format!("no sticker {i}")));
    }
    let mut guard = lock_session()?;
    let s = session_mut(&mut guard, args["token"].as_str())?;
    let (pw, ph) = s.preview_size();
    let size = (pw.min(ph) as f32 / 3.0).max(MIN_OBJECT_SIZE);
    s.objects.push(Sticker { sprite: i, cx: pw as f32 / 2.0, cy: ph as f32 / 2.0, size, angle: 0.0 });
    s.selected = Some(s.objects.len() - 1);
    s.drag = None;
    refresh(s)?;
    Ok(json!({}))
}

fn drag_payload(args: &Value) -> (String, f32, f32) {
    let phase = args["phase"].as_str().unwrap_or("update").to_string();
    let dx = args["dx"].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0) as f32;
    let dy = args["dy"].as_f64().filter(|v| v.is_finite()).unwrap_or(0.0) as f32;
    (phase, dx, dy)
}

/// `obj.<i>.<op>`: 重ねのオブジェクト操作。
fn object_event(rest: &str, args: &Value) -> Result<Value, Value> {
    let (index, op) = rest.split_once('.').ok_or_else(|| err("D01", format!("bad object event: {rest}")))?;
    let i: usize = index.parse().map_err(|_| err("D01", format!("bad object index: {index}")))?;
    let mut guard = lock_session()?;
    let s = session_mut(&mut guard, args["token"].as_str())?;
    if i >= s.objects.len() {
        return Err(err("D01", format!("no object {i}")));
    }
    let (pw, ph) = s.preview_size();
    let is_drag = args["phase"].is_string() || args["kind"] == json!("drag");
    match op {
        "move" if !is_drag => {
            // タップ = 選択
            s.selected = Some(i);
            s.drag = None;
            sync_overlay(s);
            Ok(json!({}))
        }
        "move" => {
            let (phase, dx, dy) = drag_payload(args);
            s.selected = Some(i);
            let o = &mut s.objects[i];
            o.cx = (o.cx + dx).clamp(0.0, pw as f32);
            o.cy = (o.cy + dy).clamp(0.0, ph as f32);
            s.drag = if phase == "end" { None } else { Some(Drag { kind: DragKind::Move, index: i, point: (0.0, 0.0), start_dist: 0.0, start_size: 0.0, dir: (0.0, 0.0) }) };
            refresh(s)?;
            Ok(json!({}))
        }
        "scale" => {
            let (phase, dx, dy) = drag_payload(args);
            s.selected = Some(i);
            let o = s.objects[i];
            let mut d = match s.drag {
                Some(d) if d.kind == DragKind::Scale && d.index == i && phase != "start" => d,
                _ => {
                    let p = scale_handle_center(&o);
                    let dist = ((p.0 - o.cx).powi(2) + (p.1 - o.cy).powi(2)).sqrt().max(1.0);
                    Drag { kind: DragKind::Scale, index: i, point: p, start_dist: dist, start_size: o.size, dir: ((p.0 - o.cx) / dist, (p.1 - o.cy) / dist) }
                }
            };
            d.point = (d.point.0 + dx, d.point.1 + dy);
            // 中心→ハンドル方向への符号付き射影。内側へ引くほど小さく(中心を越えたら最小)、外側へ引くほど大きく
            let proj = (d.point.0 - o.cx) * d.dir.0 + (d.point.1 - o.cy) * d.dir.1;
            // 縦横比は常に 1:1(スプライトは正方形)。距離の比で一辺を変える
            let max_size = (pw.max(ph) as f32) * 2.0;
            s.objects[i].size = (d.start_size * proj / d.start_dist).clamp(MIN_OBJECT_SIZE, max_size);
            s.drag = if phase == "end" { None } else { Some(d) };
            refresh(s)?;
            Ok(json!({}))
        }
        "rotate" => {
            let (phase, dx, dy) = drag_payload(args);
            s.selected = Some(i);
            let o = s.objects[i];
            let mut d = match s.drag {
                Some(d) if d.kind == DragKind::Rotate && d.index == i && phase != "start" => d,
                _ => Drag { kind: DragKind::Rotate, index: i, point: rotate_handle_center(&o), start_dist: 0.0, start_size: o.size, dir: (0.0, 0.0) },
            };
            d.point = (d.point.0 + dx, d.point.1 + dy);
            // ハンドルは「上」に居るので atan2 に π/2 を足すと角度 0 が真上になる
            let a = (d.point.1 - o.cy).atan2(d.point.0 - o.cx) + std::f32::consts::FRAC_PI_2;
            if a.is_finite() && (d.point.0 - o.cx).abs() + (d.point.1 - o.cy).abs() > 0.5 {
                s.objects[i].angle = a;
            }
            s.drag = if phase == "end" { None } else { Some(d) };
            refresh(s)?;
            Ok(json!({}))
        }
        "delete" => {
            s.objects.remove(i);
            s.drag = None;
            s.selected = if s.objects.is_empty() { None } else { Some(i.min(s.objects.len() - 1)) };
            refresh(s)?;
            Ok(json!({}))
        }
        "dup" => {
            let mut o = s.objects[i];
            o.cx = (o.cx + DUP_OFFSET).min(pw as f32);
            o.cy = (o.cy + DUP_OFFSET).min(ph as f32);
            s.objects.push(o);
            s.selected = Some(s.objects.len() - 1);
            s.drag = None;
            refresh(s)?;
            Ok(json!({}))
        }
        other => Err(err("D01", format!("unknown object op: {other}"))),
    }
}

fn handle(fn_name: &str, args: Value) -> Result<Value, Value> {
    match fn_name {
        "plugin.manifest" => Ok(manifest(PLUGIN_ID, PLUGIN_NAME, VERSION, "action", DESCRIPTION, &["composer.media"], &["fow.widget"])),
        "media.catalog" => Ok(catalog()),
        "media.edit" => media_edit(args),
        "media.action" => media_action(args),
        "media.commit" => media_commit(args),
        "media.cancel" => media_cancel(args),
        "closeSticker" => {
            if let Ok(mut g) = SESSION.lock() {
                if let Some(s) = g.as_mut() {
                    close_sticker_fow(s);
                }
            }
            Ok(json!({}))
        }
        "stickerClosed" => {
            if let Ok(mut g) = SESSION.lock() {
                if let Some(s) = g.as_mut() {
                    s.sticker_fow = false;
                }
            }
            Ok(json!({}))
        }
        "lifecycle" => Ok(json!({})),
        other => {
            if let Some(rest) = other.strip_prefix("face.") {
                face_toggle(rest, &args)
            } else if let Some(rest) = other.strip_prefix("sticker.add.") {
                sticker_add(rest, &args)
            } else if let Some(rest) = other.strip_prefix("obj.") {
                object_event(rest, &args)
            } else {
                Err(unsupported(other))
            }
        }
    }
}

fn root() -> Value {
    // 本体ツリーは使わない(UI はエディターの重ね + 下付き FOW)。
    rrl::column("root", vec![])
}

rasq_plugin! { scope: "sample.media-process:main", root: root, call: handle }

// ---------------------------------------------------------------------------
// native テスト(cargo test -p media-process-plugin)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use rasq_plugin_kit::{set_host_hook, take_host_log};

    /// SESSION はプロセス全体で 1 つなので、セッションを触るテストは直列化する。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn flat_png(w: u32, h: u32, px: [u8; 4]) -> String {
        B64.encode(encode_png(&RgbaImage::from_pixel(w, h, Rgba(px))).unwrap())
    }

    fn decode_b64_png(b64: &str) -> RgbaImage {
        assert!(!b64.starts_with("data:"), "bytesB64 must be raw base64, not a data: URI");
        image::load_from_memory_with_format(&B64.decode(b64).unwrap(), ImageFormat::Png).unwrap().to_rgba8()
    }

    fn last_call<'a>(log: &'a [(String, Value)], f: &str) -> Option<&'a Value> {
        log.iter().filter(|(n, _)| n == f).last().map(|(_, a)| a)
    }

    fn last_overlay(log: &[(String, Value)]) -> Value {
        last_call(log, "composer.media.overlay").expect("composer.media.overlay call")["root"].clone()
    }

    fn find_key<'a>(node: &'a Value, key: &str) -> Option<&'a Value> {
        if node["k"] == json!(key) {
            return Some(node);
        }
        node["c"].as_array()?.iter().find_map(|n| find_key(n, key))
    }

    fn all_keys(node: &Value, out: &mut Vec<String>) {
        if let Some(k) = node["k"].as_str() {
            out.push(k.to_string());
        }
        if let Some(c) = node["c"].as_array() {
            for n in c {
                all_keys(n, out);
            }
        }
    }

    fn rect_of(node: &Value) -> (f64, f64, f64, f64) {
        (node["p"]["left"].as_f64().unwrap(), node["p"]["top"].as_f64().unwrap(), node["p"]["width"].as_f64().unwrap(), node["p"]["height"].as_f64().unwrap())
    }

    /// 白い 300×200 の画像でセッションを開き、ログを空にする。
    fn open_session(token: &str) {
        set_host_hook(|f, _| if f == "fow.open" { json!({"opened": true}) } else { json!({}) });
        let _ = call_native("media.cancel", json!({}));
        take_host_log();
        let r = call_native(
            "media.edit",
            json!({"token": token, "index": 0, "name": "photo.jpg", "mimeType": "image/png", "width": 300, "height": 200, "bytesB64": flat_png(300, 200, [255, 255, 255, 255])}),
        )
        .unwrap();
        assert_eq!(r, catalog());
    }

    fn drag(fn_name: &str, phase: &str, dx: f64, dy: f64) {
        call_native(fn_name, json!({"kind": "drag", "phase": phase, "x": 5.0, "y": 5.0, "dx": dx, "dy": dy})).unwrap();
    }

    // ---- 単体: ops / sprites / faces ----------------------------------------

    #[test]
    fn rect_mapping_between_coordinate_systems() {
        // フル 2000×1000 → プレビュー 360×180 → 検知 480×240
        let full = RectF::new(1000.0, 500.0, 200.0, 100.0);
        let prev = full.map((2000, 1000), (360, 180));
        assert_eq!(prev, RectF::new(180.0, 90.0, 36.0, 18.0));
        let det = full.map((2000, 1000), (480, 240));
        assert_eq!(det, RectF::new(240.0, 120.0, 48.0, 24.0));
        // 逆方向で元に戻る(浮動小数の丸め分は許す)
        let back = det.map((480, 240), (2000, 1000));
        for (a, b) in [(back.x, full.x), (back.y, full.y), (back.w, full.w), (back.h, full.h)] {
            assert!((a - b).abs() < 1e-2, "round trip: {back:?} vs {full:?}");
        }
        // expanded は中心を保つ
        let e = full.expanded(0.5);
        assert_eq!((e.x + e.w / 2.0, e.y + e.h / 2.0), (1100.0, 550.0));
        assert_eq!((e.w, e.h), (300.0, 150.0));
        // clamp_to は画像に収める / 空は None
        assert_eq!(RectF::new(-10.0, -10.0, 30.0, 30.0).clamp_to(100, 100), Some((0, 0, 20, 20)));
        assert_eq!(RectF::new(95.0, 50.0, 30.0, 30.0).clamp_to(100, 100), Some((95, 50, 5, 30)));
        assert_eq!(RectF::new(200.0, 50.0, 30.0, 30.0).clamp_to(100, 100), None);
        assert_eq!(RectF::new(f32::NAN, 0.0, 1.0, 1.0).clamp_to(100, 100), None);
        // 縮小の倍率で map しても同じ(ops::downscale と整合)
        let img = RgbaImage::new(2000, 1000);
        assert_eq!(ops::downscale(&img, 360).dimensions(), (360, 180));
        assert_eq!(ops::downscale(&img, 480).dimensions(), (480, 240));
    }

    #[test]
    fn blur_rect_softens_edge_inside_rect_only() {
        // 左半分 黒 / 右半分 白 の 40×20
        let mut img = RgbaImage::from_fn(40, 20, |x, _| if x < 20 { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) });
        let before = img.clone();
        ops::blur_rect(&mut img, (10, 5, 20, 10), 4, 2);
        // 矩形の中、境界のすぐ両側は中間色になる
        let l = img.get_pixel(19, 10).0[0];
        let r = img.get_pixel(20, 10).0[0];
        assert!(l > 30 && l < 225, "left of edge should be gray: {l}");
        assert!(r > 30 && r < 225, "right of edge should be gray: {r}");
        assert!(l < r);
        // アルファは 255 のまま
        assert_eq!(img.get_pixel(19, 10).0[3], 255);
        // 矩形の外は無変化
        for (x, y) in [(5u32, 10u32), (35, 10), (19, 2), (20, 17), (0, 0), (39, 19)] {
            assert_eq!(img.get_pixel(x, y), before.get_pixel(x, y), "pixel outside rect changed at {x},{y}");
        }
        // 矩形の内側でも境界から遠い所は元の色に近い(端は clamp なので純色)
        assert_eq!(img.get_pixel(10, 10).0[0], 0);
        assert_eq!(img.get_pixel(29, 10).0[0], 255);
        // 半径は矩形の短辺に比例(最低 2)
        assert_eq!(ops::blur_radius_for(80.0, 120.0), 10);
        assert_eq!(ops::blur_radius_for(4.0, 4.0), 2);
        // はみ出した矩形 / 空矩形でも panic しない
        ops::blur_rect(&mut img, (35, 15, 100, 100), 3, 1);
        ops::blur_rect(&mut img, (100, 100, 10, 10), 3, 1);
        ops::blur_rect(&mut img, (0, 0, 0, 0), 3, 1);
    }

    #[test]
    fn sprites_are_rasterized_with_alpha() {
        for i in 0..sprites::SPRITE_COUNT {
            let s = sprites::sprite(i);
            assert_eq!(s.dimensions(), (sprites::SPRITE_SIZE, sprites::SPRITE_SIZE));
            let opaque = s.pixels().filter(|p| p.0[3] == 255).count();
            let clear = s.pixels().filter(|p| p.0[3] == 0).count();
            assert!(opaque > 1000, "sprite {i} should have a solid body: {opaque}");
            assert!(clear > 500, "sprite {i} should have transparent corners: {clear}");
            assert_eq!(s.get_pixel(0, 0).0[3], 0, "sprite {i} corner must be transparent");
            assert_eq!(s.get_pixel(48, 48).0[3], 255, "sprite {i} center must be opaque");
            assert!(!sprites::label(i).is_empty());
            assert!(sprite_data_uri(i).starts_with("data:image/png;base64,"));
        }
    }

    #[test]
    fn composite_sprite_rotation_and_scale() {
        let mut img = RgbaImage::from_pixel(200, 200, Rgba([0, 0, 255, 255]));
        let st = Sticker { sprite: 0, cx: 100.0, cy: 100.0, size: 60.0, angle: 0.7 };
        ops::composite_sprite(&mut img, sprites::sprite(0), &st);
        // 中心は塗られる、遠くは元のまま
        assert_ne!(img.get_pixel(100, 100).0, [0, 0, 255, 255]);
        assert_eq!(img.get_pixel(10, 10).0, [0, 0, 255, 255]);
        assert_eq!(img.get_pixel(100, 100).0[3], 255);
        // バウンディングボックスは回転で広がる(sqrt2 倍まで)
        let (hx, _) = st.half_extent();
        assert!(hx > 30.0 && hx <= 30.0 * std::f32::consts::SQRT_2 + 1e-3);
        // 画面外にはみ出しても panic しない
        let off = Sticker { sprite: 1, cx: -10.0, cy: 205.0, size: 80.0, angle: 3.0 };
        ops::composite_sprite(&mut img, sprites::sprite(1), &off);
        let scaled = st.scaled(2.0, 2.0);
        assert_eq!((scaled.cx, scaled.cy, scaled.size, scaled.angle), (200.0, 200.0, 120.0, 0.7));
    }

    #[test]
    fn detector_loads_embedded_model_and_finds_nothing_in_noise() {
        // 単色 / 縞 の合成画像に顔は無い。モデルのパース(埋め込み 1.2MB)が通ることの確認
        let gray = image::GrayImage::from_fn(240, 160, |x, y| image::Luma([((x / 8 + y / 8) % 2 * 200) as u8]));
        let found = faces::detect(&gray).expect("embedded model must parse");
        assert!(found.is_empty(), "synthetic stripes must not contain faces: {found:?}");
        // 小さすぎる画像は空
        assert!(faces::detect(&image::GrayImage::new(10, 10)).unwrap().is_empty());
        // フル画像経由(480 → 再検知も通る)でも空で、panic しない
        let big = RgbaImage::from_fn(600, 400, |x, y| Rgba([((x / 16 + y / 16) % 2 * 255) as u8, 128, 64, 255]));
        assert!(faces::detect_in_image(&big).unwrap().is_empty());
    }

    /// 実写で検知器が動くことの確認(リポジトリに写真は入れないので手動):
    /// `FACE_FIXTURE=/path/to/photo.png cargo test -p media-process-plugin -- --ignored real_photo`
    #[test]
    #[ignore]
    fn real_photo_from_env_has_faces() {
        let path = std::env::var("FACE_FIXTURE").expect("FACE_FIXTURE=<png/jpeg with a frontal face>");
        let full = ops::downscale(&image::open(&path).expect("open fixture").to_rgba8(), APPLY_MAX_SIDE);
        let found = faces::detect_in_image(&full).unwrap();
        eprintln!("{} face(s) in {path} ({}x{}): {found:?}", found.len(), full.width(), full.height());
        assert!(!found.is_empty(), "expected at least one face in {path}");
        for f in &found {
            assert!(f.clamp_to(full.width(), full.height()).is_some(), "face box lies in the full image: {f:?}");
        }
    }

    // ---- call: マニフェスト / 一覧 / 入力検証 ---------------------------------

    #[test]
    fn manifest_shape() {
        let m = call_native("plugin.manifest", json!({})).unwrap();
        assert_eq!(m["schema"], json!("rasq.plugin-manifest/1"));
        assert_eq!(m["id"], json!("sample.media-process"));
        assert_eq!(m["name"], json!("加工(顔ぼかし / スタンプ)(サンプル)"));
        assert_eq!(m["version"], json!("0.1.0"));
        assert_eq!(m["kind"], json!("action"));
        assert_eq!(m["slots"], json!(["composer.media"]));
        assert_eq!(m["permissions"], json!(["fow.widget"]));
    }

    #[test]
    fn catalog_is_static_and_needs_no_session() {
        let _g = serial();
        let _ = call_native("media.cancel", json!({}));
        take_host_log();
        let r = call_native("media.catalog", json!({})).unwrap();
        assert!(take_host_log().is_empty(), "media.catalog must not call host functions");
        let gs = r["genres"].as_array().unwrap();
        assert_eq!(gs.len(), 1);
        assert_eq!(gs[0]["id"], json!("process"));
        assert!(gs[0].get("label").is_none() && gs[0].get("icon").is_none(), "standard genre must not carry label / icon");
        let acts = gs[0]["actions"].as_array().unwrap();
        assert_eq!(acts.iter().map(|a| a["id"].as_str().unwrap()).collect::<Vec<_>>(), ["sticker", "reset"]);
        assert_eq!(acts[0]["label"], json!("スタンプ"));
        assert_eq!(acts[0]["icon"], json!("emoji_emotions"));
        assert_eq!(acts[1]["label"], json!("リセット"));
        assert_eq!(acts[1]["icon"], json!("restart_alt"));
        assert_eq!(call_native("media.commit", json!({"token": "t"})).unwrap_err()["code"], json!("D01"));
    }

    #[test]
    fn rejects_bad_input() {
        let _g = serial();
        let _ = call_native("media.cancel", json!({}));
        assert_eq!(call_native("media.edit", json!({"token": "t", "mimeType": "image/gif", "bytesB64": "AAAA"})).unwrap_err()["code"], json!("C01"));
        assert_eq!(call_native("media.edit", json!({"token": "t", "mimeType": "image/png", "bytesB64": "!!not base64!!"})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("media.edit", json!({"token": "t", "mimeType": "image/png", "bytesB64": "AAAAAAAA"})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("media.edit", json!({"token": "t"})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("nope", json!({})).unwrap_err()["code"], json!("C01"));
        assert_eq!(call_native("objective", json!({})).unwrap_err()["code"], json!("C01"));
        // セッション無しのイベントは D01(panic しない)
        assert_eq!(call_native("media.action", json!({"token": "t", "genre": "process", "action": "faces"})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("sticker.add.0", json!({})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("obj.0.move", json!({"phase": "update", "dx": 1, "dy": 1})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("face.0", json!({})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("media.commit", json!({"token": "t"})).unwrap_err()["code"], json!("D01"));
        let _ = call_native("closeSticker", json!({}));
        let _ = call_native("stickerClosed", json!({}));
        assert_eq!(call_native("media.cancel", json!({"token": "t"})).unwrap(), json!({}));

        // セッションがあっても壊れた index / 不明な操作 / 別 token は D01
        open_session("tok-bad");
        assert_eq!(call_native("sticker.add.9", json!({})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("sticker.add.x", json!({})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("obj.0.move", json!({"phase": "update", "dx": 1, "dy": 1})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("face.0", json!({})).unwrap_err()["code"], json!("D01"));
        call_native("sticker.add.0", json!({})).unwrap();
        assert_eq!(call_native("obj.0.explode", json!({})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("obj.a.move", json!({})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("media.action", json!({"token": "other", "genre": "process", "action": "reset"})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("media.action", json!({"token": "tok-bad", "genre": "tone", "action": "gamma"})).unwrap_err()["code"], json!("D01"));
        assert_eq!(call_native("media.commit", json!({"token": "other"})).unwrap_err()["code"], json!("D01"));
        // NaN / 文字列の dx でも落ちない
        call_native("obj.0.move", json!({"phase": "update", "dx": "x", "dy": null})).unwrap();
        let _ = call_native("media.cancel", json!({"token": "tok-bad"}));
        take_host_log();
    }

    // ---- call: プレビュー / スタンプ / 重ね -------------------------------------

    #[test]
    fn media_edit_pushes_preview_with_size() {
        let _g = serial();
        open_session("tok-edit");
        let log = take_host_log();
        let p = last_call(&log, "composer.media.preview").expect("preview");
        assert_eq!(p["token"], json!("tok-edit"));
        assert_eq!(p["mimeType"], json!("image/png"));
        assert_eq!(p["width"], json!(300));
        assert_eq!(p["height"], json!(200));
        let img = decode_b64_png(p["bytesB64"].as_str().unwrap());
        assert_eq!(img.dimensions(), (300, 200));
        assert_eq!(img.get_pixel(150, 100).0, [255, 255, 255, 255]);
        // 重ねはまだ出さない
        assert!(last_call(&log, "composer.media.overlay").is_none());

        // 大きい画像はプレビュー長辺 360 に縮む(width/height もその値)
        let _ = call_native("media.cancel", json!({"token": "tok-edit"}));
        take_host_log();
        call_native("media.edit", json!({"token": "tok-big", "name": "a.png", "mimeType": "image/png", "bytesB64": flat_png(1200, 600, [10, 20, 30, 255])})).unwrap();
        let log = take_host_log();
        let p = last_call(&log, "composer.media.preview").unwrap();
        assert_eq!(p["width"], json!(360));
        assert_eq!(p["height"], json!(180));
        assert_eq!(decode_b64_png(p["bytesB64"].as_str().unwrap()).dimensions(), (360, 180));
        let _ = call_native("media.cancel", json!({"token": "tok-big"}));
        take_host_log();
    }

    #[test]
    fn sticker_action_opens_fow_and_add_places_selected_object() {
        let _g = serial();
        open_session("tok-st");
        // sticker → FOW "sticker-pick" に 3 つの Image(data: URI、tap → sticker.add.<i>)
        call_native("media.action", json!({"token": "tok-st", "genre": "process", "action": "sticker"})).unwrap();
        let log = take_host_log();
        let (_, open) = log.iter().find(|(f, _)| f == "fow.open").expect("fow.open");
        assert_eq!(open["id"], json!(STICKER_FOW_ID));
        assert_eq!(open["onClosed"], json!("stickerClosed"));
        assert_eq!(open["spec"]["type"], json!("widget"));
        for i in 0..3 {
            let n = find_key(&open["root"], &format!("sticker.{i}")).expect("sticker image");
            assert_eq!(n["t"], json!("Image"));
            assert!(n["p"]["src"].as_str().unwrap().starts_with("data:image/png;base64,"));
            assert_eq!(n["on"]["tap"], json!(format!("sticker.add.{i}")));
            assert_eq!(n["p"]["width"], json!(64.0));
            assert_eq!(n["p"]["height"], json!(64.0));
        }
        assert_eq!(find_key(&open["root"], "close").unwrap()["on"]["tap"], json!("closeSticker"));
        // 2 回目は開き直さない
        call_native("media.action", json!({"token": "tok-st", "genre": "process", "action": "sticker"})).unwrap();
        assert!(take_host_log().iter().all(|(f, _)| f != "fow.open"));

        // 追加 → 中央、一辺 = 短辺/3、選択、重ねにオブジェクト枠 + ハンドル + ボタン
        call_native("sticker.add.1", json!({"kind": "tap"})).unwrap();
        let log = take_host_log();
        let ov = last_overlay(&log);
        assert_eq!(ov["t"], json!("Stack"));
        assert_eq!(ov["p"]["width"], json!(300.0));
        assert_eq!(ov["p"]["height"], json!(200.0));
        let obj = find_key(&ov, "obj.0").expect("obj.0 box");
        assert_eq!(obj["t"], json!("Box"));
        assert_eq!(obj["on"]["drag"], json!("obj.0.move"));
        assert_eq!(obj["on"]["tap"], json!("obj.0.move"));
        let (l, t, w, h) = rect_of(obj);
        assert!((w - 200.0 / 3.0).abs() < 1e-3 && (h - w).abs() < 1e-3, "size = short side / 3: {w}x{h}");
        assert!((l + w / 2.0 - 150.0).abs() < 1e-3 && (t + h / 2.0 - 100.0).abs() < 1e-3, "centered: {l},{t}");
        let scale = find_key(&ov, "obj.0.scale").expect("scale handle");
        assert_eq!(scale["on"]["drag"], json!("obj.0.scale"));
        let (sl, st, sw, sh) = rect_of(scale);
        assert!((sl + sw / 2.0 - (l + w)).abs() < 1e-3 && (st + sh / 2.0 - (t + h)).abs() < 1e-3, "scale handle at bottom-right corner");
        let rot = find_key(&ov, "obj.0.rotate").expect("rotate handle");
        assert_eq!(rot["on"]["drag"], json!("obj.0.rotate"));
        let (rl, rt, rw, _) = rect_of(rot);
        assert!((rl + rw / 2.0 - 150.0).abs() < 1e-3 && rt < t, "rotate handle centered above the object");
        let del = find_key(&ov, "obj.0.delete").expect("delete button");
        assert_eq!(del["on"]["tap"], json!("obj.0.delete"));
        assert_eq!(del["c"][0]["t"], json!("Icon"));
        assert_eq!(del["c"][0]["p"]["icon"], json!("delete"));
        let dup = find_key(&ov, "obj.0.dup").expect("dup button");
        assert_eq!(dup["on"]["tap"], json!("obj.0.dup"));
        assert_eq!(dup["c"][0]["p"]["icon"], json!("content_copy"));
        // プレビューにスタンプが描かれている(中央が白でなくなる)
        let p = last_call(&log, "composer.media.preview").unwrap();
        let img = decode_b64_png(p["bytesB64"].as_str().unwrap());
        assert_ne!(img.get_pixel(150, 100).0, [255, 255, 255, 255]);
        assert_eq!(img.get_pixel(5, 5).0, [255, 255, 255, 255]);

        let _ = call_native("media.cancel", json!({"token": "tok-st"}));
        take_host_log();
    }

    #[test]
    fn move_drag_changes_position() {
        let _g = serial();
        open_session("tok-mv");
        call_native("sticker.add.0", json!({})).unwrap();
        take_host_log();
        drag("obj.0.move", "start", 0.0, 0.0);
        drag("obj.0.move", "update", 30.0, -10.0);
        drag("obj.0.move", "update", 10.0, -10.0);
        drag("obj.0.move", "end", 0.0, 0.0);
        let log = take_host_log();
        let ov = last_overlay(&log);
        let (l, t, w, h) = rect_of(find_key(&ov, "obj.0").unwrap());
        assert!((l + w / 2.0 - 190.0).abs() < 1e-3, "cx moved by +40: {}", l + w / 2.0);
        assert!((t + h / 2.0 - 80.0).abs() < 1e-3, "cy moved by -20: {}", t + h / 2.0);
        // 動かすたびにプレビューも出し直す(4 回)
        assert_eq!(log.iter().filter(|(f, _)| f == "composer.media.preview").count(), 4);
        // 端を越えても中心は画面内で止まる
        drag("obj.0.move", "update", -1000.0, -1000.0);
        let ov = last_overlay(&take_host_log());
        let (l, t, w, h) = rect_of(find_key(&ov, "obj.0").unwrap());
        assert!((l + w / 2.0).abs() < 1e-3 && (t + h / 2.0).abs() < 1e-3);
        let _ = call_native("media.cancel", json!({"token": "tok-mv"}));
        take_host_log();
    }

    #[test]
    fn scale_drag_keeps_aspect_and_min_size() {
        let _g = serial();
        open_session("tok-sc");
        call_native("sticker.add.2", json!({})).unwrap();
        let (_, _, w0, _) = rect_of(find_key(&last_overlay(&take_host_log()), "obj.0").unwrap());
        // 右下のハンドルを外側へ(中心-ハンドルの距離が 1.5 倍になるように)
        let half = w0 / 2.0;
        drag("obj.0.scale", "start", 0.0, 0.0);
        drag("obj.0.scale", "update", half * 0.5, half * 0.5);
        drag("obj.0.scale", "end", 0.0, 0.0);
        let ov = last_overlay(&take_host_log());
        let (l, t, w, h) = rect_of(find_key(&ov, "obj.0").unwrap());
        assert!((w - w0 * 1.5).abs() < 0.5, "size ×1.5: {w0} → {w}");
        assert!((w - h).abs() < 1e-3, "aspect stays 1:1");
        assert!((l + w / 2.0 - 150.0).abs() < 1e-3 && (t + h / 2.0 - 100.0).abs() < 1e-3, "center unchanged");
        // ハンドルも新しい角に居る
        let (sl, st, sw, sh) = rect_of(find_key(&ov, "obj.0.scale").unwrap());
        assert!((sl + sw / 2.0 - (l + w)).abs() < 1e-3 && (st + sh / 2.0 - (t + h)).abs() < 1e-3);
        // 内側へ大きく → 最小 16px で止まる
        drag("obj.0.scale", "start", 0.0, 0.0);
        drag("obj.0.scale", "update", -500.0, -500.0);
        drag("obj.0.scale", "end", 0.0, 0.0);
        let (_, _, w, h) = rect_of(find_key(&last_overlay(&take_host_log()), "obj.0").unwrap());
        assert!((w - MIN_OBJECT_SIZE as f64).abs() < 1e-3 && (h - w).abs() < 1e-3, "min size: {w}");
        let _ = call_native("media.cancel", json!({"token": "tok-sc"}));
        take_host_log();
    }

    #[test]
    fn rotate_drag_changes_angle() {
        let _g = serial();
        open_session("tok-rot");
        call_native("sticker.add.0", json!({})).unwrap();
        let ov = last_overlay(&take_host_log());
        let (_, _, w0, h0) = rect_of(find_key(&ov, "obj.0").unwrap());
        let (rl, rt, _, rh) = rect_of(find_key(&ov, "obj.0.rotate").unwrap());
        // 回転ハンドル(真上)を右へ、中心の高さまで下ろす → 90° 回転
        let handle_cy = rt + rh / 2.0;
        let dist = 100.0 - handle_cy;
        drag("obj.0.rotate", "start", 0.0, 0.0);
        drag("obj.0.rotate", "update", dist, dist);
        drag("obj.0.rotate", "end", 0.0, 0.0);
        let ov = last_overlay(&take_host_log());
        let (l, t, w, h) = rect_of(find_key(&ov, "obj.0").unwrap());
        // 90° なら正方形のバウンディングボックスは元と同じ大きさで、中心も同じ
        assert!((w - w0).abs() < 0.5 && (h - h0).abs() < 0.5, "90° keeps the aabb: {w0}x{h0} → {w}x{h}");
        assert!((l + w / 2.0 - 150.0).abs() < 1e-3 && (t + h / 2.0 - 100.0).abs() < 1e-3);
        // ハンドルは右側(中心の高さ)へ移った
        let (nl, nt, nw, nh) = rect_of(find_key(&ov, "obj.0.rotate").unwrap());
        assert!(nl + nw / 2.0 > 150.0 + w0 / 2.0, "rotate handle moved to the right: {}", nl + nw / 2.0);
        assert!((nt + nh / 2.0 - 100.0).abs() < 0.5, "rotate handle at center height: {}", nt + nh / 2.0);
        assert!((rl - nl).abs() > 10.0);
        // 45° にすると aabb は √2 倍に広がる
        drag("obj.0.rotate", "start", 0.0, 0.0);
        drag("obj.0.rotate", "update", 0.0, dist);
        drag("obj.0.rotate", "end", 0.0, 0.0);
        let (_, _, w, h) = rect_of(find_key(&last_overlay(&take_host_log()), "obj.0").unwrap());
        assert!((w - w0 * std::f64::consts::SQRT_2).abs() < 0.5 && (w - h).abs() < 1e-3, "45°: {w}");
        let _ = call_native("media.cancel", json!({"token": "tok-rot"}));
        take_host_log();
    }

    #[test]
    fn dup_creates_second_object_offset_and_delete_removes() {
        let _g = serial();
        open_session("tok-dup");
        call_native("sticker.add.1", json!({})).unwrap();
        take_host_log();
        call_native("obj.0.dup", json!({"kind": "tap"})).unwrap();
        let ov = last_overlay(&take_host_log());
        let o0 = rect_of(find_key(&ov, "obj.0").expect("obj.0"));
        let o1 = rect_of(find_key(&ov, "obj.1").expect("obj.1"));
        assert!((o1.0 - o0.0 - DUP_OFFSET as f64).abs() < 1e-3 && (o1.1 - o0.1 - DUP_OFFSET as f64).abs() < 1e-3, "dup offset +16: {o0:?} {o1:?}");
        assert_eq!((o1.2, o1.3), (o0.2, o0.3));
        assert_eq!(find_key(&ov, "obj.1").unwrap()["on"]["drag"], json!("obj.1.move"));
        // 複製が選択される(ハンドルは obj.1 に付き、obj.0 には無い)
        let mut ks = Vec::new();
        all_keys(&ov, &mut ks);
        assert!(ks.contains(&"obj.1.scale".to_string()) && ks.contains(&"obj.1.rotate".to_string()) && ks.contains(&"obj.1.delete".to_string()) && ks.contains(&"obj.1.dup".to_string()));
        assert!(!ks.contains(&"obj.0.scale".to_string()));
        // タップで obj.0 を選択し直す
        call_native("obj.0.move", json!({"kind": "tap"})).unwrap();
        let ov = last_overlay(&take_host_log());
        let mut ks = Vec::new();
        all_keys(&ov, &mut ks);
        assert!(ks.contains(&"obj.0.scale".to_string()) && !ks.contains(&"obj.1.scale".to_string()));
        assert_eq!(find_key(&ov, "obj.0").unwrap()["p"]["borderColor"], json!("#FF9500"));
        // 削除 → obj.1 だった物が obj.0 になる(位置で判る)
        call_native("obj.0.delete", json!({"kind": "tap"})).unwrap();
        let ov = last_overlay(&take_host_log());
        assert!(find_key(&ov, "obj.1").is_none());
        let rest = rect_of(find_key(&ov, "obj.0").unwrap());
        assert!((rest.0 - o1.0).abs() < 1e-3 && (rest.1 - o1.1).abs() < 1e-3);
        // 最後を消すと重ねは null で消える
        call_native("obj.0.delete", json!({"kind": "tap"})).unwrap();
        let log = take_host_log();
        assert!(last_overlay(&log).is_null());
        assert_eq!(call_native("obj.0.delete", json!({})).unwrap_err()["code"], json!("D01"));
        let _ = call_native("media.cancel", json!({"token": "tok-dup"}));
        take_host_log();
    }

    #[test]
    fn commit_composites_sticker_on_full_image() {
        let _g = serial();
        set_host_hook(|f, _| if f == "fow.open" { json!({"opened": true}) } else { json!({}) });
        let _ = call_native("media.cancel", json!({}));
        take_host_log();
        // フル 900×600(プレビューは 360×240)
        call_native("media.edit", json!({"token": "tok-c", "name": "in.jpg", "mimeType": "image/png", "bytesB64": flat_png(900, 600, [255, 255, 255, 255])})).unwrap();
        call_native("media.action", json!({"token": "tok-c", "genre": "process", "action": "sticker"})).unwrap();
        call_native("sticker.add.0", json!({})).unwrap();
        // 左上寄りへ移動(プレビュー px で -100, -60 → フルでは -250, -150)
        drag("obj.0.move", "update", -100.0, -60.0);
        take_host_log();
        let r = call_native("media.commit", json!({"token": "tok-c"})).unwrap();
        assert_eq!(r["mimeType"], json!("image/png"));
        assert_eq!(r["name"], json!("in.png"));
        let out = decode_b64_png(r["bytesB64"].as_str().unwrap());
        assert_eq!(out.dimensions(), (900, 600));
        // スタンプの中心 = (450-250, 300-150) = (200, 150)、一辺 = 80 × 2.5 = 200
        assert_ne!(out.get_pixel(200, 150).0, [255, 255, 255, 255], "sticker center must be painted");
        assert_ne!(out.get_pixel(160, 150).0, [255, 255, 255, 255]);
        assert_eq!(out.get_pixel(450, 300).0, [255, 255, 255, 255], "original center must stay white");
        assert_eq!(out.get_pixel(880, 580).0, [255, 255, 255, 255]);
        assert_eq!(out.get_pixel(200, 40).0, [255, 255, 255, 255], "outside the sticker's 200px box");
        // 重ねを消し、FOW を閉じ、状態を捨てる
        let log = take_host_log();
        assert!(last_overlay(&log).is_null());
        assert!(log.iter().any(|(f, a)| f == "fow.close" && a["id"] == json!(STICKER_FOW_ID)));
        assert_eq!(call_native("media.commit", json!({"token": "tok-c"})).unwrap_err()["code"], json!("D01"));
        take_host_log();
    }

    #[test]
    fn faces_on_synthetic_image_toasts_without_overlay() {
        let _g = serial();
        open_session("tok-f");
        take_host_log();
        let r = call_native("media.action", json!({"token": "tok-f", "genre": "process", "action": "faces"})).unwrap();
        assert_eq!(r, json!({}));
        let log = take_host_log();
        let toast = last_call(&log, "toast").expect("toast when no face");
        assert_eq!(toast["message"], json!("顔が見つかりませんでした"));
        assert!(last_call(&log, "composer.media.overlay").is_none(), "no overlay without faces");
        // 顔が無いのでプレビューも出し直さない
        assert!(last_call(&log, "composer.media.preview").is_none());
        // 顔 0 は無いのでトグルは D01
        assert_eq!(call_native("face.0", json!({})).unwrap_err()["code"], json!("D01"));
        let _ = call_native("media.cancel", json!({"token": "tok-f"}));
        take_host_log();
    }

    #[test]
    fn reset_and_cancel_clear_everything() {
        let _g = serial();
        open_session("tok-r");
        call_native("media.action", json!({"token": "tok-r", "genre": "process", "action": "sticker"})).unwrap();
        call_native("sticker.add.0", json!({})).unwrap();
        call_native("obj.0.dup", json!({})).unwrap();
        take_host_log();
        call_native("media.action", json!({"token": "tok-r", "genre": "process", "action": "reset"})).unwrap();
        let log = take_host_log();
        assert!(last_overlay(&log).is_null(), "reset clears the overlay");
        let p = last_call(&log, "composer.media.preview").unwrap();
        let img = decode_b64_png(p["bytesB64"].as_str().unwrap());
        assert_eq!(img.get_pixel(150, 100).0, [255, 255, 255, 255], "preview restored");
        assert_eq!(call_native("obj.0.move", json!({"kind": "tap"})).unwrap_err()["code"], json!("D01"));
        // FOW は開いたまま(続けて貼れる)→ cancel で閉じる
        assert!(!log.iter().any(|(f, _)| f == "fow.close"));
        call_native("sticker.add.2", json!({})).unwrap();
        take_host_log();
        assert_eq!(call_native("media.cancel", json!({"token": "tok-r"})).unwrap(), json!({}));
        let log = take_host_log();
        assert!(last_overlay(&log).is_null());
        assert!(log.iter().any(|(f, a)| f == "fow.close" && a["id"] == json!(STICKER_FOW_ID)));
        assert_eq!(call_native("sticker.add.0", json!({})).unwrap_err()["code"], json!("D01"));
        // ホスト側で FOW が閉じられた後は fow.close を送らない
        open_session("tok-r2");
        call_native("media.action", json!({"token": "tok-r2", "genre": "process", "action": "sticker"})).unwrap();
        call_native("stickerClosed", json!({})).unwrap();
        take_host_log();
        call_native("media.cancel", json!({"token": "tok-r2"})).unwrap();
        assert!(!take_host_log().iter().any(|(f, _)| f == "fow.close"));
    }
}
