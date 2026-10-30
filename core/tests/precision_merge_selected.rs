//! Stamp Selected and Merge Group at 8 bits (`dr_doc_stamp_selected`,
//! `dr_doc_merge_group`), 16 bits and 32-bit float (the `stamp_selected` and
//! `merge_group` structure edits of `dr_precision_doc_edit`), #1 phase 4c.
//!
//! **Bounds, fixed here before anything was measured:** 8 bits 1 code of
//! 255; 16 bits 2 codes of 65535; float relative 1e-5 against
//! `max(|want|, 1)`. Every pixel claim is checked against an f64 W3C oracle
//! written in this file (straight source-over with the separable blend
//! function of `common/native_blend_oracle.rs`, alpha × opacity × mask),
//! reading the document's own stored samples: the stamped leaf against its
//! members composited over transparency on their union extent, the merged
//! group's leaf against its children likewise, and the document's composite
//! unchanged by Merge Group for an isolated Normal and an isolated Multiply
//! group. Refusals are checked by their sentence.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::{
    dr_doc_free, dr_doc_layer_count, dr_doc_with_layer_blend_mode, dr_doc_with_layer_opacity,
};
use doctored_core::ffi_group::dr_doc_group_layers;
use doctored_core::ffi_mask::dr_doc_set_mask;
use doctored_core::ffi_merge_selected::{dr_doc_merge_group, dr_doc_stamp_selected};
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{
    add_layer, apply, doc_from, flat_pixels, layer_dims, layer_name, layer_offset, layer_pixels,
};
use crate::native_blend_oracle as blend_oracle;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 5;
const H: u32 = 3;
const MULTIPLY: i32 = 1;

// ------------------------------------------------------- the f64 oracle --

/// Straight `cs` at source alpha `sa` over the straight backdrop `b` through
/// the separable blend `mode` (W3C compositing-1).
fn over(b: [f64; 4], cs: [f64; 3], sa: f64, mode: i32) -> [f64; 4] {
    let ab = b[3];
    let ao = sa + ab * (1.0 - sa);
    if ao <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        let blended = blend_oracle::separable(mode, b[c], cs[c]);
        out[c] = (sa * (1.0 - ab) * cs[c] + sa * ab * blended + (1.0 - sa) * ab * b[c]) / ao;
    }
    out[3] = ao;
    out
}

/// One member as the oracle reads it: straight samples in [0, 1] over a
/// `w × h` rect at `offset`, its opacity, blend mode and canvas mask.
struct Member {
    samples: Vec<[f64; 4]>,
    w: u32,
    h: u32,
    offset: (i32, i32),
    opacity: f64,
    mode: i32,
    mask: Option<Vec<f64>>,
}

impl Member {
    fn at(&self, x: i64, y: i64) -> Option<[f64; 4]> {
        let (px, py) = (x - i64::from(self.offset.0), y - i64::from(self.offset.1));
        if px < 0 || py < 0 || px >= i64::from(self.w) || py >= i64::from(self.h) {
            return None;
        }
        Some(self.samples[(py * i64::from(self.w) + px) as usize])
    }

    fn coverage(&self, x: i64, y: i64) -> f64 {
        let Some(mask) = &self.mask else {
            return 1.0;
        };
        if x < 0 || y < 0 || x >= i64::from(W) || y >= i64::from(H) {
            return 0.0;
        }
        mask[(y * i64::from(W) + x) as usize]
    }
}

/// `members` composited bottom first over `acc` at canvas `(x, y)`.
fn stack(members: &[Member], x: i64, y: i64, mut acc: [f64; 4]) -> [f64; 4] {
    for m in members {
        if let Some(s) = m.at(x, y) {
            acc = over(
                acc,
                [s[0], s[1], s[2]],
                s[3] * m.opacity * m.coverage(x, y),
                m.mode,
            );
        }
    }
    acc
}

// ------------------------------------------------------------ 16 bits --

fn varied16(w: u32, h: u32, seed: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let j = i
                .wrapping_mul(2_654_435_761)
                .wrapping_add(seed.wrapping_mul(40_503));
            let alpha = match (i + seed) % 4 {
                0 | 1 => 65535,
                2 => 40000,
                _ => 15000,
            };
            [
                (j % 65536) as u16,
                (j.rotate_left(11) % 65536) as u16,
                (j.rotate_left(23) % 65536) as u16,
                alpha,
            ]
        })
        .collect()
}

fn raster16(name: &str, w: u32, h: u32, offset: (i32, i32), seed: u32) -> Layer {
    let image =
        PrecisionImage::from_rgba16(w, h, varied16(w, h, seed), IccProfile::srgb(), None).unwrap();
    Layer::raster(name, image, offset, 1.0, true).unwrap()
}

