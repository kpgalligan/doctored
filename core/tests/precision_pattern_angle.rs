//! Pattern Overlay Angle at 16 bits, at 32 bits and in CMYK ink. The
//! oracle is written here from the runner's model
//! (`scripts/photoshop-probe/pattern_overlay_take.py`, TAKE4's DECLARED4:
//! TWO-PASS — the PYRAMID level turned into its bounding raster, then that
//! raster cut along the exact turned lattice, each instance clamped, shift
//! (−1, 0), pivot O): the 16-bit render within one
//! 8-bit code of it, the 32-bit render's linear values (VERBATIM: code / 255)
//! within one code, and a CMYK layer's inks against the Colour Overlay of the
//! texels the oracle mixes (an independently shipped effect). The typed `.rz`
//! claims word 93 right after 46, or right after a CMYK file's rung, both
//! ways.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::dr_precision_doc_save_psd;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_psd_import::open_psd;
use crate::cmyk_structure_fixture::free;
use crate::cmyk_style_fixture::{backdrop, flat, hard_rect, layer, styled};
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (96, 64);
const RECT: (i32, i32, u32, u32) = (13, 9, 50, 40);
const GREEN: [u8; 4] = [0x30, 0xa0, 0x50, 255];
/// The runner's fitted index shift (TAKE4's DECLARED4), level texels in the
/// tile's own frame.
const SHIFT: (f64, f64) = (-1.0, 0.0);

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
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// take1's tile T: 12 x 8, opaque, (20x + 10, 30y + 15, 200 | 60).
fn tile_t() -> (u32, u32, Vec<u8>) {
    let mut rgba = Vec::new();
    for y in 0..8u32 {
        for x in 0..12u32 {
            rgba.extend_from_slice(&[
                (20 * x + 10) as u8,
                (30 * y + 15) as u8,
                if (x + y).is_multiple_of(2) { 200 } else { 60 },
                255,
            ]);
        }
    }
    (12, 8, rgba)
}

fn style(tile: &(u32, u32, Vec<u8>), extra: Value) -> Value {
    let mut effect = json!({"type":"pattern_overlay",
        "pattern":{"name":"T","width":tile.0,"height":tile.1,"rgba":base64(&tile.2)}});
    for (k, v) in extra.as_object().unwrap() {
        effect[k] = v.clone();
    }
    json!({"effects":[effect]})
}

/// The PYRAMID level of an opaque tile (straight unit RGB per texel; the
/// tiles here are opaque, so the premultiplied mean is the plain one) and
/// its residual scale.
struct Level {
    w: usize,
    h: usize,
    px: Vec<[f64; 3]>,
    sx: f64,
    sy: f64,
}

