//! Independent native retouch references, through the public edit ABI.
//! Bilinear taps and source-over are derived in f64 without product helpers.
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn image(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}
fn document(w: u32, h: u32, layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}
fn raster(w: u32, h: u32, pixels: Vec<[u16; 4]>, offset: (i32, i32)) -> Layer {
    Layer::raster("Pixels", image(w, h, pixels), offset, 1.0, true).unwrap()
}
fn pixels(d: &PrecisionDocument, layer: usize) -> Vec<[u16; 4]> {
    d.layer_source(layer).unwrap().to_rgba16().unwrap()
}
fn execute(
    d: &PrecisionDocument,
    request: Value,
    coverage: &[u16],
) -> Result<Option<Box<PrecisionDocument>>, String> {
    execute_control(d, request, coverage, LIMIT, None)
}
fn execute_control(
    d: &PrecisionDocument,
    request: Value,
    coverage: &[u16],
    limit: u64,
    control: Option<&DrJobControl>,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let request = CString::new(request.to_string()).unwrap();
    unsafe {
        let mut err = ptr::null_mut();
        let out = dr_precision_doc_edit(
            d,
            request.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            limit,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut err,
        );
        if !err.is_null() {
            let message = CStr::from_ptr(err).to_string_lossy().into_owned();
            dr_string_free(err);
            assert!(out.is_null(), "failure must not publish a document");
            return Err(message);
        }
        Ok((!out.is_null()).then(|| Box::from_raw(out)))
    }
}
fn quantize(value: f64) -> u16 {
    (value * 65535.0).round().clamp(0.0, 65535.0) as u16
}
fn over(dst: [u16; 4], src: [u16; 4], coverage: u16, opacity: f64) -> [u16; 4] {
    if coverage == 0 || src[3] == 0 {
        return dst;
    }
    let d = dst.map(|v| f64::from(v) / 65535.0);
    let s = src.map(|v| f64::from(v) / 65535.0);
    let sa = s[3] * f64::from(coverage) / 65535.0 * opacity;
    let a = sa + d[3] * (1.0 - sa);
    [
        quantize((s[0] * sa + d[0] * d[3] * (1.0 - sa)) / a),
        quantize((s[1] * sa + d[1] * d[3] * (1.0 - sa)) / a),
        quantize((s[2] * sa + d[2] * d[3] * (1.0 - sa)) / a),
        quantize(a),
    ]
}
fn bilinear(source: &[[u16; 4]], w: u32, h: u32, x: f64, y: f64) -> [u16; 4] {
    let u = x - 0.5;
    let v = y - 0.5;
    let left = u.floor() as i64;
    let top = v.floor() as i64;
    let mut sums = [0.0; 4];
    for (dy, wy) in [(0, 1.0 - (v - v.floor())), (1, v - v.floor())] {
        for (dx, wx) in [(0, 1.0 - (u - u.floor())), (1, u - u.floor())] {
            let (px, py) = (left + dx, top + dy);
            if px < 0 || py < 0 || px >= i64::from(w) || py >= i64::from(h) {
                continue;
            }
            let p = source[py as usize * w as usize + px as usize];
            let weighted_alpha = f64::from(p[3]) * wx * wy;
            sums[3] += weighted_alpha;
            for c in 0..3 {
                sums[c] += f64::from(p[c]) * weighted_alpha;
            }
        }
    }
    if sums[3] == 0.0 {
        return [0; 4];
    }
    [
        (sums[0] / sums[3]).round() as u16,
        (sums[1] / sums[3]).round() as u16,
        (sums[2] / sums[3]).round() as u16,
        sums[3].round() as u16,
    ]
}

