//! #2: Vivid Light, Linear Light and Divide in an 8-bit RGB document blend
//! on CODES. The backdrop and the source are each rounded to their 8-bit code
//! (`b`, `s`), and the result is a code (`rdiv(n, d)` is `n / d` rounded half
//! up):
//!
//! - Linear Light: `clamp(b + 2s − 256, 0, 255)`;
//! - Divide: `s = 0` gives 0 over `b = 0`, else 255; otherwise
//!   `min(255, rdiv(255·b, s))`;
//! - Vivid Light: `s = 0` gives 0 and `s = 255` gives 255; below 128 the burn
//!   half `t = (255·s + 64) >> 7`, `max(0, 255 − rdiv(255·(255 − b), t))`;
//!   from 128 the dodge half `t = rdiv(255·(s − 128), 127)`, 255 when
//!   `t ≥ 255`, else `min(255, rdiv(255·b, 255 − t))`.
//!
//! A Fill below 100 % moves Vivid Light's and Linear Light's source toward
//! code 128 before the code rule: `128/255 + f·(cs − 128/255)`, `f` the byte
//! Fill (153/255 at 60 %, 64/255 at 25 %). A recorded take
//! (`psd_appearance_578_take.py`, takes 1 and 2) measured these forms on
//! every judged 8-bit tile, the clip tile the float forms missed included.
//!
//! The oracle is the rule above, in integers, written here. A clip group's
//! source is the code of the member's Normal lerp over the opaque base (the
//! group buffer holds it in float, so its code is what the base's blend
//! reads). Every 8-bit case asserts the full-frame flatten
//! (`dr_doc_flattened`) and that the regional render (`dr_doc_render_region`
//! over the canvas at divisor 1) equals it byte for byte. The guards: Divide's
//! opaque lattice, every mode at source alpha 96, and the float forms a
//! `linear_light` byte document and a 16-bit document keep.

use std::ffi::c_char;
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::{dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width};
use doctored_core::ffi_doc::*;
use doctored_core::ffi_evaluation::dr_doc_set_evaluation;
use doctored_core::ffi_regional::dr_doc_render_region;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common::{
    add_layer, apply, apply_or_keep, doc_from, set_style, take_err_string, BLEND_DIVIDE,
    BLEND_LINEAR_LIGHT, BLEND_VIVID_LIGHT,
};

/// The lattice: backdrop grey `x`, source grey `y`, every code pair.
const N: u32 = 256;
/// A clip case's canvas; every layer is uniform, so `PROBE` is the answer.
const CLIP: u32 = 8;
const PROBE: (u32, u32) = (3, 3);
/// The clip member's alpha (the measured topology).
const MEMBER_ALPHA: u8 = 96;

const MODES: [i32; 3] = [BLEND_VIVID_LIGHT, BLEND_LINEAR_LIGHT, BLEND_DIVIDE];

// ---------------------------------------------------------- the oracle --

/// `n / d` rounded half up.
fn rdiv(n: i64, d: i64) -> i64 {
    (2 * n + d) / (2 * d)
}

fn linear_light8(b: u8, s: u8) -> u8 {
    (i64::from(b) + 2 * i64::from(s) - 256).clamp(0, 255) as u8
}

fn divide8(b: u8, s: u8) -> u8 {
    let (b, s) = (i64::from(b), i64::from(s));
    if s == 0 {
        return if b == 0 { 0 } else { 255 };
    }
    rdiv(255 * b, s).min(255) as u8
}

fn vivid_light8(b: u8, s: u8) -> u8 {
    let (b, s) = (i64::from(b), i64::from(s));
    if s == 0 {
        return 0;
    }
    if s == 255 {
        return 255;
    }
    if s < 128 {
        let t = (255 * s + 64) >> 7;
        return (255 - rdiv(255 * (255 - b), t)).max(0) as u8;
    }
    let t = rdiv(255 * (s - 128), 127);
    if t >= 255 {
        255
    } else {
        rdiv(255 * b, 255 - t).min(255) as u8
    }
}

