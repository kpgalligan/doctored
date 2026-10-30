//! Select ▸ Color Range's hue presets (Reds … Magentas), Invert and Localized
//! Color Clusters, through `dr_selection_color_range` at 8 bits and
//! `dr_precision_doc_select`'s `color_range` kind at 16 and 32 bits and on a
//! CMYK document's view. The oracles are this file's own restatement of the
//! rules in `color_range_band`'s module doc, written without the core:
//!
//! * a band's coverage is the pixel's share of that one hue-wheel slot — a
//!   primary (R, G, B) is `max(0, c − max(other two))`, a secondary (Y, C, M)
//!   is `max(0, min(its two) − the third)` — over the encoded codes, times
//!   `2 − max(r, g, b)` (take2's shipped `D2M`), clamped to 1;
//! * Invert is `1 − coverage` before the source alpha scales it;
//! * a cluster point weights its sample's ramp by `1 − d/R` (clamped), `d`
//!   the Euclidean pixel-centre distance and `R = range/100 · max(w, h)`;
//!   `R = 0` keeps only the point's own pixel;
//! * Skin Tones is take3's `LAB (70, 18, 19) · RAMP (0, 18, 0.25, 0)`: ΔE76
//!   in the built-in sRGB's D50 Lab (restated below from its published
//!   s15Fixed16 columns and 1024-entry curve), full within 18, then a linear
//!   shoulder `max(0.01, 0.25·F)` wide.
//!
//! The Photoshop 26.10.0 fixture (`fixtures/photoshop_color_range`, take1 of
//! `scripts/photoshop-probe/color_range_671_take.py`, take2 of
//! `color_range_671_take2.py` and take3 of `color_range_671_take3.py`) pins
//! the bands' residual on both takes, Photoshop's own Invert, the cluster's
//! UNMEASURED reason and Skin Tones' MISS residual.
use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_open};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit_plan, dr_precision_doc_select};
use doctored_core::ffi_selection_tools::dr_selection_color_range;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, take_err_string};
use crate::float_fixture::float_doc;
use crate::native_scalar;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const BANDS: [&str; 6] = ["reds", "yellows", "greens", "cyans", "blues", "magentas"];

/// `got` is `want` rounded, or either neighbour when `want` sits on a half
/// (the core's f32 and this f64 oracle may break that tie apart).
fn rounds_to(got: f64, want: f64) -> bool {
    if (want.fract() - 0.5).abs() < 1e-4 {
        got == want.floor() || got == want.ceil()
    } else {
        got == want.round()
    }
}

/// The oracle band coverage 0…1 of `rgb` (0…1), module doc.
fn band(index: usize, rgb: [f64; 3]) -> f64 {
    let [r, g, b] = rgb;
    let v = match index {
        0 => r - g.max(b),
        2 => g - r.max(b),
        4 => b - r.max(g),
        1 => r.min(g) - b,
        3 => g.min(b) - r,
        _ => r.min(b) - g,
    };
    (v.max(0.0) * (2.0 - r.max(g).max(b))).min(1.0)
}

