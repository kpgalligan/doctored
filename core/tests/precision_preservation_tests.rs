//! `dr_precision_doc_flat_state_losses`: the editable state a flattened
//! file of a typed document has no place for. A plain one-layer Grayscale or
//! CMYK document answers `[]`; each kind of state added through its own
//! export adds its own line and no other. The oracle is the file itself: a
//! Grayscale document carrying the state is written through the kept-gray
//! export and a CMYK one through its TIFF save, each reopened, and the
//! reopened document holds none of what the list named (read through the
//! guide, sampler, channel and layer accessors), while a plain document
//! reopens with nothing to name. The byte list's words and order are
//! `preservation_tests.rs`'s.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::dr_precision_doc_save_cmyk;
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_precision_doc_export_encode,
};
use doctored_core::ffi_gray::{dr_precision_doc_convert_to_gray, dr_precision_doc_open_gray};
use doctored_core::ffi_guide::{
    dr_precision_doc_add_guide, dr_precision_doc_guide_count, dr_precision_doc_set_ruler_origin,
};
use doctored_core::ffi_precision::dr_precision_doc_open;
use doctored_core::ffi_preservation::dr_precision_doc_flat_state_losses;
use doctored_core::ffi_sampler::{
    dr_precision_doc_color_sampler_count, dr_precision_doc_with_color_samplers,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use tempfile::TempDir;

use crate::cmyk_structure_fixture::{
    cmyk_layers, free, then, with_levels, with_mask, with_opacity, RgbLayer, LIMIT,
};
use crate::common::{cpath, take_err_string};
use crate::gray_fixture::gray_icc;

const W: u32 = 6;
const H: u32 = 4;

fn opaque() -> Vec<[u16; 4]> {
    (0..W * H)
        .map(|i| {
            let v = (i * 2500 + 1000) as u16;
            [v, v, v, 65535]
        })
        .collect()
}

/// A one-layer 16-bit Grayscale document ("Background") of `pixels`.
fn gray(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    let icc = gray_icc("Preservation Gray", 2.2);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_gray(
            rgb,
            icc.as_ptr(),
            icc.len(),
            c"{\"depth\":16}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to Gray: {}", take_err_string(err));
    free(rgb);
    out
}

/// A one-layer CMYK document ("Background") of `pixels` in the press profile.
fn cmyk(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    cmyk_layers(
        W,
        H,
        vec![RgbLayer {
            name: "Background",
            w: W,
            h: H,
            offset: (0, 0),
            pixels,
        }],
    )
}

fn losses(doc: *const PrecisionDocument, alpha_kept: bool) -> Vec<String> {
    let mut err = ptr::null_mut();
    let json = unsafe { dr_precision_doc_flat_state_losses(doc, alpha_kept, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let list = serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(json) };
    list
}

/// A typed result, the input freed.
fn swap(
    doc: *mut PrecisionDocument,
    next: *mut PrecisionDocument,
    what: &str,
) -> *mut PrecisionDocument {
    assert!(!next.is_null(), "{what} refused");
    free(doc);
    next
}

fn plane() -> Vec<u16> {
    (0..W * H).map(|i| (i * 3000) as u16).collect()
}

type Edit = fn(*mut PrecisionDocument) -> *mut PrecisionDocument;

/// Each kind of state, through its own export, with the one line it adds.
fn states() -> Vec<(&'static str, &'static str, Edit)> {
    vec![
        ("guide", "guides and ruler origin", |d| {
            swap(d, unsafe { dr_precision_doc_add_guide(d, 1, 2.0) }, "guide")
        }),
        ("ruler origin", "guides and ruler origin", |d| {
            swap(
                d,
                unsafe { dr_precision_doc_set_ruler_origin(d, 2.0, 1.0) },
                "ruler",
            )
        }),
        ("alpha channel", "saved selection channels", |d| {
            then(
                d,
                r#"{"op":"set_channel","index":null,"name":"Alpha 1"}"#,
                &plane(),
            )
        }),
        ("sampler", "color samplers", |d| {
            let xys = [2u32, 1, 1];
            swap(
                d,
                unsafe { dr_precision_doc_with_color_samplers(d, xys.as_ptr(), 1) },
                "sampler",
            )
        }),
        ("mask", "layer masks", |d| with_mask(d, 0, &plane())),
        ("opacity", "layer properties", |d| with_opacity(d, 0, 0.5)),
        ("hidden", "layer properties", |d| {
            then(
                d,
                r#"{"op":"layer_visible","layer":0,"visible":false}"#,
                &[],
            )
        }),
        ("blend mode", "layer properties", |d| {
            then(
                d,
                r#"{"op":"layer_blend","version":1,"layer":0,"blend_mode":1}"#,
                &[],
            )
        }),
        ("lock", "layer properties", |d| {
            then(d, r#"{"op":"layer_lock","layer":0,"locks":1}"#, &[])
        }),
        ("label", "layer properties", |d| {
            then(d, r#"{"op":"layer_label","layer":0,"label":3}"#, &[])
        }),
        ("name", "layer properties", |d| {
            then(
                d,
                r#"{"op":"rename_layer","layer":0,"name":"Retouch"}"#,
                &[],
            )
        }),
        // Named "Background" too, so its name is no layer property.
        ("adjustment record", "layers and groups", |d| {
            then(
                with_levels(d, 1.0),
                r#"{"op":"rename_layer","layer":1,"name":"Background"}"#,
                &[],
            )
        }),
    ]
}

#[test]
fn a_plain_gray_or_cmyk_document_loses_nothing() {
    let doc = gray(opaque());
    assert!(losses(doc, true).is_empty(), "{:?}", losses(doc, true));
    assert!(losses(doc, false).is_empty(), "{:?}", losses(doc, false));
    free(doc);
    let doc = cmyk(opaque());
    assert!(losses(doc, false).is_empty(), "{:?}", losses(doc, false));
    free(doc);
}

#[test]
fn each_kind_of_state_names_its_own_line_and_no_other() {
    for (mode, make) in [("gray", gray as fn(_) -> _), ("cmyk", cmyk)] {
        for (what, line, edit) in states() {
            let doc = edit(make(opaque()));
            assert_eq!(
                losses(doc, mode == "gray"),
                vec![line.to_string()],
                "{mode} {what}"
            );
            free(doc);
        }
    }
}

#[test]
fn transparency_counts_only_where_the_file_has_no_alpha() {
    let mut pixels = opaque();
    pixels[3][3] = 30000;
    let doc = gray(pixels.clone());
    assert!(losses(doc, true).is_empty(), "{:?}", losses(doc, true));
    assert_eq!(losses(doc, false), vec!["pixel transparency".to_string()]);
    free(doc);
    let doc = cmyk(pixels);
    assert_eq!(losses(doc, false), vec!["pixel transparency".to_string()]);
    free(doc);
}

/// A kept-gray export of `doc` in `format` (`png` or `jpeg`), reopened.
fn gray_round_trip(
    doc: *const PrecisionDocument,
    format: &str,
    dir: &TempDir,
) -> *mut PrecisionDocument {
    let depth = if format == "jpeg" { 8 } else { 16 };
    let request = CString::new(format!(
        r#"{{"format":"{format}","bit_depth":{depth},"jpeg_quality":100,"profile":"document","dither":"none"}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_encode(
            doc,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!result.is_null(), "{format}: {}", take_err_string(err));
    let mut len = 0usize;
    let bytes = unsafe {
        let p = dr_export_result_bytes(result, &mut len);
        std::slice::from_raw_parts(p, len).to_vec()
    };
    unsafe { dr_export_result_free(result) };
    let path = dir.path().join(format!("flat.{format}"));
    std::fs::write(&path, bytes).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_open_gray(
            cpath(&path).as_ptr(),
            c"gray_gamma_22".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "reopen {format}: {}", take_err_string(err));
    out
}

/// The CMYK TIFF save of `doc`, reopened.
fn cmyk_round_trip(doc: *const PrecisionDocument, dir: &TempDir) -> *mut PrecisionDocument {
    let path = dir.path().join("flat.tif");
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_cmyk(
                doc,
                cpath(&path).as_ptr(),
                c"{\"format\":\"tiff\"}".as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_open(cpath(&path).as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "reopen: {}", take_err_string(err));
    out
}

/// What the reopened file kept of the state the list names: guides, the ruler
/// origin, samplers, alpha channels, layers, and the one layer's opacity,
/// visibility, lock, label, name and mask.
fn kept(doc: *const PrecisionDocument) -> Vec<&'static str> {
    let d = unsafe { &*doc };
    let layer = &d.root_layers()[0];
    let mut out = Vec::new();
    if unsafe { dr_precision_doc_guide_count(doc) } > 0 {
        out.push("guides");
    }
    if unsafe { dr_precision_doc_color_sampler_count(doc) } > 0 {
        out.push("samplers");
    }
    if !d.channels().is_empty() {
        out.push("channels");
    }
    if d.root_layers().len() != 1 {
        out.push("layers");
    }
    if layer.opacity() != 1.0 || !layer.visible() || layer.locks() != 0 || layer.label() != 0 {
        out.push("layer properties");
    }
    if layer.name() != "Background" {
        out.push("name");
    }
    if layer.mask().is_some() {
        out.push("mask");
    }
    out
}

#[test]
fn the_flat_file_drops_what_the_list_names_and_a_plain_file_reopens_plain() {
    let dir = TempDir::new().unwrap();
    let rich: Edit = |d| {
        let d = swap(d, unsafe { dr_precision_doc_add_guide(d, 0, 1.0) }, "guide");
        let xys = [2u32, 1, 1];
        let d = swap(
            d,
            unsafe { dr_precision_doc_with_color_samplers(d, xys.as_ptr(), 1) },
            "s",
        );
        let d = then(
            d,
            r#"{"op":"set_channel","index":null,"name":"Alpha 1"}"#,
            &plane(),
        );
        let d = then(d, r#"{"op":"layer_label","layer":0,"label":3}"#, &[]);
        let d = then(d, r#"{"op":"layer_lock","layer":0,"locks":1}"#, &[]);
        let d = then(
            d,
            r#"{"op":"rename_layer","layer":0,"name":"Retouch"}"#,
            &[],
        );
        with_mask(with_opacity(d, 0, 0.5), 0, &plane())
    };
    let named = [
        "layer masks",
        "layer properties",
        "saved selection channels",
        "guides and ruler origin",
        "color samplers",
    ];
    for mode in ["gray", "cmyk"] {
        let make = if mode == "gray" { gray } else { cmyk };
        let doc = rich(make(opaque()));
        assert_eq!(losses(doc, mode == "gray"), named, "{mode}");
        assert_eq!(
            kept(doc),
            [
                "guides",
                "samplers",
                "channels",
                "layer properties",
                "name",
                "mask"
            ],
            "{mode}: the state took"
        );
        let again = if mode == "gray" {
            gray_round_trip(doc, "png", &dir)
        } else {
            cmyk_round_trip(doc, &dir)
        };
        assert!(
            kept(again).is_empty(),
            "{mode}: the file kept {:?}",
            kept(again)
        );
        assert!(
            losses(again, mode == "gray").is_empty(),
            "{mode}: {:?}",
            losses(again, true)
        );
        free(again);
        free(doc);
        let plain = make(opaque());
        let again = if mode == "gray" {
            gray_round_trip(plain, "png", &dir)
        } else {
            cmyk_round_trip(plain, &dir)
        };
        assert!(
            losses(again, mode == "gray").is_empty(),
            "{mode} plain: {:?}",
            losses(again, true)
        );
        free(again);
        free(plain);
    }
}

/// A Grayscale pixel at alpha 0 with a gray value: the kept-gray file writes
/// the composite, whose clear pixels hold 0, so the value is "hidden pixel
/// colors" even in a PNG, which keeps alpha. A JPEG, which keeps none,
/// reopens opaque, which "pixel transparency" names.
#[test]
fn clear_pixels_lose_their_colour_and_a_jpeg_its_transparency() {
    let dir = TempDir::new().unwrap();
    let mut pixels = opaque();
    pixels[0] = [30000, 30000, 30000, 0];
    let doc = gray(pixels);
    let stored = unsafe { &*doc }.root_layers()[0]
        .source()
        .unwrap()
        .rgba16_at(0, 0)
        .unwrap();
    assert!(
        stored[3] == 0 && stored[0] != 0,
        "fixture keeps the clear pixel's gray: {stored:?}"
    );
    assert_eq!(losses(doc, true), vec!["hidden pixel colors".to_string()]);
    assert_eq!(
        losses(doc, false),
        vec![
            "pixel transparency".to_string(),
            "hidden pixel colors".to_string()
        ]
    );
    let png = gray_round_trip(doc, "png", &dir);
    let reopened = unsafe { &*png }.root_layers()[0]
        .source()
        .unwrap()
        .rgba16_at(0, 0)
        .unwrap();
    assert_eq!(reopened[3], 0, "PNG keeps the transparency");
    assert_eq!(reopened[0], 0, "PNG drops the clear pixel's gray");
    free(png);
    let jpeg = gray_round_trip(doc, "jpeg", &dir);
    let reopened = unsafe { &*jpeg }.root_layers()[0]
        .source()
        .unwrap()
        .rgba16_at(0, 0)
        .unwrap();
    assert_eq!(reopened[3], 65535, "JPEG drops the transparency");
    free(jpeg);
    free(doc);
}

#[test]
fn a_null_document_is_refused() {
    unsafe {
        assert!(dr_precision_doc_flat_state_losses(ptr::null(), true, ptr::null_mut()).is_null());
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_flat_state_losses(ptr::null(), false, &mut err).is_null());
        assert!(take_err_string(err).contains("NULL"));
    }
}
