//! AUTHORED colours on an 8/16-bit document tagged with an RGB LUT
//! profile convert into it once, as measured in Photoshop 26.10.0 (the
//! `lut_colours` take, verdict B-APP): a layer style's Color and Gradient
//! Overlay, an artwork render, `srgb` paint and a PSD `lfx2` record all land
//! on the profile's own numbers for the authored sRGB colour, and the
//! host-facing `dr_authored_color` agrees with the overlay byte for byte. A
//! LUT profile that cannot convert both ways (a decode-only one) keeps the
//! numbers everywhere.
//!
//! The encodable LUT is built here byte by byte (`common/icc_lut_builders`):
//! an RGB `mntr` profile with an XYZ PCS, a D50 `wtpt`, a grid-2 `mft2`
//! `B2A0` whose corner map is `d = 16/255 + x·219/255` per channel (`x` the
//! table's encoded XYZ input), and its inverse `A2B0` (a 256-entry input
//! curve `x = (d − 16/255)·255/219`, exact on every encoded value). The
//! oracle is this file's own arithmetic: the IEC 61966-2.1 decode, the
//! published Bradford-D50 sRGB → XYZ matrix, the `mft2` XYZ input encoding
//! (`XYZ · 32768 / 65535`) and the closed-form corner map, rounded once. It
//! calls nothing in `icc_lut` or `icc_intent`.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_artwork::dr_artwork_render;
use doctored_core::ffi_authored_color::dr_authored_color;
use doctored_core::ffi_precision::dr_precision_image_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_doc_save_psd, dr_psd_import_report};
use doctored_core::icc::IccProfile;
use doctored_core::icc_lut::D50;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as PLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, styled, take_err_string};
use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_mft2_with_input, tag_wtpt};
use crate::psd_descriptor_fixture::rgbc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const COLOURS: [[u8; 3]; 3] = [[0x00, 0xff, 0x00], [0x2e, 0x80, 0xc8], [0xbc, 0x40, 0x10]];
const BASE: [u8; 4] = [90, 90, 90, 255];
const W: u32 = 4;
const H: u32 = 4;

// ------------------------------------------------------------- profiles --

fn corner(c: &[f64]) -> Vec<f64> {
    c.iter().map(|x| 16.0 / 255.0 + x * 219.0 / 255.0).collect()
}

/// `x = (d − 16/255)·255/219`, clamped, at d = k/255 — exact between the
/// nodes because the map is affine there and its kinks sit on nodes 16, 235.
fn inverse_curve() -> Vec<u16> {
    (0..256)
        .map(|k| ((((k as f64) - 16.0) / 219.0).clamp(0.0, 1.0) * 65535.0).round() as u16)
        .collect()
}

fn lut(with_b2a: bool) -> Vec<u8> {
    let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", tag_desc("Synthetic 16-235 LUT")),
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

fn decode_only() -> Vec<u8> {
    lut(false)
}

/// An RGB profile whose only table is truncated: unreadable, numbers mode.
fn unreadable() -> Vec<u8> {
    build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Unreadable LUT")),
            (
                b"A2B0",
                vec![b'm', b'f', b't', b'2', 0, 0, 0, 0, 3, 3, 0, 0],
            ),
        ],
    )
}

fn parsed(bytes: &[u8]) -> Arc<IccProfile> {
    Arc::new(IccProfile::parse(bytes).expect("an RGB profile"))
}

// --------------------------------------------------------------- oracle --

fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The published Bradford-adapted (D50) sRGB → XYZ matrix.
const SRGB_D50: [[f64; 3]; 3] = [
    [0.436_074_7, 0.385_064_9, 0.143_080_4],
    [0.222_504_5, 0.716_878_6, 0.060_616_9],
    [0.013_932_2, 0.097_104_5, 0.714_173_3],
];

/// The authored sRGB byte triple as the LUT document's continuous numbers.
fn oracle(rgb: [u8; 3]) -> [f64; 3] {
    let lin = rgb.map(|v| decode(f64::from(v) / 255.0));
    std::array::from_fn(|r| {
        let xyz: f64 = (0..3).map(|c| SRGB_D50[r][c] * lin[c]).sum();
        let x = (xyz * 32768.0 / 65535.0).clamp(0.0, 1.0);
        16.0 / 255.0 + x * 219.0 / 255.0
    })
}

fn oracle8(rgb: [u8; 3]) -> [u8; 3] {
    oracle(rgb).map(|v| (v * 255.0).round() as u8)
}

