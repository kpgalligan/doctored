//! #547 Indexed Color phase 1a's open: palette PNGs at depth 1, 2, 4 and 8
//! and GIFs written by the `png` and `gif` encoders open as Indexed
//! documents keeping every index, the palette (duplicate entries included)
//! and the transparent index; `dr_path_indexed_source` answers NONE for the
//! files that keep today's open; a corrupt file refuses without a panic.

use std::path::Path;

use doctored_core::ffi_indexed::dr_path_indexed_source;
use serde_json::json;
use tempfile::TempDir;

use crate::common::cpath;
use crate::indexed_fixture::{
    descriptor, free, indices, layer16, open_indexed, write_gif, write_png,
};

fn source(path: &Path) -> i32 {
    let c = cpath(path);
    unsafe { dr_path_indexed_source(c.as_ptr()) }
}

/// The indices a `w`×`h` plane over `count` entries: every entry used.
fn plane(w: usize, h: usize, count: usize) -> Vec<u8> {
    (0..w * h)
        .map(|i| ((i * 7 + i / w) % count) as u8)
        .collect()
}

#[test]
fn a_palette_png_keeps_every_index_at_every_depth() {
    let dir = TempDir::new().unwrap();
    // A duplicate entry (1 and 3 are the same red): its indices stay apart.
    let palette: Vec<[u8; 3]> = vec![
        [10, 20, 30],
        [200, 0, 0],
        [0, 90, 255],
        [200, 0, 0],
        [7, 7, 7],
        [250, 240, 230],
    ];
    for depth in [1u8, 2, 4, 8] {
        let count = (1usize << depth).min(palette.len());
        let entries = &palette[..count.max(2)];
        let (w, h) = (13usize, 5usize);
        let indices_in = plane(w, h, entries.len());
        let path = dir.path().join(format!("p{depth}.png"));
        write_png(
            &path,
            (w as u32, h as u32),
            depth,
            entries,
            None,
            &indices_in,
        );
        assert_eq!(source(&path), 1, "depth {depth}: DR_INDEXED_SOURCE_PNG");
        let doc = open_indexed(&path).unwrap();
        let d = descriptor(doc);
        assert_eq!(d["color_mode"], json!("indexed"));
        assert_eq!(d["layer_count"], json!(1));
        assert_eq!(
            d["indexed"],
            json!({"colors": entries.len(), "transparent_index": null})
        );
        assert_eq!(
            d["supported_operations"],
            json!([
                "render",
                "sample",
                "convert",
                "export",
                "paint",
                "erase",
                "fill_stroke",
                "clipboard_copy",
                "clipboard_cut",
                "clipboard_copy_merged",
                "geometry_crop",
                "geometry_canvas_size",
                "geometry_rotate",
                "geometry_flip",
                "geometry_resize",
                "geometry_straighten",
                "geometry_rotate_canvas",
                "clipboard_paste",
                "geometry_affine",
                "geometry_perspective",
                "geometry_affine_set",
                "geometry_perspective_set",
                "selected_pixels_transform",
                "geometry_warp",
                "layer_name",
                "layer_label",
                "layer_lock",
                "selected_pixels_move",
                "layer_visible",
                "layer_move",
                "color_assign",
                "color_convert",
                "selection",
                "channel_set",
                "read_plane",
                "channel_remove",
                "channel_read",
                "clone",
                "histogram"
            ])
        );
        let got = indices(doc);
        assert_eq!(
            got,
            indices_in.iter().map(|&i| i32::from(i)).collect::<Vec<_>>()
        );
        let render = layer16(doc);
        for (pixel, &index) in render.iter().zip(&indices_in) {
            let rgb = entries[usize::from(index)];
            assert_eq!(
                *pixel,
                [
                    rgb[0] as u16 * 257,
                    rgb[1] as u16 * 257,
                    rgb[2] as u16 * 257,
                    65535
                ]
            );
        }
        free(doc);
    }
}

