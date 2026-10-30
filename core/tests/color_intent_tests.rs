//! Rendering intents, black-point compensation and the general
//! `ColorPipeline`, against arithmetic written here.
//!
//! The oracles are independent: the ISO 18619 BPC formula is written out
//! from the published equations, the absolute media-white scaling from the
//! ICC one, and the fast-path identity is proved by comparing
//! `ColorPipeline::color8` with the SHIPPED `Transform::color` over all
//! 16 777 216 byte triples' worth of a dense sweep rather than by assuming
//! the two call the same code.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_output_color::dr_proof_render;
use doctored_core::icc::IccProfile;
use doctored_core::icc_black_point;
use doctored_core::icc_intent::{self, ColorPipeline, RenderingIntent};
use doctored_core::icc_lut::{xyz_to_lab, ProofProfile, D50};
use doctored_core::icc_transform::Transform;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;

use crate::icc_lut_builders as builders;

use crate::common;
use common::{img_pixels, take_err_string};

fn srgb() -> std::sync::Arc<IccProfile> {
    IccProfile::srgb()
}

fn p3() -> std::sync::Arc<IccProfile> {
    IccProfile::display_p3()
}

// ------------------------------------------------------------ the names --

#[test]
fn the_four_intents_round_trip_every_spelling() {
    let table = [
        (RenderingIntent::Perceptual, "perceptual", 0, 0usize),
        (RenderingIntent::RelativeColorimetric, "relative", 1, 1),
        (RenderingIntent::Saturation, "saturation", 2, 2),
        (RenderingIntent::AbsoluteColorimetric, "absolute", 3, 1),
    ];
    for (intent, name, code, slot) in table {
        assert_eq!(intent.name(), name);
        assert_eq!(RenderingIntent::parse(name), Some(intent));
        assert_eq!(intent.to_c(), code);
        assert_eq!(RenderingIntent::from_c(code), Some(intent));
        assert_eq!(intent.table(), slot, "{name} reads table {slot}");
    }
    assert_eq!(RenderingIntent::parse("Perceptual"), None);
    assert_eq!(RenderingIntent::parse(""), None);
    assert_eq!(RenderingIntent::from_c(-1), None);
    assert_eq!(RenderingIntent::from_c(4), None);
    assert_eq!(
        RenderingIntent::default(),
        RenderingIntent::RelativeColorimetric
    );
    assert!(RenderingIntent::AbsoluteColorimetric.is_absolute());
    assert!(!RenderingIntent::RelativeColorimetric.is_absolute());
}

// ------------------------------------------------------- the fast path --

