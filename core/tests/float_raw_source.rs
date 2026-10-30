//! A camera RAW record in a 32-bit float document, through the
//! RAW source, local-mask and source-edit FFI.
//!
//! A 16-bit SDR development's master stays u16 and its fallback is the f32
//! render into linear light; a 32-bit float (linear) development keeps
//! an f32 master, which attaches only in a float document, is stored under
//! typed word 78 and never narrows out of one.
//! Oracles, none read off the implementation: a rendered sample
//! is the f64 chain over the profiles' own tag data (`common::icc_chain`: the
//! master's curve, then `inv(dst.to_pcs) · src.to_pcs` when the profiles
//! differ, never clamped), within 1e-6 · max(1, |want|), alpha exactly
//! `code / 65535`; the original camera bytes, the descriptor and the mask
//! bytes are compared with what the test wrote; the refusal sentence is
//! written out here.

use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi_develop_inspect::dr_precision_doc_source_master;
use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::ffi_precision_conversion::{
    dr_precision_doc_narrow_structure_plan, dr_precision_doc_narrow_structure_to_u16,
    dr_precision_doc_narrow_structure_to_u8,
};
use doctored_core::ffi_raw_local::{
    dr_precision_doc_attach_raw_local_masks, dr_precision_doc_raw_local_mask,
    dr_raw_local_mask_edit,
};
use doctored_core::ffi_raw_source::{
    dr_precision_doc_attach_raw_source, dr_precision_doc_attach_raw_source_from,
    dr_precision_doc_raw_source_copy, dr_precision_doc_raw_source_from_plan,
    dr_precision_doc_raw_source_length, dr_precision_doc_raw_source_plan,
};
use doctored_core::ffi_retained_source::dr_precision_doc_mixed_source_plan;
use doctored_core::ffi_source_attachment::dr_precision_doc_source_attachment_kind;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::raw_local_mask::RawLocalMask;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::icc_chain::Chain;
use crate::float_fixture::float_doc;
use crate::float_placed_image::{
    assert_rendered, base, bits, byte_master, combined, edit, floats, info, mixed, oracle, owned,
    p3_codes, paste, place, transform, typed_master,
};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 5;
const H: u32 = 4;
const ORIGINAL: &[u8] = b"II*\0camera original for the float RAW tests\0\xff\x89\x11";
const MASTER_DEPTH: &str = "RAW attachment requires a genuine u16 or f32 developed master";
const FLOAT_MASTER_OWNER: &str =
    "RAW attachment of a 32-bit float developed master requires a 32-bit float document";
const RAW_MASTER_WORD: &str = "invalid native float RAW master word";
const FLOAT_RAW_NARROW: &str = "holds a 32-bit float Camera RAW development; redevelop it as a 16-bit SDR master or rasterize it first";
/// The kind tag `dr_precision_doc_source_attachment_kind` answers for RAW
/// local masks (0 is none, 1 host media).
const RAW_LOCAL_MASKS: u32 = 2;

// ------------------------------------------------------------- plumbing --

/// A RAW descriptor of `ORIGINAL` developed at `exposure`.
fn descriptor(exposure: f64) -> Value {
    json!({"schema_version":1,"original_filename":"camera.dng",
        "content_sha256":"b".repeat(64),
        "decoder":{"identifier":"test.decoder","version":"1.0","implementation":"test-platform"},
        "recipe":{"schema_version":1,"exposure":exposure,"white_balance":[5500.0,0.0],
            "output_space":"display_p3"}})
}

/// `dr_precision_doc_raw_source_plan` for `ORIGINAL`.
fn raw_plan(doc: &PrecisionDocument, layer: usize, descriptor: &Value) -> Result<Value, String> {
    let text = CString::new(descriptor.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_raw_source_plan(doc, layer, ORIGINAL.len(), text.as_ptr(), &mut err)
    };
    match owned(err) {
        Some(message) => Err(message),
        None => Ok(serde_json::from_str(&owned(out).expect("a plan")).unwrap()),
    }
}

