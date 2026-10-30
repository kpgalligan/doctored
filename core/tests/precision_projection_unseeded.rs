//! Black-box suite for 13B-3c-2b: the typed renders name their output-view
//! policy. The host projection and the regional window are `Unseeded`, and
//! so is every internal projection read only through tile reads — Copy /
//! Copy Merged (`Window::read` walks `row_segments` rows), both conversion
//! previews (paired runs) and the bake's opacity scan (`runs_rgba16`) — with
//! answers, checkpoints, reports and refusals unchanged. Since 13B-3d-3a the
//! renders installed as a layer (merges, stamp, flatten, the bake) are
//! `Unseeded` too (`precision_layer_producers.rs`); only the export chains'
//! renders stay `Seeded` (13B-3e).
//!
//! Oracles are independent of the implementation: every expected answer is
//! computed HERE — Copy's window sliced by hand out of the render's (or the
//! layer's) owned samples with the coverage rules written out (u16
//! `(a·c + 32767) / 65535`, f32 `a·(c / 65535.0)`), the previews' deltas
//! from `render().to_rgba16()` against `flattened()` ×257.
//!
//! Every `record_*` test also prints one line per case —
//! `unseeded <label> <WxH> <sha256> checkpoints=<n> reports=<sha256>` — so
//! the same file run against HEAD and the tree diffs byte-identical answers
//! and checkpoint/report sequences. It uses only APIs present at `694194c`.
//!
//! `no_view_*` tests fail at `694194c` by design. 13B-3c-2b's
//! `seeded_outputs_keep_their_view` (decision D2) moved to
//! `precision_layer_producers.rs` in 13B-3d-3a, inverted: those renders now
//! leave no view. The view cache is process-wide, so every test holds
//! `serial()`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_output_color::{
    dr_precision_doc_conversion_preflight, dr_precision_doc_convert_to_profile_with,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{ClipboardPayload, ClipboardRead};
use doctored_core::precision_conversion_narrow::Narrowed;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::PrecisionStorage;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use doctored_core::precision_render::{PrecisionLayer, PrecisionScene};
use doctored_core::tiff_float::FloatTiff;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::sha256;
use crate::resource_report;
use crate::serial;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 600;
const H: u32 = 520;
/// 600×520 crosses both tile seams (x 256, 512; y 256, 512).
const FIXTURES: [(u32, u32); 3] = [(W, H), (257, 3), (1, 1)];
/// `DR_PROFILE_DISPLAY_P3`, the builtin non-sRGB profile.
const PROFILE_DISPLAY_P3: std::ffi::c_int = 1;

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

fn owned_json(result: *mut c_char, error: *mut c_char) -> Value {
    assert!(error.is_null(), "{}", take(error));
    assert!(!result.is_null());
    let value = serde_json::from_slice(unsafe { CStr::from_ptr(result) }.to_bytes()).unwrap();
    unsafe { dr_string_free(result) };
    value
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

/// One job's checkpoint count and the sha256 of its report log.
struct Recorded {
    checkpoints: u64,
    reports: String,
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

    fn print(&self, label: &str, dimensions: (u32, u32)) {
        let rec = Recorded {
            checkpoints: self.checkpoints,
            reports: sha256::sha256_hex(self.log.as_bytes()),
        };
        print_line(label, dimensions, &self.answers, &rec);
    }
}

fn print_line(label: &str, (w, h): (u32, u32), answer: &[u8], recorded: &Recorded) {
    println!(
        "unseeded {label} {w}x{h} {} checkpoints={} reports={}",
        sha256::sha256_hex(answer),
        recorded.checkpoints,
        recorded.reports
    );
}

fn edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take(err));
    *unsafe { Box::from_raw(out) }
}

