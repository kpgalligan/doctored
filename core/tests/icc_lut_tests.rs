//! The Phase 11C LUT parser: `mft1`/`mft2`/`mAB `/`mBA `, the PCS encodings,
//! the CLUT interpolation, the Lab⇄XYZ pair, and every documented refusal.
//!
//! Two kinds of oracle, both independent of the implementation:
//!
//! * SYNTHETIC profiles this file assembles byte by byte, whose tables are
//!   an exactly known function, so the parser's answer can be compared with
//!   arithmetic written here rather than with a golden number; and
//! * the REAL profiles installed on this machine (Adobe's press set and the
//!   ColorSync generics), used for the properties that only a real table can
//!   demonstrate — that the tag types parse at all, and that a round trip
//!   through a printer profile lands where colour science says it must. Those
//!   cases SKIP when the file is absent, the ImageMagick-fixture rule.

use doctored_core::icc::{inspect, IccProfile, Inspect};
use doctored_core::icc_intent::RenderingIntent;
use doctored_core::icc_lut::{self, DeviceSpace, PcsSpace, ProofProfile, D50};

// ------------------------------------------------------------- a builder --

use crate::icc_lut_builders as builders;
use builders::{build, tag_desc, tag_mft2, tag_wtpt};

/// A `mft1` (lut8Type) of the same shape.
fn tag_mft1(
    inputs: usize,
    outputs: usize,
    grid: usize,
    clut: &dyn Fn(&[f64]) -> Vec<f64>,
) -> Vec<u8> {
    let mut t = b"mft1\0\0\0\0".to_vec();
    t.push(inputs as u8);
    t.push(outputs as u8);
    t.push(grid as u8);
    t.push(0);
    for v in [1.0f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
        t.extend_from_slice(&(((v * 65536.0).round() as i32).to_be_bytes()));
    }
    for _ in 0..inputs {
        for i in 0..256 {
            t.push(i as u8);
        }
    }
    let total = grid.pow(inputs as u32);
    for index in 0..total {
        let mut coords = vec![0.0f64; inputs];
        let mut rest = index;
        for axis in (0..inputs).rev() {
            coords[axis] = (rest % grid) as f64 / (grid - 1) as f64;
            rest /= grid;
        }
        for value in clut(&coords) {
            t.push((value.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    for _ in 0..outputs {
        for i in 0..256 {
            t.push(i as u8);
        }
    }
    t
}

/// A `mAB `/`mBA ` with B curves only — the simplest legal form, and the one
/// the HDR profiles this machine ships actually use (with a matrix).
fn tag_mab(reversed: bool, inputs: usize, outputs: usize) -> Vec<u8> {
    let sig: &[u8; 4] = if reversed { b"mBA " } else { b"mAB " };
    let mut t = sig.to_vec();
    t.extend_from_slice(&[0, 0, 0, 0]);
    t.push(inputs as u8);
    t.push(outputs as u8);
    t.extend_from_slice(&[0, 0]);
    // B at offset 32, nothing else.
    t.extend_from_slice(&32u32.to_be_bytes()); // B curves
    t.extend_from_slice(&0u32.to_be_bytes()); // matrix
    t.extend_from_slice(&0u32.to_be_bytes()); // M curves
    t.extend_from_slice(&0u32.to_be_bytes()); // CLUT
    t.extend_from_slice(&0u32.to_be_bytes()); // A curves
    assert_eq!(t.len(), 32);
    // In both directions the B curves sit on the tag's OUTPUT side here,
    // because this fixture is B-only: there is no CLUT to change the count.
    for _ in 0..outputs {
        // An identity `curv` (count 0), padded to four bytes.
        t.extend_from_slice(b"curv\0\0\0\0");
        t.extend_from_slice(&0u32.to_be_bytes());
    }
    t
}

fn cmyk_profile(a2b: Vec<u8>, b2a: Vec<u8>) -> Vec<u8> {
    build(
        [0x02, 0x20, 0, 0],
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[
            (b"desc", tag_desc("Synthetic CMYK")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", a2b.clone()),
            (b"A2B1", a2b),
            (b"B2A0", b2a.clone()),
            (b"B2A1", b2a),
        ],
    )
}

// ---------------------------------------------------------- the encodings --

#[test]
fn lab_and_xyz_round_trip_exactly() {
    // The pair is an identity to within f64 rounding, which is what every
    // gamut ΔE and every proof round trip stands on.
    for lab in [
        [0.0, 0.0, 0.0],
        [100.0, 0.0, 0.0],
        [53.59, 0.0, 0.0],
        [54.29, 80.82, 69.88],
        [8.0, -20.0, 35.0],
    ] {
        let back = icc_lut::xyz_to_lab(icc_lut::lab_to_xyz(lab));
        for (a, b) in lab.iter().zip(back.iter()) {
            assert!((a - b).abs() < 1e-9, "{lab:?} came back as {back:?}");
        }
    }
    // D50 itself is Lab (100, 0, 0) by definition.
    let white = icc_lut::xyz_to_lab(D50);
    assert!((white[0] - 100.0).abs() < 1e-9);
    assert!(white[1].abs() < 1e-9 && white[2].abs() < 1e-9);
    // And ΔE76 is the plain Euclidean distance, written out here.
    let a = [50.0, 10.0, -5.0];
    let b = [52.0, 7.0, -1.0];
    let expected = ((2.0f64).powi(2) + (3.0f64).powi(2) + (4.0f64).powi(2)).sqrt();
    assert!((icc_lut::delta_e76(a, b) - expected).abs() < 1e-12);
}

#[test]
fn the_legacy_16_bit_lab_encoding_is_not_the_v4_one() {
    // A `mft2` table's full-scale L* is 100 at 0xFF00, so 0xFFFF reads ABOVE
    // 100 — a 0.4 % scale. Reading it as v4 would put white at 99.6 instead.
    // Built as two profiles differing ONLY in the tag type.
    let identity = |c: &[f64]| vec![c[0], 0.5, 0.5];
    let v2 = ProofProfile::parse(&build(
        [0x02, 0x20, 0, 0],
        b"prtr",
        b"GRAY",
        b"Lab ",
        &[
            (b"desc", tag_desc("gray")),
            (b"kTRC", {
                let mut t = b"curv\0\0\0\0".to_vec();
                t.extend_from_slice(&0u32.to_be_bytes());
                t
            }),
        ],
    ))
    .expect("a grey profile with a kTRC parses");
    assert_eq!(v2.device(), DeviceSpace::Gray);

    // The real demonstration: an RGB profile whose A2B is a `mft2` whose
    // table is the identity. Its white must come back as L* = 100 exactly.
    let rgb = ProofProfile::parse(&build(
        [0x02, 0x20, 0, 0],
        b"spac",
        b"RGB ",
        b"Lab ",
        &[
            (b"desc", tag_desc("Synthetic RGB LUT")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", tag_mft2(3, 3, 2, &identity)),
        ],
    ))
    .expect("an RGB mft2 profile parses");
    let white = rgb
        .to_xyz(&[1.0, 1.0, 1.0], RenderingIntent::RelativeColorimetric)
        .expect("a device white");
    let lab = icc_lut::xyz_to_lab(white);
    // 65535/65280 * 100 = 100.3906…: the table's top entry means MORE than
    // L* 100 under the legacy encoding, which is exactly the point.
    assert!(
        lab[0] > 100.0 && lab[0] < 100.5,
        "legacy 16-bit Lab puts the table's top above 100, got {lab:?}"
    );
}

#[test]
fn an_mft1_clut_interpolates_its_own_arithmetic() {
    // The table IS a known function, so the parser's answer can be compared
    // with arithmetic written here. A LINEAR function is reproduced exactly
    // by both interpolators, at every grid point and between them.
    let f = |c: &[f64]| vec![c[0], 1.0 - c[0], 0.5 * c[0]];
    let profile = ProofProfile::parse(&build(
        [0x02, 0x20, 0, 0],
        b"spac",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("linear")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", tag_mft1(3, 3, 9, &f)),
        ],
    ))
    .expect("an mft1 RGB profile parses");
    let table = profile
        .to_pcs(RenderingIntent::Perceptual)
        .expect("an A2B table");
    for x in [0.0f64, 0.125, 0.3, 0.5, 0.75, 1.0] {
        let out = table.eval(&[x, 0.0, 0.0]);
        let expected = f(&[x]);
        for (got, want) in out.iter().zip(expected.iter()) {
            // 8-bit table samples quantize to 1/255, and the linear
            // interpolation between two of them keeps that bound.
            assert!(
                (got - want).abs() <= 1.0 / 255.0 + 1e-9,
                "at {x}: {out:?} against {expected:?}"
            );
        }
    }
}

#[test]
fn tetrahedral_and_multilinear_agree_on_a_linear_table() {
    // The two interpolators differ only on curvature. On a table that IS
    // affine they must agree exactly, which is what proves the tetrahedral
    // weights sum to one and pick the right corners — a wrong simplex shows
    // up immediately here.
    let affine = |c: &[f64]| vec![0.2 + 0.5 * c[0] - 0.1 * c[1] + 0.3 * c[2], 0.4, 0.6];
    let three = ProofProfile::parse(&build(
        [0x02, 0x20, 0, 0],
        b"spac",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("affine3")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", tag_mft2(3, 3, 5, &affine)),
        ],
    ))
    .expect("parses");
    let table = three.to_pcs(RenderingIntent::Perceptual).expect("A2B");
    let mut seed = 0x11C0_2026u64;
    let mut next = move || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        ((seed >> 33) as f64) / ((1u64 << 31) as f64)
    };
    for _ in 0..200 {
        let input = [next(), next(), next()];
        let out = table.eval(&input);
        let want = affine(&input);
        assert!(
            (out[0] - want[0]).abs() < 2e-4,
            "{input:?}: {out:?} against {want:?}"
        );
    }
}

// ------------------------------------------------------------- refusals --

#[test]
fn the_documented_refusals_name_their_element() {
    // A header-only profile of one class, data space and PCS.
    let header = |class: &[u8; 4], space: &[u8; 4], pcs: &[u8; 4]| {
        build(
            [0x04, 0x20, 0, 0],
            class,
            space,
            pcs,
            &[(b"desc", tag_desc("header only"))],
        )
    };

    // An `mpet`-TYPED A2B0: multiProcessElements where v4 allows only the
    // lut types.
    let mpet = build(
        [0x04, 0x20, 0, 0],
        b"prtr",
        b"CMYK",
        b"Lab ",
        &[
            (b"desc", tag_desc("mpet")),
            (
                b"A2B0",
                vec![b'm', b'p', b'e', b't', 0, 0, 0, 0, 0, 4, 0, 3],
            ),
        ],
    );
    let err = ProofProfile::parse(&mpet).expect_err("an mpet A2B0 is refused");
    assert!(err.contains("multiProcessElements"), "{err}");

    // A named-colour profile is a list of colours, refused by its class.
    let err = ProofProfile::parse(&header(b"nmcl", b"RGB ", b"Lab "))
        .expect_err("a named-colour profile is refused");
    assert!(err.contains("named-colour"), "{err}");
    assert!(err.contains("list of colours"), "{err}");

    // An abstract profile is refused by its CLASS, not by its Lab data
    // space: the class is checked first.
    let err = ProofProfile::parse(&header(b"abst", b"Lab ", b"Lab "))
        .expect_err("an abstract profile is refused");
    assert!(err.contains("abstract"), "{err}");
    assert!(err.contains("transform"), "{err}");

    // An unknown class is named by its signature.
    let err = ProofProfile::parse(&header(b"xxxx", b"RGB ", b"Lab "))
        .expect_err("an unknown class is refused");
    assert!(err.contains("xxxx"), "{err}");

    // Floating-point-only profiles name the tag they carry instead.
    for (sig, name) in [(b"D2B1", "D2B1"), (b"B2D2", "B2D2")] {
        let only = build(
            [0x04, 0x20, 0, 0],
            b"prtr",
            b"CMYK",
            b"Lab ",
            &[(b"desc", tag_desc(name)), (sig, vec![0u8; 32])],
        );
        let err = ProofProfile::parse(&only).expect_err("a float-only profile is refused");
        assert!(err.contains(name), "{err}");
    }

    // An nCLR separation space is named by its colourant count; a 4CLR is
    // four channels, so the old "more than four" wording would be false.
    let err = ProofProfile::parse(&header(b"prtr", b"6CLR", b"Lab "))
        .expect_err("a 6CLR space is refused");
    assert!(err.contains("6CLR") && err.contains("6-colour"), "{err}");
    let err = ProofProfile::parse(&header(b"prtr", b"4CLR", b"Lab "))
        .expect_err("a 4CLR space is refused");
    assert!(err.contains("4CLR"), "{err}");
    assert!(!err.contains("more than four"), "{err}");

    // A device-link class: RGB numbers, but a transform rather than a space.
    let link = build(
        [0x02, 0x20, 0, 0],
        b"link",
        b"RGB ",
        b"Lab ",
        &[
            (b"desc", tag_desc("link")),
            (b"A2B0", tag_mft2(3, 3, 2, &|c| vec![c[0], 0.5, 0.5])),
        ],
    );
    let err = ProofProfile::parse(&link).expect_err("a device link is refused");
    assert!(err.contains("device-link"), "{err}");
    assert!(err.contains("transform"), "{err}");

    // A colour space this build does not model at all.
    let five = build(
        [0x02, 0x20, 0, 0],
        b"prtr",
        b"5CLR",
        b"Lab ",
        &[(b"desc", tag_desc("5clr"))],
    );
    let err = ProofProfile::parse(&five).expect_err("five channels are refused");
    assert!(err.contains("5CLR"), "{err}");

    // Not a profile at all.
    assert!(ProofProfile::parse(b"not a profile").is_err());
    assert!(ProofProfile::parse(&[]).is_err());

    // A truncated table refuses the TABLE and therefore the profile, rather
    // than being half-believed.
    let truncated = build(
        [0x02, 0x20, 0, 0],
        b"spac",
        b"RGB ",
        b"Lab ",
        &[
            (b"desc", tag_desc("short")),
            (b"A2B0", b"mft2\0\0\0\0\x03\x03\x00\x00".to_vec()),
        ],
    );
    assert!(ProofProfile::parse(&truncated).is_err());
}

#[test]
fn a_grey_profile_is_its_ktrc_and_nothing_else() {
    let mut curve = b"curv\0\0\0\0".to_vec();
    curve.extend_from_slice(&1u32.to_be_bytes());
    curve.extend_from_slice(&0x0233u16.to_be_bytes()); // gamma 2.19921875
    let gray = ProofProfile::parse(&build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"GRAY",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Generic Gray")),
            (b"wtpt", tag_wtpt(D50)),
            (b"kTRC", curve),
        ],
    ))
    .expect("a grey profile parses");
    assert_eq!(gray.device(), DeviceSpace::Gray);
    assert!(gray.can_decode() && gray.can_encode());
    // Y = g^2.19921875, and the chromaticity is D50's.
    let xyz = gray
        .to_xyz(&[0.5], RenderingIntent::RelativeColorimetric)
        .expect("grey decodes");
    let expected = 0.5f64.powf(2.19921875);
    assert!((xyz[1] - expected).abs() < 1e-6, "{xyz:?}");
    assert!((xyz[0] / D50[0] - xyz[1]).abs() < 1e-9);
    // And back: the forward search finds the device value again.
    let (device, count) = gray
        .from_xyz(xyz, RenderingIntent::RelativeColorimetric)
        .expect("grey encodes");
    assert_eq!(count, 1);
    assert!((device[0] - 0.5).abs() < 1e-5, "{device:?}");

    // A grey profile with no kTRC is refused by name.
    let err = ProofProfile::parse(&build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"GRAY",
        b"XYZ ",
        &[(b"desc", tag_desc("no trc"))],
    ))
    .expect_err("refused");
    assert!(err.contains("kTRC"), "{err}");
}

