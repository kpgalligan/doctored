//! #2: Darker Color and Lighter Color in an 8-bit RGB document compare the
//! two triples' 30/59/11 lumas as CODES — each component rounded to its
//! 8-bit code, the luma summed in 2^15 fixed point with the weights 9830 /
//! 19334 / 3604 and rounded half up (`+ 16384 >> 15`) — and keep the
//! backdrop when the two codes tie. A recorded take
//! (`psd_appearance_578_take.py`, take1 and take2) measured that rule on the
//! two pairs below, where a float luma breaks the tie the other way: pair 1,
//! backdrop (237, 203, 0) under source (191, 191, 191), lumas 190.87 and 191,
//! both code 191 (Lighter Color's float compare picks the source); pair 2,
//! backdrop (1, 217, 209) under source (100, 179, 138), lumas 151.32 and
//! 150.79, both code 151 (Darker Color's float compare picks the source).
//!
//! The oracle is the rule above, written here on the codes. Every topology
//! case is opaque, so the composite IS the pick, and asserts it on the
//! full-frame flatten (`dr_doc_flattened`) and that the regional render
//! (`dr_doc_render_region` over the canvas at divisor 1) equals the full
//! frame byte for byte: a plain layer, a clipped member, a clip base, a
//! styled clip base, a moded group, an adjustment layer, a styled layer, a
//! styled adjustment layer (which the regional render refuses today, so its
//! equality is asserted whenever it is admitted) and a shallow knockout
//! inside a group. A pair the
//! codes do not tie still picks by luma; a 16-bit document keeps the float
//! compare (the same pairs × 257 pick the source), and a `linear_light` byte
//! document keeps the W3C compare on its linear values.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::{dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width};
use doctored_core::ffi_doc::*;
use doctored_core::ffi_evaluation::dr_doc_set_evaluation;
use doctored_core::ffi_group::dr_doc_group_layers;
use doctored_core::ffi_regional::dr_doc_render_region;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common::{
    add_layer, adjust_meta, apply, apply_or_keep, doc_from, set_meta, set_style, take_err_string,
    BLEND_DARKER_COLOR, BLEND_LIGHTER_COLOR, BLEND_PASS_THROUGH,
};

const W: u32 = 24;
const H: u32 = 8;
/// Every fixture is uniform over its shape, and the shape is `SHAPE`'s
/// columns where one is narrower than the canvas: this one interior pixel is
/// the whole answer.
const PROBE: (u32, u32) = (7, 4);
/// The columns of a shaped layer (styled layers, a clip member's left part).
const SHAPE: (u32, u32) = (2, 10);
/// A member clipped to a base covers only these columns, leaving `PROBE` on
/// the base alone.
const MEMBER: (u32, u32) = (2, 5);
/// An outside effect whose pixels land 12 columns away from the shape, so
/// none reaches `PROBE` (a drop shadow of size 0 is the shape moved).
const SHADOW: &str = r#"{"effects":[{"type":"drop_shadow","angle":0,"use_global_light":false,"distance":12,"size":0}]}"#;
/// A colour no pick returns: what a clip member and a knockout's sibling hold.
const OTHER: [u8; 3] = [30, 90, 200];

/// A `(backdrop, source)` pair of opaque codes.
type Pair = ([u8; 3], [u8; 3]);

/// The take's two tie pairs.
const PAIR_1: Pair = ([237, 203, 0], [191, 191, 191]);
const PAIR_2: Pair = ([1, 217, 209], [100, 179, 138]);
/// Each mode with the pair whose float compare picks differently from the
/// code compare.
const TIES: [(i32, Pair); 2] = [(BLEND_LIGHTER_COLOR, PAIR_1), (BLEND_DARKER_COLOR, PAIR_2)];

// ---------------------------------------------------------- the oracle --

/// The 30/59/11 luma of 8-bit codes in 2^15 fixed point, rounded half up.
fn lum8(c: [u8; 3]) -> u32 {
    (9830 * u32::from(c[0]) + 19334 * u32::from(c[1]) + 3604 * u32::from(c[2]) + 16384) >> 15
}