/// The code rule of `mode`.
fn code_rule(mode: i32, b: u8, s: u8) -> u8 {
    match mode {
        BLEND_VIVID_LIGHT => vivid_light8(b, s),
        BLEND_LINEAR_LIGHT => linear_light8(b, s),
        BLEND_DIVIDE => divide8(b, s),
        other => panic!("mode {other} has no code rule here"),
    }
}

/// The byte Fill of `percent` as a code over 255 (153 at 60, 64 at 25).
fn fill_code(percent: i64) -> i64 {
    rdiv(percent * 255, 100)
}

/// The source moved toward code 128 at the byte Fill `f` (over 255), as a
/// code: `round(128 + f·(s − 128)/255)`.
fn folded(s: u8, f: i64) -> u8 {
    rdiv(128 * 255 + f * (i64::from(s) - 128), 255) as u8
}

/// `round(b + a·(blended − b)/255)`: the code rule composited at source
/// alpha `a` over an opaque backdrop.
fn at_alpha(b: u8, blended: u8, a: u8) -> u8 {
    let (b, x, a) = (i64::from(b), i64::from(blended), i64::from(a));
    rdiv(255 * b + a * (x - b), 255) as u8
}

/// A clip group's source code: the member at `MEMBER_ALPHA` Normal over the
/// opaque base.
fn group_code(base: u8, member: u8) -> u8 {
    at_alpha(base, member, MEMBER_ALPHA)
}

/// The float forms (`blend.rs`'s masters), in f64 on [0, 1].
fn float_rule(mode: i32, b: f64, s: f64) -> f64 {
    let burn = |b: f64, s: f64| {
        if b >= 1.0 {
            1.0
        } else if s <= 0.0 {
            0.0
        } else {
            1.0 - ((1.0 - b) / s).min(1.0)
        }
    };
    let dodge = |b: f64, s: f64| {
        if b <= 0.0 {
            0.0
        } else if s >= 1.0 {
            1.0
        } else {
            (b / (1.0 - s)).min(1.0)
        }
    };
    match mode {
        BLEND_VIVID_LIGHT if s <= 0.0 => 0.0,
        BLEND_VIVID_LIGHT if s >= 1.0 => 1.0,
        BLEND_VIVID_LIGHT if s <= 0.5 => burn(b, 2.0 * s),
        BLEND_VIVID_LIGHT => dodge(b, 2.0 * s - 1.0),
        BLEND_LINEAR_LIGHT => (b + 2.0 * s - 1.0).clamp(0.0, 1.0),
        BLEND_DIVIDE if s <= 0.0 => {
            if b <= 0.0 {
                0.0
            } else {
                1.0
            }
        }
        BLEND_DIVIDE => (b / s).min(1.0),
        other => panic!("mode {other} has no float rule here"),
    }
}

/// The analytic sRGB decode and encode, for the `linear_light` guard.
fn decode(code: u8) -> f64 {
    let x = f64::from(code) / 255.0;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    255.0
        * if v <= 0.003_130_8 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
}

fn name(mode: i32) -> &'static str {
    match mode {
        BLEND_VIVID_LIGHT => "Vivid Light",
        BLEND_LINEAR_LIGHT => "Linear Light",
        _ => "Divide",
    }
}

fn blend_mode(mode: i32) -> BlendMode {
    BlendMode::from_c(mode).expect("a blend mode")
}

// -------------------------------------------------------- the fixtures --

fn grey(v: u8, a: u8) -> Rgba<u8> {
    Rgba([v, v, v, a])
}

fn uniform(c: [u8; 3], a: u8) -> RgbaImage {
    RgbaImage::from_pixel(CLIP, CLIP, Rgba([c[0], c[1], c[2], a]))
}

fn set_blend(doc: *mut DrDocument, idx: usize, mode: i32) -> *mut DrDocument {
    apply_or_keep(doc, |d| unsafe {
        dr_doc_with_layer_blend_mode(d, idx, mode)
    })
}

