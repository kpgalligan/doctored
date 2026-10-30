//! Flat-save protection through the C contract. Each fixture changes one
//! independent persisted property of a plain, otherwise saveable document.

use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, LayerKind, LayerRaster};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::dr_doc_save_image;
use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_preservation::dr_doc_flat_save_losses;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::Resolution;
use doctored_core::style::LayerStyle;
use image::{GrayImage, Rgba, RgbaImage};

use crate::common;
use common::take_err_string;

fn exported(doc: &DrDocument, format: i32, strip: bool) -> DrDocument {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("explicit-export");
    let cpath = common::cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_doc_save_image(
                doc,
                ptr::null(),
                cpath.as_ptr(),
                format,
                100,
                true,
                strip,
                ptr::null_mut(),
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    DrDocument::open(path.to_str().unwrap()).unwrap()
}

/// Minimal independent TIFF stream: X/Y RATIONALs and an inch unit, with no
/// orientation, thumbnail or other capture fields. The writer patches the
/// two initial 72/1 rationals to the document's fractional resolution.
fn density_exif() -> Arc<[u8]> {
    let mut bytes = b"II\x2a\x00\x08\x00\x00\x00\x03\x00".to_vec();
    for (tag, kind, value) in [(0x11au16, 5u16, 50u32), (0x11b, 5, 58), (0x128, 3, 2)] {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for value in [72u32, 1, 72, 1] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Arc::from(bytes)
}

fn plain() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(3, 2, Rgba([23, 91, 166, 255])))
}

fn losses(doc: &DrDocument, format: i32, embed: bool, strip: bool) -> Vec<String> {
    unsafe {
        let mut err = ptr::null_mut();
        let json = dr_doc_flat_save_losses(doc, format, embed, strip, &mut err);
        assert!(!json.is_null(), "{}", take_err_string(err));
        assert!(err.is_null());
        let result = serde_json::from_str(CStr::from_ptr(json).to_str().unwrap()).unwrap();
        dr_string_free(json);
        result
    }
}

#[test]
fn ordinary_background_remains_saveable_in_flat_formats() {
    for format in 0..=5 {
        assert!(losses(&plain(), format, true, false).is_empty());
    }
}

