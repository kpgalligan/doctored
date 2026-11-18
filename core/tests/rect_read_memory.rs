//! Black-box: the reads that follow their rect, never the canvas (13B-9a).
//! A typed Clone or Heal resamples only the rect its coverage bounds, the
//! Align with Layer box reads its mask in place, and a group mask's
//! thumbnail places the mask's rows without a copy of the plane.
//!
//! The bounds, declared before measuring, on a 3000 × 2000 canvas
//! (`N` = 6,000,000 pixels); each is the peak live heap during the export
//! call above the live bytes just before it:
//! - a 100 × 100 `sample: current` dab (`dr_precision_doc_edit`; 16-bit and
//!   32-bit float, Clone and Heal) stays below `2·N` bytes, which one
//!   canvas-sized u16 coverage plane reaches by itself;
//! - `dr_doc_fill_align_box` and `dr_precision_doc_fill_align_box` over a
//!   canvas-sized mask stay below `N/2`, half of the byte mask's own size;
//! - `dr_doc_layer_mask_thumbnail` of a group's canvas-sized mask at
//!   `max_side` 64 stays below `N/2`.
//!
//! 13B-9b's bound, declared before measuring, on a 4000 × 3000 canvas (`SN`
//! = 12,000,000 pixels): a 48 × 48 `sample: current` Spot Heal
//! (`content_aware`, `proximity_match`) or Content-Aware Fill through
//! `dr_precision_doc_edit`, at 16 bits, 32-bit float and CMYK, stays below
//! `SN` bytes, one byte a canvas pixel. A canvas plate (8 or 16 B a pixel,
//! 10 on ink) breaks it, and so does a hole plane and component walk over the
//! canvas rather than the request's bounds (one byte a canvas pixel each).
//!
//! 13B-9c's bounds, declared before measuring, on the first canvas: a
//! 100 × 100 `sample: current` Clone with a selection, in canvas form and in
//! rect form (`coverage_rect`), at 16 bits and 32-bit float, stays below
//! `2·N` (the canvas-sized selection domain reaches it by itself), and the
//! rect form's plan is at least 1.25 × its peak and below its fixed
//! bookkeeping plus `2·N` (the in-place plan charged `40·N` before); and the
//! 100 × 100 `sample: current` Clone and Heal dabs on the document converted
//! to CMYK stay below `2·N` (the canvas ink plate and canvas passes break it).
//!
//! 13B-9's CMYK brush bounds, declared before measuring, on the document
//! converted to CMYK: a 100 × 100 dab of Dodge, Burn, the Blur, Sharpen and
//! Desaturate brushes (`dr_precision_doc_edit`), the History Brush painting
//! the pre-dodge state back over the dodged one
//! (`dr_precision_doc_history_brush`) and a size-100 hard Smudge stroke of
//! three dabs across the dab at strength 0.5 (`dr_precision_doc_smudge`) each
//! stay below `2·N` (one canvas pass is `8·N`); and Smudge's plan
//! (`dr_precision_doc_smudge_plan`, no selection, no Sample All Layers) less
//! its canvas display term (21 bytes a canvas pixel) is at least 1.25 × the
//! stroke's counted peak, and the whole plan is below `30·N`.
//!
//! The coverage brushes' rect-form bounds, declared before measuring, on the
//! same CMYK document: the 100 × 100 dab of Dodge, the Blur, Sharpen and
//! Desaturate brushes and the History Brush sent over its rect
//! (`coverage_rect` = `DAB_RECT`) peaks below `2·N`, its plan less the
//! display term is at least 1.25 × the counted peak, and the whole plan is
//! below `30·N` (the canvas form's by-layer plans, printed for the record,
//! charge each layer pixel); a 1500 × 1200 stroke's plan keeps the same
//! 1.25 × margin, which there holds the per-pixel terms rather than the
//! fixed bookkeeping.
//!
//! The warm whole-layer Copy's bound, declared before measuring, on the first
//! canvas: `PrecisionDocument::clipboard_copy` of layer 0 over the whole
//! canvas, no coverage, at 16 bits and 32-bit float, peaks at most 1.25 ×
//! its payload (8 or 16 bytes a canvas pixel); a contiguous staging buffer
//! tiled after the read is 2 ×.
//!
//! The figures are live heap bytes from the tracking allocator below; the
//! allocator is process-wide, so the binary is its own and every test holds
//! `serial()`.

