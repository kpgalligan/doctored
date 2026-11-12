//! The layer REFERENCE POINT (Photoshop's `fxrp`) and every
//! linked pattern anchor under Image Size, Canvas Size and Crop, which map
//! the point by the canvas map itself (POINT, rounded), at 8 bits (the plain
//! ops and the byte geometry request through the FFI), 16 bits, 32-bit float
//! and CMYK16, on rasters, a group record, a retained-source layer and a
//! masked layer: the input keeps its points (purity, so undo restores them),
//! the linked tile renders at phase + reference′, a style set after the op
//! anchors at offset′ − reference′, and `.rz` and PSD keep the point. The
//! families phase 2 shipped (the angled crop, quarter turns, flips, Free
//! Transform) are `style_pattern_reference_turns.rs`, and the filtered
//! Rotate Canvas (TAKE2) is `style_pattern_reference_rotate.rs`; both share
//! this file's build and helpers.
//!
//! THE ORACLE is the rule written out per op, never the kernel: Image Size
//! (round(x W′/W), round(y H′/H)), Crop to a window at (cx, cy) (x − cx,
//! y − cy), Canvas Size landing the old canvas at (ox, oy) (x + ox, y + oy);
//! a linked anchor is held to offset′ − anchor′ = the rule applied to
//! offset − anchor. The tile oracle is `style_pattern_reference.rs`'s: at
//! scale 1 with an integer origin every pixel reads one texel of the opaque
//! tile.
//!
//! THE PHOTOSHOP FIXTURE (`fixtures/psd_photoshop/reference_point/
//! photoshop.json`, `scripts/photoshop-probe/reference_point_take.py` take
//! 1): the take's document rebuilt at 8 bits, the same op applied, and every
//! judged record's reference against round(Photoshop's `fxrp`). BOUND,
//! declared before measuring: 0 px for each phase 1 family (IMAGE-SIZE,
//! CANVAS-SIZE, CROP).

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_doc_save_psd, dr_psd_import_report};
use doctored_core::ffi_retained_source::dr_doc_mixed_source_edit;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use image::imageops::FilterType;
use image::RgbaImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_geometry_fixture::record;
use crate::common::{byte_geometry_edit, cpath, ffi_style, solid, styled, take_err_string};

pub(crate) const LIMIT: u64 = REFERENCE_WORKING_BYTES;
type Rect = (i32, i32, u32, u32);
/// The take's document (`reference_point_take.py`, DOCUMENT).
pub(crate) const CANVAS: (u32, u32) = (120, 80);
const L0: Rect = (13, 9, 50, 40);
const L1: Rect = (70, 30, 40, 24);
const L2: Rect = (20, 55, 30, 16);
pub(crate) const WHITE: [u8; 4] = [255; 4];
const GREEN: [u8; 4] = [0x30, 0xa0, 0x50, 255];
const BLUE: [u8; 4] = [0x30, 0x50, 0xa0, 255];
const RED: [u8; 4] = [0xa0, 0x50, 0x30, 255];
/// Each styled raster's overlay phase.
pub(crate) const PHASES: [(&str, (i32, i32)); 2] = [("L0", (0, 0)), ("L1", (5, 2))];
const NAMES: [&str; 5] = ["Background", "L0", "L1", "L2", "G"];

// ---------------------------------------------------------------- the tile --

