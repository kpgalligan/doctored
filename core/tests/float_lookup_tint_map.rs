//! The float_adjust take3 (`scripts/photoshop-probe/
//! float_adjust_take3.py`, receipt `doctored-fixtures/photoshop/
//! float-adjust-473/take3`, `judged.json` authoritative). Take3 built the
//! map (pencil) form of Curves in a 32-bit float document and pinned
//! Color Lookup's device-link form, now built (below); no Black &
//! White Tint form matched there; take6 pinned it, and
//! `black_and_white_tint.rs` tests it.
//!
//! COLOR LOOKUP. The oracle is take3's pinned members, written here
//! in binary64 from the runners' docstrings (`float_adjust_take3.py`,
//! `float_adjust_take4.py`): the table on the stored values (CL-domain
//! `linear`), an input above 1 taken as 1 and one below 0 as 0 (CL-range
//! `clamp`, CL-neg `zero`), tetrahedral interpolation (CL-interp) over
//! entries held as a link's 16-bit codes (CL-out `table-clip`), then the
//! strength lerp. `color_lookup_oracle_reproduces_photoshop_take3` pins it
//! against Photoshop's own numbers and `color_lookup_renders_its_take3_form`
//! renders a baked device link and the same table as direct params against
//! it. Every other table runs the same lookup on the
//! stored values: a 1D table, a domain other than 0…1 (0…16 here), entries
//! outside 0…1 and an abstract look, each against an oracle written here
//! (linear per channel for 1D; for a 2-node 3D table whose entries are
//! affine in the input, that affine map, which tetrahedral interpolation
//! reproduces exactly), over patches at 1.0, 2.0, 8.0, 0.999, −0.25 and
//! 0.18.
//!
//! The oracle is written here in binary64 from the runner's docstring, never
//! read off the implementation: each map is read as straight lines between
//! its 256 entries on `X = 255·v`, its first and last segments continued
//! along their own slopes beyond 0…255 (CM-in `lin`, CM-hi and CM-lo
//! `slope`), the channel map before the master (CM-order `channel-first`),
//! nothing clamped. `oracles_reproduce_photoshop_take3` pins it against
//! Photoshop's own numbers (within `2e-3·max(1, |v|)`, the runner's T), and
//! `curves_map_renders_its_take3_form` renders the take's three sets over
//! take2's 42-patch grid against it within `1e-5·max(1, |want|)`.

use doctored_core::icc::IccProfile;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::float_adjustments::{add, base64, float_row, grid, grid_doc, render};
use crate::icc_lookup_tests::{desc, mft2, parse_bytes, profile};

/// A drawn map: entry `i` is the output code for input code `i`.
pub(crate) type Map256 = [f64; 256];

