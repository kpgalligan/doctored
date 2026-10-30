//! Black-box suite for 13B-3e-4a: the two host intakes
//! (`dr_precision_doc_from_rgba16`, `dr_precision_doc_from_host_pixels`) and
//! the RAW develop output are unseeded, the RAW source geometry reads its
//! source through ONE transient copy, and the FFI lend pins a PRIVATE copy
//! that never reads or writes the view cache — with every answer, refusal,
//! checkpoint count and report log unchanged.
//!
//! Oracles are independent of the implementation: intake samples are the
//! host buffer itself; an `argb16_be` buffer is decoded big-endian HERE and
//! EXIF orientation 6 is the permutation written HERE.
//!
//! `record_host_intake` prints one line per case and fixture —
//! `intake <case> <WxH> <sha256> checkpoints=<n> reports=<sha256>` (the hash
//! of `null`, `err:<message>` or the answer's bytes; every call that takes a
//! job control runs under a recording `DrJobControl`, a multi-step case sums
//! its steps' checkpoints and concatenates their logs) — so the same file
//! run against HEAD and the tree diffs byte-identical answers and
//! checkpoint/report sequences. `peak_host_intake` prints
//! `peak <case> <WxH> peak=<bytes> held=<bytes>` (live heap bytes above the
//! case's baseline, from the tracking allocator below). It uses only APIs
//! present at `6b97c48`.
//!
//! `no_view_host_intake` fails at `6b97c48` by design (there the intakes and
//! the develop output seed the view cache, the geometry binds its source's
//! view and the lend pins a cached view); the other tests pass there. The
//! view cache is process-wide, so every test holds `serial()`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_develop_inspect::dr_precision_image_develop_preview;
use doctored_core::ffi_host_pixels::dr_precision_doc_from_host_pixels;
use doctored_core::ffi_precision::dr_precision_image_pixels_rgba16;
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::raw_local_develop::develop;
use serde_json::{json, Value};

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
/// Host rows carry 16 padding bytes.
const PADDING: usize = 16;

const CASES: [&str; 12] = [
    "intake/from_rgba16",
    "intake/host_rgba16_le",
    "intake/host_argb16_be_orient6",
    "develop/intake_vignette",
    "geometry/intake_exif6",
    "geometry/intake_lens_bilinear",
    "geometry/intake_identity",
    "pipeline/intake_develop_lens",
    "pipeline/intake_preview",
    "save/intake_native",
    "save/developed_native",
    "lend/intake_u16",
];