#[test]
fn rich_single_layer_properties_require_native_preservation() {
    type Case = (&'static str, fn(&mut DrDocument));
    let cases: &[Case] = &[
        ("layer masks", |d| {
            d.layers[0].mask = Some(Arc::new(GrayImage::from_pixel(3, 2, image::Luma([255]))))
        }),
        ("layer masks", |d| d.layers[0].mask_enabled = false),
        ("editable layer descriptions", |d| {
            d.layers[0].meta = Some("{\"future\":true}".into())
        }),
        ("layer styles", |d| {
            d.layers[0].style = Some(Arc::new(LayerStyle::default()))
        }),
        ("layer properties", |d| d.layers[0].name = "Retouch".into()),
        ("layer properties", |d| d.layers[0].opacity = 0.5),
        ("layer properties", |d| d.layers[0].visible = false),
        ("layer properties", |d| {
            d.layers[0].blend = BlendMode::Multiply
        }),
        ("layer properties", |d| d.layers[0].clipped = true),
        ("layer properties", |d| d.layers[0].locks = 1),
        ("layer properties", |d| d.layers[0].link = 1),
        ("layer bounds and off-canvas pixels", |d| {
            d.layers[0].offset = (-1, 0)
        }),
        ("layer bounds and off-canvas pixels", |d| d.width = 2),
        ("guides and ruler origin", |d| d.ruler_origin = (1.0, 0.0)),
        ("global light settings", |d| d.global_light.angle = 45.0),
        ("layers and groups", |d| d.layers[0].kind = LayerKind::Group),
    ];
    for &(expected, change) in cases {
        let mut doc = plain();
        change(&mut doc);
        for format in 0..=5 {
            assert!(
                losses(&doc, format, true, false)
                    .iter()
                    .any(|s| s == expected),
                "{expected}, format {format}"
            );
        }
    }
}

#[test]
fn masks_descriptions_channels_and_guides_still_survive_native_reopen() {
    let doc = plain()
        .add_mask(0, doctored_core::doc::MaskKind::RevealAll)
        .unwrap();
    let mut doc = doc;
    doc.layers[0].meta = Some("{\"type\":\"future\",\"text\":\"keep me\"}".into());
    let doc = doc
        .add_channel(
            "Subject",
            &[0, 32, 127, 255, 128, 0],
            3,
            2,
            [255, 0, 0],
            0.5,
        )
        .unwrap();
    let doc = doc
        .add_guide(doctored_core::doc_guide::GuideOrientation::Vertical, 1.0)
        .unwrap();
    assert!(losses(&doc, 0, true, false).contains(&"saved selection channels".into()));
    assert!(losses(&doc, 0, true, false).contains(&"guides and ruler origin".into()));
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rich.rz");
    doc.save_native(path.to_str().unwrap()).unwrap();
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert_eq!(reopened.layers[0].meta, doc.layers[0].meta);
    assert_eq!(reopened.layers[0].mask, doc.layers[0].mask);
    assert_eq!(reopened.channels[0].data, doc.channels[0].data);
    assert_eq!(reopened.guides[0].position, 1.0);
    assert_eq!(
        losses(&doc, 0, true, false),
        losses(&reopened, 0, true, false)
    );
}

#[test]
fn layer_comps_require_native_preservation() {
    let request = CString::new(r#"{"op":"layer_comp","action":"save","name":"A"}"#).unwrap();
    let mut err = ptr::null_mut();
    let edited = unsafe { dr_doc_edit_layer_comps(&plain(), request.as_ptr(), &mut err) };
    assert!(!edited.is_null(), "{}", take_err_string(err));
    let doc = unsafe { (*edited).clone() };
    unsafe { dr_doc_free(edited) };
    for format in 0..=5 {
        assert_eq!(
            losses(&doc, format, true, false)
                .iter()
                .filter(|s| *s == "layer comps")
                .count(),
            1,
            "format {format}"
        );
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("comps.rz");
    doc.save_native(path.to_str().unwrap()).unwrap();
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        losses(&doc, 0, true, false),
        losses(&reopened, 0, true, false)
    );
}

#[test]
fn profile_density_packets_and_export_options_affect_preservation() {
    let mut doc = plain();
    doc.profile = IccProfile::display_p3();
    doc.resolution = Resolution { x: 300.0, y: 300.0 };
    assert!(losses(&doc, 0, true, false).is_empty());
    assert!(losses(&doc, 1, true, false).is_empty());
    assert!(losses(&doc, 0, false, false).contains(&"color profile".into()));
    assert!(losses(&doc, 2, true, false).is_empty());
    let tiff = exported(&doc, 2, false);
    assert_eq!(tiff.profile.bytes(), doc.profile.bytes());
    assert_eq!(tiff.resolution, doc.resolution);

    doc.metadata.xmp = Some(Arc::from(b"<x:xmpmeta>capture</x:xmpmeta>".as_slice()));
    assert!(losses(&doc, 0, true, false).is_empty());
    assert!(losses(&doc, 0, true, true).contains(&"XMP metadata".into()));
    doc.metadata.xmp = Some(Arc::from(vec![b'x'; 70_000]));
    assert!(losses(&doc, 1, true, false).contains(&"XMP metadata".into()));
    assert!(!losses(&doc, 0, true, false).contains(&"XMP metadata".into()));
    doc.metadata.exif = Some(Arc::from(b"malformed".as_slice()));
    assert!(losses(&doc, 0, true, false).contains(&"EXIF metadata".into()));
}

#[test]
fn transparency_is_checked_separately_from_editing_state() {
    let mut doc = plain();
    let mut img = doc.layers[0].pixels.to_contiguous().unwrap();
    img.get_pixel_mut(0, 0)[3] = 100;
    doc.layers[0].pixels = LayerRaster::new(img);
    assert!(losses(&doc, 0, true, false).is_empty());
    assert_eq!(losses(&doc, 1, true, false), ["pixel transparency"]);
    assert_eq!(losses(&doc, 4, true, false), ["pixel transparency"]);
}

#[test]
fn hidden_colors_require_native_even_when_a_flat_format_carries_alpha() {
    let mut doc = plain();
    let mut img = doc.layers[0].pixels.to_contiguous().unwrap();
    img.get_pixel_mut(0, 0).0 = [251, 17, 180, 0];
    doc.layers[0].pixels = LayerRaster::new(img);
    for format in 0..=5 {
        assert!(losses(&doc, format, true, false).contains(&"hidden pixel colors".into()));
    }
    // Explicit Export is still allowed and demonstrates why Save must warn.
    let flat = exported(&doc, 0, false);
    assert_eq!(
        flat.layers[0].pixels.contiguous_image().get_pixel(0, 0).0,
        [0, 0, 0, 0]
    );
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("hidden-colors.rz");
    doc.save_native(path.to_str().unwrap()).unwrap();
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        *reopened.layers[0].pixels.contiguous_image(),
        *doc.layers[0].pixels.contiguous_image()
    );

    let mut img = doc.layers[0].pixels.to_contiguous().unwrap();
    img.get_pixel_mut(0, 0).0 = [0, 0, 0, 0];
    doc.layers[0].pixels = LayerRaster::new(img);
    assert!(losses(&doc, 0, true, false).is_empty());
}

#[test]
fn density_preservation_uses_written_precision_and_normalized_exif() {
    let mut doc = plain();
    doc.resolution = Resolution {
        x: 300.25,
        y: 150.5,
    };
    assert!(losses(&doc, 1, true, false).contains(&"print resolution".into()));
    let jpeg = exported(&doc, 1, false);
    assert_eq!(jpeg.resolution, Resolution { x: 300.0, y: 151.0 });

    // PNG's inherent pixels-per-metre rounding is permitted at <= .013 ppi.
    assert!(losses(&doc, 0, true, false).is_empty());
    let png = exported(&doc, 0, false);
    assert!((png.resolution.x - doc.resolution.x).abs() <= 0.013);
    assert!((png.resolution.y - doc.resolution.y).abs() <= 0.013);

    doc.metadata.exif = Some(density_exif());
    assert!(losses(&doc, 1, true, false).is_empty());
    assert_eq!(exported(&doc, 1, false).resolution, doc.resolution);
    assert!(losses(&doc, 1, true, true).contains(&"print resolution".into()));
    assert_eq!(exported(&doc, 1, true).resolution, jpeg.resolution);

    // Merely retaining an EXIF packet is insufficient; it must state density.
    doc.metadata.exif = Some(Arc::from(
        b"II\x2a\x00\x08\x00\x00\x00\0\0\0\0\0\0".as_slice(),
    ));
    let reported = losses(&doc, 1, true, false);
    assert!(!reported.contains(&"EXIF metadata".into()));
    assert!(reported.contains(&"print resolution".into()));

    doc.metadata.exif = None;
    doc.resolution = Resolution {
        x: 300.01,
        y: 150.99,
    };
    assert!(losses(&doc, 1, true, false).is_empty());
    doc.resolution.x = 300.02;
    assert!(losses(&doc, 1, true, false).contains(&"print resolution".into()));

    doc.resolution = Resolution { x: 72.01, y: 72.0 };
    assert!(losses(&doc, 3, true, false).contains(&"print resolution".into()));

    // The inherited reader interprets JFIF's 1x1 as "unstated", not one ppi.
    doc.resolution = Resolution { x: 1.0, y: 1.0 };
    assert!(losses(&doc, 1, true, false).contains(&"print resolution".into()));
    assert_eq!(exported(&doc, 1, false).resolution, Resolution::default());
}

#[test]
fn a_custom_srgb_profile_requires_its_authored_bytes_to_survive() {
    let mut bytes = IccProfile::srgb().bytes().to_vec();
    // Alter only the optional manufacturer signature, outside the color model.
    bytes[48..52].copy_from_slice(b"TEST");
    let mut doc = plain();
    doc.profile = Arc::new(IccProfile::parse(&bytes).unwrap());
    assert!(doc.profile.describes_srgb());
    assert!(!doc.profile.is_builtin_srgb());
    assert!(losses(&doc, 0, true, false).is_empty());
    assert_eq!(
        exported(&doc, 0, false).profile.bytes(),
        doc.profile.bytes()
    );
    assert!(losses(&doc, 0, false, false).contains(&"color profile".into()));
    assert!(losses(&doc, 2, true, false).is_empty());
    assert_eq!(
        exported(&doc, 2, false).profile.bytes(),
        doc.profile.bytes()
    );
    for format in [3, 4] {
        assert!(losses(&doc, format, true, false).contains(&"color profile".into()));
    }
}

#[test]
fn null_safety_sweep_and_invalid_format() {
    unsafe {
        assert!(dr_doc_flat_save_losses(ptr::null(), 0, true, false, ptr::null_mut()).is_null());
        let mut err = ptr::null_mut();
        assert!(dr_doc_flat_save_losses(&plain(), 999, true, false, &mut err).is_null());
        assert!(take_err_string(err).contains("format"));
    }
}
