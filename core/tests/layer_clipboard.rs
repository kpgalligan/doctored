//! Edit ▸ Paste of a LAYER clipboard — `dr_doc_paste_layers` at 8
//! bits (through the FFI) and `PrecisionDocument::paste_layers` at 16 bits
//! and 32-bit float (through the Rust API).
//!
//! The oracle is the rule as `doc_paste_layers`' module doc states it,
//! written out here: every clip record lands, in order, at the slot, its
//! level raised by the slot's; every property rides (pixels and masks are the
//! SAME allocations); every id is fresh; each link id is re-numbered to an
//! unused one; every root joins the slot's clip run; a placement offset moves
//! exactly what the Move tool moves; and a paste over a hidden empty layer
//! projects exactly what the clip projects.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, Layer as ByteLayer, LayerKind};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_layer_clipboard::dr_doc_paste_layers;
use doctored_core::ffi_precision::dr_precision_image_storage_identity;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::layer_raster::LayerRaster;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::PasteProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::LayerStyle;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";
const ABOVE: u32 = 0;
const INTO_GROUP: u32 = 2;
const PROFILE_REFUSAL: &str = "pasted layers must be in the document's profile; convert them first";
const DEPTH_REFUSAL: &str = "pasted layers must be at the document's depth; convert them first";
const OFFSET: (i32, i32) = (37, -11);

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

// ================================================================= 8 bits ==

/// `dr_doc_paste_layers`, its refusal text as `Err`.
fn paste8(
    doc: &DrDocument,
    clip: &DrDocument,
    anchor: usize,
    position: u32,
    offset: (i32, i32),
) -> Result<DrDocument, String> {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_paste_layers(
            doc, clip, anchor, position, offset.0, offset.1, false, &mut err,
        )
    };
    if let Some(message) = owned(err) {
        assert!(out.is_null());
        return Err(message);
    }
    assert!(!out.is_null(), "a paste is never a silent no-op");
    Ok(*unsafe { Box::from_raw(out) })
}

fn image(w: u32, h: u32, v: u8) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            v.wrapping_add((x * 13) as u8),
            255 - v,
            v.wrapping_add((y * 29) as u8),
            if (x + y).is_multiple_of(3) { 255 } else { 180 },
        ])
    })
}

fn names8(doc: &DrDocument) -> Vec<&str> {
    doc.layers.iter().map(|l| l.name.as_str()).collect()
}

fn depths8(doc: &DrDocument) -> Vec<u16> {
    doc.layers.iter().map(|l| l.depth).collect()
}

/// Bottom-first `Bg`, `Masked` (masked, Multiply 50 %, label 2, link 1),
/// `Clip` (clipped, styled, link 1), `G` (the group of the two, 80 %),
/// `Adjust` (an invert adjustment) on a 10×8 sRGB canvas.
fn source8() -> DrDocument {
    let mut doc = DrDocument::from_pixels(image(10, 8, 10));
    doc.layers[0].name = "Bg".into();
    let doc = doc
        .adding_image_layer(0, image(5, 4, 60), "Masked")
        .unwrap()
        .adding_image_layer(1, image(4, 3, 90), "Clip")
        .unwrap()
        .adding_image_layer(2, image(3, 3, 120), "Adjust")
        .unwrap();
    let (mut doc, group, _, _) = doc.group_layers(&[1, 2], "G").expect("the group");
    assert_eq!(group, 3);
    {
        let masked = &mut doc.layers[1];
        masked.offset = (1, 2);
        masked.mask = Some(Arc::new(GrayImage::from_fn(5, 4, |x, y| {
            Luma([(x * 50 + y * 7) as u8])
        })));
        masked.mask_properties = MaskProperties::at((1, 2));
        masked.opacity = 0.5;
        masked.blend = BlendMode::Multiply;
        masked.label = 2;
        masked.link = 1;
    }
    doc.layers[2].offset = (2, 1);
    doc.layers[2].clipped = true;
    doc.layers[2].link = 1;
    doc.layers[3].opacity = 0.8;
    doc.layers[4].meta = Some(json!({"type":"adjust","op":"invert","params":{}}).to_string());
    let style = LayerStyle::from_json(
        r##"{"effects":[{"type":"drop_shadow","use_global_light":true,"distance":2,"size":1,
            "spread":0,"opacity":0.5,"color":"#000000"}]}"##,
    )
    .unwrap();
    doc.set_layer_style(2, Some(Arc::new(style))).unwrap()
}

