//! Both style SETTERS refuse a key the style schema does not
//! name, by its parser path (`style_json_keys`), as the typed Blend If edit
//! refuses one ("unknown native blend-if field"): `dr_doc_set_layer_style`
//! answers "style: unknown layer style field <path>" and the typed
//! `layer_style` edit "invalid native layer style: unknown layer style field
//! <path>". The motivating case is take1's INSPECT k1: a drop shadow sent
//! with `blend_mode` (the key is `blend`) was silently drawn at Multiply.
//!
//! Every key the canonical writer emits is accepted back (each effect kind,
//! custom contours, gradients, pattern fills, a bevel's texture, Blend If
//! ranges, the knockout keys), the CMYK `ink` slots and the wide `linear`
//! slots included; a key valid on one effect kind is refused on another; the
//! parser itself stays tolerant (the lenient and import paths never run the
//! check); and a refused call changes nothing.

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};

use crate::common::{rect_layer_doc, take_err_string, try_styled};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const RED: [u8; 4] = [255, 0, 0, 255];

fn byte_doc() -> DrDocument {
    rect_layer_doc((24, 20), (6, 6, 8, 6), RED)
}

fn typed_doc() -> PrecisionDocument {
    let pixels = (0..24 * 20)
        .map(|i| {
            let (x, y) = (i % 24, i / 24);
            if (6..14).contains(&x) && (6..12).contains(&y) {
                [65535, 0, 0, 65535]
            } else {
                [0, 0, 0, 0]
            }
        })
        .collect();
    let image = PrecisionImage::from_rgba16(24, 20, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_parts(
        24,
        20,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("Rect", image, (0, 0), 1.0, true).unwrap()],
    )
    .unwrap()
}