/// `dr_precision_doc_attach_raw_source` of `ORIGINAL`.
fn attach_raw(
    doc: &PrecisionDocument,
    layer: usize,
    descriptor: &Value,
) -> Result<PrecisionDocument, String> {
    let text = CString::new(descriptor.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_attach_raw_source(
            doc,
            layer,
            ORIGINAL.as_ptr(),
            ORIGINAL.len(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    match owned(err) {
        Some(message) => Err(message),
        None => {
            assert!(!out.is_null(), "the attach changes the document");
            Ok(*unsafe { Box::from_raw(out) })
        }
    }
}

/// `dr_precision_doc_raw_source_from_plan`, then
/// `dr_precision_doc_attach_raw_source_from`: the original of `old`'s record
/// `layer` under a new descriptor, with the plan.
fn attach_raw_from(
    doc: &PrecisionDocument,
    layer: usize,
    old: &PrecisionDocument,
    descriptor: &Value,
) -> (Value, PrecisionDocument) {
    let text = CString::new(descriptor.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe {
        dr_precision_doc_raw_source_from_plan(doc, layer, old, layer, text.as_ptr(), &mut err)
    };
    if let Some(message) = owned(err) {
        panic!("{message}");
    }
    let plan = serde_json::from_str(&owned(plan).expect("a plan")).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_attach_raw_source_from(
            doc,
            layer,
            old,
            layer,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        panic!("{message}");
    }
    assert!(!out.is_null(), "the attach changes the document");
    (plan, *unsafe { Box::from_raw(out) })
}

/// `dr_precision_doc_raw_source_length`, then `_copy`.
fn raw_bytes(doc: &PrecisionDocument, layer: usize) -> Vec<u8> {
    let mut err = ptr::null_mut();
    let length = unsafe { dr_precision_doc_raw_source_length(doc, layer, &mut err) };
    assert!(owned(err).is_none());
    let mut bytes = vec![0x6b; length];
    let mut err = ptr::null_mut();
    let copied = unsafe {
        dr_precision_doc_raw_source_copy(doc, layer, bytes.as_mut_ptr(), bytes.len(), &mut err)
    };
    assert!(copied, "{:?}", owned(err));
    bytes
}

/// A linear-gradient local mask over the `W × H` master, from
/// `dr_raw_local_mask_edit`.
fn mask() -> Box<RawLocalMask> {
    let command = CString::new(json!({"op":"linear","from":[0,0],"to":[W,H]}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_raw_local_mask_edit(
            ptr::null(),
            W,
            H,
            command.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        panic!("{message}");
    }
    assert!(!out.is_null());
    unsafe { Box::from_raw(out) }
}

fn mask_descriptor(mask: &RawLocalMask) -> Value {
    json!({"schema_version":1,"kind":"raw_local_masks",
        "state":{"schema_version":1,"coordinates":"oriented-source-pixel-centers-v1",
            "width":W,"height":H,"nodes":["node0"]},
        "resources":[{"id":"node0","media_type":"com.rasterize.raw-local-mask",
            "sha256":"a".repeat(64),"byte_length":mask.encoded().len()}]})
}

/// `dr_precision_doc_attach_raw_local_masks` of one mask.
fn attach_mask(doc: &PrecisionDocument, layer: usize, mask: &RawLocalMask) -> PrecisionDocument {
    let text = CString::new(mask_descriptor(mask).to_string()).unwrap();
    let handles = [mask as *const RawLocalMask];
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_attach_raw_local_masks(
            doc,
            layer,
            text.as_ptr(),
            handles.as_ptr(),
            handles.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        panic!("{message}");
    }
    assert!(!out.is_null(), "the attach changes the document");
    *unsafe { Box::from_raw(out) }
}

/// `dr_precision_doc_raw_local_mask`: the stored mask's bytes.
fn mask_bytes(doc: &PrecisionDocument, layer: usize, index: usize) -> Vec<u8> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_raw_local_mask(doc, layer, index, &mut err) };
    if let Some(message) = owned(err) {
        panic!("{message}");
    }
    assert!(!out.is_null());
    unsafe { Box::from_raw(out) }.encoded().to_vec()
}

// ------------------------------------------------------------- fixtures --

/// A 16-bit Display P3 development placed at (2, 3) in a float sRGB
/// document, as a layers paste leaves it before its original is attached;
/// with the master's codes.
fn placed() -> (PrecisionDocument, Vec<[u16; 4]>) {
    let codes = p3_codes(W, H);
    let master = typed_master(W, H, codes.clone(), IccProfile::display_p3());
    let doc = mixed(&base(16, 12), None, Some(&master), place("camera", 2, 3))
        .unwrap()
        .unwrap();
    (doc, codes)
}

/// The codes of the second development: 16-bit sRGB, one sample half covered.
fn developed_codes() -> Vec<[u16; 4]> {
    (0..W * H)
        .map(|i| {
            [
                (i * 3001 + 500) as u16,
                (i * 7919 + 11) as u16,
                (65000 - i * 127) as u16,
                if i == 7 { 32768 } else { 65535 },
            ]
        })
        .collect()
}

/// The record redeveloped: the placed development with its original, mirrored
/// in place (x' = 9 − x maps the layer's columns 2…7 onto themselves, so each
/// output sample is one master sample: output column i is master column
/// 4 − i), then replaced by the sRGB development under `convert_fallback` and
/// given the original again under a changed recipe. `(old, plan, next)`.
fn redeveloped() -> (PrecisionDocument, Value, PrecisionDocument) {
    let (placed, _) = placed();
    let attached = attach_raw(&placed, 1, &descriptor(0.0)).unwrap();
    let old = edit(
        &attached,
        transform(1, [-1., 0., 0., 1., 9., 0.], "bilinear"),
    );
    let development = typed_master(W, H, developed_codes(), IccProfile::srgb());
    let replaced = mixed(
        &old,
        None,
        Some(&development),
        json!({"op":"replace","layer":1,"revision_id":"developed",
            "depth_conversion":"convert_fallback"}),
    )
    .unwrap()
    .unwrap();
    // A new revision carries no original until the host attaches it again.
    assert!(info(&replaced, 1)["raw"].is_null());
    let (plan, next) = attach_raw_from(&replaced, 1, &old, &descriptor(1.0));
    (old, plan, next)
}

fn mirrored(codes: &[[u16; 4]]) -> Vec<[u16; 4]> {
    let (w, h) = (W as usize, H as usize);
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| codes[y * w + (w - 1 - x)])
        .collect()
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_raw_original_attaches_to_a_float_placed_record() {
    let (placed, codes) = placed();
    let before = bits(&floats(&placed, 1));
    assert!(info(&placed, 1)["raw"].is_null());
    let recipe = descriptor(0.0);
    // The plan copies the original: its length is the work.
    let plan = raw_plan(&placed, 1, &recipe).unwrap();
    assert_eq!(plan["work_units"], ORIGINAL.len());
    let attached = attach_raw(&placed, 1, &recipe).unwrap();
    assert!(attached.is_float());
    assert_eq!(attached.float_refusal(), None);
    assert_eq!(raw_bytes(&attached, 1), ORIGINAL);
    let report = info(&attached, 1);
    assert_eq!(report["valid"], true);
    assert_eq!(report["master_depth"], 16);
    assert_eq!(report["fallback_depth"], 32);
    assert_eq!(report["raw"]["encoded_bytes"], ORIGINAL.len());
    assert_eq!(report["raw"]["original_filename"], "camera.dng");
    assert_eq!(report["raw"]["recipe"], recipe["recipe"]);
    // The attach renders nothing: the pixels are the place's, and they are
    // the P3 decode through P3 → sRGB, unclamped.
    let landed = floats(&attached, 1);
    assert_eq!(bits(&landed), before);
    let (s, d) = (
        Chain::of(&IccProfile::srgb()),
        Chain::of(&IccProfile::display_p3()),
    );
    let m = combined(&d, &s);
    assert_rendered(&landed, &codes, |c| oracle(&d, Some(m), c));
    assert!(landed.iter().any(|p| p[..3].iter().any(|v| *v > 1.0)));
    assert!(landed.iter().any(|p| p[..3].iter().any(|v| *v < 0.0)));
}

#[test]
fn a_raw_redevelopment_re_renders_in_linear_light_and_shares_the_original() {
    let (old, plan, next) = redeveloped();
    // Nothing is copied: the original is shared with the old record.
    assert_eq!(plan["work_units"], 0);
    assert_eq!(raw_bytes(&next, 1), ORIGINAL);
    let a = old.layer(1).unwrap().effective_source().unwrap();
    let b = next.layer(1).unwrap().effective_source().unwrap();
    assert!(Arc::ptr_eq(
        &a.asset.raw.as_ref().unwrap().encoded,
        &b.asset.raw.as_ref().unwrap().encoded
    ));
    // The new descriptor reads back; the old record keeps its own.
    let report = info(&next, 1);
    assert_eq!(report["valid"], true);
    assert_eq!(report["revision_id"], "developed");
    assert_eq!(report["master_depth"], 16);
    assert_eq!(report["fallback_depth"], 32);
    assert_eq!(report["raw"]["recipe"], descriptor(1.0)["recipe"]);
    assert_eq!(info(&old, 1)["raw"]["recipe"], descriptor(0.0)["recipe"]);
    // The layer's transform is kept, and the pixels are the new development's
    // sRGB decode through it: a mirror in place.
    assert!(next.is_float());
    assert_eq!(next.layer(1).unwrap().offset(), (2, 3));
    assert_eq!(next.layer_source(1).unwrap().dimensions(), (W, H));
    let s = Chain::of(&IccProfile::srgb());
    assert_rendered(&floats(&next, 1), &mirrored(&developed_codes()), |c| {
        oracle(&s, None, c)
    });
    // The old record still shows the first development, mirrored too.
    let d = Chain::of(&IccProfile::display_p3());
    let m = combined(&d, &s);
    assert_rendered(&floats(&old, 1), &mirrored(&p3_codes(W, H)), |c| {
        oracle(&d, Some(m), c)
    });
}

#[test]
fn raw_local_masks_attach_to_a_float_record() {
    let (_, _, next) = redeveloped();
    assert_eq!(
        unsafe { dr_precision_doc_source_attachment_kind(&next, 1) },
        0
    );
    let before = bits(&floats(&next, 1));
    let mask = mask();
    let masked = attach_mask(&next, 1, &mask);
    assert!(masked.is_float());
    assert_eq!(
        unsafe { dr_precision_doc_source_attachment_kind(&masked, 1) },
        RAW_LOCAL_MASKS
    );
    assert_eq!(mask_bytes(&masked, 1, 0), mask.encoded());
    // The attach renders nothing and keeps the original and its recipe.
    assert_eq!(bits(&floats(&masked, 1)), before);
    assert_eq!(raw_bytes(&masked, 1), ORIGINAL);
    assert_eq!(info(&masked, 1)["raw"], info(&next, 1)["raw"]);
    assert_eq!(info(&masked, 1)["fallback_depth"], 32);
}

#[test]
fn a_float_raw_document_reopens_with_its_original_and_masks() {
    let (_, _, next) = redeveloped();
    let mask = mask();
    let doc = attach_mask(&next, 1, &mask);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("raw.rz");
    let name = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let saved = unsafe {
        dr_precision_doc_save_native(&doc, name.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(saved, "{:?}", owned(err));
    let bytes = std::fs::read(&path).unwrap();
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert!(reopened.is_float());
    assert_eq!(raw_bytes(&reopened, 1), ORIGINAL);
    assert_eq!(info(&reopened, 1), info(&doc, 1));
    assert_eq!(
        info(&reopened, 1)["raw"]["recipe"],
        descriptor(1.0)["recipe"]
    );
    assert_eq!(
        unsafe { dr_precision_doc_source_attachment_kind(&reopened, 1) },
        RAW_LOCAL_MASKS
    );
    assert_eq!(mask_bytes(&reopened, 1, 0), mask.encoded());
    assert_eq!(bits(&floats(&reopened, 1)), bits(&floats(&doc, 1)));
    assert_eq!(
        reopened.layer(1).unwrap().offset(),
        doc.layer(1).unwrap().offset()
    );
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);
}

#[test]
fn a_pasted_raw_layer_keeps_its_original_in_a_float_document() {
    // The clip: a 16-bit Display P3 document whose one layer is the retained
    // development with its original, as the host opens a camera file.
    let codes = p3_codes(W, H);
    let sixteen = typed_master(W, H, codes.clone(), IccProfile::display_p3());
    let retained = edit(
        &sixteen,
        json!({"op":"retain","source_id":"camera","revision_id":"initial"}),
    );
    let recipe = descriptor(0.5);
    let clip = attach_raw(&retained, 0, &recipe).unwrap();
    assert!(!clip.is_float());
    assert_eq!(info(&clip, 0)["fallback_depth"], 16);

    let (out, report) = paste(&base(W, H), &clip);
    assert_eq!(
        report,
        json!({"blend_reset":[],"effect_blend_reset":[],"rasterized":[]})
    );
    // Record 0 is the destination's, 1 the pasted development: re-rendered
    // from its u16 master into linear light, unclamped, its original kept.
    assert!(out.is_float());
    assert_eq!(raw_bytes(&out, 1), ORIGINAL);
    assert_eq!(raw_bytes(&clip, 0), ORIGINAL);
    let landed = info(&out, 1);
    assert_eq!(landed["valid"], true);
    assert_eq!(landed["master_depth"], 16);
    assert_eq!(landed["fallback_depth"], 32);
    assert_eq!(landed["raw"], info(&clip, 0)["raw"]);
    assert_eq!(landed["raw"]["recipe"], recipe["recipe"]);
    let (s, d) = (
        Chain::of(&IccProfile::srgb()),
        Chain::of(&IccProfile::display_p3()),
    );
    let m = combined(&d, &s);
    let pixels = floats(&out, 1);
    assert_rendered(&pixels, &codes, |c| oracle(&d, Some(m), c));
    assert!(pixels
        .iter()
        .any(|p| p[..3].iter().any(|v| *v > 1.0 || *v < 0.0)));
}

/// A scene-linear development's samples: highlights above white, a component
/// below zero, all opaque.
fn linear_pixels() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let t = i as f32;
            [3.5 - t * 0.125, 0.25 + t * 0.0625, t * 0.03125 - 0.25, 1.0]
        })
        .collect()
}

/// A 32-bit float (linear) development placed at (2, 3) in a float sRGB
/// document, before its original is attached.
fn float_placed() -> PrecisionDocument {
    let master = float_doc(W, H, &IccProfile::srgb(), &[linear_pixels()]);
    mixed(&base(16, 12), None, Some(&master), place("camera", 2, 3))
        .unwrap()
        .unwrap()
}

/// `dr_precision_doc_source_master`: the f32 master's samples.
fn master_floats(doc: &PrecisionDocument, layer: usize) -> Vec<[f32; 4]> {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_source_master(doc, layer, &mut err) };
    if let Some(message) = owned(err) {
        panic!("{message}");
    }
    assert!(!out.is_null());
    unsafe { Box::from_raw(out) }
        .to_rgba32()
        .expect("an f32 master")
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn spliced(bytes: &[u8], at: usize, value: u32) -> Vec<u8> {
    let mut out = bytes[..at].to_vec();
    out.extend(value.to_le_bytes());
    out.extend(&bytes[at..]);
    out
}

fn reopen(bytes: &[u8]) -> Result<PrecisionDocument, String> {
    PrecisionDocument::parse_native(bytes, LIMIT, 0, None)
}

/// `dr_precision_doc_narrow_structure_plan`.
fn narrow_plan(doc: &PrecisionDocument, depth: u32) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_narrow_structure_plan(doc, depth, &mut err) };
    if let Some(message) = owned(err) {
        panic!("{message}");
    }
    serde_json::from_str(&owned(out).expect("a plan")).unwrap()
}

