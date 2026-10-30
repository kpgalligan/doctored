//! Pattern Overlay at 16 bits: the typed render against the
//! byte render (within 257 × the byte bound) and the typed `.rz` word 90 both
//! ways. 32-bit float and CMYK are `float_pattern_overlay` and
//! `cmyk_pattern_overlay`.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::{
    dr_job_control_create, dr_job_control_free, dr_job_control_set_scratch_limit,
};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{rect_layer_doc, styled, take_err_string};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CANVAS: (u32, u32) = (96, 64);
const RECT: (i32, i32, u32, u32) = (13, 9, 50, 40);
const GREEN: [u8; 4] = [0x30, 0xa0, 0x50, 255];

fn edit(doc: &PrecisionDocument, request: &Value) -> Result<Option<PrecisionDocument>, String> {
    let c = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        return if err.is_null() {
            Ok(None)
        } else {
            Err(take_err_string(err))
        };
    }
    Ok(Some(*unsafe { Box::from_raw(out) }))
}

fn set_style(doc: &PrecisionDocument, style: &Value) -> Result<PrecisionDocument, String> {
    edit(
        doc,
        &json!({"op":"layer_style","version":1,"layer":1,"style":style}),
    )
    .map(|d| d.expect("the style changes the document"))
}

/// `set_style` the way the host runs it (`performingPrecisionPixelEdit`):
/// plan, a job control whose scratch limit is the plan's `scratch_bytes`,
/// then the edit under that control. The request escapes every `/` (valid
/// JSON, and Foundation's default), so a base64 tile's request is longer
/// than the style's canonical body: the parse charge, not the style's, is
/// the larger figure, and the plan must carry it.
fn hosted_set_style(doc: &PrecisionDocument, style: &Value) -> PrecisionDocument {
    let request = json!({"op":"layer_style","version":1,"layer":1,"style":style});
    let c = CString::new(request.to_string().replace('/', "\\/")).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(doc, c.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!plan.is_null(), "plan: {}", take_err_string(err));
    let text = unsafe { std::ffi::CStr::from_ptr(plan) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { dr_string_free(plan) };
    let plan: Value = serde_json::from_str(&text).unwrap();
    let scratch = plan["scratch_bytes"].as_u64().unwrap();
    let control = unsafe { dr_job_control_create() };
    assert!(unsafe { dr_job_control_set_scratch_limit(control, scratch) });
    let out = unsafe {
        dr_precision_doc_edit(doc, c.as_ptr(), ptr::null(), 0, LIMIT, 0, control, &mut err)
    };
    unsafe { dr_job_control_free(control) };
    assert!(!out.is_null(), "edit: {}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

/// A 512 x 512 custom tile (1 MiB of texels, a base64 body past 1 MiB).
fn big_tile() -> Value {
    let mut rgba = Vec::with_capacity(512 * 512 * 4);
    for y in 0..512u32 {
        for x in 0..512u32 {
            rgba.extend_from_slice(&[(x % 251) as u8, (y % 241) as u8, ((x ^ y) % 239) as u8, 255]);
        }
    }
    json!({"name":"Big","width":512,"height":512,"rgba":base64(&rgba)})
}

/// The 12 x 8 take1 tile T as the custom pattern object.
fn tile_t() -> Value {
    let mut rgba = Vec::new();
    for y in 0..8u32 {
        for x in 0..12u32 {
            rgba.extend_from_slice(&[
                (20 * x + 10) as u8,
                (30 * y + 15) as u8,
                if (x + y) % 2 == 0 { 200 } else { 60 },
                255,
            ]);
        }
    }
    json!({"name":"T","width":12,"height":8,"rgba":base64(&rgba)})
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

fn style(extra: Value) -> Value {
    let mut effect = json!({"type":"pattern_overlay","pattern":tile_t()});
    for (k, v) in extra.as_object().unwrap() {
        effect[k] = v.clone();
    }
    json!({"effects":[effect]})
}

/// The byte fixture's typed twin: a white canvas under the green rect.
fn typed_doc() -> PrecisionDocument {
    let image = |w: u32, h: u32, px: [u8; 4]| {
        PrecisionImage::from_rgba16(
            w,
            h,
            vec![px.map(|v| u16::from(v) * 257); (w * h) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap()
    };
    let base = Layer::raster(
        "Base",
        image(CANVAS.0, CANVAS.1, [255; 4]),
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let rect = Layer::raster(
        "Rect",
        image(RECT.2, RECT.3, GREEN),
        (RECT.0, RECT.1),
        1.0,
        true,
    )
    .unwrap();
    PrecisionDocument::from_parts(
        CANVAS.0,
        CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![base, rect],
    )
    .unwrap()
}

fn render(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

#[test]
fn sixteen_bits_render_the_byte_picture_within_257_times_its_bound() {
    for extra in [
        json!({}),
        json!({"scale":0.37,"phase":[5,2]}),
        json!({"scale":2.0,"link_with_layer":false}),
    ] {
        let s = style(extra.clone());
        let byte = styled(&rect_layer_doc(CANVAS, RECT, GREEN), 1, &s.to_string())
            .flattened()
            .unwrap();
        let typed = render(&set_style(&typed_doc(), &s).unwrap());
        let mut worst = 0u32;
        for (i, t) in typed.iter().enumerate() {
            let b = byte.as_raw()[i * 4..i * 4 + 4].to_vec();
            for c in 0..4 {
                worst = worst.max((i32::from(t[c]) - i32::from(b[c]) * 257).unsigned_abs());
            }
        }
        assert!(worst <= 257, "{extra}: worst |typed - 257 byte| = {worst}");
    }
}

#[test]
fn the_typed_setter_anchors_and_the_word_90_round_trips() {
    let dir = TempDir::new().unwrap();
    let doc = set_style(&typed_doc(), &style(json!({"scale":0.5}))).unwrap();
    let reported: Value =
        serde_json::from_str(&doc.layer(1).unwrap().reported_style().unwrap()).unwrap();
    assert_eq!(reported["effects"][0]["anchor"], json!([13, 9]));
    let path = dir.path().join("p.rz");
    let path = path.to_str().unwrap();
    doc.save_native(path, LIMIT, 0, None).unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[8..12], &90u32.to_le_bytes(), "word 90 after 46");
    let reopened = PrecisionDocument::open_native(path, LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.layer(1).unwrap().reported_style(),
        doc.layer(1).unwrap().reported_style()
    );
    assert_eq!(render(&reopened), render(&doc));
    // Without the word the file refuses by name; the word without a pattern
    // is invalid. Both before any payload decodes.
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let cut = dir.path().join("cut.rz");
    std::fs::write(&cut, &stripped).unwrap();
    let Err(message) = PrecisionDocument::open_native(cut.to_str().unwrap(), LIMIT, 0, None) else {
        panic!("a pattern without the word must refuse");
    };
    assert_eq!(message, "native pattern overlays require DRDC 90");
    let plain = dir.path().join("plain.rz");
    typed_doc()
        .save_native(plain.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    let mut claimed = std::fs::read(&plain).unwrap();
    assert_ne!(
        &claimed[8..12],
        &90u32.to_le_bytes(),
        "no word without a pattern"
    );
    claimed.splice(8..8, 90u32.to_le_bytes());
    let forged = dir.path().join("forged.rz");
    std::fs::write(&forged, &claimed).unwrap();
    let Err(message) = PrecisionDocument::open_native(forged.to_str().unwrap(), LIMIT, 0, None)
    else {
        panic!("the word without a pattern must refuse");
    };
    assert_eq!(message, "invalid native pattern overlay word");
}

/// A 512² custom tile (the spec's largest; about 1.4 MB of style JSON, past
/// the edit FFI's 64 KiB common request bound) is set through
/// `dr_precision_doc_edit` the way the host runs it (`hosted_set_style`) and
/// edited again, both with and without the overlay, while any other op past
/// that bound still refuses.
#[test]
fn a_512_square_custom_tile_is_set_and_edited_through_the_typed_edit() {
    let big = big_tile();
    let overlay = |extra: Value| {
        let mut effect = json!({"type":"pattern_overlay","pattern":big.clone()});
        for (k, v) in extra.as_object().unwrap() {
            effect[k] = v.clone();
        }
        effect
    };
    let first = json!({"effects":[overlay(json!({}))]});
    assert!(
        first.to_string().len() > 1 << 20,
        "the request is past 1 MiB"
    );
    let doc = hosted_set_style(&typed_doc(), &first);
    let reported = |doc: &PrecisionDocument| -> Value {
        serde_json::from_str(&doc.layer(1).unwrap().reported_style().unwrap()).unwrap()
    };
    assert_eq!(reported(&doc)["effects"][0]["pattern"]["width"], json!(512));
    // A later edit of the same layer: a scale change beside a new Drop Shadow.
    let second = json!({"effects":[{"type":"drop_shadow"}, overlay(json!({"scale":0.5}))]});
    let edited = hosted_set_style(&doc, &second);
    let effects = reported(&edited)["effects"].clone();
    assert_eq!(effects.as_array().unwrap().len(), 2);
    let pattern = effects
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "pattern_overlay")
        .unwrap();
    assert_eq!(pattern["scale"], json!(0.5));
    assert_eq!(pattern["pattern"]["height"], json!(512));
    // Removing the overlay is a small request on a layer holding the big tile.
    let cleared = hosted_set_style(&edited, &json!({"effects":[{"type":"drop_shadow"}]}));
    assert!(!cleared
        .layer(1)
        .unwrap()
        .reported_style()
        .unwrap()
        .contains("pattern_overlay"));
    // Every other op keeps the 64 KiB bound.
    let long = "n".repeat(70 * 1024);
    let Err(message) = edit(
        &doc,
        &json!({"op":"rename_layer","version":1,"layer":1,"name":long}),
    ) else {
        panic!("a non-style request past 64 KiB must refuse");
    };
    assert!(
        message.contains("request exceeds the supported byte limit"),
        "{message}"
    );
}

/// The 16-bit `layer_style` edit's own footprint (the measured admission of
/// the long-request parse charge and `LayerStyleEdit::scratch_bytes`): one
/// case per process, `DR_PO_CASE` = `small` (the baseline twin: the same edit
/// with the 12 x 8 tile T), `big` (a 512 x 512 custom tile set on the
/// rectangle) or `reedit` (that layer edited again: a Drop Shadow beside the
/// overlay at scale 0.5). The request is the host's form (no `\/` escape,
/// `PrecisionPixelCommand`), the shortest text for its tile, so the parse
/// charge is the smallest against what the tile allocates. The plan is read
/// first; then the footprint, the interval reset, and the edit under a job
/// control whose limit is the plan's `scratch_bytes`, as the host runs it,
/// with the result held until E = interval peak − pre is read. Release,
/// `MallocLargeCache=0`. Prints `PO_MEASURE`.
#[test]
#[ignore = "measurement: DR_PO_CASE=small|big|reedit, Release, MallocLargeCache=0"]
fn measure_layer_style_edit() {
    use crate::float_layer_edits::{footprint_reset, rusage, INTERVAL_MAX_PHYS_FOOTPRINT};
    let case = std::env::var("DR_PO_CASE").expect("DR_PO_CASE");
    let overlay = |tile: Value, extra: Value| {
        let mut effect = json!({"type":"pattern_overlay","pattern":tile});
        for (k, v) in extra.as_object().unwrap() {
            effect[k] = v.clone();
        }
        effect
    };
    let (doc, style) = match case.as_str() {
        "small" => (
            typed_doc(),
            json!({"effects":[overlay(tile_t(), json!({}))]}),
        ),
        "big" => (
            typed_doc(),
            json!({"effects":[overlay(big_tile(), json!({}))]}),
        ),
        "reedit" => (
            set_style(
                &typed_doc(),
                &json!({"effects":[overlay(big_tile(), json!({}))]}),
            )
            .unwrap(),
            json!({"effects":[{"type":"drop_shadow"}, overlay(big_tile(), json!({"scale":0.5}))]}),
        ),
        other => panic!("unknown DR_PO_CASE {other}"),
    };
    let text = json!({"op":"layer_style","version":1,"layer":1,"style":style}).to_string();
    let length = text.len();
    let c = CString::new(text).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_edit_plan(&doc, c.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!plan.is_null(), "plan: {}", take_err_string(err));
    let info: Value =
        serde_json::from_slice(unsafe { std::ffi::CStr::from_ptr(plan) }.to_bytes()).unwrap();
    unsafe { dr_string_free(plan) };
    let scratch = info["scratch_bytes"].as_u64().unwrap();
    let control = unsafe { dr_job_control_create() };
    assert!(unsafe { dr_job_control_set_scratch_limit(control, scratch) });
    let pre = footprint_reset();
    let out = unsafe {
        dr_precision_doc_edit(
            &doc,
            c.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            control,
            &mut err,
        )
    };
    let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
    unsafe { dr_job_control_free(control) };
    assert!(!out.is_null(), "edit: {}", take_err_string(err));
    let held = unsafe { Box::from_raw(out) };
    let e = peak.saturating_sub(pre);
    println!(
        "PO_MEASURE case={case} request={length} plan={scratch} pre={pre} peak={peak} E={e} floor={}",
        (e * 5).div_ceil(4)
    );
    drop(held);
}