/// The oracle cluster weight, module doc.
fn weight(d: f64, radius: f64) -> f64 {
    if radius <= 0.0 {
        return if d <= 0.0 { 1.0 } else { 0.0 };
    }
    (1.0 - d / radius).clamp(0.0, 1.0)
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// `w × h` random straight RGBA8, every eighth pixel at alpha 128 and every
/// thirteenth transparent.
fn chart(w: u32, h: u32, salt: u64) -> Vec<u8> {
    let mut state = 0x671 ^ salt;
    let mut out = Vec::new();
    for i in 0..w * h {
        let v = splitmix(&mut state);
        let alpha = if i % 13 == 0 {
            0
        } else if i % 8 == 0 {
            128
        } else {
            255
        };
        out.extend([v as u8, (v >> 8) as u8, (v >> 16) as u8, alpha]);
    }
    out
}

/// `dr_selection_color_range` over `pixels`, or its error.
fn byte_range(pixels: &[u8], w: u32, h: u32, params: &Value) -> Result<Vec<u8>, String> {
    let text = CString::new(params.to_string()).unwrap();
    unsafe {
        let source = dr_image_from_rgba(pixels.as_ptr(), w, h);
        let doc = dr_doc_from_image(source);
        let mut out = vec![77u8; (w * h) as usize];
        let mut err = ptr::null_mut();
        let ok = dr_selection_color_range(
            doc,
            source,
            text.as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            &mut err,
        );
        dr_doc_free(doc);
        dr_image_free(source);
        if ok {
            Ok(out)
        } else {
            assert!(out.iter().all(|&v| v == 77), "a refusal writes nothing");
            Err(take_err_string(err))
        }
    }
}

/// `dr_precision_doc_select` of a composite `color_range` request, or its error.
fn native_range(doc: *const PrecisionDocument, params: &Value) -> Result<Vec<u16>, String> {
    let request = json!({"op":"selection","operation":{"kind":"color_range","params":params}});
    let text = CString::new(request.to_string()).unwrap();
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
fn every_preset_is_the_pixels_share_of_its_hue_slot_at_8_bits() {
    let (w, h) = (37u32, 23u32);
    let pixels = chart(w, h, 1);
    for (index, name) in BANDS.iter().enumerate() {
        let got = byte_range(&pixels, w, h, &json!({"mode":"hue","band":name})).unwrap();
        for (i, p) in pixels.chunks_exact(4).enumerate() {
            let rgb = [p[0], p[1], p[2]].map(|v| f64::from(v) / 255.0);
            let want = band(index, rgb) * f64::from(p[3]);
            assert!(
                rounds_to(f64::from(got[i]), want),
                "{name} pixel {i} {p:?}: {} vs {want}",
                got[i]
            );
        }
    }
}

#[test]
fn the_presets_split_a_two_slot_colour_and_leave_greys_out() {
    let pixels = [
        255, 0, 0, 255, // red
        255, 128, 0, 255, // orange: 127 of reds, 128 of yellows, each × (2 − 1)
        0, 255, 255, 255, // cyan
        128, 128, 128, 255, // grey
        255, 200, 200, 255, // pastel red: its neutral part is no hue
        128, 0, 0, 255, // dark red: 128 × (2 − 128/255)
    ];
    let reds = byte_range(&pixels, 6, 1, &json!({"mode":"hue","band":"reds"})).unwrap();
    let yellows = byte_range(&pixels, 6, 1, &json!({"mode":"hue","band":"yellows"})).unwrap();
    let cyans = byte_range(&pixels, 6, 1, &json!({"mode":"hue","band":"cyans"})).unwrap();
    // A full-value pixel keeps its amount (× 1); 128 × (2 − 128/255) = 191.75.
    assert_eq!(reds, [255, 127, 0, 0, 55, 192]);
    assert_eq!(yellows, [0, 128, 0, 0, 0, 0]);
    assert_eq!(cyans, [0, 0, 255, 0, 0, 0]);
}

#[test]
fn invert_flips_coverage_before_alpha_in_every_mode() {
    let (w, h) = (19u32, 11u32);
    let pixels = chart(w, h, 2);
    for params in [
        json!({"mode":"hue","band":"greens"}),
        json!({"mode":"sampled","samples":[[200,60,60],[30,90,200]],"tolerance":40,"softness":30}),
        json!({"mode":"tonal","lower":20,"upper":60,"softness":15}),
        json!({"mode":"skin","fuzziness":120}),
    ] {
        let plain = byte_range(&pixels, w, h, &params).unwrap();
        let mut inverted = params.clone();
        inverted["invert"] = json!(true);
        let flipped = byte_range(&pixels, w, h, &inverted).unwrap();
        for (i, p) in pixels.chunks_exact(4).enumerate() {
            match p[3] {
                0 => assert_eq!((plain[i], flipped[i]), (0, 0), "{params} {i}"),
                255 => assert!(
                    (i32::from(plain[i]) + i32::from(flipped[i]) - 255).abs() <= 1,
                    "{params} {i}: {} + {}",
                    plain[i],
                    flipped[i]
                ),
                a => assert!(
                    (i32::from(plain[i]) + i32::from(flipped[i]) - i32::from(a)).abs() <= 1,
                    "{params} {i} at alpha {a}"
                ),
            }
        }
        let mut explicit = params.clone();
        explicit["invert"] = json!(false);
        assert_eq!(byte_range(&pixels, w, h, &explicit).unwrap(), plain);
    }
}

#[test]
fn a_cluster_point_weights_its_own_samples_ramp_by_distance() {
    // Left half A, right half B; sample A pairs with a point on the left,
    // sample B with one on the right. Each pixel takes only its own colour's
    // sample, weighted by the distance to THAT sample's point.
    let (w, h) = (40u32, 20u32);
    let (a, b) = ([200u8, 60, 60], [60u8, 200, 60]);
    let mut pixels = Vec::new();
    for _y in 0..h {
        for x in 0..w {
            let c = if x < 20 { a } else { b };
            pixels.extend([c[0], c[1], c[2], 255]);
        }
    }
    let points = [(6.0, 9.0), (31.0, 4.0)];
    for range in [10.0, 25.0, 60.0, 100.0] {
        let got = byte_range(
            &pixels,
            w,
            h,
            &json!({"mode":"sampled","samples":[a,b],"tolerance":5,"softness":0,
                "points":[[6,9],[31,4]],"cluster_range":range}),
        )
        .unwrap();
        let radius = range / 100.0 * 40.0;
        for (i, v) in got.iter().enumerate() {
            let (x, y) = ((i as u32 % w) as f64, (i as u32 / w) as f64);
            let (px, py) = points[usize::from(x >= 20.0)];
            let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
            let want = weight(d, radius) * 255.0;
            assert!(
                (f64::from(*v) - want).abs() <= 1.0,
                "range {range} ({x}, {y}): {v} vs {want}"
            );
        }
    }
    // Without points the same samples select both halves fully.
    let global = byte_range(
        &pixels,
        w,
        h,
        &json!({"mode":"sampled","samples":[a,b],"tolerance":5,"softness":0}),
    )
    .unwrap();
    assert!(global.iter().all(|&v| v == 255));
}

#[test]
fn cluster_range_zero_keeps_only_the_points_own_pixel_when_it_matches() {
    let (w, h) = (9u32, 7u32);
    let mut pixels = Vec::new();
    for i in 0..w * h {
        let c = if i == 3 * w + 2 {
            [10u8, 10, 200]
        } else {
            [200, 60, 60]
        };
        pixels.extend([c[0], c[1], c[2], 255]);
    }
    let run = |x: u32, y: u32| {
        byte_range(
            &pixels,
            w,
            h,
            &json!({"mode":"sampled","samples":[[200,60,60]],"tolerance":5,"softness":0,
                "points":[[x,y]],"cluster_range":0}),
        )
        .unwrap()
    };
    let got = run(5, 4);
    for (i, v) in got.iter().enumerate() {
        assert_eq!(*v, if i == (4 * w + 5) as usize { 255 } else { 0 }, "{i}");
    }
    // The point's own pixel is another colour: nothing is selected.
    assert!(run(2, 3).iter().all(|&v| v == 0));
}

#[test]
fn malformed_presets_and_clusters_refuse_by_sentence() {
    let pixels = chart(4, 3, 3);
    let refuse = |params: Value| byte_range(&pixels, 4, 3, &params).unwrap_err();
    for params in [
        json!({"mode":"hue","band":"reds","tolerance":20}),
        json!({"mode":"hue","band":"reds","samples":[[1,2,3]]}),
        json!({"mode":"hue","band":"reds","softness":5}),
        json!({"mode":"hue","band":"reds","lower":5}),
        json!({"mode":"hue","band":"reds","upper":5}),
        json!({"mode":"hue","band":"reds","points":[[0,0]]}),
        json!({"mode":"hue","band":"reds","cluster_range":5}),
    ] {
        assert_eq!(
            refuse(params),
            "hue Color Range accepts only band and invert"
        );
    }
    for params in [
        json!({"mode":"hue"}),
        json!({"mode":"hue","band":"oranges"}),
        json!({"mode":"hue","band":3}),
    ] {
        assert_eq!(
            refuse(params),
            "Color Range band must be reds, yellows, greens, cyans, blues or magentas"
        );
    }
    let pairing = "Color Range points must pair one canvas pixel with each sample";
    for points in [
        json!([[0, 0], [1, 1]]),
        json!([]),
        json!([[0]]),
        json!([[0.5, 1]]),
        json!("here"),
    ] {
        assert_eq!(
            refuse(json!({"mode":"sampled","samples":[[1,2,3]],"points":points})),
            pairing
        );
    }
    for point in [json!([4, 0]), json!([0, 3]), json!([-1, 0])] {
        assert_eq!(
            refuse(json!({"mode":"sampled","samples":[[1,2,3]],"points":[point]})),
            "Color Range point lies outside the canvas"
        );
    }
    assert_eq!(
        refuse(json!({"mode":"sampled","samples":[[1,2,3]],"cluster_range":50})),
        "cluster_range needs points"
    );
    assert_eq!(
        refuse(json!({"mode":"sampled","samples":[[1,2,3]],"points":[[0,0]],"cluster_range":101})),
        "cluster_range must be between 0 and 100"
    );
    assert_eq!(
        refuse(json!({"mode":"sampled","samples":[[1,2,3]],"invert":1})),
        "invert must be a boolean"
    );
    assert_eq!(
        refuse(json!({"mode":"sampled","samples":[[1,2,3]],"band":"reds"})),
        "Color Range band needs mode hue"
    );
    assert_eq!(
        refuse(json!({"mode":"tonal","points":[[0,0]]})),
        "Color Range points need mode sampled"
    );
    // An unknown or missing mode refuses before the band and fuzziness checks.
    for params in [
        json!({"mode":"spectral"}),
        json!({"mode":"spectral","fuzziness":40}),
        json!({"mode":"spectral","band":"reds"}),
        json!({"mode":3,"band":"reds"}),
        json!({"fuzziness":40}),
        json!({"band":"reds"}),
        json!({"invert":true}),
    ] {
        assert_eq!(
            refuse(params),
            "Color Range mode must be sampled, tonal, hue or skin"
        );
    }
}

#[test]
fn sixteen_bit_presets_and_clusters_read_their_own_codes() {
    let (w, h) = (23u32, 17u32);
    let mut state = 0x16;
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let v = splitmix(&mut state);
            let a = if i % 9 == 0 { 30000 } else { 65535 };
            [v as u16, (v >> 16) as u16, (v >> 32) as u16, a]
        })
        .collect();
    let d = native_scalar::doc(w, h, pixels.clone());
    for (index, name) in BANDS.iter().enumerate() {
        let got = native_range(&d, &json!({"mode":"hue","band":name,"invert":index == 2})).unwrap();
        for (i, p) in pixels.iter().enumerate() {
            let rgb = [p[0], p[1], p[2]].map(|v| f64::from(v) / 65535.0);
            let mut c = band(index, rgb);
            if index == 2 {
                c = 1.0 - c;
            }
            let want = c * f64::from(p[3]);
            assert!(
                (f64::from(got[i]) - want).abs() <= 1.0,
                "{name} {i}: {} vs {want}",
                got[i]
            );
        }
    }
    // A uniform field of the sample's own colour: the cluster weight alone.
    let field = native_scalar::doc(w, h, vec![[50000, 12000, 9000, 65535]; (w * h) as usize]);
    let got = native_range(
        &field,
        &json!({"mode":"sampled","samples":[[50000,12000,9000]],"points":[[4,13]],"cluster_range":40}),
    )
    .unwrap();
    let radius = 0.4 * 23.0;
    for (i, v) in got.iter().enumerate() {
        let (x, y) = ((i as u32 % w) as f64, (i as u32 / w) as f64);
        let d = ((x - 4.0).powi(2) + (y - 13.0).powi(2)).sqrt();
        let want = weight(d, radius) * 65535.0;
        assert!(
            (f64::from(*v) - want).abs() <= 2.0,
            "({x}, {y}) {v} vs {want}"
        );
    }
    assert_eq!(
        native_range(&d, &json!({"mode":"hue","band":"teal"})).unwrap_err(),
        "Color Range band must be reds, yellows, greens, cyans, blues or magentas"
    );
    assert_eq!(
        native_range(
            &d,
            &json!({"mode":"sampled","samples":[[1,2,3]],"points":[[23,0]]})
        )
        .unwrap_err(),
        "Color Range point lies outside the canvas"
    );
}

