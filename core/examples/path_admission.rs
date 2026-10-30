//! One case of the path admission measurement: the working memory
//! `ffi_path` charges for a path's coverage, for tracing a selection into a
//! path, and for baking a vector mask at high depth, against what the
//! operation really holds. The caller measures this PROCESS's peak
//! `phys_footprint` from outside (`/usr/bin/time -l`, `MallocLargeCache=0`),
//! one case per process.
//!
//! ```text
//! path_admission coverage    <run|twin|facts> [width height circles radius polygons]
//! path_admission bake        <run|twin|facts> [width height circles radius polygons]
//! path_admission bake-raster <run|twin|facts> [width height circles radius polygons]
//! path_admission trace       <run|twin|facts> [width height combs tooth pitch tolerance]
//! ```
//!
//! `twin` builds every input the case holds (the planes, the shape JSON, the
//! document) and makes the call once under a working limit of ONE byte, so it
//! is refused before anything is allocated; `run` does the same and then
//! makes the call for real. The two therefore differ in exactly the
//! operation, and the refusal's sentence carries the plan: `plan_bytes` is
//! the retained inputs plus the scratch the export charges, and
//! `scratch_bytes` is that scratch alone. The difference in peak footprint
//! between `run` and `twin` must stay inside `scratch_bytes` with the 1.25
//! margin. `facts` prints what the case is made of and measures nothing.
//!
//! Building a case's inputs can briefly hold more than the case keeps (a
//! 16-bit layer is copied into its tiles), so a twin's peak can sit above
//! what the run holds when the call starts, and the difference would then
//! understate the call. Both modes therefore also print
//! `footprint_before_bytes`, this process's `phys_footprint` read just
//! before the first call: the run's peak less that is the call's own peak,
//! the figure held against `scratch_bytes`.
//!
//! * `coverage` — `dr_path_coverage` over the whole window, with a clip
//!   plane.
//! * `bake` — `dr_precision_doc_set_vector_mask` on a 16-bit document's one
//!   layer; `bake-raster` gives the layer a feathered raster mask first, so
//!   the bake also prepares that mask and writes the product.
//! * `trace` — `dr_path_trace`.
//!
//! The SHAPE of the first three is built to sit near the caps that size the
//! charge: `circles` four-anchor circles of `radius` pixels, which flatten
//! at 1/4096 px to hundreds of edges an arc and so fill the rasterizer's
//! edge lists (`artwork_coverage::MAX_PATH_EDGES`), and `polygons`
//! seven-anchor polygons, which fill the anchor and subpath caps
//! (`path_geometry::MAX_PATH_ANCHORS`, `MAX_SUBPATHS`) without adding many
//! edges. No two loops overlap, so the scan stays inside its work cap. The
//! trace's PLANE is `combs` combs of `tooth`-pixel teeth, `pitch` pixels
//! apart: a tooth adds two of its heights to the outline and four corners to
//! the result, which is how the plane reaches for
//! `path_trace::MAX_CONTOUR_POINTS` and the anchor cap together. The combs
//! are joined into ONE loop, because the trace's per-loop buffers are as
//! long as the loop it is fitting.
use std::ffi::{c_char, CStr, CString};
use std::fmt::Write as _;
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_path::{dr_path_coverage, dr_path_flatten, dr_path_trace};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_vector_mask::dr_precision_doc_set_vector_mask;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

/// A working limit no plan reaches.
const UNLIMITED: u64 = u64::MAX / 4;
/// The handle length of a quarter circle of radius one.
const KAPPA: f64 = 0.552_284_749_831;
/// The feather of `bake-raster`'s raster mask, in pixels.
const FEATHER: f64 = 20.0;

/// 24 MP; 27,760 anchors in 4,090 subpaths, about 511,000 edges.
const SHAPE_DEFAULTS: [f64; 5] = [6000.0, 4000.0, 290.0, 40.0, 3800.0];
/// 24 MP; an outline of about 2,050,000 pixels, about 29,900 anchors.
const TRACE_DEFAULTS: [f64; 6] = [6000.0, 4000.0, 10.0, 129.0, 8.0, 2.0];

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Run,
    Twin,
    Facts,
}

fn usage() -> ! {
    eprintln!(
        "usage: path_admission <coverage|bake|bake-raster|trace> <run|twin|facts> [numbers...]"
    );
    std::process::exit(2);
}

fn numbers(arguments: &[String], defaults: &[f64]) -> Vec<f64> {
    defaults
        .iter()
        .enumerate()
        .map(|(index, &default)| {
            arguments
                .get(index)
                .map_or(default, |text| text.parse().unwrap_or_else(|_| usage()))
        })
        .collect()
}

