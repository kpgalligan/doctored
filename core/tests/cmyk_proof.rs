//! A CMYK document's soft proof and the one-colour separation (#1 phase
//! 5), through the FFI, against the test's own chain written from the two
//! synthetic presses' tables (`common/cmyk_fixture.rs`): Preserve CMYK
//! Numbers on and off, Simulate Black Ink and Paper Color on and off, a plate, the info's
//! `"display-p3"`; Gamut Warning against the proof press, its count restated
//! as an independent ΔE76 over the fixture's tables; the gamut verdict and
//! the in-gamut colour of `separate`
//! against the fixture's Lab round trip; the RGB proof's new keys; and
//! every refusal by its sentence. No test here sets or clears the process
//! working CMYK profile.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_string_free};
use doctored_core::ffi_cmyk_colour::*;
use doctored_core::ffi_output_color::dr_proof_render;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_open};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::DrImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::{
    bpc_to_zero, gamut_delta_srgb, ink_lab, ink_lab_b, ink_to_srgb, lab_to_rgb_in, lab_to_xyz,
    press_profile, press_profile_b, press_profile_b_on_paper, press_to_press_b, rgb_lut_profile,
    source_black_b, srgb_to_ink, xyz_to_lab, D50,
};
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, img_pixels, open_image, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn cmyk_doc(dir: &TempDir, w: u32, h: u32, samples: &[[u16; 4]]) -> *mut PrecisionDocument {
    let data: Vec<u8> = samples
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let path = dir.path().join("proof.tif");
    std::fs::write(
        &path,
        cmyk_tiff(w, h, 16, 4, 0, &data, Some(&press_profile("Press A"))),
    )
    .unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

fn ink_grid() -> Vec<[u16; 4]> {
    let level = |i: u16| i * 21845;
    let mut out = Vec::new();
    for c in 0..4u16 {
        for m in 0..4u16 {
            for y in 0..4u16 {
                for k in 0..4u16 {
                    out.push([level(c), level(m), level(y), level(k)]);
                }
            }
        }
    }
    out
}

/// The proof, or its refusal; the info JSON with the image.
fn proof(
    doc: *const PrecisionDocument,
    profile: &[u8],
    setup: &str,
) -> Result<(Vec<u8>, Value), String> {
    let text = CString::new(setup).unwrap();
    let mut info = ptr::null_mut();
    let mut err = ptr::null_mut();
    let image: *mut DrImage = unsafe {
        dr_precision_doc_cmyk_proof(
            doc,
            profile.as_ptr(),
            profile.len(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut info,
            &mut err,
        )
    };
    if image.is_null() {
        return Err(take_err_string(err));
    }
    let pixels = img_pixels(image);
    unsafe { dr_image_free(image) };
    let value = serde_json::from_str(unsafe { CStr::from_ptr(info) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(info) };
    Ok((pixels, value))
}

fn byte(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The paper white the proof press declares in its `wtpt` (exact in
/// s15Fixed16), a yellowish stock.
const PAPER: [f64; 3] = [0.875, 0.9375, 0.75];

/// The proof tail into the built-in Display P3: the second press's A2B
/// (relative), scaled by its media white over D50 when `paper`, then BPC
/// from its source black unless `ink_black`.
fn tail(device: [f64; 4], ink_black: bool, paper: bool) -> [u8; 3] {
    let mut xyz = lab_to_xyz(ink_lab_b(device));
    if paper {
        xyz = std::array::from_fn(|i| xyz[i] * PAPER[i] / D50[i]);
    }
    if !ink_black {
        xyz = bpc_to_zero(xyz, source_black_b());
    }
    lab_to_rgb_in(xyz_to_lab(xyz), None, &IccProfile::display_p3()).map(byte)
}

#[test]
fn the_cmyk_proof_is_the_tail_of_the_second_press() {
    let dir = TempDir::new().unwrap();
    let grid = ink_grid();
    let doc = cmyk_doc(&dir, 16, 16, &grid);
    let b = press_profile_b("Press B");
    let b_paper = press_profile_b_on_paper("Press B", PAPER);
    for (preserve, ink_black, paper, bpc) in
        (0..16).map(|i| (i & 1 != 0, i & 2 != 0, i & 4 != 0, i & 8 != 0))
    {
        let setup = format!(
            r#"{{"preserve_numbers":{preserve},"simulate_ink_black":{ink_black},"simulate_paper_white":{paper},"black_point_compensation":{bpc}}}"#
        );
        let (pixels, info) = proof(doc, &b_paper, &setup).unwrap();
        assert_eq!(info["space"], "display-p3");
        for (ink, got) in grid.iter().zip(pixels.chunks(4)) {
            let unit = ink.map(|v| f64::from(v) / 65535.0);
            let device = if preserve {
                unit
            } else {
                press_to_press_b(unit, false, bpc)
            };
            let expected = tail(device, ink_black, paper);
            for c in 0..3 {
                assert!(
                    got[c].abs_diff(expected[c]) <= 1,
                    "{setup} {ink:?}: channel {c} {} vs {}",
                    got[c],
                    expected[c]
                );
            }
            assert_eq!(got[3], 255);
        }
    }
    // A plate: the device's one ink, ink dark.
    let (pixels, _) = proof(doc, &b, r#"{"plate":"magenta"}"#).unwrap();
    for (ink, got) in grid.iter().zip(pixels.chunks(4)) {
        let device = press_to_press_b(ink.map(|v| f64::from(v) / 65535.0), false, true);
        let grey = 255 - (device[1].clamp(0.0, 1.0) * 255.0).round() as u8;
        assert!(got[0].abs_diff(grey) <= 1 && got[0] == got[1] && got[1] == got[2]);
        assert_eq!(got[3], 255);
    }
    let (pixels, _) = proof(doc, &b, r#"{"plate":"black","preserve_numbers":true}"#).unwrap();
    for (ink, got) in grid.iter().zip(pixels.chunks(4)) {
        let grey = 255 - (f64::from(ink[3]) / 65535.0 * 255.0).round() as u8;
        assert_eq!(got[..3], [grey; 3]);
    }
    unsafe { dr_precision_doc_free(doc) };
}

/// A neutral-only proof press whose tables are linear (so its two-point
/// grids are exact): A2B maps ink to a grey of L* = 100·(1 − mean ink),
/// a* = b* = 0; B2A sends normalized Lab to the four equal inks 1 − L.
/// Its round trip keeps a colour's lightness and drops its chroma, so it
/// holds the near-neutral inks of the document press and no others.
fn press_profile_neutral(name: &str) -> Vec<u8> {
    use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_wtpt};
    let a2b = tag_mft2(4, 3, 2, &|d: &[f64]| {
        let mean = (d[0] + d[1] + d[2] + d[3]) / 4.0;
        vec![
            65280.0 * (1.0 - mean) / 65535.0,
            32768.0 / 65535.0,
            32768.0 / 65535.0,
        ]
    });
    let b2a = tag_mft2(3, 4, 2, &|lab: &[f64]| vec![1.0 - lab[0]; 4]);
    build(
        [2, 0x10, 0, 0],
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[
            (b"desc", tag_desc(name)),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", a2b.clone()),
            (b"A2B1", a2b),
            (b"B2A0", b2a.clone()),
            (b"B2A1", b2a),
        ],
    )
}

/// The gamut rule restated from the two presses' formulas: the document
/// press's Lab of `ink` (its relative A2B) against the Lab of its round trip
/// through the neutral press — B2A1 to the equal inks `t = 1 − L` (L the
/// legacy-normalized lightness), then A2B1 to `L* = 100·(1 − t)`, a* = b* =
/// 0 — as CIE76, outside above 5 (the nearest grid values are 4.67 and
/// 6.59, far from the threshold).
fn outside_neutral(ink: [f64; 4]) -> bool {
    let lab = ink_lab(ink);
    let t = 1.0 - lab[0] / 100.0 * 65280.0 / 65535.0;
    let back = [100.0 * (1.0 - t), 0.0, 0.0];
    let delta =
        ((lab[0] - back[0]).powi(2) + (lab[1] - back[1]).powi(2) + (lab[2] - back[2]).powi(2))
            .sqrt();
    delta > 5.0
}

#[test]
fn gamut_warning_flags_the_inks_the_proof_press_cannot_hold() {
    let dir = TempDir::new().unwrap();
    let grid = ink_grid();
    let doc = cmyk_doc(&dir, 16, 16, &grid);
    let b = press_profile_neutral("Press N");
    let flagged: Vec<bool> = grid
        .iter()
        .map(|ink| outside_neutral(ink.map(|v| f64::from(v) / 65535.0)))
        .collect();
    let count = flagged.iter().filter(|f| **f).count();
    assert!(
        count > 0 && count < grid.len(),
        "the grid straddles the gamut: {count}"
    );
    let (shown, info) = proof(
        doc,
        &b,
        r##"{"gamut_warning":true,"gamut_color":"#FF00FF"}"##,
    )
    .unwrap();
    let (plain, plain_info) = proof(doc, &b, "{}").unwrap();
    assert_eq!(info["out_of_gamut"], count);
    // The count is the proof's whether or not the warning is shown.
    assert_eq!(plain_info["out_of_gamut"], count);
    for (i, outside) in flagged.iter().enumerate() {
        let (got, base) = (&shown[i * 4..i * 4 + 4], &plain[i * 4..i * 4 + 4]);
        if *outside {
            assert_eq!(got, [255, 0, 255, 255], "pixel {i} {:?}", grid[i]);
        } else {
            assert_eq!(got, base, "pixel {i} {:?}", grid[i]);
        }
    }
    // A plate takes the overlay too.
    let (plate, _) = proof(
        doc,
        &b,
        r##"{"gamut_warning":true,"gamut_color":"#00FF00","plate":"cyan"}"##,
    )
    .unwrap();
    for (i, outside) in flagged.iter().enumerate() {
        if *outside {
            assert_eq!(plate[i * 4..i * 4 + 4], [0, 255, 0, 255], "plate pixel {i}");
        }
    }
    // Preserving the numbers sends them to the press unconverted: nothing is
    // flagged.
    let (kept, kept_info) =
        proof(doc, &b, r#"{"gamut_warning":true,"preserve_numbers":true}"#).unwrap();
    let (kept_plain, _) = proof(doc, &b, r#"{"preserve_numbers":true}"#).unwrap();
    assert_eq!(kept_info["out_of_gamut"], 0);
    assert_eq!(kept, kept_plain);
    unsafe { dr_precision_doc_free(doc) };
}

#[test]
fn every_proof_refusal_names_itself() {
    let dir = TempDir::new().unwrap();
    let doc = cmyk_doc(&dir, 1, 1, &[[1000, 2000, 3000, 4000]]);
    let rgb_lut = rgb_lut_profile("RGB Proof");
    assert_eq!(
        proof(doc, &rgb_lut, "{}").unwrap_err(),
        "the proof profile \"RGB Proof\" is not CMYK; a CMYK document proofs through a CMYK profile"
    );
    assert_eq!(
        proof(doc, &rgb_lut, r#"{"plate":"cyan"}"#).unwrap_err(),
        "the proof profile \"RGB Proof\" is not CMYK; a CMYK document proofs through a CMYK profile"
    );
    let b = press_profile_b("Press B");
    assert_eq!(
        proof(doc, &b, r#"{"plate":"orange"}"#).unwrap_err(),
        "proof setup plate must be cyan, magenta, yellow or black"
    );
    // The plan answers the same refusal.
    let mut err = ptr::null_mut();
    let text = unsafe {
        dr_precision_doc_cmyk_proof_plan(
            doc,
            rgb_lut.as_ptr(),
            rgb_lut.len(),
            ptr::null(),
            &mut err,
        )
    };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let plan: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    assert!(plan["refusal"].as_str().unwrap().contains("is not CMYK"));
    assert!(plan["working_memory_plan_bytes"].as_u64().unwrap() > 0);
    unsafe { dr_precision_doc_free(doc) };
}

/// The RGB proof (`dr_proof_render`) of one 8-bit pixel in sRGB.
fn rgb_proof(profile: &[u8], setup: &str) -> Result<Vec<u8>, String> {
    let dir = TempDir::new().unwrap();
    let image =
        image::RgbaImage::from_raw(2, 1, vec![200, 40, 90, 255, 30, 160, 220, 255]).unwrap();
    let flat = open_image(&dir, "flat.png", &image);
    let doc = IccProfile::srgb().bytes().to_vec();
    let text = CString::new(setup).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_proof_render(
            flat,
            doc.as_ptr(),
            doc.len(),
            profile.as_ptr(),
            profile.len(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
            &mut err,
        )
    };
    unsafe { dr_image_free(flat) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let pixels = img_pixels(out);
    unsafe { dr_image_free(out) };
    Ok(pixels)
}

#[test]
fn the_rgb_proof_takes_a_plate_of_a_cmyk_proof_only() {
    let a = press_profile("Press A");
    // Without a plate the new keys change nothing.
    assert_eq!(
        rgb_proof(&a, "{}").unwrap(),
        rgb_proof(&a, r#"{"plate":null,"preserve_numbers":false}"#).unwrap()
    );
    // A plate is the proof device's ink: sRGB → the press's B2A1 (relative,
    // BPC onto its zero destination black, which sRGB's zero black makes
    // the identity).
    let plate = rgb_proof(&a, r#"{"plate":"cyan"}"#).unwrap();
    for (rgb, got) in [[200u8, 40, 90], [30, 160, 220]]
        .iter()
        .zip(plate.chunks(4))
    {
        let ink = srgb_to_ink(rgb.map(|v| f64::from(v) / 255.0));
        let grey = 255 - (ink[0].clamp(0.0, 1.0) * 255.0).round() as u8;
        assert!(got[0].abs_diff(grey) <= 1, "{rgb:?}: {} vs {grey}", got[0]);
        assert!(got[0] == got[1] && got[1] == got[2]);
    }
    let rgb_lut = rgb_lut_profile("RGB Proof");
    assert_eq!(
        rgb_proof(&rgb_lut, r#"{"plate":"cyan"}"#).unwrap_err(),
        "the proof profile \"RGB Proof\" is not CMYK, so it has no plates"
    );
    assert_eq!(
        rgb_proof(&a, r#"{"preserve_numbers":true}"#).unwrap_err(),
        "Preserve Numbers needs a CMYK document"
    );
}

fn separate(doc: *const PrecisionDocument, request: &str) -> Result<Value, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_cmyk_separate(doc, ptr::null(), 0, text.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

#[test]
fn separate_reads_the_ink_the_verdict_and_the_in_gamut_colour() {
    let dir = TempDir::new().unwrap();
    let doc = cmyk_doc(&dir, 1, 1, &[[0, 0, 0, 0]]);
    // The oracle's ΔE decides which colour is which, with a margin.
    let inside = [0.9, 0.9, 0.9];
    let outside = [0.0, 0.0, 1.0];
    assert!(
        gamut_delta_srgb(inside) < 4.0,
        "{}",
        gamut_delta_srgb(inside)
    );
    assert!(
        gamut_delta_srgb(outside) > 6.0,
        "{}",
        gamut_delta_srgb(outside)
    );
    for (rgb, out) in [(inside, false), (outside, true)] {
        let answer = separate(
            doc,
            &format!(r#"{{"values":[{},{},{}]}}"#, rgb[0], rgb[1], rgb[2]),
        )
        .unwrap();
        assert_eq!(answer["out_of_gamut"], out, "{rgb:?}: {answer}");
        let delta = answer["delta_e"].as_f64().unwrap();
        assert!((delta - gamut_delta_srgb(rgb)).abs() < 0.05, "{delta}");
        assert_eq!(answer["profile"], "Press A");
        // sRGB's zero black onto the press's zero destination black: BPC is
        // the identity, so the ink is the relative B2A1's.
        let ink = srgb_to_ink(rgb);
        let got: Vec<f64> = answer["ink"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        for c in 0..4 {
            assert!(
                (got[c] - ink[c].clamp(0.0, 1.0)).abs() < 1.0 / 65535.0 * 2.0,
                "{rgb:?} ink {c}"
            );
        }
        let back = ink_to_srgb(std::array::from_fn(|c| got[c]), true);
        let in_gamut: Vec<f64> = answer["in_gamut"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        for c in 0..3 {
            assert!(
                (in_gamut[c] - back[c].clamp(0.0, 1.0)).abs() < 1.0 / 512.0,
                "{rgb:?} rgb {c}"
            );
        }
    }
    // An RGB document (here NULL) separates through the working CMYK, which
    // this binary leaves unset.
    assert_eq!(
        separate(ptr::null(), r#"{"values":[0,0,0]}"#).unwrap_err(),
        "no working CMYK profile is set, so a colour cannot be separated"
    );
    assert_eq!(
        separate(doc, r#"{"values":[0,0]}"#).unwrap_err(),
        "the separation request needs \"values\": [r, g, b], each 0…1"
    );
    unsafe { dr_precision_doc_free(doc) };
}
