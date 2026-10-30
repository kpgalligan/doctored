//! `.rz` words 105 (`fill_align_native`: a Gradient Fill aligned with
//! the layer) and 103 (`gradient_smoothness_native`: a gradient whose
//! Smoothness is below 100 %, in a Gradient Fill description or a stroke or
//! gradient overlay style). Neither has a table. A byte file writes them
//! inside 45 in the order 105, 103, then the words it always wrote; a typed
//! file right after 46, a CMYK file after its rung. Each round-trips its
//! description and pixels. A file with neither keeps its prior bytes (no
//! word). Each claim is checked both ways on crafted files: the word deleted
//! from a file that needs it is refused by its number, the word inserted
//! into a file that does not is refused as invalid.
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
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::json;
use tempfile::TempDir;

use crate::cmyk_structure_fixture::free;
use crate::cmyk_style_fixture::backdrop;
use crate::common::{layer_pixels, solid, styled};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 12;
const H: u32 = 8;
const ALIGN_REFUSAL: &str = "native gradient fill alignment requires DRDC 105";
const ALIGN_INVALID: &str = "invalid native gradient fill alignment word";
const SMOOTH_REFUSAL: &str = "native gradient smoothness requires DRDC 103";
const SMOOTH_INVALID: &str = "invalid native gradient smoothness word";

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

