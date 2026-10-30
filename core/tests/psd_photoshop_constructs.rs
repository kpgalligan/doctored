//! The two Photoshop-authored CONSTRUCT fixtures, through the C API.
//!
//! `core/tests/fixtures/psd_photoshop/blend_if/` and `…/masks/` are documents
//! Adobe Photoshop 2025 (26.10.0) itself authored in Run 3
//! (`scripts/verify_phase10_photoshop_appearance.py --author`, provenance and
//! pin in `scripts/photoshop-probe/README.md`), so — like the `adjustments`
//! family beside them — their framing is one no Doctored code chose.
//!
//! Everything pinned below is a MEASUREMENT of what those files yield, taken
//! on 2026-09-16, not a premise about what Photoshop ought to write. Three of
//! those measurements are findings in their own right:
//!
//! - Photoshop writes a FULL `lfx2` on an ordinary pasted layer: four of its
//!   effect slots (outer glow, satin, inner glow, bevel & emboss) arrive with
//!   `enabled: false`, and the import carries them as disabled effects, so the
//!   layer renders as if it had none. The carry sentence names them anyway.
//! - Photoshop writes every raster mask ABSOLUTE (bit 0 of the record's flags
//!   clear), and trims the stored rectangle to the mask's nonzero extent —
//!   which is why `soft`, whose ramp starts at black, is one column narrower
//!   than `hard` from the same selection.
//! - `source_relative.psd` is the byte-patched twin (flags |= 1, rectangle made
//!   relative to the record's own top/left), the only way the construct exists
//!   at all. Photoshop IGNORES bit 0 and renders the rectangle where it stands
//!   (measured in Run 3's A5 receipt), and so does Doctored: each
//!   of the twin's masks sits at its stored rectangle, so the twin is a
//!   different picture from `source.psd`.

use std::ptr;

use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_doc::dr_doc_flattened;
use doctored_core::style::{BlendIf, BlendIfChannel};
use serde_json::Value;

use crate::common;
use common::pixels;
use common::psd_photoshop::{fixture, import_report, open_psd, strings, LAYERS};

// -------------------------------------------------------------- blend if --

/// One authored overlay: its layer name and the ramp Photoshop was given.
struct Overlay {
    name: &'static str,
    channel: &'static str,
    blend_if: BlendIf,
}

fn overlay(name: &'static str, channel: &'static str, ramp: BlendIf) -> Overlay {
    Overlay {
        name,
        channel,
        blend_if: ramp,
    }
}

fn overlays() -> Vec<Overlay> {
    vec![
        overlay(
            "this_gray_soft",
            "gray",
            BlendIf {
                channel: BlendIfChannel::Gray,
                this_layer: [32, 96, 160, 224],
                underlying: [0, 0, 255, 255],
                ..BlendIf::default()
            },
        ),
        overlay(
            "this_red_hard",
            "red",
            BlendIf {
                channel: BlendIfChannel::Red,
                this_layer: [155, 155, 255, 255],
                underlying: [0, 0, 255, 255],
                ..BlendIf::default()
            },
        ),
        overlay(
            "under_red_soft",
            "red",
            BlendIf {
                channel: BlendIfChannel::Red,
                this_layer: [0, 0, 255, 255],
                underlying: [16, 80, 176, 240],
                ..BlendIf::default()
            },
        ),
        overlay(
            "under_gray_hard",
            "gray",
            BlendIf {
                channel: BlendIfChannel::Gray,
                this_layer: [0, 0, 255, 255],
                underlying: [100, 100, 255, 255],
                ..BlendIf::default()
            },
        ),
    ]
}

/// Photoshop's slots as this file's `lfx2` writes them: the four single kinds
/// as objects and the five repeating kinds as `<kind>Multi` lists, the
/// Stroke list holding two.
const SLOTS: [&str; 10] = [
    "drop_shadow",
    "outer_glow",
    "stroke",
    "stroke",
    "gradient_overlay",
    "color_overlay",
    "satin",
    "inner_glow",
    "inner_shadow",
    "bevel_emboss",
];

