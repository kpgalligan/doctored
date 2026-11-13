//! #1 Lab phase 4b-i: the selected pixels (Move and Transform Selected
//! Pixels, Puppet and Perspective Warp of a selection), Layer via Copy and
//! via Cut, Stamp Selected and the source selections (Save and Load
//! Selection) on a Lab Color document, through the FFI. Each is the RGB
//! kernel run on the L\*a\*b\* codes, so the oracle is `lab_geometry`'s RGB
//! TWIN (the identical `from_parts` document without the Lab policy).
//! Bounds, declared before measuring, at 8 and 16 bits on `pair()`:
//!
//! * (a) `move_selected` (dx/dy, with and without `duplicate`),
//!   `transform_selected` (a matrix, nearest and bilinear), `puppet_selected`,
//!   `perspective_selected`, `move_selected`/`transform_selected` on
//!   `"mask"` and `{"channel":0}`, `layer_via` copy and cut, and
//!   `stamp_selected` each equal the twin bit for bit as `lab_geometry` (a)
//!   defines it (and every alpha channel's samples too), all under one ramp
//!   selection, with every Puppet pin and Perspective quad corner inside it;
//! * (b) a hand oracle: an integer move of a hard rectangle selection on an
//!   opaque layer lands the original codes at (x + dx, y + dy) exactly, the
//!   hole has alpha 0 with its codes kept, and every other pixel is
//!   unchanged;
//! * (c) a neutral layer (a\* = b\* = 0, the 0x8080 code) keeps G = B =
//!   0x8080 on every pixel with alpha 65535 through (a)'s layer edits;
//! * (d) the `selection` request's `color` red, green and blue on a
//!   one-layer opaque Lab document equal that layer's stored codes, `alpha`
//!   equals a one-layer document's alpha, `luminance` answers
//!   `LUMINANCE_REFUSAL`, and the Magic Wand, Select Similar and Grow run at
//!   both depths (their oracle is `lab_wand.rs`'s; sampled Color Range,
//!   shipped in 4b-ii, is `lab_color_select.rs`'s; Quick Selection, shipped
//!   in 6b-ii, `lab_model_select.rs`'s);
//! * (e) since phase 6b-i `move_selected` and `transform_selected` with
//!   target red, green, blue (Lightness, a, b) and alpha, with and without a
//!   swatch, equal the same request on an sRGB-tagged RGB16 document holding
//!   the same samples bit for bit (the hole is the swatch's luma rule);
//! * (f) the descriptor lists the five 4b-i families.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit_plan, dr_precision_doc_select};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{edit, free, layer, LIMIT};
use crate::lab_geometry::{
    assert_twin, descriptor, document, pair, ramp, twin_profile, DEPTHS, H, NEUTRAL, W,
};

const LUMINANCE: &str =
    "A Lab Color document's luminosity is its Lightness channel: select from color red (the L* codes).";

/// The one ramp selection of (a): x 3..=15, y 2..=12, coverage rising
/// across x from 1/13 to full.
fn ramp_selection() -> Vec<u16> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (3..=15).contains(&x) && (2..=12).contains(&y) {
                ((x - 2) * 65535 / 13) as u16
            } else {
                0
            }
        })
        .collect()
}

/// A rotation by `degrees` and a scale about `centre`, as an affine matrix.
fn turn(degrees: f64, scale: f64, centre: (f64, f64)) -> [f64; 6] {
    let (s, c) = degrees.to_radians().sin_cos();
    let (a, b) = (c * scale, s * scale);
    [
        a,
        b,
        -b,
        a,
        centre.0 - a * centre.0 + b * centre.1,
        centre.1 - b * centre.0 - a * centre.1,
    ]
}

