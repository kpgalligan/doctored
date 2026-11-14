//! Convert to Profile into and out of an RGB LUT (A2B/B2A) profile on a
//! 16-bit document: the default conversion and the explicit-options one run
//! the relative-colorimetric, BPC-off pipeline an 8-bit document runs, alpha
//! exact; a Grayscale document's RGB Color and an Indexed Color document's
//! colour table take the same pipeline; a calibrated (`document_v2`) Exposure
//! refuses under keep-parameters and bakes under preserve-appearance; a
//! linear-light document, a decode-only destination and a Lab Color document
//! refuse by sentence.
//!
//! The LUT is built here byte by byte (`common/icc_lut_builders`), the shape
//! `lut_authored_colors.rs` uses: an RGB `mntr` profile with an XYZ PCS whose
//! grid-2 `mft2` `B2A0` maps the table's encoded XYZ input `x` to
//! `d = 16/255 + x·219/255` per channel, and whose `A2B0` inverts it through
//! a 256-entry input curve. The oracle is this file's own arithmetic, calling
//! nothing in `icc_lut` or `icc_intent`: the IEC 61966-2-1 curve, sRGB's
//! published primaries Bradford-adapted to the ICC PCS white, the `mft2` XYZ
//! encoding (`XYZ · 32768 / 65535`), the corner map, and — out of the LUT —
//! the profile's own u16 input curve read by linear interpolation. Bound,
//! fixed before measuring: 2 codes of 65535 per channel, alpha exact. Out of
//! the LUT the colours keep every sRGB channel at 0.3 or above (or exactly 0
//! or 1): the built-in sRGB's 1024-entry `curv` is rounded to u16, which
//! below an encoded value of 0.2 is worth about 2 codes on its own.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_evaluation::dr_precision_doc_set_evaluation;
use doctored_core::ffi_gray::{dr_precision_doc_convert_gray_to_rgb, dr_precision_doc_open_gray};
use doctored_core::ffi_indexed::dr_precision_doc_open_indexed;
use doctored_core::ffi_lab::dr_precision_doc_new_lab;
use doctored_core::ffi_output_color::{
    dr_precision_doc_conversion_preflight, dr_precision_doc_convert_to_profile_with,
};
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_color_plan, dr_precision_doc_conversion_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::icc_lut::D50;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::{chunk, png, srgb_chunk};
use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_mft2_with_input, tag_wtpt};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const BOUND: f64 = 2.0;
const LUT_NAME: &str = "Synthetic 16-235 LUT";

// ------------------------------------------------------------- profiles --

fn corner(c: &[f64]) -> Vec<f64> {
    c.iter().map(|x| 16.0 / 255.0 + x * 219.0 / 255.0).collect()
}

/// The A2B0 input curve: `x = (d − 16/255)·255/219`, clamped, at d = k/255.
fn inverse_curve() -> Vec<u16> {
    (0..256)
        .map(|k| ((((k as f64) - 16.0) / 219.0).clamp(0.0, 1.0) * 65535.0).round() as u16)
        .collect()
}

fn lut(with_b2a: bool) -> Vec<u8> {
    let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", tag_desc(LUT_NAME)),
        (b"wtpt", tag_wtpt(D50)),
        (
            b"A2B0",
            tag_mft2_with_input(3, 3, 2, &inverse_curve(), &|c| c.to_vec()),
        ),
    ];
    if with_b2a {
        tags.push((b"B2A0", tag_mft2(3, 3, 2, &corner)));
    }
    build([0x02, 0x20, 0, 0], b"mntr", b"RGB ", b"XYZ ", &tags)
}

fn encodable() -> Vec<u8> {
    lut(true)
}

fn srgb() -> Vec<u8> {
    IccProfile::srgb().bytes().to_vec()
}

// --------------------------------------------------------------- oracle --

fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

fn apply(m: [[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|k| m[i][k] * v[k]).sum())
}

