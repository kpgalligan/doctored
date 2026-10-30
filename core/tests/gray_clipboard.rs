//! #1 phase 4, the Grayscale clipboard (`gray_clipboard`), black-box
//! through the FFI. Copy of a Gray document makes an RZCB v5 payload that
//! round-trips byte for byte and refuses each malformed stream by name (a
//! v3 stream still decodes as RGB). Between Gray documents of one Gray ICC
//! a paste keeps the codes bit for bit; another Gray profile converts under
//! Convert within 1 code of `encode_B(Y_A(code))` (both curves computed
//! here from their gammas) and keeps the numbers under Don't Convert. RGB →
//! Gray lands within 4/65535 of the hand-computed luminance (sRGB's
//! published D50 Y row over the IEC decode, then Gray Gamma 2.2's curve) and
//! equals Image ▸ Mode ▸ Grayscale of the same pixels; Gray → RGB equals
//! Image ▸ Mode ▸ RGB Color of the source and stays neutral within 1 code;
//! CMYK → Gray equals the CMYK → ProPhoto RGB → Gray chain run as two
//! pastes. Don't Convert across modes refuses with the cross-mode sentence,
//! as does `dr_clipboard_payload_rgba8` with a profile. The pasteboard TIFF
//! holds one gray sample (and alpha only when some pixel is not opaque) at
//! the payload's depth with the Gray ICC, and reads back exactly; an
//! untagged gray TIFF is assigned the working Gray by name. New from
//! Clipboard is Gray; the layers paste refuses a mode mix. Bounds declared
//! before measuring.

use std::ffi::{c_char, CString};
use std::io::Cursor;
use std::ptr;

use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_bytes, dr_clipboard_payload_decode, dr_clipboard_payload_free,
    dr_clipboard_payload_image, dr_clipboard_payload_info, dr_clipboard_payload_rgba8,
    dr_precision_doc_clipboard_copy, dr_precision_doc_clipboard_paste,
    dr_precision_doc_from_clipboard, DrClipboardPayload,
};
use doctored_core::ffi_cmyk_clipboard::dr_clipboard_payload_icc;
use doctored_core::ffi_gray::{
    dr_precision_doc_convert_gray_to_rgb, dr_precision_doc_convert_to_gray,
    dr_precision_doc_gray_profile,
};
use doctored_core::ffi_gray_clipboard::{
    dr_clipboard_payload_from_gray_image, dr_clipboard_payload_gray_tiff,
};
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_precision::dr_precision_doc_descriptor;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

use crate::cmyk_structure_fixture::{cmyk_layers, varied, RgbLayer};
use crate::common::take_err_string;
use crate::gray_fixture::{gray_icc, gray_tiff, srgb_decode, SRGB_Y_ROW};
use crate::gray_ops::{assert_neutral, free, gray_doc, layer, plan_json, uniform_gray, LIMIT};

const W: u32 = 9;
const H: u32 = 7;
const CROSS: &str = "Don't Convert keeps numbers within one colour mode; pixels pasted between Grayscale and another mode are always converted";

fn take(err: *mut c_char) -> Option<String> {
    (!err.is_null()).then(|| take_err_string(err))
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    plan_json(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err).unwrap()
}

/// One pattern pixel: 8-bit sRGB codes ×257 that vary in every channel,
/// every fifth pixel at partial alpha.
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

