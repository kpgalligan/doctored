//! Layer comps on a CMYK document (#1, `cmyk_comps`), through the FFI. The
//! oracles are the document's own earlier state and records, and for a
//! carried style the style the LAYER ITSELF carries through the same
//! conversion (a twin converted with the style still on the layer); never an
//! image. Every comparison is exact, declared before measuring.
//!
//! - A comp saved on a CMYK document (from RGB at 8 and 16 bits) and applied
//!   after hiding, re-blending (Hue), fading and moving a layer and setting a
//!   Pass Through group to Normal restores every record's visibility,
//!   opacity, blend mode and offset and the flattened ink; a second apply is
//!   the no-op; `shift` moves the recorded offset; update, reorder and delete
//!   change the names as asked; `.doctored` keeps the comps.
//! - Canvas Size and a stroke past a raster's left edge keep the recorded
//!   offset on the layer's own, so Apply after them is the no-op.
//! - RGB → CMYK (8 and 16; from Gray and Lab too) carries every comp; Apply
//!   sets the recorded Hue and the style the layer would have carried, and
//!   skips a recorded Blend If on Red on an empty group; a flattening
//!   conversion carries none.
//! - CMYK → RGB carries every comp, each recorded style without its inks.
//! - Convert to Profile and Assign rewrite a recorded ink as the layer's own,
//!   so Apply after them is the no-op.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::{
    dr_precision_doc_convert_cmyk_to_rgb, dr_precision_doc_convert_to_cmyk,
};
use doctored_core::ffi_cmyk_colour::dr_precision_doc_cmyk_profile_edit;
use doctored_core::ffi_comps::dr_precision_doc_layer_comps_json;
use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_lab::dr_precision_doc_convert_to_lab;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::{press_profile, press_profile_b};
use crate::cmyk_structure_fixture::{
    free, open, save, then, try_edit, varied, with_opacity, LIMIT,
};
use crate::cmyk_style_fixture::{flat, free_all};
use crate::common::take_err_string;

const W: u32 = 12;
const H: u32 = 9;

/// Base (the canvas), Mid (6 × 4 at (2, 1)) and Top (5 × 4 at (4, 3)): a
/// 16-bit sRGB document, every layer Normal, opaque and visible.
fn rgb_doc() -> *mut PrecisionDocument {
    let layer = |name: &str, (w, h): (u32, u32), offset: (i32, i32), seed: u32| {
        let image = PrecisionImage::from_rgba16(w, h, varied(w, h, seed), IccProfile::srgb(), None)
            .unwrap();
        Layer::raster(name, image, offset, 1.0, true).unwrap()
    };
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            layer("Base", (W, H), (0, 0), 1),
            layer("Mid", (6, 4), (2, 1), 2),
            layer("Top", (5, 4), (4, 3), 3),
        ],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

fn to_cmyk(doc: *const PrecisionDocument, options: &str) -> *mut PrecisionDocument {
    let profile = press_profile("Press Test");
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take_err_string(err));
    out
}

