//! Layer styles and Blend If on a CMYK document's ADJUSTMENT layer, at 8
//! and 16 bits through the FFI: the RGB adjustment layer's rules
//! (`style_adjustment`) on ink.
//! The shape is the mask over the canvas (S-MASK: an unmasked adjustment's
//! outside stroke falls off the canvas), the adjustment applies after its own
//! Below effects (POST: it tones its drop shadow), Fill scales its `k` and
//! never the effects, This Layer reads the adjusted ink and Underlying the
//! composite beneath, a clip base's members land over the adjusted ink inside
//! the shape, a deep knockout over an opaque Background is the plain record,
//! and the style survives `.rz`, a layered CMYK PSD and Merge Down past the
//! canvas.
//!
//! The ORACLES never read the implementation: the backdrop is the stored
//! sample (`cmyk_style_fixture::sample`), the tone is `levels` / `1 − ink`
//! and the moves are `cmyk_structure_fixture::{tone, over}` on `L = 1 − ink`.
//! The Blend If cases sit far from their knee, so each weight is 0 or 1 by
//! inspection (the ramp itself is `cmyk_blend_if.rs`'s). Bound: one 8-bit
//! code (257) at 8 bits, 2 sixteen-bit codes at 16, per plate on paper.

use std::ffi::CString;
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_precision_color::dr_precision_doc_add_adjustment;
use doctored_core::ffi_psd::dr_precision_doc_save_psd;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::style::{BlendIf, BlendIfChannel};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_psd_import::{open_psd, report};
use crate::cmyk_structure_fixture::{
    assert_flat, cmyk_word, composite, free, levels, open, over, save, then, tone, try_edit,
    with_mask, Ink, LIMIT,
};
use crate::cmyk_style_fixture::{backdrop, flat, hard_rect, layer, sample, styled};
use crate::common::{cpath, take_err_string};

const W: u32 = 40;
const H: u32 = 24;
/// Four bands of 10 columns. Black 45 % reads 140 under its own ramp but
/// 124 adjusted by Levels and 115 inverted; 90 % reads 25, 43 and 230
/// (module doc: every weight far from the 128 / 129 knee).
const COLUMNS: [[f64; 4]; 4] = [
    [20.0, 10.0, 30.0, 0.0],
    [0.0, 30.0, 0.0, 45.0],
    [60.0, 10.0, 0.0, 90.0],
    [30.0, 50.0, 20.0, 20.0],
];
/// The adjustment's mask `(x0, y0, x1, y1)`.
const MASK: (u32, u32, u32, u32) = (8, 6, 24, 18);
const STROKE_INK: [f64; 4] = [0.0, 1.0, 1.0, 0.0];
const SHADOW_INK: [f64; 4] = [0.0, 0.0, 0.0, 1.0];
/// The drop shadow's shift: angle 180 moves it +6 in x.
const SHADOW_DX: u32 = 6;

fn bound(depth: u32) -> u16 {
    if depth == 8 {
        257
    } else {
        2
    }
}

fn band(x: u32) -> usize {
    (x as usize * COLUMNS.len()) / W as usize
}

