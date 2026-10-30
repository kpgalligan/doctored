//! #578 part 4: the contoured Inner Shadow read on the unrounded matte
//! (`style_fx_inner_shadow::CONTOUR_UNROUNDED`, candidate IC1) and the
//! knocked-out Drop Shadow landing under the layer
//! (`style_composite::SHADOW_UNDER_LAYER`, DK1) against Photoshop 26.10's
//! own pixels (the runner `scripts/photoshop-probe/effect_kernels_578_p4_take.py`).
//!
//! - THE IDENTIFY ROWS (always run): the identify take's 80 Inner Shadow and
//!   24 Drop Shadow rows, committed as `fixtures/psd_photoshop/
//!   effect_kernels_578_p4/identify_profiles.json` (README). Each row is
//!   rebuilt as the take built it on its picture (`identify_578::
//!   p4_coverage`): an Inner Shadow (255, 226, 170) Normal 100 %, Angle 120,
//!   on a black layer over opaque black, read on red along y = 112 and x =
//!   128 over the runner's ROW (within Euclidean `2 (size + distance) + 2` of
//!   the 0.5 contour); a Drop Shadow (48, 24, 112) Size 8, Distance 5, Angle
//!   135 under a (24, 64, 148) layer over (196, 204, 92), every RGB sample
//!   on that row and column.
//! - THE CONFIRM TAKE (SKIP when absent): `effect-kernels-578-p4/take1`
//!   under `~/devel/doctored-fixtures/photoshop`, its 64 rows rebuilt from
//!   the take's own `inputs/pictures` with each row's Doctored style
//!   (`rows.json`), against Photoshop's readback `rows/<case>.png`: the
//!   Inner Shadow rows on blue over the ROW, the Drop and Inner Shadow rows
//!   over every RGB sample.
//!
//! Every row meets the runner's gate, max <= 2 and mean <= 0.5 codes (16
//! bits as `/ 257`), or asserts its own measured number (`PINNED`, rounded
//! up in the third decimal), never a wider one ([`PINNED`] says why each
//! misses).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::identify_578::{p4_coverage, render_rgb_alpha};

const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;

fn identify_fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/effect_kernels_578_p4/identify_profiles.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("the committed fixture"))
        .expect("json")
}

fn field(name: &str, tag: &str) -> f64 {
    name.split('_')
        .skip(2)
        .find_map(|p| p.strip_prefix(tag).and_then(|v| v.parse().ok()))
        .unwrap_or_else(|| panic!("{name}: no {tag}"))
}

/// An identify Inner Shadow `ic_<pic>_<contour>_s<S>_c<C>_d<D>_<d>` as
/// `(picture, size + distance, style)`.
fn ic_case(name: &str) -> (String, f64, Value) {
    let parts: Vec<&str> = name.split('_').collect();
    let (size, choke, distance) = (field(name, "s"), field(name, "c"), field(name, "d"));
    let mut fx = json!({"type":"inner_shadow","blend":"normal","color":"#ffe2aa","opacity":1,
        "use_global_light":false,"angle":120,"distance":distance,"size":size,
        "choke":choke / 100.0,"noise":0});
    let contour = match parts[2] {
        "lin" => None,
        "cone" => Some("cone"),
        "coni" => Some("cone_inverted"),
        "hr" => Some("half_round"),
        "ring" => Some("ring"),
        other => panic!("{name}: unknown contour tag {other}"),
    };
    if let Some(preset) = contour {
        fx["contour"] = json!({ "preset": preset });
    }
    (
        parts[1].to_string(),
        size + distance,
        json!({ "effects": [fx] }),
    )
}

/// An identify Drop Shadow `ds_<pic>_f<F>_k<K>_<n80|m75>_<d>` as `(picture,
/// style)`.
fn ds_case(name: &str) -> (String, Value) {
    let parts: Vec<&str> = name.split('_').collect();
    let (fill, knocked) = (field(name, "f"), field(name, "k") != 0.0);
    let (blend, opacity) = match parts[4] {
        "n80" => ("normal", 0.8),
        "m75" => ("multiply", 0.75),
        other => panic!("{name}: unknown blend tag {other}"),
    };
    let fx = json!({"type":"drop_shadow","blend":blend,"color":"#301870","opacity":opacity,
        "use_global_light":false,"angle":135,"distance":5,"size":8,"spread":0,"noise":0,
        "layer_knocks_out":knocked});
    (
        parts[1].to_string(),
        json!({"fill_opacity": fill / 100.0, "effects": [fx]}),
    )
}

