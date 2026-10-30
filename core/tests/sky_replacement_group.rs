//! Sky Replacement's "New Layers" output (M2b) through the FFI:
//! `dr_doc_sky_replace` / `dr_precision_doc_sky_replace` with
//! `"output": "new_layers"`, and the plan `dr_sky_replace_scratch_bytes`
//! for each output, at 8 and 16 bits.
//!
//! # The oracles
//!
//! * Structure: the group and its three children as `sky_group`'s module doc
//!   lists them, read straight off the result.
//! * Composite: the New Layers document's projection against the Duplicate
//!   Layer document's projection from the same inputs — the invariant the
//!   group exists to keep (the Duplicate pixel is checked against its closed
//!   form in `sky_replacement.rs`).
//! * Round trips: `.rz` and PSD save and reopen against the document saved.
//!
//! # Bounds, declared before the first run
//!
//! * Composite: ±3 codes at 8 bits, ±6 codes at 16 bits (the curves, the
//!   mask and the sky are each quantized once, where the Duplicate pixel is
//!   quantized once in all).
//! * `.rz`: structure and curve params identical, mask samples exact.
//! * PSD: structure identical, curve points within 0.5, sky pixels and mask
//!   samples exact.
#![allow(clippy::needless_range_loop)]

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument, LayerKind};
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_doc::{dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_save_native,
};
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_save_psd, dr_precision_doc_open_psd, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::ffi_sky::{dr_precision_doc_sky_replace, dr_sky_replace_scratch_bytes};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common::{cpath, flat_pixels, take_err_string};
use crate::float_fixture::float_doc;
use crate::sky_replacement::{
    binary_coverage, build, layer_count, params, replace, sky_handle, two_band, typed_layer,
    uniform_sky, Depth, Doc, FORE, H, HORIZON, LIMIT, OLD, SKY, W,
};

const GROUP: &str = "Sky Replacement Group";
const NAMES: [&str; 3] = ["Foreground Color", "Foreground Lighting", "Sky"];
const PSD_LAYERS: i32 = 0;

pub(crate) fn new_layers(p: Value) -> Value {
    let mut v = p;
    v["output"] = json!("new_layers");
    v
}

pub(crate) fn duplicate(p: Value) -> Value {
    let mut v = p;
    v["output"] = json!("duplicate_layer");
    v
}

fn typed(doc: &Doc) -> &PrecisionDocument {
    match doc {
        Doc::Typed(d) => d,
        Doc::Byte(_) => panic!("a typed document"),
    }
}

fn byte(doc: &Doc) -> &DrDocument {
    match doc {
        Doc::Byte(d) => d,
        Doc::Typed(_) => panic!("a byte document"),
    }
}

/// One entry's facts, the same at both depths: name, kind, blend mode,
/// opacity, clipped, the mask's samples over 65535 (None without a mask) and
/// the adjustment's curve params.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) kind: &'static str,
    pub(crate) blend: BlendMode,
    pub(crate) opacity: f32,
    pub(crate) clipped: bool,
    pub(crate) mask: Option<Vec<u16>>,
    pub(crate) curves: Option<Value>,
}

fn curves_of(meta: Option<&str>) -> Option<Value> {
    let value: Value = serde_json::from_str(meta?).ok()?;
    assert_eq!(value["type"], "adjust");
    assert_eq!(value["op"], "curves");
    let params = &value["params"];
    Some(json!({"r": params["r"], "g": params["g"], "b": params["b"]}))
}

pub(crate) fn entries(doc: &Doc) -> Vec<Entry> {
    match doc {
        Doc::Byte(d) => d
            .layers
            .iter()
            .map(|l| Entry {
                name: l.name.clone(),
                kind: if l.kind == LayerKind::Group {
                    "group"
                } else if l.meta.is_some() {
                    "adjustment"
                } else {
                    "raster"
                },
                blend: l.blend,
                opacity: l.opacity,
                clipped: l.clipped,
                mask: l
                    .mask
                    .as_ref()
                    .map(|m| m.as_raw().iter().map(|&v| u16::from(v) * 257).collect()),
                curves: if l.kind == LayerKind::Group {
                    None
                } else {
                    curves_of(l.meta.as_deref())
                },
            })
            .collect(),
        Doc::Typed(d) => (0..d.layer_count())
            .map(|i| {
                let l = d.layer(i).unwrap();
                Entry {
                    name: l.name().to_string(),
                    kind: if l.children().is_some() {
                        "group"
                    } else if d.adjustment_description(i).is_some() {
                        "adjustment"
                    } else {
                        "raster"
                    },
                    blend: l.blend_mode(),
                    opacity: l.opacity(),
                    clipped: l.clipped(),
                    mask: l.mask().map(|m| m.plane().samples().to_vec()),
                    curves: curves_of(d.adjustment_description(i)),
                }
            })
            .collect(),
    }
}

