//! #1 Indexed Color phase 3a: the Move tool (`affine_set` translations,
//! `move_selected`), the layer row's visibility and order, the selected
//! pixels of an alpha channel, the background rule on a document with no
//! transparent index, and the refusals phase 3a decides for good
//! (`indexed_transform`, `indexed_geometry`, `indexed_edge::clear_index`,
//! `indexed_export`, `indexed_document`).
//!
//! The oracles are independent of the adapters: for a move the index plane
//! moved by hand (on a palette with a duplicate entry, which no colour
//! lookup could keep), each cleared pixel the entry nearest white computed
//! here by the fixture's own search; for a feathered move the expansion
//! route (Image ▸ Mode ▸ RGB Color, the same request on that RGB document,
//! then the fixture's own re-index); for an alpha channel the same request
//! on the RGB document.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_clipboard::{dr_clipboard_payload_free, dr_precision_doc_clipboard_cut};
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::ffi_puppet::dr_precision_doc_puppet_warp_plan;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{
    edit_plan, export, free, indices, layer16, nearest, open_rgb, oracle, paint, palette_of,
    render16, rgb_doc, set_table, to_indexed, to_rgb, window, Ink, LIMIT,
};

const W: u32 = 12;
const H: u32 = 8;

const HIDDEN: &str =
    "The Index layer is hidden: show it before exporting or saving a copy that keeps Indexed Color.";

const INDEX_CHANNEL: &str = "An Indexed Color document has one channel, Index: it has no red, green, blue or alpha plane to edit; target the layer.";

/// The 12 × 8 scene: black, white and (190, 50, 50) bands with two marker
/// pixels, its last row clear when `clear_row` (the transparent index).
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

fn retabled(doc: *mut PrecisionDocument, table: Value) -> *mut PrecisionDocument {
    let out = set_table(doc, &table).unwrap().unwrap();
    free(doc);
    out
}

/// The clear-row scene with entry 1 (the white band) a duplicate of entry 0
/// and t = 3.
fn duplicate_t() -> *mut PrecisionDocument {
    retabled(
        scene(true),
        json!({"colors":[[0,0,0],[0,0,0],[190,50,50],[255,255,255]],"transparent_index":3}),
    )
}

/// The clear-row scene with distinct colours, t (3) unique.
fn distinct_t() -> *mut PrecisionDocument {
    retabled(
        scene(true),
        json!({"colors":[[0,0,0],[255,255,255],[190,50,50],[9,9,9]],"transparent_index":3}),
    )
}

/// The opaque scene with no transparent index, entry 1 a duplicate of 0 and
/// no white entry: the entry nearest white is the pale 3.
fn no_t() -> *mut PrecisionDocument {
    retabled(
        scene(false),
        json!({"colors":[[0,0,0],[0,0,0],[190,50,50],[230,240,250]],"transparent_index":null}),
    )
}

/// The layer of an Indexed document is its palette's render of its plane.
fn assert_renders(doc: *const PrecisionDocument) {
    let (palette, t) = palette_of(doc);
    for (pixel, &index) in layer16(doc).iter().zip(&indices(doc)) {
        let c = palette[index as usize];
        let alpha = if Some(index as u8) == t { 0 } else { 65535 };
        assert_eq!(
            *pixel,
            [
                u16::from(c[0]) * 257,
                u16::from(c[1]) * 257,
                u16::from(c[2]) * 257,
                alpha
            ]
        );
    }
}

/// The edit's result, which must exist.
fn edited(doc: *const PrecisionDocument, request: &Value, input: &[u16]) -> *mut PrecisionDocument {
    paint(doc, request, input)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request}: nothing moved"))
}

/// `before` translated by (dx, dy) on the canvas, `fill` where nothing lands.
fn translated(before: &[i32], (w, h): (u32, u32), dx: i64, dy: i64, fill: i32) -> Vec<i32> {
    let (w, h) = (i64::from(w), i64::from(h));
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = (x - dx, y - dy);
            out.push(if sx >= 0 && sy >= 0 && sx < w && sy < h {
                before[(sy * w + sx) as usize]
            } else {
                fill
            });
        }
    }
    out
}

