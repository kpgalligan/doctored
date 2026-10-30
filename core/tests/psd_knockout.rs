//! PSD `knko` / `infx` — Phase 12E-3c, both directions and both depths.
//!
//! Every claim here is an EQUALITY against an INDEPENDENT reference: the
//! committed Photoshop family's own `blocks.json` on the way in, and this
//! file's own `8BIM<key>` + `u32` length + body byte scan on the way out. No
//! expectation is read back out of a Doctored report and then compared with
//! itself, and no golden image is used.
//!
//! - (a) every record of the committed `knockout/source.psd` that carries a
//!   `knko` or an `infx` imports onto the layer of that name, and E9's two
//!   blending-options sentences land on the right records.
//! - (b) an 8- and a 16-bit round trip of a document holding a Deep raster at
//!   fill 0.5, a Shallow knockout GROUP and an `infx` raster with a colour
//!   overlay.
//! - (c) setting a key and clearing it restores the exact bytes.
//! - (d) a BAKED style writes neither block and reports the narrowed loss.
//! - (e) D13's two sentences: present for a qualifying bottom under a Deep
//!   raster, absent for a true Background, a trimmed bottom, a hidden bottom,
//!   a Shallow inside a group and a Deep whose style is BAKED (no `knko` is
//!   written, so that file punches nothing in Photoshop).
//! - (f) the framings that are NOT carried, and the closing divider whose
//!   `knko` marker loss stays.
//! - (g) P7-39: D6's clip-base narrowing is lifted, so neither side
//!   discloses anything for any clip-base case at either depth, and the
//!   committed Photoshop render `knockout_bottom/interior_on_clip_base` is
//!   reproduced at 8 and 16 bits.
//! - (h) 12F-1's two carried fixes (whose P7-39 sentences are gone with
//!   the narrowing, so (h1)-(h3) now pin their absence), each case at 8 AND
//!   16 bits: C1's ONE clip-run question — a VISIBLE SIBLING in the contiguous clipped run
//!   directly above the base AT THE BASE'S OWN LEVEL, a group read through
//!   its folder record — as (h1) the clipped-FOLDER false positive and (h2)
//!   the clipped-GROUP miss; C2's "renders" as (h3) — an enabled grouped
//!   effect its settings do not skip, so a zero-opacity overlay, satin or
//!   glow, or a zero-size edge glow, renders nothing and discloses nothing,
//!   while a live grouped effect beside one still does; and (h4) the SECOND
//!   reader of `ExportStyle.clip_base`, the BAKED-style clause, which the
//!   same fix moves at 8 bits.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_group::dr_doc_group_layers;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{
    dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_open_psd,
    dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as TLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES as LIMIT};
use doctored_core::style::{EffectKind, Knockout, LayerStyle};
use serde_json::Value;
use tempfile::TempDir;

use crate::common;
use crate::psd_mask_fixture as psd_fixture;

use common::psd_photoshop::{fixture as family_file, import_report, open_psd, strings, LAYERS};
use common::{add_layer, cpath, doc_from, set_style, solid, take_err_string};
use psd_fixture::{block, fixture as psd_fixture_bytes, Record};

const W: u32 = 8;
const H: u32 = 6;
const LAYERS_MODE: i32 = 0;

/// A colour overlay is a GROUPED interior effect, so `blend_interior_as_group`
/// beside it is not an identity and the style is actually stored.
const INTERIOR_STYLE: &str = "{\"blend_interior_as_group\":true,\"effects\":\
     [{\"type\":\"color_overlay\",\"blend\":\"normal\",\"color\":\"#00ff00\",\"opacity\":1}]}";
/// A sub-1 px stroke is `bake_reason`'s first case, so this style is
/// RASTERIZED rather than written — E2's condition.
const BAKED_STYLE: &str = "{\"knockout\":\"deep\",\"blend_interior_as_group\":true,\"effects\":\
     [{\"type\":\"stroke\",\"size\":0.5}]}";

// ------------------------------------------------------- the byte scanner --

/// Every `8BIM<key>` block body in `bytes`, in FILE order. This walk is this
/// test's own — it re-reads the u32 length itself rather than asking the
/// writer where it put anything.
fn scan(bytes: &[u8], key: &[u8; 4]) -> Vec<Vec<u8>> {
    let mut needle = b"8BIM".to_vec();
    needle.extend_from_slice(key);
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 12 <= bytes.len() {
        if bytes[at..at + 8] == needle[..] {
            let length = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            if at + 12 + length <= bytes.len() {
                out.push(bytes[at + 12..at + 12 + length].to_vec());
                at += 12 + length;
                continue;
            }
        }
        at += 1;
    }
    out
}

// ------------------------------------------------------------- the plumbing --

fn export_report(doc: *const DrDocument) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_doc_psd_export_report(doc, 0, LAYERS_MODE, &mut err) };
    assert!(!json.is_null(), "export report: {}", take_err_string(err));
    let text = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(json) };
    serde_json::from_str(&text).expect("export report JSON")
}

fn write_psd(doc: *const DrDocument, path: &Path) -> Vec<u8> {
    let c = cpath(path);
    let mut err: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_doc_save_psd(doc, ptr::null(), c.as_ptr(), 0, LAYERS_MODE, &mut err) };
    assert!(ok, "save_psd: {}", take_err_string(err));
    std::fs::read(path).expect("the written PSD")
}

fn typed_export_report(doc: &PrecisionDocument) -> Value {
    let mut err: *mut c_char = ptr::null_mut();
    let json = unsafe { dr_precision_doc_psd_export_report(doc, 0, LAYERS_MODE, &mut err) };
    assert!(!json.is_null(), "typed export report refused");
    let text = unsafe { CStr::from_ptr(json) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(json) };
    serde_json::from_str(&text).expect("typed export report JSON")
}

fn typed_write_psd(doc: &PrecisionDocument, path: &Path) -> Vec<u8> {
    let projection = doc.render(LIMIT, 0, None).expect("the typed projection");
    let c = CString::new(path.to_str().unwrap()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_psd(
                doc,
                &projection,
                c.as_ptr(),
                0,
                LAYERS_MODE,
                LIMIT,
                0,
                &mut err,
            )
        },
        "the 16-bit PSD write refused: {}",
        take_err_string(err)
    );
    std::fs::read(path).expect("the written 16-bit PSD")
}

