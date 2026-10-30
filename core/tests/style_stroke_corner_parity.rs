//! #2: the Layer Style Outside, Center and Inside Stroke against
//! Photoshop's own renders, at 8 and 16 bits. The fixtures
//! (`tests/fixtures/psd_photoshop/stroke_corner_578/`, README) are the
//! flattened TIFFs of three Photoshop 26.10.0 takes:
//! `scripts/photoshop-probe/stroke_exposure_578_take.py` (take1),
//! `stroke_exposure_578_take2.py` (take2, `t2_*`) and
//! `stroke_composite_578_take.py` (take 3, `c1_*`): a red Normal stroke on
//! #30a050 sources over a 96 x 64 white backdrop.
//!
//! The bound was declared in each runner before its take: per case, max <= 2
//! codes and mean <= 0.5 over the pixels where either render is more than
//! 0.5 code from the same document without the stroke (16-bit samples as
//! `/ 257`). take1's disc and soft-edge rows ran over the take's unstyled
//! rect layer (take1's RECORDED harness defect), so they are rebuilt on that
//! stack here.
//!
//! A row below 100 % Fill is gated against a FIXED baseline: the layer at
//! its exact Fill, written in f64 (`exact_fill_plain`), not the core's
//! render without the stroke. The core's own baseline now composites the
//! Fill at its byte too, which took the interior pixels where Photoshop
//! (151) and the exact Fill (the 151.5 tie, 152) differed out of the gate:
//! take 3's 8-bit disc Center 3 at 50 % Fill went from 608 gated pixels to
//! 292 and its mean rose from 3.941 to 7.000 with no pixel worse. On the
//! fixed gate it keeps its 608 pixels and its mean fell to 3.362 (#2
//! controller ruling, 2026-10-17).
//!
//! Every Outside row meets it at 8 and 16 bits: the band is Photoshop's
//! (seeded from the anti-aliased edge) and it lands UNDER the layer as
//! Photoshop's disjoint union (`style_stroke_distance::band_under_layer`),
//! and the layer's Fill composites at its byte (`opacity_byte`:
//! 50 % → 128 / 255, 30 % → 77 / 255; its 16-bit interior reads 151.093
//! where 128 / 255 gives 151.094 and 0.5 gave 151.5), which was the 8-bit
//! partial-Fill rows' one-code miss. So do take 3's disc Center 6 (the inner
//! half measured from the anti-aliased edge, `inside_corner_band`) and its
//! rect Center 6. The anti-aliased Inside rows take NA1 too
//! (`inside_corner_band`, the Stroke confirm take's `Sn`): the disc Inside 3
//! meets at 8 and 16 bits (from 16 codes, 115 before the fourth Stroke take;
//! its band edge on the curved contour is NA1's), and the soft Inside 4 at
//! 50 % meets at 16 bits (`MET_16`) and keeps 1.0 / 0.82 at 8.
//! The rest assert their own measured residual, never a wider number: the
//! anti-aliased Center rows off the disc at 100 % and the disc Center 3 at
//! 50 % Fill keep a residual in the inside band's replacement
//! (`replace_under_band`), which no measured member fits, and the soft
//! Inside 4 at 50 % keeps the stroke opacity's 8-bit quantization.

use std::path::{Path, PathBuf};

use doctored_core::doc::DrDocument;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use image::RgbaImage;

use crate::common;

const W: u32 = 96;
const H: u32 = 64;
const RECT: (u32, u32, u32, u32) = (28, 20, 40, 24);
const SOURCE: [u8; 3] = [0x30, 0xa0, 0x50];
const MAX: f64 = 2.0;
const MEAN: f64 = 0.5;
const GATE: f64 = 0.5;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd_photoshop/stroke_corner_578")
}

/// `round(255 v)` of a unit value, half up — the take's source writer.
fn byte(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}

