//! #1 phase 3a: Grayscale PSD, PSB and layered TIFF files as Grayscale
//! documents, through the C API.
//!
//! Oracles: gray files assembled here byte by byte from Adobe's published
//! framing (the records through `psd16_fixture`'s `Record` and `pack`, the
//! descriptors through `psd_descriptor_fixture`, a big-endian TIFF directory
//! from TIFF 6.0) — never through the crate's writer. (A Gray document's
//! PSD export, and so its round trips, shipped in #1 phase 3b.)
//! A `Grsc` colour's gray is the Gray profile's `kTRC` of `1 − K/100`,
//! computed here from the test profile's own gamma.

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_export::{
    dr_export_result_bytes, dr_export_result_free, dr_precision_doc_export_encode,
};
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_psd::{
    dr_precision_doc_open_gray_psd, dr_precision_doc_open_psd, dr_psd_import_report,
    dr_tiff_has_layers,
};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::gray_fixture::{gray_icc, srgb_encode};
use crate::gray_ops::LIMIT;
use crate::psd16_fixture::{block, mask_body, Record};
use crate::psd_descriptor_fixture::{body, boolean, doub, enumeration, objc, rgbc, untf};

const W: u32 = 8;
const H: u32 = 6;
/// The test profile's `kTRC` gamma as its u8Fixed8 stores it.
const GAMMA: f64 = 563.0 / 256.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Psd,
    Psb,
    Tiff,
}

// ------------------------------------------------------------ the builder --

/// Codes at `depth` as the file stores them, big-endian at 16 bits.
fn sample_bytes(depth: u16, values: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    for v in values {
        if depth == 16 {
            out.extend(v.to_be_bytes());
        } else {
            out.push(*v as u8);
        }
    }
    out
}

/// A Raw channel: compression word 0, then the samples.
fn raw(depth: u16, values: &[u16]) -> Vec<u8> {
    let mut out = vec![0, 0];
    out.extend(sample_bytes(depth, values));
    out
}

/// A gray raster record: channel 0 the gray, `-1` the alpha when given.
fn gray_layer(
    name: &str,
    bounds: [i32; 4],
    gray: &[u16],
    alpha: Option<&[u16]>,
    depth: u16,
) -> Record {
    let mut channels = Vec::new();
    if let Some(alpha) = alpha {
        channels.push((-1, raw(depth, alpha)));
    }
    channels.push((0, raw(depth, gray)));
    Record::raster(name, bounds, channels)
}

/// A pixel-less record: its two empty placeholder channels.
fn pixel_less(name: &str) -> Record {
    Record::raster(name, [0; 4], vec![(-1, vec![0, 0]), (0, vec![0, 0])])
}

/// A folder (1 open, 2 closed, 3 the closing divider) with a gray file's two
/// placeholder channels.
fn folder(name: &str, divider: u32, blend: [u8; 4]) -> Record {
    Record {
        channels: vec![(-1, vec![0, 0]), (0, vec![0, 0])],
        ..Record::folder(name, divider, blend)
    }
}

/// One record's bytes: `psd16_fixture`'s layout, with a PSB's u64 channel
/// lengths when `large`.
fn encode(record: &Record, large: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for value in record.bounds {
        out.extend(value.to_be_bytes());
    }
    out.extend((record.channels.len() as u16).to_be_bytes());
    for (id, data) in &record.channels {
        out.extend(id.to_be_bytes());
        if large {
            out.extend((data.len() as u64).to_be_bytes());
        } else {
            out.extend((data.len() as u32).to_be_bytes());
        }
    }
    out.extend(b"8BIM");
    out.extend(record.blend);
    out.extend([record.opacity, record.clipping, record.flags, 0]);
    let mut extra = (record.mask_body.len() as u32).to_be_bytes().to_vec();
    extra.extend(&record.mask_body);
    extra.extend((record.ranges.len() as u32).to_be_bytes());
    extra.extend(&record.ranges);
    extra.push(record.name.len() as u8);
    extra.extend(record.name.as_bytes());
    while !extra.len().is_multiple_of(4) {
        extra.push(0);
    }
    if record.divider != 0 {
        let mut body = record.divider.to_be_bytes().to_vec();
        if matches!(record.divider, 1 | 2) {
            body.extend(b"8BIM");
            body.extend(record.blend);
        }
        extra.extend(block(b"lsct", &body));
    }
    extra.extend(&record.blocks);
    out.extend((extra.len() as u32).to_be_bytes());
    out.extend(extra);
    out
}

