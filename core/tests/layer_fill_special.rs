//! #578's special Fill (`blend_fill`): Color Burn, Linear Burn, Color Dodge,
//! Linear Dodge (Add) and Vivid Light fold a Fill `f` below 1 into the blend —
//! `B(cb, n + f·(cs − n))`, `n` = 1 for the burns, 0 for the dodges, ½ for
//! Vivid Light — and composite it at the layer's alpha × opacity over the
//! backdrop, the Fill reappearing only as the source's coverage over
//! transparency (form Fb: `αo = sa·f·(1 − ab) + ab`, `co·αo = sa·f·(1 −
//! ab)·cs + sa·ab·B + (1 − sa)·ab·cb`). The oracle is written here in f64
//! from that statement and the separable blend formulas, and run through
//! `DrDocument` (8 bits, ≤ 1 code) and `PrecisionDocument` (16 bits, ≤ 2 /
//! 65535) on take1's own tile — its eight backdrop colours by its eight source
//! colours at alpha 255 and 128 — at Fill 60 % and 25 %, over the opaque
//! backdrop and inside a Normal group; Photoshop 26.10.0's take1 patches are
//! pinned. The byte tile is also driven through `dr_doc_*` alone. Every other
//! mode, and Fill 1, keep the plain Fill byte for byte, and the non-separable
//! modes and Dissolve below Fill 1 render at 8 and 16 bits without panicking.

use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_with_layer_blend_mode};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};
use tempfile::TempDir;

use crate::common::{add_layer, apply, doc_from, flat_pixels, set_style, BLEND_COLOR_BURN};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// take1's backdrop columns and source rows (`psd_appearance_578_take.py`).
const BACK: [[u8; 3]; 8] = [
    [0, 0, 0],
    [64, 64, 64],
    [128, 128, 128],
    [191, 191, 191],
    [255, 255, 255],
    [237, 203, 0],
    [85, 205, 113],
    [99, 107, 11],
];
const SRC: [[u8; 3]; 8] = [
    [0, 0, 0],
    [64, 64, 64],
    [191, 191, 191],
    [255, 255, 255],
    [26, 34, 17],
    [81, 26, 85],
    [80, 112, 181],
    [255, 41, 180],
];
const FIVE: [BlendMode; 5] = [
    BlendMode::ColorBurn,
    BlendMode::LinearBurn,
    BlendMode::ColorDodge,
    BlendMode::Addition,
    BlendMode::VividLight,
];
const FILLS: [f64; 2] = [0.6, 0.25];

// ---------------------------------------------------------- the oracle --

fn dodge(b: f64, s: f64) -> f64 {
    if b == 0.0 {
        0.0
    } else if s >= 1.0 {
        1.0
    } else {
        (b / (1.0 - s)).min(1.0)
    }
}

fn burn(b: f64, s: f64) -> f64 {
    if b >= 1.0 {
        1.0
    } else if s <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - b) / s).min(1.0)
    }
}

/// `B(cb, cs)` of the five modes.
fn blend(mode: BlendMode, b: f64, s: f64) -> f64 {
    match mode {
        BlendMode::ColorBurn => burn(b, s),
        BlendMode::LinearBurn => (b + s - 1.0).max(0.0),
        BlendMode::ColorDodge => dodge(b, s),
        BlendMode::Addition => (b + s).min(1.0),
        BlendMode::VividLight if s <= 0.0 => 0.0,
        BlendMode::VividLight if s >= 1.0 => 1.0,
        BlendMode::VividLight if s <= 0.5 => burn(b, 2.0 * s),
        BlendMode::VividLight => dodge(b, 2.0 * s - 1.0),
        other => panic!("no oracle for {other:?}"),
    }
}

fn neutral(mode: BlendMode) -> f64 {
    match mode {
        BlendMode::ColorBurn | BlendMode::LinearBurn => 1.0,
        BlendMode::VividLight => 0.5,
        _ => 0.0,
    }
}

