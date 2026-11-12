//! Save and Open take a job control: the byte open (`dr_doc_open`), the
//! native, flat and PSD saves, the byte PSD open, the typed PSD save,
//! `export16` and `save_projection16`. Black-box through the FFI.
//!
//! Oracles:
//! - A pre-cancelled control answers exactly `operation cancelled` and
//!   returns NULL or false; a save leaves an existing destination
//!   byte-identical and its folder holding nothing else. A control already
//!   used answers `job control has already been used`.
//! - The count. Every save writes its file, and every open reads it, through
//!   `cancel_io::Polled`: one checkpoint per `POLL_BYTES` (65,536, restated
//!   here) moved. So over a seeded-noise fixture whose file is at least four
//!   intervals long, a call's checkpoints beyond its 2×2 twin's are at least
//!   `len / POLL_BYTES - 1`, where a call with no polling had none; a flat or
//!   native open also decodes through the same adapter, so it polls at least
//!   twice that. The last checkpoint of every save is its check before the
//!   rename, so the one before it is the write's last poll: a worker PARKED
//!   there is cancelled inside the write, and the destination keeps its
//!   previous bytes. An open parked at its third checkpoint is inside the
//!   file's read.
//! - A controlled save writes the uncontrolled one's bytes.

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len, dr_doc_save_image};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_from_image, dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_precision::{
    dr_precision_doc_export16, dr_precision_doc_free, dr_precision_doc_render,
    dr_precision_image_free,
};
use doctored_core::ffi_precision_export::dr_precision_doc_save_projection16;
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_save_psd, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::common;
use common::probe::{cancelled_parked_at, parked_line};
use common::{cpath, take_err_string};

/// `cancel_io::POLL_BYTES`, restated.
const POLL_BYTES: u64 = 65_536;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const PROFILE_SRGB: i32 = 0;
const FMT_PNG: i32 = 0;
const FMT_JPEG: i32 = 1;
const FMT_TIFF: i32 = 2;
const FMT_BMP: i32 = 3;
const KEEP: &[u8] = b"the previous file";

/// Seeded noise (xorshift64*), `len` bytes.
fn noise(len: usize, mut state: u64) -> Vec<u8> {
    (0..len)
        .map(|_| {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
        })
        .collect()
}

/// A byte document and an image of its opaque noise, as addresses so a
/// worker closure can carry them.
#[derive(Clone, Copy)]
struct Bytes {
    doc: usize,
    image: usize,
}

impl Bytes {
    fn new(w: u32, h: u32) -> Self {
        let mut pixels = noise((w * h * 4) as usize, u64::from(w) * 7919 + u64::from(h));
        for alpha in pixels.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
        let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
        assert!(!image.is_null());
        let doc = unsafe { dr_doc_from_image(image) };
        assert!(!doc.is_null());
        Bytes {
            doc: doc as usize,
            image: image as usize,
        }
    }

    fn doc(self) -> *const DrDocument {
        self.doc as *const DrDocument
    }

    fn image(self) -> *const DrImage {
        self.image as *const DrImage
    }

    fn free(self) {
        unsafe { dr_doc_free(self.doc as *mut DrDocument) };
        unsafe { dr_image_free(self.image as *mut DrImage) };
    }
}

/// A typed 16-bit noise document and its rendered projection.
#[derive(Clone, Copy)]
struct Typed {
    doc: usize,
    projection: usize,
}

