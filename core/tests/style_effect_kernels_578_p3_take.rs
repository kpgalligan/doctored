//! #578 part 3: the shipped Satin law (`style_fx_satin`: the box-pair matte
//! and the per-copy contour, the runner
//! `scripts/photoshop-probe/effect_kernels_578_p3_take.py`, its candidates
//! SA1 and SA2l) against Photoshop 26.10's own pixels, at 8 and 16 bits.
//!
//! - THE IDENTIFY ROWS (always run): the identify take's 80 satin rows,
//!   committed as `fixtures/psd_photoshop/effect_kernels_578_p3/
//!   identify_profiles.json` (README). Each row is rebuilt as the take built
//!   it: a black layer of the take's picture (`identify_578::p3_coverage`)
//!   over opaque black with the satin (255, 226, 170) Normal 100 %, read on
//!   red along y = 112 and x = 128, over the samples of the runner's ROW
//!   (within Euclidean `2 (size + distance) + 2` of the 0.5 contour).
//! - THE CONFIRM TAKE (SKIP when absent): `effect-kernels-578-p3/take1`
//!   under `~/devel/doctored-fixtures/photoshop`, its 80 rows rebuilt from
//!   the take's own `inputs/pictures` (the source at the file's depth over
//!   opaque black) with each row's Doctored style (`rows.json`), against
//!   Photoshop's readback `rows/<case>.png` on the row's channel over the
//!   ROW.
//!
//! Every row meets the runner's gate, max <= 2 and mean <= 0.5 codes (16
//! bits as `/ 257`), or asserts its own measured number (`PINNED`, rounded
//! up in the third decimal), never a wider one. The pinned rows are 24
//! contoured rows whose max the kernel's byte matte lifts past 2 codes
//! ([`PINNED`]).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::identify_578::{p3_coverage, render_rgb_alpha};

const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

fn identify_fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/effect_kernels_578_p3/identify_profiles.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("the committed fixture"))
        .expect("json")
}

/// The identify satin of a row name `sa_<pic>_<contour>_s<S>_d<D>_a<A>_i<I>_<d>`
/// as `(picture, size, distance, style)`.
fn identify_case(name: &str) -> (String, f64, f64, Value) {
    let parts: Vec<&str> = name.split('_').collect();
    let field = |tag: &str| -> f64 {
        parts
            .iter()
            .find_map(|p| p.strip_prefix(tag).and_then(|v| v.parse().ok()))
            .unwrap_or_else(|| panic!("{name}: no {tag}"))
    };
    let (size, distance, angle) = (field("s"), field("d"), field("a"));
    let invert = field("i") != 0.0;
    let mut fx = json!({"type":"satin","blend":"normal","color":"#ffe2aa","opacity":1,
        "size":size,"distance":distance,"angle":angle,"invert":invert});
    let contour = match parts[2] {
        "lin" => None,
        "cone" => Some("cone"),
        "hr" => Some("half_round"),
        "rsd" => Some("rolling_slope_descending"),
        other => panic!("{name}: unknown contour tag {other}"),
    };
    if let Some(preset) = contour {
        fx["contour"] = json!({ "preset": preset });
    }
    (
        parts[1].to_string(),
        size,
        distance,
        json!({ "effects": [fx] }),
    )
}

/// `(max, mean)` of `|ours − theirs|`.
fn error(ours: &[f64], theirs: &[f64], what: &str) -> (f64, f64) {
    assert!(!ours.is_empty(), "{what}: no sample in the ROW");
    let e: Vec<f64> = ours
        .iter()
        .zip(theirs)
        .map(|(o, t)| (o - t).abs())
        .collect();
    let max = e.iter().copied().fold(0.0, f64::max);
    (max, e.iter().sum::<f64>() / e.len() as f64)
}

/// The Euclidean distance between pixel centres from `(x, y)` to the
/// nearest pixel on the other side of the 0.5 contour (the runner's
/// `max(d_out, d_in)`), brute force over the pixels that touch the other
/// side. The same brute force as `style_bevel_kernels_578_take.rs`'s
/// `contour_distance`, here per sample.
struct Edges {
    w: i64,
    inside: Vec<bool>,
    edge: [Vec<(i64, i64)>; 2],
}

impl Edges {
    fn new(w: u32, h: u32, a: &[f64]) -> Edges {
        let (wi, hi) = (i64::from(w), i64::from(h));
        let inside: Vec<bool> = a.iter().map(|&v| v >= 0.5).collect();
        let at = |x: i64, y: i64| inside[(y * wi + x) as usize];
        let mut edge: [Vec<(i64, i64)>; 2] = [Vec::new(), Vec::new()];
        for y in 0..hi {
            for x in 0..wi {
                let me = at(x, y);
                let touches = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dy)| {
                    let (nx, ny) = (x + dx, y + dy);
                    (0..wi).contains(&nx) && (0..hi).contains(&ny) && at(nx, ny) != me
                });
                if touches {
                    edge[usize::from(me)].push((x, y));
                }
            }
        }
        Edges {
            w: wi,
            inside,
            edge,
        }
    }

    fn distance(&self, x: i64, y: i64) -> f64 {
        let other = &self.edge[usize::from(!self.inside[(y * self.w + x) as usize])];
        let d2 = other
            .iter()
            .map(|&(qx, qy)| (qx - x).pow(2) + (qy - y).pow(2))
            .min()
            .unwrap_or(i64::MAX);
        (d2 as f64).sqrt()
    }
}

