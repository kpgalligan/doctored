//! Four nonseparable native modes (Hue, Saturation, Color, Luminosity) through
//! the unchanged f32 blend.rs NonSeparable kernel, persisted by conditional
//! native v33 / LBLD schema5 and carried by 16-bit PSD. The oracle is the
//! independent f64 W3C equation set in common/native_blend_oracle.rs.
use doctored_core::doc::BlendMode;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;
use tempfile::TempDir;

use crate::common;
use crate::native_blend_oracle;
use crate::psd16_fixture as fixture;
use common::{cpath, take_err_string};
use fixture::Record;

use crate::identity;

/// Every native encode below goes through here. Phase 12E-1 made the
/// always-written identity wrapper the OUTERMOST `.rz` word and its `LIDS`
/// table the file's last block; this file's subject is an EARLIER wrapper,
/// so the outer one is asserted and then peeled, leaving byte-for-byte what
/// the writer these tests were written against produced.
fn encoded(doc: &PrecisionDocument) -> Vec<u8> {
    identity::typed_prior_body(&doc.encode_native(LIMIT, 0, None).unwrap())
}

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const MODES: [i32; 4] = [23, 24, 25, 26];
const LAYERS: i32 = 0;
/// The ordinary-bound eligibility ratio: the SetSat amplification term
/// `1024 * s / delta` fits inside the 0.15-code slack above the 0.5
/// quantization half-step once `delta >= 6830 * s`.
const ELIGIBLE_RATIO: f64 = 6830.;

fn edit(
    doc: &PrecisionDocument,
    request: Value,
    limit: u64,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            limit,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut err,
        )
    };
    if !err.is_null() {
        let message = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        assert!(result.is_null());
        return Err(message);
    }
    Ok((!result.is_null()).then(|| *unsafe { Box::from_raw(result) }))
}
fn request(index: usize, mode: i32) -> Value {
    json!({"op":"layer_blend","version":1,"layer":index,"blend_mode":mode})
}
fn changed(doc: &PrecisionDocument, index: usize, mode: i32) -> PrecisionDocument {
    edit(doc, request(index, mode), LIMIT, None)
        .unwrap()
        .unwrap()
}
fn layer(name: &str, pixels: Vec<[u16; 4]>, offset: i32, opacity: f32) -> Layer {
    Layer::raster(
        name,
        PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None)
            .unwrap(),
        (offset, 0),
        opacity,
        true,
    )
    .unwrap()
}
fn document(width: u32, layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        width,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}
fn mask(samples: Vec<u16>, density: f32) -> PrecisionMask {
    let mut properties = MaskProperties::at((0, 0));
    properties.density = density;
    PrecisionMask::new(
        PrecisionPlane::from_u16(samples.len() as u32, 1, samples, None).unwrap(),
        properties,
        true,
    )
    .unwrap()
}
fn render(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}
fn n(p: [u16; 4]) -> [f64; 4] {
    p.map(|v| f64::from(v) / 65535.)
}
fn rgb(p: [f64; 4]) -> [f64; 3] {
    [p[0], p[1], p[2]]
}
/// W3C source-over with the nonseparable B(Cb, Cs) evaluated on the layer
/// colors; mode 0 is plain Normal for group-internal composition.
fn over(b: [f64; 4], s: [f64; 4], weight: f64, mode: i32) -> [f64; 4] {
    let sa = s[3] * weight;
    let alpha = sa + b[3] * (1. - sa);
    if alpha == 0. {
        return [0.; 4];
    }
    let blended = if mode == 0 {
        rgb(s)
    } else {
        native_blend_oracle::hsl(mode, rgb(b), rgb(s))
    };
    let mut out = [0.; 4];
    out[3] = alpha;
    for c in 0..3 {
        out[c] = (sa * ((1. - b[3]) * s[c] + b[3] * blended[c]) + (1. - sa) * b[3] * b[c]) / alpha;
    }
    out
}
fn close_components(actual: [u16; 4], expected: [f64; 4], tolerance: f64) -> f64 {
    let mut maximum: f64 = 0.;
    for c in 0..4 {
        let error = (f64::from(actual[c]) - expected[c] * 65535.).abs();
        assert!(
            error <= if c == 3 { 0.65 } else { tolerance },
            "channel {c}: {actual:?}, {expected:?}, error {error}"
        );
        maximum = maximum.max(error);
    }
    maximum
}
fn spread(p: [f64; 3]) -> f64 {
    p.into_iter().fold(f64::NEG_INFINITY, f64::max) - p.into_iter().fold(f64::INFINITY, f64::min)
}
/// `(delta, s)` from the authored colors, in codes and normalized saturation:
/// the rescaled triple is the source for Hue and the backdrop for Saturation;
/// `s` is the other pixel's saturation. Color/Luminosity have no SetSat term.
fn conditioning(mode: i32, base: [f64; 3], source: [f64; 3]) -> Option<(f64, f64)> {
    match mode {
        23 => Some((spread(source) * 65535., spread(base))),
        24 => Some((spread(base) * 65535., spread(source))),
        _ => None,
    }
}
fn eligible(mode: i32, base: [f64; 3], source: [f64; 3]) -> bool {
    conditioning(mode, base, source)
        .is_none_or(|(delta, s)| delta == 0. || s == 0. || delta >= ELIGIBLE_RATIO * s)
}
/// The minimum `delta / s` over Hue/Saturation pixels, the qualified region.
fn ratio(mode: i32, base: [f64; 3], source: [f64; 3]) -> Option<f64> {
    conditioning(mode, base, source)
        .filter(|&(delta, s)| delta > 0. && s > 0.)
        .map(|(delta, s)| delta / s)
}

