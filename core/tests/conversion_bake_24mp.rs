//! The 16-bit Convert's own admission plan at 24 MP stays at or above
//! its measured floor, with `{"adjustments":"preserve_appearance"}` (three
//! bakes) and at the default options (the convert colour plan). A 6000×4000
//! opaque u16 sRGB raster (the `base` pattern below) under
//! three Curves `{"rgb":[[0,0],[128,160],[255,255]]}`, converted to Display
//! P3 under an 8 GiB working limit.
//!
//! The floor is ⌈1.25 × the largest measured footprint E⌉ in
//! `precision_color_policy`'s module doc (`# Admission`): one case per
//! process, three runs, `MallocLargeCache=0`, a baseline twin.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_conversion_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{set_process_working_limit, PrecisionImage};
use serde_json::Value;

const W: u32 = 6000;
const H: u32 = 4000;
/// The working limit the measurement planned at (a 16 GB Mac's).
const LIMIT: u64 = 8_589_934_592;
/// ⌈1.25 × 1,351,140,456⌉, the measured preserve Convert's largest E.
const CONVERT16P_FLOOR: u64 = 1_688_925_570;
/// ⌈1.25 × 386,810,144⌉, the measured default Convert's largest E.
const CONVERT16_FLOOR: u64 = 483_512_680;
const PRESERVE: &str = r#"{"adjustments":"preserve_appearance"}"#;
const CURVES: &str = r#"{"rgb":[[0,0],[128,160],[255,255]]}"#;

fn take(text: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    out
}

/// `Base`: an x/y ramp with a hashed blue, opaque.
fn base(x: u64, y: u64, i: u64) -> [u16; 4] {
    [
        (x * 65535 / u64::from(W - 1)) as u16,
        (y * 65535 / u64::from(H - 1)) as u16,
        ((i * 2654435761) % 65536) as u16,
        65535,
    ]
}

fn document() -> *mut PrecisionDocument {
    let pixels: Vec<[u16; 4]> = (0..u64::from(W) * u64::from(H))
        .map(|i| base(i % u64::from(W), i / u64::from(W), i))
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let doc =
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap();
    let mut handle = Box::into_raw(Box::new(doc));
    let name = CString::new("Curves").unwrap();
    let description = CString::new(format!(
        r#"{{"type":"adjust","evaluation":"precision_v1","op":"curves","params":{CURVES}}}"#
    ))
    .unwrap();
    for _ in 0..3 {
        let mut err = ptr::null_mut();
        let next = unsafe {
            dr_precision_doc_add_adjustment(
                handle,
                name.as_ptr(),
                description.as_ptr(),
                1.0,
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!next.is_null(), "{}", take(err));
        unsafe { dr_precision_doc_free(handle) };
        handle = next;
    }
    handle
}

/// `dr_precision_doc_conversion_plan`'s `scratch_bytes` under `options`
/// (None = NULL, the defaults).
fn plan(doc: *const PrecisionDocument, options: Option<&str>) -> u64 {
    let profile = IccProfile::display_p3().bytes().to_vec();
    let options = options.map(|o| CString::new(o).unwrap());
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_conversion_plan(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ref().map_or(ptr::null(), |o| o.as_ptr()),
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take(err));
    let plan: Value = serde_json::from_str(&take(out)).unwrap();
    plan["scratch_bytes"].as_u64().unwrap()
}

#[test]
fn the_24mp_convert_plans_hold_their_measured_floors() {
    assert!(set_process_working_limit(LIMIT));
    let doc = document();
    let preserve = plan(doc, Some(PRESERVE));
    let default = plan(doc, None);
    println!("convert16p {preserve} convert16 {default}");
    assert!(
        preserve >= CONVERT16P_FLOOR,
        "{preserve} < floor {CONVERT16P_FLOOR}"
    );
    assert!(
        default >= CONVERT16_FLOOR,
        "{default} < floor {CONVERT16_FLOOR}"
    );
    unsafe { dr_precision_doc_free(doc) };
}