/// The sRGB encode, written here (IEC 61966-2-1).
fn srgb_encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

#[test]
fn a_float_document_reads_its_display_codes_with_white_held() {
    let samples: Vec<[f32; 4]> = vec![
        [1.0, 0.0, 0.0, 1.0],
        [2.5, 0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0, 1.0],
        [0.0, 1.0, 1.0, 1.0],
        [1.0, 0.2, 0.0, 1.0],
        [0.4, 0.4, 0.4, 1.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, -0.5, 3.0, 1.0],
    ];
    let w = samples.len() as u32;
    let d = float_doc(w, 1, &IccProfile::srgb(), std::slice::from_ref(&samples));
    for (index, name) in BANDS.iter().enumerate() {
        let got = native_range(&d, &json!({"mode":"hue","band":name})).unwrap();
        for (i, p) in samples.iter().enumerate() {
            let rgb = [p[0], p[1], p[2]].map(|v| srgb_encode(f64::from(v)));
            let want = band(index, rgb) * f64::from(p[3].clamp(0.0, 1.0)) * 65535.0;
            assert!(
                (f64::from(got[i]) - want).abs() <= 3.0,
                "{name} {i} {p:?}: {} vs {want}",
                got[i]
            );
        }
    }
}

/// An opaque 8-bit CMYK document of `samples` (C, M, Y, K codes, 16-bit
/// scale) in the synthetic press profile, opened through the typed open.
fn cmyk_doc(dir: &TempDir, w: u32, h: u32, samples: &[[u16; 4]]) -> *mut PrecisionDocument {
    let data: Vec<u8> = samples
        .iter()
        .flatten()
        .map(|v| ((u32::from(*v) + 128) / 257) as u8)
        .collect();
    let path = dir.path().join("range.tif");
    let icc = press_profile("Press Select");
    std::fs::write(&path, cmyk_tiff(w, h, 8, 4, 0, &data, Some(&icc))).unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open: {}", take_err_string(err));
    doc
}