const MASK: [u16; 15] = [
    0, 9000, 20000, 33000, 45000, 52000, 60000, 65535, 30000, 12000, 65535, 5000, 41000, 27000,
    65535,
];

fn mask16() -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(W, H, MASK.to_vec(), None).unwrap(),
        Default::default(),
        true,
    )
    .unwrap()
}

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

fn edit(document: &PrecisionDocument, request: Value) -> Result<Box<PrecisionDocument>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let planned =
        unsafe { dr_precision_doc_edit_plan(document, text.as_ptr(), LIMIT, 0, &mut error) };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    let plan: Value =
        serde_json::from_str(&unsafe { CStr::from_ptr(planned) }.to_string_lossy()).unwrap();
    unsafe { dr_string_free(planned) };
    assert!(
        plan["scratch_bytes"].as_u64().is_some_and(|v| v > 0),
        "{plan}"
    );
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            document,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if let Some(error) = take_error(error) {
        return Err(error);
    }
    assert!(!out.is_null(), "{request} changed nothing");
    Ok(unsafe { Box::from_raw(out) })
}

fn must(document: &PrecisionDocument, request: Value) -> Box<PrecisionDocument> {
    edit(document, request.clone()).unwrap_or_else(|e| panic!("{request}: {e}"))
}

fn index_of(document: &PrecisionDocument, name: &str) -> usize {
    (0..document.layer_count())
        .find(|&i| document.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("{name} is present"))
}

fn names(document: &PrecisionDocument) -> Vec<String> {
    (0..document.layer_count())
        .map(|i| document.layer(i).unwrap().name().to_string())
        .collect()
}

/// `[Base, A (3 × 2 at (−1, 0)), B (Multiply 0.6, masked), C]`.
fn fixture16() -> PrecisionDocument {
    let mut base = raster16("Base", W, H, (0, 0), 1);
    let image = PrecisionImage::from_rgba16(
        W,
        H,
        varied16(W, H, 1)
            .into_iter()
            .map(|p| [p[0], p[1], p[2], 65535])
            .collect(),
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    base = Layer::raster(base.name(), image, (0, 0), 1.0, true).unwrap();
    let document = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            base,
            raster16("A", 3, 2, (-1, 0), 2),
            raster16("B", W, H, (0, 0), 3).with_mask(Some(mask16())),
            raster16("C", W, H, (0, 0), 4),
        ],
    )
    .unwrap();
    let document = must(
        &document,
        json!({"op":"layer_blend","version":1,"layer":2,"blend_mode":MULTIPLY}),
    );
    let mut err = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_precision::dr_precision_doc_set_layer_opacity(
            &*document, 2, 0.6, &mut err,
        )
    };
    assert!(!out.is_null(), "{:?}", take_error(err));
    *unsafe { Box::from_raw(out) }
}

/// The oracle's view of record `index` (its own samples, offset, opacity,
/// mode and mask), at 16 bits or float.
fn member(document: &PrecisionDocument, index: usize) -> Member {
    let layer = document.layer(index).unwrap();
    let source = layer.source().expect("a raster record");
    let (w, h) = source.dimensions();
    let samples = match source.to_rgba16() {
        Ok(s) => s
            .iter()
            .map(|p| p.map(|v| f64::from(v) / 65535.0))
            .collect(),
        Err(_) => source
            .to_rgba32()
            .unwrap()
            .iter()
            .map(|p| p.map(f64::from))
            .collect(),
    };
    let mode = match layer.blend_mode() {
        doctored_core::doc::BlendMode::Multiply => MULTIPLY,
        _ => 0,
    };
    let mask = layer.mask().map(|m| {
        m.plane()
            .samples()
            .iter()
            .map(|&v| f64::from(v) / 65535.0)
            .collect()
    });
    Member {
        samples,
        w,
        h,
        offset: layer.offset(),
        opacity: f64::from(layer.opacity()),
        mode,
        mask,
    }
}