/// sRGB → PCS XYZ: the published sRGB primaries (ITU-R BT.709, D65 white
/// 0.3127, 0.3290) Bradford-adapted to the ICC's own PCS white, D50 =
/// (0.9642, 1, 0.8249). At 16 bits the PCS white matters: the matrix
/// published for D50 = (0.96422, 1, 0.82521) puts sRGB white 0.0003 higher
/// in Z, which is 9 codes of 65535 through this LUT.
fn srgb_d50() -> [[f64; 3]; 3] {
    let xyz = |x: f64, y: f64| [x / y, 1.0, (1.0 - x - y) / y];
    let columns = [xyz(0.64, 0.33), xyz(0.30, 0.60), xyz(0.15, 0.06)];
    let basis: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|k| columns[k][i]));
    let white = xyz(0.3127, 0.3290);
    let scale = apply(inverse(basis), white);
    let d65: [[f64; 3]; 3] =
        std::array::from_fn(|i| std::array::from_fn(|k| basis[i][k] * scale[k]));
    let bradford = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let (s, d) = (apply(bradford, white), apply(bradford, D50));
    let cone = [
        [d[0] / s[0], 0.0, 0.0],
        [0.0, d[1] / s[1], 0.0],
        [0.0, 0.0, d[2] / s[2]],
    ];
    mul(mul(inverse(bradford), mul(cone, bradford)), d65)
}

fn inverse(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let cof = |r: usize, c: usize| {
        let (r0, r1) = ((r + 1) % 3, (r + 2) % 3);
        let (c0, c1) = ((c + 1) % 3, (c + 2) % 3);
        m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
    };
    std::array::from_fn(|r| std::array::from_fn(|c| cof(c, r) / det))
}

/// sRGB unit values as the LUT document's continuous numbers.
fn into_lut(rgb: [f64; 3]) -> [f64; 3] {
    let xyz = apply(srgb_d50(), rgb.map(decode));
    xyz.map(|v| {
        let x = (v * 32768.0 / 65535.0).clamp(0.0, 1.0);
        16.0 / 255.0 + x * 219.0 / 255.0
    })
}

/// The LUT document's unit numbers as sRGB unit values: the profile's own
/// u16 input curve by linear interpolation, the `mft2` XYZ decoding, the
/// matrix inverted, clamped and encoded.
fn out_of_lut(device: [f64; 3]) -> [f64; 3] {
    let curve = inverse_curve();
    let x = device.map(|d| {
        let t = d.clamp(0.0, 1.0) * 255.0;
        let i = (t.floor() as usize).min(254);
        let f = t - i as f64;
        (f64::from(curve[i]) * (1.0 - f) + f64::from(curve[i + 1]) * f) / 65535.0
    });
    let xyz = x.map(|v| v * 65535.0 / 32768.0);
    apply(inverse(srgb_d50()), xyz).map(encode)
}

fn code(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// sRGB unit colours for the conversion INTO the LUT: black, white, ramps
/// and colours across the cube.
const INTO: [[f64; 3]; 10] = [
    [0.0, 0.0, 0.0],
    [1.0, 1.0, 1.0],
    [0.02, 0.02, 0.02],
    [0.18, 0.18, 0.18],
    [0.5, 0.5, 0.5],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.8, 0.3, 0.1],
    [0.18, 0.5, 0.9],
];

/// sRGB unit colours whose LUT numbers the conversion OUT of it starts from
/// (module doc: every channel at 0.3 or above, or exactly 0 or 1).
const OUT: [[f64; 3]; 7] = [
    [0.0, 0.0, 0.0],
    [1.0, 1.0, 1.0],
    [0.3, 0.3, 0.3],
    [0.5, 0.5, 0.5],
    [0.8, 0.4, 0.3],
    [0.35, 0.6, 0.9],
    [0.9, 0.85, 0.3],
];

// ------------------------------------------------------------ documents --

fn document(pixels: &[[u16; 4]], profile: &[u8]) -> *mut PrecisionDocument {
    let profile = Arc::new(IccProfile::parse(profile).expect("an RGB profile"));
    let image = PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels.to_vec(), profile, None)
        .expect("image");
    let doc = PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
        .expect("document");
    Box::into_raw(Box::new(doc))
}

fn samples(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(0)
        .expect("layer 0")
        .to_rgba16()
        .expect("rgba16")
}

fn text(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(s)
}

/// The converted document and its report, or the refusal.
fn convert(
    doc: *const PrecisionDocument,
    profile: &[u8],
    options: &str,
) -> Result<(*mut PrecisionDocument, Value), String> {
    let options = CString::new(options).unwrap();
    let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
    let out = unsafe {
        dr_precision_doc_convert_to_profile_with(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut report,
            &mut err,
        )
    };
    let report = text(report);
    if out.is_null() {
        return Err(text(err).unwrap_or_else(|| "no change".into()));
    }
    let report = report.map_or(Value::Null, |r| serde_json::from_str(&r).unwrap());
    Ok((out, report))
}