/// Rows that miss the gate, at their own measured `(max, mean)` (module
/// doc): contoured rows only, every Linear row meets it. The kernel rounds
/// the matte to a byte before the contour, whose table rises up to 2–3
/// codes per code on Cone, Half Round's foot and Rolling Slope -
/// Descending, so a contoured row's max reads 1–3 codes above the runner's
/// unrounded model (2.03 at worst on the confirm take); every mean stays
/// under 0.3.
const PINNED: &[(&str, f64, f64)] = &[
    ("sa_disc_cone_s16_d5_a45_i0_8", 3.000, 0.256),
    ("sa_disc_cone_s16_d5_a45_i1_8", 3.000, 0.292),
    ("sa_disc_cone_s6_d5_a45_i0_8", 3.000, 0.136),
    ("sa_disc_rsd_s6_d5_a45_i0_8", 3.000, 0.125),
    ("sa_disc_rsd_s6_d5_a45_i1_8", 4.000, 0.209),
    ("sa_ell2_cone_s8_d6_a75_i0_16", 3.479, 0.151),
    ("sa_ell2_cone_s8_d6_a75_i0_8", 4.000, 0.168),
    ("sa_ell2_cone_s8_d6_a75_i1_16", 3.471, 0.154),
    ("sa_ell2_cone_s8_d6_a75_i1_8", 3.000, 0.176),
    ("sa_ell2_hr_s8_d6_a75_i0_16", 2.545, 0.077),
    ("sa_ell2_hr_s8_d6_a75_i0_8", 3.000, 0.100),
    ("sa_ell2_hr_s8_d6_a75_i1_16", 2.537, 0.079),
    ("sa_ell_cone_s16_d5_a45_i0_8", 3.000, 0.228),
    ("sa_ell_cone_s16_d5_a45_i1_8", 3.000, 0.245),
    ("sa_ell_cone_s6_d5_a45_i0_8", 3.000, 0.183),
    ("sa_ell_rsd_s6_d5_a45_i0_8", 4.000, 0.141),
    ("sa_ell_rsd_s6_d5_a45_i1_8", 5.000, 0.193),
    ("sa_rr_cone_s8_d6_a75_i0_16", 2.954, 0.075),
    ("sa_rr_cone_s8_d6_a75_i0_8", 3.000, 0.148),
    ("sa_rr_cone_s8_d6_a75_i1_16", 2.962, 0.077),
    ("sa_rr_cone_s8_d6_a75_i1_8", 3.000, 0.070),
    ("sa_rr_hr_s8_d6_a75_i0_16", 2.250, 0.071),
    ("sa_rr_hr_s8_d6_a75_i0_8", 3.000, 0.091),
    ("sa_rr_hr_s8_d6_a75_i1_16", 2.242, 0.073),
];

/// `None` when the row meets the gate or its pin, else why not.
fn check_row(name: &str, (max, mean): (f64, f64)) -> Option<String> {
    eprintln!("{name}: {max:.6} / {mean:.6}");
    if let Some(&(_, pm, pn)) = PINNED.iter().find(|(n, _, _)| *n == name) {
        return (max > pm || mean > pn)
            .then(|| format!("{name}: max {max:.4} (pinned {pm}), mean {mean:.4} (pinned {pn})"));
    }
    (max > MAX || mean > MEAN)
        .then(|| format!("{name}: max {max:.4}, mean {mean:.4} against {MAX} / {MEAN}"))
}

