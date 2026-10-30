//! DELETE CROPPED PIXELS — the Crop tool's option that trims layer
//! storage to the new canvas, at 8 bits (`dr_doc_delete_cropped_pixels`) and
//! at 16 bits and float (the typed straighten's `delete_cropped` flag).
//!
//! The oracle is the rule itself, written out here independently: each
//! eligible raster (no meta/description, no retained source) becomes its
//! intersection with the canvas, holding exactly the samples the untrimmed
//! result shows there; one wholly outside becomes a 1×1 transparent pixel at
//! (0, 0); masks and every other record are what the plain crop leaves; a
//! layer comp entry bound to a trimmed layer moves by that layer's own
//! origin delta.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::doc_comps::LayerComp;
use doctored_core::doc_guide::GuideOrientation;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::doc_sampler::ColorSampler;
use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
use doctored_core::ffi_delete_cropped::dr_doc_delete_cropped_pixels;
use doctored_core::ffi_doc::dr_doc_crop;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::take_err_string;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";
const W: u32 = 20;
const H: u32 = 16;
/// The crop window every case uses.
const WINDOW: (u32, u32, u32, u32) = (4, 3, 10, 8);

// ------------------------------------------------------------- plumbing --

fn owned(out: *mut DrDocument) -> Option<DrDocument> {
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

fn delete(doc: &DrDocument) -> Option<DrDocument> {
    owned(unsafe { dr_doc_delete_cropped_pixels(doc) })
}

fn crop(doc: &DrDocument) -> DrDocument {
    owned(unsafe { dr_doc_crop(doc, WINDOW.0, WINDOW.1, WINDOW.2, WINDOW.3) }).expect("a crop")
}

fn byte_save_comp(doc: &DrDocument) -> DrDocument {
    let c =
        CString::new(json!({"op":"layer_comp","action":"save","name":"Shot"}).to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_edit_layer_comps(doc, c.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    owned(out).unwrap()
}

fn typed_edit(doc: &PrecisionDocument, request: &Value) -> Option<PrecisionDocument> {
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
    assert!(
        !out.is_null() || err.is_null(),
        "{request}: {}",
        take_err_string(err)
    );
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

fn typed_must(doc: &PrecisionDocument, request: &Value) -> PrecisionDocument {
    typed_edit(doc, request).unwrap_or_else(|| panic!("{request} changed nothing"))
}

fn straighten(matrix: [f64; 6], filter: &str, delete_cropped: bool) -> Value {
    json!({"op": "straighten", "matrix": matrix, "filter": filter,
           "x": WINDOW.0, "y": WINDOW.1, "width": WINDOW.2, "height": WINDOW.3,
           "delete_cropped": delete_cropped})
}

const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// The Crop tool's matrix: rotate by `−angle` about the window's centre.
fn rotation(angle: f64) -> [f64; 6] {
    let cx = f64::from(WINDOW.0) + f64::from(WINDOW.2) / 2.0;
    let cy = f64::from(WINDOW.1) + f64::from(WINDOW.3) / 2.0;
    let radians = -angle * std::f64::consts::PI / 180.0;
    let (s, c) = (radians.sin(), radians.cos());
    [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]
}

/// `(x, y, w, h)` of a rect intersected with the canvas, `None` when empty.
fn clip(rect: (i32, i32, u32, u32), canvas: (u32, u32)) -> Option<(i32, i32, u32, u32)> {
    let (x0, y0) = (rect.0.max(0), rect.1.max(0));
    let x1 = (i64::from(rect.0) + i64::from(rect.2)).min(i64::from(canvas.0)) as i32;
    let y1 = (i64::from(rect.1) + i64::from(rect.3)).min(i64::from(canvas.1)) as i32;
    (x0 < x1 && y0 < y1).then(|| (x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
}

fn entry_offset(comp: &LayerComp, id: u64) -> Option<(i32, i32)> {
    comp.entries
        .iter()
        .find(|e| e.layer_id == id)
        .expect("an entry")
        .offset
}

// ---------------------------------------------------------------- 8 bits --

fn pattern(w: u32, h: u32, seed: u8) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            seed.wrapping_add((x * 37 + y * 11) as u8),
            seed.wrapping_add((x * 5 + y * 53) as u8),
            seed,
            255,
        ])
    })
}

fn byte_named(doc: &DrDocument, name: &str) -> usize {
    doc.layers
        .iter()
        .position(|l| l.name == name)
        .unwrap_or_else(|| panic!("no layer named {name}"))
}

/// A fixture layer: name, dimensions, offset.
type Placed = (&'static str, (u32, u32), (i32, i32));

/// Base (canvas), Over (reaches past the window's top-left), Away (wholly
/// outside the window), Inside (inside it), Masked (past the right edge, with
/// a mask), Text (described, past the bottom edge), Tone (an adjustment).
fn byte_fixture() -> DrDocument {
    let mut doc = DrDocument::from_pixels(pattern(W, H, 9));
    doc.layers[0].name = "Base".into();
    let layers: [Placed; 6] = [
        ("Over", (8, 6), (1, 1)),
        ("Away", (3, 2), (16, 13)),
        ("Inside", (2, 2), (6, 5)),
        ("Masked", (6, 4), (11, 5)),
        ("Text", (5, 5), (7, 8)),
        ("Tone", (1, 1), (0, 0)),
    ];
    for (i, (name, dims, at)) in layers.into_iter().enumerate() {
        doc = doc
            .adding_image_layer(i, pattern(dims.0, dims.1, 30 + 20 * i as u8), name)
            .unwrap();
        doc.layers[i + 1].offset = at;
    }
    let masked = byte_named(&doc, "Masked");
    doc = doc.add_mask(masked, MaskKind::RevealAll).unwrap();
    let text = byte_named(&doc, "Text");
    doc.layers[text].meta = Some(r#"{"type":"text","version":1}"#.into());
    let tone = byte_named(&doc, "Tone");
    doc.layers[tone].meta = Some(json!({"type":"adjust","op":"invert","params":{}}).to_string());
    doc
}

#[test]
fn byte_trims_each_eligible_raster_to_the_canvas() {
    let cropped = crop(&byte_fixture());
    let out = delete(&cropped).expect("layers reach outside the canvas");
    let canvas = (out.width, out.height);
    assert_eq!(canvas, (WINDOW.2, WINDOW.3));
    for (before, after) in cropped.layers.iter().zip(&out.layers) {
        let (lw, lh) = before.pixels.dimensions();
        let rect = (before.offset.0, before.offset.1, lw, lh);
        let eligible = before.meta.is_none();
        match (eligible, clip(rect, canvas)) {
            (false, _) => {
                assert_eq!(after.offset, before.offset, "{} untouched", before.name);
                assert!(after.pixels.same_bytes(&before.pixels), "{}", before.name);
            }
            (true, None) => {
                assert_eq!(after.offset, (0, 0), "{} wholly outside", before.name);
                assert_eq!(after.pixels.dimensions(), (1, 1));
                assert_eq!(after.pixels.sample_at(0, 0), Some([0, 0, 0, 0]));
            }
            (true, Some(c)) => {
                assert_eq!(
                    (after.offset.0, after.offset.1),
                    (c.0, c.1),
                    "{} origin",
                    before.name
                );
                assert_eq!(after.pixels.dimensions(), (c.2, c.3), "{}", before.name);
                for y in 0..c.3 {
                    for x in 0..c.2 {
                        let bx = (c.0 + x as i32 - before.offset.0) as u32;
                        let by = (c.1 + y as i32 - before.offset.1) as u32;
                        assert_eq!(
                            after.pixels.sample_at(x, y),
                            before.pixels.sample_at(bx, by),
                            "{} kept sample ({x}, {y})",
                            before.name
                        );
                    }
                }
            }
        }
        // A mask is canvas-placed and is never trimmed.
        assert_eq!(after.mask_properties.offset, before.mask_properties.offset);
        match (&before.mask, &after.mask) {
            (Some(a), Some(b)) => assert!(std::sync::Arc::ptr_eq(a, b), "{}", before.name),
            (None, None) => {}
            _ => panic!("{} mask presence changed", before.name),
        }
    }
    let inside = byte_named(&out, "Inside");
    assert!(out.layers[inside]
        .pixels
        .same_bytes(&cropped.layers[inside].pixels));
    assert_eq!(out.layers[byte_named(&out, "Base")].offset, (0, 0));
    // Nothing reaches outside any more: the second pass is a no-op.
    assert!(delete(&out).is_none(), "an unchanged result is NULL");
}

#[test]
fn byte_delete_is_null_when_nothing_reaches_outside_and_on_null() {
    let doc = DrDocument::from_pixels(pattern(W, H, 3));
    assert!(delete(&doc).is_none());
    assert!(unsafe { dr_doc_delete_cropped_pixels(ptr::null()) }.is_null());
}

#[test]
fn byte_delete_is_lock_exempt() {
    let cropped = crop(&byte_fixture());
    let over = byte_named(&cropped, "Over");
    let locked = cropped.with_layer_locks(over, 7).expect("Lock All");
    let out = delete(&locked).expect("a locked layer is trimmed too");
    assert_eq!(out.layers[over].offset, (0, 0));
    assert_eq!(out.layers[over].pixels.dimensions(), (5, 4));
}

#[test]
fn byte_comps_move_with_the_trimmed_origin() {
    let doc = byte_save_comp(&byte_fixture());
    // Move Over after the comp was saved, so the comp records an offset that
    // is NOT the layer's current one.
    let over = byte_named(&doc, "Over");
    let mut moved = doc.clone();
    moved.layers[over].offset = (0, 2);
    let cropped = crop(&moved);
    let recorded = entry_offset(&cropped.layer_comps()[0], cropped.layers[over].id).unwrap();
    let before = cropped.layers[over].offset;
    let out = delete(&cropped).unwrap();
    let after = out.layers[over].offset;
    let id = out.layers[over].id;
    assert_eq!(
        entry_offset(&out.layer_comps()[0], id),
        Some((
            recorded.0 + after.0 - before.0,
            recorded.1 + after.1 - before.1
        )),
        "the recorded offset moves by the trim delta"
    );
    // An untrimmed layer's entry is unchanged.
    let inside = byte_named(&out, "Inside");
    assert_eq!(
        entry_offset(&out.layer_comps()[0], out.layers[inside].id),
        entry_offset(&cropped.layer_comps()[0], cropped.layers[inside].id)
    );
}

// ---------------------------------------------------------- 16 bits, float --

fn image(w: u32, h: u32, seed: u16) -> PrecisionImage {
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let v = seed.wrapping_add((i * 37) as u16);
            [v, v.wrapping_mul(3), 65535 - v, 65535]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).expect("an image")
}

fn typed_mask(w: u32, h: u32, offset: (i32, i32)) -> PrecisionMask {
    let samples: Vec<u16> = (0..w * h).map(|i| (i * 911 % 65536) as u16).collect();
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, samples, None).expect("a mask plane"),
        MaskProperties::at(offset),
        true,
    )
    .expect("a mask")
}

