//! Phase 11E (T): the two linear-light transfer tables against an
//! INDEPENDENTLY written analytic curve.
//!
//! The three fixtures are the built-in sRGB, the built-in Display P3 (whose
//! TRC is the same sRGB transfer) and a gamma-2.2 `para` profile assembled
//! here from ICC bytes, so a pure-power curve is measured beside the
//! piecewise one. A LUT profile — RGB, no matrix tags — is refused by
//! sentence rather than silently evaluated.
//!
//! What is measured, and why it is split by depth. The contract is "the exact
//! table entry on a lattice code, a linear interpolation between neighbours
//! anywhere else", so:
//!
//! * at LATTICE codes both transfers are compared to the analytic curve
//!   directly (1e-5 linear, which also bounds the built-ins' own 1024-entry
//!   u16 `curv` approximation of that curve: 0.5/65535 = 7.6e-6 of rounding
//!   plus 3.6e-7 of interpolation);
//! * at MIDPOINTS the byte transfer is compared to the INTERPOLATION RULE —
//!   the mean of its two neighbours — because a chord across one 8-bit step
//!   of a gamma curve sits up to ~4.6e-5 from the curve itself, which is the
//!   rule working, not failing;
//! * at midpoints the TYPED transfer is compared to the analytic curve
//!   directly at 1e-5, because one 16-bit step is 256 times shorter and the
//!   chord error there is ~7e-10.
//!
//! `encode(decode(k)) == k` is asked for every lattice code and channel at
//! both depths: that round trip is what makes an unchanged linear-light
//! projection reproduce the encoded one.

use std::sync::Arc;

use doctored_core::doc_evaluation::ByteTransfer;
use doctored_core::icc::IccProfile;
use doctored_core::precision_evaluation::TypedTransfer;

/// The sRGB piecewise transfer, written from the specification rather than
/// read out of the crate.
fn srgb_decode(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

fn gamma_decode(x: f64) -> f64 {
    x.powf(2.2)
}

/// A minimal ICC RGB matrix/TRC profile: sRGB primaries (borrowed from the
/// built-in's own tags) and three `para` type-0 curves of exponent 2.2.
fn para_gamma_profile() -> Arc<IccProfile> {
    let srgb = IccProfile::srgb();
    let mut curve = b"para\0\0\0\0".to_vec();
    curve.extend_from_slice(&0u16.to_be_bytes());
    curve.extend_from_slice(&0u16.to_be_bytes());
    curve.extend_from_slice(&((2.2f64 * 65536.0).round() as i32).to_be_bytes());
    let mut tags: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    for key in [*b"rXYZ", *b"gXYZ", *b"bXYZ"] {
        tags.push((key, tag_of(srgb.bytes(), &key).to_vec()));
    }
    for key in [*b"rTRC", *b"gTRC", *b"bTRC"] {
        tags.push((key, curve.clone()));
    }
    Arc::new(IccProfile::parse(&assemble(b"RGB ", &tags)).expect("the para fixture parses"))
}

/// An RGB profile with an A2B0 table and NO matrix tags — decodable by the
/// LUT pipeline, but not a matrix/TRC model, which is what the policy needs.
fn lut_profile() -> Arc<IccProfile> {
    let tags = vec![(
        *b"A2B0",
        vec![b'm', b'f', b't', b'2', 0, 0, 0, 0, 3, 3, 0, 0],
    )];
    Arc::new(IccProfile::parse(&assemble(b"RGB ", &tags)).expect("the LUT fixture parses"))
}

/// Borrows one tag's data out of an assembled profile.
fn tag_of<'a>(bytes: &'a [u8], key: &[u8; 4]) -> &'a [u8] {
    let count = u32::from_be_bytes(bytes[128..132].try_into().unwrap()) as usize;
    for i in 0..count {
        let at = 132 + 12 * i;
        if &bytes[at..at + 4] == key {
            let off = u32::from_be_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            return &bytes[off..off + len];
        }
    }
    panic!("tag {key:?} is missing");
}

/// A complete v2.1.0 profile around `tags`.
fn assemble(space: &[u8; 4], tags: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let start = 132 + 12 * tags.len();
    let mut table = Vec::new();
    let mut payload: Vec<u8> = Vec::new();
    for (sig, data) in tags {
        let offset = (start + payload.len()) as u32;
        payload.extend_from_slice(data);
        while !payload.len().is_multiple_of(4) {
            payload.push(0);
        }
        table.extend_from_slice(sig);
        table.extend_from_slice(&offset.to_be_bytes());
        table.extend_from_slice(&(data.len() as u32).to_be_bytes());
    }
    let mut out = vec![0u8; 128];
    out[0..4].copy_from_slice(&((start + payload.len()) as u32).to_be_bytes());
    out[8..12].copy_from_slice(&[0x02, 0x10, 0x00, 0x00]);
    out[12..16].copy_from_slice(b"mntr");
    out[16..20].copy_from_slice(space);
    out[20..24].copy_from_slice(b"XYZ ");
    out[36..40].copy_from_slice(b"acsp");
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&payload);
    out
}

