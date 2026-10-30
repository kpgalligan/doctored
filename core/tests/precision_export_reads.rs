//! Black-box suite for 13B-3e-1: the typed export chain, the 16-bit PSD
//! writer and the float TIFF / float native writers read tiles (runs, rows,
//! one rect) instead of binding a whole-plane view, and every export render,
//! export constructor and the style bake stop seeding the view cache — with
//! every output byte, refusal, checkpoint count and report log unchanged.
//!
//! Oracles are independent of the implementation: a trimmed layer export is
//! the canvas export cropped at the alpha bounding box computed HERE (both
//! decoded through the `image` PNG decoder); a PSD's records, read back by
//! the independent `psd16_fixture` reader, hold the projection's and the
//! base layer's samples; a float TIFF export reopens to the projection's f32
//! bits and a float native stream reopens to its source layers' samples.
//!
//! `record_export_reads` prints one line per site and fixture —
//! `export <site> <WxH> <sha256> checkpoints=<n> reports=<sha256>` (the hash
//! of `null`, `err:<message>` or the answer's bytes; a `.rz` stream with its
//! trailing `LIDS` ids zeroed, since ids are process-minted; checkpoints and
//! reports are recorded where the API takes a control, else 0 / empty) — so
//! the same file run against HEAD and the tree diffs byte-identical answers
//! and checkpoint/report sequences. It uses only APIs present at `2c04c47`.
//!
//! Re-baseline: the trimmed typed layer export's tile-band scan polls
//! the export's job once per 256-row band, so for a fixture of canvas height
//! h, with B = h.div_ceil(256) (600×520 → 3, 512×512 → 2, 257×3 → 1,
//! 1×1 → 1), `layer/png16_trimmed`, `layer/tiff16_p3_trimmed` and
//! `layer/empty_trimmed` record `checkpoints` = before + B. The `reports=`
//! hash changes for `png16_trimmed` and `tiff16_p3_trimmed` only — every
//! report after the scan (the codec's `report(0.1, 1)` onward) carries
//! checkpoints + B, with progress bits and stages unchanged — and
//! `empty_trimmed`'s is unchanged, because its refusal follows the scan.
//! Every other row and every output hash is byte-identical. A baseline taken
//! before the scan polled is re-baselined by adding B, never by re-measuring.
//!
//! `no_view_export_reads` fails at `2c04c47` by design; the other tests pass
//! there. The view cache is process-wide, so every test holds `serial()`.

use std::ffi::{c_char, CStr, CString};
use std::io::Cursor;
use std::ptr;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::doc_export::ExportRequest;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_export16;
use doctored_core::ffi_psd::dr_precision_doc_save_psd;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use doctored_core::tiff_float::FloatTiff;
use image::ImageDecoder;
use serde_json::Value;

use crate::common::sha256;
use crate::serial;

use crate::psd16_fixture as fixture;
use crate::resource_report;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 600;
const H: u32 = 520;
/// 600×520 has four tile shapes, 512×512 four tiles of one shape, 257×3 two
/// shapes and 1×1 one.
const FIXTURES: [(u32, u32); 4] = [(W, H), (512, 512), (257, 3), (1, 1)];
const SHARPEN: &str = r#""sharpen":{"radius":1.4,"amount":1.75,"threshold":0.0}"#;
const STROKE: &str = r#"{"effects":[{"type":"stroke","size":0.5}]}"#;

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