#[test]
fn the_default_matrix_pipeline_is_the_shipped_transform_byte_for_byte() {
    // This is the byte-identity guarantee, MEASURED. If `ColorPipeline` ever
    // stops holding the shipped `Transform` on this path, some code below
    // differs by a code somewhere in the 16.7 M cube and this fails.
    let source = srgb();
    let destination = p3();
    let shipped = Transform::new(
        source.model().expect("sRGB is matrix/TRC"),
        destination.model().expect("P3 is matrix/TRC"),
    );
    for intent in [
        RenderingIntent::Perceptual,
        RenderingIntent::RelativeColorimetric,
        RenderingIntent::Saturation,
    ] {
        let pipeline = ColorPipeline::new(&source, &destination, intent, false)
            .expect("two matrix profiles prepare");
        assert!(!pipeline.is_identity());
        // A dense stride over the cube (32^3 = 32768 triples) plus every
        // neutral and every primary ramp, which is where a rounding change
        // would show first. The shipped answer comes from `Transform::apply`,
        // the public entry point the document conversion itself uses.
        let mut cases: Vec<[u8; 3]> = Vec::new();
        for r in (0u16..=255).step_by(8) {
            for g in (0u16..=255).step_by(8) {
                for b in (0u16..=255).step_by(8) {
                    cases.push([r as u8, g as u8, b as u8]);
                }
            }
        }
        for v in 0u16..=255 {
            let v = v as u8;
            cases.push([v, v, v]);
            cases.push([v, 0, 0]);
            cases.push([0, v, 0]);
            cases.push([0, 0, v]);
        }
        let mut buffer = image::RgbaImage::new(cases.len() as u32, 1);
        for (x, rgb) in cases.iter().enumerate() {
            buffer.put_pixel(x as u32, 0, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
        }
        shipped.apply(&mut buffer);
        for (x, rgb) in cases.iter().enumerate() {
            let want = buffer.get_pixel(x as u32, 0);
            assert_eq!(
                pipeline.color8(*rgb),
                [want[0], want[1], want[2]],
                "{rgb:?} {intent:?}"
            );
        }
    }
}

#[test]
fn the_same_space_pipeline_is_the_identity_and_says_so() {
    let pipeline = ColorPipeline::new(
        &srgb(),
        &srgb(),
        RenderingIntent::RelativeColorimetric,
        false,
    )
    .expect("prepares");
    assert!(pipeline.is_identity());
    for v in 0u16..=255 {
        let rgb = [v as u8, (255 - v) as u8, 128];
        assert_eq!(pipeline.color8(rgb), rgb);
    }
}

#[test]
fn absolute_and_bpc_leave_the_fast_path() {
    // Neither option can take the shipped `Transform`, so both must produce
    // a pipeline that is NOT reported as the identity even between two
    // profiles describing the same space.
    for (intent, bpc) in [
        (RenderingIntent::AbsoluteColorimetric, false),
        (RenderingIntent::RelativeColorimetric, true),
    ] {
        let pipeline = ColorPipeline::new(&srgb(), &srgb(), intent, bpc).expect("prepares");
        assert!(!pipeline.is_identity(), "{intent:?} bpc={bpc}");
        // And between two profiles of the same space with D50 whites and
        // equal black points, it is still numerically an identity — which is
        // the arithmetic, not the branch.
        for v in [0u8, 1, 64, 128, 200, 255] {
            let out = pipeline.color8([v, v, v]);
            for c in out {
                assert!(
                    (i32::from(c) - i32::from(v)).abs() <= 1,
                    "{v} came back {out:?} under {intent:?} bpc={bpc}"
                );
            }
        }
    }
}

// -------------------------------------------------------- black points --

#[test]
fn a_matrix_profiles_black_point_is_its_own_device_zero() {
    let black = icc_black_point::black_point(&srgb(), RenderingIntent::RelativeColorimetric)
        .expect("sRGB has a black point");
    // sRGB's curve is 0 at 0, so the PCS black is the origin.
    for c in black {
        assert!(c.abs() < 1e-9, "{black:?}");
    }
    assert_eq!(icc_intent::media_white(&srgb()), D50);
}

#[test]
fn the_bpc_terms_are_iso_18619s_own_formula() {
    // Written out from the published equations, then compared against what
    // the module computes through its public entry points: a pipeline built
    // with BPC between two profiles whose black points we KNOW.
    let source_black = [0.0, 0.0, 0.0];
    let destination_black = [0.02, 0.02, 0.018];
    let terms = icc_intent::bpc_terms(source_black, destination_black);
    for i in 0..3 {
        let a = (D50[i] - destination_black[i]) / (D50[i] - source_black[i]);
        let b = D50[i] * (destination_black[i] - source_black[i]) / (D50[i] - source_black[i]);
        assert!((terms[i].0 - a).abs() < 1e-12, "a[{i}]");
        assert!((terms[i].1 - b).abs() < 1e-12, "b[{i}]");
        // The two fixed points the formula is FOR: source black lands on
        // destination black, and D50 does not move.
        let mapped_black = terms[i].0 * source_black[i] + terms[i].1;
        assert!((mapped_black - destination_black[i]).abs() < 1e-12);
        let mapped_white = terms[i].0 * D50[i] + terms[i].1;
        assert!((mapped_white - D50[i]).abs() < 1e-12);
    }
    // Equal black points are the identity, which is why BPC between two
    // matrix/TRC profiles changes nothing.
    let identity = icc_intent::bpc_terms([0.01, 0.01, 0.01], [0.01, 0.01, 0.01]);
    for (a, b) in identity {
        assert!((a - 1.0).abs() < 1e-12 && b.abs() < 1e-12);
    }
}

#[test]
fn a_press_profiles_black_point_is_a_neutral_dark_grey() {
    let Ok(bytes) = std::fs::read(
        "/Library/Application Support/Adobe/Color/Profiles/Recommended/CoatedFOGRA39.icc",
    ) else {
        eprintln!("skipping: CoatedFOGRA39.icc is not installed");
        return;
    };
    let proof = ProofProfile::parse(&bytes).expect("FOGRA39 parses");
    let black = icc_black_point::proof_black_point(&proof, RenderingIntent::RelativeColorimetric);
    // A coated press sheet's black is NOT zero — that is the whole reason
    // BPC exists — and it is a long way below mid grey.
    let lab = doctored_core::icc_lut::xyz_to_lab(black);
    assert!(
        lab[0] > 0.5,
        "a printed black is not the PCS origin: {lab:?}"
    );
    assert!(lab[0] < 25.0, "but it is still black: {lab:?}");
    // lcms makes every measured black neutral.
    assert!(lab[1].abs() < 1e-6 && lab[2].abs() < 1e-6, "{lab:?}");
}

// ------------------------------------------- lcms's black-point rule --
//
// Synthetic `mft2` CMYK/RGB profiles whose blacks are known by construction,
// against Little CMS's `cmsDetectBlackPoint` written out: v4 perceptual /
// saturation is the fixed perceptual black; an OUTPUT (`prtr`) CMYK profile
// at relative is Lab 0 through B2A0 and back through A2B1; anything else is
// the darkest colorant through the intent's own A2B; a measured black is
// made neutral with L* ≤ 50; a missing table is zero.

/// lcms's `cmsPERCEPTUAL_BLACK_X/Y/Z`.
const LCMS_PERCEPTUAL_BLACK: [f64; 3] = [0.00336, 0.0034731, 0.00287];

/// v2 legacy 16-bit Lab, normalized: L* = 100 at 0xFF00, a* = 0 at 0x8000.
fn enc_l(lstar: f64) -> f64 {
    lstar / 100.0 * 65280.0 / 65535.0
}

fn enc_ab(v: f64) -> f64 {
    (v + 128.0) * 256.0 / 65535.0
}

/// A CMYK→Lab table on a two-point grid: L* = `white` − `drop`·k, a* =
/// `tint`·k, b* = 0 (c, m and y ignored).
fn a2b_cmyk(white: f64, drop: f64, tint: f64) -> Vec<u8> {
    builders::tag_mft2(4, 3, 2, &move |d: &[f64]| {
        vec![enc_l(white - drop * d[3]), enc_ab(tint * d[3]), enc_ab(0.0)]
    })
}

/// A Lab→CMYK table: k = `k_at_black`·(1 − L/100), c = m = y = 0.
fn b2a_cmyk(k_at_black: f64) -> Vec<u8> {
    builders::tag_mft2(3, 4, 2, &move |lab: &[f64]| {
        vec![0.0, 0.0, 0.0, k_at_black * (1.0 - lab[0])]
    })
}

fn cmyk_lut_profile(
    version: [u8; 4],
    class: &[u8; 4],
    tags: &[(&[u8; 4], Vec<u8>)],
) -> ProofProfile {
    let mut all: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", builders::tag_desc("Black Test")),
        (b"wtpt", builders::tag_wtpt(D50)),
    ];
    all.extend(tags.iter().cloned());
    ProofProfile::parse(&builders::build(version, class, b"CMYK", b"Lab ", &all))
        .expect("the synthetic profile parses")
}