/// The group at `at` (its children at `at - 3 .. at`): names, kinds, modes,
/// opacities, masks and clip flags as `p` asks for.
fn assert_group(doc: &Doc, at: usize, p: &Value) {
    let all = entries(doc);
    let n = (W * H) as usize;
    let group = &all[at];
    assert_eq!(group.name, GROUP);
    assert_eq!(group.kind, "group");
    assert_eq!(group.blend, BlendMode::PassThrough);
    assert_eq!(group.opacity, 1.0);
    assert!(!group.clipped && group.mask.is_none());
    let children = &all[at - 3..at];
    for (child, name) in children.iter().zip(NAMES) {
        assert_eq!(child.name, name);
        assert!(!child.clipped, "{name} is unclipped");
        assert_eq!(child.mask.as_ref().map(Vec::len), Some(n), "{name} mask");
    }
    let lighting = p["lighting"].as_f64().unwrap_or(50.0) as f32 / 100.0;
    let color = p["color_adjustment"].as_f64().unwrap_or(50.0) as f32 / 100.0;
    let mode = if p["lighting_mode"] == "screen" {
        BlendMode::Screen
    } else {
        BlendMode::Multiply
    };
    assert_eq!(
        (children[0].kind, children[0].blend, children[0].opacity),
        ("adjustment", BlendMode::Normal, color)
    );
    assert_eq!(
        (children[1].kind, children[1].blend, children[1].opacity),
        ("adjustment", mode, lighting)
    );
    assert_eq!(
        (children[2].kind, children[2].blend, children[2].opacity),
        ("raster", BlendMode::Normal, 1.0)
    );
    for child in &children[..2] {
        assert!(child.mask.as_ref().unwrap().iter().all(|&v| v == 65535));
        assert!(child.curves.is_some());
    }
    // The lighting curve is flat at the sky's mean.
    let flat = &children[1].curves.as_ref().unwrap();
    for c in ["r", "g", "b"] {
        assert_eq!(flat[c][0][1], flat[c][1][1], "lighting {c} is flat");
    }
    // The sky's mask covers the sky and spares the foreground's bottom row.
    let mask = children[2].mask.as_ref().unwrap();
    assert_eq!(mask[0], 65535);
    assert_eq!(mask[n - 1], 0);
    match doc {
        Doc::Byte(d) => {
            let (g, sky) = (&d.layers[at], &d.layers[at - 1]);
            assert!(g.open);
            assert_eq!(sky.offset, (0, 0));
            assert_eq!(sky.pixels.dimensions(), (W, H));
            for k in 1..=3 {
                assert_eq!(d.layers[at - k].depth, g.depth + 1);
            }
        }
        Doc::Typed(d) => {
            let g = d.layer(at).unwrap();
            assert!(g.open() && g.is_pass_through());
            assert_eq!(g.children().unwrap().len(), 3);
            let sky = d.layer(at - 1).unwrap();
            assert_eq!(sky.offset(), (0, 0));
            assert_eq!(sky.source().unwrap().dimensions(), (W, H));
        }
    }
}

// --------------------------------------------------------------- structure --

#[test]
fn new_layers_builds_the_sky_replacement_group_above_the_source() {
    let cov = binary_coverage(W, H);
    for depth in [Depth::Byte, Depth::Sixteen] {
        for mode in ["multiply", "screen"] {
            let p = json!({"lighting_mode": mode, "lighting": 40, "color_adjustment": 70});
            let doc = build(depth, W, H, &[two_band(W, H, OLD, FORE)]);
            let out = replace(&doc, 0, &uniform_sky(SKY), &cov, &new_layers(p.clone())).unwrap();
            assert_eq!(layer_count(&out), 5, "{depth:?} {mode}");
            assert_group(&out, 4, &p);
            assert_eq!(
                entries(&out)[0],
                entries(&doc)[0],
                "the source is untouched"
            );
        }
        // A name names the group.
        let doc = build(depth, W, H, &[two_band(W, H, OLD, FORE)]);
        let named = new_layers(json!({"name": "Dusk"}));
        let out = replace(&doc, 0, &uniform_sky(SKY), &cov, &named).unwrap();
        assert_eq!(entries(&out)[4].name, "Dusk");
    }
}