/// The CMYK document's view as an RGB16 document (Display P3, its own
/// profile), the `cmyk_select.rs` pattern.
fn view_document(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let image: PrecisionImage = unsafe { &*doc }.render(LIMIT, 0, None).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ))
}

#[test]
fn a_cmyk_documents_presets_and_clusters_are_the_same_request_on_its_view() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (16u32, 12u32);
    let mut state = 0xC4;
    let samples: Vec<[u16; 4]> = (0..w * h)
        .map(|i| match i % 4 {
            0 => [0, 65535, 65535, 0],
            1 => [65535, 0, 0, 0],
            _ => {
                let v = splitmix(&mut state);
                [
                    v as u16,
                    (v >> 16) as u16,
                    (v >> 32) as u16,
                    (v >> 50) as u16,
                ]
            }
        })
        .collect();
    let doc = cmyk_doc(&dir, w, h, &samples);
    let rgb = view_document(doc);
    let mut cases: Vec<Value> = BANDS
        .iter()
        .map(|b| json!({"mode":"hue","band":b}))
        .collect();
    cases.push(json!({"mode":"hue","band":"reds","invert":true}));
    cases.push(
        json!({"mode":"sampled","samples":[[50000,12000,9000],[3000,40000,52000]],
        "tolerance":30,"softness":20,"points":[[2,3],[12,9]],"cluster_range":45}),
    );
    cases.push(json!({"mode":"sampled","samples":[[50000,12000,9000]],"invert":true}));
    cases.push(json!({"mode":"skin"}));
    cases.push(json!({"mode":"skin","fuzziness":160,"invert":true}));
    for params in &cases {
        assert_eq!(
            native_range(doc, params).unwrap(),
            native_range(rgb, params).unwrap(),
            "{params}"
        );
    }
    // Press red reads as Reds on the view, press cyan does not.
    let reds = native_range(doc, &cases[0]).unwrap();
    assert!(reds[0] > reds[1], "{} vs {}", reds[0], reds[1]);
    unsafe {
        dr_precision_doc_free(rgb);
        dr_precision_doc_free(doc);
    }
}

fn fixture_folder() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/photoshop_color_range")
}

/// The red plane of a fixture PNG.
fn plane(name: &str) -> (u32, u32, Vec<u8>) {
    let image = image::open(fixture_folder().join(name)).unwrap().to_rgba8();
    let (w, h) = image.dimensions();
    (w, h, image.pixels().map(|p| p.0[0]).collect())
}