#[test]
fn fractional_rotated_scaled_sources_match_independent_premultiplied_taps() {
    let (w, h) = (17, 13);
    let source: Vec<_> = (0..w * h)
        .map(|i| {
            [
                10001 + (i * 37) as u16,
                21002 + (i * 19) as u16,
                35003 + (i * 11) as u16,
                [0, 1, 2, 129, 32769, 65535][i as usize % 6],
            ]
        })
        .collect();
    let before: Vec<_> = (0..w * h)
        .map(|i| {
            [
                30101 + i as u16,
                19002,
                7013,
                [1, 32769, 65535][i as usize % 3],
            ]
        })
        .collect();
    let coverage: Vec<_> = (0..w * h)
        .map(|i| [0, 1, 257, 32768, 65535][i as usize % 5])
        .collect();
    let d = document(
        w,
        h,
        vec![
            raster(w, h, before.clone(), (0, 0)),
            raster(w, h, source.clone(), (0, 0)),
        ],
    );
    for matrix in [
        [1.0, 0.0, 0.0, 1.0, 0.25, -0.375],
        [0.0, 1.0, -1.0, 0.0, 14.25, -1.125],
        [0.75, 0.125, -0.25, 1.125, 1.25, -0.125],
        [-1.0, 0.0, 0.0, 1.0, 16.75, 0.25],
    ] {
        let out = execute(
            &d,
            json!({"op":"retouch_clone","layer":0,"source_layer":1,
            "sample":"current","matrix":matrix,"opacity":0.625}),
            &coverage,
        )
        .unwrap()
        .unwrap();
        for (i, actual) in pixels(&out, 0).iter().enumerate() {
            let x = (i % w as usize) as f64 + 0.5;
            let y = (i / w as usize) as f64 + 0.5;
            let sampled = bilinear(
                &source,
                w,
                h,
                matrix[0] * x + matrix[2] * y + matrix[4],
                matrix[1] * x + matrix[3] * y + matrix[5],
            );
            let expected = over(before[i], sampled, coverage[i], 0.625);
            if coverage[i] == 0 {
                assert_eq!(*actual, before[i]);
            }
            for c in 0..4 {
                assert!(
                    actual[c].abs_diff(expected[c]) <= 2,
                    "matrix {matrix:?}, pixel {i}, component {c}: {actual:?} vs {expected:?}"
                );
            }
        }
        assert_eq!(pixels(&out, 1), source);
    }
    assert_eq!(pixels(&d, 0), before);
}

#[test]
fn sample_below_live_adjustment_prevents_double_application_inside_group() {
    let original = vec![[10001, 20002, 30003, 65535], [30001, 40002, 50003, 65535]];
    let invert =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    let group = Layer::pass_through(
        "Group",
        vec![
            raster(2, 1, original.clone(), (0, 0)),
            Layer::adjustment("Grade", &invert, 1.0, true, &IccProfile::srgb()).unwrap(),
        ],
        1.0,
        true,
    )
    .unwrap();
    let d = document(2, 1, vec![group]);
    for (sample, ignore, expected) in [
        ("current", false, original[0]),
        ("current_and_below", false, original[0]),
        ("all", true, original[0]),
        ("all", false, [55534, 45533, 35532, 65535]),
    ] {
        let out = execute(
            &d,
            json!({"op":"retouch_clone","layer":0,"sample":sample,
            "ignore_adjustments":ignore,"matrix":[1,0,0,1,-1,0]}),
            &[0, 65535],
        )
        .unwrap()
        .unwrap();
        assert_eq!(pixels(&out, 0)[0], original[0]);
        for (a, e) in pixels(&out, 0)[1].iter().zip(expected) {
            assert!(a.abs_diff(e) <= 1);
        }
        let rendered = out.render(LIMIT, 0, None).unwrap();
        for (c, expected) in expected.iter().enumerate().take(3) {
            assert!(rendered.rgba16_at(1, 0).unwrap()[c].abs_diff(65535 - expected) <= 1);
        }
    }
    assert_eq!(pixels(&d, 0), original);
}