fn preflight(
    doc: *const PrecisionDocument,
    profile: &[u8],
    options: &str,
) -> Result<Value, String> {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_conversion_preflight(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    match text(out) {
        Some(json) => Ok(serde_json::from_str(&json).unwrap()),
        None => Err(text(err).unwrap_or_default()),
    }
}

/// Every pixel within [`BOUND`] codes of `want`, alpha exact.
fn assert_near(got: &[[u16; 4]], want: &[[u16; 4]], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..3 {
            assert!(
                (f64::from(g[c]) - f64::from(w[c])).abs() <= BOUND,
                "{what}: pixel {i} channel {c}: {g:?} against {w:?}"
            );
        }
        assert_eq!(g[3], w[3], "{what}: pixel {i} alpha");
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_sixteen_bit_document_converts_into_and_out_of_a_lut_profile() {
    let lut = encodable();
    // Into the LUT, at the default options and at options that take the
    // explicit-options path (nothing to bake, but not the defaults).
    let pixels: Vec<[u16; 4]> = INTO
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let [r, g, b] = c.map(code);
            [r, g, b, 65535 - 1000 * i as u16]
        })
        .collect();
    let want: Vec<[u16; 4]> = pixels
        .iter()
        .map(|p| {
            let unit = [p[0], p[1], p[2]].map(|v| f64::from(v) / 65535.0);
            let [r, g, b] = into_lut(unit).map(code);
            [r, g, b, p[3]]
        })
        .collect();
    for options in ["{}", "{\"adjustments\":\"preserve_appearance\"}"] {
        let doc = document(&pixels, &srgb());
        let (out, _) = convert(doc, &lut, options).unwrap_or_else(|e| panic!("{options}: {e}"));
        assert_eq!(
            unsafe { &*out }.profile().bytes().to_vec(),
            lut,
            "{options}"
        );
        assert_near(&samples(out), &want, &format!("sRGB → LUT {options}"));
        unsafe {
            dr_precision_doc_free(out);
            dr_precision_doc_free(doc);
        }
    }
    // Out of the LUT, back to sRGB, including the code past the table's
    // range (65535: XYZ above white, clamped to white).
    let mut pixels: Vec<[u16; 4]> = OUT
        .iter()
        .map(|c| {
            let [r, g, b] = into_lut(*c).map(code);
            [r, g, b, 65535]
        })
        .collect();
    pixels.push([65535, 65535, 65535, 65535]);
    let want: Vec<[u16; 4]> = pixels
        .iter()
        .map(|p| {
            let unit = [p[0], p[1], p[2]].map(|v| f64::from(v) / 65535.0);
            let [r, g, b] = out_of_lut(unit).map(code);
            [r, g, b, p[3]]
        })
        .collect();
    for options in ["{}", "{\"adjustments\":\"preserve_appearance\"}"] {
        let doc = document(&pixels, &lut);
        let (out, _) = convert(doc, &srgb(), options).unwrap_or_else(|e| panic!("{options}: {e}"));
        assert_eq!(
            unsafe { &*out }.profile().bytes().to_vec(),
            srgb(),
            "{options}"
        );
        assert_near(&samples(out), &want, &format!("LUT → sRGB {options}"));
        unsafe {
            dr_precision_doc_free(out);
            dr_precision_doc_free(doc);
        }
    }
}

