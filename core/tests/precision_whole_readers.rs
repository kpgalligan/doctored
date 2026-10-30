//! Black-box suite for 13B-3e-3a: the whole readers of an installed typed
//! layer or caller image — the selected Pixelate and automatic tone
//! (`selected_filter`, `selected_auto_levels`), the RGBA plane read, the
//! refinement cutout's nonempty scan, the warp's source read and its WARP
//! encode — read tiles instead of binding a whole-plane view, and the scalar
//! filter's gray image is built unseeded, with every answer, refusal,
//! checkpoint count and report log unchanged.
//!
//! Oracles are independent of the implementation: a plane read equals the
//! layer's own `to_rgba16()` codes (and the placement written HERE); an
//! opaque Pixelate equals the u64 block means computed HERE within 1 code; a
//! cutout's new layer is the foreground's samples under the caller's matte.
//!
//! `record_whole_reads` prints one line per case and fixture —
//! `whole <case> <WxH> <sha256> checkpoints=<n> reports=<sha256>` (the hash
//! of `null`, `err:<message>` or the answer's bytes; every call runs under a
//! recording `DrJobControl`) — so the same file run against HEAD and the
//! tree diffs byte-identical answers and checkpoint/report sequences. It
//! uses only APIs present at `a2541cf`.
//!
//! `no_view_whole_reads` fails at `a2541cf` by design (there each reader
//! binds its input's view); the other tests pass there. The view cache is
//! process-wide, so every test holds `serial()`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{
    dr_precision_doc_edit, dr_precision_doc_edit_plan, dr_precision_doc_read_plane,
    dr_precision_doc_scalar_auto_levels,
};
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_auto_levels;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{
    ClipboardPayload, ClipboardRead, PastePlacement, PasteProfile, PasteRequest,
};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::selected_adjustment::SelectedEdit;
use doctored_core::warp::WarpEdit;
use serde_json::{json, Value};

use crate::common::sha256;
use crate::resource_report;
use crate::serial;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 600;
const H: u32 = 520;
/// 600×520 has four tile shapes, 512×512 four tiles of one shape, 257×3 two
/// shapes and 1×1 one.
const FIXTURES: [(u32, u32); 4] = [(W, H), (512, 512), (257, 3), (1, 1)];

const CASES: [&str; 15] = [
    "selected/pixelate7",
    "selected/pixelate7_rect",
    "selected/auto_color_selection",
    "selected/auto_tone_apply",
    "plane/red",
    "plane/alpha_canvas",
    "scalar/pixelate5",
    "scalar/auto_contrast",
    "cutout/matte",
    "cutout/empty",
    "warp/edit",
    "warp/reedit",
    "warp/save",
    "paste/read_plane",
    "paste/from_clipboard_pixelate",
];

// ------------------------------------------------------------- plumbing --

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    text
}

/// An owned string answer, or the error.
fn owned_text(result: *mut c_char, error: *mut c_char) -> Result<String, String> {
    if result.is_null() {
        return Err(take(error));
    }
    assert!(error.is_null());
    let text = unsafe { CStr::from_ptr(result) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(result) };
    Ok(text)
}

fn evict() {
    resource_report::evict(&[]).expect("an eviction report");
}

/// Runs `call` under a fresh recording `DrJobControl`; answers the call's
/// value, the checkpoint count and the raw report log.
fn recorded<T>(call: impl FnOnce(&DrJobControl) -> T) -> (T, u64, String) {
    let control = DrJobControl::default();
    control.record_reports();
    let out = call(&control);
    let log: String = control
        .reports()
        .iter()
        .map(|r| format!("{} {} {}\n", r.checkpoints, r.progress.to_bits(), r.stage))
        .collect();
    (out, control.checkpoints(), log)
}

// ------------------------------------------------------------- fixtures --

/// A deterministic u16 pattern: alpha 0 (nonzero hidden RGB), 65535 and
/// partial samples; `opaque` forces alpha 65535.
fn pattern(width: u32, height: u32, seed: u32, opaque: bool) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let alpha = match (x + 2 * y + seed) % 11 {
                0 => 0,
                1 | 2 => u16::MAX,
                k => (k * 5953) as u16,
            };
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16 | 1,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                if opaque { u16::MAX } else { alpha },
            ]
        })
        .collect()
}

