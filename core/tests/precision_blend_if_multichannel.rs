//! Blend If on several channels at once on typed documents:
//! the 16-bit, 32-bit float and CMYK 8/16 weights of several ranges against
//! the oracles below; `layer_blend_if` and `layer_style` with `extra`, and the
//! CMYK mode refusal of an RGB range AS WRITTEN (an identity Red in `extra`,
//! an identity Red primary beside a Cyan extra), with Gray + Cyan accepted on
//! an EMPTY isolated group of a CMYK8 document; the typed `.rz` word 101 right
//! after the CMYK slot with `LBIF` schema 2 (round trip, both refusals, a
//! comp-only claim, every schema 2 refusal); words 101 then 100 (a wide
//! style colour) together in one file, byte 8-bit, typed 16-bit and CMYK8
//! (order, round trip, a missing 100, the pair swapped); the layered PSD at 16 bits and
//! on CMYK (slots 0 and 1 written, both ranges read back); and the typed
//! layer clipboard.
//!
//! THE ORACLES (this file's restatement of the spec, not the crate's code).
//! Every range is weighted as at one channel — its This Layer ramp on the
//! layer's pixel times its Underlying ramp on the composite beneath — and
//! several ranges MULTIPLY. At 16 bits a channel reads `round(65535 · v)`
//! (gray `v = 0.30 r + 0.59 g + 0.11 b`) against knees `257 k`; at 32 bits
//! the stored linear value `× 65535`, unrounded, against knees `257 k` whose
//! zero ends sit `65535 / 32768` further out (a full ramp weights 1); on
//! CMYK each ink is its stored code, a plate reads `code_max − code`, Gray
//! the luminosity of the codes' naive RGB, and the knees move by INC (HALF on
//! Yellow) and scale by `code_max / 255` (`cmyk_blend_if`'s closed form).

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_precision::dr_precision_doc_info;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_psd_import::open_psd;
use crate::cmyk_structure_fixture::{free, then, try_edit};
use crate::cmyk_style_fixture::{backdrop, flat, layer, paint, sample};
use crate::common::{cpath, rect_layer_doc, styled, take_err_string};
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FULL: [u8; 4] = [0, 0, 255, 255];

/// One range: `(channel, this, underlying)`.
type Range = (&'static str, [u8; 4], [u8; 4]);

fn range_json((channel, this, under): Range) -> Value {
    json!({"channel": channel, "this_layer": this, "underlying": under})
}

/// The `blend_if` value: the first range's keys plus `extra`.
fn blend_if_of(ranges: &[Range]) -> Value {
    let mut b = range_json(ranges[0]);
    if ranges.len() > 1 {
        b["extra"] = Value::Array(ranges[1..].iter().map(|r| range_json(*r)).collect());
    }
    b
}

fn blend_if_request(layer: usize, blend_if: &Value) -> String {
    json!({"op":"layer_blend_if","version":1,"layer":layer,"blend_if":blend_if}).to_string()
}

fn style_request(layer: usize, blend_if: &Value) -> String {
    json!({"op":"layer_style","version":1,"layer":layer,"style":{"blend_if":blend_if}}).to_string()
}

fn ramp_at(v: f64, k: [f64; 4]) -> f64 {
    if v < k[0] {
        0.0
    } else if v < k[1] {
        (v - k[0]) / (k[1] - k[0])
    } else if v <= k[2] {
        1.0
    } else if v <= k[3] {
        (k[3] - v) / (k[3] - k[2])
    } else {
        0.0
    }
}

fn unit(channel: &str, rgb: [f64; 3]) -> f64 {
    match channel {
        "gray" => 0.30 * rgb[0] + 0.59 * rgb[1] + 0.11 * rgb[2],
        "red" => rgb[0],
        "green" => rgb[1],
        "blue" => rgb[2],
        other => panic!("channel {other}"),
    }
}

fn info(doc: *const PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_info(doc, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    let text = unsafe { std::ffi::CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(out) };
    serde_json::from_str(&text).unwrap()
}

fn layer_blend_if(doc: *const PrecisionDocument, index: usize) -> Value {
    info(doc)["layers"][index]["blend_if"].clone()
}

// ------------------------------------------------------------ 16 bits --

const W: u32 = 32;
const H: u32 = 8;

/// R the x ramp, G = B the y ramp, each a few codes off the 8-bit grid.
fn source16(x: u32, y: u32) -> [u16; 4] {
    let r = (x * 2048 + 37) as u16;
    let g = (y * 8192 + 11) as u16;
    [r, g, g, 65535]
}

fn rgb16_doc() -> *mut PrecisionDocument {
    let image = |f: &dyn Fn(u32, u32) -> [u16; 4]| {
        let px = (0..W * H).map(|i| f(i % W, i / W)).collect();
        PrecisionImage::from_rgba16(W, H, px, IccProfile::srgb(), None).unwrap()
    };
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Black", image(&|_, _| [0, 0, 0, 65535]), (0, 0), 1.0, true).unwrap(),
            Layer::raster("Source", image(&source16), (0, 0), 1.0, true).unwrap(),
        ],
    )
    .unwrap();
    Box::into_raw(Box::new(doc))
}

