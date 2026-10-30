//! Layered CMYK PSD, PSB and TIFF import (#547 phase 3), through the host's
//! route (`dr_psd_import_report`, then `dr_precision_doc_open_psd` mode 0),
//! against files built byte by byte by `common/cmyk_psd_fixture.rs`, an
//! independent writer of Adobe's framing: the ink (`max − stored`, 8-bit
//! codes ×257), transparency, offsets (off-canvas kept), masks (enabled,
//! disabled, outside default), the tree (Normal and Pass Through groups),
//! modes, clipping, Fill, locks and labels exactly; the Levels, Curves and
//! Hue/Saturation records' parameters against values computed here from the
//! bodies; a carried Selective Color and knockout, a discarded Blend If
//! (#547, the styles batch) and a declined mapped curve by name; the
//! refusals (a missing ink channel; an RGB record's channel id 3); and the
//! colour-4 report by name.

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use doctored_core::cmyk_image::CmykSample;
use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_psd::{dr_precision_doc_open_psd, dr_psd_import_report, dr_tiff_has_layers};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_psd_fixture::{ink_channels, layered_tiff, plane, psd, Comp, PsdSpec, Rec};
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const W: u32 = 12;
const H: u32 = 8;

pub fn report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value =
        serde_json::from_str(unsafe { std::ffi::CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

pub fn open_psd(path: &Path, mode: i32) -> Result<*mut PrecisionDocument, String> {
    let fingerprint = report(path)["source_fingerprint"]
        .as_str()
        .unwrap()
        .to_string();
    let fingerprint = CString::new(fingerprint).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_psd(
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

fn free(doc: *mut PrecisionDocument) {
    unsafe { doctored_core::ffi_precision::dr_precision_doc_free(doc) };
}

/// A deterministic 8-bit-representable CMYKA16 rectangle: every code ×257,
/// alpha opaque unless `alpha`.
fn pixels(w: u32, h: u32, seed: u32, alpha: bool) -> Vec<CmykSample> {
    (0..w * h)
        .map(|i| {
            let j = i.wrapping_mul(2_654_435_761).wrapping_add(seed * 977);
            let code = |shift: u32| u16::try_from((j >> shift) & 0xFF).unwrap() * 257;
            let a = if alpha {
                u16::try_from((i * 37 + seed) % 256).unwrap() * 257
            } else {
                65535
            };
            [code(0), code(8), code(16), code(24), a]
        })
        .collect()
}

fn rect(top: i32, left: i32, h: u32, w: u32) -> [i32; 4] {
    [top, left, top + h as i32, left + w as i32]
}

/// A record's raster channels at `depth`.
fn raster(
    name: &str,
    bounds: [i32; 4],
    samples: &[CmykSample],
    alpha: bool,
    depth: u16,
    comp: Comp,
    large: bool,
) -> Rec {
    let width = (bounds[3] - bounds[1]) as usize;
    Rec::raster(
        name,
        bounds,
        ink_channels(samples, width, depth, alpha, comp, large),
    )
}

/// A mask plane of `w × h` coverage codes at `depth` (8-bit values given as
/// codes; 16-bit ×257).
fn mask_plane(w: u32, h: u32, depth: u16, comp: Comp, large: bool) -> (Vec<u16>, Vec<u8>) {
    let codes: Vec<u16> = (0..w * h)
        .map(|i| u16::try_from((i * 41 + 7) % 256).unwrap())
        .collect();
    let stored: Vec<u16> = if depth == 8 {
        codes.clone()
    } else {
        codes.iter().map(|c| c * 257).collect()
    };
    let expected = codes.iter().map(|c| c * 257).collect();
    (
        expected,
        plane(&stored, w as usize, h as usize, depth, comp, large),
    )
}

/// The `levl` body (version 2, 29 records) with records 0, 1 (C) and 4 (K).
fn levl() -> Vec<u8> {
    let mut records = vec![[0u16, 255, 0, 255, 100]; 29];
    records[0] = [10, 245, 0, 255, 120];
    records[1] = [0, 230, 10, 255, 100];
    records[4] = [20, 255, 0, 240, 90];
    let mut out = 2u16.to_be_bytes().to_vec();
    for record in records {
        for field in record {
            out.extend(field.to_be_bytes());
        }
    }
    out
}

/// A point `curv` body: bits 0 (composite), 1 (C), 4 (K), points stored
/// `(output, input)` in the `L` domain, then a zero pad byte.
fn curv() -> Vec<u8> {
    let mut out = vec![0];
    out.extend(1u16.to_be_bytes());
    out.extend(0x13u32.to_be_bytes());
    for points in [
        [(0u16, 0u16), (96, 128), (255, 255)],
        [(0, 0), (128, 100), (255, 255)],
        [(0, 0), (64, 80), (255, 255)],
    ] {
        out.extend(3u16.to_be_bytes());
        for (input, output) in points {
            out.extend(output.to_be_bytes());
            out.extend(input.to_be_bytes());
        }
    }
    out.push(0);
    out
}

/// A mapped `curv` body: is_map 1, composite only.
fn curv_map() -> Vec<u8> {
    let mut out = vec![1];
    out.extend(1u16.to_be_bytes());
    out.extend(1u32.to_be_bytes());
    out.extend((0..=255u8).rev());
    out.push(0);
    out
}

/// Photoshop's default `hue2` band ranges, the Reds band carrying hue −15,
/// saturation +20, the master hue +20 and saturation +10.
const RANGES: [[i16; 4]; 6] = [
    [315, 345, 15, 45],
    [15, 45, 75, 105],
    [75, 105, 135, 165],
    [135, 165, 195, 225],
    [195, 225, 255, 285],
    [255, 285, 315, 345],
];

fn hue2() -> Vec<u8> {
    let mut out = 2u16.to_be_bytes().to_vec();
    out.extend([0, 0]);
    for v in [0i16, 25, 0, 20, 10, 0] {
        out.extend(v.to_be_bytes());
    }
    for (band, range) in RANGES.iter().enumerate() {
        for v in range {
            out.extend(v.to_be_bytes());
        }
        let (hue, sat) = if band == 0 { (-15i16, 20i16) } else { (0, 0) };
        for v in [hue, sat, 0] {
            out.extend(v.to_be_bytes());
        }
    }
    out
}

/// What the structured file holds, for the assertions.
struct Built {
    bytes: Vec<u8>,
    base: Vec<CmykSample>,
    ramp: Vec<CmykSample>,
    ramp_mask: Vec<u16>,
    off: Vec<CmykSample>,
    child_a: Vec<CmykSample>,
    child_b: Vec<CmykSample>,
    group_mask: Vec<u16>,
    pass_child: Vec<CmykSample>,
}

/// The structured CMYK file at `depth` (module doc): every construct the
/// importer carries, plus a declined mapped curve.
fn structured(depth: u16, large: bool, lr16: bool, comp: Comp, icc: Option<&[u8]>) -> Built {
    let records = records_for(depth, comp, large);
    let max: u16 = if depth == 8 { 255 } else { 65535 };
    let composite = (0..4).map(|_| vec![max; (W * H) as usize]).collect();
    let bytes = psd(
        &PsdSpec {
            canvas: (W, H),
            depth,
            icc,
            composite,
            merged_alpha: false,
            large,
            lr16,
            resources: Vec::new(),
        },
        &records,
    );
    Built {
        bytes,
        base: pixels(W, H, 1, false),
        ramp: pixels(11, 6, 2, true),
        ramp_mask: mask_plane(9, 5, depth, comp, large).0,
        off: pixels(4, 3, 3, true),
        child_a: pixels(6, 4, 4, true),
        child_b: pixels(6, 4, 5, true),
        group_mask: mask_plane(W, H, depth, comp, large).0,
        pass_child: pixels(12, 2, 6, true),
    }
}

fn names(layers: &[PrecisionDocumentLayer]) -> Vec<&str> {
    layers.iter().map(PrecisionDocumentLayer::name).collect()
}

fn samples(layer: &PrecisionDocumentLayer) -> Vec<CmykSample> {
    layer
        .cmyk_source()
        .expect("a CMYK raster")
        .to_contiguous()
        .unwrap()
}

fn param(value: &Value, path: &[&str]) -> f64 {
    let mut at = value;
    for key in path {
        at = &at[*key];
    }
    at.as_f64().unwrap_or_else(|| panic!("{path:?} in {value}"))
}

fn close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() < 1e-9, "{what}: {a} vs {b}");
}

fn assert_tree(doc: *const PrecisionDocument, built: &Built) {
    let doc = unsafe { &*doc };
    let roots = doc.root_layers();
    assert_eq!(
        names(roots),
        [
            "Base",
            "Ramp",
            "Off",
            "Group N",
            "Pass",
            "Levels",
            "Curves",
            "HueSat",
            "Selective",
            "Knock"
        ]
    );
    let [base, ramp, off, group, pass, levels, curves, hue, selective, knock] = roots else {
        unreachable!()
    };
    assert_eq!(samples(base), built.base);
    assert_eq!(base.offset(), (0, 0));
    assert_eq!(samples(ramp), built.ramp);
    assert_eq!(ramp.offset(), (-2, -1), "the off-canvas offset is kept");
    assert_eq!(ramp.blend_mode(), BlendMode::Multiply);
    let mask = ramp.mask().expect("Ramp's mask");
    assert!(mask.enabled());
    assert_eq!(mask.properties().offset, (1, 1));
    assert_eq!(mask.properties().outside_coverage, 0);
    assert_eq!(mask.plane().samples(), built.ramp_mask);
    assert_eq!(samples(off), built.off);
    assert!(!off.visible());
    let mask = off.mask().expect("Off's mask");
    assert!(!mask.enabled());
    assert_eq!(mask.properties().outside_coverage, 255);
    assert!(!group.is_pass_through());
    assert_eq!(group.blend_mode(), BlendMode::Normal);
    assert_eq!(group.opacity(), 200.0 / 255.0);
    assert_eq!(group.label(), 2);
    assert!(group.open());
    assert_eq!(
        group.mask().expect("the group's mask").plane().samples(),
        built.group_mask
    );
    let children = group.children().unwrap();
    assert_eq!(names(children), ["Child A", "Child B"]);
    assert_eq!(samples(&children[0]), built.child_a);
    assert_eq!(children[0].blend_mode(), BlendMode::Screen);
    assert_eq!(children[0].opacity(), 128.0 / 255.0);
    assert_eq!(children[0].label(), 3);
    assert_eq!(children[0].locks(), 1, "the transparency lock");
    assert_eq!(samples(&children[1]), built.child_b);
    assert!(children[1].clipped());
    assert_eq!(children[1].blend_mode(), BlendMode::Overlay);
    assert_eq!(
        children[1].style().map(|s| s.fill_opacity),
        Some(102.0 / 255.0)
    );
    assert!(pass.is_pass_through());
    assert!(!pass.open(), "a closed folder");
    assert_eq!(samples(&pass.children().unwrap()[0]), built.pass_child);
    assert_eq!(pass.children().unwrap()[0].blend_mode(), BlendMode::Darken);
    assert_eq!(levels.opacity(), 230.0 / 255.0);
    assert!(
        levels.mask().is_none(),
        "Photoshop's empty no-mask body is no mask"
    );
    // An adjustment record carries its Fill and knockout.
    let carried = curves.style().expect("the adjustment's style carries");
    assert_eq!(carried.fill_opacity, 102.0 / 255.0);
    assert_eq!(carried.knockout, doctored_core::style::Knockout::Shallow);
    assert_eq!(hue.blend_mode(), BlendMode::Color);
    // Knockout CARRIES since the styles batch (#547), Blend If since the
    // Blend If batch.
    let knocked = knock.style().expect("the knockout carries");
    assert_eq!(knocked.knockout, doctored_core::style::Knockout::Shallow);
    let ranges = knock.blend_if().expect("the Blend If carries");
    assert_eq!(ranges.channel, doctored_core::style::BlendIfChannel::Gray);
    assert_eq!(ranges.this_layer, [0, 0, 200, 255]);
    assert_eq!(ranges.underlying, [0, 0, 255, 255]);
    assert!(selective.blend_mode() == BlendMode::Normal);
    // The adjustment records' parameters, from the bodies above.
    let description = |name: &str| -> Value {
        let index = (0..doc.layer_count())
            .find(|&i| doc.layer(i).unwrap().name() == name)
            .unwrap();
        serde_json::from_str(doc.adjustment_description(index).expect("an adjustment")).unwrap()
    };
    let l = description("Levels");
    assert_eq!(l["op"], "levels");
    assert_eq!(l["evaluation"], "precision_v1");
    let p = &l["params"];
    close(param(p, &["black"]), 10.0 / 255.0, "levels black");
    close(param(p, &["white"]), 245.0 / 255.0, "levels white");
    close(param(p, &["gamma"]), 1.2, "levels gamma");
    assert!(p.get("output_black").is_none());
    close(param(p, &["c", "white"]), 230.0 / 255.0, "c white");
    close(
        param(p, &["c", "output_black"]),
        10.0 / 255.0,
        "c output black",
    );
    close(param(p, &["k", "black"]), 20.0 / 255.0, "k black");
    close(
        param(p, &["k", "output_white"]),
        240.0 / 255.0,
        "k output white",
    );
    close(param(p, &["k", "gamma"]), 0.9, "k gamma");
    assert!(
        p.get("m").is_none() && p.get("y").is_none(),
        "identity inks are no keys"
    );
    let c = description("Curves");
    assert_eq!(c["op"], "curves");
    // Each stored point (in, out) reflected to (255 − in, 255 − out), ascending.
    assert_eq!(
        c["params"]["cmyk"],
        serde_json::json!([[0, 0], [159, 127], [255, 255]])
    );
    assert_eq!(
        c["params"]["c"],
        serde_json::json!([[0, 0], [127, 155], [255, 255]])
    );
    assert_eq!(
        c["params"]["k"],
        serde_json::json!([[0, 0], [191, 175], [255, 255]])
    );
    let h = description("HueSat");
    assert_eq!(h["op"], "hue_saturation");
    close(param(&h["params"], &["hue"]), 20.0, "hue");
    close(param(&h["params"], &["saturation"]), 0.1, "saturation");
    // Reds: inner = arc(345, 15) / 2 = 15, centre (345 + 15) mod 360 = 0,
    // falloff (arc(315, 345) + arc(15, 45)) / 2 = 30.
    let reds = &h["params"]["bands"]["reds"];
    close(param(reds, &["center"]), 0.0, "reds centre");
    close(param(reds, &["inner"]), 15.0, "reds inner");
    close(param(reds, &["falloff"]), 30.0, "reds falloff");
    close(param(reds, &["hue"]), -15.0, "reds hue");
    close(param(reds, &["saturation"]), 0.2, "reds saturation");
}

fn has(list: &Value, text: &str) -> bool {
    list.as_array()
        .unwrap()
        .iter()
        .any(|v| v.as_str().unwrap().contains(text))
}

fn assert_report(r: &Value, depth: u16) {
    assert_eq!(r["import_precision"], "cmyk", "{depth}-bit");
    assert_eq!(r["color_mode"], "CMYK");
    assert_eq!(r["layered_available"], true, "{}", r["layered_refusal"]);
    assert_eq!(r["stored_composite_available"], true);
    assert_eq!(r["profile"], "Press Test");
    assert_eq!(r["layer_count"], 12);
    assert_eq!(r["group_count"], 2);
    let general = r["general_limitations"].as_array().unwrap();
    assert!(general[0]
        .as_str()
        .unwrap()
        .starts_with("A CMYK file opens as a CMYK document"));
    assert!(general
        .iter()
        .all(|g| !g.as_str().unwrap().contains("Embedded RGB ICC")));
    let (losses, observations) = (&r["verified_losses"], &r["observations"]);
    assert!(has(observations, "This file opens as a CMYK document, keeping its own ink and embedded CMYK profile, with either choice."));
    assert!(has(observations, "(“Selective”) is a Photoshop Selective Color adjustment layer (selc); layered import carries it as a Doctored selective_color adjustment layer"));
    assert!(has(losses, "(“MapCurve”) is a Photoshop Curves adjustment layer (curv) with no pixels whose curve is drawn (mapped), which has no CMYK form in Doctored"));
    assert!(has(observations, "(“Knock”) has Photoshop layer effects (knockout (shallow) and Blend If ranges on gray); layered import carries them as a Doctored layer style."));
    assert!(!has(losses, "Blend If"), "{losses}");
    // A CMYK adjustment record carries its style.
    assert!(has(observations, "(“Curves”) has Photoshop layer effects (fill opacity 40 %, knockout (shallow)); layered import carries them as a Doctored layer style."));
    assert!(!has(losses, "(“Curves”)"), "{losses}");
    assert!(has(observations, "(“Levels”) is a Photoshop Levels adjustment layer (levl); layered import carries it as a Doctored levels adjustment layer"));
    for list in [losses, observations] {
        for text in ["sRGB", "white", "16-bit document"] {
            assert!(!has(list, text), "{text} in {list}");
        }
    }
    let pixels: u64 = [W * H, 11 * 6, 4 * 3, 6 * 4, 6 * 4, 12 * 2, 2 * 2].iter().map(|p| u64::from(*p)).sum::<u64>()
        // Levels, Curves, HueSat and Selective are no pixels; MapCurve is
        // omitted.
        ;
    let plan = r["layered_plan_bytes"].as_u64().expect("the layered plan");
    assert!(plan >= pixels * 10 + u64::from(W * H) * 27, "plan {plan}");
}

#[test]
fn a_layered_cmyk_psd_imports_every_construct_at_both_depths() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    for (depth, comp) in [
        (8u16, Comp::Rle),
        (16, Comp::ZipPredicted),
        (16, Comp::Raw),
        (8, Comp::Zip),
    ] {
        let built = structured(depth, false, depth == 16, comp, Some(&profile));
        let path = dir.path().join(format!("structured{depth}-{comp:?}.psd"));
        std::fs::write(&path, &built.bytes).unwrap();
        let r = report(&path);
        assert_report(&r, depth);
        let doc = open_psd(&path, 0).unwrap_or_else(|e| panic!("{depth} {comp:?}: {e}"));
        assert!(unsafe { &*doc }.is_cmyk());
        assert_eq!(
            unsafe { &*doc }.cmyk_policy().unwrap().depth().bits(),
            u32::from(depth)
        );
        assert_tree(doc, &built);
        free(doc);
    }
}

#[test]
fn a_layered_cmyk_psb_imports_as_its_psd_twin() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    for depth in [8u16, 16] {
        let built = structured(depth, true, depth == 16, Comp::Rle, Some(&profile));
        let path = dir.path().join(format!("structured{depth}.psb"));
        std::fs::write(&path, &built.bytes).unwrap();
        let r = report(&path);
        assert_eq!(r["format"], "PSB");
        assert_report(&r, depth);
        let doc = open_psd(&path, 0).unwrap();
        assert_tree(doc, &built);
        free(doc);
    }
}