fn rnd(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Take3's M1: m0 = 30, m1 = 34, 30 + round(0.8 i) between, m254 = 233,
/// m255 = 239 (steep ends, so each continuation has its own slope).
pub(crate) fn m1() -> Map256 {
    std::array::from_fn(|i| match i {
        0 => 30.0,
        1 => 34.0,
        254 => 233.0,
        255 => 239.0,
        _ => 30.0 + rnd(0.8 * i as f64),
    })
}

/// Take3's M2: a zigzag, `clamp(i ± 24)` (odd up, even down).
pub(crate) fn m2() -> Map256 {
    std::array::from_fn(|i| {
        let step = if i % 2 == 1 { 24.0 } else { -24.0 };
        (i as f64 + step).clamp(0.0, 255.0)
    })
}

/// Take3's M3 red channel: `round(255·sqrt(i/255))`.
pub(crate) fn sqrt_map() -> Map256 {
    std::array::from_fn(|i| rnd(255.0 * (i as f64 / 255.0).sqrt()))
}

pub(crate) fn identity() -> Map256 {
    std::array::from_fn(|i| i as f64)
}

/// One map at a continuous input code, its end segments continued.
fn stage(map: &Map256, x: f64) -> f64 {
    let i = if x < 1.0 {
        0
    } else if x >= 254.0 {
        254
    } else {
        x.floor() as usize
    };
    map[i] + (x - i as f64) * (map[i + 1] - map[i])
}

/// The take3 form on stored linear values: per component the channel map,
/// then the master, in 0–255 units.
pub(crate) fn curves_map_oracle(
    master: &Map256,
    channels: [&Map256; 3],
    rgb: [f64; 3],
) -> [f64; 3] {
    std::array::from_fn(|c| stage(master, stage(channels[c], 255.0 * rgb[c])) / 255.0)
}

/// The `curves` params of a map set: `rgb_map` and, when given, `r_map`.
pub(crate) fn map_params(master: &Map256, red: Option<&Map256>) -> Value {
    let list = |m: &Map256| m.iter().map(|&v| v as u32).collect::<Vec<u32>>();
    match red {
        Some(red) => json!({"rgb_map": list(master), "r_map": list(red)}),
        None => json!({"rgb_map": list(master)}),
    }
}

/// (set, stored input, Photoshop's srgb32 output) copied from take3's `map`
/// job TIFFs (`M?_create_srgb32.tif`; wide32 reads the same numbers).
/// CM-in: M1/M2 interior; CM-order: M3 interior; CM-hi: M1/M3 above 1;
/// CM-lo: M1 (and M3's red) below 0. Photoshop's f32 samples are written as
/// the binary64 literals of their exact values.
const TAKE3: [(&str, [f32; 3], [f64; 3]); 14] = [
    // CM-in
    (
        "M1",
        [0.18, 0.18, 0.18],
        [
            0.26235297322273254,
            0.26235297322273254,
            0.26235297322273254,
        ],
    ),
    (
        "M1",
        [0.5, 0.2, 0.1],
        [0.5176470875740051, 0.27843141555786133, 0.198039248585701],
    ),
    (
        "M2",
        [0.18, 0.18, 0.18],
        [
            0.10470560193061829,
            0.10470560193061829,
            0.10470560193061829,
        ],
    ),
    (
        "M2",
        [0.3, 0.6, 0.2],
        [0.30000001192092896, 0.6941176652908325, 0.29411694407463074],
    ),
    // CM-order
    (
        "M3",
        [0.18, 0.18, 0.18],
        [0.4549020230770111, 0.26235297322273254, 0.26235297322273254],
    ),
    (
        "M3",
        [0.5, 0.2, 0.1],
        [0.6843137741088867, 0.27843141555786133, 0.198039248585701],
    ),
    (
        "M3",
        [0.3, 0.6, 0.2],
        [0.5549019575119019, 0.5960784554481506, 0.27843141555786133],
    ),
    // CM-hi
    (
        "M1",
        [1.5, 1.5, 1.5],
        [3.937208890914917, 3.937208890914917, 3.937208890914917],
    ),
    (
        "M1",
        [2.0, 0.5, 0.25],
        [6.93716287612915, 0.5176470875740051, 0.31666669249534607],
    ),
    (
        "M3",
        [4.0, 0.3, 2.0],
        [18.936979293823242, 0.3588235378265381, 6.93716287612915],
    ),
    // CM-lo (M3's red goes through the steep sqrt map first)
    (
        "M1",
        [-0.25, 0.5, 1.0],
        [-0.8823528289794922, 0.5176470875740051, 0.9372549653053284],
    ),
    (
        "M1",
        [0.5, -0.1, 0.2],
        [
            0.5176470875740051,
            -0.28235289454460144,
            0.27843141555786133,
        ],
    ),
    (
        "M1",
        [-0.05, -0.05, -0.05],
        [
            -0.08235291391611099,
            -0.08235291391611099,
            -0.08235291391611099,
        ],
    ),
    (
        "M3",
        [-0.25, 0.5, 1.0],
        [-15.88235092163086, 0.5176470875740051, 0.9372549653053284],
    ),
];

fn set_oracle(set: &str, rgb: [f64; 3]) -> [f64; 3] {
    let id = identity();
    match set {
        "M1" => curves_map_oracle(&m1(), [&id, &id, &id], rgb),
        "M2" => curves_map_oracle(&m2(), [&id, &id, &id], rgb),
        _ => curves_map_oracle(&m1(), [&sqrt_map(), &id, &id], rgb),
    }
}

fn set_params(set: &str) -> Value {
    match set {
        "M1" => map_params(&m1(), None),
        "M2" => map_params(&m2(), None),
        _ => map_params(&m1(), Some(&sqrt_map())),
    }
}

#[test]
fn oracles_reproduce_photoshop_take3() {
    const T: f64 = 2e-3;
    for (set, x, got) in TAKE3 {
        let want = set_oracle(set, x.map(f64::from));
        for c in 0..3 {
            let err = (got[c] - want[c]).abs();
            assert!(
                err <= T * want[c].abs().max(1.0),
                "{set} {x:?} channel {c}: Photoshop {}, oracle {}",
                got[c],
                want[c]
            );
        }
    }
}

#[test]
fn curves_map_renders_its_take3_form() {
    // The map form reads stored values, whatever the profile: take3 measured
    // the same numbers on sRGB and ROMM.
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        for set in ["M1", "M2", "M3"] {
            let out = render(&add(
                &grid_doc(profile.bytes()),
                "curves",
                &set_params(set),
                1.0,
            ));
            for (patch, got) in grid().iter().zip(&out) {
                let want = set_oracle(set, patch.map(f64::from));
                for c in 0..3 {
                    let err = (f64::from(got[c]) - want[c]).abs();
                    assert!(
                        err <= 1e-5 * want[c].abs().max(1.0),
                        "{set} {patch:?} channel {c}: got {}, want {}",
                        got[c],
                        want[c]
                    );
                }
                assert_eq!(got[3], 1.0);
            }
        }
    }
}

