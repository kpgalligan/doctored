//! 13B-4: byte layer masks, byte channels and typed mask planes are stored as
//! shared 256×256 tiles, so a 300×300 dab retains the tiles it touches and
//! not a whole plane.
//!
//! The counted cases read the app's own resource report
//! (`dr_mixed_resources_report_with_raw_masks`) over every history handle: a
//! 1536×1024 plane is 24 whole tiles and each dab lies inside four of them,
//! so the states share the base plane plus exactly 4 new tiles per dab. Each
//! base plane is uniform, so it is ONE filled tile shared by all 24 slots
//! (13B-5, `uniform_coverage_planes`). The
//! painted samples are checked against the lerp computed here (an opaque gray
//! overlay sample writes its own gray), and the saved native mask against the
//! `len, w, h, row-major bytes` run this file builds itself.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_channel::{
    dr_doc_add_channel, dr_doc_channel_plane, dr_doc_painting_channel,
};
use doctored_core::ffi_doc::{
    dr_doc_adding_layer_mask, dr_doc_free, dr_doc_open, dr_doc_painting_layer_mask,
    dr_doc_save_native,
};
use doctored_core::ffi_group::dr_doc_group_layers;
use doctored_core::ffi_mask::dr_doc_set_mask;
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_save_native,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_scope::dr_doc_layer_mask_storage_image;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::{img_dims, img_pixels, take_err_string};
use crate::resource_report;

const W: u32 = 1536;
const H: u32 = 1024;
const DABS: usize = 3;
const TILE: u64 = 256 * 256;

/// Dab `i`'s canvas rect: 300×300 at `(100 + 512·i, 100)`, inside four tiles.
fn dab(i: usize) -> (u32, u32, u32, u32) {
    (100 + 512 * i as u32, 100, 300, 300)
}

/// Writes `value` into the rect of a canvas-frame premultiplied overlay.
fn stamp(overlay: &mut [u8], canvas_w: u32, (x, y, w, h): (u32, u32, u32, u32), value: [u8; 4]) {
    for row in y..y + h {
        let at = (row as usize * canvas_w as usize + x as usize) * 4;
        for pixel in overlay[at..at + w as usize * 4].chunks_exact_mut(4) {
            pixel.copy_from_slice(&value);
        }
    }
}

fn unique_bytes(report: &Value, category: &str) -> u64 {
    report["categories"][category]["unique_bytes"]
        .as_u64()
        .unwrap_or_else(|| panic!("no {category} in {report}"))
}

fn base() -> *mut DrDocument {
    Box::into_raw(Box::new(DrDocument::from_pixels(RgbaImage::from_pixel(
        W,
        H,
        Rgba([90, 120, 30, 255]),
    ))))
}

fn free_all(handles: Vec<*mut DrDocument>) {
    for handle in handles {
        unsafe { dr_doc_free(handle) };
    }
}

/// Entry `idx`'s stored mask samples and offset, through the FFI.
fn stored_mask(doc: *const DrDocument, idx: usize) -> ((u32, u32), (i32, i32), Vec<u8>) {
    let (mut ox, mut oy) = (0, 0);
    let image = unsafe { dr_doc_layer_mask_storage_image(doc, idx, &mut ox, &mut oy) };
    assert!(!image.is_null(), "a stored mask");
    let dims = img_dims(image);
    let gray = img_pixels(image).chunks_exact(4).map(|p| p[0]).collect();
    unsafe { dr_image_free(image) };
    (dims, (ox, oy), gray)
}

/// The canvas-sized channel `i`, through the FFI.
fn channel(doc: *const DrDocument, i: usize, (w, h): (u32, u32)) -> Vec<u8> {
    let mut out = vec![0_u8; w as usize * h as usize];
    assert!(unsafe { dr_doc_channel_plane(doc, i, out.as_mut_ptr(), w, h) });
    out
}

