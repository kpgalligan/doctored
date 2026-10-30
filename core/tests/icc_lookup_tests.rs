//! `dr_lut_parse_profile`, Color Lookup's ICC LOOKS — an
//! RGB→RGB device link, stored at its own grid when it is a plain grid of at
//! most 33 nodes and otherwise sampled at 33³, and an abstract profile
//! (Lab or XYZ on either side) baked through the document's
//! profile at 33³ — into the ordinary 3D `color_lookup` table. The
//! re-bake, the XYZ oracles and word 71 are `icc_lookup_rebake.rs`.
//!
//! Every bound is declared here before anything is measured:
//!
//! - a byte-built two-node `mft2` PERMUTATION link, `(r, g, b) → (g, b, r)`:
//!   stored at size 2, exact to 1e-4 at every node;
//! - plain links of 2, 6, 11 and 33 nodes store `size` = `source_size` = n,
//!   each node the link's 16-bit entry within 1e-6; a 65-node link stores
//!   33 / 65; a 6-node link with a non-identity input curve stores 33 / 33;
//! - 6- and 11-node checkerboard links (take5's LKn) at take5's `cells`
//!   points: through `apply_at` within 1e-5 of an f64 tetrahedral read of
//!   the link's own grid, the 33-node bake missing by more than 1e-3 at one
//!   point at least; through `dr_image_adjust_op` within 1 code;
//! - a byte-built IDENTITY Lab abstract over sRGB: the identity to 1e-3;
//! - the lcms links: 8 nodes against `transicc -l` constants, within 1/255;
//! - the lcms abstracts over an sRGB document, against
//!   `ColorPipeline::new(&srgb, &display_p3, Relative, false).color` (which is
//!   what "P3 numbers read as sRGB" does to sRGB): mean ≤ 1/255, max ≤ 6/255;
//! - both depths reproduce the node values: 8 bits through
//!   `dr_image_adjust_op` at the eight corners (±1 code), 16 bits through
//!   `PrecisionAdjustment::parse` + `apply_at` at interior nodes (1e-4);
//! - an XYZ→XYZ abstract LOADS (once refused);
//! - the refusals: a `prtr` class, a CMYK link, a `D2B0`-only link, a
//!   `B2D2`-only link (the probe walks every `D2B0..3` / `B2D0..3`),
//!   an `mpet` A2B0 and an abstract with a NULL document;
//! - Adobe's `/Library/Application Support/Adobe/Color Profiles/*.icc`, each
//!   file's expectation read from its OWN header (class at byte 12, spaces at
//!   16 and 20): every RGB/RGB link and Lab/XYZ abstract loads, every other
//!   file refuses naming its class or space, and at least 13 load. SKIPPED
//!   when the folder is absent.
//!
//! The fixtures under `tests/fixtures/icc_lookup/` were built once with
//! LittleCMS 2.19, with `P3='/System/Library/ColorSync/Profiles/Display P3.icc'`:
//!
//! ```text
//! linkicc        -o srgb_to_p3.link.icc        '*sRGB' "$P3"
//! linkicc -r 2.1 -o srgb_to_p3.v2.link.icc     '*sRGB' "$P3"
//! linkicc -x -t1        -o p3_as_srgb.abst.icc    '*Lab' "$P3" '*sRGB' '*Lab'
//! linkicc -x -t1 -r 2.1 -o p3_as_srgb.v2.abst.icc '*Lab' "$P3" '*sRGB' '*Lab'
//! ```
//!
//! and each abstract's header class checked to read `abst`. The link
//! constants below are `transicc -n -l <link>` at the listed nodes.

use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::adjust_precision::PrecisionAdjustment;
use doctored_core::ffi::*;
use doctored_core::ffi_adjust::*;
use doctored_core::icc::IccProfile;
use doctored_core::icc_intent::{ColorPipeline, RenderingIntent};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common;
use common::*;

const SIZE: usize = 33;

// ------------------------------------------------------------- plumbing --

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/icc_lookup")
        .join(name)
}

fn srgb_bytes() -> Vec<u8> {
    IccProfile::srgb().bytes().to_vec()
}