fn parse_style(json: &str) -> LayerStyle {
    LayerStyle::from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn typed_image() -> PrecisionImage {
    PrecisionImage::from_rgba16(
        W,
        H,
        vec![[40000u16, 20000, 10000, 65535]; (W * H) as usize],
        IccProfile::srgb(),
        None,
    )
    .unwrap()
}

fn typed_raster(name: &str, style: Option<&str>) -> TLayer {
    let layer = TLayer::raster(name, typed_image(), (0, 0), 1.0, true).unwrap();
    match style {
        Some(json) => layer
            .with_layer_style(Some(&parse_style(json)), (W, H))
            .unwrap_or_else(|e| panic!("{name}: {e}")),
        None => layer,
    }
}

fn typed_document(layers: Vec<TLayer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

/// The byte C-shaped document of test (b): `Bottom`, a Shallow knockout GROUP
/// `Knock` holding `Inner`, a Deep raster at fill 0.5 and an `infx` raster.
fn byte_round_trip_document(dir: &TempDir) -> *mut DrDocument {
    let mut doc = doc_from(dir, "bottom.png", &solid(W, H, [20, 40, 60, 255]));
    doc = add_layer(
        dir,
        "inner.png",
        doc,
        0,
        &solid(W, H, [200, 40, 40, 255]),
        "Inner",
    );
    // Group entry 1 alone: the folder record lands at index 2, its child at 1.
    let indices = [1usize];
    let (mut group, mut cleared, mut cleared_len, mut reordered, mut reordered_len) =
        (0usize, [0usize; 8], 0usize, [0usize; 8], 0usize);
    let name = CString::new("Knock").unwrap();
    let grouped = unsafe {
        dr_doc_group_layers(
            doc,
            indices.as_ptr(),
            indices.len(),
            name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            8,
        )
    };
    assert!(!grouped.is_null(), "group_layers failed");
    unsafe { dr_doc_free(doc) };
    let mut doc = set_style(grouped, group, "{\"knockout\":\"shallow\"}");
    doc = add_layer(
        dir,
        "deep.png",
        doc,
        group,
        &solid(W, H, [40, 200, 40, 255]),
        "DeepKnock",
    );
    doc = set_style(
        doc,
        group + 1,
        "{\"fill_opacity\":0.5,\"knockout\":\"deep\"}",
    );
    doc = add_layer(
        dir,
        "interior.png",
        doc,
        group + 1,
        &solid(W, H, [40, 40, 200, 255]),
        "InteriorOn",
    );
    set_style(doc, group + 2, INTERIOR_STYLE)
}

// ------------------------------------------ (a) the committed source.psd --

#[test]
fn every_knko_and_infx_of_the_committed_photoshop_source_imports_onto_its_layer() {
    let path = family_file("knockout", "source.psd");
    let blocks: Value = serde_json::from_str(
        &std::fs::read_to_string(family_file("knockout", "blocks.json")).unwrap(),
    )
    .expect("blocks.json");
    let report = import_report(&path);
    let doc = open_psd(&path, &report, LAYERS);

    let mut checked = 0usize;
    for record in blocks["records"].as_object().expect("records").values() {
        let name = record["name"].as_str().expect("record name");
        if name == "</Layer group>" {
            continue;
        }
        let knockout = match record.get("knko").and_then(Value::as_str) {
            Some("01000000") => Knockout::Shallow,
            Some("02000000") => Knockout::Deep,
            None | Some("00000000") => Knockout::None,
            other => panic!("{name}: unmapped knko body {other:?}"),
        };
        let interior = match record.get("infx").and_then(Value::as_str) {
            Some("01000000") => true,
            None | Some("00000000") => false,
            other => panic!("{name}: unmapped infx body {other:?}"),
        };
        let layer = doc
            .layers
            .iter()
            .find(|layer| layer.name == name)
            .unwrap_or_else(|| panic!("no imported layer named {name}"));
        let style = layer.style.as_deref();
        assert_eq!(
            style.map_or(Knockout::None, |s| s.knockout),
            knockout,
            "{name}: knko"
        );
        assert_eq!(
            style.is_some_and(|s| s.blend_interior_as_group),
            interior,
            "{name}: infx"
        );
        checked += 1;
    }
    assert!(checked >= 18, "the whole family was walked, not a corner");

    // E9: the record whose `infx` was CARRIED withdraws the shared sentence
    // and its `clbl` states its own case; the record whose `infx` is at its
    // default keeps the pinned sentence byte-for-byte.
    const PINNED: &str = "has additional clipping/interior blending metadata; its payload is not interpreted or retained, and its presence alone does not establish an active effect.";
    const CLBL_ONLY: &str = "has additional clipping blending metadata (clbl); its payload is not interpreted or retained, and its presence alone does not establish an active effect.";
    let observations = strings(&report, "observations");
    let about = |name: &str| -> Vec<String> {
        observations
            .iter()
            .filter(|text| text.contains(&format!("“{name}”")))
            .cloned()
            .collect()
    };
    let on = about("InteriorOn");
    assert!(
        on.iter().any(|text| text.ends_with(CLBL_ONLY)),
        "InteriorOn reports the clbl-only sentence: {on:?}"
    );
    assert!(
        !on.iter().any(|text| text.ends_with(PINNED)),
        "and never the pinned one beside it: {on:?}"
    );
    let off = about("InteriorOff");
    assert!(
        off.iter().any(|text| text.ends_with(PINNED)),
        "InteriorOff keeps the pinned sentence: {off:?}"
    );
    assert!(
        !off.iter().any(|text| text.ends_with(CLBL_ONLY)),
        "and never the new one: {off:?}"
    );
}

// --------------------------------------------------- (b) the round trips --

#[test]
fn the_byte_export_writes_both_keys_in_record_order_and_they_reopen_per_layer() {
    let dir = TempDir::new().unwrap();
    let doc = byte_round_trip_document(&dir);
    let path = dir.path().join("keys.psd");
    let bytes = write_psd(doc, &path);

    // FILE order: Bottom, the closing divider, Inner, the FOLDER record
    // Knock, DeepKnock, InteriorOn. Only the folder and the Deep raster
    // carry a `knko`; only the interior raster carries an `infx`.
    assert_eq!(
        scan(&bytes, b"knko"),
        vec![vec![1, 0, 0, 0], vec![2, 0, 0, 0]],
        "the exact knko bodies, in record order"
    );
    assert_eq!(
        scan(&bytes, b"infx"),
        vec![vec![1, 0, 0, 0]],
        "the exact infx body"
    );

    let report = import_report(&path);
    let losses = strings(&report, "verified_losses");
    assert!(
        !losses.iter().any(|text| text.contains("knockout metadata")),
        "no knockout marker loss survives the carry: {losses:?}"
    );
    let observations = strings(&report, "observations");
    for needle in ["knockout (shallow)", "knockout (deep)"] {
        assert!(
            observations.iter().any(|text| text.contains(needle)),
            "the import report names {needle}: {observations:?}"
        );
    }

    let reopened = open_psd(&path, &report, LAYERS);
    let of = |name: &str| -> Option<LayerStyle> {
        reopened
            .layers
            .iter()
            .find(|layer| layer.name == name)
            .and_then(|layer| layer.style.as_deref().cloned())
    };
    assert_eq!(of("Knock").map(|s| s.knockout), Some(Knockout::Shallow));
    let deep = of("DeepKnock").expect("the Deep raster keeps a style");
    assert_eq!(deep.knockout, Knockout::Deep);
    // `iOpa` stores fill opacity in 1/255 steps, so 0.5 comes back as 128/255.
    assert!(
        (deep.fill_opacity - 0.5).abs() <= 1.0 / 255.0,
        "fill survives too: {}",
        deep.fill_opacity
    );
    assert!(of("InteriorOn").is_some_and(|s| s.blend_interior_as_group));
    unsafe { dr_doc_free(doc) };
}

#[test]
fn the_typed_export_writes_both_keys_in_record_order_and_they_reopen_per_layer() {
    let dir = TempDir::new().unwrap();
    let inner = typed_raster("Inner", None);
    let knock = TLayer::isolated_normal("Knock", vec![inner], 1.0, true)
        .unwrap()
        .with_layer_style(Some(&parse_style("{\"knockout\":\"shallow\"}")), (W, H))
        .unwrap();
    let doc = typed_document(vec![
        typed_raster("Bottom", None),
        knock,
        typed_raster(
            "DeepKnock",
            Some("{\"fill_opacity\":0.5,\"knockout\":\"deep\"}"),
        ),
        typed_raster("InteriorOn", Some(INTERIOR_STYLE)),
    ]);
    let path = dir.path().join("keys16.psd");
    let bytes = typed_write_psd(&doc, &path);
    assert_eq!(
        scan(&bytes, b"knko"),
        vec![vec![1, 0, 0, 0], vec![2, 0, 0, 0]],
        "the same bodies in the same record order at 16 bits"
    );
    assert_eq!(scan(&bytes, b"infx"), vec![vec![1, 0, 0, 0]]);

    let report = import_report(&path);
    assert_eq!(report["depth"], 16);
    let losses = strings(&report, "verified_losses");
    assert!(
        !losses.iter().any(|text| text.contains("knockout metadata")),
        "no knockout marker loss at 16 bits: {losses:?}"
    );
    let observations = strings(&report, "observations");
    for needle in ["knockout (shallow)", "knockout (deep)"] {
        assert!(
            observations.iter().any(|text| text.contains(needle)),
            "the 16-bit import report names {needle}: {observations:?}"
        );
    }
}

// ---------------------------------------------------- (c) exact restores --

#[test]
fn setting_a_key_and_clearing_it_restores_the_exact_psd_bytes_at_both_depths() {
    let dir = TempDir::new().unwrap();
    const PLAIN: &str = "{\"fill_opacity\":0.5}";
    const KEYED: &str = "{\"fill_opacity\":0.5,\"knockout\":\"deep\"}";

    let mut doc = doc_from(dir_ref(&dir), "b.png", &solid(W, H, [20, 40, 60, 255]));
    doc = add_layer(
        &dir,
        "t.png",
        doc,
        0,
        &solid(W, H, [200, 40, 40, 255]),
        "Top",
    );
    doc = set_style(doc, 1, PLAIN);
    let before = write_psd(doc, &dir.path().join("before.psd"));
    doc = set_style(doc, 1, KEYED);
    let keyed = write_psd(doc, &dir.path().join("keyed.psd"));
    assert_ne!(keyed, before, "a written key changes the bytes");
    doc = set_style(doc, 1, PLAIN);
    assert_eq!(
        write_psd(doc, &dir.path().join("after.psd")),
        before,
        "clearing the key restores the byte writer's exact bytes"
    );
    unsafe { dr_doc_free(doc) };

    let plain = typed_document(vec![
        typed_raster("Bottom", None),
        typed_raster("Top", Some(PLAIN)),
    ]);
    let keyed = typed_document(vec![
        typed_raster("Bottom", None),
        typed_raster("Top", Some(KEYED)),
    ]);
    let a = typed_write_psd(&plain, &dir.path().join("plain16.psd"));
    let b = typed_write_psd(&keyed, &dir.path().join("keyed16.psd"));
    assert!(scan(&a, b"knko").is_empty(), "no block at the default");
    assert_eq!(scan(&b, b"knko"), vec![vec![2, 0, 0, 0]]);
    assert_eq!(a.len() + 16, b.len(), "exactly one 16-byte block is added");
}

/// `doc_from` borrows the directory; naming the borrow keeps the call above
/// readable next to the moves around it.
fn dir_ref(dir: &TempDir) -> &TempDir {
    dir
}

// ---------------------------------------------------------- (d) the bake --

#[test]
fn a_baked_style_writes_neither_block_and_carries_the_narrowed_loss() {
    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "b.png", &solid(W, H, [20, 40, 60, 255]));
    doc = add_layer(
        &dir,
        "t.png",
        doc,
        0,
        &solid(W, H, [200, 40, 40, 255]),
        "Top",
    );
    doc = set_style(doc, 1, BAKED_STYLE);
    let report = export_report(doc);
    let losses = strings(&report, "losses");
    const SENTENCE: &str = "Layer “Top” uses knockout (deep) and Blend Interior Effects as Group, but its style is rasterized for this file, so it is not written: the rasterized pixels already hold the effects a knockout would punch with.";
    assert!(
        losses.iter().any(|text| text == SENTENCE),
        "E2's narrowed loss: {losses:?}"
    );
    assert!(
        !strings(&report, "observations")
            .iter()
            .any(|text| text.contains("written as Photoshop's own")),
        "and never the written-style observation beside it"
    );
    let bytes = write_psd(doc, &dir.path().join("baked.psd"));
    assert!(scan(&bytes, b"knko").is_empty(), "no knko beside a bake");
    assert!(scan(&bytes, b"infx").is_empty(), "no infx beside a bake");
    unsafe { dr_doc_free(doc) };
}

// ------------------------------------------------------------- (e) D13 --

const IMPORT_SENTENCE: &str = "This file has no Photoshop Background layer, and a knockout can reach its bottom layer, which Doctored treats as the Background because it is visible, Normal, 100 % opacity, unmasked, unstyled, canvas-sized and at the origin. Photoshop punches a Deep knockout (or a Shallow one outside every group) through to transparency when a file has no Background layer, so the hole can show the bottom layer here where it shows transparency in Photoshop. This is a deliberate Doctored rule, not an import error.";
const EXPORT_SENTENCE: &str = "A layer uses knockout over this document's bottom layer, which Doctored treats as the Background. A PSD written by Doctored stores it as an ordinary layer, and Photoshop punches a Deep knockout (or a Shallow one outside every group) through to transparency when a file has no Background layer, so the hole can show transparency in Photoshop where it shows the bottom layer here.";

/// The two sentences of one Doctored-written PSD: (export, import).
fn d13(doc: *const DrDocument, dir: &TempDir, name: &str) -> (bool, bool) {
    let exported = strings(&export_report(doc), "losses")
        .iter()
        .any(|text| text == EXPORT_SENTENCE);
    let path = dir.path().join(name);
    write_psd(doc, &path);
    let imported = strings(&import_report(&path), "verified_losses")
        .iter()
        .any(|text| text == IMPORT_SENTENCE);
    (exported, imported)
}

#[test]
fn d13_fires_for_a_qualifying_bottom_under_a_deep_raster_and_for_nothing_else() {
    let dir = TempDir::new().unwrap();

    // PRESENT: a plain canvas-sized bottom under a Deep raster.
    let mut doc = doc_from(&dir, "b.png", &solid(W, H, [20, 40, 60, 255]));
    doc = add_layer(
        &dir,
        "t.png",
        doc,
        0,
        &solid(W, H, [200, 40, 40, 255]),
        "Top",
    );
    doc = set_style(doc, 1, "{\"knockout\":\"deep\"}");
    assert_eq!(
        d13(doc, &dir, "present.psd"),
        (true, true),
        "both sentences fire on a qualifying bottom"
    );

    // ABSENT: a bottom SMALLER than the canvas cannot be the Background. The
    // canvas is grown AFTER the layers are built, so only the rectangles
    // differ from the case above.
    let mut trimmed = doc_from(&dir, "small.png", &solid(W, H, [20, 40, 60, 255]));
    trimmed = add_layer(
        &dir,
        "t2.png",
        trimmed,
        0,
        &solid(W, H, [200, 40, 40, 255]),
        "Top",
    );
    trimmed = set_style(trimmed, 1, "{\"knockout\":\"deep\"}");
    let grown = unsafe { dr_doc_canvas_resize(trimmed, W + 4, H, 0, 0) };
    assert!(!grown.is_null(), "canvas_resize failed");
    unsafe { dr_doc_free(trimmed) };
    let trimmed = grown;
    assert_eq!(d13(trimmed, &dir, "trimmed.psd"), (false, false));
    unsafe { dr_doc_free(trimmed) };

    // ABSENT: a HIDDEN bottom is not the Background either.
    let mut hidden = doc_from(&dir, "b2.png", &solid(W, H, [20, 40, 60, 255]));
    hidden = add_layer(
        &dir,
        "t3.png",
        hidden,
        0,
        &solid(W, H, [200, 40, 40, 255]),
        "Top",
    );
    hidden = set_style(hidden, 1, "{\"knockout\":\"deep\"}");
    let toggled = unsafe { dr_doc_with_layer_visible(hidden, 0, false) };
    assert!(!toggled.is_null());
    unsafe { dr_doc_free(hidden) };
    assert_eq!(d13(toggled, &dir, "hidden.psd"), (false, false));
    unsafe { dr_doc_free(toggled) };

    // ABSENT: a Shallow INSIDE a group never reaches the bottom.
    let shallow = byte_round_trip_shallow_only(&dir);
    assert_eq!(d13(shallow, &dir, "shallow.psd"), (false, false));
    unsafe { dr_doc_free(shallow) };
    unsafe { dr_doc_free(doc) };

    // ABSENT: the only knockout reaching this qualifying bottom is BAKED, so
    // no `knko` is written at all and the file cannot punch through to
    // transparency in Photoshop. The per-layer "it is not written" loss is
    // the whole story there, and D13 must not claim otherwise beside it.
    let mut baked = doc_from(&dir, "b4.png", &solid(W, H, [20, 40, 60, 255]));
    baked = add_layer(
        &dir,
        "t4.png",
        baked,
        0,
        &solid(W, H, [200, 40, 40, 255]),
        "Top",
    );
    baked = set_style(baked, 1, BAKED_STYLE);
    assert!(
        strings(&export_report(baked), "losses")
            .iter()
            .any(|text| text.contains("so it is not written")),
        "the baked style still states that its knockout is not written"
    );
    assert_eq!(d13(baked, &dir, "baked13.psd"), (false, false));
    unsafe { dr_doc_free(baked) };

    // The TYPED depth answers the same way, from its own predicate.
    let typed = typed_document(vec![
        typed_raster("Bottom", None),
        typed_raster("Top", Some("{\"knockout\":\"deep\"}")),
    ]);
    assert!(
        strings(&typed_export_report(&typed), "losses")
            .iter()
            .any(|text| text == EXPORT_SENTENCE),
        "the 16-bit export states D13 too"
    );
    let typed_path = dir.path().join("present16.psd");
    typed_write_psd(&typed, &typed_path);
    assert!(
        strings(&import_report(&typed_path), "verified_losses")
            .iter()
            .any(|text| text == IMPORT_SENTENCE),
        "and the 16-bit file it wrote reports it on the way back in"
    );
    let typed_plain = typed_document(vec![
        typed_raster("Bottom", None),
        typed_raster("Top", None),
    ]);
    assert!(
        !strings(&typed_export_report(&typed_plain), "losses")
            .iter()
            .any(|text| text == EXPORT_SENTENCE),
        "and only when a knockout reaches the bottom"
    );
    // The typed twin of the baked case above: nothing is written, so nothing
    // is disclosed.
    let typed_baked = typed_document(vec![
        typed_raster("Bottom", None),
        typed_raster("Top", Some(BAKED_STYLE)),
    ]);
    assert!(
        !strings(&typed_export_report(&typed_baked), "losses")
            .iter()
            .any(|text| text == EXPORT_SENTENCE),
        "and never for a knockout whose style is rasterized"
    );

    // ABSENT: the committed Photoshop file, whose record 0 IS a true
    // Background — the evidence D13 was decided on.
    let source = family_file("knockout", "source.psd");
    assert!(
        !strings(&import_report(&source), "verified_losses")
            .iter()
            .any(|text| text == IMPORT_SENTENCE),
        "a true Photoshop Background never fires the disclosure"
    );
}

/// `Bottom` with a Shallow knockout raster INSIDE a group: no knockout of
/// this document reaches the bottom entry.
fn byte_round_trip_shallow_only(dir: &TempDir) -> *mut DrDocument {
    let mut doc = doc_from(dir, "sb.png", &solid(W, H, [20, 40, 60, 255]));
    doc = add_layer(
        dir,
        "si.png",
        doc,
        0,
        &solid(W, H, [200, 40, 40, 255]),
        "Inner",
    );
    let indices = [1usize];
    let (mut group, mut cleared, mut cleared_len, mut reordered, mut reordered_len) =
        (0usize, [0usize; 8], 0usize, [0usize; 8], 0usize);
    let name = CString::new("Wrap").unwrap();
    let grouped = unsafe {
        dr_doc_group_layers(
            doc,
            indices.as_ptr(),
            indices.len(),
            name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            8,
        )
    };
    assert!(!grouped.is_null(), "group_layers failed");
    unsafe { dr_doc_free(doc) };
    set_style(grouped, 1, "{\"knockout\":\"shallow\"}")
}

// ------------------------------ (g) P7-39, no clip-base disclosure --

/// Whether a report line is a P7-39 clip-base disclosure, in either of the
/// two wordings 12E-3d shipped. P7-39 lifted D6's narrowing, so Doctored
/// renders the flag ON there as Photoshop does and neither side says a word.
fn clip_disclosure(text: &str) -> bool {
    text.contains("Blend Interior Effects as Group") && text.contains("clipped to it")
}

/// Whether one Doctored-written PSD carries a P7-39 sentence: (export,
/// import).
fn clip_interior(doc: *const DrDocument, dir: &TempDir, name: &str) -> (bool, bool) {
    let exported = strings(&export_report(doc), "losses")
        .iter()
        .any(|text| clip_disclosure(text));
    let path = dir.path().join(name);
    write_psd(doc, &path);
    let imported = strings(&import_report(&path), "verified_losses")
        .iter()
        .any(|text| clip_disclosure(text));
    (exported, imported)
}

/// `Bottom`, a clip BASE carrying `style`, and one raster clipped to it.
/// `visible_member` is the member's visibility.
fn clip_base_document(
    dir: &TempDir,
    tag: &str,
    style: &str,
    visible_member: bool,
    with_member: bool,
) -> *mut DrDocument {
    let mut doc = doc_from(dir, &format!("{tag}b.png"), &solid(W, H, [20, 40, 60, 255]));
    doc = add_layer(
        dir,
        &format!("{tag}base.png"),
        doc,
        0,
        &solid(W, H, [200, 40, 40, 255]),
        "Base",
    );
    doc = set_style(doc, 1, style);
    if !with_member {
        return doc;
    }
    doc = add_layer(
        dir,
        &format!("{tag}member.png"),
        doc,
        1,
        &solid(W, H, [40, 200, 40, 255]),
        "Member",
    );
    let clipped = unsafe { dr_doc_with_layer_clipped(doc, 2, true) };
    assert!(!clipped.is_null(), "with_layer_clipped failed");
    unsafe { dr_doc_free(doc) };
    if visible_member {
        return clipped;
    }
    let hidden = unsafe { dr_doc_with_layer_visible(clipped, 2, false) };
    assert!(!hidden.is_null(), "with_layer_visible failed");
    unsafe { dr_doc_free(clipped) };
    hidden
}

/// A colour overlay with the flag OFF: the same effect, so only the flag
/// differs between the present case and this one.
const OVERLAY_ONLY: &str = "{\"effects\":\
     [{\"type\":\"color_overlay\",\"blend\":\"normal\",\"color\":\"#00ff00\",\"opacity\":1}]}";

#[test]
fn p7_39_no_clip_base_case_discloses_anything_at_either_depth() {
    let dir = TempDir::new().unwrap();
    for (tag, style, visible, with_member) in [
        ("p", INTERIOR_STYLE, true, true),
        ("l", INTERIOR_STYLE, true, false),
        ("h", INTERIOR_STYLE, false, true),
        ("o", OVERLAY_ONLY, true, true),
        ("k", BAKED_STYLE, true, true),
    ] {
        let doc = clip_base_document(&dir, tag, style, visible, with_member);
        assert_eq!(
            clip_interior(doc, &dir, &format!("clip_{tag}.psd")),
            (false, false),
            "{tag}: the flag renders as in Photoshop, so nothing is disclosed"
        );
        unsafe { dr_doc_free(doc) };
    }
    let member = typed_raster("Member", None).with_clipped(true);
    let typed = typed_document(vec![
        typed_raster("Bottom", None),
        typed_raster("Base", Some(INTERIOR_STYLE)),
        member,
    ]);
    assert_eq!(
        typed_clip_interior(&typed, &dir, "clip_present16.psd"),
        (false, false),
        "nor at 16 bits"
    );
}

/// The committed Photoshop render `knockout_bottom/interior_on_clip_base`
/// (12E-3d's observation (a)): `Bottom`, `ClipBase` (Multiply, Blend
/// Interior Effects as Group ON, a green colour overlay) and `ClipMember`
/// clipped to it at (16, 16), built from the family's own layer PNGs at 8
/// and 16 bits and compared with Photoshop's `_8` / `_16` renders. Bound,
/// declared before measuring: max 2, mean 0.5 codes (16 bits divided by 257).
#[test]
fn p7_39_interior_on_a_clip_base_matches_photoshop_at_both_depths() {
    let family =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd_photoshop/knockout_bottom");
    let png = |name: &str| image::open(family.join(name)).unwrap().to_rgba8();
    let (bottom, extra, high) = (png("base.png"), png("extra.png"), png("high.png"));
    let side = bottom.width();
    let style = "{\"blend_interior_as_group\":true,\"effects\":\
         [{\"type\":\"color_overlay\",\"blend\":\"normal\",\"color\":\"#00ff00\",\"opacity\":1}]}";
    let mut member = image::RgbaImage::new(side, side);
    image::imageops::overlay(&mut member, &high, 16, 16);
    let residual = |got: Vec<[f64; 3]>, want: Vec<[f64; 3]>| {
        let diffs: Vec<f64> = got
            .iter()
            .zip(&want)
            .flat_map(|(g, w)| (0..3).map(move |k| (g[k] - w[k]).abs()))
            .collect();
        let max = diffs.iter().copied().fold(0.0, f64::max);
        (max, diffs.iter().sum::<f64>() / diffs.len() as f64)
    };

    let dir = TempDir::new().unwrap();
    let mut doc = doc_from(&dir, "bottom.png", &bottom);
    doc = add_layer(&dir, "extra.png", doc, 0, &extra, "ClipBase");
    let multiply = unsafe { dr_doc_with_layer_blend_mode(doc, 1, 1) };
    assert!(!multiply.is_null());
    unsafe { dr_doc_free(doc) };
    doc = set_style(multiply, 1, style);
    doc = add_layer(&dir, "member.png", doc, 1, &member, "ClipMember");
    let doc = byte_clipped(doc, 2);
    let got8: Vec<[f64; 3]> = common::flat_pixels(doc)
        .chunks(4)
        .map(|p| [p[0], p[1], p[2]].map(f64::from))
        .collect();
    unsafe { dr_doc_free(doc) };
    let want8: Vec<[f64; 3]> = image::open(family.join("interior_on_clip_base_8.png"))
        .unwrap()
        .to_rgb8()
        .pixels()
        .map(|p| p.0.map(f64::from))
        .collect();
    let (max, mean) = residual(got8, want8);
    assert!(max <= 2.0 && mean <= 0.5, "8 bits: max {max}, mean {mean}");

    let promote = |img: &image::RgbaImage| -> PrecisionImage {
        let pixels = img
            .pixels()
            .map(|p| p.0.map(|c| u16::from(c) * 257))
            .collect();
        PrecisionImage::from_rgba16(side, side, pixels, IccProfile::srgb(), None).unwrap()
    };
    let raster = |name: &str, img: &image::RgbaImage| {
        TLayer::raster(name, promote(img), (0, 0), 1.0, true).unwrap()
    };
    let base = raster("ClipBase", &extra)
        .with_layer_style(Some(&LayerStyle::from_json(style).unwrap()), (side, side))
        .unwrap()
        .with_blend_mode(doctored_core::doc::BlendMode::Multiply)
        .unwrap();
    let typed = PrecisionDocument::from_parts(
        side,
        side,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            raster("Bottom", &bottom),
            base,
            raster("ClipMember", &member).with_clipped(true),
        ],
    )
    .unwrap();
    let got16: Vec<[f64; 3]> = typed
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
        .iter()
        .map(|p| [p[0], p[1], p[2]].map(|c| f64::from(c) / 257.0))
        .collect();
    let want16: Vec<[f64; 3]> = image::open(family.join("interior_on_clip_base_16.png"))
        .unwrap()
        .to_rgb16()
        .pixels()
        .map(|p| p.0.map(|c| f64::from(c) / 257.0))
        .collect();
    let (max, mean) = residual(got16, want16);
    assert!(max <= 2.0 && mean <= 0.5, "16 bits: max {max}, mean {mean}");
}

