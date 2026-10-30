//! The layer REFERENCE POINT (Photoshop's `fxrp`) and every
//! linked pattern anchor under the Crop tool's angled crop, the canvas
//! quarter turns and flips, and Free Transform (one layer, a set, a
//! parallelogram Perspective), at 8 bits (the plain ops and the byte
//! geometry request through the FFI), 16 bits, 32-bit float and CMYK16, on
//! rasters, a group record, a retained-source layer and a masked layer; and
//! the turned copy of each linked tile a turn or a flip makes. The build and
//! the helpers are `style_pattern_reference_geometry.rs`'s.
//!
//! THE ORACLES are written out per op, never the kernel. The canvas maps on
//! a W x H canvas: R90 (H − y, x), R180 (W − x, H − y), R270 (y, W − x), FH
//! (W − x, y), FV (x, H − y). Free Transform: M(r) for each take1 matrix
//! built here about L1's bounds centre (97, 45) from the case's own numbers.
//! The angled crop: take1's CROP-A matrix, `rotation(−10, (10, 6, 100,
//! 68))` (a +10° turn of the content), with the window corner subtracted. Every image is rounded half away from zero; a linked anchor
//! is held to offset′ − anchor′ = the map applied to offset − anchor. The
//! turned tile: each texel of T read through the test's own texel map (R90
//! T′(i, j) = T(j, 7 − i), R180 T(11 − i, 7 − j), R270 T(11 − j, i), FH
//! T(11 − i, j), FV T(i, 7 − j)), under a Custom source with the pattern's
//! name; at scale 1 every pixel p of L0 and L1 (inset 1 px) reads T at
//! floor(L⁻¹(p + 0.5 − O)) mod (12, 8), L the map's linear part and O =
//! phase + reference′.
//!
//! BOUNDS, declared before measuring: 0 codes for the 8-bit render against
//! the oracle, 1 code (v16 / 257) for the 16-bit render; 0 px for every
//! record against round(Photoshop's `fxrp`) in the committed fixture
//! (`fixtures/psd_photoshop/reference_point/photoshop.json`); 2 codes per RGB
//! channel against take1's own TIFFs
//! (`~/devel/doctored-fixtures/photoshop/reference-point-667/take1/jobs`,
//! skipped by name when absent) on every pixel of L0's and L1's
//! `bounds_after` inset 1 px — R90, R180, R270, FH and FV against `_after`
//! and `_reopened`, FT-F against `_reopened` only.

use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::doc_transform::Affine;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_retained_source::dr_doc_mixed_source_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Record};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::retained_source::{SourceEdit, SourceInput};
use doctored_core::style::{Effect, LayerStyle, Pattern, PatternSource, PatternTile};
use image::imageops::FilterType;
use image::RgbaImage;
use serde_json::{json, Value};
use std::ffi::CString;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common::{solid, styled, take_err_string};
use crate::style_pattern_reference_geometry::{
    anchor8, byte_request, facts16, facts8, fxrp, index16, index8, minus, names_order, open8,
    rotation, save8, style, take_doc8, texel, typed16, typed_edit, Facts, CANVAS, LIMIT, PHASES,
    WHITE,
};

// --------------------------------------------------------------- the maps --

/// A canvas quarter turn or flip, with its rule written out (module doc).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Turn {
    R90,
    R180,
    R270,
    Fh,
    Fv,
}

const TURNS: [Turn; 5] = [Turn::R90, Turn::R180, Turn::R270, Turn::Fh, Turn::Fv];

impl Turn {
    /// `p` after the turn of a `canvas`-sized document.
    fn point(self, p: (i32, i32), canvas: (u32, u32)) -> (i32, i32) {
        let (w, h) = (canvas.0 as i32, canvas.1 as i32);
        match self {
            Turn::R90 => (h - p.1, p.0),
            Turn::R180 => (w - p.0, h - p.1),
            Turn::R270 => (p.1, w - p.0),
            Turn::Fh => (w - p.0, p.1),
            Turn::Fv => (p.0, h - p.1),
        }
    }

    /// The source texel (in a `tw x th` tile) of the turned tile's (i, j).
    fn source(self, i: u32, j: u32, tw: u32, th: u32) -> (u32, u32) {
        match self {
            Turn::R90 => (j, th - 1 - i),
            Turn::R180 => (tw - 1 - i, th - 1 - j),
            Turn::R270 => (tw - 1 - j, i),
            Turn::Fh => (tw - 1 - i, j),
            Turn::Fv => (i, th - 1 - j),
        }
    }

    /// The turned size of a `tw x th` tile.
    fn size(self, tw: u32, th: u32) -> (u32, u32) {
        match self {
            Turn::R90 | Turn::R270 => (th, tw),
            _ => (tw, th),
        }
    }

