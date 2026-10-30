//! Phase 12E-2b: PLACEMENT at a chosen slot — `place`'s optional
//! `anchor`/`position` pair at BOTH depths, the `dr_doc_artwork_at` /
//! `dr_precision_doc_artwork_at` exports, and the PLAN ≡ EDIT claim the two
//! `source_plan` exports carry.
//!
//! Every oracle is written out here and never read off the implementation:
//!
//! * the expected structure table (name, level, clipped) after each of the
//!   three positions, at both depths;
//! * `Arc::ptr_eq` at byte depth, and `PrecisionImage::ptr_eq` for pixels
//!   and sample-buffer ADDRESS equality for masks at high depth, for every
//!   surviving record — a placement splices one node in and must copy no
//!   neighbour;
//! * the identity table: every prior id carried in order, exactly ONE mint;
//! * the input document, unchanged, after every call (purity);
//! * the LEGACY push (`place` with neither field) as the oracle for
//!   `position:"top"` on a flat document;
//! * the ABSENT-position plan's JSON as the oracle for the positioned plan's,
//!   field for field — the plan is position-independent, which is exactly why
//!   the plan GUARD is what makes a pushing plan detectable;
//! * a reopen → resave of the saved `.rz`, compared byte for byte.
//!
//! Tolerances, fixed BEFORE anything was measured: every comparison in this
//! file is EXACT. Structure, ids, addresses, plan JSON and file bytes are all
//! equalities, and no pixel tolerance is used anywhere.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{DrDocument, LayerRaster, MaskKind};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{dr_doc_artwork_at, dr_precision_doc_artwork_at};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_save_native,
};
use doctored_core::ffi_retained_source::{
    dr_doc_mixed_source_edit, dr_doc_mixed_source_plan, dr_precision_doc_source_edit,
    dr_precision_doc_source_plan,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const ART: &str = r#"{"type":"shape","version":1,"probe":"opaque to the core"}"#;

// ------------------------------------------------------------- plumbing --

unsafe fn owned(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    value
}

fn c(value: &Value) -> CString {
    CString::new(value.to_string()).unwrap()
}

/// The refusal a byte request phrases; panics when it succeeded, since a
/// document is not a message.
fn refusal8(doc: &DrDocument, source: &DrDocument, request: &Value) -> String {
    match edit8(doc, source, request) {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused"),
    }
}

/// The typed twin of [`refusal8`].
fn refusal16(doc: &PrecisionDocument, source: &PrecisionDocument, request: &Value) -> String {
    match edit16(doc, source, request) {
        Err(message) => message,
        Ok(_) => panic!("{request} must be refused"),
    }
}

/// `{"op":"place", …}` with the optional `anchor`/`position` pair present only
/// when the caller names a slot, so the ABSENT case is literally the request
/// every pre-12E-2b caller sends.
fn place(name: &str, id: &str, slot: Option<(usize, &str)>, scaled: bool) -> Value {
    let mut value = json!({"op":"place","source_id":id,"revision_id":"revision_a",
                           "name":name,"x":0,"y":0});
    if scaled {
        value["source_transform"] = json!([0.5, 0.0, 0.0, 0.5, 0.0, 0.0]);
    }
    if let Some((anchor, position)) = slot {
        value["anchor"] = json!(anchor);
        value["position"] = json!(position);
    }
    value
}

// --------------------------------------------------- byte-side accessors --

#[derive(Debug, PartialEq, Eq, Clone)]
struct Row {
    name: String,
    level: usize,
    clipped: bool,
}

fn table8(doc: &DrDocument) -> Vec<Row> {
    doc.layers
        .iter()
        .map(|l| Row {
            name: l.name.clone(),
            level: usize::from(l.depth),
            clipped: l.clipped,
        })
        .collect()
}

fn table16(doc: &PrecisionDocument) -> Vec<Row> {
    fn walk(layers: &[Layer], level: usize, out: &mut Vec<Row>) {
        for layer in layers {
            if let Some(children) = layer.children() {
                walk(children, level + 1, out);
            }
            out.push(Row {
                name: layer.name().to_string(),
                level,
                clipped: layer.clipped(),
            });
        }
    }
    let mut out = Vec::new();
    walk(doc.root_layers(), 0, &mut out);
    out
}

fn want(rows: &[(&str, usize, bool)]) -> Vec<Row> {
    rows.iter()
        .map(|(name, level, clipped)| Row {
            name: (*name).to_string(),
            level: *level,
            clipped: *clipped,
        })
        .collect()
}

fn ids8(doc: &DrDocument) -> Vec<u64> {
    doc.layers.iter().map(|l| l.id).collect()
}

fn ids16(doc: &PrecisionDocument) -> Vec<u64> {
    (0..doc.layer_count())
        .map(|i| doc.layer_id(i).expect("an id"))
        .collect()
}

fn index8(doc: &DrDocument, name: &str) -> usize {
    doc.layers
        .iter()
        .position(|l| l.name == name)
        .unwrap_or_else(|| panic!("{name} is present"))
}

fn index16(doc: &PrecisionDocument, name: &str) -> usize {
    table16(doc)
        .iter()
        .position(|r| r.name == name)
        .unwrap_or_else(|| panic!("{name} is present"))
}

/// Every record the placement did not create, by NAME, with its pixel buffer
/// and mask plane compared by ADDRESS.
fn shared8(before: &DrDocument, after: &DrDocument) {
    for layer in &before.layers {
        let mirror = after
            .layers
            .iter()
            .find(|l| l.name == layer.name)
            .unwrap_or_else(|| panic!("{} survived", layer.name));
        assert!(
            LayerRaster::ptr_eq(&layer.pixels, &mirror.pixels),
            "{}'s pixels are the same allocation",
            layer.name
        );
        match (&layer.mask, &mirror.mask) {
            (Some(a), Some(b)) => assert!(
                Arc::ptr_eq(a, b),
                "{}'s mask is the same allocation",
                layer.name
            ),
            (None, None) => {}
            _ => panic!("{}'s mask presence changed", layer.name),
        }
        assert_eq!(layer.id, mirror.id, "{}'s identity is carried", layer.name);
    }
}

fn samples16(doc: &PrecisionDocument, name: &str) -> PrecisionImage {
    doc.layer_source(index16(doc, name))
        .expect("a raster record")
        .clone()
}

fn mask16(doc: &PrecisionDocument, name: &str) -> usize {
    doc.layer(index16(doc, name))
        .and_then(|l| l.mask())
        .expect("a masked record")
        .plane()
        .samples()
        .as_ptr() as usize
}

// -------------------------------------------------------------- fixtures --

/// The BYTE fixture, exactly three entries: `Plain` is an ordinary masked
/// raster with NO retained source, and `Group` holds `Inner`. The sourceless
/// entry 0 is what makes a pushing PLAN detectable — the plan guard asks the
/// hypothetical's record at the placed index for a retained source, and a plan
/// that pushed instead of splicing would be asking `Inner` or `Plain`.
fn byte_fixture() -> DrDocument {
    let doc = DrDocument::from_pixels(RgbaImage::from_fn(4, 2, |x, y| {
        Rgba([(x * 31 + y * 7) as u8, (x * 13) as u8, (y * 57) as u8, 255])
    }));
    let doc = doc.with_layer_name(0, "Plain").expect("rename");
    let doc = doc.add_mask(0, MaskKind::RevealAll).expect("a mask");
    let doc = doc
        .adding_image_layer(
            0,
            RgbaImage::from_pixel(4, 2, Rgba([9, 8, 7, 255])),
            "Inner",
        )
        .expect("Inner");
    let (doc, group, _, _) = doc.group_layers(&[1], "Group").expect("Group");
    assert_eq!(group, 2);
    assert_eq!(
        table8(&doc),
        want(&[
            ("Plain", 0, false),
            ("Inner", 1, false),
            ("Group", 0, false)
        ])
    );
    doc
}

/// The byte input document a `place` reads its master from: one plain raster.
fn byte_source() -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(4, 2, |x, y| {
        Rgba([
            (x * 61 + 3) as u8,
            (y * 97 + 11) as u8,
            (x * 5 + y * 23) as u8,
            if x == 0 { 0 } else { 255 },
        ])
    }))
}

