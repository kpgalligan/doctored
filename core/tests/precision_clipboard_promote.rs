//! The clipboard's promotion into a 32-bit float document
//! (`precision_clipboard_promote`), the Preserve mode of
//! `ClipboardPayload::converted_with`, `to_rgba8_in` (and its FFI
//! `dr_clipboard_payload_rgba8` with a profile) and the paste's exact
//! admission threshold (`clipboard_paste_plan`).
//!
//! Every colour reference is the independent f64 chain over the profile's
//! own tag DATA (`common::icc_chain`), never read off the implementation:
//! code → linear by the profile's `curv` table, then
//! `inv(dst.to_pcs) · src.to_pcs` on linear RGB.
//!
//! Tolerances, fixed before anything was measured: a promoted linear
//! component is within 1e-5 of its f64 chain (the core decodes through a
//! 65536-entry f32 table and rounds the matrix product once to f32), alpha
//! is exactly `code as f32 / 65535`; an f32 → f32 conversion is within 1e-5
//! relative (to max(|want|, 1)); Preserve relabels bit-exactly; the
//! `to_rgba8_in` equalities and the admission threshold are EXACT.

use crate::common;

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_decode, dr_clipboard_payload_free, dr_clipboard_payload_rgba8,
    dr_precision_doc_clipboard_paste, DrClipboardPayload,
};
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::Resolution;
use doctored_core::precision_clipboard::{ClipboardPayload, PasteProfile, PasteRequest};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use serde_json::json;

use common::icc_chain::{apply, inverse, Chain};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

// ------------------------------------------------------------- plumbing --

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    Some(text)
}

enum Samples {
    U16(Vec<[u16; 4]>),
    F32(Vec<[f32; 4]>),
}

/// The test's own RZCB v3 writer: no coverage, no editable section; a u16
/// payload carries operator 0, an f32 one `Clip` (operator 0, white 4).
fn drcb(w: u32, h: u32, profile: &[u8], samples: &Samples) -> Vec<u8> {
    let mut out = b"RZCB".to_vec();
    out.extend(3u32.to_le_bytes());
    out.extend(
        match samples {
            Samples::U16(_) => 1u32,
            Samples::F32(_) => 2u32,
        }
        .to_le_bytes(),
    );
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    out.extend(3i32.to_le_bytes());
    out.extend((-2i32).to_le_bytes());
    out.extend(72f32.to_le_bytes());
    out.extend(72f32.to_le_bytes());
    out.extend((profile.len() as u32).to_le_bytes());
    out.extend(profile);
    out.push(0);
    out.extend(0u32.to_le_bytes());
    out.extend(4f32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    match samples {
        Samples::U16(p) => p.iter().flatten().for_each(|v| out.extend(v.to_le_bytes())),
        Samples::F32(p) => p.iter().flatten().for_each(|v| out.extend(v.to_le_bytes())),
    }
    out
}

fn decode(bytes: &[u8]) -> ClipboardPayload {
    ClipboardPayload::decode(bytes, LIMIT, 0).expect("a payload")
}

/// The same stream as an FFI handle.
fn handle(bytes: &[u8]) -> *mut DrClipboardPayload {
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_clipboard_payload_decode(bytes.as_ptr(), bytes.len(), LIMIT, 0, &mut err) };
    assert_eq!(take_error(err), None);
    assert!(!out.is_null());
    out
}

/// Every u16 code family: the ends, the midpoint, and a spread with
/// varied alpha.
fn u16_pattern(n: u32) -> Vec<[u16; 4]> {
    let alphas = [0u16, 1, 32768, 65534, 65535];
    (0..n)
        .map(|i| {
            let k = i as usize;
            [
                ((k * 7919) % 65536) as u16,
                ((k * 104_729 + 3) % 65536) as u16,
                [0u16, 65535, 32768, 1, 65534][k % 5],
                alphas[k % 5],
            ]
        })
        .collect()
}

fn request(placement: &str, profile: &str) -> PasteRequest {
    serde_json::from_value(json!({
        "name": "Pasted", "anchor": 0, "position": "above",
        "placement": placement, "profile": profile,
    }))
    .unwrap()
}

/// A float document whose one raster holds `pixels`.
fn float_doc(w: u32, h: u32, profile: &[u8], pixels: Vec<[f32; 4]>) -> PrecisionDocument {
    let payload = decode(&drcb(w, h, profile, &Samples::F32(pixels)));
    let doc = PrecisionDocument::from_clipboard(&payload, LIMIT, 0, None).expect("float doc");
    assert!(doc.is_float());
    doc
}

