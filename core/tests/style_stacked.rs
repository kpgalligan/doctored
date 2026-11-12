//! STACKED layer-effect instances — several Drop Shadows,
//! Inner Shadows, Strokes, Colour Overlays or Gradient Overlays on one layer —
//! black-box: the JSON's instance rules and their refusals by name, the
//! renderer at 8, 16 and 32 bits against closed forms written here, Merge
//! Down's extent, and the `.rz` word 89 at both depths (the claim checked both
//! ways, a comp's recorded style included).
//!
//! The oracle is Photoshop's measured rule (`scripts/photoshop-probe/
//! stacked_effects_take.py`, take1: TOPFIRST and R), restated in Doctored's
//! terms and never read off the implementation: within a kind the LATER
//! instance draws on top, each with its own colour, opacity and geometry, and
//! each Inside Stroke replaces everything of the layer under it over the one
//! backdrop `B0` beneath the layer. Every edge here is hard and every probe is
//! on a mid-line, so a stroke of size `s` covers the pixels whose axis
//! distance to the rect is `1..=s` in full, and a hard shadow of distance `d`
//! at 90 degrees covers the rect moved down by `d`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_comps::CompRequest;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES as LIMIT};
use doctored_core::style::{is_stacked, EffectKind, LayerStyle, MAX_INSTANCES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use crate::float_fixture::float_doc;
use crate::identity;
use common::{cpath, flat_pixels, parse, solid, styled, take_err_string};

const CANVAS: (u32, u32) = (64, 48);
/// x 20..44, y 14..30.
const RECT: (i32, i32, u32, u32) = (20, 14, 24, 16);
const X0: u32 = 20;
const Y0: u32 = 14;
const X1: u32 = 44;
const Y1: u32 = 30;
const MX: u32 = 32;
const MY: u32 = 22;
const SOURCE: [u8; 3] = [48, 160, 80];
const RED: [f64; 3] = [1.0, 0.0, 0.0];
const BLUE: [f64; 3] = [0.0, 0.0, 1.0];
const WHITE3: [f64; 3] = [1.0, 1.0, 1.0];
/// The words, spelled out here rather than imported, so a change to a
/// constant cannot quietly change the claim.
const STACKED_WORD: u32 = 89;

// ------------------------------------------------------------ the oracle --

fn unit(c: [u8; 3]) -> [f64; 3] {
    c.map(|v| f64::from(v) / 255.0)
}

/// W3C source-over (Normal) of `cs` at `sa` onto an opaque `cb`.
fn over(cb: [f64; 3], cs: [f64; 3], sa: f64) -> [f64; 3] {
    std::array::from_fn(|c| cb[c] + sa * (cs[c] - cb[c]))
}

fn assert_near8(got: [u8; 4], want: [f64; 3], what: &str) {
    for c in 0..3 {
        let w = want[c] * 255.0;
        assert!(
            (f64::from(got[c]) - w).abs() <= 1.0,
            "{what} channel {c}: got {got:?}, want {want:?}"
        );
    }
}

/// The two outside strokes' closed form at axis distance `d` outside the
/// rect: blue (size 2, on top) over red (size 4) over the white backdrop.
fn two_strokes(d: u32) -> [f64; 3] {
    match d {
        1..=2 => BLUE,
        3..=4 => RED,
        _ => WHITE3,
    }
}

/// The two inside bands' closed form (take1's rule R) at axis distance `d`
/// inside the rect: blue (3 px, 50 %, on top) and red (6 px, 50 %) each
/// replace over B0 = white; past both bands, the layer.
fn two_bands(d: u32, b0: [f64; 3], layer: [f64; 3], ink: (&[f64; 3], &[f64; 3])) -> [f64; 3] {
    match d {
        1..=3 => over(b0, *ink.1, 0.5),
        4..=6 => over(b0, *ink.0, 0.5),
        _ => layer,
    }
}

/// `(point, axis distance)` on each mid-line, OUTSIDE the rect, for `1..=n`.
fn outside_probes(n: u32) -> Vec<((u32, u32), u32)> {
    let mut out = Vec::new();
    for d in 1..=n {
        out.push(((MX, Y0 - d), d));
        out.push(((MX, Y1 - 1 + d), d));
        out.push(((X0 - d, MY), d));
        out.push(((X1 - 1 + d, MY), d));
    }
    out
}

/// `(point, axis distance)` on each mid-line, INSIDE the rect, for `1..=n`.
fn inside_probes(n: u32) -> Vec<((u32, u32), u32)> {
    let mut out = Vec::new();
    for d in 1..=n {
        out.push(((MX, Y0 + d - 1), d));
        out.push(((MX, Y1 - d), d));
        out.push(((X0 + d - 1, MY), d));
        out.push(((X1 - d, MY), d));
    }
    out
}

const STROKES: &str = "{\"effects\":[\
    {\"type\":\"stroke\",\"position\":\"outside\",\"size\":4,\"color\":\"#ff0000\"},\
    {\"type\":\"stroke\",\"position\":\"outside\",\"size\":2,\"color\":\"#0000ff\"}]}";
const BANDS: &str = "{\"effects\":[\
    {\"type\":\"stroke\",\"position\":\"inside\",\"size\":6,\"color\":\"#ff0000\",\"opacity\":0.5},\
    {\"type\":\"stroke\",\"position\":\"inside\",\"size\":3,\"color\":\"#0000ff\",\"opacity\":0.5}]}";

fn shadows(kind: &str) -> String {
    format!(
        "{{\"effects\":[\
         {{\"type\":\"{kind}\",\"color\":\"#0000ff\",\"distance\":8,\"size\":0,\"angle\":90,\
          \"use_global_light\":false,\"opacity\":1,\"blend\":\"normal\"}},\
         {{\"type\":\"{kind}\",\"color\":\"#ff0000\",\"distance\":4,\"size\":0,\"angle\":90,\
          \"use_global_light\":false,\"opacity\":1,\"blend\":\"normal\"}}]}}"
    )
}

// ---------------------------------------------------------- the fixtures --

fn rect_doc() -> DrDocument {
    let rect = RgbaImage::from_pixel(RECT.2, RECT.3, Rgba([SOURCE[0], SOURCE[1], SOURCE[2], 255]));
    DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, [255, 255, 255, 255]))
        .adding_image_layer(0, rect, "Rect")
        .and_then(|d| d.with_layer_offset(1, RECT.0, RECT.1))
        .map(|mut d| {
            // Made at RECT: its reference point stays (0, 0).
            d.layers[1].reference = (0, 0);
            d
        })
        .expect("the rect layer")
}

