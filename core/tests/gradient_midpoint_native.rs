//! `.rz` word 104 (`gradient_midpoint_native`): a gradient whose stop
//! midpoints are not all 50 %, in a Gradient Fill description or a stroke or
//! gradient overlay style.
//!
//! # The stored pre-104 files
//!
//! `tests/fixtures/rz_gradient_103/byte.rz` and `typed.rz` are FORMAT
//! ORACLES, not golden images: a background, a layer with a three-stop
//! Gradient Overlay and a Gradient Fill at Smoothness 50 % aligned with the
//! layer (so they carry words 105 and 103), written by the writers that
//! predate word 104, never by this build:
//!
//! - commit: dc950add (the last writers without word 104), in this lane's
//!   worktree before any `core/src` edit;
//! - builder: an ignored `write_pre104` test in this file (since deleted)
//!   saving the byte document with `save_native` and the typed one with
//!   `save_native(path, LIMIT, 0, None)` into `$DR_OUT_DIR`;
//! - command: `DR_OUT_DIR=<worktree>/core/tests/fixtures/rz_gradient_103
//!   make t F=gradient_midpoint_native T=write_pre104 A=--ignored`;
//! - bytes: 1383, sha256
//!   `07eff5055491592fba8ff683c84666792881261d613a484e6618a24b55323a01`
//!   (byte); 5459, sha256
//!   `6517e11f8b44fa9624d3e13162a2ce978cc81b1304afc02614b323d670ccac6e`
//!   (typed).
//!
//! Each opens, renders as a fresh document built the writer's way renders
//! (only the opened document is re-saved: a fresh one is built in another
//! process than the writer's), and re-saves byte for byte: the current writers
//! must reproduce them. The planes inside
//! them are encoded by the PNG crate, so a dependency bump that moves the PNG
//! encoder's bytes fails those tests ON PURPOSE.
//!
//! # The word
//!
//! A byte file writes 104 inside 45 in the order 105, 104, 103, then the
//! words it always wrote; a typed file right after 46, a CMYK file after its
//! rung, in the same order. It round-trips its description, its style and
//! its pixels. A file without a midpoint other than 50 % keeps its prior
//! bytes (no word). The claim is checked both ways on crafted files: the
//! word deleted from a file that needs it is refused by its number, the word
//! inserted into a file that does not is refused as invalid.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{
    dr_artwork_render, dr_doc_artwork, dr_precision_doc_artwork,
    dr_precision_doc_artwork_description, dr_precision_doc_artwork_metadata,
};
use doctored_core::ffi_doc::dr_doc_layer_meta;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_structure_fixture::free;
use crate::cmyk_style_fixture::backdrop;
use crate::common::{layer_pixels, solid, styled};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 12;
const H: u32 = 8;
const MID_REFUSAL: &str = "native gradient midpoints require DRDC 104";
const MID_INVALID: &str = "invalid native gradient midpoint word";
const ALIGN_REFUSAL: &str = "native gradient fill alignment requires DRDC 105";
const SMOOTH_REFUSAL: &str = "native gradient smoothness requires DRDC 103";
const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/rz_gradient_103"
);

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> String {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_str().unwrap().to_string()
}

fn refusal(words: &[u32], k: usize) -> &'static str {
    match words[k] {
        105 => ALIGN_REFUSAL,
        104 => MID_REFUSAL,
        _ => SMOOTH_REFUSAL,
    }
}