const PEAK_CASES: [&str; 9] = [
    "intake/from_rgba16",
    "intake/host_rgba16_le",
    "pipeline/intake_develop",
    "pipeline/intake_exif6",
    "pipeline/intake_lens_bilinear",
    "pipeline/intake_develop_lens",
    "pipeline/intake_preview",
    "lend/intake_u16",
    "lend/seeded_u16",
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

fn report(images: &[&PrecisionImage]) -> Value {
    let handles: Vec<*const PrecisionImage> =
        images.iter().map(|i| *i as *const PrecisionImage).collect();
    resource_report::report(&[], &[], &[], &handles, 0, 0).expect("a resource report")
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

/// `samples` packed as host rows of `w·8 + PADDING` bytes: `rgba16_le`, or
/// `argb16_be` (A, R, G, B, each big-endian).
fn host_bytes(samples: &[[u16; 4]], (w, h): (u32, u32), big_endian: bool) -> Vec<u8> {
    let row = w as usize * 8 + PADDING;
    let mut out = vec![0xa5; row * h as usize];
    for (i, p) in samples.iter().enumerate() {
        let at = (i / w as usize) * row + (i % w as usize) * 8;
        let words = if big_endian {
            [p[3], p[0], p[1], p[2]].map(u16::to_be_bytes)
        } else {
            p.map(u16::to_le_bytes)
        };
        out[at..at + 8].copy_from_slice(words.as_flattened());
    }
    out
}

fn vignette_text((w, h): (u32, u32)) -> String {
    json!({"stops":-1.0,"center":[f64::from(w) / 2.0, f64::from(h) / 2.0],"falloff":1.0,
        "reference_size":null,"reference_from_source":null})
    .to_string()
}

const EXIF6: &str = r#"{"version":1,"kind":"exif","orientation":6}"#;
const IDENTITY: &str = r#"{"version":1,"kind":"exif","orientation":1}"#;

fn lens_text() -> String {
    json!({"version":1,"kind":"lens","profile_id":"reads","center":[0.5,0.5],
        "radial":[0.02,0.0,0.0],"red_scale":0.997,"blue_scale":1.003,"filter":"bilinear"})
    .to_string()
}

// ---------------------------------------------------------------- sites --

/// `dr_precision_doc_from_rgba16` over `samples`.
fn intake_rgba16(
    samples: &[[u16; 4]],
    (w, h): (u32, u32),
    control: Option<&DrJobControl>,
) -> Result<PrecisionDocument, String> {
    let profile = IccProfile::srgb();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_from_rgba16(
            samples.as_ptr().cast(),
            samples.len() * 4,
            w,
            h,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            ptr::null(),
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

/// `dr_precision_doc_from_host_pixels` over host rows of `w·8 + PADDING`.
fn intake_host(
    bytes: &[u8],
    (w, h): (u32, u32),
    layout: &str,
    orientation: u32,
    control: Option<&DrJobControl>,
) -> Result<PrecisionDocument, String> {
    let profile = IccProfile::srgb();
    let name = CString::new(layout).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_from_host_pixels(
            bytes.as_ptr(),
            bytes.len(),
            w,
            h,
            w as usize * 8 + PADDING,
            name.as_ptr(),
            orientation,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
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

fn run_vignette(
    base: &PrecisionDocument,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    develop(
        base,
        &[],
        &[],
        "[]",
        Some(&vignette_text(base.dimensions())),
        LIMIT,
        0,
        control,
    )
}

/// `dr_precision_image_develop_preview`: width, height (u32 LE) and the
/// RGBA8 bytes.
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

/// `encode_native` with the trailing `LIDS` ids (process-minted) zeroed.
fn save(doc: &PrecisionDocument, control: Option<&DrJobControl>) -> Result<Vec<u8>, String> {
    doc.encode_native(LIMIT, 0, control).map(|mut stream| {
        let start = stream.len().saturating_sub(8 * doc.layer_count());
        stream[start..].fill(0);
        stream
    })
}

// ---------------------------------------------------------------- cases --

/// The host buffer a case starts from (built before any measurement).
enum Host {
    Samples(Vec<[u16; 4]>),
    Bytes(Vec<u8>),
}

fn host(name: &str, (w, h): (u32, u32)) -> Host {
    match name {
        "intake/host_rgba16_le" => Host::Bytes(host_bytes(&pattern(w, h, 1), (w, h), false)),
        "intake/host_argb16_be_orient6" => Host::Bytes(host_bytes(&pattern(w, h, 1), (w, h), true)),
        _ => Host::Samples(pattern(w, h, 1)),
    }
}

/// A case's answer and every intermediate the app holds, alive until
/// dropped. `lend` is the lent pointer (into `docs[0]` or `image`) and its
/// byte count; `host` keeps the case's host buffer alive (it is part of a
/// peak line's baseline).
#[derive(Default)]
struct Held {
    host: Option<Host>,
    docs: Vec<Option<PrecisionDocument>>,
    image: Option<PrecisionImage>,
    bytes: Option<Vec<u8>>,
    lend: Option<(*const u8, usize)>,
}

impl Held {
    fn last(&self) -> Option<&PrecisionDocument> {
        self.docs.last().and_then(Option::as_ref)
    }
    /// The image a lend or a report reads.
    fn lent_image(&self) -> &PrecisionImage {
        self.image
            .as_ref()
            .or_else(|| self.docs[0].as_ref().and_then(|d| d.layer_source(0)))
            .unwrap()
    }
}

fn doc_bytes(doc: &PrecisionDocument, out: &mut Vec<u8>) {
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
            let (w, h) = image.dimensions();
            out.extend(w.to_le_bytes());
            out.extend(h.to_le_bytes());
            out.extend_from_slice(image.profile().bytes());
            for p in image.to_rgba16().unwrap() {
                for v in p {
                    out.extend(v.to_le_bytes());
                }
            }
        }
    }
}

fn answer_bytes(held: &Held) -> Vec<u8> {
    let mut out = Vec::new();
    for doc in &held.docs {
        match doc {
            Some(doc) => doc_bytes(doc, &mut out),
            None => out.extend_from_slice(b"null"),
        }
    }
    if let Some(bytes) = &held.bytes {
        out.extend_from_slice(bytes);
    }
    if let Some((p, n)) = held.lend {
        out.extend_from_slice(unsafe { std::slice::from_raw_parts(p, n) });
    }
    out
}

/// The case's CALL over its host buffer: the intake, then the case's steps.
fn call(name: &str, host: Host, size: (u32, u32), rec: &mut Recorder) -> Result<Held, String> {
    let mut held = Held::default();
    match (name, host) {
        ("intake/host_rgba16_le", Host::Bytes(bytes)) => {
            let doc = rec.step(|c| intake_host(&bytes, size, "rgba16_le", 1, c))?;
            held.docs.push(Some(doc));
            held.host = Some(Host::Bytes(bytes));
            return Ok(held);
        }
        ("intake/host_argb16_be_orient6", Host::Bytes(bytes)) => {
            let doc = rec.step(|c| intake_host(&bytes, size, "argb16_be", 6, c))?;
            held.docs.push(Some(doc));
            held.host = Some(Host::Bytes(bytes));
            return Ok(held);
        }
        ("lend/seeded_u16", Host::Samples(pixels)) => {
            let image =
                PrecisionImage::from_rgba16(size.0, size.1, pixels, IccProfile::srgb(), None)?;
            let lent = unsafe { dr_precision_image_pixels_rgba16(&image) };
            if lent.is_null() {
                return Err("the u16 lend answered NULL".into());
            }
            held.lend = Some((lent.cast(), size.0 as usize * size.1 as usize * 8));
            held.image = Some(image);
            return Ok(held);
        }
        (_, Host::Samples(samples)) => {
            let base = rec.step(|c| intake_rgba16(&samples, size, c))?;
            held.docs.push(Some(base));
            held.host = Some(Host::Samples(samples));
        }
        (other, _) => panic!("{other}: an unexpected host buffer"),
    }
    let count = size.0 as usize * size.1 as usize;
    let base = held.docs[0].as_ref().unwrap();
    match name {
        "intake/from_rgba16" => {}
        "develop/intake_vignette" | "pipeline/intake_develop" => {
            let developed = rec.step(|c| run_vignette(base, c))?;
            held.docs.push(developed);
        }
        "geometry/intake_exif6" | "pipeline/intake_exif6" => {
            let turned = rec.step(|c| base.raw_source_geometry(EXIF6, LIMIT, 0, c))?;
            held.docs.push(turned);
        }
        "geometry/intake_lens_bilinear" | "pipeline/intake_lens_bilinear" => {
            let lens = lens_text();
            let corrected = rec.step(|c| base.raw_source_geometry(&lens, LIMIT, 0, c))?;
            held.docs.push(corrected);
        }
        "geometry/intake_identity" => {
            let same = rec.step(|c| base.raw_source_geometry(IDENTITY, LIMIT, 0, c))?;
            held.docs.push(same);
        }
        "pipeline/intake_develop_lens" => {
            let developed = rec.step(|c| run_vignette(base, c))?;
            let lens = lens_text();
            let corrected = {
                let input = developed.as_ref().unwrap_or(base);
                rec.step(|c| input.raw_source_geometry(&lens, LIMIT, 0, c))?
            };
            held.docs.push(developed);
            held.docs.push(corrected);
        }
        "pipeline/intake_preview" => {
            let image = base.layer_source(0).unwrap();
            let shown = rec.step(|c| preview(image, 3, 3, c))?;
            held.bytes = Some(shown);
        }
        "save/intake_native" => {
            let stream = rec.step(|c| save(base, c))?;
            held.bytes = Some(stream);
        }
        "save/developed_native" => {
            let developed = rec.step(|c| run_vignette(base, c))?;
            let stream = {
                let input = developed.as_ref().unwrap_or(base);
                rec.step(|c| save(input, c))?
            };
            held.docs.push(developed);
            held.bytes = Some(stream);
        }
        "lend/intake_u16" => {
            let image = base.layer_source(0).unwrap();
            let lent = unsafe { dr_precision_image_pixels_rgba16(image) };
            if lent.is_null() {
                return Err("the u16 lend answered NULL".into());
            }
            held.lend = Some((lent.cast(), count * 8));
        }
        other => panic!("unknown case {other}"),
    }
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
fn record_host_intake() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        for name in CASES {
            let host = host(name, (w, h));
            evict();
            let mut rec = Recorder::new(true);
            let out = call(name, host, (w, h), &mut rec);
            println!(
                "intake {name} {w}x{h} {} checkpoints={} reports={}",
                sha256::sha256_hex(&outcome(&out)),
                rec.checkpoints,
                sha256::sha256_hex(rec.log.as_bytes())
            );
        }
    }
}

#[test]
fn no_view_host_intake() {
    let _serial = serial();
    let plane = u64::from(W) * u64::from(H) * 8;
    for name in CASES {
        let host = host(name, (W, H));
        evict();
        let held = call(name, host, (W, H), &mut Recorder::new(false))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let seed = |doc: Option<&PrecisionDocument>| {
            doc.map_or(0, |d| {
                let (dw, dh) = d.dimensions();
                u64::from(dw) * u64::from(dh) * 8
            })
        };
        // The views kept since 13B-3e-4a: a geometry output's seed
        // (`raw_source_geometry.rs:470`) and the preview SOURCE's view
        // (`develop_inspect.rs:136`), both 13B-3e-4c's.
        let expected = match name.split('/').next().unwrap() {
            "intake" | "develop" | "save" | "lend" => 0,
            "geometry" => seed(held.last()),
            "pipeline" if name == "pipeline/intake_preview" => plane,
            "pipeline" => seed(held.last()),
            _ => unreachable!(),
        };
        if name == "geometry/intake_identity" {
            assert!(held.last().is_none(), "{name}: the identity answers None");
        }
        if name.starts_with("geometry/") && name != "geometry/intake_identity" {
            assert!(held.last().is_some(), "{name}: an output");
        }
        assert_eq!(contiguous_view_bytes(), expected, "{name}: views held");
        if name.starts_with("lend/") {
            let image = held.lent_image();
            let (w, h) = image.dimensions();
            let measured = report(&[image]);
            // Only a 16-bit plane lends: 8 bytes per RGBA16 pixel.
            assert_eq!(
                measured["categories"]["precision_views"]["unique_bytes"].as_u64(),
                Some(u64::from(w) * u64::from(h) * 8),
                "{name}: the lend is retained"
            );
        }
        drop(held);
    }
}

/// Destination `(x, y)` of a `w`×`h` stored image under EXIF orientation 6
/// (rotate 90° clockwise to display) reads stored `(y, h − 1 − x)`.
fn exif6_source((_w, h): (u32, u32), x: u32, y: u32) -> (u32, u32) {
    (y, h - 1 - x)
}

#[test]
fn intake_samples_match_the_host_buffer() {
    let _serial = serial();
    for (w, h) in FIXTURES {
        let expected = pattern(w, h, 1);
        let doc = intake_rgba16(&expected, (w, h), None).unwrap();
        assert_eq!(
            doc.layer_source(0).unwrap().to_rgba16().unwrap(),
            expected,
            "{w}x{h} from_rgba16"
        );
        let le = host_bytes(&expected, (w, h), false);
        let doc = intake_host(&le, (w, h), "rgba16_le", 1, None).unwrap();
        assert_eq!(
            doc.layer_source(0).unwrap().to_rgba16().unwrap(),
            expected,
            "{w}x{h} rgba16_le"
        );

        let be = host_bytes(&expected, (w, h), true);
        let row = w as usize * 8 + PADDING;
        let decoded: Vec<[u16; 4]> = (0..(w * h) as usize)
            .map(|i| {
                let at = (i / w as usize) * row + (i % w as usize) * 8;
                let word = |k: usize| u16::from_be_bytes([be[at + 2 * k], be[at + 2 * k + 1]]);
                [word(1), word(2), word(3), word(0)]
            })
            .collect();
        assert_eq!(decoded, expected, "{w}x{h} the big-endian decode");
        let doc = intake_host(&be, (w, h), "argb16_be", 6, None).unwrap();
        let image = doc.layer_source(0).unwrap();
        let (dw, dh) = image.dimensions();
        assert_eq!((dw, dh), (h, w), "{w}x{h} orientation 6 turns");
        let output = image.to_rgba16().unwrap();
        for y in 0..dh {
            for x in 0..dw {
                let (sx, sy) = exif6_source((w, h), x, y);
                assert_eq!(
                    output[(y * dw + x) as usize],
                    decoded[(sy * w + sx) as usize],
                    "{w}x{h} orientation 6 ({x},{y})"
                );
            }
        }

        evict();
        let doc = intake_rgba16(&expected, (w, h), None).unwrap();
        let lent = unsafe { dr_precision_image_pixels_rgba16(doc.layer_source(0).unwrap()) };
        assert!(!lent.is_null());
        let lent = unsafe { std::slice::from_raw_parts(lent, (w * h * 4) as usize) };
        assert_eq!(lent, expected.as_flattened(), "{w}x{h} the lent bytes");
    }
}

#[test]
fn peak_host_intake() {
    let _serial = serial();
    let size = (W, H);
    for name in PEAK_CASES {
        evict();
        let host = host(name, size);
        let baseline = LIVE.load(Ordering::Relaxed);
        PEAK.store(baseline, Ordering::Relaxed);
        let held = call(name, host, size, &mut Recorder::new(false))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
        let live = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
        drop(held);
        println!("peak {name} {}x{} peak={peak} held={live}", size.0, size.1);
        assert!(peak >= live, "{name}: peak {peak} below held {live}");
    }
}