/// `[backdrop grey x, source grey y at alpha a in mode]` over the lattice.
fn lattice(dir: &TempDir, mode: i32, a: u8) -> *mut DrDocument {
    let backdrop = RgbaImage::from_fn(N, N, |x, _| grey(x as u8, 255));
    let source = RgbaImage::from_fn(N, N, |_, y| grey(y as u8, a));
    let doc = doc_from(dir, "back.png", &backdrop);
    let doc = add_layer(dir, "source.png", doc, 0, &source, "Source");
    set_blend(doc, 1, mode)
}

/// `[backdrop, base in mode, member at MEMBER_ALPHA Normal clipped to it]`.
fn clip_base(
    dir: &TempDir,
    mode: i32,
    backdrop: [u8; 3],
    base: [u8; 3],
    member: [u8; 3],
) -> *mut DrDocument {
    let doc = doc_from(dir, "back.png", &uniform(backdrop, 255));
    let doc = add_layer(dir, "base.png", doc, 0, &uniform(base, 255), "Base");
    let doc = set_blend(doc, 1, mode);
    let doc = add_layer(
        dir,
        "member.png",
        doc,
        1,
        &uniform(member, MEMBER_ALPHA),
        "Member",
    );
    apply(doc, |d| unsafe { dr_doc_with_layer_clipped(d, 2, true) })
}

fn image_bytes(out: *mut doctored_core::DrImage) -> Vec<u8> {
    let (w, h) = unsafe { (dr_image_width(out), dr_image_height(out)) };
    let pixels = unsafe { dr_image_pixels_rgba(out) };
    let bytes = unsafe { std::slice::from_raw_parts(pixels, (w * h * 4) as usize) }.to_vec();
    unsafe { dr_image_free(out) };
    bytes
}

/// The full-frame flatten and the regional render over the `side`-square
/// canvas at divisor 1, asserted equal; frees `doc`.
fn rendered(doc: *mut DrDocument, side: u32, what: &str) -> Vec<u8> {
    let mut err: *mut c_char = ptr::null_mut();
    let flat = unsafe { dr_doc_flattened(doc, u64::MAX, ptr::null(), &mut err) };
    assert!(
        !flat.is_null(),
        "{what}: flattened: {}",
        take_err_string(err)
    );
    let full = image_bytes(flat);
    let mut err: *mut c_char = ptr::null_mut();
    let region =
        unsafe { dr_doc_render_region(doc, 0, 0, side, side, 1, u64::MAX, ptr::null(), &mut err) };
    unsafe { dr_doc_free(doc) };
    assert!(
        !region.is_null(),
        "{what}: render_region: {}",
        take_err_string(err)
    );
    assert!(
        full == image_bytes(region),
        "{what}: the regional render differs from the full frame"
    );
    full
}

/// Every lattice pixel's three channels within `tol` codes of `want(b, s)`.
fn assert_lattice(bytes: &[u8], want: impl Fn(u8, u8) -> f64, tol: f64, what: &str) {
    let mut misses = 0usize;
    let mut worst = 0.0f64;
    let mut first = Vec::new();
    for y in 0..N {
        for x in 0..N {
            let i = ((y * N + x) * 4) as usize;
            let w = want(x as u8, y as u8);
            for c in 0..3 {
                let d = (f64::from(bytes[i + c]) - w).abs();
                worst = worst.max(d);
                if d > tol {
                    misses += 1;
                    if first.len() < 6 {
                        first.push((x, y, bytes[i + c], w));
                    }
                }
            }
            assert_eq!(bytes[i + 3], 255, "{what}: ({x}, {y}) is opaque");
        }
    }
    assert!(
        misses == 0,
        "{what}: {misses} channels beyond {tol} (worst {worst}); (b, s, got, want): {first:?}"
    );
}

fn probe(bytes: &[u8]) -> [u8; 3] {
    let i = ((PROBE.1 * CLIP + PROBE.0) * 4) as usize;
    assert_eq!(bytes[i + 3], 255, "the clip probe is opaque");
    [bytes[i], bytes[i + 1], bytes[i + 2]]
}

/// The clip-base oracle: the code rule on the group's code per channel.
fn clip_want(mode: i32, backdrop: [u8; 3], base: [u8; 3], member: [u8; 3]) -> [u8; 3] {
    [0, 1, 2].map(|k| code_rule(mode, backdrop[k], group_code(base[k], member[k])))
}