fn flat(doc: &DrDocument) -> Vec<u8> {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let out = flat_pixels(handle);
    unsafe { dr_doc_free(handle) };
    out
}

fn at(img: &[u8], (x, y): (u32, u32)) -> [u8; 4] {
    common::pixel(img, CANVAS.0, x, y)
}

fn image16(w: u32, h: u32, f: impl Fn(u32, u32) -> [u16; 4]) -> PrecisionImage {
    let pixels = (0..w * h).map(|i| f(i % w, i / w)).collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

fn typed_doc() -> PrecisionDocument {
    let s = SOURCE.map(|v| 257 * u16::from(v));
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster(
                "Back",
                image16(CANVAS.0, CANVAS.1, |_, _| [65535; 4]),
                (0, 0),
                1.0,
                true,
            )
            .unwrap(),
            Layer::raster(
                "Rect",
                image16(RECT.2, RECT.3, |_, _| [s[0], s[1], s[2], 65535]),
                (RECT.0, RECT.1),
                1.0,
                true,
            )
            .unwrap(),
        ],
    )
    .unwrap()
}

fn take(err: *mut c_char) -> String {
    if err.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    s
}

/// `dr_precision_doc_edit` `layer_style` on `layer`, asserting a change.
fn typed_styled(doc: &PrecisionDocument, layer: usize, style: Value) -> PrecisionDocument {
    let request = json!({"op":"layer_style","version":1,"layer":layer,"style":style});
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "{request}: {}", take(err));
    assert!(!out.is_null(), "{request} changed nothing");
    *unsafe { Box::from_raw(out) }
}

