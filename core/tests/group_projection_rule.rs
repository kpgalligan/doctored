//! Every read of a GROUP as a layer — `layer_canvas_image`, a
//! layer-scoped analysis, a group Copy — uses ONE rule at every depth: the
//! group's RAW projection, its children composited among themselves
//! WITHOUT the group's own opacity, blend mode, mask, style, Blend If or
//! clipping, visibility forced on (`raw_group_document` at the typed
//! depths, `group_projection` at 8 bits).
//!
//! The fixture's group carries every one of those properties at once —
//! opacity 0.5, Screen (Multiply at float), a mask, a layer style (none at
//! float), Blend If, clipped to a base raster under it — over a background,
//! so a read that kept any of them would differ. The oracle is independent of the read under test: a SECOND
//! document holding only the same children in an isolated Normal group at
//! opacity 1, visible, with nothing under it (`composite_only`), read by its
//! ordinary composite.
//!
//! Tolerances, fixed before anything was run: the typed `layer_canvas_image`
//! equals the oracle's render sample for sample at u16 and bit for bit at
//! float; the layer-scoped histogram equals the oracle's composite histogram
//! exactly; a group Copy matches Copy Merged of the oracle within 1 code at
//! u16 (the tolerance `precision_clipboard_group.rs` fixed); the byte
//! `dr_doc_layer_canvas_image` equals the byte oracle's flatten exactly.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, LayerKind, MaskKind};
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_doc::dr_doc_layer_canvas_image;
use doctored_core::ffi_precision::{dr_precision_doc_layer_canvas_image, dr_precision_image_free};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{ClipboardPayload, ClipboardRead};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::scoped_statistics::AnalysisRequest;
use doctored_core::style::LayerStyle;
use doctored_core::tiff_float::FloatTiff;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 7;
const H: u32 = 6;
const COVERAGE: [u16; 5] = [0, 1, 32768, 65534, 65535];

/// A style that changes a group's composite: a drop shadow and Blend If.
const STYLE: &str = r##"{"blend_if":{"channel":"gray","this_layer":[10,40,200,240],
    "underlying":[0,0,255,255]},"effects":[{"type":"drop_shadow","use_global_light":false,
    "angle":45,"distance":2,"size":1,"spread":0,"opacity":0.8,"color":"#102030"}]}"##;

// ------------------------------------------------------------- fixtures --

/// A deterministic u16 pattern with sub-byte codes and partial alpha.
fn pattern(w: u32, h: u32, seed: u32) -> Vec<[u16; 4]> {
    let codes = [0x0101u16, 0x0100, 1, 65534, 0, 65535, 12345, 40000];
    (0..w * h)
        .map(|i| {
            let k = (i + seed) as usize;
            [
                codes[k % 8],
                codes[(k + 3) % 8],
                codes[(k + 5) % 8],
                codes[(k * 7 + seed as usize) % 8],
            ]
        })
        .collect()
}

fn u16_image(w: u32, h: u32, seed: u32, profile: &Arc<IccProfile>) -> PrecisionImage {
    PrecisionImage::from_rgba16(w, h, pattern(w, h, seed), profile.clone(), None).expect("image")
}

/// The group's children, bottom first: a base, a raster clipped to it at an
/// offset, and a Multiply raster at 0.7.
fn children(profile: &Arc<IccProfile>) -> Vec<Layer> {
    let base = Layer::raster("Base", u16_image(6, 5, 1, profile), (0, 0), 1.0, true).unwrap();
    let clipped = Layer::raster("Clipped", u16_image(4, 4, 2, profile), (2, 1), 1.0, true)
        .unwrap()
        .with_clipped(true);
    let multiply = Layer::raster("Multiply", u16_image(W, H, 3, profile), (0, 0), 0.7, true)
        .unwrap()
        .with_blend_mode(BlendMode::Multiply)
        .unwrap();
    vec![base, clipped, multiply]
}

fn document(layers: Vec<Layer>, profile: Arc<IccProfile>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        profile,
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .expect("document")
}

