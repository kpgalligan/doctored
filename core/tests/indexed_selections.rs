//! #1 Indexed Color phase 3b: the Select menu's source selections on an
//! Indexed document (`dr_precision_doc_select`: Color Range, Similar and
//! Grow, the alpha), an alpha channel's set and read (Save and Load
//! Selection, Quick Mask), and the commands that refuse for good (HDR
//! Toning, Auto-Blend Layers and, since phase 3c, Refine Edge).
//!
//! A colour selection samples the palette colours (an index's colour), so a
//! duplicate entry's pixels are selected with its twin's. The oracles are
//! masks built by hand from the index plane and the palette.

use std::collections::VecDeque;
use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_auto_blend::dr_precision_doc_auto_blend_plan;
use doctored_core::ffi_hdr_toning::dr_precision_doc_hdr_toning_plan;
use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::ffi_precision_edit::{dr_precision_doc_read_plane, dr_precision_doc_select};
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{
    color_table, free, indices, open_rgb, paint, palette_of, render16, rgb_doc, set_table,
    to_indexed, LIMIT,
};

const W: u32 = 12;
const H: u32 = 8;
const N: usize = (W * H) as usize;

const INDEX_CHANNEL: &str = "An Indexed Color document has one channel, Index: it has no red, green, blue or alpha plane to edit; target the layer.";