/// The 8-bit pick: the source when its luma code is strictly lower (Darker
/// Color) or higher (Lighter Color), else the backdrop.
fn pick8(mode: i32, cb: [u8; 3], cs: [u8; 3]) -> [u8; 3] {
    let source = match mode {
        BLEND_DARKER_COLOR => lum8(cs) < lum8(cb),
        BLEND_LIGHTER_COLOR => lum8(cs) > lum8(cb),
        other => panic!("mode {other} is not a pick"),
    };
    if source {
        cs
    } else {
        cb
    }
}

/// The analytic sRGB decode, for the `linear_light` case.
fn decode(code: u8) -> f64 {
    let x = f64::from(code) / 255.0;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// The W3C pick on the decoded linear values (0.3 / 0.59 / 0.11 in f64).
fn pick_linear(mode: i32, cb: [u8; 3], cs: [u8; 3]) -> [u8; 3] {
    let lum = |c: [u8; 3]| 0.3 * decode(c[0]) + 0.59 * decode(c[1]) + 0.11 * decode(c[2]);
    let source = match mode {
        BLEND_DARKER_COLOR => lum(cs) < lum(cb),
        BLEND_LIGHTER_COLOR => lum(cs) > lum(cb),
        other => panic!("mode {other} is not a pick"),
    };
    if source {
        cs
    } else {
        cb
    }
}

fn name(mode: i32) -> &'static str {
    if mode == BLEND_DARKER_COLOR {
        "Darker Color"
    } else {
        "Lighter Color"
    }
}

// -------------------------------------------------------- the fixtures --

fn opaque(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

/// A canvas-sized opaque `c`.
fn full(c: [u8; 3]) -> RgbaImage {
    RgbaImage::from_pixel(W, H, Rgba(opaque(c)))
}

/// A canvas-sized layer opaque `c` on `columns`, transparent elsewhere.
fn columns(c: [u8; 3], columns: (u32, u32)) -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, _| {
        if x >= columns.0 && x < columns.1 {
            Rgba(opaque(c))
        } else {
            Rgba([0; 4])
        }
    })
}

fn set_blend(doc: *mut DrDocument, idx: usize, mode: i32) -> *mut DrDocument {
    apply_or_keep(doc, |d| unsafe {
        dr_doc_with_layer_blend_mode(d, idx, mode)
    })
}

fn set_clipped(doc: *mut DrDocument, idx: usize) -> *mut DrDocument {
    apply(doc, |d| unsafe { dr_doc_with_layer_clipped(d, idx, true) })
}

/// `dr_doc_group_layers` over `idx`, answering the group's index.
fn group(doc: *mut DrDocument, idx: &[usize]) -> (*mut DrDocument, usize) {
    let cname = CString::new("G").expect("no interior NUL");
    let cap = unsafe { dr_doc_layer_count(doc) } + 1;
    let mut group = usize::MAX;
    let mut cleared = vec![0usize; cap];
    let mut cleared_len = 0usize;
    let mut reordered = vec![0usize; cap];
    let mut reordered_len = 0usize;
    let out = unsafe {
        dr_doc_group_layers(
            doc,
            idx.as_ptr(),
            idx.len(),
            cname.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            cap,
        )
    };
    assert!(!out.is_null(), "group_layers({idx:?}) refused");
    unsafe { dr_doc_free(doc) };
    (out, group)
}

/// A per-channel curve in its map form that sends every code to `c`: the
/// adjustment whose output on any backdrop is `c` exactly.
fn constant_curves(c: [u8; 3]) -> String {
    let map = |v: u8| format!("[{}]", vec![v.to_string(); 256].join(","));
    adjust_meta(
        "curves",
        &format!(
            r#"{{"r_map":{},"g_map":{},"b_map":{}}}"#,
            map(c[0]),
            map(c[1]),
            map(c[2])
        ),
    )
}

