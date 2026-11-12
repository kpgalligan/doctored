//! Blend If on a CMYK document (#1, the Blend If batch): the rule fitted to
//! Photoshop 26.10.0 on take1 and confirmed on take2
//! (`scripts/photoshop-probe/cmyk_styles_take.py`, THE BLEND IF FIT), written
//! here as an independent closed form and checked over a grid of inks —
//! backdrop rows × source columns — on Gray and each plate, This Layer and
//! Underlying split ramps, at 8 bits exactly and at 16 bits within one code;
//! an 8-bit hard Underlying edge over a semi-transparent composite (the
//! backdrop read as its stored code); a 16-bit Gray ramp that splits two
//! backdrops sharing one 8-bit luminosity; Merge Down, Duplicate, Convert and
//! Assign Profile, 8↔16 Bits, the mode conversions, `.rz` (word 86) and the
//! layered PSD; and every refusal by its sentence.
//!
//! THE CLOSED FORM. Each ink is first its stored code at the document's
//! depth (`code_max` 255 or 65535). A plate reads `code_max − code`; Gray
//! reads `round(code_max·(0.30·n_r + 0.59·n_g + 0.11·n_b))` of the naive RGB
//! `n = ((1 − c)(1 − k), (1 − m)(1 − k), (1 − y)(1 − k))`. Each ramp
//! `[lo0, lo1, hi0, hi1]` (8-bit units) has its knees moved — INC
//! `[lo0 − 1, lo1, hi0, hi1 + 1]` on Gray, Cyan, Magenta and Black, HALF
//! `[lo0 − ½, lo1 − ½, hi0 + ½, hi1 + ½]` on Yellow — and scaled by
//! `code_max / 255`; the weight is the This Layer ramp on the source × the
//! Underlying ramp on the backdrop, and an opaque Normal layer over an
//! opaque backdrop gives `round(cb + w·(cs − cb))` per plate.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::{
    dr_precision_doc_convert_cmyk_to_rgb, dr_precision_doc_convert_cmyk_to_rgb_plan,
    dr_precision_doc_convert_to_cmyk,
};
use doctored_core::ffi_cmyk_colour::dr_precision_doc_cmyk_profile_edit;
use doctored_core::ffi_precision::dr_precision_doc_duplicate_layer;
use doctored_core::ffi_psd::{dr_precision_doc_psd_export_report, dr_precision_doc_save_psd};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::style::{BlendIf, BlendIfChannel};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::{press_profile, press_profile_b};
use crate::cmyk_psd_import::open_psd;
use crate::cmyk_structure_fixture::{
    cmyk_word, free, open, refusal, save, then, try_edit, with_opacity, LIMIT,
};
use crate::cmyk_style_fixture::{backdrop, edit, flat, hard_rect, layer, paint, sample};
use crate::common::{cpath, take_err_string};

const CHANNELS: [&str; 5] = ["gray", "cyan", "magenta", "yellow", "black"];
const FULL: [u8; 4] = [0, 0, 255, 255];
/// `(this_layer, underlying)`: take1's two split cases and one with every
/// knee split on both sides.
const RAMPS: [([u8; 4], [u8; 4]); 3] = [
    ([0, 0, 60, 140], FULL),
    (FULL, [60, 140, 255, 255]),
    ([30, 90, 200, 240], [20, 70, 180, 250]),
];
/// The grid's 8-bit codes: paper, take1's source, values on and beside the
/// knees (P = 255 − code: 115 → 140, 195 → 60, 196 → 59, 155 → 100), and
/// full ink. No Gray value is an exact half code (where f32 and f64 may round
/// apart): (60, 180, 30, 10) would be 122.5.
const GRID: [[u16; 4]; 12] = [
    [0, 0, 0, 0],
    [26, 153, 77, 0],
    [128, 64, 200, 30],
    [200, 10, 90, 120],
    [60, 180, 31, 10],
    [255, 255, 255, 255],
    [90, 90, 90, 90],
    [10, 230, 140, 60],
    [170, 40, 110, 0],
    [35, 75, 250, 200],
    [115, 195, 60, 155],
    [196, 116, 176, 96],
];

// ------------------------------------------------------------ the oracle --

