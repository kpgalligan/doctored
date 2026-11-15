//! The typed rendered-plane cache (`precision_style_cache`), black-box
//! through the FFI: `dr_precision_doc_render`, `dr_precision_doc_render_region`,
//! `dr_precision_doc_edit`, `dr_mixed_resources_report_with_raw_masks` and
//! `dr_mixed_resources_evict_caches`. Mounted in `suite_serial`: it evicts
//! process-wide memos.
//!
//! Load-independent facts only:
//! - STORE. After a typed render the report holds `style_cache_planes`.
//! - HIT. A second render (full or regional) polls strictly fewer job
//!   checkpoints than the cold render of the same document built afresh
//!   (the plane render polls inside every effect), its result equals the
//!   first, and the report's planes are unchanged in bytes and allocations.
//! - ORACLE. Every cached render equals the render of the same document
//!   built afresh (a new style `Arc`, new image identities), sample for
//!   sample.
//! - MISS, one case per key term: the layer's alpha, the mask's plane, its
//!   enabled flag, density, feather and offset relative to the layer, the
//!   global light under a global-light effect, the layer offset under a
//!   canvas-aligned gradient, and a styled adjustment's mask; each against
//!   the fresh oracle.
//! - EVICTION, KEY LIFETIME and the DRAIN of a replaced style; CANCEL is a
//!   pin (a render cancelled before it starts stores nothing, with or
//!   without the cache).
use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_precision::dr_precision_doc_render;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_regional::dr_precision_doc_render_region;
use doctored_core::ffi_resources::dr_mixed_resources_evict_caches;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};

use crate::float_fixture::float_doc;
use crate::resource_report::report;
use crate::serial;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (64, 56);
const LAYER: (u32, u32) = (48, 40);
const AT: (i32, i32) = (7, 5);
/// Drop shadow (global light), stroke and outer glow.
const STYLE: &str = r#"{"effects":[{"type":"drop_shadow"},{"type":"stroke","size":3},{"type":"outer_glow","size":6}]}"#;
/// [`STYLE`] in the modes a float document admits (its outer glow's default,
/// Screen, has no 32-bit form).
const FLOAT_STYLE: &str = r#"{"effects":[{"type":"drop_shadow"},{"type":"stroke","size":3},{"type":"outer_glow","size":6,"blend":"linear_dodge"}]}"#;
/// A radial gradient overlay aligned to the canvas, not the layer.
const ALIGNED: &str = r#"{"effects":[{"type":"gradient_overlay","gradient":{"style":"radial","align_with_layer":false}}]}"#;

// ------------------------------------------------------------- fixtures --

fn disc(x: u32, y: u32) -> [u16; 4] {
    let (dx, dy) = (x as f32 - 23.5, y as f32 - 19.5);
    let r = (dx * dx + dy * dy).sqrt();
    let a = ((19.0 - r) / 5.0).clamp(0.0, 1.0);
    [
        (x * 1300) as u16,
        (y * 1500) as u16,
        30000 + ((x + y) * 300) as u16,
        (a * 65535.0).round() as u16,
    ]
}

fn mask_samples() -> Vec<u16> {
    (0..LAYER.1)
        .flat_map(|y| (0..LAYER.0).map(move |x| (((x * 5 + y * 3) % 256) * 257) as u16))
        .collect()
}

fn background() -> Layer {
    let n = (CANVAS.0 * CANVAS.1) as usize;
    let image = PrecisionImage::from_rgba16(
        CANVAS.0,
        CANVAS.1,
        vec![[40000, 30000, 20000, 65535]; n],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    Layer::raster("B", image, (0, 0), 1.0, true).unwrap()
}

fn mask() -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(LAYER.0, LAYER.1, mask_samples(), None).unwrap(),
        MaskProperties::at(AT),
        true,
    )
    .unwrap()
}

fn style(json: &str) -> LayerStyle {
    LayerStyle::from_json(json).unwrap()
}

/// A 16-bit document: the background and a 48×40 soft disc at (7, 5)
/// carrying `json`, masked when asked. Built afresh on every call: new image
/// identities and a new style `Arc` (the ORACLE's document).
fn deep(json: &str, masked: bool) -> PrecisionDocument {
    let pixels: Vec<[u16; 4]> = (0..LAYER.1)
        .flat_map(|y| (0..LAYER.0).map(move |x| disc(x, y)))
        .collect();
    let image =
        PrecisionImage::from_rgba16(LAYER.0, LAYER.1, pixels, IccProfile::srgb(), None).unwrap();
    let mut top = Layer::raster("Top", image, AT, 1.0, true).unwrap();
    if masked {
        top = top.with_mask(Some(mask()));
    }
    let top = top.with_layer_style(Some(&style(json)), CANVAS).unwrap();
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![background(), top],
    )
    .unwrap()
}

