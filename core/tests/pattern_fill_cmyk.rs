//! Pattern fill layers on a CMYK document: the `pattern`
//! component of `dr_precision_doc_cmyk_artwork` at 8 and 16 bits.
//!
//! Oracles: at scale 1 and angle 0 with an integer origin O, pixel (x, y)
//! is the ink of tile[(x − Ox) mod w, (y − Oy) mod h] — O = phase − anchor
//! when linked, phase when unlinked — where a texel's ink is
//! `dr_cmyk_artwork_colors`' sRGB → ink answer under the document's profile,
//! put by this file on an 8-bit document's grid (`round(p · 2.55)` of the
//! four-decimal percent p, the effect-colour arithmetic measured in
//! Photoshop) and kept as is at 16 bits; and a Pattern Overlay of the same
//! fill on an opaque canvas raster, which renders the same samples.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_cmyk_artwork::{dr_cmyk_artwork_colors, dr_precision_doc_cmyk_artwork};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;
use crate::cmyk_structure_fixture::{free, json as take_json, LIMIT};
use crate::cmyk_style_fixture::{backdrop, flat, hard_rect, layer, styled};
use crate::common::take_err_string;

const W: u32 = 13;
const H: u32 = 7;

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// The 4 × 3 probe tile: twelve distinct opaque colours.
fn probe(x: u32, y: u32) -> [u8; 3] {
    [
        (40 + 50 * x) as u8,
        (30 + 70 * y) as u8,
        (200 - 40 * x - 20 * y) as u8,
    ]
}

/// A custom tile of `w × h` opaque texels `f(x, y)`.
fn tile(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Value {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&f(x, y));
            rgba.push(255);
        }
    }
    json!({"name": "Probe", "width": w, "height": h, "rgba": base64(&rgba)})
}

/// The fill keys: a linked layer carries `anchor` = −reference.
fn fill(pattern: Value, phase: (i32, i32), link: Option<(i32, i32)>) -> Value {
    let mut f = json!({"pattern": pattern, "angle": 0, "scale": 1,
        "link_with_layer": link.is_some(), "phase": [phase.0, phase.1]});
    if let Some(r) = link {
        f["anchor"] = json!([-r.0, -r.1]);
    }
    f
}

/// (the linked layer's reference, or None when unlinked; phase).
type Placement = (Option<(i32, i32)>, (i32, i32));

const PLACEMENTS: [Placement; 3] = [
    (Some((5, -3)), (1, 2)),
    (None, (1, 2)),
    (Some((0, 0)), (-2, 4)),
];

fn origin(link: Option<(i32, i32)>, phase: (i32, i32)) -> (i32, i32) {
    match link {
        Some(r) => (phase.0 + r.0, phase.1 + r.1),
        None => phase,
    }
}

/// A `W × H` CMYK document at `depth`, one paper layer.
fn base(depth: u32) -> *mut PrecisionDocument {
    backdrop(W, H, depth, &[[0.0; 4]])
}

