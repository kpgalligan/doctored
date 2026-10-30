//! R2: the Magic Wand, Color Range, Quick Selection, Select Similar and
//! Select and Mask's edge stage (rule E) in a 32-bit float document, through
//! `dr_precision_doc_select`, each comparing the R1 display copy —
//! `round(65535·E(clamp(v, 0, 1)))`, the profile's forward curve, never the
//! tone map — so a value above white compares as white; and the `sample`
//! reply's `center_display_codes`.
//!
//! The edge stage's oracle is a 16-bit twin: a document holding each pixel's
//! R1 code, refined with the same request, equal sample for sample. Its codes
//! are the f64 formula's, within the one code the profile's tabulated curve
//! allows (`a_sample_reply_carries_the_center_display_codes`), taken from the
//! float document's own `sample` reply so the twin's guide is the float one's.
//!
//! The oracle is written here: the sRGB encode spelled out, R1 codes
//! computed in f64 from the authored samples, and the expected coverage of
//! each case derived from those codes (equal codes in, a code past the
//! tolerance out). The values include 1.0, 2.0, 8.0, 0.999, −0.25 and 0.18.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit_plan, dr_precision_doc_select};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::scoped_statistics::AnalysisRequest;
use serde_json::{json, Value};

use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 6;
const H: u32 = 3;
const VALUES: [f32; 6] = [1.0, 2.0, 8.0, 0.999, -0.25, 0.18];

/// The sRGB encode (E), written out from IEC 61966-2.1.
fn encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// R1's code of one linear component.
fn code(v: f32) -> u16 {
    (encode(f64::from(v)) * 65535.0).round() as u16
}

fn gray(v: f32) -> [f32; 4] {
    [v, v, v, 1.0]
}

/// Row 0 holds the six values; row 1 is 0.999 throughout; row 2 is 8.0,
/// cut off from row 0's whites by row 1.
fn pixels() -> Vec<[f32; 4]> {
    let mut out: Vec<[f32; 4]> = VALUES.iter().map(|v| gray(*v)).collect();
    out.extend([gray(0.999); W as usize]);
    out.extend([gray(8.0); W as usize]);
    out
}

fn doc() -> PrecisionDocument {
    float_doc(W, H, &IccProfile::srgb(), &[pixels()])
}

fn take(error: *mut std::ffi::c_char) -> String {
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    text
}

fn plan(doc: &PrecisionDocument, request: &Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut error) };
    if out.is_null() {
        return Err(take(error));
    }
    Ok(serde_json::from_str(&take(out)).unwrap())
}

fn select(doc: &PrecisionDocument, source: &str, operation: Value, input: &[u16]) -> Vec<u16> {
    let mut request = json!({"op":"selection","source":source,"operation":operation});
    if source == "layer" {
        request["layer"] = json!(0);
    }
    let planned = plan(doc, &request).expect("the selection plans");
    assert_eq!(planned["input_samples"], input.len());
    let mut out = vec![999u16; planned["output_samples"].as_u64().unwrap() as usize];
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            input.as_ptr(),
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "{}", take(error));
    out
}

/// Full where `selected`, zero elsewhere.
fn coverage(selected: impl Fn(usize, [f32; 4]) -> bool) -> Vec<u16> {
    pixels()
        .iter()
        .enumerate()
        .map(|(i, p)| if selected(i, *p) { 65535 } else { 0 })
        .collect()
}

#[test]
fn the_oracle_separates_white_from_0_999_and_black_from_0_18() {
    assert_eq!(code(1.0), 65535);
    assert_eq!(code(8.0), 65535);
    assert!(code(0.999) < 65535 - 10);
    assert_eq!(code(-0.25), 0);
    assert!(code(0.18) > 1000);
}

