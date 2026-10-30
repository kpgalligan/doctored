//! Soft proofing and the gamut warning through the public C FFI: the chain,
//! the gamut rule, the overlay, the reported space, the NULL sweep, and the
//! promise the whole feature rests on — that proofing changes nothing.
//!
//! The gamut oracle is written HERE, per pixel, from the rule's own words
//! ("the CIE76 ΔE between a colour's Lab and the Lab of its `B2A1 → A2B1`
//! round trip, above 5.0"), evaluated through `icc_lut`'s public model rather
//! than through the proof path being measured. The two must agree exactly,
//! including under `perceptual`, which is the whole objection the rule was
//! written to answer.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_output_color::*;
use doctored_core::ffi_precision::dr_precision_image_preview_rgba8;
use doctored_core::icc::IccProfile;
use doctored_core::icc_intent::RenderingIntent;
use doctored_core::icc_lut::{self, ProofProfile};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};

use crate::common;
use common::probe::{cancelled_parked_at, entry_count};
use common::*;

const FOGRA: &str =
    "/Library/Application Support/Adobe/Color/Profiles/Recommended/CoatedFOGRA39.icc";

fn proof_bytes() -> Option<Vec<u8>> {
    std::fs::read(FOGRA).ok()
}

fn srgb_bytes() -> Vec<u8> {
    IccProfile::srgb().bytes().to_vec()
}

/// A flat image handle from a pixel list, through the public entry point.
fn image_of(pixels: &[[u8; 4]]) -> *mut DrImage {
    let mut buffer = RgbaImage::new(pixels.len() as u32, 1);
    for (x, px) in pixels.iter().enumerate() {
        buffer.put_pixel(x as u32, 0, Rgba(*px));
    }
    let raw = buffer.into_raw();
    let handle = unsafe { dr_image_from_rgba(raw.as_ptr(), pixels.len() as u32, 1) };
    assert!(!handle.is_null());
    handle
}

fn setup(json: &str) -> CString {
    CString::new(json).expect("static json")
}

/// The proof render's pixels and reported info.
fn render(
    flat: *const DrImage,
    document: &[u8],
    proof: &[u8],
    json: &str,
) -> (Vec<u8>, serde_json::Value) {
    let setup = setup(json);
    let mut info: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_proof_render(
            flat,
            document.as_ptr(),
            document.len(),
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut info,
            &mut err,
        )
    };
    if out.is_null() {
        panic!("proof render failed: {}", take_err_string(err));
    }
    assert!(err.is_null());
    let pixels = img_pixels(out);
    unsafe { dr_image_free(out) };
    let info = take_err_string(info);
    (pixels, serde_json::from_str(&info).expect("info is JSON"))
}

/// The independent gamut oracle: the rule's own words, evaluated through the
/// public LUT model.
fn gamut_delta(profile: &ProofProfile, source_profile: &IccProfile, rgb: [u8; 3]) -> f64 {
    let relative = RenderingIntent::RelativeColorimetric;
    let model = source_profile.model().expect("sRGB is matrix/TRC");
    let encoded = rgb.map(|c| f64::from(c) / 255.0);
    // The pixel's own PCS colour, through the DOCUMENT's model.
    let xyz = model.to_pcs_xyz64(encoded);
    let Some((device, count)) = profile.from_xyz(xyz, relative) else {
        return 0.0;
    };
    let Some(back) = profile.to_xyz(&device[..count], relative) else {
        return 0.0;
    };
    icc_lut::delta_e76(icc_lut::xyz_to_lab(xyz), icc_lut::xyz_to_lab(back))
}

/// The rule's own threshold, applied to the oracle above.
fn out_of_gamut(profile: &ProofProfile, source_profile: &IccProfile, rgb: [u8; 3]) -> bool {
    gamut_delta(profile, source_profile, rgb) > 5.0
}

// ------------------------------------------------------------ the chain --

#[test]
fn a_proof_reports_the_space_it_is_in_and_copies_alpha() {
    let Some(proof) = proof_bytes() else {
        eprintln!("skipping: {FOGRA} is not installed");
        return;
    };
    let document = srgb_bytes();
    let source = [
        [255u8, 255, 255, 255],
        [128, 128, 128, 200],
        [0, 0, 0, 0],
        [180, 120, 90, 64],
    ];
    let flat = image_of(&source);
    let (pixels, info) = render(flat, &document, &proof, "{}");
    unsafe { dr_image_free(flat) };
    // The document's own profile CAN encode, so the proof comes back in it.
    assert_eq!(info["space"], "document");
    // Alpha is coverage, not colour, and is copied untouched.
    for (i, px) in source.iter().enumerate() {
        assert_eq!(pixels[i * 4 + 3], px[3], "alpha at {i}");
    }
    // White proofs near white and black proofs near black; the proof is a
    // simulation, not a scramble.
    assert!(
        pixels[0] > 230 && pixels[1] > 230 && pixels[2] > 230,
        "{pixels:?}"
    );
    let black = &pixels[8..11];
    assert!(black.iter().all(|&c| c < 60), "{black:?}");
}

