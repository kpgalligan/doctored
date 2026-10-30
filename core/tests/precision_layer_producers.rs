//! Black-box suite for 13B-3d-3a: every typed LAYER producer that flips
//! through an existing unseeded constructor or `rebuilt_*` leaves no view —
//! the merges (`merge_layers`, `merge_down` through the members render,
//! `merge_visible`), `stamp_visible`, `flatten`, the preserve-appearance
//! bake, the warp output, the artwork render, Rasterize, the converted paste
//! and `ClipboardPayload::converted`, `float_to_u16` and the structure
//! narrowing to u16 — with every answer, checkpoint count, report log and
//! refusal unchanged.
//!
//! Oracles are independent of the implementation: a flatten / merge-visible
//! / stamp leaf is the input document's own `render` (`same_samples`), and
//! every other answer is pinned by its record line against HEAD.
//!
//! `record_layer_producers` prints one line per site, depth and fixture —
//! `producer <site>/<depth> <WxH> <sha256> checkpoints=<n> reports=<sha256>`,
//! the hash of `null`, `err:<message>` or every answer layer's dimensions,
//! offset and little-endian samples (u16 codes, f32 bits; the artwork line
//! also hashes the rendered handle) — so the same file run against HEAD and
//! the tree diffs byte-identical answers and checkpoint/report sequences. It
//! uses only APIs present at `63421b3`.
//!
//! `no_view_layer_producers` fails at `63421b3` by design (HEAD seeds each
//! output); the other two tests pass there. The warp arm holds no view since
//! 13B-3e-3a: a non-translation warp reads its source through ONE transient
//! copy (`transient_rgba16`, freed on return) instead of binding it whole.
//! The view cache is process-wide, so every test holds `serial()`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{dr_artwork_render, dr_precision_doc_artwork};
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_output_color::dr_precision_doc_convert_to_profile_with;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{
    ClipboardPayload, ClipboardRead, PastePlacement, PasteProfile, PasteRequest,
};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::tiff_float::FloatTiff;
use doctored_core::warp::WarpEdit;
use serde_json::{json, Value};

use crate::common::sha256;
use crate::resource_report;
use crate::serial;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 600;
const H: u32 = 520;
/// 600×520 crosses both tile seams (x 256, 512; y 256, 512).
const FIXTURES: [(u32, u32); 3] = [(W, H), (257, 3), (1, 1)];
const PROFILE_DISPLAY_P3: std::ffi::c_int = 1;

// ------------------------------------------------------------- plumbing --

fn pattern16(width: u32, height: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                (9000 + (x * 3001 + y * 1777 + seed * 97) % 56536) as u16,
            ]
        })
        .collect()
}

/// Extended-range samples (negative and above-white RGB) with alpha 0
/// (hidden RGB), 1 and fractions, row-major.
fn extended(width: u32, height: u32, seed: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let v = ((x * 37 + y * 101 + seed * 13) % 1000) as f32 / 1000.0;
            let r = if (x + y) % 9 == 0 { 0.0 } else { v * 6.0 - 1.0 };
            let g = if (x * y) % 7 == 3 { -0.25 } else { 1.0 - v };
            let b = ((x ^ y) % 97) as f32 / 24.0;
            let a = match (x + 2 * y) % 11 {
                0 => 0.0,
                1 | 2 => 1.0,
                k => k as f32 / 11.0,
            };
            [r, g, b, a]
        })
        .collect()
}

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    text
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

/// Accumulates one line's answers, checkpoints and report logs.
#[derive(Default)]
struct Line {
    answers: Vec<u8>,
    checkpoints: u64,
    log: String,
}

impl Line {
    fn add<T>(&mut self, (out, checkpoints, log): (T, u64, String)) -> T {
        self.checkpoints += checkpoints;
        self.log.push_str(&log);
        out
    }

    fn print(&self, label: &str, (w, h): (u32, u32)) {
        println!(
            "producer {label} {w}x{h} {} checkpoints={} reports={}",
            sha256::sha256_hex(&self.answers),
            self.checkpoints,
            sha256::sha256_hex(self.log.as_bytes())
        );
    }
}