const V2: [u8; 4] = [2, 0x10, 0, 0];

/// Asserts `xyz` is the neutral black at `lstar` (a* = b* = 0).
fn assert_neutral_black(xyz: [f64; 3], lstar: f64, what: &str) {
    let lab = xyz_to_lab(xyz);
    assert!(
        (lab[0] - lstar).abs() < 0.05,
        "{what}: L* {} expected {lstar} ({lab:?})",
        lab[0]
    );
    assert!(
        lab[1].abs() < 1e-6 && lab[2].abs() < 1e-6,
        "{what}: not neutral {lab:?}"
    );
}

#[test]
fn an_output_cmyk_profile_at_relative_takes_its_perceptual_black() {
    // A2B (both tables): L* = 100 − 95·k, tinted. B2A0 sends Lab 0 to k = 1
    // (L* 5); B2A1 sends it to k = 0.8 (L* 24), the ink-limited black the
    // old B2A1 → A2B1 round trip measured.
    let a2b = a2b_cmyk(100.0, 95.0, 6.0);
    let proof = cmyk_lut_profile(
        V2,
        b"prtr",
        &[
            (b"A2B0", a2b.clone()),
            (b"A2B1", a2b),
            (b"B2A0", b2a_cmyk(1.0)),
            (b"B2A1", b2a_cmyk(0.8)),
        ],
    );
    assert!(proof.is_output_class());
    let relative =
        icc_black_point::proof_black_point(&proof, RenderingIntent::RelativeColorimetric);
    assert_neutral_black(relative, 5.0, "prtr relative");
    // Absolute is asked as relative.
    let absolute =
        icc_black_point::proof_black_point(&proof, RenderingIntent::AbsoluteColorimetric);
    assert_eq!(absolute, relative);
}

#[test]
fn a_missing_perceptual_table_is_a_zero_black_and_input_profiles_take_the_darker_colorant() {
    // A2B only: an output profile cannot run B2A0, so lcms answers zero; the
    // same tables as an input (`scnr`) profile take the darker colorant,
    // full ink through A2B1 = L* 5.
    let a2b = a2b_cmyk(100.0, 95.0, 0.0);
    let tags = [(b"A2B0", a2b.clone()), (b"A2B1", a2b)];
    let output = cmyk_lut_profile(V2, b"prtr", &tags);
    assert_eq!(
        icc_black_point::proof_black_point(&output, RenderingIntent::RelativeColorimetric),
        [0.0, 0.0, 0.0]
    );
    let input = cmyk_lut_profile(V2, b"scnr", &tags);
    assert!(!input.is_output_class());
    assert_neutral_black(
        icc_black_point::proof_black_point(&input, RenderingIntent::RelativeColorimetric),
        5.0,
        "scnr relative",
    );
}

#[test]
fn the_darker_colorant_is_read_through_the_asked_intents_own_table() {
    // No B2A at all. A2B0's full ink is L* 8, A2B1's L* 20, both tinted.
    let proof_tags = [
        (b"A2B0", a2b_cmyk(100.0, 92.0, 12.0)),
        (b"A2B1", a2b_cmyk(100.0, 80.0, -9.0)),
    ];
    // A v2 output profile at perceptual has no fixed reference medium and
    // no relative ink-limit discount: the darker colorant through A2B0.
    let output = cmyk_lut_profile(V2, b"prtr", &proof_tags);
    assert_neutral_black(
        icc_black_point::proof_black_point(&output, RenderingIntent::Perceptual),
        8.0,
        "prtr perceptual",
    );
    // An input profile at relative: the darker colorant through A2B1.
    let input = cmyk_lut_profile(V2, b"scnr", &proof_tags);
    assert_neutral_black(
        icc_black_point::proof_black_point(&input, RenderingIntent::RelativeColorimetric),
        20.0,
        "scnr relative",
    );
}

#[test]
fn a_black_lighter_than_mid_grey_clips_to_l_50() {
    // Full ink is only L* 70: the perceptual black of this output profile is
    // clipped to L* 50, as lcms clips it.
    let a2b = a2b_cmyk(100.0, 30.0, 4.0);
    let proof = cmyk_lut_profile(
        V2,
        b"prtr",
        &[
            (b"A2B0", a2b.clone()),
            (b"A2B1", a2b),
            (b"B2A0", b2a_cmyk(1.0)),
        ],
    );
    assert_neutral_black(
        icc_black_point::proof_black_point(&proof, RenderingIntent::RelativeColorimetric),
        50.0,
        "L* 70 black",
    );
}

#[test]
fn a_darker_colorant_lighter_than_l_95_is_a_zero_black_but_a_perceptual_one_clips() {
    // Full ink is only L* 97. As an input profile that is the darker
    // colorant, which lcms answers with zero (a synthetic negative profile);
    // the same tables on an output profile with a B2A0 are its perceptual
    // black, which lcms clips to L* 50.
    //
    // The darker colorant's other zero case, L* below 0, is not pinned: the
    // only table these builders write is mft2, whose legacy Lab encoding
    // (and its XYZ one) cannot express a negative L*.
    let a2b = a2b_cmyk(100.0, 3.0, 4.0);
    let input = cmyk_lut_profile(
        V2,
        b"scnr",
        &[(b"A2B0", a2b.clone()), (b"A2B1", a2b.clone())],
    );
    assert_eq!(
        icc_black_point::proof_black_point(&input, RenderingIntent::RelativeColorimetric),
        [0.0, 0.0, 0.0]
    );
    let output = cmyk_lut_profile(
        V2,
        b"prtr",
        &[
            (b"A2B0", a2b.clone()),
            (b"A2B1", a2b),
            (b"B2A0", b2a_cmyk(1.0)),
        ],
    );
    assert_neutral_black(
        icc_black_point::proof_black_point(&output, RenderingIntent::RelativeColorimetric),
        50.0,
        "prtr L* 97 perceptual black",
    );
}