fn value(json: &str) -> Value {
    serde_json::from_str(json).unwrap()
}

// --------------------------------------------------------- the JSON rules --

fn overlays(n: usize) -> String {
    let list: Vec<String> = (0..n)
        .map(|i| {
            format!(
                "{{\"type\":\"color_overlay\",\"color\":\"#{:02x}0000\"}}",
                i * 20
            )
        })
        .collect();
    format!("{{\"effects\":[{}]}}", list.join(","))
}

fn overlay_reds(style: &LayerStyle) -> Vec<String> {
    let json: Value = serde_json::from_str(&style.to_json()).unwrap();
    json["effects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "color_overlay")
        .map(|e| e["color"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn strict_json_holds_ten_of_a_repeating_kind_and_refuses_by_name_past_it() {
    assert_eq!(MAX_INSTANCES, 10);
    let ten = parse(&overlays(10));
    assert!(is_stacked(&ten));
    let reds = overlay_reds(&ten);
    assert_eq!(reds.len(), 10);
    assert_eq!(reds[0], "#000000");
    assert_eq!(reds[9], "#b40000", "array order is kept: render order");
    assert_eq!(parse(&ten.to_json()), ten, "the canonical JSON round-trips");
    assert_eq!(
        LayerStyle::from_json(&overlays(11)).unwrap_err(),
        "effects[10].type exceeds 10 instances of color_overlay"
    );
    // Every repeating kind is admitted; a single kind still refuses its
    // second instance with the sentence it always had.
    for kind in [
        "drop_shadow",
        "inner_shadow",
        "stroke",
        "color_overlay",
        "gradient_overlay",
    ] {
        let two = format!("{{\"effects\":[{{\"type\":\"{kind}\"}},{{\"type\":\"{kind}\"}}]}}");
        assert!(is_stacked(&parse(&two)), "{kind}");
        assert!(EffectKind::from_type_name(kind).unwrap().repeats());
    }
    for kind in ["outer_glow", "inner_glow", "bevel_emboss", "satin"] {
        let two = format!("{{\"effects\":[{{\"type\":\"{kind}\"}},{{\"type\":\"{kind}\"}}]}}");
        assert_eq!(
            LayerStyle::from_json(&two).unwrap_err(),
            "effects[1].type duplicates an earlier effect",
            "{kind}"
        );
        assert!(!EffectKind::from_type_name(kind).unwrap().repeats());
    }
    // A disabled second instance is still an instance: stored, stacked.
    let disabled =
        parse("{\"effects\":[{\"type\":\"stroke\"},{\"type\":\"stroke\",\"enabled\":false}]}");
    assert!(is_stacked(&disabled));
    assert!(!is_stacked(&parse("{\"effects\":[{\"type\":\"stroke\"}]}")));
}

#[test]
fn lenient_json_keeps_the_first_ten_and_the_first_single_and_normalized_agrees() {
    let lenient = LayerStyle::from_json_lenient(&overlays(12)).unwrap();
    let reds = overlay_reds(&lenient);
    assert_eq!(reds.len(), 10, "the first ten");
    assert_eq!(reds[9], "#b40000");
    let satins = LayerStyle::from_json_lenient(
        "{\"effects\":[{\"type\":\"satin\",\"color\":\"#112233\"},\
          {\"type\":\"satin\",\"color\":\"#445566\"}]}",
    )
    .unwrap();
    assert_eq!(satins.effects.len(), 1);
    assert!(satins.to_json().contains("#112233"), "the first is kept");

    // `normalized` re-establishes the same invariant on a constructed value:
    // stable-sorted by kind, the instances in their order, the cap applied.
    let a = parse(&overlays(10));
    let shadow = parse("{\"effects\":[{\"type\":\"drop_shadow\"}]}");
    let satin = parse("{\"effects\":[{\"type\":\"satin\"}]}");
    let mut built = LayerStyle::default();
    built.effects = a.effects.clone();
    built.effects.insert(3, shadow.effects[0].clone());
    built.effects.push(satin.effects[0].clone());
    built.effects.push(satin.effects[0].clone());
    built.effects.push(a.effects[0].clone());
    let normalized = built.normalized();
    let kinds: Vec<EffectKind> = normalized.effects.iter().map(|e| e.kind()).collect();
    assert_eq!(kinds.len(), 12, "the shadow, ten overlays, one satin");
    assert_eq!(kinds[0], EffectKind::DropShadow);
    assert_eq!(kinds[11], EffectKind::Satin);
    assert_eq!(overlay_reds(&normalized), overlay_reds(&a));
}

// ----------------------------------------------------- 8 bits, through FFI --

#[test]
fn two_outside_strokes_stack_the_later_on_top() {
    let img = flat(&styled(&rect_doc(), 1, STROKES));
    for (point, d) in outside_probes(6) {
        assert_near8(at(&img, point), two_strokes(d), &format!("{point:?} d {d}"));
    }
    assert_near8(
        at(&img, (MX, MY)),
        unit(SOURCE),
        "the interior is untouched",
    );
}

#[test]
fn two_hard_drop_shadows_land_at_their_own_distances() {
    let img = flat(&styled(&rect_doc(), 1, &shadows("drop_shadow")));
    // Red (distance 4) is the later instance, so it is on top where both land.
    for k in 0..10u32 {
        let want = match k {
            0..=3 => RED,
            4..=7 => BLUE,
            _ => WHITE3,
        };
        assert_near8(at(&img, (MX, Y1 + k)), want, &format!("row {}", Y1 + k));
    }
    assert_near8(at(&img, (MX, Y0 - 1)), WHITE3, "nothing above");
    assert_near8(at(&img, (MX, MY)), unit(SOURCE), "the layer covers them");
}

#[test]
fn two_hard_inner_shadows_stack_inside_the_top_edge() {
    let img = flat(&styled(&rect_doc(), 1, &shadows("inner_shadow")));
    for k in 0..10u32 {
        let want = match k {
            0..=3 => RED,
            4..=7 => BLUE,
            _ => unit(SOURCE),
        };
        assert_near8(at(&img, (MX, Y0 + k)), want, &format!("row {}", Y0 + k));
    }
}

#[test]
fn two_colour_overlays_composite_in_order() {
    let style = "{\"effects\":[\
        {\"type\":\"color_overlay\",\"color\":\"#ff0000\",\"opacity\":1},\
        {\"type\":\"color_overlay\",\"color\":\"#0000ff\",\"opacity\":0.5}]}";
    let img = flat(&styled(&rect_doc(), 1, style));
    for point in [(MX, MY), (X0, Y0), (X1 - 1, Y1 - 1)] {
        assert_near8(at(&img, point), over(RED, BLUE, 0.5), &format!("{point:?}"));
    }
    assert_near8(at(&img, (MX, Y0 - 1)), WHITE3, "outside");
}

#[test]
fn two_inside_bands_each_replace_over_the_backdrop_rule_r() {
    let img = flat(&styled(&rect_doc(), 1, BANDS));
    for (point, d) in inside_probes(8) {
        let want = two_bands(d, WHITE3, unit(SOURCE), (&RED, &BLUE));
        assert_near8(at(&img, point), want, &format!("{point:?} d {d}"));
    }
    // Rule O (only the lowest band replaces) would draw the blue band over
    // the red one's replacement; R does not.
    let rule_o = over(over(WHITE3, RED, 0.5), BLUE, 0.5);
    let got = at(&img, (MX, Y0));
    assert!(
        (f64::from(got[1]) - rule_o[1] * 255.0).abs() > 20.0,
        "not rule O: {got:?}"
    );
}

#[test]
fn merge_down_grows_to_hold_every_stacked_shadow() {
    // A 20x20 red layer at (30, 30) over a 10x10 white one at (40, 40) on a
    // transparent 100x100 canvas, with two hard shadows: one cast LEFT by 10
    // (angle 0) and one cast DOWN by 8 (angle 90). Size 0 -> pad 2: the
    // merged rect is the red rect grown by 2, then by 10 on the left (the
    // first shadow) and 8 below (the second) — a single-shadow extent would
    // lose one of them.
    let base = DrDocument::from_pixels(solid(100, 100, [0, 0, 0, 0]))
        .adding_image_layer(0, solid(10, 10, [255, 255, 255, 255]), "Lower")
        .unwrap()
        .with_layer_offset(1, 40, 40)
        .unwrap()
        .adding_image_layer(1, solid(20, 20, [255, 0, 0, 255]), "Upper")
        .unwrap()
        .with_layer_offset(2, 30, 30)
        .unwrap();
    let doc = styled(
        &base,
        2,
        "{\"effects\":[\
         {\"type\":\"drop_shadow\",\"size\":0,\"distance\":10,\"angle\":0,\
          \"use_global_light\":false},\
         {\"type\":\"drop_shadow\",\"size\":0,\"distance\":8,\"angle\":90,\
          \"use_global_light\":false}]}",
    );
    assert_eq!(doc.layers[2].style.as_ref().unwrap().pad(), 2);
    let before = doc.flattened().unwrap().into_raw();
    let merged = doc.merging_down(2).expect("merge");
    let layer = &merged.layers[1];
    assert_eq!(layer.offset, (18, 28), "grown 10 left by the first shadow");
    assert_eq!(
        layer.pixels.dimensions(),
        (34, 32),
        "and 8 down by the second"
    );
    assert_eq!(
        merged.flattened().unwrap().into_raw(),
        before,
        "the projection survives the merge"
    );
}

// --------------------------------------------------------- 16 and 32 bits --

#[test]
fn the_16_bit_strokes_and_bands_match_the_closed_forms() {
    let probe = |doc: &PrecisionDocument,
                 points: Vec<((u32, u32), u32)>,
                 want: &dyn Fn(u32) -> [f64; 3]| {
        let out = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
        for ((x, y), d) in points {
            let got = out[(y * CANVAS.0 + x) as usize];
            let w = want(d);
            for c in 0..3 {
                let g = f64::from(got[c]) / 65535.0;
                assert!(
                    (g - w[c]).abs() * 255.0 <= 0.02,
                    "({x}, {y}) d {d} channel {c}: got {g}, want {}",
                    w[c]
                );
            }
        }
    };
    let strokes = typed_styled(&typed_doc(), 1, value(STROKES));
    probe(&strokes, outside_probes(6), &two_strokes);
    let bands = typed_styled(&typed_doc(), 1, value(BANDS));
    probe(&bands, inside_probes(8), &|d| {
        two_bands(d, WHITE3, unit(SOURCE), (&RED, &BLUE))
    });
}

#[test]
fn the_32_bit_strokes_and_bands_match_the_closed_forms_in_linear_light() {
    let (w, h) = CANVAS;
    let source = [0.2f32, 0.5, 0.1, 1.0];
    let back = vec![[1.0f32, 1.0, 1.0, 1.0]; (w * h) as usize];
    let rect: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if (X0..X1).contains(&x) && (Y0..Y1).contains(&y) {
                source
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let base = float_doc(w, h, &IccProfile::srgb(), &[back, rect]);
    let layer = [0.2, 0.5, 0.1];
    let check = |doc: &PrecisionDocument,
                 points: Vec<((u32, u32), u32)>,
                 want: &dyn Fn(u32) -> [f64; 3]| {
        assert_eq!(doc.float_refusal(), None);
        let out = doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap();
        for ((x, y), d) in points {
            let got = out[(y * w + x) as usize];
            let want = want(d);
            for c in 0..3 {
                assert!(
                    (f64::from(got[c]) - want[c]).abs() <= 2e-5 * want[c].abs().max(1.0),
                    "({x}, {y}) d {d} channel {c}: got {}, want {}",
                    got[c],
                    want[c]
                );
            }
        }
    };
    // #ff0000 and #0000ff are exact in linear light; the source is the layer.
    check(
        &typed_styled(&base, 1, value(STROKES)),
        outside_probes(6),
        &two_strokes,
    );
    check(
        &typed_styled(&base, 1, value(BANDS)),
        inside_probes(8),
        &|d| two_bands(d, WHITE3, layer, (&RED, &BLUE)),
    );
}

// ------------------------------------------------------------- byte .rz --

fn save(doc: &DrDocument, path: &std::path::Path) -> Vec<u8> {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let mut err: *mut c_char = ptr::null_mut();
    let ok =
        unsafe { dr_doc_save_native(handle, cpath(path).as_ptr(), std::ptr::null(), &mut err) };
    unsafe { dr_doc_free(handle) };
    assert!(ok, "save_native: {}", take_err_string(err));
    std::fs::read(path).expect("the saved file")
}

fn open(bytes: &[u8], dir: &TempDir, name: &str) -> Result<DrDocument, String> {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_open(cpath(&path).as_ptr(), std::ptr::null(), &mut err) };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(*unsafe { Box::from_raw(out) })
    }
}

fn insert_word(bytes: &[u8], at: usize, word: u32) -> Vec<u8> {
    let cut = 4 + at * 4;
    let mut out = bytes[..cut].to_vec();
    out.extend_from_slice(&word.to_le_bytes());
    out.extend_from_slice(&bytes[cut..]);
    out
}

fn remove_word(bytes: &[u8], at: usize) -> Vec<u8> {
    let cut = 4 + at * 4;
    let mut out = bytes[..cut].to_vec();
    out.extend_from_slice(&bytes[cut + 4..]);
    out
}

fn style_json(doc: &DrDocument) -> Option<String> {
    doc.layers[1].style.as_ref().map(|s| s.to_json())
}

#[test]
fn word_89_is_the_first_word_inside_45_only_for_a_stacked_style() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("doc.rz");
    let single = styled(
        &rect_doc(),
        1,
        "{\"effects\":[{\"type\":\"stroke\",\"size\":4}]}",
    );
    let single_bytes = save(&single, &path);
    assert_eq!(
        identity::drdc_word(&single_bytes, 0),
        identity::IDENTITY_BYTE
    );
    assert_ne!(identity::drdc_word(&single_bytes, 1), STACKED_WORD);

    let stacked = styled(&single, 1, STROKES);
    let bytes = save(&stacked, &path);
    assert_eq!(identity::drdc_word(&bytes, 1), STACKED_WORD, "45 -> 89");
    let tail = identity::identity_tail(2);
    assert_eq!(
        &bytes[bytes.len() - tail..bytes.len() - tail + 4],
        b"LIDS",
        "word 89 appends no table"
    );
    let reopened = open(&bytes, &dir, "reopen.rz").expect("reopen");
    assert_eq!(
        style_json(&reopened),
        style_json(&stacked),
        "the instances survive"
    );
    assert_eq!(flat(&reopened), flat(&stacked), "and so does the picture");
    assert_eq!(
        save(&reopened, &dir.path().join("again.rz")),
        bytes,
        "resave is identical"
    );

    // A disabled second instance is stored, so it claims the word too.
    let disabled = styled(
        &single,
        1,
        "{\"effects\":[{\"type\":\"stroke\",\"size\":4},\
         {\"type\":\"stroke\",\"size\":2,\"enabled\":false}]}",
    );
    assert_eq!(
        identity::drdc_word(&save(&disabled, &path), 1),
        STACKED_WORD
    );
}

#[test]
fn the_byte_reader_refuses_a_broken_89_claim_both_ways() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("doc.rz");
    let single_bytes = save(
        &styled(&rect_doc(), 1, "{\"effects\":[{\"type\":\"stroke\"}]}"),
        &path,
    );
    let message = open(
        &insert_word(&single_bytes, 1, STACKED_WORD),
        &dir,
        "forged.rz",
    )
    .err()
    .expect("89 with no stacked style must refuse");
    assert!(
        message.ends_with("invalid native stacked effects word"),
        "{message}"
    );

    let bytes = save(&styled(&rect_doc(), 1, STROKES), &path);
    let message = open(&remove_word(&bytes, 1), &dir, "stripped.rz")
        .err()
        .expect("a stacked style with no 89 must refuse");
    assert!(
        message.ends_with("native stacked layer effects require DRDC 89"),
        "{message}"
    );
}

#[test]
fn a_stacked_style_held_only_by_a_byte_comp_claims_89() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("doc.rz");
    let stacked = styled(&rect_doc(), 1, STROKES);
    let request = CompRequest::parse(
        &json!({"op":"layer_comp","action":"save","name":"Stacked","appearance":true}).to_string(),
    )
    .unwrap();
    let with_comp = stacked.edit_layer_comps(&request).unwrap().expect("a comp");
    let live_single = styled(
        &with_comp,
        1,
        "{\"effects\":[{\"type\":\"stroke\",\"size\":4}]}",
    );
    assert!(!is_stacked(live_single.layers[1].style.as_ref().unwrap()));
    let bytes = save(&live_single, &path);
    assert_eq!(
        identity::drdc_word(&bytes, 1),
        STACKED_WORD,
        "the comp claims it"
    );
    let reopened = open(&bytes, &dir, "reopen.rz").expect("reopen");
    assert_eq!(reopened.layer_comps().len(), 1);
    let message = open(&remove_word(&bytes, 1), &dir, "stripped.rz")
        .err()
        .expect("a comp's stacked style with no 89 must refuse");
    assert!(
        message.ends_with("native stacked layer effects require DRDC 89"),
        "{message}"
    );
}