fn group_mask() -> PrecisionMask {
    let plane: Vec<u16> = (0..W * H).map(|i| COVERAGE[i as usize % 5]).collect();
    PrecisionMask::new(
        PrecisionPlane::from_u16(W, H, plane, None).unwrap(),
        Default::default(),
        true,
    )
    .unwrap()
}

/// Postorder: Background 0, Clip base 1, Base 2, Clipped 3, Multiply 4, G 5.
/// G is at 0.5, Screen, masked, styled with Blend If and clipped to 1.
fn styled_document(profile: &Arc<IccProfile>) -> PrecisionDocument {
    let style = LayerStyle::from_json(STYLE).expect("the style");
    let group = Layer::isolated_normal("G", children(profile), 0.5, true)
        .unwrap()
        .with_blend_mode(BlendMode::Screen)
        .unwrap()
        .with_mask(Some(group_mask()))
        .with_layer_style(Some(&style), (W, H))
        .expect("the group takes the style")
        .with_clipped(true);
    let background =
        Layer::raster("Background", u16_image(W, H, 4, profile), (0, 0), 1.0, true).unwrap();
    let clip_base =
        Layer::raster("Clip base", u16_image(5, 4, 6, profile), (1, 1), 1.0, true).unwrap();
    let doc = document(vec![background, clip_base, group], profile.clone());
    let g = doc.layer(5).unwrap();
    assert!(g.children().is_some() && g.clipped() && g.blend_if().is_some());
    doc
}

/// The oracle: the same children in an isolated Normal group at opacity 1,
/// visible, with nothing under it.
fn composite_only(profile: &Arc<IccProfile>) -> PrecisionDocument {
    let group = Layer::isolated_normal("Composite", children(profile), 1.0, true).unwrap();
    document(vec![group], profile.clone())
}

// ------------------------------------------------------------------ FFI --

unsafe fn take_error(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    text
}

/// `dr_precision_doc_layer_canvas_image`, owned.
fn layer_canvas(doc: &PrecisionDocument, index: usize) -> PrecisionImage {
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_layer_canvas_image(doc, index, LIMIT, 0, ptr::null(), &mut error)
    };
    assert!(!out.is_null(), "{}", unsafe { take_error(error) });
    let image = unsafe { (*out).clone() };
    unsafe { dr_precision_image_free(out) };
    image
}

fn render(doc: &PrecisionDocument) -> PrecisionImage {
    doc.render(LIMIT, 0, None).expect("the oracle renders")
}

/// One typed edit through `dr_precision_doc_edit` that must change the
/// document.
fn edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", unsafe { take_error(err) });
    *unsafe { Box::from_raw(out) }
}

fn histogram(doc: &PrecisionDocument, request: Value) -> Value {
    let request = AnalysisRequest::parse(&request.to_string()).unwrap();
    let mut out = doc
        .scoped_analysis(&request, &[], LIMIT, 0, None)
        .expect("the histogram");
    // The scope names the request, which differs by construction.
    let object = out.as_object_mut().unwrap();
    object.remove("scope");
    object.remove("source");
    out
}

fn copy(layer: usize) -> ClipboardRead {
    serde_json::from_value(json!({"op":"copy","layer":layer,"x":0,"y":0,"width":W,"height":H}))
        .unwrap()
}

fn copy_merged() -> ClipboardRead {
    serde_json::from_value(json!({"op":"copy_merged","x":0,"y":0,"width":W,"height":H})).unwrap()
}

fn assert_within_one_code(got: &ClipboardPayload, want: &ClipboardPayload) {
    assert_eq!(got.storage(), PrecisionStorage::Rgba16Unorm);
    assert_eq!(got.offset(), want.offset());
    assert_eq!(got.dimensions(), want.dimensions());
    let got = got.image().to_rgba16().unwrap();
    let want = want.image().to_rgba16().unwrap();
    assert!(want.iter().any(|p| p[3] > 0), "the oracle is not empty");
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        for c in 0..4 {
            assert!(
                (i32::from(g[c]) - i32::from(w[c])).abs() <= 1,
                "pixel {i} ch{c}: {} vs {}",
                g[c],
                w[c]
            );
        }
    }
}

