//! #1: the flattened depth conversions (8→16, 16→8, 32→8) carry every
//! saved channel, alpha and spot, with its name, order, options and ink, as
//! Preserve Layers and the 32→16 tone map already do. Oracles are literal
//! arithmetic written here: `v · 257` up and `(v + 128) / 257` down. The
//! spot's ink and solidity are checked across every depth step, and the
//! plans list `saved_channels` as preserved and charge the channel planes.
use crate::common::take_err_string;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_float::dr_precision_doc_float_to_u16;
use doctored_core::ffi_hdr_toning::dr_doc_hdr_toning_plan;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_conversion::{
    dr_doc_promote_flattened, dr_doc_promote_flattened_plan, dr_precision_doc_downconvert,
    dr_precision_doc_downconvert_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use doctored_core::spot_channel::SpotInk;
use image::{Rgba, RgbaImage};
use serde_json::Value;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// 16 × 16 = 256 samples, so the alpha plane can hold every byte value once.
const W: u32 = 16;
const H: u32 = 16;
const N: u64 = (W * H) as u64;
const RED: [f64; 3] = [54.29, 80.81, 69.89];

unsafe fn text(pointer: *mut c_char) -> String {
    assert!(!pointer.is_null());
    let result = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { dr_string_free(pointer) };
    result
}

fn ink() -> SpotInk {
    SpotInk::new(RED, 60).unwrap()
}

fn alpha_plane() -> Vec<u8> {
    (0..=255u8).collect()
}

fn spot_plane() -> Vec<u8> {
    (0..N as u32).map(|i| ((i * 37 + 11) % 256) as u8).collect()
}

/// A byte document holding an alpha channel (green rubylith, 35 %, Color
/// Indicates: Selected Areas) and then a spot channel.
fn byte_fixture() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        Rgba([(x * 15) as u8, (y * 15) as u8, 90, 255])
    }))
    .add_channel("Mask", &alpha_plane(), W, H, [255, 0, 0], 0.5)
    .unwrap()
    .set_channel_overlay(0, [0, 200, 0], 0.35, true)
    .unwrap()
    .add_spot_channel("Red", ink(), &spot_plane())
    .unwrap()
}

/// Name, overlay colour, overlay opacity, Color Indicates: Selected, ink.
type Row = (String, [u8; 3], f32, bool, Option<SpotInk>);

/// The byte fields every conversion must carry, in order.
fn byte_rows(doc: &DrDocument) -> Vec<Row> {
    doc.channels
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                c.overlay_color,
                c.overlay_opacity,
                c.color_indicates_selected,
                c.spot,
            )
        })
        .collect()
}

fn typed_rows(doc: &PrecisionDocument) -> Vec<Row> {
    doc.channels()
        .iter()
        .map(|c| {
            let m = c.metadata();
            (
                c.name().to_string(),
                m.overlay_color,
                m.overlay_opacity,
                m.color_indicates_selected,
                m.spot,
            )
        })
        .collect()
}

fn promote(doc: &DrDocument) -> PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_promote_flattened(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let result = unsafe { (*out).clone() };
    unsafe { dr_precision_doc_free(out) };
    result
}

