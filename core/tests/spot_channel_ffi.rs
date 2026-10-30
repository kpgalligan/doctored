//! #547 spot channels, phase 1a-2 (`ffi_spot_channel`, `spot_view`): every
//! export through the C contract. The overprint against independent oracles
//! (a black ink is `round(c·(1 − w))` at any solidity; a red ink on an sRGB
//! document is the kernel run on the ink's own sRGB encoding; a profile the
//! pipeline cannot target reads as sRGB, with no error); the ink range and
//! NULL refusals; the byte and typed add, set and read round trips; the
//! typed info JSON's `spot`; the PSD, PSB and layered TIFF refusal in the
//! report at 8 and 16 bits and in Gray, and in both saves; and the flat
//! save's loss line.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_doc_info};
use doctored_core::ffi_preservation::dr_doc_flat_save_losses;
use doctored_core::ffi_psd::{
    dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_psd_export_report,
    dr_precision_doc_save_psd,
};
use doctored_core::ffi_spot_channel::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::spot_channel::{
    SpotInk, FLAT_LOSS, NOT_SPOT_REFUSAL, PSD_REFUSAL, TIFF_REFUSAL,
};
use doctored_core::spot_overprint::overprint_rgba8;
use doctored_core::spot_view::ink_encoded;
use doctored_core::DrImage;
use image::{Rgba, RgbaImage};
use serde_json::Value;

use crate::cmyk_fixture::press_profile;
use crate::common::{cpath, take_err_string};
use crate::icc_lut_builders::{build, tag_desc, tag_wtpt};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 4;
const H: u32 = 3;

fn base() -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        Rgba([(x * 60 + 13) as u8, (y * 80 + 7) as u8, 201, 255])
    })
}

fn byte_doc() -> DrDocument {
    DrDocument::from_pixels(base())
}

/// Twelve coverage values, 0 and 255 among them.
fn coverage() -> Vec<u8> {
    vec![0, 255, 17, 51, 102, 128, 200, 254, 1, 77, 160, 230]
}

/// The composite the overprint runs over: the documents' own pixels.
fn composite() -> *mut DrImage {
    let pixels = base().into_raw();
    unsafe { dr_image_from_rgba(pixels.as_ptr(), W, H) }
}

fn pixels(image: *const DrImage) -> Vec<u8> {
    let image = unsafe { &*image };
    let (w, h) = (W as usize, H as usize);
    unsafe {
        std::slice::from_raw_parts(doctored_core::ffi::dr_image_pixels_rgba(image), w * h * 4)
    }
    .to_vec()
}

