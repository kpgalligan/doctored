//! An abstract Color Lookup look keeps its profile bytes under
//! `abstract_profile` and is re-baked through the new profile on every
//! profile write — Assign, a keep-parameters Convert and a Don't-Convert
//! paste — so a stored Abstract profile is evaluated through the
//! document's current one; XYZ abstracts load.
//!
//! Every bound is declared here before anything is measured:
//!
//! - a Lab abstract's params carry `abstract_profile` = the file's bytes;
//! - a byte-built `mft2` XYZ→XYZ IDENTITY abstract over sRGB: the identity to
//!   1e-3; an XYZ→XYZ abstract whose lut16 MATRIX is `M`, over an independent
//!   linear profile whose colorants are the identity (so document RGB IS PCS
//!   XYZ): `M · rgb` at every node, to 1e-3; a Lab→XYZ abstract loads;
//! - at 8 and 16 bits, after Assign, a keep-parameters Convert and a
//!   Don't-Convert paste, the look's baked keys are BIT FOR BIT what
//!   `dr_lut_parse_profile(fixture, new profile)` answers, `strength` is
//!   kept, and the Convert report says "kept";
//! - a preserve-appearance Convert bakes the look exactly as it bakes today's
//!   look (the same meta with no `abstract_profile`): same report, same
//!   composite;
//! - a `.cube` and a link meta are byte-identical through Assign;
//! - `.rz` word 71 at both depths: the look round-trips, a file whose word
//!   is removed or forged refuses by name, a look-free file carries no word
//!   and re-saves byte for byte;
//! - an abstract over 4 MiB refuses; `dr_lut_bake_profile` answers
//!   `dr_lut_parse_profile`'s JSON for the same bytes.

use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::*;
use doctored_core::ffi_adjust::*;
use doctored_core::ffi_color::dr_doc_assign_profile;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_layer_clipboard::{dr_doc_paste_layers, dr_precision_doc_paste_layers};
use doctored_core::ffi_output_color::*;
use doctored_core::ffi_precision::*;
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_assign_profile,
};
use doctored_core::ffi_precision_inspect::dr_precision_doc_adjustment_description;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use crate::icc_lut_builders::{build, tag_desc, tag_mft2};
use common::*;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SIZE: usize = 33;
const W: u32 = 12;
const H: u32 = 8;
const INSERT_TOP: u32 = 1;
const BAKED_KEYS: [&str; 8] = [
    "kind",
    "size",
    "source_size",
    "domain_min",
    "domain_max",
    "table",
    "title",
    "abstract_profile",
];

// ------------------------------------------------------------- plumbing --

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/icc_lookup")
        .join(name)
}

fn srgb() -> Vec<u8> {
    IccProfile::srgb().bytes().to_vec()
}

fn p3() -> Vec<u8> {
    IccProfile::display_p3().bytes().to_vec()
}

fn owned(text: *mut c_char, err: *mut c_char) -> Result<Value, String> {
    if text.is_null() {
        return Err(take_err_string(err));
    }
    assert!(err.is_null(), "err_out set on success");
    let s = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    Ok(serde_json::from_str(&s).expect("the core emits valid JSON"))
}

/// `dr_lut_parse_profile` on `path` through `document`.
fn parse(path: &Path, document: &[u8]) -> Result<Value, String> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let json =
        unsafe { dr_lut_parse_profile(c.as_ptr(), document.as_ptr(), document.len(), &mut err) };
    owned(json, err)
}

/// `dr_lut_bake_profile` on `look` through `document`.
fn bake(look: &[u8], document: &[u8]) -> Result<Value, String> {
    let mut err = ptr::null_mut();
    let json = unsafe {
        dr_lut_bake_profile(
            look.as_ptr(),
            look.len(),
            document.as_ptr(),
            document.len(),
            &mut err,
        )
    };
    owned(json, err)
}

/// RFC 4648 base64, decoded here rather than by the core.
fn b64_decode(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digits: Vec<u32> = text
        .bytes()
        .take_while(|&b| b != b'=')
        .map(|b| ALPHABET.iter().position(|&c| c == b).expect("base64 digit") as u32)
        .collect();
    let mut out = Vec::new();
    for chunk in digits.chunks(4) {
        let word = chunk
            .iter()
            .enumerate()
            .fold(0u32, |w, (i, &d)| w | d << (18 - 6 * i));
        let bytes = [(word >> 16) as u8, (word >> 8) as u8, word as u8];
        out.extend_from_slice(&bytes[..chunk.len() - 1]);
    }
    out
}