/// The layer clipboard of `G` and `Adjust`: [Masked, Clip, G, Adjust].
fn clip8() -> DrDocument {
    let clip = source8().duplicate_to_new_document(&[3, 4]).unwrap();
    assert_eq!(names8(&clip), ["Masked", "Clip", "G", "Adjust"]);
    clip
}

fn same_layer8(a: &ByteLayer, b: &ByteLayer) {
    assert_eq!(a.name, b.name);
    assert!(
        LayerRaster::ptr_eq(&a.pixels, &b.pixels),
        "{} pixels shared",
        a.name
    );
    assert_eq!(a.offset, b.offset, "{} offset", a.name);
    assert_eq!(a.opacity, b.opacity);
    assert_eq!(a.blend, b.blend);
    assert_eq!(a.visible, b.visible);
    assert_eq!(a.kind, b.kind);
    assert_eq!(a.label, b.label);
    assert_eq!(a.meta, b.meta);
    match (&a.mask, &b.mask) {
        (Some(x), Some(y)) => assert!(Arc::ptr_eq(x, y), "{} mask shared", a.name),
        (None, None) => {}
        _ => panic!("{} mask presence", a.name),
    }
    assert!(a.mask_properties == b.mask_properties);
    match (&a.style, &b.style) {
        (Some(x), Some(y)) => assert!(Arc::ptr_eq(x, y), "{} style shared", a.name),
        (None, None) => {}
        _ => panic!("{} style presence", a.name),
    }
}

#[test]
fn byte_paste_keeps_structure_into_another_canvas() {
    let clip = clip8();
    let mut dest = DrDocument::from_pixels(image(14, 9, 200));
    dest.layers[0].name = "Base".into();
    let out = paste8(&dest, &clip, 0, ABOVE, (0, 0)).unwrap();
    assert_eq!((out.width, out.height), (14, 9), "the destination canvas");
    assert_eq!(names8(&out), ["Base", "Masked", "Clip", "G", "Adjust"]);
    assert_eq!(depths8(&out), [0, 1, 1, 0, 0]);
    for (i, source) in clip.layers.iter().enumerate() {
        let pasted = &out.layers[i + 1];
        same_layer8(source, pasted);
        assert_eq!(pasted.clipped, source.clipped, "{} clip", source.name);
    }
    assert_eq!(out.layers[3].kind, LayerKind::Group);
    assert!(out.layers[2].clipped && !out.layers[1].clipped);
    // The destination is untouched apart from the splice.
    assert!(LayerRaster::ptr_eq(
        &out.layers[0].pixels,
        &dest.layers[0].pixels
    ));
    assert_eq!(out.layers[0].id, dest.layers[0].id);
}

#[test]
fn byte_ids_are_fresh_across_two_pastes() {
    let clip = clip8();
    let dest = DrDocument::from_pixels(image(10, 8, 1));
    let once = paste8(&dest, &clip, 0, ABOVE, (0, 0)).unwrap();
    let twice = paste8(&once, &clip, 4, ABOVE, (0, 0)).unwrap();
    assert_eq!(twice.layers.len(), 9);
    let source: Vec<u64> = clip.layers.iter().map(|l| l.id).collect();
    let mut ids: Vec<u64> = twice.layers.iter().map(|l| l.id).collect();
    assert!(ids[1..].iter().all(|id| *id != 0 && !source.contains(id)));
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 9, "every id distinct");
}

