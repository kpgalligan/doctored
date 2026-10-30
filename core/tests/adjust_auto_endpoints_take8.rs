//! Auto Tone take 8 (`scripts/photoshop-probe/levels_auto_438_take8.py`,
//! `doctored-fixtures/photoshop/levels-auto-438/take8`, `measured.json`
//! sha256 021022fe17c964e501d2b4096a7497a4b3fdba1a390e5e05997bd6f31aaff8c4,
//! `judged.json` sha256
//! bc7dfb7b61ac0ddd88bd39b26e470d05d7c770c93d29cc272c668f719d7d8a23): fresh
//! pictures Q1-Q4 and Photoshop 26.10.0's `levl` rows, against the shipped
//! Auto Tone rule (`adjust_auto_endpoints`' module doc and oracle).
//!
//! The take's prefit refuted histogram smoothing at both ends before it ran
//! (no kernel of B3, B5, G3, G5 was measurably closer than today's on takes
//! 1-7), so today's rule stays, and these rows hold it to its own measured Q
//! residuals (`judged.json`'s `shipped_q`, max codes): white 33 at 8 and 16
//! bits, black 15 at 8 bits and 5 at 16.
//!
//! The pictures are the runner's `q_fixture()` draws spelled out:
//! `r_fixture(100 + i)` (`random.Random(4580 + i)`), each tail ending with its
//! first body bin. Q2 and Q4 also ran at 16 bits (257·c, `mask` only).

use crate::adjust_auto_endpoints::{oracle, ours16, ours8, scope, wide, Fixture};

/// The shipped rule's take-8 residuals (module doc): depth -> (white,
/// black) in codes.
fn bound8(depth: u32) -> (f64, f64) {
    match depth {
        8 => (33.0, 15.0),
        _ => (33.0, 5.0),
    }
}

/// One row: the picture (an index of [`PICTURES_Q`]), its depth and case
/// (`true` = `clip`, the photo over white), the clip and Photoshop's `levl`
/// (black, white) per channel.
type TakeRow = (usize, u32, bool, f64, [(i64, i64); 3]);

#[test]
fn eight_bit_auto_tone_is_the_declared_rule_on_take_eights_pictures() {
    for (index, f) in PICTURES_Q.iter().enumerate() {
        for clip_case in [false, true] {
            let pixels = scope(f, clip_case);
            for clip in [0.0f32, 0.001, 0.003, 0.005] {
                let got = ours8(&pixels, clip);
                let want = oracle(&wide(&pixels), 255, f64::from(clip), 1.0);
                for c in 0..3 {
                    let code = |v: f64| (v * 255.0).round() as i64;
                    assert_eq!(
                        (code(got[c].0), code(got[c].1)),
                        (code(want[c].0), code(want[c].1)),
                        "Q{} clip-case {clip_case}, clip {clip}, channel {c}",
                        index + 1
                    );
                }
            }
        }
    }
}

#[test]
fn photoshop_rows_of_take_eight_hold_the_shipped_residuals() {
    for &(index, depth, clip_case, clip, photoshop) in &TAKE8_ROWS {
        let pixels = scope(&PICTURES_Q[index], clip_case);
        let got = if depth == 8 {
            ours8(&pixels, clip as f32)
        } else {
            let wide16: Vec<[u16; 3]> = pixels
                .iter()
                .map(|p| p.map(|v| u16::from(v) * 257))
                .collect();
            ours16(&wide16, clip)
        };
        let (white_bound, black_bound) = bound8(depth);
        for (c, ((black, white), (ps_black, ps_white))) in got.iter().zip(photoshop).enumerate() {
            let label = format!(
                "Q{} depth {depth} clip-case {clip_case} clip {clip} channel {c}",
                index + 1
            );
            assert!(
                ((black * 255.0).round() - ps_black as f64).abs() <= black_bound,
                "{label}: black {black} vs {ps_black}"
            );
            assert!(
                ((white * 255.0).round() - ps_white as f64).abs() <= white_bound,
                "{label}: white {white} vs {ps_white}"
            );
        }
    }
}