    /// L⁻¹(u, v), L the turn's linear part.
    fn unturned(self, u: f64, v: f64) -> (f64, f64) {
        match self {
            Turn::R90 => (v, -u),
            Turn::R180 => (-u, -v),
            Turn::R270 => (-v, u),
            Turn::Fh => (-u, v),
            Turn::Fv => (u, -v),
        }
    }

    fn request(self) -> Value {
        json!({"op": match self {
            Turn::R90 => "rotate90",
            Turn::R180 => "rotate180",
            Turn::R270 => "rotate270",
            Turn::Fh => "flip_horizontal",
            Turn::Fv => "flip_vertical",
        }})
    }

    fn plain(self, doc: &DrDocument) -> DrDocument {
        match self {
            Turn::R90 => doc.rotate90(),
            Turn::R180 => doc.rotate180(),
            Turn::R270 => doc.rotate270(),
            Turn::Fh => doc.flip_horizontal(),
            Turn::Fv => doc.flip_vertical(),
        }
        .unwrap_or_else(|| panic!("{self:?} changes the document"))
    }

    /// take1's case name.
    fn case(self) -> &'static str {
        match self {
            Turn::R90 => "R90",
            Turn::R180 => "R180",
            Turn::R270 => "R270",
            Turn::Fh => "FH",
            Turn::Fv => "FV",
        }
    }
}

/// `m` = [a, b, c, d, tx, ty] applied to `p`, rounded half away from zero.
fn applied(m: [f64; 6], p: (i32, i32)) -> (i32, i32) {
    let (x, y) = (f64::from(p.0), f64::from(p.1));
    (
        (m[0] * x + m[2] * y + m[4]).round() as i32,
        (m[1] * x + m[3] * y + m[5]).round() as i32,
    )
}

/// The linear part `l` = [a, b, c, d] about `centre`: C + L(p − C).
fn about(l: [f64; 4], centre: (f64, f64)) -> [f64; 6] {
    let (cx, cy) = centre;
    [
        l[0],
        l[1],
        l[2],
        l[3],
        cx - l[0] * cx - l[2] * cy,
        cy - l[1] * cx - l[3] * cy,
    ]
}

/// L1's bounds centre in take1: (77, 33)–(117, 57).
const L1_CENTRE: (f64, f64) = (97.0, 45.0);

/// A Free Transform case's linear part from take1's edit: `width`/`height`
/// in percent, `angle` in degrees (clockwise on screen), `skew` [x, y] in
/// degrees.
fn free_transform(edit: &Value) -> [f64; 4] {
    let percent = |key: &str| edit[key].as_f64().map_or(1.0, |v| v / 100.0);
    if let Some(angle) = edit["angle"].as_f64() {
        let (s, c) = angle.to_radians().sin_cos();
        return [c, s, -s, c];
    }
    if let Some(skew) = edit["skew"].as_array() {
        let (kx, ky) = (skew[0].as_f64().unwrap(), skew[1].as_f64().unwrap());
        return [1.0, ky.to_radians().tan(), kx.to_radians().tan(), 1.0];
    }
    [percent("width"), 0.0, 0.0, percent("height")]
}

