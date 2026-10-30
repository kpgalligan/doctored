//! R5's turned edge against Photoshop's own render (take1 of
//! `crop-extend-665`, committed with its manifest under
//! `fixtures/photoshop_crop_extend/`; no new take).
//!
//! Photoshop's crop resamples with its Image Interpolation preference
//! (Bicubic Automatic by default). The fit runs the shared f64 reference
//! (`tests/common/inverse_mapping.rs`) over the take's source padded with the
//! background colour, at the take's crop (-10, -6, 84 × 60) turned by +8 and
//! -8 degrees, once per candidate Keys cubic a ∈ {-0.5 (today's bicubic),
//! -0.75, -1.0}, and judges each the way the take's `judge` does: the EDGE
//! BAND is every pixel of the candidate's result that is not the pad colour
//! but lies within 2 px (Chebyshev) of one that is; its worst code against
//! Photoshop is bounded by the ±2 declared before the take.
//!
//! The fit: no candidate's band is within ±2 (a = -0.5: 15
//! codes, interior 7; -0.75: 13, interior 2; -1.0: 16, interior 7, each at
//! -8°, the nearer sign), so today's kernel stays, and the production crop at
//! bicubic is asserted within 1 code of the a = -0.5 reference.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_delete_cropped::dr_doc_delete_cropped_pixels;
use doctored_core::ffi_geometry::dr_doc_geometry_edit;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use image::RgbaImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::inverse_mapping::{invert, reference_rgba};

/// The take's crop window, `(x, y, width, height)`.
const WINDOW: (i32, i32, u32, u32) = (-10, -6, 84, 60);
/// Take1's swatch, #C83228, opaque.
const PAD: [u32; 4] = [200, 50, 40, 255];
/// The take's declared edge-band bound, in codes.
const BOUND: u32 = 2;
/// The candidates' Keys parameters; -0.5 is today's bicubic.
const CANDIDATES: [f64; 3] = [-0.5, -0.75, -1.0];

fn folder() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/photoshop_crop_extend")
}

fn load(name: &str) -> RgbaImage {
    image::open(folder().join(name))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .to_rgba8()
}

/// The Crop tool's matrix: rotate by `−angle` about the window's centre.
fn rotation(angle: f64) -> [f64; 6] {
    let cx = f64::from(WINDOW.0) + f64::from(WINDOW.2) / 2.0;
    let cy = f64::from(WINDOW.1) + f64::from(WINDOW.3) / 2.0;
    let radians = -angle * std::f64::consts::PI / 180.0;
    let (s, c) = (radians.sin(), radians.cos());
    [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]
}

fn kind(a: f64) -> String {
    if a == -0.5 {
        "bicubic".into()
    } else {
        format!("keys:{a}")
    }
}

/// A reference plane: its samples, offset and size.
type Plane = (Vec<[u32; 4]>, (i32, i32), (u32, u32));

/// `before` padded with the colour by a margin wider than every tap reaches
/// from the window, so the reference reads exactly what the core's
/// `pad_rect` plane holds at every tap.
fn padded(before: &RgbaImage) -> Plane {
    let margin = 40u32;
    let (w, h) = before.dimensions();
    let (pw, ph) = (w + 2 * margin, h + 2 * margin);
    let mut out = vec![PAD; (pw * ph) as usize];
    for (x, y, p) in before.enumerate_pixels() {
        out[((y + margin) * pw + x + margin) as usize] = p.0.map(u32::from);
    }
    (out, (-(margin as i32), -(margin as i32)), (pw, ph))
}

fn reference(before: &RgbaImage, angle: f64, a: f64) -> Vec<[u32; 4]> {
    let (plane, at, dims) = padded(before);
    reference_rgba(
        &plane,
        dims,
        at,
        WINDOW,
        invert(rotation(angle)),
        &kind(a),
        255,
    )
}

/// The take's `codes`: the largest difference; two transparent pixels agree.
fn codes(a: [u32; 4], b: [u32; 4]) -> u32 {
    if a[3] == 0 && b[3] == 0 {
        return 0;
    }
    (0..4).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0)
}

