//! Phase 13A-6a: the typed (u16) Modify Selection kernels — Feather, Smooth,
//! Grow, Shrink, Border and Refine's stages — evaluated over the selection's
//! SUPPORT grown by the kernel's reach (`precision_selection_support`), reached
//! through the unchanged `dr_selection_u16`. The oracle is the retained
//! WHOLE-PLANE reference, `precision_selection_plane::reference_apply` (the
//! pre-13A-6a canvas path, statement for statement): within the work cap the
//! bounded result equals it sample for sample; beyond it (`reference_apply`
//! returns the work-limit sentence) the bounded result finishes at the canvas
//! dimensions and equals `reference_apply` on a canvas holding ONLY the window
//! (the cumulative window for Refine), embedded, with zeros outside — the
//! windows restated here by hand from each kernel's declared reach. The
//! bounded path has no work refusal: a full-support request past the
//! old cap completes (compared, away from the cut, with the same request on
//! the top half of the plane, which the reference still admits), while the
//! whole-plane reference keeps its bound and its sentence, which is how the
//! window oracle is chosen. The envelope pin
//! proves the shared distance transform translation-exact past coordinate
//! 4096 (it fails on the pre-13A-6a f32 `edt_1d`).
//!
//! Default tests are load-independent (13A-5b-iii-b-1): a pre-cancelled
//! token, a cancel PARKED inside the crop's blur (13A-5b-iii-b-3's seam) and a
//! cancelled-then-complete run equal to the reference. DECLARED before
//! measuring (13A-5a's model `T_f + W_s·c_s`, `T_f` = 1 ms): a cancel issued
//! inside the crop's horizontal Gaussian pass returns within
//! `T_f + 4096·c_cell`, `c_cell` one output cell of that pass calibrated from
//! the worker's own uncancelled checkpoint timeline; the `#[ignore]`d probe
//! prints PASS/MISS through `make cancel-bench`. Misses are recorded, never
//! widened.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_selection_u16::{dr_selection_u16, dr_selection_u16_plan};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::precision_selection_plane::reference_apply;
use serde_json::{json, Value};

use crate::common;
use common::probe::*;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SENTENCE: &str = "native selection exceeds the work limit";

type Rect = (u32, u32, u32, u32);

// ------------------------------------------------------------- plumbing --

unsafe fn message(e: *mut c_char) -> String {
    if e.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(e) };
    s
}

/// `dr_selection_u16` into a sentinel-filled output under `control` (NULL:
/// none). On failure the output must be untouched.
fn bounded_with(
    w: u32,
    h: u32,
    request: &Value,
    input: &[u16],
    control: *const DrJobControl,
) -> Result<Vec<u16>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut out = vec![54321u16; w as usize * h as usize];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_selection_u16(
            w,
            h,
            text.as_ptr(),
            input.as_ptr(),
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            control,
            &mut e,
        )
    };
    let m = unsafe { message(e) };
    if ok {
        Ok(out)
    } else {
        assert!(
            out.iter().all(|&v| v == 54321),
            "a refused edit wrote its output"
        );
        Err(m)
    }
}

fn bounded(w: u32, h: u32, request: &Value, input: &[u16]) -> Result<Vec<u16>, String> {
    bounded_with(w, h, request, input, ptr::null())
}

fn plan(w: u32, h: u32, request: &Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut e = ptr::null_mut();
    let raw = unsafe { dr_selection_u16_plan(w, h, text.as_ptr(), LIMIT, 0, &mut e) };
    if raw.is_null() {
        return Err(unsafe { message(e) });
    }
    let s = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(raw) };
    Ok(serde_json::from_str(&s).unwrap())
}

fn reference(w: u32, h: u32, request: &Value, input: &[u16]) -> Result<Vec<u16>, String> {
    reference_apply(&request.to_string(), input, w, h)
}

/// A private SplitMix64 (the fixtures only need reproducible noise).
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A coverage code: 0 about a quarter of the time, else any code
    /// (odd and sub-byte codes included).
    fn code(&mut self) -> u16 {
        let v = self.next();
        if v.is_multiple_of(4) {
            0
        } else {
            (v >> 16) as u16
        }
    }
}