fn weight16(ranges: &[Range], src: [u16; 4], under: [u16; 4]) -> f64 {
    let read = |c: &str, p: [u16; 4]| {
        (65535.0 * unit(c, [0, 1, 2].map(|i| f64::from(p[i]) / 65535.0))).round()
    };
    let knees = |r: [u8; 4]| r.map(|k| f64::from(k) * 257.0);
    ranges
        .iter()
        .map(|(c, this, below)| {
            ramp_at(read(c, src), knees(*this)) * ramp_at(read(c, under), knees(*below))
        })
        .product()
}

#[test]
fn sixteen_bit_ranges_multiply_against_the_oracle() {
    let base = rgb16_doc();
    for ranges in [
        vec![
            ("gray", [0, 0, 96, 192], FULL),
            ("red", [0, 0, 64, 224], FULL),
        ],
        vec![
            ("green", [10, 70, 200, 250], FULL),
            ("blue", [0, 0, 255, 255], [0, 0, 255, 255]),
            ("red", [30, 90, 180, 250], FULL),
        ],
    ] {
        let doc = then(
            clone_of(base),
            &blend_if_request(1, &blend_if_of(&ranges)),
            &[],
        );
        let rendered = unsafe { &*doc }
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap();
        let mut differs = false;
        for (i, got) in rendered.iter().enumerate() {
            let (x, y) = (i as u32 % W, i as u32 / W);
            let src = source16(x, y);
            let w = weight16(&ranges, src, [0, 0, 0, 65535]);
            let min = ranges
                .iter()
                .map(|r| weight16(&[*r], src, [0, 0, 0, 65535]))
                .fold(1.0, f64::min);
            differs |= (w - min).abs() > 0.02;
            for c in 0..3 {
                let want = w * f64::from(src[c]);
                assert!(
                    (f64::from(got[c]) - want).abs() <= 2.0,
                    "{ranges:?} at ({x}, {y}) channel {c}: got {}, want {want} (w {w})",
                    got[c]
                );
            }
        }
        assert!(differs, "the scene separates PRODUCT from MIN");
        free(doc);
    }
    free(base);
}

/// An owned copy of `doc` as a fresh handle.
fn clone_of(doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    Box::into_raw(Box::new(unsafe { &*doc }.clone()))
}

// ----------------------------------------------------------- 32 bits --

fn float_side(v: f64, ramp: [u8; 4]) -> f64 {
    if ramp == FULL {
        return 1.0;
    }
    let e = 65535.0 / 32768.0;
    let k = ramp.map(|k| f64::from(k) * 257.0);
    ramp_at(v * 65535.0, [k[0] - e, k[1], k[2], k[3] + e])
}

#[test]
fn thirty_two_bit_ranges_multiply_on_the_stored_linear_value() {
    let srgb = IccProfile::srgb();
    let (w, h) = (24u32, 6u32);
    let source = |x: u32, y: u32| [x as f32 / 16.0, y as f32 / 5.0, y as f32 / 5.0, 1.0];
    let back: Vec<[f32; 4]> = vec![[0.0, 0.0, 0.0, 1.0]; (w * h) as usize];
    let src: Vec<[f32; 4]> = (0..w * h).map(|i| source(i % w, i / w)).collect();
    let doc = float_doc(w, h, &srgb, &[back, src]);
    let ranges: [Range; 2] = [
        ("gray", [0, 0, 96, 192], FULL),
        ("red", [0, 0, 64, 224], FULL),
    ];
    let handle = clone_of(&doc);
    let styled = then(handle, &blend_if_request(1, &blend_if_of(&ranges)), &[]);
    let out = unsafe { &*styled }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba32()
        .unwrap();
    for (i, got) in out.iter().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let cs = source(x, y).map(f64::from);
        let wt: f64 = ranges
            .iter()
            .map(|(c, this, _)| float_side(unit(c, [cs[0], cs[1], cs[2]]), *this))
            .product();
        for c in 0..3 {
            let want = wt * cs[c];
            assert!(
                (f64::from(got[c]) - want).abs() <= 1e-4,
                "({x}, {y}) channel {c}: got {}, want {want} (w {wt})",
                got[c]
            );
        }
    }
    free(styled);
}

// -------------------------------------------------------------- CMYK --

const CN: u32 = 8;
const CM: u32 = 6;

/// The cell's 8-bit codes: C the x ramp, K the y ramp.
fn cell_codes(x: u32, y: u32) -> [u16; 4] {
    [
        (x * 255 / (CN - 1)) as u16,
        0,
        0,
        (y * 255 / (CM - 1)) as u16,
    ]
}

/// The cell's ink in percent at `depth` (16 bits: the 8-bit code promoted
/// and moved a few codes off the 8-bit grid).
fn cell_ink(x: u32, y: u32, depth: u32) -> [f64; 4] {
    let codes = cell_codes(x, y);
    if depth == 8 {
        return codes.map(|c| f64::from(c) * 100.0 / 255.0);
    }
    [0, 1, 2, 3].map(|c| {
        let promoted = i64::from(codes[c]) * 257 + ((x + 3 * y + c as u32) % 5) as i64 * 23 - 46;
        promoted.clamp(0, 65535) as f64 * 100.0 / 65535.0
    })
}