#[path = "common/cmyk_fixture.rs"]
mod cmyk_fixture;
mod common;
#[path = "common/float_fixture.rs"]
mod float_fixture;
#[path = "common/icc_lut_builders.rs"]
mod icc_lut_builders;

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use doctored_core::doc::{DrDocument, GrayRaster};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::{dr_image_free, dr_image_height, dr_image_width, dr_string_free};
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_doc::{dr_doc_adding_layer_mask, dr_doc_free, dr_doc_layer_mask_thumbnail};
use doctored_core::ffi_fill_align::{dr_doc_fill_align_box, dr_precision_doc_fill_align_box};
use doctored_core::ffi_group::dr_doc_group_layers;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_retouch_brush::{
    dr_precision_doc_history_brush, dr_precision_doc_history_brush_plan, dr_precision_doc_smudge,
    dr_precision_doc_smudge_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::layer_raster::LayerRaster;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::ClipboardRead;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use image::{GrayImage, RgbaImage};
use serde_json::{json, Value};

// ------------------------------------------------------------ allocator --

/// Tracks live heap bytes and their peak (`alloc_zeroed` / `realloc`
/// included).
struct Tracked;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grew(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Tracked {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, length: usize) -> *mut u8 {
        let p = unsafe { System.realloc(pointer, layout, length) };
        if !p.is_null() {
            if length >= layout.size() {
                grew(length - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - length, Ordering::Relaxed);
            }
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: Tracked = Tracked;

// ------------------------------------------------------------- plumbing --

const W: u32 = 3000;
const H: u32 = 2000;
/// Canvas pixels.
const N: usize = W as usize * H as usize;
/// The admission limit: high enough that only the bytes are measured.
const LIMIT: u64 = 64 << 30;
/// The dab: a 100 × 100 rect of full coverage.
const DAB: (u32, u32, u32, u32) = (1000, 800, 100, 100);
/// The box the masks below cover.
const BOX: (u32, u32, u32, u32) = (700, 450, 1300, 900);

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Starts a measurement: the live bytes now, with the peak reset to them.
fn begin() -> usize {
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    base
}

fn peak_since(base: usize) -> usize {
    PEAK.load(Ordering::Relaxed).saturating_sub(base)
}

fn inside(i: usize, (x, y, w, h): (u32, u32, u32, u32)) -> bool {
    let (px, py) = ((i % W as usize) as u32, (i / W as usize) as u32);
    px >= x && px < x + w && py >= y && py < y + h
}

fn coverage(rect: (u32, u32, u32, u32)) -> Vec<u16> {
    (0..N)
        .map(|i| if inside(i, rect) { u16::MAX } else { 0 })
        .collect()
}

fn pattern16() -> Vec<[u16; 4]> {
    (0..N as u32)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            [
                ((x * 2711 + y * 911) % 65536) as u16,
                ((x * 523 + y * 3301) % 65536) as u16,
                (((x ^ y) * 1999) % 65536) as u16,
                65535,
            ]
        })
        .collect()
}

fn pattern32() -> Vec<[f32; 4]> {
    pattern16()
        .into_iter()
        .map(|p| {
            [
                f32::from(p[0]) / 65535.0 * 2.0,
                f32::from(p[1]) / 65535.0,
                f32::from(p[2]) / 65535.0 * 0.5,
                1.0,
            ]
        })
        .collect()
}

fn document16(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("a document")
}

fn raster16() -> Layer {
    let image = PrecisionImage::from_rgba16(W, H, pattern16(), IccProfile::srgb(), None)
        .expect("a layer image");
    Layer::raster("L", image, (0, 0), 1.0, true).expect("a layer")
}

fn take(error: *mut std::ffi::c_char) -> String {
    if error.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    text
}

/// One `dr_precision_doc_edit` call: its peak above the live bytes before
/// it. The answer must be a document.
fn dab_peak(doc: &PrecisionDocument, request: &Value, samples: &[u16]) -> usize {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let base = begin();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let peak = peak_since(base);
    assert!(!out.is_null(), "{request}: {}", take(error));
    drop(unsafe { Box::from_raw(out) });
    peak
}

fn requests() -> [(&'static str, Value); 2] {
    let matrix = [1.0, 0.0, 0.0, 1.0, 300.0, 0.0];
    [
        (
            "clone",
            json!({"op":"retouch_clone","layer":0,"sample":"current","matrix":matrix,"opacity":1.0}),
        ),
        (
            "heal",
            json!({"op":"retouch_heal","layer":0,"sample":"current","matrix":matrix,"opacity":1.0}),
        ),
    ]
}

// ---------------------------------------------------------------- tests --

/// Every request's dab peak on `doc` (each figure printed), then the bound.
fn assert_dabs_below_bound(depth: &str, doc: &PrecisionDocument) {
    let samples = coverage(DAB);
    let peaks: Vec<(&str, usize)> = requests()
        .iter()
        .map(|(label, request)| (*label, dab_peak(doc, request, &samples)))
        .collect();
    for (label, peak) in &peaks {
        println!("{depth} {label} 100x100 dab at {W}x{H}: peak {peak}");
    }
    for (label, peak) in peaks {
        assert!(
            peak < 2 * N,
            "{depth} {label}: the peak {peak} reaches {}",
            2 * N
        );
    }
}

#[test]
fn a_16_bit_current_dab_peaks_below_two_bytes_a_canvas_pixel() {
    let _serial = serial();
    assert_dabs_below_bound("16-bit", &document16(vec![raster16()]));
}

#[test]
fn a_float_current_dab_peaks_below_two_bytes_a_canvas_pixel() {
    let _serial = serial();
    let doc = float_fixture::float_doc(W, H, &IccProfile::srgb(), &[pattern32()]);
    assert_dabs_below_bound("float", &doc);
}

/// 13B-9c's CMYK bound, declared before measuring: the same dabs on the
/// document converted to CMYK stay below `2·N` (a canvas ink plate is 10
/// bytes a pixel, a canvas pass 8).
#[test]
fn a_cmyk_current_dab_peaks_below_two_bytes_a_canvas_pixel() {
    let _serial = serial();
    let rgb = document16(vec![raster16()]);
    let profile = cmyk_fixture::press_profile("Press Test");
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            &rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take(error));
    drop(rgb);
    let doc = *unsafe { Box::from_raw(out) };
    assert!(doc.is_cmyk());
    assert_dabs_below_bound("cmyk", &doc);
}

// ------------------------------------------------- CMYK brush dabs --

/// A hard round tip (`dr_brush_u16`'s request).
const HARD: &str =
    r#"{"version":1,"hardness":1.0,"roundness":1.0,"angle_degrees":0.0,"pixel":false}"#;

/// The canvas term every CMYK plan charges for the host's display
/// preparation (`cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL`).
const DISPLAY_BYTES_PER_PIXEL: u64 = 21;

/// [`raster16`]'s document converted into the synthetic press profile.
fn cmyk_dab_document() -> PrecisionDocument {
    let rgb = document16(vec![raster16()]);
    let profile = cmyk_fixture::press_profile("Press Test");
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            &rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take(error));
    let doc = *unsafe { Box::from_raw(out) };
    assert!(doc.is_cmyk());
    doc
}

/// One `dr_precision_doc_edit` call's document (unmeasured).
fn edited(doc: &PrecisionDocument, request: &Value, samples: &[u16]) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take(error));
    *unsafe { Box::from_raw(out) }
}

