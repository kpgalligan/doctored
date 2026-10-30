//! Black-box suite for 13B-3e-4c-2a: no f32 constructor seeds the view cache
//! (`from_rgba32` / `from_rgba32_extended` tile unseeded and the cache has
//! no f32 view type) — with every answer, refusal, checkpoint count and
//! report log unchanged.
//!
//! Oracles are independent of the implementation: a constructor's samples
//! and runs are the input buffer itself (f32 bits), and the refusals
//! are the two sentences written out here.
//!
//! `record_f32_constructors` prints one line per case and fixture —
//! `f32 <case> <WxH> <sha256> checkpoints=<n> reports=<sha256>` (the hash of
//! `err:<message>` or the answer's bytes; every call that takes a job
//! control runs under a recording `DrJobControl`) — so the same file run
//! against HEAD and the tree diffs byte-identical answers and
//! checkpoint/report sequences. `no_view_f32_constructors` prints
//! `views <case> 600x520 held=<bytes> expected=<bytes>` and
//! `peak_f32_constructors` prints `peak <case> 600x520 peak=<bytes>
//! held=<bytes>` (live heap bytes above the case's baseline, from the
//! tracking allocator below). It uses only APIs present at `31981c3`.
//!
//! `no_view_f32_constructors` fails at `31981c3` by design (there both f32
//! constructors seed the view cache with the adopted buffer); the other
//! tests pass there. The view cache is process-wide, so every test holds
//! `serial()`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use doctored_core::contiguous_views::contiguous_view_bytes;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};

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

const NORMALIZED: &str = "precision float components must be finite and normalized to [0,1]";
const EXTENDED: &str = "32-bit float samples must be finite within ±65504 with alpha in [0,1]";

const CASES: [&str; 7] = [
    "ctor/from_rgba32",
    "ctor/from_rgba32_extended",
    "ctor/from_rgba32_refused",
    "ctor/from_rgba32_extended_refused",
    "rebuilt/f32",
    "rect/f32",
    "convert/f32_to_u16",
];

const VIEW_CASES: [&str; 3] = [
    "ctor/from_rgba32",
    "ctor/from_rgba32_extended",
    "ctor/from_rgba16",
];

const PEAK_CASES: [&str; 4] = [
    "ctor/from_rgba32",
    "ctor/from_rgba32_extended",
    "rebuilt/f32",
    "ctor/from_rgba16",
];

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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
            "f32 {label} {w}x{h} {} checkpoints={} reports={}",
            sha256::sha256_hex(&self.answers),
            self.checkpoints,
            sha256::sha256_hex(self.log.as_bytes())
        );
    }
}

/// Normalized samples in [0, 1] (alpha 0, 1 and fractions), row-major.
fn normalized(width: u32, height: u32, seed: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let v = ((x * 37 + y * 101 + seed * 13) % 1000) as f32 / 999.0;
            let a = match (x + 2 * y) % 11 {
                0 => 0.0,
                1 | 2 => 1.0,
                k => k as f32 / 11.0,
            };
            [v, 1.0 - v, ((x ^ y) % 97) as f32 / 96.0, a]
        })
        .collect()
}

/// Extended-range samples (negative and above-white RGB) with alpha 0, 1 and
/// fractions, row-major.
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

fn pattern16(width: u32, height: u32) -> Vec<[u16; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            [
                ((x * 2711 + y * 911) % 65536) as u16,
                ((x * 523 + y * 3301) % 65536) as u16,
                (((x ^ y) * 1999) % 65536) as u16,
                (9000 + (x * 3001 + y * 1777) % 56536) as u16,
            ]
        })
        .collect()
}

/// f32 bits little-endian.
fn f32_bytes(samples: &[[f32; 4]]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|p| p.iter().flat_map(|c| c.to_bits().to_le_bytes()))
        .collect()
}

/// An image's dimensions and samples as bytes (u16 codes, f32 bits).
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
        PrecisionStorage::Rgba32Float => out.extend(f32_bytes(&image.to_rgba32().unwrap())),
    }
    out
}