fn geometry(doc: &PrecisionDocument, request: Value) -> Option<PrecisionDocument> {
    let parsed = GeometryEdit::parse(&request.to_string()).expect("a geometry request");
    doc.geometry(&parsed, LIMIT, 0, None)
        .expect("the edit runs")
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
    let path = directory.path().join("unseeded.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// The "Off" layer's rect and the "Seam" layer's rect for a `w`×`h` canvas:
/// at 600×520, 300×200 at (−100, 400) and 390×260 at (130, 140)
/// (`precision_image_reads::u16_document`).
fn off_rect(w: u32, h: u32) -> (u32, u32, i32, i32) {
    (
        (w / 2).max(1),
        (h * 5 / 13).max(1),
        -((w / 6) as i32),
        (h * 10 / 13) as i32,
    )
}

fn seam_rect(w: u32, h: u32) -> (u32, u32, i32, i32) {
    (
        (w * 13 / 20).max(1),
        (h / 2).max(1),
        (w * 13 / 60) as i32,
        (h * 7 / 26) as i32,
    )
}

/// A u16 document of three rasters: the base, an offset layer partly off
/// the canvas at opacity 0.8, and "Seam" across the tile seams.
fn u16_document(w: u32, h: u32) -> PrecisionDocument {
    let (ow, oh, ox, oy) = off_rect(w, h);
    let (sw, sh, sx, sy) = seam_rect(w, h);
    let layers = vec![
        Layer::raster("Base", image16(w, h, pattern16(w, h, 1)), (0, 0), 1.0, true).unwrap(),
        Layer::raster(
            "Off",
            image16(ow, oh, pattern16(ow, oh, 2)),
            (ox, oy),
            0.8,
            true,
        )
        .unwrap(),
        Layer::raster(
            "Seam",
            image16(sw, sh, pattern16(sw, sh, 3)),
            (sx, sy),
            1.0,
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

/// `precision_image_reads::float_multi_document`'s shape: the float base, a
/// copy of the "Seam" window pasted in place, then its duplicate moved by an
/// integer translation and set to blend mode 1.
fn float_multi_document(w: u32, h: u32) -> PrecisionDocument {
    let doc = float_document(w, h, &extended(w, h, 21));
    let (sw, sh, sx, sy) = seam_rect(w, h);
    let copied = doc
        .clipboard_copy(
            &serde_json::from_value(
                json!({"op":"copy","layer":0,"x":sx,"y":sy,"width":sw,"height":sh}),
            )
            .unwrap(),
            None,
            LIMIT,
            0,
            None,
        )
        .expect("a float copy");
    let doc = doc
        .clipboard_paste(
            &copied,
            &serde_json::from_value(
                json!({"name":"Seam","anchor":0,"position":"top","placement":"in_place"}),
            )
            .unwrap(),
            LIMIT,
            0,
            None,
        )
        .expect("a float paste");
    let doc = doc
        .duplicate_layer(1)
        .expect("a duplicate")
        .expect("a changed document");
    let top = doc.layer_count() - 1;
    let (dx, dy) = (-((w / 3) as i32), (h * 25 / 52) as i32);
    let doc = if (dx, dy) == (0, 0) {
        doc
    } else {
        geometry(
            &doc,
            json!({"op":"affine","layer":top,"target":"pixels","matrix":[1,0,0,1,dx,dy],"filter":"bilinear"}),
        )
        .expect("a move")
    };
    edit(
        &doc,
        json!({"op":"layer_blend","version":1,"layer":top,"blend_mode":1}),
    )
}

/// The canvas coverage pattern: 0 / 1 / 32768 / 65534 / 65535 and two
/// intermediates.
fn coverage_pattern(w: u32, h: u32) -> Vec<u16> {
    const VALUES: [u16; 7] = [0, 1, 32768, 65534, 65535, 12345, 40000];
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            VALUES[((x * 3 + y * 5) % 7) as usize]
        })
        .collect()
}

/// The three Copy Merged windows: the whole canvas, one crossing both seams
/// (as far as the canvas has them), and 1×1 at the last pixel.
fn merged_windows(w: u32, h: u32) -> [(u32, u32, u32, u32); 3] {
    let span = |n: u32| {
        if n > 256 {
            (250, (n - 250).min(20))
        } else {
            (0, n)
        }
    };
    let ((sx, sw), (sy, sh)) = (span(w), span(h));
    [(0, 0, w, h), (sx, sy, sw, sh), (w - 1, h - 1, 1, 1)]
}

fn cover_u16(alpha: u16, coverage: u16) -> u16 {
    ((u32::from(alpha) * u32::from(coverage) + 32767) / 65535) as u16
}

fn cover_f32(alpha: f32, coverage: u16) -> f32 {
    alpha * (f32::from(coverage) / 65535.0)
}

/// The window's expected samples out of `source` (`sw`×`sh`, row-major)
/// placed at `offset`: zeros where it does not reach, alpha covered.
fn window_of<T: Copy + Default>(
    source: &[[T; 4]],
    (sw, sh): (u32, u32),
    offset: (i32, i32),
    (x, y, width, height): (u32, u32, u32, u32),
    coverage: Option<(&[u16], u32)>,
    cover: impl Fn(T, u16) -> T,
) -> Vec<[T; 4]> {
    let mut out = Vec::new();
    for py in 0..height {
        for px in 0..width {
            let (cx, cy) = (i64::from(x + px), i64::from(y + py));
            let (lx, ly) = (cx - i64::from(offset.0), cy - i64::from(offset.1));
            let mut p = if lx >= 0 && ly >= 0 && lx < i64::from(sw) && ly < i64::from(sh) {
                source[ly as usize * sw as usize + lx as usize]
            } else {
                [T::default(); 4]
            };
            let c = coverage.map_or(u16::MAX, |(c, cw)| {
                c[(y + py) as usize * cw as usize + (x + px) as usize]
            });
            p[3] = cover(p[3], c);
            out.push(p);
        }
    }
    out
}

fn cropped(
    coverage: Option<(&[u16], u32)>,
    (x, y, width, height): (u32, u32, u32, u32),
) -> Option<Vec<u16>> {
    coverage.map(|(c, cw)| {
        (0..height)
            .flat_map(|py| (0..width).map(move |px| (px, py)))
            .map(|(px, py)| c[(y + py) as usize * cw as usize + (x + px) as usize])
            .collect()
    })
}

fn bits(samples: &[[f32; 4]]) -> Vec<[u32; 4]> {
    samples.iter().map(|p| p.map(f32::to_bits)).collect()
}

/// Checks one payload against the expected window and coverage.
fn check_payload(
    payload: &ClipboardPayload,
    expected: Expected,
    window: (u32, u32, u32, u32),
    coverage: Option<(&[u16], u32)>,
    what: &str,
) {
    assert_eq!(
        payload.offset(),
        (window.0 as i32, window.1 as i32),
        "{what}: offset"
    );
    assert_eq!(
        payload.coverage().map(<[u16]>::to_vec),
        cropped(coverage, window),
        "{what}: coverage"
    );
    assert_eq!(payload.image().dimensions(), (window.2, window.3), "{what}");
    match expected {
        Expected::U16(samples) => {
            assert_eq!(payload.image().to_rgba16().unwrap(), samples, "{what}")
        }
        Expected::F32(samples) => assert_eq!(
            bits(&payload.image().to_rgba32().unwrap()),
            bits(&samples),
            "{what}"
        ),
    }
}

enum Expected {
    U16(Vec<[u16; 4]>),
    F32(Vec<[f32; 4]>),
}

fn copy_request(read: Value) -> ClipboardRead {
    serde_json::from_value(read).unwrap()
}

// ------------------------------------------------------------ record_* --

#[test]
fn record_copy_merged() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let pattern = coverage_pattern(w, h);
        for (depth, doc) in [
            ("u16", u16_document(w, h)),
            ("f32", float_multi_document(w, h)),
        ] {
            let render = doc.render(LIMIT, 0, None).unwrap();
            let mut line = Line::default();
            for window in merged_windows(w, h) {
                for coverage in [None, Some((pattern.as_slice(), w))] {
                    let (x, y, width, height) = window;
                    let read = copy_request(
                        json!({"op":"copy_merged","x":x,"y":y,"width":width,"height":height}),
                    );
                    let payload = line.add(recorded(|c| {
                        doc.clipboard_copy(&read, coverage.map(|(c, _)| c), LIMIT, 0, Some(c))
                            .unwrap()
                    }));
                    let expected = match render.storage() {
                        PrecisionStorage::Rgba16Unorm => Expected::U16(window_of(
                            &render.to_rgba16().unwrap(),
                            (w, h),
                            (0, 0),
                            window,
                            coverage,
                            cover_u16,
                        )),
                        PrecisionStorage::Rgba32Float => Expected::F32(window_of(
                            &render.to_rgba32().unwrap(),
                            (w, h),
                            (0, 0),
                            window,
                            coverage,
                            cover_f32,
                        )),
                    };
                    let what = format!("{depth} {w}×{h} window {window:?}");
                    check_payload(&payload, expected, window, coverage, &what);
                    line.answers.extend(payload.encode().unwrap());
                }
            }
            line.print(&format!("copy_merged/{depth}"), (w, h));
        }
    }
}

#[test]
fn record_copy_layer() {
    let _serial = serial();
    let pattern = coverage_pattern(W, H);
    let windows = [
        // partly outside the layer, crossing its top-left corner
        (100, 120, 80, 60),
        // overlaps the layer vertically, entirely left of it
        (0, 200, 100, 10),
        // overlaps the layer vertically, entirely right of it
        (530, 200, 60, 10),
        // entirely above it
        (0, 0, 600, 100),
        // inside it, across both tile seams
        (200, 230, 150, 100),
    ];
    for (depth, doc) in [
        ("u16", u16_document(W, H)),
        ("f32", float_multi_document(W, H)),
    ] {
        let index = (0..doc.layer_count())
            .find(|&i| doc.layer(i).unwrap().name() == "Seam")
            .expect("a Seam layer");
        let layer = doc.layer(index).unwrap();
        assert_eq!(layer.offset(), (130, 140), "{depth}");
        let source = layer.source().expect("a raster").clone();
        assert_eq!(source.dimensions(), (390, 260), "{depth}");
        let mut line = Line::default();
        for window in windows {
            for coverage in [None, Some((pattern.as_slice(), W))] {
                let (x, y, width, height) = window;
                let read = copy_request(
                    json!({"op":"copy","layer":index,"x":x,"y":y,"width":width,"height":height}),
                );
                let payload = line.add(recorded(|c| {
                    doc.clipboard_copy(&read, coverage.map(|(c, _)| c), LIMIT, 0, Some(c))
                        .unwrap()
                }));
                let expected = match source.storage() {
                    PrecisionStorage::Rgba16Unorm => Expected::U16(window_of(
                        &source.to_rgba16().unwrap(),
                        (390, 260),
                        (130, 140),
                        window,
                        coverage,
                        cover_u16,
                    )),
                    PrecisionStorage::Rgba32Float => Expected::F32(window_of(
                        &source.to_rgba32().unwrap(),
                        (390, 260),
                        (130, 140),
                        window,
                        coverage,
                        cover_f32,
                    )),
                };
                let what = format!("{depth} copy window {window:?}");
                check_payload(&payload, expected, window, coverage, &what);
                line.answers.extend(payload.encode().unwrap());
            }
        }
        line.print(&format!("copy_layer/{depth}"), (W, H));
    }
}

/// `narrow_structure_preview`'s keys over two u16 planes, written out.
fn preview_oracle(before: &[[u16; 4]], after: &[[u16; 4]]) -> Value {
    let (mut max, mut sum, mut compared, mut skipped) = (0_u16, 0_u64, 0_u64, 0_u64);
    for (b, a) in before.iter().zip(after) {
        let channels = if b[3] == 0 && a[3] == 0 {
            skipped += 3;
            3..4
        } else {
            0..4
        };
        for c in channels {
            let delta = b[c].abs_diff(a[c]);
            max = max.max(delta);
            sum += u64::from(delta);
            compared += 1;
        }
    }
    let mean = if compared == 0 {
        0.0
    } else {
        sum as f64 / compared as f64
    };
    json!({"scale":65535,"max_delta":max,"mean_delta":mean,
        "compared_samples":compared,"skipped_latent_rgb":skipped})
}

fn widened(image: &RgbaImage) -> Vec<[u16; 4]> {
    image
        .pixels()
        .map(|p| p.0.map(|v| u16::from(v) * 257))
        .collect()
}

#[test]
fn record_narrow_preview() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        // The Rgba8 arm: a u16 document against its byte twin.
        let doc = u16_document(w, h);
        let twin = doc
            .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
            .expect("the byte twin");
        let mut line = Line::default();
        let value = line.add(recorded(|c| {
            doc.narrow_structure_preview(Narrowed::Rgba8(&twin), LIMIT, 0, Some(c))
                .unwrap()
        }));
        let before = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
        let after = widened(&twin.flattened().unwrap());
        assert_eq!(value, preview_oracle(&before, &after), "rgba8 {w}×{h}");
        line.answers.extend(value.to_string().bytes());
        line.print("narrow_preview/rgba8", (w, h));

        // The Rgba16 arm: a one-raster float document against its 16-bit
        // twin — delta 0, the counts from the alpha-zero pixels. Alpha is
        // 0 or 1 here: at a fractional alpha the u16 twin's own composite
        // re-derives straight RGB and may differ by one code.
        let samples: Vec<[f32; 4]> = extended(w, h, 5)
            .into_iter()
            .map(|[r, g, b, a]| [r, g, b, if a > 0.0 { 1.0 } else { 0.0 }])
            .collect();
        let doc = float_document(w, h, &samples);
        let narrowed = doc
            .narrow_structure_to_u16(LIMIT, 0, None)
            .expect("the 16-bit twin");
        let mut line = Line::default();
        let value = line.add(recorded(|c| {
            doc.narrow_structure_preview(Narrowed::Rgba16(&narrowed), LIMIT, 0, Some(c))
                .unwrap()
        }));
        let n = u64::from(w) * u64::from(h);
        let hidden = samples.iter().filter(|p| p[3] == 0.0).count() as u64;
        assert_eq!(
            value,
            json!({"scale":65535,"max_delta":0,"mean_delta":0.0,
                "compared_samples":4 * n - 3 * hidden,"skipped_latent_rgb":3 * hidden}),
            "rgba16 {w}×{h}"
        );
        line.answers.extend(value.to_string().bytes());
        line.print("narrow_preview/rgba16", (w, h));

        // The same arm at fractional alpha (0, 1 and k/11, unmodified): the
        // deltas are only recorded; the counts still follow the alpha-zero
        // pixels, which no fractional alpha rounds to.
        let samples = extended(w, h, 5);
        let doc = float_document(w, h, &samples);
        let narrowed = doc
            .narrow_structure_to_u16(LIMIT, 0, None)
            .expect("the 16-bit twin");
        let mut line = Line::default();
        let value = line.add(recorded(|c| {
            doc.narrow_structure_preview(Narrowed::Rgba16(&narrowed), LIMIT, 0, Some(c))
                .unwrap()
        }));
        let hidden = samples.iter().filter(|p| p[3] == 0.0).count() as u64;
        assert_eq!(
            value["skipped_latent_rgb"],
            3 * hidden,
            "fractional {w}×{h}"
        );
        assert_eq!(
            value["compared_samples"],
            4 * n - 3 * hidden,
            "fractional {w}×{h}"
        );
        line.answers.extend(value.to_string().bytes());
        line.print("narrow_preview/rgba16_fractional", (w, h));
    }
    // The twin gate, unchanged.
    let doc = u16_document(2, 2);
    let other = u16_document(3, 2);
    assert_eq!(
        doc.narrow_structure_preview(Narrowed::Rgba16(&other), LIMIT, 0, None)
            .unwrap_err(),
        "the converted document is not this document's narrowed twin"
    );
}

