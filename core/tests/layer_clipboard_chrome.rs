//! What travels with copied LAYERS into another document — the
//! layer comps, alpha channels, guides, colour samplers and global light —
//! through `dr_doc_paste_layers` at 8 bits and `dr_precision_doc_paste_layers`
//! at 16 bits, the clip built by Duplicate to New Document as the host builds
//! it.
//!
//! Photoshop 26.10.0 measured every point (`scripts/photoshop-probe/
//! paste_layers.py`, `c3`; receipt `paste-layers-470c3/take1`): the copied
//! layers bring no comp, channel, guide or sampler, and the destination's own
//! are untouched (in place and centred alike); a pasted style that uses the
//! global light keeps the flag and casts by the DESTINATION's light, which the
//! paste leaves alone. Every one of those is the `none` branch, so what is
//! asserted here is today's paste. Comps are the one `phase2` point: applying
//! a comp HIDES a pasted layer the comp does not know (measured in
//! Photoshop); the paste itself leaves the destination's comps as they were.
//!
//! Oracles, none read off the implementation: the destination's comps JSON
//! (each comp's derived `unrecorded` list aside), channel count, guides,
//! samplers and light are the ones it had before the paste, and `unrecorded`
//! names exactly the two pasted layers; applying its comp after hiding its own
//! Background shows the Background again and hides both pasted layers; each
//! pasted style is the source's, byte for byte, `use_global_light` included.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::doc_comps::CompRequest;
use doctored_core::doc_guide::GuideOrientation;
use doctored_core::doc_sampler::ColorSampler;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_channel::dr_doc_channel_count;
use doctored_core::ffi_comps::{
    dr_doc_edit_layer_comps, dr_doc_layer_comps_json, dr_precision_doc_layer_comps_json,
};
use doctored_core::ffi_group::dr_doc_duplicate_to_new_document;
use doctored_core::ffi_layer_clipboard::{dr_doc_paste_layers, dr_precision_doc_paste_layers};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_style::{
    dr_doc_global_light_altitude, dr_doc_global_light_angle, dr_doc_layer_style,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionPlane};
use doctored_core::style::{GlobalLight, LayerStyle};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const ABOVE: u32 = 0;
const W: u32 = 64;
const H: u32 = 16;
const SOURCE_LIGHT: GlobalLight = GlobalLight {
    angle: 30.0,
    altitude: 45.0,
};
const DEST_LIGHT: GlobalLight = GlobalLight {
    angle: 150.0,
    altitude: 20.0,
};
/// `PG_lit`'s shadow reads the global light; `PG_local`'s casts at its own
/// 90 degrees (straight down), as the probe's `global_light` case authors
/// them.
const LIT: &str = r##"{"effects":[{"type":"drop_shadow","use_global_light":true,
    "distance":4,"size":0,"spread":0,"opacity":1,"color":"#000000","blend":"normal"}]}"##;
const LOCAL: &str = r##"{"effects":[{"type":"drop_shadow","use_global_light":false,
    "angle":90,"distance":4,"size":0,"spread":0,"opacity":1,"color":"#000000",
    "blend":"normal"}]}"##;

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

fn comp(request: Value) -> CompRequest {
    CompRequest::parse(&request.to_string()).expect("a comp request")
}

fn style(json: &str) -> Arc<LayerStyle> {
    Arc::new(LayerStyle::from_json(json).expect("a style"))
}

fn samplers() -> [ColorSampler; 2] {
    [
        ColorSampler {
            x: 5,
            y: 5,
            sample_size: 1,
        },
        ColorSampler {
            x: 40,
            y: 12,
            sample_size: 1,
        },
    ]
}

fn dest_sampler() -> ColorSampler {
    ColorSampler {
        x: 60,
        y: 2,
        sample_size: 3,
    }
}

// ================================================================= 8 bits ==

fn square(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]))
}

/// A grey 64 x 16 document under `light` holding only its Background.
fn grey8(light: GlobalLight) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([128, 128, 128, 255])));
    doc.layers[0].name = "Background".into();
    doc.set_global_light(light).expect("the light")
}

