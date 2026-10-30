//! Document-context Exposure/White Balance, through FFI. The White Balance
//! oracle decodes matrix tags independently, uses analytic inverse gamma/sRGB
//! encoding and a published-formula Bradford oracle, never the production ICC
//! transform; equivalent physical patches are authored independently in
//! multiple profiles. The Exposure oracle is the per-channel rule (#2):
//! each channel at the profile's effective pure gamma — 2.2 for the built-in
//! sRGB and P3 tables, the `curv` exponent for a pure-gamma profile.

use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_adjust::dr_image_adjust_op;
use doctored_core::ffi_calibrated::dr_doc_adjust_image;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_insert::dr_doc_adding_layer_at;
use doctored_core::icc::IccProfile;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use common::white_balance_oracle::{linear_white_balance, mat_vec};
use common::*;

struct TestImage(*mut DrImage);
impl std::ops::Deref for TestImage {
    type Target = DrImage;
    fn deref(&self) -> &DrImage {
        unsafe { &*self.0 }
    }
}
impl Drop for TestImage {
    fn drop(&mut self) {
        unsafe {
            dr_image_free(self.0);
        }
    }
}
fn test_image(pixels: &RgbaImage) -> TestImage {
    let raw =
        unsafe { dr_image_from_rgba(pixels.as_raw().as_ptr(), pixels.width(), pixels.height()) };
    assert!(!raw.is_null());
    TestImage(raw)
}

fn be32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn tag<'a>(bytes: &'a [u8], key: &[u8; 4]) -> &'a [u8] {
    for n in 0..be32(bytes, 128) as usize {
        let entry = 132 + n * 12;
        if &bytes[entry..entry + 4] == key {
            let start = be32(bytes, entry + 4) as usize;
            return &bytes[start..start + be32(bytes, entry + 8) as usize];
        }
    }
    panic!("missing tag");
}

/// Decode actual fixed-point matrix tags, not the core's parsed model. Its
/// primaries are fixture data; matrix inversion and transfer math are ours.
fn matrix(profile: &IccProfile) -> [[f64; 3]; 3] {
    let columns: [[f64; 3]; 3] = [b"rXYZ", b"gXYZ", b"bXYZ"].map(|key| {
        let data = tag(profile.bytes(), key);
        std::array::from_fn(|r| f64::from(be32(data, 8 + r * 4) as i32) / 65536.0)
    });
    std::array::from_fn(|r| std::array::from_fn(|c| columns[c][r]))
}

/// Gauss-Jordan elimination, independent of the core's cofactor inverse.
fn inverse(matrix: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut rows: [[f64; 6]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            if c < 3 {
                matrix[r][c]
            } else {
                f64::from(c - 3 == r)
            }
        })
    });
    for col in 0..3 {
        let pivot = (col..3)
            .max_by(|&a, &b| rows[a][col].abs().total_cmp(&rows[b][col].abs()))
            .unwrap();
        rows.swap(col, pivot);
        let scale = rows[col][col];
        for value in &mut rows[col] {
            *value /= scale;
        }
        for row in 0..3 {
            if row == col {
                continue;
            }
            let scale = rows[row][col];
            let pivot_row = rows[col];
            for (value, pivot_value) in rows[row].iter_mut().zip(pivot_row) {
                *value -= scale * pivot_value;
            }
        }
    }
    std::array::from_fn(|r| std::array::from_fn(|c| rows[r][c + 3]))
}

fn decode(v: u8, gamma: Option<f64>) -> f64 {
    let x = f64::from(v) / 255.0;
    match gamma {
        Some(g) => x.powf(g),
        None if x <= 0.04045 => x / 12.92,
        None => ((x + 0.055) / 1.055).powf(2.4),
    }
}

fn encode(x: f64, gamma: Option<f64>) -> u8 {
    let x = x.clamp(0.0, 1.0);
    let y = match gamma {
        Some(g) => x.powf(1.0 / g),
        None if x <= 0.0031308 => x * 12.92,
        None => 1.055 * x.powf(1.0 / 2.4) - 0.055,
    };
    (y * 255.0).round() as u8
}

