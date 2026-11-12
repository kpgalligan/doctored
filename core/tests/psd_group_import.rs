//! Folder fidelity through the existing PSD/report/native C APIs. Fixtures are
//! independent byte constructions; expected pixels use explicit source-over
//! and multiply arithmetic, never the importer or compositor as an oracle.
//! DR_PHASE10A_FIXTURES optionally exports the small review fixtures + manifest.

use crate::common;
use common::{cpath, flat_pixels, take_err_string};
use doctored_core::doc::{BlendMode, DrDocument, LayerKind};
use doctored_core::ffi_doc::{dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_psd_import_report};
use serde_json::{json, Value};
use std::ffi::CString;
use std::path::Path;
use std::ptr;
use tempfile::TempDir;

#[derive(Clone)]
struct Record {
    name: &'static str,
    rgb: [u8; 3],
    opacity: u8,
    flags: u8,
    clipped: u8,
    mask: bool,
    key: [u8; 4],
    blocks: Vec<u8>,
}
fn block(key: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = b"8BIM".to_vec();
    bytes.extend(key);
    bytes.extend((body.len() as u32).to_be_bytes());
    bytes.extend(body);
    bytes
}
impl Record {
    fn raster(name: &'static str, rgb: [u8; 3]) -> Self {
        Self {
            name,
            rgb,
            opacity: 255,
            flags: 8,
            clipped: 0,
            mask: false,
            key: *b"norm",
            blocks: vec![],
        }
    }
    fn folder(name: &'static str, kind: u32, blend: Option<&[u8; 4]>) -> Self {
        let mut body = kind.to_be_bytes().to_vec();
        if let Some(blend) = blend {
            body.extend(b"8BIM");
            body.extend(blend);
        }
        Self {
            blocks: block(b"lsct", &body),
            ..Self::raster(name, [1, 2, 3])
        }
    }
    fn close() -> Self {
        Self::folder("end", 3, None)
    }
    fn bytes(&self) -> (Vec<u8>, Vec<u8>) {
        let mut bytes = Vec::new();
        for n in [0i32, 0, 1, 1] {
            bytes.extend(n.to_be_bytes());
        }
        bytes.extend(4u16.to_be_bytes());
        for id in [0i16, 1, 2, -1] {
            bytes.extend(id.to_be_bytes());
            bytes.extend(3u32.to_be_bytes());
        }
        bytes.extend(b"8BIM");
        bytes.extend(self.key);
        bytes.extend([self.opacity, self.clipped, self.flags, 0]);
        let mut extra = if self.mask {
            let mut bytes = 20u32.to_be_bytes().to_vec();
            bytes.extend([0; 24]); // mask20 and empty Blend If length
            bytes
        } else {
            vec![0; 8]
        }; // empty mask and Blend If ranges
        extra.push(self.name.len() as u8);
        extra.extend(self.name.as_bytes());
        while !extra.len().is_multiple_of(4) {
            extra.push(0);
        }
        extra.extend(&self.blocks);
        bytes.extend((extra.len() as u32).to_be_bytes());
        bytes.extend(extra);
        let mut channels = Vec::new();
        for value in [self.rgb[0], self.rgb[1], self.rgb[2], 255] {
            channels.extend([0, 0, value]);
        }
        (bytes, channels)
    }
}

