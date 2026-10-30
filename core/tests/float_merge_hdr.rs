//! Merge to HDR through the FFI (`dr_doc_merge_hdr*` at 8 bits,
//! `dr_precision_doc_merge_hdr*` at 16): the exposure-weighted scene-linear
//! merge, the exposure estimate, the ghost test, the refusals and the plan.
//!
//! # The oracles
//!
//! * **The statement** ([`statement`]): the merge and its lattice statistics
//!   written again here in f64, pixel by pixel, from the rule in
//!   `merge_hdr.rs`'s module doc. It reads the same decoded linear samples the
//!   core reads (the document profile's `TypedTransfer::decode` of each stored
//!   code, which is not what is under test) and shares no line with the
//!   kernel. Bound: 1e-5 relative per component.
//! * **The scene**: frames authored from a KNOWN radiance through the analytic
//!   sRGB curve written here, clipped at 1 and quantized, so the merge is
//!   compared with the radiance that made the frames. Radiance `R · (1, 0.9,
//!   0.8)`, `R` from 6.0e-4 to 6.0e-4 · 2^14 across the width; exposures
//!   −4, 0, +4 and −4, −2, 0, +2, +4. Bounds, fixed before any measurement:
//!   1e-3 relative per channel at 16 bits and 0.06 at 8 bits; with Gaussian
//!   noise of sigma 0.001 linear on 29 half-stop bands of 16×64 px, each
//!   band's mean within 0.05 at both depths.
//! * **Ghosts**: no noise, exposures −2, 0, +2, a flat background of 0.08 and
//!   a patch of 0.5 (linear, at the reference exposure) at a different place
//!   in each frame. `0 < deghosted_pixels ≤` the union of the patch's
//!   footprints, and inside the union the merge is the reference. The same
//!   background with no patch and EVs given 0.2 off deghosts nothing.
//!
//! Every document here has a transparent "Canvas" layer at index 0, so frame
//! `k` is layer `k + 1` and a request that confused a layer index with a
//! position would read the wrong pixels.
#![allow(clippy::needless_range_loop)]

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{DrDocument, LayerKind, LayerRaster};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::dr_job_control_set_scratch_limit;
use doctored_core::ffi_merge_hdr::{
    dr_doc_merge_hdr, dr_doc_merge_hdr_exposures, dr_doc_merge_hdr_plan,
    dr_precision_doc_merge_hdr, dr_precision_doc_merge_hdr_exposures,
    dr_precision_doc_merge_hdr_plan,
};
use doctored_core::float_document::ToneMap;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_evaluation::TypedTransfer;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// Full scale of the two depths.
const DEPTHS: [(&str, u32); 2] = [("8-bit", 255), ("16-bit", 65535)];
/// The scene's colour: radiance `R` times this.
const TINT: [f64; 3] = [1.0, 0.9, 0.8];

// ---------------------------------------------------------------- fixtures --

/// One exposure as stored codes at its depth's full scale, straight RGBA.
#[derive(Clone)]
struct Frame {
    name: String,
    size: (u32, u32),
    offset: (i32, i32),
    codes: Vec<[u32; 4]>,
}

impl Frame {
    /// A canvas-sized frame at offset (0, 0) from a per-pixel rule.
    fn filling(name: &str, canvas: (u32, u32), mut rule: impl FnMut(u32, u32) -> [u32; 4]) -> Self {
        let mut codes = Vec::with_capacity((canvas.0 * canvas.1) as usize);
        for y in 0..canvas.1 {
            for x in 0..canvas.0 {
                codes.push(rule(x, y));
            }
        }
        Self {
            name: name.into(),
            size: canvas,
            offset: (0, 0),
            codes,
        }
    }
}

/// The analytic sRGB encode, written here: linear light to an encoded value.
fn srgb_encode(linear: f64) -> f64 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// A linear value clipped at 1 and stored at `full`.
fn stored(linear: f64, full: u32) -> u32 {
    (srgb_encode(linear.clamp(0.0, 1.0)) * f64::from(full)).round() as u32
}

/// An opaque pixel of radiance `rgb` exposed by `ev` stops.
fn exposed(rgb: [f64; 3], ev: f64, full: u32) -> [u32; 4] {
    let gain = ev.exp2();
    [
        stored(rgb[0] * gain, full),
        stored(rgb[1] * gain, full),
        stored(rgb[2] * gain, full),
        full,
    ]
}

fn tinted(radiance: f64) -> [f64; 3] {
    TINT.map(|c| radiance * c)
}

/// The spec scene's radiance at column `x` of `width`: 14 stops across.
fn scene_radiance(x: u32, width: u32) -> f64 {
    6.0e-4 * (14.0 * f64::from(x) / f64::from(width - 1)).exp2()
}

/// A document at one depth behind the three exports.
enum Subject {
    Byte(Box<DrDocument>),
    Typed(Box<PrecisionDocument>),
}

fn byte_image(size: (u32, u32), codes: &[[u32; 4]]) -> RgbaImage {
    RgbaImage::from_fn(size.0, size.1, |x, y| {
        Rgba(codes[(y * size.0 + x) as usize].map(|v| v as u8))
    })
}

fn byte_document(canvas: (u32, u32), frames: &[Frame]) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::new(canvas.0, canvas.1));
    doc.layers[0].name = "Canvas".into();
    for (k, frame) in frames.iter().enumerate() {
        doc = doc
            .adding_image_layer(k, byte_image(frame.size, &frame.codes), &frame.name)
            .expect("the frame's layer");
        doc.layers[k + 1].offset = frame.offset;
    }
    assert_eq!((doc.width, doc.height), canvas);
    assert_eq!(doc.layers.len(), frames.len() + 1);
    doc
}

fn typed_document(canvas: (u32, u32), frames: &[Frame]) -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let image = |size: (u32, u32), codes: Vec<[u16; 4]>| {
        PrecisionImage::from_rgba16(size.0, size.1, codes, profile.clone(), None).unwrap()
    };
    let blank = vec![[0u16; 4]; (canvas.0 * canvas.1) as usize];
    let mut layers =
        vec![
            PrecisionDocumentLayer::raster("Canvas", image(canvas, blank), (0, 0), 1., true)
                .unwrap(),
        ];
    for frame in frames {
        let codes = frame.codes.iter().map(|p| p.map(|v| v as u16)).collect();
        layers.push(
            PrecisionDocumentLayer::raster(
                &frame.name,
                image(frame.size, codes),
                frame.offset,
                1.,
                true,
            )
            .unwrap(),
        );
    }
    PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        profile,
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn subject(full: u32, canvas: (u32, u32), frames: &[Frame]) -> Subject {
    if full == 255 {
        Subject::Byte(Box::new(byte_document(canvas, frames)))
    } else {
        Subject::Typed(Box::new(typed_document(canvas, frames)))
    }
}

// --------------------------------------------------------------------- ffi --

/// Takes an owned C string (NULL = `None`) and frees it.
fn taken(pointer: *mut c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    Some(text)
}

fn control_pointer(control: Option<&DrJobControl>) -> *const DrJobControl {
    control.map_or(ptr::null(), |c| c as *const DrJobControl)
}