#[test]
fn byte_links_are_renumbered_past_the_destination() {
    let clip = clip8();
    assert_eq!(clip.layers[0].link, clip.layers[1].link);
    assert_ne!(clip.layers[0].link, 0);
    let mut dest = DrDocument::from_pixels(image(10, 8, 1))
        .adding_image_layer(0, image(2, 2, 3), "Partner")
        .unwrap();
    dest.layers[0].link = 1;
    dest.layers[1].link = 1;
    let out = paste8(&dest, &clip, 1, ABOVE, (0, 0)).unwrap();
    let links: Vec<u32> = out.layers.iter().map(|l| l.link).collect();
    assert_eq!(
        links,
        [1, 1, 2, 2, 0, 0],
        "the pasted link group is a new one"
    );
}

#[test]
fn byte_roots_join_a_clip_run() {
    let clip = clip8();
    let mut dest = DrDocument::from_pixels(image(10, 8, 1))
        .adding_image_layer(0, image(4, 4, 7), "A")
        .unwrap();
    dest.layers[1].clipped = true;
    let out = paste8(&dest, &clip, 0, ABOVE, (0, 0)).unwrap();
    assert_eq!(
        names8(&out),
        ["Background", "Masked", "Clip", "G", "Adjust", "A"]
    );
    let clipped: Vec<bool> = out.layers.iter().map(|l| l.clipped).collect();
    // Roots (G, Adjust) join; the children keep their own flags.
    assert_eq!(clipped, [false, false, true, true, true, true]);
}

#[test]
fn byte_into_group_lands_as_the_top_children() {
    let clip = clip8();
    let mut dest = DrDocument::from_pixels(image(10, 8, 1));
    dest.layers[0].name = "X".into();
    let (dest, group, _, _) = dest.group_layers(&[0], "DG").unwrap();
    assert_eq!(group, 1);
    let out = paste8(&dest, &clip, 1, INTO_GROUP, (0, 0)).unwrap();
    assert_eq!(names8(&out), ["X", "Masked", "Clip", "G", "Adjust", "DG"]);
    assert_eq!(depths8(&out), [1, 2, 2, 1, 1, 0]);
}

#[test]
fn byte_refusals_are_named() {
    let clip = clip8();
    // Ten nested groups put the innermost one's children at level 10.
    let mut dest = DrDocument::from_pixels(image(10, 8, 1));
    for _ in 0..10 {
        let last = dest.layers.len() - 1;
        dest = dest.group_layers(&[last], "N").unwrap().0;
    }
    assert_eq!(dest.layers[1].depth, 9);
    let err = paste8(&dest, &clip, 1, INTO_GROUP, (0, 0))
        .err()
        .expect("a refusal");
    assert_eq!(
        err,
        "pasting these layers here would nest them 11 levels deep; the limit is 10"
    );
    let mut p3 = DrDocument::from_pixels(image(10, 8, 1));
    p3.profile = IccProfile::display_p3();
    assert_eq!(
        paste8(&p3, &clip, 0, ABOVE, (0, 0))
            .err()
            .expect("a refusal"),
        PROFILE_REFUSAL
    );
    let plain = DrDocument::from_pixels(image(10, 8, 1));
    assert!(paste8(&plain, &clip, 5, ABOVE, (0, 0))
        .err()
        .expect("a refusal")
        .contains("out of range"));
    assert!(paste8(&plain, &clip, 0, INTO_GROUP, (0, 0))
        .err()
        .expect("a refusal")
        .contains("into_group requires a group"));
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_doc_paste_layers(&plain, ptr::null(), 0, ABOVE, 0, 0, false, &mut err) }
            .is_null()
    );
    assert!(owned(err).is_some(), "a NULL clip is named");
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_doc_paste_layers(&plain, &clip, 0, 9, 0, 0, false, &mut err) }.is_null());
    assert!(owned(err).unwrap().contains("position"));
}

#[test]
fn byte_paste_over_a_hidden_empty_layer_projects_the_clip() {
    let clip = clip8();
    let mut dest = DrDocument::from_pixels(RgbaImage::new(10, 8));
    dest.layers[0].visible = false;
    let out = paste8(&dest, &clip, 0, ABOVE, (0, 0)).unwrap();
    let want = clip.flattened().unwrap();
    let got = out.flattened().unwrap();
    assert_eq!(got.as_raw(), want.as_raw());
}