#[test]
fn the_gamut_mask_equals_an_independent_delta_e_check_under_both_intents() {
    let Some(proof_bytes) = proof_bytes() else {
        eprintln!("skipping: {FOGRA} is not installed");
        return;
    };
    let proof = ProofProfile::parse(&proof_bytes).expect("FOGRA39 parses");
    let document_profile = IccProfile::srgb();
    let document = srgb_bytes();

    // A spread that deliberately straddles the boundary, plus the three
    // cases the rule was argued over.
    let mut source: Vec<[u8; 4]> = vec![
        [0, 255, 0, 255],     // far outside a coated press gamut
        [128, 128, 128, 255], // comfortably inside
        [180, 120, 90, 255],  // a skin tone, inside
        [255, 255, 255, 255],
        [0, 0, 0, 255],
    ];
    let mut seed = 0x11C_0F0F_u64;
    let mut next = move || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (seed >> 56) as u8
    };
    // A colour whose ΔE sits ON the threshold would test floating-point
    // luck rather than the rule, so the spread skips that hairline.
    while source.len() < 125 {
        let candidate = [next(), next(), next(), 255];
        let delta = gamut_delta(
            &proof,
            &document_profile,
            [candidate[0], candidate[1], candidate[2]],
        );
        if (delta - 5.0).abs() < 0.05 {
            continue;
        }
        source.push(candidate);
    }
    let flat = image_of(&source);

    let expected: Vec<bool> = source
        .iter()
        .map(|px| out_of_gamut(&proof, &document_profile, [px[0], px[1], px[2]]))
        .collect();

    // The three cases named in the rule's own argument.
    assert!(expected[0], "sRGB green is outside FOGRA39");
    assert!(!expected[1], "mid grey is inside FOGRA39");
    assert!(!expected[2], "an ordinary skin tone is inside FOGRA39");

    for intent in ["relative", "perceptual", "saturation", "absolute"] {
        let json = format!("{{\"intent\":\"{intent}\"}}");
        let setup = setup(&json);
        let mut mask = vec![0u8; source.len()];
        let mut count = 0u64;
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            dr_proof_gamut_mask(
                flat,
                document.as_ptr(),
                document.len(),
                proof_bytes.as_ptr(),
                proof_bytes.len(),
                setup.as_ptr(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                mask.as_mut_ptr(),
                mask.len(),
                &mut count,
                &mut err,
            )
        };
        assert!(ok, "{intent}: {}", take_err_string(err));
        assert!(err.is_null());
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(
                mask[i] == 255,
                *want,
                "pixel {i} {:?} under {intent}",
                source[i]
            );
        }
        assert_eq!(count as usize, expected.iter().filter(|v| **v).count());
    }
    unsafe { dr_image_free(flat) };
}

#[test]
fn the_warning_overlay_is_the_chosen_colour_and_only_there() {
    let Some(proof_bytes) = proof_bytes() else {
        eprintln!("skipping: {FOGRA} is not installed");
        return;
    };
    let proof = ProofProfile::parse(&proof_bytes).expect("parses");
    let document_profile = IccProfile::srgb();
    let document = srgb_bytes();
    let source = [
        [0u8, 255, 0, 255],
        [128, 128, 128, 255],
        [255, 0, 255, 255],
        [90, 90, 92, 255],
    ];
    let flat = image_of(&source);
    let (plain, _) = render(flat, &document, &proof_bytes, "{}");
    let (warned, info) = render(
        flat,
        &document,
        &proof_bytes,
        "{\"gamut_warning\":true,\"gamut_color\":\"#FF00FF\"}",
    );
    unsafe { dr_image_free(flat) };
    let mut flagged = 0;
    for (i, px) in source.iter().enumerate() {
        let outside = out_of_gamut(&proof, &document_profile, [px[0], px[1], px[2]]);
        if outside {
            flagged += 1;
            assert_eq!(&warned[i * 4..i * 4 + 3], &[255, 0, 255], "overlay at {i}");
        } else {
            assert_eq!(
                &warned[i * 4..i * 4 + 3],
                &plain[i * 4..i * 4 + 3],
                "an in-gamut pixel is untouched at {i}"
            );
        }
    }
    assert!(
        flagged > 0,
        "the fixture must contain an out-of-gamut colour"
    );
    assert_eq!(info["out_of_gamut"].as_u64().unwrap() as usize, flagged);
}

#[test]
fn simulating_paper_white_and_ink_black_move_the_ends() {
    let Some(proof) = proof_bytes() else {
        eprintln!("skipping: {FOGRA} is not installed");
        return;
    };
    let document = srgb_bytes();
    let source = [[255u8, 255, 255, 255], [0, 0, 0, 255]];
    let flat = image_of(&source);
    let (plain, _) = render(flat, &document, &proof, "{}");
    let (paper, _) = render(flat, &document, &proof, "{\"simulate_paper_white\":true}");
    let (ink, _) = render(flat, &document, &proof, "{\"simulate_ink_black\":true}");
    unsafe { dr_image_free(flat) };
    // Paper white is DARKER and warmer than the display's white.
    let plain_white: i32 = plain[0..3].iter().map(|&c| i32::from(c)).sum();
    let paper_white: i32 = paper[0..3].iter().map(|&c| i32::from(c)).sum();
    assert!(
        paper_white < plain_white,
        "simulating the paper must dim the white: {:?} against {:?}",
        &paper[0..3],
        &plain[0..3]
    );
    // Ink black is LIGHTER than the compensated black: turning BPC's final
    // mapping off is what exposes the medium's real darkest tone.
    let plain_black: i32 = plain[4..7].iter().map(|&c| i32::from(c)).sum();
    let ink_black: i32 = ink[4..7].iter().map(|&c| i32::from(c)).sum();
    assert!(
        ink_black > plain_black,
        "simulating the ink must lift the black: {:?} against {:?}",
        &ink[4..7],
        &plain[4..7]
    );
}