/// The History Brush painting `source`'s layer 0 back over `doc`'s under
/// `samples`, laid out over `rect` (the canvas when `None`): its peak above
/// the live bytes before it.
fn history_peak(
    doc: &PrecisionDocument,
    source: &PrecisionDocument,
    samples: &[u16],
    rect: Option<(u32, u32, u32, u32)>,
) -> usize {
    let rect = rect.map(|(x, y, w, h)| [x, y, w, h]);
    let mut error = ptr::null_mut();
    let base = begin();
    let out = unsafe {
        dr_precision_doc_history_brush(
            doc,
            0,
            source,
            0,
            1.0,
            samples.as_ptr(),
            samples.len(),
            rect.as_ref().map_or(ptr::null(), |r| r.as_ptr()),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let peak = peak_since(base);
    assert!(!out.is_null(), "history brush: {}", take(error));
    drop(unsafe { Box::from_raw(out) });
    peak
}

/// Three size-100 hard dabs across [`DAB`] (`dr_brush_u16`'s tuples).
const SMUDGE_DABS: [f64; 12] = [
    1010.0, 850.0, 100.0, 1.0, 1050.0, 850.0, 100.0, 1.0, 1090.0, 850.0, 100.0, 1.0,
];

/// The Smudge stroke at strength 0.5: its peak above the live bytes before it.
fn smudge_peak(doc: &PrecisionDocument) -> usize {
    let brush = CString::new(HARD).unwrap();
    let mut error = ptr::null_mut();
    let base = begin();
    let out = unsafe {
        dr_precision_doc_smudge(
            doc,
            0,
            brush.as_ptr(),
            SMUDGE_DABS.as_ptr(),
            SMUDGE_DABS.len() / 4,
            0.5,
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let peak = peak_since(base);
    assert!(!out.is_null(), "smudge: {}", take(error));
    drop(unsafe { Box::from_raw(out) });
    peak
}

/// The Smudge stroke's plan (no selection, no Sample All Layers).
fn smudge_scratch(doc: &PrecisionDocument) -> u64 {
    let brush = CString::new(HARD).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_smudge_plan(
            doc,
            0,
            brush.as_ptr(),
            SMUDGE_DABS.as_ptr(),
            SMUDGE_DABS.len() / 4,
            false,
            false,
            LIMIT,
            0,
            &mut error,
        )
    };
    assert!(!out.is_null(), "smudge plan: {}", take(error));
    let plan: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    plan["scratch_bytes"].as_u64().unwrap()
}

/// 13B-9's CMYK brush bound (module doc), declared before measuring: every
/// stroke on the CMYK document peaks below `2·N`.
#[test]
fn a_cmyk_brush_dab_peaks_below_two_bytes_a_canvas_pixel() {
    let _serial = serial();
    let doc = cmyk_dab_document();
    let samples = coverage(DAB);
    let dodge = json!({"op":"dodge_burn","layer":0,"exposure":0.5,"range":1,"burn":false});
    let brush = |kind: &str| json!({"op":"retouch_brush","layer":0,"kind":kind,"strength":0.7});
    let requests = [
        ("dodge", dodge.clone()),
        (
            "burn",
            json!({"op":"dodge_burn","layer":0,"exposure":0.5,"range":1,"burn":true}),
        ),
        ("blur", brush("blur")),
        ("sharpen", brush("sharpen")),
        ("desaturate", brush("desaturate")),
    ];
    let mut peaks: Vec<(&str, usize)> = requests
        .iter()
        .map(|(label, request)| (*label, dab_peak(&doc, request, &samples)))
        .collect();
    let dodged = edited(&doc, &dodge, &samples);
    peaks.push(("history brush", history_peak(&dodged, &doc, &samples, None)));
    drop(dodged);
    peaks.push(("smudge", smudge_peak(&doc)));
    for (label, peak) in &peaks {
        println!("cmyk {label} 100x100 at {W}x{H}: peak {peak}");
    }
    for (label, peak) in peaks {
        assert!(
            peak < 2 * N,
            "cmyk {label}: the peak {peak} reaches {}",
            2 * N
        );
    }
}

/// 13B-9's CMYK Smudge plan bound (module doc), declared before measuring:
/// the plan less its canvas display term holds 1.25 × the stroke's counted
/// peak, and the whole plan stays below `30·N`.
#[test]
fn a_cmyk_smudge_plan_holds_its_peak_and_follows_its_footprint() {
    let _serial = serial();
    let doc = cmyk_dab_document();
    let peak = smudge_peak(&doc);
    let plan = smudge_scratch(&doc);
    println!("cmyk smudge 3 dabs at {W}x{H}: peak {peak}, plan {plan}");
    let display = DISPLAY_BYTES_PER_PIXEL * N as u64;
    assert!(
        plan.saturating_sub(display) as f64 >= 1.25 * peak as f64,
        "the plan {plan} less the display term {display} is below 1.25 × the peak {peak}"
    );
    assert!(
        plan < 30 * N as u64,
        "the plan {plan} reaches {}",
        30 * N as u64
    );
}

/// The long stroke of the coverage-brush plan bound (module doc): full
/// coverage over this 1496 × 1196 rect, sent over [`LONG_RECT`].
const LONG_DAB: (u32, u32, u32, u32) = (750, 450, 1496, 1196);
/// [`LONG_DAB`] grown by 2 px: a 1500 × 1200 `coverage_rect`.
const LONG_RECT: (u32, u32, u32, u32) = (748, 448, 1500, 1200);

/// Full coverage over `stroke`, as the samples of `rect` (row-major).
fn rect_coverage(stroke: (u32, u32, u32, u32), (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u16> {
    let (sx, sy, sw, sh) = stroke;
    (0..w * h)
        .map(|i| {
            let (px, py) = (x + i % w, y + i / w);
            if px >= sx && px < sx + sw && py >= sy && py < sy + sh {
                u16::MAX
            } else {
                0
            }
        })
        .collect()
}

/// The History Brush's plan painting `source`'s layer 0 over `doc`'s, its
/// input over `rect` (the canvas when `None`), no selection.
fn history_scratch(
    doc: &PrecisionDocument,
    source: &PrecisionDocument,
    rect: Option<(u32, u32, u32, u32)>,
) -> u64 {
    let rect = rect.map(|(x, y, w, h)| [x, y, w, h]);
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_history_brush_plan(
            doc,
            0,
            source,
            0,
            false,
            rect.as_ref().map_or(ptr::null(), |r| r.as_ptr()),
            LIMIT,
            0,
            &mut error,
        )
    };
    assert!(!out.is_null(), "history brush plan: {}", take(error));
    let plan: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    plan["scratch_bytes"].as_u64().unwrap()
}

/// 13B-9's CMYK coverage-brush plan bounds (module doc), declared before
/// measuring: each brush's rect-form dab peaks below `2·N`, its plan less the
/// canvas display term holds 1.25 × the counted peak, and the whole plan
/// stays below `30·N`; the long stroke holds the same 1.25 × margin. The
/// canvas form's plans (the by-layer bound) are printed for the record.
#[test]
fn a_cmyk_coverage_brush_plan_holds_its_peak_and_follows_its_rect() {
    let _serial = serial();
    let doc = cmyk_dab_document();
    let display = DISPLAY_BYTES_PER_PIXEL * N as u64;
    let dodge = json!({"op":"dodge_burn","layer":0,"exposure":0.5,"range":1,"burn":false});
    let brush = |kind: &str| json!({"op":"retouch_brush","layer":0,"kind":kind,"strength":0.7});
    let requests = [
        ("dodge", dodge.clone()),
        ("blur", brush("blur")),
        ("sharpen", brush("sharpen")),
        ("desaturate", brush("desaturate")),
    ];
    let dodged = edited(&doc, &dodge, &coverage(DAB));
    for (label, request) in &requests {
        let plan = scratch_bytes(&doc, request);
        println!("cmyk {label} canvas form at {W}x{H}: plan {plan}");
    }
    let plan = history_scratch(&dodged, &doc, None);
    println!("cmyk history brush canvas form at {W}x{H}: plan {plan}");
    let mut failures = Vec::new();
    for (stroke, rect, dab) in [(DAB, DAB_RECT, true), (LONG_DAB, LONG_RECT, false)] {
        let samples = rect_coverage(stroke, rect);
        let (x, y, w, h) = rect;
        let mut rows: Vec<(&str, usize, u64)> = requests
            .iter()
            .map(|(label, request)| {
                let mut request = request.clone();
                request["coverage_rect"] = json!([x, y, w, h]);
                let peak = dab_peak(&doc, &request, &samples);
                (*label, peak, scratch_bytes(&doc, &request))
            })
            .collect();
        rows.push((
            "history brush",
            history_peak(&dodged, &doc, &samples, Some(rect)),
            history_scratch(&dodged, &doc, Some(rect)),
        ));
        for (label, peak, plan) in rows {
            println!("cmyk {label} rect form {w}x{h} at {W}x{H}: peak {peak}, plan {plan}");
            let label = format!("cmyk {label} rect form {w}x{h}");
            if dab && peak >= 2 * N {
                failures.push(format!("{label}: the peak {peak} reaches {}", 2 * N));
            }
            if (plan.saturating_sub(display) as f64) < 1.25 * peak as f64 {
                failures.push(format!(
                    "{label}: the plan {plan} less the display term {display} is below 1.25 × \
                     the peak {peak}"
                ));
            }
            if dab && plan >= 30 * N as u64 {
                failures.push(format!(
                    "{label}: the plan {plan} reaches {}",
                    30 * N as u64
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ------------------------------------------------- selected rect dabs --

/// The in-place plan's fixed bookkeeping (13B-9c): its own eight
/// `BOOKKEEPING_BYTES` (1 MiB each) and the `Current` taps' four at the
/// plan's 5/4 margin.
const FIXED: u64 = 13 << 20;
/// The rect a selected dab names: [`DAB`] grown by 2 px.
const DAB_RECT: (u32, u32, u32, u32) = (998, 798, 104, 104);

/// `plane` (canvas-sized) cropped to `rect`, row-major.
fn crop(plane: &[u16], (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u16> {
    let mut out = Vec::with_capacity((w * h) as usize);
    for row in y..y + h {
        let at = (row * W + x) as usize;
        out.extend_from_slice(&plane[at..at + w as usize]);
    }
    out
}

fn scratch_bytes(doc: &PrecisionDocument, request: &Value) -> u64 {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut error) };
    assert!(!out.is_null(), "{request}: {}", take(error));
    let plan: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    plan["scratch_bytes"].as_u64().unwrap()
}

/// 13B-9c's bounds, declared before measuring: a `sample: current` Clone
/// with a selection, its 100 × 100 dab sent in canvas form and in rect form
/// (`coverage_rect` = [`DAB_RECT`]), peaks below `2·N` in both (the
/// canvas-sized selection domain alone reaches it), and the rect form's plan
/// is at least 1.25 × its counted peak and below [`FIXED`] + `2·N`.
fn assert_selected_clone_below_bound(depth: &str, doc: &PrecisionDocument) {
    let stroke = coverage(DAB);
    let gate: Vec<u16> = (0..N).map(|i| ((i % W as usize) * 21 + 1) as u16).collect();
    let request = json!({"op":"retouch_clone","layer":0,"sample":"current","selection":true,
        "matrix":[1.0, 0.0, 0.0, 1.0, 300.0, 0.0],"opacity":1.0});
    let mut canvas = stroke.clone();
    canvas.extend_from_slice(&gate);
    let canvas_peak = dab_peak(doc, &request, &canvas);
    drop(canvas);
    println!("{depth} selected clone 100x100 at {W}x{H}: canvas form peak {canvas_peak}");
    let (x, y, w, h) = DAB_RECT;
    let mut rect = crop(&stroke, DAB_RECT);
    rect.extend(crop(&gate, DAB_RECT));
    let mut rect_request = request.clone();
    rect_request["coverage_rect"] = json!([x, y, w, h]);
    let rect_peak = dab_peak(doc, &rect_request, &rect);
    let plan = scratch_bytes(doc, &rect_request);
    println!("{depth} selected clone 100x100 at {W}x{H}: rect form peak {rect_peak}, plan {plan}");
    assert!(
        canvas_peak < 2 * N,
        "{depth}: the canvas form's peak {canvas_peak} reaches {}",
        2 * N
    );
    assert!(
        rect_peak < 2 * N,
        "{depth}: the rect form's peak {rect_peak} reaches {}",
        2 * N
    );
    assert!(
        plan as f64 >= 1.25 * rect_peak as f64,
        "{depth}: the plan {plan} is below 1.25 × the peak {rect_peak}"
    );
    assert!(
        plan < FIXED + 2 * N as u64,
        "{depth}: the plan {plan} reaches {}",
        FIXED + 2 * N as u64
    );
}

#[test]
fn a_16_bit_selected_clone_peaks_below_two_bytes_a_canvas_pixel() {
    let _serial = serial();
    assert_selected_clone_below_bound("16-bit", &document16(vec![raster16()]));
}

#[test]
fn a_float_selected_clone_peaks_below_two_bytes_a_canvas_pixel() {
    let _serial = serial();
    let doc = float_fixture::float_doc(W, H, &IccProfile::srgb(), &[pattern32()]);
    assert_selected_clone_below_bound("float", &doc);
}

/// The box `BOX` covers, as `[x, y, w, h]`.
fn expected_box() -> [i32; 4] {
    [BOX.0, BOX.1, BOX.2, BOX.3].map(|v| v as i32)
}

#[test]
fn the_align_box_reads_its_mask_in_place_at_both_depths() {
    let _serial = serial();
    // Byte: a canvas-sized mask with coverage inside `BOX` only.
    let mut doc = DrDocument::from_pixels(RgbaImage::new(W, H));
    let pixels: Vec<u8> = (0..N * 4).map(|i| (i * 7 % 251) as u8).collect();
    doc.layers[0].pixels = LayerRaster::new(RgbaImage::from_raw(W, H, pixels).unwrap());
    let mask: Vec<u8> = (0..N)
        .map(|i| {
            if inside(i, BOX) {
                1 + (i % 254) as u8
            } else {
                0
            }
        })
        .collect();
    doc.layers[0].mask = Some(GrayRaster::new(GrayImage::from_raw(W, H, mask).unwrap()));
    doc.layers[0].mask_enabled = true;
    doc.layers[0].mask_properties = MaskProperties::at((0, 0));
    let mut xywh = [0i32; 4];
    let base = begin();
    let found = unsafe { dr_doc_fill_align_box(&doc, 0, xywh.as_mut_ptr()) };
    let byte = peak_since(base);
    println!("byte fill_align_box at {W}x{H}: peak {byte}");
    assert!(found);
    assert_eq!(xywh, expected_box(), "the byte mask's coverage bounds");
    drop(doc);

    // Typed: the same coverage as a u16 mask plane.
    let samples: Vec<u16> = (0..N)
        .map(|i| {
            if inside(i, BOX) {
                1 + (i % 65534) as u16
            } else {
                0
            }
        })
        .collect();
    let plane = PrecisionPlane::from_u16(W, H, samples, None).unwrap();
    let mask = PrecisionMask::new(plane, MaskProperties::at((0, 0)), true).unwrap();
    let typed = document16(vec![raster16().with_mask(Some(mask))]);
    let mut xywh = [0i32; 4];
    let base = begin();
    let found = unsafe { dr_precision_doc_fill_align_box(&typed, 0, xywh.as_mut_ptr()) };
    let typed_peak = peak_since(base);
    println!("typed fill_align_box at {W}x{H}: peak {typed_peak}");
    assert!(found);
    assert_eq!(xywh, expected_box(), "the typed mask's coverage bounds");
    assert!(byte < N / 2, "byte: the peak {byte} reaches {}", N / 2);
    assert!(
        typed_peak < N / 2,
        "typed: the peak {typed_peak} reaches {}",
        N / 2
    );
}

#[test]
fn a_group_mask_thumbnail_places_rows_without_a_plane_copy() {
    let _serial = serial();
    let doc = Box::into_raw(Box::new(DrDocument::from_pixels(RgbaImage::new(W, H))));
    let name = CString::new("Group").unwrap();
    let mut group = usize::MAX;
    let grouped = unsafe {
        dr_doc_group_layers(
            doc,
            [0usize].as_ptr(),
            1,
            name.as_ptr(),
            &mut group,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        )
    };
    unsafe { dr_doc_free(doc) };
    assert!(!grouped.is_null(), "group_layers");
    let plane: Vec<u8> = (0..N)
        .map(|i| if inside(i, BOX) { (i % 251) as u8 } else { 0 })
        .collect();
    // MASK_FROM_SELECTION: the plane as the group's canvas-sized mask.
    let masked = unsafe { dr_doc_adding_layer_mask(grouped, group, 2, plane.as_ptr(), W, H) };
    unsafe { dr_doc_free(grouped) };
    assert!(!masked.is_null(), "group mask");
    drop(plane);
    let base = begin();
    let thumb = unsafe { dr_doc_layer_mask_thumbnail(masked, group, 64) };
    let peak = peak_since(base);
    assert!(!thumb.is_null(), "the group mask's thumbnail");
    let size = unsafe { (dr_image_width(thumb), dr_image_height(thumb)) };
    unsafe { dr_image_free(thumb) };
    unsafe { dr_doc_free(masked) };
    println!("group mask thumbnail at {W}x{H}: peak {peak}");
    assert_eq!(size, (64, 43), "the canvas view's fit");
    assert!(
        peak < N / 2,
        "group thumbnail: the peak {peak} reaches {}",
        N / 2
    );
}

// ------------------------------------------------------ synthesis dabs --

/// The synthesis canvas (module doc).
const SW: u32 = 4000;
const SH: u32 = 3000;
/// Its pixels: the bound in bytes.
const SN: usize = SW as usize * SH as usize;
/// The 48 × 48 hole.
const HOLE: (u32, u32, u32, u32) = (1400, 900, 48, 48);

/// An opaque 16-bit layer of the synthesis canvas, varied in every channel.
fn synthesis16() -> PrecisionDocument {
    let pixels = (0..SW * SH)
        .map(|i| {
            let (x, y) = (i % SW, i / SW);
            [
                ((x * 2711 + y * 911) % 65536) as u16,
                ((x * 523 + y * 3301) % 65536) as u16,
                (((x ^ y) * 1999) % 65536) as u16,
                65535,
            ]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(SW, SH, pixels, IccProfile::srgb(), None)
        .expect("a layer image");
    PrecisionDocument::from_parts(
        SW,
        SH,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("L", image, (0, 0), 1.0, true).expect("a layer")],
    )
    .expect("a document")
}

/// [`synthesis16`] in the synthetic press profile.
fn synthesis_cmyk() -> PrecisionDocument {
    let rgb = synthesis16();
    let profile = cmyk_fixture::press_profile("Press Test");
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            &rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take(error));
    *unsafe { Box::from_raw(out) }
}

/// Full coverage over [`HOLE`] on the synthesis canvas.
fn hole_coverage() -> Vec<u16> {
    let (x, y, w, h) = HOLE;
    (0..SN)
        .map(|i| {
            let (px, py) = ((i % SW as usize) as u32, (i / SW as usize) as u32);
            if px >= x && px < x + w && py >= y && py < y + h {
                u16::MAX
            } else {
                0
            }
        })
        .collect()
}

/// Every synthesis request's peak on `doc` (each printed), then the bound.
fn assert_synthesis_below_bound(depth: &str, doc: &PrecisionDocument) {
    let samples = hole_coverage();
    let (x, y, w, h) = HOLE;
    let rect = json!({"layer":0,"sample":"current","x":x,"y":y,"width":w,"height":h,"seed":7});
    let with = |extra: Value| {
        let mut request = rect.clone();
        for (k, v) in extra.as_object().unwrap() {
            request[k] = v.clone();
        }
        request
    };
    let requests = [
        (
            "spot_heal content_aware",
            with(json!({"op":"spot_heal","type":"content_aware"})),
        ),
        (
            "spot_heal proximity_match",
            with(json!({"op":"spot_heal","type":"proximity_match"})),
        ),
        (
            "content_aware_fill",
            with(json!({"op":"content_aware_fill"})),
        ),
    ];
    let peaks: Vec<(&str, usize)> = requests
        .iter()
        .map(|(label, request)| (*label, dab_peak(doc, request, &samples)))
        .collect();
    for (label, peak) in &peaks {
        println!("{depth} {label} 48x48 at {SW}x{SH}: peak {peak}");
    }
    for (label, peak) in peaks {
        assert!(peak < SN, "{depth} {label}: the peak {peak} reaches {SN}");
    }
}

#[test]
fn a_16_bit_current_synthesis_peaks_below_one_byte_a_canvas_pixel() {
    let _serial = serial();
    assert_synthesis_below_bound("16-bit", &synthesis16());
}

#[test]
fn a_float_current_synthesis_peaks_below_one_byte_a_canvas_pixel() {
    let _serial = serial();
    let doc = synthesis16()
        .promote_to_float(LIMIT, 0, None)
        .expect("a float document");
    assert!(doc.is_float());
    assert_synthesis_below_bound("float", &doc);
}

#[test]
fn a_cmyk_current_synthesis_peaks_below_one_byte_a_canvas_pixel() {
    let _serial = serial();
    let doc = synthesis_cmyk();
    assert!(doc.is_cmyk());
    assert_synthesis_below_bound("cmyk", &doc);
}

/// A whole-canvas Copy of layer 0 (`PrecisionDocument::clipboard_copy`, no
/// coverage, so no encoding): its peak above the live bytes before it,
/// against the bound, `payload` × 5/4.
fn assert_whole_layer_copy_bound(depth: &str, doc: &PrecisionDocument, payload: usize) {
    let read = ClipboardRead::Copy {
        layer: 0,
        x: 0,
        y: 0,
        width: W,
        height: H,
    };
    let base = begin();
    let copied = doc
        .clipboard_copy(&read, None, LIMIT, 0, None)
        .expect("a payload");
    let peak = peak_since(base);
    drop(copied);
    println!("{depth} whole-layer Copy at {W}x{H}: peak {peak}, payload {payload}");
    assert!(
        4 * peak <= 5 * payload,
        "{depth}: the peak {peak} exceeds 1.25 × the payload {payload}"
    );
}

/// The warm whole-layer Copy's bound, declared before measuring: the Copy
/// builds its payload straight into tiles, so its peak is at most 1.25 × the
/// payload (a contiguous staging buffer beside the tiles is 2 ×).
#[test]
fn a_whole_layer_copy_at_16_bits_peaks_at_most_five_quarters_of_its_payload() {
    let _serial = serial();
    assert_whole_layer_copy_bound("16-bit", &document16(vec![raster16()]), N * 8);
}

#[test]
fn a_whole_layer_copy_at_float_peaks_at_most_five_quarters_of_its_payload() {
    let _serial = serial();
    let doc = float_fixture::float_doc(W, H, &IccProfile::srgb(), &[pattern32()]);
    assert_whole_layer_copy_bound("float", &doc, N * 16);
}
