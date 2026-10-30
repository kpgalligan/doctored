//! Whole-canvas layer-mask Invert through `dr_doc_invert_mask`.
//!
//! The oracle is the canvas-wide effective coverage
//! (`DrDocument::layer_effective_mask_image`, the reader
//! `mask_outside_coverage.rs` uses) taken before and after: with no selection
//! every canvas sample must read `255 − before`, inside the stored rectangle
//! and outside it; with a selection every canvas sample must read the f64
//! interpolation of `before` and `255 − before` by its coverage.
use doctored_core::doc::{DrDocument, LayerKind, LayerRaster};
use doctored_core::doc_lock::LOCK_ALL;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_mask::*;
use image::{GrayImage, Rgba, RgbaImage};
use std::ptr;
use std::sync::Arc;

const W: u32 = 9;
const H: u32 = 7;

/// A 9×7 document whose one layer carries a `w × h` mask at `offset` with
/// distinct stored values and the given outside coverage.
fn masked(w: u32, h: u32, offset: (i32, i32), outside: u8) -> DrDocument {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([31, 72, 119, 255])));
    let values: Vec<u8> = (0..w * h).map(|i| (i * 37 + 11) as u8).collect();
    let layer = &mut doc.layers[0];
    layer.mask = Some(Arc::new(GrayImage::from_raw(w, h, values).unwrap()));
    layer.mask_properties = MaskProperties::at(offset);
    layer.mask_properties.outside_coverage = outside;
    doc
}

fn effective(doc: &DrDocument, idx: usize) -> Vec<u8> {
    doc.layer_effective_mask_image(idx)
        .expect("an effective mask")
        .pixels()
        .map(|p| p[0])
        .collect()
}

fn invert(doc: &DrDocument, idx: usize, selection: Option<&[u8]>) -> Option<Box<DrDocument>> {
    let out = unsafe {
        match selection {
            Some(s) => dr_doc_invert_mask(doc, idx, s.as_ptr(), s.len()),
            None => dr_doc_invert_mask(doc, idx, ptr::null(), 0),
        }
    };
    (!out.is_null()).then(|| unsafe { Box::from_raw(out) })
}

/// The independent scalar rule: `before` and `255 − before` interpolated by
/// coverage `q` in f64, rounded once.
fn scoped(before: u8, q: u8) -> u8 {
    ((f64::from(before) * f64::from(255 - q) + f64::from(255 - before) * f64::from(q)) / 255.0)
        .round() as u8
}

/// A canvas selection with zero, partial and full coverage spread over the
/// stored rectangle and the area outside it.
fn selection() -> Vec<u8> {
    (0..W * H)
        .map(|i| [0u8, 255, 128, 255, 1, 64, 0, 254][(i % 8) as usize])
        .collect()
}

#[test]
fn no_selection_inverts_inside_and_outside_the_stored_rectangle() {
    for outside in [0u8, 255] {
        let doc = masked(3, 2, (4, 3), outside);
        let before = effective(&doc, 0);
        let out = invert(&doc, 0, None).expect("an inverted mask");
        let after = effective(&out, 0);
        for (i, (&b, &a)) in before.iter().zip(&after).enumerate() {
            assert_eq!(a, 255 - b, "canvas sample {i}, outside {outside}");
        }
        // The stored rectangle and every other property are kept.
        let (old, new) = (&doc.layers[0], &out.layers[0]);
        assert_eq!(new.mask.as_ref().unwrap().dimensions(), (3, 2));
        assert_eq!(new.mask_properties.offset, (4, 3));
        assert_eq!(new.mask_properties.outside_coverage, 255 - outside);
        assert_eq!(new.mask_properties.linked, old.mask_properties.linked);
        assert_eq!(new.mask_properties.density, old.mask_properties.density);
        assert_eq!(new.mask_properties.feather, old.mask_properties.feather);
        assert_eq!(new.mask_enabled, old.mask_enabled);
        // Twice is the original mask, exactly.
        let back = invert(&out, 0, None).expect("a second inversion");
        assert!(unsafe { dr_doc_layer_masks_equal(&doc, &*back, 0) });
    }
}