/// An image's dimensions and samples as bytes (u16 codes, f32 bits,
/// little-endian).
fn image_bytes(image: &PrecisionImage) -> Vec<u8> {
    let (w, h) = image.dimensions();
    let mut out: Vec<u8> = [w, h].iter().flat_map(|v| v.to_le_bytes()).collect();
    match image.storage() {
        PrecisionStorage::Rgba16Unorm => out.extend(
            image
                .to_rgba16()
                .unwrap()
                .iter()
                .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes())),
        ),
        PrecisionStorage::Rgba32Float => out.extend(
            image
                .to_rgba32()
                .unwrap()
                .iter()
                .flat_map(|p| p.iter().flat_map(|c| c.to_bits().to_le_bytes())),
        ),
    }
    out
}

/// Every record's offset and raster (dimensions and samples), in postorder.
fn document_bytes(doc: &PrecisionDocument) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..doc.layer_count() {
        let layer = doc.layer(i).unwrap();
        let (x, y) = layer.offset();
        out.extend(x.to_le_bytes());
        out.extend(y.to_le_bytes());
        if let Some(image) = layer.source() {
            out.extend(image_bytes(image));
        }
    }
    out
}

/// `null`, `err:<message>` or [`document_bytes`].
fn outcome_bytes(outcome: &Result<Option<PrecisionDocument>, String>) -> Vec<u8> {
    match outcome {
        Ok(Some(doc)) => document_bytes(doc),
        Ok(None) => b"null".to_vec(),
        Err(e) => format!("err:{e}").into_bytes(),
    }
}

fn builtin_bytes(which: std::ffi::c_int) -> Vec<u8> {
    let len = dr_builtin_profile_len(which);
    let mut out = vec![0u8; len];
    assert!(unsafe { dr_builtin_profile(which, out.as_mut_ptr(), len) });
    out
}

// ------------------------------------------------------------- fixtures --