impl Subject {
    fn exposures(&self, request: &Value, control: Option<&DrJobControl>) -> Result<Value, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut error: *mut c_char = ptr::null_mut();
        let control = control_pointer(control);
        let out = unsafe {
            match self {
                Subject::Byte(doc) => {
                    dr_doc_merge_hdr_exposures(&**doc, text.as_ptr(), control, &mut error)
                }
                Subject::Typed(doc) => {
                    dr_precision_doc_merge_hdr_exposures(&**doc, text.as_ptr(), control, &mut error)
                }
            }
        };
        match taken(out) {
            Some(reply) => {
                assert!(error.is_null(), "a reply comes with no error");
                Ok(serde_json::from_str(&reply).expect("reply JSON"))
            }
            None => Err(taken(error).expect("a NULL reply names its reason")),
        }
    }

    fn plan(&self, request: &Value) -> Result<Value, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut error: *mut c_char = ptr::null_mut();
        let out = unsafe {
            match self {
                Subject::Byte(doc) => dr_doc_merge_hdr_plan(&**doc, text.as_ptr(), &mut error),
                Subject::Typed(doc) => {
                    dr_precision_doc_merge_hdr_plan(&**doc, text.as_ptr(), &mut error)
                }
            }
        };
        match taken(out) {
            Some(reply) => Ok(serde_json::from_str(&reply).expect("plan JSON")),
            None => Err(taken(error).expect("a NULL plan names its reason")),
        }
    }

    fn merge_at(
        &self,
        request: &Value,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<(PrecisionDocument, Value), String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut error: *mut c_char = ptr::null_mut();
        let mut report: *mut c_char = ptr::null_mut();
        let control = control_pointer(control);
        let out = unsafe {
            match self {
                Subject::Byte(doc) => dr_doc_merge_hdr(
                    &**doc,
                    text.as_ptr(),
                    limit,
                    other,
                    control,
                    &mut report,
                    &mut error,
                ),
                Subject::Typed(doc) => dr_precision_doc_merge_hdr(
                    &**doc,
                    text.as_ptr(),
                    limit,
                    other,
                    control,
                    &mut report,
                    &mut error,
                ),
            }
        };
        if out.is_null() {
            assert!(report.is_null(), "a refusal writes no report");
            return Err(taken(error).expect("a NULL document names its reason"));
        }
        assert!(error.is_null(), "a document comes with no error");
        let report = taken(report).expect("a document comes with its report");
        Ok((
            *unsafe { Box::from_raw(out) },
            serde_json::from_str(&report).expect("report JSON"),
        ))
    }

    fn merge(&self, request: &Value) -> (PrecisionDocument, Value) {
        match self.merge_at(request, LIMIT, 0, None) {
            Ok(out) => out,
            Err(reason) => panic!("{request} was refused: {reason}"),
        }
    }

    fn refusal(&self, request: &Value) -> String {
        self.refused_at(request, LIMIT, 0, None)
    }

    fn refused_at(
        &self,
        request: &Value,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> String {
        match self.merge_at(request, limit, other, control) {
            Ok(_) => panic!("{request} was not refused"),
            Err(reason) => reason,
        }
    }
}

/// The request for frames `0..evs.len()` (layers `1..`), reference frame
/// `reference`.
fn request(evs: &[f64], reference: usize, deghost: bool) -> Value {
    json!({
        "version": 1,
        "layers": (1..=evs.len()).collect::<Vec<_>>(),
        "evs": evs,
        "reference": reference + 1,
        "deghost": deghost,
    })
}

fn merged_pixels(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.layer_source(0)
        .expect("the merged raster")
        .to_rgba32()
        .ok()
        .unwrap()
}

// ----------------------------------------------------------- the statement --

/// One frame's sample at a canvas pixel as the adapter contract states it.
#[derive(Clone, Copy)]
struct Seen {
    linear: [f64; 3],
    level: f64,
    alpha: f64,
}

/// Every frame on the canvas grid: `None` outside the frame's rectangle.
fn seen(full: u32, canvas: (u32, u32), frames: &[Frame]) -> Vec<Vec<Option<Seen>>> {
    let transfer = TypedTransfer::for_profile(&IccProfile::srgb()).unwrap();
    let widen = 65535 / full;
    frames
        .iter()
        .map(|frame| {
            let mut out = vec![None; (canvas.0 * canvas.1) as usize];
            for y in 0..canvas.1 as i64 {
                for x in 0..canvas.0 as i64 {
                    let (lx, ly) = (x - i64::from(frame.offset.0), y - i64::from(frame.offset.1));
                    if lx < 0
                        || ly < 0
                        || lx >= i64::from(frame.size.0)
                        || ly >= i64::from(frame.size.1)
                    {
                        continue;
                    }
                    let code = frame.codes[(ly * i64::from(frame.size.0) + lx) as usize];
                    let encoded = [code[0], code[1], code[2]].map(|v| (v * widen) as f32 / 65535.0);
                    out[(y * i64::from(canvas.0) + x) as usize] = Some(Seen {
                        linear: transfer.decode(encoded).map(f64::from),
                        level: f64::from(code[0].max(code[1]).max(code[2])) / f64::from(full),
                        alpha: f64::from(code[3]) / f64::from(full),
                    });
                }
            }
            out
        })
        .collect()
}

/// The weight of a level: 0 at or below 0.02 and at or above 0.98, 1 on
/// [0.10, 0.90], linear between.
fn trust(level: f64) -> f64 {
    if level <= 0.02 || level >= 0.98 {
        0.0
    } else if level < 0.10 {
        (level - 0.02) / 0.08
    } else if level <= 0.90 {
        1.0
    } else {
        (0.98 - level) / 0.08
    }
}

