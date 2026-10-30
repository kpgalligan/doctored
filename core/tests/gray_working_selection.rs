//! Load Selection's luminosity at 8 and 16 bits in Color Settings' Gray
//! working space (`gray_working`): the composite's grey
//! is `Y^e`, Y the document profile's relative PCS Y of the encoded RGB, e
//! the space's exponent (Dot Gain d %: ln 0.5 / ln(0.5 − d/100); Gray Gamma
//! γ: 1/γ). 8 bits through `dr_doc_composite_gray_plane`, 16 bits through
//! the native `luminance` selection with `gray`.
//!
//! Oracles: Photoshop 26.10.0's readings on its 26 patches
//! (`scripts/photoshop-probe/gray_luminance_take.py` take1,
//! `doctored-fixtures/photoshop/gray-luminance/take1`, `decided.json`; the
//! same codes at 8 and 16 bits) within 1 code, and this file's own formula
//! from the IEC 61966-2.1 curve and the built-in sRGB's Y row (its
//! rXYZ/gXYZ/bXYZ tags, read here) for all seven spaces.

use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::dr_doc_assign_profile;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_gray_plane::dr_doc_composite_gray_plane;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit_plan, dr_precision_doc_select};
use doctored_core::icc::IccProfile;
use doctored_core::icc_lut::D50;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_structure_fixture::{cmyk_layers, free, RgbLayer};
use crate::common::doc_from;
use crate::float_fixture::float_doc;
use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_wtpt};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// The take's patches: eight neutrals, then 18 colours (`Random(4421)`).
const PATCHES: [[u8; 3]; 26] = [
    [8, 8, 8],
    [16, 16, 16],
    [24, 24, 24],
    [48, 48, 48],
    [96, 96, 96],
    [128, 128, 128],
    [160, 160, 160],
    [224, 224, 224],
    [232, 68, 88],
    [58, 28, 182],
    [65, 144, 48],
    [179, 95, 102],
    [200, 31, 174],
    [198, 35, 22],
    [145, 10, 8],
    [146, 74, 252],
    [255, 195, 65],
    [226, 13, 100],
    [186, 50, 213],
    [126, 91, 155],
    [44, 95, 115],
    [189, 217, 241],
    [58, 52, 132],
    [122, 227, 175],
    [50, 135, 99],
    [174, 27, 143],
];

/// Photoshop's `sel_rgb` codes per patch, by Gray working space.
const TAKE: [(&str, [u8; 26]); 3] = [
    (
        "dot_gain_20",
        [
            8, 12, 17, 34, 74, 105, 140, 215, 109, 43, 105, 98, 90, 82, 53, 93, 193, 95, 93, 83,
            67, 201, 44, 194, 98, 75,
        ],
    ),
    (
        "gray_gamma_22",
        [
            17, 23, 30, 51, 96, 127, 159, 223, 130, 63, 126, 120, 112, 104, 74, 115, 205, 117, 115,
            105, 89, 212, 64, 205, 119, 97,
        ],
    ),
    (
        "dot_gain_15",
        [
            5, 8, 11, 25, 62, 93, 128, 210, 96, 33, 92, 85, 77, 69, 42, 80, 185, 82, 80, 70, 55,
            195, 34, 186, 85, 62,
        ],
    ),
];