/// A `w`×`h` plane, zero outside `rect`, random codes inside it.
fn selection(w: u32, h: u32, rect: Rect, seed: u64) -> Vec<u16> {
    let mut noise = Noise(seed);
    let mut out = vec![0u16; w as usize * h as usize];
    for y in rect.1..rect.3 {
        for x in rect.0..rect.2 {
            out[(y * w + x) as usize] = noise.code();
        }
    }
    out
}

/// The window's samples of a `w`-wide plane.
fn crop(plane: &[u16], w: u32, window: Rect) -> Vec<u16> {
    let mut out = Vec::new();
    for y in window.1..window.3 {
        let row = (y * w) as usize;
        out.extend_from_slice(&plane[row + window.0 as usize..row + window.2 as usize]);
    }
    out
}

/// `rect`, grown by `reach` and clamped to the canvas — restated by hand.
fn grown(rect: Rect, reach: u32, w: u32, h: u32) -> Rect {
    (
        rect.0.saturating_sub(reach),
        rect.1.saturating_sub(reach),
        (rect.2 + reach).min(w),
        (rect.3 + reach).min(h),
    )
}

/// The beyond-cap oracle: `reference_apply` on a canvas holding ONLY
/// `window` of `input`, embedded in a zeroed `w`×`h` plane.
fn window_oracle(w: u32, h: u32, request: &Value, input: &[u16], window: Rect) -> Vec<u16> {
    let (ww, wh) = (window.2 - window.0, window.3 - window.1);
    let inner = reference(ww, wh, request, &crop(input, w, window))
        .unwrap_or_else(|e| panic!("the window oracle refused {request}: {e}"));
    let mut out = vec![0u16; w as usize * h as usize];
    for y in 0..wh {
        let at = ((window.1 + y) * w + window.0) as usize;
        out[at..at + ww as usize]
            .copy_from_slice(&inner[(y * ww) as usize..((y + 1) * ww) as usize]);
    }
    out
}

fn first_difference(a: &[u16], b: &[u16], w: u32) -> String {
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(i) => format!(
            "first difference at ({}, {}): {} vs {}",
            i as u32 % w,
            i as u32 / w,
            a[i],
            b[i]
        ),
        None => format!("lengths {} vs {}", a.len(), b.len()),
    }
}

/// Within the cap: the bounded result equals `reference_apply` exactly.
fn assert_exact(w: u32, h: u32, request: &Value, input: &[u16], what: &str) {
    let expected = reference(w, h, request, input)
        .unwrap_or_else(|e| panic!("{what}: the reference refused {request}: {e}"));
    let actual = bounded(w, h, request, input)
        .unwrap_or_else(|e| panic!("{what}: the bounded edit refused {request}: {e}"));
    assert!(
        actual == expected,
        "{what}: {request} differs from the whole-plane reference ({})",
        first_difference(&actual, &expected, w)
    );
}

/// Beyond the cap: the reference refuses; the bounded result equals the
/// window oracle.
fn assert_beyond(w: u32, h: u32, request: &Value, input: &[u16], window: Rect, what: &str) {
    assert_eq!(
        reference(w, h, request, input).err().as_deref(),
        Some(SENTENCE),
        "{what}: {request} is not beyond the cap on the whole plane"
    );
    let actual = bounded(w, h, request, input)
        .unwrap_or_else(|e| panic!("{what}: the bounded edit refused {request}: {e}"));
    assert_eq!(actual.len(), w as usize * h as usize, "{what}: dimensions");
    let expected = window_oracle(w, h, request, input, window);
    assert!(
        actual == expected,
        "{what}: {request} differs from the window oracle ({})",
        first_difference(&actual, &expected, w)
    );
}