fn middle(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

/// A sample that counts toward the lattice statistics: opaque, fully trusted.
fn sound(sample: Option<Seen>) -> Option<f64> {
    let sample = sample?;
    (sample.alpha == 1.0 && trust(sample.level) == 1.0)
        .then(|| (sample.linear[0] + sample.linear[1] + sample.linear[2]) / 3.0)
}

/// The lattice's pixel indices: every `stride`-th row and column from 0.
fn lattice(canvas: (u32, u32)) -> Vec<usize> {
    let area = f64::from(canvas.0) * f64::from(canvas.1);
    let stride = ((area / 262_144.0).sqrt().ceil() as usize).max(1);
    let mut out = Vec::new();
    for y in (0..canvas.1 as usize).step_by(stride) {
        for x in (0..canvas.0 as usize).step_by(stride) {
            out.push(y * canvas.0 as usize + x);
        }
    }
    out
}

struct Expected {
    pixels: Vec<[f64; 4]>,
    all_clipped: u64,
    all_dark: u64,
    deghosted: u64,
    sigma: Vec<Option<f64>>,
}

/// The merge as `merge_hdr.rs`'s module doc states it, in f64.
fn statement(
    canvas: (u32, u32),
    frames: &[Vec<Option<Seen>>],
    evs: &[f64],
    reference: usize,
    deghost: bool,
) -> Expected {
    let stops: Vec<f64> = evs.iter().map(|ev| ev - evs[reference]).collect();
    // Each judged frame's (median residual, threshold).
    let mut tests: Vec<Option<(f64, f64)>> = vec![None; frames.len()];
    let mut sigma = vec![None; frames.len()];
    if deghost {
        let points = lattice(canvas);
        for i in (0..frames.len()).filter(|i| *i != reference) {
            let residuals: Vec<f64> = points
                .iter()
                .filter_map(|&p| {
                    let (own, anchor) = (sound(frames[i][p])?, sound(frames[reference][p])?);
                    Some(own.ln() - anchor.ln() - stops[i] * std::f64::consts::LN_2)
                })
                .collect();
            if residuals.len() < 64 {
                continue;
            }
            let centre = middle(residuals.clone());
            let spread = 1.4826 * middle(residuals.iter().map(|r| (r - centre).abs()).collect());
            tests[i] = Some((centre, (4.0 * spread).max(1.1f64.ln())));
            sigma[i] = Some(spread);
        }
    }
    let mut out = Expected {
        pixels: Vec::new(),
        all_clipped: 0,
        all_dark: 0,
        deghosted: 0,
        sigma,
    };
    for p in 0..(canvas.0 * canvas.1) as usize {
        let covering: Vec<(usize, Seen)> = frames
            .iter()
            .enumerate()
            .filter_map(|(i, frame)| frame[p].filter(|s| s.alpha > 0.0).map(|s| (i, s)))
            .collect();
        if covering.is_empty() {
            out.pixels.push([0.0; 4]);
            continue;
        }
        let alpha = covering.iter().map(|(_, s)| s.alpha).fold(0.0, f64::max);
        let radiance = |i: usize, s: &Seen| s.linear.map(|v| v / stops[i].exp2());
        let ghost = sound(frames[reference][p]).is_some_and(|anchor| {
            covering.iter().any(|(i, s)| {
                let (Some((centre, tau)), Some(own)) = (tests[*i], sound(Some(*s))) else {
                    return false;
                };
                let residual = own.ln() - anchor.ln() - stops[*i] * std::f64::consts::LN_2;
                (residual - centre).abs() > tau
            })
        });
        let weights: Vec<f64> = covering
            .iter()
            .map(|(i, s)| s.alpha * trust(s.level) * stops[*i].exp2())
            .collect();
        let total: f64 = weights.iter().sum();
        let rgb = if ghost {
            out.deghosted += 1;
            frames[reference][p].unwrap().linear
        } else if total > 0.0 {
            let mut sum = [0.0; 3];
            for ((i, s), w) in covering.iter().zip(&weights) {
                let own = radiance(*i, s);
                for c in 0..3 {
                    sum[c] += w * own[c];
                }
            }
            sum.map(|v| v / total)
        } else if covering.iter().all(|(_, s)| s.level >= 0.98) {
            out.all_clipped += 1;
            let mut pick = covering[0];
            for candidate in &covering[1..] {
                if stops[candidate.0] < stops[pick.0] {
                    pick = *candidate;
                }
            }
            radiance(pick.0, &pick.1)
        } else {
            out.all_dark += 1;
            let below: Vec<(usize, Seen)> = covering
                .iter()
                .copied()
                .filter(|(_, s)| s.level < 0.98)
                .collect();
            let mut pick = below[0];
            for candidate in &below[1..] {
                if stops[candidate.0] > stops[pick.0] {
                    pick = *candidate;
                }
            }
            radiance(pick.0, &pick.1)
        };
        out.pixels.push([rgb[0], rgb[1], rgb[2], alpha]);
    }
    out
}

fn assert_matches_statement(got: &[[f32; 4]], want: &[[f64; 4]], canvas: (u32, u32), what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: pixel count");
    for (p, (g, w)) in got.iter().zip(want).enumerate() {
        let (x, y) = (p as u32 % canvas.0, p as u32 / canvas.0);
        for c in 0..3 {
            let (g, w) = (f64::from(g[c]), w[c]);
            assert!(
                (g - w).abs() <= 1e-5 * w.abs(),
                "{what}: pixel ({x}, {y}) channel {c} is {g}, the statement says {w}"
            );
        }
        assert!(
            (f64::from(g[3]) - w[3]).abs() <= 1e-6,
            "{what}: pixel ({x}, {y}) alpha is {}, the statement says {}",
            g[3],
            w[3]
        );
    }
}

fn count(report: &Value, key: &str) -> u64 {
    report[key]
        .as_u64()
        .unwrap_or_else(|| panic!("the report's {key}: {report}"))
}

// ------------------------------------------------------- kernel vs statement --

fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^ (x >> 12)
}

/// A scene that reaches every branch of the merge. Canvas 96×64, exposures
/// −3, 0 (the reference) and +2.5. Rows 0…3 are clipped in every frame and
/// rows 4…7 dark in every frame; frame 0 is half transparent on columns
/// 20…27 and 42…43 and holds a patch four times brighter (a subject that
/// moved) at (50…59, 30…39); the reference is a quarter opaque on columns
/// 40…43, so columns 42…43 merge half transparent below the third frame; both
/// are transparent in the corner x < 6, y ≥ 56, which the third frame, a
/// 70×40 layer at (40, −10) that leaves the canvas on two sides, does not
/// reach either.
fn branch_scene(full: u32) -> ((u32, u32), Vec<Frame>, Vec<f64>) {
    let canvas = (96u32, 64u32);
    let evs = vec![-3.0, 0.0, 2.5];
    let radiance = |x: u32, y: u32| -> [f64; 3] {
        if y < 4 {
            return [50.0; 3];
        }
        if y < 8 {
            return [1e-5; 3];
        }
        let grain = 0.6 + 0.8 * f64::from(hash(y * 96 + x) & 0xffff) / 65535.0;
        let base = 0.004 * (8.0 * f64::from(x) / 95.0).exp2() * grain;
        [base, base * (0.7 + 0.3 * f64::from(y) / 63.0), base * 0.85]
    };
    let corner = |x: u32, y: u32| x < 6 && y >= 56;
    let short = Frame::filling("Short", canvas, |x, y| {
        let patch = (50..60).contains(&x) && (30..40).contains(&y);
        let scene = radiance(x, y).map(|v| if patch { v * 4.0 } else { v });
        let mut pixel = exposed(scene, evs[0], full);
        if (20..28).contains(&x) || (42..44).contains(&x) {
            pixel[3] = full / 2;
        }
        if corner(x, y) {
            pixel[3] = 0;
        }
        pixel
    });
    let middle_frame = Frame::filling("Middle", canvas, |x, y| {
        let mut pixel = exposed(radiance(x, y), evs[1], full);
        if (40..44).contains(&x) {
            pixel[3] = full / 4;
        }
        if corner(x, y) {
            pixel[3] = 0;
        }
        pixel
    });
    let (size, offset) = ((70u32, 40u32), (40i32, -10i32));
    let mut codes = Vec::new();
    for ly in 0..size.1 as i32 {
        for lx in 0..size.0 as i32 {
            let (x, y) = (lx + offset.0, ly + offset.1);
            // Off-canvas pixels are never read; any colour will do there.
            let scene = if x >= 0 && y >= 0 && x < 96 && y < 64 {
                radiance(x as u32, y as u32)
            } else {
                [0.3; 3]
            };
            codes.push(exposed(scene, evs[2], full));
        }
    }
    let long = Frame {
        name: "Long".into(),
        size,
        offset,
        codes,
    };
    (canvas, vec![short, middle_frame, long], evs)
}