/// Paper under a "Source" layer whose cells hold `cell_ink`.
fn cmyk_grid(depth: u32) -> *mut PrecisionDocument {
    let cell = |cx: u32, cy: u32| -> Vec<u16> {
        (0..CN * CM)
            .map(|p| {
                if p % CN == cx && p / CN == cy {
                    65535
                } else {
                    0
                }
            })
            .collect()
    };
    let mut doc = layer(
        backdrop(CN, CM, depth, &[[0.0; 4]]),
        "Source",
        cell_ink(0, 0, depth),
        &cell(0, 0),
    );
    for cy in 0..CM {
        for cx in 0..CN {
            if (cx, cy) != (0, 0) {
                doc = paint(doc, 1, cell_ink(cx, cy, depth), &cell(cx, cy));
            }
        }
    }
    doc
}

fn ink_value(channel: &str, codes: [f64; 4], code_max: f64) -> f64 {
    let plate = |p: usize| code_max - codes[p];
    match channel {
        "cyan" => plate(0),
        "magenta" => plate(1),
        "yellow" => plate(2),
        "black" => plate(3),
        _ => {
            let u = codes.map(|c| c / code_max);
            let paper = 1.0 - u[3];
            (code_max * unit(channel, [0, 1, 2].map(|i| (1.0 - u[i]) * paper))).round()
        }
    }
}

fn ink_knees(channel: &str, ramp: [u8; 4], code_max: f64) -> [f64; 4] {
    let shift = if channel == "yellow" {
        [-0.5, -0.5, 0.5, 0.5]
    } else {
        [-1.0, 0.0, 0.0, 1.0]
    };
    [0, 1, 2, 3].map(|i| (f64::from(ramp[i]) + shift[i]) * code_max / 255.0)
}

fn ink_weight(ranges: &[Range], cs: [f64; 4], cb: [f64; 4], code_max: f64) -> f64 {
    ranges
        .iter()
        .map(|(c, this, under)| {
            ramp_at(ink_value(c, cs, code_max), ink_knees(c, *this, code_max))
                * ramp_at(ink_value(c, cb, code_max), ink_knees(c, *under, code_max))
        })
        .product()
}

fn assert_cmyk(depth: u32) {
    let code_max = if depth == 8 { 255.0 } else { 65535.0 };
    let base = cmyk_grid(depth);
    let ranges: [Range; 2] = [
        ("cyan", [0, 0, 96, 192], FULL),
        ("black", [0, 0, 64, 224], FULL),
    ];
    let doc = then(
        clone_of(base),
        &blend_if_request(1, &blend_if_of(&ranges)),
        &[],
    );
    let samples = flat(doc);
    let mut opened = 0;
    for y in 0..CM {
        for x in 0..CN {
            let codes = |index: usize| {
                let s = sample(base, index, x, y).expect("a sample");
                [0, 1, 2, 3].map(|c| (s[c] * code_max).round())
            };
            let (cb, cs) = (codes(0), codes(1));
            let w = ink_weight(&ranges, cs, cb, code_max);
            if w > 0.0 && w < 1.0 {
                opened += 1;
            }
            let got = samples[(y * CN + x) as usize];
            for c in 0..4 {
                let want16 = ((cb[c] + w * (cs[c] - cb[c])) * 65535.0 / code_max).round();
                assert!(
                    (f64::from(got[c]) - want16).abs() <= 1.0,
                    "{depth}-bit ({x}, {y}) plate {c}: got {}, want {want16} (w {w})",
                    got[c]
                );
            }
        }
    }
    assert!(opened > 0, "a cell on a ramp's slope");
    free(doc);
    free(base);
}

#[test]
fn cmyk_plate_ranges_multiply_at_eight_bits() {
    assert_cmyk(8);
}

#[test]
fn cmyk_plate_ranges_multiply_at_sixteen_bits() {
    assert_cmyk(16);
}

// ------------------------------------------------- requests and refusals --

const RED_NOT_HELD: &str = "Blend If on the Red channel is unavailable in a CMYK document";

