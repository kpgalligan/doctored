//! #547 Indexed Color phase 1a's palettes and dither, black-box through the
//! FFI against hand-built tables: an image holding a table's colours in its
//! order converts with Dither None to indices 0, 1, 2, … (each colour maps to
//! itself, the lowest index on a duplicate), so the tables below — Web's
//! 216-colour cube, System (Mac OS) and System (Windows) typed out, the
//! Uniform cubes with forced colours and Exact's first-occurrence order — are
//! checked entry by entry; the Uniform and Exact refusals by their sentences
//! and N; and Floyd–Steinberg against a 3×2 case computed by hand. Phase
//! 1b's Adaptive palette (median cut) is checked against splits and means
//! computed by hand, its stop, its padding and its refusal.

use serde_json::json;

use crate::indexed_fixture::{descriptor, free, indices, opaque, plan, rgb_doc, to_indexed};

/// The table `colors` as a 1-pixel-high image, converted under `options`
/// with Dither None: its indices and the palette size.
fn mapped(colors: &[[u8; 3]], mut options: serde_json::Value) -> (Vec<i32>, u64) {
    options["dither"] = json!("none");
    let doc = rgb_doc(colors.len() as u32, 1, &opaque(colors));
    let indexed = to_indexed(doc, &options).unwrap();
    let out = (
        indices(indexed),
        descriptor(indexed)["indexed"]["colors"].as_u64().unwrap(),
    );
    free(indexed);
    free(doc);
    out
}

/// The `k`-level cube, r slowest, levels round(255i/(k−1)) (hand-built).
fn cube(k: usize) -> Vec<[u8; 3]> {
    let levels: Vec<u8> = (0..k)
        .map(|i| (255.0 * i as f64 / (k - 1) as f64).round() as u8)
        .collect();
    let mut out = Vec::new();
    for &r in &levels {
        for &g in &levels {
            for &b in &levels {
                out.push([r, g, b]);
            }
        }
    }
    out
}

fn first_occurrence(table: &[[u8; 3]]) -> Vec<i32> {
    table
        .iter()
        .map(|c| table.iter().position(|e| e == c).unwrap() as i32)
        .collect()
}

#[test]
fn web_is_the_216_colour_cube() {
    // Entry 36r + 6g + b at level 51i.
    let mut web = Vec::new();
    for i in 0..216u32 {
        let (r, g, b) = (i / 36, i / 6 % 6, i % 6);
        web.push([(51 * r) as u8, (51 * g) as u8, (51 * b) as u8]);
    }
    let (got, colors) = mapped(&web, json!({"palette":"web"}));
    assert_eq!(colors, 216);
    assert_eq!(got, (0..216).collect::<Vec<i32>>());
}

#[test]
fn system_mac_os_is_its_typed_table() {
    let levels = [0xFF, 0xCC, 0x99, 0x66, 0x33, 0x00];
    let ramp = [0xEE, 0xDD, 0xBB, 0xAA, 0x88, 0x77, 0x55, 0x44, 0x22, 0x11];
    let mut mac: Vec<[u8; 3]> = Vec::new();
    for r in levels {
        for g in levels {
            for b in levels {
                mac.push([r, g, b]);
            }
        }
    }
    mac.pop(); // 000000 leaves the cube, for entry 255.
    assert_eq!(mac.len(), 215);
    mac.extend(ramp.map(|v| [v, 0, 0]));
    mac.extend(ramp.map(|v| [0, v, 0]));
    mac.extend(ramp.map(|v| [0, 0, v]));
    mac.extend(ramp.map(|v| [v, v, v]));
    mac.push([0, 0, 0]);
    let (got, colors) = mapped(&mac, json!({"palette":"system_mac"}));
    assert_eq!(colors, 256);
    assert_eq!(got, (0..256).collect::<Vec<i32>>());
}

