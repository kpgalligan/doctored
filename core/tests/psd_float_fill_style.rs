//! A Solid Color fill and a layer style in a layered 32-bit PSD, PSB and
//! TIFF, through the C API: a float document's export writes
//! the fill as Photoshop's pixel-less 32-bit `SoCo` and every style and fill
//! colour as Photoshop's 32-bit numbers (the document's linear light × 255),
//! and the 32-bit open carries the fill back. A Gradient or Pattern fill has
//! no 32-bit record Photoshop has shown, so each refuses by name in both
//! directions.
//!
//! Oracles: the sRGB transfer functions and the × 255 are written here; the
//! written numbers are read off the file's bytes by this file's own scanner
//! (`blocks_in`, `rgbc_numbers`) and its own `Lr32` record parser
//! ([`lr32`], which `psd_photoshop_float32_679.rs` also runs over
//! Photoshop's file); every raster sample and mask code is compared bit for
//! bit with the document that was written. Never a golden file.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_artwork::{dr_artwork_render_float, dr_precision_doc_artwork_at};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::layered_tiff_little_endian;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::psd_photoshop::{blocks_in, rgbc_numbers};
use crate::common::{cpath, take_err_string};
use crate::float_fixture::float_doc;
use crate::psd_gradient_fill_layer::{rgbc_items, Gd};

const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;
const PSD: i32 = 0;
const PSB: i32 = 1;
const TIFF: i32 = 2;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 16;
const H: u32 = 12;
/// The fill's authored sRGB bytes (the take's `#2828C8`).
const FILL: [u8; 3] = [40, 40, 200];
/// The shadow's authored colour.
const SHADOW: &str = "#FF8000";

// ------------------------------------------------------------- oracles --

/// The sRGB EOTF (IEC 61966-2-1), encoded 0…1 → linear light.
pub(crate) fn srgb_eotf(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// The sRGB OETF, linear light → encoded 0…1.
pub(crate) fn srgb_oetf(v: f64) -> f64 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// One layer record of a PSD's `Lr32` block, as this file's parser reads it.
pub(crate) struct Lr32Record {
    /// top, left, bottom, right.
    pub(crate) rect: [i32; 4],
    /// (id, stored length) per channel, in record order.
    pub(crate) channels: Vec<(i16, u32)>,
    pub(crate) blend: [u8; 4],
    pub(crate) opacity: u8,
    pub(crate) clipping: u8,
    pub(crate) flags: u8,
    /// The file offset of the record's extra-data length word.
    pub(crate) extra_at: usize,
    /// The mask data body (its length word excluded).
    pub(crate) mask: Vec<u8>,
    pub(crate) name: String,
    /// (key, file offset of the block's signature, body) per additional block.
    pub(crate) blocks: Vec<([u8; 4], usize, Vec<u8>)>,
    /// Each channel's stored bytes (compression word, then data).
    pub(crate) data: Vec<Vec<u8>>,
}

impl Lr32Record {
    /// The body of the record's `key` block.
    pub(crate) fn block(&self, key: &[u8; 4]) -> Option<&[u8]> {
        self.blocks
            .iter()
            .find(|(k, _, _)| k == key)
            .map(|(_, _, body)| body.as_slice())
    }
}

/// A PSD's `Lr32` block: where its length word sits, its count, its records.
pub(crate) struct Lr32 {
    pub(crate) length_at: usize,
    pub(crate) count: i16,
    pub(crate) records: Vec<Lr32Record>,
}

fn be16(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}

fn be32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// The first `8BIMLr32` block of a big-endian classic PSD (u32 lengths),
/// parsed field by field: the record headers in order, then each record's
/// channel data in channel order.
pub(crate) fn lr32(bytes: &[u8]) -> Lr32 {
    let start = bytes
        .windows(8)
        .position(|w| w == b"8BIMLr32")
        .expect("an Lr32 block");
    let length_at = start + 8;
    let mut at = start + 12;
    let count = be16(bytes, at) as i16;
    at += 2;
    let mut records = Vec::new();
    for _ in 0..count.unsigned_abs() {
        let rect = [0, 1, 2, 3].map(|k| be32(bytes, at + 4 * k) as i32);
        at += 16;
        let n = be16(bytes, at) as usize;
        at += 2;
        let channels = (0..n)
            .map(|k| (be16(bytes, at + 6 * k) as i16, be32(bytes, at + 6 * k + 2)))
            .collect();
        at += 6 * n;
        assert_eq!(&bytes[at..at + 4], b"8BIM", "a record's blend signature");
        let blend: [u8; 4] = bytes[at + 4..at + 8].try_into().unwrap();
        let (opacity, clipping, flags) = (bytes[at + 8], bytes[at + 9], bytes[at + 10]);
        at += 12;
        let extra_at = at;
        let end = at + 4 + be32(bytes, at) as usize;
        at += 4;
        let mask_len = be32(bytes, at) as usize;
        let mask = bytes[at + 4..at + 4 + mask_len].to_vec();
        at += 4 + mask_len;
        at += 4 + be32(bytes, at) as usize;
        let name_len = bytes[at] as usize;
        let name = String::from_utf8_lossy(&bytes[at + 1..at + 1 + name_len]).into_owned();
        at += (1 + name_len).next_multiple_of(4);
        let mut blocks = Vec::new();
        while at + 12 <= end {
            let key: [u8; 4] = bytes[at + 4..at + 8].try_into().unwrap();
            let len = be32(bytes, at + 8) as usize;
            blocks.push((key, at, bytes[at + 12..at + 12 + len].to_vec()));
            at += 12 + len;
        }
        at = end;
        records.push(Lr32Record {
            rect,
            channels,
            blend,
            opacity,
            clipping,
            flags,
            extra_at,
            mask,
            name,
            blocks,
            data: Vec::new(),
        });
    }
    for record in &mut records {
        for (_, len) in &record.channels {
            record.data.push(bytes[at..at + *len as usize].to_vec());
            at += *len as usize;
        }
    }
    Lr32 {
        length_at,
        count,
        records,
    }
}

// ------------------------------------------------------------ C plumbing --

/// An FFI result handle taken back as an owned value.
fn take(handle: *mut PrecisionDocument, err: *mut c_char, what: &str) -> PrecisionDocument {
    assert!(!handle.is_null(), "{what}: {}", take_err_string(err));
    *unsafe { Box::from_raw(handle) }
}

fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("report JSON")
}

