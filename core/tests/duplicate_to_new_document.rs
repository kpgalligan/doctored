//! Layer ▸ Duplicate to New Document: the given entries, each with its
//! whole subtree, copied into a NEW document — at 8 bits through
//! `dr_doc_duplicate_to_new_document`, at 16 bits and 32-bit float through
//! the `{op:"duplicate_to_new_document"}` request on `dr_precision_doc_edit`.
//!
//! The oracle is the rule as the spec states it, written out here rather than
//! read off the implementation: the document chrome that belongs to the
//! PICTURE (canvas, profile, resolution, evaluation policy, tone map, global
//! light) is equal to the source's; the chrome that belongs to the SOURCE
//! (channels, guides, comps, metadata) is gone; each root lands at the top
//! level with its subtree intact; pixels and masks are the SAME allocations;
//! every id is fresh; a root keeps its clip only over a copied base with no
//! foreign root between; a link id survives only when two copies share it.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument, LayerKind};
use doctored_core::doc_comps::CompRequest;
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_guide::GuideOrientation;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_group::dr_doc_duplicate_to_new_document;
use doctored_core::ffi_precision::dr_precision_image_storage_identity;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::layer_raster::LayerRaster;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use doctored_core::style::{GlobalLight, LayerStyle};
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

// ================================================================= 8 bits ==

/// `dr_doc_duplicate_to_new_document` over `idx`; `None` for NULL.
fn duplicate8(doc: &DrDocument, idx: &[usize]) -> Option<DrDocument> {
    let out = unsafe { dr_doc_duplicate_to_new_document(doc, idx.as_ptr(), idx.len()) };
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

fn names8(doc: &DrDocument) -> Vec<&str> {
    doc.layers.iter().map(|l| l.name.as_str()).collect()
}

fn find8<'a>(doc: &'a DrDocument, name: &str) -> &'a doctored_core::doc::Layer {
    doc.layers
        .iter()
        .find(|l| l.name == name)
        .unwrap_or_else(|| panic!("{name} is present"))
}

fn image(w: u32, h: u32, v: u8) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba([v, 255 - v, v / 2, 255]))
}

/// Bottom-first: `Bg`, `Masked`, `Clip1` (clipped), `X` (clipped, inside
/// `G`), `G` (a clipped group), `Clip2` (clipped), `Text`, `Adjust` — so the
/// clip base of `Clip1`, `G` and `Clip2` is `Masked`. `Bg`/`Masked` share
/// link 1, `Clip2`/`Text` link 2. One channel, a guide, a moved ruler origin,
/// one comp, metadata, Display P3, 300 x 150 ppi, linear light and a moved
/// global light, so every carried and every dropped field has something to
/// say.
fn fixture8() -> DrDocument {
    fixture8_in(Evaluation::Encoded)
}