#[test]
fn layer_blend_if_carries_extra_and_a_cmyk_document_refuses_rgb_ranges_as_written() {
    // On RGB16 the canonical value comes back, gray first.
    let rgb = rgb16_doc();
    let written = json!({"channel": "red", "this_layer": [0, 0, 64, 224],
        "extra": [{"channel": "gray", "this_layer": [0, 0, 96, 192]}, {"channel": "blue"}]});
    let set = then(clone_of(rgb), &blend_if_request(1, &written), &[]);
    let canonical = blend_if_of(&[
        ("gray", [0, 0, 96, 192], FULL),
        ("red", [0, 0, 64, 224], FULL),
    ]);
    assert_eq!(layer_blend_if(set, 1), canonical);
    // The echo is an exact no-op.
    assert!(try_edit(set, &blend_if_request(1, &canonical), &[])
        .unwrap()
        .is_null());
    // A key the request does not know refuses; `extra` is known.
    let unknown = json!({"channel":"gray","this_layer":[0,0,9,9],"bogus":1});
    assert_eq!(
        try_edit(rgb, &blend_if_request(1, &unknown), &[]).unwrap_err(),
        "unknown native blend-if field bogus"
    );
    // The same strict key set holds inside each `extra` range.
    let nested = json!({"channel":"gray","this_layer":[0,0,9,9],
        "extra":[{"channel":"red","this_layer":[0,0,9,9],"bogus":1}]});
    assert_eq!(
        try_edit(rgb, &blend_if_request(1, &nested), &[]).unwrap_err(),
        "unknown native blend-if field bogus"
    );
    // A CMYK plate on RGB16, even as an identity extra.
    let cyan = json!({"channel":"gray","this_layer":[0,0,96,192],"extra":[{"channel":"cyan"}]});
    assert_eq!(
        try_edit(rgb, &blend_if_request(1, &cyan), &[]).unwrap_err(),
        "Blend If on the Cyan channel needs a CMYK document"
    );
    free(set);
    free(rgb);

    let cmyk = cmyk_grid(8);
    let gray_cyan = blend_if_of(&[
        ("gray", [0, 0, 96, 192], FULL),
        ("cyan", [0, 0, 64, 224], FULL),
    ]);
    for request in [
        blend_if_request(1, &gray_cyan),
        style_request(1, &gray_cyan),
    ] {
        let set = then(clone_of(cmyk), &request, &[]);
        assert_eq!(layer_blend_if(set, 1), gray_cyan, "{request}");
        free(set);
    }
    for blend_if in [
        // An IDENTITY Red in `extra` (canonicalization would drop it).
        json!({"channel":"gray","this_layer":[0,0,96,192],"extra":[{"channel":"red"}]}),
        // An identity Red PRIMARY beside a non-identity Cyan extra.
        json!({"channel":"red","extra":[{"channel":"cyan","this_layer":[0,0,64,224]}]}),
        // A non-identity Red extra.
        json!({"channel":"cyan","this_layer":[0,0,64,224],
            "extra":[{"channel":"red","this_layer":[0,0,9,9]}]}),
    ] {
        for request in [blend_if_request(1, &blend_if), style_request(1, &blend_if)] {
            assert_eq!(
                try_edit(cmyk, &request, &[]).unwrap_err(),
                RED_NOT_HELD,
                "{request}"
            );
        }
    }
    free(cmyk);
}

#[test]
fn gray_and_cyan_are_accepted_on_an_empty_isolated_group_of_a_cmyk8_document() {
    let doc = cmyk_grid(8);
    let grouped = then(
        doc,
        r#"{"op":"group_layers","layers":[1],"name":"G","pass_through":false}"#,
        &[],
    );
    // Moving the member back out leaves the group empty.
    let emptied = then(
        grouped,
        r#"{"op":"move_layer_to","from":1,"to":0,"depth":0}"#,
        &[],
    );
    let d = unsafe { &*emptied };
    let group = (0..d.layer_count())
        .find(|&i| d.layer(i).unwrap().name() == "G")
        .expect("the group");
    assert_eq!(
        d.layer(group).unwrap().children().map(|c| c.len()),
        Some(0),
        "the group is empty"
    );
    let gray_cyan = blend_if_of(&[
        ("gray", [0, 0, 96, 192], FULL),
        ("cyan", [0, 0, 64, 224], FULL),
    ]);
    for request in [
        blend_if_request(group, &gray_cyan),
        style_request(group, &gray_cyan),
    ] {
        let set = then(clone_of(emptied), &request, &[]);
        assert_eq!(layer_blend_if(set, group), gray_cyan, "{request}");
        free(set);
    }
    free(emptied);
}

// ------------------------------------------------------------ .rz 101 --

fn save(doc: *const PrecisionDocument, path: &Path) -> Vec<u8> {
    unsafe { &*doc }
        .save_native(path.to_str().unwrap(), LIMIT, 0, None)
        .unwrap();
    std::fs::read(path).unwrap()
}

fn open_bytes(dir: &TempDir, name: &str, bytes: &[u8]) -> Result<PrecisionDocument, String> {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    PrecisionDocument::open_native(path.to_str().unwrap(), LIMIT, 0, None)
}

fn refused(dir: &TempDir, name: &str, bytes: &[u8]) -> String {
    open_bytes(dir, name, bytes).err().expect("a refusal")
}

fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn lbif(data: &[u8]) -> usize {
    data.windows(4).rposition(|v| v == b"LBIF").unwrap()
}

const GRAY_RED: [Range; 2] = [
    ("gray", [0, 0, 96, 192], FULL),
    ("red", [0, 0, 64, 224], FULL),
];

#[test]
fn the_typed_word_101_round_trips_and_refuses_both_ways() {
    let dir = TempDir::new().unwrap();
    let base = rgb16_doc();
    let single = then(
        clone_of(base),
        &blend_if_request(1, &blend_if_of(&GRAY_RED[..1])),
        &[],
    );
    let plain = save(single, &dir.path().join("single.rz"));
    assert_eq!(word(&plain, 4), 46);
    assert_ne!(word(&plain, 8), 101, "no 101 for one range");
    assert_eq!(word(&plain, lbif(&plain) + 4), 1, "schema 1, unchanged");
    let multi = then(
        clone_of(base),
        &blend_if_request(1, &blend_if_of(&GRAY_RED)),
        &[],
    );
    let bytes = save(multi, &dir.path().join("multi.rz"));
    assert_eq!(word(&bytes, 4), 46);
    assert_eq!(word(&bytes, 8), 101, "right after 46 on RGB");
    assert_eq!(word(&bytes, lbif(&bytes) + 4), 2, "schema 2");
    let reopened = open_bytes(&dir, "again.rz", &bytes).unwrap();
    assert_eq!(
        reopened.layer(1).unwrap().blend_if(),
        unsafe { &*multi }.layer(1).unwrap().blend_if()
    );
    assert_eq!(
        reopened
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap(),
        unsafe { &*multi }
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap()
    );
    let mut stripped = bytes.clone();
    stripped.drain(8..12);
    let message = refused(&dir, "cut.rz", &stripped);
    assert!(
        message.ends_with("native multichannel blend if requires DRDC 101"),
        "{message}"
    );
    let mut forged = plain.clone();
    forged.splice(8..8, 101u32.to_le_bytes());
    assert_eq!(
        refused(&dir, "forged.rz", &forged),
        "invalid native multichannel blend if word"
    );
    free(single);
    free(multi);
    free(base);
}

