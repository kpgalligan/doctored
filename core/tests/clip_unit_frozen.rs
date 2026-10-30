//! #2's clip rule (CL2, Photoshop's clipping, ruling 1): a clipped member
//! composites onto its base with the base's alpha FROZEN — over a base pixel
//! at alpha `F` the colour becomes `lerp(Cb, B(Cb, Cs), sa)` and the alpha
//! stays `F` (Fill 1); a base whose own pixels carry a Fill below 1 keeps
//! that Fill RELATIVE to the footprint (the members onto the base colour at
//! alpha `fill`, the result scaled back onto `F`). The oracle is written here
//! in f64 from that statement and the W3C blend table, never read off the
//! implementation, and run through `DrDocument` (8 bits) and
//! `PrecisionDocument` (16 bits and 32-bit float) at base alphas 128 and 64
//! under two members, a group base, a unit inside a Normal group, a
//! drop-shadow-styled base and a Fill 50 % base. An opaque base renders bit
//! for bit as before the rule changed (bytes pinned from the pre-change
//! build).

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

// ---------------------------------------------------------- the oracle --

/// One separable W3C blend function `B(cb, cs)`.
fn blend(mode: BlendMode, cb: f64, cs: f64) -> f64 {
    let hard_light = |b: f64, s: f64| {
        if s <= 0.5 {
            b * 2.0 * s
        } else {
            let s2 = 2.0 * s - 1.0;
            b + s2 - b * s2
        }
    };
    match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::Overlay => hard_light(cs, cb),
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

/// The clip unit's buffer (module doc): the base colour at footprint `f`
/// with its own Fill `fill`, each member onto it with the alpha frozen.
fn unit(base: [f64; 3], f: f64, fill: f64, members: &[Member]) -> [f64; 4] {
    if f <= 0.0 {
        return [0.0; 4];
    }
    if fill >= 1.0 {
        // `lerp(Cb, B(Cb, Cs), sa)`, the alpha kept at `f`.
        let mut c = base;
        for &(cs, sa, mode) in members {
            for k in 0..3 {
                c[k] += (blend(mode, c[k], cs[k]) - c[k]) * sa;
            }
        }
        return [c[0], c[1], c[2], f];
    }
    // The relative rule: the base colour at alpha `fill`, each member by
    // the W3C step, the result's alpha scaled onto the footprint.
    let mut px = [base[0], base[1], base[2], fill];
    for &(cs, sa, mode) in members {
        px = over(px, cs, sa, mode);
    }
    px[3] *= f;
    px
}

fn unit8(c: [u8; 3]) -> [f64; 3] {
    c.map(|v| f64::from(v) / 255.0)
}

fn codes(px: [f64; 4], top: f64) -> [f64; 4] {
    px.map(|v| v * top)
}

fn assert_near(got: &[f64], want: &[f64], tolerance: f64, what: &str) {
    for (k, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(
            (g - w).abs() <= tolerance,
            "{what} channel {k}: {got:?} vs {want:?} (tolerance {tolerance})"
        );
    }
}

// --------------------------------------------------------- the fixtures --

const BACKDROP: [u8; 3] = [200, 120, 40];
const BASE: [u8; 3] = [30, 160, 220];
const BASE_ALPHAS: [u8; 2] = [128, 64];
const M1: ([u8; 3], u8) = ([240, 60, 100], 96);
const M2: ([u8; 3], u8) = ([90, 200, 30], 160);
const PAIRS: [(BlendMode, BlendMode); 4] = [
    (BlendMode::Normal, BlendMode::Multiply),
    (BlendMode::Multiply, BlendMode::Normal),
    (BlendMode::Screen, BlendMode::Overlay),
    (BlendMode::Overlay, BlendMode::Screen),
];

fn members(m1: BlendMode, m2: BlendMode) -> [Member; 2] {
    [
        (unit8(M1.0), f64::from(M1.1) / 255.0, m1),
        (unit8(M2.0), f64::from(M2.1) / 255.0, m2),
    ]
}

/// The whole canvas: the opaque backdrop, the unit's buffer over it at
/// Normal (`group_quantum` > 0 quantizes the unit first, an isolated group's
/// buffer at that depth).
fn want(f: f64, fill: f64, pair: (BlendMode, BlendMode), group_quantum: f64) -> [f64; 4] {
    let mut buf = unit(unit8(BASE), f, fill, &members(pair.0, pair.1));
    if group_quantum > 0.0 {
        buf = buf.map(|v| (v * group_quantum).round() / group_quantum);
    }
    let backdrop = unit8(BACKDROP);
    over(
        [backdrop[0], backdrop[1], backdrop[2], 1.0],
        [buf[0], buf[1], buf[2]],
        buf[3],
        BlendMode::Normal,
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

/// Byte: backdrop, base (alpha per column), two clipped members.
fn byte_doc(pair: (BlendMode, BlendMode), fill: f32) -> DrDocument {
    let w = BASE_ALPHAS.len() as u32;
    let doc = DrDocument::from_pixels(solid(w, BACKDROP, &[255]))
        .adding_image_layer(0, solid(w, BASE, &BASE_ALPHAS), "Base")
        .unwrap()
        .adding_image_layer(1, solid(w, M1.0, &[M1.1]), "M1")
        .unwrap()
        .with_layer_clipped(2, true)
        .unwrap()
        .adding_image_layer(2, solid(w, M2.0, &[M2.1]), "M2")
        .unwrap()
        .with_layer_clipped(3, true)
        .unwrap();
    let mut doc = moded(moded(doc, 2, pair.0), 3, pair.1);
    if fill < 1.0 {
        doc.layers[1].style = Some(Arc::new(
            LayerStyle::from_json(&format!(r#"{{"fill_opacity":{fill}}}"#)).unwrap(),
        ));
    }
    doc
}

fn byte_pixels(doc: &DrDocument) -> Vec<[f64; 4]> {
    let flat = doc.flattened().unwrap();
    flat.pixels().map(|p| p.0.map(f64::from)).collect()
}

fn image16(w: u32, rgb: [u8; 3], alphas: &[u8]) -> PrecisionImage {
    let px: Vec<[u16; 4]> = (0..w as usize)
        .map(|x| {
            let a = alphas[x % alphas.len()];
            [rgb[0], rgb[1], rgb[2], a].map(|v| u16::from(v) * 257)
        })
        .collect();
    PrecisionImage::from_rgba16(w, 1, px, IccProfile::srgb(), None).unwrap()
}

fn raster16(name: &str, rgb: [u8; 3], alphas: &[u8]) -> Layer {
    Layer::raster(
        name,
        image16(BASE_ALPHAS.len() as u32, rgb, alphas),
        (0, 0),
        1.0,
        true,
    )
    .unwrap()
}

fn typed_members(pair: (BlendMode, BlendMode)) -> [Layer; 2] {
    [
        raster16("M1", M1.0, &[M1.1])
            .with_blend_mode(pair.0)
            .unwrap()
            .with_clipped(true),
        raster16("M2", M2.0, &[M2.1])
            .with_blend_mode(pair.1)
            .unwrap()
            .with_clipped(true),
    ]
}

fn document16(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        BASE_ALPHAS.len() as u32,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn typed_pixels(doc: &PrecisionDocument) -> Vec<[f64; 4]> {
    doc.render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
        .into_iter()
        .map(|p| p.map(f64::from))
        .collect()
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

// ------------------------------------------------------------- the tests --

#[test]
fn byte_members_land_on_a_semi_transparent_base_with_its_alpha_frozen() {
    for pair in PAIRS {
        let got = byte_pixels(&byte_doc(pair, 1.0));
        for (x, &a) in BASE_ALPHAS.iter().enumerate() {
            let want = codes(want(f64::from(a) / 255.0, 1.0, pair, 0.0), 255.0);
            assert_near(
                &got[x],
                &want,
                1.0,
                &format!("8-bit {pair:?} base alpha {a}"),
            );
        }
    }
}

#[test]
fn typed_members_land_on_a_semi_transparent_base_with_its_alpha_frozen() {
    for pair in PAIRS {
        let [m1, m2] = typed_members(pair);
        let doc = document16(vec![
            raster16("B", BACKDROP, &[255]),
            raster16("Base", BASE, &BASE_ALPHAS),
            m1,
            m2,
        ]);
        let got = typed_pixels(&doc);
        for (x, &a) in BASE_ALPHAS.iter().enumerate() {
            let want = codes(want(f64::from(a) / 255.0, 1.0, pair, 0.0), 65535.0);
            assert_near(
                &got[x],
                &want,
                1.0,
                &format!("16-bit {pair:?} base alpha {a}"),
            );
        }
    }
}

#[test]
fn float_members_land_on_a_semi_transparent_base_with_its_alpha_frozen() {
    // Float documents composite Normal and Multiply among these four.
    let profile = IccProfile::srgb();
    let px = |rgb: [u8; 3], a: u8| -> [f32; 4] {
        [rgb[0], rgb[1], rgb[2], a].map(|v| f32::from(v) / 255.0)
    };
    for pair in [PAIRS[0], PAIRS[1]] {
        let records = vec![
            BASE_ALPHAS.map(|_| px(BACKDROP, 255)).to_vec(),
            BASE_ALPHAS.map(|a| px(BASE, a)).to_vec(),
            BASE_ALPHAS.map(|_| px(M1.0, M1.1)).to_vec(),
            BASE_ALPHAS.map(|_| px(M2.0, M2.1)).to_vec(),
        ];
        let mut doc = float_doc(BASE_ALPHAS.len() as u32, 1, &profile, &records);
        for (layer, mode) in [(2usize, pair.0), (3, pair.1)] {
            doc = edit(
                &doc,
                json!({"op":"layer_clipped","layer":layer,"clipped":true}),
            );
            if mode != BlendMode::Normal {
                doc = edit(
                    &doc,
                    json!({"op":"layer_blend","version":1,"layer":layer,"blend_mode":mode.to_c()}),
                );
            }
        }
        let got: Vec<[f64; 4]> = doc
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba32()
            .unwrap()
            .into_iter()
            .map(|p| p.map(f64::from))
            .collect();
        for (x, &a) in BASE_ALPHAS.iter().enumerate() {
            let want = want(f64::from(a) / 255.0, 1.0, pair, 0.0);
            assert_near(
                &got[x],
                &want,
                1e-5,
                &format!("float {pair:?} base alpha {a}"),
            );
        }
    }
}

#[test]
fn a_group_base_and_a_unit_inside_a_normal_group_follow_the_same_rule() {
    let pair = PAIRS[0];
    // Byte: the base inside a Normal group, the members clipped to the group.
    let plain = byte_doc(pair, 1.0);
    // A new group is Pass Through; these are isolated Normal groups.
    let (grouped_base, group, _, _) = plain.group_layers(&[1], "G").unwrap();
    let grouped_base = grouped_base
        .with_layer_blend_mode(group, BlendMode::Normal)
        .unwrap();
    assert_eq!(
        grouped_base.layers.iter().filter(|l| l.clipped).count(),
        2,
        "the members stay clipped, now to the group"
    );
    let got = byte_pixels(&grouped_base);
    for (x, &a) in BASE_ALPHAS.iter().enumerate() {
        let want = codes(want(f64::from(a) / 255.0, 1.0, pair, 255.0), 255.0);
        assert_near(
            &got[x],
            &want,
            1.0,
            &format!("8-bit group base, base alpha {a}"),
        );
    }
    // Byte: the whole unit inside a Normal group (its buffer quantized).
    let (inside, group, _, _) = plain.group_layers(&[1, 2, 3], "G").unwrap();
    let inside = inside
        .with_layer_blend_mode(group, BlendMode::Normal)
        .unwrap();
    assert_eq!(inside.layers.iter().filter(|l| l.clipped).count(), 2);
    let got = byte_pixels(&inside);
    for (x, &a) in BASE_ALPHAS.iter().enumerate() {
        let want = codes(want(f64::from(a) / 255.0, 1.0, pair, 255.0), 255.0);
        assert_near(
            &got[x],
            &want,
            1.0,
            &format!("8-bit unit in a group, base alpha {a}"),
        );
    }
    // 16 bits: the same two structures.
    let [m1, m2] = typed_members(pair);
    let base_group =
        Layer::isolated_normal("G", vec![raster16("Base", BASE, &BASE_ALPHAS)], 1.0, true).unwrap();
    let doc = document16(vec![
        raster16("B", BACKDROP, &[255]),
        base_group,
        m1.clone(),
        m2.clone(),
    ]);
    let got = typed_pixels(&doc);
    for (x, &a) in BASE_ALPHAS.iter().enumerate() {
        let want = codes(want(f64::from(a) / 255.0, 1.0, pair, 65535.0), 65535.0);
        assert_near(
            &got[x],
            &want,
            1.0,
            &format!("16-bit group base, base alpha {a}"),
        );
    }
    let unit_group = Layer::isolated_normal(
        "G",
        vec![raster16("Base", BASE, &BASE_ALPHAS), m1, m2],
        1.0,
        true,
    )
    .unwrap();
    let doc = document16(vec![raster16("B", BACKDROP, &[255]), unit_group]);
    let got = typed_pixels(&doc);
    for (x, &a) in BASE_ALPHAS.iter().enumerate() {
        let want = codes(want(f64::from(a) / 255.0, 1.0, pair, 65535.0), 65535.0);
        assert_near(
            &got[x],
            &want,
            1.0,
            &format!("16-bit unit in a group, base alpha {a}"),
        );
    }
}

#[test]
fn a_fill_50_base_keeps_its_fill_relative_to_the_footprint() {
    // The base's Fill composites at its byte, 128 / 255 (#2).
    for pair in PAIRS {
        let got = byte_pixels(&byte_doc(pair, 0.5));
        for (x, &a) in BASE_ALPHAS.iter().enumerate() {
            let want = codes(
                want(
                    f64::from(a) / 255.0,
                    crate::common::byte_unit(0.5),
                    pair,
                    0.0,
                ),
                255.0,
            );
            assert_near(
                &got[x],
                &want,
                1.0,
                &format!("8-bit Fill 50 % {pair:?} alpha {a}"),
            );
        }
        let [m1, m2] = typed_members(pair);
        let doc = document16(vec![
            raster16("B", BACKDROP, &[255]),
            raster16("Base", BASE, &BASE_ALPHAS),
            m1,
            m2,
        ]);
        let doc = edit(
            &doc,
            json!({"op":"layer_style","version":1,"layer":1,"style":{"fill_opacity":0.5}}),
        );
        let got = typed_pixels(&doc);
        for (x, &a) in BASE_ALPHAS.iter().enumerate() {
            let want = codes(
                want(
                    f64::from(a) / 255.0,
                    crate::common::byte_unit(0.5),
                    pair,
                    0.0,
                ),
                65535.0,
            );
            assert_near(
                &got[x],
                &want,
                1.0,
                &format!("16-bit Fill 50 % {pair:?} alpha {a}"),
            );
        }
    }
}

#[test]
fn a_drop_shadow_styled_base_renders_its_interior_as_its_unstyled_twin() {
    // The base sits at x 10-11 of a 24-px canvas; its shadow lands 8 px
    // away, outside the base, so the base's own pixels show the unit alone.
    let pair = PAIRS[0];
    let canvas = |rgb: [u8; 3], alphas: &[u8]| {
        RgbaImage::from_fn(24, 1, |x, _| {
            if (10..12).contains(&x) {
                Rgba([rgb[0], rgb[1], rgb[2], alphas[(x - 10) as usize]])
            } else {
                Rgba([rgb[0], rgb[1], rgb[2], 0])
            }
        })
    };
    let plain = DrDocument::from_pixels(RgbaImage::from_pixel(
        24,
        1,
        Rgba([BACKDROP[0], BACKDROP[1], BACKDROP[2], 255]),
    ))
    .adding_image_layer(0, canvas(BASE, &BASE_ALPHAS), "Base")
    .unwrap()
    .adding_image_layer(1, solid(24, M1.0, &[M1.1]), "M1")
    .unwrap()
    .with_layer_clipped(2, true)
    .unwrap()
    .adding_image_layer(2, solid(24, M2.0, &[M2.1]), "M2")
    .unwrap()
    .with_layer_clipped(3, true)
    .unwrap();
    let plain = moded(moded(plain, 2, pair.0), 3, pair.1);
    let mut styled = plain.clone();
    styled.layers[1].style = Some(Arc::new(
        LayerStyle::from_json(
            r#"{"effects":[{"type":"drop_shadow","angle":0,"use_global_light":false,"distance":8,"size":0}]}"#,
        )
        .unwrap(),
    ));
    let a = plain.flattened().unwrap();
    let b = styled.flattened().unwrap();
    assert_ne!(a.as_raw(), b.as_raw(), "the shadow draws somewhere");
    for x in 10..12 {
        assert_eq!(
            b.get_pixel(x, 0).0,
            a.get_pixel(x, 0).0,
            "pixel {x}: the styled base's interior is its unstyled twin's"
        );
        let want = codes(
            want(
                f64::from(BASE_ALPHAS[(x - 10) as usize]) / 255.0,
                1.0,
                pair,
                0.0,
            ),
            255.0,
        );
        let got = a.get_pixel(x, 0).0.map(f64::from);
        assert_near(&got, &want, 1.0, &format!("pixel {x} against the oracle"));
    }
}

/// One opaque-base patch of the PRE-change build's own render
/// (`psd_appearance_578_take.py --build --round 2`, the Debug app before
/// `clip_unit` existed; c_member tiles, base alpha 255, member alpha 96):
/// mode, backdrop, base, member colour, the 8-bit and the 16-bit render.
type Pin = (BlendMode, [u8; 3], [u8; 3], [u8; 3], [u8; 3], [u16; 3]);

const OPAQUE_BASE_PINS: [Pin; 16] = [
    (
        BlendMode::Normal,
        [0, 0, 0],
        [1, 217, 209],
        [0, 0, 0],
        [1, 135, 130],
        [160, 34774, 33492],
    ),
    (
        BlendMode::Normal,
        [64, 64, 64],
        [235, 38, 201],
        [64, 64, 64],
        [171, 48, 149],
        [43850, 12282, 38402],
    ),
    (
        BlendMode::Normal,
        [128, 128, 128],
        [241, 105, 215],
        [20, 102, 107],
        [158, 104, 174],
        [40555, 26695, 44806],
    ),
    (
        BlendMode::Normal,
        [191, 191, 191],
        [213, 21, 45],
        [189, 69, 57],
        [204, 39, 50],
        [52419, 10041, 12726],
    ),
    (
        BlendMode::Multiply,
        [0, 0, 0],
        [1, 217, 209],
        [0, 0, 0],
        [1, 135, 130],
        [160, 34774, 33492],
    ),
    (
        BlendMode::Multiply,
        [64, 64, 64],
        [235, 38, 201],
        [64, 64, 64],
        [169, 27, 144],
        [43365, 7012, 37091],
    ),
    (
        BlendMode::Multiply,
        [128, 128, 128],
        [241, 105, 215],
        [20, 102, 107],
        [157, 81, 168],
        [40448, 20890, 43182],
    ),
    (
        BlendMode::Multiply,
        [191, 191, 191],
        [213, 21, 45],
        [189, 69, 57],
        [192, 15, 32],
        [49407, 3915, 8184],
    ),
    (
        BlendMode::Screen,
        [0, 0, 0],
        [1, 217, 209],
        [0, 0, 0],
        [1, 217, 209],
        [257, 55769, 53713],
    ),
    (
        BlendMode::Screen,
        [64, 64, 64],
        [235, 38, 201],
        [64, 64, 64],
        [237, 59, 206],
        [60881, 15035, 52968],
    ),
    (
        BlendMode::Screen,
        [128, 128, 128],
        [241, 105, 215],
        [20, 102, 107],
        [241, 128, 221],
        [62043, 32790, 56879],
    ),
    (
        BlendMode::Screen,
        [191, 191, 191],
        [213, 21, 45],
        [189, 69, 57],
        [225, 45, 63],
        [57753, 11523, 16107],
    ),
    (
        BlendMode::Overlay,
        [0, 0, 0],
        [1, 217, 209],
        [0, 0, 0],
        [1, 203, 192],
        [160, 52092, 49262],
    ),
    (
        BlendMode::Overlay,
        [64, 64, 64],
        [235, 38, 201],
        [64, 64, 64],
        [231, 31, 191],
        [59431, 7935, 49055],
    ),
    (
        BlendMode::Overlay,
        [128, 128, 128],
        [241, 105, 215],
        [20, 102, 107],
        [237, 97, 213],
        [60795, 24953, 54633],
    ),
    (
        BlendMode::Overlay,
        [191, 191, 191],
        [213, 21, 45],
        [189, 69, 57],
        [221, 17, 36],
        [56701, 4465, 9158],
    ),
];

#[test]
fn an_opaque_base_renders_bit_for_bit_as_before_the_rule_changed() {
    let one = |rgb: [u8; 3], a: u8| RgbaImage::from_pixel(1, 1, Rgba([rgb[0], rgb[1], rgb[2], a]));
    let one16 = |name: &str, rgb: [u8; 3], a: u8| {
        let px = vec![[rgb[0], rgb[1], rgb[2], a].map(|v| u16::from(v) * 257)];
        let image = PrecisionImage::from_rgba16(1, 1, px, IccProfile::srgb(), None).unwrap();
        Layer::raster(name, image, (0, 0), 1.0, true).unwrap()
    };
    for (mode, back, base, member, eight, sixteen) in OPAQUE_BASE_PINS {
        let mut doc = DrDocument::from_pixels(one(back, 255))
            .adding_image_layer(0, one(base, 255), "Base")
            .unwrap()
            .adding_image_layer(1, one(member, 96), "M")
            .unwrap()
            .with_layer_clipped(2, true)
            .unwrap();
        if mode != BlendMode::Normal {
            doc = doc.with_layer_blend_mode(2, mode).unwrap();
        }
        let got = doc.flattened().unwrap().get_pixel(0, 0).0;
        assert_eq!(
            [got[0], got[1], got[2]],
            eight,
            "8-bit {mode:?} over {back:?}"
        );
        let typed = PrecisionDocument::from_parts(
            1,
            1,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![
                one16("B", back, 255),
                one16("Base", base, 255),
                one16("M", member, 96)
                    .with_blend_mode(mode)
                    .unwrap()
                    .with_clipped(true),
            ],
        )
        .unwrap();
        let got = typed.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()[0];
        assert_eq!(
            [got[0], got[1], got[2]],
            sixteen,
            "16-bit {mode:?} over {back:?}"
        );
    }
}

/// Photoshop 26.10.0's own take2 patches (`psd-appearance-578/take2`,
/// `rgb8_clip`, c_member, base alpha 160, member alpha 96): mode, backdrop,
/// base, member colour, Photoshop's centre and the old rule's answer. The CL2
/// judge put these tiles within 1 code (max 1, means 0.38 and 0.25), so
/// Doctored is pinned within 1 code of Photoshop, where the old rule was up
/// to 28 codes off.
#[test]
fn take2_patches_follow_photoshop_within_their_judged_residual() {
    type Patch = (BlendMode, [u8; 3], [u8; 3], [u8; 3], [u8; 3], [u8; 3]);
    let patches: [Patch; 8] = [
        (
            BlendMode::Normal,
            [255, 255, 255],
            [53, 200, 200],
            [191, 191, 191],
            [161, 219, 219],
            [171, 218, 218],
        ),
        (
            BlendMode::Normal,
            [223, 5, 93],
            [128, 12, 136],
            [255, 255, 255],
            [194, 66, 148],
            [202, 84, 157],
        ),
        (
            BlendMode::Normal,
            [255, 255, 255],
            [53, 200, 200],
            [189, 69, 57],
            [160, 190, 187],
            [170, 180, 176],
        ),
        (
            BlendMode::Normal,
            [223, 5, 93],
            [128, 12, 136],
            [0, 0, 0],
            [133, 6, 88],
            [124, 6, 78],
        ),
        (
            BlendMode::Multiply,
            [255, 255, 255],
            [53, 200, 200],
            [255, 255, 255],
            [128, 220, 220],
            [151, 227, 227],
        ),
        (
            BlendMode::Multiply,
            [223, 5, 93],
            [128, 12, 136],
            [255, 255, 255],
            [163, 9, 120],
            [178, 37, 134],
        ),
        (
            BlendMode::Multiply,
            [255, 255, 255],
            [53, 200, 200],
            [158, 45, 133],
            [123, 182, 198],
            [136, 171, 194],
        ),
        (
            BlendMode::Multiply,
            [223, 5, 93],
            [128, 12, 136],
            [100, 179, 138],
            [145, 8, 106],
            [145, 28, 108],
        ),
    ];
    let one = |rgb: [u8; 3], a: u8| RgbaImage::from_pixel(1, 1, Rgba([rgb[0], rgb[1], rgb[2], a]));
    for (mode, back, base, member, photoshop, old) in patches {
        let doc = DrDocument::from_pixels(one(back, 255))
            .adding_image_layer(0, one(base, 160), "Base")
            .unwrap()
            .adding_image_layer(1, one(member, 96), "M")
            .unwrap()
            .with_layer_clipped(2, true)
            .unwrap();
        let got = moded(doc, 2, mode).flattened().unwrap().get_pixel(0, 0).0;
        for k in 0..3 {
            assert!(
                got[k].abs_diff(photoshop[k]) <= 1,
                "{mode:?} {member:?} over {base:?} at 160: {got:?} vs Photoshop {photoshop:?} \
                 (the old rule gave {old:?})"
            );
        }
    }
}