/// Either rule, by the reference's own verdict on the whole plane.
fn assert_matches(w: u32, h: u32, request: &Value, input: &[u16], window: Rect, what: &str) {
    match reference(w, h, request, input) {
        Ok(_) => assert_exact(w, h, request, input, what),
        Err(e) if e == SENTENCE => assert_beyond(w, h, request, input, window, what),
        Err(e) => panic!("{what}: the reference refused {request}: {e}"),
    }
}

/// The declared reach, restated: the Gaussian's half width for
/// Feather/Smooth (`nearest_odd(2r + 1).max(3) / 2`), `ceil(r) + 1` for the
/// three morphology kinds.
fn reach(kind: &str, radius: f64) -> u32 {
    if kind == "feather" || kind == "smooth" {
        let x = radius * 2. + 1.;
        let n = x.round().max(1.) as u32;
        let odd = if n % 2 == 1 {
            n
        } else if x - f64::from(n - 1) <= f64::from(n + 1) - x {
            n - 1
        } else {
            n + 1
        };
        odd.max(3) / 2
    } else {
        radius.ceil() as u32 + 1
    }
}

const KINDS: [&str; 5] = ["feather", "smooth", "grow", "shrink", "border"];

// --------------------------------------------------------- within the cap --

const CW: u32 = 120;
const CH: u32 = 90;

/// The small-canvas fixtures: (name, plane, the support rect it was drawn in).
fn fixtures() -> Vec<(&'static str, Vec<u16>, Rect)> {
    let mut out = vec![
        ("interior", (30, 25, 70, 60)),
        ("left edge", (0, 30, 20, 50)),
        ("top edge", (40, 0, 60, 15)),
        ("right edge", (100, 40, 120, 70)),
        ("bottom edge", (50, 70, 80, 90)),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (name, rect))| (name, selection(CW, CH, rect, 11 + i as u64), rect))
    .collect::<Vec<_>>();
    let mut single = vec![0u16; (CW * CH) as usize];
    single[(44 * CW + 61) as usize] = 40001;
    out.push(("1×1", single, (61, 44, 62, 45)));
    out.push(("empty", vec![0u16; (CW * CH) as usize], (0, 0, 0, 0)));
    let mut noise = Noise(97);
    let whole = (0..CW * CH)
        .map(|_| noise.code().max(1))
        .collect::<Vec<_>>();
    out.push(("full canvas", whole, (0, 0, CW, CH)));
    out
}

#[test]
fn every_kind_and_radius_equals_the_whole_plane_reference() {
    for (name, input, _) in fixtures() {
        for kind in KINDS {
            for radius in [0.5, 1.0, 7.3, 64.0, 250.0] {
                let request = json!({"kind": kind, "radius": radius});
                assert_exact(CW, CH, &request, &input, name);
            }
        }
    }
}

#[test]
fn refine_compositions_equal_the_whole_plane_reference() {
    let requests = [
        json!({"kind":"refine","smooth":3,"feather":2.5,"shift":1.5,"contrast":0.4}),
        json!({"kind":"refine","shift":-2.3}),
        json!({"kind":"refine","shift":-6,"feather":4,"contrast":1}),
        json!({"kind":"refine","smooth":64,"shift":64,"feather":64,"contrast":0.9}),
        json!({"kind":"refine","smooth":1}),
        json!({"kind":"refine","contrast":1}),
        json!({"kind":"refine"}),
    ];
    for (name, input, _) in fixtures() {
        for request in &requests {
            assert_exact(CW, CH, request, &input, name);
        }
    }
}

#[test]
fn an_empty_selection_stays_empty_at_every_kind() {
    let empty = vec![0u16; (CW * CH) as usize];
    for kind in KINDS {
        let out = bounded(CW, CH, &json!({"kind": kind, "radius": 9}), &empty).unwrap();
        assert!(out.iter().all(|&v| v == 0), "{kind}");
    }
}

