//! Phase 13A-1 (and 13A-2a's admissions): the 8-bit regional document render against the full-frame
//! projection. The oracle is INDEPENDENT of the regional code: the full
//! `dr_doc_flattened` projection is cropped and top-left-cell sampled here, in
//! plain index arithmetic, and every admitted stack must match it byte for
//! byte. Refusals are asserted by their exact sentence, construct and index.

use std::ffi::c_char;
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument, LayerKind, MaskKind};
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::ffi::*;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_regional::*;
use image::{Rgba, RgbaImage};
use serde_json::Value;

use crate::common;
use common::{adjust_meta, img_dims, img_pixels, plasma, styled, take_err_string};

use crate::region_plan;

const W: u32 = 29;
const H: u32 = 19;

// ------------------------------------------------------------- plumbing --

fn handle(doc: &DrDocument) -> *mut DrDocument {
    Box::into_raw(Box::new(doc.clone()))
}

/// The full projection through `dr_doc_flattened`.
fn full(doc: &DrDocument) -> Vec<u8> {
    let h = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_flattened(h, 0, std::ptr::null(), &mut err) };
    assert!(!img.is_null(), "flattened: {}", take_err_string(err));
    let out = img_pixels(img);
    unsafe {
        dr_image_free(img);
        dr_doc_free(h);
    }
    out
}

/// The independent oracle: pixel (x + i·d, y + j·d) of the full projection
/// for every output (i, j), row-major.
fn expected(
    full: &[u8],
    canvas_w: u32,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> (u32, u32, Vec<u8>) {
    let ow = w.div_ceil(d);
    let oh = h.div_ceil(d);
    let mut out = Vec::new();
    for j in 0..oh {
        for i in 0..ow {
            let cx = u64::from(x) + u64::from(i) * u64::from(d);
            let cy = u64::from(y) + u64::from(j) * u64::from(d);
            let at = ((cy * u64::from(canvas_w) + cx) * 4) as usize;
            out.extend_from_slice(&full[at..at + 4]);
        }
    }
    (ow, oh, out)
}

fn region(
    doc: &DrDocument,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
    budget: u64,
) -> Result<(u32, u32, Vec<u8>), String> {
    let hd = handle(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe { dr_doc_render_region(hd, x, y, w, h, d, budget, ptr::null(), &mut err) };
    unsafe { dr_doc_free(hd) };
    if img.is_null() {
        return Err(take_err_string(err));
    }
    assert!(err.is_null(), "err_out set on success");
    let (ow, oh) = img_dims(img);
    let px = img_pixels(img);
    unsafe { dr_image_free(img) };
    Ok((ow, oh, px))
}

fn plan(
    doc: &DrDocument,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
    budget: u64,
) -> Result<Value, String> {
    region_plan::byte(doc, (x, y, w, h), d, budget)
}

/// Every window and divisor below equals the oracle. The windows cover the
/// four edges, the corners, the interior, 1×1 and the whole canvas.
fn assert_regions_match(doc: &DrDocument, what: &str) {
    let reference = full(doc);
    let (cw, ch) = (doc.width, doc.height);
    let windows = [
        (0, 0, cw, ch),
        (3, 2, 11, 7),
        (0, 0, 5, 4),
        (cw - 6, 0, 6, 5),
        (0, ch - 3, 7, 3),
        (cw - 4, ch - 5, 4, 5),
        (0, 4, cw, 3),
        (7, 0, 2, ch),
        (cw - 1, ch - 1, 1, 1),
        (0, 0, 1, 1),
        (13, 9, 1, 1),
    ];
    for window in windows {
        for d in [1, 2, 3, 7, u32::MAX] {
            let got = region(doc, window, d, u64::MAX)
                .unwrap_or_else(|e| panic!("{what}: {window:?}/{d}: {e}"));
            assert_eq!(
                got,
                expected(&reference, cw, window, d),
                "{what}: window {window:?} divisor {d}"
            );
        }
    }
}

/// A plasma base with partial alpha in one band, so every mode sees both an
/// opaque and a translucent backdrop.
fn base() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(W, H, |x, y| {
        let c = plasma(x, y);
        let alpha = if y < 4 {
            (90 + (x * 20) % 120) as u8
        } else {
            255
        };
        Rgba([c[0], c[1], c[2], alpha])
    }))
}

fn top_pixels(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x * 37 + y * 11) as u8,
            (200 - x * 5) as u8,
            (y * 23 + 7) as u8,
            (40 + (x * 13 + y * 29) % 216) as u8,
        ])
    })
}