#[test]
fn a_layered_cmyk_tiff_imports_its_records_and_has_no_stored_composite() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    for depth in [8u16, 16] {
        let built = structured(depth, false, false, Comp::Rle, Some(&profile));
        // The same records in a big-endian CMYK TIFF's block.
        let strip = vec![[0u16, 0, 0, 0, if depth == 8 { 255 } else { 65535 }]; (W * H) as usize];
        let records = records_for(depth, Comp::Rle, false);
        let bytes = layered_tiff((W, H), depth, Some(&profile), &strip, &records);
        let path = dir.path().join(format!("structured{depth}.tif"));
        std::fs::write(&path, &bytes).unwrap();
        assert!(unsafe { dr_tiff_has_layers(cpath(&path).as_ptr()) });
        let r = report(&path);
        assert_eq!(r["format"], "TIFF");
        assert_eq!(r["import_precision"], "cmyk");
        assert_eq!(r["layered_available"], true, "{}", r["layered_refusal"]);
        assert_eq!(r["stored_composite_available"], false);
        assert_eq!(
            r["stored_composite_refusal"],
            "a TIFF's flat image opens through the ordinary TIFF open"
        );
        assert_eq!(r["profile"], "Press Test", "the IFD's ICC");
        assert!(
            !has(&r["observations"], "merged transparency"),
            "a TIFF has no saved composite to describe"
        );
        let doc = open_psd(&path, 0).unwrap();
        assert_tree(doc, &built);
        free(doc);
        assert_eq!(
            open_psd(&path, 1).unwrap_err(),
            format!(
                "PSD {}: a TIFF's flat image opens through the ordinary TIFF open",
                path.display()
            )
        );
    }
}