/// A 20×20 clip: `G` (a group with a LINKED mask) holding `Child`, `Linked`
/// (a raster with a linked mask) and `Unlinked` (a raster with an unlinked
/// mask).
fn offset_clip8() -> DrDocument {
    let mut clip = DrDocument::from_pixels(image(6, 6, 5));
    clip.width = 20;
    clip.height = 20;
    clip.layers[0].name = "Child".into();
    clip.layers[0].offset = (1, 1);
    let (clip, _, _, _) = clip.group_layers(&[0], "G").unwrap();
    let mut clip = clip
        .adding_image_layer(1, image(5, 5, 40), "Linked")
        .unwrap()
        .adding_image_layer(2, image(5, 5, 80), "Unlinked")
        .unwrap();
    let plane = |v: u8| Some(Arc::new(GrayImage::from_pixel(3, 3, Luma([v]))));
    clip.layers[1].mask = plane(200);
    clip.layers[1].mask_properties = MaskProperties::at((2, 2));
    clip.layers[1].offset = (2, 2);
    clip.layers[2].offset = (5, 6);
    clip.layers[2].mask = plane(150);
    clip.layers[2].mask_properties = MaskProperties::at((4, 4));
    clip.layers[3].offset = (7, 8);
    clip.layers[3].mask = plane(100);
    let mut unlinked = MaskProperties::at((3, 3));
    unlinked.linked = false;
    clip.layers[3].mask_properties = unlinked;
    assert_eq!(names8(&clip), ["Child", "G", "Linked", "Unlinked"]);
    clip
}

#[test]
fn byte_placement_offset_moves_what_the_move_tool_moves() {
    let clip = offset_clip8();
    let dest = DrDocument::from_pixels(image(30, 30, 9));
    let at_zero = paste8(&dest, &clip, 0, ABOVE, (0, 0)).unwrap();
    let moved = paste8(&dest, &clip, 0, ABOVE, OFFSET).unwrap();
    assert_eq!(moved.layers.len(), 5, "one result document");
    let shift = |p: (i32, i32)| (p.0 + OFFSET.0, p.1 + OFFSET.1);
    for i in 1..5 {
        let (a, b) = (&at_zero.layers[i], &moved.layers[i]);
        match a.name.as_str() {
            "Child" | "Linked" | "Unlinked" => assert_eq!(b.offset, shift(a.offset), "{}", a.name),
            _ => {}
        }
        let mask_moves = a.mask.is_some() && a.mask_properties.linked;
        let want = if mask_moves {
            shift(a.mask_properties.offset)
        } else {
            a.mask_properties.offset
        };
        assert_eq!(b.mask_properties.offset, want, "{} mask", a.name);
        assert!(LayerRaster::ptr_eq(&a.pixels, &b.pixels));
    }
    assert_eq!(
        moved.layers[4].mask_properties.offset,
        (3, 3),
        "unlinked stays"
    );
    assert_eq!(
        moved.layers[2].mask_properties.offset,
        shift((2, 2)),
        "group mask"
    );
    // The same as the (0, 0) paste followed by the Move tool.
    let then = at_zero
        .move_layers(&[1, 2, 3, 4], OFFSET.0, OFFSET.1)
        .unwrap();
    for (a, b) in then.layers.iter().zip(&moved.layers) {
        assert_eq!(a.offset, b.offset, "{}", a.name);
        assert!(a.mask_properties == b.mask_properties, "{}", a.name);
    }
}

// ============================================================ high depth ==

fn u16_image(w: u32, h: u32, seed: u16, profile: Arc<IccProfile>) -> PrecisionImage {
    let px: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            let i = i as u16;
            [
                seed.wrapping_mul(31).wrapping_add(i * 257),
                65535 - seed.wrapping_add(i * 3),
                seed.wrapping_add(i * 1031),
                if i.is_multiple_of(3) { 65535 } else { 40000 },
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, px, profile, None).unwrap()
}

