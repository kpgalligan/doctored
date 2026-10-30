//! 12H-2 structure-preserving NARROWING (16→8, float→16, float→8): every
//! carried record, the per-record quantizer and tone map against independent
//! oracles, the fixed preview-delta bound, the plan's shape, the blocked
//! kinds by name with the input untouched, retained-source and warp carriage
//! with every staleness guard tied to the landed layer, native round trip,
//! NULL safety and cancellation.
//!
//! The 16-bit fixture is 12H-1's byte fixture (restated below from
//! `precision_conversion_structure.rs`) carried to 16 bits by
//! `promote_structure`; the float fixture is authored here through
//! `tiff_float::encode_to_vec` and `open_preserving`, then layered through
//! the typed edit surface.
use doctored_core::doc::BlendMode;
use doctored_core::doc::{DrDocument, Layer, LayerKind, LayerRaster};
use doctored_core::doc_comps::CompRequest;
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_guide::GuideOrientation;
use doctored_core::doc_lock::{LOCK_PIXELS, LOCK_POSITION};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::{dr_doc_clone, dr_doc_free};
use doctored_core::ffi_jobs::dr_job_control_set_scratch_limit;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_conversion::*;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::float_document::ToneMap;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_adjustment::PrecisionAdjustmentState;
use doctored_core::precision_conversion_narrow::NarrowTarget;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask_edit::MaskEdit;
use doctored_core::precision_quantize::PrecisionDither;
use doctored_core::retained_source::SourceEdit;
use doctored_core::style::LayerStyle;
use doctored_core::tiff_float::{self, FloatTiff};
use doctored_core::warp::WarpEdit;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 40;
const H: u32 = 30;

unsafe fn text(pointer: *mut c_char) -> String {
    assert!(!pointer.is_null());
    let result = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { dr_string_free(pointer) };
    result
}

fn pixels(w: u32, h: u32, seed: u32, alpha: bool) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let a = if alpha {
            ((x * 23 + y * 41 + seed * 7) % 256) as u8
        } else {
            255
        };
        Rgba([
            ((x * 13 + seed * 29) % 256) as u8,
            ((y * 17 + seed * 11) % 256) as u8,
            ((x + y + seed * 5) * 9 % 256) as u8,
            a,
        ])
    })
}

fn fresh(image: RgbaImage, name: &str) -> Layer {
    let mut layer = DrDocument::from_pixels(image).layers.remove(0);
    layer.name = name.into();
    layer
}

fn group(name: &str, blend: BlendMode, open: bool) -> Layer {
    let mut layer = fresh(RgbaImage::new(1, 1), name);
    layer.kind = LayerKind::Group;
    layer.blend = blend;
    layer.open = open;
    layer
}

fn nested(mut layer: Layer, depth: u16) -> Layer {
    layer.depth = depth;
    layer
}

const STYLE: &str = r#"{"fill_opacity":0.8,"knockout":"shallow","blend_if":{"channel":"gray","this_layer":[0,0,200,255],"underlying":[0,30,255,255]},"effects":[{"type":"drop_shadow","use_global_light":false,"distance":3,"size":2}]}"#;