#[test]
fn the_plans_charge_the_pipeline_and_a_decode_only_destination_refuses() {
    let lut = encodable();
    let doc = document(&[[30000, 20000, 10000, 65535]; 4], &srgb());
    for (name, json) in [
        ("color_plan", unsafe {
            let mut err = ptr::null_mut();
            let out =
                dr_precision_doc_color_plan(doc, lut.as_ptr(), lut.len(), true, LIMIT, 0, &mut err);
            text(out).ok_or_else(|| text(err).unwrap_or_default())
        }),
        ("conversion_plan", unsafe {
            let mut err = ptr::null_mut();
            let options = CString::new("{}").unwrap();
            let out = dr_precision_doc_conversion_plan(
                doc,
                lut.as_ptr(),
                lut.len(),
                options.as_ptr(),
                LIMIT,
                0,
                &mut err,
            );
            text(out).ok_or_else(|| text(err).unwrap_or_default())
        }),
    ] {
        let plan: Value =
            serde_json::from_str(&json.unwrap_or_else(|e| panic!("{name}: {e}"))).unwrap();
        assert!(
            plan["scratch_bytes"].as_u64().unwrap() > 0,
            "{name}: {plan}"
        );
    }
    assert_eq!(
        preflight(doc, &lut, "{}").unwrap()["adjustments"],
        Value::Array(vec![])
    );
    let decode_only = lut_decode_only();
    let error = preflight(doc, &decode_only, "{}").unwrap_err();
    assert!(
        error.contains(&format!(
            "the destination profile \"{LUT_NAME}\" carries no PCS to device table"
        )),
        "{error}"
    );
    assert!(convert(doc, &decode_only, "{}").is_err());
    unsafe { dr_precision_doc_free(doc) };
}

fn lut_decode_only() -> Vec<u8> {
    lut(false)
}

/// The byte rule at 16 bits (`doc_color_destination`): a calibrated
/// Exposure the LUT cannot evaluate refuses under keep, naming the layer and
/// offering the bake, and bakes under preserve-appearance; a linear-light
/// document refuses the destination with its sentence.
#[test]
fn a_calibrated_layer_or_linear_light_into_a_lut_follows_the_byte_rule() {
    let lut = encodable();
    let doc = document(&[[30000, 20000, 10000, 65535]; 4], &srgb());
    let name = CString::new("Exposure").unwrap();
    let description = CString::new(
        serde_json::json!({
            "type": "adjust",
            "evaluation": "precision_v1",
            "op": "exposure",
            "params": {"exposure": 0.7, "evaluation": "document_v2"},
        })
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let layered = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            name.as_ptr(),
            description.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!layered.is_null(), "{}", take_err_string(err));
    let refusal = format!(
        "\"Exposure\" uses calibrated Exposure/White Balance (document_v2), which only a \
         matrix/TRC profile can evaluate, and \"{LUT_NAME}\" is not one. Merge it down or delete \
         it, or convert to a matrix/TRC profile. Or choose preserve appearance to bake it."
    );
    assert_eq!(preflight(layered, &lut, "{}").unwrap_err(), refusal);
    assert_eq!(convert(layered, &lut, "{}").unwrap_err(), refusal);
    let preserve = "{\"adjustments\":\"preserve_appearance\"}";
    let rows = preflight(layered, &lut, preserve).unwrap()["adjustments"].clone();
    let reason = format!(
        "exposure with document_v2 evaluation cannot be evaluated in \"{LUT_NAME}\", so it is \
         baked into \"Exposure (baked)\""
    );
    assert_eq!(rows[0]["action"], "baked", "{rows}");
    assert_eq!(rows[0]["reason"], reason.as_str());
    let (out, report) = convert(layered, &lut, preserve).unwrap();
    assert_eq!(report[0]["action"], "baked", "{report}");
    let converted = unsafe { &*out };
    assert_eq!(converted.profile().bytes().to_vec(), lut);
    assert_eq!(
        converted.layer(1).map(|l| l.name().to_string()).as_deref(),
        Some("Exposure (baked)")
    );
    unsafe {
        dr_precision_doc_free(out);
        dr_precision_doc_free(layered);
    }
    // Linear-light evaluation needs the destination's matrix/TRC model.
    let mut err = ptr::null_mut();
    let linear = unsafe { dr_precision_doc_set_evaluation(doc, 1, &mut err) };
    assert!(!linear.is_null(), "{}", take_err_string(err));
    let sentence = format!(
        "“{LUT_NAME}” has no matrix/TRC model this build can decode, so linear-light evaluation \
         cannot be used with it. Convert to a matrix/TRC profile first, or keep encoded evaluation."
    );
    assert_eq!(preflight(linear, &lut, "{}").unwrap_err(), sentence);
    assert_eq!(convert(linear, &lut, "{}").unwrap_err(), sentence);
    unsafe {
        dr_precision_doc_free(linear);
        dr_precision_doc_free(doc);
    }
}

