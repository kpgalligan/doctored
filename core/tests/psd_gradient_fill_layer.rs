//! Gradient fill layers in PSD: our 8- and 16-bit RGB and
//! CMYK layered exports write a gradient fill layer as Photoshop's `GdFl` —
//! at 8 bits its rendered pixels over its mask's rectangle (the canvas without
//! one), its `-2` mask, flags `0x18`; at 16 bits PIXEL-LESS (bounds 0, empty
//! colour planes, its `-2` mask) — with the descriptor shape Photoshop itself
//! writes (take1's GL8 body) plus the description's `Algn`, its stops in the
//! DOCUMENT's numbers (`RGBC`, or `CMYC` ink). Our importer reads them back as
//! the same fill layer. A stop outside sRGB (Display P3 green) crosses as a
//! wide `stop_linears` colour. Every setting a Doctored gradient fill cannot
//! hold is named as a loss (Smoothness and Align with Layer are carried); a
//! `GdFl` that does not decode is omitted with its reason when pixel-less
//! and stays a raster with its marker losses when it has pixels; a file
//! whose profile cannot be rendered into declines it like its `SoCo`. A
//! carried gradient is admitted as a canvas raster.
//! Photoshop's own seven take1 files read as their gradient within the
//! recorded bound; GM8, aligned with its 40 × 30 mask at (8, 6), spans that
//! box.
//!
//! Oracles are independent: an sRGB file's `RGBC` is the stop · 255; the
//! Display P3 ↔ sRGB matrix is built from the published primaries
//! (`common/p3_oracle.rs`); descriptors are read by this file's own walker,
//! never the core's reader; written planes are the document's own samples.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::ffi_artwork::{dr_artwork_render, dr_doc_artwork, dr_precision_doc_artwork};
use doctored_core::ffi_cmyk_artwork::dr_precision_doc_cmyk_artwork;
use doctored_core::ffi_fill_align::{dr_doc_fill_align_box, dr_precision_doc_fill_align_box};
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
use crate::common::psd_photoshop::blocks_in;
use crate::common::{solid, take_err_string};
use crate::psd16_fixture as fixture16;
use crate::psd_descriptor_fixture as builder;
use crate::psd_mask_fixture as fixture8;
use crate::psd_pattern_fill_cmyk::{
    description as cmyk_description, free, open, save_layers, then,
};
use crate::psd_pattern_fill_layer::{
    framed_records, import_report, open16, open8, owned, pixel_less, plane, save16, save8,
    try_open16, Framed,
};
use crate::psd_solid_fill_layer::{block, falling_profile, record, retag};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
pub(crate) const W: u32 = 96;
pub(crate) const H: u32 = 64;
const MASK_AT: (i32, i32) = (10, 6);
const MASK_SIZE: (u32, u32) = (60, 44);
const MASKED_RECT: [i32; 4] = [6, 10, 50, 70];
const CANVAS_RECT: [i32; 4] = [0, 0, H as i32, W as i32];

/// The stops: position, sRGB bytes, opacity (take1's, on the byte grid).
const STOPS: [(f64, [u8; 3], f64); 3] = [
    (0.0, [220, 40, 40], 1.0),
    (0.5, [40, 90, 200], 1.0),
    (1.0, [250, 230, 60], 0.4),
];

/// The CMYK fill's inks, one per stop.
const INKS: [[f64; 4]; 3] = [
    [0.7, 0.2, 0.1, 0.05],
    [0.1, 0.55, 0.9, 0.0],
    [0.0, 0.3, 0.6, 0.2],
];

/// Every number of `got` within `tol` of `want`'s.
fn near(got: &[f64], want: &[f64], tol: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: {got:?} vs {want:?}");
    let ok = got.iter().zip(want).all(|(g, w)| (g - w).abs() <= tol);
    assert!(ok, "{what}: {got:?} vs {want:?} (±{tol})");
}

/// Two sample lists within `tol` per channel.
pub(crate) fn near_samples<const N: usize>(
    got: &[[u16; N]],
    want: &[[u16; N]],
    tol: i32,
    what: &str,
) {
    assert_eq!(got.len(), want.len(), "{what}");
    for (i, (a, b)) in got.iter().zip(want).enumerate() {
        let ok = a
            .iter()
            .zip(b)
            .all(|(x, y)| (i32::from(*x) - i32::from(*y)).abs() <= tol);
        assert!(ok, "{what} at {i}: {a:?} vs {b:?}");
    }
}

// ------------------------------------------------------- descriptor walker --

/// A descriptor body read by this file's own walker (Adobe's "Descriptor
/// structure") as `path → (type, number, text)`: the path joins keys and list
/// indices with `/`; the type is the OSType, with a `UntF`'s unit, an `Objc`'s
/// class or an `enum`'s type; the number is a value, a bool as 0/1 or a
/// list's count; the text an enum's id.
pub(crate) type Flat = BTreeMap<String, (String, f64, String)>;

struct R<'a>(&'a [u8], usize, Flat);

impl R<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        self.1 += n;
        &self.0[self.1 - n..self.1]
    }
    fn u32(&mut self) -> u32 {
        u32::from_be_bytes(self.take(4).try_into().unwrap())
    }
    fn key(&mut self) -> String {
        let n = self.u32() as usize;
        String::from_utf8_lossy(self.take(if n == 0 { 4 } else { n })).into_owned()
    }
    fn f64(&mut self) -> f64 {
        f64::from_be_bytes(self.take(8).try_into().unwrap())
    }
    fn value(&mut self, path: String, kind: &[u8]) {
        let (kind, n, s) = match kind {
            b"Objc" => {
                let class = self.object(&path);
                (format!("Objc:{class}"), 0.0, String::new())
            }
            b"VlLs" => {
                let n = self.u32();
                for i in 0..n {
                    let k = self.take(4).to_vec();
                    self.value(format!("{path}/{i}"), &k);
                }
                ("VlLs".into(), f64::from(n), String::new())
            }
            b"UntF" => {
                let unit = String::from_utf8_lossy(self.take(4)).into_owned();
                (format!("UntF:{unit}"), self.f64(), String::new())
            }
            b"doub" => ("doub".into(), self.f64(), String::new()),
            b"long" => {
                let v = i32::from_be_bytes(self.take(4).try_into().unwrap());
                ("long".into(), f64::from(v), String::new())
            }
            b"bool" => ("bool".into(), f64::from(self.take(1)[0]), String::new()),
            b"TEXT" => {
                let n = self.u32() as usize;
                self.take(2 * n);
                ("TEXT".into(), 0.0, String::new())
            }
            b"enum" => {
                let t = self.key();
                (format!("enum:{t}"), 0.0, self.key())
            }
            other => panic!("descriptor type {}", String::from_utf8_lossy(other)),
        };
        self.2.insert(path, (kind, n, s));
    }
    /// An object's items under `path`; its class.
    fn object(&mut self, path: &str) -> String {
        let name = self.u32() as usize;
        self.take(2 * name);
        let class = self.key();
        for _ in 0..self.u32() {
            let key = self.key();
            let kind = self.take(4).to_vec();
            let path = if path.is_empty() {
                key
            } else {
                format!("{path}/{key}")
            };
            self.value(path, &kind);
        }
        class
    }
}

/// A `GdFl` body: `u32 16`, then the descriptor.
pub(crate) fn read_body(body: &[u8]) -> Flat {
    let mut r = R(body, 0, Flat::new());
    assert_eq!(r.u32(), 16, "descriptor version 16");
    r.object("");
    r.2
}

pub(crate) fn n(f: &Flat, path: &str) -> f64 {
    f.get(path).unwrap_or_else(|| panic!("{path} in {f:?}")).1
}

fn id<'a>(f: &'a Flat, path: &str) -> &'a str {
    &f.get(path).unwrap_or_else(|| panic!("{path} in {f:?}")).2
}

/// Every path (list indices as `#`) with its type.
fn shape(f: &Flat) -> BTreeSet<String> {
    f.iter()
        .map(|(path, (kind, _, _))| {
            let path: Vec<&str> = path
                .split('/')
                .map(|s| if s.parse::<usize>().is_ok() { "#" } else { s })
                .collect();
            format!("{}:{kind}", path.join("/"))
        })
        .collect()
}

