//! At 16 bits through the FFI: the `refine_edge` selection
//! kind (Select and Mask's edge stage), Quick Selection's brush parameters in
//! the typed `quick` kind, and `dr_precision_doc_estimate_foreground` with its
//! plan. The oracles are the byte suites' (`edge_refine.rs`,
//! `selection_quick.rs`, `foreground_estimation.rs`), restated at u16 codes:
//! a composite of two known colours under a known alpha.
use crate::common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use crate::native_scalar as native;
use doctored_core::ffi_foreground::*;
use doctored_core::ffi_precision_edit::*;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use native::{error, text, LIMIT};
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

/// The plan of a composite-source selection `operation`, or its refusal.
fn selection_plan(d: &PrecisionDocument, operation: &Value) -> Result<Value, String> {
    let request = text(json!({"op":"selection","operation":operation}));
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(d, request.as_ptr(), LIMIT, 0, &mut e) };
    if p.is_null() {
        return Err(unsafe { error(e) });
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { doctored_core::ffi::dr_string_free(p) };
    Ok(serde_json::from_str(&s).unwrap())
}

/// `dr_precision_doc_select` over the composite with `input`. On failure the
/// caller's buffer must be untouched.
fn select(d: &PrecisionDocument, operation: Value, input: &[u16]) -> Result<Vec<u16>, String> {
    let plan = selection_plan(d, &operation)?;
    let request = text(json!({"op":"selection","operation":operation}));
    let mut out = vec![999_u16; plan["output_samples"].as_u64().unwrap() as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            d,
            request.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    if ok {
        Ok(out)
    } else {
        assert!(out.iter().all(|v| *v == 999), "a failed call wrote output");
        Err(unsafe { error(e) })
    }
}

/// A `size`² u16 composite `C = round(a·F + (1 − a)·B)` (F and B the byte
/// colours × 257) whose alpha is a disc of `radius` about the centre with a
/// 6 px linear ramp.
struct Composite {
    document: PrecisionDocument,
    alpha: Vec<f64>,
    from_contour: Vec<f64>,
    mask: Vec<u16>,
}

fn composite(size: u32, radius: f64, f: [u16; 3], b: [u16; 3]) -> Composite {
    let centre = f64::from(size) / 2.0;
    let (mut pixels, mut alpha, mut from_contour, mut mask) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for y in 0..size {
        for x in 0..size {
            let d = (f64::from(x) + 0.5 - centre).hypot(f64::from(y) + 0.5 - centre);
            let a = ((radius + 3.0 - d) / 6.0).clamp(0.0, 1.0);
            let code = |c: usize| {
                (a * f64::from(f[c]) * 257.0 + (1.0 - a) * f64::from(b[c]) * 257.0).round() as u16
            };
            pixels.push([code(0), code(1), code(2), 65535]);
            alpha.push(a);
            from_contour.push((d - radius).abs());
            mask.push(if a >= 0.5 { 65535 } else { 0 });
        }
    }
    Composite {
        document: native::doc(size, size, pixels),
        alpha,
        from_contour,
        mask,
    }
}

fn assert_recovers_alpha(k: &Composite, out: &[u16], mean_bound: f64, what: &str) {
    let (mut total, mut count, mut worst) = (0.0, 0, 0.0_f64);
    for (i, &value) in out.iter().enumerate() {
        let error = (f64::from(value) / 65535.0 - k.alpha[i]).abs();
        worst = worst.max(error);
        if k.alpha[i] > 0.0 && k.alpha[i] < 1.0 {
            total += error;
            count += 1;
        }
        if k.from_contour[i] > 9.0 {
            assert_eq!(value, k.mask[i], "{what}: pixel {i} is outside the band");
        }
    }
    let mean = total / f64::from(count);
    assert!(
        mean <= mean_bound / 255.0,
        "{what}: mean error {}",
        mean * 255.0
    );
    assert!(
        worst <= 64.0 / 255.0,
        "{what}: worst error {}",
        worst * 255.0
    );
    assert!(
        out.iter().any(|v| v % 257 != 0),
        "{what}: every code is a byte code"
    );
}

#[test]
fn refine_edge_recovers_the_known_alpha_at_native_depth() {
    for (f, b) in [
        ([200, 60, 40], [40, 90, 200]),
        // Equal luminance: only colour separates them.
        ([200, 50, 50], [50, 92, 76]),
    ] {
        let k = composite(96, 30.0, f, b);
        let plain = select(
            &k.document,
            json!({"kind":"refine_edge","edge_radius":8}),
            &k.mask,
        )
        .unwrap();
        assert_recovers_alpha(&k, &plain, 16.0, "smart off");
        let smart = select(
            &k.document,
            json!({"kind":"refine_edge","edge_radius":8,"smart_radius":true}),
            &k.mask,
        )
        .unwrap();
        assert_recovers_alpha(&k, &smart, 24.0, "smart on");
    }
}

#[test]
fn refine_edge_plans_its_input_and_honours_the_area_plane() {
    let k = composite(96, 30.0, [200, 60, 40], [40, 90, 200]);
    let n = 96 * 96_u64;
    let plain =
        selection_plan(&k.document, &json!({"kind":"refine_edge","edge_radius":8})).unwrap();
    let with_area = selection_plan(
        &k.document,
        &json!({"kind":"refine_edge","edge_radius":8,"area":true}),
    )
    .unwrap();
    let wand = selection_plan(
        &k.document,
        &json!({"kind":"magic_wand","x":1,"y":1,"tolerance":0}),
    )
    .unwrap();
    assert_eq!(plain["input_samples"], n);
    assert_eq!(with_area["input_samples"], 2 * n);
    assert_eq!(plain["output_samples"], n);
    // The edge stage's term: its input twice over and 64 bytes a pixel.
    let scratch = |plan: &Value| plan["scratch_bytes"].as_u64().unwrap();
    assert_eq!(scratch(&plain) - scratch(&wand), 2 * n + 64 * n);
    assert_eq!(scratch(&with_area) - scratch(&wand), 4 * n + 64 * n);

    // EXCLUDE on the right half keeps those codes; the left is refined.
    let mut input = k.mask.clone();
    input.extend((0..n as usize).map(|i| if i % 96 >= 48 { 0_u16 } else { 128 }));
    let out = select(
        &k.document,
        json!({"kind":"refine_edge","edge_radius":8,"area":true}),
        &input,
    )
    .unwrap();
    let mut refined = 0;
    for (i, &value) in out.iter().enumerate() {
        if i % 96 >= 48 {
            assert_eq!(value, k.mask[i], "excluded pixel {i}");
        } else if value != k.mask[i] {
            refined += 1;
        }
    }
    assert!(refined > 0);
    // INCLUDE with no radius refines under the stroke only.
    let mut input = k.mask.clone();
    input.extend((0..n as usize).map(|i| if i % 96 < 30 { 255_u16 } else { 128 }));
    let out = select(
        &k.document,
        json!({"kind":"refine_edge","edge_radius":0,"area":true}),
        &input,
    )
    .unwrap();
    assert_ne!(out, k.mask);
    for (i, &value) in out.iter().enumerate() {
        if i % 96 >= 30 {
            assert_eq!(value, k.mask[i], "pixel {i} is outside the stroke");
        }
    }
    // No radius and no plane returns the input codes.
    assert_eq!(
        select(
            &k.document,
            json!({"kind":"refine_edge","edge_radius":0}),
            &k.mask
        )
        .unwrap(),
        k.mask
    );
    // A category that is not one of the three, in or out of byte range.
    for bad in [7_u16, 65535, 256] {
        let mut input = k.mask.clone();
        input.extend(std::iter::repeat_n(128_u16, n as usize));
        input[n as usize + 100] = bad;
        let message = select(
            &k.document,
            json!({"kind":"refine_edge","edge_radius":8,"area":true}),
            &input,
        )
        .unwrap_err();
        assert!(
            message.contains("0 (exclude), 128 (automatic) or 255 (include)"),
            "{bad}: {message}"
        );
    }
    // Out-of-range and unknown fields refuse in the plan; a wrong input
    // length refuses in the call.
    for operation in [
        json!({"kind":"refine_edge","edge_radius":65}),
        json!({"kind":"refine_edge","edge_radius":-1}),
        json!({"kind":"refine_edge"}),
        json!({"kind":"refine_edge","edge_radius":2,"edge_strength":1}),
    ] {
        assert!(
            !selection_plan(&k.document, &operation)
                .unwrap_err()
                .is_empty(),
            "{operation}"
        );
    }
    assert!(select(
        &k.document,
        json!({"kind":"refine_edge","edge_radius":8}),
        &k.mask[1..]
    )
    .unwrap_err()
    .contains("length"));
}

#[test]
fn refine_edge_plans_in_a_float_document_and_decontaminate_refuses_by_name() {
    let pixels = vec![[0.25_f32, 0.5, 8.0, 1.0]; 16];
    let float = crate::float_fixture::float_doc(4, 4, &IccProfile::srgb(), &[pixels]);
    // The edge stage reads the R1 display copy at float (rule E;
    // its coverage is `float_smart_selection.rs`'s), as does the Magic Wand.
    assert!(selection_plan(&float, &json!({"kind":"refine_edge","edge_radius":2})).is_ok());
    assert!(selection_plan(
        &float,
        &json!({"kind":"magic_wand","x":1,"y":1,"tolerance":0}),
    )
    .is_ok());
    // Decontaminate Colors refuses first, by its operator row, at the plan and
    // the run, whatever the source.
    let want = "Select and Mask's Decontaminate Colors is unavailable in a 32-bit float document";
    let composite = float.layer_source(0).unwrap();
    assert_eq!(foreground_plan(&float, composite).unwrap_err(), want);
    let matte = vec![32768_u16; 16];
    assert_eq!(
        refused(&float, composite, &matte, r#"{"color_space":"encoded"}"#),
        want
    );
    let (wide, _, _) = foreground_fixture(4, 4);
    assert_eq!(
        foreground_plan(&float, source(&wide)).unwrap_err(),
        want,
        "a 16-bit source in a float document"
    );
}

// ----------------------------------------------------------- typed quick --

#[test]
fn typed_quick_selection_takes_the_brush_radius_and_smoothing() {
    let red = [51400_u16, 15420, 10280, 65535];
    let blue = [10280_u16, 23130, 51400, 65535];
    let green = [7710_u16, 51400, 15420, 65535];
    // A hard edge is followed exactly.
    let pixels: Vec<[u16; 4]> = (0..64 * 64)
        .map(|i| if i % 64 < 32 { red } else { blue })
        .collect();
    let d = native::doc(64, 64, pixels);
    let out = select(
        &d,
        json!({"kind":"quick","params":{"seeds":[[16,32]],"radius":6,"smooth":1}}),
        &[],
    )
    .unwrap();
    for (i, value) in out.iter().enumerate() {
        assert_eq!(*value, if i % 64 < 32 { 65535 } else { 0 }, "pixel {i}");
    }
    // An enclosed five-pixel island is filled by the brush, not by the growth.
    let island = [(20, 20), (19, 20), (21, 20), (20, 19), (20, 21)];
    let pixels: Vec<[u16; 4]> = (0..64 * 64)
        .map(|i| {
            if island.contains(&(i % 64, i / 64)) {
                green
            } else {
                red
            }
        })
        .collect();
    let d = native::doc(64, 64, pixels);
    let brushed = select(
        &d,
        json!({"kind":"quick","params":{"seeds":[[40,40]],"radius":6}}),
        &[],
    )
    .unwrap();
    assert!(brushed.iter().all(|v| *v == 65535));
    let plain = select(
        &d,
        json!({"kind":"quick","params":{"seeds":[[40,40]]}}),
        &[],
    )
    .unwrap();
    for (i, value) in plain.iter().enumerate() {
        let inside = island.contains(&(i as u32 % 64, i as u32 / 64));
        assert_eq!(*value, if inside { 0 } else { 65535 }, "pixel {i}");
    }
    // The brush-pixel cap refuses in the plan, naming the brush and the seeds.
    let wide = native::doc(320, 320, vec![red; 320 * 320]);
    let seeds = vec![json!([160, 160]); 800];
    let message = selection_plan(
        &wide,
        &json!({"kind":"quick","params":{"seeds":seeds,"radius":149.5}}),
    )
    .unwrap_err();
    assert!(message.contains("brush size 300"), "{message}");
    assert!(message.contains("800 seeds"), "{message}");
}

// ------------------------------------------------------ typed foreground --

/// `foreground_estimation.rs`'s fixture at u16: a horizontal alpha ramp from
/// 0 (x ≤ 30%) to 1 (x ≥ 70%) compositing one known colour over another.
fn foreground_fixture(w: u32, h: u32) -> (PrecisionDocument, Vec<u16>, [f64; 3]) {
    let foreground = [0.18, 0.36, 0.62];
    let background = [0.91, 0.06, 0.12];
    let (mut pixels, mut matte) = (Vec::new(), Vec::new());
    for _ in 0..h {
        for x in 0..w {
            let a = (((f64::from(x) / f64::from(w - 1) - 0.3) / 0.4).clamp(0.0, 1.0) * 65535.0)
                .round() as u16;
            let t = f64::from(a) / 65535.0;
            let code = |c: usize| {
                ((t * foreground[c] + (1.0 - t) * background[c]) * 65535.0).round() as u16
            };
            pixels.push([code(0), code(1), code(2), 65535]);
            matte.push(a);
        }
    }
    (native::doc(w, h, pixels), matte, foreground)
}

fn source(d: &PrecisionDocument) -> &PrecisionImage {
    d.layer_source(0).unwrap()
}

fn foreground_plan(d: &PrecisionDocument, image: &PrecisionImage) -> Result<Value, String> {
    let mut e = ptr::null_mut();
    let p = unsafe { dr_precision_doc_estimate_foreground_plan(d, image, &mut e) };
    if p.is_null() {
        return Err(unsafe { error(e) });
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { doctored_core::ffi::dr_string_free(p) };
    Ok(serde_json::from_str(&s).unwrap())
}

fn estimate(
    d: &PrecisionDocument,
    image: &PrecisionImage,
    matte: &[u16],
    params: &str,
) -> Result<Option<PrecisionImage>, String> {
    let params = CString::new(params).unwrap();
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_estimate_foreground(
            d,
            image,
            matte.as_ptr(),
            matte.len(),
            params.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    if !e.is_null() {
        assert!(out.is_null());
        return Err(unsafe { error(e) });
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

/// The refusal of an estimate that must not succeed.
fn refused(d: &PrecisionDocument, image: &PrecisionImage, matte: &[u16], params: &str) -> String {
    estimate(d, image, matte, params)
        .err()
        .expect("the estimate was not refused")
}

#[test]
fn the_typed_estimate_removes_a_known_background_and_keeps_the_rest_exact() {
    let (d, matte, foreground) = foreground_fixture(97, 65);
    let before = source(&d).to_rgba16().unwrap();
    let plan = foreground_plan(&d, source(&d)).unwrap();
    let n = 97 * 65_u64;
    assert_eq!(plan["input_samples"], n);
    assert_eq!(plan["scratch_bytes"], 8 * 1024 * 1024 + n * 136);
    assert_eq!(plan["retained_bytes"], d.retained_bytes());
    let out = estimate(&d, source(&d), &matte, r#"{"color_space":"encoded"}"#)
        .unwrap()
        .expect("an estimate");
    let after = out.to_rgba16().unwrap();
    let (mut initial, mut remaining, mut samples) = (0.0, 0.0, 0);
    for (i, &a) in matte.iter().enumerate() {
        assert_eq!(after[i][3], 65535);
        if a == 0 || a == 65535 {
            assert_eq!(after[i], before[i], "pixel {i} is outside the matte's edge");
            continue;
        }
        let weight = f64::from(a) / 65535.0;
        for c in 0..3 {
            initial += weight * (f64::from(before[i][c]) / 65535.0 - foreground[c]).abs();
            remaining += weight * (f64::from(after[i][c]) / 65535.0 - foreground[c]).abs();
            samples += 1;
        }
    }
    // The byte suite's predeclared bounds: an alpha-weighted mean error of at
    // most 0.02 and three quarters of the contamination gone.
    assert!(
        remaining / f64::from(samples) <= 0.02,
        "{}",
        remaining / f64::from(samples)
    );
    assert!(remaining <= initial * 0.25, "{initial} -> {remaining}");
    // The source is untouched, and half strength lands between the two.
    assert_eq!(source(&d).to_rgba16().unwrap(), before);
    let half = estimate(
        &d,
        source(&d),
        &matte,
        r#"{"color_space":"encoded","strength":0.5}"#,
    )
    .unwrap()
    .expect("an estimate")
    .to_rgba16()
    .unwrap();
    let middle = (97 * 32 + 48) as usize;
    for c in 0..3 {
        let (lo, hi) = (
            before[middle][c].min(after[middle][c]),
            before[middle][c].max(after[middle][c]),
        );
        assert!(
            (lo..=hi).contains(&half[middle][c]),
            "channel {c} at half strength"
        );
    }
}

#[test]
fn the_typed_estimate_names_its_refusals_and_its_no_ops() {
    let (d, matte, _) = foreground_fixture(17, 3);
    let encoded = r#"{"color_space":"encoded"}"#;
    // No-ops: strength 0, a binary matte, an empty or full matte.
    let binary: Vec<u16> = matte
        .iter()
        .map(|a| if *a < 32768 { 0 } else { 65535 })
        .collect();
    for (mask, params) in [
        (matte.clone(), r#"{"color_space":"encoded","strength":0}"#),
        (binary, encoded),
        (vec![0; matte.len()], encoded),
        (vec![65535; matte.len()], encoded),
    ] {
        assert!(estimate(&d, source(&d), &mask, params).unwrap().is_none());
    }
    // Linear is refused by name; malformed parameters refuse.
    let message = refused(&d, source(&d), &matte, r#"{"color_space":"linear"}"#);
    assert!(message.contains("linear"), "{message}");
    for params in [
        "{}",
        "[]",
        r#"{"color_space":"srgb"}"#,
        r#"{"color_space":"encoded","strength":2}"#,
        r#"{"color_space":"encoded","alpha":1}"#,
    ] {
        assert!(
            !refused(&d, source(&d), &matte, params).is_empty(),
            "{params}"
        );
    }
    // A constant fractional matte cannot separate two colours.
    assert!(
        refused(&d, source(&d), &vec![32768; matte.len()], encoded).contains("constant fractional")
    );
    // A matte of another length.
    assert!(refused(&d, source(&d), &matte[1..], encoded).contains("dimensions"));
    // A source pixel that is not opaque.
    let mut pixels = source(&d).to_rgba16().unwrap();
    pixels[0][3] = 65534;
    let translucent = native::doc(17, 3, pixels);
    assert!(refused(&translucent, source(&translucent), &matte, encoded).contains("opaque source"));
    // A source of another size, in the plan and in the call.
    let (small, _, _) = foreground_fixture(9, 3);
    assert!(foreground_plan(&d, source(&small))
        .unwrap_err()
        .contains("canvas-sized RGBA16"));
    assert!(refused(&d, source(&small), &matte, encoded).contains("canvas-sized RGBA16"));
    // Admission against the caller's limit.
    let params = CString::new(encoded).unwrap();
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_estimate_foreground(
            &d,
            source(&d),
            matte.as_ptr(),
            matte.len(),
            params.as_ptr(),
            1,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(out.is_null());
    assert!(!unsafe { error(e) }.is_empty());
}

#[test]
fn the_typed_estimate_tolerates_null_and_a_used_or_cancelled_control() {
    let (d, matte, _) = foreground_fixture(17, 3);
    let params = CString::new(r#"{"color_space":"encoded"}"#).unwrap();
    unsafe {
        let mut e = ptr::null_mut();
        assert!(
            dr_precision_doc_estimate_foreground_plan(ptr::null(), source(&d), &mut e).is_null()
        );
        assert!(!error(e).is_empty());
        let mut e = ptr::null_mut();
        assert!(dr_precision_doc_estimate_foreground_plan(&d, ptr::null(), &mut e).is_null());
        assert!(!error(e).is_empty());
        assert!(dr_precision_doc_estimate_foreground_plan(
            ptr::null(),
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        let image: *const PrecisionImage = source(&d);
        let document: *const PrecisionDocument = &d;
        for (doc, img, m, p) in [
            (ptr::null(), image, matte.as_ptr(), params.as_ptr()),
            (document, ptr::null(), matte.as_ptr(), params.as_ptr()),
            (document, image, ptr::null(), params.as_ptr()),
            (document, image, matte.as_ptr(), ptr::null()),
        ] {
            let mut e = ptr::null_mut();
            assert!(dr_precision_doc_estimate_foreground(
                doc,
                img,
                m,
                matte.len(),
                p,
                LIMIT,
                0,
                ptr::null(),
                &mut e
            )
            .is_null());
            assert!(!error(e).is_empty());
        }
        assert!(dr_precision_doc_estimate_foreground(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        // A cancelled control, then the same control again.
        let token = doctored_core::ffi_jobs::dr_job_control_create();
        doctored_core::ffi_jobs::dr_job_control_cancel(token);
        for expect in ["cancelled", "already been used"] {
            let mut e = ptr::null_mut();
            assert!(dr_precision_doc_estimate_foreground(
                document,
                image,
                matte.as_ptr(),
                matte.len(),
                params.as_ptr(),
                LIMIT,
                0,
                token,
                &mut e
            )
            .is_null());
            assert!(error(e).contains(expect));
            assert_eq!(
                doctored_core::ffi_jobs::dr_job_control_scratch_bytes(token),
                0
            );
        }
        doctored_core::ffi_jobs::dr_job_control_free(token);
    }
}

#[test]
fn a_cancel_inside_the_typed_solve_never_publishes() {
    let (d, matte, _) = foreground_fixture(257, 129);
    let addresses = (
        &d as *const PrecisionDocument as usize,
        source(&d) as *const PrecisionImage as usize,
    );
    let matte = &matte;
    let render = move |token: usize| {
        let params = CString::new(r#"{"color_space":"encoded"}"#).unwrap();
        let mut e = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_estimate_foreground(
                addresses.0 as _,
                addresses.1 as _,
                matte.as_ptr(),
                matte.len(),
                params.as_ptr(),
                LIMIT,
                0,
                token as _,
                &mut e,
            )
        };
        let published = !out.is_null();
        if published {
            drop(unsafe { Box::from_raw(out) });
        }
        (published, unsafe { error(e) })
    };
    // k = 1: the coarsest pyramid level polls twice before its sweep's report
    // (`foreground_estimation.rs`'s byte twin, the same solver).
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress >= 0.15, render);
    let n0 = entry.checkpoints;
    assert!(
        next.expect("the solve never reported again").checkpoints > n0 + 2,
        "the solve polls fewer than two checkpoints before its next report"
    );
    let parked = cancelled_parked_at(
        n0 + 2,
        &|progress, stage| stage == entry.stage && progress == entry.progress,
        render,
    );
    assert!(parked.scratch > 0);
    eprintln!("typed foreground cancellation: {}", parked_line(&parked));
}