/// Opaque endpoint triples, gray triples and asymmetric triples whose channels
/// are pairwise at least 6830 codes apart.
fn eligible_colors() -> Vec<[u16; 4]> {
    let mut colors = Vec::new();
    for bits in 0..8u16 {
        colors.push([
            65535 * (bits & 1),
            65535 * ((bits >> 1) & 1),
            65535 * ((bits >> 2) & 1),
            65535,
        ]);
    }
    for gray in [1u16, 17, 32767, 32768, 65534] {
        colors.push([gray, gray, gray, 65535]);
    }
    for [r, g, b] in [
        [12345u16, 34567, 56789],
        [65535, 40000, 0],
        [7000, 50000, 20000],
        [60000, 30000, 1],
        [1, 20000, 65535],
        [45000, 10000, 30000],
        [65535, 58000, 51000],
        [200, 7100, 14100],
        [32767, 25000, 40000],
        [50000, 21000, 65000],
    ] {
        for (x, y) in [(r, g), (g, b), (r, b)] {
            assert!(x.abs_diff(y) >= 6830);
        }
        colors.push([r, g, b, 65535]);
    }
    colors
}

#[test]
fn hsl_modes_endpoints_grays_and_asymmetric_colors_meet_the_ordinary_bound() {
    let colors = eligible_colors();
    let mut base = Vec::new();
    let mut source = Vec::new();
    for &b in &colors {
        for &s in &colors {
            base.push(b);
            source.push(s);
        }
    }
    for opacity in [1f32, 0.625] {
        let doc = document(
            base.len() as u32,
            vec![
                layer("Backdrop", base.clone(), 0, 1.),
                layer("Source", source.clone(), 0, opacity),
            ],
        );
        for mode in MODES {
            let mut maximum: f64 = 0.;
            let mut minimum_ratio = f64::INFINITY;
            for (i, p) in render(&changed(&doc, 1, mode)).into_iter().enumerate() {
                let (b, s) = (n(base[i]), n(source[i]));
                assert!(
                    eligible(mode, rgb(b), rgb(s)),
                    "mode {mode} pixel {i} is outside the ordinary-bound region"
                );
                if let Some(r) = ratio(mode, rgb(b), rgb(s)) {
                    minimum_ratio = minimum_ratio.min(r);
                }
                maximum = maximum.max(close_components(
                    p,
                    over(b, s, crate::common::byte_unit(f64::from(opacity)), mode),
                    0.65,
                ));
            }
            eprintln!(
                "grid mode {mode} opacity {opacity}: maximum unrounded-f64 code error {maximum}, minimum delta/s {minimum_ratio}"
            );
        }
    }
}