/// Fresh ICC fixture: retained matrix tags, our own gamma curve and table.
fn gamma_profile(base: &IccProfile, gamma: u16) -> Arc<IccProfile> {
    let mut curve = b"curv\0\0\0\0".to_vec();
    curve.extend_from_slice(&1u32.to_be_bytes());
    curve.extend_from_slice(&gamma.to_be_bytes());
    trc_profile(base, curve)
}

/// Fresh ICC fixture: retained matrix tags, our own `para` curve (function
/// `kind`, `params` as s15Fixed16) and table.
fn para_profile(base: &IccProfile, kind: u16, params: &[f64]) -> Arc<IccProfile> {
    let mut curve = b"para\0\0\0\0".to_vec();
    curve.extend_from_slice(&kind.to_be_bytes());
    curve.extend_from_slice(&[0, 0]);
    for p in params {
        curve.extend_from_slice(&((p * 65536.0).round() as i32).to_be_bytes());
    }
    trc_profile(base, curve)
}

/// `base`'s matrix tags with `curve` as all three TRCs.
fn trc_profile(base: &IccProfile, curve: Vec<u8>) -> Arc<IccProfile> {
    let mut tags = Vec::new();
    for key in [b"rXYZ", b"gXYZ", b"bXYZ"] {
        tags.push((*key, tag(base.bytes(), key).to_vec()));
    }
    for key in [b"rTRC", b"gTRC", b"bTRC"] {
        tags.push((*key, curve.clone()));
    }
    let mut bytes = base.bytes()[..128].to_vec();
    bytes.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    bytes.resize(132 + 12 * tags.len(), 0);
    for (n, (key, data)) in tags.into_iter().enumerate() {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        let offset = bytes.len();
        bytes.extend_from_slice(&data);
        let entry = 132 + 12 * n;
        bytes[entry..entry + 4].copy_from_slice(&key);
        bytes[entry + 4..entry + 8].copy_from_slice(&(offset as u32).to_be_bytes());
        bytes[entry + 8..entry + 12].copy_from_slice(&(data.len() as u32).to_be_bytes());
    }
    let len = bytes.len() as u32;
    bytes[..4].copy_from_slice(&len.to_be_bytes());
    Arc::new(IccProfile::parse(&bytes).unwrap())
}

fn contexts() -> Vec<(Arc<IccProfile>, Option<f64>)> {
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    vec![
        (srgb.clone(), None),
        (p3.clone(), None),
        (gamma_profile(&srgb, 461), Some(461.0 / 256.0)),
        (gamma_profile(&p3, 563), Some(563.0 / 256.0)),
    ]
}