fn image(width: u32, height: u32, seed: u32, opaque: bool) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        width,
        height,
        pattern(width, height, seed, opaque),
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

/// The second raster: half size at `(−w/6, 2h/3)`, partly off the canvas.
fn second_placement((w, h): (u32, u32)) -> ((u32, u32), (i32, i32)) {
    (
        ((w / 2).max(1), (h / 2).max(1)),
        (-((w / 6) as i32), (h * 2 / 3) as i32),
    )
}

/// A canvas-sized base plus the second raster (opacity 0.8).
fn document((w, h): (u32, u32), opaque: bool) -> PrecisionDocument {
    let ((sw, sh), offset) = second_placement((w, h));
    let layers = vec![
        Layer::raster("Base", image(w, h, 1, opaque), (0, 0), 1.0, true).unwrap(),
        Layer::raster("Off", image(sw, sh, 2, opaque), offset, 0.8, true).unwrap(),
    ];
    PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

/// Canvas coverage 65535 where `x < w/2 + 3`.
fn rect_coverage((w, h): (u32, u32)) -> Vec<u16> {
    (0..w * h)
        .map(|i| if i % w < w / 2 + 3 { u16::MAX } else { 0 })
        .collect()
}

/// Canvas matte 65535 in `[w/4, 3w/4) × [h/4, 3h/4)`.
fn centred_matte((w, h): (u32, u32)) -> Vec<u16> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if (w / 4..w * 3 / 4).contains(&x) && (h / 4..h * 3 / 4).contains(&y) {
                u16::MAX
            } else {
                0
            }
        })
        .collect()
}

/// A `columns`×`rows` bilinear lattice of non-uniform displacements on
/// layer 0's pixels (not a translation); `k` varies the field.
fn warp_request(columns: u32, rows: u32, k: f64) -> WarpEdit {
    let values: Vec<[f64; 2]> = (0..rows)
        .flat_map(|y| {
            (0..columns).map(move |x| {
                let fx = f64::from(x) / f64::from(columns - 1);
                let fy = f64::from(y) / f64::from(rows - 1);
                [
                    k * (0.35 + 0.58 * fx - 0.31 * fy + 0.12 * fx * fy),
                    k * (-0.27 + 0.33 * fy + 0.19 * fx - 0.17 * fx * fy),
                ]
            })
        })
        .collect();
    let request = json!({"op":"set","layer":0,"target":"pixels","columns":columns,"rows":rows,
        "displacements":values,"filter":"bilinear"});
    WarpEdit::parse(&request.to_string()).expect("a warp request")
}

fn selected(op: &str, params: Value, scope: &str) -> SelectedEdit {
    SelectedEdit::parse(
        &json!({"layer":0,"op":op,"params":params,"analysis_scope":scope}).to_string(),
    )
    .unwrap()
}

// ---------------------------------------------------------------- sites --

fn plan(doc: &PrecisionDocument, request: &Value) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut error) };
    Ok(serde_json::from_str(&owned_text(out, error)?).unwrap())
}