fn open(path: &Path) -> Result<PrecisionDocument, String> {
    let report = import_report(path);
    let expected = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let document = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            LAYERS,
            expected.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if document.is_null() {
        Err(take_err_string(error))
    } else {
        Ok(take(document, error, "open"))
    }
}

fn export_report(document: &PrecisionDocument, container: i32) -> Value {
    let mut error = ptr::null_mut();
    let json =
        unsafe { dr_precision_doc_psd_export_report(document, container, LAYERS, &mut error) };
    assert!(!json.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(json)).expect("export report JSON")
}

fn save(
    document: &PrecisionDocument,
    path: &Path,
    container: i32,
    mode: i32,
) -> Result<(), String> {
    let projection = document.render(LIMIT, 0, None).unwrap();
    let mut error = ptr::null_mut();
    if unsafe {
        dr_precision_doc_save_psd(
            document,
            &projection,
            cpath(path).as_ptr(),
            container,
            mode,
            LIMIT,
            0,
            std::ptr::null(),
            &mut error,
        )
    } {
        Ok(())
    } else {
        Err(take_err_string(error))
    }
}

fn edit(document: &PrecisionDocument, request: &str) -> PrecisionDocument {
    let c = CString::new(request).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    take(out, err, request)
}

/// A float artwork raster of `components` in `profile`
/// (`dr_artwork_render_float`).
fn render_float(profile: &Arc<IccProfile>, components: &str) -> PrecisionImage {
    let components = CString::new(components).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_artwork_render_float(
            W,
            H,
            components.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!image.is_null(), "{}", take_err_string(err));
    *unsafe { Box::from_raw(image) }
}

