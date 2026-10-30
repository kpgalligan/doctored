//! #1 Lab phase 4c, the Lab Color clipboard (`lab_clipboard`), black-box
//! through the FFI at 8 and 16 bits. The oracles are the Mode conversions
//! of the same pixels, run here through their own exports. Bounds, declared
//! before measuring:
//!
//! * (a) Copy of a Lab document is an RZCB v6 payload (`info`: mode lab,
//!   version 6, `lab_depth` the document's) whose bytes decode to equal
//!   bytes, and its paste into a second Lab document of the same depth
//!   equals the source codes exactly under Convert and Don't Convert; each
//!   decode refusal answers its message (a bad depth byte, an inner stream
//!   that is not v3, inner storage that is not RGBA16, an inner profile that
//!   is not the twin, the byte-origin flag, a trailing byte). A version that
//!   is not 6 never reaches the Lab decoder through the FFI (the version
//!   word picks the decoder), so it has no case here;
//! * (b) a Lab → RGB paste into a 16-bit sRGB document equals
//!   `dr_precision_doc_convert_lab_to_rgb` (sRGB, `{"black_point_compensation":
//!   true}`) of the same codes, exactly;
//! * (c) an RGB → Lab paste equals `dr_precision_doc_convert_to_lab`
//!   (`{"depth":16}` / `{"depth":8}`) of the same RGB pixels, exactly;
//! * (d) a Lab → Gray paste equals the Gray paste of (b)'s conversion into
//!   ProPhoto RGB, a CMYK → Lab paste equals (c)'s rule on the CMYK →
//!   ProPhoto payload, a Lab → CMYK paste is a CMYK document, and Don't
//!   Convert refuses across modes both ways (and in the RGBA8 read given a
//!   profile) with the Lab cross-mode sentence;
//! * (e) Duplicate to New Document keeps `color_mode` lab and the depth and
//!   the codes; the layers paste refuses Lab ↔ RGB with the Lab mix
//!   sentence; a 16-bit payload into an 8-bit Lab document lands every
//!   sample on the ×257 grid, each the nearest grid code of the source's;
//! * (f) New from Clipboard of a Lab payload is a Lab document of its depth
//!   holding its codes, and the trimmed payload stays Lab;
//! * (g, phase 6a-i) the RGBA8 read of a Lab payload under a control ends at
//!   progress exactly 1.0, and a cancel with the worker parked inside the
//!   read half (past the first report at progress ≥ 0.5) returns
//!   `operation cancelled` with no scratch left reserved;
//! * (h, phase 6a-i) a payload declaring 8 bits whose codes are off the
//!   ×257 grid (a 16-bit copy's bytes with the depth byte set to 8) decodes;
//!   pasted into an 8-bit Lab document every sample and alpha is the nearest
//!   ×257 code of the source's, New from Clipboard of it is an 8-bit Lab
//!   document on the grid, and pasted into a 16-bit Lab document it keeps
//!   the codes exactly.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_bytes, dr_clipboard_payload_decode, dr_clipboard_payload_free,
    dr_clipboard_payload_image, dr_clipboard_payload_info, dr_clipboard_payload_rgba8,
    dr_clipboard_payload_trim, dr_precision_doc_clipboard_copy, dr_precision_doc_clipboard_paste,
    dr_precision_doc_from_clipboard, DrClipboardPayload,
};
use doctored_core::ffi_jobs::{dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_convert_to_lab, dr_precision_doc_new_lab,
};
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, varied, RgbLayer};
use crate::common::probe::{cancelled_parked_at, entry_count};
use crate::common::take_err_string;
use crate::gray_ops::{edit, free, plan_json, uniform_gray, LIMIT};
use crate::lab_geometry::{descriptor, document, ramp, twin_profile, DEPTHS, H, W};