fn encode(v: f64) -> f64 {
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
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

/// The document's 8-bit numbers back to the sRGB byte triple they mean: the
/// corner map inverted, the `mft2` XYZ decoding, the matrix inverted, the
/// IEC 61966-2.1 encode, rounded once. An 8-bit document number is one code
/// of a 219-code range, so a channel near black does not come back as the
/// authored byte; this is what it does come back as.
fn decoded8(device: [u8; 3]) -> [u8; 3] {
    let xyz = device.map(|d| (f64::from(d) - 16.0) / 219.0 * 65535.0 / 32768.0);
    let m = inverse(SRGB_D50);
    std::array::from_fn(|r| {
        let lin: f64 = (0..3).map(|c| m[r][c] * xyz[c]).sum();
        (encode(lin.clamp(0.0, 1.0)) * 255.0).round() as u8
    })
}

fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

fn near8(got: [u8; 3], want: [u8; 3], tolerance: u8, what: &str) {
    for c in 0..3 {
        assert!(
            got[c].abs_diff(want[c]) <= tolerance,
            "{what}: {got:?} against {want:?}"
        );
    }
}

fn authored(profile: &[u8], to_document: bool, rgb: [f32; 3]) -> Option<[f32; 3]> {
    let mut out = [f32::NAN; 3];
    let ok = unsafe {
        dr_authored_color(
            profile.as_ptr(),
            profile.len(),
            to_document,
            rgb.as_ptr(),
            out.as_mut_ptr(),
        )
    };
    ok.then_some(out)
}

fn unit(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(|v| f32::from(v) / 255.0)
}

// ------------------------------------------------------------ documents --

fn overlay(colour: [u8; 3]) -> String {
    json!({"effects":[{"type":"color_overlay","blend":"normal","color":hex(colour),
                       "opacity":1}]})
    .to_string()
}

fn gradient(colour: [u8; 3]) -> String {
    json!({"effects":[{"type":"gradient_overlay","blend":"normal","opacity":1,
        "gradient":{"stops":[{"position":0.0,"color":hex(colour),"opacity":1.0},
                             {"position":1.0,"color":hex(colour),"opacity":1.0}],
                    "style":"linear","angle":0.0,"scale":1.0,"reverse":false,
                    "align_with_layer":true}}]})
    .to_string()
}

fn byte_doc(profile: &Arc<IccProfile>) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba(BASE)));
    doc.profile = Arc::clone(profile);
    doc
}

fn centre(doc: &DrDocument) -> [u8; 3] {
    let flat = doc.flattened().expect("a projection");
    let p = flat.get_pixel(2, 2).0;
    [p[0], p[1], p[2]]
}

fn typed_styled(profile: &Arc<IccProfile>, style: &str) -> [u16; 4] {
    let image = PrecisionImage::from_rgba16(
        W,
        H,
        vec![[23130, 23130, 23130, 65535]; (W * H) as usize],
        Arc::clone(profile),
        None,
    )
    .expect("a 16-bit raster");
    let style = LayerStyle::from_json(style).expect("the style");
    let layer = PLayer::raster("Styled", image, (0, 0), 1.0, true)
        .unwrap()
        .with_layer_style(Some(&style), (W, H))
        .expect("the typed style");
    let document = PrecisionDocument::from_parts(
        W,
        H,
        Arc::clone(profile),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .expect("a typed document");
    let projection = document.render(LIMIT, 0, None).expect("projection");
    projection.rgba16_at(2, 2).expect("a pixel")
}

fn save_psd(doc: &DrDocument, path: &Path) {
    let mut error: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_psd(doc, ptr::null(), cpath(path).as_ptr(), 0, 0, &mut error) },
        "save: {}",
        take_err_string(error)
    );
}

fn reopen_psd(path: &Path) -> Box<DrDocument> {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    let report: Value = serde_json::from_str(&take_err_string(raw)).expect("report JSON");
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), 0, token.as_ptr(), &mut error) };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    unsafe { Box::from_raw(raw) }
}

fn block_body(bytes: &[u8], key: &[u8; 4]) -> Option<Vec<u8>> {
    let mut marker = b"8BIM".to_vec();
    marker.extend(key);
    let at = bytes
        .windows(8)
        .position(|window| window == marker.as_slice())?;
    let len = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
    Some(bytes[at + 12..at + 12 + len].to_vec())
}