fn image_bytes(out: *mut doctored_core::DrImage) -> Vec<u8> {
    let (w, h) = unsafe { (dr_image_width(out), dr_image_height(out)) };
    let pixels = unsafe { dr_image_pixels_rgba(out) };
    let bytes = unsafe { std::slice::from_raw_parts(pixels, (w * h * 4) as usize) }.to_vec();
    unsafe { dr_image_free(out) };
    bytes
}

/// The full-frame flatten.
fn flatten(doc: *const DrDocument) -> Vec<u8> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_flattened(doc, u64::MAX, ptr::null(), &mut err) };
    assert!(!out.is_null(), "flattened: {}", take_err_string(err));
    image_bytes(out)
}

/// The regional compositor over the whole canvas at divisor 1, or its
/// refusal.
fn region(doc: *const DrDocument) -> Result<Vec<u8>, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_render_region(doc, 0, 0, W, H, 1, u64::MAX, ptr::null(), &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    Ok(image_bytes(out))
}

fn at(bytes: &[u8], (x, y): (u32, u32)) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
}

/// The pick at `PROBE` on the flatten, and the regional render equal to the
/// flatten; frees `doc`.
fn assert_pick(doc: *mut DrDocument, want: [u8; 3], what: &str) {
    let full = flatten(doc);
    let regional = region(doc);
    unsafe { dr_doc_free(doc) };
    assert_eq!(at(&full, PROBE), opaque(want), "{what}: the flatten's pick");
    let regional = regional.unwrap_or_else(|e| panic!("{what}: render_region: {e}"));
    assert!(
        full == regional,
        "{what}: the regional render differs from the full frame"
    );
}

/// [`assert_pick`] for a document the regional render refuses today (a
/// styled adjustment layer): the flatten's pick, and the regional render
/// equal to it whenever it admits the document.
fn assert_flat_pick(doc: *mut DrDocument, want: [u8; 3], what: &str) {
    let full = flatten(doc);
    let regional = region(doc);
    unsafe { dr_doc_free(doc) };
    assert_eq!(at(&full, PROBE), opaque(want), "{what}: the flatten's pick");
    if let Ok(regional) = regional {
        assert!(
            full == regional,
            "{what}: the regional render differs from the full frame"
        );
    }
}

/// `[backdrop, top]`: the backdrop canvas-sized, `top` above it.
fn two(dir: &TempDir, cb: [u8; 3], top: &RgbaImage) -> *mut DrDocument {
    let doc = doc_from(dir, "back.png", &full(cb));
    add_layer(dir, "top.png", doc, 0, top, "Top")
}

// ------------------------------------------------------------- the tests --

#[test]
fn a_plain_layer_picks_by_the_luma_codes_and_keeps_the_backdrop_on_a_tie() {
    let dir = TempDir::new().unwrap();
    for mode in [BLEND_LIGHTER_COLOR, BLEND_DARKER_COLOR] {
        for (cb, cs) in [PAIR_1, PAIR_2] {
            let doc = set_blend(two(&dir, cb, &full(cs)), 1, mode);
            assert_pick(
                doc,
                pick8(mode, cb, cs),
                &format!("{} {cb:?} {cs:?}", name(mode)),
            );
        }
    }
}

#[test]
fn a_clipped_member_picks_by_the_luma_codes_over_its_base() {
    let dir = TempDir::new().unwrap();
    for (mode, (cb, cs)) in TIES {
        // [base cb, member cs clipped in the mode].
        let doc = set_clipped(set_blend(two(&dir, cb, &full(cs)), 1, mode), 1);
        assert_pick(doc, cb, &format!("clipped member {}", name(mode)));
    }
}

#[test]
fn a_clip_base_carrying_the_mode_picks_by_the_luma_codes() {
    let dir = TempDir::new().unwrap();
    for (mode, (cb, cs)) in TIES {
        // [backdrop cb, base cs in the mode, a Normal member clipped to it on
        // `MEMBER`'s columns]: `PROBE` reads the base alone.
        let doc = set_blend(two(&dir, cb, &full(cs)), 1, mode);
        let doc = add_layer(
            &dir,
            "member.png",
            doc,
            1,
            &columns(OTHER, MEMBER),
            "Member",
        );
        let doc = set_clipped(doc, 2);
        assert_pick(doc, cb, &format!("clip base {}", name(mode)));
    }
}