fn mask(w: u32, h: u32, offset: (i32, i32), linked: bool) -> PrecisionMask {
    let samples: Vec<u16> = (0..w * h).map(|i| (i * 4099) as u16).collect();
    let mut properties = MaskProperties::at(offset);
    properties.linked = linked;
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, samples, None).unwrap(),
        properties,
        true,
    )
    .unwrap()
}

fn doc16(w: u32, h: u32, profile: Arc<IccProfile>, layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        w,
        h,
        profile,
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

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
    if let Some(message) = owned(err) {
        panic!("{request}: {message}");
    }
    assert!(!out.is_null(), "{request} changed nothing");
    *unsafe { Box::from_raw(out) }
}

fn names16(doc: &PrecisionDocument) -> Vec<String> {
    (0..doc.layer_count())
        .map(|i| doc.layer(i).unwrap().name().to_string())
        .collect()
}

fn pixel_address(doc: &PrecisionDocument, index: usize) -> usize {
    let source = doc.layer_source(index).expect("a raster record");
    unsafe { dr_precision_image_storage_identity(source) as usize }
}

fn paste16(
    doc: &PrecisionDocument,
    clip: &PrecisionDocument,
    anchor: usize,
    position: InsertPosition,
    offset: (i32, i32),
) -> Result<PrecisionDocument, String> {
    doc.paste_layers(
        clip,
        anchor,
        position,
        offset,
        PasteProfile::Convert,
        LIMIT,
        0,
    )
    .map(|(document, _)| document)
}

/// The 8-bit source's shape at 16 bits: `Bg`, `G` holding `Masked` and a
/// clipped, styled `Clip`, then `Adjust`; `Masked` and `Clip` linked.
fn source16() -> PrecisionDocument {
    let srgb = IccProfile::srgb();
    let style = LayerStyle::from_json(
        r##"{"effects":[{"type":"drop_shadow","use_global_light":true,"distance":2,"size":1,
            "spread":0,"opacity":0.5,"color":"#000000"}]}"##,
    )
    .unwrap();
    let masked = Layer::raster(
        "Masked",
        u16_image(5, 4, 60, srgb.clone()),
        (1, 2),
        0.5,
        true,
    )
    .unwrap()
    .with_mask(Some(mask(5, 4, (1, 2), true)))
    .with_blend_mode(BlendMode::Multiply)
    .unwrap()
    .with_label(2);
    let clipped = Layer::raster("Clip", u16_image(4, 3, 90, srgb.clone()), (2, 1), 1.0, true)
        .unwrap()
        .with_clipped(true)
        .with_layer_style(Some(&style), (10, 8))
        .unwrap();
    let group = Layer::isolated_normal("G", vec![masked, clipped], 0.8, true).unwrap();
    let description =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    let doc = doc16(
        10,
        8,
        srgb.clone(),
        vec![
            Layer::raster("Bg", u16_image(10, 8, 10, srgb.clone()), (0, 0), 1.0, true).unwrap(),
            group,
            Layer::adjustment("Adjust", &description, 1.0, true, &srgb).unwrap(),
        ],
    );
    edit(&doc, json!({"op":"link_layers","layers":[1,2]}))
}

fn clip16() -> PrecisionDocument {
    let clip = source16().duplicate_to_new_document(&[3, 4]).unwrap();
    assert_eq!(names16(&clip), ["Masked", "Clip", "G", "Adjust"]);
    clip
}