/// The typed twin of `byte_fixture` in canvas coordinates before the crop,
/// plus a described layer and a group, and the canvas-space records a crop
/// carries: a saved channel, three guides (one outside the window), a moved
/// ruler origin and two colour samplers (one outside the window).
fn typed_fixture() -> PrecisionDocument {
    let base = Layer::raster("Base", image(W, H, 100), (0, 0), 1.0, true).unwrap();
    let over = Layer::raster("Over", image(8, 6, 2000), (1, 1), 1.0, true).unwrap();
    let away = Layer::raster("Away", image(3, 2, 4000), (16, 13), 1.0, true).unwrap();
    let inside = Layer::raster("Inside", image(2, 2, 6000), (6, 5), 1.0, true).unwrap();
    let masked = Layer::raster("Masked", image(6, 4, 8000), (11, 5), 1.0, true)
        .unwrap()
        .with_mask(Some(typed_mask(6, 4, (11, 5))));
    let inner = Layer::raster("Inner", image(6, 5, 9000), (12, 9), 1.0, true).unwrap();
    let group = Layer::isolated_normal("Group", vec![inner], 1.0, true).unwrap();
    let text = Layer::raster("Text", image(5, 5, 10000), (7, 8), 1.0, true).unwrap();
    let channel = PrecisionChannel::new(
        "Coverage",
        PrecisionPlane::from_u16(
            W,
            H,
            (0..W * H).map(|i| (i * 523 % 65536) as u16).collect(),
            None,
        )
        .expect("a channel plane"),
    )
    .expect("a channel");
    let doc = PrecisionDocument::from_parts_and_channels(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![base, over, away, inside, masked, group, text],
        vec![channel],
    )
    .expect("a typed document");
    let mut doc = doc;
    for (orientation, position) in [
        (GuideOrientation::Vertical, 7.0),
        (GuideOrientation::Horizontal, 5.5),
        (GuideOrientation::Vertical, 17.0),
    ] {
        doc = doc.add_guide(orientation, position).expect("a guide");
    }
    let doc = doc.set_ruler_origin(2.0, 1.0).expect("the origin moves");
    let doc = doc
        .with_color_samplers(&[
            ColorSampler {
                x: 6,
                y: 5,
                sample_size: 1,
            },
            ColorSampler {
                x: 16,
                y: 13,
                sample_size: 3,
            },
        ])
        .expect("two samplers");
    let index = typed_named(&doc, "Text");
    let description = CString::new(r#"{"type":"shape","version":3,"probe":"113"}"#).unwrap();
    let name = CString::new("Text").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let source = doc.layer_source(index).unwrap();
    let out = unsafe {
        doctored_core::ffi_artwork::dr_precision_doc_artwork(
            &doc,
            index,
            true,
            source,
            7,
            8,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let described = *unsafe { Box::from_raw(out) };
    assert!(described.artwork_description(index).is_some());
    described
}

fn typed_named(doc: &PrecisionDocument, name: &str) -> usize {
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record named {name}"))
}

fn bits(image: &PrecisionImage, x: u32, y: u32) -> [u32; 4] {
    image.sample_at(x, y).expect("a sample").map(f32::to_bits)
}

/// Every record of `trimmed` against the same record of `kept` (the same op
/// without the flag): an eligible raster is the clip of `kept`'s extent to the
/// canvas with `kept`'s samples there, a wholly outside one a 1×1 transparent
/// pixel at (0, 0), and everything else — masks included — is `kept`'s.
fn assert_trimmed(kept: &PrecisionDocument, trimmed: &PrecisionDocument, eligible: &[&str]) {
    let canvas = trimmed.dimensions();
    assert_eq!(canvas, kept.dimensions());
    for i in 0..kept.layer_count() {
        let (a, b) = (kept.layer(i).unwrap(), trimmed.layer(i).unwrap());
        let name = a.name();
        assert_eq!(
            a.mask()
                .map(|m| (m.properties().offset, m.plane().samples().to_vec())),
            b.mask()
                .map(|m| (m.properties().offset, m.plane().samples().to_vec())),
            "{name}: masks are not trimmed"
        );
        let (Some(ka), Some(kb)) = (kept.layer_source(i), trimmed.layer_source(i)) else {
            continue;
        };
        let (kw, kh) = ka.dimensions();
        let rect = (a.offset().0, a.offset().1, kw, kh);
        if !eligible.contains(&name) {
            assert_eq!(b.offset(), a.offset(), "{name} untouched");
            assert_eq!(kb.dimensions(), ka.dimensions(), "{name} untouched");
            continue;
        }
        match clip(rect, canvas) {
            None => {
                assert_eq!(b.offset(), (0, 0), "{name} wholly outside");
                assert_eq!(kb.dimensions(), (1, 1), "{name}");
                assert_eq!(kb.sample_at(0, 0).unwrap()[3], 0.0, "{name} transparent");
            }
            Some(c) => {
                assert_eq!(b.offset(), (c.0, c.1), "{name} origin");
                assert_eq!(kb.dimensions(), (c.2, c.3), "{name} dims");
                for y in 0..c.3 {
                    for x in 0..c.2 {
                        let ax = (c.0 + x as i32 - rect.0) as u32;
                        let ay = (c.1 + y as i32 - rect.1) as u32;
                        assert_eq!(
                            bits(kb, x, y),
                            bits(ka, ax, ay),
                            "{name} kept sample ({x}, {y})"
                        );
                    }
                }
            }
        }
    }
}

const ELIGIBLE: [&str; 6] = ["Base", "Over", "Away", "Inside", "Masked", "Inner"];

/// The canvas-space records a crop carries: the guides, the ruler origin, the
/// colour samplers and each saved channel's samples.
type Chrome = (
    Vec<(GuideOrientation, f64)>,
    (f64, f64),
    Vec<ColorSampler>,
    Vec<Vec<u16>>,
);

fn chrome(doc: &PrecisionDocument) -> Chrome {
    (
        doc.guides()
            .iter()
            .map(|g| (g.orientation, g.position))
            .collect(),
        doc.ruler_origin(),
        doc.color_samplers().to_vec(),
        doc.channels()
            .iter()
            .map(|c| c.plane().samples().to_vec())
            .collect(),
    )
}

#[test]
fn typed_crop_with_delete_trims_and_equals_the_plain_crop_inside() {
    let doc = typed_fixture();
    let plain = typed_must(
        &doc,
        &json!({"op":"crop","x":WINDOW.0,"y":WINDOW.1,"width":WINDOW.2,"height":WINDOW.3}),
    );
    // The identity straighten over the window IS the crop (the host sends a
    // crop with the option this way).
    let identity = typed_must(&doc, &straighten(IDENTITY, "nearest", false));
    assert_trimmed(&plain, &identity, &[]);
    let trimmed = typed_must(&doc, &straighten(IDENTITY, "nearest", true));
    assert_trimmed(&plain, &trimmed, &ELIGIBLE);
    // Guides, the ruler origin, colour samplers and the saved channel are the
    // plain crop's on both routes: every 16-bit crop with the option runs as
    // this straighten.
    let cropped = chrome(&plain);
    assert_ne!(cropped, chrome(&doc), "the crop moves these records");
    assert_eq!(
        cropped.0.len(),
        2,
        "the guide outside the window is dropped"
    );
    assert_eq!(
        cropped.2,
        [ColorSampler {
            x: 2,
            y: 2,
            sample_size: 1
        }],
        "the sampler inside moves with the window; the one outside is dropped"
    );
    assert_eq!(cropped.3.len(), 1);
    assert_eq!(cropped.3[0].len(), (WINDOW.2 * WINDOW.3) as usize);
    assert_eq!(chrome(&identity), cropped, "the identity straighten");
    assert_eq!(chrome(&trimmed), cropped, "the straighten with the flag");
    let text = typed_named(&trimmed, "Text");
    assert_eq!(
        trimmed.artwork_description(text),
        plain.artwork_description(typed_named(&plain, "Text")),
        "a described layer is what the plain crop leaves (the host restores the description)"
    );
    // A second pass trims only Text, whose description the core dropped with
    // the move (the host puts it back); every other record is already inside.
    let second = typed_must(
        &trimmed,
        &json!({"op":"straighten","matrix":IDENTITY,"filter":"nearest","x":0,"y":0,
                "width":WINDOW.2,"height":WINDOW.3,"delete_cropped":true}),
    );
    let mut all = ELIGIBLE.to_vec();
    all.push("Text");
    assert_trimmed(&trimmed, &second, &all);
    assert_eq!(second.layer_source(text).unwrap().dimensions(), (5, 3));
}

#[test]
fn typed_straighten_with_delete_keeps_its_filter() {
    let doc = typed_fixture();
    let m = rotation(7.5);
    let kept = typed_must(&doc, &straighten(m, "bicubic", false));
    let trimmed = typed_must(&doc, &straighten(m, "bicubic", true));
    // Only the destinations shrink: every kept sample is the bicubic result's.
    assert_trimmed(
        &kept,
        &trimmed,
        &["Base", "Over", "Away", "Inside", "Masked", "Inner"],
    );
}

#[test]
fn typed_comps_move_with_the_clip_delta() {
    let doc = typed_fixture();
    let saved = typed_must(
        &doc,
        &json!({"op":"layer_comp","action":"save","name":"Shot"}),
    );
    // Move Over after the comp, so its recorded offset is not its current one.
    let over = typed_named(&saved, "Over");
    let moved = typed_must(
        &saved,
        &json!({"op":"affine_set","layers":[over],"matrix":[1,0,0,1,-1,1],"filter":"nearest"}),
    );
    let id = moved.layer(over).unwrap().id();
    let plain = typed_must(&moved, &straighten(IDENTITY, "nearest", false));
    let trimmed = typed_must(&moved, &straighten(IDENTITY, "nearest", true));
    let recorded = entry_offset(&plain.layer_comps()[0], id).unwrap();
    let (before, after) = (
        plain.layer(over).unwrap().offset(),
        trimmed.layer(over).unwrap().offset(),
    );
    assert_ne!(before, after, "Over was trimmed");
    assert_eq!(
        entry_offset(&trimmed.layer_comps()[0], id),
        Some((
            recorded.0 + after.0 - before.0,
            recorded.1 + after.1 - before.1
        ))
    );
    let inside = typed_named(&trimmed, "Inside");
    let inside_id = trimmed.layer(inside).unwrap().id();
    assert_eq!(
        entry_offset(&trimmed.layer_comps()[0], inside_id),
        entry_offset(&plain.layer_comps()[0], inside_id),
        "an untrimmed layer's entry is the plain crop's"
    );
}

#[test]
fn a_float_crop_with_delete_trims() {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return;
    }
    let doc = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    let (w, h) = doc.dimensions();
    let window = (1, 1, w - 3, h - 2);
    let request = |delete: bool| {
        json!({"op":"straighten","matrix":IDENTITY,"filter":"nearest","x":window.0,
               "y":window.1,"width":window.2,"height":window.3,"delete_cropped":delete})
    };
    let kept = typed_must(&doc, &request(false));
    let trimmed = typed_must(&doc, &request(true));
    assert!(trimmed.is_float());
    let names: Vec<String> = (0..doc.layer_count())
        .map(|i| doc.layer(i).unwrap().name().to_string())
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_trimmed(&kept, &trimmed, &names);
}

/// Delete Cropped Pixels moves a trimmed raster's offset without
/// moving its content, so a linked Pattern Overlay's anchor moves with the
/// offset — the tile's canvas origin, phase + (offset − anchor), is the one
/// the plain crop leaves. The byte twin is `style_pattern_overlay`'s
/// `delete_cropped_pixels_rebases_the_anchor_and_keeps_the_picture`.
#[test]
fn typed_delete_rebases_a_linked_pattern_anchor() {
    let base = Layer::raster("Base", image(W, H, 100), (0, 0), 1.0, true).unwrap();
    let patterned = Layer::raster("P", image(12, 10, 3000), (-3, -2), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![base, patterned],
    )
    .expect("a typed document");
    let doc = typed_must(
        &doc,
        &json!({"op":"layer_style","version":1,"layer":1,
                "style":{"effects":[{"type":"pattern_overlay","pattern":{"preset":"stripes"}}]}}),
    );
    let reference = |doc: &PrecisionDocument| {
        let layer = doc.layer(1).unwrap();
        let style: Value = serde_json::from_str(&layer.reported_style().unwrap()).unwrap();
        let anchor = &style["effects"][0]["anchor"];
        let (ax, ay) = (anchor[0].as_i64().unwrap(), anchor[1].as_i64().unwrap());
        let (ox, oy) = layer.offset();
        (i64::from(ox) - ax, i64::from(oy) - ay)
    };
    assert_eq!(reference(&doc), (0, 0), "anchored at the offset when set");
    let kept = typed_must(&doc, &straighten(IDENTITY, "nearest", false));
    let trimmed = typed_must(&doc, &straighten(IDENTITY, "nearest", true));
    assert_ne!(
        kept.layer(1).unwrap().offset(),
        trimmed.layer(1).unwrap().offset(),
        "the trim moves the offset"
    );
    assert_eq!(reference(&trimmed), reference(&kept), "the tile stays put");
}