/// Photoshop's take1 masks: Doctored equals the shipped `D2M` prediction
/// (this file's oracle) and lies within the residual take2 recorded for it
/// against Photoshop (exact, pinned so it cannot grow); Photoshop's
/// Invert is its own Reds flipped exactly; and its Localized Color Clusters
/// mask is uniform, the reason the cluster is UNMEASURED.
#[test]
fn photoshops_take1_masks_pin_the_shipped_bands_and_the_unmeasured_cluster() {
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(fixture_folder().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["bands"]["member"], json!(["D2M"]));
    assert_eq!(manifest["bands"]["verdict"], "SHIP (take2)");
    let judged = manifest["bands"]["judge"].as_array().unwrap();
    let mut checked = 0;
    for case in manifest["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        match case["kind"].as_str().unwrap() {
            "preset" => {
                let (w, h, rgb) = {
                    let image = image::open(fixture_folder().join(case["input"].as_str().unwrap()))
                        .unwrap()
                        .to_rgba8();
                    let (w, h) = image.dimensions();
                    (w, h, image.into_raw())
                };
                let index = BANDS
                    .iter()
                    .position(|b| *b == case["band_name"].as_str().unwrap())
                    .unwrap();
                let invert = case["invert"].as_bool().unwrap();
                let got = byte_range(
                    &rgb,
                    w,
                    h,
                    &json!({"mode":"hue","band":BANDS[index],"invert":invert}),
                )
                .unwrap();
                let (pw, ph, photoshop) = plane(case["photoshop"].as_str().unwrap());
                assert_eq!((pw, ph), (w, h), "{name}");
                // `flips`: ties the core broke down where the take's
                // prediction (half up) broke them up; each moves the
                // residual by at most one code.
                let (mut max, mut sum, mut flips) = (0u8, 0u64, 0u64);
                for (i, p) in rgb.chunks_exact(4).enumerate() {
                    let c = band(index, [p[0], p[1], p[2]].map(|v| f64::from(v) / 255.0));
                    let want = 255.0 * if invert { 1.0 - c } else { c };
                    assert!(
                        rounds_to(f64::from(got[i]), want),
                        "{name} {i}: {} vs {want}",
                        got[i]
                    );
                    if f64::from(got[i]) != (want + 0.5).floor() {
                        flips += 1;
                    }
                    let d = got[i].abs_diff(photoshop[i]);
                    max = max.max(d);
                    sum += u64::from(d);
                }
                let mean = sum as f64 / photoshop.len() as f64;
                if case["fixture"] == "hw2" {
                    let recorded = &judged[index];
                    let slack = u64::from(flips > 0);
                    assert!(
                        u64::from(max) <= recorded["max"].as_u64().unwrap() + slack,
                        "{name}: max {max} against the judged {recorded}"
                    );
                    assert!(
                        mean <= recorded["mean"].as_f64().unwrap()
                            + flips as f64 / photoshop.len() as f64
                            + 1e-9,
                        "{name}: mean {mean} against the judged {recorded} ({flips} ties)"
                    );
                }
                checked += 1;
            }
            "sampled" => {
                let (_, _, photoshop) = plane(case["photoshop"].as_str().unwrap());
                assert!(
                    photoshop.iter().all(|&v| v == photoshop[0]),
                    "{name}: a uniform mask (no cluster position)"
                );
                checked += 1;
            }
            _ => {}
        }
    }
    assert_eq!(checked, 9);
    assert_eq!(manifest["invert"]["verdict"], "PASS");
    let (_, _, reds) = plane("ps_hw1_reds.png");
    let (_, _, inverted) = plane("ps_hw1_reds_invert.png");
    assert!(reds.iter().zip(&inverted).all(|(a, b)| 255 - a == *b));
    assert_eq!(manifest["cluster"]["verdict"], "UNMEASURED");
    assert_eq!(
        manifest["cluster"]["member"],
        json!(["LIN", "max", "euclid"])
    );
}

/// Photoshop's take2 masks on the held-out HW3 chart, at 8 and 16 bits:
/// Doctored equals the shipped `D2M` prediction (this file's oracle; a tie
/// may break either way) and lies within the residual take2 recorded for
/// `D2M` against Photoshop (8-bit scale, 16-bit values ÷ 257), each tie the
/// core broke the other way allowed one native code.
#[test]
fn photoshops_take2_masks_pin_the_shipped_bands_at_8_and_16_bits() {
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(fixture_folder().join("manifest.json")).unwrap())
            .unwrap();
    let take2 = &manifest["take2"];
    assert_eq!(take2["member"], json!(["D2M"]));
    assert_eq!(take2["verdict"], "SHIP");
    let mut checked = 0;
    for case in take2["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let Some(index) = BANDS.iter().position(|b| *b == case["band_name"]) else {
            continue;
        };
        let image = image::open(fixture_folder().join(case["input"].as_str().unwrap())).unwrap();
        let (w, h) = (image.width(), image.height());
        let depth = case["depth"].as_u64().unwrap();
        let photoshop = image::open(fixture_folder().join(case["photoshop"].as_str().unwrap()))
            .unwrap()
            .to_rgba16();
        assert_eq!(photoshop.dimensions(), (w, h), "{name}");
        // Everything at the native scale: `scale` codes a 8-bit code.
        let (rgb, got, ps, max_code, scale): (Vec<[f64; 3]>, Vec<f64>, Vec<f64>, f64, f64) =
            if depth == 8 {
                let rgba = image.to_rgba8().into_raw();
                let got =
                    byte_range(&rgba, w, h, &json!({"mode":"hue","band":BANDS[index]})).unwrap();
                (
                    rgba.chunks_exact(4)
                        .map(|p| [p[0], p[1], p[2]].map(f64::from))
                        .collect(),
                    got.iter().map(|&v| f64::from(v)).collect(),
                    photoshop
                        .pixels()
                        .map(|p| f64::from(p.0[0] / 257))
                        .collect(),
                    255.0,
                    1.0,
                )
            } else {
                let pixels: Vec<[u16; 4]> = image.to_rgba16().pixels().map(|p| p.0).collect();
                let d = native_scalar::doc(w, h, pixels.clone());
                let got = native_range(&d, &json!({"mode":"hue","band":BANDS[index]})).unwrap();
                (
                    pixels
                        .iter()
                        .map(|p| [p[0], p[1], p[2]].map(f64::from))
                        .collect(),
                    got.iter().map(|&v| f64::from(v)).collect(),
                    photoshop.pixels().map(|p| f64::from(p.0[0])).collect(),
                    65535.0,
                    257.0,
                )
            };
        let (mut max, mut sum, mut flips) = (0.0f64, 0.0f64, 0u64);
        for i in 0..rgb.len() {
            let want = max_code * band(index, rgb[i].map(|v| v / max_code));
            if depth == 8 {
                assert!(rounds_to(got[i], want), "{name} {i}: {} vs {want}", got[i]);
            } else {
                assert!(
                    (got[i] - want).abs() <= 1.0,
                    "{name} {i}: {} vs {want}",
                    got[i]
                );
            }
            if got[i] != (want + 0.5).floor() {
                flips += 1;
            }
            let d = (got[i] - ps[i]).abs() / scale;
            max = max.max(d);
            sum += d;
        }
        let mean = sum / rgb.len() as f64;
        let recorded = &case["residual"]["d2m"];
        let slack = if flips > 0 { 1.0 / scale } else { 0.0 };
        assert!(
            max <= recorded["max"].as_f64().unwrap() + slack + 1e-9,
            "{name}: max {max} against the recorded {recorded}"
        );
        assert!(
            mean <= recorded["mean"].as_f64().unwrap()
                + flips as f64 / scale / rgb.len() as f64
                + 1e-9,
            "{name}: mean {mean} against the recorded {recorded} ({flips} ties)"
        );
        checked += 1;
    }
    assert_eq!(checked, 12);
}