const CROSS: &str = "Don't Convert keeps numbers within one colour mode; pixels pasted between Lab Color and another mode are always converted";
const MIX: &str = "Lab Color layers paste only into a Lab Color document, and a Lab Color document takes only Lab Color layers: convert the clip first";

fn take(err: *mut c_char) -> Option<String> {
    (!err.is_null()).then(|| take_err_string(err))
}

/// A one-layer Lab document of `ramp(seed)` at `bits`.
fn lab_doc(bits: u32, seed: u32) -> *mut PrecisionDocument {
    let image =
        PrecisionImage::from_rgba16(W, H, ramp(W, H, bits, seed), twin_profile(), None).unwrap();
    document(
        &[Layer::raster("Background", image, (0, 0), 1.0, true).unwrap()],
        bits,
        true,
    )
}

/// One pattern pixel: 8-bit sRGB codes ×257 that vary in every channel,
/// every fifth pixel at partial alpha (`gray_clipboard.rs`'s).
fn pattern(i: u32) -> [u16; 4] {
    let r = (i * 37 + 11) % 256;
    let g = (i * 91 + 60) % 256;
    let b = (i * 53 + 140) % 256;
    let a = if i % 5 == 2 { 41_120 } else { 65_535 };
    [(r * 257) as u16, (g * 257) as u16, (b * 257) as u16, a]
}

fn doc_in(profile: std::sync::Arc<IccProfile>, pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, profile, None).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ))
}

/// The 16-bit sRGB pattern document.
fn rgb_doc() -> *mut PrecisionDocument {
    doc_in(IccProfile::srgb(), (0..W * H).map(pattern).collect())
}