#[test]
fn the_group_lands_above_the_clip_run_and_is_never_clipped() {
    let cov = binary_coverage(W, H);
    let layers = [two_band(W, H, OLD, FORE), two_band(W, H, FORE, OLD)];
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = match build(depth, W, H, &layers) {
            Doc::Byte(mut d) => {
                d.layers[1].clipped = true;
                Doc::Byte(d)
            }
            Doc::Typed(_) => Doc::Typed(
                PrecisionDocument::from_parts(
                    W,
                    H,
                    IccProfile::srgb(),
                    Metadata::default(),
                    Resolution::default(),
                    vec![
                        typed_layer("L0", W, H, &layers[0], (0, 0)),
                        typed_layer("L1", W, H, &layers[1], (0, 0)).with_clipped(true),
                    ],
                )
                .unwrap(),
            ),
        };
        let p = json!({});
        let out = replace(&doc, 0, &uniform_sky(SKY), &cov, &new_layers(p.clone())).unwrap();
        assert_eq!(layer_count(&out), 6, "{depth:?}");
        let all = entries(&out);
        assert!(all[1].clipped, "the clip run keeps its member");
        assert_group(&out, 5, &p);
    }
}

// --------------------------------------------------------------- composite --

/// An opaque scene: the old sky above the horizon; below it mid-tones, a
/// near-white and a near-black band, in thirds across.
fn scene() -> Vec<[f64; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let c = if y < HORIZON {
                OLD
            } else if x < W / 3 {
                FORE
            } else if x < 2 * W / 3 {
                [0.97, 0.96, 0.98]
            } else {
                [0.03, 0.04, 0.02]
            };
            [c[0], c[1], c[2], 1.0]
        })
        .collect()
}

/// A vertical gradient of `top` to `bottom`, so the placed sky varies.
pub(crate) fn gradient_sky(top: [u8; 3], bottom: [u8; 3]) -> RgbaImage {
    RgbaImage::from_fn(32, 16, |_, y| {
        let t = f64::from(y) / 15.0;
        let c: [u8; 3] = std::array::from_fn(|k| {
            (f64::from(top[k]) * (1.0 - t) + f64::from(bottom[k]) * t).round() as u8
        });
        Rgba([c[0], c[1], c[2], 255])
    })
}

/// The projection as unit samples.
pub(crate) fn projection(doc: &Doc) -> Vec<f64> {
    match doc {
        Doc::Byte(d) => flat_pixels(d as *const DrDocument)
            .iter()
            .map(|&v| f64::from(v) / 255.0)
            .collect(),
        Doc::Typed(d) => d
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap()
            .iter()
            .flat_map(|p| p.map(|v| f64::from(v) / 65535.0))
            .collect(),
    }
}

#[test]
fn the_group_composites_to_the_duplicate_layer_output() {
    let cov = binary_coverage(W, H);
    let skies = [
        ("warm", gradient_sky([250, 150, 60], [255, 200, 120])),
        ("cool", gradient_sky([30, 70, 200], [90, 140, 230])),
    ];
    for depth in [Depth::Byte, Depth::Sixteen] {
        let bound = match depth {
            Depth::Byte => 3.0,
            Depth::Sixteen => 6.0,
        };
        let doc = build(depth, W, H, &[scene()]);
        for (what, sky) in &skies {
            for mode in ["multiply", "screen"] {
                let p = json!({"fade_edge": 3, "lighting": 50, "color_adjustment": 100,
                               "edge_lighting": 50, "lighting_mode": mode});
                let flat = projection(&replace(&doc, 0, sky, &cov, &duplicate(p.clone())).unwrap());
                let grouped = projection(&replace(&doc, 0, sky, &cov, &new_layers(p)).unwrap());
                let worst = flat
                    .iter()
                    .zip(&grouped)
                    .map(|(a, b)| (a - b).abs() * depth.max())
                    .fold(0.0, f64::max);
                assert!(
                    worst <= bound + 1e-6,
                    "{depth:?} {what} {mode}: {worst} codes past {bound}"
                );
            }
        }
    }
}