/// take1's four Free Transform matrices, about L1's centre.
fn ft_matrices() -> [(&'static str, [f64; 6]); 4] {
    [
        ("FT-S", json!({"width": 50, "height": 75})),
        ("FT-R", json!({"angle": 30})),
        ("FT-K", json!({"skew": [20, 0]})),
        ("FT-F", json!({"width": -100})),
    ]
    .map(|(name, edit)| (name, about(free_transform(&edit), L1_CENTRE)))
}

/// `after` holds `before` with each record in `moved` mapped by `map` and
/// every other record unchanged (offset included).
fn assert_mapped(
    before: &Facts,
    after: &Facts,
    moved: &[&str],
    map: impl Fn((i32, i32)) -> (i32, i32),
    what: &str,
) {
    assert_eq!(before.len(), after.len(), "{what}: the same records");
    for (b, a) in before.iter().zip(after) {
        assert_eq!(b.0, a.0, "{what}: record order");
        if !moved.contains(&b.0.as_str()) {
            assert_eq!(a, b, "{what}: {} is unchanged", b.0);
            continue;
        }
        assert_eq!(a.1, map(b.1), "{what}: {}'s reference", b.0);
        match (b.3, a.3) {
            (Some(ab), Some(aa)) => assert_eq!(
                minus(a.2, aa),
                map(minus(b.2, ab)),
                "{what}: {}'s offset − anchor",
                b.0
            ),
            (None, None) => {}
            other => panic!("{what}: {}'s anchor {other:?}", b.0),
        }
    }
}

const ALL: [&str; 5] = ["Background", "L0", "L1", "L2", "G"];

// --------------------------------------------------------------- the tiles --

/// take1's tile T as a tile.
fn tile_t() -> PatternTile {
    let mut rgba = Vec::new();
    for y in 0..8 {
        for x in 0..12 {
            rgba.extend_from_slice(&texel(x, y));
        }
    }
    PatternTile {
        width: 12,
        height: 8,
        rgba: Arc::from(rgba),
    }
}

/// `tile` turned by the test's own texel map.
fn turned(tile: &PatternTile, turn: Turn) -> PatternTile {
    let (w, h) = turn.size(tile.width, tile.height);
    let mut rgba = Vec::new();
    for j in 0..h {
        for i in 0..w {
            let (x, y) = turn.source(i, j, tile.width, tile.height);
            let at = ((y * tile.width + x) * 4) as usize;
            rgba.extend_from_slice(&tile.rgba[at..at + 4]);
        }
    }
    PatternTile {
        width: w,
        height: h,
        rgba: Arc::from(rgba),
    }
}

/// The one pattern overlay of `style`: (linked, source, tile).
fn overlay(style: Option<&LayerStyle>) -> (bool, PatternSource, Arc<PatternTile>) {
    let style = style.expect("a style");
    let fills: Vec<_> = style
        .effects
        .iter()
        .filter_map(|e| match e {
            Effect::PatternOverlay(p) => Some(&p.fill),
            _ => None,
        })
        .collect();
    assert_eq!(fills.len(), 1, "one pattern overlay");
    let fill = fills[0];
    (
        fill.link_with_layer,
        fill.pattern.source.clone(),
        Arc::clone(&fill.pattern.tile),
    )
}

/// The take document plus a preset-sourced linked overlay on L2 and an
/// UNLINKED overlay of T on the Background.
fn extras_doc8() -> DrDocument {
    let doc = take_doc8();
    let preset = json!({"effects":[{"type":"pattern_overlay",
        "pattern":{"preset":"stripes"},"phase":[0, 0]}]});
    let doc = styled(&doc, index8(&doc, "L2"), &preset.to_string());
    let mut unlinked = style((3, 1));
    unlinked["effects"][0]["link_with_layer"] = json!(false);
    styled(&doc, index8(&doc, "Background"), &unlinked.to_string())
}

/// Every fill of the extras document after `turn` (module doc), from a
/// record lookup by name; L0's and L1's tiles back.
fn assert_tiles(
    style_of: impl Fn(&str) -> Option<Arc<LayerStyle>>,
    turn: Turn,
    what: &str,
) -> (Arc<PatternTile>, Arc<PatternTile>) {
    let t = tile_t();
    let custom = |name: &str| PatternSource::Custom { name: name.into() };
    let mut linked = Vec::new();
    for name in ["L0", "L1"] {
        let (link, source, tile) = overlay(style_of(name).as_deref());
        assert!(link, "{what}: {name} stays linked");
        assert_eq!(source, custom("T"), "{what}: {name}'s source");
        assert_eq!(*tile, turned(&t, turn), "{what}: {name}'s turned tile");
        linked.push(tile);
    }
    let preset = Pattern::preset("stripes").unwrap();
    let (_, source, tile) = overlay(style_of("L2").as_deref());
    assert_eq!(
        source,
        custom(&preset.name()),
        "{what}: a preset becomes Custom under its name"
    );
    assert_eq!(
        *tile,
        turned(&preset.tile, turn),
        "{what}: the preset's turned tile"
    );
    let (link, source, tile) = overlay(style_of("Background").as_deref());
    assert!(!link, "{what}: the Background's overlay is unlinked");
    assert_eq!(source, custom("T"), "{what}: the unlinked source");
    assert_eq!(*tile, t, "{what}: an unlinked overlay keeps its tile");
    (linked.remove(0), linked.remove(0))
}

fn style8(doc: &DrDocument) -> impl Fn(&str) -> Option<Arc<LayerStyle>> + '_ {
    move |name| doc.layers[index8(doc, name)].style.clone()
}

fn style16(doc: &PrecisionDocument) -> impl Fn(&str) -> Option<Arc<LayerStyle>> + '_ {
    move |name| doc.layer(index16(doc, name)).unwrap().style().cloned()
}

/// The texel T puts at canvas pixel (x, y) after `turn`, origin `o`.
fn turned_want(turn: Turn, o: (i32, i32), x: i32, y: i32) -> [u8; 4] {
    let u = f64::from(x) + 0.5 - f64::from(o.0);
    let v = f64::from(y) + 0.5 - f64::from(o.1);
    let (a, b) = turn.unturned(u, v);
    texel(
        (a.floor() as i64).rem_euclid(12) as u32,
        (b.floor() as i64).rem_euclid(8) as u32,
    )
}

