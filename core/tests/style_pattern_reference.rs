//! The layer REFERENCE POINT (Photoshop's `fxrp`) and the AT-CREATION
//! anchor it gives a linked Pattern Overlay, at both depths: a move before
//! the style anchors where the layer was MADE (O = phase + the moves since),
//! every translation accumulates the point (an offset write, a move, a group
//! move, a pure-translation transform of a raster, a group record or a
//! retained source), a non-translation transform maps it by its matrix,
//! while Delete Cropped Pixels and the two placements (a paste, a
//! layer comp's restore) leave it, the
//! `.rz` word 91 / `LREF` round trip with its four refusals by name, and the
//! PSD `fxrp` both ways (an unstyled moved layer, Photoshop's own P5).
//!
//! The oracle is the tile itself: at scale 1 with an integer origin every
//! pixel reads exactly one texel, (x − Ox) mod w, (y − Oy) mod h, of an
//! opaque tile at Normal 100 %. Never a golden.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_comps::{CompCaptures, CompRequest};
use doctored_core::doc_transform::Affine;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba};
use doctored_core::ffi_doc::{dr_doc_add_image_layer_at, dr_doc_free};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_doc_save_psd, dr_psd_import_report};
use doctored_core::ffi_retained_source::dr_doc_mixed_source_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{ClipboardPayload, PasteRequest};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::RgbaImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, ffi_style, rect_layer_doc, styled, take_err_string, try_styled};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (96, 64);
const RECT: (i32, i32, u32, u32) = (13, 9, 50, 40);
const GREEN: [u8; 4] = [0x30, 0xa0, 0x50, 255];

// ---------------------------------------------------------------- helpers --