/// (a)'s layer edits, each under the ramp selection (pins and quad corners
/// inside x 3..=15, y 2..=12).
fn layer_edits() -> Vec<Value> {
    vec![
        json!({"op":"move_selected","layer":0,"dx":4,"dy":3}),
        json!({"op":"move_selected","layer":0,"dx":-3,"dy":2,"duplicate":true}),
        json!({"op":"transform_selected","layer":0,"matrix":turn(13.0, 1.1, (9.0, 7.0)),
            "filter":"nearest"}),
        json!({"op":"transform_selected","layer":0,"matrix":turn(13.0, 1.1, (9.0, 7.0)),
            "filter":"bilinear"}),
        json!({"op":"puppet_selected","layer":0,"pins":[
            {"from":[6.0,5.0],"to":[8.0,6.0]},{"from":[12.0,9.0],"to":[12.0,8.0]}]}),
        json!({"op":"perspective_selected","layer":0,"quads":[{
            "from":[4.0,3.0,14.0,3.0,14.0,11.0,4.0,11.0],
            "to":[5.0,4.0,15.0,2.0,14.0,12.0,3.0,10.0]}]}),
        json!({"op":"layer_via","layer":0,"cut":false,"name":"Via","selection":true}),
        json!({"op":"layer_via","layer":0,"cut":true,"name":"Via","selection":true}),
    ]
}

/// (a)'s plane edits: layer 1's mask and alpha channel 0.
fn plane_edits() -> Vec<Value> {
    let matrix = turn(-9.0, 0.9, (9.0, 7.0));
    vec![
        json!({"op":"move_selected","layer":1,"dx":2,"dy":-1,"target":"mask"}),
        json!({"op":"transform_selected","layer":1,"target":"mask","matrix":matrix,
            "filter":"bilinear"}),
        json!({"op":"move_selected","layer":1,"dx":-2,"dy":1,"target":{"channel":0}}),
        json!({"op":"transform_selected","layer":1,"target":{"channel":0},"matrix":matrix,
            "filter":"bilinear"}),
    ]
}

/// `doc` with an alpha channel of a diagonal ramp; frees `doc`.
fn with_channel(doc: *mut PrecisionDocument) -> *mut PrecisionDocument {
    let plane: Vec<u16> = (0..W * H)
        .map(|i| (((i % W) + (i / W)) * 65535 / (W + H - 2)) as u16)
        .collect();
    let request = json!({"op":"set_channel","index":null,"name":"Alpha 1"});
    let out = edit(doc, &request, &plane).unwrap().unwrap();
    free(doc);
    out
}

/// One edit under `coverage`; it must change the document.
fn must(
    doc: *const PrecisionDocument,
    request: &Value,
    coverage: &[u16],
    what: &str,
) -> *mut PrecisionDocument {
    edit(doc, request, coverage)
        .unwrap_or_else(|e| panic!("{what}: {e}"))
        .unwrap_or_else(|| panic!("{what}: no change"))
}

/// Every alpha channel's samples.
fn channels(doc: *const PrecisionDocument) -> Vec<Vec<u16>> {
    unsafe { &*doc }
        .channels()
        .iter()
        .map(|c| c.plane().samples().to_vec())
        .collect()
}

#[test]
fn lab_selected_pixels_equal_the_rgb_kernel_on_the_codes() {
    let coverage = ramp_selection();
    for bits in DEPTHS {
        let (lab, rgb) = pair(bits);
        let (lab, rgb) = (with_channel(lab), with_channel(rgb));
        let mut cases: Vec<(Value, &[u16])> = layer_edits()
            .into_iter()
            .chain(plane_edits())
            .map(|r| (r, coverage.as_slice()))
            .collect();
        cases.push((
            json!({"op":"stamp_selected","layers":[0,1],"name":"S (merged)"}),
            &[],
        ));
        for (request, input) in &cases {
            let what = format!("{request} at {bits} bits");
            let a = must(lab, request, input, &what);
            let b = must(rgb, request, input, &what);
            assert_eq!(channels(a), channels(b), "{what}: channels");
            assert_twin(a, b, bits, &what);
        }
        free(rgb);
        free(lab);
    }
}

/// A one-layer document of `pixels` (Lab at `bits` when `lab`).
fn single(pixels: Vec<[u16; 4]>, bits: u32, name: &str) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, twin_profile(), None).unwrap();
    let record = Layer::raster(name, image, (0, 0), 1.0, true).unwrap();
    document(&[record], bits, true)
}

