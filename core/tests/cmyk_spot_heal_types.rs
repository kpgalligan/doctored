//! The Spot Healing Brush's Type on a CMYK document's four inks,
//! through `dr_precision_doc_edit`'s `"type"`, against oracles written here:
//!
//! * Proximity Match heals the stroke to the flat ink planted inside a copy
//!   of the stroke's surroundings 40 px to its right (the converted picture
//!   carries the copy exactly, since the conversion is per pixel);
//! * the three types differ, a seed repeats bit for bit, alpha is never
//!   written and nothing outside the working window moves;
//! * Create Texture removes a dark spot from a flat and from a noise field
//!   rather than tiling it back: every ink's healed interior has the
//!   spotless field's mean and no more than its spread.

use doctored_core::precision_document::PrecisionDocument;
use serde_json::json;

use crate::cmyk_structure_fixture::{cmyk_layers, free, try_edit, RgbLayer};

const W: u32 = 200;
const H: u32 = 140;
const CENTRE: (u32, u32) = (80, 70);
const SHIFT: u32 = 40;
const RADIUS: f64 = 10.5;

/// A fixed hash of `(x, y, salt)` as a 16-bit code.
fn noise(x: u32, y: u32, salt: u32) -> u16 {
    let mut v = x
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(y.wrapping_mul(0x85EB_CA77))
        .wrapping_add(salt.wrapping_mul(0xC2B2_AE3D));
    v ^= v >> 15;
    v = v.wrapping_mul(0x2C1B_3C6D);
    v ^= v >> 12;
    (v >> 16) as u16
}

fn distance(x: u32, y: u32, c: (u32, u32)) -> f64 {
    (f64::from(x) - f64::from(c.0)).hypot(f64::from(y) - f64::from(c.1))
}

/// The noise picture with a dark spot under the stroke and, 40 px right, a
/// flat colour inside a copy of the stroke's surroundings; converted to
/// CMYK.
fn document() -> *mut PrecisionDocument {
    let base = |x: u32, y: u32| [noise(x, y, 1), noise(x, y, 2), noise(x, y, 3), 65535];
    let planted = (CENTRE.0 + SHIFT, CENTRE.1);
    let pixels = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let d = distance(x, y, planted);
            if d <= RADIUS {
                [52000, 9000, 30000, 65535]
            } else if d <= RADIUS + 6.0 {
                base(x - SHIFT, y)
            } else if distance(x, y, CENTRE) <= RADIUS {
                [2000, 2000, 2000, 65535]
            } else {
                base(x, y)
            }
        })
        .collect();
    cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Background",
            w: W,
            h: H,
            offset: (0, 0),
            pixels,
        }],
    )
}

fn coverage() -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            if distance(i % W, i / W, CENTRE) <= RADIUS {
                65535
            } else {
                0
            }
        })
        .collect()
}

fn heal(doc: *const PrecisionDocument, kind: &str, seed: u64) -> Vec<[u16; 5]> {
    let request = json!({"op":"spot_heal","layer":0,"x":CENTRE.0 - 11,"y":CENTRE.1 - 11,
        "width":23,"height":23,"seed":seed,"type":kind});
    let out =
        try_edit(doc, &request.to_string(), &coverage()).unwrap_or_else(|e| panic!("{kind}: {e}"));
    assert!(!out.is_null(), "{kind} changed the record");
    let samples = unsafe { &*out }.cmyk_layer_samples(0).unwrap();
    free(out);
    samples
}

#[test]
fn proximity_match_takes_the_planted_patch_on_the_inks() {
    let doc = document();
    let before = unsafe { &*doc }.cmyk_layer_samples(0).unwrap();
    let ink = before[((CENTRE.1) * W + CENTRE.0 + SHIFT) as usize];
    let after = heal(doc, "proximity_match", 7);
    let mut probes = 0;
    for (i, s) in after.iter().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        if distance(x, y, CENTRE) <= 8.5 {
            probes += 1;
            for c in 0..4 {
                assert!(
                    s[c].abs_diff(ink[c]) <= 257,
                    "({x},{y}) ink {c}: {} against the planted {}",
                    s[c],
                    ink[c]
                );
            }
        }
    }
    assert!(probes > 200);
    free(doc);
}