fn taken(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    text
}

/// The byte count in an admission refusal (`precision operation needs N
/// bytes including retained inputs; …`): the whole plan.
fn plan_of(refusal: &str) -> u64 {
    let digits: String = refusal
        .split("needs ")
        .nth(1)
        .unwrap_or_else(|| panic!("not an admission refusal: {refusal}"))
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().expect("the plan's byte count")
}

/// A value that varies with position.
fn sample(index: usize, channel: usize) -> u16 {
    ((index as u32)
        .wrapping_mul(2_654_435_761)
        .rotate_left(channel as u32 * 7)
        >> 16) as u16
}

struct Shape {
    text: String,
    anchors: usize,
    subpaths: usize,
}

impl Shape {
    fn anchor(&mut self, first: bool, point: [f64; 2], before: [f64; 2], after: [f64; 2]) {
        let comma = if first { "" } else { "," };
        let smooth = before != point;
        let _ = write!(
            self.text,
            r#"{comma}{{"x":{:.4},"y":{:.4},"in":[{:.4},{:.4}],"out":[{:.4},{:.4}],"smooth":{smooth}}}"#,
            point[0], point[1], before[0], before[1], after[0], after[1]
        );
        self.anchors += 1;
    }

    fn open(&mut self) {
        let comma = if self.subpaths == 0 { "" } else { "," };
        let _ = write!(self.text, r#"{comma}{{"closed":true,"anchors":["#);
        self.subpaths += 1;
    }

    fn circle(&mut self, cx: f64, cy: f64, r: f64) {
        let k = KAPPA * r;
        self.open();
        self.anchor(true, [cx + r, cy], [cx + r, cy - k], [cx + r, cy + k]);
        self.anchor(false, [cx, cy + r], [cx + k, cy + r], [cx - k, cy + r]);
        self.anchor(false, [cx - r, cy], [cx - r, cy + k], [cx - r, cy - k]);
        self.anchor(false, [cx, cy - r], [cx - k, cy - r], [cx + k, cy - r]);
        self.text.push_str("]}");
    }

    fn polygon(&mut self, cx: f64, cy: f64, r: f64, sides: usize) {
        self.open();
        for side in 0..sides {
            let turn = std::f64::consts::TAU * (side as f64 + 0.3) / sides as f64;
            let point = [cx + r * turn.cos(), cy + r * turn.sin()];
            self.anchor(side == 0, point, point, point);
        }
        self.text.push_str("]}");
    }
}

/// The circles in a band across the top, the polygons in rows from there to
/// the bottom, every loop clear of its neighbours.
fn shape(n: &[f64]) -> Shape {
    let (width, height) = (n[0], n[1]);
    let (circles, radius, polygons) = (n[2] as usize, n[3], n[4] as usize);
    let mut out = Shape {
        text: String::from(r#"{"fill_rule":"nonzero","subpaths":["#),
        anchors: 0,
        subpaths: 0,
    };
    let pitch = radius * 2.5;
    let across = ((width - pitch) / pitch).floor().max(1.0) as usize;
    for index in 0..circles {
        let (column, row) = (index % across, index / across);
        out.circle(
            pitch * (column as f64 + 0.6),
            pitch * (row as f64 + 0.6),
            radius,
        );
    }
    let band = pitch * circles.div_ceil(across) as f64;
    let (small, step) = (12.0, 40.0);
    let across = ((width - step) / step).floor().max(1.0) as usize;
    let rows = polygons.div_ceil(across).max(1);
    let down = (height - band - step) / rows as f64;
    assert!(down >= 2.0 * small + 2.0, "the polygons do not fit");
    for index in 0..polygons {
        let (column, row) = (index % across, index / across);
        out.polygon(
            step * (column as f64 + 0.8),
            band + 0.5 * step + down * (row as f64 + 0.5),
            small,
            7,
        );
    }
    out.text.push_str("]}");
    out
}

/// How many points the shape flattens to at the rasterizer's 1/4096 px: the
/// edges its outline takes, to within the horizontal ones it drops.
fn flattened_points(shape: &Shape) -> usize {
    let text = CString::new(shape.text.as_str()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_path_flatten(text.as_ptr(), 1.0 / 4096.0, &mut error) };
    assert!(!out.is_null(), "{}", taken(error));
    let polylines = taken(out);
    // `[{"closed":…,"points":[[x,y],…]},…]`: one bracket for the list, one a
    // subpath for its points, one a point.
    polylines.matches('[').count() - 1 - shape.subpaths
}

fn shape_facts(case: &str, n: &[f64], shape: &Shape) -> Value {
    json!({
        "case": case, "mode": "facts", "width": n[0], "height": n[1],
        "anchors": shape.anchors, "subpaths": shape.subpaths,
        "shape_json_bytes": shape.text.len(),
        "flattened_points": flattened_points(shape),
    })
}

/// This process's current `phys_footprint`, the figure whose peak
/// `/usr/bin/time -l` reports.
fn footprint() -> u64 {
    let mut info: libc::rusage_info_v0 = unsafe { std::mem::zeroed() };
    let status = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as libc::c_int,
            libc::RUSAGE_INFO_V0,
            (&mut info as *mut libc::rusage_info_v0).cast(),
        )
    };
    assert_eq!(status, 0, "proc_pid_rusage");
    info.ri_phys_footprint
}