#[test]
fn absolute_colorimetric_puts_the_source_white_back() {
    // Absolute must do more than pick relative's table: it scales by the two
    // media whites, so a proof of WHITE under absolute is not the proof of
    // white under relative on a medium whose paper is not D50.
    let Some(proof_bytes) = proof_bytes() else {
        eprintln!("skipping: {FOGRA} is not installed");
        return;
    };
    let proof = ProofProfile::parse(&proof_bytes).expect("parses");
    let white = proof.media_white();
    assert!(
        white[1] < 1.0,
        "the fixture's paper is not the PCS white: {white:?}"
    );
    let document = srgb_bytes();
    let source = [
        [255u8, 255, 255, 255],
        [200, 190, 180, 255],
        [64, 96, 128, 255],
    ];
    let flat = image_of(&source);
    let (relative, _) = render(flat, &document, &proof_bytes, "{\"intent\":\"relative\"}");
    let (absolute, _) = render(flat, &document, &proof_bytes, "{\"intent\":\"absolute\"}");
    unsafe { dr_image_free(flat) };
    assert_ne!(
        &relative[..12],
        &absolute[..12],
        "absolute colorimetric must differ from relative on a non-D50 medium"
    );
}

#[test]
fn a_proof_touches_nothing_it_was_given() {
    let Some(proof) = proof_bytes() else {
        eprintln!("skipping: {FOGRA} is not installed");
        return;
    };
    let document = srgb_bytes();
    let source = [[200u8, 30, 40, 255], [10, 200, 90, 128]];
    let flat = image_of(&source);
    let before = img_pixels(flat);
    let _ = render(flat, &document, &proof, "{\"gamut_warning\":true}");
    let after = img_pixels(flat);
    assert_eq!(before, after, "the source image is immutable");
    unsafe { dr_image_free(flat) };
}

// ------------------------------------------------------------ refusals --

#[test]
fn a_profile_that_cannot_be_proofed_through_says_why() {
    let document = srgb_bytes();
    let flat = image_of(&[[128u8, 128, 128, 255]]);
    // sRGB itself: a matrix/TRC profile with no A2B/B2A tables at all.
    let mut info: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let setup = setup("{}");
    let out = unsafe {
        dr_proof_render(
            flat,
            document.as_ptr(),
            document.len(),
            document.as_ptr(),
            document.len(),
            setup.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut info,
            &mut err,
        )
    };
    assert!(out.is_null());
    let message = take_err_string(err);
    assert!(
        message.contains("table") || message.contains("A2B"),
        "the refusal must name the element: {message}"
    );
    if !info.is_null() {
        unsafe { dr_string_free(info) };
    }
    unsafe { dr_image_free(flat) };
}

#[test]
fn an_unknown_setup_key_is_refused_rather_than_ignored() {
    let Some(proof) = proof_bytes() else { return };
    let document = srgb_bytes();
    let flat = image_of(&[[128u8, 128, 128, 255]]);
    let setup = setup("{\"intnet\":\"perceptual\"}");
    let mut info: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_proof_render(
            flat,
            document.as_ptr(),
            document.len(),
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut info,
            &mut err,
        )
    };
    assert!(out.is_null());
    assert!(take_err_string(err).contains("intnet"));
    if !info.is_null() {
        unsafe { dr_string_free(info) };
    }
    unsafe { dr_image_free(flat) };
}

#[test]
fn a_mask_buffer_of_the_wrong_length_is_refused() {
    let Some(proof) = proof_bytes() else { return };
    let document = srgb_bytes();
    let flat = image_of(&[[1u8, 2, 3, 255], [4, 5, 6, 255]]);
    let setup = setup("{}");
    let mut mask = [0u8; 5];
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_proof_gamut_mask(
            flat,
            document.as_ptr(),
            document.len(),
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            mask.as_mut_ptr(),
            mask.len(),
            ptr::null_mut(),
            &mut err,
        )
    };
    assert!(!ok);
    assert!(take_err_string(err).contains("length"));
    unsafe { dr_image_free(flat) };
}