/// The leaf `name` against `members` composited over transparency, over its
/// own extent `frame`.
fn assert_leaf(
    document: &PrecisionDocument,
    name: &str,
    members: &[Member],
    frame: ((i32, i32), (u32, u32)),
    float: bool,
) {
    let index = index_of(document, name);
    let layer = document.layer(index).unwrap();
    let source = layer.source().expect("a raster leaf");
    assert_eq!(
        (layer.offset(), source.dimensions()),
        frame,
        "{name}'s frame"
    );
    let ((x0, y0), (w, h)) = frame;
    let got: Vec<[f64; 4]> = if float {
        source
            .to_rgba32()
            .unwrap()
            .iter()
            .map(|p| p.map(f64::from))
            .collect()
    } else {
        source
            .to_rgba16()
            .unwrap()
            .iter()
            .map(|p| p.map(|v| f64::from(v) / 65535.0))
            .collect()
    };
    for y in 0..h {
        for x in 0..w {
            let want = stack(
                members,
                i64::from(x0 + x as i32),
                i64::from(y0 + y as i32),
                [0.0; 4],
            );
            let g = got[(y * w + x) as usize];
            // A transparent pixel's colour means nothing.
            let channels = if want[3] <= 0.0 { 3..4 } else { 0..4 };
            for c in channels {
                close(
                    g[c],
                    want[c],
                    float,
                    &format!("{name} ({x}, {y}) channel {c}"),
                );
            }
        }
    }
}

fn close(got: f64, want: f64, float: bool, what: &str) {
    if float {
        let scale = want.abs().max(1.0);
        assert!(
            (got - want).abs() <= 1e-5 * scale,
            "{what}: {got} vs {want}"
        );
    } else {
        let want = want.clamp(0.0, 1.0);
        assert!(
            (got - want).abs() * 65535.0 <= 2.0,
            "{what}: {} vs {}",
            got * 65535.0,
            want * 65535.0
        );
    }
}

fn render(document: &PrecisionDocument) -> Vec<[f64; 4]> {
    let image = document.render(LIMIT, 0, None).unwrap();
    match image.to_rgba16() {
        Ok(s) => s
            .iter()
            .map(|p| p.map(|v| f64::from(v) / 65535.0))
            .collect(),
        Err(_) => image
            .to_rgba32()
            .unwrap()
            .iter()
            .map(|p| p.map(f64::from))
            .collect(),
    }
}

fn assert_unchanged(
    before: &PrecisionDocument,
    after: &PrecisionDocument,
    float: bool,
    what: &str,
) {
    for (i, (b, a)) in render(before).iter().zip(render(after)).enumerate() {
        for c in 0..4 {
            close(a[c], b[c], float, &format!("{what} pixel {i} channel {c}"));
        }
    }
}

fn promoted(document: &PrecisionDocument) -> Box<PrecisionDocument> {
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_promote_to_float(document, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "promote: {:?}", take_error(err));
    unsafe { Box::from_raw(out) }
}

fn stamp_case(document: &PrecisionDocument, float: bool) {
    let members = [member(document, 1), member(document, 2)];
    let after = must(
        document,
        json!({"op":"stamp_selected","layers":[2,1],"name":"B (merged)"}),
    );
    assert_eq!(names(&after), ["Base", "A", "B", "B (merged)", "C"]);
    // A ∪ B = (−1, 0)…(5, 3).
    assert_leaf(&after, "B (merged)", &members, ((-1, 0), (W + 1, H)), float);
    let leaf = after.layer(3).unwrap();
    assert_eq!(leaf.opacity(), 1.0);
    assert_eq!(leaf.blend_mode(), doctored_core::doc::BlendMode::Normal);
    assert!(!leaf.clipped() && leaf.mask().is_none());
    assert!(
        (0..4).all(|i| document.layer_id(i) != after.layer_id(3)),
        "a new id"
    );
    // The originals are untouched.
    for (i, name) in ["Base", "A", "B", "C"].iter().enumerate() {
        let index = index_of(&after, name);
        assert_eq!(after.layer_id(index), document.layer_id(i), "{name}'s id");
    }
    assert_eq!(render(document).len(), render(&after).len());
}

fn merge_group_case(document: &PrecisionDocument, float: bool) {
    for mode in [0, MULTIPLY] {
        let grouped = must(
            document,
            json!({"op":"group_layers","layers":[1,2],"name":"G","pass_through":false}),
        );
        let g = index_of(&grouped, "G");
        let mut err = ptr::null_mut();
        let out = unsafe {
            doctored_core::ffi_precision::dr_precision_doc_set_layer_opacity(
                &*grouped, g, 0.7, &mut err,
            )
        };
        assert!(!out.is_null(), "{:?}", take_error(err));
        let mut grouped = unsafe { Box::from_raw(out) };
        if mode != 0 {
            grouped = must(
                &grouped,
                json!({"op":"layer_blend","version":1,"layer":g,"blend_mode":mode}),
            );
        }
        let children = [member(&grouped, 1), member(&grouped, 2)];
        let id = grouped.layer_id(g);
        let after = must(&grouped, json!({"op":"merge_group","layer":g}));
        assert_eq!(names(&after), ["Base", "G", "C"]);
        assert_leaf(&after, "G", &children, ((-1, 0), (W + 1, H)), float);
        let leaf = after.layer(1).unwrap();
        assert!(leaf.children().is_none());
        assert_eq!(after.layer_id(1), id);
        assert_eq!(leaf.opacity(), 0.7);
        assert_unchanged(&grouped, &after, float, &format!("merge group mode {mode}"));
    }
}