#[test]
fn system_windows_is_its_typed_table() {
    let mut win: Vec<[u8; 3]> = vec![
        [0x00, 0x00, 0x00],
        [0x80, 0x00, 0x00],
        [0x00, 0x80, 0x00],
        [0x80, 0x80, 0x00],
        [0x00, 0x00, 0x80],
        [0x80, 0x00, 0x80],
        [0x00, 0x80, 0x80],
        [0xC0, 0xC0, 0xC0],
        [0xC0, 0xDC, 0xC0],
        [0xA6, 0xCA, 0xF0],
    ];
    win.extend(cube(6));
    win.extend(std::iter::repeat_n([0, 0, 0], 20));
    win.extend([
        [0xFF, 0xFB, 0xF0],
        [0xA0, 0xA0, 0xA4],
        [0x80, 0x80, 0x80],
        [0xFF, 0x00, 0x00],
        [0x00, 0xFF, 0x00],
        [0xFF, 0xFF, 0x00],
        [0x00, 0x00, 0xFF],
        [0xFF, 0x00, 0xFF],
        [0x00, 0xFF, 0xFF],
        [0xFF, 0xFF, 0xFF],
    ]);
    assert_eq!(win.len(), 256);
    let (got, colors) = mapped(&win, json!({"palette":"system_windows"}));
    assert_eq!(colors, 256);
    // A duplicate maps to its first entry: black is 0, never 10 or 226.
    assert_eq!(got, first_occurrence(&win));
}

#[test]
fn uniform_takes_the_largest_cube_that_fits() {
    // 256 colours: the 6-level cube; 100: the 4-level (5³ = 125 does not fit).
    let (got, colors) = mapped(&cube(6), json!({"palette":"uniform"}));
    assert_eq!((colors, got), (216, (0..216).collect::<Vec<i32>>()));
    let (got, colors) = mapped(&cube(4), json!({"palette":"uniform","colors":100}));
    assert_eq!((colors, got), (64, (0..64).collect::<Vec<i32>>()));
}

#[test]
fn uniform_with_forced_black_and_white_at_216_is_the_6_level_cube() {
    // t = 0, k = 6: black and white lead, then the cube without them.
    let mut table = vec![[0, 0, 0], [255, 255, 255]];
    table.extend(
        cube(6)
            .into_iter()
            .filter(|c| *c != [0, 0, 0] && *c != [255, 255, 255]),
    );
    assert_eq!(table.len(), 216);
    let (got, colors) = mapped(
        &table,
        json!({"palette":"uniform","colors":216,"forced":"black_white"}),
    );
    assert_eq!((colors, got), (216, (0..216).collect::<Vec<i32>>()));
}

#[test]
fn uniform_with_forced_primaries_leads_with_them() {
    // colors 30: 3³ = 27 holds the eight primaries, so 27 entries.
    let primaries: Vec<[u8; 3]> = vec![
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [0, 255, 255],
        [255, 0, 255],
        [255, 255, 0],
        [0, 0, 0],
        [255, 255, 255],
    ];
    let mut table = primaries.clone();
    table.extend(cube(3).into_iter().filter(|c| !primaries.contains(c)));
    assert_eq!(table.len(), 27);
    let (got, colors) = mapped(
        &table,
        json!({"palette":"uniform","colors":30,"forced":"primaries"}),
    );
    assert_eq!((colors, got), (27, (0..27).collect::<Vec<i32>>()));
}

#[test]
fn uniform_refuses_with_the_colours_it_needs() {
    let refusal = |pixels: &[[u8; 4]], options: serde_json::Value| -> String {
        let doc = rgb_doc(pixels.len() as u32, 1, pixels);
        let p = plan(doc, &options);
        let err = to_indexed(doc, &options).unwrap_err();
        free(doc);
        assert_eq!(
            p["refusal"].as_str(),
            Some(err.as_str()),
            "plan and convert agree"
        );
        err
    };
    let opaque_pixel = [[10, 20, 30, 255]];
    // colors 7 → N = |cube_2| = 8.
    assert_eq!(
        refusal(&opaque_pixel, json!({"palette":"uniform","colors":7})),
        "The Uniform palette needs at least 8 colours with these options."
    );
    // colors 8 with a transparent pixel → N = 8 + 1.
    assert_eq!(
        refusal(
            &[[10, 20, 30, 255], [0, 0, 0, 0]],
            json!({"palette":"uniform","colors":8})
        ),
        "The Uniform palette needs at least 9 colours with these options."
    );
    // Forced Web at 215 → N = |web ∪ cube_2| = 216.
    assert_eq!(
        refusal(
            &opaque_pixel,
            json!({"palette":"uniform","colors":215,"forced":"web"})
        ),
        "The Uniform palette needs at least 216 colours with these options."
    );
}