/// Two u16 rasters in `profile`: an opaque canvas-sized base and the "Off"
/// layer (`w/2`×`5h/13` at `(−w/6, 10h/13)`, opacity 0.8), partly off the
/// canvas.
fn two_layer_document(w: u32, h: u32, profile: Arc<IccProfile>) -> PrecisionDocument {
    let (ow, oh, ox, oy) = (
        (w / 2).max(1),
        (h * 5 / 13).max(1),
        -((w / 6) as i32),
        (h * 10 / 13) as i32,
    );
    let base: Vec<[u16; 4]> = pattern16(w, h, 1)
        .into_iter()
        .map(|[r, g, b, _]| [r, g, b, u16::MAX])
        .collect();
    let image = |w: u32, h: u32, pixels: Vec<[u16; 4]>| {
        PrecisionImage::from_rgba16(w, h, pixels, profile.clone(), None).unwrap()
    };
    let layers = vec![
        Layer::raster("Base", image(w, h, base), (0, 0), 1.0, true).unwrap(),
        Layer::raster(
            "Off",
            image(ow, oh, pattern16(ow, oh, 2)),
            (ox, oy),
            0.8,
            true,
        )
        .unwrap(),
    ];
    PrecisionDocument::from_parts(
        w,
        h,
        profile.clone(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

/// A float document holding one `width`×`height` raster of `samples` (RGBA,
/// straight alpha), through a float TIFF opened with `open_preserving`.
fn float_document(width: u32, height: u32, samples: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples: samples.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("producers.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// A window across both seams at 600×520, contained in every fixture.
fn seam_window(w: u32, h: u32) -> (u32, u32, u32, u32) {
    (w / 6, h / 5, (w * 3 / 4).max(1), (h * 4 / 5).max(1))
}

fn copy_read(layer: usize, (x, y, width, height): (u32, u32, u32, u32)) -> ClipboardRead {
    ClipboardRead::Copy {
        layer,
        x,
        y,
        width,
        height,
    }
}

fn paste_request() -> PasteRequest {
    PasteRequest {
        name: "Pasted".into(),
        anchor: 0,
        position: InsertPosition::Top,
        placement: PastePlacement::InPlace,
        profile: PasteProfile::Convert,
        frame: None,
    }
}

/// Two float rasters: the base and a seam window of a second float
/// document's samples pasted in place above it.
fn float_two_layer_document(w: u32, h: u32) -> PrecisionDocument {
    let doc = float_document(w, h, &extended(w, h, 5));
    let other = float_document(w, h, &extended(w, h, 9));
    let payload = other
        .clipboard_copy(&copy_read(0, seam_window(w, h)), None, LIMIT, 0, None)
        .expect("a float copy");
    doc.clipboard_paste(&payload, &paste_request(), LIMIT, 0, None)
        .expect("a float paste")
}

/// Canvas-sized u16 coverage with 0, 65535 and fractions.
fn ramp_coverage(w: u32, h: u32) -> Vec<u16> {
    (0..w * h)
        .map(|i| match i % 5 {
            0 => 0,
            1 | 2 => u16::MAX,
            _ => ((i * 7919) % 65536) as u16,
        })
        .collect()
}

/// An opaque u16 raster plus a `precision_v1` invert adjustment
/// (`precision_projection_unseeded.rs`' `bake_document(None)` shape).
fn bake_document(w: u32, h: u32) -> PrecisionDocument {
    let samples: Vec<[u16; 4]> = pattern16(w, h, 7)
        .into_iter()
        .map(|[r, g, b, _]| [r, g, b, u16::MAX])
        .collect();
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster(
            "Base",
            PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None).unwrap(),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()],
    )
    .unwrap();
    doc.add_adjustment(
        "Invert",
        &json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string(),
        1.0,
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}

/// A 3×3 bilinear warp of layer 0 whose nodes move differently (not a
/// translation).
fn warp_request() -> WarpEdit {
    let (columns, rows) = (3_u32, 3_u32);
    let values: Vec<[f64; 2]> = (0..rows)
        .flat_map(|y| {
            (0..columns).map(move |x| {
                let fx = f64::from(x) / f64::from(columns - 1);
                let fy = f64::from(y) / f64::from(rows - 1);
                [
                    0.35 + 0.58 * fx - 0.31 * fy + 0.12 * fx * fy,
                    -0.27 + 0.33 * fy + 0.19 * fx - 0.17 * fx * fy,
                ]
            })
        })
        .collect();
    let request = json!({"op":"set","layer":0,"target":"pixels","columns":columns,"rows":rows,
        "displacements":values,"filter":"bilinear"});
    WarpEdit::parse(&request.to_string()).expect("a warp request")
}

// ---------------------------------------------------------------- sites --

/// `dr_precision_doc_edit` with optional coverage.
fn edit(
    doc: &PrecisionDocument,
    request: &Value,
    coverage: &[u16],
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if coverage.is_empty() {
                ptr::null()
            } else {
                coverage.as_ptr()
            },
            coverage.len(),
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

/// The preserve-appearance conversion to Display P3.
fn bake(
    doc: &PrecisionDocument,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let p3 = builtin_bytes(PROFILE_DISPLAY_P3);
    let options = CString::new(r#"{"adjustments":"preserve_appearance"}"#).unwrap();
    let mut error = ptr::null_mut();
    let mut report = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_profile_with(
            doc,
            p3.as_ptr(),
            p3.len(),
            options.as_ptr(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut report,
            &mut error,
        )
    };
    if !report.is_null() {
        unsafe { dr_string_free(report) };
    }
    if out.is_null() {
        return Err(take(error));
    }
    Ok(Some(*unsafe { Box::from_raw(out) }))
}

/// `dr_artwork_render` of one red coverage over `ramp_coverage`.
fn artwork_render(
    w: u32,
    h: u32,
    control: Option<&DrJobControl>,
) -> Result<PrecisionImage, String> {
    let description = CString::new(r#"[{"kind":"coverage","color":[1,0,0,1]}]"#).unwrap();
    let samples = ramp_coverage(w, h);
    let profile = IccProfile::srgb();
    let mut error = ptr::null_mut();
    let image = unsafe {
        dr_artwork_render(
            w,
            h,
            description.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if image.is_null() {
        return Err(take(error));
    }
    Ok(*unsafe { Box::from_raw(image) })
}

/// `dr_precision_doc_artwork` publishing `image` as a new top layer.
fn artwork_publish(
    doc: &PrecisionDocument,
    image: &PrecisionImage,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let name = CString::new("Artwork").unwrap();
    let description = CString::new(r#"{"type":"shape","version":1}"#).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork(
            doc,
            0,
            false,
            image,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
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

/// `precision_constructor_seeds.rs`' Doctored request: a 300×200 shape at
/// (−40, 350) over `ramp_coverage(300, 200)`.
fn rasterize_request() -> (Value, Vec<u16>) {
    let (rw, rh) = (300_u32, 200_u32);
    (
        json!({"op":"rasterize","name":"Shape","width":rw,"height":rh,"x":-40,"y":350,
            "colors":[[0.25,0.5,0.75,1.0]],"color_space":"document"}),
        ramp_coverage(rw, rh),
    )
}

/// The inputs of one fixture.
struct Inputs {
    size: (u32, u32),
    u16_doc: PrecisionDocument,
    f32_doc: PrecisionDocument,
    bake_doc: PrecisionDocument,
    p3_payload: ClipboardPayload,
}

impl Inputs {
    fn new((w, h): (u32, u32)) -> Self {
        let p3 = two_layer_document(w, h, IccProfile::display_p3());
        let coverage = ramp_coverage(w, h);
        let p3_payload = p3
            .clipboard_copy(
                &copy_read(1, seam_window(w, h)),
                Some(&coverage),
                LIMIT,
                0,
                None,
            )
            .expect("a P3 payload");
        Self {
            size: (w, h),
            u16_doc: two_layer_document(w, h, IccProfile::srgb()),
            f32_doc: float_two_layer_document(w, h),
            bake_doc: bake_document(w, h),
            p3_payload,
        }
    }

    fn depth(&self, depth: &str) -> &PrecisionDocument {
        match depth {
            "u16" => &self.u16_doc,
            _ => &self.f32_doc,
        }
    }
}

/// What a site answers: the document it produces (and, for the artwork,
/// the rendered handle; for `converted`, the converted payload).
enum Answer {
    Document(Result<Option<PrecisionDocument>, String>),
    Artwork(Result<(PrecisionImage, Option<PrecisionDocument>), String>),
    Payload(Result<Option<ClipboardPayload>, String>),
}

impl Answer {
    fn bytes(&self) -> Vec<u8> {
        match self {
            Answer::Document(outcome) => outcome_bytes(outcome),
            Answer::Artwork(Ok((image, doc))) => {
                let mut out = image_bytes(image);
                out.extend(outcome_bytes(&Ok(doc.clone())));
                out
            }
            Answer::Artwork(Err(e)) => format!("err:{e}").into_bytes(),
            Answer::Payload(Ok(Some(payload))) => image_bytes(payload.image()),
            Answer::Payload(Ok(None)) => b"null".to_vec(),
            Answer::Payload(Err(e)) => format!("err:{e}").into_bytes(),
        }
    }
}

const STRUCTURE: [&str; 5] = [
    "merge_layers",
    "merge_down",
    "merge_visible",
    "stamp_visible",
    "flatten",
];

fn structure_request(site: &str) -> Value {
    match site {
        "merge_layers" => json!({"op":"merge_layers","layers":[0,1]}),
        "merge_down" => json!({"op":"merge_down","layer":1}),
        "merge_visible" => json!({"op":"merge_visible"}),
        "stamp_visible" => json!({"op":"stamp_visible","name":"Stamp","anchor":1}),
        "flatten" => json!({"op":"flatten"}),
        other => panic!("not a structure site: {other}"),
    }
}

/// Every `(site, depth)` pair this suite runs.
fn sites() -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    for site in STRUCTURE {
        out.push((site, "u16"));
        out.push((site, "f32"));
    }
    out.extend([
        ("bake", "u16"),
        ("warp", "u16"),
        ("artwork", "u16"),
        ("rasterize", "u16"),
        ("paste_convert", "u16"),
        ("converted", "u16"),
        ("float_to_u16", "f32"),
        ("narrow_to_u16", "f32"),
    ]);
    out
}

/// Runs one site over `inputs`, each job under its own recording control,
/// accumulated into `line`.
fn run(site: &str, depth: &str, inputs: &Inputs, line: &mut Line) -> Answer {
    let doc = inputs.depth(depth);
    let (w, h) = inputs.size;
    match site {
        s if STRUCTURE.contains(&s) => {
            let request = structure_request(s);
            Answer::Document(line.add(recorded(|c| edit(doc, &request, &[], Some(c)))))
        }
        "bake" => Answer::Document(line.add(recorded(|c| bake(&inputs.bake_doc, Some(c))))),
        "warp" => {
            let request = warp_request();
            Answer::Document(line.add(recorded(|c| doc.warp_edit(&request, LIMIT, 0, Some(c)))))
        }
        "artwork" => {
            let image = line.add(recorded(|c| artwork_render(w, h, Some(c))));
            Answer::Artwork(image.and_then(|image| {
                let doc = line.add(recorded(|c| artwork_publish(doc, &image, Some(c))))?;
                Ok((image, doc))
            }))
        }
        "rasterize" => {
            let (request, coverage) = rasterize_request();
            Answer::Document(line.add(recorded(|c| edit(doc, &request, &coverage, Some(c)))))
        }
        "paste_convert" => Answer::Document(line.add(recorded(|c| {
            doc.clipboard_paste(&inputs.p3_payload, &paste_request(), LIMIT, 0, Some(c))
                .map(Some)
        }))),
        "converted" => Answer::Payload(line.add(recorded(|c| {
            inputs.p3_payload.converted(
                PrecisionStorage::Rgba16Unorm,
                &IccProfile::srgb(),
                LIMIT,
                0,
                Some(c),
            )
        }))),
        "float_to_u16" => {
            Answer::Document(line.add(recorded(|c| doc.float_to_u16(LIMIT, 0, Some(c)).map(Some))))
        }
        "narrow_to_u16" => Answer::Document(line.add(recorded(|c| {
            doc.narrow_structure_to_u16(LIMIT, 0, Some(c)).map(Some)
        }))),
        other => panic!("unknown site {other}"),
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn record_layer_producers() {
    let _serial = serial();
    for size in FIXTURES {
        let inputs = Inputs::new(size);
        for (site, depth) in sites() {
            let mut line = Line::default();
            evict();
            let answer = run(site, depth, &inputs, &mut line);
            line.answers.extend(answer.bytes());
            line.print(&format!("{site}/{depth}"), size);
        }
    }
}

#[test]
fn no_view_layer_producers() {
    let _serial = serial();
    let inputs = Inputs::new((W, H));
    for (site, depth) in sites() {
        let label = format!("{site}/{depth}");
        evict();
        assert_eq!(contiguous_view_bytes(), 0, "{label}: evicted");
        let mut line = Line::default();
        let answer = run(site, depth, &inputs, &mut line);
        let ok = match &answer {
            Answer::Document(outcome) => matches!(outcome, Ok(Some(_))),
            Answer::Artwork(outcome) => matches!(outcome, Ok((_, Some(_)))),
            Answer::Payload(outcome) => matches!(outcome, Ok(Some(_))),
        };
        assert!(ok, "{label}: a produced answer");
        assert_eq!(
            contiguous_view_bytes(),
            0,
            "{label}: the held answer's produced layer holds no view (13B-3d-3a)"
        );
        drop(answer);
    }
}

#[test]
fn layer_renders_equal_the_projection() {
    let _serial = serial();
    for size in FIXTURES {
        let inputs = Inputs::new(size);
        for depth in ["u16", "f32"] {
            let doc = inputs.depth(depth);
            let projection = doc.render(LIMIT, 0, None).expect("the projection");
            for (site, leaf) in [("flatten", 0), ("merge_visible", 0), ("stamp_visible", 2)] {
                let label = format!("{site}/{depth} {size:?}");
                let out = edit(doc, &structure_request(site), &[], None)
                    .unwrap_or_else(|e| panic!("{label}: {e}"))
                    .unwrap_or_else(|| panic!("{label}: a changed document"));
                let layer = out.layer(leaf).expect("the produced record");
                assert_eq!(layer.offset(), (0, 0), "{label}: at the canvas origin");
                assert!(
                    layer.source().expect("a raster").same_samples(&projection),
                    "{label}: the layer is the input's projection"
                );
            }
        }
    }
}