/// take1's tile T: 12 x 8, opaque, (20x + 10, 30y + 15, 200 | 60).
fn texel(x: u32, y: u32) -> [u8; 4] {
    [
        (20 * x + 10) as u8,
        (30 * y + 15) as u8,
        if (x + y).is_multiple_of(2) { 200 } else { 60 },
        255,
    ]
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// One linked pattern overlay of tile T with `phase`, no anchor.
fn style(phase: (i32, i32)) -> Value {
    let mut rgba = Vec::new();
    for y in 0..8 {
        for x in 0..12 {
            rgba.extend_from_slice(&texel(x, y));
        }
    }
    json!({"effects":[{"type":"pattern_overlay",
        "pattern":{"name":"T","width":12,"height":8,"rgba":base64(&rgba)},
        "phase":[phase.0, phase.1]}]})
}

/// The texel tile T puts at canvas pixel (x, y) with its origin at `o`.
fn want(o: (i32, i32), x: i32, y: i32) -> [u8; 4] {
    texel(
        (x - o.0).rem_euclid(12) as u32,
        (y - o.1).rem_euclid(8) as u32,
    )
}

fn rect_doc() -> DrDocument {
    rect_layer_doc(CANVAS, RECT, GREEN)
}

fn anchor8(doc: &DrDocument, idx: usize) -> Value {
    let style: Value = serde_json::from_str(&ffi_style(doc, idx).expect("a style")).unwrap();
    style["effects"][0]["anchor"].clone()
}

/// Every pixel of `doc` inside `rect` is tile T at canvas origin `o`.
fn assert_tile8(doc: &DrDocument, rect: (i32, i32, u32, u32), o: (i32, i32), what: &str) {
    let flat = doc.flattened().unwrap();
    for y in rect.1..rect.1 + rect.3 as i32 {
        for x in rect.0..rect.0 + rect.2 as i32 {
            assert_eq!(
                flat.get_pixel(x as u32, y as u32).0,
                want(o, x, y),
                "{what} at ({x}, {y})"
            );
        }
    }
}

fn translate(dx: f64, dy: f64) -> Affine {
    Affine::from_array([1.0, 0.0, 0.0, 1.0, dx, dy])
}

fn byte_geometry(doc: &DrDocument, request: Value) -> DrDocument {
    let edit = GeometryEdit::parse(&request.to_string()).unwrap();
    doc.geometry_edit(&edit, LIMIT, 0, None)
        .unwrap()
        .expect("the geometry changes the document")
}

fn typed_geometry(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let edit = GeometryEdit::parse(&request.to_string()).unwrap();
    doc.geometry(&edit, LIMIT, 0, None)
        .unwrap()
        .expect("the geometry changes the document")
}

fn image16(w: u32, h: u32, px: [u8; 4]) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        w,
        h,
        vec![px.map(|v| u16::from(v) * 257); (w * h) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

/// The byte fixture's typed twin: a white canvas under the green rect, made
/// at RECT.
fn typed_doc() -> PrecisionDocument {
    let base = Layer::raster(
        "Base",
        image16(CANVAS.0, CANVAS.1, [255; 4]),
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let rect = Layer::raster(
        "Rect",
        image16(RECT.2, RECT.3, GREEN),
        (RECT.0, RECT.1),
        1.0,
        true,
    )
    .unwrap();
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![base, rect],
    )
    .unwrap()
}

fn typed_edit(doc: &PrecisionDocument, request: &Value) -> PrecisionDocument {
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

fn typed_styled(doc: &PrecisionDocument, layer: usize, style: &Value) -> PrecisionDocument {
    typed_edit(
        doc,
        &json!({"op":"layer_style","version":1,"layer":layer,"style":style}),
    )
}

fn anchor16(doc: &PrecisionDocument, layer: usize) -> Value {
    let reported: Value =
        serde_json::from_str(&doc.layer(layer).unwrap().reported_style().unwrap()).unwrap();
    reported["effects"][0]["anchor"].clone()
}

fn reference16(doc: &PrecisionDocument, layer: usize) -> (i32, i32) {
    doc.layer(layer).unwrap().reference()
}

// ------------------------------------------------------------- AT-CREATION --

#[test]
fn a_move_before_the_style_anchors_where_the_layer_was_made() {
    let moved = rect_doc().move_layers(&[1], 7, 3).unwrap();
    assert_eq!(moved.layers[1].reference, (7, 3));
    let doc = styled(&moved, 1, &style((5, 2)).to_string());
    assert_eq!(
        anchor8(&doc, 1),
        json!([13, 9]),
        "anchor = offset − reference"
    );
    let rect = (RECT.0 + 7, RECT.1 + 3, RECT.2, RECT.3);
    assert_tile8(&doc, rect, (12, 5), "byte, O = phase + (7, 3)");
    // A layer styled where it was made anchors at its offset (slice 1).
    let made = styled(&rect_doc(), 1, &style((5, 2)).to_string());
    assert_eq!(anchor8(&made, 1), json!([13, 9]));
    assert_tile8(&made, RECT, (5, 2), "byte, unmoved");

    let typed = typed_geometry(
        &typed_doc(),
        json!({"op":"affine_set","layers":[1],"matrix":[1,0,0,1,7,3],"filter":"nearest"}),
    );
    assert_eq!(reference16(&typed, 1), (7, 3));
    let typed = typed_styled(&typed, 1, &style((5, 2)));
    assert_eq!(anchor16(&typed, 1), json!([13, 9]));
    let pixels = typed.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    for y in rect.1..rect.1 + rect.3 as i32 {
        for x in rect.0..rect.0 + rect.2 as i32 {
            let got = pixels[(y as u32 * CANVAS.0 + x as u32) as usize];
            let w = want((12, 5), x, y);
            for c in 0..3 {
                let code = f64::from(got[c]) / 257.0;
                assert!(
                    (code - f64::from(w[c])).abs() <= 1.0,
                    "typed at ({x}, {y}): {got:?} against {w:?}"
                );
            }
        }
    }
}

#[test]
fn every_translation_accumulates_the_reference() {
    // An offset write.
    let doc = rect_doc().with_layer_offset(1, 20, 12).unwrap();
    assert_eq!(doc.layers[1].reference, (7, 3));
    // A pure-translation transform of the raster.
    let doc = doc
        .transform_layer(
            1,
            translate(4.0, -2.0),
            image::imageops::FilterType::Nearest,
        )
        .unwrap();
    assert_eq!(doc.layers[1].reference, (11, 1));
    assert_eq!(doc.layers[1].offset, (24, 10));
    // A group move: the child and the group record.
    let (grouped, group, _, _) = rect_doc().group_layers(&[1], "G").unwrap();
    let child = grouped
        .layers
        .iter()
        .position(|l| l.name == "Rect")
        .unwrap();
    let moved = grouped.move_layers(&[group], 2, 1).unwrap();
    assert_eq!(moved.layers[child].reference, (2, 1));
    assert_eq!(moved.layers[group].reference, (2, 1));
    // A pure-translation transform of a group: the group record too.
    let shifted = byte_geometry(
        &moved,
        json!({"op":"affine_set","layers":[group],"matrix":[1,0,0,1,4,-2],"filter":"nearest"}),
    );
    assert_eq!(shifted.layers[child].reference, (6, -1));
    assert_eq!(shifted.layers[group].reference, (6, -1));
    // The typed twin: a single-record affine and a set.
    let typed = typed_geometry(
        &typed_doc(),
        json!({"op":"affine","layer":1,"target":"pixels","matrix":[1,0,0,1,-3,2],"filter":"nearest"}),
    );
    assert_eq!(reference16(&typed, 1), (-3, 2));
    assert_eq!(typed.layer(1).unwrap().offset(), (10, 11));
}

#[test]
fn a_retained_source_layer_accumulates_a_pure_translation() {
    let source = DrDocument::from_pixels(RgbaImage::from_fn(32, 24, |x, y| {
        image::Rgba([(x * 7) as u8, (y * 9) as u8, 90, 255])
    }));
    let request = CString::new(
        json!({"op":"place","source_id":"fixture-a","name":"Placed","x":0,"y":0}).to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let placed = unsafe {
        dr_doc_mixed_source_edit(
            &source,
            &source,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!placed.is_null(), "{}", take_err_string(err));
    let placed = *unsafe { Box::from_raw(placed) };
    assert!(
        placed.layers[1].retained_source.is_some(),
        "a retained source"
    );
    let moved = placed
        .transform_layer(1, translate(5.0, 6.0), image::imageops::FilterType::Nearest)
        .unwrap();
    assert!(moved.layers[1].retained_source.is_some());
    assert_eq!(moved.layers[1].reference, (5, 6));
    let set = byte_geometry(
        &moved,
        json!({"op":"affine_set","layers":[1],"matrix":[1,0,0,1,-1,2],"filter":"nearest"}),
    );
    assert_eq!(set.layers[1].reference, (4, 8));
    // A scale maps the point by its matrix: (4, 8) / 2.
    let scaled = set
        .transform_layer(
            1,
            Affine::from_array([0.5, 0.0, 0.0, 0.5, 0.0, 0.0]),
            image::imageops::FilterType::Triangle,
        )
        .unwrap();
    assert_eq!(scaled.layers[1].reference, (2, 4));
}

#[test]
fn transforms_map_the_reference_and_delete_cropped_pixels_leaves_it() {
    // Free Transform maps the point by its matrix, M((7, 3)).
    let moved = rect_doc().move_layers(&[1], 7, 3).unwrap();
    let scaled = moved
        .transform_layer(
            1,
            Affine::from_array([2.0, 0.0, 0.0, 2.0, 0.0, 0.0]),
            image::imageops::FilterType::Nearest,
        )
        .unwrap();
    assert_eq!(scaled.layers[1].reference, (14, 6));
    let typed = typed_geometry(
        &typed_doc(),
        json!({"op":"affine","layer":1,"target":"pixels","matrix":[2,0,0,2,0,0],"filter":"nearest"}),
    );
    assert_eq!(reference16(&typed, 1), (0, 0));
    // Delete Cropped Pixels moves the offset, not the content: the anchor
    // rebases (slice 1) and the reference stays.
    let doc = rect_layer_doc(CANVAS, (-5, -3, 40, 30), GREEN)
        .move_layers(&[1], -1, 0)
        .unwrap();
    assert_eq!(doc.layers[1].reference, (-1, 0));
    let trimmed = doc
        .delete_cropped_pixels()
        .expect("the layer reaches outside");
    assert_eq!(trimmed.layers[1].offset, (0, 0));
    assert_eq!(trimmed.layers[1].reference, (-1, 0));
}

#[test]
fn placements_leave_the_reference() {
    // A layer comp's restore of a moved layer.
    let saved = rect_doc()
        .edit_layer_comps(&CompRequest::Save {
            name: "Home".into(),
            comment: String::new(),
            captures: CompCaptures::default(),
        })
        .unwrap()
        .unwrap();
    let moved = saved.move_layers(&[1], 7, 3).unwrap();
    let restored = moved
        .edit_layer_comps(&CompRequest::Apply { comp: 0 })
        .unwrap()
        .unwrap();
    assert_eq!(restored.layers[1].offset, (RECT.0, RECT.1));
    assert_eq!(
        restored.layers[1].reference,
        (7, 3),
        "a placement, not a move"
    );

    // A byte paste lands where it is made.
    let pixels = vec![200u8; 10 * 6 * 4];
    let image = unsafe { dr_image_from_rgba(pixels.as_ptr(), 10, 6) };
    let name = CString::new("Pasted").unwrap();
    let doc = rect_doc();
    let pasted = unsafe { dr_doc_add_image_layer_at(&doc, 1, image, name.as_ptr(), 40, 30) };
    unsafe { dr_image_free(image) };
    assert!(!pasted.is_null());
    let byte = unsafe { (*pasted).clone() };
    unsafe { dr_doc_free(pasted) };
    assert_eq!(byte.layers.len(), 3);
    assert_eq!(byte.layers[2].offset, (40, 30));
    assert_eq!(byte.layers[2].reference, (0, 0));
    assert_eq!(byte.layers[2].name, "Pasted");
    let byte = styled(&byte, 2, &style((0, 0)).to_string());
    assert_eq!(anchor8(&byte, 2), json!([40, 30]), "made at (40, 30)");

    // The typed paste lands the same way (`clipboard_paste`, in place).
    let mut drcb = b"RZCB".to_vec();
    for word in [1u32, 1, 10, 6] {
        drcb.extend(word.to_le_bytes());
    }
    drcb.extend(40i32.to_le_bytes());
    drcb.extend(30i32.to_le_bytes());
    drcb.extend(72f32.to_le_bytes());
    drcb.extend(72f32.to_le_bytes());
    let profile = IccProfile::srgb();
    drcb.extend((profile.bytes().len() as u32).to_le_bytes());
    drcb.extend_from_slice(profile.bytes());
    drcb.push(0);
    for _ in 0..10 * 6 * 4 {
        drcb.extend((200u16 * 257).to_le_bytes());
    }
    let payload = ClipboardPayload::decode(&drcb, LIMIT, 0).unwrap();
    let request: PasteRequest = serde_json::from_value(
        json!({"name":"Pasted","anchor":1,"position":"above","placement":"in_place"}),
    )
    .unwrap();
    let typed = typed_doc()
        .clipboard_paste(&payload, &request, LIMIT, 0, None)
        .unwrap();
    assert_eq!(typed.layer(2).unwrap().offset(), (40, 30));
    assert_eq!(reference16(&typed, 2), (0, 0));
    let typed = typed_styled(&typed, 2, &style((0, 0)));
    assert_eq!(
        anchor16(&typed, 2),
        anchor8(&byte, 2),
        "byte paste = typed paste"
    );
}

// --------------------------------------------------------------------- .rz --

#[test]
fn the_reference_round_trips_through_word_91_at_both_depths() {
    let dir = TempDir::new().unwrap();
    // Byte: 91 is the first word inside 45.
    let moved = rect_doc().move_layers(&[1], 7, 3).unwrap();
    let path = dir.path().join("moved.rz");
    moved.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &45u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &91u32.to_le_bytes(), "word 91 inside 45");
    assert!(bytes.windows(4).any(|w| w == b"LREF"));
    let reopened = DrDocument::open(path.to_str().unwrap()).unwrap();
    assert_eq!(reopened.layers[1].reference, (7, 3));
    assert_eq!(reopened.layers[0].reference, (0, 0));
    // An unmoved document writes neither.
    let plain = dir.path().join("plain.rz");
    rect_doc().save_native(plain.to_str().unwrap()).unwrap();
    let plain = std::fs::read(&plain).unwrap();
    assert_ne!(&plain[8..12], &91u32.to_le_bytes());
    assert!(!plain.windows(4).any(|w| w == b"LREF"));
    // A styled moved document holds 91 then 90.
    let styled_moved = styled(&moved, 1, &style((0, 0)).to_string());
    let both = dir.path().join("both.rz");
    styled_moved.save_native(both.to_str().unwrap()).unwrap();
    let both_bytes = std::fs::read(&both).unwrap();
    assert_eq!(&both_bytes[8..12], &91u32.to_le_bytes());
    assert_eq!(&both_bytes[12..16], &90u32.to_le_bytes());
    let back = DrDocument::open(both.to_str().unwrap()).unwrap();
    assert_eq!(back.layers[1].reference, (7, 3));
    assert_eq!(back.flattened().unwrap(), styled_moved.flattened().unwrap());

    // Typed: 91 right after 46 (no CMYK slot in an RGB file).
    let typed = typed_geometry(
        &typed_doc(),
        json!({"op":"affine_set","layers":[1],"matrix":[1,0,0,1,-4,6],"filter":"nearest"}),
    );
    let path = dir.path().join("typed.rz");
    typed
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[4..8], &46u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &91u32.to_le_bytes(), "word 91 after 46");
    let reopened = PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert_eq!(reference16(&reopened, 1), (-4, 6));
    assert_eq!(reference16(&reopened, 0), (0, 0));
    let plain = dir.path().join("typed_plain.rz");
    typed_doc()
        .save_native(plain.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let plain = std::fs::read(&plain).unwrap();
    assert!(!plain.windows(4).any(|w| w == b"LREF"));
}

#[test]
fn a_crafted_lref_table_refuses_by_name() {
    let dir = TempDir::new().unwrap();
    let moved = rect_doc().move_layers(&[1], 7, 3).unwrap();
    let path = dir.path().join("moved.rz");
    moved.save_native(path.to_str().unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let at = bytes.windows(4).position(|w| w == b"LREF").unwrap();
    let write = |offset: usize, value: u32| {
        let mut forged = bytes.clone();
        forged[at + offset..at + offset + 4].copy_from_slice(&value.to_le_bytes());
        forged
    };
    let mut both_zero = write(16, 0);
    both_zero[at + 20..at + 24].copy_from_slice(&0u32.to_le_bytes());
    for (forged, sentence) in [
        (write(4, 2), "unsupported native layer reference schema"),
        (write(8, 0), "invalid native layer reference count"),
        (
            write(12, 99),
            "invalid native layer reference order or index",
        ),
        (both_zero, "invalid native layer reference point"),
    ] {
        let file = dir.path().join("forged.rz");
        std::fs::write(&file, &forged).unwrap();
        let Err(message) = DrDocument::open(file.to_str().unwrap()) else {
            panic!("{sentence}: the forged table must refuse");
        };
        assert!(message.contains(sentence), "{message}");
    }
}

// --------------------------------------------------------------------- PSD --

fn import_report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(raw)).expect("import report JSON")
}

fn open8(path: &Path) -> DrDocument {
    let report = import_report(path);
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), 0, token.as_ptr(), &mut error) };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    *unsafe { Box::from_raw(raw) }
}