// ----------------------------------------------- (f) what is NOT carried --

const CANVAS: [u32; 2] = [2, 1];
const RGBA: [u8; 8] = [11, 29, 47, 255, 101, 151, 199, 0];

fn raster_record() -> Record {
    Record::raster([0, 0, 1, 2], &RGBA, false)
}

fn report_of(bytes: &[u8], dir: &TempDir, name: &str) -> Value {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    let report = import_report(&path);
    assert_eq!(report["layered_available"], true, "{report}");
    report
}

#[test]
fn unmapped_framings_are_not_carried_and_a_closing_divider_keeps_its_marker() {
    let dir = TempDir::new().unwrap();
    // An unrecognized knockout VALUE, a `knko` of the wrong LENGTH, and an
    // `infx` whose body is not Photoshop's framing: none is carried, each
    // keeps exactly the sentence it had before 12E-3c.
    for (key, body, fragment, name) in [
        (
            b"knko",
            &[3, 0, 0, 0][..],
            "nonzero or unrecognized knockout",
            "knko3.psd",
        ),
        (
            b"knko",
            &[1, 0][..],
            "nonzero or unrecognized knockout",
            "knko2.psd",
        ),
    ] {
        let mut record = raster_record();
        record.blocks = block(key, body);
        let report = report_of(&psd_fixture_bytes(&[record], CANVAS, false), &dir, name);
        assert!(
            strings(&report, "verified_losses")
                .iter()
                .any(|text| text.contains(fragment)),
            "{name}: {report}"
        );
        let doc = open_psd(&dir.path().join(name), &report, LAYERS);
        assert!(
            doc.layers.iter().all(|layer| layer.style.is_none()),
            "{name}: nothing is carried"
        );
    }

    let mut record = raster_record();
    record.blocks = block(b"infx", &[127; 4]);
    let report = report_of(
        &psd_fixture_bytes(&[record], CANVAS, false),
        &dir,
        "infx127.psd",
    );
    assert!(
        strings(&report, "observations")
            .iter()
            .any(|text| text.contains("payload is not interpreted or retained")),
        "the pinned sentence stands for an unmapped infx: {report}"
    );

    // A CLOSING divider's style is attached to nothing, so its `knko` is
    // never offered to the codec and its marker loss stays (objection 1).
    let mut close = Record::folder(3, *b"norm");
    close.blocks = block(b"knko", &[1, 0, 0, 0]);
    let folder = Record::folder(2, *b"norm");
    let report = report_of(
        &psd_fixture_bytes(
            &[raster_record(), close, raster_record(), folder],
            CANVAS,
            false,
        ),
        &dir,
        "divider.psd",
    );
    assert!(
        strings(&report, "verified_losses").iter().any(|text| text
            == "PSD bounding-divider metadata has nonzero or unrecognized knockout metadata; layered import discards these settings without applying them."),
        "the divider keeps its exact sentence: {report}"
    );
}