/// The knees of `ramp` on `channel` at `code_max` (module doc).
fn knees(channel: &str, ramp: [u8; 4], code_max: f64) -> [f64; 4] {
    let shift = if channel == "yellow" {
        [-0.5, -0.5, 0.5, 0.5]
    } else {
        [-1.0, 0.0, 0.0, 1.0]
    };
    let scale = code_max / 255.0;
    [0, 1, 2, 3].map(|i| (f64::from(ramp[i]) + shift[i]) * scale)
}

fn ramp_at(v: f64, k: [f64; 4]) -> f64 {
    if v < k[0] {
        0.0
    } else if v < k[1] {
        (v - k[0]) / (k[1] - k[0])
    } else if v <= k[2] {
        1.0
    } else if v <= k[3] {
        (k[3] - v) / (k[3] - k[2])
    } else {
        0.0
    }
}

/// The value a ramp reads from stored CODES at `code_max` (module doc).
fn value(channel: &str, codes: [f64; 4], code_max: f64) -> f64 {
    let plate = |p: usize| code_max - codes[p];
    match channel {
        "cyan" => plate(0),
        "magenta" => plate(1),
        "yellow" => plate(2),
        "black" => plate(3),
        _ => {
            let u = codes.map(|c| c / code_max);
            let paper = 1.0 - u[3];
            let n = [
                (1.0 - u[0]) * paper,
                (1.0 - u[1]) * paper,
                (1.0 - u[2]) * paper,
            ];
            (code_max * (0.30 * n[0] + 0.59 * n[1] + 0.11 * n[2])).round()
        }
    }
}

fn weight(
    channel: &str,
    this: [u8; 4],
    under: [u8; 4],
    cs: [f64; 4],
    cb: [f64; 4],
    code_max: f64,
) -> f64 {
    ramp_at(value(channel, cs, code_max), knees(channel, this, code_max))
        * ramp_at(
            value(channel, cb, code_max),
            knees(channel, under, code_max),
        )
}

/// `round(cb + w·(cs − cb))` per plate, unrounded (the caller rounds).
fn mixed(cb: [f64; 4], cs: [f64; 4], w: f64) -> [f64; 4] {
    [0, 1, 2, 3].map(|c| cb[c] + w * (cs[c] - cb[c]))
}

// ------------------------------------------------------------ the inputs --

/// The grid's ink at `i`: the 8-bit code itself, or at 16 bits that code
/// promoted and moved off the 8-bit grid by a few codes.
fn grid_ink(i: usize, depth: u32) -> [f64; 4] {
    let codes = GRID[i];
    if depth == 8 {
        return codes.map(|c| f64::from(c) * 100.0 / 255.0);
    }
    [0, 1, 2, 3].map(|c| {
        let promoted = i64::from(codes[c]) * 257 + ((i * 7 + c * 3) % 5) as i64 * 37 - 74;
        promoted.clamp(0, 65535) as f64 * 100.0 / 65535.0
    })
}

/// An `n × n` document: the backdrop's row `r` holds ink `r`, the "Source"
/// layer's column `c` holds ink `c`.
fn grid_doc(depth: u32) -> *mut PrecisionDocument {
    let n = GRID.len() as u32;
    let mut doc = backdrop(n, n, depth, &[[0.0; 4]]);
    let band = |row: bool, k: u32| -> Vec<u16> {
        (0..n * n)
            .map(|p| {
                let at = if row { p / n } else { p % n };
                if at == k {
                    65535
                } else {
                    0
                }
            })
            .collect()
    };
    for r in 0..n {
        let ink = grid_ink(r as usize, depth);
        // The fill is paper already.
        if ink != [0.0; 4] {
            doc = paint(doc, 0, ink, &band(true, r));
        }
    }
    doc = layer(doc, "Source", grid_ink(0, depth), &band(false, 0));
    for c in 1..n {
        doc = paint(doc, 1, grid_ink(c as usize, depth), &band(false, c));
    }
    doc
}

/// The stored codes of layer `index` at `(x, y)` at `code_max`.
fn codes(doc: *const PrecisionDocument, index: usize, x: u32, y: u32, code_max: f64) -> [f64; 4] {
    let s = sample(doc, index, x, y).expect("a sample");
    [0, 1, 2, 3].map(|c| (s[c] * code_max).round())
}

/// The index `layer(i)` answers `name` at.
fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

fn blend_if(channel: &str, this: [u8; 4], under: [u8; 4]) -> Value {
    json!({"channel": channel, "this_layer": this, "underlying": under})
}

