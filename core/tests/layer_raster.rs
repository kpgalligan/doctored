//! Black-box contract suite for `LayerRaster` (13B-2a), the byte layer-pixel
//! accessor. Every oracle is independent of the accessor: dimensions and
//! bytes come from the source `RgbaImage`, rect reads are hand-sliced from
//! it with `get_pixel`. The 6000×4000 phase-4 corpus rect crosses 256-px tile
//! boundaries so 13B-2b reruns this suite unchanged over tiles.

use crate::phase4_corpus;
use crate::serial;

use std::sync::Arc;

use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::style::LayerStyle;
use image::{Rgba, RgbaImage};

fn patterned(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            (x * 7 + y) as u8,
            (y * 13 + x * 3) as u8,
            (x ^ (y * 5)) as u8,
            (x + y * 2) as u8,
        ])
    })
}

fn oracle(image: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for row in y..y + h {
        for column in x..x + w {
            out.push(image.get_pixel(column, row).0);
        }
    }
    out
}

#[test]
fn dimensions_equal_the_source_image() {
    let _serial = serial();
    let image = patterned(37, 19);
    let raster = LayerRaster::new(image.clone());
    assert_eq!(raster.dimensions(), image.dimensions());
    assert_eq!(raster.width(), 37);
    assert_eq!(raster.height(), 19);
}

#[test]
fn contiguous_image_is_shared_and_equals_the_source() {
    let _serial = serial();
    let image = patterned(41, 23);
    let raster = LayerRaster::new(image.clone());
    let a = raster.contiguous_image();
    let b = raster.contiguous_image();
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(a.as_raw(), image.as_raw());
    assert_eq!(a.dimensions(), image.dimensions());
}

#[test]
fn to_contiguous_is_an_independent_exact_copy() {
    let _serial = serial();
    let image = patterned(29, 31);
    let raster = LayerRaster::new(image.clone());
    let mut copy = raster.to_contiguous().unwrap();
    assert_eq!(copy.dimensions(), image.dimensions());
    for (x, y, pixel) in image.enumerate_pixels() {
        assert_eq!(copy.get_pixel(x, y), pixel);
    }
    copy.put_pixel(3, 4, Rgba([1, 2, 3, 4]));
    copy.put_pixel(0, 0, Rgba([9, 9, 9, 9]));
    assert_eq!(raster.contiguous_image().as_raw(), image.as_raw());
}

#[test]
fn read_rect_matches_hand_sliced_oracles() {
    let _serial = serial();
    let image = patterned(53, 47);
    let raster = LayerRaster::new(image.clone());
    let (w, h) = image.dimensions();
    let cases = [
        (10, 12, 7, 9),       // interior
        (0, 5, 4, 3),         // left edge
        (w - 6, 5, 6, 3),     // right edge
        (8, 0, 5, 4),         // top edge
        (8, h - 4, 5, 4),     // bottom edge
        (0, 0, w, h),         // full plane
        (20, 30, 1, 1),       // 1×1 rect
        (w - 1, h - 1, 1, 1), // bottom-right sample
    ];
    for (x, y, rw, rh) in cases {
        assert_eq!(
            raster.read_rect(x, y, rw, rh),
            Some(oracle(&image, x, y, rw, rh)),
            "rect ({x}, {y}, {rw}, {rh})"
        );
    }
}

#[test]
fn read_rect_on_a_one_pixel_plane() {
    let _serial = serial();
    let image = RgbaImage::from_pixel(1, 1, Rgba([10, 20, 30, 40]));
    let raster = LayerRaster::new(image);
    assert_eq!(raster.read_rect(0, 0, 1, 1), Some(vec![[10, 20, 30, 40]]));
    assert_eq!(raster.read_rect(1, 0, 1, 1), None);
    assert_eq!(raster.read_rect(0, 1, 1, 1), None);
}

#[test]
fn read_rect_crosses_tile_boundaries_on_the_corpus() {
    let _serial = serial();
    let (width, height) = phase4_corpus::DIMENSIONS[0];
    let image = phase4_corpus::image(width, height);
    let raster = LayerRaster::new(image.clone());
    // Spans the 256/512/768 column and 256/512 row tile seams.
    let (x, y, w, h) = (250, 200, 530, 330);
    assert_eq!(
        raster.read_rect(x, y, w, h),
        Some(oracle(&image, x, y, w, h))
    );
    let edge = (width - 300, height - 260, 300, 260);
    assert_eq!(
        raster.read_rect(edge.0, edge.1, edge.2, edge.3),
        Some(oracle(&image, edge.0, edge.1, edge.2, edge.3))
    );
}

