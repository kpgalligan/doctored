//! Black-box suite for 13B-3c-2a: the consumers of a typed projection handle
//! read the tiles — byte quantization (`to_rgba8` / `quantize`), the u16
//! histogram and sample, the high-depth compare, the typed soft proof, the
//! scoped Layer-source analysis, the float display (`display_rgba8`, through
//! `tone_mapped_u16`) and the float sample; the float scoped histogram is
//! covered by `float_scoped_statistics.rs` and pinned view-free here.
//!
//! Oracles are independent of the implementation: every expected answer is
//! computed HERE from the row-major `Vec` a fixture was built from (the rules
//! written out: nearest `(v + 128) / 257`, the ordered 8×8 Bayer rule with
//! its table copied, the 257-wide display bins), and
//! the pointwise answers (`display_rgba8`, the proof) at seam and corner
//! positions equal a 1×1 image's answer for that one sample.
//!
//! Every `record_*` test also prints one line per consumer and fixture —
//! `consumer <label> <WxH> <sha256> checkpoints=<n> reports=<sha256>` — the
//! answer's hash, the job's checkpoint count and the hash of its report log,
//! so the same file run against HEAD and the tree diffs byte-identical
//! answers and checkpoint/report sequences. It uses only APIs present at
//! `656a0aa`.
//!
//! `no_view_*` tests: after an eviction every consumer leaves
//! `contiguous_view_bytes() == 0` (they fail at `656a0aa` by design). The
//! view cache is process-wide, so every test holds `serial()`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_evaluation::dr_precision_image_compare;
use doctored_core::ffi_output_color::dr_precision_proof_render;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use doctored_core::scoped_statistics::AnalysisRequest;
use doctored_core::tiff_float::FloatTiff;
use doctored_core::DrImage;
use serde_json::{json, Value};

use crate::common::sha256;
use crate::resource_report;
use crate::serial;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// 600×520 crosses both tile seams (x 256, 512; y 256, 512).
const FIXTURES: [(u32, u32); 3] = [(600, 520), (257, 3), (1, 1)];

// ------------------------------------------------------------- plumbing --

/// u16 codes with exact 0 / 65535 channels, whole-black and whole-white RGB,
/// alpha 0 (hidden RGB) and alpha 65535.
fn codes16(width: u32, height: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let mut p = [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                (9000 + (x * 3001 + y * 1777 + seed * 97) % 56536) as u16,
            ];
            if (x + y) % 13 == 0 {
                p[0] = 0;
            }
            if (x * 3 + y) % 17 == 1 {
                p[1] = 65535;
            }
            if (x + y) % 29 == 5 {
                p[0] = 0;
                p[1] = 0;
                p[2] = 0;
            }
            if (x * 5 + y) % 31 == 2 {
                p[0] = 65535;
                p[1] = 65535;
                p[2] = 65535;
            }
            match (x + 2 * y) % 11 {
                0 => p[3] = 0,
                1 => p[3] = 65535,
                _ => {}
            }
            p
        })
        .collect()
}

/// Extended-range float samples (interleaved RGBA, straight alpha): exact
/// zeros, negatives, above-white RGB, alpha 0 and alpha 1.
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

/// A float document holding one raster of `samples`, through a float TIFF
/// opened with `open_preserving` (`precision_image_reads`' helper).
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
    let path = directory.path().join("consumers.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// The float raster of a one-layer float document, checked to hold exactly
/// the TIFF samples (so the samples are the oracle's input).
fn float_raster(doc: &PrecisionDocument, samples: &[[f32; 4]]) -> PrecisionImage {
    let image = doc
        .layer(0)
        .and_then(|layer| layer.source())
        .expect("one float raster")
        .clone();
    let held: Vec<[u32; 4]> = image
        .to_rgba32()
        .unwrap()
        .into_iter()
        .map(|p| p.map(f32::to_bits))
        .collect();
    let wanted: Vec<[u32; 4]> = samples.iter().map(|p| p.map(f32::to_bits)).collect();
    assert_eq!(held, wanted, "the raster holds the TIFF samples");
    image
}

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    text
}

