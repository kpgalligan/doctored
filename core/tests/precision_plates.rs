//! Black-box suite for 13B-3c-2c-1: the retouch plates, the selection source
//! and the typed resample inputs leave no contiguous view. The Current plate
//! copies the layer's rows, the cut plate renders `Unseeded`, the transformed
//! plate and every resample input read ONE transient copy of the whole plane,
//! and the selection source is copied onto the canvas by rows — with every
//! answer, checkpoint count, report log and refusal unchanged. Since
//! 13B-3d-3b the resample outputs are seeded or not per caller (always
//! unseeded since 13B-3e-4c-2a); the geometry outputs installed as a layer
//! are unseeded
//! (`precision_filled_outputs.rs`' `no_view_resample_outputs`).
//!
//! Oracles are independent of the implementation: the Current plate is the
//! layer placed on a zero canvas HERE, the source selections are the alpha /
//! Rec.709 luminance (`0.2126 R + 0.7152 G + 0.0722 B`, rounded) / green code
//! of that placement or of the render, a nearest resize is the pixel-centre
//! point rule `s = (2d + 1)·S / (2D)` and the float quarter turn and flip are
//! written-out permutations.
//!
//! Every `record_*` test also prints one line per case —
//! `plates <label> <WxH> <sha256> checkpoints=<n> reports=<sha256>` — so the
//! same file run against HEAD and the tree diffs byte-identical answers and
//! checkpoint/report sequences. It uses only APIs present at `549b10e`.
//!
//! `no_view_*` tests fail at `549b10e` by design. The view cache is
//! process-wide, so every test holds `serial()`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_select};
use doctored_core::ffi_retouch::{
    dr_precision_doc_retouch_source, dr_precision_image_transform_retouch_source,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use doctored_core::tiff_float::FloatTiff;
use serde_json::{json, Value};

use crate::common::sha256;
use crate::resource_report;
use crate::serial;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 600;
const H: u32 = 520;
/// 600×520 crosses both tile seams (x 256, 512; y 256, 512).
const FIXTURES: [(u32, u32); 3] = [(W, H), (257, 3), (1, 1)];
/// `RetouchSampling::from_code`'s three codes.
const MODES: [(u32, &str); 3] = [(0, "current"), (1, "current_and_below"), (2, "all")];

// ------------------------------------------------------------- plumbing --

fn pattern16(width: u32, height: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                (9000 + (x * 3001 + y * 1777 + seed * 97) % 56536) as u16,
            ]
        })
        .collect()
}

/// Extended-range samples (negative and above-white RGB) with alpha 0
/// (hidden RGB), 1 and fractions, row-major.
fn extended(width: u32, height: u32, seed: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let v = ((x * 37 + y * 101 + seed * 13) % 1000) as f32 / 1000.0;
            let r = if (x + y) % 9 == 0 { 0.0 } else { v * 6.0 - 1.0 };
            let g = if (x * y) % 7 == 3 { -0.25 } else { 1.0 - v };
            let b = ((x ^ y) % 97) as f32 / 24.0;
            let a = match (x + 2 * y) % 11 {
                0 => 0.0,
                1 | 2 => 1.0,
                k => k as f32 / 11.0,
            };
            [r, g, b, a]
        })
        .collect()
}

fn image16(width: u32, height: u32, pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(width, height, pixels, IccProfile::srgb(), None).unwrap()
}

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    text
}

fn evict() {
    resource_report::evict(&[]).expect("an eviction report");
}

/// The contiguous views held over `documents` (the report's top-level
/// `contiguous_view_bytes`).
fn document_views(documents: &[&PrecisionDocument]) -> u64 {
    let handles: Vec<*const PrecisionDocument> = documents.iter().map(|d| *d as *const _).collect();
    let value = resource_report::report(&[], &[], &handles, &[], 0, 0).expect("a resource report");
    value["contiguous_view_bytes"].as_u64().unwrap()
}

