//! #547 Indexed Color phase 3c: the tools that named phase 3c, through the
//! FFI. The Magic Wand and Quick Selection select from the palette colours;
//! the alpha channels' commands and scalar edits keep the plane and the
//! render and survive `.rz`; Clone Stamp and the History Brush paint the RGB
//! storage and re-index each pixel to its own nearest entry; the rest refuse
//! by name, for good.
//!
//! The oracles are the tests' own: masks and copies built by hand from the
//! index plane, the expansion route's re-index (`indexed_fixture::oracle_with`
//! with the `Own` ink) and `palette[index]` for the render.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::ffi_precision_edit::{
    dr_precision_doc_read_plane, dr_precision_doc_scalar_statistics, dr_precision_doc_select,
};
use doctored_core::ffi_retouch::dr_precision_doc_retouch_from;
use doctored_core::ffi_retouch_brush::dr_precision_doc_history_brush;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{
    color_table, free, indices, open_rgb, oracle_with, paint, palette_of, render16, rgb_doc,
    to_indexed, to_rgb, window, Ink, LIMIT,
};

const W: u32 = 12;
const H: u32 = 8;
const N: usize = (W * H) as usize;

const INDEX_CHANNEL: &str = "An Indexed Color document has one channel, Index: it has no red, green, blue or alpha plane to edit; target the layer.";

/// Black, white and (190, 50, 50) bands, a red marker at (1, 1) and a black
/// one at (10, 2); opaque, Exact, no dither.
fn scene() -> *mut PrecisionDocument {
    let mut pixels = Vec::new();
    for y in 0..H {
        for x in 0..W {
            pixels.push(if (x, y) == (1, 1) {
                [190, 50, 50, 255]
            } else if x < 4 || (x, y) == (10, 2) {
                [0, 0, 0, 255]
            } else if x < 8 {
                [255, 255, 255, 255]
            } else {
                [190, 50, 50, 255]
            });
        }
    }
    let rgb = rgb_doc(W, H, &pixels);
    let out = to_indexed(rgb, &json!({"palette":"exact","dither":"none"})).unwrap();
    free(rgb);
    out
}

fn select(doc: *const PrecisionDocument, operation: Value) -> Result<Vec<u16>, String> {
    let text = CString::new(json!({"op":"selection","operation":operation}).to_string()).unwrap();
    let mut out = vec![0u16; N];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if ok {
        Ok(out)
    } else {
        Err(take_err_string(err))
    }
}