/// A canvas coverage of 65535 inside `[x0, x1) × [y0, y1)`, else 0.
fn rect(x0: u32, y0: u32, x1: u32, y1: u32) -> Vec<u16> {
    (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .map(|(x, y)| {
            if (x0..x1).contains(&x) && (y0..y1).contains(&y) {
                65535
            } else {
                0
            }
        })
        .collect()
}

/// A graded disc of coverage around (6, 4), radius 3.5, ramping over 2 px.
fn disc() -> Vec<u16> {
    let mut out = Vec::new();
    for y in 0..H {
        for x in 0..W {
            let d = ((f64::from(x) - 6.0).powi(2) + (f64::from(y) - 4.0).powi(2)).sqrt();
            out.push((((3.5 - d) / 2.0).clamp(0.0, 1.0) * 65535.0).round() as u16);
        }
    }
    out
}

/// A hard selection's move by hand: the hole takes `hole` (none with
/// `duplicate`), then each selected source pixel lands at (+dx, +dy) on the
/// canvas unless it is t, which lands nothing.
fn moved_selection(
    before: &[i32],
    selected: &[u16],
    (dx, dy): (i64, i64),
    t: Option<u8>,
    hole: i32,
    duplicate: bool,
) -> Vec<i32> {
    let (w, h) = (i64::from(W), i64::from(H));
    let mut out = before.to_vec();
    if !duplicate {
        for (i, &c) in selected.iter().enumerate() {
            if c == 65535 {
                out[i] = hole;
            }
        }
    }
    for (i, &c) in selected.iter().enumerate() {
        let (x, y) = (i as i64 % w, i as i64 / w);
        let (nx, ny) = (x + dx, y + dy);
        if c == 65535 && Some(before[i] as u8) != t && nx >= 0 && ny >= 0 && nx < w && ny < h {
            out[(ny * w + nx) as usize] = before[i];
        }
    }
    out
}

fn move_selected(dx: i32, dy: i32, duplicate: bool) -> Value {
    json!({"op":"move_selected","layer":0,"dx":dx,"dy":dy,"duplicate":duplicate})
}

#[test]
fn moves_keep_every_index_exactly() {
    let doc = duplicate_t();
    let (_, t) = palette_of(doc);
    assert_eq!(t, Some(3));
    let before = indices(doc);
    assert!(before.contains(&1), "the duplicate entry is in use");
    // `move_layers` (the Move tool): a whole-pixel `affine_set`, run Nearest.
    let out = edited(
        doc,
        &json!({"op":"affine_set","layers":[0],"matrix":[1.0,0.0,0.0,1.0,3.0,1.0],
                "filter":"nearest"}),
        &[],
    );
    assert_eq!(indices(out), translated(&before, (W, H), 3, 1, 3));
    assert_eq!(unsafe { &*out }.layer(0).unwrap().offset(), (0, 0));
    assert_renders(out);
    free(out);
    // The row's offset: `affine_set` with links not followed.
    let out = edited(
        doc,
        &json!({"op":"affine_set","layers":[0],"matrix":[1.0,0.0,0.0,1.0,-2.0,0.0],
                "filter":"nearest","follow_links":false}),
        &[],
    );
    assert_eq!(indices(out), translated(&before, (W, H), -2, 0, 3));
    assert_eq!(unsafe { &*out }.layer(0).unwrap().offset(), (0, 0));
    assert_renders(out);
    free(out);
    // The Move tool with a hard selection, with and without ⌥ (duplicate),
    // once across t's row (a float of t lands nothing).
    for (selection, (dx, dy)) in [(rect(2, 1, 6, 4), (3, 1)), (rect(2, 5, 6, 8), (3, -2))] {
        for duplicate in [false, true] {
            let request = move_selected(dx as i32, dy as i32, duplicate);
            let out = edited(doc, &request, &selection);
            assert_eq!(
                indices(out),
                moved_selection(&before, &selection, (dx, dy), t, 3, duplicate),
                "{request}"
            );
            assert_eq!(unsafe { &*out }.layer(0).unwrap().offset(), (0, 0));
            assert_renders(out);
            free(out);
        }
    }
    free(doc);
}

#[test]
fn a_feathered_move_is_the_rgb_move_reindexed() {
    let doc = distinct_t();
    let (palette, t) = palette_of(doc);
    let before = indices(doc);
    let coverage = disc();
    let request = move_selected(2, 1, false);
    let out = edited(doc, &request, &coverage);
    let rgb = to_rgb(doc);
    let twin = edited(rgb, &request, &coverage);
    let expected = oracle(&palette, t, Some(&before), &window(twin), Ink::Own).unwrap();
    assert_eq!(indices(out), expected);
    assert_renders(out);
    free(twin);
    free(rgb);
    free(out);
    free(doc);
}

/// `dr_precision_doc_clipboard_cut` of the whole canvas under `coverage`:
/// the new document.
fn cut(doc: *const PrecisionDocument, coverage: &[u16]) -> *mut PrecisionDocument {
    let text =
        CString::new(json!({"op":"cut","layer":0,"x":0,"y":0,"width":W,"height":H}).to_string())
            .unwrap();
    let mut err = ptr::null_mut();
    let mut payload = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_clipboard_cut(
            doc,
            text.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut payload,
            &mut err,
        )
    };
    assert!(err.is_null(), "{}", take_err_string(err));
    unsafe { dr_clipboard_payload_free(payload) };
    assert!(!out.is_null(), "the cut moves indices");
    out
}