#[test]
fn hsl_modes_every_native_mask_code_partial_alpha_density_and_opacity() {
    let colors = eligible_colors();
    let base: Vec<_> = (0..=u16::MAX)
        .map(|i| {
            let mut p = colors[i as usize % colors.len()];
            p[3] = [0, 1, 32769, 65535][i as usize % 4];
            p
        })
        .collect();
    let source: Vec<_> = (0..=u16::MAX)
        .map(|i| {
            let mut p = colors[(i as usize * 7 + 3) % colors.len()];
            p[3] = [65535, 32768, 1, 0, 45001][i as usize % 5];
            p
        })
        .collect();
    let doc = document(
        65536,
        vec![
            layer("Base", base.clone(), 0, 1.),
            layer("Ink", source.clone(), 0, 0.625)
                .with_mask(Some(mask((0..=u16::MAX).collect(), 0.75))),
        ],
    );
    for mode in MODES {
        let next = changed(&doc, 1, mode);
        assert!(PrecisionImage::ptr_eq(
            doc.layer_source(1).unwrap(),
            next.layer_source(1).unwrap()
        ));
        let mut maximum: f64 = 0.;
        let mut minimum_ratio = f64::INFINITY;
        for (i, p) in render(&next).into_iter().enumerate() {
            let (b, s) = (n(base[i]), n(source[i]));
            assert!(eligible(mode, rgb(b), rgb(s)));
            if let Some(r) = ratio(mode, rgb(b), rgb(s)) {
                minimum_ratio = minimum_ratio.min(r);
            }
            maximum = maximum.max(close_components(
                p,
                over(
                    b,
                    s,
                    crate::common::byte_unit(0.625) * (0.25 + 0.75 * i as f64 / 65535.),
                    mode,
                ),
                0.65,
            ));
        }
        eprintln!(
            "mask mode {mode}: maximum unrounded-f64 code error {maximum}, minimum delta/s {minimum_ratio}"
        );
        assert_eq!(
            next.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
            source
        );
        assert_eq!(doc.layer(1).unwrap().blend_mode(), BlendMode::Normal);
    }
}