/// The take's `judge` for a turned row: `(band, interior)` worst codes of
/// `ours` against `theirs`.
fn judge(ours: &[[u32; 4]], theirs: &RgbaImage) -> (u32, u32) {
    let (w, h) = (WINDOW.2 as i64, WINDOW.3 as i64);
    let at = |x: i64, y: i64| ours[(y * w + x) as usize];
    let (mut band, mut interior) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let mine = at(x, y);
            if mine == PAD {
                continue;
            }
            let d = codes(mine, theirs.get_pixel(x as u32, y as u32).0.map(u32::from));
            let near = (y - 2..=y + 2).filter(|j| (0..h).contains(j)).any(|j| {
                (x - 2..=x + 2)
                    .filter(|i| (0..w).contains(i))
                    .any(|i| at(i, j) == PAD)
            });
            if near {
                band = band.max(d);
            } else {
                interior = interior.max(d);
            }
        }
    }
    (band, interior)
}

/// The fit table: per candidate, the sign with the smaller band, as
/// `(a, angle, band, interior)`.
fn fit_table() -> Vec<(f64, f64, u32, u32)> {
    let before = load("R5_before.png");
    let after = load("R5_after.png");
    assert_eq!(
        after.dimensions(),
        (WINDOW.2, WINDOW.3),
        "the take's result size"
    );
    CANDIDATES
        .iter()
        .map(|&a| {
            [8.0, -8.0]
                .iter()
                .map(|&angle| {
                    let (band, interior) = judge(&reference(&before, angle, a), &after);
                    (a, angle, band, interior)
                })
                .min_by_key(|row| (row.2, row.3))
                .expect("two signs")
        })
        .collect()
}

/// The production crop at bicubic: `dr_doc_geometry_edit`'s straighten with
/// the Background fill and Delete Cropped Pixels, then the host trim.
fn production(before: &RgbaImage, angle: f64) -> Vec<[u32; 4]> {
    let doc = DrDocument::from_pixels(before.clone());
    let request: Value = json!({"op":"straighten","matrix":rotation(angle),"filter":"bicubic",
        "x":WINDOW.0,"y":WINDOW.1,"width":WINDOW.2,"height":WINDOW.3,"delete_cropped":true,
        "background":{"color":[PAD[0] as f64 / 255.0, PAD[1] as f64 / 255.0, PAD[2] as f64 / 255.0],
            "color_space":"document"}});
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_geometry_edit(
            &doc,
            c.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    let out = *unsafe { Box::from_raw(out) };
    let trimmed = unsafe { dr_doc_delete_cropped_pixels(&out) };
    let out = if trimmed.is_null() {
        out
    } else {
        *unsafe { Box::from_raw(trimmed) }
    };
    assert_eq!((out.width, out.height), (WINDOW.2, WINDOW.3));
    let layer = &out.layers[0];
    assert_eq!(
        layer.offset,
        (0, 0),
        "the padded Background covers the window"
    );
    (0..WINDOW.3)
        .flat_map(|y| (0..WINDOW.2).map(move |x| (x, y)))
        .map(|(x, y)| {
            layer
                .pixels
                .sample_at(x, y)
                .unwrap_or([0; 4])
                .map(u32::from)
        })
        .collect()
}

#[test]
fn r5_edge_band_fit_against_photoshop() {
    let table = fit_table();
    println!("R5 fit (take1, crop (-10, -6, 84, 60), bound ±{BOUND}):");
    println!("  a      angle  band  interior");
    for (a, angle, band, interior) in &table {
        println!("  {a:<6} {angle:<6} {band:<5} {interior}");
    }
    // No candidate fits (the batch's fit, table in its notes and the ADR):
    // today's kernel stays, and a candidate that later fits must adopt it.
    for (a, angle, band, _) in &table {
        assert!(
            *band > BOUND,
            "a = {a} at {angle}° fits (band {band}): adopt it"
        );
    }
    let before = load("R5_before.png");
    let today = table[0];
    let ours = production(&before, today.1);
    let oracle = reference(&before, today.1, -0.5);
    let worst = ours
        .iter()
        .zip(&oracle)
        .map(|(g, w)| codes(*g, *w))
        .max()
        .unwrap_or(0);
    assert!(
        worst <= 1,
        "production bicubic is {worst} codes off the a = -0.5 oracle"
    );
}
