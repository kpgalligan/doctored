//! Pattern Overlay Angle at 8 bits: the rotated render against an
//! oracle written here from the runner's model
//! (`scripts/photoshop-probe/pattern_overlay_take.py`, TAKE4's DECLARED4:
//! TWO-PASS — the PYRAMID level turned into its bounding raster, then that
//! raster cut along the exact turned lattice, each instance clamped, shift
//! (−1, 0), pivot O), angle 0 against the angle omitted, the JSON's
//! normalization and refusal, Photoshop's angle TIFFs (take1 to take4,
//! #3's TAKE5 `rot5` and TAKE6 `rot6` rows from
//! `~/devel/doctored-fixtures`, when present) within the residual the
//! runners recorded, and the byte `.rz` word 93 both ways.
//! Every style is applied through `dr_doc_set_layer_style`. Never a golden.

use std::path::PathBuf;

use doctored_core::doc::DrDocument;
use doctored_core::doc_comps::{CompCaptures, CompRequest};
use doctored_core::style::LayerStyle;
use image::RgbaImage;
use serde_json::Value;

use crate::common;
use common::*;

const CANVAS: (u32, u32) = (96, 64);
const RECT: (i32, i32, u32, u32) = (13, 9, 50, 40);
const GREEN: [u8; 4] = [0x30, 0xa0, 0x50, 255];

/// Standard base64 (RFC 4648, padded), written here so the test does not
/// borrow the core's encoder.
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A tile: `(w, h, straight RGBA8)`.
type Tile = (u32, u32, Vec<u8>);

fn tile_of(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Tile {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&f(x, y));
        }
    }
    (w, h, rgba)
}

/// A checker-and-ramp tile: (a x + b, c y + d, 200 | 60), opaque — the
/// shape of every take's tile.
fn ramp(w: u32, h: u32, a: u32, b: u32, c: u32, d: u32) -> Tile {
    tile_of(w, h, |x, y| {
        [
            (a * x + b) as u8,
            (c * y + d) as u8,
            if (x + y) % 2 == 0 { 200 } else { 60 },
            255,
        ]
    })
}

/// take1's T (12 x 8).
fn tile_t() -> Tile {
    ramp(12, 8, 20, 10, 30, 15)
}

/// take2's T2 (11 x 7).
fn tile_t2() -> Tile {
    ramp(11, 7, 22, 15, 35, 20)
}

/// take3's T3 (9 x 5), take4's odd tile at an angle.
fn tile_t3() -> Tile {
    ramp(9, 5, 25, 20, 45, 25)
}

/// T4 (13 x 10, the runner's `texel4`), TAKE5's tile at an angle.
fn tile_t4() -> Tile {
    ramp(13, 10, 18, 12, 22, 18)
}

/// TAKE6's T5 (7 x 11, odd both ways).
fn tile_t5() -> Tile {
    ramp(7, 11, 30, 15, 20, 20)
}

/// TAKE6's T6 (16 x 10).
fn tile_t6() -> Tile {
    ramp(16, 10, 14, 9, 23, 12)
}

fn custom(tile: &Tile, name: &str) -> String {
    format!(
        "{{\"name\":\"{name}\",\"width\":{},\"height\":{},\"rgba\":\"{}\"}}",
        tile.0,
        tile.1,
        base64(&tile.2)
    )
}

/// A style holding one pattern overlay with `pattern` and `extra` spliced in.
fn overlay(pattern: &str, extra: &str) -> String {
    let extra = if extra.is_empty() {
        String::new()
    } else {
        format!(",{extra}")
    };
    format!("{{\"effects\":[{{\"type\":\"pattern_overlay\",\"pattern\":{pattern}{extra}}}]}}")
}

fn rect_doc() -> DrDocument {
    rect_layer_doc(CANVAS, RECT, GREEN)
}

/// The f64 PYRAMID level (the runner's `pyramid`, ODD-DOUBLE): premultiplied
/// unit texels and the residual scale per axis.
struct Level {
    w: usize,
    h: usize,
    px: Vec<[f64; 4]>,
    sx: f64,
    sy: f64,
}