/// The probe's source: `PG_lit` and `PG_local` over grey, an alpha channel,
/// guides H 4 and V 20, samplers (5,5) and (40,12), comps `One` and `Two`,
/// and a 30/45 light.
fn source8() -> DrDocument {
    let mut doc = grey8(SOURCE_LIGHT)
        .adding_image_layer(0, square(16, 8), "PG_lit")
        .and_then(|d| d.adding_image_layer(1, square(16, 8), "PG_local"))
        .expect("two layers");
    doc.layers[1].offset = (8, 4);
    doc.layers[2].offset = (40, 4);
    let doc = doc
        .set_layer_style(1, Some(style(LIT)))
        .and_then(|d| d.set_layer_style(2, Some(style(LOCAL))))
        .expect("the styles");
    let plane: Vec<u8> = (0..W * H)
        .map(|i| if i % W < 32 { 255 } else { 0 })
        .collect();
    let mut doc = doc
        .add_channel("Alpha PT", &plane, W, H, [255, 0, 0], 0.5)
        .and_then(|d| d.add_guide(GuideOrientation::Horizontal, 4.0))
        .and_then(|d| d.add_guide(GuideOrientation::Vertical, 20.0))
        .and_then(|d| d.with_color_samplers(&samplers()))
        .expect("the chrome");
    doc.layers[1].visible = false;
    let mut doc = doc
        .edit_layer_comps(&comp(
            json!({"op":"layer_comp","action":"save","name":"One"}),
        ))
        .unwrap()
        .unwrap();
    doc.layers[1].visible = true;
    doc.layers[2].visible = false;
    let mut doc = doc
        .edit_layer_comps(&comp(
            json!({"op":"layer_comp","action":"save","name":"Two"}),
        ))
        .unwrap()
        .unwrap();
    doc.layers[2].visible = true;
    doc
}

/// The destination: grey under a 150/20 light, a V 50 guide, one sampler and
/// the comp `Dest` over its own Background.
fn dest8() -> DrDocument {
    grey8(DEST_LIGHT)
        .add_guide(GuideOrientation::Vertical, 50.0)
        .and_then(|d| d.with_color_samplers(&[dest_sampler()]))
        .expect("the chrome")
        .edit_layer_comps(&comp(
            json!({"op":"layer_comp","action":"save","name":"Dest"}),
        ))
        .unwrap()
        .unwrap()
}

/// The clip the host's Copy builds: `PG_lit` and `PG_local` duplicated to a
/// new document.
fn clip8(source: &DrDocument) -> DrDocument {
    let idx = [1usize, 2];
    let out = unsafe { dr_doc_duplicate_to_new_document(source, idx.as_ptr(), idx.len()) };
    assert!(!out.is_null(), "the clip");
    *unsafe { Box::from_raw(out) }
}

fn paste8(dest: &DrDocument, clip: &DrDocument, offset: (i32, i32)) -> DrDocument {
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_doc_paste_layers(dest, clip, 0, ABOVE, offset.0, offset.1, false, &mut err) };
    assert_eq!(owned(err), None, "the paste");
    assert!(!out.is_null(), "a paste is never a silent no-op");
    *unsafe { Box::from_raw(out) }
}

fn comps8(doc: &DrDocument) -> String {
    let mut err = ptr::null_mut();
    let json = owned(unsafe { dr_doc_layer_comps_json(doc, &mut err) });
    assert_eq!(owned(err), None);
    json.expect("a comps report")
}

/// The comps report with each comp's derived `unrecorded` key removed, so a
/// paste can be compared with the destination it landed in.
fn without_unrecorded(report: &str) -> Value {
    let mut value: Value = serde_json::from_str(report).expect("a comps report");
    for comp in value["comps"].as_array_mut().expect("comps") {
        comp.as_object_mut().expect("a comp").remove("unrecorded");
    }
    value
}

/// Each comp's `unrecorded` list, as (layer, name) pairs.
fn unrecorded(report: &str) -> Vec<Vec<(u64, String)>> {
    let value: Value = serde_json::from_str(report).expect("a comps report");
    value["comps"]
        .as_array()
        .expect("comps")
        .iter()
        .map(|comp| {
            comp["unrecorded"]
                .as_array()
                .expect("unrecorded")
                .iter()
                .map(|row| {
                    (
                        row["layer"].as_u64().expect("a layer"),
                        row["name"].as_str().expect("a name").to_string(),
                    )
                })
                .collect()
        })
        .collect()
}

fn pasted_names() -> Vec<Vec<(u64, String)>> {
    vec![vec![(1, "PG_lit".to_string()), (2, "PG_local".to_string())]]
}