/// The layer information: the count (negative: merged alpha), the records,
/// their channel data, padded even.
fn layer_info(records: &[Record], large: bool) -> Vec<u8> {
    let mut info = (-(records.len() as i16)).to_be_bytes().to_vec();
    for record in records {
        info.extend(encode(record, large));
    }
    for record in records {
        for (_, data) in &record.channels {
            info.extend(data);
        }
    }
    if !info.len().is_multiple_of(2) {
        info.push(0);
    }
    info
}

fn resource(id: u16, body: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend(id.to_be_bytes());
    out.extend([0, 0]);
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    if !body.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

fn resources(icc: Option<&[u8]>) -> Vec<u8> {
    let mut out = icc.map_or(Vec::new(), |icc| resource(1039, icc));
    out.extend(resource(
        1005,
        &crate::psd16_fixture::resolution_info(72.0, 72.0),
    ));
    out
}

/// White matte at `depth`, rounded.
fn matte(depth: u16, color: u16, alpha: u16) -> u16 {
    let max = if depth == 16 { 65535.0 } else { 255.0 };
    let (c, a) = (f64::from(color), f64::from(alpha));
    ((c * a + max * (max - a)) / max).round() as u16
}

/// A complete gray file of `records`; `composite` is straight (gray, alpha)
/// codes at `depth`, stored white-matted with its alpha channel (a TIFF's
/// strip straight).
fn gray_file(
    kind: Kind,
    depth: u16,
    records: &[Record],
    icc: Option<&[u8]>,
    composite: &[(u16, u16)],
) -> Vec<u8> {
    if kind == Kind::Tiff {
        return gray_tiff(depth, records, icc, composite);
    }
    let large = kind == Kind::Psb;
    let mut out = b"8BPS".to_vec();
    out.extend((if large { 2u16 } else { 1 }).to_be_bytes());
    out.extend([0; 6]);
    out.extend(2u16.to_be_bytes());
    out.extend(H.to_be_bytes());
    out.extend(W.to_be_bytes());
    out.extend(depth.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend(0u32.to_be_bytes());
    let res = resources(icc);
    out.extend((res.len() as u32).to_be_bytes());
    out.extend(res);
    let info = layer_info(records, large);
    let mut section = Vec::new();
    if large {
        section.extend((info.len() as u64).to_be_bytes());
    } else {
        section.extend((info.len() as u32).to_be_bytes());
    }
    section.extend(info);
    section.extend(0u32.to_be_bytes());
    if large {
        out.extend((section.len() as u64).to_be_bytes());
    } else {
        out.extend((section.len() as u32).to_be_bytes());
    }
    out.extend(section);
    out.extend(0u16.to_be_bytes());
    let gray: Vec<u16> = composite.iter().map(|&(g, a)| matte(depth, g, a)).collect();
    let alpha: Vec<u16> = composite.iter().map(|&(_, a)| a).collect();
    out.extend(sample_bytes(depth, &gray));
    out.extend(sample_bytes(depth, &alpha));
    out
}

/// A big-endian layered gray TIFF (TIFF 6.0): one chunky strip of straight
/// gray and alpha, BitsPerSample's two SHORTs inline, tag 37724's block.
fn gray_tiff(
    depth: u16,
    records: &[Record],
    icc: Option<&[u8]>,
    composite: &[(u16, u16)],
) -> Vec<u8> {
    let mut strip = Vec::new();
    for &(g, a) in composite {
        strip.extend(sample_bytes(depth, &[g, a]));
    }
    let info = layer_info(records, false);
    let mut block37724 = b"Adobe Photoshop Document Data Block\0".to_vec();
    block37724.extend(b"8BIM");
    block37724.extend(if depth == 8 { b"Layr" } else { b"Lr16" });
    block37724.extend((info.len() as u32).to_be_bytes());
    block37724.extend(&info);
    while !block37724.len().is_multiple_of(4) {
        block37724.push(0);
    }
    let res = resources(icc);
    let mut out = b"MM\0*".to_vec();
    out.extend([0; 4]);
    let strip_at = out.len() as u32;
    out.extend(&strip);
    let place = |out: &mut Vec<u8>, bytes: &[u8]| {
        if !out.len().is_multiple_of(2) {
            out.push(0);
        }
        let at = out.len() as u32;
        out.extend(bytes);
        at
    };
    let res_at = place(&mut out, &res);
    let icc_at = icc.map(|icc| place(&mut out, icc));
    let block_at = place(&mut out, &block37724);
    if !out.len().is_multiple_of(2) {
        out.push(0);
    }
    let directory = out.len() as u32;
    let short = |v: u16| {
        let b = v.to_be_bytes();
        [b[0], b[1], 0, 0]
    };
    let d = depth.to_be_bytes();
    let mut entries: Vec<(u16, u16, u32, [u8; 4])> = vec![
        (256, 4, 1, W.to_be_bytes()),
        (257, 4, 1, H.to_be_bytes()),
        (258, 3, 2, [d[0], d[1], d[0], d[1]]),
        (259, 3, 1, short(1)),
        (262, 3, 1, short(1)),
        (273, 4, 1, strip_at.to_be_bytes()),
        (277, 3, 1, short(2)),
        (278, 4, 1, H.to_be_bytes()),
        (279, 4, 1, (strip.len() as u32).to_be_bytes()),
        (284, 3, 1, short(1)),
        (338, 3, 1, short(2)),
        (34377, 1, res.len() as u32, res_at.to_be_bytes()),
    ];
    if let (Some(icc), Some(at)) = (icc, icc_at) {
        entries.push((34675, 7, icc.len() as u32, at.to_be_bytes()));
    }
    entries.push((37724, 7, block37724.len() as u32, block_at.to_be_bytes()));
    out.extend((entries.len() as u16).to_be_bytes());
    for (tag, kind, count, value) in entries {
        out.extend(tag.to_be_bytes());
        out.extend(kind.to_be_bytes());
        out.extend(count.to_be_bytes());
        out.extend(value);
    }
    out.extend([0; 4]);
    out[4..8].copy_from_slice(&directory.to_be_bytes());
    out
}

/// A `levl` body: version 2, record 0 `master`, 28 identity records.
fn levels_body(master: [u16; 5]) -> Vec<u8> {
    let mut out = 2u16.to_be_bytes().to_vec();
    for i in 0..29 {
        let record = if i == 0 {
            master
        } else {
            [0, 255, 0, 255, 100]
        };
        for v in record {
            out.extend(v.to_be_bytes());
        }
    }
    out
}

/// An `lfx2` block of one Color Overlay whose colour is `Grsc` {`Gry ` k}.
fn grsc_overlay(k: f64) -> Vec<u8> {
    let mut body = 0u32.to_be_bytes().to_vec();
    body.extend(body16(&[
        untf(b"Scl ", b"#Prc", 100.0),
        boolean(b"masterFXSwitch", 1),
        objc(
            b"SoFi",
            b"SoFi",
            &[
                boolean(b"enab", 1),
                enumeration(b"Md  ", b"BlnM", b"Nrml"),
                objc(b"Clr ", b"Grsc", &[doub(b"Gry ", k)]),
                untf(b"Opct", b"#Prc", 100.0),
            ],
        ),
    ]));
    block(b"lfx2", &body)
}

fn body16(items: &[Vec<u8>]) -> Vec<u8> {
    body(16, b"null", items)
}

/// A pixel-less Solid Color Fill (`SoCo`) record's block.
fn solid_fill() -> Vec<u8> {
    block(b"SoCo", &body16(&[rgbc(b"Clr ", 200.0, 40.0, 90.0)]))
}

/// Blending ranges: slot 0 (Gray) and slot 1 (the channel copy, read as Red).
fn ranges(gray: [u8; 4], channel: [u8; 4]) -> Vec<u8> {
    let full = [0u8, 0, 255, 255];
    [gray, full, channel, full].concat()
}

// ------------------------------------------------------------ the C API --

fn report_of(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value =
        serde_json::from_str(unsafe { std::ffi::CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn open(path: &Path, mode: i32, working: &str) -> Result<*mut PrecisionDocument, String> {
    let report = report_of(path);
    let fingerprint = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let working = CString::new(working).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_gray_psd(
            cpath(path).as_ptr(),
            mode,
            fingerprint.as_ptr(),
            working.as_ptr(),
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

fn written(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn free(doc: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(doc) };
}

fn descriptor(doc: *const PrecisionDocument) -> Value {
    unsafe { &*doc }.descriptor().unwrap()
}

/// Whether the document's kept-gray TIFF export (`gray_export`, phase 1b:
/// its Gray ICC verbatim in tag 34675) holds `icc` byte for byte: the Gray
/// profile the open adopted, read back through a path that never touches
/// the PSD writer (whose Gray form shipped in #1 phase 3b).
fn embeds_gray_icc(doc: *const PrecisionDocument, depth: u16, icc: &[u8]) -> bool {
    let request = CString::new(format!(
        r#"{{"format":"tiff","bit_depth":{depth},"profile":"document","dither":"none"}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_export_encode(
            doc,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!result.is_null(), "{}", take_err_string(err));
    let mut len = 0usize;
    let bytes = unsafe {
        let p = dr_export_result_bytes(result, &mut len);
        std::slice::from_raw_parts(p, len).to_vec()
    };
    unsafe { dr_export_result_free(result) };
    bytes.windows(icc.len()).any(|w| w == icc)
}

fn find<'a>(layers: &'a [PrecisionDocumentLayer], name: &str) -> &'a PrecisionDocumentLayer {
    for layer in layers {
        if layer.name() == name {
            return layer;
        }
        if let Some(children) = layer.children() {
            if let Some(found) = children.iter().find(|l| l.name() == name) {
                return found;
            }
        }
    }
    panic!("no layer {name}");
}

/// A layer's u16 samples.
fn samples(layer: &PrecisionDocumentLayer) -> Vec<[u16; 4]> {
    layer.source().unwrap().to_rgba16().unwrap()
}

/// `v` at `depth` as the document stores it (8 bits ×257).
fn stored(depth: u16, v: u16) -> u16 {
    if depth == 8 {
        v * 257
    } else {
        v
    }
}

// ------------------------------------------------------- the fixture file --

/// The synthetic scene: an opaque Back, a masked Shape with an alpha ramp in
/// a 50 % Normal group, a Multiply layer, a Levels record, a Color Overlay
/// in `Grsc` with Blend If Gray and its channel copy, and a fill.
fn scene(depth: u16) -> Vec<Record> {
    let max = if depth == 16 { 65535u32 } else { 255 };
    let code = |i: u32, n: u32| (i * max / n) as u16;
    let back: Vec<u16> = (0..W * H).map(|i| code(i, W * H)).collect();
    let shape: Vec<u16> = (0..12).map(|i| code(11 - i, 12)).collect();
    let alpha: Vec<u16> = (0..12).map(|i| code(i + 1, 12)).collect();
    let mask: Vec<u16> = (0..12).map(|i| code(i, 11)).collect();
    let mut shape = gray_layer("Shape", [1, 2, 4, 6], &shape, Some(&alpha), depth);
    shape.mask_body = mask_body([1, 2, 4, 6], 0, 0);
    shape.channels.push((-2, raw(depth, &mask)));
    let mut group = folder("Group", 1, *b"norm");
    group.opacity = 128;
    let mut multiply = gray_layer("Multiply", [0, 0, 6, 8], &[code(1, 2); 48], None, depth);
    multiply.blend = *b"mul ";
    multiply.opacity = 153;
    let mut levels = pixel_less("Levels");
    levels.blocks = block(b"levl", &levels_body([10, 240, 0, 255, 120]));
    let mut over = gray_layer("Over", [0, 0, 2, 2], &[code(1, 3); 4], None, depth);
    over.blocks = grsc_overlay(25.0);
    over.ranges = ranges([0, 0, 200, 255], [0, 0, 200, 255]);
    let mut fill = pixel_less("Fill");
    fill.blocks = solid_fill();
    vec![
        gray_layer("Back", [0, 0, 6, 8], &back, None, depth),
        folder("</Layer group>", 3, *b"norm"),
        shape,
        group,
        multiply,
        levels,
        over,
        fill,
    ]
}

fn composite(depth: u16) -> Vec<(u16, u16)> {
    let max = if depth == 16 { 65535 } else { 255 };
    (0..W * H)
        .map(|i| {
            (
                (i * 37 % 200) as u16,
                if i % 5 == 0 { max / 2 } else { max },
            )
        })
        .collect()
}

#[test]
fn gray_psd_psb_and_tiff_open_as_gray_documents_with_their_layers() {
    let dir = TempDir::new().unwrap();
    let icc = gray_icc("Test Gray", 2.2);
    for depth in [8u16, 16] {
        for kind in [Kind::Psd, Kind::Psb, Kind::Tiff] {
            let what = format!("{kind:?} {depth}");
            let records = scene(depth);
            let bytes = gray_file(kind, depth, &records, Some(&icc), &composite(depth));
            let path = written(&dir, &format!("scene{depth}{kind:?}.bin"), &bytes);
            if kind == Kind::Tiff {
                assert!(
                    unsafe { dr_tiff_has_layers(cpath(&path).as_ptr()) },
                    "{what}"
                );
            }
            let report = report_of(&path);
            assert_eq!(report["import_precision"], "gray", "{what}: {report}");
            assert_eq!(report["profile"], "Test Gray", "{what}");
            assert_eq!(report["layered_available"], true, "{what}: {report}");
            let observations = report["observations"].to_string();
            assert!(
                observations.contains("(“Over”) has Photoshop layer effects (color_overlay and Blend If ranges on gray)"),
                "{what}: {report}"
            );
            let losses = report["verified_losses"].to_string();
            assert!(
                losses.contains("(“Fill”) is a Photoshop"),
                "{what}: {losses}"
            );
            assert!(losses.contains("omits fill layers"), "{what}: {losses}");

            let doc = open(&path, 0, "dot_gain_20").unwrap_or_else(|e| panic!("{what}: {e}"));
            let d = unsafe { &*doc };
            assert!(d.is_gray(), "{what}");
            let desc = descriptor(doc);
            assert_eq!(desc["color_mode"], "gray", "{what}: {desc}");
            assert_eq!(desc["gray"]["profile"], "Test Gray", "{what}: {desc}");
            assert_eq!(desc["gray"]["depth"], depth, "{what}: {desc}");
            // The Gray ICC travels verbatim: the open adopts the file's
            // bytes, which the kept-gray TIFF export embeds as they are.
            assert!(
                embeds_gray_icc(doc, depth, &icc),
                "{what}: the Gray ICC verbatim"
            );
            let names: Vec<&str> = d.root_layers().iter().map(|l| l.name()).collect();
            assert_eq!(
                names,
                ["Back", "Group", "Multiply", "Levels", "Over"],
                "{what}"
            );
            let max = if depth == 16 { 65535u32 } else { 255 };
            let code = |i: u32, n: u32| (i * max / n) as u16;
            // Back: every code exactly (×257 at 8 bits), R = G = B, opaque.
            for (i, p) in samples(find(d.root_layers(), "Back")).iter().enumerate() {
                let want = stored(depth, code(i as u32, W * H));
                assert_eq!(*p, [want, want, want, 65535], "{what} Back {i}");
            }
            // Shape: its rectangle, offset, alpha and mask.
            let shape = find(d.root_layers(), "Shape");
            assert_eq!(shape.offset(), (2, 1), "{what}");
            for (i, p) in samples(shape).iter().enumerate() {
                let g = stored(depth, code(11 - i as u32, 12));
                let a = stored(depth, code(i as u32 + 1, 12));
                assert_eq!(*p, [g, g, g, a], "{what} Shape {i}");
            }
            let mask = shape.mask().expect("mask");
            assert!(mask.enabled(), "{what}");
            let want: Vec<u16> = (0..12).map(|i| stored(depth, code(i, 11))).collect();
            assert_eq!(mask.plane().samples(), &want[..], "{what} mask");
            let group = find(d.root_layers(), "Group");
            assert!(!group.is_pass_through(), "{what}");
            assert!((group.opacity() - 128.0 / 255.0).abs() < 1e-6, "{what}");
            let multiply = find(d.root_layers(), "Multiply");
            assert_eq!(multiply.blend_mode(), BlendMode::Multiply, "{what}");
            // Levels: the composite record is the master (H-L).
            let levels: Value =
                serde_json::from_str(d.adjustment_description(index(doc, "Levels")).unwrap())
                    .unwrap();
            assert_eq!(levels["op"], "levels", "{what}: {levels}");
            assert!((levels["params"]["black"].as_f64().unwrap() - 10.0 / 255.0).abs() < 1e-6);
            assert!((levels["params"]["gamma"].as_f64().unwrap() - 1.2).abs() < 1e-6);
            assert!(levels["params"].get("r").is_none(), "{what}: {levels}");
            // Over: the Grsc overlay is the gray of kTRC(0.75); Blend If Gray only.
            let over = find(d.root_layers(), "Over");
            let style: Value = serde_json::from_str(&over.reported_style().unwrap()).unwrap();
            let y = 0.75f64.powf(GAMMA);
            let v = (255.0 * srgb_encode(y)).round() as u8;
            let hex = format!("#{v:02x}{v:02x}{v:02x}");
            assert_eq!(
                style["effects"][0]["type"], "color_overlay",
                "{what}: {style}"
            );
            assert_eq!(
                style["effects"][0]["color"],
                hex.as_str(),
                "{what}: {style}"
            );
            let blend_if = over.blend_if().expect("blend if");
            let ranges: Vec<_> = blend_if.ranges().collect();
            assert_eq!(ranges.len(), 1, "{what}: the channel copy is dropped");
            assert_eq!(ranges[0].this_layer, [0, 0, 200, 255], "{what}");
            free(doc);

            // The saved composite (a TIFF has none here).
            match open(&path, 1, "dot_gain_20") {
                Ok(doc) => {
                    assert_ne!(kind, Kind::Tiff);
                    let d = unsafe { &*doc };
                    assert!(d.is_gray());
                    assert_eq!(d.root_layers().len(), 1);
                    for (p, &(g, a)) in samples(&d.root_layers()[0]).iter().zip(&composite(depth)) {
                        assert_eq!(p[3], stored(depth, a), "{what}");
                        assert!(p[0] == p[1] && p[1] == p[2], "{what}");
                        if a == max as u16 {
                            assert_eq!(p[0], stored(depth, g), "{what}");
                        }
                    }
                    free(doc);
                }
                Err(e) => {
                    assert_eq!(kind, Kind::Tiff, "{what}: {e}");
                    assert!(e.contains("ordinary TIFF open"), "{e}");
                }
            }
        }
    }
}

#[test]
fn an_untagged_file_takes_the_working_gray_and_a_non_gray_profile_refuses() {
    let dir = TempDir::new().unwrap();
    let records = vec![gray_layer("Back", [0, 0, 6, 8], &[100; 48], None, 8)];
    let path = written(
        &dir,
        "untagged.psd",
        &gray_file(Kind::Psd, 8, &records, None, &composite(8)),
    );
    let report = report_of(&path);
    assert_eq!(report["import_precision"], "gray");
    assert_eq!(report["profile"], Value::Null, "{report}");
    assert!(
        report["observations"]
            .to_string()
            .contains("the working Gray (Color Settings) is assigned"),
        "{report}"
    );
    assert!(!report.to_string().contains("Dot Gain 20"), "{report}");
    let doc = open(&path, 0, "gray_gamma_22").unwrap();
    let desc = unsafe { &*doc }.info().unwrap();
    assert_eq!(descriptor(doc)["gray"]["profile"], "Gray Gamma 2.2");
    let warnings = desc["import_warnings"].to_string();
    assert!(
        warnings
            .contains("no usable Gray profile, so the working Gray “Gray Gamma 2.2” was assigned"),
        "{desc}"
    );
    free(doc);

    let srgb = IccProfile::srgb();
    let path = written(
        &dir,
        "srgb.psd",
        &gray_file(Kind::Psd, 8, &records, Some(srgb.bytes()), &composite(8)),
    );
    let report = report_of(&path);
    // `GrayPolicy::new`'s sentence, on both choices and at open.
    let refusal = report["layered_refusal"].as_str().unwrap().to_string();
    assert!(
        refusal.starts_with("cannot use the Gray profile: "),
        "{report}"
    );
    assert_eq!(
        report["stored_composite_refusal"],
        refusal.as_str(),
        "{report}"
    );
    let error = open(&path, 0, "dot_gain_20").unwrap_err();
    assert!(error.ends_with(&refusal), "{error}");
}

#[test]
fn blend_if_off_gray_drops_the_style_by_name_and_a_refused_adjustment_is_omitted() {
    let dir = TempDir::new().unwrap();
    let icc = gray_icc("Test Gray", 2.2);
    let mut over = gray_layer("Over", [0, 0, 6, 8], &[90; 48], None, 16);
    over.blocks = grsc_overlay(40.0);
    over.ranges = ranges([0, 0, 200, 255], [30, 40, 255, 255]);
    // A Color Balance record: a kind a Grayscale document cannot hold.
    let mut hue = pixel_less("Hue");
    let mut body = vec![0, 10];
    body.extend([0u8; 16]);
    body.extend([1, 0]);
    hue.blocks = block(b"blnc", &body);
    let records = vec![
        gray_layer("Back", [0, 0, 6, 8], &[30_000; 48], None, 16),
        over,
        hue,
    ];
    let path = written(
        &dir,
        "refused.psd",
        &gray_file(Kind::Psd, 16, &records, Some(&icc), &composite(16)),
    );
    let report = report_of(&path);
    let losses = report["verified_losses"].to_string();
    assert!(
        losses.contains("Layer 1 (“Over”) has Photoshop layer effects that a Grayscale document cannot carry (Blend If on the Red channel is unavailable in a Grayscale document.)"),
        "{report}"
    );
    let doc = open(&path, 0, "dot_gain_20").unwrap();
    let d = unsafe { &*doc };
    let over = find(d.root_layers(), "Over");
    assert!(over.style().is_none() && over.blend_if().is_none());
    assert!(
        losses.contains("(“Hue”) is a Photoshop Color Balance adjustment layer (blnc) that a Grayscale document cannot hold (Color Balance is unavailable in a Grayscale document.)"),
        "{losses}"
    );
    assert_eq!(
        d.root_layers().len(),
        2,
        "the refused adjustment is omitted"
    );
    free(doc);
}

#[test]
fn a_file_of_only_a_fill_and_a_refused_adjustment_opens_its_composite() {
    // #1 phase 3b, review nit 2: the gray open drops every fill and every
    // adjustment the Gray table refuses, so a file of only those imports
    // nothing and Import Layers opens the stored composite, as RGB does.
    let dir = TempDir::new().unwrap();
    let icc = gray_icc("Test Gray", 2.2);
    let mut fill = pixel_less("Fill");
    fill.blocks = solid_fill();
    let mut hue = pixel_less("Hue");
    let mut body = vec![0, 10];
    body.extend([0u8; 16]);
    body.extend([1, 0]);
    hue.blocks = block(b"blnc", &body);
    let records = vec![fill, hue];
    for depth in [8u16, 16] {
        let path = written(
            &dir,
            &format!("nothing{depth}.psd"),
            &gray_file(Kind::Psd, depth, &records, Some(&icc), &composite(depth)),
        );
        let report = report_of(&path);
        assert_eq!(report["layered_available"], true, "{report}");
        assert!(
            report["observations"].to_string().contains("Every layer record is an adjustment or fill layer with no pixels; Import Layers opens the stored composite as a single layer."),
            "{report}"
        );
        let doc = open(&path, 0, "dot_gain_20").unwrap();
        let d = unsafe { &*doc };
        assert!(d.is_gray());
        assert_eq!(d.root_layers().len(), 1, "the composite as one layer");
        let max = if depth == 16 { 65535u16 } else { 255 };
        for (p, &(g, a)) in samples(&d.root_layers()[0]).iter().zip(&composite(depth)) {
            assert_eq!(p[3], stored(depth, a));
            assert!(p[0] == p[1] && p[1] == p[2]);
            if a == max {
                assert_eq!(p[0], stored(depth, g));
            }
        }
        free(doc);
    }
}

#[test]
fn a_gray_file_refuses_the_rgb_typed_open_by_name() {
    let dir = TempDir::new().unwrap();
    let records = vec![gray_layer("Back", [0, 0, 6, 8], &[100; 48], None, 16)];
    let path = written(
        &dir,
        "gray16.psd",
        &gray_file(Kind::Psd, 16, &records, None, &composite(16)),
    );
    let report = report_of(&path);
    let fingerprint = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open_psd(
            cpath(&path).as_ptr(),
            0,
            fingerprint.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(doc.is_null());
    assert!(
        take_err_string(err).contains("this is a Grayscale file; open it as a Grayscale document")
    );
}

fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

#[test]
fn photoshops_measured_gray_channel_forms_carry() {
    // `gray-files-547/take1`: Photoshop keeps a Grayscale Blend If in slot 1
    // (slot 0 the default) and a Grayscale Levels in record 1 (record 0 the
    // identity); both are the Gray channel's.
    let dir = TempDir::new().unwrap();
    let icc = gray_icc("Test Gray", 2.2);
    let full = [0u8, 0, 255, 255];
    let mut ranged = gray_layer("Ranged", [0, 0, 6, 8], &[200; 48], None, 8);
    ranged.ranges = [full, full, full, [0, 0, 96, 160]].concat();
    let mut levels = pixel_less("Levels");
    let mut body = levels_body([0, 255, 0, 255, 100]);
    body[12..22].copy_from_slice(&[0, 10, 0, 240, 0, 0, 0, 255, 0, 120]);
    levels.blocks = block(b"levl", &body);
    let records = vec![
        gray_layer("Back", [0, 0, 6, 8], &[60; 48], None, 8),
        ranged,
        levels,
    ];
    let path = written(
        &dir,
        "measured.psd",
        &gray_file(Kind::Psd, 8, &records, Some(&icc), &composite(8)),
    );
    let doc = open(&path, 0, "dot_gain_20").unwrap();
    let d = unsafe { &*doc };
    let ranges: Vec<_> = find(d.root_layers(), "Ranged")
        .blend_if()
        .expect("the Gray range")
        .ranges()
        .collect();
    assert_eq!(ranges.len(), 1, "{ranges:?}");
    assert_eq!(format!("{:?}", ranges[0].channel), "Gray");
    assert_eq!(ranges[0].underlying, [0, 0, 96, 160]);
    let levels: Value =
        serde_json::from_str(d.adjustment_description(index(doc, "Levels")).unwrap()).unwrap();
    let r = &levels["params"]["r"];
    assert!(
        (r["black"].as_f64().unwrap() - 10.0 / 255.0).abs() < 1e-6,
        "{levels}"
    );
    assert!(
        (r["gamma"].as_f64().unwrap() - 1.2).abs() < 1e-6,
        "{levels}"
    );
    assert_eq!(levels["params"]["g"], *r, "{levels}");
    assert_eq!(levels["params"]["b"], *r, "{levels}");
    free(doc);
}