fn level_of(tile: &Tile, scale: f64) -> Level {
    let (w0, h0) = (tile.0 as usize, tile.1 as usize);
    let mut px: Vec<[f64; 4]> = tile
        .2
        .chunks(4)
        .map(|p| {
            let a = f64::from(p[3]) / 255.0;
            [
                f64::from(p[0]) / 255.0 * a,
                f64::from(p[1]) / 255.0 * a,
                f64::from(p[2]) / 255.0 * a,
                a,
            ]
        })
        .collect();
    let (mut w, mut h) = (w0, h0);
    let (mut pw, mut ph) = (w0 as f64, h0 as f64);
    let halvings = ((1.0 / scale).log2().floor()).max(0.0) as u32;
    let mean = |a: [f64; 4], b: [f64; 4]| -> [f64; 4] {
        [
            (a[0] + b[0]) / 2.0,
            (a[1] + b[1]) / 2.0,
            (a[2] + b[2]) / 2.0,
            (a[3] + b[3]) / 2.0,
        ]
    };
    for _ in 0..halvings {
        let nw = if w % 2 == 0 { w / 2 } else { w };
        if w % 2 == 1 {
            pw *= 2.0;
        }
        let mut next = vec![[0.0; 4]; nw * h];
        for y in 0..h {
            for j in 0..nw {
                next[y * nw + j] = mean(px[y * w + (2 * j) % w], px[y * w + (2 * j + 1) % w]);
            }
        }
        px = next;
        w = nw;
        let nh = if h % 2 == 0 { h / 2 } else { h };
        if h % 2 == 1 {
            ph *= 2.0;
        }
        let mut next = vec![[0.0; 4]; w * nh];
        for x in 0..w {
            for j in 0..nh {
                next[j * w + x] = mean(px[((2 * j) % h) * w + x], px[((2 * j + 1) % h) * w + x]);
            }
        }
        px = next;
        h = nh;
    }
    Level {
        w,
        h,
        px,
        sx: scale * pw / w as f64,
        sy: scale * ph / h as f64,
    }
}

/// The runner's fitted index shift (TAKE4's DECLARED4), level texels in
/// the tile's own frame.
const SHIFT: (f64, f64) = (-1.0, 0.0);

/// (cos t, sin t) of t = +`deg`, EXACT at a multiple of 90 degrees (the
/// runner's `rotation4`).
fn rot(deg: f64) -> (f64, f64) {
    if deg % 90.0 == 0.0 {
        return [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)]
            [((deg / 90.0).round() as i64).rem_euclid(4) as usize];
    }
    let t = deg.to_radians();
    (t.cos(), t.sin())
}

/// The premultiplied bilinear sum of a `w` x `h` raster `at(i, j)` at (u, v),
/// texel j at index j, the taps clamped inside it.
fn clamped(w: usize, h: usize, at: impl Fn(usize, usize) -> [f64; 4], u: f64, v: f64) -> [f64; 4] {
    let (i0, j0) = (u.floor(), v.floor());
    let (fu, fv) = (u - i0, v - j0);
    let mut acc = [0.0; 4];
    for (di, dj, k) in [
        (0.0, 0.0, (1.0 - fu) * (1.0 - fv)),
        (1.0, 0.0, fu * (1.0 - fv)),
        (0.0, 1.0, (1.0 - fu) * fv),
        (1.0, 1.0, fu * fv),
    ] {
        let i = ((i0 + di) as i64).clamp(0, w as i64 - 1) as usize;
        let j = ((j0 + dj) as i64).clamp(0, h as i64 - 1) as usize;
        for (slot, v) in acc.iter_mut().zip(at(i, j)) {
            *slot += k * v;
        }
    }
    acc
}

