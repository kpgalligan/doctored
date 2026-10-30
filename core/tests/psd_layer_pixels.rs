//! Independent PSD byte fixtures qualify the FFI's complete straight RGBA
//! storage, signed placement, native persistence, refusal and input purity.
//! DR_PHASE10A_PIXEL_FIXTURES exports authored fixtures/manifests for Release MCP.
use crate::common;
use common::{cpath, flat_pixels, take_err_string};
use doctored_core::doc::{DrDocument, LayerKind};
use doctored_core::ffi_doc::{dr_doc_open, dr_doc_save_native, dr_doc_with_layer_offset};
use doctored_core::ffi_psd::{dr_doc_open_psd, dr_psd_import_report};
use serde_json::{json, Value};
use std::ffi::CString;
use std::path::Path;
use std::ptr;
use tempfile::TempDir;

#[derive(Clone)]
struct Record {
    bounds: [i32; 4],
    channels: Vec<(i16, Vec<u8>)>,
    name: &'static str,
    divider: u32,
    opacity: u8,
    flags: u8,
    extra: Vec<u8>,
}
fn raw(samples: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0, 0];
    bytes.extend(samples);
    bytes
}
fn rows(rows: &[&[u8]]) -> Vec<u8> {
    let mut bytes = vec![0, 1];
    for row in rows {
        bytes.extend((row.len() as u16).to_be_bytes());
    }
    for row in rows {
        bytes.extend(*row);
    }
    bytes
}
fn rle(samples: &[u8], width: usize) -> Vec<u8> {
    // Test encoder deliberately emits one literal packet and a legal no-op
    // per row. Repeated packets are separately hand-authored below.
    let encoded: Vec<Vec<u8>> = samples
        .chunks(width)
        .map(|row| {
            let mut bytes = vec![128, (row.len() - 1) as u8];
            bytes.extend(row);
            bytes
        })
        .collect();
    rows(&encoded.iter().map(Vec::as_slice).collect::<Vec<_>>())
}
impl Record {
    fn raster(bounds: [i32; 4], rgba: &[u8], color: u16, compress: [bool; 4], alpha: bool) -> Self {
        let w = (bounds[3] - bounds[1]) as usize;
        let ids: &[i16] = match (color, alpha) {
            (1, false) => &[0],
            (1, true) => &[0, -1],
            (_, false) => &[0, 1, 2],
            (_, true) => &[0, 1, 2, -1],
        };
        let channels = ids
            .iter()
            .map(|id| {
                let component = if *id == -1 { 3 } else { *id as usize };
                let samples: Vec<u8> = rgba.chunks_exact(4).map(|p| p[component]).collect();
                (
                    *id,
                    if compress[component] {
                        rle(&samples, w)
                    } else {
                        raw(&samples)
                    },
                )
            })
            .collect();
        Self {
            bounds,
            channels,
            name: "Pixels",
            divider: 0,
            opacity: 255,
            flags: 8,
            extra: vec![],
        }
    }
    fn folder(divider: u32) -> Self {
        Self {
            bounds: [0; 4],
            channels: vec![],
            name: "Duplicate",
            divider,
            opacity: if divider == 2 { 128 } else { 255 },
            flags: 8,
            extra: vec![],
        }
    }
    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in self.bounds {
            bytes.extend(value.to_be_bytes());
        }
        bytes.extend((self.channels.len() as u16).to_be_bytes());
        for (id, channel) in &self.channels {
            bytes.extend(id.to_be_bytes());
            bytes.extend((channel.len() as u32).to_be_bytes());
        }
        bytes.extend(b"8BIMnorm");
        bytes.extend([self.opacity, 0, self.flags, 0]);
        let mut extra = vec![0; 8];
        extra.push(self.name.len() as u8);
        extra.extend(self.name.as_bytes());
        while !extra.len().is_multiple_of(4) {
            extra.push(0);
        }
        if self.divider > 0 {
            extra.extend(b"8BIMlsct");
            extra.extend(4u32.to_be_bytes());
            extra.extend(self.divider.to_be_bytes());
        }
        extra.extend(&self.extra);
        bytes.extend((extra.len() as u32).to_be_bytes());
        bytes.extend(extra);
        bytes
    }
}
fn fixture(records: &[Record], color: u16) -> Vec<u8> {
    // PSD file order is bottom first; all channel bytes follow all records.
    let mut info = (records.len() as i16).to_be_bytes().to_vec();
    for record in records {
        info.extend(record.encode());
    }
    for record in records {
        for (_, bytes) in &record.channels {
            info.extend(bytes);
        }
    }
    if !info.len().is_multiple_of(2) {
        info.push(0);
    }
    let mut bytes = b"8BPS\0\x01\0\0\0\0\0\0".to_vec();
    bytes.extend((if color == 1 { 1u16 } else { 3 }).to_be_bytes());
    bytes.extend(2u32.to_be_bytes());
    bytes.extend(2u32.to_be_bytes());
    bytes.extend(8u16.to_be_bytes());
    bytes.extend(color.to_be_bytes());
    bytes.extend([0; 8]);
    bytes.extend(((info.len() + 8) as u32).to_be_bytes());
    bytes.extend((info.len() as u32).to_be_bytes());
    bytes.extend(info);
    bytes.extend([0; 6]); // global mask length, raw composite compression
    bytes.extend([13; 4]);
    if color == 3 {
        bytes.extend([27; 4]);
        bytes.extend([41; 4]);
    }
    bytes
}
fn report(path: &Path) -> Value {
    let mut error = ptr::null_mut();
    let result = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut error) };
    assert!(!result.is_null(), "{}", take_err_string(error));
    serde_json::from_str(&take_err_string(result)).unwrap()
}
fn open(path: &Path, mode: i32, notice: &Value) -> Result<Box<DrDocument>, String> {
    let fingerprint = CString::new(notice["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let result =
        unsafe { dr_doc_open_psd(cpath(path).as_ptr(), mode, fingerprint.as_ptr(), &mut error) };
    if result.is_null() {
        Err(take_err_string(error))
    } else {
        assert!(error.is_null());
        Ok(unsafe { Box::from_raw(result) })
    }
}
fn save(doc: &DrDocument, path: &Path) -> Vec<u8> {
    let mut error = ptr::null_mut();
    assert!(
        unsafe { dr_doc_save_native(doc, cpath(path).as_ptr(), &mut error) },
        "{}",
        take_err_string(error)
    );
    std::fs::read(path).unwrap()
}
fn reopen(path: &Path) -> Box<DrDocument> {
    let mut error = ptr::null_mut();
    let doc = unsafe { dr_doc_open(cpath(path).as_ptr(), &mut error) };
    assert!(!doc.is_null(), "{}", take_err_string(error));
    unsafe { Box::from_raw(doc) }
}
fn exported(name: &str, bytes: &[u8], expected: &Value) {
    if let Ok(directory) = std::env::var("DR_PHASE10A_PIXEL_FIXTURES") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(Path::new(&directory).join(format!("{name}.psd")), bytes).unwrap();
        std::fs::write(
            Path::new(&directory).join(format!("{name}.psd.json")),
            serde_json::to_vec_pretty(expected).unwrap(),
        )
        .unwrap();
    }
}
const FULL: [u8; 24] = [
    11, 21, 31, 0, 41, 51, 61, 128, 71, 81, 91, 255, 101, 111, 121, 255, 131, 141, 151, 64, 161,
    171, 181, 255,
];
const CROP: [u8; 16] = [
    41, 51, 61, 128, 71, 81, 91, 255, 131, 141, 151, 64, 161, 171, 181, 255,
];
const FOUR: [u8; 16] = [
    200, 30, 20, 255, 40, 80, 120, 128, 19, 23, 29, 0, 7, 11, 13, 255,
];
const MOVED: [u8; 16] = [
    200, 30, 20, 255, 40, 80, 120, 128, 0, 0, 0, 0, 7, 11, 13, 255,
];
const SAVED: [u8; 16] = [
    13, 27, 41, 255, 13, 27, 41, 255, 13, 27, 41, 255, 13, 27, 41, 255,
];

fn single_case(
    name: &str,
    record: Record,
    color: u16,
    rgba: &[u8],
    composite: &[u8],
    moved: Option<&[u8]>,
) {
    let bytes = fixture(std::slice::from_ref(&record), color);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.psd");
    std::fs::write(&path, &bytes).unwrap();
    let notice = report(&path);
    assert_eq!(notice["width"], 2);
    assert_eq!(notice["height"], 2);
    assert_eq!(notice["layered_available"], true, "{notice}");
    assert!(!notice["verified_losses"]
        .to_string()
        .contains("outside the canvas"));
    let doc = open(&path, 0, &notice).unwrap();
    let [top, left, bottom, right] = record.bounds;
    let check = |d: &DrDocument| {
        assert_eq!((d.width, d.height), (2, 2));
        assert_eq!(d.layers.len(), 1);
        let layer = &d.layers[0];
        assert_eq!(
            layer.pixels.dimensions(),
            ((right - left) as u32, (bottom - top) as u32)
        );
        assert_eq!(layer.offset, (left, top));
        assert_eq!(layer.pixels.contiguous_image().as_raw(), rgba);
        assert_eq!(flat_pixels(d), composite);
    };
    check(&doc);
    let native = dir.path().join("before.rz");
    let before = save(&doc, &native);
    let reopened = reopen(&native);
    check(&reopened);
    assert_eq!(save(&reopened, &dir.path().join("after.rz")), before);
    if let Some(expected) = moved {
        let moved = unsafe { dr_doc_with_layer_offset(&*reopened, 0, 0, 0) };
        assert!(!moved.is_null());
        let moved = unsafe { Box::from_raw(moved) };
        assert_eq!(moved.layers[0].pixels.contiguous_image().as_raw(), rgba);
        assert_eq!(flat_pixels(&*moved), expected);
        check(&doc);
        check(&reopened);
        assert_eq!(save(&reopened, &native), before);
    }
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let stored = if color == 1 {
        [13, 13, 13, 255].repeat(4)
    } else {
        SAVED.to_vec()
    };
    assert_eq!(flat_pixels(&*open(&path, 1, &notice).unwrap()), stored);
    let mut manifest = json!({"description":name, "canvas":[2,2], "layers":[{
        "name":"Pixels", "kind":"raster", "depth":0, "offset":[left,top],
        "size":[right-left,bottom-top], "rgba":rgba,"visible":true,"opacity":1.0,"blend":"normal","open":true
    }], "composite_rgba":composite,"stored_composite_rgba":stored});
    if let Some(expected) = moved {
        manifest["moved"] = json!({"layer":0,"offset":[0,0],"composite_rgba":expected});
    }
    exported(name, &bytes, &manifest);
}
#[test]
fn full_negative_raw_and_rle_rectangles_keep_hidden_rgb() {
    for compressed in [false, true] {
        single_case(
            if compressed {
                "negative_rle"
            } else {
                "negative_raw"
            },
            Record::raster([0, -1, 2, 2], &FULL, 3, [compressed; 4], true),
            3,
            &FULL,
            &CROP,
            None,
        );
    }
}
#[test]
fn wholly_offcanvas_and_rle_rectangles_ending_at_zero_can_return_to_canvas() {
    for (name, bounds) in [
        ("offcanvas_rle", [3, 4, 5, 6]),
        ("ending_zero_rle", [-2, -2, 0, 0]),
    ] {
        single_case(
            name,
            Record::raster(bounds, &FOUR, 3, [true; 4], true),
            3,
            &FOUR,
            &[0; 16],
            Some(&MOVED),
        );
    }
}
#[test]
fn absent_alpha_is_opaque_and_compression_can_differ_by_channel() {
    let small = [3, 5, 7, 255];
    let expected = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 5, 7, 255];
    single_case(
        "opaque_small",
        Record::raster([1, 1, 2, 2], &small, 3, [false; 4], false),
        3,
        &small,
        &expected,
        None,
    );
    let opaque = [
        3, 5, 7, 255, 11, 13, 17, 255, 19, 23, 29, 255, 31, 37, 41, 255,
    ];
    single_case(
        "opaque_raw",
        Record::raster([0, 0, 2, 2], &opaque, 3, [false; 4], false),
        3,
        &opaque,
        &opaque,
        None,
    );
    single_case(
        "mixed_channels",
        Record::raster([0, -1, 2, 2], &FULL, 3, [false, true, false, true], true),
        3,
        &FULL,
        &CROP,
        None,
    );
}
#[test]
fn grayscale_uses_channel_zero_and_preserves_alpha() {
    let gray = [
        7, 7, 7, 0, 80, 80, 80, 64, 130, 130, 130, 128, 240, 240, 240, 255,
    ];
    let expected = [80, 80, 80, 64, 0, 0, 0, 0, 240, 240, 240, 255, 0, 0, 0, 0];
    single_case(
        "grayscale_rle",
        Record::raster([0, -1, 2, 1], &gray, 1, [true; 4], true),
        1,
        &gray,
        &expected,
        None,
    );
    let opaque = [
        7, 7, 7, 255, 80, 80, 80, 255, 130, 130, 130, 255, 240, 240, 240, 255,
    ];
    single_case(
        "grayscale_opaque_raw",
        Record::raster([0, 0, 2, 2], &opaque, 1, [false; 4], false),
        1,
        &opaque,
        &opaque,
        None,
    );
}
#[test]
fn packbits_repeat_and_literal_packets_decode_exactly() {
    let rgba = [
        5, 10, 20, 255, 5, 11, 21, 128, 5, 12, 22, 0, 7, 30, 40, 1, 7, 31, 41, 2, 7, 32, 42, 3,
    ];
    let mut record = Record::raster([0, -1, 2, 2], &rgba, 3, [true; 4], true);
    record.channels[0].1 = rows(&[&[254, 5], &[128, 254, 7, 128]]);
    let expected = [5, 11, 21, 128, 0, 0, 0, 0, 7, 31, 41, 2, 7, 32, 42, 3];
    single_case("repeat_rle", record, 3, &rgba, &expected, None);
}
#[test]
fn signed_extremes_are_storage_offsets_and_never_decoder_arithmetic() {
    for bounds in [
        [i32::MIN, i32::MIN, i32::MIN + 2, i32::MIN + 2],
        [i32::MAX - 2, i32::MAX - 2, i32::MAX, i32::MAX],
    ] {
        single_case(
            "signed_extreme",
            Record::raster(bounds, &FOUR, 3, [true; 4], true),
            3,
            &FOUR,
            &[0; 16],
            Some(&MOVED),
        );
    }
}
#[test]
fn duplicate_names_and_folder_state_correlate_by_record_ordinal() {
    let mut first = Record::raster([-2, -2, 0, 0], &FOUR, 3, [true; 4], true);
    first.name = "Duplicate";
    let mut second = Record::raster([3, 4, 5, 6], &FOUR, 3, [false; 4], true);
    second.name = "Duplicate";
    let bytes = fixture(&[Record::folder(3), first, second, Record::folder(2)], 3);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("group.psd");
    std::fs::write(&path, &bytes).unwrap();
    let notice = report(&path);
    let doc = open(&path, 0, &notice).unwrap();
    let native = dir.path().join("group.rz");
    save(&doc, &native);
    for doc in [doc, reopen(&native)] {
        assert_eq!(doc.layers.len(), 3);
        assert_eq!(
            doc.layers.iter().map(|l| l.depth).collect::<Vec<_>>(),
            [1, 1, 0]
        );
        assert_eq!(doc.layers[0].offset, (-2, -2));
        assert_eq!(doc.layers[1].offset, (4, 3));
        for layer in &doc.layers[..2] {
            assert_eq!(layer.pixels.contiguous_image().as_raw(), &FOUR);
            assert_eq!(layer.name, "Duplicate");
        }
        assert_eq!(doc.layers[2].kind, LayerKind::Group);
        assert!(!doc.layers[2].open);
        assert_eq!(doc.layers[2].opacity, 128.0 / 255.0);
        assert!(doc.layers[2].visible);
        assert_eq!(flat_pixels(&*doc), [0; 16]);
    }
    let raster = |offset| json!({"name":"Duplicate","kind":"raster","depth":1,"offset":offset,"size":[2,2],"rgba":FOUR,"visible":true,"opacity":1.0,"blend":"normal","open":true});
    exported(
        "duplicate_group",
        &bytes,
        &json!({"description":"duplicate names, off-canvas children, closed isolated fractional-opacity folder","canvas":[2,2],"layers":[raster([-2,-2]),raster([4,3]),{"name":"Duplicate","kind":"group","depth":0,"offset":[0,0],"size":[1,1],"rgba":[0,0,0,0],"visible":true,"opacity":128.0/255.0,"blend":"normal","open":false}],"composite_rgba":vec![0;16],"stored_composite_rgba":SAVED}),
    );
}