#[test]
fn stamp_selected_at_16_bits_and_float() {
    let document = fixture16();
    stamp_case(&document, false);
    stamp_case(&promoted(&document), true);
}

#[test]
fn merge_group_at_16_bits_and_float() {
    let document = fixture16();
    merge_group_case(&document, false);
    merge_group_case(&promoted(&document), true);
}

#[test]
fn a_pass_through_group_merges_as_normal() {
    let document = fixture16();
    let grouped = must(
        &document,
        json!({"op":"group_layers","layers":[1,2],"name":"P","pass_through":true}),
    );
    let p = index_of(&grouped, "P");
    let after = must(&grouped, json!({"op":"merge_group","layer":p}));
    let leaf = after.layer(index_of(&after, "P")).unwrap();
    assert_eq!(leaf.blend_mode(), doctored_core::doc::BlendMode::Normal);
    assert!(leaf.children().is_none());
}

#[test]
fn typed_refusals_are_named() {
    let document = fixture16();
    let grouped = must(
        &document,
        json!({"op":"group_layers","layers":[2,3],"name":"G","pass_through":false}),
    );
    // [Base, A, B, C, G]
    let refused = |request: Value| {
        edit(&grouped, request.clone())
            .err()
            .unwrap_or_else(|| panic!("{request} was refused"))
    };
    assert_eq!(
        refused(json!({"op":"stamp_selected","layers":[1],"name":"S"})),
        "Stamp Selected requires a group or at least two layers; layer 1 is not a group"
    );
    assert_eq!(
        refused(json!({"op":"stamp_selected","layers":[1,2],"name":"S"})),
        "Stamp Selected requires layers that share one parent"
    );
    assert_eq!(
        refused(json!({"op":"stamp_selected","layers":[],"name":"S"})),
        "Stamp Selected requires at least one layer"
    );
    assert_eq!(
        refused(json!({"op":"merge_group","layer":0})),
        "Merge Group requires a visible group; layer 0 is not one"
    );
    let locked = must(&grouped, json!({"op":"layer_lock","layer":4,"locks":7}));
    let sentence = edit(&locked, json!({"op":"merge_group","layer":4}))
        .err()
        .unwrap();
    assert!(
        sentence.starts_with("high-depth layer 4 is locked"),
        "{sentence}"
    );
    // An unknown key refuses.
    assert!(edit(&grouped, json!({"op":"merge_group","layer":4,"x":1})).is_err());
}

// ------------------------------------------------------------- 8 bits --

fn varied8(w: u32, h: u32, seed: u32, opaque: bool) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let i = y * w + x;
        let j = i
            .wrapping_mul(2_654_435_761)
            .wrapping_add(seed.wrapping_mul(40_503));
        let a = if opaque {
            255
        } else {
            [255, 255, 160, 60][((i + seed) % 4) as usize]
        };
        Rgba([
            (j % 256) as u8,
            (j.rotate_left(11) % 256) as u8,
            (j.rotate_left(23) % 256) as u8,
            a,
        ])
    })
}

/// `[Base, A, B (Multiply 0.6, masked), C]` at 8 bits, every raster
/// canvas-sized (the byte insert lands at the origin).
fn fixture8(dir: &TempDir) -> *mut DrDocument {
    let mut doc = doc_from(dir, "base.png", &varied8(W, H, 1, true));
    doc = add_layer(dir, "a.png", doc, 0, &varied8(W, H, 2, false), "A");
    doc = add_layer(dir, "b.png", doc, 1, &varied8(W, H, 3, false), "B");
    doc = add_layer(dir, "c.png", doc, 2, &varied8(W, H, 4, false), "C");
    doc = apply(doc, |d| unsafe {
        dr_doc_with_layer_blend_mode(d, 2, MULTIPLY)
    });
    doc = apply(doc, |d| unsafe { dr_doc_with_layer_opacity(d, 2, 0.6) });
    let mask: Vec<u8> = MASK
        .iter()
        .map(|&v| (u32::from(v) * 255 / 65535) as u8)
        .collect();
    apply(doc, |d| unsafe {
        dr_doc_set_mask(d, 2, mask.as_ptr(), W, H, 0, 0, true, 0)
    })
}