fn bytes_pattern(w: u32, h: u32, seed: u32, alpha: bool) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let a = if alpha {
            ((x * 23 + y * 41 + seed * 7) % 256) as u8
        } else {
            255
        };
        Rgba([
            ((x * 13 + seed * 29) % 256) as u8,
            ((y * 17 + seed * 11) % 256) as u8,
            ((x + y + seed * 5) * 9 % 256) as u8,
            a,
        ])
    })
}

#[test]
fn record_promote_preview() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let one = DrDocument::from_pixels(bytes_pattern(w, h, 1, true));
        let mut two = DrDocument::from_pixels(bytes_pattern(w, h, 2, true));
        let mut top = DrDocument::from_pixels(bytes_pattern(w, h, 3, true))
            .layers
            .remove(0);
        top.name = "Top".into();
        top.opacity = 0.5;
        two.layers.push(top);
        for (label, doc) in [("one_layer", one), ("two_layers", two)] {
            let promoted = doc
                .promote_structure(LIMIT, 0, None)
                .expect("the promotion");
            let mut line = Line::default();
            let value = line.add(recorded(|c| {
                doc.promote_structure_preview(&promoted, LIMIT, 0, Some(c))
                    .unwrap()
            }));
            let before = widened(&doc.flattened().unwrap());
            let after = promoted
                .render(LIMIT, 0, None)
                .unwrap()
                .to_rgba16()
                .unwrap();
            let oracle = preview_oracle(&before, &after);
            assert_eq!(value, oracle, "{label} {w}×{h}");
            if label == "one_layer" {
                assert_eq!(value["max_delta"], 0, "{label} {w}×{h}");
            }
            line.answers.extend(value.to_string().bytes());
            line.print(&format!("promote_preview/{label}"), (w, h));
        }
    }
}