/// Form Fb (module doc): `cs` at alpha `sa` (no Fill) and Fill `f` onto
/// `dst`.
fn special(dst: [f64; 4], cs: [f64; 3], sa: f64, f: f64, mode: BlendMode) -> [f64; 4] {
    let ab = dst[3];
    let ao = sa * f * (1.0 - ab) + ab;
    if ao <= 0.0 {
        return dst;
    }
    let n = neutral(mode);
    let mut out = [0.0, 0.0, 0.0, ao];
    for k in 0..3 {
        let b = blend(mode, dst[k], n + f * (cs[k] - n));
        out[k] = (sa * f * (1.0 - ab) * cs[k] + sa * ab * b + (1.0 - sa) * ab * dst[k]) / ao;
    }
    out
}

/// Normal source-over of a straight `cs` at `sa`.
fn normal(dst: [f64; 4], cs: [f64; 3], sa: f64) -> [f64; 4] {
    let ab = dst[3];
    let ao = sa + ab * (1.0 - sa);
    let mut out = [0.0, 0.0, 0.0, ao];
    for k in 0..3 {
        out[k] = (sa * cs[k] + (1.0 - sa) * ab * dst[k]) / ao;
    }
    out
}

fn unit(c: [u8; 3]) -> [f64; 3] {
    c.map(|v| f64::from(v) / 255.0)
}

fn alpha_of(row: usize) -> u8 {
    if row < 8 {
        255
    } else {
        128
    }
}

/// The patch `(row, column)` of the tile, as codes at `top` (255 or 65535);
/// `grouped`: the layer inside a Normal group, its buffer quantized at `top`.
fn oracle(mode: BlendMode, f: f64, row: usize, col: usize, grouped: bool, top: f64) -> [f64; 3] {
    // The byte Fill below 32 bits (#578, `layer_byte_opacity.rs`):
    // 60 % composites at 153 / 255, 25 % at 64 / 255.
    let f = (f * 255.0 + 0.5 + 1e-3).floor() / 255.0;
    let backdrop = unit(BACK[col]);
    let backdrop = [backdrop[0], backdrop[1], backdrop[2], 1.0];
    let cs = unit(SRC[row % 8]);
    let sa = f64::from(alpha_of(row)) / 255.0;
    let out = if grouped {
        let g = special([0.0; 4], cs, sa, f, mode).map(|v| (v * top).round() / top);
        normal(backdrop, [g[0], g[1], g[2]], g[3])
    } else {
        special(backdrop, cs, sa, f, mode)
    };
    [out[0] * top, out[1] * top, out[2] * top]
}

// --------------------------------------------------------- the fixtures --

fn backdrop8() -> RgbaImage {
    RgbaImage::from_fn(8, 16, |x, _| {
        let c = BACK[x as usize];
        Rgba([c[0], c[1], c[2], 255])
    })
}

fn source8() -> RgbaImage {
    RgbaImage::from_fn(8, 16, |_, y| {
        let c = SRC[y as usize % 8];
        Rgba([c[0], c[1], c[2], alpha_of(y as usize)])
    })
}