// ----------------------------------------------------------- Color Lookup --

/// A 3D look: `n` nodes per axis and the entry at a node's (r, g, b)
/// fractions of the 0…1 domain.
#[derive(Clone, Copy)]
struct Look {
    n: usize,
    entry: fn([f64; 3]) -> [f64; 3],
}

/// Take3's L1 `gain_half_2`.
const L1: Look = Look {
    n: 2,
    entry: |v| v.map(|c| 0.5 * c),
};
/// Take3's L2 `s_curve_17`.
const L2: Look = Look {
    n: 17,
    entry: |v| v.map(|c| 3.0 * c * c - 2.0 * c * c * c),
};
/// Take3's L4 `cross_5`: red is bilinear, so only the interpolation's own
/// shape decides it between nodes.
const L4: Look = Look {
    n: 5,
    entry: |[r, g, b]| [r * g, g, b],
};
/// Not a take3 set: a table whose top entries (0.8715, 0.9, 0.75) are not
/// short binary fractions, so an f32 strength lerp from a large stored input
/// would lose them (40000.0 + (0.8715 − 40000.0) rounds at 1/256).
const SKEW: Look = Look {
    n: 3,
    entry: |[r, g, b]| [0.2715 + 0.6 * r * g, 0.9 * g, 0.25 + 0.5 * b],
};

/// Take5's LKn checkerboard wave (`float_adjust_take5.py`): at node
/// (i_r, i_g, i_b), channel c is 0.5 + 0.25(−1)^i_c + 0.1(−1)^(i_c + i_(c+1))
/// plus 0.15(s_(c+1) − 0.5), s = i/(n − 1) — a grid whose own-grid
/// tetrahedral read differs from any 33-node bake of it.
fn wave(n: usize, s: [f64; 3]) -> [f64; 3] {
    let idx = s.map(|v| (v * (n - 1) as f64).round() as usize);
    let sign = |k: usize| if k.is_multiple_of(2) { 1.0 } else { -1.0 };
    std::array::from_fn(|c| {
        let next = (c + 1) % 3;
        0.5 + 0.25 * sign(idx[c]) + 0.1 * sign(idx[c] + idx[next]) + 0.15 * (s[next] - 0.5)
    })
}
const WAVE6: Look = Look {
    n: 6,
    entry: |s| wave(6, s),
};
const WAVE11: Look = Look {
    n: 11,
    entry: |s| wave(11, s),
};