/// `ramp`'s codes, every pixel opaque.
fn opaque(bits: u32) -> Vec<[u16; 4]> {
    ramp(W, H, bits, 0)
        .into_iter()
        .map(|[l, a, b, _]| [l, a, b, 65535])
        .collect()
}

#[test]
fn an_integer_move_lands_the_codes_and_leaves_a_clear_hole() {
    let (x0, y0, w, h, dx, dy) = (4u32, 3u32, 7u32, 5u32, 3i32, 2i32);
    let inside = |x: i32, y: i32| {
        (x0 as i32..(x0 + w) as i32).contains(&x) && (y0 as i32..(y0 + h) as i32).contains(&y)
    };
    let coverage: Vec<u16> = (0..W * H)
        .map(|i| {
            if inside((i % W) as i32, (i / W) as i32) {
                65535
            } else {
                0
            }
        })
        .collect();
    for bits in DEPTHS {
        let before = opaque(bits);
        let doc = single(before.clone(), bits, "Layer 1");
        let request = json!({"op":"move_selected","layer":0,"dx":dx,"dy":dy});
        let what = format!("{request} at {bits} bits");
        let out = must(doc, &request, &coverage, &what);
        assert_eq!(descriptor(out)["color_mode"], "lab", "{what}");
        let after = layer(out, 0, bits);
        assert_eq!(
            after.len(),
            before.len(),
            "{what}: the layer keeps its size"
        );
        for (k, p) in after.iter().enumerate() {
            let (x, y) = ((k as u32 % W) as i32, (k as u32 / W) as i32);
            let old = before[k].map(u32::from);
            if inside(x - dx, y - dy) {
                let src = before[((y - dy) as u32 * W + (x - dx) as u32) as usize];
                assert_eq!(*p, src.map(u32::from), "{what}: landed ({x},{y})");
            } else if inside(x, y) {
                assert_eq!(p[3], 0, "{what}: hole ({x},{y}) clear");
                assert_eq!(p[..3], old[..3], "{what}: hole ({x},{y}) keeps its codes");
            } else {
                assert_eq!(*p, old, "{what}: untouched ({x},{y})");
            }
        }
        free(out);
        free(doc);
    }
}

/// An opaque full-canvas layer: an L\* ramp at a\* = b\* = 0.
fn neutral(bits: u32) -> *mut PrecisionDocument {
    let pixels = opaque(bits)
        .into_iter()
        .map(|[l, _, _, a]| [l, NEUTRAL as u16, NEUTRAL as u16, a])
        .collect();
    single(pixels, bits, "Background")
}

#[test]
fn a_neutral_lab_layer_stays_neutral_under_the_selected_pixels() {
    let coverage = ramp_selection();
    for bits in DEPTHS {
        let doc = neutral(bits);
        for request in layer_edits() {
            let what = format!("{request} at {bits} bits");
            let out = must(doc, &request, &coverage, &what);
            for i in 0..unsafe { &*out }.layer_count() {
                for (k, p) in layer(out, i, bits).iter().enumerate() {
                    if p[3] == 65535 {
                        assert!(
                            p[1] == NEUTRAL && p[2] == NEUTRAL,
                            "{what}: record {i} pixel {k}: {p:?}"
                        );
                    }
                }
            }
            free(out);
        }
        free(doc);
    }
}

/// `dr_precision_doc_select` of `operation` over the composite.
pub(crate) fn select(doc: *const PrecisionDocument, operation: &Value) -> Result<Vec<u16>, String> {
    let request = json!({"op":"selection","operation":operation}).to_string();
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if plan.is_null() {
        return Err(take_err_string(err));
    }
    let planned: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(plan) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(plan) };
    let mut out = vec![0u16; planned["output_samples"].as_u64().unwrap() as usize];
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if ok {
        Ok(out)
    } else {
        Err(take_err_string(err))
    }
}