#[test]
fn three_native_sampling_modes_have_independent_source_over_values() {
    let bottom = Layer::raster(
        "Bottom",
        image(2, 1, vec![[10001, 20001, 30001, 65535]; 2]),
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let target = Layer::raster(
        "Target",
        image(2, 1, vec![[30003, 40003, 50003, 65535]; 2]),
        (0, 0),
        0.5,
        true,
    )
    .unwrap();
    let top = Layer::raster(
        "Top",
        image(2, 1, vec![[50005, 20005, 10005, 65535]; 2]),
        (0, 0),
        0.5,
        true,
    )
    .unwrap();
    let d = document(2, 1, vec![bottom, target, top]);
    // Each 50 % at its byte, 128 / 255 (#578).
    let q = crate::common::byte_unit(0.5);
    let mix = |b: [f64; 3], t: [f64; 3]| [0, 1, 2].map(|c| b[c] + (t[c] - b[c]) * q);
    let below = mix([10001., 20001., 30001.], [30003., 40003., 50003.]);
    let all = mix(below, [50005., 20005., 10005.]);
    let rgba = |v: [f64; 3]| {
        [
            v[0].round() as u16,
            v[1].round() as u16,
            v[2].round() as u16,
            65535,
        ]
    };
    for (mode, expected) in [
        ("current", [30003, 40003, 50003, 65535]),
        ("current_and_below", rgba(below)),
        ("all", rgba(all)),
    ] {
        let out = execute(
            &d,
            json!({"op":"retouch_clone","layer":1,"sample":mode,
            "output":"new_layer"}),
            &[65535, 0],
        )
        .unwrap()
        .unwrap();
        assert_eq!(out.layer_count(), 4);
        for (a, e) in pixels(&out, 2)[0].iter().zip(expected) {
            assert!(a.abs_diff(e) <= 1, "{mode}: {a} vs {e}");
        }
        assert_eq!(pixels(&out, 2)[1], [0; 4]);
        assert_eq!(pixels(&out, 1), pixels(&d, 1));
    }
}

#[test]
fn explicit_expansion_preserves_storage_mask_and_native_reopen() {
    let original = vec![[101, 202, 303, 65535]; 4];
    let mut properties = MaskProperties::at((-1, 2));
    properties.linked = false;
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(3, 1, vec![1, 32769, 65535], None).unwrap(),
        properties,
        false,
    )
    .unwrap();
    let target = raster(2, 2, original.clone(), (2, 1)).with_mask(Some(mask));
    let source = vec![[10001, 20002, 30003, 65535]; 24];
    let d = document(6, 4, vec![target, raster(6, 4, source, (0, 0))]);
    let mut coverage = vec![0; 24];
    coverage[0] = 65535;
    let base = json!({"op":"retouch_clone","layer":0,"source_layer":1,"sample":"current"});
    assert!(execute(&d, base.clone(), &coverage).unwrap().is_none());
    let mut request = base;
    request["expand_destination"] = json!(true);
    let out = execute(&d, request, &coverage).unwrap().unwrap();
    assert_eq!(out.layer_source(0).unwrap().dimensions(), (6, 4));
    assert_eq!(out.layer(0).unwrap().offset(), (0, 0));
    assert_eq!(pixels(&out, 0)[0], [10001, 20002, 30003, 65535]);
    for y in 0..4 {
        for x in 0..6 {
            let i = y * 6 + x;
            if i == 0 {
                continue;
            }
            assert_eq!(
                pixels(&out, 0)[i],
                if (2..4).contains(&x) && (1..3).contains(&y) {
                    original[0]
                } else {
                    [0; 4]
                }
            );
        }
    }
    let saved = out.encode_native(LIMIT, 0, None).unwrap();
    let reopened = PrecisionDocument::parse_native(&saved, LIMIT, 0, None).unwrap();
    assert_eq!(pixels(&reopened, 0), pixels(&out, 0));
    let restored_mask = reopened.layer(0).unwrap().mask().unwrap();
    assert_eq!(restored_mask.plane().samples(), &[1, 32769, 65535]);
    assert_eq!(restored_mask.properties().offset, (-1, 2));
    assert!(!restored_mask.properties().linked);
    assert!(!restored_mask.enabled());
    assert_eq!(pixels(&d, 0), original);
}

