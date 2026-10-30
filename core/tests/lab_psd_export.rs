//! #1 Lab phase 3b: a Lab Color document's PSD, PSB and layered-TIFF
//! export, through the C API.
//!
//! Oracles, never the writer itself:
//!
//! * (a) the ROUND TRIP: each file opened by the Lab open (phase 3a's
//!   reader, whose own oracles are `lab_psd.rs`'s byte-built files) gives
//!   back the same tree — names, kinds, order, blend, opacity, Fill,
//!   visibility, clipping, masks, groups, the style's effect colour within
//!   one code a channel and Blend If exactly — with the codes exact at 8
//!   bits, and at 16 bits L\* and alpha exact and a\*/b\* the 15-bit snap of
//!   `lab_psd_fixture` (the measured rule, written from the take's
//!   decision); a second round trip is byte-identical;
//! * (b) the ENCODE oracle: every grid a\* value i ∈ 0…32768 of a 16-bit
//!   document is stored as `lab_psd_fixture::psd_sample`; for i ∈ 0…32640
//!   the Photoshop-writable v = round(i·65535/32768) reads back and is
//!   written again as itself; for i ∈ 32641…32768 (past the document's a\*
//!   ceiling of 127) v reads as code 65535 and is written as the i = 32640
//!   sample;
//! * (c) the LAYOUT moves L1–L4 (`psd_export_lab`'s module doc), byte by
//!   byte by readers written here from Adobe's published framing and TIFF
//!   6.0, against Photoshop's OWN Lab files read in place
//!   (`~/devel/doctored-fixtures/photoshop/lab-files-547/take2`, never
//!   committed; that half SKIPS with an eprintln when absent);
//! * (d) `LbCl` both ways is the identity on a 16-step sRGB grid and on
//!   #40a0c8 exactly; an adjustment layer exports as a raster of its stamp
//!   with a loss; the report says `color_mode` lab, its depth and that no
//!   profile is embedded;
//! * (e) the readback take's files in place
//!   (`lab-export-547/take1/ours/`, SKIPPED when absent or not judged PASS):
//!   each layered file opened by the core and rendered is within 257 of
//!   Photoshop's own flattened TIFF of it at every cell centre.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_lab::dr_precision_doc_new_lab;
use doctored_core::ffi_precision::dr_precision_doc_set_layer_opacity;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_lab_psd, dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
    dr_psd_import_report, dr_tiff_has_layers,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_psd_fixture::{self as fx, Comp, PsdSpec, Rec};
use crate::common::{cpath, take_err_string};
use crate::gray_ops::{edit, free, LIMIT};
use crate::lab_fixture::srgb_to_lab;
use crate::lab_psd_fixture::{
    cielab_units, code_of, lab_channels, lab_psd, psd_sample, psd_units, read_lab_tiff, units_of,
};

const W: u32 = 8;
const H: u32 = 6;
const IDENTITY_RANGE: [u8; 8] = [0, 0, 0xff, 0xff, 0, 0, 0xff, 0xff];
/// The Color Overlay's authored colour (3a's H-C colour).
const OVERLAY: &str = "#40a0c8";

// ------------------------------------------------------------ the C API --

fn json_of(text: *mut std::os::raw::c_char) -> Value {
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn import_report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    json_of(text)
}

fn open(path: &Path, mode: i32) -> Result<*mut PrecisionDocument, String> {
    let report = import_report(path);
    let fingerprint = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_lab_psd(
            cpath(path).as_ptr(),
            mode,
            fingerprint.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(doc)
    }
}

fn export_report(doc: *const PrecisionDocument, container: i32, mode: i32) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_psd_export_report(doc, container, mode, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    json_of(text)
}