/// [`fixture8`] under `evaluation`. Linear light refuses a layer style, so
/// the style rides only on the encoded fixture.
fn fixture8_in(evaluation: Evaluation) -> DrDocument {
    let mut doc = DrDocument::from_pixels(image(8, 6, 10));
    doc.layers[0].name = "Bg".into();
    for (i, name) in ["Masked", "Clip1", "X", "Clip2", "Text", "Adjust"]
        .iter()
        .enumerate()
    {
        doc = doc
            .adding_image_layer(i, image(5, 4, 40 + i as u8 * 20), name)
            .expect("a layer");
    }
    let (grouped, group, _, _) = doc.group_layers(&[3], "G").expect("the group");
    assert_eq!(group, 4);
    let mut doc = grouped;
    assert_eq!(
        names8(&doc),
        ["Bg", "Masked", "Clip1", "X", "G", "Clip2", "Text", "Adjust"]
    );
    let style = LayerStyle::from_json(
        r##"{"effects":[{"type":"drop_shadow","use_global_light":true,"distance":3,"size":2,
            "spread":0,"opacity":0.5,"color":"#000000"}]}"##,
    )
    .expect("a style");
    if evaluation == Evaluation::Encoded {
        doc = doc
            .set_layer_style(1, Some(Arc::new(style)))
            .expect("the style");
    }
    {
        let masked = &mut doc.layers[1];
        masked.offset = (-3, 4);
        masked.mask = Some(Arc::new(GrayImage::from_pixel(5, 4, image::Luma([77]))));
        masked.opacity = 0.5;
        masked.blend = BlendMode::Multiply;
        masked.label = 3;
        masked.locks = 1;
        masked.visible = false;
    }
    doc.layers[2].clipped = true;
    doc.layers[3].clipped = true;
    doc.layers[4].clipped = true;
    doc.layers[4].open = false;
    doc.layers[5].clipped = true;
    doc.layers[0].link = 1;
    doc.layers[1].link = 1;
    doc.layers[5].link = 2;
    doc.layers[6].link = 2;
    doc.layers[6].meta = Some(json!({"type":"text","version":1,"payload":"hello"}).to_string());
    doc.layers[7].meta = Some(json!({"type":"adjust","op":"invert","params":{}}).to_string());

    doc.profile = IccProfile::display_p3();
    let mut doc = match evaluation {
        Evaluation::Encoded => doc,
        policy => doc
            .with_evaluation(policy)
            .expect("the policy")
            .expect("a change"),
    };
    doc.global_light = GlobalLight {
        angle: 30.0,
        altitude: 60.0,
    };
    doc.resolution = Resolution { x: 300.0, y: 150.0 };
    doc.metadata = Metadata {
        exif: Some(Arc::from(&b"Exif\0\0MM"[..])),
        xmp: None,
        iptc: None,
    };
    let plane = vec![9u8; 8 * 6];
    doc = doc
        .add_channel("Alpha 1", &plane, 8, 6, [255, 0, 0], 0.5)
        .expect("a channel");
    doc = doc
        .add_guide(GuideOrientation::Horizontal, 2.0)
        .expect("a guide");
    doc = doc.set_ruler_origin(1.0, 2.0).expect("the origin");
    let request =
        CompRequest::parse(&json!({"op":"layer_comp","action":"save","name":"Shot"}).to_string())
            .expect("a comp request");
    doc.edit_layer_comps(&request)
        .expect("the comp saves")
        .expect("a change")
}

#[test]
fn byte_document_chrome_follows_the_picture_not_the_source() {
    let doc = fixture8_in(Evaluation::LinearLight);
    let out = duplicate8(&doc, &[1]).expect("a duplicate");
    assert_eq!((out.width, out.height), (doc.width, doc.height));
    assert!(Arc::ptr_eq(&out.profile, &doc.profile), "the same profile");
    assert_eq!(out.profile.bytes(), doc.profile.bytes());
    assert_eq!(out.resolution, doc.resolution);
    assert_eq!(out.evaluation(), Evaluation::LinearLight);
    assert_eq!(out.global_light, doc.global_light);
    assert!(out.channels.is_empty(), "channels are not carried");
    assert!(out.guides.is_empty(), "guides are not carried");
    assert_eq!(out.ruler_origin, (0.0, 0.0));
    assert!(out.layer_comps().is_empty(), "comps are not carried");
    assert_eq!(out.metadata, Metadata::default());
}

#[test]
fn byte_roots_land_at_the_top_level_with_their_subtrees() {
    let doc = fixture8();
    // `X` is inside `G`, so the set is `Masked` and `G`.
    let out = duplicate8(&doc, &[4, 3, 1]).expect("a duplicate");
    assert_eq!(names8(&out), ["Masked", "X", "G"]);
    let depths: Vec<u16> = out.layers.iter().map(|l| l.depth).collect();
    assert_eq!(depths, [0, 1, 0]);
    assert_eq!(out.layers[2].kind, LayerKind::Group);
    assert!(!out.layers[2].open, "the group's open state is carried");

    // Every property, verbatim.
    let (a, b) = (find8(&doc, "Masked"), find8(&out, "Masked"));
    assert!(
        LayerRaster::ptr_eq(&a.pixels, &b.pixels),
        "pixels are shared"
    );
    assert!(
        Arc::ptr_eq(a.mask.as_ref().unwrap(), b.mask.as_ref().unwrap()),
        "the mask is shared"
    );
    assert!(Arc::ptr_eq(
        a.style.as_ref().unwrap(),
        b.style.as_ref().unwrap()
    ));
    assert_eq!(b.offset, (-3, 4), "an off-canvas offset is kept");
    assert_eq!(b.name, "Masked", "no \" copy\" suffix");
    assert_eq!(b.opacity, 0.5);
    assert_eq!(b.blend, BlendMode::Multiply);
    assert!(!b.visible);
    assert_eq!((b.label, b.locks), (3, 1));

    // Fresh ids, unique, none of them the source's.
    let source: Vec<u64> = doc.layers.iter().map(|l| l.id).collect();
    let mut fresh: Vec<u64> = out.layers.iter().map(|l| l.id).collect();
    assert!(fresh.iter().all(|id| *id != 0 && !source.contains(id)));
    fresh.sort_unstable();
    fresh.dedup();
    assert_eq!(fresh.len(), out.layers.len());
}