/// The seven spaces with their exponents, written from their definitions.
fn spaces() -> [(&'static str, f64); 7] {
    let dot_gain = |d: f64| 0.5f64.ln() / (0.5 - d / 100.0).ln();
    [
        ("dot_gain_10", dot_gain(10.0)),
        ("dot_gain_15", dot_gain(15.0)),
        ("dot_gain_20", dot_gain(20.0)),
        ("dot_gain_25", dot_gain(25.0)),
        ("dot_gain_30", dot_gain(30.0)),
        ("gray_gamma_18", 1.0 / 1.8),
        ("gray_gamma_22", 1.0 / 2.2),
    ]
}

// --------------------------------------------------------------- oracle --

fn be32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// The Y entries of a profile's rXYZ, gXYZ and bXYZ tags (s15Fixed16).
fn y_row(profile: &IccProfile) -> [f64; 3] {
    let icc: &[u8] = profile.bytes();
    let found = |sig: &[u8; 4]| {
        (0..be32(icc, 128) as usize)
            .map(|n| 132 + n * 12)
            .find(|&entry| &icc[entry..entry + 4] == sig)
            .map(|entry| be32(icc, entry + 4) as usize)
            .unwrap_or_else(|| panic!("no {} tag", String::from_utf8_lossy(sig)))
    };
    [b"rXYZ", b"gXYZ", b"bXYZ"].map(|sig| {
        let at = found(sig);
        assert_eq!(&icc[at..at + 4], b"XYZ ");
        f64::from(be32(icc, at + 12) as i32) / 65536.0
    })
}

/// IEC 61966-2.1's decode of a unit value.
fn decode(u: f64) -> f64 {
    if u <= 0.04045 {
        u / 12.92
    } else {
        ((u + 0.055) / 1.055).powf(2.4)
    }
}

/// The grey (0…1) of unit RGB `rgb` at exponent `e`.
fn grey(rgb: [f64; 3], e: f64) -> f64 {
    let y = y_row(&IccProfile::srgb());
    let y = (0..3).map(|c| y[c] * decode(rgb[c])).sum::<f64>();
    y.clamp(0.0, 1.0).powf(e)
}

// ------------------------------------------------------------ fixtures --

/// A `n × 1` 8-bit document of `pixels` (opaque).
fn byte_doc(dir: &TempDir, pixels: &[[u8; 3]]) -> *mut doctored_core::doc::DrDocument {
    let n = pixels.len() as u32;
    let image = RgbaImage::from_fn(n, 1, |x, _| {
        let p = pixels[x as usize];
        Rgba([p[0], p[1], p[2], 255])
    });
    doc_from(dir, "gray.png", &image)
}

fn gray_plane(doc: *const doctored_core::doc::DrDocument, space: &str, w: u32) -> Option<Vec<u8>> {
    let name = CString::new(space).unwrap();
    let mut out = vec![0u8; w as usize];
    let ok = unsafe { dr_doc_composite_gray_plane(doc, name.as_ptr(), out.as_mut_ptr(), w, 1) };
    ok.then_some(out)
}

/// A `n × 1` 16-bit document of `pixels` in `profile`.
fn u16_doc(pixels: &[[u16; 3]], profile: Arc<IccProfile>) -> PrecisionDocument {
    let n = pixels.len() as u32;
    let samples = pixels.iter().map(|p| [p[0], p[1], p[2], 65535]).collect();
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(n, 1, samples, profile, None).unwrap(),
        Default::default(),
        Default::default(),
    )
    .unwrap()
}

fn ffi_error(error: *mut std::ffi::c_char) -> String {
    assert!(!error.is_null());
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    text
}

fn luminance(gray: Option<&str>) -> Value {
    let mut operation = json!({"kind":"luminance"});
    if let Some(gray) = gray {
        operation["gray"] = json!(gray);
    }
    json!({"op":"selection","source":"composite","operation":operation})
}

fn plan(doc: *const PrecisionDocument, request: &Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut error) };
    if out.is_null() {
        return Err(ffi_error(error));
    }
    let value = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(out) };
    Ok(serde_json::from_str(&value).unwrap())
}

fn select(doc: &PrecisionDocument, request: Value) -> Result<Vec<u16>, String> {
    let planned = plan(doc, &request)?;
    let mut out = vec![999u16; planned["output_samples"].as_u64().unwrap() as usize];
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
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
            &mut error,
        )
    };
    if !ok {
        return Err(ffi_error(error));
    }
    Ok(out)
}

fn code8(coverage: u16) -> i32 {
    (f64::from(coverage) * 255.0 / 65535.0).round() as i32
}

// ---------------------------------------------------------------- tests --

#[test]
fn eight_bit_luminosity_reads_photoshops_take() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(&dir, &PATCHES);
    for (space, want) in TAKE {
        let got = gray_plane(doc, space, 26).expect("the plane");
        for (i, (&g, &w)) in got.iter().zip(want.iter()).enumerate() {
            assert!(
                g.abs_diff(w) <= 1,
                "{space} patch {i} {:?}: {g} vs Photoshop {w}",
                PATCHES[i]
            );
        }
    }
    unsafe { dr_doc_free(doc) };
}