fn set_blend_if(
    doc: *const PrecisionDocument,
    index: usize,
    ranges: &Value,
) -> *mut PrecisionDocument {
    let out = edit(
        doc,
        &json!({"op":"layer_blend_if","version":1,"layer":index,"blend_if":ranges}),
    )
    .unwrap_or_else(|e| panic!("{ranges}: {e}"));
    assert!(!out.is_null(), "{ranges} changed nothing");
    out
}

/// Every grid pixel's flattened composite against the closed form within one
/// 16-bit code (the result's own rounding): at 8 bits that is the fitted
/// weight exactly — a weight off by the smallest ramp step moves a
/// two-code plate difference by more than 6 codes.
fn assert_grid(depth: u32) {
    let code_max = if depth == 8 { 255.0 } else { 65535.0 };
    let base = grid_doc(depth);
    let n = GRID.len() as u32;
    for channel in CHANNELS {
        for (this, under) in RAMPS {
            let ranges = blend_if(channel, this, under);
            let doc = set_blend_if(base, 1, &ranges);
            let samples = flat(doc);
            let mut opened = 0;
            for y in 0..n {
                for x in 0..n {
                    let cb = codes(base, 0, x, y, code_max);
                    let cs = codes(base, 1, x, y, code_max);
                    let w = weight(channel, this, under, cs, cb, code_max);
                    if w > 0.0 && w < 1.0 {
                        opened += 1;
                    }
                    let want = mixed(cb, cs, w);
                    let got = samples[(y * n + x) as usize];
                    for c in 0..4 {
                        // The flattened composite keeps 16-bit precision at
                        // either depth, so the 8-bit mix is compared ×257.
                        let want16 = (want[c] * 65535.0 / code_max).round();
                        let g = f64::from(got[c]);
                        assert!(
                            (g - want16).abs() <= 1.0,
                            "{depth}-bit {ranges} at ({x}, {y}) plate {c}: got {g}, want {want16} (w {w}, cs {cs:?}, cb {cb:?})"
                        );
                    }
                }
            }
            assert!(opened > 0, "{ranges}: no patch on a ramp's slope");
            free(doc);
        }
    }
    free(base);
}

// ------------------------------------------------------------- the tests --

#[test]
fn the_fitted_rule_holds_on_ink_at_eight_bits_exactly() {
    assert_grid(8);
}

#[test]
fn the_fitted_rule_holds_on_ink_at_sixteen_bits_within_a_code() {
    assert_grid(16);
}

#[test]
fn the_underlying_reads_the_backdrops_stored_code_at_eight_bits() {
    // Black L 99 (ink code 156) under a 70 % layer of Black L 100 (155):
    // the composite is L 99.7, stored as ink code 155, so L 100; a Black
    // Underlying edge at 100 (INC knees [99, 100]) weights the top layer 1,
    // where the unrounded composite would weight it 0.7.
    let (w, h) = (4, 2);
    let all = vec![65535u16; (w * h) as usize];
    let k = |code: f64| [0.0, 0.0, 0.0, code * 100.0 / 255.0];
    let doc = backdrop(w, h, 8, &[k(156.0)]);
    let doc = layer(doc, "Mid", k(155.0), &all);
    let doc = with_opacity(doc, 1, 0.7);
    let top = [200.0, 30.0, 0.0, 0.0].map(|c: f64| c * 100.0 / 255.0);
    let doc = layer(doc, "Top", top, &hard_rect(w, h, (0, 0, 2, h)));
    let shown = set_blend_if(doc, 2, &blend_if("black", FULL, [100, 100, 255, 255]));
    let samples = flat(shown);
    assert_eq!(
        samples[0][..4],
        [200 * 257, 30 * 257, 0, 0],
        "weight 1 over the stored code"
    );
    // Outside it: the 70 % composite itself, 155.3 codes.
    assert!(
        samples[3][..3] == [0, 0, 0] && samples[3][3].abs_diff(39912) <= 1,
        "outside the layer: {:?}",
        samples[3]
    );
    free(shown);
    free(doc);
}

