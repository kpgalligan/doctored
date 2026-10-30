//! #547 Lab phase 4b-ii: sampled Color Range on a Lab Color document,
//! through the FFI (`dr_precision_doc_select`). The rule is the one
//! `scripts/photoshop-probe/lab_select_take.py`'s take1 confirmed (its C2):
//! a pixel's distance from a sample is the largest of |ΔL\*|, |Δa\*| and
//! |Δb\*| in L\*a\*b\* units, the codes decoded as L\* = 100·v/65535 and
//! a\*, b\* = 255·v/65535 − 128; coverage is alpha times the ramp (full
//! within `tolerance`, then a linear shoulder `softness` wide) of the
//! nearest sample's distance. Bounds, declared before measuring, at 8 and
//! 16 bits:
//!
//! * (a) on a synthetic opaque Lab layer (every pixel the field (52, 10, −6)
//!   plus its own offsets, a few transparent), two cases — two samples at
//!   tolerance 6 and softness 10, one sample at tolerance 12.3 and softness
//!   0 — equal this file's own f64 oracle of the rule within one u16 code
//!   on every pixel, and the diagonal pixel (the field + 5 units in all
//!   three) is fully selected at tolerance 6, which ΔE76 (8.66) would not;
//! * (b) the Paint Bucket follows the Magic Wand's Lab rule
//!   (`lab_wand.rs`);
//! * (c) since phase 6b-i Color Range's tonal mode equals the ramp of the
//!   codes' own L\* outside `lower`…`upper` (times alpha) within one u16
//!   code, and its hue (reds, blues) and skin modes equal the same request
//!   on the document's 16-bit ProPhoto RGB conversion (relative, no black
//!   point compensation) within 257 coverage codes, at 8 and 16 bits; the
//!   Magic Wand runs and Select Similar refuses at 8 bits naming #683;
//! * (d) take1, read IN PLACE (`~/devel/doctored-fixtures/photoshop/
//!   lab-select-547/take1`; skipped with a note when absent, a panic unless
//!   its `decided.json` is `valid` with Color Range `C2`): each saved scene
//!   opened by `dr_precision_doc_open_lab`, Color Range from the case's
//!   sampler at softness 0 and the tolerance at C2's separating midpoint
//!   selects exactly the regions Photoshop selected (each cell's centre
//!   pixel on the scene's 12-pixel grid from (60, 6), 138 cells, and the
//!   click (21, 37); selected = the capture's coverage (100 − L\*)/100 at
//!   least 1/255), at Fuzziness 20, 50 and 100. Nothing from the take is
//!   committed.
use std::ffi::CString;
use std::path::PathBuf;
use std::ptr;

use doctored_core::ffi_lab::{dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_open_lab};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::{cpath, take_err_string};
use crate::gray_ops::{free, LIMIT};
use crate::lab_geometry::{document, ramp, twin_profile, DEPTHS, H, W};
use crate::lab_psd_fixture::read_lab_tiff;
use crate::lab_selection::select;

const FIELD: [f64; 3] = [52.0, 10.0, -6.0];

/// One L\*a\*b\* colour's u16 codes at `bits` (the 8-bit grid ×257).
fn codes(lab: [f64; 3], bits: u32) -> [u16; 3] {
    let wide = |unit: f64, top: f64| {
        let v = (unit / top).clamp(0.0, 1.0);
        if bits == 8 {
            ((v * 255.0).round() * 257.0) as u16
        } else {
            (v * 65535.0).round() as u16
        }
    };
    [
        wide(lab[0], 100.0),
        wide(lab[1] + 128.0, 255.0),
        wide(lab[2] + 128.0, 255.0),
    ]
}

/// The oracle's decoding of one code triple (module doc).
fn units(p: [u16; 3]) -> [f64; 3] {
    let v = |c: u16| f64::from(c) / 65535.0;
    [
        100.0 * v(p[0]),
        255.0 * v(p[1]) - 128.0,
        255.0 * v(p[2]) - 128.0,
    ]
}

/// The oracle's distance: the largest per-channel difference in units.
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|c| (a[c] - b[c]).abs()).fold(0.0, f64::max)
}

/// The oracle's coverage of one pixel (module doc).
fn oracle(pixel: [u16; 4], samples: &[[u16; 3]], tolerance: f64, softness: f64) -> f64 {
    if pixel[3] == 0 {
        return 0.0;
    }
    let colour = units([pixel[0], pixel[1], pixel[2]]);
    let d = samples
        .iter()
        .map(|s| distance(colour, units(*s)))
        .fold(f64::INFINITY, f64::min);
    let ramp = if d <= tolerance {
        1.0
    } else if softness == 0.0 {
        0.0
    } else {
        (1.0 - (d - tolerance) / softness).clamp(0.0, 1.0)
    };
    (ramp * f64::from(pixel[3])).round()
}