fn builtin_bytes(which: std::ffi::c_int) -> Vec<u8> {
    let len = dr_builtin_profile_len(which);
    let mut out = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(which, out.as_mut_ptr(), len) });
    out
}

/// An opaque u16 raster plus a `precision_v1` invert adjustment; `hole`
/// sets one alpha to 65534.
fn bake_document(hole: Option<(u32, u32)>) -> PrecisionDocument {
    let mut samples = pattern16(W, H, 7);
    for p in samples.iter_mut() {
        p[3] = u16::MAX;
    }
    if let Some((x, y)) = hole {
        samples[(y * W + x) as usize][3] = 65534;
    }
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("Base", image16(W, H, samples), (0, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    doc.add_adjustment(
        "Invert",
        &json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string(),
        1.0,
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}

/// The preflight's JSON, or its refusal.
fn preflight(doc: &PrecisionDocument, profile: &[u8], options: &CStr) -> Result<Value, String> {
    let mut err: *mut c_char = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_conversion_preflight(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    if result.is_null() {
        return Err(take(err));
    }
    Ok(owned_json(result, err))
}

/// The conversion's document and report, or its refusal.
fn convert(
    doc: &PrecisionDocument,
    profile: &[u8],
    options: &CStr,
    control: &DrJobControl,
) -> Result<(PrecisionDocument, Value), String> {
    let mut err: *mut c_char = ptr::null_mut();
    let mut report: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile_with(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            control,
            &mut report,
            &mut err,
        )
    };
    if out.is_null() {
        return Err(take(err));
    }
    let report = if report.is_null() {
        Value::Null
    } else {
        owned_json(report, ptr::null_mut())
    };
    Ok((*unsafe { Box::from_raw(out) }, report))
}

#[test]
fn record_bake() {
    let _serial = serial();
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);
    let options = CString::new(r#"{"adjustments":"preserve_appearance"}"#).unwrap();
    let doc = bake_document(None);
    let mut line = Line::default();
    let plan = preflight(&doc, &p3, &options).expect("the preflight");
    let row = plan["adjustments"]
        .as_array()
        .expect("adjustment rows")
        .iter()
        .find(|row| row["name"] == "Invert")
        .expect("the Invert row")
        .clone();
    assert_eq!(row["action"], "baked", "{plan}");
    line.answers.extend(plan.to_string().bytes());
    let (converted, report) = line
        .add(recorded(|c| convert(&doc, &p3, &options, c)))
        .expect("the conversion bakes");
    line.answers.extend(report.to_string().bytes());
    let projection = converted.render(LIMIT, 0, None).unwrap();
    for p in projection.to_rgba16().unwrap() {
        line.answers.extend(p.iter().flat_map(|c| c.to_le_bytes()));
    }
    line.print("bake", (W, H));

    for hole in [(256, 256), (599, 519)] {
        let doc = bake_document(Some(hole));
        let sentence =
            "\"Invert\" sits over transparent pixels, which preserve-appearance cannot bake";
        assert_eq!(
            preflight(&doc, &p3, &options).unwrap_err(),
            sentence,
            "preflight, hole at {hole:?}"
        );
        let mut line = Line::default();
        let refusal = line
            .add(recorded(|c| convert(&doc, &p3, &options, c)))
            .map(|_| ())
            .unwrap_err();
        assert_eq!(refusal, sentence, "conversion, hole at {hole:?}");
        line.answers.extend(refusal.bytes());
        line.print(&format!("bake_refusal/{}_{}", hole.0, hole.1), (W, H));
    }
}

// ------------------------------------------------------------ no_view_* --

#[test]
fn no_view_copy_layer() {
    let _serial = serial();
    for (depth, doc) in [
        ("u16", u16_document(W, H)),
        ("f32", float_multi_document(W, H)),
    ] {
        let index = (0..doc.layer_count())
            .find(|&i| doc.layer(i).unwrap().name() == "Seam")
            .expect("a Seam layer");
        let read = copy_request(
            json!({"op":"copy","layer":index,"x":100,"y":120,"width":300,"height":200}),
        );
        evict();
        assert_eq!(document_views(&[&doc]), 0, "{depth}: evicted");
        let payload = doc.clipboard_copy(&read, None, LIMIT, 0, None).unwrap();
        assert_eq!(
            document_views(&[&doc]),
            0,
            "{depth}: Copy binds no view over the document"
        );
        drop(payload);
    }
}

#[test]
fn no_view_scene_render() {
    let _serial = serial();
    let layers = vec![
        PrecisionLayer::raster(image16(W, H, pattern16(W, H, 1)), (0, 0), 1.0).unwrap(),
        PrecisionLayer::raster(image16(390, 260, pattern16(390, 260, 3)), (130, 140), 0.7).unwrap(),
    ];
    let scene = PrecisionScene::new(W, H, IccProfile::srgb(), layers, None, false).unwrap();
    for storage in [PrecisionStorage::Rgba16Unorm, PrecisionStorage::Rgba32Float] {
        evict();
        assert_eq!(contiguous_view_bytes(), 0, "evicted");
        let output = scene.render(storage, LIMIT, 0, None).unwrap();
        assert_eq!(
            contiguous_view_bytes(),
            0,
            "PrecisionScene::render seeds no view ({storage:?})"
        );
        drop(output);
    }
}
