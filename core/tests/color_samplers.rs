//! COLOUR SAMPLERS as document state at 8, 16 and 32-bit float — the
//! setter's refusals, every canvas-geometry op at both depths, the flat-save
//! loss, the `.rz` words 59/60 and `CSMP` table, and PSD image resource 1073
//! both ways.
//!
//! The geometry oracles are written out here by hand: the integer pixel
//! permutation a quarter turn or a flip performs, the crop and canvas-size
//! shift with its drop, the resize scale of the pixel centre, and the
//! rotation of the centre about the canvas centre — none of them read from
//! the core. The 1073 layout is pinned against PHOTOSHOP-AUTHORED files
//! (`tests/fixtures/psd_photoshop/color_samplers/`, Photoshop 2025 26.10.0),
//! and the hand-built bodies below follow what those files and a Photoshop
//! read-back measured (`psd_samplers`' module doc).

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_preservation::dr_doc_flat_save_losses;
use doctored_core::ffi_psd::{
    dr_doc_open_psd, dr_doc_psd_export_report, dr_doc_save_psd, dr_precision_doc_open_psd,
    dr_precision_doc_psd_export_report, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::ffi_sampler::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 64;
const H: u32 = 48;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";
const PHOTOSHOP: &str = "tests/fixtures/psd_photoshop/color_samplers";

/// `x, y, sample_size`, as the FFI carries a sampler.
type Triple = (u32, u32, u32);

/// The four samplers every geometry case starts from: two corners, one
/// inside, one near the top edge.
const START: [Triple; 4] = [(0, 0, 1), (63, 47, 3), (10, 20, 5), (40, 5, 1)];

// ---------------------------------------------------------------- helpers --

fn byte(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_pixel(w, h, Rgba([200, 100, 50, 255])))
}

fn typed(w: u32, h: u32) -> PrecisionDocument {
    let pixels = vec![[12345u16, 23456, 34567, 65535]; (w * h) as usize];
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap()
}

fn float_document() -> PrecisionDocument {
    PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).expect("the float fixture")
}

fn flatten(list: &[Triple]) -> Vec<u32> {
    list.iter().flat_map(|&(x, y, s)| [x, y, s]).collect()
}