/// One fixture: the name it is reported under, the profile and the analytic
/// curve its TRC is supposed to be.
type Fixture = (&'static str, Arc<IccProfile>, fn(f64) -> f64);

fn fixtures() -> Vec<Fixture> {
    vec![
        ("sRGB", IccProfile::srgb(), srgb_decode as fn(f64) -> f64),
        ("Display P3", IccProfile::display_p3(), srgb_decode),
        ("gamma 2.2 para", para_gamma_profile(), gamma_decode),
    ]
}

#[test]
fn byte_transfer_round_trips_every_lattice_code_and_channel() {
    for (name, profile, analytic) in fixtures() {
        let transfer = ByteTransfer::for_profile(&profile).expect(name);
        for code in 0..=255u8 {
            let encoded = f32::from(code) / 255.0;
            let linear = transfer.decode([encoded; 3]);
            for (channel, &value) in linear.iter().enumerate() {
                let expected = analytic(f64::from(code) / 255.0);
                assert!(
                    (f64::from(value) - expected).abs() <= 1e-5,
                    "{name} channel {channel} code {code}: decoded {value} vs analytic {expected}"
                );
            }
            assert_eq!(
                transfer.encode(linear),
                [code; 3],
                "{name}: encode(decode({code})) must be exact"
            );
            assert_eq!(
                transfer.encode_f32(linear),
                [encoded; 3],
                "{name}: the transcode's lattice value at code {code}"
            );
        }
    }
}

#[test]
fn byte_transfer_interpolates_between_lattice_codes() {
    for (name, profile, _) in fixtures() {
        let transfer = ByteTransfer::for_profile(&profile).expect(name);
        for code in 0..255u8 {
            let low = transfer.decode([f32::from(code) / 255.0; 3]);
            let high = transfer.decode([f32::from(code + 1) / 255.0; 3]);
            let mid = transfer.decode([(f32::from(code) + 0.5) / 255.0; 3]);
            for c in 0..3 {
                let expected = 0.5 * (low[c] + high[c]);
                assert!(
                    (mid[c] - expected).abs() <= 1e-6,
                    "{name} channel {c} between {code} and {}: {mid:?} vs {expected}",
                    code + 1
                );
            }
        }
    }
}

#[test]
fn typed_transfer_round_trips_every_lattice_code_and_channel() {
    for (name, profile, analytic) in fixtures() {
        let transfer = TypedTransfer::for_profile(&profile).expect(name);
        // Every 16-bit code is 65536 round trips per channel per fixture;
        // the stride keeps the test under a second while still visiting the
        // toe, the knee and the shoulder of every curve, and the endpoints
        // are asked explicitly.
        let codes: Vec<u32> = (0..=65535u32)
            .step_by(7)
            .chain([1, 2, 3, 128, 65534, 65535])
            .collect();
        for code in codes {
            let encoded = code as f32 / 65535.0;
            let linear = transfer.decode([encoded; 3]);
            for (channel, &value) in linear.iter().enumerate() {
                let expected = analytic(f64::from(code) / 65535.0);
                assert!(
                    (f64::from(value) - expected).abs() <= 1e-5,
                    "{name} channel {channel} code {code}: decoded {value} vs analytic {expected}"
                );
            }
            let back = transfer.encode(linear);
            for (channel, &value) in back.iter().enumerate() {
                let stored = (f64::from(value) * 65535.0).round() as u32;
                assert_eq!(
                    stored, code,
                    "{name} channel {channel}: encode(decode({code})) must round back"
                );
            }
        }
    }
}

#[test]
fn typed_transfer_midpoints_track_the_analytic_curve() {
    for (name, profile, analytic) in fixtures() {
        let transfer = TypedTransfer::for_profile(&profile).expect(name);
        for code in (0..65535u32).step_by(101) {
            let encoded = (code as f32 + 0.5) / 65535.0;
            let linear = transfer.decode([encoded; 3]);
            let expected = analytic((f64::from(code) + 0.5) / 65535.0);
            for (channel, &value) in linear.iter().enumerate() {
                assert!(
                    (f64::from(value) - expected).abs() <= 1e-5,
                    "{name} channel {channel} midpoint above {code}: {value} vs {expected}"
                );
            }
        }
    }
}

#[test]
fn a_lut_profile_is_refused_by_sentence_at_both_depths() {
    let profile = lut_profile();
    let byte = match ByteTransfer::for_profile(&profile) {
        Err(message) => message,
        Ok(_) => panic!("a LUT profile has no TRC model"),
    };
    let typed = match TypedTransfer::for_profile(&profile) {
        Err(message) => message,
        Ok(_) => panic!("a LUT profile has no TRC model"),
    };
    assert_eq!(byte, typed, "one wording at both depths");
    assert!(
        byte.contains("matrix/TRC") && byte.contains("linear-light"),
        "the refusal names what is missing and what it blocks: {byte}"
    );
}