#[test]
fn read_rect_zero_area_and_out_of_plane() {
    let _serial = serial();
    let raster = LayerRaster::new(patterned(16, 12));
    assert_eq!(raster.read_rect(3, 4, 0, 5), Some(vec![]));
    assert_eq!(raster.read_rect(3, 4, 5, 0), Some(vec![]));
    assert_eq!(raster.read_rect(16, 12, 0, 0), Some(vec![]));
    assert_eq!(raster.read_rect(10, 0, 7, 1), None);
    assert_eq!(raster.read_rect(0, 8, 1, 5), None);
    assert_eq!(raster.read_rect(17, 0, 0, 0), None);
    // Would pass a wrapping `x + w` / `y + h` check and panic on the slice.
    assert_eq!(raster.read_rect(u32::MAX, 0, 2, 1), None);
    assert_eq!(raster.read_rect(0, u32::MAX, 1, 2), None);
}

#[test]
fn ptr_eq_is_allocation_identity() {
    let _serial = serial();
    let image = patterned(8, 8);
    let a = LayerRaster::new(image.clone());
    let clone = a.clone();
    let equal_bytes = LayerRaster::new(image.clone());
    assert!(LayerRaster::ptr_eq(&a, &clone));
    assert!(!LayerRaster::ptr_eq(&a, &equal_bytes));
    let shared = Arc::new(image);
    let adopted = LayerRaster::shared(Arc::clone(&shared));
    assert!(Arc::ptr_eq(&adopted.contiguous_image(), &shared));
}

#[test]
fn weak_count_tracks_held_keys_through_the_style_cache() {
    let _serial = serial();
    // `key()` is crate-private; a styled composite is the black-box way to
    // make a key held (the cache lives inside the `Arc<LayerStyle>`), and
    // dropping the only document holding that style releases it.
    let fresh = LayerRaster::new(patterned(4, 4));
    assert_eq!(fresh.weak_count(), 0);
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(40, 30, Rgba([255; 4])))
        .adding_image_layer(
            0,
            RgbaImage::from_pixel(10, 10, Rgba([255, 0, 0, 255])),
            "L",
        )
        .unwrap();
    let style = LayerStyle::from_json(
        "{\"effects\":[{\"type\":\"stroke\",\"size\":2,\"color\":\"#0000FF\"}]}",
    )
    .unwrap();
    let styled = doc.set_layer_style(1, Some(Arc::new(style))).unwrap();
    let pixels = styled.layers[1].pixels.clone();
    assert_eq!(pixels.weak_count(), 0, "nothing rendered yet");
    let _ = styled.flattened().unwrap();
    assert_eq!(pixels.weak_count(), 1, "one key held while the style lives");
    drop(styled);
    assert_eq!(pixels.weak_count(), 0, "the key dropped with its cache");
}

#[test]
fn documents_share_and_split_rasters() {
    let _serial = serial();
    let doc = DrDocument::from_pixels(patterned(24, 18));
    let renamed = doc.with_layer_name(0, "Renamed").unwrap();
    assert!(LayerRaster::ptr_eq(
        &doc.layers[0].pixels,
        &renamed.layers[0].pixels
    ));
    let repainted = doc.with_layer_pixels(0, patterned(24, 18)).unwrap();
    assert!(!LayerRaster::ptr_eq(
        &doc.layers[0].pixels,
        &repainted.layers[0].pixels
    ));
    assert_eq!(
        *doc.layers[0].pixels.contiguous_image(),
        *repainted.layers[0].pixels.contiguous_image()
    );
}

#[test]
fn each_group_owns_a_private_raster() {
    let _serial = serial();
    let doc = DrDocument::from_pixels(patterned(30, 20))
        .adding_image_layer(0, patterned(5, 5), "A")
        .unwrap()
        .adding_image_layer(1, patterned(6, 6), "B")
        .unwrap();
    let (doc, first, _, _) = doc.group_layers(&[1], "G1").unwrap();
    let (doc, second, _, _) = doc.group_layers(&[first + 1], "G2").unwrap();
    assert_ne!(first, second);
    assert!(!LayerRaster::ptr_eq(
        &doc.layers[first].pixels,
        &doc.layers[second].pixels
    ));
}