/// The stored 3D table as `SIZE³` RGB triples, red fastest.
fn table(params: &Value) -> Vec<[f32; 3]> {
    assert_eq!(params["size"], SIZE, "{params}");
    let bytes = b64_decode(params["table"].as_str().expect("table"));
    let floats: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    assert_eq!(floats.len(), SIZE * SIZE * SIZE * 3);
    floats.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect()
}

fn at(k: usize) -> f64 {
    k as f64 / (SIZE - 1) as f64
}

/// The worst channel difference between `params`' table and `oracle(rgb)`.
fn worst(params: &Value, oracle: impl Fn([f64; 3]) -> [f64; 3]) -> f64 {
    let table = table(params);
    let mut worst = 0.0f64;
    for b in 0..SIZE {
        for g in 0..SIZE {
            for r in 0..SIZE {
                let got = table[(b * SIZE + g) * SIZE + r];
                let want = oracle([at(r), at(g), at(b)]);
                for c in 0..3 {
                    worst = worst.max((f64::from(got[c]) - want[c]).abs());
                }
            }
        }
    }
    worst
}

/// `mft2` 3→3 identity-curve table with `matrix` (row-major) and a grid-2
/// CLUT through `clut`.
fn mft2_with_matrix(matrix: [f64; 9], clut: &dyn Fn(&[f64]) -> Vec<f64>) -> Vec<u8> {
    let mut tag = tag_mft2(3, 3, 2, clut);
    for (i, v) in matrix.iter().enumerate() {
        let fixed = (v * 65536.0).round() as i32;
        tag[12 + 4 * i..16 + 4 * i].copy_from_slice(&fixed.to_be_bytes());
    }
    tag
}

const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn abstract_profile(space: &[u8; 4], pcs: &[u8; 4], a2b0: Vec<u8>) -> Vec<u8> {
    build(
        [2, 0x10, 0, 0],
        b"abst",
        space,
        pcs,
        &[(b"desc", tag_desc("Built")), (b"A2B0", a2b0)],
    )
}

/// A matrix/TRC RGB profile whose colorants are the identity and whose
/// curves are gamma 1: its RGB numbers ARE PCS XYZ, which is what makes
/// `M · rgb` an oracle independent of the code under test.
fn linear_identity_profile() -> Vec<u8> {
    let xyz = |v: [f64; 3]| {
        let mut t = b"XYZ \0\0\0\0".to_vec();
        for c in v {
            t.extend_from_slice(&((c * 65536.0).round() as i32).to_be_bytes());
        }
        t
    };
    let mut curve = b"curv\0\0\0\0".to_vec();
    curve.extend_from_slice(&1u32.to_be_bytes());
    curve.extend_from_slice(&256u16.to_be_bytes());
    build(
        [2, 0x10, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Linear identity")),
            (b"rXYZ", xyz([1.0, 0.0, 0.0])),
            (b"gXYZ", xyz([0.0, 1.0, 0.0])),
            (b"bXYZ", xyz([0.0, 0.0, 1.0])),
            (b"rTRC", curve.clone()),
            (b"gTRC", curve.clone()),
            (b"bTRC", curve),
        ],
    )
}

/// The look's params with `strength` 0.6, as a host stores them.
fn look_params(document: &[u8]) -> Value {
    let mut params = parse(&fixture("p3_as_srgb.abst.icc"), document).expect("the look loads");
    params["strength"] = json!(0.6);
    params
}

/// `got`'s baked keys are bit for bit `want`'s, and `strength` is kept.
fn assert_rebaked(got: &Value, want: &Value, what: &str) {
    for key in BAKED_KEYS {
        assert_eq!(got.get(key), want.get(key), "{what}: {key}");
    }
    assert_eq!(got["strength"], json!(0.6), "{what}: strength");
}

fn opaque() -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        Rgba([(x * 20) as u8, (y * 30) as u8, ((x + y) * 9) as u8, 255])
    })
}

// ------------------------------------------------------ byte documents --

fn byte_doc(dir: &TempDir, meta: Option<&str>) -> *mut DrDocument {
    let doc = doc_from(dir, "base.png", &opaque());
    match meta {
        None => doc,
        Some(meta) => {
            let doc = add_layer(dir, "look.png", doc, 0, &opaque(), "Look");
            set_meta(doc, 1, meta)
        }
    }
}