/// 12H-1's byte fixture, restated: every record kind, at mixed levels.
fn byte_fixture() -> DrDocument {
    let mut doc = DrDocument::from_pixels(pixels(W, H, 1, false));
    doc.layers[0].name = "Background".into();
    let mut masked = fresh(pixels(17, 11, 2, true), "Masked");
    masked.offset = (5, 4);
    masked.opacity = 0.8;
    masked.blend = BlendMode::Multiply;
    masked.mask = Some(Arc::new(GrayImage::from_fn(17, 11, |x, y| {
        Luma([((x * 15 + y * 7) % 256) as u8])
    })));
    masked.mask_properties = MaskProperties::at((5, 4));
    masked.mask_properties.feather = 2.0;
    masked.label = 3;
    masked.locks = LOCK_POSITION;
    masked.link = 1;
    let mut clipped = fresh(pixels(12, 9, 3, true), "Clipped");
    clipped.offset = (8, 6);
    clipped.clipped = true;
    clipped.link = 1;
    let child_a = nested(fresh(pixels(W, H, 4, true), "Child A"), 1);
    let mut child_b = nested(fresh(pixels(20, 12, 5, true), "Child B"), 1);
    child_b.offset = (-3, 10);
    let isolated = group("Isolated", BlendMode::Screen, false);
    let pass_child = nested(fresh(pixels(25, 20, 6, true), "Pass Child"), 1);
    let mut levels = nested(fresh(RgbaImage::new(1, 1), "Levels"), 1);
    levels.meta = Some(r#"{"type":"adjust","op":"levels","params":{"gamma":0.8}}"#.into());
    levels.opacity = 0.6;
    let mut pass = group("Pass", BlendMode::PassThrough, true);
    pass.label = 5;
    let mut title = fresh(pixels(14, 6, 7, true), "Title");
    title.offset = (2, 20);
    title.meta = Some(r#"{"type":"text","version":1,"string":"Hi"}"#.into());
    let mut styled = fresh(pixels(16, 12, 8, true), "Styled");
    styled.offset = (18, 9);
    styled.style = Some(Arc::new(LayerStyle::from_json(STYLE).unwrap()));
    styled.locks = LOCK_PIXELS;
    doc.layers.extend([
        masked, clipped, child_a, child_b, isolated, pass_child, levels, pass, title, styled,
    ]);
    doc = doc
        .add_channel(
            "Saved",
            &(0..W * H).map(|i| (i % 256) as u8).collect::<Vec<_>>(),
            W,
            H,
            [0, 128, 255],
            0.25,
        )
        .unwrap();
    doc = doc
        .edit_layer_comps(
            &CompRequest::parse(r#"{"op":"layer_comp","action":"save","name":"Comp A"}"#).unwrap(),
        )
        .unwrap()
        .unwrap();
    doc = doc.add_guide(GuideOrientation::Vertical, 12.5).unwrap();
    doc = doc.add_guide(GuideOrientation::Horizontal, 7.0).unwrap();
    doc = doc.set_ruler_origin(3.0, 4.0).unwrap();
    doc.profile = IccProfile::display_p3();
    doc.resolution = Resolution { x: 240.0, y: 180.0 };
    doc.metadata.xmp = Some(Arc::from(b"captured XMP".as_slice()));
    doc
}

fn promote(doc: &DrDocument) -> PrecisionDocument {
    doc.promote_structure(LIMIT, 0, None)
        .unwrap_or_else(|e| panic!("{e}"))
}

fn narrow8(doc: &PrecisionDocument, dither: PrecisionDither) -> DrDocument {
    doc.narrow_structure_to_u8(dither, LIMIT, 0, None)
        .unwrap_or_else(|e| panic!("{e}"))
}

/// The byte handle after `dr_doc_clone` — a `doc_op` round, so the next
/// mutation's `detach_invalid_sources` has run on it.
fn detached(doc: &DrDocument) -> DrDocument {
    unsafe {
        let out = dr_doc_clone(doc);
        assert!(!out.is_null());
        let copy = (*out).clone();
        dr_doc_free(out);
        copy
    }
}

// ------------------------------------------------------------- 16 → 8 --

#[test]
fn sixteen_to_eight_carries_every_record_and_the_chrome_back() {
    let original = byte_fixture();
    let sixteen = promote(&original);
    let back = narrow8(&sixteen, PrecisionDither::None);
    assert_eq!(back.layers.len(), original.layers.len());
    for (i, (old, new)) in original.layers.iter().zip(&back.layers).enumerate() {
        assert_eq!(new.id, old.id, "id {i}");
        assert_eq!(new.name, old.name, "name {i}");
        assert_eq!(new.kind, old.kind, "kind {i}");
        assert_eq!(new.depth, old.depth, "depth {i}");
        assert_eq!(new.opacity, old.opacity, "opacity {i}");
        assert_eq!(new.visible, old.visible, "visible {i}");
        assert_eq!(new.blend, old.blend, "blend {i}");
        assert_eq!(new.clipped, old.clipped, "clipped {i}");
        assert_eq!(new.locks, old.locks, "locks {i}");
        assert_eq!(new.link, old.link, "link {i}");
        assert_eq!(new.label, old.label, "label {i}");
        assert_eq!(new.mask_enabled, old.mask_enabled, "mask enabled {i}");
        assert!(new.mask_properties == old.mask_properties, "mask props {i}");
        assert_eq!(
            new.style.as_ref().map(|s| s.to_json()),
            old.style.as_ref().map(|s| s.to_json()),
            "style {i}"
        );
        // ×257 then nearest is the identity on every byte code, so each
        // plane comes back EXACTLY: rasters, masks, the 1×1 group and
        // adjustment planes alike.
        assert_eq!(
            new.pixels.contiguous_image().as_raw(),
            old.pixels.contiguous_image().as_raw(),
            "pixels {i}"
        );
        assert_eq!(
            new.mask.as_ref().map(|m| m.as_raw().clone()),
            old.mask.as_ref().map(|m| m.as_raw().clone()),
            "mask {i}"
        );
        if old.kind == LayerKind::Raster {
            assert_eq!(new.offset, old.offset, "offset {i}");
        }
        match old.meta.as_deref() {
            Some(meta) if meta.contains("adjust") => {
                let value: Value = serde_json::from_str(new.meta.as_deref().unwrap()).unwrap();
                assert_eq!(value["op"], "levels");
                assert_eq!(value["params"]["gamma"], 0.8);
                assert_eq!(value["evaluation"], "precision_v1");
            }
            other => assert_eq!(new.meta.as_deref(), other, "meta {i}"),
        }
        // The group disclosure round-trips.
        assert_eq!(new.open, old.open, "open {i}");
    }
    assert_eq!(back.channels.len(), 1);
    assert_eq!(back.channels[0].name, "Saved");
    assert_eq!(
        back.channels[0].data.as_raw(),
        original.channels[0].data.as_raw()
    );
    assert_eq!(back.channels[0].overlay_color, [0, 128, 255]);
    assert_eq!(back.channels[0].overlay_opacity, 0.25);
    assert_eq!(back.layer_comps(), original.layer_comps());
    assert_eq!(back.guides, original.guides);
    assert_eq!(back.ruler_origin, (3.0, 4.0));
    assert_eq!(back.resolution.x, 240.0);
    assert_eq!(back.resolution.y, 180.0);
    assert_eq!(back.profile.bytes(), original.profile.bytes());
    assert_eq!(back.metadata, original.metadata);
    assert_eq!(back.evaluation(), Evaluation::Encoded);
    // Every carried record composites exactly as it did at 8 bits.
    assert_eq!(back.flattened().unwrap(), original.flattened().unwrap());
}

/// A typed document whose samples are NOT byte codes ×257.
fn typed_samples() -> PrecisionDocument {
    let (w, h) = (23_u32, 17_u32);
    let samples: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            [
                ((i * 3901 + 63101) % 65536) as u16,
                ((i * 779 + 17411) % 65536) as u16,
                ((i * 4999 + 44351) % 65536) as u16,
                ((i * 1237 + 129) % 65536) as u16,
            ]
        })
        .collect();
    let profile = IccProfile::srgb();
    let image = PrecisionImage::from_rgba16(w, h, samples, profile.clone(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Samples", image, (2, -1), 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        w + 4,
        h,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
}

#[test]
fn quantization_is_nearest_exactly_and_ordered_within_one_with_exact_alpha() {
    let typed = typed_samples();
    let source = typed.layer_source(0).unwrap().to_rgba16().unwrap();
    let nearest = narrow8(&typed, PrecisionDither::None);
    let ordered = narrow8(&typed, PrecisionDither::Ordered);
    let nearest_image = nearest.layers[0].pixels.contiguous_image();
    let dithered_image = ordered.layers[0].pixels.contiguous_image();
    for (i, sample) in source.iter().enumerate() {
        // The oracle: (v + 128) / 257, written out here.
        let expected = sample.map(|v| ((u32::from(v) + 128) / 257) as u8);
        let got = nearest_image.as_raw()[i * 4..i * 4 + 4].to_vec();
        assert_eq!(got, expected.to_vec(), "nearest {i}");
        let dithered = &dithered_image.as_raw()[i * 4..i * 4 + 4];
        for c in 0..3 {
            assert!(
                (i32::from(dithered[c]) - i32::from(expected[c])).abs() <= 1,
                "ordered {i}/{c}"
            );
        }
        assert_eq!(dithered[3], expected[3], "alpha {i} is never dithered");
    }
    assert_eq!(nearest.layers[0].offset, (2, -1));
}

#[test]
fn preview_delta_is_within_the_fixed_bound() {
    let sixteen = promote(&byte_fixture());
    let back = narrow8(&sixteen, PrecisionDither::None);
    let mut err = ptr::null_mut();
    let preview: Value = unsafe {
        serde_json::from_str(&text(dr_precision_doc_narrow_structure_preview(
            &sixteen,
            &back,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )))
        .unwrap()
    };
    eprintln!("12H-2 16→8 full-fixture preview delta: {preview}");
    assert_eq!(preview["scale"], 65535);
    assert!(preview["compared_samples"].as_u64().unwrap() > 0);
    assert!(preview["max_delta"].as_u64().unwrap() <= 771, "{preview}");
    assert!(
        preview["mean_delta"].as_f64().unwrap() <= 128.5,
        "{preview}"
    );
}

#[test]
fn plan_json_shape_and_golden_arrays() {
    let sixteen = promote(&byte_fixture());
    let mut err = ptr::null_mut();
    let plan: Value = unsafe {
        serde_json::from_str(&text(dr_precision_doc_narrow_structure_plan(
            &sixteen, 8, &mut err,
        )))
        .unwrap()
    };
    assert_eq!(plan["flattened"], false);
    assert_eq!(plan["structure"], "preserve");
    assert_eq!(plan["direction"], "rgba16_to_rgba8");
    assert!(plan.get("widening").is_none());
    assert!(plan.get("tone_map").is_none());
    assert_eq!(
        plan["rgb_quantization"],
        "explicit none or ordered_8x8_v1 per record"
    );
    assert_eq!(plan["alpha_quantization"], "nearest, never dithered");
    let preserved = plan["preserved"].as_array().unwrap();
    assert_eq!(preserved.len(), 29);
    assert_eq!(preserved[0], "raster_pixels_quantized");
    assert_eq!(preserved[1], "layer_offsets");
    assert!(preserved.contains(&json!("global_light")));
    assert_eq!(preserved[28], "evaluation_policy");
    assert_eq!(plan["discarded"], json!([]));
    assert_eq!(plan["blocked"], json!([]));
    let notes: Vec<(u64, String)> = plan["rendering_differences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["index"].as_u64().unwrap(),
                row["note"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(notes
        .iter()
        .any(|(i, n)| *i == 5 && n.contains("byte rounding")));
    assert!(notes
        .iter()
        .any(|(i, n)| *i == 7 && n.contains("adjustment")));
    assert!(!notes.iter().any(|(i, _)| *i == 8));
    // Output: every raster ×4, the unmasked Levels' 1×1 plane ×4, the one
    // mask and the one channel ×1.
    let rasters: u64 = (0..sixteen.layer_count())
        .filter_map(|i| sixteen.layer_source(i))
        .map(|s| u64::from(s.dimensions().0) * u64::from(s.dimensions().1) * 4)
        .sum();
    assert_eq!(
        plan["output_bytes"].as_u64().unwrap(),
        rasters + 4 + 17 * 11 + u64::from(W * H)
    );
    // 16 on a 16-bit source is an error, not a row.
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_precision_doc_narrow_structure_plan(&sixteen, 16, &mut err) }.is_null());
    assert!(unsafe { text(err) }.contains("already stores 16-bit"));
    let mut err = ptr::null_mut();
    assert!(unsafe { dr_precision_doc_narrow_structure_plan(&sixteen, 12, &mut err) }.is_null());
    assert!(unsafe { text(err) }.contains("8 or 16"));
}

/// A raster nested `levels` groups deep.
fn deep(levels: usize) -> PrecisionDocumentLayer {
    let profile = IccProfile::srgb();
    let image = PrecisionImage::from_rgba16(2, 2, vec![[65535; 4]; 4], profile, None).unwrap();
    let mut node = PrecisionDocumentLayer::raster("Leaf", image, (0, 0), 1.0, true).unwrap();
    for level in 0..levels {
        node = PrecisionDocumentLayer::isolated_normal(&format!("G{level}"), vec![node], 1.0, true)
            .unwrap();
    }
    node
}

#[test]
fn a_nest_deeper_than_ten_blocks_first_by_name_with_the_input_untouched() {
    let base = typed_samples();
    let profile = base.profile().clone();
    let bottom = PrecisionDocumentLayer::raster(
        "Bottom",
        PrecisionImage::from_rgba16(1, 1, vec![[1; 4]], profile.clone(), None).unwrap(),
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let rebuild = |leaf: PrecisionDocumentLayer| {
        PrecisionDocument::from_parts(
            8,
            8,
            profile.clone(),
            Metadata::default(),
            Resolution::default(),
            vec![bottom.clone(), leaf],
        )
        .unwrap()
    };
    // Ten levels fit an 8-bit document.
    let ten = rebuild(deep(10));
    assert!(ten
        .narrow_structure_plan(NarrowTarget::Rgba8)
        .unwrap()
        .blocked
        .is_empty());
    let back = narrow8(&ten, PrecisionDither::None);
    assert_eq!(back.layers.iter().map(|l| l.depth).max(), Some(10));
    // Eleven do not: the leaf (record 1) sits at level 11.
    let eleven = rebuild(deep(11));
    let plan = eleven.narrow_structure_plan(NarrowTarget::Rgba8).unwrap();
    let first = &plan.blocked[0];
    assert_eq!(first.index, 1);
    assert_eq!(first.name, "Leaf");
    assert_eq!(
        first.note,
        "nested 11 deep; an 8-bit document holds 10 levels"
    );
    let failure = eleven
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .err()
        .unwrap();
    assert_eq!(Some(failure.clone()), plan.refusal());
    assert!(failure.starts_with(
        "Preserve Layers cannot convert this document: layer 1 \"Leaf\": nested 11 deep"
    ));
    assert_eq!(eleven.layer_count(), 13);
}

#[test]
fn every_admitted_typed_adjustment_has_a_byte_evaluation() {
    // The "no 8-bit evaluation" row is a BACKSTOP: the typed parser runs the
    // byte `Adjustment::from_op` itself, so every description a typed
    // document can hold is byte-readable. Each admitted op carries.
    let base = typed_samples();
    for description in [
        json!({"type":"adjust","evaluation":"precision_v1","op":"levels","params":{"gamma":0.8}}),
        json!({"type":"adjust","evaluation":"precision_v1","op":"invert","params":{}}),
        json!({"type":"adjust","evaluation":"precision_v1","op":"exposure","params":{"exposure":0.5}}),
        json!({"type":"adjust","evaluation":"precision_v1","op":"curves","params":{"rgb":[[0.0,0.0],[0.5,0.6],[1.0,1.0]]}}),
        json!({"type":"adjust","evaluation":"precision_v1","op":"hue_saturation","params":{"saturation":0.2}}),
    ] {
        let doc = base
            .add_adjustment("Adjust", &description.to_string(), 1.0, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        let plan = doc.narrow_structure_plan(NarrowTarget::Rgba8).unwrap();
        assert!(
            plan.blocked.is_empty(),
            "{description}: {:?}",
            plan.refusal()
        );
        let back = narrow8(&doc, PrecisionDither::None);
        let meta: Value = serde_json::from_str(back.layers[1].meta.as_deref().unwrap()).unwrap();
        assert_eq!(meta["op"], description["op"]);
    }
}

#[test]
fn a_masked_adjustment_keeps_its_canvas_extent_at_eight_bits() {
    // A byte adjustment's mask covers only its PIXEL extent
    // (`doc::composite_adjustment_into`), so the canvas-sized transparent
    // plane a byte host creates must come back for a masked one.
    let mut doc = DrDocument::from_pixels(pixels(W, H, 31, false));
    let mut levels = fresh(RgbaImage::new(W, H), "Levels");
    levels.meta = Some(r#"{"type":"adjust","op":"levels","params":{"gamma":0.6}}"#.into());
    levels.mask = Some(Arc::new(GrayImage::from_fn(W, H, |x, _| {
        Luma([if x < W / 2 { 255 } else { 40 }])
    })));
    levels.mask_properties = MaskProperties::at((0, 0));
    doc.layers.push(levels);
    let back = narrow8(&promote(&doc), PrecisionDither::None);
    assert_eq!(back.layers[1].pixels.dimensions(), (W, H));
    assert_eq!(back.layers[1].offset, (0, 0));
    assert_eq!(back.flattened().unwrap(), doc.flattened().unwrap());
    let plan = promote(&doc)
        .narrow_structure_plan(NarrowTarget::Rgba8)
        .unwrap();
    // Background ×4 + the adjustment's canvas plane ×4 + its mask ×1.
    assert_eq!(plan.output_bytes, u64::from(W * H) * 9);
}

// ----------------------------------------------- sources and warps 16→8 --

fn placed() -> DrDocument {
    let master = DrDocument::from_pixels(pixels(6, 4, 11, false));
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"12h2-master","revision_id":"v1",
                "name":"Retained","x":2,"y":1})
        .to_string(),
    )
    .unwrap();
    byte_fixture()
        .source_edit(Some(&master), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

#[test]
fn a_retained_source_is_carried_back_and_stays_effective() {
    let original = placed();
    let index = original.layers.len() - 1;
    assert!(original.layers[index].retained_source.is_some());
    let sixteen = promote(&original);
    assert!(sixteen.layer(index).unwrap().effective_source().is_some());
    let plan = sixteen.narrow_structure_plan(NarrowTarget::Rgba8).unwrap();
    assert!(plan
        .rendering_differences
        .iter()
        .any(|row| row.index == index && row.note.contains("quantizes its fallback at 8 bits")));
    let back = narrow8(&sixteen, PrecisionDither::None);
    let layer = &back.layers[index];
    assert_eq!(
        layer.pixels.contiguous_image().as_raw(),
        original.layers[index].pixels.contiguous_image().as_raw()
    );
    let source = layer.effective_source(&back.profile).expect("effective");
    assert!(LayerRaster::ptr_eq(&source.rendered, &layer.pixels));
    assert_eq!(source.asset.id, "12h2-master");
    assert!(back.source_info(index).unwrap()["valid"] == true);
    let again = detached(&back);
    assert!(again.layers[index].retained_source.is_some());
    assert!(again.layers[index]
        .effective_source(&again.profile)
        .is_some());
}

fn field(layer: usize, target: &str) -> WarpEdit {
    let (columns, rows) = (3_u32, 3_u32);
    let values: Vec<[f64; 2]> = (0..rows)
        .flat_map(|y| {
            (0..columns).map(move |x| {
                let fx = f64::from(x) / f64::from(columns - 1);
                let fy = f64::from(y) / f64::from(rows - 1);
                [
                    0.35 + 0.58 * fx - 0.31 * fy + 0.12 * fx * fy,
                    -0.27 + 0.33 * fy + 0.19 * fx - 0.17 * fx * fy,
                ]
            })
        })
        .collect();
    WarpEdit::parse(
        &json!({"op":"set","layer":layer,"target":target,"columns":columns,"rows":rows,
            "displacements":values,"filter":"bicubic"})
        .to_string(),
    )
    .unwrap()
}

/// Three warp shapes: an unmasked Pixels warp (0), a Pixels warp on a
/// MASKED layer whose linked mask rides (1), and a Mask-target warp (2).
fn warped() -> DrDocument {
    let mut doc = DrDocument::from_pixels(pixels(W, H, 21, true));
    for (name, seed) in [("Masked", 22), ("Mask target", 23)] {
        let mut layer = fresh(pixels(15, 11, seed, true), name);
        layer.offset = (4, 3);
        layer.mask = Some(Arc::new(GrayImage::from_fn(13, 9, |x, y| {
            Luma([((x * 19 + y * 11 + seed) % 256) as u8])
        })));
        layer.mask_properties = MaskProperties::at((5, 4));
        doc.layers.push(layer);
    }
    for (layer, target) in [(0, "pixels"), (1, "pixels"), (2, "mask")] {
        doc = doc
            .warp_edit(&field(layer, target), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert!(doc.warp_info(layer).unwrap()["field"].is_object());
    }
    doc
}

fn assert_byte_warps_effective(doc: &DrDocument) {
    for (i, layer) in doc.layers.iter().enumerate() {
        let binding = layer.warp.as_ref().expect("the warp is carried");
        assert!(doc.warp_info(i).unwrap()["field"].is_object(), "warp {i}");
        assert!(Arc::ptr_eq(&binding.profile, &doc.profile), "profile {i}");
        if let Some(rendered) = &binding.rendered_pixels {
            assert!(LayerRaster::ptr_eq(rendered, &layer.pixels), "pixels {i}");
            assert_eq!(binding.rendered_offset, layer.offset);
        }
        if i > 0 {
            let rendered = binding.rendered_mask.as_ref().expect("a rendered mask");
            assert!(
                Arc::ptr_eq(rendered, layer.mask.as_ref().unwrap()),
                "mask {i}"
            );
            assert_eq!(binding.rendered_mask_offset, layer.mask_properties.offset);
        }
    }
}

#[test]
fn warps_are_carried_back_in_three_shapes_and_survive_detach() {
    let original = warped();
    let sixteen = promote(&original);
    for i in 0..3 {
        assert!(
            sixteen.warp_info(i).unwrap()["field"].is_object(),
            "typed {i}"
        );
    }
    let back = narrow8(&sixteen, PrecisionDither::None);
    assert_byte_warps_effective(&back);
    for (i, (old, new)) in original.layers.iter().zip(&back.layers).enumerate() {
        let (a, b) = (old.warp.as_ref().unwrap(), new.warp.as_ref().unwrap());
        // The retained originals come back exactly (×257, then nearest).
        assert_eq!(
            a.source_pixels
                .as_ref()
                .map(|p| p.contiguous_image().as_raw().clone()),
            b.source_pixels
                .as_ref()
                .map(|p| p.contiguous_image().as_raw().clone()),
            "source pixels {i}"
        );
        assert_eq!(
            a.source_mask.as_ref().map(|m| m.as_raw().clone()),
            b.source_mask.as_ref().map(|m| m.as_raw().clone()),
            "source mask {i}"
        );
        assert_eq!(a.target, b.target);
        assert_eq!(a.rendered_pixels.is_some(), b.rendered_pixels.is_some());
    }
    let again = detached(&back);
    assert_byte_warps_effective(&again);
    // A re-edit samples the retained original: clearing restores it.
    let cleared = back
        .warp_edit(
            &WarpEdit::parse(r#"{"op":"clear","layer":0}"#).unwrap(),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        cleared.layers[0].pixels.contiguous_image().as_raw(),
        DrDocument::from_pixels(pixels(W, H, 21, true)).layers[0]
            .pixels
            .contiguous_image()
            .as_raw()
    );
}

#[test]
fn byte_native_save_reopen_resave_is_byte_identical() {
    let directory = TempDir::new().unwrap();
    for (label, doc) in [
        (
            "fixture",
            narrow8(&promote(&byte_fixture()), PrecisionDither::None),
        ),
        (
            "warped",
            narrow8(&promote(&warped()), PrecisionDither::None),
        ),
        (
            "placed",
            narrow8(&promote(&placed()), PrecisionDither::None),
        ),
    ] {
        let first = directory.path().join(format!("{label}-a.rz"));
        let second = directory.path().join(format!("{label}-b.rz"));
        doc.save_native(first.to_str().unwrap()).unwrap();
        let reopened = DrDocument::open(first.to_str().unwrap()).unwrap();
        reopened.save_native(second.to_str().unwrap()).unwrap();
        assert!(
            std::fs::read(&first).unwrap() == std::fs::read(&second).unwrap(),
            "{label}: reopen → resave differs"
        );
        for (i, layer) in reopened.layers.iter().enumerate() {
            assert_eq!(layer.id, doc.layers[i].id, "{label} id {i}");
        }
    }
}

// ------------------------------------------------------------- float --

fn srgb_encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn changed(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
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
    assert!(!out.is_null(), "{request}");
    *unsafe { Box::from_raw(out) }
}

/// An in-range sRGB float document under `clip`, then layered through the
/// typed edit surface: a duplicate (Multiply, clipped), a group holding a
/// masked, labelled copy, an Exposure adjustment, a lock, a guide, a comp.
fn float_fixture(directory: &TempDir) -> PrecisionDocument {
    let (w, h) = (24_u32, 18_u32);
    let samples: Vec<f32> = (0..w * h)
        .flat_map(|i| {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            [
                x / (w - 1) as f32,
                y / (h - 1) as f32,
                ((x + y) % 7.0) / 6.0,
                0.25 + 0.75 * ((x * 3.0 + y) % 5.0) / 4.0,
            ]
        })
        .collect();
    let path = directory.path().join("scene.tiff");
    let bytes = tiff_float::encode_to_vec(
        &FloatTiff {
            width: w,
            height: h,
            channels: 4,
            alpha: Some(false),
            samples,
            icc: None,
            resolution: None,
            metadata: Metadata::default(),
            non_finite: 0,
        },
        false,
    )
    .unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None).unwrap();
    assert!(doc.is_float());
    assert_eq!(doc.tone_map(), Some(ToneMap::Clip));
    let doc = doc.duplicate_layer(0).unwrap().unwrap();
    let doc = doc.duplicate_layer(1).unwrap().unwrap();
    let doc = changed(
        &doc,
        json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":1}),
    );
    let doc = changed(&doc, json!({"op":"layer_clipped","layer":1,"clipped":true}));
    let doc = doc.group_layers(&[2], "Group", false).unwrap().unwrap().0;
    let mask = MaskEdit::parse(
        &json!({"op":"set_mask","layer":2,"width":10,"height":8,"x":3,"y":2}).to_string(),
    )
    .unwrap();
    let values: Vec<u16> = (0..80).map(|i| (i * 811 % 65536) as u16).collect();
    let doc = doc
        .mask_edit(&mask, &values, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let doc = changed(&doc, json!({"op":"layer_label","layer":2,"label":4}));
    let doc = changed(&doc, json!({"op":"layer_lock","layer":0,"locks":4}));
    let exposure = json!({"type":"adjust","evaluation":"precision_v1","op":"exposure",
                          "params":{"exposure":0.25}})
    .to_string();
    let doc = doc
        .add_adjustment_at(
            "Exposure",
            &exposure,
            0.9,
            3,
            InsertPosition::Above,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let doc = doc.add_guide(GuideOrientation::Vertical, 5.0).unwrap();
    let doc = doc.set_ruler_origin(1.0, 2.0).unwrap();
    let doc = doc
        .edit_layer_comps(
            &CompRequest::parse(r#"{"op":"layer_comp","action":"save","name":"Float comp"}"#)
                .unwrap(),
        )
        .unwrap()
        .unwrap();
    assert!(doc.float_refusal().is_none());
    doc
}

fn assert_rows_carried(float: &PrecisionDocument, typed: &PrecisionDocument) {
    assert_eq!(typed.layer_count(), float.layer_count());
    for i in 0..float.layer_count() {
        let (a, b) = (float.layer(i).unwrap(), typed.layer(i).unwrap());
        assert_eq!(typed.layer_id(i), float.layer_id(i), "id {i}");
        assert_eq!(b.name(), a.name());
        assert_eq!(b.opacity(), a.opacity());
        assert_eq!(b.visible(), a.visible());
        assert_eq!(b.clipped(), a.clipped());
        assert_eq!(b.label(), a.label());
        assert_eq!(b.locks(), a.locks());
        assert_eq!(b.blend_mode(), a.blend_mode());
        assert_eq!(b.offset(), a.offset());
        assert_eq!(b.children().map(<[_]>::len), a.children().map(<[_]>::len));
        assert_eq!(
            b.mask().map(|m| m.plane().samples().to_vec()),
            a.mask().map(|m| m.plane().samples().to_vec())
        );
    }
    assert_eq!(typed.layer_comps(), float.layer_comps());
    assert_eq!(typed.guides(), float.guides());
    assert_eq!(typed.ruler_origin(), float.ruler_origin());
}

#[test]
fn float_to_sixteen_tone_maps_every_record_and_carries_the_rest() {
    let directory = TempDir::new().unwrap();
    let float = float_fixture(&directory);
    let plan = float.narrow_structure_plan(NarrowTarget::Rgba16).unwrap();
    let json = plan.json();
    assert_eq!(json["direction"], "rgba32_float_to_rgba16");
    assert_eq!(json["tone_map"], json!({"operator":"clip"}));
    assert_eq!(json["discarded"], json!(["extended_range"]));
    assert_eq!(json["preserved"][0], "raster_pixels_tone_mapped");
    assert!(json.get("rgb_quantization").is_none());
    assert!(plan.blocked.is_empty(), "{:?}", plan.refusal());
    let typed = float.narrow_structure_to_u16(LIMIT, 0, None).unwrap();
    assert!(!typed.is_float());
    assert_eq!(typed.evaluation(), Evaluation::LinearLight);
    assert_rows_carried(&float, &typed);
    for i in 0..float.layer_count() {
        let Some(source) = float.layer_source(i) else {
            continue;
        };
        assert!(
            plan.rendering_differences
                .iter()
                .any(|row| row.index == i && row.note.contains("tone-mapped per record")),
            "RD row on float raster {i}"
        );
        let wide = typed.layer_source(i).unwrap().to_rgba16().ok().unwrap();
        for (f, u) in source.to_rgba32().ok().unwrap().iter().zip(wide.iter()) {
            for c in 0..3 {
                let oracle = (65535.0 * srgb_encode(f64::from(f[c]))).round();
                assert!(
                    (f64::from(u[c]) - oracle).abs() <= 1.0,
                    "{i}: {f:?} → {u:?}"
                );
            }
            let alpha = (65535.0 * f64::from(f[3])).round();
            assert!((f64::from(u[3]) - alpha).abs() <= 1.0);
        }
    }
    assert!(plan
        .rendering_differences
        .iter()
        .any(|row| row.name == "Exposure" && row.note.contains("after the per-record tone map")));
    let mut err = ptr::null_mut();
    let preview: Value = unsafe {
        serde_json::from_str(&text(dr_precision_doc_narrow_structure_preview(
            &float,
            ptr::null(),
            &typed,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )))
        .unwrap()
    };
    eprintln!("12H-2 float→16 preview delta: {preview}");
    assert!(preview["max_delta"].as_u64().unwrap() <= 771, "{preview}");
    assert!(
        preview["mean_delta"].as_f64().unwrap() <= 128.5,
        "{preview}"
    );
    // 16 on a u16 source refuses.
    let failure = typed.narrow_structure_to_u16(LIMIT, 0, None).err().unwrap();
    assert!(failure.contains("already stores 16-bit"), "{failure}");
    // The typed native round trip.
    let first = typed.encode_native(LIMIT, 0, None).unwrap();
    let reopened = PrecisionDocument::parse_native(&first, LIMIT, 0, None).unwrap();
    assert!(first == reopened.encode_native(LIMIT, 0, None).unwrap());
}

/// The float→16 plan charges each re-prepared adjustment's measured
/// record plus ONE peak preparation allowance, so an added White Balance
/// raises the scratch figure by at least its three 65,537-entry f64 encoder
/// tables and by less than one allowance (`adjustment_repreparation.rs`
/// covers the other two plans). A calibrated Exposure holds no encoder since
/// #2.
#[test]
fn float_to_sixteen_charges_each_added_adjustment_below_one_allowance() {
    let directory = TempDir::new().unwrap();
    let float = float_fixture(&directory);
    let balance = json!({"type":"adjust","evaluation":"precision_v1","op":"white_balance",
                          "params":{"temperature":5200}})
    .to_string();
    let more = float
        .add_adjustment("White Balance 2", &balance, 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let before = float.narrow_structure_plan(NarrowTarget::Rgba16).unwrap();
    let after = more.narrow_structure_plan(NarrowTarget::Rgba16).unwrap();
    assert!(after.blocked.is_empty(), "{:?}", after.refusal());
    let delta = after.scratch_bytes - before.scratch_bytes;
    let allowance =
        PrecisionAdjustmentState::prepare_scratch_bytes(&balance, &IccProfile::srgb()).unwrap();
    assert!(
        delta >= 3 * 65_537 * 8,
        "delta {delta} is below the encoder tables"
    );
    assert!(
        delta < allowance,
        "delta {delta} is not below one allowance {allowance}"
    );
}

/// The float→16 plan walk admits each adjustment preparation BEFORE
/// it builds it. With room for the retained input and the bookkeeping but
/// not one preparation allowance, the op refuses from its plan at the
/// retained input plus the allowance (≥ its 4 MiB floor) plus the 4 MiB
/// bookkeeping — below the whole-operation figure it used to refuse with.
#[test]
fn planning_admits_each_preparation_before_building_it() {
    const MIB4: u64 = 4 * 1024 * 1024;
    let directory = TempDir::new().unwrap();
    let float = float_fixture(&directory);
    assert!((0..float.layer_count()).any(|i| float.adjustment_description(i).is_some()));
    let plan = float.narrow_structure_plan(NarrowTarget::Rgba16).unwrap();
    assert!(plan.blocked.is_empty(), "{:?}", plan.refusal());
    let whole = plan.retained_bytes + plan.scratch_bytes + plan.output_bytes;
    let floor = plan.retained_bytes + 2 * MIB4;
    let refusal = float
        .narrow_structure_to_u16(plan.retained_bytes + MIB4, 0, None)
        .err()
        .expect("refused");
    let n: u64 = refusal
        .strip_prefix("precision operation needs ")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("not an admission refusal: {refusal}"));
    assert!(n < whole, "{n} is not below the whole operation's {whole}");
    assert!(
        n >= floor,
        "{n} is below retained + allowance + bookkeeping {floor}"
    );
}

#[test]
fn float_to_eight_tone_maps_then_quantizes_every_record() {
    let directory = TempDir::new().unwrap();
    let float = float_fixture(&directory);
    let plan = float
        .narrow_structure_plan(NarrowTarget::Rgba8)
        .unwrap()
        .json();
    assert_eq!(plan["direction"], "rgba32_float_to_rgba8");
    assert_eq!(plan["tone_map"], json!({"operator":"clip"}));
    assert_eq!(plan["discarded"], json!(["extended_range"]));
    assert_eq!(plan["alpha_quantization"], "nearest, never dithered");
    assert_eq!(plan["blocked"], json!([]));
    for dither in [PrecisionDither::None, PrecisionDither::Ordered] {
        let back = narrow8(&float, dither);
        assert_eq!(back.evaluation(), Evaluation::LinearLight);
        assert_eq!(back.layers.len(), float.layer_count());
        for (i, layer) in back.layers.iter().enumerate() {
            let record = float.layer(i).unwrap();
            assert_eq!(Some(layer.id), float.layer_id(i));
            assert_eq!(layer.name, record.name());
            assert_eq!(layer.clipped, record.clipped());
            assert_eq!(layer.label, record.label());
            assert_eq!(layer.locks, record.locks());
            assert_eq!(layer.blend, record.blend_mode());
            if let Some(mask) = record.mask() {
                let expected: Vec<u8> = mask
                    .plane()
                    .samples()
                    .iter()
                    .map(|v| ((u32::from(*v) + 128) / 257) as u8)
                    .collect();
                assert_eq!(layer.mask.as_ref().unwrap().as_raw(), &expected);
                assert!(&layer.mask_properties == mask.properties());
            }
            let Some(source) = float.layer_source(i) else {
                continue;
            };
            for (f, b) in source
                .to_rgba32()
                .ok()
                .unwrap()
                .iter()
                .zip(layer.pixels.contiguous_image().pixels())
            {
                for c in 0..3 {
                    let oracle = (255.0 * srgb_encode(f64::from(f[c]))).round();
                    assert!(
                        (f64::from(b.0[c]) - oracle).abs() <= 1.0,
                        "{i}: {f:?} → {b:?}"
                    );
                }
                let alpha = (255.0 * f64::from(f[3])).round();
                assert!((f64::from(b.0[3]) - alpha).abs() <= 1.0);
            }
        }
        assert_eq!(back.layer_comps(), float.layer_comps());
        assert_eq!(back.guides, float.guides());
        let mut err = ptr::null_mut();
        let preview: Value = unsafe {
            serde_json::from_str(&text(dr_precision_doc_narrow_structure_preview(
                &float,
                &back,
                ptr::null(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )))
            .unwrap()
        };
        eprintln!("12H-2 float→8 ({dither:?}) preview delta: {preview}");
        assert!(preview["max_delta"].as_u64().unwrap() <= 771, "{preview}");
        assert!(
            preview["mean_delta"].as_f64().unwrap() <= 128.5,
            "{preview}"
        );
    }
}

// ------------------------------------------------ FFI, NULL, cancellation --

#[test]
fn narrowing_exports_are_null_safe() {
    let sixteen = promote(&byte_fixture());
    let back = narrow8(&sixteen, PrecisionDither::None);
    unsafe {
        assert!(dr_precision_doc_narrow_structure_plan(ptr::null(), 8, ptr::null_mut()).is_null());
        assert!(dr_precision_doc_narrow_structure_to_u8(
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_narrow_structure_to_u16(
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_narrow_structure_preview(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        // Neither and both converted handles are errors.
        for (u8_handle, u16_handle) in [
            (ptr::null(), ptr::null()),
            (
                &back as *const DrDocument,
                &sixteen as *const PrecisionDocument,
            ),
        ] {
            let mut err = ptr::null_mut();
            assert!(dr_precision_doc_narrow_structure_preview(
                &sixteen,
                u8_handle,
                u16_handle,
                LIMIT,
                0,
                ptr::null(),
                &mut err
            )
            .is_null());
            assert!(text(err).contains("exactly one converted document"));
        }
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_narrow_structure_to_u8(
            &sixteen,
            7,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!text(err).is_empty());
        let mut err = ptr::null_mut();
        let out =
            dr_precision_doc_narrow_structure_to_u8(&sixteen, 1, LIMIT, 0, ptr::null(), &mut err);
        assert!(!out.is_null());
        assert!(err.is_null());
        dr_doc_free(out);
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_narrow_structure_to_u16(
            &sixteen,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(text(err).contains("already stores 16-bit"));
        let directory = TempDir::new().unwrap();
        let float = float_fixture(&directory);
        let mut err = ptr::null_mut();
        let out = dr_precision_doc_narrow_structure_to_u16(&float, LIMIT, 0, ptr::null(), &mut err);
        assert!(!out.is_null());
        dr_precision_doc_free(out);
    }
}

#[test]
fn cancellation_and_capacity_release_the_token_and_leave_the_input() {
    let sixteen = promote(&byte_fixture());
    let before = sixteen.render(LIMIT, 0, None).unwrap();
    let token = DrJobControl::default();
    token.cancel();
    let failure = sixteen
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, Some(&token))
        .err()
        .unwrap();
    assert!(failure.contains("cancelled"), "{failure}");
    assert_eq!(token.scratch_bytes(), 0);
    let token = DrJobControl::default();
    unsafe {
        assert!(dr_job_control_set_scratch_limit(&token, 1));
    }
    assert!(sixteen
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, Some(&token))
        .is_err());
    assert_eq!(token.scratch_bytes(), 0);
    assert!(sixteen
        .narrow_structure_to_u8(PrecisionDither::None, 64 * 1024, 0, None)
        .is_err());
    let directory = TempDir::new().unwrap();
    let float = float_fixture(&directory);
    let token = DrJobControl::default();
    token.cancel();
    assert!(float
        .narrow_structure_to_u16(LIMIT, 0, Some(&token))
        .err()
        .unwrap()
        .contains("cancelled"));
    assert_eq!(token.scratch_bytes(), 0);
    let after = sixteen.render(LIMIT, 0, None).unwrap();
    assert_eq!(before.to_rgba16().ok(), after.to_rgba16().ok());
}
