//! Black-box suite for 13B-3d-3b: the FILLED planes — `add_layer_at`'s
//! transparent leaf, the sourced structure shapes of the promotion plan and
//! the narrowing's masked-adjustment plane — are built over
//! `TilePlane::filled` (one tile per tile shape, no view); the resample
//! outputs are always unseeded (no output view since 13B-3e-4c-2a: the
//! geometry outputs, the placed source's Sample arm, `develop_detail` and
//! the export resize); and `precision_codec::encode` walks runs, so a save
//! / PNG16 / TIFF16 of a flattened document binds no view — with every
//! answer, checkpoint count, report log and refusal unchanged.
//!
//! Oracles are independent of the implementation: an added layer is all
//! zero (f32 `+0.0` bits) and its retained bytes are the distinct tile
//! shapes' bytes plus 8 bytes of index per tile, computed HERE from the
//! 256-pixel tile edge; its document's projection is the input's; a
//! regional window is the full projection's crop; an encoded PNG16 / TIFF16
//! decodes (through the `image` PNG decoder and the `tiff` crate) to the
//! encoded image's samples.
//!
//! `record_filled_outputs` prints one line per site, depth and fixture —
//! `filled <site>/<depth> <WxH> <sha256> checkpoints=<n> reports=<sha256>`
//! (the hash of `null`, `err:<message>` or the answer's bytes; a `.rz`
//! stream with its trailing `LIDS` ids zeroed, since ids are process-minted)
//! — so the same file run against HEAD and the tree diffs byte-identical
//! answers and checkpoint/report sequences. It uses only APIs present at
//! `e250ba6`.
//!
//! `no_view_filled_planes`, `filled_planes_share_one_tile_per_shape`,
//! `no_view_resample_outputs` and `no_view_encode` fail at `e250ba6` by
//! design; the other tests pass there. The narrowing seeded its BASE raster
//! (`precision_conversion_narrow.rs:233`) until 13B-3e-4b unseeded it, so
//! that half asserts 0 since then. The view cache is process-wide, so every
//! test holds `serial()`.

use std::ffi::{c_char, CStr, CString};
use std::io::Cursor;
use std::ptr;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportRequest;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_develop_inspect::dr_precision_image_develop_detail;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_regional::dr_precision_doc_render_region;
use doctored_core::ffi_retained_source::dr_precision_doc_source_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_codec::{self, PrecisionFormat};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::precision_quantize::PrecisionDither;
use doctored_core::retained_source::SourceEdit;
use doctored_core::tiff_float::FloatTiff;
use image::{ImageDecoder, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::sha256;
use crate::resource_report;
use crate::serial;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 600;
const H: u32 = 520;
/// 600×520 has four tile shapes, 512×512 four tiles of one shape, 257×3 two
/// shapes and 1×1 one.
const FIXTURES: [(u32, u32); 4] = [(W, H), (512, 512), (257, 3), (1, 1)];
/// The tile store's tile edge, restated.
const TILE: u32 = 256;

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

fn byte_pixels(w: u32, h: u32, seed: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            ((x * 13 + seed * 29) % 256) as u8,
            ((y * 17 + seed * 11) % 256) as u8,
            ((x + y + seed * 5) * 9 % 256) as u8,
            ((x * 23 + y * 41 + seed * 7) % 256) as u8,
        ])
    })
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

/// `resource_report::report` over byte documents and typed images.
fn report(bytes: &[&DrDocument], images: &[&PrecisionImage]) -> Value {
    let bytes: Vec<*const DrDocument> = bytes.iter().map(|d| *d as *const _).collect();
    let images: Vec<*const PrecisionImage> = images.iter().map(|i| *i as *const _).collect();
    resource_report::report(&bytes, &[], &[], &images, 0, 0).expect("a resource report")
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
            "filled {label} {w}x{h} {} checkpoints={} reports={}",
            sha256::sha256_hex(&self.answers),
            self.checkpoints,
            sha256::sha256_hex(self.log.as_bytes())
        );
    }
}