/// A 4400×4400 canvas with the support inside [4200, 4300)²: beyond
/// coordinate 4096 on both axes, where only an exact (f64) envelope keeps the
/// cropped and the whole-plane distance transforms equal.
#[test]
fn a_support_past_coordinate_4096_on_both_axes_is_exact() {
    const S: u32 = 4400;
    let rect = (4207, 4203, 4291, 4298);
    let input = selection(S, S, rect, 4242);
    for kind in KINDS {
        // Feather/Smooth 64 on this 4400² plane are beyond the cap on the
        // whole plane, so `assert_matches` checks them against the window
        // oracle.
        for radius in [1.0, 7.3, 64.0] {
            let request = json!({"kind": kind, "radius": radius});
            let window = grown(rect, reach(kind, radius), S, S);
            assert_matches(S, S, &request, &input, window, "far support");
        }
    }
    let request = json!({"kind":"refine","smooth":2,"shift":-3,"feather":3,"contrast":0.5});
    let window = grown(
        rect,
        reach("smooth", 2.) + reach("shrink", 3.) + reach("feather", 3.),
        S,
        S,
    );
    assert_matches(S, S, &request, &input, window, "far refine");
}

/// The envelope pin: the whole-plane distance transform of a support at
/// x ∈ [8400, 8700) on a 9000×8 canvas equals, over x ∈ [7800, 9000), the
/// same request on a 1200×8 canvas with the support translated to [600, 900).
/// The pre-13A-6a f32 envelope mis-selects there (`f + q²` past 2^24).
#[test]
fn the_distance_transform_is_translation_exact_past_coordinate_4096() {
    let far = selection(9000, 8, (8400, 0, 8700, 8), 8409);
    let near = crop(&far, 9000, (7800, 0, 9000, 8));
    for kind in ["grow", "shrink", "border"] {
        for radius in [10.0, 0.5] {
            let request = json!({"kind": kind, "radius": radius});
            let whole = reference(9000, 8, &request, &far).unwrap();
            let translated = reference(1200, 8, &request, &near).unwrap();
            let seen = crop(&whole, 9000, (7800, 0, 9000, 8));
            assert!(
                seen == translated,
                "{request}: the far transform differs from its translation ({})",
                first_difference(&seen, &translated, 1200)
            );
        }
    }
}

// --------------------------------------------------------- beyond the cap --

/// B1's fixture: a 100×100 random selection at (500, 400) on a 2048×1024
/// canvas — 2,097,152 pixels, past feather 250's 1,996,008-pixel cap.
const BW: u32 = 2048;
const BH: u32 = 1024;
const B_RECT: Rect = (500, 400, 600, 500);

fn b1() -> Vec<u16> {
    selection(BW, BH, B_RECT, 1996)
}

#[test]
fn a_250px_feather_and_smooth_of_a_small_selection_finish_beyond_the_cap() {
    let input = b1();
    for kind in ["feather", "smooth"] {
        let request = json!({"kind": kind, "radius": 250});
        // The plan reports scratch only: it no longer refuses.
        let p = plan(BW, BH, &request).unwrap();
        assert_eq!(p["width"], BW);
        assert_eq!(p["height"], BH);
        let window = grown(B_RECT, 250, BW, BH);
        assert_eq!(window, (250, 150, 850, 750));
        assert_beyond(BW, BH, &request, &input, window, kind);
    }
}

#[test]
fn morphology_of_a_small_selection_finishes_beyond_the_cap() {
    // 8000×8000 = 64,000,000 pixels, past morphology's 62.5 MP.
    const S: u32 = 8000;
    let rect = (7000, 7100, 7100, 7180);
    let input = selection(S, S, rect, 64);
    for kind in ["grow", "shrink", "border"] {
        let request = json!({"kind": kind, "radius": 10});
        assert_beyond(S, S, &request, &input, grown(rect, 11, S, S), kind);
    }
}