/// An entry as a device link's `mft2` holds it: a 16-bit code over 65535,
/// inside 0…1 by construction (CL-out `table-clip`).
fn code16(v: f64) -> f64 {
    rnd(65535.0 * v.clamp(0.0, 1.0)) / 65535.0
}

/// Take3's pinned form on a stored triple (module doc), then the strength
/// lerp. Tetrahedral: the corner walks from the cell's (0, 0, 0) to its
/// (1, 1, 1) along the axes in DECREASING fraction order (ties in r, g, b
/// order), adding each axis' fraction times the step it makes.
fn color_lookup_oracle(look: Look, strength: f64, rgb: [f64; 3]) -> [f64; 3] {
    let n = look.n;
    let last = (n - 1) as f64;
    let cell: [(usize, f64); 3] = std::array::from_fn(|c| {
        let t = rgb[c].clamp(0.0, 1.0) * last;
        let i = (t.floor() as usize).min(n - 2);
        (i, t - i as f64)
    });
    let node = |at: [usize; 3]| (look.entry)(at.map(|i| i as f64 / last)).map(code16);
    let mut order = [0usize, 1, 2];
    order.sort_by(|a, b| cell[*b].1.total_cmp(&cell[*a].1));
    let mut at = [cell[0].0, cell[1].0, cell[2].0];
    let mut previous = node(at);
    let mut mapped = previous;
    for axis in order {
        at[axis] += 1;
        let here = node(at);
        for c in 0..3 {
            mapped[c] += cell[axis].1 * (here[c] - previous[c]);
        }
        previous = here;
    }
    std::array::from_fn(|c| rgb[c] + (mapped[c] - rgb[c]) * strength)
}

/// The look as `color_lookup` params at its own grid, red fastest, holding
/// the same 16-bit codes the link does.
fn direct_params(look: Look) -> Value {
    let n = look.n;
    let last = (n - 1) as f64;
    let mut bytes = Vec::with_capacity(n * n * n * 12);
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                for v in (look.entry)([r, g, b].map(|i| i as f64 / last)) {
                    bytes.extend((code16(v) as f32).to_le_bytes());
                }
            }
        }
    }
    json!({"kind": "3d", "size": n, "table": base64(&bytes)})
}

/// Take3's L4 as direct params, for the shared built and applied cases of
/// `float_adjustments.rs` and `float_adjust_apply.rs`.
pub(crate) fn cross_5_params() -> Value {
    direct_params(L4)
}

/// [`cross_5_params`]' oracle at full strength.
pub(crate) fn cross_5_oracle(rgb: [f64; 3]) -> [f64; 3] {
    color_lookup_oracle(L4, 1.0, rgb)
}

/// The look as an ICC v2 device link (`mft2`), the form take3 authored.
fn link_bytes(name: &str, look: Look) -> Vec<u8> {
    profile(
        b"link",
        b"RGB ",
        b"RGB ",
        2,
        &[(b"desc", desc(name)), (b"A2B0", mft2(look.n, look.entry))],
    )
}