/// The typed `layer_style` edit on layer 0: `Ok(None)` for a no-op.
fn typed_edit(doc: &PrecisionDocument, style: &Value) -> Result<Option<PrecisionDocument>, String> {
    let request =
        CString::new(json!({"op":"layer_style","version":1,"layer":0,"style":style}).to_string())
            .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        assert!(out.is_null());
        return Err(take_err_string(err));
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

/// Each style with an unknown key, and the path both setters name.
fn refused() -> Vec<(Value, &'static str)> {
    vec![
        (json!({"blend_mode":"normal"}), "blend_mode"),
        (
            json!({"effects":[{"type":"drop_shadow","blend_mode":"normal"}]}),
            "effects[0].blend_mode",
        ),
        (
            json!({"effects":[
                {"type":"color_overlay"},
                {"type":"gradient_overlay","gradient":{"stops":[
                    {"position":0,"color":"#000000"},
                    {"position":1,"colour":"#ffffff"}]}}]}),
            "effects[1].gradient.stops[1].colour",
        ),
        (
            json!({"blend_if":{"channel":"gray","this_layer":[0,0,128,128],
                   "extra":[{"channel":"red","under":[0,0,255,255]}]}}),
            "blend_if.extra[0].under",
        ),
        (
            json!({"blend_if":{"channel":"gray","mode":"this"}}),
            "blend_if.mode",
        ),
        (
            json!({"effects":[{"type":"outer_glow","contour":{"points":[
                {"x":0,"y":0},{"x":255,"y":255,"smooth":true}]}}]}),
            "effects[0].contour.points[1].smooth",
        ),
        (
            json!({"effects":[{"type":"pattern_overlay","pattern":{"preset":"checkerboard","tile":1}}]}),
            "effects[0].pattern.tile",
        ),
        (
            json!({"effects":[{"type":"stroke","fill_type":"pattern",
                   "pattern_fill":{"scale":1,"offset":[0,0]}}]}),
            "effects[0].pattern_fill.offset",
        ),
        (
            json!({"effects":[{"type":"bevel_emboss","texture_enabled":true,
                   "texture":{"depth":1,"strength":2}}]}),
            "effects[0].texture.strength",
        ),
        // A key of another kind: `spread` is a drop shadow's and an outer
        // glow's, not an inner glow's.
        (
            json!({"effects":[{"type":"inner_glow","spread":0.5}]}),
            "effects[0].spread",
        ),
        (
            json!({"effects":[{"type":"satin","noise":0.5}]}),
            "effects[0].noise",
        ),
    ]
}

#[test]
fn the_byte_setter_refuses_an_unknown_key_by_its_path() {
    let doc = byte_doc();
    for (style, path) in refused() {
        assert!(
            LayerStyle::from_json(&style.to_string()).is_ok(),
            "{style}: the parser itself accepts it"
        );
        let error = try_styled(&doc, 1, Some(&style.to_string())).err();
        assert_eq!(
            error.as_deref(),
            Some(format!("style: unknown layer style field {path}").as_str()),
            "{style}"
        );
    }
    assert!(doc.layers[1].style.is_none(), "nothing changed");
}

#[test]
fn the_typed_edit_refuses_an_unknown_key_by_its_path() {
    let doc = typed_doc();
    for (style, path) in refused() {
        assert_eq!(
            typed_edit(&doc, &style).err(),
            Some(format!(
                "invalid native layer style: unknown layer style field {path}"
            )),
            "{style}"
        );
    }
    assert!(doc.layer(0).unwrap().reported_style().is_none());
}

/// Every effect kind with every optional sub-object the writer emits.
fn rich() -> Value {
    json!({
        "fill_opacity": 0.75,
        "knockout": "shallow",
        "blend_interior_as_group": true,
        "blend_if": {"channel":"gray","this_layer":[0,10,200,255],"underlying":[0,0,255,255],
                     "extra":[{"channel":"red","this_layer":[0,0,255,255],"underlying":[5,20,255,255]}]},
        "effects": [
            {"type":"drop_shadow","blend":"multiply","color":"#102030","ink":[0.1,0.2,0.3,0.4],
             "linear":[0.5,0.25,0.125],"contour":{"points":[{"x":0,"y":0},{"x":128,"y":200,"corner":true},{"x":255,"y":255}]},
             "anti_aliased":true,"noise":0.1,"layer_knocks_out":false},
            {"type":"inner_shadow","choke":0.2,"contour":{"preset":"cone"}},
            {"type":"outer_glow","fill_type":"gradient","jitter":0.2,"range":0.6,
             "gradient":{"stops":[{"position":0,"color":"#ff0000","opacity":1,"ink":[0,1,1,0]},{"position":1,"color":"#0000ff"}]}},
            {"type":"inner_glow","source":"center"},
            {"type":"stroke","fill_type":"pattern","position":"inside",
             "gradient":{"style":"shape_burst","dither":true,"reverse":true,"align_with_layer":false,"angle":30,"scale":0.5},
             "pattern_fill":{"pattern":{"preset":"checkerboard"},"scale":2,"angle":15,"link_with_layer":false,"phase":[1,2],"anchor":[3,4]}},
            {"type":"color_overlay","color":"#00ff00"},
            {"type":"gradient_overlay","gradient":{"style":"radial","dither":true}},
            {"type":"pattern_overlay","pattern":{"preset":"checkerboard"},"scale":0.5,"phase":[2,1],"anchor":[1,1]},
            {"type":"bevel_emboss","technique":"chisel_soft","highlight_linear":[1.2,1.0,0.9],"shadow_ink":[0,0,0,1],
             "gloss_contour":{"preset":"ring"},"gloss_anti_aliased":true,"contour_enabled":true,
             "contour_anti_aliased":true,"contour_range":0.3,"texture_enabled":true,
             "texture":{"pattern":{"preset":"checkerboard"},"depth":-0.5,"invert":true,"scale":1.5,"phase":[0,3]}},
            {"type":"satin","invert":false,"contour":{"preset":"gaussian"}}
        ]
    })
}

#[test]
fn every_written_key_is_accepted_back_by_both_setters() {
    let style = rich();
    // Byte: the authored style, then its canonical form on a fresh layer.
    let doc = try_styled(&byte_doc(), 1, Some(&style.to_string()))
        .unwrap()
        .expect("a change");
    let canonical = doc.layers[1].style.as_ref().unwrap().to_json();
    let again = try_styled(&byte_doc(), 1, Some(&canonical))
        .unwrap()
        .expect("a change");
    assert_eq!(again.layers[1].style.as_ref().unwrap().to_json(), canonical);
    // Typed: the same, through `reported_style`.
    let typed = typed_edit(&typed_doc(), &style).unwrap().expect("a change");
    let reported = typed.layer(0).unwrap().reported_style().unwrap();
    let value: Value = serde_json::from_str(&reported).unwrap();
    let back = typed_edit(&typed_doc(), &value).unwrap().expect("a change");
    assert_eq!(back.layer(0).unwrap().reported_style().unwrap(), reported);
    // The canonical JSON names `version` and `anchor`; both are schema keys.
    assert!(canonical.contains("\"version\"") && canonical.contains("\"anchor\""));
}