/// `dr_precision_doc_narrow_structure_to_u16`'s refusal, if it refuses.
fn narrow_sixteen(doc: &PrecisionDocument) -> Option<String> {
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_narrow_structure_to_u16(doc, LIMIT, 0, ptr::null(), &mut err) };
    let refusal = owned(err);
    if !out.is_null() {
        drop(unsafe { Box::from_raw(out) });
    }
    assert_eq!(out.is_null(), refusal.is_some());
    refusal
}

/// `dr_precision_doc_narrow_structure_to_u8`'s refusal, if it refuses.
fn narrow_eight(doc: &PrecisionDocument) -> Option<String> {
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_narrow_structure_to_u8(doc, 0, LIMIT, 0, ptr::null(), &mut err) };
    let refusal = owned(err);
    if !out.is_null() {
        drop(unsafe { Box::from_raw(out) });
    }
    assert_eq!(out.is_null(), refusal.is_some());
    refusal
}

#[test]
fn a_raw_original_attaches_to_a_float_master_only_in_a_float_document() {
    // In a float document the f32 master takes the camera original.
    let placed = float_placed();
    assert_eq!(info(&placed, 1)["master_depth"], 32);
    let before = bits(&floats(&placed, 1));
    let recipe = descriptor(0.0);
    let plan = raw_plan(&placed, 1, &recipe).unwrap();
    assert_eq!(plan["work_units"], ORIGINAL.len());
    let attached = attach_raw(&placed, 1, &recipe).unwrap();
    assert!(attached.is_float());
    assert_eq!(attached.float_refusal(), None);
    assert_eq!(raw_bytes(&attached, 1), ORIGINAL);
    let report = info(&attached, 1);
    assert_eq!(report["valid"], true);
    assert_eq!(report["master_depth"], 32);
    assert_eq!(report["fallback_depth"], 32);
    assert_eq!(report["raw"]["encoded_bytes"], ORIGINAL.len());
    assert_eq!(report["raw"]["recipe"], recipe["recipe"]);
    // The attach renders nothing, and the master is the development bit for
    // bit: values above white and below zero included.
    assert_eq!(bits(&floats(&attached, 1)), before);
    assert_eq!(bits(&master_floats(&attached, 1)), bits(&linear_pixels()));
    assert!(master_floats(&attached, 1)
        .iter()
        .any(|p| p[..3].iter().any(|v| *v > 1.0)));

    // A 16-bit document may hold a float master, never a
    // camera original on one.
    let mut request = place("camera", 0, 0);
    request["depth_conversion"] = json!("convert_fallback");
    let master = float_doc(W, H, &IccProfile::srgb(), &[linear_pixels()]);
    let sixteen = mixed(
        &typed_master(8, 8, p3_codes(8, 8), IccProfile::srgb()),
        None,
        Some(&master),
        request,
    )
    .unwrap()
    .unwrap();
    assert!(!sixteen.is_float());
    assert_eq!(info(&sixteen, 1)["master_depth"], 32);
    for refusal in [
        raw_plan(&sixteen, 1, &recipe).unwrap_err(),
        attach_raw(&sixteen, 1, &recipe)
            .err()
            .expect("the attach refuses"),
    ] {
        assert_eq!(refusal, FLOAT_MASTER_OWNER);
    }
    assert!(info(&sixteen, 1)["raw"].is_null());

    // An 8-bit master takes none anywhere.
    let eight = mixed(
        &base(16, 12),
        Some(&byte_master(W, H, |x, y| {
            [(x * 41 + y * 7) as u8, 200, (x * y * 23 + 9) as u8, 255]
        })),
        None,
        place("eight", 0, 0),
    )
    .unwrap()
    .unwrap();
    assert_eq!(info(&eight, 1)["master_depth"], 8);
    assert_eq!(raw_plan(&eight, 1, &recipe).unwrap_err(), MASTER_DEPTH);
    assert_eq!(
        attach_raw(&eight, 1, &recipe)
            .err()
            .expect("the attach refuses"),
        MASTER_DEPTH
    );
    assert!(info(&eight, 1)["raw"].is_null());
}