/// `doc` with a raster added on top at `offset`.
fn with_raster(
    doc: &DrDocument,
    pixels: RgbaImage,
    offset: (i32, i32),
    blend: BlendMode,
    opacity: f32,
) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, pixels, "Raster")
        .expect("add layer");
    let top = out.layers.len() - 1;
    let layer = &mut out.layers[top];
    layer.offset = offset;
    layer.blend = blend;
    layer.opacity = opacity;
    out
}

/// `doc` with an adjustment layer (`op`, `params`) added on top.
fn with_adjustment(doc: &DrDocument, op: &str, params: &str, blend: BlendMode) -> DrDocument {
    let mut out = doc
        .adding_image_layer(doc.layers.len() - 1, RgbaImage::new(1, 1), op)
        .expect("add layer");
    let top = out.layers.len() - 1;
    out.layers[top].meta = Some(adjust_meta(op, params));
    out.layers[top].blend = blend;
    out.layers[top].opacity = 0.8;
    out
}

fn refused(doc: &DrDocument) -> (String, String) {
    let window = (2, 1, 9, 6);
    let from_plan = plan(doc, window, 1, u64::MAX).expect_err("the plan must refuse");
    let from_render = region(doc, window, 1, u64::MAX).expect_err("the render must refuse");
    (from_plan, from_render)
}

// ---------------------------------------------------------------- tests --

#[test]
fn all_27_blend_modes_on_a_two_layer_stack_match_the_projection() {
    for code in 0..27 {
        let mode = BlendMode::from_c(code).expect("a raster blend mode");
        let doc = with_raster(&base(), top_pixels(17, 12), (5, 3), mode, 0.7);
        assert_regions_match(&doc, &format!("mode {code}"));
    }
}

#[test]
fn offsets_partly_and_wholly_off_canvas_match_the_projection() {
    for offset in [
        (-6, -4),
        (20, 12),
        (-3, 10),
        (W as i32 + 5, 0),
        (0, H as i32 + 1),
        (-40, -40),
        (i32::MIN, i32::MAX),
    ] {
        let doc = with_raster(&base(), top_pixels(17, 12), offset, BlendMode::Screen, 0.9);
        assert_regions_match(&doc, &format!("offset {offset:?}"));
    }
}

#[test]
fn invisible_and_zero_opacity_entries_are_skipped_before_pixel_checks() {
    let doc = with_raster(
        &base(),
        top_pixels(17, 12),
        (2, 2),
        BlendMode::Multiply,
        0.6,
    );
    // A hidden styled and masked layer, and a zero-opacity one: both skipped,
    // so their style and mask never reach the pixel checks.
    let hidden = with_raster(&doc, top_pixels(9, 9), (1, 1), BlendMode::Normal, 1.0);
    let hidden = styled(
        &hidden,
        2,
        "{\"effects\":[{\"type\":\"stroke\",\"size\":2,\"color\":\"#FF0000\"}]}",
    );
    let mut hidden = hidden.add_mask(2, MaskKind::HideAll).expect("mask");
    hidden.layers[2].visible = false;
    let mut zero = with_raster(&hidden, top_pixels(9, 9), (4, 4), BlendMode::Normal, 1.0);
    zero.layers[3].opacity = 0.0;
    let zero = styled(
        &zero,
        3,
        "{\"effects\":[{\"type\":\"stroke\",\"size\":2,\"color\":\"#00FF00\"}]}",
    );
    assert_regions_match(&zero, "hidden + zero-opacity");
    let listed = plan(&zero, (0, 0, W, H), 1, u64::MAX).unwrap();
    let indices: Vec<_> = listed["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["index"].as_u64().unwrap())
        .collect();
    assert_eq!(indices, [0, 1]);
}

#[test]
fn pointwise_adjustments_match_the_projection() {
    let doc = with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Overlay, 0.8);
    for (op, params) in [
        ("invert", "{}"),
        ("levels", "{\"black\":0.1,\"white\":0.9,\"gamma\":1.8}"),
        ("hue_saturation", "{\"hue\":40,\"saturation\":0.3}"),
        (
            "exposure",
            "{\"evaluation\":\"document_v2\",\"exposure\":0.7,\"offset\":0.017,\"gamma\":1.2}",
        ),
        ("posterize", "{\"levels\":4}"),
        // Zero amounts: the guide is never read, so this Shadows/Highlights
        // is pointwise and admitted.
        (
            "shadows_highlights",
            "{\"shadows\":{\"amount\":0},\"highlights\":{\"amount\":0},\"midtone_contrast\":0.3}",
        ),
    ] {
        for mode in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Dissolve] {
            let adjusted = with_adjustment(&doc, op, params, mode);
            assert_regions_match(&adjusted, &format!("{op} {mode:?}"));
            let listed = plan(&adjusted, (0, 0, 4, 4), 1, u64::MAX).unwrap();
            let top = &listed["layers"][2];
            assert_eq!(top["kind"], "adjustment");
            assert_eq!(top["operator"], op);
            assert_eq!(top["neighborhood"]["kind"], "pointwise");
        }
    }
}