/// The takes' source alphas (the runners' `rect_plane`, `disc_plane`,
/// `soft_plane` and `half_plane`, restated): the hard rect; the r 12 disc at
/// (48, 32) as the 16 x 16-sample area; the rect with a 3 px linear edge; the
/// rect at alpha 128.
fn plane(kind: &str) -> Vec<u8> {
    let (x0, y0, rw, rh) = RECT;
    let mut out = Vec::with_capacity((W * H) as usize);
    for y in 0..H {
        for x in 0..W {
            out.push(match kind {
                "rect" | "half" => {
                    if x >= x0 && x < x0 + rw && y >= y0 && y < y0 + rh {
                        if kind == "rect" {
                            255
                        } else {
                            128
                        }
                    } else {
                        0
                    }
                }
                "disc" => {
                    let mut n = 0u32;
                    for j in 0..16 {
                        let sy = f64::from(y) + (f64::from(j) + 0.5) / 16.0 - 32.0;
                        for i in 0..16 {
                            let sx = f64::from(x) + (f64::from(i) + 0.5) / 16.0 - 48.0;
                            n += u32::from(sx * sx + sy * sy <= 144.0);
                        }
                    }
                    (255.0 * f64::from(n) / 256.0 + 0.5).floor() as u8
                }
                _ => {
                    let edge = |t: f64| (t / 3.0 + 0.5).clamp(0.0, 1.0);
                    let (fx, fy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                    byte(
                        edge(fx - f64::from(x0))
                            .min(edge(f64::from(x0 + rw) - fx))
                            .min(edge(fy - f64::from(y0)))
                            .min(edge(f64::from(y0 + rh) - fy)),
                    )
                }
            });
        }
    }
    out
}

/// The layers under and including the case's own, bottom first. take1:
/// `rect` alone, or (its stack) `rect` then `disc`, then `soft`. take2: the
/// case's own layer alone.
fn stack(take: u32, doc: &'static str) -> Vec<&'static str> {
    if take >= 2 {
        return vec![doc];
    }
    let order = ["rect", "disc", "soft"];
    let at = order
        .iter()
        .position(|d| *d == doc)
        .expect("a take document");
    order[..=at].to_vec()
}

/// The case's style: a red stroke at `opacity` (0..1) on a layer at `fill`
/// (0..1).
fn style(position: &str, size: u32, fill: f64, opacity: f64) -> String {
    format!(
        "{{\"fill_opacity\":{fill},\"effects\":[{{\"type\":\"stroke\",\"size\":{size},\"position\":\"{position}\",\"color\":\"#ff0000\",\"opacity\":{opacity}}}]}}"
    )
}

/// The 8-bit render, styled or not, as unit RGB.
fn render8(layers: &[&str], styled: Option<&str>) -> Vec<[f64; 3]> {
    let mut d = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, image::Rgba([255; 4])));
    for (i, kind) in layers.iter().enumerate() {
        let alpha = plane(kind);
        let pixels = RgbaImage::from_fn(W, H, |x, y| {
            let a = alpha[(y * W + x) as usize];
            image::Rgba([SOURCE[0], SOURCE[1], SOURCE[2], a])
        });
        d = d.adding_image_layer(i, pixels, kind).expect("add layer");
    }
    if let Some(json) = styled {
        d = common::styled(&d, layers.len(), json);
    }
    let flat = d.flattened().expect("flatten");
    flat.pixels()
        .map(|p| [0, 1, 2].map(|k| f64::from(p[k]) / 255.0))
        .collect()
}

/// The 16-bit render (sources and backdrop promoted by x 257), as unit RGB.
fn render16(kinds: &[&str], styled: Option<&str>) -> Vec<[f64; 3]> {
    let image = |pixels: Vec<[u16; 4]>| {
        PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).expect("image")
    };
    let mut layers = vec![Layer::raster(
        "white",
        image(vec![[65535; 4]; (W * H) as usize]),
        (0, 0),
        1.0,
        true,
    )
    .expect("backdrop")];
    for (i, kind) in kinds.iter().enumerate() {
        let pixels = plane(kind)
            .iter()
            .map(|&a| {
                let s = SOURCE.map(|v| u16::from(v) * 257);
                [s[0], s[1], s[2], u16::from(a) * 257]
            })
            .collect();
        let mut layer = Layer::raster(kind, image(pixels), (0, 0), 1.0, true).expect("layer");
        if i + 1 == kinds.len() {
            if let Some(json) = styled {
                let parsed = LayerStyle::from_json(json).expect("style");
                layer = layer
                    .with_layer_style(Some(&parsed), (W, H))
                    .expect("styled");
            }
        }
        layers.push(layer);
    }
    let d = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("document");
    d.render(REFERENCE_WORKING_BYTES, 0, None)
        .expect("render")
        .to_rgba16()
        .expect("rgba16")
        .into_iter()
        .map(|p| [0, 1, 2].map(|k| f64::from(p[k]) / 65535.0))
        .collect()
}

/// Photoshop's render of one case, as unit RGB.
fn photoshop(name: &str) -> Vec<[f64; 3]> {
    let img = image::open(fixtures().join(format!("{name}.png"))).expect("fixture");
    if name.ends_with("rgb16") {
        img.to_rgba16()
            .pixels()
            .map(|p| [0, 1, 2].map(|k| f64::from(p[k]) / 65535.0))
            .collect()
    } else {
        img.to_rgba8()
            .pixels()
            .map(|p| [0, 1, 2].map(|k| f64::from(p[k]) / 255.0))
            .collect()
    }
}

