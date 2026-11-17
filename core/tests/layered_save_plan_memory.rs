//! Black-box: every 8- and 16-bit layered PSD, PSB and TIFF save holds its
//! records and ONE file buffer, reads its spot planes in place, and stays
//! inside the plan its export report states as `working_memory_plan_bytes`:
//! RGB (`PsdExportReport::working_plan`: the file, what the save holds beside
//! it, a PSD's matted composite planes, the stamps and the fixed
//! bookkeeping), Lab (the same plan with its three composite planes, a
//! layered TIFF's one encoded row in place of them), CMYK (the measured
//! 68 B a pixel or the counted records and five composite planes, whichever
//! is larger) and Gray and Duotone (`gray_save_plan`). The bound, declared
//! before measuring, is that plan itself: the peak live heap above the bytes
//! live before the save never exceeds it. Each typed save also admits
//! exactly that plan beside the document's retained bytes, read from its own
//! refusal at a limit of 1, except a 16-bit Duotone save, which admits its
//! 8-bit copy first. Each typed save's file is no longer than the report's
//! `worst_case_bytes`. The byte save is measured with its composite supplied
//! (`flat`), as the host supplies it.
//!
//! Every plane is a hashed sample per pixel and channel (an 8-bit Lab
//! document's on the ×257 grid), so the records and the file reach their RLE
//! worst case at 8 bits as at 16; a modular ramp does not fill it at 8. The
//! figures are live heap bytes from the tracking allocator below; the
//! allocator is process-wide, so the binary is its own and every test holds
//! `serial()`.

#[path = "common/cmyk_fixture.rs"]
mod cmyk_fixture;
mod common;
#[path = "common/icc_lut_builders.rs"]
mod icc_lut_builders;

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_duotone::dr_precision_doc_set_duotone;
use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_lab::dr_precision_doc_new_lab;
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

/// The RGB cases' canvas.
const W: u32 = 3000;
const H: u32 = 2000;
/// The colour-mode cases' canvas: every term of their plans but the wide
/// encoder's grows with the pixels, so a smaller canvas reaches the same
/// verdicts.
const MW: u32 = 1500;
const MH: u32 = 1000;
/// The admission limit of the peak cases: high enough that only the bytes
/// are measured.
const LIMIT: u64 = 64 << 30;
/// `DrLayeredContainer`'s PSD and TIFF, and `PsdExportMode`'s Layers and
/// Flattened.
const PSD: i32 = 0;
const TIFF: i32 = 2;
const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;

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