/// Colour stop `i`'s `RGBC` on 0…255 (`Rd  ` or `redFloat` · 255).
fn rgbc(f: &Flat, i: usize) -> Vec<f64> {
    [
        ("Rd  ", "redFloat"),
        ("Grn ", "greenFloat"),
        ("Bl  ", "blueFloat"),
    ]
    .iter()
    .map(|(k, x)| {
        let at = format!("Grad/Clrs/{i}/Clr /{k}");
        f.get(&at)
            .map_or_else(|| n(f, &format!("Grad/Clrs/{i}/Clr /{x}")) * 255.0, |v| v.1)
    })
    .collect()
}

/// Colour stop `i`'s `CMYC` percentages.
fn cmyc(f: &Flat, i: usize) -> Vec<f64> {
    ["Cyn ", "Mgnt", "Ylw ", "Blck"]
        .iter()
        .map(|k| n(f, &format!("Grad/Clrs/{i}/Clr /{k}")))
        .collect()
}

/// The written `GdFl` of the record named `name`.
pub(crate) fn gdfl_of(bytes: &[u8], name: &str) -> Flat {
    read_body(block(record(&framed_records(bytes), name), b"GdFl"))
}

// ---------------------------------------------------------------- plumbing --

/// The `GradientFill` object: STOPS (colours `hex`), `style`, `angle`, scale.
pub(crate) fn gradient(style: &str, angle: f64, scale: f64, hex: &[String]) -> Value {
    let stops: Vec<Value> = STOPS
        .iter()
        .zip(hex)
        .map(|((p, _, o), hex)| json!({"position": p, "color": hex, "opacity": o}))
        .collect();
    json!({"stops": stops, "style": style, "angle": angle, "scale": scale,
           "reverse": false, "align_with_layer": false})
}

