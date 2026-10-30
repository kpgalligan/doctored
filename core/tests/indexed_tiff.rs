//! #547 Indexed Color phase 1b's palette TIFF open, through the FFI against
//! TIFFs built by hand (`indexed_fixture::palette_tiff`): Photometric 3 at 4
//! and 8 bits, with and without an alpha sample, opens as an Indexed
//! document whose palette is the ColorMap's `(v + 128) / 257`, whose indices
//! are the stored samples and whose transparent index is the one index the
//! clear pixels hold; `dr_path_indexed_source` answers TIFF (3) for them
//! and NONE for an RGB TIFF; a clear pixel sharing an index with an opaque
//! one refuses by its sentence; a truncated file refuses without a panic.

use std::path::Path;

use doctored_core::ffi_indexed::dr_path_indexed_source;
use serde_json::json;
use tempfile::TempDir;

use crate::common::cpath;
use crate::indexed_fixture::{descriptor, free, indices, layer16, open_indexed, palette_tiff};

fn source(path: &Path) -> i32 {
    let c = cpath(path);
    unsafe { dr_path_indexed_source(c.as_ptr()) }
}

/// A ColorMap of `entries` colours: every red, every green, every blue, the
/// values deliberately off the ×257 grid so `(v + 128) / 257` is tested.
fn color_map(entries: usize) -> Vec<u16> {
    let mut map = Vec::with_capacity(entries * 3);
    for c in 0..3usize {
        for i in 0..entries {
            map.push(((i * 997 + c * 4099) % 65536) as u16);
        }
    }
    map
}

/// Entry i of [`color_map`] as an 8-bit colour, computed independently.
fn entry(map: &[u16], entries: usize, i: usize) -> [u8; 3] {
    [0, 1, 2].map(|c| ((u32::from(map[c * entries + i]) + 128) / 257) as u8)
}

#[test]
fn a_palette_tiff_opens_with_its_colour_map_and_every_index() {
    let dir = TempDir::new().unwrap();
    for bits in [4u16, 8] {
        let entries = 1usize << bits;
        let map = color_map(entries);
        let (w, h) = (5u32, 3u32);
        let stored: Vec<u8> = (0..15).map(|i| ((i * 7 + 3) % entries) as u8).collect();
        let path = dir.path().join(format!("p{bits}.tif"));
        std::fs::write(&path, palette_tiff((w, h), bits, 3, &map, None, &stored)).unwrap();
        assert_eq!(source(&path), 3, "{bits}: DR_INDEXED_SOURCE_TIFF");
        let doc = open_indexed(&path).unwrap();
        let d = descriptor(doc);
        assert_eq!(d["color_mode"], json!("indexed"));
        assert_eq!(
            d["indexed"],
            json!({"colors": entries, "transparent_index": null}),
            "{bits}"
        );
        assert_eq!(
            indices(doc),
            stored.iter().map(|&i| i32::from(i)).collect::<Vec<_>>()
        );
        for (pixel, &index) in layer16(doc).iter().zip(&stored) {
            let rgb = entry(&map, entries, usize::from(index));
            assert_eq!(
                *pixel,
                [
                    u16::from(rgb[0]) * 257,
                    u16::from(rgb[1]) * 257,
                    u16::from(rgb[2]) * 257,
                    65535
                ],
                "{bits}: index {index}"
            );
        }
        free(doc);
    }
}

#[test]
fn an_alpha_palette_tiff_takes_the_clear_index_as_its_transparent_index() {
    let dir = TempDir::new().unwrap();
    let map = color_map(256);
    let stored: Vec<u8> = vec![0, 5, 9, 5, 200, 9];
    let alpha: Vec<u8> = stored
        .iter()
        .map(|&i| if i == 5 { 0 } else { 255 })
        .collect();
    let path = dir.path().join("a.tif");
    std::fs::write(
        &path,
        palette_tiff((3, 2), 8, 3, &map, Some(&alpha), &stored),
    )
    .unwrap();
    assert_eq!(source(&path), 3);
    let doc = open_indexed(&path).unwrap();
    assert_eq!(
        descriptor(doc)["indexed"],
        json!({"colors": 256, "transparent_index": 5})
    );
    assert_eq!(indices(doc), vec![0, 5, 9, 5, 200, 9]);
    assert_eq!(layer16(doc)[1][3], 0, "the transparent index renders clear");
    free(doc);
    // Every alpha 255: no transparent index.
    let path = dir.path().join("opaque.tif");
    std::fs::write(
        &path,
        palette_tiff((3, 2), 8, 3, &map, Some(&[255; 6]), &stored),
    )
    .unwrap();
    let doc = open_indexed(&path).unwrap();
    assert_eq!(descriptor(doc)["indexed"]["transparent_index"], json!(null));
    free(doc);
}

#[test]
fn an_alpha_that_is_not_one_transparent_index_refuses() {
    let dir = TempDir::new().unwrap();
    let map = color_map(256);
    let stored: Vec<u8> = vec![0, 5, 9, 5];
    let sentence = "the palette TIFF's alpha is not one transparent index";
    for (name, alpha) in [
        // Index 5 is clear once and opaque once.
        ("mixed", [255u8, 0, 255, 255]),
        // Two indices are clear.
        ("two", [0, 0, 255, 0]),
        // A partial alpha.
        ("partial", [255, 128, 255, 128]),
    ] {
        let path = dir.path().join(format!("{name}.tif"));
        std::fs::write(
            &path,
            palette_tiff((2, 2), 8, 3, &map, Some(&alpha), &stored),
        )
        .unwrap();
        let err = open_indexed(&path).unwrap_err();
        assert!(err.contains(sentence), "{name}: {err}");
    }
}

#[test]
fn an_rgb_tiff_is_not_a_palette_tiff_and_a_truncated_one_refuses() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("rgb.tif");
    image::RgbImage::from_pixel(2, 2, image::Rgb([1, 2, 3]))
        .save(&path)
        .unwrap();
    assert_eq!(source(&path), 0, "Photometric 2");
    // Photometric 1 with a colour map is not a palette TIFF either.
    let path = dir.path().join("gray.tif");
    let map = color_map(16);
    std::fs::write(&path, palette_tiff((2, 2), 4, 1, &map, None, &[0, 1, 2, 3])).unwrap();
    assert_eq!(source(&path), 0, "Photometric 1");
    // A colour map of the wrong length.
    let path = dir.path().join("short-map.tif");
    std::fs::write(
        &path,
        palette_tiff((2, 2), 4, 3, &map[..30], None, &[0, 1, 2, 3]),
    )
    .unwrap();
    assert_eq!(source(&path), 0, "ColorMap of 30");
    // The strip cut short: the header still answers, the open refuses.
    let path = dir.path().join("cut.tif");
    let map = color_map(256);
    let stored: Vec<u8> = (0..=255).collect();
    let bytes = palette_tiff((16, 16), 8, 3, &map, None, &stored);
    std::fs::write(&path, &bytes[..bytes.len() - 100]).unwrap();
    assert_eq!(source(&path), 3);
    assert!(open_indexed(&path).is_err());
    // Cut inside the directory: not a palette TIFF, and no panic.
    std::fs::write(&path, &bytes[..40]).unwrap();
    assert_eq!(source(&path), 0);
    assert!(open_indexed(&path).is_err());
}