/// The 32-bit float RAW open: the development is the document, retained in
/// place, so its one layer is its own f32 master.
#[test]
fn a_float_development_is_retained_in_place_as_its_own_master() {
    let pixels = linear_pixels();
    let development = float_doc(W, H, &IccProfile::srgb(), std::slice::from_ref(&pixels));
    let retained = edit(
        &development,
        json!({"op":"retain","source_id":"camera","revision_id":"initial"}),
    );
    assert!(retained.is_float());
    assert_eq!(retained.float_refusal(), None);
    let report = info(&retained, 0);
    assert_eq!(report["valid"], true);
    assert_eq!(report["master_depth"], 32);
    assert_eq!(report["fallback_depth"], 32);
    assert_eq!(bits(&floats(&retained, 0)), bits(&pixels));
    assert_eq!(bits(&master_floats(&retained, 0)), bits(&pixels));
    let attached = attach_raw(&retained, 0, &descriptor(0.0)).unwrap();
    assert_eq!(raw_bytes(&attached, 0), ORIGINAL);
    assert_eq!(bits(&floats(&attached, 0)), bits(&pixels));
    let bytes = attached.encode_native(LIMIT, 0, None).unwrap();
    assert_eq!(
        (word(&bytes, 4), word(&bytes, 8), word(&bytes, 12)),
        (46, 78, 67)
    );
    let reopened = reopen(&bytes).unwrap();
    assert_eq!(info(&reopened, 0), info(&attached, 0));
    assert_eq!(bits(&master_floats(&reopened, 0)), bits(&pixels));
    assert_eq!(raw_bytes(&reopened, 0), ORIGINAL);
}

