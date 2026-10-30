//! Exact whole-canvas operations return no result when every stored field is
//! identical, and still report a change for latent raw state.

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::doc_guide::GuideOrientation;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::tiff_float::{encode_to_vec, FloatTiff};
use image::{GrayImage, RgbaImage};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn centered_byte(pixel: [u8; 4]) -> DrDocument {
    let image = RgbaImage::from_pixel(3, 3, image::Rgba(pixel));
    let doc = DrDocument::from_pixels(image);
    doc.set_ruler_origin(1.5, 1.5).unwrap()
}

fn assert_every_byte_exact_changes(doc: &DrDocument, what: &str) {
    assert!(doc.rotate90().is_some(), "{what}: rotate90");
    assert!(doc.rotate180().is_some(), "{what}: rotate180");
    assert!(doc.rotate270().is_some(), "{what}: rotate270");
    assert!(doc.flip_horizontal().is_some(), "{what}: flip horizontal");
    assert!(doc.flip_vertical().is_some(), "{what}: flip vertical");
}

#[test]
fn byte_symmetric_exact_ops_are_none() {
    let doc = centered_byte([20, 40, 60, 255]);
    assert!(doc.rotate90().is_none());
    assert!(doc.rotate180().is_none());
    assert!(doc.rotate270().is_none());
    assert!(doc.flip_horizontal().is_none());
    assert!(doc.flip_vertical().is_none());
}