/// A styled Invert ADJUSTMENT over the background, with a mask.
fn adjusted() -> PrecisionDocument {
    let description =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    let adjustment = Layer::adjustment("Invert", &description, 1.0, true, &IccProfile::srgb())
        .unwrap()
        .with_mask(Some(mask()))
        .with_layer_style(Some(&style(STYLE)), CANVAS)
        .unwrap();
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![background(), adjustment],
    )
    .unwrap()
}

/// A 32-bit float document of the same picture, styled through the edit.
fn float() -> PrecisionDocument {
    let n = (CANVAS.0 * CANVAS.1) as usize;
    let bg = vec![[0.6f32, 0.4, 2.5, 1.0]; n];
    let top: Vec<[f32; 4]> = (0..n as u32)
        .map(|i| {
            let (x, y) = (i % CANVAS.0, i / CANVAS.0);
            let inside = (7..55).contains(&x) && (5..45).contains(&y);
            if !inside {
                return [0.0; 4];
            }
            let p = disc(x - 7, y - 5);
            [3.0 * p[0] as f32 / 65535.0, 0.3, 1.2, p[3] as f32 / 65535.0]
        })
        .collect();
    let doc = float_doc(CANVAS.0, CANVAS.1, &IccProfile::srgb(), &[bg, top]);
    edited(
        &doc,
        json!({"op":"layer_style","version":1,"layer":1,
            "style":serde_json::from_str::<Value>(FLOAT_STYLE).unwrap()}),
    )
}

// ------------------------------------------------------------ the FFI --

fn take(e: *mut c_char) -> String {
    if e.is_null() {
        return String::new();
    }
    let s = unsafe { std::ffi::CStr::from_ptr(e) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(e) };
    s
}

fn edited(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    let error = take(e);
    assert!(!out.is_null(), "{request}: {error}");
    *unsafe { Box::from_raw(out) }
}

/// Every sample's bits, whatever the storage.
fn bits(image: &PrecisionImage) -> Vec<u32> {
    match image.storage() {
        PrecisionStorage::Rgba16Unorm => image
            .to_rgba16()
            .unwrap()
            .into_iter()
            .flatten()
            .map(u32::from)
            .collect(),
        PrecisionStorage::Rgba32Float => image
            .to_rgba32()
            .unwrap()
            .into_iter()
            .flatten()
            .map(f32::to_bits)
            .collect(),
    }
}

/// A full render's samples and the checkpoints its job polled.
fn render(doc: &PrecisionDocument) -> (Vec<u32>, u64) {
    let control = unsafe { dr_job_control_create() };
    let mut e = ptr::null_mut();
    let out = unsafe { dr_precision_doc_render(doc, LIMIT, 0, control, &mut e) };
    let error = take(e);
    let checkpoints = unsafe { &*control }.checkpoints();
    unsafe { dr_job_control_free(control) };
    assert!(!out.is_null(), "render: {error}");
    let image = unsafe { Box::from_raw(out) };
    (bits(&image), checkpoints)
}

/// The whole canvas through the regional compositor, and its checkpoints.
fn region(doc: &PrecisionDocument) -> (Vec<u32>, u64) {
    let control = unsafe { dr_job_control_create() };
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_render_region(doc, 0, 0, CANVAS.0, CANVAS.1, 1, LIMIT, 0, control, &mut e)
    };
    let error = take(e);
    let checkpoints = unsafe { &*control }.checkpoints();
    unsafe { dr_job_control_free(control) };
    assert!(!out.is_null(), "region: {error}");
    let image = unsafe { Box::from_raw(out) };
    (bits(&image), checkpoints)
}

/// `style_cache_planes`' unique bytes and allocation count over `docs`.
fn planes(docs: &[&PrecisionDocument]) -> (u64, u64) {
    let typed: Vec<*const PrecisionDocument> = docs.iter().map(|d| *d as *const _).collect();
    let value = report(&[], &[], &typed, &[], 0, 0).unwrap();
    let category = &value["categories"]["style_cache_planes"];
    (
        category["unique_bytes"].as_u64().unwrap_or(0),
        category["allocation_count"].as_u64().unwrap_or(0),
    )
}