/// The 12 × 8 scene: black, white and (190, 50, 50) bands with two marker
/// pixels, its last row clear (the transparent index) when `clear_row`.
fn scene(clear_row: bool) -> *mut PrecisionDocument {
    let mut pixels = Vec::new();
    for y in 0..H {
        for x in 0..W {
            pixels.push(if clear_row && y == H - 1 {
                [0, 0, 0, 0]
            } else if (x, y) == (1, 1) {
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

/// The opaque scene with entry 1 a duplicate of entry 0 and an unused white
/// entry 3.
fn duplicate_opaque() -> *mut PrecisionDocument {
    let doc = scene(false);
    let out = set_table(
        doc,
        &json!({"colors":[[0,0,0],[0,0,0],[190,50,50],[255,255,255]],"transparent_index":null}),
    )
    .unwrap()
    .unwrap();
    free(doc);
    out
}

fn select(
    doc: *const PrecisionDocument,
    request: &Value,
    input: &[u16],
) -> Result<Vec<u16>, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut out = vec![0u16; N];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_precision_doc_select(
            doc,
            text.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
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

fn kind(operation: Value) -> Value {
    json!({"op":"selection","operation":operation})
}

fn read_plane(doc: *const PrecisionDocument, target: &str) -> Result<Vec<u16>, String> {
    let request = json!({"op":"read_plane","target":target,"index":0});
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
    if ok {
        Ok(out)
    } else {
        Err(take_err_string(err))
    }
}

/// Each pixel's palette colour (its index's entry).
fn colours(doc: *const PrecisionDocument) -> Vec<[u8; 3]> {
    let (palette, _) = palette_of(doc);
    indices(doc).iter().map(|&i| palette[i as usize]).collect()
}

/// 65535 where `keep` holds, else 0.
fn mask(keep: impl Fn(usize) -> bool) -> Vec<u16> {
    (0..N).map(|i| if keep(i) { 65535 } else { 0 }).collect()
}

#[test]
fn color_range_samples_the_palette_colours() {
    let doc = duplicate_opaque();
    let (palette, _) = palette_of(doc);
    let plane = indices(doc);
    assert!(
        plane.contains(&0) && plane.contains(&1),
        "both twins in use"
    );
    let wide = palette[0].map(|c| u32::from(c) * 257);
    let request = kind(json!({"kind":"color_range","params":{"mode":"sampled",
        "samples":[wide],"tolerance":0,"softness":0}}));
    let got = select(doc, &request, &[]).unwrap();
    // Entry 0's pixels and its duplicate entry 1's: the same colour.
    let want = mask(|i| plane[i] == 0 || plane[i] == 1);
    assert_eq!(got, want);
    free(doc);
}

/// The pixels of `start`'s colour, 4-connected to it when `contiguous`.
fn same_colour(colours: &[[u8; 3]], start: usize, contiguous: bool) -> Vec<u16> {
    let c = colours[start];
    if !contiguous {
        return mask(|i| colours[i] == c);
    }
    let mut out = vec![0u16; N];
    out[start] = 65535;
    let mut queue = VecDeque::from([start]);
    while let Some(i) = queue.pop_front() {
        let (x, y) = ((i as u32 % W) as i64, (i as u32 / W) as i64);
        for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
            if nx < 0 || ny < 0 || nx >= i64::from(W) || ny >= i64::from(H) {
                continue;
            }
            let j = (ny * i64::from(W) + nx) as usize;
            if out[j] == 0 && colours[j] == c {
                out[j] = 65535;
                queue.push_back(j);
            }
        }
    }
    out
}

#[test]
fn similar_and_grow_sample_the_palette_colours() {
    let doc = duplicate_opaque();
    let colours = colours(doc);
    // One reference pixel, (0, 0): black, entry 0; the white band is entry 1,
    // black too, and the marker (10, 2) black but apart.
    let mut reference = vec![0u16; N];
    reference[0] = 65535;
    for contiguous in [true, false] {
        let request = kind(json!({"kind":"similar","tolerance":0,"contiguous":contiguous}));
        let got = select(doc, &request, &reference).unwrap();
        assert_eq!(
            got,
            same_colour(&colours, 0, contiguous),
            "contiguous {contiguous}"
        );
    }
    let marker = (2 * W + 10) as usize;
    let grow = select(
        doc,
        &kind(json!({"kind":"similar","tolerance":0,"contiguous":true})),
        &reference,
    )
    .unwrap();
    assert_eq!(grow[marker], 0, "the marker is not contiguous");
    free(doc);
}

#[test]
fn the_alpha_is_opaque_off_the_transparent_index() {
    let doc = scene(true);
    let (_, t) = palette_of(doc);
    let t = i32::from(t.expect("the clear row is the transparent index"));
    let plane = indices(doc);
    let want = mask(|i| plane[i] != t);
    let layer = json!({"op":"selection","source":"layer","layer":0,
                       "operation":{"kind":"alpha"}});
    assert_eq!(select(doc, &layer, &[]).unwrap(), want, "layer");
    assert_eq!(
        select(doc, &kind(json!({"kind":"alpha"})), &[]).unwrap(),
        want,
        "composite"
    );
    free(doc);
}

#[test]
fn a_colour_channel_and_the_rest_refuse_by_name() {
    let doc = scene(true);
    for operation in [
        json!({"kind":"color","channel":"red"}),
        json!({"kind":"luminance"}),
    ] {
        assert_eq!(
            select(doc, &kind(operation.clone()), &[]),
            Err(INDEX_CHANNEL.into()),
            "{operation}"
        );
    }
    assert_eq!(read_plane(doc, "red"), Err(INDEX_CHANNEL.into()));
    // Phase 3c: the Magic Wand selects (`indexed_tools.rs`); Refine Edge
    // refuses for good.
    assert_eq!(
        select(
            doc,
            &kind(json!({"kind":"refine_edge","edge_radius":8,"smart_radius":true})),
            &[]
        ),
        Err("refine_edge is not available in Indexed Color.".into())
    );
    free(doc);
}

#[test]
fn an_alpha_channel_is_saved_read_and_kept_by_rz() {
    let dir = TempDir::new().unwrap();
    let doc = scene(true);
    let plane: Vec<u16> = (0..N as u32).map(|i| (i * 683) as u16).collect();
    let saved = paint(
        doc,
        &json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
        &plane,
    )
    .unwrap()
    .expect("a new channel");
    assert!(unsafe { &*saved }.is_indexed());
    assert_eq!(indices(saved), indices(doc));
    assert_eq!(render16(saved), render16(doc));
    assert_eq!(read_plane(saved, "channel").unwrap(), plane);
    let path = dir.path().join("channel.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_save_native(saved, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "{}",
        take_err_string(err)
    );
    let back = open_rgb(&path).unwrap();
    assert_eq!(indices(back), indices(doc));
    assert_eq!(color_table(back), color_table(doc));
    assert_eq!(read_plane(back, "channel").unwrap(), plane);
    free(back);
    free(saved);
    free(doc);
}

#[test]
fn hdr_toning_and_auto_blend_refuse_for_good() {
    let doc = scene(true);
    let request = CString::new(r#"{"method":"highlight_compression"}"#).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_hdr_toning_plan(doc, request.as_ptr(), 16, &mut err) };
    assert!(plan.is_null());
    assert_eq!(
        take_err_string(err),
        "hdr_toning is not available in Indexed Color."
    );
    let request = CString::new(r#"{"layers":[0],"seamless":false}"#).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe { dr_precision_doc_auto_blend_plan(doc, request.as_ptr(), &mut err) };
    assert!(plan.is_null());
    assert_eq!(
        take_err_string(err),
        "auto_blend_layers needs more than one layer, and an Indexed Color document holds one."
    );
    free(doc);
}
