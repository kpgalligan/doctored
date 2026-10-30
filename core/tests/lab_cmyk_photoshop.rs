//! #547 Lab phase 5b: Photoshop's take of Image ▸ Mode ▸ CMYK Color from a
//! Lab Color document at 8 and 16 bits (`~/devel/doctored-fixtures/
//! photoshop/lab-cmyk-547/take1`, never committed; `scripts/photoshop-probe/
//! lab_cmyk_take.py`, whose docstring fixes the scene, candidates, gate and
//! build table), judged and read back in place through the FFI.
//!
//! Each candidate's input is Photoshop's own Lab, `codesN.tif` opened by
//! `dr_precision_doc_open_lab`; the destination is the ICC embedded in
//! Photoshop's `cmyk.tif` (opened by `dr_precision_doc_open`, read by
//! `dr_precision_doc_cmyk_profile`), relative colorimetric with BPC:
//!
//! * C1, today's route: the codes at 16 bits (`dr_precision_doc_lab_depth_edit`),
//!   `dr_precision_doc_convert_lab_to_rgb` into the built-in ProPhoto RGB
//!   (relative, BPC), then `dr_precision_doc_convert_to_cmyk` at depth N;
//! * C2, Lab PCS straight to the profile's B2A: `dr_precision_doc_convert_to_cmyk`
//!   at depth N of an RGB u16 document of the same codes in the twin
//!   (`dr_precision_doc_profile` of the Lab document).
//!
//! The gate, declared before the take: per ink per cell, r = 255 ×
//! (Photoshop's centre 4 × 4 mean − the candidate's), max |r| ≤ 1.0 over 26
//! cells × 4 inks.
//!
//! With `DR_LAB_CMYK_JUDGE=<take>/judged.json` set, the one test writes
//! `{"lab8":{"C1":{"max","mean"},"C2":{…}},"lab16":{…}}` (|r| in 8-bit ink
//! codes) for the runner's `--decide`.
//!
//! take1 read MISS (max / mean |r| 6.06 / 1.04 at 8 bits, 5.88 / 1.02 at 16,
//! C1 = C2), so Lab → CMYK is NOT confirmed by Photoshop: the residual is
//! most likely in the → CMYK conversion itself, not the Lab leg. Unset, the
//! test is therefore a REGRESSION RECORD of that residual, not a PASS gate:
//! it reads take1 in place (SKIP when absent), asserts both cases VALID and
//! C1 and C2 agreeing in the record, that today's
//! `dr_precision_doc_convert_to_cmyk` of each opened `codesN.tif` is C1's
//! prediction sample for sample, and that its residual and C2's do not
//! exceed take1's recorded maxima ([`RECORDED_MAX`]). The bound is the
//! measured value, so it can only tighten.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi_cmyk::{dr_precision_doc_cmyk_profile, dr_precision_doc_convert_to_cmyk};
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_lab_depth_edit, dr_precision_doc_open_lab,
};
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_open};
use doctored_core::ffi_precision_inspect::dr_precision_doc_profile;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Map, Value};

use crate::common::take_err_string;
use crate::gray_ops::{free, plan_json, result, LIMIT};

const W: usize = 104;
const PATCHES: usize = 26;
const CANDIDATES: [&str; 2] = ["C1", "C2"];

/// take1's recorded max |r| per depth (`judged.json`, C1 = C2), in 8-bit ink
/// codes: that residual, pinned at the measured value.
const RECORDED_MAX: [(u32, f64); 2] = [(8, 6.062499999999979), (16, 5.8832684824902515)];

fn descriptor(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    plan_json(unsafe { dr_precision_doc_descriptor(doc, &mut err) }, err).unwrap()
}

