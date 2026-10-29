//! Getting pixels to the terminal: through a temporary file when the
//! terminal shares our filesystem (measured at start-up, never assumed), or
//! inline as compressed PNG otherwise (over SSH, for instance).

use crate::term::kitty_gfx::{self, EncodeStats, Format, Medium, Quiet, Transmit};
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug)]
pub struct Uploader {
    pub temp_file: bool,
    /// Tag raster images as Display P3 (`G=1`).
    pub gamut_p3: bool,
}

/// PNG with the encoder's default compression: about half the size of
/// its fastest setting, for a few tens of milliseconds per sheet.
pub fn png(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let enc = PngEncoder::new_with_quality(&mut out, CompressionType::Default, FilterType::Adaptive);
    enc.write_image(rgba, w, h, image::ExtendedColorType::Rgba8)
        .expect("encoding a PNG into memory");
    out
}

fn srgb_to_linear(c: u8) -> f32 {
    let x = c as f32 / 255.0;
    if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(x: f32) -> u8 {
    let x = x.clamp(0.0, 1.0);
    let v = if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
    (v * 255.0).round() as u8
}

/// Re-expresses sRGB art in Display P3 (same colours), then lets the
/// already-vivid ones (flowers, a fox's coat) reach a little further into
/// the wider gamut. Greys and pale colours are untouched, so the hen stays
/// white.
pub fn to_vivid_p3(rgba: &[u8]) -> Vec<u8> {
    let lut: Vec<f32> = (0..=255u8).map(srgb_to_linear).collect();
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        let (r, g, b) = (lut[px[0] as usize], lut[px[1] as usize], lut[px[2] as usize]);
        let pr = 0.822_462 * r + 0.177_538 * g;
        let pg = 0.033_194 * r + 0.966_806 * g;
        let pb = 0.017_083 * r + 0.072_397 * g + 0.910_520 * b;
        let max = pr.max(pg).max(pb);
        let min = pr.min(pg).min(pb);
        let sat = if max > 1e-4 { (max - min) / max } else { 0.0 };
        let k = 1.0 + 0.28 * sat * sat;
        let l = 0.2290 * pr + 0.6917 * pg + 0.0793 * pb;
        out.extend_from_slice(&[
            linear_to_srgb(l + (pr - l) * k),
            linear_to_srgb(l + (pg - l) * k),
            linear_to_srgb(l + (pb - l) * k),
            px[3],
        ]);
    }
    out
}

/// IEEE 754 half precision, rounding to nearest even.
pub fn f32_to_f16(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x007f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if mant != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = mant | 0x0080_0000;
        let shift = (14 - e) as u32;
        let half = m >> shift;
        let rem = m & ((1 << shift) - 1);
        let mid = 1 << (shift - 1);
        let round = rem > mid || (rem == mid && half & 1 == 1);
        return sign | (half + u32::from(round)) as u16;
    }
    let m = mant >> 13;
    let rem = mant & 0x1fff;
    let mut h = ((e as u32) << 10) | m;
    if rem > 0x1000 || (rem == 0x1000 && m & 1 == 1) {
        h += 1;
    }
    sign | h as u16
}

/// Opaque pixels whose every linear component is at least this are
/// white-hot and glow in HDR.
const GLINT_FROM: f32 = 0.92;

/// The white-hot parts of sRGB art as an HDR overlay (linear extended sRGB,
/// f16): white at `boost` times SDR white where the art is opaque and
/// white, fading out over near-white pixels, transparent everywhere else.
/// Placed over the ordinary sprite, it makes only those parts brighter.
///
/// A terminal without headroom clips the overlay to SDR white, and white
/// over the opaque near-white pixels it covers changes them by at most
/// `1 - GLINT_FROM` (nothing where they are white). So it can be shown
/// before the terminal has any headroom to offer; on macOS a terminal that
/// turns its extended range on only while something bright is on screen
/// needs that. Translucent pixels are left out: over a sprite pixel that
/// already lets the garden through, white would show even then.
pub fn hdr_glints(rgba: &[u8], boost: f32) -> Vec<u8> {
    let lut: Vec<f32> = (0..=255u8).map(srgb_to_linear).collect();
    let mut out = Vec::with_capacity(rgba.len() * 2);
    for px in rgba.chunks_exact(4) {
        let whitest = lut[px[0].min(px[1]).min(px[2]) as usize];
        let t = ((whitest - GLINT_FROM) / (1.0 - GLINT_FROM)).clamp(0.0, 1.0);
        let alpha = if px[3] == 255 { t * t * (3.0 - 2.0 * t) } else { 0.0 };
        let v = if alpha > 0.0 { boost } else { 0.0 };
        for c in [v, v, v, alpha] {
            out.extend_from_slice(&f32_to_f16(c).to_le_bytes());
        }
    }
    out
}

impl Uploader {
    /// Transmits straight-alpha RGBA pixels as image `id`.
    pub fn rgba(&self, out: &mut Vec<u8>, id: u32, w: u32, h: u32, rgba: &[u8]) -> EncodeStats {
        let converted;
        let rgba = if self.gamut_p3 {
            converted = to_vivid_p3(rgba);
            &converted[..]
        } else {
            rgba
        };
        if self.temp_file {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = kitty_gfx::temp_path(&format!("{}-{id}-{n}", std::process::id()));
            let t = Transmit {
                id,
                format: Format::Rgba,
                width: w,
                height: h,
                data: rgba,
                compress: false,
                medium: Medium::TempFile(path),
                quiet: Quiet::ErrorsOnly,
                gamut: u8::from(self.gamut_p3),
            };
            if let Ok(s) = t.write(out) {
                return s;
            }
        }
        if self.gamut_p3 {
            // A PNG carries no gamut tag through the protocol, so wide-gamut
            // pixels go as compressed RGBA.
            return Transmit {
                id,
                format: Format::Rgba,
                width: w,
                height: h,
                data: rgba,
                compress: true,
                medium: Medium::Direct,
                quiet: Quiet::ErrorsOnly,
                gamut: 1,
            }
            .write(out)
            .unwrap_or_default();
        }
        let png = png(rgba, w, h);
        Transmit {
            id,
            format: Format::Png,
            width: w,
            height: h,
            data: &png,
            compress: false,
            medium: Medium::Direct,
            quiet: Quiet::ErrorsOnly,
            gamut: 0,
        }
        .write(out)
        .unwrap_or_default()
    }