/// A gradient fill's description and its render: `smoothness` a fraction,
/// `align` its Align with Layer.
fn gradient_fill(smoothness: f64, align: bool) -> (String, PrecisionImage) {
    let mut gradient = json!({"stops": [
        {"position": 0, "color": "#203040", "opacity": 1},
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

// ------------------------------------------------------------------ byte --

fn byte_fill(smoothness: f64, align: bool) -> (DrDocument, String) {
    let doc = DrDocument::from_pixels(solid(W, H, [255; 4]));
    let (description, image) = gradient_fill(smoothness, align);
    let name = CString::new("Gradient Fill").unwrap();
    let text = CString::new(description.as_str()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_artwork(
            &doc,
            0,
            false,
            &image,
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
    (*unsafe { Box::from_raw(out) }, description)
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
fn the_byte_words_105_and_103_come_first_round_trip_and_refuse_both_ways() {
    let dir = TempDir::new().unwrap();
    let (plain, _) = byte_fill(1.0, false);
    let plain = byte_save(&plain, &dir.path().join("plain.rz"));
    assert_eq!(word(&plain, 4), 45);
    assert!(![103, 105].contains(&word(&plain, 8)), "no word by default");

    for (name, smoothness, align, words) in [
        ("align", 1.0, true, vec![105]),
        ("smooth", 0.5, false, vec![103]),
        ("both", 0.25, true, vec![105, 103]),
    ] {
        let (doc, description) = byte_fill(smoothness, align);
        let bytes = byte_save(&doc, &dir.path().join(format!("{name}.rz")));
        assert_eq!(word(&bytes, 4), 45);
        for (k, want) in words.iter().enumerate() {
            assert_eq!(word(&bytes, 8 + 4 * k), *want, "{name}: word {k}");
        }
        assert_eq!(
            word(&bytes, 8 + 4 * words.len()),
            word(&plain, 8),
            "{name}: then the plain file's word"
        );
        let back = DrDocument::open(&write(&dir, &format!("{name}-again.rz"), &bytes)).unwrap();
        assert_eq!(
            owned(unsafe { dr_doc_layer_meta(&back, 1) }).as_deref(),
            Some(description.as_str()),
            "{name}"
        );
        assert_eq!(layer_pixels(&back, 1), layer_pixels(&doc, 1), "{name}");
        // Each word deleted: refused by its number.
        for (k, want) in words.iter().enumerate() {
            let mut cut = bytes.clone();
            cut.drain(8 + 4 * k..12 + 4 * k);
            let message = byte_refusal(&dir, &format!("{name}-cut{k}.rz"), &cut);
            let refusal = if *want == 105 {
                ALIGN_REFUSAL
            } else {
                SMOOTH_REFUSAL
            };
            assert!(message.ends_with(refusal), "{name}: {message}");
        }
    }
    // Inserted into a file that holds neither: invalid.
    for (w, invalid) in [(105u32, ALIGN_INVALID), (103, SMOOTH_INVALID)] {
        let mut forged = plain.clone();
        forged.splice(8..8, w.to_le_bytes());
        let message = byte_refusal(&dir, &format!("forged{w}.rz"), &forged);
        assert!(message.ends_with(invalid), "{w}: {message}");
    }
    // 103 ahead of 105 is not the order: 105 is then read as invalid.
    let (both, _) = byte_fill(0.25, true);
    let mut swapped = byte_save(&both, &dir.path().join("swap.rz"));
    swapped[8..16].copy_from_slice(&[103u32.to_le_bytes(), 105u32.to_le_bytes()].concat());
    assert!(DrDocument::open(&write(&dir, "swapped.rz", &swapped)).is_err());
}

#[test]
fn a_style_smoothness_claims_word_103_in_a_byte_file() {
    let dir = TempDir::new().unwrap();
    let doc = DrDocument::from_pixels(solid(W, H, [255; 4]))
        .adding_image_layer(0, solid(6, 4, [200, 20, 20, 255]), "L")
        .unwrap();
    let style = json!({"effects": [{"type": "gradient_overlay",
        "gradient": {"smoothness": 0.5}}]});
    let doc = styled(&doc, 1, &style.to_string());
    let bytes = byte_save(&doc, &dir.path().join("style.rz"));
    assert_eq!(word(&bytes, 8), 103, "a style's Smoothness");
    let back = DrDocument::open(&write(&dir, "style-again.rz", &bytes)).unwrap();
    assert_eq!(
        back.layers[1].style.as_deref().map(|s| s.to_json()),
        doc.layers[1].style.as_deref().map(|s| s.to_json())
    );
    let mut cut = bytes.clone();
    cut.drain(8..12);
    assert!(byte_refusal(&dir, "style-cut.rz", &cut).ends_with(SMOOTH_REFUSAL));
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

fn typed_fill(smoothness: f64, align: bool) -> (PrecisionDocument, String, PrecisionImage) {
    let (description, image) = gradient_fill(smoothness, align);
    let name = CString::new("Gradient Fill").unwrap();
    let text = CString::new(description.as_str()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork(
            &typed_doc(),
            0,
            false,
            &image,
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
    (*unsafe { Box::from_raw(out) }, description, image)
}

fn typed_save(doc: &PrecisionDocument, path: &Path) -> Vec<u8> {
    doc.save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    std::fs::read(path).unwrap()
}

fn typed_open(dir: &TempDir, name: &str, bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::open_native(&write(dir, name, bytes), LIMIT, 0, None)
}

fn description_of(doc: &PrecisionDocument, layer: usize) -> Option<String> {
    owned(unsafe { dr_precision_doc_artwork_description(doc, layer) })
}

#[test]
fn the_typed_words_105_and_103_round_trip_and_refuse_both_ways() {
    let dir = TempDir::new().unwrap();
    let (plain, _, _) = typed_fill(1.0, false);
    let plain = typed_save(&plain, &dir.path().join("plain.rz"));
    assert_eq!(word(&plain, 4), 46);
    assert!(![103, 105].contains(&word(&plain, 8)), "no word by default");
    for (name, smoothness, align, words) in [
        ("align", 1.0, true, vec![105]),
        ("smooth", 0.5, false, vec![103]),
        ("both", 0.25, true, vec![105, 103]),
    ] {
        let (doc, description, image) = typed_fill(smoothness, align);
        let bytes = typed_save(&doc, &dir.path().join(format!("{name}.rz")));
        assert_eq!(word(&bytes, 4), 46);
        for (k, want) in words.iter().enumerate() {
            assert_eq!(word(&bytes, 8 + 4 * k), *want, "{name}: word {k}");
        }
        assert_eq!(word(&bytes, 8 + 4 * words.len()), word(&plain, 8), "{name}");
        let back = typed_open(&dir, &format!("{name}-again.rz"), &bytes).unwrap();
        assert_eq!(description_of(&back, 1), Some(description), "{name}");
        assert_eq!(
            back.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
            image.to_rgba16().ok().unwrap(),
            "{name}"
        );
        for (k, want) in words.iter().enumerate() {
            let mut cut = bytes.clone();
            cut.drain(8 + 4 * k..12 + 4 * k);
            let message = typed_open(&dir, &format!("{name}-cut{k}.rz"), &cut)
                .err()
                .expect("a refusal");
            let refusal = if *want == 105 {
                ALIGN_REFUSAL
            } else {
                SMOOTH_REFUSAL
            };
            assert!(message.ends_with(refusal), "{name}: {message}");
        }
    }
    for (w, invalid) in [(105u32, ALIGN_INVALID), (103, SMOOTH_INVALID)] {
        let mut forged = plain.clone();
        forged.splice(8..8, w.to_le_bytes());
        let message = typed_open(&dir, &format!("forged{w}.rz"), &forged)
            .err()
            .expect("a refusal");
        assert!(message.ends_with(invalid), "{w}: {message}");
    }
}

#[test]
fn a_cmyk_file_claims_words_105_and_103_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let base = backdrop(W, H, 16, &[[0.0; 4]]);
    let (description, _) = gradient_fill(0.5, true);
    let text = CString::new(description.as_str()).unwrap();
    let mut err = ptr::null_mut();
    let doc =
        unsafe { dr_precision_doc_artwork_metadata(base, 0, text.as_ptr(), LIMIT, 0, &mut err) };
    assert_eq!(owned(err), None);
    assert!(!doc.is_null());
    let bytes = typed_save(unsafe { &*doc }, &dir.path().join("cmyk.rz"));
    assert_eq!(word(&bytes, 4), 46);
    assert!((82..=88).contains(&word(&bytes, 8)), "the CMYK rung");
    assert_eq!(word(&bytes, 12), 105, "105 after the rung");
    assert_eq!(word(&bytes, 16), 103, "then 103");
    let back = typed_open(&dir, "cmyk-again.rz", &bytes).unwrap();
    assert!(back.is_cmyk());
    assert_eq!(description_of(&back, 0), Some(description));
    let mut cut = bytes.clone();
    cut.drain(12..16);
    let message = typed_open(&dir, "cmyk-cut.rz", &cut)
        .err()
        .expect("a refusal");
    assert!(message.ends_with(ALIGN_REFUSAL), "{message}");
    free(doc);
    free(base);
}