#[test]
fn byte_text_and_adjustment_layers_stay_described() {
    let doc = fixture8();
    let out = duplicate8(&doc, &[6, 7]).expect("a duplicate");
    assert_eq!(names8(&out), ["Text", "Adjust"]);
    assert_eq!(out.layers[0].meta, doc.layers[6].meta);
    assert_eq!(out.layers[1].meta, doc.layers[7].meta);
    // A document holding only an adjustment layer is admitted.
    let only = duplicate8(&doc, &[7]).expect("an adjustment-only duplicate");
    assert_eq!(only.layers.len(), 1);
}

#[test]
fn byte_clips_survive_only_over_a_copied_base() {
    let doc = fixture8();
    let clipped =
        |out: &DrDocument| -> Vec<bool> { out.layers.iter().map(|l| l.clipped).collect() };
    // Base copied, nothing between.
    let out = duplicate8(&doc, &[1, 2]).unwrap();
    assert_eq!(clipped(&out), [false, true]);
    // Base not copied.
    let out = duplicate8(&doc, &[2]).unwrap();
    assert_eq!(clipped(&out), [false]);
    // A FOREIGN root (`X`, from inside `G`) between the base and `Clip2`
    // releases both of them; `X` has no base of its own.
    let out = duplicate8(&doc, &[1, 3, 5]).unwrap();
    assert_eq!(names8(&out), ["Masked", "X", "Clip2"]);
    assert_eq!(clipped(&out), [false, false, false]);
    // A clipped root over the SAME base between them keeps both.
    let out = duplicate8(&doc, &[1, 4, 5]).unwrap();
    assert_eq!(names8(&out), ["Masked", "X", "G", "Clip2"]);
    assert_eq!(
        clipped(&out),
        [false, true, true, true],
        "X inside the copied subtree keeps its flag"
    );
    // The group alone: its root clip is released, its child's is not.
    let out = duplicate8(&doc, &[4]).unwrap();
    assert_eq!(clipped(&out), [true, false]);
}

#[test]
fn byte_links_survive_only_when_shared() {
    let doc = fixture8();
    let links = |out: &DrDocument| -> Vec<u32> { out.layers.iter().map(|l| l.link).collect() };
    assert_eq!(links(&duplicate8(&doc, &[0, 1]).unwrap()), [1, 1]);
    assert_eq!(links(&duplicate8(&doc, &[1]).unwrap()), [0]);
    assert_eq!(links(&duplicate8(&doc, &[1, 6]).unwrap()), [0, 0]);
    assert_eq!(links(&duplicate8(&doc, &[5, 6]).unwrap()), [2, 2]);
}

#[test]
fn byte_refusals_are_null_and_the_source_is_unchanged() {
    let doc = fixture8();
    let ids: Vec<u64> = doc.layers.iter().map(|l| l.id).collect();
    assert!(duplicate8(&doc, &[]).is_none());
    assert!(duplicate8(&doc, &[8]).is_none());
    assert!(unsafe { dr_doc_duplicate_to_new_document(&doc, ptr::null(), 1) }.is_null());
    let _ = duplicate8(&doc, &[1, 4]).unwrap();
    assert_eq!(doc.layers.iter().map(|l| l.id).collect::<Vec<_>>(), ids);
    assert_eq!(doc.channels.len(), 1);
    assert_eq!(doc.guides.len(), 1);
    assert_eq!(doc.layer_comps().len(), 1);
}