/// take1's tile T: 12 x 8, opaque, (20x + 10, 30y + 15, 200 | 60).
pub(crate) fn texel(x: u32, y: u32) -> [u8; 4] {
    [
        (20 * x + 10) as u8,
        (30 * y + 15) as u8,
        if (x + y).is_multiple_of(2) { 200 } else { 60 },
        255,
    ]
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

/// One linked pattern overlay of tile T with `phase`, no anchor.
pub(crate) fn style(phase: (i32, i32)) -> Value {
    let mut rgba = Vec::new();
    for y in 0..8 {
        for x in 0..12 {
            rgba.extend_from_slice(&texel(x, y));
        }
    }
    json!({"effects":[{"type":"pattern_overlay",
        "pattern":{"name":"T","width":12,"height":8,"rgba":base64(&rgba)},
        "phase":[phase.0, phase.1]}]})
}

/// The texel tile T puts at canvas pixel (x, y) with its origin at `o`.
fn want(o: (i32, i32), x: i32, y: i32) -> [u8; 4] {
    texel(
        (x - o.0).rem_euclid(12) as u32,
        (y - o.1).rem_euclid(8) as u32,
    )
}

// -------------------------------------------------------------- the oracle --

/// A whole-canvas op of a shipped family, with its rule written out.
#[derive(Clone, Copy, Debug)]
enum Op {
    Resize(u32, u32),
    Canvas(u32, u32, (i32, i32)),
    Crop(u32, u32, u32, u32),
}

const OPS: [Op; 5] = [
    Op::Resize(187, 127),
    Op::Resize(77, 51),
    Op::Canvas(151, 97, (15, 8)),
    Op::Canvas(100, 70, (-10, -5)),
    Op::Crop(10, 6, 100, 68),
];

impl Op {
    /// `r` after the op on a `canvas`-sized document (module doc).
    fn oracle(self, r: (i32, i32), canvas: (u32, u32)) -> (i32, i32) {
        match self {
            Op::Resize(w, h) => (
                (f64::from(r.0) * f64::from(w) / f64::from(canvas.0)).round() as i32,
                (f64::from(r.1) * f64::from(h) / f64::from(canvas.1)).round() as i32,
            ),
            Op::Canvas(_, _, (ox, oy)) => (r.0 + ox, r.1 + oy),
            Op::Crop(cx, cy, _, _) => (r.0 - cx as i32, r.1 - cy as i32),
        }
    }

    fn request(self) -> Value {
        match self {
            Op::Resize(w, h) => json!({"op":"resize","width":w,"height":h,"filter":"nearest"}),
            Op::Canvas(w, h, (x, y)) => {
                json!({"op":"canvas_size","width":w,"height":h,"x":x,"y":y})
            }
            Op::Crop(x, y, w, h) => json!({"op":"crop","x":x,"y":y,"width":w,"height":h}),
        }
    }

    fn plain(self, doc: &DrDocument) -> DrDocument {
        match self {
            Op::Resize(w, h) => doc.resize(w, h, FilterType::Nearest),
            Op::Canvas(w, h, origin) => doc.canvas_resize(w, h, origin),
            Op::Crop(x, y, w, h) => doc.crop(x, y, w, h),
        }
        .unwrap_or_else(|| panic!("{self:?} changes the document"))
    }

    /// Scale 1: the tile oracle applies.
    fn unscaled(self) -> bool {
        !matches!(self, Op::Resize(..))
    }
}

pub(crate) fn minus(a: (i32, i32), b: (i32, i32)) -> (i32, i32) {
    (a.0 - b.0, a.1 - b.1)
}

fn pair(v: &Value) -> Option<(i32, i32)> {
    Some((v.get(0)?.as_i64()? as i32, v.get(1)?.as_i64()? as i32))
}

// ------------------------------------------------------------- 8-bit build --

pub(crate) fn index8(doc: &DrDocument, name: &str) -> usize {
    doc.layers
        .iter()
        .position(|l| l.name == name)
        .unwrap_or_else(|| panic!("no layer {name}"))
}

/// A raster MADE at `rect` above `below`: its reference point stays (0, 0).
fn made(doc: &DrDocument, below: usize, rect: Rect, color: [u8; 4], name: &str) -> DrDocument {
    let doc = doc
        .adding_image_layer(below, solid(rect.2, rect.3, color), name)
        .unwrap();
    let idx = index8(&doc, name);
    let mut placed = doc.with_layer_offset(idx, rect.0, rect.1).unwrap();
    placed.layers[idx].reference = (0, 0);
    placed
}

/// The take's document at 8 bits: L0 linked at phase (0, 0); L1 moved (7, 3)
/// and then linked at phase (5, 2); unstyled L2 inside group G, G moved
/// (5, −2).
pub(crate) fn take_doc8() -> DrDocument {
    let doc = DrDocument::from_pixels(solid(CANVAS.0, CANVAS.1, WHITE));
    let doc = made(&doc, 0, L0, GREEN, "L0");
    let doc = styled(&doc, index8(&doc, "L0"), &style((0, 0)).to_string());
    let doc = made(&doc, index8(&doc, "L0"), L1, BLUE, "L1");
    let l1 = index8(&doc, "L1");
    let doc = doc.move_layers(&[l1], 7, 3).unwrap();
    let doc = styled(&doc, l1, &style((5, 2)).to_string());
    let doc = made(&doc, l1, L2, RED, "L2");
    let (doc, group, _, _) = doc.group_layers(&[index8(&doc, "L2")], "G").unwrap();
    doc.move_layers(&[group], 5, -2).unwrap()
}

/// [`take_doc8`] on a `canvas`-sized canvas: Canvas Size anchored top left,
/// so every point and anchor is unchanged.
pub(crate) fn take_doc8_sized(canvas: (u32, u32)) -> DrDocument {
    let doc = take_doc8();
    if canvas == CANVAS {
        return doc;
    }
    let out = doc.canvas_resize(canvas.0, canvas.1, (0, 0)).unwrap();
    assert_eq!(
        facts8(&out),
        facts8(&doc),
        "Canvas Size top left keeps the points"
    );
    out
}

pub(crate) fn anchor8(doc: &DrDocument, idx: usize) -> Option<(i32, i32)> {
    let style: Value = serde_json::from_str(&ffi_style(doc, idx)?).unwrap();
    pair(&style["effects"][0]["anchor"])
}

/// Each entry's (name, reference, offset as the anchor rule reads it, anchor).
pub(crate) type Facts = Vec<(String, (i32, i32), (i32, i32), Option<(i32, i32)>)>;

pub(crate) fn facts8(doc: &DrDocument) -> Facts {
    doc.layers
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let offset = if l.name == "G" { (0, 0) } else { l.offset };
            (l.name.clone(), l.reference, offset, anchor8(doc, i))
        })
        .collect()
}

