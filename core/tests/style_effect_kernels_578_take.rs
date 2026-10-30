//! #2 part 2, phase 1: the shipped box-pair law of the Drop Shadow, the
//! Inner Shadow and the Outer Glow (`style_matte`, the runner
//! `scripts/photoshop-probe/effect_kernels_578_take.py`, its candidates D1,
//! I1 and O1) against Photoshop 26.10's own pixels, at 8 and 16 bits.
//!
//! - THE IDENTIFY ROWS (always run): identify-578 take3's 80 and take4's 96
//!   Outer Glow profiles, committed as
//!   `fixtures/psd_photoshop/identify_578/outer_glow_take34_profiles.json`
//!   (README). Each row is rebuilt as the takes built it: a black layer of
//!   the take's picture over opaque black (`identify_578::alpha` binary,
//!   `take4_coverage` anti-aliased, its 16-bit alpha `floor(65535 a + 1/2)`)
//!   with a white Normal 100 % glow, read on red as the take's profile
//!   (take3: the half plane's columns 128 ... 319, the disc's bins −32 ...
//!   39, the corner's outside diagonal; take4: the three edge bands'
//!   columns 128 ... 319, the disc's bins), against Photoshop's values over
//!   the whole profile.
//! - THE CONFIRM TAKE (SKIP when absent): `effect-kernels-578/take1` under
//!   `~/devel/doctored-fixtures/photoshop`, its 88 rows rebuilt from the
//!   take's own `inputs/pictures` (the opaque backdrop and the source at the
//!   file's depth) with each row's Doctored style (`rows.json`), against
//!   Photoshop's readback `rows/<case>.png` over every RGB sample.
//!
//! Every row meets the runner's gate, max <= 2 and mean <= 0.5 codes (16
//! bits as `/ 257`), or asserts its own measured number (`PINNED`, rounded
//! up in the third decimal), never a wider one. The pinned rows are the
//! runner's named bins (Spread 100 % at Size 8, 12 and 32: Photoshop's band
//! past S; Size 1 at n = S; take4's disc at Spread 50 and 80 %), the Spread
//! 100 % rows today's kernel keeps, and the confirm take's rows the model
//! misses (its RECORDED section).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::identify_578::{
    alpha, profile2, render16_alpha, render8_alpha, render_rgb_alpha, take4_coverage,
};

const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

fn identify_fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/identify_578/outer_glow_take34_profiles.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("the committed fixture"))
        .expect("json")
}

/// `(size, spread, range)` of a row name `og[4]_<shape>_s<S>_p<P>_r<R>_<d>`.
fn params(name: &str) -> (f64, f64, f64) {
    let field = |tag: &str| -> f64 {
        name.split('_')
            .find_map(|p| p.strip_prefix(tag).and_then(|v| v.parse().ok()))
            .unwrap_or_else(|| panic!("{name}: no {tag}"))
    };
    (field("s"), field("p"), field("r"))
}

fn glow_style(size: f64, spread: f64, range: f64) -> Value {
    json!({"effects":[{"type":"outer_glow","blend":"normal","color":"#ffffff","opacity":1,
        "size":size,"spread":spread / 100.0,"range":range / 100.0,"noise":0,"jitter":0}]})
}

/// The red codes of a take3 / take4 row's render (module doc).
fn identify_render(shape: &str, depth: u32, style: &Value) -> (u32, Vec<f64>) {
    let (w, h, coverage) = match shape {
        "aahalf" | "aadisc" => take4_coverage(shape),
        _ => {
            let (w, h, inside) = alpha(shape);
            (
                w,
                h,
                inside.iter().map(|&i| f64::from(u8::from(i))).collect(),
            )
        }
    };
    let codes = if depth == 8 {
        let a: Vec<u8> = coverage
            .iter()
            .map(|&c| (255.0 * c + 0.5).floor() as u8)
            .collect();
        render8_alpha(w, h, &a, style)
    } else {
        let a: Vec<u16> = coverage
            .iter()
            .map(|&c| (65535.0 * c + 0.5).floor() as u16)
            .collect();
        render16_alpha(w, h, &a, style)
    };
    (w, codes)
}