fn copy_layer(doc: *const PrecisionDocument, index: usize) -> *mut DrClipboardPayload {
    let (w, h) = unsafe { &*doc }.dimensions();
    let text = CString::new(format!(
        r#"{{"op":"copy","layer":{index},"x":0,"y":0,"width":{w},"height":{h}}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_copy(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "copy: {}", take_err_string(err));
    out
}

fn paste(
    doc: *const PrecisionDocument,
    payload: *const DrClipboardPayload,
    profile: &str,
) -> Result<*mut PrecisionDocument, String> {
    let anchor = unsafe { &*doc }.layer_count() - 1;
    let text = CString::new(format!(
        r#"{{"name":"Pasted","anchor":{anchor},"position":"top","placement":"in_place","profile":"{profile}"}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_paste(
            doc,
            payload,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take(err).expect("a refusal names itself"))
    } else {
        Ok(out)
    }
}

fn layer16(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

/// The pasted (top) record's samples.
fn top16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    layer16(doc, unsafe { &*doc }.layer_count() - 1)
}

fn bytes(payload: *const DrClipboardPayload) -> Vec<u8> {
    let mut length = 0usize;
    let pointer = unsafe { dr_clipboard_payload_bytes(payload, &mut length) };
    unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
}

fn decode(bytes: &[u8]) -> Result<*mut DrClipboardPayload, String> {
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_clipboard_payload_decode(bytes.as_ptr(), bytes.len(), LIMIT, 0, &mut err) };
    if out.is_null() {
        Err(take(err).expect("a refusal names itself"))
    } else {
        Ok(out)
    }
}

fn info(payload: *const DrClipboardPayload) -> Value {
    let mut err = ptr::null_mut();
    plan_json(unsafe { dr_clipboard_payload_info(payload, &mut err) }, err).unwrap()
}

fn drop_payload(payload: *mut DrClipboardPayload) {
    unsafe { dr_clipboard_payload_free(payload) };
}

fn payload_samples(payload: *const DrClipboardPayload) -> Vec<[u16; 4]> {
    let image = unsafe { dr_clipboard_payload_image(payload) };
    assert!(!image.is_null());
    let image = unsafe { Box::from_raw(image) };
    image.to_rgba16().unwrap()
}

/// Image ▸ Mode ▸ RGB Color of `doc` into `profile`, relative colorimetric
/// with BPC.
fn lab_to_rgb(doc: *const PrecisionDocument, profile: &IccProfile) -> *mut PrecisionDocument {
    let options = CString::new(r#"{"black_point_compensation":true}"#).unwrap();
    let icc = profile.bytes();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            doc,
            icc.as_ptr(),
            icc.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to rgb: {}", take_err_string(err));
    out
}

/// Image ▸ Mode ▸ Lab Color of `doc` at `bits`.
fn to_lab(doc: *const PrecisionDocument, bits: u32) -> *mut PrecisionDocument {
    let options = CString::new(json!({"depth":bits}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_lab(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!out.is_null(), "to lab: {}", take_err_string(err));
    out
}

fn cmyk_doc(seed: u32) -> *mut PrecisionDocument {
    cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Ink",
            w: W,
            h: H,
            offset: (0, 0),
            pixels: varied(W, H, seed),
        }],
    )
}

#[test]
fn a_lab_copy_is_a_v6_payload_that_round_trips_and_refuses_each_malformed_stream() {
    for bits in DEPTHS {
        let source = lab_doc(bits, 0);
        let payload = copy_layer(source, 0);
        let encoded = bytes(payload);
        assert_eq!(&encoded[..4], b"RZCB");
        assert_eq!(u32::from_le_bytes(encoded[4..8].try_into().unwrap()), 6);
        assert_eq!(u32::from(encoded[8]), bits);
        let d = info(payload);
        assert_eq!(d["version"], 6);
        assert_eq!(d["mode"], "lab");
        assert_eq!(d["lab_depth"], bits);
        assert_eq!(payload_samples(payload), layer16(source, 0));
        let again = decode(&encoded).unwrap();
        assert_eq!(bytes(again), encoded, "the v6 round trip at {bits} bits");
        drop_payload(again);
        let target = lab_doc(bits, 9);
        for mode in ["convert", "preserve"] {
            let out = paste(target, payload, mode).unwrap();
            assert_eq!(top16(out), layer16(source, 0), "{mode} at {bits} bits");
            assert_eq!(descriptor(out)["color_mode"], "lab");
            assert_eq!(descriptor(out)["lab"]["depth"], bits);
            free(out);
        }
        free(target);
        drop_payload(payload);
        free(source);
    }

    let source = lab_doc(16, 0);
    let payload = copy_layer(source, 0);
    let encoded = bytes(payload);
    let inner = 9usize;
    let with = |at: usize, value: &[u8]| {
        let mut out = encoded.clone();
        out[at..at + value.len()].copy_from_slice(value);
        out
    };
    assert_eq!(
        decode(&with(8, &[12])).unwrap_err(),
        "clipboard payload Lab depth 12 is not 8 or 16"
    );
    assert_eq!(
        decode(&with(inner + 4, &2u32.to_le_bytes())).unwrap_err(),
        "a Lab Color clipboard payload must hold one RZCB version 3 stream"
    );
    assert_eq!(
        decode(&with(inner + 8, &2u32.to_le_bytes())).unwrap_err(),
        "a Lab Color clipboard payload must hold 16-bit storage"
    );
    let rgb = rgb_doc();
    let rgb_payload = copy_layer(rgb, 0);
    let mut foreign = encoded[..inner].to_vec();
    foreign.extend_from_slice(&bytes(rgb_payload));
    assert_eq!(
        decode(&foreign).unwrap_err(),
        "a Lab Color clipboard payload's pixels must be in the Lab twin"
    );
    let profile_len =
        u32::from_le_bytes(encoded[inner + 36..inner + 40].try_into().unwrap()) as usize;
    let flags = inner + 40 + profile_len;
    assert_eq!(
        decode(&with(flags, &[encoded[flags] | 2])).unwrap_err(),
        "a Lab Color clipboard payload never carries the byte-origin flag"
    );
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(decode(&trailing)
        .unwrap_err()
        .starts_with("clipboard payload is "));
    // v3 still decodes, as RGB.
    let v3 = decode(&bytes(rgb_payload)).unwrap();
    assert_eq!(info(v3)["mode"], "rgb");
    drop_payload(v3);
    drop_payload(rgb_payload);
    drop_payload(payload);
    free(rgb);
    free(source);
}

#[test]
fn lab_into_rgb_equals_mode_rgb_color_with_bpc() {
    let srgb = IccProfile::srgb();
    for bits in DEPTHS {
        let source = lab_doc(bits, 3);
        let reference = lab_to_rgb(source, &srgb);
        let target = rgb_doc();
        let payload = copy_layer(source, 0);
        assert_eq!(paste(target, payload, "preserve").unwrap_err(), CROSS);
        let out = paste(target, payload, "convert").unwrap();
        assert_eq!(descriptor(out)["color_mode"], "rgb");
        assert_eq!(top16(out), layer16(reference, 0), "{bits} bits");
        free(out);
        drop_payload(payload);
        free(target);
        free(reference);
        free(source);
    }
}

#[test]
fn rgb_into_lab_equals_mode_lab_color() {
    let rgb = rgb_doc();
    let payload = copy_layer(rgb, 0);
    for bits in DEPTHS {
        let reference = to_lab(rgb, bits);
        let target = lab_doc(bits, 5);
        assert_eq!(paste(target, payload, "preserve").unwrap_err(), CROSS);
        let out = paste(target, payload, "convert").unwrap();
        assert_eq!(descriptor(out)["color_mode"], "lab");
        assert_eq!(top16(out), layer16(reference, 0), "{bits} bits");
        free(out);
        free(target);
        free(reference);
    }
    drop_payload(payload);
    free(rgb);
}

#[test]
fn gray_and_cmyk_cross_lab_through_prophoto_and_dont_convert_refuses() {
    let prophoto = IccProfile::prophoto_rgb();
    // Lab → Gray: the Gray paste of the ProPhoto conversion.
    let source = lab_doc(16, 7);
    let payload = copy_layer(source, 0);
    let gray = uniform_gray(W, H, 16, 0.5);
    assert_eq!(paste(gray, payload, "preserve").unwrap_err(), CROSS);
    let direct = paste(gray, payload, "convert").unwrap();
    assert_eq!(descriptor(direct)["color_mode"], "gray");
    let staged = lab_to_rgb(source, &prophoto);
    let rgb = copy_layer(staged, 0);
    let chained = paste(gray, rgb, "convert").unwrap();
    assert_eq!(top16(direct), top16(chained), "Lab into Gray");
    free(chained);
    drop_payload(rgb);
    free(staged);
    free(direct);
    // Lab → CMYK converts; Don't Convert refuses.
    let cmyk = cmyk_doc(3);
    assert_eq!(paste(cmyk, payload, "preserve").unwrap_err(), CROSS);
    let inked = paste(cmyk, payload, "convert").unwrap();
    assert_eq!(descriptor(inked)["color_mode"], "cmyk");
    free(inked);
    // The RGBA8 read refuses Don't Convert given a profile.
    let srgb = IccProfile::srgb();
    let rgba8 = |profile: Option<&[u8]>, preserve: bool| {
        let (p, n) = profile.map_or((ptr::null(), 0), |b| (b.as_ptr(), b.len()));
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_clipboard_payload_rgba8(payload, p, n, preserve, LIMIT, 0, ptr::null(), &mut err)
        };
        if out.is_null() {
            Err(take(err).expect("a refusal names itself"))
        } else {
            unsafe { dr_image_free(out) };
            Ok(())
        }
    };
    assert_eq!(rgba8(Some(srgb.bytes()), true).unwrap_err(), CROSS);
    rgba8(Some(srgb.bytes()), false).unwrap();
    rgba8(None, false).unwrap();
    drop_payload(payload);
    free(source);

    // CMYK → Lab: Mode ▸ Lab Color's rule on the CMYK → ProPhoto payload.
    let ink = copy_layer(cmyk, 0);
    let pro_doc = doc_in(prophoto, vec![[0, 0, 0, 65535]; (W * H) as usize]);
    let staged = paste(pro_doc, ink, "convert").unwrap();
    let rgb = copy_layer(staged, 1);
    for bits in DEPTHS {
        let lab = lab_doc(bits, 2);
        assert_eq!(paste(lab, ink, "preserve").unwrap_err(), CROSS);
        let direct = paste(lab, ink, "convert").unwrap();
        let chained = paste(lab, rgb, "convert").unwrap();
        assert_eq!(
            top16(direct),
            top16(chained),
            "CMYK into Lab at {bits} bits"
        );
        free(chained);
        free(direct);
        free(lab);
    }
    drop_payload(rgb);
    free(staged);
    free(pro_doc);
    drop_payload(ink);
    free(cmyk);
    free(gray);
}