fn look_meta(params: &Value) -> String {
    adjust_meta("color_lookup", &params.to_string())
}

fn byte_params(doc: *const DrDocument, index: usize) -> Value {
    let meta = ffi_meta(doc, index).expect("a look meta");
    let root: Value = serde_json::from_str(&meta).unwrap();
    root["params"].clone()
}

fn byte_assign(doc: *const DrDocument, profile: &[u8]) -> *mut DrDocument {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_assign_profile(doc, profile.as_ptr(), profile.len(), &mut err) };
    assert!(!out.is_null(), "assign: {}", take_err_string(err));
    out
}

fn byte_convert(doc: *const DrDocument, profile: &[u8], options: &str) -> (*mut DrDocument, Value) {
    let options = CString::new(options).unwrap();
    let mut report = ptr::null_mut();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_convert_to_profile_with(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            ptr::null(),
            &mut report,
            &mut err,
        )
    };
    assert!(!out.is_null(), "convert: {}", take_err_string(err));
    (out, owned(report, ptr::null_mut()).expect("a report"))
}

// ----------------------------------------------------- typed documents --

fn typed_doc(description: Option<&str>) -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = opaque()
        .pixels()
        .map(|p| {
            [
                u16::from(p[0]) * 257,
                u16::from(p[1]) * 257,
                u16::from(p[2]) * 257,
                65535,
            ]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let doc =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    let handle = Box::into_raw(Box::new(doc));
    let Some(description) = description else {
        return handle;
    };
    let name = CString::new("Look").unwrap();
    let description = CString::new(description).unwrap();
    let mut err = ptr::null_mut();
    let next = unsafe {
        dr_precision_doc_add_adjustment(
            handle,
            name.as_ptr(),
            description.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!next.is_null(), "{}", take_err_string(err));
    unsafe { dr_precision_doc_free(handle) };
    next
}

fn typed_description(params: &Value) -> String {
    json!({"type":"adjust","evaluation":"precision_v1","op":"color_lookup","params":params})
        .to_string()
}

fn typed_params(doc: *const PrecisionDocument, index: usize) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_adjustment_description(doc, index, &mut err) };
    owned(text, err).expect("a description")["params"].clone()
}

fn typed_assign(doc: *const PrecisionDocument, profile: &[u8]) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_assign_profile(
            doc,
            profile.as_ptr(),
            profile.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "typed assign: {}", take_err_string(err));
    out
}

fn typed_convert(
    doc: *const PrecisionDocument,
    profile: &[u8],
    options: &str,
) -> (*mut PrecisionDocument, Value) {
    let options = CString::new(options).unwrap();
    let mut report = ptr::null_mut();
    let mut err = ptr::null_mut();
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
    assert!(!out.is_null(), "typed convert: {}", take_err_string(err));
    (out, owned(report, ptr::null_mut()).expect("a report"))
}

fn typed_render(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "render: {}", take_err_string(err));
    let out = unsafe { &*image }.to_rgba16().expect("RGBA16");
    unsafe { dr_precision_image_free(image) };
    out
}

fn report_action(report: &Value, index: usize) -> String {
    report
        .as_array()
        .expect("an array")
        .iter()
        .find(|row| row["index"] == index)
        .map(|row| row["action"].as_str().unwrap().to_string())
        .expect("the look's row")
}

// ---------------------------------------------------------- the kernel --

#[test]
fn a_lab_abstract_keeps_its_profile_bytes() {
    let path = fixture("p3_as_srgb.abst.icc");
    let params = parse(&path, &srgb()).expect("the abstract loads");
    let kept = b64_decode(params["abstract_profile"].as_str().expect("the key"));
    assert_eq!(kept, std::fs::read(&path).unwrap());
    // A link keeps nothing: its numbers are the document's numbers.
    let link = parse(&fixture("srgb_to_p3.link.icc"), &srgb()).expect("the link loads");
    assert!(link.get("abstract_profile").is_none(), "{link}");
}

