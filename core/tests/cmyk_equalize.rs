//! Image ▸ Adjustments ▸ Equalize on a CMYK document (#1), through the
//! selected edit, against an independent f64 histogram equalization written
//! here: ONE table from the summed histograms of the four `L = 1 − ink`
//! plates (256 levels in an 8-bit document, `round(L / 257)`; 65536 in a
//! 16-bit one), each sample weighted by coverage × alpha, applied on `L` and
//! lerped on ink by the coverage under `selected`, the whole layer under
//! `entire` or with no selection. An offset layer inside a group; an
//! identity answers NULL; a group refuses by name.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_cmyk_colour::dr_precision_doc_cmyk_profile_edit;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, free, then, varied, RgbLayer, LIMIT};
use crate::common::take_err_string;

const W: u32 = 20;
const H: u32 = 16;
const TARGET: usize = 1;

fn document() -> *mut PrecisionDocument {
    let doc = cmyk_layers(
        W,
        H,
        vec![
            RgbLayer {
                name: "Background",
                w: W,
                h: H,
                offset: (0, 0),
                pixels: varied(W, H, 1),
            },
            RgbLayer {
                name: "Offset",
                w: 13,
                h: 10,
                offset: (5, 4),
                pixels: varied(13, 10, 8),
            },
        ],
    );
    then(
        doc,
        r#"{"op":"group_layers","layers":[1],"name":"Group","pass_through":false}"#,
        &[],
    )
}

fn eight(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let text = CString::new(r#"{"op":"depth","depth":8}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_profile_edit(
            doc,
            ptr::null(),
            0,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "8 bits: {}", take_err_string(err));
    out
}

fn selected(
    doc: *const PrecisionDocument,
    request: &Value,
    coverage: &[u16],
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            text.as_ptr(),
            if coverage.is_empty() {
                ptr::null()
            } else {
                coverage.as_ptr()
            },
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        return Err(take_err_string(err));
    }
    Ok(out)
}

/// A selection: full on the left, a ramp, nothing on the right.
fn selection() -> Vec<u16> {
    (0..W * H)
        .map(|i| match i % W {
            x if x < 9 => 65535,
            x if x < 14 => (65535 * (14 - x) / 6) as u16,
            _ => 0,
        })
        .collect()
}

fn table(bins: &[f64]) -> Vec<f64> {
    let first = bins.iter().position(|&w| w > 0.0).unwrap();
    let total: f64 = bins.iter().sum();
    let floor = bins[first];
    let max = (bins.len() - 1) as f64;
    let mut cdf = 0.0;
    bins.iter()
        .map(|&w| {
            cdf += w;
            (max * ((cdf - floor) / (total - floor)).clamp(0.0, 1.0)).round()
        })
        .collect()
}

#[test]
fn equalize_on_ink_matches_the_oracle_at_8_and_16_bits_in_each_scope() {
    let sixteen = document();
    let eight_bit = eight(sixteen);
    for (doc, depth) in [(sixteen, 16), (eight_bit, 8)] {
        let layer = unsafe { &*doc }.layer(TARGET).unwrap();
        let (w, _) = layer.cmyk_source().unwrap().dimensions();
        let (ox, oy) = layer.offset();
        let source = unsafe { &*doc }.cmyk_layer_samples(TARGET).unwrap();
        let levels = if depth == 8 { 256 } else { 65536 };
        let level = |ink: u16| -> usize {
            let l = 65535 - usize::from(ink);
            if depth == 8 {
                (l as f64 / 257.0).round() as usize
            } else {
                l
            }
        };
        let canvas = |i: usize, coverage: &[u16]| -> f64 {
            if coverage.is_empty() {
                return 1.0;
            }
            let (x, y) = (i as i32 % w as i32 + ox, i as i32 / w as i32 + oy);
            if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
                0.0
            } else {
                f64::from(coverage[(y as u32 * W + x as u32) as usize]) / 65535.0
            }
        };
        let sel = selection();
        for (coverage, scope) in [
            (Vec::new(), "selected"),
            (sel.clone(), "selected"),
            (sel.clone(), "entire"),
        ] {
            let mut bins = vec![0.0f64; levels];
            for (i, s) in source.iter().enumerate() {
                let k = canvas(i, &coverage) * f64::from(s[4]) / 65535.0;
                for &ink in &s[..4] {
                    if k > 0.0 {
                        bins[level(ink)] += k;
                    }
                }
            }
            let lut = table(&bins);
            let scale = if depth == 8 { 257.0 } else { 1.0 };
            let request = json!({"layer":TARGET,"op":"equalize","params":{"scope":scope}});
            let out = selected(doc, &request, &coverage).unwrap();
            assert!(!out.is_null(), "{depth} {scope}");
            let got = unsafe { &*out }.cmyk_layer_samples(TARGET).unwrap();
            for (i, s) in source.iter().enumerate() {
                let t = if scope == "entire" {
                    1.0
                } else {
                    canvas(i, &coverage)
                };
                for p in 0..5 {
                    let want = if p == 4 || s[4] == 0 || t == 0.0 {
                        f64::from(s[p])
                    } else {
                        let ink = 65535.0 - lut[level(s[p])] * scale;
                        (f64::from(s[p]) + t * (ink - f64::from(s[p]))).round()
                    };
                    assert!(
                        (f64::from(got[i][p]) - want).abs() <= 1.0,
                        "{depth} {scope} pixel {i} plate {p}: {} vs {want}",
                        got[i][p]
                    );
                }
            }
            // The background is never touched.
            assert_eq!(
                unsafe { &*out }.cmyk_layer_samples(0),
                unsafe { &*doc }.cmyk_layer_samples(0)
            );
            free(out);
        }
    }
    free(eight_bit);
    free(sixteen);
}

#[test]
fn an_identity_equalize_is_null_and_a_group_refuses() {
    let doc = document();
    // Solid ink at 100 % or 0 % on every plate: L at the two ends only.
    let n = (W * H) as usize;
    let solid = then(
        doc,
        r#"{"op":"paint_blend","layer":1,"ink":[1,0,1,0,1],"opacity":1}"#,
        &vec![65535u16; n],
    );
    let out = selected(solid, &json!({"layer":TARGET,"op":"equalize"}), &[]).unwrap();
    assert!(out.is_null(), "nothing changes");
    assert_eq!(
        selected(solid, &json!({"layer":2,"op":"equalize"}), &[]).unwrap_err(),
        "Equalize needs a raster layer: a group has no pixels of its own"
    );
    free(solid);
}