/// The synthetic layer of (a): pixel 0 the diagonal (field + 5 in all
/// three), every other the field plus its own half-unit offsets, every
/// eleventh transparent.
fn scene(bits: u32) -> Vec<[u16; 4]> {
    (0..W * H)
        .map(|i| {
            let lab = if i == 0 {
                [FIELD[0] + 5.0, FIELD[1] + 5.0, FIELD[2] + 5.0]
            } else {
                let step = |k: u32, m: u32| f64::from((i * k) % m) * 0.5 - f64::from(m / 4);
                [
                    FIELD[0] + step(7, 41),
                    FIELD[1] + step(11, 37),
                    FIELD[2] + step(13, 43),
                ]
            };
            let [l, a, b] = codes(lab, bits);
            let alpha = if i % 11 == 10 { 0 } else { 65535 };
            [l, a, b, alpha]
        })
        .collect()
}

fn lab_document(pixels: Vec<[u16; 4]>, bits: u32) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, twin_profile(), None).unwrap();
    let record = Layer::raster("Background", image, (0, 0), 1.0, true).unwrap();
    document(&[record], bits, true)
}

fn sampled(samples: &[[u16; 3]], tolerance: f64, softness: f64) -> Value {
    json!({"kind":"color_range","params":{"mode":"sampled","samples":samples,
        "tolerance":tolerance,"softness":softness}})
}

#[test]
fn lab_color_range_equals_the_per_channel_units_oracle() {
    for bits in DEPTHS {
        let pixels = scene(bits);
        let doc = lab_document(pixels.clone(), bits);
        let field = codes(FIELD, bits);
        let other = codes([60.0, -4.0, 12.0], bits);
        for (samples, tolerance, softness) in
            [(vec![field, other], 6.0, 10.0), (vec![field], 12.3, 0.0)]
        {
            let what = format!("{samples:?} at tolerance {tolerance}, {bits} bits");
            let got = select(doc, &sampled(&samples, tolerance, softness))
                .unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_eq!(got.len(), pixels.len(), "{what}");
            for (k, (&code, &pixel)) in got.iter().zip(&pixels).enumerate() {
                let want = oracle(pixel, &samples, tolerance, softness);
                assert!(
                    (f64::from(code) - want).abs() <= 1.0,
                    "{what}: pixel {k} {pixel:?}: {code} against {want}"
                );
            }
            if tolerance == 6.0 {
                assert_eq!(
                    got[0], 65535,
                    "{what}: the diagonal is in by the units rule"
                );
            }
        }
        free(doc);
    }
}

/// The tonal oracle of (c): `ramp((lower − L).max(L − upper).max(0), 0,
/// softness)` on the codes' L\* (module doc), times alpha.
fn tonal_oracle(pixel: [u16; 4], lower: f64, upper: f64, softness: f64) -> f64 {
    if pixel[3] == 0 {
        return 0.0;
    }
    let l = units([pixel[0], pixel[1], pixel[2]])[0];
    let d = (lower - l).max(l - upper).max(0.0);
    let ramp = if d <= 0.0 {
        1.0
    } else if softness == 0.0 {
        0.0
    } else {
        (1.0 - d / softness).clamp(0.0, 1.0)
    };
    (ramp * f64::from(pixel[3])).round()
}