#[test]
fn a_v4_profile_at_perceptual_and_saturation_is_the_fixed_perceptual_black() {
    let a2b = a2b_cmyk(100.0, 95.0, 0.0);
    let proof = cmyk_lut_profile(
        [4, 0x20, 0, 0],
        b"prtr",
        &[(b"A2B0", a2b), (b"B2A0", b2a_cmyk(1.0))],
    );
    for intent in [RenderingIntent::Perceptual, RenderingIntent::Saturation] {
        assert_eq!(
            icc_black_point::proof_black_point(&proof, intent),
            LCMS_PERCEPTUAL_BLACK,
            "{intent:?}"
        );
    }
}

#[test]
fn an_rgb_lut_profile_takes_its_device_zero_made_neutral() {
    // A `mntr` RGB A2B1 whose device zero is L* 10, tinted.
    let a2b1 = builders::tag_mft2(3, 3, 2, &|d: &[f64]| {
        let mean = (d[0] + d[1] + d[2]) / 3.0;
        vec![
            enc_l(10.0 + 90.0 * mean),
            enc_ab(5.0 * (1.0 - mean)),
            enc_ab(-3.0),
        ]
    });
    let bytes = builders::build(
        V2,
        b"mntr",
        b"RGB ",
        b"Lab ",
        &[
            (b"desc", builders::tag_desc("RGB Black Test")),
            (b"wtpt", builders::tag_wtpt(D50)),
            (b"A2B1", a2b1),
        ],
    );
    let proof = ProofProfile::parse(&bytes).expect("parses");
    assert_neutral_black(
        icc_black_point::proof_black_point(&proof, RenderingIntent::RelativeColorimetric),
        10.0,
        "mntr relative",
    );
}

// ------------------------------------------------- the general pipeline --

#[test]
fn absolute_colorimetric_scales_by_the_two_media_whites() {
    // Between two profiles with the SAME media white, absolute is relative.
    // The interesting case needs a profile with a non-D50 `wtpt`, which is
    // a LUT profile here — FOGRA39's own white is well below D50.
    let Ok(bytes) = std::fs::read(
        "/Library/Application Support/Adobe/Color/Profiles/Recommended/CoatedFOGRA39.icc",
    ) else {
        eprintln!("skipping: CoatedFOGRA39.icc is not installed");
        return;
    };
    let proof = ProofProfile::parse(&bytes).expect("parses");
    let white = proof.media_white();
    assert!(
        white[1] < D50[1],
        "a press profile's media white is below the PCS white: {white:?}"
    );
    // The scaling a conversion INTO this medium would apply, written here.
    let scale: [f64; 3] = std::array::from_fn(|i| D50[i] / white[i]);
    for s in scale {
        assert!(s > 1.0, "scaling up is what putting the paper back means");
    }
}

#[test]
fn a_lut_destination_converts_and_a_gray_one_is_refused() {
    let Ok(bytes) =
        std::fs::read("/Library/Application Support/Adobe/Color/Profiles/VideoHD16-235.icc")
    else {
        eprintln!("skipping: VideoHD16-235.icc is not installed");
        return;
    };
    let lut = IccProfile::parse(&bytes).expect("an RGB v4 LUT profile is storable");
    assert!(lut.model().is_none(), "it has no matrix/TRC model");
    assert!(lut.can_decode() && lut.can_encode());
    let lut = std::sync::Arc::new(lut);

    // sRGB -> the LUT profile -> back is a near-identity: that is what makes
    // the two tables a pair, and it needs no golden number.
    let forward = ColorPipeline::new(&srgb(), &lut, RenderingIntent::RelativeColorimetric, false)
        .expect("prepares");
    let back = ColorPipeline::new(&lut, &srgb(), RenderingIntent::RelativeColorimetric, false)
        .expect("prepares");
    let mut worst = 0i32;
    for v in (16u16..=240).step_by(8) {
        let rgb = [v as u8, v as u8, v as u8];
        let round = back.color8(forward.color8(rgb));
        for c in 0..3 {
            worst = worst.max((i32::from(round[c]) - i32::from(rgb[c])).abs());
        }
    }
    assert!(worst <= 3, "the round trip drifted {worst} codes");

    // A CMYK or Gray profile is not an RGB document profile at all, so it
    // never reaches `ColorPipeline` — `IccProfile::parse` refuses it first.
    let Ok(gray) = std::fs::read("/System/Library/ColorSync/Profiles/Generic Gray Profile.icc")
    else {
        return;
    };
    assert!(
        IccProfile::parse(&gray).is_none(),
        "a grey profile is not storable"
    );
}

#[test]
fn a_pipeline_reports_its_own_scratch() {
    let pipeline =
        ColorPipeline::new(&srgb(), &p3(), RenderingIntent::Perceptual, false).expect("prepares");
    assert!(pipeline.scratch_bytes() > 0);
    // The continuous path costs a 65537-entry f64 table per channel, so the
    // declared scratch must be at least that.
    assert!(pipeline.scratch_bytes() >= 65537 * 3 * 8);
}