fn downconvert(doc: &PrecisionDocument) -> DrDocument {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_downconvert(doc, 0, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let result = unsafe { (*out).clone() };
    unsafe { dr_doc_free(out) };
    result
}

fn float_to_u16(doc: &PrecisionDocument) -> PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_float_to_u16(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let result = unsafe { (*out).clone() };
    unsafe { dr_precision_doc_free(out) };
    result
}

fn widened(plane: &[u8]) -> Vec<u16> {
    plane.iter().map(|&v| u16::from(v) * 257).collect()
}

fn nearest(plane: &[u16]) -> Vec<u8> {
    plane
        .iter()
        .map(|&v| ((u32::from(v) + 128) / 257) as u8)
        .collect()
}

#[test]
fn byte_channels_survive_flatten_8_to_16_to_8_exactly() {
    let doc = byte_fixture();
    let rows = byte_rows(&doc);
    assert_eq!(rows.len(), 2);
    let sixteen = promote(&doc);
    assert_eq!(typed_rows(&sixteen), rows);
    assert_eq!(sixteen.layer_count(), 1);
    for (i, original) in [alpha_plane(), spot_plane()].iter().enumerate() {
        assert_eq!(
            sixteen.channels()[i].plane().samples(),
            widened(original).as_slice(),
            "channel {i} at 16 bits is byte · 257"
        );
    }
    let eight = downconvert(&sixteen);
    assert_eq!(byte_rows(&eight), rows);
    assert_eq!(eight.channel_plane(0).unwrap(), alpha_plane().as_slice());
    assert_eq!(eight.channel_plane(1).unwrap(), spot_plane().as_slice());
    assert_eq!(byte_rows(&doc), rows, "the input is never mutated");
}

#[test]
fn a_typed_flatten_to_8_bits_rounds_each_channel_sample_to_nearest() {
    let pixels = (0..N as u32)
        .map(|i| [(i * 250) as u16, 40000, 1234, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let typed =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    let mut coverage = vec![0u16, 128, 129, 385, 32767, 65535];
    let mut seed = 0x2545_f491_u32;
    while coverage.len() < N as usize {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        coverage.push((seed >> 16) as u16);
    }
    let typed = typed
        .add_spot_channel("Red", ink(), &coverage, LIMIT, 0, None)
        .unwrap();
    let eight = downconvert(&typed);
    assert_eq!(eight.channels.len(), 1);
    assert_eq!(eight.channels[0].spot, Some(ink()));
    assert_eq!(
        eight.channel_plane(0).unwrap(),
        nearest(&coverage).as_slice()
    );
    assert_eq!(
        &eight.channel_plane(0).unwrap()[..6],
        &[0, 0, 1, 1, 127, 255]
    );
}

#[test]
fn the_spot_ink_and_solidity_survive_every_depth_step() {
    let doc = byte_fixture();
    let rows = byte_rows(&doc);
    assert_eq!(rows[1].4, Some(ink()));
    let typed_ok = |label: &str, out: &PrecisionDocument| {
        assert_eq!(typed_rows(out), rows, "{label}");
    };
    let byte_ok = |label: &str, out: &DrDocument| {
        assert_eq!(byte_rows(out), rows, "{label}");
    };

    let flat16 = promote(&doc);
    typed_ok("flatten 8→16", &flat16);
    byte_ok("flatten 16→8", &downconvert(&flat16));
    let preserve16 = doc.promote_structure(LIMIT, 0, None).unwrap();
    typed_ok("preserve 8→16", &preserve16);
    byte_ok(
        "preserve 16→8",
        &preserve16
            .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
            .unwrap(),
    );

    let float8 = doc.promote_to_float(LIMIT, 0, None).unwrap();
    typed_ok("8→32", &float8);
    let float16 = preserve16.promote_to_float(LIMIT, 0, None).unwrap();
    typed_ok("16→32", &float16);
    for (i, channel) in float16.channels().iter().enumerate() {
        assert_eq!(
            channel.plane().samples(),
            preserve16.channels()[i].plane().samples(),
            "channel {i} at 32 bits is the 16-bit plane verbatim"
        );
    }
    for (label, float) in [("from 8", &float8), ("from 16", &float16)] {
        byte_ok(&format!("flatten 32→8 {label}"), &downconvert(float));
        typed_ok(&format!("flatten 32→16 {label}"), &float_to_u16(float));
        typed_ok(
            &format!("preserve 32→16 {label}"),
            &float.narrow_structure_to_u16(LIMIT, 0, None).unwrap(),
        );
        byte_ok(
            &format!("preserve 32→8 {label}"),
            &float
                .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
                .unwrap(),
        );
    }
}

#[test]
fn the_flattened_plans_preserve_and_charge_the_saved_channels() {
    let doc = byte_fixture();
    let c = doc.channels.len() as u64;
    assert_eq!(c, 2);
    let check = |plan: &Value, output: u64| {
        let preserved = plan["preserved"].as_array().unwrap();
        let discarded = plan["discarded"].as_array().unwrap();
        assert!(preserved.iter().any(|v| v == "saved_channels"), "{plan}");
        assert!(!discarded.iter().any(|v| v == "saved_channels"), "{plan}");
        assert_eq!(plan["output_bytes"], output, "{plan}");
    };
    let mut err = ptr::null_mut();
    let promote_plan: Value =
        serde_json::from_str(&unsafe { text(dr_doc_promote_flattened_plan(&doc, &mut err)) })
            .unwrap();
    check(&promote_plan, N * 8 + N * 2 * c);
    let sixteen = promote(&doc);
    let down_plan: Value = serde_json::from_str(&unsafe {
        text(dr_precision_doc_downconvert_plan(&sixteen, &mut err))
    })
    .unwrap();
    check(&down_plan, N * 4 + N * c);

    let request = CString::new(r#"{"method":"equalize_histogram"}"#).unwrap();
    let reply = unsafe { dr_doc_hdr_toning_plan(&doc, request.as_ptr(), &mut err) };
    assert!(!reply.is_null(), "{}", take_err_string(err));
    let reply: Value = serde_json::from_str(&unsafe { text(reply) }).unwrap();
    assert_eq!(reply["output_bytes"], N * 4 + N * c);
}
