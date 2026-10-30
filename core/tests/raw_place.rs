//! A camera RAW file placed into an 8-, 16- or 32-bit document, as the
//! host's `RawSourcePlacement` does it: the u16 development placed under
//! `convert_fallback` (optionally scaled by `source_transform`, optionally
//! linked), then given its original through the RAW attach FFI; Replace
//! Contents is a `replace` and a fresh attach.
//!
//! Oracles, none read off the implementation: a 16-bit same-profile render is
//! the master's own codes; an 8-bit one is within 1 of `round(c · 255 /
//! 65535)`; a float one is the f64 chain over the profile's own tag data
//! (`common::icc_chain`, through `float_placed_image::assert_rendered`); the
//! original camera bytes are compared with what the test wrote; the refusal
//! sentence is written out here.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_raw_source::{
    dr_doc_attach_raw_source, dr_doc_raw_source_copy, dr_doc_raw_source_length,
    dr_precision_doc_attach_raw_source, dr_precision_doc_raw_source_copy,
    dr_precision_doc_raw_source_length,
};
use doctored_core::ffi_retained_source::{
    dr_doc_mixed_source_edit, dr_doc_source_info, dr_precision_doc_mixed_source_edit,
    dr_precision_doc_source_info,
};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::icc_chain::Chain;
use crate::float_placed_image::{assert_rendered, base, floats, oracle, typed_master};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 16;
const H: u32 = 12;
const ORIGINAL: &[u8] = b"II*\0the placed camera file\0\x89\xfe\x01";
const SECOND: &[u8] = b"II*\0the replacing camera file, a little longer\0\x07";
const FRESH_REVISION: &str =
    "RAW attachment requires a fresh source revision; redevelop through Replace Contents";
const LINK: &str = "/Volumes/Cards/DCIM/camera.dng";

// ------------------------------------------------------------- plumbing --

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// A RAW descriptor of a camera file developed at `exposure`.
fn descriptor(exposure: f64) -> CString {
    CString::new(
        json!({"schema_version":1,"original_filename":"camera.dng",
            "content_sha256":"c".repeat(64),
            "decoder":{"identifier":"test.decoder","version":"1.0","implementation":"test-platform"},
            "recipe":{"schema_version":1,"exposure":exposure,"white_balance":[5500.0,0.0],
                "output_space":"srgb"}})
        .to_string(),
    )
    .unwrap()
}

/// The host's place request: `convert_fallback` at every depth.
fn place(link: Option<&str>, matrix: Option<[f64; 6]>) -> Value {
    let mut request = json!({"op":"place","source_id":"camera","revision_id":"first",
        "name":"camera","x":0,"y":0,"anchor":0,"position":"above",
        "depth_conversion":"convert_fallback",
        "link": link.map_or(Value::Null, |path| json!({"path":path,"sha256":"c".repeat(64)}))});
    if let Some(matrix) = matrix {
        request["source_transform"] = json!(matrix);
    }
    request
}

/// The host's Replace Contents request, carrying `link`.
fn replace(link: Value) -> Value {
    json!({"op":"replace","layer":1,"revision_id":"second",
        "depth_conversion":"convert_fallback","link":link})
}

/// One document of either kind, so each step is written once per depth.
enum Doc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}

impl Doc {
    fn edit(&self, asset: Option<&PrecisionDocument>, request: Value) -> Result<Doc, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let asset = asset.map_or(ptr::null(), |a| a as *const _);
        let out = match self {
            Doc::Byte(doc) => unsafe {
                let out = dr_doc_mixed_source_edit(
                    doc,
                    ptr::null(),
                    asset,
                    text.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                );
                (!out.is_null()).then(|| Doc::Byte(*Box::from_raw(out)))
            },
            Doc::Typed(doc) => unsafe {
                let out = dr_precision_doc_mixed_source_edit(
                    doc,
                    ptr::null(),
                    asset,
                    text.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                );
                (!out.is_null()).then(|| Doc::Typed(*Box::from_raw(out)))
            },
        };
        if let Some(message) = owned(err) {
            return Err(message);
        }
        Ok(out.expect("the edit changes the document"))
    }

    fn attach(&self, bytes: &[u8], exposure: f64) -> Result<Doc, String> {
        let text = descriptor(exposure);
        let mut err = ptr::null_mut();
        let out = match self {
            Doc::Byte(doc) => unsafe {
                let out = dr_doc_attach_raw_source(
                    doc,
                    1,
                    bytes.as_ptr(),
                    bytes.len(),
                    text.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                );
                (!out.is_null()).then(|| Doc::Byte(*Box::from_raw(out)))
            },
            Doc::Typed(doc) => unsafe {
                let out = dr_precision_doc_attach_raw_source(
                    doc,
                    1,
                    bytes.as_ptr(),
                    bytes.len(),
                    text.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                );
                (!out.is_null()).then(|| Doc::Typed(*Box::from_raw(out)))
            },
        };
        if let Some(message) = owned(err) {
            return Err(message);
        }
        Ok(out.expect("the attach changes the document"))
    }

    fn info(&self) -> Value {
        let mut err = ptr::null_mut();
        let out = match self {
            Doc::Byte(doc) => unsafe { dr_doc_source_info(doc, 1, &mut err) },
            Doc::Typed(doc) => unsafe { dr_precision_doc_source_info(doc, 1, &mut err) },
        };
        assert!(owned(err).is_none());
        serde_json::from_str(&owned(out).expect("source info")).unwrap()
    }

    fn original(&self) -> Vec<u8> {
        let mut err = ptr::null_mut();
        let length = match self {
            Doc::Byte(doc) => unsafe { dr_doc_raw_source_length(doc, 1, &mut err) },
            Doc::Typed(doc) => unsafe { dr_precision_doc_raw_source_length(doc, 1, &mut err) },
        };
        assert!(owned(err).is_none());
        let mut bytes = vec![0x5a; length];
        let copied = match self {
            Doc::Byte(doc) => unsafe {
                dr_doc_raw_source_copy(doc, 1, bytes.as_mut_ptr(), length, &mut err)
            },
            Doc::Typed(doc) => unsafe {
                dr_precision_doc_raw_source_copy(doc, 1, bytes.as_mut_ptr(), length, &mut err)
            },
        };
        assert!(copied, "{:?}", owned(err));
        bytes
    }

    /// The placed layer's offset and size.
    fn bounds(&self) -> (i32, i32, u32, u32) {
        match self {
            Doc::Byte(doc) => {
                let layer = &doc.layers[1];
                let (w, h) = layer.pixels.dimensions();
                (layer.offset.0, layer.offset.1, w, h)
            }
            Doc::Typed(doc) => {
                let (x, y) = doc.layer(1).unwrap().offset();
                let (w, h) = doc.layer_source(1).unwrap().dimensions();
                (x, y, w, h)
            }
        }
    }
}