fn rgba32(payload: &ClipboardPayload) -> Vec<[f32; 4]> {
    payload.image().to_rgba32().ok().unwrap()
}

/// The f64 chain: `code` decoded by `decode`'s curve, then `matrix`.
fn promoted_oracle(decode: &Chain, matrix: Option<[[f64; 3]; 3]>, code: [u16; 4]) -> [f64; 4] {
    let linear: [f64; 3] = std::array::from_fn(|c| decode.decode(f64::from(code[c]) / 65535.0));
    let rgb = matrix.map_or(linear, |m| apply(m, linear));
    [rgb[0], rgb[1], rgb[2], f64::from(code[3]) / 65535.0]
}

fn combined(src: &Chain, dst: &Chain) -> [[f64; 3]; 3] {
    let inv = inverse(dst.to_pcs);
    std::array::from_fn(|r| {
        std::array::from_fn(|c| (0..3).map(|k| inv[r][k] * src.to_pcs[k][c]).sum())
    })
}

fn assert_promoted(got: &[[f32; 4]], codes: &[[u16; 4]], want: impl Fn([u16; 4]) -> [f64; 4]) {
    assert_eq!(got.len(), codes.len());
    for (i, (g, code)) in got.iter().zip(codes).enumerate() {
        let w = want(*code);
        for c in 0..3 {
            let d = (f64::from(g[c]) - w[c]).abs();
            assert!(d <= 1e-5, "pixel {i} channel {c}: {} vs {}", g[c], w[c]);
        }
        assert_eq!(
            g[3],
            f32::from(code[3]) / 65535.0,
            "pixel {i}: alpha is exact"
        );
    }
}

// ------------------------------------------------------------ promotion --

#[test]
fn u16_promotes_into_float_against_the_f64_chain() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let (s, d) = (Chain::of(&srgb), Chain::of(&p3));
    let codes = u16_pattern(90);
    // The same profile: the TRC alone.
    let payload = decode(&drcb(9, 10, srgb.bytes(), &Samples::U16(codes.clone())));
    let out = payload
        .converted_with(
            PrecisionStorage::Rgba32Float,
            &srgb,
            PasteProfile::Convert,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .expect("a promotion");
    assert_eq!(out.storage(), PrecisionStorage::Rgba32Float);
    assert_eq!(out.offset(), (3, -2));
    assert!(!out.byte_origin());
    assert_promoted(&rgba32(&out), &codes, |c| promoted_oracle(&s, None, c));
    // P3 → sRGB: the P3 curve, then the combined matrix (negatives kept).
    let p3_payload = decode(&drcb(9, 10, p3.bytes(), &Samples::U16(codes.clone())));
    let out = p3_payload
        .converted_with(
            PrecisionStorage::Rgba32Float,
            &srgb,
            PasteProfile::Convert,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(&**out.image().profile().bytes(), &srgb.bytes()[..]);
    let m = combined(&d, &s);
    assert_promoted(&rgba32(&out), &codes, |c| promoted_oracle(&d, Some(m), c));
    assert!(
        rgba32(&out).iter().any(|p| p[..3].iter().any(|v| *v < 0.0)),
        "a saturated P3 green is negative in sRGB linear"
    );
    // Preserve: the numbers read as the DESTINATION's encoding, no matrix.
    let out = p3_payload
        .converted_with(
            PrecisionStorage::Rgba32Float,
            &srgb,
            PasteProfile::Preserve,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_promoted(&rgba32(&out), &codes, |c| promoted_oracle(&s, None, c));
}

#[test]
fn byte_origin_promotes_like_its_widened_codes() {
    let srgb = IccProfile::srgb();
    let s = Chain::of(&srgb);
    let flat: Vec<u8> = (0..64u32)
        .flat_map(|i| {
            [
                (i * 4) as u8,
                (255 - i * 3) as u8,
                (i * 37 % 256) as u8,
                (i * 4 + 3) as u8,
            ]
        })
        .collect();
    let payload = ClipboardPayload::from_rgba8(
        &flat,
        8,
        8,
        srgb.bytes(),
        (0, 0),
        Resolution::default(),
        LIMIT,
        0,
    )
    .unwrap();
    assert!(payload.byte_origin());
    let codes: Vec<[u16; 4]> = flat
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]].map(|v| u16::from(v) * 257))
        .collect();
    let out = payload
        .converted_with(
            PrecisionStorage::Rgba32Float,
            &srgb,
            PasteProfile::Convert,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert!(!out.byte_origin());
    assert_promoted(&rgba32(&out), &codes, |c| promoted_oracle(&s, None, c));
}

