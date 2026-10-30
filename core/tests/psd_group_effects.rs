//! A GROUP's layer effects in a PSD. Photoshop writes an
//! unstacked style on a folder record as `lfxs` — the `lfx2` body under a
//! section key — on Pass Through and Normal groups alike (take1's p1 and p3;
//! `convert-smart-object-158/take2`'s `BI_event.psd`), so the importer reads
//! the first `lfx2` or `lfxs` of a record and the writers put `lfxs` on every
//! group record at 8 and 16 bits; a styled Pass Through group keeps its
//! `pass` key and reopens as a styled Pass Through group.
//!
//! THE PHOTOSHOP BOUND (declared before measuring): Doctored's layered
//! import of take1's `jobs/b8/p{1,2,3}_rgb8.psd`, rendered at 8 bits and
//! after the 8→16 structure conversion (read as 8-bit codes), is within 1
//! code per channel of Photoshop's own `p{1,2,3}_rgb8.tif` on P_PIXELS —
//! every pixel clear by 8 px of HOLE (24, 48, 48, 32), BAND (72, 48, 48, 32),
//! SQUARE (152, 8, 32, 32) and MEMBER (96, 24, 64, 64). The take folder is
//! outside the repo; the tests skip when it is absent. A miss is recorded,
//! never widened.

use std::path::Path;

use doctored_core::doc::{BlendMode, DrDocument, Layer, LayerKind};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use image::{Rgba, RgbaImage};
use serde_json::Value;
use tempfile::TempDir;

use crate::common::psd_photoshop::blocks_in;
use crate::common::{flat_pixels, free, open_ok, pixels, styled, take1_558};
use crate::psd16_fixture as fixture16;
use crate::psd_descriptor_fixture::{boolean, objc, rgbc};
use crate::psd_layer_effects::{file16, import_report, lfx2, open_psd, open_typed, save, save16};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 24;
const H: u32 = 16;
const SQUARE: (u32, u32, u32, u32) = (14, 4, 6, 6);
const GROUP: usize = 3;
const STROKE_AND_BLEND_IF: &str = r##"{"blend_if":{"channel":"gray","this_layer":[0,0,128,128],"underlying":[0,0,255,255]},"effects":[{"type":"stroke","size":2,"color":"#ff0000"}]}"##;
const STROKE_ONLY: &str = r##"{"effects":[{"type":"stroke","size":2,"color":"#ff0000"}]}"##;
const BLEND_IF_ONLY: &str =
    r#"{"blend_if":{"channel":"gray","this_layer":[0,0,128,128],"underlying":[0,0,255,255]}}"#;

// ------------------------------------------------------------- the scene --

