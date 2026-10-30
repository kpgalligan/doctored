//! Photoshop 26.10.0's own 32-bit layered TIFF and 32-bit PSD with a fill
//! layer and a layer style (take1,
//! `scripts/photoshop-probe/takes_675_679.py`), parsed only.
//!
//! `core/tests/fixtures/psd_photoshop/float32_679/` holds the take's two
//! saves of a 16 x 12 sRGB scene built from the runner's constant-colour
//! float TIFFs: `layered32.tif` (Bottom, and Top cleared at x >= 8, saved
//! with layers, little-endian) and `fill_style32.psd` (Base, a Solid Color
//! fill of RGB 40/40/200, Shadowed with a Drop Shadow). They are the record
//! the 32-bit layered TIFF writer and the 32-bit fill and style records are
//! built from. Both are REQUIRED. The TIFF: its layered
//! open must give `photoshop.json`'s rows and the scene's declared constants
//! within the take's bound, 1e-3·|v| + 1e-6. The PSD: its
//! records (depth, the layer block and its count, the fill and effect
//! records), its layered open against `photoshop.json`'s rows and the
//! scene's constants, the fill's numbers read as linear light × 255 (H-raw)
//! and equal to the file's own stored composite, and a structural oracle:
//! Doctored's re-save of the opened file compared with Photoshop's `Lr32`
//! field by field (`psd_float_fill_style::lr32`, this suite's own parser).

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_psd::{dr_precision_doc_open_psd, dr_precision_doc_save_psd};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::psd_photoshop::{blocks_in, fixture, import_report, rgbc_numbers};
use crate::common::{cpath, take_err_string};
use crate::psd_float_fill_style::{lr32, srgb_oetf};

const FAMILY: &str = "float32_679";
/// The prefix of TIFF tag 37724's body (ImageSourceData).
const DATA_BLOCK: &[u8] = b"Adobe Photoshop Document Data Block\0";

fn little_endian(bytes: &[u8]) -> bool {
    match &bytes[..4] {
        b"II*\0" => true,
        b"MM\0*" => false,
        other => panic!("not a TIFF: {other:?}"),
    }
}

fn u16_in(bytes: &[u8], little: bool) -> u16 {
    let pair = [bytes[0], bytes[1]];
    if little {
        u16::from_le_bytes(pair)
    } else {
        u16::from_be_bytes(pair)
    }
}

fn u32_in(bytes: &[u8], little: bool) -> u32 {
    let quad: [u8; 4] = bytes[..4].try_into().unwrap();
    if little {
        u32::from_le_bytes(quad)
    } else {
        u32::from_be_bytes(quad)
    }
}

/// IFD0's entries as (tag, value bytes) in file order, each value read
/// inline or at its offset, for an `II` or an `MM` file.
fn ifd0(bytes: &[u8]) -> Vec<(u16, Vec<u8>)> {
    let little = little_endian(bytes);
    let first = u32_in(&bytes[4..], little) as usize;
    let count = u16_in(&bytes[first..], little) as usize;
    (0..count)
        .map(|i| {
            let at = first + 2 + 12 * i;
            let tag = u16_in(&bytes[at..], little);
            let kind = u16_in(&bytes[at + 2..], little);
            let n = u32_in(&bytes[at + 4..], little) as usize;
            let unit = match kind {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 | 13 => 4,
                5 | 10 | 12 | 16..=18 => 8,
                other => panic!("tag {tag}: TIFF type {other}"),
            };
            let size = unit * n;
            let value = if size <= 4 {
                bytes[at + 8..at + 8 + size].to_vec()
            } else {
                let offset = u32_in(&bytes[at + 8..], little) as usize;
                bytes[offset..offset + size].to_vec()
            };
            (tag, value)
        })
        .collect()
}

fn tag(entries: &[(u16, Vec<u8>)], wanted: u16) -> Vec<u8> {
    entries
        .iter()
        .find(|(t, _)| *t == wanted)
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| panic!("IFD0 has no tag {wanted}"))
}

fn shorts(value: &[u8], little: bool) -> Vec<u16> {
    value.chunks(2).map(|c| u16_in(c, little)).collect()
}

