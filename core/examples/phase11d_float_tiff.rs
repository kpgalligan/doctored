//! Writes one 32-bit float TIFF through the Phase 11D seam, so an
//! INDEPENDENT reader (`tiffinfo`, `magick`) can be pointed at a file this
//! crate produced.
//!
//! It exists because the seam has no FFI export and no app surface by
//! design — 11F owns that — and a claim that libtiff reads what this writes
//! needs a file to make it about. The drive
//! (`scripts/verify_phase11d_formats.py`, gate D4) runs it.
//!
//! Usage: `cargo run --release --example phase11d_float_tiff -- <path>`

use doctored_core::tiff_float::{self, FloatTiff};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let Some(path) = arguments.next() else {
        eprintln!("usage: phase11d_float_tiff <path>");
        std::process::exit(2);
    };
    let (width, height) = (64u32, 48u32);
    // A ramp per channel plus an above-one value in one corner, so a reader
    // that clamps or rescales is visible in its own report.
    let mut samples = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            samples.push(x as f32 / (width - 1) as f32);
            samples.push(y as f32 / (height - 1) as f32);
            samples.push(if x == 0 && y == 0 { 4.0 } else { 0.25 });
        }
    }
    let image = FloatTiff {
        width,
        height,
        channels: 3,
        alpha: None,
        samples,
        icc: None,
        resolution: None,
        metadata: Default::default(),
        non_finite: 0,
    };
    let bytes = match tiff_float::encode_to_vec(&image, false) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    // Read it back through the same seam before handing it to anyone else:
    // a file this crate cannot decode is not evidence about the encoder.
    match tiff_float::decode(&bytes, &path, 512 * 1024 * 1024) {
        Ok(decoded) => {
            assert_eq!(decoded.samples, image.samples, "round trip");
            println!(
                "{} x {} x {} float samples",
                decoded.width, decoded.height, decoded.channels
            );
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
    if let Err(error) = std::fs::write(&path, &bytes) {
        eprintln!("failed to write {path}: {error}");
        std::process::exit(1);
    }
}