/// `dr_precision_doc_read_plane` sized by its plan.
fn read_plane(
    doc: &PrecisionDocument,
    target: &str,
    index: usize,
    canvas: bool,
    control: Option<&DrJobControl>,
) -> Result<Vec<u16>, String> {
    let request = json!({"op":"read_plane","target":target,"index":index,"canvas":canvas});
    let n = plan(doc, &request)?["output_samples"].as_u64().unwrap() as usize;
    let mut out = vec![0_u16; n];
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_read_plane(
            doc,
            text.as_ptr(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if !ok {
        return Err(take(error));
    }
    Ok(out)
}

/// `dr_precision_doc_edit` of a `scalar_edit` on layer 0 (no selection),
/// its input sized by the plan.
fn scalar_edit(
    doc: &PrecisionDocument,
    target: &str,
    operation: Value,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let request = json!({"op":"scalar_edit","target":target,"index":0,"operation":operation,
        "selection":false});
    let n = plan(doc, &request)?["input_samples"].as_u64().unwrap() as usize;
    let input = vec![0_u16; n];
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            input.as_ptr(),
            input.len(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if !error.is_null() {
        return Err(take(error));
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn selected_auto_levels(
    doc: &PrecisionDocument,
    request: &Value,
    coverage: &[u16],
    control: Option<&DrJobControl>,
) -> Result<String, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_auto_levels(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    owned_text(out, error)
}

fn scalar_auto_levels(
    doc: &PrecisionDocument,
    request: &Value,
    control: Option<&DrJobControl>,
) -> Result<String, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_scalar_auto_levels(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    owned_text(out, error)
}

fn whole_copy((w, h): (u32, u32)) -> ClipboardRead {
    ClipboardRead::Copy {
        layer: 0,
        x: 0,
        y: 0,
        width: w,
        height: h,
    }
}

fn centred_paste() -> PasteRequest {
    PasteRequest {
        name: "Pasted".into(),
        anchor: 0,
        position: InsertPosition::Top,
        placement: PastePlacement::Centered,
        profile: PasteProfile::Convert,
        frame: None,
    }
}

// ---------------------------------------------------------------- cases --

/// One case's untimed inputs.
struct Built {
    doc: PrecisionDocument,
    coverage: Vec<u16>,
    foreground: Option<PrecisionImage>,
    payload: Option<ClipboardPayload>,
}

fn build(name: &str, size: (u32, u32), opaque: bool) -> Built {
    let doc = document(size, opaque);
    let mut built = Built {
        doc,
        coverage: Vec::new(),
        foreground: None,
        payload: None,
    };
    match name {
        "selected/pixelate7_rect" | "selected/auto_color_selection" => {
            built.coverage = rect_coverage(size);
        }
        "cutout/matte" | "cutout/empty" => {
            built.foreground = Some(image(size.0, size.1, 3, opaque));
            built.coverage = if name == "cutout/matte" {
                centred_matte(size)
            } else {
                vec![0; size.0 as usize * size.1 as usize]
            };
        }
        "warp/reedit" | "warp/save" => {
            // A field the core refuses (the 1×1 contraction bound) leaves
            // the unwarped document as the input.
            if let Ok(Some(warped)) = built
                .doc
                .warp_edit(&warp_request(4, 4, 1.0), LIMIT, 0, None)
            {
                built.doc = warped;
            }
        }
        "paste/read_plane" => {
            let payload = built
                .doc
                .clipboard_copy(&whole_copy(size), None, LIMIT, 0, None)
                .unwrap();
            built.doc = built
                .doc
                .clipboard_paste(&payload, &centred_paste(), LIMIT, 0, None)
                .unwrap();
        }
        "paste/from_clipboard_pixelate" => {
            let payload = built
                .doc
                .clipboard_copy(&whole_copy(size), None, LIMIT, 0, None)
                .unwrap();
            built.doc = PrecisionDocument::from_clipboard(&payload, LIMIT, 0, None).unwrap();
            built.payload = Some(payload);
        }
        _ => {}
    }
    built
}

/// A call's answer, alive until dropped.
#[allow(clippy::large_enum_variant)]
enum Answer {
    Doc(PrecisionDocument),
    Samples(Vec<u16>),
    Text(String),
    Bytes(Vec<u8>),
}

impl Answer {
    /// Documents: per layer offset (i32 LE ×2), dims, `to_rgba16()` LE,
    /// mask samples LE when present; plane reads u16 LE; JSON text; `.rz`
    /// bytes.
    fn bytes(&self) -> Vec<u8> {
        match self {
            Self::Doc(doc) => {
                let mut out = Vec::new();
                for i in 0..doc.layer_count() {
                    let layer = doc.layer(i).unwrap();
                    let (x, y) = layer.offset();
                    out.extend(x.to_le_bytes());
                    out.extend(y.to_le_bytes());
                    if let Some(image) = layer.source() {
                        let (w, h) = image.dimensions();
                        out.extend(w.to_le_bytes());
                        out.extend(h.to_le_bytes());
                        for p in image.to_rgba16().unwrap() {
                            for v in p {
                                out.extend(v.to_le_bytes());
                            }
                        }
                    }
                    if let Some(mask) = layer.mask() {
                        for v in mask.plane().samples() {
                            out.extend(v.to_le_bytes());
                        }
                    }
                }
                out
            }
            Self::Samples(samples) => samples.iter().flat_map(|v| v.to_le_bytes()).collect(),
            Self::Text(text) => text.as_bytes().to_vec(),
            Self::Bytes(bytes) => bytes.clone(),
        }
    }
}

type Outcome = Result<Option<Answer>, String>;

fn doc_answer(out: Result<Option<PrecisionDocument>, String>) -> Outcome {
    out.map(|d| d.map(Answer::Doc))
}

/// The case's CALL over its built inputs.
fn call(name: &str, built: &Built, control: Option<&DrJobControl>) -> Outcome {
    let doc = &built.doc;
    match name {
        "selected/pixelate7" | "selected/pixelate7_rect" => doc_answer(doc.selected_edit(
            &selected("pixelate", json!({"block":7}), "whole_target"),
            &built.coverage,
            LIMIT,
            0,
            control,
        )),
        "selected/auto_color_selection" => selected_auto_levels(
            doc,
            &json!({"layer":0,"op":"auto_color","params":{"clip":0.001},
                "analysis_scope":"selection"}),
            &built.coverage,
            control,
        )
        .map(|t| Some(Answer::Text(t))),
        "selected/auto_tone_apply" => doc_answer(doc.selected_edit(
            &selected("auto_tone", json!({}), "whole_target"),
            &[],
            LIMIT,
            0,
            control,
        )),
        "plane/red" => read_plane(doc, "red", 0, false, control).map(|s| Some(Answer::Samples(s))),
        "plane/alpha_canvas" => {
            read_plane(doc, "alpha", 1, true, control).map(|s| Some(Answer::Samples(s)))
        }
        "scalar/pixelate5" => doc_answer(scalar_edit(
            doc,
            "green",
            json!({"kind":"filter","edit":{"layer":0,"op":"pixelate","params":{"block":5}}}),
            control,
        )),
        "scalar/auto_contrast" => scalar_auto_levels(
            doc,
            &json!({"op":"scalar_edit","target":"blue","index":0,"selection":false,
                "operation":{"kind":"filter","edit":{"layer":0,"op":"auto_contrast",
                "params":{"clip":0.001}}}}),
            control,
        )
        .map(|t| Some(Answer::Text(t))),
        "cutout/matte" | "cutout/empty" => doc_answer(doc.add_refinement_cutout(
            built.foreground.as_ref().unwrap(),
            &built.coverage,
            LIMIT,
            0,
            control,
        )),
        "warp/edit" => doc_answer(doc.warp_edit(&warp_request(4, 4, 1.0), LIMIT, 0, control)),
        "warp/reedit" => doc_answer(doc.warp_edit(&warp_request(3, 3, 0.8), LIMIT, 0, control)),
        "warp/save" => doc.encode_native(LIMIT, 0, control).map(|mut stream| {
            // The trailing `LIDS` ids are process-minted: zeroed.
            let start = stream.len().saturating_sub(8 * doc.layer_count());
            stream[start..].fill(0);
            Some(Answer::Bytes(stream))
        }),
        "paste/read_plane" => {
            let pasted = doc.layer_count() - 1;
            read_plane(doc, "red", pasted, false, control).map(|s| Some(Answer::Samples(s)))
        }
        "paste/from_clipboard_pixelate" => doc_answer(doc.selected_edit(
            &selected("pixelate", json!({"block":7}), "whole_target"),
            &[],
            LIMIT,
            0,
            control,
        )),
        other => panic!("unknown case {other}"),
    }
}

fn outcome(outcome: &Outcome) -> Vec<u8> {
    match outcome {
        Ok(Some(answer)) => answer.bytes(),
        Ok(None) => b"null".to_vec(),
        Err(e) => format!("err:{e}").into_bytes(),
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn record_whole_reads() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        for name in CASES {
            let built = build(name, (w, h), false);
            evict();
            let (out, checkpoints, log) = recorded(|c| call(name, &built, Some(c)));
            println!(
                "whole {name} {w}x{h} {} checkpoints={checkpoints} reports={}",
                sha256::sha256_hex(&outcome(&out)),
                sha256::sha256_hex(log.as_bytes())
            );
        }
    }
}

#[test]
fn no_view_whole_reads() {
    let _serial = serial();
    for name in CASES {
        let built = build(name, (W, H), false);
        evict();
        let answer = call(name, &built, None).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(contiguous_view_bytes(), 0, "{name}: no view held");
        drop(answer);
    }
}

#[test]
fn plane_read_matches_the_samples() {
    let _serial = serial();
    for size in FIXTURES {
        let built = build("plane/red", size, false);
        let doc = &built.doc;
        for index in 0..2 {
            let samples = doc.layer_source(index).unwrap().to_rgba16().unwrap();
            for (c, target) in ["red", "green", "blue", "alpha"].iter().enumerate() {
                let read = read_plane(doc, target, index, false, None).unwrap();
                let expected: Vec<u16> = samples.iter().map(|p| p[c]).collect();
                assert_eq!(read, expected, "{size:?} layer {index} {target}");
            }
        }
        let Ok(Some(Answer::Samples(placed))) = call("plane/alpha_canvas", &built, None) else {
            panic!("{size:?}: a canvas alpha read")
        };
        let (w, h) = size;
        let ((sw, sh), (ox, oy)) = second_placement(size);
        let source = pattern(sw, sh, 2, false);
        let mut expected = vec![0_u16; w as usize * h as usize];
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let (sx, sy) = (x - i64::from(ox), y - i64::from(oy));
                if sx >= 0 && sy >= 0 && sx < i64::from(sw) && sy < i64::from(sh) {
                    expected[y as usize * w as usize + x as usize] =
                        source[sy as usize * sw as usize + sx as usize][3];
                }
            }
        }
        assert_eq!(placed, expected, "{size:?} alpha_canvas");
    }
}

#[test]
fn pixelate_matches_the_block_means() {
    let _serial = serial();
    const BLOCK: usize = 7;
    for size in FIXTURES {
        let built = build("selected/pixelate7", size, true);
        // `None` is a no-op edit: the input unchanged (a 1×1 block).
        let result = match call("selected/pixelate7", &built, None) {
            Ok(Some(Answer::Doc(out))) => out.layer_source(0).unwrap().to_rgba16().unwrap(),
            Ok(None) => built.doc.layer_source(0).unwrap().to_rgba16().unwrap(),
            _ => panic!("{size:?}: a pixelated document"),
        };
        let (w, h) = (size.0 as usize, size.1 as usize);
        let input = pattern(size.0, size.1, 1, true);
        for by in (0..h).step_by(BLOCK) {
            for bx in (0..w).step_by(BLOCK) {
                let (y1, x1) = ((by + BLOCK).min(h), (bx + BLOCK).min(w));
                let count = ((y1 - by) * (x1 - bx)) as u64;
                let mut sums = [0_u64; 3];
                for y in by..y1 {
                    for x in bx..x1 {
                        for (c, sum) in sums.iter_mut().enumerate() {
                            *sum += u64::from(input[y * w + x][c]);
                        }
                    }
                }
                let mean = sums.map(|s| ((s as f64) / count as f64).round() as u16);
                for y in by..y1 {
                    for x in bx..x1 {
                        let p = result[y * w + x];
                        for c in 0..3 {
                            assert!(
                                p[c].abs_diff(mean[c]) <= 1,
                                "{size:?} ({x},{y}) channel {c}: {} vs {}",
                                p[c],
                                mean[c]
                            );
                        }
                        assert_eq!(p[3], u16::MAX, "{size:?} ({x},{y}) alpha");
                    }
                }
            }
        }
    }
}

#[test]
fn cutout_layer_is_the_foreground() {
    let _serial = serial();
    for size in FIXTURES {
        let built = build("cutout/matte", size, false);
        let empty = build("cutout/empty", size, false);
        assert!(
            matches!(call("cutout/empty", &empty, None), Ok(None)),
            "{size:?}: an empty matte is a no-op"
        );
        if built.coverage.iter().all(|&v| v == 0) {
            // The centred half is empty at 1×1.
            assert!(matches!(call("cutout/matte", &built, None), Ok(None)));
            continue;
        }
        let Ok(Some(Answer::Doc(out))) = call("cutout/matte", &built, None) else {
            panic!("{size:?}: a cutout document")
        };
        let before = built.doc.layer_count();
        assert_eq!(out.layer_count(), before + 1, "{size:?}: one new layer");
        let top = out.layer(before).unwrap();
        assert_eq!(
            top.source().unwrap().to_rgba16().unwrap(),
            built.foreground.as_ref().unwrap().to_rgba16().unwrap(),
            "{size:?}: the foreground's samples"
        );
        assert_eq!(
            top.mask().expect("a matte mask").plane().samples(),
            &centred_matte(size)[..],
            "{size:?}: the mask is the matte"
        );
        for i in 0..before {
            assert!(
                !out.layer(i).unwrap().visible(),
                "{size:?}: layer {i} hidden"
            );
        }
    }
}