#[test]
fn duplicate_keeps_lab_the_layers_paste_refuses_a_mix_and_16_into_8_is_on_the_grid() {
    for bits in DEPTHS {
        let source = lab_doc(bits, 1);
        let out = edit(
            source,
            &json!({"op":"duplicate_to_new_document","layers":[0]}),
            &[],
        )
        .unwrap()
        .unwrap();
        assert_eq!(descriptor(out)["color_mode"], "lab");
        assert_eq!(descriptor(out)["lab"]["depth"], bits);
        assert_eq!(layer16(out, 0), layer16(source, 0), "{bits} bits");
        free(out);
        free(source);
    }
    let lab = lab_doc(16, 0);
    let rgb = rgb_doc();
    for (doc, layers) in [(lab, rgb), (rgb, lab)] {
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_paste_layers(
                doc,
                layers,
                0,
                0,
                0,
                0,
                false,
                LIMIT,
                0,
                ptr::null_mut(),
                &mut err,
            )
        };
        assert!(out.is_null());
        assert_eq!(take(err).unwrap(), MIX);
    }
    let payload = copy_layer(lab, 0);
    let eight = lab_doc(8, 4);
    let out = paste(eight, payload, "convert").unwrap();
    let want: Vec<[u16; 4]> = layer16(lab, 0)
        .iter()
        .map(|p| p.map(|v| ((u32::from(v) + 128) / 257 * 257) as u16))
        .collect();
    assert_eq!(top16(out), want);
    free(out);
    free(eight);
    drop_payload(payload);
    free(rgb);
    free(lab);
}