// ---- Skin Tones ----

/// The built-in sRGB's D50 Lab of `rgb` (0…1), restated from its published
/// s15Fixed16 columns, its 1024-entry IEC 61966-2-1 curve (linearly
/// interpolated), the D50 white and CIE's exact epsilon and kappa.
fn srgb_lab(rgb: [f64; 3]) -> [f64; 3] {
    const COLUMNS: [[i32; 3]; 3] = [
        [0x6FA0, 0x38F5, 0x0390],
        [0x6297, 0xB787, 0x18D9],
        [0x249F, 0x0F84, 0xB6C3],
    ];
    const WHITE: [f64; 3] = [0.96422, 1.0, 0.82521];
    let table = |i: usize| {
        let x = i as f64 / 1023.0;
        let v = if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        };
        (v * 65535.0).round()
    };
    let linear = |u: f64| {
        let pos = u.clamp(0.0, 1.0) * 1023.0;
        let i = pos.floor() as usize;
        if i >= 1023 {
            return table(1023) / 65535.0;
        }
        let f = pos - i as f64;
        (table(i) * (1.0 - f) + table(i + 1) * f) / 65535.0
    };
    let mut xyz = [0.0; 3];
    for (c, column) in COLUMNS.iter().enumerate() {
        let l = linear(rgb[c]);
        for (slot, raw) in xyz.iter_mut().zip(column) {
            *slot += f64::from(*raw) / 65536.0 * l;
        }
    }
    let (epsilon, kappa) = (216.0 / 24389.0, 24389.0 / 27.0);
    let f = |t: f64| {
        if t > epsilon {
            t.cbrt()
        } else {
            (kappa * t + 16.0) / 116.0
        }
    };
    let [fx, fy, fz] = [0, 1, 2].map(|k| f(xyz[k] / WHITE[k]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// The oracle Skin Tones coverage 0…1 of `rgb` (0…1) at `fuzziness`, and the
/// pixel's distance past the tolerance (module doc).
fn skin(rgb: [f64; 3], fuzziness: f64) -> (f64, f64) {
    let lab = srgb_lab(rgb);
    let d = ((lab[0] - 70.0).powi(2) + (lab[1] - 18.0).powi(2) + (lab[2] - 19.0).powi(2)).sqrt();
    let shoulder = (0.25 * fuzziness).max(0.01);
    if d <= 18.0 {
        (1.0, d - 18.0)
    } else {
        ((1.0 - (d - 18.0) / shoulder).clamp(0.0, 1.0), d - 18.0)
    }
}

/// [`rounds_to`] with a wider tie: the shoulder is as narrow as 0.25·F ΔE,
/// so the core's f32 Lab (about 1e-5 off) moves a code by up to 0.002 there.
fn skin_rounds_to(got: f64, want: f64) -> bool {
    if (want.fract() - 0.5).abs() < 2e-3 {
        got == want.floor() || got == want.ceil()
    } else {
        got == want.round()
    }
}

/// `w × h` straight RGBA8 around a skin colour: each channel within ±48 of
/// (205, 150, 120), every eighth pixel at alpha 128, every thirteenth
/// transparent.
fn skin_chart(w: u32, h: u32, salt: u64) -> Vec<u8> {
    let mut state = 0x5C1 ^ salt;
    let mut out = Vec::new();
    for i in 0..w * h {
        let v = splitmix(&mut state);
        let alpha = if i % 13 == 0 {
            0
        } else if i % 8 == 0 {
            128
        } else {
            255
        };
        let around = |base: u8, bits: u64| (i32::from(base) + (bits % 97) as i32 - 48) as u8;
        out.extend([
            around(205, v),
            around(150, v >> 16),
            around(120, v >> 32),
            alpha,
        ]);
    }
    out
}

#[test]
fn skin_tones_is_the_lab_distance_ramp_at_8_bits() {
    let (w, h) = (41u32, 29u32);
    let pixels = skin_chart(w, h, 1);
    for fuzziness in [0.0, 20.0, 40.0, 120.0, 200.0] {
        let got = byte_range(&pixels, w, h, &json!({"mode":"skin","fuzziness":fuzziness})).unwrap();
        let (mut partial, mut full) = (0, 0);
        for (i, p) in pixels.chunks_exact(4).enumerate() {
            let (c, past) = skin([p[0], p[1], p[2]].map(|v| f64::from(v) / 255.0), fuzziness);
            // At Fuzziness 0 the 0.01 shoulder is a hard edge at 18: a pixel
            // within 1e-3 of it may fall either side under f32.
            if fuzziness == 0.0 && past.abs() < 1e-3 {
                continue;
            }
            let want = c * f64::from(p[3]);
            assert!(
                skin_rounds_to(f64::from(got[i]), want),
                "F {fuzziness} pixel {i} {p:?}: {} vs {want}",
                got[i]
            );
            if p[3] == 255 {
                partial += usize::from(c > 0.0 && c < 1.0);
                full += usize::from(c == 1.0);
            }
        }
        if fuzziness >= 40.0 {
            assert!(
                partial > 20 && full > 20,
                "F {fuzziness}: {partial} / {full}"
            );
        }
    }
    // The default Fuzziness is 40.
    assert_eq!(
        byte_range(&pixels, w, h, &json!({"mode":"skin"})).unwrap(),
        byte_range(&pixels, w, h, &json!({"mode":"skin","fuzziness":40})).unwrap()
    );
}

#[test]
fn skin_tones_at_16_bits_reads_its_own_codes() {
    let (w, h) = (23u32, 17u32);
    let mut state = 0x5C16;
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let v = splitmix(&mut state);
            let around = |base: u16, bits: u64| base - 12000 + (bits % 24000) as u16;
            let a = if i % 9 == 0 { 30000 } else { 65535 };
            [
                around(52000, v),
                around(39000, v >> 16),
                around(31000, v >> 32),
                a,
            ]
        })
        .collect();
    let d = native_scalar::doc(w, h, pixels.clone());
    for (fuzziness, invert) in [(40.0, false), (150.0, true)] {
        let got = native_range(
            &d,
            &json!({"mode":"skin","fuzziness":fuzziness,"invert":invert}),
        )
        .unwrap();
        for (i, p) in pixels.iter().enumerate() {
            let (mut c, _) = skin(
                [p[0], p[1], p[2]].map(|v| f64::from(v) / 65535.0),
                fuzziness,
            );
            if invert {
                c = 1.0 - c;
            }
            let want = c * f64::from(p[3]);
            assert!(
                (f64::from(got[i]) - want).abs() <= 2.0,
                "F {fuzziness} {i}: {} vs {want}",
                got[i]
            );
        }
    }
}