fn typed_image(w: u32, h: u32, seed: u16) -> PrecisionImage {
    PrecisionImage::from_rgba16(
        w,
        h,
        (0..w * h)
            .map(|i| {
                let i = i as u16;
                [
                    seed.wrapping_add(i.wrapping_mul(1103)),
                    seed.wrapping_add(i.wrapping_mul(7109)),
                    seed.wrapping_add(i.wrapping_mul(9973)),
                    if i.is_multiple_of(5) { 0 } else { 65535 },
                ]
            })
            .collect(),
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

/// The TYPED fixture, the byte one's twin: `[Plain(masked), Inner, Group]` in
/// postorder, `Plain` carrying no retained source.
fn typed_fixture() -> PrecisionDocument {
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(4, 2, vec![65535; 8], None).unwrap(),
        Default::default(),
        true,
    )
    .unwrap();
    let doc = PrecisionDocument::from_parts(
        4,
        2,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Plain", typed_image(4, 2, 1000), (0, 0), 1.0, true)
                .unwrap()
                .with_mask(Some(mask)),
            Layer::isolated_normal(
                "Group",
                vec![Layer::raster("Inner", typed_image(4, 2, 30000), (0, 0), 1.0, true).unwrap()],
                1.0,
                true,
            )
            .unwrap(),
        ],
    )
    .unwrap();
    assert_eq!(
        table16(&doc),
        want(&[
            ("Plain", 0, false),
            ("Inner", 1, false),
            ("Group", 0, false)
        ])
    );
    doc
}