fn description(op: &str) -> String {
    format!(r#"{{"type":"adjust","evaluation":"precision_v1","op":"{op}","params":{{}}}}"#)
}

/// `op`'s description: `levels` is `cmyk_structure_fixture::LEVELS`.
fn adjustment_of(op: &str) -> String {
    if op == "levels" {
        crate::cmyk_structure_fixture::LEVELS.to_string()
    } else {
        description(op)
    }
}

/// An adjustment record named "Adj" added on top, the input freed.
fn with_adjustment(doc: *mut PrecisionDocument, op: &str) -> *mut PrecisionDocument {
    let text = CString::new(adjustment_of(op)).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            c"Adj".as_ptr(),
            text.as_ptr(),
            1.0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{op}: {}", take_err_string(err));
    free(doc);
    out
}

/// The backdrop and an `op` adjustment above it (layer 1).
fn scene(depth: u32, op: &str) -> *mut PrecisionDocument {
    with_adjustment(backdrop(W, H, depth, &COLUMNS), op)
}

/// The oracle's tone of `op` on one ink.
fn toned_by(op: &str, ink: [f64; 4]) -> [f64; 4] {
    match op {
        "levels" => levels(ink),
        _ => ink.map(|v| 1.0 - v),
    }
}

fn ink4(ink: Ink) -> [f64; 4] {
    [ink[0], ink[1], ink[2], ink[3]]
}

fn opaque(ink: [f64; 4]) -> Ink {
    [ink[0], ink[1], ink[2], ink[3], 1.0]
}

/// The stored backdrop at `(x, y)`.
fn back(doc: *const PrecisionDocument, x: u32, y: u32) -> Ink {
    sample(doc, 0, x, y).expect("the backdrop covers the canvas")
}

/// `ink` toned toward `adjusted` by `k` (the adjustment record's step).
fn toned(ink: Ink, adjusted: [f64; 4], k: f64) -> Ink {
    let mut out = ink;
    tone(&mut out, adjusted, k, BlendMode::Normal);
    out
}

fn inside(x: u32, y: u32, rect: (u32, u32, u32, u32)) -> bool {
    x >= rect.0 && x < rect.2 && y >= rect.1 && y < rect.3
}

/// Every pixel `want` answers against the composite.
fn check(
    doc: *const PrecisionDocument,
    depth: u32,
    what: &str,
    want: impl Fn(u32, u32) -> Option<Ink>,
) {
    let mut judged = 0;
    for y in 0..H {
        for x in 0..W {
            if let Some(want) = want(x, y) {
                assert_flat(
                    composite(doc, x as i32, y as i32),
                    want,
                    bound(depth),
                    &format!("{what} ({depth}-bit) at ({x}, {y})"),
                );
                judged += 1;
            }
        }
    }
    assert!(judged > 0, "{what}: nothing judged");
}

fn stroke(size: u32) -> Value {
    json!({"type":"stroke","size":size,"position":"outside","ink":STROKE_INK,"blend":"normal","opacity":1.0})
}

fn shadow() -> Value {
    json!({"type":"drop_shadow","ink":SHADOW_INK,"blend":"normal","opacity":1.0,"angle":180,
        "use_global_light":false,"distance":SHADOW_DX,"spread":0,"size":0,"layer_knocks_out":false})
}

fn black(this_layer: [u8; 4], underlying: [u8; 4]) -> Value {
    json!({"channel":"black","this_layer":this_layer,"underlying":underlying})
}

fn with_blend_if(doc: *mut PrecisionDocument, ranges: &Value) -> *mut PrecisionDocument {
    then(
        doc,
        &json!({"op":"layer_blend_if","version":1,"layer":1,"blend_if":ranges}).to_string(),
        &[],
    )
}

// ------------------------------------------------------------ (a) Blend If --

#[test]
fn this_layer_reads_the_adjusted_ink_and_underlying_the_composite_beneath() {
    const FULL: [u8; 4] = [0, 0, 255, 255];
    const HALF: [u8; 4] = [0, 0, 128, 128];
    // The weight per band: Black's value `255 − code` ≤ 128 shows the record.
    let cases = [
        ("levels", black(HALF, FULL), [0.0, 1.0, 1.0, 0.0]),
        ("invert", black(HALF, FULL), [1.0, 1.0, 0.0, 1.0]),
        ("levels", black(FULL, HALF), [0.0, 0.0, 1.0, 0.0]),
        ("invert", black(FULL, HALF), [0.0, 0.0, 1.0, 0.0]),
    ];
    for depth in [8, 16] {
        for (op, ranges, weights) in &cases {
            let doc = with_blend_if(scene(depth, op), ranges);
            check(doc, depth, &format!("{op} {ranges}"), |x, y| {
                let b = back(doc, x, y);
                Some(if weights[band(x)] == 1.0 {
                    opaque(toned_by(op, ink4(b)))
                } else {
                    b
                })
            });
            free(doc);
        }
    }
}

// --------------------------------------------------- (b) S-MASK and POST --

#[test]
fn an_unmasked_stroke_rings_nothing_and_a_masked_shadow_is_toned_inside_the_mask() {
    for depth in [8, 16] {
        let doc = styled(scene(depth, "levels"), 1, &json!({"effects":[stroke(2)]}));
        check(doc, depth, "unmasked stroke", |x, y| {
            Some(opaque(levels(ink4(back(doc, x, y)))))
        });
        free(doc);

        let doc = with_mask(scene(depth, "levels"), 1, &hard_rect(W, H, MASK));
        let doc = styled(doc, 1, &json!({"effects":[shadow()]}));
        let (x0, y0, x1, y1) = MASK;
        check(doc, depth, "masked shadow", |x, y| {
            let rows = y > y0 && y + 1 < y1;
            let b = back(doc, x, y);
            if rows && x > x0 + SHADOW_DX && x + 1 < x1 {
                // Inside the mask under the shadow: the shadow, toned (POST).
                Some(opaque(levels(SHADOW_INK)))
            } else if rows && x > x1 && x + 1 < x1 + SHADOW_DX {
                // Outside the mask: the shadow over the backdrop, untoned.
                Some(opaque(SHADOW_INK))
            } else if rows && x > x0 && x + 1 < x0 + SHADOW_DX {
                Some(opaque(levels(ink4(b))))
            } else if x >= x1 + SHADOW_DX + 2 || y + 2 < y0 || y >= y1 + 2 {
                Some(b)
            } else {
                None
            }
        });
        free(doc);
    }
}

// --------------------------------------------------------------- (c) Fill --

#[test]
fn fill_scales_the_tone_and_never_the_effects() {
    for depth in [8, 16] {
        let doc = with_mask(scene(depth, "levels"), 1, &hard_rect(W, H, MASK));
        let doc = styled(doc, 1, &json!({"fill_opacity":0.5,"effects":[stroke(2)]}));
        let (x0, y0, x1, y1) = MASK;
        check(doc, depth, "fill 50 %", |x, y| {
            let b = back(doc, x, y);
            if (y == y0 - 1 || y == y0 - 2) && x >= x0 + 2 && x + 2 < x1 {
                // The ring above the mask at full strength.
                Some(opaque(STROKE_INK))
            } else if x > x0 && x + 1 < x1 && y > y0 && y + 1 < y1 {
                // The Fill's 50 % at its byte (#2).
                Some(toned(b, levels(ink4(b)), crate::common::byte_unit(0.5)))
            } else if x >= x1 + 4 || y >= y1 + 4 || y + 4 < y0 {
                Some(b)
            } else {
                None
            }
        });
        free(doc);
    }
}

// ---------------------------------------------------------- (d) clip base --

#[test]
fn a_styled_adjustment_clip_base_shows_its_member_over_the_adjusted_ink_inside_the_shape() {
    const MEMBER: (u32, u32, u32, u32) = (16, 2, 36, 22);
    const MEMBER_INK: [f64; 4] = [0.0, 100.0, 100.0, 0.0];
    let half: Vec<u16> = (0..W * H)
        .map(|p| {
            if inside(p % W, p / W, MEMBER) {
                32768
            } else {
                0
            }
        })
        .collect();
    let a = 32768.0 / 65535.0;
    for depth in [8, 16] {
        let doc = with_mask(scene(depth, "levels"), 1, &hard_rect(W, H, MASK));
        let doc = styled(doc, 1, &json!({"effects":[stroke(1)]}));
        let doc = layer(doc, "Member", MEMBER_INK, &half);
        let doc = then(
            doc,
            r#"{"op":"layer_clipped","layer":2,"clipped":true}"#,
            &[],
        );
        let (x0, y0, x1, y1) = MASK;
        check(doc, depth, "clip base", |x, y| {
            let b = back(doc, x, y);
            let rows = y > y0 && y + 1 < y1;
            if rows && x > MEMBER.0 && x + 1 < x1 {
                let mut out = opaque(levels(ink4(b)));
                over(
                    &mut out,
                    MEMBER_INK.map(|v| v / 100.0),
                    a,
                    BlendMode::Normal,
                );
                Some(out)
            } else if rows && x > x0 && x + 1 < MEMBER.0 {
                Some(opaque(levels(ink4(b))))
            } else if x >= x1 + 4 || y >= y1 + 4 || y + 4 < y0 {
                // The member outside the shape shows nothing.
                Some(b)
            } else {
                None
            }
        });
        free(doc);
    }
}

// ----------------------------------------------------------- (e) knockout --

#[test]
fn a_deep_knockout_over_an_opaque_background_is_the_plain_record() {
    for depth in [8, 16] {
        let plain = styled(scene(depth, "levels"), 1, &json!({"fill_opacity":0.6}));
        let knocked = styled(
            scene(depth, "levels"),
            1,
            &json!({"fill_opacity":0.6,"knockout":"deep"}),
        );
        let (a, b) = (flat(plain), flat(knocked));
        for (i, (p, q)) in a.iter().zip(&b).enumerate() {
            for c in 0..4 {
                assert!(
                    p[c].abs_diff(q[c]) <= bound(depth),
                    "{depth}-bit pixel {i} plate {c}: plain {p:?} knocked out {q:?}"
                );
            }
        }
        free(plain);
        free(knocked);
    }
}

// --------------------------------------------------------- (f) Fill only --

#[test]
fn a_fill_only_style_takes_the_styled_path() {
    for depth in [8, 16] {
        for op in ["levels", "invert"] {
            let doc = styled(scene(depth, op), 1, &json!({"fill_opacity":0.4}));
            check(doc, depth, &format!("{op} fill 40 %"), |x, y| {
                let b = back(doc, x, y);
                Some(toned(b, toned_by(op, ink4(b)), 0.4))
            });
            free(doc);
        }
    }
}

// --------------------------------------------------------------- (g) .rz --

fn full_style(depth: u32) -> *mut PrecisionDocument {
    let doc = styled(
        scene(depth, "levels"),
        1,
        &json!({"fill_opacity":0.7,"effects":[stroke(2)]}),
    );
    with_blend_if(doc, &black([0, 0, 128, 128], [0, 0, 255, 255]))
}

#[test]
fn rz_writes_word_86_and_reads_the_style_back_byte_for_byte() {
    for depth in [8, 16] {
        let doc = full_style(depth);
        let dir = TempDir::new().unwrap();
        let bytes = save(doc, &dir.path().join("first.rz"));
        assert_eq!(cmyk_word(&bytes), 86, "{depth}-bit");
        let back = open(&dir.path().join("first.rz")).unwrap();
        let again = save(back, &dir.path().join("second.rz"));
        assert_eq!(bytes, again, "{depth}-bit: the resave");
        let (a, b) = (unsafe { &*doc }, unsafe { &*back });
        let (la, lb) = (a.layer(1).unwrap(), b.layer(1).unwrap());
        assert_eq!(la.reported_style(), lb.reported_style(), "{depth}-bit");
        assert!(la.reported_style().is_some());
        assert_eq!(la.blend_if(), lb.blend_if(), "{depth}-bit");
        assert_eq!(flat(doc), flat(back), "{depth}-bit");
        free(doc);
        free(back);
    }
}

// ---------------------------------------------------------------- (h) PSD --

fn named<'a>(doc: &'a PrecisionDocument, name: &str) -> &'a PrecisionDocumentLayer {
    (0..doc.layer_count())
        .map(|i| doc.layer(i).unwrap())
        .find(|l| l.name() == name)
        .unwrap_or_else(|| panic!("no layer {name}"))
}