/// A gradient fill's description and its render: `smoothness` a fraction,
/// `align` its Align with Layer, `midpoint` the first stop's.
fn gradient_fill(smoothness: f64, align: bool, midpoint: f64) -> (String, PrecisionImage) {
    let mut first = json!({"position": 0, "color": "#203040", "opacity": 1});
    if midpoint != 0.5 {
        first["midpoint"] = json!(midpoint);
    }
    let mut gradient = json!({"stops": [first,
        {"position": 1, "color": "#e0c0a0", "opacity": 1}],
        "style": "linear", "angle": 0, "scale": 1, "reverse": false,
        "align_with_layer": align});
    if smoothness != 1.0 {
        gradient["smoothness"] = json!(smoothness);
    }
    let description = json!({"type": "fill", "version": 1, "fill": "gradient",
        "gradient": gradient, "canvas": [W, H]})
    .to_string();
    let components = CString::new(
        json!([{"kind": "gradient", "gradient": gradient,
            "colors": [[32.0 / 255.0, 48.0 / 255.0, 64.0 / 255.0],
                [224.0 / 255.0, 192.0 / 255.0, 160.0 / 255.0]]}])
        .to_string(),
    )
    .unwrap();
    let profile = IccProfile::srgb();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_artwork_render(
            W,
            H,
            components.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    (description, *unsafe { Box::from_raw(out) })
}

/// The cases every depth writes: (name, smoothness, align, midpoint, words).
fn cases() -> Vec<(&'static str, f64, bool, f64, Vec<u32>)> {
    vec![
        ("mid", 1.0, false, 0.3, vec![104]),
        ("all", 0.25, true, 0.7, vec![105, 104, 103]),
    ]
}

// ------------------------------------------------------------------ byte --

fn byte_publish(
    doc: &DrDocument,
    index: usize,
    description: &str,
    image: &PrecisionImage,
) -> DrDocument {
    let name = CString::new("Gradient Fill").unwrap();
    let text = CString::new(description).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_artwork(
            doc,
            index,
            false,
            image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}

fn byte_fill(smoothness: f64, align: bool, midpoint: f64) -> (DrDocument, String) {
    let doc = DrDocument::from_pixels(solid(W, H, [255; 4]));
    let (description, image) = gradient_fill(smoothness, align, midpoint);
    (byte_publish(&doc, 0, &description, &image), description)
}

fn byte_save(doc: &DrDocument, path: &Path) -> Vec<u8> {
    doc.save_native(path.to_str().unwrap()).unwrap();
    std::fs::read(path).unwrap()
}

fn byte_refusal(dir: &TempDir, name: &str, bytes: &[u8]) -> String {
    match DrDocument::open(&write(dir, name, bytes)) {
        Ok(_) => panic!("{name}: must refuse"),
        Err(message) => message,
    }
}

#[test]
fn the_byte_word_104_sits_between_105_and_103_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let (plain, _) = byte_fill(1.0, false, 0.5);
    let plain = byte_save(&plain, &dir.path().join("plain.rz"));
    assert_eq!(word(&plain, 4), 45);
    assert!(
        ![103, 104, 105].contains(&word(&plain, 8)),
        "no word by default"
    );
    for (name, smoothness, align, midpoint, words) in cases() {
        let (doc, description) = byte_fill(smoothness, align, midpoint);
        let bytes = byte_save(&doc, &dir.path().join(format!("{name}.rz")));
        assert_eq!(word(&bytes, 4), 45);
        for (k, want) in words.iter().enumerate() {
            assert_eq!(word(&bytes, 8 + 4 * k), *want, "{name}: word {k}");
        }
        assert_eq!(word(&bytes, 8 + 4 * words.len()), word(&plain, 8), "{name}");
        let back = DrDocument::open(&write(&dir, &format!("{name}-again.rz"), &bytes)).unwrap();
        assert_eq!(
            owned(unsafe { dr_doc_layer_meta(&back, 1) }).as_deref(),
            Some(description.as_str()),
            "{name}"
        );
        assert_eq!(layer_pixels(&back, 1), layer_pixels(&doc, 1), "{name}");
        for k in 0..words.len() {
            let mut cut = bytes.clone();
            cut.drain(8 + 4 * k..12 + 4 * k);
            let message = byte_refusal(&dir, &format!("{name}-cut{k}.rz"), &cut);
            assert!(message.ends_with(refusal(&words, k)), "{name}: {message}");
        }
    }
    let mut forged = plain.clone();
    forged.splice(8..8, 104u32.to_le_bytes());
    assert!(byte_refusal(&dir, "forged.rz", &forged).ends_with(MID_INVALID));
    // 103 ahead of 104 is not the order.
    let (all, _) = byte_fill(0.25, false, 0.7);
    let mut swapped = byte_save(&all, &dir.path().join("swap.rz"));
    assert_eq!((word(&swapped, 8), word(&swapped, 12)), (104, 103));
    swapped[8..16].copy_from_slice(&[103u32.to_le_bytes(), 104u32.to_le_bytes()].concat());
    assert!(DrDocument::open(&write(&dir, "swapped.rz", &swapped)).is_err());
}

#[test]
fn a_style_midpoint_claims_word_104_in_a_byte_file() {
    let dir = TempDir::new().unwrap();
    let doc = DrDocument::from_pixels(solid(W, H, [255; 4]))
        .adding_image_layer(0, solid(6, 4, [200, 20, 20, 255]), "L")
        .unwrap();
    for (effect, key) in [
        ("gradient_overlay", "midpoint"),
        ("stroke", "opacity_midpoint"),
    ] {
        let style = json!({"effects": [{"type": effect, "gradient": {"stops": [
            {"position": 0, "color": "#000000", key: 0.2},
            {"position": 1, "color": "#ffffff"}]}}]});
        let styled_doc = styled(&doc, 1, &style.to_string());
        let bytes = byte_save(&styled_doc, &dir.path().join(format!("{effect}.rz")));
        assert_eq!(word(&bytes, 8), 104, "{effect}");
        let back = DrDocument::open(&write(&dir, &format!("{effect}-again.rz"), &bytes)).unwrap();
        assert_eq!(
            back.layers[1].style.as_deref().map(|s| s.to_json()),
            styled_doc.layers[1].style.as_deref().map(|s| s.to_json())
        );
        let mut cut = bytes.clone();
        cut.drain(8..12);
        assert!(byte_refusal(&dir, &format!("{effect}-cut.rz"), &cut).ends_with(MID_REFUSAL));
    }
}

// ----------------------------------------------------------------- typed --

fn typed_doc() -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            W,
            H,
            vec![[1000, 2000, 3000, 65535]; (W * H) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Default::default(),
        Default::default(),
    )
    .unwrap()
}

