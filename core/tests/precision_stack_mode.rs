//! Stack Modes at 16 bits and 32-bit float, through
//! `dr_precision_doc_edit` / `dr_precision_doc_edit_plan`'s `stack_mode`
//! request.
//!
//! The reference is `common/stack_oracle.rs`. At 16 bits every mode but
//! those with an f64 step (`REAL_STEP_MODES`) is compared exactly, those
//! within one code. At
//! float every component is within 1e-6 relative of the f64 reference, and a
//! minimum, a maximum and a median over an odd number of members are a
//! member's own value BIT FOR BIT, values above white and below zero
//! included. Structure is read back from the records, refusals by sentence.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::float_document::float_operators_json;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_capability::capability;
use doctored_core::precision_clipboard::{ClipboardPayload, PasteRequest};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::stack_stat::{
    output_plane_copies, plan_scratch, NO_PIXELS, OUTPUT_PLANE_COPIES_16, OUTPUT_PLANE_COPIES_32,
    OUTPUT_PLANE_COPIES_8,
};
use serde_json::{json, Value};

use crate::float_fixture::{drcb, float_doc};
use crate::stack_oracle::{self, Member, MODES, REAL_STEP_MODES};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CW: u32 = 12;
const CH: u32 = 10;

/// Offset and size of the five members; each reaches past the canvas on a
/// different side, and none covers their union.
const SHAPES: [((i32, i32), (u32, u32)); 5] = [
    ((1, 1), (8, 7)),
    ((-3, 2), (9, 6)),
    ((5, -4), (10, 9)),
    ((2, 3), (4, 12)),
    ((-1, -2), (7, 5)),
];

// ------------------------------------------------------------- plumbing --

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    Some(text)
}

fn edit_with(
    document: &PrecisionDocument,
    request: &Value,
    limit: u64,
    control: *const DrJobControl,
) -> Result<Option<Box<PrecisionDocument>>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            document,
            text.as_ptr(),
            ptr::null(),
            0,
            limit,
            0,
            control,
            &mut error,
        )
    };
    if let Some(error) = take_error(error) {
        assert!(result.is_null(), "a refusal leaves no document");
        return Err(error);
    }
    Ok((!result.is_null()).then(|| unsafe { Box::from_raw(result) }))
}

fn must(document: &PrecisionDocument, request: &Value) -> Box<PrecisionDocument> {
    match edit_with(document, request, LIMIT, ptr::null()) {
        Ok(Some(out)) => out,
        Ok(None) => panic!("{request} reported no change"),
        Err(message) => panic!("{request}: {message}"),
    }
}

fn plan_at(document: &PrecisionDocument, request: &Value, limit: u64) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(document, text.as_ptr(), limit, 0, &mut error) };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    assert!(!out.is_null(), "a plan with no error");
    let value = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(out) };
    Ok(serde_json::from_str(&value).expect("plan JSON"))
}

/// The refusal's sentence, which the plan gives too.
fn refusal(document: &PrecisionDocument, request: &Value) -> String {
    let message = match edit_with(document, request, LIMIT, ptr::null()) {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused"),
    };
    assert_eq!(
        plan_at(document, request, LIMIT).unwrap_err(),
        message,
        "{request}: the plan agrees"
    );
    message
}

fn request(layers: &[usize], above: usize, mode: &str) -> Value {
    json!({"op": "stack_mode", "layers": layers, "above": above, "mode": mode, "name": "Stat"})
}

fn names(document: &PrecisionDocument) -> Vec<String> {
    (0..document.layer_count())
        .map(|i| document.layer(i).unwrap().name().to_string())
        .collect()
}

fn visibility(document: &PrecisionDocument) -> Vec<bool> {
    (0..document.layer_count())
        .map(|i| document.layer(i).unwrap().visible())
        .collect()
}