/// (set, stored input, Photoshop's srgb32 output) copied from take3's
/// `lookup` job TIFFs (`L?_create_srgb32.tif`, read with the runner's
/// `Reading`). CL-interp: L4 interior; CL-range: an input above 1; CL-neg:
/// an input below 0. Photoshop's f32 samples are written as the binary64
/// literals of their exact values.
const TAKE3_LOOKUP: [(&str, [f32; 3], [f64; 3]); 14] = [
    // CL-interp
    (
        "L4",
        [0.3, 0.3, 0.3],
        [
            0.10000153630971909,
            0.30000460147857666,
            0.30000460147857666,
        ],
    ),
    (
        "L4",
        [0.7, 0.7, 0.7],
        [0.4999954104423523, 0.6999984383583069, 0.6999984383583069],
    ),
    (
        "L4",
        [0.3, 0.6, 0.2],
        [0.1875028759241104, 0.6000031232833862, 0.20000305771827698],
    ),
    (
        "L4",
        [0.8, 0.7, 0.05],
        [0.5624963641166687, 0.6999984383583069, 0.050000764429569244],
    ),
    // CL-range
    (
        "L4",
        [2.0, 0.5, 0.25],
        [0.5000076293945312, 0.5000076293945312, 0.2500038146972656],
    ),
    ("L4", [1.5, 1.2, 0.1], [1.0, 1.0, 0.10000152885913849]),
    (
        "L4",
        [4.0, 0.3, 2.0],
        [0.30000460147857666, 0.30000460147857666, 1.0],
    ),
    (
        "L1",
        [16.0, 16.0, 16.0],
        [0.5000076293945312, 0.5000076293945312, 0.5000076293945312],
    ),
    (
        "L2",
        [2.0, 0.5, 0.25],
        [1.0, 0.5000076293945312, 0.15625238418579102],
    ),
    // CL-neg
    ("L4", [-0.25, 0.5, 1.0], [0.0, 0.5000076293945312, 1.0]),
    ("L4", [0.5, -0.1, 0.2], [0.0, 0.0, 0.20000305771827698]),
    ("L4", [-0.05, -0.05, -0.05], [0.0, 0.0, 0.0]),
    (
        "L1",
        [0.5, -0.1, 0.2],
        [0.2500038146972656, 0.0, 0.10000152885913849],
    ),
    (
        "L2",
        [0.5, -0.1, 0.2],
        [0.5000076293945312, 0.0, 0.10507973283529282],
    ),
];

fn look_named(set: &str) -> Look {
    match set {
        "L1" => L1,
        "L2" => L2,
        "L4" => L4,
        "wave6" => WAVE6,
        "wave11" => WAVE11,
        _ => SKEW,
    }
}

#[test]
fn color_lookup_oracle_reproduces_photoshop_take3() {
    const T: f64 = 2e-3;
    for (set, x, got) in TAKE3_LOOKUP {
        let want = color_lookup_oracle(look_named(set), 1.0, x.map(f64::from));
        for c in 0..3 {
            let err = (got[c] - want[c]).abs();
            assert!(
                err <= T * want[c].abs().max(1.0),
                "{set} {x:?} channel {c}: Photoshop {}, oracle {}",
                got[c],
                want[c]
            );
        }
    }
}

#[test]
fn color_lookup_renders_its_take3_form() {
    let dir = TempDir::new().unwrap();
    // Take2's grid plus three large stored values, where the table value
    // must survive the strength-1 return (the f32 lerp would round it away).
    let mut patches = grid();
    patches.extend([[40000.0; 3], [65504.0, 4096.3, 0.5], [4096.3; 3]]);
    for set in ["L1", "L2", "L4", "skew", "wave6", "wave11"] {
        let look = look_named(set);
        // A plain link of at most 33 nodes is stored at its own grid,
        // so the baked table is the link's own nodes, read tetrahedrally.
        let baked = parse_bytes(&dir, &link_bytes(set, look), None).expect("the link loads");
        assert_eq!(baked["size"], look.n, "{set}");
        // The form reads stored values, whatever the profile: take3
        // measured the same numbers on sRGB and ROMM.
        for document in [IccProfile::srgb(), IccProfile::display_p3()] {
            let doc = float_row(document.bytes(), &patches);
            for (origin, params) in [("link", baked.clone()), ("direct", direct_params(look))] {
                for strength in [1.0, 0.5] {
                    let mut params = params.clone();
                    params["strength"] = json!(strength);
                    let out = render(&add(&doc, "color_lookup", &params, 1.0));
                    for (patch, got) in patches.iter().zip(&out) {
                        let want = color_lookup_oracle(look, strength, patch.map(f64::from));
                        for c in 0..3 {
                            let err = (f64::from(got[c]) - want[c]).abs();
                            assert!(
                                err <= 1e-5 * want[c].abs().max(1.0),
                                "{set} {origin} strength {strength} {patch:?} channel {c}: \
                                 got {}, want {}",
                                got[c],
                                want[c]
                            );
                        }
                        assert_eq!(got[3], 1.0);
                    }
                }
            }
        }
    }
}