/// A Lab Color document's numbers are its codes, not colours of its twin:
/// the preflight and both plans refuse with its own sentence, which the
/// convert export already answers.
#[test]
fn a_lab_document_refuses_a_profile_conversion_at_every_entry() {
    let request = CString::new("{\"depth\":16,\"fill_srgb\":[0.5,0.4,0.3,1.0]}").unwrap();
    let mut err = ptr::null_mut();
    let lab = unsafe { dr_precision_doc_new_lab(4, 4, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!lab.is_null(), "{}", take_err_string(err));
    let sentence = "a Lab Color document has no document profile: it converts through Image ▸ \
                    Mode (dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_convert_to_cmyk, \
                    dr_precision_doc_convert_to_gray)";
    for profile in [srgb(), encodable()] {
        assert_eq!(preflight(lab, &profile, "{}").unwrap_err(), sentence);
        let mut err = ptr::null_mut();
        let plan = unsafe {
            dr_precision_doc_color_plan(
                lab,
                profile.as_ptr(),
                profile.len(),
                true,
                LIMIT,
                0,
                &mut err,
            )
        };
        assert_eq!(text(plan), None);
        assert_eq!(text(err).as_deref(), Some(sentence));
    }
    unsafe { dr_precision_doc_free(lab) };
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn open_with(
    path: &Path,
    open: impl Fn(*const c_char, *mut *mut c_char) -> *mut PrecisionDocument,
) -> *mut PrecisionDocument {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = open(c.as_ptr(), &mut err);
    assert!(!doc.is_null(), "open {path:?}: {}", take_err_string(err));
    doc
}

/// Image ▸ Mode ▸ RGB Color into the LUT from an sGray document runs the same
/// pipeline (the twin is sRGB's own curve and columns), and an Indexed Color
/// document's Convert converts its table through it, read at 8-bit codes.
#[test]
fn gray_to_rgb_and_an_indexed_table_convert_through_the_same_pipeline() {
    let dir = TempDir::new().unwrap();
    let lut = encodable();
    let levels = [0.0, 0.3, 0.5, 0.8, 1.0];
    let gray: Vec<u16> = levels.iter().map(|v| code(*v)).collect();
    let path = write(
        &dir,
        "sgray.png",
        &png(5, 1, 16, 0, 1, &gray, &[srgb_chunk()]),
    );
    let working = CString::new("sgray").unwrap();
    let doc = open_with(&path, |p, e| unsafe {
        dr_precision_doc_open_gray(p, working.as_ptr(), LIMIT, 0, ptr::null(), e)
    });
    let mut err = ptr::null_mut();
    let rgb = unsafe {
        dr_precision_doc_convert_gray_to_rgb(
            doc,
            lut.as_ptr(),
            lut.len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!rgb.is_null(), "{}", take_err_string(err));
    let want: Vec<[u16; 4]> = gray
        .iter()
        .map(|g| {
            let v = f64::from(*g) / 65535.0;
            let [r, g, b] = into_lut([v, v, v]).map(code);
            [r, g, b, 65535]
        })
        .collect();
    assert_near(&samples(rgb), &want, "sGray → RGB into the LUT");
    unsafe {
        dr_precision_doc_free(rgb);
        dr_precision_doc_free(doc);
    }
    // The Indexed table.
    let palette: [[u8; 3]; 3] = [[0, 0, 0], [200, 120, 60], [255, 255, 255]];
    let plte: Vec<u8> = palette.iter().flatten().copied().collect();
    let path = write(
        &dir,
        "indexed.png",
        &png(
            3,
            1,
            8,
            3,
            1,
            &[0, 1, 2],
            &[srgb_chunk(), chunk(b"PLTE", &plte)],
        ),
    );
    let doc = open_with(&path, |p, e| unsafe {
        dr_precision_doc_open_indexed(p, LIMIT, 0, ptr::null(), e)
    });
    let (out, _) = convert(doc, &lut, "{}").expect("the Indexed table converts into the LUT");
    assert_eq!(unsafe { &*out }.profile().bytes().to_vec(), lut);
    for (pixel, entry) in samples(out).iter().zip(palette) {
        let unit = entry.map(|v| f64::from(v) / 255.0);
        let want = into_lut(unit).map(|v| (v * 255.0).round());
        for c in 0..3 {
            let got = f64::from(pixel[c]) / 257.0;
            assert!(
                (got - want[c]).abs() <= 1.0,
                "table entry {entry:?} channel {c}: {got} against {}",
                want[c]
            );
        }
    }
    unsafe {
        dr_precision_doc_free(out);
        dr_precision_doc_free(doc);
    }
}