/// Whether record `a` of `before` and record `b` of `after` hold the same
/// image by identity (no pixel was rewritten).
fn same_pixels(before: &PrecisionDocument, a: usize, after: &PrecisionDocument, b: usize) -> bool {
    PrecisionImage::ptr_eq(
        before.layer(a).unwrap().source().unwrap(),
        after.layer(b).unwrap().source().unwrap(),
    )
}

fn mix(layer: u32, i: u32) -> u32 {
    let mut v = layer
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(i.wrapping_mul(0x85EB_CA6B));
    v ^= v >> 15;
    v = v.wrapping_mul(0x2C1B_3C6D);
    v ^ (v >> 13)
}

/// The oracle's view of record `index`, read back from the document.
fn member(document: &PrecisionDocument, index: usize) -> Member {
    let record = document.layer(index).unwrap();
    let image = record.source().expect("a raster record");
    let (width, height) = image.dimensions();
    let pixels = match image.to_rgba16() {
        Ok(codes) => codes.iter().map(|p| p.map(f64::from)).collect(),
        Err(_) => image
            .to_rgba32()
            .unwrap()
            .iter()
            .map(|p| p.map(f64::from))
            .collect(),
    };
    Member {
        offset: record.offset(),
        width,
        height,
        pixels,
    }
}

fn assert_extent_exceeds(members: &[Member], extent: (i32, i32, u32, u32)) {
    for m in members {
        assert!(
            u64::from(extent.2) * u64::from(extent.3) > u64::from(m.width) * u64::from(m.height),
            "the extent is larger than every member"
        );
    }
}

// ---------------------------------------------------------------- 16 bits --

/// A varied RGBA16 pixel: alpha 0, well under half, one code under half
/// (32767), the first code at least half (32768), well over and opaque.
fn code(layer: u32, i: u32) -> [u16; 4] {
    let v = mix(layer, i);
    let alpha = match v % 7 {
        0 => 0,
        1 => 32767,
        2 => 32768,
        3 => 15000,
        4 => 50000,
        _ => 65535,
    };
    [
        (v >> 4) as u16,
        (v >> 12) as u16,
        (v >> 16) as u16 ^ 0x5A5A,
        alpha,
    ]
}

fn image16(layer: u32, w: u32, h: u32) -> PrecisionImage {
    let pixels: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            // Member 1 carries a transparent hole.
            if layer == 1 && (2..5).contains(&(i % w)) && (1..4).contains(&(i / w)) {
                [0; 4]
            } else {
                code(layer, i)
            }
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
}