fn level_of(tile: &(u32, u32, Vec<u8>), scale: f64) -> Level {
    let (mut w, mut h) = (tile.0 as usize, tile.1 as usize);
    let mut px: Vec<[f64; 3]> = tile
        .2
        .chunks(4)
        .map(|p| [0, 1, 2].map(|c| f64::from(p[c]) / 255.0))
        .collect();
    let (mut pw, mut ph) = (w as f64, h as f64);
    let halvings = ((1.0 / scale).log2().floor()).max(0.0) as u32;
    let mean = |a: [f64; 3], b: [f64; 3]| [0, 1, 2].map(|c| (a[c] + b[c]) / 2.0);
    for _ in 0..halvings {
        let nw = if w % 2 == 0 { w / 2 } else { w };
        if w % 2 == 1 {
            pw *= 2.0;
        }
        let mut next = vec![[0.0; 3]; nw * h];
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
        let mut next = vec![[0.0; 3]; w * nh];
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

/// The four clamped bilinear taps of a `w` x `h` grid at (u, v): (index,
/// weight), texel j at index j.
fn bilinear(w: usize, h: usize, u: f64, v: f64) -> [((usize, usize), f64); 4] {
    let (i0, j0) = (u.floor(), v.floor());
    let (fu, fv) = (u - i0, v - j0);
    let at = |di: f64, dj: f64| {
        (
            ((i0 + di) as i64).clamp(0, w as i64 - 1) as usize,
            ((j0 + dj) as i64).clamp(0, h as i64 - 1) as usize,
        )
    };
    [
        (at(0.0, 0.0), (1.0 - fu) * (1.0 - fv)),
        (at(1.0, 0.0), fu * (1.0 - fv)),
        (at(0.0, 1.0), (1.0 - fu) * fv),
        (at(1.0, 1.0), fu * fv),
    ]
}

/// The (level texel, weight) taps the model reads at canvas pixel (x, y)
/// with the tile's canvas origin `o` and Photoshop's angle `deg`: TWO-PASS,
/// the four raster taps of pass 2 each expanded into the four level taps of
/// pass 1 that made that raster texel.
fn taps(l: &Level, o: (f64, f64), deg: f64, x: f64, y: f64) -> Vec<((usize, usize), f64)> {
    let (c, s) = rot(deg);
    let (w, h) = (l.w as f64, l.h as f64);
    let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)];
    let xs = corners.map(|(u, v)| c * u + s * v);
    let ys = corners.map(|(u, v)| -s * u + c * v);
    let x0 = xs.iter().copied().fold(f64::INFINITY, f64::min).floor();
    let y0 = ys.iter().copied().fold(f64::INFINITY, f64::min).floor();
    let rw = (xs.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil() - x0) as usize + 1;
    let rh = (ys.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil() - y0) as usize + 1;
    let v1 = (c * w * l.sx, -s * w * l.sx);
    let v2 = (s * h * l.sy, c * h * l.sy);
    let det = v1.0 * v2.1 - v1.1 * v2.0;
    let (dx, dy) = (x - o.0, y - o.1);
    let a = ((dx * v2.1 - dy * v2.0) / det).floor();
    let b = ((v1.0 * dy - v1.1 * dx) / det).floor();
    let (lx, ly) = (dx - a * v1.0 - b * v2.0, dy - a * v1.1 - b * v2.1);
    let qx = lx / l.sx + c * SHIFT.0 + s * SHIFT.1 - x0;
    let qy = ly / l.sy - s * SHIFT.0 + c * SHIFT.1 - y0;
    let mut out = Vec::new();
    for ((i, j), k) in bilinear(rw, rh, qx, qy) {
        let (rx, ry) = (x0 + i as f64, y0 + j as f64);
        for (texel, k1) in bilinear(l.w, l.h, c * rx - s * ry, s * rx + c * ry) {
            out.push((texel, k * k1));
        }
    }
    out
}

/// The model's straight unit RGB at canvas pixel (x, y).
fn sample(l: &Level, o: (f64, f64), deg: f64, x: f64, y: f64) -> [f64; 3] {
    let mut out = [0.0; 3];
    for ((i, j), k) in taps(l, o, deg, x, y) {
        for (slot, v) in out.iter_mut().zip(l.px[j * l.w + i]) {
            *slot += k * v;
        }
    }
    out
}

fn image16(w: u32, h: u32, px: [u8; 4]) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        w,
        h,
        vec![px.map(|v| u16::from(v) * 257); (w * h) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

/// A white canvas under the green rect made at RECT, at 16 bits.
fn typed_doc() -> PrecisionDocument {
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster(
                "Base",
                image16(CANVAS.0, CANVAS.1, [255; 4]),
                (0, 0),
                1.0,
                true,
            )
            .unwrap(),
            Layer::raster(
                "Rect",
                image16(RECT.2, RECT.3, GREEN),
                (RECT.0, RECT.1),
                1.0,
                true,
            )
            .unwrap(),
        ],
    )
    .unwrap()
}

fn set_style(doc: &PrecisionDocument, style: &Value) -> Result<PrecisionDocument, String> {
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(*unsafe { Box::from_raw(out) })
}

fn inside(x: u32, y: u32) -> bool {
    let (x, y) = (x as i32, y as i32);
    x >= RECT.0 && y >= RECT.1 && x < RECT.0 + RECT.2 as i32 && y < RECT.1 + RECT.3 as i32
}

const CASES: [(f64, f64); 4] = [(30.0, 1.0), (-45.0, 0.5), (135.0, 0.37), (20.0, 2.0)];

#[test]
fn sixteen_bits_render_the_oracle_within_one_code() {
    let t = tile_t();
    for (deg, scale) in CASES {
        let doc = set_style(&typed_doc(), &style(&t, json!({"angle":deg,"scale":scale}))).unwrap();
        let pixels = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
        let l = level_of(&t, scale);
        let mut worst = 0f64;
        for y in 0..CANVAS.1 {
            for x in 0..CANVAS.0 {
                let got = pixels[(y * CANVAS.0 + x) as usize];
                let want = if inside(x, y) {
                    sample(&l, (0.0, 0.0), deg, f64::from(x), f64::from(y)).map(|v| v * 255.0)
                } else {
                    [255.0; 3]
                };
                for c in 0..3 {
                    worst = worst.max((f64::from(got[c]) / 257.0 - want[c]).abs());
                }
            }
        }
        assert!(worst <= 1.0, "angle {deg} scale {scale}: {worst} codes");
    }
}