#[test]
fn a_clear_pixel_takes_the_entry_nearest_white_without_t() {
    let doc = no_t();
    let (palette, t) = palette_of(doc);
    assert_eq!(t, None);
    // Computed here: the entry nearest white, never a transparent index.
    let bg = nearest(&palette, None, [255, 255, 255]);
    assert_eq!(bg, 3);
    let before = indices(doc);
    // The erasers: the RGB erase re-indexed; every pixel left clear is bg.
    for request in [
        json!({"op":"erase_native","layer":0,"opacity":1.0}),
        json!({"op":"erase","layer":0,"opacity":1.0}),
    ] {
        let out = edited(doc, &request, &disc());
        let rgb = to_rgb(doc);
        let twin = edited(rgb, &request, &disc());
        let after = window(twin);
        let expected = oracle(&palette, t, Some(&before), &after, Ink::Clear).unwrap();
        let got = indices(out);
        assert_eq!(got, expected, "{request}");
        for (i, px) in after.iter().enumerate() {
            if px[3] < 128 {
                assert_eq!(got[i], bg, "{request}: pixel {i}");
            }
        }
        assert!(after.iter().any(|px| px[3] < 128), "{request} clears");
        assert_renders(out);
        free(twin);
        free(rgb);
        free(out);
    }
    // Canvas Size growth: the new area is bg.
    let out = edited(
        doc,
        &json!({"op":"canvas_size","width":14,"height":10,"x":1,"y":1}),
        &[],
    );
    let grown: Vec<i32> = (0..10i64)
        .flat_map(|y| (0..14i64).map(move |x| (x, y)))
        .map(|(x, y)| {
            let (sx, sy) = (x - 1, y - 1);
            if (0..i64::from(W)).contains(&sx) && (0..i64::from(H)).contains(&sy) {
                before[(sy * i64::from(W) + sx) as usize]
            } else {
                bg
            }
        })
        .collect();
    assert_eq!(indices(out), grown);
    assert_renders(out);
    free(out);
    // A layer translation: the uncovered area is bg.
    let out = edited(
        doc,
        &json!({"op":"affine","layer":0,"target":"pixels",
                "matrix":[1.0,0.0,0.0,1.0,2.0,1.0],"filter":"nearest"}),
        &[],
    );
    assert_eq!(indices(out), translated(&before, (W, H), 2, 1, bg));
    assert_renders(out);
    free(out);
    // The Move tool's hole is bg.
    let selection = rect(4, 2, 8, 5);
    let out = edited(doc, &move_selected(3, 1, false), &selection);
    assert_eq!(
        indices(out),
        moved_selection(&before, &selection, (3, 1), None, bg, false)
    );
    assert_renders(out);
    free(out);
    // Cut: every cut pixel is bg.
    let out = cut(doc, &selection);
    let expected: Vec<i32> = before
        .iter()
        .zip(&selection)
        .map(|(&i, &c)| if c == 65535 { bg } else { i })
        .collect();
    assert_eq!(indices(out), expected);
    assert_renders(out);
    free(out);
    free(doc);
}