#[test]
fn float_converts_between_profiles_and_refuses_out_of_range() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let m = combined(&Chain::of(&p3), &Chain::of(&srgb));
    let pixels = vec![
        [0.25, 0.5, 0.75, 1.0],
        [-3.5, 70.0, 0.125, 0.5],
        [1.0, 0.0, 0.0, 0.25],
        [0.0, 1.0, 0.0, 0.0],
    ];
    let payload = decode(&drcb(2, 2, p3.bytes(), &Samples::F32(pixels.clone())));
    let out = payload
        .converted_with(
            PrecisionStorage::Rgba32Float,
            &srgb,
            PasteProfile::Convert,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    for (g, p) in rgba32(&out).iter().zip(&pixels) {
        let want = apply(m, [p[0], p[1], p[2]].map(f64::from));
        for c in 0..3 {
            let d = (f64::from(g[c]) - want[c]).abs();
            assert!(
                d <= 1e-5 * want[c].abs().max(1.0),
                "{} vs {}",
                g[c],
                want[c]
            );
        }
        assert_eq!(g[3], p[3], "alpha kept");
    }
    // Preserve between profiles at the same storage converts nothing.
    assert!(payload
        .converted_with(
            PrecisionStorage::Rgba32Float,
            &srgb,
            PasteProfile::Preserve,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .is_none());
    // P3's red at the stored maximum leaves the range in sRGB: refused, not
    // clipped.
    let hot = decode(&drcb(
        1,
        1,
        p3.bytes(),
        &Samples::F32(vec![[65504.0, 0.0, 0.0, 1.0]]),
    ));
    let err = hot
        .converted_with(
            PrecisionStorage::Rgba32Float,
            &srgb,
            PasteProfile::Convert,
            LIMIT,
            0,
            None,
        )
        .err()
        .unwrap();
    assert!(err.contains("outside the 32-bit float range"), "{err}");
}

#[test]
fn preserve_relabels_sixteen_bits_exactly_through_the_ffi() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let codes = u16_pattern(12);
    let target = PrecisionDocument::from_clipboard(
        &decode(&drcb(4, 3, srgb.bytes(), &Samples::U16(u16_pattern(12)))),
        LIMIT,
        0,
        None,
    )
    .unwrap();
    let source = handle(&drcb(4, 3, p3.bytes(), &Samples::U16(codes.clone())));
    for (profile, exact) in [("preserve", true), ("convert", false)] {
        let text = CString::new(
            json!({"name":"Pasted","anchor":0,"position":"above","placement":"in_place","profile":profile})
                .to_string(),
        )
        .unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_clipboard_paste(
                &target,
                source,
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert_eq!(take_error(err), None);
        assert!(!out.is_null());
        let pasted = unsafe { &*out }.layer_source(1).unwrap();
        assert_eq!(&**pasted.profile().bytes(), &srgb.bytes()[..]);
        let samples = pasted.to_rgba16().ok().unwrap();
        assert_eq!(samples == codes, exact, "{profile}");
        unsafe { dr_precision_doc_free(out) };
    }
    unsafe { dr_clipboard_payload_free(source) };
}

// ------------------------------------------------------------ to_rgba8_in --

/// `dr_clipboard_payload_rgba8` with `profile` (None = the payload's own).
fn ffi_rgba8(
    payload: *const DrClipboardPayload,
    profile: Option<&[u8]>,
    preserve: bool,
) -> Vec<u8> {
    let (bytes, len) = profile.map_or((ptr::null(), 0), |p| (p.as_ptr(), p.len()));
    let mut err = ptr::null_mut();
    let image: *mut DrImage = unsafe {
        dr_clipboard_payload_rgba8(
            payload,
            bytes,
            len,
            preserve,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(take_error(err), None);
    assert!(!image.is_null());
    let n = unsafe { doctored_core::ffi::dr_image_width(image) * dr_image_height(image) } as usize;
    let out = unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(image), n * 4) }.to_vec();
    unsafe { dr_image_free(image) };
    out
}

#[test]
fn to_rgba8_in_reads_through_the_u16_conversion() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    // A float payload: its u16 conversion (own tone map), then the read.
    let floats = drcb(
        2,
        2,
        srgb.bytes(),
        &Samples::F32(vec![
            [2.0, 0.5, -1.0, 1.0],
            [0.25, 0.75, 0.1, 0.5],
            [0.0, 0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0, 1.0],
        ]),
    );
    let want = decode(&floats)
        .converted(PrecisionStorage::Rgba16Unorm, &srgb, LIMIT, 0, None)
        .unwrap()
        .unwrap()
        .to_rgba8(LIMIT, 0, None)
        .unwrap();
    let ffi = handle(&floats);
    assert_eq!(ffi_rgba8(ffi, None, false), want.as_raw().clone());
    unsafe { dr_clipboard_payload_free(ffi) };
    // A P3 u16 payload into sRGB: `converted`, then the read.
    let stream = drcb(4, 3, p3.bytes(), &Samples::U16(u16_pattern(12)));
    let payload = decode(&stream);
    let want = payload
        .converted(PrecisionStorage::Rgba16Unorm, &srgb, LIMIT, 0, None)
        .unwrap()
        .unwrap()
        .to_rgba8(LIMIT, 0, None)
        .unwrap();
    let ffi = handle(&stream);
    assert_eq!(
        ffi_rgba8(ffi, Some(srgb.bytes()), false),
        want.as_raw().clone()
    );
    // Preserve into sRGB: the payload's own read.
    let own = payload.to_rgba8(LIMIT, 0, None).unwrap();
    assert_eq!(
        ffi_rgba8(ffi, Some(srgb.bytes()), true),
        own.as_raw().clone()
    );
    assert_eq!(ffi_rgba8(ffi, None, false), own.as_raw().clone());
    unsafe { dr_clipboard_payload_free(ffi) };
}