#[test]
fn sixteen_bit_paste_keeps_structure_and_rebinds_the_profile() {
    let clip = clip16();
    // A DIFFERENT profile handle with the same bytes, on another canvas.
    let profile = IccProfile::srgb();
    let dest = doc16(
        14,
        9,
        profile.clone(),
        vec![Layer::raster(
            "Base",
            u16_image(14, 9, 3, profile.clone()),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()],
    );
    let out = paste16(&dest, &clip, 0, InsertPosition::Above, (0, 0)).unwrap();
    assert_eq!(out.dimensions(), (14, 9));
    assert_eq!(names16(&out), ["Base", "Masked", "Clip", "G", "Adjust"]);
    assert!(Arc::ptr_eq(out.profile(), dest.profile()));
    for i in 0..clip.layer_count() {
        let (a, b) = (clip.layer(i).unwrap(), out.layer(i + 1).unwrap());
        assert_eq!(a.name(), b.name());
        assert_eq!(a.opacity(), b.opacity());
        assert_eq!(a.visible(), b.visible());
        assert_eq!(a.clipped(), b.clipped());
        assert_eq!(a.offset(), b.offset());
        assert_eq!(a.label(), b.label());
        assert_ne!(a.id(), b.id());
        assert_eq!(a.style().is_some(), b.style().is_some());
        match (a.mask(), b.mask()) {
            (Some(x), Some(y)) => {
                assert_eq!(x.plane().samples(), y.plane().samples());
                assert!(x.properties() == y.properties());
            }
            (None, None) => {}
            _ => panic!("{} mask presence", a.name()),
        }
        if a.source().is_some() {
            assert_eq!(pixel_address(&clip, i), pixel_address(&out, i + 1));
            assert!(Arc::ptr_eq(b.source().unwrap().profile(), dest.profile()));
        }
    }
    assert!(out.layer(3).unwrap().children().is_some());
    assert_eq!(
        out.adjustment_description(4),
        clip.adjustment_description(3),
        "the adjustment travels"
    );
}

#[test]
fn sixteen_bit_ids_links_and_into_group() {
    let clip = clip16();
    let srgb = IccProfile::srgb();
    let dest = doc16(
        10,
        8,
        srgb.clone(),
        vec![
            Layer::raster("A", u16_image(2, 2, 1, srgb.clone()), (0, 0), 1.0, true).unwrap(),
            Layer::isolated_normal(
                "DG",
                vec![
                    Layer::raster("X", u16_image(2, 2, 2, srgb.clone()), (0, 0), 1.0, true)
                        .unwrap(),
                ],
                1.0,
                true,
            )
            .unwrap(),
        ],
    );
    let dest = edit(&dest, json!({"op":"link_layers","layers":[0,1]}));
    let dest_link = dest.layer(0).unwrap().link();
    assert_ne!(dest_link, 0);
    let out = paste16(&dest, &clip, 2, InsertPosition::IntoGroup, (0, 0)).unwrap();
    assert_eq!(
        names16(&out),
        ["A", "X", "Masked", "Clip", "G", "Adjust", "DG"]
    );
    let (masked, clipped) = (out.layer(2).unwrap(), out.layer(3).unwrap());
    assert_eq!(masked.link(), clipped.link());
    assert!(masked.link() != 0 && masked.link() != dest_link);
    assert_eq!(out.layer(0).unwrap().link(), dest_link);
    let twice = paste16(&out, &clip, 6, InsertPosition::Above, (0, 0)).unwrap();
    let mut ids: Vec<u64> = (0..twice.layer_count())
        .map(|i| twice.layer(i).unwrap().id())
        .collect();
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "every id distinct across two pastes");
}

#[test]
fn sixteen_bit_profile_refusal_and_projection() {
    let clip = clip16();
    let p3 = IccProfile::display_p3();
    let other = doc16(
        10,
        8,
        p3.clone(),
        vec![Layer::raster("P", u16_image(2, 2, 1, p3), (0, 0), 1.0, true).unwrap()],
    );
    assert_eq!(
        paste16(&other, &clip, 0, InsertPosition::Above, (0, 0))
            .err()
            .expect("a refusal"),
        PROFILE_REFUSAL
    );
    let srgb = IccProfile::srgb();
    let empty = PrecisionImage::from_rgba16(10, 8, vec![[0; 4]; 80], srgb.clone(), None).unwrap();
    let dest = doc16(
        10,
        8,
        srgb,
        vec![Layer::raster("Empty", empty, (0, 0), 1.0, false).unwrap()],
    );
    let out = paste16(&dest, &clip, 0, InsertPosition::Above, (0, 0)).unwrap();
    let want = clip.render(LIMIT, 0, None).unwrap();
    let got = out.render(LIMIT, 0, None).unwrap();
    assert!(got.same_samples(&want), "the paste projects the clip");
}