fn reopened_overlay(doc: &DrDocument) -> [u8; 3] {
    let style: Value = serde_json::from_str(
        &doc.layers[0]
            .style
            .as_deref()
            .expect("the reopened style")
            .to_json(),
    )
    .unwrap();
    let text = style["effects"][0]["color"].as_str().expect("a colour");
    let v = |i: usize| u8::from_str_radix(&text[1 + 2 * i..3 + 2 * i], 16).unwrap();
    [v(0), v(1), v(2)]
}

fn artwork(profile: &[u8], colour: [u8; 3]) -> [u16; 4] {
    let n = (W * H) as usize;
    let c = unit(colour);
    let description =
        CString::new(json!([{"kind":"coverage","color":[c[0], c[1], c[2], 1.0]}]).to_string())
            .unwrap();
    let samples = vec![65535u16; n];
    let mut error: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_artwork_render(
            W,
            H,
            description.as_ptr(),
            samples.as_ptr(),
            n,
            ptr::null(),
            0,
            profile.as_ptr(),
            profile.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!image.is_null(), "artwork: {}", take_err_string(error));
    let pixel = unsafe { &*image }.rgba16_at(2, 2).expect("a pixel");
    unsafe { dr_precision_image_free(image) };
    pixel
}

fn painted(profile: &Arc<IccProfile>, colour: [u8; 3]) -> [u16; 4] {
    let image = PrecisionImage::from_rgba16(
        2,
        1,
        vec![[1000, 2000, 3000, 65535]; 2],
        Arc::clone(profile),
        None,
    )
    .unwrap();
    let doc =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    let c = unit(colour);
    let request = CString::new(
        json!({"op":"paint","layer":0,"color":[c[0], c[1], c[2], 1.0],"color_space":"srgb",
               "opacity":1.0})
        .to_string(),
    )
    .unwrap();
    let coverage = [65535u16, 65535];
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            &doc,
            request.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null(), "paint: {}", take_err_string(error));
    let out = unsafe { Box::from_raw(out) };
    out.layer_source(0).unwrap().rgba16_at(0, 0).unwrap()
}

