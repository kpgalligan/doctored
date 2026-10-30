//! Five additional continuous native modes.
//! The product continues to use its existing f32 blend.rs kernels unchanged.
use doctored_core::doc::BlendMode;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

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
const MODES: [i32; 5] = [13, 18, 19, 20, 22];
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
// Independent encoded-RGB equations. Divide's zero divisor is white except at
// 0/0, which keeps the backdrop's 0, and Vivid Light's halves take a black
// source to 0 and a white one to 1 before the backdrop's ends: the edge
// conventions take1 measured (`scripts/photoshop-probe/blend_edge_take.py`).
fn blend(b: f64, s: f64, mode: i32) -> f64 {
    match mode {
        0 => s,
        13 => (b - s).max(0.),
        18 if s == 0. => 0.,
        18 if s == 1. => 1.,
        18 if s <= 0.5 => {
            if b == 1. {
                1.
            } else if s == 0. {
                0.
            } else {
                1. - ((1. - b) / (2. * s)).min(1.)
            }
        }
        18 => {
            if b == 0. {
                0.
            } else if s == 1. {
                1.
            } else {
                (b / (2. * (1. - s))).min(1.)
            }
        }
        19 => (b + 2. * s - 1.).clamp(0., 1.),
        20 if s <= 0.5 => b.min(2. * s),
        20 => b.max(2. * s - 1.),
        22 if s == 0. && b == 0. => 0.,
        22 if s == 0. => 1.,
        22 => (b / s).min(1.),
        _ => panic!("unqualified oracle mode"),
    }
}
fn over(b: [f64; 4], s: [f64; 4], weight: f64, mode: i32) -> [f64; 4] {
    let sa = s[3] * weight;
    let alpha = sa + b[3] * (1. - sa);
    if alpha == 0. {
        return [0.; 4];
    }
    let mut out = [0.; 4];
    out[3] = alpha;
    for c in 0..3 {
        out[c] = (sa * ((1. - b[3]) * s[c] + b[3] * blend(b[c], s[c], mode))
            + (1. - sa) * b[3] * b[c])
            / alpha;
    }
    out
}
fn close_components(actual: [u16; 4], expected: [f64; 4], tolerances: [f64; 3]) -> f64 {
    let mut maximum: f64 = 0.;
    for c in 0..4 {
        let error = (f64::from(actual[c]) - expected[c] * 65535.).abs();
        assert!(
            error <= if c == 3 { 0.65 } else { tolerances[c] },
            "channel {c}: {actual:?}, {expected:?}, error {error}"
        );
        maximum = maximum.max(error);
    }
    maximum
}
fn close(actual: [u16; 4], expected: [f64; 4], tolerance: f64) -> f64 {
    close_components(actual, expected, [tolerance; 3])
}
fn authored_tolerances(base: [u16; 4], source: [u16; 4], mode: i32) -> [f64; 3] {
    std::array::from_fn(|c| {
        // Only Vivid's transformed Dodge/Burn divisors near an endpoint
        // receive the qualified finite-fixture allowance. Authored integer
        // equality pins the exact saturation boundary independently of f32.
        let b = u32::from(base[c]);
        let s = u32::from(source[c]);
        if mode == 18
            && ((s >= 64880 && b > 0 && b <= 2 * (65535 - s))
                || (s <= 655 && b < 65535 && 65535 - b <= 2 * s))
        {
            1.6
        } else {
            0.65
        }
    })
}