fn mode_name(mode: i32) -> &'static str {
    if mode == FLATTENED {
        "flattened"
    } else {
        "layers"
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

/// A hashed word for pixel `i` of layer `k` (splitmix64's finaliser), four
/// independent 16-bit samples.
fn hashed(i: usize, k: usize) -> u64 {
    let mut j = (i as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add((k as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03));
    j ^= j >> 29;
    j = j.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    j ^ (j >> 32)
}

/// A byte picture whose four planes are hashed.
fn byte_pixels() -> RgbaImage {
    let n = W as usize * H as usize;
    let mut pixels = Vec::with_capacity(n * 4);
    for i in 0..n {
        pixels.extend_from_slice(&hashed(i, 0).to_le_bytes()[..4]);
    }
    RgbaImage::from_raw(W, H, pixels).expect("a canvas")
}

/// A 16-bit layer `k` of `w`×`h` in `profile`: a hashed sample per pixel and
/// channel, each on the ×257 grid of an 8-bit document when `eight`.
fn noisy(w: u32, h: u32, k: usize, eight: bool, profile: Arc<IccProfile>) -> PrecisionImage {
    let n = w as usize * h as usize;
    let rgba: Vec<[u16; 4]> = (0..n)
        .map(|i| {
            let j = hashed(i, k);
            std::array::from_fn(|c| {
                let v = (j >> (16 * c)) as u16;
                if eight {
                    (v >> 8) * 257
                } else {
                    v
                }
            })
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, rgba, profile, None).expect("a 16-bit layer")
}

fn typed_spot(w: u32, h: u32) -> Vec<u16> {
    (0..w as usize * h as usize)
        .map(|i| (i * 40_503 % 65521) as u16)
        .collect()
}

/// A document of `layers` full-canvas raster layers of [`noisy`] samples in
/// `profile`.
fn layered(
    w: u32,
    h: u32,
    layers: usize,
    eight: bool,
    profile: Arc<IccProfile>,
) -> PrecisionDocument {
    let records = (0..layers)
        .map(|k| {
            PrecisionDocumentLayer::raster(
                &format!("Layer {k}"),
                noisy(w, h, k, eight, profile.clone()),
                (0, 0),
                1.0,
                true,
            )
            .expect("a raster layer")
        })
        .collect();
    PrecisionDocument::from_parts(
        w,
        h,
        profile,
        Metadata::default(),
        Resolution::default(),
        records,
    )
    .expect("a layered document")
}

/// A 16-bit RGB document of `layers` layers, with one spot channel when
/// `spot`.
fn rgb_doc(w: u32, h: u32, layers: usize, spot: bool) -> PrecisionDocument {
    let document = layered(w, h, layers, false, IccProfile::srgb());
    if spot {
        document
            .add_spot_channel("Spot", ink(), &typed_spot(w, h), LIMIT, 0, None)
            .expect("a typed spot channel")
    } else {
        document
    }
}

/// [`rgb_doc`] converted to Grayscale (Dot Gain 20 %) at `depth`, with one
/// spot channel when `spot`.
fn gray_doc(w: u32, h: u32, layers: usize, depth: u32, spot: bool) -> PrecisionDocument {
    let rgb = rgb_doc(w, h, layers, false);
    let options =
        CString::new(format!(r#"{{"space":"dot_gain_20","depth":{depth}}}"#)).expect("the options");
    let mut error = ptr::null_mut();
    let gray = unsafe {
        dr_precision_doc_convert_to_gray(
            &rgb,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!gray.is_null(), "to Grayscale: {:?}", take(error));
    let gray = *unsafe { Box::from_raw(gray) };
    if spot {
        gray.add_spot_channel("Spot", ink(), &typed_spot(w, h), LIMIT, 0, None)
            .expect("a Gray spot channel")
    } else {
        gray
    }
}

/// `gray` as a two-ink Duotone (Black and a warm ink, straight curves).
fn duotone_of(gray: &PrecisionDocument) -> PrecisionDocument {
    let line = |low: f64, high: f64| {
        let mut curve = vec![Value::Null; 13];
        curve[0] = json!(low);
        curve[12] = json!(high);
        Value::Array(curve)
    };
    let options = json!({"inks": [
        {"name": "Black", "lab": [0, 0, 0], "curve": line(0.0, 100.0)},
        {"name": "Rust", "lab": [55, 45, 55], "curve": line(0.0, 100.0)}]});
    let options = CString::new(options.to_string()).expect("the options");
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_duotone(gray, options.as_ptr(), &mut error) };
    assert!(!out.is_null(), "to Duotone: {:?}", take(error));
    *unsafe { Box::from_raw(out) }
}

/// The Lab twin profile a Lab document carries.
fn lab_twin() -> Arc<IccProfile> {
    let request = CString::new(r#"{"depth":16,"fill_srgb":[1,1,1,1]}"#).expect("the request");
    let mut error = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(1, 1, request.as_ptr(), LIMIT, 0, &mut error) };
    assert!(!doc.is_null(), "a Lab document: {:?}", take(error));
    let doc = unsafe { Box::from_raw(doc) };
    doc.profile().clone()
}

/// A Lab document at `depth` of `layers` layers of [`noisy`] L, a, b and
/// alpha codes (partial alpha everywhere).
fn lab_doc(w: u32, h: u32, layers: usize, depth: u32) -> PrecisionDocument {
    layered(w, h, layers, depth == 8, lab_twin())
        .with_lab_depth(depth)
        .expect("a Lab document")
}

/// [`rgb_doc`] converted to CMYK (the synthetic press profile) at `depth`,
/// with one spot channel when `spot`.
fn cmyk_doc(w: u32, h: u32, layers: usize, depth: u32, spot: bool) -> PrecisionDocument {
    let rgb = rgb_doc(w, h, layers, false);
    let profile = cmyk_fixture::press_profile("Press Test");
    let options = CString::new(format!(r#"{{"depth":{depth}}}"#)).expect("the options");
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            &rgb,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null(), "to CMYK: {:?}", take(error));
    let cmyk = *unsafe { Box::from_raw(out) };
    if spot {
        cmyk.add_spot_channel("Spot", ink(), &typed_spot(w, h), LIMIT, 0, None)
            .expect("a CMYK spot channel")
    } else {
        cmyk
    }
}

fn typed_report(document: &PrecisionDocument, container: i32, mode: i32) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_precision_doc_psd_export_report(document, container, mode, &mut error) };
    json_of(raw, error)
}

fn typed_save(
    document: &PrecisionDocument,
    projection: Option<&PrecisionImage>,
    path: &Path,
    (container, mode): (i32, i32),
    limit: u64,
) -> Result<(), String> {
    let path = c_path(path);
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            document,
            projection.map_or(ptr::null(), |p| p as *const PrecisionImage),
            path.as_ptr(),
            container,
            mode,
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

/// The figure the save's admission refusal names: the plan + the retained
/// bytes (no other retained bytes are passed).
fn needed_of(message: &str) -> Option<u64> {
    message
        .strip_prefix("precision operation needs ")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|figure| figure.parse().ok())
}

/// Each `(container, mode)` save of `document`: it succeeds at [`LIMIT`];
/// the file it writes is no longer than the report's `worst_case_bytes`
/// (the file term of every plan); its peak stays inside the scratch it
/// admits (the refusal at a limit of 1 less the document's retained bytes);
/// its report states the plan; and the admitted scratch IS that plan.
/// `two_step` (a 16-bit Duotone, which admits its 8-bit copy before the
/// copy's save) asserts the peak against the plan instead. Each failure is
/// appended to `failures`, so a test reports every case's together
/// ([`no_failures`]).
fn typed_saves_hold_their_plan(
    case: &str,
    document: &PrecisionDocument,
    projection: Option<&PrecisionImage>,
    saves: &[(i32, i32)],
    two_step: bool,
    failures: &mut Vec<String>,
) {
    let folder = tempfile::TempDir::new().expect("a scratch folder");
    for &(container, mode) in saves {
        let label = format!("{case} {} {}", name(container), mode_name(mode));
        let report = typed_report(document, container, mode);
        assert_eq!(report["available"], Value::Bool(true), "{label}: {report}");
        let path = folder
            .path()
            .join(format!("{}-{}.out", name(container), mode));
        let admitted = if two_step {
            None
        } else {
            let message = typed_save(document, projection, &path, (container, mode), 1)
                .expect_err("a limit of 1 refuses");
            let needed = needed_of(&message)
                .unwrap_or_else(|| panic!("{label}: not the admission refusal: {message}"));
            Some(needed - document.retained_bytes())
        };
        let (peak, saved) =
            measured(|| typed_save(document, projection, &path, (container, mode), LIMIT));
        if let Err(e) = saved {
            failures.push(format!("{label}: the save: {e}"));
            continue;
        }
        match (
            std::fs::metadata(&path).map(|file| file.len()),
            report["worst_case_bytes"].as_u64(),
        ) {
            (Ok(written), Some(worst)) if written > worst => failures.push(format!(
                "{label}: the file is {written} bytes, above the report's worst case {worst} (by {})",
                written - worst
            )),
            (Err(e), _) => failures.push(format!("{label}: the written file: {e}")),
            (_, None) => failures.push(format!("{label}: the report states no worst case")),
            _ => {}
        }
        let peak = peak as u64;
        let plan = report["working_memory_plan_bytes"].as_u64();
        if let Some(admitted) = admitted {
            if peak > admitted {
                failures.push(format!(
                    "{label}: the save peaks at {peak} above the {admitted} it admits (by {})",
                    peak - admitted
                ));
            }
        }
        match (plan, admitted) {
            (None, _) => failures.push(format!("{label}: the report states no plan")),
            (Some(plan), Some(admitted)) if admitted != plan => failures.push(format!(
                "{label}: the save admits {admitted}, not the plan {plan} its report states"
            )),
            (Some(plan), None) if peak > plan => failures.push(format!(
                "{label}: the save peaks at {peak} above its plan {plan} (by {})",
                peak - plan
            )),
            _ => {}
        }
    }
}

fn no_failures(failures: &[String]) {
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The layered PSD and TIFF of `document`.
const LAYERED: [(i32, i32); 2] = [(TIFF, LAYERS), (PSD, LAYERS)];

/// The byte save of `document` with its composite supplied, each container
/// in `containers`: its peak stays inside the report's plan.
fn byte_saves_peak_within_the_plan(document: &DrDocument, picture: &RgbaImage, containers: &[i32]) {
    let flat = unsafe { dr_image_from_rgba(picture.as_raw().as_ptr(), W, H) };
    assert!(!flat.is_null());
    let folder = tempfile::TempDir::new().expect("a scratch folder");
    let mut failures = Vec::new();
    for &container in containers {
        let mut error = ptr::null_mut();
        let raw = unsafe { dr_doc_psd_export_report(document, container, LAYERS, &mut error) };
        let plan = plan_of(&json_of(raw, error));
        let path = c_path(&folder.path().join(format!("byte-{}.out", name(container))));
        let mut error = ptr::null_mut();
        let (peak, ok) = measured(|| unsafe {
            dr_doc_save_psd(
                document,
                flat,
                path.as_ptr(),
                container,
                LAYERS,
                ptr::null(),
                &mut error,
            )
        });
        assert!(ok, "byte {}: the save: {:?}", name(container), take(error));
        if peak > plan {
            failures.push(format!(
                "byte {}: the save peaks at {peak} above its plan {plan} (by {})",
                name(container),
                peak - plan
            ));
        }
    }
    unsafe { dr_image_free(flat) };
    no_failures(&failures);
}

/// `document`'s host projection (RGBA16 in its own profile).
fn projection_of(document: &PrecisionDocument) -> PrecisionImage {
    document.render(LIMIT, 0, None).expect("the projection")
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
    byte_saves_peak_within_the_plan(&document, &picture, &[TIFF, PSD]);
}

/// With no spot channel there is no spot-row header term in the plan, so
/// the one encoded composite row in flight, the colour transform and the
/// record headers are the bookkeeping term's alone.
#[test]
fn byte_layered_psd_without_a_spot_channel_peaks_within_the_plan() {
    let _serial = serial();
    let picture = byte_pixels();
    let document = DrDocument::from_pixels(picture.clone());
    byte_saves_peak_within_the_plan(&document, &picture, &[PSD]);
}

#[test]
fn sixteen_bit_layered_saves_with_a_spot_channel_peak_within_the_plan() {
    let _serial = serial();
    let document = rgb_doc(W, H, 1, true);
    let projection = projection_of(&document);
    let mut failures = Vec::new();
    typed_saves_hold_their_plan(
        "16-bit spot",
        &document,
        Some(&projection),
        &LAYERED,
        false,
        &mut failures,
    );
    no_failures(&failures);
}

#[test]
fn sixteen_bit_layered_saves_of_three_layers_peak_within_the_plan() {
    let _serial = serial();
    let document = rgb_doc(W, H, 3, false);
    let projection = projection_of(&document);
    let mut failures = Vec::new();
    typed_saves_hold_their_plan(
        "16-bit three layers",
        &document,
        Some(&projection),
        &LAYERED,
        false,
        &mut failures,
    );
    no_failures(&failures);
}

#[test]
fn sixteen_bit_layered_psd_without_a_spot_channel_holds_its_plan() {
    let _serial = serial();
    let document = rgb_doc(W, H, 1, false);
    let projection = projection_of(&document);
    let mut failures = Vec::new();
    typed_saves_hold_their_plan(
        "16-bit no spot",
        &document,
        Some(&projection),
        &[(PSD, LAYERS)],
        false,
        &mut failures,
    );
    no_failures(&failures);
}

/// Lab documents at `depth` of one and of three layers, their layered PSD
/// and TIFF.
fn lab_layered_cases(depth: u32) {
    let mut failures = Vec::new();
    for layers in [1, 3] {
        let document = lab_doc(MW, MH, layers, depth);
        let projection = projection_of(&document);
        let case = format!("Lab {depth}-bit {layers} layer(s)");
        typed_saves_hold_their_plan(
            &case,
            &document,
            Some(&projection),
            &LAYERED,
            false,
            &mut failures,
        );
    }
    no_failures(&failures);
}

#[test]
fn lab_layered_saves_hold_their_plan_at_eight_bits() {
    let _serial = serial();
    lab_layered_cases(8);
}

#[test]
fn lab_layered_saves_hold_their_plan_at_sixteen_bits() {
    let _serial = serial();
    lab_layered_cases(16);
}

#[test]
fn lab_flattened_saves_hold_their_plan() {
    let _serial = serial();
    let document = lab_doc(MW, MH, 1, 16);
    let projection = projection_of(&document);
    let mut failures = Vec::new();
    typed_saves_hold_their_plan(
        "Lab 16-bit",
        &document,
        Some(&projection),
        &[(TIFF, FLATTENED), (PSD, FLATTENED)],
        false,
        &mut failures,
    );
    no_failures(&failures);
}

/// A Lab layered TIFF as wide as the format stores (30000 pixels a side,
/// `LayeredContainer::max_side`): the strip row's matting scratch is fixed
/// whatever the width and its one encoded row is charged in the plan.
#[test]
fn wide_lab_layered_tiffs_hold_their_plan() {
    let _serial = serial();
    let mut failures = Vec::new();
    for depth in [16, 8] {
        let document = lab_doc(30_000, 133, 1, depth);
        let projection = projection_of(&document);
        let case = format!("wide Lab {depth}-bit");
        typed_saves_hold_their_plan(
            &case,
            &document,
            Some(&projection),
            &[(TIFF, LAYERS)],
            false,
            &mut failures,
        );
    }
    no_failures(&failures);
}

/// Past the measured 68 B a pixel: at 16 bits from six full-canvas layers,
/// at 8 bits from thirteen, the records and the five composite planes are
/// the plan.
#[test]
fn cmyk_layered_psds_of_many_layers_hold_their_plan() {
    let _serial = serial();
    let mut failures = Vec::new();
    for (depth, layers) in [(16, 6), (8, 13)] {
        let document = cmyk_doc(MW, MH, layers, depth, false);
        let case = format!("CMYK {depth}-bit {layers} layers");
        typed_saves_hold_their_plan(
            &case,
            &document,
            None,
            &[(PSD, LAYERS)],
            false,
            &mut failures,
        );
    }
    no_failures(&failures);
}

#[test]
fn cmyk_one_layer_saves_hold_their_plan() {
    let _serial = serial();
    let mut failures = Vec::new();
    for spot in [false, true] {
        let document = cmyk_doc(MW, MH, 1, 16, spot);
        let case = format!("CMYK 16-bit one layer, spot {spot}");
        typed_saves_hold_their_plan(&case, &document, None, &LAYERED, false, &mut failures);
    }
    no_failures(&failures);
}

#[test]
fn gray_layered_saves_hold_their_plan() {
    let _serial = serial();
    let mut failures = Vec::new();
    for depth in [8, 16] {
        let document = gray_doc(MW, MH, 3, depth, false);
        let projection = projection_of(&document);
        let case = format!("Gray {depth}-bit three layers");
        typed_saves_hold_their_plan(
            &case,
            &document,
            Some(&projection),
            &LAYERED,
            false,
            &mut failures,
        );
    }
    // A Gray layered TIFF refuses a spot channel by name.
    let document = gray_doc(MW, MH, 3, 16, true);
    let projection = projection_of(&document);
    typed_saves_hold_their_plan(
        "Gray 16-bit three layers, spot",
        &document,
        Some(&projection),
        &[(PSD, LAYERS)],
        false,
        &mut failures,
    );
    no_failures(&failures);
}

#[test]
fn duotone_layered_psds_hold_their_plan() {
    let _serial = serial();
    let mut failures = Vec::new();
    for depth in [8, 16] {
        let document = duotone_of(&gray_doc(MW, MH, 3, depth, false));
        let projection = projection_of(&document);
        let case = format!("Duotone {depth}-bit three layers");
        typed_saves_hold_their_plan(
            &case,
            &document,
            Some(&projection),
            &[(PSD, LAYERS)],
            depth == 16,
            &mut failures,
        );
    }
    no_failures(&failures);
}

/// A flattened Gray or Duotone file's one composite record writes its alpha
/// beside the gray (`channels_gray`), though the merged composite carries
/// none, so the report's worst case counts both planes.
#[test]
fn gray_and_duotone_flattened_saves_hold_their_plan() {
    let _serial = serial();
    let mut failures = Vec::new();
    let document = gray_doc(MW, MH, 1, 16, false);
    let projection = projection_of(&document);
    typed_saves_hold_their_plan(
        "Gray 16-bit",
        &document,
        Some(&projection),
        &[(TIFF, FLATTENED), (PSD, FLATTENED)],
        false,
        &mut failures,
    );
    let document = duotone_of(&gray_doc(MW, MH, 1, 8, false));
    let projection = projection_of(&document);
    typed_saves_hold_their_plan(
        "Duotone 8-bit",
        &document,
        Some(&projection),
        &[(PSD, FLATTENED)],
        false,
        &mut failures,
    );
    no_failures(&failures);
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
    let document = rgb_doc(w, h, 1, true);
    let projection = projection_of(&document);
    let folder = tempfile::TempDir::new().expect("a scratch folder");
    let mut retained = Vec::new();
    for container in [TIFF, PSD] {
        let plan = plan_of(&typed_report(&document, container, LAYERS)) as u64;
        let path = folder
            .path()
            .join(format!("boundary-{}.out", name(container)));
        let save = |limit: u64| {
            typed_save(
                &document,
                Some(&projection),
                &path,
                (container, LAYERS),
                limit,
            )
        };
        let refusal = |limit: u64, needed: u64| {
            format!(
                "precision operation needs {needed} bytes including retained inputs; working limit is {limit} bytes"
            )
        };
        let message = save(1).expect_err("a limit of 1 refuses");
        let needed = needed_of(&message)
            .unwrap_or_else(|| panic!("{}: not the admission refusal: {message}", name(container)));
        assert_eq!(message, refusal(1, needed), "{}", name(container));
        assert_eq!(
            save(needed - 1),
            Err(refusal(needed - 1, needed)),
            "{}: one byte under the admitted total",
            name(container)
        );
        assert!(
            !path.exists(),
            "{}: a refused save writes no file",
            name(container)
        );
        save(needed)
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