fn within_half_code(got: [u16; 4], want: [f64; 3], what: &str) {
    for c in 0..3 {
        let expected = want[c] * 65535.0;
        assert!(
            (f64::from(got[c]) - expected).abs() <= 0.5 * 257.0,
            "{what}: {got:?} against {:?}",
            want.map(|v| v * 65535.0)
        );
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn the_oracle_is_the_16_235_corner_map() {
    // Pure green's XYZ Y is 0.7169: the device number is 16 + 0.3585·219.
    let green = oracle8([0, 255, 0]);
    assert_eq!(
        green[1],
        (16.0f64 + 0.716_878_6 * 32768.0 / 65535.0 * 219.0).round() as u8
    );
    assert!(green.iter().all(|v| (16..=235).contains(v)));
}

#[test]
fn style_overlays_composite_to_the_lut_oracle_at_8_bits() {
    let profile = parsed(&encodable());
    let doc = byte_doc(&profile);
    for colour in COLOURS {
        let want = oracle8(colour);
        let solid = centre(&styled(&doc, 0, &overlay(colour)));
        near8(solid, want, 1, &format!("colour overlay {}", hex(colour)));
        let ramp = centre(&styled(&doc, 0, &gradient(colour)));
        near8(ramp, want, 1, &format!("gradient overlay {}", hex(colour)));
        // The host's question answers the overlay's numbers exactly.
        let host = authored(&encodable(), true, unit(colour)).expect("an encodable LUT");
        assert_eq!(
            host.map(|v| (v * 255.0).round() as u8),
            solid,
            "dr_authored_color agrees with the overlay for {}",
            hex(colour)
        );
    }
}

#[test]
fn style_overlays_composite_to_the_lut_oracle_at_16_bits() {
    let profile = parsed(&encodable());
    for colour in COLOURS {
        let want = oracle8(colour).map(|v| f64::from(v) * 257.0);
        for (what, style) in [("colour", overlay(colour)), ("gradient", gradient(colour))] {
            let got = typed_styled(&profile, &style);
            for c in 0..3 {
                assert!(
                    (f64::from(got[c]) - want[c]).abs() <= 257.0,
                    "{what} overlay {} at 16 bits: {got:?} against {want:?}",
                    hex(colour)
                );
            }
        }
    }
}

#[test]
fn convert_into_the_lut_keeps_a_styles_appearance_and_its_json() {
    let profile = parsed(&encodable());
    for colour in COLOURS {
        let srgb = byte_doc(&IccProfile::srgb());
        let doc = styled(&srgb, 0, &overlay(colour));
        assert_eq!(
            centre(&doc),
            colour,
            "on sRGB the overlay is the authored hex"
        );
        let before = doc.layers[0].style.as_deref().unwrap().to_json();
        let converted = doc
            .convert_to_profile(&profile)
            .expect("convert")
            .expect("a change");
        assert_eq!(
            converted.layers[0].style.as_deref().unwrap().to_json(),
            before,
            "the style JSON is untouched"
        );
        near8(
            centre(&converted),
            oracle8(colour),
            1,
            &format!("the overlay keeps its appearance: {}", hex(colour)),
        );
    }
}

#[test]
fn artwork_renders_the_lut_oracle_and_an_unreadable_lut_renders_the_numbers() {
    for colour in COLOURS {
        within_half_code(
            artwork(&encodable(), colour),
            oracle(colour),
            &format!("artwork {}", hex(colour)),
        );
        let raw = artwork(&unreadable(), colour);
        for c in 0..3 {
            assert!(
                (i32::from(raw[c]) - i32::from(colour[c]) * 257).abs() <= 1,
                "an unreadable LUT renders the numbers: {raw:?} for {}",
                hex(colour)
            );
        }
    }
}

#[test]
fn srgb_paint_at_16_bits_is_the_lut_oracle() {
    let profile = parsed(&encodable());
    for colour in COLOURS {
        within_half_code(
            painted(&profile, colour),
            oracle(colour),
            &format!("paint {}", hex(colour)),
        );
    }
}

#[test]
fn psd_export_writes_the_lut_numbers_and_reimport_decodes_them_back_to_srgb() {
    let dir = TempDir::new().unwrap();
    let profile = parsed(&encodable());
    for (i, colour) in COLOURS.into_iter().enumerate() {
        let doc = styled(&byte_doc(&profile), 0, &overlay(colour));
        let path = dir.path().join(format!("lut-{i}.psd"));
        save_psd(&doc, &path);
        let bytes = std::fs::read(&path).unwrap();
        let body = block_body(&bytes, b"lfx2").expect("an lfx2 block");
        // The written triple is the document's own numbers for the colour:
        // the composite's, which is the oracle within its one code.
        let want = centre(&doc);
        near8(
            want,
            oracle8(colour),
            1,
            &format!("the overlay of {}", hex(colour)),
        );
        let needle = rgbc(
            b"Clr ",
            f64::from(want[0]),
            f64::from(want[1]),
            f64::from(want[2]),
        );
        assert!(
            body.windows(needle.len()).any(|w| w == needle),
            "SoFi RGBC is the document numbers {want:?} for {}",
            hex(colour)
        );
        let back = reopen_psd(&path);
        near8(
            reopened_overlay(&back),
            decoded8(want),
            1,
            &format!("re-import of {} written as {want:?}", hex(colour)),
        );
    }
}

#[test]
fn a_decode_only_lut_keeps_the_numbers_in_both_directions() {
    let bytes = decode_only();
    let profile = parsed(&bytes);
    assert!(authored(&bytes, true, [0.0, 1.0, 0.0]).is_none());
    assert!(authored(&bytes, false, [0.0, 1.0, 0.0]).is_none());
    let dir = TempDir::new().unwrap();
    for (i, colour) in COLOURS.into_iter().enumerate() {
        let doc = styled(&byte_doc(&profile), 0, &overlay(colour));
        assert_eq!(
            centre(&doc),
            colour,
            "the overlay composites to the numbers"
        );
        let path = dir.path().join(format!("decode-only-{i}.psd"));
        save_psd(&doc, &path);
        let bytes = std::fs::read(&path).unwrap();
        let body = block_body(&bytes, b"lfx2").expect("an lfx2 block");
        let needle = rgbc(
            b"Clr ",
            f64::from(colour[0]),
            f64::from(colour[1]),
            f64::from(colour[2]),
        );
        assert!(
            body.windows(needle.len()).any(|w| w == needle),
            "the numbers are written as authored for {}",
            hex(colour)
        );
        assert_eq!(
            reopened_overlay(&reopen_psd(&path)),
            colour,
            "and read back unchanged"
        );
    }
}