// ---------------------------------------------------------------- u16 --

#[test]
fn a_u16_group_canvas_image_is_its_raw_projection() {
    let srgb = IccProfile::srgb();
    let got = layer_canvas(&styled_document(&srgb), 5);
    let want = render(&composite_only(&srgb));
    assert_eq!(got.storage(), PrecisionStorage::Rgba16Unorm);
    let want = want.to_rgba16().unwrap();
    assert!(want.iter().any(|p| p[3] > 0), "the oracle is not empty");
    assert_eq!(got.to_rgba16().unwrap(), want);
}

#[test]
fn a_u16_group_histogram_is_its_raw_projections() {
    let srgb = IccProfile::srgb();
    let got = histogram(
        &styled_document(&srgb),
        json!({"operation":"histogram","source":"layer","layer":5}),
    );
    let want = histogram(
        &composite_only(&srgb),
        json!({"operation":"histogram","source":"composite"}),
    );
    assert_eq!(got, want);
}

#[test]
fn a_u16_group_copy_agrees_with_the_same_projection() {
    let srgb = IccProfile::srgb();
    let got = styled_document(&srgb)
        .clipboard_copy(&copy(5), None, LIMIT, 0, None)
        .expect("a group copies");
    let want = composite_only(&srgb)
        .clipboard_copy(&copy_merged(), None, LIMIT, 0, None)
        .expect("copy merged");
    assert_within_one_code(&got, &want);
}

// -------------------------------------------------------------- float --

fn float_document(width: u32, height: u32, samples: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples: samples.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("layer.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    doc
}

/// Extended-range straight samples with partial alpha.
fn extended(width: u32, height: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let v = ((x * 37 + y * 101) % 1000) as f32 / 1000.0;
            let a = match (x + 2 * y) % 5 {
                0 => 0.25,
                1 => 0.6,
                _ => 1.0,
            };
            [v * 6.0 - 1.0, 1.0 - v, ((x ^ y) % 97) as f32 / 24.0, a]
        })
        .collect()
}

#[test]
fn a_float_group_canvas_image_is_its_raw_projection_bit_for_bit() {
    // Background 0, its duplicate 1 grouped alone in G 2.
    let doc = float_document(W, H, &extended(W, H));
    let doubled = doc.duplicate_layer(0).unwrap().expect("duplicated");
    let (grouped, _) = doubled
        .group_layers(&[1], "G", false)
        .unwrap()
        .expect("grouped");
    assert!(grouped.layer(2).and_then(|l| l.children()).is_some());
    let oracle = grouped.remove_layers(&[0]).unwrap().expect("group only");

    // G at 0.5, Multiply (Screen is not a float mode), masked, with Blend
    // If, clipped to the background.
    let doc = grouped.set_layer_opacity(2, 0.5).unwrap().expect("opacity");
    let doc = edit(
        &doc,
        json!({"op":"layer_blend","version":1,"layer":2,"blend_mode":BlendMode::Multiply.to_c()}),
    );
    let doc = edit(
        &doc,
        json!({"op":"layer_blend_if","version":1,"layer":2,
            "blend_if":{"channel":"gray","this_layer":[10,40,200,240],"underlying":[0,0,255,255]}}),
    );
    let doc = edit(&doc, json!({"op":"layer_clipped","layer":2,"clipped":true}));
    let doc = doc
        .set_layer_mask(2, Some(group_mask()), LIMIT, 0, None)
        .unwrap()
        .expect("masked");
    let g = doc.layer(2).unwrap();
    assert!(g.clipped() && g.blend_if().is_some() && g.mask().is_some());

    let got = layer_canvas(&doc, 2);
    assert_eq!(got.storage(), PrecisionStorage::Rgba32Float);
    let bits = |image: &PrecisionImage| -> Vec<[u32; 4]> {
        image
            .to_rgba32()
            .unwrap()
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect()
    };
    assert_eq!(bits(&got), bits(&render(&oracle)));
}