/// `after` holds `before`'s facts moved by `op` (module doc).
fn assert_followed(before: &Facts, after: &Facts, op: Op, canvas: (u32, u32), what: &str) {
    assert_eq!(before.len(), after.len(), "{what}: the same records");
    for (b, a) in before.iter().zip(after) {
        assert_eq!(b.0, a.0, "{what}: record order");
        assert_eq!(a.1, op.oracle(b.1, canvas), "{what}: {}'s reference", b.0);
        match (b.3, a.3) {
            (Some(ab), Some(aa)) => assert_eq!(
                minus(a.2, aa),
                op.oracle(minus(b.2, ab), canvas),
                "{what}: {}'s offset − anchor",
                b.0
            ),
            (None, None) => {}
            other => panic!("{what}: {}'s anchor {other:?}", b.0),
        }
    }
}

/// Every pixel of a styled raster's rect inside the canvas is tile T at
/// phase + reference.
fn assert_tile8(doc: &DrDocument, what: &str) {
    let flat = doc.flattened().unwrap();
    for (name, phase) in PHASES {
        let layer = &doc.layers[index8(doc, name)];
        let (w, h) = layer.pixels.dimensions();
        let o = (phase.0 + layer.reference.0, phase.1 + layer.reference.1);
        for y in layer.offset.1.max(0)..(layer.offset.1 + h as i32).min(doc.height as i32) {
            for x in layer.offset.0.max(0)..(layer.offset.0 + w as i32).min(doc.width as i32) {
                assert_eq!(
                    flat.get_pixel(x as u32, y as u32).0,
                    want(o, x, y),
                    "{what}: {name} at ({x}, {y})"
                );
            }
        }
    }
}

pub(crate) fn byte_request(doc: &DrDocument, request: &Value) -> DrDocument {
    let (out, err) = byte_geometry_edit(doc, request);
    assert!(!out.is_null(), "{request}: {err:?}");
    let result = unsafe { (*out).clone() };
    unsafe { dr_doc_free(out) };
    result
}

// ------------------------------------------------------------------- 8-bit --

#[test]
fn the_take_document_carries_photoshops_before_points() {
    let doc = take_doc8();
    let got: Vec<_> = NAMES
        .iter()
        .map(|n| doc.layers[index8(&doc, n)].reference)
        .collect();
    assert_eq!(got, [(0, 0), (0, 0), (7, 3), (5, -2), (5, -2)]);
    assert_eq!(anchor8(&doc, index8(&doc, "L0")), Some((13, 9)));
    assert_eq!(anchor8(&doc, index8(&doc, "L1")), Some((70, 30)));
}