#[test]
fn every_proof_export_tolerates_null() {
    let mut err: *mut c_char = ptr::null_mut();
    let mut info: *mut c_char = ptr::null_mut();
    let mut count = 0u64;
    unsafe {
        assert!(dr_proof_render(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            ptr::null(),
            &mut info,
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(dr_proof_render(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut()
        )
        .is_null());

        assert!(!dr_proof_gamut_mask(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
            0,
            &mut count,
            &mut err
        ));
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(!dr_proof_gamut_mask(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut()
        ));

        assert!(dr_precision_proof_render(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            ptr::null(),
            &mut info,
            &mut err
        )
        .is_null());
        dr_string_free(err);
        err = ptr::null_mut();
        assert!(!dr_precision_proof_gamut_mask(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut(),
            0,
            &mut count,
            &mut err
        ));
        dr_string_free(err);
        err = ptr::null_mut();

        assert!(dr_icc_describe(ptr::null(), 0, &mut err).is_null());
        dr_string_free(err);
        assert!(dr_icc_describe(ptr::null(), 0, ptr::null_mut()).is_null());
    }
    if !info.is_null() {
        unsafe { dr_string_free(info) };
    }
}

#[test]
fn describe_names_every_field_a_host_shows() {
    let srgb = srgb_bytes();
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_icc_describe(srgb.as_ptr(), srgb.len(), &mut err) };
    assert!(!json.is_null());
    assert!(err.is_null());
    let text = take_err_string(json);
    let value: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(value["space"], "rgb");
    assert_eq!(value["model"], "matrix");
    assert_eq!(value["can_decode"], true);
    assert_eq!(value["can_encode"], true);
    // sRGB has no A2B/B2A tables, so it cannot be PROOFED through, and the
    // refusal names the element.
    assert_eq!(value["proofable"], false);
    assert!(value["refusal"].is_string());
    assert!(value["media_white"].is_array());

    if let Some(proof) = proof_bytes() {
        let json = unsafe { dr_icc_describe(proof.as_ptr(), proof.len(), ptr::null_mut()) };
        assert!(!json.is_null());
        let value: serde_json::Value = serde_json::from_str(&take_err_string(json)).expect("JSON");
        assert_eq!(value["space"], "cmyk");
        assert_eq!(value["model"], "lut");
        assert_eq!(value["proofable"], true);
        assert!(value["refusal"].is_null());
    }
}

// ------------------------------------------------------- typed admission --

/// macOS's own CMYK LUT profile. sRGB cannot stand in: it has no A2B/B2A
/// tables, so a proof through it is refused before any pixel is read.
const GENERIC_CMYK: &str = "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc";

/// A u16 fixture of at most 64 distinct colours, no channel a ×257 byte code.
fn typed_fixture(w: u32, h: u32) -> PrecisionImage {
    let code = |j: u32| (1 + 1021 * (j % 64)) as u16;
    let pixels = (0..w * h)
        .map(|i| [code(i), code(i * 5), code(i * 11), 65535])
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

/// The colour memo's peak, from the rule's words: at most 2^20 entries, held
/// in hashbrown's bucket count for that many (4 below 4, 8 below 8, else the
/// next power of two of n·8/7); each bucket one `(u64, ([f32; 3], f32))`
/// entry plus one control byte; ×3/2 because the old table is live while the
/// last grow rehashes; +64 for the two tables' control-group tails.
fn memo_oracle(pixels: u64) -> u64 {
    let n = pixels.min(1 << 20);
    let buckets = if n < 4 {
        4
    } else if n < 8 {
        8
    } else {
        (n * 8 / 7).next_power_of_two()
    };
    let bucket = std::mem::size_of::<(u64, ([f32; 3], f32))>() as u64 + 1;
    buckets * bucket * 3 / 2 + 64
}

fn typed_setup() -> CString {
    setup(r#"{"gamut_warning":true}"#)
}

/// The typed render's pixels and info, or its error.
fn typed_render(
    image: &PrecisionImage,
    proof: &[u8],
    limit: u64,
    other: u64,
) -> Result<(Vec<u8>, String), String> {
    let setup = typed_setup();
    let mut info: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_proof_render(
            image,
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            limit,
            other,
            ptr::null(),
            &mut info,
            &mut err,
        )
    };
    if out.is_null() {
        assert!(info.is_null());
        return Err(take_err_string(err));
    }
    let (w, h) = unsafe { (dr_image_width(out), dr_image_height(out)) };
    let bytes = unsafe {
        std::slice::from_raw_parts(dr_image_pixels_rgba(out), w as usize * h as usize * 4)
    }
    .to_vec();
    unsafe { dr_image_free(out) };
    Ok((bytes, take_err_string(info)))
}

/// The typed gamut mask into caller buffers.
fn typed_mask(
    image: &PrecisionImage,
    proof: &[u8],
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    out: &mut [u8],
    count: &mut u64,
) -> Result<(), String> {
    let setup = typed_setup();
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_proof_gamut_mask(
            image,
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            limit,
            other,
            control,
            out.as_mut_ptr(),
            out.len(),
            count,
            &mut err,
        )
    };
    if ok {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

/// The parked-cancel render: the mask on the probe's control.
fn masked<'a>(
    image: &'a PrecisionImage,
    proof: &'a [u8],
    out: &'a mut [u8],
    count: &'a mut u64,
) -> impl FnOnce(usize) -> (bool, String) + Send + 'a {
    move |token| {
        let control = token as *const DrJobControl;
        match typed_mask(
            image,
            proof,
            REFERENCE_WORKING_BYTES,
            0,
            control,
            out,
            count,
        ) {
            Ok(()) => (true, String::new()),
            Err(e) => (false, e),
        }
    }
}

const SENTINEL: u8 = 0x5A;
const COUNT_SENTINEL: u64 = u64::MAX;

#[test]
fn the_typed_proof_exports_admit_their_scratch_against_the_working_limit() {
    let Ok(proof) = std::fs::read(GENERIC_CMYK) else {
        eprintln!("skipping: {GENERIC_CMYK} is not installed");
        return;
    };
    assert_eq!(memo_oracle(1100 * 1000), 78_643_264);
    let profile_bytes = ProofProfile::parse(&proof).expect("proof profile").bytes();
    for (w, h) in [(64u32, 64u32), (1100, 1000)] {
        let image = typed_fixture(w, h);
        let pixels = u64::from(w) * u64::from(h);
        let need = image.retained_bytes() + pixels * 5 + profile_bytes + memo_oracle(pixels);

        let reference = typed_render(&image, &proof, REFERENCE_WORKING_BYTES, 0).expect("render");
        let e = typed_render(&image, &proof, need - 1, 0).unwrap_err();
        assert!(e.contains("working limit"), "{w}×{h} render: {e}");
        let at = typed_render(&image, &proof, need, 0).expect("render at need");
        assert!(at == reference, "{w}×{h}: the render at the limit differs");
        let e = typed_render(&image, &proof, need, 1).unwrap_err();
        assert!(e.contains("working limit"), "{w}×{h} render + other: {e}");
        let e = typed_render(&image, &proof, 0, 0).unwrap_err();
        assert!(
            e.contains("precision working limit must be positive"),
            "{e}"
        );

        let n = pixels as usize;
        let (mut expected, mut expected_count) = (vec![SENTINEL; n], COUNT_SENTINEL);
        typed_mask(
            &image,
            &proof,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut expected,
            &mut expected_count,
        )
        .expect("mask");
        for (limit, other, refusal) in [
            (need - 1, 0, "working limit"),
            (need, 1, "working limit"),
            (0, 0, "precision working limit must be positive"),
        ] {
            let (mut out, mut count) = (vec![SENTINEL; n], COUNT_SENTINEL);
            let e = typed_mask(
                &image,
                &proof,
                limit,
                other,
                ptr::null(),
                &mut out,
                &mut count,
            )
            .unwrap_err();
            assert!(e.contains(refusal), "{w}×{h} mask at {limit}/{other}: {e}");
            assert!(out.iter().all(|&b| b == SENTINEL), "a refused mask wrote");
            assert_eq!(count, COUNT_SENTINEL, "a refused mask wrote its count");
        }
        let (mut out, mut count) = (vec![SENTINEL; n], COUNT_SENTINEL);
        typed_mask(&image, &proof, need, 0, ptr::null(), &mut out, &mut count)
            .expect("mask at need");
        assert!(out == expected, "{w}×{h}: the mask at the limit differs");
        assert_eq!(count, expected_count);
    }
}

#[test]
fn a_cancel_parked_inside_the_typed_gamut_mask_returns_nothing_and_holds_nothing() {
    let Ok(proof) = std::fs::read(GENERIC_CMYK) else {
        eprintln!("skipping: {GENERIC_CMYK} is not installed");
        return;
    };
    // 256×256 = 16 of the loop's 4096-pixel checkpoints, after `begin`'s
    // and the reserve's.
    let image = typed_fixture(256, 256);
    let n = 256 * 256;
    let (mut expected, mut expected_count) = (vec![SENTINEL; n], COUNT_SENTINEL);
    typed_mask(
        &image,
        &proof,
        REFERENCE_WORKING_BYTES,
        0,
        ptr::null(),
        &mut expected,
        &mut expected_count,
    )
    .expect("mask");

    let (mut out, mut count) = (vec![SENTINEL; n], COUNT_SENTINEL);
    let n0 = entry_count(
        &|_, stage, _| stage == 1,
        masked(&image, &proof, &mut out, &mut count),
    );
    let (mut out, mut count) = (vec![SENTINEL; n], COUNT_SENTINEL);
    cancelled_parked_at(
        n0 + 1 + 5,
        &|_, stage| stage == 1,
        masked(&image, &proof, &mut out, &mut count),
    );
    assert!(out.iter().all(|&b| b == SENTINEL), "a cancelled mask wrote");
    assert_eq!(count, COUNT_SENTINEL, "a cancelled mask wrote its count");

    let control = unsafe { dr_job_control_create() };
    let (mut out, mut count) = (vec![SENTINEL; n], COUNT_SENTINEL);
    let result = typed_mask(
        &image,
        &proof,
        REFERENCE_WORKING_BYTES,
        0,
        control,
        &mut out,
        &mut count,
    );
    let scratch = unsafe { dr_job_control_scratch_bytes(control) };
    unsafe { dr_job_control_free(control) };
    result.expect("a fresh control completes");
    assert!(out == expected, "the completed mask differs");
    assert_eq!(count, expected_count);
    assert_eq!(scratch, 0);
}

// --------------------------------------------------------- byte admission --

/// A byte fixture of at most 64 distinct colours, through the public entry
/// point.
fn byte_fixture(w: u32, h: u32) -> *mut DrImage {
    let code = |j: u32| (3 + 4 * (j % 64)) as u8;
    let raw: Vec<u8> = (0..w * h)
        .flat_map(|i| [code(i), code(i * 5), code(i * 11), 255])
        .collect();
    let handle = unsafe { dr_image_from_rgba(raw.as_ptr(), w, h) };
    assert!(!handle.is_null());
    handle
}

/// The byte render's pixels and info, or its error.
fn byte_render(
    image: *const DrImage,
    document: &[u8],
    proof: &[u8],
    limit: u64,
    other: u64,
) -> Result<(Vec<u8>, String), String> {
    let setup = typed_setup();
    let mut info: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_proof_render(
            image,
            document.as_ptr(),
            document.len(),
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            limit,
            other,
            ptr::null(),
            &mut info,
            &mut err,
        )
    };
    if out.is_null() {
        assert!(info.is_null());
        return Err(take_err_string(err));
    }
    let pixels = img_pixels(out);
    unsafe { dr_image_free(out) };
    Ok((pixels, take_err_string(info)))
}