// ------------- (h) C1 the clip-run question, C2 what "renders" means --
//
// (h1)-(h3) assert the P7-39 pair `(export, import)` at 8 AND 16 bits;
// (h4) the SECOND reader of `ExportStyle.clip_base`, the BAKED-style clause.

/// The flag ON beside a single BELOW effect: nothing GROUPED renders, so C2's
/// question answers no however the clip run reads.
const SHADOW_ONLY_INTERIOR: &str = "{\"blend_interior_as_group\":true,\"effects\":\
     [{\"type\":\"drop_shadow\",\"size\":2}]}";
/// The same flag with the colour overlay PRESENT but DISABLED beside an
/// enabled drop shadow — C2 reads ENABLED effects only.
const DISABLED_OVERLAY_INTERIOR: &str = "{\"blend_interior_as_group\":true,\"effects\":\
     [{\"type\":\"drop_shadow\",\"size\":2},\
      {\"type\":\"color_overlay\",\"blend\":\"normal\",\"color\":\"#00ff00\",\"opacity\":1,\
        \"enabled\":false}]}";
/// The flag with an ENABLED grouped effect that its SETTINGS skip — the
/// renderer emits nothing for any shape, so C2's question answers no.
const ZERO_OVERLAY_INTERIOR: &str = "{\"blend_interior_as_group\":true,\"effects\":\
     [{\"type\":\"drop_shadow\",\"size\":2},\
      {\"type\":\"color_overlay\",\"blend\":\"normal\",\"color\":\"#00ff00\",\"opacity\":0}]}";
