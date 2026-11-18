//! #2's CL3: a clipping base below 100 % Fill pays its Fill ONCE, on the
//! whole unit. The unit is built as at Fill 1 — the base's pixels at full
//! strength, each visible member onto them by CL2 (`clip_unit`: the colour
//! becomes `lerp(Cb, B(Cb, Cs), sa)` and the alpha stays the footprint `F`)
//! — and the finished unit lands on its backdrop through the base's mode at
//! `F · W`, `W` the base's Fill and Opacity as one weight
//! (`opacity_byte::joint`: the two bytes' product rounded to a byte below 32
//! bits, the exact product in a float scene). A recorded take measured it
//! (`clip_base_fill_2_take.py` take1, in the private archive). The oracle is
//! written here in f64 from that statement and the W3C blend table, never
//! read off the implementation, and run through the FFI at 8, 16 and 32 bits,
//! on CMYK (`cmyk_structure_fixture`'s oracle), and through both regional
//! renderers. A unit with no visible member and an adjustment base (direct
//! or routed) keep today's render.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_precision::*;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_regional::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::cmyk_structure_fixture as cmyk;
use crate::common::{img_dims, img_pixels, joint_unit, styled, take_err_string};
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

// ---------------------------------------------------------- the oracle --

/// One separable W3C blend function `B(cb, cs)`.
fn blend(mode: BlendMode, cb: f64, cs: f64) -> f64 {
    match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        // Hard Light with the operands swapped.
        BlendMode::Overlay if cb <= 0.5 => 2.0 * cb * cs,
        BlendMode::Overlay => {
            let b2 = 2.0 * cb - 1.0;
            cs + b2 - cs * b2
        }
        other => panic!("no oracle for {other:?}"),
    }
}

/// The W3C source-over step: straight `cs` at `sa` in `mode` onto `dst`.
fn over(dst: [f64; 4], cs: [f64; 3], sa: f64, mode: BlendMode) -> [f64; 4] {
    let ab = dst[3];
    let ao = sa + ab * (1.0 - sa);
    if ao <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0, 0.0, 0.0, ao];
    for c in 0..3 {
        let b = blend(mode, dst[c], cs[c]);
        out[c] = (sa * (1.0 - ab) * cs[c] + sa * ab * b + (1.0 - sa) * ab * dst[c]) / ao;
    }
    out
}

/// One clipped member: straight colour, alpha × opacity, mode.
type Member = ([f64; 3], f64, BlendMode);

/// CL3's unit at a base pixel of colour `base` and footprint `f`: each
/// member's frozen lerp over the base colour, the alpha `f` (no Fill in it).
fn unit(base: [f64; 3], f: f64, members: &[Member]) -> [f64; 4] {
    if f <= 0.0 {
        return [0.0; 4];
    }
    let mut c = base;
    for &(cs, sa, mode) in members {
        for k in 0..3 {
            c[k] += (blend(mode, c[k], cs[k]) - c[k]) * sa;
        }
    }
    [c[0], c[1], c[2], f]
}

/// `W`: the base's Fill and Opacity as one weight (module doc).
fn weight(fill: f64, opacity: f64, float: bool) -> f64 {
    if float {
        fill * opacity
    } else {
        joint_unit(fill, opacity)
    }
}

/// The finished unit onto the opaque `back` through `mode` at `F · W`.
fn landed(back: [f64; 3], u: [f64; 4], w: f64, mode: BlendMode) -> [f64; 4] {
    over(
        [back[0], back[1], back[2], 1.0],
        [u[0], u[1], u[2]],
        u[3] * w,
        mode,
    )
}

fn unit8(c: [u8; 3]) -> [f64; 3] {
    c.map(|v| f64::from(v) / 255.0)
}

fn assert_near(got: &[f64], want: &[f64], tolerance: f64, what: &str) {
    for (k, (g, w)) in got.iter().zip(want).enumerate().take(3) {
        assert!(
            (g - w).abs() <= tolerance,
            "{what} channel {k}: {got:?} vs {want:?} (tolerance {tolerance})"
        );
    }
}

// --------------------------------------------------------- the fixtures --

const BACKDROP: [u8; 3] = [200, 120, 40];
const BASE: [u8; 3] = [30, 160, 220];
const ALPHAS: [u8; 3] = [255, 160, 64];
const M1: ([u8; 3], u8) = ([240, 60, 100], 96);
const M2: ([u8; 3], u8) = ([90, 200, 30], 160);

/// A two-member unit over the base: the base's mode, the members' modes,
/// the base's Fill and Opacity.
#[derive(Clone, Copy, Debug)]
struct Case {
    base: BlendMode,
    pair: (BlendMode, BlendMode),
    fill: f32,
    opacity: f32,
}

/// Fill 25 / 50 / 0 %, two member pairs, a Multiply base and a base at
/// Opacity 0.6.
fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    for fill in [0.25, 0.5, 0.0] {
        for pair in [
            (BlendMode::Normal, BlendMode::Multiply),
            (BlendMode::Screen, BlendMode::Overlay),
        ] {
            out.push(Case {
                base: BlendMode::Normal,
                pair,
                fill,
                opacity: 1.0,
            });
        }
        out.push(Case {
            base: BlendMode::Multiply,
            pair: (BlendMode::Normal, BlendMode::Multiply),
            fill,
            opacity: 1.0,
        });
        out.push(Case {
            base: BlendMode::Normal,
            pair: (BlendMode::Normal, BlendMode::Multiply),
            fill,
            opacity: 0.6,
        });
    }
    out
}