#[test]
fn xyz_abstracts_load_and_match_an_independent_oracle() {
    // Identity XYZ→XYZ over sRGB: the identity.
    let identity = abstract_profile(
        b"XYZ ",
        b"XYZ ",
        mft2_with_matrix(IDENTITY, &|x| x.to_vec()),
    );
    let params = bake(&identity, &srgb()).expect("an XYZ abstract loads");
    let error = worst(&params, |rgb| rgb);
    assert!(error <= 1e-3, "identity XYZ abstract drifted by {error}");

    // A non-identity lut16 matrix over the linear identity profile: M·rgb.
    // Every row sums to at most 1, so nothing clamps.
    let m = [0.5, 0.25, 0.0, 0.0, 0.75, 0.25, 0.25, 0.0, 0.5];
    let matrix = abstract_profile(b"XYZ ", b"XYZ ", mft2_with_matrix(m, &|x| x.to_vec()));
    let params = bake(&matrix, &linear_identity_profile()).expect("a matrix abstract loads");
    let error = worst(&params, |v| {
        [0, 1, 2].map(|i| m[3 * i] * v[0] + m[3 * i + 1] * v[1] + m[3 * i + 2] * v[2])
    });
    assert!(error <= 1e-3, "matrix XYZ abstract drifted by {error}");

    // A Lab→XYZ abstract loads (the matrix is the identity; the input is Lab).
    let lab_to_xyz = abstract_profile(
        b"Lab ",
        b"XYZ ",
        mft2_with_matrix(IDENTITY, &|x| x.to_vec()),
    );
    let params = bake(&lab_to_xyz, &srgb()).expect("a Lab→XYZ abstract loads");
    table(&params);
    assert!(params["abstract_profile"].is_string());
}

#[test]
fn an_oversized_abstract_refuses_and_bake_equals_parse() {
    let mut big = abstract_profile(b"Lab ", b"Lab ", tag_mft2(3, 3, 2, &|x| x.to_vec()));
    big.resize((4 << 20) + 16, 0);
    let size = big.len() as u32;
    big[0..4].copy_from_slice(&size.to_be_bytes());
    let message = bake(&big, &srgb()).expect_err("an oversized abstract");
    assert!(
        message.contains(&format!(
            "the abstract profile is too large to keep with its layer ({} bytes, max 4194304)",
            big.len()
        )),
        "{message}"
    );
    for name in ["p3_as_srgb.abst.icc", "srgb_to_p3.link.icc"] {
        let bytes = std::fs::read(fixture(name)).unwrap();
        assert_eq!(
            bake(&bytes, &srgb()).expect("bake"),
            parse(&fixture(name), &srgb()).expect("parse"),
            "{name}"
        );
    }
}

// ------------------------------------------------- re-bake, at 8 bits --

#[test]
fn byte_profile_writes_rebake_an_abstract_look() {
    let dir = TempDir::new().unwrap();
    let want = parse(&fixture("p3_as_srgb.abst.icc"), &p3()).unwrap();
    let doc = byte_doc(&dir, Some(&look_meta(&look_params(&srgb()))));

    let assigned = byte_assign(doc, &p3());
    assert_rebaked(&byte_params(assigned, 1), &want, "assign");
    unsafe { dr_doc_free(assigned) };

    let (converted, report) = byte_convert(doc, &p3(), "{}");
    assert_rebaked(&byte_params(converted, 1), &want, "convert keep");
    assert_eq!(report_action(&report, 1), "kept");
    unsafe { dr_doc_free(converted) };

    // A Don't-Convert paste of the sRGB clip into a P3 document.
    let plain = byte_doc(&dir, None);
    let destination = byte_assign(plain, &p3());
    let mut err = ptr::null_mut();
    let pasted =
        unsafe { dr_doc_paste_layers(destination, doc, 0, INSERT_TOP, 0, 0, true, &mut err) };
    assert!(!pasted.is_null(), "paste: {}", take_err_string(err));
    let top = unsafe { dr_doc_layer_count(pasted) } - 1;
    assert_rebaked(&byte_params(pasted, top), &want, "paste");
    unsafe {
        dr_doc_free(pasted);
        dr_doc_free(destination);
        dr_doc_free(plain);
        dr_doc_free(doc);
    }
}

#[test]
fn a_byte_preserve_appearance_convert_bakes_the_look_as_today() {
    let dir = TempDir::new().unwrap();
    let params = look_params(&srgb());
    let mut today = params.clone();
    today.as_object_mut().unwrap().remove("abstract_profile");
    let preserve = r#"{"adjustments":"preserve_appearance"}"#;
    let doc = byte_doc(&dir, Some(&look_meta(&params)));
    let twin = byte_doc(&dir, Some(&look_meta(&today)));
    let (converted, report) = byte_convert(doc, &p3(), preserve);
    let (expected, expected_report) = byte_convert(twin, &p3(), preserve);
    assert_eq!(report_action(&report, 1), "baked");
    assert_eq!(report, expected_report);
    assert_eq!(layer_name(converted, 1), "Look (baked)");
    assert_eq!(flat_pixels(converted), flat_pixels(expected));
    unsafe {
        dr_doc_free(converted);
        dr_doc_free(expected);
        dr_doc_free(doc);
        dr_doc_free(twin);
    }
}