// ------------------------------------------------------------- round trips --

fn grouped(depth: Depth) -> Doc {
    let doc = build(depth, W, H, &[scene()]);
    let p = json!({"lighting_mode": "screen", "lighting": 60, "color_adjustment": 80,
                   "edge_lighting": 50});
    replace(
        &doc,
        0,
        &gradient_sky([250, 150, 60], [255, 200, 120]),
        &binary_coverage(W, H),
        &new_layers(p),
    )
    .unwrap()
}

pub(crate) fn save_rz(doc: &Doc, path: &Path) -> Doc {
    let target = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    match doc {
        Doc::Byte(d) => {
            assert!(
                unsafe { dr_doc_save_native(d, target.as_ptr(), &mut err) },
                "{}",
                take_err_string(err)
            );
            let out = unsafe { dr_doc_open(target.as_ptr(), &mut err) };
            assert!(!out.is_null(), "{}", take_err_string(err));
            Doc::Byte(*unsafe { Box::from_raw(out) })
        }
        Doc::Typed(d) => {
            assert!(
                unsafe {
                    dr_precision_doc_save_native(
                        d,
                        target.as_ptr(),
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut err,
                    )
                },
                "{}",
                take_err_string(err)
            );
            let out =
                unsafe { dr_precision_doc_open(target.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
            assert!(!out.is_null(), "{}", take_err_string(err));
            Doc::Typed(*unsafe { Box::from_raw(out) })
        }
    }
}

pub(crate) fn sky_samples(doc: &Doc, at: usize) -> Vec<u16> {
    match doc {
        Doc::Byte(d) => crate::common::layer_bytes(d, at)
            .iter()
            .map(|&v| u16::from(v))
            .collect(),
        Doc::Typed(d) => d
            .layer(at)
            .unwrap()
            .source()
            .unwrap()
            .to_rgba16()
            .unwrap()
            .concat(),
    }
}

#[test]
fn the_group_survives_rz_save_and_reopen() {
    let dir = TempDir::new().unwrap();
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = grouped(depth);
        let back = save_rz(&doc, &dir.path().join(format!("{depth:?}.rz")));
        assert_eq!(entries(&back), entries(&doc), "{depth:?}");
        assert_eq!(sky_samples(&back, 3), sky_samples(&doc, 3));
    }
}

fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(raw)).expect("import report JSON")
}

pub(crate) fn save_psd(doc: &Doc, path: &Path) -> Doc {
    let target = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    match doc {
        Doc::Byte(d) => assert!(
            unsafe { dr_doc_save_psd(d, ptr::null(), target.as_ptr(), 0, PSD_LAYERS, &mut err) },
            "{}",
            take_err_string(err)
        ),
        Doc::Typed(d) => {
            let projection = d.render(LIMIT, 0, None).unwrap();
            assert!(
                unsafe {
                    dr_precision_doc_save_psd(
                        d,
                        &projection,
                        target.as_ptr(),
                        0,
                        PSD_LAYERS,
                        LIMIT,
                        0,
                        &mut err,
                    )
                },
                "{}",
                take_err_string(err)
            );
        }
    }
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    match doc {
        Doc::Byte(_) => {
            let out =
                unsafe { dr_doc_open_psd(target.as_ptr(), PSD_LAYERS, token.as_ptr(), &mut err) };
            assert!(!out.is_null(), "{}", take_err_string(err));
            Doc::Byte(*unsafe { Box::from_raw(out) })
        }
        Doc::Typed(_) => {
            let out = unsafe {
                dr_precision_doc_open_psd(
                    target.as_ptr(),
                    PSD_LAYERS,
                    token.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null::<DrJobControl>(),
                    &mut err,
                )
            };
            assert!(!out.is_null(), "{}", take_err_string(err));
            Doc::Typed(*unsafe { Box::from_raw(out) })
        }
    }
}

