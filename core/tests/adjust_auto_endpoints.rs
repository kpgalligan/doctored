//! Levels Auto Tone's endpoints, measured against Photoshop 26.10.0 as
//! far as the declared families reach (`ops_auto::tone_endpoints`, every
//! caller). The rule, from `scripts/verify_levels_auto_photoshop.py`'s
//! docstring (takes 4 and 5, RULING-1), with `unit` one whole counted
//! pixel's mass:
//!
//! * black: today's count rule, the lowest code whose running mass from the
//!   bottom exceeds `cut = clip·mass` (at clip 0 the darkest occupied code);
//! * white at clip 0: the brightest occupied code;
//! * white at clip > 0 (take 4's member Ns β 3 θ 0.70 MIX3 J1 B8 E0): each
//!   channel walks its 256 bins (a 16-bit code binned `round(c/257)`) from
//!   the top with `T = cut + 3·unit`, clipping a bin while `T − run ≥ 0.7·b`
//!   (never the lowest occupied); a pixel above ANY channel's stop bin leaves
//!   every channel's kept set; each white is the kept pixels' top bin when it
//!   holds three pixels' mass, else one below the lowest removed bin above it.
//!
//! A channel whose black is not below its white is left alone. No member of
//! the declared family held every endpoint within ±1; the shipped rule is
//! measurably closer than the one it replaced, and take 5's residuals are
//! the bounds below.
//!
//! The oracle is binary64 from that rule, never read off the core, checked
//! through `dr_image_auto_levels` (8 bits) and
//! `dr_precision_doc_selected_auto_levels` (16 bits, `unit` 65535) on the
//! takes' own designed pictures — take 3's G1 and take 5's V1-V6, rebuilt here
//! from their generators' draws — and against Photoshop's rows on them.
//! Round 2's takes 6 and 7 (a white coupling and a black gap placement,
//! neither measurably closer, so this rule stays) are
//! `adjust_auto_endpoints_rows.rs`.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_adjust::dr_image_auto_levels;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_auto_levels;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

/// Take 5's residuals (module doc), in codes: the shipped white's maximum
/// and today's black's.
const WHITE_BOUND: f64 = 14.0;
const BLACK_BOUND: f64 = 32.0;
/// The white walk's constants (module doc).
const THETA: f64 = 0.7;
const BETA: f64 = 3.0;
const STOP_PIXELS: f64 = 3.0;

// ------------------------------------------------------------ pictures --