/// (max, mean, gated count) in 8-bit codes over the gated pixels (module doc).
fn residual(ours: &[[f64; 3]], plain: &[[f64; 3]], ps: &[[f64; 3]]) -> (f64, f64, usize) {
    let codes = |a: &[f64; 3], b: &[f64; 3]| {
        (0..3)
            .map(|k| (a[k] - b[k]).abs() * 255.0)
            .fold(0.0, f64::max)
    };
    let (mut worst, mut total, mut n) = (0.0f64, 0.0, 0usize);
    for ((o, p), r) in ours.iter().zip(plain).zip(ps) {
        if codes(o, p).max(codes(r, p)) <= GATE {
            continue;
        }
        let e = codes(o, r);
        worst = worst.max(e);
        total += e;
        n += 1;
    }
    (worst, if n == 0 { 0.0 } else { total / n as f64 }, n)
}

/// The fixed gate's baseline for a row below 100 % Fill (module doc): the
/// case's one layer over the white backdrop at its EXACT Fill, written in f64
/// and rounded half up to the depth. This is the document without the stroke
/// as the core rendered it before the byte Fill shipped, so the gated pixel
/// set does not move with the Fill rule under test.
fn exact_fill_plain(layers: &[&str], fill: f64, depth: u32) -> Vec<[f64; 3]> {
    assert_eq!(layers.len(), 1, "the partial-Fill rows stack one layer");
    let top = if depth == 8 { 255.0 } else { 65535.0 };
    plane(layers[0])
        .iter()
        .map(|&a| {
            let a = f64::from(a) / 255.0 * fill;
            SOURCE.map(|s| {
                let v = f64::from(s) / 255.0 * a + (1.0 - a);
                (v * top + 0.5).floor() / top
            })
        })
        .collect()
}

/// One take1 case at 100 % fill and stroke opacity.
fn case(doc: &'static str, position: &str, size: u32, depth: u32) -> (f64, f64) {
    measure(1, doc, position, size, "", 1.0, 1.0, depth)
}

/// One case of `take` (module doc); `tag` is the fixture name's suffix for
/// a fill or stroke opacity below 100 %.
#[allow(clippy::too_many_arguments)]
fn measure(
    take: u32,
    doc: &'static str,
    position: &str,
    size: u32,
    tag: &str,
    fill: f64,
    opacity: f64,
    depth: u32,
) -> (f64, f64) {
    let prefix = match take {
        1 => "",
        2 => "t2_",
        _ => "c1_",
    };
    let name = format!("{prefix}{doc}_{position}{size}{tag}_rgb{depth}");
    let layers = stack(take, doc);
    let render = if depth == 8 { render8 } else { render16 };
    let ours = render(&layers, Some(&style(position, size, fill, opacity)));
    // The document without the stroke: at 0 % fill the layer shows nothing,
    // so that is the backdrop alone; below 100 % it is the layer at its
    // exact Fill (the fixed gate, module doc).
    let plain = if fill == 0.0 {
        render(&[], None)
    } else if fill < 1.0 {
        exact_fill_plain(&layers, fill, depth)
    } else {
        render(&layers, None)
    };
    let (max, mean, n) = residual(&ours, &plain, &photoshop(&name));
    eprintln!("{name}: max {max:.3} mean {mean:.4} over {n} pixels");
    assert!(n > 0, "{name}: no pixel is gated");
    (max, mean)
}

#[test]
fn the_hard_rect_outside_strokes_meet_photoshop_at_8_and_16_bits() {
    for depth in [8, 16] {
        for size in [1, 2, 4, 7, 10] {
            let (max, mean) = case("rect", "outside", size, depth);
            assert!(
                max <= MAX && mean <= MEAN,
                "rect outside {size} px at {depth} bits: max {max:.3}, mean {mean:.4}"
            );
        }
    }
}

#[test]
fn the_hard_rect_center_strokes_meet_photoshop_at_8_and_16_bits() {
    for depth in [8, 16] {
        for size in [4, 5] {
            let (max, mean) = case("rect", "center", size, depth);
            assert!(
                max <= MAX && mean <= MEAN,
                "rect center {size} px at {depth} bits: max {max:.3}, mean {mean:.4}"
            );
        }
    }
}

/// take1's anti-aliased rows (module doc) meet the bound.
#[test]
fn the_take1_anti_aliased_shapes_meet_photoshop_at_8_and_16_bits() {
    for depth in [8, 16] {
        for doc in ["disc", "soft"] {
            let (max, mean) = case(doc, "outside", 4, depth);
            assert!(
                max <= MAX && mean <= MEAN,
                "{doc} outside 4 px at {depth} bits: max {max:.3}, mean {mean:.4}"
            );
        }
    }
}

