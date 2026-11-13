//! Black-box suite for 13B-3e-4c-1: the ONE whole-plane view read left, the
//! exception kept by decision (13B-3e-4c-1 D2) that the Develop preview binds
//! (the live-filter preview, its other reader, was removed 2026-10-10).
//! The develop preview (`dr_precision_image_develop_preview`) is its public
//! reader here, so this suite pins the view-cache contract through it:
//!
//! - (a) a SEEDED plane (`PrecisionImage::from_rgba16`) serves the preview
//!   from its seed — no whole-plane allocation, the cache unchanged;
//! - (b) an UNSEEDED plane (`from_rgba16_unseeded`), or a seeded one after
//!   an eviction, materializes ONE view on its first preview (the cache
//!   grows by `w·h·8`, the peak is at least that) and the second preview
//!   hits it (no whole-plane allocation, the cache unchanged, equal bytes).
//!
//! Oracles are independent of the implementation: the fixture is a
//! patterned `Vec` built HERE, the preview bytes are the cell-centre sample
//! of each 4×4 cell rounded to bytes HERE, and the memory figures are live
//! heap bytes from the tracking allocator below against `w·h·8`. It uses
//! only APIs present at `9a30000` and passes there. The view cache is
//! process-wide, so every test holds `serial()`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{c_char, CStr};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_develop_inspect::dr_precision_image_develop_preview;
use doctored_core::icc::IccProfile;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};

#[path = "common/resource_report.rs"]
mod resource_report;

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

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 600;
const H: u32 = 520;
const DIVISOR: u32 = 4;
/// One whole-plane u16 view of the fixture.
const VIEW: u64 = W as u64 * H as u64 * 8;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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

fn pattern(width: u32, height: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            [
                (i.wrapping_mul(977) & 0xffff) as u16,
                (65535 - (i.wrapping_mul(131) & 0xffff)) as u16,
                ((i ^ 0x5a5a) & 0xffff) as u16,
                if i.is_multiple_of(3) {
                    0
                } else {
                    (30000 + i % 30000) as u16
                },
            ]
        })
        .collect()
}

/// The preview's bytes, written HERE: per `DIVISOR` cell of the whole
/// image, the sample at the cell centre `(x0 + (x1 − x0)/2, y0 + (y1 −
/// y0)/2)` rounded to the nearest byte per channel.
fn oracle(samples: &[[u16; 4]], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for y0 in (0..height).step_by(DIVISOR as usize) {
        let y1 = (y0 + DIVISOR).min(height);
        for x0 in (0..width).step_by(DIVISOR as usize) {
            let x1 = (x0 + DIVISOR).min(width);
            let (x, y) = (x0 + (x1 - x0) / 2, y0 + (y1 - y0) / 2);
            let sample = samples[y as usize * width as usize + x as usize];
            out.extend(sample.map(|v| (f64::from(v) / 257.0).round() as u8));
        }
    }
    out
}

/// One `dr_precision_image_develop_preview(image, 0, 0, w, h, 4, 0, …)`:
/// its bytes and the heap peak above the live bytes before the call.
fn preview(image: &PrecisionImage) -> (Vec<u8>, u64) {
    let (w, h) = image.dimensions();
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_image_develop_preview(
            image,
            0,
            0,
            w,
            h,
            DIVISOR,
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline) as u64;
    assert!(!result.is_null(), "a preview: {}", take(error));
    let (width, height) = unsafe { (dr_image_width(result), dr_image_height(result)) };
    assert_eq!((width, height), (w.div_ceil(DIVISOR), h.div_ceil(DIVISOR)));
    let bytes = unsafe {
        std::slice::from_raw_parts(
            dr_image_pixels_rgba(result),
            width as usize * height as usize * 4,
        )
    }
    .to_vec();
    unsafe { dr_image_free(result) };
    (bytes, peak)
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_seeded_plane_serves_the_preview_from_its_seed() {
    let _serial = serial();
    evict();
    let samples = pattern(W, H);
    let expected = oracle(&samples, W, H);
    assert_eq!(samples.capacity(), samples.len());
    let image = PrecisionImage::from_rgba16(W, H, samples, IccProfile::srgb(), None).unwrap();
    assert_eq!(contiguous_view_bytes(), VIEW, "the seed is the one view");
    let (bytes, peak) = preview(&image);
    assert_eq!(bytes, expected);
    assert!(peak < VIEW, "the seed serves the preview: peak {peak}");
    assert_eq!(contiguous_view_bytes(), VIEW, "the views are unchanged");
}

#[test]
fn an_unseeded_or_evicted_plane_materializes_one_view_on_its_first_preview() {
    let _serial = serial();
    evict();
    let samples = pattern(W, H);
    let expected = oracle(&samples, W, H);
    let unseeded =
        PrecisionImage::from_rgba16_unseeded(W, H, samples.clone(), IccProfile::srgb(), None)
            .unwrap();
    assert_eq!(
        contiguous_view_bytes(),
        0,
        "an unseeded plane holds no view"
    );
    let seeded = PrecisionImage::from_rgba16(W, H, samples, IccProfile::srgb(), None).unwrap();
    for (name, image) in [("unseeded", unseeded), ("evicted", seeded)] {
        evict();
        assert_eq!(contiguous_view_bytes(), 0, "{name}: no view before");
        let (first, peak) = preview(&image);
        assert_eq!(first, expected, "{name}: first preview");
        assert_eq!(
            contiguous_view_bytes(),
            VIEW,
            "{name}: the first preview materializes one view"
        );
        assert!(peak >= VIEW, "{name}: the materialization peak {peak}");
        let (second, peak) = preview(&image);
        assert_eq!(second, first, "{name}: the second preview's bytes");
        assert!(peak < VIEW, "{name}: the second preview hits: peak {peak}");
        assert_eq!(
            contiguous_view_bytes(),
            VIEW,
            "{name}: the views are unchanged"
        );
    }
}
