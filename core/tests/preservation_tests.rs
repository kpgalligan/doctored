//! Flat-save protection through the C contract. Each fixture changes one
//! independent persisted property of a plain, otherwise saveable document.
//! The pixel lines' scans run once per raster (`alpha_scans_pending`), and a
//! written raster answers for its own bytes while the handle it came from
//! keeps its answer.

use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, LayerKind, LayerRaster};
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_string_free};
use doctored_core::ffi_color::dr_doc_save_image;
use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_with_layer_name, dr_doc_with_layer_pixels};
use doctored_core::ffi_evaluation::{dr_doc_evaluation, dr_doc_set_evaluation};
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
                std::ptr::null(),
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
        ("layer properties", |d| d.layers[0].label = 3),
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

/// The colour label is persisted state a flat file has no place for: the
/// list names it, and the flat file reopens unlabelled.
#[test]
fn a_colour_label_is_lost_by_every_flat_format() {
    let mut doc = plain();
    doc.layers[0].label = 3;
    for format in 0..=5 {
        assert_eq!(
            losses(&doc, format, true, false),
            vec!["layer properties".to_string()],
            "format {format}"
        );
    }
    assert_eq!(exported(&doc, 0, false).layers[0].label, 0);
}

/// The linear-light evaluation policy is persisted state a flat file has no
/// place for: under it every flat format names it, the file reopens
/// `encoded`, and back under `encoded` the list is empty again.
#[test]
fn a_linear_light_policy_is_lost_by_every_flat_format() {
    let linear = unsafe {
        let mut err = ptr::null_mut();
        let out = dr_doc_set_evaluation(&plain(), Evaluation::LinearLight.code(), &mut err);
        assert!(!out.is_null(), "{}", take_err_string(err));
        Box::from_raw(out)
    };
    assert_eq!(
        unsafe { dr_doc_evaluation(&*linear) },
        Evaluation::LinearLight.code()
    );
    for format in 0..=5 {
        assert_eq!(
            losses(&linear, format, true, false),
            vec!["linear-light evaluation".to_string()],
            "format {format}"
        );
    }
    let reopened = exported(&linear, 0, false);
    assert_eq!(
        unsafe { dr_doc_evaluation(&reopened) },
        Evaluation::Encoded.code()
    );
    assert!(losses(&reopened, 0, true, false).is_empty());
    let back = unsafe {
        let mut err = ptr::null_mut();
        let out = dr_doc_set_evaluation(&*linear, Evaluation::Encoded.code(), &mut err);
        assert!(!out.is_null(), "{}", take_err_string(err));
        Box::from_raw(out)
    };
    assert!(losses(&back, 0, true, false).is_empty());
}

/// `plain()` with two more opaque layers named "Background", each its own
/// raster, so its plain list is "layers and groups" alone.
fn three_layers() -> *mut DrDocument {
    let mut doc = plain();
    for _ in 0..2 {
        doc = doc
            .adding_layer(doc.layers.len() - 1, "Background")
            .unwrap();
        let top = doc.layers.len() - 1;
        doc = doc
            .with_layer_pixels(top, RgbaImage::from_pixel(3, 2, Rgba([23, 91, 166, 255])))
            .unwrap();
    }
    Box::into_raw(Box::new(doc))
}

/// Layer 1's pixels with pixel (0, 0) at alpha 0 holding a colour, through
/// the FFI's pixel write.
fn hide_one(doc: *const DrDocument) -> *mut DrDocument {
    let mut bytes = [23u8, 91, 166, 255].repeat(6);
    bytes[..4].copy_from_slice(&[251, 17, 180, 0]);
    unsafe {
        let image = dr_image_from_rgba(bytes.as_ptr(), 3, 2);
        assert!(!image.is_null());
        let out = dr_doc_with_layer_pixels(doc, 1, image);
        dr_image_free(image);
        assert!(!out.is_null(), "the pixel write changed nothing");
        out
    }
}