/// Runs `call` under a fresh recording `DrJobControl`; answers the call's
/// value, the checkpoint count and the raw report log.
fn recorded<T>(call: impl FnOnce(&DrJobControl) -> T) -> (T, u64, String) {
    let control = DrJobControl::default();
    control.record_reports();
    let out = call(&control);
    let log: String = control
        .reports()
        .iter()
        .map(|r| format!("{} {} {}\n", r.checkpoints, r.progress.to_bits(), r.stage))
        .collect();
    (out, control.checkpoints(), log)
}

/// Accumulates one line's answers, checkpoints and report logs.
#[derive(Default)]
struct Line {
    answers: Vec<u8>,
    checkpoints: u64,
    log: String,
}

impl Line {
    fn add<T>(&mut self, (out, checkpoints, log): (T, u64, String)) -> T {
        self.checkpoints += checkpoints;
        self.log.push_str(&log);
        out
    }

    fn print(&self, label: &str, (w, h): (u32, u32)) {
        println!(
            "plates {label} {w}x{h} {} checkpoints={} reports={}",
            sha256::sha256_hex(&self.answers),
            self.checkpoints,
            sha256::sha256_hex(self.log.as_bytes())
        );
    }
}

fn u16_bytes(samples: &[[u16; 4]]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()))
        .collect()
}

fn bits(samples: &[[f32; 4]]) -> Vec<[u32; 4]> {
    samples.iter().map(|p| p.map(f32::to_bits)).collect()
}

/// An image's dimensions and samples as bytes (u16 codes, f32 bits).
fn image_bytes(image: &PrecisionImage) -> Vec<u8> {
    let (w, h) = image.dimensions();
    let mut out: Vec<u8> = [w, h].iter().flat_map(|v| v.to_le_bytes()).collect();
    match image.storage() {
        PrecisionStorage::Rgba16Unorm => out.extend(u16_bytes(&image.to_rgba16().unwrap())),
        PrecisionStorage::Rgba32Float => out.extend(
            bits(&image.to_rgba32().unwrap())
                .iter()
                .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes())),
        ),
    }
    out
}

/// Every record's offset and raster samples, in postorder.
fn document_bytes(doc: &PrecisionDocument) -> Vec<u8> {
    let (w, h) = doc.dimensions();
    let mut out: Vec<u8> = [w, h].iter().flat_map(|v| v.to_le_bytes()).collect();
    for i in 0..doc.layer_count() {
        let layer = doc.layer(i).unwrap();
        out.extend(layer.name().bytes());
        let (x, y) = layer.offset();
        out.extend(x.to_le_bytes());
        out.extend(y.to_le_bytes());
        if let Some(image) = layer.source() {
            out.extend(image_bytes(image));
        }
    }
    out
}

/// A result's answer bytes: `Ok(Some)` the document, `Ok(None)` "none", an
/// error its sentence.
fn outcome_bytes(outcome: &Result<Option<PrecisionDocument>, String>) -> Vec<u8> {
    match outcome {
        Ok(Some(doc)) => document_bytes(doc),
        Ok(None) => b"none".to_vec(),
        Err(e) => format!("error: {e}").into_bytes(),
    }
}

/// The "Off" layer's rect for a `w`×`h` canvas: at 600×520, 300×200 at
/// (−100, 400) — partly off the canvas, across the y 512 seam.
fn off_rect(w: u32, h: u32) -> (u32, u32, i32, i32) {
    (
        (w / 2).max(1),
        (h * 5 / 13).max(1),
        -((w / 6) as i32),
        (h * 10 / 13) as i32,
    )
}