/// The structured file's records, file order (bottom first).
fn records_for(depth: u16, comp: Comp, large: bool) -> Vec<Rec> {
    let ramp = pixels(11, 6, 2, true);
    let off = pixels(4, 3, 3, true);
    let child_a = pixels(6, 4, 4, true);
    let child_b = pixels(6, 4, 5, true);
    let pass_child = pixels(12, 2, 6, true);
    let (_, ramp_plane) = mask_plane(9, 5, depth, comp, large);
    let (_, off_plane) = mask_plane(2, 2, depth, comp, large);
    let (_, group_plane) = mask_plane(W, H, depth, comp, large);
    let r = |name, bounds, samples: &[CmykSample], alpha| {
        raster(name, bounds, samples, alpha, depth, comp, large)
    };
    let mut ramp_rec =
        r("Ramp", rect(-1, -2, 6, 11), &ramp, true).mask(rect(1, 1, 5, 9), 0, true, ramp_plane);
    ramp_rec.blend = *b"mul ";
    let mut off_rec =
        r("Off", rect(5, 8, 3, 4), &off, true).mask(rect(5, 8, 2, 2), 255, false, off_plane);
    off_rec.flags |= 2;
    let mut child_a_rec = r("Child A", rect(0, 0, 4, 6), &child_a, true)
        .with_block(b"lclr", &[0, 3, 0, 0, 0, 0, 0, 0])
        .with_block(b"lspf", &[0, 0, 0, 1]);
    child_a_rec.blend = *b"scrn";
    child_a_rec.opacity = 128;
    let mut child_b_rec =
        r("Child B", rect(2, 2, 4, 6), &child_b, true).with_block(b"iOpa", &[102, 0, 0, 0]);
    child_b_rec.blend = *b"over";
    child_b_rec.clipping = 1;
    let mut group = Rec::folder("Group N", 1, *b"norm")
        .mask(rect(0, 0, H, W), 255, true, group_plane)
        .with_block(b"lclr", &[0, 2, 0, 0, 0, 0, 0, 0]);
    group.opacity = 200;
    let mut pass_child_rec = r("Pass child", rect(6, 0, 2, 12), &pass_child, true);
    pass_child_rec.blend = *b"dark";
    let mut levels = Rec::empty("Levels").with_block(b"levl", &levl());
    levels.mask_body = crate::cmyk_psd_fixture::mask_body([0; 4], 255, 0);
    levels.channels.push((-2, vec![0, 0]));
    levels.opacity = 230;
    // A styled adjustment record: its Fill and knockout carry.
    let curves = Rec::empty("Curves")
        .with_block(b"curv", &curv())
        .with_block(b"iOpa", &[102, 0, 0, 0])
        .with_block(b"knko", &[1, 0, 0, 0]);
    let mut hue = Rec::empty("HueSat").with_block(b"hue2", &hue2());
    hue.blend = *b"colr";
    // A Selective Color record (version 1, relative, Reds 10/−10/5/0), which a
    // CMYK document carries since the styles batch.
    let mut selc = vec![0u8; 84];
    selc[1] = 1;
    for (i, v) in [10i16, -10, 5, 0].into_iter().enumerate() {
        selc[12 + 2 * i..14 + 2 * i].copy_from_slice(&v.to_be_bytes());
    }
    let selective = Rec::empty("Selective").with_block(b"selc", &selc);
    let mapped = Rec::empty("MapCurve").with_block(b"curv", &curv_map());
    let mut knock = r("Knock", rect(0, 10, 2, 2), &pixels(2, 2, 9, true), true)
        .with_block(b"knko", &[1, 0, 0, 0]);
    knock.ranges = [[0u8, 0, 200, 255], [0, 0, 255, 255]].concat();
    knock
        .ranges
        .extend(std::iter::repeat_n([0u8, 0, 255, 255], 8).flatten());
    vec![
        r("Base", rect(0, 0, H, W), &pixels(W, H, 1, false), false),
        ramp_rec,
        off_rec,
        Rec::folder("</Layer group>", 3, *b"norm"),
        child_a_rec,
        child_b_rec,
        group,
        Rec::folder("</Layer group>", 3, *b"norm"),
        pass_child_rec,
        Rec::folder("Pass", 2, *b"pass"),
        levels,
        curves,
        hue,
        selective,
        mapped,
        knock,
    ]
}