fn member8(doc: *const DrDocument, idx: usize, opacity: f64, mode: i32, mask: bool) -> Member {
    let (w, h) = layer_dims(doc, idx);
    let samples = layer_pixels(doc, idx)
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]].map(|v| f64::from(v) / 255.0))
        .collect();
    let mask = mask.then(|| {
        MASK.iter()
            .map(|&v| f64::from((u32::from(v) * 255 / 65535) as u8) / 255.0)
            .collect()
    });
    Member {
        samples,
        w,
        h,
        offset: layer_offset(doc, idx),
        opacity,
        mode,
        mask,
    }
}

fn assert_leaf8(doc: *const DrDocument, idx: usize, members: &[Member]) {
    let (w, h) = layer_dims(doc, idx);
    let (x0, y0) = layer_offset(doc, idx);
    let got = layer_pixels(doc, idx);
    for y in 0..h {
        for x in 0..w {
            let want = stack(
                members,
                i64::from(x0 + x as i32),
                i64::from(y0 + y as i32),
                [0.0; 4],
            );
            let at = ((y * w + x) * 4) as usize;
            let channels = if want[3] <= 0.0 { 3..4 } else { 0..4 };
            for c in channels {
                let expected = want[c].clamp(0.0, 1.0) * 255.0;
                assert!(
                    (f64::from(got[at + c]) - expected).abs() <= 1.0,
                    "8-bit leaf ({x}, {y}) channel {c}: {} vs {expected}",
                    got[at + c]
                );
            }
        }
    }
}

#[test]
fn stamp_selected_and_merge_group_at_8_bits() {
    let dir = TempDir::new().unwrap();
    let doc = fixture8(&dir);
    let members = [
        member8(doc, 1, 1.0, 0, false),
        member8(doc, 2, 0.6, MULTIPLY, true),
    ];
    let name = CString::new("B (merged)").unwrap();
    let idx = [2usize, 1];
    let stamped = unsafe { dr_doc_stamp_selected(doc, idx.as_ptr(), idx.len(), name.as_ptr()) };
    assert!(!stamped.is_null());
    assert_eq!(unsafe { dr_doc_layer_count(stamped) }, 5);
    assert_eq!(layer_name(stamped, 3), "B (merged)");
    assert_leaf8(stamped, 3, &members);
    assert_eq!(flat_pixels(doc).len(), flat_pixels(stamped).len());
    // One entry that is not a group, and a NULL name, refuse.
    let one = [1usize];
    assert!(unsafe { dr_doc_stamp_selected(doc, one.as_ptr(), 1, name.as_ptr()) }.is_null());
    assert!(unsafe { dr_doc_stamp_selected(doc, idx.as_ptr(), 2, ptr::null()) }.is_null());
    assert!(
        unsafe { dr_doc_merge_group(doc, 1) }.is_null(),
        "not a group"
    );
    for mode in [0, MULTIPLY] {
        let gname = CString::new("G").unwrap();
        let cap = 8usize;
        let (mut group, mut cleared, mut cleared_len, mut reordered, mut reordered_len) = (
            usize::MAX,
            vec![0usize; cap],
            0usize,
            vec![0usize; cap],
            0usize,
        );
        let pair = [1usize, 2];
        let grouped = unsafe {
            dr_doc_group_layers(
                doc,
                pair.as_ptr(),
                pair.len(),
                gname.as_ptr(),
                &mut group,
                cleared.as_mut_ptr(),
                &mut cleared_len,
                reordered.as_mut_ptr(),
                &mut reordered_len,
                cap,
            )
        };
        assert!(!grouped.is_null());
        let mut grouped = apply(grouped, |d| unsafe {
            dr_doc_with_layer_blend_mode(d, group, mode)
        });
        grouped = apply(grouped, |d| unsafe {
            dr_doc_with_layer_opacity(d, group, 0.7)
        });
        let before = flat_pixels(grouped);
        let merged = unsafe { dr_doc_merge_group(grouped, group) };
        assert!(!merged.is_null(), "merge group mode {mode}");
        assert_eq!(unsafe { dr_doc_layer_count(merged) }, 3);
        assert_eq!(layer_name(merged, 1), "G");
        assert_leaf8(merged, 1, &members);
        let after = flat_pixels(merged);
        for (i, (a, b)) in after.iter().zip(&before).enumerate() {
            assert!(
                a.abs_diff(*b) <= 1,
                "mode {mode}: flattened byte {i}: {a} vs {b}"
            );
        }
        unsafe {
            dr_doc_free(merged);
            dr_doc_free(grouped);
        }
    }
    unsafe {
        dr_doc_free(stamped);
        dr_doc_free(doc);
    }
}