fn evict(docs: &[&PrecisionDocument]) -> Value {
    let typed: Vec<*const PrecisionDocument> = docs.iter().map(|d| *d as *const _).collect();
    let mut e = ptr::null_mut();
    let out = unsafe {
        dr_mixed_resources_evict_caches(ptr::null(), 0, typed.as_ptr(), typed.len(), &mut e)
    };
    let error = take(e);
    assert!(!out.is_null(), "evict: {error}");
    serde_json::from_str(&take(out)).unwrap()
}

/// The checks every cached document takes: its first render stores planes;
/// a second full render and (when `regional`) a regional render each hit
/// (fewer checkpoints than a fresh document's cold render of the same kind),
/// equal the first and leave the planes as they were; and the first equals
/// the fresh oracle. A styled adjustment has no regional form.
fn hits(
    doc: &PrecisionDocument,
    fresh: &dyn Fn() -> PrecisionDocument,
    regional: bool,
    what: &str,
) {
    assert_eq!(
        planes(&[doc]),
        (0, 0),
        "{what}: a new document holds no planes"
    );
    let (first, cold) = render(doc);
    let stored = planes(&[doc]);
    assert!(
        stored.0 > 0 && stored.1 > 0,
        "{what}: the render stores its planes"
    );
    let (second, warm) = render(doc);
    assert_eq!(second, first, "{what}: the second render equals the first");
    assert!(
        warm < cold,
        "{what}: the second render hits ({warm} < {cold} checkpoints)"
    );
    assert_eq!(planes(&[doc]), stored, "{what}: a hit stores nothing");
    let (oracle, _) = render(&fresh());
    assert_eq!(
        first, oracle,
        "{what}: the cached render equals a fresh document's"
    );
    if !regional {
        return;
    }
    let (window, regional_warm) = region(doc);
    let (fresh_window, regional_cold) = region(&fresh());
    assert_eq!(
        window, fresh_window,
        "{what}: the regional render equals a fresh one"
    );
    assert!(
        regional_warm < regional_cold,
        "{what}: the regional render hits ({regional_warm} < {regional_cold})"
    );
    assert_eq!(
        planes(&[doc]),
        stored,
        "{what}: the regional hit stores nothing"
    );
}

/// `edit` misses: the edited document's render differs from `before`, equals
/// the fresh document with the same edit, and polls as many checkpoints as
/// that cold render.
fn misses(
    doc: &PrecisionDocument,
    fresh: &dyn Fn() -> PrecisionDocument,
    edit: &dyn Fn(&PrecisionDocument) -> PrecisionDocument,
    what: &str,
) {
    let (before, _) = render(doc);
    let changed = edit(doc);
    let (after, warm) = render(&changed);
    let (oracle, cold) = render(&edit(&fresh()));
    assert_ne!(after, before, "{what}: the edit changes the render");
    assert_eq!(
        after, oracle,
        "{what}: the render equals a fresh document's"
    );
    assert_eq!(warm, cold, "{what}: the edit misses and renders in full");
}

fn mask_properties(
    layer: usize,
    at: (i32, i32),
    density: f64,
    feather: f64,
    enabled: bool,
) -> Value {
    json!({"op":"mask_properties","layer":layer,"x":at.0,"y":at.1,"linked":false,
        "density":density,"feather":feather,"enabled":enabled})
}

/// One edit a MISS case applies.
type Edit = Box<dyn Fn(&PrecisionDocument) -> PrecisionDocument>;

// ------------------------------------------------------------------ tests --

#[test]
fn a_typed_render_stores_its_planes_and_the_next_render_reads_them() {
    let _serial = serial();
    hits(&deep(STYLE, false), &|| deep(STYLE, false), true, "16-bit");
    hits(
        &deep(STYLE, true),
        &|| deep(STYLE, true),
        true,
        "16-bit masked",
    );
    hits(&adjusted(), &adjusted, false, "styled adjustment");
    hits(&float(), &float, true, "float");
}

#[test]
fn an_unrelated_edit_keeps_the_planes() {
    let _serial = serial();
    let multiply =
        json!({"op":"layer_blend","version":1,"layer":0,"blend_mode":BlendMode::Multiply.to_c()});
    let doc = deep(STYLE, true);
    render(&doc);
    let stored = planes(&[&doc]);
    let changed = edited(&doc, multiply.clone());
    let (after, warm) = render(&changed);
    let (oracle, cold) = render(&edited(&deep(STYLE, true), multiply));
    assert_eq!(after, oracle);
    assert!(
        warm < cold,
        "the styled layer's planes are read ({warm} < {cold})"
    );
    assert_eq!(planes(&[&doc, &changed]), stored, "the planes are shared");
}