fn assert_clip_base(mode: i32, backdrop: [u8; 3], base: [u8; 3], member: [u8; 3], want: [u8; 3]) {
    assert_eq!(
        clip_want(mode, backdrop, base, member),
        want,
        "the oracle's {} triple over {backdrop:?}, base {base:?}, member {member:?}",
        name(mode)
    );
    let dir = TempDir::new().unwrap();
    let what = format!(
        "{} clip base {base:?} member {member:?} over {backdrop:?}",
        name(mode)
    );
    let got = probe(&rendered(
        clip_base(&dir, mode, backdrop, base, member),
        CLIP,
        &what,
    ));
    assert_eq!(got, want, "{what}");
}

fn assert_code_lattice(mode: i32) {
    let dir = TempDir::new().unwrap();
    let what = format!("{} opaque lattice", name(mode));
    let bytes = rendered(lattice(&dir, mode, 255), N, &what);
    assert_lattice(&bytes, |b, s| f64::from(code_rule(mode, b, s)), 0.0, &what);
}

/// Each `(mode, member, want)` of the measured topology's column 1: backdrop
/// 64, base (235, 38, 201).
const COLUMN_1: [(i32, [u8; 3], [u8; 3]); 6] = [
    (BLEND_VIVID_LIGHT, [191, 191, 191], [221, 0, 141]),
    (BLEND_VIVID_LIGHT, [189, 69, 57], [221, 0, 75]),
    (BLEND_LINEAR_LIGHT, [191, 191, 191], [244, 0, 202]),
    (BLEND_LINEAR_LIGHT, [189, 69, 57], [244, 0, 102]),
    (BLEND_DIVIDE, [191, 191, 191], [75, 170, 83]),
    (BLEND_DIVIDE, [189, 69, 57], [75, 255, 111]),
];
const COLUMN_1_BACKDROP: [u8; 3] = [64, 64, 64];
const COLUMN_1_BASE: [u8; 3] = [235, 38, 201];

fn assert_column_1(mode: i32) {
    for (m, member, want) in COLUMN_1 {
        if m == mode {
            assert_clip_base(mode, COLUMN_1_BACKDROP, COLUMN_1_BASE, member, want);
        }
    }
}

