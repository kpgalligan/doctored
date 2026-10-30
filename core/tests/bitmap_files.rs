//! #547 Bitmap phase 1c's 1-bit PNG, TIFF and BMP files through the FFI:
//! files the test writes byte by byte (13 × 3, so each row ends in three pad
//! bits) answer `dr_path_bitmap_source` and open as Bitmap documents with the
//! oracle's bits and resolution, whatever their pad bits hold; the files the
//! claim leaves to their ordinary open answer 0; the kept exports write the
//! headers the test parses (`bit_depth` 1) and reopen as the same plane, with
//! a preview of the same bits; JPEG goes through the Gray storage, every other
//! format refuses by sentence, and the plan is the declared shape. ImageMagick,
//! when installed, reads each export and writes `-monochrome` files that open
//! as Bitmap documents (SKIP otherwise).

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;

use doctored_core::ffi_bitmap::{dr_path_bitmap_source, dr_precision_doc_open_bitmap};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::json;
use tempfile::TempDir;

use crate::bitmap_mode::{bits, gray_doc, to_bitmap};
use crate::common::{cpath, take_err_string};
use crate::gray_fixture::{chunk, gray_icc, iccp, png};
use crate::indexed_fixture::{descriptor, export, export_plan, free, write_png, LIMIT};

const W: u32 = 13;
const H: u32 = 3;

/// The oracle: 1 white, 0 black, row-major; both colours reach the last
/// pixel of a row.
pub(crate) fn oracle() -> Vec<u8> {
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            u8::from((x * 7 + y * 3) % 5 < 2)
        })
        .collect()
}

