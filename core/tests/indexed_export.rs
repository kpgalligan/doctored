//! #547 Indexed Color phase 1b's kept export, through the FFI: PNG, GIF and
//! TIFF of an Indexed document with profile `document` reopen
//! (`dr_precision_doc_open_indexed`) with every index, the palette's entries
//! and the transparent index, against the document's own plane; the PNG's
//! `PLTE`/`tRNS` and the GIF's table are read back raw (the stated black
//! padding after the palette), the TIFF's ColorMap by a hand-written
//! directory walk; a transparent index no pixel uses is not a TIFF's; the
//! kept refusals by their sentences; a JPEG and an sRGB PNG export RGB; the
//! TIFF's preview decodes its palette.

use serde_json::{json, Value};
use tempfile::TempDir;

use crate::indexed_fixture::{
    descriptor, export, export_plan, free, indices, layer16, opaque, open_indexed, rgb_doc,
    to_indexed, write_png,
};
use doctored_core::precision_document::PrecisionDocument;

/// The five opaque colours of [`indexed_doc`], its Exact palette's order.
const COLORS: [[u8; 3]; 5] = [
    [200, 10, 10],
    [10, 200, 10],
    [10, 10, 200],
    [50, 60, 70],
    [255, 255, 0],
];

/// A 4×2 Indexed document: Exact, Dither None, one clear pixel, so the
/// palette is COLORS then the white transparent entry (index 5) and the
/// plane is [0, 1, 2, 3, 4, 0, 5, 2].
fn indexed_doc() -> *mut PrecisionDocument {
    let mut pixels = opaque(&[
        COLORS[0], COLORS[1], COLORS[2], COLORS[3], COLORS[4], COLORS[0],
    ]);
    pixels.push([0, 0, 0, 0]);
    pixels.push([10, 10, 200, 255]);
    let rgb = rgb_doc(4, 2, &pixels);
    let doc = to_indexed(rgb, &json!({"palette":"exact","dither":"none"})).unwrap();
    free(rgb);
    assert_eq!(indices(doc), vec![0, 1, 2, 3, 4, 0, 5, 2]);
    doc
}

fn palette_bytes() -> Vec<u8> {
    let mut out: Vec<u8> = COLORS.concat();
    out.extend([255, 255, 255]);
    out
}

/// Every PNG chunk's type and data, in order (a test-side reader).
fn png_chunks(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let mut out = Vec::new();
    let mut at = 8;
    while at + 8 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = bytes[at + 4..at + 8].try_into().unwrap();
        out.push((kind, bytes[at + 8..at + 8 + length].to_vec()));
        at += 12 + length;
    }
    out
}

/// A little-endian classic TIFF's first-directory SHORT array `tag`.
fn tiff_shorts(bytes: &[u8], tag: u16) -> Option<Vec<u16>> {
    assert_eq!(&bytes[..4], b"II*\0");
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let ifd = u32_at(4);
    for i in 0..usize::from(u16_at(ifd)) {
        let entry = ifd + 2 + i * 12;
        if u16_at(entry) == tag {
            assert_eq!(u16_at(entry + 2), 3, "a SHORT array");
            let count = u32_at(entry + 4);
            let start = if count <= 2 {
                entry + 8
            } else {
                u32_at(entry + 8)
            };
            return Some((0..count).map(|k| u16_at(start + 2 * k)).collect());
        }
    }
    None
}

/// `bytes` written to `dir` and reopened as an Indexed document.
fn reopened(dir: &TempDir, name: &str, bytes: &[u8]) -> *mut PrecisionDocument {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    open_indexed(&path).unwrap()
}

fn kept(format: &str) -> String {
    json!({"format": format, "profile": "document", "dither": "none"}).to_string()
}

#[test]
fn a_kept_png_keeps_its_palette_transparent_index_and_every_index() {
    let dir = TempDir::new().unwrap();
    let doc = indexed_doc();
    let (bytes, info, preview) = export(doc, &kept("png")).unwrap();
    assert_eq!(info["bit_depth"], json!(8));
    assert_eq!(info["format"], json!("png"));
    let chunks = png_chunks(&bytes);
    let ihdr = &chunks[0].1;
    assert_eq!(
        (ihdr[8], ihdr[9]),
        (4, 3),
        "colour type 3 at 4 bits (6 entries)"
    );
    let chunk = |kind: &[u8; 4]| {
        chunks
            .iter()
            .find(|(k, _)| k == kind)
            .map(|(_, d)| d.clone())
    };
    assert_eq!(chunk(b"PLTE").unwrap(), palette_bytes());
    assert_eq!(chunk(b"tRNS").unwrap(), vec![255, 255, 255, 255, 255, 0]);
    let back = reopened(&dir, "kept.png", &bytes);
    assert_eq!(
        descriptor(back)["indexed"],
        json!({"colors": 6, "transparent_index": 5})
    );
    assert_eq!(indices(back), indices(doc));
    assert_eq!(
        layer16(back),
        layer16(doc),
        "every entry used, byte for byte"
    );
    assert_eq!(
        preview[6][3], 0,
        "the preview is clear at the transparent index"
    );
    free(back);
    free(doc);
}

#[test]
fn a_kept_gif_pads_its_table_with_black_to_a_power_of_two() {
    let dir = TempDir::new().unwrap();
    let doc = indexed_doc();
    let (bytes, info, _) = export(doc, &kept("gif")).unwrap();
    assert_eq!(info["format"], json!("gif"));
    assert_eq!(&bytes[..6], b"GIF89a");
    // A global table of 2^(2 + 1) = 8 entries: the palette, then black.
    assert_eq!(bytes[10] & 0x87, 0x82);
    let mut table = palette_bytes();
    table.extend([0; 6]);
    assert_eq!(&bytes[13..13 + 24], &table[..]);
    let back = reopened(&dir, "kept.gif", &bytes);
    assert_eq!(
        descriptor(back)["indexed"],
        json!({"colors": 8, "transparent_index": 5})
    );
    assert_eq!(indices(back), indices(doc));
    assert_eq!(layer16(back), layer16(doc));
    free(back);
    free(doc);
}

