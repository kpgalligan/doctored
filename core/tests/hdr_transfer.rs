//! v1.x HDR photos: the BT.2100 transfers and the one reference-white
//! rule (`hdr_transfer`), plus the BT.2020 primaries, the general primaries
//! conversion and the luminance row (`float_primaries`).
//!
//! The oracles are the published formulas written out again here, in their
//! own spelling: SMPTE ST 2084's PQ EOTF and inverse with the constants as
//! decimals, BT.2100 Table 5's HLG inverse OETF and OOTF, and BT.2087's
//! BT.2020 → BT.709 matrix and BT.709 / BT.2020's published luma weights.

use doctored_core::float_primaries::{conversion, luminance_row, BT2020, DISPLAY_P3, REC709};
use doctored_core::hdr_transfer::{
    linear_to_pq_signal, pq_eotf_nits, pq_signal, signal_to_linear, HdrTransfer, HLG_PEAK_NITS,
    REFERENCE_WHITE_NITS,
};

/// ST 2084, the constants as published decimals.
fn pq_oracle(signal: f64) -> f64 {
    let m1 = 0.159_301_757_812_5;
    let m2 = 78.843_75;
    let c1 = 0.835_937_5;
    let c2 = 18.851_562_5;
    let c3 = 18.687_5;
    let p = signal.powf(1.0 / m2);
    10000.0 * ((p - c1).max(0.0) / (c2 - c3 * p)).powf(1.0 / m1)
}

/// BT.2100 Table 5's inverse OETF.
fn hlg_inverse_oetf(signal: f64) -> f64 {
    let a = 0.178_832_77;
    let b = 0.284_668_92;
    let c = 0.559_910_73;
    if signal <= 0.5 {
        signal * signal / 3.0
    } else {
        (((signal - c) / a).exp() + b) / 12.0
    }
}

const REC709_LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];
const BT2020_LUMA: [f64; 3] = [0.2627, 0.6780, 0.0593];

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance * b.abs().max(1.0)
}

#[test]
fn the_reference_white_rule_is_203_nits() {
    assert_eq!(REFERENCE_WHITE_NITS, 203.0);
    assert_eq!(HLG_PEAK_NITS, 1000.0);
    // BT.2408's reference: 203 cd/m² is about 58 % PQ.
    let white = pq_signal(203.0);
    assert!((white - 0.5807).abs() < 5e-4, "{white}");
    let linear = signal_to_linear(HdrTransfer::Pq, [white; 3], REC709_LUMA);
    for v in linear {
        assert!((v - 1.0).abs() < 1e-9, "{v}");
    }
}

#[test]
fn pq_matches_st_2084_across_the_range() {
    for i in 0..=1023 {
        let signal = f64::from(i) / 1023.0;
        let ours = pq_eotf_nits(signal);
        let oracle = pq_oracle(signal);
        assert!(close(ours, oracle, 1e-12), "{signal}: {ours} vs {oracle}");
    }
    assert!(close(pq_eotf_nits(1.0), 10000.0, 1e-12));
    assert_eq!(pq_eotf_nits(0.0), 0.0);
    // Published reference points: 100 cd/m² ≈ 0.5081, 1000 cd/m² ≈ 0.7518.
    assert!((pq_signal(100.0) - 0.5081).abs() < 5e-4);
    assert!((pq_signal(1000.0) - 0.7518).abs() < 5e-4);
    // The inverse is exact to rounding, and holds its input to 0…10000.
    for nits in [0.01, 1.0, 48.0, 203.0, 600.0, 4000.0, 10000.0] {
        let back = pq_oracle(pq_signal(nits));
        assert!(close(back, nits, 1e-9), "{nits}: {back}");
    }
    assert!(close(pq_signal(20000.0), 1.0, 1e-15));
    assert_eq!(pq_signal(-5.0), pq_signal(0.0));
}

#[test]
fn linear_to_pq_writes_203_nits_per_unit() {
    let signal = linear_to_pq_signal([1.0, 4.926_108, -0.5]);
    assert!(close(pq_oracle(signal[0]), 203.0, 1e-9));
    assert!(close(pq_oracle(signal[1]), 1000.0, 1e-6));
    assert_eq!(signal[2], pq_signal(0.0));
    // Past 10000 cd/m² is PQ's ceiling.
    assert!(close(linear_to_pq_signal([100.0; 3])[0], 1.0, 1e-15));
}