// --------------------------------------------------------------- byte --

fn byte_layer(w: u32, h: u32, seed: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let k = x * 31 + y * 17 + seed * 53;
        let a = match (x + y + seed) % 4 {
            0 => 0,
            1 => 96,
            _ => 255,
        };
        Rgba([(k % 251) as u8, (k * 3 % 241) as u8, (k * 7 % 239) as u8, a])
    })
}

/// Bottom-first: `under` (bottom), then Base, Clipped (clipped to Base) and
/// Multiply at 0.7, grouped; the group's index is returned.
fn byte_grouped(under: Vec<(RgbaImage, (i32, i32))>) -> (DrDocument, usize) {
    let mut doc = DrDocument::from_pixels(RgbaImage::new(W, H));
    let mut layers = under;
    let first_child = layers.len();
    layers.push((byte_layer(6, 5, 1), (0, 0)));
    layers.push((byte_layer(4, 4, 2), (2, 1)));
    layers.push((byte_layer(W, H, 3), (0, 0)));
    for (i, (pixels, offset)) in layers.into_iter().enumerate() {
        doc = doc.adding_image_layer(i, pixels, "L").expect("a layer");
        doc.layers[i + 1].offset = offset;
    }
    // Drop the transparent canvas layer from_pixels made.
    let doc = doc.remove_layers(&[0]).expect("removed");
    let c = first_child;
    let doc = doc.with_layer_clipped(c + 1, true).expect("clipped");
    let doc = doc.with_layer_opacity(c + 2, 0.7).expect("opacity");
    let doc = doc
        .with_layer_blend_mode(c + 2, BlendMode::Multiply)
        .expect("multiply");
    let (doc, ..) = doc.group_layers(&[c, c + 1, c + 2], "G").expect("grouped");
    let group = doc
        .layers
        .iter()
        .position(|l| l.kind == LayerKind::Group)
        .expect("a group");
    (doc, group)
}

fn byte_canvas_image(doc: &DrDocument, idx: usize) -> RgbaImage {
    let image = unsafe { dr_doc_layer_canvas_image(doc, idx) };
    assert!(!image.is_null());
    unsafe {
        let (w, h) = (dr_image_width(image), dr_image_height(image));
        let bytes =
            std::slice::from_raw_parts(dr_image_pixels_rgba(image), (w * h * 4) as usize).to_vec();
        dr_image_free(image);
        RgbaImage::from_raw(w, h, bytes).unwrap()
    }
}

#[test]
fn a_byte_group_canvas_image_is_its_raw_projection() {
    let (doc, g) = byte_grouped(vec![
        (byte_layer(W, H, 4), (0, 0)),
        (byte_layer(5, 4, 6), (1, 1)),
    ]);
    let mask: Vec<u8> = (0..W * H)
        .map(|i| [0, 1, 128, 254, 255][i as usize % 5])
        .collect();
    let style = Arc::new(LayerStyle::from_json(STYLE).expect("the style"));
    let doc = doc.with_layer_opacity(g, 0.5).expect("opacity");
    let doc = doc
        .with_layer_blend_mode(g, BlendMode::Screen)
        .expect("screen");
    let doc = doc
        .add_mask(g, MaskKind::FromSelection(&mask))
        .expect("masked");
    let doc = doc.set_layer_style(g, Some(style)).expect("styled");
    let doc = doc.with_layer_clipped(g, true).expect("clipped");
    let group = &doc.layers[g];
    assert!(group.clipped && group.mask.is_some() && group.style.is_some());

    let (oracle, _) = byte_grouped(Vec::new());
    let want = oracle.flattened().expect("the oracle flattens");
    assert!(want.pixels().any(|p| p[3] > 0), "the oracle is not empty");
    assert_eq!(byte_canvas_image(&doc, g), want);
}
