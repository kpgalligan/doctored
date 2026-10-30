//! KNOCKOUT (Shallow / Deep) and "Blend Interior Effects as Group" in the
//! TYPED compositor — Phase 12E-3b's twin of `knockout_composite.rs`. Every
//! rule under test is `style_knockout`'s D3–D8; `precision_style_knockout`
//! restates none of its own, so these are the SAME scenes one depth up.
//!
//! # The tolerance, stated BEFORE anything was measured
//!
//! **T3 — the typed render against the oracle: 1 code of 65535 per channel,
//! ALPHA INCLUDED.** The oracle is written from D3–D6 in f64 — `over` is the
//! W3C source-over equation transcribed from the specification and `mix` is
//! the premultiplied lerp D3 names — and every scene is hard-edged
//! (canvas-sized opaque fills at PROMOTED codes, `257·c8`) except the one that
//! exists to probe a soft edge. One code of 65535 is the whole quantization
//! budget and nothing hides in it.
//!
//! Three claims are EQUALITIES rather than tolerances: a typed document
//! carrying neither key renders exactly what it rendered before the keys
//! existed; a float document refuses what it cannot hold (an effect blend mode
//! outside its float set) by the 32-bit float sentence; and
//! a `linear_light` document refuses one by the evaluation sentence.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_retouch::dr_precision_doc_retouch_source;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::style::LayerStyle;
use serde_json::{json, Value};

use crate::common::byte_unit;

const W: u32 = 8;
const H: u32 = 8;
const PX: usize = (4 * W + 4) as usize;

const BG: [u8; 4] = [204, 34, 34, 255];
const MID: [u8; 4] = [34, 204, 34, 255];
const HIGH: [u8; 4] = [34, 34, 204, 255];
const EXTRA: [u8; 4] = [204, 204, 34, 255];

/// T3: one 16-bit code, alpha included.
const T3: u16 = 1;

// -------------------------------------------------------------- plumbing --

fn promoted(p: [u8; 4]) -> [u16; 4] {
    p.map(|v| u16::from(v) * 257)
}

fn image(pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap()
}

fn layer(name: &str, color: [u8; 4]) -> Layer {
    Layer::raster(
        name,
        image(vec![promoted(color); (W * H) as usize]),
        (0, 0),
        1.0,
        true,
    )
    .unwrap()
}

/// A canvas-sized layer whose alpha RAMPS across the row, for the one soft-edge
/// scene. Colour is constant so only the shape varies.
fn ramp(name: &str, color: [u8; 4]) -> Layer {
    let pixels = (0..W * H)
        .map(|i| {
            let a = ((i % u64::from(W) as u32) * 65535 / (W - 1)) as u16;
            let c = promoted(color);
            [c[0], c[1], c[2], a]
        })
        .collect();
    Layer::raster(name, image(pixels), (0, 0), 1.0, true).unwrap()
}

