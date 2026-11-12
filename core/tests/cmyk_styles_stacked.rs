//! On a CMYK document's ink, STACKED effect instances each
//! print their OWN ink in render order (the later on top), a disabled
//! instance takes no ink slot, the `.rz` rung 88 (above 87 in the
//! `cmyk_native` ladder) is written for a stacked style on a record or held
//! only by a comp and its claim refused both ways, and a layered CMYK PSD
//! carries the instances back through `lmfx`.
//!
//! Oracles: `common/cmyk_structure_fixture.rs`'s f64 W3C step (`over`, never
//! the implementation) on hard edges, probed on the mid-lines, so a stroke of
//! size `s` covers the pixels whose axis distance to the rect is `1..=s` in
//! full. Bound: 2 16-bit codes a plate.

use std::path::Path;
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::doc_comps::CompRequest;
use doctored_core::ffi_psd::dr_precision_doc_save_psd;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_psd_import::open_psd;
use crate::cmyk_structure_fixture::{cmyk_word, free, open, save, Ink, LIMIT};
use crate::cmyk_style_fixture::{
    assert_paper, backdrop, flat, hard_rect, layer, over_to, percent, styled,
};
use crate::common::{cpath, take_err_string};

const W: u32 = 48;
const H: u32 = 40;
/// x 12..36, y 10..30.
const RECT: (u32, u32, u32, u32) = (12, 10, 36, 30);
const MX: u32 = 24;
const MY: u32 = 20;
const PAPER: [[f64; 4]; 1] = [[0.0, 0.0, 0.0, 0.0]];
const SOURCE: [f64; 4] = [70.0, 0.0, 90.0, 0.0];
const RED: [f64; 4] = [0.0, 100.0, 100.0, 0.0];
const BLUE: [f64; 4] = [100.0, 100.0, 0.0, 0.0];
/// The words, spelled out here rather than imported.
const STACKED_RUNG: u32 = 88;
const EFFECTS_RUNG: u32 = 86;

fn paper() -> Ink {
    [0.0, 0.0, 0.0, 0.0, 1.0]
}

fn source() -> Ink {
    over_to(paper(), percent(SOURCE), 1.0, BlendMode::Normal)
}

fn shaped() -> *mut PrecisionDocument {
    layer(
        backdrop(W, H, 16, &PAPER),
        "Shape",
        SOURCE,
        &hard_rect(W, H, RECT),
    )
}

fn ink(i: [f64; 4]) -> Value {
    json!(percent(i))
}

fn overlays() -> Value {
    json!({"effects":[
        {"type":"color_overlay","ink":ink(RED),"opacity":1},
        {"type":"color_overlay","ink":ink(BLUE),"opacity":0.5}]})
}

fn strokes() -> Value {
    json!({"effects":[
        {"type":"stroke","position":"outside","size":4,"ink":ink(RED)},
        {"type":"stroke","position":"outside","size":2,"ink":ink(BLUE)}]})
}

fn at(samples: &[[u16; 5]], x: u32, y: u32) -> [u16; 5] {
    samples[(y * W + x) as usize]
}

#[test]
fn stacked_colour_overlays_print_each_instances_own_ink_in_order() {
    let doc = styled(shaped(), 1, &overlays());
    let samples = flat(doc);
    let red = over_to(source(), percent(RED), 1.0, BlendMode::Normal);
    let want = over_to(red, percent(BLUE), 0.5, BlendMode::Normal);
    for (x, y) in [(MX, MY), (RECT.0, RECT.1), (RECT.2 - 1, RECT.3 - 1)] {
        assert_paper(at(&samples, x, y), want, 2, &format!("({x}, {y})"));
    }
    assert_paper(at(&samples, MX, RECT.1 - 1), paper(), 2, "outside");
    // Not instance 0's ink twice (`solid_slot` by kind alone).
    let first_twice = over_to(red, percent(RED), 0.5, BlendMode::Normal);
    let got = at(&samples, MX, MY);
    let cyan = crate::cmyk_structure_fixture::code(first_twice[0]);
    assert!(
        got[0].abs_diff(cyan) > 1000,
        "each instance its own ink: {got:?}"
    );
    free(doc);
}

#[test]
fn stacked_outside_strokes_print_the_later_on_top() {
    let doc = styled(shaped(), 1, &strokes());
    let samples = flat(doc);
    let red = over_to(paper(), percent(RED), 1.0, BlendMode::Normal);
    let blue = over_to(paper(), percent(BLUE), 1.0, BlendMode::Normal);
    for d in 1..=6u32 {
        let want = match d {
            1..=2 => blue,
            3..=4 => red,
            _ => paper(),
        };
        for (x, y) in [
            (MX, RECT.1 - d),
            (MX, RECT.3 - 1 + d),
            (RECT.0 - d, MY),
            (RECT.2 - 1 + d, MY),
        ] {
            assert_paper(at(&samples, x, y), want, 2, &format!("({x}, {y}) d {d}"));
        }
    }
    assert_paper(at(&samples, MX, MY), source(), 2, "the interior");
    free(doc);
}