fn outcome_bytes(outcome: Result<Vec<u8>, String>) -> Vec<u8> {
    match outcome {
        Ok(bytes) => bytes,
        Err(e) => format!("err:{e}").into_bytes(),
    }
}

fn from_rgba32(
    (w, h): (u32, u32),
    pixels: Vec<[f32; 4]>,
    control: Option<&DrJobControl>,
) -> Result<PrecisionImage, String> {
    PrecisionImage::from_rgba32(w, h, pixels, IccProfile::srgb(), control)
}

fn from_rgba32_extended(
    (w, h): (u32, u32),
    pixels: Vec<[f32; 4]>,
    control: Option<&DrJobControl>,
) -> Result<PrecisionImage, String> {
    PrecisionImage::from_rgba32_extended(w, h, pixels, IccProfile::srgb(), control)
}

/// The last ≤3×2 corner of a `w`×`h` plane.
fn corner((w, h): (u32, u32)) -> (u32, u32, u32, u32) {
    let (cw, ch) = (w.min(3), h.min(2));
    (w - cw, h - ch, cw, ch)
}

/// Runs one recorded case; answers the line.
fn record_case(name: &str, size: (u32, u32)) -> Line {
    let mut line = Line::default();
    let answer: Result<Vec<u8>, String> = match name {
        "ctor/from_rgba32" => {
            let pixels = normalized(size.0, size.1, 1);
            line.add(recorded(|c| from_rgba32(size, pixels, Some(c))))
                .map(|image| image_bytes(&image))
        }
        "ctor/from_rgba32_extended" => {
            let pixels = extended(size.0, size.1, 2);
            line.add(recorded(|c| from_rgba32_extended(size, pixels, Some(c))))
                .map(|image| image_bytes(&image))
        }
        "ctor/from_rgba32_refused" => {
            let mut pixels = normalized(size.0, size.1, 3);
            pixels.last_mut().unwrap()[3] = f32::NAN;
            line.add(recorded(|c| from_rgba32(size, pixels, Some(c))))
                .map(|image| image_bytes(&image))
        }
        "ctor/from_rgba32_extended_refused" => {
            let mut pixels = extended(size.0, size.1, 4);
            pixels.last_mut().unwrap()[0] = 70000.0;
            line.add(recorded(|c| from_rgba32_extended(size, pixels, Some(c))))
                .map(|image| image_bytes(&image))
        }
        "rebuilt/f32" => {
            let base = from_rgba32_extended(size, extended(size.0, size.1, 5), None).unwrap();
            let mut pixels = extended(size.0, size.1, 5);
            pixels.last_mut().unwrap()[1] = 3.5;
            base.rebuilt_rgba32_extended(size.0, size.1, pixels, IccProfile::srgb())
                .map(|image| {
                    let mut bytes = image_bytes(&image);
                    bytes.extend((image.shared_tile_count(&base) as u64).to_le_bytes());
                    bytes
                })
        }
        "rect/f32" => {
            let base = from_rgba32_extended(size, extended(size.0, size.1, 6), None).unwrap();
            let (x, y, w, h) = corner(size);
            let samples: Vec<[f32; 4]> = (0..w * h)
                .map(|i| [i as f32 * 0.5 - 1.0, 2.25, -0.125, 0.5])
                .collect();
            base.with_rect_rgba32(x, y, w, h, &samples)
                .map(|next| match next {
                    Some(image) => image_bytes(&image),
                    None => b"none".to_vec(),
                })
        }
        "convert/f32_to_u16" => {
            let base = from_rgba32_extended(size, extended(size.0, size.1, 7), None).unwrap();
            line.add(recorded(|c| {
                base.convert(PrecisionStorage::Rgba16Unorm, LIMIT, 0, Some(c))
            }))
            .map(|image| image_bytes(&image))
        }
        other => panic!("{other}: an unknown case"),
    };
    line.answers = outcome_bytes(answer);
    line
}

// ------------------------------------------------------------------ tests --

#[test]
fn record_f32_constructors() {
    let _serial = serial();
    for size in FIXTURES {
        for name in CASES {
            evict();
            record_case(name, size).print(name, size);
        }
    }
}