#[test]
fn every_schema_2_refusal_lands_before_any_png_decodes() {
    let dir = TempDir::new().unwrap();
    let base = rgb16_doc();
    let multi = then(
        clone_of(base),
        &blend_if_request(1, &blend_if_of(&GRAY_RED)),
        &[],
    );
    let good = save(multi, &dir.path().join("good.rz"));
    let at = lbif(&good);
    assert_eq!(word(&good, at + 8), 1, "one entry");
    assert_eq!(word(&good, at + 16), 2, "two ranges");
    // Every corruption also breaks a PNG, so the framing refusal must win.
    let corrupt = |data: &mut Vec<u8>| {
        let idat = data.windows(4).position(|b| b == b"IDAT").unwrap();
        data[idat + 4] ^= 0xff;
    };
    let mut cases: Vec<(Vec<u8>, &str)> = Vec::new();
    for n in [0u32, 9] {
        let mut bad = good.clone();
        bad[at + 16..at + 20].copy_from_slice(&n.to_le_bytes());
        cases.push((bad, "invalid native blend-if range count"));
    }
    let mut swapped = good.clone();
    swapped[at + 20..at + 24].copy_from_slice(&1u32.to_le_bytes());
    swapped[at + 32..at + 36].copy_from_slice(&0u32.to_le_bytes());
    cases.push((swapped, "invalid native blend-if channel order"));
    let mut identity = good.clone();
    identity[at + 36..at + 40].copy_from_slice(&FULL);
    cases.push((identity, "invalid native blend-if identity entry"));
    // A schema 2 table whose every entry holds one range: the single-range
    // file with 101 claimed, schema 2 and n = 1 inserted.
    let single = then(
        clone_of(base),
        &blend_if_request(1, &blend_if_of(&GRAY_RED[..1])),
        &[],
    );
    let plain = save(single, &dir.path().join("plain.rz"));
    let mut lone = plain.clone();
    let p = lbif(&lone);
    lone[p + 4..p + 8].copy_from_slice(&2u32.to_le_bytes());
    lone.splice(p + 16..p + 16, 1u32.to_le_bytes());
    lone.splice(8..8, 101u32.to_le_bytes());
    cases.push((lone, "noncanonical native blend-if schema"));
    for (i, (mut bad, needle)) in cases.into_iter().enumerate() {
        corrupt(&mut bad);
        let message = refused(&dir, &format!("bad-{i}.rz"), &bad);
        assert_eq!(message, needle, "case {i}");
    }
    free(single);
    free(multi);
    free(base);
}

#[test]
fn a_comp_only_multichannel_blend_if_claims_the_typed_word() {
    use doctored_core::doc_comps::{CompCaptures, CompRequest};
    let dir = TempDir::new().unwrap();
    let base = rgb16_doc();
    let multi = then(
        clone_of(base),
        &blend_if_request(1, &blend_if_of(&GRAY_RED)),
        &[],
    );
    let save_comp = CompRequest::Save {
        name: "Ranges".into(),
        comment: String::new(),
        captures: CompCaptures::default(),
    };
    let saved = unsafe { &*multi }
        .edit_layer_comps(&save_comp)
        .unwrap()
        .unwrap();
    let cleared = then(
        Box::into_raw(Box::new(saved)),
        &blend_if_request(1, &Value::Null),
        &[],
    );
    assert_eq!(unsafe { &*cleared }.layer(1).unwrap().blend_if(), None);
    let bytes = save(cleared, &dir.path().join("comp.rz"));
    assert_eq!(word(&bytes, 8), 101);
    assert!(open_bytes(&dir, "comp-again.rz", &bytes).is_ok());
    free(cleared);
    free(multi);
    free(base);
}

#[test]
fn a_cmyk_file_claims_word_101_after_its_rung() {
    let dir = TempDir::new().unwrap();
    let doc = then(
        cmyk_grid(16),
        &blend_if_request(
            1,
            &blend_if_of(&[
                ("gray", [0, 0, 96, 192], FULL),
                ("black", [0, 0, 64, 224], FULL),
            ]),
        ),
        &[],
    );
    let bytes = save(doc, &dir.path().join("cmyk.rz"));
    assert_eq!(word(&bytes, 4), 46);
    assert!((82..=88).contains(&word(&bytes, 8)), "the CMYK rung");
    assert_eq!(word(&bytes, 12), 101, "101 after the rung");
    let reopened = open_bytes(&dir, "cmyk-again.rz", &bytes).unwrap();
    assert_eq!(
        reopened.layer(1).unwrap().blend_if(),
        unsafe { &*doc }.layer(1).unwrap().blend_if()
    );
    let reopened = Box::into_raw(Box::new(reopened));
    assert_eq!(flat(reopened), flat(doc));
    let mut stripped = bytes.clone();
    stripped.drain(12..16);
    let message = refused(&dir, "cmyk-cut.rz", &stripped);
    assert!(
        message.ends_with("native multichannel blend if requires DRDC 101"),
        "{message}"
    );
    free(reopened);
    free(doc);
}

