//! 12H-1 structure-preserving 8→16 conversion: every carried record, the
//! bit-exact widening, an independent f64 source-over oracle, the fixed
//! preview-delta bound, the plan's shape, each blocked kind refusing by name
//! with the input untouched, native round trip, NULL safety and cancellation;
//! and (12H-2) retained-source and warp carriage, the document-row wording.
use doctored_core::doc::BlendMode;
use doctored_core::doc::{DrDocument, Layer, LayerKind, LayerRaster};
use doctored_core::doc_comps::CompRequest;
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_guide::GuideOrientation;
use doctored_core::doc_lock::{LOCK_PIXELS, LOCK_POSITION};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::dr_job_control_set_scratch_limit;
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_conversion::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::Resolution;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::retained_source::SourceEdit;
use doctored_core::style::{GlobalLight, LayerStyle};
use doctored_core::warp::WarpEdit;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr};
use std::ptr;
use std::sync::Arc;

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

/// A fresh layer with its own minted identity.
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

/// Every record kind this slice carries, at mixed levels.
fn fixture() -> DrDocument {
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
    let mut text = fresh(pixels(14, 6, 7, true), "Title");
    text.offset = (2, 20);
    text.meta = Some(r#"{"type":"text","version":1,"string":"Hi"}"#.into());
    let mut styled = fresh(pixels(16, 12, 8, true), "Styled");
    styled.offset = (18, 9);
    styled.style = Some(Arc::new(LayerStyle::from_json(STYLE).unwrap()));
    styled.locks = LOCK_PIXELS;
    doc.layers.extend([
        masked, clipped, child_a, child_b, isolated, pass_child, levels, pass, text, styled,
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

fn convert(doc: &DrDocument) -> PrecisionDocument {
    doc.promote_structure(LIMIT, 0, None)
        .unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn structure_ids_masks_channels_styles_and_chrome_are_carried() {
    let doc = fixture();
    let typed = convert(&doc);
    assert_eq!(typed.layer_count(), doc.layers.len());
    for (i, layer) in doc.layers.iter().enumerate() {
        let record = typed.layer(i).unwrap();
        assert_eq!(typed.layer_id(i), Some(layer.id), "id {i}");
        assert_eq!(record.name(), layer.name, "name {i}");
        assert_eq!(record.opacity(), layer.opacity, "opacity {i}");
        assert_eq!(record.visible(), layer.visible, "visible {i}");
        assert_eq!(record.clipped(), layer.clipped, "clipped {i}");
        assert_eq!(record.label(), layer.label, "label {i}");
        assert_eq!(record.locks(), layer.locks, "locks {i}");
        assert_eq!(record.link(), layer.link, "link {i}");
        assert_eq!(record.blend_mode(), layer.blend, "blend {i}");
        match layer.kind {
            LayerKind::Group => {
                assert!(record.children().is_some(), "group {i}");
                assert_eq!(
                    record.is_pass_through(),
                    layer.blend == BlendMode::PassThrough
                );
            }
            LayerKind::Raster if layer.meta.as_deref().is_some_and(|m| m.contains("adjust")) => {
                let description = typed.adjustment_description(i).unwrap();
                let value: Value = serde_json::from_str(description).unwrap();
                assert_eq!(value["op"], "levels");
                assert_eq!(value["params"]["gamma"], 0.8);
            }
            LayerKind::Raster => {
                assert_eq!(record.offset(), layer.offset, "offset {i}");
                assert_eq!(typed.artwork_description(i), layer.meta.as_deref());
            }
        }
        match &layer.mask {
            Some(_) => {
                let mask = record.mask().unwrap();
                assert!(
                    mask.properties() == &layer.mask_properties,
                    "mask props {i}"
                );
                assert_eq!(mask.enabled(), layer.mask_enabled);
            }
            None => assert!(record.mask().is_none()),
        }
        assert_eq!(
            record.reported_style(),
            layer.style.as_ref().map(|s| s.to_json()),
            "style {i}"
        );
    }
    let styled = typed.layer(10).unwrap();
    assert!(styled.blend_if().is_some());
    assert!(styled.reported_style().unwrap().contains("shallow"));
    assert_eq!(typed.channels().len(), 1);
    let channel = &typed.channels()[0];
    assert_eq!(channel.name(), "Saved");
    assert_eq!(channel.metadata().overlay_color, [0, 128, 255]);
    assert_eq!(channel.metadata().overlay_opacity, 0.25);
    assert!(!channel.metadata().color_indicates_selected);
    assert!(channel.metadata().provenance.is_none());
    assert_eq!(typed.layer_comps(), doc.layer_comps());
    assert_eq!(typed.guides(), doc.guides.as_slice());
    assert_eq!(typed.ruler_origin(), (3.0, 4.0));
    assert_eq!(typed.resolution().x, 240.0);
    assert_eq!(typed.resolution().y, 180.0);
    assert_eq!(typed.profile().bytes(), doc.profile.bytes());
    assert_eq!(typed.metadata(), &doc.metadata);
    assert_eq!(typed.evaluation(), Evaluation::Encoded);
}

#[test]
fn widening_is_bit_exact_for_rasters_masks_and_channels() {
    let doc = fixture();
    let typed = convert(&doc);
    for (i, layer) in doc.layers.iter().enumerate() {
        if let Some(source) = typed.layer_source(i) {
            let expected: Vec<[u16; 4]> = layer
                .pixels
                .contiguous_image()
                .pixels()
                .map(|p| p.0.map(|v| u16::from(v) * 257))
                .collect();
            assert_eq!(
                source.to_rgba16().ok().unwrap(),
                expected.as_slice(),
                "raster {i}"
            );
            assert_eq!(source.dimensions(), layer.pixels.dimensions());
        }
        if let Some(mask) = &layer.mask {
            let expected: Vec<u16> = mask.as_raw().iter().map(|v| u16::from(*v) * 257).collect();
            let plane = typed.layer(i).unwrap().mask().unwrap().plane();
            assert_eq!(plane.samples(), expected.as_slice(), "mask {i}");
        }
    }
    let expected: Vec<u16> = doc.channels[0]
        .data
        .as_raw()
        .iter()
        .map(|v| u16::from(*v) * 257)
        .collect();
    assert_eq!(typed.channels()[0].plane().samples(), expected.as_slice());
}

/// Straight source-over in f64 on normalized values.
fn over(top: [f64; 4], opacity: f64, bottom: [f64; 4]) -> [f64; 4] {
    let at = top[3] * opacity;
    let ab = bottom[3];
    let ao = at + ab * (1.0 - at);
    if ao == 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = (top[c] * at + bottom[c] * ab * (1.0 - at)) / ao;
    }
    out[3] = ao;
    out
}

#[test]
fn pointwise_typed_render_matches_an_f64_source_over_oracle() {
    let mut doc = DrDocument::from_pixels(pixels(W, H, 9, true));
    let mut top = fresh(pixels(22, 17, 10, true), "Top");
    top.offset = (7, 5);
    top.opacity = 0.65;
    doc.layers.push(top);
    let typed = convert(&doc);
    let render = typed.render(LIMIT, 0, None).unwrap();
    let samples = render.to_rgba16().ok().unwrap();
    let norm = |p: &Rgba<u8>| p.0.map(|v| f64::from(u16::from(v) * 257) / 65535.0);
    let mut worst = 0.0_f64;
    let bottom_image = doc.layers[0].pixels.contiguous_image();
    let top_image = doc.layers[1].pixels.contiguous_image();
    for y in 0..H {
        for x in 0..W {
            let bottom = norm(bottom_image.get_pixel(x, y));
            let (lx, ly) = (x as i32 - 7, y as i32 - 5);
            let expected = if (0..22).contains(&lx) && (0..17).contains(&ly) {
                over(
                    norm(top_image.get_pixel(lx as u32, ly as u32)),
                    // The layer's 65 % at its byte (#2).
                    crate::common::byte_unit(f64::from(0.65_f32)),
                    bottom,
                )
            } else {
                bottom
            };
            let actual = samples[(y * W + x) as usize];
            let channels = if expected[3] == 0.0 { 3..4 } else { 0..4 };
            for c in channels {
                let delta = (f64::from(actual[c]) - expected[c] * 65535.0).abs();
                worst = worst.max(delta);
            }
        }
    }
    assert!(
        worst <= 1.0,
        "typed render differs from the f64 oracle by {worst} codes"
    );
}

#[test]
fn full_fixture_preview_delta_is_within_the_fixed_bound() {
    let doc = fixture();
    let typed = convert(&doc);
    let mut err = ptr::null_mut();
    let preview: Value = unsafe {
        serde_json::from_str(&text(dr_doc_promote_structure_preview(
            &doc,
            &typed,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )))
        .unwrap()
    };
    eprintln!("12H-1 full-fixture preview delta: {preview}");
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
    let doc = fixture();
    let mut err = ptr::null_mut();
    let plan: Value = unsafe {
        serde_json::from_str(&text(dr_doc_promote_structure_plan(&doc, &mut err))).unwrap()
    };
    assert_eq!(plan["flattened"], false);
    assert_eq!(plan["structure"], "preserve");
    assert_eq!(plan["direction"], "rgba8_to_rgba16");
    assert_eq!(plan["widening"], "code_times_257");
    assert_eq!(
        plan["preserved"],
        json!([
            "raster_pixels_times_257",
            "layer_offsets",
            "names",
            "opacity",
            "visibility",
            "blend_modes",
            "groups",
            "pass_through_groups",
            "group_open_state",
            "adjustment_layers",
            "text_shape_descriptions",
            "layer_masks",
            "saved_channels",
            "layer_styles",
            "global_light",
            "blend_if",
            "knockout",
            "clipping",
            "locks",
            "links",
            "labels",
            "layer_ids",
            "layer_comps",
            "guides",
            "ruler_origin",
            "print_resolution",
            "icc_profile",
            "capture_metadata",
            "evaluation_policy"
        ])
    );
    assert_eq!(plan["discarded"], json!([]));
    assert_eq!(plan["blocked"], json!([]));
    // Output: every raster ×8, the one mask and the one channel ×2.
    let rasters: u64 = doc
        .layers
        .iter()
        .filter(|l| {
            l.kind == LayerKind::Raster
                && l.meta.as_deref()
                    != Some(r#"{"type":"adjust","op":"levels","params":{"gamma":0.8}}"#)
        })
        .map(|l| u64::from(l.pixels.width()) * u64::from(l.pixels.height()) * 8)
        .sum();
    assert_eq!(
        plan["output_bytes"].as_u64().unwrap(),
        rasters + 17 * 11 * 2 + u64::from(W * H) * 2
    );
    let notes: Vec<(u64, String)> = plan["rendering_differences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            assert!(row["id"].as_u64().unwrap() > 0);
            (
                row["index"].as_u64().unwrap(),
                row["note"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(notes
        .iter()
        .any(|(i, n)| *i == 5 && n.contains("isolated group")));
    assert!(notes
        .iter()
        .any(|(i, n)| *i == 7 && n.contains("adjustment")));
    assert!(notes.iter().any(|(i, n)| *i == 10 && n.contains("style")));
    assert!(notes
        .iter()
        .any(|(i, n)| *i == 10 && n.contains("Blend If")));
    assert!(notes
        .iter()
        .any(|(i, n)| *i == 1 && n.contains("mask feather")));
    assert!(!notes.iter().any(|(i, _)| *i == 8 || *i == 9));
    // A non-default global light carries: nothing is discarded and no
    // styled layer is noted for it.
    let lit = doc
        .set_global_light(GlobalLight {
            angle: 45.0,
            altitude: 60.0,
        })
        .unwrap();
    let plan = lit.promote_structure_plan().unwrap().json();
    assert_eq!(plan["discarded"], json!([]));
    assert!(!plan["rendering_differences"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["note"].as_str().unwrap().contains("global light")));
}

fn refuses(doc: &DrDocument, needle: &str) -> String {
    let plan = doc.promote_structure_plan().unwrap();
    let reason = plan.refusal().expect("a blocked plan");
    assert!(reason.contains(needle), "{reason}");
    let failure = doc.promote_structure(LIMIT, 0, None).err().unwrap();
    assert_eq!(failure, reason);
    reason
}

#[test]
fn each_blocked_kind_refuses_by_name_with_the_input_untouched() {
    let base = fixture();
    let before = base.flattened().unwrap();

    let mut host = base.clone();
    host.layers[9].meta = Some(r#"{"type":"live_photo","version":1}"#.into());
    let reason = refuses(&host, "host record");
    assert!(reason.contains("layer 9 \"Title\""), "{reason}");

    let mut adjustment = base.clone();
    adjustment.layers[7].meta =
        Some(r#"{"type":"adjust","op":"levels","params":{"gamma":0.8},"host":1}"#.into());
    refuses(
        &adjustment,
        "the levels adjustment has no 16-bit evaluation",
    );

    let mut blend = base.clone();
    blend.layers[1].blend = BlendMode::PassThrough;
    refuses(&blend, "Pass Through requires a group");

    // 12H-1 review nit 2: invalid mask properties and a group carrying meta.
    let mut mask = base.clone();
    mask.layers[1].mask_properties.outside_coverage = 7;
    let reason = refuses(
        &mask,
        "its layer mask properties exceed the typed mask limits",
    );
    assert!(reason.contains("layer 1 \"Masked\""), "{reason}");
    let mut meta = base.clone();
    meta.layers[5].meta = Some(r#"{"host":1}"#.into());
    let reason = refuses(&meta, "a group carrying host metadata has no 16-bit record");
    assert!(reason.contains("layer 5 \"Isolated\""), "{reason}");
    // A linear-light byte document holding an operator the typed walk
    // refuses is UNREACHABLE: the byte `refused_operator` refuses a superset
    // of the typed one (a style, the seven blend modes, a guided
    // Shadows/Highlights), so such a document cannot be built. A warp
    // binding and a retained source now CARRY (the carriage tests below).

    assert_eq!(base.flattened().unwrap(), before);
    assert_eq!(base.layers.len(), 11);
    assert!(base.layers[9].meta.as_deref().unwrap().contains("text"));
}

#[test]
fn a_document_row_names_no_layer() {
    let mut doc = fixture();
    doc.channels[0].name = "c".repeat(100_000);
    let plan = doc.promote_structure_plan().unwrap();
    assert_eq!(plan.blocked.len(), 1);
    assert_eq!(plan.blocked[0].id, 0);
    let reason = plan.refusal().unwrap();
    assert_eq!(
        reason,
        format!(
            "Preserve Layers cannot convert this document: {}",
            plan.blocked[0].note
        )
    );
    assert_eq!(doc.promote_structure(LIMIT, 0, None).err(), Some(reason));
}

/// A 4×4 background under a 1×`h` raster named "Tall" whose inner shadow at
/// `distance` (size 0, choke 0) reaches `distance + 2` pixels, so its padded
/// plane is `(1 + 2·pad)·(h + 2·pad)` without any 100 MP of pixels.
fn tall(h: u32, distance: u32) -> DrDocument {
    let mut doc = DrDocument::from_pixels(pixels(4, 4, 3, false));
    let mut layer = fresh(
        RgbaImage::from_pixel(1, h, Rgba([200, 10, 10, 255])),
        "Tall",
    );
    layer.style = Some(Arc::new(
        LayerStyle::from_json(&format!(
            r#"{{"effects":[{{"type":"inner_shadow","distance":{distance},"size":0,"choke":0}}]}}"#
        ))
        .unwrap(),
    ));
    doc.layers.push(layer);
    doc
}

#[test]
fn an_oversize_style_plane_blocks_by_name_and_the_limit_itself_carries() {
    // `MAX_PIXELS` is `pub(crate)`: 100 MP, written out. Without this gate
    // the plan passes and only the op fails, with the constructor's sentence.
    const MAX_PIXELS: u64 = 100_000_000;
    let pad = |doc: &DrDocument| doc.layers[1].style.as_ref().unwrap().pad();
    let plane = |doc: &DrDocument| {
        let (w, h) = doc.layers[1].pixels.dimensions();
        u64::from(w + 2 * pad(doc)) * u64::from(h + 2 * pad(doc))
    };

    // pad = 310 + 2 = 312, so (1 + 624)·(159_377 + 624) = 100_000_625.
    let over = tall(159_377, 310);
    assert_eq!(pad(&over), 312);
    assert_eq!(plane(&over), MAX_PIXELS + 625);
    let before = over.flattened().unwrap();
    let reason = refuses(
        &over,
        "its layer style's padded plane exceeds the pixel limit",
    );
    assert!(reason.contains("layer 1 \"Tall\""), "{reason}");
    let plan = over.promote_structure_plan().unwrap();
    assert_eq!(plan.blocked.len(), 1);
    assert_eq!(plan.blocked[0].index, 1);
    assert_eq!(plan.blocked[0].id, over.layers[1].id);
    assert_eq!(over.flattened().unwrap(), before);
    assert!(over.layers[1].style.is_some());

    // (1 + 624)·(159_376 + 624) = 100_000_000 exactly: `>` admits it, so the
    // limit itself converts and carries the style. Never flatten this one —
    // it renders the whole ~100 MP plane.
    let at = tall(159_376, 310);
    assert_eq!(pad(&at), 312);
    assert_eq!(plane(&at), MAX_PIXELS);
    assert!(at.promote_structure_plan().unwrap().refusal().is_none());
    assert!(convert(&at).layer(1).unwrap().style().is_some());

    // The pad caps at `style::MAX_PAD` (1024): (1 + 2048)·(48_757 + 2048)
    // is about 104.1 MP whatever the reach formula says.
    let capped = tall(48_757, 2000);
    assert_eq!(pad(&capped), 1024);
    assert!(plane(&capped) > MAX_PIXELS);
    refuses(
        &capped,
        "its layer style's padded plane exceeds the pixel limit",
    );
}

fn placed() -> DrDocument {
    let master = DrDocument::from_pixels(pixels(6, 4, 11, false));
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"12h-master","revision_id":"v1",
                "name":"Retained","x":2,"y":1})
        .to_string(),
    )
    .unwrap();
    fixture()
        .source_edit(Some(&master), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

#[test]
fn a_retained_source_is_carried_and_stays_effective() {
    let doc = placed();
    let index = doc.layers.len() - 1;
    let plan = doc.promote_structure_plan().unwrap();
    assert!(plan.blocked.is_empty(), "{:?}", plan.refusal());
    assert!(plan
        .rendering_differences
        .iter()
        .any(|row| row.index == index
            && row
                .note
                .contains("source re-renders at 16 bits on its next edit")));
    let typed = convert(&doc);
    let record = typed.layer(index).unwrap();
    let source = record.effective_source().expect("the binding is effective");
    assert_eq!(source.asset.id, "12h-master");
    // Its cached rendering is the byte fallback ×257.
    let widened: Vec<[u16; 4]> = doc.layers[index]
        .pixels
        .contiguous_image()
        .pixels()
        .map(|p| p.0.map(|v| u16::from(v) * 257))
        .collect();
    assert_eq!(
        source.rendered.to_rgba16().ok().unwrap(),
        widened.as_slice()
    );
    assert_eq!(typed.source_info(index).unwrap()["valid"], true);
    // A rebuild runs `clean_typed_layers`; the binding survives it.
    let rebuilt = typed.duplicate_layer(0).unwrap().unwrap();
    assert!(rebuilt
        .layer(index + 1)
        .unwrap()
        .effective_source()
        .is_some());
    // A STALE binding is disclosed and not carried.
    let mut stale = doc.clone();
    stale.layers[index].pixels = LayerRaster::new(pixels(6, 4, 12, true));
    let plan = stale.promote_structure_plan().unwrap();
    assert!(plan
        .rendering_differences
        .iter()
        .any(|row| row.index == index
            && row.note == "stale binding not carried; a save drops it too"));
    assert!(convert(&stale)
        .layer(index)
        .unwrap()
        .effective_source()
        .is_none());
}

fn warp_field() -> WarpEdit {
    let values: Vec<[f64; 2]> = (0..3)
        .flat_map(|y| {
            (0..3).map(move |x| {
                let (fx, fy) = (f64::from(x) / 2.0, f64::from(y) / 2.0);
                [0.35 + 0.58 * fx - 0.31 * fy, -0.27 + 0.33 * fy + 0.19 * fx]
            })
        })
        .collect();
    WarpEdit::parse(
        &json!({"op":"set","layer":0,"target":"pixels","columns":3,"rows":3,
            "displacements":values,"filter":"bicubic"})
        .to_string(),
    )
    .unwrap()
}

#[test]
fn a_warp_binding_is_carried_with_widened_originals() {
    let doc = fixture()
        .warp_edit(&warp_field(), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(doc.warp_info(0).unwrap()["field"].is_object());
    let plan = doc.promote_structure_plan().unwrap();
    assert!(plan.blocked.is_empty(), "{:?}", plan.refusal());
    assert!(plan
        .rendering_differences
        .iter()
        .any(|row| row.index == 0
            && row.note == "warp re-edits at 16 bits from the widened original"));
    let typed = convert(&doc);
    assert!(typed.warp_info(0).unwrap()["field"].is_object());
    let rebuilt = typed.duplicate_layer(3).unwrap().unwrap();
    assert!(rebuilt.warp_info(0).unwrap()["field"].is_object());
    // Clearing restores the retained original: the byte original ×257.
    let cleared = typed
        .warp_edit(
            &WarpEdit::parse(r#"{"op":"clear","layer":0}"#).unwrap(),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let original: Vec<[u16; 4]> = fixture().layers[0]
        .pixels
        .contiguous_image()
        .pixels()
        .map(|p| p.0.map(|v| u16::from(v) * 257))
        .collect();
    assert_eq!(
        cleared.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        original.as_slice()
    );
}

#[test]
fn more_than_the_record_cap_is_the_plans_first_row_not_the_constructors_sentence() {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(2, 2, Rgba([9, 8, 7, 255])));
    for i in 0..1024 {
        doc.layers.push(fresh(
            RgbaImage::from_pixel(1, 1, Rgba([1, 2, 3, 255])),
            &format!("L{i}"),
        ));
    }
    assert_eq!(doc.layers.len(), 1025);
    let plan = doc.promote_structure_plan().unwrap();
    let first = &plan.blocked[0];
    assert_eq!(first.index, 1024);
    assert_eq!(first.id, doc.layers[1024].id);
    assert_eq!(first.name, "L1023");
    assert!(first
        .note
        .contains("1025 layers exceed the 1024-record limit of a 16-bit document"));
    let failure = doc.promote_structure(LIMIT, 0, None).err().unwrap();
    assert_eq!(Some(failure.clone()), plan.refusal());
    assert!(
        !failure.contains("high-depth document exceeds"),
        "{failure}"
    );
    assert_eq!(doc.layers.len(), 1025);
}

#[test]
fn linear_light_policy_carries_with_rebuilt_tables() {
    let mut doc = DrDocument::from_pixels(pixels(W, H, 12, true));
    doc.layers.push(fresh(pixels(10, 10, 13, true), "Top"));
    let doc = doc
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .unwrap();
    let typed = convert(&doc);
    assert_eq!(typed.evaluation(), Evaluation::LinearLight);
    assert!(typed.linear_light_refusal().is_none());
}

#[test]
fn native_save_reopen_resave_is_byte_identical() {
    let typed = convert(&fixture());
    let first = typed.encode_native(LIMIT, 0, None).unwrap();
    let reopened = PrecisionDocument::parse_native(&first, LIMIT, 0, None).unwrap();
    let second = reopened.encode_native(LIMIT, 0, None).unwrap();
    assert!(first == second, "reopen → resave differs");
    assert_eq!(reopened.layer_count(), typed.layer_count());
    for i in 0..typed.layer_count() {
        assert_eq!(reopened.layer_id(i), typed.layer_id(i));
    }
}

#[test]
fn structure_exports_are_null_safe() {
    let doc = fixture();
    unsafe {
        assert!(dr_doc_promote_structure_plan(ptr::null(), ptr::null_mut()).is_null());
        assert!(
            dr_doc_promote_structure(ptr::null(), LIMIT, 0, ptr::null(), ptr::null_mut()).is_null()
        );
        assert!(dr_doc_promote_structure_preview(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        let mut err = ptr::null_mut();
        assert!(dr_doc_promote_structure_preview(
            &doc,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(!text(err).is_empty());
        let mut err = ptr::null_mut();
        let out = dr_doc_promote_structure(&doc, LIMIT, 0, ptr::null(), &mut err);
        assert!(!out.is_null());
        assert!(err.is_null());
        dr_precision_doc_free(out);
    }
}

#[test]
fn cancellation_and_capacity_release_the_token_and_leave_the_input() {
    let doc = fixture();
    let before = doc.flattened().unwrap();
    let token = DrJobControl::default();
    token.cancel();
    let failure = doc.promote_structure(LIMIT, 0, Some(&token)).err().unwrap();
    assert!(failure.contains("cancelled"), "{failure}");
    assert_eq!(token.scratch_bytes(), 0);
    let token = DrJobControl::default();
    unsafe {
        assert!(dr_job_control_set_scratch_limit(&token, 1));
    }
    assert!(doc.promote_structure(LIMIT, 0, Some(&token)).is_err());
    assert_eq!(token.scratch_bytes(), 0);
    assert!(doc.promote_structure(64 * 1024, 0, None).is_err());
    assert_eq!(doc.flattened().unwrap(), before);
}