#[test]
fn the_types_differ_repeat_and_keep_the_outside_of_the_window_on_the_inks() {
    let doc = document();
    let before = unsafe { &*doc }.cmyk_layer_samples(0).unwrap();
    // The window pad from `inpaint_plan::requested_ring`'s rule restated: a
    // 21 px disc asks for 21 px, Proximity Match for 2·21 + 14.
    let mut results = Vec::new();
    for (kind, pad) in [
        ("content_aware", 21),
        ("create_texture", 21),
        ("proximity_match", 56),
    ] {
        let out = heal(doc, kind, 11);
        assert_eq!(out, heal(doc, kind, 11), "{kind}: a seed repeats");
        let (x0, y0, x1, y1) = (
            CENTRE.0 as i64 - 11 - pad,
            CENTRE.1 as i64 - 11 - pad,
            CENTRE.0 as i64 + 11 + pad,
            CENTRE.1 as i64 + 11 + pad,
        );
        for (i, (a, b)) in out.iter().zip(&before).enumerate() {
            let (x, y) = ((i as u32 % W) as i64, (i as u32 / W) as i64);
            assert_eq!(a[4], b[4], "{kind}: alpha is never written");
            if x < x0 || x > x1 || y < y0 || y > y1 {
                assert_eq!(a, b, "{kind}: ({x},{y}) outside the window moved");
            }
        }
        results.push(out);
    }
    assert_ne!(results[0], results[1]);
    assert_ne!(results[0], results[2]);
    assert_ne!(results[1], results[2]);
    free(doc);
}

/// The mean and standard deviation of ink `c` over the pixels within 6.5 px
/// of the stroke's segment (70, 48)–(90, 48).
fn interior_stats(samples: &[[u16; 5]], w: u32, c: usize) -> (f64, f64) {
    let (mut sum, mut squares, mut n) = (0f64, 0f64, 0usize);
    for (i, s) in samples.iter().enumerate() {
        let (x, y) = (f64::from(i as u32 % w), f64::from(i as u32 / w));
        let along = (x - 70.0).clamp(0.0, 20.0);
        if (x - 70.0 - along).hypot(y - 48.0) <= 6.5 {
            let v = f64::from(s[c]);
            sum += v;
            squares += v * v;
            n += 1;
        }
    }
    let mean = sum / n as f64;
    (mean, (squares / n as f64 - mean * mean).max(0.0).sqrt())
}

#[test]
fn create_texture_heals_the_spot_on_the_inks_instead_of_tiling_it_back() {
    // Photoshop take1's geometry: a dark disc of radius 5 at (80, 48) under
    // a 21 px stroke along (70, 48)–(90, 48), on a flat and a noise field.
    let (w, h) = (160u32, 96u32);
    let segment = |x: u32, y: u32| {
        let along = (f64::from(x) - 70.0).clamp(0.0, 20.0);
        (f64::from(x) - 70.0 - along).hypot(f64::from(y) - 48.0)
    };
    let flat = |_: u32, _: u32| 32896u16;
    let grain = |x: u32, y: u32| 18000 + noise(x, y, 12) % 15000;
    for (name, value) in [
        ("flat", &flat as &dyn Fn(u32, u32) -> u16),
        ("noise", &grain),
    ] {
        let picture = |spot: bool| {
            let pixels = (0..w * h)
                .map(|i| {
                    let (x, y) = (i % w, i / w);
                    if spot && (f64::from(x) - 80.0).hypot(f64::from(y) - 48.0) <= 5.0 {
                        [10280, 10280, 10280, 65535]
                    } else {
                        let v = value(x, y);
                        [v, v, v, 65535]
                    }
                })
                .collect();
            cmyk_layers(
                w,
                h,
                vec![RgbLayer {
                    name: "Background",
                    w,
                    h,
                    offset: (0, 0),
                    pixels,
                }],
            )
        };
        let spotless = picture(false);
        let reference = unsafe { &*spotless }.cmyk_layer_samples(0).unwrap();
        free(spotless);
        let doc = picture(true);
        let coverage: Vec<u16> = (0..w * h)
            .map(|i| {
                if segment(i % w, i / w) <= 10.5 {
                    65535
                } else {
                    0
                }
            })
            .collect();
        let request = json!({"op":"spot_heal","layer":0,"x":60,"y":38,"width":41,"height":21,
            "seed":0,"type":"create_texture"});
        let out = try_edit(doc, &request.to_string(), &coverage).unwrap();
        assert!(!out.is_null(), "{name}: the stroke heals");
        let healed = unsafe { &*out }.cmyk_layer_samples(0).unwrap();
        free(out);
        free(doc);
        for c in 0..4 {
            let (field_mean, field_spread) = interior_stats(&reference, w, c);
            let (mean, spread) = interior_stats(&healed, w, c);
            // 6 and 3 codes of 255, as the 8-bit oracle's bounds.
            assert!(
                (mean - field_mean).abs() <= 6.0 * 257.0,
                "{name} ink {c}: mean {mean} against the field's {field_mean}"
            );
            assert!(
                spread <= field_spread + 3.0 * 257.0,
                "{name} ink {c}: spread {spread} against the field's {field_spread}"
            );
        }
    }
}