/// The take's profile of a render (module doc).
fn identify_profile(shape: &str, w: u32, codes: &[f64]) -> Vec<f64> {
    let at = |x: u32, y: u32| codes[(y * w + x) as usize];
    match shape {
        "half" => profile2("half", w, codes),
        "disc" | "aadisc" => profile2("disc", w, codes),
        "corner" => (0..32).map(|k| at(31 - k, 31 - k)).collect(),
        "aahalf" => [(80, 144), (240, 304), (400, 464)]
            .iter()
            .flat_map(|&(y0, y1)| {
                (128..320).map(move |x| (y0..y1).map(|y| at(x, y)).sum::<f64>() / 64.0)
            })
            .collect(),
        _ => panic!("unknown shape {shape}"),
    }
}

fn error(ours: &[f64], theirs: &[f64], what: &str) -> (f64, f64) {
    assert_eq!(ours.len(), theirs.len(), "{what}: profile length");
    let e: Vec<f64> = ours
        .iter()
        .zip(theirs)
        .map(|(a, b)| (a - b).abs())
        .collect();
    (
        e.iter().copied().fold(0.0, f64::max),
        e.iter().sum::<f64>() / e.len() as f64,
    )
}

fn identify_row(name: &str, row: &Value) -> (f64, f64) {
    let shape = row["shape"].as_str().unwrap();
    let depth = row["depth"].as_u64().unwrap() as u32;
    let (size, spread, range) = params(name);
    let (w, codes) = identify_render(shape, depth, &glow_style(size, spread, range));
    let ours = identify_profile(shape, w, &codes);
    let theirs: Vec<f64> = row["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    error(&ours, &theirs, name)
}

/// The rows that do not meet the gate, each with its measured `(max,
/// mean)` in codes, rounded up in the third decimal (module doc).
const PINNED: &[(&str, f64, f64)] = &[
    // take3 (14 of 80): the bins (Size 1 at n = S, Photoshop's 6-code tail;
    // Spread 100 % at Size 8 and 32, its band past S) and the Size 8 disc at
    // Spread 100 % (today's binarized band on its edge).
    ("og_corner_s32_p100_r50_16", 255.001, 16.344),
    ("og_corner_s32_p100_r50_8", 255.001, 16.344),
    ("og_disc_s8_p100_r50_16", 4.801, 0.072),
    ("og_disc_s8_p100_r50_8", 4.815, 0.067),
    ("og_half_s1_p100_r50_16", 5.977, 0.037),
    ("og_half_s1_p100_r50_8", 6.001, 0.032),
    ("og_half_s1_p50_r50_16", 5.977, 0.037),
    ("og_half_s1_p50_r50_8", 6.001, 0.032),
    ("og_half_s1_p80_r50_16", 5.977, 0.037),
    ("og_half_s1_p80_r50_8", 6.001, 0.032),
    ("og_half_s32_p100_r50_16", 255.001, 2.662),
    ("og_half_s32_p100_r50_8", 255.001, 2.657),
    ("og_half_s8_p100_r50_16", 55.783, 0.296),
    ("og_half_s8_p100_r50_8", 56.001, 0.292),
    // take4 (42 of 96): the disc at Spread 50 and 80 % (a bin: no declared
    // form reads the curved contour) and every Spread 100 % row (today's
    // kernel: its binarized band, and Photoshop's band past S at Size 12
    // and 32).
    ("og4_aadisc_s12_p100_r100_16", 127.001, 21.280),
    ("og4_aadisc_s12_p100_r100_8", 127.001, 21.276),
    ("og4_aadisc_s12_p100_r50_16", 17.671, 0.250),
    ("og4_aadisc_s12_p100_r50_8", 17.635, 0.245),
    ("og4_aadisc_s12_p50_r100_16", 2.145, 0.197),
    ("og4_aadisc_s12_p50_r100_8", 2.110, 0.193),
    ("og4_aadisc_s12_p50_r50_16", 4.954, 0.230),
    ("og4_aadisc_s12_p50_r50_8", 4.929, 0.225),
    ("og4_aadisc_s12_p80_r100_16", 11.026, 0.372),
    ("og4_aadisc_s12_p80_r100_8", 11.086, 0.370),
    ("og4_aadisc_s12_p80_r50_16", 19.569, 0.379),
    ("og4_aadisc_s12_p80_r50_8", 19.559, 0.374),
    ("og4_aadisc_s32_p100_r100_16", 127.001, 56.454),
    ("og4_aadisc_s32_p100_r100_8", 127.001, 56.450),
    ("og4_aadisc_s32_p50_r100_16", 2.338, 0.549),
    ("og4_aadisc_s32_p50_r100_8", 2.312, 0.543),
    ("og4_aadisc_s32_p50_r50_16", 5.434, 0.622),
    ("og4_aadisc_s32_p50_r50_8", 5.512, 0.614),
    ("og4_aadisc_s32_p80_r100_16", 10.294, 0.965),
    ("og4_aadisc_s32_p80_r100_8", 10.309, 0.963),
    ("og4_aadisc_s32_p80_r50_16", 20.979, 0.977),
    ("og4_aadisc_s32_p80_r50_8", 20.904, 0.973),
    ("og4_aadisc_s4_p100_r100_16", 127.001, 6.691),
    ("og4_aadisc_s4_p100_r100_8", 127.001, 6.687),
    ("og4_aadisc_s4_p100_r50_16", 25.815, 0.372),
    ("og4_aadisc_s4_p100_r50_8", 25.832, 0.368),
    ("og4_aadisc_s4_p80_r100_16", 2.636, 0.087),
    ("og4_aadisc_s4_p80_r100_8", 2.617, 0.081),
    ("og4_aadisc_s4_p80_r50_16", 5.726, 0.110),
    ("og4_aadisc_s4_p80_r50_8", 5.624, 0.105),
    ("og4_aahalf_s12_p100_r100_16", 175.001, 8.428),
    ("og4_aahalf_s12_p100_r100_8", 175.001, 8.422),
    ("og4_aahalf_s12_p100_r50_16", 255.001, 0.635),
    ("og4_aahalf_s12_p100_r50_8", 255.001, 0.631),
    ("og4_aahalf_s32_p100_r100_16", 255.001, 24.381),
    ("og4_aahalf_s32_p100_r100_8", 255.001, 24.376),
    ("og4_aahalf_s32_p100_r50_16", 255.001, 3.105),
    ("og4_aahalf_s32_p100_r50_8", 255.001, 3.099),
    ("og4_aahalf_s4_p100_r100_16", 127.001, 2.553),
    ("og4_aahalf_s4_p100_r100_8", 127.001, 2.547),
    ("og4_aahalf_s4_p100_r50_16", 129.491, 0.231),
    ("og4_aahalf_s4_p100_r50_8", 129.001, 0.224),
    // The confirm take's Outer Glow (24 of 44): Spread 70 % (the model's
    // own misses, up to 6.5 / 0.054) and Spread 100 % (today's kernel, the
    // band past S).
    ("og_hp_s20_p100_r50_16", 68.004, 0.356),
    ("og_hp_s20_p100_r50_8", 68.001, 0.351),
    ("og_hp_s20_p70_r50_16", 5.584, 0.038),
    ("og_hp_s20_p70_r50_8", 6.001, 0.027),
    ("og_hp_s20_p70_r75_16", 3.510, 0.034),
    ("og_hp_s20_p70_r75_8", 4.001, 0.024),
    ("og_hp_s6_p100_r50_16", 68.004, 0.215),
    ("og_hp_s6_p100_r50_8", 68.001, 0.208),
    ("og_hp_s6_p70_r50_16", 4.335, 0.010),
    ("og_hp_s6_p70_r50_8", 5.001, 0.004),
    ("og_hp_s6_p70_r75_16", 2.639, 0.009),
    ("og_hp_s6_p70_r75_8", 3.001, 0.003),
    ("og_rr_s20_p100_r50_16", 67.989, 0.263),
    ("og_rr_s20_p100_r50_8", 68.001, 0.258),
    ("og_rr_s20_p70_r50_16", 6.226, 0.055),
    ("og_rr_s20_p70_r50_8", 7.001, 0.047),
    ("og_rr_s20_p70_r75_16", 4.265, 0.050),
    ("og_rr_s20_p70_r75_8", 5.001, 0.041),
    ("og_rr_s6_p100_r50_16", 41.429, 0.102),
    ("og_rr_s6_p100_r50_8", 41.001, 0.095),
    ("og_rr_s6_p70_r50_16", 5.833, 0.015),
    ("og_rr_s6_p70_r50_8", 6.001, 0.009),
    ("og_rr_s6_p70_r75_16", 3.818, 0.014),
    ("og_rr_s6_p70_r75_8", 4.001, 0.007),
    // The confirm take's shadows (all 32): every mean within 0.061 codes,
    // the max on the pictures' partial pixels, where the shadows' composite
    // is not the model's (a disclosed residual, the #2 comment): the drop
    // shadow's knocked-out landing, `m (1 − a)²` where Photoshop reads
    // `m (1 − a)`, is the residual DK1 fixed in #2 part 4; its pins stay
    // unmoved upper bounds because the take1 `rows.json` is absent. The inner
    // shadow composites over the layer's composite where Photoshop
    // composites inside the layer before it lands at `a`.
    ("ds_hp_s10_p20_d4_a180_16", 33.288, 0.061),
    ("ds_hp_s10_p20_d4_a180_8", 33.001, 0.059),
    ("ds_hp_s16_p0_d0_a90_16", 17.086, 0.040),
    ("ds_hp_s16_p0_d0_a90_8", 18.001, 0.041),
    ("ds_hp_s6_p0_d4_a90_16", 16.685, 0.034),
    ("ds_hp_s6_p0_d4_a90_8", 17.001, 0.025),
    ("ds_hp_s7_p50_d6_a135_16", 34.557, 0.061),
    ("ds_hp_s7_p50_d6_a135_8", 34.001, 0.055),
    ("ds_rr_s10_p20_d4_a180_16", 34.001, 0.040),
    ("ds_rr_s10_p20_d4_a180_8", 34.001, 0.041),
    ("ds_rr_s16_p0_d0_a90_16", 16.253, 0.037),
    ("ds_rr_s16_p0_d0_a90_8", 16.001, 0.036),
    ("ds_rr_s6_p0_d4_a90_16", 33.234, 0.031),
    ("ds_rr_s6_p0_d4_a90_8", 33.001, 0.024),
    ("ds_rr_s7_p50_d6_a135_16", 36.106, 0.047),
    ("ds_rr_s7_p50_d6_a135_8", 36.001, 0.041),
    ("is_hp_s10_p20_d4_a180_16", 25.265, 0.027),
    ("is_hp_s10_p20_d4_a180_8", 25.001, 0.020),
    ("is_hp_s16_p0_d0_a90_16", 24.288, 0.038),
    ("is_hp_s16_p0_d0_a90_8", 24.001, 0.032),
    ("is_hp_s6_p0_d4_a90_16", 31.717, 0.038),
    ("is_hp_s6_p0_d4_a90_8", 31.001, 0.032),
    ("is_hp_s7_p50_d6_a135_16", 33.012, 0.039),
    ("is_hp_s7_p50_d6_a135_8", 33.001, 0.033),
    ("is_rr_s10_p20_d4_a180_16", 33.067, 0.041),
    ("is_rr_s10_p20_d4_a180_8", 33.001, 0.035),
    ("is_rr_s16_p0_d0_a90_16", 19.191, 0.035),
    ("is_rr_s16_p0_d0_a90_8", 20.001, 0.028),
    ("is_rr_s6_p0_d4_a90_16", 32.109, 0.033),
    ("is_rr_s6_p0_d4_a90_8", 32.001, 0.025),
    ("is_rr_s7_p50_d6_a135_16", 34.464, 0.045),
    ("is_rr_s7_p50_d6_a135_8", 35.001, 0.040),
];

fn assert_row(name: &str, (max, mean): (f64, f64)) {
    if let Some(&(_, pm, pn)) = PINNED.iter().find(|(n, _, _)| *n == name) {
        assert!(
            max <= pm && mean <= pn,
            "{name}: max {max:.3} (pinned {pm}), mean {mean:.4} (pinned {pn})"
        );
        return;
    }
    assert!(
        max <= MAX && mean <= MEAN,
        "{name}: max {max:.3}, mean {mean:.4} against {MAX} / {MEAN}"
    );
}

#[test]
fn the_identify_outer_glow_rows_meet_the_gate_or_their_pins() {
    let fixture = identify_fixture();
    let rows = fixture.as_object().unwrap();
    assert_eq!(rows.len(), 176, "take3's 80 and take4's 96 rows");
    for (name, row) in rows {
        assert_row(name, identify_row(name, row));
    }
}

// ------------------------------------------------------ the confirm take --

fn take_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h).join("devel/doctored-fixtures/photoshop/effect-kernels-578/take1")
    })
}