const ZERO_GRADIENT_INTERIOR: &str = "{\"blend_interior_as_group\":true,\"effects\":\
     [{\"type\":\"drop_shadow\",\"size\":2},\
      {\"type\":\"gradient_overlay\",\"opacity\":0}]}";
/// The explicit distance keeps the satin from being baked on export.
const ZERO_SATIN_INTERIOR: &str = "{\"blend_interior_as_group\":true,\"effects\":\
     [{\"type\":\"drop_shadow\",\"size\":2},\
      {\"type\":\"satin\",\"opacity\":0,\"distance\":4}]}";
const ZERO_GLOW_INTERIOR: &str = "{\"blend_interior_as_group\":true,\"effects\":\
     [{\"type\":\"drop_shadow\",\"size\":2},\
      {\"type\":\"inner_glow\",\"opacity\":0}]}";
/// An EDGE glow of size 0 draws nothing at any opacity.
const ZERO_SIZE_EDGE_GLOW_INTERIOR: &str = "{\"blend_interior_as_group\":true,\"effects\":\
     [{\"type\":\"drop_shadow\",\"size\":2},\
      {\"type\":\"inner_glow\",\"source\":\"edge\",\"size\":0,\"opacity\":0.75}]}";
/// A live overlay that is DISABLED beside an enabled satin its settings skip.
const DISABLED_LIVE_PLUS_ZERO_INTERIOR: &str = "{\"blend_interior_as_group\":true,\"effects\":\
     [{\"type\":\"drop_shadow\",\"size\":2},\
      {\"type\":\"color_overlay\",\"blend\":\"normal\",\"color\":\"#00ff00\",\"opacity\":1,\
        \"enabled\":false},\
      {\"type\":\"satin\",\"opacity\":0,\"distance\":4}]}";
