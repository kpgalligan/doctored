//! Stack Modes on the byte document, through `dr_doc_stack_mode` and
//! `dr_doc_stack_mode_plan`.
//!
//! The reference is `common/stack_oracle.rs`, which looks every extent pixel
//! up member by member and rounds through quotient and remainder. Every mode
//! but those with an f64 step (`REAL_STEP_MODES`: the standard deviation,
//! skewness, kurtosis, entropy) is compared exactly; those within one code.
//! Structure is read back from the entries themselves, refusals by sentence.

use doctored_core::doc::{DrDocument, LayerKind, LayerRaster};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_stack::{dr_doc_stack_mode, dr_doc_stack_mode_plan};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::stack_stat::{plan_scratch, NO_PIXELS, OUTPUT_PLANE_COPIES_8};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::common::*;
use crate::stack_oracle::{self, Member, MODES, REAL_STEP_MODES};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CW: u32 = 12;
const CH: u32 = 10;

/// Offset and size of members 1…4; member 0 is the canvas-sized bottom layer.
/// Each reaches past the canvas on a different side.
const SHAPES: [((i32, i32), (u32, u32)); 4] = [
    ((-3, 2), (9, 6)),
    ((5, -4), (10, 9)),
    ((2, 3), (4, 12)),
    ((-1, -2), (7, 5)),
];

// ----------------------------------------------------------- the fixture --

fn mix(layer: u32, x: u32, y: u32) -> u32 {
    let mut v = layer
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(x.wrapping_mul(0x85EB_CA6B))
        .wrapping_add(y.wrapping_mul(0xC2B2_AE35));
    v ^= v >> 15;
    v = v.wrapping_mul(0x2C1B_3C6D);
    v ^ (v >> 13)
}

/// A varied pixel: alpha 0, well under half, one code under half (127), the
/// first code at least half (128), well over and opaque.
fn sample(layer: u32, x: u32, y: u32) -> [u8; 4] {
    let v = mix(layer, x, y);
    let alpha = match v % 7 {
        0 => 0,
        1 => 127,
        2 => 128,
        3 => 60,
        4 => 200,
        _ => 255,
    };
    // Member 1 carries a transparent hole.
    let alpha = if layer == 1 && (2..5).contains(&x) && (1..4).contains(&y) {
        0
    } else {
        alpha
    };
    [(v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8, alpha]
}

fn picture(layer: u32, w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| Rgba(sample(layer, x, y)))
}

/// `n` raster layers "L0"…, bottom first: L0 canvas-sized, the rest `SHAPES`.
fn stack(n: usize) -> DrDocument {
    let mut doc = DrDocument::from_pixels(picture(0, CW, CH));
    doc.layers[0].name = "L0".into();
    for i in 1..n {
        let ((ox, oy), (w, h)) = SHAPES[i - 1];
        doc = doc
            .adding_image_layer(i - 1, picture(i as u32, w, h), &format!("L{i}"))
            .unwrap()
            .with_layer_offset(i, ox, oy)
            .unwrap();
    }
    doc
}

fn request(layers: &[usize], above: usize, mode: &str) -> Value {
    json!({"op": "stack_mode", "layers": layers, "above": above, "mode": mode, "name": "Stat"})
}

fn run_with(
    doc: *const DrDocument,
    req: &Value,
    limit: u64,
    control: *const DrJobControl,
) -> (Option<DrDocument>, Option<String>) {
    let text = CString::new(req.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_stack_mode(doc, text.as_ptr(), limit, 0, control, &mut err) };
    take_doc(out, err)
}

fn run(doc: &DrDocument, req: &Value) -> DrDocument {
    match run_with(doc, req, LIMIT, ptr::null()) {
        (Some(out), None) => out,
        (_, err) => panic!("{req}: {err:?}"),
    }
}

fn refusal(doc: &DrDocument, req: &Value) -> String {
    let (out, err) = run_with(doc, req, LIMIT, ptr::null());
    assert!(out.is_none(), "{req} must be refused");
    err.unwrap_or_else(|| panic!("{req}: a refusal carries a sentence"))
}