fn assert_none_failed(failed: &[String]) {
    assert!(
        failed.is_empty(),
        "{} rows fail:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

fn floats(v: &Value, key: &str, name: &str) -> Vec<f64> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: no {key}"))
        .iter()
        .map(|x| x.as_f64().expect("a number"))
        .collect()
}

/// Each identify picture's size, alpha and contour edges, built once.
type Pictures = Vec<(String, (u32, u32, Vec<f64>, Edges))>;

#[test]
fn the_identify_satin_rows_meet_the_gate_or_their_pins() {
    let fixture = identify_fixture();
    let rows = fixture.as_object().expect("rows");
    assert_eq!(rows.len(), 80, "the identify take's 80 satin rows");
    let mut pictures: Pictures = Vec::new();
    let mut failed = Vec::new();
    for (name, row) in rows {
        assert_eq!(row["take"], "effect-kernels-578-p3/identify1", "{name}");
        assert_eq!(row["depth"], 8, "{name}");
        assert_eq!(row["channel"], 0, "{name}");
        let (pic, size, distance, style) = identify_case(name);
        if !pictures.iter().any(|(k, _)| *k == pic) {
            let (w, h, a) = p3_coverage(&pic);
            let edges = Edges::new(w, h, &a);
            pictures.push((pic.clone(), (w, h, a, edges)));
        }
        let (w, h, a, edges) = &pictures.iter().find(|(k, _)| *k == pic).expect("picture").1;
        let ours = render_rgb_alpha(8, *w, *h, a, [0.0; 3], [0.0; 3], &style);
        let (ps_row, ps_col) = (floats(row, "row", name), floats(row, "col", name));
        assert_eq!(
            (ps_row.len(), ps_col.len()),
            (*w as usize, *h as usize),
            "{name}"
        );
        let radius = 2.0 * (size + distance) + 2.0;
        let (mut o, mut t) = (Vec::new(), Vec::new());
        let samples = (0..*w)
            .map(|x| (x, 112u32, ps_row[x as usize]))
            .chain((0..*h).map(|y| (128u32, y, ps_col[y as usize])));
        for (x, y, ps) in samples {
            if edges.distance(i64::from(x), i64::from(y)) <= radius {
                o.push(ours[(y * *w + x) as usize][0]);
                t.push(ps);
            }
        }
        failed.extend(check_row(name, error(&o, &t, name)));
    }
    assert_none_failed(&failed);
}

// ------------------------------------------------------ the confirm take --

fn take_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h).join("devel/doctored-fixtures/photoshop/effect-kernels-578-p3/take1")
    })
}

/// The unit alpha of every pixel of a picture.
fn picture_alpha(path: &Path) -> (u32, u32, Vec<f64>) {
    let img = image::open(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .to_rgba16();
    let (w, h) = img.dimensions();
    let a = img.pixels().map(|p| f64::from(p.0[3]) / 65535.0).collect();
    (w, h, a)
}

type Fields = Vec<((String, u32), (u32, u32, Vec<f64>, Vec<f64>))>;

/// One confirm row: the render of its picture with its style against
/// Photoshop's readback, on its channel over the ROW (module doc).
fn take_row(take: &Path, name: &str, row: &Value, fields: &mut Fields) -> (f64, f64) {
    let pic = row["picture"].as_str().expect("picture");
    let depth = row["depth"].as_u64().expect("depth") as u32;
    let channel = row["channel"].as_u64().expect("channel") as usize;
    let fx = &row["style"]["effects"][0];
    let radius =
        2.0 * (fx["size"].as_f64().expect("size") + fx["distance"].as_f64().expect("d")) + 2.0;
    let key = (pic.to_string(), depth);
    if !fields.iter().any(|(k, _)| *k == key) {
        let path = take.join(format!("inputs/pictures/{pic}_source_{depth}.png"));
        let (w, h, a) = picture_alpha(&path);
        let edges = Edges::new(w, h, &a);
        let d = (0..i64::from(h))
            .flat_map(|y| (0..i64::from(w)).map(move |x| (x, y)))
            .map(|(x, y)| edges.distance(x, y))
            .collect();
        fields.push((key.clone(), (w, h, a, d)));
    }
    let (w, h, a, dist) = &fields.iter().find(|(k, _)| *k == key).expect("field").1;
    let ours = render_rgb_alpha(depth, *w, *h, a, [0.0; 3], [0.0; 3], &row["style"]);
    let theirs = image::open(take.join(format!("rows/{name}.png")))
        .expect("the row's readback")
        .to_rgba16();
    assert_eq!(
        theirs.dimensions(),
        (*w, *h),
        "{name}: the readback's size is the picture's"
    );
    let (mut o, mut t) = (Vec::new(), Vec::new());
    for ((mine, ps), &d) in ours.iter().zip(theirs.pixels()).zip(dist) {
        if d <= radius {
            o.push(mine[channel]);
            t.push(f64::from(ps.0[channel]) / 257.0);
        }
    }
    error(&o, &t, name)
}

#[test]
fn the_confirm_take_satin_rows_meet_the_gate_or_their_pins() {
    let Some(take) = take_dir() else {
        eprintln!("SKIP: HOME is unset, so the confirm take cannot be found");
        return;
    };
    let rows_path = take.join("rows.json");
    if !rows_path.is_file() {
        eprintln!("SKIP: {} absent", rows_path.display());
        return;
    }
    let rows: Value =
        serde_json::from_str(&std::fs::read_to_string(&rows_path).expect("rows.json"))
            .expect("json");
    let rows = rows.as_object().expect("rows");
    assert_eq!(rows.len(), 80, "40 satin cases per depth");
    let mut fields = Fields::new();
    let mut failed = Vec::new();
    for (name, row) in rows {
        assert_eq!(row["kind"], "satin", "{name}");
        failed.extend(check_row(name, take_row(&take, name, row, &mut fields)));
    }
    assert_none_failed(&failed);
}