#[test]
fn forced_colours_apply_to_uniform_and_adaptive_only() {
    let doc = rgb_doc(1, 1, &[[1, 2, 3, 255]]);
    let err = to_indexed(doc, &json!({"palette":"web","forced":"primaries"})).unwrap_err();
    assert_eq!(
        err,
        "Forced colours apply to the Uniform and Adaptive palettes only."
    );
    let err = to_indexed(doc, &json!({"palette":"median"})).unwrap_err();
    assert_eq!(
        err,
        "mode option palette must be exact, web, uniform, adaptive, system_mac, system_windows or \
         custom"
    );
    free(doc);
}

/// The palette of an Indexed document every entry of which some pixel uses:
/// entry i is the layer's colour at the first pixel of index i.
fn used_palette(doc: *const doctored_core::precision_document::PrecisionDocument) -> Vec<[u8; 3]> {
    let colors = descriptor(doc)["indexed"]["colors"].as_u64().unwrap() as i32;
    let plane = indices(doc);
    let layer = crate::indexed_fixture::layer16(doc);
    (0..colors)
        .map(|i| {
            let at = plane
                .iter()
                .position(|&v| v == i)
                .expect("every entry is used");
            [0, 1, 2].map(|c| (layer[at][c] / 257) as u8)
        })
        .collect()
}

/// A (×5), B, C, D (×3) in one row: the median cut's hand-computed image.
fn four_colours() -> Vec<[u8; 4]> {
    let (a, b, c, d) = ([10, 0, 0], [20, 0, 0], [100, 0, 0], [200, 50, 0]);
    let mut colors = vec![a; 5];
    colors.extend([b, c]);
    colors.extend([d; 3]);
    opaque(&colors)
}

#[test]
fn adaptive_is_the_hand_computed_median_cut() {
    // One box of A, B, C, D (keys in that order), total 10. Its widest
    // channel is r (190 against g's 50): sorted A, B, C, D; k = 1 holds
    // 2·5 ≥ 10, so {A} keeps box 0 and {B, C, D} is box 1. Box 1's mean:
    // r (20 + 100 + 600 + 2) / 5 = 144, g (150 + 2) / 5 = 30.
    let doc = rgb_doc(10, 1, &four_colours());
    let indexed = to_indexed(
        doc,
        &json!({"palette":"adaptive","colors":2,"dither":"none"}),
    )
    .unwrap();
    assert_eq!(used_palette(indexed), vec![[10, 0, 0], [144, 30, 0]]);
    // B is 10 from A; C is nearer (144, 30, 0) (2836 against 8100).
    assert_eq!(indices(indexed), vec![0, 0, 0, 0, 0, 0, 1, 1, 1, 1]);
    free(indexed);
    // Budget 3: box 1 (the only one of ≥ 2 colours) splits on r, sorted B,
    // C, D: k = 1 (2·1 < 5) and k = 2 (2·2 < 5) fail, so the cut is n − 1 =
    // 2: {B, C} keeps box 1, mean r (120 + 1) / 2 = 60; {D} is box 2.
    let indexed = to_indexed(
        doc,
        &json!({"palette":"adaptive","colors":3,"dither":"none"}),
    )
    .unwrap();
    assert_eq!(
        used_palette(indexed),
        vec![[10, 0, 0], [60, 0, 0], [200, 50, 0]]
    );
    assert_eq!(indices(indexed), vec![0, 0, 0, 0, 0, 0, 1, 2, 2, 2]);
    free(indexed);
    // Forced Black and White lead; the cut runs at 4 − 2 = 2 entries.
    let indexed = to_indexed(
        doc,
        &json!({"palette":"adaptive","colors":4,"forced":"black_white","dither":"none"}),
    )
    .unwrap();
    assert_eq!(descriptor(indexed)["indexed"]["colors"], json!(4));
    let layer = crate::indexed_fixture::layer16(indexed);
    assert_eq!(indices(indexed), vec![2, 2, 2, 2, 2, 2, 3, 3, 3, 3]);
    assert_eq!(layer[0], [10 * 257, 0, 0, 65535]);
    assert_eq!(layer[6], [144 * 257, 30 * 257, 0, 65535]);
    free(indexed);
    free(doc);
}