fn assert_points_close(got: &Value, want: &Value, what: &str) {
    for c in ["r", "g", "b"] {
        let (g, w) = (got[c].as_array().unwrap(), want[c].as_array().unwrap());
        assert_eq!(g.len(), w.len(), "{what} {c}");
        for (a, b) in g.iter().zip(w) {
            for k in 0..2 {
                let (a, b) = (a[k].as_f64().unwrap(), b[k].as_f64().unwrap());
                assert!((a - b).abs() <= 0.5, "{what} {c}: {a} vs {b}");
            }
        }
    }
}

#[test]
fn the_group_survives_psd_export_and_reopen() {
    let dir = TempDir::new().unwrap();
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = grouped(depth);
        let back = save_psd(&doc, &dir.path().join(format!("{depth:?}.psd")));
        let (before, after) = (entries(&doc), entries(&back));
        assert_eq!(after.len(), before.len(), "{depth:?}");
        for (a, b) in after.iter().zip(&before) {
            assert_eq!(
                (&a.name, a.kind, a.blend, a.clipped, &a.mask),
                (&b.name, b.kind, b.blend, b.clipped, &b.mask),
                "{depth:?} {}",
                b.name
            );
            assert!(
                (a.opacity - b.opacity).abs() <= 0.5 / 255.0,
                "{} opacity",
                b.name
            );
            match (&a.curves, &b.curves) {
                (Some(got), Some(want)) => assert_points_close(got, want, &b.name),
                (None, None) => {}
                _ => panic!("{depth:?} {}: curves differ", b.name),
            }
        }
        assert_eq!(sky_samples(&back, 3), sky_samples(&doc, 3));
    }
}

// --------------------------------------------------------------- refusals --

fn refused(doc: &Doc, layer: usize, coverage: &[u16], p: &Value) -> String {
    replace(doc, layer, &uniform_sky(SKY), coverage, p)
        .err()
        .expect("a refusal")
}

/// `doc` with layer 0 wrapped in `levels` nested groups.
fn nested(doc: &Doc, levels: usize) -> Doc {
    match doc {
        Doc::Byte(d) => {
            let mut d = d.clone();
            for _ in 0..levels {
                d = d.group_layers(&[0], "G").unwrap().0;
            }
            Doc::Byte(d)
        }
        Doc::Typed(d) => {
            let mut d = d.clone();
            for _ in 0..levels {
                d = d.group_layers(&[0], "G", false).unwrap().unwrap().0;
            }
            Doc::Typed(d)
        }
    }
}