/// `(max, mean)` of `|ours − theirs|`.
fn error(ours: &[f64], theirs: &[f64], what: &str) -> (f64, f64) {
    assert!(!ours.is_empty(), "{what}: no sample");
    let e: Vec<f64> = ours
        .iter()
        .zip(theirs)
        .map(|(o, t)| (o - t).abs())
        .collect();
    let max = e.iter().copied().fold(0.0, f64::max);
    (max, e.iter().sum::<f64>() / e.len() as f64)
}

/// The Euclidean distance between pixel centres from each pixel to the
/// nearest pixel on the other side of the 0.5 contour (the runner's
/// `max(d_out, d_in)`), brute force over the pixels that touch the other
/// side (`style_effect_kernels_578_p3_take.rs`'s `Edges`, per pixel).
fn contour_distance(w: u32, h: u32, a: &[f64]) -> Vec<f64> {
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
    (0..hi)
        .flat_map(|y| (0..wi).map(move |x| (x, y)))
        .map(|(x, y)| {
            let other = &edge[usize::from(!at(x, y))];
            let d2 = other
                .iter()
                .map(|&(qx, qy)| (qx - x).pow(2) + (qy - y).pow(2))
                .min()
                .unwrap_or(i64::MAX);
            (d2 as f64).sqrt()
        })
        .collect()
}