/// Take 8's pictures (module doc), Q1-Q4.
#[rustfmt::skip]
static PICTURES_Q: [Fixture; 4] = [
    // Q1 (`r_fixture(101)`, seed 4581)
    Fixture {
        canvas: (154, 100),
        photo: (102, 66),
        shared: false,
        tails: [
            &[
                (12, 1), (14, 1), (16, 2), (18, 1), (20, 2), (21, 1), (22, 1), (24, 2),
                (26, 1), (28, 1), (30, 2), (31, 1), (33, 1), (34, 2), (36, 1), (38, 1),
                (39, 1), (41, 1), (42, 1), (44, 1), (45, 1), (46, 2), (47, 1), (48, 1),
                (49, 1), (51, 2), (52, 1), (54, 2), (55, 2), (56, 1), (57, 2), (58, 1),
                (85, 2),
            ],
            &[
                (10, 2), (11, 1), (13, 1), (15, 1), (17, 2), (18, 1), (19, 2), (21, 1),
                (23, 1), (24, 2), (26, 1), (27, 2), (28, 1), (30, 2), (32, 2), (34, 1),
                (36, 13),
            ],
            &[
                (8, 2), (9, 1), (10, 2), (11, 2), (12, 1), (13, 2), (15, 1), (16, 2),
                (18, 2), (19, 1), (21, 1), (22, 2), (23, 2), (24, 1), (25, 2), (26, 1),
                (28, 2), (29, 2), (30, 1), (32, 2), (34, 1), (36, 1), (38, 1), (40, 1),
                (41, 1), (43, 2), (44, 1), (45, 1), (46, 1), (47, 1), (48, 1), (50, 2),
                (51, 1), (53, 1), (55, 1), (86, 5),
            ],
        ],
        heads: [
            &[(251, 5), (238, 1), (227, 8), (218, 3), (211, 1), (202, 5)],
            &[(224, 13), (221, 1), (196, 5)],
            &[(254, 1), (244, 2), (230, 12), (198, 13)],
        ],
        bodies: [(86, 186, 4, 3), (37, 181, 4, 4), (87, 174, 3, 4)],
    },
    // Q2 (`r_fixture(102)`, seed 4582)
    Fixture {
        canvas: (155, 138),
        photo: (126, 111),
        shared: true,
        tails: [
            &[
                (12, 1), (13, 2), (14, 1), (15, 2), (17, 2), (19, 2), (20, 2), (21, 2),
                (23, 1), (24, 2), (25, 1), (27, 2), (29, 2), (30, 1), (31, 2), (32, 2),
                (34, 2), (35, 1), (37, 1), (38, 2), (40, 2), (41, 1), (43, 1), (45, 1),
                (47, 2), (48, 2), (50, 1), (52, 2), (53, 2), (54, 2), (55, 1), (57, 2),
                (59, 1), (60, 1), (61, 2), (62, 2), (64, 1), (89, 5),
            ],
            &[
                (7, 1), (9, 1), (10, 2), (12, 2), (13, 1), (15, 1), (16, 2), (17, 1),
                (18, 1), (20, 2), (22, 1), (24, 1), (25, 2), (26, 1), (28, 1), (30, 1),
                (32, 1), (33, 2), (35, 2), (37, 2), (38, 2), (40, 1), (41, 2), (42, 1),
                (43, 2), (44, 1), (45, 2), (47, 1), (48, 2), (50, 1), (51, 2), (52, 2),
                (54, 1), (55, 1), (57, 1), (58, 2), (59, 1), (60, 1), (61, 1), (75, 2),
            ],
            &[
                (3, 2), (4, 1), (6, 2), (8, 2), (10, 1), (12, 2), (14, 2), (16, 1),
                (17, 1), (19, 1), (21, 1), (22, 2), (23, 2), (24, 1), (25, 1), (39, 1),
            ],
        ],
        heads: [
            &[(247, 3), (238, 2), (235, 1), (227, 1), (226, 8), (217, 1), (206, 2), (205, 1)],
            &[(247, 1), (246, 1), (244, 8), (216, 5), (199, 2), (197, 5)],
            &[(248, 13), (228, 2), (223, 1), (220, 1), (216, 13), (197, 2)],
        ],
        bodies: [(90, 178, 6, 4), (76, 185, 2, 2), (40, 176, 2, 6)],
    },
    // Q3 (`r_fixture(103)`, seed 4583)
    Fixture {
        canvas: (144, 124),
        photo: (106, 91),
        shared: false,
        tails: [
            &[
                (5, 2), (7, 1), (8, 1), (10, 2), (12, 2), (13, 2), (15, 1), (17, 1),
                (19, 1), (21, 2), (22, 2), (23, 2), (25, 2), (26, 1), (28, 2), (29, 2),
                (31, 1), (32, 2), (34, 2), (35, 2), (37, 1), (38, 2), (39, 1), (41, 2),
                (43, 1), (44, 2), (46, 1), (47, 1), (49, 1), (62, 2),
            ],
            &[
                (1, 2), (3, 1), (4, 2), (6, 2), (7, 1), (8, 2), (9, 1), (11, 2),
                (12, 1), (14, 2), (16, 1), (18, 2), (19, 2), (21, 1), (23, 2), (46, 8),
            ],
            &[
                (2, 2), (3, 1), (5, 2), (7, 1), (8, 1), (10, 1), (12, 2), (13, 2),
                (14, 1), (16, 1), (18, 1), (20, 1), (22, 1), (23, 1), (24, 2), (25, 2),
                (26, 1), (27, 1), (29, 1), (30, 1), (32, 2), (34, 1), (35, 1), (37, 2),
                (39, 1), (40, 1), (42, 2), (43, 3),
            ],
        ],
        heads: [
            &[(253, 5), (250, 5), (238, 1), (231, 13), (225, 1), (206, 1), (201, 8)],
            &[(244, 5), (238, 5), (231, 8), (221, 1), (212, 5)],
            &[(253, 2), (245, 1), (241, 13), (237, 2), (227, 2), (224, 13), (215, 13)],
        ],
        bodies: [(63, 177, 7, 5), (47, 184, 4, 2), (44, 185, 6, 2)],
    },
    // Q4 (`r_fixture(104)`, seed 4584)
    Fixture {
        canvas: (143, 62),
        photo: (122, 53),
        shared: true,
        tails: [
            &[
                (10, 1), (11, 1), (13, 2), (14, 1), (16, 1), (18, 1), (19, 2), (20, 1),
                (21, 2), (22, 1), (23, 1), (24, 2), (25, 1), (27, 2), (29, 2), (30, 2),
                (32, 2), (34, 1), (56, 2),
            ],
            &[
                (11, 1), (12, 2), (14, 2), (15, 2), (16, 2), (17, 2), (19, 1), (21, 2),
                (22, 2), (23, 2), (25, 1), (26, 1), (28, 2), (30, 2), (31, 1), (32, 2),
                (33, 1), (35, 2), (36, 1), (37, 1), (38, 1), (39, 1), (40, 2), (41, 1),
                (43, 1), (58, 8),
            ],
            &[
                (10, 1), (11, 1), (13, 2), (14, 1), (16, 2), (18, 2), (20, 1), (22, 1),
                (23, 2), (25, 1), (26, 2), (27, 1), (28, 1), (29, 1), (31, 2), (35, 13),
            ],
        ],
        heads: [
            &[(253, 5), (248, 5), (214, 3), (205, 1)],
            &[(251, 1), (237, 1), (225, 1), (222, 5), (219, 5), (204, 13), (201, 2), (197, 8)],
            &[(243, 2), (229, 3), (225, 2), (218, 2), (213, 13)],
        ],
        bodies: [(57, 191, 7, 3), (59, 186, 4, 6), (36, 183, 6, 3)],
    },
];

