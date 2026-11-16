//! The byte native open over the stream reader (13C-2b): `dr_doc_open`
//! reads a `.doctored` file through `native_stream::Reader` and decodes each
//! plane in the writer's form row by row into tiles (`png_rows8_read`), and
//! any other stored PNG whole, as before. Black-box through
//! `dr_doc_save_native` / `dr_doc_open`, with oracles that do not read the
//! file through the reader:
//! - a round trip reopens sample for sample equal to the document saved;
//! - a layer or channel PNG spliced over the writer's record in another form
//!   equals `image`'s own decode of that PNG, done here;
//! - a truncated file answers `Err` at every length, and a cut inside a layer
//!   PNG, a mask or a channel PNG names the document;
//! - a corrupt payload answers `Err` with its plane's sentence;
//! - a typed file keeps the RGBA8 reader's refusal;
//! - the chunk walk that tells the writer's form polls the job once every
//!   `job_control::EVERY_INTERVAL` (4096, restated) chunks: a layer PNG
//!   followed by many empty chunks past its IEND (still the writer's form,
//!   and bytes png's reader never reaches) opens equal to the document and
//!   adds exactly that many checkpoints to the same file without them, and
//!   a worker parked at the walk's last poll and cancelled answers exactly
//!   `operation cancelled`;
//! - a typed file holding an 8-bit embedded source (whose PNG the same walk
//!   reads) answers exactly `operation cancelled` when parked and cancelled
//!   at each of its open's checkpoints.

use doctored_core::doc::{DrDocument, LayerKind, MaskKind};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_color::{dr_builtin_profile, dr_builtin_profile_len};
use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_save_native,
};
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::job_control::{DrJobControl, Report};
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::retained_source::{SourceEdit, SourceInput};
use image::codecs::png::PngEncoder;
use image::{
    DynamicImage, ExtendedColorType, GrayImage, ImageBuffer, ImageEncoder, Luma, Rgba, RgbaImage,
};
use serde_json::json;
use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;
use tempfile::TempDir;

use crate::common::probe::{cancelled_parked_at, parked_line, reports};
use crate::gray_fixture::chunk;

const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Every byte of every pixel from the generator.
fn noise(w: u32, h: u32, seed: u64) -> RgbaImage {
    let mut state = seed;
    let mut out = RgbaImage::new(w, h);
    for p in out.pixels_mut() {
        p.0 = (splitmix(&mut state) as u32).to_le_bytes();
    }
    out
}

/// A coverage plane whose byte at `(x, y)` is a formula of both, so a lost,
/// moved or swapped row shows.
fn plane(w: u32, h: u32, seed: u32) -> GrayImage {
    GrayImage::from_fn(w, h, |x, y| {
        Luma([(x.wrapping_mul(7) ^ y.wrapping_mul(13).wrapping_add(seed) ^ (x / 9)) as u8])
    })
}

unsafe fn text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(pointer) };
    value
}

/// Saves through the export and answers the file's path.
fn save(doc: &DrDocument, dir: &Path, name: &str) -> String {
    let path = dir.join(name).to_str().unwrap().to_string();
    let c = CString::new(path.clone()).unwrap();
    let mut error = ptr::null_mut();
    let ok = unsafe { dr_doc_save_native(doc, c.as_ptr(), ptr::null(), &mut error) };
    assert!(ok, "{name}: {}", unsafe { text(error) });
    path
}

/// Opens through the export, or answers its error sentence.
fn open(path: &str) -> Result<Box<DrDocument>, String> {
    let c = CString::new(path).unwrap();
    let mut error = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c.as_ptr(), ptr::null(), &mut error) };
    if doc.is_null() {
        return Err(unsafe { text(error) });
    }
    Ok(unsafe { Box::from_raw(doc) })
}