/// A byte document with one spot channel added through the FFI.
fn with_spot(doc: &DrDocument, lab: [f64; 3], solidity: u8) -> *mut DrDocument {
    let name = CString::new("PANTONE").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_add_spot_channel(
            doc,
            name.as_ptr(),
            coverage().as_ptr(),
            lab[0],
            lab[1],
            lab[2],
            solidity,
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

fn overprinted(doc: *const DrDocument) -> Vec<u8> {
    let source = composite();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_spot_overprint(doc, source, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let result = pixels(out);
    unsafe {
        dr_image_free(out);
        dr_image_free(source);
    }
    result
}

#[test]
fn a_black_ink_darkens_by_its_coverage_at_any_solidity() {
    let doc = byte_doc();
    let before = base().into_raw();
    for solidity in [0u8, 40, 100] {
        let spot = with_spot(&doc, [0.0, 0.0, 0.0], solidity);
        let after = overprinted(spot);
        for (i, cov) in coverage().into_iter().enumerate() {
            let w = f64::from(cov) / 255.0;
            for c in 0..3 {
                let expected = (f64::from(before[i * 4 + c]) * (1.0 - w)).round() as u8;
                assert_eq!(
                    after[i * 4 + c],
                    expected,
                    "solidity {solidity} px {i} c {c}"
                );
            }
            assert_eq!(after[i * 4 + 3], 255);
        }
        unsafe { dr_doc_free(spot) };
    }
}

#[test]
fn a_red_ink_on_srgb_is_the_kernel_on_its_srgb_encoding() {
    let doc = byte_doc();
    let red = SpotInk::new([54.29, 80.81, 69.89], 60).unwrap();
    let encoded = ink_encoded(&red, Some(&IccProfile::srgb())).unwrap();
    assert!(
        encoded[0] > 0.9 && encoded[1] < 0.1 && encoded[2] < 0.1,
        "{encoded:?}"
    );
    let mut expected = base().into_raw();
    assert!(overprint_rgba8(&mut expected, &coverage(), encoded, 0.6));
    let spot = with_spot(&doc, red.lab(), 60);
    assert_eq!(overprinted(spot), expected);
    // A pixel at coverage 0 is untouched.
    assert_eq!(overprinted(spot)[..4], base().into_raw()[..4]);

    // The bare-image export agrees, with no profile and with sRGB's bytes.
    let source = composite();
    let srgb = IccProfile::srgb().bytes().to_vec();
    for (icc, len) in [(ptr::null(), 0), (srgb.as_ptr(), srgb.len())] {
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_image_spot_overprint(
                source,
                coverage().as_ptr(),
                12,
                red.lab()[0],
                red.lab()[1],
                red.lab()[2],
                60,
                icc,
                len,
                &mut err,
            )
        };
        assert!(!out.is_null(), "{}", take_err_string(err));
        assert_eq!(pixels(out), expected);
        unsafe { dr_image_free(out) };
    }
    unsafe {
        dr_image_free(source);
        dr_doc_free(spot);
    }
}

#[test]
fn a_profile_the_pipeline_cannot_target_overprints_as_srgb() {
    let red = SpotInk::new([54.29, 80.81, 69.89], 30).unwrap();
    let srgb_spot = with_spot(&byte_doc(), red.lab(), 30);
    let expected = overprinted(srgb_spot);
    // An RGB display profile with neither a matrix/TRC model nor a LUT: it
    // parses, and `ColorPipeline` refuses it as a destination.
    let opaque = build(
        [2, 0x10, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Opaque RGB")),
            (b"wtpt", tag_wtpt([0.9642, 1.0, 0.8249])),
        ],
    );
    let profile = IccProfile::parse(&opaque).expect("an unmodelled RGB profile parses");
    assert!(profile.model().is_none());
    let mut odd = byte_doc();
    odd.profile = Arc::new(profile);
    let odd_spot = with_spot(&odd, red.lab(), 30);
    assert_eq!(overprinted(odd_spot), expected);

    // The bare image: the same profile's bytes, and a CMYK profile's bytes,
    // which no RGB composite can carry, both read as sRGB.
    let press = press_profile("Press Test");
    let source = composite();
    for icc in [&opaque, &press] {
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_image_spot_overprint(
                source,
                coverage().as_ptr(),
                12,
                red.lab()[0],
                red.lab()[1],
                red.lab()[2],
                30,
                icc.as_ptr(),
                icc.len(),
                &mut err,
            )
        };
        assert!(!out.is_null(), "{}", take_err_string(err));
        assert_eq!(pixels(out), expected);
        unsafe { dr_image_free(out) };
    }
    unsafe {
        dr_image_free(source);
        dr_doc_free(srgb_spot);
        dr_doc_free(odd_spot);
    }
}

fn err_of(err: *mut c_char) -> String {
    assert!(!err.is_null(), "an error was expected");
    take_err_string(err)
}

#[test]
fn the_byte_exports_refuse_a_bad_ink_and_null_arguments() {
    let doc = byte_doc();
    let name = CString::new("PANTONE").unwrap();
    let lab_sentence =
        "the spot ink colour must be [L, a, b] with L from 0 to 100 and a and b from -128 to 127";
    unsafe {
        let mut err = ptr::null_mut();
        let add = |name: *const c_char, cov: *const u8, l: f64, s: u8, err: &mut *mut c_char| {
            dr_doc_add_spot_channel(&doc, name, cov, l, 0.0, 0.0, s, err)
        };
        assert!(add(name.as_ptr(), coverage().as_ptr(), 100.5, 0, &mut err).is_null());
        assert_eq!(err_of(err), lab_sentence);
        err = ptr::null_mut();
        assert!(add(name.as_ptr(), coverage().as_ptr(), 50.0, 101, &mut err).is_null());
        assert_eq!(err_of(err), "solidity must be from 0 to 100 %");
        err = ptr::null_mut();
        assert!(add(ptr::null(), coverage().as_ptr(), 50.0, 0, &mut err).is_null());
        assert_eq!(err_of(err), "name is NULL");
        err = ptr::null_mut();
        assert!(add(name.as_ptr(), ptr::null(), 50.0, 0, &mut err).is_null());
        assert_eq!(err_of(err), "spot channel coverage is required");

        // No spot channel: NULL with no message.
        let source = composite();
        err = ptr::null_mut();
        assert!(dr_doc_spot_overprint(&doc, source, &mut err).is_null());
        assert!(err.is_null());
        let spot = with_spot(&doc, [50.0, 0.0, 0.0], 0);
        assert!(dr_doc_spot_overprint(spot, ptr::null(), &mut err).is_null());
        assert_eq!(err_of(err), "required handle is NULL");
        let small = dr_image_from_rgba([0u8; 4].as_ptr(), 1, 1);
        err = ptr::null_mut();
        assert!(dr_doc_spot_overprint(spot, small, &mut err).is_null());
        assert_eq!(
            err_of(err),
            "the composite does not match this document's canvas"
        );

        let image = |cov: *const u8, len: usize, s: u8, err: &mut *mut c_char| {
            dr_image_spot_overprint(source, cov, len, 50.0, 0.0, 0.0, s, ptr::null(), 0, err)
        };
        err = ptr::null_mut();
        assert!(image(coverage().as_ptr(), 11, 0, &mut err).is_null());
        assert_eq!(err_of(err), "the spot coverage does not match the image");
        err = ptr::null_mut();
        assert!(image(ptr::null(), 12, 0, &mut err).is_null());
        assert_eq!(err_of(err), "spot channel coverage is required");
        err = ptr::null_mut();
        assert!(image(coverage().as_ptr(), 12, 101, &mut err).is_null());
        assert_eq!(err_of(err), "solidity must be from 0 to 100 %");
        err = ptr::null_mut();
        assert!(dr_image_spot_overprint(
            ptr::null(),
            coverage().as_ptr(),
            12,
            50.0,
            0.0,
            0.0,
            0,
            ptr::null(),
            0,
            &mut err
        )
        .is_null());
        assert_eq!(err_of(err), "required handle is NULL");
        dr_image_free(small);
        dr_image_free(source);
        dr_doc_free(spot);
    }
}

#[test]
fn the_byte_spot_channel_adds_sets_and_reads_back() {
    let doc = byte_doc();
    let spot = with_spot(&doc, [60.0, 40.0, 30.0], 70);
    unsafe {
        let added = &*spot;
        assert_eq!(added.channels.len(), 1);
        assert_eq!(added.channels[0].name, "PANTONE");
        assert_eq!(added.channel_plane(0).unwrap(), coverage().as_slice());
        let mut lab = [0.0f64; 3];
        let mut solidity = 0u8;
        assert!(dr_doc_channel_spot(
            spot,
            0,
            lab.as_mut_ptr(),
            &mut solidity
        ));
        assert_eq!((lab, solidity), ([60.0, 40.0, 30.0], 70));
        assert!(!dr_doc_channel_spot(
            spot,
            1,
            lab.as_mut_ptr(),
            &mut solidity
        ));

        let mut err = ptr::null_mut();
        // Nothing changes: NULL, no message.
        assert!(
            dr_doc_set_spot_channel(spot, 0, ptr::null(), 60.0, 40.0, 30.0, 70, &mut err).is_null()
        );
        assert!(err.is_null());
        // Out of range: NULL, no message.
        assert!(
            dr_doc_set_spot_channel(spot, 3, ptr::null(), 45.0, 0.0, 0.0, 10, &mut err).is_null()
        );
        assert!(err.is_null());
        let varnish = CString::new("Varnish").unwrap();
        let set =
            dr_doc_set_spot_channel(spot, 0, varnish.as_ptr(), 45.0, -20.0, -35.0, 25, &mut err);
        assert!(!set.is_null(), "{}", take_err_string(err));
        let set_doc = &*set;
        assert_eq!(set_doc.channels[0].name, "Varnish");
        assert!(dr_doc_channel_spot(
            set,
            0,
            lab.as_mut_ptr(),
            ptr::null_mut()
        ));
        assert_eq!(lab, [45.0, -20.0, -35.0]);
        assert!(dr_doc_channel_spot(set, 0, ptr::null_mut(), &mut solidity));
        assert_eq!(solidity, 25);
        assert!(
            dr_doc_set_spot_channel(spot, 0, ptr::null(), 45.0, 0.0, 0.0, 120, &mut err).is_null()
        );
        assert_eq!(err_of(err), "solidity must be from 0 to 100 %");

        // An alpha channel answers no ink and refuses the set silently.
        let alpha = (*spot)
            .add_channel("Alpha 1", &coverage(), W, H, [255, 0, 0], 0.5)
            .unwrap();
        assert!(!dr_doc_channel_spot(
            &alpha,
            1,
            lab.as_mut_ptr(),
            &mut solidity
        ));
        err = ptr::null_mut();
        assert!(
            dr_doc_set_spot_channel(&alpha, 1, ptr::null(), 45.0, 0.0, 0.0, 10, &mut err).is_null()
        );
        assert!(err.is_null());
        dr_doc_free(set);
        dr_doc_free(spot);
    }
}

fn rgb16() -> PrecisionDocument {
    let pixels = (0..W * H)
        .map(|i| [(i * 3000) as u16, 40000, (60000 - i * 2000) as u16, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

fn info(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_info(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn typed_add(doc: *const PrecisionDocument, lab: [f64; 3], solidity: u8) -> *mut PrecisionDocument {
    let name = CString::new("PANTONE").unwrap();
    let wide: Vec<u16> = coverage().iter().map(|&c| u16::from(c) * 257).collect();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_spot_channel(
            doc,
            name.as_ptr(),
            lab[0],
            lab[1],
            lab[2],
            solidity,
            wide.as_ptr(),
            wide.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

#[test]
fn the_typed_spot_channel_adds_sets_and_lists_its_ink() {
    let doc = Box::into_raw(Box::new(rgb16()));
    let spot = typed_add(doc, [60.0, 40.0, 30.0], 70);
    let channels = &info(spot)["channels"];
    assert_eq!(channels[0]["name"], "PANTONE");
    assert_eq!(
        channels[0]["spot"],
        serde_json::json!({"lab": [60.0, 40.0, 30.0], "solidity": 70})
    );
    assert!(info(doc)["channels"].as_array().unwrap().is_empty());
    unsafe {
        let name = CString::new("PANTONE").unwrap();
        let mut err = ptr::null_mut();
        let short = [0u16; 11];
        assert!(dr_precision_doc_add_spot_channel(
            doc,
            name.as_ptr(),
            50.0,
            0.0,
            0.0,
            0,
            short.as_ptr(),
            short.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert_eq!(err_of(err), "the spot coverage does not match the canvas");
        err = ptr::null_mut();
        assert!(dr_precision_doc_add_spot_channel(
            doc,
            name.as_ptr(),
            50.0,
            0.0,
            0.0,
            0,
            ptr::null(),
            12,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert_eq!(err_of(err), "spot channel coverage is required");

        // A no-op: NULL with no message.
        err = ptr::null_mut();
        assert!(dr_precision_doc_set_spot_channel(
            spot,
            0,
            ptr::null(),
            60.0,
            40.0,
            30.0,
            70,
            LIMIT,
            0,
            &mut err
        )
        .is_null());
        assert!(err.is_null());
        let varnish = CString::new("Varnish").unwrap();
        let set = dr_precision_doc_set_spot_channel(
            spot,
            0,
            varnish.as_ptr(),
            45.0,
            -20.0,
            -35.0,
            25,
            LIMIT,
            0,
            &mut err,
        );
        assert!(!set.is_null(), "{}", take_err_string(err));
        let row = &info(set)["channels"][0];
        assert_eq!(row["name"], "Varnish");
        assert_eq!(
            row["spot"],
            serde_json::json!({"lab": [45.0, -20.0, -35.0], "solidity": 25})
        );
        assert!(dr_precision_doc_set_spot_channel(
            spot,
            4,
            ptr::null(),
            45.0,
            0.0,
            0.0,
            10,
            LIMIT,
            0,
            &mut err
        )
        .is_null());
        assert_eq!(err_of(err), "typed channel index is out of range");

        // An alpha channel lists `spot: null` and refuses the set by name.
        let alpha = (*spot)
            .mask_edit(
                &doctored_core::precision_mask_edit::MaskEdit::SetChannel {
                    index: None,
                    name: "Alpha 1".into(),
                },
                &[0u16; 12],
                LIMIT,
                0,
                None,
            )
            .unwrap()
            .unwrap();
        let alpha = Box::into_raw(Box::new(alpha));
        assert_eq!(info(alpha)["channels"][1]["spot"], Value::Null);
        err = ptr::null_mut();
        assert!(dr_precision_doc_set_spot_channel(
            alpha,
            1,
            ptr::null(),
            45.0,
            0.0,
            0.0,
            10,
            LIMIT,
            0,
            &mut err
        )
        .is_null());
        assert_eq!(err_of(err), NOT_SPOT_REFUSAL);
        dr_precision_doc_free(alpha);
        dr_precision_doc_free(set);
        dr_precision_doc_free(spot);
        dr_precision_doc_free(doc);
    }
}

fn report(text: *mut c_char, err: *mut c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

/// Since #547 phase 1b a PSD or PSB at 8 or 16 bits carries spot channels
/// (`psd_export_preflight::spots_written_for`), and since 1c-2b the layered
/// TIFF of an RGB or CMYK document; a Lab PSD/PSB still refuses them by
/// name rather than dropping them, and a Gray or Lab layered TIFF refuses
/// with the TIFF's sentence.
#[test]
fn lab_and_a_gray_tiff_refuse_a_spot_channel_by_name_and_psd_and_rgb_tiff_carry_it() {
    let dir = tempfile::tempdir().unwrap();
    // 8 bits: the byte report and save; every container carries them.
    let spot = with_spot(&byte_doc(), [60.0, 40.0, 30.0], 70);
    unsafe {
        for container in 0..3 {
            for mode in 0..2 {
                let mut err = ptr::null_mut();
                let value = report(
                    dr_doc_psd_export_report(spot, container, mode, &mut err),
                    err,
                );
                assert_ne!(value["refusal"], PSD_REFUSAL, "{container} {mode}");
                assert_ne!(value["refusal"], TIFF_REFUSAL, "{container} {mode}");
            }
        }
        let file = dir.path().join("byte.psd");
        let path = cpath(&file);
        let mut err = ptr::null_mut();
        assert!(
            dr_doc_save_psd(spot, ptr::null(), path.as_ptr(), 0, 0, &mut err),
            "{}",
            take_err_string(err)
        );
        assert!(file.exists());
        dr_doc_free(spot);
    }

    // 16 bits: every container carries them. Gray: PSD and PSB carry them,
    // the layered TIFF refuses with the TIFF's sentence. Lab: every
    // container refuses, the layered TIFF with the TIFF's sentence.
    let rgb = rgb16();
    let gray = rgb
        .convert_to_gray(None, r#"{"space":"gray_gamma_22"}"#, LIMIT, 0, None)
        .unwrap();
    let lab = rgb
        .convert_to_lab(r#"{"depth":16}"#, LIMIT, 0, None)
        .unwrap();
    for (mode_name, doc) in [("rgb16", rgb), ("gray", gray), ("lab", lab)] {
        let refused =
            |container: i32| mode_name == "lab" || (container == 2 && mode_name == "gray");
        let sentence = |container: i32| {
            if container == 2 {
                TIFF_REFUSAL
            } else {
                PSD_REFUSAL
            }
        };
        let doc = Box::into_raw(Box::new(doc));
        let spot = typed_add(doc, [60.0, 40.0, 30.0], 70);
        unsafe {
            for container in 0..3 {
                for mode in 0..2 {
                    let mut err = ptr::null_mut();
                    let value = report(
                        dr_precision_doc_psd_export_report(spot, container, mode, &mut err),
                        err,
                    );
                    if refused(container) {
                        assert_eq!(value["available"], false, "{mode_name} {container} {mode}");
                        assert_eq!(
                            value["refusal"],
                            sentence(container),
                            "{mode_name} {container}"
                        );
                    } else {
                        assert_ne!(value["refusal"], PSD_REFUSAL, "{mode_name} {container}");
                        assert_ne!(value["refusal"], TIFF_REFUSAL, "{mode_name} {container}");
                    }
                }
            }
            let mut err = ptr::null_mut();
            let plain = report(dr_precision_doc_psd_export_report(doc, 0, 0, &mut err), err);
            assert_ne!(
                plain["refusal"], PSD_REFUSAL,
                "{mode_name}: no spot, no refusal"
            );
            // A refused save names the refusal before it reads anything else;
            // the typed writes themselves are `spot_psd.rs`'s.
            if refused(0) {
                let file = dir.path().join(format!("{mode_name}.psd"));
                let path = cpath(&file);
                err = ptr::null_mut();
                assert!(!dr_precision_doc_save_psd(
                    spot,
                    ptr::null(),
                    path.as_ptr(),
                    0,
                    0,
                    LIMIT,
                    0,
                    &mut err
                ));
                assert_eq!(err_of(err), PSD_REFUSAL, "{mode_name}");
                assert!(!file.exists());
            }
            dr_precision_doc_free(spot);
            dr_precision_doc_free(doc);
        }
    }
}

#[test]
fn a_flat_save_names_the_spot_channels_it_composites() {
    let losses = |doc: *const DrDocument| -> Vec<String> {
        let mut err = ptr::null_mut();
        let text = unsafe { dr_doc_flat_save_losses(doc, 0, true, false, &mut err) };
        assert!(!text.is_null(), "{}", take_err_string(err));
        let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap());
        unsafe { dr_string_free(text) };
        value.unwrap()
    };
    let spot = with_spot(&byte_doc(), [60.0, 40.0, 30.0], 70);
    let found = losses(spot);
    assert!(found.contains(&FLAT_LOSS.to_string()), "{found:?}");
    assert!(
        !found.contains(&"saved selection channels".to_string()),
        "a spot channel is not a saved selection: {found:?}"
    );
    let alpha = unsafe { &*spot }
        .add_channel("Alpha 1", &coverage(), W, H, [255, 0, 0], 0.5)
        .unwrap();
    let both = losses(&alpha);
    assert!(both.contains(&FLAT_LOSS.to_string()));
    assert!(both.contains(&"saved selection channels".to_string()));
    assert!(!losses(&byte_doc()).contains(&FLAT_LOSS.to_string()));
    unsafe { dr_doc_free(spot) };
}