// ------------------------------------------------------------ typed .rz --

fn typed_with(style: Option<&str>) -> PrecisionDocument {
    let doc = typed_doc();
    match style {
        Some(json) => typed_styled(&doc, 1, value(json)),
        None => doc,
    }
}

fn encode(doc: &PrecisionDocument) -> Vec<u8> {
    doc.encode_native(LIMIT, 0, None).unwrap()
}

fn parse_typed(bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::parse_native(bytes, LIMIT, 0, None)
}

#[test]
fn the_typed_word_89_follows_46_and_its_claim_is_checked_both_ways() {
    let single = encode(&typed_with(Some("{\"effects\":[{\"type\":\"stroke\"}]}")));
    assert_ne!(identity::drdc_word(&single, 1), STACKED_WORD);
    let doc = typed_with(Some(STROKES));
    let bytes = encode(&doc);
    assert_eq!(identity::drdc_word(&bytes, 1), STACKED_WORD, "46 -> 89");
    let reopened = parse_typed(&bytes).expect("reopen");
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style(),
        "the instances survive"
    );
    assert_eq!(encode(&reopened), bytes, "resave is identical");

    let message = parse_typed(&insert_word(&single, 1, STACKED_WORD))
        .err()
        .expect("89 with no stacked style must refuse");
    assert!(
        message.ends_with("invalid native stacked effects word"),
        "{message}"
    );
    let message = parse_typed(&remove_word(&bytes, 1))
        .err()
        .expect("a stacked style with no 89 must refuse");
    assert!(
        message.ends_with("native stacked layer effects require DRDC 89"),
        "{message}"
    );
}

#[test]
fn a_stacked_style_held_only_by_a_typed_comp_claims_89() {
    let stacked = typed_with(Some(STROKES));
    let request = CompRequest::parse(
        &json!({"op":"layer_comp","action":"save","name":"Stacked","appearance":true}).to_string(),
    )
    .unwrap();
    let with_comp = stacked.edit_layer_comps(&request).unwrap().expect("a comp");
    let live_single = typed_styled(
        &with_comp,
        1,
        value("{\"effects\":[{\"type\":\"stroke\"}]}"),
    );
    let bytes = encode(&live_single);
    assert_eq!(
        identity::drdc_word(&bytes, 1),
        STACKED_WORD,
        "the comp claims it"
    );
    let reopened = parse_typed(&bytes).expect("reopen");
    assert_eq!(reopened.layer_comps().len(), 1);
    let message = parse_typed(&remove_word(&bytes, 1))
        .err()
        .expect("a comp's stacked style with no 89 must refuse");
    assert!(
        message.ends_with("native stacked layer effects require DRDC 89"),
        "{message}"
    );
}