fn owned_string(result: *mut c_char, error: *mut c_char) -> Result<String, String> {
    if result.is_null() {
        return Err(take(error));
    }
    assert!(error.is_null());
    let text = unsafe { CStr::from_ptr(result) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(result) };
    Ok(text)
}

fn image_bytes(image: *mut DrImage) -> (u32, u32, Vec<u8>) {
    assert!(!image.is_null());
    let (w, h) = unsafe { (dr_image_width(image), dr_image_height(image)) };
    let bytes = unsafe {
        std::slice::from_raw_parts(dr_image_pixels_rgba(image), w as usize * h as usize * 4)
    }
    .to_vec();
    unsafe { dr_image_free(image) };
    (w, h, bytes)
}

fn evict() {
    resource_report::evict(&[]).expect("an eviction report");
}

/// One job's checkpoint count and the sha256 of its report log.
struct Recorded {
    checkpoints: u64,
    reports: String,
}

/// Runs `call` under a fresh recording `DrJobControl`.
fn recorded<T>(call: impl FnOnce(&DrJobControl) -> T) -> (T, Recorded) {
    let control = DrJobControl::default();
    control.record_reports();
    let out = call(&control);
    let log: String = control
        .reports()
        .iter()
        .map(|r| format!("{} {} {}\n", r.checkpoints, r.progress.to_bits(), r.stage))
        .collect();
    let recorded = Recorded {
        checkpoints: control.checkpoints(),
        reports: sha256::sha256_hex(log.as_bytes()),
    };
    (out, recorded)
}

/// A consumer without a job: no checkpoint, no report.
fn unrecorded() -> Recorded {
    Recorded {
        checkpoints: 0,
        reports: sha256::sha256_hex(b""),
    }
}

fn print_line(label: &str, (w, h): (u32, u32), answer: &[u8], recorded: &Recorded) {
    println!(
        "consumer {label} {w}x{h} {} checkpoints={} reports={}",
        sha256::sha256_hex(answer),
        recorded.checkpoints,
        recorded.reports
    );
}

/// ~40 seam / corner positions inside a `w`×`h` image.
fn positions(w: u32, h: u32) -> Vec<(u32, u32)> {
    let xs = [0, 1, 255, 256, 257, 511, 512, w.saturating_sub(2), w - 1];
    let ys = [0, 1, 255, 256, 257, 511, h.saturating_sub(2), h - 1];
    let mut out = Vec::new();
    for y in ys {
        for x in xs {
            if x < w && y < h && !out.contains(&(x, y)) {
                out.push((x, y));
            }
        }
    }
    out
}

// ------------------------------------------------------ quantize oracles --

const BAYER: [u32; 64] = [
    0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26, 12, 44, 4, 36, 14, 46, 6, 38, 60,
    28, 52, 20, 62, 30, 54, 22, 3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49, 17, 57, 25, 15,
    47, 7, 39, 13, 45, 5, 37, 63, 31, 55, 23, 61, 29, 53, 21,
];

fn nearest(v: u16) -> u8 {
    ((u32::from(v) + 128) / 257) as u8
}

/// None: every channel to the nearest byte. Ordered: RGB `floor(v / 257 +
/// (rank + 0.5) / 64)` clamped to 255 with rank the canvas-anchored Bayer
/// entry at `(x & 7, y & 7)`; alpha to the nearest byte.
fn quantized(source: &[[u16; 4]], width: u32, ordered: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(source.len() * 4);
    for (i, p) in source.iter().enumerate() {
        let (x, y) = (i % width as usize, i / width as usize);
        let rank = BAYER[(y & 7) * 8 + (x & 7)];
        for v in &p[..3] {
            out.push(if ordered {
                ((u32::from(*v) * 128 + (2 * rank + 1) * 257) / (257 * 128)).min(255) as u8
            } else {
                nearest(*v)
            });
        }
        out.push(nearest(p[3]));
    }
    out
}

