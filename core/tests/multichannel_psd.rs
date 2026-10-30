//! #547 Multichannel (`designs/multichannel-mode.md`): Doctored has no spot
//! channels, so a colour-mode-7 PSD or PSB is refused by name at every depth,
//! never by a phase. The files are crafted here (a bare header, empty
//! sections and a raw composite), written into a `TempDir` and read through
//! the FFI:
//!
//! * the compatibility report names the colour mode "Multichannel" and both
//!   choices refuse with the sentence, for PSD and PSB at 8, 16 and 32 bits;
//! * the byte, precision, Gray and Lab opens all refuse, the first two with
//!   the sentence and the last two by their own "is not" admissions.

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_precision_doc_open_gray_psd, dr_precision_doc_open_lab_psd,
    dr_precision_doc_open_psd, dr_psd_import_report,
};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};

const SENTENCE: &str = "Multichannel files are not supported: Doctored has no spot channels";
const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 4;
const H: u32 = 3;

/// Header (8BPS, version 1 = PSD / 2 = PSB, 6 zero bytes, channels, height, width, depth, colour mode 7),
/// empty colour-mode-data and resource sections (u32 0), empty layer-and-mask section
/// (u32 0 for PSD, u64 0 for PSB), compression 0, then channels × width × height raw samples at `depth`.
fn multichannel_file(version: u16, depth: u16, channels: u16) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"8BPS");
    bytes.extend_from_slice(&version.to_be_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&channels.to_be_bytes());
    bytes.extend_from_slice(&H.to_be_bytes());
    bytes.extend_from_slice(&W.to_be_bytes());
    bytes.extend_from_slice(&depth.to_be_bytes());
    bytes.extend_from_slice(&7u16.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes.extend_from_slice(&0u32.to_be_bytes());
    if version == 2 {
        bytes.extend_from_slice(&0u64.to_be_bytes());
    } else {
        bytes.extend_from_slice(&0u32.to_be_bytes());
    }
    bytes.extend_from_slice(&0u16.to_be_bytes());
    let samples = usize::from(channels) * (W * H) as usize * usize::from(depth / 8);
    bytes.extend((0..samples).map(|i| (i * 37 % 251) as u8));
    bytes
}

fn written(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn fingerprint(path: &Path) -> CString {
    CString::new(report(path)["source_fingerprint"].as_str().unwrap()).unwrap()
}

fn refused_precision(doc: *mut PrecisionDocument, err: *mut std::os::raw::c_char) -> String {
    if !doc.is_null() {
        unsafe { dr_precision_doc_free(doc) };
        panic!("a precision open admitted a Multichannel file");
    }
    take_err_string(err)
}

#[test]
fn the_report_names_multichannel_and_refuses_both_choices() {
    let dir = TempDir::new().unwrap();
    for version in [1u16, 2] {
        for depth in [8u16, 16, 32] {
            let name = format!("multichannel-v{version}-{depth}.psd");
            let path = written(&dir, &name, &multichannel_file(version, depth, 3));
            let r = report(&path);
            assert_eq!(r["color_mode"], "Multichannel", "{name}: {r}");
            assert_eq!(r["layered_available"], false, "{name}: {r}");
            assert_eq!(r["stored_composite_available"], false, "{name}: {r}");
            for key in ["layered_refusal", "stored_composite_refusal"] {
                let refusal = r[key].as_str().unwrap_or_default();
                assert!(refusal.contains(SENTENCE), "{name} {key}: {r}");
            }
        }
    }
}

#[test]
fn every_psd_open_refuses_a_multichannel_file_by_name() {
    let dir = TempDir::new().unwrap();
    let byte = written(&dir, "mc8.psd", &multichannel_file(1, 8, 3));
    let deep = written(&dir, "mc16.psd", &multichannel_file(1, 16, 3));
    let float = written(&dir, "mc32.psb", &multichannel_file(2, 32, 3));

    let print = fingerprint(&byte);
    for mode in [0, 1] {
        let mut err = ptr::null_mut();
        let doc = unsafe { dr_doc_open_psd(cpath(&byte).as_ptr(), mode, print.as_ptr(), &mut err) };
        if !doc.is_null() {
            unsafe { dr_doc_free(doc) };
            panic!("the byte open admitted a Multichannel file (mode {mode})");
        }
        let error = take_err_string(err);
        assert!(error.contains(SENTENCE), "byte mode {mode}: {error}");
    }

    for path in [&deep, &float] {
        let print = fingerprint(path);
        for mode in [0, 1] {
            let mut err = ptr::null_mut();
            let doc = unsafe {
                dr_precision_doc_open_psd(
                    cpath(path).as_ptr(),
                    mode,
                    print.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
            };
            let error = refused_precision(doc, err);
            assert!(error.contains(SENTENCE), "{path:?} mode {mode}: {error}");
        }
    }

    let working = CString::new("dot_gain_20").unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray_psd(
            cpath(&byte).as_ptr(),
            0,
            print.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let error = refused_precision(doc, err);
    assert!(
        error.contains("a Multichannel file is not Grayscale"),
        "{error}"
    );

    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_lab_psd(
            cpath(&byte).as_ptr(),
            0,
            print.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    let error = refused_precision(doc, err);
    assert!(
        error.contains("a Multichannel file is not Lab Color"),
        "{error}"
    );
}