#[test]
fn a_refine_summing_past_the_cap_finishes_over_its_cumulative_window() {
    // 4096×2048: each stage under the cap on the canvas (smooth 40:
    // 8,388,608·2·81 = 1.36·10⁹), their sum over it.
    const W: u32 = 4096;
    const H: u32 = 2048;
    let rect = (1000, 900, 1100, 1000);
    let input = selection(W, H, rect, 40);
    let request = json!({"kind":"refine","smooth":40,"shift":-3,"feather":40,"contrast":0.5});
    let cumulative = reach("smooth", 40.) + reach("shrink", 3.) + reach("feather", 40.);
    assert_eq!(cumulative, 40 + 4 + 40);
    assert_beyond(
        W,
        H,
        &request,
        &input,
        grown(rect, cumulative, W, H),
        "refine",
    );
}

#[test]
fn full_support_beyond_the_old_cap_completes() {
    // A single kind: the window is the canvas, 2,097,152 px × 2 × 501 taps
    // = 2.1·10⁹ units, past the 2·10⁹ the bounded path once refused at.
    // Every sample is nonzero, so the support is the whole plane.
    let mut noise = Noise(606);
    let full = (0..BW * BH)
        .map(|_| noise.code().max(1))
        .collect::<Vec<_>>();
    let request = json!({"kind":"feather","radius":250});
    assert!(
        plan(BW, BH, &request).is_ok(),
        "the plan reports scratch only"
    );
    // The whole-plane reference keeps its bound (the oracle's own).
    assert_eq!(
        reference(BW, BH, &request, &full).err().as_deref(),
        Some(SENTENCE)
    );
    let actual = bounded(BW, BH, &request, &full)
        .unwrap_or_else(|e| panic!("a full-support feather past the old cap refused: {e}"));
    assert_eq!(actual.len(), (BW * BH) as usize);
    // The independent expectation: the same request on the TOP HALF of the
    // plane, which the reference admits (1.05·10⁹ units). The Gaussian reads
    // rows y − 250 ..= y + 250, clamped at the plane's edges, so every row
    // whose taps stay above the cut (y + 250 ≤ 511) reads the same samples
    // in the same order on both planes.
    let half = BH / 2;
    let top = &full[..(BW * half) as usize];
    let expected = reference(BW, half, &request, top)
        .unwrap_or_else(|e| panic!("the top-half reference refused: {e}"));
    let reach = reach("feather", 250.);
    assert_eq!(reach, 250);
    let rows = half - reach;
    assert_eq!(rows, 262);
    let compared = (BW * rows) as usize;
    assert!(
        actual[..compared] == expected[..compared],
        "rows above the cut differ from the top-half reference ({})",
        first_difference(&actual[..compared], &expected[..compared], BW)
    );
    // Refine, per stage under and summed over: a 10 MP select-all
    // `refine smooth 40 feather 40` = 2·1.62·10⁹ + 10⁷ (contrast). The
    // reference still refuses; the bounded run is entered (a cancelled
    // control answers `operation cancelled`, not a work refusal).
    const W: u32 = 4000;
    const H: u32 = 2500;
    let all = vec![65535u16; (W * H) as usize];
    let request = json!({"kind":"refine","smooth":40,"feather":40,"contrast":0.5});
    assert!(plan(W, H, &request).is_ok());
    assert_eq!(
        reference(W, H, &request, &all).err().as_deref(),
        Some(SENTENCE)
    );
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let result = bounded_with(W, H, &request, &all, control);
    unsafe { dr_job_control_free(control) };
    assert_eq!(result.err().as_deref(), Some("operation cancelled"));
}

#[test]
fn the_reference_covers_the_spatial_kinds_and_refine_only() {
    assert_eq!(
        reference(2, 1, &json!({"kind":"invert"}), &[1, 2])
            .err()
            .as_deref(),
        Some("reference covers spatial kinds and refine only")
    );
}

// ------------------------------------------------------------ cancellation --

fn edit(w: u32, h: u32, request: Value, input: Vec<u16>) -> impl FnOnce(usize) -> (bool, String) {
    move |token| match bounded_with(w, h, &request, &input, token as *const DrJobControl) {
        Ok(_) => (true, String::new()),
        Err(e) => (false, e),
    }
}

/// Within the cap, so the completed run is compared with `reference_apply`
/// itself: a 1024×1024 canvas, the B1 selection's rect, feather 64 (window
/// 228×228 of the canvas).
const PW: u32 = 1024;
const PH: u32 = 1024;