fn save8(doc: &DrDocument, path: &Path) {
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_doc_save_psd(doc, ptr::null(), cpath(path).as_ptr(), 0, 0, &mut error) };
    assert!(ok, "save: {}", take_err_string(error));
}

#[test]
fn an_unstyled_moved_layer_carries_its_fxrp_through_a_psd() {
    let dir = TempDir::new().unwrap();
    let moved = rect_doc().move_layers(&[1], 7, 3).unwrap();
    let path = dir.path().join("moved.psd");
    save8(&moved, &path);
    let bytes = std::fs::read(&path).unwrap();
    let at = bytes
        .windows(8)
        .position(|w| w == b"8BIMfxrp")
        .expect("an fxrp block");
    let x = f64::from_be_bytes(bytes[at + 12..at + 20].try_into().unwrap());
    let y = f64::from_be_bytes(bytes[at + 20..at + 28].try_into().unwrap());
    assert_eq!((x, y), (7.0, 3.0));
    let back = open8(&path);
    assert_eq!(back.layers[1].reference, (7, 3));
    assert_eq!(back.layers[1].offset, (20, 12));
    // An unmoved document writes none.
    let plain = dir.path().join("plain.psd");
    save8(&rect_doc(), &plain);
    let plain = std::fs::read(&plain).unwrap();
    assert!(!plain.windows(8).any(|w| w == b"8BIMfxrp"));
}

#[test]
fn photoshops_p5_reads_its_reference_and_re_anchors_where_it_was_made() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/pattern_overlay/p5_rgb8.psd");
    let doc = open8(&path);
    let index = doc
        .layers
        .iter()
        .position(|l| l.style.is_some())
        .expect("the styled layer");
    assert_eq!(doc.layers[index].reference, (7, 3), "Photoshop's fxrp");
    let before = doc.flattened().unwrap();
    // The style with its anchor stripped re-anchors at offset − reference:
    // the same picture.
    let mut style: Value = serde_json::from_str(&ffi_style(&doc, index).unwrap()).unwrap();
    for effect in style["effects"].as_array_mut().unwrap() {
        effect.as_object_mut().unwrap().remove("anchor");
    }
    let cleared = try_styled(&doc, index, None).unwrap().unwrap();
    let again = styled(&cleared, index, &style.to_string());
    assert_eq!(anchor8(&again, index), anchor8(&doc, index));
    assert_eq!(again.flattened().unwrap(), before);
}