fn members(pair: (BlendMode, BlendMode)) -> [Member; 2] {
    [
        (unit8(M1.0), f64::from(M1.1) / 255.0, pair.0),
        (unit8(M2.0), f64::from(M2.1) / 255.0, pair.1),
    ]
}

/// The oracle's pixel at base alpha `a`; below 32 bits the Fill and the
/// Opacity are read as their bytes.
fn want(case: &Case, a: u8, float: bool) -> [f64; 4] {
    let (fill, opacity) = (f64::from(case.fill), f64::from(case.opacity));
    landed(
        unit8(BACKDROP),
        unit(unit8(BASE), f64::from(a) / 255.0, &members(case.pair)),
        weight(fill, opacity, float),
        case.base,
    )
}

fn solid(w: u32, rgb: [u8; 3], alphas: &[u8]) -> RgbaImage {
    RgbaImage::from_fn(w, 1, |x, _| {
        Rgba([rgb[0], rgb[1], rgb[2], alphas[x as usize % alphas.len()]])
    })
}

/// `doc` with layer `idx` in `mode` (the setter answers `None` for no change).
fn moded(doc: DrDocument, idx: usize, mode: BlendMode) -> DrDocument {
    if mode == BlendMode::Normal {
        doc
    } else {
        doc.with_layer_blend_mode(idx, mode).unwrap()
    }
}

/// Byte: backdrop, base (alpha per column), two clipped members, no style.
fn byte_plain(case: &Case) -> DrDocument {
    let w = ALPHAS.len() as u32;
    let mut doc = DrDocument::from_pixels(solid(w, BACKDROP, &[255]))
        .adding_image_layer(0, solid(w, BASE, &ALPHAS), "Base")
        .unwrap();
    for (i, (rgb, a)) in [M1, M2].into_iter().enumerate() {
        doc = doc
            .adding_image_layer(1 + i, solid(w, rgb, &[a]), "M")
            .unwrap()
            .with_layer_clipped(2 + i, true)
            .unwrap();
    }
    let mut doc = moded(
        moded(moded(doc, 1, case.base), 2, case.pair.0),
        3,
        case.pair.1,
    );
    if case.opacity < 1.0 {
        doc = doc.with_layer_opacity(1, case.opacity).unwrap();
    }
    doc
}

/// [`byte_plain`] with the base's Fill (through the FFI).
fn byte_doc(case: &Case) -> DrDocument {
    styled(
        &byte_plain(case),
        1,
        &format!(r#"{{"fill_opacity":{}}}"#, case.fill),
    )
}

/// `dr_doc_flattened`'s pixels.
fn byte_pixels(doc: &DrDocument) -> Vec<[f64; 4]> {
    let h = Box::into_raw(Box::new(doc.clone()));
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_flattened(h, 0, ptr::null(), &mut err) };
    assert!(!img.is_null(), "flattened: {}", take_err_string(err));
    let px = img_pixels(img);
    unsafe {
        dr_image_free(img);
        dr_doc_free(h);
    }
    px.chunks(4)
        .map(|p| [p[0], p[1], p[2], p[3]].map(f64::from))
        .collect()
}

fn image16(w: u32, rgb: [u8; 3], alphas: &[u8]) -> PrecisionImage {
    let px: Vec<[u16; 4]> = (0..w as usize)
        .map(|x| [rgb[0], rgb[1], rgb[2], alphas[x % alphas.len()]].map(|v| u16::from(v) * 257))
        .collect();
    PrecisionImage::from_rgba16(w, 1, px, IccProfile::srgb(), None).unwrap()
}

fn raster16(name: &str, rgb: [u8; 3], alphas: &[u8], opacity: f32) -> Layer {
    Layer::raster(
        name,
        image16(ALPHAS.len() as u32, rgb, alphas),
        (0, 0),
        opacity,
        true,
    )
    .unwrap()
}

fn document16(w: u32, h: u32, layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let request = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        let text = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        panic!("edit: {text}");
    }
    assert!(!out.is_null(), "the edit changes the document");
    *unsafe { Box::from_raw(out) }
}

fn with_style(doc: &PrecisionDocument, layer: usize, style: Value) -> PrecisionDocument {
    edit(
        doc,
        json!({"op":"layer_style","version":1,"layer":layer,"style":style}),
    )
}

/// 16 bits: backdrop, base, two clipped members, the base's Fill.
fn typed_plain(case: &Case) -> PrecisionDocument {
    let m = |name: &str, (rgb, a): ([u8; 3], u8), mode: BlendMode| {
        raster16(name, rgb, &[a], 1.0)
            .with_blend_mode(mode)
            .unwrap()
            .with_clipped(true)
    };
    document16(
        ALPHAS.len() as u32,
        1,
        vec![
            raster16("B", BACKDROP, &[255], 1.0),
            raster16("Base", BASE, &ALPHAS, case.opacity)
                .with_blend_mode(case.base)
                .unwrap(),
            m("M1", M1, case.pair.0),
            m("M2", M2, case.pair.1),
        ],
    )
}