/// `doc` through Image ▸ Mode ▸ Grayscale into `icc` (else `space`) at 16 bits.
fn to_gray(
    doc: *const PrecisionDocument,
    icc: Option<&[u8]>,
    space: &str,
) -> *mut PrecisionDocument {
    let options = CString::new(format!(r#"{{"space":"{space}","depth":16}}"#)).unwrap();
    let (p, n) = icc.map_or((ptr::null(), 0), |b| (b.as_ptr(), b.len()));
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_gray(
            doc,
            p,
            n,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to gray: {}", take_err_string(err));
    out
}

fn copy(doc: *const PrecisionDocument, request: &str) -> *mut DrClipboardPayload {
    let text = CString::new(request).unwrap();
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
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    out
}

fn copy_layer(doc: *const PrecisionDocument, index: usize) -> *mut DrClipboardPayload {
    let (w, h) = unsafe { &*doc }.dimensions();
    copy(
        doc,
        &format!(r#"{{"op":"copy","layer":{index},"x":0,"y":0,"width":{w},"height":{h}}}"#),
    )
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

/// The pasted (top) record's samples as u16 codes.
fn top16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    let d = unsafe { &*doc };
    d.layer_source(d.layer_count() - 1)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn layer16(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .unwrap()
        .to_rgba16()
        .unwrap()
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

fn gray_profile(doc: *const PrecisionDocument) -> Vec<u8> {
    let mut length = 0usize;
    let pointer = unsafe { dr_precision_doc_gray_profile(doc, &mut length) };
    assert!(!pointer.is_null());
    unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
}

fn payload_samples(payload: *const DrClipboardPayload) -> Vec<[u16; 4]> {
    let image = unsafe { dr_clipboard_payload_image(payload) };
    assert!(!image.is_null());
    let image = unsafe { Box::from_raw(image) };
    image.to_rgba16().unwrap()
}

fn tiff_of(payload: *const DrClipboardPayload) -> Result<Vec<u8>, String> {
    let mut length = 7usize;
    let mut err = ptr::null_mut();
    let pointer =
        unsafe { dr_clipboard_payload_gray_tiff(payload, LIMIT, 0, &mut length, &mut err) };
    if pointer.is_null() {
        assert_eq!(length, 0);
        return Err(take(err).expect("a refusal names itself"));
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec())
}

fn from_tiff(
    bytes: &[u8],
    fallback: Option<&[u8]>,
) -> Result<Option<(*mut DrClipboardPayload, Option<String>)>, String> {
    let working = CString::new("gray_gamma_22").unwrap();
    let (fp, fl) = fallback.map_or((ptr::null(), 0), |b| (b.as_ptr(), b.len()));
    let mut assigned = ptr::null_mut();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_clipboard_payload_from_gray_image(
            bytes.as_ptr(),
            bytes.len(),
            fp,
            fl,
            working.as_ptr(),
            LIMIT,
            0,
            &mut assigned,
            &mut err,
        )
    };
    if let Some(e) = take(err) {
        return Err(e);
    }
    if out.is_null() {
        return Ok(None);
    }
    Ok(Some((out, take(assigned))))
}

#[test]
fn copy_makes_a_v5_payload_that_round_trips_and_refuses_each_malformed_stream() {
    let gray = gray_doc(W, H, 16);
    let payload = copy_layer(gray, 0);
    let encoded = bytes(payload);
    assert_eq!(&encoded[..4], b"RZCB");
    assert_eq!(u32::from_le_bytes(encoded[4..8].try_into().unwrap()), 5);
    assert_eq!(encoded[8], 16);
    let d = info(payload);
    assert_eq!(d["version"], 5);
    assert_eq!(d["mode"], "gray");
    assert_eq!(d["gray_depth"], 16);
    assert_eq!(d["gray_profile"], descriptor(gray)["gray"]["profile"]);
    let icc = gray_profile(gray);
    let mut length = 0usize;
    let p = unsafe { dr_clipboard_payload_icc(payload, &mut length) };
    assert_eq!(unsafe { std::slice::from_raw_parts(p, length) }, &icc[..]);
    let again = decode(&encoded).unwrap();
    assert_eq!(bytes(again), encoded, "the v5 round trip");
    assert_eq!(payload_samples(again), layer16(gray, 0));
    drop_payload(again);

    let icc_len = u32::from_le_bytes(encoded[9..13].try_into().unwrap()) as usize;
    assert_eq!(&encoded[13..13 + icc_len], &icc[..]);
    let inner = 13 + icc_len;
    let with = |at: usize, value: &[u8]| {
        let mut out = encoded.clone();
        out[at..at + value.len()].copy_from_slice(value);
        out
    };
    let header = |icc: &[u8], depth: u8, inner: &[u8]| {
        let mut out = b"RZCB".to_vec();
        out.extend_from_slice(&5u32.to_le_bytes());
        out.push(depth);
        out.extend_from_slice(&(icc.len() as u32).to_le_bytes());
        out.extend_from_slice(icc);
        out.extend_from_slice(inner);
        out
    };
    assert_eq!(
        decode(&with(8, &[12])).unwrap_err(),
        "clipboard payload Gray depth 12 is not 8, 16 or 32"
    );
    assert!(
        decode(&header(IccProfile::srgb().bytes(), 16, &encoded[inner..]))
            .unwrap_err()
            .starts_with("cannot use the Gray profile")
    );
    assert_eq!(
        decode(&with(inner + 4, &2u32.to_le_bytes())).unwrap_err(),
        "a Grayscale clipboard payload must hold one RZCB version 3 stream"
    );
    assert_eq!(
        decode(&with(8, &[32])).unwrap_err(),
        "a Grayscale clipboard payload's storage does not match its depth"
    );
    let rgb = rgb_doc();
    let rgb_payload = copy_layer(rgb, 0);
    let rgb_bytes = bytes(rgb_payload);
    assert_eq!(
        decode(&header(&icc, 16, &rgb_bytes)).unwrap_err(),
        "a Grayscale clipboard payload's pixels must be in its Gray profile's twin"
    );
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(decode(&trailing)
        .unwrap_err()
        .starts_with("clipboard payload is "));
    // v3 still decodes, as RGB.
    let v3 = decode(&rgb_bytes).unwrap();
    assert_eq!(info(v3)["mode"], "rgb");
    drop_payload(v3);
    drop_payload(rgb_payload);
    drop_payload(payload);
    free(rgb);
    free(gray);
}

#[test]
fn between_gray_documents_one_profile_is_exact_and_another_converts_or_keeps_numbers() {
    let rgb = rgb_doc();
    let (gamma_a, gamma_b) = (2.2, 1.8);
    let icc_a = gray_icc("Gamma A", gamma_a);
    let icc_b = gray_icc("Gamma B", gamma_b);
    let a = to_gray(rgb, Some(&icc_a), "gray_gamma_22");
    let b = to_gray(rgb, Some(&icc_b), "gray_gamma_22");
    let payload = copy_layer(a, 0);
    // The same Gray ICC: bit for bit, under either mode.
    for mode in ["convert", "preserve"] {
        let out = paste(a, payload, mode).unwrap();
        assert_eq!(top16(out), layer16(a, 0), "{mode}");
        assert_eq!(descriptor(out)["color_mode"], "gray");
        free(out);
    }
    // Another Gray ICC under Don't Convert: the numbers are kept.
    let kept = paste(b, payload, "preserve").unwrap();
    assert_eq!(top16(kept), layer16(a, 0));
    free(kept);
    // Under Convert: within 1 code of encode_B(Y_A(code)), each curve its
    // u8Fixed8 gamma.
    let fixed = |g: f64| (g * 256.0).round() / 256.0;
    let converted = paste(b, payload, "convert").unwrap();
    let got = top16(converted);
    for (i, (p, s)) in got.iter().zip(layer16(a, 0)).enumerate() {
        assert!(p[0] == p[1] && p[1] == p[2], "pixel {i}: {p:?}");
        let y = (f64::from(s[0]) / 65535.0).powf(fixed(gamma_a));
        let oracle = (y.powf(1.0 / fixed(gamma_b)) * 65535.0).round() as i64;
        assert!(
            (i64::from(p[0]) - oracle).abs() <= 1,
            "pixel {i}: {} vs {oracle}",
            p[0]
        );
        assert_eq!(p[3], s[3], "pixel {i} alpha");
    }
    free(converted);
    drop_payload(payload);
    free(a);
    free(b);
    free(rgb);
}

#[test]
fn rgb_into_gray_is_the_luminance_and_equals_mode_grayscale() {
    let rgb = rgb_doc();
    let reference = to_gray(rgb, None, "gray_gamma_22");
    let target = uniform_gray(W, H, 16, 0.5);
    let payload = copy_layer(rgb, 0);
    assert_eq!(paste(target, payload, "preserve").unwrap_err(), CROSS);
    let out = paste(target, payload, "convert").unwrap();
    let got = top16(out);
    assert_eq!(
        got,
        layer16(reference, 0),
        "Mode ▸ Grayscale of the same pixels"
    );
    for (i, p) in got.iter().enumerate() {
        let s = pattern(i as u32);
        let y: f64 = (0..3)
            .map(|c| SRGB_Y_ROW[c] * srgb_decode(f64::from(s[c]) / 65535.0))
            .sum();
        let oracle = (y.powf(1.0 / 2.2) * 65535.0).round() as i64;
        assert!(p[0] == p[1] && p[1] == p[2], "pixel {i}");
        assert!(
            (i64::from(p[0]) - oracle).abs() <= 4,
            "pixel {i}: {} vs {oracle}",
            p[0]
        );
        assert_eq!(p[3], s[3]);
    }
    free(out);
    drop_payload(payload);
    free(target);
    free(reference);
    free(rgb);
}

#[test]
fn gray_into_rgb_equals_mode_rgb_color() {
    let gray = gray_doc(W, H, 16);
    let srgb = IccProfile::srgb();
    let mut err = ptr::null_mut();
    let reference = unsafe {
        dr_precision_doc_convert_gray_to_rgb(
            gray,
            srgb.bytes().as_ptr(),
            srgb.bytes().len(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!reference.is_null(), "{}", take_err_string(err));
    let target = rgb_doc();
    let payload = copy_layer(gray, 0);
    assert_eq!(paste(target, payload, "preserve").unwrap_err(), CROSS);
    let out = paste(target, payload, "convert").unwrap();
    assert_eq!(descriptor(out)["color_mode"], "rgb");
    let got = top16(out);
    assert_eq!(got, layer16(reference, 0));
    for p in &got {
        assert!(
            p[0].abs_diff(p[1]) <= 1 && p[1].abs_diff(p[2]) <= 1,
            "{p:?}"
        );
    }
    // Into CMYK it converts, and Don't Convert refuses.
    let cmyk = cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Ink",
            w: W,
            h: H,
            offset: (0, 0),
            pixels: varied(W, H, 3),
        }],
    );
    assert_eq!(paste(cmyk, payload, "preserve").unwrap_err(), CROSS);
    let inked = paste(cmyk, payload, "convert").unwrap();
    assert_eq!(descriptor(inked)["color_mode"], "cmyk");
    free(inked);
    free(cmyk);
    free(out);
    drop_payload(payload);
    free(target);
    free(reference);
    free(gray);
}

#[test]
fn cmyk_into_gray_is_the_prophoto_chain() {
    let cmyk = cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Ink",
            w: W,
            h: H,
            offset: (0, 0),
            pixels: varied(W, H, 11),
        }],
    );
    let ink = copy_layer(cmyk, 0);
    let gray = uniform_gray(W, H, 16, 0.5);
    assert_eq!(paste(gray, ink, "preserve").unwrap_err(), CROSS);
    let direct = paste(gray, ink, "convert").unwrap();
    // The chain: CMYK → ProPhoto RGB (a paste), then RGB → Gray (a paste).
    let prophoto = doc_in(
        IccProfile::prophoto_rgb(),
        vec![[0, 0, 0, 65535]; (W * H) as usize],
    );
    let staged = paste(prophoto, ink, "convert").unwrap();
    let rgb = copy_layer(staged, 1);
    let chained = paste(gray, rgb, "convert").unwrap();
    let got = top16(direct);
    assert_neutral(&layer(direct, 1, 16), "CMYK into Gray");
    assert_eq!(got, top16(chained));
    free(chained);
    drop_payload(rgb);
    free(staged);
    free(prophoto);
    free(direct);
    free(gray);
    drop_payload(ink);
    free(cmyk);
}

/// The TIFF's dimensions, samples a pixel, bits, ICC and samples.
fn read_tiff(bytes: &[u8]) -> ((u32, u32), u16, u16, Vec<u8>, DecodingResult) {
    let mut decoder = Decoder::new(Cursor::new(bytes)).unwrap();
    let dims = decoder.dimensions().unwrap();
    let samples = decoder
        .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)
        .unwrap()
        .unwrap_or(1);
    let bits = decoder
        .find_tag_unsigned_vec::<u16>(Tag::BitsPerSample)
        .unwrap()
        .unwrap();
    let icc = decoder.get_tag_u8_vec(Tag::IccProfile).unwrap();
    let image = decoder.read_image().unwrap();
    (dims, samples, bits[0], icc, image)
}

