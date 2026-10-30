//! #1 Lab phase 6a-i, type, shape and fill layers, paths and vector masks
//! on a Lab Color document (`lab_artwork`), through the FFI. The oracles are
//! the RGB rendering of the same components converted by Image ▸ Mode ▸ Lab
//! Color's own export, and the hand-computed CIE L\*a\*b\* of an sRGB colour
//! (`lab_fixture::srgb_to_lab`). Bounds, declared before measuring:
//!
//! * (a) a coverage polygon of an sRGB colour rendered (`dr_artwork_render`)
//!   on the twin and published on a 16-bit Lab document equals the same
//!   components rendered in sRGB, published on a 16-bit sRGB document and
//!   converted by `dr_precision_doc_convert_to_lab` `{"depth":16}`: alpha
//!   exact, and wherever alpha is above 0 each of L, a and b within 2 codes
//!   (a fully transparent pixel has no colour: the twin's render stores 0,
//!   the conversion converts black); an intrinsic RGBA8 glyph component the
//!   same;
//! * (b) published on an 8-bit Lab document every sample and alpha is on
//!   the ×257 grid and within 257 codes of (a)'s; republishing the same
//!   image answers no change (NULL, no error);
//! * (c) a gradient fill #ff0000 @0.25 → #0000ff @0.75, angle 0, the default
//!   Classic ramp and 50% midpoints, on a 64 × 1 raster: columns 0…12 within
//!   0.05 L\*a\*b\* units of `srgb_to_lab` of red and columns 52…63 of blue
//!   (both well inside the flat ends, t ≤ 0.2 and t ≥ 0.8); column 32 the
//!   mean of columns 0's and 63's codes within 1 code. A linear gradient
//!   samples a pixel's top-left corner (`style_gradient::GradientSampler::
//!   t`), so at angle 0 column `x` reads t = x / w: column `w / 2` of an EVEN
//!   width is t = 0.5, segment fraction 0.5, where the two-stop ramp is the
//!   mean;
//! * (d) a pattern fill of a custom 2 × 1 red/blue tile: each pixel within
//!   (0.6 L\*, 1.0 a\*/b\*) of `srgb_to_lab` of its texel (the 8-bit Lab
//!   bound of `lab_pattern_stroke.rs`), not the texel's sRGB numbers;
//! * (e) RGB → Lab of an sRGB document holding a described shape layer: the
//!   plan has no refusal and no flatten, the layer (name, id) is kept as
//!   pixels and `dr_precision_doc_artwork_description` is null;
//! * (f) a 16-bit Lab document with a shape layer, a path and a vector mask
//!   round-trips `.rz` (description, path, vector mask and samples equal);
//!   16 → 8 Bits/Channel keeps the description, its samples on the grid;
//! * (g) `dr_precision_doc_psd_export_report` of a Lab document with a solid
//!   or gradient fill layer has no refusal and lists the described-layer
//!   loss, and `dr_precision_doc_save_psd` succeeds within the report's
//!   `worst_case_bytes`, at 8 and 16 bits.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{
    dr_artwork_path_coverage, dr_artwork_render, dr_precision_doc_artwork,
    dr_precision_doc_artwork_description,
};
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_to_lab, dr_precision_doc_convert_to_lab_plan,
    dr_precision_doc_lab_depth_edit, dr_precision_doc_new_lab,
};
use doctored_core::ffi_path::{dr_precision_doc_paths_json, dr_precision_doc_with_paths_json};
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_open, dr_precision_doc_save_native,
    dr_precision_image_free,
};
use doctored_core::ffi_psd::{dr_precision_doc_psd_export_report, dr_precision_doc_save_psd};
use doctored_core::ffi_vector_mask::{
    dr_precision_doc_set_vector_mask, dr_precision_doc_vector_mask_json,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_ops::{free, plan_json, LIMIT};
use crate::lab_fixture::{decode, srgb_to_lab};

const W: u32 = 16;
const H: u32 = 12;
const SHAPE: &str = r#"{"type":"shape","version":4,"artwork":{"ink":"srgb"}}"#;
const TEXT: &str = r#"{"type":"text","version":3,"payload":"A"}"#;
/// (a)'s colour bound, in codes.
const RENDER_CODES: i64 = 2;
/// (c)'s flat-end bound, in L\*a\*b\* units.
const STOP_UNITS: f64 = 0.05;
/// (d)'s bound: L\*, then a\* and b\*.
const LAB_BOUND: (f64, f64) = (0.6, 1.0);
const RED: [f64; 3] = [1.0, 0.0, 0.0];
const BLUE: [f64; 3] = [0.0, 0.0, 1.0];

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// A new `w` × `h` white Lab document at `bits` (File ▸ New ▸ Lab Color).
fn new_lab(w: u32, h: u32, bits: u32) -> *mut PrecisionDocument {
    let request = CString::new(json!({"depth":bits,"fill_srgb":[1,1,1,1]}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(w, h, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

/// A white 16-bit sRGB document.
fn new_srgb(w: u32, h: u32) -> *mut PrecisionDocument {
    let pixels = vec![[65535u16; 4]; (w * h) as usize];
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ))
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    plan_json(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err).unwrap()
}

/// An anti-aliased quadrilateral's coverage over `W × H`.
fn polygon() -> Vec<u16> {
    let commands = [0u8, 1, 1, 1, 4];
    let points: [[f64; 2]; 4] = [[2.3, 1.7], [13.6, 3.2], [10.4, 9.8], [3.1, 8.4]];
    let mut out = vec![0u16; (W * H) as usize];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_artwork_path_coverage(
            commands.as_ptr(),
            commands.len(),
            points.as_ptr().cast(),
            points.len(),
            W,
            H,
            false,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    out
}

/// The one renderer's image of `components` in `doc`'s own profile.
fn render(
    doc: *const PrecisionDocument,
    (w, h): (u32, u32),
    components: &Value,
    samples: &[u16],
    intrinsic: &[u8],
) -> *mut PrecisionImage {
    let description = CString::new(components.to_string()).unwrap();
    let profile = unsafe { &*doc }.profile().clone();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_artwork_render(
            w,
            h,
            description.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            if intrinsic.is_empty() {
                ptr::null()
            } else {
                intrinsic.as_ptr()
            },
            intrinsic.len(),
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "render: {}", take_err_string(err));
    out
}

/// `image` published at (0, 0): a new top record, or over `replacing`.
fn artwork(
    doc: *const PrecisionDocument,
    replacing: Option<usize>,
    image: *const PrecisionImage,
    description: &str,
) -> Result<Option<*mut PrecisionDocument>, String> {
    let name = CString::new("Shape").unwrap();
    let description = CString::new(description).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork(
            doc,
            replacing.unwrap_or(0),
            replacing.is_some(),
            image,
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
    if !out.is_null() {
        Ok(Some(out))
    } else if err.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(err))
    }
}

fn publish(
    doc: *const PrecisionDocument,
    image: *const PrecisionImage,
    description: &str,
) -> *mut PrecisionDocument {
    artwork(doc, None, image, description)
        .unwrap_or_else(|e| panic!("publish: {e}"))
        .expect("a new layer changes the document")
}

fn top(doc: *const PrecisionDocument) -> usize {
    unsafe { &*doc }.layer_count() - 1
}

fn layer16(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn description(doc: *const PrecisionDocument, index: usize) -> Option<String> {
    owned(unsafe { dr_precision_doc_artwork_description(doc, index) })
}

/// Image ▸ Mode ▸ Lab Color at `bits`.
fn to_lab(doc: *const PrecisionDocument, bits: u32) -> *mut PrecisionDocument {
    let options = CString::new(json!({"depth":bits}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_lab(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!out.is_null(), "to lab: {}", take_err_string(err));
    out
}

/// (a)'s comparison: alpha exact, colour within [`RENDER_CODES`] where
/// alpha is above 0.
fn assert_render_equal(got: &[[u16; 4]], want: &[[u16; 4]], what: &str) {
    assert_eq!(got.len(), want.len());
    let mut covered = 0;
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert_eq!(g[3], w[3], "{what} pixel {i}: alpha");
        if g[3] == 0 {
            continue;
        }
        covered += 1;
        for c in 0..3 {
            assert!(
                (i64::from(g[c]) - i64::from(w[c])).abs() <= RENDER_CODES,
                "{what} pixel {i} channel {c}: {g:?} against {w:?}"
            );
        }
    }
    assert!(covered > 0, "{what}: nothing covered");
}

/// A `W × H` colour glyph: varied colours, some partly and some fully
/// transparent.
fn glyph() -> Vec<u8> {
    (0..W * H)
        .flat_map(|i| {
            let alpha = match i % 9 {
                0 => 0,
                4 => 120,
                _ => 255,
            };
            [
                (i * 37 % 256) as u8,
                (i * 91 + 60) as u8,
                (i * 53 + 140) as u8,
                alpha,
            ]
        })
        .collect()
}

#[test]
fn a_shape_and_a_glyph_on_the_twin_equal_the_rgb_render_converted_by_the_mode_rule() {
    let coverage = polygon();
    let shape = json!([{"kind":"coverage","color":[0.9,0.3,0.1,1.0]}]);
    let intrinsic = glyph();
    let text = json!([{"kind":"intrinsic_rgba8","opacity":1}]);
    for (what, components, samples, bytes, kind) in [
        ("shape", &shape, &coverage[..], &[][..], SHAPE),
        ("glyph", &text, &[][..], &intrinsic[..], TEXT),
    ] {
        let lab = new_lab(W, H, 16);
        let rgb = new_srgb(W, H);
        let on_twin = render(lab, (W, H), components, samples, bytes);
        let in_srgb = render(rgb, (W, H), components, samples, bytes);
        let l = publish(lab, on_twin, kind);
        let r = publish(rgb, in_srgb, kind);
        let converted = to_lab(r, 16);
        assert_eq!(description(l, top(l)).as_deref(), Some(kind));
        assert_render_equal(
            &layer16(l, top(l)),
            &layer16(converted, top(converted)),
            what,
        );

        // (b) the 8-bit document's grid, and a no-op republication.
        let eight = new_lab(W, H, 8);
        let e = publish(eight, on_twin, kind);
        let sixteen = layer16(l, top(l));
        let mut off_grid = false;
        for (i, (p, q)) in layer16(e, top(e)).iter().zip(&sixteen).enumerate() {
            off_grid |= q.iter().any(|v| v % 257 != 0);
            assert!(p.iter().all(|v| v % 257 == 0), "{what} pixel {i}: {p:?}");
            for c in 0..4 {
                assert!(
                    (i64::from(p[c]) - i64::from(q[c])).abs() <= 257,
                    "{what} pixel {i}: {p:?} against {q:?}"
                );
            }
        }
        assert!(off_grid, "{what}: the 16-bit render is off the grid");
        assert_eq!(
            artwork(e, Some(top(e)), on_twin, kind).unwrap(),
            None,
            "{what}: republishing the same image changes nothing"
        );
        for image in [on_twin, in_srgb] {
            unsafe { dr_precision_image_free(image) };
        }
        for d in [e, eight, converted, r, l, rgb, lab] {
            free(d);
        }
    }
}

#[test]
fn a_gradient_fill_reaches_each_stops_lab_and_interpolates_its_codes() {
    let (w, h) = (64u32, 1u32);
    let lab = new_lab(w, h, 16);
    let gradient = json!({"stops":[
            {"position":0.25,"color":"#ff0000","opacity":1},
            {"position":0.75,"color":"#0000ff","opacity":1}],
        "style":"linear","angle":0,"scale":1,"reverse":false,"align_with_layer":false});
    let components = json!([{"kind":"gradient","gradient":gradient,"colors":[RED,BLUE]}]);
    let image = render(lab, (w, h), &components, &[], &[]);
    let fill = json!({"type":"fill","version":1,"fill":"gradient","gradient":gradient,
        "canvas":[w,h]})
    .to_string();
    let out = publish(lab, image, &fill);
    let pixels = layer16(out, top(out));
    for (columns, stop) in [(0..=12usize, RED), (52..=63, BLUE)] {
        let want = srgb_to_lab(stop);
        for x in columns {
            let got = decode(pixels[x]);
            assert_eq!(pixels[x][3], 65535, "column {x}: alpha");
            for c in 0..3 {
                assert!(
                    (got[c] - want[c]).abs() <= STOP_UNITS,
                    "column {x} channel {c}: {got:?} against {want:?}"
                );
            }
        }
    }
    let (first, last, middle) = (pixels[0], pixels[63], pixels[32]);
    for c in 0..3 {
        let mean = (f64::from(first[c]) + f64::from(last[c])) / 2.0;
        assert!(
            (f64::from(middle[c]) - mean).abs() <= 1.0,
            "column 32 channel {c}: {middle:?} against the mean of {first:?} and {last:?}"
        );
    }
    unsafe { dr_precision_image_free(image) };
    free(out);
    free(lab);
}

/// Standard base64 (RFC 4648, padded), written here.
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

#[test]
fn a_pattern_fill_converts_each_texel_from_srgb() {
    let (w, h) = (6u32, 2u32);
    let lab = new_lab(w, h, 16);
    let tile = [255u8, 0, 0, 255, 0, 0, 255, 255];
    let fill = json!({"pattern":{"name":"T","width":2,"height":1,"rgba":base64(&tile)},
        "angle":0,"scale":1,"link_with_layer":false,"phase":[0,0]});
    let components = json!([{"kind":"pattern","fill":fill}]);
    let image = render(lab, (w, h), &components, &[], &[]);
    let out = publish(
        lab,
        image,
        r#"{"type":"fill","version":1,"fill":"pattern"}"#,
    );
    let pixels = layer16(out, top(out));
    for (i, p) in pixels.iter().enumerate() {
        let texel = if i.is_multiple_of(2) { RED } else { BLUE };
        let want = srgb_to_lab(texel);
        let got = decode(*p);
        let bounds = [LAB_BOUND.0, LAB_BOUND.1, LAB_BOUND.1];
        for c in 0..3 {
            assert!(
                (got[c] - want[c]).abs() <= bounds[c],
                "pixel {i} channel {c}: {got:?} against {want:?}"
            );
        }
        assert_eq!(p[3], 65535, "pixel {i}: alpha");
        let numbers = texel.map(|v| (v * 65535.0) as u16);
        assert_ne!(
            &p[..3],
            &numbers[..],
            "pixel {i} holds the texel's sRGB numbers"
        );
    }
    unsafe { dr_precision_image_free(image) };
    free(out);
    free(lab);
}

#[test]
fn rgb_to_lab_keeps_a_shape_layer_as_its_pixels() {
    let rgb = new_srgb(W, H);
    let components = json!([{"kind":"coverage","color":[0.2,0.6,0.9,1.0]}]);
    let image = render(rgb, (W, H), &components, &polygon(), &[]);
    let shaped = publish(rgb, image, SHAPE);
    let index = top(shaped);
    let id = unsafe { &*shaped }.layer_id(index);
    let options = CString::new(r#"{"depth":16}"#).unwrap();
    let mut err = ptr::null_mut();
    let plan = plan_json(
        unsafe { dr_precision_doc_convert_to_lab_plan(shaped, options.as_ptr(), &mut err) },
        err,
    )
    .unwrap();
    assert!(plan["refusal"].is_null(), "{plan}");
    assert_eq!(plan["flatten_required"], false, "{plan}");
    let lab = to_lab(shaped, 16);
    let l = unsafe { &*lab };
    assert_eq!(descriptor(lab)["color_mode"], "lab");
    assert_eq!(l.layer_count(), 2);
    assert_eq!(l.layer(index).unwrap().name(), "Shape");
    assert_eq!(l.layer_id(index), id);
    assert_eq!(description(lab, index), None);
    unsafe { dr_precision_image_free(image) };
    for d in [lab, shaped, rgb] {
        free(d);
    }
}

/// The paths JSON with each path's session id dropped (an open assigns
/// fresh ones).
fn paths(doc: *const PrecisionDocument) -> Option<Value> {
    let text = owned(unsafe { dr_precision_doc_paths_json(doc) })?;
    let mut value: Value = serde_json::from_str(&text).unwrap();
    for path in value["paths"].as_array_mut()? {
        path.as_object_mut()?.remove("id");
    }
    Some(value)
}

fn vector_mask(doc: *const PrecisionDocument, index: usize) -> Option<String> {
    owned(unsafe { dr_precision_doc_vector_mask_json(doc, index) })
}

#[test]
fn an_rz_round_trip_and_8_bits_keep_the_shape_its_path_and_its_vector_mask() {
    let dir = TempDir::new().unwrap();
    let quad = json!({"fill_rule":"nonzero","subpaths":[{"closed":true,
        "anchors":[[2.5,2.0],[12.0,2.5],[13.5,10.5],[3.0,9.0]]}]});
    let lab = new_lab(W, H, 16);
    let components = json!([{"kind":"coverage","color":[0.2,0.6,0.9,1.0]}]);
    let image = render(lab, (W, H), &components, &polygon(), &[]);
    let shaped = publish(lab, image, SHAPE);
    let index = top(shaped);
    let mut path = quad.clone();
    path["id"] = json!(0);
    path["name"] = json!("Outline");
    path["kind"] = json!("saved");
    let text = CString::new(json!({"paths":[path]}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let pathed = unsafe { dr_precision_doc_with_paths_json(shaped, text.as_ptr(), &mut err) };
    assert!(!pathed.is_null(), "paths: {}", take_err_string(err));
    let shape = CString::new(quad.to_string()).unwrap();
    let masked = unsafe {
        dr_precision_doc_set_vector_mask(
            pathed,
            index,
            shape.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!masked.is_null(), "vector mask: {}", take_err_string(err));
    let file = dir.path().join("artwork.rz");
    let c = cpath(&file);
    assert!(
        unsafe {
            dr_precision_doc_save_native(masked, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let back = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!back.is_null(), "{}", take_err_string(err));
    assert_eq!(descriptor(back)["color_mode"], "lab");
    assert_eq!(description(back, index).as_deref(), Some(SHAPE));
    assert!(paths(back).is_some());
    assert_eq!(paths(back), paths(masked));
    assert!(vector_mask(back, index).is_some());
    assert_eq!(vector_mask(back, index), vector_mask(masked, index));
    assert_eq!(layer16(back, index), layer16(masked, index));

    let eight =
        unsafe { dr_precision_doc_lab_depth_edit(back, 8, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!eight.is_null(), "16 → 8: {:?}", owned(err));
    assert_eq!(descriptor(eight)["lab"]["depth"], 8);
    assert_eq!(description(eight, index).as_deref(), Some(SHAPE));
    assert!(layer16(eight, index)
        .iter()
        .all(|p| p.iter().all(|v| v % 257 == 0)));
    unsafe { dr_precision_image_free(image) };
    for d in [eight, back, masked, pathed, shaped, lab] {
        free(d);
    }
}

#[test]
fn a_lab_solid_fill_exports_to_psd_as_its_pixels_with_the_loss_reported() {
    let dir = TempDir::new().unwrap();
    let gradient = json!({"stops":[
            {"position":0.25,"color":"#ff0000","opacity":1},
            {"position":0.75,"color":"#0000ff","opacity":1}],
        "style":"linear","angle":0,"scale":1,"reverse":false,"align_with_layer":false});
    for (bits, kind) in [
        (8u32, "solid"),
        (16, "solid"),
        (8, "gradient"),
        (16, "gradient"),
    ] {
        let lab = new_lab(W, H, bits);
        let (components, fill) = if kind == "solid" {
            (
                json!([{"kind":"fill","color":[0.25,0.5,0.75,1.0]}]),
                json!({"type":"fill","version":1,"fill":"solid",
                    "color":[0.25,0.5,0.75,1.0],"canvas":[W,H]}),
            )
        } else {
            (
                json!([{"kind":"gradient","gradient":gradient,"colors":[RED,BLUE]}]),
                json!({"type":"fill","version":1,"fill":"gradient","gradient":gradient,
                    "canvas":[W,H]}),
            )
        };
        let bits_kind = format!("{bits} {kind}");
        let image = render(lab, (W, H), &components, &[], &[]);
        let out = publish(lab, image, &fill.to_string());
        let mut err = ptr::null_mut();
        let report = owned(unsafe { dr_precision_doc_psd_export_report(out, 0, 0, &mut err) })
            .unwrap_or_else(|| panic!("report: {}", take_err_string(err)));
        let report: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(report["available"], true, "{bits_kind}: {report}");
        assert!(
            report["losses"]
                .to_string()
                .contains("has a re-editable description"),
            "{bits_kind}: {report}"
        );
        let projection = unsafe { &*out }.render(LIMIT, 0, None).unwrap();
        let path = dir.path().join(format!("fill{bits}{kind}.psd"));
        let ok = unsafe {
            dr_precision_doc_save_psd(
                out,
                &projection,
                cpath(&path).as_ptr(),
                0,
                0,
                LIMIT,
                0,
                &mut err,
            )
        };
        assert!(ok, "{bits_kind}: {}", take_err_string(err));
        // A Lab fill is written as its pixels, never as a fill record, and
        // the preflight's worst case still bounds the file.
        let written = std::fs::metadata(&path).unwrap().len();
        assert!(
            written <= report["worst_case_bytes"].as_u64().unwrap(),
            "{bits_kind}: {written} bytes written: {report}"
        );
        unsafe { dr_precision_image_free(image) };
        free(out);
        free(lab);
    }
}
