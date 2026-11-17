//! The transient copies 13B-4 introduced, each beside the plan that admits
//! its operation (`designs/phase-13b-tiled-status.md`, the transient-copy
//! line under #10): the peak live heap above the baseline during each call
//! and that plan's `scratch_bytes`, per canvas size. A counting
//! `#[global_allocator]`; nothing is timed. Each JSON line is one row:
//! `plan_scratch` (0 where the operation admits no plan), the measured
//! `peak_above_baseline` and their difference.
//!
//! - the selected-plane mask move (`move_selected`, `target: "mask"`) at
//!   8 and 16 bits: the stored plane's copy;
//! - a typed 0.9× resize of a masked layer and a typed `set_channel`: the
//!   mask's resample copy and `PrecisionPlane::from_u16` holding its input
//!   beside its tiles (`precision_geometry_plan`'s comment);
//! - the Align with Layer box at both depths, read in place since 13B-9a
//!   (no plan);
//! - a byte Levels Auto over a masked adjustment layer (the effective
//!   mask's copy in `scoped_auto`, beside `scoped_analysis_plan`);
//! - a layered TIFF save with one spot channel, 8-bit (`dr_doc_save_psd`,
//!   its composite supplied as the host supplies it) and 16-bit
//!   (`dr_precision_doc_save_psd`, which admits that plan), each beside its
//!   export report's `working_memory_plan_bytes`: the records and one file
//!   buffer, the spot planes read in place (`tiff_spot`'s
//!   `layered_tiff_spots`).
//!
//! `cd core && cargo run --release --example phase13b_transient_copies`

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

use doctored_core::doc::{DrDocument, GrayRaster};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_psd::{
    dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_psd_export_report,
    dr_precision_doc_save_psd,
};
use doctored_core::ffi_scoped_statistics::{dr_doc_scoped_analysis, dr_doc_scoped_analysis_plan};
use doctored_core::ffi_selected_pixels::{
    dr_doc_selected_pixels_edit, dr_doc_selected_pixels_edit_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::layer_raster::LayerRaster;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::spot_channel::SpotInk;
use image::{GrayImage, RgbaImage};
use serde_json::{json, Value};

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

/// The admission limit: high enough that only the bytes are measured.
const LIMIT: u64 = 64 << 30;
/// `DrLayeredContainer`'s TIFF and `PsdExportMode`'s Layers.
const TIFF: i32 = 2;
const LAYERS: i32 = 0;

fn begin() -> usize {
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    base
}

fn take(error: *mut std::ffi::c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    Some(text)
}

fn plan_json(raw: *mut std::ffi::c_char) -> Value {
    let text = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(raw) };
    serde_json::from_str(&text).expect("plan json")
}

fn coverage(w: u32, h: u32, rect: (u32, u32, u32, u32)) -> Vec<u16> {
    let mut out = vec![0u16; w as usize * h as usize];
    for y in rect.1..rect.1 + rect.3 {
        for x in rect.0..rect.0 + rect.2 {
            out[y as usize * w as usize + x as usize] = u16::MAX;
        }
    }
    out
}

fn report(case: &str, canvas: (u32, u32), plan: u64, peak: usize, extra: Value) {
    println!(
        "{}",
        json!({"case": case, "canvas": format!("{}x{}", canvas.0, canvas.1),
               "plan_scratch": plan, "peak_above_baseline": peak,
               "peak_minus_plan": peak as i64 - plan as i64, "extra": extra})
    );
}

/// A ramp mask plane (no sample 255 throughout, so no plane is uniform).
fn ramp_mask(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 13 % 253) as u8).collect()
}