fn refused(records: &[Record], expected: &str) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("refused.psd");
    let good = fixture(
        &[Record::raster([0, 0, 2, 2], &FOUR, 3, [false; 4], true)],
        3,
    );
    std::fs::write(&path, &good).unwrap();
    let prior_report = report(&path);
    let captured = open(&path, 0, &prior_report).unwrap();
    let native = dir.path().join("captured.rz");
    let captured_bytes = save(&captured, &native);
    let bytes = fixture(records, 3);
    std::fs::write(&path, &bytes).unwrap();
    let notice = report(&path);
    assert_eq!(notice["layered_available"], false, "{notice}");
    assert!(
        notice["layered_refusal"]
            .as_str()
            .unwrap()
            .contains(expected),
        "{notice}"
    );
    assert_eq!(notice["stored_composite_available"], true);
    assert!(open(&path, 0, &notice).is_err());
    assert_eq!(flat_pixels(&*open(&path, 1, &notice).unwrap()), SAVED);
    assert_eq!(save(&captured, &native), captured_bytes);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    if expected == "decoded width" {
        exported(
            "malformed_rle",
            &bytes,
            &json!({
                "description":"one RLE row is short, despite a channel total matching four pixels",
                "canvas":[2,2],"layered_available":false,"layered_refusal_contains":expected,
                "stored_composite_rgba":SAVED
            }),
        );
    }
}
#[test]
fn partial_missing_duplicate_and_unknown_channels_refuse() {
    let record = Record::raster([0, 0, 2, 2], &FOUR, 3, [false; 4], true);
    for index in 0..3 {
        let mut bad = record.clone();
        bad.channels.remove(index);
        refused(&[bad], "missing a required color");
    }
    for channel in [vec![0, 0], vec![0, 0, 9], vec![0, 0, 1, 2, 3, 4, 5]] {
        let mut bad = record.clone();
        bad.channels[0].1 = channel;
        refused(&[bad], "length mismatch");
    }
    let mut bad = record.clone();
    bad.channels.push(bad.channels[0].clone());
    refused(&[bad], "duplicate channel");
    let mut bad = record;
    bad.channels[0].0 = 7;
    refused(&[bad], "unknown or duplicate channel");
}
#[test]
fn malformed_rle_rows_tables_lengths_and_zip_refuse() {
    let record = Record::raster([0, 0, 2, 2], &FOUR, 3, [true; 4], true);
    let cases = [
        (vec![0, 1, 0], "row table"),
        (rows(&[&[2, 1, 2, 3], &[0, 1]]), "RLE row"), // spills a row despite correct total4
        (rows(&[&[0, 1], &[2, 1, 2, 3]]), "decoded width"),
        (rows(&[&[1, 9], &[1, 1, 2]]), "RLE row"), // truncated literal
        (rows(&[&[255], &[1, 1, 2]]), "RLE row"),  // truncated repeat
        (vec![0, 1, 0, 5, 0, 3, 1, 1, 2], "truncated"),
        (
            {
                let mut b = rows(&[&[1, 1, 2], &[1, 3, 4]]);
                b.push(128);
                b
            },
            "trailing",
        ),
        // ZIP is read at 8 bits, so a ZIP word over bytes that are
        // not a zlib stream refuses as a bad stream, and only a word outside
        // Adobe's table is an unsupported compression.
        (vec![0, 2, 1, 2], "invalid zlib header"),
        (vec![0, 4, 1, 2], "unsupported compression"),
    ];
    for (channel, expected) in cases {
        let mut bad = record.clone();
        bad.channels[0].1 = channel;
        refused(&[bad], expected);
    }
    let mut bad = record;
    bad.channels[0].1 = vec![0];
    refused(&[bad], "compression bytes");
}
#[test]
fn empty_reversed_huge_and_aggregate_rectangles_refuse_before_pixel_allocation() {
    let record = Record::raster([0, 0, 2, 2], &FOUR, 3, [false; 4], true);
    for bounds in [
        [0, 0, 0, 2],
        [0, 0, 2, 0],
        [0, 0, -1, 2],
        [0, 0, 2, -1],
        [i32::MIN, i32::MIN, i32::MAX, i32::MAX],
        [0, 0, 30_000, 30_000],
    ] {
        let mut bad = record.clone();
        bad.bounds = bounds;
        refused(&[bad], "rectangle");
    }
    let mut large = record;
    large.bounds = [0, 0, 10_000, 10_000];
    refused(&vec![large; 5], "aggregate layer pixels");
}
#[test]
fn differently_sized_mask_payload_is_dropped_without_shifting_raster_channels() {
    let mut record = Record::raster([-2, -2, 0, 0], &FOUR, 3, [true; 4], true);
    // Mask owns three rows, unlike the two-row raster. Its payload stays opaque.
    record
        .channels
        .insert(0, (-2, rows(&[&[0, 3], &[0, 4], &[0, 5]])));
    let bytes = fixture(&[record], 3);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("mask.psd");
    std::fs::write(&path, bytes).unwrap();
    let notice = report(&path);
    assert!(notice["verified_losses"]
        .to_string()
        .contains("raster mask channel"));
    let doc = open(&path, 0, &notice).unwrap();
    assert!(doc.layers[0].mask.is_none());
    assert_eq!(doc.layers[0].pixels.contiguous_image().as_raw(), &FOUR);
    assert_eq!(doc.layers[0].offset, (-2, -2));
}