/// The byte gamut mask into caller buffers.
#[allow(clippy::too_many_arguments)]
fn byte_mask(
    image: *const DrImage,
    document: &[u8],
    proof: &[u8],
    limit: u64,
    other: u64,
    control: *const DrJobControl,
    out: &mut [u8],
    count: &mut u64,
) -> Result<(), String> {
    let setup = typed_setup();
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_proof_gamut_mask(
            image,
            document.as_ptr(),
            document.len(),
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            limit,
            other,
            control,
            out.as_mut_ptr(),
            out.len(),
            count,
            &mut err,
        )
    };
    if ok {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

#[test]
fn the_byte_proof_exports_admit_their_scratch_against_the_working_limit() {
    let Ok(proof) = std::fs::read(GENERIC_CMYK) else {
        eprintln!("skipping: {GENERIC_CMYK} is not installed");
        return;
    };
    let document = srgb_bytes();
    let profile_bytes = ProofProfile::parse(&proof).expect("proof profile").bytes();
    for (w, h) in [(64u32, 64u32), (1100, 1000)] {
        let image = byte_fixture(w, h);
        let pixels = u64::from(w) * u64::from(h);
        let need = pixels * 4
            + pixels * 5
            + profile_bytes
            + 10 * document.len() as u64
            + memo_oracle(pixels);

        let reference =
            byte_render(image, &document, &proof, REFERENCE_WORKING_BYTES, 0).expect("render");
        let e = byte_render(image, &document, &proof, need - 1, 0).unwrap_err();
        assert!(e.contains("working limit"), "{w}×{h} render: {e}");
        let at = byte_render(image, &document, &proof, need, 0).expect("render at need");
        assert!(at == reference, "{w}×{h}: the render at the limit differs");
        let e = byte_render(image, &document, &proof, need, 1).unwrap_err();
        assert!(e.contains("working limit"), "{w}×{h} render + other: {e}");
        let e = byte_render(image, &document, &proof, 0, 0).unwrap_err();
        assert!(e.contains("positive"), "{e}");

        let n = pixels as usize;
        let (mut expected, mut expected_count) = (vec![SENTINEL; n], COUNT_SENTINEL);
        byte_mask(
            image,
            &document,
            &proof,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut expected,
            &mut expected_count,
        )
        .expect("mask");
        for (limit, other, refusal) in [
            (need - 1, 0, "working limit"),
            (need, 1, "working limit"),
            (0, 0, "positive"),
        ] {
            let (mut out, mut count) = (vec![SENTINEL; n], COUNT_SENTINEL);
            let e = byte_mask(
                image,
                &document,
                &proof,
                limit,
                other,
                ptr::null(),
                &mut out,
                &mut count,
            )
            .unwrap_err();
            assert!(e.contains(refusal), "{w}×{h} mask at {limit}/{other}: {e}");
            assert!(out.iter().all(|&b| b == SENTINEL), "a refused mask wrote");
            assert_eq!(count, COUNT_SENTINEL, "a refused mask wrote its count");
        }
        let (mut out, mut count) = (vec![SENTINEL; n], COUNT_SENTINEL);
        byte_mask(
            image,
            &document,
            &proof,
            need,
            0,
            ptr::null(),
            &mut out,
            &mut count,
        )
        .expect("mask at need");
        assert!(out == expected, "{w}×{h}: the mask at the limit differs");
        assert_eq!(count, expected_count);
        unsafe { dr_image_free(image) };
    }
}

/// The parked-cancel byte render: the mask on the probe's control. The image
/// travels as an address because a raw handle is not `Send`.
fn byte_masked<'a>(
    image: usize,
    document: &'a [u8],
    proof: &'a [u8],
    out: &'a mut [u8],
    count: &'a mut u64,
) -> impl FnOnce(usize) -> (bool, String) + Send + 'a {
    move |token| {
        let control = token as *const DrJobControl;
        let image = image as *const DrImage;
        match byte_mask(
            image,
            document,
            proof,
            REFERENCE_WORKING_BYTES,
            0,
            control,
            out,
            count,
        ) {
            Ok(()) => (true, String::new()),
            Err(e) => (false, e),
        }
    }
}