fn typed_source() -> PrecisionDocument {
    PrecisionDocument::from_image(
        typed_image(4, 2, 4242),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

// ------------------------------------------------------- FFI edit / plan --

fn edit8(doc: &DrDocument, source: &DrDocument, request: &Value) -> Result<DrDocument, String> {
    let text = c(request);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_mixed_source_edit(
            doc,
            source,
            ptr::null(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return Err(unsafe { owned(err) });
    }
    let result = unsafe { &*out }.clone();
    unsafe { dr_doc_free(out) };
    Ok(result)
}

fn edit16(
    doc: &PrecisionDocument,
    source: &PrecisionDocument,
    request: &Value,
) -> Result<PrecisionDocument, String> {
    let text = c(request);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_source_edit(doc, source, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    if out.is_null() {
        return Err(unsafe { owned(err) });
    }
    let result = unsafe { &*out }.clone();
    unsafe { dr_precision_doc_free(out) };
    Ok(result)
}

fn plan8(doc: &DrDocument, source: &DrDocument, request: &Value) -> Result<Value, String> {
    let text = c(request);
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_doc_mixed_source_plan(doc, source, ptr::null(), text.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(unsafe { owned(err) });
    }
    Ok(serde_json::from_str(&unsafe { owned(out) }).unwrap())
}

fn plan16(
    doc: &PrecisionDocument,
    source: &PrecisionDocument,
    request: &Value,
) -> Result<Value, String> {
    let text = c(request);
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_source_plan(doc, source, text.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(unsafe { owned(err) });
    }
    Ok(serde_json::from_str(&unsafe { owned(out) }).unwrap())
}

// ------------------------------------------- (a) the three positions, 8bit --

#[test]
fn a_byte_placement_lands_where_the_rule_says() {
    let doc = byte_fixture();
    let source = byte_source();
    let before = table8(&doc);
    let before_ids = ids8(&doc);

    let above = edit8(
        &doc,
        &source,
        &place("Placed", "a", Some((0, "above")), false),
    )
    .expect("above");
    assert_eq!(
        table8(&above),
        want(&[
            ("Plain", 0, false),
            ("Placed", 0, false),
            ("Inner", 1, false),
            ("Group", 0, false),
        ])
    );

    let into = edit8(
        &doc,
        &source,
        &place("Placed", "a", Some((2, "into_group")), false),
    )
    .expect("into_group");
    assert_eq!(
        table8(&into),
        want(&[
            ("Plain", 0, false),
            ("Inner", 1, false),
            ("Placed", 1, false),
            ("Group", 0, false),
        ])
    );

    let top = edit8(
        &doc,
        &source,
        &place("Placed", "a", Some((1, "top")), false),
    )
    .expect("top");
    assert_eq!(
        table8(&top),
        want(&[
            ("Plain", 0, false),
            ("Inner", 1, false),
            ("Group", 0, false),
            ("Placed", 0, false),
        ])
    );

    // The LEGACY request — neither field — is exactly `top` here, and its
    // placed layer carries the same retained source.
    let legacy = edit8(&doc, &source, &place("Placed", "a", None, false)).expect("legacy");
    assert_eq!(table8(&legacy), table8(&top));
    assert!(legacy.has_retained_source(3) && top.has_retained_source(3));
    assert_eq!(
        legacy.source_info(3).unwrap(),
        top.source_info(3).unwrap(),
        "the legacy push and `top` describe the same source"
    );
    assert_eq!(
        legacy.layers[3].pixels.contiguous_image().as_raw(),
        top.layers[3].pixels.contiguous_image().as_raw()
    );

    for (out, landing) in [(&above, 1usize), (&into, 2), (&top, 3)] {
        shared8(&doc, out);
        assert!(
            out.has_retained_source(landing),
            "the source is AT the slot"
        );
        let fresh = out.layers[landing].id;
        let carried: Vec<u64> = ids8(out).into_iter().filter(|&id| id != fresh).collect();
        assert_eq!(carried, before_ids, "every prior id is carried, in order");
        assert!(fresh != 0 && !before_ids.contains(&fresh), "one fresh mint");
        assert_eq!(index8(out, "Placed"), landing);
    }
    // PURITY.
    assert_eq!(table8(&doc), before);
    assert_eq!(ids8(&doc), before_ids);
}

// ------------------------------------------ (b) the three positions, 16bit --

#[test]
fn b_typed_placement_lands_where_the_rule_says() {
    let doc = typed_fixture();
    let source = typed_source();
    let before = table16(&doc);
    let before_ids = ids16(&doc);
    let plain = samples16(&doc, "Plain");
    let inner = samples16(&doc, "Inner");
    let plain_mask = mask16(&doc, "Plain");

    let above = edit16(
        &doc,
        &source,
        &place("Placed", "a", Some((0, "above")), false),
    )
    .expect("above");
    assert_eq!(
        table16(&above),
        want(&[
            ("Plain", 0, false),
            ("Placed", 0, false),
            ("Inner", 1, false),
            ("Group", 0, false),
        ])
    );

    let into = edit16(
        &doc,
        &source,
        &place("Placed", "a", Some((2, "into_group")), false),
    )
    .expect("into_group");
    assert_eq!(
        table16(&into),
        want(&[
            ("Plain", 0, false),
            ("Inner", 1, false),
            ("Placed", 1, false),
            ("Group", 0, false),
        ])
    );

    let top = edit16(
        &doc,
        &source,
        &place("Placed", "a", Some((1, "top")), false),
    )
    .expect("top");
    let legacy = edit16(&doc, &source, &place("Placed", "a", None, false)).expect("legacy");
    assert_eq!(table16(&top), table16(&legacy));
    assert_eq!(
        legacy.source_info(3).unwrap(),
        top.source_info(3).unwrap(),
        "the legacy push and `top` describe the same source"
    );
    assert_eq!(
        legacy.layer_source(3).unwrap().to_rgba16().unwrap(),
        top.layer_source(3).unwrap().to_rgba16().unwrap()
    );

    for (out, landing) in [(&above, 1usize), (&into, 2), (&top, 3)] {
        assert!(
            PrecisionImage::ptr_eq(&samples16(out, "Plain"), &plain),
            "Plain's samples are shared"
        );
        assert!(
            PrecisionImage::ptr_eq(&samples16(out, "Inner"), &inner),
            "Inner's samples are shared"
        );
        assert_eq!(mask16(out, "Plain"), plain_mask, "the mask is shared");
        assert!(
            out.has_retained_source(landing),
            "the source is AT the slot"
        );
        let fresh = out.layer_id(landing).expect("an id");
        let carried: Vec<u64> = ids16(out).into_iter().filter(|&id| id != fresh).collect();
        assert_eq!(carried, before_ids, "every prior id is carried, in order");
        assert!(fresh != 0 && !before_ids.contains(&fresh), "one fresh mint");
        assert_eq!(index16(out, "Placed"), landing);
    }
    // PURITY.
    assert_eq!(table16(&doc), before);
    assert_eq!(ids16(&doc), before_ids);
}

// ------------------------------------------------------ (c) the clip run --

#[test]
fn c_a_placement_joins_a_clip_run_at_both_depths() {
    // [Base, ClipTop(clipped)] — a placement ABOVE `Base` sits between them,
    // and the rule says it JOINS the run rather than becoming its new base.
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(4, 2, Rgba([20, 40, 60, 255])));
    let doc = doc.with_layer_name(0, "Base").expect("rename");
    let doc = doc
        .adding_image_layer(
            0,
            RgbaImage::from_pixel(4, 2, Rgba([1, 2, 3, 255])),
            "ClipTop",
        )
        .expect("ClipTop");
    let doc = doc.with_layer_clipped(1, true).expect("clip");
    let out = edit8(
        &doc,
        &byte_source(),
        &place("Placed", "a", Some((0, "above")), false),
    )
    .expect("above the clip base");
    assert_eq!(
        table8(&out),
        want(&[
            ("Base", 0, false),
            ("Placed", 0, true),
            ("ClipTop", 0, true),
        ])
    );

    let doc = PrecisionDocument::from_parts(
        4,
        2,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Base", typed_image(4, 2, 500), (0, 0), 1.0, true).unwrap(),
            Layer::raster("ClipTop", typed_image(4, 2, 600), (0, 0), 1.0, true)
                .unwrap()
                .with_clipped(true),
        ],
    )
    .unwrap();
    let out = edit16(
        &doc,
        &typed_source(),
        &place("Placed", "a", Some((0, "above")), false),
    )
    .expect("above the clip base");
    assert_eq!(
        table16(&out),
        want(&[
            ("Base", 0, false),
            ("Placed", 0, true),
            ("ClipTop", 0, true),
        ])
    );
}

// ------------------------------------------------------- (d) PLAN ≡ EDIT --

#[test]
fn d_the_positioned_plan_forecasts_the_positioned_edit() {
    // The plan's three numbers are position-INDEPENDENT (they count bytes and
    // work, not slots), so the positioned plan must equal the absent-position
    // plan field for field. That equality is not what discriminates: the PLAN
    // GUARD is. Entry 0 carries NO retained source, so a hypothetical that
    // pushed while `prepared.index` moved to the slot would be asking a
    // sourceless record for its source and would refuse here.
    let doc = byte_fixture();
    let source = byte_source();
    let legacy = plan8(&doc, &source, &place("Placed", "a", None, true)).expect("the legacy plan");
    for slot in [(0usize, "above"), (2, "into_group"), (1, "top")] {
        let request = place("Placed", "a", Some(slot), true);
        let planned = plan8(&doc, &source, &request).expect("a positioned plan");
        assert_eq!(
            planned, legacy,
            "{slot:?}: the plan is position-independent"
        );
        for key in ["retained_bytes", "scratch_bytes", "work_units"] {
            assert!(planned.get(key).is_some(), "the plan reports {key}");
        }
        // …and the EDIT then lands the sourced layer exactly at the slot the
        // plan forecast, with a `dest` that is NOT the rendered extent (the
        // request scales by a half), which is what makes the `dest`
        // substitution key on the same index at both sites.
        let out = edit8(&doc, &source, &request).expect("the positioned edit");
        let landing = index8(&out, "Placed");
        assert!(out.has_retained_source(landing));
        assert_eq!(out.layers[landing].pixels.dimensions(), (2, 1));
    }

    let doc = typed_fixture();
    let source = typed_source();
    let legacy = plan16(&doc, &source, &place("Placed", "a", None, true)).expect("the legacy plan");
    for slot in [(0usize, "above"), (2, "into_group"), (1, "top")] {
        let request = place("Placed", "a", Some(slot), true);
        let planned = plan16(&doc, &source, &request).expect("a positioned plan");
        assert_eq!(
            planned, legacy,
            "{slot:?}: the plan is position-independent"
        );
        let out = edit16(&doc, &source, &request).expect("the positioned edit");
        let landing = index16(&out, "Placed");
        assert!(out.has_retained_source(landing));
        assert_eq!(
            out.layer_source(landing).unwrap().dimensions(),
            (2, 1),
            "the scaled destination extent"
        );
    }
}

// ----------------------------------------------------------- (e) refusals --

#[test]
fn e_every_refusal_is_named() {
    let doc = byte_fixture();
    let source = byte_source();
    assert_eq!(
        refusal8(
            &doc,
            &source,
            &place("Placed", "a", Some((99, "above")), false)
        ),
        "layer insert anchor is out of range"
    );
    assert_eq!(
        refusal8(
            &doc,
            &source,
            &place("Placed", "a", Some((0, "into_group")), false)
        ),
        "into_group requires a group layer"
    );
    // One field without the other is refused rather than defaulted.
    let mut lonely = place("Placed", "a", None, false);
    lonely["anchor"] = json!(0);
    assert_eq!(
        refusal8(&doc, &source, &lonely),
        "placement position requires both an anchor layer and a position"
    );
    let mut lonely = place("Placed", "a", None, false);
    lonely["position"] = json!("above");
    assert_eq!(
        refusal8(&doc, &source, &lonely),
        "placement position requires both an anchor layer and a position"
    );
    // An unknown spelling is refused by the parser, never read as `above`.
    let unknown = place("Placed", "a", Some((0, "sideways")), false);
    assert!(refusal8(&doc, &source, &unknown).starts_with("invalid source request"));

    let doc = typed_fixture();
    let source = typed_source();
    assert_eq!(
        refusal16(
            &doc,
            &source,
            &place("Placed", "a", Some((99, "above")), false)
        ),
        "layer insert anchor is out of range"
    );
    assert_eq!(
        refusal16(
            &doc,
            &source,
            &place("Placed", "a", Some((0, "into_group")), false)
        ),
        "into_group requires a group layer"
    );
    // The PLAN refuses with the same sentences, before any pixel is decoded.
    assert_eq!(
        plan16(
            &doc,
            &source,
            &place("Placed", "a", Some((99, "above")), false)
        )
        .unwrap_err(),
        "layer insert anchor is out of range"
    );
}

// ------------------------------------------------------ (f) the .rz round --

#[test]
fn f_a_positioned_placement_survives_a_native_round_trip() {
    let dir = TempDir::new().unwrap();

    let doc = byte_fixture();
    let placed = edit8(
        &byte_fixture(),
        &byte_source(),
        &place("Placed", "a", Some((2, "into_group")), false),
    )
    .expect("into_group");
    let first = byte_native(&placed, &dir, "one.rz");
    let reopened = byte_open(&first, &dir, "two.rz");
    assert_eq!(table8(&reopened), table8(&placed), "the stack survives");
    assert!(reopened.has_retained_source(2), "and so does the source");
    assert_eq!(
        reopened.source_info(2).unwrap(),
        placed.source_info(2).unwrap()
    );
    assert_eq!(ids8(&reopened), ids8(&placed), "ids are persisted");
    let second = byte_native(&reopened, &dir, "three.rz");
    assert_eq!(first, second, "reopen → resave is byte-identical");
    assert_eq!(table8(&doc), table8(&byte_fixture()), "purity");

    let placed = edit16(
        &typed_fixture(),
        &typed_source(),
        &place("Placed", "a", Some((2, "into_group")), false),
    )
    .expect("into_group");
    let first = typed_native(&placed, &dir, "one16.rz");
    let reopened = typed_open(&first, &dir, "two16.rz");
    assert_eq!(table16(&reopened), table16(&placed), "the stack survives");
    assert!(reopened.has_retained_source(2), "and so does the source");
    assert_eq!(
        reopened.source_info(2).unwrap(),
        placed.source_info(2).unwrap()
    );
    assert_eq!(ids16(&reopened), ids16(&placed), "ids are persisted");
    let second = typed_native(&reopened, &dir, "three16.rz");
    assert_eq!(first, second, "reopen → resave is byte-identical");
}

fn byte_native(doc: &DrDocument, dir: &TempDir, name: &str) -> Vec<u8> {
    let path = dir.path().join(name);
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, cpath.as_ptr(), &mut err) },
        "{}",
        unsafe { owned(err) }
    );
    std::fs::read(path).unwrap()
}