/// A skipped overlay beside a LIVE inner glow: the glow still moves.
const ZERO_OVERLAY_PLUS_LIVE_GLOW_INTERIOR: &str = "{\"blend_interior_as_group\":true,\"effects\":\
     [{\"type\":\"drop_shadow\",\"size\":2},\
      {\"type\":\"color_overlay\",\"blend\":\"normal\",\"color\":\"#00ff00\",\"opacity\":0},\
      {\"type\":\"inner_glow\",\"opacity\":0.75}]}";
/// The BAKED clause (`psd_export_preflight`, the rasterize sentence), the
/// second and last reader of `ExportStyle.clip_base`.
const BAKED_CLIP_CLAUSE: &str = " Layers clipped to it are written above the rasterized effects.";

/// The re-imported `Base` still carries every one of `kinds`, ENABLED: the
/// skipped effect is written and read back, only its disclosure is gone. A
/// 16-bit file opens only as a typed document, so it reopens through the
/// typed PSD open.
fn reimported_base_carries(path: &Path, kinds: &[EffectKind]) {
    let report = import_report(path);
    let style: LayerStyle = if report["depth"] == 16 {
        let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
        let file = CString::new(path.to_str().unwrap()).unwrap();
        let mut err: *mut c_char = ptr::null_mut();
        let raw = unsafe {
            dr_precision_doc_open_psd(
                file.as_ptr(),
                LAYERS,
                token.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!raw.is_null(), "typed open_psd failed");
        let doc = unsafe { Box::from_raw(raw) };
        let base = doc.layer(typed_index_of(&doc, "Base")).unwrap();
        (**base.style().expect("Base keeps its style")).clone()
    } else {
        let doc = open_psd(path, &report, LAYERS);
        let base = doc
            .layers
            .iter()
            .find(|layer| layer.name == "Base")
            .expect("the re-imported file holds Base");
        (*base.style.clone().expect("Base keeps its style")).clone()
    };
    for &kind in kinds {
        assert!(
            style.effect(kind).is_some_and(|e| e.enabled()),
            "{}: Base carries an enabled {kind:?}",
            path.display()
        );
    }
}

fn byte_group(doc: *mut DrDocument, index: usize, name: &str) -> (*mut DrDocument, usize) {
    let indices = [index];
    let (mut group, mut cleared, mut cleared_len, mut reordered, mut reordered_len) =
        (0usize, [0usize; 8], 0usize, [0usize; 8], 0usize);
    let name = CString::new(name).unwrap();
    let grouped = unsafe {
        dr_doc_group_layers(
            doc,
            indices.as_ptr(),
            indices.len(),
            name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            8,
        )
    };
    assert!(!grouped.is_null(), "group_layers failed");
    unsafe { dr_doc_free(doc) };
    (grouped, group)
}

fn byte_clipped(doc: *mut DrDocument, index: usize) -> *mut DrDocument {
    let out = unsafe { dr_doc_with_layer_clipped(doc, index, true) };
    assert!(!out.is_null(), "with_layer_clipped failed");
    unsafe { dr_doc_free(doc) };
    out
}

fn byte_hidden(doc: *mut DrDocument, index: usize) -> *mut DrDocument {
    let out = unsafe { dr_doc_with_layer_visible(doc, index, false) };
    assert!(!out.is_null(), "with_layer_visible failed");
    unsafe { dr_doc_free(doc) };
    out
}

/// (h1) byte: `Bottom`, then a clipped, visible GROUP holding one child that
/// carries `style`. The child is NOT a clip base — the folder record above it
/// in file order is its own PARENT.
fn byte_child_in_clipped_group(dir: &TempDir, tag: &str, style: &str) -> *mut DrDocument {
    let mut doc = doc_from(dir, &format!("{tag}b.png"), &solid(W, H, [20, 40, 60, 255]));
    doc = add_layer(
        dir,
        &format!("{tag}c.png"),
        doc,
        0,
        &solid(W, H, [200, 40, 40, 255]),
        "Child",
    );
    doc = set_style(doc, 1, style);
    let (doc, group) = byte_group(doc, 1, "G");
    byte_clipped(doc, group)
}

/// (h2)/(h4) byte: `Bottom`, a `Base` carrying `style`, and a GROUP clipped to
/// the base holding one raster. `visible_group` is the FOLDER's visibility.
fn byte_base_under_clipped_group(
    dir: &TempDir,
    tag: &str,
    style: &str,
    visible_group: bool,
) -> *mut DrDocument {
    let mut doc = doc_from(dir, &format!("{tag}b.png"), &solid(W, H, [20, 40, 60, 255]));
    doc = add_layer(
        dir,
        &format!("{tag}base.png"),
        doc,
        0,
        &solid(W, H, [200, 40, 40, 255]),
        "Base",
    );
    doc = set_style(doc, 1, style);
    doc = add_layer(
        dir,
        &format!("{tag}inner.png"),
        doc,
        1,
        &solid(W, H, [40, 200, 40, 255]),
        "Inner",
    );
    let (doc, group) = byte_group(doc, 2, "G");
    let doc = byte_clipped(doc, group);
    if visible_group {
        doc
    } else {
        byte_hidden(doc, group)
    }
}

/// One typed structure edit through the ONE request surface — which is how a
/// black-box test builds a typed GROUP, `PrecisionDocumentLayer::group` being
/// crate-private.
fn typed_edit(document: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    unsafe { Box::from_raw(out) }
}

fn typed_index_of(document: &PrecisionDocument, name: &str) -> usize {
    (0..document.layer_count())
        .find(|&i| document.layer(i).is_some_and(|l| l.name() == name))
        .unwrap_or_else(|| panic!("{name} is present"))
}

fn typed_group_over(document: &PrecisionDocument, layer: usize) -> Box<PrecisionDocument> {
    typed_edit(
        document,
        serde_json::json!({"op": "group_layers", "layers": [layer], "name": "G",
                           "pass_through": false}),
    )
}

fn typed_set_clipped(document: &PrecisionDocument, layer: usize) -> Box<PrecisionDocument> {
    typed_edit(
        document,
        serde_json::json!({"op": "layer_clipped", "layer": layer, "clipped": true}),
    )
}

fn typed_set_hidden(document: &PrecisionDocument, layer: usize) -> Box<PrecisionDocument> {
    typed_edit(
        document,
        serde_json::json!({"op": "layer_visible", "layer": layer, "visible": false}),
    )
}

/// Whether one Doctored-written 16-bit PSD carries a P7-39 sentence:
/// (export, import) — the typed twin of `clip_interior`.
fn typed_clip_interior(doc: &PrecisionDocument, dir: &TempDir, name: &str) -> (bool, bool) {
    let exported = strings(&typed_export_report(doc), "losses")
        .iter()
        .any(|text| clip_disclosure(text));
    let path = dir.path().join(name);
    typed_write_psd(doc, &path);
    let imported = strings(&import_report(&path), "verified_losses")
        .iter()
        .any(|text| clip_disclosure(text));
    (exported, imported)
}

/// (h1) A record inside a CLIPPED FOLDER is not a clip base: the folder record
/// framed directly above it is its own PARENT. The old import walk read that
/// folder as a clipped sibling and disclosed a document that has no clip run
/// at all.
#[test]
fn h1_a_styled_child_of_a_clipped_group_is_not_a_clip_base_at_either_depth() {
    let dir = TempDir::new().unwrap();

    let byte = byte_child_in_clipped_group(&dir, "h1", INTERIOR_STYLE);
    assert_eq!(
        clip_interior(byte, &dir, "h1_child.psd"),
        (false, false),
        "the base's own enclosing folder ends the level, it does not join the run"
    );
    unsafe { dr_doc_free(byte) };

    let typed = typed_document(vec![
        typed_raster("Bottom", None),
        typed_raster("Child", Some(INTERIOR_STYLE)),
    ]);
    let grouped = typed_group_over(&typed, 1);
    let index = typed_index_of(&grouped, "G");
    let clipped = typed_set_clipped(&grouped, index);
    assert_eq!(
        typed_clip_interior(&clipped, &dir, "h1_child16.psd"),
        (false, false),
        "and the 16-bit file it writes says the same on both sides"
    );
}

/// (h2) A clipped, visible GROUP above the base IS a member of the run: the
/// byte export's `depth == depth` guard used to end the run at that group's
/// first CHILD, and the import walk never reached its folder record.
#[test]
fn h2_a_visible_clipped_group_is_a_member_of_the_run_and_a_hidden_one_is_not() {
    let dir = TempDir::new().unwrap();

    let visible = byte_base_under_clipped_group(&dir, "h2v", INTERIOR_STYLE, true);
    assert_eq!(
        clip_interior(visible, &dir, "h2_group.psd"),
        (false, false),
        "a visible clipped group: the flag renders as in Photoshop, nothing disclosed"
    );
    unsafe { dr_doc_free(visible) };

    let hidden = byte_base_under_clipped_group(&dir, "h2h", INTERIOR_STYLE, false);
    assert_eq!(
        clip_interior(hidden, &dir, "h2_group_hidden.psd"),
        (false, false),
        "a hidden clipped group is not a visible member"
    );
    unsafe { dr_doc_free(hidden) };

    let typed = typed_document(vec![
        typed_raster("Bottom", None),
        typed_raster("Base", Some(INTERIOR_STYLE)),
        typed_raster("Inner", None),
    ]);
    let grouped = typed_group_over(&typed, 2);
    let index = typed_index_of(&grouped, "G");
    let clipped = typed_set_clipped(&grouped, index);
    assert_eq!(
        typed_clip_interior(&clipped, &dir, "h2_group16.psd"),
        (false, false),
        "nor at 16 bits"
    );
    let hidden = typed_set_hidden(&clipped, typed_index_of(&clipped, "G"));
    assert_eq!(
        typed_clip_interior(&hidden, &dir, "h2_group_hidden16.psd"),
        (false, false),
        "and a hidden folder discloses nothing at 16 bits either"
    );
}

/// (h3) C2: the flag discloses only when an ENABLED grouped interior effect
/// actually renders — that is, one its settings do not skip. A drop shadow
/// alone is a BELOW effect, a DISABLED colour overlay moves nothing, and an
/// enabled overlay, satin or glow at opacity 0 (or an edge glow of size 0)
/// renders nothing for any shape. The skipped effect itself survives the
/// round trip.
#[test]
fn h3_the_flag_alone_discloses_nothing_when_no_grouped_interior_effect_renders() {
    let dir = TempDir::new().unwrap();
    let rows: [(&str, &str, &str, &[EffectKind]); 8] = [
        ("h3s", SHADOW_ONLY_INTERIOR, "h3_shadow", &[]),
        ("h3d", DISABLED_OVERLAY_INTERIOR, "h3_disabled", &[]),
        (
            "h3zo",
            ZERO_OVERLAY_INTERIOR,
            "h3_zero_overlay",
            &[EffectKind::ColorOverlay],
        ),
        (
            "h3zg",
            ZERO_GRADIENT_INTERIOR,
            "h3_zero_gradient",
            &[EffectKind::GradientOverlay],
        ),
        (
            "h3zs",
            ZERO_SATIN_INTERIOR,
            "h3_zero_satin",
            &[EffectKind::Satin],
        ),
        (
            "h3zi",
            ZERO_GLOW_INTERIOR,
            "h3_zero_glow",
            &[EffectKind::InnerGlow],
        ),
        (
            "h3ze",
            ZERO_SIZE_EDGE_GLOW_INTERIOR,
            "h3_zero_size_edge_glow",
            &[EffectKind::InnerGlow],
        ),
        (
            "h3dz",
            DISABLED_LIVE_PLUS_ZERO_INTERIOR,
            "h3_disabled_plus_zero",
            &[EffectKind::Satin],
        ),
    ];
    for (tag, style, file, carried) in rows {
        let byte = clip_base_document(&dir, tag, style, true, true);
        assert_eq!(
            clip_interior(byte, &dir, &format!("{file}.psd")),
            (false, false),
            "{file}: a flag that moves nothing renders the same in both applications"
        );
        unsafe { dr_doc_free(byte) };

        let typed = typed_document(vec![
            typed_raster("Bottom", None),
            typed_raster("Base", Some(style)),
            typed_raster("Member", None).with_clipped(true),
        ]);
        assert_eq!(
            typed_clip_interior(&typed, &dir, &format!("{file}16.psd")),
            (false, false),
            "{file}: and the same at 16 bits"
        );

        reimported_base_carries(&dir.path().join(format!("{file}.psd")), carried);
        reimported_base_carries(&dir.path().join(format!("{file}16.psd")), carried);
    }
}

/// (h3) A skipped grouped effect does not mask a LIVE one beside it: the
/// inner glow still moves under the flag, so the pair still discloses.
#[test]
fn h3_a_live_grouped_effect_beside_a_skipped_one_still_discloses() {
    let dir = TempDir::new().unwrap();
    let style = ZERO_OVERLAY_PLUS_LIVE_GLOW_INTERIOR;
    let byte = clip_base_document(&dir, "h3m", style, true, true);
    assert_eq!(
        clip_interior(byte, &dir, "h3_mixed.psd"),
        (false, false),
        "the live glow moves under the flag, as in Photoshop: nothing disclosed"
    );
    unsafe { dr_doc_free(byte) };

    let typed = typed_document(vec![
        typed_raster("Bottom", None),
        typed_raster("Base", Some(style)),
        typed_raster("Member", None).with_clipped(true),
    ]);
    assert_eq!(
        typed_clip_interior(&typed, &dir, "h3_mixed16.psd"),
        (false, false),
        "and the same at 16 bits"
    );
}

/// (h4) The SECOND reader of `ExportStyle.clip_base`: a BAKED base under a
/// visible clipped GROUP newly gains the clause at 8 bits (the 16-bit report
/// already said it), and a hidden folder leaves it absent at both depths.
#[test]
fn h4_the_baked_style_clause_follows_the_same_clip_run_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let clause_in = |report: &Value| {
        strings(report, "losses").iter().any(|text| {
            text.contains("rasterized into the layer's pixels") && text.contains(BAKED_CLIP_CLAUSE)
        })
    };

    let visible = byte_base_under_clipped_group(&dir, "h4v", BAKED_STYLE, true);
    assert!(
        clause_in(&export_report(visible)),
        "a baked base under a visible clipped group states the clause"
    );
    unsafe { dr_doc_free(visible) };

    let hidden = byte_base_under_clipped_group(&dir, "h4h", BAKED_STYLE, false);
    assert!(
        !clause_in(&export_report(hidden)),
        "and states nothing with the folder hidden"
    );
    unsafe { dr_doc_free(hidden) };

    let typed = typed_document(vec![
        typed_raster("Bottom", None),
        typed_raster("Base", Some(BAKED_STYLE)),
        typed_raster("Inner", None),
    ]);
    let grouped = typed_group_over(&typed, 2);
    let index = typed_index_of(&grouped, "G");
    let clipped = typed_set_clipped(&grouped, index);
    assert!(
        clause_in(&typed_export_report(&clipped)),
        "the 16-bit report said it already and still does"
    );
    let hidden = typed_set_hidden(&clipped, typed_index_of(&clipped, "G"));
    assert!(
        !clause_in(&typed_export_report(&hidden)),
        "absent at 16 bits with the folder hidden"
    );
}