#[test]
fn hlg_is_the_inverse_oetf_then_the_ootf_at_1000_nits() {
    for luma in [REC709_LUMA, BT2020_LUMA] {
        for codes in [
            [0.75, 0.75, 0.75],
            [0.9, 0.4, 0.2],
            [0.3, 0.6, 1.0],
            [1.0; 3],
        ] {
            let scene = codes.map(hlg_inverse_oetf);
            let ys: f64 = (0..3).map(|c| luma[c] * scene[c]).sum();
            let expected = scene.map(|e| 1000.0 * ys.powf(0.2) * e / 203.0);
            let ours = signal_to_linear(HdrTransfer::Hlg, codes, luma);
            for c in 0..3 {
                assert!(
                    close(ours[c], expected[c], 1e-6),
                    "{codes:?} {c}: {} vs {}",
                    ours[c],
                    expected[c]
                );
            }
        }
    }
    // A 75 % HLG grey is reference white, within BT.2408's rounding.
    let grey = signal_to_linear(HdrTransfer::Hlg, [0.75; 3], BT2020_LUMA);
    assert!((grey[0] - 1.0).abs() < 0.002, "{grey:?}");
    // HLG white is the 1000 cd/m² peak.
    let peak = signal_to_linear(HdrTransfer::Hlg, [1.0; 3], BT2020_LUMA);
    assert!(close(peak[0] * 203.0, 1000.0, 1e-6), "{peak:?}");
    assert_eq!(
        signal_to_linear(HdrTransfer::Hlg, [0.0; 3], BT2020_LUMA),
        [0.0; 3]
    );
}

#[test]
fn a_transfer_outside_pq_and_hlg_refuses_by_name() {
    assert_eq!(HdrTransfer::parse("pq"), Ok(HdrTransfer::Pq));
    assert_eq!(HdrTransfer::parse("hlg"), Ok(HdrTransfer::Hlg));
    assert_eq!(
        HdrTransfer::parse("srgb").unwrap_err(),
        "HDR transfer \"srgb\" is not one of pq or hlg"
    );
}

#[test]
fn bt2020_luminance_and_the_bt2087_conversion() {
    let row = luminance_row(&BT2020).unwrap();
    for c in 0..3 {
        assert!((row[c] - BT2020_LUMA[c]).abs() < 1e-4, "{row:?}");
    }
    let row = luminance_row(&REC709).unwrap();
    for c in 0..3 {
        assert!((row[c] - REC709_LUMA[c]).abs() < 1e-4, "{row:?}");
    }
    // ITU-R BT.2087's BT.2020 → BT.709 matrix (same D65 white, so no
    // adaptation enters).
    let published = [
        [1.6605, -0.5876, -0.0728],
        [-0.1246, 1.1329, -0.0083],
        [-0.0182, -0.1006, 1.1187],
    ];
    let m = conversion(&BT2020, &REC709).unwrap();
    for i in 0..3 {
        for k in 0..3 {
            assert!((m[i][k] - published[i][k]).abs() < 2e-4, "{m:?}");
        }
    }
    // There and back is the identity; a space to itself is the identity —
    // to the precision of the crate's published Bradford inverse, whose
    // rounding leaves about 1e-7 even between equal whites.
    let back = conversion(&REC709, &BT2020).unwrap();
    for i in 0..3 {
        for k in 0..3 {
            let product: f64 = (0..3).map(|j| m[i][j] * back[j][k]).sum();
            assert!((product - f64::from(u8::from(i == k))).abs() < 1e-6);
            let same = conversion(&DISPLAY_P3, &DISPLAY_P3).unwrap();
            assert!((same[i][k] - f64::from(u8::from(i == k))).abs() < 1e-6);
        }
    }
    let collinear = [[0.3, 0.3], [0.3, 0.3], [0.3, 0.3], [0.3127, 0.3290]];
    assert!(conversion(&collinear, &REC709).is_none());
    assert!(luminance_row(&collinear).is_none());
}