#[test]
fn new_from_clipboard_of_a_lab_payload_is_a_lab_document_of_its_depth() {
    for bits in DEPTHS {
        let source = lab_doc(bits, 6);
        let payload = copy_layer(source, 0);
        let mut err = ptr::null_mut();
        let made =
            unsafe { dr_precision_doc_from_clipboard(payload, LIMIT, 0, ptr::null(), &mut err) };
        assert!(!made.is_null(), "{}", take_err_string(err));
        assert_eq!(descriptor(made)["color_mode"], "lab");
        assert_eq!(descriptor(made)["lab"]["depth"], bits);
        assert_eq!(layer16(made, 0), layer16(source, 0), "{bits} bits");
        free(made);
        drop_payload(payload);
        free(source);
    }
    // A partly transparent layer trims and stays Lab.
    let mut pixels = ramp(W, H, 16, 0);
    for (i, p) in pixels.iter_mut().enumerate() {
        if i as u32 % W < 3 {
            p[3] = 0;
        }
    }
    let image = PrecisionImage::from_rgba16(W, H, pixels, twin_profile(), None).unwrap();
    let source = document(
        &[Layer::raster("Background", image, (0, 0), 1.0, true).unwrap()],
        16,
        true,
    );
    let payload = copy_layer(source, 0);
    let mut err = ptr::null_mut();
    let trimmed = unsafe { dr_clipboard_payload_trim(payload, LIMIT, 0, &mut err) };
    assert!(!trimmed.is_null(), "{:?}", take(err));
    let d = info(trimmed);
    assert_eq!(d["mode"], "lab");
    assert_eq!(d["width"], W - 3);
    drop_payload(trimmed);
    drop_payload(payload);
    free(source);
}