/// `bits` (`w` a row) packed MSB first, a set bit `white_bit`'s value for
/// white; `pad_ones` sets every bit past the width.
pub(crate) fn pack(bits: &[u8], w: u32, white_bit: bool, pad_ones: bool) -> Vec<u8> {
    let line = (w as usize).div_ceil(8);
    let rows = bits.len() / w as usize;
    let mut out = vec![0u8; line * rows];
    for (y, row) in bits.chunks_exact(w as usize).enumerate() {
        for x in 0..line * 8 {
            let set = match row.get(x) {
                Some(&b) => (b == 1) == white_bit,
                None => pad_ones,
            };
            if set {
                out[y * line + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    out
}

/// A 1-bit gray PNG (colour type 0) of packed `rows`, `extra` chunks before
/// IDAT, filter 0.
fn png1(w: u32, h: u32, rows: &[u8], extra: &[Vec<u8>]) -> Vec<u8> {
    let line = (w as usize).div_ceil(8);
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[1, 0, 0, 0, 0]);
    let mut raw = Vec::new();
    for row in rows.chunks_exact(line) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    for c in extra {
        out.extend_from_slice(c);
    }
    out.extend(chunk(
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
    ));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// One TIFF directory entry: tag, type, count and its value bytes (inline
/// when four or fewer, else written after the directory).
type Entry = (u16, u16, u32, Vec<u8>);

fn short(tag: u16, v: u16) -> Entry {
    (tag, 3, 1, v.to_le_bytes().to_vec())
}

fn long(tag: u16, v: u32) -> Entry {
    (tag, 4, 1, v.to_le_bytes().to_vec())
}

/// A little-endian classic TIFF of one uncompressed strip of packed `rows`:
/// one 1-bit sample, `photometric`, and `extra` entries.
fn tiff1(w: u32, h: u32, photometric: u16, rows: &[u8], extra: Vec<Entry>) -> Vec<u8> {
    let mut entries = vec![
        long(256, w),
        long(257, h),
        short(258, 1),
        short(259, 1),
        short(262, photometric),
        long(273, 0),
        short(277, 1),
        long(278, h),
        long(279, rows.len() as u32),
        short(284, 1),
    ];
    entries.extend(extra);
    entries.sort_by_key(|e| e.0);
    let ifd_len = 2 + 12 * entries.len() + 4;
    let mut tail = Vec::new();
    let mut table = Vec::new();
    let data_at = 8 + ifd_len;
    let mut out_of_line = data_at + rows.len();
    for (tag, kind, count, value) in &entries {
        table.extend_from_slice(&tag.to_le_bytes());
        table.extend_from_slice(&kind.to_le_bytes());
        table.extend_from_slice(&count.to_le_bytes());
        if *tag == 273 {
            table.extend_from_slice(&(data_at as u32).to_le_bytes());
        } else if value.len() <= 4 {
            let mut field = value.clone();
            field.resize(4, 0);
            table.extend_from_slice(&field);
        } else {
            table.extend_from_slice(&(out_of_line as u32).to_le_bytes());
            tail.extend_from_slice(value);
            if tail.len() % 2 == 1 {
                tail.push(0);
            }
            out_of_line = data_at + rows.len() + tail.len();
        }
    }
    let mut out = b"II*\0".to_vec();
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&table);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(rows);
    out.extend_from_slice(&tail);
    out
}

/// A TIFF RATIONAL entry of `n / 1`.
fn rational(tag: u16, n: u32) -> Entry {
    let mut v = n.to_le_bytes().to_vec();
    v.extend_from_slice(&1u32.to_le_bytes());
    (tag, 5, 1, v)
}

/// A 1 bpp BMP (BITMAPINFOHEADER) of `bits`: `palette` (RGB) entries 0 and
/// 1, a bit 1 being entry 1; rows bottom-up, padded to four bytes with
/// `pad_ones` setting every bit past the width, padding bytes included.
fn bmp1(w: u32, h: u32, bits: &[u8], palette: [[u8; 3]; 2], ppm: u32, pad_ones: bool) -> Vec<u8> {
    let white_bit = palette[1] == [255, 255, 255];
    let packed = pack(bits, w, white_bit, pad_ones);
    let line = (w as usize).div_ceil(8);
    let padded = (w as usize).div_ceil(32) * 4;
    let offset = 14 + 40 + 8;
    let size = offset + padded * h as usize;
    let mut out = b"BM".to_vec();
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&((padded * h as usize) as u32).to_le_bytes());
    out.extend_from_slice(&(ppm as i32).to_le_bytes());
    out.extend_from_slice(&(ppm as i32).to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for [r, g, b] in palette {
        out.extend_from_slice(&[b, g, r, 0]);
    }
    for y in (0..h as usize).rev() {
        out.extend_from_slice(&packed[y * line..(y + 1) * line]);
        let fill = if pad_ones { 0xFF } else { 0 };
        out.extend(std::iter::repeat_n(fill, padded - line));
    }
    out
}

/// `dr_path_bitmap_source` of `path`.
pub(crate) fn source(path: &Path) -> i32 {
    let c = cpath(path);
    unsafe { dr_path_bitmap_source(c.as_ptr()) }
}

/// `dr_precision_doc_open_bitmap` of `path` under the gamma 2.2 working
/// Gray, or its error.
pub(crate) fn open(path: &Path) -> Result<*mut PrecisionDocument, String> {
    let c = cpath(path);
    let working = CString::new("gray_gamma_22").unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_open_bitmap(
            c.as_ptr(),
            working.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn written(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn ppi(doc: *const PrecisionDocument) -> (f64, f64) {
    let r = &descriptor(doc)["resolution"];
    (r["x_ppi"].as_f64().unwrap(), r["y_ppi"].as_f64().unwrap())
}

/// Opens `path` as a Bitmap document and checks its mode, its bits and its
/// resolution (within a thousandth of a pixel per inch: PNG and BMP store
/// pixels per metre).
fn assert_opens(path: &Path, expected: &[u8], resolution: (f64, f64)) {
    let doc = open(path).unwrap();
    let d = descriptor(doc);
    assert_eq!(d["color_mode"], json!("bitmap"), "{}", path.display());
    assert_eq!(
        (d["width"].clone(), d["height"].clone()),
        (json!(W), json!(H))
    );
    assert_eq!(bits(doc), expected, "{}", path.display());
    let (x, y) = ppi(doc);
    assert!(
        (x - resolution.0).abs() < 1e-3 && (y - resolution.1).abs() < 1e-3,
        "{}: {x} × {y}",
        path.display()
    );
    free(doc);
}

#[test]
fn each_file_opens_as_bitmap_with_the_oracle_bits_and_resolution() {
    let dir = TempDir::new().unwrap();
    let bits_ = oracle();
    // PNG: 1 white (PNG §7.2), pHYs 11811 per metre.
    let mut phys = 11811u32.to_be_bytes().to_vec();
    phys.extend_from_slice(&11811u32.to_be_bytes());
    phys.push(1);
    let png_path = written(
        &dir,
        "a.png",
        &png1(
            W,
            H,
            &pack(&bits_, W, true, false),
            &[chunk(b"pHYs", &phys)],
        ),
    );
    assert_eq!(source(&png_path), 1);
    assert_opens(&png_path, &bits_, (11811.0 * 0.0254, 11811.0 * 0.0254));
    // TIFF BlackIsZero (1 white) at 150 ppi, and WhiteIsZero (1 black).
    let res = || vec![rational(282, 150), rational(283, 150), short(296, 2)];
    let black_is_zero = written(
        &dir,
        "b.tif",
        &tiff1(W, H, 1, &pack(&bits_, W, true, false), res()),
    );
    assert_eq!(source(&black_is_zero), 2);
    assert_opens(&black_is_zero, &bits_, (150.0, 150.0));
    let white_is_zero = written(
        &dir,
        "w.tif",
        &tiff1(W, H, 0, &pack(&bits_, W, false, false), res()),
    );
    assert_eq!(source(&white_is_zero), 2);
    assert_opens(&white_is_zero, &bits_, (150.0, 150.0));
    // BMP with a white-first palette (a bit 0 is white), 3937 per metre.
    let bmp = written(
        &dir,
        "w.bmp",
        &bmp1(W, H, &bits_, [[255, 255, 255], [0, 0, 0]], 3937, false),
    );
    assert_eq!(source(&bmp), 3);
    assert_opens(&bmp, &bits_, (3937.0 * 0.0254, 3937.0 * 0.0254));
    // A BMP with no resolution is at 72 ppi; a Gray iCCP is kept.
    let plain = written(
        &dir,
        "k.bmp",
        &bmp1(W, H, &bits_, [[0, 0, 0], [255, 255, 255]], 0, false),
    );
    assert_opens(&plain, &bits_, (72.0, 72.0));
    let icc = gray_icc("Test Gray 1.8", 1.8);
    let tagged = written(
        &dir,
        "i.png",
        &png1(W, H, &pack(&bits_, W, true, false), &[iccp(&icc)]),
    );
    assert_eq!(source(&tagged), 1);
    let doc = open(&tagged).unwrap();
    assert_eq!(descriptor(doc)["gray"]["profile"], json!("Test Gray 1.8"));
    free(doc);
}

#[test]
fn pad_bits_set_in_the_file_open_as_the_oracle() {
    let dir = TempDir::new().unwrap();
    let bits_ = oracle();
    let png_path = written(
        &dir,
        "p.png",
        &png1(W, H, &pack(&bits_, W, true, true), &[]),
    );
    assert_opens(&png_path, &bits_, (72.0, 72.0));
    for (name, palette) in [
        ("black.bmp", [[0, 0, 0], [255, 255, 255]]),
        ("white.bmp", [[255, 255, 255], [0, 0, 0]]),
    ] {
        let path = written(&dir, name, &bmp1(W, H, &bits_, palette, 0, true));
        assert_opens(&path, &bits_, (72.0, 72.0));
    }
    let tiff = written(
        &dir,
        "p.tif",
        &tiff1(W, H, 0, &pack(&bits_, W, false, true), vec![]),
    );
    assert_opens(&tiff, &bits_, (72.0, 72.0));
}

#[test]
fn files_the_claim_leaves_alone_answer_zero() {
    let dir = TempDir::new().unwrap();
    let bits_ = oracle();
    let rows = pack(&bits_, W, true, false);
    let samples: Vec<u16> = bits_.iter().map(|&b| u16::from(b) * 255).collect();
    let mut cases = vec![
        ("gray8.png", png(W, H, 8, 0, 1, &samples, &[])),
        ("gray2.png", png(W, H, 2, 0, 1, &samples, &[])),
        ("trns.png", png1(W, H, &rows, &[chunk(b"tRNS", &[0, 1])])),
        (
            "rgb.bmp",
            bmp1(W, H, &bits_, [[255, 0, 0], [0, 0, 255]], 0, false),
        ),
        (
            "rgb-icc.tif",
            tiff1(
                W,
                H,
                1,
                &rows,
                vec![(
                    34675,
                    7,
                    IccProfile::srgb().bytes().len() as u32,
                    IccProfile::srgb().bytes().to_vec(),
                )],
            ),
        ),
        ("fill2.tif", tiff1(W, H, 1, &rows, vec![short(266, 2)])),
    ];
    let palette = dir.path().join("palette.png");
    write_png(
        &palette,
        (W, H),
        1,
        &[[0, 0, 0], [255, 255, 255]],
        None,
        &bits_,
    );
    cases.push(("palette.png", std::fs::read(&palette).unwrap()));
    for (name, bytes) in cases {
        let path = written(&dir, name, &bytes);
        assert_eq!(source(&path), 0, "{name}");
        assert!(open(&path).is_err(), "{name}");
    }
}

/// A Bitmap document of the oracle (Threshold of a 0/255 gray).
pub(crate) fn oracle_doc() -> *mut PrecisionDocument {
    let codes: Vec<u8> = oracle().iter().map(|&b| b * 255).collect();
    let gray = gray_doc(W, H, &codes);
    let bitmap = to_bitmap(gray, &json!({"method":"threshold"})).unwrap();
    free(gray);
    assert_eq!(bits(bitmap), oracle());
    bitmap
}

fn request(format: &str) -> String {
    json!({"format":format,"profile":"document","bit_depth":8}).to_string()
}

/// IFD0 of a classic TIFF, every value widened to u32.
fn tiff_tags(data: &[u8]) -> std::collections::HashMap<u16, Vec<u32>> {
    let big = &data[..2] == b"MM";
    let u16_at = |at: usize| {
        let b = [data[at], data[at + 1]];
        if big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        }
    };
    let u32_at = |at: usize| {
        let b = [data[at], data[at + 1], data[at + 2], data[at + 3]];
        if big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        }
    };
    assert_eq!(u16_at(2), 42);
    let ifd = u32_at(4) as usize;
    let mut tags = std::collections::HashMap::new();
    for i in 0..usize::from(u16_at(ifd)) {
        let at = ifd + 2 + 12 * i;
        let (tag, kind, count) = (u16_at(at), u16_at(at + 2), u32_at(at + 4) as usize);
        let size = match kind {
            3 => 2,
            4 => 4,
            5 => 8,
            _ => 1,
        };
        let base = if size * count <= 4 {
            at + 8
        } else {
            u32_at(at + 8) as usize
        };
        let values = (0..count)
            .map(|k| match kind {
                3 => u32::from(u16_at(base + 2 * k)),
                4 => u32_at(base + 4 * k),
                5 => u32_at(base + 8 * k),
                _ => u32::from(data[base + k]),
            })
            .collect();
        tags.insert(tag, values);
    }
    tags
}

/// The preview's pixels as bits: white or black, opaque; any other fails.
fn preview_bits(preview: &[[u8; 4]]) -> Vec<u8> {
    preview
        .iter()
        .map(|p| match p {
            [255, 255, 255, 255] => 1,
            [0, 0, 0, 255] => 0,
            other => panic!("not a Bitmap preview pixel: {other:?}"),
        })
        .collect()
}

#[test]
fn kept_png_tiff_and_bmp_exports_write_the_plane_and_reopen_equal() {
    let dir = TempDir::new().unwrap();
    let doc = oracle_doc();
    let plane = pack(&oracle(), W, true, false);
    for format in ["png", "tiff", "bmp"] {
        let (bytes, info, preview) = export(doc, &request(format)).unwrap();
        assert_eq!(
            (info["bit_depth"].clone(), info["format"].clone()),
            (json!(1), json!(format)),
            "{format}"
        );
        assert_eq!(preview_bits(&preview), oracle(), "{format} preview");
        match format {
            "png" => {
                assert_eq!(&bytes[12..16], b"IHDR");
                assert_eq!(
                    (
                        u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
                        u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
                        bytes[24],
                        bytes[25]
                    ),
                    (W, H, 1, 0),
                    "png IHDR: depth 1, colour type 0"
                );
                let mut decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
                decoder.set_transformations(png::Transformations::IDENTITY);
                let mut reader = decoder.read_info().unwrap();
                let mut rows = vec![0u8; reader.output_buffer_size().unwrap()];
                reader.next_frame(&mut rows).unwrap();
                assert_eq!(rows, plane, "png rows");
            }
            "tiff" => {
                let tags = tiff_tags(&bytes);
                let one = |tag: u16| tags.get(&tag).map(|v| v[0]);
                assert_eq!(
                    (one(256), one(257), one(258), one(259), one(262), one(277)),
                    (Some(W), Some(H), Some(1), Some(1), Some(1), Some(1)),
                    "tiff: 1 bit, uncompressed, BlackIsZero, one sample"
                );
                let (offset, count) = (one(273).unwrap() as usize, one(279).unwrap() as usize);
                assert_eq!(&bytes[offset..offset + count], &plane[..], "tiff strip");
                assert!(tags.contains_key(&34675), "the Gray profile is embedded");
            }
            _ => {
                assert_eq!(&bytes[..2], b"BM");
                let le16 = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
                let le32 = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                assert_eq!(
                    (le32(14), le32(18), le32(22), le16(26), le16(28), le32(30)),
                    (40, W, H, 1, 1, 0),
                    "bmp header: 1 bpp, BI_RGB, bottom-up"
                );
                assert_eq!(
                    &bytes[54..62],
                    &[0, 0, 0, 0, 255, 255, 255, 0],
                    "black, white"
                );
                let offset = le32(10) as usize;
                let line = (W as usize).div_ceil(8);
                for y in 0..H as usize {
                    let at = offset + (H as usize - 1 - y) * 4;
                    assert_eq!(&bytes[at..at + line], &plane[y * line..(y + 1) * line]);
                }
            }
        }
        let path = written(&dir, &format!("kept.{format}"), &bytes);
        assert_ne!(source(&path), 0, "{format}");
        let reopened = open(&path).unwrap();
        assert_eq!(bits(reopened), oracle(), "{format} reopens equal");
        assert_eq!(descriptor(reopened)["color_mode"], json!("bitmap"));
        free(reopened);
    }
    free(doc);
}

#[test]
fn jpeg_goes_through_gray_and_every_other_format_refuses_by_sentence() {
    let doc = oracle_doc();
    let (_, info, preview) = export(doc, &request("jpeg")).unwrap();
    assert_eq!(
        (info["bit_depth"].clone(), info["format"].clone()),
        (json!(8), json!("jpeg"))
    );
    assert_eq!(preview.len(), (W * H) as usize);
    // An RGB profile exports RGB from the storage, the plane's render.
    let srgb = json!({"format":"png","profile":"srgb","bit_depth":8}).to_string();
    let (_, info, preview) = export(doc, &srgb).unwrap();
    assert_eq!(info["bit_depth"], json!(8));
    assert_eq!(preview_bits(&preview), oracle());
    for (format, name) in [("gif", "gif"), ("webp", "webp")] {
        let sentence = format!(
            "A Bitmap document exports a 1-bit PNG, TIFF or BMP, or JPEG through its 8-bit \
             Grayscale render, with profile document; {name} has neither. Choose profile srgb, \
             display-p3 or file to export RGB."
        );
        assert_eq!(export(doc, &request(format)).unwrap_err(), sentence);
        assert_eq!(export_plan(doc, &request(format)).unwrap_err(), sentence);
    }
    let depth = "A Bitmap export to PNG, TIFF or BMP with profile document writes 1 bit a pixel, \
                 so it takes bit_depth 8; choose profile srgb, display-p3 or file for a deeper RGB \
                 copy.";
    let sixteen = json!({"format":"png","profile":"document","bit_depth":16}).to_string();
    assert_eq!(export(doc, &sixteen).unwrap_err(), depth);
    assert_eq!(export_plan(doc, &sixteen).unwrap_err(), depth);
    let size =
        "A Bitmap export to PNG, TIFF or BMP with profile document keeps every pixel, so it \
                takes no size or sharpen; choose profile srgb, display-p3 or file to export a \
                resized or sharpened RGB copy.";
    let resized = json!({"format":"tiff","profile":"document","bit_depth":8,
                         "size":{"mode":"width","value":26}})
    .to_string();
    assert_eq!(export(doc, &resized).unwrap_err(), size);
    // The kept plan: 2 B a pixel and bookkeeping.
    for format in ["png", "tiff", "bmp"] {
        let plan = export_plan(doc, &request(format)).unwrap();
        assert_eq!(
            plan["scratch_bytes"],
            json!(u64::from(W * H) * 2 + 1024 * 1024),
            "{format}"
        );
        assert_eq!(
            (plan["width"].clone(), plan["height"].clone()),
            (json!(W), json!(H))
        );
    }
    free(doc);
}

/// ImageMagick's `magick`, when installed.
pub(crate) fn magick() -> Option<PathBuf> {
    let found = Command::new("magick").arg("-version").output().ok()?;
    found.status.success().then(|| PathBuf::from("magick"))
}

/// ImageMagick's 8-bit gray reading of `path` (`gray:-`), one byte a pixel.
pub(crate) fn magick_gray(path: &Path) -> Vec<u8> {
    let out = Command::new("magick")
        .arg(format!("{}[0]", path.display()))
        .args(["-depth", "8", "gray:-"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}: {}",
        path.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

#[test]
fn imagemagick_reads_the_exports_and_its_monochrome_files_open_as_bitmap() {
    if magick().is_none() {
        eprintln!("SKIP: ImageMagick (magick) is not installed");
        return;
    }
    let dir = TempDir::new().unwrap();
    let doc = oracle_doc();
    let expected: Vec<u8> = oracle().iter().map(|&b| b * 255).collect();
    for format in ["png", "tiff", "bmp"] {
        let (bytes, _, _) = export(doc, &request(format)).unwrap();
        let path = written(&dir, &format!("kept.{format}"), &bytes);
        assert_eq!(
            magick_gray(&path),
            expected,
            "ImageMagick reads the {format}"
        );
    }
    free(doc);
    // A ramp ImageMagick turns to black and white itself. `-depth 1` makes
    // its TIFF writer store one bit a sample, as its PNG and BMP writers do
    // for a two-colour image on their own (it keeps the source's 8 bits in a
    // TIFF otherwise).
    let mut pgm = format!("P5\n{W} {H}\n255\n").into_bytes();
    pgm.extend((0..W * H).map(|i| ((i % W) * 20 + (i / W) * 7) as u8));
    let ramp = written(&dir, "ramp.pgm", &pgm);
    for (name, value) in [("m.png", 1), ("m.tif", 2), ("m.bmp", 3)] {
        let path = dir.path().join(name);
        let status = Command::new("magick")
            .arg(&ramp)
            .args(["-monochrome", "-depth", "1"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success(), "{name}");
        assert_eq!(source(&path), value, "{name}");
        let theirs: Vec<u8> = magick_gray(&path)
            .iter()
            .map(|&g| u8::from(g >= 128))
            .collect();
        let doc = open(&path).unwrap();
        assert_eq!(descriptor(doc)["color_mode"], json!("bitmap"), "{name}");
        assert_eq!(bits(doc), theirs, "{name}");
        free(doc);
    }
}

#[test]
fn an_open_reports_its_error_by_path() {
    let dir = TempDir::new().unwrap();
    let path = written(&dir, "x.png", b"not a png");
    let err = open(&path).unwrap_err();
    assert!(err.contains("x.png"), "{err}");
    let missing = CString::new(dir.path().join("none.png").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { dr_path_bitmap_source(missing.as_ptr()) }, 0);
}