fn plan(doc: *const DrDocument, req: &Value) -> Result<Value, String> {
    let text = CString::new(req.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let p = unsafe { dr_doc_stack_mode_plan(doc, text.as_ptr(), &mut err) };
    if p.is_null() {
        return Err(take_err_string(err));
    }
    assert!(err.is_null());
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Ok(serde_json::from_str(&s).unwrap())
}

/// The oracle's view of entry `index`, read back through the FFI.
fn member(doc: &DrDocument, index: usize) -> Member {
    let (width, height) = doc.layers[index].pixels.dimensions();
    Member {
        offset: doc.layers[index].offset,
        width,
        height,
        pixels: layer_bytes(doc, index)
            .chunks_exact(4)
            .map(|p| [p[0], p[1], p[2], p[3]].map(f64::from))
            .collect(),
    }
}

// ------------------------------------------------------------- the cases --

#[test]
fn every_mode_matches_the_oracle_for_two_to_five_members() {
    for n in 2..=5usize {
        let doc = stack(n);
        let layers: Vec<usize> = (0..n).collect();
        let members: Vec<Member> = layers.iter().map(|&i| member(&doc, i)).collect();
        let extent = stack_oracle::extent(&members);
        for m in &members {
            assert!(
                u64::from(extent.2) * u64::from(extent.3)
                    > u64::from(m.width) * u64::from(m.height),
                "{n} members: the extent is larger than every member"
            );
        }
        for mode in MODES {
            let out = run(&doc, &request(&layers, n - 1, mode));
            assert_eq!(out.layers.len(), n + 1);
            let made = &out.layers[n];
            assert_eq!(made.offset, (extent.0, extent.1), "{mode}, {n}: origin");
            assert_eq!(made.pixels.dimensions(), (extent.2, extent.3));
            let got = layer_bytes(&out, n);
            let want = stack_oracle::integer(mode, &members, 255);
            let slack = i64::from(REAL_STEP_MODES.contains(&mode));
            let mut covered = 0;
            for (i, (g, w)) in got.chunks_exact(4).zip(&want).enumerate() {
                assert_eq!(
                    u64::from(g[3]),
                    w[3],
                    "{mode}, {n} members, pixel {i}: alpha"
                );
                for c in 0..3 {
                    assert!(
                        (i64::from(g[c]) - w[c] as i64).abs() <= slack,
                        "{mode}, {n} members, pixel {i} channel {c}: {} against {}",
                        g[c],
                        w[c]
                    );
                }
                covered += usize::from(w[3] > 0);
            }
            assert!(covered > 60, "{mode}, {n}: only {covered} covered pixels");
            assert!(
                covered < want.len(),
                "{mode}, {n}: some pixel stays transparent"
            );
        }
    }
}

/// Everything about entry `index` that the edit must leave alone.
fn untouched(before: &DrDocument, b: usize, after: &DrDocument, a: usize, what: &str) {
    let (old, new) = (&before.layers[b], &after.layers[a]);
    assert!(
        LayerRaster::ptr_eq(&old.pixels, &new.pixels),
        "{what}: pixels"
    );
    assert_eq!(old.id, new.id, "{what}: identity");
    assert_eq!(old.name, new.name, "{what}: name");
    assert_eq!(old.offset, new.offset, "{what}: offset");
    assert_eq!(old.opacity, new.opacity, "{what}: opacity");
    assert_eq!(old.depth, new.depth, "{what}: level");
    assert_eq!(old.clipped, new.clipped, "{what}: clipped");
}

#[test]
fn the_result_lands_above_the_anchor_and_only_the_members_are_hidden() {
    let doc = stack(5).with_layer_opacity(2, 0.5).unwrap();
    let members = [member(&doc, 0), member(&doc, 2)];
    let extent = stack_oracle::extent(&members);

    // `above` the highest member.
    let out = run(&doc, &request(&[2, 0], 2, "median"));
    let names: Vec<&str> = out.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["L0", "L1", "L2", "Stat", "L3", "L4"]);
    let made = &out.layers[3];
    assert_eq!(made.kind, LayerKind::Raster);
    assert_eq!(made.depth, 0);
    assert_eq!(made.offset, (extent.0, extent.1));
    assert!(made.visible && !made.clipped && made.mask.is_none() && made.meta.is_none());
    assert_eq!(made.opacity, 1.0);
    for (b, a) in [(0, 0), (1, 1), (2, 2), (3, 4), (4, 5)] {
        untouched(&doc, b, &out, a, &format!("entry {b}"));
    }
    let visible: Vec<bool> = out.layers.iter().map(|l| l.visible).collect();
    assert_eq!(visible, [false, true, false, true, true, true]);

    // `above` a visible non-member two layers higher: directly above it.
    let out = run(&doc, &request(&[0, 2], 4, "median"));
    let names: Vec<&str> = out.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["L0", "L1", "L2", "L3", "L4", "Stat"]);
    for i in 0..5 {
        untouched(&doc, i, &out, i, &format!("entry {i}"));
    }
    let visible: Vec<bool> = out.layers.iter().map(|l| l.visible).collect();
    assert_eq!(visible, [false, true, false, true, true, true]);

    // A member that was already hidden stays hidden and is still read.
    let hidden = doc.with_layer_visible(0, false).unwrap();
    let out = run(&hidden, &request(&[0, 2], 2, "mean"));
    assert!(!out.layers[0].visible && !out.layers[2].visible);
    assert_eq!(
        layer_bytes(&out, 3),
        layer_bytes(&run(&doc, &request(&[0, 2], 2, "mean")), 3)
    );
}

