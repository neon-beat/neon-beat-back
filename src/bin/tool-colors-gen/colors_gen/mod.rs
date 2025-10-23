//! Helper tool to generate colors to be used for backend's teams

#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic, clippy::nursery)]

mod generator;
mod html;
mod json_io;
mod reorder;
mod warp;

use anyhow::Result;
use generator::{hsv_perceptual_hue, hsv_warped};
use html::write_html_grid;
use reorder::reorder_maximin;
use warp::WarpParams;

use json_io::save_hsv_json;
use std::{env, fs, path::PathBuf};

const N: usize = 20;
const GRID_COLS: usize = 5;

pub fn run() -> Result<()> {
    let target_dir = env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let out_dir = target_dir.join("tool-colors-gen");
    fs::create_dir_all(&out_dir)?;

    // Warp parameters (peak-normalized)
    let warp_params = WarpParams::new(
        140.0, // center of compressed band (greenish)
        0.9,   // 0=no warp, 0.6..1.4 sensible, ↑ compresses more
        5.0,   // controls width: larger = narrower notch (≈ 1/σ^2). Try 3..8
    );

    // Variant 1: HSV warped
    let hsv_warp = hsv_warped(N, warp_params);
    let warped_path = write_html_grid(
        &format!(
            "HSV warped (μ={}°, strength={}, κ={})",
            warp_params.mu_deg, warp_params.strength, warp_params.kappa
        ),
        GRID_COLS,
        &hsv_warp,
        out_dir.join("hsv20_warped.html"),
    )?;
    let ordered_warp = reorder_maximin(&hsv_warp);
    let warped_maximin_path = write_html_grid(
        &format!(
            "HSV warped (μ={}°, strength={}, κ={}) — OKLab maximin",
            warp_params.mu_deg, warp_params.strength, warp_params.kappa
        ),
        GRID_COLS,
        &ordered_warp,
        out_dir.join("hsv20_warped_maximin.html"),
    )?;
    let warped_json_path = save_hsv_json(
        out_dir.join("hsv20_warped_maximin.json"),
        &ordered_warp.iter().map(|s| s.hsv).collect::<Vec<_>>(),
    )?;

    // Variant 2: Perceptual-hue (OKLCH)
    let hsv_perc = hsv_perceptual_hue(N);
    let perc_path = write_html_grid(
        "HSV perceptual hue (OKLCH)",
        GRID_COLS,
        &hsv_perc,
        out_dir.join("hsv20_perceptual.html"),
    )?;
    let ordered_perc = reorder_maximin(&hsv_perc);
    let perc_maximin_path = write_html_grid(
        "HSV perceptual hue (OKLCH) — OKLab maximin",
        GRID_COLS,
        &ordered_perc,
        out_dir.join("hsv20_perceptual_maximin.html"),
    )?;
    let perc_json_path = save_hsv_json(
        out_dir.join("hsv20_perceptual_maximin.json"),
        &ordered_perc.iter().map(|s| s.hsv).collect::<Vec<_>>(),
    )?;

    println!(
        "Generated color assets in {}:\n  - {}\n  - {}\n  - {}\n  - {}\n  - {}\n  - {}",
        out_dir.display(),
        warped_path.display(),
        warped_maximin_path.display(),
        warped_json_path.display(),
        perc_path.display(),
        perc_maximin_path.display(),
        perc_json_path.display()
    );

    Ok(())
}