#[test]
fn a_kept_tiff_keeps_every_index_and_its_colour_map_and_previews_its_palette() {
    let dir = TempDir::new().unwrap();
    let doc = indexed_doc();
    let (bytes, info, preview) = export(doc, &kept("tiff")).unwrap();
    assert_eq!(info["format"], json!("tiff"));
    assert_eq!(tiff_shorts(&bytes, 262), Some(vec![3]), "Photometric 3");
    assert_eq!(tiff_shorts(&bytes, 258), Some(vec![8, 8]));
    assert_eq!(tiff_shorts(&bytes, 338), Some(vec![2]), "straight alpha");
    let map = tiff_shorts(&bytes, 320).unwrap();
    assert_eq!(map.len(), 768);
    let palette = palette_bytes();
    for c in 0..3 {
        for i in 0..256 {
            let want = palette.get(i * 3 + c).map_or(0, |&v| u16::from(v) * 257);
            assert_eq!(map[c * 256 + i], want, "channel {c} entry {i}");
        }
    }
    let back = reopened(&dir, "kept.tif", &bytes);
    assert_eq!(
        descriptor(back)["indexed"],
        json!({"colors": 256, "transparent_index": 5})
    );
    assert_eq!(indices(back), indices(doc));
    assert_eq!(layer16(back), layer16(doc));
    free(back);
    // The preview is the palette's colours, clear at the transparent index.
    let plane = [0usize, 1, 2, 3, 4, 0, 5, 2];
    for (pixel, &index) in preview.iter().zip(&plane) {
        let rgb = &palette[index * 3..index * 3 + 3];
        let alpha = if index == 5 { 0 } else { 255 };
        assert_eq!(*pixel, [rgb[0], rgb[1], rgb[2], alpha]);
    }
    free(doc);
}

#[test]
fn a_transparent_index_no_pixel_uses_is_not_recorded_by_a_tiff() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("unused.png");
    write_png(
        &path,
        (2, 1),
        8,
        &[[1, 2, 3], [4, 5, 6], [7, 8, 9]],
        Some(&[255, 0]),
        &[0, 2],
    );
    let doc = open_indexed(&path).unwrap();
    assert_eq!(descriptor(doc)["indexed"]["transparent_index"], json!(1));
    let (png, _, _) = export(doc, &kept("png")).unwrap();
    let back = reopened(&dir, "unused-back.png", &png);
    assert_eq!(descriptor(back)["indexed"]["transparent_index"], json!(1));
    free(back);
    let (tiff, _, _) = export(doc, &kept("tiff")).unwrap();
    let back = reopened(&dir, "unused-back.tif", &tiff);
    assert_eq!(
        descriptor(back)["indexed"]["transparent_index"],
        json!(null)
    );
    assert_eq!(indices(back), vec![0, 2]);
    free(back);
    free(doc);
}

#[test]
fn a_kept_export_refuses_another_depth_a_size_and_a_sharpen() {
    let doc = indexed_doc();
    let depth = "An Indexed Color export with profile document keeps the palette at 8 bits; \
                 choose bit_depth 8, or profile srgb, display-p3 or file to export RGB.";
    let size = "An Indexed Color export with profile document keeps every index, so it takes \
                no size or sharpen; choose profile srgb, display-p3 or file to export a resized \
                or sharpened RGB copy.";
    let cases: [(Value, &str); 3] = [
        (
            json!({"format":"png","profile":"document","bit_depth":16}),
            depth,
        ),
        (
            json!({"format":"tiff","profile":"document","size":{"mode":"width","value":2}}),
            size,
        ),
        (
            json!({"format":"gif","profile":"document",
                   "sharpen":{"radius":1.5,"amount":1.0,"threshold":0.0}}),
            size,
        ),
    ];
    for (request, sentence) in cases {
        let request = request.to_string();
        assert_eq!(export(doc, &request).unwrap_err(), sentence, "{request}");
        assert_eq!(
            export_plan(doc, &request).unwrap_err(),
            sentence,
            "{request}"
        );
    }
    // The canvas's own width is no resize.
    let same = json!({"format":"png","profile":"document","size":{"mode":"width","value":4}});
    assert!(export(doc, &same.to_string()).is_ok());
    let plan = export_plan(doc, &kept("png")).unwrap();
    assert_eq!(
        (plan["width"].clone(), plan["height"].clone()),
        (json!(4), json!(2))
    );
    assert!(plan["scratch_bytes"].as_u64().unwrap() > 0);
    free(doc);
}

#[test]
fn every_other_request_exports_rgb() {
    let doc = indexed_doc();
    let (bytes, info, preview) = export(
        doc,
        &json!({"format":"jpeg","profile":"document"}).to_string(),
    )
    .unwrap();
    assert_eq!(
        (info["format"].clone(), info["bit_depth"].clone()),
        (json!("jpeg"), json!(8))
    );
    assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    assert_eq!(preview.len(), 8);
    // An sRGB PNG is the RGB render, not a palette file.
    let (bytes, _, _) = export(doc, &json!({"format":"png","profile":"srgb"}).to_string()).unwrap();
    assert_ne!(png_chunks(&bytes)[0].1[9], 3, "not colour type 3");
    free(doc);
}