/// Straight unit RGBA at canvas pixel (x, y): the runner's TWO-PASS with
/// the tile's canvas origin `o` and Photoshop's angle `deg`. Pass 1: raster
/// texel q (integers over R(−t) of [0, w] x [0, h]) is the level at R(t) q,
/// clamped bilinear. Pass 2: d = (x, y) − o falls in the lattice instance
/// of v1 = (c w s'x, −s w s'x), v2 = (s h s'y, c h s'y) at the local point
/// l, which reads the raster at l / s' + R(−t) SHIFT, clamped bilinear.
fn sample(l: &Level, o: (f64, f64), deg: f64, x: f64, y: f64) -> [f64; 4] {
    let (c, s) = rot(deg);
    let (w, h) = (l.w as f64, l.h as f64);
    let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)];
    let xs = corners.map(|(u, v)| c * u + s * v);
    let ys = corners.map(|(u, v)| -s * u + c * v);
    let x0 = xs.iter().copied().fold(f64::INFINITY, f64::min).floor();
    let y0 = ys.iter().copied().fold(f64::INFINITY, f64::min).floor();
    let rw = (xs.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil() - x0) as usize + 1;
    let rh = (ys.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil() - y0) as usize + 1;
    let raster = |i: usize, j: usize| {
        let (qx, qy) = (x0 + i as f64, y0 + j as f64);
        clamped(
            l.w,
            l.h,
            |a, b| l.px[b * l.w + a],
            c * qx - s * qy,
            s * qx + c * qy,
        )
    };
    let v1 = (c * w * l.sx, -s * w * l.sx);
    let v2 = (s * h * l.sy, c * h * l.sy);
    let det = v1.0 * v2.1 - v1.1 * v2.0;
    let (dx, dy) = (x - o.0, y - o.1);
    let a = ((dx * v2.1 - dy * v2.0) / det).floor();
    let b = ((v1.0 * dy - v1.1 * dx) / det).floor();
    let (lx, ly) = (dx - a * v1.0 - b * v2.0, dy - a * v1.1 - b * v2.1);
    let qx = lx / l.sx + c * SHIFT.0 + s * SHIFT.1 - x0;
    let qy = ly / l.sy - s * SHIFT.0 + c * SHIFT.1 - y0;
    let acc = clamped(rw, rh, raster, qx, qy);
    if acc[3] <= 0.0 {
        return [0.0; 4];
    }
    [acc[0] / acc[3], acc[1] / acc[3], acc[2] / acc[3], acc[3]]
}

/// The Normal composite of the sample over an opaque `under`, the coverage
/// quantized as the renderer stores it.
fn over(s: [f64; 4], under: [u8; 4]) -> [u8; 4] {
    let a = (s[3] * 255.0).round() / 255.0;
    let mut out = [255u8; 4];
    for c in 0..3 {
        out[c] = (s[c] * 255.0 * a + f64::from(under[c]) * (1.0 - a)).round() as u8;
    }
    out
}

fn inside(rect: (i32, i32, u32, u32), x: u32, y: u32) -> bool {
    let (x, y) = (x as i32, y as i32);
    x >= rect.0 && y >= rect.1 && x < rect.0 + rect.2 as i32 && y < rect.1 + rect.3 as i32
}

/// Asserts the flattened `doc`: inside `rect` the oracle, outside white;
/// ±1 colour, alpha exact.
fn assert_rotated(
    doc: &DrDocument,
    rect: (i32, i32, u32, u32),
    l: &Level,
    o: (f64, f64),
    deg: f64,
    what: &str,
) {
    let flat = doc.flattened().unwrap();
    for y in 0..doc.height {
        for x in 0..doc.width {
            let want = if inside(rect, x, y) {
                over(sample(l, o, deg, f64::from(x), f64::from(y)), GREEN)
            } else {
                WHITE
            };
            assert_close(&flat.get_pixel(x, y).0, &want, &format!("{what} ({x},{y})"));
        }
    }
}

fn effect0(doc: &DrDocument, idx: usize) -> Value {
    let text = ffi_style(doc as *const DrDocument, idx).expect("a style");
    serde_json::from_str::<Value>(&text).unwrap()["effects"][0].clone()
}

// ----------------------------------------------------------------- render --

