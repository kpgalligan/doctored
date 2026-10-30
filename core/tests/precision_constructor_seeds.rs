//! Black-box suite for 13B-3c-2c-2: the Copy / Cut payload and `convert`'s
//! cross-storage output are built UNSEEDED, and the payload's two sequential
//! readers (the RZCB `encode` and `converted`) walk runs — with every RZCB
//! byte, typed sample, checkpoint count, report log and refusal unchanged.
//! Since 13B-3d-3a the layer producers are unseeded
//! (`precision_layer_producers.rs`), so `no_view_converted_input` asserts 0
//! and fails at `63421b3` by design (13B-3d-3a); since 13B-3d-3b
//! `add_layer_at`'s planes are filled and seed nothing — its seeded test
//! moved to `precision_filled_outputs.rs` as `no_view_filled_planes`.
//!
//! Oracles are independent of the implementation: the pixel section of an
//! RZCB stream is the payload image's samples written out little-endian
//! here (u16 codes, f32 bits), and `convert` is `v / 65535` (u16 → f32) and
//! `round(clamp(v, 0, 1) · 65535)` (f32 → u16), written out here.
//!
//! Every `record_*` test also prints one line per case —
//! `seeds <label> <WxH> <sha256> checkpoints=<n> reports=<sha256>` — so the
//! same file run against HEAD and the tree diffs byte-identical answers and
//! checkpoint/report sequences. It uses only APIs present at `71e4873`.
//!
//! `no_view_*` tests fail at `71e4873` by design. The view cache is
//! process-wide, so every test holds `serial()`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_bytes, dr_clipboard_payload_image, dr_precision_doc_clipboard_copy,
    dr_precision_doc_clipboard_cut, DrClipboardPayload,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{ClipboardPayload, ClipboardRead};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
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
/// The RZCB header before the profile bytes: ten 4-byte words.
const FIXED_HEADER: usize = 40;

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
            "seeds {label} {w}x{h} {} checkpoints={} reports={}",
            sha256::sha256_hex(&self.answers),
            self.checkpoints,
            sha256::sha256_hex(self.log.as_bytes())
        );
    }
}

/// u16 codes little-endian.
fn u16_bytes(samples: &[[u16; 4]]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()))
        .collect()
}

/// f32 bits little-endian.
fn f32_bytes(samples: &[[f32; 4]]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|p| p.iter().flat_map(|c| c.to_bits().to_le_bytes()))
        .collect()
}