#[test]
fn a_missing_ink_channel_refuses_the_layers_by_name() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    let samples = pixels(4, 4, 1, true);
    let mut channels = ink_channels(&samples, 4, 8, true, Comp::Raw, false);
    channels.retain(|(id, _)| *id != 3);
    let records = vec![Rec::raster("No black", rect(0, 0, 4, 4), channels)];
    let bytes = psd(
        &PsdSpec {
            canvas: (4, 4),
            depth: 8,
            icc: Some(&profile),
            composite: (0..4).map(|_| vec![255; 16]).collect(),
            merged_alpha: false,
            large: false,
            lr16: false,
            resources: Vec::new(),
        },
        &records,
    );
    let path = dir.path().join("noblack.psd");
    std::fs::write(&path, &bytes).unwrap();
    let r = report(&path);
    assert_eq!(r["layered_available"], false);
    assert_eq!(
        r["layered_refusal"],
        "PSD raster is missing a required color channel"
    );
    assert_eq!(
        r["stored_composite_available"], true,
        "the composite still opens"
    );
    assert!(open_psd(&path, 0)
        .unwrap_err()
        .contains("PSD raster is missing a required color channel"));
}

#[test]
fn an_rgb_record_with_channel_id_three_still_refuses() {
    let dir = TempDir::new().unwrap();
    let samples = pixels(4, 4, 1, true);
    // An RGB record with a stray id 3: the CMYK planes, read as RGB.
    let records = vec![Rec::raster(
        "Stray",
        rect(0, 0, 4, 4),
        ink_channels(&samples, 4, 8, true, Comp::Raw, false),
    )];
    let mut bytes = psd(
        &PsdSpec {
            canvas: (4, 4),
            depth: 8,
            icc: None,
            composite: (0..3).map(|_| vec![255; 16]).collect(),
            merged_alpha: false,
            large: false,
            lr16: false,
            resources: Vec::new(),
        },
        &records,
    );
    bytes[24..26].copy_from_slice(&3u16.to_be_bytes());
    let path = dir.path().join("rgb3.psd");
    std::fs::write(&path, &bytes).unwrap();
    let r = report(&path);
    assert_eq!(r["import_precision"], "rgba8");
    assert_eq!(r["layered_available"], false);
    assert_eq!(
        r["layered_refusal"],
        "PSD layer has an unknown or duplicate channel ID"
    );
}