/// Every pixel of `rect` (x, y, w, h), inset 1 px and inside `canvas`.
fn styled_pixels(
    rect: (i32, i32, u32, u32),
    canvas: (u32, u32),
) -> impl Iterator<Item = (i32, i32)> {
    let (ox, oy, w, h) = rect;
    let xs = (ox + 1).max(0)..(ox + w as i32 - 1).min(canvas.0 as i32);
    let ys = (oy + 1).max(0)..(oy + h as i32 - 1).min(canvas.1 as i32);
    ys.flat_map(move |y| xs.clone().map(move |x| (x, y)))
}

fn assert_turned_render8(doc: &DrDocument, turn: Turn, what: &str) {
    let flat = doc.flattened().unwrap();
    let canvas = (doc.width, doc.height);
    for (name, phase) in PHASES {
        let layer = &doc.layers[index8(doc, name)];
        let (w, h) = layer.pixels.dimensions();
        let o = (phase.0 + layer.reference.0, phase.1 + layer.reference.1);
        let mut n = 0;
        for (x, y) in styled_pixels((layer.offset.0, layer.offset.1, w, h), canvas) {
            assert_eq!(
                flat.get_pixel(x as u32, y as u32).0,
                turned_want(turn, o, x, y),
                "{what}: {name} at ({x}, {y})"
            );
            n += 1;
        }
        assert!(n > 0, "{what}: {name} has pixels to read");
    }
}

fn assert_turned_render16(doc: &PrecisionDocument, turn: Turn, what: &str) {
    let (w, h) = doc.dimensions();
    let pixels = doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();
    for (name, phase) in PHASES {
        let layer = doc.layer(index16(doc, name)).unwrap();
        let (lw, lh) = layer.source().unwrap().dimensions();
        let (ox, oy) = layer.offset();
        let o = (phase.0 + layer.reference().0, phase.1 + layer.reference().1);
        for (x, y) in styled_pixels((ox, oy, lw, lh), (w, h)) {
            let got = pixels[(y as u32 * w + x as u32) as usize];
            let t = turned_want(turn, o, x, y);
            for c in 0..3 {
                let code = f64::from(got[c]) / 257.0;
                assert!(
                    (code - f64::from(t[c])).abs() <= 1.0,
                    "{what}: {name} at ({x}, {y}): {got:?} against {t:?}"
                );
            }
        }
    }
}

// ------------------------------------------------------- turns and flips --

#[test]
fn byte_turns_and_flips_map_every_point_and_turn_every_linked_tile() {
    let doc = extras_doc8();
    let before = facts8(&doc);
    let t = tile_t();
    for turn in TURNS {
        let plain = turn.plain(&doc);
        let what = format!("{turn:?} plain");
        assert_mapped(
            &before,
            &facts8(&plain),
            &ALL,
            |p| turn.point(p, CANVAS),
            &what,
        );
        let (l0, l1) = assert_tiles(style8(&plain), turn, &what);
        assert!(Arc::ptr_eq(&l0, &l1), "{what}: L0 and L1 share one copy");
        assert_turned_render8(&plain, turn, &what);

        let request = byte_request(&doc, &turn.request());
        assert_eq!(
            facts8(&request),
            facts8(&plain),
            "{turn:?}: request = plain"
        );
        assert_tiles(style8(&request), turn, &format!("{turn:?} request"));

        assert_eq!(facts8(&doc), before, "{turn:?}: the input keeps its points");
        let (_, _, kept) = overlay(doc.layers[index8(&doc, "L1")].style.as_deref());
        assert_eq!(*kept, t, "{turn:?}: the input keeps its tile");
    }
    // Rotate Canvas 90 is the exact quarter turn.
    let quarter = byte_request(
        &doc,
        &json!({"op":"rotate_canvas","degrees":90,"filter":"bilinear"}),
    );
    assert_eq!(
        facts8(&quarter),
        facts8(&Turn::R90.plain(&doc)),
        "rotate_canvas 90"
    );
    assert_tiles(style8(&quarter), Turn::R90, "rotate_canvas 90");
}

fn cmyk_of(rgb: &PrecisionDocument) -> PrecisionDocument {
    let profile = press_profile("Press Test");
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take_err_string(err));
    *unsafe { Box::from_raw(out) }
}

/// The extras document at 16 bits, float and CMYK16.
fn typed_extras() -> Vec<(&'static str, PrecisionDocument)> {
    let sixteen = extras_doc8().promote_structure(LIMIT, 0, None).unwrap();
    let float = sixteen.promote_to_float(LIMIT, 0, None).unwrap();
    let cmyk = cmyk_of(&sixteen);
    vec![("RGB16", sixteen), ("float", float), ("CMYK16", cmyk)]
}