#[test]
fn the_wand_compares_display_codes_contiguous_and_global_on_both_scopes() {
    let d = doc();
    let seed = |contiguous: bool| json!({"kind":"magic_wand","x":2,"y":0,"tolerance":0,"contiguous":contiguous});
    for source in ["layer", "composite"] {
        // Global: every pixel whose R1 code is white's.
        assert_eq!(
            select(&d, source, seed(false), &[]),
            coverage(|_, p| code(p[0]) == 65535),
            "{source} global"
        );
        // Contiguous: row 0's whites only; row 1's 0.999 cuts row 2 off.
        assert_eq!(
            select(&d, source, seed(true), &[]),
            coverage(|i, p| i < W as usize && code(p[0]) == 65535),
            "{source} contiguous"
        );
        // A tolerance that reaches 0.999's code from white (two codes of
        // margin for the profile's tabulated curve) takes row 1 and so row 2
        // too.
        let reach = u32::from(65535 - code(0.999)) + 2;
        let wide = json!({"kind":"magic_wand","x":2,"y":0,"tolerance":reach,"contiguous":true});
        assert_eq!(
            select(&d, source, wide, &[]),
            coverage(|_, p| u32::from(65535 - code(p[0])) <= reach),
            "{source} reach"
        );
    }
}

#[test]
fn color_range_samples_r1_and_highlights_take_above_white() {
    let d = doc();
    // Sampled at white's R1 code, no tolerance: exactly the display whites.
    let sampled = json!({"kind":"color_range","params":{"mode":"sampled",
        "samples":[[65535,65535,65535]],"tolerance":0,"softness":0}});
    assert_eq!(
        select(&d, "composite", sampled, &[]),
        coverage(|_, p| code(p[0]) == 65535)
    );
    // Sampled at 0.18's own code: that pixel alone (0.01 ΔE76 absorbs one
    // code of difference between the profile's tabulated curve and the
    // formula; every other value is far away).
    let c = code(0.18);
    let sampled = json!({"kind":"color_range","params":{"mode":"sampled",
        "samples":[[c,c,c]],"tolerance":0.01,"softness":0}});
    assert_eq!(
        select(&d, "layer", sampled, &[]),
        coverage(|_, p| p[0] == 0.18)
    );
    // Highlights (L* 90…100): 1.0, 2.0, 8.0 and 0.999 are in; 0.18 and
    // −0.25 are out.
    let highlights = json!({"kind":"color_range","params":{"mode":"tonal",
        "lower":90,"upper":100,"softness":5}});
    let got = select(&d, "composite", highlights, &[]);
    for (i, p) in pixels().iter().enumerate() {
        if p[0] >= 0.999 {
            assert!(
                got[i] >= 65534,
                "pixel {i} ({}) is a highlight: {}",
                p[0],
                got[i]
            );
        } else {
            assert_eq!(got[i], 0, "pixel {i} ({}) is not", p[0]);
        }
    }
}

#[test]
fn quick_selection_grows_through_equal_display_codes() {
    let d = doc();
    // No tolerance and no edge: the seed's four-connected set of equal
    // codes, which is row 0's three whites.
    let quick = json!({"kind":"quick","params":{"seeds":[[0,0]],"tolerance":0,"edge":0}});
    assert_eq!(
        select(&d, "composite", quick, &[]),
        coverage(|i, p| i < W as usize && code(p[0]) == 65535)
    );
}

#[test]
fn select_similar_takes_every_pixel_at_the_reference_display_code() {
    let d = doc();
    // The reference is the 8.0 pixel alone.
    let mut reference = vec![0u16; (W * H) as usize];
    reference[2] = 65535;
    let similar = json!({"kind":"similar","tolerance":0,"contiguous":false});
    assert_eq!(
        select(&d, "composite", similar, &reference),
        coverage(|_, p| code(p[0]) == 65535)
    );
}

// ---- the edge stage (rule E) ----------------------------------------------

const EW: u32 = 24;
const EH: u32 = 16;

/// `refine_edge` at `radius` with smart radius over `input` coverage.
fn refined(doc: &PrecisionDocument, radius: f32, input: &[u16]) -> Vec<u16> {
    select(
        doc,
        "composite",
        json!({"kind":"refine_edge","edge_radius":radius,"smart_radius":true}),
        input,
    )
}