/// Rows that miss the gate, at their own measured `(max, mean)` (module
/// doc), rounded up in the third decimal. Every Drop Shadow row meets it.
/// - Cone - Inverted (16 identify rows): its table starts at 251, so the
///   whole interior away from the edge reads `C(0)`, where Photoshop reads
///   1.3 ... 1.7 codes lower on average (contour-557's `is_cone_inverted`
///   the same, 1.90 under IC1); max 4 ... 5.
/// - Cone, Half Round and Ring at Size 16 and Choke 40 % (7 identify rows)
///   and at Size 10 and Choke 30 % (11 confirm rows): the contour's steep
///   stretches lift the max to 3 ... 7 codes (the runner's RECORDED: IC1
///   leaves Ring 7 / 0.182 where today read 8 / 0.231); every mean within
///   0.31.
/// - The confirm take's Inner Shadow composite rows at Fill 100 % (8 rows,
///   IK, confirmed but not built in phase 1): the shadow composites over the
///   layer's composite where Photoshop composites it inside the layer, 26
///   ... 28 codes on the partial ring, every mean within 0.06.
const PINNED: &[(&str, f64, f64)] = &[
    ("ic_disc_cone_s16_c40_d7_8", 3.001, 0.205),
    ("ic_disc_coni_s16_c0_d0_8", 4.001, 1.309),
    ("ic_disc_coni_s16_c0_d7_8", 4.001, 1.248),
    ("ic_disc_coni_s16_c40_d0_8", 4.001, 1.368),
    ("ic_disc_coni_s16_c40_d7_8", 4.001, 1.373),
    ("ic_disc_coni_s6_c0_d0_8", 4.001, 1.465),
    ("ic_disc_coni_s6_c0_d7_8", 4.001, 1.666),
    ("ic_disc_coni_s6_c40_d0_8", 4.001, 1.322),
    ("ic_disc_coni_s6_c40_d7_8", 4.001, 1.630),
    ("ic_disc_ring_s16_c40_d0_8", 3.001, 0.280),
    ("ic_disc_ring_s16_c40_d7_8", 5.001, 0.310),
    ("ic_ell3_cone_s10_c30_d5_16", 3.829, 0.137),
    ("ic_ell3_cone_s10_c30_d5_8", 4.001, 0.124),
    ("ic_ell3_hr_s10_c30_d5_16", 3.355, 0.098),
    ("ic_ell3_hr_s10_c30_d5_8", 3.001, 0.084),
    ("ic_ell3_ring_s10_c30_d5_16", 7.086, 0.196),
    ("ic_ell3_ring_s10_c30_d5_8", 7.001, 0.186),
    ("ic_ell_cone_s16_c40_d7_8", 3.001, 0.160),
    ("ic_ell_coni_s16_c0_d0_8", 4.001, 1.339),
    ("ic_ell_coni_s16_c0_d7_8", 4.001, 1.524),
    ("ic_ell_coni_s16_c40_d0_8", 4.001, 1.331),
    ("ic_ell_coni_s16_c40_d7_8", 5.001, 1.598),
    ("ic_ell_coni_s6_c0_d0_8", 4.001, 1.393),
    ("ic_ell_coni_s6_c0_d7_8", 4.001, 1.657),
    ("ic_ell_coni_s6_c40_d0_8", 4.001, 1.411),
    ("ic_ell_coni_s6_c40_d7_8", 4.001, 1.648),
    ("ic_ell_hr_s16_c40_d7_8", 3.001, 0.096),
    ("ic_ell_ring_s16_c40_d0_8", 3.001, 0.162),
    ("ic_ell_ring_s16_c40_d7_8", 6.001, 0.225),
    ("ic_rr3_cone_s10_c30_d5_16", 2.565, 0.064),
    ("ic_rr3_cone_s10_c30_d5_8", 3.001, 0.039),
    ("ic_rr3_hr_s10_c30_d5_16", 2.366, 0.054),
    ("ic_rr3_ring_s10_c30_d5_16", 4.849, 0.089),
    ("ic_rr3_ring_s10_c30_d5_8", 5.001, 0.080),
    ("is_ell3_f100_m75_16", 26.440, 0.053),
    ("is_ell3_f100_m75_8", 27.001, 0.048),
    ("is_ell3_f100_n70_16", 27.892, 0.055),
    ("is_ell3_f100_n70_8", 28.001, 0.048),
    ("is_rr3_f100_m75_16", 26.156, 0.031),
    ("is_rr3_f100_m75_8", 27.001, 0.032),
    ("is_rr3_f100_n70_16", 27.623, 0.033),
    ("is_rr3_f100_n70_8", 28.001, 0.025),
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

fn values(v: &Value, name: &str) -> Vec<f64> {
    v.as_array()
        .unwrap_or_else(|| panic!("{name}: not an array"))
        .iter()
        .flat_map(|x| match x {
            Value::Array(rgb) => rgb.iter().map(|c| c.as_f64().expect("a number")).collect(),
            other => vec![other.as_f64().expect("a number")],
        })
        .collect()
}

/// Each identify picture's size, alpha and contour distances, built once.
type Pictures = Vec<(String, (u32, u32, Vec<f64>, Vec<f64>))>;

fn picture<'a>(pictures: &'a mut Pictures, pic: &str) -> &'a (u32, u32, Vec<f64>, Vec<f64>) {
    if !pictures.iter().any(|(k, _)| k == pic) {
        let (w, h, a) = p4_coverage(pic);
        let d = contour_distance(w, h, &a);
        pictures.push((pic.to_string(), (w, h, a, d)));
    }
    &pictures.iter().find(|(k, _)| k == pic).expect("picture").1
}

#[test]
fn the_identify_inner_and_drop_shadow_rows_meet_the_gate_or_their_pins() {
    let fixture = identify_fixture();
    let rows = fixture.as_object().expect("rows");
    assert_eq!(rows.len(), 104, "80 Inner Shadow and 24 Drop Shadow rows");
    let mut pictures = Pictures::new();
    let mut failed = Vec::new();
    for (name, row) in rows {
        assert_eq!(row["take"], "effect-kernels-578-p4/identify1", "{name}");
        assert_eq!(row["depth"], 8, "{name}");
        let shadow = row["family"] == "DK";
        let (pic, style, radius, source, backdrop) = if shadow {
            let (pic, style) = ds_case(name);
            let src = [24.0, 64.0, 148.0].map(|v| v / 255.0);
            let back = [196.0, 204.0, 92.0].map(|v| v / 255.0);
            (pic, style, f64::INFINITY, src, back)
        } else {
            assert_eq!(row["family"], "IC", "{name}");
            assert_eq!(row["channel"], 0, "{name}");
            let (pic, reach, style) = ic_case(name);
            (pic, style, 2.0 * reach + 2.0, [0.0; 3], [0.0; 3])
        };
        let (w, h, a, dist) = picture(&mut pictures, &pic);
        let ours = render_rgb_alpha(8, *w, *h, a, source, backdrop, &style);
        let (ps_row, ps_col) = (values(&row["row"], name), values(&row["col"], name));
        let per = if shadow { 3 } else { 1 };
        assert_eq!(
            (ps_row.len(), ps_col.len()),
            (*w as usize * per, *h as usize * per),
            "{name}"
        );
        let (mut o, mut t) = (Vec::new(), Vec::new());
        let samples = (0..*w)
            .map(|x| (x, 112u32, x as usize))
            .map(|(x, y, k)| (x, y, &ps_row, k))
            .chain((0..*h).map(|y| (128u32, y, &ps_col, y as usize)));
        for (x, y, ps, k) in samples {
            let i = (y * *w + x) as usize;
            if dist[i] > radius {
                continue;
            }
            for c in 0..per {
                o.push(ours[i][c]);
                t.push(ps[k * per + c]);
            }
        }
        failed.extend(check_row(name, error(&o, &t, name)));
    }
    assert_none_failed(&failed);
}