#[test]
fn record_quantize_and_preview() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let source = codes16(w, h, 1);
        let image = image16(w, h, source.clone());
        let (preview, rec) = recorded(|c| image.to_rgba8(LIMIT, 0, Some(c)).unwrap());
        assert_eq!(preview.dimensions(), (w, h));
        let preview = preview.into_raw();
        assert_eq!(preview, quantized(&source, w, false), "{w}×{h} preview");
        print_line("preview_rgba8", (w, h), &preview, &rec);
        for (dither, ordered, label) in [
            (PrecisionDither::None, false, "quantize_none"),
            (PrecisionDither::Ordered, true, "quantize_ordered"),
        ] {
            let (out, rec) = recorded(|c| image.quantize(dither, LIMIT, 0, Some(c)).unwrap());
            let out = out.into_raw();
            assert_eq!(out, quantized(&source, w, ordered), "{w}×{h} {label}");
            print_line(label, (w, h), &out, &rec);
        }
        assert_eq!(
            image.quantize_scratch_bytes().unwrap(),
            1024 * 1024 + u64::from(w) * u64::from(h) * 4
        );
    }
    let float = image32_unit();
    assert_eq!(
        float.quantize_scratch_bytes().unwrap_err(),
        "byte quantization requires rgba16_unorm"
    );
    assert!(float
        .quantize(PrecisionDither::None, LIMIT, 0, None)
        .unwrap_err()
        .contains("byte quantization requires rgba16_unorm"));
}

