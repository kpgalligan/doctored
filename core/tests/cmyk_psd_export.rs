//! Layered CMYK PSD, PSB and TIFF export (#1 phase 3): a structured CMYK
//! document (`common/cmyk_structure_fixture.rs`: Normal and Pass Through
//! groups with masks, opacity and modes, a clipped record, Fill, a hidden
//! layer, a label and a lock, Levels, Curves and Hue/Saturation records) at
//! 16 and 8 bits, written in every container and both modes through
//! `dr_precision_doc_save_psd` with a NULL projection, then read by the
//! independent reader (`common/cmyk_psd_fixture.rs`): the records, channel
//! ids, `max − ink` planes, masks, `lsct`, `iOpa`, `lspf`, `lclr`, the
//! adjustment bodies byte-exact against bodies written out here, the
//! five-plane composite against 4a's f64 oracle within 1 code, then reopened
//! through Import Layers: structure and ink exact. The export report's CMYK
//! fields, and the RGB document's refusal of a NULL projection.

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk_colour::dr_precision_doc_cmyk_profile_edit;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_psd::{dr_precision_doc_psd_export_report, dr_precision_doc_save_psd};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_psd_fixture::{read, ReadFile, ReadRecord};
use crate::cmyk_psd_import::open_psd;
use crate::cmyk_structure_fixture::{
    cmyk_layers, composite, free, oracle, ramp, then, varied, with_fill, with_levels, with_mask,
    with_mode, with_opacity, RgbLayer, LIMIT,
};
use crate::common::{cpath, take_err_string};

const W: u32 = 9;
const H: u32 = 7;
const PSD: i32 = 0;
const PSB: i32 = 1;
const TIFF: i32 = 2;
const LAYERS: i32 = 0;
const FLATTENED: i32 = 1;

const CURVES: &str = r#"{"type":"adjust","evaluation":"precision_v1","op":"curves","params":{"cmyk":[[0,0],[100,140],[255,255]],"k":[[0,0],[128,100],[255,255]]}}"#;
const HUE: &str = r#"{"type":"adjust","evaluation":"precision_v1","op":"hue_saturation","params":{"hue":15,"saturation":0.1,"lightness":-0.05}}"#;

fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

fn group(
    doc: *mut PrecisionDocument,
    names: &[&str],
    name: &str,
    pass: bool,
) -> *mut PrecisionDocument {
    let layers: Vec<String> = names.iter().map(|n| index(doc, n).to_string()).collect();
    then(
        doc,
        &format!(
            r#"{{"op":"group_layers","layers":[{}],"name":"{name}","pass_through":{pass}}}"#,
            layers.join(",")
        ),
        &[],
    )
}

fn edit(doc: *mut PrecisionDocument, name: &str, body: &str) -> *mut PrecisionDocument {
    let i = index(doc, name);
    then(doc, &body.replace("LAYER", &i.to_string()), &[])
}

fn add(
    doc: *mut PrecisionDocument,
    name: &str,
    description: &str,
    opacity: f32,
) -> *mut PrecisionDocument {
    let name = CString::new(name).unwrap();
    let text = CString::new(description).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            name.as_ptr(),
            text.as_ptr(),
            opacity,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "add: {}", take_err_string(err));
    free(doc);
    out
}