#[test]
fn a_sixteen_bit_gray_ramp_splits_backdrops_that_share_an_eight_bit_gray() {
    // K-only backdrops: Gray is 65535 − k. 25400 and 25560 are both 8-bit
    // gray 99, and an Underlying edge at 100 (INC knees 99·257 = 25443,
    // 100·257 = 25700) weights them 0 and 117/257.
    let (w, h) = (2, 1);
    let ks = [65535.0 - 25400.0, 65535.0 - 25560.0];
    let mut doc = backdrop(w, h, 16, &[[0.0; 4]]);
    for (x, kc) in ks.iter().enumerate() {
        let coverage: Vec<u16> = (0..w)
            .map(|p| if p as usize == x { 65535 } else { 0 })
            .collect();
        doc = paint(doc, 0, [0.0, 0.0, 0.0, kc * 100.0 / 65535.0], &coverage);
    }
    for (x, kc) in ks.iter().enumerate() {
        assert_eq!(codes(doc, 0, x as u32, 0, 65535.0)[3], *kc, "stored k");
        assert_eq!(((65535.0 - kc) / 257.0_f64).round(), 99.0, "8-bit gray 99");
    }
    let source = [80.0, 0.0, 0.0, 0.0];
    let doc = layer(doc, "Source", source, &vec![65535; (w * h) as usize]);
    let ranges = blend_if("gray", FULL, [100, 100, 255, 255]);
    let shown = set_blend_if(doc, 1, &ranges);
    let samples = flat(shown);
    for x in 0..2u32 {
        let cb = codes(doc, 0, x, 0, 65535.0);
        let cs = codes(doc, 1, x, 0, 65535.0);
        let wgt = weight("gray", FULL, [100, 100, 255, 255], cs, cb, 65535.0);
        assert!(
            (wgt - [0.0, 117.0 / 257.0][x as usize]).abs() < 1e-9,
            "weight {wgt}"
        );
        let want = mixed(cb, cs, wgt);
        for c in 0..4 {
            assert!(
                (f64::from(samples[x as usize][c]) - want[c].round()).abs() <= 1.0,
                "x {x} plate {c}: {:?} vs {want:?}",
                samples[x as usize]
            );
        }
    }
    assert_ne!(samples[0], samples[1], "the ramp splits them");
    free(shown);
    free(doc);
}

fn styled_grid() -> (*mut PrecisionDocument, *mut PrecisionDocument) {
    let base = grid_doc(8);
    let doc = set_blend_if(base, 1, &blend_if("magenta", [0, 0, 60, 140], FULL));
    (base, doc)
}