// ------------------------------------------------------- .rz 101 and 100 --

/// A wide style colour (word 100, `wide_color_native`): linear components
/// off the sRGB byte grid, so no hex replaces them.
const WIDE: [f64; 3] = [0.9, -0.03, 0.123_456_7];

/// A style holding both a multichannel Blend If (word 101) and a wide
/// colour (word 100).
fn both_style(ranges: &[Range]) -> Value {
    json!({"blend_if": blend_if_of(ranges),
        "effects": [{"type": "drop_shadow", "linear": WIDE}]})
}

/// The same style claiming neither word: one range and a byte colour. Its
/// file holds every other wrapper word the full style's does, so the words
/// after 101 and 100 must be its words after the same slot.
fn narrow_style(ranges: &[Range]) -> Value {
    json!({"blend_if": blend_if_of(&ranges[..1]),
        "effects": [{"type": "drop_shadow", "color": "#123456"}]})
}

/// `bytes` with the words at `at` and `at + 4` exchanged.
fn swapped(bytes: &[u8], at: usize) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out[at..at + 4].copy_from_slice(&bytes[at + 4..at + 8]);
    out[at + 4..at + 8].copy_from_slice(&bytes[at..at + 4]);
    out
}

/// Opens `bytes` written under a file name, keeping only the refusal.
type Opener<'a> = dyn Fn(&str, &[u8]) -> Result<(), String> + 'a;

/// The refusals both words' layout must give, `first` being the offset of
/// word 101 (word 100 right after it): dropping 100's four bytes refuses by
/// 100's own name, and 100 written ahead of 101 refuses with `swapped_refusal`
/// (101 is then read where the body version sits) rather than reading
/// either word as the other.
fn assert_layout_refusals(bytes: &[u8], first: usize, swapped_refusal: &str, open: &Opener) {
    let mut cut = bytes.to_vec();
    cut.drain(first + 4..first + 8);
    let message = open("no-100.rz", &cut).expect_err("a wide colour without word 100");
    assert!(
        message.ends_with("native wide style colour requires DRDC 100"),
        "{message}"
    );
    let message = open("swapped.rz", &swapped(bytes, first)).expect_err("100 ahead of 101");
    assert!(message.contains(swapped_refusal), "{message}");
}

/// The typed reader's refusal of an unknown body version.
const TYPED_SWAPPED: &str = "high-depth DRDC document";