#[test]
fn a_linear_light_document_matches_its_projection() {
    let doc = with_raster(
        &base(),
        top_pixels(17, 12),
        (-2, 4),
        BlendMode::Multiply,
        0.75,
    );
    let doc = with_adjustment(&doc, "levels", "{\"gamma\":0.7}", BlendMode::Normal);
    let linear = doc
        .with_evaluation(Evaluation::LinearLight)
        .unwrap()
        .expect("a policy change");
    assert_ne!(
        full(&linear),
        full(&doc),
        "the policy changes the projection"
    );
    assert_regions_match(&linear, "linear light");
    // The policy's own refusal is raised by both calls, verbatim: a profile
    // replaced without its rebind leaves the transfer stale.
    let mut refused_mode = linear.clone();
    refused_mode.profile = doctored_core::icc::IccProfile::display_p3();
    let reference = {
        let h = handle(&refused_mode);
        let mut err: *mut c_char = ptr::null_mut();
        assert!(unsafe { dr_doc_flattened(h, 0, std::ptr::null(), &mut err) }.is_null());
        unsafe { dr_doc_free(h) };
        take_err_string(err)
    };
    let (from_plan, from_render) = refused(&refused_mode);
    assert_eq!(from_plan, reference);
    assert_eq!(from_render, reference);
}

#[test]
fn request_edges_empty_and_overflow_refuse_with_phase_4_sentences() {
    let doc = with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Normal, 1.0);
    for (window, d, message) in [
        (
            (0, 0, 0, 1),
            1,
            "render region must have nonzero dimensions",
        ),
        (
            (0, 0, 1, 0),
            1,
            "render region must have nonzero dimensions",
        ),
        ((W, 0, 1, 1), 1, "render region is outside the source image"),
        (
            (0, H - 1, 1, 2),
            1,
            "render region is outside the source image",
        ),
        (
            (u32::MAX, 0, 2, 1),
            1,
            "render region is outside the source image",
        ),
        (
            (0, u32::MAX, 1, u32::MAX),
            1,
            "render region is outside the source image",
        ),
        ((0, 0, 1, 1), 0, "render preview divisor must be at least 1"),
    ] {
        assert_eq!(plan(&doc, window, d, u64::MAX).unwrap_err(), message);
        assert_eq!(region(&doc, window, d, u64::MAX).unwrap_err(), message);
    }
}

#[test]
fn the_output_budget_boundary_is_exact() {
    let doc = with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Normal, 1.0);
    // 11×7 at divisor 2 is 6×4 = 24 pixels = 96 bytes.
    let window = (3, 2, 11, 7);
    let got = region(&doc, window, 2, 96).expect("the exact budget admits");
    assert_eq!((got.0, got.1), (6, 4));
    let message = "render output requires 96 bytes; budget is 95 bytes";
    assert_eq!(plan(&doc, window, 2, 95).unwrap_err(), message);
    assert_eq!(region(&doc, window, 2, 95).unwrap_err(), message);
}

