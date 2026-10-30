//! #1 Bitmap phase 2a-i through the FFI: painting in black and white and
//! the bit-exact geometry (`bitmap_edit`, the rule `bitmap_method::
//! white_over`).
//!
//! The oracles are independent of the adapter: literal bits on 4 × 1
//! documents for the rule (a soft edge, an opacity, a gray ink either side
//! of 128); for each paint op the expansion route (Image ▸ Mode ▸ Grayscale
//! at Size Ratio 1, the same request with the ink this file snaps by hand,
//! then this file's own threshold of its canvas window over the background,
//! written from the rule's definition); for the geometry the plane moved by
//! hand.

use std::ptr;

use doctored_core::ffi_guide::{
    dr_precision_doc_add_guide, dr_precision_doc_guide_count, dr_precision_doc_guide_orientation,
    dr_precision_doc_guide_position,
};
use doctored_core::ffi_precision::dr_precision_doc_save_native;
use doctored_core::ffi_precision_metadata::dr_precision_doc_set_resolution;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::bitmap_mode::{bits, gray_doc, to_bitmap, to_gray};
use crate::common::{cpath, take_err_string};
use crate::indexed_fixture::{descriptor, edit_plan, free, open_rgb, paint, window, LIMIT};

pub(crate) const W: u32 = 12;
pub(crate) const H: u32 = 8;

/// The adapter's declared bytes per output pixel (`bitmap_edit`).
const BYTES_PER_PIXEL: u64 = 34;

/// The 12 × 8 scene's bits: black left of x = 6, white right of it, with a
/// white marker at (2, 2) and a black one at (9, 5).
pub(crate) fn scene_bits() -> Vec<u8> {
    (0..H)
        .flat_map(|y| {
            (0..W).map(move |x| match (x, y) {
                (2, 2) => 1,
                (9, 5) => 0,
                _ => u8::from(x >= 6),
            })
        })
        .collect()
}

/// A Bitmap document of `bits` (1 white), by 50% Threshold from Gray.
pub(crate) fn bitmap_of(width: u32, height: u32, bits: &[u8]) -> *mut PrecisionDocument {
    let codes: Vec<u8> = bits.iter().map(|&b| if b == 1 { 255 } else { 0 }).collect();
    let gray = gray_doc(width, height, &codes);
    let out = to_bitmap(gray, &json!({"method":"threshold"})).unwrap();
    free(gray);
    out
}

pub(crate) fn scene() -> *mut PrecisionDocument {
    bitmap_of(W, H, &scene_bits())
}