#[test]
fn typed_turns_and_flips_map_every_point_and_turn_every_linked_tile() {
    let byte = extras_doc8();
    for (depth, doc) in typed_extras() {
        let before = facts16(&doc);
        assert_eq!(
            before,
            names_order(facts8(&byte)),
            "{depth}: the conversion carries the facts"
        );
        for turn in TURNS {
            let out = typed_edit(&doc, &turn.request());
            let what = format!("{depth} {turn:?}");
            assert_mapped(
                &before,
                &facts16(&out),
                &ALL,
                |p| turn.point(p, CANVAS),
                &what,
            );
            assert_tiles(style16(&out), turn, &what);
            if depth == "RGB16" {
                assert_turned_render16(&out, turn, &what);
            }
            assert_eq!(facts16(&doc), before, "{what}: purity");
            let (_, _, kept) = overlay(style16(&doc)("L1").as_deref());
            assert_eq!(*kept, tile_t(), "{what}: the input keeps its tile");
        }
        let quarter = typed_edit(
            &doc,
            &json!({"op":"rotate_canvas","degrees":90,"filter":"bilinear"}),
        );
        let what = format!("{depth} rotate_canvas 90");
        assert_mapped(
            &before,
            &facts16(&quarter),
            &ALL,
            |p| Turn::R90.point(p, CANVAS),
            &what,
        );
        assert_tiles(style16(&quarter), Turn::R90, &what);
    }
}

#[test]
fn a_uniform_sixteen_bit_flip_is_an_edit_equal_to_the_byte_twin() {
    let doc = DrDocument::from_pixels(solid(120, 90, WHITE));
    let doc = styled(&doc, 0, &style((0, 0)).to_string());
    let byte = doc.flip_horizontal().unwrap();
    assert_eq!(byte.layers[0].reference, (120, 0));
    assert_eq!(anchor8(&byte, 0), Some((-120, 0)), "the anchor moved");
    let (_, _, byte_tile) = overlay(byte.layers[0].style.as_deref());
    assert_eq!(
        *byte_tile,
        turned(&tile_t(), Turn::Fh),
        "the byte tile mirrored"
    );

    let typed = doc.promote_structure(LIMIT, 0, None).unwrap();
    // `typed_edit` asserts the edit is not `None`.
    let out = typed_edit(&typed, &json!({"op":"flip_horizontal"}));
    let record = out.layer(0).unwrap();
    assert_eq!(record.reference(), (120, 0), "the typed reference");
    let (_, _, tile) = overlay(record.style().map(|s| &**s));
    assert_eq!(*tile, *byte_tile, "the typed tile is the byte tile");
    let reported: Value = serde_json::from_str(&record.reported_style().unwrap()).unwrap();
    assert_eq!(
        reported["effects"][0]["anchor"],
        json!([-120, 0]),
        "the typed anchor"
    );
}

#[test]
fn rz_and_psd_keep_the_turned_tile_and_the_point() {
    let dir = TempDir::new().unwrap();
    let out = Turn::R90.plain(&take_doc8());
    let rz = dir.path().join("r90.rz");
    out.save_native(rz.to_str().unwrap()).unwrap();
    let back = DrDocument::open(rz.to_str().unwrap()).unwrap();
    assert_eq!(facts8(&back), facts8(&out), ".doctored facts");
    let psd = dir.path().join("r90.psd");
    save8(&out, &psd);
    let reopened = open8(&psd);
    for (name, _) in PHASES {
        let want = overlay(out.layers[index8(&out, name)].style.as_deref()).2;
        for (form, doc) in [(".rz", &back), ("PSD", &reopened)] {
            let i = index8(doc, name);
            let (_, _, tile) = overlay(doc.layers[i].style.as_deref());
            assert_eq!(*tile, *want, "{form}: {name}'s turned tile");
            assert_eq!(
                minus(doc.layers[i].offset, anchor8(doc, i).unwrap()),
                minus(
                    out.layers[index8(&out, name)].offset,
                    anchor8(&out, index8(&out, name)).unwrap()
                ),
                "{form}: {name}'s offset − anchor"
            );
        }
    }
}

// ------------------------------------------------------------ angled crop --

/// The straighten request over `window` with take1's CROP-A matrix, and its
/// windowed oracle. Photoshop's crop angle 10 turns the content by +10°
/// about the window's centre: `rotation(−10, …)` in the helper's sign (which
/// turns by −angle); `rotation(10, …)` reads L1 at (−9, 7), not take1's
/// (4.230, −11.641).
fn angled(window: (i32, i32, u32, u32)) -> (Value, [f64; 6]) {
    let m = rotation(-10.0, (10, 6, 100, 68));
    let request = json!({"op":"straighten","matrix":m,"filter":"bilinear",
        "x":window.0,"y":window.1,"width":window.2,"height":window.3});
    let windowed = [
        m[0],
        m[1],
        m[2],
        m[3],
        m[4] - f64::from(window.0),
        m[5] - f64::from(window.1),
    ];
    (request, windowed)
}