#[test]
fn the_rotated_tile_matches_the_oracle() {
    let t = tile_t();
    for (deg, scale) in [
        (30.0, 1.0),
        (-30.0, 1.0),
        (45.0, 0.5),
        (90.0, 1.0),
        (-135.0, 0.37),
        (170.0, 2.0),
        (20.0, 0.25),
        (180.0, 1.0),
        (-90.0, 1.0),
    ] {
        let json = overlay(
            &custom(&t, "T"),
            &format!("\"angle\":{deg},\"scale\":{scale}"),
        );
        let doc = styled(&rect_doc(), 1, &json);
        assert_rotated(
            &doc,
            RECT,
            &level_of(&t, scale),
            (0.0, 0.0),
            deg,
            &format!("angle {deg} scale {scale}"),
        );
    }
}

#[test]
fn the_tile_turns_about_its_origin() {
    // Unlinked with a phase: O = phase. Linked and moved after the style:
    // O follows the move (FOLLOW).
    let t = tile_t();
    let unlinked = styled(
        &rect_doc(),
        1,
        &overlay(
            &custom(&t, "T"),
            "\"angle\":30,\"scale\":0.5,\"link_with_layer\":false,\"phase\":[5,2]",
        ),
    );
    assert_rotated(
        &unlinked,
        RECT,
        &level_of(&t, 0.5),
        (5.0, 2.0),
        30.0,
        "unlinked, phase (5, 2)",
    );
    let linked = styled(
        &rect_doc(),
        1,
        &overlay(&custom(&t, "T"), "\"angle\":30,\"scale\":0.5"),
    );
    let moved = linked
        .with_layer_offset(1, RECT.0 + 7, RECT.1 + 3)
        .expect("the move");
    assert_rotated(
        &moved,
        (RECT.0 + 7, RECT.1 + 3, RECT.2, RECT.3),
        &level_of(&t, 0.5),
        (7.0, 3.0),
        30.0,
        "linked, moved (+7, +3)",
    );
}

#[test]
fn angle_zero_is_the_angle_omitted() {
    let t = tile_t();
    for extra in ["\"scale\":0.37", "\"scale\":2,\"phase\":[5,2]"] {
        let omitted = styled(&rect_doc(), 1, &overlay(&custom(&t, "T"), extra));
        for zero in ["\"angle\":0", "\"angle\":360", "\"angle\":-0.0"] {
            let doc = styled(
                &rect_doc(),
                1,
                &overlay(&custom(&t, "T"), &format!("{zero},{extra}")),
            );
            assert_eq!(
                doc.flattened().unwrap(),
                omitted.flattened().unwrap(),
                "{zero} {extra}: bit-identical"
            );
            assert_eq!(effect0(&doc, 1), effect0(&omitted, 1), "{zero} {extra}");
        }
    }
}

// ------------------------------------------------------------------- JSON --

#[test]
fn the_angle_is_normalized_and_always_written() {
    let omitted = parse(&overlay("{\"preset\":\"dots\"}", ""));
    let json: Value = serde_json::from_str(&omitted.to_json()).unwrap();
    assert_eq!(json["effects"][0]["angle"], 0.0, "written at its default");
    for (given, want) in [
        ("30.5", 30.5),
        ("-45", -45.0),
        ("190", -170.0),
        ("180", -180.0),
        ("-180", -180.0),
        ("540", -180.0),
        ("-725", -5.0),
    ] {
        let style = parse(&overlay(
            "{\"preset\":\"dots\"}",
            &format!("\"angle\":{given}"),
        ));
        let json: Value = serde_json::from_str(&style.to_json()).unwrap();
        assert_eq!(json["effects"][0]["angle"], want, "angle {given}");
        assert_eq!(
            parse(&style.to_json()),
            style,
            "angle {given} re-parses equal"
        );
    }
    let doc = rect_doc();
    for bad in ["\"x\"", "null", "true", "[30]"] {
        let got = style_error(
            &doc as *const DrDocument,
            1,
            &overlay("{\"preset\":\"dots\"}", &format!("\"angle\":{bad}")),
        );
        assert_eq!(
            got.as_deref(),
            Some("style: effects[0].angle must be a number"),
            "angle {bad}"
        );
    }
}

