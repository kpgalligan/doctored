//! Black-box: an RGB document's layered PSD and TIFF save at 8 and 16 bits
//! holds its records and ONE file buffer, reads its spot planes in place, and
//! stays inside the plan its export report states as
//! `working_memory_plan_bytes` (`PsdExportReport::working_plan`: the file,
//! what the save holds beside it, a PSD's matted composite planes, the
//! stamps). The bound, declared before measuring, is that plan itself: the
//! peak live heap above the bytes live before the save never exceeds it.
//! The byte save is measured with its composite supplied (`flat`), as the
//! host supplies it. The 16-bit save admits exactly that plan beside the
//! document's retained bytes, read at the boundary from its own refusal.
//!
//! Every plane is non-uniform and poorly compressible (modular fills), so
//! the records and the file reach their worst case. The figures are live
//! heap bytes from the tracking allocator below; the allocator is
//! process-wide, so the binary is its own and every test holds `serial()`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_psd::{
    dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_psd_export_report,
    dr_precision_doc_save_psd,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::spot_channel::SpotInk;
use image::RgbaImage;
use serde_json::Value;

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
/// The admission limit of the peak cases: high enough that only the bytes
/// are measured.
const LIMIT: u64 = 64 << 30;
/// `DrLayeredContainer`'s PSD and TIFF, and `PsdExportMode`'s Layers.
const PSD: i32 = 0;
const TIFF: i32 = 2;
const LAYERS: i32 = 0;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn take(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn json_of(raw: *mut c_char, error: *mut c_char) -> Value {
    assert!(!raw.is_null(), "the export report: {:?}", take(error));
    let text = take(raw).unwrap_or_default();
    serde_json::from_str(&text).expect("the report's JSON")
}

fn plan_of(report: &Value) -> usize {
    assert_eq!(report["available"], Value::Bool(true), "{report}");
    report["working_memory_plan_bytes"]
        .as_u64()
        .unwrap_or_else(|| panic!("no working_memory_plan_bytes: {report}")) as usize
}

fn name(container: i32) -> &'static str {
    if container == TIFF {
        "TIFF"
    } else {
        "PSD"
    }
}

fn c_path(path: &Path) -> CString {
    CString::new(path.to_str().expect("a UTF-8 path")).expect("a C path")
}

/// The peak live heap above the bytes live before `call`, and its answer.
fn measured<T>(call: impl FnOnce() -> T) -> (usize, T) {
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let out = call();
    (PEAK.load(Ordering::Relaxed).saturating_sub(baseline), out)
}

fn ink() -> SpotInk {
    SpotInk::new([55.25, 70.5, -30.75], 40).expect("an ink")
}

/// A byte picture whose four planes are modular ramps (no repeat of three).
fn byte_pixels() -> RgbaImage {
    let n = W as usize * H as usize;
    let pixels: Vec<u8> = (0..n * 4).map(|i| (i * 7 % 251) as u8).collect();
    RgbaImage::from_raw(W, H, pixels).expect("a canvas")
}