#[test]
fn the_result_takes_the_anchors_level_inside_and_above_a_group() {
    let (doc, group, _, _) = stack(4).group_layers(&[1, 2], "G").unwrap();
    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["L0", "L1", "L2", "G", "L3"]);
    assert_eq!(group, 3);

    let out = run(&doc, &request(&[1, 2], 2, "maximum"));
    let names: Vec<&str> = out.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["L0", "L1", "L2", "Stat", "G", "L3"]);
    assert_eq!(out.layers[3].depth, 1, "inside the group");
    assert_eq!(out.layers[4].kind, LayerKind::Group);
    assert!(out.layers[4].visible, "the group is not a member");

    // The anchor need not be a raster: above the group's whole subtree.
    let out = run(&doc, &request(&[1, 2], 3, "maximum"));
    let names: Vec<&str> = out.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["L0", "L1", "L2", "G", "Stat", "L3"]);
    assert_eq!(out.layers[4].depth, 0);
    assert!(out.layers[3].visible);
}

#[test]
fn refusals_name_their_reason_in_plan_and_edit() {
    let doc = stack(3);
    let both = |doc: &DrDocument, req: &Value| {
        let message = refusal(doc, req);
        assert_eq!(
            plan(doc, req).unwrap_err(),
            message,
            "{req}: the plan agrees"
        );
        message
    };
    assert_eq!(
        both(&doc, &request(&[1], 1, "mean")),
        "stack mode requires at least two layers"
    );
    assert_eq!(
        both(&doc, &request(&[1, 1], 1, "mean")),
        "stack mode lists layer 1 more than once"
    );
    assert_eq!(
        both(&doc, &request(&[0, 9], 9, "mean")),
        "stack mode layer 9 is out of range"
    );
    assert_eq!(
        both(&doc, &request(&[0, 1], 9, "mean")),
        "stack mode cannot insert above layer 9: it is out of range"
    );
    assert_eq!(
        both(&doc, &request(&[0, 2], 1, "mean")),
        "stack mode cannot insert above layer 1: it is below layer 2, the highest stacked layer"
    );
    let (grouped, group, _, _) = doc.group_layers(&[0, 1], "G").unwrap();
    assert_eq!(
        both(&grouped, &request(&[0, group], group, "mean")),
        format!("Stack Mode requires raster layers; layer {group} is a group")
    );
    let mut adjusted = doc.clone();
    adjusted.layers[1].meta = Some(adjust_meta("invert", "{}"));
    assert_eq!(
        both(&adjusted, &request(&[0, 1], 1, "mean")),
        "Stack Mode requires raster layers; layer 1 is an adjustment layer"
    );
    let message = both(&doc, &request(&[0, 1], 1, "trimmed_mean"));
    assert!(
        message.starts_with("invalid stack mode request") && message.contains("unknown variant"),
        "{message}"
    );
    let extra = json!({"op": "stack_mode", "layers": [0, 1], "above": 1, "mode": "mean",
                       "name": "Stat", "sampler": "bicubic"});
    let message = both(&doc, &extra);
    assert!(
        message.starts_with("invalid stack mode request") && message.contains("unknown field"),
        "{message}"
    );
    let missing = json!({"op": "stack_mode", "layers": [0, 1], "mode": "mean", "name": "Stat"});
    let message = both(&doc, &missing);
    assert!(
        message.starts_with("invalid stack mode request") && message.contains("above"),
        "{message}"
    );
    let other = json!({"op": "layer_via", "layers": [0, 1], "above": 1, "mode": "mean",
                       "name": "Stat"});
    assert_eq!(
        both(&doc, &other),
        "invalid stack mode request: op must be stack_mode"
    );

    // Members with no pixel anywhere: the plan passes, the edit refuses.
    let clear = DrDocument::from_pixels(RgbaImage::new(CW, CH))
        .adding_image_layer(0, RgbaImage::new(5, 4), "Clear")
        .unwrap();
    assert!(plan(&clear, &request(&[0, 1], 1, "mean")).is_ok());
    assert_eq!(refusal(&clear, &request(&[0, 1], 1, "mean")), NO_PIXELS);
}