/// A styled layer at Fill `percent` over the lattice, within 1 code of the
/// code rule on the source folded toward 128.
fn assert_fill(mode: i32, percent: i64) {
    let dir = TempDir::new().unwrap();
    let doc = lattice(&dir, mode, 255);
    let doc = set_style(
        doc,
        1,
        &format!(r#"{{"fill_opacity":{}}}"#, percent as f64 / 100.0),
    );
    let what = format!("{} at Fill {percent} %", name(mode));
    let bytes = rendered(doc, N, &what);
    let f = fill_code(percent);
    assert_lattice(
        &bytes,
        |b, s| f64::from(code_rule(mode, b, folded(s, f))),
        1.0,
        &what,
    );
}

// ------------------------------------------------------------- the tests --

#[test]
fn vivid_light_blends_every_code_pair_by_the_code_rule() {
    assert_code_lattice(BLEND_VIVID_LIGHT);
}

#[test]
fn linear_light_blends_every_code_pair_by_the_code_rule() {
    assert_code_lattice(BLEND_LINEAR_LIGHT);
}

/// A guard: on code inputs Divide's float form differs from the code rule
/// only on f32 half-code ties.
#[test]
fn divide_blends_every_code_pair_by_the_code_rule() {
    assert_code_lattice(BLEND_DIVIDE);
}

#[test]
fn a_vivid_light_clip_base_blends_its_groups_code() {
    assert_column_1(BLEND_VIVID_LIGHT);
}

#[test]
fn a_linear_light_clip_base_blends_its_groups_code() {
    assert_column_1(BLEND_LINEAR_LIGHT);
}

#[test]
fn a_divide_clip_base_blends_its_groups_code() {
    assert_column_1(BLEND_DIVIDE);
}

/// Divide's widest gaps in the topology: the group's float source 21.46 and
/// 34.55 divide to 238 and 244, their codes 21 and 35 to 243 and 240.
#[test]
fn a_grey_divide_clip_base_divides_by_the_groups_code() {
    assert_clip_base(BLEND_DIVIDE, [20; 3], [0; 3], [57; 3], [243; 3]);
    assert_clip_base(BLEND_DIVIDE, [33; 3], [21; 3], [57; 3], [240; 3]);
}

#[test]
fn linear_light_at_fill_folds_toward_code_128() {
    assert_fill(BLEND_LINEAR_LIGHT, 60);
    assert_fill(BLEND_LINEAR_LIGHT, 25);
}

/// The float fold at ½ is up to 3 codes off this at Fill 60 %.
#[test]
fn vivid_light_at_fill_folds_toward_code_128() {
    assert_fill(BLEND_VIVID_LIGHT, 60);
    assert_fill(BLEND_VIVID_LIGHT, 25);
}

/// A guard: at source alpha 96 the float forms are within 1 code too.
#[test]
fn each_mode_at_alpha_96_lands_within_one_code_of_the_code_rule() {
    let dir = TempDir::new().unwrap();
    for mode in MODES {
        let what = format!("{} lattice at alpha 96", name(mode));
        let bytes = rendered(lattice(&dir, mode, 96), N, &what);
        assert_lattice(
            &bytes,
            |b, s| f64::from(at_alpha(b, code_rule(mode, b, s), 96)),
            1.0,
            &what,
        );
    }
}

/// A `linear_light` byte document blends its decoded values by the float
/// forms. Divide's bound is 2: near black the document's decode departs from
/// the analytic curve by a few percent, and the quotient of two such values
/// (backdrop 2 over source 3, say) carries that into the result.
#[test]
fn a_linear_light_byte_document_keeps_the_float_forms() {
    let dir = TempDir::new().unwrap();
    for mode in MODES {
        let tol = if mode == BLEND_DIVIDE { 2.0 } else { 1.0 };
        let doc = lattice(&dir, mode, 255);
        let mut err: *mut c_char = ptr::null_mut();
        let linear = unsafe { dr_doc_set_evaluation(doc, 1, &mut err) };
        assert!(!linear.is_null(), "linear switch: {}", take_err_string(err));
        unsafe { dr_doc_free(doc) };
        let what = format!("linear-light {} lattice", name(mode));
        let bytes = rendered(linear, N, &what);
        assert_lattice(
            &bytes,
            |b, s| encode(float_rule(mode, decode(b), decode(s))),
            tol,
            &what,
        );
    }
}

/// A 16-bit document blends by the float forms: the same lattice × 257,
/// within 1 of 65535 of the f64 form (the code rule is up to 514 away).
#[test]
fn a_16_bit_document_keeps_the_float_forms() {
    let image16 = |f: &dyn Fn(u32, u32) -> u16| {
        let px = (0..N * N)
            .map(|i| {
                let v = f(i % N, i / N) * 257;
                [v, v, v, 65535]
            })
            .collect();
        PrecisionImage::from_rgba16(N, N, px, IccProfile::srgb(), None).unwrap()
    };
    for mode in MODES {
        let doc = PrecisionDocument::from_parts(
            N,
            N,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![
                Layer::raster("B", image16(&|x, _| x as u16), (0, 0), 1.0, true).unwrap(),
                Layer::raster("S", image16(&|_, y| y as u16), (0, 0), 1.0, true)
                    .unwrap()
                    .with_blend_mode(blend_mode(mode))
                    .unwrap(),
            ],
        )
        .unwrap();
        let render = doc
            .render(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap();
        let mut misses = Vec::new();
        for y in 0..N {
            for x in 0..N {
                let want = float_rule(mode, f64::from(x) / 255.0, f64::from(y) / 255.0) * 65535.0;
                let got = render[(y * N + x) as usize];
                for &channel in &got[..3] {
                    if (f64::from(channel) - want).abs() > 1.0 && misses.len() < 6 {
                        misses.push((x, y, channel, want));
                    }
                }
            }
        }
        assert!(
            misses.is_empty(),
            "16-bit {} lattice: (b, s, got, want): {misses:?}",
            name(mode)
        );
    }
}