impl Typed {
    fn new(w: u32, h: u32) -> Self {
        let samples: Vec<u16> = noise((w * h * 8) as usize, 0x9E37_79B9 ^ u64::from(w))
            .chunks_exact(2)
            .map(|b| u16::from_ne_bytes([b[0], b[1]]) | 0xF000)
            .collect();
        let len = dr_builtin_profile_len(PROFILE_SRGB);
        let mut profile = vec![0u8; len];
        assert!(unsafe { dr_builtin_profile(PROFILE_SRGB, profile.as_mut_ptr(), len) });
        let mut err: *mut c_char = ptr::null_mut();
        let doc = unsafe {
            dr_precision_doc_from_rgba16(
                samples.as_ptr(),
                samples.len(),
                w,
                h,
                profile.as_ptr(),
                profile.len(),
                ptr::null(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!doc.is_null(), "{}", take_err_string(err));
        let mut err: *mut c_char = ptr::null_mut();
        let projection = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
        assert!(!projection.is_null(), "{}", take_err_string(err));
        Typed {
            doc: doc as usize,
            projection: projection as usize,
        }
    }

    fn doc(self) -> *const PrecisionDocument {
        self.doc as *const PrecisionDocument
    }

    fn projection(self) -> *const PrecisionImage {
        self.projection as *const PrecisionImage
    }

    fn free(self) {
        unsafe { dr_precision_image_free(self.projection as *mut PrecisionImage) };
        unsafe { dr_precision_doc_free(self.doc as *mut PrecisionDocument) };
    }
}

fn failed(ok: bool, err: *mut c_char) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

/// One save under test: what it writes from which fixture.
#[derive(Clone, Copy)]
enum Save {
    Native(Bytes),
    Flat(Bytes, i32),
    Psd(Bytes),
    TypedPsd(Typed),
    Export16(Typed),
    Projection16(Typed),
}

impl Save {
    fn name(self) -> String {
        match self {
            Save::Native(_) => "native".into(),
            Save::Flat(_, format) => format!("flat {format}"),
            Save::Psd(_) => "PSD".into(),
            Save::TypedPsd(_) => "16-bit PSD".into(),
            Save::Export16(_) => "export16".into(),
            Save::Projection16(_) => "save_projection16".into(),
        }
    }

    fn file(self) -> &'static str {
        match self {
            Save::Native(_) => "out.doctored",
            Save::Flat(_, FMT_JPEG) => "out.jpg",
            Save::Flat(_, FMT_TIFF) => "out.tif",
            Save::Flat(_, FMT_BMP) => "out.bmp",
            Save::Psd(_) | Save::TypedPsd(_) => "out.psd",
            Save::Flat(..) | Save::Export16(_) | Save::Projection16(_) => "out.png",
        }
    }

    /// The same save over another fixture size.
    fn with(self, bytes: Bytes, typed: Typed) -> Self {
        match self {
            Save::Native(_) => Save::Native(bytes),
            Save::Flat(_, format) => Save::Flat(bytes, format),
            Save::Psd(_) => Save::Psd(bytes),
            Save::TypedPsd(_) => Save::TypedPsd(typed),
            Save::Export16(_) => Save::Export16(typed),
            Save::Projection16(_) => Save::Projection16(typed),
        }
    }

    fn run(self, path: &Path, control: *const DrJobControl) -> Result<(), String> {
        let path = cpath(path);
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            match self {
                Save::Native(b) => dr_doc_save_native(b.doc(), path.as_ptr(), control, &mut err),
                Save::Flat(b, format) => dr_doc_save_image(
                    b.doc(),
                    b.image(),
                    path.as_ptr(),
                    format,
                    90,
                    true,
                    false,
                    ptr::null_mut(),
                    control,
                    &mut err,
                ),
                Save::Psd(b) => {
                    dr_doc_save_psd(b.doc(), b.image(), path.as_ptr(), 0, 0, control, &mut err)
                }
                Save::TypedPsd(t) => dr_precision_doc_save_psd(
                    t.doc(),
                    t.projection(),
                    path.as_ptr(),
                    0,
                    0,
                    LIMIT,
                    0,
                    control,
                    &mut err,
                ),
                Save::Export16(t) => dr_precision_doc_export16(
                    t.doc(),
                    path.as_ptr(),
                    0,
                    true,
                    false,
                    LIMIT,
                    0,
                    ptr::null_mut(),
                    control,
                    &mut err,
                ),
                Save::Projection16(t) => dr_precision_doc_save_projection16(
                    t.doc(),
                    t.projection(),
                    path.as_ptr(),
                    0,
                    true,
                    false,
                    LIMIT,
                    0,
                    ptr::null_mut(),
                    control,
                    &mut err,
                ),
            }
        };
        failed(ok, err)
    }
}

fn saves(bytes: Bytes, typed: Typed) -> Vec<Save> {
    vec![
        Save::Native(bytes),
        Save::Flat(bytes, FMT_PNG),
        Save::Flat(bytes, FMT_JPEG),
        Save::Flat(bytes, FMT_TIFF),
        Save::Flat(bytes, FMT_BMP),
        Save::Psd(bytes),
        Save::TypedPsd(typed),
        Save::Export16(typed),
        Save::Projection16(typed),
    ]
}

/// One open under test: the byte open of any file, or the byte PSD open.
#[derive(Clone)]
enum Open {
    Doc(PathBuf),
    Psd(PathBuf, CString),
}

impl Open {
    fn psd(path: &Path) -> Self {
        let mut err: *mut c_char = ptr::null_mut();
        let report = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
        assert!(!report.is_null(), "{}", take_err_string(err));
        let report: Value = serde_json::from_str(&take_err_string(report)).unwrap();
        let fingerprint = report["source_fingerprint"].as_str().unwrap();
        Open::Psd(path.to_path_buf(), CString::new(fingerprint).unwrap())
    }

    fn path(&self) -> &Path {
        match self {
            Open::Doc(path) | Open::Psd(path, _) => path,
        }
    }