#[test]
fn the_merge_is_the_f64_statement_at_both_depths_with_and_without_deghosting() {
    for (depth, full) in DEPTHS {
        let (canvas, frames, evs) = branch_scene(full);
        let source = subject(full, canvas, &frames);
        let samples = seen(full, canvas, &frames);
        for deghost in [false, true] {
            let what = format!("{depth}, deghost {deghost}");
            let want = statement(canvas, &samples, &evs, 1, deghost);
            let (merged, report) = source.merge(&request(&evs, 1, deghost));
            assert_eq!(merged.dimensions(), canvas, "{what}: the canvas");
            assert_matches_statement(&merged_pixels(&merged), &want.pixels, canvas, &what);
            assert_eq!(report["reference"], json!(2), "{what}: the reference layer");
            assert_eq!(
                count(&report, "all_clipped_pixels"),
                want.all_clipped,
                "{what}"
            );
            assert_eq!(count(&report, "all_dark_pixels"), want.all_dark, "{what}");
            assert_eq!(count(&report, "deghosted_pixels"), want.deghosted, "{what}");
            let sigmas = report["noise_sigma"].as_array().expect("noise_sigma");
            assert_eq!(sigmas.len(), 3, "{what}: one sigma per frame");
            for (got, want) in sigmas.iter().zip(&want.sigma) {
                match want {
                    None => assert!(got.is_null(), "{what}: an unjudged frame has no sigma"),
                    Some(want) => {
                        let got = got.as_f64().expect("a judged frame's sigma");
                        assert!((got - want).abs() <= 1e-6 * want.abs() + 1e-12, "{what}");
                    }
                }
            }
            // The scene reaches every branch.
            assert!(
                want.all_clipped > 0 && want.all_dark > 0,
                "{what}: fallbacks"
            );
            assert_eq!(want.deghosted > 0, deghost, "{what}: ghosts");
            assert_eq!(
                want.sigma.iter().filter(|s| s.is_some()).count(),
                if deghost { 2 } else { 0 },
                "{what}: both other frames are judged"
            );
            assert!(want.pixels.iter().any(|p| p[3] == 0.0), "{what}: uncovered");
            assert!(
                want.pixels.iter().any(|p| p[3] > 0.0 && p[3] < 1.0),
                "{what}: partial alpha"
            );
        }
    }
}

// ----------------------------------------------------- recovery of the scene --

/// The spec scene's frames: `width` columns of the 14-stop ramp, `height`
/// rows, one frame per EV; `noise` adds a sample of it to each exposed
/// linear channel before the clip.
fn scene_frames(
    full: u32,
    canvas: (u32, u32),
    evs: &[f64],
    radiance: impl Fn(u32) -> f64,
    mut noise: impl FnMut() -> f64,
) -> Vec<Frame> {
    evs.iter()
        .map(|ev| {
            Frame::filling(&format!("EV {ev:+}"), canvas, |x, _| {
                let gain = ev.exp2();
                let rgb = tinted(radiance(x)).map(|v| v * gain + noise());
                [
                    stored(rgb[0], full),
                    stored(rgb[1], full),
                    stored(rgb[2], full),
                    full,
                ]
            })
        })
        .collect()
}

const BRACKETS: [&[f64]; 2] = [&[-4.0, 0.0, 4.0], &[-4.0, -2.0, 0.0, 2.0, 4.0]];

#[test]
fn three_and_five_exposures_recover_the_scene_radiance_at_both_depths() {
    let canvas = (464u32, 4u32);
    for evs in BRACKETS {
        let reference = evs.iter().position(|ev| *ev == 0.0).unwrap();
        for (depth, full) in DEPTHS {
            let bound = if full == 255 { 0.06 } else { 1e-3 };
            let what = format!("{depth}, {} exposures", evs.len());
            let frames = scene_frames(full, canvas, evs, |x| scene_radiance(x, canvas.0), || 0.0);
            let (merged, report) =
                subject(full, canvas, &frames).merge(&request(evs, reference, false));
            assert_eq!(count(&report, "all_clipped_pixels"), 0, "{what}");
            assert_eq!(count(&report, "all_dark_pixels"), 0, "{what}");
            for (p, got) in merged_pixels(&merged).iter().enumerate() {
                let x = p as u32 % canvas.0;
                let want = tinted(scene_radiance(x, canvas.0));
                for c in 0..3 {
                    let error = (f64::from(got[c]) - want[c]).abs() / want[c];
                    assert!(
                        error <= bound,
                        "{what}: column {x} channel {c} is {} for radiance {}, off by {error}",
                        got[c],
                        want[c]
                    );
                }
                assert_eq!(got[3], 1.0, "{what}: opaque frames merge opaque");
            }
        }
    }
}

/// A seeded generator of standard normal samples: xorshift64* into
/// Box-Muller.
struct Gaussian(u64);

impl Gaussian {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let bits = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11;
        (bits as f64 + 0.5) / (1u64 << 53) as f64
    }

    fn sample(&mut self) -> f64 {
        let (u, v) = (self.uniform(), self.uniform());
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }
}

#[test]
fn noisy_half_stop_bands_average_to_the_scene_radiance_at_both_depths() {
    let canvas = (29 * 16u32, 64u32);
    let band_radiance = |x: u32| 6.0e-4 * (f64::from(x / 16) / 2.0).exp2();
    for evs in BRACKETS {
        let reference = evs.iter().position(|ev| *ev == 0.0).unwrap();
        for (depth, full) in DEPTHS {
            let what = format!("{depth}, {} noisy exposures", evs.len());
            let mut generator = Gaussian(0x5EED_0564 + u64::from(full) + evs.len() as u64);
            let frames = scene_frames(full, canvas, evs, band_radiance, || {
                0.001 * generator.sample()
            });
            let (merged, _) = subject(full, canvas, &frames).merge(&request(evs, reference, false));
            let pixels = merged_pixels(&merged);
            for band in 0..29u32 {
                let want = tinted(band_radiance(band * 16));
                let mut sum = [0.0f64; 3];
                for y in 0..canvas.1 {
                    for x in band * 16..band * 16 + 16 {
                        let pixel = pixels[(y * canvas.0 + x) as usize];
                        for c in 0..3 {
                            sum[c] += f64::from(pixel[c]);
                        }
                    }
                }
                for c in 0..3 {
                    let mean = sum[c] / (16.0 * 64.0);
                    let error = (mean - want[c]).abs() / want[c];
                    assert!(
                        error <= 0.05,
                        "{what}: band {band} channel {c} averages {mean} for radiance {}, off by {error}",
                        want[c]
                    );
                }
            }
        }
    }
}

// --------------------------------------------------- fallbacks and coverage --

/// The linear value the document's transfer gives a stored code.
fn linear_of(code: u32, full: u32) -> f64 {
    let transfer = TypedTransfer::for_profile(&IccProfile::srgb()).unwrap();
    f64::from(transfer.decode([(code * (65535 / full)) as f32 / 65535.0; 3])[0])
}

fn assert_near(got: f32, want: f64, what: &str) {
    assert!(
        (f64::from(got) - want).abs() <= 1e-5 * want.abs(),
        "{what}: {got} is not {want}"
    );
}