// ------------------------------------------------------------------ paste --

#[test]
fn paste_into_float_lands_one_float_record_through_the_ffi() {
    let srgb = IccProfile::srgb();
    let target = float_doc(5, 4, srgb.bytes(), vec![[0.5, 0.25, 2.0, 1.0]; 20]);
    let source = handle(&drcb(3, 2, srgb.bytes(), &Samples::U16(u16_pattern(6))));
    let text =
        CString::new(r#"{"name":"Pasted","anchor":0,"position":"above","placement":"centered"}"#)
            .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_paste(
            &target,
            source,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(take_error(err), None);
    let doc = unsafe { &*out };
    assert_eq!(doc.layer_count(), target.layer_count() + 1);
    let record = doc.layer(1).unwrap();
    assert_eq!(
        record.source().unwrap().storage(),
        PrecisionStorage::Rgba32Float
    );
    assert_eq!(record.offset(), (1, 1));
    assert!(doc.is_float());
    unsafe { dr_precision_doc_free(out) };
    unsafe { dr_clipboard_payload_free(source) };
}

/// Refuses at `plan − 1`, admits at `plan`, with `allocation ≤ plan`.
fn assert_exact_threshold(doc: &PrecisionDocument, payload: &ClipboardPayload, what: &str) {
    let request = request("centered", "convert");
    let plan = doc.clipboard_paste_plan(payload, &request).unwrap();
    let allocation = doc.clipboard_paste_allocation(payload, &request).unwrap();
    assert!(allocation <= plan, "{what}: {allocation} > {plan}");
    let err = doc
        .clipboard_paste(payload, &request, plan - 1, 0, None)
        .err()
        .unwrap_or_else(|| panic!("{what}: admitted at plan − 1"));
    assert!(err.contains("working limit"), "{what}: {err}");
    let landed = doc
        .clipboard_paste(payload, &request, plan, 0, None)
        .unwrap_or_else(|e| panic!("{what}: refused at the plan: {e}"));
    assert_eq!(landed.layer_count(), doc.layer_count() + 1, "{what}");
}

#[test]
fn paste_plan_is_the_exact_threshold() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let small = float_doc(4, 4, srgb.bytes(), vec![[0.5; 4]; 16]);
    let large = float_doc(
        1200,
        900,
        srgb.bytes(),
        vec![[0.25, 0.5, 1.5, 1.0]; 1_080_000],
    );
    let wide = decode(&drcb(
        640,
        480,
        p3.bytes(),
        &Samples::U16(u16_pattern(640 * 480)),
    ));
    let tiny = decode(&drcb(3, 2, srgb.bytes(), &Samples::U16(u16_pattern(6))));
    let p3_float = decode(&drcb(2, 2, p3.bytes(), &Samples::F32(vec![[0.5; 4]; 4])));
    for (doc, name) in [(&small, "small"), (&large, "large")] {
        assert_exact_threshold(doc, &wide, &format!("{name}: wide P3 u16"));
        assert_exact_threshold(doc, &tiny, &format!("{name}: tiny sRGB u16"));
        assert_exact_threshold(doc, &p3_float, &format!("{name}: P3 float"));
    }
    // Nothing converts: the plan is the insertion alone.
    let same = decode(&drcb(2, 2, srgb.bytes(), &Samples::F32(vec![[0.5; 4]; 4])));
    assert_exact_threshold(&small, &same, "small: same float");
}