#[test]
fn a_mask_dab_retains_four_new_tiles() {
    let base = base();
    let masked = unsafe { dr_doc_adding_layer_mask(base, 0, 0, ptr::null(), 0, 0) };
    assert!(!masked.is_null(), "a masked document");
    let mut overlay = vec![0_u8; W as usize * H as usize * 4];
    let mut handles = vec![masked];
    for i in 0..DABS {
        stamp(&mut overlay, W, dab(i), [0, 0, 0, 255]);
        let next = unsafe { dr_doc_painting_layer_mask(handles[i], 0, overlay.as_ptr(), W, H) };
        assert!(!next.is_null(), "a painted mask");
        handles.push(next);
        stamp(&mut overlay, W, dab(i), [0; 4]);
    }
    let last = stored_mask(handles[DABS], 0).2;
    for y in 0..H {
        for x in 0..W {
            let inside = (0..DABS).any(|i| {
                let (dx, dy, dw, dh) = dab(i);
                (dx..dx + dw).contains(&x) && (dy..dy + dh).contains(&y)
            });
            let want = if inside { 0 } else { 255 };
            assert_eq!(last[(y * W + x) as usize], want, "mask at ({x}, {y})");
        }
    }
    let documents: Vec<*const DrDocument> = handles.iter().map(|h| *h as *const _).collect();
    let report = resource_report::report(&documents, &[], &[], &[], 0, 0).expect("a report");
    assert_eq!(
        unique_bytes(&report, "layer_masks"),
        TILE + DABS as u64 * 4 * TILE,
        "{} states of a mask share one filled tile plus 4 tiles per dab",
        DABS + 1
    );
    free_all(handles);
    unsafe { dr_doc_free(base) };
}

#[test]
fn a_channel_dab_retains_four_new_tiles() {
    let base = base();
    let name = CString::new("Alpha 1").unwrap();
    let plane = vec![255_u8; W as usize * H as usize];
    let with =
        unsafe { dr_doc_add_channel(base, name.as_ptr(), plane.as_ptr(), W, H, 255, 0, 0, 0.5) };
    assert!(!with.is_null(), "a document with a channel");
    let mut overlay = vec![0_u8; W as usize * H as usize * 4];
    let mut handles = vec![with];
    for i in 0..DABS {
        stamp(&mut overlay, W, dab(i), [0, 0, 0, 255]);
        let next = unsafe { dr_doc_painting_channel(handles[i], 0, overlay.as_ptr(), W, H) };
        assert!(!next.is_null(), "a painted channel");
        handles.push(next);
        stamp(&mut overlay, W, dab(i), [0; 4]);
    }
    // Painting the last dab again changes nothing: the channel's latch.
    stamp(&mut overlay, W, dab(DABS - 1), [0, 0, 0, 255]);
    let again = unsafe { dr_doc_painting_channel(handles[DABS], 0, overlay.as_ptr(), W, H) };
    assert!(again.is_null(), "an unchanged channel stroke answers NULL");
    let last = channel(handles[DABS], 0, (W, H));
    for i in 0..DABS {
        let (x, y, _, _) = dab(i);
        assert_eq!(last[(y * W + x) as usize], 0, "dab {i} painted");
        assert_eq!(last[(y * W + x - 1) as usize], 255, "left of dab {i} kept");
    }
    let documents: Vec<*const DrDocument> = handles.iter().map(|h| *h as *const _).collect();
    let report = resource_report::report(&documents, &[], &[], &[], 0, 0).expect("a report");
    assert_eq!(
        unique_bytes(&report, "saved_channels"),
        TILE + DABS as u64 * 4 * TILE,
        "{} states of a channel share one filled tile plus 4 tiles per dab",
        DABS + 1
    );
    free_all(handles);
    unsafe { dr_doc_free(base) };
}