/// `(take, doc, position, size, tag, fill, stroke opacity)`.
type Row = (u32, &'static str, &'static str, u32, &'static str, f64, f64);

/// take2's Outside rows and take 3's rows that meet the bound at both
/// depths (module doc), the disc Inside 3 since the confirm take.
const MET: [Row; 15] = [
    (2, "disc", "outside", 2, "", 1.0, 1.0),
    (2, "disc", "outside", 4, "", 1.0, 1.0),
    (2, "disc", "outside", 7, "", 1.0, 1.0),
    (2, "soft", "outside", 2, "", 1.0, 1.0),
    (2, "soft", "outside", 4, "", 1.0, 1.0),
    (2, "soft", "outside", 7, "", 1.0, 1.0),
    (2, "half", "outside", 4, "", 1.0, 1.0),
    (2, "rect", "outside", 4, "", 1.0, 1.0),
    (2, "rect", "outside", 7, "", 1.0, 1.0),
    (2, "disc", "outside", 4, "_fill0", 0.0, 1.0),
    (3, "half", "outside", 6, "", 1.0, 1.0),
    (3, "disc", "outside", 5, "_op60", 1.0, 0.6),
    (3, "disc", "center", 6, "", 1.0, 1.0),
    (3, "rect", "center", 6, "", 1.0, 1.0),
    (3, "disc", "inside", 3, "", 1.0, 1.0),
];

#[test]
fn the_take2_and_take3_rows_meet_photoshop_at_8_and_16_bits() {
    for depth in [8, 16] {
        for (take, doc, position, size, tag, fill, opacity) in MET {
            let (max, mean) = measure(take, doc, position, size, tag, fill, opacity, depth);
            assert!(
                max <= MAX && mean <= MEAN,
                "take {take} {doc} {position} {size}{tag} at {depth} bits: max {max:.3}, \
                 mean {mean:.4}"
            );
        }
    }
    for (take, doc, position, size, tag, fill, opacity) in MET_16 {
        let (max, mean) = measure(take, doc, position, size, tag, fill, opacity, 16);
        assert!(
            max <= MAX && mean <= MEAN,
            "take {take} {doc} {position} {size}{tag} at 16 bits: max {max:.3}, mean {mean:.4}"
        );
    }
}

/// take 3's rows that meet the bound at 16 bits only (module doc): the soft
/// Inside 4 at 50 % on the anti-aliased band (its 8-bit twin is pinned).
const MET_16: [Row; 1] = [(3, "soft", "inside", 4, "_op50", 1.0, 0.5)];

/// take 3's Outside rows at 50 % and 30 % Fill meet the bound at 8 and 16
/// bits, the 8-bit rows since the layer's Fill composites at its byte
/// (`opacity_byte`, module doc).
#[test]
fn the_take3_partial_fill_outside_rows_meet_photoshop_at_8_and_16_bits() {
    for depth in [8, 16] {
        for (doc, size, tag, fill) in [
            ("disc", 3, "_fill50", 0.5),
            ("soft", 5, "_fill30", 0.3),
            ("rect", 3, "_fill50", 0.5),
        ] {
            let (max, mean) = measure(3, doc, "outside", size, tag, fill, 1.0, depth);
            assert!(
                max <= MAX && mean <= MEAN,
                "{doc} outside {size}{tag} at {depth} bits: max {max:.3}, mean {mean:.4}"
            );
        }
    }
}

/// The recorded misses (module doc): each row's measured residual, pinned.
#[test]
fn the_remaining_rows_keep_their_measured_residual() {
    for ((take, doc, position, size, tag, fill, opacity), depth, (max_bound, mean_bound)) in PINNED
    {
        let (max, mean) = measure(take, doc, position, size, tag, fill, opacity, depth);
        assert!(
            max <= max_bound && mean <= mean_bound,
            "take {take} {doc} {position} {size}{tag} at {depth} bits: max {max:.3} \
             (pinned {max_bound}), mean {mean:.4} (pinned {mean_bound})"
        );
    }
}

// Measured on the shipped composite against the takes (max, mean in codes,
// rounded up in the third decimal): the recorded #2 residual.
type Pinned = (Row, u32, (f64, f64));
const PINNED: [Pinned; 7] = [
    (
        (2, "disc", "center", 4, "_op50", 1.0, 0.5),
        8,
        (63.001, 9.409),
    ),
    (
        (2, "disc", "center", 4, "_op50", 1.0, 0.5),
        16,
        (62.756, 9.205),
    ),
    (
        (3, "soft", "center", 4, "_op50", 1.0, 0.5),
        8,
        (42.001, 10.969),
    ),
    (
        (3, "soft", "center", 4, "_op50", 1.0, 0.5),
        16,
        (42.005, 10.719),
    ),
    (
        (3, "disc", "center", 3, "_fill50", 0.5, 1.0),
        8,
        (43.001, 3.362),
    ),
    (
        (3, "disc", "center", 3, "_fill50", 0.5, 1.0),
        16,
        (42.759, 6.939),
    ),
    (
        (3, "soft", "inside", 4, "_op50", 1.0, 0.5),
        8,
        (1.001, 0.816),
    ),
];