/// Two u16 rasters: an opaque canvas-sized base and the "Off" layer at
/// opacity 0.8, partly off the canvas.
fn plates_document(w: u32, h: u32) -> PrecisionDocument {
    let (ow, oh, ox, oy) = off_rect(w, h);
    let base: Vec<[u16; 4]> = pattern16(w, h, 1)
        .into_iter()
        .map(|[r, g, b, _]| [r, g, b, u16::MAX])
        .collect();
    let layers = vec![
        Layer::raster("Base", image16(w, h, base), (0, 0), 1.0, true).unwrap(),
        Layer::raster(
            "Off",
            image16(ow, oh, pattern16(ow, oh, 2)),
            (ox, oy),
            0.8,
            true,
        )
        .unwrap(),
    ];
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

/// A float document holding one `width`×`height` raster of `samples` (RGBA,
/// straight alpha), through a float TIFF opened with `open_preserving`.
fn float_document(width: u32, height: u32, samples: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples: samples.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("plates.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// `source` (`sw`×`sh`, row-major) placed at `offset` on a zero `w`×`h`
/// canvas.
fn canvas_mapped(
    source: &[[u16; 4]],
    (sw, sh): (u32, u32),
    offset: (i32, i32),
    (w, h): (u32, u32),
) -> Vec<[u16; 4]> {
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let lx = i64::from(x) - i64::from(offset.0);
            let ly = i64::from(y) - i64::from(offset.1);
            out.push(
                if lx >= 0 && ly >= 0 && lx < i64::from(sw) && ly < i64::from(sh) {
                    source[ly as usize * sw as usize + lx as usize]
                } else {
                    [0; 4]
                },
            );
        }
    }
    out
}

/// Layer `index` placed on the document's zero canvas.
fn layer_on_canvas(doc: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    let layer = doc.layer(index).unwrap();
    let image = layer.source().unwrap();
    canvas_mapped(
        &image.to_rgba16().unwrap(),
        image.dimensions(),
        layer.offset(),
        doc.dimensions(),
    )
}

fn retouch_source(
    doc: &PrecisionDocument,
    layer: usize,
    mode: u32,
    control: Option<&DrJobControl>,
) -> Result<Box<PrecisionImage>, String> {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_retouch_source(
            doc,
            layer,
            mode,
            false,
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if out.is_null() {
        return Err(take(error));
    }
    assert!(error.is_null());
    Ok(unsafe { Box::from_raw(out) })
}

fn transformed(
    image: &PrecisionImage,
    matrix: [f64; 6],
    control: Option<&DrJobControl>,
) -> Result<Box<PrecisionImage>, String> {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_image_transform_retouch_source(
            image,
            matrix.as_ptr(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if out.is_null() {
        return Err(take(error));
    }
    assert!(error.is_null());
    Ok(unsafe { Box::from_raw(out) })
}

fn edit(
    doc: &PrecisionDocument,
    request: &Value,
    coverage: &[u16],
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if !error.is_null() {
        assert!(out.is_null(), "a failure publishes nothing");
        return Err(take(error));
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn select(
    doc: &PrecisionDocument,
    request: &Value,
    control: Option<&DrJobControl>,
) -> Result<Vec<u16>, String> {
    let (w, h) = doc.dimensions();
    let mut out = vec![999_u16; (w * h) as usize];
    let text = CString::new(request.to_string()).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if !ok {
        return Err(take(error));
    }
    Ok(out)
}

fn geometry(
    doc: &PrecisionDocument,
    request: &Value,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let parsed = GeometryEdit::parse(&request.to_string()).expect("a geometry request");
    doc.geometry(&parsed, LIMIT, 0, control)
}

/// Canvas coverage 65535 inside the rect `(x, y, w, h)` (clipped).
fn boxed(canvas: (u32, u32), (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u16> {
    let (cw, ch) = canvas;
    (0..cw * ch)
        .map(|i| {
            let (px, py) = (i % cw, i / cw);
            if px >= x && py >= y && px < x + w && py < y + h {
                u16::MAX
            } else {
                0
            }
        })
        .collect()
}

/// A rect across both seams where the canvas has them, else the middle.
fn seam_box(w: u32, h: u32) -> (u32, u32, u32, u32) {
    let span = |n: u32| {
        if n > 256 {
            (248, 16)
        } else {
            (n / 2, 1)
        }
    };
    let ((x, bw), (y, bh)) = (span(w), span(h));
    (x, y, bw, bh)
}

// ------------------------------------------------------------ record_* --

#[test]
fn record_retouch_source() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let doc = plates_document(w, h);
        let projection = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
        for layer in [0, 1] {
            for (mode, name) in MODES {
                let mut line = Line::default();
                evict();
                let plate = line
                    .add(recorded(|c| retouch_source(&doc, layer, mode, Some(c))))
                    .expect("a plate");
                assert_eq!(plate.dimensions(), (w, h));
                let samples = plate.to_rgba16().unwrap();
                if mode == 0 {
                    assert_eq!(
                        samples,
                        layer_on_canvas(&doc, layer),
                        "the Current plate is the layer on the canvas ({w}×{h}, layer {layer})"
                    );
                }
                if mode == 2 {
                    assert_eq!(
                        samples, projection,
                        "the All plate is the projection ({w}×{h})"
                    );
                }
                line.answers.extend(image_bytes(&plate));
                line.print(&format!("retouch_source/{name}/layer{layer}"), (w, h));
            }
        }
        let refusal = retouch_source(&doc, 2, 0, None).map(|_| ()).unwrap_err();
        assert_eq!(refusal, "retouch requires a raster source layer");
    }
}

#[test]
fn record_transform_retouch_source() {
    let _serial = serial();
    let (c, s) = (17_f64.to_radians().cos(), 17_f64.to_radians().sin());
    let matrices: [(&str, [f64; 6]); 4] = [
        ("identity", [1., 0., 0., 1., 0., 0.]),
        ("translate", [1., 0., 0., 1., 0.5, -0.25]),
        ("rotate", [c, s, -s, c, 13.5, -40.25]),
        ("scale", [1.5, 0., 0., 1.5, -10., -5.]),
    ];
    for (w, h) in FIXTURES {
        let doc = plates_document(w, h);
        let plate = retouch_source(&doc, 1, 0, None).expect("the Current plate");
        let input = plate.to_rgba16().unwrap();
        for (label, matrix) in matrices {
            let mut line = Line::default();
            evict();
            let out = line
                .add(recorded(|c| transformed(&plate, matrix, Some(c))))
                .expect("a transformed plate");
            assert_eq!(out.dimensions(), (w, h));
            if label == "identity" {
                // One tap of weight 1: the alpha code is the input's.
                let alpha: Vec<u16> = out.to_rgba16().unwrap().iter().map(|p| p[3]).collect();
                let expected: Vec<u16> = input.iter().map(|p| p[3]).collect();
                assert_eq!(alpha, expected, "identity alpha ({w}×{h})");
            }
            line.answers.extend(image_bytes(&out));
            line.print(&format!("transform_retouch_source/{label}"), (w, h));
        }
    }
}

#[test]
fn record_retouch_ops() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let doc = plates_document(w, h);
        let n = (w * h) as usize;
        let seam = seam_box(w, h);
        let wide = (w / 3, h / 3, (w / 3).max(1), (h / 3).max(1));
        // Maps `wide` onto the "Off" layer: at 600×520, (49.5, 373.25) on.
        let reach = (-f64::from(w / 4) - 0.5, f64::from(h * 5 / 13) + 0.25);
        let bounds = if w > 256 {
            (240, 240, 32, 32)
        } else {
            (0, 0, w, h)
        };
        let cases: [(&str, Value, Vec<u16>); 5] = [
            (
                "retouch_clone/layer",
                json!({"op":"retouch_clone","layer":0,"source_layer":1,"sample":"current",
                    "matrix":[1,0,0,1,reach.0,reach.1],"opacity":0.75}),
                boxed((w, h), wide),
            ),
            (
                "retouch_clone/new_layer",
                json!({"op":"retouch_clone","layer":1,"sample":"current_and_below",
                    "output":"new_layer","matrix":[1,0,0,1,-3.25,2.5]}),
                boxed((w, h), wide),
            ),
            (
                "retouch_heal/new_layer",
                json!({"op":"retouch_heal","layer":0,"sample":"all","output":"new_layer",
                    "matrix":[1,0,0,1,18,0],"opacity":0.5}),
                boxed((w, h), wide),
            ),
            (
                "spot_heal",
                json!({"op":"spot_heal","layer":0,"x":bounds.0,"y":bounds.1,
                    "width":bounds.2,"height":bounds.3,"seed":12345}),
                boxed((w, h), seam),
            ),
            (
                "content_aware_fill",
                json!({"op":"content_aware_fill","layer":0,"x":bounds.0,"y":bounds.1,
                    "width":bounds.2,"height":bounds.3,"seed":7}),
                boxed((w, h), seam),
            ),
        ];
        for (label, request, coverage) in cases {
            assert_eq!(coverage.len(), n);
            let mut line = Line::default();
            evict();
            let outcome = line.add(recorded(|c| edit(&doc, &request, &coverage, Some(c))));
            if (w, h) == (W, H) {
                assert!(
                    matches!(outcome, Ok(Some(_))),
                    "{label} {w}×{h}: {:?}",
                    outcome.as_ref().map(|_| ())
                );
            }
            line.answers.extend(outcome_bytes(&outcome));
            line.print(&format!("retouch/{label}"), (w, h));
        }
    }
}

/// The Rec.709 luminance code of a u16 pixel, rounded (the selection's
/// luminance source, written out).
fn luminance(p: [u16; 4]) -> u16 {
    let value = f64::from(0.2126_f32) * f64::from(p[0])
        + f64::from(0.7152_f32) * f64::from(p[1])
        + f64::from(0.0722_f32) * f64::from(p[2]);
    value.round().clamp(0., 65535.) as u16
}

/// A source selection's per-pixel code, written out.
type Code = fn([u16; 4]) -> u16;

#[test]
fn record_selection() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let doc = plates_document(w, h);
        let projection = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
        let layer_canvas = layer_on_canvas(&doc, 1);
        let (sx, sy) = (256.min(w - 1), 256.min(h - 1));
        for (source, layer, oracle) in [
            ("layer", Some(1), &layer_canvas),
            ("composite", None, &projection),
        ] {
            let kinds: [(&str, Value, Option<Code>); 6] = [
                ("alpha", json!({"kind":"alpha"}), Some(|p| p[3])),
                ("luminance", json!({"kind":"luminance"}), Some(luminance)),
                (
                    "green",
                    json!({"kind":"color","channel":"green"}),
                    Some(|p| p[1]),
                ),
                (
                    "magic_wand",
                    json!({"kind":"magic_wand","x":sx,"y":sy,"tolerance":3000}),
                    None,
                ),
                (
                    "color_range",
                    json!({"kind":"color_range","params":{"mode":"sampled",
                        "samples":[[30001,30001,30001]],"tolerance":0,"softness":0}}),
                    None,
                ),
                (
                    "quick",
                    json!({"kind":"quick","params":{"seeds":[[sx,sy]],"tolerance":0,"edge":0}}),
                    None,
                ),
            ];
            for (kind, operation, code) in kinds {
                let request =
                    json!({"op":"selection","source":source,"layer":layer,"operation":operation});
                let mut line = Line::default();
                evict();
                let out = line.add(recorded(|c| select(&doc, &request, Some(c))));
                if let Some(code) = code {
                    let expected: Vec<u16> = oracle.iter().map(|p| code(*p)).collect();
                    assert_eq!(
                        out.as_ref().expect("a selection"),
                        &expected,
                        "{source}/{kind} {w}×{h}"
                    );
                }
                match &out {
                    Ok(samples) => line
                        .answers
                        .extend(samples.iter().flat_map(|v| v.to_le_bytes())),
                    Err(e) => line.answers.extend(format!("error: {e}").bytes()),
                }
                line.print(&format!("selection/{source}/{kind}"), (w, h));
            }
        }
    }
}

/// The pixel-centre point rule of a nearest resize, written out.
fn nearest_oracle(
    source: &[[u16; 4]],
    (sw, sh): (u32, u32),
    (dw, dh): (u32, u32),
) -> Vec<[u16; 4]> {
    let mut out = Vec::new();
    for y in 0..dh {
        let sy = (2 * u64::from(y) + 1) * u64::from(sh) / (2 * u64::from(dh));
        for x in 0..dw {
            let sx = (2 * u64::from(x) + 1) * u64::from(sw) / (2 * u64::from(dw));
            out.push(source[sy as usize * sw as usize + sx as usize]);
        }
    }
    out
}

#[test]
fn record_geometry() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let doc = plates_document(w, h);
        let (ow, oh, ox, oy) = off_rect(w, h);
        let (x0, y0) = (f64::from(ox), f64::from(oy));
        let (x1, y1) = (x0 + f64::from(ow), y0 + f64::from(oh));
        let (rw, rh) = ((w * 3 / 4).max(1), (h * 2 / 3).max(1));
        let cases: [(&str, Value); 4] = [
            (
                "u16/affine_bilinear",
                json!({"op":"affine","layer":1,"target":"pixels",
                    "matrix":[0.9,0.2,-0.2,0.9,12.5,-7.25],"filter":"bilinear"}),
            ),
            (
                "u16/perspective",
                json!({"op":"perspective","layer":1,"target":"pixels",
                    "quad":[x0 + 0.5, y0 + 0.25, x1 - 0.75, y0, x1, y1 + 0.5, x0 - 0.25, y1],
                    "filter":"bilinear"}),
            ),
            (
                "u16/resize_bicubic",
                json!({"op":"resize","width":rw,"height":rh,"filter":"bicubic"}),
            ),
            (
                "u16/resize_nearest",
                json!({"op":"resize","width":rw,"height":rh,"filter":"nearest"}),
            ),
        ];
        for (label, request) in cases {
            let mut line = Line::default();
            evict();
            let outcome = line.add(recorded(|c| geometry(&doc, &request, Some(c))));
            if label == "u16/resize_nearest" && (rw, rh) != (w, h) {
                let out = outcome.as_ref().unwrap().as_ref().expect("a resize");
                let base = out.layer_source(0).unwrap();
                assert_eq!(base.dimensions(), (rw, rh));
                assert_eq!(
                    base.to_rgba16().unwrap(),
                    nearest_oracle(
                        &doc.layer_source(0).unwrap().to_rgba16().unwrap(),
                        (w, h),
                        (rw, rh)
                    ),
                    "nearest {w}×{h}"
                );
            }
            line.answers.extend(outcome_bytes(&outcome));
            line.print(&format!("geometry/{label}"), (w, h));
        }

        let samples = extended(w, h, 9);
        let float = float_document(w, h, &samples);
        for (label, request) in [
            ("f32/rotate90", json!({"op":"rotate90"})),
            ("f32/flip_horizontal", json!({"op":"flip_horizontal"})),
        ] {
            let mut line = Line::default();
            evict();
            let outcome = line.add(recorded(|c| geometry(&float, &request, Some(c))));
            let out = outcome
                .as_ref()
                .unwrap()
                .as_ref()
                .expect("a changed document");
            let image = out.layer_source(0).unwrap();
            let expected: Vec<[f32; 4]> = if label == "f32/rotate90" {
                // Clockwise: destination (x, y) of the h×w result reads
                // source (y, h − 1 − x).
                assert_eq!(image.dimensions(), (h, w));
                (0..w)
                    .flat_map(|y| (0..h).map(move |x| (x, y)))
                    .map(|(x, y)| samples[((h - 1 - x) * w + y) as usize])
                    .collect()
            } else {
                assert_eq!(image.dimensions(), (w, h));
                (0..h)
                    .flat_map(|y| (0..w).map(move |x| (x, y)))
                    .map(|(x, y)| samples[(y * w + (w - 1 - x)) as usize])
                    .collect()
            };
            assert_eq!(
                bits(&image.to_rgba32().unwrap()),
                bits(&expected),
                "{label} {w}×{h}"
            );
            line.answers.extend(outcome_bytes(&outcome));
            line.print(&format!("geometry/{label}"), (w, h));
        }
    }
}