#[test]
fn the_continuous_and_byte_colours_agree_within_a_code() {
    // `color` is the f32 entry point the 16-bit paths use and `color8` the
    // byte one; on the fast path they are two shipped transforms, and they
    // must not disagree by more than the byte grid.
    let pipeline = ColorPipeline::new(&srgb(), &p3(), RenderingIntent::RelativeColorimetric, false)
        .expect("prepares");
    for v in (0u16..=255).step_by(3) {
        let rgb = [v as u8, (255 - v) as u8, 128];
        let bytes = pipeline.color8(rgb);
        let continuous = pipeline.color(rgb.map(|c| f32::from(c) / 255.0));
        for c in 0..3 {
            let scaled = (continuous[c] * 255.0).round() as i32;
            assert!(
                (scaled - i32::from(bytes[c])).abs() <= 1,
                "{rgb:?}: {bytes:?} against {continuous:?}"
            );
        }
    }
}

// ------------------------------------- a neutral, clipped TRC black --
//
// lcms's matrix-shaper and Gray source black IS `BlackPointAsDarkerColorant`:
// the device zero pushed forward, made neutral with L* ≤ 50, and zero when
// its L* is above 95. Written out here from CIE L* and the s15Fixed16 and
// `curv` encodings the profile bytes hold.

/// The s15Fixed16 a `XYZ ` tag stores `v` as, read back.
fn s15(v: f64) -> f64 {
    (v * 65536.0).round() / 65536.0
}

/// A two-entry `curv` whose output at 0 is `toe` (a u16 code) and at 1 is
/// full scale.
fn curv_toe(toe: u16) -> Vec<u8> {
    let mut t = b"curv\0\0\0\0".to_vec();
    t.extend_from_slice(&2u32.to_be_bytes());
    t.extend_from_slice(&toe.to_be_bytes());
    t.extend_from_slice(&65535u16.to_be_bytes());
    t
}

/// The D50-adapted sRGB colorants, as the synthetic matrix profiles carry
/// them (columns r, g, b).
const COLORANTS: [[f64; 3]; 3] = [
    [0.4361, 0.2225, 0.0139],
    [0.3851, 0.7169, 0.0971],
    [0.1431, 0.0606, 0.7141],
];

/// A matrix/TRC RGB profile whose three curves lift device zero to `toes`.
fn tinted_toe_profile(toes: [u16; 3]) -> std::sync::Arc<IccProfile> {
    let bytes = builders::build(
        V2,
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", builders::tag_desc("Tinted Toe")),
            (b"wtpt", builders::tag_wtpt(D50)),
            (b"rXYZ", builders::tag_wtpt(COLORANTS[0])),
            (b"gXYZ", builders::tag_wtpt(COLORANTS[1])),
            (b"bXYZ", builders::tag_wtpt(COLORANTS[2])),
            (b"rTRC", curv_toe(toes[0])),
            (b"gTRC", curv_toe(toes[1])),
            (b"bTRC", curv_toe(toes[2])),
        ],
    );
    let profile = IccProfile::parse(&bytes).expect("the tinted-toe profile is storable");
    assert!(profile.model().is_some(), "it is matrix/TRC");
    std::sync::Arc::new(profile)
}

/// CIE L* of a relative luminance.
fn lstar_of(y: f64) -> f64 {
    if y > 216.0 / 24389.0 {
        116.0 * y.cbrt() - 16.0
    } else {
        24389.0 / 27.0 * y
    }
}

/// The relative luminance of an L*.
fn y_of(lstar: f64) -> f64 {
    let f = (lstar + 16.0) / 116.0;
    if f * f * f > 216.0 / 24389.0 {
        f * f * f
    } else {
        lstar * 27.0 / 24389.0
    }
}

/// The neutral XYZ at `lstar`: a* = b* = 0 is the D50 white scaled by Y.
fn neutral(lstar: f64) -> [f64; 3] {
    let y = y_of(lstar);
    [D50[0] * y, D50[1] * y, D50[2] * y]
}

/// `BlackPointAsDarkerColorant` over a device-zero luminance, written out.
fn darker_colorant_oracle(y: f64) -> [f64; 3] {
    let lstar = lstar_of(y);
    if !(0.0..=95.0).contains(&lstar) {
        return [0.0, 0.0, 0.0];
    }
    neutral(lstar.min(50.0))
}

fn assert_xyz_close(actual: [f64; 3], expected: [f64; 3], tolerance: f64, what: &str) {
    for c in 0..3 {
        assert!(
            (actual[c] - expected[c]).abs() < tolerance,
            "{what}: {actual:?} expected {expected:?}"
        );
    }
}

const INTENTS: [RenderingIntent; 4] = [
    RenderingIntent::Perceptual,
    RenderingIntent::RelativeColorimetric,
    RenderingIntent::Saturation,
    RenderingIntent::AbsoluteColorimetric,
];