fn typed_doc(case: &Case) -> PrecisionDocument {
    with_style(&typed_plain(case), 1, json!({"fill_opacity": case.fill}))
}

/// `dr_precision_doc_render` as 16-bit codes.
fn typed_pixels(doc: &PrecisionDocument) -> Vec<[f64; 4]> {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "render: {}", take_err_string(err));
    rgba16(image)
}

/// A typed image's 16-bit samples, the image freed.
fn rgba16(image: *mut PrecisionImage) -> Vec<[f64; 4]> {
    let (w, h) = unsafe {
        (
            dr_precision_image_width(image),
            dr_precision_image_height(image),
        )
    };
    let p = unsafe { dr_precision_image_pixels_rgba16(image) };
    assert!(!p.is_null(), "a 16-bit image");
    let px: Vec<[f64; 4]> = unsafe { std::slice::from_raw_parts(p, (w * h * 4) as usize) }
        .chunks(4)
        .map(|q| [q[0], q[1], q[2], q[3]].map(f64::from))
        .collect();
    unsafe { dr_precision_image_free(image) };
    px
}

fn codes(px: [f64; 4], top: f64) -> [f64; 4] {
    px.map(|v| v * top)
}

// ------------------------------------------------------------- the tests --

#[test]
fn byte_units_pay_their_fill_once() {
    for case in cases() {
        let got = byte_pixels(&byte_doc(&case));
        for (x, &a) in ALPHAS.iter().enumerate() {
            assert_near(
                &got[x],
                &codes(want(&case, a, false), 255.0),
                1.0,
                &format!("8-bit {case:?} base alpha {a}"),
            );
        }
    }
}

#[test]
fn typed_units_pay_their_fill_once() {
    for case in cases() {
        let got = typed_pixels(&typed_doc(&case));
        for (x, &a) in ALPHAS.iter().enumerate() {
            assert_near(
                &got[x],
                &codes(want(&case, a, false), 65535.0),
                1.0,
                &format!("16-bit {case:?} base alpha {a}"),
            );
        }
    }
}

#[test]
fn float_units_pay_their_exact_fill_once() {
    // Float documents composite Normal and Multiply among the oracle's
    // modes; Fill 0.3 is no byte (77 / 255 = 0.302), so the exact Fill is
    // what the float scene lands at.
    let profile = IccProfile::srgb();
    let px = |rgb: [u8; 3], a: u8| -> [f32; 4] {
        [rgb[0], rgb[1], rgb[2], a].map(|v| f32::from(v) / 255.0)
    };
    for base in [BlendMode::Normal, BlendMode::Multiply] {
        for (fill, opacity) in [(0.3f32, 1.0f32), (0.3, 0.6), (0.0, 1.0)] {
            let case = Case {
                base,
                pair: (BlendMode::Normal, BlendMode::Multiply),
                fill,
                opacity,
            };
            let records = vec![
                ALPHAS.map(|_| px(BACKDROP, 255)).to_vec(),
                ALPHAS.map(|a| px(BASE, a)).to_vec(),
                ALPHAS.map(|_| px(M1.0, M1.1)).to_vec(),
                ALPHAS.map(|_| px(M2.0, M2.1)).to_vec(),
            ];
            let mut doc = float_doc(ALPHAS.len() as u32, 1, &profile, &records);
            for (layer, mode) in [(1usize, base), (2, case.pair.0), (3, case.pair.1)] {
                if layer > 1 {
                    doc = edit(
                        &doc,
                        json!({"op":"layer_clipped","layer":layer,"clipped":true}),
                    );
                }
                if mode != BlendMode::Normal {
                    doc = edit(
                        &doc,
                        json!({"op":"layer_blend","version":1,"layer":layer,"blend_mode":mode.to_c()}),
                    );
                }
            }
            doc = with_style(&doc, 1, json!({ "fill_opacity": fill }));
            if opacity < 1.0 {
                let mut err: *mut c_char = ptr::null_mut();
                let out = unsafe { dr_precision_doc_set_layer_opacity(&doc, 1, opacity, &mut err) };
                assert!(!out.is_null(), "opacity: {}", take_err_string(err));
                doc = *unsafe { Box::from_raw(out) };
            }
            let got: Vec<[f64; 4]> = doc
                .render(LIMIT, 0, None)
                .unwrap()
                .to_rgba32()
                .unwrap()
                .into_iter()
                .map(|p| p.map(f64::from))
                .collect();
            for (x, &a) in ALPHAS.iter().enumerate() {
                assert_near(
                    &got[x],
                    &want(&case, a, true),
                    1e-5,
                    &format!("float {case:?} base alpha {a}"),
                );
            }
        }
    }
}

