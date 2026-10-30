//! Photoshop's take of Image ▸ Mode ▸ CMYK Color from an sRGB
//! document at 8 and 16 bits on U.S. Web Coated (SWOP) v2
//! (`~/devel/doctored-fixtures/photoshop/rgb-cmyk-684/take1`, never
//! committed; `scripts/photoshop-probe/rgb_cmyk_take.py`, whose docstring
//! fixes the scene, candidates, gate and build table), judged and read back
//! in place through the FFI.
//!
//! Each candidate is `dr_precision_doc_convert_to_cmyk` of a 104 × 24 RGB u16
//! document of the take's `scene.json` codes (8-bit × 257) in its sRGB ICC,
//! into the ICC embedded in Photoshop's `cmyk.tif`
//! (`dr_precision_doc_cmyk_profile` of the opened file):
//!
//! * C1 `{"depth":N}`, today's route (relative with BPC by default);
//! * C2 relative without BPC; C3 perceptual with BPC; C4 perceptual without;
//! * C5, C1's document against Photoshop's `cmyk_nodither.tif` at 8 bits
//!   (`convertProfile` with dither off), and against `cmyk.tif` at 16.
//!
//! The gate, declared before the take: per ink per patch, r = 255 ×
//! (Photoshop's centre 4 × 4 mean − the candidate's), max |r| ≤ 1.0 over 39
//! patches × 4 inks.
//!
//! With `DR_RGB_CMYK_JUDGE=<take>/judged.json` set, the one test writes
//! `{"rgb8":{Ck:{"max","mean","patches"}},"rgb16":{…},"lab":{…}|null}` for the
//! runner's `--decide`. `lab` re-measures #1 Lab phase 5b's take
//! (`lab-cmyk-547/take1`) in place through its C1 route (the codes at 16
//! bits, into the built-in ProPhoto relative with BPC) and then each of
//! C1–C4's options, over its 26 cells; null when that take is absent.
//!
//! take1 read MISS: C1 max / mean |r| 6.75 / 0.98 at 8 bits and 6.95 / 0.98
//! at 16, every other candidate worse, Photoshop's cyan high and magenta and
//! yellow low throughout, most in the near-black shadows; the residual is
//! the CMS engine's (ACE against ours), accepted by the owner (2026-10-22).
//!
//! Unset, the test is the conversion's REGRESSION RECORD: it reads take1 in
//! place (SKIP when absent), asserts the take usable, that C1's output is the
//! explicit relative-with-BPC output sample for sample, that C1's residual
//! does not exceed take1's recorded maxima ([`RECORDED_MAX`]), and that the
//! Lab re-measure of C1 equals Lab 5b's recorded maxima
//! ([`LAB_RECORDED_MAX`]).

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi_cmyk::{dr_precision_doc_cmyk_profile, dr_precision_doc_convert_to_cmyk};
use doctored_core::ffi_lab::{
    dr_precision_doc_convert_lab_to_rgb, dr_precision_doc_lab_depth_edit, dr_precision_doc_open_lab,
};
use doctored_core::ffi_precision::{dr_precision_doc_descriptor, dr_precision_doc_open};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Map, Value};

use crate::common::take_err_string;
use crate::gray_ops::{free, plan_json, result, LIMIT};

const W: usize = 104;
const H: usize = 24;
const PATCHES: usize = 39;
const LAB_PATCHES: usize = 26;
const CANDIDATES: [&str; 5] = ["C1", "C2", "C3", "C4", "C5"];
const LAB_CANDIDATES: [&str; 4] = ["C1", "C2", "C3", "C4"];

/// take1's recorded C1 max |r| per depth (`judged.json`), in 8-bit ink
/// codes, pinned at the measured value. take1 read MISS (no candidate met
/// the gate; outcome "miss-engine"), so this records the residual and can
/// only tighten.
const RECORDED_MAX: [(u32, f64); 2] = [(8, 6.750000000000004), (16, 6.953307392996113)];

/// Lab 5b's take1 recorded max |r| per depth, in 8-bit ink codes.
const LAB_RECORDED_MAX: [(u32, f64); 2] = [(8, 6.062499999999979), (16, 5.8832684824902515)];

fn fixtures() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join("devel/doctored-fixtures/photoshop"))
}

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

/// Photoshop's CMYK file, checked to be CMYK at 104 × 24.
fn open_cmyk(path: &Path) -> *mut PrecisionDocument {
    let doc = open(path, false);
    let d = descriptor(doc);
    assert_eq!(d["color_mode"], "cmyk", "{}", path.display());
    assert_eq!(
        (d["width"].as_u64(), d["height"].as_u64()),
        (Some(W as u64), Some(H as u64)),
        "{}",
        path.display()
    );
    doc
}

/// The ICC Photoshop embedded in its CMYK file.
fn press_of(cmyk: *const PrecisionDocument) -> Vec<u8> {
    let mut length = 0;
    let pointer = unsafe { dr_precision_doc_cmyk_profile(cmyk, &mut length) };
    assert!(!pointer.is_null() && length > 0);
    unsafe { std::slice::from_raw_parts(pointer, length) }.to_vec()
}