#[test]
fn hsl_modes_meaningful_alpha_translated_nested_groups_and_pass_through() {
    let b = [12345, 34567, 56789, 32123];
    let a = [51234, 22223, 34567, 43210];
    let z = [22444, 44555, 11666, 54321];
    for mode in MODES {
        let inner = Layer::isolated_normal(
            "Inner",
            vec![
                layer("A", vec![a; 3], -1, 0.75),
                layer("Z", vec![z; 2], 1, 0.5),
            ],
            0.625,
            true,
        )
        .unwrap()
        .with_blend_mode(BlendMode::from_c(mode).unwrap())
        .unwrap()
        .with_mask(Some(mask(vec![1, 32769, 65535], 1.)));
        let outer = Layer::pass_through("Outer", vec![inner], 0.875, true).unwrap();
        let doc = document(3, vec![layer("B", vec![b; 3], 0, 1.), outer]);
        let result = render(&doc);
        let mut maximum: f64 = 0.;
        let mut minimum_ratio = f64::INFINITY;
        for x in 0..3 {
            let first = if x < 2 {
                over([0.; 4], n(a), crate::common::byte_unit(0.75), 0)
            } else {
                [0.; 4]
            };
            let group = if x >= 1 {
                over(first, n(z), crate::common::byte_unit(0.5), 0)
            } else {
                first
            };
            assert!(eligible(mode, rgb(n(b)), rgb(group)));
            if let Some(r) = ratio(mode, rgb(n(b)), rgb(group)) {
                minimum_ratio = minimum_ratio.min(r);
            }
            let after = over(
                n(b),
                group,
                crate::common::byte_unit(0.625) * [1., 32769., 65535.][x] / 65535.,
                mode,
            );
            let alpha = n(b)[3] * (1. - crate::common::byte_unit(0.875))
                + after[3] * crate::common::byte_unit(0.875);
            let mut expected = [0.; 4];
            expected[3] = alpha;
            for c in 0..3 {
                expected[c] = (n(b)[c] * n(b)[3] * (1. - crate::common::byte_unit(0.875))
                    + after[c] * after[3] * crate::common::byte_unit(0.875))
                    / alpha;
            }
            maximum = maximum.max(close_components(result[x], expected, 0.65));
        }
        eprintln!(
            "group mode {mode}: maximum unrounded-f64 code error {maximum}, minimum delta/s {minimum_ratio}"
        );
        let passing = changed(&doc, 3, 27);
        assert_ne!(render(&passing), result);
        assert_eq!(render(&changed(&passing, 3, mode)), result);
        let renamed = edit(
            &doc,
            json!({"op":"rename_layer","layer":1,"name":"Edited child"}),
            LIMIT,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(renamed.layer(3).unwrap().blend_mode().to_c(), mode);
        let doubled = renamed.duplicate_layer(2).unwrap().unwrap();
        assert_eq!(doubled.layer(4).unwrap().blend_mode().to_c(), mode);
    }
}

#[test]
fn hsl_mode_v33_downgrades_exactly_and_old_schemas_refuse_new_meaning() {
    let base = document(
        1,
        vec![
            layer("A", vec![[30001, 30002, 30003, 0]], -1, 0.5),
            layer("B", vec![[12345, 54321, 23456, 1]], 0, 1.),
        ],
    );
    let normal = encoded(&base);
    let basic = changed(&base, 0, 1);
    let v26 = encoded(&basic);
    let separable = changed(&basic, 1, 3);
    let v27 = encoded(&separable);
    let four = changed(&basic, 1, 10);
    let v30 = encoded(&four);
    let five = changed(&basic, 1, 13);
    let v31 = encoded(&five);
    assert_eq!(&v31[4..8], &31u32.to_le_bytes());
    for mode in MODES {
        let new = changed(&five, 1, mode);
        let bytes = encoded(&new);
        assert_eq!(&bytes[4..8], &33u32.to_le_bytes());
        let tag = bytes.windows(4).rposition(|v| v == b"LBLD").unwrap();
        assert_eq!(&bytes[tag + 4..tag + 8], &5u32.to_le_bytes());
        assert_eq!(&bytes[8..tag], &v31[8..tag]);
        assert_eq!(bytes.len(), v31.len());
        assert_eq!(
            new.native_plan().unwrap().encoded_bound,
            five.native_plan().unwrap().encoded_bound
        );
        let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
        assert_eq!(encoded(&reopened), bytes);
        assert_eq!(reopened.layer(1).unwrap().blend_mode().to_c(), mode);
        assert_eq!(encoded(&changed(&reopened, 1, 13)), v31);
        assert_eq!(encoded(&changed(&reopened, 1, 10)), v30);
        assert_eq!(encoded(&changed(&reopened, 1, 3)), v27);
        let back = changed(&reopened, 1, 0);
        assert_eq!(encoded(&back), v26);
        assert_eq!(encoded(&changed(&back, 0, 0)), normal);
        for index in 0..2 {
            assert_eq!(
                reopened.layer_source(index).unwrap().to_rgba16().ok(),
                base.layer_source(index).unwrap().to_rgba16().ok()
            );
        }
        // Every historical schema stays frozen when current eligibility expands.
        for (version, schema) in [(26u32, 1u32), (27, 2), (30, 3), (31, 4)] {
            let mut bad = bytes.clone();
            bad[4..8].copy_from_slice(&version.to_le_bytes());
            bad[tag + 4..tag + 8].copy_from_slice(&schema.to_le_bytes());
            let idat = bad.windows(4).position(|v| v == b"IDAT").unwrap();
            bad[idat + 4] ^= 255;
            assert!(PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
                .err()
                .unwrap()
                .contains("blend mode"));
        }
    }
}

#[test]
fn hsl_modes_strict_ffi_and_hidden_predecode_refusals_preserve_input() {
    let adjustment = Layer::adjustment(
        "Hidden adjustment",
        r#"{"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}"#,
        1.,
        false,
        &IccProfile::srgb(),
    )
    .unwrap();
    let doc = document(
        1,
        vec![
            layer("A", vec![[1, 2, 3, 0]], 0, 1.),
            adjustment,
            Layer::pass_through("Passing", vec![], 1., false).unwrap(),
        ],
    );
    let before = encoded(&doc);
    for mode in MODES {
        let changed = changed(&doc, 0, mode);
        assert!(edit(&changed, request(0, mode), LIMIT, None)
            .unwrap()
            .is_none());
        // A typed adjustment carries the same modes (v37/schema7).
        assert!(edit(&doc, request(1, mode), LIMIT, None).unwrap().is_some());
        assert!(edit(&doc, request(0, mode), 1, None).is_err());
        let cancel = DrJobControl::default();
        cancel.cancel();
        assert!(edit(&doc, request(0, mode), LIMIT, Some(&cancel)).is_err());
    }
    for mode in [27, -1, 99] {
        let value = request(0, mode);
        assert!(edit(&doc, value.clone(), LIMIT, None).is_err());
        let json = CString::new(value.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let plan = unsafe { dr_precision_doc_edit_plan(&doc, json.as_ptr(), LIMIT, 0, &mut err) };
        assert!(plan.is_null() && !err.is_null());
        unsafe { dr_string_free(err) };
    }
    let new = changed(&doc, 0, 23);
    let bytes = encoded(&new);
    let tag = bytes.windows(4).rposition(|v| v == b"LBLD").unwrap();
    let idat = bytes.windows(4).position(|v| v == b"IDAT").unwrap();
    for (at, value, word) in [
        (tag + 4, 1, "schema"),
        (tag + 4, 2, "schema"),
        (tag + 4, 3, "schema"),
        (tag + 4, 4, "schema"),
        (tag + 4, 6, "schema"),
        (tag + 8, 0, "count"),
        (tag + 8, 4, "count"),
        (tag + 12, 3, "index"),
        (tag + 12, 1, "isolated"),
        (tag + 12, 2, "isolated"),
        (tag + 16, 21, "mode"),
        (tag + 16, 14, "mode"),
        (tag + 16, 16, "mode"),
        (8, 29, "body version"),
    ] {
        let mut bad = bytes.clone();
        bad[at..at + 4].copy_from_slice(&u32::to_le_bytes(value));
        bad[idat + 4] ^= 255;
        let error = PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
            .err()
            .unwrap();
        assert!(error.contains(word), "{error}");
    }
    for end in tag..bytes.len() {
        assert!(PrecisionDocument::parse_native(&bytes[..end], LIMIT, 0, None).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(PrecisionDocument::parse_native(&trailing, LIMIT, 0, None).is_err());
    assert_eq!(encoded(&doc), before);
}

#[test]
fn near_gray_setsat_witness_stays_inside_the_amplification_bound() {
    // blend.rs set_sat divides two f32 differences of near-equal operands, each
    // carrying <= 2^-24 absolute rounding: about 512 * s / delta codes of
    // output error. The bound doubles that on top of the ordinary 0.65.
    for (delta, saturated) in [
        (1u16, [0u16, 30000, 65535]),
        (2, [0, 30000, 65535]),
        (16, [0, 30000, 65535]),
        (1, [20000, 25000, 36384]),
        (2, [20000, 25000, 36384]),
        (16, [20000, 25000, 36384]),
    ] {
        let s = f64::from(saturated[2] - saturated[0]) / 65535.;
        assert!((s - 1.).abs() < 1e-9 || (s - 0.25).abs() < 1e-3);
        let near_gray = [32000, 32000 + delta / 2 + delta % 2, 32000 + delta];
        assert_eq!(near_gray[2] - near_gray[0], delta);
        let saturated = [saturated[0], saturated[1], saturated[2], 65535];
        let near_gray = [near_gray[0], near_gray[1], near_gray[2], 65535];
        for (mode, base, source) in [(23, saturated, near_gray), (24, near_gray, saturated)] {
            let (measured_delta, measured_s) =
                conditioning(mode, rgb(n(base)), rgb(n(source))).unwrap();
            assert!((measured_delta - f64::from(delta)).abs() < 1e-6);
            assert!((measured_s - s).abs() < 1e-9);
            assert!(!eligible(mode, rgb(n(base)), rgb(n(source))));
            let doc = document(
                1,
                vec![
                    layer("Backdrop", vec![base], 0, 1.),
                    layer("Source", vec![source], 0, 1.),
                ],
            );
            let actual = render(&changed(&doc, 1, mode))[0];
            let expected = over(n(base), n(source), 1., mode);
            let bound = 0.65 + 1024. * s / f64::from(delta);
            let mut maximum: f64 = 0.;
            for c in 0..3 {
                let error = (f64::from(actual[c]) - expected[c] * 65535.).abs();
                assert!(
                    error <= bound,
                    "mode {mode} delta {delta} s {s}: channel {c} error {error} exceeds {bound}"
                );
                maximum = maximum.max(error);
            }
            assert!((f64::from(actual[3]) - expected[3] * 65535.).abs() <= 0.65);
            eprintln!(
                "witness mode {mode} delta {delta} s {s}: maximum error {maximum} (bound {bound})"
            );
        }
    }
}

fn report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("report JSON")
}
fn open16(path: &Path) -> (Value, PrecisionDocument) {
    let notice = report(path);
    let losses: Vec<&str> = notice["verified_losses"]
        .as_array()
        .expect("verified_losses")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(losses.is_empty(), "{losses:?}");
    let expected = CString::new(notice["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            expected.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!document.is_null(), "{}", take_err_string(error));
    assert!(error.is_null());
    (notice, *unsafe { Box::from_raw(document) })
}
fn info(document: &PrecisionDocument) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { doctored_core::ffi_precision::dr_precision_doc_info(document, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("info JSON")
}
fn planes(rgba: &[[u16; 4]]) -> Vec<(i16, Vec<u8>)> {
    [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            let samples: Vec<u16> = rgba.iter().map(|p| p[component]).collect();
            (id, fixture::raw_plane(&samples))
        })
        .collect()
}

#[test]
fn hsl_modes_round_trip_through_sixteen_bit_psd_and_import_a_saturation_folder() {
    let directory = TempDir::new().expect("tempdir");
    let colors = eligible_colors();
    let width = 4u32;
    let base: Vec<[u16; 4]> = colors[13..17].to_vec();
    let mut layers = vec![layer("Base", base.clone(), 0, 1.)];
    for (k, mode) in MODES.into_iter().enumerate() {
        let pixels: Vec<[u16; 4]> = colors[14 + k..18 + k].to_vec();
        layers.push(
            layer(&format!("Mode {mode}"), pixels, 0, 1.)
                .with_blend_mode(BlendMode::from_c(mode).unwrap())
                .unwrap(),
        );
    }
    let source = document(width, layers);
    let projection = source.render(LIMIT, 0, None).unwrap();
    let path = directory.path().join("hsl16.psd");
    let mut error = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_psd(
                &source,
                &projection,
                cpath(&path).as_ptr(),
                0,
                LAYERS,
                LIMIT,
                0,
                std::ptr::null(),
                &mut error,
            )
        },
        "{}",
        take_err_string(error)
    );
    assert!(error.is_null());
    let file = fixture::read(&std::fs::read(&path).unwrap());
    let keys: Vec<[u8; 4]> = file.records.iter().map(|r| r.blend).collect();
    assert_eq!(
        keys,
        [*b"norm", *b"hue ", *b"sat ", *b"colr", *b"lum "],
        "one Photoshop key per mode"
    );
    let (_, back) = open16(&path);
    let layers = info(&back)["layers"].as_array().unwrap().clone();
    let modes: Vec<i64> = layers
        .iter()
        .map(|l| l["blend_mode"].as_i64().unwrap())
        .collect();
    assert_eq!(modes, [0, 23, 24, 25, 26]);
    assert_eq!(
        back.render(LIMIT, 0, None).unwrap().to_rgba16().ok(),
        projection.to_rgba16().ok()
    );
    for index in 0..5 {
        assert_eq!(
            back.layer_source(index).unwrap().to_rgba16().ok(),
            source.layer_source(index).unwrap().to_rgba16().ok()
        );
    }
    // A PSD import marks record 0 (word 113, `background_native`) first.
    assert_eq!(&encoded(&back)[4..8], &113u32.to_le_bytes());
    assert_eq!(&encoded(&back)[8..12], &33u32.to_le_bytes());

    // An independently authored closed `sat ` folder at opacity 128.
    let child: Vec<[u16; 4]> = colors[13..17].to_vec();
    let composite: Vec<[u16; 4]> = base.clone();
    let mut inner = Record::folder("Inner", 2, *b"sat ");
    inner.opacity = 128;
    let records = vec![
        Record::raster("Base", [0, 0, 1, width as i32], planes(&base)),
        Record::folder("</Layer group>", 3, *b"norm"),
        Record::raster("Child", [0, 0, 1, width as i32], planes(&child)),
        inner,
    ];
    let bytes = fixture::fixture(
        (width, 1),
        &records,
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &composite,
        true,
        false,
    );
    let authored = directory.path().join("authored-sat.psd");
    std::fs::write(&authored, bytes).unwrap();
    let (_, imported) = open16(&authored);
    let layers = info(&imported)["layers"].as_array().unwrap().clone();
    let names: Vec<&str> = layers.iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Base", "Child", "Inner"]);
    assert_eq!(layers[2]["kind"], Value::from("isolated_blend_group"));
    assert_eq!(layers[2]["blend_mode"], Value::from(24));
    assert_eq!(layers[2]["opacity"], Value::from(128.0f32 / 255.0));
    assert_eq!(
        imported.layer(2).unwrap().blend_mode(),
        BlendMode::Saturation
    );
    let expected = over(n(base[0]), n(child[0]), 128. / 255., 24);
    close_components(
        imported
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap()[0],
        expected,
        0.65,
    );
}