fn adjusted(doc: &DrDocument, image: &DrImage, op: &str, params: &Value) -> Vec<u8> {
    let op = CString::new(op).unwrap();
    let params = CString::new(params.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_adjust_image(doc, image, op.as_ptr(), params.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    assert!(err.is_null());
    let bytes = pixels(out);
    unsafe {
        dr_image_free(out);
    }
    bytes
}

fn oracle(doc: &DrDocument, gamma: Option<f64>, rgb: [u8; 3], op: &str, params: &Value) -> [u8; 3] {
    if op == "exposure" {
        // Per channel at the effective pure gamma (module doc): no matrix,
        // no TRC.
        let g = gamma.unwrap_or(2.2);
        let gain = params["exposure"].as_f64().unwrap_or(0.0).exp2();
        let offset = params["offset"].as_f64().unwrap_or(0.0);
        let power = params["gamma"].as_f64().unwrap_or(1.0);
        return rgb.map(|v| {
            let u = (f64::from(v) / 255.0).powf(g) * gain + offset;
            // Gamma Correction is the reciprocal power (measured against
            // Photoshop).
            let u = u.signum() * u.abs().powf(1.0 / power);
            (u.clamp(0.0, 1.0).powf(1.0 / g) * 255.0).round() as u8
        });
    }
    let reference = matrix(&IccProfile::srgb());
    let xyz = mat_vec(matrix(&doc.profile), rgb.map(|v| decode(v, gamma)));
    let linear = mat_vec(inverse(reference), xyz);
    let edited = linear_white_balance(
        linear,
        params["temperature"].as_f64().unwrap_or(6504.0),
        params["tint"].as_f64().unwrap_or(0.0),
    );
    mat_vec(inverse(matrix(&doc.profile)), mat_vec(reference, edited)).map(|v| encode(v, gamma))
}

#[test]
fn calibrated_operations_follow_an_independent_oracle_in_four_profiles() {
    let src = RgbaImage::from_fn(256, 3, |x, y| {
        Rgba([
            x as u8,
            ((x * 47 + y * 93) % 256) as u8,
            ((x * 19 + y * 71) % 256) as u8,
            [0, 129, 255][y as usize],
        ])
    });
    for (profile, gamma) in contexts() {
        let doc = DrDocument::from_pixels(src.clone())
            .assign_profile(&profile)
            .expect("a fresh document takes any RGB profile")
            .unwrap_or_else(|| DrDocument::from_pixels(src.clone()));
        for (op, params) in [
            (
                "exposure",
                json!({"evaluation":"document_v2", "exposure":0.7,"offset":0.017,"gamma":1.2}),
            ),
            (
                "exposure",
                json!({"evaluation":"document_v2", "exposure":-1.0,"offset":-0.01,"gamma":0.8}),
            ),
            (
                "white_balance",
                json!({"evaluation":"document_v2", "temperature":5200,"tint":28}),
            ),
            (
                "white_balance",
                json!({"evaluation":"document_v2", "temperature":11000,"tint":-40}),
            ),
        ] {
            let output = adjusted(&doc, &test_image(&src), op, &params);
            for (input, out) in src.pixels().zip(output.chunks_exact(4)) {
                let expected = oracle(&doc, gamma, [input[0], input[1], input[2]], op, &params);
                for c in 0..3 {
                    assert!(
                        (i16::from(out[c]) - i16::from(expected[c])).abs() <= 1,
                        "{op}, gamma={gamma:?}, input={input:?}: {out:?} vs {expected:?}"
                    );
                }
                assert_eq!(out[3], input[3]);
            }
        }
    }
}

#[test]
fn equivalent_physical_patches_remain_equivalent_after_white_balance() {
    let reference = matrix(&IccProfile::srgb());
    let patches = [
        [0.07, 0.10, 0.14],
        [0.20, 0.13, 0.09],
        [0.18, 0.22, 0.15],
        [0.25, 0.25, 0.25],
    ];
    // White Balance only: Exposure runs per channel in each document's own
    // primaries (#2), so equivalent colours in two profiles no longer
    // receive one operation.
    for (op, params) in [(
        "white_balance",
        json!({"evaluation":"document_v2","temperature":5600,"tint":15}),
    )] {
        let mut first: Option<Vec<[f64; 3]>> = None;
        for (profile, gamma) in contexts() {
            let to_xyz = matrix(&profile);
            let from_xyz = inverse(to_xyz);
            let source = RgbaImage::from_fn(patches.len() as u32, 1, |x, _| {
                let rgb = mat_vec(from_xyz, mat_vec(reference, patches[x as usize]))
                    .map(|v| encode(v, gamma));
                Rgba([rgb[0], rgb[1], rgb[2], 255])
            });
            let mut doc = DrDocument::from_pixels(source.clone());
            doc.profile = profile;
            let bytes = adjusted(&doc, &test_image(&source), op, &params);
            let xyz: Vec<_> = bytes
                .chunks_exact(4)
                .map(|px| mat_vec(to_xyz, [px[0], px[1], px[2]].map(|v| decode(v, gamma))))
                .collect();
            if let Some(first) = &first {
                for (a, b) in first.iter().zip(xyz) {
                    for c in 0..3 {
                        // Two independent input and output 8-bit quantizations;
                        // 0.004 XYZ is below one midtone encoded RGB code.
                        assert!((a[c] - b[c]).abs() < 0.004, "{op}: {a:?} vs {b:?}");
                    }
                }
            } else {
                first = Some(xyz);
            }
        }
    }
}

#[test]
fn defaults_are_identities_and_wide_gamut_exposure_runs_per_channel() {
    for (profile, _) in contexts() {
        let src = RgbaImage::from_fn(256, 1, |x, _| Rgba([x as u8, 255 - x as u8, 71, x as u8]));
        let mut doc = DrDocument::from_pixels(src.clone());
        doc.profile = profile;
        for op in ["exposure", "white_balance"] {
            assert_eq!(
                adjusted(
                    &doc,
                    &test_image(&src),
                    op,
                    &json!({"evaluation":"document_v2"})
                ),
                *src.as_raw()
            );
        }
    }
    let src = RgbaImage::from_pixel(1, 1, Rgba([0, 200, 0, 255]));
    let mut doc = DrDocument::from_pixels(src.clone());
    doc.profile = IccProfile::display_p3();
    let result = adjusted(
        &doc,
        &test_image(&src),
        "exposure",
        &json!({"evaluation":"document_v2","exposure":-1}),
    );
    // Per channel at 2.2 (the built-in P3 table): 200 · 0.5^(1/2.2).
    let green = (200.0 * 0.5f64.powf(1.0 / 2.2)).round() as u8;
    assert_eq!(result, [0, green, 0, 255]);
}

/// Rule Γ's `para` arm: a pure-power `para` (b = 0, ROMM's shape: γ 1.8 with
/// a linear toe below d) runs Exposure at its own exponent, toe ignored, not
/// at the 2.2 an offset curve takes.
#[test]
fn exposure_in_a_pure_power_para_profile_runs_at_its_own_exponent() {
    // Type 3: (a·x + b)^g at x >= d, else c·x. d = 1/16 is exact in
    // s15Fixed16, and c is the continuous toe's slope rounded DOWN to the
    // fixed grid, so the curve never steps down at d (the precision check).
    let fixed_g = (1.8f64 * 65536.0).round() / 65536.0;
    let d = 1.0f64 / 16.0;
    let c = (d.powf(fixed_g - 1.0) * 65536.0).floor() / 65536.0;
    let profile = para_profile(&IccProfile::srgb(), 3, &[fixed_g, 1.0, 0.0, c, d]);
    let src = RgbaImage::from_fn(256, 1, |x, _| {
        Rgba([
            x as u8,
            ((x * 47) % 256) as u8,
            ((x * 19 + 71) % 256) as u8,
            255,
        ])
    });
    let mut doc = DrDocument::from_pixels(src.clone());
    doc.profile = profile;
    let params = json!({"evaluation":"document_v2", "exposure":0.7,"offset":0.017,"gamma":1.2});
    let output = adjusted(&doc, &test_image(&src), "exposure", &params);
    let mut off_at_2_2 = 0;
    for (input, out) in src.pixels().zip(output.chunks_exact(4)) {
        let rgb = [input[0], input[1], input[2]];
        let expected = oracle(&doc, Some(fixed_g), rgb, "exposure", &params);
        let at_2_2 = oracle(&doc, None, rgb, "exposure", &params);
        for c in 0..3 {
            assert!(
                (i16::from(out[c]) - i16::from(expected[c])).abs() <= 1,
                "input={input:?}: {out:?} vs {expected:?} at gamma {fixed_g}"
            );
            if (i16::from(out[c]) - i16::from(at_2_2[c])).abs() > 1 {
                off_at_2_2 += 1;
            }
        }
    }
    assert!(off_at_2_2 > 0, "the case tells g from 2.2");
}

/// An offset `para` that PASSES the precision check keeps rule Γ's 2.2 at 8
/// and 16 bits: ITU-2020's shape (type 3: g 2.2222, a 0.90967, b 0.09033,
/// c 0.22222, d 0.08099; its toe ends just below the power branch's start)
/// is not the falling-boundary class, whose 1.883 applies only where the
/// check fails (#2).
#[test]
fn exposure_in_a_passing_offset_para_profile_stays_at_2_2() {
    let profile = para_profile(
        &IccProfile::srgb(),
        3,
        &[2.2222, 0.90967, 0.09033, 0.22222, 0.08099],
    );
    let src = RgbaImage::from_fn(256, 1, |x, _| {
        Rgba([
            x as u8,
            ((x * 47) % 256) as u8,
            ((x * 19 + 71) % 256) as u8,
            255,
        ])
    });
    let mut doc = DrDocument::from_pixels(src.clone());
    doc.profile = profile.clone();
    let params = json!({"evaluation":"document_v2", "exposure":0.7,"offset":0.017,"gamma":1.2});
    // 8 bits, through `dr_doc_adjust_image`.
    let output = adjusted(&doc, &test_image(&src), "exposure", &params);
    for (input, out) in src.pixels().zip(output.chunks_exact(4)) {
        let expected = oracle(
            &doc,
            Some(2.2),
            [input[0], input[1], input[2]],
            "exposure",
            &params,
        );
        for c in 0..3 {
            assert!(
                (i16::from(out[c]) - i16::from(expected[c])).abs() <= 1,
                "8-bit input={input:?}: {out:?} vs {expected:?} at 2.2"
            );
        }
    }
    // 16 bits, through the typed kernel: the same rule, unquantized, on
    // every 8-bit code x 257.
    let description = json!({
        "type": "adjust", "evaluation": "precision_v1", "op": "exposure", "params": params
    })
    .to_string();
    let op = PrecisionAdjustment::parse(&description, &profile).expect("a passing profile");
    let gain = 0.7f64.exp2();
    for v in 0..=255u16 {
        let x = f64::from(v * 257) / 65535.0;
        let u = x.powf(2.2) * gain + 0.017;
        let want = (u.powf(1.0 / 1.2).clamp(0.0, 1.0).powf(1.0 / 2.2) * 65535.0).round();
        let got = op.apply_at([x as f32; 3], 0.0, (0, 0));
        let got = (f64::from(got[0]).clamp(0.0, 1.0) * 65535.0).round();
        assert!(
            (got - want).abs() <= 1.0,
            "16-bit {v}: {got} vs {want} at 2.2"
        );
    }
}

#[test]
fn legacy_and_calibrated_descriptions_reopen_with_exact_destructive_layer_parity() {
    let dir = TempDir::new().unwrap();
    let src = RgbaImage::from_fn(256, 1, |x, _| {
        Rgba([x as u8, (x * 31) as u8, (x * 47) as u8, 255])
    });
    for (op, mut params) in [
        (
            "exposure",
            json!({"exposure":0.8,"offset":0.015,"gamma":1.2}),
        ),
        ("white_balance", json!({"temperature":5200,"tint":23})),
    ] {
        for evaluation in [None, Some("legacy_srgb"), Some("document_v2")] {
            if let Some(value) = evaluation {
                params["evaluation"] = json!(value);
            }
            let mut doc = DrDocument::from_pixels(src.clone());
            doc.profile = IccProfile::display_p3();
            let image = test_image(&src);
            let expected = adjusted(&doc, &image, op, &params);
            if evaluation != Some("document_v2") {
                let cop = CString::new(op).unwrap();
                let cp = CString::new(params.to_string()).unwrap();
                let mut err = ptr::null_mut();
                let original =
                    unsafe { dr_image_adjust_op(&*image, cop.as_ptr(), cp.as_ptr(), &mut err) };
                assert!(!original.is_null(), "{}", take_err_string(err));
                assert_eq!(
                    pixels(original),
                    expected,
                    "old exported kernel stays byte-exact"
                );
                unsafe {
                    dr_image_free(original);
                }
            }
            let meta = json!({"type":"adjust","op":op,"params":params}).to_string();
            let added = unsafe {
                dr_doc_adding_layer_at(
                    &doc,
                    0,
                    0, /* DR_INSERT_ABOVE */
                    CString::new("Adjustment").unwrap().as_ptr(),
                )
            };
            assert!(!added.is_null());
            let layer = unsafe {
                dr_doc_with_layer_meta(added, 1, CString::new(meta.clone()).unwrap().as_ptr())
            };
            assert!(!layer.is_null());
            let layer = unsafe { Box::from_raw(layer) };
            unsafe {
                dr_doc_free(added);
            }
            assert_eq!(*layer.flattened().unwrap().as_raw(), expected);
            let file = dir.path().join(format!("{op}-{evaluation:?}.rz"));
            layer.save_native(file.to_str().unwrap()).unwrap();
            let bytes = std::fs::read(&file).unwrap();
            // Since 12E-1 the outermost word is the always-written identity
            // wrapper (45) and the body word follows it.
            assert_eq!(be_version(&bytes), 45);
            assert_eq!(body_version(&bytes), 10);
            if evaluation.is_none() {
                // Version-8 descriptions had the same byte layout but no
                // calibrated key. Reopening one must keep its exact look — so
                // the file this writes is a genuine OLD one: the identity
                // wrapper and its `LIDS` tail are removed, not relabelled.
                let mut bytes = older(&bytes, layer.layers.len());
                bytes[4..8].copy_from_slice(&8u32.to_le_bytes());
                std::fs::write(&file, &bytes).unwrap();
            }
            let reopen = DrDocument::open(file.to_str().unwrap()).unwrap();
            assert_eq!(reopen.layers[1].meta.as_deref(), Some(meta.as_str()));
            assert_eq!(*reopen.flattened().unwrap().as_raw(), expected);
            assert_eq!(reopen.profile.bytes(), layer.profile.bytes());
        }
    }
}

fn be_version(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[4..8].try_into().unwrap())
}

fn body_version(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[8..12].try_into().unwrap())
}