/// Every alpha channel's samples.
fn channels(doc: *const PrecisionDocument) -> Vec<Vec<u16>> {
    unsafe { &*doc }
        .channels()
        .iter()
        .map(|c| c.plane().samples().to_vec())
        .collect()
}

/// The scene converted with an alpha channel of a diagonal ramp.
fn with_channel() -> *mut PrecisionDocument {
    let mut pixels = Vec::new();
    for y in 0..H {
        for x in 0..W {
            pixels.push(if x < 6 {
                [0, 0, 0, 255]
            } else if y < 4 {
                [255, 255, 255, 255]
            } else {
                [190, 50, 50, 255]
            });
        }
    }
    let rgb = rgb_doc(W, H, &pixels);
    let plane: Vec<u16> = (0..W * H)
        .map(|i| (((i % W) + (i / W)) * 65535 / (W + H - 2)) as u16)
        .collect();
    let channelled = edited(
        rgb,
        &json!({"op":"set_channel","index":null,"name":"Alpha 1"}),
        &plane,
    );
    free(rgb);
    let out = to_indexed(channelled, &json!({"palette":"exact","dither":"none"})).unwrap();
    free(channelled);
    out
}

#[test]
fn an_alpha_channel_moves_and_transforms_as_on_rgb() {
    let doc = with_channel();
    assert_eq!(channels(doc).len(), 1, "the conversion carries the channel");
    let rgb = to_rgb(doc);
    let coverage = disc();
    for request in [
        json!({"op":"move_selected","layer":0,"dx":2,"dy":1,"target":{"channel":0}}),
        json!({"op":"transform_selected","layer":0,"target":{"channel":0},
                "matrix":[0.9,0.1,-0.1,0.9,1.0,0.5],"filter":"bilinear"}),
    ] {
        let out = edited(doc, &request, &coverage);
        let twin = edited(rgb, &request, &coverage);
        assert_eq!(channels(out), channels(twin), "{request}");
        assert_ne!(channels(out), channels(doc), "{request}");
        assert_eq!(indices(out), indices(doc), "{request}");
        assert_eq!(render16(out), render16(doc), "{request}");
        assert!(unsafe { &*out }.is_indexed());
        free(twin);
        free(out);
    }
    free(rgb);
    // A mask target refuses for good; a colour plane has no channel to float.
    for op in ["move_selected", "transform_selected"] {
        let mut request = json!({"op":op,"layer":0,"target":"mask","dx":1,"dy":0,
                                 "matrix":[1.0,0.0,0.0,1.0,1.0,0.0],"filter":"nearest"});
        let fields = |r: &mut Value| {
            let object = r.as_object_mut().unwrap();
            if op == "move_selected" {
                object.remove("matrix");
                object.remove("filter");
            } else {
                object.remove("dx");
                object.remove("dy");
            }
        };
        fields(&mut request);
        assert_eq!(
            paint(doc, &request, &coverage),
            Err("transform_mask is not available in Indexed Color.".into()),
            "{request}"
        );
        for target in ["red", "green", "blue", "alpha"] {
            request["target"] = json!(target);
            assert_eq!(
                paint(doc, &request, &coverage),
                Err(INDEX_CHANNEL.into()),
                "{request}"
            );
        }
    }
    free(doc);
}