// File records are bottom-first, channels follow all records in the same order.
fn fixture(records: &[Record]) -> Vec<u8> {
    let mut info = (records.len() as i16).to_be_bytes().to_vec();
    let mut channels = Vec::new();
    for record in records {
        let (r, c) = record.bytes();
        info.extend(r);
        channels.extend(c);
    }
    info.extend(channels);
    if !info.len().is_multiple_of(2) {
        info.push(0);
    }
    let mut layers = (info.len() as u32).to_be_bytes().to_vec();
    layers.extend(info);
    layers.extend(0u32.to_be_bytes());
    let mut bytes = b"8BPS\0\x01\0\0\0\0\0\0".to_vec();
    bytes.extend(3u16.to_be_bytes());
    bytes.extend(1u32.to_be_bytes());
    bytes.extend(1u32.to_be_bytes());
    bytes.extend(8u16.to_be_bytes());
    bytes.extend(3u16.to_be_bytes());
    bytes.extend([0; 8]); // no color-mode data or resources
    bytes.extend((layers.len() as u32).to_be_bytes());
    bytes.extend(layers);
    bytes.extend([0, 0, 17, 29, 43]); // deliberately distinct saved composite
    bytes
}
fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}
fn report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    assert!(err.is_null());
    serde_json::from_str(&take_err_string(out)).unwrap()
}
fn open(path: &Path, mode: i32, report: &Value) -> Result<Box<DrDocument>, String> {
    let fingerprint = CString::new(report["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe {
        dr_doc_open_psd(
            cpath(path).as_ptr(),
            mode,
            fingerprint.as_ptr(),
            std::ptr::null(),
            &mut err,
        )
    };
    if doc.is_null() {
        Err(take_err_string(err))
    } else {
        assert!(err.is_null());
        Ok(unsafe { Box::from_raw(doc) })
    }
}
fn pixels(doc: &DrDocument) -> Vec<u8> {
    flat_pixels(doc as *const _)
}
fn save_reopen(dir: &TempDir, doc: &DrDocument) -> Box<DrDocument> {
    let path = cpath(&dir.path().join("roundtrip.rz"));
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, path.as_ptr(), std::ptr::null(), &mut err) },
        "{}",
        take_err_string(err)
    );
    let reopened = unsafe { dr_doc_open(path.as_ptr(), std::ptr::null(), &mut err) };
    assert!(!reopened.is_null(), "{}", take_err_string(err));
    unsafe { Box::from_raw(reopened) }
}
fn blend_expected(back: [u8; 3], source: [u8; 3], opacity: u8, multiply: bool) -> [u8; 4] {
    let w = f64::from(opacity) / 255.0;
    let mut out = [0, 0, 0, 255];
    for c in 0..3 {
        let b = f64::from(back[c]);
        let s = f64::from(source[c]);
        out[c] = (b * (1.0 - w) + (if multiply { b * s / 255.0 } else { s }) * w).round() as u8;
    }
    out
}
fn export(name: &str, bytes: &[u8], expected: [u8; 4], description: &str, groups: Value) {
    if let Ok(dir) = std::env::var("DR_PHASE10A_FIXTURES") {
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), bytes).unwrap();
        std::fs::write(dir.join(format!("{name}.json")), serde_json::to_vec_pretty(&json!({"fixture": name, "groups": groups, "size": [1,1], "layered_rgba": expected, "stored_composite_rgba": [17,29,43,255], "description": description, "provenance": "independently constructed in core/tests/psd_group_import.rs; source-over/multiply arithmetic oracle"})).unwrap()).unwrap();
    }
}