#[test]
fn record_tone_map() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let doc = float_document(w, h, &extended(w, h, 11));
        let render = doc.render(LIMIT, 0, None).unwrap();

        let mut line = Line::default();
        evict();
        let display = line
            .add(recorded(|c| doc.display_rgba8(&render, LIMIT, 0, Some(c))))
            .expect("a display image");
        line.answers.extend(display.as_raw());
        line.print("tone_map/display_rgba8", (w, h));

        let mut line = Line::default();
        evict();
        let byte = line
            .add(recorded(|c| {
                doc.downconvert(PrecisionDither::None, LIMIT, 0, Some(c))
            }))
            .expect("a downconversion");
        line.answers.extend(byte.flattened().unwrap().as_raw());
        line.print("tone_map/downconvert", (w, h));

        let mut line = Line::default();
        evict();
        let wide = line
            .add(recorded(|c| doc.float_to_u16(LIMIT, 0, Some(c))))
            .expect("a float→u16 conversion");
        line.answers.extend(document_bytes(&wide));
        line.print("tone_map/float_to_u16", (w, h));

        let mut line = Line::default();
        evict();
        let byte = line
            .add(recorded(|c| {
                doc.narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, Some(c))
            }))
            .expect("a narrowing to 8 bits");
        line.answers.extend(byte.flattened().unwrap().as_raw());
        line.print("tone_map/narrow_structure_to_u8", (w, h));

        let mut line = Line::default();
        evict();
        let wide = line
            .add(recorded(|c| doc.narrow_structure_to_u16(LIMIT, 0, Some(c))))
            .expect("a narrowing to 16 bits");
        line.answers.extend(document_bytes(&wide));
        line.print("tone_map/narrow_structure_to_u16", (w, h));
    }
}