fn typed_publish(
    doc: &PrecisionDocument,
    index: usize,
    description: &str,
    image: &PrecisionImage,
) -> PrecisionDocument {
    let name = CString::new("Gradient Fill").unwrap();
    let text = CString::new(description).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork(
            doc,
            index,
            false,
            image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}

fn typed_save(doc: &PrecisionDocument, path: &Path) -> Vec<u8> {
    doc.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    std::fs::read(path).unwrap()
}

fn typed_open(dir: &TempDir, name: &str, bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::open_native(&write(dir, name, bytes), LIMIT, 0, None)
}

#[test]
fn the_typed_word_104_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let (plain_description, plain_image) = gradient_fill(1.0, false, 0.5);
    let plain = typed_publish(&typed_doc(), 0, &plain_description, &plain_image);
    let plain = typed_save(&plain, &dir.path().join("plain.rz"));
    assert_eq!(word(&plain, 4), 46);
    assert!(
        ![103, 104, 105].contains(&word(&plain, 8)),
        "no word by default"
    );
    for (name, smoothness, align, midpoint, words) in cases() {
        let (description, image) = gradient_fill(smoothness, align, midpoint);
        let doc = typed_publish(&typed_doc(), 0, &description, &image);
        let bytes = typed_save(&doc, &dir.path().join(format!("{name}.rz")));
        assert_eq!(word(&bytes, 4), 46);
        for (k, want) in words.iter().enumerate() {
            assert_eq!(word(&bytes, 8 + 4 * k), *want, "{name}: word {k}");
        }
        assert_eq!(word(&bytes, 8 + 4 * words.len()), word(&plain, 8), "{name}");
        let back = typed_open(&dir, &format!("{name}-again.rz"), &bytes).unwrap();
        assert_eq!(
            owned(unsafe { dr_precision_doc_artwork_description(&back, 1) }),
            Some(description),
            "{name}"
        );
        assert_eq!(
            back.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
            image.to_rgba16().ok().unwrap(),
            "{name}"
        );
        for k in 0..words.len() {
            let mut cut = bytes.clone();
            cut.drain(8 + 4 * k..12 + 4 * k);
            let message = typed_open(&dir, &format!("{name}-cut{k}.rz"), &cut)
                .err()
                .expect("a refusal");
            assert!(message.ends_with(refusal(&words, k)), "{name}: {message}");
        }
    }
    let mut forged = plain.clone();
    forged.splice(8..8, 104u32.to_le_bytes());
    let message = typed_open(&dir, "forged.rz", &forged)
        .err()
        .expect("a refusal");
    assert!(message.ends_with(MID_INVALID), "{message}");
}