#[test]
fn a_group_base_with_a_fill_style_pays_it_once() {
    let case = Case {
        base: BlendMode::Normal,
        pair: (BlendMode::Normal, BlendMode::Multiply),
        fill: 0.5,
        opacity: 1.0,
    };
    // Byte: the base inside a Normal group, the members clipped to the
    // group, the group at Fill 50 %. A new group is Pass Through.
    let (grouped, group, _, _) = byte_plain(&case).group_layers(&[1], "G").unwrap();
    let grouped = grouped
        .with_layer_blend_mode(group, BlendMode::Normal)
        .unwrap();
    assert_eq!(grouped.layers.iter().filter(|l| l.clipped).count(), 2);
    let got = byte_pixels(&styled(&grouped, group, r#"{"fill_opacity":0.5}"#));
    for (x, &a) in ALPHAS.iter().enumerate() {
        assert_near(
            &got[x],
            &codes(want(&case, a, false), 255.0),
            1.0,
            &format!("8-bit group base at Fill 50 %, base alpha {a}"),
        );
    }
    // 16 bits: postorder B 0, Base 1, G 2, M1 3, M2 4.
    let plain = typed_plain(&case);
    let layers = plain.root_layers().to_vec();
    let base_group = Layer::isolated_normal("G", vec![layers[1].clone()], 1.0, true).unwrap();
    let doc = document16(
        ALPHAS.len() as u32,
        1,
        vec![
            layers[0].clone(),
            base_group,
            layers[2].clone(),
            layers[3].clone(),
        ],
    );
    let got = typed_pixels(&with_style(&doc, 2, json!({"fill_opacity": 0.5})));
    for (x, &a) in ALPHAS.iter().enumerate() {
        assert_near(
            &got[x],
            &codes(want(&case, a, false), 65535.0),
            1.0,
            &format!("16-bit group base at Fill 50 %, base alpha {a}"),
        );
    }
}

#[test]
fn a_knockout_base_with_a_visible_member_pays_its_fill_once() {
    // Shallow at the root is Deep, which punches to the Background — here
    // the backdrop itself — so the routed unit lands where the direct one
    // would: `q = fill · min(1, U.a / s)` over the backdrop.
    let style = r#"{"fill_opacity":0.5,"knockout":"shallow"}"#;
    for pair in [
        (BlendMode::Normal, BlendMode::Multiply),
        (BlendMode::Screen, BlendMode::Overlay),
    ] {
        let case = Case {
            base: BlendMode::Normal,
            pair,
            fill: 0.5,
            opacity: 1.0,
        };
        let got = byte_pixels(&styled(&byte_plain(&case), 1, style));
        for (x, &a) in ALPHAS.iter().enumerate() {
            assert_near(
                &got[x],
                &codes(want(&case, a, false), 255.0),
                1.0,
                &format!("8-bit knockout base {pair:?} alpha {a}"),
            );
        }
        let doc = with_style(&typed_plain(&case), 1, serde_json::from_str(style).unwrap());
        let got = typed_pixels(&doc);
        for (x, &a) in ALPHAS.iter().enumerate() {
            assert_near(
                &got[x],
                &codes(want(&case, a, false), 65535.0),
                1.0,
                &format!("16-bit knockout base {pair:?} alpha {a}"),
            );
        }
    }
}

#[test]
fn a_unit_with_no_visible_member_keeps_its_base_at_its_fill() {
    // The base alone at its Fill: `F · fill` through its mode, as before. At
    // Opacity 1 that is the member-less twin's render byte for byte; below
    // it the unopened unit lands its Fill and its Opacity as two products,
    // a code from the twin's one joint weight, as it did before CL3.
    for case in cases() {
        let mut hidden = byte_doc(&case);
        hidden.layers[2].visible = false;
        hidden.layers[3].visible = false;
        let alone = styled(
            &moded(
                {
                    let mut d = DrDocument::from_pixels(solid(3, BACKDROP, &[255]))
                        .adding_image_layer(0, solid(3, BASE, &ALPHAS), "Base")
                        .unwrap();
                    if case.opacity < 1.0 {
                        d = d.with_layer_opacity(1, case.opacity).unwrap();
                    }
                    d
                },
                1,
                case.base,
            ),
            1,
            &format!(r#"{{"fill_opacity":{}}}"#, case.fill),
        );
        let got = byte_pixels(&hidden);
        if case.opacity >= 1.0 {
            assert_eq!(
                got,
                byte_pixels(&alone),
                "{case:?}: the hidden members change nothing"
            );
        }
        for (x, &a) in ALPHAS.iter().enumerate() {
            let fill = joint_unit(f64::from(case.fill), f64::from(case.opacity));
            let base = unit8(BASE);
            let want = over(
                [
                    unit8(BACKDROP)[0],
                    unit8(BACKDROP)[1],
                    unit8(BACKDROP)[2],
                    1.0,
                ],
                base,
                f64::from(a) / 255.0 * fill,
                case.base,
            );
            assert_near(
                &got[x],
                &codes(want, 255.0),
                1.0,
                &format!("{case:?} alpha {a}"),
            );
        }
        let layers = typed_doc(&case).root_layers().to_vec();
        let hidden_member = |(rgb, a): ([u8; 3], u8)| {
            Layer::raster("H", image16(3, rgb, &[a]), (0, 0), 1.0, false)
                .unwrap()
                .with_clipped(true)
        };
        let hidden16 = document16(
            3,
            1,
            vec![
                layers[0].clone(),
                layers[1].clone(),
                hidden_member(M1),
                hidden_member(M2),
            ],
        );
        let got16 = typed_pixels(&hidden16);
        if case.opacity >= 1.0 {
            let alone16 = typed_pixels(&document16(3, 1, layers[..2].to_vec()));
            assert_eq!(
                got16, alone16,
                "16-bit {case:?}: the hidden members change nothing"
            );
        }
        for (x, &a) in ALPHAS.iter().enumerate() {
            let fill = joint_unit(f64::from(case.fill), f64::from(case.opacity));
            let back = unit8(BACKDROP);
            let want = over(
                [back[0], back[1], back[2], 1.0],
                unit8(BASE),
                f64::from(a) / 255.0 * fill,
                case.base,
            );
            // Below Opacity 1 the two products sit within 0.001 of the
            // joint byte, under 64 codes at 16 bits.
            let tolerance = if case.opacity >= 1.0 { 1.0 } else { 64.0 };
            assert_near(
                &got16[x],
                &codes(want, 65535.0),
                tolerance,
                &format!("16-bit {case:?} alpha {a}"),
            );
        }
    }
}

/// The byte render of a Levels ADJUSTMENT base at Fill 50 % with a visible
/// Normal member, pinned from the build before CL3: an adjustment base puts
/// no pixels in the unit, so its Fill weighs the adjustment alone and the
/// rule leaves it where it was.
const ADJUSTMENT_BASE_PIN: [[u8; 4]; 3] =
    [[240, 60, 100, 255], [228, 87, 78, 255], [216, 115, 56, 255]];

/// Byte: the backdrop, a Levels adjustment (layer 1), a Normal member at
/// alphas 255 / 160 / 64 clipped to it, no style.
fn byte_adjustment_base() -> DrDocument {
    let mut doc = DrDocument::from_pixels(solid(3, BACKDROP, &[255]))
        .adding_image_layer(0, RgbaImage::new(1, 1), "levels")
        .unwrap();
    doc.layers[1].meta =
        Some(crate::common::adjust_meta("levels", "{\"black\":0.1,\"gamma\":1.6}").into());
    doc.adding_image_layer(1, solid(3, M1.0, &ALPHAS), "M")
        .unwrap()
        .with_layer_clipped(2, true)
        .unwrap()
}

fn byte_codes(doc: &DrDocument) -> Vec<[u8; 4]> {
    byte_pixels(doc)
        .into_iter()
        .map(|p| p.map(|v| v as u8))
        .collect()
}

#[test]
fn an_adjustment_base_keeps_its_render() {
    let doc = styled(&byte_adjustment_base(), 1, r#"{"fill_opacity":0.5}"#);
    assert_eq!(
        byte_codes(&doc),
        ADJUSTMENT_BASE_PIN.to_vec(),
        "the adjustment base's render"
    );
}

/// The same adjustment base ROUTED (a Shallow knockout, which at the root
/// punches to the backdrop) at Fill 50 %, pinned from the build before CL3 at
/// 8 and 16 bits and on CMYK: the unit pass takes the adjustment's frame,
/// which a visible member does not open, so the frame keeps its Fill and
/// lands at `min(1, U.a / s)` as before (at 8 bits, the direct render).
const ROUTED_ADJUSTMENT: &str = r#"{"fill_opacity":0.5,"knockout":"shallow"}"#;
const ROUTED_ADJUSTMENT_PIN: [[u8; 4]; 3] = ADJUSTMENT_BASE_PIN;
const ROUTED_ADJUSTMENT_PIN16: [[u16; 4]; 3] = [
    [61680, 15420, 25700, 65535],
    [58565, 22436, 20087, 65535],
    [55417, 29526, 14415, 65535],
];
const ROUTED_ADJUSTMENT_PIN_CMYK: [[u16; 5]; 3] = [
    [0, 65535, 65535, 0, 65535],
    [12347, 51615, 52285, 7518, 65535],
    [15905, 25654, 28798, 8869, 65535],
];

#[test]
fn a_routed_adjustment_base_keeps_its_render_at_8_bits() {
    let doc = styled(&byte_adjustment_base(), 1, ROUTED_ADJUSTMENT);
    assert_eq!(
        byte_codes(&doc),
        ROUTED_ADJUSTMENT_PIN.to_vec(),
        "the routed adjustment base's render"
    );
}

#[test]
fn a_routed_adjustment_base_keeps_its_render_at_16_bits() {
    let levels = json!({"type":"adjust","evaluation":"precision_v1","op":"levels",
        "params":{"black":0.1,"gamma":1.6}})
    .to_string();
    let doc = document16(
        3,
        1,
        vec![
            raster16("B", BACKDROP, &[255], 1.0),
            Layer::adjustment("levels", &levels, 1.0, true, &IccProfile::srgb()).unwrap(),
            raster16("M", M1.0, &ALPHAS, 1.0).with_clipped(true),
        ],
    );
    let doc = with_style(&doc, 1, serde_json::from_str(ROUTED_ADJUSTMENT).unwrap());
    let got: Vec<[u16; 4]> = typed_pixels(&doc)
        .into_iter()
        .map(|p| p.map(|v| v as u16))
        .collect();
    assert_eq!(
        got,
        ROUTED_ADJUSTMENT_PIN16.to_vec(),
        "the 16-bit routed adjustment base's render"
    );
}

#[test]
fn a_routed_adjustment_base_keeps_its_render_on_cmyk() {
    let mut bottom = cmyk::varied(3, 1, 1);
    for p in &mut bottom {
        p[3] = 65535;
    }
    let doc = cmyk::cmyk_layers(
        3,
        1,
        vec![cmyk::RgbLayer {
            name: "Bottom",
            w: 3,
            h: 1,
            offset: (0, 0),
            pixels: bottom,
        }],
    );
    let doc = cmyk::with_levels(doc, 1.0);
    let doc = cmyk::then(
        doc,
        r#"{"op":"add_layer","name":"M","anchor":1,"position":"above"}"#,
        &[],
    );
    let doc = cmyk::then(
        doc,
        r#"{"op":"paint_blend","layer":2,"ink":[0,1,1,0,1],"opacity":1}"#,
        &[65535, 40000, 12000],
    );
    let doc = cmyk::then(
        doc,
        r#"{"op":"layer_clipped","layer":2,"clipped":true}"#,
        &[],
    );
    let doc = cmyk::then(
        doc,
        &format!(r#"{{"op":"layer_style","version":1,"layer":1,"style":{ROUTED_ADJUSTMENT}}}"#),
        &[],
    );
    let got: Vec<[u16; 5]> = (0..3).map(|x| cmyk::composite(doc, x, 0)).collect();
    cmyk::free(doc);
    assert_eq!(
        got,
        ROUTED_ADJUSTMENT_PIN_CMYK.to_vec(),
        "the CMYK routed adjustment base's render"
    );
}

// ------------------------------------------------------------- regional --

const RW: u32 = 12;
const RH: u32 = 6;
const RBASE: (i32, i32, u32, u32) = (3, 1, 6, 4);
const RALPHAS: [u8; 6] = [255, 160, 64, 255, 160, 64];
/// Strictly inside the canvas, across the base's left edge.
const RWINDOW: (u32, u32, u32, u32) = (2, 1, 6, 3);

fn rback(x: u32, y: u32) -> [u8; 3] {
    [
        (40 + x * 17) as u8,
        (200 - y * 23) as u8,
        (90 + x * 5 + y * 9) as u8,
    ]
}

/// The regional oracle at canvas `(x, y)`: the backdrop outside the base,
/// the Fill 0.5 / Opacity 0.6 unit with its Multiply member inside it.
fn regional_want(x: u32, y: u32) -> [f64; 4] {
    let back = unit8(rback(x, y));
    let (bx, by, bw, bh) = RBASE;
    let inside =
        (bx as u32..bx as u32 + bw).contains(&x) && (by as u32..by as u32 + bh).contains(&y);
    if !inside {
        return [back[0], back[1], back[2], 1.0];
    }
    let a = RALPHAS[(x - bx as u32) as usize];
    let member = (unit8(M1.0), f64::from(M1.1) / 255.0, BlendMode::Multiply);
    landed(
        back,
        unit(unit8(BASE), f64::from(a) / 255.0, &[member]),
        weight(0.5, f64::from(0.6f32), false),
        BlendMode::Normal,
    )
}

fn regional_byte_doc() -> DrDocument {
    let (bx, by, bw, bh) = RBASE;
    let back = RgbaImage::from_fn(RW, RH, |x, y| {
        let c = rback(x, y);
        Rgba([c[0], c[1], c[2], 255])
    });
    let base = RgbaImage::from_fn(bw, bh, |x, _| {
        Rgba([BASE[0], BASE[1], BASE[2], RALPHAS[x as usize]])
    });
    let mut doc = DrDocument::from_pixels(back)
        .adding_image_layer(0, base, "Base")
        .unwrap()
        .with_layer_offset(1, bx, by)
        .unwrap()
        .with_layer_opacity(1, 0.6)
        .unwrap()
        .adding_image_layer(
            1,
            RgbaImage::from_pixel(RW, RH, Rgba([M1.0[0], M1.0[1], M1.0[2], M1.1])),
            "M",
        )
        .unwrap()
        .with_layer_clipped(2, true)
        .unwrap();
    doc = doc.with_layer_blend_mode(2, BlendMode::Multiply).unwrap();
    styled(&doc, 1, r#"{"fill_opacity":0.5}"#)
}

#[test]
fn the_byte_regional_render_pays_the_fill_once() {
    let hd = Box::into_raw(Box::new(regional_byte_doc()));
    let (x, y, w, h) = RWINDOW;
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_render_region(hd, x, y, w, h, 1, u64::MAX, ptr::null(), &mut err) };
    unsafe { dr_doc_free(hd) };
    assert!(!img.is_null(), "region: {}", take_err_string(err));
    assert_eq!(img_dims(img), (w, h));
    let px = img_pixels(img);
    unsafe { dr_image_free(img) };
    for j in 0..h {
        for i in 0..w {
            let at = ((j * w + i) * 4) as usize;
            let got = [px[at], px[at + 1], px[at + 2], px[at + 3]].map(f64::from);
            assert_near(
                &got,
                &codes(regional_want(x + i, y + j), 255.0),
                1.0,
                &format!("8-bit regional ({}, {})", x + i, y + j),
            );
        }
    }
}

#[test]
fn the_typed_regional_render_pays_the_fill_once() {
    let (bx, by, bw, bh) = RBASE;
    let back: Vec<[u16; 4]> = (0..RH)
        .flat_map(|y| (0..RW).map(move |x| (x, y)))
        .map(|(x, y)| {
            let c = rback(x, y);
            [c[0], c[1], c[2], 255].map(|v| u16::from(v) * 257)
        })
        .collect();
    let base: Vec<[u16; 4]> = (0..bh)
        .flat_map(|_| (0..bw).map(|x| [BASE[0], BASE[1], BASE[2], RALPHAS[x as usize]]))
        .map(|p| p.map(|v| u16::from(v) * 257))
        .collect();
    let member =
        vec![[M1.0[0], M1.0[1], M1.0[2], M1.1].map(|v| u16::from(v) * 257); (RW * RH) as usize];
    let image = |w, h, px| PrecisionImage::from_rgba16(w, h, px, IccProfile::srgb(), None).unwrap();
    let doc = document16(
        RW,
        RH,
        vec![
            Layer::raster("B", image(RW, RH, back), (0, 0), 1.0, true).unwrap(),
            Layer::raster("Base", image(bw, bh, base), (bx, by), 0.6, true).unwrap(),
            Layer::raster("M", image(RW, RH, member), (0, 0), 1.0, true)
                .unwrap()
                .with_blend_mode(BlendMode::Multiply)
                .unwrap()
                .with_clipped(true),
        ],
    );
    let doc = with_style(&doc, 1, json!({"fill_opacity": 0.5}));
    let (x, y, w, h) = RWINDOW;
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_precision_doc_render_region(&doc, x, y, w, h, 1, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!image.is_null(), "region: {}", take_err_string(err));
    let px = rgba16(image);
    assert_eq!(px.len(), (w * h) as usize);
    for j in 0..h {
        for i in 0..w {
            assert_near(
                &px[(j * w + i) as usize],
                &codes(regional_want(x + i, y + j), 65535.0),
                1.0,
                &format!("16-bit regional ({}, {})", x + i, y + j),
            );
        }
    }
}

// ----------------------------------------------------------------- CMYK --

/// A CMYK fill-only clip base (the stack's plain clip unit) at Fill 0.5,
/// then at Opacity 0.6 too, and a styled one (a drop shadow 8 px left of a
/// 4-px base, so the base's own pixels show the unit alone), against the
/// fixture's f64 oracle.
#[test]
fn cmyk_units_pay_their_fill_once() {
    let full = |name, seed| cmyk::RgbLayer {
        name,
        w: 9,
        h: 3,
        offset: (0, 0),
        pixels: cmyk::varied(9, 3, seed),
    };
    let mut bottom = cmyk::varied(9, 3, 1);
    for p in &mut bottom {
        p[3] = 65535;
    }
    let mut doc = cmyk::cmyk_layers(
        9,
        3,
        vec![
            cmyk::RgbLayer {
                pixels: bottom.clone(),
                ..full("Bottom", 1)
            },
            full("Base", 2),
            full("M", 3),
        ],
    );
    doc = cmyk::with_fill(doc, 1, 0.5);
    doc = cmyk::then(
        doc,
        r#"{"op":"layer_clipped","layer":2,"clipped":true}"#,
        &[],
    );
    doc = cmyk::with_mode(doc, 2, BlendMode::Multiply.to_c());
    cmyk::assert_composite(doc, 2, "CMYK Fill 0.5 base");
    doc = cmyk::with_opacity(doc, 1, 0.6);
    cmyk::assert_composite(doc, 2, "CMYK Fill 0.5 base at Opacity 0.6");
    cmyk::free(doc);
    // The styled base: 4 × 3 at (10, 0) on a 16-px canvas.
    let mut doc = cmyk::cmyk_layers(
        16,
        3,
        vec![
            cmyk::RgbLayer {
                name: "Bottom",
                w: 16,
                h: 3,
                offset: (0, 0),
                pixels: {
                    let mut p = cmyk::varied(16, 3, 4);
                    for q in &mut p {
                        q[3] = 65535;
                    }
                    p
                },
            },
            cmyk::RgbLayer {
                name: "Base",
                w: 4,
                h: 3,
                offset: (10, 0),
                pixels: cmyk::varied(4, 3, 5),
            },
            cmyk::RgbLayer {
                name: "M",
                w: 16,
                h: 3,
                offset: (0, 0),
                pixels: cmyk::varied(16, 3, 6),
            },
        ],
    );
    doc = cmyk::then(
        doc,
        r#"{"op":"layer_style","version":1,"layer":1,"style":{"fill_opacity":0.5,"effects":[{"type":"drop_shadow","angle":0,"use_global_light":false,"distance":8,"size":0}]}}"#,
        &[],
    );
    doc = cmyk::then(
        doc,
        r#"{"op":"layer_clipped","layer":2,"clipped":true}"#,
        &[],
    );
    doc = cmyk::with_mode(doc, 2, BlendMode::Multiply.to_c());
    let base_alpha = cmyk::varied(4, 3, 5);
    for y in 0..3 {
        for x in 10..14 {
            // A styled base's footprint is its planes' byte shape: exact on
            // an opaque or empty pixel, within 1 / 510 of a partial one
            // (under 128 codes at 16 bits).
            let a = base_alpha[(y * 4 + x - 10) as usize][3];
            let bound = if a == 0 || a == 65535 { 2 } else { 128 };
            cmyk::assert_flat(
                cmyk::composite(doc, x, y),
                cmyk::oracle(doc, i64::from(x), i64::from(y)),
                bound,
                &format!("CMYK styled base at ({x}, {y})"),
            );
        }
    }
    cmyk::free(doc);
}

// ---------------------------------------------------- the take's patches --

/// One take1 patch (`clip-base-fill-2/take1`, `cbf_rgb8`, Photoshop 26.10.0):
/// base mode, member mode, Fill %, Opacity %, backdrop, base colour and
/// alpha, member colour and alpha, Photoshop's centre and the rule before
/// CL3's answer.
type Patch = (
    BlendMode,
    BlendMode,
    u8,
    u8,
    [u8; 3],
    ([u8; 3], u8),
    ([u8; 3], u8),
    [u8; 3],
    [u8; 3],
);

const TAKE1_PATCHES: [Patch; 8] = [
    // f25_normal
    (
        BlendMode::Normal,
        BlendMode::Normal,
        25,
        100,
        [255, 255, 255],
        ([104, 138, 42], 160),
        ([64, 64, 64], 96),
        [229, 232, 223],
        [195, 198, 189],
    ),
    // f25_multiply
    (
        BlendMode::Normal,
        BlendMode::Multiply,
        25,
        100,
        [175, 213, 88],
        ([63, 159, 21], 64),
        ([191, 191, 191], 255),
        [167, 207, 83],
        [170, 203, 103],
    ),
    // f75_screen
    (
        BlendMode::Normal,
        BlendMode::Screen,
        75,
        100,
        [162, 130, 246],
        ([78, 33, 18], 160),
        ([167, 150, 61], 96),
        [143, 107, 149],
        [143, 109, 138],
    ),
    // f75_overlay
    (
        BlendMode::Normal,
        BlendMode::Overlay,
        75,
        100,
        [64, 64, 64],
        ([198, 64, 178], 255),
        ([5, 186, 0], 255),
        [123, 86, 92],
        [109, 117, 76],
    ),
    // f0_screen
    (
        BlendMode::Normal,
        BlendMode::Screen,
        0,
        100,
        [128, 128, 128],
        ([37, 27, 167], 255),
        ([167, 150, 61], 96),
        [128, 128, 128],
        [143, 136, 103],
    ),
    // op60_f50_normal
    (
        BlendMode::Normal,
        BlendMode::Normal,
        50,
        60,
        [175, 213, 88],
        ([63, 159, 21], 64),
        ([191, 191, 191], 96),
        [170, 210, 88],
        [171, 209, 91],
    ),
    // op60_f50_multiply
    (
        BlendMode::Normal,
        BlendMode::Multiply,
        50,
        60,
        [255, 255, 255],
        ([104, 138, 42], 160),
        ([13, 166, 119], 255),
        [208, 224, 211],
        [162, 207, 185],
    ),
    // bplain_multiply
    (
        BlendMode::Multiply,
        BlendMode::Normal,
        50,
        100,
        [162, 130, 246],
        ([78, 33, 18], 160),
        ([5, 186, 0], 96),
        [121, 104, 172],
        [102, 99, 143],
    ),
];

/// Photoshop's own take1 patches: the judge put CL3 within 1 code of every
/// 8-bit tile they come from (max 1, means at most 0.203), so Doctored is
/// pinned within 1 code of Photoshop there, where the rule before CL3 was up
/// to 46 codes off.
#[test]
fn take1_patches_follow_photoshop_within_their_judged_residual() {
    let one = |rgb: [u8; 3], a: u8| RgbaImage::from_pixel(1, 1, Rgba([rgb[0], rgb[1], rgb[2], a]));
    for (base_mode, member_mode, fill, opacity, back, base, member, photoshop, old) in TAKE1_PATCHES
    {
        let mut doc = DrDocument::from_pixels(one(back, 255))
            .adding_image_layer(0, one(base.0, base.1), "Base")
            .unwrap()
            .adding_image_layer(1, one(member.0, member.1), "M")
            .unwrap()
            .with_layer_clipped(2, true)
            .unwrap();
        doc = moded(moded(doc, 1, base_mode), 2, member_mode);
        if opacity < 100 {
            doc = doc
                .with_layer_opacity(1, f32::from(opacity) / 100.0)
                .unwrap();
        }
        let doc = styled(
            &doc,
            1,
            &format!(r#"{{"fill_opacity":{}}}"#, f32::from(fill) / 100.0),
        );
        let got = byte_pixels(&doc)[0];
        for k in 0..3 {
            assert!(
                (got[k] - f64::from(photoshop[k])).abs() <= 1.0,
                "{base_mode:?} base at Fill {fill} % / Opacity {opacity} %, {member_mode:?} \
                 member {member:?} over {base:?}: {got:?} vs Photoshop {photoshop:?} (the rule \
                 before CL3 gave {old:?})"
            );
        }
    }
}