// -------------------------------------------------------- Photoshop's takes --

/// The takes, each with the number of its rows below: take1 to take4
/// (`pattern-overlay-557`), TAKE5 (the `rot5` job of `rotation-667-669`'s
/// take1) and TAKE6 (`pattern-overlay-557/take6`'s `rot6` job).
const TAKES: [(&str, usize); 6] = [
    ("take1", 2),
    ("take2", 4),
    ("take3", 10),
    ("take4", 10),
    ("take5", 10),
    ("take6", 10),
];

/// The job folder (RGB8) of the take `key` names.
fn take_folder(key: &str) -> Option<PathBuf> {
    let root: PathBuf = std::env::var_os("HOME")?.into();
    let photoshop = root.join("devel/doctored-fixtures/photoshop");
    Some(match key {
        "take5" => photoshop.join("rotation-667-669/take1/jobs/rot5"),
        "take6" => photoshop.join("pattern-overlay-557/take6/jobs/rot6"),
        _ => photoshop.join(format!("pattern-overlay-557/{key}/jobs/rgb8")),
    })
}

/// The take's TIFF of `case` (RGB8) when it exists.
fn take_tiff(key: &str, case: &str) -> Option<RgbaImage> {
    let path = take_folder(key)?.join(format!("{case}.tif"));
    let doc = DrDocument::open(path.to_str()?).ok()?;
    doc.flattened().ok()
}

/// One angle row: its take, case, tile, angle, scale, placement, and the
/// residual the runner recorded for the shipped model (max, mean codes).
struct Row {
    take: &'static str,
    case: &'static str,
    tile: Tile,
    angle: f64,
    scale: f64,
    /// (link, phase, move after the style, canvas).
    place: (bool, [i32; 2], (i32, i32), (u32, u32)),
    residual: (f64, f64),
}

fn row(
    take: &'static str,
    case: &'static str,
    angle: f64,
    scale: f64,
    residual: (f64, f64),
) -> Row {
    Row {
        take,
        case,
        tile: tile_t(),
        angle,
        scale,
        place: (true, [0, 0], (0, 0), CANVAS),
        residual,
    }
}