#[test]
fn every_key_term_misses_when_it_changes() {
    let _serial = serial();
    let masked = || deep(STYLE, true);
    let cases: Vec<(&str, Edit)> = vec![
        (
            "the layer's alpha",
            Box::new(|d| {
                edited(
                    d,
                    json!({"op":"scalar_edit","target":"alpha","index":1,"selection":false,
                        "operation":{"kind":"fill","value":20000}}),
                )
            }),
        ),
        (
            "the mask's plane",
            Box::new(|d| {
                edited(
                    d,
                    json!({"op":"scalar_edit","target":"mask","index":1,"selection":false,
                        "operation":{"kind":"fill","value":50000}}),
                )
            }),
        ),
        (
            "the mask disabled",
            Box::new(|d| edited(d, mask_properties(1, AT, 1.0, 0.0, false))),
        ),
        (
            "the mask's density",
            Box::new(|d| edited(d, mask_properties(1, AT, 0.5, 0.0, true))),
        ),
        (
            "the mask's feather",
            Box::new(|d| edited(d, mask_properties(1, AT, 1.0, 2.0, true))),
        ),
        (
            "the mask moved against the layer",
            Box::new(|d| edited(d, mask_properties(1, (AT.0 + 3, AT.1 + 2), 1.0, 0.0, true))),
        ),
        (
            "the global light",
            Box::new(|d| edited(d, json!({"op":"global_light","angle":45,"altitude":60}))),
        ),
    ];
    for (what, edit) in &cases {
        let doc = masked();
        render(&doc);
        misses(&doc, &masked, edit.as_ref(), what);
    }
    let aligned = || deep(ALIGNED, false);
    let doc = aligned();
    render(&doc);
    misses(
        &doc,
        &aligned,
        &|d| {
            edited(
                d,
                json!({"op":"align_set","layers":[1],"edge":"left","to_canvas":true}),
            )
        },
        "the layer moved under a canvas-aligned gradient",
    );
    let doc = adjusted();
    render(&doc);
    misses(
        &doc,
        &adjusted,
        &|d| {
            edited(
                d,
                json!({"op":"scalar_edit","target":"mask","index":1,"selection":false,
                    "operation":{"kind":"fill","value":50000}}),
            )
        },
        "a styled adjustment's mask",
    );
}

#[test]
fn eviction_drops_the_typed_planes_and_keeps_the_projection() {
    let _serial = serial();
    let doc = deep(STYLE, true);
    let image = doc.layer_source(1).unwrap().clone();
    let weak = image.weak_count();
    let (before, _) = render(&doc);
    assert_eq!(image.weak_count(), weak + 1, "the entry holds one weak key");
    let stored = planes(&[&doc]);
    assert!(stored.0 > 0);
    let evicted = evict(&[&doc]);
    assert!(
        evicted["removed_cache_bytes"].as_u64().unwrap() >= stored.0,
        "{evicted}"
    );
    assert_eq!(planes(&[&doc]), (0, 0));
    assert_eq!(image.weak_count(), weak, "eviction releases the key");
    let (after, cold) = render(&doc);
    assert_eq!(after, before, "the projection is unchanged");
    let (_, fresh) = render(&deep(STYLE, true));
    assert_eq!(cold, fresh, "the render after eviction is cold");
}

#[test]
fn a_cancelled_render_caches_nothing() {
    let _serial = serial();
    let doc = deep(STYLE, false);
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let mut e = ptr::null_mut();
    let out = unsafe { dr_precision_doc_render(&doc, LIMIT, 0, control, &mut e) };
    unsafe { dr_job_control_free(control) };
    assert!(out.is_null());
    assert_eq!(take(e), "operation cancelled");
    assert_eq!(planes(&[&doc]), (0, 0));
}

#[test]
fn a_style_edit_drains_the_replaced_style() {
    let _serial = serial();
    let doc = deep(STYLE, false);
    render(&doc);
    assert!(planes(&[&doc]).0 > 0);
    let restyled = edited(
        &doc,
        json!({"op":"layer_style","version":1,"layer":1,
            "style":{"effects":[{"type":"drop_shadow","distance":9}]}}),
    );
    assert_eq!(
        planes(&[&doc]),
        (0, 0),
        "the replaced style keeps no planes"
    );
    let (got, _) = render(&restyled);
    let (want, _) = render(&edited(
        &deep(STYLE, false),
        json!({"op":"layer_style","version":1,"layer":1,
            "style":{"effects":[{"type":"drop_shadow","distance":9}]}}),
    ));
    assert_eq!(got, want);
}