fn light8(doc: &DrDocument) -> (f32, f32) {
    unsafe {
        (
            dr_doc_global_light_angle(doc),
            dr_doc_global_light_altitude(doc),
        )
    }
}

fn style8(doc: &DrDocument, idx: usize) -> String {
    owned(unsafe { dr_doc_layer_style(doc, idx) }).expect("a style")
}

#[test]
fn byte_paste_keeps_the_destination_chrome_and_brings_none_of_the_source() {
    let source = source8();
    let clip = clip8(&source);
    let dest = dest8();
    for offset in [(0, 0), (16, 8)] {
        let out = paste8(&dest, &clip, offset);
        assert_eq!(out.layers.len(), 3, "{offset:?}");
        assert_eq!(
            without_unrecorded(&comps8(&out)),
            without_unrecorded(&comps8(&dest)),
            "the comps, {offset:?}"
        );
        assert_eq!(unrecorded(&comps8(&out)), pasted_names(), "{offset:?}");
        assert_eq!(unsafe { dr_doc_channel_count(&out) }, 0, "no channel rides");
        assert_eq!(light8(&out), (150.0, 20.0), "the destination's light");
        assert_eq!(out.guides, dest.guides, "the guides, {offset:?}");
        assert_eq!(out.color_samplers(), &[dest_sampler()], "the samplers");
        assert_eq!(style8(&out, 1), style8(&source, 1), "PG_lit's style");
        assert_eq!(style8(&out, 2), style8(&source, 2), "PG_local's style");
        assert!(style8(&out, 1).contains(r#""use_global_light":true"#));
    }
}

#[test]
fn byte_destination_comp_hides_the_pasted_layers() {
    let mut out = paste8(&dest8(), &clip8(&source8()), (0, 0));
    out.layers[0].visible = false;
    let request = CString::new(r#"{"op":"layer_comp","action":"apply","comp":0}"#).unwrap();
    let mut err = ptr::null_mut();
    let applied = unsafe { dr_doc_edit_layer_comps(&out, request.as_ptr(), &mut err) };
    assert_eq!(owned(err), None);
    assert!(!applied.is_null(), "the Background comes back");
    let applied = *unsafe { Box::from_raw(applied) };
    assert!(applied.layers[0].visible, "the comp's own layer");
    assert!(
        !applied.layers[1].visible && !applied.layers[2].visible,
        "a pasted layer the comp does not know is hidden"
    );
}

// ================================================================ 16 bits ==

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
    assert_eq!(owned(err), None, "{request}");
    assert!(!out.is_null(), "{request} changed nothing");
    *unsafe { Box::from_raw(out) }
}

fn raster16(name: &str, w: u32, h: u32, v: u16, offset: (i32, i32)) -> Layer {
    let px = vec![[v, v, v, 65535]; (w * h) as usize];
    let image = PrecisionImage::from_rgba16(w, h, px, IccProfile::srgb(), None).unwrap();
    Layer::raster(name, image, offset, 1.0, true).unwrap()
}

fn grey16(
    layers: Vec<Layer>,
    channels: Vec<PrecisionChannel>,
    light: GlobalLight,
) -> PrecisionDocument {
    let mut all = vec![raster16("Background", W, H, 32896, (0, 0))];
    all.extend(layers);
    PrecisionDocument::from_parts_and_channels(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        all,
        channels,
    )
    .unwrap()
    .with_global_light(light)
    .unwrap()
    .unwrap()
}

fn source16() -> PrecisionDocument {
    let plane: Vec<u16> = (0..W * H)
        .map(|i| if i % W < 32 { 65535 } else { 0 })
        .collect();
    let channel = PrecisionChannel::new(
        "Alpha PT",
        PrecisionPlane::from_u16(W, H, plane, None).unwrap(),
    )
    .unwrap();
    let doc = grey16(
        vec![
            raster16("PG_lit", 16, 8, 65535, (8, 4)),
            raster16("PG_local", 16, 8, 65535, (40, 4)),
        ],
        vec![channel],
        SOURCE_LIGHT,
    );
    let lit: Value = serde_json::from_str(LIT).unwrap();
    let local: Value = serde_json::from_str(LOCAL).unwrap();
    let doc = edit(
        &doc,
        json!({"op":"layer_style","version":1,"layer":1,"style":lit}),
    );
    let doc = edit(
        &doc,
        json!({"op":"layer_style","version":1,"layer":2,"style":local}),
    );
    let doc = doc
        .add_guide(GuideOrientation::Horizontal, 4.0)
        .and_then(|d| d.add_guide(GuideOrientation::Vertical, 20.0))
        .and_then(|d| d.with_color_samplers(&samplers()))
        .expect("the chrome");
    let doc = edit(
        &doc,
        json!({"op":"layer_visible","layer":1,"visible":false}),
    );
    let doc = edit(
        &doc,
        json!({"op":"layer_comp","action":"save","name":"One"}),
    );
    let doc = edit(&doc, json!({"op":"layer_visible","layer":1,"visible":true}));
    let doc = edit(
        &doc,
        json!({"op":"layer_visible","layer":2,"visible":false}),
    );
    let doc = edit(
        &doc,
        json!({"op":"layer_comp","action":"save","name":"Two"}),
    );
    edit(&doc, json!({"op":"layer_visible","layer":2,"visible":true}))
}

fn dest16() -> PrecisionDocument {
    let doc = grey16(Vec::new(), Vec::new(), DEST_LIGHT)
        .add_guide(GuideOrientation::Vertical, 50.0)
        .and_then(|d| d.with_color_samplers(&[dest_sampler()]))
        .expect("the chrome");
    edit(
        &doc,
        json!({"op":"layer_comp","action":"save","name":"Dest"}),
    )
}

fn paste16(
    dest: &PrecisionDocument,
    clip: &PrecisionDocument,
    offset: (i32, i32),
) -> PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_paste_layers(
            dest,
            clip,
            0,
            ABOVE,
            offset.0,
            offset.1,
            false,
            LIMIT,
            0,
            ptr::null_mut(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None, "the paste");
    assert!(!out.is_null(), "a paste is never a silent no-op");
    *unsafe { Box::from_raw(out) }
}

fn comps16(doc: &PrecisionDocument) -> String {
    let mut err = ptr::null_mut();
    let json = owned(unsafe { dr_precision_doc_layer_comps_json(doc, &mut err) });
    assert_eq!(owned(err), None);
    json.expect("a comps report")
}

fn style16(doc: &PrecisionDocument, idx: usize) -> String {
    doc.layer(idx)
        .and_then(|l| l.style())
        .map(|s| s.to_json())
        .expect("a style")
}

#[test]
fn sixteen_bit_paste_keeps_the_destination_chrome_and_brings_none_of_the_source() {
    let source = source16();
    let clip = edit(
        &source,
        json!({"op":"duplicate_to_new_document","layers":[1, 2]}),
    );
    let dest = dest16();
    for offset in [(0, 0), (16, 8)] {
        let out = paste16(&dest, &clip, offset);
        assert_eq!(out.layer_count(), 3, "{offset:?}");
        assert_eq!(
            without_unrecorded(&comps16(&out)),
            without_unrecorded(&comps16(&dest)),
            "the comps, {offset:?}"
        );
        assert_eq!(unrecorded(&comps16(&out)), pasted_names(), "{offset:?}");
        assert!(out.channels().is_empty(), "no channel rides");
        assert_eq!(out.global_light(), DEST_LIGHT, "the destination's light");
        assert_eq!(out.guides(), dest.guides(), "the guides, {offset:?}");
        assert_eq!(out.color_samplers(), &[dest_sampler()], "the samplers");
        assert_eq!(style16(&out, 1), style16(&source, 1), "PG_lit's style");
        assert_eq!(style16(&out, 2), style16(&source, 2), "PG_local's style");
        assert!(style16(&out, 1).contains(r#""use_global_light":true"#));
    }
}

#[test]
fn sixteen_bit_destination_comp_hides_the_pasted_layers() {
    let clip = edit(
        &source16(),
        json!({"op":"duplicate_to_new_document","layers":[1, 2]}),
    );
    let out = paste16(&dest16(), &clip, (0, 0));
    let hidden = edit(
        &out,
        json!({"op":"layer_visible","layer":0,"visible":false}),
    );
    let applied = edit(
        &hidden,
        json!({"op":"layer_comp","action":"apply","comp":0}),
    );
    let shown = |i: usize| applied.layer(i).unwrap().visible();
    assert!(shown(0), "the comp's own layer");
    assert!(
        !shown(1) && !shown(2),
        "a pasted layer the comp does not know is hidden"
    );
}