fn image32_unit() -> PrecisionImage {
    PrecisionImage::from_rgba32(
        2,
        2,
        vec![[0.25, 0.5, 0.75, 1.0]; 4],
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

// ----------------------------------------------------- histogram oracle --

fn display_bin(v: u16) -> usize {
    ((u32::from(v) + 128) / 257) as usize
}

/// The u16 histogram's rules written out: positions `0, s, 2s, …`; alpha
/// counts every sampled position; RGB / luma only alpha > 0; luma is the
/// Rec.709 f32 coefficients over f64 codes, `round(luma / 257)` clamped.
fn histogram16(source: &[[u16; 4]], stride: usize) -> Value {
    let mut bins = vec![vec![0u64; 256]; 4];
    let mut alpha_bins = vec![0u64; 256];
    let (mut zero, mut maximum) = ([0u64; 4], [0u64; 4]);
    let (mut total, mut sampled, mut luma_zero, mut luma_max) = (0u64, 0u64, 0u64, 0u64);
    for p in source.iter().step_by(stride) {
        sampled += 1;
        alpha_bins[display_bin(p[3])] += 1;
        zero[3] += u64::from(p[3] == 0);
        maximum[3] += u64::from(p[3] == 65535);
        if p[3] == 0 {
            continue;
        }
        total += 1;
        for c in 0..3 {
            bins[c][display_bin(p[c])] += 1;
            zero[c] += u64::from(p[c] == 0);
            maximum[c] += u64::from(p[c] == 65535);
        }
        let luma = f64::from(0.2126_f32) * f64::from(p[0])
            + f64::from(0.7152_f32) * f64::from(p[1])
            + f64::from(0.0722_f32) * f64::from(p[2]);
        bins[3][(luma / 257.0).round().clamp(0.0, 255.0) as usize] += 1;
        luma_zero += u64::from(p[..3].iter().all(|v| *v == 0));
        luma_max += u64::from(p[..3].iter().all(|v| *v == 65535));
    }
    json!({"bins":bins,"alpha_bins":alpha_bins,"total":total,"sampled_pixels":sampled,
        "exact_zero_rgba":zero,"exact_max_rgba":maximum,
        "exact_zero_luma":luma_zero,"exact_max_luma":luma_max,"stride":stride})
}

fn pick(value: &Value, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .map(|k| ((*k).to_string(), value[*k].clone()))
            .collect(),
    )
}

#[test]
fn record_histogram() {
    let _serial = serial();
    let keys = [
        "bins",
        "alpha_bins",
        "total",
        "sampled_pixels",
        "exact_zero_rgba",
        "exact_max_rgba",
        "exact_zero_luma",
        "exact_max_luma",
        "stride",
    ];
    for (w, h) in FIXTURES {
        let source = codes16(w, h, 2);
        let image = image16(w, h, source.clone());
        let mut answers = String::new();
        let mut log = String::new();
        let mut checkpoints = 0;
        for stride in [1u32, 7, 4097] {
            let (value, rec) =
                recorded(|c| image.histogram_checked(stride, LIMIT, 0, Some(c)).unwrap());
            assert_eq!(
                pick(&value, &keys),
                histogram16(&source, stride as usize),
                "{w}×{h} stride {stride}"
            );
            answers.push_str(&value.to_string());
            log.push_str(&rec.reports);
            checkpoints += rec.checkpoints;
        }
        let rec = Recorded {
            checkpoints,
            reports: sha256::sha256_hex(log.as_bytes()),
        };
        print_line("histogram", (w, h), answers.as_bytes(), &rec);
    }
    let error = image32_unit()
        .histogram_checked(1, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap_err();
    assert_eq!(error, "precision histogram requires rgba16_unorm");
}

// -------------------------------------------------------- sample oracle --

/// Probe centres: corners, seams, and positions partly and wholly off-image.
fn probes(w: u32, h: u32) -> Vec<(i32, i32)> {
    let (w, h) = (w as i32, h as i32);
    vec![
        (0, 0),
        (w - 1, 0),
        (0, h - 1),
        (w - 1, h - 1),
        (255.min(w - 1), 256.min(h - 1)),
        (256.min(w - 1), 255.min(h - 1)),
        (w / 2, h / 2),
        (-1, -1),
        (w, h),
        (-2, 0),
        (w + 1, h - 1),
        (-3, -3),
        (w + 2, h + 2),
    ]
}

#[test]
fn record_sample() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let source = codes16(w, h, 3);
        let image = image16(w, h, source.clone());
        let at = |x: i64, y: i64| source[y as usize * w as usize + x as usize];
        let inside = |x: i64, y: i64| x >= 0 && y >= 0 && x < i64::from(w) && y < i64::from(h);
        let mut answers = String::new();
        for (x, y) in probes(w, h) {
            for reach in 0..=2u32 {
                let r = i64::from(reach);
                let (mut sum, mut count) = ([0u64; 4], 0u64);
                for py in i64::from(y) - r..=i64::from(y) + r {
                    for px in i64::from(x) - r..=i64::from(x) + r {
                        if inside(px, py) {
                            for (slot, code) in sum.iter_mut().zip(at(px, py)) {
                                *slot += u64::from(code);
                            }
                            count += 1;
                        }
                    }
                }
                let answer = image.sample(x, y, reach);
                if count == 0 {
                    assert_eq!(
                        answer.as_ref().unwrap_err(),
                        "precision sample is outside the image",
                        "{w}×{h} ({x}, {y}) reach {reach}"
                    );
                    answers.push_str(answer.as_ref().unwrap_err());
                    continue;
                }
                let value = answer.unwrap();
                let center =
                    inside(i64::from(x), i64::from(y)).then(|| at(i64::from(x), i64::from(y)));
                assert_eq!(
                    pick(&value, &["sum_rgba16", "sample_count", "center_rgba16"]),
                    json!({"sum_rgba16":sum,"sample_count":count,"center_rgba16":center}),
                    "{w}×{h} ({x}, {y}) reach {reach}"
                );
                answers.push_str(&value.to_string());
            }
        }
        print_line("sample", (w, h), answers.as_bytes(), &unrecorded());
    }
    let image = image16(2, 2, codes16(2, 2, 3));
    assert_eq!(
        image.sample(0, 0, 3).unwrap_err(),
        "precision sample reach must be 0, 1 or 2"
    );
    assert_eq!(
        image32_unit().sample(0, 0, 3).unwrap_err(),
        "precision sampling requires rgba16_unorm",
        "the storage gate precedes the reach check"
    );
}

// ------------------------------------------------------- compare oracle --

fn compare(a: &PrecisionImage, b: &PrecisionImage) -> Result<String, String> {
    let mut error = ptr::null_mut();
    let result = unsafe { dr_precision_image_compare(a, b, &mut error) };
    owned_string(result, error)
}