#[test]
fn cube_and_link_metas_are_untouched_by_assign() {
    let dir = TempDir::new().unwrap();
    let cube = dir.path().join("look.cube");
    std::fs::write(
        &cube,
        "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n0.9 0.9 0.9\n",
    )
    .unwrap();
    let c = cpath(&cube);
    let mut err = ptr::null_mut();
    let cube_params = owned(unsafe { dr_lut_parse_cube(c.as_ptr(), &mut err) }, err).unwrap();
    let link_params = parse(&fixture("srgb_to_p3.link.icc"), &srgb()).unwrap();
    for (what, params) in [("cube", cube_params), ("link", link_params)] {
        let meta = look_meta(&params);
        let doc = byte_doc(&dir, Some(&meta));
        let assigned = byte_assign(doc, &p3());
        assert_eq!(
            ffi_meta(assigned, 1).as_deref(),
            Some(meta.as_str()),
            "{what}"
        );
        unsafe { dr_doc_free(assigned) };
        unsafe { dr_doc_free(doc) };

        let typed = typed_doc(Some(&typed_description(&params)));
        let before = typed_params(typed, 1);
        let assigned = typed_assign(typed, &p3());
        assert_eq!(typed_params(assigned, 1), before, "typed {what}");
        unsafe { dr_precision_doc_free(assigned) };
        unsafe { dr_precision_doc_free(typed) };
    }
}

// ------------------------------------------------ re-bake, at 16 bits --

#[test]
fn typed_profile_writes_rebake_an_abstract_look() {
    let want = parse(&fixture("p3_as_srgb.abst.icc"), &p3()).unwrap();
    let doc = typed_doc(Some(&typed_description(&look_params(&srgb()))));

    let assigned = typed_assign(doc, &p3());
    assert_rebaked(&typed_params(assigned, 1), &want, "typed assign");
    unsafe { dr_precision_doc_free(assigned) };

    let (converted, report) = typed_convert(doc, &p3(), "{}");
    assert_rebaked(&typed_params(converted, 1), &want, "typed convert keep");
    assert_eq!(report_action(&report, 1), "kept");
    unsafe { dr_precision_doc_free(converted) };

    let plain = typed_doc(None);
    let destination = typed_assign(plain, &p3());
    let mut err = ptr::null_mut();
    let pasted = unsafe {
        dr_precision_doc_paste_layers(
            destination,
            doc,
            0,
            INSERT_TOP,
            0,
            0,
            true,
            LIMIT,
            0,
            ptr::null_mut(),
            &mut err,
        )
    };
    assert!(!pasted.is_null(), "typed paste: {}", take_err_string(err));
    assert_rebaked(&typed_params(pasted, 2), &want, "typed paste");
    unsafe {
        dr_precision_doc_free(pasted);
        dr_precision_doc_free(destination);
        dr_precision_doc_free(plain);
        dr_precision_doc_free(doc);
    }
}

#[test]
fn a_typed_preserve_appearance_convert_bakes_the_look_as_today() {
    let params = look_params(&srgb());
    let mut today = params.clone();
    today.as_object_mut().unwrap().remove("abstract_profile");
    let preserve = r#"{"adjustments":"preserve_appearance"}"#;
    let doc = typed_doc(Some(&typed_description(&params)));
    let twin = typed_doc(Some(&typed_description(&today)));
    let (converted, report) = typed_convert(doc, &p3(), preserve);
    let (expected, expected_report) = typed_convert(twin, &p3(), preserve);
    assert_eq!(report_action(&report, 1), "baked");
    assert_eq!(report, expected_report);
    assert_eq!(typed_render(converted), typed_render(expected));
    unsafe {
        dr_precision_doc_free(converted);
        dr_precision_doc_free(expected);
        dr_precision_doc_free(doc);
        dr_precision_doc_free(twin);
    }
}

// ------------------------------------------------------------- word 71 --

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn byte_save(doc: *const DrDocument, path: &Path) -> Vec<u8> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, c.as_ptr(), &mut err) },
        "save: {}",
        take_err_string(err)
    );
    std::fs::read(path).unwrap()
}