/// The unit RGB of `path`'s first pixel and the unit alpha of every pixel.
fn picture(path: &Path) -> (u32, u32, [f64; 3], Vec<f64>) {
    let img = image::open(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .to_rgba16();
    let (w, h) = img.dimensions();
    let first = img.get_pixel(0, 0).0;
    let rgb = [0, 1, 2].map(|k| f64::from(first[k]) / 65535.0);
    let a = img.pixels().map(|p| f64::from(p.0[3]) / 65535.0).collect();
    (w, h, rgb, a)
}

/// One confirm row: the render of its picture with its style against
/// Photoshop's readback, every RGB sample (module doc).
fn take_row(take: &Path, name: &str, row: &Value) -> (f64, f64) {
    let pic = row["picture"].as_str().unwrap();
    let depth = row["depth"].as_u64().unwrap() as u32;
    let pictures = take.join("inputs/pictures");
    let (w, h, backdrop, _) = picture(&pictures.join(format!("{pic}_backdrop_{depth}.png")));
    let (_, _, source, a) = picture(&pictures.join(format!("{pic}_source_{depth}.png")));
    let ours = render_rgb_alpha(depth, w, h, &a, source, backdrop, &row["style"]);
    let theirs = image::open(take.join(format!("rows/{name}.png")))
        .expect("the row's readback")
        .to_rgba16();
    assert_eq!(
        theirs.dimensions(),
        (w, h),
        "{name}: the readback's size is the picture's"
    );
    let mut e = Vec::with_capacity(ours.len() * 3);
    for (o, t) in ours.iter().zip(theirs.pixels()) {
        for (&ours_k, &theirs_k) in o.iter().zip(&t.0[..3]) {
            e.push((ours_k - f64::from(theirs_k) / 257.0).abs());
        }
    }
    (
        e.iter().copied().fold(0.0, f64::max),
        e.iter().sum::<f64>() / e.len() as f64,
    )
}

#[test]
fn the_confirm_take_rows_meet_the_gate_or_their_pins() {
    let Some(take) = take_dir() else {
        eprintln!("SKIP: HOME is unset, so the confirm take cannot be found");
        return;
    };
    let rows_path = take.join("rows.json");
    if !rows_path.is_file() {
        eprintln!("SKIP: {} absent", rows_path.display());
        return;
    }
    let rows: Value = serde_json::from_str(&std::fs::read_to_string(&rows_path).unwrap()).unwrap();
    let rows = rows.as_object().unwrap();
    assert_eq!(rows.len(), 88, "44 cases per depth");
    for (name, row) in rows {
        assert_row(name, take_row(&take, name, row));
    }
}