#[test]
fn the_angled_crop_maps_every_point_at_every_depth() {
    let doc = take_doc8();
    let before = facts8(&doc);
    for window in [(10, 6, 100, 68), (-4, -3, 130, 90)] {
        let (request, m) = angled(window);
        let out = byte_request(&doc, &request);
        let what = format!("byte straighten {window:?}");
        assert_mapped(&before, &facts8(&out), &ALL, |p| applied(m, p), &what);
        if window.0 == 10 {
            assert_eq!(
                out.layers[index8(&out, "L1")].reference,
                (4, -12),
                "take1's L1 reading, rounded"
            );
        }
        let sixteen = typed16();
        for (depth, typed) in [
            ("RGB16", sixteen.clone()),
            ("float", sixteen.promote_to_float(LIMIT, 0, None).unwrap()),
            ("CMYK16", cmyk_of(&sixteen)),
        ] {
            let before16 = facts16(&typed);
            assert_eq!(
                before16,
                names_order(before.clone()),
                "{depth}: the conversion"
            );
            let out = typed_edit(&typed, &request);
            let what = format!("{depth} straighten {window:?}");
            assert_mapped(&before16, &facts16(&out), &ALL, |p| applied(m, p), &what);
        }
    }
}

// --------------------------------------------------------- Free Transform --