#[test]
fn the_pasteboard_tiff_is_one_gray_channel_and_reads_back_exactly() {
    // 8 bits with partial alpha: two samples, round(v / 257).
    let gray8 = gray_doc(W, H, 8);
    let payload = copy_layer(gray8, 0);
    let tiff = tiff_of(payload).unwrap();
    let (dims, samples, bits, icc, image) = read_tiff(&tiff);
    assert_eq!((dims, samples, bits), ((W, H), 2, 8));
    assert_eq!(icc, gray_profile(gray8));
    let DecodingResult::U8(values) = image else {
        panic!("8-bit samples")
    };
    let source = layer16(gray8, 0);
    assert_eq!(values.len(), source.len() * 2);
    for (p, s) in values.chunks_exact(2).zip(&source) {
        assert_eq!(
            [u32::from(p[0]), u32::from(p[1])],
            [s[0], s[3]].map(|v| (u32::from(v) + 128) / 257)
        );
    }
    // It reads back as the same payload, its own ICC kept.
    let (back, assigned) = from_tiff(&tiff, None).unwrap().unwrap();
    assert_eq!(assigned, None);
    assert_eq!(payload_samples(back), payload_samples(payload));
    assert_eq!(info(back)["gray_profile"], info(payload)["gray_profile"]);
    assert_eq!(info(back)["gray_depth"], 8);
    // New from Clipboard of a Gray payload is a Gray document.
    let mut err = ptr::null_mut();
    let made = unsafe { dr_precision_doc_from_clipboard(back, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!made.is_null(), "{}", take_err_string(err));
    assert_eq!(descriptor(made)["color_mode"], "gray");
    assert_eq!(descriptor(made)["gray"], descriptor(gray8)["gray"]);
    free(made);
    drop_payload(back);
    drop_payload(payload);
    free(gray8);

    // 16 bits, opaque: one sample, the codes themselves.
    let gray16 = uniform_gray(W, H, 16, 0.4);
    let payload = copy_layer(gray16, 0);
    let tiff = tiff_of(payload).unwrap();
    let (dims, samples, bits, _, image) = read_tiff(&tiff);
    assert_eq!((dims, samples, bits), ((W, H), 1, 16));
    let DecodingResult::U16(values) = image else {
        panic!("16-bit samples")
    };
    let source = layer16(gray16, 0);
    assert_eq!(values, source.iter().map(|p| p[0]).collect::<Vec<_>>());
    let (back, _) = from_tiff(&tiff, None).unwrap().unwrap();
    assert_eq!(payload_samples(back), source);
    drop_payload(back);
    drop_payload(payload);
    free(gray16);

    // 32 bits: 16-bit samples, one or two a pixel.
    let gray32 = gray_doc(W, H, 32);
    let payload = copy_layer(gray32, 0);
    let (dims, samples, bits, _, _) = read_tiff(&tiff_of(payload).unwrap());
    assert_eq!((dims, samples, bits), ((W, H), 2, 16));
    drop_payload(payload);
    free(gray32);

    // An untagged gray TIFF takes the working Gray, by name; a fallback Gray
    // profile wins over it.
    let data: Vec<u8> = (0..W * H).map(|i| (i * 3) as u8).collect();
    let untagged = gray_tiff(W, H, 8, 1, 1, None, None, &data, None);
    let (back, assigned) = from_tiff(&untagged, None).unwrap().unwrap();
    let assigned = assigned.expect("the untagged sentence");
    assert!(assigned.contains("Gray Gamma 2.2"), "{assigned}");
    assert_eq!(
        payload_samples(back)
            .iter()
            .map(|p| p[0])
            .collect::<Vec<_>>(),
        data.iter().map(|&v| u16::from(v) * 257).collect::<Vec<_>>()
    );
    drop_payload(back);
    let fallback = gray_icc("Fallback Gray", 1.8);
    let (back, assigned) = from_tiff(&untagged, Some(&fallback)).unwrap().unwrap();
    assert!(assigned.unwrap().contains("Fallback Gray"));
    assert_eq!(info(back)["gray_profile"], "Fallback Gray");
    drop_payload(back);
    // Any other bytes: no payload and no error.
    assert!(from_tiff(b"not an image", None).unwrap().is_none());
}

#[test]
fn the_rgba8_read_refuses_preserve_across_modes_and_the_layers_paste_refuses_a_mix() {
    let gray = gray_doc(W, H, 16);
    let payload = copy_layer(gray, 0);
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
    rgba8(None, true).unwrap();
    rgba8(None, false).unwrap();

    let rgb = rgb_doc();
    let mix = "Grayscale layers paste only into a Grayscale document, and a Grayscale document takes only Grayscale layers: convert the clip first";
    for (doc, layers) in [(gray, rgb), (rgb, gray)] {
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
        assert_eq!(take(err).unwrap(), mix);
    }
    drop_payload(payload);
    free(rgb);
    free(gray);
}

#[test]
fn every_gray_clipboard_export_tolerates_null() {
    unsafe {
        let mut length = 9usize;
        assert!(dr_precision_doc_gray_profile(ptr::null(), &mut length).is_null());
        assert_eq!(length, 0);
        assert!(dr_precision_doc_gray_profile(ptr::null(), ptr::null_mut()).is_null());
        let rgb = rgb_doc();
        let mut length = 9usize;
        assert!(dr_precision_doc_gray_profile(rgb, &mut length).is_null());
        assert_eq!(length, 0);

        let mut length = 9usize;
        let mut err = ptr::null_mut();
        assert!(
            dr_clipboard_payload_gray_tiff(ptr::null(), LIMIT, 0, &mut length, &mut err).is_null()
        );
        assert_eq!(length, 0);
        assert!(take(err).is_some());
        assert!(dr_clipboard_payload_gray_tiff(
            ptr::null(),
            LIMIT,
            0,
            ptr::null_mut(),
            ptr::null_mut()
        )
        .is_null());
        // An RGB payload has no gray TIFF.
        let payload = copy_layer(rgb, 0);
        assert_eq!(
            tiff_of(payload).unwrap_err(),
            "the clipboard payload is not Grayscale"
        );
        drop_payload(payload);
        free(rgb);

        let mut err = ptr::null_mut();
        let mut assigned = ptr::null_mut();
        assert!(dr_clipboard_payload_from_gray_image(
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            &mut assigned,
            &mut err
        )
        .is_null());
        assert!(take(err).is_some());
        assert!(assigned.is_null());
        let bytes = [0u8; 4];
        let mut err = ptr::null_mut();
        assert!(dr_clipboard_payload_from_gray_image(
            bytes.as_ptr(),
            4,
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null_mut(),
            &mut err
        )
        .is_null());
        assert_eq!(take(err).unwrap(), "working_gray is NULL");
    }
}