/// The pixel scans behind the list run once per raster, not once per call:
/// one list leaves no raster pending, a pixel write leaves only the raster it
/// wrote, and a rename or the first handle leaves none.
#[test]
fn each_raster_is_scanned_once_and_a_pixel_write_rescans_only_its_own() {
    let doc = three_layers();
    let pending = |d: *const DrDocument| unsafe { &*d }.alpha_scans_pending();
    assert_eq!(pending(doc), 3, "a fresh document");
    assert_eq!(
        losses(unsafe { &*doc }, 1, true, false),
        ["layers and groups"]
    );
    assert_eq!(pending(doc), 0, "one list scans every raster");
    let hidden = hide_one(doc);
    assert_eq!(pending(hidden), 1, "the write made one raster");
    assert_eq!(pending(doc), 0, "the first handle keeps its answers");
    let name = CString::new("Retouch").unwrap();
    let renamed = unsafe { dr_doc_with_layer_name(doc, 2, name.as_ptr()) };
    assert!(!renamed.is_null());
    assert_eq!(pending(renamed), 0, "a rename shares every raster");
    unsafe {
        dr_doc_free(renamed);
        dr_doc_free(hidden);
        dr_doc_free(doc);
    }
}

/// A raster's answer is its own bytes': the written handle names the hidden
/// colour everywhere and the transparency in JPEG, and none in GIF, whose
/// palette keeps alpha 0, so all three facts are kept apart; the handle it
/// came from, asked again, names neither.
#[test]
fn a_written_raster_answers_for_its_own_bytes() {
    let doc = three_layers();
    let hidden = hide_one(doc);
    let (png, jpeg, gif) = (0, 1, 4);
    let lists = |d: *const DrDocument| {
        let d = unsafe { &*d };
        [png, jpeg, gif].map(|format| losses(d, format, true, false))
    };
    assert_eq!(
        lists(hidden),
        [
            vec!["layers and groups", "hidden pixel colors"],
            vec![
                "layers and groups",
                "pixel transparency",
                "hidden pixel colors"
            ],
            vec!["layers and groups", "hidden pixel colors"],
        ]
    );
    assert_eq!(lists(doc), [["layers and groups"]; 3], "the first handle");
    unsafe {
        dr_doc_free(hidden);
        dr_doc_free(doc);
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

/// The byte list's wall-clock record on a 24 MP document of four opaque
/// layers whose bytes vary per pixel: the first call, the rest of one
/// `get_document`'s 23 calls (the six flat formats and the three
/// host-encoded ones, then each `writableTypes` call's six), and File ▸
/// Save's 9 on a freshly built document.
mod bench {
    use std::time::{Duration, Instant};

    use doctored_core::doc::{DrDocument, LayerRaster};
    use image::RgbaImage;

    use super::losses;

    const W: u32 = 6000;
    const H: u32 = 4000;
    const LAYERS: u32 = 4;
    /// Calls 2 to 23 on an unchanged document read no pixel, declared
    /// before measuring.
    const REST_BOUND: Duration = Duration::from_millis(5);

    /// Opaque bytes varying per pixel, seeded by `seed`.
    fn opaque_varied(seed: u32) -> RgbaImage {
        let bytes = (0..W * H)
            .flat_map(|i| {
                let j = i
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(seed.wrapping_mul(40_503));
                [j as u8, (j >> 8) as u8, (j >> 16) as u8, u8::MAX]
            })
            .collect();
        RgbaImage::from_raw(W, H, bytes).unwrap()
    }

    fn document() -> DrDocument {
        let mut doc = DrDocument::from_pixels(RgbaImage::new(W, H));
        doc.layers[0].pixels = LayerRaster::unseeded(opaque_varied(0));
        for k in 1..LAYERS {
            doc = doc
                .adding_layer(doc.layers.len() - 1, "Background")
                .unwrap();
            let top = doc.layers.len() - 1;
            doc.layers[top].pixels = LayerRaster::unseeded(opaque_varied(k));
        }
        doc
    }

    /// Each of `calls` lists of `doc` timed, cycling through PNG to WebP.
    fn timed(doc: &DrDocument, calls: usize) -> Vec<Duration> {
        (0..calls)
            .map(|i| {
                let began = Instant::now();
                losses(doc, (i % 6) as i32, true, false);
                began.elapsed()
            })
            .collect()
    }

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn byte_lists_after_the_first_read_no_pixel() {
        let times = timed(&document(), 23);
        let rest: Duration = times[1..].iter().sum();
        let save: Duration = timed(&document(), 9).iter().sum();
        eprintln!(
            "flat-save list, 8-bit RGB, {W}x{H}, {LAYERS} opaque layers: first call {:?}; \
             calls 2..23 {rest:?}; get_document's 23 {:?}; Save's 9 on a fresh document \
             {save:?}",
            times[0],
            times[0] + rest,
        );
        assert!(
            rest <= REST_BOUND,
            "calls 2..23 took {rest:?}, over {REST_BOUND:?}"
        );
    }
}
