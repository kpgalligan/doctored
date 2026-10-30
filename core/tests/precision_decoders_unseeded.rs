//! Black-box suite for 13B-3e-4b: every reader-side typed decoder — the
//! typed `.rz` layer / master PNG, the WARP original, the f32 `.rz` raster,
//! the 16-bit PNG / TIFF open, the 32-bit float TIFF open, the 16-bit PSD
//! layer and composite, and the RZCB / RGBA8 clipboard payloads — builds its
//! plane without seeding the view cache, a bitwise-uniform decoded plane
//! (the reopened empty layer) is ONE filled plane, and the narrowing's byte
//! planes are unseeded — with every answer, refusal, checkpoint count and
//! report log unchanged.
//!
//! Oracles are independent of the implementation: a decoded plane holds the
//! samples written HERE (or the in-memory document's, for a `.rz` stream);
//! a pixel-less PSD record is a canvas of zeros; an RZCB stream is built and
//! read little-endian HERE; an RGBA8 payload is `v·257`; a filled plane
//! retains exactly what a fresh `add_layer_at` plane of its size retains.
//!
//! `record_decoders` prints one line per case and fixture —
//! `decode <case> <WxH> <sha256> checkpoints=<n> reports=<sha256>` (the hash
//! of `err:<message>` or the answer's bytes; every call that takes a job
//! control runs under a recording `DrJobControl`, a multi-step case sums its
//! steps' checkpoints and concatenates their logs; every `.rz` stream hashed
//! has its trailing `LIDS` ids zeroed, since ids are process-minted) — so the
//! same file run against HEAD and the tree diffs byte-identical answers and
//! checkpoint/report sequences. `peak_decoders` prints
//! `peak <case> <WxH> peak=<bytes> held=<bytes>` (live heap bytes above the
//! case's baseline, from the tracking allocator below). It uses only APIs
//! present at `277c88a`.
//!
//! `no_view_decoders` and `reopened_uniform_planes_are_filled` fail at
//! `277c88a` by design (there every decoder seeds the view cache and a
//! uniform plane is dense); the other tests pass there. The view cache is
//! process-wide, so every test holds `serial()`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::doc::DrDocument;
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_develop_inspect::{
    dr_precision_doc_source_master16, dr_precision_image_develop_preview,
};
use doctored_core::ffi_psd::{dr_precision_doc_open_psd, dr_psd_import_report};
use doctored_core::ffi_retained_source::dr_precision_doc_source_edit;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{PastePlacement, PasteProfile, PasteRequest};
use doctored_core::precision_clipboard_codec::ClipboardPayload;
use doctored_core::precision_codec::{self, PrecisionFormat};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::precision_quantize::PrecisionDither;
use doctored_core::tiff_float::FloatTiff;
use doctored_core::warp::WarpEdit;
use serde_json::{json, Value};
use tempfile::TempDir;

#[path = "common/psd16_fixture.rs"]
mod psd16;
#[path = "common/resource_report.rs"]
mod resource_report;
#[path = "common/sha256.rs"]
mod sha256;

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

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 600;
const H: u32 = 520;
/// 600×520 has four tile shapes, 512×512 four tiles of one shape, 257×3 two
/// shapes and 1×1 one.
const FIXTURES: [(u32, u32); 4] = [(W, H), (512, 512), (257, 3), (1, 1)];
/// `dr_precision_doc_open_psd`'s modes.
const PSD_LAYERS: i32 = 0;
const PSD_COMPOSITE: i32 = 1;

const CASES: [&str; 18] = [
    "rz16/reopen",
    "rz16/master",
    "rzf/reopen",
    "png16/open",
    "tiff16/open",
    "psd16/layered",
    "psd16/composite",
    "tiff32/open",
    "drcb16/decode",
    "drcbf/decode",
    "rgba8/payload",
    "drcb16/paste",
    "narrow/u16_rgba8",
    "narrow/f32_rgba8",
    "render/u16",
    "render/f32",
    "pipeline/rz16_master_preview",
    "pipeline/png16_preview",
];

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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

/// `resource_report::report` over typed images.
fn report(images: &[&PrecisionImage]) -> Value {
    let handles: Vec<*const PrecisionImage> =
        images.iter().map(|i| *i as *const PrecisionImage).collect();
    resource_report::report(&[], &[], &[], &handles, 0, 0).expect("a resource report")
}

fn category(report: &Value, name: &str) -> u64 {
    report["categories"][name]["unique_bytes"]
        .as_u64()
        .unwrap_or(0)
}

/// Runs each step under a fresh `DrJobControl` (a control is single-use)
/// when recording; sums the checkpoint counts and concatenates the raw
/// report logs.
struct Recorder {
    recording: bool,
    checkpoints: u64,
    log: String,
}

impl Recorder {
    fn new(recording: bool) -> Self {
        Self {
            recording,
            checkpoints: 0,
            log: String::new(),
        }
    }
    fn step<T>(&mut self, call: impl FnOnce(Option<&DrJobControl>) -> T) -> T {
        if !self.recording {
            return call(None);
        }
        let control = DrJobControl::default();
        control.record_reports();
        let out = call(Some(&control));
        for r in control.reports() {
            self.log += &format!("{} {} {}\n", r.checkpoints, r.progress.to_bits(), r.stage);
        }
        self.checkpoints += control.checkpoints();
        out
    }
}