fn parse(json: &str) -> LayerStyle {
    LayerStyle::from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn styled(layer: Layer, json: &str) -> Layer {
    layer
        .with_layer_style(Some(&parse(json)), (W, H))
        .unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn document(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn render(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn knockout_style(knockout: &str, fill: f32) -> String {
    format!("{{\"knockout\":\"{knockout}\",\"fill_opacity\":{fill}}}")
}

fn edit(doc: &PrecisionDocument, request: Value) -> Result<Option<PrecisionDocument>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        let text = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        assert!(out.is_null());
        return Err(text);
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

// --------------------------------------------------------------- oracle --

fn u(px: [u8; 4]) -> [f64; 4] {
    px.map(|v| f64::from(v) / 255.0)
}

/// `over(B, C, a, Normal)`: the W3C source-over equation, transcribed
/// (`co = as·((1 − ab)·Cs + ab·B(Cb, Cs)) + (1 − as)·ab·Cb`,
/// `ao = as + ab·(1 − as)`), straight in and straight out.
fn over(b: [f64; 4], cs: [f64; 3], sa: f64) -> [f64; 4] {
    let ab = b[3];
    let ao = sa + ab * (1.0 - sa);
    if ao <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        let co = sa * ((1.0 - ab) * cs[c] + ab * cs[c]) + (1.0 - sa) * ab * b[c];
        out[c] = co / ao;
    }
    out[3] = ao;
    out
}

/// D3's premultiplied lerp.
fn mix(a: [f64; 4], k: [f64; 4], t: f64) -> [f64; 4] {
    let alpha = a[3] + (k[3] - a[3]) * t;
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        let x = a[c] * a[3];
        let y = k[c] * k[3];
        out[c] = (x + (y - x) * t) / alpha;
    }
    out[3] = alpha;
    out
}

/// D3's whole statement for one opaque uniform pixel: `K = over(B0, C, q)`,
/// `out = mix(acc, K, s·opacity)`.
fn unit_pixel(
    acc: [u8; 4],
    b0: [f64; 4],
    color: [u8; 4],
    q: f64,
    s: f64,
    opacity: f64,
) -> [f64; 4] {
    let c = u(color);
    let k = if q > 0.0 {
        over(b0, [c[0], c[1], c[2]], q)
    } else {
        b0
    };
    mix(u(acc), k, s * opacity)
}

const CLEAR: [f64; 4] = [0.0; 4];

fn assert_t3(got: [u16; 4], want: [f64; 4], what: &str) {
    for c in 0..4 {
        let expected = (want[c].clamp(0.0, 1.0) * 65535.0).round() as u16;
        assert!(
            got[c].abs_diff(expected) <= T3,
            "{what}: channel {c} got {} want {} (T3 = {T3})",
            got[c],
            expected
        );
    }
}

// ------------------------------------------------------------ D5: the BG --

#[test]
fn root_shallow_over_a_qualifying_background_punches_to_it_at_every_fill() {
    for fill in [0.0f64, 0.25, 1.0] {
        for mode in ["shallow", "deep"] {
            let doc = document(vec![
                layer("BG", BG),
                styled(layer("Top", HIGH), &knockout_style(mode, fill as f32)),
            ]);
            let got = render(&doc)[PX];
            // `B0` is the accumulator right after the Background composited:
            // an opaque BG over transparent is BG itself.
            let want = unit_pixel(BG, u(BG), HIGH, byte_unit(fill), 1.0, 1.0);
            assert_t3(got, want, &format!("{mode} at fill {fill}"));
        }
    }
}

#[test]
fn every_typed_d5_disqualifier_leaves_the_root_backdrop_transparent() {
    // The byte clauses, plus the ONE the typed rule has to name for itself:
    // Blend If is a LAYER field at this depth, so "no style" does not cover it.
    let top = || styled(layer("Top", HIGH), &knockout_style("deep", 0.0));
    let hole = |doc: PrecisionDocument, what: &str| {
        let got = render(&doc)[PX];
        assert_t3(got, mix(CLEAR, CLEAR, 1.0), what);
    };
    hole(
        document(vec![
            layer("BG", BG)
                .with_blend_mode(BlendMode::Multiply)
                .unwrap(),
            top(),
        ]),
        "a non-Normal entry 0",
    );
    hole(
        document(vec![
            Layer::raster(
                "BG",
                image(vec![promoted(BG); (W * H) as usize]),
                (0, 0),
                0.5,
                true,
            )
            .unwrap(),
            top(),
        ]),
        "entry 0 below opacity 1",
    );
    hole(
        document(vec![
            Layer::raster(
                "BG",
                image(vec![promoted(BG); (W * H) as usize]),
                (1, 0),
                1.0,
                true,
            )
            .unwrap(),
            top(),
        ]),
        "entry 0 off the origin",
    );
    hole(
        document(vec![
            styled(layer("BG", BG), "{\"fill_opacity\":0.9}"),
            top(),
        ]),
        "a STYLED entry 0",
    );
    // The typed-only clause: a Blend If on entry 0.
    let blend_if = layer("BG", BG)
        .with_blend_if(Some(
            parse("{\"blend_if\":{\"channel\":\"gray\",\"this\":[0,0,255,255],\"underlying\":[10,10,255,255]}}")
                .blend_if
                .unwrap(),
        ))
        .unwrap();
    hole(
        document(vec![blend_if, top()]),
        "entry 0 carrying a Blend If",
    );
    // And entry 1 CLIPPED, which would make entry 0 a clip base.
    let doc = document(vec![
        layer("BG", BG),
        styled(layer("Top", HIGH), &knockout_style("deep", 0.0)).with_clipped(true),
    ]);
    let got = render(&doc)[PX];
    // A clipped member punches to the BASE-ONLY snapshot, which is the
    // Background's own pixels inside the clip buffer — so this one is NOT a
    // hole, and the point is only that D5 did not fire for the LEVEL.
    assert_t3(got, u(BG), "entry 1 clipped");
}

// ----------------------------------------------------------- D4: groups --

#[test]
fn a_pass_through_group_gives_shallow_its_own_entry_state_and_deep_the_background() {
    // `[BG, Mid, P(Pass Through){K}]`: Shallow sees BG-under-Mid, Deep sees BG.
    for (mode, want) in [("shallow", MID), ("deep", BG)] {
        let doc = document(vec![
            layer("BG", BG),
            layer("Mid", MID),
            Layer::pass_through(
                "P",
                vec![styled(layer("K", HIGH), &knockout_style(mode, 0.0))],
                1.0,
                true,
            )
            .unwrap(),
        ]);
        assert_t3(
            render(&doc)[PX],
            u(want),
            &format!("{mode} in a pass-through group"),
        );
    }
}

#[test]
fn an_isolated_group_is_a_hard_stop_for_both_modes() {
    for mode in ["shallow", "deep"] {
        let doc = document(vec![
            layer("BG", BG),
            layer("Mid", MID),
            Layer::isolated_normal(
                "I",
                vec![styled(layer("K", HIGH), &knockout_style(mode, 0.0))],
                1.0,
                true,
            )
            .unwrap(),
        ]);
        // The group's buffer starts transparent, so the hole shows what lies
        // beneath the GROUP once the group composites: Mid.
        assert_t3(
            render(&doc)[PX],
            u(MID),
            &format!("{mode} inside an isolated group"),
        );
    }
}

#[test]
fn a_pass_through_group_nested_in_an_isolated_one_keeps_both_boundaries() {
    // `[BG, I(Normal){O, P(Pass Through){K}}]`: Shallow reads P's entry state,
    // which is I's buffer after O; Deep is stopped by I and reads transparent,
    // so the hole shows BG.
    for (mode, want) in [("shallow", u(MID)), ("deep", u(BG))] {
        let doc = document(vec![
            layer("BG", BG),
            Layer::isolated_normal(
                "I",
                vec![
                    layer("O", MID),
                    Layer::pass_through(
                        "P",
                        vec![styled(layer("K", HIGH), &knockout_style(mode, 0.0))],
                        1.0,
                        true,
                    )
                    .unwrap(),
                ],
                1.0,
                true,
            )
            .unwrap(),
        ]);
        assert_t3(render(&doc)[PX], want, &format!("{mode} nested"));
    }
}

// -------------------------------------------------------- D4: clip units --

#[test]
fn a_clipped_member_punches_to_its_base_alone_over_an_unstyled_base() {
    let doc = document(vec![
        layer("BG", BG),
        layer("Base", MID),
        styled(layer("Member", HIGH), &knockout_style("deep", 0.0)).with_clipped(true),
    ]);
    // The base-only snapshot is Base over a TRANSPARENT clip buffer, so the
    // hole shows Base's own pixels — never BG.
    assert_t3(render(&doc)[PX], u(MID), "an unstyled base");
}

#[test]
fn a_clipped_member_punches_to_its_base_alone_over_a_styled_base() {
    // A FILL-only style still routes the base through `composite_styled_into`,
    // which is the second of D4's three member loops. The base-only snapshot
    // is the unit buffer after the base's pixel pass — Base at fill 0.5 over a
    // TRANSPARENT clip buffer — so the hole carries that, and the finished
    // buffer composites onto BG at alpha 0.5.
    let doc = document(vec![
        layer("BG", BG),
        styled(layer("Base", MID), "{\"fill_opacity\":0.5}"),
        styled(layer("Member", HIGH), &knockout_style("deep", 0.0)).with_clipped(true),
    ]);
    let m = u(MID);
    assert_t3(
        render(&doc)[PX],
        over(u(BG), [m[0], m[1], m[2]], byte_unit(0.5)),
        "a styled base",
    );
}

#[test]
fn a_knockout_base_evaluates_its_members_against_its_own_base_only_state() {
    // The base itself carries a knockout, so its unit pass runs — and the
    // member's `B0` is that unit buffer after the base's PIXEL pass.
    let doc = document(vec![
        layer("BG", BG),
        styled(layer("Base", MID), &knockout_style("deep", 1.0)),
        styled(layer("Member", HIGH), &knockout_style("deep", 0.0)).with_clipped(true),
    ]);
    assert_t3(render(&doc)[PX], u(MID), "a knockout clip base");
}

// ------------------------------------------------------------ D3: shape --

#[test]
fn a_partial_alpha_shape_gets_no_fringe_at_q_one() {
    // D3's identity: at `q = 1` under Normal the unit pass equals the plain
    // render for ANY `B0`, so a soft edge gets no fringe. The two documents
    // take different arithmetic to the same number, which is why this is T3
    // rather than an equality.
    let plain = document(vec![layer("BG", BG), ramp("Soft", HIGH)]);
    let knocked = document(vec![
        layer("BG", BG),
        styled(ramp("Soft", HIGH), &knockout_style("deep", 1.0)),
    ]);
    let (a, b) = (render(&plain), render(&knocked));
    for (i, (p, k)) in a.iter().zip(b.iter()).enumerate() {
        for c in 0..4 {
            assert!(
                p[c].abs_diff(k[c]) <= T3,
                "pixel {i} channel {c}: plain {} vs knockout {}",
                p[c],
                k[c]
            );
        }
    }
}

// ----------------------------------------------------------------- D6 --

/// `[BG, Base(Multiply, colour overlay, optional flag)]`.
fn interior_document(grouped: bool, fill: f32) -> PrecisionDocument {
    let json = format!(
        "{{\"blend_interior_as_group\":{grouped},\"fill_opacity\":{fill},\"effects\":[\
         {{\"type\":\"color_overlay\",\"color\":\"#00ff00\",\"opacity\":1.0}}]}}"
    );
    document(vec![
        layer("BG", BG),
        styled(layer("Base", EXTRA), &json)
            .with_blend_mode(BlendMode::Multiply)
            .unwrap(),
    ])
}

#[test]
fn blend_interior_as_group_moves_the_four_grouped_contributions_into_the_unit() {
    // OFF (the default): the overlay lands on the accumulator with its
    // OWN Normal blend, so pure green shows. ON: the overlay joins the unit and
    // the unit meets the backdrop ONCE with the layer's Multiply.
    let off = render(&interior_document(false, 1.0))[PX];
    let on = render(&interior_document(true, 1.0))[PX];
    assert_ne!(off, on, "the flag has to change the picture");
    assert_t3(
        off,
        u([0, 255, 0, 255]),
        "OFF: the overlay is its own source",
    );
    // ON: green multiplied against the BG.
    let bg = u(BG);
    let want = [bg[0] * 0.0, bg[1] * 1.0, bg[2] * 0.0, 1.0];
    assert_t3(
        on,
        want,
        "ON: the unit meets the backdrop once, with Multiply",
    );
}

/// P7-39 at 16 bits: on a clip base with visible members the flag evaluates
/// ON. The unit is the base at fill 1, then the member (left half only,
/// clamped to the footprint), then the overlay at half opacity (alpha
/// restored); it meets the opaque backdrop ONCE with Multiply at
/// `q = fill`: `out = (1 − q)·BG + q·BG·U`.
#[test]
fn the_grouped_flag_evaluates_on_on_a_clip_base_with_visible_members_at_16_bits() {
    let build = |grouped: bool, fill: f32| {
        let json = format!(
            "{{\"blend_interior_as_group\":{grouped},\"fill_opacity\":{fill},\"effects\":[\
             {{\"type\":\"color_overlay\",\"color\":\"#00ff00\",\"opacity\":0.5}}]}}"
        );
        let half = (0..W * H)
            .map(|i| {
                if i % W < W / 2 {
                    promoted(HIGH)
                } else {
                    [0; 4]
                }
            })
            .collect();
        let member = Layer::raster("M", image(half), (0, 0), 1.0, true)
            .unwrap()
            .with_clipped(true);
        document(vec![
            layer("BG", BG),
            styled(layer("Base", EXTRA), &json)
                .with_blend_mode(BlendMode::Multiply)
                .unwrap(),
            member,
        ])
    };
    let bg = u(BG);
    let unit = |under: [u8; 4]| {
        let c = u(under);
        over([c[0], c[1], c[2], 1.0], [0.0, 1.0, 0.0], 0.5)
    };
    let expect = |under: [u8; 4], q: f64| {
        let k = unit(under);
        let mut out = [0.0, 0.0, 0.0, 1.0];
        for c in 0..3 {
            out[c] = (1.0 - q) * bg[c] + q * bg[c] * k[c];
        }
        out
    };
    for fill in [1.0f32, 0.5] {
        let got = render(&build(true, fill));
        let q = byte_unit(f64::from(fill));
        let row = (4 * W) as usize;
        assert_t3(
            got[row + 1],
            expect(HIGH, q),
            &format!("member half at fill {fill}"),
        );
        assert_t3(
            got[row + 6],
            expect(EXTRA, q),
            &format!("bare half at fill {fill}"),
        );
    }
    assert_ne!(
        render(&build(false, 1.0)),
        render(&build(true, 1.0)),
        "non-vacuity: the flag changes the picture"
    );
}

/// A cut is not a structure change, and since P7-39 the flag no longer asks
/// about members: the CURRENT-AND-BELOW plate of the base draws the grouped
/// base exactly as the document with its member HIDDEN does.
#[test]
fn the_grouped_flag_is_on_under_a_cut_at_16_bits() {
    let build = |grouped: bool, member_visible: bool| {
        let json = format!(
            "{{\"blend_interior_as_group\":{grouped},\"effects\":[\
             {{\"type\":\"color_overlay\",\"color\":\"#00ff00\",\"opacity\":1.0}}]}}"
        );
        let member = Layer::raster(
            "M",
            image(vec![promoted(HIGH); (W * H) as usize]),
            (0, 0),
            1.0,
            member_visible,
        )
        .unwrap()
        .with_clipped(true);
        document(vec![
            layer("BG", BG),
            styled(layer("Base", EXTRA), &json)
                .with_blend_mode(BlendMode::Multiply)
                .unwrap(),
            member,
        ])
    };
    assert_eq!(
        plate(&build(true, true), 1),
        render(&build(true, false)),
        "the plate draws the grouped base as the member-less document does"
    );
    assert_ne!(
        render(&build(false, false)),
        render(&build(true, false)),
        "non-vacuity: the flag changes the member-less picture"
    );
}

fn plate(doc: &PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe {
        dr_precision_doc_retouch_source(
            doc,
            index,
            1,
            false,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null(), "unexpected message");
    assert!(!img.is_null(), "the typed plate refused");
    unsafe { Box::from_raw(img) }.to_rgba16().unwrap()
}

// ------------------------------------------------ refusals and identity --

#[test]
fn a_float_document_holds_a_knockout_style_and_refuses_by_the_float_sentence() {
    let path = std::path::Path::new("tests/fixtures/float_11f/scene.tiff");
    assert!(path.exists(), "the committed float fixture is missing");
    let doc = PrecisionDocument::open_preserving(
        path.to_str().unwrap(),
        REFERENCE_WORKING_BYTES,
        0,
        None,
    )
    .expect("the float fixture opens");
    // A float document holds the knockout; what it still
    // cannot hold — an effect mode outside its float set — refuses by the
    // float sentence.
    assert!(edit(
        &doc,
        json!({"op":"layer_style","version":1,"layer":0,"style":{"knockout":"deep"}}),
    )
    .is_ok());
    let message = edit(
        &doc,
        json!({"op":"layer_style","version":1,"layer":0,"style":{"knockout":"deep",
               "effects":[{"type":"outer_glow","blend":"screen"}]}}),
    )
    .err()
    .expect("a float document must refuse");
    assert!(
        message.contains("32-bit float"),
        "the refusal names the depth: {message}"
    );
}

#[test]
fn a_linear_light_typed_document_refuses_a_knockout_style_by_the_evaluation_sentence() {
    let doc = document(vec![
        layer("BG", BG),
        styled(layer("Top", HIGH), &knockout_style("deep", 0.0)),
    ]);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        doctored_core::ffi_evaluation::dr_precision_doc_set_evaluation(&doc, 1, &mut err)
    };
    assert!(out.is_null(), "linear light accepted a knockout style");
    let message = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    assert!(
        message.contains("a layer style or Blend If"),
        "the evaluation policy's own sentence: {message}"
    );
}

#[test]
fn a_typed_document_with_neither_key_renders_identically() {
    // The identity D2/D9 rest on, one depth up: spelling the defaults out
    // changes nothing, and neither does the new compositor path existing.
    let plain = document(vec![
        layer("BG", BG),
        styled(layer("Top", HIGH), "{\"fill_opacity\":0.5}"),
    ]);
    let explicit = document(vec![
        layer("BG", BG),
        styled(
            layer("Top", HIGH),
            "{\"fill_opacity\":0.5,\"knockout\":\"none\",\"blend_interior_as_group\":false}",
        ),
    ]);
    assert_eq!(render(&plain), render(&explicit));
}

#[test]
fn the_render_plan_grows_by_the_scratch_the_design_charges() {
    // D8 at 16 bits. Two claims, each isolating one term:
    //
    // 1. D5's Background snapshot is `16·n`, charged ONCE and only when entry
    //    0 qualifies. The two documents below differ in exactly that clause
    //    (opacity 1 versus 0.5 on entry 0), so their plans differ by exactly
    //    one snapshot.
    // 2. A routed unit costs something a document without the keys never
    //    pays. That one is an INEQUALITY, because the plan of a document with
    //    neither key is bounded by its finalization rather than by the sum the
    //    knockout terms join.
    let n = u64::from(W) * u64::from(H);
    let with_bg = document(vec![
        layer("BG", BG),
        styled(layer("Top", HIGH), &knockout_style("deep", 0.5)),
    ]);
    let without_bg = document(vec![
        Layer::raster(
            "BG",
            image(vec![promoted(BG); (W * H) as usize]),
            (0, 0),
            0.5,
            true,
        )
        .unwrap(),
        styled(layer("Top", HIGH), &knockout_style("deep", 0.5)),
    ]);
    let plain = document(vec![
        layer("BG", BG),
        styled(layer("Top", HIGH), "{\"fill_opacity\":0.5}"),
    ]);
    let a = with_bg.render_plan().unwrap().scratch_bytes;
    let b = without_bg.render_plan().unwrap().scratch_bytes;
    let c = plain.render_plan().unwrap().scratch_bytes;
    assert_eq!(
        a - b,
        16 * n,
        "D5's Background snapshot is one 16-byte plane per canvas pixel"
    );
    assert!(
        b > c,
        "a routed unit is charged: {b} bytes against {c} without the keys"
    );
}