#[test]
fn a_cancel_parked_inside_the_byte_gamut_mask_returns_nothing_and_holds_nothing() {
    let Ok(proof) = std::fs::read(GENERIC_CMYK) else {
        eprintln!("skipping: {GENERIC_CMYK} is not installed");
        return;
    };
    let document = srgb_bytes();
    // 256×256 = 16 of the loop's 4096-pixel checkpoints, after `begin`'s
    // and the reserve's.
    let image = byte_fixture(256, 256);
    let address = image as usize;
    let n = 256 * 256;
    let (mut expected, mut expected_count) = (vec![SENTINEL; n], COUNT_SENTINEL);
    byte_mask(
        image,
        &document,
        &proof,
        REFERENCE_WORKING_BYTES,
        0,
        ptr::null(),
        &mut expected,
        &mut expected_count,
    )
    .expect("mask");

    let (mut out, mut count) = (vec![SENTINEL; n], COUNT_SENTINEL);
    let n0 = entry_count(
        &|_, stage, _| stage == 1,
        byte_masked(address, &document, &proof, &mut out, &mut count),
    );
    let (mut out, mut count) = (vec![SENTINEL; n], COUNT_SENTINEL);
    cancelled_parked_at(
        n0 + 1 + 5,
        &|_, stage| stage == 1,
        byte_masked(address, &document, &proof, &mut out, &mut count),
    );
    assert!(out.iter().all(|&b| b == SENTINEL), "a cancelled mask wrote");
    assert_eq!(count, COUNT_SENTINEL, "a cancelled mask wrote its count");

    let control = unsafe { dr_job_control_create() };
    let (mut out, mut count) = (vec![SENTINEL; n], COUNT_SENTINEL);
    let result = byte_mask(
        image,
        &document,
        &proof,
        REFERENCE_WORKING_BYTES,
        0,
        control,
        &mut out,
        &mut count,
    );
    let scratch = unsafe { dr_job_control_scratch_bytes(control) };
    unsafe { dr_job_control_free(control) };
    unsafe { dr_image_free(image) };
    result.expect("a fresh control completes");
    assert!(out == expected, "the completed mask differs");
    assert_eq!(count, expected_count);
    assert_eq!(scratch, 0);
}

// ---------------------------------------------------- 16-bit display codes --

/// The proof profiles the depth tests run through: macOS's Generic CMYK
/// always, FOGRA39 as an extra run when it is installed. Each must describe
/// itself as proofable before a pixel is compared.
fn depth_profiles() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for path in [GENERIC_CMYK, FOGRA] {
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("skipping {path}: not installed");
            continue;
        };
        let json = unsafe { dr_icc_describe(bytes.as_ptr(), bytes.len(), ptr::null_mut()) };
        assert!(!json.is_null(), "{path} does not describe");
        let value: serde_json::Value =
            serde_json::from_str(&take_err_string(json)).expect("describe is JSON");
        assert_eq!(value["proofable"], true, "{path} is not proofable");
        out.push(bytes);
    }
    out
}

