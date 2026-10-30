//! The 8-bit native save plan (`drdc_save_plan`). (a) every
//! fixture's saved file fits `encoded_bound`, the admitted figure covers it,
//! and the export answers the method; (b) a NULL document answers 0 with an
//! error; (c) the plan and `referenced − views` do not move when the views are
//! evicted. Only (c) evicts in this binary.

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::doc_comps::CompRequest;
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_guide::GuideOrientation;
use doctored_core::doc_sampler::ColorSampler;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::{dr_doc_native_save_scratch_bytes, dr_doc_save_native};
use doctored_core::ffi_raw_source::dr_doc_attach_raw_source;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use doctored_core::style::LayerStyle;
use doctored_core::warp::WarpEdit;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;
use tempfile::TempDir;

use crate::resource_report;
use crate::serial;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

unsafe fn text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    value
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Four-channel noise: every byte of every pixel from the generator.
fn noise(w: u32, h: u32, seed: u64) -> RgbaImage {
    let mut state = seed;
    let mut out = RgbaImage::new(w, h);
    for p in out.pixels_mut() {
        p.0 = (splitmix(&mut state) as u32).to_le_bytes();
    }
    out
}

fn noise16(w: u32, h: u32, seed: u64) -> PrecisionDocument {
    let mut state = seed;
    let pixels = (0..w * h)
        .map(|_| {
            let v = splitmix(&mut state);
            [v as u16, (v >> 16) as u16, (v >> 32) as u16, 65535]
        })
        .collect();
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::display_p3(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

/// The FFI figure, which must answer exactly the method.
fn ffi_scratch(doc: &DrDocument) -> u64 {
    let mut error = ptr::null_mut();
    let bytes = unsafe { dr_doc_native_save_scratch_bytes(doc, &mut error) };
    assert!(error.is_null(), "{}", unsafe { text(error) });
    bytes
}

/// Saves through the export and answers the file's bytes.
fn saved(doc: &DrDocument, dir: &TempDir, name: &str) -> Vec<u8> {
    let path = dir.path().join(name);
    let c = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let ok = unsafe { dr_doc_save_native(doc, c.as_ptr(), &mut error) };
    assert!(ok, "{name}: {}", unsafe { text(error) });
    std::fs::read(&path).unwrap()
}

/// (a)'s three assertions for one fixture; answers the file's version words
/// (every word from 28 up after the magic, then the base version), so a
/// fixture can show it carries the tables it claims.
fn check(doc: &DrDocument, dir: &TempDir, name: &str) -> Vec<u32> {
    let plan = doc.native_save_plan().unwrap();
    let bytes = saved(doc, dir, name);
    let len = bytes.len() as u64;
    assert!(
        len <= plan.encoded_bound,
        "{name}: file {len} > encoded_bound {}",
        plan.encoded_bound
    );
    assert!(plan.write_scratch_bytes >= plan.encoded_bound, "{name}");
    assert_eq!(
        ffi_scratch(doc),
        doc.native_save_scratch_bytes().unwrap(),
        "{name}"
    );
    let mut words = Vec::new();
    for chunk in bytes[4..].chunks_exact(4) {
        let word = u32::from_le_bytes(chunk.try_into().unwrap());
        words.push(word);
        if word < 28 {
            break;
        }
    }
    words
}

/// A white Background plus a placed noise layer (the `place` op, so the
/// layer holds its retained 8-bit source).
fn placed(w: u32, h: u32) -> DrDocument {
    let background = DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([255; 4])));
    let source = DrDocument::from_pixels(noise(w, h, 0x5EED));
    let edit = SourceEdit::parse(
        &json!({"op":"place","source_id":"n","revision_id":"a","name":"Noise","x":0,"y":0})
            .to_string(),
    )
    .unwrap();
    background
        .source_edit(Some(&source), &edit, LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

fn adjust(op: &str, params: Value) -> String {
    json!({"type": "adjust", "op": op, "params": params}).to_string()
}

/// A warp field over `layer` (the shape of `common/warp_fixture.rs`).
fn warp(doc: &DrDocument, layer: usize) -> DrDocument {
    let values: Vec<[f64; 2]> = (0..3)
        .flat_map(|y| (0..3).map(move |x| [0.3 * f64::from(x) - 0.2, 0.25 * f64::from(y) - 0.1]))
        .collect();
    let edit = WarpEdit::parse(
        &json!({"op":"set","layer":layer,"target":"pixels","columns":3,"rows":3,
            "displacements":values,"filter":"bilinear"})
        .to_string(),
    )
    .unwrap();
    doc.warp_edit(&edit, LIMIT, 0, None).unwrap().unwrap()
}

/// Fixture 3a, a LINEAR-LIGHT document (which refuses styles and retained
/// sources, so those ride in 3b): masks with properties and outside coverage,
/// two channels, levels and curves meta carrying their words, guides, a
/// non-sRGB profile with EXIF/XMP/IPTC, comps, labels, samplers and a warp.
fn rich_linear() -> DrDocument {
    let (w, h) = (96, 64);
    let mut doc = DrDocument::from_pixels(noise(w, h, 3));
    doc.profile = IccProfile::display_p3();
    doc.metadata = Metadata {
        exif: Some(Arc::from(&b"Exif\0\0probe-exif-packet"[..])),
        xmp: Some(Arc::from(&b"<x:xmpmeta>probe</x:xmpmeta>"[..])),
        iptc: Some(Arc::from(&b"\x1c\x02\x05probe-iptc"[..])),
    };
    let mut doc = doc
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .unwrap();
    doc = doc
        .adding_image_layer(0, noise(40, 30, 4), "Masked")
        .unwrap();
    doc = doc.add_mask(1, MaskKind::RevealAll).unwrap();
    doc = doc.set_mask_properties(1, false, 0.6, 2.5).unwrap();
    doc = doc
        .adding_image_layer(1, noise(w, h, 5), "Outside")
        .unwrap();
    doc = doc
        .set_mask(
            2,
            GrayImage::from_pixel(20, 10, Luma([90])),
            (5, 7),
            true,
            255,
        )
        .unwrap();
    doc = doc
        .adding_image_layer(2, RgbaImage::new(w, h), "Levels")
        .unwrap();
    doc.layers[3].meta = Some(adjust(
        "levels",
        json!({"black": 0.1, "white": 0.9, "gamma": 1.4, "output_black": 0.05,
            "g": {"black": 0.15, "white": 1.0, "gamma": 0.7}}),
    ));
    doc = doc
        .adding_image_layer(3, RgbaImage::new(w, h), "Curves")
        .unwrap();
    let map: Vec<u8> = (0..=255).rev().collect();
    doc.layers[4].meta = Some(adjust("curves", json!({ "rgb_map": map })));
    doc.layers[1].label = 3;
    doc.layers[4].label = 7;
    let plane = noise(w, h, 6).into_raw();
    let first: Vec<u8> = plane.iter().step_by(4).copied().collect();
    doc = doc
        .add_channel("Alpha 1", &first, w, h, [255, 0, 0], 0.5)
        .unwrap();
    doc = doc
        .add_channel(
            "Alpha 2",
            &vec![200; (w * h) as usize],
            w,
            h,
            [0, 0, 255],
            0.4,
        )
        .unwrap();
    doc = doc.add_guide(GuideOrientation::Horizontal, 12.5).unwrap();
    doc = doc.add_guide(GuideOrientation::Vertical, 40.0).unwrap();
    doc = doc
        .edit_layer_comps(
            &CompRequest::parse(
                &json!({"op":"layer_comp","action":"save","name":"Shot"}).to_string(),
            )
            .unwrap(),
        )
        .unwrap()
        .unwrap();
    doc = doc
        .with_color_samplers(&[
            ColorSampler {
                x: 3,
                y: 4,
                sample_size: 3,
            },
            ColorSampler {
                x: 50,
                y: 20,
                sample_size: 1,
            },
        ])
        .unwrap();
    warp(&doc, 0)
}

/// Fixture 3b, an ENCODED document: a styled layer, a warp, and a 16-bit
/// original placed into the byte document carrying a camera RAW original.
fn rich_sources() -> DrDocument {
    let (w, h) = (80, 60);
    let mut doc = DrDocument::from_pixels(noise(w, h, 7));
    doc.profile = IccProfile::display_p3();
    doc = warp(&doc, 0);
    doc = doc
        .adding_image_layer(0, noise(30, 20, 8), "Styled")
        .unwrap();
    let style = LayerStyle::from_json(
        r##"{"effects":[{"type":"drop_shadow","use_global_light":true,"distance":3,"size":2,
            "spread":0,"opacity":0.5,"color":"#000000"}]}"##,
    )
    .unwrap();
    doc = doc.set_layer_style(1, Some(Arc::new(style))).unwrap();
    let master = noise16(50, 40, 9);
    let edit = SourceEdit::parse(
        &json!({"op":"place","source_id":"camera","revision_id":"a","name":"Camera",
            "x":0,"y":0,"depth_conversion":"convert_fallback"})
        .to_string(),
    )
    .unwrap();
    doc = doc
        .source_edit_input(Some(SourceInput::Typed(&master)), &edit, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let layer = doc.layers.len() - 1;
    let resource = noise(64, 64, 10).into_raw();
    let descriptor = CString::new(
        json!({"schema_version":1,"original_filename":"probe.dng","content_sha256":SHA,
            "decoder":{"identifier":"independent","version":"1","implementation":"fixture"},
            "recipe":{"schema_version":3,"tint":11.999950408935547}})
        .to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let attached = unsafe {
        dr_doc_attach_raw_source(
            &doc,
            layer,
            resource.as_ptr(),
            resource.len(),
            descriptor.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!attached.is_null(), "{}", unsafe { text(error) });
    unsafe { *Box::from_raw(attached) }
}

/// (a) The file never exceeds `encoded_bound`; a longer one means a plan term
/// is wrong, never this assertion.
#[test]
fn saved_file_fits_the_encoded_bound() {
    let _serial = serial();
    let dir = TempDir::new().unwrap();
    check(
        &DrDocument::from_pixels(noise(640, 480, 1)),
        &dir,
        "noise.rz",
    );
    check(&placed(640, 480), &dir, "placed.rz");
    let linear = check(&rich_linear(), &dir, "rich-linear.rz");
    for word in [45, 63, 61, 59, 53, 43, 41, 38, 28, 10] {
        assert!(linear.contains(&word), "{word} missing from {linear:?}");
    }
    let sources = check(&rich_sources(), &dir, "rich-sources.rz");
    for word in [45, 53, 18] {
        assert!(sources.contains(&word), "{word} missing from {sources:?}");
    }
}

/// (b) NULL answers 0 with a non-empty error, and tolerates a NULL `err_out`.
#[test]
fn null_document_answers_zero() {
    let _serial = serial();
    let mut error = ptr::null_mut();
    assert_eq!(
        unsafe { dr_doc_native_save_scratch_bytes(ptr::null(), &mut error) },
        0
    );
    assert!(!unsafe { text(error) }.is_empty());
    assert_eq!(
        unsafe { dr_doc_native_save_scratch_bytes(ptr::null(), ptr::null_mut()) },
        0
    );
}

/// (c) The plan reads stored dimensions only, and `referenced − views` (the
/// archive's `stored`) is the same live and evicted.
#[test]
fn plan_does_not_depend_on_views() {
    let _serial = serial();
    let doc = placed(640, 480);
    let stored = |report: &Value| {
        report["referenced_bytes"].as_u64().unwrap()
            - report["contiguous_view_bytes"].as_u64().unwrap()
    };
    let live = resource_report::report(&[&doc as *const DrDocument], &[], &[], &[], 0, 0).unwrap();
    assert!(
        live["contiguous_view_bytes"].as_u64().unwrap() > 0,
        "{live}"
    );
    let before = ffi_scratch(&doc);
    resource_report::evict(&[&doc as *const DrDocument]).unwrap();
    let evicted =
        resource_report::report(&[&doc as *const DrDocument], &[], &[], &[], 0, 0).unwrap();
    assert_eq!(
        evicted["contiguous_view_bytes"].as_u64().unwrap(),
        0,
        "{evicted}"
    );
    assert_eq!(ffi_scratch(&doc), before);
    assert_eq!(stored(&evicted), stored(&live));
}