#[test]
fn a_selection_inverts_by_coverage_across_the_whole_canvas() {
    for outside in [0u8, 255] {
        // The stored plane hangs off the canvas' left and bottom edges.
        let doc = masked(3, 3, (-1, 5), outside);
        let before = effective(&doc, 0);
        let stored: Vec<u8> = doc.layers[0].mask.as_ref().unwrap().as_raw().clone();
        let selection = selection();
        let out = invert(&doc, 0, Some(&selection)).expect("a scoped inversion");
        let after = effective(&out, 0);
        for i in 0..(W * H) as usize {
            assert_eq!(
                after[i],
                scoped(before[i], selection[i]),
                "canvas sample {i}, outside {outside}"
            );
        }
        // The plane grew to hold the canvas and its own off-canvas part.
        let layer = &out.layers[0];
        let plane = layer.mask.as_ref().unwrap();
        assert_eq!(layer.mask_properties.offset, (-1, 0));
        assert_eq!(plane.dimensions(), (10, 8));
        assert_eq!(layer.mask_properties.outside_coverage, outside);
        // Stored samples outside the canvas are not selected: unchanged.
        for (i, &value) in stored.iter().enumerate() {
            let (x, y) = (i as i32 % 3 - 1, i as i32 / 3 + 5);
            if x >= 0 && y < H as i32 {
                continue;
            }
            let at = (y as u32, (x + 1) as u32);
            assert_eq!(plane.get_pixel(at.1, at.0)[0], value, "stored sample {i}");
        }
    }
}

#[test]
fn a_selection_wholly_inside_the_stored_rectangle_still_reads_right_everywhere() {
    let doc = masked(3, 2, (4, 3), 255);
    let before = effective(&doc, 0);
    let mut selection = vec![0u8; (W * H) as usize];
    selection[(3 * W + 5) as usize] = 255;
    selection[(4 * W + 6) as usize] = 90;
    let out = invert(&doc, 0, Some(&selection)).expect("a scoped inversion");
    let after = effective(&out, 0);
    for i in 0..(W * H) as usize {
        assert_eq!(
            after[i],
            scoped(before[i], selection[i]),
            "canvas sample {i}"
        );
    }
}

#[test]
fn a_group_mask_keeps_its_rectangle() {
    let mut doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([9, 9, 9, 255])));
    let child = doc.layers[0].clone();
    doc.layers[0].kind = LayerKind::Group;
    doc.layers[0].pixels = LayerRaster::new(RgbaImage::new(1, 1));
    doc.layers[0].offset = (2, -1);
    doc.layers[0].mask_properties.offset = (2, -1);
    doc.layers[0].mask = Some(Arc::new(
        GrayImage::from_raw(W, H, (0..W * H).map(|v| (v * 3) as u8).collect()).unwrap(),
    ));
    doc.layers.push(child);
    doc.layers[1].depth = 1;
    let stored: Vec<u8> = doc.layers[0].mask.as_ref().unwrap().as_raw().clone();

    // No selection: every stored sample and the outside constant flip.
    let whole = invert(&doc, 0, None).expect("an inverted group mask");
    let before = effective(&doc, 0);
    let after = effective(&whole, 0);
    for (i, (&b, &a)) in before.iter().zip(&after).enumerate() {
        assert_eq!(a, 255 - b, "canvas sample {i}");
    }
    assert_eq!(whole.layers[0].mask_properties.outside_coverage, 255);

    // With a selection the plane keeps its size and offset; each stored
    // sample takes the coverage at its own canvas position, 0 off the canvas.
    let selection = selection();
    let out = invert(&doc, 0, Some(&selection)).expect("a scoped group inversion");
    let layer = &out.layers[0];
    assert_eq!(layer.mask.as_ref().unwrap().dimensions(), (W, H));
    assert_eq!(layer.mask_properties.offset, (2, -1));
    assert_eq!(layer.mask_properties.outside_coverage, 0);
    for (i, &value) in stored.iter().enumerate() {
        let (cx, cy) = (i as i32 % W as i32 + 2, i as i32 / W as i32 - 1);
        let q = if cx < 0 || cy < 0 || cx >= W as i32 || cy >= H as i32 {
            0
        } else {
            selection[(cy as u32 * W + cx as u32) as usize]
        };
        assert_eq!(
            layer.mask.as_ref().unwrap().as_raw()[i],
            scoped(value, q),
            "stored sample {i}"
        );
    }
}

#[test]
fn refusals_answer_null() {
    let doc = masked(3, 2, (4, 3), 0);
    // A selection that selects nothing changes nothing.
    assert!(invert(&doc, 0, Some(&vec![0u8; (W * H) as usize])).is_none());
    // A selection of the wrong length.
    assert!(invert(&doc, 0, Some(&vec![255u8; (W * H - 1) as usize])).is_none());
    // An index out of range, and a layer with no mask.
    assert!(invert(&doc, 3, None).is_none());
    let plain = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([1, 2, 3, 255])));
    assert!(invert(&plain, 0, None).is_none());
    // Lock All protects the mask.
    let mut locked = masked(3, 2, (4, 3), 0);
    locked.layers[0].locks = LOCK_ALL;
    assert!(invert(&locked, 0, None).is_none());
    assert!(invert(&locked, 0, Some(&vec![255u8; (W * H) as usize])).is_none());
    // A NULL document.
    assert!(unsafe { dr_doc_invert_mask(ptr::null(), 0, ptr::null(), 0) }.is_null());
}