#[test]
fn a_disabled_instance_takes_no_ink_slot() {
    let style = json!({"effects":[
        {"type":"color_overlay","ink":ink(RED),"opacity":1,"enabled":false},
        {"type":"color_overlay","ink":ink(BLUE),"opacity":0.5}]});
    let doc = styled(shaped(), 1, &style);
    let want = over_to(source(), percent(BLUE), 0.5, BlendMode::Normal);
    assert_paper(
        at(&flat(doc), MX, MY),
        want,
        2,
        "the enabled instance's ink",
    );
    free(doc);
}

// ------------------------------------------------------------- rung 88 --

fn reported(doc: *const PrecisionDocument) -> Option<String> {
    unsafe { &*doc }.layer(1).unwrap().reported_style()
}

fn with_word(bytes: &[u8], word: u32) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out[8..12].copy_from_slice(&word.to_le_bytes());
    out
}

fn open_bytes(dir: &TempDir, name: &str, bytes: &[u8]) -> Result<*mut PrecisionDocument, String> {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    open(&path)
}

#[test]
fn a_stacked_style_writes_rung_88_and_reopens_with_its_instances() {
    let dir = TempDir::new().unwrap();
    let single = styled(
        shaped(),
        1,
        &json!({"effects":[{"type":"color_overlay","ink":ink(RED)}]}),
    );
    let single_bytes = save(single, &dir.path().join("single.rz"));
    assert_eq!(
        cmyk_word(&single_bytes),
        EFFECTS_RUNG,
        "one instance stays 86"
    );

    let doc = styled(shaped(), 1, &overlays());
    let bytes = save(doc, &dir.path().join("stacked.rz"));
    assert_eq!(cmyk_word(&bytes), STACKED_RUNG);
    assert_ne!(
        u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
        89,
        "the RGB word is never written beside it"
    );
    let back = open_bytes(&dir, "reopen.rz", &bytes).expect("reopen");
    assert_eq!(reported(back), reported(doc), "the instances survive");
    assert_eq!(flat(back), flat(doc), "and so does the picture");
    assert_eq!(
        save(back, &dir.path().join("again.rz")),
        bytes,
        "resave identical"
    );

    // A pre-88 rung holding a stacked style refuses; 88 holding none too.
    let message = open_bytes(&dir, "86.rz", &with_word(&bytes, EFFECTS_RUNG))
        .expect_err("86 with a stacked style must refuse");
    assert!(
        message.ends_with("a CMYK stacked layer effect requires DRDC 88"),
        "{message}"
    );
    let message = open_bytes(&dir, "forged.rz", &with_word(&single_bytes, STACKED_RUNG))
        .expect_err("88 with no stacked style must refuse");
    assert!(
        message.ends_with("invalid native CMYK stacked effects word"),
        "{message}"
    );
    for d in [single, doc, back] {
        free(d);
    }
}

#[test]
fn a_stacked_style_held_only_by_a_comp_writes_and_claims_88() {
    let dir = TempDir::new().unwrap();
    let doc = styled(shaped(), 1, &overlays());
    let request = CompRequest::parse(
        &json!({"op":"layer_comp","action":"save","name":"Stacked","appearance":true}).to_string(),
    )
    .unwrap();
    let with_comp = unsafe { &*doc }
        .edit_layer_comps(&request)
        .unwrap()
        .expect("a comp");
    let with_comp = Box::into_raw(Box::new(with_comp));
    let single = styled(
        with_comp,
        1,
        &json!({"effects":[{"type":"color_overlay","ink":ink(RED)}]}),
    );
    let bytes = save(single, &dir.path().join("comp.rz"));
    assert_eq!(cmyk_word(&bytes), STACKED_RUNG, "the comp claims it");
    let back = open_bytes(&dir, "reopen.rz", &bytes).expect("reopen");
    assert_eq!(unsafe { &*back }.layer_comps().len(), 1);
    let message = open_bytes(&dir, "86.rz", &with_word(&bytes, EFFECTS_RUNG))
        .expect_err("a comp's stacked style under 86 must refuse");
    assert!(
        message.ends_with("a CMYK stacked layer effect requires DRDC 88"),
        "{message}"
    );
    for d in [doc, single, back] {
        free(d);
    }
}

// ----------------------------------------------------------------- PSD --

fn save_psd(doc: *const PrecisionDocument, path: &Path) {
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
}

fn effects_of(doc: *const PrecisionDocument, name: &str) -> Value {
    let doc = unsafe { &*doc };
    let index = (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"));
    let style: Value =
        serde_json::from_str(&doc.layer(index).unwrap().reported_style().unwrap()).unwrap();
    style["effects"].clone()
}

#[test]
fn a_stacked_cmyk_style_round_trips_through_its_layered_psd() {
    let dir = TempDir::new().unwrap();
    let mut style = strokes();
    let overlay = overlays();
    for e in overlay["effects"].as_array().unwrap() {
        style["effects"].as_array_mut().unwrap().push(e.clone());
    }
    let doc = styled(shaped(), 1, &style);
    let path = dir.path().join("stacked.psd");
    save_psd(doc, &path);
    let back = open_psd(&path, 0).expect("reopen the PSD");
    assert_eq!(effects_of(back, "Shape"), effects_of(doc, "Shape"));
    free(back);
    free(doc);
}
