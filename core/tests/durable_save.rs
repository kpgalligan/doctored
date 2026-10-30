//! Every core file writer publishes through `save_atomically`, which now
//! barrier-flushes the temporary before the rename and best-effort flushes the
//! parent directory after it. These guards drive all seven writers through the
//! FFI: a bare relative path (whose parent is the empty string, flushed as
//! ".") and a relative subdirectory path must save, and a directory this
//! process cannot open for its flush must not turn a published save into a
//! failure. The flush calls themselves are counted by the lldb receipt, not
//! here.

mod common;

use common::{cpath, doc_from, solid, take_err_string};
use doctored_core::doc::DrDocument;
use doctored_core::doc_export::ExportResult;
use doctored_core::ffi::{dr_image_free, dr_image_save};
use doctored_core::ffi_doc::{dr_doc_flattened, dr_doc_free, dr_doc_save_native};
use doctored_core::ffi_export::{
    dr_doc_export_encode, dr_export_result_free, dr_export_result_write,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_export16, dr_precision_doc_free, dr_precision_doc_open,
    dr_precision_doc_save_native,
};
use doctored_core::ffi_psd::dr_doc_save_psd;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::DrImage;
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use std::ffi::{c_char, c_int, CString};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::ptr;
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
/// `DR_FORMAT_PNG`.
const FMT_PNG: c_int = 0;
const FLOAT_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/float_11f/scene.tiff"
);

/// Every opened input the writer table borrows, freed on drop.
struct Fixtures {
    _dir: TempDir,
    doc: *mut DrDocument,
    flat: *mut DrImage,
    export: *mut ExportResult,
    u16_doc: *mut PrecisionDocument,
    float_doc: *mut PrecisionDocument,
}

impl Drop for Fixtures {
    fn drop(&mut self) {
        unsafe {
            dr_export_result_free(self.export);
            dr_image_free(self.flat);
            dr_doc_free(self.doc);
            dr_precision_doc_free(self.u16_doc);
            dr_precision_doc_free(self.float_doc);
        }
    }
}

fn open_precision(path: &Path) -> *mut PrecisionDocument {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    doc
}

/// Opens every input with absolute paths, before any test changes the cwd.
fn fixtures() -> Fixtures {
    let dir = TempDir::new().unwrap();
    let doc = doc_from(&dir, "byte.png", &solid(6, 4, [200, 90, 30, 255]));
    let mut err: *mut c_char = ptr::null_mut();
    let flat = unsafe { dr_doc_flattened(doc, 0, std::ptr::null(), &mut err) };
    assert!(!flat.is_null(), "{}", take_err_string(err));
    let request = CString::new(r#"{"format":"png"}"#).unwrap();
    let export =
        unsafe { dr_doc_export_encode(doc, ptr::null(), request.as_ptr(), ptr::null(), &mut err) };
    assert!(!export.is_null(), "{}", take_err_string(err));

    let rgba16: Vec<u8> = (0..6u16)
        .flat_map(|i| [1000 + i * 7, 2000 + i * 3, 3000 + i, 65535])
        .flat_map(u16::to_ne_bytes)
        .collect();
    let mut encoded = Vec::new();
    PngEncoder::new(&mut encoded)
        .write_image(&rgba16, 3, 2, ExtendedColorType::Rgba16)
        .unwrap();
    let u16_path = dir.path().join("deep.png");
    std::fs::write(&u16_path, encoded).unwrap();
    let u16_doc = open_precision(&u16_path);
    let float_doc = open_precision(Path::new(FLOAT_FIXTURE));
    Fixtures {
        _dir: dir,
        doc,
        flat,
        export,
        u16_doc,
        float_doc,
    }
}

fn check(ok: bool, err: *mut c_char, what: &str, path: &Path) -> Result<(), String> {
    if ok {
        assert!(err.is_null(), "{what}: success must not set err_out");
        Ok(())
    } else {
        Err(format!(
            "{what} -> {}: {}",
            path.display(),
            take_err_string(err)
        ))
    }
}

type Writer<'a> = (
    &'static str,
    &'static str,
    Box<dyn Fn(&Path) -> Result<(), String> + 'a>,
);