#[test]
fn the_row_hides_and_keeps_its_place() {
    let dir = TempDir::new().unwrap();
    let doc = distinct_t();
    let hidden = edited(
        doc,
        &json!({"op":"layer_visible","layer":0,"visible":false}),
        &[],
    );
    assert!(!unsafe { &*hidden }.layer(0).unwrap().visible());
    assert_eq!(indices(hidden), indices(doc));
    assert_eq!(layer16(hidden), layer16(doc));
    assert!(render16(hidden).iter().all(|px| px[3] == 0));
    // A kept-Indexed export refuses while it is hidden.
    let request = json!({"format":"png","profile":"document","dither":"none"}).to_string();
    assert_eq!(export(hidden, &request).err(), Some(HIDDEN.into()));
    assert!(export(doc, &request).is_ok());
    // `.rz` keeps it hidden.
    let path = dir.path().join("hidden.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(hidden, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let back = open_rgb(&path).unwrap();
    assert!(unsafe { &*back }.is_indexed());
    assert!(!unsafe { &*back }.layer(0).unwrap().visible());
    assert_eq!(indices(back), indices(doc));
    free(back);
    let shown = edited(
        hidden,
        &json!({"op":"layer_visible","layer":0,"visible":true}),
        &[],
    );
    assert_eq!(render16(shown), render16(doc));
    free(shown);
    free(hidden);
    // The one layer is already in its place.
    assert_eq!(
        paint(
            doc,
            &json!({"op":"move_layer_to","from":0,"to":0,"depth":0}),
            &[]
        ),
        Ok(None)
    );
    free(doc);
}

#[test]
fn one_layer_and_permanent_refusals_name_their_reason() {
    let doc = distinct_t();
    for (request, op) in [
        (json!({"op":"align_set","layers":[0]}), "align_set"),
        (
            json!({"op":"distribute_set","layers":[0]}),
            "distribute_set",
        ),
        (json!({"op":"remove_layers","layers":[0]}), "remove_layers"),
        (json!({"op":"flatten"}), "flatten"),
    ] {
        assert_eq!(
            paint(doc, &request, &[]),
            Err(format!(
                "{op} needs more than one layer, and an Indexed Color document holds one."
            ))
        );
    }
    assert_eq!(
        paint(
            doc,
            &json!({"op":"puppet_selected","layer":0}),
            &rect(0, 0, 2, 2)
        ),
        Err("puppet_selected is not available in Indexed Color.".into())
    );
    let layers = rgb_doc(2, 2, &[[1, 2, 3, 255]; 4]);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_paste_layers(
            doc,
            layers,
            0,
            0,
            0,
            0,
            false,
            LIMIT,
            0,
            ptr::null_mut(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        take_err_string(err),
        "paste_layers is not available in Indexed Color."
    );
    free(layers);
    let puppet = CString::new(
        json!({"op":"puppet_warp","layer":0,"mode":"normal","pins":[
            {"from":[3.0,3.0],"to":[4.0,3.0]},{"from":[8.0,4.0],"to":[8.0,4.0]}]})
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let plan =
        unsafe { dr_precision_doc_puppet_warp_plan(doc, puppet.as_ptr(), LIMIT, 0, &mut err) };
    assert!(plan.is_null());
    assert_eq!(
        take_err_string(err),
        "puppet_warp is not available in Indexed Color."
    );
    // Phase 3c: Clone Stamp paints in place; onto a new layer it refuses as
    // Layer ▸ New does.
    assert_eq!(
        paint(
            doc,
            &json!({"op":"retouch_clone","layer":0,"output":"new_layer"}),
            &[]
        ),
        Err("add_layer is not available in Indexed Color.".into())
    );
    free(doc);
}

#[test]
fn the_move_plan_covers_its_term() {
    let doc = distinct_t();
    let rgb = to_rgb(doc);
    let canvas = u64::from(W * H);
    let request = move_selected(2, 1, false);
    let indexed = edit_plan(doc, &request).unwrap();
    let twin = edit_plan(rgb, &request).unwrap();
    // transform_scratch: (old + new canvas) × 21 B/px and the 32 MiB memo.
    let floor = twin["scratch_bytes"].as_u64().unwrap() + 2 * canvas * 21 + (1 << 24) * 2;
    let scratch = indexed["scratch_bytes"].as_u64().unwrap();
    assert!(scratch >= floor, "{scratch} < {floor}");
    assert_eq!(indexed["input_samples"], json!(canvas));
    free(rgb);
    free(doc);
}