#[test]
fn f32_samples_match_their_source() {
    let _serial = serial();
    for size in FIXTURES {
        let (w, h) = size;
        for (label, input, extended_range) in [
            ("from_rgba32", normalized(w, h, 1), false),
            ("from_rgba32_extended", extended(w, h, 2), true),
        ] {
            let expected = f32_bytes(&input);
            let image = if extended_range {
                from_rgba32_extended(size, input.clone(), None)
            } else {
                from_rgba32(size, input.clone(), None)
            }
            .unwrap_or_else(|e| panic!("{label} {w}x{h}: {e}"));
            assert_eq!(image.storage(), PrecisionStorage::Rgba32Float);
            assert_eq!(
                f32_bytes(&image.to_rgba32().unwrap()),
                expected,
                "{label} {w}x{h}: to_rgba32"
            );
            let walked: Vec<[f32; 4]> = image.runs_rgba32().unwrap().flatten().copied().collect();
            assert_eq!(f32_bytes(&walked), expected, "{label} {w}x{h}: runs_rgba32");
        }

        let mut refused = normalized(w, h, 3);
        refused.last_mut().unwrap()[3] = f32::NAN;
        assert_eq!(
            from_rgba32(size, refused, None).err().as_deref(),
            Some(NORMALIZED),
            "{w}x{h}: from_rgba32 refuses a NaN"
        );
        let mut refused = extended(w, h, 4);
        refused.last_mut().unwrap()[0] = 70000.0;
        assert_eq!(
            from_rgba32_extended(size, refused, None).err().as_deref(),
            Some(EXTENDED),
            "{w}x{h}: from_rgba32_extended refuses 70000"
        );
    }
}

/// Builds the held value of a view / peak case at `size`.
fn held_case(name: &str, size: (u32, u32)) -> Box<dyn std::any::Any> {
    let (w, h) = size;
    match name {
        "ctor/from_rgba32" => Box::new(from_rgba32(size, normalized(w, h, 1), None).unwrap()),
        "ctor/from_rgba32_extended" => {
            Box::new(from_rgba32_extended(size, extended(w, h, 2), None).unwrap())
        }
        "ctor/from_rgba16" => Box::new(
            PrecisionImage::from_rgba16(w, h, pattern16(w, h), IccProfile::srgb(), None).unwrap(),
        ),
        other => panic!("{other}: an unknown case"),
    }
}

#[test]
fn no_view_f32_constructors() {
    let _serial = serial();
    let mut failures = Vec::new();
    for name in VIEW_CASES {
        evict();
        let held = held_case(name, (W, H));
        let bytes = contiguous_view_bytes();
        let expected = if name == "ctor/from_rgba16" {
            u64::from(W) * u64::from(H) * 8
        } else {
            0
        };
        println!("views {name} {W}x{H} held={bytes} expected={expected}");
        if bytes != expected {
            failures.push(format!("{name}: {bytes} view bytes, expected {expected}"));
        }
        drop(held);
    }
    evict();
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn peak_f32_constructors() {
    let _serial = serial();
    let size = (W, H);
    for name in PEAK_CASES {
        evict();
        let base = (name == "rebuilt/f32")
            .then(|| from_rgba32_extended(size, extended(W, H, 5), None).unwrap());
        let baseline = LIVE.load(Ordering::Relaxed);
        PEAK.store(baseline, Ordering::Relaxed);
        let held: Box<dyn std::any::Any> = match &base {
            Some(base) => {
                let mut pixels = extended(W, H, 5);
                pixels.last_mut().unwrap()[1] = 3.5;
                Box::new(
                    base.rebuilt_rgba32_extended(W, H, pixels, IccProfile::srgb())
                        .unwrap(),
                )
            }
            None => held_case(name, size),
        };
        let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
        let live = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
        drop(held);
        drop(base);
        println!("peak {name} {W}x{H} peak={peak} held={live}");
        assert!(peak >= live, "{name}: peak {peak} below held {live}");
    }
    evict();
}