#[test]
fn a_tinted_matrix_toe_is_made_neutral_and_clipped() {
    // Unequal toes, so the device zero carries a cast. Y = Σ colorant_Y ·
    // curve(0), the curve's first entry over 65535.
    for (toes, what) in [
        ([1966u16, 524, 1311], "a dark toe, L* ≈ 12"),
        ([29491, 22938, 32768], "a toe between L* 50 and 95"),
        ([63570, 62914, 64225], "a toe above L* 95"),
    ] {
        let profile = tinted_toe_profile(toes);
        let y: f64 = (0..3)
            .map(|c| s15(COLORANTS[c][1]) * f64::from(toes[c]) / 65535.0)
            .sum();
        let expected = darker_colorant_oracle(y);
        for intent in INTENTS {
            let black = icc_black_point::black_point(&profile, intent).expect("matrix black");
            assert_xyz_close(black, expected, 1e-9, &format!("{what} {intent:?}"));
            let lab = xyz_to_lab(black);
            assert!(
                lab[1].abs() < 1e-9 && lab[2].abs() < 1e-9,
                "{what}: {lab:?}"
            );
        }
        match what {
            "a dark toe, L* ≈ 12" => assert!((lstar_of(y) - 12.0).abs() < 1.0, "{y}"),
            "a toe between L* 50 and 95" => {
                assert!((50.0..95.0).contains(&lstar_of(y)), "{y}");
                assert!((xyz_to_lab(expected)[0] - 50.0).abs() < 1e-9);
            }
            _ => {
                assert!(lstar_of(y) > 95.0, "{y}");
                assert_eq!(expected, [0.0, 0.0, 0.0]);
            }
        }
    }
}

#[test]
fn a_lifted_gray_trc_is_clipped_like_a_darker_colorant() {
    for toe in [3277u16, 26214, 64880] {
        let bytes = builders::build(
            V2,
            b"mntr",
            b"GRAY",
            b"XYZ ",
            &[
                (b"desc", builders::tag_desc("Lifted Gray")),
                (b"wtpt", builders::tag_wtpt(D50)),
                (b"kTRC", curv_toe(toe)),
            ],
        );
        let gray = ProofProfile::parse(&bytes).expect("gray parses");
        let expected = darker_colorant_oracle(f64::from(toe) / 65535.0);
        for intent in INTENTS {
            assert_xyz_close(
                icc_black_point::proof_black_point(&gray, intent),
                expected,
                1e-9,
                &format!("toe {toe} {intent:?}"),
            );
        }
    }
}

#[test]
fn srgb_and_p3_blacks_stay_exactly_zero() {
    for profile in [srgb(), p3()] {
        for intent in INTENTS {
            assert_eq!(
                icc_black_point::black_point(&profile, intent),
                Some([0.0, 0.0, 0.0]),
                "{} {intent:?}",
                profile.name()
            );
            assert_eq!(
                icc_black_point::destination_black_point(&profile, intent),
                Some([0.0, 0.0, 0.0]),
                "{} {intent:?}",
                profile.name()
            );
        }
    }
}

// ------------------------------ the destination black point --
//
// lcms's `cmsDetectDestinationBlackPoint`: a LUT destination's black is where
// its own Lab → device → Lab round trip bottoms out, not its darkest
// colorant. Synthetic `mft2` CMYK profiles whose round trips are known by
// construction (A2B: L* = 100 − 95·k), plus lcms's algorithm written out
// below for a round trip with a kink.

#[test]
fn a_matrix_destination_is_its_source_black() {
    for profile in [srgb(), p3(), tinted_toe_profile([1966, 524, 1311])] {
        for intent in INTENTS {
            assert_eq!(
                icc_black_point::destination_black_point(&profile, intent),
                icc_black_point::black_point(&profile, intent),
                "{} {intent:?}",
                profile.name()
            );
        }
    }
}

/// An output CMYK profile with the shared A2B pair and the given B2A tables.
fn press_with(b2a: &[(&[u8; 4], Vec<u8>)]) -> ProofProfile {
    let a2b = a2b_cmyk(100.0, 95.0, 6.0);
    let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![(b"A2B0", a2b.clone()), (b"A2B1", a2b)];
    tags.extend(b2a.iter().cloned());
    cmyk_lut_profile(V2, b"prtr", &tags)
}

#[test]
fn a_nearly_straight_round_trip_keeps_the_source_black() {
    // B2A1 inverts A2B1 up to the legacy L* encoding: within 4 L* of the
    // identity above the shadows, so the destination black is the source
    // (perceptual) black, L* 5.
    let proof = press_with(&[(b"B2A0", b2a_cmyk(1.0)), (b"B2A1", b2a_cmyk(1.0))]);
    let relative = RenderingIntent::RelativeColorimetric;
    let source = icc_black_point::proof_black_point(&proof, relative);
    assert_neutral_black(source, 5.0, "source");
    assert_xyz_close(
        icc_black_point::proof_destination_black_point(&proof, relative),
        source,
        1e-12,
        "straight round trip",
    );
}

#[test]
fn an_ink_limited_round_trip_fits_to_a_zero_black() {
    // B2A1 stops at k = 0.8, so the round trip is a LIFTED straight line
    // (L* 24 at the bottom): not straight, and its fit's root is L* 0.
    let proof = press_with(&[(b"B2A0", b2a_cmyk(1.0)), (b"B2A1", b2a_cmyk(0.8))]);
    let relative = RenderingIntent::RelativeColorimetric;
    assert_neutral_black(
        icc_black_point::proof_black_point(&proof, relative),
        5.0,
        "the source black is still the perceptual one",
    );
    let destination = icc_black_point::proof_destination_black_point(&proof, relative);
    for c in destination {
        assert!(c.abs() < 1e-9, "{destination:?}");
    }
    // Absolute is asked as relative.
    assert_eq!(
        icc_black_point::proof_destination_black_point(
            &proof,
            RenderingIntent::AbsoluteColorimetric
        ),
        destination
    );
}

#[test]
fn a_flat_round_trip_is_a_zero_black() {
    let flat = builders::tag_mft2(3, 4, 2, &|_: &[f64]| vec![0.0, 0.0, 0.0, 0.5]);
    let proof = press_with(&[(b"B2A0", b2a_cmyk(1.0)), (b"B2A1", flat)]);
    assert_eq!(
        icc_black_point::proof_destination_black_point(
            &proof,
            RenderingIntent::RelativeColorimetric
        ),
        [0.0, 0.0, 0.0]
    );
}