// ------------------------------------------------------------- fixtures --

/// A deterministic u16 pattern: alpha 0 (hidden RGB including 0 / 65535),
/// 65535 and partial samples, with 0 / 65535 components on alpha-positive
/// samples too.
fn pattern(width: u32, height: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let alpha = match (x + 3 * y + seed) % 13 {
                0 => 0,
                1..=4 => u16::MAX,
                k => (k * 4099) as u16,
            };
            let mut p = [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                (((x ^ y) * 1999 + seed) % 65536) as u16,
                alpha,
            ];
            match (x * 7 + y * 5 + seed) % 17 {
                0 => p[0] = 0,
                1 => p[1] = u16::MAX,
                2 => {
                    p[2] = 0;
                    p[0] = u16::MAX;
                }
                3 => p = [0, 0, 0, alpha],
                4 => p = [u16::MAX, u16::MAX, u16::MAX, alpha],
                _ => {}
            }
            p
        })
        .collect()
}

fn opaque(width: u32, height: u32, seed: u32) -> Vec<[u16; 4]> {
    pattern(width, height, seed)
        .into_iter()
        .map(|[r, g, b, _]| [r, g, b, u16::MAX])
        .collect()
}

/// Extended-range f32 samples (RGB in [−1, 3], alpha in [0, 1]).
fn pattern32(width: u32, height: u32) -> Vec<[f32; 4]> {
    pattern(width, height, 5)
        .into_iter()
        .map(|p| {
            let v = p.map(|c| f32::from(c) / 65535.0);
            [v[0] * 4.0 - 1.0, v[1] * 2.0, v[2] - 0.5, v[3]]
        })
        .collect()
}

fn pattern8(width: u32, height: u32) -> Vec<u8> {
    (0..width * height)
        .flat_map(|i| {
            let (x, y) = (i % width, i / width);
            [
                ((x * 13 + 29) % 256) as u8,
                ((y * 17 + 11) % 256) as u8,
                ((x + y) * 9 % 256) as u8,
                ((x * 23 + y * 41) % 256) as u8,
            ]
        })
        .collect()
}

fn le16(samples: &[[u16; 4]]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()))
        .collect()
}

fn le32(samples: &[[f32; 4]]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|p| p.iter().flat_map(|c| c.to_le_bytes()))
        .collect()
}

/// An RZCB v3 stream written HERE (the codec's module doc): no coverage,
/// no editable section, offset (0, 0), 72 ppi, tone map 0 / white 1.
fn drcb(storage: u32, (w, h): (u32, u32), profile: &[u8], samples: &[u8]) -> Vec<u8> {
    let mut out = b"RZCB".to_vec();
    for word in [3, storage, w, h] {
        out.extend(word.to_le_bytes());
    }
    out.extend(0_i32.to_le_bytes());
    out.extend(0_i32.to_le_bytes());
    out.extend(72.0_f32.to_le_bytes());
    out.extend(72.0_f32.to_le_bytes());
    out.extend((profile.len() as u32).to_le_bytes());
    out.extend(profile);
    out.push(0);
    out.extend(0_u32.to_le_bytes());
    out.extend(1.0_f32.to_le_bytes());
    out.extend(0_u32.to_le_bytes());
    out.extend(samples);
    out
}

/// The sample section of an [`drcb`] stream, read little-endian HERE.
fn drcb_samples16(stream: &[u8]) -> Vec<[u16; 4]> {
    let profile = u32::from_le_bytes(stream[36..40].try_into().unwrap()) as usize;
    stream[40 + profile + 1 + 8 + 4..]
        .chunks_exact(8)
        .map(|p| std::array::from_fn(|c| u16::from_le_bytes([p[2 * c], p[2 * c + 1]])))
        .collect()
}

fn drcb_samples32(stream: &[u8]) -> Vec<[f32; 4]> {
    let profile = u32::from_le_bytes(stream[36..40].try_into().unwrap()) as usize;
    stream[40 + profile + 1 + 8 + 4..]
        .chunks_exact(16)
        .map(|p| {
            std::array::from_fn(|c| f32::from_le_bytes(p[4 * c..4 * c + 4].try_into().unwrap()))
        })
        .collect()
}

fn image16(w: u32, h: u32, samples: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None).unwrap()
}

/// One canvas-sized sRGB u16 raster of `samples`.
fn u16_doc((w, h): (u32, u32), samples: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("Base", image16(w, h, samples), (0, 0), 1.0, true).unwrap()],
    )
    .unwrap()
}