/// The rows and the residuals the runner records for the shipped model
/// (each row's own max and mean, in codes).
fn rows() -> Vec<Row> {
    let mut out = vec![
        row("take1", "a30_rgb8", 30.0, 1.0, RESIDUAL_TAKE1[0]),
        row("take1", "a90_rgb8", 90.0, 1.0, RESIDUAL_TAKE1[1]),
        row("take2", "a15_rgb8", 15.0, 0.5, RESIDUAL_TAKE2[0]),
        row("take2", "a45_rgb8", 45.0, 0.5, RESIDUAL_TAKE2[1]),
        row("take2", "a90_rgb8", 90.0, 0.5, RESIDUAL_TAKE2[2]),
        row("take2", "a30_rgb8", 30.0, 0.5, RESIDUAL_TAKE2[3]),
        row("take3", "a20_rgb8", 20.0, 1.0, RESIDUAL_TAKE3[0]),
        row("take3", "a60_rgb8", 60.0, 0.37, RESIDUAL_TAKE3[1]),
        row("take3", "an45_rgb8", -45.0, 1.0, RESIDUAL_TAKE3[2]),
        row("take3", "a135_rgb8", 135.0, 0.7, RESIDUAL_TAKE3[3]),
        row("take3", "a30s150_rgb8", 30.0, 1.5, RESIDUAL_TAKE3[4]),
        row("take3", "a170_rgb8", 170.0, 0.25, RESIDUAL_TAKE3[5]),
    ];
    let mut moved = row("take3", "a30p2_rgb8", 30.0, 0.5, RESIDUAL_TAKE3[6]);
    moved.place.2 = (7, 3);
    let mut unlinked = row("take3", "a30u_rgb8", 30.0, 0.5, RESIDUAL_TAKE3[7]);
    unlinked.place.0 = false;
    unlinked.place.1 = [5, 2];
    let mut wide = row("take3", "a30w_rgb8", 30.0, 0.5, RESIDUAL_TAKE3[8]);
    wide.place.3 = (112, 80);
    let mut t2 = row("take3", "a25t2_rgb8", 25.0, 0.5, RESIDUAL_TAKE3[9]);
    t2.tile = tile_t2();
    out.extend([moved, unlinked, wide, t2]);
    for (k, (case, angle, scale)) in [
        ("a10_rgb8", 10.0, 1.0),
        ("a10h_rgb8", 10.0, 0.45),
        ("a65_rgb8", 65.0, 1.0),
        ("a65h_rgb8", 65.0, 0.45),
        ("a120_rgb8", 120.0, 1.0),
        ("a120h_rgb8", 120.0, 0.45),
        ("an20_rgb8", -20.0, 1.0),
        ("an20h_rgb8", -20.0, 0.45),
        ("t3a40_rgb8", 40.0, 1.0),
        ("t3a40h_rgb8", 40.0, 0.45),
    ]
    .into_iter()
    .enumerate()
    {
        let mut r = row("take4", case, angle, scale, RESIDUAL_TAKE4[k]);
        if case.starts_with("t3") {
            r.tile = tile_t3();
        }
        out.push(r);
    }
    for (k, (case, angle, scale)) in [
        ("a5_rgb8", 5.0, 1.0),
        ("a5h_rgb8", 5.0, 0.45),
        ("a50_rgb8", 50.0, 1.0),
        ("a50h_rgb8", 50.0, 0.45),
        ("a150_rgb8", 150.0, 1.0),
        ("a150h_rgb8", 150.0, 0.45),
        ("an35_rgb8", -35.0, 1.0),
        ("an35h_rgb8", -35.0, 0.45),
        ("t4a25_rgb8", 25.0, 1.0),
        ("t4a25h_rgb8", 25.0, 0.45),
    ]
    .into_iter()
    .enumerate()
    {
        let mut r = row("take5", case, angle, scale, RESIDUAL_TAKE5[k]);
        if case.starts_with("t4") {
            r.tile = tile_t4();
        }
        out.push(r);
    }
    for (k, (case, angle, scale)) in [
        ("t6a35_rgb8", 35.0, 1.0),
        ("t6a35m_rgb8", 35.0, 0.6),
        ("t6a80_rgb8", 80.0, 1.0),
        ("t6a80m_rgb8", 80.0, 0.6),
        ("t6a145_rgb8", 145.0, 1.0),
        ("t6a145m_rgb8", 145.0, 0.6),
        ("t6an50_rgb8", -50.0, 1.0),
        ("t6an50m_rgb8", -50.0, 0.6),
        ("t5a110_rgb8", 110.0, 1.0),
        ("t5a110m_rgb8", 110.0, 0.6),
    ]
    .into_iter()
    .enumerate()
    {
        let mut r = row("take6", case, angle, scale, RESIDUAL_TAKE6[k]);
        r.tile = if case.starts_with("t5") {
            tile_t5()
        } else {
            tile_t6()
        };
        r.place.3 = (112, 80);
        out.push(r);
    }
    out
}