/// The typed render's pixels and info under `json`.
fn typed_render_json(
    image: &PrecisionImage,
    proof: &[u8],
    json: &str,
    control: *const DrJobControl,
) -> Result<(Vec<u8>, String), String> {
    let setup = setup(json);
    let mut info: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_proof_render(
            image,
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            control,
            &mut info,
            &mut err,
        )
    };
    if out.is_null() {
        assert!(info.is_null());
        return Err(take_err_string(err));
    }
    let pixels = img_pixels(out);
    unsafe { dr_image_free(out) };
    Ok((pixels, take_err_string(info)))
}

/// The typed gamut mask under `json`, into `out`.
fn typed_mask_json(
    image: &PrecisionImage,
    proof: &[u8],
    json: &str,
    out: &mut [u8],
    count: &mut u64,
) -> Result<(), String> {
    let setup = setup(json);
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_proof_gamut_mask(
            image,
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            out.as_mut_ptr(),
            out.len(),
            count,
            &mut err,
        )
    };
    if ok {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

/// The byte render's pixels and info under `json`, with sRGB as the
/// document profile.
fn byte_render_json(image: *const DrImage, proof: &[u8], json: &str) -> (Vec<u8>, String) {
    let document = srgb_bytes();
    let setup = setup(json);
    let mut info: *mut c_char = ptr::null_mut();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_proof_render(
            image,
            document.as_ptr(),
            document.len(),
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut info,
            &mut err,
        )
    };
    if out.is_null() {
        panic!("byte proof render failed: {}", take_err_string(err));
    }
    let pixels = img_pixels(out);
    unsafe { dr_image_free(out) };
    (pixels, take_err_string(info))
}

/// The byte gamut mask under `json`.
fn byte_mask_json(image: *const DrImage, proof: &[u8], json: &str, n: usize) -> (Vec<u8>, u64) {
    let document = srgb_bytes();
    let setup = setup(json);
    let mut out = vec![SENTINEL; n];
    let mut count = COUNT_SENTINEL;
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_proof_gamut_mask(
            image,
            document.as_ptr(),
            document.len(),
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            out.as_mut_ptr(),
            out.len(),
            &mut count,
            &mut err,
        )
    };
    assert!(ok, "byte gamut mask failed: {}", take_err_string(err));
    (out, count)
}

/// Byte RGBA pixels as a public byte handle.
fn byte_handle(w: u32, h: u32, raw: &[u8]) -> *mut DrImage {
    let handle = unsafe { dr_image_from_rgba(raw.as_ptr(), w, h) };
    assert!(!handle.is_null());
    handle
}

#[test]
fn a_times_257_typed_proof_is_the_byte_proof_of_its_codes() {
    let profiles = depth_profiles();
    if profiles.is_empty() {
        return;
    }
    // 32×16 codes spanning the cube, the saturated primaries (out of any
    // press gamut) and a few partial alphas.
    let (w, h) = (32u32, 16u32);
    let mut bytes: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
    for i in 0..w * h {
        let px: [u8; 4] = match i {
            0 => [255, 0, 0, 255],
            1 => [0, 255, 0, 255],
            2 => [0, 0, 255, 255],
            3 => [0, 255, 255, 128],
            _ => [
                (i * 37 % 256) as u8,
                (i * 91 % 256) as u8,
                (i * 13 % 256) as u8,
                if i % 7 == 0 { (i % 256) as u8 } else { 255 },
            ],
        };
        bytes.extend_from_slice(&px);
    }
    let wide: Vec<[u16; 4]> = bytes
        .chunks_exact(4)
        .map(|p| std::array::from_fn(|c| u16::from(p[c]) * 257))
        .collect();
    let typed = PrecisionImage::from_rgba16(w, h, wide, IccProfile::srgb(), None).unwrap();
    let flat = byte_handle(w, h, &bytes);
    let n = (w * h) as usize;
    let mut compared = 0;
    for proof in &profiles {
        for intent in ["perceptual", "relative", "saturation", "absolute"] {
            for bpc in [false, true] {
                for (paper, ink) in [(false, false), (true, false), (false, true), (true, true)] {
                    for warning in [false, true] {
                        let json = format!(
                            r##"{{"intent":"{intent}","black_point_compensation":{bpc},"simulate_paper_white":{paper},"simulate_ink_black":{ink},"gamut_warning":{warning},"gamut_color":"#11EE22"}}"##
                        );
                        let byte = byte_render_json(flat, proof, &json);
                        let typed_out =
                            typed_render_json(&typed, proof, &json, ptr::null()).expect("typed");
                        assert!(typed_out.0 == byte.0, "{json}: the pixels differ");
                        assert_eq!(typed_out.1, byte.1, "{json}: the info differs");

                        let (byte_mask, byte_count) = byte_mask_json(flat, proof, &json, n);
                        let (mut mask, mut count) = (vec![SENTINEL; n], COUNT_SENTINEL);
                        typed_mask_json(&typed, proof, &json, &mut mask, &mut count)
                            .expect("typed mask");
                        assert!(mask == byte_mask, "{json}: the masks differ");
                        assert_eq!(count, byte_count, "{json}: the counts differ");
                        compared += 1;
                    }
                }
            }
        }
    }
    assert_eq!(compared, profiles.len() * 64);
    unsafe { dr_image_free(flat) };
}