#[test]
fn adaptive_stops_when_no_box_can_split_and_drops_forced_duplicates() {
    // Two colours at budget 8: two entries, one a colour.
    let doc = rgb_doc(2, 1, &opaque(&[[1, 2, 3], [200, 100, 50]]));
    let indexed = to_indexed(
        doc,
        &json!({"palette":"adaptive","colors":8,"dither":"none"}),
    )
    .unwrap();
    assert_eq!(used_palette(indexed), vec![[1, 2, 3], [200, 100, 50]]);
    free(indexed);
    free(doc);
    // Black and red under Forced Black and White: the cut's black drops.
    let doc = rgb_doc(2, 1, &opaque(&[[0, 0, 0], [255, 0, 0]]));
    let indexed = to_indexed(
        doc,
        &json!({"palette":"adaptive","colors":4,"forced":"black_white","dither":"none"}),
    )
    .unwrap();
    assert_eq!(descriptor(indexed)["indexed"]["colors"], json!(3));
    assert_eq!(indices(indexed), vec![0, 2]);
    assert_eq!(
        crate::indexed_fixture::layer16(indexed)[1],
        [65535, 0, 0, 65535]
    );
    free(indexed);
    free(doc);
    // One colour pads with black.
    let doc = rgb_doc(1, 1, &opaque(&[[40, 50, 60]]));
    let indexed = to_indexed(doc, &json!({"palette":"adaptive","dither":"none"})).unwrap();
    assert_eq!(descriptor(indexed)["indexed"]["colors"], json!(2));
    assert_eq!(indices(indexed), vec![0]);
    free(indexed);
    free(doc);
}

#[test]
fn adaptive_on_a_fully_transparent_canvas_pads_and_appends_the_transparent_entry() {
    // No opaque colour: the cut has nothing, the padding is black and white,
    // and the transparent entry (white, no matte) follows them.
    let doc = rgb_doc(2, 1, &[[0, 0, 0, 0], [5, 5, 5, 0]]);
    let indexed = to_indexed(doc, &json!({"palette":"adaptive","dither":"diffusion"})).unwrap();
    let d = descriptor(indexed);
    assert_eq!(
        (
            d["indexed"]["colors"].clone(),
            d["indexed"]["transparent_index"].clone()
        ),
        (json!(3), json!(2))
    );
    assert_eq!(indices(indexed), vec![2, 2]);
    free(indexed);
    free(doc);
}

#[test]
fn adaptive_refuses_with_the_colours_it_needs() {
    let refusal = |pixels: &[[u8; 4]], options: serde_json::Value| -> String {
        let doc = rgb_doc(pixels.len() as u32, 1, pixels);
        let p = plan(doc, &options);
        let err = to_indexed(doc, &options).unwrap_err();
        free(doc);
        assert_eq!(
            p["refusal"].as_str(),
            Some(err.as_str()),
            "plan and convert agree"
        );
        err
    };
    // Forced Black and White at 2 colours: N = 2 + 0 + 1.
    assert_eq!(
        refusal(
            &[[10, 20, 30, 255]],
            json!({"palette":"adaptive","colors":2,"forced":"black_white"})
        ),
        "The Adaptive palette needs at least 3 colours with these options."
    );
    // At 3 with a transparent pixel: N = 2 + 1 + 1.
    assert_eq!(
        refusal(
            &[[10, 20, 30, 255], [0, 0, 0, 0]],
            json!({"palette":"adaptive","colors":3,"forced":"black_white"})
        ),
        "The Adaptive palette needs at least 4 colours with these options."
    );
}