#[test]
fn record_compare() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let (left, right) = (codes16(w, h, 4), codes16(w, h, 5));
        let (a, b) = (image16(w, h, left.clone()), image16(w, h, right.clone()));
        let mut answers = String::new();
        for (x, y, xs, ys) in [(&a, &b, &left, &right), (&a, &a, &left, &left)] {
            let (mut count, mut changed, mut total, mut max) = (0u64, 0u64, 0u64, 0u32);
            for (p, q) in xs.iter().zip(ys.iter()) {
                for c in 0..4 {
                    let d = u32::from(p[c].abs_diff(q[c]));
                    count += 1;
                    total += u64::from(d);
                    changed += u64::from(d != 0);
                    max = max.max(d);
                }
            }
            let text = compare(x, y).unwrap();
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                pick(
                    &value,
                    &[
                        "depth",
                        "width",
                        "height",
                        "max_abs",
                        "mean_abs",
                        "changed_fraction"
                    ]
                ),
                json!({"depth":16,"width":w,"height":h,"max_abs":max,
                    "mean_abs":total as f64 / count as f64,
                    "changed_fraction":changed as f64 / count as f64}),
                "{w}×{h}"
            );
            answers.push_str(&text);
        }
        print_line("compare", (w, h), answers.as_bytes(), &unrecorded());
    }
    let a = image16(3, 2, codes16(3, 2, 1));
    let b = image16(2, 3, codes16(2, 3, 1));
    assert!(compare(&a, &b)
        .unwrap_err()
        .contains("compared images have different dimensions"));
    let float = image32_unit();
    let small = image16(2, 2, codes16(2, 2, 1));
    assert!(compare(&small, &float)
        .unwrap_err()
        .contains("comparing high-depth images requires RGBA16 UNORM storage"));
}

// --------------------------------------------------------- the soft proof --

const FOGRA: &str =
    "/Library/Application Support/Adobe/Color/Profiles/Recommended/CoatedFOGRA39.icc";

/// FOGRA39 when installed, else sRGB.
fn proof_profile() -> Vec<u8> {
    std::fs::read(FOGRA).unwrap_or_else(|_| IccProfile::srgb().bytes().to_vec())
}

fn proof(
    image: &PrecisionImage,
    profile: &[u8],
    control: Option<&DrJobControl>,
) -> (u32, u32, Vec<u8>, String) {
    let setup = CString::new(json!({"gamut_warning":true}).to_string()).unwrap();
    let mut info = ptr::null_mut();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_proof_render(
            image,
            profile.as_ptr(),
            profile.len(),
            setup.as_ptr(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c as *const _),
            &mut info,
            &mut error,
        )
    };
    assert!(!out.is_null(), "proof: {}", take(error));
    let info = owned_string(info, ptr::null_mut()).expect("proof info");
    let (w, h, bytes) = image_bytes(out);
    (w, h, bytes, info)
}

#[test]
fn record_proof_render() {
    let _serial = serial();
    let profile = proof_profile();
    for (w, h) in FIXTURES {
        let source = codes16(w, h, 6);
        let image = image16(w, h, source.clone());
        let ((pw, ph, bytes, info), rec) = recorded(|c| proof(&image, &profile, Some(c)));
        assert_eq!((pw, ph), (w, h));
        for (x, y) in positions(w, h) {
            let one = image16(1, 1, vec![source[(y * w + x) as usize]]);
            let (_, _, alone, _) = proof(&one, &profile, None);
            let at = (y * w + x) as usize * 4;
            assert_eq!(bytes[at..at + 4], alone[..], "{w}×{h} ({x}, {y})");
        }
        let mut answer = bytes;
        answer.extend_from_slice(info.as_bytes());
        print_line("proof_render", (w, h), &answer, &rec);
    }
}

// ------------------------------------------------ scoped Layer histogram --

/// The scoped fixture: the document, raster 1's codes, its size and offset.
type ScopedFixture = (PrecisionDocument, Vec<[u16; 4]>, (u32, u32), (i32, i32));