/// Row (i): a byte Levels Auto over a masked adjustment layer.
fn byte_auto(w: u32, h: u32) {
    let n = w as usize * h as usize;
    let mut doc = DrDocument::from_pixels(RgbaImage::new(w, h));
    let pixels: Vec<u8> = (0..n * 4).map(|i| (i * 7 % 251) as u8).collect();
    doc.layers[0].pixels = LayerRaster::new(RgbaImage::from_raw(w, h, pixels).unwrap());
    let mut doc = doc
        .adding_image_layer(0, RgbaImage::new(w, h), "Levels")
        .expect("an adjustment layer");
    let meta = json!({"type":"adjust","op":"levels","params":{}}).to_string();
    doc.layers[1].meta = Some(meta.into());
    doc.layers[1].mask = Some(GrayRaster::new(
        GrayImage::from_raw(w, h, ramp_mask(n)).unwrap(),
    ));
    doc.layers[1].mask_enabled = true;
    let text = CString::new(
        json!({"operation":"histogram","source":"backdrop","below":1,"auto":{"mode":"tone"}})
            .to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let plan = unsafe { dr_doc_scoped_analysis_plan(&doc, text.as_ptr(), &mut error) };
    if let Some(message) = take(error) {
        println!("{}", json!({"case": "byte auto plan", "error": message}));
        return;
    }
    let plan = plan_json(plan)["scratch_bytes"].as_u64().unwrap_or(0);
    let mut error = ptr::null_mut();
    let base = begin();
    let out = unsafe {
        dr_doc_scoped_analysis(
            &doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let peak = PEAK.load(Ordering::Relaxed) - base;
    let message = take(error);
    report(
        "byte levels auto, masked adjustment layer",
        (w, h),
        plan,
        peak,
        json!({"ok": !out.is_null(), "error": message, "copy_w_h": n}),
    );
    if !out.is_null() {
        unsafe { dr_string_free(out) };
    }
}

/// Row (ii): a layered TIFF save with one spot channel, 8 and 16 bits.
fn spot_tiff(w: u32, h: u32) {
    let n = w as usize * h as usize;
    let folder = tempfile::TempDir::new().expect("a scratch folder");
    let ink = SpotInk::new([55.25, 70.5, -30.75], 40).unwrap();
    let ramp: Vec<u8> = (0..n).map(|i| (i * 37 % 256) as u8).collect();

    // 8 bits: the composite supplied, as the host supplies it; the plan is
    // the report's.
    let pixels: Vec<u8> = (0..n * 4).map(|i| (i * 7 % 251) as u8).collect();
    let picture = RgbaImage::from_raw(w, h, pixels).unwrap();
    let flat = unsafe { dr_image_from_rgba(picture.as_raw().as_ptr(), w, h) };
    let byte = DrDocument::from_pixels(picture)
        .add_spot_channel("Spot", ink, &ramp)
        .expect("a byte spot channel");
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_doc_psd_export_report(&byte, TIFF, LAYERS, &mut error) };
    if let Some(message) = take(error) {
        println!(
            "{}",
            json!({"case": "byte spot TIFF report", "error": message})
        );
        return;
    }
    let plan = plan_json(raw)["working_memory_plan_bytes"]
        .as_u64()
        .unwrap_or(0);
    let path = CString::new(folder.path().join("spot8.tif").to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let base = begin();
    let ok = unsafe {
        dr_doc_save_psd(
            &byte,
            flat,
            path.as_ptr(),
            TIFF,
            LAYERS,
            ptr::null(),
            &mut error,
        )
    };
    let peak = PEAK.load(Ordering::Relaxed) - base;
    let message = take(error);
    report(
        "byte layered TIFF save, one spot channel",
        (w, h),
        plan,
        peak,
        json!({"ok": ok, "error": message, "spot_plane_w_h": n}),
    );
    unsafe { dr_image_free(flat) };
    drop(byte);

    // 16 bits: the save's admitted scratch (module doc).
    let rgba: Vec<[u16; 4]> = (0..n)
        .map(|i| {
            [
                (i * 31 % 65521) as u16,
                (i * 17 % 65519) as u16,
                (i % 65535) as u16,
                65535,
            ]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, rgba, IccProfile::srgb(), None).unwrap();
    let wide: Vec<u16> = ramp.iter().map(|&v| u16::from(v) * 257).collect();
    let typed = PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
        .unwrap()
        .add_spot_channel("Spot", ink, &wide, LIMIT, 0, None)
        .expect("a typed spot channel");
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_precision_doc_psd_export_report(&typed, TIFF, LAYERS, &mut error) };
    if let Some(message) = take(error) {
        println!(
            "{}",
            json!({"case": "typed spot TIFF report", "error": message})
        );
        return;
    }
    let report_json = plan_json(raw);
    let worst = report_json["worst_case_bytes"].as_u64().unwrap_or(0);
    let bake = report_json["bake_scratch_bytes"].as_u64().unwrap_or(0);
    let plan = report_json["working_memory_plan_bytes"]
        .as_u64()
        .unwrap_or(0);
    let projection = typed.render(LIMIT, 0, None).expect("the projection");
    let path = CString::new(folder.path().join("spot16.tif").to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let base = begin();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            &typed,
            &projection,
            path.as_ptr(),
            TIFF,
            LAYERS,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    let peak = PEAK.load(Ordering::Relaxed) - base;
    let message = take(error);
    report(
        "typed 16-bit layered TIFF save, one spot channel",
        (w, h),
        plan,
        peak,
        json!({"ok": ok, "error": message, "spot_plane_2_w_h": 2 * n,
               "worst_case_bytes": worst, "bake_scratch_bytes": bake}),
    );
}

fn main() {
    for (w, h) in [(3000u32, 2000u32), (6000, 4000)] {
        let n = w as usize * h as usize;
        let request =
            json!({"op":"move_selected","layer":0,"dx":10,"dy":5,"target":"mask"}).to_string();
        let text = CString::new(request.clone()).unwrap();
        let cover = coverage(w, h, (1000, 800, 200, 200));
        // ---- byte: a canvas-sized mask (ramp, so no plane is uniform).
        let mut doc = DrDocument::from_pixels(RgbaImage::new(w, h));
        let pixels: Vec<u8> = (0..n * 4).map(|i| (i * 7 % 251) as u8).collect();
        doc.layers[0].pixels = LayerRaster::new(RgbaImage::from_raw(w, h, pixels).unwrap());
        doc.layers[0].mask = Some(GrayRaster::new(
            GrayImage::from_raw(w, h, ramp_mask(n)).unwrap(),
        ));
        doc.layers[0].mask_enabled = true;
        doc.layers[0].mask_properties = MaskProperties::at((0, 0));
        let mut error = ptr::null_mut();
        let plan = unsafe { dr_doc_selected_pixels_edit_plan(&doc, text.as_ptr(), &mut error) };
        assert!(take(error).is_none());
        let plan = plan_json(plan)["scratch_bytes"].as_u64().unwrap();
        let base = begin();
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_doc_selected_pixels_edit(
                &doc,
                text.as_ptr(),
                cover.as_ptr(),
                cover.len(),
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            )
        };
        let peak = PEAK.load(Ordering::Relaxed) - base;
        let message = take(error);
        report(
            "byte move_selected mask",
            (w, h),
            plan,
            peak,
            json!({"ok": !out.is_null(), "error": message, "copy_w_h": n}),
        );
        if !out.is_null() {
            drop(unsafe { Box::from_raw(out) });
        }
        // The Align with Layer box: no plan; read in place since 13B-9a.
        let base = begin();
        let answer = doc.fill_align_box(0);
        let peak = PEAK.load(Ordering::Relaxed) - base;
        report(
            "byte fill_align_box",
            (w, h),
            0,
            peak,
            json!({"box": answer}),
        );
        drop(doc);

        // ---- typed: a canvas-sized 16-bit mask.
        let profile = IccProfile::srgb();
        let rgba: Vec<[u16; 4]> = (0..n)
            .map(|i| {
                [
                    (i * 31 % 65521) as u16,
                    (i * 17 % 65519) as u16,
                    (i % 65535) as u16,
                    65535,
                ]
            })
            .collect();
        let image = PrecisionImage::from_rgba16(w, h, rgba, profile.clone(), None).unwrap();
        let samples: Vec<u16> = (0..n).map(|i| (i * 2711 % 65531) as u16).collect();
        let plane = PrecisionPlane::from_u16(w, h, samples, None).unwrap();
        let mask = PrecisionMask::new(plane, MaskProperties::at((0, 0)), true).unwrap();
        let layer = Layer::raster("L", image, (0, 0), 1.0, true)
            .unwrap()
            .with_mask(Some(mask));
        let typed = PrecisionDocument::from_parts(
            w,
            h,
            profile,
            Metadata::default(),
            Resolution::default(),
            vec![layer],
        )
        .unwrap();
        let mut error = ptr::null_mut();
        let plan =
            unsafe { dr_precision_doc_edit_plan(&typed, text.as_ptr(), LIMIT, 0, &mut error) };
        if let Some(message) = take(error) {
            println!("{}", json!({"case": "typed plan", "error": message}));
            continue;
        }
        let plan = plan_json(plan);
        let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
        let base = begin();
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_edit(
                &typed,
                text.as_ptr(),
                cover.as_ptr(),
                cover.len(),
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            )
        };
        let peak = PEAK.load(Ordering::Relaxed) - base;
        let message = take(error);
        report(
            "typed move_selected mask",
            (w, h),
            scratch,
            peak,
            json!({"ok": !out.is_null(), "error": message, "copy_2_w_h": 2 * n, "plan": plan}),
        );
        if !out.is_null() {
            drop(unsafe { Box::from_raw(out) });
        }
        // A typed geometry over a masked layer (the mask resamples through
        // `transform_plane`: its source copy and `from_u16`'s adoption).
        for (label, text) in [
            (
                "typed resize 0.9x, masked layer",
                json!({"op":"resize","width":w * 9 / 10,"height":h * 9 / 10,"filter":"bilinear"}),
            ),
            (
                "typed set_channel from samples",
                json!({"op":"set_channel","index":null,"name":"A"}),
            ),
        ] {
            let text = CString::new(text.to_string()).unwrap();
            let channel: Vec<u16> = (0..n).map(|i| (i * 19 % 65519) as u16).collect();
            let samples: &[u16] = if label.contains("set_channel") {
                &channel
            } else {
                &[]
            };
            let mut error = ptr::null_mut();
            let plan =
                unsafe { dr_precision_doc_edit_plan(&typed, text.as_ptr(), LIMIT, 0, &mut error) };
            if let Some(message) = take(error) {
                println!("{}", json!({"case": label, "plan_error": message}));
                continue;
            }
            let plan = plan_json(plan);
            let scratch = plan["scratch_bytes"].as_u64().unwrap_or(0);
            let base = begin();
            let mut error = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_edit(
                    &typed,
                    text.as_ptr(),
                    if samples.is_empty() {
                        ptr::null()
                    } else {
                        samples.as_ptr()
                    },
                    samples.len(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut error,
                )
            };
            let peak = PEAK.load(Ordering::Relaxed) - base;
            let message = take(error);
            report(
                label,
                (w, h),
                scratch,
                peak,
                json!({"ok": !out.is_null(), "error": message, "plan": plan}),
            );
            if !out.is_null() {
                drop(unsafe { Box::from_raw(out) });
            }
        }
        // The Align with Layer box: no plan; read in place since 13B-9a.
        let base = begin();
        let answer = typed.fill_align_box(0);
        let peak = PEAK.load(Ordering::Relaxed) - base;
        report(
            "typed fill_align_box",
            (w, h),
            0,
            peak,
            json!({"box": answer}),
        );
        drop(typed);
        // from_u16: the input beside its tiles (no plan of its own; admission
        // charges bookkeeping only).
        let samples: Vec<u16> = (0..n).map(|i| (i * 977 % 65521) as u16).collect();
        let base = begin();
        let plane = PrecisionPlane::from_u16(w, h, samples, None).unwrap();
        let peak = PEAK.load(Ordering::Relaxed) - base;
        report(
            "PrecisionPlane::from_u16 (input moved in)",
            (w, h),
            0,
            peak,
            json!({"tiles_2_w_h": 2 * n, "kept": LIVE.load(Ordering::Relaxed) as i64 - base as i64}),
        );
        drop(plane);
        byte_auto(w, h);
        spot_tiff(w, h);
    }
}
