//! A 32-bit float document built from exact samples, for the files that test
//! a float composite against an oracle: the test's own RZCB v3
//! payload writer (float samples, no coverage, no editable section, offset
//! (0, 0) — `float_blend_modes.rs`'s), `PrecisionDocument::from_clipboard` for
//! the first record and `clipboard_paste` (in place, Preserve) for every
//! record above it, each pasted bit for bit. A suite root mounts it with
//! `#[path = "common/float_fixture.rs"] mod float_fixture;`.
#![allow(dead_code)]

use std::sync::Arc;

use doctored_core::icc::IccProfile;
use doctored_core::precision_clipboard::{ClipboardPayload, PasteRequest};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::json;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// RZCB v3 with f32 samples: `w × h` straight RGBA, row 0 = top.
pub fn drcb(w: u32, h: u32, profile: &[u8], pixels: &[[f32; 4]]) -> Vec<u8> {
    let mut out = b"RZCB".to_vec();
    out.extend(3u32.to_le_bytes());
    out.extend(2u32.to_le_bytes());
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    out.extend(0i32.to_le_bytes());
    out.extend(0i32.to_le_bytes());
    out.extend(72f32.to_le_bytes());
    out.extend(72f32.to_le_bytes());
    out.extend((profile.len() as u32).to_le_bytes());
    out.extend(profile);
    out.push(0);
    out.extend(0u32.to_le_bytes());
    out.extend(4f32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    pixels
        .iter()
        .flatten()
        .for_each(|v| out.extend(v.to_le_bytes()));
    out
}

fn payload(
    w: u32,
    h: u32,
    profile: &Arc<IccProfile>,
    pixels: &[[f32; 4]],
    limit: u64,
) -> ClipboardPayload {
    assert_eq!(pixels.len(), (w * h) as usize);
    ClipboardPayload::decode(&drcb(w, h, profile.bytes(), pixels), limit, 0).expect("a payload")
}

/// A `w × h` float document in `profile` whose records, bottom first, hold
/// `records` exactly (each canvas-sized, named "L0", "L1", …).
pub fn float_doc(
    w: u32,
    h: u32,
    profile: &Arc<IccProfile>,
    records: &[Vec<[f32; 4]>],
) -> PrecisionDocument {
    float_doc_at(LIMIT, w, h, profile, records)
}

/// [`float_doc`] with every step admitted at `limit` rather than the
/// reference limit: a 100 MP float record is past what 2 GiB admits.
pub fn float_doc_at(
    limit: u64,
    w: u32,
    h: u32,
    profile: &Arc<IccProfile>,
    records: &[Vec<[f32; 4]>],
) -> PrecisionDocument {
    let (first, rest) = records.split_first().expect("one record at least");
    let mut doc =
        PrecisionDocument::from_clipboard(&payload(w, h, profile, first, limit), limit, 0, None)
            .expect("a float document");
    assert!(doc.is_float());
    for (i, pixels) in rest.iter().enumerate() {
        let request: PasteRequest = serde_json::from_value(json!({
            "name": format!("L{}", i + 1), "anchor": i, "position": "above",
            "placement": "in_place", "profile": "preserve",
        }))
        .unwrap();
        doc = doc
            .clipboard_paste(
                &payload(w, h, profile, pixels, limit),
                &request,
                limit,
                0,
                None,
            )
            .expect("the record pastes");
    }
    assert_eq!(doc.layer_count(), records.len());
    // A paste may trim a record to its visible pixels; every visible sample
    // lands bit for bit at its canvas position either way.
    for (i, pixels) in records.iter().enumerate() {
        let layer = doc.layer(i).unwrap();
        let (ox, oy) = layer.offset();
        let image = layer.source().unwrap();
        for (k, want) in pixels.iter().enumerate() {
            let (x, y) = ((k as u32 % w) as i64, (k as u32 / w) as i64);
            let (lx, ly) = (x - i64::from(ox), y - i64::from(oy));
            let got = (lx >= 0 && ly >= 0)
                .then(|| image.sample_at(lx as u32, ly as u32))
                .flatten();
            if want[3] > 0.0 {
                assert_eq!(
                    got.map(|g| g.map(f32::to_bits)),
                    Some(want.map(f32::to_bits)),
                    "record {i} sample ({x}, {y}) lands bit for bit"
                );
            } else {
                assert!(got.is_none_or(|g| g[3] == 0.0), "record {i} ({x}, {y})");
            }
        }
    }
    doc
}

/// A two-record `w × h` sRGB float document for the regional suites: an
/// opaque backdrop running from -0.5 to 3.5 across x with a negative green,
/// and above it a source shaped as an ellipse with a graded rim, its colour
/// above white on the left and holding a negative component, transparent
/// outside.
pub fn shaped_pair(w: u32, h: u32) -> PrecisionDocument {
    let backdrop = (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            [
                -0.5 + 4.0 * x / w as f32,
                0.25 - 0.5 * y / h as f32,
                0.6,
                1.0,
            ]
        })
        .collect();
    let source = (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f32 + 0.5, (i / w) as f32 + 0.5);
            let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
            let r = (((x - cx) / (cx - 2.0)).powi(2) + ((y - cy) / (cy - 2.0)).powi(2)).sqrt();
            let alpha = ((1.0 - r) * 4.0).clamp(0.0, 1.0);
            if alpha <= 0.0 {
                [0.0; 4]
            } else {
                [2.5 - 3.0 * x / w as f32, -0.125, 0.75 * y / h as f32, alpha]
            }
        })
        .collect();
    float_doc(w, h, &IccProfile::srgb(), &[backdrop, source])
}
