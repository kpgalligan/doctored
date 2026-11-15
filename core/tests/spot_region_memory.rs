//! Black-box: a byte document's REGION spot overprint
//! (`dr_doc_spot_overprint_region`, the regional render's spot pass) reads
//! each spot channel's tiles in place (13B-4), so its heap peak follows the
//! region, never the canvas. The bound, declared before measuring: the peak
//! above the live bytes before the call stays below ONE spot channel's
//! canvas-sized plane (`w·h` bytes), which a whole-plane copy of a channel
//! reaches by itself. The bytes are the full overprint's samples at each
//! region. The figures are live heap bytes from the tracking allocator below;
//! the allocator is process-wide, so the binary is its own and every test
//! holds `serial()`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{c_char, CStr};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_spot_channel::dr_doc_spot_overprint;
use doctored_core::ffi_spot_merge::dr_doc_spot_overprint_region;
use doctored_core::spot_channel::SpotInk;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};

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

const W: u32 = 2400;
const H: u32 = 1600;
/// One spot channel's canvas-sized coverage plane, in bytes.
const PLANE: usize = W as usize * H as usize;

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

fn base() -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        Rgba([(x * 3 + y) as u8, (y * 7 + 11) as u8, (x ^ y) as u8, 255])
    })
}

fn coverage(seed: u32) -> Vec<u8> {
    (0..W * H)
        .map(|i| (i.wrapping_mul(2_654_435_761).wrapping_add(seed) >> 24) as u8)
        .collect()
}

fn image_of(pixels: &RgbaImage) -> *mut DrImage {
    unsafe { dr_image_from_rgba(pixels.as_raw().as_ptr(), pixels.width(), pixels.height()) }
}

fn read(image: *mut DrImage, w: u32, h: u32) -> Vec<u8> {
    let out =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(image), (w * h * 4) as usize) }
            .to_vec();
    unsafe { dr_image_free(image) };
    out
}

/// The `(x, y, divisor)` region of the `W`-wide `full` (`w`×`h` output).
fn sampled(full: &[u8], x: u32, y: u32, d: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for j in 0..h {
        for i in 0..w {
            let at = (((y + j * d) * W + x + i * d) * 4) as usize;
            out.extend_from_slice(&full[at..at + 4]);
        }
    }
    out
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_region_overprint_peaks_below_one_coverage_plane() {
    let _serial = serial();
    let doc = DrDocument::from_pixels(base())
        .add_spot_channel(
            "Red",
            SpotInk::new([54.29, 80.81, 69.89], 40).unwrap(),
            &coverage(1),
        )
        .unwrap()
        .add_spot_channel(
            "Blue",
            SpotInk::new([30.0, 20.0, -60.0], 0).unwrap(),
            &coverage(0x9e37),
        )
        .unwrap();
    let mut err = ptr::null_mut();
    let source = image_of(&base());
    let full = unsafe { dr_doc_spot_overprint(&doc, source, &mut err) };
    assert!(!full.is_null(), "the full overprint: {}", take(err));
    let full = read(full, W, H);
    unsafe { dr_image_free(source) };
    let canvas = base();
    // A 256 × 256 window across tile edges, a whole-canvas preview at
    // divisor 8, and an off-grid window at divisor 3.
    for (x, y, d, w, h) in [
        (1000, 700, 1, 256, 256),
        (0, 0, 8, W / 8, H / 8),
        (129, 300, 3, 300, 200),
    ] {
        let crop = RgbaImage::from_fn(w, h, |i, j| *canvas.get_pixel(x + i * d, y + j * d));
        let region = image_of(&crop);
        let baseline = LIVE.load(Ordering::Relaxed);
        PEAK.store(baseline, Ordering::Relaxed);
        let out = unsafe { dr_doc_spot_overprint_region(&doc, region, x, y, d, &mut err) };
        let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
        unsafe { dr_image_free(region) };
        assert!(!out.is_null(), "({x},{y}) /{d}: {}", take(err));
        assert!(
            read(out, w, h) == sampled(&full, x, y, d, w, h),
            "({x},{y}) /{d}: the full overprint's samples"
        );
        assert!(
            peak < PLANE,
            "({x},{y}) /{d}: the peak {peak} reaches one coverage plane ({PLANE} bytes)"
        );
    }
}