#[test]
fn a_float_raw_master_reopens_bit_exact_under_word_78() {
    let attached = attach_raw(&float_placed(), 1, &descriptor(0.5)).unwrap();
    let mask = mask();
    let doc = attach_mask(&attached, 1, &mask);
    for (doc, masked) in [(&attached, false), (&doc, true)] {
        let bytes = doc.encode_native(LIMIT, 0, None).unwrap();
        // Word 78 sits immediately before the float-master word 67.
        assert_eq!(
            (word(&bytes, 4), word(&bytes, 8), word(&bytes, 12)),
            (46, 78, 67)
        );
        let reopened = reopen(&bytes).unwrap();
        assert!(reopened.is_float());
        assert_eq!(info(&reopened, 1), info(doc, 1));
        assert_eq!(info(&reopened, 1)["master_depth"], 32);
        assert_eq!(
            info(&reopened, 1)["raw"]["recipe"],
            descriptor(0.5)["recipe"]
        );
        assert_eq!(raw_bytes(&reopened, 1), ORIGINAL);
        assert_eq!(bits(&master_floats(&reopened, 1)), bits(&linear_pixels()));
        assert_eq!(bits(&floats(&reopened, 1)), bits(&floats(doc, 1)));
        if masked {
            assert_eq!(
                unsafe { dr_precision_doc_source_attachment_kind(&reopened, 1) },
                RAW_LOCAL_MASKS
            );
            assert_eq!(mask_bytes(&reopened, 1, 0), mask.encoded());
        }
        assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);

        // The asset without its word: the file is the same bytes less 78.
        let mut without = bytes[..8].to_vec();
        without.extend(&bytes[12..]);
        assert_eq!(word(&without, 8), 67);
        assert_eq!(
            reopen(&without).err().as_deref(),
            Some(if masked {
                "retained RAW requires genuine u16 master"
            } else {
                "retained RAW requires a genuine RGBA16 developed master"
            })
        );
    }

    // The word without such an asset: a float master with no original, a
    // u16 RAW master in a float document, and a file with no word 67.
    let plain = float_placed().encode_native(LIMIT, 0, None).unwrap();
    assert_eq!((word(&plain, 4), word(&plain, 8)), (46, 67));
    assert!(reopen(&plain).is_ok());
    assert_eq!(
        reopen(&spliced(&plain, 8, 78)).err().as_deref(),
        Some(RAW_MASTER_WORD)
    );
    let (_, _, sdr) = redeveloped();
    let sdr = sdr.encode_native(LIMIT, 0, None).unwrap();
    assert_eq!(word(&sdr, 4), 46);
    assert_ne!(word(&sdr, 8), 78);
    assert_ne!(word(&sdr, 8), 67);
    assert!(reopen(&sdr).is_ok());
    assert_eq!(
        reopen(&spliced(&sdr, 8, 78)).err().as_deref(),
        Some(RAW_MASTER_WORD)
    );
}