#[test]
fn where_no_frame_is_trusted_one_frame_stands_alone() {
    let canvas = (8u32, 8u32);
    for (depth, full) in DEPTHS {
        // A code well under 2% of full scale, and one above 98%.
        let (dark, bright) = (full / 100, full - full / 200);
        let flat =
            |name: &str, code: u32| Frame::filling(name, canvas, |_, _| [code, code, code, full]);
        let cases: [(&str, [u32; 3], &str, usize); 3] = [
            // Every frame clipped: the shortest exposure (EV −1, frame 1).
            ("all clipped", [full, bright, full], "all_clipped_pixels", 1),
            // Every frame dark: the longest exposure (EV +3, frame 2).
            ("all dark", [dark, dark / 2, dark], "all_dark_pixels", 2),
            // Clipped and dark frames: the longest one below the clip level (EV 2, frame 0).
            (
                "clipped or dark",
                [dark, dark / 2, full],
                "all_dark_pixels",
                0,
            ),
        ];
        let evs = [2.0, -1.0, 3.0];
        for (name, codes, counter, chosen) in cases {
            let what = format!("{depth}, {name}");
            let frames: Vec<Frame> = codes
                .iter()
                .enumerate()
                .map(|(k, code)| flat(&format!("F{k}"), *code))
                .collect();
            // Frame 0 is the reference, so frame k's radiance is linear / 2^(ev_k − 2).
            let (merged, report) = subject(full, canvas, &frames).merge(&request(&evs, 0, true));
            let want = linear_of(codes[chosen], full) / (evs[chosen] - evs[0]).exp2();
            for pixel in merged_pixels(&merged) {
                for c in 0..3 {
                    assert_near(pixel[c], want, &what);
                }
                assert_eq!(pixel[3], 1.0, "{what}: alpha");
            }
            assert_eq!(
                count(&report, counter),
                64,
                "{what}: every pixel is counted"
            );
            let other = if counter == "all_dark_pixels" {
                "all_clipped_pixels"
            } else {
                "all_dark_pixels"
            };
            assert_eq!(count(&report, other), 0, "{what}: and only once");
            assert_eq!(
                count(&report, "deghosted_pixels"),
                0,
                "{what}: nothing is judged"
            );
        }
    }
}

#[test]
fn alpha_weights_a_frame_and_the_result_takes_the_largest_alpha() {
    let canvas = (6u32, 4u32);
    for (depth, full) in DEPTHS {
        // Two fully trusted greys one stop apart; frame 1 is the reference.
        let (a, b) = (stored(0.10, full), stored(0.22, full));
        let (alpha_a, alpha_b) = (full / 5, full / 2);
        // Frame 0 covers columns 0…3 only (a 4×4 layer at x = 0) and frame 1
        // columns 2…5 (a 4×4 layer at x = 2), so columns 2 and 3 hold both.
        let frame = |name: &str, code: u32, alpha: u32, x: i32| Frame {
            name: name.into(),
            size: (4, 4),
            offset: (x, 0),
            codes: vec![[code, code, code, alpha]; 16],
        };
        let frames = [frame("A", a, alpha_a, 0), frame("B", b, alpha_b, 2)];
        let evs = [-1.0, 0.0];
        let (merged, _) = subject(full, canvas, &frames).merge(&request(&evs, 1, false));
        let pixels = merged_pixels(&merged);
        let (la, lb) = (linear_of(a, full), linear_of(b, full));
        let (fa, fb) = (
            f64::from(alpha_a) / f64::from(full),
            f64::from(alpha_b) / f64::from(full),
        );
        for (p, pixel) in pixels.iter().enumerate() {
            let x = p as u32 % canvas.0;
            let what = format!("{depth}, column {x}");
            let (rgb, alpha) = match x {
                // Frame A alone: its radiance on the reference's scale.
                0 | 1 => (la * 2.0, fa),
                // Both: w = alpha · 2^e, each frame's radiance linear / 2^e.
                2 | 3 => ((fa * 0.5 * (la * 2.0) + fb * lb) / (fa * 0.5 + fb), fb),
                _ => (lb, fb),
            };
            for c in 0..3 {
                assert_near(pixel[c], rgb, &what);
            }
            assert!((f64::from(pixel[3]) - alpha).abs() <= 1e-6, "{what}: alpha");
        }
        // A canvas wider than both frames: the uncovered column is transparent.
        let (merged, _) = subject(full, (7, 5), &frames).merge(&request(&evs, 1, false));
        let pixels = merged_pixels(&merged);
        for (p, pixel) in pixels.iter().enumerate() {
            let (x, y) = (p % 7, p / 7);
            if x == 6 || y == 4 {
                assert_eq!(*pixel, [0.0; 4], "{depth}: ({x}, {y}) has no frame");
            } else {
                assert!(pixel[3] > 0.0, "{depth}: ({x}, {y}) is covered");
            }
        }
    }
}

// ------------------------------------------------------------------ ghosts --

/// The ghost fixture: 48×32, exposures −2, 0 (the reference), +2, background
/// 0.08 and, when `patch` is set, a 6×6 patch of 0.5 (both linear at the
/// reference exposure) at a different place in each frame.
const GHOST_CANVAS: (u32, u32) = (48, 32);
const GHOST_PATCHES: [(u32, u32); 3] = [(4, 4), (20, 12), (36, 20)];

fn in_patch(k: usize, x: u32, y: u32) -> bool {
    let (px, py) = GHOST_PATCHES[k];
    (px..px + 6).contains(&x) && (py..py + 6).contains(&y)
}

fn ghost_frames(full: u32, patch: bool) -> Vec<Frame> {
    [-2.0, 0.0, 2.0]
        .iter()
        .enumerate()
        .map(|(k, ev)| {
            Frame::filling(&format!("G{k}"), GHOST_CANVAS, |x, y| {
                let value = if patch && in_patch(k, x, y) {
                    0.5
                } else {
                    0.08
                };
                exposed([value; 3], *ev, full)
            })
        })
        .collect()
}

#[test]
fn a_moving_patch_is_given_to_the_reference_within_its_footprints() {
    let evs = [-2.0, 0.0, 2.0];
    for (depth, full) in DEPTHS {
        let bound = if full == 255 { 0.06 } else { 1e-3 };
        let frames = ghost_frames(full, true);
        let source = subject(full, GHOST_CANVAS, &frames);
        let samples = seen(full, GHOST_CANVAS, &frames);
        let (merged, report) = source.merge(&request(&evs, 1, true));
        let pixels = merged_pixels(&merged);
        let deghosted = count(&report, "deghosted_pixels");
        let union = 3 * 36;
        assert!(
            deghosted > 0 && deghosted <= union,
            "{depth}: {deghosted} deghosted pixels for a union of {union}"
        );
        for (p, pixel) in pixels.iter().enumerate() {
            let (x, y) = (p as u32 % GHOST_CANVAS.0, p as u32 / GHOST_CANVAS.0);
            let what = format!("{depth}, ({x}, {y})");
            let anchor = samples[1][p].unwrap().linear;
            if in_patch(0, x, y) || in_patch(1, x, y) {
                // The short frame and the reference disagree and both are
                // fully trusted: the reference stands alone, exactly.
                for c in 0..3 {
                    assert_eq!(
                        f64::from(pixel[c]),
                        anchor[c],
                        "{what}: the reference alone"
                    );
                }
            } else {
                // Elsewhere, the long frame's clipped patch included, the
                // merge is the reference's radiance within the depth's bound.
                for c in 0..3 {
                    let error = (f64::from(pixel[c]) - anchor[c]).abs() / anchor[c];
                    assert!(error <= bound, "{what}: off the reference by {error}");
                }
            }
        }
        // Without the test the short frame's patch shows through.
        let (plain, report) = source.merge(&request(&evs, 1, false));
        assert_eq!(count(&report, "deghosted_pixels"), 0, "{depth}: not asked");
        let p = (GHOST_PATCHES[0].1 * GHOST_CANVAS.0 + GHOST_PATCHES[0].0) as usize;
        let ghost = f64::from(merged_pixels(&plain)[p][0]);
        let anchor = samples[1][p].unwrap().linear[0];
        assert!(
            (ghost - anchor).abs() / anchor > 0.1,
            "{depth}: the fixture has a ghost to remove ({ghost} against {anchor})"
        );
    }
}