// ------------------------------------------------------ the confirm take --

fn take_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h).join("devel/doctored-fixtures/photoshop/effect-kernels-578-p4/take1")
    })
}

/// The unit RGB of `path`'s first pixel and the unit alpha of every pixel.
fn input(path: &Path) -> (u32, u32, [f64; 3], Vec<f64>) {
    let img = image::open(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .to_rgba16();
    let (w, h) = img.dimensions();
    let first = img.get_pixel(0, 0).0;
    let rgb = [0, 1, 2].map(|k| f64::from(first[k]) / 65535.0);
    let a = img.pixels().map(|p| f64::from(p.0[3]) / 65535.0).collect();
    (w, h, rgb, a)
}

type Fields = Vec<((String, u32), Vec<f64>)>;

/// One confirm row (module doc).
fn take_row(take: &Path, name: &str, row: &Value, fields: &mut Fields) -> (f64, f64) {
    let pic = row["picture"].as_str().expect("picture");
    let depth = row["depth"].as_u64().expect("depth") as u32;
    let family = row["family"].as_str().expect("family");
    let shadow = matches!(family, "DK" | "IK");
    let tag = if shadow {
        format!("{pic}_sh")
    } else {
        pic.to_string()
    };
    let pictures = take.join("inputs/pictures");
    let (w, h, backdrop, _) = input(&pictures.join(format!("{tag}_backdrop_{depth}.png")));
    let (_, _, source, a) = input(&pictures.join(format!("{tag}_source_{depth}.png")));
    let ours = render_rgb_alpha(depth, w, h, &a, source, backdrop, &row["style"]);
    let theirs = image::open(take.join(format!("rows/{name}.png")))
        .expect("the row's readback")
        .to_rgba16();
    assert_eq!(theirs.dimensions(), (w, h), "{name}: the readback's size");
    let (mut o, mut t) = (Vec::new(), Vec::new());
    if shadow {
        for (mine, ps) in ours.iter().zip(theirs.pixels()) {
            for (&m, &p) in mine.iter().zip(&ps.0[..3]) {
                o.push(m);
                t.push(f64::from(p) / 257.0);
            }
        }
        return error(&o, &t, name);
    }
    let channel = row["channel"].as_u64().expect("channel") as usize;
    let fx = &row["style"]["effects"][0];
    let radius =
        2.0 * (fx["size"].as_f64().expect("size") + fx["distance"].as_f64().expect("d")) + 2.0;
    let key = (pic.to_string(), depth);
    if !fields.iter().any(|(k, _)| *k == key) {
        fields.push((key.clone(), contour_distance(w, h, &a)));
    }
    let dist = &fields.iter().find(|(k, _)| *k == key).expect("field").1;
    for ((mine, ps), &d) in ours.iter().zip(theirs.pixels()).zip(dist) {
        if d <= radius {
            o.push(mine[channel]);
            t.push(f64::from(ps.0[channel]) / 257.0);
        }
    }
    error(&o, &t, name)
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
    let rows: Value =
        serde_json::from_str(&std::fs::read_to_string(&rows_path).expect("rows.json"))
            .expect("json");
    let rows = rows.as_object().expect("rows");
    assert_eq!(rows.len(), 64, "IC 12, DS 12 and IS 8 rows per depth");
    let mut fields = Fields::new();
    let mut failed = Vec::new();
    for (name, row) in rows {
        failed.extend(check_row(name, take_row(&take, name, row, &mut fields)));
    }
    assert_none_failed(&failed);
}
