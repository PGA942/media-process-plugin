//! 画像処理の純粋関数(ホスト / wasm ABI に依存しない)。
//!
//! - 矩形の座標系変換([`RectF`]: フル画像 ⇄ プレビュー ⇄ 検知用縮小画像)
//! - 矩形内のボックスぼかし([`blur_rect`]、分離型 2 パス、半径は矩形の大きさに比例 [`blur_radius_for`])
//! - スタンプ(オブジェクト)の合成([`composite_sprite`]: 回転 + 拡縮 + バイリニア補間 + アルファ合成)
//!
//! すべて `RgbaImage`(8bit RGBA)の上で動き、panic しない(範囲外は clamp / skip)。

use image::{imageops, GrayImage, RgbaImage};

/// 浮動小数の矩形(左上 + 幅高)。単位は文脈による(フル画像 px / プレビュー px / 検知画像 px)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// `from` の画素系から `to` の画素系へ(x / y を別々の比率で)写す。
    pub fn map(self, from: (u32, u32), to: (u32, u32)) -> RectF {
        let sx = if from.0 == 0 { 1.0 } else { to.0 as f32 / from.0 as f32 };
        let sy = if from.1 == 0 { 1.0 } else { to.1 as f32 / from.1 as f32 };
        RectF::new(self.x * sx, self.y * sy, self.w * sx, self.h * sy)
    }

    /// 中心を保って `frac`(0.2 = 20%)だけ広げる。
    pub fn expanded(self, frac: f32) -> RectF {
        let dw = self.w * frac;
        let dh = self.h * frac;
        RectF::new(self.x - dw / 2.0, self.y - dh / 2.0, self.w + dw, self.h + dh)
    }

    /// `w × h` の画像に収まる整数矩形 `(x, y, w, h)`。はみ出しは切り詰め、空なら `None`。
    pub fn clamp_to(self, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
        if !(self.x.is_finite() && self.y.is_finite() && self.w.is_finite() && self.h.is_finite()) {
            return None;
        }
        let x0 = self.x.floor().max(0.0) as i64;
        let y0 = self.y.floor().max(0.0) as i64;
        let x1 = ((self.x + self.w).ceil() as i64).min(width as i64);
        let y1 = ((self.y + self.h).ceil() as i64).min(height as i64);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some((x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// 長辺が `max_side` を超えていれば縮小(box filter、安価)。それ以外は clone。
pub fn downscale(img: &RgbaImage, max_side: u32) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    let longest = w.max(h);
    if longest <= max_side || longest == 0 {
        return img.clone();
    }
    let scale = max_side as f64 / longest as f64;
    let nw = ((w as f64 * scale).round() as u32).max(1);
    let nh = ((h as f64 * scale).round() as u32).max(1);
    imageops::thumbnail(img, nw, nh)
}

/// 輝度(BT.601)のグレースケール。検知器の入力。
pub fn to_gray(img: &RgbaImage) -> GrayImage {
    imageops::grayscale(img)
}

/// ぼかし半径 = 矩形の短辺の 1/8(最低 2px)。矩形の大きさに比例するので、プレビューでもフル画像でも見え方が揃う。
pub fn blur_radius_for(w: f32, h: f32) -> u32 {
    ((w.min(h) / 8.0).round() as i64).max(2) as u32
}

/// 1 ラインのボックスぼかし(端は clamp = 窓を切り詰めて平均)。`src` → `dst`(長さ同じ、RGBA 4 要素ずつ)。
fn box_line(src: &[[u32; 4]], dst: &mut [[u32; 4]], radius: usize) {
    let n = src.len();
    if n == 0 {
        return;
    }
    // 累積和(先頭に 0)
    let mut prefix = vec![[0u32; 4]; n + 1];
    for i in 0..n {
        for c in 0..4 {
            prefix[i + 1][c] = prefix[i][c] + src[i][c];
        }
    }
    for i in 0..n {
        let a = i.saturating_sub(radius);
        let b = (i + radius).min(n - 1);
        let count = (b - a + 1) as u32;
        for c in 0..4 {
            let sum = prefix[b + 1][c] - prefix[a][c];
            dst[i][c] = (sum + count / 2) / count;
        }
    }
}

/// 画像の矩形 `(x, y, w, h)` の中だけをボックスぼかしする(分離型、水平 → 垂直を `passes` 回)。
/// 矩形の外は読まない / 書かない(端は矩形の内側で clamp)。矩形がはみ出していれば切り詰める。
pub fn blur_rect(img: &mut RgbaImage, rect: (u32, u32, u32, u32), radius: u32, passes: u32) {
    let (x0, y0, w, h) = rect;
    if w == 0 || h == 0 || radius == 0 || passes == 0 || x0 >= img.width() || y0 >= img.height() {
        return;
    }
    let w = w.min(img.width() - x0) as usize;
    let h = h.min(img.height() - y0) as usize;
    let r = radius as usize;

    let mut buf: Vec<[u32; 4]> = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let p = img.get_pixel(x0 + x as u32, y0 + y as u32).0;
            buf.push([p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32]);
        }
    }
    let mut tmp = vec![[0u32; 4]; w * h];
    let mut col_src = vec![[0u32; 4]; h];
    let mut col_dst = vec![[0u32; 4]; h];
    for _ in 0..passes {
        // 水平
        for y in 0..h {
            box_line(&buf[y * w..(y + 1) * w], &mut tmp[y * w..(y + 1) * w], r);
        }
        // 垂直
        for x in 0..w {
            for y in 0..h {
                col_src[y] = tmp[y * w + x];
            }
            box_line(&col_src, &mut col_dst, r);
            for y in 0..h {
                buf[y * w + x] = col_dst[y];
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let v = buf[y * w + x];
            img.put_pixel(x0 + x as u32, y0 + y as u32, image::Rgba([v[0] as u8, v[1] as u8, v[2] as u8, v[3] as u8]));
        }
    }
}

/// 貼り付けたスタンプ(オブジェクト)。座標はプレビュー px(適用時に [`Sticker::scaled`] でフル画像へ)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sticker {
    /// スプライト番号([`crate::sprites`])
    pub sprite: usize,
    /// 中心
    pub cx: f32,
    pub cy: f32,
    /// 一辺の長さ(スプライトは正方形。縦横比は常に 1:1)
    pub size: f32,
    /// 回転(ラジアン、時計回り)
    pub angle: f32,
}