/// `document` with an artwork record `name` (its `image` and `description`)
/// directly above layer `anchor`.
fn publish(
    document: &PrecisionDocument,
    anchor: usize,
    image: &PrecisionImage,
    name: &str,
    description: &str,
) -> PrecisionDocument {
    let (name, description) = (
        CString::new(name).unwrap(),
        CString::new(description).unwrap(),
    );
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork_at(
            document,
            anchor,
            0,
            image,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    take(out, err, "artwork")
}

fn bits_of(image: &PrecisionImage) -> Vec<[u32; 4]> {
    image
        .runs_rgba32()
        .expect("an f32 image")
        .flatten()
        .map(|p| p.map(f32::to_bits))
        .collect()
}

fn samples_of(image: &PrecisionImage) -> Vec<[f32; 4]> {
    image
        .runs_rgba32()
        .expect("an f32 image")
        .flatten()
        .copied()
        .collect()
}

fn mask_codes(document: &PrecisionDocument, index: usize) -> Option<(Vec<u16>, (i32, i32))> {
    let mask = document.layer(index)?.mask()?;
    Some((mask.plane().samples().to_vec(), mask.properties().offset))
}

fn description_of(document: &PrecisionDocument, index: usize) -> Value {
    serde_json::from_str(
        document
            .artwork_description(index)
            .expect("a fill description"),
    )
    .expect("description JSON")
}

// --------------------------------------------------------------- scene --

/// The fill's description colour: its authored sRGB bytes / 255.
fn fill_color() -> [f64; 3] {
    FILL.map(|v| f64::from(v) / 255.0)
}

/// Base (an extended-range ramp), Fill (a Solid Color fill of [`FILL`] with
/// a graded mask) and Shadowed (white on x in [4, 12), y in [3, 9), with a
/// Drop Shadow of [`SHADOW`]), bottom first: the take's scene.
fn scene() -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let base: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let i = i as f32;
            [0.25 + 0.013 * i, 0.5, 2.0 - 0.004 * i, 1.0]
        })
        .collect();
    let shadowed: Vec<[f32; 4]> = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (4..12).contains(&x) && (3..9).contains(&y) {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let document = float_doc(W, H, &profile, &[base, shadowed]);
    let color = fill_color();
    // A float document's artwork numbers are linear light (H-raw).
    let linear = color.map(srgb_eotf);
    let image = render_float(
        &profile,
        &json!([{"kind": "fill", "color": [linear[0], linear[1], linear[2], 1.0]}]).to_string(),
    );
    let description = json!({"type": "fill", "version": 1, "fill": "solid",
                             "color": [color[0], color[1], color[2], 1.0], "canvas": [W, H]});
    let document = publish(&document, 0, &image, "Fill", &description.to_string());
    let codes: Vec<u16> = (0..W * H)
        .map(|i| match i {
            0 => 0,
            1 => 65535,
            _ => ((i * 4099 + 17) % 65536) as u16,
        })
        .collect();
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(W, H, codes, None).expect("a mask plane"),
        MaskProperties::at((0, 0)),
        true,
    )
    .expect("a mask");
    let document = document
        .set_layer_mask(1, Some(mask), LIMIT, 0, None)
        .expect("a float mask")
        .expect("a change");
    let style = json!({"op": "layer_style", "version": 1, "layer": 2, "style": {"effects": [
        {"type": "drop_shadow", "color": SHADOW, "blend": "multiply", "opacity": 0.75,
         "angle": 120, "use_global_light": false, "distance": 2, "spread": 0, "size": 2}]}});
    let document = edit(&document, &style.to_string());
    assert_eq!(document.layer_count(), 3);
    assert_eq!(document.layer(1).unwrap().name(), "Fill");
    document
}

/// A float document whose top record is a fill of `description`, its
/// pixels any float raster.
fn described(description: &Value) -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let base = vec![[0.25f32, 0.5, 2.0, 1.0]; (W * H) as usize];
    let document = float_doc(W, H, &profile, &[base]);
    let image = render_float(&profile, r#"[{"kind":"fill","color":[0.3,0.6,0.15,1.0]}]"#);
    publish(&document, 0, &image, "Fill", &description.to_string())
}