fn byte_open(bytes: &[u8], dir: &TempDir, name: &str) -> DrDocument {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_doc_open(cpath.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", unsafe { owned(err) });
    let doc = unsafe { &*out }.clone();
    unsafe { dr_doc_free(out) };
    doc
}

fn typed_native(doc: &PrecisionDocument, dir: &TempDir, name: &str) -> Vec<u8> {
    let path = dir.path().join(name);
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(doc, cpath.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        unsafe { owned(err) }
    );
    std::fs::read(path).unwrap()
}

fn typed_open(bytes: &[u8], dir: &TempDir, name: &str) -> PrecisionDocument {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_open(cpath.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{}", unsafe { owned(err) });
    let doc = unsafe { &*out }.clone();
    unsafe { dr_precision_doc_free(out) };
    doc
}

// --------------------------------------------------------- (g) artwork_at --

#[test]
fn g_the_artwork_exports_place_at_the_three_positions() {
    let name = CString::new("Art").unwrap();
    let description = CString::new(ART).unwrap();
    let image = typed_image(4, 2, 777);

    // BYTE. Codes 0/1/2 land at the slot the rule names; an unknown code and a
    // NULL handle are refused rather than defaulted.
    let doc = byte_fixture();
    for (code, anchor, landing, level) in
        [(0u32, 0usize, 1usize, 0usize), (1, 1, 3, 0), (2, 2, 2, 1)]
    {
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_doc_artwork_at(
                &doc,
                anchor,
                code,
                &image,
                0,
                0,
                name.as_ptr(),
                description.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!out.is_null(), "code {code}: {}", unsafe { owned(err) });
        let result = unsafe { &*out }.clone();
        unsafe { dr_doc_free(out) };
        assert_eq!(index8(&result, "Art"), landing, "code {code}");
        assert_eq!(usize::from(result.layers[landing].depth), level);
        assert_eq!(result.layers[landing].meta.as_deref(), Some(ART));
        shared8(&doc, &result);
    }
    let mut err = ptr::null_mut();
    assert!(unsafe {
        dr_doc_artwork_at(
            &doc,
            0,
            9,
            &image,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    }
    .is_null());
    assert_eq!(unsafe { owned(err) }, "unsupported layer insert position");
    let mut err = ptr::null_mut();
    assert!(unsafe {
        dr_doc_artwork_at(
            ptr::null(),
            0,
            0,
            &image,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    }
    .is_null());
    assert_eq!(unsafe { owned(err) }, "required handle is NULL");
    // `into_group` on a NON-group refuses by name.
    let mut err = ptr::null_mut();
    assert!(unsafe {
        dr_doc_artwork_at(
            &doc,
            0,
            2,
            &image,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    }
    .is_null());
    assert_eq!(unsafe { owned(err) }, "into_group requires a group layer");

    // TYPED.
    let doc = typed_fixture();
    for (code, anchor, landing, level) in
        [(0u32, 0usize, 1usize, 0usize), (1, 1, 3, 0), (2, 2, 2, 1)]
    {
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_artwork_at(
                &doc,
                anchor,
                code,
                &image,
                0,
                0,
                name.as_ptr(),
                description.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!out.is_null(), "code {code}: {}", unsafe { owned(err) });
        let result = unsafe { &*out }.clone();
        unsafe { dr_precision_doc_free(out) };
        assert_eq!(index16(&result, "Art"), landing, "code {code}");
        assert_eq!(table16(&result)[landing].level, level);
        assert_eq!(result.artwork_description(landing), Some(ART));
        assert!(PrecisionImage::ptr_eq(
            &samples16(&result, "Plain"),
            &samples16(&doc, "Plain")
        ));
        assert_eq!(mask16(&result, "Plain"), mask16(&doc, "Plain"));
    }
    let mut err = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_artwork_at(
            &doc,
            0,
            9,
            &image,
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    }
    .is_null());
    assert_eq!(unsafe { owned(err) }, "unsupported layer insert position");
    let mut err = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_artwork_at(
            &doc,
            0,
            0,
            ptr::null(),
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    }
    .is_null());
    assert_eq!(unsafe { owned(err) }, "required handle is NULL");
}
