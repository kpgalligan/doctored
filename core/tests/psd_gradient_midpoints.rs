//! Gradient stop midpoints through PSD and `.asl`: a Gradient Fill
//! layer's `GdFl`, a Gradient Overlay's and a Stroke Gradient's `lfx2` at 8
//! bits, and an `.asl` style library, each writing our stop `i`'s
//! round(midpoint × 100) as `Mdpn` on Photoshop's colour stop (`Clrt`)
//! `i + 1` and round(opacity_midpoint × 100) on its transparency stop
//! (`TrnS`) `i + 1`, stop 0 50 — read back by `psd_gradient_fill_layer`'s
//! own descriptor walker, never by the core — and each reading the same
//! midpoints and the same pixels within a code back. A Photoshop-shaped
//! `GdFl` whose second colour stop holds `Mdpn` 30 reads as the first
//! stop's 0.3 in `psd_gradient_fill_layer`'s loss test (`Mid`).
//!
//! Which segment a stop's `Mdpn` governs: in the model stop `i`'s midpoint
//! governs segment `i..i + 1`; in Photoshop's file segment `i` reads stop
//! `i + 1`'s `Mdpn` (`gradient_map_678_take.py`'s confirm take: a
//! four-stop Gradient Fill with distinct midpoints, 1.0 / 0.005 codes under
//! that reading, 63 / 21.9 under stop `i`'s own). Four stops with distinct
//! midpoints pin the translation both ways, written and read, against the
//! pixels (`photoshops_next_stop_mdpn_governs_each_segment_both_ways`), and
//! a `Grdn` an earlier build wrote (stop `i`'s own `Mdpn` on stop `i`)
//! re-reads its midpoints one stop earlier, the deliberate re-read change
//! (`an_earlier_builds_grdn_reimports_its_midpoints_one_stop_earlier`).

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_artwork::dr_doc_artwork;
use doctored_core::ffi_style::{dr_asl_decode, dr_asl_encode};
use doctored_core::icc::IccProfile;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{ffi_style, midpoint_t, smooth_ramp, solid, styled, take_err_string};
use crate::psd_descriptor_fixture as builder;
use crate::psd_gradient_fill_layer::{
    empty8, gdfl_of, n, parse, read_body, render, rgbc_items, Gd, W,
};
use crate::psd_mask_fixture as fixture8;
use crate::psd_pattern_fill_layer::{framed_records, open8, owned, save8};
use crate::psd_solid_fill_layer::{block, record};

const LIMIT: u64 = doctored_core::precision_image::REFERENCE_WORKING_BYTES;
const H: u32 = 8;

/// Three stops: colour midpoint 0.25 on the first, opacity midpoint 0.7 on
/// the second.
fn gradient(align: bool) -> Value {
    json!({"stops": [
        {"position": 0, "color": "#203040", "opacity": 1, "midpoint": 0.25},
        {"position": 0.4, "color": "#f08040", "opacity": 0.4, "opacity_midpoint": 0.7},
        {"position": 1, "color": "#40c0e0", "opacity": 1}],
        "style": "linear", "angle": 0, "scale": 1, "reverse": false,
        "align_with_layer": align})
}

/// The stops' `Mdpn` under `prefix` (`Grad`, `GrFl/Grad`, `FrFX/Grad`):
/// (colour, transparency) per stop.
fn mdpn(flat: &crate::psd_gradient_fill_layer::Flat, prefix: &str) -> Vec<(f64, f64)> {
    (0..3)
        .map(|i| {
            (
                n(flat, &format!("{prefix}/Clrs/{i}/Mdpn")),
                n(flat, &format!("{prefix}/Trns/{i}/Mdpn")),
            )
        })
        .collect()
}

/// Photoshop stop `i + 1` carries our stop `i`'s midpoints (module doc).
const WANT: [(f64, f64); 3] = [(50.0, 50.0), (25.0, 50.0), (50.0, 70.0)];

fn assert_midpoints(stops: &Value, what: &str) {
    assert_eq!(stops[0]["midpoint"], json!(0.25), "{what}: {stops}");
    assert!(
        stops[0].get("opacity_midpoint").is_none(),
        "{what}: {stops}"
    );
    assert_eq!(stops[1]["opacity_midpoint"], json!(0.7), "{what}: {stops}");
    assert!(stops[1].get("midpoint").is_none(), "{what}: {stops}");
    assert!(stops[2].get("midpoint").is_none(), "{what}: {stops}");
}