#[test]
fn merged_alpha_and_saved_channels_are_named_and_untagged_refuses_without_a_working_profile() {
    let dir = TempDir::new().unwrap();
    let profile = press_profile("Press Test");
    let samples = pixels(4, 4, 1, true);
    let records = vec![Rec::raster(
        "Only",
        rect(0, 0, 4, 4),
        ink_channels(&samples, 4, 8, true, Comp::Rle, false),
    )];
    let spec = |icc| PsdSpec {
        canvas: (4, 4),
        depth: 8,
        icc,
        // Four inks, the merged alpha and one more saved channel.
        composite: (0..6).map(|_| vec![255; 16]).collect(),
        merged_alpha: true,
        large: false,
        lr16: false,
        resources: Vec::new(),
    };
    let path = dir.path().join("alpha.psd");
    std::fs::write(&path, psd(&spec(Some(&profile)), &records)).unwrap();
    let r = report(&path);
    assert!(has(&r["observations"], "This CMYK PSD's saved composite has merged transparency, which is not read: the composite opens as stored, matted on white."));
    assert!(has(&r["verified_losses"], "1 additional saved channels (spot or alpha) detected in the header; the CMYK composite does not retain them."));
    let doc = open_psd(&path, 0).unwrap();
    assert_eq!(samples, unsafe { &*doc }.cmyk_layer_samples(0).unwrap());
    free(doc);
    // No embedded profile and (in this binary) no working CMYK profile: both
    // choices refuse by name.
    let untagged = dir.path().join("untagged.psd");
    std::fs::write(&untagged, psd(&spec(None), &records)).unwrap();
    let r = report(&untagged);
    let refusal = "the CMYK PSD has no embedded colour profile and no working CMYK profile is set";
    assert_eq!(r["layered_refusal"], refusal);
    assert_eq!(r["stored_composite_refusal"], refusal);
}