#[test]
fn the_source_selections_read_the_codes_and_the_colour_kinds_run() {
    for bits in DEPTHS {
        let doc = single(opaque(bits), bits, "Background");
        let codes = layer(doc, 0, bits);
        for (c, channel) in ["red", "green", "blue"].into_iter().enumerate() {
            let operation = json!({"kind":"color","channel":channel});
            let got = select(doc, &operation).unwrap_or_else(|e| panic!("{operation}: {e}"));
            let want: Vec<u16> = codes.iter().map(|p| p[c] as u16).collect();
            assert_eq!(got, want, "{operation} at {bits} bits");
        }
        let alpha_doc = single(ramp(W, H, bits, 0), bits, "Background");
        let got = select(alpha_doc, &json!({"kind":"alpha"})).unwrap();
        let want: Vec<u16> = layer(alpha_doc, 0, bits)
            .iter()
            .map(|p| p[3] as u16)
            .collect();
        assert_eq!(got, want, "alpha at {bits} bits");
        free(alpha_doc);
        assert_eq!(
            select(doc, &json!({"kind":"luminance"})).unwrap_err(),
            LUMINANCE
        );
        // The wand, Select Similar and Grow run at both depths (`lab_wand.rs`
        // holds their oracle).
        let wand = json!({"kind":"magic_wand","x":3,"y":2,"tolerance":40,"contiguous":true});
        select(doc, &wand).unwrap_or_else(|e| panic!("{wand} at {bits} bits: {e}"));
        let cover: Vec<u16> = (0..W * H)
            .map(|i| {
                let inside = (1..5).contains(&(i % W)) && (1..5).contains(&(i / W));
                if inside {
                    65535
                } else {
                    0
                }
            })
            .collect();
        for contiguous in [false, true] {
            let operation = json!({"kind":"similar","tolerance":257 * 10,"contiguous":contiguous});
            let got = crate::precision_select::select(doc, &operation, &cover)
                .unwrap_or_else(|e| panic!("{operation} at {bits} bits: {e}"));
            assert!(got.contains(&65535), "{operation} at {bits} bits selects");
        }
        free(doc);
    }
}

/// `pixels` in an sRGB-tagged RGB16 document (no Lab policy).
fn srgb_single(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let record = Layer::raster("Background", image, (0, 0), 1.0, true).unwrap();
    document(&[record], 16, false)
}

#[test]
fn a_plane_targets_selected_pixels_are_the_rgb_planes_and_the_families_are_listed() {
    let coverage = ramp_selection();
    let matrix = turn(11.0, 0.95, (9.0, 7.0));
    for bits in DEPTHS {
        let pixels = ramp(W, H, bits, 2);
        let lab = single(pixels.clone(), bits, "Background");
        let rgb = srgb_single(pixels);
        for target in ["red", "green", "blue", "alpha"] {
            let swatch = (target != "alpha").then_some([30, 140, 220]);
            let mut requests = vec![
                json!({"op":"move_selected","layer":0,"dx":3,"dy":2,"target":target}),
                json!({"op":"transform_selected","layer":0,"target":target,"matrix":matrix,
                    "filter":"bilinear"}),
            ];
            if let Some(swatch) = swatch {
                requests.push(
                    json!({"op":"move_selected","layer":0,"dx":-2,"dy":1,"target":target,
                    "swatch":swatch}),
                );
            }
            for request in requests {
                let what = format!("{request} at {bits} bits");
                let a = must(lab, &request, &coverage, &what);
                let b = must(rgb, &request, &coverage, &what);
                assert_eq!(descriptor(a)["color_mode"], "lab", "{what}");
                assert_eq!(layer(a, 0, bits), layer(b, 0, 16), "{what}");
                free(a);
                free(b);
            }
        }
        free(rgb);
        free(lab);
    }
    let doc = neutral(16);
    let operations = descriptor(doc)["supported_operations"].clone();
    for family in [
        "selected_pixels_move",
        "selected_pixels_transform",
        "layer_via",
        "layer_stamp_selected",
        "selection",
    ] {
        assert!(
            operations.as_array().unwrap().contains(&family.into()),
            "{family}: {operations}"
        );
    }
    free(doc);
}
