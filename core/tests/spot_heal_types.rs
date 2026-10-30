//! The Spot Healing Brush's Type at 8, 16 and 32 bits: the byte
//! path through `dr_doc_spot_heal_layer_job`'s `spot_type`, the typed path
//! through `dr_precision_doc_edit`'s `"type"`. Each verdict is an oracle
//! written here on a synthetic field:
//!
//! * Proximity Match, with a copy of the stroke's surroundings planted 40 px
//!   to its right around a flat colour, heals the stroke to that colour —
//!   the planted patch is the only one whose band matches exactly, and a
//!   patch whose band matches leaves the healing blend nothing to correct;
//! * Create Texture keeps the texture UNDER the stroke (fine two-tone noise)
//!   where the picture around it is coarse stripes, which Content-Aware
//!   copies instead, and never tiles back the spot it removes: on a flat
//!   field and on a noise field the healed interior has the spotless
//!   field's mean and no more than its spread;
//! * the three types differ, a seed repeats bit for bit, a second Create
//!   Texture stroke changes the picture again, and nothing outside the
//!   working window moves;
//! * an unknown code, an unknown name, a `type` on another op, a stroke
//!   with no room for a patch and a transparent pool refuse.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_heal::dr_doc_spot_heal_to_new_layer;
use doctored_core::ffi_jobs::{
    dr_doc_spot_heal_layer_job, dr_job_control_create, dr_job_control_free,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

use crate::common::{doc_of, layer_bytes, take_doc};
use crate::float_fixture::float_doc;

/// The wire names and C codes, in code order.
const TYPES: [(&str, u32); 3] = [
    ("content_aware", 0),
    ("create_texture", 1),
    ("proximity_match", 2),
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Depth {
    Byte,
    Sixteen,
    Float,
}

const DEPTHS: [Depth; 3] = [Depth::Byte, Depth::Sixteen, Depth::Float];

/// A fixed hash of `(x, y, salt)` as a byte: an iid-looking field no shift
/// of itself matches.
fn noise(x: u32, y: u32, salt: u32) -> u8 {
    let mut v = x
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(y.wrapping_mul(0x85EB_CA77))
        .wrapping_add(salt.wrapping_mul(0xC2B2_AE3D));
    v ^= v >> 15;
    v = v.wrapping_mul(0x2C1B_3C6D);
    v ^= v >> 12;
    v = v.wrapping_mul(0x297A_2D39);
    v ^= v >> 15;
    (v >> 24) as u8
}

/// The distance from `(x, y)` to the segment `a`–`b`.
fn to_segment((x, y): (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length == 0.0 {
        0.0
    } else {
        (((x - a.0) * dx + (y - a.1) * dy) / length).clamp(0.0, 1.0)
    };
    ((x - a.0 - t * dx).powi(2) + (y - a.1 - t * dy).powi(2)).sqrt()
}

/// A hard round stroke of `radius` along `a`–`b`, canvas-sized.
fn stroke(w: u32, h: u32, a: (f64, f64), b: (f64, f64), radius: f64) -> Vec<bool> {
    (0..w * h)
        .map(|i| to_segment((f64::from(i % w), f64::from(i / w)), a, b) <= radius)
        .collect()
}

/// The covered pixels' bounding box `(x, y, w, h)`.
fn bounds(w: u32, cover: &[bool]) -> (u32, u32, u32, u32) {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, c) in cover.iter().enumerate() {
        if *c {
            let (x, y) = (i as u32 % w, i as u32 / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    (x0, y0, x1 - x0 + 1, y1 - y0 + 1)
}

/// The byte spot heal of an opaque `field` under `cover`, type `code`, seed
/// `seed`, through the job export: the healed RGB, `None` for no change.
fn byte_heal(
    (w, h): (u32, u32),
    field: &[[u8; 3]],
    cover: &[bool],
    code: u32,
    seed: u64,
) -> Result<Option<Vec<[u8; 3]>>, String> {
    let doc = doc_of(w, h, |x, y| field[(y * w + x) as usize]);
    let handle = Box::into_raw(Box::new(doc));
    let overlay: Vec<u8> = cover
        .iter()
        .flat_map(|c| [0, 0, 0, if *c { 255 } else { 0 }])
        .collect();
    let control = unsafe { dr_job_control_create() };
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_spot_heal_layer_job(
            handle,
            0,
            overlay.as_ptr(),
            w,
            h,
            1.0,
            0,
            seed,
            code,
            0,
            false,
            control,
            &mut err,
        )
    };
    unsafe { dr_job_control_free(control) };
    unsafe { dr_doc_free(handle) };
    match take_doc(out, err) {
        (_, Some(message)) => Err(message),
        (None, None) => Ok(None),
        (Some(doc), None) => Ok(Some(
            layer_bytes(&doc, 0)
                .chunks(4)
                .map(|p| [p[0], p[1], p[2]])
                .collect(),
        )),
    }
}

/// `dr_precision_doc_edit` with `request` and u16 coverage.
fn typed_edit(
    doc: &PrecisionDocument,
    request: &Value,
    coverage: &[u16],
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if !error.is_null() {
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        assert!(out.is_null());
        return Err(message);
    }
    Ok((!out.is_null()).then(|| unsafe { Box::from_raw(out) }))
}

/// A one-raster typed document of the byte `field` at `depth`: 16-bit codes
/// `v·257`, or 32-bit float linear samples `v/255`.
fn typed_document((w, h): (u32, u32), field: &[[u8; 3]], depth: Depth) -> PrecisionDocument {
    match depth {
        Depth::Sixteen => {
            let pixels = field
                .iter()
                .map(|p| [p[0], p[1], p[2], 255].map(|v| u16::from(v) * 257))
                .collect();
            PrecisionDocument::from_image(
                PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
                Metadata::default(),
                Resolution::default(),
            )
            .unwrap()
        }
        Depth::Float => {
            let pixels: Vec<[f32; 4]> = field
                .iter()
                .map(|p| {
                    [
                        f32::from(p[0]) / 255.0,
                        f32::from(p[1]) / 255.0,
                        f32::from(p[2]) / 255.0,
                        1.0,
                    ]
                })
                .collect();
            float_doc(w, h, &IccProfile::srgb(), &[pixels])
        }
        Depth::Byte => unreachable!("the byte path is byte_heal"),
    }
}

/// The spot heal at `depth`, every result in byte units (16-bit codes / 257,
/// float samples × 255) so one oracle reads all three.
fn heal(
    depth: Depth,
    (w, h): (u32, u32),
    field: &[[u8; 3]],
    cover: &[bool],
    kind: usize,
    seed: u64,
) -> Result<Option<Vec<[f64; 3]>>, String> {
    let (name, code) = TYPES[kind];
    if depth == Depth::Byte {
        return byte_heal((w, h), field, cover, code, seed)
            .map(|out| out.map(|p| p.iter().map(|q| q.map(f64::from)).collect()));
    }
    let doc = typed_document((w, h), field, depth);
    let (x, y, bw, bh) = bounds(w, cover);
    let mut request = json!({"op":"spot_heal","layer":0,"x":x,"y":y,"width":bw,"height":bh,
        "seed":seed});
    if code != 0 {
        request["type"] = json!(name);
    }
    let coverage: Vec<u16> = cover.iter().map(|c| if *c { 65535 } else { 0 }).collect();
    let Some(out) = typed_edit(&doc, &request, &coverage)? else {
        return Ok(None);
    };
    let source = out.layer_source(0).unwrap();
    Ok(Some(match depth {
        Depth::Sixteen => source
            .to_rgba16()
            .unwrap()
            .iter()
            .map(|p| [0, 1, 2].map(|c| f64::from(p[c]) / 257.0))
            .collect(),
        _ => source
            .to_rgba32()
            .unwrap()
            .iter()
            .map(|p| [0, 1, 2].map(|c| f64::from(p[c]) * 255.0))
            .collect(),
    }))
}

/// The picture the Proximity test heals: a noise field with a dark spot
/// under a 21 px disc at `centre`, and 40 px to its right a copy of the
/// disc's surroundings (out to 6 px past its edge) around a flat `X`.
const X: [u8; 3] = [200, 40, 120];
const SHIFT: u32 = 40;

fn planted((w, h): (u32, u32), centre: (u32, u32)) -> (Vec<[u8; 3]>, Vec<bool>) {
    let base = |x: u32, y: u32| [noise(x, y, 1), noise(x, y, 2), noise(x, y, 3)];
    let radius = 10.5;
    let at = |x: u32, y: u32, c: (u32, u32)| {
        (f64::from(x) - f64::from(c.0)).hypot(f64::from(y) - f64::from(c.1))
    };
    let planted_at = (centre.0 + SHIFT, centre.1);
    let field = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let d = at(x, y, planted_at);
            if d <= radius {
                X
            } else if d <= radius + 6.0 {
                base(x - SHIFT, y)
            } else if at(x, y, centre) <= radius {
                [10, 10, 10]
            } else {
                base(x, y)
            }
        })
        .collect();
    let cover = (0..w * h)
        .map(|i| at(i % w, i / w, centre) <= radius)
        .collect();
    (field, cover)
}

#[test]
fn proximity_match_takes_the_planted_patch_at_every_depth() {
    let size = (200, 140);
    let centre = (80, 70);
    let (field, cover) = planted(size, centre);
    for depth in DEPTHS {
        let out = heal(depth, size, &field, &cover, 2, 7)
            .unwrap()
            .expect("the stroke heals");
        let mut probes = 0;
        for (i, p) in out.iter().enumerate() {
            let (x, y) = (i as u32 % size.0, i as u32 / size.0);
            let d = (f64::from(x) - f64::from(centre.0)).hypot(f64::from(y) - f64::from(centre.1));
            if d <= 8.5 {
                probes += 1;
                for c in 0..3 {
                    assert!(
                        (p[c] - f64::from(X[c])).abs() <= 1.0,
                        "{depth:?} ({x},{y}) channel {c}: {} against the planted {}",
                        p[c],
                        X[c]
                    );
                }
            }
        }
        assert!(probes > 200, "{probes} interior probes");
    }
}

/// Mean |Δx| and the standard deviation of one channel over `inside`.
fn texture_stats(w: u32, values: &[f64], inside: &[bool]) -> (f64, f64) {
    let (mut steps, mut count) = (0f64, 0usize);
    let (mut sum, mut squares, mut n) = (0f64, 0f64, 0usize);
    for (i, v) in values.iter().enumerate() {
        if !inside[i] {
            continue;
        }
        sum += v;
        squares += v * v;
        n += 1;
        if (i as u32 % w) + 1 < w && inside[i + 1] {
            steps += (values[i + 1] - v).abs();
            count += 1;
        }
    }
    let mean = sum / n as f64;
    (
        steps / count as f64,
        (squares / n as f64 - mean * mean).max(0.0).sqrt(),
    )
}

#[test]
fn create_texture_keeps_the_strokes_own_texture_where_content_aware_copies_the_stripes() {
    let (w, h) = (160u32, 96u32);
    let (a, b) = ((70.0, 48.0), (90.0, 48.0));
    let cover = stroke(w, h, a, b, 10.5);
    // Inside: iid two-tone noise at 90 / 110 (mean |Δx| 10, std 10).
    // Outside: 2 px vertical stripes at 0 / 100 (mean |Δx| 50).
    let field: Vec<[u8; 3]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let v = if cover[i as usize] {
                if noise(x, y, 9) & 1 == 0 {
                    90
                } else {
                    110
                }
            } else if (x / 2) % 2 == 0 {
                0
            } else {
                100
            };
            [v, v, v]
        })
        .collect();
    let interior: Vec<bool> = (0..w * h)
        .map(|i| to_segment((f64::from(i % w), f64::from(i / w)), a, b) <= 7.5)
        .collect();
    let original: Vec<f64> = field.iter().map(|p| f64::from(p[0])).collect();
    let (noise_step, noise_std) = texture_stats(w, &original, &interior);
    assert!((noise_step - 10.0).abs() < 2.0 && (noise_std - 10.0).abs() < 1.0);
    for depth in DEPTHS {
        let channel = |kind| -> Vec<f64> {
            heal(depth, (w, h), &field, &cover, kind, 3)
                .unwrap()
                .expect("the stroke heals")
                .iter()
                .map(|p| p[0])
                .collect()
        };
        let (step, std) = texture_stats(w, &channel(1), &interior);
        assert!(step < 30.0, "{depth:?}: Create Texture's mean |dx| {step}");
        assert!(
            (std / noise_std - 1.0).abs() <= 0.5,
            "{depth:?}: Create Texture's std {std} against the noise's {noise_std}"
        );
        let (aware, _) = texture_stats(w, &channel(0), &interior);
        assert!(aware > 35.0, "{depth:?}: Content-Aware's mean |dx| {aware}");
    }
}