/// The recorded residuals of the shipped model (runner docstring, TAKE4's
/// recorded fit: DECLARED4, TWO-PASS): take1's A30 and A90; take2's A15,
/// A45, A90 and A30; take3's ten RGB8 rotation rows. Photoshop's rotation
/// is not this model (only the A90s are within the declared bound); these
/// pin what ships.
const RESIDUAL_TAKE1: [(f64, f64); 2] = [(126.349, 51.39), (0.0, 0.0)];
const RESIDUAL_TAKE2: [(f64, f64); 4] = [
    (54.669, 23.95),
    (98.411, 51.946),
    (0.0, 0.0),
    (87.821, 38.113),
];
const RESIDUAL_TAKE3: [(f64, f64); 10] = [
    (137.166, 49.21),
    (155.126, 63.024),
    (136.407, 55.954),
    (123.259, 34.632),
    (100.0, 21.71),
    (136.197, 81.179),
    (87.821, 38.113),
    (86.385, 38.948),
    (87.821, 38.113),
    (120.44, 39.908),
];
/// take4's ten held-out rows (A10, A10H, A65, A65H, A120, A120H, AN20,
/// AN20H, T3A40, T3A40H) under the same model, as the runner's take4 judge
/// recorded them: the confirming take MISSES the bound on every row.
const RESIDUAL_TAKE4: [(f64, f64); 10] = [
    (128.925, 48.423),
    (85.554, 42.599),
    (108.145, 43.944),
    (64.427, 22.293),
    (110.111, 51.11),
    (63.133, 21.467),
    (145.438, 47.413),
    (135.409, 74.318),
    (132.205, 54.692),
    (144.84, 63.338),
];
/// TAKE5's ten held-out rows (A5, A5H, A50, A50H, A150, A150H, AN35, AN35H,
/// T4A25, T4A25H; #3's `rot5` job) under the same model, as the runner's
/// TAKE5 judge recorded them (`judged5.json`'s `rows4`).
const RESIDUAL_TAKE5: [(f64, f64); 10] = [
    (132.485, 34.711),
    (89.277, 45.625),
    (134.491, 53.881),
    (146.241, 50.057),
    (146.209, 50.724),
    (87.781, 48.112),
    (119.141, 51.286),
    (127.026, 70.53),
    (137.0, 48.448),
    (153.397, 75.329),
];
/// TAKE6's ten confirm rows on the 112 x 80 canvas (T6A35, T6A35M, T6A80,
/// T6A80M, T6A145, T6A145M, T6AN50, T6AN50M, T5A110, T5A110M;
/// `pattern_overlay_669_take.py`) under the same model, as its judge
/// recorded them (`judged6.json`'s `rows4`): RECORD, no row within the bound.
const RESIDUAL_TAKE6: [(f64, f64); 10] = [
    (132.393, 47.71),
    (74.485, 33.104),
    (98.121, 32.57),
    (63.399, 22.235),
    (139.0, 58.174),
    (65.0, 24.583),
    (144.62, 57.154),
    (124.048, 44.275),
    (114.225, 44.371),
    (67.005, 19.669),
];

/// The slack over a recorded residual: the runner scores the f64 model, the
/// render is quantized to 8-bit codes (half a code) — and a code more for
/// the coverage's own quantization.
const SLACK_MAX: f64 = 2.0;
const SLACK_MEAN: f64 = 0.5;

#[test]
fn photoshops_angles_match_within_the_recorded_residual() {
    let mut checked = [0usize; TAKES.len()];
    let mut present = [false; TAKES.len()];
    for r in rows() {
        let at = TAKES
            .iter()
            .position(|(key, _)| *key == r.take)
            .expect("a known take");
        present[at] = take_folder(r.take).is_some_and(|f| f.is_dir());
        let Some(ps) = take_tiff(r.take, r.case) else {
            continue;
        };
        let (link, phase, mv, canvas) = r.place;
        let doc = rect_layer_doc(canvas, RECT, GREEN);
        let doc = styled(
            &doc,
            1,
            &overlay(
                &custom(&r.tile, "T"),
                &format!(
                    "\"angle\":{},\"scale\":{},\"link_with_layer\":{link},\"phase\":[{},{}]",
                    r.angle, r.scale, phase[0], phase[1]
                ),
            ),
        );
        let doc = if mv == (0, 0) {
            doc
        } else {
            doc.with_layer_offset(1, RECT.0 + mv.0, RECT.1 + mv.1)
                .expect("the move")
        };
        let ours = doc.flattened().unwrap();
        let (mut worst, mut total, mut n) = (0f64, 0f64, 0f64);
        for y in 0..RECT.3 as i32 {
            for x in 0..RECT.2 as i32 {
                let (px, py) = ((RECT.0 + mv.0 + x) as u32, (RECT.1 + mv.1 + y) as u32);
                let (a, b) = (ours.get_pixel(px, py).0, ps.get_pixel(px, py).0);
                let e = (0..3)
                    .map(|c| f64::from((i32::from(a[c]) - i32::from(b[c])).abs()))
                    .fold(0.0, f64::max);
                worst = worst.max(e);
                total += e;
                n += 1.0;
            }
        }
        let mean = total / n;
        assert!(
            worst <= r.residual.0 + SLACK_MAX && mean <= r.residual.1 + SLACK_MEAN,
            "{} {}: max {worst} mean {mean:.3} against the recorded {:?}",
            r.take,
            r.case,
            r.residual
        );
        checked[at] += 1;
    }
    for (at, (take, want)) in TAKES.into_iter().enumerate() {
        if present[at] {
            assert_eq!(checked[at], want, "every {take} row present is compared");
        } else {
            eprintln!("SKIP: {take}'s angle TIFFs are absent on this machine");
        }
    }
}

