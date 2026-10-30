//! Independent legacy appearance gate for the Gaussian kernel now also used
//! by float tone guides. This oracle performs one f64 two-dimensional sum,
//! independent of the production separable f32 passes and final byte write.
use doctored_core::ffi_doc::dr_selection_feather;

fn reference(input: &[u8], w: usize, h: usize, radius: f32, taps: i64) -> Vec<u8> {
    let sigma = 0.3 * (f64::from(radius) - 1.0) + 0.8;
    let half = taps / 2;
    let mut output = Vec::with_capacity(input.len());
    for y in 0..h {
        for x in 0..w {
            let mut numerator = 0.0;
            let mut denominator = 0.0;
            for dy in -half..=half {
                for dx in -half..=half {
                    let weight = (-((dx * dx + dy * dy) as f64) / (2.0 * sigma * sigma)).exp();
                    let sx = (x as i64 + dx).clamp(0, w as i64 - 1) as usize;
                    let sy = (y as i64 + dy).clamp(0, h as i64 - 1) as usize;
                    numerator += f64::from(input[sy * w + sx]) * weight;
                    denominator += weight;
                }
            }
            output.push((numerator / denominator).round() as u8);
        }
    }
    output
}

#[test]
fn byte_feather_still_matches_independent_2d_gaussian_at_edges_and_interiors() {
    let (w, h) = (13, 9);
    let mut patterns = Vec::new();
    for (x, y) in [(0, 0), (w - 1, h - 1), (w / 2, h / 2), (0, h / 2)] {
        let mut input = vec![0; w * h];
        input[y * w + x] = 255;
        patterns.push(input);
    }
    patterns.push(
        (0..w * h)
            .map(|i| ((i * 73 + (i / w) * 17) % 256) as u8)
            .collect(),
    );
    // Kernel widths are specified independently for these fixed radii, including
    // the lower-odd tie at radius1.5. No production sizing helper enters the oracle.
    for (radius, taps) in [(0.25, 3), (1.0, 3), (1.5, 3), (2.0, 5), (4.25, 9)] {
        for input in &patterns {
            let expected = reference(input, w, h, radius, taps);
            let mut actual = input.clone();
            assert!(unsafe {
                dr_selection_feather(actual.as_mut_ptr(), w as u32, h as u32, radius)
            });
            for (a, b) in actual.iter().zip(expected) {
                // Independent2D f64 summation vs separable f32 may straddle one
                // final half-code, so a single output-code difference is allowed.
                assert!(
                    a.abs_diff(b) <= 1,
                    "radius{radius}: actual{a}, reference{b}"
                );
            }
        }
    }
}

#[test]
fn byte_feather_identity_and_constant_coverage_remain_exact() {
    let original: Vec<_> = (0..256).map(|i| i as u8).collect();
    for radius in [0.0, -1.0] {
        let mut actual = original.clone();
        assert!(unsafe { dr_selection_feather(actual.as_mut_ptr(), 16, 16, radius) });
        assert_eq!(actual, original);
    }
    for coverage in [0, 1, 7, 127, 128, 254, 255] {
        for radius in [0.25, 1.5, 4.25, 12.0] {
            let mut actual = vec![coverage; 63];
            assert!(unsafe { dr_selection_feather(actual.as_mut_ptr(), 9, 7, radius) });
            assert_eq!(actual, vec![coverage; 63]);
        }
    }
    assert!(!unsafe { dr_selection_feather(std::ptr::null_mut(), 1, 1, 1.0) });
}