#[test]
fn promoted_byte_clip_pastes_at_sixteen_bits_exactly() {
    let clip = clip8();
    let promoted = clip.promote_structure(LIMIT, 0, None).unwrap();
    let srgb = IccProfile::srgb();
    let dest = doc16(
        10,
        8,
        srgb.clone(),
        vec![Layer::raster("Base", u16_image(10, 8, 4, srgb), (0, 0), 1.0, true).unwrap()],
    );
    let out = paste16(&dest, &promoted, 0, InsertPosition::Above, (0, 0)).unwrap();
    assert_eq!(names16(&out), ["Base", "Masked", "Clip", "G", "Adjust"]);
    let byte = clip.layers[0].pixels.to_contiguous().unwrap();
    let typed = out.layer_source(1).unwrap().to_rgba16().unwrap();
    for (b, t) in byte.pixels().zip(typed.iter()) {
        for c in 0..4 {
            assert_eq!(u32::from(t[c]), u32::from(b[c]) * 257);
        }
    }
}

fn float_source() -> Option<PrecisionDocument> {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return None;
    }
    let base = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    assert!(base.is_float());
    let exposure = json!({"type":"adjust","evaluation":"precision_v1","op":"exposure",
                          "params":{"exposure":0.25}})
    .to_string();
    let adjusted = base
        .add_adjustment("Exposure", &exposure, 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let second = adjusted.duplicate_layer(0).unwrap().unwrap();
    let second = edit(
        &second,
        json!({"op":"rename_layer","layer":1,"name":"Second"}),
    );
    let grouped = edit(
        &second,
        json!({"op":"group_layers","layers":[1],"name":"G","pass_through":false}),
    );
    assert_eq!(names16(&grouped), ["Background", "Second", "G", "Exposure"]);
    Some(grouped)
}

#[test]
fn float_paste_is_admitted_at_its_allocation() {
    let Some(source) = float_source() else {
        return;
    };
    let clip = source.duplicate_to_new_document(&[2, 3]).unwrap();
    let base = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    let allocation = base
        .paste_layers_allocation(&clip, PasteProfile::Convert)
        .unwrap();
    let need = base.retained_bytes() + allocation;
    let paste = |limit: u64| {
        base.paste_layers(
            &clip,
            0,
            InsertPosition::Above,
            (0, 0),
            PasteProfile::Convert,
            limit,
            0,
        )
    };
    let err = paste(need - 1).err().expect("a refusal");
    assert!(err.contains("working limit"), "{err}");
    let (out, report) = paste(need).unwrap();
    assert_eq!(
        report.json(),
        r#"{"blend_reset":[],"effect_blend_reset":[],"rasterized":[]}"#,
        "a float clip in the document's own profile promotes nothing"
    );
    assert!(out.is_float());
    assert_eq!(out.float_refusal(), None, "the float renderer accepts it");
    assert_eq!(out.tone_map(), base.tone_map());
    assert_eq!(names16(&out), ["Background", "Second", "G", "Exposure"]);
    assert_eq!(pixel_address(&out, 1), pixel_address(&clip, 0));
}