fn inside(x: u32, y: u32, (rx, ry, rw, rh): (u32, u32, u32, u32)) -> bool {
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

fn byte_layer(name: &str, image: RgbaImage) -> Layer {
    let mut layer = DrDocument::from_pixels(image).layers.remove(0);
    layer.name = name.into();
    layer
}

/// A backdrop under a Pass Through group holding an opaque square and an
/// Invert record, the group carrying `style`.
fn scene(style: &str) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        let v = (12 + x * 9 + y) as u8;
        Rgba([v, 255 - v, v / 2 + 60, 255])
    }));
    let mut square = byte_layer(
        "Square",
        RgbaImage::from_fn(W, H, |x, y| {
            if inside(x, y, SQUARE) {
                Rgba([200, 200, 200, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        }),
    );
    square.depth = 1;
    let mut adjustment = byte_layer("Invert", RgbaImage::new(1, 1));
    adjustment.meta = Some(r#"{"type":"adjust","op":"invert","params":{}}"#.into());
    adjustment.depth = 1;
    let mut group = byte_layer("P", RgbaImage::new(1, 1));
    group.kind = LayerKind::Group;
    group.blend = BlendMode::PassThrough;
    doc.layers.extend([square, adjustment, group]);
    styled(&doc, GROUP, style)
}

/// The file's group record: its `lsct` divider names `pass`.
fn assert_pass_divider(bytes: &[u8], what: &str) {
    let dividers = blocks_in(bytes, b"lsct");
    assert!(
        dividers.iter().any(|body| {
            let kind = u32::from_be_bytes(body[0..4].try_into().unwrap());
            (kind == 1 || kind == 2) && body.len() >= 12 && &body[8..12] == b"pass"
        }),
        "{what}: no `pass` folder divider"
    );
}

fn assert_effect_keys(bytes: &[u8], effects: bool, what: &str) {
    assert_eq!(
        blocks_in(bytes, b"lfxs").len(),
        usize::from(effects),
        "{what}: lfxs"
    );
    assert!(blocks_in(bytes, b"lfx2").is_empty(), "{what}: no lfx2");
}

fn no_group_loss(report: &Value, what: &str) {
    let losses = report["verified_losses"].as_array().unwrap();
    assert!(
        !losses.iter().any(|l| l.as_str().unwrap().contains("“P”")),
        "{what}: {losses:?}"
    );
}

fn close(a: &[u8], b: &[u8], bound: i32, what: &str) {
    assert_eq!(a.len(), b.len(), "{what}");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!(
            (i32::from(*x) - i32::from(*y)).abs() <= bound,
            "{what}: byte {i}: {x} vs {y}"
        );
    }
}

#[test]
fn an_eight_bit_styled_pass_through_group_writes_lfxs_and_reopens() {
    let dir = TempDir::new().unwrap();
    for (i, text) in [STROKE_AND_BLEND_IF, STROKE_ONLY, BLEND_IF_ONLY]
        .into_iter()
        .enumerate()
    {
        let doc = scene(text);
        let path = dir.path().join(format!("p8_{i}.psd"));
        save(&doc, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_effect_keys(&bytes, text.contains("stroke"), text);
        assert_pass_divider(&bytes, text);
        let report = import_report(&path);
        no_group_loss(&report, text);
        let back = open_psd(&path, &report);
        let group = back
            .layers
            .iter()
            .find(|l| l.name == "P")
            .expect("the group");
        assert_eq!(group.kind, LayerKind::Group, "{text}");
        assert_eq!(group.blend, BlendMode::PassThrough, "{text}");
        assert!(group.style.is_some(), "{text}");
        close(
            &flat_pixels(&*back),
            &flat_pixels(&doc),
            1,
            &format!("8-bit reopen {text}"),
        );
    }
}

#[test]
fn a_sixteen_bit_styled_pass_through_group_writes_lfxs_and_reopens() {
    let dir = TempDir::new().unwrap();
    for (i, text) in [STROKE_AND_BLEND_IF, STROKE_ONLY, BLEND_IF_ONLY]
        .into_iter()
        .enumerate()
    {
        let doc = scene(text).promote_structure(LIMIT, 0, None).unwrap();
        assert!(doc.layer(GROUP).unwrap().is_pass_through(), "{text}");
        let projection = doc.render(LIMIT, 0, None).unwrap();
        let path = dir.path().join(format!("p16_{i}.psd"));
        save16(&doc, &projection, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_effect_keys(&bytes, text.contains("stroke"), text);
        assert_pass_divider(&bytes, text);
        let report = import_report(&path);
        no_group_loss(&report, text);
        let back = open_typed(&path, &report);
        let group = (0..back.layer_count())
            .map(|i| back.layer(i).unwrap())
            .find(|l| l.name() == "P")
            .expect("the group");
        assert!(group.is_pass_through(), "{text}");
        assert!(group.reported_style().is_some(), "{text}");
        let got = back.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
        let want = projection.to_rgba16().unwrap();
        for (j, (g, w)) in got.iter().zip(&want).enumerate() {
            for c in 0..4 {
                assert!(
                    (i32::from(g[c]) - i32::from(w[c])).abs() <= 257,
                    "16-bit reopen {text}: pixel {j} channel {c}: {g:?} vs {w:?}"
                );
            }
        }
    }
}

#[test]
fn a_hand_built_lfxs_on_a_pass_through_folder_is_read() {
    let dir = TempDir::new().unwrap();
    let overlay = lfx2(&[
        boolean(b"masterFXSwitch", 1),
        objc(
            b"SoFi",
            b"SoFi",
            &[boolean(b"enab", 1), rgbc(b"Clr ", 1.0, 2.0, 3.0)],
        ),
    ]);
    let px = [[4097u16, 51234, 60000, 65535], [4098, 51233, 60000, 32768]];
    let planes: Vec<(i16, Vec<u8>)> = [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, c)| {
            (
                id,
                fixture16::raw_plane(&px.iter().map(|p| p[c]).collect::<Vec<_>>()),
            )
        })
        .collect();
    let base = fixture16::Record::raster("Base", [0, 0, 1, 2], planes.clone());
    let closer = fixture16::Record::folder("</Layer group>", 3, *b"norm");
    let art = fixture16::Record::raster("Art", [0, 0, 1, 2], planes);
    let mut folder = fixture16::Record::folder("Outer", 1, *b"pass");
    folder.blocks.extend(fixture16::block(b"lfxs", &overlay));
    let path = dir.path().join("lfxs16.psd");
    std::fs::write(&path, file16(&[base, closer, art, folder])).unwrap();
    let report = import_report(&path);
    let losses = report["verified_losses"].as_array().unwrap();
    assert!(
        !losses
            .iter()
            .any(|l| l.as_str().unwrap().contains("“Outer”")),
        "{losses:?}"
    );
    let document = open_typed(&path, &report);
    let outer = (0..document.layer_count())
        .map(|i| document.layer(i).unwrap())
        .find(|l| l.name() == "Outer")
        .unwrap();
    assert!(outer.is_pass_through());
    let style: Value = serde_json::from_str(&outer.reported_style().expect("its style")).unwrap();
    assert_eq!(style["effects"][0]["type"], "color_overlay", "{style}");
}

// ------------------------------------------------------------- Photoshop --

const P_W: u32 = 192;
const P_H: u32 = 128;
const HOLE: (u32, u32, u32, u32) = (24, 48, 48, 32);
const BAND: (u32, u32, u32, u32) = (72, 48, 48, 32);
const P_SQUARE: (u32, u32, u32, u32) = (152, 8, 32, 32);
const MEMBER: (u32, u32, u32, u32) = (96, 24, 64, 64);

fn clear_of(x: u32, y: u32, grow: u32, (rx, ry, rw, rh): (u32, u32, u32, u32)) -> bool {
    let (x, y, grow) = (i64::from(x), i64::from(y), i64::from(grow));
    let (rx, ry, rw, rh) = (i64::from(rx), i64::from(ry), i64::from(rw), i64::from(rh));
    !(rx - grow <= x && x < rx + rw + grow && ry - grow <= y && y < ry + rh + grow)
}

fn p_pixels() -> Vec<(u32, u32)> {
    (0..P_H)
        .flat_map(|y| (0..P_W).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            [HOLE, BAND, P_SQUARE, MEMBER]
                .iter()
                .all(|r| clear_of(x, y, 8, *r))
        })
        .collect()
}

/// Photoshop's flattened row as 8-bit RGBA.
fn photoshop_row(path: &Path) -> Vec<u8> {
    let image = open_ok(path);
    let out = pixels(image);
    free(image);
    out
}

/// The largest per-channel difference over P_PIXELS, RGB only.
fn worst(got: &[u8], want: &[u8]) -> i32 {
    p_pixels()
        .iter()
        .flat_map(|&(x, y)| {
            let i = ((y * P_W + x) * 4) as usize;
            (0..3).map(move |c| (i32::from(got[i + c]) - i32::from(want[i + c])).abs())
        })
        .max()
        .unwrap()
}

#[test]
fn photoshops_p_rows_meet_the_isolate_bound_at_8_and_16_bits() {
    for n in 1..=3 {
        let (Some(psd), Some(tif)) = (
            take1_558("b8", &format!("p{n}_rgb8.psd")),
            take1_558("b8", &format!("p{n}_rgb8.tif")),
        ) else {
            eprintln!("skipping: blend-if-multichannel-558/take1 is absent");
            return;
        };
        let bytes = std::fs::read(&psd).unwrap();
        // Photoshop's own file: p1 and p3 carry `lfxs` on the folder, p2
        // (an unstyled clip base) none, and none writes `lfx2` there.
        assert_eq!(!blocks_in(&bytes, b"lfxs").is_empty(), n != 2, "p{n}");
        let want = photoshop_row(&tif);
        assert_eq!(want.len(), (P_W * P_H * 4) as usize, "p{n}");
        let report = import_report(&psd);
        let doc = open_psd(&psd, &report);
        let group = doc
            .layers
            .iter()
            .find(|l| l.kind == LayerKind::Group)
            .expect("the group");
        assert_eq!(group.blend, BlendMode::PassThrough, "p{n}");
        assert_eq!(group.style.is_some(), n != 2, "p{n}");
        let eight = flat_pixels(&*doc);
        let miss8 = worst(&eight, &want);
        let sixteen: PrecisionDocument = doc.promote_structure(LIMIT, 0, None).unwrap();
        let rendered: Vec<u8> = sixteen
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap()
            .into_iter()
            .flatten()
            .map(|v| (f64::from(v) / 257.0).round() as u8)
            .collect();
        let miss16 = worst(&rendered, &want);
        eprintln!("p{n}: worst {miss8} codes at 8 bits, {miss16} after 8→16");
        assert!(miss8 <= 1, "p{n} at 8 bits: {miss8} codes");
        assert!(miss16 <= 1, "p{n} after 8→16: {miss16} codes");
    }
}