/// A fresh save with the 12E-1 identity wrapper removed: the outermost word
/// and the `LIDS` tail (tag + schema + count + one u64 per record) are cut
/// away, so what is left is byte-for-byte what the prior writer produced.
fn older(bytes: &[u8], records: usize) -> Vec<u8> {
    let tail = 12 + 8 * records;
    let mut out = bytes[..4].to_vec();
    out.extend_from_slice(&bytes[8..bytes.len() - tail]);
    out
}

#[test]
fn invalid_evaluations_and_missing_colour_context_refuse_without_changing_inputs() {
    let src = RgbaImage::from_pixel(1, 1, Rgba([70, 120, 190, 127]));
    let image = test_image(&src);
    let mut doc = DrDocument::from_pixels(src.clone());
    let op = CString::new("exposure").unwrap();
    for params in [
        json!({"evaluation":"future_v3"}),
        json!({"evaluation":null}),
        json!([]),
    ] {
        let params = CString::new(params.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out =
            unsafe { dr_doc_adjust_image(&doc, &*image, op.as_ptr(), params.as_ptr(), &mut err) };
        assert!(out.is_null());
        assert!(!err.is_null());
        take_err_string(err);
    }
    let params = CString::new(r#"{"evaluation":"document_v2","exposure":1}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_image_adjust_op(&*image, op.as_ptr(), params.as_ptr(), &mut err) };
    assert!(out.is_null());
    assert!(take_err_string(err).contains("colour context"));
    // A valid RGB profile with no transform tags is kept but unmodellable.
    let mut profile = IccProfile::srgb().bytes()[..128].to_vec();
    profile.extend_from_slice(&0u32.to_be_bytes());
    profile[..4].copy_from_slice(&132u32.to_be_bytes());
    doc.profile = Arc::new(IccProfile::parse(&profile).unwrap());
    assert!(doc.profile.model().is_none());
    err = ptr::null_mut();
    let out = unsafe { dr_doc_adjust_image(&doc, &*image, op.as_ptr(), params.as_ptr(), &mut err) };
    assert!(out.is_null());
    assert!(take_err_string(err).contains("matrix/TRC"));
    assert_eq!(pixels(&*image), *src.as_raw());
    doc.layers[0].meta = Some(
        r#"{"type":"adjust","op":"exposure","params":{"evaluation":"document_v2","exposure":1}}"#
            .into(),
    );
    assert!(doc.flattened().unwrap_err().contains("matrix/TRC"));
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("rescue.rz");
    doc.save_native(file.to_str().unwrap()).unwrap();
    let reopen = DrDocument::open(file.to_str().unwrap()).unwrap();
    assert_eq!(doc.layers[0].meta, reopen.layers[0].meta);
    assert!(reopen.flattened().unwrap_err().contains("matrix/TRC"));
    assert!(unsafe {
        dr_doc_adjust_image(
            ptr::null(),
            &*image,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
        )
    }
    .is_null());
    assert!(unsafe {
        dr_doc_adjust_image(&doc, ptr::null(), ptr::null(), ptr::null(), ptr::null_mut())
    }
    .is_null());
}