/// Every stored sample of `got` equals `want`'s: each layer's kind, offset,
/// pixels, mask (presence, placement, samples) and 8-bit source, and each
/// channel's plane.
fn assert_same(name: &str, want: &DrDocument, got: &DrDocument) {
    assert_eq!(got.layers.len(), want.layers.len(), "{name}: layer count");
    for (index, (a, b)) in want.layers.iter().zip(&got.layers).enumerate() {
        assert!(a.kind == b.kind, "{name}: layer {index} kind");
        assert_eq!(a.offset, b.offset, "{name}: layer {index} offset");
        if a.kind == LayerKind::Raster {
            assert_eq!(
                a.pixels.dimensions(),
                b.pixels.dimensions(),
                "{name}: layer {index} size"
            );
            assert!(
                a.pixels.to_contiguous().unwrap().as_raw()
                    == b.pixels.to_contiguous().unwrap().as_raw(),
                "{name}: layer {index} pixels differ"
            );
        }
        match (&a.mask, &b.mask) {
            (None, None) => {}
            (Some(x), Some(y)) => {
                assert_eq!(x.dimensions(), y.dimensions(), "{name}: mask {index} size");
                assert_eq!(
                    a.mask_properties.offset, b.mask_properties.offset,
                    "{name}: mask {index} offset"
                );
                assert!(
                    x.samples() == y.samples(),
                    "{name}: mask {index} samples differ"
                );
            }
            _ => panic!("{name}: layer {index} mask presence differs"),
        }
        let source = |layer: &doctored_core::doc::Layer| {
            layer
                .retained_source
                .as_ref()
                .and_then(|s| s.asset.rgba8())
                .map(|pixels| pixels.to_contiguous().unwrap().as_raw().clone())
        };
        assert!(
            source(a) == source(b),
            "{name}: layer {index} 8-bit source differs"
        );
    }
    assert_eq!(got.channels.len(), want.channels.len(), "{name}: channels");
    for (index, (a, b)) in want.channels.iter().zip(&got.channels).enumerate() {
        assert!(
            a.data.samples() == b.data.samples(),
            "{name}: channel {index} differs"
        );
    }
}