    fn run(&self, control: *const DrJobControl) -> Result<(), String> {
        let mut err: *mut c_char = ptr::null_mut();
        let doc = unsafe {
            match self {
                Open::Doc(path) => dr_doc_open(cpath(path).as_ptr(), control, &mut err),
                Open::Psd(path, fingerprint) => dr_doc_open_psd(
                    cpath(path).as_ptr(),
                    0,
                    fingerprint.as_ptr(),
                    control,
                    &mut err,
                ),
            }
        };
        if doc.is_null() {
            return Err(take_err_string(err));
        }
        unsafe { dr_doc_free(doc) };
        Ok(())
    }
}

/// The folder's names, sorted.
fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The checkpoint count of one completed run under a fresh control.
fn counted(run: impl FnOnce(*const DrJobControl) -> Result<(), String>) -> u64 {
    let control = DrJobControl::default();
    run(&control as *const DrJobControl).unwrap_or_else(|e| panic!("the run failed: {e}"));
    control.checkpoints()
}

fn outcome(result: Result<(), String>) -> (bool, String) {
    match result {
        Ok(()) => (true, String::new()),
        Err(e) => (false, e),
    }
}

/// `path` holds `KEEP` and is its folder's only name.
fn assert_kept(path: &Path, what: &str) {
    assert_eq!(
        std::fs::read(path).unwrap(),
        KEEP,
        "{what}: the destination changed"
    );
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        listing(path.parent().unwrap()),
        vec![name],
        "{what}: a staged file was left beside the destination"
    );
}

#[test]
fn a_cancelled_control_refuses_every_save_and_open_and_keeps_the_destination() {
    let (bytes, typed) = (Bytes::new(8, 8), Typed::new(8, 8));
    for save in saves(bytes, typed) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(save.file());
        std::fs::write(&path, KEEP).unwrap();
        let cancelled = DrJobControl::default();
        cancelled.cancel();
        assert_eq!(
            save.run(&path, &cancelled),
            Err("operation cancelled".to_string()),
            "{}",
            save.name()
        );
        assert_kept(&path, &save.name());
        let used = DrJobControl::default();
        save.run(&path, &used)
            .unwrap_or_else(|e| panic!("{}: {e}", save.name()));
        assert_eq!(
            save.run(&path, &used),
            Err("job control has already been used".to_string()),
            "{}",
            save.name()
        );
    }
    let dir = TempDir::new().unwrap();
    let mut opens = Vec::new();
    for save in [
        Save::Native(bytes),
        Save::Flat(bytes, FMT_PNG),
        Save::Psd(bytes),
    ] {
        let path = dir.path().join(save.file());
        save.run(&path, ptr::null()).unwrap();
        opens.push(Open::Doc(path));
    }
    opens.push(Open::psd(&dir.path().join("out.psd")));
    for open in opens {
        let cancelled = DrJobControl::default();
        cancelled.cancel();
        assert_eq!(
            open.run(&cancelled),
            Err("operation cancelled".to_string()),
            "{}",
            open.path().display()
        );
        let used = DrJobControl::default();
        open.run(&used).unwrap();
        assert_eq!(
            open.run(&used),
            Err("job control has already been used".to_string())
        );
    }
    bytes.free();
    typed.free();
}

#[test]
fn null_inputs_with_a_live_control_fail_without_a_panic() {
    let control = DrJobControl::default();
    let c = &control as *const DrJobControl;
    let path = CString::new("/nonexistent/doctored-null.png").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    unsafe {
        assert!(dr_doc_open(ptr::null(), c, &mut err).is_null());
        take_err_string(err);
        assert!(!dr_doc_save_native(
            ptr::null(),
            path.as_ptr(),
            c,
            ptr::null_mut()
        ));
        assert!(!dr_doc_save_image(
            ptr::null(),
            ptr::null(),
            path.as_ptr(),
            FMT_PNG,
            90,
            true,
            false,
            ptr::null_mut(),
            c,
            ptr::null_mut(),
        ));
        assert!(dr_doc_open_psd(ptr::null(), 0, ptr::null(), c, ptr::null_mut()).is_null());
        assert!(!dr_doc_save_psd(
            ptr::null(),
            ptr::null(),
            path.as_ptr(),
            0,
            0,
            c,
            ptr::null_mut()
        ));
        assert!(!dr_precision_doc_save_psd(
            ptr::null(),
            ptr::null(),
            path.as_ptr(),
            0,
            0,
            LIMIT,
            0,
            c,
            ptr::null_mut(),
        ));
        assert!(!dr_precision_doc_export16(
            ptr::null(),
            path.as_ptr(),
            0,
            true,
            false,
            LIMIT,
            0,
            ptr::null_mut(),
            c,
            ptr::null_mut(),
        ));
        assert!(!dr_precision_doc_save_projection16(
            ptr::null(),
            ptr::null(),
            path.as_ptr(),
            0,
            true,
            false,
            LIMIT,
            0,
            ptr::null_mut(),
            c,
            ptr::null_mut(),
        ));
    }
}