fn near(a: &[u8], b: &[u8], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!(x.abs_diff(*y) <= 1, "{what} sample {i}: {x} vs {y}");
    }
}

#[test]
fn a_gradient_fill_writes_and_reads_its_midpoints_as_mdpn() {
    let dir = TempDir::new().unwrap();
    let g = gradient(false);
    let colors = [
        [32.0 / 255.0, 48.0 / 255.0, 64.0 / 255.0],
        [240.0 / 255.0, 128.0 / 255.0, 64.0 / 255.0],
        [64.0 / 255.0, 192.0 / 255.0, 224.0 / 255.0],
    ];
    let components = json!([{"kind": "gradient", "gradient": g, "colors": colors}]).to_string();
    let description = json!({"type": "fill", "version": 1, "fill": "gradient",
        "gradient": g, "canvas": [W, H]})
    .to_string();
    let image = render(&IccProfile::srgb(), &components, (W, H));
    let doc = DrDocument::from_pixels(solid(W, H, [255; 4]));
    let (name, text) = (
        CString::new("Grad").unwrap(),
        CString::new(description).unwrap(),
    );
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_artwork(
            &doc,
            0,
            false,
            &image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    let doc = *unsafe { Box::from_raw(out) };
    let path = dir.path().join("grad.psd");
    save8(&doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(mdpn(&gdfl_of(&bytes, "Grad"), "Grad"), WANT);
    let back = open8(&path);
    let d = parse(back.layers[1].meta.as_deref().expect("a description"));
    assert_midpoints(&d["gradient"]["stops"], "GdFl");
    near(
        back.layers[1].pixels.to_contiguous().unwrap().as_raw(),
        doc.layers[1].pixels.to_contiguous().unwrap().as_raw(),
        "GdFl pixels",
    );
}

/// A layer styled with `effect` (one Gradient Overlay or Stroke whose
/// gradient is [`gradient`]).
fn effect_doc(effect: Value) -> DrDocument {
    let doc = DrDocument::from_pixels(solid(W, H, [255; 4]))
        .adding_image_layer(0, solid(W - 16, H - 4, [200, 20, 20, 255]), "FX")
        .unwrap();
    let doc = doc.with_layer_offset(1, 8, 2).unwrap();
    styled(&doc, 1, &json!({"effects": [effect]}).to_string())
}

#[test]
fn an_overlay_and_a_stroke_write_and_read_their_midpoints_as_mdpn() {
    let dir = TempDir::new().unwrap();
    for (key, effect) in [
        (
            "GrFl",
            json!({"type": "gradient_overlay", "gradient": gradient(true)}),
        ),
        (
            "FrFX",
            json!({"type": "stroke", "size": 3, "position": "outside",
                "fill_type": "gradient", "gradient": gradient(true)}),
        ),
    ] {
        let doc = effect_doc(effect);
        let path = dir.path().join(format!("{key}.psd"));
        save8(&doc, &path);
        let bytes = std::fs::read(&path).unwrap();
        // `lfx2`: the object-effects version, then the descriptor.
        let records = framed_records(&bytes);
        let lfx2 = block(record(&records, "FX"), b"lfx2");
        let flat = read_body(&lfx2[4..]);
        assert_eq!(mdpn(&flat, &format!("{key}/Grad")), WANT, "{key}");
        let back = open8(&path);
        let style: Value =
            serde_json::from_str(&ffi_style(&back as *const DrDocument, 1).expect("a style"))
                .unwrap();
        assert_midpoints(&style["effects"][0]["gradient"]["stops"], key);
        near(
            back.flattened().unwrap().as_raw(),
            doc.flattened().unwrap().as_raw(),
            key,
        );
    }
}

fn base64(text: &str) -> Vec<u8> {
    let value = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("not base64: {other}"),
        }
    };
    let mut out = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let mut v = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            v |= if c == b'=' { 0 } else { value(c) } << (18 - 6 * i);
        }
        out.extend_from_slice(&v.to_be_bytes()[1..4 - pad]);
    }
    out
}