#[test]
fn the_photoshop_blend_if_file_imports_with_every_authored_ramp() {
    let path = fixture("blend_if", "source.psd");
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    assert_eq!(report["import_precision"], Value::from("rgba8"));
    assert_eq!(report["layer_count"], Value::from(5));
    assert_eq!(
        strings(&report, "verified_losses"),
        Vec::<String>::new(),
        "{report}"
    );
    let observations = strings(&report, "observations");

    let doc = open_psd(&path, &report, LAYERS);
    assert_eq!(doc.layers.len(), 5);
    assert_eq!(doc.layers[0].name, "Base");
    assert!(doc.layers[0].style.is_none(), "the base carries no style");
    for wanted in overlays() {
        let layer = doc
            .layers
            .iter()
            .find(|l| l.name == wanted.name)
            .unwrap_or_else(|| panic!("layer {}", wanted.name));
        let style = layer
            .style
            .as_ref()
            .unwrap_or_else(|| panic!("{} has a style", wanted.name));
        assert_eq!(style.blend_if, Some(wanted.blend_if), "{}", wanted.name);
        assert_eq!(style.fill_opacity, 1.0, "{}", wanted.name);
        // Photoshop's own disabled slots, carried as written.
        let carried: Vec<String> = style
            .effects
            .iter()
            .map(|effect| effect.kind().type_name().to_string())
            .collect();
        assert_eq!(carried, SLOTS, "{}", wanted.name);
        assert!(
            style.effects.iter().all(|effect| !effect.enabled()),
            "{} carries only DISABLED Photoshop slots",
            wanted.name
        );
        assert!(
            observations.iter().any(|o| o.contains(&format!(
                "(“{}”) has Photoshop layer effects (drop_shadow, outer_glow, stroke, gradient_overlay, color_overlay, satin, inner_glow, inner_shadow, bevel_emboss and Blend If ranges on {}); layered import carries them as a Doctored layer style.",
                wanted.name, wanted.channel
            ))),
            "the {} carry sentence: {observations:?}",
            wanted.name
        );
    }
}

// ----------------------------------------------------------------- masks --

/// One authored mask: the layer's name, the mask's own size and its canvas
/// placement, exactly as this file yields them.
struct Masked {
    name: &'static str,
    dimensions: (u32, u32),
    offset: (i32, i32),
}

fn masked() -> Vec<Masked> {
    vec![
        Masked {
            name: "hard",
            dimensions: (32, 36),
            offset: (28, 24),
        },
        // One column narrower, and one column further right, than `hard` from
        // the SAME selection: Photoshop trims the stored rectangle to the
        // nonzero extent and this mask's ramp starts at black.
        Masked {
            name: "soft",
            dimensions: (31, 36),
            offset: (29, 24),
        },
        Masked {
            name: "wide",
            dimensions: (40, 64),
            offset: (0, 0),
        },
    ]
}