/// A 2-node 3D identity is NOT the sheet's unloaded table (that one is 1D):
/// it renders under the link form, so it clamps.
#[test]
fn a_two_node_3d_identity_clamps_as_a_link_does() {
    let identity = Look { n: 2, entry: |v| v };
    let srgb = IccProfile::srgb();
    let doc = float_row(srgb.bytes(), &[[4.0, 0.5, -0.25], [1.0, 0.0, 0.25]]);
    let out = render(&add(&doc, "color_lookup", &direct_params(identity), 1.0));
    assert_eq!(out[0], [1.0, 0.5, 0.0, 1.0]);
    assert_eq!(out[1], [1.0, 0.0, 0.25, 1.0]);
}

/// A table of `kind` and `size` from flat f32 entries, with extra keys.
fn table_params(kind: &str, size: usize, entries: &[f32], extra: Value) -> Value {
    let bytes: Vec<u8> = entries.iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut params = json!({"kind": kind, "size": size, "table": base64(&bytes)});
    for (key, value) in extra.as_object().expect("an object") {
        params[key] = value.clone();
    }
    params
}

/// The 2-node 3D table whose node (r, g, b) holds `lo` or `hi` per axis.
fn two_node(lo: f32, hi: f32) -> Vec<f32> {
    let at = |i: usize| if i == 0 { lo } else { hi };
    let mut out = Vec::with_capacity(24);
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                out.extend([at(r), at(g), at(b)]);
            }
        }
    }
    out
}

/// The patches every other table case renders, declared before running:
/// 1.0, 2.0, 8.0, 0.999, −0.25 and 0.18 on each channel.
const PATCHES_487: [[f32; 3]; 4] = [
    [1.0, 2.0, 8.0],
    [0.999, -0.25, 0.18],
    [8.0, 0.18, 1.0],
    [2.0, 0.999, -0.25],
];

/// A 1D table's oracle: channel c's input taken into the domain
/// `lo…hi` and clamped to its ends, read linearly between `nodes`, then the
/// strength lerp toward it from the stored value.
fn lut_1d_oracle(nodes: &[[f64; 3]], (lo, hi): (f64, f64), strength: f64, x: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|c| {
        let t = ((x[c] - lo) / (hi - lo)).clamp(0.0, 1.0) * (nodes.len() - 1) as f64;
        let i = (t.floor() as usize).min(nodes.len() - 2);
        let f = t - i as f64;
        let mapped = nodes[i][c] + (nodes[i + 1][c] - nodes[i][c]) * f;
        x[c] + (mapped - x[c]) * strength
    })
}

/// `params` rendered over [`PATCHES_487`] in an sRGB float document, each
/// channel within `1e-5·max(1, |want|)` of `oracle`; returns the render.
fn assert_lookup_renders(
    params: &Value,
    oracle: impl Fn([f64; 3]) -> [f64; 3],
    what: &str,
) -> Vec<[f32; 4]> {
    let srgb = IccProfile::srgb();
    let doc = add(
        &float_row(srgb.bytes(), &PATCHES_487),
        "color_lookup",
        params,
        1.0,
    );
    assert_eq!(doc.float_refusal(), None, "{what}");
    let out = render(&doc);
    for (patch, got) in PATCHES_487.iter().zip(&out) {
        let want = oracle(patch.map(f64::from));
        for c in 0..3 {
            let err = (f64::from(got[c]) - want[c]).abs();
            assert!(
                err <= 1e-5 * want[c].abs().max(1.0),
                "{what} {patch:?} channel {c}: got {}, want {}",
                got[c],
                want[c]
            );
        }
        assert_eq!(got[3], 1.0, "{what}");
    }
    out
}