#[test]
fn exact_keeps_the_first_occurrence_order_and_refuses_past_its_budget() {
    let table = [
        [9, 9, 9],
        [200, 1, 1],
        [9, 9, 9],
        [0, 0, 0],
        [200, 1, 1],
        [5, 6, 7],
    ];
    let (got, colors) = mapped(&table, json!({"palette":"exact"}));
    assert_eq!((colors, got), (4, vec![0, 1, 0, 2, 1, 3]));
    // One colour pads with black; a black one with white.
    let (_, colors) = mapped(&[[40, 50, 60]], json!({"palette":"exact"}));
    assert_eq!(colors, 2);
    let doc = rgb_doc(4, 1, &opaque(&[[1, 1, 1], [2, 2, 2], [3, 3, 3], [4, 4, 4]]));
    let p = plan(doc, &json!({"palette":"exact","colors":3}));
    assert_eq!(
        p["refusal"],
        json!("The Exact palette holds at most 3 colours; this image has more.")
    );
    free(doc);
    // A transparent pixel takes one entry of the budget.
    let mut pixels = opaque(&[[1, 1, 1], [2, 2, 2], [3, 3, 3], [4, 4, 4]]);
    pixels.push([0, 0, 0, 0]);
    let doc = rgb_doc(5, 1, &pixels);
    assert_eq!(
        to_indexed(doc, &json!({"palette":"exact","colors":4})).unwrap_err(),
        "The Exact palette holds at most 3 colours; this image has more."
    );
    let indexed = to_indexed(doc, &json!({"palette":"exact","colors":5})).unwrap();
    let d = descriptor(indexed);
    assert_eq!(
        (
            d["indexed"]["colors"].clone(),
            d["indexed"]["transparent_index"].clone()
        ),
        (json!(5), json!(4))
    );
    assert_eq!(indices(indexed), vec![0, 1, 2, 3, 4]);
    free(indexed);
    free(doc);
    // Two colours, one opaque and one transparent: the padding stays inside the budget.
    let doc = rgb_doc(2, 1, &[[40, 50, 60, 255], [0, 0, 0, 0]]);
    let indexed = to_indexed(doc, &json!({"palette":"exact","colors":2})).unwrap();
    let d = descriptor(indexed);
    assert_eq!(
        (
            d["indexed"]["colors"].clone(),
            d["indexed"]["transparent_index"].clone()
        ),
        (json!(2), json!(1))
    );
    assert_eq!(indices(indexed), vec![0, 1]);
    free(indexed);
    free(doc);
}

/// The 3×2 gray image under Uniform 8 (black = 0, white = 7) with diffusion.
fn diffused(row0: [u8; 3], amount: u32, preserve_exact: bool) -> Vec<i32> {
    let mut pixels: Vec<[u8; 4]> = row0.iter().map(|&v| [v, v, v, 255]).collect();
    pixels.extend([[100, 100, 100, 255]; 3]);
    let doc = rgb_doc(3, 2, &pixels);
    let indexed = to_indexed(
        doc,
        &json!({"palette":"uniform","colors":8,"dither":"diffusion","amount":amount,
                "preserve_exact":preserve_exact}),
    )
    .unwrap();
    let out = indices(indexed);
    free(indexed);
    free(doc);
    out
}