// ============================================================ high depth ==

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn edit_with(
    doc: &PrecisionDocument,
    request: Value,
    samples: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        assert!(out.is_null());
        return Err(message);
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn changed(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    edit_with(doc, request.clone(), &[])
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} reported no change"))
}

fn refused(doc: &PrecisionDocument, layers: &[usize]) -> String {
    match duplicate(doc, layers) {
        Err(message) => message,
        Ok(_) => panic!("{layers:?} must be refused"),
    }
}

fn duplicate(doc: &PrecisionDocument, layers: &[usize]) -> Result<PrecisionDocument, String> {
    edit_with(
        doc,
        json!({"op":"duplicate_to_new_document","layers":layers}),
        &[],
    )
    .map(|d| d.expect("a duplicate is never NULL"))
}

/// `(name, nesting level)` in postorder, walked here.
fn table(doc: &PrecisionDocument) -> Vec<(String, usize)> {
    fn walk(layers: &[Layer], depth: usize, out: &mut Vec<(String, usize)>) {
        for layer in layers {
            if let Some(children) = layer.children() {
                walk(children, depth + 1, out);
            }
            out.push((layer.name().to_string(), depth));
        }
    }
    let mut out = Vec::new();
    walk(doc.root_layers(), 0, &mut out);
    out
}

fn names(doc: &PrecisionDocument) -> Vec<String> {
    table(doc).into_iter().map(|(name, _)| name).collect()
}

fn index(doc: &PrecisionDocument, name: &str) -> usize {
    names(doc)
        .iter()
        .position(|n| n == name)
        .unwrap_or_else(|| panic!("{name} is present"))
}

fn ids(doc: &PrecisionDocument) -> Vec<u64> {
    (0..doc.layer_count())
        .map(|i| doc.layer(i).unwrap().id())
        .collect()
}

fn pixel_address(doc: &PrecisionDocument, name: &str) -> usize {
    let source = doc.layer_source(index(doc, name)).expect("a raster record");
    unsafe { dr_precision_image_storage_identity(source) as usize }
}

fn mask_address(doc: &PrecisionDocument, name: &str) -> usize {
    doc.layer(index(doc, name))
        .and_then(|l| l.mask())
        .expect("a masked record")
        .plane()
        .samples()
        .as_ptr() as usize
}

fn raster(name: &str, offset: (i32, i32)) -> Layer {
    let px: Vec<[u16; 4]> = (0..4u16)
        .map(|i| [1001 + i, 2003 + i, 3007 + i, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(4, 1, px, IccProfile::srgb(), None).unwrap();
    Layer::raster(name, image, offset, 1.0, true).unwrap()
}

/// The 8-bit fixture's stack at 16 bits, postorder.
fn fixture16() -> PrecisionDocument {
    let description =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(4, 1, vec![0, 123, 32769, 65535], None).unwrap(),
        Default::default(),
        true,
    )
    .unwrap();
    let layers = vec![
        raster("Bg", (0, 0)),
        Layer::raster(
            "Masked",
            raster("Masked", (0, 0)).source().unwrap().clone(),
            (-3, 4),
            0.5,
            true,
        )
        .unwrap()
        .with_mask(Some(mask))
        .with_label(3)
        .with_locks(1),
        raster("Clip1", (0, 0)).with_clipped(true),
        Layer::isolated_normal("G", vec![raster("X", (0, 0)).with_clipped(true)], 1.0, true)
            .unwrap()
            .with_clipped(true),
        raster("Clip2", (0, 0)).with_clipped(true),
        raster("Text", (0, 0)),
        Layer::adjustment("Adjust", &description, 1.0, true, &IccProfile::srgb()).unwrap(),
    ];
    let doc = PrecisionDocument::from_parts_and_channels(
        4,
        1,
        IccProfile::srgb(),
        Metadata {
            exif: Some(Arc::from(&b"Exif\0\0MM"[..])),
            xmp: None,
            iptc: None,
        },
        Resolution { x: 300.0, y: 150.0 },
        layers,
        vec![PrecisionChannel::new(
            "Alpha 1",
            PrecisionPlane::from_u16(4, 1, vec![0, 100, 200, 65535], None).unwrap(),
        )
        .unwrap()],
    )
    .unwrap();
    assert_eq!(
        names(&doc),
        ["Bg", "Masked", "Clip1", "X", "G", "Clip2", "Text", "Adjust"]
    );
    let doc = changed(&doc, json!({"op":"link_layers","layers":[0,1]}));
    let doc = changed(&doc, json!({"op":"link_layers","layers":[5,6]}));
    let doc = doc
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .unwrap();
    let doc = doc
        .with_global_light(GlobalLight {
            angle: 30.0,
            altitude: 60.0,
        })
        .unwrap()
        .unwrap();
    let doc = doc.add_guide(GuideOrientation::Vertical, 2.0).unwrap();
    let request =
        CompRequest::parse(&json!({"op":"layer_comp","action":"save","name":"Shot"}).to_string())
            .unwrap();
    let doc = doc.edit_layer_comps(&request).unwrap().unwrap();
    // The text description, through the artwork surface.
    use doctored_core::ffi_artwork::dr_precision_doc_artwork_metadata;
    let text = json!({"type":"text","version":1,"payload":"hello"}).to_string();
    let value = CString::new(text).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_artwork_metadata(&doc, 6, value.as_ptr(), LIMIT, 0, &mut err) };
    assert!(owned(err).is_none() && !out.is_null());
    *unsafe { Box::from_raw(out) }
}