/// An image's dimensions and samples as bytes (u16 codes, f32 bits).
fn image_bytes(image: &PrecisionImage) -> Vec<u8> {
    let (w, h) = image.dimensions();
    let mut out: Vec<u8> = [w, h].iter().flat_map(|v| v.to_le_bytes()).collect();
    match image.storage() {
        PrecisionStorage::Rgba16Unorm => out.extend(u16_bytes(&image.to_rgba16().unwrap())),
        PrecisionStorage::Rgba32Float => out.extend(f32_bytes(&image.to_rgba32().unwrap())),
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

/// Two u16 rasters in `profile`: an opaque canvas-sized base and the "Off"
/// layer (`w/2`×`5h/13` at `(−w/6, 10h/13)`, opacity 0.8), partly off the
/// canvas.
fn two_layer_document(w: u32, h: u32, profile: std::sync::Arc<IccProfile>) -> PrecisionDocument {
    let (ow, oh, ox, oy) = (
        (w / 2).max(1),
        (h * 5 / 13).max(1),
        -((w / 6) as i32),
        (h * 10 / 13) as i32,
    );
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
    let path = directory.path().join("seeds.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// Canvas-sized u16 coverage with 0, 65535 and fractions.
fn ramp_coverage(w: u32, h: u32) -> Vec<u16> {
    (0..w * h)
        .map(|i| match i % 5 {
            0 => 0,
            1 | 2 => u16::MAX,
            _ => ((i * 7919) % 65536) as u16,
        })
        .collect()
}

/// A window across both seams at 600×520, contained in every fixture.
fn seam_window(w: u32, h: u32) -> (u32, u32, u32, u32) {
    (w / 6, h / 5, (w * 3 / 4).max(1), (h * 4 / 5).max(1))
}

fn copy_request(layer: Option<usize>, (x, y, width, height): (u32, u32, u32, u32)) -> Value {
    match layer {
        Some(layer) => {
            json!({"op":"copy","layer":layer,"x":x,"y":y,"width":width,"height":height})
        }
        None => json!({"op":"copy_merged","x":x,"y":y,"width":width,"height":height}),
    }
}

fn coverage_parts(coverage: Option<&[u16]>) -> (*const u16, usize) {
    coverage.map_or((ptr::null(), 0), |c| (c.as_ptr(), c.len()))
}

/// Copy / Copy Merged through `dr_precision_doc_clipboard_copy`.
fn copy(
    doc: &PrecisionDocument,
    request: &Value,
    coverage: Option<&[u16]>,
    control: Option<&DrJobControl>,
) -> Result<Box<DrClipboardPayload>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let (cov, len) = coverage_parts(coverage);
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_copy(
            doc,
            text.as_ptr(),
            cov,
            len,
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

type CutOutcome = (Box<DrClipboardPayload>, Option<Box<PrecisionDocument>>);

/// Cut through `dr_precision_doc_clipboard_cut`.
fn cut(
    doc: &PrecisionDocument,
    layer: usize,
    (x, y, width, height): (u32, u32, u32, u32),
    coverage: Option<&[u16]>,
    control: Option<&DrJobControl>,
) -> Result<CutOutcome, String> {
    let text = CString::new(
        json!({"op":"cut","layer":layer,"x":x,"y":y,"width":width,"height":height}).to_string(),
    )
    .unwrap();
    let (cov, len) = coverage_parts(coverage);
    let mut payload = ptr::null_mut();
    let mut error = ptr::null_mut();
    let next = unsafe {
        dr_precision_doc_clipboard_cut(
            doc,
            text.as_ptr(),
            cov,
            len,
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut payload,
            &mut error,
        )
    };
    if !error.is_null() {
        assert!(next.is_null() && payload.is_null());
        return Err(take(error));
    }
    assert!(!payload.is_null());
    Ok((
        unsafe { Box::from_raw(payload) },
        (!next.is_null()).then(|| unsafe { Box::from_raw(next) }),
    ))
}

/// The handle's RZCB bytes (`dr_clipboard_payload_bytes`).
fn stream(handle: &DrClipboardPayload) -> Vec<u8> {
    let mut length = 0;
    let bytes = unsafe { dr_clipboard_payload_bytes(handle, &mut length) };
    assert!(!bytes.is_null());
    unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec()
}

/// Checks the stream's pixel section against the payload image's samples
/// written out here (u16 codes or f32 bits, little-endian).
fn check_pixel_section(handle: &DrClipboardPayload, bytes: &[u8], label: &str) {
    let image = unsafe { Box::from_raw(dr_clipboard_payload_image(handle)) };
    let profile_len = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
    // Profile, flag byte, tone-map operator + white, editable length.
    let start = FIXED_HEADER + profile_len + 1 + 8 + 4;
    let expected = match image.storage() {
        PrecisionStorage::Rgba16Unorm => u16_bytes(&image.to_rgba16().unwrap()),
        PrecisionStorage::Rgba32Float => f32_bytes(&image.to_rgba32().unwrap()),
    };
    assert_eq!(
        &bytes[start..start + expected.len()],
        &expected[..],
        "{label}: the RZCB pixel section is the payload's samples"
    );
}

// ------------------------------------------------------------ record_* --

#[test]
fn record_clipboard_payloads() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let u16_doc = two_layer_document(w, h, IccProfile::srgb());
        let float = float_document(w, h, &extended(w, h, 5));
        let coverage = ramp_coverage(w, h);
        let seam = seam_window(w, h);
        let whole = (0, 0, w, h);
        for (depth, doc, layer) in [("u16", &u16_doc, 1), ("f32", &float, 0)] {
            let cases: [(&str, Value, Option<&[u16]>); 6] = [
                ("copy/seam", copy_request(Some(layer), seam), None),
                (
                    "copy/seam_coverage",
                    copy_request(Some(layer), seam),
                    Some(&coverage),
                ),
                ("copy/whole", copy_request(Some(layer), whole), None),
                (
                    "copy/whole_coverage",
                    copy_request(Some(layer), whole),
                    Some(&coverage),
                ),
                ("copy_merged/seam", copy_request(None, seam), None),
                (
                    "copy_merged/whole_coverage",
                    copy_request(None, whole),
                    Some(&coverage),
                ),
            ];
            for (label, request, cov) in cases {
                let label = format!("clipboard/{depth}/{label}");
                let mut line = Line::default();
                evict();
                let handle = line
                    .add(recorded(|c| copy(doc, &request, cov, Some(c))))
                    .unwrap_or_else(|e| panic!("{label} {w}×{h}: {e}"));
                let bytes = stream(&handle);
                check_pixel_section(&handle, &bytes, &label);
                line.answers.extend(&bytes);
                line.print(&label, (w, h));
            }
            let cuts: [(&str, Option<&[u16]>); 2] =
                [("cut/seam", None), ("cut/seam_coverage", Some(&coverage))];
            for (label, cov) in cuts {
                let label = format!("clipboard/{depth}/{label}");
                let mut line = Line::default();
                evict();
                let (handle, next) = line
                    .add(recorded(|c| cut(doc, layer, seam, cov, Some(c))))
                    .unwrap_or_else(|e| panic!("{label} {w}×{h}: {e}"));
                let bytes = stream(&handle);
                check_pixel_section(&handle, &bytes, &label);
                line.answers.extend(&bytes);
                match &next {
                    Some(next) => line.answers.extend(document_bytes(next)),
                    None => line.answers.extend(b"none"),
                }
                line.print(&label, (w, h));
            }
        }
    }
}

#[test]
fn record_converted() {
    let _serial = serial();
    let srgb = IccProfile::srgb();
    for (w, h) in FIXTURES {
        let p3 = two_layer_document(w, h, IccProfile::display_p3());
        let float = float_document(w, h, &extended(w, h, 7));
        let seam = seam_window(w, h);
        let coverage = ramp_coverage(w, h);
        let read = |layer: usize| ClipboardRead::Copy {
            layer,
            x: seam.0,
            y: seam.1,
            width: seam.2,
            height: seam.3,
        };
        let p3_payload = p3
            .clipboard_copy(&read(1), Some(&coverage), LIMIT, 0, None)
            .expect("a P3 payload");
        let float_payload = float
            .clipboard_copy(&read(0), None, LIMIT, 0, None)
            .expect("a float payload");
        let float_profile = float_payload.image().profile().clone();
        let cases: [(&str, &ClipboardPayload, _); 3] = [
            ("converted/p3_u16_to_srgb_u16", &p3_payload, srgb.clone()),
            ("converted/f32_to_srgb_u16", &float_payload, srgb.clone()),
            ("converted/f32_to_own_u16", &float_payload, float_profile),
        ];
        for (label, payload, profile) in cases {
            let mut line = Line::default();
            evict();
            let out = line.add(recorded(|c| {
                payload.converted(PrecisionStorage::Rgba16Unorm, &profile, LIMIT, 0, Some(c))
            }));
            match &out {
                Ok(Some(converted)) => line.answers.extend(converted.encode().unwrap()),
                Ok(None) => line.answers.extend(b"none"),
                Err(e) => line.answers.extend(format!("error: {e}").bytes()),
            }
            line.print(label, (w, h));
        }
    }
}

#[test]
fn record_convert() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let codes = pattern16(w, h, 3);
        let wide =
            PrecisionImage::from_rgba16(w, h, codes.clone(), IccProfile::srgb(), None).unwrap();
        let samples = extended(w, h, 4);
        let float =
            PrecisionImage::from_rgba32_extended(w, h, samples.clone(), IccProfile::srgb(), None)
                .unwrap();

        let mut line = Line::default();
        evict();
        let out = line
            .add(recorded(|c| {
                wide.convert(PrecisionStorage::Rgba32Float, LIMIT, 0, Some(c))
            }))
            .expect("u16 → f32");
        let expected: Vec<[f32; 4]> = codes
            .iter()
            .map(|p| p.map(|v| f32::from(v) / 65535.0))
            .collect();
        assert_eq!(
            f32_bytes(&out.to_rgba32().unwrap()),
            f32_bytes(&expected),
            "u16 → f32 {w}×{h}"
        );
        line.answers.extend(image_bytes(&out));
        line.print("convert/u16_to_f32", (w, h));

        let mut line = Line::default();
        evict();
        let out = line
            .add(recorded(|c| {
                float.convert(PrecisionStorage::Rgba16Unorm, LIMIT, 0, Some(c))
            }))
            .expect("f32 → u16");
        let expected: Vec<[u16; 4]> = samples
            .iter()
            .map(|p| p.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16))
            .collect();
        assert_eq!(out.to_rgba16().unwrap(), expected, "f32 → u16 {w}×{h}");
        line.answers.extend(image_bytes(&out));
        line.print("convert/f32_to_u16", (w, h));
    }
}