/// Mean and standard deviation of one channel over `inside`.
fn mean_and_spread(values: &[f64], inside: &[bool]) -> (f64, f64) {
    let (mut sum, mut squares, mut n) = (0f64, 0f64, 0usize);
    for (v, keep) in values.iter().zip(inside) {
        if *keep {
            sum += v;
            squares += v * v;
            n += 1;
        }
    }
    let mean = sum / n as f64;
    (mean, (squares / n as f64 - mean * mean).max(0.0).sqrt())
}

#[test]
fn create_texture_heals_the_spot_instead_of_tiling_it_back() {
    // Photoshop take1's geometry: a (40, 40, 40) disc of radius 5 at
    // (80, 48) under a 21 px stroke along (70, 48)–(90, 48).
    let (w, h) = (160u32, 96u32);
    let (a, b) = ((70.0, 48.0), (90.0, 48.0));
    let cover = stroke(w, h, a, b, 10.5);
    let interior: Vec<bool> = (0..w * h)
        .map(|i| to_segment((f64::from(i % w), f64::from(i / w)), a, b) <= 6.5)
        .collect();
    let flat = |_: u32, _: u32| 128u8;
    let grain = |x: u32, y: u32| 70 + noise(x, y, 12) % 61;
    for (name, value) in [
        ("flat", &flat as &dyn Fn(u32, u32) -> u8),
        ("noise", &grain),
    ] {
        let spotless: Vec<f64> = (0..w * h).map(|i| f64::from(value(i % w, i / w))).collect();
        let (field_mean, field_spread) = mean_and_spread(&spotless, &interior);
        let field: Vec<[u8; 3]> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                if (f64::from(x) - 80.0).hypot(f64::from(y) - 48.0) <= 5.0 {
                    [40, 40, 40]
                } else {
                    [value(x, y); 3]
                }
            })
            .collect();
        for depth in DEPTHS {
            let out = heal(depth, (w, h), &field, &cover, 1, 0)
                .unwrap()
                .expect("the stroke heals");
            for c in 0..3 {
                let channel: Vec<f64> = out.iter().map(|p| p[c]).collect();
                let (mean, spread) = mean_and_spread(&channel, &interior);
                assert!(
                    (mean - field_mean).abs() <= 6.0,
                    "{name} {depth:?} channel {c}: mean {mean} against the field's {field_mean}"
                );
                assert!(
                    spread <= field_spread + 3.0,
                    "{name} {depth:?} channel {c}: spread {spread} against the field's \
                     {field_spread}"
                );
            }
        }
    }
}