/// `doc` with a canvas-sized artwork layer of `components` published above
/// layer 0 under `description`.
fn publish(
    doc: *const PrecisionDocument,
    components: &str,
    description: &str,
) -> Result<*mut PrecisionDocument, String> {
    let components = CString::new(components).unwrap();
    let name = CString::new("Pattern Fill").unwrap();
    let description = CString::new(description).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_artwork(
            doc,
            0,
            false,
            0,
            W,
            H,
            components.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(out)
}

fn description(fill: &Value) -> String {
    json!({"type": "fill", "version": 1, "fill": "pattern", "pattern_fill": fill,
        "canvas": [W, H]})
    .to_string()
}

fn components(fill: &Value) -> String {
    json!([{"kind": "pattern", "fill": fill}]).to_string()
}

/// Each texel's ink as the layer stores it (module doc): sRGB → ink through
/// `dr_cmyk_artwork_colors` (relative, BPC), on an 8-bit document's grid.
fn texel_inks(depth: u32, colours: &[[u8; 3]]) -> Vec<[u16; 4]> {
    let profile = press_profile("Press Test");
    let values: Vec<Vec<f64>> = colours
        .iter()
        .map(|c| c.iter().map(|v| f64::from(*v) / 255.0).collect())
        .collect();
    let request = CString::new(json!({ "values": values }).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let text = unsafe {
        dr_cmyk_artwork_colors(
            ptr::null(),
            0,
            profile.as_ptr(),
            profile.len(),
            request.as_ptr(),
            &mut err,
        )
    };
    let answer = take_json(text, err);
    answer["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ink| {
            let ink: Vec<f64> = ink
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            std::array::from_fn(|c| {
                // The ink as the core holds it (f32), then the grid.
                let v = f64::from(ink[c] as f32);
                if depth == 8 {
                    let percent = (v * 1e6).round() / 1e4;
                    let code = (percent * 2.55 + 0.5).floor().clamp(0.0, 255.0);
                    code as u16 * 257
                } else {
                    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
                }
            })
        })
        .collect()
}

#[test]
fn a_pattern_component_is_its_texels_ink_at_its_origin() {
    let colours: Vec<[u8; 3]> = (0..12).map(|i| probe(i % 4, i / 4)).collect();
    for depth in [8, 16] {
        let inks = texel_inks(depth, &colours);
        for (link, phase) in PLACEMENTS {
            let f = fill(tile(4, 3, probe), phase, link);
            let doc = base(depth);
            let out = publish(doc, &components(&f), &description(&f)).unwrap();
            let samples = unsafe { &*out }.cmyk_layer_samples(1).unwrap();
            let o = origin(link, phase);
            for y in 0..H {
                for x in 0..W {
                    let i = (x as i32 - o.0).rem_euclid(4) as u32;
                    let j = (y as i32 - o.1).rem_euclid(3) as u32;
                    let want = inks[(j * 4 + i) as usize];
                    let got = samples[(y * W + x) as usize];
                    assert_eq!(got[4], 65535, "{depth}-bit alpha at ({x},{y})");
                    for c in 0..4 {
                        assert!(
                            (i32::from(got[c]) - i32::from(want[c])).abs() <= 1,
                            "{depth}-bit {link:?} {phase:?} ({x},{y}) plate {c}: \
                             got {got:?}, want {want:?}"
                        );
                    }
                }
            }
            free(out);
            free(doc);
        }
    }
}

/// A canvas-sized opaque raster carrying a Pattern Overlay of the same fill
/// (its explicit anchor included) renders the fill layer's samples.
#[test]
fn a_cmyk_pattern_fill_matches_a_cmyk_pattern_overlay() {
    for depth in [8, 16] {
        for (link, phase) in PLACEMENTS {
            let f = fill(tile(4, 3, probe), phase, link);
            let doc = base(depth);
            let with_fill = publish(doc, &components(&f), &description(&f)).unwrap();
            let mut effect = f.clone();
            effect["type"] = json!("pattern_overlay");
            let shape = layer(
                base(depth),
                "Shape",
                [10.0, 60.0, 30.0, 0.0],
                &hard_rect(W, H, (0, 0, W, H)),
            );
            let overlaid = styled(shape, 1, &json!({ "effects": [effect] }));
            assert_eq!(
                flat(with_fill),
                flat(overlaid),
                "{depth}-bit {link:?} {phase:?}"
            );
            free(overlaid);
            free(with_fill);
            free(doc);
        }
    }
}

#[test]
fn a_large_pattern_component_is_admitted_and_malformed_ones_are_refused() {
    // A 160 × 160 custom tile: its base64 alone is past 64 KiB, and so is
    // the description.
    let big = fill(
        tile(160, 160, |x, y| [x as u8, y as u8, 7]),
        (0, 0),
        Some((0, 0)),
    );
    let text = components(&big);
    assert!(text.len() > 64 * 1024 && description(&big).len() > 64 * 1024);
    let doc = base(16);
    let out = publish(doc, &text, &description(&big)).unwrap();
    assert!(!out.is_null());
    let ink = texel_inks(16, &[[3, 5, 7]])[0];
    let got = unsafe { &*out }.cmyk_layer_samples(1).unwrap()[(5 * W + 3) as usize];
    for c in 0..4 {
        assert!(
            (i32::from(got[c]) - i32::from(ink[c])).abs() <= 1,
            "{got:?}"
        );
    }
    free(out);
    let good = fill(tile(4, 3, probe), (0, 0), None);
    let cases: Vec<(String, &str)> = vec![
        (
            json!([{"kind": "pattern", "fill": 7}]).to_string(),
            "fill must be an object",
        ),
        (
            components(&json!({"pattern": {"preset": "plaid"}})),
            "fill.pattern.preset must be one of",
        ),
        (
            components(&json!({"pattern": {"width": 4, "height": 3, "rgba": "AAAA"}})),
            "fill.pattern.rgba must be base64",
        ),
        (
            json!([{"kind": "pattern", "fill": good, "ink": [0, 0, 0, 0, 1]}]).to_string(),
            "unknown field",
        ),
    ];
    for (components, needle) in cases {
        let message = publish(doc, &components, &description(&good)).expect_err("a refusal");
        assert!(message.contains(needle), "{components}: {message}");
    }
    free(doc);
}