#[test]
fn decode_envelope_includes_metadata_copies_before_any_rgba_allocation() {
    let mut large = Record::raster([0, 0, 2, 2], &FOUR, 3, [false; 4], true);
    large.bounds = [0, 0, 10_000, 10_000];
    let mut records = vec![large; 4];
    records[0].extra.extend(b"8BIMzzzz");
    records[0].extra.extend((8u32 * 1024 * 1024).to_be_bytes());
    records[0].extra.resize(12 + 8 * 1024 * 1024, 0);
    refused(&records, "2 GiB decode memory envelope");
}
#[test]
fn discarded_folder_channels_and_divider_masks_do_not_widen_silently() {
    let child = Record::raster([0, 0, 2, 2], &FOUR, 3, [false; 4], true);
    let mut folder = Record::folder(2);
    folder.channels.push((0, vec![0, 4]));
    refused(
        &[Record::folder(3), child.clone(), folder],
        "unsupported compression",
    );
    let mut close = Record::folder(3);
    close.channels.push((-2, vec![0, 0]));
    refused(&[close, child, Record::folder(2)], "bounding divider");
}

#[test]
fn unsupported_mask_codec_is_disclosed_and_does_not_enter_pixel_decoder() {
    let mut record = Record::raster([0, 0, 2, 2], &FOUR, 3, [true; 4], true);
    record.channels.insert(0, (-2, vec![0, 2, 99, 88, 77]));
    let bytes = fixture(&[record], 3);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("mask-zip.psd");
    std::fs::write(&path, &bytes).unwrap();
    let notice = report(&path);
    assert!(notice["verified_losses"]
        .to_string()
        .contains("raster mask channel"));
    let doc = open(&path, 0, &notice).unwrap();
    assert!(doc.layers[0].mask.is_none());
    assert_eq!(doc.layers[0].pixels.contiguous_image().as_raw(), &FOUR);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn metadata_view_does_not_bypass_classic_canvas_header_limits() {
    let record = Record::raster([0, 0, 2, 2], &FOUR, 3, [false; 4], true);
    let mut bytes = fixture(&[record], 3);
    bytes[14..18].copy_from_slice(&1u32.to_be_bytes());
    bytes[18..22].copy_from_slice(&30_001u32.to_be_bytes());
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("too-wide.psd");
    std::fs::write(&path, bytes).unwrap();
    let notice = report(&path);
    assert_eq!(notice["layered_available"], false);
    assert_eq!(notice["stored_composite_available"], false);
    assert!(notice["layered_refusal"]
        .as_str()
        .unwrap()
        .contains("30000-pixel"));
}