/// `dr_lut_parse_profile` on `path`, with `document` as the document profile.
fn parse(path: &Path, document: Option<&[u8]>) -> Result<Value, String> {
    let c = cpath(path);
    let (bytes, len) = document.map_or((ptr::null(), 0), |d| (d.as_ptr(), d.len()));
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_lut_parse_profile(c.as_ptr(), bytes, len, &mut err) };
    if json.is_null() {
        return Err(take_err_string(err));
    }
    assert!(err.is_null(), "err_out set on success");
    let text = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(json) };
    Ok(serde_json::from_str(&text).expect("the core emits valid JSON"))
}

pub(crate) fn parse_bytes(
    dir: &TempDir,
    bytes: &[u8],
    document: Option<&[u8]>,
) -> Result<Value, String> {
    let path = dir.path().join("look.icc");
    std::fs::write(&path, bytes).expect("write profile");
    parse(&path, document)
}

/// RFC 4648 base64, decoded here rather than by the core.
fn b64_decode(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digits: Vec<u32> = text
        .bytes()
        .take_while(|&b| b != b'=')
        .map(|b| ALPHABET.iter().position(|&c| c == b).expect("base64 digit") as u32)
        .collect();
    let mut out = Vec::new();
    for chunk in digits.chunks(4) {
        let word = chunk
            .iter()
            .enumerate()
            .fold(0u32, |w, (i, &d)| w | d << (18 - 6 * i));
        let bytes = [(word >> 16) as u8, (word >> 8) as u8, word as u8];
        out.extend_from_slice(&bytes[..chunk.len() - 1]);
    }
    out
}

/// The stored 3D table: its `size` (never above `source_size`) and its
/// `size³` RGB triples, red fastest.
fn table(params: &Value) -> (usize, Vec<[f32; 3]>) {
    assert_eq!(params["kind"], "3d", "{params}");
    let size = params["size"].as_u64().expect("size") as usize;
    let source = params["source_size"].as_u64().expect("source_size") as usize;
    assert!(source >= size, "{params}");
    let bytes = b64_decode(params["table"].as_str().expect("table"));
    let floats: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    assert_eq!(floats.len(), size * size * size * 3);
    (
        size,
        floats.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
    )
}

/// A table stored at the 33³ bake (`size` and `source_size` both 33).
fn table33(params: &Value) -> Vec<[f32; 3]> {
    assert_eq!(params["size"], SIZE, "{params}");
    assert_eq!(params["source_size"], SIZE, "{params}");
    table(params).1
}

fn node(table: &[[f32; 3]], size: usize, r: usize, g: usize, b: usize) -> [f32; 3] {
    table[(b * size + g) * size + r]
}

/// Node `k`'s coordinate on a `size`-node axis.
fn at(size: usize, k: usize) -> f64 {
    k as f64 / (size - 1) as f64
}

// ------------------------------------------------ building profiles here --

/// An ICC profile of `class` from `space` to `pcs` at major `version`,
/// carrying `tags` in order, each 4-byte aligned.
pub(crate) fn profile(
    class: &[u8; 4],
    space: &[u8; 4],
    pcs: &[u8; 4],
    version: u8,
    tags: &[(&[u8; 4], Vec<u8>)],
) -> Vec<u8> {
    let mut data = Vec::new();
    let mut table = Vec::new();
    let start = 128 + 4 + 12 * tags.len();
    for (sig, body) in tags {
        let offset = start + data.len();
        table.extend_from_slice(*sig);
        table.extend_from_slice(&(offset as u32).to_be_bytes());
        table.extend_from_slice(&(body.len() as u32).to_be_bytes());
        data.extend_from_slice(body);
        while data.len() % 4 != 0 {
            data.push(0);
        }
    }
    let mut out = vec![0u8; 128];
    out[8] = version;
    out[12..16].copy_from_slice(class);
    out[16..20].copy_from_slice(space);
    out[20..24].copy_from_slice(pcs);
    out[36..40].copy_from_slice(b"acsp");
    out.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&data);
    let size = out.len() as u32;
    out[0..4].copy_from_slice(&size.to_be_bytes());
    out
}

/// A `lut16Type` (`mft2`) 3→3 table: identity matrix, two-entry identity
/// curves, and a `grid`³ CLUT whose node at normalized input `x` holds
/// `clut(x)` — first input varying SLOWEST, as ICC.1 lays it out.
pub(crate) fn mft2(grid: usize, clut: impl Fn([f64; 3]) -> [f64; 3]) -> Vec<u8> {
    mft2_with_input(grid, &[0, 65535], clut)
}