/// The three targets: 8-bit, 16-bit and 32-bit float sRGB, `W × H`, one
/// opaque record each.
fn targets() -> [(u32, Doc); 3] {
    let sixteen = typed_master(
        W,
        H,
        vec![[9000, 30000, 50000, 65535]; (W * H) as usize],
        IccProfile::srgb(),
    );
    [
        (
            8,
            Doc::Byte(DrDocument::from_pixels(RgbaImage::from_pixel(
                W,
                H,
                Rgba([9, 120, 200, 255]),
            ))),
        ),
        (16, Doc::Typed(sixteen)),
        (32, Doc::Typed(base(W, H))),
    ]
}

/// A development's codes: a ramp over every channel, one sample half covered.
fn developed() -> Vec<[u16; 4]> {
    (0..W * H)
        .map(|i| {
            [
                (i * 337 + 101) as u16,
                (65535 - i * 271) as u16,
                (i * 1913 % 65536) as u16,
                if i == 29 { 32768 } else { 65535 },
            ]
        })
        .collect()
}

/// Asserts the placed layer renders `codes` at `depth` (same profile).
fn assert_codes(doc: &Doc, depth: u32, codes: &[[u16; 4]], what: &str) {
    match (doc, depth) {
        (Doc::Byte(doc), 8) => {
            let got: Vec<[u8; 4]> = doc.layers[1]
                .pixels
                .to_contiguous()
                .unwrap()
                .pixels()
                .map(|p| p.0)
                .collect();
            assert_eq!(got.len(), codes.len(), "{what}");
            for (i, (g, c)) in got.iter().zip(codes).enumerate() {
                for k in 0..4 {
                    let want = (f64::from(c[k]) * 255.0 / 65535.0).round();
                    assert!(
                        (f64::from(g[k]) - want).abs() <= 1.0,
                        "{what}: pixel {i} channel {k}: {} vs {want}",
                        g[k]
                    );
                }
            }
        }
        (Doc::Typed(doc), 16) => {
            assert_eq!(
                doc.layer_source(1).unwrap().to_rgba16().unwrap(),
                codes,
                "{what}"
            );
        }
        (Doc::Typed(doc), 32) => {
            let s = Chain::of(&IccProfile::srgb());
            assert_rendered(&floats(doc, 1), codes, |c| oracle(&s, None, c));
        }
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn a_u16_development_places_with_its_original_at_every_depth() {
    let codes = developed();
    let master = typed_master(W, H, codes.clone(), IccProfile::srgb());
    for (depth, target) in targets() {
        let placed = target.edit(Some(&master), place(None, None)).unwrap();
        // A new placement carries no original until the host attaches it.
        assert!(placed.info()["raw"].is_null(), "{depth}-bit");
        let attached = placed.attach(ORIGINAL, 0.5).unwrap();
        let report = attached.info();
        assert_eq!(report["valid"], true, "{depth}-bit");
        assert_eq!(report["master_depth"], 16, "{depth}-bit");
        assert_eq!(report["fallback_depth"], depth, "{depth}-bit");
        // A same-depth fallback converts nothing.
        let conversion = if depth == 16 {
            "none"
        } else {
            "convert_fallback"
        };
        assert_eq!(report["depth_conversion"], conversion, "{depth}-bit");
        assert_eq!(
            report["raw"]["encoded_bytes"],
            ORIGINAL.len(),
            "{depth}-bit"
        );
        assert_eq!(
            report["raw"]["original_filename"], "camera.dng",
            "{depth}-bit"
        );
        assert!(report["link"].is_null(), "{depth}-bit");
        assert_eq!(attached.original(), ORIGINAL, "{depth}-bit");
        assert_eq!(attached.bounds(), (0, 0, W, H), "{depth}-bit");
        assert_codes(&attached, depth, &codes, &format!("{depth}-bit place"));
    }
}

#[test]
fn a_resolution_scaled_place_halves_the_bounds_at_every_depth() {
    let uniform = [41000_u16, 12000, 52000, 65535];
    let master = typed_master(W, H, vec![uniform; (W * H) as usize], IccProfile::srgb());
    let half = [0.5, 0., 0., 0.5, 0., 0.];
    for (depth, target) in targets() {
        let placed = target
            .edit(Some(&master), place(None, Some(half)))
            .unwrap()
            .attach(ORIGINAL, 0.0)
            .unwrap();
        assert_eq!(placed.bounds(), (0, 0, W / 2, H / 2), "{depth}-bit");
        let (w, h) = (W / 2, H / 2);
        // The interior: every sample one pixel or more from the edge.
        let interior: Vec<usize> = (1..h - 1)
            .flat_map(|y| (1..w - 1).map(move |x| (y * w + x) as usize))
            .collect();
        match (&placed, depth) {
            (Doc::Byte(doc), 8) => {
                let got: Vec<[u8; 4]> = doc.layers[1]
                    .pixels
                    .to_contiguous()
                    .unwrap()
                    .pixels()
                    .map(|p| p.0)
                    .collect();
                for &i in &interior {
                    for k in 0..4 {
                        let want = (f64::from(uniform[k]) * 255.0 / 65535.0).round();
                        assert!((f64::from(got[i][k]) - want).abs() <= 1.0, "8-bit {i} {k}");
                    }
                }
            }
            (Doc::Typed(doc), 16) => {
                let got = doc.layer_source(1).unwrap().to_rgba16().unwrap();
                for &i in &interior {
                    for k in 0..4 {
                        let d = (i32::from(got[i][k]) - i32::from(uniform[k])).abs();
                        assert!(d <= 1, "16-bit {i} {k}: {} vs {}", got[i][k], uniform[k]);
                    }
                }
            }
            (Doc::Typed(doc), 32) => {
                let got = floats(doc, 1);
                let s = Chain::of(&IccProfile::srgb());
                let picked: Vec<[f32; 4]> = interior.iter().map(|&i| got[i]).collect();
                assert_rendered(&picked, &vec![uniform; picked.len()], |c| {
                    oracle(&s, None, c)
                });
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn a_linked_place_keeps_its_link_and_original() {
    let master = typed_master(W, H, developed(), IccProfile::srgb());
    for (depth, target) in targets() {
        let linked = target
            .edit(Some(&master), place(Some(LINK), None))
            .unwrap()
            .attach(ORIGINAL, 0.5)
            .unwrap();
        let report = linked.info();
        assert_eq!(report["link"]["path"], LINK, "{depth}-bit");
        assert_eq!(report["link"]["sha256"], "c".repeat(64), "{depth}-bit");
        assert_eq!(
            report["raw"]["encoded_bytes"],
            ORIGINAL.len(),
            "{depth}-bit"
        );
        assert_eq!(linked.original(), ORIGINAL, "{depth}-bit");
    }
}

#[test]
fn replace_contents_takes_a_fresh_original_and_keeps_the_link() {
    let first = typed_master(W, H, developed(), IccProfile::srgb());
    let codes: Vec<[u16; 4]> = developed()
        .iter()
        .map(|c| [c[2], c[0], c[1], c[3]])
        .collect();
    let second = typed_master(W, H, codes.clone(), IccProfile::srgb());
    for (depth, target) in targets() {
        let placed = target
            .edit(Some(&first), place(Some(LINK), None))
            .unwrap()
            .attach(ORIGINAL, 0.5)
            .unwrap();
        // The record already holds an original: a second attach refuses.
        assert_eq!(
            placed.attach(SECOND, 1.0).err().as_deref(),
            Some(FRESH_REVISION),
            "{depth}-bit"
        );
        let link = placed.info()["link"].clone();
        let replaced = placed.edit(Some(&second), replace(link.clone())).unwrap();
        assert!(replaced.info()["raw"].is_null(), "{depth}-bit");
        let attached = replaced.attach(SECOND, 1.0).unwrap();
        let report = attached.info();
        assert_eq!(report["revision_id"], "second", "{depth}-bit");
        assert_eq!(report["link"], link, "{depth}-bit");
        assert_eq!(report["raw"]["encoded_bytes"], SECOND.len(), "{depth}-bit");
        assert_eq!(report["raw"]["recipe"]["exposure"], 1.0, "{depth}-bit");
        assert_eq!(attached.original(), SECOND, "{depth}-bit");
        assert_codes(&attached, depth, &codes, &format!("{depth}-bit replace"));
        // The replaced record's own original is untouched.
        assert_eq!(placed.original(), ORIGINAL, "{depth}-bit");
    }
}