/// The byte retained-source document of the phase 1 test: a 32 x 24 source
/// placed and moved (5, 6).
fn retained8() -> DrDocument {
    let source = DrDocument::from_pixels(RgbaImage::from_fn(32, 24, |x, y| {
        image::Rgba([(x * 7) as u8, (y * 9) as u8, 90, 255])
    }));
    let request = CString::new(
        json!({"op":"place","source_id":"fixture-a","name":"Placed","x":0,"y":0}).to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let placed = unsafe {
        dr_doc_mixed_source_edit(
            &source,
            &source,
            ptr::null(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!placed.is_null(), "{}", take_err_string(err));
    let placed = *unsafe { Box::from_raw(placed) };
    let moved = placed.move_layers(&[1], 5, 6).unwrap();
    assert!(moved.layers[1].retained_source.is_some());
    moved
}

#[test]
fn byte_free_transform_maps_the_point_and_keeps_the_tile() {
    let doc = take_doc8();
    let masked = doc
        .add_mask(index8(&doc, "L1"), MaskKind::RevealAll)
        .unwrap();
    let t = tile_t();
    for (name, m) in ft_matrices() {
        for (form, input) in [("raster", &doc), ("masked", &masked)] {
            let before = facts8(input);
            let l1 = index8(input, "L1");
            let out = input
                .transform_layer(l1, Affine::from_array(m), FilterType::Nearest)
                .unwrap_or_else(|| panic!("{name} {form} transforms"));
            let what = format!("byte {name} {form}");
            assert_mapped(&before, &facts8(&out), &["L1"], |p| applied(m, p), &what);
            let (_, _, tile) = overlay(out.layers[l1].style.as_deref());
            assert_eq!(*tile, t, "{what}: Free Transform keeps the tile");
            assert_eq!(out.layers[l1].mask.is_some(), form == "masked", "{what}");
            assert_eq!(facts8(input), before, "{what}: purity");
        }
        // A set transform of the group: G's record and its child L2.
        let before = facts8(&doc);
        let g = index8(&doc, "G");
        let out = byte_request(
            &doc,
            &json!({"op":"affine_set","layers":[g],"matrix":m,"filter":"nearest"}),
        );
        let what = format!("byte {name} set of G");
        assert_mapped(
            &before,
            &facts8(&out),
            &["G", "L2"],
            |p| applied(m, p),
            &what,
        );
    }
    let retained = retained8();
    let m = about(free_transform(&json!({"angle": 30})), (21.0, 18.0));
    let out = retained
        .transform_layer(1, Affine::from_array(m), FilterType::Nearest)
        .unwrap();
    assert!(out.layers[1].retained_source.is_some(), "still bound");
    assert_eq!(
        out.layers[1].reference,
        applied(m, (5, 6)),
        "the retained source"
    );
    assert_eq!(retained.layers[1].reference, (5, 6), "purity");
}

/// A typed retained-source document: a 32 x 24 canvas with the source placed
/// at (2, 1) and then translated (5, 6) (its reference point (5, 6)).
fn retained16() -> (PrecisionDocument, usize) {
    let image = |w: u32, h: u32| {
        let pixels: Vec<[u16; 4]> = (0..w * h)
            .map(|i| [(i * 37) as u16, (i * 91) as u16, 9000, 65535])
            .collect();
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap()
    };
    let parts = |w: u32, h: u32, name: &str| {
        PrecisionDocument::from_parts(
            w,
            h,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![Record::raster(name, image(w, h), (0, 0), 1.0, true).unwrap()],
        )
        .unwrap()
    };
    let master = parts(12, 8, "Original");
    let canvas = parts(32, 24, "Background");
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"reference-master","revision_id":"v1",
                "name":"Retained","x":2,"y":1})
        .to_string(),
    )
    .unwrap();
    let placed = canvas
        .source_edit_input(Some(SourceInput::Typed(&master)), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let index = placed.layer_count() - 1;
    assert!(placed.source_info(index).is_ok(), "a retained source");
    let moved = typed_edit(
        &placed,
        &json!({"op":"affine","layer":index,"target":"pixels",
                "matrix":[1,0,0,1,5,6],"filter":"nearest"}),
    );
    assert_eq!(moved.layer(index).unwrap().reference(), (5, 6));
    (moved, index)
}

#[test]
fn typed_free_transform_maps_the_point_at_16_bits_float_and_cmyk() {
    let byte = take_doc8();
    let masked = byte
        .add_mask(index8(&byte, "L1"), MaskKind::RevealAll)
        .unwrap();
    let sixteen = typed16();
    let masked16 = masked.promote_structure(LIMIT, 0, None).unwrap();
    let docs = [
        ("RGB16", sixteen.clone()),
        ("float", sixteen.promote_to_float(LIMIT, 0, None).unwrap()),
        ("CMYK16", cmyk_of(&sixteen)),
        ("RGB16 masked", masked16),
    ];
    for (name, m) in ft_matrices() {
        for (depth, doc) in &docs {
            let before = facts16(doc);
            let l1 = index16(doc, "L1");
            let out = typed_edit(
                doc,
                &json!({"op":"affine","layer":l1,"target":"pixels","matrix":m,"filter":"nearest"}),
            );
            let what = format!("{depth} {name}");
            assert_mapped(&before, &facts16(&out), &["L1"], |p| applied(m, p), &what);
            let (_, _, tile) = overlay(style16(&out)("L1").as_deref());
            assert_eq!(*tile, tile_t(), "{what}: Free Transform keeps the tile");
            assert_eq!(facts16(doc), before, "{what}: purity");
            let g = index16(doc, "G");
            let out = typed_edit(
                doc,
                &json!({"op":"affine_set","layers":[g],"matrix":m,"filter":"nearest"}),
            );
            let what = format!("{depth} {name} set of G");
            assert_mapped(
                &before,
                &facts16(&out),
                &["G", "L2"],
                |p| applied(m, p),
                &what,
            );
        }
    }
    let (retained, index) = retained16();
    let m = about(free_transform(&json!({"angle": 30})), (13.0, 11.0));
    let out = typed_edit(
        &retained,
        &json!({"op":"affine","layer":index,"target":"pixels","matrix":m,"filter":"nearest"}),
    );
    assert!(out.source_info(index).is_ok(), "still bound");
    assert_eq!(
        out.layer(index).unwrap().reference(),
        applied(m, (5, 6)),
        "the typed retained source"
    );
}

#[test]
fn a_parallelogram_perspective_follows_the_free_transform_rule() {
    // L1 (77, 33, 40, 24) onto a parallelogram: q2 = q1 + q3 − q0.
    let quad = [80.0, 30.0, 120.0, 36.0, 114.0, 66.0, 74.0, 60.0];
    let (rx, ry, rw, rh) = (77.0, 33.0, 40.0, 24.0);
    let a = (quad[2] - quad[0]) / rw;
    let b = (quad[3] - quad[1]) / rw;
    let c = (quad[6] - quad[0]) / rh;
    let d = (quad[7] - quad[1]) / rh;
    let m = [
        a,
        b,
        c,
        d,
        quad[0] - a * rx - c * ry,
        quad[1] - b * rx - d * ry,
    ];
    let doc = take_doc8();
    let before = facts8(&doc);
    let l1 = index8(&doc, "L1");
    let out = doc
        .perspective_layer(l1, quad, FilterType::Nearest)
        .unwrap();
    assert_mapped(&before, &facts8(&out), &["L1"], |p| applied(m, p), "byte");
    let typed = typed16();
    let l1 = index16(&typed, "L1");
    let out = typed_edit(
        &typed,
        &json!({"op":"perspective","layer":l1,"target":"pixels","quad":quad,"filter":"nearest"}),
    );
    assert_mapped(
        &facts16(&typed),
        &facts16(&out),
        &["L1"],
        |p| applied(m, p),
        "RGB16",
    );
}

// --------------------------------------------------------- Photoshop --

fn fixture() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/psd_photoshop/reference_point/photoshop.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn photoshops_take_holds_for_turns_flips_the_angled_crop_and_free_transform() {
    let fixture = fixture();
    let cases = &fixture["cases"];
    let doc = take_doc8();
    let mut judged = 0;
    let mut check = |case: &str, out: &DrDocument, records: &[&str]| {
        let canvas = &cases[case]["canvas_after"];
        assert_eq!(
            (u64::from(out.width), u64::from(out.height)),
            (canvas[0].as_u64().unwrap(), canvas[1].as_u64().unwrap()),
            "{case}: the canvas"
        );
        for record in ["L0", "L1", "L2", "G"] {
            let after = fxrp(&cases[case]["records"][record]["fxrp_after"]);
            let got = out.layers[index8(out, record)].reference;
            if records.contains(&record) {
                // BOUND 0 px (module doc).
                assert_eq!(
                    got, after,
                    "{case}: {record} against round(Photoshop's fxrp)"
                );
                judged += 1;
            } else {
                assert_eq!(
                    got,
                    doc.layers[index8(&doc, record)].reference,
                    "{case}: {record} is unchanged"
                );
                assert_eq!(got, after, "{case}: Photoshop left {record} too");
            }
        }
    };
    for turn in TURNS {
        check(turn.case(), &turn.plain(&doc), &["L0", "L1", "L2", "G"]);
    }
    let (request, _) = angled((10, 6, 100, 68));
    assert_eq!(cases["CROP-A"]["edit"]["angle"], 10);
    check(
        "CROP-A",
        &byte_request(&doc, &request),
        &["L0", "L1", "L2", "G"],
    );
    for (name, m) in ft_matrices() {
        assert_eq!(
            about(free_transform(&cases[name]["edit"]), L1_CENTRE),
            m,
            "{name}: the case's own numbers"
        );
        let l1 = index8(&doc, "L1");
        let out = doc
            .transform_layer(l1, Affine::from_array(m), FilterType::Nearest)
            .unwrap();
        check(name, &out, &["L1"]);
    }
    assert_eq!(judged, 6 * 4 + 4, "every judged record");
}

/// take1's TIFF `<case>_<form>.tif` of `job`, flattened at 8 bits; `None`
/// when the fixture folder is absent.
fn take1_tiff(job: &str, case: &str, form: &str) -> Option<RgbaImage> {
    let home: PathBuf = std::env::var_os("HOME")?.into();
    let path = home
        .join("devel/doctored-fixtures/photoshop/reference-point-667/take1/jobs")
        .join(job)
        .join(format!("{case}_{form}.tif"));
    if !path.exists() {
        return None;
    }
    let doc = DrDocument::open(path.to_str()?).expect("take1's TIFF opens");
    Some(doc.flattened().expect("take1's TIFF flattens"))
}

#[test]
fn turned_and_flipped_renders_match_photoshops_take1_within_two_codes() {
    let fixture = fixture();
    let doc = take_doc8();
    let mut runs: Vec<(&str, &str, &str, DrDocument)> = TURNS
        .iter()
        .flat_map(|turn| {
            let job = if matches!(turn, Turn::Fh | Turn::Fv) {
                "flip_ft"
            } else {
                "crop_turn"
            };
            let out = turn.plain(&doc);
            [
                (job, turn.case(), "after", out.clone()),
                (job, turn.case(), "reopened", out),
            ]
        })
        .collect();
    let (_, flip) = ft_matrices()[3];
    let l1 = index8(&doc, "L1");
    let flipped = doc
        .transform_layer(l1, Affine::from_array(flip), FilterType::Nearest)
        .unwrap();
    runs.push(("flip_ft", "FT-F", "reopened", flipped));
    let mut compared = 0;
    for (job, case, form, ours) in runs {
        let Some(ps) = take1_tiff(job, case, form) else {
            eprintln!("skipping {case}_{form}: take1 fixture absent");
            continue;
        };
        let flat = ours.flattened().unwrap();
        assert_eq!(
            flat.dimensions(),
            ps.dimensions(),
            "{case}_{form}: the canvas"
        );
        let mut worst = 0i32;
        for record in ["L0", "L1"] {
            let b = &fixture["cases"][case]["records"][record]["bounds_after"];
            let n = |i: usize| b[i].as_i64().unwrap() as i32;
            let rect = (n(0), n(1), (n(2) - n(0)) as u32, (n(3) - n(1)) as u32);
            for (x, y) in styled_pixels(rect, flat.dimensions()) {
                let (a, p) = (
                    flat.get_pixel(x as u32, y as u32).0,
                    ps.get_pixel(x as u32, y as u32).0,
                );
                for c in 0..3 {
                    let e = (i32::from(a[c]) - i32::from(p[c])).abs();
                    worst = worst.max(e);
                    // BOUND 2 codes (module doc).
                    assert!(
                        e <= 2,
                        "{case}_{form}: {record} at ({x}, {y}): {a:?} against {p:?}"
                    );
                }
            }
        }
        eprintln!("{case}_{form}: worst {worst} codes");
        compared += 1;
    }
    assert!(
        compared == 0 || compared == 11,
        "every render or none: {compared}"
    );
}