#[test]
fn words_101_then_100_round_trip_together_at_8_and_16_bits_and_on_cmyk8() {
    let dir = TempDir::new().unwrap();

    // 8 bits (byte `.rz`): 45, 101, 100, then what the narrow file holds
    // right after 45.
    let byte_open = |name: &str, bytes: &[u8]| -> Result<DrDocument, String> {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        DrDocument::open(path.to_str().unwrap())
    };
    let byte_save = |name: &str, doc: &DrDocument| -> Vec<u8> {
        let path = dir.path().join(name);
        doc.save_native(path.to_str().unwrap()).unwrap();
        std::fs::read(&path).unwrap()
    };
    let base = rect_layer_doc((W, H), (4, 2, 12, 4), [200, 120, 40, 255]);
    let narrow = byte_save(
        "byte-narrow.rz",
        &styled(&base, 1, &narrow_style(&GRAY_RED).to_string()),
    );
    assert_eq!(word(&narrow, 4), 45);
    assert!(
        !(90..=101).contains(&word(&narrow, 8)),
        "no conditional word"
    );
    let doc = styled(&base, 1, &both_style(&GRAY_RED).to_string());
    let bytes = byte_save("byte-both.rz", &doc);
    assert_eq!(word(&bytes, 4), 45);
    assert_eq!(word(&bytes, 8), 101, "101 FIRST inside 45");
    assert_eq!(word(&bytes, 12), 100, "100 right after 101");
    assert_eq!(
        word(&bytes, 16),
        word(&narrow, 8),
        "then the narrow file's word"
    );
    let reopened = byte_open("byte-again.rz", &bytes).unwrap();
    let style = |d: &DrDocument| d.layers[1].style.as_deref().unwrap().to_json();
    assert_eq!(style(&reopened), style(&doc));
    let stored: Value = serde_json::from_str(&style(&reopened)).unwrap();
    assert_eq!(stored["blend_if"]["extra"].as_array().unwrap().len(), 1);
    assert!(stored["effects"][0]["linear"].is_array());
    assert_eq!(reopened.flattened().unwrap(), doc.flattened().unwrap());
    assert_layout_refusals(&bytes, 8, "unsupported DRDC version 101", &|n, b| {
        byte_open(n, b).map(|_| ())
    });

    // 16 bits (typed `.rz`): 46, 101, 100, then the narrow file's word.
    let base = rgb16_doc();
    let request = json!({"op":"layer_style","version":1,"layer":1,
        "style": narrow_style(&GRAY_RED)})
    .to_string();
    let narrow_doc = then(clone_of(base), &request, &[]);
    let narrow = save(narrow_doc, &dir.path().join("typed-narrow.rz"));
    free(narrow_doc);
    assert_eq!(word(&narrow, 4), 46);
    assert!(
        !(90..=101).contains(&word(&narrow, 8)),
        "no conditional word"
    );
    let request = json!({"op":"layer_style","version":1,"layer":1,
        "style": both_style(&GRAY_RED)})
    .to_string();
    let doc = then(clone_of(base), &request, &[]);
    let bytes = save(doc, &dir.path().join("typed-both.rz"));
    assert_eq!(word(&bytes, 4), 46);
    assert_eq!(word(&bytes, 8), 101, "101 right after 46 on RGB");
    assert_eq!(word(&bytes, 12), 100, "100 right after 101");
    assert_eq!(
        word(&bytes, 16),
        word(&narrow, 8),
        "then the narrow file's word"
    );
    assert_eq!(word(&bytes, lbif(&bytes) + 4), 2, "LBIF schema 2");
    let reopened = open_bytes(&dir, "typed-again.rz", &bytes).unwrap();
    let d = unsafe { &*doc };
    assert_eq!(
        reopened.layer(1).unwrap().blend_if(),
        d.layer(1).unwrap().blend_if()
    );
    assert!(reopened
        .layer(1)
        .unwrap()
        .blend_if()
        .unwrap()
        .is_multichannel());
    let reported = reopened.layer(1).unwrap().reported_style();
    assert_eq!(reported, d.layer(1).unwrap().reported_style());
    let stored: Value = serde_json::from_str(&reported.unwrap()).unwrap();
    assert!(stored["effects"][0]["linear"].is_array());
    assert_eq!(
        reopened
            .render(LIMIT, 0, None)
            .unwrap()
            .to_rgba16()
            .unwrap(),
        d.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
    );
    assert_layout_refusals(&bytes, 8, TYPED_SWAPPED, &|n, b| {
        open_bytes(&dir, n, b).map(|_| ())
    });
    free(doc);
    free(base);

    // CMYK 8 bits: 46, the rung, 101, 100, then the narrow file's word.
    let base = cmyk_grid(8);
    let ranges: [Range; 2] = [
        ("gray", [0, 0, 96, 192], FULL),
        ("black", [0, 0, 64, 224], FULL),
    ];
    let request = json!({"op":"layer_style","version":1,"layer":1,
        "style": narrow_style(&ranges)})
    .to_string();
    let narrow_doc = then(clone_of(base), &request, &[]);
    let narrow = save(narrow_doc, &dir.path().join("cmyk-narrow.rz"));
    free(narrow_doc);
    assert!(
        !(90..=101).contains(&word(&narrow, 12)),
        "no conditional word"
    );
    let request = json!({"op":"layer_style","version":1,"layer":1,
        "style": both_style(&ranges)})
    .to_string();
    let doc = then(clone_of(base), &request, &[]);
    let bytes = save(doc, &dir.path().join("cmyk-both.rz"));
    assert_eq!(word(&bytes, 4), 46);
    assert!((82..=88).contains(&word(&bytes, 8)), "the CMYK rung");
    assert_eq!(word(&bytes, 8), word(&narrow, 8), "the narrow file's rung");
    assert_eq!(word(&bytes, 12), 101, "101 after the rung");
    assert_eq!(word(&bytes, 16), 100, "100 right after 101");
    assert_eq!(
        word(&bytes, 20),
        word(&narrow, 12),
        "then the narrow file's word"
    );
    let reopened = open_bytes(&dir, "cmyk-again.rz", &bytes).unwrap();
    let d = unsafe { &*doc };
    assert_eq!(
        reopened.layer(1).unwrap().blend_if(),
        d.layer(1).unwrap().blend_if()
    );
    assert!(reopened
        .layer(1)
        .unwrap()
        .blend_if()
        .unwrap()
        .is_multichannel());
    let reported = reopened.layer(1).unwrap().reported_style();
    assert_eq!(reported, d.layer(1).unwrap().reported_style());
    let stored: Value = serde_json::from_str(&reported.unwrap()).unwrap();
    assert!(stored["effects"][0]["linear"].is_array());
    let reopened = Box::into_raw(Box::new(reopened));
    assert_eq!(flat(reopened), flat(doc));
    assert_layout_refusals(&bytes, 12, TYPED_SWAPPED, &|n, b| {
        open_bytes(&dir, n, b).map(|_| ())
    });
    free(reopened);
    free(doc);
    free(base);
}

// ------------------------------------------------------------------ PSD --

/// A layered PSD; an RGB document hands in its projection, a CMYK one none.
fn save_psd(doc: *const PrecisionDocument, path: &Path) {
    let d = unsafe { &*doc };
    let projection = (!d.is_cmyk()).then(|| d.render(LIMIT, 0, None).unwrap());
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            projection
                .as_ref()
                .map_or(ptr::null(), |p| p as *const PrecisionImage),
            cpath(path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
}

fn ranges_section(slots: &[[u8; 8]]) -> Vec<u8> {
    let mut section = 40u32.to_be_bytes().to_vec();
    for i in 0..5 {
        section.extend(
            slots
                .get(i)
                .copied()
                .unwrap_or([0, 0, 255, 255, 0, 0, 255, 255]),
        );
    }
    section
}

fn count(bytes: &[u8], needle: &[u8]) -> usize {
    bytes.windows(needle.len()).filter(|w| *w == needle).count()
}

fn import_report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let raw = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(raw)).unwrap()
}