/// A `w`×`h` canvas over a base raster, with raster 1 of `lw`×`lh` at
/// `offset`, partly off the canvas.
fn scoped_fixture((w, h): (u32, u32)) -> ScopedFixture {
    let (lw, lh, offset) = match (w, h) {
        (600, 520) => (300, 200, (-100, 400)),
        (257, 3) => (100, 2, (200, 2)),
        _ => (2, 2, (-1, -1)),
    };
    let layer = codes16(lw, lh, 8);
    let layers = vec![
        Layer::raster("Base", image16(w, h, codes16(w, h, 7)), (0, 0), 1.0, true).unwrap(),
        Layer::raster("Off", image16(lw, lh, layer.clone()), offset, 0.8, true).unwrap(),
    ];
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    (doc, layer, (lw, lh), offset)
}

#[test]
fn record_scoped_layer_histogram() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let (doc, layer, (lw, lh), (ox, oy)) = scoped_fixture((w, h));
        let read = |i: usize| {
            let x = (i % w as usize) as i64 - i64::from(ox);
            let y = (i / w as usize) as i64 - i64::from(oy);
            if x < 0 || y < 0 || x >= i64::from(lw) || y >= i64::from(lh) {
                [0u16; 4]
            } else {
                layer[y as usize * lw as usize + x as usize]
            }
        };
        let n = w as usize * h as usize;
        let mut answers = String::new();
        let mut log = String::new();
        let mut checkpoints = 0;
        for stride in [1u32, 7] {
            let (mut sums, mut zero, mut maximum) = ([0u64; 4], [0u64; 4], [0u64; 4]);
            let (mut rgb, mut sampled) = (0u64, 0u64);
            for i in 0..n {
                let p = read(i);
                sampled += u64::from(i % stride as usize == 0);
                sums[3] += u64::from(p[3]) * 65535;
                zero[3] += u64::from(p[3] == 0);
                maximum[3] += u64::from(p[3] == 65535);
                if p[3] == 0 {
                    continue;
                }
                rgb += 1;
                for c in 0..3 {
                    sums[c] += u64::from(p[c]) * 65535;
                    zero[c] += u64::from(p[c] == 0);
                    maximum[c] += u64::from(p[c] == 65535);
                }
            }
            let request = AnalysisRequest::parse(
                &json!({"operation":"histogram","source":"layer","layer":1,"stride":stride})
                    .to_string(),
            )
            .unwrap();
            let (value, rec) = recorded(|c| {
                doc.scoped_analysis(&request, &[], LIMIT, 0, Some(c))
                    .unwrap()
            });
            assert_eq!(
                pick(
                    &value,
                    &[
                        "weighted_code_sums_rgba",
                        "exact_zero_rgba",
                        "exact_max_rgba",
                        "full_rgb_pixel_count",
                        "full_alpha_weight_units",
                        "sampled_pixels",
                    ]
                ),
                json!({"weighted_code_sums_rgba":sums,"exact_zero_rgba":zero,
                    "exact_max_rgba":maximum,"full_rgb_pixel_count":rgb,
                    "full_alpha_weight_units":n as u64 * 65535,"sampled_pixels":sampled}),
                "{w}×{h} stride {stride}"
            );
            answers.push_str(&value.to_string());
            log.push_str(&rec.reports);
            checkpoints += rec.checkpoints;
        }
        // A sample at the layer's canvas corner (partly off the layer).
        let (cx, cy) = (ox.max(0), oy.max(0));
        let request = AnalysisRequest::parse(
            &json!({"operation":"sample","source":"layer","layer":1,"x":cx,"y":cy,"reach":1})
                .to_string(),
        )
        .unwrap();
        let (value, rec) = recorded(|c| {
            doc.scoped_analysis(&request, &[], LIMIT, 0, Some(c))
                .unwrap()
        });
        let mut sums = [0u64; 4];
        let mut count = 0u64;
        for py in i64::from(cy) - 1..=i64::from(cy) + 1 {
            for px in i64::from(cx) - 1..=i64::from(cx) + 1 {
                if px >= 0 && py >= 0 && px < i64::from(w) && py < i64::from(h) {
                    let p = read(py as usize * w as usize + px as usize);
                    for c in 0..4 {
                        sums[c] += u64::from(p[c]) * 65535;
                    }
                    count += 1;
                }
            }
        }
        let center = read(cy as usize * w as usize + cx as usize);
        assert_eq!(
            pick(
                &value,
                &["weighted_sum_rgba_codes", "sample_count", "center_rgba16"]
            ),
            json!({"weighted_sum_rgba_codes":sums,"sample_count":count,"center_rgba16":center}),
            "{w}×{h} sample"
        );
        answers.push_str(&value.to_string());
        log.push_str(&rec.reports);
        checkpoints += rec.checkpoints;
        let rec = Recorded {
            checkpoints,
            reports: sha256::sha256_hex(log.as_bytes()),
        };
        print_line("scoped_layer_histogram", (w, h), answers.as_bytes(), &rec);
    }
}