// ------------------------------------------------------------ no_view_* --

#[test]
fn no_view_retouch_plates() {
    let _serial = serial();
    let doc = plates_document(W, H);
    for (mode, name) in MODES {
        evict();
        assert_eq!(contiguous_view_bytes(), 0, "{name}: evicted");
        let plate = retouch_source(&doc, 1, mode, None).expect("a plate");
        assert_eq!(
            contiguous_view_bytes(),
            0,
            "{name}: a held plate holds no view"
        );
        let moved = transformed(&plate, [1., 0., 0., 1., 0.5, -0.25], None).expect("a transform");
        assert_eq!(
            contiguous_view_bytes(),
            0,
            "{name}: a held transformed plate holds no view"
        );
        drop((moved, plate));
    }
}

#[test]
fn no_view_retouch_clone_source_layer() {
    let _serial = serial();
    let doc = plates_document(W, H);
    let request = json!({"op":"retouch_clone","layer":0,"source_layer":1,"sample":"current",
        "matrix":[1,0,0,1,-150.5,200.25],"opacity":0.75});
    let coverage = boxed((W, H), (200, 200, 120, 120));
    evict();
    assert_eq!(document_views(&[&doc]), 0, "evicted");
    let out = edit(&doc, &request, &coverage, None)
        .expect("the clone runs")
        .expect("a changed document");
    assert_eq!(
        document_views(&[&doc]),
        0,
        "the in-place retouch reads one rect of the target (13B-3d-2)"
    );
    drop(out);
}