#[test]
fn a_typed_mask_dab_retains_four_new_tiles() {
    let n = W as usize * H as usize;
    let image = PrecisionImage::from_rgba16(
        W,
        H,
        vec![[9000, 20000, 31000, 65535]; n],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let base =
        PrecisionDocument::from_image(image, Default::default(), Default::default()).unwrap();
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(W, H, vec![65535; n], None).unwrap(),
        MaskProperties::at((0, 0)),
        true,
    )
    .unwrap();
    let masked = base
        .set_layer_mask(0, Some(mask), REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    let request = CString::new(
        json!({"op":"scalar_edit","target":"mask","index":0,"selection":false,
            "operation":{"kind":"paint","value":0,"opacity":1.0}})
        .to_string(),
    )
    .unwrap();
    let mut coverage = vec![0_u16; n];
    let mut handles: Vec<*mut PrecisionDocument> = vec![Box::into_raw(Box::new(masked))];
    for i in 0..DABS {
        let (x, y, w, h) = dab(i);
        for row in y..y + h {
            let at = (row * W + x) as usize;
            coverage[at..at + w as usize].fill(65535);
        }
        let mut error: *mut c_char = ptr::null_mut();
        let next = unsafe {
            dr_precision_doc_edit(
                handles[i],
                request.as_ptr(),
                coverage.as_ptr(),
                coverage.len(),
                REFERENCE_WORKING_BYTES,
                0,
                ptr::null(),
                &mut error,
            )
        };
        assert!(
            !next.is_null(),
            "a painted typed mask: {}",
            take_err_string(error)
        );
        handles.push(next);
        coverage.fill(0);
    }
    let documents: Vec<*const PrecisionDocument> = handles.iter().map(|h| *h as *const _).collect();
    let report = resource_report::report(&[], &[], &documents, &[], 0, 0).expect("a report");
    let plane = TILE * 2;
    let bytes = unique_bytes(&report, "precision_plane");
    let bound = plane + DABS as u64 * 4 * TILE * 2;
    assert!(
        (plane..=bound).contains(&bytes),
        "{} states of a typed mask hold {bytes} bytes: one filled tile ({plane}) plus at most 4 \
         tiles per dab ({bound})",
        DABS + 1
    );
    for handle in handles {
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// An opaque gray dab on a mask stored at its own offset and size (not the
/// layer's) lands on the mask samples under it: canvas `(x, y)` is mask
/// `(x − ox, y − oy)`, and the part of the dab off the mask changes nothing.
#[test]
fn a_dab_on_a_moved_mask_writes_its_own_samples() {
    let base = base();
    let (mw, mh, ox, oy) = (700_u32, 500_u32, 50_i32, 30_i32);
    let pattern: Vec<u8> = (0..mw * mh)
        .map(|i| ((i % mw) * 3 + (i / mw) * 7) as u8)
        .collect();
    let masked = unsafe { dr_doc_set_mask(base, 0, pattern.as_ptr(), mw, mh, ox, oy, false, 0) };
    assert!(!masked.is_null(), "a mask at its own offset");
    // One dab inside the mask, one across its right and bottom edges.
    let dabs = [(100, 100, 300, 300), (600, 400, 300, 300)];
    let mut overlay = vec![0_u8; W as usize * H as usize * 4];
    for rect in dabs {
        stamp(&mut overlay, W, rect, [77, 77, 77, 255]);
    }
    let painted = unsafe { dr_doc_painting_layer_mask(masked, 0, overlay.as_ptr(), W, H) };
    assert!(!painted.is_null(), "a painted mask");
    let (dims, offset, got) = stored_mask(painted, 0);
    assert_eq!(
        (dims, offset),
        ((mw, mh), (ox, oy)),
        "the mask keeps its rect"
    );
    for my in 0..mh {
        for mx in 0..mw {
            let (cx, cy) = (mx as i32 + ox, my as i32 + oy);
            let under = dabs.iter().any(|&(x, y, w, h)| {
                (x as i32..(x + w) as i32).contains(&cx) && (y as i32..(y + h) as i32).contains(&cy)
            });
            let want = if under {
                77
            } else {
                pattern[(my * mw + mx) as usize]
            };
            assert_eq!(
                got[(my * mw + mx) as usize],
                want,
                "mask sample ({mx}, {my})"
            );
        }
    }
    free_all(vec![painted, masked]);
    unsafe { dr_doc_free(base) };
}

/// A GROUP's mask is canvas-sized at the group's offset, whatever its
/// members' extent: a dab writes the canvas samples under it.
#[test]
fn a_dab_on_a_group_mask_writes_its_canvas_samples() {
    let base = base();
    let name = CString::new("Group").unwrap();
    let (mut group, mut cleared_len, mut reordered_len) = (0_usize, 0_usize, 0_usize);
    let mut cleared = [0_usize; 4];
    let mut reordered = [0_usize; 4];
    let grouped = unsafe {
        dr_doc_group_layers(
            base,
            [0_usize].as_ptr(),
            1,
            name.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            4,
        )
    };
    assert!(!grouped.is_null(), "a group");
    let masked = unsafe { dr_doc_adding_layer_mask(grouped, group, 0, ptr::null(), 0, 0) };
    assert!(!masked.is_null(), "a masked group");
    let rect = (700, 500, 300, 300);
    let mut overlay = vec![0_u8; W as usize * H as usize * 4];
    stamp(&mut overlay, W, rect, [40, 40, 40, 255]);
    let painted = unsafe { dr_doc_painting_layer_mask(masked, group, overlay.as_ptr(), W, H) };
    assert!(!painted.is_null(), "a painted group mask");
    let (dims, _, got) = stored_mask(painted, group);
    assert_eq!(dims, (W, H), "a group mask is canvas-sized");
    let (x0, y0, w, h) = rect;
    for y in 0..H {
        for x in 0..W {
            let under = (x0..x0 + w).contains(&x) && (y0..y0 + h).contains(&y);
            let want = if under { 40 } else { 255 };
            assert_eq!(got[(y * W + x) as usize], want, "group mask at ({x}, {y})");
        }
    }
    free_all(vec![painted, masked, grouped]);
    unsafe { dr_doc_free(base) };
}

/// The native file holds a mask as `len, w, h` (u32 LE) and its row-major
/// samples; a 300×270 mask is four clipped tiles, so a writer that walked
/// them in tile order instead of row order would write other bytes. A mask
/// reached by dabs saves the bytes the dabs' lerp gives, and the mask and a
/// channel reopen sample for sample.
#[test]
fn a_saved_mask_and_channel_are_their_row_major_samples() {
    let (cw, ch) = (300_u32, 270_u32);
    let base = Box::into_raw(Box::new(DrDocument::from_pixels(RgbaImage::from_pixel(
        cw,
        ch,
        Rgba([10, 20, 30, 255]),
    ))));
    let pattern: Vec<u8> = (0..cw * ch)
        .map(|i| ((i % cw) * 5 + (i / cw) * 11 + (i % 7)) as u8)
        .collect();
    let masked = unsafe { dr_doc_set_mask(base, 0, pattern.as_ptr(), cw, ch, 0, 0, true, 0) };
    assert!(!masked.is_null(), "a masked document");
    let name = CString::new("Alpha 1").unwrap();
    let plane: Vec<u8> = pattern.iter().map(|v| v.wrapping_mul(3)).collect();
    let with = unsafe {
        dr_doc_add_channel(
            masked,
            name.as_ptr(),
            plane.as_ptr(),
            cw,
            ch,
            255,
            0,
            0,
            0.5,
        )
    };
    assert!(!with.is_null(), "a document with a channel");
    // A dab across the four tiles' corner.
    let rect = (200, 200, 100, 70);
    let mut overlay = vec![0_u8; cw as usize * ch as usize * 4];
    stamp(&mut overlay, cw, rect, [9, 9, 9, 255]);
    let painted = unsafe { dr_doc_painting_layer_mask(with, 0, overlay.as_ptr(), cw, ch) };
    assert!(!painted.is_null(), "a painted mask");
    let mut want = pattern.clone();
    let (x0, y0, w, h) = rect;
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            want[(y * cw + x) as usize] = 9;
        }
    }
    let dir = tempfile::tempdir().unwrap();
    for (what, doc, mask) in [("built whole", with, &pattern), ("dabbed", painted, &want)] {
        let path = dir.path().join(format!("{}.rz", what.replace(' ', "_")));
        let cpath = CString::new(path.to_str().unwrap()).unwrap();
        let mut error: *mut c_char = ptr::null_mut();
        let saved = unsafe { dr_doc_save_native(doc, cpath.as_ptr(), ptr::null(), &mut error) };
        assert!(saved, "{what}: save: {}", take_err_string(error));
        let bytes = std::fs::read(&path).unwrap();
        let mut run = Vec::new();
        run.extend_from_slice(&(cw * ch).to_le_bytes());
        run.extend_from_slice(&cw.to_le_bytes());
        run.extend_from_slice(&ch.to_le_bytes());
        run.extend_from_slice(mask);
        assert!(
            bytes
                .windows(run.len())
                .any(|window| window == run.as_slice()),
            "{what}: the file holds the mask's row-major run"
        );
        let reopened = unsafe { dr_doc_open(cpath.as_ptr(), ptr::null(), &mut error) };
        assert!(
            !reopened.is_null(),
            "{what}: reopen: {}",
            take_err_string(error)
        );
        assert_eq!(
            &stored_mask(reopened, 0).2,
            mask,
            "{what}: the reopened mask"
        );
        assert_eq!(
            channel(reopened, 0, (cw, ch)),
            plane,
            "{what}: the reopened channel"
        );
        unsafe { dr_doc_free(reopened) };
    }
    free_all(vec![painted, with, masked, base]);
}

/// The typed native file holds a mask or channel plane as its byte length
/// (u64 LE) and its row-major u16 samples (LE); a 300×270 plane is four
/// clipped tiles, so a tile-order walk would write other bytes. The reopened
/// planes answer the same samples.
#[test]
fn a_saved_typed_mask_and_channel_are_their_row_major_samples() {
    let (cw, ch) = (300_u32, 270_u32);
    let n = cw as usize * ch as usize;
    let mask_samples: Vec<u16> = (0..n as u32)
        .map(|i| ((i % cw) * 211 + (i / cw) * 97 + i % 13) as u16)
        .collect();
    let channel_samples: Vec<u16> = mask_samples.iter().map(|v| v.wrapping_mul(7)).collect();
    let image = PrecisionImage::from_rgba16(
        cw,
        ch,
        vec![[9000, 20000, 31000, 65535]; n],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let layer = doctored_core::precision_document::PrecisionDocumentLayer::raster(
        "Base",
        image,
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let channel = doctored_core::precision_mask::PrecisionChannel::new(
        "Alpha 1",
        PrecisionPlane::from_u16(cw, ch, channel_samples.clone(), None).unwrap(),
    )
    .unwrap();
    let doc = PrecisionDocument::from_parts_and_channels(
        cw,
        ch,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        vec![layer],
        vec![channel],
    )
    .unwrap();
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(cw, ch, mask_samples.clone(), None).unwrap(),
        MaskProperties::at((0, 0)),
        true,
    )
    .unwrap();
    let doc = doc
        .set_layer_mask(0, Some(mask), REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("typed.rz");
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let saved = unsafe {
        dr_precision_doc_save_native(
            &doc,
            cpath.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(saved, "save: {}", take_err_string(error));
    let bytes = std::fs::read(&path).unwrap();
    for (what, samples) in [("mask", &mask_samples), ("channel", &channel_samples)] {
        let mut run = Vec::with_capacity(8 + n * 2);
        run.extend_from_slice(&(n as u64 * 2).to_le_bytes());
        for sample in samples {
            run.extend_from_slice(&sample.to_le_bytes());
        }
        assert!(
            bytes
                .windows(run.len())
                .any(|window| window == run.as_slice()),
            "the file holds the {what}'s row-major run"
        );
    }
    let reopened = unsafe {
        dr_precision_doc_open(
            cpath.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!reopened.is_null(), "reopen: {}", take_err_string(error));
    let back = unsafe { &*reopened };
    let mask = back.layer(0).unwrap().mask().expect("a reopened mask");
    assert_eq!(mask.plane().samples(), mask_samples, "the reopened mask");
    assert_eq!(
        back.channels()[0].plane().samples(),
        channel_samples,
        "the reopened channel"
    );
    unsafe { dr_precision_doc_free(reopened) };
}