#[test]
fn folder_opacity_visibility_and_blends_affect_pixels_and_survive_native_reopen() {
    let dir = TempDir::new().unwrap();
    let back = [120, 180, 240];
    let source = [200, 100, 50];
    for (name, key, hidden, opacity) in [
        ("group-hidden.psd", *b"mul ", true, 128),
        ("group-multiply.psd", *b"mul ", false, 128),
        ("group-normal.psd", *b"norm", false, 128),
        ("group-transparent.psd", *b"norm", false, 0),
    ] {
        let mut folder = Record::folder("Folder", 2, Some(&key));
        folder.opacity = opacity;
        folder.flags |= if hidden { 2 } else { 0 };
        // Closing divider intentionally disagrees on all folder properties.
        let mut closing = Record::close();
        closing.opacity = 7;
        closing.flags = 10;
        closing.key = *b"scrn";
        let bytes = fixture(&[
            Record::raster("Backdrop", back),
            closing,
            Record::raster("Art", source),
            folder,
        ]);
        let expected = if hidden {
            [back[0], back[1], back[2], 255]
        } else {
            blend_expected(back, source, opacity, key == *b"mul ")
        };
        export(name, &bytes, expected, "Bottom-first rows Backdrop depth0, Art depth1, Folder depth0; Folder closed, opacity as filename case, hidden only in group-hidden. lsct overrides ordinary Normal record key.",json!([{ "name":"Folder", "visible":!hidden,"opacity":f64::from(opacity)/255.0,"blend":if key == *b"mul " {"Multiply"} else {"Normal"},"open":false }]));
        let path = write(&dir, name, &bytes);
        let report = report(&path);
        assert_eq!(report["layered_available"], true);
        assert!(!report["general_limitations"]
            .to_string()
            .contains("groups import visible"));
        assert!(report["observations"].to_string().contains("are retained"));
        let doc = open(&path, 0, &report).unwrap();
        assert_eq!(pixels(&doc), expected);
        let group = &doc.layers[2];
        assert_eq!(group.kind, LayerKind::Group);
        assert_eq!(group.opacity, opacity as f32 / 255.0);
        assert_eq!(group.visible, !hidden);
        assert!(!group.open);
        assert_eq!(
            group.blend,
            if key == *b"mul " {
                BlendMode::Multiply
            } else {
                BlendMode::Normal
            }
        );
        let restored = save_reopen(&dir, &doc);
        assert_eq!(pixels(&restored), expected);
        for (a, b) in doc.layers.iter().zip(&restored.layers) {
            assert_eq!(
                (&a.name, a.kind, a.depth, a.opacity, a.blend, a.visible, a.open),
                (&b.name, b.kind, b.depth, b.opacity, b.blend, b.visible, b.open)
            );
            assert_eq!(
                a.pixels.contiguous_image().as_raw(),
                b.pixels.contiguous_image().as_raw()
            );
        }
        assert_eq!(pixels(&open(&path, 1, &report).unwrap()), [17, 29, 43, 255]);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn pass_through_reaches_backdrop_while_normal_isolates_children() {
    let dir = TempDir::new().unwrap();
    for (name, key, multiply) in [
        ("group-pass.psd", b"pass", true),
        ("group-isolated.psd", b"norm", false),
    ] {
        let mut child = Record::raster("Child multiply", [200, 100, 50]);
        child.key = *b"mul ";
        let mut folder = Record::folder("Folder", 1, Some(key));
        folder.opacity = 128;
        let bytes = fixture(&[
            Record::raster("Backdrop", [120, 180, 240]),
            Record::close(),
            child,
            folder,
        ]);
        let expected = blend_expected([120, 180, 240], [200, 100, 50], 128, multiply);
        export(name,&bytes,expected,"Backdrop + 128/255 folder containing Multiply child; Pass Through reaches backdrop, Normal isolates.",json!([{ "name":"Folder", "visible":true,"opacity":128.0/255.0,"blend":if multiply {"Pass Through"} else {"Normal"},"open":true }]));
        let path = write(&dir, name, &bytes);
        let doc = open(&path, 0, &report(&path)).unwrap();
        assert_eq!(pixels(&doc), expected);
        assert!(doc.layers[2].open);
    }
}

#[test]
fn nested_folder_ids_keep_individual_properties_and_gate_once_per_level() {
    let dir = TempDir::new().unwrap();
    let mut inner = Record::folder("Inner", 2, Some(b"mul "));
    inner.opacity = 128;
    let mut outer = Record::folder("Outer", 1, Some(b"norm"));
    outer.opacity = 192;
    let bytes = fixture(&[
        Record::raster("Backdrop", [120, 180, 240]),
        Record::close(),
        Record::raster("Inside base", [180, 120, 60]),
        Record::close(),
        Record::raster("Art", [80, 160, 240]),
        inner,
        outer,
    ]);
    // Keep floating coverage through BOTH group gates; only final output rounds.
    let mut expected = [0, 0, 0, 255];
    for c in 0..3 {
        let b: f64 = [120., 180., 240.][c];
        let base = [180., 120., 60.][c];
        let art = [80., 160., 240.][c];
        let inside = base * (1. - 128. / 255.) + base * art / 255. * 128. / 255.;
        expected[c] = (b * (1. - 192. / 255.) + inside * 192. / 255.).round() as u8;
    }
    export("group-nested.psd",&bytes,expected,"Rows Backdrop0, Inside base1, Art2, Inner1 (closed Multiply 128/255), Outer0 (open Normal 192/255). Inner gates over Inside base; Outer isolates then gates over Backdrop.",json!([{ "name":"Inner", "visible":true,"opacity":128.0/255.0,"blend":"Multiply","open":false },{ "name":"Outer", "visible":true,"opacity":192.0/255.0,"blend":"Normal","open":true }]));
    let path = write(&dir, "nested.psd", &bytes);
    let doc = open(&path, 0, &report(&path)).unwrap();
    assert_eq!(pixels(&doc), expected);
    assert_eq!(
        doc.layers.iter().map(|l| l.depth).collect::<Vec<_>>(),
        [0, 1, 2, 1, 0]
    );
    assert_eq!(doc.layers[3].name, "Inner");
    assert_eq!(doc.layers[3].opacity, 128. / 255.);
    assert!(!doc.layers[3].open);
    assert_eq!(doc.layers[4].name, "Outer");
    assert_eq!(doc.layers[4].opacity, 192. / 255.);
    assert!(doc.layers[4].open);
    assert_eq!(pixels(&save_reopen(&dir, &doc)), expected);
}

#[test]
fn absent_optional_lsct_blend_uses_record_key_and_flat_defaults_remain() {
    let dir = TempDir::new().unwrap();
    for key in [*b"norm", *b"pass", *b"mul "] {
        let mut folder = Record::folder("Folder", 1, None);
        folder.key = key;
        let path = write(
            &dir,
            "minimal.psd",
            &fixture(&[Record::close(), Record::raster("Art", [9, 19, 29]), folder]),
        );
        let doc = open(&path, 0, &report(&path)).unwrap();
        assert_eq!(
            doc.layers[1].blend,
            match &key {
                b"norm" => BlendMode::Normal,
                b"pass" => BlendMode::PassThrough,
                _ => BlendMode::Multiply,
            }
        );
        assert_eq!(doc.layers[1].opacity, 1.);
        assert!(doc.layers[1].visible);
    }
    let path = write(
        &dir,
        "flat.psd",
        &fixture(&[Record::raster("Plain", [9, 19, 29])]),
    );
    let doc = open(&path, 0, &report(&path)).unwrap();
    assert_eq!(pixels(&doc), [9, 19, 29, 255]);
    assert_eq!(doc.layers[0].blend, BlendMode::Normal);
    assert_eq!(doc.layers[0].depth, 0);
}

#[test]
fn unsupported_group_options_remain_explicit_losses() {
    let dir = TempDir::new().unwrap();
    let mut folder = Record::folder("Folder", 1, Some(b"pass"));
    folder.clipped = 1;
    folder.mask = true;
    // Extend lsct to its documented scene subtype; ordinary folder semantics
    // can be retained, but this unsupported timeline state must be disclosed.
    folder.blocks[8..12].copy_from_slice(&16u32.to_be_bytes());
    folder.blocks.extend(1u32.to_be_bytes());
    let path = write(
        &dir,
        "scene.psd",
        &fixture(&[Record::close(), Record::raster("Art", [9, 19, 29]), folder]),
    );
    let report = report(&path);
    assert_eq!(report["layered_available"], true);
    assert!(report["verified_losses"]
        .to_string()
        .contains("scene subtype"));
    assert!(report["verified_losses"]
        .to_string()
        .contains("raster mask record/channel"));
    assert!(report["general_limitations"]
        .to_string()
        .contains("Layer/group masks"));
    assert!(report["general_limitations"]
        .to_string()
        .contains("Layer styles"));
    // An 8-bit import retains the folder's clipping byte.
    let doc = open(&path, 0, &report).unwrap();
    let folder = doc
        .layers
        .iter()
        .find(|l| l.kind == LayerKind::Group)
        .expect("folder");
    assert!(folder.clipped, "the clipped folder is imported clipped");
}

fn assert_layer_refusal(dir: &TempDir, name: &str, bytes: &[u8], reason: &str) {
    let path = write(dir, name, bytes);
    let report = report(&path);
    assert_eq!(report["layered_available"], false, "{name}: {report}");
    assert!(
        report["layered_refusal"].as_str().unwrap().contains(reason),
        "{name}: {report}"
    );
    assert_eq!(
        report["stored_composite_available"], true,
        "{name}: {report}"
    );
    let error = open(&path, 0, &report).err().unwrap();
    assert!(error.contains(reason), "{error}");
    assert!(!error.contains("internal error"), "{error}");
    assert_eq!(pixels(&open(&path, 1, &report).unwrap()), [17, 29, 43, 255]);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn malformed_group_records_refuse_layers_without_losing_valid_saved_composite() {
    let dir = TempDir::new().unwrap();
    let original = Record::folder("Folder", 1, Some(b"pass"));
    for (name, blocks, reason) in [
        (
            "unknown-blend.psd",
            block(
                b"lsct",
                &[0, 0, 0, 1, b'8', b'B', b'I', b'M', b'n', b'o', b'p', b'e'],
            ),
            "group blend key",
        ),
        (
            "duplicate.psd",
            [original.blocks.clone(), original.blocks.clone()].concat(),
            "duplicate",
        ),
        (
            "signature.psd",
            block(
                b"lsct",
                &[0, 0, 0, 1, b'b', b'a', b'd', b'!', b'p', b'a', b's', b's'],
            ),
            "blend signature",
        ),
        (
            "short.psd",
            block(b"lsct", &[0, 1]),
            "section-divider length",
        ),
        (
            "unknown-type.psd",
            block(b"lsct", &4u32.to_be_bytes()),
            "section-divider type",
        ),
        (
            "nested-divider.psd",
            block(b"lsdk", &1u32.to_be_bytes()),
            "nested section-divider",
        ),
    ] {
        let mut folder = original.clone();
        folder.blocks = blocks;
        assert_layer_refusal(
            &dir,
            name,
            &fixture(&[Record::close(), Record::raster("Art", [9, 19, 29]), folder]),
            reason,
        );
    }
    assert_layer_refusal(
        &dir,
        "unclosed.psd",
        &fixture(&[Record::raster("Art", [9, 19, 29]), original.clone()]),
        "missing its closing",
    );
    assert_layer_refusal(
        &dir,
        "underflow.psd",
        &fixture(&[Record::close(), Record::raster("Art", [9, 19, 29])]),
        "unmatched closing",
    );
    let mut bytes = fixture(&[
        Record::close(),
        Record::raster("Art", [9, 19, 29]),
        original,
    ]);
    // First record begins at 44: rectangle16 + channel count2 + first ID2.
    bytes[64..68].copy_from_slice(&1u32.to_be_bytes());
    assert_layer_refusal(&dir, "short-channel.psd", &bytes, "compression bytes");
}

#[test]
fn record_metadata_and_depth_limits_refuse_before_third_party_decode() {
    let dir = TempDir::new().unwrap();
    let mut bytes = fixture(&[Record::raster("Art", [9, 19, 29])]);
    bytes[42..44].copy_from_slice(&2049u16.to_be_bytes());
    assert_layer_refusal(&dir, "too-many.psd", &bytes, "too many layer records");
    let mut records = vec![Record::close(); 129];
    records.push(Record::raster("Art", [9, 19, 29]));
    records.extend(vec![Record::folder("Folder", 1, Some(b"pass")); 129]);
    assert_layer_refusal(&dir, "too-deep.psd", &fixture(&records), "128-level");
    let mut huge = Record::raster("Art", [9, 19, 29]);
    huge.blocks = block(b"skip", &[]).repeat(65_537);
    assert_layer_refusal(
        &dir,
        "too-many-blocks.psd",
        &fixture(&[huge]),
        "65536-block",
    );
    let too_many_rows = vec![Record::raster("Art", [9, 19, 29]); 1025];
    assert_layer_refusal(
        &dir,
        "too-many-rows.psd",
        &fixture(&too_many_rows),
        "1024-row",
    );
}

#[test]
fn malformed_lengths_and_stale_fingerprint_preserve_captured_native_state() {
    let dir = TempDir::new().unwrap();
    let bytes = fixture(&[
        Record::close(),
        Record::raster("Art", [9, 19, 29]),
        Record::folder("Folder", 1, Some(b"pass")),
    ]);
    let path = write(&dir, "source.psd", &bytes);
    let before = report(&path);
    let captured = open(&path, 0, &before).unwrap();
    let native = dir.path().join("before.rz");
    let mut err = ptr::null_mut();
    assert!(unsafe {
        dr_doc_save_native(
            &*captured,
            cpath(&native).as_ptr(),
            std::ptr::null(),
            &mut err,
        )
    });
    let original_native = std::fs::read(&native).unwrap();
    let mut malformed = bytes.clone();
    malformed[38..42].copy_from_slice(&u32::MAX.to_be_bytes());
    std::fs::write(&path, &malformed).unwrap();
    assert!(open(&path, 0, &before)
        .err()
        .unwrap()
        .contains("changed since"));
    assert_layer_refusal(
        &dir,
        "invalid-info.psd",
        &malformed,
        "truncated PSD layer record",
    );
    // A forged extra-data length was ignored by the dependency before this walk.
    let mut malformed_extra = bytes.clone();
    malformed_extra[98..102].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_layer_refusal(
        &dir,
        "invalid-extra.psd",
        &malformed_extra,
        "truncated PSD layer record",
    );
    assert_eq!(pixels(&captured), [9, 19, 29, 255]);
    assert!(unsafe {
        dr_doc_save_native(
            &*captured,
            cpath(&native).as_ptr(),
            std::ptr::null(),
            &mut err,
        )
    });
    assert_eq!(std::fs::read(&native).unwrap(), original_native);
}

#[test]
fn unicode_block_counts_cannot_borrow_bytes_from_following_blocks_or_channels() {
    let dir = TempDir::new().unwrap();
    let mut folder = Record::folder("Folder", 1, Some(b"pass"));
    folder.blocks = [block(b"luni", &8u32.to_be_bytes()), folder.blocks.clone()].concat();
    assert_layer_refusal(
        &dir,
        "escaped-unicode.psd",
        &fixture(&[Record::close(), Record::raster("Art", [9, 19, 29]), folder]),
        "Unicode name exceeds",
    );
    let mut folder = Record::folder("Folder", 1, Some(b"pass"));
    let mut name = 2u32.to_be_bytes().to_vec();
    name.extend([0, 0x41, 0x03, 0xa9]);
    folder.blocks.extend(block(b"luni", &name));
    let path = write(
        &dir,
        "unicode.psd",
        &fixture(&[Record::close(), Record::raster("Art", [9, 19, 29]), folder]),
    );
    let doc = open(&path, 0, &report(&path)).unwrap();
    assert_eq!(doc.layers[1].name, "AΩ");
}

#[test]
fn additional_block_lengths_and_channel_claims_are_bounded() {
    let dir = TempDir::new().unwrap();
    let mut odd = Record::raster("Art", [9, 19, 29]);
    odd.blocks = block(b"skip", &[0]);
    assert_layer_refusal(
        &dir,
        "odd-block.psd",
        &fixture(&[odd]),
        "odd PSD additional",
    );
    let mut truncated = Record::raster("Art", [9, 19, 29]);
    truncated.blocks = block(b"skip", &[]);
    truncated.blocks[8..12].copy_from_slice(&1000u32.to_be_bytes());
    assert_layer_refusal(
        &dir,
        "long-block.psd",
        &fixture(&[truncated]),
        "truncated PSD layer record",
    );
    let mut bytes = fixture(&[Record::raster("Art", [9, 19, 29])]);
    bytes[64..68].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_layer_refusal(
        &dir,
        "long-channel.psd",
        &bytes,
        "truncated PSD layer record",
    );
}

#[test]
fn sibling_and_empty_groups_with_duplicate_names_keep_distinct_folder_properties() {
    let dir = TempDir::new().unwrap();
    let mut bottom = Record::folder("Same", 2, Some(b"norm"));
    bottom.opacity = 91;
    bottom.flags = 10;
    let mut top = Record::folder("Same", 1, Some(b"scrn"));
    top.opacity = 173;
    let path = write(
        &dir,
        "siblings.psd",
        &fixture(&[
            Record::close(),
            Record::raster("Art", [9, 19, 29]),
            bottom,
            Record::close(),
            top,
        ]),
    );
    let doc = open(&path, 0, &report(&path)).unwrap();
    assert_eq!(doc.layers.len(), 3);
    assert_eq!(
        (
            doc.layers[1].opacity,
            doc.layers[1].blend,
            doc.layers[1].visible,
            doc.layers[1].open
        ),
        (91. / 255., BlendMode::Normal, false, false)
    );
    assert_eq!(
        (
            doc.layers[2].opacity,
            doc.layers[2].blend,
            doc.layers[2].visible,
            doc.layers[2].open
        ),
        (173. / 255., BlendMode::Screen, true, true)
    );
    assert_eq!(pixels(&doc), [0, 0, 0, 0]);
}

#[test]
fn record_and_block_limit_boundaries_remain_admitted_and_deep_loss_is_disclosed() {
    let dir = TempDir::new().unwrap();
    let mut records = vec![Record::close(); 128];
    records.push(Record::raster("Art", [9, 19, 29]));
    records.extend(vec![Record::folder("Folder", 1, Some(b"pass")); 128]);
    let path = write(&dir, "deep-boundary.psd", &fixture(&records));
    let r = report(&path);
    assert_eq!(r["layered_available"], true);
    assert!(r["verified_losses"].to_string().contains("deeper than 10"));
    assert_eq!(pixels(&open(&path, 0, &r).unwrap()), [9, 19, 29, 255]);
    let mut record = Record::raster("Art", [9, 19, 29]);
    record.blocks = block(b"skip", &[]).repeat(65_536);
    let path = write(&dir, "block-boundary.psd", &fixture(&[record]));
    assert_eq!(
        pixels(&open(&path, 0, &report(&path)).unwrap()),
        [9, 19, 29, 255]
    );
    let records = [Record::close(), Record::folder("Empty", 1, Some(b"pass"))]
        .into_iter()
        .cycle()
        .take(2048)
        .collect::<Vec<_>>();
    let path = write(&dir, "row-boundary.psd", &fixture(&records));
    assert_eq!(open(&path, 0, &report(&path)).unwrap().layers.len(), 1024);
}

#[test]
fn metadata_byte_cap_is_checked_before_decoder_and_keeps_composite_choice() {
    let dir = TempDir::new().unwrap();
    // One 64 MiB+2 extra-data field, with no decoder pixels allocated from it.
    // This stays below 400 MiB including builder/file/report overlap.
    let mut record = Record::raster("Art", [9, 19, 29]);
    record.blocks = block(b"skip", &vec![0; 64 * 1024 * 1024 - 22]);
    let bytes = fixture(&[record]);
    assert_layer_refusal(&dir, "metadata-cap.psd", &bytes, "64 MiB");
}