#[test]
fn no_view_layer_selection() {
    let _serial = serial();
    let doc = plates_document(W, H);
    for operation in [
        json!({"kind":"alpha"}),
        json!({"kind":"magic_wand","x":0,"y":450,"tolerance":3000}),
    ] {
        let request = json!({"op":"selection","source":"layer","layer":1,"operation":operation});
        evict();
        assert_eq!(document_views(&[&doc]), 0, "evicted");
        let out = select(&doc, &request, None).expect("a selection");
        assert_eq!(
            document_views(&[&doc]),
            0,
            "{operation}: a Layer-source selection binds no view over the document"
        );
        drop(out);
    }
}

#[test]
fn no_view_geometry_inputs() {
    let _serial = serial();
    let doc = plates_document(W, H);
    for request in [
        json!({"op":"affine","layer":1,"target":"pixels",
            "matrix":[0.9,0.2,-0.2,0.9,12.5,-7.25],"filter":"bilinear"}),
        json!({"op":"resize","width":450,"height":347,"filter":"bicubic"}),
    ] {
        evict();
        assert_eq!(document_views(&[&doc]), 0, "evicted");
        let out = geometry(&doc, &request, None)
            .expect("the edit runs")
            .expect("a changed document");
        assert_eq!(
            document_views(&[&doc]),
            0,
            "{request}: the resample input leaves no view"
        );
        drop(out);
    }
    let float = float_document(W, H, &extended(W, H, 9));
    evict();
    assert_eq!(document_views(&[&float]), 0, "evicted");
    let out = geometry(&float, &json!({"op":"rotate90"}), None)
        .expect("the turn runs")
        .expect("a changed document");
    assert_eq!(
        document_views(&[&float]),
        0,
        "the float quarter turn's input leaves no view"
    );
    drop(out);
}