/// Two raster layers and a placed source over a canvas that crosses a tile
/// edge both ways: the placed 8-bit source carries a uniform (Reveal All)
/// mask, the masked raster an independent mask placed inside the canvas, the
/// group around it a canvas mask, and the document a channel.
fn round_trip_document() -> DrDocument {
    let (w, h) = (300_u32, 270_u32);
    let background = DrDocument::from_pixels(noise(w, h, 1));
    let source = DrDocument::from_pixels(noise(w, h, 2));
    let edit = SourceEdit::parse(
        &json!({"op":"place","source_id":"s","revision_id":"a","name":"Placed","x":0,"y":0})
            .to_string(),
    )
    .unwrap();
    let doc = background
        .source_edit(Some(&source), &edit, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    assert!(doc.layers[1].retained_source.is_some());
    let doc = doc.add_mask(1, MaskKind::RevealAll).unwrap();
    let doc = doc
        .adding_image_layer(1, noise(290, 265, 3), "Masked")
        .unwrap();
    let doc = doc
        .set_mask(2, plane(281, 263, 5), (6, 4), false, 0)
        .unwrap();
    let (doc, group, _, _) = doc.group_layers(&[2], "Group").unwrap();
    let canvas = plane(w, h, 11);
    let doc = doc
        .add_mask(group, MaskKind::FromSelection(canvas.as_raw()))
        .unwrap();
    let channel = plane(w, h, 17);
    doc.add_channel("Alpha 1", channel.as_raw(), w, h, [255, 0, 0], 0.5)
        .unwrap()
}

#[test]
fn a_round_trip_reopens_every_sample() {
    let doc = round_trip_document();
    assert!(doc.layers.iter().any(|l| l.kind == LayerKind::Group));
    let dir = TempDir::new().unwrap();
    let path = save(&doc, dir.path(), "round.doctored");
    let reopened = open(&path).unwrap();
    assert_same("round trip", &doc, &reopened);
}

/// The `(start, length)` of every PNG record of `file`, in order: at each
/// signature, the length its u32 prefix gives, and the scan resumes after
/// the record.
fn records(file: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut at = 8;
    while let Some(found) = file[at..].windows(8).position(|w| w == SIGNATURE) {
        let start = at + found;
        let len = u32::from_le_bytes(file[start - 4..start].try_into().unwrap()) as usize;
        out.push((start, len));
        at = start + len;
    }
    out
}

/// `file` with PNG record `index` replaced by `png`, its u32 length prefix
/// rewritten.
fn spliced(file: &[u8], index: usize, png: &[u8]) -> Vec<u8> {
    let (start, len) = records(file)[index];
    let mut out = file[..start - 4].to_vec();
    out.extend((png.len() as u32).to_le_bytes());
    out.extend(png);
    out.extend(&file[start + len..]);
    out
}

/// A 64×48 noise raster with one channel: record 0 is the layer PNG and
/// record 1 the channel's.
fn small_document() -> (DrDocument, RgbaImage) {
    let raster = noise(64, 48, 4);
    let channel = plane(64, 48, 23);
    let doc = DrDocument::from_pixels(raster.clone())
        .add_channel("Alpha 1", channel.as_raw(), 64, 48, [0, 0, 255], 0.5)
        .unwrap();
    (doc, raster)
}

fn encode(raw: &[u8], w: u32, h: u32, color: ExtendedColorType) -> Vec<u8> {
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(raw, w, h, color)
        .unwrap();
    png
}

/// An Adam7 PNG of `raw` (`channels` bytes a pixel, depth 8, colour type
/// `colour`), built from the PNG specification's pass table with filter 0
/// on every row.
fn adam7(raw: &[u8], w: u32, h: u32, channels: usize, colour: u8) -> Vec<u8> {
    const PASSES: [(u32, u32, u32, u32); 7] = [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ];
    let mut data = Vec::new();
    for (x0, y0, dx, dy) in PASSES {
        if x0 >= w || y0 >= h {
            continue;
        }
        let mut y = y0;
        while y < h {
            data.push(0);
            let mut x = x0;
            while x < w {
                let at = (y as usize * w as usize + x as usize) * channels;
                data.extend_from_slice(&raw[at..at + channels]);
                x += dx;
            }
            y += dy;
        }
    }
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, colour, 0, 0, 1]);
    let mut out = SIGNATURE.to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&data, 6),
    ));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// `png` with a `tEXt` chunk after its IHDR.
fn with_text(png: &[u8]) -> Vec<u8> {
    let mut out = png[..33].to_vec();
    out.extend(chunk(b"tEXt", b"Comment\0a stored PNG in another form"));
    out.extend(&png[33..]);
    out
}

/// `image`'s own decode of `png`.
fn decoded(png: &[u8]) -> DynamicImage {
    image::load_from_memory_with_format(png, image::ImageFormat::Png).unwrap()
}