impl Sticker {
    /// 座標 / サイズを `sx` / `sy` 倍(サイズは `sx`)。
    pub fn scaled(&self, sx: f32, sy: f32) -> Sticker {
        Sticker { sprite: self.sprite, cx: self.cx * sx, cy: self.cy * sy, size: self.size * sx, angle: self.angle }
    }

    /// 回転後の軸平行バウンディングボックスの半幅・半高。
    pub fn half_extent(&self) -> (f32, f32) {
        let hs = self.size / 2.0;
        let e = hs * (self.angle.cos().abs() + self.angle.sin().abs());
        (e, e)
    }

    /// 回転後の軸平行バウンディングボックス。
    pub fn bbox(&self) -> RectF {
        let (hx, hy) = self.half_extent();
        RectF::new(self.cx - hx, self.cy - hy, hx * 2.0, hy * 2.0)
    }

    /// 中心からの相対ベクトル `(dx, dy)`(未回転)を回転して絶対座標にする。
    pub fn local_to_world(&self, dx: f32, dy: f32) -> (f32, f32) {
        let (s, c) = self.angle.sin_cos();
        (self.cx + dx * c - dy * s, self.cy + dx * s + dy * c)
    }
}

/// 事前乗算 RGBA(0–255 の f32)をバイリニアで標本化する。範囲外は透明。
fn sample_premul(sprite: &RgbaImage, sx: f32, sy: f32) -> [f32; 4] {
    let (w, h) = (sprite.width() as i64, sprite.height() as i64);
    let x0 = sx.floor();
    let y0 = sy.floor();
    let fx = sx - x0;
    let fy = sy - y0;
    let (x0, y0) = (x0 as i64, y0 as i64);
    let mut acc = [0f32; 4];
    for (ox, oy, wgt) in [(0, 0, (1.0 - fx) * (1.0 - fy)), (1, 0, fx * (1.0 - fy)), (0, 1, (1.0 - fx) * fy), (1, 1, fx * fy)] {
        let (x, y) = (x0 + ox, y0 + oy);
        if x < 0 || y < 0 || x >= w || y >= h || wgt <= 0.0 {
            continue;
        }
        let p = sprite.get_pixel(x as u32, y as u32).0;
        let a = p[3] as f32 / 255.0;
        acc[0] += p[0] as f32 * a * wgt;
        acc[1] += p[1] as f32 * a * wgt;
        acc[2] += p[2] as f32 * a * wgt;
        acc[3] += p[3] as f32 * wgt;
    }
    acc
}

/// スプライトを `st`(中心 / 一辺 / 回転)で `dst` にアルファ合成する(逆写像 + バイリニア)。
pub fn composite_sprite(dst: &mut RgbaImage, sprite: &RgbaImage, st: &Sticker) {
    if sprite.width() == 0 || sprite.height() == 0 || !(st.size > 0.0) || !st.cx.is_finite() || !st.cy.is_finite() {
        return;
    }
    let Some((x0, y0, w, h)) = st.bbox().expanded(0.05).clamp_to(dst.width(), dst.height()) else { return };
    let scale = sprite.width() as f32 / st.size; // 出力 px → スプライト px
    let (sin, cos) = st.angle.sin_cos();
    let (sw, sh) = (sprite.width() as f32, sprite.height() as f32);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let px = x as f32 + 0.5 - st.cx;
            let py = y as f32 + 0.5 - st.cy;
            // 逆回転
            let ux = px * cos + py * sin;
            let uy = -px * sin + py * cos;
            let sx = ux * scale + sw / 2.0 - 0.5;
            let sy = uy * scale + sh / 2.0 - 0.5;
            if sx < -1.0 || sy < -1.0 || sx > sw || sy > sh {
                continue;
            }
            let s = sample_premul(sprite, sx, sy);
            let a = s[3] / 255.0;
            if a <= 0.0 {
                continue;
            }
            let d = dst.get_pixel_mut(x, y);
            let da = d.0[3] as f32 / 255.0;
            let out_a = a + da * (1.0 - a);
            let mut out = [0u8; 4];
            for c in 0..3 {
                let dc = d.0[c] as f32 * da;
                let v = s[c] + dc * (1.0 - a);
                out[c] = if out_a > 0.0 { (v / out_a).round().clamp(0.0, 255.0) as u8 } else { 0 };
            }
            out[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
            d.0 = out;
        }
    }
}