// ----------------------------------------------------- the float display --

#[test]
fn record_display_rgba8() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let samples = extended(w, h, 1);
        let doc = float_document(w, h, &samples);
        let projection = doc.render(LIMIT, 0, None).expect("a float projection");
        let (out, rec) = recorded(|c| doc.display_rgba8(&projection, LIMIT, 0, Some(c)).unwrap());
        assert_eq!(out.dimensions(), (w, h));
        let bytes = out.into_raw();
        for (x, y) in positions(w, h) {
            let one = float_document(1, 1, &[samples[(y * w + x) as usize]]);
            let alone = one.render(LIMIT, 0, None).unwrap();
            let alone = one
                .display_rgba8(&alone, LIMIT, 0, None)
                .unwrap()
                .into_raw();
            let at = (y * w + x) as usize * 4;
            assert_eq!(bytes[at..at + 4], alone[..], "{w}×{h} ({x}, {y})");
        }
        print_line("display_rgba8", (w, h), &bytes, &rec);
    }
}

// ------------------------------------------------------- float sample --

fn stops(v: f64) -> f64 {
    v.max(2.0_f64.powi(-16)).log2()
}

#[test]
fn record_float_sample() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let samples = extended(w, h, 3);
        let doc = float_document(w, h, &samples);
        let image = float_raster(&doc, &samples);
        let at = |x: i64, y: i64| samples[y as usize * w as usize + x as usize];
        let inside = |x: i64, y: i64| x >= 0 && y >= 0 && x < i64::from(w) && y < i64::from(h);
        let mut answers = String::new();
        for (x, y) in probes(w, h) {
            for reach in 0..=2u32 {
                let r = i64::from(reach);
                let (mut sum, mut count) = ([0f64; 4], 0u64);
                let (mut above, mut below) = ([false; 3], [false; 3]);
                for py in i64::from(y) - r..=i64::from(y) + r {
                    for px in i64::from(x) - r..=i64::from(x) + r {
                        if !inside(px, py) {
                            continue;
                        }
                        let p = at(px, py);
                        for c in 0..4 {
                            sum[c] += f64::from(p[c]);
                        }
                        for c in 0..3 {
                            above[c] |= p[c] > 1.0;
                            below[c] |= p[c] < 0.0;
                        }
                        count += 1;
                    }
                }
                let answer = image.float_sample(x, y, reach);
                if count == 0 {
                    assert_eq!(
                        answer.as_ref().unwrap_err(),
                        "precision sample is outside the image",
                        "{w}×{h} ({x}, {y}) reach {reach}"
                    );
                    answers.push_str(answer.as_ref().unwrap_err());
                    continue;
                }
                let value = answer.unwrap();
                let center =
                    inside(i64::from(x), i64::from(y)).then(|| at(i64::from(x), i64::from(y)));
                let means = sum.map(|v| v / count as f64);
                assert_eq!(
                    pick(
                        &value,
                        &[
                            "center_rgba32",
                            "mean_rgba32",
                            "sample_count",
                            "above_white",
                            "below_zero",
                            "stops"
                        ]
                    ),
                    json!({"center_rgba32":center,"mean_rgba32":means,"sample_count":count,
                        "above_white":above,"below_zero":below,
                        "stops":[stops(means[0]),stops(means[1]),stops(means[2])]}),
                    "{w}×{h} ({x}, {y}) reach {reach}"
                );
                answers.push_str(&value.to_string());
            }
        }
        print_line("float_sample", (w, h), answers.as_bytes(), &unrecorded());
    }
    let small = image16(2, 2, codes16(2, 2, 1));
    assert_eq!(
        small.float_sample(0, 0, 0).unwrap_err(),
        "float sampling requires rgba32_float"
    );
}