fn byte_open(path: &Path) -> Result<*mut DrDocument, String> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

fn typed_save(doc: *const PrecisionDocument, path: &Path) -> Vec<u8> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "typed save: {}",
        take_err_string(err)
    );
    std::fs::read(path).unwrap()
}

fn typed_open(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

/// `bytes` with the four bytes at 8 removed, or `71` inserted there.
fn without_word(bytes: &[u8]) -> Vec<u8> {
    [&bytes[..8], &bytes[12..]].concat()
}

fn with_word(bytes: &[u8]) -> Vec<u8> {
    [&bytes[..8], &71u32.to_le_bytes(), &bytes[8..]].concat()
}

#[test]
fn byte_word_71_guards_the_kept_profile() {
    let dir = TempDir::new().unwrap();
    let params = look_params(&srgb());
    let doc = byte_doc(&dir, Some(&look_meta(&params)));
    let path = dir.path().join("look.rz");
    let bytes = byte_save(doc, &path);
    assert_eq!((word(&bytes, 4), word(&bytes, 8)), (45, 71));
    let reopened = byte_open(&path).expect("the look reopens");
    assert_eq!(ffi_meta(reopened, 1), ffi_meta(doc, 1));
    assert_eq!(flat_pixels(reopened), flat_pixels(doc));
    unsafe { dr_doc_free(reopened) };

    let forged = dir.path().join("forged.rz");
    std::fs::write(&forged, without_word(&bytes)).unwrap();
    let message = byte_open(&forged).unwrap_err();
    assert!(
        message.ends_with(": Color Lookup abstract looks require DRDC 71"),
        "{message}"
    );

    // A look-free document writes no word and re-saves byte for byte.
    let plain = byte_doc(
        &dir,
        Some(&adjust_meta("curves", r#"{"rgb":[[0,0],[255,240]]}"#)),
    );
    let plain_path = dir.path().join("plain.rz");
    let plain_bytes = byte_save(plain, &plain_path);
    assert_eq!(word(&plain_bytes, 4), 45);
    assert_ne!(word(&plain_bytes, 8), 71);
    let reopened = byte_open(&plain_path).expect("the plain file reopens");
    assert_eq!(
        byte_save(reopened, &dir.path().join("again.rz")),
        plain_bytes
    );
    unsafe { dr_doc_free(reopened) };
    std::fs::write(&forged, with_word(&plain_bytes)).unwrap();
    let message = byte_open(&forged).unwrap_err();
    assert!(message.ends_with(": invalid native look word"), "{message}");
    unsafe {
        dr_doc_free(plain);
        dr_doc_free(doc);
    }
}

#[test]
fn typed_word_71_guards_the_kept_profile() {
    let dir = TempDir::new().unwrap();
    let doc = typed_doc(Some(&typed_description(&look_params(&srgb()))));
    let path = dir.path().join("look.rz");
    let bytes = typed_save(doc, &path);
    assert_eq!((word(&bytes, 4), word(&bytes, 8)), (46, 71));
    let reopened = typed_open(&path).expect("the typed look reopens");
    assert_eq!(typed_params(reopened, 1), typed_params(doc, 1));
    assert_eq!(typed_render(reopened), typed_render(doc));
    unsafe { dr_precision_doc_free(reopened) };

    let forged = dir.path().join("forged.rz");
    std::fs::write(&forged, without_word(&bytes)).unwrap();
    assert_eq!(
        typed_open(&forged).unwrap_err(),
        "Color Lookup abstract looks require DRDC 71"
    );

    let plain = typed_doc(None);
    let plain_path = dir.path().join("plain.rz");
    let plain_bytes = typed_save(plain, &plain_path);
    assert_eq!(word(&plain_bytes, 4), 46);
    assert_ne!(word(&plain_bytes, 8), 71);
    let reopened = typed_open(&plain_path).expect("the plain file reopens");
    assert_eq!(
        typed_save(reopened, &dir.path().join("again.rz")),
        plain_bytes
    );
    unsafe { dr_precision_doc_free(reopened) };
    std::fs::write(&forged, with_word(&plain_bytes)).unwrap();
    assert_eq!(typed_open(&forged).unwrap_err(), "invalid native look word");
    unsafe {
        dr_precision_doc_free(plain);
        dr_precision_doc_free(doc);
    }
}