/// A 16-bit layer `k` of `w`×`h` whose four planes are modular ramps.
fn typed_image(w: u32, h: u32, k: usize) -> PrecisionImage {
    let n = w as usize * h as usize;
    let rgba: Vec<[u16; 4]> = (0..n)
        .map(|i| {
            [
                ((i * 31 + k * 1009) % 65521) as u16,
                ((i * 17 + k * 7919) % 65519) as u16,
                ((i + k * 104_729) % 65535) as u16,
                ((i * 23 + k * 3) % 65497) as u16,
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, rgba, IccProfile::srgb(), None).expect("a 16-bit layer")
}

fn typed_spot(w: u32, h: u32) -> Vec<u16> {
    (0..w as usize * h as usize)
        .map(|i| (i * 40_503 % 65521) as u16)
        .collect()
}

/// A 16-bit document of one raster layer and one spot channel.
fn typed_with_spot(w: u32, h: u32) -> PrecisionDocument {
    PrecisionDocument::from_image(
        typed_image(w, h, 0),
        Metadata::default(),
        Resolution::default(),
    )
    .expect("a 16-bit document")
    .add_spot_channel("Spot", ink(), &typed_spot(w, h), LIMIT, 0, None)
    .expect("a typed spot channel")
}

fn typed_report(document: &PrecisionDocument, container: i32) -> Value {
    let mut error = ptr::null_mut();
    let raw =
        unsafe { dr_precision_doc_psd_export_report(document, container, LAYERS, &mut error) };
    json_of(raw, error)
}

fn typed_save(
    document: &PrecisionDocument,
    projection: &PrecisionImage,
    path: &Path,
    container: i32,
    limit: u64,
) -> Result<(), String> {
    let path = c_path(path);
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            document,
            projection,
            path.as_ptr(),
            container,
            LAYERS,
            limit,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let message = take(error);
    if ok {
        Ok(())
    } else {
        Err(message.unwrap_or_else(|| "<no error message>".into()))
    }
}

/// Each container's 16-bit save of `document`: it succeeds and its peak
/// stays inside the report's plan.
fn typed_saves_peak_within_the_plan(document: &PrecisionDocument, case: &str) {
    let folder = tempfile::TempDir::new().expect("a scratch folder");
    let projection = document.render(LIMIT, 0, None).expect("the projection");
    for container in [TIFF, PSD] {
        let plan = plan_of(&typed_report(document, container));
        let path = folder
            .path()
            .join(format!("{case}-{}.out", name(container)));
        let (peak, saved) = measured(|| typed_save(document, &projection, &path, container, LIMIT));
        saved.unwrap_or_else(|e| panic!("{case} {}: the save: {e}", name(container)));
        assert!(
            peak <= plan,
            "{case} {}: the save peaks at {peak} above its plan {plan} (by {})",
            name(container),
            peak - plan
        );
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn byte_layered_saves_with_a_spot_channel_peak_within_the_plan() {
    let _serial = serial();
    let n = W as usize * H as usize;
    let coverage: Vec<u8> = (0..n).map(|i| (i * 37 % 256) as u8).collect();
    let picture = byte_pixels();
    let document = DrDocument::from_pixels(picture.clone())
        .add_spot_channel("Spot", ink(), &coverage)
        .expect("a byte spot channel");
    // The host's composite: the one layer's own picture.
    let flat = unsafe { dr_image_from_rgba(picture.as_raw().as_ptr(), W, H) };
    assert!(!flat.is_null());
    drop(picture);
    let folder = tempfile::TempDir::new().expect("a scratch folder");
    for container in [TIFF, PSD] {
        let mut error = ptr::null_mut();
        let raw = unsafe { dr_doc_psd_export_report(&document, container, LAYERS, &mut error) };
        let plan = plan_of(&json_of(raw, error));
        let path = c_path(&folder.path().join(format!("byte-{}.out", name(container))));
        let mut error = ptr::null_mut();
        let (peak, ok) = measured(|| unsafe {
            dr_doc_save_psd(
                &document,
                flat,
                path.as_ptr(),
                container,
                LAYERS,
                ptr::null(),
                &mut error,
            )
        });
        assert!(ok, "byte {}: the save: {:?}", name(container), take(error));
        assert!(
            peak <= plan,
            "byte {}: the save peaks at {peak} above its plan {plan} (by {})",
            name(container),
            peak - plan
        );
    }
    unsafe { dr_image_free(flat) };
}

#[test]
fn sixteen_bit_layered_saves_with_a_spot_channel_peak_within_the_plan() {
    let _serial = serial();
    typed_saves_peak_within_the_plan(&typed_with_spot(W, H), "16-bit spot");
}

#[test]
fn sixteen_bit_layered_saves_of_three_layers_peak_within_the_plan() {
    let _serial = serial();
    let layers = (0..3)
        .map(|k| {
            PrecisionDocumentLayer::raster(
                &format!("Layer {k}"),
                typed_image(W, H, k),
                (0, 0),
                1.0,
                true,
            )
            .expect("a raster layer")
        })
        .collect();
    let document = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("a three-layer document");
    typed_saves_peak_within_the_plan(&document, "16-bit three layers");
}

/// The 16-bit save admits the report's plan beside the document's retained
/// bytes, at the boundary: N read from the refusal at `limit` 1 is refused
/// at N − 1 (no file) and written at N, and N − the plan is the same
/// figure, the retained bytes, for a TIFF and a PSD, whose plans differ by
/// the PSD's composite planes and by what each holds beside its file. No
/// adjustment layer, so no stamp admits on its own.
#[test]
fn the_sixteen_bit_save_admits_exactly_its_reported_plan() {
    let _serial = serial();
    let (w, h) = (640, 480);
    let document = typed_with_spot(w, h);
    let projection = document.render(LIMIT, 0, None).expect("the projection");
    let folder = tempfile::TempDir::new().expect("a scratch folder");
    let mut retained = Vec::new();
    for container in [TIFF, PSD] {
        let plan = plan_of(&typed_report(&document, container)) as u64;
        let path = folder
            .path()
            .join(format!("boundary-{}.out", name(container)));
        let refusal = |limit: u64, needed: u64| {
            format!(
                "precision operation needs {needed} bytes including retained inputs; working limit is {limit} bytes"
            )
        };
        let message = typed_save(&document, &projection, &path, container, 1)
            .expect_err("a limit of 1 refuses");
        let needed: u64 = message
            .strip_prefix("precision operation needs ")
            .and_then(|rest| rest.split(' ').next())
            .and_then(|figure| figure.parse().ok())
            .unwrap_or_else(|| panic!("{}: not the admission refusal: {message}", name(container)));
        assert_eq!(message, refusal(1, needed), "{}", name(container));
        assert_eq!(
            typed_save(&document, &projection, &path, container, needed - 1),
            Err(refusal(needed - 1, needed)),
            "{}: one byte under the admitted total",
            name(container)
        );
        assert!(
            !path.exists(),
            "{}: a refused save writes no file",
            name(container)
        );
        typed_save(&document, &projection, &path, container, needed)
            .unwrap_or_else(|e| panic!("{}: the save at its admitted total: {e}", name(container)));
        assert!(
            path.exists(),
            "{}: the save at its total writes",
            name(container)
        );
        retained.push(needed as i64 - plan as i64);
    }
    assert_eq!(
        retained[0], retained[1],
        "the TIFF and the PSD admit their plans beside the same retained bytes"
    );
    assert!(retained[0] > 0, "the document retains its layer and plane");
}
