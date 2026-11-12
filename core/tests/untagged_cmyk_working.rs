//! The process-wide working CMYK profile (`dr_set_working_cmyk_profile`):
//! an untagged CMYK TIFF or JPEG the core opens is converted from it exactly
//! as the same file TAGGED with that profile converts, and the import report
//! names it. Exactly one test, because the profile is process-global and
//! tests in one binary run in parallel.
//!
//! The oracle is the tagged twin: the embedded-profile CMYK conversion is
//! itself checked against written-out oracles in `open_profile_tests`. The
//! working profile is macOS's own Generic CMYK, read from disk.
//!
//! The same test holds the CMYK document cases that need the slot set
//! (#1): with it set an untagged CMYK file opens through the typed open
//! as a CMYK document ASSIGNED the working profile, under its sentence, and
//! Image ▸ Mode ▸ CMYK Color with no profile converts into it. Since #1
//! phase 3 a well-formed planar untagged TIFF opens the same way, and an
//! untagged layered CMYK PSD imports its layers under the working profile.

#[path = "common/cmyk_jpeg_builder.rs"]
mod cmyk_jpeg_builder;
#[path = "common/cmyk_psd_fixture.rs"]
mod cmyk_psd_fixture;
#[path = "common/cmyk_tiff_builder.rs"]
mod cmyk_tiff_builder;
mod common;

use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::ptr;

use cmyk_jpeg_builder::{cmyk_jpeg, CmykJpeg};
use cmyk_tiff_builder::{cmyk_tiff, cmyk_tiff_planes};
use common::{cpath, layer_pixels, take_err_string};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::{
    dr_path_cmyk_source, dr_precision_doc_cmyk_profile, dr_precision_doc_convert_to_cmyk,
    dr_precision_doc_is_cmyk,
};
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open};
use doctored_core::ffi_metadata_report::{
    dr_path_metadata_import_report, dr_platform_conversion_sentence, dr_set_working_cmyk_profile,
};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_info, dr_precision_doc_open,
};
use doctored_core::ffi_psd::{dr_precision_doc_open_psd, dr_psd_import_report};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use tempfile::TempDir;

const GENERIC_CMYK: &str = "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc";
const DR_PLATFORM_SOURCE_CMYK: std::ffi::c_int = 1;

const UNSET: &str = "The image is CMYK with no embedded colour profile and no working CMYK \
     profile is set, so its colours were converted to sRGB without colour management as it \
     was read.";

fn converted_from_working(name: &str) -> String {
    format!(
        "The image is CMYK with no embedded colour profile, so it was converted to sRGB from \
         the working CMYK profile “{name}” as it was read."
    )
}

fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// `dr_doc_open`'s first layer, asserting the open succeeds.
fn open_pixels(path: &Path) -> Vec<u8> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c.as_ptr(), std::ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open {path:?}: {}", take_err_string(err));
    let pixels = layer_pixels(doc, 0);
    unsafe { dr_doc_free(doc) };
    pixels
}