fn hex_of(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

pub(crate) fn stop_hex() -> Vec<String> {
    STOPS.iter().map(|s| hex_of(s.1)).collect()
}

fn grey_hex() -> Vec<String> {
    vec!["#808080".to_string(); 3]
}

pub(crate) fn unit_colors() -> Vec<[f64; 3]> {
    STOPS
        .iter()
        .map(|s| s.1.map(|v| f64::from(v) / 255.0))
        .collect()
}

/// An RGB gradient fill's render component and description; `linear` its
/// optional `stop_linears`.
fn rgb_fill(g: &Value, colors: &[[f64; 3]], linear: Option<&[[f64; 3]]>) -> (String, String) {
    let mut c = json!({"kind": "gradient", "gradient": g, "colors": colors});
    let mut d = json!({"type": "fill", "version": 1, "fill": "gradient", "gradient": g,
                       "canvas": [W, H]});
    if let Some(linear) = linear {
        c["linear"] = json!(linear);
        d["stop_linears"] = json!(linear);
    }
    (json!([c]).to_string(), d.to_string())
}

pub(crate) fn render(
    profile: &Arc<IccProfile>,
    components: &str,
    size: (u32, u32),
) -> PrecisionImage {
    let components = CString::new(components).unwrap();
    let (icc, mut err) = (profile.bytes(), ptr::null_mut());
    let out = unsafe {
        dr_artwork_render(
            size.0,
            size.1,
            components.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            icc.as_ptr(),
            icc.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    *unsafe { Box::from_raw(out) }
}

/// `doc` with `fill` (component, description) published above layer `index`
/// as `name`.
fn publish8(doc: &DrDocument, index: usize, fill: &(String, String), name: &str) -> DrDocument {
    let image = render(&doc.profile, &fill.0, (W, H));
    let (name, text) = (
        CString::new(name).unwrap(),
        CString::new(fill.1.clone()).unwrap(),
    );
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_artwork(
            doc,
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
    *unsafe { Box::from_raw(out) }
}

fn mask_value(x: u32, y: u32) -> u8 {
    ((x * 4 + y * 3) % 251) as u8 + 4
}

fn mask8() -> GrayImage {
    GrayImage::from_fn(MASK_SIZE.0, MASK_SIZE.1, |x, y| Luma([mask_value(x, y)]))
}

fn mask16() -> Vec<u16> {
    mask8()
        .into_raw()
        .iter()
        .map(|v| u16::from(*v) * 257)
        .collect()
}

/// White, then one gradient fill "Grad", masked with `mask8` at `MASK_AT`
/// when `masked`, at opacity 0.6.
fn byte_doc(profile: &Arc<IccProfile>, fill: &(String, String), masked: bool) -> DrDocument {
    let mut doc = DrDocument::from_pixels(solid(W, H, [255; 4]));
    doc.profile = profile.clone();
    let mut doc = publish8(&doc, 0, fill, "Grad");
    if masked {
        doc = doc
            .add_mask(1, MaskKind::FromSelection(&vec![255u8; (W * H) as usize]))
            .expect("the mask");
        doc.layers[1].mask = Some(Arc::new(mask8()));
        doc.layers[1].mask_properties = doctored_core::doc_mask::MaskProperties::at(MASK_AT);
    }
    doc.with_layer_opacity(1, 0.6).expect("the opacity")
}

fn typed_parts(profile: &Arc<IccProfile>, layers: Vec<Layer>) -> PrecisionDocument {
    let (metadata, resolution) = (Metadata::default(), Resolution::default());
    PrecisionDocument::from_parts(W, H, profile.clone(), metadata, resolution, layers).unwrap()
}

/// `byte_doc` at 16 bits: the mask's samples are `mask_value` · 257.
fn typed_doc(
    profile: &Arc<IccProfile>,
    fill: &(String, String),
    masked: bool,
) -> PrecisionDocument {
    let white = vec![[65535; 4]; (W * H) as usize];
    let white = PrecisionImage::from_rgba16(W, H, white, profile.clone(), None).unwrap();
    let base = Layer::raster("Base", white, (0, 0), 1.0, true).unwrap();
    let doc = typed_parts(profile, vec![base]);
    let image = render(profile, &fill.0, (W, H));
    let (name, text) = (
        CString::new("Grad").unwrap(),
        CString::new(fill.1.clone()).unwrap(),
    );
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork(
            &doc,
            0,
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
    let doc = *unsafe { Box::from_raw(out) };
    let mut layers: Vec<Layer> = (0..2).map(|i| doc.layer(i).unwrap().clone()).collect();
    if masked {
        use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
        let plane = PrecisionPlane::from_u16(MASK_SIZE.0, MASK_SIZE.1, mask16(), None).unwrap();
        let at = doctored_core::doc_mask::MaskProperties::at(MASK_AT);
        let mask = PrecisionMask::new(plane, at, true).unwrap();
        layers[1] = layers[1].clone().with_mask(Some(mask));
    }
    let doc = typed_parts(profile, layers);
    doc.set_layer_opacity(1, 0.6).unwrap().expect("the opacity")
}

pub(crate) fn parse(description: &str) -> Value {
    serde_json::from_str(description).expect("description JSON")
}

fn rows(d: &Value, key: &str) -> Vec<Vec<f64>> {
    let list = d[key].as_array().unwrap_or_else(|| panic!("{key}: {d}"));
    list.iter()
        .map(|r| {
            r.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect()
        })
        .collect()
}

/// A description's gradient against STOPS, `style`, `angle` and `scale`, its
/// hexes `hex`.
/// `align` is the file's `Algn` (absent = true).
fn assert_gradient(
    d: &Value,
    style: &str,
    angle: f64,
    scale: f64,
    hex: &[String],
    align: bool,
    what: &str,
) {
    assert_eq!(d["fill"], "gradient", "{what}: {d}");
    let g = &d["gradient"];
    assert_eq!(g["style"], style, "{what}");
    assert_eq!(
        (g["reverse"].clone(), g["align_with_layer"].clone()),
        (json!(false), json!(align)),
        "{what}"
    );
    let got = [&g["angle"], &g["scale"]].map(|v| v.as_f64().unwrap());
    near(&got, &[angle, scale], 1e-3, what);
    let stops = g["stops"].as_array().unwrap();
    assert_eq!(stops.len(), STOPS.len(), "{what}");
    for ((stop, (p, _, o)), hex) in stops.iter().zip(STOPS).zip(hex) {
        let got = [&stop["position"], &stop["opacity"]].map(|v| v.as_f64().unwrap());
        near(&got, &[p, o], 1e-3, what);
        assert_eq!(stop["color"].as_str().unwrap(), hex, "{what}");
    }
}

/// One stored channel `id` of `r`, decoded at `sample` bytes over `w × h`.
fn channel(r: &Framed, id: i16, w: usize, h: usize, sample: usize) -> Vec<u8> {
    plane(
        &r.channels.iter().find(|(i, _)| *i == id).unwrap().1,
        w,
        h,
        sample,
    )
}

/// Bit 4, `GdFl`, and a `-2` channel exactly when `masked`.
fn assert_fill_record(r: &Framed, rect: [i32; 4], masked: bool) {
    assert_eq!(r.bounds, rect, "{}'s rectangle", r.name);
    assert_eq!(r.flags & 0x18, 0x18, "bit 4");
    assert!(r.blocks.iter().any(|(k, _)| k == b"GdFl"), "GdFl");
    assert_eq!(r.channels.iter().any(|(id, _)| *id == -2), masked, "-2");
    if masked {
        assert_eq!(r.mask, Some((MASKED_RECT, 0)), "the mask's rectangle");
    }
}

/// Photoshop's 8-bit form: `assert_fill_record`, channels -1, 0, 1, 2 holding
/// `pixel` over `rect`, the `-2` channel holding `mask8`.
fn assert_pixel_form(
    r: &Framed,
    rect: [i32; 4],
    masked: bool,
    pixel: &dyn Fn(u32, u32) -> [u8; 4],
) {
    assert_fill_record(r, rect, masked);
    let (w, h) = ((rect[3] - rect[1]) as usize, (rect[2] - rect[0]) as usize);
    for (id, c) in [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)] {
        let samples = channel(r, id, w, h, 1);
        let want: Vec<u8> = (0..w * h)
            .map(|k| {
                pixel(
                    rect[1] as u32 + (k % w) as u32,
                    rect[0] as u32 + (k / w) as u32,
                )[c]
            })
            .collect();
        assert_eq!(samples, want, "channel {id}");
    }
    if masked {
        let (mw, mh) = (MASK_SIZE.0 as usize, MASK_SIZE.1 as usize);
        assert_eq!(
            channel(r, -2, mw, mh, 1),
            mask8().into_raw(),
            "the -2 samples"
        );
    }
}

/// Photoshop's 16-bit form: no rectangle and empty colour planes `ids`.
fn assert_pixel_less(r: &Framed, ids: &[i16], masked: bool) {
    assert_fill_record(r, [0; 4], masked);
    for id in ids {
        assert!(channel(r, *id, 0, 0, 2).is_empty(), "channel {id} is empty");
    }
}

/// Our written body holds every key, type and unit of Photoshop's own (take1's
/// GL8), `Algn` false, Classic, STOPS' locations and opacities, `type_id`,
/// angle 30, scale 80, and each stop's `RGBC` = `want(i)` within 1e-4.
fn assert_body(ours: &Flat, type_id: &str, want: &dyn Fn(usize) -> Vec<f64>, what: &str) {
    let gl8 = crate::common::psd_photoshop::fixture("gradient_fill_layer", "GL8.psd");
    let theirs = read_body(&blocks_in(&std::fs::read(gl8).unwrap(), b"GdFl")[0]);
    let missing: Vec<String> = shape(&theirs).difference(&shape(ours)).cloned().collect();
    assert!(missing.is_empty(), "{what}: missing {missing:?}");
    assert_eq!(ours["Algn"].0, "bool", "{what}");
    assert_eq!(n(ours, "Algn"), 0.0, "{what}: Algn false");
    assert_eq!(id(ours, "gradientsInterpolationMethod"), "Gcls", "{what}");
    assert_eq!(id(ours, "Type"), type_id, "{what}");
    // The scale and opacities are f32 fractions, written · 100 (`percent`).
    let top = [n(ours, "Angl"), n(ours, "Scl "), n(ours, "Grad/Intr")];
    near(&top, &[30.0, 80.0, 4096.0], 1e-5, what);
    assert_eq!(n(ours, "Grad/Clrs"), 3.0, "{what}");
    for (i, (p, _, o)) in STOPS.iter().enumerate() {
        let lctn = (p * 4096.0).round();
        let got = [
            n(ours, &format!("Grad/Clrs/{i}/Lctn")),
            n(ours, &format!("Grad/Trns/{i}/Lctn")),
        ];
        assert_eq!(got, [lctn, lctn], "{what} {i}");
        near(
            &[n(ours, &format!("Grad/Trns/{i}/Opct"))],
            &[o * 100.0],
            1e-5,
            what,
        );
        near(&rgbc(ours, i), &want(i), 1e-4, what);
    }
}

// ------------------------------------------------------------ round trips --

#[test]
fn an_8_and_16_bit_gradient_fill_layer_round_trips_in_photoshops_gdfl_form() {
    let dir = TempDir::new().unwrap();
    let srgb = IccProfile::srgb();
    let fill = rgb_fill(
        &gradient("linear", 30.0, 0.8, &stop_hex()),
        &unit_colors(),
        None,
    );
    let byte_stop = |i: usize| STOPS[i].1.map(f64::from).to_vec();
    let carried = |report: &Value, form: &str| {
        let observed = report["observations"].to_string();
        let want = format!("(“Grad”) is a Photoshop Gradient Fill layer (GdFl) {form}");
        assert!(observed.contains(&want), "{observed}");
        assert!(
            !report["verified_losses"].to_string().contains("Grad"),
            "{report}"
        );
    };

    // 8 bits, masked: the mask's rectangle, its -2 bytes, the colour planes.
    let doc = byte_doc(&srgb, &fill, true);
    let path = dir.path().join("grad8.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    let pixels = doc.layers[1].pixels.to_contiguous().unwrap();
    let records = framed_records(&bytes);
    let r = record(&records, "Grad");
    assert_pixel_form(r, MASKED_RECT, true, &|x, y| pixels.get_pixel(x, y).0);
    assert_body(&gdfl_of(&bytes, "Grad"), "Lnr ", &byte_stop, "psd8");
    carried(&import_report(&path), "in Photoshop's form");
    let reopened = open8(&path);
    assert_eq!(reopened.layers.len(), 2);
    let layer = &reopened.layers[1];
    let d = parse(layer.meta.as_deref().expect("a description"));
    assert_gradient(&d, "linear", 30.0, 0.8, &stop_hex(), false, "psd8");
    assert!(d.get("stop_linears").is_none(), "no wide stops");
    assert_eq!(layer.offset, (0, 0));
    assert_eq!(
        layer.pixels.to_contiguous().unwrap(),
        pixels,
        "psd8: pixels"
    );
    assert!((layer.opacity - 0.6).abs() < 1e-3, "opacity");
    let mask = layer.mask.as_deref().map(|m| m.as_raw().clone());
    assert_eq!(mask, Some(mask8().into_raw()), "the mask");
    assert_eq!(layer.mask_properties.offset, MASK_AT);
    assert_eq!(reopened.flattened().unwrap(), doc.flattened().unwrap());

    // 16 bits, masked: pixel-less with its -2 mask.
    let doc = typed_doc(&srgb, &fill, true);
    let path = dir.path().join("grad16.psd");
    save16(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert_pixel_less(
        record(&framed_records(&bytes), "Grad"),
        &[-1, 0, 1, 2],
        true,
    );
    assert_body(&gdfl_of(&bytes, "Grad"), "Lnr ", &byte_stop, "psd16");
    carried(&import_report(&path), "with no pixels;");
    let reopened = open16(&path);
    let d = parse(reopened.artwork_description(1).expect("a description"));
    assert_gradient(&d, "linear", 30.0, 0.8, &stop_hex(), false, "psd16");
    let layer = reopened.layer(1).unwrap();
    assert_eq!(layer.offset(), (0, 0));
    let want = doc.layer(1).unwrap().source().unwrap().to_rgba16().unwrap();
    near_samples(
        &layer.source().unwrap().to_rgba16().unwrap(),
        &want,
        1,
        "psd16",
    );
    assert!((layer.opacity() - 0.6).abs() < 1e-3);
    assert_eq!(layer.mask().expect("the mask").properties().offset, MASK_AT);
}

/// A hand-built `GdFl` in take1's shape, edited before it is framed: one
/// `Clrt` item list per colour stop, one `TrnS` list per transparency stop
/// (opacities 100 … 40), the `Grdn` head and the top-level items.
pub(crate) struct Gd {
    pub(crate) stops: Vec<Vec<Vec<u8>>>,
    pub(crate) trns: Vec<Vec<Vec<u8>>>,
    pub(crate) grdn: Vec<Vec<u8>>,
    pub(crate) top: Vec<Vec<u8>>,
}

impl Gd {
    pub(crate) fn new(colours: Vec<Vec<u8>>, locations: &[i32]) -> Self {
        let last = locations.len() - 1;
        let at = |i: usize| builder::long(b"Lctn", locations[i]);
        let mid = || builder::long(b"Mdpn", 50);
        let user = || builder::enumeration(b"Type", b"Clry", b"UsrS");
        let stops = colours.into_iter().enumerate();
        let stops = stops
            .map(|(i, clr)| vec![clr, user(), at(i), mid()])
            .collect();
        let opct = |i: usize| builder::untf(b"Opct", b"#Prc", if i == last { 40.0 } else { 100.0 });
        let trns = (0..=last).map(|i| vec![opct(i), at(i), mid()]).collect();
        let grdn = vec![
            builder::text(b"Nm  ", ""),
            builder::enumeration(b"GrdF", b"GrdF", b"CstS"),
            builder::doub(b"Intr", 4096.0),
        ];
        let method = b"gradientInterpolationMethodType";
        let top = vec![
            builder::enumeration(b"gradientsInterpolationMethod", method, b"Gcls"),
            builder::untf(b"Angl", b"#Ang", 30.0),
            builder::enumeration(b"Type", b"GrdT", b"Lnr "),
            builder::untf(b"Scl ", b"#Prc", 80.0),
        ];
        Gd {
            stops,
            trns,
            grdn,
            top,
        }
    }

    /// take1's stops.
    fn take() -> Self {
        let take1 = STOPS.map(|s| s.1.map(f64::from));
        Gd::new(rgbc_items(&take1), &[0, 2048, 4096])
    }

    pub(crate) fn edited(mut self, edit: impl FnOnce(&mut Gd)) -> Vec<u8> {
        edit(&mut self);
        let list = |key: &[u8], class: &[u8], entries: &[Vec<Vec<u8>>]| {
            let mut bytes = (entries.len() as u32).to_be_bytes().to_vec();
            for items in entries {
                bytes.extend(b"Objc");
                bytes.extend(builder::desc(class, items));
            }
            builder::item(key, b"VlLs", &bytes)
        };
        self.grdn.push(list(b"Clrs", b"Clrt", &self.stops));
        self.grdn.push(list(b"Trns", b"TrnS", &self.trns));
        self.top.push(builder::objc(b"Grad", b"Grdn", &self.grdn));
        let mut body = builder::body(16, b"null", &self.top);
        if body.len() % 2 == 1 {
            body.push(0);
        }
        body
    }
}

pub(crate) fn rgbc_items(rgb: &[[f64; 3]]) -> Vec<Vec<u8>> {
    rgb.iter()
        .map(|c| builder::rgbc(b"Clr ", c[0], c[1], c[2]))
        .collect()
}

fn cmyc_items(inks: &[[f64; 4]]) -> Vec<Vec<u8>> {
    let keys: [&[u8]; 4] = [b"Cyn ", b"Mgnt", b"Ylw ", b"Blck"];
    inks.iter()
        .map(|ink| {
            let items: Vec<Vec<u8>> = keys
                .iter()
                .zip(ink)
                .map(|(k, v)| builder::doub(k, *v))
                .collect();
            builder::objc(b"Clr ", b"CMYC", &items)
        })
        .collect()
}

#[test]
fn a_display_p3_green_band_crosses_as_wide_stops_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let p3 = IccProfile::display_p3();
    // White, P3 green, P3 green, grey 64: the green band covers x 38..57.
    let numbers: [[f64; 3]; 4] = [[255.0; 3], [0.0, 255.0, 0.0], [0.0, 255.0, 0.0], [64.0; 3]];
    let locations = [0, 1638, 2458, 4096];
    let oracle: Vec<[f64; 3]> = numbers
        .iter()
        .map(|n| p3_to_linear_srgb(n.map(|v| v / 255.0)))
        .collect();
    assert!(oracle[1][0] < -0.1, "outside sRGB: {:?}", oracle[1]);
    let check = |d: &Value, what: &str| {
        let linear = rows(d, "stop_linears");
        assert_eq!(linear.len(), 4, "{what}");
        for (got, want) in linear.iter().zip(&oracle) {
            near(got, want, 1e-4, what);
        }
    };
    let green = |f: &Flat, what: &str| {
        for i in 1..3 {
            near(&rgbc(f, i), &[0.0, 255.0, 0.0], 1.0, what);
        }
    };
    let body = Gd::new(rgbc_items(&numbers), &locations).edited(|g| {
        g.top[1] = builder::untf(b"Angl", b"#Ang", 0.0);
        g.top[3] = builder::untf(b"Scl ", b"#Prc", 100.0);
    });

    // 16 bits: a hand-built Display P3 file holding Photoshop's pixel-less
    // `GdFl`.
    let empty = [-1i16, 0, 1, 2].map(|id| (id, fixture16::raw_plane(&[])));
    let mut fill = fixture16::Record::raster("Band", [0; 4], empty.to_vec());
    fill.blocks = fixture16::block(b"GdFl", &body);
    let white = vec![[65535u16; 4]; (W * H) as usize];
    let planes = [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)].map(|(id, c)| {
        let plane: Vec<u16> = white.iter().map(|p| p[c]).collect();
        (id, fixture16::raw_plane(&plane))
    });
    let base = fixture16::Record::raster("Base", CANVAS_RECT, planes.to_vec());
    let path = dir.path().join("p3-16.psd");
    let dpi = (72.0, 72.0);
    let file = fixture16::fixture((W, H), &[base, fill], p3.bytes(), dpi, &white, true, false);
    std::fs::write(&path, file).unwrap();
    let observed = import_report(&path)["observations"].to_string();
    assert!(
        observed.contains("(“Band”) is a Photoshop Gradient Fill layer (GdFl) with no pixels; layered import carries it as a Doctored gradient fill layer with its stops, style, angle, scale, reverse, mask, opacity, blend mode and clipping. Its stop colours are kept as wide colours."),
        "{observed}"
    );
    let doc = open16(&path);
    let d = parse(doc.artwork_description(1).expect("a description"));
    check(&d, "p3-16");
    let pixels = doc.layer(1).unwrap().source().unwrap().to_rgba16().unwrap();
    let band: Vec<[u16; 4]> = (39..57).map(|x| pixels[(20 * W + x) as usize]).collect();
    near_samples(&band, &[[0, 65535, 0, 65535]; 18], 257, "p3-16 band");
    let again = dir.path().join("p3-16-again.psd");
    save16(&doc, &again);
    let ours = gdfl_of(&std::fs::read(&again).unwrap(), "Band");
    green(&ours, "p3-16 re-export");

    // 8 bits: a Display P3 byte document whose fill's description holds the
    // wide stops, exported and reopened.
    let hex = ["#ffffff", "#00ff00", "#00ff00", "#404040"];
    let stops: Vec<Value> = (0..4)
        .map(|i| {
            json!({"position": f64::from(locations[i]) / 4096.0, "color": hex[i],
                   "opacity": if i == 3 { 0.4 } else { 1.0 }})
        })
        .collect();
    let g = json!({"stops": stops, "style": "linear", "angle": 0.0, "scale": 1.0,
                   "reverse": false, "align_with_layer": false});
    let colors: Vec<[f64; 3]> = numbers.iter().map(|n| n.map(|v| v / 255.0)).collect();
    let doc = byte_doc(&p3, &rgb_fill(&g, &colors, Some(&oracle)), false);
    let path = dir.path().join("p3-8.psd");
    save8(&doc, &path);
    let ours = gdfl_of(&std::fs::read(&path).unwrap(), "Grad");
    for (i, want) in numbers.iter().enumerate() {
        near(&rgbc(&ours, i), want, 1.0, "p3-8 RGBC");
    }
    let reopened = open8(&path);
    let layer = &reopened.layers[1];
    let d = parse(layer.meta.as_deref().expect("a description"));
    check(&d, "p3-8");
    let pixels = layer.pixels.to_contiguous().unwrap();
    let band: Vec<[u16; 4]> = (39..57)
        .map(|x| pixels.get_pixel(x, 20).0.map(u16::from))
        .collect();
    near_samples(&band, &[[0, 255, 0, 255]; 18], 1, "p3-8 band");
    let again = dir.path().join("p3-8-again.psd");
    save8(&reopened, &again);
    let ours = gdfl_of(&std::fs::read(&again).unwrap(), "Grad");
    green(&ours, "p3-8 re-export");
}

// ------------------------------------------------------------------- CMYK --

type Doc = *mut PrecisionDocument;

/// One CMYK gradient fill of `g` and `inks` published above layer 0.
fn publish_ink(doc: Doc, g: &Value, inks: &[[f64; 4]], size: (u32, u32)) -> Doc {
    let components = json!([{"kind": "gradient", "gradient": g, "inks": inks}]);
    let description = json!({"type": "fill", "version": 2, "fill": "gradient", "ink": "cmyk",
        "gradient": g, "stop_inks": inks, "canvas": [size.0, size.1]});
    let components = CString::new(components.to_string()).unwrap();
    let description = CString::new(description.to_string()).unwrap();
    let name = CString::new("Grad").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_artwork(
            doc,
            0,
            false,
            0,
            size.0,
            size.1,
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

/// Paper (`size`, `depth`), then one CMYK gradient fill of `inks`, masked
/// with `mask16` at `MASK_AT` when `masked`.
fn cmyk_doc(depth: u32, size: (u32, u32), inks: &[[f64; 4]], masked: bool) -> Doc {
    let request = format!(r#"{{"depth":{depth},"fill":[0,0,0,0,1],"layer_name":"Paper"}}"#);
    let profile = press_profile("Press Test");
    let doc = PrecisionDocument::new_cmyk(size.0, size.1, Some(&profile), &request, LIMIT, 0);
    let g = gradient("linear", 30.0, 0.8, &grey_hex());
    let doc = publish_ink(Box::into_raw(Box::new(doc.unwrap())), &g, inks, size);
    if !masked {
        return doc;
    }
    let request = json!({"op": "set_mask", "layer": 1, "width": MASK_SIZE.0,
        "height": MASK_SIZE.1, "x": MASK_AT.0, "y": MASK_AT.1});
    then(doc, &request, &mask16())
}

#[test]
fn a_cmyk_gradient_fill_layer_round_trips_its_inks_in_cmyc() {
    let dir = TempDir::new().unwrap();
    for depth in [8u32, 16] {
        let what = format!("cmyk{depth}");
        let doc = cmyk_doc(depth, (W, H), &INKS, true);
        let path = dir.path().join(format!("{what}.psd"));
        save_layers(doc, &path);
        let bytes = std::fs::read(&path).unwrap();
        let records = framed_records(&bytes);
        let r = record(&records, "Grad");
        let samples = unsafe { &*doc }.cmyk_layer_samples(1).unwrap();
        if depth == 16 {
            assert_pixel_less(r, &[-1, 0, 1, 2, 3], true);
        } else {
            assert_fill_record(r, MASKED_RECT, true);
            let (w, h) = (MASK_SIZE.0 as usize, MASK_SIZE.1 as usize);
            for (id, c) in [(-1i16, 4usize), (0, 0), (1, 1), (2, 2), (3, 3)] {
                let want: Vec<u8> = (0..w * h)
                    .map(|k| {
                        let (x, y) = (
                            MASK_AT.0 as u32 + (k % w) as u32,
                            MASK_AT.1 as u32 + (k / w) as u32,
                        );
                        let v = ((u32::from(samples[(y * W + x) as usize][c]) + 128) / 257) as u8;
                        if id == -1 {
                            v
                        } else {
                            255 - v
                        }
                    })
                    .collect();
                assert_eq!(channel(r, id, w, h, 1), want, "{what}: channel {id}");
            }
        }
        let body = gdfl_of(&bytes, "Grad");
        assert_eq!(n(&body, "Algn"), 0.0, "{what}: Algn false");
        for (i, ink) in INKS.iter().enumerate() {
            let got = cmyc(&body, i);
            near(&got, &ink.map(|v| v * 100.0), 5e-5, &what);
            assert!(
                got.iter().all(|v| (v * 1e4).round() / 1e4 == *v),
                "{what}: four decimals"
            );
        }
        let report = import_report(&path);
        let observed = report["observations"].to_string();
        assert!(
            observed.contains("(“Grad”) is a Photoshop Gradient Fill layer (GdFl)"),
            "{observed}"
        );
        assert!(
            !report["verified_losses"].to_string().contains("Grad"),
            "{what}"
        );
        let back = open(&path);
        let d = cmyk_description(back, 1).expect("a description");
        assert_eq!(
            (&d["version"], &d["ink"], &d["fill"]),
            (&json!(2), &json!("cmyk"), &json!("gradient"))
        );
        for (got, want) in rows(&d, "stop_inks").iter().zip(INKS) {
            near(got, &want, 1e-4, &what);
        }
        assert_eq!(
            unsafe { &*back }.cmyk_layer_samples(1),
            Some(samples),
            "{what}: samples"
        );
        free(back);
        free(doc);
    }
}

// --------------------------------------------------------- crafted files --

/// A pixel-less 8-bit record carrying `key`/`body`.
pub(crate) fn empty8(name: &'static str, key: &[u8; 4], body: &[u8]) -> fixture8::Record {
    let mut record = fixture8::Record::raster([0; 4], &[], false);
    record.name = name;
    record.blocks = fixture8::block(key, body);
    record
}

#[test]
fn every_setting_a_gradient_fill_cannot_hold_is_named_as_a_loss() {
    let dir = TempDir::new().unwrap();
    let method_type = b"gradientInterpolationMethodType";
    let offset = [
        builder::untf(b"Hrzn", b"#Prc", 12.5),
        builder::untf(b"Vrtc", b"#Prc", -4.0),
    ];
    let method =
        Some("gradient uses the Perc interpolation method, which Doctored draws as Classic");
    // Photoshop stop i + 1's `Mdpn` governs segment i.
    let mid = Gd::take().edited(|g| g.stops[1][3] = builder::long(b"Mdpn", 30));
    let tmid = Gd::take().edited(|g| g.trns[2][2] = builder::long(b"Mdpn", 70));
    let smooth = Gd::take().edited(|g| g.grdn[2] = builder::doub(b"Intr", 2048.0));
    let perc = Gd::take().edited(|g| {
        g.top[0] = builder::enumeration(b"gradientsInterpolationMethod", method_type, b"Perc")
    });
    let ofst = Gd::take().edited(|g| g.top.push(builder::objc(b"Ofst", b"Pnt ", &offset)));
    let dthr = Gd::take().edited(|g| g.top.push(builder::boolean(b"Dthr", 1)));
    let last = Gd::take().edited(|g| g.stops[0][3] = builder::long(b"Mdpn", 10));
    let unaligned = Gd::take().edited(|g| g.top.push(builder::boolean(b"Algn", 0)));
    // (name, body, masked, the loss fragment). Midpoints, Smoothness
    // and Align with Layer are carried, so none is a loss, masked or
    // not; the FIRST stop's `Mdpn` is never drawn (it governs no
    // segment).
    let cases: Vec<(&'static str, Vec<u8>, bool, Option<&str>)> = vec![
        ("Mid", mid, false, None),
        ("TMid", tmid, false, None),
        ("Smooth", smooth, false, None),
        ("Method", perc, false, method),
        (
            "Offset",
            ofst,
            false,
            Some("gradient Offset is (12.5 %, -4 %)"),
        ),
        ("Dither", dthr, false, Some("gradient is dithered")),
        ("Last", last, false, None),
        ("Plain", Gd::take().edited(|_| {}), false, None),
        ("Aligned", Gd::take().edited(|_| {}), true, None),
        ("Unaligned", unaligned, true, None),
    ];
    let mut base = fixture8::Record::raster([0, 0, 6, 8], &[200; 192], false);
    base.name = "Base";
    let mask = fixture8::ExpectedMask {
        bounds: [1, 1, 4, 5],
        coverage: vec![255; 12],
        outside: 0,
        enabled: true,
    };
    let mut records = vec![base];
    for (name, bytes, masked, _) in &cases {
        let record = empty8(name, b"GdFl", bytes);
        records.push(if *masked {
            record.mask(&mask, false)
        } else {
            record
        });
    }
    let path = dir.path().join("losses.psd");
    std::fs::write(&path, fixture8::fixture(&records, [8, 6], false)).unwrap();
    let report = import_report(&path);
    let losses: Vec<&str> = report["verified_losses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let observed = report["observations"].to_string();
    // The report counts layers top first.
    for (index, (name, _, _, fragment)) in cases.iter().enumerate() {
        let layer = format!(
            "Layer {} (“{name}”) is a Photoshop Gradient Fill layer (GdFl)",
            cases.len() - 1 - index
        );
        assert!(
            observed.contains(&format!("{layer} with no pixels;")),
            "{name}: {observed}"
        );
        let mine: Vec<String> = losses
            .iter()
            .filter(|l| l.contains(&format!("(“{name}”)")))
            .map(|l| l.to_string())
            .collect();
        let want: Vec<String> = fragment
            .iter()
            .map(|f| format!("{layer} whose {f}; layered import carries the fill without it, so its appearance can differ from Photoshop's."))
            .collect();
        assert_eq!(mine, want, "{name}");
    }
    let opened = open8(&path);
    assert_eq!(
        opened.layers.len(),
        records.len(),
        "every gradient is carried"
    );
    // The midpoints are read, one stop earlier: `Mdpn` 30 on
    // the second colour stop is the first stop's midpoint, 70 on the third
    // transparency stop the second's opacity midpoint; the first stop's
    // `Mdpn` governs nothing.
    let stops = |layer: usize| {
        let d = parse(opened.layers[layer].meta.as_deref().expect("a description"));
        d["gradient"]["stops"].clone()
    };
    let mid = stops(1);
    assert_eq!(mid[0]["midpoint"], json!(0.3), "{mid}");
    assert!(mid[0].get("opacity_midpoint").is_none(), "{mid}");
    let tmid = stops(2);
    assert_eq!(tmid[1]["opacity_midpoint"], json!(0.7), "{tmid}");
    assert!(tmid[1].get("midpoint").is_none(), "{tmid}");
    let last = stops(7);
    assert!(
        last.as_array()
            .unwrap()
            .iter()
            .all(|s| s.get("midpoint").is_none()),
        "{last}"
    );
}

#[test]
fn a_gdfl_that_does_not_decode_is_omitted_or_stays_a_raster() {
    let dir = TempDir::new().unwrap();
    let ink = cmyc_items(&[[10.0, 20.0, 30.0, 0.0]; 3]);
    let trns = Gd::take().edited(|g| g.trns[1][1] = builder::long(b"Lctn", 1000));
    let noise = Gd::take().edited(|g| g.grdn[1] = builder::enumeration(b"GrdF", b"GrdF", b"ClNs"));
    let burst = builder::enumeration(b"Type", b"GrdT", b"shapeburst");
    let burst = Gd::take().edited(|g| g.top[2] = burst);
    let cases: Vec<(&'static str, Vec<u8>, &str)> = vec![
        (
            "Trns",
            trns,
            "gradient transparency stops do not match its colour stops",
        ),
        ("Noise", noise, "gradient is not a custom colour gradient"),
        (
            "Ink",
            Gd::new(ink, &[0, 2048, 4096]).edited(|_| {}),
            "effect colour is not an RGB descriptor",
        ),
        ("Burst", burst, "gradient.style must be one of"),
    ];
    let mut base = fixture8::Record::raster([0, 0, 2, 2], &[200; 16], false);
    base.name = "Base";
    let mut records = vec![base];
    for (name, bytes, _) in &cases {
        records.push(empty8(name, b"GdFl", bytes));
    }
    let mut raster = fixture8::Record::raster([0, 0, 2, 2], &[90; 16], false);
    raster.name = "Raster";
    raster.flags = 0x18;
    raster.blocks = fixture8::block(b"GdFl", &cases[0].1);
    records.push(raster);
    let path = dir.path().join("declined8.psd");
    std::fs::write(&path, fixture8::fixture(&records, [2, 2], false)).unwrap();
    let losses = import_report(&path)["verified_losses"].to_string();
    for (name, _, reason) in &cases {
        let want = format!(
            "(“{name}”) is a Photoshop Gradient Fill layer (GdFl) with no pixels whose {reason}"
        );
        assert!(losses.contains(&want), "{name}: {losses}");
    }
    for fragment in [
        "(“Raster”) has adjustment/fill metadata",
        "(“Raster”) declares its raster pixels irrelevant",
    ] {
        assert!(losses.contains(fragment), "{fragment}: {losses}");
    }
    let doc = open8(&path);
    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Base", "Raster"]);
    assert!(
        doc.layers[1].meta.is_none(),
        "the raster keeps its pixels only"
    );

    // CMYK: a stop that is an `RGBC`, pixel-less and on a raster.
    let samples = vec![[1000u16, 2000, 3000, 4000, 65535]; 4];
    let channels = cmyk_psd::ink_channels(&samples, 2, 8, true, Comp::Raw, false);
    let mut raster = Rec::raster("Raster", [0, 0, 2, 2], channels.clone());
    raster.flags = 0x18;
    let rgb = Gd::take().edited(|_| {});
    let records = vec![
        Rec::raster("Base", [0, 0, 2, 2], channels),
        Rec::empty("Rgb").with_block(b"GdFl", &rgb),
        raster.with_block(b"GdFl", &rgb),
    ];
    let profile = press_profile("Press Test");
    let spec = PsdSpec {
        canvas: (2, 2),
        depth: 8,
        icc: Some(&profile),
        composite: (0..4).map(|_| vec![255; 4]).collect(),
        merged_alpha: false,
        large: false,
        lr16: false,
        resources: Vec::new(),
    };
    let path = dir.path().join("declined-cmyk.psd");
    std::fs::write(&path, cmyk_psd::psd(&spec, &records)).unwrap();
    let losses = import_report(&path)["verified_losses"].to_string();
    assert!(
        losses.contains("(“Rgb”) is a Photoshop Gradient Fill layer (GdFl) with no pixels whose gradient stop colour is not a CMYK colour; layered import omits it and its mask."),
        "{losses}"
    );
    assert!(
        losses.contains("(“Raster”) has adjustment/fill metadata"),
        "{losses}"
    );
    let back = open(&path);
    assert_eq!(
        unsafe { &*back }.layer_count(),
        2,
        "the pixel-less one is omitted"
    );
    assert!(
        cmyk_description(back, 1).is_none(),
        "the raster keeps its pixels only"
    );
    free(back);
}

#[test]
fn a_profile_that_cannot_be_rendered_into_declines_its_gradient_like_its_solid() {
    let dir = TempDir::new().unwrap();
    let g = gradient("linear", 30.0, 0.8, &stop_hex());
    let doc = byte_doc(
        &IccProfile::srgb(),
        &rgb_fill(&g, &unit_colors(), None),
        false,
    );
    let color = [0.3, 0.6, 0.15, 1.0];
    let solid = (
        json!([{"kind": "fill", "color": color}]).to_string(),
        json!({"type": "fill", "version": 1, "fill": "solid", "color": color, "canvas": [W, H]})
            .to_string(),
    );
    let doc = publish8(&doc, 1, &solid, "Solid");
    let path = dir.path().join("srgb.psd");
    save8(&doc, &path);
    let bytes = pixel_less(&pixel_less(&std::fs::read(&path).unwrap(), "Grad"), "Solid");
    let path = dir.path().join("falling.psd");
    std::fs::write(&path, retag(&bytes, &falling_profile())).unwrap();
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    let losses = report["verified_losses"].to_string();
    for (name, kind) in [
        ("Grad", "Gradient Fill layer (GdFl)"),
        ("Solid", "Solid Color Fill layer (SoCo)"),
    ] {
        let want = format!("(“{name}”) is a Photoshop {kind} with no pixels whose colour cannot be rendered in the file's profile (");
        assert!(losses.contains(&want), "{name}: {losses}");
    }
    assert_eq!(open8(&path).layers.len(), 1, "both are omitted");
}

#[test]
fn a_carried_gradient_is_admitted_as_a_canvas_raster_not_its_rectangle() {
    let dir = TempDir::new().unwrap();
    let doc = cmyk_doc(8, (W, H), &INKS, true);
    let path = dir.path().join("grad-cmyk8.psd");
    save_layers(doc, &path);
    free(doc);
    // The fill's key renamed, so the same record is a plain raster of its
    // 60 × 44 rectangle.
    let mut raster = std::fs::read(&path).unwrap();
    let at = raster
        .windows(8)
        .position(|w| w == b"8BIMGdFl")
        .expect("the GdFl");
    raster[at + 4..at + 8].copy_from_slice(b"XdFl");
    let raster_path = dir.path().join("raster-cmyk8.psd");
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
        fill >= plain + uncovered * 10,
        "fill {fill}, raster {plain}, uncovered {uncovered}"
    );
}

// ------------------------------------------------- Photoshop's own files --

/// Per pixel the largest channel difference over the first `channels` of each
/// `stride`, in 8-bit codes: its maximum and mean.
fn residual(ours: &[f64], theirs: &[f64], channels: usize, stride: usize) -> (f64, f64) {
    let errors: Vec<f64> = ours
        .chunks(stride)
        .zip(theirs.chunks(stride))
        .map(|(a, b)| {
            (0..channels)
                .map(|c| (a[c] - b[c]).abs())
                .fold(0.0, f64::max)
        })
        .collect();
    let worst = errors.iter().copied().fold(0.0, f64::max);
    (worst, errors.iter().sum::<f64>() / errors.len() as f64)
}

/// GA8's SEAM bin (the controller's RULING-1: the bin declared for the
/// identify rows applies to the existing Angle row as well;
/// `gradient_identify_673_fit.py`'s `ga8_seam`, restated): a pixel of the
/// 64 × 48 canvas whose closed square reaches within 2° of the seam θ = 30°
/// (the gradient angle), θ = atan2(−dy, dx) from the canvas centre, read at
/// its four corners and its centre — one within 2°, or two corners on either
/// side of it (signed differences under 90°). A pixel straddling the
/// discontinuity reads the first or the last stop on any sub-pixel offset.
fn ga8_seam(x: usize, y: usize) -> bool {
    let (w, h, angle) = (64.0, 48.0, 30.0);
    let gap = |ox: f64, oy: f64| {
        let theta = (-(y as f64 + oy - h / 2.0)).atan2(x as f64 + ox - w / 2.0);
        (theta.to_degrees() - angle + 180.0).rem_euclid(360.0) - 180.0
    };
    let corners = [gap(0.0, 0.0), gap(1.0, 0.0), gap(0.0, 1.0), gap(1.0, 1.0)];
    let near = corners
        .iter()
        .chain([gap(0.5, 0.5)].iter())
        .any(|d| d.abs() <= 2.0);
    let side = |positive: bool| {
        corners
            .iter()
            .any(|d| d.abs() < 90.0 && (*d > 0.0) == positive && *d != 0.0)
    };
    near || (side(true) && side(false))
}

/// [`residual`] over the pixels outside GA8's [`ga8_seam`] bin (RGBA bytes
/// of the 64 × 48 canvas, the first three channels), plus the count of the
/// pixels inside it off by more than 2 codes.
fn ga8_residual(ours: &[f64], theirs: &[f64]) -> ((f64, f64), usize) {
    let (mut outside_ours, mut outside_theirs, mut off) = (Vec::new(), Vec::new(), 0);
    for (i, (a, b)) in ours.chunks(4).zip(theirs.chunks(4)).enumerate() {
        if ga8_seam(i % 64, i / 64) {
            off += usize::from((0..3).any(|c| (a[c] - b[c]).abs() > 2.0));
        } else {
            outside_ours.extend_from_slice(a);
            outside_theirs.extend_from_slice(b);
        }
    }
    (residual(&outside_ours, &outside_theirs, 3, 4), off)
}

/// The lowered pins of Photoshop's own files (`key`, max, mean), measured
/// on the shipped Linear, Angle, Reflected and Radial law (GF8 44 / 20.55 →
/// 20 / 5.837 on Reflected's; GR8 77 / 23.86 → 7 / 1.895 on Radial's in
/// take 2; GM8 89 / 16.337 → 9 / 0.885 once its aligned gradient spans its
/// mask's coverage box) and rounded up in the third decimal;
/// a key here is held to the lower of this and `photoshop.json`'s READ
/// bound. GA8's is outside its [`ga8_seam`] bin, whose pixels off by more
/// than 2 codes are pinned at [`GA8_SEAM_OFF`] (today's kernel: 69).
const LOWERED_673: &[(&str, f64, f64)] = &[
    ("GA8", 7.0, 2.493),
    ("GC8", 12.0, 4.327),
    ("GF8", 20.0, 5.837),
    ("GL16", 9.432, 3.654),
    ("GL8", 10.0, 3.698),
    ("GM8", 9.0, 0.885),
    ("GR8", 7.0, 1.895),
];

/// GA8's seam pixels off by more than 2 codes on the shipped law.
const GA8_SEAM_OFF: usize = 16;

/// A 16-bit TIFF's composite as 8-bit codes (v / 257).
fn tiff16(path: &Path) -> Vec<f64> {
    let c = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        doctored_core::ffi_precision::dr_precision_doc_open(
            c.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    let samples = unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap();
    free(doc);
    samples
        .iter()
        .flat_map(|p| p.map(|v| f64::from(v) / 257.0))
        .collect()
}

/// The take's seven `GdFl` files (`fixtures/psd_photoshop/gradient_fill_layer`,
/// `scripts/photoshop-probe/gradient_fill_672_take.py --export-fixture`):
/// each read as its gradient, the layer a fresh render of the expected
/// component within 1 code, the flattened document within `photoshop.json`'s
/// recorded READ of Photoshop's TIFF or the lower [`LOWERED_673`] pin (GA8
/// outside its [`ga8_seam`] bin; GM8 spanning its mask's box), and the
/// re-export keeping Photoshop's numbers.
#[test]
fn photoshops_own_gradient_fill_layers_read_as_their_gradient_within_the_bound() {
    let dir = TempDir::new().unwrap();
    let manifest = crate::common::psd_photoshop::fixture("gradient_fill_layer", "photoshop.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let files = manifest["files"].as_object().unwrap();
    assert_eq!(files.len(), 7, "the take's seven GdFl cases");
    for (key, entry) in files {
        let path = |kind: &str| {
            let name = entry[kind]["file"].as_str().unwrap();
            crate::common::psd_photoshop::fixture("gradient_fill_layer", name)
        };
        for kind in ["psd", "tiff"] {
            let sha = crate::common::sha256::sha256_hex(&std::fs::read(path(kind)).unwrap());
            assert_eq!(
                sha,
                entry[kind]["sha256"].as_str().unwrap(),
                "{key}: the take's {kind}"
            );
        }
        let bytes = std::fs::read(path("psd")).unwrap();
        let cmyk = entry["case"]["mode"] == "cmyk";
        let psd = if cmyk {
            // Saved untagged (phase 1's rule): tagged with the press profile.
            let tagged = dir.path().join(format!("{key}-tagged.psd"));
            std::fs::write(&tagged, retag(&bytes, &press_profile("Press Test"))).unwrap();
            tagged
        } else {
            path("psd")
        };
        let theirs = read_body(&blocks_in(&bytes, b"GdFl")[0]);
        let report = import_report(&psd);
        let pixel_less = entry["form"]["bounds"] == json!([0, 0, 0, 0]);
        let form = if pixel_less {
            "with no pixels;"
        } else {
            "in Photoshop's form"
        };
        let layer = "(“GF”) is a Photoshop Gradient Fill layer (GdFl)";
        let observed = report["observations"].to_string();
        assert!(
            observed.contains(&format!("{layer} {form}")),
            "{key}: {observed}"
        );
        // Align with Layer is carried: no file names a loss.
        let losses = report["verified_losses"].to_string();
        assert_eq!(losses.matches("(“GF”)").count(), 0, "{key}: {losses}");
        // The file's `Algn` (absent = true) and the box an aligned fill
        // spans: GM8's 40 × 30 mask at (8, 6), else the 64 × 48 canvas.
        let align = theirs.get("Algn").is_none_or(|v| v.1 != 0.0);
        let frame = (key == "GM8").then_some([8, 6, 40, 30]);
        assert!(key != "GM8" || align, "GM8 is aligned");
        let style = match key.as_str() {
            "GR8" => "radial",
            "GA8" => "angle",
            "GF8" => "reflected",
            _ => "linear",
        };
        let read = &entry["read"];
        let mut bound = (
            read["max"].as_f64().unwrap().ceil(),
            read["mean"].as_f64().unwrap() + 0.01,
        );
        if let Some((_, max, mean)) = LOWERED_673.iter().find(|p| p.0 == key.as_str()) {
            bound = (bound.0.min(*max), bound.1.min(*mean));
        }
        let within = |(worst, mean): (f64, f64)| {
            assert!(
                worst <= bound.0 && mean <= bound.1,
                "{key}: {worst} / {mean} vs {bound:?}"
            );
        };
        let again = dir.path().join(format!("{key}-again.psd"));
        if cmyk {
            let inks: Vec<Vec<f64>> = (0..3).map(|i| cmyc(&theirs, i)).collect();
            let back = open(&psd);
            let d = cmyk_description(back, 1).expect("the fill's description");
            assert_eq!(d["gradient"]["style"], style, "{key}");
            for (got, want) in rows(&d, "stop_inks").iter().zip(&inks) {
                near(
                    got,
                    &want.iter().map(|v| v / 100.0).collect::<Vec<_>>(),
                    1e-4,
                    key,
                );
            }
            // The layer is a fresh render of the expected component.
            let unit: Vec<[f64; 4]> = inks
                .iter()
                .map(|i| std::array::from_fn(|c| i[c] / 100.0))
                .collect();
            let fresh = cmyk_doc(8, (64, 48), &unit, false);
            let got = unsafe { &*back }.cmyk_layer_samples(1).unwrap();
            near_samples(
                &got,
                &unsafe { &*fresh }.cmyk_layer_samples(1).unwrap(),
                257,
                key,
            );
            free(fresh);
            // Flattened onto paper (no ink) against Photoshop's TIFF.
            let tiff =
                crate::cmyk_psd_fixture::read_flat_tiff(&std::fs::read(path("tiff")).unwrap());
            let ours: Vec<f64> = got
                .iter()
                .flat_map(|s| {
                    (0..4)
                        .map(move |c| (f64::from(s[c]) * f64::from(s[4]) / 65535.0 / 257.0).round())
                })
                .collect();
            let theirs_flat: Vec<f64> = (0..got.len())
                .flat_map(|i| tiff.planes.iter().take(4).map(move |p| f64::from(p[i])))
                .collect();
            within(residual(&ours, &theirs_flat, 4, 4));
            save_layers(back, &again);
            free(back);
        } else {
            let numbers: Vec<Vec<f64>> = (0..3).map(|i| rgbc(&theirs, i)).collect();
            let hex: Vec<String> = numbers
                .iter()
                .map(|c| hex_of(std::array::from_fn(|k| c[k].round() as u8)))
                .collect();
            let colors: Vec<[f64; 3]> = numbers
                .iter()
                .map(|c| std::array::from_fn(|k| c[k].round() / 255.0))
                .collect();
            let g = gradient(style, 30.0, 0.8, &hex);
            let mut components: Value =
                serde_json::from_str(&rgb_fill(&g, &colors, None).0).unwrap();
            if let Some(frame) = frame {
                components[0]["box"] = json!(frame);
            }
            let fresh = render(&IccProfile::srgb(), &components.to_string(), (64, 48));
            let fresh = fresh.to_rgba16().unwrap();
            let check = |d: &Value| {
                assert_gradient(d, style, 30.0, 0.8, &hex, align, key);
                assert!(d.get("stop_linears").is_none(), "{key}: sRGB stops");
            };
            let want_box = frame.unwrap_or([0, 0, 64, 48]);
            if entry["case"]["depth"] == 16 {
                let doc = open16(&psd);
                check(&parse(doc.artwork_description(1).expect("a description")));
                let mut xywh = [0i32; 4];
                assert!(unsafe { dr_precision_doc_fill_align_box(&doc, 1, xywh.as_mut_ptr()) });
                assert_eq!(xywh, want_box, "{key}: the align box");
                let layer = doc.layer(1).unwrap().source().unwrap().to_rgba16().unwrap();
                near_samples(&layer, &fresh, 257, key);
                let ours = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
                let ours: Vec<f64> = ours
                    .iter()
                    .flat_map(|p| p.map(|v| f64::from(v) / 257.0))
                    .collect();
                within(residual(&ours, &tiff16(&path("tiff")), 3, 4));
                save16(&doc, &again);
            } else {
                let doc = open8(&psd);
                let layer = &doc.layers[1];
                check(&parse(layer.meta.as_deref().expect("a description")));
                let mut xywh = [0i32; 4];
                assert!(unsafe { dr_doc_fill_align_box(&doc, 1, xywh.as_mut_ptr()) });
                assert_eq!(xywh, want_box, "{key}: the align box");
                assert_eq!(layer.offset, (0, 0), "{key}");
                let pixels: Vec<[u16; 4]> = layer
                    .pixels
                    .to_contiguous()
                    .unwrap()
                    .pixels()
                    .map(|p| p.0.map(u16::from))
                    .collect();
                let fresh: Vec<[u16; 4]> = fresh
                    .iter()
                    .map(|p| p.map(|v| ((u32::from(v) + 128) / 257) as u16))
                    .collect();
                near_samples(&pixels, &fresh, 1, key);
                if key == "GM8" {
                    let mask = layer.mask.as_deref().expect("GM8's mask");
                    assert_eq!(
                        (mask.dimensions(), layer.mask_properties.offset),
                        ((40, 30), (8, 6))
                    );
                }
                let theirs_flat = DrDocument::open(path("tiff").to_str().unwrap())
                    .unwrap()
                    .flattened()
                    .unwrap();
                let as_f = |raw: &[u8]| raw.iter().map(|v| f64::from(*v)).collect::<Vec<_>>();
                let (ours, theirs_flat) = (
                    as_f(doc.flattened().unwrap().as_raw()),
                    as_f(theirs_flat.as_raw()),
                );
                if key == "GA8" {
                    let (outside, off) = ga8_residual(&ours, &theirs_flat);
                    assert!(off <= GA8_SEAM_OFF, "GA8: {off} seam pixels off by > 2");
                    within(outside);
                } else {
                    within(residual(&ours, &theirs_flat, 3, 4));
                }
                save8(&doc, &again);
            }
        }
        // The re-export keeps Photoshop's numbers.
        let ours = gdfl_of(&std::fs::read(&again).unwrap(), "GF");
        assert_eq!(id(&ours, "Type"), id(&theirs, "Type"), "{key}: Type");
        let top = |f: &Flat| [n(f, "Angl"), n(f, "Scl ")];
        near(&top(&ours), &top(&theirs), 1e-5, key);
        for i in 0..3 {
            let lctn = |f: &Flat| n(f, &format!("Grad/Clrs/{i}/Lctn"));
            assert_eq!(lctn(&ours), lctn(&theirs), "{key}: Lctn");
            if cmyk {
                near(&cmyc(&ours, i), &cmyc(&theirs, i), 0.4, key);
            } else {
                near(&rgbc(&ours, i), &rgbc(&theirs, i), 1.0, key);
            }
            let opct = |f: &Flat| n(f, &format!("Grad/Trns/{i}/Opct"));
            near(&[opct(&ours)], &[opct(&theirs)], 0.5, key);
        }
    }
}