#[test]
fn sixteen_bit_chrome_follows_the_picture_not_the_source() {
    let doc = fixture16();
    let out = duplicate(&doc, &[1]).unwrap();
    assert!(!out.is_float());
    assert_eq!(out.dimensions(), doc.dimensions());
    assert!(Arc::ptr_eq(out.profile(), doc.profile()));
    assert_eq!(out.profile().bytes(), doc.profile().bytes());
    assert_eq!(out.resolution(), doc.resolution());
    assert_eq!(out.evaluation(), Evaluation::LinearLight);
    assert_eq!(out.global_light(), doc.global_light());
    assert!(out.channels().is_empty());
    assert!(out.guides().is_empty());
    assert_eq!(out.ruler_origin(), (0.0, 0.0));
    assert!(out.layer_comps().is_empty());
    assert_eq!(out.metadata(), &Metadata::default());
}

#[test]
fn sixteen_bit_roots_subtrees_properties_and_ids() {
    let doc = fixture16();
    let out = duplicate(&doc, &[4, 3, 1]).unwrap();
    assert_eq!(
        table(&out),
        [("Masked".into(), 0), ("X".into(), 1), ("G".into(), 0)]
    );
    assert_eq!(pixel_address(&out, "Masked"), pixel_address(&doc, "Masked"));
    assert_eq!(pixel_address(&out, "X"), pixel_address(&doc, "X"));
    assert_eq!(mask_address(&out, "Masked"), mask_address(&doc, "Masked"));
    let masked = out.layer(0).unwrap();
    assert_eq!(masked.offset(), (-3, 4), "an off-canvas offset is kept");
    assert_eq!(masked.opacity(), 0.5);
    assert_eq!((masked.label(), masked.locks()), (3, 1));
    let source = ids(&doc);
    let mut fresh = ids(&out);
    assert!(fresh.iter().all(|id| *id != 0 && !source.contains(id)));
    fresh.sort_unstable();
    fresh.dedup();
    assert_eq!(fresh.len(), out.layer_count());

    let described = duplicate(&doc, &[6, 7]).unwrap();
    assert_eq!(
        described.artwork_description(0),
        doc.artwork_description(6),
        "the text layer stays described"
    );
    assert_eq!(
        described.adjustment_description(1),
        doc.adjustment_description(7),
        "the adjustment travels"
    );
    let only = duplicate(&doc, &[7]).unwrap();
    assert_eq!(
        only.layer_count(),
        1,
        "an adjustment-only document is admitted"
    );
}