#[test]
fn the_photoshop_mask_file_imports_with_every_authored_placement() {
    let path = fixture("masks", "source.psd");
    let report = import_report(&path);
    assert_eq!(report["layered_available"], Value::Bool(true), "{report}");
    assert_eq!(report["import_precision"], Value::from("rgba8"));
    assert_eq!(report["layer_count"], Value::from(4));
    assert_eq!(
        strings(&report, "verified_losses"),
        Vec::<String>::new(),
        "{report}"
    );
    let observations = strings(&report, "observations");
    assert_eq!(
        observations
            .iter()
            .filter(|o| o.contains("The imported mask is independent (unlinked)"))
            .count(),
        3,
        "{observations:?}"
    );

    let doc = open_psd(&path, &report, LAYERS);
    assert_eq!(doc.layers.len(), 4);
    assert!(doc.layers[0].mask.is_none(), "the base has no mask");
    for wanted in masked() {
        let layer = doc
            .layers
            .iter()
            .find(|l| l.name == wanted.name)
            .unwrap_or_else(|| panic!("layer {}", wanted.name));
        // The overlay itself is the same 40 x 40 raster at (12, 8) three times.
        assert_eq!(layer.pixels.dimensions(), (40, 40), "{}", wanted.name);
        assert_eq!(layer.offset, (12, 8), "{}", wanted.name);
        let mask = layer
            .mask
            .as_ref()
            .unwrap_or_else(|| panic!("{} has a mask", wanted.name));
        assert_eq!(mask.dimensions(), wanted.dimensions, "{}", wanted.name);
        assert_eq!(
            layer.mask_properties.offset, wanted.offset,
            "{}",
            wanted.name
        );
        assert_eq!(layer.mask_properties.outside_coverage, 0, "{}", wanted.name);
        assert!(layer.mask_enabled, "{}", wanted.name);
        // PSD linkage is never inferred, on any of these records.
        assert!(!layer.mask_properties.linked, "{}", wanted.name);
        assert!(
            observations.iter().any(|o| o.contains(&format!(
                "(“{}”): ordinary raster mask samples, absolute placement",
                wanted.name
            ))),
            "the {} placement sentence: {observations:?}",
            wanted.name
        );
    }
}

#[test]
fn the_relative_twin_reads_as_photoshop_does() {
    let absolute_path = fixture("masks", "source.psd");
    let relative_path = fixture("masks", "source_relative.psd");
    let absolute_report = import_report(&absolute_path);
    let relative_report = import_report(&relative_path);
    assert_eq!(
        strings(&relative_report, "verified_losses"),
        Vec::<String>::new(),
        "{relative_report}"
    );
    let observations = strings(&relative_report, "observations");
    assert_eq!(
        observations
            .iter()
            .filter(|o| o.contains(
                "placement at its stored rectangle (the PSD position-relative flag, bit 0, is set and not applied, as Photoshop reads it)"
            ))
            .count(),
        3,
        "{observations:?}"
    );

    let absolute = open_psd(&absolute_path, &absolute_report, LAYERS);
    let relative = open_psd(&relative_path, &relative_report, LAYERS);
    assert_eq!(relative.layers.len(), absolute.layers.len());
    for wanted in masked() {
        let here = relative
            .layers
            .iter()
            .find(|l| l.name == wanted.name)
            .expect("the twin's layer");
        let there = absolute
            .layers
            .iter()
            .find(|l| l.name == wanted.name)
            .expect("the absolute layer");
        let mask = here.mask.as_ref().expect("the twin's mask");
        assert_eq!(mask.dimensions(), wanted.dimensions, "{}", wanted.name);
        // The twin's rectangle was written relative to its layer's top/left,
        // and it is read where it is stored: the absolute placement minus the
        // layer's own offset.
        assert_eq!(here.offset, there.offset, "{}", wanted.name);
        assert_eq!(
            here.mask_properties.offset,
            (
                wanted.offset.0 - here.offset.0,
                wanted.offset.1 - here.offset.1
            ),
            "{}",
            wanted.name
        );
        assert_eq!(
            mask.as_raw(),
            there.mask.as_ref().expect("the absolute mask").as_raw(),
            "{} samples",
            wanted.name
        );
    }

    // The masks moved, so the two openings are different pictures.
    let flat_absolute =
        unsafe { dr_doc_flattened(&*absolute, 0, std::ptr::null(), ptr::null_mut()) };
    let flat_relative =
        unsafe { dr_doc_flattened(&*relative, 0, std::ptr::null(), ptr::null_mut()) };
    assert!(!flat_absolute.is_null() && !flat_relative.is_null());
    assert_ne!(pixels(flat_absolute), pixels(flat_relative));
    unsafe {
        dr_image_free(flat_absolute);
        dr_image_free(flat_relative);
    }
}