#[test]
fn five_modes_endpoints_saturation_neighbors_and_documented_f32_division_bound() {
    let codes = [
        0, 1, 2, 3, 17, 53, 54, 106, 107, 108, 169, 170, 171, 212, 213, 214, 255, 256, 257, 339,
        340, 341, 655, 656, 16383, 16384, 32766, 32767, 32768, 32769, 49151, 64879, 64880, 65364,
        65365, 65366, 65427, 65428, 65429, 65532, 65533, 65534, 65535,
    ];
    let mut base = Vec::new();
    let mut source = Vec::new();
    for &b in &codes {
        for &s in &codes {
            base.push([b, s, 65535 - b, 65535]);
            source.push([s, b, 65535 - s, 65535]);
        }
    }
    let doc = document(
        base.len() as u32,
        vec![
            layer("Backdrop", base.clone(), 0, 1.),
            layer("Source", source.clone(), 0, 1.),
        ],
    );
    let witness_base = [212, 212, 212, 65535];
    for source_code in [65427, 65428, 65429] {
        let witness_source = [source_code, source_code, source_code, 65535];
        let witness = document(
            1,
            vec![
                layer("Wb", vec![witness_base], 0, 1.),
                layer("Ws", vec![witness_source], 0, 1.),
            ],
        );
        let actual = render(&changed(&witness, 1, 18))[0];
        let rational = (65535. * 106. / f64::from(65535 - source_code)).min(65535.);
        assert!(
            (over(n(witness_base), n(witness_source), 1., 18)[0] * 65535. - rational).abs() < 1e-7
        );
        let error = (f64::from(actual[0]) - rational).abs();
        assert!(error <= 1.6);
        if source_code == 65428 {
            assert!(error > 1.);
        }
    }
    for mode in MODES {
        let mut maximum: f64 = 0.;
        for (i, p) in render(&changed(&doc, 1, mode)).into_iter().enumerate() {
            maximum = maximum.max(close_components(
                p,
                over(n(base[i]), n(source[i]), 1., mode),
                authored_tolerances(base[i], source[i], mode),
            ));
        }
        eprintln!("mode {mode} maximum unrounded-f64 code error: {maximum}");
        // This witness requires the documented division bound, rather than
        // silently pretending the shared f32 kernel equals a double kernel.
        if mode == 18 {
            assert!(maximum > 1.);
        }
    }
}
#[test]
fn five_modes_every_native_mask_code_partial_alpha_density_and_opacity() {
    let base: Vec<_> = (0..=u16::MAX)
        .map(|i| {
            [
                i,
                65535 - i,
                i.wrapping_add(173),
                [0, 1, 32769, 65535][i as usize % 4],
            ]
        })
        .collect();
    let source: Vec<_> = (0..=u16::MAX)
        .map(|i| {
            [
                65535 - i,
                i.wrapping_add(17),
                23457,
                [65535, 32768, 1, 0, 45001][i as usize % 5],
            ]
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
        for (i, p) in render(&next).into_iter().enumerate() {
            close_components(
                p,
                over(
                    n(base[i]),
                    n(source[i]),
                    crate::common::byte_unit(0.625) * (0.25 + 0.75 * i as f64 / 65535.),
                    mode,
                ),
                authored_tolerances(base[i], source[i], mode),
            );
        }
        assert_eq!(
            next.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
            source
        );
        assert_eq!(doc.layer(1).unwrap().blend_mode(), BlendMode::Normal);
    }
}
#[test]
fn five_modes_meaningful_alpha_translated_nested_groups_and_pass_through() {
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
            close(result[x], expected, 0.65);
        }
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
fn five_mode_v31_downgrades_exactly_and_old_schemas_refuse_new_meaning() {
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
    for mode in MODES {
        let new = changed(&four, 1, mode);
        let bytes = encoded(&new);
        assert_eq!(&bytes[4..8], &31u32.to_le_bytes());
        let tag = bytes.windows(4).rposition(|v| v == b"LBLD").unwrap();
        assert_eq!(&bytes[tag + 4..tag + 8], &4u32.to_le_bytes());
        assert_eq!(&bytes[8..tag], &v27[8..tag]);
        assert_eq!(bytes.len(), v27.len());
        assert_eq!(
            new.native_plan().unwrap().encoded_bound,
            separable.native_plan().unwrap().encoded_bound
        );
        let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
        assert_eq!(encoded(&changed(&reopened, 1, 10)), v30);
        assert_eq!(encoded(&reopened), bytes);
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
        for (version, schema) in [(26u32, 1u32), (27, 2), (30, 3)] {
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
fn five_modes_strict_ffi_and_hidden_predecode_refusals_preserve_input() {
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
    let new = changed(&doc, 0, 18);
    let bytes = encoded(&new);
    let tag = bytes.windows(4).rposition(|v| v == b"LBLD").unwrap();
    let idat = bytes.windows(4).position(|v| v == b"IDAT").unwrap();
    for (at, value, word) in [
        (tag + 4, 1, "schema"),
        (tag + 4, 2, "schema"),
        (tag + 4, 3, "schema"),
        (tag + 4, 5, "schema"),
        (tag + 8, 0, "count"),
        (tag + 8, 4, "count"),
        (tag + 12, 3, "index"),
        (tag + 12, 1, "isolated"),
        (tag + 12, 2, "isolated"),
        (tag + 16, 21, "mode"),
        (tag + 16, 14, "mode"),
        (tag + 16, 16, "mode"),
        (tag + 16, 23, "mode"),
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
fn exact_half_source_group_pins_vivid_linear_and_pin_light_seams() {
    let backdrop = [12345, 34567, 56789, 65535];
    for mode in [18, 19, 20] {
        // Native u16 has no exact .5 code, and since #2 neither has a
        // layer opacity (its byte: 50 % is 128 / 255). This opaque
        // group's black plus white under a hidden mask at density 0.5
        // (coverage `1 − 0.5·(1 − 0)`, exactly .5) produces .5 without an
        // intermediate u16 store.
        let group = Layer::isolated_normal(
            "Exact half",
            vec![
                layer("Black", vec![[0, 0, 0, 65535]], 0, 1.),
                layer("White", vec![[65535; 4]], 0, 1.).with_mask(Some(mask(vec![0], 0.5))),
            ],
            0.625,
            true,
        )
        .unwrap()
        .with_blend_mode(BlendMode::from_c(mode).unwrap())
        .unwrap();
        let doc = document(1, vec![layer("Backdrop", vec![backdrop], 0, 1.), group]);
        let expected = over(
            n(backdrop),
            [0.5, 0.5, 0.5, 1.],
            crate::common::byte_unit(0.625),
            mode,
        );
        close(render(&doc)[0], expected, 0.65);
        assert_eq!(render(&doc)[0], backdrop);
    }
}