#[test]
fn a_constant_ev_error_on_a_static_scene_deghosts_nothing() {
    for (depth, full) in DEPTHS {
        let frames = ghost_frames(full, false);
        let source = subject(full, GHOST_CANVAS, &frames);
        // The frames are really at −2, 0, +2: each given EV is 0.2 stops off,
        // more than the 10% floor, and the same everywhere.
        let (_, report) = source.merge(&request(&[-2.2, 0.0, 2.2], 1, true));
        assert_eq!(count(&report, "deghosted_pixels"), 0, "{depth}");
        let sigmas = report["noise_sigma"].as_array().unwrap();
        assert!(sigmas[1].is_null(), "{depth}: the reference is not judged");
        for k in [0, 2] {
            assert_eq!(
                sigmas[k].as_f64(),
                Some(0.0),
                "{depth}: a flat frame has no noise"
            );
        }
    }
}

// --------------------------------------------------------------- exposures --

fn evs_of(reply: &Value) -> Vec<f64> {
    reply["evs"]
        .as_array()
        .unwrap_or_else(|| panic!("an estimate: {reply}"))
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

#[test]
fn exposures_are_estimated_from_the_pixels_relative_to_the_first_listed_layer() {
    let canvas = (464u32, 4u32);
    for (depth, full) in DEPTHS {
        let bound = if full == 255 { 0.1 } else { 0.01 };
        for evs in BRACKETS {
            let frames = scene_frames(full, canvas, evs, |x| scene_radiance(x, canvas.0), || 0.0);
            let source = subject(full, canvas, &frames);
            // Listed out of exposure order: the answer follows the list.
            let mut order: Vec<usize> = (0..evs.len()).collect();
            order.rotate_left(1);
            order.swap(0, evs.len() - 1);
            let layers: Vec<usize> = order.iter().map(|k| k + 1).collect();
            let reply = source
                .exposures(&json!({"version": 1, "layers": layers}), None)
                .unwrap();
            let got = evs_of(&reply);
            assert_eq!(got.len(), evs.len());
            assert_eq!(got[0], 0.0, "{depth}: relative to the first listed layer");
            for (position, k) in order.iter().enumerate() {
                let want = evs[*k] - evs[order[0]];
                assert!(
                    (got[position] - want).abs() <= bound,
                    "{depth}, {} exposures: layer {} is estimated {} for {want}",
                    evs.len(),
                    k + 1,
                    got[position]
                );
            }
        }
    }
}

#[test]
fn frames_with_no_shared_exposure_answer_a_reason_instead_of_an_estimate() {
    let canvas = (464u32, 4u32);
    for (depth, full) in DEPTHS {
        // Eight stops apart: where the short frame is in its trusted band the
        // long one is clipped.
        let evs = [0.0, 8.0];
        let frames = scene_frames(full, canvas, &evs, |x| scene_radiance(x, canvas.0), || 0.0);
        let source = subject(full, canvas, &frames);
        let reply = source
            .exposures(&json!({"version": 1, "layers": [2, 1]}), None)
            .unwrap();
        assert!(reply["evs"].is_null(), "{depth}: no estimate: {reply}");
        assert_eq!(
            reply["reason"],
            json!(
                "the exposures of “EV +0” and “EV +8” could not be estimated (too few pixels are well exposed in both); give each frame's EV"
            ),
            "{depth}: the pair is named darkest first"
        );
        // The merge itself needs no estimate.
        let (_, report) = source.merge(&request(&evs, 0, false));
        assert_eq!(report["reference"], json!(1), "{depth}");
    }
}

// ---------------------------------------------------------------- refusals --

/// A two-frame stack both exports accept, for the refusals to vary.
fn pair(full: u32) -> ((u32, u32), Vec<Frame>) {
    let canvas = (16u32, 8u32);
    let frames = scene_frames(
        full,
        canvas,
        &[0.0, 2.0],
        |x| 0.01 + 0.01 * f64::from(x),
        || 0.0,
    );
    (canvas, frames)
}

/// The profile fixture of `precision_conversion.rs`: sRGB's header with no
/// tags, so there is no matrix/TRC model.
fn unmodelled() -> Arc<IccProfile> {
    let mut bytes = IccProfile::srgb().bytes()[..128].to_vec();
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes[..4].copy_from_slice(&132u32.to_be_bytes());
    let profile = IccProfile::parse(&bytes).unwrap();
    assert!(profile.model().is_none());
    Arc::new(profile)
}

#[test]
fn a_malformed_request_is_refused_by_every_export() {
    for (depth, full) in DEPTHS {
        let (canvas, frames) = pair(full);
        let source = subject(full, canvas, &frames);
        let good = request(&[0.0, 2.0], 0, false);
        assert!(
            source.plan(&good).is_ok(),
            "{depth}: the base request plans"
        );
        let altered = |key: &str, value: Value| {
            let mut out = good.clone();
            out[key] = value;
            out
        };
        let many: Vec<usize> = (1..=17).collect();
        let cases: Vec<(Value, &str)> = vec![
            (altered("version", json!(2)), "version 2 is not supported"),
            (
                altered("layers", json!([1])),
                "takes 2 to 16 layers; 1 were given",
            ),
            (
                altered("layers", json!(many)),
                "takes 2 to 16 layers; 17 were given",
            ),
            (altered("layers", json!([1, 1])), "layer 1 is listed twice"),
            (altered("layers", json!([1, 9])), "layer 9 is out of range"),
            (
                altered("evs", json!([0.0])),
                "one EV per layer; 1 were given for 2 layers",
            ),
            (
                altered("reference", json!(0)),
                "reference 0 is not one of the listed layers",
            ),
            (altered("surprise", json!(true)), "unknown field"),
            (altered("layers", json!("all")), "is not valid"),
            (
                altered("evs", json!([0.0, 16.5])),
                "“EV +2” is 16.5 EV from the reference “EV +0”",
            ),
        ];
        for (bad, reason) in &cases {
            let got = source.refusal(bad);
            assert!(
                got.contains(reason),
                "{depth}: {bad} was refused with: {got}"
            );
            let planned = source.plan(bad).unwrap_err();
            assert_eq!(planned, got, "{depth}: the plan refuses in the same words");
        }
        // The exposures request is `version` and `layers` alone.
        let estimate = |value: Value| source.exposures(&value, None).unwrap_err();
        assert!(
            estimate(good.clone()).contains("unknown field"),
            "{depth}: evs"
        );
        assert!(estimate(json!({"version": 1, "layers": [1]})).contains("takes 2 to 16 layers"));
        assert!(estimate(json!({"version": 3, "layers": [1, 2]})).contains("version 3"));
        assert!(
            estimate(json!({"version": 1, "layers": [1, 7]})).contains("layer 7 is out of range")
        );
        // Past 64 KiB the request is refused before it is parsed.
        let mut padded = good.clone();
        padded["surprise"] = json!("x".repeat(64 * 1024));
        assert!(
            source
                .refusal(&padded)
                .contains("exceeds the supported byte limit"),
            "{depth}: an oversized request"
        );
    }
}

#[test]
fn a_layer_that_is_not_an_exposure_is_refused_by_name() {
    let evs = [0.0, 2.0];
    let all = |source: &Subject, layers: Value, reason: &str, what: &str| {
        let mut merge = request(&evs, 0, false);
        merge["layers"] = layers.clone();
        merge["reference"] = layers[0].clone();
        let got = source.refusal(&merge);
        assert!(
            got.contains(reason),
            "{what}: the merge was refused with: {got}"
        );
        assert_eq!(source.plan(&merge).unwrap_err(), got, "{what}: the plan");
        let estimate = source
            .exposures(&json!({"version": 1, "layers": layers}), None)
            .unwrap_err();
        assert_eq!(estimate, got, "{what}: the estimate");
    };
    // 8 bits: a group, an adjustment layer, a profile with no model.
    let (canvas, frames) = pair(255);
    let mut doc = byte_document(canvas, &frames);
    doc.layers[1].depth = 1;
    doc.layers[2].depth = 1;
    let mut group = doc.layers[0].clone();
    group.kind = LayerKind::Group;
    group.name = "Bracket".into();
    group.pixels = LayerRaster::new(RgbaImage::new(1, 1));
    doc.layers.push(group);
    let mut adjustment = doc.layers[0].clone();
    adjustment.name = "Curve".into();
    adjustment.meta = Some(r#"{"type":"adjust","op":"levels","params":{"gamma":0.72}}"#.into());
    doc.layers.push(adjustment);
    let grouped = Subject::Byte(Box::new(doc));
    all(
        &grouped,
        json!([1, 3]),
        "“Bracket” is a group; Merge to HDR takes raster layers",
        "byte group",
    );
    all(
        &grouped,
        json!([2, 4]),
        "“Curve” is an adjustment layer; Merge to HDR takes raster layers",
        "byte adjustment",
    );
    let mut doc = byte_document(canvas, &frames);
    doc.profile = unmodelled();
    all(
        &Subject::Byte(Box::new(doc)),
        json!([1, 2]),
        "has no matrix/TRC model this build can decode, so it cannot carry a 32-bit float document",
        "byte profile",
    );
    // 16 bits: a group.
    let (canvas, frames) = pair(65535);
    let profile = IccProfile::srgb();
    let raster = |name: &str, frame: &Frame| {
        let codes = frame.codes.iter().map(|p| p.map(|v| v as u16)).collect();
        let image =
            PrecisionImage::from_rgba16(canvas.0, canvas.1, codes, profile.clone(), None).unwrap();
        PrecisionDocumentLayer::raster(name, image, (0, 0), 1., true).unwrap()
    };
    let folder = PrecisionDocumentLayer::isolated_normal(
        "Bracket",
        vec![raster("Inside", &frames[1])],
        1.,
        true,
    )
    .unwrap();
    let typed = PrecisionDocument::from_parts(
        canvas.0,
        canvas.1,
        profile.clone(),
        Metadata::default(),
        Resolution::default(),
        vec![raster("Below", &frames[0]), folder],
    )
    .unwrap();
    // Postorder: 0 "Below", 1 "Inside", 2 the group.
    let typed = Subject::Typed(Box::new(typed));
    all(
        &typed,
        json!([0, 2]),
        "“Bracket” is a group; Merge to HDR takes raster layers",
        "typed group",
    );
    assert!(typed
        .plan(
            &json!({"version": 1, "layers": [0, 1], "evs": evs, "reference": 0, "deghost": false})
        )
        .is_ok());
    // 32 bits: there is nothing to merge to.
    let record = vec![[0.5f32, 0.5, 0.5, 1.0]; 16];
    let float = float_doc(4, 4, &profile, &[record.clone(), record]);
    let name = float.layer(0).unwrap().name().to_string();
    all(
        &Subject::Typed(Box::new(float)),
        json!([0, 1]),
        &format!("“{name}” is already 32-bit float; Merge to HDR takes 8- or 16-bit exposures"),
        "float",
    );
}

// ------------------------------------------------------------- the document --

#[test]
fn the_result_is_a_new_one_layer_float_document_and_the_source_is_untouched() {
    let (canvas, frames) = pair(255);
    let mut doc = byte_document(canvas, &frames);
    doc.profile = IccProfile::display_p3();
    doc.resolution = Resolution { x: 240.0, y: 300.0 };
    doc.metadata.xmp = Some(Arc::from(b"one exposure's packet".as_slice()));
    let before: Vec<Vec<u8>> = doc
        .layers
        .iter()
        .map(|layer| layer.pixels.contiguous_image().as_raw().clone())
        .collect();
    let source = Subject::Byte(Box::new(doc));
    let (merged, report) = source.merge(&request(&[0.0, 2.0], 1, false));
    assert!(merged.is_float());
    assert_eq!(merged.dimensions(), canvas);
    assert_eq!(merged.layer_count(), 1);
    let layer = merged.layer(0).unwrap();
    assert_eq!(layer.name(), "Merged HDR");
    assert_eq!(layer.offset(), (0, 0));
    assert_eq!(layer.source().unwrap().dimensions(), canvas);
    assert_eq!(merged.tone_map(), Some(ToneMap::Clip));
    assert_eq!(merged.profile().bytes(), IccProfile::display_p3().bytes());
    assert_eq!(merged.resolution(), Resolution { x: 240.0, y: 300.0 });
    assert_eq!(
        merged.metadata(),
        &Metadata::default(),
        "no frame's packets"
    );
    assert!(merged.float_refusal().is_none());
    assert_eq!(report["reference"], json!(2));
    let Subject::Byte(doc) = &source else {
        unreachable!()
    };
    assert_eq!(doc.layers.len(), 3);
    for (layer, pixels) in doc.layers.iter().zip(&before) {
        assert_eq!(layer.pixels.contiguous_image().as_raw(), pixels);
    }
}

// -------------------------------------------------------------- the memory --

#[test]
fn the_plan_covers_every_reservation_and_admission_counts_the_source() {
    for (depth, full) in DEPTHS {
        let (canvas, frames, evs) = branch_scene(full);
        let source = subject(full, canvas, &frames);
        let merge = request(&evs, 1, true);
        let plan = source.plan(&merge).unwrap();
        let pixels = u64::from(canvas.0) * u64::from(canvas.1);
        assert_eq!(plan["width"], json!(canvas.0), "{depth}");
        assert_eq!(plan["height"], json!(canvas.1), "{depth}");
        assert_eq!(plan["frames"], json!(3), "{depth}");
        let output = plan["output_bytes"].as_u64().unwrap();
        let scratch = plan["scratch_bytes"].as_u64().unwrap();
        assert_eq!(output, 16 * pixels, "{depth}: the result's bytes");
        // The adoption copy, the transfer's tables and the bookkeeping at least.
        assert!(
            scratch >= 16 * pixels + 3 * 65536 * 4 + 4 * 1024 * 1024,
            "{depth}"
        );
        // Every `reserve` is checked against the token's limit, so success at
        // the plan is the witness that no reservation exceeds it, and the
        // refusal one byte under it that the whole plan is reserved.
        let control = DrJobControl::default();
        assert!(unsafe { dr_job_control_set_scratch_limit(&control, output + scratch) });
        let merged = source.merge_at(&merge, LIMIT, 0, Some(&control));
        assert!(merged.is_ok(), "{depth}: {:?}", merged.err());
        assert_eq!(control.scratch_bytes(), 0, "{depth}: released");
        assert_eq!(control.progress(), 1.0, "{depth}: complete");
        let control = DrJobControl::default();
        assert!(unsafe { dr_job_control_set_scratch_limit(&control, output + scratch - 1) });
        let refused = source.refused_at(&merge, LIMIT, 0, Some(&control));
        assert!(refused.contains("working memory"), "{depth}: {refused}");
        assert_eq!(control.scratch_bytes(), 0, "{depth}: released");
        // Admission: the plan alone does not fit a limit of its own size,
        // because the source's retained bytes count too.
        let refused = source.refused_at(&merge, output + scratch, 0, None);
        assert!(refused.contains("working limit"), "{depth}: {refused}");
        let refused = source.refused_at(&merge, LIMIT, LIMIT - output - scratch, None);
        assert!(refused.contains("working limit"), "{depth}: {refused}");
    }
}

#[test]
fn a_cancelled_job_answers_no_document_and_no_estimate() {
    for (depth, full) in DEPTHS {
        let (canvas, frames) = pair(full);
        let source = subject(full, canvas, &frames);
        let control = DrJobControl::default();
        control.cancel();
        let refused = source.refused_at(&request(&[0.0, 2.0], 0, true), LIMIT, 0, Some(&control));
        assert_eq!(refused, "operation cancelled", "{depth}");
        assert_eq!(control.scratch_bytes(), 0, "{depth}");
        let control = DrJobControl::default();
        control.cancel();
        let refused = source
            .exposures(&json!({"version": 1, "layers": [1, 2]}), Some(&control))
            .unwrap_err();
        assert_eq!(refused, "operation cancelled", "{depth}");
        // A token is single-use.
        let control = DrJobControl::default();
        let merge = request(&[0.0, 2.0], 0, false);
        assert!(
            source.merge_at(&merge, LIMIT, 0, Some(&control)).is_ok(),
            "{depth}"
        );
        let again = source.refused_at(&merge, LIMIT, 0, Some(&control));
        assert_eq!(again, "job control has already been used", "{depth}");
    }
}

// ------------------------------------------------------------- null safety --

/// The six exports called with these handles and requests (each may be
/// NULL), a NULL control and, unless `reasons`, NULL `report_out` and
/// `err_out`. Answers each export's name, whether it answered NULL and the
/// reason it gave; every result is freed.
unsafe fn sweep(
    byte: *const DrDocument,
    typed: *const PrecisionDocument,
    estimate: *const c_char,
    merge: *const c_char,
    reasons: bool,
) -> Vec<(&'static str, bool, Option<String>)> {
    let mut out = Vec::new();
    let mut note = |name: &'static str, null: bool, error: *mut c_char| {
        out.push((name, null, taken(error)));
    };
    let slot = |error: &mut *mut c_char| {
        if reasons {
            error as *mut *mut c_char
        } else {
            ptr::null_mut()
        }
    };
    let none = ptr::null();
    unsafe {
        let mut error = ptr::null_mut();
        let text = dr_doc_merge_hdr_exposures(byte, estimate, none, slot(&mut error));
        note("dr_doc_merge_hdr_exposures", taken(text).is_none(), error);
        let mut error = ptr::null_mut();
        let text = dr_doc_merge_hdr_plan(byte, merge, slot(&mut error));
        note("dr_doc_merge_hdr_plan", taken(text).is_none(), error);
        let (mut error, mut report) = (ptr::null_mut(), ptr::null_mut());
        let doc = dr_doc_merge_hdr(
            byte,
            merge,
            LIMIT,
            0,
            none,
            slot(&mut report),
            slot(&mut error),
        );
        assert_eq!(taken(report).is_some(), reasons && !doc.is_null());
        note("dr_doc_merge_hdr", doc.is_null(), error);
        if !doc.is_null() {
            drop(Box::from_raw(doc));
        }
        let mut error = ptr::null_mut();
        let text = dr_precision_doc_merge_hdr_exposures(typed, estimate, none, slot(&mut error));
        note(
            "dr_precision_doc_merge_hdr_exposures",
            taken(text).is_none(),
            error,
        );
        let mut error = ptr::null_mut();
        let text = dr_precision_doc_merge_hdr_plan(typed, merge, slot(&mut error));
        note(
            "dr_precision_doc_merge_hdr_plan",
            taken(text).is_none(),
            error,
        );
        let (mut error, mut report) = (ptr::null_mut(), ptr::null_mut());
        let doc = dr_precision_doc_merge_hdr(
            typed,
            merge,
            LIMIT,
            0,
            none,
            slot(&mut report),
            slot(&mut error),
        );
        assert_eq!(taken(report).is_some(), reasons && !doc.is_null());
        note("dr_precision_doc_merge_hdr", doc.is_null(), error);
        if !doc.is_null() {
            drop(Box::from_raw(doc));
        }
    }
    out
}

#[test]
fn every_merge_export_tolerates_null() {
    let (canvas, frames) = pair(255);
    let byte = byte_document(canvas, &frames);
    let (canvas, frames) = pair(65535);
    let typed = typed_document(canvas, &frames);
    let merge = CString::new(request(&[0.0, 2.0], 0, false).to_string()).unwrap();
    let estimate = CString::new(json!({"version": 1, "layers": [1, 2]}).to_string()).unwrap();
    let (merge, estimate) = (merge.as_ptr(), estimate.as_ptr());
    let (no_byte, no_typed): (*const DrDocument, *const PrecisionDocument) =
        (ptr::null(), ptr::null());
    unsafe {
        // A NULL handle names itself, and is quiet with nowhere to say so.
        for (name, null, reason) in sweep(no_byte, no_typed, estimate, merge, true) {
            assert!(null, "{name}: a NULL handle answers NULL");
            assert_eq!(reason.as_deref(), Some("required handle is NULL"), "{name}");
        }
        for (name, null, reason) in sweep(no_byte, no_typed, ptr::null(), ptr::null(), false) {
            assert!(null && reason.is_none(), "{name}: every argument NULL");
        }
        // A NULL request on a live handle.
        for (name, null, reason) in sweep(&byte, &typed, ptr::null(), ptr::null(), true) {
            assert!(null, "{name}: a NULL request answers NULL");
            assert_eq!(
                reason.as_deref(),
                Some("merge to hdr request is NULL"),
                "{name}"
            );
        }
        // NULL control, report_out and err_out on calls that succeed.
        for (name, null, _) in sweep(&byte, &typed, estimate, merge, false) {
            assert!(!null, "{name}: the optional arguments may be NULL");
        }
        for (name, null, reason) in sweep(&byte, &typed, estimate, merge, true) {
            assert!(!null && reason.is_none(), "{name}: a result has no error");
        }
    }
}