/// The seven writers, each named with the extension it saves under.
fn writers(f: &Fixtures) -> Vec<Writer<'_>> {
    vec![
        (
            "dr_image_save",
            "png",
            Box::new(move |path: &Path| {
                let c = cpath(path);
                let mut err: *mut c_char = ptr::null_mut();
                let ok = unsafe { dr_image_save(f.flat, c.as_ptr(), FMT_PNG, 90, &mut err) };
                check(ok, err, "dr_image_save", path)
            }),
        ),
        (
            "dr_doc_save_native",
            "rz",
            Box::new(move |path: &Path| {
                let c = cpath(path);
                let mut err: *mut c_char = ptr::null_mut();
                let ok = unsafe { dr_doc_save_native(f.doc, c.as_ptr(), &mut err) };
                check(ok, err, "dr_doc_save_native", path)
            }),
        ),
        (
            "dr_doc_save_psd",
            "psd",
            Box::new(move |path: &Path| {
                let c = cpath(path);
                let mut err: *mut c_char = ptr::null_mut();
                let ok = unsafe { dr_doc_save_psd(f.doc, ptr::null(), c.as_ptr(), 0, 0, &mut err) };
                check(ok, err, "dr_doc_save_psd", path)
            }),
        ),
        (
            "dr_export_result_write",
            "export.png",
            Box::new(move |path: &Path| {
                let c = cpath(path);
                let mut err: *mut c_char = ptr::null_mut();
                let ok = unsafe { dr_export_result_write(f.export, c.as_ptr(), false, &mut err) };
                check(ok, err, "dr_export_result_write", path)
            }),
        ),
        (
            "dr_precision_doc_save_native (u16)",
            "u16.rz",
            Box::new(move |path: &Path| {
                let c = cpath(path);
                let mut err: *mut c_char = ptr::null_mut();
                let ok = unsafe {
                    dr_precision_doc_save_native(
                        f.u16_doc,
                        c.as_ptr(),
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut err,
                    )
                };
                check(ok, err, "dr_precision_doc_save_native (u16)", path)
            }),
        ),
        (
            "dr_precision_doc_save_native (float)",
            "float.rz",
            Box::new(move |path: &Path| {
                let c = cpath(path);
                let mut err: *mut c_char = ptr::null_mut();
                let ok = unsafe {
                    dr_precision_doc_save_native(
                        f.float_doc,
                        c.as_ptr(),
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut err,
                    )
                };
                check(ok, err, "dr_precision_doc_save_native (float)", path)
            }),
        ),
        (
            "dr_precision_doc_export16",
            "16.png",
            Box::new(move |path: &Path| {
                let c = cpath(path);
                let mut err: *mut c_char = ptr::null_mut();
                let ok = unsafe {
                    dr_precision_doc_export16(
                        f.u16_doc,
                        c.as_ptr(),
                        0,
                        true,
                        false,
                        LIMIT,
                        0,
                        ptr::null_mut(),
                        &mut err,
                    )
                };
                check(ok, err, "dr_precision_doc_export16", path)
            }),
        ),
    ]
}

/// Every entry of `dir`, recursively, whose name carries the core's
/// temporary infix.
fn temporaries(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(temporaries(&path));
        } else if path.to_string_lossy().contains("dr-tmp") {
            found.push(path);
        }
    }
    found
}

fn assert_written(path: &Path) {
    let size = std::fs::metadata(path)
        .unwrap_or_else(|e| panic!("{} missing: {e}", path.display()))
        .len();
    assert!(size > 0, "{} is empty", path.display());
}

/// A bare relative path has the parent `""` (flushed as "."), and a relative
/// subdirectory path a relative parent. This is the only test that changes
/// the process cwd; the other opens and writes by absolute path.
#[test]
fn every_writer_saves_to_bare_and_subdirectory_relative_paths() {
    let f = fixtures();
    let out = TempDir::new().unwrap();
    std::fs::create_dir(out.path().join("sub")).unwrap();
    // Restores the cwd even if a writer panics.
    struct RestoreCwd(PathBuf);
    impl Drop for RestoreCwd {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let restore = RestoreCwd(std::env::current_dir().unwrap());
    std::env::set_current_dir(out.path()).unwrap();
    let results: Vec<(String, Result<(), String>)> = writers(&f)
        .iter()
        .flat_map(|(name, ext, write)| {
            let bare = PathBuf::from(format!("name.{ext}"));
            let nested = PathBuf::from(format!("sub/name.{ext}"));
            [
                (format!("{name} bare"), write(&bare)),
                (format!("{name} sub"), write(&nested)),
            ]
        })
        .collect();
    drop(restore);
    for (what, result) in &results {
        assert!(result.is_ok(), "{what}: {result:?}");
    }
    for (_, ext, _) in writers(&f) {
        assert_written(&out.path().join(format!("name.{ext}")));
        assert_written(&out.path().join(format!("sub/name.{ext}")));
    }
    assert_eq!(temporaries(out.path()), Vec::<PathBuf>::new());
}

/// A write-and-search-only directory (0o300) accepts the temporary and the
/// rename, but cannot be opened for its flush: the save still succeeds.
#[test]
fn every_writer_succeeds_when_the_directory_cannot_be_opened_for_its_flush() {
    let f = fixtures();
    let out = TempDir::new().unwrap();
    let locked = out.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o300)).unwrap();
    let results: Vec<(&str, Result<(), String>)> = writers(&f)
        .iter()
        .map(|(name, ext, write)| (*name, write(&locked.join(format!("name.{ext}")))))
        .collect();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    for (what, result) in &results {
        assert!(result.is_ok(), "{what}: {result:?}");
    }
    for (_, ext, _) in writers(&f) {
        assert_written(&locked.join(format!("name.{ext}")));
    }
    assert_eq!(temporaries(out.path()), Vec::<PathBuf>::new());
}