#[test]
fn sixteen_bit_luminosity_reads_photoshops_take() {
    let pixels: Vec<[u16; 3]> = PATCHES
        .iter()
        .map(|p| p.map(|v| u16::from(v) * 257))
        .collect();
    let doc = u16_doc(&pixels, IccProfile::srgb());
    for (space, want) in TAKE {
        let got = select(&doc, luminance(Some(space))).expect("the selection");
        for (i, (&g, &w)) in got.iter().zip(want.iter()).enumerate() {
            assert!(
                (code8(g) - i32::from(w)).abs() <= 1,
                "{space} patch {i}: {} vs Photoshop {w}",
                code8(g)
            );
        }
    }
}

/// Every neutral, then a 6³ grid of colours.
fn oracle_pixels() -> Vec<[u8; 3]> {
    let mut out: Vec<[u8; 3]> = (0..=255u8).map(|v| [v; 3]).collect();
    for r in 0..6u8 {
        for g in 0..6u8 {
            for b in 0..6u8 {
                out.push([r * 51, g * 51, b * 51]);
            }
        }
    }
    out
}

#[test]
fn every_space_is_the_profile_y_through_its_curve() {
    let dir = TempDir::new().unwrap();
    let pixels = oracle_pixels();
    let n = pixels.len() as u32;
    let doc = byte_doc(&dir, &pixels);
    let wide: Vec<[u16; 3]> = pixels
        .iter()
        .map(|p| p.map(|v| u16::from(v) * 257))
        .collect();
    let typed = u16_doc(&wide, IccProfile::srgb());
    for (space, e) in spaces() {
        let eight = gray_plane(doc, space, n).expect("the plane");
        let sixteen = select(&typed, luminance(Some(space))).expect("the selection");
        for (i, p) in pixels.iter().enumerate() {
            let want = grey(p.map(|v| f64::from(v) / 255.0), e);
            let want8 = (want * 255.0).round();
            assert!(
                (f64::from(eight[i]) - want8).abs() <= 1.0,
                "{space} {p:?}: 8-bit {} vs {want8}",
                eight[i]
            );
            let want16 = want * 65535.0;
            assert!(
                (f64::from(sixteen[i]) - want16).abs() <= 257.0,
                "{space} {p:?}: 16-bit {} vs {want16}",
                sixteen[i]
            );
        }
        // White is 1 and black 0 in every space.
        assert_eq!((eight[255], eight[0]), (255, 0), "{space}");
    }
    unsafe { dr_doc_free(doc) };
}

#[test]
fn without_gray_the_luminance_is_unchanged() {
    let pixels: Vec<[u16; 3]> = PATCHES
        .iter()
        .map(|p| p.map(|v| u16::from(v) * 257))
        .collect();
    let doc = u16_doc(&pixels, IccProfile::srgb());
    let rec709 = select(&doc, luminance(None)).unwrap();
    let gray = select(&doc, luminance(Some("dot_gain_20"))).unwrap();
    assert_ne!(
        rec709, gray,
        "the Gray working space changes the luminosity"
    );
    // Rec. 709 over the encoded codes, as before: pure red's patch is 0.2126.
    let red = u16_doc(&[[65535, 0, 0]], IccProfile::srgb());
    let got = select(&red, luminance(None)).unwrap();
    assert!(
        (f64::from(got[0]) - 0.2126 * 65535.0).abs() <= 2.0,
        "{}",
        got[0]
    );
}