fn to_rgb(doc: *const PrecisionDocument, options: &str) -> *mut PrecisionDocument {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_cmyk_to_rgb(
            doc,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to RGB: {}", take_err_string(err));
    out
}

fn profile_edit(doc: *const PrecisionDocument, icc: &[u8], op: &str) -> *mut PrecisionDocument {
    let request = CString::new(json!({ "op": op }).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_profile_edit(
            doc,
            icc.as_ptr(),
            icc.len(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{op}: {}", take_err_string(err));
    out
}

/// An edit that must change the document; the input is KEPT.
fn must(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    let out = try_edit(doc, &request.to_string(), &[]).unwrap_or_else(|e| panic!("{request}: {e}"));
    assert!(!out.is_null(), "{request} changed nothing");
    out
}

fn comp(action: &str, fields: Value) -> Value {
    let mut request = json!({"op":"layer_comp","action":action});
    for (key, value) in fields.as_object().unwrap() {
        request[key] = value.clone();
    }
    request
}

fn comps(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_layer_comps_json(doc, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn names(doc: *const PrecisionDocument) -> Vec<String> {
    comps(doc)["comps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect()
}

/// Comp `index`'s entry for the record named `name`.
fn entry(doc: *const PrecisionDocument, index: usize, name: &str) -> Value {
    comps(doc)["comps"][index]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == name)
        .unwrap_or_else(|| panic!("no entry {name}"))
        .clone()
}

/// The postorder index of the record named `name`.
fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

fn offset(doc: *const PrecisionDocument, name: &str) -> (i32, i32) {
    unsafe { &*doc }.layer(index(doc, name)).unwrap().offset()
}

fn mode(doc: *const PrecisionDocument, name: &str) -> i32 {
    unsafe { &*doc }
        .layer(index(doc, name))
        .unwrap()
        .blend_mode()
        .to_c()
}

/// The record's reported style, parsed (`Null` for none).
fn style(doc: *const PrecisionDocument, name: &str) -> Value {
    unsafe { &*doc }
        .layer(index(doc, name))
        .unwrap()
        .reported_style()
        .map_or(Value::Null, |s| serde_json::from_str(&s).unwrap())
}

/// A comp entry's recorded style, parsed.
fn recorded_style(entry: &Value) -> Value {
    serde_json::from_str(entry["style"].as_str().expect("a recorded style")).unwrap()
}

/// One record's name, visibility, opacity, blend mode and offset.
type Properties = (String, bool, f32, i32, (i32, i32));

/// Each record's [`Properties`].
fn properties(doc: *const PrecisionDocument) -> Vec<Properties> {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .map(|i| {
            let l = doc.layer(i).unwrap();
            let named = l.name().to_string();
            (
                named,
                l.visible(),
                l.opacity(),
                l.blend_mode().to_c(),
                l.offset(),
            )
        })
        .collect()
}

fn blend(name: &str, doc: *const PrecisionDocument, mode: BlendMode) -> Value {
    json!({"op":"layer_blend","version":1,"layer":index(doc, name),"blend_mode":mode.to_c()})
}

#[test]
fn a_comp_saved_on_cmyk_restores_every_record_and_the_ink() {
    let dir = TempDir::new().unwrap();
    for depth in [8, 16] {
        let rgb = rgb_doc();
        let grouped = must(
            rgb,
            &json!({"op":"group_layers","layers":[1],"name":"G","pass_through":true}),
        );
        let cmyk = to_cmyk(grouped, &format!(r#"{{"depth":{depth}}}"#));
        assert!(unsafe { &*cmyk }.is_cmyk());
        assert_eq!(mode(cmyk, "G"), BlendMode::PassThrough.to_c());
        let saved = must(cmyk, &comp("save", json!({"name":"A"})));
        let before = properties(saved);
        let ink = flat(saved);
        assert_eq!(entry(saved, 0, "Top")["offset"], json!([4, 3]), "{depth}");
        let top = index(saved, "Top");
        let changed = must(
            saved,
            &json!({"op":"layer_visible","layer":top,"visible":false}),
        );
        let changed = then(
            changed,
            &blend("Top", saved, BlendMode::Hue).to_string(),
            &[],
        );
        let changed = with_opacity(changed, top, 0.5);
        let moved = json!({"op":"affine","layer":top,"target":"pixels",
            "matrix":[1,0,0,1,2,1],"filter":"nearest"});
        let changed = then(changed, &moved.to_string(), &[]);
        let changed = then(
            changed,
            &blend("G", saved, BlendMode::Normal).to_string(),
            &[],
        );
        let now = properties(changed);
        let t = top;
        assert!(
            now[t].1 != before[t].1
                && now[t].2 != before[t].2
                && now[t].3 != before[t].3
                && now[t].4 != before[t].4,
            "{depth}: every property of Top moved: {:?} from {:?}",
            now[t],
            before[t]
        );
        assert_eq!(mode(changed, "G"), BlendMode::Normal.to_c());
        let applied = must(changed, &comp("apply", json!({"comp":0})));
        assert!(unsafe { &*applied }.is_cmyk());
        assert_eq!(properties(applied), before, "{depth}");
        assert_eq!(flat(applied), ink, "{depth}: the flattened ink");
        assert!(
            try_edit(applied, &comp("apply", json!({"comp":0})).to_string(), &[])
                .unwrap()
                .is_null(),
            "{depth}: a second apply is the no-op"
        );
        let shifted = must(applied, &comp("shift", json!({"layer":top,"dx":3,"dy":-1})));
        assert_eq!(entry(shifted, 0, "Top")["offset"], json!([7, 2]));
        let two = must(applied, &comp("save", json!({"name":"B"})));
        assert_eq!(names(two), ["A", "B"]);
        let renamed = must(two, &comp("update", json!({"comp":0,"name":"A2"})));
        assert_eq!(names(renamed), ["A2", "B"]);
        let reordered = must(renamed, &comp("reorder", json!({"comp":1,"to":0})));
        assert_eq!(names(reordered), ["B", "A2"]);
        let deleted = must(reordered, &comp("delete", json!({"comp":0})));
        assert_eq!(names(deleted), ["A2"]);
        let file = dir.path().join(format!("comps-{depth}.doctored"));
        save(deleted, &file);
        let back = open(&file).unwrap();
        assert!(unsafe { &*back }.is_cmyk());
        assert_eq!(comps(back), comps(deleted), "{depth}: .doctored");
        assert_eq!(entry(back, 0, "Top")["offset"], json!([4, 3]));
        free_all(&[
            back, deleted, reordered, renamed, two, shifted, applied, changed, saved, cmyk,
            grouped, rgb,
        ]);
    }
}

#[test]
fn recorded_offsets_follow_canvas_size_and_a_stroke_past_the_left_edge() {
    let rgb = rgb_doc();
    let cmyk = to_cmyk(rgb, "{}");
    let saved = must(
        cmyk,
        &comp(
            "save",
            json!({"name":"P","visibility":false,"position":true,"appearance":false}),
        ),
    );
    let sized = must(
        saved,
        &json!({"op":"canvas_size","width":W + 6,"height":H + 4,"x":3,"y":2}),
    );
    assert_eq!(offset(sized, "Top"), (7, 5), "the canvas moved under Top");
    assert_eq!(entry(sized, 0, "Top")["offset"], json!([7, 5]));
    let apply = comp("apply", json!({"comp":0})).to_string();
    assert!(
        try_edit(sized, &apply, &[]).unwrap().is_null(),
        "canvas size"
    );
    // Ink painted at canvas x 1…3 grows Top (left edge 7) to the left.
    let (w, h) = unsafe { &*sized }.dimensions();
    let coverage: Vec<u16> = (0..w * h)
        .map(|p| {
            let (x, y) = (p % w, p / w);
            if (1..4).contains(&x) && (6..9).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect();
    let paint = json!({"op":"paint_blend","layer":index(sized, "Top"),
        "ink":[0.2,0.4,0.6,0.1,1],"opacity":1});
    let grown = try_edit(sized, &paint.to_string(), &coverage).unwrap();
    assert!(!grown.is_null());
    assert_eq!(offset(grown, "Top"), (1, 5), "the stroke grew Top leftward");
    assert_eq!(entry(grown, 0, "Top")["offset"], json!([1, 5]));
    assert!(try_edit(grown, &apply, &[]).unwrap().is_null(), "growth");
    free_all(&[grown, sized, saved, cmyk, rgb]);
}

/// A red drop shadow (sRGB, no ink).
fn red_shadow(doc: *const PrecisionDocument, name: &str) -> Value {
    json!({"op":"layer_style","version":1,"layer":index(doc, name),"style":{"effects":[
        {"type":"drop_shadow","color":"#ff0000","distance":2,"size":1}]}})
}

#[test]
fn rgb_to_cmyk_carries_every_comp_and_apply_skips_only_what_cmyk_cannot_hold() {
    let rgb = rgb_doc();
    // An empty group "E": Mid grouped, then removed.
    let grouped = must(rgb, &json!({"op":"group_layers","layers":[1],"name":"E"}));
    let emptied = must(
        grouped,
        &json!({"op":"remove_layers","layers":[index(grouped, "Mid")]}),
    );
    let e = index(emptied, "E");
    assert_eq!(
        unsafe { &*emptied }
            .layer(e)
            .unwrap()
            .children()
            .map(<[_]>::len),
        Some(0)
    );
    let styled = must(emptied, &red_shadow(emptied, "Top"));
    let hue = must(styled, &blend("Top", styled, BlendMode::Hue));
    let red_if = must(
        hue,
        &json!({"op":"layer_blend_if","version":1,"layer":e,
            "blend_if":{"channel":"red","this_layer":[20,40,220,240],"underlying":[0,0,255,255]}}),
    );
    let saved = must(
        red_if,
        &comp("save", json!({"name":"Look","comment":"red"})),
    );
    assert!(entry(saved, 0, "E")["style"]
        .as_str()
        .unwrap()
        .contains("red"));
    // Back to a document CMYK holds as it is: Normal, no style, no Blend If.
    let plain = must(saved, &blend("Top", saved, BlendMode::Normal));
    let plain = then(
        plain,
        &json!({"op":"layer_style","version":1,"layer":index(saved, "Top"),"style":null})
            .to_string(),
        &[],
    );
    let plain = then(
        plain,
        &json!({"op":"layer_blend_if","version":1,"layer":e,"blend_if":null}).to_string(),
        &[],
    );
    for depth in [8, 16] {
        let options = format!(r#"{{"depth":{depth}}}"#);
        let twin = to_cmyk(styled, &options);
        let cmyk = to_cmyk(plain, &options);
        assert_eq!(
            comps(cmyk)["comps"],
            comps(plain)["comps"],
            "{depth}: carried"
        );
        let applied = must(cmyk, &comp("apply", json!({"comp":0})));
        assert!(unsafe { &*applied }.is_cmyk());
        assert_eq!(
            mode(applied, "Top"),
            BlendMode::Hue.to_c(),
            "{depth}: Hue is applied"
        );
        assert_eq!(
            style(applied, "Top"),
            style(twin, "Top"),
            "{depth}: the carried style"
        );
        assert_ne!(style(applied, "Top"), Value::Null);
        assert!(
            unsafe { &*applied }
                .layer(index(applied, "E"))
                .unwrap()
                .blend_if()
                .is_none(),
            "{depth}: the Red Blend If is skipped"
        );
        free_all(&[applied, cmyk, twin]);
    }
    // A flattening conversion (the Red Blend If is a blocker) carries none.
    let flattened = to_cmyk(saved, r#"{"flatten":true}"#);
    assert_eq!(comps(saved)["count"], 1);
    assert_eq!(comps(flattened)["count"], 0);
    // Gray and Lab sources carry the comp through the same body.
    let gray = {
        let options = CString::new(r#"{"space":"gray_gamma_22","depth":16}"#).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_convert_to_gray(
                plain,
                ptr::null(),
                0,
                options.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(!out.is_null(), "to Gray: {}", take_err_string(err));
        out
    };
    let lab = {
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_convert_to_lab(plain, c"{}".as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        };
        assert!(!out.is_null(), "to Lab: {}", take_err_string(err));
        out
    };
    let from_gray = to_cmyk(gray, "{}");
    let from_lab = to_cmyk(lab, "{}");
    assert_eq!(names(from_gray), ["Look"], "from Gray");
    assert_eq!(names(from_lab), ["Look"], "from Lab");
    free_all(&[
        from_lab, from_gray, lab, gray, flattened, plain, saved, red_if, hue, styled, emptied,
        grouped, rgb,
    ]);
}

/// The CMYK document with an inked drop shadow on Top.
fn inked() -> *mut PrecisionDocument {
    let rgb = rgb_doc();
    let cmyk = to_cmyk(rgb, "{}");
    free(rgb);
    let style = json!({"op":"layer_style","version":1,"layer":index(cmyk, "Top"),"style":{
        "effects":[{"type":"drop_shadow","ink":[0.1,0.6,0.3,0.0],"distance":2,"size":1}]}});
    then(cmyk, &style.to_string(), &[])
}

#[test]
fn cmyk_to_rgb_carries_every_comp_without_its_inks() {
    let doc = inked();
    assert!(style(doc, "Top").to_string().contains("\"ink\""));
    // An empty group "E": Mid grouped, then removed.
    let grouped = must(
        doc,
        &json!({"op":"group_layers","layers":[index(doc, "Mid")],"name":"E"}),
    );
    let emptied = must(
        grouped,
        &json!({"op":"remove_layers","layers":[index(grouped, "Mid")]}),
    );
    let e = index(emptied, "E");
    let twin = to_rgb(emptied, "{}");
    let cyan = must(
        emptied,
        &json!({"op":"layer_blend_if","version":1,"layer":e,
            "blend_if":{"channel":"cyan","this_layer":[20,40,220,240],"underlying":[0,0,255,255]}}),
    );
    let saved = must(cyan, &comp("save", json!({"name":"Ink"})));
    let plain = must(
        saved,
        &json!({"op":"layer_style","version":1,"layer":index(saved, "Top"),"style":null}),
    );
    let plain = then(
        plain,
        &json!({"op":"layer_blend_if","version":1,"layer":e,"blend_if":null}).to_string(),
        &[],
    );
    let rgb = to_rgb(plain, "{}");
    assert!(!unsafe { &*rgb }.is_cmyk());
    assert_eq!(names(rgb), ["Ink"]);
    let recorded = recorded_style(&entry(rgb, 0, "Top"));
    assert!(!recorded.to_string().contains("\"ink\""), "{recorded}");
    assert_eq!(recorded, style(twin, "Top"), "the layer's own rule");
    assert!(entry(rgb, 0, "E")["style"]
        .as_str()
        .unwrap()
        .contains("cyan"));
    let applied = must(rgb, &comp("apply", json!({"comp":0})));
    assert_eq!(style(applied, "Top"), style(twin, "Top"));
    assert!(
        unsafe { &*applied }
            .layer(index(applied, "E"))
            .unwrap()
            .blend_if()
            .is_none(),
        "the Cyan Blend If is skipped on RGB"
    );
    // A flattening conversion carries none.
    let flattened = to_rgb(saved, r#"{"flatten":true}"#);
    assert_eq!(comps(flattened)["count"], 0);
    free_all(&[
        flattened, applied, rgb, plain, saved, cyan, twin, emptied, grouped, doc,
    ]);
}

#[test]
fn convert_and_assign_to_profile_rewrite_a_recorded_ink_as_the_layers_own() {
    let doc = inked();
    let saved = must(doc, &comp("save", json!({"name":"Ink"})));
    let before = recorded_style(&entry(saved, 0, "Top"));
    let b = press_profile_b("B");
    for op in ["convert", "assign"] {
        let next = profile_edit(saved, &b, op);
        let recorded = recorded_style(&entry(next, 0, "Top"));
        assert_ne!(recorded, before, "{op}: the profiles differ");
        assert_eq!(
            recorded,
            style(next, "Top"),
            "{op}: the layer's own rewrite"
        );
        assert!(
            try_edit(next, &comp("apply", json!({"comp":0})).to_string(), &[])
                .unwrap()
                .is_null(),
            "{op}: apply is the no-op"
        );
        free(next);
    }
    free_all(&[saved, doc]);
}