#[test]
fn every_save_polls_as_its_file_is_written_and_a_parked_cancel_keeps_the_destination() {
    let (small, small_typed) = (Bytes::new(2, 2), Typed::new(2, 2));
    let (large, large_typed) = (Bytes::new(768, 768), Typed::new(512, 512));
    for save in saves(large, large_typed) {
        let name = save.name();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(save.file());
        let stages = counted(|c| save.with(small, small_typed).run(&path, c));
        let total = counted(|c| save.run(&path, c));
        let controlled = std::fs::read(&path).unwrap();
        save.run(&path, ptr::null()).unwrap();
        let len = std::fs::read(&path).unwrap().len() as u64;
        assert!(
            len >= 4 * POLL_BYTES,
            "{name}: the fixture writes {len} bytes"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            controlled,
            "{name}: polling changed the bytes"
        );
        assert!(
            total - stages >= len / POLL_BYTES - 1,
            "{name}: {total} checkpoints for a {len}-byte file, {stages} for the 2×2 twin"
        );
        std::fs::write(&path, KEEP).unwrap();
        // The last checkpoint is the check before the rename; the one
        // before it is the write's last poll.
        let parked = cancelled_parked_at(total - 1, &|p, _| p < 1.0, |token| {
            outcome(save.run(&path, token as *const DrJobControl))
        });
        assert_kept(&path, &name);
        println!("{name}: {}", parked_line(&parked));
    }
    small.free();
    small_typed.free();
    large.free();
    large_typed.free();
}

#[test]
fn every_open_polls_as_its_file_is_read_and_decoded() {
    let (small, large) = (Bytes::new(2, 2), Bytes::new(768, 768));
    let dirs = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let files = |bytes: Bytes, dir: &Path| -> Vec<(Open, bool)> {
        let mut opens = Vec::new();
        for (save, decoded) in [
            (Save::Native(bytes), true),
            (Save::Flat(bytes, FMT_PNG), true),
            (Save::Flat(bytes, FMT_JPEG), true),
            (Save::Flat(bytes, FMT_TIFF), true),
            (Save::Psd(bytes), false),
        ] {
            let path = dir.join(save.file());
            save.run(&path, ptr::null()).unwrap();
            opens.push((Open::Doc(path), decoded));
        }
        opens.push((Open::psd(&dir.join("out.psd")), false));
        opens
    };
    let twins = files(small, dirs.0.path());
    for ((open, decoded), (twin, _)) in files(large, dirs.1.path()).into_iter().zip(twins) {
        let name = format!(
            "{} ({})",
            open.path().display(),
            if matches!(open, Open::Psd(..)) {
                "dr_doc_open_psd"
            } else {
                "dr_doc_open"
            }
        );
        let len = std::fs::metadata(open.path()).unwrap().len();
        assert!(len >= 4 * POLL_BYTES, "{name}: the fixture is {len} bytes");
        let stages = counted(|c| twin.run(c));
        let total = counted(|c| open.run(c));
        // The read alone polls once per interval; a decode through the same
        // adapter moves the file's bytes a second time.
        let floor = if decoded {
            2 * (len / POLL_BYTES - 1)
        } else {
            len / POLL_BYTES - 1
        };
        assert!(
            total - stages >= floor,
            "{name}: {total} checkpoints for a {len}-byte file, {stages} for the 2×2 twin"
        );
        // Begin is the first checkpoint and the read's first poll the
        // second, so the third is inside the read.
        let parked = cancelled_parked_at(3, &|p, _| p < 1.0, |token| {
            outcome(open.run(token as *const DrJobControl))
        });
        println!("{name}: {}", parked_line(&parked));
    }
    small.free();
    large.free();
}

mod bench {
    use super::*;
    use crate::common::probe::{cancelled_after_checkpoints, verdict};

    /// The bound the original qualification held on 24 and 48 MP fixtures
    /// (`designs/phase-4-jobs-resource-status.md`), declared before measuring.
    const BOUND_S: f64 = 2.0;

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn a_24_mp_native_save_cancelled_inside_its_layer_encode_returns_promptly() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("out.doctored");
        // The begin and the layer's progress report precede its PNG encode.
        let large = Bytes::new(6000, 4000);
        let probe = cancelled_after_checkpoints(
            &|_, _, checkpoints| checkpoints >= 2,
            0,
            &|p, s| p < 1.0 && s == 1,
            |token| outcome(Save::Native(large).run(&path, token as *const DrJobControl)),
        );
        println!(
            "24 MP native save cancelled inside its layer encode: {:.3} s → {} (bound {BOUND_S} s)",
            probe.latency.as_secs_f64(),
            verdict(probe.latency, BOUND_S)
        );
        large.free();
    }
}