/// One designed photo (`verify_levels_auto_photoshop.generic_photo`): per
/// channel its ascending dark `tails` from the first pixel, its descending
/// `heads` on the last pixels (all channels on the same pixels when
/// `shared`, else channel c's ending c·L before the end, L the longest head)
/// and `lo + (a x + b y) % (hi − lo + 1)` elsewhere (`bodies` = (lo, hi, a,
/// b)). `canvas` is the document the `clip` case counts.
pub(crate) struct Fixture {
    pub(crate) canvas: (u32, u32),
    pub(crate) photo: (u32, u32),
    pub(crate) shared: bool,
    pub(crate) tails: [&'static [(u8, u32)]; 3],
    pub(crate) heads: [&'static [(u8, u32)]; 3],
    pub(crate) bodies: [(u32, u32, u32, u32); 3],
}

fn expand(runs: &[(u8, u32)]) -> Vec<u8> {
    runs.iter()
        .flat_map(|&(code, n)| std::iter::repeat_n(code, n as usize))
        .collect()
}

pub(crate) fn photo(f: &Fixture) -> Vec<[u8; 3]> {
    let (w, h) = (f.photo.0 as usize, f.photo.1 as usize);
    let n = w * h;
    let tails = f.tails.map(expand);
    let heads = f.heads.map(expand);
    let longest = heads.iter().map(Vec::len).max().unwrap();
    (0..n)
        .map(|i| {
            let (x, y) = ((i % w) as u32, (i / w) as u32);
            std::array::from_fn(|c| {
                let (lo, hi, a, b) = f.bodies[c];
                let mut value = (lo + (a * x + b * y) % (hi - lo + 1)) as u8;
                if i < tails[c].len() {
                    value = tails[c][i];
                }
                let end = n - if f.shared { 0 } else { c * longest };
                if i + longest >= end && i < end {
                    let j = i + longest - end;
                    if j < heads[c].len() {
                        value = heads[c][j];
                    }
                }
                value
            })
        })
        .collect()
}

/// The counted pixels: the photo, or for `clip` the whole canvas (the photo
/// over white).
pub(crate) fn scope(f: &Fixture, clip_case: bool) -> Vec<[u8; 3]> {
    let mut pixels = photo(f);
    if clip_case {
        let extra = (f.canvas.0 * f.canvas.1 - f.photo.0 * f.photo.1) as usize;
        pixels.extend(std::iter::repeat_n([255u8; 3], extra));
    }
    pixels
}

/// A 16-bit twin: 257·c, plus a small per-pixel offset off the lattice so the
/// 8-bit binning is exercised.
fn sixteen(pixels: &[[u8; 3]]) -> Vec<[u16; 3]> {
    pixels
        .iter()
        .enumerate()
        .map(|(i, p)| {
            std::array::from_fn(|c| {
                let base = u32::from(p[c]) * 257;
                let wiggle = ((i * 7 + c * 3) % 5) as u32 * 20;
                (if base >= 65535 - 100 {
                    base
                } else {
                    base + wiggle
                }) as u16
            })
        })
        .collect()
}

// -------------------------------------------------------------- oracle --

/// The declared rule (module doc) over `pixels` at codes of `top` (255 or
/// 65535), each pixel of mass `unit`: per channel `(black, white)` as
/// fractions, `(0, 1)` for a channel left alone.
pub(crate) fn oracle(pixels: &[[u32; 3]], top: u32, clip: f64, unit: f64) -> [(f64, f64); 3] {
    let size = top as usize + 1;
    let total = pixels.len() as f64 * unit;
    let cut = clip * total;
    let bin = |v: u32| {
        if top == 255 {
            v as usize
        } else {
            ((2 * v + 257) / 514) as usize
        }
    };
    let mut native = vec![vec![0f64; size]; 3];
    let mut eight = [[0f64; 256]; 3];
    for p in pixels {
        for c in 0..3 {
            native[c][p[c] as usize] += unit;
            eight[c][bin(p[c])] += unit;
        }
    }
    let black: [Option<usize>; 3] = std::array::from_fn(|c| {
        let mut run = 0.0;
        (0..size).find(|&v| {
            run += native[c][v];
            run > cut
        })
    });
    let white: [Option<f64>; 3] = if clip == 0.0 {
        std::array::from_fn(|c| {
            (0..size)
                .rev()
                .find(|&v| native[c][v] > 0.0)
                .map(|v| v as f64 / f64::from(top))
        })
    } else {
        let t = cut + BETA * unit;
        let stops: [usize; 3] = std::array::from_fn(|c| {
            let occupied: Vec<usize> = (0..256).rev().filter(|&v| eight[c][v] > 0.0).collect();
            let mut run = 0.0;
            for (k, &v) in occupied.iter().enumerate() {
                let b = eight[c][v];
                if k + 1 < occupied.len() && t - run >= THETA * b {
                    run += b;
                    continue;
                }
                return v;
            }
            unreachable!()
        });
        let mut kept = [[0f64; 256]; 3];
        let mut removed = [[0f64; 256]; 3];
        for p in pixels {
            let b = p.map(bin);
            let clipped = (0..3).any(|c| b[c] > stops[c]);
            for c in 0..3 {
                if clipped {
                    removed[c][b[c]] += unit;
                } else {
                    kept[c][b[c]] += unit;
                }
            }
        }
        std::array::from_fn(|c| {
            let top_kept = (0..256).rev().find(|&v| kept[c][v] > 0.0)?;
            let last = (top_kept + 1..256).find(|&v| removed[c][v] > 0.0);
            let code = match last {
                Some(l) if kept[c][top_kept] < STOP_PIXELS * unit => l - 1,
                _ => top_kept,
            };
            Some(code as f64 / 255.0)
        })
    };
    std::array::from_fn(|c| match (black[c], white[c]) {
        (Some(lo), Some(hi)) if (lo as f64 / f64::from(top)) < hi => {
            (lo as f64 / f64::from(top), hi)
        }
        _ => (0.0, 1.0),
    })
}

// ---------------------------------------------------------- the core --

/// `dr_image_auto_levels` Auto Tone at `clip`: per channel `(black, white)`.
pub(crate) fn ours8(pixels: &[[u8; 3]], clip: f32) -> [(f64, f64); 3] {
    let rgba: Vec<u8> = pixels
        .iter()
        .flat_map(|p| [p[0], p[1], p[2], 255])
        .collect();
    let image = unsafe { dr_image_from_rgba(rgba.as_ptr(), pixels.len() as u32, 1) };
    assert!(!image.is_null());
    let mut out = [0f32; 9];
    let changed = unsafe { dr_image_auto_levels(image, ptr::null(), 0, clip, out.as_mut_ptr()) };
    unsafe { dr_image_free(image) };
    std::array::from_fn(|c| {
        if changed {
            (f64::from(out[c]), f64::from(out[3 + c]))
        } else {
            (0.0, 1.0)
        }
    })
}

/// `dr_precision_doc_selected_auto_levels` Auto Tone at `clip`.
pub(crate) fn ours16(pixels: &[[u16; 3]], clip: f64) -> [(f64, f64); 3] {
    let rgba: Vec<[u16; 4]> = pixels.iter().map(|p| [p[0], p[1], p[2], 65535]).collect();
    let document = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(rgba.len() as u32, 1, rgba, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let request = CString::new(
        json!({"layer":0,"op":"auto_tone","params":{"clip":clip},"analysis_scope":"whole_target"})
            .to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let text = unsafe {
        dr_precision_doc_selected_auto_levels(
            &document,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(error.is_null() && !text.is_null());
    let value: Value = serde_json::from_slice(unsafe { CStr::from_ptr(text) }.to_bytes()).unwrap();
    unsafe { dr_string_free(text) };
    std::array::from_fn(|c| {
        (
            value["black"][c].as_f64().unwrap(),
            value["white"][c].as_f64().unwrap(),
        )
    })
}

pub(crate) fn wide(pixels: &[[u8; 3]]) -> Vec<[u32; 3]> {
    pixels.iter().map(|p| p.map(u32::from)).collect()
}

/// Every designed picture: take 3's G1 and take 5's V1-V6.
fn pictures() -> Vec<(&'static str, &'static Fixture)> {
    let mut out = vec![("G1", &G1)];
    for (name, f) in ["V1", "V2", "V3", "V4", "V5", "V6"]
        .iter()
        .zip(FIXTURES.iter())
    {
        out.push((name, f));
    }
    out
}

// --------------------------------------------------------------- 8 bits --

#[test]
fn eight_bit_auto_tone_is_the_declared_rule() {
    for (name, f) in pictures() {
        for clip_case in [false, true] {
            let pixels = scope(f, clip_case);
            for clip in [0.0f32, 0.001, 0.003, 0.005, 0.01, 0.1] {
                let got = ours8(&pixels, clip);
                let want = oracle(&wide(&pixels), 255, f64::from(clip), 1.0);
                for c in 0..3 {
                    let code = |v: f64| (v * 255.0).round() as i64;
                    assert_eq!(
                        (code(got[c].0), code(got[c].1)),
                        (code(want[c].0), code(want[c].1)),
                        "{name} clip-case {clip_case}, clip {clip}, channel {c}"
                    );
                }
            }
        }
    }
}

// -------------------------------------------------------------- 16 bits --

#[test]
fn sixteen_bit_auto_tone_is_the_declared_rule() {
    for (name, f) in pictures() {
        let pixels = sixteen(&photo(f));
        let codes: Vec<[u32; 3]> = pixels.iter().map(|p| p.map(u32::from)).collect();
        for clip in [0.0, 0.001, 0.003, 0.005, 0.05] {
            let got = ours16(&pixels, clip);
            let want = oracle(&codes, 65535, clip, 65535.0);
            for c in 0..3 {
                assert_eq!(
                    (got[c].0 * 65535.0).round(),
                    (want[c].0 * 65535.0).round(),
                    "{name} clip {clip}, channel {c}: black"
                );
                assert!(
                    (got[c].1 - want[c].1).abs() < 1e-6,
                    "{name} clip {clip}, channel {c}: white {} vs {}",
                    got[c].1,
                    want[c].1
                );
            }
        }
    }
}

// ----------------------------------------------------- Photoshop's rows --

/// One take row: the fixture (0 = G1, else V<n>), its depth and case, the
/// clip and Photoshop's `levl` (black, white).
struct Row {
    fixture: usize,
    depth: u32,
    clip_case: bool,
    clip: f64,
    photoshop: [(i64, i64); 3],
}

#[test]
fn photoshop_rows_hold_take_fives_residuals() {
    for row in G1_ROWS.iter().chain(ROWS) {
        let f = if row.fixture == 0 {
            &G1
        } else {
            &FIXTURES[row.fixture - 1]
        };
        let pixels = scope(f, row.clip_case);
        let got = if row.depth == 8 {
            ours8(&pixels, row.clip as f32)
        } else {
            let wide16: Vec<[u16; 3]> = pixels
                .iter()
                .map(|p| p.map(|v| u16::from(v) * 257))
                .collect();
            ours16(&wide16, row.clip)
        };
        for (c, ((black, white), (ps_black, ps_white))) in got.iter().zip(row.photoshop).enumerate()
        {
            let label = format!(
                "fixture {} depth {} clip-case {} clip {} channel {c}",
                row.fixture, row.depth, row.clip_case, row.clip
            );
            assert!(
                ((black * 255.0).round() - ps_black as f64).abs() <= BLACK_BOUND,
                "{label}: black {black}"
            );
            assert!(
                ((white * 255.0).round() - ps_white as f64).abs() <= WHITE_BOUND,
                "{label}: white {white}"
            );
        }
    }
}

/// Take 3's G1 (`probe3('G1')`): 30 dark singles per channel, shared heads.
pub(crate) const G1: Fixture = Fixture {
    canvas: (110, 64),
    photo: (92, 50),
    shared: true,
    tails: [&G1_TAIL_R, &G1_TAIL_G, &G1_TAIL_B],
    heads: [
        &[(250, 8), (240, 8)],
        &[(245, 7), (230, 13)],
        &[
            (252, 1),
            (249, 1),
            (246, 1),
            (243, 1),
            (240, 1),
            (237, 1),
            (234, 1),
            (231, 1),
            (228, 1),
            (225, 1),
            (222, 1),
            (219, 1),
            (216, 1),
            (213, 1),
            (210, 1),
            (207, 1),
            (204, 1),
            (201, 1),
        ],
    ],
    bodies: [(110, 190, 3, 7), (110, 190, 5, 3), (110, 190, 7, 5)],
};

const fn singles(start: u8, step: u8) -> [(u8, u32); 30] {
    let mut out = [(0u8, 1u32); 30];
    let mut i = 0;
    while i < 30 {
        out[i] = (start + step * i as u8, 1);
        i += 1;
    }
    out
}
const G1_TAIL_R: [(u8, u32); 30] = singles(3, 3);
const G1_TAIL_G: [(u8, u32); 30] = singles(20, 2);
const G1_TAIL_B: [(u8, u32); 30] = singles(5, 3);

/// Take 4's A1 (G1 again, `mask`).
const G1_ROWS: &[Row] = &[
    Row {
        fixture: 0,
        depth: 8,
        clip_case: false,
        clip: 0.001,
        photoshop: [(22, 240), (33, 230), (24, 227)],
    },
    Row {
        fixture: 0,
        depth: 8,
        clip_case: false,
        clip: 0.003,
        photoshop: [(49, 190), (51, 190), (51, 190)],
    },
    Row {
        fixture: 0,
        depth: 8,
        clip_case: false,
        clip: 0.005,
        photoshop: [(76, 190), (69, 190), (78, 190)],
    },
];

// Take 5's V1-V6 (`v_fixture`'s draws from `random.Random(4380 + i)`) and
// its rows (the Kab3 rows, black 0.3 % and white 0.1 %, have no single-clip
// Doctored form and are left out).
const FIXTURES: [Fixture; 6] = [
    Fixture {
        canvas: (115, 150),
        photo: (75, 97),
        shared: false,
        tails: [
            &[
                (5, 1),
                (6, 2),
                (7, 1),
                (9, 2),
                (10, 2),
                (11, 1),
                (12, 1),
                (13, 2),
                (15, 1),
                (17, 2),
                (18, 1),
                (20, 2),
                (22, 1),
                (23, 2),
                (24, 2),
                (26, 2),
                (27, 2),
                (29, 1),
                (31, 1),
                (32, 1),
                (34, 2),
                (36, 2),
                (37, 1),
                (38, 2),
                (39, 1),
                (40, 2),
                (42, 1),
                (44, 1),
                (46, 2),
                (47, 1),
                (49, 1),
                (51, 2),
                (53, 1),
                (55, 2),
                (56, 1),
                (58, 2),
                (60, 1),
                (61, 2),
                (62, 2),
                (63, 2),
            ],
            &[
                (4, 1),
                (5, 1),
                (6, 1),
                (8, 2),
                (10, 2),
                (11, 1),
                (12, 1),
                (13, 1),
                (14, 2),
                (16, 2),
                (18, 2),
                (20, 1),
                (21, 2),
                (22, 1),
                (24, 1),
                (25, 2),
                (26, 1),
                (27, 1),
                (28, 2),
                (30, 1),
                (31, 1),
                (33, 2),
                (35, 1),
                (37, 1),
                (38, 2),
                (39, 2),
                (40, 2),
            ],
            &[
                (10, 1),
                (12, 2),
                (13, 2),
                (15, 2),
                (17, 1),
                (18, 2),
                (19, 2),
                (21, 2),
                (22, 1),
                (24, 1),
                (25, 1),
                (26, 1),
                (28, 1),
                (30, 2),
                (31, 1),
                (32, 2),
                (33, 1),
                (34, 2),
                (35, 1),
                (37, 1),
                (38, 2),
                (40, 2),
                (42, 2),
                (43, 2),
                (44, 2),
                (46, 1),
                (48, 1),
                (49, 1),
                (50, 2),
                (52, 1),
                (54, 1),
            ],
        ],
        heads: [
            &[(241, 1), (236, 8), (222, 13), (208, 1)],
            &[(254, 3), (249, 3), (247, 1), (246, 3), (235, 1), (213, 8)],
            &[(254, 1), (215, 3), (204, 2)],
        ],
        bodies: [(75, 191, 3, 7), (62, 183, 2, 6), (64, 180, 6, 4)],
    },
    Fixture {
        canvas: (135, 127),
        photo: (108, 102),
        shared: true,
        tails: [
            &[
                (7, 1),
                (9, 2),
                (10, 1),
                (12, 2),
                (14, 2),
                (15, 2),
                (17, 1),
                (18, 1),
                (19, 2),
                (20, 2),
                (21, 2),
                (23, 2),
                (25, 1),
                (27, 1),
                (28, 2),
                (30, 2),
                (31, 2),
                (33, 1),
                (35, 2),
                (37, 1),
                (38, 2),
                (40, 1),
                (41, 1),
                (43, 2),
                (44, 1),
                (45, 1),
                (46, 2),
                (48, 2),
                (50, 2),
                (51, 1),
                (52, 1),
            ],
            &[
                (4, 1),
                (5, 2),
                (7, 1),
                (9, 2),
                (10, 2),
                (12, 2),
                (13, 1),
                (14, 2),
                (16, 1),
                (18, 2),
                (20, 1),
                (22, 1),
                (24, 2),
                (25, 1),
                (26, 1),
                (27, 2),
                (28, 1),
            ],
            &[
                (5, 2),
                (6, 2),
                (8, 1),
                (9, 2),
                (11, 2),
                (12, 2),
                (13, 1),
                (15, 1),
                (16, 2),
                (17, 1),
                (18, 1),
                (19, 1),
                (21, 1),
                (22, 1),
                (23, 2),
                (24, 1),
                (25, 2),
                (26, 1),
                (27, 2),
                (29, 1),
                (31, 2),
                (32, 2),
                (34, 2),
                (35, 1),
                (37, 2),
                (39, 1),
                (40, 2),
                (41, 1),
                (42, 1),
                (44, 2),
                (45, 2),
                (46, 2),
                (48, 2),
                (50, 2),
                (51, 2),
                (52, 2),
            ],
        ],
        heads: [
            &[(243, 1), (205, 1), (199, 1)],
            &[(227, 8), (218, 1), (214, 1)],
            &[(254, 8), (243, 13), (239, 1), (232, 1), (206, 3), (197, 3)],
        ],
        bodies: [(81, 183, 3, 7), (50, 187, 4, 5), (78, 189, 4, 6)],
    },
    Fixture {
        canvas: (102, 90),
        photo: (70, 61),
        shared: false,
        tails: [
            &[
                (10, 2),
                (12, 2),
                (13, 1),
                (15, 2),
                (16, 1),
                (18, 2),
                (20, 1),
                (21, 1),
                (23, 2),
                (24, 1),
                (25, 2),
                (26, 1),
                (27, 2),
                (29, 1),
                (30, 1),
                (32, 1),
                (33, 1),
                (34, 2),
                (35, 1),
                (37, 2),
                (38, 1),
                (39, 2),
                (41, 1),
                (43, 1),
                (44, 1),
                (46, 1),
                (47, 2),
                (49, 2),
                (51, 2),
                (52, 1),
                (54, 1),
                (55, 1),
                (57, 1),
            ],
            &[
                (7, 2),
                (8, 2),
                (10, 1),
                (12, 1),
                (13, 1),
                (14, 2),
                (15, 1),
                (16, 1),
                (18, 1),
                (20, 2),
                (21, 1),
                (22, 2),
                (23, 2),
                (24, 1),
                (26, 1),
                (27, 2),
                (28, 2),
                (29, 1),
                (30, 1),
                (31, 1),
                (32, 1),
                (34, 1),
                (35, 1),
                (37, 2),
                (38, 2),
                (40, 1),
                (42, 2),
                (43, 2),
                (44, 1),
                (46, 1),
                (48, 2),
                (50, 2),
                (51, 1),
                (53, 1),
                (55, 2),
                (56, 1),
            ],
            &[
                (8, 2),
                (10, 1),
                (12, 1),
                (14, 2),
                (16, 1),
                (17, 2),
                (18, 2),
                (19, 2),
                (20, 1),
                (22, 2),
                (24, 2),
                (26, 2),
                (27, 1),
                (28, 1),
                (30, 1),
                (32, 1),
                (34, 2),
                (36, 2),
                (37, 1),
                (38, 2),
                (39, 1),
                (41, 1),
                (42, 1),
                (43, 1),
                (44, 1),
                (46, 1),
                (48, 1),
                (49, 1),
                (50, 1),
                (52, 1),
                (54, 2),
                (56, 1),
                (58, 2),
            ],
        ],
        heads: [
            &[
                (248, 8),
                (247, 2),
                (238, 1),
                (234, 5),
                (222, 13),
                (221, 1),
                (214, 5),
                (209, 2),
            ],
            &[
                (247, 1),
                (246, 13),
                (231, 3),
                (226, 3),
                (213, 3),
                (203, 2),
                (201, 2),
                (196, 1),
            ],
            &[
                (241, 13),
                (238, 1),
                (233, 2),
                (227, 2),
                (223, 1),
                (217, 2),
                (210, 5),
            ],
        ],
        bodies: [(79, 172, 2, 2), (87, 178, 3, 7), (90, 173, 2, 4)],
    },
    Fixture {
        canvas: (74, 157),
        photo: (53, 111),
        shared: true,
        tails: [
            &[
                (12, 2),
                (13, 1),
                (15, 1),
                (16, 2),
                (18, 1),
                (19, 1),
                (21, 1),
                (22, 2),
                (24, 2),
                (26, 1),
                (27, 1),
                (28, 1),
                (29, 2),
                (30, 1),
                (31, 1),
                (32, 2),
                (33, 2),
                (35, 1),
                (37, 1),
                (38, 2),
                (39, 1),
                (41, 2),
                (43, 1),
                (45, 1),
                (47, 1),
                (49, 2),
                (50, 2),
                (51, 2),
                (53, 2),
                (54, 1),
                (55, 2),
                (56, 1),
                (57, 1),
                (58, 1),
            ],
            &[
                (8, 1),
                (10, 2),
                (11, 2),
                (12, 2),
                (13, 2),
                (15, 2),
                (16, 1),
                (17, 1),
                (19, 2),
                (21, 1),
                (23, 2),
                (24, 2),
                (26, 1),
                (28, 1),
                (29, 1),
                (31, 1),
                (32, 2),
                (33, 2),
                (34, 2),
            ],
            &[
                (5, 2),
                (7, 1),
                (9, 2),
                (11, 2),
                (13, 1),
                (14, 2),
                (15, 1),
                (16, 1),
                (18, 2),
                (19, 2),
                (20, 1),
                (21, 2),
                (22, 2),
                (24, 2),
                (26, 1),
                (27, 1),
                (28, 2),
                (29, 1),
            ],
        ],
        heads: [
            &[(240, 5), (230, 2), (222, 1)],
            &[(243, 8), (224, 5), (219, 1), (217, 1), (209, 2), (205, 1)],
            &[(253, 1), (242, 1), (238, 2), (233, 3), (217, 1), (209, 3)],
        ],
        bodies: [(76, 192, 2, 6), (49, 186, 4, 6), (52, 187, 6, 3)],
    },
    Fixture {
        canvas: (138, 109),
        photo: (112, 89),
        shared: false,
        tails: [
            &[
                (2, 1),
                (4, 1),
                (6, 1),
                (7, 1),
                (8, 1),
                (9, 2),
                (11, 2),
                (13, 2),
                (14, 1),
                (16, 1),
                (17, 1),
                (18, 2),
                (19, 1),
                (20, 1),
                (21, 1),
                (23, 1),
                (25, 1),
                (27, 1),
                (28, 1),
                (30, 1),
            ],
            &[
                (6, 2),
                (8, 1),
                (10, 1),
                (11, 2),
                (12, 1),
                (13, 2),
                (15, 1),
                (16, 1),
                (18, 2),
                (20, 1),
                (21, 2),
                (23, 2),
                (24, 1),
                (25, 2),
                (27, 1),
                (29, 1),
                (31, 1),
                (33, 2),
                (34, 2),
                (35, 2),
            ],
            &[
                (6, 1),
                (8, 1),
                (10, 1),
                (11, 2),
                (13, 2),
                (15, 1),
                (17, 1),
                (19, 2),
                (21, 1),
                (22, 2),
                (24, 2),
                (25, 2),
                (26, 1),
                (27, 1),
                (29, 1),
                (31, 2),
                (32, 2),
                (33, 2),
                (35, 1),
                (37, 1),
                (38, 1),
                (40, 2),
                (41, 1),
                (43, 1),
            ],
        ],
        heads: [
            &[
                (251, 3),
                (249, 1),
                (242, 8),
                (236, 13),
                (226, 2),
                (218, 1),
                (203, 5),
            ],
            &[(253, 8), (232, 2), (224, 1), (209, 1), (203, 2)],
            &[(245, 3), (236, 2), (234, 1), (230, 2), (205, 2), (203, 1)],
        ],
        bodies: [(45, 192, 2, 3), (49, 192, 7, 2), (71, 182, 3, 3)],
    },
    Fixture {
        canvas: (116, 133),
        photo: (86, 98),
        shared: true,
        tails: [
            &[
                (1, 2),
                (3, 2),
                (4, 2),
                (6, 1),
                (8, 2),
                (9, 1),
                (11, 2),
                (12, 2),
                (14, 1),
                (16, 1),
                (18, 2),
                (19, 1),
                (20, 1),
                (22, 1),
                (23, 2),
                (25, 2),
            ],
            &[
                (0, 2),
                (12, 2),
                (14, 1),
                (15, 2),
                (17, 1),
                (19, 1),
                (20, 2),
                (21, 2),
                (22, 1),
                (24, 2),
                (25, 2),
                (27, 2),
                (28, 1),
                (29, 2),
                (30, 1),
                (32, 1),
                (33, 2),
                (34, 2),
                (36, 1),
                (37, 1),
            ],
            &[
                (4, 1),
                (6, 1),
                (7, 1),
                (8, 2),
                (9, 1),
                (10, 1),
                (12, 2),
                (14, 1),
                (16, 2),
                (18, 2),
                (19, 1),
                (20, 1),
                (22, 2),
                (23, 1),
                (24, 1),
                (26, 1),
                (27, 2),
                (28, 1),
                (30, 2),
                (31, 1),
                (33, 2),
                (35, 2),
                (37, 1),
                (38, 2),
                (40, 1),
                (41, 1),
                (43, 1),
                (45, 2),
                (46, 1),
                (48, 2),
                (50, 2),
                (51, 2),
            ],
        ],
        heads: [
            &[(255, 2), (244, 3), (234, 3), (230, 13), (203, 8)],
            &[(227, 1), (224, 2), (201, 2)],
            &[(249, 2), (240, 13), (226, 1), (205, 1)],
        ],
        bodies: [(50, 184, 2, 7), (67, 170, 3, 5), (66, 170, 4, 4)],
    },
];
const ROWS: &[Row] = &[
    Row {
        fixture: 1,
        depth: 8,
        clip_case: true,
        clip: 0.001,
        photoshop: [(23, 255), (22, 255), (29, 255)],
    },
    Row {
        fixture: 1,
        depth: 8,
        clip_case: true,
        clip: 0.003,
        photoshop: [(58, 255), (62, 255), (64, 255)],
    },
    Row {
        fixture: 1,
        depth: 8,
        clip_case: true,
        clip: 0.005,
        photoshop: [(76, 255), (63, 255), (65, 255)],
    },
    Row {
        fixture: 1,
        depth: 8,
        clip_case: false,
        clip: 0.001,
        photoshop: [(12, 222), (13, 245), (18, 180)],
    },
    Row {
        fixture: 1,
        depth: 8,
        clip_case: false,
        clip: 0.003,
        photoshop: [(26, 191), (27, 182), (33, 180)],
    },
    Row {
        fixture: 1,
        depth: 8,
        clip_case: false,
        clip: 0.005,
        photoshop: [(40, 190), (40, 182), (44, 179)],
    },
    Row {
        fixture: 2,
        depth: 8,
        clip_case: true,
        clip: 0.001,
        photoshop: [(23, 255), (24, 255), (22, 255)],
    },
    Row {
        fixture: 2,
        depth: 8,
        clip_case: true,
        clip: 0.003,
        photoshop: [(81, 255), (50, 255), (50, 255)],
    },
    Row {
        fixture: 2,
        depth: 8,
        clip_case: true,
        clip: 0.005,
        photoshop: [(82, 255), (51, 255), (78, 255)],
    },
    Row {
        fixture: 2,
        depth: 8,
        clip_case: false,
        clip: 0.001,
        photoshop: [(19, 183), (15, 187), (16, 243)],
    },
    Row {
        fixture: 2,
        depth: 8,
        clip_case: false,
        clip: 0.003,
        photoshop: [(41, 183), (50, 187), (35, 188)],
    },
    Row {
        fixture: 2,
        depth: 8,
        clip_case: false,
        clip: 0.005,
        photoshop: [(81, 182), (50, 186), (52, 188)],
    },
    Row {
        fixture: 3,
        depth: 8,
        clip_case: true,
        clip: 0.001,
        photoshop: [(21, 255), (17, 255), (19, 255)],
    },
    Row {
        fixture: 3,
        depth: 8,
        clip_case: true,
        clip: 0.003,
        photoshop: [(39, 255), (35, 255), (38, 255)],
    },
    Row {
        fixture: 3,
        depth: 8,
        clip_case: true,
        clip: 0.005,
        photoshop: [(79, 255), (54, 255), (90, 255)],
    },
    Row {
        fixture: 3,
        depth: 8,
        clip_case: false,
        clip: 0.001,
        photoshop: [(15, 247), (13, 245), (15, 240)],
    },
    Row {
        fixture: 3,
        depth: 8,
        clip_case: false,
        clip: 0.003,
        photoshop: [(25, 222), (22, 230), (22, 227)],
    },
    Row {
        fixture: 3,
        depth: 8,
        clip_case: false,
        clip: 0.005,
        photoshop: [(33, 221), (28, 203), (31, 209)],
    },
    Row {
        fixture: 4,
        depth: 8,
        clip_case: true,
        clip: 0.001,
        photoshop: [(27, 255), (19, 255), (19, 255)],
    },
    Row {
        fixture: 4,
        depth: 8,
        clip_case: true,
        clip: 0.003,
        photoshop: [(50, 255), (49, 255), (52, 255)],
    },
    Row {
        fixture: 4,
        depth: 8,
        clip_case: true,
        clip: 0.005,
        photoshop: [(76, 255), (49, 255), (53, 255)],
    },
    Row {
        fixture: 4,
        depth: 8,
        clip_case: false,
        clip: 0.001,
        photoshop: [(20, 192), (13, 224), (14, 209)],
    },
    Row {
        fixture: 4,
        depth: 8,
        clip_case: false,
        clip: 0.003,
        photoshop: [(32, 192), (25, 185), (22, 187)],
    },
    Row {
        fixture: 4,
        depth: 8,
        clip_case: false,
        clip: 0.005,
        photoshop: [(42, 192), (49, 185), (52, 187)],
    },
    Row {
        fixture: 5,
        depth: 8,
        clip_case: true,
        clip: 0.001,
        photoshop: [(20, 255), (23, 255), (25, 255)],
    },
    Row {
        fixture: 5,
        depth: 8,
        clip_case: true,
        clip: 0.003,
        photoshop: [(45, 255), (49, 255), (71, 255)],
    },
    Row {
        fixture: 5,
        depth: 8,
        clip_case: true,
        clip: 0.005,
        photoshop: [(46, 255), (50, 255), (72, 255)],
    },
    Row {
        fixture: 5,
        depth: 8,
        clip_case: false,
        clip: 0.001,
        photoshop: [(15, 236), (18, 203), (22, 182)],
    },
    Row {
        fixture: 5,
        depth: 8,
        clip_case: false,
        clip: 0.003,
        photoshop: [(45, 192), (49, 192), (40, 182)],
    },
    Row {
        fixture: 5,
        depth: 8,
        clip_case: false,
        clip: 0.005,
        photoshop: [(45, 191), (49, 191), (71, 181)],
    },
    Row {
        fixture: 6,
        depth: 8,
        clip_case: true,
        clip: 0.001,
        photoshop: [(18, 255), (25, 255), (22, 255)],
    },
    Row {
        fixture: 6,
        depth: 8,
        clip_case: true,
        clip: 0.003,
        photoshop: [(50, 255), (67, 255), (66, 255)],
    },
    Row {
        fixture: 6,
        depth: 8,
        clip_case: true,
        clip: 0.005,
        photoshop: [(51, 255), (68, 255), (67, 255)],
    },
    Row {
        fixture: 6,
        depth: 8,
        clip_case: false,
        clip: 0.001,
        photoshop: [(10, 229), (20, 170), (15, 225)],
    },
    Row {
        fixture: 6,
        depth: 8,
        clip_case: false,
        clip: 0.003,
        photoshop: [(50, 184), (34, 170), (32, 170)],
    },
    Row {
        fixture: 6,
        depth: 8,
        clip_case: false,
        clip: 0.005,
        photoshop: [(50, 184), (67, 170), (50, 170)],
    },
    Row {
        fixture: 2,
        depth: 16,
        clip_case: false,
        clip: 0.001,
        photoshop: [(19, 183), (15, 187), (16, 243)],
    },
    Row {
        fixture: 2,
        depth: 16,
        clip_case: false,
        clip: 0.003,
        photoshop: [(41, 183), (50, 187), (35, 188)],
    },
    Row {
        fixture: 2,
        depth: 16,
        clip_case: false,
        clip: 0.005,
        photoshop: [(81, 182), (50, 186), (52, 188)],
    },
    Row {
        fixture: 4,
        depth: 16,
        clip_case: false,
        clip: 0.001,
        photoshop: [(20, 192), (13, 224), (14, 209)],
    },
    Row {
        fixture: 4,
        depth: 16,
        clip_case: false,
        clip: 0.003,
        photoshop: [(32, 192), (25, 185), (22, 187)],
    },
    Row {
        fixture: 4,
        depth: 16,
        clip_case: false,
        clip: 0.005,
        photoshop: [(42, 192), (49, 185), (52, 187)],
    },
];