/// A float document holding one raster of `samples`, through a float TIFF
/// opened with `open_preserving`.
fn float_doc((w, h): (u32, u32), samples: &[[f32; 4]]) -> PrecisionDocument {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("float.tiff");
    std::fs::write(&path, float_tiff((w, h), samples)).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

fn float_tiff((w, h): (u32, u32), samples: &[[f32; 4]]) -> Vec<u8> {
    let tiff = FloatTiff {
        width: w,
        height: h,
        channels: 4,
        alpha: Some(false),
        samples: samples.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("a float TIFF")
}

/// The `warp_fixture::field` lattice (3×3, bilinear) on layer 0's pixels.
fn warp_request() -> WarpEdit {
    let values: Vec<[f64; 2]> = (0..3)
        .flat_map(|y| {
            (0..3).map(move |x| {
                let (fx, fy) = (f64::from(x) / 2.0, f64::from(y) / 2.0);
                [
                    0.35 + 0.58 * fx - 0.31 * fy + 0.12 * fx * fy,
                    -0.27 + 0.33 * fy + 0.19 * fx - 0.17 * fx * fy,
                ]
            })
        })
        .collect();
    let request = json!({"op":"set","layer":0,"target":"pixels","columns":3,"rows":3,
        "displacements":values,"filter":"bilinear"});
    WarpEdit::parse(&request.to_string()).unwrap()
}

/// `doc` with the WARP on layer 0 (or `doc` itself where the warp refuses
/// or changes nothing, e.g. 1×1).
fn warped(doc: PrecisionDocument) -> PrecisionDocument {
    match doc.warp_edit(&warp_request(), LIMIT, 0, None) {
        Ok(Some(out)) => out,
        _ => doc,
    }
}

/// `doc` plus a top `add_layer_at` layer.
fn with_empty(doc: &PrecisionDocument) -> PrecisionDocument {
    doc.add_layer_at("Empty", 0, InsertPosition::Top)
        .expect("an added layer")
        .expect("a changed document")
}

/// What a fresh `add_layer_at` plane over `doc` retains.
fn fresh_retained(doc: &PrecisionDocument) -> u64 {
    let out = with_empty(doc);
    out.layer_source(out.layer_count() - 1)
        .expect("a raster")
        .retained_bytes()
}

fn native(doc: &PrecisionDocument) -> Vec<u8> {
    doc.encode_native(LIMIT, 0, None)
        .expect("a .doctored stream")
}

/// The `rz16/reopen` document: a pattern layer, WARPed, plus an
/// `add_layer_at` layer.
fn rz16_doc(size: (u32, u32)) -> PrecisionDocument {
    with_empty(&warped(u16_doc(size, pattern(size.0, size.1, 1))))
}

/// A pattern layer plus a layer placed from a u16 retained source (the
/// `retained_source_native.rs` `place` edit).
fn master_doc(size: (u32, u32)) -> PrecisionDocument {
    let base = u16_doc(size, pattern(size.0, size.1, 1));
    let source = u16_doc(size, pattern(size.0, size.1, 2));
    let request = CString::new(
        json!({"op":"place","source_id":"13b3e4b-master","name":"Original","x":0,"y":0})
            .to_string(),
    )
    .unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_source_edit(
            &base,
            &source,
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!out.is_null(), "the place: {}", take(error));
    *unsafe { Box::from_raw(out) }
}

fn rzf_doc(size: (u32, u32)) -> PrecisionDocument {
    with_empty(&float_doc(size, &pattern32(size.0, size.1)))
}

/// A u16 document holding one canvas-sized raster at (0, 0), WARPed, and
/// one `precision_v1` invert adjustment with a canvas-sized mask (13B-3d-3b's
/// masked-adjustment document plus the WARP).
fn masked_doc((w, h): (u32, u32)) -> PrecisionDocument {
    let doc = warped(u16_doc((w, h), opaque(w, h, 7)));
    let doc = doc
        .add_adjustment(
            "Invert",
            &json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"})
                .to_string(),
            1.0,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let top = doc.layer_count() - 1;
    let mask = MaskEdit::parse(
        &json!({"op":"set_mask","layer":top,"width":w,"height":h,"x":0,"y":0}).to_string(),
    )
    .unwrap();
    let values: Vec<u16> = (0..w * h)
        .map(|i| if i % w < w / 2 { u16::MAX } else { 10280 })
        .collect();
    doc.mask_edit(&mask, &values, LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

/// `precision_codec::save` of a pattern image to `name` in `directory`.
fn codec_file(
    directory: &Path,
    name: &str,
    (w, h): (u32, u32),
    format: PrecisionFormat,
) -> PathBuf {
    let path = directory.join(name);
    precision_codec::save(
        &image16(w, h, pattern(w, h, 4)),
        &Metadata::default(),
        Resolution::default(),
        path.to_str().unwrap(),
        format,
        true,
        false,
        LIMIT,
        0,
        None,
    )
    .expect("the codec file");
    path
}

/// Four raw channel planes (alpha first) of a PSD raster record.
fn psd_planes(rgba: &[[u16; 4]]) -> Vec<(i16, Vec<u8>)> {
    [(-1_i16, 3_usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            let samples: Vec<u16> = rgba.iter().map(|p| p[component]).collect();
            (id, psd16::raw_plane(&samples))
        })
        .collect()
}

/// A 16-bit sRGB PSD: a canvas-sized content record (the pattern), a
/// pixel-less record above it, and an opaque stored composite.
fn psd_bytes((w, h): (u32, u32)) -> Vec<u8> {
    let content = psd16::Record::raster(
        "Content",
        [0, 0, h as i32, w as i32],
        psd_planes(&pattern(w, h, 6)),
    );
    let empty = psd16::Record::raster(
        "Empty",
        [0; 4],
        [-1_i16, 0, 1, 2]
            .into_iter()
            .map(|id| (id, vec![0, 0]))
            .collect(),
    );
    psd16::fixture(
        (w, h),
        &[content, empty],
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &opaque(w, h, 6),
        true,
        false,
    )
}

fn psd_fingerprint(path: &Path) -> String {
    let text = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(text.as_ptr(), &mut error) };
    assert!(!json.is_null(), "the PSD report: {}", take(error));
    let value: Value = serde_json::from_str(&take(json)).unwrap();
    value["source_fingerprint"].as_str().unwrap().to_string()
}

// ---------------------------------------------------------------- sites --

/// `encode_native` with the trailing `LIDS` ids (process-minted) zeroed.
fn save(doc: &PrecisionDocument, control: Option<&DrJobControl>) -> Result<Vec<u8>, String> {
    doc.encode_native(LIMIT, 0, control).map(|mut stream| {
        let start = stream.len().saturating_sub(8 * doc.layer_count());
        stream[start..].fill(0);
        stream
    })
}

fn parse(bytes: &[u8], control: Option<&DrJobControl>) -> Result<PrecisionDocument, String> {
    PrecisionDocument::parse_native(bytes, LIMIT, 0, control)
}

fn open(path: &Path, control: Option<&DrJobControl>) -> Result<PrecisionDocument, String> {
    PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, control)
}

fn open_psd(
    path: &Path,
    mode: i32,
    fingerprint: &str,
    control: Option<&DrJobControl>,
) -> Result<PrecisionDocument, String> {
    let text = CString::new(path.to_str().unwrap()).unwrap();
    let expected = CString::new(fingerprint).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_open_psd(
            text.as_ptr(),
            mode,
            expected.as_ptr(),
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if out.is_null() {
        return Err(take(error));
    }
    assert!(error.is_null());
    Ok(*unsafe { Box::from_raw(out) })
}

/// `dr_precision_doc_source_master16` of the first layer carrying one.
fn master16(doc: &PrecisionDocument) -> Result<PrecisionImage, String> {
    let mut last = "no layer".to_string();
    for layer in 0..doc.layer_count() {
        let mut error = ptr::null_mut();
        let out = unsafe { dr_precision_doc_source_master16(doc, layer, &mut error) };
        if !out.is_null() {
            return Ok(*unsafe { Box::from_raw(out) });
        }
        last = take(error);
    }
    Err(last)
}

/// `dr_precision_image_develop_preview` of the whole image: width, height
/// (u32 LE) and the RGBA8 bytes.
fn preview(
    image: &PrecisionImage,
    divisor: u32,
    clipping: u32,
    control: Option<&DrJobControl>,
) -> Result<Vec<u8>, String> {
    let (w, h) = image.dimensions();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_image_develop_preview(
            image,
            0,
            0,
            w,
            h,
            divisor,
            clipping,
            LIMIT,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut error,
        )
    };
    if result.is_null() {
        return Err(take(error));
    }
    assert!(error.is_null());
    let (width, height) = unsafe { (dr_image_width(result), dr_image_height(result)) };
    let mut out = [width.to_le_bytes(), height.to_le_bytes()].concat();
    out.extend_from_slice(unsafe {
        std::slice::from_raw_parts(
            dr_image_pixels_rgba(result),
            width as usize * height as usize * 4,
        )
    });
    unsafe { dr_image_free(result) };
    Ok(out)
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

// ---------------------------------------------------------------- cases --

/// The input a case starts from (built before any measurement).
enum Input {
    /// A `.rz` or RZCB stream.
    Bytes(Vec<u8>),
    /// A file in its own temporary directory, and a PSD's fingerprint.
    File {
        _directory: TempDir,
        path: PathBuf,
        fingerprint: String,
    },
    Rgba8(Vec<u8>, (u32, u32)),
    /// An RZCB stream and the document it is pasted into.
    Paste(Vec<u8>, PrecisionDocument),
    Doc(PrecisionDocument),
}

fn input(name: &str, (w, h): (u32, u32)) -> Input {
    let file = |make: &dyn Fn(&Path) -> PathBuf| {
        let directory = TempDir::new().unwrap();
        let path = make(directory.path());
        let fingerprint = if name.starts_with("psd16/") {
            psd_fingerprint(&path)
        } else {
            String::new()
        };
        Input::File {
            _directory: directory,
            path,
            fingerprint,
        }
    };
    match name {
        "rz16/reopen" | "render/u16" => Input::Bytes(native(&rz16_doc((w, h)))),
        "rz16/master" | "pipeline/rz16_master_preview" => Input::Bytes(native(&master_doc((w, h)))),
        "rzf/reopen" | "render/f32" => Input::Bytes(native(&rzf_doc((w, h)))),
        "png16/open" | "pipeline/png16_preview" => {
            file(&|d| codec_file(d, "open.png", (w, h), PrecisionFormat::Png))
        }
        "tiff16/open" => file(&|d| codec_file(d, "open.tiff", (w, h), PrecisionFormat::Tiff)),
        "psd16/layered" | "psd16/composite" => file(&|d| {
            let path = d.join("open.psd");
            std::fs::write(&path, psd_bytes((w, h))).unwrap();
            path
        }),
        "tiff32/open" => file(&|d| {
            let path = d.join("open32.tiff");
            std::fs::write(&path, float_tiff((w, h), &pattern32(w, h))).unwrap();
            path
        }),
        "drcb16/decode" => Input::Bytes(drcb(
            1,
            (w, h),
            IccProfile::srgb().bytes(),
            &le16(&pattern(w, h, 8)),
        )),
        "drcbf/decode" => Input::Bytes(drcb(
            2,
            (w, h),
            IccProfile::srgb().bytes(),
            &le32(&pattern32(w, h)),
        )),
        "rgba8/payload" => Input::Rgba8(pattern8(w, h), (w, h)),
        "drcb16/paste" => Input::Paste(
            drcb(
                1,
                (w, h),
                IccProfile::srgb().bytes(),
                &le16(&pattern(w, h, 8)),
            ),
            u16_doc((w, h), pattern(w, h, 3)),
        ),
        "narrow/u16_rgba8" => Input::Doc(masked_doc((w, h))),
        "narrow/f32_rgba8" => Input::Doc(float_doc((w, h), &pattern32(w, h))),
        other => panic!("unknown case {other}"),
    }
}

/// A case's answer and every intermediate the app holds, alive until
/// dropped. `docs`, `images`, `narrowed`, `payload` and `bytes` are hashed;
/// `kept` is held only.
#[derive(Default)]
struct Held {
    input: Option<Input>,
    kept: Vec<PrecisionDocument>,
    kept_images: Vec<PrecisionImage>,
    docs: Vec<PrecisionDocument>,
    images: Vec<PrecisionImage>,
    narrowed: Option<DrDocument>,
    payload: Option<ClipboardPayload>,
    bytes: Vec<u8>,
}

/// An image's dimensions and samples as bytes (u16 codes, f32 bits,
/// little-endian).
fn image_bytes(image: &PrecisionImage) -> Vec<u8> {
    let (w, h) = image.dimensions();
    let mut out: Vec<u8> = [w, h].iter().flat_map(|v| v.to_le_bytes()).collect();
    out.extend_from_slice(image.profile().bytes());
    match image.storage() {
        PrecisionStorage::Rgba16Unorm => out.extend(le16(&image.to_rgba16().unwrap())),
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

/// Every record's offset and raster, in postorder.
fn document_bytes(doc: &PrecisionDocument) -> Vec<u8> {
    let mut out = Vec::new();
    let (w, h) = doc.dimensions();
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    out.extend((doc.layer_count() as u32).to_le_bytes());
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

/// Every byte layer's offset, pixels and mask.
fn byte_document_bytes(doc: &DrDocument) -> Vec<u8> {
    let mut out = Vec::new();
    for layer in &doc.layers {
        out.extend(layer.offset.0.to_le_bytes());
        out.extend(layer.offset.1.to_le_bytes());
        let pixels = layer.pixels.to_contiguous().unwrap();
        out.extend(pixels.width().to_le_bytes());
        out.extend(pixels.height().to_le_bytes());
        out.extend(pixels.as_raw());
        if let Some(mask) = &layer.mask {
            out.extend(mask.width().to_le_bytes());
            out.extend(mask.height().to_le_bytes());
            out.extend(mask.as_raw());
        }
    }
    out
}

fn answer_bytes(held: &Held) -> Vec<u8> {
    let mut out = Vec::new();
    for doc in &held.docs {
        out.extend(document_bytes(doc));
    }
    for image in &held.images {
        out.extend(image_bytes(image));
    }
    if let Some(doc) = &held.narrowed {
        out.extend(byte_document_bytes(doc));
    }
    if let Some(payload) = &held.payload {
        out.extend(image_bytes(payload.image()));
        out.extend(payload.encode().unwrap());
    }
    out.extend_from_slice(&held.bytes);
    out
}

/// The case's CALL over its input.
fn call(name: &str, input: Input, rec: &mut Recorder) -> Result<Held, String> {
    let mut held = Held::default();
    match (name, &input) {
        ("rz16/reopen" | "rzf/reopen", Input::Bytes(stream)) => {
            let doc = rec.step(|c| parse(stream, c))?;
            let resaved = rec.step(|c| save(&doc, c))?;
            held.docs.push(doc);
            held.bytes = resaved;
        }
        ("rz16/master", Input::Bytes(stream)) => {
            let doc = rec.step(|c| parse(stream, c))?;
            let master = master16(&doc)?;
            let resaved = rec.step(|c| save(&doc, c))?;
            held.kept.push(doc);
            held.images.push(master);
            held.bytes = resaved;
        }
        ("render/u16" | "render/f32", Input::Bytes(stream)) => {
            let doc = rec.step(|c| parse(stream, c))?;
            let projection = rec.step(|c| doc.render(LIMIT, 0, c))?;
            held.kept.push(doc);
            held.images.push(projection);
        }
        ("pipeline/rz16_master_preview", Input::Bytes(stream)) => {
            let doc = rec.step(|c| parse(stream, c))?;
            let master = master16(&doc)?;
            held.bytes = rec.step(|c| preview(&master, 4, 0, c))?;
            held.kept.push(doc);
            held.kept_images.push(master);
        }
        ("png16/open" | "tiff16/open" | "tiff32/open", Input::File { path, .. }) => {
            let doc = rec.step(|c| open(path, c))?;
            held.docs.push(doc);
        }
        ("pipeline/png16_preview", Input::File { path, .. }) => {
            let doc = rec.step(|c| open(path, c))?;
            held.bytes = rec.step(|c| preview(doc.layer_source(0).unwrap(), 4, 0, c))?;
            held.kept.push(doc);
        }
        (
            "psd16/layered" | "psd16/composite",
            Input::File {
                path, fingerprint, ..
            },
        ) => {
            let mode = if name == "psd16/layered" {
                PSD_LAYERS
            } else {
                PSD_COMPOSITE
            };
            let doc = rec.step(|c| open_psd(path, mode, fingerprint, c))?;
            held.docs.push(doc);
        }
        ("drcb16/decode" | "drcbf/decode", Input::Bytes(stream)) => {
            held.payload = Some(ClipboardPayload::decode(stream, LIMIT, 0)?);
        }
        ("rgba8/payload", Input::Rgba8(pixels, (w, h))) => {
            let (w, h) = (*w, *h);
            held.payload = Some(ClipboardPayload::from_rgba8(
                pixels,
                w,
                h,
                IccProfile::srgb().bytes(),
                (0, 0),
                Resolution::default(),
                LIMIT,
                0,
            )?);
        }
        ("drcb16/paste", Input::Paste(stream, target)) => {
            let payload = ClipboardPayload::decode(stream, LIMIT, 0)?;
            let pasted =
                rec.step(|c| target.clipboard_paste(&payload, &paste_request(), LIMIT, 0, c))?;
            held.docs.push(pasted);
            held.payload = Some(payload);
        }
        ("narrow/u16_rgba8" | "narrow/f32_rgba8", Input::Doc(doc)) => {
            let narrowed =
                rec.step(|c| doc.narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, c))?;
            held.narrowed = Some(narrowed);
        }
        (other, _) => panic!("{other}: an unexpected input"),
    }
    held.input = Some(input);
    Ok(held)
}

fn outcome(outcome: &Result<Held, String>) -> Vec<u8> {
    match outcome {
        Ok(held) => answer_bytes(held),
        Err(e) => format!("err:{e}").into_bytes(),
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn record_decoders() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        for name in CASES {
            let input = input(name, (w, h));
            evict();
            let mut rec = Recorder::new(true);
            let out = call(name, input, &mut rec);
            println!(
                "decode {name} {w}x{h} {} checkpoints={} reports={}",
                sha256::sha256_hex(&outcome(&out)),
                rec.checkpoints,
                sha256::sha256_hex(rec.log.as_bytes())
            );
        }
    }
}

#[test]
fn no_view_decoders() {
    let _serial = serial();
    let mut measured = Vec::new();
    for name in CASES {
        let input = input(name, (W, H));
        evict();
        let held =
            call(name, input, &mut Recorder::new(false)).unwrap_or_else(|e| panic!("{name}: {e}"));
        // The two pipelines' first whole read (`develop_inspect.rs:136`,
        // 13B-3e-4c's) materializes the decoded source once: `w·h·8`.
        let expected = if name.starts_with("pipeline/") {
            let source = held
                .kept_images
                .first()
                .or_else(|| held.kept[0].layer_source(0))
                .unwrap();
            let (w, h) = source.dimensions();
            u64::from(w) * u64::from(h) * 8
        } else {
            0
        };
        let views = contiguous_view_bytes();
        println!("views {name} {W}x{H} held={views} expected={expected}");
        measured.push((name, views, expected));
        drop(held);
    }
    for (name, views, expected) in measured {
        assert_eq!(views, expected, "{name}: views held");
    }
}

fn assert_filled(image: &PrecisionImage, reference: u64, what: &str) {
    assert_eq!(
        image.retained_bytes(),
        reference,
        "{what}: a fresh add_layer_at plane's retained bytes"
    );
}

fn assert_dense(image: &PrecisionImage, what: &str) {
    let (w, h) = image.dimensions();
    let bpp = image.storage().bytes_per_pixel();
    assert_eq!(
        category(&report(&[image]), "precision_pixels"),
        u64::from(w) * u64::from(h) * bpp,
        "{what}: dense"
    );
}

#[test]
fn reopened_uniform_planes_are_filled() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let at = format!("{w}x{h}");

        let doc = parse(&native(&rz16_doc((w, h))), None).unwrap();
        let top = doc.layer_count() - 1;
        assert_filled(
            doc.layer_source(top).unwrap(),
            fresh_retained(&doc),
            &format!("{at} .doctored u16 add_layer_at"),
        );

        let doc = parse(&native(&rzf_doc((w, h))), None).unwrap();
        let top = doc.layer_count() - 1;
        assert_filled(
            doc.layer_source(top).unwrap(),
            fresh_retained(&doc),
            &format!("{at} .doctored f32 add_layer_at"),
        );

        let directory = TempDir::new().unwrap();
        let path = directory.path().join("uniform.psd");
        std::fs::write(&path, psd_bytes((w, h))).unwrap();
        let doc = open_psd(&path, PSD_LAYERS, &psd_fingerprint(&path), None).unwrap();
        assert_filled(
            doc.layer_source(1).unwrap(),
            fresh_retained(&doc),
            &format!("{at} PSD pixel-less record"),
        );

        let uniform = vec![[4097, 12289, 65534, 40000]; (w * h) as usize];
        let doc = parse(&native(&u16_doc((w, h), uniform.clone())), None).unwrap();
        assert_filled(
            doc.layer_source(0).unwrap(),
            fresh_retained(&doc),
            &format!("{at} .doctored uniform nonzero u16"),
        );

        let payload = ClipboardPayload::decode(
            &drcb(1, (w, h), IccProfile::srgb().bytes(), &le16(&uniform)),
            LIMIT,
            0,
        )
        .unwrap();
        let host = PrecisionDocument::from_image(
            payload.image().clone(),
            Metadata::default(),
            Resolution::default(),
        )
        .unwrap();
        assert_filled(
            payload.image(),
            fresh_retained(&host),
            &format!("{at} uniform RZCB"),
        );

        if (w, h) == (1, 1) {
            // A single sample cannot differ from itself.
            continue;
        }
        let mut last = uniform;
        *last.last_mut().unwrap() = [4097, 12289, 65534, 40001];
        let doc = parse(&native(&u16_doc((w, h), last.clone())), None).unwrap();
        assert_dense(
            doc.layer_source(0).unwrap(),
            &format!("{at} .doctored last sample differs"),
        );
        let payload = ClipboardPayload::decode(
            &drcb(1, (w, h), IccProfile::srgb().bytes(), &le16(&last)),
            LIMIT,
            0,
        )
        .unwrap();
        assert_dense(payload.image(), &format!("{at} RZCB last sample differs"));

        let mut signed = vec![[0.0_f32; 4]; (w * h) as usize];
        signed[(w * h / 2) as usize][1] = -0.0;
        let doc = parse(&native(&float_doc((w, h), &signed)), None).unwrap();
        assert_dense(
            doc.layer_source(0).unwrap(),
            &format!("{at} .doctored f32 −0.0"),
        );
        let payload = ClipboardPayload::decode(
            &drcb(2, (w, h), IccProfile::srgb().bytes(), &le32(&signed)),
            LIMIT,
            0,
        )
        .unwrap();
        assert_dense(payload.image(), &format!("{at} RZCB f32 −0.0"));
    }
}

fn bits32(samples: &[[f32; 4]]) -> Vec<[u32; 4]> {
    samples.iter().map(|p| p.map(f32::to_bits)).collect()
}

#[test]
fn decoded_samples_match_their_source() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let at = format!("{w}x{h}");

        // `.rz` u16: every reopened record holds the written samples.
        for written in [rz16_doc((w, h)), master_doc((w, h))] {
            let doc = parse(&native(&written), None).unwrap();
            assert_eq!(doc.layer_count(), written.layer_count());
            for i in 0..doc.layer_count() {
                if let Some(image) = written.layer_source(i) {
                    assert_eq!(
                        doc.layer_source(i).unwrap().to_rgba16().unwrap(),
                        image.to_rgba16().unwrap(),
                        "{at} .doctored layer {i}"
                    );
                }
            }
            if let Ok(master) = master16(&written) {
                assert_eq!(
                    master16(&doc).unwrap().to_rgba16().unwrap(),
                    master.to_rgba16().unwrap(),
                    "{at} .doctored master"
                );
            }
        }
        let doc = parse(&native(&u16_doc((w, h), pattern(w, h, 1))), None).unwrap();
        assert_eq!(
            doc.layer_source(0).unwrap().to_rgba16().unwrap(),
            pattern(w, h, 1),
            "{at} .doctored pattern"
        );
        let doc = parse(&native(&rz16_doc((w, h))), None).unwrap();
        let top = doc.layer_source(doc.layer_count() - 1).unwrap();
        assert_eq!(top.dimensions(), (w, h));
        assert!(
            top.to_rgba16().unwrap().iter().all(|p| *p == [0; 4]),
            "{at} .doctored empty layer"
        );

        // `.rz` f32.
        let written = rzf_doc((w, h));
        let doc = parse(&native(&written), None).unwrap();
        assert_eq!(
            bits32(&doc.layer_source(0).unwrap().to_rgba32().unwrap()),
            bits32(&pattern32(w, h)),
            "{at} .doctored f32"
        );
        let top = doc.layer_source(doc.layer_count() - 1).unwrap();
        assert!(
            top.to_rgba32()
                .unwrap()
                .iter()
                .all(|p| p.iter().all(|c| c.to_bits() == 0)),
            "{at} .doctored f32 empty layer"
        );

        // PNG16 / TIFF16 / TIFF32 open.
        let directory = TempDir::new().unwrap();
        for (name, format) in [
            ("open.png", PrecisionFormat::Png),
            ("open.tiff", PrecisionFormat::Tiff),
        ] {
            let path = codec_file(directory.path(), name, (w, h), format);
            let doc = open(&path, None).unwrap();
            assert_eq!(
                doc.layer_source(0).unwrap().to_rgba16().unwrap(),
                pattern(w, h, 4),
                "{at} {name}"
            );
        }
        let path = directory.path().join("open32.tiff");
        std::fs::write(&path, float_tiff((w, h), &pattern32(w, h))).unwrap();
        let doc = open(&path, None).unwrap();
        assert_eq!(
            bits32(&doc.layer_source(0).unwrap().to_rgba32().unwrap()),
            bits32(&pattern32(w, h)),
            "{at} TIFF32"
        );

        // PSD: the content record, the pixel-less record (a canvas of
        // zeros) and the opaque composite.
        let path = directory.path().join("open.psd");
        std::fs::write(&path, psd_bytes((w, h))).unwrap();
        let fingerprint = psd_fingerprint(&path);
        let doc = open_psd(&path, PSD_LAYERS, &fingerprint, None).unwrap();
        assert_eq!(
            doc.layer_source(0).unwrap().to_rgba16().unwrap(),
            pattern(w, h, 6),
            "{at} PSD content"
        );
        let empty = doc.layer_source(1).unwrap();
        assert_eq!(empty.dimensions(), (w, h), "{at} PSD pixel-less extent");
        assert_eq!(doc.layer(1).unwrap().offset(), (0, 0));
        assert!(
            empty.to_rgba16().unwrap().iter().all(|p| *p == [0; 4]),
            "{at} PSD pixel-less zeros"
        );
        let doc = open_psd(&path, PSD_COMPOSITE, &fingerprint, None).unwrap();
        assert_eq!(
            doc.layer_source(0).unwrap().to_rgba16().unwrap(),
            opaque(w, h, 6),
            "{at} PSD composite"
        );

        // RZCB, read little-endian here, and RGBA8 `v·257`.
        let stream = drcb(
            1,
            (w, h),
            IccProfile::srgb().bytes(),
            &le16(&pattern(w, h, 8)),
        );
        let payload = ClipboardPayload::decode(&stream, LIMIT, 0).unwrap();
        assert_eq!(
            payload.image().to_rgba16().unwrap(),
            drcb_samples16(&stream),
            "{at} RZCB u16"
        );
        let stream = drcb(
            2,
            (w, h),
            IccProfile::srgb().bytes(),
            &le32(&pattern32(w, h)),
        );
        let payload = ClipboardPayload::decode(&stream, LIMIT, 0).unwrap();
        assert_eq!(
            bits32(&payload.image().to_rgba32().unwrap()),
            bits32(&drcb_samples32(&stream)),
            "{at} RZCB f32"
        );
        let pixels = pattern8(w, h);
        let payload = ClipboardPayload::from_rgba8(
            &pixels,
            w,
            h,
            IccProfile::srgb().bytes(),
            (0, 0),
            Resolution::default(),
            LIMIT,
            0,
        )
        .unwrap();
        let widened: Vec<[u16; 4]> = pixels
            .chunks_exact(4)
            .map(|p| std::array::from_fn(|c| u16::from(p[c]) * 257))
            .collect();
        assert_eq!(payload.image().to_rgba16().unwrap(), widened, "{at} RGBA8");
    }
}

#[test]
fn resave_is_byte_identical() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let uniform = vec![[4097, 12289, 65534, 40000]; (w * h) as usize];
        for (label, doc) in [
            ("rz16", rz16_doc((w, h))),
            ("master", master_doc((w, h))),
            ("rzf", rzf_doc((w, h))),
            ("uniform", u16_doc((w, h), uniform)),
        ] {
            let bytes = native(&doc);
            let reopened = parse(&bytes, None).unwrap();
            assert!(
                native(&reopened) == bytes,
                "{w}x{h} {label}: the resave is byte-identical"
            );
        }
    }
}

#[test]
fn peak_decoders() {
    let _serial = serial();
    let size = (W, H);
    for name in CASES {
        let input = input(name, size);
        evict();
        let baseline = LIVE.load(Ordering::Relaxed);
        PEAK.store(baseline, Ordering::Relaxed);
        let held =
            call(name, input, &mut Recorder::new(false)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
        let live = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
        drop(held);
        println!("peak {name} {}x{} peak={peak} held={live}", size.0, size.1);
        assert!(peak >= live, "{name}: peak {peak} below held {live}");
    }
}