fn report(case: &str, mode: Mode, plan: u64, retained: u64, before: u64, detail: Value) -> Value {
    json!({
        "case": case,
        "mode": if mode == Mode::Run { "run" } else { "twin" },
        "plan_bytes": plan,
        "retained_bytes": retained,
        "scratch_bytes": plan - retained,
        "footprint_before_bytes": before,
        "detail": detail,
    })
}

fn coverage_case(mode: Mode, n: &[f64]) -> Value {
    let shape = shape(n);
    if mode == Mode::Facts {
        return shape_facts("coverage", n, &shape);
    }
    let (width, height) = (n[0] as u32, n[1] as u32);
    let count = width as usize * height as usize;
    let text = CString::new(shape.text.as_str()).unwrap();
    // Both planes are written here, so the twin holds them as the run does.
    let clip: Vec<u16> = (0..count).map(|i| sample(i, 0) | 0x8000).collect();
    let mut output = vec![1u16; count];
    let mut cover = |limit: u64| {
        let mut error = ptr::null_mut();
        let ok = unsafe {
            dr_path_coverage(
                text.as_ptr(),
                0,
                0,
                width,
                height,
                true,
                clip.as_ptr(),
                count,
                output.as_mut_ptr(),
                count,
                limit,
                0,
                ptr::null(),
                &mut error,
            )
        };
        (ok, taken(error))
    };
    let before = footprint();
    let (ok, refusal) = cover(1);
    assert!(!ok);
    let plan = plan_of(&refusal);
    if mode == Mode::Run {
        let (ok, error) = cover(UNLIMITED);
        assert!(ok, "{error}");
    }
    let covered = output.iter().filter(|&&v| v > 1).count();
    let retained = shape.text.len() as u64 + count as u64 * 4;
    report(
        "coverage",
        mode,
        plan,
        retained,
        before,
        json!({"anchors": shape.anchors, "subpaths": shape.subpaths, "covered_samples": covered}),
    )
}

fn edited(document: PrecisionDocument, request: Value, samples: &[u16]) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let payload = if samples.is_empty() {
        ptr::null()
    } else {
        samples.as_ptr()
    };
    let out = unsafe {
        dr_precision_doc_edit(
            &document,
            text.as_ptr(),
            payload,
            samples.len(),
            UNLIMITED,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null(), "{}", taken(error));
    *unsafe { Box::from_raw(out) }
}

fn bake_case(mode: Mode, raster: bool, n: &[f64]) -> Value {
    let case = if raster { "bake-raster" } else { "bake" };
    let shape = shape(n);
    if mode == Mode::Facts {
        return shape_facts(case, n, &shape);
    }
    let (width, height) = (n[0] as u32, n[1] as u32);
    let count = width as usize * height as usize;
    let pixels: Vec<[u16; 4]> = (0..count)
        .map(|i| [sample(i, 0), sample(i, 1), sample(i, 2), 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(width, height, pixels, IccProfile::srgb(), None)
        .expect("the layer's pixels");
    let mut document =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default())
            .expect("the document");
    if raster {
        let coverage: Vec<u16> = (0..count).map(|i| sample(i, 1) | 0x8000).collect();
        document = edited(
            document,
            json!({"op": "set_mask", "layer": 0, "width": width, "height": height}),
            &coverage,
        );
        document = edited(
            document,
            json!({"op": "mask_properties", "layer": 0, "x": 0, "y": 0, "linked": true,
                   "density": 1.0, "feather": FEATHER, "enabled": true}),
            &[],
        );
    }
    let text = CString::new(shape.text.as_str()).unwrap();
    let set = |limit: u64| {
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_set_vector_mask(
                &document,
                0,
                text.as_ptr(),
                limit,
                0,
                ptr::null(),
                &mut error,
            )
        };
        (out, taken(error))
    };
    let before = footprint();
    let (out, refusal) = set(1);
    assert!(out.is_null());
    let plan = plan_of(&refusal);
    let retained = document.retained_bytes();
    let mut masked_bytes = 0;
    if mode == Mode::Run {
        let (out, error) = set(UNLIMITED);
        assert!(!out.is_null(), "{error}");
        let masked = unsafe { Box::from_raw(out) };
        masked_bytes = masked.retained_bytes();
    }
    report(
        case,
        mode,
        plan,
        retained,
        before,
        json!({"anchors": shape.anchors, "subpaths": shape.subpaths,
               "result_retained_bytes": masked_bytes}),
    )
}

