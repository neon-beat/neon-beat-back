#![forbid(unsafe_code)]

use anyhow::Result;
use palette::Hsv;
use std::{fs::File, io::BufWriter, path::PathBuf};

/// Serialize a vector of HSV colors to a JSON file.
pub fn save_hsv_json(path: impl AsRef<std::path::Path>, colors: &[Hsv]) -> Result<PathBuf> {
    let path = path.as_ref();
    let f = File::create(path)?;
    let w = BufWriter::new(f);
    serde_json::to_writer_pretty(w, colors)?;
    Ok(path.to_path_buf())
}
