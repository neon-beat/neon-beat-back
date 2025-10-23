#![forbid(unsafe_code)]

use super::generator::Swatch;
use anyhow::Result;
use std::fs::File;
use std::io::{BufWriter, Write};

#[inline]
fn rgb_hex(c: palette::Srgb) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (r, g, b) = {
        let r = (c.red * 255.0).round().clamp(0.0, 255.0) as u8;
        let g = (c.green * 255.0).round().clamp(0.0, 255.0) as u8;
        let b = (c.blue * 255.0).round().clamp(0.0, 255.0) as u8;
        (r, g, b)
    };
    format!("#{r:02X}{g:02X}{b:02X}")
}

#[inline]
fn hsv_label(sw: &Swatch) -> String {
    let h = (sw.hsv.hue.into_degrees() % 360.0 + 360.0) % 360.0;
    let s = (sw.hsv.saturation.clamp(0.0, 1.0)) * 100.0;
    format!("{h:.0}°, {s:.0}%, 100%")
}

pub fn write_html_grid(
    title: &str,
    cols: usize,
    swatches: &[Swatch],
    path: impl AsRef<std::path::Path>,
) -> Result<std::path::PathBuf> {
    let path = path.as_ref();
    let f = File::create(path)?;
    let mut w = BufWriter::new(f);
    writeln!(
        w,
        r#"<!doctype html><meta charset="utf-8">
<style>
  body{{margin:0;background:#111;color:#eee;font-family:system-ui}}
  h2{{margin:12px}}
  .g{{display:grid;grid-template-columns:repeat({cols},1fr);gap:6px;padding:8px}}
  .s{{aspect-ratio:3/1;border-radius:10px;display:flex;align-items:center;justify-content:center;
      font-weight:700;text-shadow:0 1px 2px rgba(0,0,0,.35)}}
</style>
<h2>{title}</h2>
<div class="g">"#
    )?;
    for sw in swatches {
        let hex = rgb_hex(sw.rgb);
        writeln!(
            w,
            r#"<div class="s" style="background:{hex}">{} | {hex}</div>"#,
            hsv_label(sw)
        )?;
    }
    writeln!(w, "</div>")?;
    Ok(path.to_path_buf())
}