fn style(fill: f64) -> Arc<LayerStyle> {
    Arc::new(LayerStyle::from_json(&format!(r#"{{"fill_opacity":{fill}}}"#)).unwrap())
}

fn byte_doc(mode: BlendMode, fill: Option<f64>, opacity: f32) -> DrDocument {
    let mut doc = DrDocument::from_pixels(backdrop8())
        .adding_image_layer(0, source8(), "Layer")
        .unwrap();
    if mode != BlendMode::Normal {
        doc = doc.with_layer_blend_mode(1, mode).unwrap();
    }
    if opacity < 1.0 {
        doc = doc.with_layer_opacity(1, opacity).unwrap();
    }
    doc.layers[1].style = fill.map(style);
    doc
}

fn image16(img: &RgbaImage) -> PrecisionImage {
    let px = img
        .pixels()
        .map(|p| p.0.map(|v| u16::from(v) * 257))
        .collect();
    PrecisionImage::from_rgba16(img.width(), img.height(), px, IccProfile::srgb(), None).unwrap()
}

fn typed_doc(mode: BlendMode, fill: f64, grouped: bool) -> PrecisionDocument {
    let layer = Layer::raster("Layer", image16(&source8()), (0, 0), 1.0, true)
        .unwrap()
        .with_blend_mode(mode)
        .unwrap()
        .with_layer_style(Some(&*style(fill)), (8, 16))
        .unwrap();
    let top = if grouped {
        Layer::isolated_normal("G", vec![layer], 1.0, true).unwrap()
    } else {
        layer
    };
    PrecisionDocument::from_parts(
        8,
        16,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("B", image16(&backdrop8()), (0, 0), 1.0, true).unwrap(),
            top,
        ],
    )
    .unwrap()
}

fn assert_tile(
    got: &dyn Fn(usize, usize) -> [f64; 3],
    want: &dyn Fn(usize, usize) -> [f64; 3],
    tol: f64,
    what: &str,
) {
    for row in 0..16 {
        for col in 0..8 {
            let (g, w) = (got(row, col), want(row, col));
            for k in 0..3 {
                assert!(
                    (g[k] - w[k]).abs() <= tol,
                    "{what} patch ({row}, {col}) channel {k}: {g:?} vs {w:?}"
                );
            }
        }
    }
}

// ------------------------------------------------------------- the tests --

#[test]
fn byte_special_fill_follows_the_f1_fb_oracle_over_the_backdrop_and_in_a_group() {
    for mode in FIVE {
        for f in FILLS {
            let doc = byte_doc(mode, Some(f), 1.0);
            let flat = doc.flattened().unwrap();
            let px = |row: usize, col: usize| {
                let p = flat.get_pixel(col as u32, row as u32).0;
                [p[0], p[1], p[2]].map(f64::from)
            };
            assert_tile(
                &px,
                &|r, c| oracle(mode, f, r, c, false, 255.0),
                1.0,
                &format!("8-bit {mode:?} Fill {f}"),
            );
            // A new group is Pass Through; this one is an isolated Normal group.
            let (grouped, group, _, _) = doc.group_layers(&[1], "G").unwrap();
            let grouped = grouped
                .with_layer_blend_mode(group, BlendMode::Normal)
                .unwrap();
            let flat = grouped.flattened().unwrap();
            let px = |row: usize, col: usize| {
                let p = flat.get_pixel(col as u32, row as u32).0;
                [p[0], p[1], p[2]].map(f64::from)
            };
            assert_tile(
                &px,
                &|r, c| oracle(mode, f, r, c, true, 255.0),
                1.0,
                &format!("8-bit {mode:?} Fill {f} in a Normal group"),
            );
        }
    }
}

#[test]
fn typed_special_fill_follows_the_f1_fb_oracle_over_the_backdrop_and_in_a_group() {
    for mode in FIVE {
        for f in FILLS {
            for grouped in [false, true] {
                let render = typed_doc(mode, f, grouped)
                    .render(LIMIT, 0, None)
                    .unwrap()
                    .to_rgba16()
                    .unwrap();
                let px = |row: usize, col: usize| {
                    let p = render[row * 8 + col];
                    [p[0], p[1], p[2]].map(f64::from)
                };
                assert_tile(
                    &px,
                    &|r, c| oracle(mode, f, r, c, grouped, 65535.0),
                    2.0,
                    &format!("16-bit {mode:?} Fill {f} grouped {grouped}"),
                );
            }
        }
    }
}

#[test]
fn fill_1_and_every_other_mode_keep_the_plain_fill_byte_for_byte() {
    // A special mode at Fill 1 is the unstyled layer.
    for mode in FIVE {
        let styled = byte_doc(mode, Some(1.0), 0.6).flattened().unwrap();
        let plain = byte_doc(mode, None, 0.6).flattened().unwrap();
        assert_eq!(styled.as_raw(), plain.as_raw(), "{mode:?} at Fill 1");
    }
    // Every other mode: Fill 60 % is opacity 60 %.
    for mode in [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::LinearLight,
        BlendMode::HardMix,
        BlendMode::Difference,
    ] {
        let fill = byte_doc(mode, Some(0.6), 1.0).flattened().unwrap();
        let opacity = byte_doc(mode, None, 0.6).flattened().unwrap();
        assert_eq!(
            fill.as_raw(),
            opacity.as_raw(),
            "{mode:?}: Fill 60 % is opacity 60 %"
        );
    }
}

/// The 8-bit tile driven through the FFI alone (`dr_doc_*`, the host's path):
/// Color Burn set by `dr_doc_with_layer_blend_mode`, Fill 60 % by
/// `dr_doc_set_layer_style`, the flatten read back by `dr_doc_flattened`.
#[test]
fn the_ffi_byte_path_gives_the_f1_fb_oracle() {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "back.png", &backdrop8());
    let doc = add_layer(&dir, "src.png", doc, 0, &source8(), "Layer");
    let doc = apply(doc, |d| unsafe {
        dr_doc_with_layer_blend_mode(d, 1, BLEND_COLOR_BURN)
    });
    let doc = set_style(doc, 1, r#"{"fill_opacity":0.6}"#);
    let flat = flat_pixels(doc);
    unsafe { dr_doc_free(doc) };
    let px = |row: usize, col: usize| {
        let i = (row * 8 + col) * 4;
        [flat[i], flat[i + 1], flat[i + 2]].map(f64::from)
    };
    assert_tile(
        &px,
        &|r, c| oracle(BlendMode::ColorBurn, 0.6, r, c, false, 255.0),
        1.0,
        "FFI 8-bit ColorBurn Fill 0.6",
    );
}

/// The non-separable modes and Dissolve take no special Fill (`fill_blend`
/// answers `None` for them), so a Fill below 1 must render, never panic, at
/// both depths; the six non-separables keep the plain Fill byte for byte.
#[test]
fn non_separable_modes_and_dissolve_below_fill_1_render_without_panicking() {
    for mode in [
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
        BlendMode::DarkerColor,
        BlendMode::LighterColor,
        BlendMode::Dissolve,
    ] {
        let fill = byte_doc(mode, Some(0.6), 1.0)
            .flattened()
            .unwrap_or_else(|e| panic!("8-bit {mode:?} at Fill 60 % flattens: {e}"));
        assert!(
            typed_doc(mode, 0.6, false).render(LIMIT, 0, None).is_ok(),
            "16-bit {mode:?} at Fill 60 % renders"
        );
        if mode != BlendMode::Dissolve {
            let opacity = byte_doc(mode, None, 0.6).flattened().unwrap();
            assert_eq!(
                fill.as_raw(),
                opacity.as_raw(),
                "{mode:?}: Fill 60 % is opacity 60 %"
            );
        }
    }
}

/// Photoshop 26.10.0's take1 (`psd-appearance-578/take1`, `rgb8_fill`): the
/// patch, Photoshop's centre, and the plain Fill's answer there (what this
/// build replaced). Doctored lands within 1 code of Photoshop.
#[test]
fn the_special_fill_matches_photoshops_take1_patches() {
    type Pin = (BlendMode, f64, usize, usize, [u8; 3], [u8; 3]);
    let pins: [Pin; 9] = [
        (BlendMode::ColorBurn, 0.6, 0, 5, [210, 125, 0], [95, 81, 0]),
        (BlendMode::ColorBurn, 0.6, 4, 5, [216, 146, 0], [142, 81, 0]),
        (BlendMode::LinearBurn, 0.25, 4, 1, [7, 9, 4], [48, 48, 48]),
        (
            BlendMode::ColorDodge,
            0.6,
            3,
            7,
            [248, 255, 28],
            [193, 196, 157],
        ),
        (
            BlendMode::ColorDodge,
            0.6,
            11,
            7,
            [174, 181, 20],
            [146, 152, 84],
        ),
        (
            BlendMode::ColorDodge,
            0.25,
            3,
            7,
            [132, 143, 15],
            [138, 144, 72],
        ),
        (
            BlendMode::Addition,
            0.6,
            7,
            7,
            [252, 132, 119],
            [193, 132, 119],
        ),
        (
            BlendMode::Addition,
            0.25,
            7,
            3,
            [255, 201, 236],
            [207, 201, 207],
        ),
        (BlendMode::VividLight, 0.6, 3, 0, [0, 0, 0], [153, 153, 153]),
    ];
    for (mode, f, row, col, photoshop, plain) in pins {
        let flat = byte_doc(mode, Some(f), 1.0).flattened().unwrap();
        let got = flat.get_pixel(col as u32, row as u32).0;
        for k in 0..3 {
            assert!(
                got[k].abs_diff(photoshop[k]) <= 1,
                "{mode:?} Fill {f} patch ({row}, {col}): {got:?} vs Photoshop {photoshop:?} \
                 (the plain Fill gave {plain:?})"
            );
        }
    }
}