#[test]
fn a_typed_ramp_between_byte_codes_is_proofed_at_full_depth() {
    let profiles = depth_profiles();
    if profiles.is_empty() {
        return;
    }
    // A grey ramp of every 15th code, none of them a ×257 byte code.
    let ramp: Vec<u16> = (1..=65535u32)
        .step_by(15)
        .map(|v| v as u16)
        .filter(|v| v % 257 != 0)
        .collect();
    assert!(ramp.len() >= 4096, "the ramp has {} steps", ramp.len());
    let w = ramp.len() as u32;
    let wide: Vec<[u16; 4]> = ramp.iter().map(|&v| [v, v, v, 65535]).collect();
    let typed = PrecisionImage::from_rgba16(w, 1, wide, IccProfile::srgb(), None).unwrap();
    let rounded: Vec<u8> = ramp
        .iter()
        .flat_map(|&v| {
            let c = ((f64::from(v) / 257.0).round()) as u8;
            [c, c, c, 255]
        })
        .collect();
    let truncated: Vec<u8> = ramp
        .iter()
        .flat_map(|&v| {
            let c = (v >> 8) as u8;
            [c, c, c, 255]
        })
        .collect();
    for proof in &profiles {
        let (typed_out, _) = typed_render_json(&typed, proof, "{}", ptr::null()).expect("typed");
        let sums: Vec<u32> = typed_out
            .chunks_exact(4)
            .map(|p| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]))
            .collect();
        for (i, pair) in sums.windows(2).enumerate() {
            assert!(
                pair[1] >= pair[0],
                "R+G+B falls from {} to {} at ramp step {} (code {})",
                pair[0],
                pair[1],
                i + 1,
                ramp[i + 1]
            );
        }
        for (name, codes) in [("round(v/257)", &rounded), ("v >> 8", &truncated)] {
            let flat = byte_handle(w, 1, codes);
            let (byte_out, _) = byte_render_json(flat, proof, "{}");
            unsafe { dr_image_free(flat) };
            assert!(
                byte_out != typed_out,
                "the typed ramp's proof equals the byte proof of its {name} codes"
            );
        }
    }
}

#[test]
fn the_typed_proof_exports_refuse_float_storage() {
    let profiles = depth_profiles();
    let Some(proof) = profiles.first() else {
        return;
    };
    let pixels = vec![[0.25f32, 0.5, 0.75, 1.0]; 16];
    let float = PrecisionImage::from_rgba32(4, 4, pixels, IccProfile::srgb(), None).unwrap();
    let e = typed_render_json(&float, proof, "{}", ptr::null()).unwrap_err();
    assert!(e.contains("RGBA16"), "{e}");
    let (mut out, mut count) = (vec![SENTINEL; 16], COUNT_SENTINEL);
    let e = typed_mask_json(&float, proof, "{}", &mut out, &mut count).unwrap_err();
    assert!(e.contains("RGBA16"), "{e}");
    assert!(out.iter().all(|&b| b == SENTINEL), "a refused mask wrote");
    assert_eq!(count, COUNT_SENTINEL, "a refused mask wrote its count");
}

#[test]
fn a_typed_mask_buffer_of_the_wrong_length_is_refused() {
    let profiles = depth_profiles();
    let Some(proof) = profiles.first() else {
        return;
    };
    let image = typed_fixture(4, 4);
    for len in [15usize, 17] {
        let (mut out, mut count) = (vec![SENTINEL; len], COUNT_SENTINEL);
        let e = typed_mask_json(&image, proof, "{}", &mut out, &mut count).unwrap_err();
        assert!(e.contains("length"), "{len}: {e}");
        assert!(out.iter().all(|&b| b == SENTINEL), "a refused mask wrote");
        assert_eq!(count, COUNT_SENTINEL, "a refused mask wrote its count");
    }
}

/// The canvas's 16-bit proof at 24 MP: the projection's display codes, then
/// the byte proof of them. The bound is declared here, before measuring;
/// the exact typed proof's time is printed as information only.
#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (display-code proof)"]
fn display_code_proof_throughput_at_24_mp() {
    const BOUND_S: f64 = 2.0;
    let Ok(proof) = std::fs::read(GENERIC_CMYK) else {
        eprintln!("skipping: {GENERIC_CMYK} is not installed");
        return;
    };
    let (w, h) = (6000u32, 4000u32);
    let pixels: Vec<[u16; 4]> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                let r = (u64::from(x) * 65535 / u64::from(w - 1)) as u16;
                let g = (u64::from(y) * 65535 / u64::from(h - 1)) as u16;
                let b = (u64::from(x + y) * 65535 / u64::from(w + h - 2)) as u16;
                [r, g, b, 65535]
            })
        })
        .collect();
    let typed = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    let document = srgb_bytes();
    let setup = setup("{}");

    let start = std::time::Instant::now();
    let mut err: *mut c_char = ptr::null_mut();
    let display = unsafe {
        dr_precision_image_preview_rgba8(&typed, REFERENCE_WORKING_BYTES, 0, ptr::null(), &mut err)
    };
    assert!(!display.is_null(), "preview: {}", take_err_string(err));
    let codes = start.elapsed();
    let mut info: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_proof_render(
            display,
            document.as_ptr(),
            document.len(),
            proof.as_ptr(),
            proof.len(),
            setup.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut info,
            &mut err,
        )
    };
    let total = start.elapsed();
    assert!(!out.is_null(), "byte proof: {}", take_err_string(err));
    take_err_string(info);
    unsafe {
        dr_image_free(out);
        dr_image_free(display);
    }
    let proofed = total - codes;
    eprintln!(
        "display-code proof at {w}×{h}: display codes {:.3} s + byte proof {:.3} s = \
         {:.3} s; bound {BOUND_S:.1} s; {}",
        codes.as_secs_f64(),
        proofed.as_secs_f64(),
        total.as_secs_f64(),
        if total.as_secs_f64() <= BOUND_S {
            "PASS"
        } else {
            "MISS"
        }
    );

    let start = std::time::Instant::now();
    typed_render_json(&typed, &proof, "{}", ptr::null()).expect("exact typed proof");
    eprintln!(
        "exact typed proof at {w}×{h}: {:.3} s (information only)",
        start.elapsed().as_secs_f64()
    );
}
