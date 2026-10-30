//! The 8-bit native save plan against gate M's measured peaks at the
//! probe's sizes, built one after another (8256×5504, the 7500×7500 pair).
//! Alone in its binary: each case holds up to ~1.4 GB while it is checked.
//!
//! `MAX_E` is gate M's largest `ri_interval_max_phys_footprint` rise over the
//! six runs of each case (three with views live, three evicted;
//! `MallocLargeCache=0`, one case per process, Apple M2 Max, release; the
//! table is in `designs/phase-4-jobs-resource-status.md`). The plan, which
//! carries no added margin, must cover 1.25× it in every case.
//!
//! HEAD's `referenced × 4 + 64 MiB` (gate S, a77a225), live / evicted:
//! S1 4,429,660,464 / 2,975,547,696; S2 1,521,284,236 / 794,227,852;
//! S2p 2,975,520,520 / 2,248,464,136; S3 3,667,211,696 / 1,867,211,696.
//! S5 has S1's planes, so HEAD's figure is S1's. (Gate M's S4, a 64 MB
//! host-media resource attached to the placed layer, went with host-media
//! attachments on 2026-10-10.)

use doctored_core::doc::DrDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::retained_source::SourceEdit;
use image::{Rgba, RgbaImage};
use serde_json::json;
const W45: u32 = 8256;
const H45: u32 = 5504;

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Gate S's noise: RGB from the generator, alpha 255.
fn noise8(w: u32, h: u32, seed: u64) -> RgbaImage {
    let mut state = seed;
    let mut out = RgbaImage::new(w, h);
    for p in out.pixels_mut() {
        let v = splitmix(&mut state);
        p.0 = [v as u8, (v >> 8) as u8, (v >> 16) as u8, 255];
    }
    out
}

/// S5's noise: all four bytes of every pixel from the generator.
fn noise8_full(w: u32, h: u32, seed: u64) -> RgbaImage {
    let mut state = seed;
    let mut out = RgbaImage::new(w, h);
    for p in out.pixels_mut() {
        p.0 = (splitmix(&mut state) as u32).to_le_bytes();
    }
    out
}

/// File ▸ New's white Background with `source` placed over it.
fn placed(source: RgbaImage) -> DrDocument {
    let background = DrDocument::from_pixels(RgbaImage::from_pixel(W45, H45, Rgba([255; 4])));
    let source = DrDocument::from_pixels(source);
    let edit = SourceEdit::parse(
        &json!({"op":"place","source_id":"n","revision_id":"a","name":"Noise","x":0,"y":0})
            .to_string(),
    )
    .unwrap();
    background
        .source_edit(Some(&source), &edit, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap()
}

fn check(name: &str, doc: DrDocument, max_e: u64) {
    let scratch = doc.native_save_scratch_bytes().unwrap();
    let floor = (max_e * 5).div_ceil(4);
    assert!(
        scratch >= floor,
        "{name}: plan {scratch} < 1.25 × max E {max_e}"
    );
}

#[test]
fn plan_covers_gate_m_peaks_at_45_mp() {
    check("S1", placed(noise8(W45, H45, 0x5EED)), 744_637_016);
    check(
        "S2",
        DrDocument::from_pixels(noise8(W45, H45, 0x5EED)),
        561_971_704,
    );
    check(
        "S2p",
        placed(noise8(W45, H45, 0x5EED))
            .remove_layers(&[0])
            .unwrap(),
        743_752_304,
    );
    check(
        "S3",
        DrDocument::from_pixels(noise8(7500, 7500, 1))
            .adding_image_layer(0, noise8(7500, 7500, 2), "B")
            .unwrap(),
        920_666_856,
    );
    check("S5", placed(noise8_full(W45, H45, 0x5EED)), 795_493_000);
}