#[test]
fn a_styled_clip_base_carrying_the_mode_picks_by_the_luma_codes() {
    let dir = TempDir::new().unwrap();
    for (mode, (cb, cs)) in TIES {
        let doc = set_blend(two(&dir, cb, &columns(cs, SHAPE)), 1, mode);
        let doc = set_style(doc, 1, SHADOW);
        let doc = add_layer(
            &dir,
            "member.png",
            doc,
            1,
            &columns(OTHER, MEMBER),
            "Member",
        );
        let doc = set_clipped(doc, 2);
        assert_pick(doc, cb, &format!("styled clip base {}", name(mode)));
    }
}

#[test]
fn a_group_carrying_the_mode_picks_by_the_luma_codes() {
    let dir = TempDir::new().unwrap();
    for (mode, (cb, cs)) in TIES {
        // An isolated group in the mode; its one Normal child is the source.
        let (doc, g) = group(two(&dir, cb, &full(cs)), &[1]);
        let doc = set_blend(doc, g, mode);
        assert_pick(doc, cb, &format!("group {}", name(mode)));
    }
}

#[test]
fn an_adjustment_layer_carrying_the_mode_picks_by_the_luma_codes() {
    let dir = TempDir::new().unwrap();
    for (mode, (cb, cs)) in TIES {
        let adjusted = |dir: &TempDir| set_meta(two(dir, cb, &full(cs)), 1, &constant_curves(cs));
        // Under Normal the adjustment's output on the backdrop is the source.
        assert_pick(
            adjusted(&dir),
            cs,
            &format!("adjustment Normal {}", name(mode)),
        );
        let doc = set_blend(adjusted(&dir), 1, mode);
        assert_pick(doc, cb, &format!("adjustment layer {}", name(mode)));
    }
}

#[test]
fn a_styled_layer_carrying_the_mode_picks_by_the_luma_codes_inside_its_shape() {
    let dir = TempDir::new().unwrap();
    for (mode, (cb, cs)) in TIES {
        let doc = set_blend(two(&dir, cb, &columns(cs, SHAPE)), 1, mode);
        let doc = set_style(doc, 1, SHADOW);
        assert_pick(doc, cb, &format!("styled layer {}", name(mode)));
    }
}

#[test]
fn a_styled_adjustment_layer_carrying_the_mode_picks_by_the_luma_codes() {
    let dir = TempDir::new().unwrap();
    for (mode, (cb, cs)) in TIES {
        let styled = |dir: &TempDir| {
            let doc = set_meta(two(dir, cb, &columns(cs, SHAPE)), 1, &constant_curves(cs));
            set_style(doc, 1, SHADOW)
        };
        assert_flat_pick(
            styled(&dir),
            cs,
            &format!("styled adjustment Normal {}", name(mode)),
        );
        let doc = set_blend(styled(&dir), 1, mode);
        assert_flat_pick(doc, cb, &format!("styled adjustment {}", name(mode)));
    }
}