#[test]
fn float_and_sixteen_bit_refuse_each_other_and_narrowing_chains() {
    let Some(source) = float_source() else {
        return;
    };
    let clip = source.duplicate_to_new_document(&[2, 3]).unwrap();
    let narrowed_dest = source.narrow_structure_to_u16(LIMIT, 0, None).unwrap();
    assert!(!narrowed_dest.is_float());
    assert_eq!(
        paste16(&narrowed_dest, &clip, 0, InsertPosition::Above, (0, 0))
            .err()
            .expect("a refusal"),
        DEPTH_REFUSAL
    );
    // The other direction, 16-bit layers into the float document, is no
    // longer a refusal: it promotes (`layer_clipboard_float.rs`).
    let clip16 = clip.narrow_structure_to_u16(LIMIT, 0, None).unwrap();
    let out = paste16(&narrowed_dest, &clip16, 3, InsertPosition::Above, (0, 0)).unwrap();
    assert_eq!(
        names16(&out),
        [
            "Background",
            "Second",
            "G",
            "Exposure",
            "Second",
            "G",
            "Exposure"
        ]
    );
    assert_eq!(pixel_address(&out, 4), pixel_address(&clip16, 0));
}

/// The 8-bit offset clip at 16 bits.
fn offset_clip16() -> PrecisionDocument {
    let srgb = IccProfile::srgb();
    let child =
        Layer::raster("Child", u16_image(6, 6, 5, srgb.clone()), (1, 1), 1.0, true).unwrap();
    let group = Layer::isolated_normal("G", vec![child], 1.0, true)
        .unwrap()
        .with_mask(Some(mask(3, 3, (2, 2), true)));
    let linked = Layer::raster(
        "Linked",
        u16_image(5, 5, 40, srgb.clone()),
        (5, 6),
        1.0,
        true,
    )
    .unwrap()
    .with_mask(Some(mask(3, 3, (4, 4), true)));
    let unlinked = Layer::raster(
        "Unlinked",
        u16_image(5, 5, 80, srgb.clone()),
        (7, 8),
        1.0,
        true,
    )
    .unwrap()
    .with_mask(Some(mask(3, 3, (3, 3), false)));
    doc16(20, 20, srgb, vec![group, linked, unlinked])
}

fn mask_offset(doc: &PrecisionDocument, i: usize) -> (i32, i32) {
    doc.layer(i).unwrap().mask().unwrap().properties().offset
}

#[test]
fn sixteen_bit_placement_offset_moves_what_typed_move_moves() {
    let clip = offset_clip16();
    let srgb = IccProfile::srgb();
    let dest = doc16(
        30,
        30,
        srgb.clone(),
        vec![Layer::raster("Base", u16_image(30, 30, 9, srgb), (0, 0), 1.0, true).unwrap()],
    );
    let at_zero = paste16(&dest, &clip, 0, InsertPosition::Above, (0, 0)).unwrap();
    let moved = paste16(&dest, &clip, 0, InsertPosition::Above, OFFSET).unwrap();
    assert_eq!(
        names16(&moved),
        ["Base", "Child", "G", "Linked", "Unlinked"]
    );
    let shift = |p: (i32, i32)| (p.0 + OFFSET.0, p.1 + OFFSET.1);
    for i in [1, 3, 4] {
        assert_eq!(
            moved.layer(i).unwrap().offset(),
            shift(at_zero.layer(i).unwrap().offset()),
            "raster {i}"
        );
    }
    assert_eq!(
        mask_offset(&moved, 2),
        shift((2, 2)),
        "the group's linked mask"
    );
    assert_eq!(mask_offset(&moved, 3), shift((4, 4)), "a linked mask");
    assert_eq!(mask_offset(&moved, 4), (3, 3), "an unlinked mask stays");
    // The same as the (0, 0) paste followed by typed Move of the block.
    let request = json!({"op":"affine_set","layers":[1,2,3,4],
        "matrix":[1.0,0.0,0.0,1.0,f64::from(OFFSET.0),f64::from(OFFSET.1)],"filter":"nearest"});
    let then = at_zero
        .geometry(
            &GeometryEdit::parse(&request.to_string()).unwrap(),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    for i in 0..moved.layer_count() {
        let (a, b) = (then.layer(i).unwrap(), moved.layer(i).unwrap());
        assert_eq!(a.offset(), b.offset(), "{}", a.name());
        assert_eq!(
            a.mask().map(|m| m.properties().offset),
            b.mask().map(|m| m.properties().offset),
            "{} mask",
            a.name()
        );
    }
}