/// An image's dimensions and samples as bytes (u16 codes, f32 bits,
/// little-endian).
fn image_bytes(image: &PrecisionImage) -> Vec<u8> {
    let (w, h) = image.dimensions();
    let mut out: Vec<u8> = [w, h].iter().flat_map(|v| v.to_le_bytes()).collect();
    match image.storage() {
        PrecisionStorage::Rgba16Unorm => out.extend(
            image
                .to_rgba16()
                .unwrap()
                .iter()
                .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes())),
        ),
        PrecisionStorage::Rgba32Float => out.extend(
            image
                .to_rgba32()
                .unwrap()
                .iter()
                .flat_map(|p| p.iter().flat_map(|c| c.to_bits().to_le_bytes())),
        ),
    }
    out
}

/// Every record's offset and raster (dimensions and samples), in postorder.
fn document_bytes(doc: &PrecisionDocument) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..doc.layer_count() {
        let layer = doc.layer(i).unwrap();
        let (x, y) = layer.offset();
        out.extend(x.to_le_bytes());
        out.extend(y.to_le_bytes());
        if let Some(image) = layer.source() {
            out.extend(image_bytes(image));
        }
    }
    out
}

/// Every byte layer's offset, pixels and mask.
fn byte_document_bytes(doc: &DrDocument) -> Vec<u8> {
    let mut out = Vec::new();
    for layer in &doc.layers {
        out.extend(layer.offset.0.to_le_bytes());
        out.extend(layer.offset.1.to_le_bytes());
        let pixels = layer.pixels.to_contiguous().unwrap();
        out.extend(pixels.width().to_le_bytes());
        out.extend(pixels.height().to_le_bytes());
        out.extend(pixels.as_raw());
        if let Some(mask) = &layer.mask {
            out.extend(mask.width().to_le_bytes());
            out.extend(mask.height().to_le_bytes());
            out.extend(mask.as_raw());
        }
    }
    out
}

/// `null`, `err:<message>` or `bytes(answer)`.
fn outcome<T>(outcome: &Result<Option<T>, String>, bytes: impl Fn(&T) -> Vec<u8>) -> Vec<u8> {
    match outcome {
        Ok(Some(answer)) => bytes(answer),
        Ok(None) => b"null".to_vec(),
        Err(e) => format!("err:{e}").into_bytes(),
    }
}

/// A `.rz` stream with its trailing `LIDS` ids (the last `8·records` bytes)
/// zeroed: ids are minted by a process-wide counter.
fn native_bytes(stream: &[u8], records: usize) -> Vec<u8> {
    let mut out = stream.to_vec();
    let start = out.len().saturating_sub(8 * records);
    out[start..].fill(0);
    out
}

/// The distinct tile shapes of a `w`×`h` plane (256-pixel tiles, the last
/// column / row clipped) and its tile count.
fn tile_shapes((w, h): (u32, u32)) -> (Vec<(u32, u32)>, u64) {
    let (columns, rows) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let mut shapes = Vec::new();
    for ty in 0..rows {
        for tx in 0..columns {
            let shape = ((w - tx * TILE).min(TILE), (h - ty * TILE).min(TILE));
            if !shapes.contains(&shape) {
                shapes.push(shape);
            }
        }
    }
    (shapes, u64::from(columns) * u64::from(rows))
}

/// A filled plane's sample bytes by hand: one tile per shape.
fn filled_bytes(size: (u32, u32), bytes_per_pixel: u64) -> u64 {
    tile_shapes(size)
        .0
        .iter()
        .map(|&(w, h)| u64::from(w) * u64::from(h) * bytes_per_pixel)
        .sum()
}

fn category(report: &Value, name: &str) -> u64 {
    report["categories"][name]["unique_bytes"]
        .as_u64()
        .unwrap_or(0)
}

// ------------------------------------------------------------- fixtures --