/// A plasma-like smooth field with a dark textured spot, for the comparisons that
/// need a picture every type can heal.
fn spotted((w, h): (u32, u32), centre: (u32, u32)) -> (Vec<[u8; 3]>, Vec<bool>) {
    let cover = stroke(
        w,
        h,
        (f64::from(centre.0), f64::from(centre.1)),
        (f64::from(centre.0), f64::from(centre.1)),
        8.5,
    );
    let field = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if cover[i as usize] {
                [15 + noise(x, y, 6) / 4, 15 + noise(x, y, 7) / 4, 15]
            } else {
                let mut p = crate::common::plasma(x, y);
                p[2] = p[2].saturating_add(noise(x, y, 4) / 8);
                p
            }
        })
        .collect();
    (field, cover)
}

#[test]
fn the_types_differ_repeat_from_a_seed_and_keep_the_outside_of_the_window() {
    let size = (120u32, 100u32);
    let centre = (60, 50);
    let (field, cover) = spotted(size, centre);
    let area = cover.iter().filter(|c| **c).count() as f64;
    // The window's pad, from `inpaint_plan::requested_ring`'s rule restated:
    // the hole's radius, never under 21; Proximity Match's search ring is
    // twice that plus two patches.
    let base = (area / std::f64::consts::PI).sqrt().ceil().max(21.0);
    let (bx, by, bw, bh) = bounds(size.0, &cover);
    for depth in DEPTHS {
        let mut results = Vec::new();
        for (kind, (name, _)) in TYPES.iter().enumerate() {
            let out = heal(depth, size, &field, &cover, kind, 11)
                .unwrap()
                .expect("the stroke heals");
            let again = heal(depth, size, &field, &cover, kind, 11)
                .unwrap()
                .unwrap();
            let bits =
                |v: &[[f64; 3]]| -> Vec<u64> { v.iter().flatten().map(|f| f.to_bits()).collect() };
            assert_eq!(bits(&out), bits(&again), "{depth:?} {name}: a seed repeats");
            let pad = if kind == 2 { 2.0 * base + 14.0 } else { base } as i64;
            for (i, p) in out.iter().enumerate() {
                let (x, y) = (i as i64 % size.0 as i64, i as i64 / size.0 as i64);
                let outside = x < i64::from(bx) - pad
                    || x >= i64::from(bx + bw) + pad
                    || y < i64::from(by) - pad
                    || y >= i64::from(by + bh) + pad;
                if outside {
                    let want = field[i].map(f64::from);
                    for c in 0..3 {
                        assert!(
                            (p[c] - want[c]).abs()
                                <= if depth == Depth::Float { 1e-3 } else { 0.0 },
                            "{depth:?} {name}: ({x},{y}) outside the window moved"
                        );
                    }
                }
            }
            results.push(out);
        }
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            assert_ne!(
                results[i], results[j],
                "{depth:?}: {} and {} heal alike",
                TYPES[i].0, TYPES[j].0
            );
        }
    }
}