// ------------------------------------------------------------------ .rz 93 --

fn angled_doc() -> DrDocument {
    styled(
        &rect_doc(),
        1,
        &overlay(&custom(&tile_t(), "T"), "\"angle\":30,\"scale\":0.5"),
    )
}

#[test]
fn the_byte_word_93_round_trips_and_refuses_both_ways() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = angled_doc();
    let path = dir.path().join("a.rz");
    doc.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
    assert_eq!(
        &bytes[8..12],
        &93u32.to_le_bytes(),
        "the FIRST word inside 45"
    );
    assert_eq!(&bytes[12..16], &90u32.to_le_bytes(), "93 implies 90");
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        reopened.layers[1].style.as_deref().map(LayerStyle::to_json),
        doc.layers[1].style.as_deref().map(LayerStyle::to_json)
    );
    assert_eq!(reopened.flattened().unwrap(), doc.flattened().unwrap());
    // The angle without the word.
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = DrDocument::open(cut.to_str().unwrap()) else {
        panic!("an angle without the word must refuse");
    };
    assert!(
        message.ends_with("native pattern overlay angles require DRDC 93"),
        "{message}"
    );
    // The word over an unangled pattern overlay.
    let plain_path = dir.path().join("plain.rz");
    styled(
        &rect_doc(),
        1,
        &overlay(&custom(&tile_t(), "T"), "\"scale\":0.5"),
    )
    .save_native(plain_path.to_str().unwrap())
    .unwrap();
    let mut plain = std::fs::read(&plain_path).unwrap();
    assert_eq!(&plain[8..12], &90u32.to_le_bytes(), "no 93 at angle 0");
    plain.splice(8..8, 93u32.to_le_bytes());
    let forged = dir.path().join("forged.rz");
    std::fs::write(&forged, &plain).unwrap();
    let Err(message) = DrDocument::open(forged.to_str().unwrap()) else {
        panic!("the word without an angle must refuse");
    };
    assert!(
        message.ends_with("invalid native pattern overlay angle word"),
        "{message}"
    );
}

#[test]
fn angle_zero_writes_no_word_and_the_style_carries_angle_zero() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = styled(&rect_doc(), 1, &overlay("{\"preset\":\"grid\"}", ""));
    let path = dir.path().join("z.rz");
    doc.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[8..12], &90u32.to_le_bytes());
    assert!(
        !bytes.windows(4).take(64).any(|w| w == 93u32.to_le_bytes()),
        "no 93 among the leading words"
    );
    assert!(
        bytes.windows(11).any(|w| w == b"\"angle\":0.0"),
        "the stored style carries its angle"
    );
}

#[test]
fn a_comp_only_angle_claims_the_word() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = angled_doc();
    let save = CompRequest::Save {
        name: "Angled".into(),
        comment: String::new(),
        captures: CompCaptures::default(),
    };
    let saved = doc.edit_layer_comps(&save).unwrap().unwrap();
    let cleared = try_styled(&saved, 1, None).unwrap().unwrap();
    assert!(cleared.layers[1].style.is_none());
    let path = dir.path().join("comp.rz");
    cleared.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[8..12], &93u32.to_le_bytes());
    assert_eq!(&bytes[12..16], &90u32.to_le_bytes());
    assert!(DrDocument::open(path.to_str().unwrap()).is_ok());
}
