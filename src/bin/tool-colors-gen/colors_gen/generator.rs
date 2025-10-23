#![forbid(unsafe_code)]

use super::warp::{HueCdf, WarpParams, weight};
use palette::{Clamp, FromColor, Hsv, Oklab, Oklch, Srgb};

const V_FIXED: f32 = 1.0; // LEDs: you modulate brightness later
const L_OK: f32 = 0.74; // perceptual lightness for hue sampling
const C_OK: f32 = 0.12; // chroma, modest to stay in-gamut

// Public type used across modules
#[derive(Clone, Copy, Debug)]
pub struct Swatch {
    pub hsv: Hsv,
    pub rgb: Srgb,
    pub lab: Oklab,
}

impl Swatch {
    #[inline]
    pub fn from_hsv(hsv: Hsv) -> Self {
        let rgb: Srgb = Srgb::from_color(hsv).clamp();
        let lab: Oklab = Oklab::from_color(rgb);
        Self { hsv, rgb, lab }
    }
}

const fn saturation_at(i: usize) -> f32 {
    if i % 2 == 0 { 1.0 } else { 0.6 }
}

/// Variant 1: HSV hues warped via notch.
#[must_use]
pub fn hsv_warped(n: usize, params: WarpParams) -> Vec<Swatch> {
    let cdf = HueCdf::new(4096, |h| weight(h, params));
    (0..n)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let t = (i as f32 + 0.5) / n as f32; // midpoints
            let h = cdf.invert(t);
            let s = saturation_at(i);
            Swatch::from_hsv(Hsv::new(h, s, V_FIXED))
        })
        .collect()
}

/// Variant 2: Perceptual hue spacing (OKLCH) -> HSV hue; same S/V policy.
#[must_use]
pub fn hsv_perceptual_hue(n: usize) -> Vec<Swatch> {
    (0..n)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let h_ok = i as f32 * 360.0 / n as f32;
            let rgb: Srgb = Srgb::from_color(Oklch::new(L_OK, C_OK, h_ok)).clamp();
            let hsv_from_ok = Hsv::from_color(rgb);
            let h_deg = hsv_from_ok.hue.into_degrees();
            let s = saturation_at(i);
            Swatch::from_hsv(Hsv::new(h_deg, s, V_FIXED))
        })
        .collect()
}