// ------------------------------------------------------------ no_view_* --

#[test]
fn no_view_clipboard_payloads() {
    let _serial = serial();
    let u16_doc = two_layer_document(W, H, IccProfile::srgb());
    let float = float_document(W, H, &extended(W, H, 5));
    let whole = (0, 0, W, H);
    for (depth, doc, layer) in [("u16", &u16_doc, 1), ("f32", &float, 0)] {
        for request in [copy_request(Some(layer), whole), copy_request(None, whole)] {
            evict();
            assert_eq!(contiguous_view_bytes(), 0, "{depth}: evicted");
            let handle = copy(doc, &request, None, None).expect("a copy");
            assert_eq!(
                contiguous_view_bytes(),
                0,
                "{depth} {request}: a held payload handle holds no view"
            );
            drop(handle);
        }
        evict();
        let (handle, next) = cut(doc, layer, seam_window(W, H), None, None).expect("a cut");
        let next = next.expect("a changed document");
        assert_eq!(
            contiguous_view_bytes(),
            0,
            "{depth}: a Cut leaves no view (13B-3d-1)"
        );
        drop((handle, next));
    }
}

#[test]
fn no_view_converted_input() {
    let _serial = serial();
    let srgb = IccProfile::srgb();
    let p3 = two_layer_document(W, H, IccProfile::display_p3());
    let float = float_document(W, H, &extended(W, H, 7));
    let whole = ClipboardRead::Copy {
        layer: 0,
        x: 0,
        y: 0,
        width: W,
        height: H,
    };
    for (label, doc) in [("p3 u16", &p3), ("f32", &float)] {
        let payload = doc
            .clipboard_copy(&whole, None, LIMIT, 0, None)
            .expect("a payload");
        evict();
        assert_eq!(contiguous_view_bytes(), 0, "{label}: evicted");
        let out = payload
            .converted(PrecisionStorage::Rgba16Unorm, &srgb, LIMIT, 0, None)
            .expect("a conversion")
            .expect("a converted payload");
        assert_eq!(
            contiguous_view_bytes(),
            0,
            "{label}: the converted output holds no view (13B-3d-3a)"
        );
        drop((out, payload));
    }
}

#[test]
fn no_view_convert() {
    let _serial = serial();
    let wide =
        PrecisionImage::from_rgba16(W, H, pattern16(W, H, 3), IccProfile::srgb(), None).unwrap();
    let float =
        PrecisionImage::from_rgba32_extended(W, H, extended(W, H, 4), IccProfile::srgb(), None)
            .unwrap();
    for (label, image, storage) in [
        ("u16 → f32", &wide, PrecisionStorage::Rgba32Float),
        ("f32 → u16", &float, PrecisionStorage::Rgba16Unorm),
    ] {
        evict();
        assert_eq!(contiguous_view_bytes(), 0, "{label}: evicted");
        let out = image
            .convert(storage, LIMIT, 0, None)
            .expect("a conversion");
        assert_eq!(
            contiguous_view_bytes(),
            0,
            "{label}: a held converted image holds no view"
        );
        drop(out);
    }
}