/// The 16-bit document promoted to 32-bit float.
fn float_doc() -> PrecisionDocument {
    let float = typed_doc().promote_to_float(LIMIT, 0, None).unwrap();
    assert!(float.is_float());
    float
}

#[test]
fn thirty_two_bits_read_the_rotated_codes_as_linear_values() {
    let t = tile_t();
    for (deg, scale) in CASES {
        let doc = set_style(&float_doc(), &style(&t, json!({"angle":deg,"scale":scale}))).unwrap();
        let pixels = doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap();
        let l = level_of(&t, scale);
        let mut worst = 0f64;
        for y in 0..CANVAS.1 {
            for x in 0..CANVAS.0 {
                if !inside(x, y) {
                    continue;
                }
                let got = pixels[(y * CANVAS.0 + x) as usize];
                let want = sample(&l, (0.0, 0.0), deg, f64::from(x), f64::from(y));
                for c in 0..3 {
                    worst = worst.max((f64::from(got[c]) - want[c]).abs() * 255.0);
                }
            }
        }
        assert!(worst <= 1.0, "angle {deg} scale {scale}: {worst} codes");
    }
}

// ------------------------------------------------------------------- CMYK --

const W: u32 = 12;
const H: u32 = 6;

/// The 4 × 3 probe tile: twelve distinct colours.
fn probe(x: u32, y: u32) -> [u8; 3] {
    [
        (40 + 50 * x) as u8,
        (30 + 70 * y) as u8,
        (200 - 40 * x - 20 * y) as u8,
    ]
}

fn probe_tile() -> (u32, u32, Vec<u8>) {
    let mut rgba = Vec::new();
    for y in 0..3 {
        for x in 0..4 {
            rgba.extend_from_slice(&probe(x, y));
            rgba.push(255);
        }
    }
    (4, 3, rgba)
}

/// A `W × H` CMYK document at `depth` with one opaque full-canvas layer.
fn cmyk_base(depth: u32) -> *mut PrecisionDocument {
    layer(
        backdrop(W, H, depth, &[[0.0; 4]]),
        "Shape",
        [10.0, 60.0, 30.0, 0.0],
        &hard_rect(W, H, (0, 0, W, H)),
    )
}

fn cmyk_flat(depth: u32, style: &Value) -> Vec<[u16; 5]> {
    let doc = styled(cmyk_base(depth), 1, style);
    let out = flat(doc);
    free(doc);
    out
}

fn color_overlay(rgb: [u8; 3]) -> Value {
    let hex = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
    json!({"effects":[{"type":"color_overlay","color":hex}]})
}

#[test]
fn cmyk_inks_mix_the_colour_overlays_of_the_texels_the_model_reads() {
    let tile = probe_tile();
    let l = level_of(&tile, 1.0);
    for depth in [8, 16] {
        // Every texel's ink, from the Colour Overlay of its colour.
        let inks: Vec<Vec<[u16; 5]>> = (0..12)
            .map(|i| cmyk_flat(depth, &color_overlay(probe(i % 4, i / 4))))
            .collect();
        let bound = if depth == 8 { 257.0 } else { 1.0 };
        for deg in [90.0, 30.0, -60.0] {
            let patterned = cmyk_flat(depth, &style(&tile, json!({"angle":deg})));
            for y in 0..H {
                for x in 0..W {
                    let i = (y * W + x) as usize;
                    let mut want = [0.0; 4];
                    for ((tx, ty), k) in taps(&l, (0.0, 0.0), deg, f64::from(x), f64::from(y)) {
                        for (c, slot) in want.iter_mut().enumerate() {
                            *slot += k * f64::from(inks[ty * 4 + tx][i][c]);
                        }
                    }
                    for (c, w) in want.iter().enumerate() {
                        let got = f64::from(patterned[i][c]);
                        assert!(
                            (got - w.round()).abs() <= bound,
                            "{depth}-bit angle {deg} ({x}, {y}) plate {c}: {got} against {w}"
                        );
                    }
                }
            }
        }
    }
}