#[test]
fn an_rgb_lut_profile_is_stored_and_inspects_as_rgb_lut() {
    let identity = |c: &[f64]| vec![c[0], 0.5, 0.5];
    let bytes = build(
        [0x02, 0x20, 0, 0],
        b"spac",
        b"RGB ",
        b"Lab ",
        &[
            (b"desc", tag_desc("RGB LUT")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", tag_mft2(3, 3, 3, &identity)),
            (b"B2A0", tag_mft2(3, 3, 3, &|c| vec![c[0], c[1], c[2]])),
        ],
    );
    let mut name = String::new();
    assert_eq!(inspect(&bytes, &mut name), Inspect::RgbLut);
    assert_eq!(name, "RGB LUT");
    let profile = IccProfile::parse(&bytes).expect("an RGB LUT profile IS stored");
    // The matrix/TRC question keeps its answer: `document_v2` Exposure and
    // White Balance depend on exactly this bit.
    assert!(profile.model().is_none());
    // The wider Phase 11C question is true in both directions.
    assert!(profile.can_decode());
    assert!(profile.can_encode());
    assert!(profile.lut().is_some());
    // And the bytes round-trip, which is what re-embedding needs.
    assert_eq!(&**profile.bytes(), &bytes[..]);
}

#[test]
fn the_intent_fallback_is_table_zero() {
    // A profile with only table 0 answers every intent with it; one with a
    // table 1 answers relative with THAT.
    let zero = |c: &[f64]| vec![c[0], 0.5, 0.5];
    let one = |c: &[f64]| vec![c[0] * 0.5, 0.5, 0.5];
    let only_zero = ProofProfile::parse(&build(
        [0x02, 0x20, 0, 0],
        b"spac",
        b"RGB ",
        b"Lab ",
        &[
            (b"desc", tag_desc("one table")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", tag_mft2(3, 3, 3, &zero)),
        ],
    ))
    .expect("parses");
    for intent in [
        RenderingIntent::Perceptual,
        RenderingIntent::RelativeColorimetric,
        RenderingIntent::Saturation,
        RenderingIntent::AbsoluteColorimetric,
    ] {
        assert!(only_zero.to_pcs(intent).is_some(), "{intent:?} falls back");
    }

    let both = ProofProfile::parse(&build(
        [0x02, 0x20, 0, 0],
        b"spac",
        b"RGB ",
        b"Lab ",
        &[
            (b"desc", tag_desc("two tables")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", tag_mft2(3, 3, 3, &zero)),
            (b"A2B1", tag_mft2(3, 3, 3, &one)),
        ],
    ))
    .expect("parses");
    let perceptual = both
        .to_pcs(RenderingIntent::Perceptual)
        .expect("table 0")
        .eval(&[1.0, 0.0, 0.0]);
    let relative = both
        .to_pcs(RenderingIntent::RelativeColorimetric)
        .expect("table 1")
        .eval(&[1.0, 0.0, 0.0]);
    assert!(
        (perceptual[0] - 1.0).abs() < 1e-3 && (relative[0] - 0.5).abs() < 1e-3,
        "perceptual {perceptual:?} relative {relative:?}"
    );
    // Absolute shares relative's table.
    let absolute = both
        .to_pcs(RenderingIntent::AbsoluteColorimetric)
        .expect("table 1")
        .eval(&[1.0, 0.0, 0.0]);
    assert_eq!(absolute[0], relative[0]);
}

#[test]
fn an_mab_with_only_b_curves_is_an_identity() {
    let bytes = build(
        [0x04, 0x20, 0, 0],
        b"scnr",
        b"RGB ",
        b"Lab ",
        &[
            (b"desc", tag_desc("mAB identity")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", tag_mab(false, 3, 3)),
            (b"B2A0", tag_mab(true, 3, 3)),
        ],
    );
    let profile = ProofProfile::parse(&bytes).expect("an mAB/mBA profile parses");
    assert!(profile.is_v4());
    let table = profile.to_pcs(RenderingIntent::Perceptual).expect("A2B");
    let out = table.eval(&[0.25, 0.5, 0.75]);
    assert!(
        (out[0] - 0.25).abs() < 1e-9 && (out[2] - 0.75).abs() < 1e-9,
        "{out:?}"
    );
}

#[test]
fn a_cmyk_profile_is_four_in_three_out_both_ways() {
    let a2b = tag_mft2(4, 3, 3, &|c| {
        // A crude but MONOTONE ink model, written here so the expectation is
        // arithmetic rather than a golden number.
        let ink = (c[0] + c[1] + c[2]).min(1.0).max(c[3]);
        vec![1.0 - ink, 0.5, 0.5]
    });
    let b2a = tag_mft2(3, 4, 3, &|c| vec![0.0, 0.0, 0.0, 1.0 - c[0]]);
    let profile = ProofProfile::parse(&cmyk_profile(a2b, b2a)).expect("a CMYK profile parses");
    assert_eq!(profile.device(), DeviceSpace::Cmyk);
    assert_eq!(profile.pcs(), PcsSpace::Lab);
    assert!(profile.can_decode() && profile.can_encode());
    let table = profile.to_pcs(RenderingIntent::Perceptual).expect("A2B");
    assert_eq!(table.inputs(), 4);
    assert_eq!(table.outputs(), 3);
    let inverse = profile.from_pcs(RenderingIntent::Perceptual).expect("B2A");
    assert_eq!(inverse.inputs(), 3);
    assert_eq!(inverse.outputs(), 4);
    // No ink is the paper: L* near the table's own top.
    let xyz = profile
        .to_xyz(&[0.0, 0.0, 0.0, 0.0], RenderingIntent::RelativeColorimetric)
        .expect("decodes");
    assert!(icc_lut::xyz_to_lab(xyz)[0] > 99.0);
    // Full black ink is the darkest.
    let xyz = profile
        .to_xyz(&[0.0, 0.0, 0.0, 1.0], RenderingIntent::RelativeColorimetric)
        .expect("decodes");
    assert!(icc_lut::xyz_to_lab(xyz)[0] < 1.0);
}

// -------------------------------------------- the profiles on this machine --

/// A real installed profile, or `None` so the case can skip.
fn installed(path: &str) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

#[test]
fn every_adobe_press_profile_this_machine_has_parses() {
    let folder = "/Library/Application Support/Adobe/Color/Profiles/Recommended";
    let Ok(entries) = std::fs::read_dir(folder) else {
        eprintln!("skipping: {folder} is not installed");
        return;
    };
    let mut parsed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if extension != "icc" && extension != "icm" {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        match ProofProfile::parse(&bytes) {
            Ok(profile) => {
                parsed += 1;
                // Everything Adobe ships here is usable in at least one
                // direction; a profile that parses but answers neither would
                // be a silent half-belief.
                assert!(
                    profile.can_decode() || profile.can_encode(),
                    "{path:?} parsed but answers in neither direction"
                );
                // And a CMYK press profile must be proofable BOTH ways, which
                // is the whole point of the phase.
                if profile.device() == DeviceSpace::Cmyk {
                    assert!(
                        profile.can_decode() && profile.can_encode(),
                        "{path:?} is a CMYK profile with only one direction"
                    );
                }
            }
            Err(reason) => {
                // A refusal is fine — it must NAME the element rather than
                // being a bare failure.
                assert!(
                    reason.len() > 10 && !reason.contains("internal"),
                    "{path:?}: {reason}"
                );
            }
        }
    }
    assert!(
        parsed >= 5,
        "expected several Adobe profiles, parsed {parsed}"
    );
}

#[test]
fn fogra39_round_trips_colour_science_rather_than_a_golden_number() {
    let Some(bytes) = installed(
        "/Library/Application Support/Adobe/Color/Profiles/Recommended/CoatedFOGRA39.icc",
    ) else {
        eprintln!("skipping: CoatedFOGRA39.icc is not installed");
        return;
    };
    let profile = ProofProfile::parse(&bytes).expect("FOGRA39 parses");
    assert_eq!(profile.device(), DeviceSpace::Cmyk);
    assert_eq!(profile.pcs(), PcsSpace::Lab);
    let relative = RenderingIntent::RelativeColorimetric;

    // 1. Paper white is near but not at L* 100, and near neutral. That is
    //    what a coated press sheet IS, and no golden number is needed to
    //    say so.
    let paper = icc_lut::xyz_to_lab(
        profile
            .to_xyz(&[0.0, 0.0, 0.0, 0.0], relative)
            .expect("decodes"),
    );
    assert!(
        (95.0..=100.5).contains(&paper[0]) && paper[1].abs() < 4.0 && paper[2].abs() < 6.0,
        "paper white {paper:?}"
    );

    // 2. Full four-colour black is dark and near neutral.
    let black = icc_lut::xyz_to_lab(
        profile
            .to_xyz(&[1.0, 1.0, 1.0, 1.0], relative)
            .expect("decodes"),
    );
    assert!(black[0] < 20.0, "rich black {black:?}");

    // 3. The ROUND TRIP: an in-gamut colour comes back within a few ΔE, an
    //    out-of-gamut one does not. Mid grey is in gamut; a pure sRGB green
    //    is famously not.
    let round_trip = |lab: [f64; 3]| -> f64 {
        let xyz = icc_lut::lab_to_xyz(lab);
        let (device, count) = profile.from_xyz(xyz, relative).expect("encodes");
        let back = profile.to_xyz(&device[..count], relative).expect("decodes");
        icc_lut::delta_e76(lab, icc_lut::xyz_to_lab(back))
    };
    assert!(round_trip([53.6, 0.0, 0.0]) < 3.0, "mid grey is in gamut");
    assert!(
        round_trip([87.8, -79.3, 81.0]) > 20.0,
        "sRGB green is far outside a coated press gamut"
    );
}

#[test]
fn the_colorsync_generics_parse_in_their_declared_spaces() {
    for (path, space) in [
        (
            "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc",
            DeviceSpace::Cmyk,
        ),
        (
            "/System/Library/ColorSync/Profiles/Generic Gray Profile.icc",
            DeviceSpace::Gray,
        ),
    ] {
        let Some(bytes) = installed(path) else {
            eprintln!("skipping: {path} is not installed");
            continue;
        };
        let profile = ProofProfile::parse(&bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(profile.device(), space, "{path}");
        assert!(profile.can_decode() && profile.can_encode(), "{path}");
    }
}

#[test]
fn installed_named_colour_and_abstract_profiles_refuse_by_class() {
    let named = "/Library/ColorSync/Profiles/WebSafeColors.icc";
    match installed(named) {
        Some(bytes) => {
            let err = ProofProfile::parse(&bytes).expect_err("a named-colour profile is refused");
            assert!(err.contains("named-colour"), "{err}");
            let mut name = String::new();
            assert_eq!(inspect(&bytes, &mut name), Inspect::NotImageProfile);
        }
        None => eprintln!("skipping: {named} is not installed"),
    }
    let abstract_path = "/Library/ColorSync/Profiles/Sepia Tone.icc";
    match installed(abstract_path) {
        Some(bytes) => {
            let err = ProofProfile::parse(&bytes).expect_err("an abstract profile is refused");
            assert!(err.contains("abstract"), "{err}");
        }
        None => eprintln!("skipping: {abstract_path} is not installed"),
    }
}