    /// Transmits a premultiplied tiny-skia pixmap.
    pub fn pixmap(&self, out: &mut Vec<u8>, id: u32, pm: &tiny_skia::Pixmap) -> EncodeStats {
        let rgba = pm.clone().take_demultiplied();
        self.rgba(out, id, pm.width(), pm.height(), &rgba)
    }

    /// Transmits HDR pixels: linear extended sRGB, f16 per channel. Values
    /// above the terminal's headroom are clipped (the default, `T=0`), never
    /// compressed, so an image whose only values above one are white looks
    /// like its SDR original where there is no headroom.
    pub fn hdr(&self, out: &mut Vec<u8>, id: u32, w: u32, h: u32, f16le: &[u8]) -> EncodeStats {
        if self.temp_file {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = kitty_gfx::temp_path(&format!("{}-{id}-{n}", std::process::id()));
            let t = Transmit {
                id,
                format: Format::HdrF16,
                width: w,
                height: h,
                data: f16le,
                compress: false,
                medium: Medium::TempFile(path),
                quiet: Quiet::ErrorsOnly,
                gamut: 0,
            };
            if let Ok(s) = t.write(out) {
                return s;
            }
        }
        Transmit {
            id,
            format: Format::HdrF16,
            width: w,
            height: h,
            data: f16le,
            compress: true,
            medium: Medium::Direct,
            quiet: Quiet::ErrorsOnly,
            gamut: 0,
        }
        .write(out)
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn half_floats() {
        use super::f32_to_f16;
        assert_eq!(f32_to_f16(1.0), 0x3c00);
        assert_eq!(f32_to_f16(2.0), 0x4000);
        assert_eq!(f32_to_f16(0.5), 0x3800);
        assert_eq!(f32_to_f16(0.0), 0);
        assert_eq!(f32_to_f16(-1.0), 0xbc00);
        assert_eq!(f32_to_f16(65504.0), 0x7bff);
        assert_eq!(f32_to_f16(1e9), 0x7c00);
        assert_eq!(f32_to_f16(1.0 / 16384.0), 0x0400, "smallest normal");
        assert_eq!(f32_to_f16(1.0 / 16777216.0), 0x0001, "smallest subnormal");
    }

    fn f16_to_f32(h: u16) -> f32 {
        let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
        let exp = ((h >> 10) & 0x1f) as i32;
        let mant = (h & 0x3ff) as f32;
        sign * match exp {
            0 => mant * 2f32.powi(-24),
            _ => (1.0 + mant / 1024.0) * 2f32.powi(exp - 15),
        }
    }

    fn decode(f16le: &[u8]) -> Vec<f32> {
        f16le.chunks_exact(2).map(|b| f16_to_f32(u16::from_le_bytes([b[0], b[1]]))).collect()
    }

    /// Art with white, near-white, pale, saturated, dark and translucent
    /// pixels.
    const ART: [u8; 28] = [
        255, 255, 255, 255, // white
        253, 251, 250, 255, // nearly white
        255, 247, 194, 255, // the pale yellow of a sparkle
        255, 210, 63, 255, // the yellow of a star
        255, 79, 134, 255, // a heart's pink
        60, 60, 60, 255, // dark grey
        255, 255, 255, 128, // white, half transparent (an outer edge)
    ];

    #[test]
    fn only_opaque_white_glows() {
        let out = decode(&super::hdr_glints(&ART, 3.0));
        assert_eq!(&out[0..4], &[3.0, 3.0, 3.0, 1.0], "white at three times SDR white");
        assert!(out[7] > 0.0 && out[7] < 1.0, "near-white fades: {:?}", &out[4..8]);
        for (i, px) in out.chunks_exact(4).enumerate().skip(2) {
            assert_eq!(px[3], 0.0, "pixel {i} does not glow");
        }
    }

    #[test]
    fn without_headroom_the_glints_change_nothing() {
        // A terminal without headroom clips the overlay at SDR white before
        // blending it over the sprite; what results is the sprite.
        let hdr = decode(&super::hdr_glints(&ART, 3.0));
        for (i, (s, h)) in ART.chunks_exact(4).zip(hdr.chunks_exact(4)).enumerate() {
            for ch in 0..3 {
                let under = super::srgb_to_linear(s[ch]);
                let seen = h[3] * h[ch].min(1.0) + (1.0 - h[3]) * under;
                let tol = if i == 0 { 0.0 } else { 1.0 - super::GLINT_FROM };
                assert!((seen - under).abs() <= tol, "pixel {i} channel {ch}: {seen} vs {under}");
            }
        }
    }

    #[test]
    fn grey_survives_and_red_grows() {
        let px = [200u8, 200, 200, 255, 220, 40, 30, 255];
        let out = super::to_vivid_p3(&px);
        assert!((out[0] as i32 - 200).abs() <= 1 && (out[1] as i32 - 200).abs() <= 1);
        // A saturated red, re-expressed in P3, has a lower red component
        // than in sRGB (P3 red is redder), but its chroma is boosted.
        assert!(out[4] < 220 && out[5] < 90);
        assert_eq!(out[7], 255);
    }
}