/// `null`, `err:<message>` or the answer's bytes.
fn outcome(outcome: &Result<Option<Vec<u8>>, String>) -> Vec<u8> {
    match outcome {
        Ok(Some(answer)) => answer.clone(),
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

// ------------------------------------------------------------- fixtures --

/// Two u16 rasters: an opaque canvas-sized base and the "Off" layer
/// (`w/2`×`5h/13` at `(−w/6, 10h/13)`, opacity 0.8), partly off the canvas;
/// with `style`, the Off layer carries it.
fn two_layer_document(w: u32, h: u32, style: Option<&str>) -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let (ow, oh, ox, oy) = off_rect(w, h);
    let base: Vec<[u16; 4]> = pattern16(w, h, 1)
        .into_iter()
        .map(|[r, g, b, _]| [r, g, b, u16::MAX])
        .collect();
    let image = |w: u32, h: u32, pixels: Vec<[u16; 4]>| {
        PrecisionImage::from_rgba16(w, h, pixels, profile.clone(), None).unwrap()
    };
    let mut off = Layer::raster(
        "Off",
        image(ow, oh, pattern16(ow, oh, 2)),
        (ox, oy),
        0.8,
        true,
    )
    .unwrap();
    if let Some(style) = style {
        let style = LayerStyle::from_json(style).unwrap();
        off = off.with_layer_style(Some(&style), (w, h)).unwrap();
    }
    let layers = vec![
        Layer::raster("Base", image(w, h, base), (0, 0), 1.0, true).unwrap(),
        off,
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
    let path = directory.path().join("export_reads.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// The inputs of one fixture.
struct Inputs {
    size: (u32, u32),
    u16_doc: PrecisionDocument,
    projection: PrecisionImage,
    styled: PrecisionDocument,
    styled_projection: PrecisionImage,
    empty_top: PrecisionDocument,
    f32_doc: PrecisionDocument,
    f32_projection: PrecisionImage,
}

impl Inputs {
    fn new((w, h): (u32, u32)) -> Self {
        let u16_doc = two_layer_document(w, h, None);
        let projection = u16_doc.render(LIMIT, 0, None).expect("the projection");
        let styled = two_layer_document(w, h, Some(STROKE));
        let styled_projection = styled
            .render(LIMIT, 0, None)
            .expect("the styled projection");
        let empty_top = u16_doc
            .add_layer_at("Empty", 0, InsertPosition::Top)
            .expect("an added layer")
            .expect("a changed document");
        let f32_doc = float_document(w, h, &extended(w, h, 5));
        let f32_projection = f32_doc
            .render(LIMIT, 0, None)
            .expect("the float projection");
        Self {
            size: (w, h),
            u16_doc,
            projection,
            styled,
            styled_projection,
            empty_top,
            f32_doc,
            f32_projection,
        }
    }
}

// ---------------------------------------------------------------- sites --

/// Every site, in record order, and whether its API takes a control.
const SITES: [(&str, bool); 18] = [
    ("encoded/png16_sharpen", true),
    ("encoded/tiff16_p3", true),
    ("encoded/png16_half_sharpen_p3", true),
    ("encoded/png8_p3", true),
    ("encoded/tiff16_render_sharpen_p3", true),
    ("layer/png16_trimmed", true),
    ("layer/tiff16_p3_trimmed", true),
    ("layer/png16_canvas", true),
    ("layer/empty_trimmed", true),
    ("export16/png", false),
    ("export16/tiff", false),
    ("psd/flattened", false),
    ("psd/layers_baked", false),
    ("float/tiff32", true),
    ("float/tiff32_render", true),
    ("float/png16", true),
    ("float/png16_half_sharpen", true),
    ("float/native", true),
];

fn request(json: &str) -> Result<ExportRequest, String> {
    ExportRequest::parse(json)
}

fn encoded(
    doc: &PrecisionDocument,
    projection: Option<&PrecisionImage>,
    json: &str,
    control: Option<&DrJobControl>,
) -> Result<Option<Vec<u8>>, String> {
    doc.export_encoded(projection, &request(json)?, LIMIT, 0, control)
        .map(|result| Some(result.bytes().to_vec()))
}

fn layer(
    doc: &PrecisionDocument,
    idx: usize,
    trimmed: bool,
    json: &str,
    control: Option<&DrJobControl>,
) -> Result<Option<Vec<u8>>, String> {
    doc.export_layer_encoded(idx, trimmed, &request(json)?, LIMIT, 0, control)
        .map(|result| Some(result.bytes().to_vec()))
}

/// Runs `write` against a fresh temporary path and answers the written
/// file's bytes; the path in a refusal is replaced by `<path>`.
fn written(
    name: &str,
    write: impl FnOnce(&CStr) -> Result<(), String>,
) -> Result<Option<Vec<u8>>, String> {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join(name);
    let text = path.to_str().unwrap().to_string();
    let c_path = CString::new(text.clone()).unwrap();
    write(&c_path).map_err(|e| e.replace(&text, "<path>"))?;
    Ok(Some(std::fs::read(&path).unwrap()))
}

fn export16(doc: &PrecisionDocument, format: u32) -> Result<Option<Vec<u8>>, String> {
    let name = if format == 0 {
        "export.png"
    } else {
        "export.tiff"
    };
    written(name, |path| {
        let mut error = ptr::null_mut();
        let mut carried = 0_u32;
        let ok = unsafe {
            dr_precision_doc_export16(
                doc,
                path.as_ptr(),
                format,
                true,
                false,
                LIMIT,
                0,
                &mut carried,
                &mut error,
            )
        };
        if ok {
            Ok(())
        } else {
            Err(take(error))
        }
    })
}

fn save_psd(
    doc: &PrecisionDocument,
    projection: &PrecisionImage,
    mode: i32,
) -> Result<Option<Vec<u8>>, String> {
    written("export.psd", |path| {
        let mut error = ptr::null_mut();
        let ok = unsafe {
            dr_precision_doc_save_psd(
                doc,
                projection,
                path.as_ptr(),
                0,
                mode,
                LIMIT,
                0,
                &mut error,
            )
        };
        if ok {
            Ok(())
        } else {
            Err(take(error))
        }
    })
}

/// One site's answer.
fn site(
    inputs: &Inputs,
    name: &str,
    control: Option<&DrJobControl>,
) -> Result<Option<Vec<u8>>, String> {
    let half = (inputs.size.0 / 2).max(1);
    let size = format!(r#""size":{{"mode":"width","value":{half}}}"#);
    let (doc, projection) = (&inputs.u16_doc, &inputs.projection);
    let (float, float_projection) = (&inputs.f32_doc, &inputs.f32_projection);
    match name {
        "encoded/png16_sharpen" => encoded(
            doc,
            Some(projection),
            &format!(r#"{{"format":"png","bit_depth":16,{SHARPEN}}}"#),
            control,
        ),
        "encoded/tiff16_p3" => encoded(
            doc,
            Some(projection),
            r#"{"format":"tiff","bit_depth":16,"profile":"display-p3"}"#,
            control,
        ),
        "encoded/png16_half_sharpen_p3" => encoded(
            doc,
            Some(projection),
            &format!(
                r#"{{"format":"png","bit_depth":16,{size},{SHARPEN},"profile":"display-p3"}}"#
            ),
            control,
        ),
        "encoded/png8_p3" => encoded(
            doc,
            Some(projection),
            r#"{"format":"png","bit_depth":8,"profile":"display-p3"}"#,
            control,
        ),
        "encoded/tiff16_render_sharpen_p3" => encoded(
            doc,
            None,
            &format!(r#"{{"format":"tiff","bit_depth":16,{SHARPEN},"profile":"display-p3"}}"#),
            control,
        ),
        "layer/png16_trimmed" => layer(doc, 1, true, r#"{"format":"png","bit_depth":16}"#, control),
        "layer/tiff16_p3_trimmed" => layer(
            doc,
            1,
            true,
            r#"{"format":"tiff","bit_depth":16,"profile":"display-p3"}"#,
            control,
        ),
        "layer/png16_canvas" => layer(doc, 1, false, r#"{"format":"png","bit_depth":16}"#, control),
        "layer/empty_trimmed" => layer(
            &inputs.empty_top,
            inputs.empty_top.layer_count() - 1,
            true,
            r#"{"format":"png","bit_depth":16}"#,
            control,
        ),
        "export16/png" => export16(doc, 0),
        "export16/tiff" => export16(doc, 1),
        "psd/flattened" => save_psd(&inputs.styled, &inputs.styled_projection, 1),
        "psd/layers_baked" => save_psd(&inputs.styled, &inputs.styled_projection, 0),
        "float/tiff32" => encoded(
            float,
            Some(float_projection),
            r#"{"format":"tiff","bit_depth":32}"#,
            control,
        ),
        "float/tiff32_render" => {
            encoded(float, None, r#"{"format":"tiff","bit_depth":32}"#, control)
        }
        "float/png16" => encoded(
            float,
            Some(float_projection),
            r#"{"format":"png","bit_depth":16}"#,
            control,
        ),
        "float/png16_half_sharpen" => encoded(
            float,
            Some(float_projection),
            &format!(r#"{{"format":"png","bit_depth":16,{size},{SHARPEN}}}"#),
            control,
        ),
        "float/native" => {
            let records = float.layer_count();
            float
                .encode_native(LIMIT, 0, control)
                .map(|stream| Some(native_bytes(&stream, records)))
        }
        other => panic!("unknown site {other}"),
    }
}

fn record_fixture(inputs: &Inputs) {
    let (w, h) = inputs.size;
    for (name, controlled) in SITES {
        evict();
        let (out, checkpoints, log) = if controlled {
            recorded(|c| site(inputs, name, Some(c)))
        } else {
            (site(inputs, name, None), 0, String::new())
        };
        println!(
            "export {name} {w}x{h} {} checkpoints={checkpoints} reports={}",
            sha256::sha256_hex(&outcome(&out)),
            sha256::sha256_hex(log.as_bytes())
        );
    }
}

/// Decodes a PNG16 stream to its u16 samples (the `image` PNG decoder).
fn decoded_png16(bytes: &[u8]) -> ((u32, u32), Vec<[u16; 4]>) {
    let decoder = image::codecs::png::PngDecoder::new(Cursor::new(bytes)).unwrap();
    assert_eq!(decoder.color_type(), image::ColorType::Rgba16);
    let dims = decoder.dimensions();
    let mut raw = vec![0; decoder.total_bytes() as usize];
    decoder.read_image(&mut raw).unwrap();
    let samples: Vec<u16> = raw
        .chunks_exact(2)
        .map(|p| u16::from_ne_bytes([p[0], p[1]]))
        .collect();
    (
        dims,
        samples
            .chunks_exact(4)
            .map(|p| [p[0], p[1], p[2], p[3]])
            .collect(),
    )
}

// ---------------------------------------------------------------- tests --

#[test]
fn record_export_reads() {
    let _serial = serial();
    for size in FIXTURES {
        record_fixture(&Inputs::new(size));
    }
}

#[test]
fn no_view_export_reads() {
    let _serial = serial();
    let inputs = Inputs::new((W, H));
    for (name, _) in SITES {
        if name == "layer/empty_trimmed" {
            continue;
        }
        evict();
        let answer = site(&inputs, name, None)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .expect("an answer");
        assert_eq!(contiguous_view_bytes(), 0, "{name}: no view held");
        drop(answer);
    }
}

#[test]
fn trimmed_layer_export_is_the_canvas_export_cropped() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let doc = two_layer_document(w, h, None);
        let png = request(r#"{"format":"png","bit_depth":16}"#).unwrap();
        let canvas = doc
            .export_layer_encoded(1, false, &png, LIMIT, 0, None)
            .expect("the canvas export");
        let trimmed = doc
            .export_layer_encoded(1, true, &png, LIMIT, 0, None)
            .expect("the trimmed export");
        let (dims, samples) = decoded_png16(canvas.bytes());
        assert_eq!(dims, (w, h));
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..h {
            for x in 0..w {
                if samples[(y * w + x) as usize][3] != 0 {
                    (x0, y0) = (x0.min(x), y0.min(y));
                    (x1, y1) = (x1.max(x + 1), y1.max(y + 1));
                }
            }
        }
        assert!(x0 < x1 && y0 < y1, "{w}×{h}: the Off layer shows");
        let (bw, bh) = (x1 - x0, y1 - y0);
        let mut expected = Vec::new();
        for y in y0..y1 {
            let row = (y * w) as usize;
            expected.extend_from_slice(&samples[row + x0 as usize..row + x1 as usize]);
        }
        let (trimmed_dims, trimmed_samples) = decoded_png16(trimmed.bytes());
        assert_eq!(trimmed_dims, (bw, bh), "{w}×{h}: the trimmed size");
        assert_eq!(trimmed_samples, expected, "{w}×{h}: the cropped samples");
        let info: Value = trimmed.info_json();
        assert_eq!(info["layer"]["origin"], serde_json::json!([x0, y0]));
        assert_eq!(info["layer"]["size"], serde_json::json!([bw, bh]));
    }
}

#[test]
fn psd_records_hold_the_source_samples() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let styled = two_layer_document(w, h, Some(STROKE));
        let projection = styled.render(LIMIT, 0, None).expect("the projection");
        let flattened = save_psd(&styled, &projection, 1)
            .expect("the flattened PSD")
            .expect("its bytes");
        let file = fixture::read(&flattened);
        assert_eq!((file.width, file.height, file.depth), (w, h, 16));
        assert_eq!(
            file.records[0].rgba(),
            projection.to_rgba16().unwrap(),
            "{w}×{h}: the flattened record is the projection"
        );
        let layers = save_psd(&styled, &projection, 0)
            .expect("the layered PSD")
            .expect("its bytes");
        let file = fixture::read(&layers);
        let base = fixture::by_name(&file)["Base"];
        assert_eq!(
            base.rgba(),
            styled.layer_source(0).unwrap().to_rgba16().unwrap(),
            "{w}×{h}: the base record is the base layer"
        );
    }
}

#[test]
fn float_exports_round_trip_exactly() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let doc = float_document(w, h, &extended(w, h, 5));
        let projection = doc.render(LIMIT, 0, None).expect("the projection");
        let tiff = encoded(
            &doc,
            Some(&projection),
            r#"{"format":"tiff","bit_depth":32}"#,
            None,
        )
        .expect("the float TIFF")
        .expect("its bytes");
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("reopened.tiff");
        std::fs::write(&path, &tiff).unwrap();
        let reopened = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
            .expect("the float TIFF reopens");
        let bits = |image: &PrecisionImage| -> Vec<u32> {
            image
                .to_rgba32()
                .unwrap()
                .iter()
                .flatten()
                .map(|v| v.to_bits())
                .collect()
        };
        assert_eq!(
            bits(reopened.layer_source(0).expect("a raster")),
            bits(&projection),
            "{w}×{h}: the float TIFF holds the projection's bits"
        );
        let stream = doc
            .encode_native(LIMIT, 0, None)
            .expect("the native stream");
        let parsed =
            PrecisionDocument::parse_native(&stream, LIMIT, 0, None).expect("the stream reopens");
        assert_eq!(parsed.layer_count(), doc.layer_count());
        for i in 0..doc.layer_count() {
            if let Some(source) = doc.layer_source(i) {
                assert!(
                    parsed
                        .layer_source(i)
                        .expect("a raster")
                        .same_samples(source),
                    "{w}×{h}: layer {i} reopens with its samples"
                );
            }
        }
    }
}