/// The rule from its definition: white iff `g·a + bg·(255 − a) ≥ 128·255`.
pub(crate) fn thresholded(pixels: &[[u8; 4]], background: u8) -> Vec<u8> {
    pixels
        .iter()
        .map(|p| {
            let (g, a) = (u32::from(p[0]), u32::from(p[3]));
            u8::from(g * a + u32::from(background) * (255 - a) >= 128 * 255)
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

/// A light ink (it snaps to white) and a dark one (it snaps to black), and
/// the ink this file snaps each to.
fn light() -> Value {
    json!([0.9, 0.95, 1.0, 1.0])
}
fn dark() -> Value {
    json!([0.1, 0.05, 0.2, 1.0])
}
const WHITE: [f64; 4] = [1.0, 1.0, 1.0, 1.0];
const BLACK: [f64; 4] = [0.0, 0.0, 0.0, 1.0];

/// The black background key the host sends from its swatch.
pub(crate) fn black_key() -> Value {
    json!({"color":[0.0,0.0,0.0]})
}

/// One paint case: name, request, input, the snapped ink (None for the
/// erasers) and whether the background is black.
type PaintCase = (&'static str, Value, Vec<u16>, Option<[f64; 4]>, bool);

/// The paint requests of (b).
fn paint_cases() -> Vec<PaintCase> {
    let n = (W * H) as usize;
    let erase_native = json!({"op":"erase_native","layer":0,"opacity":1.0});
    let erase = json!({"op":"erase","layer":0,"opacity":1.0});
    let mut erase_native_black = erase_native.clone();
    erase_native_black["indexed_background"] = black_key();
    let mut erase_black = erase.clone();
    erase_black["indexed_background"] = black_key();
    vec![
        (
            "bucket_fill",
            json!({"op":"bucket_fill","layer":0,"color":light(),"color_space":"document",
                   "opacity":1.0,"blend_mode":0,"x":1,"y":1,"tolerance":0,"contiguous":true}),
            vec![65535; n],
            Some(WHITE),
            false,
        ),
        (
            "paint",
            json!({"op":"paint","layer":0,"color":light(),"color_space":"document","opacity":1.0}),
            disc(),
            Some(WHITE),
            false,
        ),
        (
            "fill",
            json!({"op":"paint_blend","layer":0,"color":dark(),"color_space":"srgb",
                   "opacity":0.8,"blend_mode":0}),
            vec![65535; n],
            Some(BLACK),
            false,
        ),
        (
            "stroke_layer_edge",
            json!({"op":"stroke_layer_edge","layer":0,"width":2.0,"location":"inside",
                   "color":dark(),"color_space":"document","opacity":1.0,"blend_mode":0}),
            vec![],
            Some(BLACK),
            false,
        ),
        ("erase_native", erase_native, disc(), None, false),
        ("erase", erase, disc(), None, false),
        (
            "erase_native over black",
            erase_native_black,
            disc(),
            None,
            true,
        ),
        ("erase over black", erase_black, disc(), None, true),
    ]
}

/// `request` as the Gray twin takes it: the snapped ink in document space,
/// the background key dropped (the twin is no Bitmap document).
fn twin_request(request: &Value, ink: Option<[f64; 4]>) -> Value {
    let mut twin = request.clone();
    if let Some(ink) = ink {
        twin["color"] = json!(ink);
        twin["color_space"] = json!("document");
    }
    twin.as_object_mut().unwrap().remove("indexed_background");
    twin
}

#[test]
fn the_rule_by_hand() {
    // A soft edge on black: coverage 1, 0.92, 0.09 and 0 of white paint the
    // grays 255, 233, 23 and 0: the first two turn white, the rest stay.
    let black = bitmap_of(4, 1, &[0, 0, 0, 0]);
    let soft = paint(
        black,
        &json!({"op":"paint","layer":0,"color":[1.0,1.0,1.0,1.0],"color_space":"document",
                "opacity":1.0}),
        &[65535, 60000, 6000, 0],
    )
    .unwrap()
    .unwrap();
    assert_eq!(bits(soft), vec![1, 1, 0, 0]);
    // An opacity of 0.8: white over black is 204 (white), black over white
    // is 51 (black).
    let half = bitmap_of(4, 1, &[0, 0, 1, 1]);
    let fill = |color: [f64; 4]| {
        json!({"op":"paint_blend","layer":0,"color":color,"color_space":"document",
               "opacity":0.8,"blend_mode":0})
    };
    let whitened = paint(half, &fill(WHITE), &[65535; 4]).unwrap().unwrap();
    assert_eq!(bits(whitened), vec![1, 1, 1, 1]);
    let blackened = paint(half, &fill(BLACK), &[65535; 4]).unwrap().unwrap();
    assert_eq!(bits(blackened), vec![0, 0, 0, 0]);
    // A gray ink either side of 128: 127 paints black, 128 white.
    let white = bitmap_of(4, 1, &[1, 1, 1, 1]);
    let gray_ink = |code: f64| {
        let v = code / 255.0;
        json!({"op":"paint_blend","layer":0,"color":[v,v,v,1.0],"color_space":"document",
               "opacity":1.0,"blend_mode":0})
    };
    let dark = paint(white, &gray_ink(127.0), &[65535; 4])
        .unwrap()
        .unwrap();
    assert_eq!(bits(dark), vec![0, 0, 0, 0]);
    let light = paint(black, &gray_ink(128.0), &[65535; 4])
        .unwrap()
        .unwrap();
    assert_eq!(bits(light), vec![1, 1, 1, 1]);
    for d in [light, dark, white, blackened, whitened, half, soft, black] {
        free(d);
    }
}

#[test]
fn the_threshold_boundary_at_half_opacity() {
    // The Normal blend over an opaque pixel is `a·ink + (1 − a)·old`, stored
    // as round(v · 65535); `code8` is (v + 128) / 257.
    let fill = |color: [f64; 4], opacity: f64| {
        json!({"op":"paint_blend","layer":0,"color":color,"color_space":"document",
               "opacity":opacity,"blend_mode":0})
    };
    let half = bitmap_of(4, 1, &[0, 0, 1, 1]);
    // 0.5: white over black is 0.5 → u16 32768 (32767.5 rounds up) → code
    // 128, white; white stays white.
    let white_half = paint(half, &fill(WHITE, 0.5), &[65535; 4])
        .unwrap()
        .unwrap();
    assert_eq!(bits(white_half), vec![1, 1, 1, 1]);
    // 0.5: black over white is also 0.5 → 32768 → 128, still white; black
    // stays black, so no bit moves.
    assert_eq!(paint(half, &fill(BLACK, 0.5), &[65535; 4]), Ok(None));
    // 0.51 (f32 0.50999999): white over black is 33422.85 → 33423 → code
    // 130, white.
    let white_more = paint(half, &fill(WHITE, 0.51), &[65535; 4])
        .unwrap()
        .unwrap();
    assert_eq!(bits(white_more), vec![1, 1, 1, 1]);
    // 0.51: black over white is 0.49 · 65535 = 32112.15 → 32112 → code
    // 125, black.
    let black_more = paint(half, &fill(BLACK, 0.51), &[65535; 4])
        .unwrap()
        .unwrap();
    assert_eq!(bits(black_more), vec![0, 0, 0, 0]);
    for d in [black_more, white_more, white_half, half] {
        free(d);
    }
}

#[test]
fn darken_and_lighten_by_hand() {
    // Phase 2b's walk admits Darken (6) and Lighten (7) beside Normal:
    // Darken keeps the darker of ink and pixel, Lighten the lighter, so a
    // black Darken and a white Lighten paint everything, and a white Darken
    // and a black Lighten move no bit (NULL, no error).
    let fill = |color: [f64; 4], mode: i64| {
        json!({"op":"paint_blend","layer":0,"color":color,"color_space":"document",
               "opacity":1.0,"blend_mode":mode})
    };
    let half = bitmap_of(4, 1, &[0, 0, 1, 1]);
    let darkened = paint(half, &fill(BLACK, 6), &[65535; 4]).unwrap().unwrap();
    assert_eq!(bits(darkened), vec![0, 0, 0, 0]);
    let lightened = paint(half, &fill(WHITE, 7), &[65535; 4]).unwrap().unwrap();
    assert_eq!(bits(lightened), vec![1, 1, 1, 1]);
    assert_eq!(paint(half, &fill(WHITE, 6), &[65535; 4]), Ok(None));
    assert_eq!(paint(half, &fill(BLACK, 7), &[65535; 4]), Ok(None));
    for d in [lightened, darkened, half] {
        free(d);
    }
}

#[test]
fn each_paint_op_is_the_gray_op_thresholded() {
    let doc = scene();
    let before = bits(doc);
    assert_eq!(before, scene_bits());
    let gray = to_gray(doc, &json!({})).unwrap();
    for (name, request, input, ink, black) in paint_cases() {
        let painted = paint(doc, &request, &input)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .unwrap_or_else(|| panic!("{name}: no bit moved"));
        let twin = paint(gray, &twin_request(&request, ink), &input)
            .unwrap_or_else(|e| panic!("{name} on Gray: {e}"))
            .unwrap_or_else(|| panic!("{name} on Gray: nothing changed"));
        let expected = thresholded(&window(twin), if black { 0 } else { 255 });
        let got = bits(painted);
        assert_eq!(got, expected, "{name}");
        assert_ne!(got, before, "{name}");
        let d = descriptor(painted);
        assert_eq!(
            (d["color_mode"].clone(), d["layer_count"].clone()),
            (json!("bitmap"), json!(1)),
            "{name}"
        );
        free(twin);
        free(painted);
    }
    free(gray);
    free(doc);
}

/// The plane `before` (`w × h`) moved onto `nw × nh`: each new pixel reads
/// the old one `source` names, `fill` where that is off the old canvas.
fn moved(
    before: &[u8],
    (w, h): (u32, u32),
    (nw, nh): (u32, u32),
    source: impl Fn(i64, i64) -> (i64, i64),
    fill: u8,
) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..i64::from(nh) {
        for x in 0..i64::from(nw) {
            let (sx, sy) = source(x, y);
            out.push(
                if sx >= 0 && sy >= 0 && sx < i64::from(w) && sy < i64::from(h) {
                    before[(sy * i64::from(w) + sx) as usize]
                } else {
                    fill
                },
            );
        }
    }
    out
}

/// The zero-angle crop past the canvas the Crop tool and MCP `crop` send:
/// 16 × 10 from (−2, −1), with `background` when given.
fn extended_crop(delete_cropped: bool, background: Option<Value>) -> Value {
    let mut request = json!({"op":"straighten","matrix":[1.0,0.0,0.0,1.0,0.0,0.0],
        "filter":"nearest","x":-2,"y":-1,"width":16,"height":10,
        "delete_cropped":delete_cropped});
    if let Some(background) = background {
        request["background"] = background;
    }
    request
}

#[test]
fn the_exact_geometry_moves_the_plane() {
    let doc = scene();
    let before = bits(doc);
    let (w, h) = (i64::from(W), i64::from(H));
    let size = (W, H);
    let mut canvas_black = json!({"op":"canvas_size","width":16,"height":10,"x":2,"y":1});
    canvas_black["indexed_background"] = black_key();
    type Map = Box<dyn Fn(i64, i64) -> (i64, i64)>;
    let shifted: fn() -> Map = || Box::new(|x, y| (x - 2, y - 1));
    let cases: Vec<(Value, (u32, u32), Map, u8)> = vec![
        (
            json!({"op":"rotate90"}),
            (H, W),
            Box::new(move |x, y| (y, h - 1 - x)),
            1,
        ),
        (
            json!({"op":"rotate180"}),
            size,
            Box::new(move |x, y| (w - 1 - x, h - 1 - y)),
            1,
        ),
        (
            json!({"op":"rotate270"}),
            (H, W),
            Box::new(move |x, y| (w - 1 - y, x)),
            1,
        ),
        (
            json!({"op":"flip_horizontal"}),
            size,
            Box::new(move |x, y| (w - 1 - x, y)),
            1,
        ),
        (
            json!({"op":"flip_vertical"}),
            size,
            Box::new(move |x, y| (x, h - 1 - y)),
            1,
        ),
        (
            json!({"op":"crop","x":2,"y":1,"width":6,"height":5}),
            (6, 5),
            Box::new(|x, y| (x + 2, y + 1)),
            1,
        ),
        (
            json!({"op":"canvas_size","width":16,"height":10,"x":2,"y":1}),
            (16, 10),
            shifted(),
            1,
        ),
        (canvas_black, (16, 10), shifted(), 0),
        (
            extended_crop(true, Some(black_key())),
            (16, 10),
            shifted(),
            0,
        ),
        (
            extended_crop(false, Some(black_key())),
            (16, 10),
            shifted(),
            0,
        ),
        (extended_crop(false, None), (16, 10), shifted(), 1),
    ];
    for (request, new_size, source, fill) in cases {
        let out = paint(doc, &request, &[])
            .unwrap_or_else(|e| panic!("{request}: {e}"))
            .unwrap_or_else(|| panic!("{request}: nothing moved"));
        assert_eq!(unsafe { &*out }.dimensions(), new_size, "{request}");
        assert_eq!(
            bits(out),
            moved(&before, size, new_size, source, fill),
            "{request}"
        );
        assert_eq!(descriptor(out)["color_mode"], json!("bitmap"), "{request}");
        free(out);
    }
    free(doc);
}

#[test]
fn the_refusals_name_their_tier() {
    let doc = scene();
    let blend = json!({"op":"paint_blend","layer":0,"color":light(),"color_space":"document",
                       "opacity":1.0,"blend_mode":1});
    let n = (W * H) as usize;
    // Phase 2b's walk: Fill takes Normal, Darken and Lighten in Bitmap mode;
    // Multiply (1), Dissolve (14) and Luminosity (26) refuse.
    for code in [1, 14, 26] {
        let mut request = blend.clone();
        request["blend_mode"] = json!(code);
        assert_eq!(
            paint(doc, &request, &vec![65535; n]).unwrap_err(),
            "The Brush, Fill, Stroke, the Paint Bucket, the Gradient and the Clone Stamp paint \
             in Normal, Darken or Lighten mode in a Bitmap document.",
            "{code}"
        );
    }
    let unoffered = |op: &str| {
        format!(
            "{op} is unavailable in a Bitmap document: Doctored offers it on Grayscale and \
             colour documents only; convert to Grayscale first."
        )
    };
    for (request, sentence) in [
        (
            json!({"op":"selection","operation":{"kind":"magic_wand"}}),
            "magic_wand is not available in Bitmap mode.".to_string(),
        ),
        (
            json!({"op":"selection","operation":{"kind":"color_range"}}),
            "color_range is not available in Bitmap mode.".to_string(),
        ),
        (
            json!({"op":"move_selected","layer":0,"dx":1,"dy":0,"target":"mask"}),
            unoffered("move_selected"),
        ),
        (
            json!({"op":"add_layer","name":"New","anchor":0,"position":"above"}),
            "add_layer is not available in Bitmap mode.".to_string(),
        ),
    ] {
        assert_eq!(
            paint(doc, &request, &[]).unwrap_err(),
            sentence,
            "{request}"
        );
    }
    free(doc);
}

#[test]
fn an_edit_that_moves_no_bit_is_null_with_no_error() {
    let black = bitmap_of(4, 1, &[0, 0, 0, 0]);
    let fill = |color: [f64; 4], opacity: f64| {
        json!({"op":"paint_blend","layer":0,"color":color,"color_space":"document",
               "opacity":opacity,"blend_mode":0})
    };
    // Black on black, and white at 40 % (gray 102) on black.
    assert_eq!(paint(black, &fill(BLACK, 1.0), &[65535; 4]), Ok(None));
    assert_eq!(paint(black, &fill(WHITE, 0.4), &[65535; 4]), Ok(None));
    free(black);
}

#[test]
fn a_symmetric_turn_still_swaps_the_resolution() {
    let white = bitmap_of(2, 2, &[1, 1, 1, 1]);
    let mut err = ptr::null_mut();
    let tall = unsafe { dr_precision_doc_set_resolution(white, 72.0, 144.0, &mut err) };
    assert!(!tall.is_null(), "{}", take_err_string(err));
    let turned = paint(tall, &json!({"op":"rotate90"}), &[])
        .unwrap()
        .expect("the resolution swaps, so the turn is an edit");
    let resolution = unsafe { &*turned }.resolution();
    assert_eq!((resolution.x, resolution.y), (144.0, 72.0));
    assert_eq!(bits(turned), vec![1, 1, 1, 1]);
    assert_eq!(descriptor(turned)["color_mode"], json!("bitmap"));
    for d in [turned, tall, white] {
        free(d);
    }
}

#[test]
fn a_uniform_flip_still_moves_its_guide() {
    let white = bitmap_of(10, 10, &[1; 100]);
    let guided = unsafe { dr_precision_doc_add_guide(white, 1, 2.0) };
    assert!(!guided.is_null());
    let flipped = paint(guided, &json!({"op":"flip_horizontal"}), &[])
        .unwrap()
        .expect("the guide moves, so the flip is an edit");
    assert_eq!(bits(flipped), vec![1; 100]);
    assert_eq!(unsafe { dr_precision_doc_guide_count(flipped) }, 1);
    assert_eq!(unsafe { dr_precision_doc_guide_orientation(flipped, 0) }, 1);
    assert_eq!(unsafe { dr_precision_doc_guide_position(flipped, 0) }, 8.0);
    for d in [flipped, guided, white] {
        free(d);
    }
}

fn scratch(plan: &Value) -> u64 {
    plan["scratch_bytes"].as_u64().unwrap()
}

#[test]
fn each_plan_covers_the_republish() {
    let doc = scene();
    let gray = to_gray(doc, &json!({})).unwrap();
    let canvas = u64::from(W * H);
    for (name, request, _, ink, _) in paint_cases() {
        let bitmap = edit_plan(doc, &request).unwrap();
        let twin = edit_plan(gray, &twin_request(&request, ink)).unwrap();
        assert_eq!(bitmap["input_samples"], twin["input_samples"], "{name}");
        let floor = scratch(&twin) + canvas * BYTES_PER_PIXEL;
        assert!(
            scratch(&bitmap) >= floor,
            "{name}: {} < {floor}",
            scratch(&bitmap)
        );
    }
    for (request, output) in [
        (json!({"op":"rotate90"}), canvas),
        (json!({"op":"crop","x":2,"y":1,"width":6,"height":5}), 30),
        (
            json!({"op":"canvas_size","width":16,"height":10,"x":2,"y":1}),
            160,
        ),
    ] {
        let bitmap = edit_plan(doc, &request).unwrap();
        let twin = edit_plan(gray, &request).unwrap();
        assert_eq!(bitmap["input_samples"], json!(0));
        let floor = scratch(&twin) + output * BYTES_PER_PIXEL;
        assert!(
            scratch(&bitmap) >= floor,
            "{request}: {} < {floor}",
            scratch(&bitmap)
        );
    }
    free(gray);
    free(doc);
}

#[test]
fn an_edited_document_round_trips_through_rz() {
    let dir = TempDir::new().unwrap();
    let doc = scene();
    let painted = paint(
        doc,
        &json!({"op":"paint","layer":0,"color":light(),"color_space":"document","opacity":1.0}),
        &disc(),
    )
    .unwrap()
    .unwrap();
    let turned = paint(painted, &json!({"op":"rotate90"}), &[])
        .unwrap()
        .unwrap();
    let path = dir.path().join("edited.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_save_native(turned, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
        },
        "{}",
        take_err_string(err)
    );
    let back = open_rgb(&path).unwrap();
    assert_eq!(descriptor(back)["color_mode"], json!("bitmap"));
    assert_eq!(unsafe { &*back }.dimensions(), (H, W));
    assert_eq!(bits(back), bits(turned));
    for d in [back, turned, painted, doc] {
        free(d);
    }
}