#[test]
fn new_layers_refusals_name_their_reason() {
    let cov = binary_coverage(W, H);
    let nl = |v: Value| new_layers(v);
    for depth in [Depth::Byte, Depth::Sixteen] {
        let doc = build(depth, W, H, &[two_band(W, H, OLD, FORE)]);
        assert_eq!(
            refused(&doc, 0, &cov, &json!({"output": "overlay"})),
            "sky replacement output must be duplicate_layer or new_layers"
        );
        assert_eq!(
            refused(&doc, 0, &vec![0; cov.len()], &nl(json!({}))),
            "sky replacement found no sky on layer 0"
        );
        assert_eq!(
            refused(&doc, 3, &cov, &nl(json!({}))),
            "sky replacement layer 3 is out of range"
        );
        // A group source refuses before anything else.
        let grouped = nested(&doc, 1);
        assert_eq!(
            refused(&grouped, 1, &cov, &nl(json!({}))),
            "sky replacement needs a raster layer; layer 1 is not one"
        );
        // An adjustment source.
        let (adjusted, at) = match &doc {
            Doc::Byte(d) => {
                let mut a = d.clone();
                a.layers[0].meta = Some(r#"{"type":"adjust","op":"invert"}"#.into());
                (Doc::Byte(a), 0)
            }
            Doc::Typed(d) => {
                let description = json!({"type":"adjust","evaluation":"precision_v1",
                                         "op":"exposure","params":{"exposure":0.25}});
                let a = d
                    .add_adjustment_at(
                        "Adj",
                        &description.to_string(),
                        1.0,
                        0,
                        InsertPosition::Top,
                        LIMIT,
                        0,
                        None,
                    )
                    .unwrap()
                    .unwrap();
                (Doc::Typed(a), 1)
            }
        };
        assert_eq!(
            refused(&adjusted, at, &cov, &nl(json!({}))),
            format!("sky replacement needs a raster layer; layer {at} is not one")
        );
    }
    // A source at the depth's ceiling cannot hold its group's children.
    let byte_doc = build(Depth::Byte, W, H, &[two_band(W, H, OLD, FORE)]);
    let deep = nested(&byte_doc, 10);
    assert_eq!(byte(&deep).layers[0].depth, 10);
    assert_eq!(
        refused(&deep, 0, &cov, &new_layers(json!({}))),
        "sky replacement cannot nest its group that deep"
    );
    assert!(replace(&deep, 0, &uniform_sky(SKY), &cov, &duplicate(json!({}))).is_ok());
    let typed_doc = build(Depth::Sixteen, W, H, &[two_band(W, H, OLD, FORE)]);
    let deep = nested(&typed_doc, 16);
    assert_eq!(
        refused(&deep, 0, &cov, &new_layers(json!({}))),
        "sky replacement cannot nest its group that deep"
    );
    // A typed document within 3 records of the cap takes one layer, not four.
    let (w, h) = (8u32, 8u32);
    let small = binary_coverage(w, h);
    let band = two_band(w, h, OLD, FORE);
    let records = (0..1021)
        .map(|k| typed_layer(&format!("L{k}"), w, h, &band, (0, 0)))
        .collect();
    let full = Doc::Typed(
        PrecisionDocument::from_parts(
            w,
            h,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            records,
        )
        .unwrap(),
    );
    assert!(replace(&full, 0, &uniform_sky(SKY), &small, &duplicate(json!({}))).is_ok());
    assert_eq!(
        refused(&full, 0, &small, &new_layers(json!({}))),
        "too many high-depth document records"
    );
    // 32-bit and CMYK refuse by name before the output is read.
    let n = (W * H) as usize;
    let float = Doc::Typed(float_doc(
        W,
        H,
        &IccProfile::srgb(),
        &[vec![[0.5, 0.5, 0.5, 1.0]; n]],
    ));
    assert_eq!(
        refused(&float, 0, &cov, &new_layers(json!({}))),
        "sky replacement is not available in a 32-bit document"
    );
    let rgb = Box::into_raw(Box::new(typed(&typed_doc).clone()));
    let profile = press_profile("Press Sky Group");
    let mut err = ptr::null_mut();
    let cmyk = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!cmyk.is_null(), "{}", take_err_string(err));
    let sky = sky_handle(&uniform_sky(SKY));
    let text = params(&new_layers(json!({})));
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_sky_replace(
            cmyk,
            0,
            sky,
            cov.as_ptr(),
            cov.len(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        take_err_string(err),
        "sky replacement is not available in a CMYK document"
    );
    unsafe {
        dr_image_free(sky);
        dr_precision_doc_free(cmyk);
        dr_precision_doc_free(rgb);
    }
}

// ------------------------------------------------------------------- plan --

#[test]
fn each_output_plans_its_own_bytes() {
    let mib4 = 4 * 1024 * 1024;
    let plan = |bits, output| unsafe {
        dr_sky_replace_scratch_bytes(100, 50, 20, 10, bits, output, false)
    };
    assert_eq!(plan(8, 0), 5000 * 61 + 200 * 36 + mib4);
    assert_eq!(plan(16, 0), 5000 * 65 + 200 * 36 + mib4);
    assert_eq!(plan(8, 1), 5000 * 64 + 200 * 36 + mib4);
    assert_eq!(plan(16, 1), 5000 * 71 + 200 * 36 + mib4);
    assert_eq!(plan(32, 1), 0);
    assert_eq!(plan(8, 2), 0);
    assert_eq!(
        unsafe { dr_sky_replace_scratch_bytes(0, 50, 20, 10, 8, 1, false) },
        0
    );
    // The New Layers plan is the one the doc op admits against.
    let scratch = unsafe { dr_sky_replace_scratch_bytes(W, H, 32, 16, 8, 1, false) };
    let doc = build(Depth::Byte, W, H, &[two_band(W, H, OLD, FORE)]);
    let refused = crate::sky_replacement::replace_with(
        &doc,
        0,
        &uniform_sky(SKY),
        &binary_coverage(W, H),
        &new_layers(json!({})),
        scratch,
        ptr::null(),
    );
    assert!(refused.err().unwrap().contains("working limit"));
}