// ------------------------------------------------ no consumer binds a view --

const BIG: (u32, u32) = FIXTURES[0];

/// Evicts, runs `call`, and answers the process-wide view bytes after.
fn views_after(call: impl FnOnce()) -> u64 {
    evict();
    assert_eq!(contiguous_view_bytes(), 0, "evicted");
    call();
    contiguous_view_bytes()
}

#[test]
fn no_view_quantize_and_preview() {
    let _serial = serial();
    let image = image16(BIG.0, BIG.1, codes16(BIG.0, BIG.1, 1));
    assert_eq!(
        views_after(|| {
            image.to_rgba8(LIMIT, 0, None).unwrap();
            image
                .quantize(PrecisionDither::Ordered, LIMIT, 0, None)
                .unwrap();
            image.quantize_scratch_bytes().unwrap();
        }),
        0
    );
}

#[test]
fn no_view_histogram_and_sample() {
    let _serial = serial();
    let image = image16(BIG.0, BIG.1, codes16(BIG.0, BIG.1, 2));
    assert_eq!(
        views_after(|| {
            image.histogram_checked(7, LIMIT, 0, None).unwrap();
            image.sample(256, 256, 2).unwrap();
        }),
        0
    );
}

#[test]
fn no_view_compare() {
    let _serial = serial();
    let a = image16(BIG.0, BIG.1, codes16(BIG.0, BIG.1, 4));
    let b = image16(BIG.0, BIG.1, codes16(BIG.0, BIG.1, 5));
    assert_eq!(
        views_after(|| {
            compare(&a, &b).unwrap();
        }),
        0
    );
}

#[test]
fn no_view_proof_render() {
    let _serial = serial();
    let image = image16(BIG.0, BIG.1, codes16(BIG.0, BIG.1, 6));
    let profile = proof_profile();
    assert_eq!(
        views_after(|| {
            proof(&image, &profile, None);
        }),
        0
    );
}

#[test]
fn no_view_scoped_layer_histogram() {
    let _serial = serial();
    let (doc, ..) = scoped_fixture(BIG);
    let request =
        AnalysisRequest::parse(r#"{"operation":"histogram","source":"layer","layer":1}"#).unwrap();
    assert_eq!(
        views_after(|| {
            doc.scoped_analysis(&request, &[], LIMIT, 0, None).unwrap();
        }),
        0
    );
}

#[test]
fn no_view_display_rgba8() {
    let _serial = serial();
    let doc = float_document(BIG.0, BIG.1, &extended(BIG.0, BIG.1, 1));
    let projection = doc.render(LIMIT, 0, None).unwrap();
    assert_eq!(
        views_after(|| {
            doc.display_rgba8(&projection, LIMIT, 0, None).unwrap();
        }),
        0
    );
}

#[test]
fn no_view_float_scoped_histogram_and_sample() {
    let _serial = serial();
    let samples = extended(BIG.0, BIG.1, 2);
    let doc = float_document(BIG.0, BIG.1, &samples);
    let image = float_raster(&doc, &samples);
    let request =
        AnalysisRequest::parse(r#"{"operation":"histogram","source":"composite","stride":7}"#)
            .unwrap();
    assert_eq!(
        views_after(|| {
            image
                .scoped_analysis(&request, &[], LIMIT, 0, None)
                .unwrap();
            image.float_sample(256, 256, 2).unwrap();
        }),
        0
    );
}