#[test]
fn byte_ops_map_every_point_and_linked_anchor() {
    let doc = take_doc8();
    let before = facts8(&doc);
    for op in OPS {
        let plain = op.plain(&doc);
        assert_followed(
            &before,
            &facts8(&plain),
            op,
            CANVAS,
            &format!("{op:?} plain"),
        );
        let request = byte_request(&doc, &op.request());
        assert_eq!(
            facts8(&request),
            facts8(&plain),
            "{op:?}: request = plain op"
        );
        assert_eq!(facts8(&doc), before, "{op:?}: the input keeps its points");
        if op.unscaled() {
            assert_tile8(&plain, &format!("{op:?}"));
        }
        // A style set AFTER the op anchors at offset′ − reference′.
        let l2 = index8(&plain, "L2");
        let restyled = styled(&plain, l2, &style((0, 0)).to_string());
        let layer = &restyled.layers[l2];
        assert_eq!(
            anchor8(&restyled, l2),
            Some(minus(layer.offset, layer.reference)),
            "{op:?}: a style set after the op"
        );
    }
}

#[test]
fn a_masked_and_a_retained_source_layer_follow() {
    let doc = take_doc8();
    let l1 = index8(&doc, "L1");
    let masked = doc.add_mask(l1, MaskKind::RevealAll).unwrap();
    let before = facts8(&masked);
    for op in OPS {
        let out = op.plain(&masked);
        assert!(out.layers[l1].mask.is_some(), "{op:?}: the mask rides");
        assert_followed(
            &before,
            &facts8(&out),
            op,
            CANVAS,
            &format!("{op:?} masked"),
        );
    }

    let source = DrDocument::from_pixels(RgbaImage::from_fn(32, 24, |x, y| {
        image::Rgba([(x * 7) as u8, (y * 9) as u8, 90, 255])
    }));
    let request = CString::new(
        json!({"op":"place","source_id":"fixture-a","name":"Placed","x":0,"y":0}).to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let placed = unsafe {
        dr_doc_mixed_source_edit(
            &source,
            &source,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!placed.is_null(), "{}", take_err_string(err));
    let placed = *unsafe { Box::from_raw(placed) };
    let moved = placed.move_layers(&[1], 5, 6).unwrap();
    assert!(moved.layers[1].retained_source.is_some());
    assert_eq!(moved.layers[1].reference, (5, 6));
    for op in [
        Op::Resize(48, 30),
        Op::Canvas(40, 30, (3, -2)),
        Op::Crop(2, 1, 20, 15),
    ] {
        let out = op.plain(&moved);
        assert!(
            out.layers[1].retained_source.is_some(),
            "{op:?}: still bound"
        );
        assert_eq!(
            out.layers[1].reference,
            op.oracle((5, 6), (32, 24)),
            "{op:?}: the retained source's reference"
        );
        assert_eq!(moved.layers[1].reference, (5, 6), "{op:?}: purity");
    }
}

#[test]
fn rz_and_psd_keep_the_mapped_point() {
    let dir = TempDir::new().unwrap();
    let doc = take_doc8();
    for (i, op) in OPS.into_iter().enumerate() {
        let out = op.plain(&doc);
        let rz = dir.path().join(format!("{i}.rz"));
        out.save_native(rz.to_str().unwrap()).unwrap();
        let back = DrDocument::open(rz.to_str().unwrap()).unwrap();
        assert_eq!(facts8(&back), facts8(&out), "{op:?}: .doctored");

        let psd = dir.path().join(format!("{i}.psd"));
        save8(&out, &psd);
        let reopened = open8(&psd);
        for name in NAMES {
            assert_eq!(
                reopened.layers[index8(&reopened, name)].reference,
                out.layers[index8(&out, name)].reference,
                "{op:?}: {name}'s fxrp through PSD"
            );
        }
        for (name, _) in PHASES {
            let (a, b) = (index8(&reopened, name), index8(&out, name));
            assert_eq!(
                minus(reopened.layers[a].offset, anchor8(&reopened, a).unwrap()),
                minus(out.layers[b].offset, anchor8(&out, b).unwrap()),
                "{op:?}: {name}'s offset − anchor through PSD"
            );
        }
    }
}

pub(crate) fn save8(doc: &DrDocument, path: &Path) {
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe {
        dr_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            0,
            0,
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "save: {}", take_err_string(error));
}

pub(crate) fn open8(path: &Path) -> DrDocument {
    let mut error = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!raw.is_null(), "{}", take_err_string(error));
    let report: Value = serde_json::from_str(&take_err_string(raw)).unwrap();
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let raw = unsafe {
        dr_doc_open_psd(
            cpath(path).as_ptr(),
            0,
            token.as_ptr(),
            std::ptr::null(),
            &mut error,
        )
    };
    assert!(!raw.is_null(), "open_psd: {}", take_err_string(error));
    *unsafe { Box::from_raw(raw) }
}

// ---------------------------------------------------------- typed depths --

pub(crate) fn typed_edit(doc: &PrecisionDocument, request: &Value) -> PrecisionDocument {
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
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

pub(crate) fn index16(doc: &PrecisionDocument, name: &str) -> usize {
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

fn anchor16(doc: &PrecisionDocument, idx: usize) -> Option<(i32, i32)> {
    let reported: Value = serde_json::from_str(&doc.layer(idx)?.reported_style()?).unwrap();
    pair(&reported["effects"][0]["anchor"])
}

/// The typed facts in the byte order of NAMES.
pub(crate) fn facts16(doc: &PrecisionDocument) -> Facts {
    NAMES
        .iter()
        .map(|name| {
            let i = index16(doc, name);
            let layer = doc.layer(i).unwrap();
            (
                name.to_string(),
                layer.reference(),
                layer.offset(),
                anchor16(doc, i),
            )
        })
        .collect()
}

pub(crate) fn names_order(facts: Facts) -> Facts {
    NAMES
        .iter()
        .map(|n| facts.iter().find(|f| f.0 == *n).unwrap().clone())
        .collect()
}

pub(crate) fn typed16() -> PrecisionDocument {
    take_doc8().promote_structure(LIMIT, 0, None).unwrap()
}

pub(crate) fn cmyk16() -> PrecisionDocument {
    let rgb = typed16();
    let profile = press_profile("Press Test");
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            &rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

/// The shipped ops on one typed document: every record's point and anchor
/// follow, the input keeps them, and a style set after the op anchors at
/// offset′ − reference′.
fn assert_typed_follow(doc: &PrecisionDocument, depth: &str) {
    let before = facts16(doc);
    assert_eq!(
        before,
        names_order(facts8(&take_doc8())),
        "{depth}: the conversion carries every point and anchor"
    );
    for op in OPS {
        let out = typed_edit(doc, &op.request());
        assert_followed(
            &before,
            &facts16(&out),
            op,
            CANVAS,
            &format!("{depth} {op:?}"),
        );
        assert_eq!(facts16(doc), before, "{depth} {op:?}: purity");
        let l2 = index16(&out, "L2");
        let restyled = typed_edit(
            &out,
            &json!({"op":"layer_style","version":1,"layer":l2,"style":style((0, 0))}),
        );
        let layer = restyled.layer(l2).unwrap();
        assert_eq!(
            anchor16(&restyled, l2),
            Some(minus(layer.offset(), layer.reference())),
            "{depth} {op:?}: a style set after the op"
        );
    }
    // The Crop tool's typed crop is the identity Straighten, with and
    // without Delete Cropped Pixels: both are the Crop rule.
    for delete in [false, true] {
        let out = typed_edit(
            doc,
            &json!({"op":"straighten","matrix":[1,0,0,1,0,0],"filter":"nearest",
                    "x":10,"y":6,"width":100,"height":68,"delete_cropped":delete}),
        );
        let op = Op::Crop(10, 6, 100, 68);
        let what = format!("{depth} identity straighten, delete {delete}");
        assert_followed(&before, &facts16(&out), op, CANVAS, &what);
    }
}

#[test]
fn typed_ops_map_every_point_at_16_bits_float_and_cmyk() {
    let sixteen = typed16();
    assert_typed_follow(&sixteen, "RGB16");
    // The tile at scale 1, from the 16-bit render (within 1 code).
    for op in OPS.into_iter().filter(|op| op.unscaled()) {
        let out = typed_edit(&sixteen, &op.request());
        let (w, h) = out.dimensions();
        let pixels = out.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
        for (name, phase) in PHASES {
            let layer = out.layer(index16(&out, name)).unwrap();
            let (lw, lh) = layer.source().unwrap().dimensions();
            let (ox, oy) = layer.offset();
            let o = (phase.0 + layer.reference().0, phase.1 + layer.reference().1);
            for y in oy.max(0)..(oy + lh as i32).min(h as i32) {
                for x in ox.max(0)..(ox + lw as i32).min(w as i32) {
                    let got = pixels[(y as u32 * w + x as u32) as usize];
                    let t = want(o, x, y);
                    for c in 0..3 {
                        let code = f64::from(got[c]) / 257.0;
                        assert!(
                            (code - f64::from(t[c])).abs() <= 1.0,
                            "{op:?} {name} at ({x}, {y}): {got:?} against {t:?}"
                        );
                    }
                }
            }
        }
    }
    let float = sixteen.promote_to_float(LIMIT, 0, None).unwrap();
    assert_typed_follow(&float, "float");
    let cmyk = cmyk16();
    let l0 = index16(&cmyk, "L0");
    assert_eq!(record(&cmyk, l0).1, (L0.0, L0.1), "a CMYK ink record");
    assert_typed_follow(&cmyk, "CMYK16");
}

// ------------------------------------------------------ the angled crop --

/// The Crop tool's matrix: rotate by `−angle` about the window's centre.
pub(crate) fn rotation(angle: f64, window: (u32, u32, u32, u32)) -> [f64; 6] {
    let cx = f64::from(window.0) + f64::from(window.2) / 2.0;
    let cy = f64::from(window.1) + f64::from(window.3) / 2.0;
    let radians = -angle * std::f64::consts::PI / 180.0;
    let (s, c) = (radians.sin(), radians.cos());
    [c, s, -s, c, cx - c * cx + s * cy, cy - s * cx - c * cy]
}

// --------------------------------------------------------- Photoshop --

pub(crate) fn fxrp(v: &Value) -> (i32, i32) {
    (
        v[0].as_f64().unwrap().round() as i32,
        v[1].as_f64().unwrap().round() as i32,
    )
}

#[test]
fn photoshops_take_holds_for_every_shipped_family() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/reference_point/photoshop.json");
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut judged = 0;
    for (name, case) in fixture["cases"].as_object().unwrap() {
        let family = case["family"].as_str().unwrap();
        if fixture["families"][family]["verdict"] != "SHIP" {
            continue;
        }
        let records = &case["records"];
        let bounds = |record: &str, when: &str| -> (i32, i32) {
            let b = &records[record][when];
            (b[0].as_i64().unwrap() as i32, b[1].as_i64().unwrap() as i32)
        };
        let edit = &case["edit"];
        let size = |key: &str| edit[key].as_u64().unwrap() as u32;
        let op = match family {
            "IMAGE-SIZE" => Op::Resize(size("width"), size("height")),
            "CANVAS-SIZE" => Op::Canvas(
                size("width"),
                size("height"),
                minus(bounds("L0", "bounds_after"), bounds("L0", "bounds_before")),
            ),
            "CROP" => {
                let b: Vec<u32> = edit["bounds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect();
                Op::Crop(b[0], b[1], b[2] - b[0], b[3] - b[1])
            }
            other => panic!("{name}: an unexpected shipped family {other}"),
        };
        let doc = take_doc8();
        let out = op.plain(&doc);
        let canvas = &case["canvas_after"];
        assert_eq!(
            (out.width, out.height),
            (
                canvas[0].as_u64().unwrap() as u32,
                canvas[1].as_u64().unwrap() as u32
            ),
            "{name}: the canvas"
        );
        for record in ["L0", "L1", "L2", "G"] {
            if !fixture["before_findings"][name][record].is_null() {
                continue;
            }
            let before = &records[record]["fxrp_before"];
            let ps_before = if before.is_null() {
                (0, 0)
            } else {
                fxrp(before)
            };
            assert_eq!(
                doc.layers[index8(&doc, record)].reference,
                ps_before,
                "{name}: {record} before"
            );
            // BOUND 0 px (module doc).
            assert_eq!(
                out.layers[index8(&out, record)].reference,
                fxrp(&records[record]["fxrp_after"]),
                "{name}: {record} against round(Photoshop's fxrp)"
            );
            judged += 1;
        }
    }
    assert_eq!(judged, 9 * 4, "every record of the nine shipped cases");
}