#[test]
fn the_plan_is_the_threshold() {
    let doc = stack(4);
    let layers = [0, 1, 2, 3];
    let members: Vec<Member> = layers.iter().map(|&i| member(&doc, i)).collect();
    let extent = stack_oracle::extent(&members);
    let (pixels, width) = (
        u64::from(extent.2) * u64::from(extent.3),
        u64::from(extent.2),
    );
    let req = request(&layers, 3, "median");
    let planned = plan(&doc, &req).unwrap();
    let scratch = planned["scratch_bytes"].as_u64().unwrap();
    // The formula, written out: the 8-bit factor's output planes, a row per
    // member plus one, 1 MiB, and the document's own structure term on top.
    let bare = OUTPUT_PLANE_COPIES_8 * pixels * 4 + (4 + 1) * width * 4 + 1024 * 1024;
    assert_eq!(plan_scratch(pixels, width, 4, 4, 0), Some(bare));
    let structure = scratch.checked_sub(bare).expect("at least the bare plan");
    assert!(structure < 64 * 1024, "structure term {structure}");
    assert_eq!(plan_scratch(pixels, width, 4, 4, structure), Some(scratch));
    assert_eq!(planned["work_units"], json!(pixels * 4));

    let total = planned["retained_bytes"].as_u64().unwrap() + scratch;
    let (out, err) = run_with(&doc, &req, total - 1, ptr::null());
    assert!(out.is_none());
    assert!(err.unwrap().contains("working limit"));
    let (out, err) = run_with(&doc, &req, total, ptr::null());
    assert_eq!(err, None);
    assert!(out.is_some());
}

#[test]
fn a_cancelled_control_answers_the_cancel_sentence() {
    let doc = stack(3);
    let req = request(&[0, 1, 2], 2, "median");
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let (out, err) = run_with(&doc, &req, LIMIT, control);
    unsafe { dr_job_control_free(control) };
    assert!(out.is_none());
    assert_eq!(err.as_deref(), Some("operation cancelled"));

    // An uncancelled control changes nothing about the result.
    let control = unsafe { dr_job_control_create() };
    let (out, err) = run_with(&doc, &req, LIMIT, control);
    unsafe { dr_job_control_free(control) };
    assert_eq!(err, None);
    assert_eq!(
        layer_bytes(&out.unwrap(), 3),
        layer_bytes(&run(&doc, &req), 3)
    );
}

#[test]
fn null_inputs_answer_an_error_on_both_exports() {
    let doc = stack(2);
    let text = CString::new(request(&[0, 1], 1, "mean").to_string()).unwrap();
    for (document, request) in [
        (ptr::null(), text.as_ptr()),
        (&doc as *const DrDocument, ptr::null()),
        (ptr::null(), ptr::null()),
    ] {
        let mut err: *mut c_char = ptr::null_mut();
        let p = unsafe { dr_doc_stack_mode_plan(document, request, &mut err) };
        assert!(p.is_null());
        assert!(!err.is_null(), "a NULL plan input carries an error");
        unsafe { dr_string_free(err) };

        let mut err: *mut c_char = ptr::null_mut();
        let out = unsafe { dr_doc_stack_mode(document, request, LIMIT, 0, ptr::null(), &mut err) };
        assert!(out.is_null());
        assert!(!err.is_null(), "a NULL edit input carries an error");
        unsafe { dr_string_free(err) };

        // A NULL err_out is tolerated too.
        assert!(unsafe { dr_doc_stack_mode_plan(document, request, ptr::null_mut()) }.is_null());
        assert!(unsafe {
            dr_doc_stack_mode(document, request, LIMIT, 0, ptr::null(), ptr::null_mut())
        }
        .is_null());
    }
}