#[test]
fn an_asl_style_writes_and_reads_its_midpoints() {
    let style = json!({"effects": [
        {"type": "gradient_overlay", "gradient": gradient(true)},
        {"type": "stroke", "size": 3, "fill_type": "gradient", "gradient": gradient(true)}]});
    let presets = CString::new(json!([{"name": "M", "style": style}]).to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let raw = unsafe { dr_asl_encode(presets.as_ptr(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    let out: Value = serde_json::from_str(&take_err_string(raw)).unwrap();
    assert_eq!(out["skipped"], json!([]));
    let bytes = base64(out["asl"].as_str().unwrap());
    let raw = unsafe { dr_asl_decode(bytes.as_ptr(), bytes.len(), &mut err) };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    let back: Value = serde_json::from_str(&take_err_string(raw)).unwrap();
    let effects = back["styles"][0]["style"]["effects"]
        .as_array()
        .expect("effects")
        .clone();
    let of = |kind: &str| {
        effects
            .iter()
            .find(|e| e["type"] == kind)
            .unwrap_or_else(|| panic!("{kind} in {back}"))
            .clone()
    };
    assert_midpoints(&of("gradient_overlay")["gradient"]["stops"], ".asl overlay");
    assert_midpoints(&of("stroke")["gradient"]["stops"], ".asl stroke");
}

// ------------------------------------------------- stop i → segment i..i+1 --

/// The segment test's canvas width: 200 columns, so every segment spans at
/// least 80 of them.
const SEG_W: u32 = 200;

/// The segment test's greys on the byte grid (0, 1, 0.2, 0.9 → 230 / 255).
const SEG_GREY: [u8; 4] = [0, 255, 51, 230];

/// The segment test's stop opacities.
const SEG_ALPHA: [f64; 4] = [1.0, 0.2, 1.0, 0.3];

/// The segment test's colour and opacity midpoints as `Mdpn`, the last
/// stop's 50 (none).
const SEG_MDPN: [i32; 4] = [25, 75, 40, 50];
const SEG_TMDPN: [i32; 4] = [70, 30, 60, 50];

/// The value of `stops` at `t` over `mids`, stop `i`'s midpoint governing
/// segment `i..i + 1` (`common::midpoint_t`), linear (Smoothness 0).
fn seg_want(stops: &[(f64, f64)], mids: &[f64], t: f64) -> f64 {
    smooth_ramp(stops, midpoint_t(stops, mids, t), 0.0) * 255.0
}

/// The same with the other assignment: segment `i` reads stop `i + 1`'s
/// midpoint (the last segment the last stop's).
fn seg_shifted(stops: &[(f64, f64)], mids: &[f64], t: f64) -> f64 {
    let mut shifted: Vec<f64> = mids[1..].to_vec();
    shifted.push(0.5);
    seg_want(stops, &shifted, t)
}

/// Row 0 of `doc`'s layer 1 against the stop-`i` assignment within a code,
/// grey (red) and alpha each over their own midpoints, and each segment
/// missing the shifted assignment by more than 8 codes somewhere.
fn assert_segments(
    doc: &DrDocument,
    positions: &[f64; 4],
    mids: &[f64; 4],
    tmids: &[f64; 4],
    what: &str,
) {
    let pixels = doc.layers[1].pixels.to_contiguous().unwrap();
    assert_eq!(pixels.width(), SEG_W, "{what}: a canvas-wide layer");
    let grey: Vec<(f64, f64)> = (0..4)
        .map(|i| (positions[i], f64::from(SEG_GREY[i]) / 255.0))
        .collect();
    let alpha: Vec<(f64, f64)> = (0..4).map(|i| (positions[i], SEG_ALPHA[i])).collect();
    for (channel, stops, mids, name) in [(0usize, &grey, mids, "grey"), (3, &alpha, tmids, "alpha")]
    {
        let mut apart = [0.0f64; 3];
        for x in 0..SEG_W {
            let t = f64::from(x) / f64::from(SEG_W);
            let got = f64::from(pixels.get_pixel(x, 0)[channel]);
            let want = seg_want(stops, mids, t);
            assert!(
                (got - want).abs() <= 1.0,
                "{what} {name} x {x}: {got} against {want}"
            );
            let segment = (0..3).rfind(|&i| t >= stops[i].0).unwrap_or(0);
            let miss = (got - seg_shifted(stops, mids, t)).abs();
            apart[segment] = apart[segment].max(miss);
        }
        for (segment, miss) in apart.iter().enumerate() {
            assert!(
                *miss > 8.0,
                "{what} {name} segment {segment}: the shifted assignment is only {miss} off"
            );
        }
    }
}

#[test]
fn photoshops_next_stop_mdpn_governs_each_segment_both_ways() {
    let dir = TempDir::new().unwrap();
    let mids = SEG_MDPN.map(|m| f64::from(m) / 100.0);
    let tmids = SEG_TMDPN.map(|m| f64::from(m) / 100.0);

    // Write: a Gradient Fill whose four stops carry distinct midpoints.
    let positions = [0.0, 0.3, 0.6, 1.0];
    let stops: Vec<Value> = (0..4)
        .map(|i| {
            let g = SEG_GREY[i];
            let mut s = json!({"position": positions[i],
                "color": format!("#{g:02x}{g:02x}{g:02x}"), "opacity": SEG_ALPHA[i]});
            if mids[i] != 0.5 {
                s["midpoint"] = json!(mids[i]);
            }
            if tmids[i] != 0.5 {
                s["opacity_midpoint"] = json!(tmids[i]);
            }
            s
        })
        .collect();
    let g = json!({"stops": stops, "style": "linear", "angle": 0, "scale": 1,
        "reverse": false, "align_with_layer": false, "smoothness": 0});
    let colors: Vec<[f64; 3]> = SEG_GREY
        .iter()
        .map(|&v| [f64::from(v) / 255.0; 3])
        .collect();
    let components = json!([{"kind": "gradient", "gradient": g, "colors": colors}]).to_string();
    let description = json!({"type": "fill", "version": 1, "fill": "gradient",
        "gradient": g, "canvas": [SEG_W, H]})
    .to_string();
    let image = render(&IccProfile::srgb(), &components, (SEG_W, H));
    let doc = DrDocument::from_pixels(solid(SEG_W, H, [255; 4]));
    let (name, text) = (
        CString::new("Seg").unwrap(),
        CString::new(description).unwrap(),
    );
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_artwork(
            &doc,
            0,
            false,
            &image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    let doc = *unsafe { Box::from_raw(out) };
    assert_segments(&doc, &positions, &mids, &tmids, "rendered");
    let path = dir.path().join("segments.psd");
    save8(&doc, &path);
    let flat = gdfl_of(&std::fs::read(&path).unwrap(), "Seg");
    // Photoshop stop i + 1 carries our stop i's; stop 0 writes 50.
    for i in 0..4 {
        let (clrt, trns) = if i == 0 {
            (50, 50)
        } else {
            (SEG_MDPN[i - 1], SEG_TMDPN[i - 1])
        };
        assert_eq!(
            n(&flat, &format!("Grad/Clrs/{i}/Mdpn")),
            f64::from(clrt),
            "Clrt {i}"
        );
        assert_eq!(
            n(&flat, &format!("Grad/Trns/{i}/Mdpn")),
            f64::from(trns),
            "TrnS {i}"
        );
    }
    let back = open8(&path);
    let d = parse(back.layers[1].meta.as_deref().expect("a description"));
    let read = &d["gradient"]["stops"];
    for i in 0..3 {
        assert_eq!(
            read[i]["midpoint"],
            json!(mids[i]),
            "written stop {i}: {read}"
        );
        assert_eq!(
            read[i]["opacity_midpoint"],
            json!(tmids[i]),
            "written stop {i}: {read}"
        );
    }
    assert!(read[3].get("midpoint").is_none(), "{read}");
    assert!(read[3].get("opacity_midpoint").is_none(), "{read}");
    assert_segments(&back, &positions, &mids, &tmids, "written and opened");

    // Read: a Photoshop-shaped GdFl with the same stops, Photoshop stop
    // i + 1 carrying segment i's `Mdpn`, the FIRST stop's off 50 (it
    // governs nothing).
    let clrt = [10, 25, 75, 40];
    let trns = [90, 70, 30, 60];
    let opened = photoshop_shaped(&dir, "photoshop_shaped.psd", clrt, trns);
    let read = &opened.1;
    for i in 0..3 {
        assert_eq!(
            read[i]["midpoint"],
            json!(f64::from(clrt[i + 1]) / 100.0),
            "read stop {i}: {read}"
        );
        assert_eq!(
            read[i]["opacity_midpoint"],
            json!(f64::from(trns[i + 1]) / 100.0),
            "read stop {i}: {read}"
        );
    }
    assert!(read[3].get("midpoint").is_none(), "{read}");
    assert!(read[3].get("opacity_midpoint").is_none(), "{read}");
    let read_positions = SEG_LCTN.map(|l| f64::from(l) / 4096.0);
    assert_segments(&opened.0, &read_positions, &mids, &tmids, "read");
}

const SEG_LCTN: [i32; 4] = [0, 1229, 2458, 4096];

/// A Photoshop-shaped GdFl of the segment test's stops whose colour and
/// transparency stops carry `clrt` and `trns` as `Mdpn` (built with the
/// test's own descriptor `builder`), opened: the document and its stops.
fn photoshop_shaped(
    dir: &TempDir,
    name: &str,
    clrt: [i32; 4],
    trns: [i32; 4],
) -> (DrDocument, Value) {
    let grey: Vec<[f64; 3]> = SEG_GREY.iter().map(|&v| [f64::from(v); 3]).collect();
    let body = Gd::new(rgbc_items(&grey), &SEG_LCTN).edited(|g| {
        for i in 0..4 {
            g.stops[i][3] = builder::long(b"Mdpn", clrt[i]);
            g.trns[i][0] = builder::untf(b"Opct", b"#Prc", SEG_ALPHA[i] * 100.0);
            g.trns[i][2] = builder::long(b"Mdpn", trns[i]);
        }
        g.grdn[2] = builder::doub(b"Intr", 0.0);
        g.top[1] = builder::untf(b"Angl", b"#Ang", 0.0);
        g.top[3] = builder::untf(b"Scl ", b"#Prc", 100.0);
    });
    let mut base = fixture8::Record::raster(
        [0, 0, H as i32, SEG_W as i32],
        &vec![200; (SEG_W * H * 4) as usize],
        false,
    );
    base.name = "Base";
    let records = vec![base, empty8("Seg", b"GdFl", &body)];
    let path = dir.path().join(name);
    std::fs::write(&path, fixture8::fixture(&records, [SEG_W, H], false)).unwrap();
    let opened = open8(&path);
    let d = parse(opened.layers[1].meta.as_deref().expect("a description"));
    let stops = d["gradient"]["stops"].clone();
    (opened, stops)
}

/// A deliberate re-read change: a `Grdn` an earlier build wrote put
/// stop `i`'s OWN midpoint on Photoshop stop `i` (the four distinct `Mdpn`
/// of the segment test, the last 50), and it now reimports with every
/// midpoint one stop earlier, as Photoshop reads that file.
#[test]
fn an_earlier_builds_grdn_reimports_its_midpoints_one_stop_earlier() {
    let dir = TempDir::new().unwrap();
    let (opened, read) = photoshop_shaped(&dir, "earlier_build.psd", SEG_MDPN, SEG_TMDPN);
    for i in 0..3 {
        let (m, t) = (SEG_MDPN[i + 1], SEG_TMDPN[i + 1]);
        let want = |v: i32| (v != 50).then(|| json!(f64::from(v) / 100.0));
        assert_eq!(
            read[i].get("midpoint").cloned(),
            want(m),
            "stop {i}: {read}"
        );
        assert_eq!(
            read[i].get("opacity_midpoint").cloned(),
            want(t),
            "stop {i}: {read}"
        );
    }
    assert!(read[3].get("midpoint").is_none(), "{read}");
    // The pixels follow the shifted reading: segment i at stop i + 1's.
    let shifted = |mdpn: [i32; 4]| {
        let mut out = [0.5; 4];
        for i in 0..3 {
            out[i] = f64::from(mdpn[i + 1]) / 100.0;
        }
        out
    };
    let positions = SEG_LCTN.map(|l| f64::from(l) / 4096.0);
    let pixels = opened.layers[1].pixels.to_contiguous().unwrap();
    let grey: Vec<(f64, f64)> = (0..4)
        .map(|i| (positions[i], f64::from(SEG_GREY[i]) / 255.0))
        .collect();
    let mids = shifted(SEG_MDPN);
    for x in 0..SEG_W {
        let t = f64::from(x) / f64::from(SEG_W);
        let got = f64::from(pixels.get_pixel(x, 0)[0]);
        let want = seg_want(&grey, &mids, t);
        assert!((got - want).abs() <= 1.0, "x {x}: {got} against {want}");
    }
}
