//! Solid Color fill layers in PSD: our 8- and 16-bit RGB and CMYK
//! layered exports write a solid color fill layer in Photoshop's `SoCo` form —
//! its rendered pixels over its mask's rectangle (the canvas without a mask),
//! its `-2` mask, flags `0x18` and the `SoCo` block, its colour the
//! DOCUMENT's numbers (`RGBC`, or `CMYC` ink) — and our importer reads every
//! form back as the same fill layer: its colour, mask and opacity kept, the
//! report observing it. A colour outside sRGB (Display P3 green) crosses as
//! the wide `linear` colour. A `SoCo` that does not decode is omitted with
//! its reason when pixel-less and stays a raster with its marker losses when
//! it has pixels; a file whose profile cannot be rendered into declines its
//! solids the same way while its pattern fill is still carried. A carried
//! solid is admitted as a canvas raster.
//!
//! Oracles are independent: an sRGB file's `RGBC` is the colour · 255; the
//! Display P3 ↔ sRGB matrix is built from the published primaries (both
//! D65, `common/p3_oracle.rs`), never read from the core; the written planes
//! are the document's own samples over the record's rectangle.

use std::ffi::CString;
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::ffi_artwork::{dr_artwork_render, dr_doc_artwork, dr_precision_doc_artwork};
use doctored_core::ffi_cmyk_artwork::dr_precision_doc_cmyk_artwork;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{GrayImage, Luma};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_psd_fixture::{self as cmyk_psd, Comp, PsdSpec, Rec};
use crate::common::p3_oracle::p3_to_linear_srgb;
use crate::common::psd_photoshop::{blocks_in, rgbc_numbers};
use crate::common::{solid, take_err_string};
use crate::icc_lut_builders as builders;
use crate::psd16_fixture as fixture16;
use crate::psd_descriptor_fixture as builder;
use crate::psd_mask_fixture as fixture8;
use crate::psd_pattern_fill_cmyk::{
    description as cmyk_description, free, open, save_layers, then,
};
use crate::psd_pattern_fill_layer::{
    framed_records, import_report, open16, open8, owned, pixel_less, plane, save16, save8,
    try_open16,
};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 96;
const H: u32 = 64;
/// The masked fill's mask: (10, 6), 60 × 44.
const MASK_AT: (i32, i32) = (10, 6);
const MASK_SIZE: (u32, u32) = (60, 44);
const MASKED_RECT: [i32; 4] = [6, 10, 50, 70];
const CANVAS_RECT: [i32; 4] = [0, 0, H as i32, W as i32];

/// The masked fill's colour, on the byte grid, and the unmasked one's, off it.
const A: [f64; 3] = [40.0 / 255.0, 90.0 / 255.0, 200.0 / 255.0];
const B: [f64; 3] = [0.3, 0.6, 0.15];

/// The CMYK fills' inks.
const INK_A: [f64; 4] = [0.7, 0.2, 0.1, 0.05];
const INK_B: [f64; 4] = [0.1, 0.55, 0.9, 0.0];

// ---------------------------------------------------------------- plumbing --

/// One RGB solid fill: its name, authored sRGB colour and optional wide one.
struct Fill {
    name: &'static str,
    color: [f64; 3],
    linear: Option<[f64; 3]>,
}

impl Fill {
    fn new(name: &'static str, color: [f64; 3]) -> Self {
        Fill {
            name,
            color,
            linear: None,
        }
    }

    fn description(&self) -> String {
        let mut d = json!({"type": "fill", "version": 1, "fill": "solid",
            "color": [self.color[0], self.color[1], self.color[2], 1.0], "canvas": [W, H]});
        if let Some(linear) = self.linear {
            d["linear"] = json!(linear);
        }
        d.to_string()
    }

    fn components(&self) -> String {
        let mut c = json!({"kind": "fill",
            "color": [self.color[0], self.color[1], self.color[2], 1.0]});
        if let Some(linear) = self.linear {
            c["linear"] = json!(linear);
        }
        json!([c]).to_string()
    }
}