#[test]
fn the_destination_falls_back_to_the_source_black_where_lcms_does() {
    // v4 at perceptual/saturation: the fixed perceptual black.
    let v4 = cmyk_lut_profile(
        [4, 0x20, 0, 0],
        b"prtr",
        &[
            (b"A2B0", a2b_cmyk(100.0, 95.0, 0.0)),
            (b"B2A0", b2a_cmyk(0.8)),
        ],
    );
    for intent in [RenderingIntent::Perceptual, RenderingIntent::Saturation] {
        assert_eq!(
            icc_black_point::proof_destination_black_point(&v4, intent),
            LCMS_PERCEPTUAL_BLACK,
            "{intent:?}"
        );
    }
    // No B2A at all: the darker colorant of an input profile.
    let a2b = a2b_cmyk(100.0, 95.0, 0.0);
    let input = cmyk_lut_profile(V2, b"scnr", &[(b"A2B0", a2b.clone()), (b"A2B1", a2b)]);
    for intent in INTENTS {
        assert_eq!(
            icc_black_point::proof_destination_black_point(&input, intent),
            icc_black_point::proof_black_point(&input, intent),
            "A2B-only {intent:?}"
        );
    }
    // A Gray `kTRC`.
    let bytes = builders::build(
        V2,
        b"mntr",
        b"GRAY",
        b"XYZ ",
        &[
            (b"desc", builders::tag_desc("Lifted Gray")),
            (b"wtpt", builders::tag_wtpt(D50)),
            (b"kTRC", curv_toe(3277)),
        ],
    );
    let gray = ProofProfile::parse(&bytes).expect("gray parses");
    for intent in INTENTS {
        assert_eq!(
            icc_black_point::proof_destination_black_point(&gray, intent),
            icc_black_point::proof_black_point(&gray, intent),
            "Gray {intent:?}"
        );
    }
}

/// `y = a·x² + b·x + c` fitted by least squares, solved by Cramer's rule —
/// lcms's `RootOfLeastSquaresFitQuadraticCurve` written out, with the
/// evident [0, 50] clamp in its linear case too.
fn quadratic_root_oracle(x: &[f64], y: &[f64]) -> f64 {
    if x.len() < 4 {
        return 0.0;
    }
    let n = x.len() as f64;
    let power = |v: f64, p: usize| (0..p).fold(1.0, |acc, _| acc * v);
    let s = |p: usize| x.iter().map(|&v| power(v, p)).sum::<f64>();
    let t = |p: usize| x.iter().zip(y).map(|(&v, w)| w * power(v, p)).sum::<f64>();
    let m = [[n, s(1), s(2)], [s(1), s(2), s(3)], [s(2), s(3), s(4)]];
    let v = [t(0), t(1), t(2)];
    // The signed 2×2 minor of `m` without `row` and `column`.
    let cofactor = |row: usize, column: usize| {
        let rows: Vec<usize> = (0..3).filter(|&r| r != row).collect();
        let columns: Vec<usize> = (0..3).filter(|&c| c != column).collect();
        let minor = m[rows[0]][columns[0]] * m[rows[1]][columns[1]]
            - m[rows[0]][columns[1]] * m[rows[1]][columns[0]];
        if (row + column).is_multiple_of(2) {
            minor
        } else {
            -minor
        }
    };
    let det: f64 = (0..3)
        .map(|column| m[0][column] * cofactor(0, column))
        .sum();
    // Cramer: column `j` replaced by `v`, expanded along that column.
    let replaced = |j: usize| (0..3).map(|row| v[row] * cofactor(row, j)).sum::<f64>() / det;
    let (c, b, a) = (replaced(0), replaced(1), replaced(2));
    if a.abs() < 1e-10 {
        if b.abs() < 1e-10 {
            return 0.0;
        }
        return (-c / b).clamp(0.0, 50.0);
    }
    let d = b * b - 4.0 * a * c;
    if d <= 0.0 {
        return 0.0;
    }
    ((-b + d.sqrt()) / (2.0 * a)).clamp(0.0, 50.0)
}

/// `cmsDetectDestinationBlackPoint` at relative colorimetric, written out
/// from lcms master `src/cmssamp.c`, over the profile's own round trip.
fn destination_lstar_oracle(proof: &ProofProfile) -> f64 {
    let relative = RenderingIntent::RelativeColorimetric;
    let initial = xyz_to_lab(icc_black_point::proof_black_point(proof, relative));
    let (a, b) = (initial[1].clamp(-50.0, 50.0), initial[2].clamp(-50.0, 50.0));
    let input: Vec<f64> = (0..256).map(|l| l as f64 * 100.0 / 255.0).collect();
    let mut output: Vec<f64> = input
        .iter()
        .map(|&lstar| {
            let (device, count) = proof
                .from_xyz(doctored_core::icc_lut::lab_to_xyz([lstar, a, b]), relative)
                .expect("B2A");
            xyz_to_lab(proof.to_xyz(&device[..count], relative).expect("A2B"))[0]
        })
        .collect();
    for l in (1..=254).rev() {
        output[l] = output[l].min(output[l + 1]);
    }
    let (min, max) = (output[0], output[255]);
    assert!(min < max, "the oracle's ramp rises");
    let straight =
        (0..256).all(|l| input[l] <= min + 0.2 * (max - min) || (input[l] - output[l]).abs() < 4.0);
    assert!(!straight, "the kinked round trip is not straight");
    let (x, y): (Vec<f64>, Vec<f64>) = (0..256)
        .map(|l| (input[l], (output[l] - min) / (max - min)))
        .filter(|&(_, ff)| (0.1..0.5).contains(&ff))
        .unzip();
    assert!(x.len() >= 4, "enough shadow points to fit");
    quadratic_root_oracle(&x, &y).max(0.0)
}