fn save(
    doc: *const PrecisionDocument,
    path: &Path,
    container: i32,
    mode: i32,
) -> Result<(), String> {
    let projection = unsafe { &*doc }.render(LIMIT, 0, None).unwrap();
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            &projection,
            cpath(path).as_ptr(),
            container,
            mode,
            LIMIT,
            0,
            &mut err,
        )
    };
    if ok {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

fn render16(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

fn then(doc: *mut PrecisionDocument, request: Value, input: &[u16]) -> *mut PrecisionDocument {
    let out = edit(doc, &request, input)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"));
    free(doc);
    out
}

fn with_opacity(doc: *mut PrecisionDocument, name: &str, opacity: f32) -> *mut PrecisionDocument {
    let layer = index(doc, name);
    let out = unsafe { dr_precision_doc_set_layer_opacity(doc, layer, opacity, ptr::null_mut()) };
    assert!(!out.is_null(), "{name} opacity");
    free(doc);
    out
}

// ------------------------------------------------------ the Lab document --

/// The Lab twin, from a new Lab document.
fn twin() -> Arc<IccProfile> {
    let request = CString::new(r#"{"depth":16,"fill_srgb":[1,1,1,1]}"#).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(1, 1, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    let profile = unsafe { &*doc }.profile().clone();
    free(doc);
    profile
}

/// A code on the document's grid: ×257 at 8 bits.
fn on_grid(v: u32, bits: u32) -> u16 {
    if bits == 8 {
        ((v % 256) * 257) as u16
    } else {
        (v % 65536) as u16
    }
}

/// Deterministic Lab codes of pixel `i` of a raster seeded `seed` (off the
/// 15-bit a\*/b\* grid at 16 bits), alpha partial on every third pixel when
/// `partial`.
fn pattern(i: u32, seed: u32, bits: u32, partial: bool) -> [u16; 4] {
    let wide = |k: u32| i * 7919 * k + seed * 1013 * k + 211 * k;
    let code = |k: u32| {
        if bits == 8 {
            on_grid(wide(k) % 251, 8)
        } else {
            on_grid(wide(k) * 37 + 5, 16)
        }
    };
    let alpha = if partial && i % 3 == 1 {
        on_grid(90 + i * 11, bits)
    } else {
        65535
    };
    [code(1), code(2), code(3), alpha]
}

fn raster(
    twin: &Arc<IccProfile>,
    name: &str,
    size: (u32, u32),
    offset: (i32, i32),
    seed: u32,
    bits: u32,
    partial: bool,
) -> PrecisionDocumentLayer {
    let pixels: Vec<[u16; 4]> = (0..size.0 * size.1)
        .map(|i| pattern(i, seed, bits, partial))
        .collect();
    let image = PrecisionImage::from_rgba16(size.0, size.1, pixels, twin.clone(), None).unwrap();
    PrecisionDocumentLayer::raster(name, image, offset, 1.0, true).unwrap()
}

/// The structured Lab document at `bits`, bottom-first: an opaque
/// Backdrop; a Pass Through `Group` holding a masked Multiply 50 % Shape
/// with partial alpha; a Normal 80 % `Pair` holding `Clip base` and an
/// Overlay `Clipped`; a `Hidden` layer; `Top` with a Color Overlay
/// (#40a0c8), Fill 60 % and Blend If on Lightness; and, when `levels`, a
/// Levels adjustment layer on top.
fn structured(bits: u32, levels: bool) -> *mut PrecisionDocument {
    let twin = twin();
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        twin.clone(),
        Metadata::default(),
        Resolution::default(),
        vec![
            raster(&twin, "Backdrop", (W, H), (0, 0), 1, bits, false),
            raster(&twin, "Shape", (5, 4), (2, 1), 2, bits, true),
            raster(&twin, "Clip base", (3, 3), (4, 2), 3, bits, false),
            raster(&twin, "Clipped", (4, 3), (3, 3), 4, bits, true),
            raster(&twin, "Hidden", (W, H), (0, 0), 5, bits, false),
            raster(&twin, "Top", (4, 3), (1, 2), 6, bits, false),
        ],
    )
    .unwrap()
    .with_lab_depth(bits)
    .unwrap();
    let mut doc = Box::into_raw(Box::new(doc));
    let mask: Vec<u16> = (0..20u32).map(|i| on_grid(255 - i * 11, bits)).collect();
    let shape = index(doc, "Shape");
    doc = then(
        doc,
        json!({"op":"set_mask","layer":shape,"width":5,"height":4,"x":2,"y":1,"linked":true}),
        &mask,
    );
    doc = then(
        doc,
        json!({"op":"layer_blend","version":1,"layer":shape,"blend_mode":1}),
        &[],
    );
    doc = with_opacity(doc, "Shape", 0.5);
    let clipped = index(doc, "Clipped");
    doc = then(
        doc,
        json!({"op":"layer_clipped","layer":clipped,"clipped":true}),
        &[],
    );
    doc = then(
        doc,
        json!({"op":"layer_blend","version":1,"layer":clipped,"blend_mode":3}),
        &[],
    );
    let hidden = index(doc, "Hidden");
    doc = then(
        doc,
        json!({"op":"layer_visible","layer":hidden,"visible":false}),
        &[],
    );
    let top = index(doc, "Top");
    doc = then(
        doc,
        json!({"op":"layer_style","version":1,"layer":top,"style":{"fill_opacity":0.6,"effects":[{"type":"color_overlay","color":OVERLAY,"opacity":0.7}]}}),
        &[],
    );
    doc = then(
        doc,
        json!({"op":"layer_blend_if","version":1,"layer":top,"blend_if":{"channel":"red","this_layer":[20,40,220,240],"underlying":[0,0,255,255]}}),
        &[],
    );
    let shape = index(doc, "Shape");
    doc = then(
        doc,
        json!({"op":"group_layers","layers":[shape],"name":"Group"}),
        &[],
    );
    let (base, clipped) = (index(doc, "Clip base"), index(doc, "Clipped"));
    doc = then(
        doc,
        json!({"op":"group_layers","layers":[base, clipped],"name":"Pair"}),
        &[],
    );
    let pair = index(doc, "Pair");
    doc = then(
        doc,
        json!({"op":"layer_blend","version":1,"layer":pair,"blend_mode":0}),
        &[],
    );
    doc = with_opacity(doc, "Pair", 0.8);
    if !levels {
        return doc;
    }
    let name = CString::new("Levels").unwrap();
    let request = CString::new(
        r#"{"type":"adjust","evaluation":"precision_v1","op":"levels","params":{"black":0.10196078431372549,"white":0.9019607843137255,"gamma":1.25}}"#,
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            name.as_ptr(),
            request.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    free(doc);
    out
}

/// A code as the file holds it, read back: at 8 bits its nearest byte ×257
/// (P8; a code already on the grid is itself), at 16 bits L\* and alpha
/// exact and a\*/b\* the 15-bit snap (the oracle's write then read):
/// `lab_psd_fixture`'s measured rule, independent of the writer.
fn snapped(bits: u32, channel: usize, code: u16) -> u16 {
    if bits == 8 {
        return ((u32::from(code) + 128) / 257 * 257) as u16;
    }
    if channel == 0 || channel == 3 {
        return code;
    }
    code_of(
        channel,
        psd_units(
            16,
            channel,
            psd_sample(16, channel, units_of(channel, code)),
        ),
    )
}

fn snap4(bits: u32, p: [u16; 4]) -> [u16; 4] {
    std::array::from_fn(|c| snapped(bits, c, p[c]))
}

/// An 8-bit `#rrggbb`'s bytes.
fn hex_bytes(hex: &str) -> [u8; 3] {
    std::array::from_fn(|c| u8::from_str_radix(&hex[1 + 2 * c..3 + 2 * c], 16).unwrap())
}

/// Two trees alike (module doc, (a)): `b` is `a` read back from a file.
fn same_tree(a: &[PrecisionDocumentLayer], b: &[PrecisionDocumentLayer], bits: u32, what: &str) {
    assert_eq!(a.len(), b.len(), "{what}");
    for (x, y) in a.iter().zip(b) {
        let at = format!("{what} {}", x.name());
        assert_eq!(x.name(), y.name(), "{at}");
        assert_eq!(x.children().is_some(), y.children().is_some(), "{at} kind");
        assert_eq!(x.blend_mode(), y.blend_mode(), "{at}");
        assert_eq!(x.is_pass_through(), y.is_pass_through(), "{at}");
        assert_eq!(
            (x.opacity() * 255.0).round(),
            (y.opacity() * 255.0).round(),
            "{at}"
        );
        assert_eq!(
            x.style()
                .map_or(255.0, |s| (s.fill_opacity * 255.0).round()),
            y.style()
                .map_or(255.0, |s| (s.fill_opacity * 255.0).round()),
            "{at} Fill"
        );
        assert_eq!(x.visible(), y.visible(), "{at}");
        assert_eq!(x.clipped(), y.clipped(), "{at}");
        assert_eq!(x.offset(), y.offset(), "{at}");
        assert_eq!(x.blend_if(), y.blend_if(), "{at} Blend If");
        match (x.source(), y.source()) {
            (Some(p), Some(q)) => {
                let (p, q) = (p.to_rgba16().unwrap(), q.to_rgba16().unwrap());
                assert_eq!(p.len(), q.len(), "{at}");
                for (i, (s, t)) in p.iter().zip(&q).enumerate() {
                    assert_eq!(snap4(bits, *s), *t, "{at} pixel {i}");
                    for c in 0..4 {
                        assert!(s[c].abs_diff(t[c]) <= 1, "{at} pixel {i} channel {c}");
                    }
                }
            }
            (None, None) => {}
            _ => panic!("{at}: kinds differ"),
        }
        match (x.mask(), y.mask()) {
            (Some(m), Some(n)) => {
                assert_eq!(n.plane().samples(), m.plane().samples(), "{at} mask");
                assert_eq!(n.enabled(), m.enabled(), "{at} mask enabled");
                assert_eq!(n.properties().offset, m.properties().offset, "{at}");
            }
            (None, None) => {}
            _ => panic!("{at}: mask presence differs"),
        }
        match (x.reported_style(), y.reported_style()) {
            (Some(s), Some(t)) => {
                let (s, t): (Value, Value) = (
                    serde_json::from_str(&s).unwrap(),
                    serde_json::from_str(&t).unwrap(),
                );
                let effects = |v: &Value| v["effects"].as_array().map_or(0, Vec::len);
                assert_eq!(effects(&s), effects(&t), "{at}: {s} against {t}");
                if effects(&s) > 0 {
                    assert_eq!(s["effects"][0]["type"], t["effects"][0]["type"], "{at}");
                    let (p, q) = (
                        hex_bytes(s["effects"][0]["color"].as_str().unwrap()),
                        hex_bytes(t["effects"][0]["color"].as_str().unwrap()),
                    );
                    for c in 0..3 {
                        assert!(p[c].abs_diff(q[c]) <= 1, "{at}: {s} against {t}");
                    }
                }
            }
            (None, None) => {}
            (s, t) => panic!("{at}: style {s:?} against {t:?}"),
        }
        if let (Some(p), Some(q)) = (x.children(), y.children()) {
            same_tree(p, q, bits, what);
        }
    }
}

const CONTAINERS: [(i32, &str); 3] = [(0, "psd"), (1, "psb"), (2, "tif")];

// ------------------------------------------------------------- (a) --

#[test]
fn a_lab_document_round_trips_through_psd_psb_and_tiff_at_eight_and_sixteen_bits() {
    let dir = TempDir::new().unwrap();
    for bits in [8u32, 16] {
        let source = structured(bits, false);
        let before = render16(source);
        for (container, name) in CONTAINERS {
            let what = format!("{name} {bits}");
            let report = export_report(source, container, 0);
            assert_eq!(report["available"], true, "{what}: {report}");
            assert_eq!(report["depth"], bits, "{what}");
            assert_eq!(report["color_mode"], "lab", "{what}");
            let path = dir.path().join(format!("rt{bits}.{name}"));
            save(source, &path, container, 0).unwrap_or_else(|e| panic!("{what}: {e}"));
            if container == 2 {
                assert!(
                    unsafe { dr_tiff_has_layers(cpath(&path).as_ptr()) },
                    "{what}"
                );
            }
            let notice = import_report(&path);
            assert_eq!(notice["import_precision"], "lab", "{what}: {notice}");
            let back = open(&path, 0).unwrap_or_else(|e| panic!("{what}: {e}"));
            let (a, b) = (unsafe { &*source }, unsafe { &*back });
            assert!(b.is_lab(), "{what}");
            assert_eq!(b.dimensions(), (W, H), "{what}");
            same_tree(a.root_layers(), b.root_layers(), bits, &what);
            let tolerance = if bits == 8 { 0 } else { 2 };
            for (i, (p, q)) in before.iter().zip(render16(back)).enumerate() {
                for c in 0..4 {
                    assert!(
                        p[c].abs_diff(q[c]) <= tolerance,
                        "{what} render {i}: {p:?} against {q:?}"
                    );
                }
            }
            // The second round trip is byte-identical: what was read back is
            // written again exactly as the file holds it.
            let first = std::fs::read(&path).unwrap();
            let again = dir.path().join(format!("again{bits}.{name}"));
            save(back, &again, container, 0).unwrap();
            let back2 = open(&again, 0).unwrap();
            let third = dir.path().join(format!("third{bits}.{name}"));
            save(back2, &third, container, 0).unwrap();
            assert!(
                std::fs::read(&again).unwrap() == std::fs::read(&third).unwrap(),
                "{what}: the second round trip changed the file"
            );
            if bits == 8 {
                assert!(
                    std::fs::read(&again).unwrap() == first,
                    "{what}: an 8-bit file is written again byte for byte"
                );
            }
            free(back2);
            free(back);

            // The stored composite (L1: three planes over Lab white; the
            // Backdrop is opaque, so it is the render itself, snapped).
            if container != 2 {
                let back = open(&path, 1).unwrap();
                let b = unsafe { &*back };
                assert_eq!(b.root_layers().len(), 1, "{what}");
                let samples = b.root_layers()[0].source().unwrap().to_rgba16().unwrap();
                for (p, q) in before.iter().zip(samples) {
                    let want = snap4(bits, [p[0], p[1], p[2], 65535]);
                    assert_eq!(want, q, "{what} composite");
                }
                free(back);
            }

            // Flattened: one layer, the projection.
            let flat = dir.path().join(format!("flat{bits}.{name}"));
            save(source, &flat, container, 1).unwrap_or_else(|e| panic!("{what}: {e}"));
            let back = open(&flat, 0).unwrap();
            let b = unsafe { &*back };
            assert_eq!(b.root_layers().len(), 1, "{what}");
            let samples = b.root_layers()[0].source().unwrap().to_rgba16().unwrap();
            for (p, q) in before.iter().zip(samples) {
                assert_eq!(snap4(bits, *p), q, "{what} flat");
            }
            free(back);
        }
        free(source);
    }
}

// ------------------------------------------------------------- (b) --

/// The Photoshop-writable 16-bit sample of grid value i (the take's
/// measured rule): round(i·65535/32768), ties up.
fn writable(i: u32) -> u16 {
    ((u64::from(i) * 65535 * 2 + 32768) / 65536) as u16
}

/// The document code of grid value i: round(i·257/128), ties up, at most
/// 65535 (a\* = 127).
fn grid_code(i: u32) -> u16 {
    ((u64::from(i) * 257 * 2 + 128) / 256).min(65535) as u16
}

const GRID_W: u32 = 256;
const GRID_H: u32 = 129;

#[test]
fn every_fifteen_bit_a_value_is_written_as_photoshop_writes_it() {
    let dir = TempDir::new().unwrap();
    let twin = twin();
    let values: Vec<u32> = (0..GRID_W * GRID_H).map(|i| i.min(32768)).collect();
    let pixels: Vec<[u16; 4]> = values
        .iter()
        .map(|&i| [30000, grid_code(i), 32896, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(GRID_W, GRID_H, pixels, twin.clone(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Grid", image, (0, 0), 1.0, true).unwrap();
    let doc = PrecisionDocument::from_parts(
        GRID_W,
        GRID_H,
        twin,
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
    .with_lab_depth(16)
    .unwrap();
    let doc = Box::into_raw(Box::new(doc));
    let path = dir.path().join("grid.psd");
    save(doc, &path, 0, 0).unwrap();
    let file = fx::read(&std::fs::read(&path).unwrap());
    let stored = file.by_name("Grid").plane(1).unwrap().to_vec();
    for (k, &i) in values.iter().enumerate() {
        let code = grid_code(i);
        assert_eq!(
            stored[k],
            psd_sample(16, 1, units_of(1, code)),
            "i {i}: the oracle's sample"
        );
        let want = if i <= 32640 {
            writable(i)
        } else {
            writable(32640)
        };
        assert_eq!(stored[k], want, "i {i}");
    }
    assert_eq!(writable(32640), 65279, "the a* = 127 sample");
    // i ≤ 32640 reads back as its own code and is written again as itself.
    let back = open(&path, 0).unwrap();
    let codes = unsafe { &*back }.root_layers()[0]
        .source()
        .unwrap()
        .to_rgba16()
        .unwrap();
    for (k, &i) in values.iter().enumerate() {
        assert_eq!(codes[k][1], grid_code(i), "i {i} read back");
    }
    let again = dir.path().join("again.psd");
    save(back, &again, 0, 0).unwrap();
    let file = fx::read(&std::fs::read(&again).unwrap());
    assert_eq!(file.by_name("Grid").plane(1).unwrap(), &stored[..]);
    free(back);
    free(doc);

    // i ∈ 32641…32768: Photoshop's own samples, past the document's a*
    // ceiling, read as 65535 and are written as the i = 32640 sample.
    let high: Vec<u32> = (32641..=32768).collect();
    let width = high.len();
    let samples: Vec<[u16; 4]> = high
        .iter()
        .map(|&i| [30000, writable(i), 32768, 65535])
        .collect();
    let composite: Vec<Vec<u16>> = (0..3)
        .map(|c| samples.iter().map(|s| s[c]).collect())
        .collect();
    let spec = PsdSpec {
        canvas: (width as u32, 1),
        depth: 16,
        icc: None,
        composite,
        merged_alpha: false,
        large: false,
        lr16: false,
        resources: Vec::new(),
    };
    let records = [Rec::raster(
        "High",
        [0, 0, 1, width as i32],
        lab_channels(&samples, width, 16, false, Comp::Rle, false),
    )];
    let high_path = dir.path().join("high.psd");
    std::fs::write(&high_path, lab_psd(&spec, &records)).unwrap();
    let back = open(&high_path, 0).unwrap();
    let codes = unsafe { &*back }.root_layers()[0]
        .source()
        .unwrap()
        .to_rgba16()
        .unwrap();
    assert!(codes.iter().all(|p| p[1] == 65535), "{codes:?}");
    let out = dir.path().join("high_again.psd");
    save(back, &out, 0, 0).unwrap();
    let file = fx::read(&std::fs::read(&out).unwrap());
    assert!(file
        .by_name("High")
        .plane(1)
        .unwrap()
        .iter()
        .all(|v| *v == writable(32640)));
    free(back);
}

// ------------------------------------------------------------- (c) --

fn photoshop(job: &str, name: &str) -> Option<Vec<u8>> {
    let root = PathBuf::from(std::env::var("HOME").ok()?)
        .join("devel/doctored-fixtures/photoshop/lab-files-547/take2");
    if root.join("decided.json").is_file() {
        Some(std::fs::read(root.join("jobs").join(job).join(name)).unwrap())
    } else {
        eprintln!("SKIP: the Photoshop take {} is absent", root.display());
        None
    }
}

/// Our export of the structured document at `bits` in `container`.
fn exported(dir: &TempDir, bits: u32, container: i32, name: &str) -> Vec<u8> {
    let source = structured(bits, false);
    let path = dir.path().join(name);
    save(source, &path, container, 0).unwrap();
    free(source);
    std::fs::read(path).unwrap()
}

fn be16(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}

fn be32(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

/// A big-endian TIFF's first directory (tag → (type, count, SHORT/LONG
/// values)) and its tag-37724 `Layr`/`Lr16` body (TIFF 6.0 and Adobe's
/// Photoshop TIFF notes).
type Tags = HashMap<u16, (u16, usize, Vec<u32>)>;

fn tiff_layout(bytes: &[u8]) -> (Tags, Vec<u8>) {
    assert_eq!(&bytes[..4], b"MM\0*");
    let ifd = be32(bytes, 4);
    let mut tags = HashMap::new();
    let mut block = Vec::new();
    for e in 0..usize::from(be16(bytes, ifd)) {
        let at = ifd + 2 + 12 * e;
        let (tag, kind, n) = (be16(bytes, at), be16(bytes, at + 2), be32(bytes, at + 4));
        let size = match kind {
            3 => 2,
            4 => 4,
            5 => 8,
            _ => 1,
        };
        let start = if size * n <= 4 {
            at + 8
        } else {
            be32(bytes, at + 8)
        };
        let values = match kind {
            3 => (0..n)
                .map(|k| u32::from(be16(bytes, start + 2 * k)))
                .collect(),
            4 => (0..n).map(|k| be32(bytes, start + 4 * k) as u32).collect(),
            _ => Vec::new(),
        };
        if tag == 37724 {
            block = bytes[start..start + n].to_vec();
        }
        tags.insert(tag, (kind, n, values));
    }
    let header = b"Adobe Photoshop Document Data Block\0".len();
    assert!(&block[header + 4..header + 8] == b"Layr" || &block[header + 4..header + 8] == b"Lr16");
    let len = be32(&block, header + 8);
    (tags, block[header + 12..header + 12 + len].to_vec())
}

/// The facts L4 fixes on a layered Lab TIFF at `bits`.
fn l4(bytes: &[u8], bits: u32, what: &str) {
    let (tags, info) = tiff_layout(bytes);
    let value = |tag: u16| tags[&tag].2.clone();
    assert_eq!(value(277), vec![3], "{what} SamplesPerPixel");
    assert_eq!(value(262), vec![8], "{what} Photometric CIELab");
    assert_eq!(value(258), vec![bits; 3], "{what} BitsPerSample");
    assert_eq!(value(259), vec![1], "{what} uncompressed");
    for absent in [338u16, 339, 34675] {
        assert!(!tags.contains_key(&absent), "{what}: tag {absent}");
    }
    let count = be16(&info, 0) as i16;
    assert!(count > 0, "{what}: a positive count {count}");
    let (width, height) = (value(256)[0], value(257)[0]);
    assert_eq!(
        value(279),
        vec![width * height * 3 * bits / 8],
        "{what} strip"
    );
}

#[test]
fn l1_l2_the_header_holds_three_channels_a_positive_count_and_no_profile() {
    let dir = TempDir::new().unwrap();
    for (bits, container, ours, theirs) in [
        (8u32, 0, "l1.psd", "layers8.psd"),
        (16, 0, "l1_16.psd", "layers16.psd"),
        (16, 1, "l1.psb", "layers16.psb"),
        (8, 1, "l1_8.psb", ""),
    ] {
        let file = fx::read(&exported(&dir, bits, container, ours));
        assert_eq!(
            (file.channels, file.depth, file.color),
            (3, bits as u16, 9),
            "{ours}"
        );
        assert!(!file.merged_alpha, "{ours}: a positive count");
        assert!(!file.resource_ids.contains(&1039), "{ours}: no ICC");
        assert_eq!(file.composite.len(), 3, "{ours}");
        if theirs.is_empty() {
            continue;
        }
        if let Some(bytes) = photoshop("layers2", theirs) {
            let file = fx::read(&bytes);
            assert_eq!(
                (file.channels, file.depth, file.color),
                (3, bits as u16, 9),
                "{theirs}"
            );
            assert!(
                !file.merged_alpha,
                "{theirs}: Photoshop's count is positive"
            );
            assert!(!file.resource_ids.contains(&1039), "{theirs}: no ICC");
        }
    }
}

#[test]
fn l3_folders_and_blend_if_take_photoshops_lab_forms() {
    let dir = TempDir::new().unwrap();
    let file = fx::read(&exported(&dir, 8, 0, "l3.psd"));
    let group = file.by_name("Group");
    let pair = file.by_name("Pair");
    for folder in [group, pair] {
        let lsct = folder.block(b"lsct").expect("the folder's lsct");
        assert_eq!(lsct.len(), 16, "{}", folder.name);
        assert_eq!(&lsct[4..8], b"8BIM");
        assert_eq!(&lsct[12..], &[0, 0, 0, 0]);
        assert_eq!(folder.blocks[1].0, *b"lsct", "lsct right after luni");
    }
    // Blend If on Lightness: slot 1, the composite slot 0 the default.
    let top = file.by_name("Top");
    assert_eq!(top.ranges.len(), 40);
    assert_eq!(&top.ranges[..8], &IDENTITY_RANGE);
    assert_eq!(&top.ranges[8..16], &[20, 40, 220, 240, 0, 0, 255, 255]);
    // The Color Overlay's colour is an `LbCl`, never an `RGBC`.
    let lfx2 = top.block(b"lfx2").expect("Top's effects");
    let has = |key: &[u8]| lfx2.windows(key.len()).any(|w| w == key);
    assert!(has(b"LbCl") && !has(b"RGBC"), "Top's lfx2");
    let Some(bytes) = photoshop("layers2", "layers8.psd") else {
        return;
    };
    let theirs = fx::read(&bytes);
    let normal = theirs.by_name("Normal group").block(b"lsct").unwrap();
    assert_eq!(normal, b"\0\0\0\x018BIMnorm\0\0\0\0");
    assert_eq!(pair.block(b"lsct").unwrap(), normal, "the Normal folder");
    let pass = theirs.by_name("Pass group").block(b"lsct").unwrap();
    assert_eq!(
        group.block(b"lsct").unwrap(),
        pass,
        "the Pass Through folder"
    );
    let Some(bytes) = photoshop("styled2", "styled8.psd") else {
        return;
    };
    let theirs = fx::read(&bytes);
    let ranged = theirs.by_name("Ranged");
    assert_eq!(&ranged.ranges[..8], &IDENTITY_RANGE);
    assert_eq!(&ranged.ranges[8..16], &[0, 0, 255, 255, 0, 0, 0x60, 0xa0]);
    let overlay = theirs.by_name("Overlay").block(b"lfx2").unwrap();
    assert!(overlay.windows(4).any(|w| w == b"LbCl"));
}

#[test]
fn l4_the_layered_tiff_is_three_cielab_samples_with_a_positive_count() {
    let dir = TempDir::new().unwrap();
    for bits in [8u32, 16] {
        let name = format!("l4_{bits}.tif");
        let bytes = exported(&dir, bits, 2, &name);
        l4(&bytes, bits, &name);
        // The strip is the composite over Lab white as CIELab samples: the
        // Backdrop is opaque, so it is the render itself (TIFF 6.0 §23).
        let source = structured(bits, false);
        let before = render16(source);
        free(source);
        let strip = read_lab_tiff(&bytes);
        assert_eq!((strip.samples, strip.photometric), (3, 8), "{name}");
        for (i, p) in before.iter().enumerate() {
            for (c, &code) in p.iter().take(3).enumerate() {
                let back = code_of(c, cielab_units(bits as u16, c, strip.planes[c][i]));
                // 8 bits the nearest byte exactly; 16 bits within the 1/256
                // a* step.
                let (want, bound) = if bits == 8 {
                    (snapped(8, c, code), 0)
                } else {
                    (code, 1)
                };
                assert!(
                    back.abs_diff(want) <= bound,
                    "{name} pixel {i} channel {c}: {back} against {want}"
                );
            }
        }
    }
    for (name, bits) in [("layers8.tif", 8u32), ("layers16.tif", 16)] {
        if let Some(bytes) = photoshop("layers2", name) {
            l4(&bytes, bits, name);
        }
    }
}

// ------------------------------------------------------------- (d) --

#[test]
fn lbcl_both_ways_is_the_identity_on_an_srgb_grid() {
    let steps: Vec<u8> = (0..16).map(|k| (k * 17) as u8).collect();
    for &r in &steps {
        for &g in &steps {
            for &b in &steps {
                let lab = PrecisionDocument::lbcl_of_srgb([r, g, b]);
                assert_eq!(PrecisionDocument::srgb_of_lbcl(lab), [r, g, b], "{lab:?}");
            }
        }
    }
    let overlay = hex_bytes(OVERLAY);
    let lab = PrecisionDocument::lbcl_of_srgb(overlay);
    assert_eq!(PrecisionDocument::srgb_of_lbcl(lab), overlay);
    // Against the CIE oracle (D50, the profile's own sRGB matrix).
    let cie = srgb_to_lab(overlay.map(|v| f64::from(v) / 255.0));
    for c in 0..3 {
        assert!((lab[c] - cie[c]).abs() < 1.0, "{lab:?} against {cie:?}");
    }
    // Through a file: the Color Overlay of #40a0c8 reads back exactly.
    let dir = TempDir::new().unwrap();
    for bits in [8u32, 16] {
        let source = structured(bits, false);
        let path = dir.path().join(format!("overlay{bits}.psd"));
        save(source, &path, 0, 0).unwrap();
        let back = open(&path, 0).unwrap();
        let top = index(back, "Top");
        let style: Value = serde_json::from_str(
            &unsafe { &*back }
                .layer(top)
                .unwrap()
                .reported_style()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(style["effects"][0]["color"], OVERLAY, "{style}");
        free(back);
        free(source);
    }
}

#[test]
fn an_adjustment_layer_is_written_as_its_stamp_with_a_loss() {
    let dir = TempDir::new().unwrap();
    for bits in [8u32, 16] {
        let source = structured(bits, true);
        let levels = index(source, "Levels");
        assert!(unsafe { &*source }.adjustment_description(levels).is_some());
        let report = export_report(source, 0, 0);
        assert_eq!(report["available"], true, "{report}");
        assert!(
            report["losses"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l.as_str().unwrap().contains("Levels")),
            "{report}"
        );
        let path = dir.path().join(format!("levels{bits}.psd"));
        save(source, &path, 0, 0).unwrap();
        let back = open(&path, 0).unwrap();
        let levels = index(back, "Levels");
        let b = unsafe { &*back };
        assert!(b.adjustment_description(levels).is_none(), "a raster");
        // The stamp is the picture up to and including the Levels, the top
        // layer: the source's render, as the file stores it.
        let stamp = b
            .layer(levels)
            .unwrap()
            .source()
            .unwrap()
            .to_rgba16()
            .unwrap();
        for (p, q) in render16(source).iter().zip(stamp) {
            assert_eq!(snap4(bits, *p), q, "{bits}: the stamp");
        }
        free(back);
        free(source);
    }
}

#[test]
fn the_report_names_lab_its_depth_and_no_profile() {
    for bits in [8u32, 16] {
        let source = structured(bits, false);
        for (container, name) in CONTAINERS {
            for mode in [0, 1] {
                let what = format!("{name} {bits} mode {mode}");
                let report = export_report(source, container, mode);
                assert_eq!(report["available"], true, "{what}: {report}");
                assert_eq!(report["color_mode"], "lab", "{what}");
                assert_eq!(report["depth"], bits, "{what}");
                assert!(
                    report["general_limitations"][0]
                        .as_str()
                        .unwrap()
                        .starts_with("A Lab Color document is written as a Lab Color"),
                    "{what}: {report}"
                );
                assert!(
                    report["observations"]
                        .to_string()
                        .contains("No profile is embedded"),
                    "{what}: {report}"
                );
                assert!(
                    !report["observations"].to_string().contains("The embedded"),
                    "{what}: {report}"
                );
            }
        }
        free(source);
    }
}

#[test]
fn a_null_projection_is_refused_by_name() {
    let source = structured(16, false);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("x.psd");
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            source,
            ptr::null(),
            cpath(&path).as_ptr(),
            0,
            0,
            LIMIT,
            0,
            &mut err,
        )
    };
    assert!(!ok);
    assert_eq!(
        take_err_string(err),
        "a Lab Color document's PSD export requires its projection (required handle is NULL)"
    );
    assert!(!path.exists());
    free(source);
}

// ------------------------------------------------------------- (e) --

const CELL: u32 = 8;

#[test]
fn the_takes_files_render_as_photoshop_flattened_them() {
    let Some(root) = std::env::var("HOME").ok().map(|home| {
        PathBuf::from(home).join("devel/doctored-fixtures/photoshop/lab-export-547/take1")
    }) else {
        eprintln!("SKIP: HOME is unset, so the Photoshop take cannot be found");
        return;
    };
    let judged = std::fs::read_to_string(root.join("judged.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    if judged
        .as_ref()
        .map(|j| j["verdict"] != "PASS")
        .unwrap_or(true)
    {
        eprintln!(
            "SKIP: the readback take {} is absent or not PASS",
            root.display()
        );
        return;
    }
    for name in ["rz8.psd", "rz8.tif", "rz16.psd", "rz16.psb", "rz16.tif"] {
        let doc = open(&root.join("ours").join(name), 0).unwrap();
        let d = unsafe { &*doc };
        let (w, h) = d.dimensions();
        let codes = render16(doc);
        let flat = root.join("jobs/inspect").join(format!("{name}.flat.tif"));
        let tiff = read_lab_tiff(&std::fs::read(&flat).unwrap());
        assert_eq!(tiff.photometric, 8, "{}", flat.display());
        let mut worst = 0;
        for row in 0..h / CELL {
            for col in 0..w / CELL {
                let i = ((row * CELL + CELL / 2) * w + col * CELL + CELL / 2) as usize;
                for (c, &ours) in codes[i].iter().take(3).enumerate() {
                    let theirs = code_of(c, cielab_units(tiff.bits, c, tiff.planes[c][i]));
                    let diff = ours.abs_diff(theirs);
                    worst = worst.max(diff);
                    assert!(diff <= 257, "{name} cell ({col}, {row}) channel {c}");
                }
            }
        }
        eprintln!("{name}: max {worst}");
        free(doc);
    }
}