fn render(profile: &Arc<IccProfile>, components: &str) -> PrecisionImage {
    let components = CString::new(components).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_artwork_render(
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
    assert_eq!(owned(err), None);
    *unsafe { Box::from_raw(out) }
}

/// The masked fill's mask samples, varied so a decoded plane cannot match by
/// accident.
fn mask_value(x: u32, y: u32) -> u8 {
    ((x * 4 + y * 3) % 251) as u8 + 4
}

fn mask8() -> GrayImage {
    GrayImage::from_fn(MASK_SIZE.0, MASK_SIZE.1, |x, y| Luma([mask_value(x, y)]))
}

/// White, then each fill in `profile`; the first masked (`mask8` at
/// `MASK_AT`) at opacity 0.6.
fn byte_doc(profile: &Arc<IccProfile>, fills: &[Fill]) -> DrDocument {
    let mut doc = DrDocument::from_pixels(solid(W, H, [255; 4]));
    doc.profile = profile.clone();
    for (index, fill) in fills.iter().enumerate() {
        let image = render(profile, &fill.components());
        let name = CString::new(fill.name).unwrap();
        let text = CString::new(fill.description()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_doc_artwork(
                &doc,
                index,
                false,
                &image,
                0,
                0,
                name.as_ptr(),
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert_eq!(owned(err), None);
        doc = *unsafe { Box::from_raw(out) };
    }
    let mut doc = doc
        .add_mask(1, MaskKind::FromSelection(&vec![255u8; (W * H) as usize]))
        .expect("the mask");
    doc.layers[1].mask = Some(Arc::new(mask8()));
    doc.layers[1].mask_properties = doctored_core::doc_mask::MaskProperties::at(MASK_AT);
    doc.with_layer_opacity(1, 0.6).expect("the opacity")
}

/// `byte_doc` at 16 bits: the mask's samples are `mask_value` · 257.
fn typed_doc(profile: &Arc<IccProfile>, fills: &[Fill]) -> PrecisionDocument {
    let white = PrecisionImage::from_rgba16(
        W,
        H,
        vec![[65535; 4]; (W * H) as usize],
        profile.clone(),
        None,
    )
    .unwrap();
    let base = Layer::raster("Base", white, (0, 0), 1.0, true).unwrap();
    let mut doc = PrecisionDocument::from_parts(
        W,
        H,
        profile.clone(),
        Metadata::default(),
        Resolution::default(),
        vec![base],
    )
    .unwrap();
    for (index, fill) in fills.iter().enumerate() {
        let image = render(profile, &fill.components());
        let name = CString::new(fill.name).unwrap();
        let text = CString::new(fill.description()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_artwork(
                &doc,
                index,
                false,
                &image,
                0,
                0,
                name.as_ptr(),
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert_eq!(owned(err), None);
        doc = *unsafe { Box::from_raw(out) };
    }
    let mut layers: Vec<Layer> = (0..doc.layer_count())
        .map(|i| doc.layer(i).unwrap().clone())
        .collect();
    let samples = (0..MASK_SIZE.1)
        .flat_map(|y| (0..MASK_SIZE.0).map(move |x| u16::from(mask_value(x, y)) * 257))
        .collect();
    let plane = doctored_core::precision_mask::PrecisionPlane::from_u16(
        MASK_SIZE.0,
        MASK_SIZE.1,
        samples,
        None,
    )
    .unwrap();
    let mask = doctored_core::precision_mask::PrecisionMask::new(
        plane,
        doctored_core::doc_mask::MaskProperties::at(MASK_AT),
        true,
    )
    .unwrap();
    layers[1] = layers[1].clone().with_mask(Some(mask));
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        profile.clone(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    doc.set_layer_opacity(1, 0.6).unwrap().expect("the opacity")
}

/// The record named `name` in a written file.
pub(crate) fn record<'a>(
    records: &'a [crate::psd_pattern_fill_layer::Framed],
    name: &str,
) -> &'a crate::psd_pattern_fill_layer::Framed {
    records
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("the record {name}"))
}

/// One block's body of a framed record.
pub(crate) fn block<'a>(
    record: &'a crate::psd_pattern_fill_layer::Framed,
    key: &[u8; 4],
) -> &'a [u8] {
    &record
        .blocks
        .iter()
        .find(|(k, _)| k == key)
        .unwrap_or_else(|| panic!("{}: {}", record.name, String::from_utf8_lossy(key)))
        .1
}

/// The written record named `name` is Photoshop's `SoCo` form: rectangle
/// `rect`, bit 4 beside bit 3, `SoCo` and no `PtFl`, no `fxrp` (it was never
/// moved), a `-2` channel exactly when `mask` is given and then holding
/// those samples, and its other channels (`ids`, each a component of
/// `pixel`) holding the layer's own samples over `rect` at `sample` bytes.
#[allow(clippy::too_many_arguments)]
fn assert_record(
    bytes: &[u8],
    name: &str,
    rect: [i32; 4],
    mask: Option<&dyn Fn(u32, u32) -> u16>,
    ids: &[(i16, usize)],
    pixel: &dyn Fn(u32, u32, usize) -> u16,
    sample: usize,
    what: &str,
) {
    let records = framed_records(bytes);
    let r = record(&records, name);
    assert_eq!(r.bounds, rect, "{what}: {name}'s rectangle");
    assert_eq!(r.flags & 0x18, 0x18, "{what}: {name}'s bit 4");
    assert!(r.blocks.iter().any(|(k, _)| k == b"SoCo"), "{what}: SoCo");
    assert!(
        !r.blocks.iter().any(|(k, _)| k == b"PtFl"),
        "{what}: no PtFl"
    );
    assert!(
        !r.blocks.iter().any(|(k, _)| k == b"fxrp"),
        "{what}: no fxrp"
    );
    let (w, h) = ((rect[3] - rect[1]) as usize, (rect[2] - rect[0]) as usize);
    let read = |stored: &[u8], w: usize, h: usize, at: usize| {
        let samples = plane(stored, w, h, sample);
        if sample == 1 {
            u16::from(samples[at])
        } else {
            u16::from_be_bytes([samples[at * 2], samples[at * 2 + 1]])
        }
    };
    let channel = |id: i16| {
        &r.channels
            .iter()
            .find(|(c, _)| *c == id)
            .unwrap_or_else(|| panic!("{what}: {name}'s channel {id}"))
            .1
    };
    assert_eq!(
        r.channels.iter().any(|(id, _)| *id == -2),
        mask.is_some(),
        "{what}: {name}'s mask channel"
    );
    if let Some(mask) = mask {
        assert_eq!(r.mask, Some((rect, 0)), "{what}: the mask's rectangle");
        let stored = channel(-2);
        for y in 0..MASK_SIZE.1 {
            for x in 0..MASK_SIZE.0 {
                let at = (y * MASK_SIZE.0 + x) as usize;
                let got = read(stored, MASK_SIZE.0 as usize, MASK_SIZE.1 as usize, at);
                assert_eq!(got, mask(x, y), "{what}: {name}'s -2 at ({x},{y})");
            }
        }
    }
    for &(id, component) in ids {
        let stored = channel(id);
        for j in 0..h {
            for i in 0..w {
                let (cx, cy) = (rect[1] as u32 + i as u32, rect[0] as u32 + j as u32);
                let got = read(stored, w, h, j * w + i);
                assert_eq!(
                    got,
                    pixel(cx, cy, component),
                    "{what}: {name} channel {id} at ({cx},{cy})"
                );
            }
        }
    }
}

/// The written `SoCo`'s `RGBC` numbers.
fn written_rgbc(bytes: &[u8], name: &str) -> [f64; 3] {
    let records = framed_records(bytes);
    rgbc_numbers(block(record(&records, name), b"SoCo"))
}

/// A reopened description's numbers under `key`.
fn numbers(d: &Value, key: &str) -> Vec<f64> {
    d[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key}: {d}"))
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

fn parse(description: &str) -> Value {
    serde_json::from_str(description).expect("description JSON")
}

// ------------------------------------------------------------ round trips --

#[test]
fn an_8_and_16_bit_solid_fill_layer_round_trips_in_photoshops_soco_form() {
    let dir = TempDir::new().unwrap();
    let srgb = IccProfile::srgb();
    let fills = [Fill::new("Fill 0", A), Fill::new("Fill 1", B)];
    let rgb_ids = [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)];

    // 8 bits.
    let doc = byte_doc(&srgb, &fills);
    let path = dir.path().join("solid8.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    let layer_pixels: Vec<image::RgbaImage> = (1..3)
        .map(|i| doc.layers[i].pixels.to_contiguous().unwrap())
        .collect();
    for (index, (name, rect)) in [("Fill 0", MASKED_RECT), ("Fill 1", CANVAS_RECT)]
        .into_iter()
        .enumerate()
    {
        let mask = |x: u32, y: u32| u16::from(mask_value(x, y));
        assert_record(
            &bytes,
            name,
            rect,
            (index == 0).then_some(&mask as &dyn Fn(u32, u32) -> u16),
            &rgb_ids,
            &|x, y, c| u16::from(layer_pixels[index].get_pixel(x, y).0[c]),
            1,
            "psd8",
        );
        let color = fills[index].color;
        for (n, c) in written_rgbc(&bytes, name).iter().zip(color) {
            assert!(
                (n - c * 255.0).abs() <= 1e-4,
                "psd8 {name}: RGBC {n} vs {c}"
            );
        }
    }
    let report = import_report(&path);
    let observed = report["observations"].to_string();
    for name in ["Fill 0", "Fill 1"] {
        assert!(
            observed.contains(&format!(
                "(“{name}”) is a Photoshop Solid Color Fill layer (SoCo) in Photoshop's form"
            )),
            "{observed}"
        );
    }
    assert!(
        !report["verified_losses"].to_string().contains("Fill"),
        "{}",
        report["verified_losses"]
    );
    let reopened = open8(&path);
    assert_eq!(reopened.layers.len(), 3);
    for index in 1..3 {
        let layer = &reopened.layers[index];
        let d = parse(layer.meta.as_deref().expect("a description"));
        assert_eq!(d["fill"], "solid");
        assert!(d.get("linear").is_none(), "no wide colour");
        for (got, want) in numbers(&d, "color").iter().zip(fills[index - 1].color) {
            assert!(
                (got - want).abs() <= 1.0 / 510.0,
                "psd8 {index}: {got} vs {want}"
            );
        }
        assert_eq!(layer.offset, (0, 0));
        assert_eq!(
            layer.pixels.to_contiguous().unwrap(),
            layer_pixels[index - 1],
            "psd8 {index}: pixels"
        );
    }
    assert!((reopened.layers[1].opacity - 0.6).abs() < 1e-3, "opacity");
    assert_eq!(
        reopened.layers[1]
            .mask
            .as_deref()
            .map(|m| m.as_raw().clone()),
        Some(mask8().into_raw()),
        "the mask"
    );
    assert_eq!(reopened.layers[1].mask_properties.offset, MASK_AT);
    assert_eq!(reopened.flattened().unwrap(), doc.flattened().unwrap());

    // 16 bits.
    let doc = typed_doc(&srgb, &fills);
    let path = dir.path().join("solid16.psd");
    save16(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    let samples: Vec<Vec<[u16; 4]>> = (1..3)
        .map(|i| doc.layer(i).unwrap().source().unwrap().to_rgba16().unwrap())
        .collect();
    for (index, (name, rect)) in [("Fill 0", MASKED_RECT), ("Fill 1", CANVAS_RECT)]
        .into_iter()
        .enumerate()
    {
        let mask = |x: u32, y: u32| u16::from(mask_value(x, y)) * 257;
        assert_record(
            &bytes,
            name,
            rect,
            (index == 0).then_some(&mask as &dyn Fn(u32, u32) -> u16),
            &rgb_ids,
            &|x, y, c| samples[index][(y * W + x) as usize][c],
            2,
            "psd16",
        );
        let color = fills[index].color;
        for (n, c) in written_rgbc(&bytes, name).iter().zip(color) {
            assert!(
                (n - c * 255.0).abs() <= 1e-4,
                "psd16 {name}: RGBC {n} vs {c}"
            );
        }
    }
    let report = import_report(&path);
    assert!(
        !report["verified_losses"].to_string().contains("Fill"),
        "{}",
        report["verified_losses"]
    );
    let reopened = open16(&path);
    assert_eq!(reopened.layer_count(), 3);
    for index in 1..3 {
        let d = parse(reopened.artwork_description(index).expect("a description"));
        assert_eq!(d["fill"], "solid");
        for (got, want) in numbers(&d, "color").iter().zip(fills[index - 1].color) {
            assert!(
                (got - want).abs() <= 1.0 / 510.0,
                "psd16 {index}: {got} vs {want}"
            );
        }
        let layer = reopened.layer(index).unwrap();
        assert_eq!(layer.offset(), (0, 0));
        let back = layer.source().unwrap().to_rgba16().unwrap();
        for (got, want) in back.iter().zip(&samples[index - 1]) {
            for c in 0..4 {
                assert!(
                    (i32::from(got[c]) - i32::from(want[c])).abs() <= 1,
                    "psd16 {index}: {got:?} vs {want:?}"
                );
            }
        }
    }
    let masked = reopened.layer(1).unwrap();
    assert!((masked.opacity() - 0.6).abs() < 1e-3, "opacity");
    let mask = masked.mask().expect("the mask");
    assert_eq!(mask.properties().offset, MASK_AT);
    assert_eq!(
        mask.plane().samples().to_vec(),
        doc.layer(1)
            .unwrap()
            .mask()
            .unwrap()
            .plane()
            .samples()
            .to_vec()
    );
}

/// A pixel-less `SoCo` body whose `Clr ` is an `RGBC` of `rgb` (0…255).
fn rgbc_body(rgb: [f64; 3]) -> Vec<u8> {
    builder::body(
        16,
        b"null",
        &[builder::rgbc(b"Clr ", rgb[0], rgb[1], rgb[2])],
    )
}

/// A `SoCo` body whose `Clr ` is a `CMYC` of `ink` (percent).
fn cmyc_body(ink: [f64; 4]) -> Vec<u8> {
    builder::body(
        16,
        b"null",
        &[builder::objc(
            b"Clr ",
            b"CMYC",
            &[
                builder::doub(b"Cyn ", ink[0]),
                builder::doub(b"Mgnt", ink[1]),
                builder::doub(b"Ylw ", ink[2]),
                builder::doub(b"Blck", ink[3]),
            ],
        )],
    )
}

#[test]
fn a_display_p3_green_crosses_as_a_wide_colour_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let p3 = IccProfile::display_p3();
    let oracle = p3_to_linear_srgb([0.0, 1.0, 0.0]);
    assert!(
        oracle[0] < -0.1 && oracle[2] < -0.01,
        "outside sRGB: {oracle:?}"
    );
    let check_linear = |d: &Value, what: &str| {
        for (got, want) in numbers(d, "linear").iter().zip(oracle) {
            assert!((got - want).abs() <= 1e-4, "{what}: linear {got} vs {want}");
        }
    };

    // 16 bits: a hand-built Display P3 file holding Photoshop's pixel-less
    // `SoCo` of the document's (0, 255, 0).
    let mut fill = fixture16::Record::raster(
        "Green",
        [0; 4],
        [-1i16, 0, 1, 2]
            .into_iter()
            .map(|id| (id, fixture16::raw_plane(&[])))
            .collect(),
    );
    fill.blocks = fixture16::block(b"SoCo", &rgbc_body([0.0, 255.0, 0.0]));
    let white = vec![[65535u16; 4]; (W * H) as usize];
    let base = fixture16::Record::raster(
        "Base",
        CANVAS_RECT,
        [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
            .into_iter()
            .map(|(id, c)| {
                let plane: Vec<u16> = white.iter().map(|p| p[c]).collect();
                (id, fixture16::raw_plane(&plane))
            })
            .collect(),
    );
    let path = dir.path().join("p3-16.psd");
    std::fs::write(
        &path,
        fixture16::fixture(
            (W, H),
            &[base, fill],
            p3.bytes(),
            (72.0, 72.0),
            &white,
            true,
            false,
        ),
    )
    .unwrap();
    let report = import_report(&path);
    assert!(
        report["observations"]
            .to_string()
            .contains("(“Green”) is a Photoshop Solid Color Fill layer (SoCo) with no pixels; layered import carries it as a Doctored solid color fill layer with its colour, mask, opacity, blend mode and clipping. Its colour lies outside sRGB and is kept as a wide colour."),
        "{}",
        report["observations"]
    );
    let doc = open16(&path);
    let d = parse(doc.artwork_description(1).expect("a description"));
    check_linear(&d, "p3-16");
    let pixels = doc.layer(1).unwrap().source().unwrap().to_rgba16().unwrap();
    for p in &pixels {
        for (got, want) in p.iter().zip([0u16, 65535, 0, 65535]) {
            assert!(
                (i32::from(*got) - i32::from(want)).abs() <= 1,
                "p3-16: {p:?}"
            );
        }
    }
    let again = dir.path().join("p3-16-again.psd");
    save16(&doc, &again);
    for (n, want) in written_rgbc(&std::fs::read(&again).unwrap(), "Green")
        .iter()
        .zip([0.0, 255.0, 0.0])
    {
        assert!((n - want).abs() <= 1.0, "p3-16 re-export: {n} vs {want}");
    }

    // 8 bits: a Display P3 byte document whose fill's description holds the
    // wide colour, exported and reopened.
    let green = Fill {
        name: "Green",
        color: [0.0, 1.0, 0.0],
        linear: Some(oracle),
    };
    let doc = byte_doc(&p3, &[green, Fill::new("Plain", B)]);
    let path = dir.path().join("p3-8.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    for (n, want) in written_rgbc(&bytes, "Green").iter().zip([0.0, 255.0, 0.0]) {
        assert!((n - want).abs() <= 1.0, "p3-8: RGBC {n} vs {want}");
    }
    let reopened = open8(&path);
    let layer = &reopened.layers[1];
    let d = parse(layer.meta.as_deref().expect("a description"));
    check_linear(&d, "p3-8");
    for p in layer.pixels.to_contiguous().unwrap().pixels() {
        for (got, want) in p.0.iter().zip([0u8, 255, 0, 255]) {
            assert!(
                (i32::from(*got) - i32::from(want)).abs() <= 1,
                "p3-8: {p:?}"
            );
        }
    }
    // An in-gamut colour in the same file carries no wide colour.
    let plain = parse(reopened.layers[2].meta.as_deref().expect("a description"));
    assert!(plain.get("linear").is_none(), "{plain}");
    let again = dir.path().join("p3-8-again.psd");
    save8(&reopened, &again);
    for (n, want) in written_rgbc(&std::fs::read(&again).unwrap(), "Green")
        .iter()
        .zip([0.0, 255.0, 0.0])
    {
        assert!((n - want).abs() <= 1.0, "p3-8 re-export: {n} vs {want}");
    }
}

// ------------------------------------------------------------------- CMYK --

/// One CMYK solid published as layer `layer` + 1.
fn publish_ink(
    doc: *mut PrecisionDocument,
    layer: usize,
    ink: [f64; 4],
    name: &str,
) -> *mut PrecisionDocument {
    let color = [ink[0], ink[1], ink[2], ink[3], 1.0];
    let components = CString::new(json!([{"kind": "fill", "ink": color}]).to_string()).unwrap();
    let description = CString::new(
        json!({"type": "fill", "version": 2, "fill": "solid", "ink": "cmyk",
               "color": color, "canvas": [W, H]})
        .to_string(),
    )
    .unwrap();
    let name = CString::new(name).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_artwork(
            doc,
            layer,
            false,
            0,
            W,
            H,
            components.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
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
    assert!(!out.is_null(), "publish: {}", take_err_string(err));
    free(doc);
    out
}

/// Paper, then `INK_A` (masked, varied mask) and `INK_B`, at `depth`.
fn cmyk_doc(depth: u32) -> *mut PrecisionDocument {
    let profile = press_profile("Press Test");
    let request = format!(r#"{{"depth":{depth},"fill":[0,0,0,0,1],"layer_name":"Paper"}}"#);
    let doc = PrecisionDocument::new_cmyk(W, H, Some(&profile), &request, LIMIT, 0).unwrap();
    let doc = Box::into_raw(Box::new(doc));
    let doc = publish_ink(doc, 0, INK_A, "Fill 0");
    let mask: Vec<u16> = (0..MASK_SIZE.1)
        .flat_map(|y| (0..MASK_SIZE.0).map(move |x| u16::from(mask_value(x, y)) * 257))
        .collect();
    let doc = then(
        doc,
        &json!({"op": "set_mask", "layer": 1, "width": MASK_SIZE.0, "height": MASK_SIZE.1,
                "x": MASK_AT.0, "y": MASK_AT.1}),
        &mask,
    );
    publish_ink(doc, 1, INK_B, "Fill 1")
}

#[test]
fn a_cmyk_solid_fill_layer_round_trips_its_ink_in_cmyc() {
    let dir = TempDir::new().unwrap();
    for depth in [8u32, 16] {
        let what = format!("cmyk{depth}");
        let doc = cmyk_doc(depth);
        let path = dir.path().join(format!("{what}.psd"));
        save_layers(doc, &path);
        let bytes = std::fs::read(&path).unwrap();
        let sample = (depth / 8) as usize;
        let max: u16 = if depth == 8 { 255 } else { 65535 };
        let samples: Vec<Vec<[u16; 5]>> = (1..3)
            .map(|i| unsafe { &*doc }.cmyk_layer_samples(i).unwrap())
            .collect();
        let at_depth = |v: u16| {
            if depth == 8 {
                ((u32::from(v) + 128) / 257) as u16
            } else {
                v
            }
        };
        for (index, (name, rect, ink)) in [
            ("Fill 0", MASKED_RECT, INK_A),
            ("Fill 1", CANVAS_RECT, INK_B),
        ]
        .into_iter()
        .enumerate()
        {
            let mask = |x: u32, y: u32| {
                let v = u16::from(mask_value(x, y));
                if depth == 8 {
                    v
                } else {
                    v * 257
                }
            };
            assert_record(
                &bytes,
                name,
                rect,
                (index == 0).then_some(&mask as &dyn Fn(u32, u32) -> u16),
                &[(-1i16, 4usize), (0, 0), (1, 1), (2, 2), (3, 3)],
                &|x, y, c| {
                    let v = at_depth(samples[index][(y * W + x) as usize][c]);
                    if c == 4 {
                        v
                    } else {
                        max - v
                    }
                },
                sample,
                &what,
            );
            // `CMYC` is the stored ink in percent to four decimals.
            let records = framed_records(&bytes);
            let body = block(record(&records, name), b"SoCo");
            for (key, want) in [b"Cyn doub", b"Mgntdoub", b"Ylw doub", b"Blckdoub"]
                .iter()
                .zip(ink)
            {
                let at = body.windows(8).position(|w| w == *key).expect("CMYC key");
                let got = f64::from_be_bytes(body[at + 8..at + 16].try_into().unwrap());
                assert!(
                    (got - want * 100.0).abs() <= 5e-5,
                    "{what} {name}: {got} vs {want}"
                );
                assert_eq!((got * 1e4).round() / 1e4, got, "{what}: four decimals");
            }
        }
        let report = import_report(&path);
        let observed = report["observations"].to_string();
        assert!(
            observed.contains(
                "(“Fill 0”) is a Photoshop Solid Color Fill layer (SoCo) in Photoshop's form"
            ),
            "{what}: {observed}"
        );
        assert!(
            !report["verified_losses"].to_string().contains("Fill"),
            "{what}: {}",
            report["verified_losses"]
        );
        let back = open(&path);
        let reopened = unsafe { &*back };
        assert_eq!(reopened.layer_count(), 3, "{what}");
        for (index, ink) in [(1, INK_A), (2, INK_B)] {
            let d = cmyk_description(back, index).expect("a description");
            assert_eq!(d["version"], 2, "{what}");
            assert_eq!(d["ink"], "cmyk", "{what}");
            let color = numbers(&d, "color");
            for (got, want) in color.iter().zip(ink) {
                assert!(
                    (got - want).abs() <= 1e-4,
                    "{what} {index}: {got} vs {want}"
                );
            }
            assert_eq!(color[4], 1.0);
            assert_eq!(
                reopened.cmyk_layer_samples(index),
                Some(samples[index - 1].clone()),
                "{what} {index}: samples"
            );
        }
        let mask = reopened.layer(1).unwrap().mask().expect("the mask");
        assert_eq!(mask.properties().offset, MASK_AT, "{what}");
        free(back);
        free(doc);
    }
}

// --------------------------------------------------------- crafted files --

#[test]
fn every_soco_form_is_read_and_named_in_the_report() {
    let dir = TempDir::new().unwrap();
    let doc = byte_doc(
        &IccProfile::srgb(),
        &[Fill::new("Fill 0", A), Fill::new("Fill 1", B)],
    );
    let path = dir.path().join("solid8.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    let mut unflagged = bytes.clone();
    for record in framed_records(&bytes) {
        if record.name.starts_with("Fill") {
            assert_eq!(record.flags, 0x18);
            unflagged[record.flags_at] = 0x08;
        }
    }
    let unflagged_path = dir.path().join("unflagged.psd");
    std::fs::write(&unflagged_path, &unflagged).unwrap();
    let less_path = dir.path().join("pixel-less.psd");
    std::fs::write(&less_path, pixel_less(&bytes, "Fill 0")).unwrap();
    for (path, name, clause) in [
        (
            &unflagged_path,
            "Fill 0",
            "with pixels it does not mark irrelevant (no flags bit 4)",
        ),
        (&less_path, "Fill 0", "with no pixels;"),
        (&less_path, "Fill 1", "in Photoshop's form"),
    ] {
        let report = import_report(path);
        let observed = report["observations"].to_string();
        assert!(
            observed.contains(&format!(
                "(“{name}”) is a Photoshop Solid Color Fill layer (SoCo) {clause}"
            )),
            "{name} {clause}: {observed}"
        );
        assert!(
            !report["verified_losses"].to_string().contains("Fill"),
            "{}",
            report["verified_losses"]
        );
        let reopened = open8(path);
        assert_eq!(reopened.layers.len(), 3, "{clause}");
        assert_eq!(
            reopened.flattened().unwrap(),
            doc.flattened().unwrap(),
            "{clause}"
        );
        assert_eq!(
            reopened.layers[1]
                .mask
                .as_deref()
                .map(|m| m.as_raw().clone()),
            Some(mask8().into_raw()),
            "{clause}: the mask"
        );
    }
}

/// A pixel-less 8-bit record carrying `key`/`body`.
fn empty8(name: &'static str, key: &[u8; 4], body: &[u8]) -> fixture8::Record {
    let mut record = fixture8::Record::raster([0; 4], &[], false);
    record.name = name;
    record.blocks = fixture8::block(key, body);
    record
}

#[test]
fn a_soco_that_does_not_decode_is_omitted_or_stays_a_raster() {
    let dir = TempDir::new().unwrap();
    // RGB: an ink colour and a bad version, pixel-less; an ink colour on a
    // raster in Photoshop's form.
    let mut base = fixture8::Record::raster([0, 0, 2, 2], &[200; 16], false);
    base.name = "Base";
    let mut raster = fixture8::Record::raster([0, 0, 2, 2], &[90; 16], false);
    raster.name = "Raster";
    raster.flags = 0x18;
    raster.blocks = fixture8::block(b"SoCo", &cmyc_body([10.0, 20.0, 30.0, 0.0]));
    let mut bad = rgbc_body([1.0, 2.0, 3.0]);
    bad[3] = 15;
    // Photoshop's SHAPE layer under an INVERTED vector mask (`vmsk`, version
    // 3, flags bit 0, no path records), which layered import does not carry
    // (`psd_paths`; a carried one is `psd_paths.rs`'s).
    let mut shape = fixture8::Record::raster([0, 0, 2, 2], &[60; 16], false);
    shape.name = "Shape";
    shape.flags = 0x18;
    shape.blocks = fixture8::block(b"SoCo", &rgbc_body([40.0, 90.0, 200.0]));
    shape
        .blocks
        .extend(fixture8::block(b"vmsk", &[0, 0, 0, 3, 0, 0, 0, 1]));
    let records = vec![
        base,
        empty8("Ink", b"SoCo", &cmyc_body([10.0, 20.0, 30.0, 0.0])),
        empty8("Version", b"SoCo", &bad),
        raster,
        shape,
    ];
    let path = dir.path().join("declined8.psd");
    std::fs::write(&path, fixture8::fixture(&records, [2, 2], false)).unwrap();
    let report = import_report(&path);
    let losses = report["verified_losses"].to_string();
    for (name, reason) in [
        ("Ink", "colour is not an RGB colour"),
        ("Version", "descriptor version is not 16"),
    ] {
        assert!(
            losses.contains(&format!("(“{name}”) is a Photoshop Solid Color Fill layer (SoCo) with no pixels whose {reason}; layered import omits it and its mask. The stored composite keeps the saved appearance.")),
            "{name}: {losses}"
        );
    }
    for fragment in [
        "(“Raster”) has adjustment/fill metadata",
        "(“Raster”) declares its raster pixels irrelevant",
        "(“Shape”) has adjustment/fill metadata",
    ] {
        assert!(losses.contains(fragment), "{fragment}: {losses}");
    }
    assert!(
        !report["observations"]
            .to_string()
            .contains("layered import carries it"),
        "nothing is carried"
    );
    let doc = open8(&path);
    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Base", "Raster", "Shape"]);
    assert!(
        doc.layers[1].meta.is_none(),
        "the raster keeps its pixels only"
    );
    assert!(
        doc.layers[2].meta.is_none(),
        "the shape keeps its pixels only"
    );
    assert_eq!(
        doc.layers[2].pixels.to_contiguous().unwrap().as_raw(),
        &vec![60u8; 16],
        "the shape's own pixels"
    );

    // CMYK: an RGB colour, pixel-less and on a raster.
    let samples = vec![[1000u16, 2000, 3000, 4000, 65535]; 4];
    let channels = cmyk_psd::ink_channels(&samples, 2, 8, true, Comp::Raw, false);
    let mut raster = Rec::raster("Raster", [0, 0, 2, 2], channels.clone());
    raster.flags = 0x18;
    let records = vec![
        Rec::raster("Base", [0, 0, 2, 2], channels),
        Rec::empty("Rgb").with_block(b"SoCo", &rgbc_body([1.0, 2.0, 3.0])),
        raster.with_block(b"SoCo", &rgbc_body([1.0, 2.0, 3.0])),
    ];
    let profile = press_profile("Press Test");
    let bytes = cmyk_psd::psd(
        &PsdSpec {
            canvas: (2, 2),
            depth: 8,
            icc: Some(&profile),
            composite: (0..4).map(|_| vec![255; 4]).collect(),
            merged_alpha: false,
            large: false,
            lr16: false,
            resources: Vec::new(),
        },
        &records,
    );
    let path = dir.path().join("declined-cmyk.psd");
    std::fs::write(&path, bytes).unwrap();
    let losses = import_report(&path)["verified_losses"].to_string();
    assert!(
        losses.contains("(“Rgb”) is a Photoshop Solid Color Fill layer (SoCo) with no pixels whose colour is not a CMYK colour; layered import omits it and its mask."),
        "{losses}"
    );
    assert!(
        losses.contains("(“Raster”) has adjustment/fill metadata"),
        "{losses}"
    );
    let back = open(&path);
    let reopened = unsafe { &*back };
    assert_eq!(reopened.layer_count(), 2, "the pixel-less one is omitted");
    assert!(
        cmyk_description(back, 1).is_none(),
        "the raster keeps its pixels only"
    );
    free(back);
}

/// An RGB matrix/TRC profile whose TRCs are a type-3 `para` that steps down
/// at its branch boundary (`encode_refusals_falling.rs`'s): an 8-bit
/// document may carry it, but nothing can be encoded into it at high depth.
pub(crate) fn falling_profile() -> Vec<u8> {
    let mut curve = b"para\0\0\0\0".to_vec();
    curve.extend_from_slice(&3u16.to_be_bytes());
    curve.extend_from_slice(&[0, 0]);
    for p in [2.2f64, 0.9, 0.1, 1.0, 0.5] {
        curve.extend_from_slice(&((p * 65536.0).round() as i32).to_be_bytes());
    }
    builders::build(
        [0x02, 0x10, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", builders::tag_desc("Synthetic Falling Boundary")),
            (b"rXYZ", builders::tag_wtpt([0.4361, 0.2225, 0.0139])),
            (b"gXYZ", builders::tag_wtpt([0.3851, 0.7169, 0.0971])),
            (b"bXYZ", builders::tag_wtpt([0.1431, 0.0606, 0.7141])),
            (b"rTRC", curve.clone()),
            (b"gTRC", curve.clone()),
            (b"bTRC", curve),
        ],
    )
}

/// `bytes` (a classic PSD) with its ICC resource (1039) replaced by `icc`.
pub(crate) fn retag(bytes: &[u8], icc: &[u8]) -> Vec<u8> {
    let be32 = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut at = 26;
    at += 4 + be32(at);
    let (start, end) = (at + 4, at + 4 + be32(at));
    let mut resources = Vec::new();
    let mut c = start;
    while c < end {
        let id = u16::from_be_bytes([bytes[c + 4], bytes[c + 5]]);
        let name = (usize::from(bytes[c + 6]) + 2) & !1;
        let size = be32(c + 6 + name);
        let next = c + 6 + name + 4 + size + size % 2;
        if id != 1039 {
            resources.extend(&bytes[c..next]);
        }
        c = next;
    }
    resources.extend(b"8BIM");
    resources.extend(1039u16.to_be_bytes());
    resources.extend([0, 0]);
    resources.extend((icc.len() as u32).to_be_bytes());
    resources.extend(icc);
    if icc.len() % 2 == 1 {
        resources.push(0);
    }
    let mut out = bytes[..at].to_vec();
    out.extend((resources.len() as u32).to_be_bytes());
    out.extend(resources);
    out.extend(&bytes[end..]);
    out
}

/// A 1 × 1 pattern fill's keys (tile (10, 200, 30)).
fn pattern_keys() -> Value {
    json!({"pattern": {"name": "P", "width": 1, "height": 1, "rgba": "Csge/w=="},
           "angle": 0, "scale": 1, "link_with_layer": false, "phase": [0, 0]})
}

#[test]
fn a_profile_that_cannot_be_rendered_into_declines_its_solids_and_keeps_its_pattern() {
    let dir = TempDir::new().unwrap();
    let srgb = IccProfile::srgb();
    let mut doc = byte_doc(&srgb, &[Fill::new("Less", A), Fill::new("Raster", B)]);
    // A pattern fill on top.
    let image = render(
        &srgb,
        &json!([{"kind": "pattern", "fill": pattern_keys()}]).to_string(),
    );
    let name = CString::new("Pattern").unwrap();
    let text = CString::new(
        json!({"type": "fill", "version": 1, "fill": "pattern",
               "pattern_fill": pattern_keys(), "canvas": [W, H]})
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_artwork(
            &doc,
            2,
            false,
            &image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    doc = *unsafe { Box::from_raw(out) };
    let path = dir.path().join("srgb.psd");
    save8(&doc, &path);
    let bytes = retag(
        &pixel_less(&std::fs::read(&path).unwrap(), "Less"),
        &falling_profile(),
    );
    let path = dir.path().join("falling.psd");
    std::fs::write(&path, &bytes).unwrap();
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    let losses = report["verified_losses"].to_string();
    assert!(
        losses.contains("(“Less”) is a Photoshop Solid Color Fill layer (SoCo) with no pixels whose colour cannot be rendered in the file's profile ("),
        "{losses}"
    );
    for fragment in [
        "(“Raster”) has adjustment/fill metadata",
        "(“Raster”) declares its raster pixels irrelevant",
    ] {
        assert!(losses.contains(fragment), "{fragment}: {losses}");
    }
    assert!(
        report["observations"]
            .to_string()
            .contains("(“Pattern”) is a Photoshop Pattern Fill layer (PtFl) in Photoshop's form"),
        "{}",
        report["observations"]
    );
    let reopened = open8(&path);
    let names: Vec<&str> = reopened.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names.len(), 3, "{names:?}");
    assert_eq!(names[1..], ["Raster", "Pattern"]);
    assert!(
        reopened.layers[1].meta.is_none(),
        "the raster keeps its pixels"
    );
    assert_eq!(
        reopened.layers[1].pixels.to_contiguous().unwrap(),
        doc.layers[2].pixels.to_contiguous().unwrap()
    );
    let pattern = parse(reopened.layers[2].meta.as_deref().expect("the pattern"));
    assert_eq!(pattern["fill"], "pattern");
}

#[test]
fn a_carried_solid_is_admitted_as_a_canvas_raster_not_its_rectangle() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("solid16.psd");
    save16(
        &typed_doc(
            &IccProfile::srgb(),
            &[Fill::new("Fill 0", A), Fill::new("Fill 1", B)],
        ),
        &path,
    );
    let bytes = std::fs::read(&path).unwrap();
    // The masked fill's key renamed, so the same record is a plain raster of
    // its 60 × 44 rectangle.
    let mut raster = bytes.clone();
    let at = raster
        .windows(8)
        .position(|w| w == b"8BIMSoCo")
        .expect("the first SoCo");
    raster[at + 4..at + 8].copy_from_slice(b"XoCo");
    let raster_path = dir.path().join("raster16.psd");
    std::fs::write(&raster_path, &raster).unwrap();
    let least = |path: &Path| {
        let token = import_report(path)["source_fingerprint"]
            .as_str()
            .unwrap()
            .to_string();
        let (mut lo, mut hi) = (0u64, LIMIT);
        assert!(try_open16(path, &token, hi).is_ok());
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if try_open16(path, &token, mid).is_ok() {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        hi
    };
    let (fill, plain) = (least(&path), least(&raster_path));
    let uncovered = u64::from(W * H - MASK_SIZE.0 * MASK_SIZE.1);
    assert!(
        fill >= plain + uncovered * 8,
        "fill {fill}, raster {plain}, uncovered {uncovered}"
    );
    // The Photoshop body's own block, for the record: one per fill.
    assert_eq!(blocks_in(&bytes, b"SoCo").len(), 2);
}

// ------------------------------------------------- Photoshop's own files --

/// The take's six `SoCo` files (`fixtures/psd_photoshop/solid_fill_layer`,
/// `scripts/photoshop-probe/fill_layer_672_take.py`): `photoshop.json` names
/// each case, what the take read from its `SoCo` and its PSD and TIFF with
/// their sha256.
fn photoshop_manifest() -> Value {
    let path = crate::common::psd_photoshop::fixture("solid_fill_layer", "photoshop.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).expect("photoshop.json")
}

/// The largest channel difference, in 8-bit codes, over the first three
/// channels of two RGBA8 rasters of the same size.
fn worst_code(ours: &[u8], theirs: &[u8]) -> i32 {
    assert_eq!(ours.len(), theirs.len());
    ours.chunks(4)
        .zip(theirs.chunks(4))
        .flat_map(|(a, b)| (0..3).map(move |c| (i32::from(a[c]) - i32::from(b[c])).abs()))
        .max()
        .unwrap()
}

#[test]
fn photoshops_own_solid_fill_layers_read_as_their_colour_within_the_bound() {
    use crate::common::p3_oracle::p3_to_linear_srgb;
    let dir = TempDir::new().unwrap();
    let manifest = photoshop_manifest();
    assert_eq!(manifest["source"]["verdict"], "VALID", "the take's verdict");
    let bound = manifest["bound"].as_f64().unwrap();
    let files = manifest["files"].as_object().unwrap();
    assert_eq!(files.len(), 6, "the take's six SoCo cases");
    for (key, entry) in files {
        let path = |kind: &str| {
            crate::common::psd_photoshop::fixture(
                "solid_fill_layer",
                entry[kind]["file"].as_str().unwrap(),
            )
        };
        for kind in ["psd", "tiff"] {
            assert_eq!(
                crate::common::sha256::sha256_hex(&std::fs::read(path(kind)).unwrap()),
                entry[kind]["sha256"].as_str().unwrap(),
                "{key}: the take's {kind}"
            );
        }
        let mut psd = path("psd");
        let bytes = std::fs::read(&psd).unwrap();
        if entry["case"]["mode"] == "cmyk" {
            // The take saved its CMYK file untagged (the working SWOP profile
            // would pass the fixtures' 64 KB cap), and an untagged CMYK file
            // opens under the process-wide working CMYK, which a shared test
            // binary must not set: a copy tagged with the test press profile
            // reads the same ink, which no profile converts.
            psd = dir.path().join(format!("{key}-tagged.psd"));
            std::fs::write(&psd, retag(&bytes, &press_profile("Press Test"))).unwrap();
        }
        let soco = blocks_in(&bytes, b"SoCo");
        assert_eq!(soco.len(), 1, "{key}: one SoCo");
        let report = import_report(&psd);
        let form = if entry["soco"]["bounds"] == json!([0, 0, 0, 0]) {
            "with no pixels"
        } else {
            "in Photoshop's form"
        };
        let observed = report["observations"].to_string();
        assert!(
            observed.contains(&format!(
                "(“SF”) is a Photoshop Solid Color Fill layer (SoCo) {form}"
            )),
            "{key}: {observed}"
        );
        assert!(
            !report["verified_losses"].to_string().contains("SF"),
            "{key}: {}",
            report["verified_losses"]
        );
        let case = &entry["case"];
        let again = dir.path().join(format!("{key}-again.psd"));
        if case["mode"] == "cmyk" {
            // The ink, as Photoshop's `CMYC` percent / 100.
            let ink: Vec<f64> = ["Cyn doub", "Mgntdoub", "Ylw doub", "Blckdoub"]
                .iter()
                .map(|k| {
                    let at = soco[0]
                        .windows(8)
                        .position(|w| w == k.as_bytes())
                        .expect("CMYC key");
                    f64::from_be_bytes(soco[0][at + 8..at + 16].try_into().unwrap())
                })
                .collect();
            let back = open(&psd);
            let d = cmyk_description(back, 1).expect("the fill's description");
            for (got, want) in numbers(&d, "color").iter().zip(&ink) {
                assert!((got - want / 100.0).abs() <= 1e-4, "{key}: {got} vs {want}");
            }
            // Photoshop's flattened TIFF holds the same ink everywhere.
            let theirs =
                crate::cmyk_psd_fixture::read_flat_tiff(&std::fs::read(path("tiff")).unwrap());
            let samples = unsafe { &*back }.cmyk_layer_samples(1).unwrap();
            for (i, s) in samples.iter().enumerate() {
                for (c, (v, plane)) in s.iter().zip(&theirs.planes).take(4).enumerate() {
                    let ours = f64::from((u32::from(*v) + 128) / 257);
                    let got = f64::from(plane[i]);
                    assert!(
                        (ours - got).abs() <= bound,
                        "{key}: ink {c} at {i}: {ours} vs {got}"
                    );
                }
            }
            save_layers(back, &again);
            let records = framed_records(&std::fs::read(&again).unwrap());
            let body = block(record(&records, "SF"), b"SoCo");
            for (k, want) in ["Cyn doub", "Mgntdoub", "Ylw doub", "Blckdoub"]
                .iter()
                .zip(&ink)
            {
                let at = body.windows(8).position(|w| w == k.as_bytes()).unwrap();
                let got = f64::from_be_bytes(body[at + 8..at + 16].try_into().unwrap());
                assert!(
                    (got - want).abs() <= 0.4,
                    "{key} re-export: {got} vs {want}"
                );
            }
            free(back);
            continue;
        }
        let numbers_of = rgbc_numbers(&soco[0]);
        let p3 = case["profile"] == "Display P3";
        let check = |description: &str| {
            let d = parse(description);
            assert_eq!(d["fill"], "solid", "{key}");
            if p3 {
                let oracle = p3_to_linear_srgb(numbers_of.map(|n| n / 255.0));
                for (got, want) in numbers(&d, "linear").iter().zip(oracle) {
                    assert!((got - want).abs() <= 1e-4, "{key}: linear {got} vs {want}");
                }
            } else {
                assert!(d.get("linear").is_none(), "{key}: an sRGB colour");
                for (got, want) in numbers(&d, "color").iter().zip(numbers_of) {
                    assert!((got - want / 255.0).abs() <= 1e-5, "{key}: {got} vs {want}");
                }
            }
        };
        let theirs = DrDocument::open(path("tiff").to_str().unwrap())
            .unwrap()
            .flattened()
            .unwrap();
        if case["depth"] == 16 {
            let doc = open16(&psd);
            check(doc.artwork_description(1).expect("a description"));
            let ours: Vec<u8> = doc
                .render(LIMIT, 0, None)
                .unwrap()
                .to_rgba16()
                .unwrap()
                .iter()
                .flat_map(|p| p.map(|v| ((u32::from(v) + 128) / 257) as u8))
                .collect();
            let worst = worst_code(&ours, theirs.as_raw());
            assert!(
                f64::from(worst) <= bound,
                "{key}: {worst} codes from Photoshop's TIFF"
            );
            save16(&doc, &again);
        } else {
            let doc = open8(&psd);
            let layer = &doc.layers[1];
            check(layer.meta.as_deref().expect("a description"));
            assert_eq!(layer.offset, (0, 0), "{key}: at the origin");
            assert_eq!(
                layer.pixels.dimensions(),
                (64, 48),
                "{key}: a canvas raster"
            );
            if key == "SM8" {
                // The selection's `-2` mask is kept as the fill's.
                let mask = layer.mask.as_deref().expect("SM8's mask");
                assert_eq!(mask.dimensions(), (40, 30), "SM8: the mask's size");
                assert_eq!(
                    layer.mask_properties.offset,
                    (8, 6),
                    "SM8: the mask's place"
                );
                assert!(
                    mask.as_raw().iter().all(|&v| v == 255),
                    "SM8: a hard rectangle"
                );
            }
            let worst = worst_code(doc.flattened().unwrap().as_raw(), theirs.as_raw());
            assert!(
                f64::from(worst) <= bound,
                "{key}: {worst} codes from Photoshop's TIFF"
            );
            save8(&doc, &again);
        }
        for (n, want) in written_rgbc(&std::fs::read(&again).unwrap(), "SF")
            .iter()
            .zip(numbers_of)
        {
            assert!((n - want).abs() <= bound, "{key} re-export: {n} vs {want}");
        }
    }
}