fn parked_fixture() -> (Value, Vec<u16>) {
    (
        json!({"kind":"feather","radius":64}),
        selection(PW, PH, B_RECT, 1024),
    )
}

#[test]
fn a_pre_cancelled_token_returns_nothing_and_holds_nothing() {
    let (request, input) = parked_fixture();
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let result = bounded_with(PW, PH, &request, &input, control);
    let scratch = unsafe { dr_job_control_scratch_bytes(control) };
    unsafe { dr_job_control_free(control) };
    let e = result.expect_err("a pre-cancelled edit completed");
    assert!(e.contains("cancel"), "{e}");
    assert_eq!(scratch, 0);
}

#[test]
fn a_cancel_parked_inside_the_crops_blur_returns_nothing_then_completes_exactly() {
    let (request, input) = parked_fixture();
    // The blur runs over the crop's rows, never the canvas's: every stage-6
    // report is one Gaussian row of the 228-row window, two passes.
    let log = reports(edit(PW, PH, request.clone(), input.clone()));
    let rows = log.iter().filter(|r| r.stage == 6).count();
    assert_eq!(rows, 2 * 228, "the blur did not run over the window's rows");
    let n0 = entry_count(&at_the_blur, edit(PW, PH, request.clone(), input.clone()));
    let p = cancelled_parked_at(
        n0 + 1 + 60,
        &|progress, stage| stage == 6 && progress > 0. && progress < 0.5,
        edit(PW, PH, request.clone(), input.clone()),
    );
    eprintln!(
        "13A-6a parked cancel inside the crop's horizontal pass: {}",
        parked_line(&p)
    );
    // Cancelled, then complete: equal to the whole-plane reference.
    let control = unsafe { dr_job_control_create() };
    let again = bounded_with(PW, PH, &request, &input, control);
    unsafe { dr_job_control_free(control) };
    let expected = reference(PW, PH, &request, &input).unwrap();
    assert!(again.unwrap() == expected, "the completed re-run differs");
}

// ------------------------------------------------ bench (cancel-bench) --

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict (13A-6a)"]
fn a_concurrently_cancelled_bounded_feather_returns_within_4096_cells() {
    // B1 beyond the cap: feather 250 over its 600×600 window. The horizontal
    // pass polls three times a row (the row check, `every(0)`, the report);
    // `c_cell` is one output cell of it, from the worker's own timeline
    // between the first stage-6 report and the pass's end (progress 0.5).
    const WINDOW: f64 = 600.;
    let request = json!({"kind":"feather","radius":250});
    let t = timeline(
        &at_the_blur,
        &|progress, stage, _| stage == 6 && progress >= 0.5,
        edit(BW, BH, request.clone(), b1()),
    );
    let (first, last) = (t.0[0], *t.0.last().unwrap());
    let rows = (last.1 - first.1) as f64 / 3.;
    let c_cell = (last.0 - first.0).as_secs_f64() / (rows * WINDOW);
    let p = probe(
        &at_the_blur,
        Trigger::After(90),
        &|progress, stage| stage == 6 && progress <= 0.5,
        edit(BW, BH, request, b1()),
    );
    let bound = RETURN_FLOOR_S + 4096. * c_cell;
    eprintln!(
        "13A-6a bounded feather 250 ({BW}×{BH}, window 600×600) cancel inside the crop's \
         horizontal pass: {:.3} ms cancel-to-return (issued at {:.1} ms; {}); c_cell {:.4} µs \
         ({rows:.0} rows timed); bound T_f + 4096·c_cell = {:.3} ms (T_f 1 ms); ≈{:.2} cells past \
         the floor; {}",
        p.latency.as_secs_f64() * 1e3,
        p.at.as_secs_f64() * 1e3,
        checkpoints_line(&p),
        c_cell * 1e6,
        bound * 1e3,
        past(p.latency, c_cell),
        verdict(p.latency, bound),
    );
}