/// The structured document (module doc) at 16 bits; `toned` adds the Curves
/// and Hue/Saturation records, which 4a's oracle (Levels only) does not
/// evaluate.
fn structured(toned: bool) -> *mut PrecisionDocument {
    let full = |name, seed| RgbLayer {
        name,
        w: W,
        h: H,
        offset: (0, 0),
        pixels: varied(W, H, seed),
    };
    let mut bottom = varied(W, H, 1);
    for p in &mut bottom {
        p[3] = 65535;
    }
    let mut doc = cmyk_layers(
        W,
        H,
        vec![
            RgbLayer {
                pixels: bottom,
                ..full("Bottom", 1)
            },
            full("A", 2),
            RgbLayer {
                name: "B",
                w: 6,
                h: 5,
                offset: (2, 1),
                pixels: varied(6, 5, 3),
            },
            full("C", 4),
            full("D", 5),
            full("E", 6),
            full("Hidden", 7),
        ],
    );
    doc = group(doc, &["A", "B"], "Normal", false);
    doc = with_mask(doc, index(doc, "Normal"), &ramp(W, H, 3));
    doc = with_opacity(doc, index(doc, "Normal"), 0.7);
    doc = with_mode(doc, index(doc, "Normal"), BlendMode::Screen.to_c());
    doc = with_mode(doc, index(doc, "B"), BlendMode::Overlay.to_c());
    doc = group(doc, &["C", "D"], "Pass", true);
    doc = with_mask(doc, index(doc, "Pass"), &ramp(W, H, 7));
    doc = with_opacity(doc, index(doc, "Pass"), 0.6);
    doc = with_mode(doc, index(doc, "D"), BlendMode::Multiply.to_c());
    doc = edit(
        doc,
        "E",
        r#"{"op":"layer_clipped","layer":LAYER,"clipped":true}"#,
    );
    doc = with_fill(doc, index(doc, "C"), 0.4);
    doc = with_mask(doc, index(doc, "C"), &ramp(W, H, 11));
    doc = edit(doc, "A", r#"{"op":"layer_label","layer":LAYER,"label":3}"#);
    doc = edit(doc, "D", r#"{"op":"layer_lock","layer":LAYER,"locks":1}"#);
    doc = edit(
        doc,
        "Hidden",
        r#"{"op":"layer_visible","layer":LAYER,"visible":false}"#,
    );
    // The hidden layer in Difference: an unstyled legacy-Fill pixel record
    // (its `iOpa` at 255) that leaves the composite as it was.
    doc = with_mode(doc, index(doc, "Hidden"), BlendMode::Difference.to_c());
    doc = with_levels(doc, 0.8);
    doc = with_mode(doc, index(doc, "Levels"), BlendMode::Multiply.to_c());
    if !toned {
        return doc;
    }
    doc = add(doc, "Curves", CURVES, 1.0);
    doc = with_mask(doc, index(doc, "Curves"), &ramp(W, H, 13));
    add(doc, "Hue", HUE, 0.5)
}

fn eight(doc: *mut PrecisionDocument) -> *mut PrecisionDocument {
    let request = CString::new(r#"{"op":"depth","depth":8}"#).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_profile_edit(
            doc,
            ptr::null(),
            0,
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "depth 8: {}", take_err_string(err));
    free(doc);
    out
}

fn report(doc: *const PrecisionDocument, container: i32, mode: i32) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_psd_export_report(doc, container, mode, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value =
        serde_json::from_str(unsafe { std::ffi::CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn save(
    doc: *const PrecisionDocument,
    path: &Path,
    container: i32,
    mode: i32,
) -> Result<(), String> {
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_psd(
            doc,
            ptr::null(),
            cpath(path).as_ptr(),
            container,
            mode,
            LIMIT,
            0,
            std::ptr::null(),
            &mut err,
        )
    };
    if ok {
        Ok(())
    } else {
        Err(take_err_string(err))
    }
}

/// The `levl` body of `cmyk_structure_fixture::LEVELS` (output 0.1–0.8):
/// record 0 `[0, 255, round(25.5), round(204), 100]`, 28 identity records.
fn levels_body() -> Vec<u8> {
    let mut out = 2u16.to_be_bytes().to_vec();
    for record in 0..29 {
        let fields: [u16; 5] = if record == 0 {
            [0, 255, 26, 204, 100]
        } else {
            [0, 255, 0, 255, 100]
        };
        for f in fields {
            out.extend(f.to_be_bytes());
        }
    }
    out
}

/// [`CURVES`]' `curv` body: bits 0 and 4, each ink point `(i, o)` stored in
/// the `L` domain as `(255 − i, 255 − o)` ascending, written (output, input),
/// then the pad byte.
fn curves_body() -> Vec<u8> {
    let mut out = vec![0, 0, 1];
    out.extend(0x11u32.to_be_bytes());
    for points in [
        [(0u16, 0u16), (155, 115), (255, 255)],
        [(0, 0), (127, 155), (255, 255)],
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

/// The record's ink at the file's depth against the layer's own samples
/// (8-bit: `max − round(v / 257)`).
fn assert_raster(record: &ReadRecord, layer: &PrecisionDocumentLayer, depth: u16) {
    let image = layer.cmyk_source().unwrap();
    let (w, h) = image.dimensions();
    let (ox, oy) = layer.offset();
    assert_eq!(
        record.bounds,
        [oy, ox, oy + h as i32, ox + w as i32],
        "{}",
        layer.name()
    );
    let ids: Vec<i16> = record.channels.iter().map(|(id, _)| *id).collect();
    let mut want_ids = vec![-1, 0, 1, 2, 3];
    if layer.mask().is_some() {
        want_ids.push(-2);
    }
    assert_eq!(ids, want_ids, "{}", layer.name());
    let samples = image.to_contiguous().unwrap();
    let to_depth = |v: u16| {
        if depth == 8 {
            (u32::from(v) + 128) / 257
        } else {
            u32::from(v)
        }
    };
    let max: u32 = if depth == 8 { 255 } else { 65535 };
    for (i, s) in samples.iter().enumerate() {
        assert_eq!(
            u32::from(record.plane(-1).unwrap()[i]),
            to_depth(s[4]),
            "{} alpha {i}",
            layer.name()
        );
        for (c, ink) in s.iter().take(4).enumerate() {
            assert_eq!(
                u32::from(record.plane(c as i16).unwrap()[i]),
                max - to_depth(*ink),
                "{} ink {c} {i}",
                layer.name()
            );
        }
    }
}

fn assert_mask(record: &ReadRecord, layer: &PrecisionDocumentLayer, depth: u16) {
    let Some(mask) = layer.mask() else {
        assert!(record.mask.is_none(), "{}", layer.name());
        return;
    };
    let body = record.mask.as_ref().unwrap();
    let (mx, my) = mask.properties().offset;
    let (w, h) = mask.plane().dimensions();
    assert_eq!(body.bounds, [my, mx, my + h as i32, mx + w as i32]);
    assert_eq!(body.outside, mask.properties().outside_coverage);
    assert_eq!(body.flags & 2 == 0, mask.enabled());
    let want: Vec<u16> = mask
        .plane()
        .samples()
        .iter()
        .map(|v| {
            if depth == 8 {
                ((u32::from(*v) + 128) / 257) as u16
            } else {
                *v
            }
        })
        .collect();
    assert_eq!(
        record.plane(-2).unwrap(),
        want.as_slice(),
        "{} mask",
        layer.name()
    );
}

/// The file's records in FILE order against the document's tree (postorder
/// with each group's closing divider first).
fn assert_records(file: &ReadFile, doc: &PrecisionDocument) {
    let mut expected = Vec::new();
    fn walk<'a>(
        layers: &'a [PrecisionDocumentLayer],
        out: &mut Vec<Option<&'a PrecisionDocumentLayer>>,
    ) {
        for layer in layers {
            if let Some(children) = layer.children() {
                out.push(None);
                walk(children, out);
            }
            out.push(Some(layer));
        }
    }
    walk(doc.root_layers(), &mut expected);
    assert_eq!(file.records.len(), expected.len());
    for (record, layer) in file.records.iter().zip(expected) {
        let Some(layer) = layer else {
            assert_eq!(record.divider, Some(3), "a closing divider");
            assert_eq!(record.name, "</Layer group>");
            continue;
        };
        let name = layer.name();
        assert_eq!(record.name, name);
        assert_eq!(
            record.opacity,
            (layer.opacity() * 255.0).round() as u8,
            "{name}"
        );
        assert_eq!(record.clipping, u8::from(layer.clipped()), "{name}");
        assert_eq!(record.flags & 2 != 0, !layer.visible(), "{name}");
        // A pixel record in Color Burn, Color Dodge or Difference writes
        // `iOpa` at 255 too (#2: Photoshop reads one without it as a
        // legacy layer); the fixture holds no adjustment in those modes.
        let legacy = layer.children().is_none()
            && [
                BlendMode::ColorBurn,
                BlendMode::ColorDodge,
                BlendMode::Difference,
            ]
            .contains(&layer.blend_mode());
        assert_eq!(
            record.block(b"iOpa").map(|b| b[0]),
            layer
                .style()
                .map(|s| (s.fill_opacity * 255.0).round() as u8)
                .or(legacy.then_some(255)),
            "{name}: iOpa"
        );
        assert_eq!(
            record.block(b"lclr").map(|b| b[1]),
            (layer.label() != 0).then_some(layer.label()),
            "{name}"
        );
        assert_eq!(
            record.block(b"lspf").is_some(),
            layer.locks() != 0,
            "{name}: lspf"
        );
        assert_mask(record, layer, file.depth);
        if let Some(children) = layer.children() {
            let _ = children;
            assert_eq!(
                record.divider,
                Some(if layer.open() { 1 } else { 2 }),
                "{name}"
            );
            let key = if layer.is_pass_through() {
                *b"pass"
            } else {
                blend_key(layer.blend_mode())
            };
            assert_eq!(record.divider_blend, Some(key), "{name}");
        } else if layer.cmyk_source().is_some() {
            assert_eq!(record.blend, blend_key(layer.blend_mode()), "{name}");
            assert_raster(record, layer, file.depth);
        } else {
            assert_eq!(record.bounds, [0; 4], "{name}: pixel-less");
            assert_eq!(record.blend, blend_key(layer.blend_mode()), "{name}");
            match name {
                "Levels" => assert_eq!(record.block(b"levl").unwrap(), levels_body().as_slice()),
                "Curves" => assert_eq!(record.block(b"curv").unwrap(), curves_body().as_slice()),
                "Hue" => {
                    let body = record.block(b"hue2").unwrap();
                    assert_eq!(body.len(), 100);
                    // Version 2, no colorize, then the master hue, saturation
                    // and lightness after the colorize triple.
                    assert_eq!(&body[..4], &[0, 2, 0, 0]);
                    let at = |i: usize| i16::from_be_bytes([body[i], body[i + 1]]);
                    assert_eq!((at(10), at(12), at(14)), (15, 10, -5));
                }
                other => panic!("unexpected record {other}"),
            }
        }
    }
}

fn blend_key(mode: BlendMode) -> [u8; 4] {
    match mode {
        BlendMode::Normal => *b"norm",
        BlendMode::Multiply => *b"mul ",
        BlendMode::Screen => *b"scrn",
        BlendMode::Overlay => *b"over",
        BlendMode::Difference => *b"diff",
        other => panic!("no key in this test for {other:?}"),
    }
}

/// The written composite against `want(x, y)` (straight ink and alpha in
/// [0, 1]), flattened onto paper, in 8-bit codes within 1; a PSD stores the
/// inks inverted, a TIFF's strip straight.
fn assert_composite(file: &ReadFile, want: &dyn Fn(usize, usize) -> [f64; 5]) {
    let to8 = |v: u16| {
        if file.depth == 8 {
            f64::from(v)
        } else {
            f64::from(v) / 257.0
        }
    };
    let max = if file.depth == 8 { 255u16 } else { 65535 };
    assert_eq!(file.composite.len(), 5, "five composite planes");
    for y in 0..H as usize {
        for x in 0..W as usize {
            let want = want(x, y);
            let i = y * W as usize + x;
            let alpha = to8(file.composite[4][i]);
            assert!(
                (alpha - want[4] * 255.0).abs() <= 1.0,
                "alpha at ({x}, {y})"
            );
            for c in 0..4 {
                let stored = file.composite[c][i];
                let (got, expect) = if file.tiff {
                    // The strip's straight ink; the oracle's where it shows.
                    if want[4] < 1e-3 {
                        continue;
                    }
                    (to8(stored), want[c] * 255.0)
                } else {
                    (to8(max - stored), want[c] * want[4] * 255.0)
                };
                assert!(
                    (got - expect).abs() <= 1.0,
                    "plate {c} at ({x}, {y}): {got} vs {expect}"
                );
            }
        }
    }
}

fn byte(v: f32) -> u8 {
    (v * 255.0).round() as u8
}

/// Import Layers of the written file against the source tree: structure
/// and ink exact (opacity and Fill at the file's byte).
fn same_tree(a: &[PrecisionDocumentLayer], b: &[PrecisionDocumentLayer]) {
    assert_eq!(a.len(), b.len());
    for (la, lb) in a.iter().zip(b) {
        let what = la.name();
        assert_eq!(la.name(), lb.name());
        assert_eq!(byte(la.opacity()), byte(lb.opacity()), "{what}");
        assert_eq!(la.visible(), lb.visible(), "{what}");
        assert_eq!(la.blend_mode(), lb.blend_mode(), "{what}");
        assert_eq!(la.clipped(), lb.clipped(), "{what}");
        assert_eq!(la.is_pass_through(), lb.is_pass_through(), "{what}");
        assert_eq!(la.label(), lb.label(), "{what}");
        assert_eq!(la.locks(), lb.locks(), "{what}");
        assert_eq!(
            la.style().map(|s| byte(s.fill_opacity)),
            lb.style().map(|s| byte(s.fill_opacity)),
            "{what}"
        );
        assert_eq!(la.offset(), lb.offset(), "{what}");
        match (la.mask(), lb.mask()) {
            (None, None) => {}
            (Some(ma), Some(mb)) => {
                assert_eq!(ma.plane().samples(), mb.plane().samples(), "{what}");
                assert_eq!(ma.properties().offset, mb.properties().offset, "{what}");
                assert_eq!(ma.enabled(), mb.enabled(), "{what}");
            }
            _ => panic!("{what}: the mask did not round trip"),
        }
        match (la.cmyk_source(), lb.cmyk_source()) {
            (None, None) => {}
            (Some(ia), Some(ib)) => assert!(ia.same_samples(ib), "{what}: ink"),
            _ => panic!("{what}: the pixels did not round trip"),
        }
        match (la.children(), lb.children()) {
            (None, None) => {}
            (Some(ca), Some(cb)) => same_tree(ca, cb),
            _ => panic!("{what}: the group did not round trip"),
        }
    }
}

fn description(doc: &PrecisionDocument, name: &str) -> Value {
    let i = (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap();
    serde_json::from_str(doc.adjustment_description(i).unwrap()).unwrap()
}

#[test]
fn a_structured_cmyk_document_exports_every_container_and_reopens_exactly() {
    let dir = TempDir::new().unwrap();
    let sixteen = structured(true);
    let eight = eight(structured(true));
    for (doc, depth) in [(sixteen, 16u16), (eight, 8)] {
        for (container, ext) in [(PSD, "psd"), (PSB, "psb"), (TIFF, "tif")] {
            let r = report(doc, container, LAYERS);
            assert_eq!(r["depth"], "cmyk");
            assert_eq!(r["bit_depth"], depth);
            assert_eq!(r["available"], true, "{}", r["refusal"]);
            assert_eq!(r["layer_count"], 10);
            assert_eq!(r["group_count"], 2);
            assert!(
                r["working_memory_plan_bytes"].as_u64().unwrap()
                    > r["worst_case_bytes"].as_u64().unwrap()
            );
            assert!(r["general_limitations"][0]
                .as_str()
                .unwrap()
                .starts_with("A CMYK document is written as a CMYK"));
            let path = dir.path().join(format!("layers{depth}.{ext}"));
            save(doc, &path, container, LAYERS).unwrap_or_else(|e| panic!("{depth} {ext}: {e}"));
            let file = read(&std::fs::read(&path).unwrap());
            assert_eq!(file.depth, depth);
            assert_eq!(file.color, 4);
            assert!(file.merged_alpha, "{ext}: the negative record count");
            assert_eq!(file.large, container == PSB);
            assert_eq!(file.tiff, container == TIFF);
            let document = unsafe { &*doc };
            assert_eq!(
                file.icc.as_deref(),
                Some(document.cmyk_policy().unwrap().profile_bytes()),
                "{ext}: the CMYK profile"
            );
            assert_records(&file, document);
            // The written composite is the document's own render's.
            assert_composite(&file, &|x, y| {
                composite(doc, x as i32, y as i32).map(|v| f64::from(v) / 65535.0)
            });
            // Import Layers reads it back exactly.
            let again = open_psd(&path, 0).unwrap_or_else(|e| panic!("{depth} {ext}: {e}"));
            let reopened = unsafe { &*again };
            assert_eq!(
                reopened.cmyk_policy().unwrap().depth().bits(),
                u32::from(depth)
            );
            same_tree(document.root_layers(), reopened.root_layers());
            assert_eq!(
                description(reopened, "Curves")["params"],
                description(document, "Curves")["params"]
            );
            // The `hue2` body carries every field, at its default where the
            // source omitted it.
            let (hue, source) = (description(reopened, "Hue"), description(document, "Hue"));
            for key in ["hue", "saturation", "lightness"] {
                assert_eq!(
                    hue["params"][key].as_f64(),
                    source["params"][key].as_f64(),
                    "{key}"
                );
            }
            assert_eq!(hue["params"]["colorize"], false);
            let levels = description(reopened, "Levels");
            assert_eq!(
                levels["params"]["output_black"].as_f64().unwrap(),
                26.0 / 255.0
            );
            free(again);
            // The flattened mode: the CMYK save's own bytes.
            let flat = dir.path().join(format!("flat{depth}.{ext}"));
            let rf = report(doc, container, FLATTENED);
            assert_eq!(rf["available"], true, "{}", rf["refusal"]);
            save(doc, &flat, container, FLATTENED).unwrap();
            let bytes = std::fs::read(&flat).unwrap();
            if container == TIFF {
                assert!(bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*"));
            } else {
                let flat_file = read(&bytes);
                assert!(
                    flat_file.records.is_empty(),
                    "a flattened {ext} holds no record"
                );
                assert_eq!(flat_file.composite.len(), 4);
                assert_eq!(flat_file.large, container == PSB);
                let opened = open_psd(&flat, 0).unwrap();
                assert_eq!(unsafe { &*opened }.layer_count(), 1);
                free(opened);
            }
        }
    }
    free(sixteen);
    free(eight);
}

#[test]
fn the_written_composite_is_the_4a_oracles() {
    let dir = TempDir::new().unwrap();
    let doc = structured(false);
    for (container, ext) in [(PSD, "psd"), (PSB, "psb"), (TIFF, "tif")] {
        let path = dir.path().join(format!("oracle.{ext}"));
        save(doc, &path, container, LAYERS).unwrap();
        let file = read(&std::fs::read(&path).unwrap());
        assert_composite(&file, &|x, y| oracle(doc, x as i64, y as i64));
    }
    free(doc);
}

#[test]
fn an_rgb_document_still_needs_its_projection() {
    let dir = TempDir::new().unwrap();
    let rgb = doctored_core::precision_image::PrecisionImage::from_rgba16(
        2,
        2,
        vec![[1000, 2000, 3000, 65535]; 4],
        doctored_core::icc::IccProfile::srgb(),
        None,
    )
    .unwrap();
    let doc = Box::into_raw(Box::new(
        PrecisionDocument::from_image(
            rgb,
            doctored_core::metadata::Metadata::default(),
            doctored_core::metadata::Resolution::default(),
        )
        .unwrap(),
    ));
    let path = dir.path().join("rgb.psd");
    let error = save(doc, &path, PSD, LAYERS).unwrap_err();
    assert!(error.contains("requires its projection"), "{error}");
    assert!(!path.exists());
    free(doc);
}