#[test]
fn a_sixteen_bit_psd_writes_slots_0_and_1_and_reads_both_back() {
    let dir = TempDir::new().unwrap();
    let ranges = [
        ("gray", [0, 0, 96, 192], [5, 15, 235, 250]),
        ("red", [0, 0, 64, 224], FULL),
    ];
    let doc = then(
        rgb16_doc(),
        &blend_if_request(1, &blend_if_of(&ranges)),
        &[],
    );
    let path = dir.path().join("ranges16.psd");
    save_psd(doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    let section = ranges_section(&[
        [0, 0, 96, 192, 5, 15, 235, 250],
        [0, 0, 64, 224, 0, 0, 255, 255],
    ]);
    assert_eq!(count(&bytes, &section), 1, "slot 0 gray, slot 1 red");
    let report = import_report(&path);
    assert!(
        report["observations"].as_array().unwrap().iter().any(|o| o
            .as_str()
            .unwrap()
            .contains("Blend If ranges on gray and red")),
        "{report}"
    );
    let token = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let opened = unsafe {
        dr_precision_doc_open_psd(
            cpath(&path).as_ptr(),
            0,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!opened.is_null(), "{}", take_err_string(err));
    let reopened = unsafe { &*opened };
    let source = (0..reopened.layer_count())
        .find(|&i| reopened.layer(i).unwrap().name() == "Source")
        .unwrap();
    assert_eq!(
        reopened.layer(source).unwrap().blend_if(),
        unsafe { &*doc }.layer(1).unwrap().blend_if()
    );
    free(opened);
    free(doc);
}

#[test]
fn a_cmyk_psd_writes_each_plate_range_in_its_slot_and_reads_both_back() {
    let dir = TempDir::new().unwrap();
    let doc = then(
        cmyk_grid(8),
        &blend_if_request(
            1,
            &blend_if_of(&[
                ("gray", [0, 0, 96, 192], FULL),
                ("cyan", [0, 0, 64, 224], FULL),
            ]),
        ),
        &[],
    );
    let path = dir.path().join("ranges-cmyk.psd");
    save_psd(doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    let section = ranges_section(&[
        [0, 0, 96, 192, 0, 0, 255, 255],
        [0, 0, 64, 224, 0, 0, 255, 255],
    ]);
    assert_eq!(count(&bytes, &section), 1, "slot 0 gray, slot 1 cyan");
    let back = open_psd(&path, 0).unwrap();
    let reopened = unsafe { &*back };
    let source = (0..reopened.layer_count())
        .find(|&i| reopened.layer(i).unwrap().name() == "Source")
        .unwrap();
    assert_eq!(
        reopened.layer(source).unwrap().blend_if(),
        unsafe { &*doc }.layer(1).unwrap().blend_if()
    );
    assert_eq!(flat(back), flat(doc));
    free(back);
    free(doc);
}

// ------------------------------------------------------------ clipboard --

#[test]
fn the_typed_layer_clipboard_carries_every_range() {
    let clip = then(
        rgb16_doc(),
        &blend_if_request(1, &blend_if_of(&GRAY_RED)),
        &[],
    );
    let dest = rgb16_doc();
    let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
    let out = unsafe {
        dr_precision_doc_paste_layers(
            dest,
            clip,
            0,
            0,
            0,
            0,
            false,
            LIMIT,
            0,
            &mut report,
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    if !report.is_null() {
        unsafe { dr_string_free(report) };
    }
    let pasted = unsafe { &*out };
    let want = unsafe { &*clip }.layer(1).unwrap().blend_if();
    assert!(want.is_some_and(|b| b.is_multichannel()));
    assert!(
        (0..pasted.layer_count()).any(|i| pasted.layer(i).unwrap().blend_if() == want),
        "a pasted record carries both ranges"
    );
    free(out);
    free(dest);
    free(clip);
}

// ------------------------------------------------------------- Photoshop --

#[test]
fn photoshops_own_p3_psd_imports_with_both_plate_ranges() {
    let Some(path) = crate::common::take1_558("p8", "p3_cmyk8.psd") else {
        eprintln!("skipping: blend-if-multichannel-558/take1 is absent");
        return;
    };
    let report = crate::cmyk_psd_import::report(&path);
    assert!(
        !report["verified_losses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l.as_str().unwrap().contains("Blend If")),
        "{report}"
    );
    let back = open_psd(&path, 0).unwrap();
    let d = unsafe { &*back };
    let ranged = (0..d.layer_count())
        .find(|&i| d.layer(i).unwrap().blend_if().is_some())
        .expect("the source record carries the ranges");
    assert_eq!(
        layer_blend_if(back, ranged),
        blend_if_of(&[
            ("cyan", [0, 0, 96, 192], FULL),
            ("black", [0, 0, 64, 224], FULL),
        ])
    );
    free(back);
}