#[test]
fn skin_tones_on_a_float_document_reads_its_display_codes_with_white_held() {
    let samples: Vec<[f32; 4]> = vec![
        [0.70, 0.40, 0.25, 1.0],
        [0.62, 0.36, 0.22, 1.0],
        [0.80, 0.50, 0.35, 1.0],
        [2.5, 2.5, 2.5, 1.0],
        [0.0, 0.0, 1.0, 1.0],
        [0.55, 0.30, 0.20, 0.5],
        [0.70, 0.40, 0.25, 0.0],
    ];
    let w = samples.len() as u32;
    let d = float_doc(w, 1, &IccProfile::srgb(), std::slice::from_ref(&samples));
    for fuzziness in [40.0, 160.0] {
        let got = native_range(&d, &json!({"mode":"skin","fuzziness":fuzziness})).unwrap();
        for (i, p) in samples.iter().enumerate() {
            let rgb = [p[0], p[1], p[2]].map(|v| srgb_encode(f64::from(v)));
            let (c, _) = skin(rgb, fuzziness);
            let want = c * f64::from(p[3].clamp(0.0, 1.0)) * 65535.0;
            // Half a 16-bit display code moves Lab by about 0.002, so the
            // 0.25·F shoulder moves by at most a few codes.
            assert!(
                (f64::from(got[i]) - want).abs() <= 24.0,
                "F {fuzziness} {i} {p:?}: {} vs {want}",
                got[i]
            );
        }
    }
}

#[test]
fn malformed_skin_tones_refuse_by_sentence() {
    let pixels = chart(4, 3, 4);
    let refuse = |params: Value| byte_range(&pixels, 4, 3, &params).unwrap_err();
    for params in [
        json!({"mode":"skin","band":"reds"}),
        json!({"mode":"skin","tolerance":20}),
        json!({"mode":"skin","softness":5}),
        json!({"mode":"skin","samples":[[1,2,3]]}),
        json!({"mode":"skin","lower":5}),
        json!({"mode":"skin","points":[[0,0]]}),
        json!({"mode":"skin","cluster_range":5}),
    ] {
        assert_eq!(
            refuse(params),
            "skin Color Range accepts only fuzziness and invert"
        );
    }
    for value in [json!(-1), json!(200.5), json!(1e9)] {
        assert_eq!(
            refuse(json!({"mode":"skin","fuzziness":value})),
            "fuzziness must be between 0 and 200"
        );
    }
    assert_eq!(
        refuse(json!({"mode":"skin","fuzziness":"40"})),
        "fuzziness must be a number"
    );
    for params in [
        json!({"mode":"sampled","samples":[[1,2,3]],"fuzziness":40}),
        json!({"mode":"tonal","fuzziness":40}),
        json!({"mode":"hue","band":"reds","fuzziness":40}),
    ] {
        assert_eq!(refuse(params), "Color Range fuzziness needs mode skin");
    }
    assert_eq!(
        refuse(json!({"mode":"skin","invert":"yes"})),
        "invert must be a boolean"
    );
}