/// Photoshop's take-8 rows, in the runner's `q_plan()` order.
#[rustfmt::skip]
const TAKE8_ROWS: [TakeRow; 20] = [
    (0, 8, true, 0.001, [(34, 255), (28, 255), (22, 255)]),
    (0, 8, true, 0.003, [(86, 255), (38, 255), (54, 255)]),
    (0, 8, false, 0.001, [(23, 226), (19, 220), (13, 229)]),
    (0, 8, false, 0.003, [(40, 186), (33, 181), (25, 197)]),
    (1, 8, true, 0.001, [(31, 255), (33, 255), (40, 255)]),
    (1, 8, true, 0.003, [(90, 255), (76, 255), (40, 255)]),
    (1, 8, false, 0.001, [(24, 216), (25, 199), (22, 222)]),
    (1, 8, false, 0.003, [(52, 178), (51, 184), (40, 176)]),
    (2, 8, true, 0.001, [(25, 255), (19, 255), (25, 255)]),
    (2, 8, true, 0.003, [(63, 255), (47, 255), (44, 255)]),
    (2, 8, false, 0.001, [(18, 231), (11, 230), (14, 237)]),
    (2, 8, false, 0.003, [(36, 200), (47, 183), (40, 215)]),
    (3, 8, true, 0.001, [(21, 255), (18, 255), (23, 255)]),
    (3, 8, true, 0.003, [(57, 255), (40, 255), (36, 255)]),
    (3, 8, false, 0.001, [(19, 247), (16, 219), (18, 217)]),
    (3, 8, false, 0.003, [(30, 191), (28, 201), (32, 183)]),
    (1, 16, false, 0.001, [(24, 216), (25, 199), (22, 222)]),
    (1, 16, false, 0.003, [(52, 178), (51, 184), (40, 176)]),
    (3, 16, false, 0.001, [(19, 247), (16, 219), (18, 217)]),
    (3, 16, false, 0.003, [(30, 191), (28, 201), (32, 183)]),
];