/// `doc` converted to a 16-bit ProPhoto RGB document (Image ▸ Mode ▸ RGB
/// Color, relative colorimetric, no black point compensation).
fn prophoto_twin(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let prophoto = IccProfile::prophoto_rgb();
    let options =
        CString::new(json!({"intent":"relative","black_point_compensation":false}).to_string())
            .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            doc,
            prophoto.bytes().as_ptr(),
            prophoto.bytes().len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

#[test]
fn lab_color_range_runs_every_range_and_similar_waits_at_8_bits() {
    for bits in DEPTHS {
        let pixels = scene(bits);
        let doc = lab_document(pixels.clone(), bits);
        for (lower, upper, softness) in [(45.0, 55.0, 5.0), (20.0, 50.0, 0.0)] {
            let what = format!("tonal {lower}…{upper} soft {softness} at {bits} bits");
            let params = json!({"mode":"tonal","lower":lower,"upper":upper,"softness":softness});
            let got = select(doc, &json!({"kind":"color_range","params":params}))
                .unwrap_or_else(|e| panic!("{what}: {e}"));
            for (k, (&code, &pixel)) in got.iter().zip(&pixels).enumerate() {
                let want = tonal_oracle(pixel, lower, upper, softness);
                assert!(
                    (f64::from(code) - want).abs() <= 1.0,
                    "{what}: pixel {k} {pixel:?}: {code} against {want}"
                );
            }
        }
        // Hues and Skin Tones read the codes' 16-bit ProPhoto RGB encoding:
        // the same request on the converted document.
        let colourful = lab_document(ramp(W, H, bits, 3), bits);
        let rgb = prophoto_twin(colourful);
        for params in [
            json!({"mode":"hue","band":"reds"}),
            json!({"mode":"hue","band":"blues"}),
            json!({"mode":"skin"}),
            json!({"mode":"skin","invert":true}),
        ] {
            let what = format!("{params} at {bits} bits");
            let operation = json!({"kind":"color_range","params":params});
            let ours = select(colourful, &operation).unwrap_or_else(|e| panic!("{what}: {e}"));
            let theirs = select(rgb, &operation).unwrap_or_else(|e| panic!("{what}: {e}"));
            assert!(
                theirs.iter().any(|v| *v > 0),
                "{what}: the twin selects something"
            );
            for (k, (a, b)) in ours.iter().zip(&theirs).enumerate() {
                assert!(
                    (i32::from(*a) - i32::from(*b)).abs() <= 257,
                    "{what}: pixel {k}: {a} against {b}"
                );
            }
        }
        free(rgb);
        free(colourful);
        // The Magic Wand runs at both depths (`lab_wand.rs` holds its
        // oracle); #683: Select Similar waits at 8 bits only.
        let wand = json!({"kind":"magic_wand","x":3,"y":2,"tolerance":10,"contiguous":true});
        select(doc, &wand).unwrap_or_else(|e| panic!("the wand at {bits} bits: {e}"));
        if bits == 8 {
            assert_eq!(
                select(
                    doc,
                    &json!({"kind":"similar","tolerance":10,"contiguous":false})
                )
                .unwrap_err(),
                "select_similar is unavailable in an 8-bit Lab Color document until its Lab \
                 tolerance rule is measured (#683).",
                "select_similar at {bits} bits"
            );
        }
        free(doc);
    }
}

/// take1's folder, or None when this machine has no fixtures.
fn take1() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let folder = PathBuf::from(home).join("devel/doctored-fixtures/photoshop/lab-select-547/take1");
    folder.join("decided.json").is_file().then_some(folder)
}

/// Photoshop's regions (module doc, (d)): the 138 cells' centre pixels, then
/// the click.
fn regions() -> Vec<(u32, u32)> {
    (0..138u32)
        .map(|k| (60 + 12 * (k % 20) + 4, 6 + 12 * (k / 20) + 4))
        .chain([(21, 37)])
        .collect()
}

#[test]
fn lab_color_range_selects_photoshops_regions_on_take1() {
    let Some(folder) = take1() else {
        eprintln!("SKIP: lab-select-547/take1 is absent on this machine");
        return;
    };
    let decided: Value =
        serde_json::from_str(&std::fs::read_to_string(folder.join("decided.json")).unwrap())
            .unwrap();
    assert_eq!(decided["valid"], true, "take1 is not VALID");
    assert_eq!(
        decided["build"]["select_color_range"], "C2",
        "take1's build"
    );
    let rows = decided["color_range"].as_array().unwrap();
    assert_eq!(rows.len(), 6, "take1's Color Range cases");
    for row in rows {
        let name = row["name"].as_str().unwrap();
        let bits = row["bits"].as_u64().unwrap() as u32;
        let scene = folder.join(format!("jobs/lab{bits}/scene.tif"));
        let mut err = ptr::null_mut();
        let doc = unsafe {
            dr_precision_doc_open_lab(cpath(&scene).as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        };
        assert!(!doc.is_null(), "{name}: {}", take_err_string(err));
        let (w, h) = unsafe { &*doc }.dimensions();
        assert_eq!((w, h), (300, 120), "{name}: the scene");
        let sampler: Vec<f64> = row["sampler"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let sample = codes([sampler[0], sampler[1], sampler[2]], 16);
        let midpoint = row["candidates"]["C2"]["midpoint"].as_f64().unwrap();
        let got = select(doc, &sampled(&[sample], midpoint, 0.0))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let capture = std::fs::read(folder.join(format!("jobs/select/{name}.tif"))).unwrap();
        let capture = read_lab_tiff(&capture);
        assert_eq!(
            (capture.width, capture.height),
            (300, 120),
            "{name}: the capture"
        );
        let top = if capture.bits == 8 { 255.0 } else { 65535.0 };
        let mut mismatched = Vec::new();
        for (x, y) in regions() {
            let i = (y * w + x) as usize;
            let lightness = 100.0 * f64::from(capture.planes[0][i]) / top;
            let photoshop = (100.0 - lightness) / 100.0 >= 1.0 / 255.0;
            if photoshop != (got[i] > 0) {
                mismatched.push((x, y, photoshop));
            }
        }
        assert!(
            mismatched.is_empty(),
            "{name}: regions where Doctored differs from Photoshop: {mismatched:?}"
        );
        free(doc);
    }
}