#[test]
fn the_refusals_name_themselves() {
    let floating = float_doc(2, 1, &IccProfile::srgb(), &[vec![[0.5, 0.25, 0.1, 1.0]; 2]]);
    assert_eq!(
        plan(&floating, &luminance(Some("dot_gain_20"))).unwrap_err(),
        "a 32-bit float document's luminance takes no Gray working space"
    );
    assert!(
        plan(&floating, &luminance(None)).is_ok(),
        "float luminance still plans"
    );
    let cmyk = cmyk_layers(
        2,
        1,
        vec![RgbLayer {
            name: "L",
            w: 2,
            h: 1,
            offset: (0, 0),
            pixels: vec![[40000, 20000, 10000, 65535]; 2],
        }],
    );
    assert_eq!(
        plan(cmyk, &luminance(Some("gray_gamma_22"))).unwrap_err(),
        "a CMYK document's luminosity takes no Gray working space"
    );
    assert!(
        plan(cmyk, &luminance(None)).is_ok(),
        "CMYK luminance still plans"
    );
    free(cmyk);
    let doc = u16_doc(&[[1, 2, 3]], IccProfile::srgb());
    let unknown = plan(&doc, &luminance(Some("dot_gain_99"))).unwrap_err();
    assert!(
        unknown.starts_with("invalid native source selection:")
            && unknown.contains("unknown variant"),
        "{unknown}"
    );
    let dir = TempDir::new().unwrap();
    let byte = byte_doc(&dir, &[[1, 2, 3]]);
    assert!(
        gray_plane(byte, "dot_gain_99", 1).is_none(),
        "an unknown name is false"
    );
    assert!(
        gray_plane(byte, "dot_gain_20", 2).is_none(),
        "a dimension mismatch is false"
    );
    unsafe { dr_doc_free(byte) };
}

/// A LUT-only RGB profile (`source_conversion_lut.rs`'s identity A2B/B2A).
fn lut_profile() -> Vec<u8> {
    build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic LUT RGB")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
            (b"B2A0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
        ],
    )
}

/// An RGB profile with neither a matrix/TRC model nor a LUT: no Y to read.
fn modelless_profile() -> Vec<u8> {
    build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic model-less RGB")),
            (b"wtpt", tag_wtpt(D50)),
        ],
    )
}

#[test]
fn a_profile_with_neither_model_nor_lut_refuses_the_gray_space() {
    let bytes = modelless_profile();
    let profile = Arc::new(IccProfile::parse(&bytes).unwrap());
    assert!(profile.model().is_none() && profile.lut().is_none());
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(&dir, &[[10, 20, 30]]);
    let mut err = ptr::null_mut();
    let assigned = unsafe { dr_doc_assign_profile(doc, bytes.as_ptr(), bytes.len(), &mut err) };
    assert!(!assigned.is_null(), "{}", ffi_error(err));
    unsafe { dr_doc_free(doc) };
    assert!(
        gray_plane(assigned, "dot_gain_20", 1).is_none(),
        "no Y to read is false"
    );
    unsafe { dr_doc_free(assigned) };
    let typed = u16_doc(&[[1000, 2000, 3000]], profile);
    assert_eq!(
        plan(&typed, &luminance(Some("dot_gain_20"))).unwrap_err(),
        "selection source requires a matrix/TRC RGB profile"
    );
}

#[test]
fn a_lut_profile_document_loads_its_luminosity_through_the_lut() {
    let lut = lut_profile();
    let neutrals: Vec<[u8; 3]> = (0..=8u8).map(|i| [i * 31; 3]).collect();
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(&dir, &neutrals);
    let mut err = ptr::null_mut();
    let assigned = unsafe { dr_doc_assign_profile(doc, lut.as_ptr(), lut.len(), &mut err) };
    assert!(!assigned.is_null(), "{}", ffi_error(err));
    unsafe { dr_doc_free(doc) };
    let eight = gray_plane(assigned, "dot_gain_20", neutrals.len() as u32).expect("true");
    assert!(
        eight.windows(2).all(|w| w[0] <= w[1]),
        "8-bit neutrals: {eight:?}"
    );
    unsafe { dr_doc_free(assigned) };
    let profile = Arc::new(IccProfile::parse(&lut).unwrap());
    assert!(profile.model().is_none() && profile.lut().is_some());
    let wide: Vec<[u16; 3]> = neutrals
        .iter()
        .map(|p| p.map(|v| u16::from(v) * 257))
        .collect();
    let typed = u16_doc(&wide, profile);
    let sixteen = select(&typed, luminance(Some("dot_gain_20"))).expect("it loads");
    assert!(
        sixteen.windows(2).all(|w| w[0] <= w[1]),
        "16-bit neutrals: {sixteen:?}"
    );
}