// --------------------------------------------------------------- .rz 93 --

#[test]
fn the_typed_word_93_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let doc = set_style(
        &typed_doc(),
        &style(&tile_t(), json!({"angle":30,"scale":0.5})),
    )
    .unwrap();
    let path = dir.path().join("a.rz");
    let path = path.to_str().unwrap();
    doc.save_native(path, LIMIT, 0, None).unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &93u32.to_le_bytes(), "word 93 after 46");
    assert_eq!(&bytes[12..16], &90u32.to_le_bytes(), "93 implies 90");
    let reopened = PrecisionDocument::open_native(path, LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style()
    );
    let render = |d: &PrecisionDocument| d.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    assert_eq!(render(&reopened), render(&doc));
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = PrecisionDocument::open_native(cut.to_str().unwrap(), LIMIT, 0, None) else {
        panic!("an angle without the word must refuse");
    };
    assert_eq!(message, "native pattern overlay angles require DRDC 93");
    let plain = dir.path().join("plain.rz");
    set_style(&typed_doc(), &style(&tile_t(), json!({"scale":0.5})))
        .unwrap()
        .save_native(plain.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let mut claimed = std::fs::read(&plain).unwrap();
    assert_eq!(&claimed[8..12], &90u32.to_le_bytes(), "no 93 at angle 0");
    claimed.splice(8..8, 93u32.to_le_bytes());
    let forged = dir.path().join("forged.rz");
    std::fs::write(&forged, &claimed).unwrap();
    let Err(message) = PrecisionDocument::open_native(forged.to_str().unwrap(), LIMIT, 0, None)
    else {
        panic!("the word without an angle must refuse");
    };
    assert_eq!(message, "invalid native pattern overlay angle word");
}

#[test]
fn a_cmyk_file_claims_word_93_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let doc = styled(
        cmyk_base(16),
        1,
        &style(&probe_tile(), json!({"angle":-60,"scale":0.37})),
    );
    let path = dir.path().join("cmyk.rz");
    unsafe { &*doc }
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    let rung = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert!((82..=88).contains(&rung), "the CMYK rung, got {rung}");
    assert_eq!(&bytes[12..16], &93u32.to_le_bytes(), "93 after the rung");
    assert_eq!(&bytes[16..20], &90u32.to_le_bytes());
    let reopened = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    let reopened = Box::into_raw(Box::new(reopened));
    assert_eq!(
        unsafe { &*reopened }.layer(1).unwrap().reported_style(),
        unsafe { &*doc }.layer(1).unwrap().reported_style()
    );
    assert_eq!(flat(reopened), flat(doc));
    let mut stripped = bytes.clone();
    stripped.drain(12..16);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = PrecisionDocument::open_native(cut.to_str().unwrap(), LIMIT, 0, None) else {
        panic!("a CMYK angle without the word must refuse");
    };
    assert_eq!(message, "native pattern overlay angles require DRDC 93");
    free(reopened);
    free(doc);
}

#[test]
fn a_cmyk_psd_round_trip_carries_the_angle() {
    // `Angl` both ways in a CMYK file (`psd_pattern_overlay` holds the 8-
    // and 16-bit round trips; the CMYK helpers live in this suite).
    let dir = TempDir::new().unwrap();
    for depth in [8, 16] {
        let doc = styled(
            cmyk_base(depth),
            1,
            &style(&probe_tile(), json!({"angle":-60,"scale":0.5})),
        );
        let path = dir.path().join(format!("cmyk{depth}.psd"));
        let mut err = ptr::null_mut();
        let ok = unsafe {
            dr_precision_doc_save_psd(
                doc,
                ptr::null(),
                cpath(&path).as_ptr(),
                0,
                0,
                LIMIT,
                0,
                &mut err,
            )
        };
        assert!(ok, "{}", take_err_string(err));
        let mut needle = b"AnglUntF#Ang".to_vec();
        needle.extend_from_slice(&(-60f64).to_be_bytes());
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            bytes.windows(needle.len()).any(|w| w == needle),
            "{depth}-bit Angl"
        );
        let back = open_psd(&path, 0).unwrap();
        assert_eq!(
            unsafe { &*back }.layer(1).unwrap().reported_style(),
            unsafe { &*doc }.layer(1).unwrap().reported_style(),
            "{depth}-bit style"
        );
        assert_eq!(flat(back), flat(doc), "{depth}-bit");
        free(back);
        free(doc);
    }
}