/// Two u16 rasters: an opaque canvas-sized base and the "Off" layer
/// (`w/2`×`5h/13` at `(−w/6, 10h/13)`, opacity 0.8), partly off the canvas.
fn two_layer_document(w: u32, h: u32) -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let (ow, oh, ox, oy) = off_rect(w, h);
    let base: Vec<[u16; 4]> = pattern16(w, h, 1)
        .into_iter()
        .map(|[r, g, b, _]| [r, g, b, u16::MAX])
        .collect();
    let image = |w: u32, h: u32, pixels: Vec<[u16; 4]>| {
        PrecisionImage::from_rgba16(w, h, pixels, profile.clone(), None).unwrap()
    };
    let layers = vec![
        Layer::raster("Base", image(w, h, base), (0, 0), 1.0, true).unwrap(),
        Layer::raster(
            "Off",
            image(ow, oh, pattern16(ow, oh, 2)),
            (ox, oy),
            0.8,
            true,
        )
        .unwrap(),
    ];
    PrecisionDocument::from_parts(
        w,
        h,
        profile.clone(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn off_rect(w: u32, h: u32) -> (u32, u32, i32, i32) {
    (
        (w / 2).max(1),
        (h * 5 / 13).max(1),
        -((w / 6) as i32),
        (h * 10 / 13) as i32,
    )
}

/// A float document holding one `width`×`height` raster of `samples`,
/// through a float TIFF opened with `open_preserving`.
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
    let path = directory.path().join("filled.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// A byte document whose top layer carries a placed retained source
/// (`precision_conversion_structure.rs`' `placed()` pattern).
fn placed_byte_document(w: u32, h: u32) -> DrDocument {
    let mut doc = DrDocument::from_pixels(byte_pixels(w, h, 1));
    doc.layers[0].name = "Background".into();
    let master = DrDocument::from_pixels(byte_pixels((w / 2).max(1), (h / 2).max(1), 11));
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"13b3d3b-master","revision_id":"v1",
                "name":"Retained","x":w / 4,"y":h / 4})
        .to_string(),
    )
    .unwrap();
    doc.source_edit(Some(&master), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

/// A u16 document holding exactly one canvas-sized raster at (0, 0) and one
/// `precision_v1` invert adjustment with a canvas-sized mask.
fn masked_adjustment_document(w: u32, h: u32) -> PrecisionDocument {
    let samples: Vec<[u16; 4]> = pattern16(w, h, 7)
        .into_iter()
        .map(|[r, g, b, _]| [r, g, b, u16::MAX])
        .collect();
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster(
            "Base",
            PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None).unwrap(),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()],
    )
    .unwrap();
    let doc = doc
        .add_adjustment(
            "Invert",
            &json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"})
                .to_string(),
            1.0,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let mask = MaskEdit::parse(
        &json!({"op":"set_mask","layer":1,"width":w,"height":h,"x":0,"y":0}).to_string(),
    )
    .unwrap();
    let values: Vec<u16> = (0..w * h)
        .map(|i| if i % w < w / 2 { u16::MAX } else { 10280 })
        .collect();
    doc.mask_edit(&mask, &values, LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

/// A window across both seams at 600×520, contained in every fixture.
fn seam_window(w: u32, h: u32) -> (u32, u32, u32, u32) {
    (w / 6, h / 5, (w * 3 / 4).max(1), (h * 4 / 5).max(1))
}

// ---------------------------------------------------------------- sites --

fn add_layer(doc: &PrecisionDocument) -> Result<Option<PrecisionDocument>, String> {
    doc.add_layer_at("Empty", 0, InsertPosition::Top)
}

fn geometry(
    doc: &PrecisionDocument,
    request: &Value,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let parsed = GeometryEdit::parse(&request.to_string()).expect("a geometry request");
    doc.geometry(&parsed, LIMIT, 0, control)
}

/// The u16 geometry requests over [`two_layer_document`].
fn geometry_requests(w: u32, h: u32) -> [(&'static str, Value); 3] {
    let (ow, oh, ox, oy) = off_rect(w, h);
    let (x0, y0) = (f64::from(ox), f64::from(oy));
    let (x1, y1) = (x0 + f64::from(ow), y0 + f64::from(oh));
    let (rw, rh) = ((w * 3 / 4).max(1), (h * 2 / 3).max(1));
    [
        (
            "geometry_affine",
            json!({"op":"affine","layer":1,"target":"pixels",
                "matrix":[0.9,0.2,-0.2,0.9,12.5,-7.25],"filter":"bilinear"}),
        ),
        (
            "geometry_resize",
            json!({"op":"resize","width":rw,"height":rh,"filter":"bicubic"}),
        ),
        (
            "geometry_perspective",
            json!({"op":"perspective","layer":1,"target":"pixels",
                "quad":[x0 + 0.5, y0 + 0.25, x1 - 0.75, y0, x1, y1 + 0.5, x0 - 0.25, y1],
                "filter":"bilinear"}),
        ),
    ]
}

/// `dr_precision_doc_source_edit` of `request` on `doc` with `input`.
fn source_edit(
    doc: &PrecisionDocument,
    input: Option<&PrecisionDocument>,
    request: &Value,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_source_edit(
            doc,
            input.map_or(ptr::null(), |d| d),
            text.as_ptr(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if !error.is_null() {
        return Err(take(error));
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn place_request() -> Value {
    json!({"op":"place","source_id":"13b3d3b-typed","name":"Placed","x":0,"y":0})
}

/// A non-shared affine: the retained render's Sample arm.
fn source_transform_request() -> Value {
    json!({"op":"transform","layer":2,"matrix":[0.9,0.2,-0.2,0.9,12.5,-7.25],
        "filter":"bilinear"})
}

/// `dr_precision_image_develop_detail` over `region`.
fn develop_detail(
    image: &PrecisionImage,
    (x, y, w, h): (u32, u32, u32, u32),
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionImage>, String> {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_image_develop_detail(
            image,
            x,
            y,
            w,
            h,
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if !error.is_null() {
        return Err(take(error));
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn edit(
    doc: &PrecisionDocument,
    request: &Value,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if !error.is_null() {
        return Err(take(error));
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn flattened(doc: &PrecisionDocument) -> PrecisionDocument {
    edit(doc, &json!({"op":"flatten"}), None)
        .expect("the flatten runs")
        .expect("a flattened document")
}

/// The 16-bit export requests: PNG / TIFF, plain, resized, sharpened and
/// both.
fn export_requests(w: u32) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let half = (w / 2).max(1);
    for format in ["png", "tiff"] {
        for (variant, extra) in [
            ("", String::new()),
            (
                "_resize",
                format!(r#","size":{{"mode":"width","value":{half}}}"#),
            ),
            (
                "_sharpen",
                r#","sharpen":{"radius":1.4,"amount":1.75,"threshold":0.0}"#.to_string(),
            ),
            (
                "_resize_sharpen",
                format!(
                    r#","size":{{"mode":"width","value":{half}}},"sharpen":{{"radius":1.4,"amount":1.75,"threshold":0.0}}"#
                ),
            ),
        ] {
            out.push((
                format!("export_{format}16{variant}"),
                format!(r#"{{"format":"{format}","bit_depth":16{extra}}}"#),
            ));
        }
    }
    out
}

fn export(
    doc: &PrecisionDocument,
    projection: &PrecisionImage,
    request: &str,
    control: Option<&DrJobControl>,
) -> Result<Option<Vec<u8>>, String> {
    let request = ExportRequest::parse(request)?;
    doc.export_encoded(Some(projection), &request, LIMIT, 0, control)
        .map(|result| Some(result.bytes().to_vec()))
}

/// `precision_codec::save` of `image` to a fresh temporary file, answering
/// the file's bytes; the path in a refusal is replaced by `<path>`.
fn codec_save(
    image: &PrecisionImage,
    format: PrecisionFormat,
    control: Option<&DrJobControl>,
) -> Result<Option<Vec<u8>>, String> {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join(match format {
        PrecisionFormat::Png => "filled.png",
        PrecisionFormat::Tiff => "filled.tiff",
    });
    let text = path.to_str().unwrap();
    precision_codec::save(
        image,
        &Metadata::default(),
        Resolution::default(),
        text,
        format,
        true,
        false,
        LIMIT,
        0,
        control,
    )
    .map_err(|e| e.replace(text, "<path>"))?;
    Ok(Some(std::fs::read(&path).unwrap()))
}

/// The inputs of one fixture.
struct Inputs {
    size: (u32, u32),
    u16_doc: PrecisionDocument,
    f32_doc: PrecisionDocument,
    placed_byte: DrDocument,
    masked: PrecisionDocument,
    projection: PrecisionImage,
    flat: PrecisionDocument,
    float_flat: PrecisionDocument,
}

impl Inputs {
    fn new((w, h): (u32, u32)) -> Self {
        let u16_doc = two_layer_document(w, h);
        let projection = u16_doc.render(LIMIT, 0, None).expect("the projection");
        let flat = flattened(&u16_doc);
        let f32_doc = float_document(w, h, &extended(w, h, 5));
        let float_flat = flattened(&f32_doc);
        Self {
            size: (w, h),
            u16_doc,
            f32_doc,
            placed_byte: placed_byte_document(w, h),
            masked: masked_adjustment_document(w, h),
            projection,
            flat,
            float_flat,
        }
    }
}

/// Every record line of one fixture, each job under its own recording
/// control.
fn record_fixture(inputs: &Inputs) {
    let size = inputs.size;
    let (w, h) = size;
    let print = |label: &str, line: &Line| line.print(label, size);

    for (depth, doc) in [("u16", &inputs.u16_doc), ("f32", &inputs.f32_doc)] {
        let mut line = Line::default();
        evict();
        let out = line.add(recorded(|_| add_layer(doc)));
        line.answers.extend(outcome(&out, document_bytes));
        print(&format!("add_layer/{depth}"), &line);
    }

    let mut line = Line::default();
    evict();
    let plan = line.add(recorded(|_| {
        inputs
            .placed_byte
            .promote_structure_plan()
            .map(|plan| Some(without_ids(plan.json()).to_string().into_bytes()))
    }));
    line.answers.extend(outcome(&plan, |bytes| bytes.clone()));
    print("promote_plan/u8", &line);

    let mut line = Line::default();
    evict();
    let narrowed = line.add(recorded(|c| {
        inputs
            .masked
            .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, Some(c))
            .map(Some)
    }));
    line.answers.extend(outcome(&narrowed, byte_document_bytes));
    print("narrow_masked_adjustment/u16", &line);

    for (label, request) in geometry_requests(w, h) {
        let mut line = Line::default();
        evict();
        let out = line.add(recorded(|c| geometry(&inputs.u16_doc, &request, Some(c))));
        line.answers.extend(outcome(&out, document_bytes));
        print(&format!("{label}/u16"), &line);
    }
    for (label, request) in [
        ("geometry_rotate90", json!({"op":"rotate90"})),
        ("geometry_flip", json!({"op":"flip_horizontal"})),
    ] {
        let mut line = Line::default();
        evict();
        let out = line.add(recorded(|c| geometry(&inputs.f32_doc, &request, Some(c))));
        line.answers.extend(outcome(&out, document_bytes));
        print(&format!("{label}/f32"), &line);
    }

    let mut line = Line::default();
    evict();
    let placed = line.add(recorded(|c| {
        source_edit(
            &inputs.u16_doc,
            Some(&inputs.flat),
            &place_request(),
            Some(c),
        )
    }));
    let transformed = match &placed {
        Ok(Some(doc)) => line.add(recorded(|c| {
            source_edit(doc, None, &source_transform_request(), Some(c))
        })),
        Ok(None) => Ok(None),
        Err(e) => Err(e.clone()),
    };
    line.answers.extend(outcome(&placed, document_bytes));
    line.answers.extend(outcome(&transformed, document_bytes));
    print("placed_source_affine/u16", &line);

    let mut line = Line::default();
    evict();
    let source = inputs.u16_doc.layer_source(0).unwrap();
    let detail = line.add(recorded(|c| {
        develop_detail(source, seam_window(w, h), Some(c))
    }));
    line.answers.extend(outcome(&detail, image_bytes));
    print("develop_detail/u16", &line);

    for (label, request) in export_requests(w) {
        let mut line = Line::default();
        evict();
        let out = line.add(recorded(|c| {
            export(&inputs.u16_doc, &inputs.projection, &request, Some(c))
        }));
        line.answers.extend(outcome(&out, |bytes| bytes.clone()));
        print(&format!("{label}/u16"), &line);
    }

    let mut line = Line::default();
    evict();
    let stream = line.add(recorded(|c| {
        inputs.flat.encode_native(LIMIT, 0, Some(c)).map(Some)
    }));
    let records = inputs.flat.layer_count();
    line.answers
        .extend(outcome(&stream, |bytes| native_bytes(bytes, records)));
    print("encode_native_after_flatten/u16", &line);

    let flat_image = inputs.flat.layer_source(0).unwrap();
    for (label, format) in [
        ("codec_save_png", PrecisionFormat::Png),
        ("codec_save_tiff", PrecisionFormat::Tiff),
    ] {
        let mut line = Line::default();
        evict();
        let out = line.add(recorded(|c| codec_save(flat_image, format, Some(c))));
        line.answers.extend(outcome(&out, |bytes| bytes.clone()));
        print(&format!("{label}/u16"), &line);
    }

    let mut line = Line::default();
    evict();
    let float_image = inputs.float_flat.layer_source(0).unwrap();
    let out = line.add(recorded(|c| {
        codec_save(float_image, PrecisionFormat::Png, Some(c))
    }));
    line.answers.extend(outcome(&out, |bytes| bytes.clone()));
    print("codec_encode_refusal/f32", &line);
}

/// `value` with every `"id"` key removed (ids are process-minted).
fn without_ids(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(key, _)| key != "id")
                .map(|(key, value)| (key, without_ids(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(without_ids).collect()),
        other => other,
    }
}

/// `dr_precision_doc_render_region` at divisor 1.
fn render_region(doc: &PrecisionDocument, (x, y, w, h): (u32, u32, u32, u32)) -> PrecisionImage {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_render_region(doc, x, y, w, h, 1, LIMIT, 0, ptr::null(), &mut error)
    };
    assert!(error.is_null() && !out.is_null(), "{}", take(error));
    *unsafe { Box::from_raw(out) }
}

/// The samples of `(x, y, w, h)` of `image` as [`image_bytes`] would
/// answer a `w`×`h` image of them.
fn crop_bytes(image: &PrecisionImage, (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u8> {
    let width = image.dimensions().0 as usize;
    let rows = |row: usize| (row * width + x as usize)..(row * width + (x + w) as usize);
    let mut out: Vec<u8> = [w, h].iter().flat_map(|v| v.to_le_bytes()).collect();
    match image.storage() {
        PrecisionStorage::Rgba16Unorm => {
            let samples = image.to_rgba16().unwrap();
            for row in y as usize..(y + h) as usize {
                out.extend(
                    samples[rows(row)]
                        .iter()
                        .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes())),
                );
            }
        }
        PrecisionStorage::Rgba32Float => {
            let samples = image.to_rgba32().unwrap();
            for row in y as usize..(y + h) as usize {
                out.extend(
                    samples[rows(row)]
                        .iter()
                        .flat_map(|p| p.iter().flat_map(|c| c.to_bits().to_le_bytes())),
                );
            }
        }
    }
    out
}

/// Decodes a PNG16 / TIFF16 stream to its u16 samples, independently of
/// the encoder (the `image` PNG decoder and the `tiff` crate).
fn decoded16(bytes: &[u8], format: PrecisionFormat) -> Vec<u16> {
    match format {
        PrecisionFormat::Png => {
            let decoder = image::codecs::png::PngDecoder::new(Cursor::new(bytes)).unwrap();
            assert_eq!(decoder.color_type(), image::ColorType::Rgba16);
            let mut raw = vec![0; decoder.total_bytes() as usize];
            decoder.read_image(&mut raw).unwrap();
            raw.chunks_exact(2)
                .map(|p| u16::from_ne_bytes([p[0], p[1]]))
                .collect()
        }
        PrecisionFormat::Tiff => {
            let mut decoder = tiff::decoder::Decoder::new(Cursor::new(bytes.to_vec())).unwrap();
            assert_eq!(decoder.colortype().unwrap(), tiff::ColorType::RGBA(16));
            match decoder.read_image().unwrap() {
                tiff::decoder::DecodingResult::U16(samples) => samples,
                _ => panic!("a 16-bit TIFF"),
            }
        }
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn record_filled_outputs() {
    let _serial = serial();
    for size in FIXTURES {
        record_fixture(&Inputs::new(size));
    }
}

#[test]
fn no_view_filled_planes() {
    let _serial = serial();
    let u16_doc = two_layer_document(W, H);
    let f32_doc = float_document(W, H, &extended(W, H, 5));
    for (label, doc) in [("u16", &u16_doc), ("f32", &f32_doc)] {
        evict();
        let out = add_layer(doc)
            .expect("an added layer")
            .expect("a changed document");
        assert_eq!(
            contiguous_view_bytes(),
            0,
            "{label}: the held added layer holds no view (13B-3d-3b)"
        );
        drop(out);
    }
    let masked = masked_adjustment_document(W, H);
    evict();
    let narrowed = masked
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .expect("the narrowing runs");
    assert_eq!(narrowed.layers.len(), 2);
    assert_eq!(narrowed.layers[1].pixels.dimensions(), (W, H));
    assert_eq!(
        contiguous_view_bytes(),
        0,
        "nothing from the base raster (unseeded since 13B-3e-4b) or the masked \
         adjustment's canvas plane (13B-3d-3b)"
    );
    drop(narrowed);
}

#[test]
fn filled_planes_share_one_tile_per_shape() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let u16_doc = two_layer_document(w, h);
        let f32_doc = float_document(w, h, &extended(w, h, 5));
        for (label, doc, bytes) in [("u16", &u16_doc, 8), ("f32", &f32_doc, 16)] {
            let out = add_layer(doc)
                .expect("an added layer")
                .expect("a changed document");
            let top = out.layer_count() - 1;
            assert_eq!(out.layer(top).unwrap().offset(), (0, 0));
            let image = out.layer_source(top).expect("a raster");
            assert_eq!(image.dimensions(), (w, h));
            // `retained_bytes()` also counts the (shared) profile, which the
            // report lists apart. Its term comes from a SEPARATE 1×1 probe
            // on the sRGB profile, not from the report under test; the ICC
            // bytes alone undercount it (the parsed profile, its name and
            // its sampled TRC tables count too).
            let probe =
                PrecisionImage::from_rgba16(1, 1, vec![[0; 4]], IccProfile::srgb(), None).unwrap();
            let profile = category(&report(&[], &[&probe]), "profile");
            assert!(
                profile > IccProfile::srgb().bytes().len() as u64,
                "the retained profile exceeds its ICC bytes"
            );
            let measured = report(&[], &[image]);
            let (pixels, index) = (filled_bytes((w, h), bytes), 8 * tile_shapes((w, h)).1);
            assert_eq!(
                category(&measured, "precision_pixels"),
                pixels,
                "{label} {w}×{h}: one tile per shape"
            );
            assert_eq!(
                category(&measured, "tile_structure"),
                index,
                "{label} {w}×{h}: 8 bytes of index per tile"
            );
            assert_eq!(
                image.retained_bytes(),
                pixels + index + profile,
                "{label} {w}×{h}: the plane plus the profile, nothing else"
            );
            match image.storage() {
                PrecisionStorage::Rgba16Unorm => {
                    assert!(image.to_rgba16().unwrap().iter().all(|p| *p == [0; 4]));
                }
                PrecisionStorage::Rgba32Float => {
                    assert!(image
                        .to_rgba32()
                        .unwrap()
                        .iter()
                        .all(|p| p.iter().all(|c| c.to_bits() == 0)));
                }
            }
        }
    }
    let masked = masked_adjustment_document(W, H);
    let narrowed = masked
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .expect("the narrowing runs");
    assert_eq!(
        category(&report(&[&narrowed], &[]), "layer_pixels"),
        u64::from(W) * u64::from(H) * 4 + filled_bytes((W, H), 4),
        "the base raster's tiles plus one tile per shape of the adjustment's plane"
    );
    assert!(narrowed.layers[1]
        .pixels
        .to_contiguous()
        .unwrap()
        .as_raw()
        .iter()
        .all(|v| *v == 0));
}

#[test]
fn no_view_resample_outputs() {
    let _serial = serial();
    let doc = two_layer_document(W, H);
    for (label, request) in geometry_requests(W, H) {
        evict();
        let out = geometry(&doc, &request, None)
            .expect("the edit runs")
            .expect("a changed document");
        assert_eq!(contiguous_view_bytes(), 0, "{label}: no view held");
        drop(out);
    }
    let float = float_document(W, H, &extended(W, H, 9));
    for request in [json!({"op":"rotate90"}), json!({"op":"flip_horizontal"})] {
        evict();
        let out = geometry(&float, &request, None)
            .expect("the turn runs")
            .expect("a changed document");
        assert_eq!(contiguous_view_bytes(), 0, "{request}: no view held");
        drop(out);
    }
    let master = flattened(&doc);
    let placed = source_edit(&doc, Some(&master), &place_request(), None)
        .expect("the place runs")
        .expect("a placed document");
    evict();
    let out = source_edit(&placed, None, &source_transform_request(), None)
        .expect("the transform runs")
        .expect("a transformed document");
    assert_eq!(
        contiguous_view_bytes(),
        0,
        "the placed source's Sample arm holds no view"
    );
    drop(out);
    evict();
    let detail = develop_detail(doc.layer_source(0).unwrap(), seam_window(W, H), None)
        .expect("the detail runs")
        .expect("a detail image");
    assert_eq!(contiguous_view_bytes(), 0, "develop_detail holds no view");
    drop(detail);
}

#[test]
fn no_view_encode() {
    let _serial = serial();
    let flat = flattened(&two_layer_document(W, H));
    let image = flat.layer_source(0).unwrap();
    evict();
    let stream = flat.encode_native(LIMIT, 0, None).expect("the native save");
    assert_eq!(contiguous_view_bytes(), 0, "encode_native binds no view");
    drop(stream);
    for format in [PrecisionFormat::Png, PrecisionFormat::Tiff] {
        evict();
        codec_save(image, format, None).expect("the save runs");
        assert_eq!(contiguous_view_bytes(), 0, "{format:?}: save binds no view");
    }
    for request in [
        r#"{"format":"png","bit_depth":16}"#,
        r#"{"format":"tiff","bit_depth":16}"#,
    ] {
        evict();
        export(&flat, image, request, None).expect("the export runs");
        assert_eq!(
            contiguous_view_bytes(),
            0,
            "{request}: export binds no view"
        );
    }
}

#[test]
fn filled_layer_renders_and_windows_equal_the_input() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let u16_doc = two_layer_document(w, h);
        let f32_doc = float_document(w, h, &extended(w, h, 5));
        for (label, doc) in [("u16", &u16_doc), ("f32", &f32_doc)] {
            let out = add_layer(doc)
                .expect("an added layer")
                .expect("a changed document");
            let before = doc.render(LIMIT, 0, None).expect("the input's projection");
            let after = out.render(LIMIT, 0, None).expect("the projection");
            assert!(
                after.same_samples(&before),
                "{label} {w}×{h}: an empty layer changes no projected sample"
            );
            let edge = ((w * 5 / 6).min(w - 1), (h * 5 / 6).min(h - 1));
            for window in [
                (edge.0, edge.1, w - edge.0, h - edge.1),
                seam_window(w, h),
                (w / 2, h / 2, 1, 1),
            ] {
                assert_eq!(
                    image_bytes(&render_region(&out, window)),
                    crop_bytes(&after, window),
                    "{label} {w}×{h} window {window:?}"
                );
            }
        }
    }
}

#[test]
fn encoded_samples_decode_exactly() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let flat = flattened(&two_layer_document(w, h));
        let image = flat.layer_source(0).unwrap();
        let expected: Vec<u16> = image.to_rgba16().unwrap().into_iter().flatten().collect();
        for format in [PrecisionFormat::Png, PrecisionFormat::Tiff] {
            let bytes = codec_save(image, format, None)
                .expect("the save runs")
                .expect("the file");
            assert_eq!(
                decoded16(&bytes, format),
                expected,
                "{format:?} {w}×{h}: the saved samples"
            );
            let request = match format {
                PrecisionFormat::Png => r#"{"format":"png","bit_depth":16}"#,
                PrecisionFormat::Tiff => r#"{"format":"tiff","bit_depth":16}"#,
            };
            let bytes = export(&flat, image, request, None)
                .expect("the export runs")
                .expect("the bytes");
            assert_eq!(
                decoded16(&bytes, format),
                expected,
                "{format:?} {w}×{h}: the exported samples"
            );
        }
    }
}