/// The import report's `profile_refusal`.
fn refusal(path: &Path) -> Option<String> {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let json = unsafe { dr_path_metadata_import_report(c.as_ptr(), &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let report: serde_json::Value =
        serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(json) };
    report
        .get("profile_refusal")
        .map(|s| s.as_str().unwrap().to_string())
}

/// Sets the working profile (`None` clears), returning the error on refusal.
fn set(icc: Option<&[u8]>) -> Result<(), String> {
    let (data, len) = icc.map_or((ptr::null(), 0), |b| (b.as_ptr(), b.len()));
    let mut err = ptr::null_mut();
    if unsafe { dr_set_working_cmyk_profile(data, len, &mut err) } {
        assert!(err.is_null());
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

fn naive(bytes: &[u8]) -> Vec<u8> {
    image::load_from_memory(bytes)
        .unwrap()
        .to_rgba8()
        .into_raw()
}

#[test]
fn an_untagged_cmyk_file_converts_from_the_working_cmyk_profile() {
    let generic =
        std::fs::read(GENERIC_CMYK).unwrap_or_else(|e| panic!("macOS ships {GENERIC_CMYK}: {e}"));
    let dir = TempDir::new().unwrap();

    // Every (c, m, y, k) on a coarse lattice, so the twins compare across
    // the whole table rather than at one colour.
    let levels = [0u8, 64, 128, 191, 255];
    let mut data = Vec::new();
    for &c in &levels {
        for &m in &levels {
            for &y in &levels {
                for &k in &levels {
                    data.extend_from_slice(&[c, m, y, k]);
                }
            }
        }
    }
    let width = (data.len() / 4) as u32;
    let untagged_tiff = cmyk_tiff(width, 1, 8, 4, 0, &data, None);
    let tagged_tiff = cmyk_tiff(width, 1, 8, 4, 0, &data, Some(&generic));
    let untagged_tiff_path = write(&dir, "untagged.tif", &untagged_tiff);
    let tagged_tiff_path = write(&dir, "tagged.tif", &tagged_tiff);

    let jpeg = |icc: Option<&[u8]>| {
        let extra = CmykJpeg {
            adobe_transform: Some(0),
            icc,
            exif: None,
        };
        cmyk_jpeg(16, 8, [200, 100, 50, 25], &extra)
    };
    let untagged_jpeg = jpeg(None);
    let untagged_jpeg_path = write(&dir, "untagged.jpg", &untagged_jpeg);
    let tagged_jpeg_path = write(&dir, "tagged.jpg", &jpeg(Some(&generic)));

    // A planar untagged CMYK TIFF: the one strip's PlanarConfiguration
    // entry (tag 284, SHORT, count 1) rewritten from 1 to 2.
    let mut planar = untagged_tiff.clone();
    let entry = [0x1C, 0x01, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0];
    let at = planar
        .windows(entry.len())
        .position(|w| w == entry)
        .expect("the PlanarConfiguration entry");
    planar[at + 8] = 2;
    let planar_path = write(&dir, "untagged-planar.tif", &planar);

    // Nothing set yet in this process.
    assert_eq!(refusal(&untagged_tiff_path).as_deref(), Some(UNSET));

    set(Some(&generic)).expect("Generic CMYK is a working CMYK profile");

    // The untagged files open exactly as their Generic-CMYK-tagged twins.
    assert_eq!(
        open_pixels(&untagged_tiff_path),
        open_pixels(&tagged_tiff_path),
        "untagged TIFF vs tagged twin"
    );
    assert_ne!(
        open_pixels(&untagged_tiff_path),
        naive(&untagged_tiff),
        "the working profile changed the numbers"
    );
    assert_eq!(
        open_pixels(&untagged_jpeg_path),
        open_pixels(&tagged_jpeg_path),
        "untagged JPEG vs tagged twin"
    );

    // #1: the typed open makes the untagged files CMYK documents assigned
    // the working profile, under the sentence saying so, and Image ▸ Mode ▸
    // CMYK Color with no profile converts into it.
    assert_eq!(
        unsafe { dr_path_cmyk_source(cpath(&untagged_tiff_path).as_ptr()) },
        1
    );
    assert_eq!(
        unsafe { dr_path_cmyk_source(cpath(&untagged_jpeg_path).as_ptr()) },
        2
    );
    let assigned = "The image is CMYK with no embedded colour profile, so the working CMYK \
         profile “Generic CMYK Profile” was assigned as it was read.";
    for path in [&untagged_tiff_path, &untagged_jpeg_path] {
        let c = cpath(path);
        let mut err = ptr::null_mut();
        let doc = unsafe {
            dr_precision_doc_open(c.as_ptr(), 2 * 1024 * 1024 * 1024, 0, ptr::null(), &mut err)
        };
        assert!(
            !doc.is_null(),
            "typed open {path:?}: {}",
            take_err_string(err)
        );
        assert!(unsafe { dr_precision_doc_is_cmyk(doc) });
        let mut length = 0usize;
        let bytes = unsafe { dr_precision_doc_cmyk_profile(doc, &mut length) };
        assert_eq!(
            unsafe { std::slice::from_raw_parts(bytes, length) },
            &generic[..]
        );
        let json = unsafe { dr_precision_doc_info(doc, &mut err) };
        assert!(!json.is_null(), "{}", take_err_string(err));
        let info: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().unwrap()).unwrap();
        unsafe {
            dr_string_free(json);
            dr_precision_doc_free(doc);
        }
        assert_eq!(info["import_warnings"], serde_json::json!([assigned]));
    }
    // #1 phase 3: a well-formed PLANAR untagged TIFF opens the same way,
    // with its chunky twin's ink.
    let planes: Vec<Vec<u16>> = (0..4)
        .map(|c| data.chunks(4).map(|s| u16::from(s[c])).collect())
        .collect();
    let planar_ok = write(
        &dir,
        "untagged-planar-ok.tif",
        &cmyk_tiff_planes(width, 1, 8, true, &[], &planes, None),
    );
    assert_eq!(
        unsafe { dr_path_cmyk_source(cpath(&planar_ok).as_ptr()) },
        1
    );
    let typed = |path: &Path| -> *mut PrecisionDocument {
        let mut err = ptr::null_mut();
        let doc = unsafe {
            dr_precision_doc_open(
                cpath(path).as_ptr(),
                2 * 1024 * 1024 * 1024,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(
            !doc.is_null(),
            "typed open {path:?}: {}",
            take_err_string(err)
        );
        doc
    };
    let warnings = |doc: *const PrecisionDocument| -> serde_json::Value {
        let mut err = ptr::null_mut();
        let json = unsafe { dr_precision_doc_info(doc, &mut err) };
        assert!(!json.is_null(), "{}", take_err_string(err));
        let info: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().unwrap()).unwrap();
        unsafe { dr_string_free(json) };
        info["import_warnings"].clone()
    };
    let (planar_doc, chunky_doc) = (typed(&planar_ok), typed(&untagged_tiff_path));
    assert_eq!(
        unsafe { &*planar_doc }.cmyk_layer_samples(0),
        unsafe { &*chunky_doc }.cmyk_layer_samples(0)
    );
    assert_eq!(warnings(planar_doc), serde_json::json!([assigned]));
    unsafe {
        dr_precision_doc_free(planar_doc);
        dr_precision_doc_free(chunky_doc);
    }
    // ...and an untagged layered CMYK PSD imports its layers under it.
    let samples: Vec<[u16; 5]> = data
        .chunks(4)
        .map(|s| [s[0], s[1], s[2], s[3], 255].map(|v| u16::from(v) * 257))
        .collect();
    let records = vec![cmyk_psd_fixture::Rec::raster(
        "Only",
        [0, 0, 1, width as i32],
        cmyk_psd_fixture::ink_channels(
            &samples,
            width as usize,
            8,
            true,
            cmyk_psd_fixture::Comp::Rle,
            false,
        ),
    )];
    let psd_path = write(
        &dir,
        "untagged-layers.psd",
        &cmyk_psd_fixture::psd(
            &cmyk_psd_fixture::PsdSpec {
                canvas: (width, 1),
                depth: 8,
                icc: None,
                composite: (0..4).map(|_| vec![255; width as usize]).collect(),
                merged_alpha: false,
                large: false,
                lr16: false,
                resources: Vec::new(),
            },
            &records,
        ),
    );
    let mut err = ptr::null_mut();
    let text = unsafe { dr_psd_import_report(cpath(&psd_path).as_ptr(), &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let report: serde_json::Value =
        serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    assert_eq!(
        report["layered_available"], true,
        "{}",
        report["layered_refusal"]
    );
    assert_eq!(report["profile"], "Generic CMYK Profile");
    assert!(report["observations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o.as_str() == Some(assigned)));
    let fingerprint =
        std::ffi::CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let layered = unsafe {
        dr_precision_doc_open_psd(
            cpath(&psd_path).as_ptr(),
            0,
            fingerprint.as_ptr(),
            2 * 1024 * 1024 * 1024,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(
        !layered.is_null(),
        "Import Layers: {}",
        take_err_string(err)
    );
    assert_eq!(unsafe { &*layered }.cmyk_layer_samples(0), Some(samples));
    assert_eq!(warnings(layered), serde_json::json!([assigned]));
    let mut length = 0usize;
    let bytes = unsafe { dr_precision_doc_cmyk_profile(layered, &mut length) };
    assert_eq!(
        unsafe { std::slice::from_raw_parts(bytes, length) },
        &generic[..]
    );
    unsafe { dr_precision_doc_free(layered) };

    let image = PrecisionImage::from_rgba16(
        2,
        1,
        vec![[30000, 20000, 10000, 65535]; 2],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    let mut err = ptr::null_mut();
    let cmyk = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            ptr::null(),
            0,
            ptr::null(),
            2 * 1024 * 1024 * 1024,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!cmyk.is_null(), "to working CMYK: {}", take_err_string(err));
    let mut length = 0usize;
    let bytes = unsafe { dr_precision_doc_cmyk_profile(cmyk, &mut length) };
    assert_eq!(
        unsafe { std::slice::from_raw_parts(bytes, length) },
        &generic[..]
    );
    unsafe {
        dr_precision_doc_free(cmyk);
        dr_precision_doc_free(rgb);
    }

    // The report names it.
    let expected = converted_from_working("Generic CMYK Profile");
    assert_eq!(
        refusal(&untagged_tiff_path).as_deref(),
        Some(expected.as_str())
    );
    assert_eq!(
        refusal(&untagged_jpeg_path).as_deref(),
        Some(expected.as_str())
    );

    // A platform decode of an untagged file did not convert from it.
    let mut err = ptr::null_mut();
    let sentence = unsafe {
        dr_platform_conversion_sentence(ptr::null(), 0, DR_PLATFORM_SOURCE_CMYK, &mut err)
    };
    assert!(!sentence.is_null(), "{}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(sentence) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { dr_string_free(sentence) };
    assert_eq!(
        text,
        "The image is CMYK, with no embedded colour profile Doctored can convert from, so it \
         was converted to sRGB as it was read."
    );

    // The gate still refuses the patched "planar" file — one strip where a
    // planar file needs one strip set a sample — by name, working profile or
    // not.
    let c = cpath(&planar_path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c.as_ptr(), std::ptr::null(), &mut err) };
    assert!(
        doc.is_null(),
        "a planar CMYK TIFF short of its strips must not open"
    );
    let error = take_err_string(err);
    assert!(
        error.contains("a planar CMYK TIFF needs one strip or tile set per sample"),
        "{error}"
    );
    assert_eq!(refusal(&planar_path), None);

    // Refusals leave the slot as it was.
    let srgb = {
        let len = dr_builtin_profile_len(0);
        let mut out = vec![0u8; len];
        assert!(unsafe { dr_builtin_profile(0, out.as_mut_ptr(), len) });
        out
    };
    // The built-in sRGB is matrix/TRC, so `ProofProfile::parse` refuses it
    // before its device space is even asked.
    let error = set(Some(&srgb)).expect_err("sRGB is not a working CMYK profile");
    assert!(
        error.starts_with("cannot use the working CMYK profile: "),
        "{error}"
    );
    let error = set(Some(b"not an icc profile at all")).expect_err("not ICC");
    assert!(error.contains("not an ICC profile"), "{error}");
    assert_eq!(
        refusal(&untagged_tiff_path).as_deref(),
        Some(expected.as_str())
    );

    // Clearing it restores the naive numbers under the unset sentence, and
    // an untagged file no longer opens as a CMYK document.
    set(None).expect("NULL clears");
    assert_eq!(
        unsafe { dr_path_cmyk_source(cpath(&untagged_tiff_path).as_ptr()) },
        0
    );
    assert_eq!(refusal(&untagged_tiff_path).as_deref(), Some(UNSET));
    assert_eq!(open_pixels(&untagged_tiff_path), naive(&untagged_tiff));
    assert_eq!(open_pixels(&untagged_jpeg_path), naive(&untagged_jpeg));

    // An empty buffer clears too.
    set(Some(&generic)).unwrap();
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_set_working_cmyk_profile(generic.as_ptr(), 0, &mut err) });
    assert!(err.is_null());
    assert_eq!(refusal(&untagged_tiff_path).as_deref(), Some(UNSET));
}