fn read_channel(doc: *const PrecisionDocument, index: usize) -> Vec<u16> {
    let request = json!({"op":"read_plane","target":"channel","index":index});
    let text = CString::new(request.to_string()).unwrap();
    let mut out = vec![0u16; N];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_read_plane(
            doc,
            text.as_ptr(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    out
}

fn statistics(doc: *const PrecisionDocument, target: &str) -> Result<(), String> {
    let request = json!({"op":"scalar_statistics","target":target,"index":0});
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_scalar_statistics(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if raw.is_null() {
        return Err(take_err_string(err));
    }
    unsafe { doctored_core::ffi::dr_string_free(raw) };
    Ok(())
}

fn saved_and_opened(dir: &TempDir, doc: *const PrecisionDocument) -> *mut PrecisionDocument {
    let path = dir.path().join("tools.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "{}",
        take_err_string(err)
    );
    open_rgb(&path).unwrap()
}

#[test]
fn the_magic_wand_and_quick_selection_select_palette_colours() {
    let doc = scene();
    let plane = indices(doc);
    let render = render16(doc);
    let black = plane[0];
    // Not contiguous: every pixel of the entry.
    let all: Vec<u16> = plane
        .iter()
        .map(|&i| if i == black { 65535 } else { 0 })
        .collect();
    assert_eq!(
        select(
            doc,
            json!({"kind":"magic_wand","x":0,"y":0,"tolerance":0,"contiguous":false})
        ),
        Ok(all.clone())
    );
    // Contiguous: the band, without the isolated marker at (10, 2).
    let mut band = all;
    band[(2 * W + 10) as usize] = 0;
    assert_eq!(
        select(
            doc,
            json!({"kind":"magic_wand","x":0,"y":0,"tolerance":0,"contiguous":true})
        ),
        Ok(band)
    );
    let quick = select(doc, json!({"kind":"quick","params":{"seeds":[[0,0]]}})).unwrap();
    assert!(quick.iter().any(|&v| v > 0));
    assert_eq!(indices(doc), plane);
    assert_eq!(render16(doc), render);
    free(doc);
}

#[test]
fn the_channel_commands_keep_the_plane_and_survive_rz() {
    let dir = TempDir::new().unwrap();
    let doc = scene();
    let plane = indices(doc);
    let render = render16(doc);
    let table = color_table(doc);
    let samples: Vec<u16> = (0..N as u32).map(|i| (i * 683) as u16).collect();
    let provenance = json!({"version":1,"kind":"camera_auxiliary","name":"Portrait Matte",
        "source_format":"one_component16","source_bits":16,"source_width":W,
        "source_height":H,"precision_basis":"decoded_auxiliary_buffer",
        "encoded_precision":"not_inspected","exif_orientation":1,"orientation_applied":true,
        "coverage_mapping":"identity_unorm","alignment":"oriented_full_image_bilinear"})
    .to_string();
    let mut current = paint(
        doc,
        &json!({"op":"import_channel","name":"Matte","width":W,"height":H,
                "provenance":provenance}),
        &samples,
    )
    .unwrap()
    .expect("an imported channel");
    let steps = [
        json!({"op":"rename_channel","index":0,"name":"Fine"}),
        json!({"op":"duplicate_channel","index":0}),
        json!({"op":"channel_properties","index":1,"overlay_color":[17,83,149],
               "overlay_opacity":0.375,"color_indicates_selected":true,"name":"Copy"}),
        json!({"op":"scalar_edit","target":"channel","index":1,
               "operation":{"kind":"morphology","operation":{"kind":"invert"}}}),
        json!({"op":"remove_channel","index":0}),
    ];
    for step in steps {
        let next = paint(current, &step, &[])
            .unwrap_or_else(|e| panic!("{step}: {e}"))
            .unwrap_or_else(|| panic!("{step} changes the document"));
        assert!(unsafe { &*next }.is_indexed(), "{step}");
        assert_eq!(indices(next), plane, "{step}");
        assert_eq!(render16(next), render, "{step}");
        free(current);
        current = next;
    }
    // One channel left: the inverted copy.
    let inverted: Vec<u16> = samples.iter().map(|&v| 65535 - v).collect();
    assert_eq!(unsafe { &*current }.channels().len(), 1);
    assert_eq!(read_channel(current, 0), inverted);
    assert!(statistics(current, "channel").is_ok());
    let back = saved_and_opened(&dir, current);
    assert_eq!(indices(back), plane);
    assert_eq!(color_table(back), table);
    assert_eq!(read_channel(back, 0), inverted);
    free(back);
    free(current);
    free(doc);
}

#[test]
fn a_colour_target_refuses_with_the_one_channel_sentence() {
    let doc = scene();
    assert_eq!(statistics(doc, "red"), Err(INDEX_CHANNEL.into()));
    assert_eq!(
        paint(
            doc,
            &json!({"op":"scalar_edit","target":"red","index":0,
                    "operation":{"kind":"fill","value":0}}),
            &[]
        ),
        Err(INDEX_CHANNEL.into())
    );
    free(doc);
}

fn band_coverage(value: u16) -> Vec<u16> {
    (0..N as u32)
        .map(|i| if (4..8).contains(&(i % W)) { value } else { 0 })
        .collect()
}

#[test]
fn clone_stamp_copies_entries_and_reindexes_its_own_colour() {
    let doc = scene();
    let before = indices(doc);
    let (palette, t) = palette_of(doc);
    let request = json!({"op":"retouch_clone","layer":0,"matrix":[1,0,0,1,-4,0]});
    // Hard, full opacity: the black band copied onto the white one.
    let out = paint(doc, &request, &band_coverage(65535))
        .unwrap()
        .expect("the clone moves indices");
    let copied: Vec<i32> = (0..N)
        .map(|i| {
            if (4..8).contains(&(i as u32 % W)) {
                before[i - 4]
            } else {
                before[i]
            }
        })
        .collect();
    assert_eq!(indices(out), copied);
    free(out);
    // A soft stroke: the RGB clone re-indexed with the Own ink.
    let rgb = to_rgb(doc);
    let soft = band_coverage(30000);
    let twin = paint(rgb, &request, &soft).unwrap().unwrap();
    let expected = oracle_with(
        &palette,
        t,
        Some(&before),
        &window(twin),
        Ink::Own,
        [255, 255, 255],
    )
    .unwrap();
    let out = paint(doc, &request, &soft).unwrap().unwrap();
    assert_eq!(indices(out), expected);
    assert_ne!(indices(out), before);
    free(out);
    free(twin);
    free(rgb);
    free(doc);
}

#[test]
fn a_clone_onto_a_new_layer_refuses_as_layer_new() {
    let doc = scene();
    let before = indices(doc);
    assert_eq!(
        paint(
            doc,
            &json!({"op":"retouch_clone","layer":0,"matrix":[1,0,0,1,-4,0],
                    "output":"new_layer"}),
            &band_coverage(65535)
        ),
        Err("add_layer is not available in Indexed Color.".into())
    );
    assert_eq!(indices(doc), before);
    free(doc);
}

#[test]
fn the_history_brush_restores_the_earlier_plane() {
    let doc = scene();
    let before = indices(doc);
    let brush = json!({"op":"paint_blend","layer":0,
        "color":[220.0/255.0, 30.0/255.0, 30.0/255.0, 1.0],"color_space":"document",
        "opacity":1.0,"blend_mode":0});
    let painted = paint(doc, &brush, &band_coverage(65535))
        .unwrap()
        .expect("the stroke moves indices");
    assert_ne!(indices(painted), before);
    let full = [65535u16; N];
    let mut err = ptr::null_mut();
    let restored = unsafe {
        dr_precision_doc_history_brush(
            painted,
            0,
            doc,
            0,
            1.0,
            full.as_ptr(),
            full.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!restored.is_null(), "{}", take_err_string(err));
    assert!(unsafe { &*restored }.is_indexed());
    assert_eq!(indices(restored), before);
    assert_eq!(render16(restored), render16(doc));
    free(restored);
    free(painted);
    free(doc);
}

#[test]
fn retouch_from_refuses_as_content_aware_move() {
    let doc = scene();
    let request =
        CString::new(json!({"op":"retouch_clone","layer":0,"matrix":[1,0,0,1,-4,0]}).to_string())
            .unwrap();
    let coverage = band_coverage(65535);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_retouch_from(
            doc,
            doc,
            request.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        take_err_string(err),
        "content_aware_move is not available in Indexed Color."
    );
    free(doc);
}

#[test]
fn the_rest_refuses_for_good_by_name() {
    let doc = scene();
    for op in ["gradient", "retouch_brush", "dodge_burn", "retouch_heal"] {
        assert_eq!(
            paint(doc, &json!({"op":op,"layer":0}), &[]),
            Err(format!("{op} is not available in Indexed Color.")),
            "{op}"
        );
    }
    assert_eq!(
        paint(doc, &json!({"op":"arrange_layer","layer":0}), &[]),
        Err(
            "arrange_layer needs more than one layer, and an Indexed Color document holds one."
                .into()
        )
    );
    assert_eq!(
        paint(doc, &json!({"op":"layer_comp"}), &[]),
        Err(
            "layer_comp is unavailable in an Indexed Color document: Doctored offers it on RGB \
             documents only."
                .into()
        )
    );
    free(doc);
}

#[test]
fn the_render_is_each_index_through_the_palette() {
    let doc = scene();
    let (palette, _) = palette_of(doc);
    let expected: Vec<[u16; 4]> = indices(doc)
        .iter()
        .map(|&i| {
            let c = palette[i as usize];
            [
                u16::from(c[0]) * 257,
                u16::from(c[1]) * 257,
                u16::from(c[2]) * 257,
                65535,
            ]
        })
        .collect();
    assert_eq!(render16(doc), expected);
    let plane: Vec<u16> = (0..N as u32).map(|i| (i * 311) as u16).collect();
    let saved = paint(
        doc,
        &json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
        &plane,
    )
    .unwrap()
    .unwrap();
    assert_eq!(read_channel(saved, 0), plane);
    free(saved);
    free(doc);
}