#[test]
fn floyd_steinberg_matches_a_hand_computed_case() {
    // Amount 100, every pixel 100 (by hand, per channel):
    // (0,0) w 100 → black, e 100: (1,0) +43.75, (0,1) +31.25, (1,1) +6.25.
    // (1,0) w 143.75 → 144 → white, e −111.25: (2,0) −48.671875,
    //   (0,1) −20.859375, (1,1) −34.765625, (2,1) −6.953125.
    // (2,0) w 51.328125 → black, e 51.328125: (1,1) +9.6240234375,
    //   (2,1) +16.0400390625.
    // (0,1) w 110.390625 → black, e: (1,1) +48.2958984375 → w 129.404296875
    // (1,1) → 129 → white, e −125.595703125: (2,1) −54.9481201171875
    // (2,1) w 54.1387939453125 → black.
    assert_eq!(
        diffused([100, 100, 100], 100, false),
        vec![0, 7, 0, 0, 7, 0]
    );
    // Amount 50: (1,0) w 121.875 → black; (2,0) w 126.66015625 → 127 →
    // black; (0,1) w 127.05078125 → black; (1,1) w 161.834716796875 → white;
    // (2,1) w 103.2193374633789 → black.
    assert_eq!(diffused([100, 100, 100], 50, false), vec![0, 0, 0, 0, 7, 0]);
    // Row 0 = 100, 0, 110. Preserve Exact: (1,0) is black exactly, takes
    // entry 0 and emits nothing; (2,0) w 110 → black, e 110; (0,1) w 131.25
    // → white; (1,1) w 72.734375 → black; (2,1) w 166.1962890625 → white.
    assert_eq!(diffused([100, 0, 110], 100, true), vec![0, 0, 0, 7, 0, 7]);
    // Without it (1,0) w 43.75 → black and emits 43.75: (2,0) w 129.140625
    // → white; (0,1) w 139.453125 → white; (1,1) w 45.771484375 → black;
    // (2,1) w 83.428466796875 → black.
    assert_eq!(diffused([100, 0, 110], 100, false), vec![0, 0, 7, 7, 0, 0]);
}

#[test]
fn transparency_and_matte_choose_the_transparent_entry() {
    // Half-transparent red, clear, opaque blue.
    let pixels = [[255, 0, 0, 100], [0, 0, 0, 0], [0, 0, 255, 255]];
    let doc = rgb_doc(3, 1, &pixels);
    // No matte: alpha < 128 is transparent; the entry is white, appended.
    let indexed = to_indexed(doc, &json!({"palette":"exact","dither":"none"})).unwrap();
    let d = descriptor(indexed);
    assert_eq!(
        (
            d["indexed"]["colors"].clone(),
            d["indexed"]["transparent_index"].clone()
        ),
        (json!(3), json!(2))
    );
    assert_eq!(indices(indexed), vec![2, 2, 0]);
    let render = crate::indexed_fixture::layer16(indexed);
    assert_eq!(
        render[0],
        [65535, 65535, 65535, 0],
        "the transparent entry is white, alpha 0"
    );
    free(indexed);
    // A matte: alpha 0 alone is transparent, the rest composited over it.
    let indexed = to_indexed(
        doc,
        &json!({"palette":"exact","dither":"none","matte":[0,255,0]}),
    )
    .unwrap();
    // 255·100/255 and 255·155/255 over green: (100, 155, 0).
    assert_eq!(indices(indexed), vec![0, 2, 1]);
    let render = crate::indexed_fixture::layer16(indexed);
    assert_eq!(render[0], [100 * 257, 155 * 257, 0, 65535]);
    assert_eq!(
        render[1],
        [0, 65535, 0, 0],
        "the transparent entry is the matte"
    );
    free(indexed);
    // Transparency off: every pixel over white, no transparent index.
    let indexed = to_indexed(
        doc,
        &json!({"palette":"exact","dither":"none","transparency":false}),
    )
    .unwrap();
    assert_eq!(
        descriptor(indexed)["indexed"]["transparent_index"],
        json!(null)
    );
    assert_eq!(indices(indexed), vec![0, 1, 2]);
    assert_eq!(crate::indexed_fixture::layer16(indexed)[1], [65535; 4]);
    free(indexed);
    // A System palette makes entry 255 the transparent index.
    let indexed = to_indexed(doc, &json!({"palette":"system_mac","dither":"none"})).unwrap();
    assert_eq!(
        descriptor(indexed)["indexed"]["transparent_index"],
        json!(255)
    );
    assert_eq!(indices(indexed)[1], 255);
    free(indexed);
    free(doc);
}