#[test]
fn a_cmyk_file_claims_word_104_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let base = backdrop(W, H, 16, &[[0.0; 4]]);
    let (description, _) = gradient_fill(0.5, true, 0.35);
    let text = CString::new(description.as_str()).unwrap();
    let mut err = ptr::null_mut();
    let doc =
        unsafe { dr_precision_doc_artwork_metadata(base, 0, text.as_ptr(), LIMIT, 0, &mut err) };
    assert_eq!(owned(err), None);
    assert!(!doc.is_null());
    let bytes = typed_save(unsafe { &*doc }, &dir.path().join("cmyk.rz"));
    assert_eq!(word(&bytes, 4), 46);
    assert!((82..=88).contains(&word(&bytes, 8)), "the CMYK rung");
    assert_eq!(
        (word(&bytes, 12), word(&bytes, 16), word(&bytes, 20)),
        (105, 104, 103),
        "105, 104, 103 after the rung"
    );
    let back = typed_open(&dir, "cmyk-again.rz", &bytes).unwrap();
    assert!(back.is_cmyk());
    assert_eq!(
        owned(unsafe { dr_precision_doc_artwork_description(&back, 0) }),
        Some(description)
    );
    let mut cut = bytes.clone();
    cut.drain(16..20);
    let message = typed_open(&dir, "cmyk-cut.rz", &cut)
        .err()
        .expect("a refusal");
    assert!(message.ends_with(MID_REFUSAL), "{message}");
    free(doc);
    free(base);
}

// ------------------------------------------------- the pre-104 fixtures --

/// The fixtures' overlay style (module doc): three stops, all at 50 %.
fn overlay_style() -> String {
    json!({"effects": [{"type": "gradient_overlay", "gradient": {"stops": [
        {"position": 0, "color": "#102030", "opacity": 1},
        {"position": 0.4, "color": "#f08040", "opacity": 0.5},
        {"position": 1, "color": "#40c0e0", "opacity": 1}]}}]})
    .to_string()
}

/// The fixtures' fill: Smoothness 50 % and aligned, its midpoints at 50 %.
fn fixture_fill() -> (String, PrecisionImage) {
    gradient_fill(0.5, true, 0.5)
}

#[test]
fn the_pre_104_byte_fixture_opens_renders_and_resaves_byte_for_byte() {
    let stored = std::fs::read(format!("{FIXTURES}/byte.rz")).unwrap();
    assert_eq!(stored.len(), 1383);
    assert_eq!((word(&stored, 8), word(&stored, 12)), (105, 103));
    let opened = DrDocument::open(&format!("{FIXTURES}/byte.rz")).unwrap();
    // A fresh document built as the writer built it.
    let (description, image) = fixture_fill();
    let doc = DrDocument::from_pixels(solid(W, H, [255; 4]))
        .adding_image_layer(0, solid(6, 4, [200, 20, 20, 255]), "Overlaid")
        .unwrap();
    let fresh = byte_publish(&styled(&doc, 1, &overlay_style()), 1, &description, &image);
    assert_eq!(opened.flattened().unwrap(), fresh.flattened().unwrap());
    let dir = TempDir::new().unwrap();
    // The rewrite carries this build's magic (`DRDC`) over the stored `RZDC`.
    assert_eq!(
        byte_save(&opened, &dir.path().join("again.rz")),
        [b"DRDC".as_slice(), &stored[4..]].concat()
    );
}

#[test]
fn the_pre_104_typed_fixture_opens_renders_and_resaves_byte_for_byte() {
    let stored = std::fs::read(format!("{FIXTURES}/typed.rz")).unwrap();
    assert_eq!(stored.len(), 5459);
    assert_eq!((word(&stored, 8), word(&stored, 12)), (105, 103));
    let opened =
        PrecisionDocument::open_native(&format!("{FIXTURES}/typed.rz"), LIMIT, 0, None).unwrap();
    let (description, image) = fixture_fill();
    let style = LayerStyle::from_json(&overlay_style()).unwrap();
    let px = |w: u32, h: u32, v: [u16; 4]| {
        PrecisionImage::from_rgba16(w, h, vec![v; (w * h) as usize], IccProfile::srgb(), None)
            .unwrap()
    };
    let base = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            PrecisionDocumentLayer::raster("Background", px(W, H, [65535; 4]), (0, 0), 1.0, true)
                .unwrap(),
            PrecisionDocumentLayer::raster(
                "Overlaid",
                px(6, 4, [51400, 5140, 5140, 65535]),
                (0, 0),
                1.0,
                true,
            )
            .unwrap()
            .with_layer_style(Some(&style), (W, H))
            .unwrap(),
        ],
    )
    .unwrap();
    let fresh = typed_publish(&base, 1, &description, &image);
    let render = |d: &PrecisionDocument| -> Value {
        json!(d.render(LIMIT, 0, None).unwrap().to_rgba16().ok().unwrap())
    };
    assert_eq!(render(&opened), render(&fresh));
    let dir = TempDir::new().unwrap();
    // The rewrite carries this build's magic (`DRDC`) over the stored `RZDC`.
    assert_eq!(
        typed_save(&opened, &dir.path().join("again.rz")),
        [b"DRDC".as_slice(), &stored[4..]].concat()
    );
}