#[test]
fn a_layer_png_in_another_form_decodes_as_image_decodes_it() {
    let (doc, raster) = small_document();
    let dir = TempDir::new().unwrap();
    let file = std::fs::read(save(&doc, dir.path(), "small.doctored")).unwrap();
    let (w, h) = raster.dimensions();
    let rgb = DynamicImage::ImageRgba8(raster.clone()).to_rgb8();
    let wide: ImageBuffer<Rgba<u16>, Vec<u16>> = ImageBuffer::from_raw(
        w,
        h,
        raster
            .as_raw()
            .iter()
            .enumerate()
            .map(|(i, &b)| u16::from(b) * 256 + (i % 7) as u16)
            .collect(),
    )
    .unwrap();
    let gray_alpha = DynamicImage::ImageRgba8(raster.clone()).to_luma_alpha8();
    let rgba8 = encode(raster.as_raw(), w, h, ExtendedColorType::Rgba8);
    let forms = [
        ("RGB8", encode(rgb.as_raw(), w, h, ExtendedColorType::Rgb8)),
        ("RGBA16", {
            let mut png = Vec::new();
            DynamicImage::ImageRgba16(wide)
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .unwrap();
            png
        }),
        ("Adam7 RGBA8", adam7(raster.as_raw(), w, h, 4, 6)),
        ("RGBA8 with tEXt", with_text(&rgba8)),
        (
            "GrayA8",
            encode(gray_alpha.as_raw(), w, h, ExtendedColorType::La8),
        ),
    ];
    for (name, png) in forms {
        let path = dir.path().join(format!("{name}.doctored"));
        std::fs::write(&path, spliced(&file, 0, &png)).unwrap();
        let reopened = open(path.to_str().unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let want = decoded(&png).into_rgba8();
        assert!(
            reopened.layers[0].pixels.to_contiguous().unwrap().as_raw() == want.as_raw(),
            "{name}: the layer differs from image's decode"
        );
        assert!(
            reopened.channels[0].data.samples() == doc.channels[0].data.samples(),
            "{name}: the channel moved"
        );
    }
}

#[test]
fn a_channel_png_in_another_form_decodes_as_image_decodes_it() {
    let (doc, raster) = small_document();
    let dir = TempDir::new().unwrap();
    let file = std::fs::read(save(&doc, dir.path(), "small.doctored")).unwrap();
    let (w, h) = (64, 48);
    let gray = doc.channels[0].data.samples();
    let colour = noise(w, h, 9);
    let wide: Vec<u8> = gray
        .iter()
        .enumerate()
        .flat_map(|(i, &v)| (u16::from(v) * 256 + (i % 5) as u16).to_be_bytes())
        .collect();
    let forms = [
        (
            "RGBA8",
            encode(colour.as_raw(), w, h, ExtendedColorType::Rgba8),
        ),
        ("Adam7 Gray8", adam7(&gray, w, h, 1, 0)),
        ("Gray16", {
            let mut ihdr = Vec::new();
            ihdr.extend_from_slice(&w.to_be_bytes());
            ihdr.extend_from_slice(&h.to_be_bytes());
            ihdr.extend_from_slice(&[16, 0, 0, 0, 0]);
            let mut data = Vec::new();
            for row in wide.chunks_exact(2 * w as usize) {
                data.push(0);
                data.extend_from_slice(row);
            }
            let mut png = SIGNATURE.to_vec();
            png.extend(chunk(b"IHDR", &ihdr));
            png.extend(chunk(
                b"IDAT",
                &miniz_oxide::deflate::compress_to_vec_zlib(&data, 6),
            ));
            png.extend(chunk(b"IEND", &[]));
            png
        }),
    ];
    for (name, png) in forms {
        let path = dir.path().join(format!("{name}.doctored"));
        std::fs::write(&path, spliced(&file, 1, &png)).unwrap();
        let reopened = open(path.to_str().unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let want = decoded(&png).to_luma8();
        assert!(
            reopened.channels[0].data.samples() == want.as_raw().as_slice(),
            "{name}: the channel differs from image's decode"
        );
        assert!(
            reopened.layers[0].pixels.to_contiguous().unwrap().as_raw() == raster.as_raw(),
            "{name}: the layer moved"
        );
    }
}

/// A 20×10 raster under a masked 15×8 layer, with a channel: a file of a few
/// kilobytes, cut at every length.
#[test]
fn a_truncated_file_answers_err_at_every_length() {
    let mask = plane(15, 8, 29);
    let doc = DrDocument::from_pixels(noise(20, 10, 6))
        .adding_image_layer(0, noise(15, 8, 7), "Masked")
        .unwrap()
        .set_mask(1, mask.clone(), (2, 1), true, 0)
        .unwrap()
        .add_channel(
            "Alpha 1",
            plane(20, 10, 31).as_raw(),
            20,
            10,
            [0, 255, 0],
            0.5,
        )
        .unwrap();
    let dir = TempDir::new().unwrap();
    let file = std::fs::read(save(&doc, dir.path(), "whole.doctored")).unwrap();
    let pngs = records(&file);
    assert_eq!(pngs.len(), 3, "two layer PNGs and a channel PNG");
    let mask_at = file
        .windows(mask.as_raw().len())
        .position(|w| w == mask.as_raw().as_slice())
        .expect("the raw mask is stored");
    let inside = [
        ("the layer PNG", pngs[1].0 + pngs[1].1 / 2),
        ("the mask", mask_at + mask.as_raw().len() / 2),
        ("the channel PNG", pngs[2].0 + pngs[2].1 / 2),
    ];
    let path = dir.path().join("cut.doctored");
    let path_text = path.to_str().unwrap().to_string();
    for len in 0..file.len() {
        std::fs::write(&path, &file[..len]).unwrap();
        let result = open(&path_text);
        assert!(
            result.is_err(),
            "a file cut at {len} of {} opened",
            file.len()
        );
        if let Some((what, _)) = inside.iter().find(|(_, at)| *at == len) {
            let message = result.err().unwrap_or_default();
            assert!(
                message.starts_with(&format!("failed to read document {path_text}: ")),
                "a cut inside {what}: {message}"
            );
        }
    }
}

/// `png` with its IHDR rewritten by `edit` and its CRC recomputed.
fn with_header(png: &[u8], edit: impl FnOnce(&mut [u8])) -> Vec<u8> {
    let mut ihdr = png[16..29].to_vec();
    edit(&mut ihdr);
    let mut out = png[..8].to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(&png[33..]);
    out
}

/// `png` with one byte flipped in the middle of its first IDAT's data, so
/// the chunk's CRC no longer matches.
fn flipped(png: &[u8]) -> Vec<u8> {
    assert_eq!(&png[37..41], b"IDAT", "the writer's IDAT follows its IHDR");
    let len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
    let mut out = png.to_vec();
    out[41 + len / 2] ^= 0x20;
    out
}

#[test]
fn a_corrupt_payload_answers_err_with_its_plane_s_sentence() {
    let (doc, raster) = small_document();
    let dir = TempDir::new().unwrap();
    let file = std::fs::read(save(&doc, dir.path(), "small.doctored")).unwrap();
    let pngs = records(&file);
    let layer_png = &file[pngs[0].0..pngs[0].0 + pngs[0].1];
    let channel_png = &file[pngs[1].0..pngs[1].0 + pngs[1].1];
    let (w, h) = raster.dimensions();
    // The writer's form of all but the last row under an IHDR that claims
    // every row.
    let short = with_header(
        &encode(
            &raster.as_raw()[..(4 * w * (h - 1)) as usize],
            w,
            h - 1,
            ExtendedColorType::Rgba8,
        ),
        |ihdr| ihdr[4..8].copy_from_slice(&h.to_be_bytes()),
    );
    let wider = |png: &[u8]| {
        with_header(png, |ihdr| {
            ihdr[..4].copy_from_slice(&(w + 1).to_be_bytes());
        })
    };
    let layer = "failed to decode layer pixels: ";
    let channel = "failed to decode channel plane: ";
    let cases = [
        ("a flipped layer IDAT byte", 0, flipped(layer_png), layer),
        (
            "a flipped channel IDAT byte",
            1,
            flipped(channel_png),
            channel,
        ),
        ("a layer IDAT one row short", 0, short, layer),
        (
            "a layer IHDR wider than its rows",
            0,
            wider(layer_png),
            layer,
        ),
        (
            "a channel IHDR wider than the canvas",
            1,
            wider(channel_png),
            "channel plane 65x48 does not match the 64x48 canvas",
        ),
    ];
    for (name, index, png, sentence) in cases {
        let path = dir.path().join("corrupt.doctored");
        std::fs::write(&path, spliced(&file, index, &png)).unwrap();
        let path = path.to_str().unwrap();
        let message = open(path)
            .err()
            .unwrap_or_else(|| panic!("{name}: it opened"));
        assert!(
            message.starts_with(&format!("failed to read document {path}: {sentence}")),
            "{name}: {message}"
        );
    }
}

#[test]
fn a_typed_native_file_keeps_the_rgba8_reader_s_refusal() {
    let (w, h) = (8_u32, 6_u32);
    let samples: Vec<u16> = (0..w * h * 4).map(|i| (i * 977) as u16 | 0xF000).collect();
    let len = dr_builtin_profile_len(0);
    let mut profile = vec![0_u8; len];
    assert!(unsafe { dr_builtin_profile(0, profile.as_mut_ptr(), len) });
    let mut error = ptr::null_mut();
    let typed = unsafe {
        dr_precision_doc_from_rgba16(
            samples.as_ptr(),
            samples.len(),
            w,
            h,
            profile.as_ptr(),
            profile.len(),
            ptr::null(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!typed.is_null(), "{}", unsafe { text(error) });
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("typed.doctored");
    let c = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_native(
            typed,
            c.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    unsafe { dr_precision_doc_free(typed) };
    assert!(ok, "{}", unsafe { text(error) });
    let message = open(path.to_str().unwrap())
        .err()
        .expect("the RGBA8 reader refuses a typed file");
    assert!(
        message.starts_with(&format!(
            "failed to read document {}: unsupported DRDC version ",
            path.display()
        )) && message.contains(" in the RGBA8 reader; "),
        "{message}"
    );
}

/// `job_control::EVERY_INTERVAL`, restated: the writer's-form chunk walk
/// polls once every this many chunks.
const WALK_INTERVAL: usize = 4096;

/// `png` with `count` empty IDAT chunks after its IEND: a walk still meets
/// nothing but IHDR, IDAT and IEND to the span's end, so it is the writer's
/// form, while png's own reader stops at the IEND and never reaches them.
fn trailing_chunks(png: &[u8], count: usize) -> Vec<u8> {
    let empty = chunk(b"IDAT", &[]);
    let mut out = png.to_vec();
    for _ in 0..count {
        out.extend_from_slice(&empty);
    }
    out
}

/// `dr_doc_open` of `path` under the job control behind `token`, the
/// document freed: (opened, the error).
fn open_under(path: &str, token: usize) -> (bool, String) {
    let c = CString::new(path).unwrap();
    let mut error = ptr::null_mut();
    let doc = unsafe { dr_doc_open(c.as_ptr(), token as *const DrJobControl, &mut error) };
    if doc.is_null() {
        return (false, unsafe { text(error) });
    }
    unsafe { dr_doc_free(doc) };
    (true, String::new())
}

/// The checkpoint count at the end of an uncontrolled run's report log
/// (the open's last report is its completion).
fn total(log: &[Report]) -> u64 {
    log.last().expect("the open reported").checkpoints
}

#[test]
fn a_long_chunk_walk_polls_and_a_cancel_parked_inside_it_returns() {
    let (doc, _) = small_document();
    let dir = TempDir::new().unwrap();
    let plain = save(&doc, dir.path(), "plain.doctored");
    let file = std::fs::read(&plain).unwrap();
    let (start, len) = records(&file)[0];
    // The writer's three chunks plus these make the walk poll `polls` times.
    let polls = 32_u64;
    let count = polls as usize * WALK_INTERVAL - 3;
    let png = trailing_chunks(&file[start..start + len], count);
    let padded = dir.path().join("padded.doctored");
    std::fs::write(&padded, spliced(&file, 0, &png)).unwrap();
    let padded = padded.to_str().unwrap();
    let reopened = open(padded).unwrap();
    assert_same("trailing chunks", &doc, &reopened);
    // The span's own decode never reads past the IEND, so every checkpoint
    // the padded file adds is the walk's.
    let theirs = reports(|token| open_under(&plain, token));
    let ours = reports(|token| open_under(padded, token));
    assert_eq!(
        total(&ours) - total(&theirs),
        polls,
        "{count} trailing chunks: the walk polls once per {WALK_INTERVAL}"
    );
    // The layer's report (stage 1, progress 0) is the checkpoint just before
    // its walk, so the walk's last poll is `polls` past it; the file without
    // the chunks has no checkpoint that far. The parked predicate below only
    // checks the stage; the count above is what proves those `polls`
    // checkpoints are the walk's.
    let entry = ours
        .iter()
        .find(|r| r.stage == 1)
        .expect("the layer's report")
        .checkpoints;
    assert!(
        entry + polls > total(&theirs),
        "the walk's last poll must lie past the plain open's {} checkpoints",
        total(&theirs)
    );
    let parked = cancelled_parked_at(
        entry + polls,
        &|progress, stage| stage == 1 && progress < 0.5,
        |token| open_under(padded, token),
    );
    println!("the walk's last poll: {}", parked_line(&parked));
}

/// `dr_precision_doc_open` of `path` under the job control behind `token`,
/// the document freed: (opened, the error).
fn open_typed_under(path: &str, token: usize) -> (bool, String) {
    let c = CString::new(path).unwrap();
    let mut error = ptr::null_mut();
    let doc = unsafe {
        dr_precision_doc_open(
            c.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            token as *const DrJobControl,
            &mut error,
        )
    };
    if doc.is_null() {
        return (false, unsafe { text(error) });
    }
    unsafe { dr_precision_doc_free(doc) };
    (true, String::new())
}

#[test]
fn a_typed_file_with_an_8_bit_source_cancels_exactly_at_every_checkpoint() {
    let (w, h) = (8_u32, 6_u32);
    let samples: Vec<u16> = (0..w * h * 4).map(|i| (i * 977) as u16 | 0xF000).collect();
    let len = dr_builtin_profile_len(0);
    let mut profile = vec![0_u8; len];
    assert!(unsafe { dr_builtin_profile(0, profile.as_mut_ptr(), len) });
    let mut error = ptr::null_mut();
    let typed = unsafe {
        dr_precision_doc_from_rgba16(
            samples.as_ptr(),
            samples.len(),
            w,
            h,
            profile.as_ptr(),
            profile.len(),
            ptr::null(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!typed.is_null(), "{}", unsafe { text(error) });
    // An 8-bit document placed keeps its RGBA8 master: the typed file
    // stores it as an embedded 8-bit source PNG in the writer's form.
    let input = DrDocument::from_pixels(noise(w, h, 9));
    let edit = SourceEdit::parse(
        &json!({"op":"place","source_id":"s","revision_id":"a","name":"Placed","x":0,"y":0,
                "depth_conversion":"convert_fallback"})
        .to_string(),
    )
    .unwrap();
    let placed = unsafe { &*typed }
        .source_edit_input(
            Some(SourceInput::Byte(&input)),
            &edit,
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .expect("the place changes the document");
    unsafe { dr_precision_doc_free(typed) };
    let source = placed.layer(1).unwrap().effective_source().unwrap();
    assert_eq!(source.asset.depth(), 8, "an 8-bit embedded source");
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("typed-source.doctored");
    let c = CString::new(path.to_str().unwrap()).unwrap();
    let mut error = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_save_native(
            &placed,
            c.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(ok, "{}", unsafe { text(error) });
    let path = path.to_str().unwrap();
    let last = total(&reports(|token| open_typed_under(path, token)));
    for count in 1..=last {
        cancelled_parked_at(count, &|_, _| true, |token| open_typed_under(path, token));
    }
}