#[test]
fn sixteen_bit_clips_and_links_follow_the_byte_rule() {
    let doc = fixture16();
    let clipped = |out: &PrecisionDocument| -> Vec<bool> {
        (0..out.layer_count())
            .map(|i| out.layer(i).unwrap().clipped())
            .collect()
    };
    assert_eq!(clipped(&duplicate(&doc, &[1, 2]).unwrap()), [false, true]);
    assert_eq!(clipped(&duplicate(&doc, &[2]).unwrap()), [false]);
    assert_eq!(
        clipped(&duplicate(&doc, &[1, 3, 5]).unwrap()),
        [false, false, false]
    );
    assert_eq!(
        clipped(&duplicate(&doc, &[1, 4, 5]).unwrap()),
        [false, true, true, true]
    );
    assert_eq!(clipped(&duplicate(&doc, &[4]).unwrap()), [true, false]);

    let links = |out: &PrecisionDocument| -> Vec<u32> {
        (0..out.layer_count())
            .map(|i| out.layer(i).unwrap().link())
            .collect()
    };
    let shared = links(&duplicate(&doc, &[0, 1]).unwrap());
    assert!(shared[0] != 0 && shared[0] == shared[1], "{shared:?}");
    assert_eq!(links(&duplicate(&doc, &[1]).unwrap()), [0]);
    assert_eq!(links(&duplicate(&doc, &[1, 6]).unwrap()), [0, 0]);
}

#[test]
fn high_depth_refusals_are_named_and_the_source_is_unchanged() {
    let doc = fixture16();
    let before = ids(&doc);
    assert!(refused(&doc, &[]).contains("at least one layer"));
    assert!(refused(&doc, &[8]).contains("out of range"));
    let _ = duplicate(&doc, &[0, 4]).unwrap();
    assert_eq!(ids(&doc), before);
    assert_eq!(doc.channels().len(), 1);
    assert_eq!(doc.guides().len(), 1);
}

#[test]
fn float_duplicate_keeps_the_float_policy() {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return;
    }
    let base = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    assert!(base.is_float());
    let (w, h) = base.dimensions();
    let ramp: Vec<u16> = (0..(w * h) as usize).map(|i| (i * 811) as u16).collect();
    let masked = edit_with(
        &base,
        json!({"op":"set_mask","layer":0,"width":w,"height":h,"x":0,"y":0,"linked":true}),
        &ramp,
    )
    .unwrap()
    .unwrap();
    let exposure = json!({"type":"adjust","evaluation":"precision_v1","op":"exposure",
                          "params":{"exposure":0.25}})
    .to_string();
    let adjusted = masked
        .add_adjustment("Exposure", &exposure, 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let second = adjusted.duplicate_layer(0).unwrap().unwrap();
    let second = changed(
        &second,
        json!({"op":"rename_layer","layer":1,"name":"Second"}),
    );
    let grouped = changed(
        &second,
        json!({"op":"group_layers","layers":[1],"name":"G","pass_through":false}),
    );
    assert_eq!(names(&grouped), ["Background", "Second", "G", "Exposure"]);

    let out = duplicate(&grouped, &[0, 2, 3]).unwrap();
    assert!(out.is_float());
    assert_eq!(out.float_refusal(), None, "the float renderer accepts it");
    assert_eq!(out.tone_map(), grouped.tone_map());
    assert_eq!(out.evaluation(), grouped.evaluation());
    assert_eq!(out.dimensions(), grouped.dimensions());
    assert!(Arc::ptr_eq(out.profile(), grouped.profile()));
    assert_eq!(out.resolution(), grouped.resolution());
    assert_eq!(out.global_light(), grouped.global_light());
    assert_eq!(
        table(&out),
        [
            ("Background".into(), 0),
            ("Second".into(), 1),
            ("G".into(), 0),
            ("Exposure".into(), 0)
        ]
    );
    assert_eq!(
        pixel_address(&out, "Background"),
        pixel_address(&grouped, "Background")
    );
    assert_eq!(
        mask_address(&out, "Background"),
        mask_address(&grouped, "Background")
    );
    assert_eq!(
        out.adjustment_description(3),
        grouped.adjustment_description(3)
    );
    let source = ids(&grouped);
    assert!(ids(&out).iter().all(|id| !source.contains(id)));
}