#[test]
fn byte_every_independent_stored_state_control_is_observed() {
    let hidden = centered_byte([7, 9, 11, 0]);
    let mut pixels = hidden.layers[0].pixels.to_contiguous().unwrap();
    pixels.get_pixel_mut(0, 0).0 = [99, 1, 2, 0];
    let hidden = hidden.with_layer_pixels(0, pixels).unwrap();
    assert_every_byte_exact_changes(&hidden, "hidden RGB is stored state");

    let mask = GrayImage::from_fn(3, 3, |x, y| {
        image::Luma([if x == 0 && y == 0 { 1 } else { 0 }])
    });
    let mut masked = centered_byte([20, 40, 60, 255])
        .add_mask(0, MaskKind::FromSelection(mask.as_raw()))
        .unwrap();
    masked.layers[0].mask_enabled = false;
    masked.layers[0].mask_properties.offset = (-1, 1);
    for (what, result) in [
        ("rotate90", masked.rotate90()),
        ("rotate180", masked.rotate180()),
        ("rotate270", masked.rotate270()),
        ("flip horizontal", masked.flip_horizontal()),
        ("flip vertical", masked.flip_vertical()),
    ] {
        let result = result.unwrap_or_else(|| panic!("mask plane/properties: {what}"));
        assert!(!result.layers[0].mask_enabled, "{what}: enabled state");
        assert!(
            result.layers[0].mask_properties != masked.layers[0].mask_properties,
            "{what}: independent mask properties"
        );
    }

    let offset = centered_byte([20, 40, 60, 255])
        .with_layer_offset(0, 1, 1)
        .unwrap();
    assert_every_byte_exact_changes(&offset, "layer offset");

    let channel = centered_byte([20, 40, 60, 255])
        .add_channel(
            "Corner",
            &[1, 0, 0, 0, 0, 0, 0, 0, 0],
            3,
            3,
            [255, 0, 0],
            0.5,
        )
        .unwrap();
    assert_every_byte_exact_changes(&channel, "saved channel");

    let guided = centered_byte([20, 40, 60, 255])
        .add_guide(GuideOrientation::Vertical, 0.5)
        .unwrap()
        .add_guide(GuideOrientation::Horizontal, 0.75)
        .unwrap();
    assert_every_byte_exact_changes(&guided, "guide chrome");

    let origin =
        DrDocument::from_pixels(RgbaImage::from_pixel(3, 3, image::Rgba([20, 40, 60, 255])))
            .set_ruler_origin(0.25, 0.75)
            .unwrap();
    assert_every_byte_exact_changes(&origin, "ruler origin");

    // Descriptions are invariant input state for the raw core operation.
    // A changed pixel control proves each result carries the opaque blob;
    // host composition of that blob is covered by geometry-noops-ui.
    let mut described = hidden.clone();
    described.layers[0].meta = Some(r#"{"type":"shape","version":3}"#.into());
    for (what, result) in [
        ("rotate90", described.rotate90()),
        ("rotate180", described.rotate180()),
        ("rotate270", described.rotate270()),
        ("flip horizontal", described.flip_horizontal()),
        ("flip vertical", described.flip_vertical()),
    ] {
        let result = result.unwrap_or_else(|| panic!("described control: {what}"));
        assert_eq!(result.layers[0].meta, described.layers[0].meta, "{what}");
    }
}

fn typed_uniform() -> PrecisionDocument {
    let samples = vec![[4_000, 8_000, 12_000, 65_535]; 9];
    let image = PrecisionImage::from_rgba16(3, 3, samples, IccProfile::srgb(), None).unwrap();
    let doc = PrecisionDocument::from_parts(
        3,
        3,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution { x: 72.0, y: 72.0 },
        vec![Layer::raster("Uniform", image, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap();
    doc.set_ruler_origin(1.5, 1.5).unwrap()
}

#[test]
fn native_symmetric_exact_ops_are_none_and_a_raw_change_is_not() {
    let doc = typed_uniform();
    for edit in [
        GeometryEdit::Rotate90,
        GeometryEdit::Rotate180,
        GeometryEdit::Rotate270,
        GeometryEdit::FlipHorizontal,
        GeometryEdit::FlipVertical,
    ] {
        assert!(doc.geometry(&edit, LIMIT, 0, None).unwrap().is_none());
    }

    let samples = vec![
        [1, 2, 3, 65_535],
        [4, 5, 6, 65_535],
        [7, 8, 9, 65_535],
        [10, 11, 12, 65_535],
        [13, 14, 15, 65_535],
        [16, 17, 18, 65_535],
        [19, 20, 21, 65_535],
        [22, 23, 24, 65_535],
        [25, 26, 27, 65_535],
    ];
    let image = PrecisionImage::from_rgba16(3, 3, samples, IccProfile::srgb(), None).unwrap();
    let changed = PrecisionDocument::from_parts(
        3,
        3,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution { x: 72.0, y: 72.0 },
        vec![Layer::raster("Changed", image, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap()
    .set_ruler_origin(1.5, 1.5)
    .unwrap();
    assert!(changed
        .geometry(&GeometryEdit::Rotate180, LIMIT, 0, None)
        .unwrap()
        .is_some());
}

#[test]
fn float_signed_zero_bits_are_compared() {
    fn float_doc(negative_at: &[usize]) -> PrecisionDocument {
        let mut samples = vec![0.0_f32; 3 * 3 * 4];
        for pixel in samples.chunks_exact_mut(4) {
            pixel[3] = 1.0;
        }
        for &pixel in negative_at {
            samples[pixel * 4] = -0.0;
        }
        let tiff = FloatTiff {
            width: 3,
            height: 3,
            channels: 4,
            alpha: Some(false),
            samples,
            icc: None,
            resolution: None,
            metadata: Metadata::default(),
            non_finite: 0,
        };
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("signed-zero.tiff");
        std::fs::write(&path, encode_to_vec(&tiff, false).unwrap()).unwrap();
        PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
            .unwrap()
            .set_ruler_origin(1.5, 1.5)
            .unwrap()
    }

    // Numerically these rasters are identical under the flip. Bitwise, the
    // lone -0 moves from the top-left to the top-right and must be a change.
    let changed = float_doc(&[0]);
    assert_eq!(
        changed.layer_source(0).unwrap().to_rgba32().ok().unwrap()[0][0].to_bits(),
        (-0.0_f32).to_bits()
    );
    let result = changed
        .geometry(&GeometryEdit::FlipHorizontal, LIMIT, 0, None)
        .unwrap()
        .expect("signed-zero movement is stored-state movement");
    assert_eq!(
        result.layer_source(0).unwrap().to_rgba32().ok().unwrap()[2][0].to_bits(),
        (-0.0_f32).to_bits()
    );

    // Matching sign bits at every symmetry-related corner are a real no-op,
    // which guards against treating every zero-bearing float raster as changed.
    let unchanged = float_doc(&[0, 2, 6, 8]);
    for edit in [
        GeometryEdit::Rotate90,
        GeometryEdit::Rotate180,
        GeometryEdit::Rotate270,
        GeometryEdit::FlipHorizontal,
        GeometryEdit::FlipVertical,
    ] {
        assert!(unchanged.geometry(&edit, LIMIT, 0, None).unwrap().is_none());
    }
}
