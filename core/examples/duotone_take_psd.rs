//! One Duotone PSD of the #547 Duotone phase 2b take
//! (`scripts/photoshop-probe/duotone_take.py`), authored through the core's C
//! API exactly as the app's File ▸ Export as PSD writes one:
//!
//! ```text
//! duotone_take_psd <gray.png> <options.json> <out.psd>
//! ```
//!
//! `gray.png` is opened as a Grayscale document
//! (`dr_precision_doc_open_gray`, the working Gray `dot_gain_20` assigned to
//! an untagged file), `options.json` (`{"inks":[{"name","lab","curve"}]}`)
//! becomes its Duotone description (`dr_precision_doc_set_duotone`), and the
//! document is saved as a PSD from its own render
//! (`dr_precision_doc_save_psd`, layered, the 2 GiB test limit). Prints one
//! JSON line `{"psd": <out>, "options": <the document's normalized
//! options>}`, or exits non-zero with the error on stderr.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_duotone::{dr_precision_doc_duotone_options, dr_precision_doc_set_duotone};
use doctored_core::ffi_gray::dr_precision_doc_open_gray;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_psd::dr_precision_doc_save_psd;

/// The 2 GiB reference working limit the test instances pin.
const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// The owned C string `text` as a Rust string (freed), or `fallback`.
fn owned(text: *mut c_char, fallback: &str) -> String {
    if text.is_null() {
        return fallback.to_string();
    }
    let out = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    out
}

fn fail(what: &str, err: *mut c_char) -> ! {
    eprintln!(
        "duotone_take_psd: {what}: {}",
        owned(err, "no error message")
    );
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: duotone_take_psd <gray.png> <options.json> <out.psd>");
        std::process::exit(2);
    }
    let options = match std::fs::read_to_string(&args[2]) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("duotone_take_psd: failed to read {}: {e}", args[2]);
            std::process::exit(1);
        }
    };
    let gray = CString::new(args[1].as_str()).expect("a path without NUL");
    let out = CString::new(args[3].as_str()).expect("a path without NUL");
    let options = CString::new(options).expect("options without NUL");
    let working = CString::new("dot_gain_20").expect("a wire name");
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray(
            gray.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if doc.is_null() {
        fail("opening the gray PNG", err);
    }
    let duotone = unsafe { dr_precision_doc_set_duotone(doc, options.as_ptr(), &mut err) };
    unsafe { dr_precision_doc_free(doc) };
    if duotone.is_null() {
        fail("setting the inks", err);
    }
    let render = match unsafe { &*duotone }.render(LIMIT, 0, None) {
        Ok(image) => image,
        Err(e) => {
            unsafe { dr_precision_doc_free(duotone) };
            eprintln!("duotone_take_psd: rendering: {e}");
            std::process::exit(1);
        }
    };
    let saved = unsafe {
        dr_precision_doc_save_psd(duotone, &render, out.as_ptr(), 0, 0, LIMIT, 0, &mut err)
    };
    if !saved {
        unsafe { dr_precision_doc_free(duotone) };
        fail("saving the PSD", err);
    }
    let stated = unsafe { dr_precision_doc_duotone_options(duotone, &mut err) };
    unsafe { dr_precision_doc_free(duotone) };
    if stated.is_null() {
        fail("reading the options back", err);
    }
    let stated = owned(stated, "null");
    let options: serde_json::Value = serde_json::from_str(&stated).expect("the core's JSON");
    println!(
        "{}",
        serde_json::json!({"psd": args[3], "options": options})
    );
}