fn document16(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        CW,
        CH,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn raster16(i: usize) -> Layer {
    let ((ox, oy), (w, h)) = SHAPES[i];
    Layer::raster(
        &format!("L{i}"),
        image16(i as u32, w, h),
        (ox, oy),
        1.0,
        true,
    )
    .unwrap()
}

/// `n` raster records "L0"…, bottom first, in `SHAPES`.
fn stack16(n: usize) -> PrecisionDocument {
    document16((0..n).map(raster16).collect())
}

#[test]
fn every_mode_matches_the_oracle_at_16_bits() {
    for n in 2..=5usize {
        let doc = stack16(n);
        let layers: Vec<usize> = (0..n).collect();
        let members: Vec<Member> = layers.iter().map(|&i| member(&doc, i)).collect();
        let extent = stack_oracle::extent(&members);
        assert_extent_exceeds(&members, extent);
        for mode in MODES {
            let out = must(&doc, &request(&layers, n - 1, mode));
            assert_eq!(out.layer_count(), n + 1);
            let made = out.layer(n).unwrap();
            assert_eq!(made.offset(), (extent.0, extent.1), "{mode}, {n}: origin");
            let image = made.source().unwrap();
            assert_eq!(image.dimensions(), (extent.2, extent.3));
            let got = image.to_rgba16().unwrap();
            let want = stack_oracle::integer(mode, &members, 65535);
            let slack = i64::from(REAL_STEP_MODES.contains(&mode));
            let mut covered = 0;
            for (i, (g, w)) in got.iter().zip(&want).enumerate() {
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
            assert!(covered > 40, "{mode}, {n}: only {covered} covered pixels");
            assert!(
                covered < want.len(),
                "{mode}, {n}: some pixel stays transparent"
            );
        }
    }
}

#[test]
fn the_record_lands_above_the_anchor_and_only_the_members_are_hidden() {
    let doc = stack16(5);
    let members = [member(&doc, 0), member(&doc, 2)];
    let extent = stack_oracle::extent(&members);

    // `above` the highest member.
    let out = must(&doc, &request(&[2, 0], 2, "median"));
    assert_eq!(names(&out), ["L0", "L1", "L2", "Stat", "L3", "L4"]);
    let made = out.layer(3).unwrap();
    assert_eq!(made.offset(), (extent.0, extent.1));
    assert!(made.visible() && !made.clipped());
    assert_eq!(made.opacity(), 1.0);
    assert_eq!(visibility(&out), [false, true, false, true, true, true]);
    for (b, a) in [(0, 0), (1, 1), (2, 2), (3, 4), (4, 5)] {
        let (old, new) = (doc.layer(b).unwrap(), out.layer(a).unwrap());
        assert!(same_pixels(&doc, b, &out, a), "record {b}: pixels");
        assert_eq!(old.offset(), new.offset(), "record {b}: offset");
        assert_eq!(old.opacity(), new.opacity(), "record {b}: opacity");
    }

    // `above` a visible non-member two layers higher: directly above it.
    let out = must(&doc, &request(&[0, 2], 4, "median"));
    assert_eq!(names(&out), ["L0", "L1", "L2", "L3", "L4", "Stat"]);
    assert_eq!(visibility(&out), [false, true, false, true, true, true]);
    assert!(same_pixels(&doc, 4, &out, 4), "the anchor is untouched");
}

#[test]
fn the_record_takes_the_anchors_level_inside_and_above_a_group() {
    let doc = document16(vec![
        raster16(0),
        Layer::isolated_normal(
            "G",
            vec![raster16(1), raster16(2).with_clipped(true)],
            1.0,
            true,
        )
        .unwrap(),
        raster16(3),
    ]);
    // Postorder: L0 0, L1 1, L2 2, G 3, L3 4.
    assert_eq!(names(&doc), ["L0", "L1", "L2", "G", "L3"]);
    let out = must(&doc, &request(&[1, 2], 2, "maximum"));
    assert_eq!(names(&out), ["L0", "L1", "L2", "Stat", "G", "L3"]);
    let children: Vec<&str> = out.root_layers()[1]
        .children()
        .unwrap()
        .iter()
        .map(|l| l.name())
        .collect();
    assert_eq!(children, ["L1", "L2", "Stat"], "inside the group");
    assert!(
        !out.layer(3).unwrap().clipped(),
        "unclipped above a clipped member"
    );
    assert!(out.layer(4).unwrap().visible(), "the group is not a member");

    // The anchor need not be a raster: above the group's whole subtree.
    let out = must(&doc, &request(&[1, 2], 3, "maximum"));
    assert_eq!(names(&out), ["L0", "L1", "L2", "G", "Stat", "L3"]);
    let roots: Vec<&str> = out.root_layers().iter().map(|l| l.name()).collect();
    assert_eq!(roots, ["L0", "G", "Stat", "L3"]);
    assert_eq!(visibility(&out), [true, false, false, true, true, true]);
}

#[test]
fn refusals_name_their_reason_in_plan_and_edit() {
    let doc = stack16(3);
    assert_eq!(
        refusal(&doc, &request(&[1], 1, "mean")),
        "stack mode requires at least two layers"
    );
    assert_eq!(
        refusal(&doc, &request(&[1, 1], 1, "mean")),
        "stack mode lists layer 1 more than once"
    );
    assert_eq!(
        refusal(&doc, &request(&[0, 9], 9, "mean")),
        "stack mode layer 9 is out of range"
    );
    assert_eq!(
        refusal(&doc, &request(&[0, 1], 9, "mean")),
        "stack mode cannot insert above layer 9: it is out of range"
    );
    assert_eq!(
        refusal(&doc, &request(&[0, 2], 1, "mean")),
        "stack mode cannot insert above layer 1: it is below layer 2, the highest stacked layer"
    );
    let grouped = document16(vec![
        raster16(0),
        Layer::isolated_normal("G", vec![raster16(1)], 1.0, true).unwrap(),
    ]);
    assert_eq!(
        refusal(&grouped, &request(&[0, 2], 2, "mean")),
        "Stack Mode requires raster layers; layer 2 is a group"
    );
    let description =
        json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string();
    let adjusted = doc
        .add_adjustment_at(
            "Invert",
            &description,
            1.0,
            2,
            InsertPosition::Above,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        refusal(&adjusted, &request(&[0, 3], 3, "mean")),
        "Stack Mode requires raster layers; layer 3 is an adjustment layer"
    );
    let message = refusal(&doc, &request(&[0, 1], 1, "trimmed_mean"));
    assert!(
        message.starts_with("invalid stack mode request") && message.contains("unknown variant"),
        "{message}"
    );
    let extra = json!({"op": "stack_mode", "layers": [0, 1], "above": 1, "mode": "mean",
                       "name": "Stat", "sampler": "bicubic"});
    let message = refusal(&doc, &extra);
    assert!(
        message.starts_with("invalid stack mode request") && message.contains("unknown field"),
        "{message}"
    );
    let missing = json!({"op": "stack_mode", "layers": [0, 1], "mode": "mean", "name": "Stat"});
    let message = refusal(&doc, &missing);
    assert!(
        message.starts_with("invalid stack mode request") && message.contains("above"),
        "{message}"
    );

    // Members with no pixel anywhere: the plan passes, the edit refuses.
    let clear = |w, h| {
        PrecisionImage::from_rgba16(
            w,
            h,
            vec![[0; 4]; (w * h) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap()
    };
    let empty = document16(vec![
        Layer::raster("A", clear(6, 5), (0, 0), 1.0, true).unwrap(),
        Layer::raster("B", clear(4, 4), (3, 3), 1.0, true).unwrap(),
    ]);
    assert!(plan_at(&empty, &request(&[0, 1], 1, "mean"), LIMIT).is_ok());
    assert_eq!(
        edit_with(&empty, &request(&[0, 1], 1, "mean"), LIMIT, ptr::null())
            .err()
            .as_deref(),
        Some(NO_PIXELS)
    );
}

/// `precision_structure::StructureEdit::scratch_bytes`' structure term,
/// restated: 4 MiB, every name twice, 4096 a record and 1024 a channel.
fn structure_term(document: &PrecisionDocument, name: &str) -> u64 {
    let names: u64 = names(document).iter().map(|n| n.len() as u64).sum();
    4 * 1024 * 1024 + (name.len() as u64 + names) * 2 + document.layer_count() as u64 * 4096
}

/// `copies` is the depth's own output-plane factor (`stack_stat`'s
/// `OUTPUT_PLANE_COPIES_16` / `_32`), named by the caller.
fn assert_plan_is_the_threshold(
    document: &PrecisionDocument,
    layers: &[usize],
    bpp: u64,
    copies: u64,
) {
    let members: Vec<Member> = layers.iter().map(|&i| member(document, i)).collect();
    let extent = stack_oracle::extent(&members);
    let (pixels, width) = (
        u64::from(extent.2) * u64::from(extent.3),
        u64::from(extent.2),
    );
    let req = request(layers, *layers.iter().max().unwrap(), "median");
    let planned = plan_at(document, &req, LIMIT).unwrap();
    assert_eq!(planned["input_samples"], json!(0));
    let scratch = planned["scratch_bytes"].as_u64().unwrap();
    let count = layers.len() as u64;
    let structure = structure_term(document, "Stat");
    // The formula, written out, and the kernel's own.
    let written = copies * pixels * bpp + (count + 1) * width * bpp + structure + 1024 * 1024;
    assert_eq!(scratch, written);
    assert_eq!(
        plan_scratch(pixels, width, bpp, count, structure),
        Some(scratch)
    );

    let total = planned["retained_bytes"].as_u64().unwrap() + scratch;
    assert!(plan_at(document, &req, total).is_ok());
    assert!(plan_at(document, &req, total - 1).is_err());
    assert!(edit_with(document, &req, total - 1, ptr::null()).is_err());
    assert!(edit_with(document, &req, total, ptr::null())
        .unwrap()
        .is_some());
}

#[test]
fn the_plan_is_the_threshold_and_the_tables_name_the_family() {
    // Each depth charges its own measured factor (`stack_stat`, "Admission"):
    // the wider the sample, the fewer output planes the whole call peaks at.
    assert_eq!(
        (
            OUTPUT_PLANE_COPIES_8,
            OUTPUT_PLANE_COPIES_16,
            OUTPUT_PLANE_COPIES_32
        ),
        (7, 5, 4)
    );
    assert_eq!(output_plane_copies(4), OUTPUT_PLANE_COPIES_8);
    assert_eq!(output_plane_copies(8), OUTPUT_PLANE_COPIES_16);
    assert_eq!(output_plane_copies(16), OUTPUT_PLANE_COPIES_32);
    assert_eq!(output_plane_copies(3), OUTPUT_PLANE_COPIES_8, "any other");
    assert_plan_is_the_threshold(&stack16(4), &[0, 1, 2, 3], 8, OUTPUT_PLANE_COPIES_16);
    assert_plan_is_the_threshold(&float_stack(3), &[1, 2, 3], 16, OUTPUT_PLANE_COPIES_32);

    for bits in [16, 32] {
        assert_eq!(
            capability("layer_stack_mode", "raster", bits)["supported"],
            true,
            "{bits} bits"
        );
        assert_eq!(
            capability("layer_stack_mode", "group", bits)["supported"],
            false,
            "{bits} bits"
        );
    }
    let table: Value = serde_json::from_str(&float_operators_json()).unwrap();
    let admitted: Vec<&str> = table["admitted"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(admitted.contains(&"layer_stack_mode"));
}

#[test]
fn a_cancelled_control_answers_the_cancel_sentence() {
    for doc in [stack16(3), float_stack(3)] {
        let layers: Vec<usize> = if doc.is_float() {
            vec![1, 2, 3]
        } else {
            vec![0, 1, 2]
        };
        let req = request(&layers, *layers.last().unwrap(), "median");
        let control = unsafe { dr_job_control_create() };
        unsafe { dr_job_control_cancel(control) };
        let result = edit_with(&doc, &req, LIMIT, control);
        unsafe { dr_job_control_free(control) };
        assert_eq!(result.err().as_deref(), Some("operation cancelled"));

        let control = unsafe { dr_job_control_create() };
        let result = edit_with(&doc, &req, LIMIT, control);
        unsafe { dr_job_control_free(control) };
        assert!(result.unwrap().is_some());
    }
}

// ------------------------------------------------------------------ float --

/// A varied float pixel: RGB from below zero to above white (4.0 and beyond),
/// alpha 0, under half, exactly half, and over.
fn linear(layer: u32, i: u32) -> [f32; 4] {
    let v = mix(layer + 40, i);
    let alpha = match v % 7 {
        0 => 0.0,
        1 => 0.49,
        2 => 0.5,
        3 => 0.25,
        4 => 0.8,
        _ => 1.0,
    };
    let red = match (v >> 3) % 5 {
        0 => 4.0,
        1 => -0.25,
        _ => ((v >> 8) % 1000) as f32 * 0.005 - 0.5,
    };
    [
        red,
        ((v >> 12) % 997) as f32 * 0.0061 - 1.0,
        ((v >> 20) % 499) as f32 * 0.0123,
        alpha,
    ]
}

/// A float document: an opaque canvas-sized "Background" at 0 that no test
/// stacks, and above it `n` records "M1"… in `SHAPES`, each pasted in place
/// from the test's own RZCB payload at its own offset.
fn float_stack(n: usize) -> PrecisionDocument {
    let profile = IccProfile::srgb();
    let base = vec![[0.25, 0.25, 0.25, 1.0]; (CW * CH) as usize];
    let mut doc = float_doc(CW, CH, &profile, &[base]);
    for i in 1..=n {
        let ((ox, oy), (w, h)) = SHAPES[i - 1];
        let pixels: Vec<[f32; 4]> = (0..w * h)
            .map(|k| {
                if i == 2 && (2..5).contains(&(k % w)) && (1..4).contains(&(k / w)) {
                    [0.0; 4]
                } else {
                    linear(i as u32, k)
                }
            })
            .collect();
        let mut bytes = drcb(w, h, profile.bytes(), &pixels);
        bytes[20..24].copy_from_slice(&ox.to_le_bytes());
        bytes[24..28].copy_from_slice(&oy.to_le_bytes());
        let payload = ClipboardPayload::decode(&bytes, LIMIT, 0).expect("a payload");
        let paste: PasteRequest = serde_json::from_value(json!({
            "name": format!("M{i}"), "anchor": i - 1, "position": "above",
            "placement": "in_place", "profile": "preserve",
        }))
        .unwrap();
        doc = doc
            .clipboard_paste(&payload, &paste, LIMIT, 0, None)
            .expect("the record pastes");
        let record = doc.layer(i).unwrap();
        assert_eq!(record.offset(), (ox, oy));
        assert_eq!(record.source().unwrap().dimensions(), (w, h));
    }
    assert!(doc.is_float());
    doc
}

#[test]
fn every_mode_matches_the_oracle_at_float() {
    for n in 2..=5usize {
        let doc = float_stack(n);
        let layers: Vec<usize> = (1..=n).collect();
        let members: Vec<Member> = layers.iter().map(|&i| member(&doc, i)).collect();
        let extent = stack_oracle::extent(&members);
        assert_extent_exceeds(&members, extent);
        for mode in MODES {
            let out = must(&doc, &request(&layers, n, mode));
            assert_eq!(out.layer_count(), n + 2);
            let made = out.layer(n + 1).unwrap();
            assert_eq!(made.name(), "Stat");
            assert_eq!(made.offset(), (extent.0, extent.1), "{mode}, {n}: origin");
            let image = made.source().unwrap();
            assert_eq!(image.dimensions(), (extent.2, extent.3));
            let got = image.to_rgba32().unwrap();
            let want = stack_oracle::real(mode, &members);
            let (mut covered, mut exact, mut above_white, mut negative) = (0, 0, 0, 0);
            for (i, (g, (w, voters))) in got.iter().zip(&want).enumerate() {
                assert_eq!(
                    g[3].to_bits(),
                    (w[3] as f32).to_bits(),
                    "{mode}, {n} members, pixel {i}: alpha"
                );
                let own_value =
                    matches!(mode, "minimum" | "maximum") || (mode == "median" && voters % 2 == 1);
                for c in 0..3 {
                    if own_value {
                        assert_eq!(
                            g[c].to_bits(),
                            (w[c] as f32).to_bits(),
                            "{mode}, {n} members, pixel {i} channel {c}: a member's own value"
                        );
                        exact += 1;
                    } else {
                        let bound = 1e-6 * w[c].abs().max(1.0);
                        assert!(
                            (f64::from(g[c]) - w[c]).abs() <= bound,
                            "{mode}, {n} members, pixel {i} channel {c}: {} against {}",
                            g[c],
                            w[c]
                        );
                    }
                    above_white += usize::from(g[c] > 1.0);
                    negative += usize::from(g[c] < 0.0);
                }
                covered += usize::from(w[3] > 0.0);
            }
            assert!(covered > 40, "{mode}, {n}: only {covered} covered pixels");
            assert!(
                covered < want.len(),
                "{mode}, {n}: some pixel stays transparent"
            );
            // Skewness, kurtosis and entropy are fractions of white.
            if !matches!(mode, "skewness" | "kurtosis" | "entropy") {
                assert!(
                    above_white > 0,
                    "{mode}, {n}: no value above white survived"
                );
            }
            if matches!(mode, "minimum" | "maximum" | "median") {
                assert!(exact > 0, "{mode}, {n}: nothing compared bit for bit");
            }
            if matches!(mode, "mean" | "median" | "minimum") {
                assert!(negative > 0, "{mode}, {n}: no value below zero survived");
            }
        }
    }
}

#[test]
fn a_float_stack_hides_its_members_and_leaves_the_rest() {
    let doc = float_stack(3);
    assert_eq!(names(&doc)[1..], ["M1", "M2", "M3"]);
    // Members 1 and 2, anchored on the visible non-member above them.
    let out = must(&doc, &request(&[1, 2], 3, "mean"));
    assert_eq!(names(&out)[1..], ["M1", "M2", "M3", "Stat"]);
    assert_eq!(visibility(&out), [true, false, false, true, true]);
    for i in 0..4 {
        assert!(same_pixels(&doc, i, &out, i), "record {i}: pixels");
    }
    assert_eq!(
        refusal(&doc, &request(&[1, 9], 9, "mean")),
        "stack mode layer 9 is out of range"
    );
    let grouped = must(
        &doc,
        &json!({"op": "group_layers", "layers": [1, 2], "name": "G", "pass_through": false}),
    );
    assert_eq!(names(&grouped)[1..], ["M1", "M2", "G", "M3"]);
    assert_eq!(
        refusal(&grouped, &request(&[1, 3], 3, "mean")),
        "Stack Mode requires raster layers; layer 3 is a group"
    );
    let inside = must(&grouped, &request(&[1, 2], 2, "range"));
    assert_eq!(names(&inside)[1..], ["M1", "M2", "Stat", "G", "M3"]);
}

/// The one float statistic that can leave the stored range: `max − min` of
/// members at opposite extremes is twice the storage bound. It is stored as
/// the bound, 65504, and the edit succeeds; it does not refuse.
#[test]
fn a_float_range_past_the_storage_bound_is_stored_as_the_bound() {
    const BOUND: f32 = 65504.0;
    let (w, h) = (4_u32, 3_u32);
    let high = vec![[BOUND, 2.5, -BOUND, 1.0]; (w * h) as usize];
    let low = vec![[-BOUND, 2.5, BOUND, 1.0]; (w * h) as usize];
    let doc = float_doc(w, h, &IccProfile::srgb(), &[high, low]);
    let out = must(&doc, &request(&[0, 1], 1, "range"));
    assert_eq!(out.layer_count(), 3);
    assert_eq!(names(&out)[2], "Stat");
    let made = out.layer(2).unwrap().source().unwrap();
    assert_eq!(made.dimensions(), (w, h));
    for (i, pixel) in made.to_rgba32().unwrap().iter().enumerate() {
        assert_eq!(
            pixel.map(f32::to_bits),
            [BOUND, 0.0, BOUND, 1.0].map(f32::to_bits),
            "pixel {i}: 131008 is stored as the bound"
        );
    }
    // The extremes themselves are members' own values and survive exactly.
    let lowest = must(&doc, &request(&[0, 1], 1, "minimum"));
    let made = lowest.layer(2).unwrap().source().unwrap();
    assert_eq!(
        made.to_rgba32().unwrap()[0].map(f32::to_bits),
        [-BOUND, 2.5, -BOUND, 1.0].map(f32::to_bits)
    );
}