/// A candidate's options at depth `bits` (module doc; C5 is C1's).
fn options(name: &str, bits: u32) -> Value {
    match name {
        "C2" => json!({"depth":bits,"intent":"relative","black_point_compensation":false}),
        "C3" => json!({"depth":bits,"intent":"perceptual","black_point_compensation":true}),
        "C4" => json!({"depth":bits,"intent":"perceptual","black_point_compensation":false}),
        _ => json!({ "depth": bits }),
    }
}

fn to_cmyk(doc: *const PrecisionDocument, press: &[u8], options: &Value) -> *mut PrecisionDocument {
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

/// The scene: an RGB u16 document of `scene.json`'s 8-bit codes × 257, patch
/// k at column k % 13 of band k / 13, in the scene's ICC.
fn scene_document(root: &Path) -> PrecisionDocument {
    let text = std::fs::read_to_string(root.join("inputs/scene.json")).unwrap();
    let scene: Value = serde_json::from_str(&text).unwrap();
    let icc = std::fs::read(scene["icc"].as_str().expect("the scene's ICC path")).unwrap();
    let profile = Arc::new(IccProfile::parse(&icc).expect("the scene's ICC parses"));
    let codes: Vec<[u16; 3]> = scene["patches"]["8"]
        .as_array()
        .expect("39 patches")
        .iter()
        .map(|p| std::array::from_fn(|c| 257 * p[c].as_u64().unwrap() as u16))
        .collect();
    assert_eq!(codes.len(), PATCHES);
    let mut pixels = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            let [r, g, b] = codes[13 * (y / 8) + x / 8];
            pixels.push([r, g, b, 65535]);
        }
    }
    let image =
        PrecisionImage::from_rgba16(W as u32, H as u32, pixels, profile.clone(), None).unwrap();
    let layer = Layer::raster("Background", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        W as u32,
        H as u32,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap();
    assert_eq!(descriptor(&doc)["color_mode"], "rgb");
    doc
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

/// Per patch [rC, rM, rY, rK] in 8-bit ink codes, signed (theirs − ours).
fn patch_residuals(theirs: &[Vec<f64>; 4], ours: &[Vec<f64>; 4], count: usize) -> Vec<[f64; 4]> {
    (0..count)
        .map(|k| std::array::from_fn(|c| 255.0 * (centre(&theirs[c], k) - centre(&ours[c], k))))
        .collect()
}

/// (max |r|, mean |r|) over every patch and ink.
fn max_mean(residuals: &[[f64; 4]]) -> (f64, f64) {
    let all: Vec<f64> = residuals.iter().flatten().map(|r| r.abs()).collect();
    let max = all.iter().copied().fold(0.0, f64::max);
    (max, all.iter().sum::<f64>() / all.len() as f64)
}

struct Depth {
    scene: PrecisionDocument,
    photoshop: *mut PrecisionDocument,
    nodither: Option<*mut PrecisionDocument>,
    press: Vec<u8>,
}

impl Depth {
    fn open(root: &Path, bits: u32) -> Self {
        let folder = root.join(format!("jobs/rgb{bits}"));
        let photoshop = open_cmyk(&folder.join("cmyk.tif"));
        let nodither = (bits == 8).then(|| open_cmyk(&folder.join("cmyk_nodither.tif")));
        let press = press_of(photoshop);
        Self {
            scene: scene_document(root),
            photoshop,
            nodither,
            press,
        }
    }

    /// The candidate's document and the Photoshop file it is judged against.
    fn candidate(&self, name: &str, bits: u32) -> (*mut PrecisionDocument, *mut PrecisionDocument) {
        let theirs = match (name, self.nodither) {
            ("C5", Some(plain)) => plain,
            _ => self.photoshop,
        };
        (
            to_cmyk(&self.scene, &self.press, &options(name, bits)),
            theirs,
        )
    }
}

impl Drop for Depth {
    fn drop(&mut self) {
        free(self.photoshop);
        if let Some(plain) = self.nodither {
            free(plain);
        }
    }
}

/// Lab 5b's C1 route into the candidate's options (module doc).
fn lab_candidate(
    lab: *const PrecisionDocument,
    press: &[u8],
    options: &Value,
) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let wide = unsafe { dr_precision_doc_lab_depth_edit(lab, 16, LIMIT, 0, ptr::null(), &mut err) };
    let wide = result(wide, err).unwrap();
    let source = wide.map_or(lab, |doc| doc as *const PrecisionDocument);
    let prophoto = IccProfile::prophoto_rgb();
    let rgb_options =
        CString::new(r#"{"intent":"relative","black_point_compensation":true}"#).unwrap();
    let mut err = ptr::null_mut();
    let rgb = unsafe {
        dr_precision_doc_convert_lab_to_rgb(
            source,
            prophoto.bytes().as_ptr(),
            prophoto.bytes().len(),
            rgb_options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!rgb.is_null(), "to ProPhoto: {}", take_err_string(err));
    let out = to_cmyk(rgb, press, options);
    free(rgb);
    if let Some(doc) = wide {
        free(doc);
    }
    out
}

/// The Lab take's folder, when both depths' files are present.
fn lab_take() -> Option<PathBuf> {
    let root = fixtures()?.join("lab-cmyk-547/take1");
    [8u32, 16]
        .iter()
        .all(|bits| {
            let folder = root.join(format!("jobs/lab{bits}"));
            folder.join(format!("codes{bits}.tif")).is_file() && folder.join("cmyk.tif").is_file()
        })
        .then_some(root)
}

/// The Lab re-measure: {"lab8": {C1..C4: {"max","mean"}}, "lab16": …}.
fn lab_remeasure(root: &Path) -> Value {
    let mut out = Map::new();
    for bits in [8u32, 16] {
        let folder = root.join(format!("jobs/lab{bits}"));
        let lab = open(&folder.join(format!("codes{bits}.tif")), true);
        assert_eq!(descriptor(lab)["lab"]["depth"], bits);
        let photoshop = open_cmyk(&folder.join("cmyk.tif"));
        let press = press_of(photoshop);
        let theirs = ink_planes(photoshop);
        let mut row = Map::new();
        for name in LAB_CANDIDATES {
            let doc = lab_candidate(lab, &press, &options(name, bits));
            let (max, mean) = max_mean(&patch_residuals(&theirs, &ink_planes(doc), LAB_PATCHES));
            row.insert(name.into(), json!({"max": max, "mean": mean}));
            free(doc);
        }
        free(photoshop);
        free(lab);
        out.insert(format!("lab{bits}"), Value::Object(row));
    }
    Value::Object(out)
}

fn judge(root: &Path, out: &Path) {
    let mut judged = Map::new();
    for bits in [8u32, 16] {
        let depth = Depth::open(root, bits);
        let mut row = Map::new();
        for name in CANDIDATES {
            let (doc, theirs) = depth.candidate(name, bits);
            let patches = patch_residuals(&ink_planes(theirs), &ink_planes(doc), PATCHES);
            let (max, mean) = max_mean(&patches);
            row.insert(
                name.into(),
                json!({"max": max, "mean": mean, "patches": patches}),
            );
            free(doc);
        }
        judged.insert(format!("rgb{bits}"), Value::Object(row));
    }
    let lab = lab_take().map_or(Value::Null, |root| lab_remeasure(&root));
    judged.insert("lab".into(), lab);
    std::fs::write(out, Value::Object(judged).to_string() + "\n").unwrap();
}

#[test]
fn the_photoshop_take_records_issue_684() {
    if let Some(out) = std::env::var_os("DR_RGB_CMYK_JUDGE") {
        let out = PathBuf::from(out);
        let root = out
            .parent()
            .expect("judged.json's take folder")
            .to_path_buf();
        judge(&root, &out);
        return;
    }
    let Some(root) = fixtures().map(|f| f.join("rgb-cmyk-684/take1")) else {
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
    // The take is usable: a verdict of the build table, no stop, the judge
    // ran, both cases valid.
    let verdict = decided["verdict"].as_str().unwrap_or("");
    assert!(
        verdict == "VALID" || verdict == "MISS",
        "verdict {verdict:?}"
    );
    for key in ["stop", "judge_error"] {
        assert!(
            decided.get(key).is_some_and(Value::is_null),
            "{key}: {:?}",
            decided.get(key)
        );
    }
    for key in ["rgb8", "rgb16"] {
        assert_eq!(
            decided["cases"][key]["valid"], true,
            "{key}: {}",
            decided["cases"][key]
        );
    }
    for (bits, recorded) in RECORDED_MAX {
        let depth = Depth::open(&root, bits);
        let (shipped, theirs) = depth.candidate("C1", bits);
        let explicit = to_cmyk(
            &depth.scene,
            &depth.press,
            &json!({"depth":bits,"intent":"relative","black_point_compensation":true}),
        );
        assert_eq!(
            unsafe { &*shipped }.cmyk_layer_samples(0),
            unsafe { &*explicit }.cmyk_layer_samples(0),
            "{bits} bits: C1 is relative with BPC"
        );
        let (max, _) = max_mean(&patch_residuals(
            &ink_planes(theirs),
            &ink_planes(shipped),
            PATCHES,
        ));
        assert!(
            max <= recorded + 1e-9,
            "{bits} bits: C1's max |r| {max} > {recorded}"
        );
        free(explicit);
        free(shipped);
    }
    if let Some(lab_root) = lab_take() {
        let lab = lab_remeasure(&lab_root);
        for (bits, recorded) in LAB_RECORDED_MAX {
            let max = lab[format!("lab{bits}")]["C1"]["max"].as_f64().unwrap();
            assert!(
                (max - recorded).abs() <= 1e-6,
                "lab{bits}: C1's re-measured max {max}, recorded {recorded}"
            );
        }
    }
}