/// [`mft2`] with `input` as each channel's input table (16-bit entries).
fn mft2_with_input(grid: usize, input: &[u16], clut: impl Fn([f64; 3]) -> [f64; 3]) -> Vec<u8> {
    let mut out = b"mft2".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&[3, 3, grid as u8, 0]);
    for i in 0..9 {
        let v: i32 = if i % 4 == 0 { 0x10000 } else { 0 };
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&(input.len() as u16).to_be_bytes());
    out.extend_from_slice(&2u16.to_be_bytes());
    let u16s = |out: &mut Vec<u8>, v: f64| {
        out.extend_from_slice(&((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes())
    };
    for _ in 0..3 {
        for entry in input {
            out.extend_from_slice(&entry.to_be_bytes());
        }
    }
    let step = (grid - 1) as f64;
    for i0 in 0..grid {
        for i1 in 0..grid {
            for i2 in 0..grid {
                let value = clut([i0 as f64 / step, i1 as f64 / step, i2 as f64 / step]);
                for v in value {
                    u16s(&mut out, v);
                }
            }
        }
    }
    for _ in 0..3 {
        u16s(&mut out, 0.0);
        u16s(&mut out, 1.0);
    }
    out
}

/// A `textDescriptionType` naming the profile.
pub(crate) fn desc(name: &str) -> Vec<u8> {
    let mut out = b"desc".to_vec();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(name.len() as u32 + 1).to_be_bytes());
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    out
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_byte_built_permutation_link_is_exact_at_every_node() {
    let dir = TempDir::new().unwrap();
    let bytes = profile(
        b"link",
        b"RGB ",
        b"RGB ",
        2,
        &[
            (b"desc", desc("Permute")),
            (b"A2B0", mft2(2, |[r, g, b]| [g, b, r])),
        ],
    );
    // A link needs no document profile.
    let params = parse_bytes(&dir, &bytes, None).expect("a link loads");
    assert_eq!(params["title"], "Permute");
    // A two-node plain grid is stored at its own grid.
    assert_eq!(
        (&params["size"], &params["source_size"]),
        (&json!(2), &json!(2))
    );
    let (size, table) = table(&params);
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let got = node(&table, size, r, g, b);
                let want = [at(size, g), at(size, b), at(size, r)];
                for c in 0..3 {
                    assert!(
                        (f64::from(got[c]) - want[c]).abs() <= 1e-4,
                        "node ({r},{g},{b}) channel {c}: {got:?} vs {want:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_byte_built_identity_abstract_is_the_identity_over_srgb() {
    let dir = TempDir::new().unwrap();
    let bytes = profile(
        b"abst",
        b"Lab ",
        b"Lab ",
        2,
        &[(b"A2B0", mft2(2, |lab| lab))],
    );
    let params = parse_bytes(&dir, &bytes, Some(&srgb_bytes())).expect("an abstract loads");
    let table = table33(&params);
    let mut worst = 0.0f64;
    for b in 0..SIZE {
        for g in 0..SIZE {
            for r in 0..SIZE {
                let got = node(&table, SIZE, r, g, b);
                for (c, want) in [at(SIZE, r), at(SIZE, g), at(SIZE, b)]
                    .into_iter()
                    .enumerate()
                {
                    worst = worst.max((f64::from(got[c]) - want).abs());
                }
            }
        }
    }
    assert!(worst <= 1e-3, "identity abstract drifted by {worst}");
}

#[test]
fn the_lcms_links_match_transicc_at_eight_nodes() {
    // (node, transicc -n -l output ×255) for each link.
    let nodes: [[usize; 3]; 8] = [
        [0, 0, 0],
        [32, 0, 0],
        [0, 32, 0],
        [0, 0, 32],
        [32, 32, 32],
        [16, 8, 4],
        [4, 20, 28],
        [25, 25, 5],
    ];
    let v4: [[f64; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [233.9611, 51.0661, 35.3424],
        [116.8988, 251.2412, 76.0584],
        [0.0, 0.0, 244.6965],
        [255.0, 254.9961, 255.0],
        [119.1790, 67.0934, 39.1206],
        [76.7276, 157.0506, 217.5992],
        [199.2218, 199.2179, 75.0311],
    ];
    let v2: [[f64; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [233.9572, 51.0661, 35.3346],
        [116.8949, 251.2412, 76.0584],
        [0.0156, 0.0, 244.6965],
        [255.0, 255.0, 255.0],
        [119.1790, 67.0973, 39.1245],
        [76.7237, 157.0506, 217.5992],
        [199.2179, 199.2179, 75.0272],
    ];
    for (file, expected) in [("srgb_to_p3.link.icc", v4), ("srgb_to_p3.v2.link.icc", v2)] {
        let params = parse(&fixture(file), None).unwrap_or_else(|e| panic!("{file}: {e}"));
        let table = table33(&params);
        for ([r, g, b], want) in nodes.iter().zip(expected) {
            let got = node(&table, SIZE, *r, *g, *b);
            for c in 0..3 {
                let diff = (f64::from(got[c]) * 255.0 - want[c]).abs();
                assert!(
                    diff <= 1.0,
                    "{file} node ({r},{g},{b}) channel {c}: {} vs {}",
                    f64::from(got[c]) * 255.0,
                    want[c]
                );
            }
        }
    }
}

#[test]
fn the_lcms_abstracts_over_srgb_match_the_srgb_to_p3_conversion() {
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    let oracle = ColorPipeline::new(&srgb, &p3, RenderingIntent::RelativeColorimetric, false)
        .expect("sRGB to Display P3");
    for file in ["p3_as_srgb.abst.icc", "p3_as_srgb.v2.abst.icc"] {
        let params =
            parse(&fixture(file), Some(&srgb_bytes())).unwrap_or_else(|e| panic!("{file}: {e}"));
        let table = table33(&params);
        let (mut sum, mut worst, mut count) = (0.0f64, 0.0f64, 0usize);
        for b in 0..SIZE {
            for g in 0..SIZE {
                for r in 0..SIZE {
                    let got = node(&table, SIZE, r, g, b);
                    let pos = |k| at(SIZE, k) as f32;
                    let want = oracle.color([pos(r), pos(g), pos(b)]);
                    for c in 0..3 {
                        let diff = f64::from((got[c] - want[c]).abs());
                        sum += diff;
                        worst = worst.max(diff);
                        count += 1;
                    }
                }
            }
        }
        let mean = sum / count as f64;
        assert!(mean <= 1.0 / 255.0, "{file}: mean {}", mean * 255.0);
        assert!(worst <= 6.0 / 255.0, "{file}: max {}", worst * 255.0);
    }
}

#[test]
fn both_depths_reproduce_the_node_values() {
    let params = parse(&fixture("srgb_to_p3.link.icc"), None).expect("link");
    let table = table33(&params);

    // 8 bits, through the one destructive export, at the eight corners.
    let corners: Vec<[usize; 3]> = (0..8)
        .map(|i| [(i & 1) * 32, (i >> 1 & 1) * 32, (i >> 2 & 1) * 32])
        .collect();
    let row = RgbaImage::from_fn(8, 1, |x, _| {
        let [r, g, b] = corners[x as usize];
        Rgba([
            (r / 32 * 255) as u8,
            (g / 32 * 255) as u8,
            (b / 32 * 255) as u8,
            255,
        ])
    });
    let dir = TempDir::new().unwrap();
    let img = open_image(&dir, "corners.png", &row);
    let op = CString::new("color_lookup").unwrap();
    let text = CString::new(params.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_adjust_op(img, op.as_ptr(), text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "adjust_op: {}", take_err_string(err));
    let pixels = img_pixels(out);
    for (x, [r, g, b]) in corners.iter().enumerate() {
        let want = node(&table, SIZE, *r, *g, *b);
        for c in 0..3 {
            let expected = (f64::from(want[c]) * 255.0).round();
            let got = f64::from(pixels[x * 4 + c]);
            assert!(
                (got - expected).abs() <= 1.0,
                "8-bit corner {x} channel {c}: {got} vs {expected}"
            );
        }
    }
    unsafe {
        dr_image_free(out);
        dr_image_free(img);
    }

    // 16 bits, through the typed preparation, at interior nodes.
    let description = json!({"type": "adjust", "evaluation": "precision_v1", "op": "color_lookup",
               "params": params})
    .to_string();
    let prepared =
        PrecisionAdjustment::parse(&description, &IccProfile::srgb()).expect("typed parse");
    for [r, g, b] in [[16, 8, 4], [4, 20, 28], [25, 25, 5], [31, 1, 17]] {
        let pos = |k| at(SIZE, k) as f32;
        let got = prepared.apply_at([pos(r), pos(g), pos(b)], 0.0, (0, 0));
        let want = node(&table, SIZE, r, g, b);
        for c in 0..3 {
            assert!(
                (got[c] - want[c]).abs() <= 1e-4,
                "16-bit node ({r},{g},{b}) channel {c}: {} vs {}",
                got[c],
                want[c]
            );
        }
    }
}

// ------------------------------------------------ a link at its own grid --

/// The take5 checkerboard wave (`scripts/photoshop-probe/float_adjust_take5.py`,
/// LKn): node (i_r, i_g, i_b) channel c is 0.5 + 0.25(−1)^i_c +
/// 0.1(−1)^(i_c + i_(c+1)) + 0.15(s_(c+1) − 0.5), s = i/(n − 1).
fn wave(n: usize, idx: [usize; 3]) -> [f64; 3] {
    let sign = |k: usize| if k.is_multiple_of(2) { 1.0 } else { -1.0 };
    std::array::from_fn(|c| {
        let next = (c + 1) % 3;
        0.5 + 0.25 * sign(idx[c])
            + 0.1 * sign(idx[c] + idx[next])
            + 0.15 * (idx[next] as f64 / (n - 1) as f64 - 0.5)
    })
}

/// A function of the node's fractions that is no simple ramp, for grids
/// whose nodes are checked one by one.
fn bumpy([r, g, b]: [f64; 3]) -> [f64; 3] {
    [
        0.5 + 0.4 * (7.0 * r + 3.0 * g).sin() * b,
        (r * g + 0.3 * b).min(1.0),
        0.2 + 0.6 * (r - b).abs(),
    ]
}

/// A link's 16-bit entry, as `mft2` stores it.
fn stored(v: f64) -> f64 {
    (v.clamp(0.0, 1.0) * 65535.0).round() / 65535.0
}

/// The independent f64 tetrahedral read (Kuhn) of an `n`-node table: per
/// axis the lower node and the fraction above it, the axes ordered by
/// fraction descending (ties in r, g, b order), the corner walked from
/// (0, 0, 0) to (1, 1, 1) one axis at a time.
fn tetra(n: usize, entry: &dyn Fn([usize; 3]) -> [f64; 3], v: [f64; 3]) -> [f64; 3] {
    let split = |x: f64| {
        let t = x.clamp(0.0, 1.0) * (n - 1) as f64;
        let i = (t.floor() as usize).min(n - 2);
        (i, t - i as f64)
    };
    let cell = v.map(split);
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| cell[b].1.total_cmp(&cell[a].1));
    let mut at = cell.map(|(i, _)| i);
    let mut previous = entry(at);
    let mut out = previous;
    for axis in order {
        at[axis] += 1;
        let here = entry(at);
        for c in 0..3 {
            out[c] += cell[axis].1 * (here[c] - previous[c]);
        }
        previous = here;
    }
    out
}

/// The take5 `cells` points for an `n`-node grid: around each interior node
/// value e = k/(n − 1), three triples that straddle the node in different
/// orders.
fn cells(n: usize) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for k in 1..n - 1 {
        let e = k as f64 / (n - 1) as f64;
        for d in [
            [-1.0 / 64.0, 1.0 / 64.0, 1.0 / 128.0],
            [1.0 / 64.0, -1.0 / 64.0, -1.0 / 128.0],
            [0.6 / 64.0, 0.2 / 64.0, -1.0 / 64.0],
        ] {
            out.push(d.map(|x| (e + x).clamp(0.0, 1.0)));
        }
    }
    out
}

fn wave_link(n: usize) -> Vec<u8> {
    let link = mft2(n, |x| {
        wave(n, x.map(|v| (v * (n - 1) as f64).round() as usize))
    });
    profile(
        b"link",
        b"RGB ",
        b"RGB ",
        2,
        &[(b"desc", desc("Wave")), (b"A2B0", link)],
    )
}

#[test]
fn a_plain_link_is_stored_at_its_own_grid() {
    let dir = TempDir::new().unwrap();
    for n in [2usize, 6, 11, 33] {
        let bytes = profile(b"link", b"RGB ", b"RGB ", 2, &[(b"A2B0", mft2(n, bumpy))]);
        let params = parse_bytes(&dir, &bytes, None).expect("a link loads");
        assert_eq!(params["size"], n, "{n}-node link");
        assert_eq!(params["source_size"], n, "{n}-node link");
        let (size, table) = table(&params);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let want = bumpy([at(n, r), at(n, g), at(n, b)]).map(stored);
                    let got = node(&table, size, r, g, b);
                    for c in 0..3 {
                        assert!(
                            (f64::from(got[c]) - want[c]).abs() <= 1e-6,
                            "{n}-node link node ({r},{g},{b}) channel {c}: {got:?} vs {want:?}"
                        );
                    }
                }
            }
        }
    }
    // Above 33 nodes the link is sampled at 33³ and reports its own grid.
    let bytes = profile(b"link", b"RGB ", b"RGB ", 2, &[(b"A2B0", mft2(65, bumpy))]);
    let params = parse_bytes(&dir, &bytes, None).expect("a 65-node link loads");
    assert_eq!(
        (&params["size"], &params["source_size"]),
        (&json!(33), &json!(65))
    );
    // A non-identity input curve is part of the transform: sampled at 33³.
    let curved = mft2_with_input(6, &[0, 20000, 65535], bumpy);
    let bytes = profile(b"link", b"RGB ", b"RGB ", 2, &[(b"A2B0", curved)]);
    let params = parse_bytes(&dir, &bytes, None).expect("a curved link loads");
    assert_eq!(
        (&params["size"], &params["source_size"]),
        (&json!(33), &json!(33))
    );
}

/// A 6- and an 11-node link render as Photoshop reads them — the
/// tetrahedral read of the link's OWN grid (receipt `float-adjust-473/take5`,
/// LG `own`) — through the typed preparation's `apply_at` within 1e-5,
/// where the 33-node bake misses by more than 1e-3, and at 8 bits through
/// the destructive op within 1 code (32 bits: `float_lookup_tint_map.rs`).
#[test]
fn a_link_renders_on_its_own_grid_at_every_depth() {
    let dir = TempDir::new().unwrap();
    for n in [6usize, 11] {
        let params = parse_bytes(&dir, &wave_link(n), None).expect("the wave link loads");
        assert_eq!(params["size"], n);
        let own = |idx: [usize; 3]| wave(n, idx).map(stored);
        let bake = |idx: [usize; 3]| tetra(n, &own, idx.map(|k| at(SIZE, k)));
        let description = json!({"type": "adjust", "evaluation": "precision_v1",
                                 "op": "color_lookup", "params": params})
        .to_string();
        let prepared =
            PrecisionAdjustment::parse(&description, &IccProfile::srgb()).expect("typed parse");
        let mut bake_miss = 0.0f64;
        for point in cells(n) {
            let input = point.map(|v| v as f32);
            let got = prepared.apply_at(input, 0.0, (0, 0));
            let v = input.map(f64::from);
            let want = tetra(n, &own, v);
            let baked = tetra(SIZE, &bake, v);
            for c in 0..3 {
                let got = f64::from(got[c]);
                assert!(
                    (got - want[c]).abs() <= 1e-5,
                    "{n}-node link at {v:?} channel {c}: {got} vs {}",
                    want[c]
                );
                bake_miss = bake_miss.max((got - baked[c]).abs());
            }
        }
        assert!(
            bake_miss > 1e-3,
            "{n}-node link: the 33-node bake is {bake_miss} away"
        );

        // 8 bits, through the destructive op, on the cells points' codes.
        let points = cells(n);
        let codes: Vec<[u8; 3]> = points
            .iter()
            .map(|p| p.map(|v| (v * 255.0).round() as u8))
            .collect();
        let row = RgbaImage::from_fn(codes.len() as u32, 1, |x, _| {
            let [r, g, b] = codes[x as usize];
            Rgba([r, g, b, 255])
        });
        let img = open_image(&dir, &format!("cells{n}.png"), &row);
        let op = CString::new("color_lookup").unwrap();
        let text = CString::new(params.to_string()).unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe { dr_image_adjust_op(img, op.as_ptr(), text.as_ptr(), &mut err) };
        assert!(!out.is_null(), "adjust_op: {}", take_err_string(err));
        let pixels = img_pixels(out);
        for (x, code) in codes.iter().enumerate() {
            let want = tetra(n, &own, code.map(|c| f64::from(c) / 255.0));
            for c in 0..3 {
                let expected = (want[c] * 255.0).round();
                let got = f64::from(pixels[x * 4 + c]);
                assert!(
                    (got - expected).abs() <= 1.0,
                    "{n}-node link, 8-bit pixel {x} channel {c}: {got} vs {expected}"
                );
            }
        }
        unsafe {
            dr_image_free(out);
            dr_image_free(img);
        }
    }
}

/// (what, profile bytes, document profile, words the refusal must name).
type Refusal<'a> = (&'a str, Vec<u8>, Option<&'a [u8]>, &'a [&'a str]);

#[test]
fn every_other_profile_is_refused_by_name() {
    let dir = TempDir::new().unwrap();
    let srgb = srgb_bytes();
    let identity = || mft2(2, |x| x);
    let cases: [Refusal; 6] = [
        (
            "a pixel class",
            profile(b"prtr", b"RGB ", b"Lab ", 2, &[(b"A2B0", identity())]),
            Some(&srgb),
            &["prtr", "RGB", "Lab"],
        ),
        (
            "a CMYK link",
            profile(b"link", b"CMYK", b"CMYK", 2, &[(b"A2B0", identity())]),
            None,
            &["link", "CMYK"],
        ),
        (
            "a D2B0-only link",
            profile(b"link", b"RGB ", b"RGB ", 4, &[(b"D2B0", vec![0; 32])]),
            None,
            &["D2B"],
        ),
        (
            "a B2D2-only link",
            profile(b"link", b"RGB ", b"RGB ", 4, &[(b"B2D2", vec![0; 32])]),
            None,
            &["B2D2"],
        ),
        (
            "an mpet A2B0",
            profile(
                b"link",
                b"RGB ",
                b"RGB ",
                4,
                &[(b"A2B0", {
                    let mut tag = b"mpet".to_vec();
                    tag.extend_from_slice(&[0; 28]);
                    tag
                })],
            ),
            None,
            &["multiProcessElements"],
        ),
        (
            "an abstract with no document",
            profile(b"abst", b"Lab ", b"Lab ", 2, &[(b"A2B0", identity())]),
            None,
            &["document"],
        ),
    ];
    for (what, bytes, document, words) in cases {
        let message = match parse_bytes(&dir, &bytes, document) {
            Err(message) => message,
            Ok(params) => panic!("{what} must be refused: {params}"),
        };
        assert!(message.starts_with("failed to read "), "{what}: {message}");
        for word in words {
            assert!(message.contains(word), "{what} names {word}: {message}");
        }
    }
    // A missing file names its path.
    let missing = dir.path().join("absent.icc");
    let message = parse(&missing, None).expect_err("a missing file");
    assert!(message.contains("absent.icc"), "{message}");
    // An XYZ abstract, once refused, now loads.
    let xyz = profile(b"abst", b"XYZ ", b"XYZ ", 2, &[(b"A2B0", identity())]);
    let params = parse_bytes(&dir, &xyz, Some(&srgb)).expect("an XYZ abstract loads");
    table33(&params);
}

#[test]
fn adobes_color_profiles_folder_loads_every_look_it_can() {
    let folder = Path::new("/Library/Application Support/Adobe/Color Profiles");
    let Ok(entries) = std::fs::read_dir(folder) else {
        eprintln!("SKIP: {} is absent", folder.display());
        return;
    };
    let srgb = srgb_bytes();
    let mut loaded = 0;
    let mut refused = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("icc") {
            continue;
        }
        let header = std::fs::read(&path).expect("read profile");
        let field = |at: usize| {
            String::from_utf8_lossy(&header[at..at + 4])
                .trim_end()
                .to_string()
        };
        let (class, space, pcs) = (field(12), field(16), field(20));
        let loads = (class == "link" && space == "RGB" && pcs == "RGB")
            || (class == "abst"
                && ["Lab", "XYZ"].contains(&space.as_str())
                && ["Lab", "XYZ"].contains(&pcs.as_str()));
        match parse(&path, Some(&srgb)) {
            Ok(params) => {
                assert!(loads, "{} ({class} {space}->{pcs}) loaded", path.display());
                table(&params);
                loaded += 1;
            }
            Err(message) => {
                assert!(
                    !loads,
                    "{} ({class} {space}->{pcs}): {message}",
                    path.display()
                );
                assert!(
                    message.contains(&class) || message.contains(&space),
                    "{}: the refusal names the class or space: {message}",
                    path.display()
                );
                refused.push(path.file_name().map(|n| n.to_string_lossy().into_owned()));
            }
        }
    }
    eprintln!("Adobe looks: {loaded} loaded, refused {refused:?}");
    assert!(loaded >= 13, "{loaded} looks loaded");
}