fn open(path: &Path, lab: bool) -> *mut PrecisionDocument {
    let text = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        if lab {
            dr_precision_doc_open_lab(text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        } else {
            dr_precision_doc_open(text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        }
    };
    assert!(
        !doc.is_null(),
        "{}: {}",
        path.display(),
        take_err_string(err)
    );
    doc
}

fn bytes_of(pointer: *const u8, length: usize) -> Vec<u8> {
    assert!(!pointer.is_null() && length > 0);
    unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
}

/// The ICC Photoshop embedded in its `cmyk.tif`.
fn press_of(cmyk: *const PrecisionDocument) -> Vec<u8> {
    let mut length = 0;
    let pointer = unsafe { dr_precision_doc_cmyk_profile(cmyk, &mut length) };
    bytes_of(pointer, length)
}

fn to_cmyk(doc: *const PrecisionDocument, press: &[u8], bits: u32) -> *mut PrecisionDocument {
    let options = json!({"depth":bits,"intent":"relative","black_point_compensation":true});
    let text = CString::new(options.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            doc,
            press.as_ptr(),
            press.len(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
        .unwrap()
        .expect("a CMYK conversion always yields a document")
}

/// C1 (module doc).
fn candidate_one(lab: *const PrecisionDocument, press: &[u8], bits: u32) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let wide = unsafe { dr_precision_doc_lab_depth_edit(lab, 16, LIMIT, 0, ptr::null(), &mut err) };
    let wide = result(wide, err).unwrap();
    let source = wide.map_or(lab, |doc| doc as *const PrecisionDocument);
    let prophoto = IccProfile::prophoto_rgb();
    let options = CString::new(r#"{"intent":"relative","black_point_compensation":true}"#).unwrap();
    let mut err = ptr::null_mut();
    let rgb = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            source,
            prophoto.bytes().as_ptr(),
            prophoto.bytes().len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!rgb.is_null(), "to ProPhoto: {}", take_err_string(err));
    let out = to_cmyk(rgb, press, bits);
    free(rgb);
    if let Some(doc) = wide {
        free(doc);
    }
    out
}

/// C2 (module doc).
fn candidate_two(lab: *const PrecisionDocument, press: &[u8], bits: u32) -> *mut PrecisionDocument {
    let mut length = 0;
    let pointer = unsafe { dr_precision_doc_profile(lab, &mut length) };
    let twin = Arc::new(IccProfile::parse(&bytes_of(pointer, length)).expect("the twin parses"));
    let doc = unsafe { &*lab };
    let source = doc.layer_source(0).expect("one raster layer");
    let (width, height) = source.dimensions();
    let image = PrecisionImage::from_rgba16(
        width,
        height,
        source.to_rgba16().unwrap(),
        twin.clone(),
        None,
    )
    .unwrap();
    let layer = Layer::raster("Background", image, (0, 0), 1.0, true).unwrap();
    let (canvas_width, canvas_height) = doc.dimensions();
    let rgb = PrecisionDocument::from_parts(
        canvas_width,
        canvas_height,
        twin,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    assert_eq!(descriptor(&rgb)["color_mode"], "rgb");
    to_cmyk(&rgb, press, bits)
}

/// Layer 0's inks as unit planes [C, M, Y, K].
fn ink_planes(doc: *const PrecisionDocument) -> [Vec<f64>; 4] {
    let samples = unsafe { &*doc }
        .cmyk_layer_samples(0)
        .expect("a CMYK record");
    std::array::from_fn(|c| samples.iter().map(|s| f64::from(s[c]) / 65535.0).collect())
}

/// Patch `k`'s centre 4 × 4 mean of `plane` (`float_selection.centre`).
fn centre(plane: &[f64], k: usize) -> f64 {
    let (i, band) = (k % 13, k / 13);
    let mut sum = 0.0;
    for y in 8 * band + 2..8 * band + 6 {
        for x in 8 * i + 2..8 * i + 6 {
            sum += plane[y * W + x];
        }
    }
    sum / 16.0
}

/// (max |r|, mean |r|) over 26 cells × 4 inks, in 8-bit ink codes.
fn residuals(theirs: &[Vec<f64>; 4], ours: &[Vec<f64>; 4]) -> (f64, f64) {
    let mut all = Vec::with_capacity(PATCHES * 4);
    for c in 0..4 {
        for k in 0..PATCHES {
            all.push((255.0 * (centre(&theirs[c], k) - centre(&ours[c], k))).abs());
        }
    }
    let max = all.iter().copied().fold(0.0, f64::max);
    (max, all.iter().sum::<f64>() / all.len() as f64)
}

struct Depth {
    lab: *mut PrecisionDocument,
    photoshop: *mut PrecisionDocument,
    press: Vec<u8>,
}

impl Depth {
    fn open(root: &Path, bits: u32) -> Self {
        let folder = root.join(format!("jobs/lab{bits}"));
        let lab = open(&folder.join(format!("codes{bits}.tif")), true);
        assert_eq!(descriptor(lab)["lab"]["depth"], bits);
        let photoshop = open(&folder.join("cmyk.tif"), false);
        let d = descriptor(photoshop);
        assert_eq!(d["color_mode"], "cmyk");
        assert_eq!(
            (d["width"].as_u64(), d["height"].as_u64()),
            (Some(104), Some(24))
        );
        let press = press_of(photoshop);
        Self {
            lab,
            photoshop,
            press,
        }
    }

    fn candidate(&self, name: &str, bits: u32) -> *mut PrecisionDocument {
        match name {
            "C1" => candidate_one(self.lab, &self.press, bits),
            _ => candidate_two(self.lab, &self.press, bits),
        }
    }
}

impl Drop for Depth {
    fn drop(&mut self) {
        free(self.photoshop);
        free(self.lab);
    }
}

fn judge(root: &Path, out: &Path) {
    let mut judged = Map::new();
    for bits in [8u32, 16] {
        let depth = Depth::open(root, bits);
        let theirs = ink_planes(depth.photoshop);
        let mut row = Map::new();
        for name in CANDIDATES {
            let doc = depth.candidate(name, bits);
            let (max, mean) = residuals(&theirs, &ink_planes(doc));
            row.insert(name.into(), json!({"max": max, "mean": mean}));
            free(doc);
        }
        judged.insert(format!("lab{bits}"), Value::Object(row));
    }
    std::fs::write(out, Value::Object(judged).to_string() + "\n").unwrap();
}

#[test]
fn the_photoshop_take_records_issue_684s_residual() {
    if let Some(out) = std::env::var_os("DR_LAB_CMYK_JUDGE") {
        let out = PathBuf::from(out);
        let root = out
            .parent()
            .expect("judged.json's take folder")
            .to_path_buf();
        judge(&root, &out);
        return;
    }
    let Some(root) = std::env::var("HOME").ok().map(|home| {
        PathBuf::from(home).join("devel/doctored-fixtures/photoshop/lab-cmyk-547/take1")
    }) else {
        eprintln!("SKIP: HOME is unset, so the Photoshop take cannot be found");
        return;
    };
    let Some(decided) = std::fs::read_to_string(root.join("decided.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        eprintln!("SKIP: {} has no decided.json", root.display());
        return;
    };
    // The take itself is VALID: both cases, no stop, the judge ran.
    assert!(decided["stop"].is_null(), "{}", decided["stop"]);
    assert!(
        decided["judge_error"].is_null(),
        "{}",
        decided["judge_error"]
    );
    for (bits, recorded) in RECORDED_MAX {
        let key = format!("lab{bits}");
        assert_eq!(
            decided["cases"][&key]["valid"], true,
            "{key}: {}",
            decided["cases"][&key]
        );
        // C1 and C2 agree in the record.
        let judged = &decided["judged"][&key];
        for name in CANDIDATES {
            let max = judged[name]["max"].as_f64().expect("a recorded max");
            assert!(
                (max - recorded).abs() <= 1e-9,
                "{key} {name}: recorded max {max}"
            );
        }
        let (one, two) = (
            judged["C1"]["mean"].as_f64().unwrap(),
            judged["C2"]["mean"].as_f64().unwrap(),
        );
        assert!(
            (one - two).abs() <= 0.01,
            "{key}: C1 mean {one}, C2 mean {two}"
        );
    }
    for (bits, recorded) in RECORDED_MAX {
        let depth = Depth::open(&root, bits);
        let theirs = ink_planes(depth.photoshop);
        let shipped = to_cmyk(depth.lab, &depth.press, bits);
        let (max, _) = residuals(&theirs, &ink_planes(shipped));
        assert!(
            max <= recorded + 1e-9,
            "{bits} bits: today's max |r| {max} > {recorded}"
        );
        let predicted = depth.candidate("C1", bits);
        assert_eq!(
            unsafe { &*shipped }.cmyk_layer_samples(0),
            unsafe { &*predicted }.cmyk_layer_samples(0),
            "{bits} bits: today's route is C1"
        );
        free(predicted);
        free(shipped);
        let other = depth.candidate("C2", bits);
        let (max, _) = residuals(&theirs, &ink_planes(other));
        assert!(
            max <= recorded + 1e-9,
            "{bits} bits: C2's max |r| {max} > {recorded}"
        );
        free(other);
    }
}