#[test]
fn a_layered_cmyk_psd_carries_the_style_and_range_both_ways() {
    for depth in [8, 16] {
        let doc = full_style(depth);
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("styled.psd");
        let mut err = ptr::null_mut();
        let ok = unsafe {
            dr_precision_doc_save_psd(
                doc,
                ptr::null(),
                cpath(&path).as_ptr(),
                0,
                0,
                LIMIT,
                0,
                std::ptr::null(),
                &mut err,
            )
        };
        assert!(ok, "{depth}-bit: {}", take_err_string(err));
        let r = report(&path);
        let losses = r["verified_losses"].as_array().unwrap();
        assert!(
            !losses.iter().any(|l| l.as_str().unwrap().contains("“Adj”")),
            "{depth}-bit: {losses:?}"
        );
        let back = open_psd(&path, 0).unwrap();
        let (a, b) = (
            named(unsafe { &*doc }, "Adj"),
            named(unsafe { &*back }, "Adj"),
        );
        assert_eq!(
            b.blend_if(),
            Some(BlendIf {
                channel: BlendIfChannel::Black,
                this_layer: [0, 0, 128, 128],
                underlying: [0, 0, 255, 255],
                ..BlendIf::default()
            }),
            "{depth}-bit"
        );
        let (sa, sb) = (a.style().unwrap(), b.style().unwrap());
        assert!(
            (sa.fill_opacity - sb.fill_opacity).abs() < 0.01,
            "{depth}-bit"
        );
        let kinds = |l: &PrecisionDocumentLayer| -> Vec<Value> {
            let style: Value = serde_json::from_str(&l.reported_style().unwrap()).unwrap();
            style["effects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["type"].clone())
                .collect()
        };
        assert_eq!(kinds(a), vec![json!("stroke")], "{depth}-bit");
        assert_eq!(kinds(a), kinds(b), "{depth}-bit");
        free(doc);
        free(back);
    }
}

// --------------------------------------------------------- (k) Merge Down --

/// A CMYK document at `depth` of one opaque "Leaf" `W + 20` wide at
/// `(-20, 0)`, converted from RGB into the press profile.
fn leaf_doc(depth: u32) -> *mut PrecisionDocument {
    let srgb = IccProfile::srgb();
    let (lw, lh) = (W + 20, H);
    let pixels: Vec<[u16; 4]> = crate::cmyk_structure_fixture::varied(lw, lh, 7)
        .into_iter()
        .map(|p| [p[0], p[1], p[2], 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(lw, lh, pixels, srgb.clone(), None).unwrap();
    let leaf = PrecisionDocumentLayer::raster("Leaf", image, (-20, 0), 1.0, true).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            W,
            H,
            srgb,
            Metadata::default(),
            Resolution::default(),
            vec![leaf],
        )
        .unwrap(),
    ));
    let profile = press_profile("Press Test");
    let options = CString::new(format!(r#"{{"depth":{depth}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            rgb,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take_err_string(err));
    free(rgb);
    out
}

fn merged(doc: *const PrecisionDocument) -> (Vec<[u16; 5]>, (i32, i32)) {
    let out = try_edit(doc, r#"{"op":"merge_down","layer":1}"#, &[])
        .unwrap_or_else(|e| panic!("merge down: {e}"));
    assert!(!out.is_null(), "merge down changed nothing");
    let d = unsafe { &*out };
    assert_eq!(d.layer_count(), 1);
    let got = (
        d.cmyk_layer_samples(0).unwrap(),
        d.layer(0).unwrap().offset(),
    );
    free(out);
    got
}

#[test]
fn merge_down_of_a_styled_adjustment_tones_its_frame_past_the_canvas() {
    for depth in [8, 16] {
        let plain = with_adjustment(leaf_doc(depth), "invert");
        let leaf = unsafe { &*plain }.cmyk_layer_samples(0).unwrap();
        let styled_doc = styled(
            with_adjustment(leaf_doc(depth), "invert"),
            1,
            &json!({"effects":[stroke(3)]}),
        );
        let (want, want_at) = merged(plain);
        let (got, got_at) = merged(styled_doc);
        assert_eq!(want_at, (-20, 0), "{depth}-bit");
        assert_eq!(got_at, want_at, "{depth}-bit");
        let lw = (W + 20) as usize;
        let max = 65535.0;
        for (i, (p, q)) in want.iter().zip(&got).enumerate() {
            for c in 0..4 {
                // No stroke ring inside the frame: the styled merge is the
                // plain one everywhere.
                assert!(
                    p[c].abs_diff(q[c]) <= bound(depth),
                    "{depth}-bit leaf pixel {i} plate {c}: plain {p:?} styled {q:?}"
                );
            }
            if i % lw < 20 {
                // Columns −20..−1 are inverted.
                for c in 0..4 {
                    let inverted = max - f64::from(leaf[i][c]);
                    assert!(
                        (f64::from(q[c]) - inverted).abs() <= f64::from(bound(depth)),
                        "{depth}-bit column {} plate {c}: {q:?} vs inverted {inverted}",
                        i % lw
                    );
                }
            }
        }
        free(plain);
        free(styled_doc);
    }
}

// ------------------------------------------------- (i), (j) Photoshop --

/// Photoshop's K file `name` from take1's `jobs/k8/` (outside the repo:
/// a CMYK file embeds the 557 KB SWOP profile), `None` when absent.
fn take_fixture(name: &str) -> Option<std::path::PathBuf> {
    crate::common::take1_558("k8", name)
}

/// A CMYK8 document's flattened ink as 8-bit codes, row-major.
fn codes8(doc: *const PrecisionDocument) -> Vec<[f64; 4]> {
    flat(doc)
        .into_iter()
        .map(|s| [0, 1, 2, 3].map(|c| f64::from(s[c]) / 257.0))
        .collect()
}

/// Photoshop's own row `name` (its CMYK TIFF) as 8-bit ink codes.
fn photoshop_row(name: &str) -> Vec<[f64; 4]> {
    let path = take_fixture(name).expect("the take folder");
    let doc = open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    let out = codes8(doc);
    free(doc);
    out
}

/// Rebuilt over Photoshop's own k0 (module doc of the runner, K): the Invert
/// record, then k1's mask and effects or k2's Black range.
fn rebuilt_k(row: &str, shadow: [f64; 4]) -> *mut PrecisionDocument {
    let k0 = open(&take_fixture("k0_cmyk8.tif").expect("the take folder")).unwrap();
    let (w, h) = unsafe { &*k0 }.dimensions();
    let doc = with_adjustment(k0, "invert");
    if row == "k2" {
        return with_blend_if(doc, &black([0, 0, 128, 128], [0, 0, 255, 255]));
    }
    let doc = with_mask(doc, 1, &hard_rect(w, h, (24, 16, 72, 48)));
    styled(
        doc,
        1,
        &json!({"effects":[
            {"type":"stroke","size":4,"position":"outside","color":"#ff0000","blend":"normal","opacity":1.0},
            {"type":"drop_shadow","ink":shadow,"blend":"normal","opacity":1.0,"angle":180,
             "use_global_light":false,"distance":8,"spread":0,"size":0,"layer_knocks_out":false}]}),
    )
}

#[test]
fn photoshops_k_rows_meet_the_shipped_rule() {
    if take_fixture("k0_cmyk8.tif").is_none() {
        eprintln!("skipping: blend-if-multichannel-558/take1 is absent");
        return;
    }
    let k1 = photoshop_row("k1_cmyk8.tif");
    let k2 = photoshop_row("k2_cmyk8.tif");
    let w = 96usize;
    // K1's S: Photoshop's shadow alone at x 77..78 (row 30), as its ink.
    let s = [0, 1, 2, 3].map(|c| (k1[30 * w + 77][c] + k1[30 * w + 78][c]) / 2.0 / 255.0);
    let doc = rebuilt_k("k1", s);
    let got = codes8(doc);
    for y in 17..=46usize {
        for x in (25..=30usize).chain(33..=70) {
            let i = y * w + x;
            for c in 0..4 {
                assert!(
                    (got[i][c] - k1[i][c]).abs() <= 2.0,
                    "k1 ({x}, {y}) plate {c}: Doctored {:?} Photoshop {:?}",
                    got[i],
                    k1[i]
                );
            }
        }
    }
    free(doc);
    let doc = rebuilt_k("k2", s);
    for (i, (g, p)) in codes8(doc).iter().zip(&k2).enumerate() {
        for c in 0..4 {
            assert!(
                (g[c] - p[c]).abs() <= 2.0,
                "k2 pixel {i} plate {c}: Doctored {g:?} Photoshop {p:?}"
            );
        }
    }
    free(doc);
}

#[test]
fn photoshops_own_k_psds_import_with_the_style_on_the_adjustment_record() {
    for (name, effects) in [("k1_cmyk8.psd", true), ("k2_cmyk8.psd", false)] {
        let Some(path) = take_fixture(name) else {
            eprintln!("skipping: blend-if-multichannel-558/take1 is absent");
            return;
        };
        let r = report(&path);
        let losses = r["verified_losses"].as_array().unwrap();
        assert!(
            !losses
                .iter()
                .any(|l| l.as_str().unwrap().contains("discards")),
            "{name}: {losses:?}"
        );
        let back = open_psd(&path, 0).unwrap();
        let d = unsafe { &*back };
        let adjustment = (0..d.layer_count())
            .map(|i| d.layer(i).unwrap())
            .find(|l| l.cmyk_source().is_none() && l.children().is_none())
            .expect("the Invert record");
        if effects {
            let style: Value =
                serde_json::from_str(&adjustment.reported_style().expect("its style")).unwrap();
            let kinds: Vec<&str> = style["effects"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["type"].as_str().unwrap())
                .collect();
            assert!(
                kinds.contains(&"stroke") && kinds.contains(&"drop_shadow"),
                "{kinds:?}"
            );
            assert!(adjustment.mask().is_some(), "{name}: the rect mask");
        } else {
            assert_eq!(
                adjustment.blend_if(),
                Some(BlendIf {
                    channel: BlendIfChannel::Black,
                    this_layer: [0, 0, 128, 128],
                    underlying: [0, 0, 255, 255],
                    ..BlendIf::default()
                }),
                "{name}"
            );
        }
        free(back);
    }
}