#[test]
fn native_heal_matches_affine_background_and_preserves_every_alpha() {
    let (w, h) = (40, 24);
    let mut original: Vec<_> = (0..w * h)
        .map(|i| {
            let x = i % w;
            let y = i / w;
            [
                12001 + (17 * x + 23 * y) as u16,
                22002 + (11 * x + 13 * y) as u16,
                32003 + (7 * x + 19 * y) as u16,
                [1, 129, 32769, 65535][i as usize % 4],
            ]
        })
        .collect();
    let clean = original.clone();
    let mut coverage = vec![0; original.len()];
    for y in 7..15 {
        for x in 8..16 {
            let i = (y * w + x) as usize;
            coverage[i] = 65535;
            if (9..15).contains(&x) && (8..14).contains(&y) {
                for value in &mut original[i][..3] {
                    *value += 4000;
                }
            }
        }
    }
    let d = document(w, h, vec![raster(w, h, original.clone(), (0, 0))]);
    let out = execute(
        &d,
        json!({"op":"retouch_heal","layer":0,"sample":"current",
        "matrix":[1,0,0,1,18,0],"opacity":0.5}),
        &coverage,
    )
    .unwrap()
    .unwrap();
    for (i, p) in pixels(&out, 0).iter().enumerate() {
        assert_eq!(p[3], original[i][3]);
        if coverage[i] == 0 {
            assert_eq!(*p, original[i]);
        }
        // Interior affine data has zero Laplacian; the one-pixel contour
        // remains prescribed destination data and is deliberately excluded.
        let (x, y) = ((i as u32) % w, (i as u32) / w);
        if (9..15).contains(&x) && (8..14).contains(&y) {
            for c in 0..3 {
                let expected = (u32::from(original[i][c]) + u32::from(clean[i][c])) as f64 / 2.0;
                assert!(
                    (f64::from(p[c]) - expected).abs() <= 3.0,
                    "pixel {i}: {p:?}, clean {:?}",
                    clean[i]
                );
            }
        }
    }
}

#[test]
fn malformed_budget_cancel_and_noop_requests_never_publish() {
    let d = document(
        4,
        2,
        vec![raster(4, 2, vec![[10001, 20002, 30003, 65535]; 8], (0, 0))],
    );
    let request = json!({"op":"retouch_clone","layer":0});
    assert!(execute(&d, request.clone(), &[65535; 8]).unwrap().is_none());
    assert!(execute(&d, request.clone(), &[0; 8]).unwrap().is_none());
    assert!(execute(&d, request.clone(), &[65535; 7]).is_err());
    for bad in [
        json!({"op":"retouch_clone","layer":99}),
        json!({"op":"retouch_clone","layer":0,"matrix":[0,0,0,0,0,0]}),
        json!({"op":"retouch_clone","layer":0,"source_layer":0,"sample":"all"}),
        json!({"op":"retouch_clone","layer":0,"unexpected":true}),
    ] {
        assert!(execute(&d, bad, &[65535; 8]).is_err());
    }
    assert!(execute_control(&d, request.clone(), &[65535; 8], 1, None).is_err());
    let control = DrJobControl::default();
    control.cancel();
    assert!(
        execute_control(&d, request, &[65535; 8], LIMIT, Some(&control))
            .err()
            .unwrap()
            .contains("cancel")
    );
    assert_eq!(control.scratch_bytes(), 0);
    assert_eq!(pixels(&d, 0), vec![[10001, 20002, 30003, 65535]; 8]);
}

#[test]
fn exact_native_clone_commutes_with_working_profile_conversion() {
    let original = vec![
        [10001, 42002, 27003, 65535],
        [31004, 12005, 53006, 65535],
        [17007, 46008, 25009, 65535],
        [32010, 29011, 11012, 65535],
    ];
    let d = document(4, 1, vec![raster(4, 1, original, (0, 0))]);
    let p3 = IccProfile::display_p3();
    let converted = d.convert_to_profile(&p3, LIMIT, 0, None).unwrap().unwrap();
    for sample in ["current", "current_and_below", "all"] {
        let request = json!({"op":"retouch_clone", "layer":0, "sample":sample,
            "matrix":[1,0,0,1,-2,0]});
        let cloned = execute(&d, request.clone(), &[0, 0, 65535, 0])
            .unwrap()
            .unwrap();
        let clone_then_convert = cloned
            .convert_to_profile(&p3, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let convert_then_clone = execute(&converted, request, &[0, 0, 65535, 0])
            .unwrap()
            .unwrap();
        assert_eq!(
            pixels(&clone_then_convert, 0),
            pixels(&convert_then_clone, 0)
        );
        assert_eq!(convert_then_clone.profile().bytes(), p3.bytes());
        for i in [0, 1, 3] {
            assert_eq!(pixels(&convert_then_clone, 0)[i], pixels(&converted, 0)[i]);
        }
    }
}