#[test]
fn every_unsupported_construct_refuses_by_name_in_both_calls() {
    let two = with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Normal, 1.0);

    // Since 13A-3a a group is admitted and equals the reference: plain, with
    // a hidden child, nested, and empty (hidden or not).
    let (grouped, ..) = two.group_layers(&[1], "Group").expect("group");
    assert_regions_match(&grouped, "a group is admitted");
    let mut hidden_child = grouped.clone();
    hidden_child.layers[1].visible = false;
    assert_regions_match(&hidden_child, "a group with a hidden child");
    let (nested, ..) = grouped.group_layers(&[2], "Outer").expect("nest");
    assert_regions_match(&nested, "a nested group");
    let mut empty = two.clone();
    let mut entry = two.layers[1].clone();
    entry.kind = LayerKind::Group;
    entry.name = "Empty".into();
    entry.visible = false;
    empty.layers.push(entry);
    assert_regions_match(&empty, "a hidden empty group");
    empty.layers[2].visible = true;
    assert_regions_match(&empty, "a visible empty group");

    // Since 13A-2a a clip member is admitted and equals the reference.
    let mut clipped = with_raster(&two, top_pixels(5, 5), (1, 1), BlendMode::Normal, 1.0);
    clipped.layers[2].clipped = true;
    clipped.layers[2].name = "Clip".into();
    assert_regions_match(&clipped, "a clip member is admitted");

    let style = styled(
        &two,
        1,
        "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}]}",
    );
    // Since 13A-3b-i a style with an enabled effect is admitted too.
    assert_regions_match(&style, "a drop shadow is admitted");

    // Since 13A-2a an enabled mask is admitted and equals the reference.
    let masked = two.add_mask(1, MaskKind::RevealAll).expect("mask");
    assert_regions_match(&masked, "an enabled mask is admitted");
    let mut disabled = masked.clone();
    disabled.layers[1].mask_enabled = false;
    assert_regions_match(&disabled, "a disabled mask is admitted");

    // Since 13A-4a a guided Shadows/Highlights at the root is admitted
    // (staged) and equals the reference.
    let guided = with_adjustment(
        &two,
        "shadows_highlights",
        "{\"shadows\":{\"amount\":0.6,\"tone\":0.4},\"radius\":12}",
        BlendMode::Normal,
    );
    assert_regions_match(&guided, "a guided Shadows/Highlights at the root");
}

#[test]
fn a_hidden_clipped_member_and_an_unclipped_hidden_one_are_admitted() {
    // Bottom-first [raster, hue_saturation, raster clipped=true visible=false].
    let doc = with_adjustment(
        &base(),
        "hue_saturation",
        "{\"hue\":-60,\"saturation\":0.2}",
        BlendMode::Normal,
    );
    let mut doc = with_raster(&doc, top_pixels(17, 12), (5, 3), BlendMode::Normal, 1.0);
    doc.layers[2].visible = false;
    let mut clipped = doc.clone();
    clipped.layers[2].clipped = true;
    // The reference really does take the private-buffer path here: the
    // adjustment base contributes nothing, so the projection is the base's —
    // and since 13A-2a the regional render takes it too.
    assert_eq!(full(&clipped), full(&base()));
    assert_regions_match(&clipped, "hidden, clipped");
    assert_regions_match(&doc, "hidden, unclipped");
}

/// 13A-5c-3: the description parse the byte plan reserves under every
/// profile — the largest non-group `meta` length × 256.
fn description_parse(doc: &DrDocument) -> u64 {
    doc.layers
        .iter()
        .filter(|l| l.kind != LayerKind::Group)
        .filter_map(|l| l.meta.as_ref().map(|m| m.len() as u64))
        .max()
        .unwrap_or(0)
        * 256
}

#[test]
fn the_plan_declares_its_contract_fields() {
    let doc = with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Screen, 0.5);
    let doc = with_adjustment(&doc, "invert", "{}", BlendMode::Normal);
    let p = plan(&doc, (3, 2, 11, 7), 3, 10_000).unwrap();
    assert_eq!(p["contract_version"], 1);
    assert_eq!(p["operation"], "composite");
    assert_eq!(p["depth"], "rgba8");
    assert_eq!(p["reference"], "dr_doc_flattened");
    let window = serde_json::json!({"x":3,"y":2,"width":11,"height":7});
    assert_eq!(p["requested_region"], window);
    assert_eq!(p["required_input_region"], window);
    assert_eq!(
        p["neighborhood"],
        serde_json::json!({"kind":"pointwise","halo_pixels":0})
    );
    assert_eq!(p["global_analysis"], false);
    assert_eq!(p["full_frame_fallback"], false);
    assert_eq!(p["preview_divisor"], 3);
    assert_eq!(p["preview_only"], true);
    assert_eq!(p["sampling"], "nearest_top_left_cell");
    // ceil(11/3) × ceil(7/3) = 4 × 3.
    assert_eq!(
        p["output"],
        serde_json::json!({"width":4,"height":3,"bytes":48})
    );
    assert_eq!(
        p["scratch_heap_bytes"],
        12 * 16 + 48 + description_parse(&doc)
    );
    assert_eq!(p["work_units"], 12 * 4);
    assert_eq!(p["cancellation_checkpoint_pixels"], 4096);
    assert_eq!(p["max_output_bytes"], 10_000);
    let layers = p["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[0]["index"], 0);
    assert_eq!(layers[0]["name"], "Background");
    assert_eq!(layers[0]["kind"], "raster");
    assert_eq!(layers[0]["operator"], "composite");
    assert_eq!(layers[2]["kind"], "adjustment");
    assert_eq!(layers[2]["operator"], "invert");
    for layer in layers {
        assert_eq!(layer["global_analysis"], false);
        assert_eq!(layer["neighborhood"]["halo_pixels"], 0);
        assert_eq!(layer["clip_base"], Value::Null);
        assert_eq!(layer["mask"], false);
        assert_eq!(layer["blend_if"], false);
    }
    assert_eq!(
        plan(&doc, (0, 0, 4, 4), 1, u64::MAX).unwrap()["preview_only"],
        false
    );
}