/// Photoshop's take3 masks on SK1, SK2 and the held-out SK3 (8 and 16 bits):
/// Doctored equals the shipped member's prediction (this file's oracle; a
/// tie may break either way, ± 1 native code at 16 bits) and lies within the
/// MISS residual take3 recorded for it against Photoshop (8-bit scale,
/// 16-bit values ÷ 257), each pixel Doctored rounds the other way allowed
/// one native code. The portrait's mask is not in the fixture.
#[test]
fn photoshops_take3_masks_pin_skin_tones() {
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(fixture_folder().join("manifest.json")).unwrap())
            .unwrap();
    let take3 = &manifest["take3"];
    assert_eq!(take3["verdict"], "MISS");
    assert_eq!(take3["member"]["family"], "LAB");
    assert_eq!(
        take3["member"]["numbers"],
        json!({"L0":70.0,"a0":18.0,"b0":19.0,"t1":0,"t0":18.0,"s1":0.25,"j0":0.0})
    );
    let mut checked = 0;
    for case in take3["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let fuzziness = case["fuzziness"].as_f64().unwrap();
        let invert = case["invert"].as_bool().unwrap();
        let params = json!({"mode":"skin","fuzziness":fuzziness,"invert":invert});
        let image = image::open(fixture_folder().join(case["input"].as_str().unwrap())).unwrap();
        let (w, h) = (image.width(), image.height());
        let depth = case["depth"].as_u64().unwrap();
        let photoshop = image::open(fixture_folder().join(case["photoshop"].as_str().unwrap()))
            .unwrap()
            .to_rgba16();
        assert_eq!(photoshop.dimensions(), (w, h), "{name}");
        let (rgb, got, ps, max_code, scale): (Vec<[f64; 3]>, Vec<f64>, Vec<f64>, f64, f64) =
            if depth == 8 {
                let rgba = image.to_rgba8().into_raw();
                let got = byte_range(&rgba, w, h, &params).unwrap();
                (
                    rgba.chunks_exact(4)
                        .map(|p| [p[0], p[1], p[2]].map(f64::from))
                        .collect(),
                    got.iter().map(|&v| f64::from(v)).collect(),
                    photoshop
                        .pixels()
                        .map(|p| f64::from(p.0[0] / 257))
                        .collect(),
                    255.0,
                    1.0,
                )
            } else {
                let pixels: Vec<[u16; 4]> = image.to_rgba16().pixels().map(|p| p.0).collect();
                let d = native_scalar::doc(w, h, pixels.clone());
                let got = native_range(&d, &params).unwrap();
                (
                    pixels
                        .iter()
                        .map(|p| [p[0], p[1], p[2]].map(f64::from))
                        .collect(),
                    got.iter().map(|&v| f64::from(v)).collect(),
                    photoshop.pixels().map(|p| f64::from(p.0[0])).collect(),
                    65535.0,
                    257.0,
                )
            };
        let (mut max, mut sum, mut flips) = (0.0f64, 0.0f64, 0u64);
        for i in 0..rgb.len() {
            let (c, past) = skin(rgb[i].map(|v| v / max_code), fuzziness);
            let want = max_code * if invert { 1.0 - c } else { c };
            let edge = fuzziness == 0.0 && past.abs() < 1e-3;
            if depth == 8 && !edge {
                assert!(
                    skin_rounds_to(got[i], want),
                    "{name} {i}: {} vs {want}",
                    got[i]
                );
            } else if !edge {
                assert!(
                    (got[i] - want).abs() <= 1.0,
                    "{name} {i}: {} vs {want}",
                    got[i]
                );
            }
            if got[i] != (want + 0.5).floor() {
                flips += 1;
            }
            let d = (got[i] - ps[i]).abs() / scale;
            max = max.max(d);
            sum += d;
        }
        let mean = sum / rgb.len() as f64;
        let recorded = &case["residual"];
        let slack = if flips > 0 { 1.0 / scale } else { 0.0 };
        assert!(
            max <= recorded["max"].as_f64().unwrap() + slack + 1e-9,
            "{name}: max {max} against the recorded {recorded}"
        );
        assert!(
            mean <= recorded["mean"].as_f64().unwrap()
                + flips as f64 / scale / rgb.len() as f64
                + 1e-9,
            "{name}: mean {mean} against the recorded {recorded} ({flips} flips)"
        );
        checked += 1;
    }
    assert_eq!(checked, 11);
    // The judged residuals the MISS pinned, on the held-out cases.
    for (name, max) in [
        ("sk3_8_f40", 254.0),
        ("sk3_8_f120", 221.0),
        ("sk3_16_f40", 255.0),
    ] {
        assert_eq!(take3["judged"][name]["max"].as_f64(), Some(max), "{name}");
    }
}