fn set_byte(doc: &DrDocument, list: &[Triple]) -> Option<DrDocument> {
    let raw = flatten(list);
    let out = unsafe { dr_doc_with_color_samplers(doc, raw.as_ptr(), list.len()) };
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

fn set_typed(doc: &PrecisionDocument, list: &[Triple]) -> Option<PrecisionDocument> {
    let raw = flatten(list);
    let out = unsafe { dr_precision_doc_with_color_samplers(doc, raw.as_ptr(), list.len()) };
    (!out.is_null()).then(|| *unsafe { Box::from_raw(out) })
}

fn byte_list(doc: &DrDocument) -> Vec<Triple> {
    (0..unsafe { dr_doc_color_sampler_count(doc) })
        .map(|i| {
            let mut out = [0u32; 3];
            assert!(unsafe { dr_doc_color_sampler(doc, i, out.as_mut_ptr()) });
            (out[0], out[1], out[2])
        })
        .collect()
}

fn typed_list(doc: &PrecisionDocument) -> Vec<Triple> {
    (0..unsafe { dr_precision_doc_color_sampler_count(doc) })
        .map(|i| {
            let mut out = [0u32; 3];
            assert!(unsafe { dr_precision_doc_color_sampler(doc, i, out.as_mut_ptr()) });
            (out[0], out[1], out[2])
        })
        .collect()
}

fn take(text: *mut c_char) -> Option<String> {
    if text.is_null() {
        return None;
    }
    let owned = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    Some(owned)
}

fn cpath(path: &Path) -> CString {
    CString::new(path.to_str().expect("utf-8 path")).expect("no NUL")
}

fn word(bytes: &[u8], index: usize) -> u32 {
    let at = 4 + 4 * index;
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn edit(request: Value) -> GeometryEdit {
    GeometryEdit::parse(&request.to_string()).expect("a geometry request")
}

/// The byte op through the request path every host op takes.
fn byte_geometry(doc: &DrDocument, request: Value) -> DrDocument {
    doc.geometry_edit(&edit(request), LIMIT, 0, None)
        .expect("the byte edit runs")
        .expect("the byte edit changes something")
}

fn typed_geometry(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    doc.geometry(&edit(request), LIMIT, 0, None)
        .expect("the typed edit runs")
        .expect("the typed edit changes something")
}

/// Keeps the samplers whose mapped pixel lies on a `w × h` canvas, in order.
fn kept(list: &[Triple], w: u32, h: u32, map: impl Fn(i64, i64) -> (i64, i64)) -> Vec<Triple> {
    list.iter()
        .filter_map(|&(x, y, s)| {
            let (nx, ny) = map(i64::from(x), i64::from(y));
            ((0..i64::from(w)).contains(&nx) && (0..i64::from(h)).contains(&ny))
                .then_some((nx as u32, ny as u32, s))
        })
        .collect()
}

/// The pixel a centre lands in under a float map, asserting it is not on a
/// pixel boundary (so the oracle and the core cannot round apart).
fn floored(v: f64) -> i64 {
    let frac = v - v.floor();
    assert!(
        frac > 1e-6 && frac < 1.0 - 1e-6,
        "{v} sits on a pixel boundary; move the sampler"
    );
    v.floor() as i64
}

// -------------------------------------------------------- the setter (T1) --

#[test]
fn the_setter_refuses_exactly_the_invalid_and_the_identical_list() {
    let b = byte(W, H);
    let t = typed(W, H);
    let refused: [&[Triple]; 5] = [
        &[(W, 0, 1)],
        &[(0, H, 1)],
        &[(1, 1, 2)],
        &[(1, 1, 256 + 1)],
        &[(0, 0, 1); 11],
    ];
    for list in refused {
        assert!(set_byte(&b, list).is_none(), "{list:?}");
        assert!(set_typed(&t, list).is_none(), "{list:?}");
    }
    // A NULL buffer with a count refuses; with no count it is the clear.
    assert!(unsafe { dr_doc_with_color_samplers(&b, ptr::null(), 2) }.is_null());
    assert!(unsafe { dr_precision_doc_with_color_samplers(&t, ptr::null(), 2) }.is_null());
    assert!(
        unsafe { dr_doc_with_color_samplers(&b, ptr::null(), 0) }.is_null(),
        "clearing an empty list is not an edit"
    );

    let ten: Vec<Triple> = (0..10).map(|i| (i, i, 1)).collect();
    let full = set_byte(&b, &ten).expect("ten fit");
    assert_eq!(byte_list(&full), ten);
    assert!(set_byte(&full, &ten).is_none(), "the identical list");
    let full16 = set_typed(&t, &ten).expect("ten fit at 16");
    assert_eq!(typed_list(&full16), ten);
    assert!(set_typed(&full16, &ten).is_none());

    let cleared = unsafe { dr_doc_with_color_samplers(&full, ptr::null(), 0) };
    assert!(!cleared.is_null());
    let cleared = unsafe { Box::from_raw(cleared) };
    assert!(byte_list(&cleared).is_empty());

    // The order is the caller's, the sizes are kept, and the input is pure.
    let list = [(5, 6, 5), (1, 2, 3), (5, 6, 5)];
    let set = set_byte(&b, &list).expect("set");
    assert_eq!(byte_list(&set), list);
    assert!(byte_list(&b).is_empty(), "the input document is untouched");
}

// ---------------------------------------------------- the geometry (T2) --

/// Every op, its request, its result canvas and the hand-written map.
/// One geometry case: its name, request, result canvas and expected list.
type GeometryCase = (&'static str, Value, (u32, u32), Vec<Triple>);

fn geometry_cases() -> Vec<GeometryCase> {
    let (wi, hi) = (i64::from(W), i64::from(H));
    vec![
        (
            "rotate90",
            json!({"op": "rotate90"}),
            (H, W),
            kept(&START, H, W, |x, y| (hi - 1 - y, x)),
        ),
        (
            "rotate180",
            json!({"op": "rotate180"}),
            (W, H),
            kept(&START, W, H, |x, y| (wi - 1 - x, hi - 1 - y)),
        ),
        (
            "rotate270",
            json!({"op": "rotate270"}),
            (H, W),
            kept(&START, H, W, |x, y| (y, wi - 1 - x)),
        ),
        (
            "flip_horizontal",
            json!({"op": "flip_horizontal"}),
            (W, H),
            kept(&START, W, H, |x, y| (wi - 1 - x, y)),
        ),
        (
            "flip_vertical",
            json!({"op": "flip_vertical"}),
            (W, H),
            kept(&START, W, H, |x, y| (x, hi - 1 - y)),
        ),
        (
            "crop",
            json!({"op": "crop", "x": 8, "y": 4, "width": 40, "height": 30}),
            (40, 30),
            kept(&START, 40, 30, |x, y| (x - 8, y - 4)),
        ),
        (
            "canvas_size",
            json!({"op": "canvas_size", "width": 50, "height": 40, "x": -10, "y": -5}),
            (50, 40),
            kept(&START, 50, 40, |x, y| (x - 10, y - 5)),
        ),
        (
            "resize",
            json!({"op": "resize", "width": 100, "height": 30, "filter": "bilinear"}),
            (100, 30),
            START
                .iter()
                .map(|&(x, y, s)| {
                    (
                        ((f64::from(x) + 0.5) * 100.0 / 64.0).floor() as u32,
                        ((f64::from(y) + 0.5) * 30.0 / 48.0).floor() as u32,
                        s,
                    )
                })
                .collect(),
        ),
    ]
}

#[test]
fn every_exact_geometry_op_moves_the_samplers_at_both_depths() {
    let b = set_byte(&byte(W, H), &START).expect("byte samplers");
    let t = set_typed(&typed(W, H), &START).expect("typed samplers");
    for (name, request, canvas, expected) in geometry_cases() {
        let moved = byte_geometry(&b, request.clone());
        assert_eq!((moved.width, moved.height), canvas, "{name}");
        assert_eq!(byte_list(&moved), expected, "byte {name}");
        let moved16 = typed_geometry(&t, request);
        assert_eq!(moved16.dimensions(), canvas, "{name}");
        assert_eq!(typed_list(&moved16), expected, "typed {name}");
    }
    // The drops are real: crop and canvas size each lose both corners.
    let crop = &geometry_cases()[5].3;
    assert_eq!(crop, &vec![(2, 16, 5), (32, 1, 1)]);
    let canvas = &geometry_cases()[6].3;
    assert_eq!(canvas, &vec![(0, 15, 5), (30, 0, 1)]);
    // The byte methods the host's flip/rotate items call agree with the
    // request path.
    assert_eq!(
        byte_list(&b.rotate90().expect("turn")),
        geometry_cases()[0].3
    );
    assert_eq!(
        byte_list(&b.crop(8, 4, 40, 30).expect("crop")),
        geometry_cases()[5].3
    );
}

#[test]
fn a_filtered_turn_rotates_the_samplers_with_the_content() {
    let start: [Triple; 3] = [(10, 20, 5), (40, 5, 1), (60, 44, 3)];
    let b = set_byte(&byte(W, H), &start).expect("byte samplers");
    let t = set_typed(&typed(W, H), &start).expect("typed samplers");
    let request = json!({"op": "rotate_canvas", "degrees": 10, "filter": "bilinear"});
    let moved = byte_geometry(&b, request.clone());
    let moved16 = typed_geometry(&t, request);
    let (nw, nh) = (moved.width, moved.height);
    assert_eq!(moved16.dimensions(), (nw, nh));
    assert!(nw > W && nh > H, "a filtered turn expands the canvas");
    // Positive degrees turn CLOCKWISE on a y-down canvas, about the centre;
    // the old canvas's top-left lands at (floor((W'-W)/2), floor((H'-H)/2)),
    // so the old centre lands that far from (W/2, H/2).
    let (s, c) = 10f64.to_radians().sin_cos();
    let centre = (
        f64::from(W) / 2.0 + (i64::from(nw) - i64::from(W)).div_euclid(2) as f64,
        f64::from(H) / 2.0 + (i64::from(nh) - i64::from(H)).div_euclid(2) as f64,
    );
    let expected: Vec<Triple> = start
        .iter()
        .filter_map(|&(x, y, size)| {
            let (dx, dy) = (
                f64::from(x) + 0.5 - f64::from(W) / 2.0,
                f64::from(y) + 0.5 - f64::from(H) / 2.0,
            );
            let px = floored(c * dx - s * dy + centre.0);
            let py = floored(s * dx + c * dy + centre.1);
            ((0..i64::from(nw)).contains(&px) && (0..i64::from(nh)).contains(&py))
                .then_some((px as u32, py as u32, size))
        })
        .collect();
    assert_eq!(expected.len(), 3, "a turn that grows the canvas drops none");
    assert_eq!(byte_list(&moved), expected);
    assert_eq!(typed_list(&moved16), expected);
}

#[test]
fn a_straighten_rotates_then_crops_the_samplers() {
    let start: [Triple; 4] = [(10, 20, 5), (40, 5, 1), (60, 44, 3), (1, 46, 1)];
    let b = set_byte(&byte(W, H), &start).expect("byte samplers");
    let t = set_typed(&typed(W, H), &start).expect("typed samplers");
    // A 3° turn about the canvas centre, CGAffineTransform order.
    let (s, c) = 3f64.to_radians().sin_cos();
    let (cx, cy) = (f64::from(W) / 2.0, f64::from(H) / 2.0);
    let m = [c, s, -s, c, cx - cx * c + cy * s, cy - cx * s - cy * c];
    let (wx, wy, ww, wh) = (3u32, 2u32, 58u32, 44u32);
    let request = json!({"op": "straighten", "matrix": m, "filter": "bilinear",
                         "x": wx, "y": wy, "width": ww, "height": wh});
    let expected: Vec<Triple> = start
        .iter()
        .filter_map(|&(x, y, size)| {
            let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let nx = floored(m[0] * px + m[2] * py + m[4] - f64::from(wx));
            let ny = floored(m[1] * px + m[3] * py + m[5] - f64::from(wy));
            ((0..i64::from(ww)).contains(&nx) && (0..i64::from(wh)).contains(&ny))
                .then_some((nx as u32, ny as u32, size))
        })
        .collect();
    assert_eq!(expected.len(), 3, "the window cuts the bottom-left sampler");
    let moved = byte_geometry(&b, request.clone());
    assert_eq!((moved.width, moved.height), (ww, wh));
    assert_eq!(byte_list(&moved), expected);
    let moved16 = typed_geometry(&t, request);
    assert_eq!(typed_list(&moved16), expected);
}

#[test]
fn a_crop_that_moves_only_a_sampler_is_still_an_edit_and_a_layer_transform_leaves_them() {
    // The crop window is the whole canvas minus one row on a document whose
    // only state below that row is a sampler: the typed fold counts it.
    let t = set_typed(&typed(W, H), &[(5, 47, 1)]).expect("set");
    let cropped = typed_geometry(
        &t,
        json!({"op": "crop", "x": 0, "y": 0, "width": 64, "height": 47}),
    );
    assert!(typed_list(&cropped).is_empty());
    // A per-layer transform moves nothing about the canvas.
    let b = set_byte(&byte(W, H), &START).expect("set");
    let moved = byte_geometry(
        &b,
        json!({"op": "affine", "layer": 0, "target": "pixels",
               "matrix": [1, 0, 0, 1, 5, 3], "filter": "nearest"}),
    );
    assert_eq!(byte_list(&moved), START);
}

// ------------------------------------------------- the flat save (T3) --

#[test]
fn a_flat_save_names_the_samplers_only_when_there_are_some() {
    let losses = |doc: &DrDocument| -> Vec<String> {
        let mut error = ptr::null_mut();
        let text = take(unsafe { dr_doc_flat_save_losses(doc, 0, true, false, &mut error) })
            .unwrap_or_else(|| panic!("{:?}", take(error)));
        serde_json::from_str(&text).expect("a JSON array")
    };
    let plain = byte(W, H);
    assert!(!losses(&plain).iter().any(|l| l == "color samplers"));
    let set = set_byte(&plain, &START).expect("set");
    assert!(losses(&set).iter().any(|l| l == "color samplers"));
}

// ------------------------------------------------------------- .rz (T4) --

fn csmp(list: &[Triple]) -> Vec<u8> {
    let mut table = b"CSMP".to_vec();
    table.extend(1u32.to_le_bytes());
    table.extend((list.len() as u32).to_le_bytes());
    for &(x, y, s) in list {
        table.extend(x.to_le_bytes());
        table.extend(y.to_le_bytes());
        table.push(s as u8);
    }
    table
}

fn lids_at(bytes: &[u8], records: usize) -> usize {
    let tail = bytes.len() - (12 + 8 * records);
    assert_eq!(&bytes[tail..tail + 4], b"LIDS", "LIDS stays LAST");
    tail
}

fn save_byte(dir: &TempDir, name: &str, doc: &DrDocument) -> Vec<u8> {
    let path = dir.path().join(name);
    doc.save_native(path.to_str().unwrap()).expect("save");
    std::fs::read(path).expect("read back")
}

fn open_byte(dir: &TempDir, name: &str, bytes: &[u8]) -> Result<DrDocument, String> {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("write");
    DrDocument::open(path.to_str().unwrap())
}

#[test]
fn byte_rz_writes_word_59_and_csmp_only_for_samplers() {
    let dir = TempDir::new().unwrap();
    let base = byte(W, H);
    let plain = save_byte(&dir, "plain.rz", &base);
    assert!(!plain.windows(4).any(|w| w == b"CSMP"));
    assert_ne!(word(&plain, 1), 59);

    let set = set_byte(&base, &START).expect("set");
    let bytes = save_byte(&dir, "set.rz", &set);
    assert_eq!((word(&bytes, 0), word(&bytes, 1)), (45, 59));
    let tail = lids_at(&bytes, 1);
    let table = csmp(&START);
    assert_eq!(
        &bytes[tail - table.len()..tail],
        &table[..],
        "CSMP sits right before LIDS"
    );
    let reopened = open_byte(&dir, "again.rz", &bytes).expect("reopen");
    assert_eq!(byte_list(&reopened), START);
    assert_eq!(
        save_byte(&dir, "resave.rz", &reopened),
        bytes,
        "reopen → resave is byte-identical"
    );

    let cleared = set_byte(&set, &[]).expect("clear");
    assert_eq!(
        save_byte(&dir, "cleared.rz", &cleared),
        plain,
        "clearing the last sampler restores the sampler-free bytes"
    );
}

#[test]
fn a_crafted_csmp_table_refuses_or_drops() {
    let dir = TempDir::new().unwrap();
    let set = set_byte(&byte(W, H), &START).expect("set");
    let good = save_byte(&dir, "good.rz", &set);
    let at = good.windows(4).position(|w| w == b"CSMP").expect("a table");
    let patched = |offset: usize, value: &[u8]| {
        let mut bytes = good.clone();
        bytes[at + offset..at + offset + value.len()].copy_from_slice(value);
        bytes
    };
    for (bytes, what, sentence) in [
        (patched(0, b"XXXX"), "a bad tag", "tag or schema"),
        (
            patched(4, &2u32.to_le_bytes()),
            "a bad schema",
            "tag or schema",
        ),
        (patched(8, &0u32.to_le_bytes()), "a zero count", "count"),
        (patched(8, &11u32.to_le_bytes()), "eleven", "count"),
        (patched(12 + 8, &[2]), "a size of 2", "sample size"),
    ] {
        let message = match open_byte(&dir, "bad.rz", &bytes) {
            Err(message) => message,
            Ok(_) => panic!("{what} must refuse"),
        };
        assert!(message.contains(sentence), "{what}: {message}");
    }
    // An entry off the canvas is DROPPED, never refused.
    let off = patched(12, &W.to_le_bytes());
    let repaired = open_byte(&dir, "off.rz", &off).expect("repaired");
    assert_eq!(byte_list(&repaired), START[1..].to_vec());
}

#[test]
fn typed_rz_writes_word_60_at_16_bits_and_at_float() {
    for (name, base) in [("16", typed(W, H)), ("float", float_document())] {
        let plain = base.encode_native(LIMIT, 0, None).expect("plain");
        assert!(!plain.windows(4).any(|w| w == b"CSMP"), "{name}");
        assert_ne!(word(&plain, 1), 60, "{name}");
        let (w, h) = base.dimensions();
        let list = [(0, 0, 1), (w - 1, h - 1, 5), (w / 2, h / 3, 3)];
        let set = set_typed(&base, &list).expect("set");
        let bytes = set.encode_native(LIMIT, 0, None).expect("bytes");
        assert_eq!((word(&bytes, 0), word(&bytes, 1)), (46, 60), "{name}");
        let tail = lids_at(&bytes, set.layer_count());
        let table = csmp(&list);
        assert_eq!(&bytes[tail - table.len()..tail], &table[..], "{name}");
        let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
        assert_eq!(typed_list(&reopened), list, "{name}");
        assert_eq!(
            reopened.encode_native(LIMIT, 0, None).expect("again"),
            bytes,
            "{name}"
        );
        let cleared = set_typed(&set, &[]).expect("clear");
        assert_eq!(
            cleared.encode_native(LIMIT, 0, None).expect("cleared"),
            plain,
            "{name}"
        );
        // A bad count refuses the typed open too.
        let at = bytes.windows(4).position(|w| w == b"CSMP").unwrap();
        let mut bad = bytes.clone();
        bad[at + 8..at + 12].copy_from_slice(&0u32.to_le_bytes());
        assert!(
            PrecisionDocument::parse_native(&bad, LIMIT, 0, None).is_err(),
            "{name}"
        );
    }
}

#[test]
fn the_samplers_word_combines_with_the_light_word() {
    let lit = {
        let raw = set_typed(&typed(W, H), &START).expect("set");
        let request = json!({"op": "global_light", "angle": 45, "altitude": 60}).to_string();
        let mut error = ptr::null_mut();
        let request = CString::new(request).unwrap();
        let out = unsafe {
            doctored_core::ffi_precision_edit::dr_precision_doc_edit(
                &raw,
                request.as_ptr(),
                ptr::null(),
                0,
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(!out.is_null(), "{:?}", take(error));
        *unsafe { Box::from_raw(out) }
    };
    let bytes = lit.encode_native(LIMIT, 0, None).expect("bytes");
    assert_eq!(
        (word(&bytes, 0), word(&bytes, 1), word(&bytes, 2)),
        (46, 60, 58)
    );
    let tail = lids_at(&bytes, 1);
    let table = csmp(&START);
    assert_eq!(&bytes[tail - table.len()..tail], &table[..]);
    assert_eq!(
        &bytes[tail - table.len() - 16..tail - table.len() - 12],
        b"LGLT"
    );
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("reopen");
    assert_eq!(typed_list(&reopened), START);
}

/// Word 59 beside `source_refusal_native`'s word 56 in one byte file: 59 is
/// peeled first (`45, 59, 56`), and the CSMP table follows RSPR, last before
/// LIDS.
#[test]
fn the_samplers_word_combines_with_the_source_refusal_word() {
    let dir = TempDir::new().unwrap();
    const SENTENCE: &str = "The embedded ICC profile is not supported; opened as sRGB.";
    let base = DrDocument::from_pixels(RgbaImage::from_pixel(9, 7, Rgba([10, 20, 30, 255])));
    let master = DrDocument::from_pixels(RgbaImage::from_pixel(5, 4, Rgba([90, 80, 70, 255])));
    let place = doctored_core::retained_source::SourceEdit::parse(
        &json!({"op": "place", "source_id": "samplers-master", "revision_id": "first",
            "name": "Placed", "x": 2, "y": 1, "profile_refusal": SENTENCE})
        .to_string(),
    )
    .expect("a place request");
    let placed = base
        .source_edit(Some(&master), &place, LIMIT, 0, None)
        .expect("the place runs")
        .expect("the place changes something");
    let list = [(0, 0, 1), (8, 6, 3), (4, 3, 5)];
    let set = set_byte(&placed, &list).expect("set");
    let bytes = save_byte(&dir, "both.rz", &set);
    assert_eq!(
        (word(&bytes, 0), word(&bytes, 1), word(&bytes, 2)),
        (45, 59, 56)
    );
    let tail = lids_at(&bytes, set.layers.len());
    let table = csmp(&list);
    assert_eq!(&bytes[tail - table.len()..tail], &table[..]);
    let mut rspr = b"RSPR".to_vec();
    rspr.extend(1u32.to_le_bytes());
    rspr.extend(1u32.to_le_bytes());
    rspr.extend(0u32.to_le_bytes());
    rspr.extend((SENTENCE.len() as u32).to_le_bytes());
    rspr.extend(SENTENCE.as_bytes());
    let at = tail - table.len() - rspr.len();
    assert_eq!(
        &bytes[at..tail - table.len()],
        &rspr[..],
        "RSPR → CSMP → LIDS"
    );
    let reopened = open_byte(&dir, "again.rz", &bytes).expect("reopen");
    assert_eq!(byte_list(&reopened), list);
    assert_eq!(save_byte(&dir, "resave.rz", &reopened), bytes);
}

#[test]
fn conversion_and_profile_edits_carry_the_samplers() {
    let b = set_byte(&byte(W, H), &START).expect("set");
    let promoted = {
        let mut error = ptr::null_mut();
        let out = unsafe {
            doctored_core::ffi_precision_conversion::dr_doc_promote_structure(
                &b,
                LIMIT,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(!out.is_null(), "{:?}", take(error));
        unsafe { Box::from_raw(out) }
    };
    assert_eq!(typed_list(&promoted), START);
    let flattened = b.flattening().expect("flatten");
    assert_eq!(byte_list(&flattened), START);
}

// ------------------------------------------------------------- PSD (T5) --

/// A PSD with image resource `id` set to `body` (replacing any), resources
/// walked by Adobe's framing: `8BIM`, u16 id, padded Pascal name, u32 length,
/// body padded to even.
fn with_resource(bytes: &[u8], id: u16, body: &[u8]) -> Vec<u8> {
    let be32 = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mode = 26 + 4 + be32(26);
    let length = be32(mode);
    let section = &bytes[mode + 4..mode + 4 + length];
    let mut rebuilt = Vec::new();
    let mut i = 0;
    while i < section.len() {
        let rid = u16::from_be_bytes([section[i + 4], section[i + 5]]);
        let name = (section[i + 6] as usize + 2) & !1;
        let size_at = i + 6 + name;
        let size = u32::from_be_bytes(section[size_at..size_at + 4].try_into().unwrap()) as usize;
        let end = size_at + 4 + size + (size & 1);
        if rid != id {
            rebuilt.extend_from_slice(&section[i..end]);
        }
        i = end;
    }
    rebuilt.extend(b"8BIM");
    rebuilt.extend(id.to_be_bytes());
    rebuilt.extend([0, 0]);
    rebuilt.extend((body.len() as u32).to_be_bytes());
    rebuilt.extend(body);
    if body.len() % 2 == 1 {
        rebuilt.push(0);
    }
    let mut out = bytes[..mode].to_vec();
    out.extend((rebuilt.len() as u32).to_be_bytes());
    out.extend(rebuilt);
    out.extend(&bytes[mode + 4 + length..]);
    out
}

/// The body of image resource `id`, by Adobe's framing.
fn resource(bytes: &[u8], id: u16) -> Option<Vec<u8>> {
    let be32 = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mode = 26 + 4 + be32(26);
    let length = be32(mode);
    let section = &bytes[mode + 4..mode + 4 + length];
    let mut i = 0;
    while i < section.len() {
        let rid = u16::from_be_bytes([section[i + 4], section[i + 5]]);
        let name = (section[i + 6] as usize + 2) & !1;
        let size_at = i + 6 + name;
        let size = u32::from_be_bytes(section[size_at..size_at + 4].try_into().unwrap()) as usize;
        if rid == id {
            return Some(section[size_at + 4..size_at + 4 + size].to_vec());
        }
        i = size_at + 4 + size + (size & 1);
    }
    None
}

fn report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let text = take(unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) })
        .unwrap_or_else(|| panic!("{:?}", take(error)));
    serde_json::from_str(&text).expect("report JSON")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|v| v.as_str().expect("a string").to_string())
        .collect()
}

fn open_psd8(path: &Path) -> DrDocument {
    let fingerprint = CString::new(report(path)["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let doc = unsafe { dr_doc_open_psd(cpath(path).as_ptr(), 0, fingerprint.as_ptr(), &mut error) };
    assert!(!doc.is_null(), "{:?}", take(error));
    *unsafe { Box::from_raw(doc) }
}

fn open_psd16(path: &Path) -> PrecisionDocument {
    let fingerprint = CString::new(report(path)["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            0,
            fingerprint.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!doc.is_null(), "{:?}", take(error));
    *unsafe { Box::from_raw(doc) }
}

fn save_psd8(doc: &DrDocument, path: &Path) {
    let mut error = ptr::null_mut();
    let ok = unsafe { dr_doc_save_psd(doc, ptr::null(), cpath(path).as_ptr(), 0, 0, &mut error) };
    assert!(ok, "{:?}", take(error));
}

fn save_psd16(doc: &PrecisionDocument, path: &Path) {
    let projection = doc.render(LIMIT, 0, None).expect("projection");
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            &projection,
            cpath(path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            &mut error,
        )
    };
    assert!(ok, "{:?}", take(error));
}

/// The 1073 body this module writes for `list`: version 2, f32 vertical then
/// horizontal (the pixel index), colour space −1, depth 8.
fn v2_body(list: &[(u32, u32)]) -> Vec<u8> {
    let mut body = 2u32.to_be_bytes().to_vec();
    body.extend((list.len() as u32).to_be_bytes());
    for &(x, y) in list {
        body.extend((y as f32).to_be_bytes());
        body.extend((x as f32).to_be_bytes());
        body.extend((-1i16).to_be_bytes());
        body.extend(8i16.to_be_bytes());
    }
    body
}

#[test]
fn photoshop_written_samplers_decode_to_their_pixels_at_8_and_16_bits() {
    let placed = vec![(5, 7, 1), (40, 3, 1), (12, 30, 1), (63, 47, 1)];
    let eight = Path::new(PHOTOSHOP).join("photoshop_8.psd");
    let sixteen = Path::new(PHOTOSHOP).join("photoshop_16.psd");
    // What Photoshop 2025 writes: version 3, 14-byte records — a u16 14, then
    // f32 VERTICAL, f32 HORIZONTAL (the pixel index), colour space −1, depth 8
    // at BOTH file depths.
    for path in [&eight, &sixteen] {
        let body = resource(&std::fs::read(path).unwrap(), 1073).expect("1073");
        assert_eq!(&body[..8], &[0, 0, 0, 3, 0, 0, 0, 4], "{path:?}");
        assert_eq!(body.len(), 8 + 4 * 14);
        assert_eq!(&body[8..10], &14u16.to_be_bytes());
        assert_eq!(&body[10..14], &7f32.to_be_bytes(), "vertical first");
        assert_eq!(&body[14..18], &5f32.to_be_bytes());
        assert_eq!(&body[18..22], &[0xff, 0xff, 0, 8]);
    }
    assert_eq!(byte_list(&open_psd8(&eight)), placed);
    assert_eq!(typed_list(&open_psd16(&sixteen)), placed);
    // Photoshop's own default samplers add only the point-size observation.
    let r = report(&eight);
    assert!(
        !strings(&r["verified_losses"])
            .iter()
            .any(|l| l.contains("color sampler")),
        "{r}"
    );
    assert!(strings(&r["observations"])
        .iter()
        .any(|o| o.starts_with("4 color samplers are imported at a point")));
}

#[test]
fn a_rasterize_psd_round_trips_its_samplers_at_8_and_16_bits() {
    let dir = TempDir::new().unwrap();
    let list = [(5u32, 7u32), (40, 3), (63, 47)];
    let triples: Vec<Triple> = list.iter().map(|&(x, y)| (x, y, 1)).collect();
    let b = set_byte(&byte(W, H), &triples).expect("set");
    let p8 = dir.path().join("set8.psd");
    save_psd8(&b, &p8);
    assert_eq!(
        resource(&std::fs::read(&p8).unwrap(), 1073),
        Some(v2_body(&list))
    );
    assert_eq!(byte_list(&open_psd8(&p8)), triples);

    let t = set_typed(&typed(W, H), &triples).expect("set");
    let p16 = dir.path().join("set16.psd");
    save_psd16(&t, &p16);
    assert_eq!(
        resource(&std::fs::read(&p16).unwrap(), 1073),
        Some(v2_body(&list))
    );
    assert_eq!(typed_list(&open_psd16(&p16)), triples);

    // A sampler-free export writes no 1073 at all.
    let plain = dir.path().join("plain8.psd");
    save_psd8(&byte(W, H), &plain);
    assert_eq!(resource(&std::fs::read(&plain).unwrap(), 1073), None);
}

/// `rasterize_8.psd` / `rasterize_16.psd` in the Photoshop family are THIS
/// writer's files, opened in Photoshop 2025 to read `colorSamplers[i]
/// .position` back (`reopen_8.json` / `reopen_16.json`, the `report` job).
/// The check here keeps that verdict tied to the writer: their 1073 bodies
/// must be what it writes today. `DR_SAMPLER_PSD_FIXTURES=<dir>` regenerates
/// them.
#[test]
fn the_photoshop_checked_rasterize_files_carry_this_writers_1073() {
    let list = [(5u32, 7u32), (40, 3), (12, 30), (63, 47)];
    let triples: Vec<Triple> = list.iter().map(|&(x, y)| (x, y, 1)).collect();
    if let Ok(out) = std::env::var("DR_SAMPLER_PSD_FIXTURES") {
        let out = Path::new(&out);
        save_psd8(
            &set_byte(&byte(W, H), &triples).unwrap(),
            &out.join("rasterize_8.psd"),
        );
        save_psd16(
            &set_typed(&typed(W, H), &triples).unwrap(),
            &out.join("rasterize_16.psd"),
        );
    }
    for name in ["rasterize_8.psd", "rasterize_16.psd"] {
        let bytes = std::fs::read(Path::new(PHOTOSHOP).join(name)).expect(name);
        assert_eq!(resource(&bytes, 1073), Some(v2_body(&list)), "{name}");
    }
}

#[test]
fn the_export_preflight_reports_a_lost_sample_size() {
    let sentence = "Color sampler sample sizes are not written; Photoshop reads every sampler at its own Sample Size.";
    let byte_report = |doc: &DrDocument| -> Value {
        let mut error = ptr::null_mut();
        let text = take(unsafe { dr_doc_psd_export_report(doc, 0, 0, &mut error) })
            .unwrap_or_else(|| panic!("{:?}", take(error)));
        serde_json::from_str(&text).unwrap()
    };
    let typed_report = |doc: &PrecisionDocument| -> Value {
        let mut error = ptr::null_mut();
        let text = take(unsafe { dr_precision_doc_psd_export_report(doc, 0, 1, &mut error) })
            .unwrap_or_else(|| panic!("{:?}", take(error)));
        serde_json::from_str(&text).unwrap()
    };
    let points = set_byte(&byte(W, H), &[(1, 1, 1)]).expect("set");
    assert!(!strings(&byte_report(&points)["losses"])
        .iter()
        .any(|l| l == sentence));
    let sized = set_byte(&byte(W, H), &[(1, 1, 1), (2, 2, 3)]).expect("set");
    assert!(strings(&byte_report(&sized)["losses"])
        .iter()
        .any(|l| l == sentence));
    let sized16 = set_typed(&typed(W, H), &[(2, 2, 5)]).expect("set");
    assert!(strings(&typed_report(&sized16)["losses"])
        .iter()
        .any(|l| l == sentence));
}

/// One hand-built 1073 case: the body, the samplers it imports and the
/// sentence fragments its report must carry.
struct Case {
    name: &'static str,
    body: Vec<u8>,
    kept: Vec<Triple>,
    losses: Vec<&'static str>,
    observations: Vec<&'static str>,
}

fn record_v2(x: f32, y: f32, space: i16) -> Vec<u8> {
    let mut r = y.to_be_bytes().to_vec();
    r.extend(x.to_be_bytes());
    r.extend(space.to_be_bytes());
    r.extend(8i16.to_be_bytes());
    r
}

fn body(version: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut b = version.to_be_bytes().to_vec();
    b.extend((records.len() as u32).to_be_bytes());
    for r in records {
        b.extend(r);
    }
    b
}

fn cases() -> Vec<Case> {
    let two = vec![(5, 7, 1), (40, 3, 1)];
    let v1 = |x: i32, y: i32| {
        let mut r = (y * 32).to_be_bytes().to_vec();
        r.extend((x * 32).to_be_bytes());
        r.extend(0i16.to_be_bytes());
        r
    };
    let v3_14 = |x: f32, y: f32| {
        let mut r = 14u16.to_be_bytes().to_vec();
        r.extend(record_v2(x, y, -1));
        r
    };
    let v3_16 = |x: f32, y: f32| {
        let mut r = 1u32.to_be_bytes().to_vec();
        r.extend(record_v2(x, y, -1));
        r
    };
    let mut short = body(2, &[record_v2(5.0, 7.0, 0), record_v2(40.0, 3.0, 0)]);
    short[7] = 3;
    vec![
        Case {
            name: "v1, 1/32 pixel",
            body: body(1, &[v1(5, 7), v1(40, 3)]),
            kept: two.clone(),
            losses: vec![],
            observations: vec!["2 color samplers are imported at a point"],
        },
        Case {
            name: "v2",
            body: body(2, &[record_v2(5.0, 7.0, 0), record_v2(40.75, 3.25, -1)]),
            kept: two.clone(),
            losses: vec![],
            observations: vec![],
        },
        Case {
            name: "v3, 14-byte records",
            body: body(3, &[v3_14(5.0, 7.0), v3_14(40.0, 3.0)]),
            kept: two.clone(),
            losses: vec![],
            observations: vec![],
        },
        Case {
            name: "v3, 16-byte records",
            body: body(3, &[v3_16(5.0, 7.0), v3_16(40.0, 3.0)]),
            kept: two.clone(),
            losses: vec![],
            observations: vec![],
        },
        Case {
            name: "a length mismatch",
            body: short,
            kept: vec![],
            losses: vec!["a length that matches none of its record layouts"],
            observations: vec![],
        },
        Case {
            name: "an unknown version",
            body: body(4, &[record_v2(5.0, 7.0, 0)]),
            kept: vec![],
            losses: vec!["has version 4, which Doctored does not read"],
            observations: vec![],
        },
        Case {
            name: "more than ten",
            body: body(2, &(0..12).map(|i| record_v2(i as f32, 1.0, 0)).collect::<Vec<_>>()),
            kept: (0..10).map(|i| (i, 1, 1)).collect(),
            losses: vec!["has 12 color samplers on the canvas; Doctored keeps the first 10 and drops the other 2"],
            observations: vec![],
        },
        Case {
            name: "off the canvas",
            body: body(
                2,
                &[
                    record_v2(64.0, 1.0, 0),
                    record_v2(f32::NAN, 1.0, 0),
                    record_v2(-0.5, 1.0, 0),
                    record_v2(3.0, 4.0, 0),
                ],
            ),
            kept: vec![(3, 4, 1)],
            losses: vec!["3 Photoshop color samplers lie outside the canvas"],
            observations: vec![],
        },
        Case {
            name: "colour space 7",
            body: body(2, &[record_v2(5.0, 7.0, 7)]),
            kept: vec![(5, 7, 1)],
            losses: vec![],
            observations: vec!["Color sampler #1 reads in Photoshop color mode 7"],
        },
    ]
}

#[test]
fn hand_built_1073_bodies_import_with_their_report_sentences() {
    let dir = TempDir::new().unwrap();
    let base_path = dir.path().join("base.psd");
    save_psd8(&byte(W, H), &base_path);
    let base = std::fs::read(&base_path).unwrap();
    for case in cases() {
        let path = dir.path().join("case.psd");
        std::fs::write(&path, with_resource(&base, 1073, &case.body)).unwrap();
        let r = report(&path);
        let losses = strings(&r["verified_losses"]);
        let observations = strings(&r["observations"]);
        for fragment in &case.losses {
            assert!(
                losses.iter().any(|l| l.contains(fragment)),
                "{}: {losses:?}",
                case.name
            );
        }
        for fragment in &case.observations {
            assert!(
                observations.iter().any(|o| o.contains(fragment)),
                "{}: {observations:?}",
                case.name
            );
        }
        if case.losses.is_empty() {
            assert!(
                !losses.iter().any(|l| l.contains("color sampler")),
                "{}: {losses:?}",
                case.name
            );
        }
        assert_eq!(byte_list(&open_psd8(&path)), case.kept, "{}", case.name);
        // The stored-composite choice carries them too.
        let fingerprint = CString::new(r["source_fingerprint"].as_str().unwrap()).unwrap();
        let mut error = ptr::null_mut();
        let composite =
            unsafe { dr_doc_open_psd(cpath(&path).as_ptr(), 1, fingerprint.as_ptr(), &mut error) };
        assert!(!composite.is_null(), "{:?}", take(error));
        let composite = unsafe { Box::from_raw(composite) };
        assert_eq!(byte_list(&composite), case.kept, "{} composite", case.name);
    }
}

#[test]
fn an_obsolete_1038_alone_is_an_observation() {
    let dir = TempDir::new().unwrap();
    let base_path = dir.path().join("base.psd");
    save_psd8(&byte(W, H), &base_path);
    let base = std::fs::read(&base_path).unwrap();
    let mut old = 1u32.to_be_bytes().to_vec();
    old.extend(1u32.to_be_bytes());
    old.extend((7i32 * 32).to_be_bytes());
    old.extend((5i32 * 32).to_be_bytes());
    old.extend((-1i16).to_be_bytes());
    let path = dir.path().join("old.psd");
    std::fs::write(&path, with_resource(&base, 1038, &old)).unwrap();
    let r = report(&path);
    assert!(strings(&r["observations"])
        .iter()
        .any(|o| o.contains("obsolete color sampler resource (1038)")));
    assert!(byte_list(&open_psd8(&path)).is_empty());
}

// --------------------------------------------------------- NULL safety --

#[test]
fn every_export_tolerates_null() {
    let mut out = [0u32; 3];
    unsafe {
        assert_eq!(dr_doc_color_sampler_count(ptr::null()), 0);
        assert!(!dr_doc_color_sampler(ptr::null(), 0, out.as_mut_ptr()));
        assert!(dr_doc_with_color_samplers(ptr::null(), ptr::null(), 0).is_null());
        assert_eq!(dr_precision_doc_color_sampler_count(ptr::null()), 0);
        assert!(!dr_precision_doc_color_sampler(
            ptr::null(),
            0,
            out.as_mut_ptr()
        ));
        assert!(dr_precision_doc_with_color_samplers(ptr::null(), ptr::null(), 0).is_null());
    }
    let b = set_byte(&byte(W, H), &START).expect("set");
    let t = set_typed(&typed(W, H), &START).expect("set");
    unsafe {
        assert!(!dr_doc_color_sampler(&b, 0, ptr::null_mut()));
        assert!(
            !dr_doc_color_sampler(&b, 4, out.as_mut_ptr()),
            "a bad index"
        );
        assert!(!dr_precision_doc_color_sampler(&t, 0, ptr::null_mut()));
        assert!(!dr_precision_doc_color_sampler(&t, 4, out.as_mut_ptr()));
    }
}