#[test]
fn a_pre_cancelled_control_returns_no_output_and_holds_no_scratch() {
    let doc = with_raster(&base(), top_pixels(17, 12), (5, 3), BlendMode::Normal, 1.0);
    let h = handle(&doc);
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_render_region(h, 0, 0, W, H, 1, u64::MAX, control, &mut err) };
    assert!(out.is_null());
    assert_eq!(take_err_string(err), "operation cancelled");
    assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);

    // An uncancelled control completes, reports stage 5 at 1.0 and releases
    // its reservation.
    let fresh = unsafe { dr_job_control_create() };
    let out = unsafe { dr_doc_render_region(h, 0, 0, W, H, 1, u64::MAX, fresh, ptr::null_mut()) };
    assert!(!out.is_null());
    assert_eq!(unsafe { dr_job_control_progress(fresh) }, 1.0);
    assert_eq!(unsafe { dr_job_control_stage(fresh) }, 5);
    assert_eq!(unsafe { dr_job_control_scratch_bytes(fresh) }, 0);
    unsafe {
        dr_image_free(out);
        dr_job_control_free(control);
        dr_job_control_free(fresh);
        dr_doc_free(h);
    }
}

/// A cancel inside the running loop returns "operation cancelled", no image
/// and no scratch. It races no poller: the loop's first report's
/// checkpoint count comes from a fresh twin's own report log and the worker
/// is cancelled PARKED at the next checkpoint (`cancelled_past_each`).
#[test]
fn a_concurrently_cancelled_render_returns_operation_cancelled() {
    let (w, h) = (3000, 2000);
    let build = || {
        let mut doc = DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
            Rgba([x as u8, y as u8, (x ^ y) as u8, 255])
        }));
        for i in 0..6 {
            doc = with_raster(
                &doc,
                RgbaImage::from_pixel(w, h, Rgba([i * 40, 90, 200 - i * 20, 180])),
                (i as i32, 0),
                BlendMode::SoftLight,
                0.9,
            );
        }
        doc
    };
    let render_at = move |address: usize| {
        move |token: usize| {
            let mut err: *mut c_char = ptr::null_mut();
            let out = unsafe {
                dr_doc_render_region(
                    address as *const DrDocument,
                    0,
                    0,
                    w,
                    h,
                    1,
                    u64::MAX,
                    token as *const _,
                    &mut err,
                )
            };
            let ok = !out.is_null();
            if ok {
                unsafe { dr_image_free(out) };
            }
            (ok, take_err_string(err))
        }
    };
    let doc = build();
    // 13A-5b-iii-b-2: the fill reports, so the loop has started once progress
    // passes the plan's loop start.
    let start = common::probe::loop_start(&plan(&doc, (0, 0, w, h), 1, u64::MAX).unwrap());
    let hd = handle(&doc);
    let twin = |token: usize| {
        let twin = handle(&build());
        let out = render_at(twin as usize)(token);
        unsafe { dr_doc_free(twin) };
        out
    };
    let parked = common::probe::cancelled_past_each(&[start + 1e-6], twin, render_at(hd as usize));
    eprintln!(
        "cancel inside the loop: {}",
        common::probe::parked_line(&parked[0])
    );
    unsafe { dr_doc_free(hd) };
}

#[test]
fn null_inputs_return_null_without_an_error() {
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe { dr_doc_render_region(ptr::null(), 0, 0, 1, 1, 1, 4, ptr::null(), &mut err) }
            .is_null()
    );
    assert!(err.is_null());
}