#[test]
fn a_kinked_round_trip_is_fitted_as_lcms_fits_it() {
    // B2A1 on a 9-point grid holds full ink up to L 0.3 (normalized), then
    // falls as a square to none: the round trip has a flat black toe, a
    // corner and a curve up to white — the shape lcms's fit is for.
    let kinked = builders::tag_mft2(3, 4, 9, &|lab: &[f64]| {
        let k = ((1.0 - lab[0]) / 0.7).powi(2).min(1.0);
        vec![0.0, 0.0, 0.0, k]
    });
    let proof = press_with(&[(b"B2A0", b2a_cmyk(1.0)), (b"B2A1", kinked)]);
    let expected = destination_lstar_oracle(&proof);
    assert!(
        expected > 1.0 && expected < 49.0,
        "the oracle's root is inside the clamp: {expected}"
    );
    let destination = icc_black_point::proof_destination_black_point(
        &proof,
        RenderingIntent::RelativeColorimetric,
    );
    let lab = xyz_to_lab(destination);
    assert!(
        (lab[0] - expected).abs() < 1e-9,
        "L* {} expected {expected}",
        lab[0]
    );
    // At the source black's own a*/b*, which lcms makes neutral.
    assert!(lab[1].abs() < 1e-6 && lab[2].abs() < 1e-6, "{lab:?}");
}

// ------------------------------- the destination black is what BPC uses --

/// The ink-limited press profile's BYTES: its relative round trip fits to a
/// zero black (see above), while its source black is L* 5.
fn ink_limited_press_bytes() -> Vec<u8> {
    let a2b = a2b_cmyk(100.0, 95.0, 0.0);
    builders::build(
        V2,
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[
            (b"desc", builders::tag_desc("Ink Limited")),
            (b"wtpt", builders::tag_wtpt(D50)),
            (b"A2B0", a2b.clone()),
            (b"A2B1", a2b),
            (b"B2A0", b2a_cmyk(1.0)),
            (b"B2A1", b2a_cmyk(0.8)),
        ],
    )
}

fn proof_pixels(proof: &[u8], setup: &str) -> Vec<u8> {
    let document = IccProfile::srgb().bytes().to_vec();
    let raw: Vec<u8> = [[0u8, 0, 0, 255], [20, 20, 20, 255], [128, 64, 32, 255]].concat();
    let flat = unsafe { dr_image_from_rgba(raw.as_ptr(), 3, 1) };
    assert!(!flat.is_null());
    let setup = CString::new(setup).expect("static json");
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
    assert!(
        !out.is_null(),
        "proof render failed: {}",
        take_err_string(err)
    );
    let pixels = img_pixels(out);
    unsafe {
        dr_image_free(out);
        dr_image_free(flat);
    }
    take_err_string(info);
    pixels
}

#[test]
fn a_proof_compensates_onto_the_proofs_destination_black() {
    // sRGB's black is zero and the ink-limited proof's DESTINATION black is
    // zero too, so BPC into the proof is the identity: on and off render the
    // same pixels (the output leg's BPC is off under simulate_ink_black).
    let proof = ink_limited_press_bytes();
    let on = proof_pixels(
        &proof,
        r#"{"intent":"relative","black_point_compensation":true,"simulate_ink_black":true}"#,
    );
    let off = proof_pixels(
        &proof,
        r#"{"intent":"relative","black_point_compensation":false,"simulate_ink_black":true}"#,
    );
    assert_eq!(on, off);
}

#[test]
fn a_lut_destination_pipeline_compensates_onto_its_destination_black() {
    // A `mntr` RGB LUT whose device zero is L* 10 and whose B2A1 is the
    // straight inverse of an un-lifted ramp: the relative round trip is a
    // lifted line, so its destination black is zero — sRGB's own — and BPC
    // changes nothing.
    let a2b1 = builders::tag_mft2(3, 3, 2, &|d: &[f64]| {
        let mean = (d[0] + d[1] + d[2]) / 3.0;
        vec![enc_l(10.0 + 90.0 * mean), enc_ab(0.0), enc_ab(0.0)]
    });
    let b2a1 = builders::tag_mft2(3, 3, 2, &|lab: &[f64]| vec![lab[0], lab[0], lab[0]]);
    let bytes = builders::build(
        V2,
        b"mntr",
        b"RGB ",
        b"Lab ",
        &[
            (b"desc", builders::tag_desc("Lifted RGB LUT")),
            (b"wtpt", builders::tag_wtpt(D50)),
            (b"A2B1", a2b1),
            (b"B2A1", b2a1),
        ],
    );
    let lut = std::sync::Arc::new(IccProfile::parse(&bytes).expect("an RGB LUT is storable"));
    assert!(lut.model().is_none());
    let relative = RenderingIntent::RelativeColorimetric;
    assert_neutral_black(
        icc_black_point::black_point(&lut, relative).expect("LUT black"),
        10.0,
        "the source black is the lifted device zero",
    );
    let on = ColorPipeline::new(&srgb(), &lut, relative, true).expect("prepares");
    let off = ColorPipeline::new(&srgb(), &lut, relative, false).expect("prepares");
    for v in (0u16..=255).step_by(5) {
        let rgb = [v as u8, (v / 2) as u8, (255 - v) as u8];
        assert_eq!(on.color8(rgb), off.color8(rgb), "{rgb:?}");
    }
}
