//! A save interrupted part-way leaves nothing behind, and the next save into a
//! folder removes what a killed one left. Black-box through the FFI.
//!
//! The sweep: every core write is staged at `<name>.dr-tmp-<pid>-<seq>`
//! (`stale_temp`), so a folder is planted with such names whose pid is a
//! process this test spawned and reaped (dead), this process's own, launchd's
//! (pid 1: alive, and `kill(1, 0)` answers EPERM) and malformed ones, plus a
//! directory and a symlink named like a dead process's temporary; one
//! `dr_image_save` into the folder must remove exactly the dead process's
//! regular files. The oracle is the name rule itself, written out here.
//!
//! The interrupted save: a typed native save parked at its last checkpoint
//! inside the streamed write, over an existing destination, is cancelled
//! there: the temporary exists while it is parked, the call answers
//! `operation cancelled`, and afterwards the folder holds only the previous
//! file with its bytes unchanged.

use std::ffi::c_char;
use std::path::Path;
use std::ptr;
use std::thread;
use std::time::Duration;

use doctored_core::ffi::{dr_image_free, dr_image_save};
use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use tempfile::TempDir;

use crate::common;
use common::probe::Released;
use common::{cpath, opaque_pattern, open_pattern, take_err_string};

const FMT_PNG: i32 = 0;

/// The pid of a child this process spawned and reaped: no process has it.
fn dead_pid() -> u32 {
    let mut child = std::process::Command::new("/usr/bin/true")
        .spawn()
        .expect("spawn a child");
    let pid = child.id();
    child.wait().expect("reap the child");
    pid
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_save_removes_only_the_temporaries_of_dead_processes() {
    let dir = TempDir::new().unwrap();
    let source = TempDir::new().unwrap();
    let image = open_pattern(&source, "pattern.png", &opaque_pattern(8, 6));
    let folder = dir.path().join("destination");
    std::fs::create_dir(&folder).unwrap();
    let (dead, own) = (dead_pid(), std::process::id());
    let removed = [
        format!("a.png.dr-tmp-{dead}-0"),
        format!(".doctored-save-{dead}-0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0.dr-tmp-{dead}-12"),
    ];
    let kept_files = [
        format!("b.dr-tmp-{own}-9"),
        "c.dr-tmp-1-0".to_string(),
        "d.dr-tmp-x-0".to_string(),
        "e.dr-tmp-0-0".to_string(),
        format!("f.dr-tmp-{dead}-"),
        format!("g.dr-tmp-{dead}-1-2"),
        "h.dr-tmp-99999999999-0".to_string(),
        format!("i.dr-tmp-{dead}-0.png"),
        "users-own.png".to_string(),
    ];
    for name in removed.iter().chain(&kept_files) {
        std::fs::write(folder.join(name), b"staged bytes").unwrap();
    }
    let directory = format!("j.dr-tmp-{dead}-0");
    std::fs::create_dir(folder.join(&directory)).unwrap();
    let target = dir.path().join("target");
    std::fs::write(&target, b"a symlink's target").unwrap();
    let link = format!("k.dr-tmp-{dead}-0");
    std::os::unix::fs::symlink(&target, folder.join(&link)).unwrap();

    let out = folder.join("out.png");
    let path = cpath(&out);
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_image_save(image, path.as_ptr(), FMT_PNG, 90, &mut err) };
    assert!(ok, "the save failed: {}", take_err_string(err));
    unsafe { dr_image_free(image) };

    let mut expected: Vec<String> = kept_files.to_vec();
    expected.extend([directory, link, "out.png".to_string()]);
    expected.sort();
    assert_eq!(
        listing(&folder),
        expected,
        "only the dead process's regular temporaries go (dead pid {dead})"
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"a symlink's target");
    let decoded = image::open(&out).expect("the saved PNG decodes").to_rgba8();
    assert_eq!(decoded, opaque_pattern(8, 6));
}

/// A 256×256 typed document of distinct samples.
fn typed_document() -> PrecisionDocument {
    let (w, h) = (256u32, 256u32);
    let samples = (0..w * h)
        .map(|i| {
            let v = (i.wrapping_mul(2_654_435_761) >> 16) as u16;
            [v, v.rotate_left(5), v.rotate_left(11), 65535]
        })
        .collect();
    let image = PrecisionImage::from_rgba16(w, h, samples, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Base", image, (0, 0), 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
}

fn save_native(
    document: &PrecisionDocument,
    path: &Path,
    control: &DrJobControl,
) -> (bool, String) {
    let path = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_native(
            document,
            path.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            control,
            &mut err,
        )
    };
    (
        ok,
        if ok {
            String::new()
        } else {
            take_err_string(err)
        },
    )
}

#[test]
fn a_save_cancelled_while_its_bytes_are_written_keeps_the_previous_file() {
    let dir = TempDir::new().unwrap();
    let document = typed_document();
    let twin = DrJobControl::default();
    let (ok, err) = save_native(&document, &dir.path().join("twin.doctored"), &twin);
    assert!(ok, "{err}");
    // The save's last checkpoint is the check after the flush; the one before
    // it is the streamed write's last.
    let inside_the_write = twin.checkpoints() - 1;
    std::fs::remove_file(dir.path().join("twin.doctored")).unwrap();

    let destination = dir.path().join("doc.doctored");
    std::fs::write(&destination, b"previous complete file").unwrap();
    let control = DrJobControl::default();
    control.park_at(inside_the_write);
    let (ok, err) = thread::scope(|scope| {
        let worker = scope.spawn(|| save_native(&document, &destination, &control));
        let _guard = Released(&control);
        assert!(
            control.wait_parked(Duration::from_secs(240)),
            "the save never reached checkpoint {inside_the_write}"
        );
        let staged = listing(dir.path());
        assert!(
            staged.iter().any(|n| n.starts_with("doc.doctored.dr-tmp-")),
            "the temporary exists while the write is parked: {staged:?}"
        );
        control.cancel();
        control.release();
        worker.join().unwrap()
    });
    assert!(!ok, "the cancelled save published");
    assert_eq!(err, "operation cancelled");
    assert_eq!(listing(dir.path()), ["doc.doctored"]);
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"previous complete file"
    );
}