#[test]
fn a_float_raw_development_never_narrows_out_of_a_float_document() {
    let doc = attach_raw(&float_placed(), 1, &descriptor(0.0)).unwrap();
    let sentence = format!(
        "Preserve Layers cannot convert this document: layer 1 \"camera\": {FLOAT_RAW_NARROW}"
    );
    for depth in [16, 8] {
        let plan = narrow_plan(&doc, depth);
        let blocked = plan["blocked"].as_array().unwrap();
        assert_eq!(blocked.len(), 1, "{depth} bits: {plan}");
        assert_eq!(blocked[0]["index"], 1);
        assert_eq!(blocked[0]["name"], "camera");
        assert_eq!(blocked[0]["reason"], FLOAT_RAW_NARROW);
    }
    assert_eq!(narrow_sixteen(&doc), Some(sentence.clone()));
    assert_eq!(narrow_eight(&doc), Some(sentence));
    // The input is untouched.
    assert!(doc.is_float());
    assert_eq!(info(&doc, 1)["master_depth"], 32);
    assert_eq!(raw_bytes(&doc, 1), ORIGINAL);

    // The same float master WITHOUT a camera original narrows, and so does
    // a RAW record whose master is u16.
    let unattached = float_placed();
    assert!(narrow_plan(&unattached, 16)["blocked"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(narrow_sixteen(&unattached), None);
    assert_eq!(narrow_eight(&unattached), None);
    let (_, _, sdr) = redeveloped();
    assert_eq!(info(&sdr, 1)["master_depth"], 16);
    for depth in [16, 8] {
        assert!(
            narrow_plan(&sdr, depth)["blocked"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{depth} bits"
        );
    }
    assert_eq!(narrow_sixteen(&sdr), None);
    assert_eq!(narrow_eight(&sdr), None);
}

/// `RawLocalWork.replaceScratch` plans a redevelopment with the layer's
/// present master standing in for the one about to be developed, which is
/// the other depth when the encoding switches. In a float document the plan
/// charges the master at `max(master, fallback)` depth, so the stand-in's
/// plan is the developed asset's.
#[test]
fn a_float_documents_replace_plan_is_the_same_for_a_u16_and_an_f32_master() {
    let replace = json!({"op":"replace","layer":1,"revision_id":"developed",
        "depth_conversion":"convert_fallback"});
    let plan = |doc: &PrecisionDocument, source: &PrecisionDocument| -> Value {
        let text = CString::new(replace.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_mixed_source_plan(doc, ptr::null(), source, text.as_ptr(), &mut err)
        };
        if let Some(message) = owned(err) {
            panic!("plan: {message}");
        }
        serde_json::from_str(&owned(out).expect("a plan")).unwrap()
    };
    let sixteen = typed_master(W, H, developed_codes(), IccProfile::srgb());
    let float = float_doc(W, H, &IccProfile::srgb(), &[linear_pixels()]);
    // The layer holds a float development, then a 16-bit one: both ways of
    // the switch.
    let float_layer = attach_raw(&float_placed(), 1, &descriptor(0.0)).unwrap();
    let (sdr_layer, _) = placed();
    let sdr_layer = attach_raw(&sdr_layer, 1, &descriptor(0.0)).unwrap();
    for doc in [&float_layer, &sdr_layer] {
        let (a, b) = (plan(doc, &sixteen), plan(doc, &float));
        assert!(a["scratch_bytes"].as_u64().unwrap() > 0);
        assert_eq!(a["scratch_bytes"], b["scratch_bytes"], "{a} vs {b}");
    }
}

/// A float RAW master holding more than four local masks carries both
/// words: 79 (`raw_locals_native`) first inside 46, then 78 immediately
/// before 67, and reopens bit-exact. The merged DRDC 79 writer keeps both.
#[test]
fn a_float_raw_master_with_five_masks_writes_words_79_and_78() {
    let attached = attach_raw(&float_placed(), 1, &descriptor(0.5)).unwrap();
    let masks: Vec<Box<RawLocalMask>> = (0..5).map(|_| mask()).collect();
    let ids: Vec<String> = (0..masks.len()).map(|i| format!("node{i}")).collect();
    let text = CString::new(
        json!({"schema_version":1,"kind":"raw_local_masks",
            "state":{"schema_version":1,"coordinates":"oriented-source-pixel-centers-v1",
                "width":W,"height":H,"nodes":ids},
            "resources":ids.iter().zip(&masks).map(|(id, m)| json!({"id":id,
                "media_type":"com.rasterize.raw-local-mask","sha256":"a".repeat(64),
                "byte_length":m.encoded().len()})).collect::<Vec<_>>()})
        .to_string(),
    )
    .unwrap();
    let handles: Vec<*const RawLocalMask> = masks.iter().map(|m| &**m as *const _).collect();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_attach_raw_local_masks(
            &attached,
            1,
            text.as_ptr(),
            handles.as_ptr(),
            handles.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        panic!("{message}");
    }
    assert!(!out.is_null());
    let doc = *unsafe { Box::from_raw(out) };
    let bytes = doc.encode_native(LIMIT, 0, None).unwrap();
    assert_eq!((word(&bytes, 4), word(&bytes, 8)), (46, 79));
    let words: Vec<u32> = (12..40).step_by(4).map(|at| word(&bytes, at)).collect();
    let at = words
        .iter()
        .position(|&w| w == 78)
        .expect("word 78 after 79");
    assert_eq!(words[at + 1], 67, "{words:?}");
    let reopened = reopen(&bytes).unwrap();
    assert!(reopened.is_float());
    assert_eq!(raw_bytes(&reopened, 1), ORIGINAL);
    for (index, mask) in masks.iter().enumerate() {
        assert_eq!(mask_bytes(&reopened, 1, index), mask.encoded());
    }
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);
}