#[test]
fn a_png_trns_with_one_clear_entry_is_the_transparent_index() {
    let dir = TempDir::new().unwrap();
    let palette = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
    let indices_in = vec![0, 1, 2, 1, 0, 2];
    let path = dir.path().join("t.png");
    write_png(
        &path,
        (3, 2),
        8,
        &palette,
        Some(&[255, 0, 255]),
        &indices_in,
    );
    let doc = open_indexed(&path).unwrap();
    assert_eq!(
        descriptor(doc)["indexed"],
        json!({"colors": 3, "transparent_index": 1})
    );
    assert_eq!(indices(doc), vec![0, 1, 2, 1, 0, 2]);
    assert_eq!(layer16(doc)[1], [4 * 257, 5 * 257, 6 * 257, 0]);
    free(doc);
}

#[test]
fn a_gif_keeps_its_palette_indices_and_transparent_index() {
    let dir = TempDir::new().unwrap();
    // Four entries, two the same: GIF tables are powers of two.
    let palette = [[255, 255, 255], [0, 0, 0], [30, 60, 90], [0, 0, 0]];
    let indices_in = plane(9, 4, 4);
    let path = dir.path().join("a.gif");
    write_gif(&path, (9, 4), &palette, Some(2), &indices_in, 1);
    assert_eq!(source(&path), 2, "DR_INDEXED_SOURCE_GIF");
    let doc = open_indexed(&path).unwrap();
    assert_eq!(
        descriptor(doc)["indexed"],
        json!({"colors": 4, "transparent_index": 2})
    );
    assert_eq!(
        indices(doc),
        indices_in.iter().map(|&i| i32::from(i)).collect::<Vec<_>>()
    );
    let render = layer16(doc);
    for (pixel, &index) in render.iter().zip(&indices_in) {
        let rgb = palette[usize::from(index)];
        let alpha = if index == 2 { 0 } else { 65535 };
        assert_eq!(
            *pixel,
            [
                rgb[0] as u16 * 257,
                rgb[1] as u16 * 257,
                rgb[2] as u16 * 257,
                alpha
            ]
        );
    }
    free(doc);
}

#[test]
fn files_outside_the_contract_keep_todays_open() {
    let dir = TempDir::new().unwrap();
    let palette = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
    let three = vec![0u8, 1, 2];
    // Two clear entries, and a partial alpha.
    for (name, trns) in [("two.png", vec![0u8, 0]), ("half.png", vec![255u8, 128])] {
        let path = dir.path().join(name);
        write_png(&path, (3, 1), 8, &palette, Some(&trns), &three);
        assert_eq!(source(&path), 0, "{name}");
        assert!(open_indexed(&path).is_err(), "{name}");
    }
    // An animated GIF (two image descriptors).
    let path = dir.path().join("two.gif");
    write_gif(&path, (2, 1), &[[0, 0, 0], [9, 9, 9]], None, &[0, 1], 2);
    assert_eq!(source(&path), 0, "two frames");
    // An RGB PNG.
    let path = dir.path().join("rgb.png");
    image::RgbImage::from_pixel(2, 2, image::Rgb([1, 2, 3]))
        .save(&path)
        .unwrap();
    assert_eq!(source(&path), 0, "RGB PNG");
    let err = open_indexed(&path).unwrap_err();
    assert!(err.contains("is not an indexed PNG, GIF or TIFF"), "{err}");
}

#[test]
fn a_corrupt_indexed_file_refuses_without_a_panic() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("c.png");
    write_png(
        &path,
        (16, 16),
        8,
        &[[0, 0, 0], [255, 255, 255]],
        None,
        &plane(16, 16, 2),
    );
    let bytes = std::fs::read(&path).unwrap();
    let idat = bytes.windows(4).position(|w| w == b"IDAT").unwrap();
    let mut cut = bytes[..idat + 10].to_vec();
    cut.extend_from_slice(&bytes[bytes.len() - 12..]);
    std::fs::write(&path, &cut).unwrap();
    assert!(open_indexed(&path).is_err());
    let gif = dir.path().join("c.gif");
    write_gif(
        &gif,
        (16, 16),
        &[[0, 0, 0], [255, 255, 255]],
        None,
        &plane(16, 16, 2),
        1,
    );
    let bytes = std::fs::read(&gif).unwrap();
    std::fs::write(&gif, &bytes[..bytes.len() / 2]).unwrap();
    let _ = open_indexed(&gif);
}