/// The nearest ×257 grid code of each sample.
fn on_grid(pixels: &[[u16; 4]]) -> Vec<[u16; 4]> {
    pixels
        .iter()
        .map(|p| p.map(|v| ((u32::from(v) + 128) / 257 * 257) as u16))
        .collect()
}

/// `payload`'s RGBA8 read into Display P3 under `control`.
fn rgba8_under(payload: *const DrClipboardPayload, control: *const DrJobControl) -> (bool, String) {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_clipboard_payload_rgba8(payload, ptr::null(), 0, false, LIMIT, 0, control, &mut err)
    };
    if out.is_null() {
        (false, take(err).expect("a refusal names itself"))
    } else {
        unsafe { dr_image_free(out) };
        (true, String::new())
    }
}

#[test]
fn the_rgba8_read_reaches_full_progress_and_a_cancel_in_its_read_half_is_seen() {
    // 128 × 64 = 8192 pixels: the 8-bit read reports at pixel 0 and 4096, so
    // a checkpoint lies strictly inside the read half.
    let request = CString::new(r#"{"depth":16,"fill_srgb":[0.8,0.3,0.1,1.0]}"#).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(128, 64, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    let payload = copy_layer(doc, 0);
    let control = unsafe { dr_job_control_create() };
    let (ok, message) = rgba8_under(payload, control);
    assert!(ok, "{message}");
    assert_eq!(unsafe { &*control }.progress(), 1.0);
    unsafe { dr_job_control_free(control) };

    let handle = payload as usize;
    let render = move |token: usize| {
        rgba8_under(
            handle as *const DrClipboardPayload,
            token as *const DrJobControl,
        )
    };
    let n0 = entry_count(&|progress, _, _| progress >= 0.5, render);
    let parked = cancelled_parked_at(n0 + 1, &|progress, _| progress >= 0.5, render);
    assert_eq!(parked.count, n0 + 1);
    drop_payload(payload);
    free(doc);
}

#[test]
fn an_off_grid_8_bit_payload_lands_on_the_grid_in_8_bits_and_keeps_its_codes_in_16() {
    let source = lab_doc(16, 8);
    let copied = copy_layer(source, 0);
    let codes = layer16(source, 0);
    assert_ne!(on_grid(&codes), codes, "the fixture holds off-grid codes");
    let mut crafted = bytes(copied);
    crafted[8] = 8;
    let payload = decode(&crafted).unwrap();
    assert_eq!(info(payload)["lab_depth"], 8);
    let eight = lab_doc(8, 4);
    let out = paste(eight, payload, "convert").unwrap();
    assert_eq!(top16(out), on_grid(&codes), "into 8 bits");
    free(out);
    let out = paste(eight, payload, "preserve").unwrap();
    assert_eq!(top16(out), on_grid(&codes), "into 8 bits, Don't Convert");
    free(out);
    let mut err = ptr::null_mut();
    let made = unsafe { dr_precision_doc_from_clipboard(payload, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!made.is_null(), "{}", take_err_string(err));
    assert_eq!(descriptor(made)["color_mode"], "lab");
    assert_eq!(descriptor(made)["lab"]["depth"], 8);
    assert_eq!(layer16(made, 0), on_grid(&codes), "New from Clipboard");
    free(made);
    let sixteen = lab_doc(16, 2);
    let out = paste(sixteen, payload, "convert").unwrap();
    assert_eq!(top16(out), codes, "into 16 bits");
    free(out);
    free(sixteen);
    free(eight);
    drop_payload(payload);
    drop_payload(copied);
    free(source);
}