fn write(directory: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = directory.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

// --------------------------------------------------------------- tests --

#[test]
fn a_solid_fill_and_a_drop_shadow_round_trip_through_32_bit_psd_psb_and_tiff() {
    let directory = TempDir::new().unwrap();
    let scene = scene();
    let fill = fill_color();
    let fill_linear = fill.map(srgb_eotf);
    for (container, name) in [(PSD, "rz32.psd"), (PSB, "rz32.psb"), (TIFF, "rz32.tif")] {
        let report = export_report(&scene, container);
        assert!(report["refusal"].is_null(), "{name}: {report}");
        let path = directory.path().join(name);
        save(&scene, &path, container, LAYERS).unwrap_or_else(|e| panic!("{name}: {e}"));
        let bytes = std::fs::read(&path).unwrap();

        // The written numbers: Photoshop's 32-bit linear light × 255.
        let soco = blocks_in(&bytes, b"SoCo");
        assert_eq!(soco.len(), 1, "{name}: one SoCo");
        for (got, want) in rgbc_numbers(&soco[0]).iter().zip(fill_linear) {
            assert!(
                (got - 255.0 * want).abs() <= 1e-3,
                "{name}: SoCo {got} against {}",
                255.0 * want
            );
        }
        let lfx2 = blocks_in(&bytes, b"lfx2");
        assert_eq!(lfx2.len(), 1, "{name}: one lfx2");
        assert!(lfx2[0].windows(4).any(|w| w == b"DrSh"), "{name}: DrSh");
        let want = [255.0, 255.0 * srgb_eotf(128.0 / 255.0), 0.0];
        for (got, want) in rgbc_numbers(&lfx2[0]).iter().zip(want) {
            assert!(
                (got - want).abs() <= 1e-3,
                "{name}: DrSh Clr {got} against {want}"
            );
        }

        // The round trip.
        let back = open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(back.layer_count(), scene.layer_count(), "{name}");
        for index in 0..scene.layer_count() {
            let (a, b) = (scene.layer(index).unwrap(), back.layer(index).unwrap());
            let what = format!("{name} layer {index}");
            assert_eq!(b.name(), a.name(), "{what}");
            assert_eq!(b.blend_mode(), a.blend_mode(), "{what}");
            assert_eq!(b.opacity(), a.opacity(), "{what}");
            assert_eq!(b.visible(), a.visible(), "{what}");
            assert_eq!(
                mask_codes(&back, index),
                mask_codes(&scene, index),
                "{what}"
            );
            let (a_image, b_image) = (a.source().unwrap(), b.source().unwrap());
            if index == 1 {
                // The fill: re-rendered from its description.
                assert_eq!(b.offset(), (0, 0), "{what}");
                assert_eq!(b_image.dimensions(), (W, H), "{what}");
                for (p, q) in samples_of(a_image).iter().zip(samples_of(b_image)) {
                    for k in 0..4 {
                        assert!((p[k] - q[k]).abs() <= 1e-6, "{what}: {p:?} against {q:?}");
                    }
                }
            } else {
                assert_eq!(b.offset(), a.offset(), "{what}");
                assert_eq!(bits_of(b_image), bits_of(a_image), "{what}");
            }
        }
        let d = description_of(&back, 1);
        assert_eq!(d["fill"], "solid", "{name}: {d}");
        assert!(d.get("linear").is_none(), "{name}: {d}");
        for (k, want) in fill.iter().enumerate() {
            let got = d["color"][k].as_f64().unwrap();
            assert!((got - want).abs() <= 1e-6, "{name}: {d}");
        }
        let style = |document: &PrecisionDocument| {
            document
                .layer(2)
                .unwrap()
                .style()
                .expect("a style")
                .to_json()
        };
        assert_eq!(style(&back), style(&scene), "{name}");
        assert!(
            style(&back).to_lowercase().contains(&SHADOW.to_lowercase()),
            "{name}: {}",
            style(&back)
        );

        if container == TIFF {
            assert_eq!(
                layered_tiff_little_endian(&bytes).unwrap_err(),
                "a little-endian layered TIFF does not carry a “SoCo” record"
            );
        }
    }
}

#[test]
fn the_32_bit_fill_record_is_photoshops_pixel_less_form() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("rz32.psd");
    save(&scene(), &path, PSD, LAYERS).unwrap();
    let records = lr32(&std::fs::read(&path).unwrap());
    assert_eq!(records.count, 3);
    let fill = &records.records[1];
    assert_eq!(fill.name, "Fill");
    assert_eq!(fill.rect, [0; 4], "no rectangle");
    let ids: Vec<i16> = fill.channels.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, [-1, 0, 1, 2, -2]);
    for (data, (id, _)) in fill.data.iter().zip(&fill.channels).take(4) {
        assert_eq!(data, &[0, 0], "channel {id}: an empty Raw plane");
    }
    assert_eq!(fill.flags, 0x18, "bits 3 and 4");
    assert!(fill.block(b"SoCo").is_some(), "the SoCo record");
    // The mask's `-2`: Raw f32 coverage over its rectangle.
    assert_eq!(fill.data[4].len(), 2 + 4 * (W * H) as usize);
    assert_eq!(
        records.records[2].block(b"lfx2").map(|b| !b.is_empty()),
        Some(true),
        "the shadow's lfx2"
    );
}