#[test]
fn merge_down_bakes_it_and_duplicate_keeps_it() {
    let (base, doc) = styled_grid();
    let before = flat(doc);
    let merged = try_edit(doc, r#"{"op":"merge_down","layer":1}"#, &[]).unwrap();
    let merged_doc = unsafe { &*merged };
    assert_eq!(merged_doc.layer_count(), 1);
    assert!(merged_doc.layer(0).unwrap().blend_if().is_none());
    assert_eq!(flat(merged), before, "Merge Down bakes the weight");
    let mut err = ptr::null_mut();
    let duplicated = unsafe { dr_precision_doc_duplicate_layer(doc, 1, &mut err) };
    assert!(!duplicated.is_null(), "{}", take_err_string(err));
    let d = unsafe { &*duplicated };
    assert_eq!(d.layer_count(), 3);
    assert_eq!(
        d.layer(2).unwrap().blend_if(),
        d.layer(1).unwrap().blend_if()
    );
    assert!(d.layer(2).unwrap().blend_if().is_some());
    for handle in [base, doc, merged, duplicated] {
        free(handle);
    }
}

fn profile_edit(
    doc: *const PrecisionDocument,
    icc: Option<&[u8]>,
    request: &str,
) -> *mut PrecisionDocument {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_profile_edit(
            doc,
            icc.map_or(ptr::null(), |b| b.as_ptr()),
            icc.map_or(0, |b| b.len()),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    out
}

#[test]
fn convert_assign_and_depth_keep_it_on_a_raster_and_a_group() {
    let (base, raster) = styled_grid();
    let grouped = then(
        grid_doc(8),
        r#"{"op":"group_layers","layers":[1],"name":"G","pass_through":false}"#,
        &[],
    );
    let group = set_blend_if(
        grouped,
        index(grouped, "G"),
        &blend_if("cyan", FULL, [60, 140, 255, 255]),
    );
    let at = |doc: *const PrecisionDocument, name: &str| {
        unsafe { &*doc }.layer(index(doc, name)).unwrap().blend_if()
    };
    let other = press_profile_b("Press B");
    for (doc, name) in [(raster, "Source"), (group, "G")] {
        let want = at(doc, name);
        assert!(want.is_some());
        for (what, icc, request) in [
            (
                "Convert to Profile",
                Some(other.as_slice()),
                r#"{"op":"convert"}"#,
            ),
            (
                "Assign Profile",
                Some(other.as_slice()),
                r#"{"op":"assign"}"#,
            ),
            ("16 Bits", None, r#"{"op":"depth","depth":16}"#),
        ] {
            let out = profile_edit(doc, icc, request);
            assert_eq!(at(out, name), want, "{name}: {what}");
            if what == "16 Bits" {
                let back = profile_edit(out, None, r#"{"op":"depth","depth":8}"#);
                assert_eq!(at(back, name), want, "{name}: 8 Bits");
                free(back);
            }
            free(out);
        }
    }
    for handle in [base, raster, grouped, group] {
        free(handle);
    }
}

fn rgb_doc() -> *mut PrecisionDocument {
    let srgb = IccProfile::srgb();
    let image = PrecisionImage::from_rgba16(
        4,
        4,
        vec![[30000, 20000, 10000, 65535]; 16],
        srgb.clone(),
        None,
    )
    .unwrap();
    let layer = PrecisionDocumentLayer::raster("Background", image, (0, 0), 1.0, true).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            4,
            4,
            srgb,
            Metadata::default(),
            Resolution::default(),
            vec![layer],
        )
        .unwrap(),
    ))
}

fn to_cmyk(doc: *const PrecisionDocument) -> Result<*mut PrecisionDocument, String> {
    let profile = press_profile("Press Test");
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            doc,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn to_rgb(doc: *const PrecisionDocument) -> Result<*mut PrecisionDocument, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_cmyk_to_rgb(
            doc,
            ptr::null(),
            0,
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

#[test]
fn gray_carries_both_ways_and_red_and_cyan_block_by_name() {
    let rgb = rgb_doc();
    let gray = set_blend_if(rgb, 0, &blend_if("gray", [0, 0, 200, 255], FULL));
    let want = unsafe { &*gray }.layer(0).unwrap().blend_if();
    let cmyk = to_cmyk(gray).unwrap();
    assert_eq!(
        unsafe { &*cmyk }.layer(0).unwrap().blend_if(),
        want,
        "RGB → CMYK"
    );
    let back = to_rgb(cmyk).unwrap();
    assert_eq!(
        unsafe { &*back }.layer(0).unwrap().blend_if(),
        want,
        "CMYK → RGB"
    );
    let red = set_blend_if(rgb, 0, &blend_if("red", [0, 0, 200, 255], FULL));
    assert_eq!(
        to_cmyk(red).unwrap_err(),
        "“Background” is a layer with Blend If on the Red channel, which a CMYK document cannot hold yet; flatten the image to convert it"
    );
    let cyan = set_blend_if(cmyk, 0, &blend_if("cyan", [0, 0, 200, 255], FULL));
    let sentence = "“Background” is a layer with Blend If on the Cyan channel, which RGB Color cannot carry; flatten the image to convert it";
    assert_eq!(to_rgb(cyan).unwrap_err(), sentence);
    let mut err = ptr::null_mut();
    let text = unsafe {
        dr_precision_doc_convert_cmyk_to_rgb_plan(cyan, ptr::null(), 0, c"{}".as_ptr(), &mut err)
    };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let plan: Value =
        serde_json::from_str(unsafe { std::ffi::CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    assert_eq!(plan["refusal"], sentence);
    assert_eq!(plan["flatten_required"], true);
    for handle in [rgb, gray, cmyk, back, red, cyan] {
        free(handle);
    }
}

#[test]
fn a_blend_if_only_document_writes_word_86_and_reads_back_identical() {
    let (base, doc) = styled_grid();
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("blend_if.rz");
    let bytes = save(doc, &path);
    assert_eq!(cmyk_word(&bytes), 86);
    let back = open(&path).unwrap();
    assert_eq!(
        unsafe { &*back }.layer(1).unwrap().blend_if(),
        unsafe { &*doc }.layer(1).unwrap().blend_if()
    );
    assert_eq!(flat(back), flat(doc));
    for handle in [base, doc, back] {
        free(handle);
    }
}

#[test]
fn each_channel_round_trips_through_a_layered_psd() {
    let base = grid_doc(8);
    let dir = TempDir::new().unwrap();
    for (i, channel) in CHANNELS.iter().enumerate() {
        let (this, under) = RAMPS[i % RAMPS.len()];
        let doc = set_blend_if(base, 1, &blend_if(channel, this, under));
        let mut err = ptr::null_mut();
        let text = unsafe { dr_precision_doc_psd_export_report(doc, 0, 0, &mut err) };
        assert!(!text.is_null(), "{}", take_err_string(err));
        let report = unsafe { std::ffi::CStr::from_ptr(text) }
            .to_str()
            .unwrap()
            .to_string();
        unsafe { dr_string_free(text) };
        assert!(
            report.contains(&format!("Blend If on {channel}")),
            "{channel}: {report}"
        );
        let path = dir.path().join(format!("{channel}.psd"));
        let ok = unsafe {
            dr_precision_doc_save_psd(
                doc,
                ptr::null(),
                cpath(&path).as_ptr(),
                0,
                0,
                LIMIT,
                0,
                std::ptr::null(),
                &mut err,
            )
        };
        assert!(ok, "{channel}: {}", take_err_string(err));
        let back = open_psd(&path, 0).unwrap();
        let reopened = unsafe { &*back };
        let got = (0..reopened.layer_count())
            .map(|i| reopened.layer(i).unwrap())
            .find(|l| l.name() == "Source")
            .and_then(|l| l.blend_if());
        assert_eq!(
            got,
            Some(BlendIf {
                channel: channel_of(channel),
                this_layer: this,
                underlying: under,
                ..BlendIf::default()
            }),
            "{channel}"
        );
        assert_eq!(flat(back), flat(doc), "{channel}: the reopened composite");
        free(back);
        free(doc);
    }
    free(base);
}

fn channel_of(name: &str) -> BlendIfChannel {
    match name {
        "gray" => BlendIfChannel::Gray,
        "cyan" => BlendIfChannel::Cyan,
        "magenta" => BlendIfChannel::Magenta,
        "yellow" => BlendIfChannel::Yellow,
        _ => BlendIfChannel::Black,
    }
}

#[test]
fn red_green_blue_refuse_by_name_and_an_adjustment_and_a_pass_through_group_accept() {
    let base = grid_doc(8);
    for (channel, name) in [("red", "Red"), ("green", "Green"), ("blue", "Blue")] {
        let ranges = blend_if(channel, [0, 0, 60, 140], FULL);
        let want = format!("Blend If on the {name} channel is unavailable in a CMYK document");
        assert_eq!(
            refusal(
                base,
                &json!({"op":"layer_blend_if","version":1,"layer":1,"blend_if":ranges}).to_string(),
                &[]
            ),
            want
        );
        assert_eq!(
            refusal(
                base,
                &json!({"op":"layer_style","version":1,"layer":1,"style":{"blend_if":ranges}})
                    .to_string(),
                &[]
            ),
            want
        );
    }
    let ranges = blend_if("gray", [0, 0, 60, 140], FULL);
    // A CMYK adjustment carries a Blend If and a style.
    let levels = crate::cmyk_structure_fixture::with_levels(grid_doc(8), 1.0);
    for request in [
        json!({"op":"layer_blend_if","version":1,"layer":2,"blend_if":ranges}),
        json!({"op":"layer_style","version":1,"layer":2,"style":{"fill_opacity":0.5}}),
    ] {
        let accepted = try_edit(levels, &request.to_string(), &[])
            .unwrap_or_else(|e| panic!("{request}: {e}"));
        assert!(!accepted.is_null(), "{request} changed nothing");
        free(accepted);
    }
    let pass = then(
        grid_doc(8),
        r#"{"op":"group_layers","layers":[1],"name":"P","pass_through":true}"#,
        &[],
    );
    // A Pass Through group carries one too.
    let request =
        json!({"op":"layer_blend_if","version":1,"layer":index(pass, "P"),"blend_if":ranges});
    let accepted =
        try_edit(pass, &request.to_string(), &[]).unwrap_or_else(|e| panic!("{request}: {e}"));
    assert!(!accepted.is_null(), "{request} changed nothing");
    free(accepted);
    for handle in [base, levels, pass] {
        free(handle);
    }
}