#[test]
fn an_abstract_look_renders_as_its_table() {
    // The kept profile is never read by the math.
    let plain = table_params("3d", 2, &two_node(0.0, 1.0), json!({}));
    let abstract_look = table_params(
        "3d",
        2,
        &two_node(0.0, 1.0),
        json!({"abstract_profile": "AAAA"}),
    );
    let identity = |x: [f64; 3]| x.map(|v| v.clamp(0.0, 1.0));
    let a = assert_lookup_renders(&plain, identity, "plain");
    let b = assert_lookup_renders(&abstract_look, identity, "abstract");
    assert_eq!(
        a.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>(),
        b.iter().map(|p| p.map(f32::to_bits)).collect::<Vec<_>>()
    );
}

#[test]
fn a_1d_table_runs_on_the_stored_values() {
    // Three nodes, the last above white on every channel.
    let nodes = [[0.0, 0.0, 0.0], [0.3, 0.5, 0.7], [1.5, 2.0, 4.0]];
    let entries: Vec<f32> = nodes.iter().flatten().map(|v| *v as f32).collect();
    for strength in [1.0, 0.5] {
        let params = table_params("1d", 3, &entries, json!({"strength": strength}));
        assert_lookup_renders(
            &params,
            |x| lut_1d_oracle(&nodes, (0.0, 1.0), strength, x),
            &format!("1d strength {strength}"),
        );
    }
    // The 2-node half table the sheet does not write.
    let half = [[0.0; 3], [0.5; 3]];
    let entries: Vec<f32> = half.iter().flatten().map(|v| *v as f32).collect();
    assert_lookup_renders(
        &table_params("1d", 2, &entries, json!({})),
        |x| lut_1d_oracle(&half, (0.0, 1.0), 1.0, x),
        "1d half",
    );
}

#[test]
fn a_domain_of_0_to_16_takes_the_stored_values_into_it() {
    // Entries 0…16 over the domain 0…16 are the identity there, so a
    // value above white keeps its magnitude and one below 0 reads 0.
    let nodes = [[0.0; 3], [16.0; 3]];
    let entries: Vec<f32> = nodes.iter().flatten().map(|v| *v as f32).collect();
    let domain = json!({"domain_max": [16.0, 16.0, 16.0]});
    assert_lookup_renders(
        &table_params("1d", 2, &entries, domain.clone()),
        |x| lut_1d_oracle(&nodes, (0.0, 16.0), 1.0, x),
        "1d 0…16",
    );
    assert_lookup_renders(
        &table_params("3d", 2, &two_node(0.0, 16.0), domain),
        |x| x.map(|v| v.clamp(0.0, 16.0)),
        "3d 0…16",
    );
}

#[test]
fn table_entries_outside_0_to_1_are_reached() {
    // A 2-node 3D table whose entries are affine in the input: tetrahedral
    // interpolation reproduces the affine map, `lo + (hi − lo)·clamp(v)`.
    for (lo, hi) in [(0.0, 4.0), (-0.5, 1.0), (0.0, 1.00001)] {
        for strength in [1.0, 0.5] {
            let params = table_params(
                "3d",
                2,
                &two_node(lo as f32, hi as f32),
                json!({"strength": strength}),
            );
            assert_lookup_renders(
                &params,
                |x| {
                    x.map(|v| {
                        let mapped = lo + (hi - lo) * v.clamp(0.0, 1.0);
                        v + (mapped - v) * strength
                    })
                },
                &format!("3d {lo}…{hi} strength {strength}"),
            );
        }
    }
}