/// A 16-bit sRGB document holding `codes` (opaque).
fn twin(codes: &[[u16; 4]]) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(EW, EH, codes.to_vec(), IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

/// Full inside columns `x0..x1`, zero elsewhere.
fn band(x0: u32, x1: u32) -> Vec<u16> {
    (0..EW * EH)
        .map(|i| {
            if (x0..x1).contains(&(i % EW)) {
                65535
            } else {
                0
            }
        })
        .collect()
}

/// The float document's own R1 code of `(x, y)`, from its `sample` reply,
/// within one code of the f64 formula's.
fn sampled_code(doc: &PrecisionDocument, x: u32, y: u32, v: f32) -> [u16; 4] {
    let request = AnalysisRequest::parse(
        &json!({"operation":"sample","source":"composite","x":x,"y":y}).to_string(),
    )
    .unwrap();
    let reply = doc.scoped_analysis(&request, &[], LIMIT, 0, None).unwrap();
    let got: [u16; 4] = serde_json::from_value(reply["center_display_codes"].clone()).unwrap();
    assert!(
        got[..3].iter().all(|g| g.abs_diff(code(v)) <= 1) && got[3] == 65535,
        "{v}: {got:?} vs {}",
        code(v)
    );
    got
}

#[test]
fn the_edge_stage_refines_r1_codes_like_its_16_bit_twin() {
    // Top half 0.18 | 0.999 at x = 12, bottom half −0.25 | 8.0, so the stage
    // sees a mid edge, a black-to-white edge and the values the formula
    // places at 0 and 65535.
    let value = |x: u32, y: u32| match (y < EH / 2, x < 12) {
        (true, true) => 0.18_f32,
        (true, false) => 0.999,
        (false, true) => -0.25,
        (false, false) => 8.0,
    };
    let pixels: Vec<[f32; 4]> = (0..EW * EH).map(|i| gray(value(i % EW, i / EW))).collect();
    let d = float_doc(EW, EH, &IccProfile::srgb(), &[pixels]);
    let codes: Vec<[u16; 4]> = (0..EW * EH)
        .map(|i| {
            let (x, y) = (i % EW, i / EW);
            sampled_code(&d, x, y, value(x, y))
        })
        .collect();
    let sixteen = twin(&codes);
    for (radius, input) in [(3.0, band(9, 15)), (6.0, band(0, 12)), (2.0, band(10, 20))] {
        let got = refined(&d, radius, &input);
        assert_eq!(got, refined(&sixteen, radius, &input), "radius {radius}");
        assert!(got != input, "radius {radius}: the stage changed nothing");
    }
}

#[test]
fn an_edge_between_two_values_above_white_refines_as_flat_white() {
    // 2.0 | 8.0 at x = 12: both are white's R1 code, so the stage sees no
    // edge there.
    let pixels: Vec<[f32; 4]> = (0..EW * EH)
        .map(|i| gray(if i % EW < 12 { 2.0 } else { 8.0 }))
        .collect();
    let d = float_doc(EW, EH, &IccProfile::srgb(), &[pixels]);
    let flat = twin(&vec![[65535; 4]; (EW * EH) as usize]);
    // A real edge at the same place, 0.18 | white, refines differently.
    let edged = twin(
        &(0..EW * EH)
            .map(|i| {
                let c = if i % EW < 12 { code(0.18) } else { 65535 };
                [c, c, c, 65535]
            })
            .collect::<Vec<_>>(),
    );
    let input = band(0, 12);
    for radius in [4.0, 8.0] {
        let got = refined(&d, radius, &input);
        assert_eq!(got, refined(&flat, radius, &input), "radius {radius}");
        assert_ne!(got, refined(&edged, radius, &input), "radius {radius}");
    }
}

#[test]
fn a_sample_reply_carries_the_center_display_codes() {
    let d = doc();
    for (x, y, v) in [(2, 0, 8.0f32), (3, 0, 0.999), (4, 0, -0.25), (5, 0, 0.18)] {
        let request = AnalysisRequest::parse(
            &json!({"operation":"sample","source":"composite","x":x,"y":y}).to_string(),
        )
        .unwrap();
        let reply = d.scoped_analysis(&request, &[], LIMIT, 0, None).unwrap();
        let got: [u16; 4] = serde_json::from_value(reply["center_display_codes"].clone()).unwrap();
        let c = code(v);
        // Within one code of the formula: the profile's curve is tabulated.
        assert!(
            got[..3].iter().all(|g| g.abs_diff(c) <= 1) && got[3] == 65535,
            "{v}: {got:?} vs {c}"
        );
    }
    let request = AnalysisRequest::parse(
        &json!({"operation":"sample","source":"composite","x":-1,"y":0,"reach":1}).to_string(),
    )
    .unwrap();
    let reply = d.scoped_analysis(&request, &[], LIMIT, 0, None).unwrap();
    assert_eq!(reply["center_display_codes"], Value::Null);
}