#[test]
fn a_shallow_knockout_in_a_group_picks_by_the_luma_codes_against_the_group_backdrop() {
    let dir = TempDir::new().unwrap();
    for (mode, (cb, cs)) in TIES {
        // [backdrop cb, G(pass through){ sibling OTHER, knockout cs in the
        // mode }]: Shallow punches through the sibling to the group's entry
        // state, cb, and the unit composites onto it in the mode.
        let doc = doc_from(&dir, "back.png", &full(cb));
        let doc = add_layer(&dir, "other.png", doc, 0, &full(OTHER), "Other");
        let doc = add_layer(&dir, "top.png", doc, 1, &full(cs), "Top");
        let doc = set_blend(doc, 2, mode);
        let doc = set_style(doc, 2, r#"{"knockout":"shallow"}"#);
        let (doc, g) = group(doc, &[1, 2]);
        let doc = set_blend(doc, g, BLEND_PASS_THROUGH);
        assert_pick(doc, cb, &format!("knockout {}", name(mode)));
    }
}

#[test]
fn a_pair_the_codes_do_not_tie_picks_by_luma() {
    let dir = TempDir::new().unwrap();
    let (dark, light) = ([64, 64, 64], [191, 191, 191]);
    for (cb, cs) in [(dark, light), (light, dark)] {
        for mode in [BLEND_LIGHTER_COLOR, BLEND_DARKER_COLOR] {
            let want = pick8(mode, cb, cs);
            let doc = set_blend(two(&dir, cb, &full(cs)), 1, mode);
            assert_pick(doc, want, &format!("{} {cb:?} {cs:?}", name(mode)));
        }
    }
    // The oracle's own sanity: the dark one wins Darker, the light one Lighter.
    assert_eq!(pick8(BLEND_DARKER_COLOR, light, dark), dark);
    assert_eq!(pick8(BLEND_LIGHTER_COLOR, dark, light), light);
}

/// The same pairs promoted to 16 bits (× 257) keep the float compare: the
/// source wins both ties.
#[test]
fn a_16_bit_document_keeps_the_float_compare() {
    let image16 = |c: [u8; 3]| {
        let px = vec![opaque(c).map(|v| u16::from(v) * 257); (W * H) as usize];
        PrecisionImage::from_rgba16(W, H, px, IccProfile::srgb(), None).unwrap()
    };
    for (mode, (cb, cs)) in [
        (BlendMode::LighterColor, PAIR_1),
        (BlendMode::DarkerColor, PAIR_2),
    ] {
        let doc = PrecisionDocument::from_parts(
            W,
            H,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![
                Layer::raster("B", image16(cb), (0, 0), 1.0, true).unwrap(),
                Layer::raster("S", image16(cs), (0, 0), 1.0, true)
                    .unwrap()
                    .with_blend_mode(mode)
                    .unwrap(),
            ],
        )
        .unwrap();
        let render = doc
            .render(REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap();
        let got = render[(PROBE.1 * W + PROBE.0) as usize];
        assert_eq!(
            got,
            opaque(cs).map(|v| u16::from(v) * 257),
            "16-bit {mode:?}"
        );
    }
}

/// A `linear_light` byte document compares its LINEAR values in float: on
/// this pair the linear lumas are 0.0059 apart (the source the lighter),
/// while the linear values rounded to codes tie and the encoded codes favour
/// the backdrop, so only the master picks the source.
#[test]
fn a_linear_light_byte_document_keeps_the_w3c_compare() {
    let dir = TempDir::new().unwrap();
    let (a, b) = ([151, 72, 250], [224, 44, 51]);
    for (mode, cb, cs) in [
        (BLEND_LIGHTER_COLOR, a, b),
        (BLEND_DARKER_COLOR, b, a),
        (BLEND_LIGHTER_COLOR, PAIR_1.0, PAIR_1.1),
        (BLEND_DARKER_COLOR, PAIR_2.0, PAIR_2.1),
    ] {
        let doc = set_blend(two(&dir, cb, &full(cs)), 1, mode);
        let mut err: *mut c_char = ptr::null_mut();
        let linear = unsafe { dr_doc_set_evaluation(doc, 1, &mut err) };
        assert!(!linear.is_null(), "linear switch: {}", take_err_string(err));
        unsafe { dr_doc_free(doc) };
        let want = pick_linear(mode, cb, cs);
        let got = at(&flatten(linear), PROBE);
        unsafe { dr_doc_free(linear) };
        for c in 0..3 {
            let delta = (i32::from(got[c]) - i32::from(want[c])).abs();
            assert!(
                delta <= 1,
                "linear {} {cb:?} {cs:?}: got {got:?}, want {want:?}",
                name(mode)
            );
        }
    }
    assert_eq!(
        pick_linear(BLEND_LIGHTER_COLOR, a, b),
        b,
        "the source is lighter in linear"
    );
    assert_eq!(
        pick8(BLEND_LIGHTER_COLOR, a, b),
        a,
        "the codes favour the backdrop"
    );
}