/// Tag 37724's entries after its prefix as (key, body): a signature `8BIM`
/// and a key, both written byte-reversed in an `II` file (`MIB8`, `23rL`), a
/// u32 length in the file's byte order, the body, padded to 4 bytes (what
/// this file holds: Lr32 933 bytes, then LMsk at the next multiple of 4).
fn data_blocks(block: &[u8], little: bool) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = DATA_BLOCK.len();
    while at + 12 <= block.len() {
        let mut signature = block[at..at + 4].to_vec();
        let mut key = block[at + 4..at + 8].to_vec();
        if little {
            signature.reverse();
            key.reverse();
        }
        assert_eq!(signature, b"8BIM", "an entry signature at {at}");
        let len = u32_in(&block[at + 8..], little) as usize;
        out.push((
            String::from_utf8_lossy(&key).into_owned(),
            block[at + 12..at + 12 + len].to_vec(),
        ));
        at += 12 + len + (4 - len % 4) % 4;
    }
    out
}

/// The take's per-sample bound (take2's): 1e-3·|v| + 1e-6.
fn within(got: f32, want: f64) -> bool {
    (f64::from(got) - want).abs() <= 1e-3 * want.abs() + 1e-6
}

#[test]
fn photoshops_32_bit_layered_tiff_parses() {
    let path = fixture(FAMILY, "layered32.tif");
    let bytes = std::fs::read(&path).expect("the committed layered32.tif");
    let little = little_endian(&bytes);
    assert!(little, "the take saved byte order IBM");
    let entries = ifd0(&bytes);
    assert_eq!(shorts(&tag(&entries, 258), little), vec![32, 32, 32]);
    assert_eq!(
        shorts(&tag(&entries, 339), little),
        vec![3, 3, 3],
        "SampleFormat IEEE float"
    );
    let block = tag(&entries, 37724);
    assert!(block.starts_with(DATA_BLOCK), "tag 37724's prefix");
    let blocks = data_blocks(&block, little);
    let layered: Vec<&(String, Vec<u8>)> = blocks
        .iter()
        .filter(|(key, _)| matches!(key.as_str(), "Layr" | "Lr16" | "Lr32"))
        .collect();
    assert_eq!(layered.len(), 1, "one layer block");
    assert_eq!(layered[0].0, "Lr32", "the 32-bit layer records go in Lr32");
    let count = u16_in(&layered[0].1, little) as i16;
    assert_eq!(count.unsigned_abs(), 2, "Bottom and Top");

    // The layered open: photoshop.json's rows, top first.
    let report = import_report(&path);
    assert_eq!(report["format"], "TIFF", "{report}");
    assert_eq!(report["import_precision"], "rgba32", "{report}");
    assert_eq!(report["layered_available"], true, "{report}");
    let expected = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let handle = unsafe {
        dr_precision_doc_open_psd(
            cpath(&path).as_ptr(),
            0,
            expected.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!handle.is_null(), "{}", take_err_string(error));
    let document = unsafe { &*handle };
    let json: Value = serde_json::from_slice(
        &std::fs::read(fixture(FAMILY, "photoshop.json")).expect("photoshop.json"),
    )
    .unwrap();
    let file = &json["files"][0];
    assert_eq!(file["name"], "layered32.tif");
    let rows = file["rows"].as_array().unwrap();
    assert_eq!(document.layer_count(), rows.len(), "the layer count");
    assert_eq!(
        document.dimensions(),
        (
            file["document"]["width"].as_u64().unwrap() as u32,
            file["document"]["height"].as_u64().unwrap() as u32
        )
    );
    let scene = &json["scene"];
    let constant = |name: &str| -> [f64; 3] {
        let values = scene[name].as_array().unwrap();
        [0, 1, 2].map(|k| values[k].as_f64().unwrap())
    };
    for (index, row) in rows.iter().rev().enumerate() {
        let layer = document.layer(index).expect("a layer");
        let name = row["name"].as_str().unwrap();
        assert_eq!(layer.name(), name);
        assert_eq!(row["blend"], "BlendMode.NORMAL", "{name}");
        assert_eq!(layer.blend_mode(), BlendMode::Normal, "{name}");
        assert_eq!(
            layer.opacity(),
            row["opacity"].as_f64().unwrap() as f32 / 100.0,
            "{name}"
        );
        assert_eq!(layer.visible(), row["visible"].as_bool().unwrap(), "{name}");
        // Photoshop's bounds are [left, top, right, bottom].
        let image = layer.source().expect("a raster");
        let (x, y) = layer.offset();
        let (w, h) = image.dimensions();
        let bounds: Vec<i64> = row["bounds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        assert_eq!(
            bounds,
            [
                i64::from(x),
                i64::from(y),
                i64::from(x) + i64::from(w),
                i64::from(y) + i64::from(h)
            ],
            "{name}: bounds"
        );
        let want = constant(&name.to_lowercase());
        for pixel in image.runs_rgba32().expect("f32 samples").flatten() {
            for k in 0..3 {
                assert!(
                    within(pixel[k], want[k]),
                    "{name}: sample {k} {} against {}",
                    pixel[k],
                    want[k]
                );
            }
            assert!(within(pixel[3], 1.0), "{name}: alpha {}", pixel[3]);
        }
    }
    unsafe { dr_precision_doc_free(handle) };
}

#[test]
fn photoshops_32_bit_fill_and_style_psd_parses() {
    let bytes =
        std::fs::read(fixture(FAMILY, "fill_style32.psd")).expect("the committed fill_style32.psd");
    assert_eq!(&bytes[..4], b"8BPS");
    assert_eq!(u16::from_be_bytes([bytes[4], bytes[5]]), 1, "a PSD");
    assert_eq!(u16::from_be_bytes([bytes[22], bytes[23]]), 32, "depth");
    assert_eq!(u16::from_be_bytes([bytes[24], bytes[25]]), 3, "RGB");
    let report = import_report(&fixture(FAMILY, "fill_style32.psd"));
    assert_eq!(report["depth"], 32);
    assert_eq!(report["import_precision"], "rgba32");
    assert_eq!(report["layer_count"], 3);
    let records = blocks_in(&bytes, b"Lr32");
    assert_eq!(records.len(), 1, "one Lr32 block");
    let count = i16::from_be_bytes([records[0][0], records[0][1]]);
    assert_eq!(count.unsigned_abs(), 3, "Base, Fill and Shadowed");
    let fills = blocks_in(&bytes, b"SoCo");
    assert_eq!(fills.len(), 1, "one Solid Color record");
    // The take asked for 40/40/200; at 32 bits Photoshop stores the colour as
    // linear light × 255, each number `f32(c / 255) · 255`, so the doubles are
    // not the integers (the depth16 fixture's `SoCo` holds 40/40/200 exactly).
    // Asserted as the file holds them.
    assert_eq!(
        rgbc_numbers(&fills[0]),
        [40.00000141561031, 40.00000141561031, 200.00000327825546]
    );
    let effects: Vec<Vec<u8>> = blocks_in(&bytes, b"lfx2")
        .into_iter()
        .chain(blocks_in(&bytes, b"lrFX"))
        .collect();
    assert!(
        effects
            .iter()
            .any(|body| body.windows(4).any(|w| w == b"DrSh")),
        "a Drop Shadow in an effects block"
    );
}

/// The layered open of `path`, as an owned document.
fn open_layered(path: &Path) -> PrecisionDocument {
    let report = import_report(path);
    assert_eq!(report["import_precision"], "rgba32", "{report}");
    assert_eq!(report["layered_available"], true, "{report}");
    let expected = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let handle = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            0,
            expected.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!handle.is_null(), "{}", take_err_string(error));
    *unsafe { Box::from_raw(handle) }
}