/// `combs` combs down the plane: a four-pixel spine across the width with
/// teeth half a `pitch` wide hanging `tooth` pixels below it, and a
/// four-pixel bar down the left edge that joins each comb to the next.
fn comb_plane(width: usize, height: usize, combs: usize, tooth: usize, pitch: usize) -> Vec<u16> {
    let mut plane = vec![0u16; width * height];
    assert!(
        8 + combs * (tooth + 12) <= height && width > 16 + pitch && pitch >= 8,
        "the combs do not fit"
    );
    for comb in 0..combs {
        let top = 8 + comb * (tooth + 12);
        for y in top..top + 4 + tooth {
            let row = &mut plane[y * width..(y + 1) * width];
            for (x, value) in row.iter_mut().enumerate().take(width - 8).skip(8) {
                if y < top + 4 || (x - 8) % pitch < pitch / 2 {
                    *value = 65535;
                }
            }
        }
        if comb + 1 < combs {
            for y in top + 4 + tooth..top + tooth + 12 {
                plane[y * width + 8..y * width + 12].fill(65535);
            }
        }
    }
    plane
}

fn trace_case(mode: Mode, n: &[f64]) -> Value {
    let (width, height) = (n[0] as u32, n[1] as u32);
    let (combs, tooth, pitch) = (n[2] as usize, n[3] as usize, n[4] as usize);
    let tolerance = n[5];
    let teeth = combs * (width as usize - 16).div_ceil(pitch);
    // Two tooth heights a tooth, and the spine's two long sides.
    let outline = teeth * 2 * tooth + combs * 2 * (width as usize - 16);
    if mode == Mode::Facts {
        return json!({
            "case": "trace", "mode": "facts", "width": width, "height": height,
            "teeth": teeth, "outline_pixels": outline, "tolerance": tolerance,
        });
    }
    let count = width as usize * height as usize;
    let plane = comb_plane(width as usize, height as usize, combs, tooth, pitch);
    let trace = |limit: u64| {
        let mut error = ptr::null_mut();
        let out = unsafe {
            dr_path_trace(
                plane.as_ptr(),
                count,
                width,
                height,
                tolerance,
                limit,
                0,
                ptr::null(),
                &mut error,
            )
        };
        (out, taken(error))
    };
    let before = footprint();
    let (out, refusal) = trace(1);
    assert!(out.is_null());
    let plan = plan_of(&refusal);
    let mut detail = json!({"teeth": teeth, "outline_pixels": outline, "tolerance": tolerance});
    if mode == Mode::Run {
        let (out, error) = trace(UNLIMITED);
        assert!(!out.is_null(), "{error}");
        // Counted in the text: a parsed tree would be this process's peak.
        let text = unsafe { CStr::from_ptr(out) }.to_bytes();
        let count_of = |needle: &[u8]| text.windows(needle.len()).filter(|w| w == &needle).count();
        detail["result_anchors"] = json!(count_of(b"\"smooth\""));
        detail["result_subpaths"] = json!(count_of(b"\"closed\""));
        detail["result_json_bytes"] = json!(text.len());
        unsafe { dr_string_free(out) };
    }
    report("trace", mode, plan, count as u64 * 2, before, detail)
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (Some(case), Some(mode)) = (arguments.first(), arguments.get(1)) else {
        usage()
    };
    let mode = match mode.as_str() {
        "run" => Mode::Run,
        "twin" => Mode::Twin,
        "facts" => Mode::Facts,
        _ => usage(),
    };
    let rest = &arguments[2..];
    let out = match case.as_str() {
        "coverage" => coverage_case(mode, &numbers(rest, &SHAPE_DEFAULTS)),
        "bake" => bake_case(mode, false, &numbers(rest, &SHAPE_DEFAULTS)),
        "bake-raster" => bake_case(mode, true, &numbers(rest, &SHAPE_DEFAULTS)),
        "trace" => trace_case(mode, &numbers(rest, &TRACE_DEFAULTS)),
        _ => usage(),
    };
    println!("{out}");
}