#[test]
fn a_gradient_or_pattern_fill_refuses_the_32_bit_layered_export_by_name() {
    let gradient = json!({"type": "fill", "version": 1, "fill": "gradient", "canvas": [W, H],
        "gradient": {"stops": [{"position": 0.0, "color": "#ff0000", "opacity": 1.0},
                               {"position": 1.0, "color": "#0000ff", "opacity": 1.0}],
                     "style": "linear", "angle": 90.0, "scale": 1.0, "reverse": false,
                     "align_with_layer": false}});
    let pattern = json!({"type": "fill", "version": 1, "fill": "pattern", "canvas": [W, H],
        "pattern_fill": {"pattern": {"name": "T", "width": 1, "height": 1,
                                     "rgba": "/wAA/w=="},
                         "angle": 0, "scale": 1, "link_with_layer": true, "phase": [0, 0]}});
    let directory = TempDir::new().unwrap();
    for (description, kind) in [(gradient, "gradient"), (pattern, "pattern")] {
        let document = described(&description);
        let sentence = format!(
            "the {kind} fill layer “Fill” has no 32-bit PSD record in Doctored; rasterize it, or export Flattened"
        );
        for container in [PSD, PSB, TIFF] {
            let report = export_report(&document, container);
            assert_eq!(report["refusal"], sentence.as_str(), "{report}");
            let path = directory.path().join(format!("kept{container}"));
            std::fs::write(&path, b"kept").unwrap();
            assert_eq!(
                save(&document, &path, container, LAYERS).expect_err("refused"),
                sentence
            );
            assert_eq!(std::fs::read(&path).unwrap(), b"kept");
            save(&document, &path, container, FLATTENED).expect("flattened writes");
        }
    }
}

#[test]
fn a_32_bit_gradient_fill_record_refuses_the_layered_open_by_name() {
    // Doctored's own 32-bit PSD with its `SoCo` replaced by a `GdFl` of
    // take1's shape (`psd_gradient_fill_layer::Gd`), every enclosing length
    // moved by the same amount.
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("rz32.psd");
    save(&scene(), &path, PSD, LAYERS).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let records = lr32(&bytes);
    let fill = &records.records[1];
    let (_, at, old) = fill
        .blocks
        .iter()
        .find(|(key, _, _)| key == b"SoCo")
        .expect("SoCo");
    let mut body = Gd::new(
        rgbc_items(&[[255.0, 0.0, 0.0], [0.0, 0.0, 255.0]]),
        &[0, 4096],
    )
    .edited(|_| {});
    while (body.len() as i64 - old.len() as i64).rem_euclid(4) != 0 {
        body.push(0);
    }
    let delta = body.len() as i64 - old.len() as i64;
    let mut spliced = bytes[..*at].to_vec();
    spliced.extend(b"8BIMGdFl");
    spliced.extend((body.len() as u32).to_be_bytes());
    spliced.extend(&body);
    spliced.extend(&bytes[at + 12 + old.len()..]);
    let colour_mode = be32(&bytes, 26) as usize;
    let resources = be32(&bytes, 30 + colour_mode) as usize;
    let layer_and_mask = 34 + colour_mode + resources;
    for word in [layer_and_mask, records.length_at, fill.extra_at] {
        assert!(word < *at);
        let moved = (i64::from(be32(&spliced, word)) + delta) as u32;
        spliced[word..word + 4].copy_from_slice(&moved.to_be_bytes());
    }
    let path = write(&directory, "gradient32.psd", &spliced);
    let sentence = "Layer 1 (“Fill”) is a Photoshop Gradient fill layer, which Doctored does not read at 32 bits; open the stored composite.";
    let report = import_report(&path);
    assert_eq!(report["layered_available"], false, "{report}");
    assert!(
        report["layered_refusal"]
            .as_str()
            .is_some_and(|text| text.contains(sentence)),
        "{report}"
    );
    let error = open(&path).err().expect("the layered open refuses");
    assert!(error.contains(sentence), "{error}");
}