/// The stored composite's R, G and B at (0, 0) of a big-endian classic PSD
/// whose image data is Raw f32 planes (this file's own reader: header, the
/// colour-mode, resource and layer-and-mask sections skipped by length).
fn composite_origin(bytes: &[u8], width: usize, height: usize) -> [f32; 3] {
    let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let colour_mode = word(26);
    let resources = word(30 + colour_mode);
    let layer_and_mask = 34 + colour_mode + resources;
    let image = layer_and_mask + 4 + word(layer_and_mask);
    assert_eq!(&bytes[image..image + 2], &[0, 0], "Raw image data");
    let plane = width * height * 4;
    [0, 1, 2].map(|k| {
        let at = image + 2 + k * plane;
        f32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
    })
}

#[test]
fn photoshops_32_bit_fill_and_style_psd_opens_to_its_rows() {
    let path = fixture(FAMILY, "fill_style32.psd");
    let bytes = std::fs::read(&path).expect("the committed fill_style32.psd");
    let json: Value = serde_json::from_slice(
        &std::fs::read(fixture(FAMILY, "photoshop.json")).expect("photoshop.json"),
    )
    .unwrap();
    let file = &json["files"][1];
    assert_eq!(file["name"], "fill_style32.psd");
    let rows = file["rows"].as_array().unwrap();
    let document = open_layered(&path);
    assert_eq!(document.layer_count(), rows.len(), "the layer count");
    let (width, height) = document.dimensions();
    // H-raw: the `SoCo` numbers are linear light × 255.
    let raw = rgbc_numbers(&blocks_in(&bytes, b"SoCo")[0]).map(|v| v / 255.0);
    let composite = composite_origin(&bytes, width as usize, height as usize);
    for k in 0..3 {
        assert!(
            within(composite[k], raw[k]),
            "the stored composite {composite:?} against {raw:?}"
        );
    }
    let base = json["scene"]["bottom"].as_array().unwrap();
    let base = [0, 1, 2].map(|k| base[k].as_f64().unwrap());
    for (index, row) in rows.iter().rev().enumerate() {
        let layer = document.layer(index).expect("a layer");
        let name = row["name"].as_str().unwrap();
        assert_eq!(layer.name(), name);
        assert_eq!(row["blend"], "BlendMode.NORMAL", "{name}");
        assert_eq!(layer.blend_mode(), BlendMode::Normal, "{name}");
        assert_eq!(
            layer.opacity(),
            row["opacity"].as_f64().unwrap() as f32 / 100.0,
            "{name}"
        );
        assert_eq!(layer.visible(), row["visible"].as_bool().unwrap(), "{name}");
        let image = layer.source().expect("a raster");
        let samples: Vec<[f32; 4]> = image
            .runs_rgba32()
            .expect("f32 samples")
            .flatten()
            .copied()
            .collect();
        match name {
            "Base" => {
                assert_eq!(
                    layer.mask().is_some(),
                    row["has_user_mask"].as_bool().unwrap()
                );
                let (x, y) = layer.offset();
                let (w, h) = image.dimensions();
                let bounds: Vec<i64> = row["bounds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_i64().unwrap())
                    .collect();
                assert_eq!(
                    bounds,
                    [
                        i64::from(x),
                        i64::from(y),
                        i64::from(x) + i64::from(w),
                        i64::from(y) + i64::from(h)
                    ],
                    "Base: bounds"
                );
                for pixel in &samples {
                    for k in 0..3 {
                        assert!(within(pixel[k], base[k]), "Base: {pixel:?}");
                    }
                    assert!(within(pixel[3], 1.0), "Base: {pixel:?}");
                }
            }
            "Fill" => {
                // Photoshop's zero-rect, default-255 mask body is no mask
                // (`MaskRecord::classify`), though its row says
                // `has_user_mask`.
                assert!(layer.mask().is_none(), "Fill: no mask");
                let d: Value =
                    serde_json::from_str(document.artwork_description(index).expect("described"))
                        .unwrap();
                assert_eq!(d["fill"], "solid", "{d}");
                for k in 0..3 {
                    let got = d["color"][k].as_f64().unwrap();
                    assert!(
                        (got - srgb_oetf(raw[k])).abs() <= 1e-6,
                        "{d} against {raw:?}"
                    );
                }
                assert_eq!(image.dimensions(), (width, height));
                for pixel in &samples {
                    for k in 0..3 {
                        assert!(within(pixel[k], raw[k]), "Fill: {pixel:?} against {raw:?}");
                    }
                    assert!(within(pixel[3], 1.0), "Fill: {pixel:?}");
                }
                for k in 0..3 {
                    assert!(
                        within(samples[0][k], f64::from(composite[k])),
                        "Fill: {:?} against the composite {composite:?}",
                        samples[0]
                    );
                }
            }
            "Shadowed" => {
                assert_eq!(
                    layer.mask().is_some(),
                    row["has_user_mask"].as_bool().unwrap()
                );
                let style: Value =
                    serde_json::from_str(&layer.style().expect("a style").to_json()).unwrap();
                // Photoshop also stores each dialog effect it left off
                // (`present: false`), carried disabled.
                let effects: Vec<&Value> = style["effects"]
                    .as_array()
                    .expect("effects")
                    .iter()
                    .filter(|effect| effect["enabled"] == true)
                    .collect();
                assert_eq!(effects.len(), 1, "one enabled effect: {style}");
                let shadow = &effects[0];
                assert_eq!(shadow["type"], "drop_shadow", "{style}");
                assert_eq!(shadow["color"], "#000000", "{style}");
                assert_eq!(shadow["blend"], "multiply", "{style}");
                assert_eq!(shadow["use_global_light"], false, "{style}");
                for (key, want) in [
                    ("opacity", 0.75),
                    ("angle", 120.0),
                    ("distance", 2.0),
                    ("size", 2.0),
                    ("spread", 0.0),
                ] {
                    assert_eq!(shadow[key].as_f64(), Some(want), "{key}: {style}");
                }
            }
            other => panic!("an unexpected row {other}"),
        }
    }
}

/// A `SoCo` body with its three `RGBC` doubles zeroed, and the doubles.
fn soco_masked(body: &[u8]) -> (Vec<u8>, [f64; 3]) {
    let mut out = body.to_vec();
    for key in [b"Rd  doub", b"Grn doub", b"Bl  doub"] {
        let at = body.windows(8).position(|w| w == key).expect("an RGBC key") + 8;
        out[at..at + 8].fill(0);
    }
    (out, rgbc_numbers(body))
}

#[test]
fn rasterizes_resave_of_the_fill_and_style_psd_matches_photoshops_lr32_field_by_field() {
    let path = fixture(FAMILY, "fill_style32.psd");
    let theirs = lr32(&std::fs::read(&path).expect("the committed fill_style32.psd"));
    let document = open_layered(&path);
    let directory = TempDir::new().unwrap();
    let out = directory.path().join("resaved32.psd");
    let projection = document
        .render(REFERENCE_WORKING_BYTES, 0, None)
        .expect("a projection");
    let mut error = ptr::null_mut();
    let saved = unsafe {
        dr_precision_doc_save_psd(
            &document,
            &projection,
            cpath(&out).as_ptr(),
            0,
            0,
            REFERENCE_WORKING_BYTES,
            0,
            &mut error,
        )
    };
    assert!(saved, "{}", take_err_string(error));
    let ours = lr32(&std::fs::read(&out).unwrap());
    assert_eq!(ours.count, 3, "our count");
    assert_eq!(theirs.count, 3, "Photoshop's count");
    let rects = [[0, 0, 12, 16], [0; 4], [3, 4, 9, 12]];
    let names = ["Base", "Fill", "Shadowed"];
    for (index, (a, b)) in ours.records.iter().zip(&theirs.records).enumerate() {
        let what = names[index];
        assert_eq!(a.name, what);
        assert_eq!(b.name, what);
        assert_eq!(a.rect, rects[index], "{what}: our rect");
        assert_eq!(b.rect, rects[index], "{what}: Photoshop's rect");
        // Photoshop's Fill also carries a bare `-2` over its zero-rect,
        // default-255 mask body, which reads as no mask, so the re-save
        // writes none; it is left out.
        let ids = |r: &crate::psd_float_fill_style::Lr32Record| -> Vec<i16> {
            r.channels
                .iter()
                .map(|(id, _)| *id)
                .filter(|id| index != 1 || *id != -2)
                .collect()
        };
        assert_eq!(ids(a), [-1, 0, 1, 2], "{what}: our channels");
        assert_eq!(ids(b), [-1, 0, 1, 2], "{what}: Photoshop's channels");
        for r in [a, b] {
            assert_eq!(&r.blend, b"norm", "{what}");
            assert_eq!(r.opacity, 255, "{what}");
            assert_eq!(r.clipping, 0, "{what}");
        }
    }
    let (base, fill) = (
        (&ours.records[0], &theirs.records[0]),
        (&ours.records[1], &theirs.records[1]),
    );
    assert_eq!((base.0.flags, base.1.flags), (0x08, 0x08), "Base flags");
    assert_eq!((fill.0.flags, fill.1.flags), (0x18, 0x18), "Fill flags");
    for r in [fill.0, fill.1] {
        for k in 0..4 {
            assert_eq!(r.channels[k].1, 2, "Fill channel {k}: 2 bytes");
            assert_eq!(r.data[k], [0, 0], "Fill channel {k}: Raw, empty");
        }
    }
    assert!(ours.records[0].mask.is_empty(), "Base: no mask body");
    let (our_body, our_numbers) = soco_masked(fill.0.block(b"SoCo").expect("our SoCo"));
    let (their_body, their_numbers) = soco_masked(fill.1.block(b"SoCo").expect("Photoshop's SoCo"));
    assert_eq!(our_body, their_body, "the SoCo bytes, doubles masked");
    for (ours, theirs) in our_numbers.iter().zip(their_numbers) {
        assert!(
            (ours - theirs).abs() <= 1e-4,
            "{our_numbers:?} against {their_numbers:?}"
        );
    }
}