#[test]
fn a_second_create_texture_stroke_textures_again() {
    let size = (120u32, 100u32);
    let (field, cover) = spotted(size, (60, 50));
    let first = byte_heal(size, &field, &cover, 1, 5).unwrap().unwrap();
    let second = byte_heal(size, &first, &cover, 1, 5)
        .unwrap()
        .expect("a second stroke changes the picture again");
    assert_ne!(first, second);
    assert_eq!(
        byte_heal(size, &field, &cover, 1, 5).unwrap().unwrap(),
        first
    );
    // The seed orders the tiled blocks, so it shows where the stroke covers
    // texture: the same stroke over the spotless plasma. Over the spot alone
    // nothing is texture and the trend fills, whatever the seed.
    let textured: Vec<[u8; 3]> = (0..size.0 * size.1)
        .map(|i| {
            let (x, y) = (i % size.0, i / size.0);
            let mut p = crate::common::plasma(x, y);
            p[2] = p[2].saturating_add(noise(x, y, 4) / 8);
            p
        })
        .collect();
    assert_ne!(
        byte_heal(size, &textured, &cover, 1, 6).unwrap().unwrap(),
        byte_heal(size, &textured, &cover, 1, 5).unwrap().unwrap(),
        "another seed"
    );
}

#[test]
fn unknown_types_and_impossible_strokes_refuse() {
    let size = (120u32, 100u32);
    let (field, cover) = spotted(size, (60, 50));
    // An unknown code: NULL with no message, on both byte exports.
    assert_eq!(byte_heal(size, &field, &cover, 3, 0), Ok(None));
    let doc = doc_of(size.0, size.1, |x, y| field[(y * size.0 + x) as usize]);
    let handle = Box::into_raw(Box::new(doc));
    let overlay: Vec<u8> = cover
        .iter()
        .flat_map(|c| [0, 0, 0, if *c { 255 } else { 0 }])
        .collect();
    let plate: Vec<u8> = field.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    let name = CString::new("Retouch").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_spot_heal_to_new_layer(
            handle,
            0,
            overlay.as_ptr(),
            size.0,
            size.1,
            1.0,
            0,
            0,
            3,
            plate.as_ptr(),
            name.as_ptr(),
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null() && err.is_null(), "an unknown new-layer type");
    // NULL inputs answer NULL whatever the type.
    let out = unsafe {
        dr_doc_spot_heal_layer_job(
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            1.0,
            0,
            0,
            1,
            0,
            false,
            ptr::null(),
            ptr::null_mut(),
        )
    };
    assert!(out.is_null());
    unsafe { dr_doc_free(handle) };

    // The typed request: an unknown name, and a type on another op.
    let typed = typed_document(size, &field, Depth::Sixteen);
    let coverage: Vec<u16> = cover.iter().map(|c| if *c { 65535 } else { 0 }).collect();
    let (x, y, w, h) = bounds(size.0, &cover);
    let bogus = json!({"op":"spot_heal","layer":0,"x":x,"y":y,"width":w,"height":h,
        "type":"bogus"});
    let message = typed_edit(&typed, &bogus, &coverage)
        .err()
        .expect("refused");
    assert!(
        message.starts_with("invalid native retouch request"),
        "{message}"
    );
    let heal = json!({"op":"retouch_heal","layer":0,"matrix":[1,0,0,1,-20,0],
        "type":"create_texture"});
    assert_eq!(
        typed_edit(&typed, &heal, &coverage).err().expect("refused"),
        "type is not an argument of this retouch operation"
    );

    // Proximity Match on a canvas with no room for a second stroke-sized
    // patch beside the first.
    let small = (30u32, 30u32);
    let tight = stroke(small.0, small.1, (15.0, 15.0), (15.0, 15.0), 9.5);
    let flat: Vec<[u8; 3]> = (0..small.0 * small.1)
        .map(|i| [noise(i, 0, 5), 90, 90])
        .collect();
    let refusal =
        "Proximity Match found no clean patch beside the stroke as large as the stroke itself; \
         use a smaller brush, or the Content-Aware type.";
    assert_eq!(
        byte_heal(small, &flat, &tight, 2, 0),
        Err(refusal.to_string())
    );
    let typed_small = typed_document(small, &flat, Depth::Sixteen);
    let (x, y, w, h) = bounds(small.0, &tight);
    let request = json!({"op":"spot_heal","layer":0,"x":x,"y":y,"width":w,"height":h,
        "type":"proximity_match"});
    let tight16: Vec<u16> = tight.iter().map(|c| if *c { 65535 } else { 0 }).collect();
    assert_eq!(
        typed_edit(&typed_small, &request, &tight16)
            .err()
            .expect("refused"),
        refusal
    );

    // Create Texture over pixels the sampled picture holds no colour for.
    let doc = doctored_core::doc::DrDocument::from_pixels(image::RgbaImage::from_fn(
        size.0,
        size.1,
        |x, y| {
            let p = field[(y * size.0 + x) as usize];
            let alpha = if cover[(y * size.0 + x) as usize] {
                0
            } else {
                255
            };
            image::Rgba([p[0], p[1], p[2], alpha])
        },
    ));
    let handle = Box::into_raw(Box::new(doc));
    let control = unsafe { dr_job_control_create() };
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_spot_heal_layer_job(
            handle,
            0,
            overlay.as_ptr(),
            size.0,
            size.1,
            1.0,
            0,
            0,
            1,
            0,
            false,
            control,
            &mut err,
        )
    };
    unsafe { dr_job_control_free(control) };
    unsafe { dr_doc_free(handle) };
    let (out, message) = take_doc(out, err);
    assert!(out.is_none());
    assert_eq!(
        message.as_deref(),
        Some(
            "Create Texture has nothing to build a texture from: the pixels under the stroke are \
             transparent in the sampled picture."
        )
    );
}
