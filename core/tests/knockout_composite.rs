//! KNOCKOUT (Shallow / Deep) and "Blend Interior Effects as Group" in the
//! BYTE compositor — Phase 12E-3, the design D1–D12 recorded in
//! `archive/phase-12e-layer-ops-status.md`. Everything black-box through
//! `dr_doc_set_layer_style` and `dr_doc_flattened`; the format's own coverage
//! (the JSON codec, `.rz` word 49, the typed and float refusals, the PSD loss)
//! is `knockout_format.rs`.
//!
//! # The tolerance, stated BEFORE anything was measured
//!
//! **T1 — the byte render against the oracle: 1 code of 255 per channel,
//! ALPHA INCLUDED.** The oracle below is written from D3–D6 in f64 — `over`
//! is the W3C source-over equation transcribed from the specification and
//! `mix` is the premultiplied lerp D3 names, neither of them calling into the
//! crate — with the separable blend functions coming from the independent
//! `tests/common/native_blend_oracle`. Every scene is hard-edged (canvas-sized
//! opaque fills) except the two that exist to probe a soft edge, so a single
//! code is the whole quantization budget and nothing hides in it.
//!
//! Two claims here are EQUALITIES rather than tolerances, because they are
//! identities and not approximations: a document carrying neither key
//! projects byte-for-byte what it projected before the keys existed, and a
//! knockout layer at `q = 1` under Normal equals the same layer with no
//! knockout at all (D3's "no fringe" statement) — the second within T1,
//! because the two take different arithmetic to the same number.

use std::ffi::c_char;
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_evaluation::dr_doc_set_evaluation;
use doctored_core::ffi_group::*;
use tempfile::TempDir;

use crate::common;
use common::*;

use crate::native_blend_oracle as oracle;
use crate::resource_report;

// -------------------------------------------------------------- the scene --

const W: u32 = 8;
const H: u32 = 8;
/// Every canvas-sized fixture is uniform, so ONE interior probe is the whole
/// picture; the two shaped fixtures name their own probes.
const PX: (u32, u32) = (4, 4);

const BG: [u8; 4] = [204, 34, 34, 255];
const MID: [u8; 4] = [34, 204, 34, 255];
const HIGH: [u8; 4] = [34, 34, 204, 255];
const EXTRA: [u8; 4] = [204, 204, 34, 255];

const NORMAL: i32 = 0;
const MULTIPLY: i32 = 1;
const PASS_THROUGH: i32 = 27;

/// A document whose entry 0 is a canvas-sized opaque `color` — D5's
/// qualifying Background, until a test disqualifies it.
fn scene(dir: &TempDir, color: [u8; 4]) -> *mut DrDocument {
    doc_from(dir, "bg.png", &solid(W, H, color))
}

/// One canvas-sized opaque layer above `idx`.
fn over_layer(
    dir: &TempDir,
    file: &str,
    doc: *mut DrDocument,
    idx: usize,
    color: [u8; 4],
    name: &str,
) -> *mut DrDocument {
    add_layer(dir, file, doc, idx, &solid(W, H, color), name)
}

/// `apply_or_keep` throughout: a fixture routinely hands a setter the value
/// the entry already holds (Normal on a fresh layer, Pass Through on a fresh
/// group), which the core answers NULL for by its purity rule.
fn set_blend(doc: *mut DrDocument, idx: usize, mode: i32) -> *mut DrDocument {
    apply_or_keep(doc, |d| unsafe {
        dr_doc_with_layer_blend_mode(d, idx, mode)
    })
}

fn set_clipped(doc: *mut DrDocument, idx: usize) -> *mut DrDocument {
    apply(doc, |d| unsafe { dr_doc_with_layer_clipped(d, idx, true) })
}

fn set_opacity(doc: *mut DrDocument, idx: usize, opacity: f32) -> *mut DrDocument {
    apply_or_keep(doc, |d| unsafe {
        dr_doc_with_layer_opacity(d, idx, opacity)
    })
}

fn set_visible(doc: *mut DrDocument, idx: usize, visible: bool) -> *mut DrDocument {
    apply_or_keep(doc, |d| unsafe {
        dr_doc_with_layer_visible(d, idx, visible)
    })
}

/// `dr_doc_group_layers` on one entry, answering the group's index.
fn group_one(doc: *mut DrDocument, idx: usize, name: &str) -> (*mut DrDocument, usize) {
    group_many(doc, &[idx], name)
}

fn group_many(doc: *mut DrDocument, idx: &[usize], name: &str) -> (*mut DrDocument, usize) {
    let cname = std::ffi::CString::new(name).expect("no interior NUL");
    let cap = unsafe { dr_doc_layer_count(doc) } + 1;
    let mut group = usize::MAX;
    let mut cleared = vec![0usize; cap];
    let mut cleared_len = 0usize;
    let mut reordered = vec![0usize; cap];
    let mut reordered_len = 0usize;
    let out = unsafe {
        dr_doc_group_layers(
            doc,
            idx.as_ptr(),
            idx.len(),
            cname.as_ptr(),
            &mut group,
            cleared.as_mut_ptr(),
            &mut cleared_len,
            reordered.as_mut_ptr(),
            &mut reordered_len,
            cap,
        )
    };
    assert!(!out.is_null(), "group_layers({idx:?}) refused");
    unsafe { dr_doc_free(doc) };
    (out, group)
}

/// The style JSON for a knockout at a given fill; `knockout` is `"none"`,
/// `"shallow"` or `"deep"`.
fn knockout_style(knockout: &str, fill: f32) -> String {
    format!("{{\"knockout\":\"{knockout}\",\"fill_opacity\":{fill}}}")
}

fn probe(doc: *const DrDocument, x: u32, y: u32) -> [u8; 4] {
    pixel(&flat_pixels(doc), W, x, y)
}

// ------------------------------------------------------------- the oracle --

/// D3's operands as f64.
fn u(px: [u8; 4]) -> [f64; 4] {
    [
        f64::from(px[0]) / 255.0,
        f64::from(px[1]) / 255.0,
        f64::from(px[2]) / 255.0,
        f64::from(px[3]) / 255.0,
    ]
}

const CLEAR: [f64; 4] = [0.0; 4];

/// `over(B, C, a, M)`: the W3C source-over of straight colour `cs` at alpha
/// `sa` onto straight `b` with blend `M`, transcribed from the specification
/// (`co = as·((1 − ab)·Cs + ab·B(Cb, Cs)) + (1 − as)·ab·Cb`, `ao = as + ab·(1 − as)`)
/// and divided back out to straight colour. `B` comes from the independent
/// `native_blend_oracle::separable`.
fn over(b: [f64; 4], cs: [f64; 3], sa: f64, mode: i32) -> [f64; 4] {
    let ab = b[3];
    let ao = sa + ab * (1.0 - sa);
    if ao <= 0.0 {
        return CLEAR;
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        let blended = oracle::separable(mode, b[c], cs[c]);
        let co = sa * ((1.0 - ab) * cs[c] + ab * blended) + (1.0 - sa) * ab * b[c];
        out[c] = (co / ao).clamp(0.0, 1.0);
    }
    out[3] = ao.min(1.0);
    out
}

/// `mix(A, K, t)`: D3's premultiplied lerp — each colour weighted by its own
/// alpha, lerped, and divided back out.
fn mix(a: [f64; 4], k: [f64; 4], t: f64) -> [f64; 4] {
    let alpha = a[3] + (k[3] - a[3]) * t;
    if alpha <= 0.0 {
        return CLEAR;
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        let pa = a[c] * a[3];
        let pk = k[c] * k[3];
        out[c] = ((pa + (pk - pa) * t) / alpha).clamp(0.0, 1.0);
    }
    out[3] = alpha.min(1.0);
    out
}

/// D3 for one pixel of a plain knockout layer: `K = over(B0, C, q, M)` then
/// `out = mix(acc, K, s·opacity)`.
fn unit_pixel(
    acc: [f64; 4],
    b0: [f64; 4],
    colour: [f64; 3],
    q: f64,
    mode: i32,
    s: f64,
    opacity: f64,
) -> [f64; 4] {
    let k = if q > 0.0 {
        over(b0, colour, q, mode)
    } else {
        b0
    };
    mix(acc, k, s * opacity)
}

/// T1: one code of 255 per channel, alpha included.
fn assert_t1(got: [u8; 4], want: [f64; 4], what: &str) {
    let want = [
        (want[0] * 255.0).round(),
        (want[1] * 255.0).round(),
        (want[2] * 255.0).round(),
        (want[3] * 255.0).round(),
    ];
    for c in 0..4 {
        let delta = (f64::from(got[c]) - want[c]).abs();
        assert!(
            delta <= 1.0,
            "{what}: channel {c} is {} , the oracle says {} (T1 = 1 code)",
            got[c],
            want[c]
        );
    }
}

// --------------------------------------------------- D5: the root backdrop --

#[test]
fn root_shallow_over_a_qualifying_background_punches_to_it_at_every_fill() {
    let dir = TempDir::new().unwrap();
    // Shallow at the ROOT is Deep (D4), so the same fixture proves both.
    for knockout in ["shallow", "deep"] {
        for (fill, mode) in [(0.0f32, NORMAL), (0.5, MULTIPLY), (1.0, NORMAL)] {
            let mut doc = scene(&dir, BG);
            doc = over_layer(&dir, "mid.png", doc, 0, MID, "Mid");
            doc = over_layer(&dir, "top.png", doc, 1, HIGH, "Top");
            doc = set_blend(doc, 2, mode);
            doc = set_style(doc, 2, &knockout_style(knockout, fill));
            let got = probe(doc, PX.0, PX.1);
            // The accumulator beneath the knockout layer is MID over BG; the
            // knockout backdrop is the Background alone.
            let acc = over(u(BG), [u(MID)[0], u(MID)[1], u(MID)[2]], 1.0, NORMAL);
            let want = unit_pixel(
                acc,
                u(BG),
                [u(HIGH)[0], u(HIGH)[1], u(HIGH)[2]],
                f64::from(fill),
                mode,
                1.0,
                1.0,
            );
            assert_t1(got, want, &format!("{knockout} at fill {fill} mode {mode}"));
            unsafe { dr_doc_free(doc) };
        }
    }
}

#[test]
fn every_d5_disqualifier_leaves_the_root_backdrop_transparent() {
    let dir = TempDir::new().unwrap();
    // At fill 0 the whole pixel IS the backdrop, so a disqualified Background
    // shows as a transparent hole — the one unambiguous answer.
    let build = |dir: &TempDir| {
        let mut doc = scene(dir, BG);
        doc = over_layer(dir, "mid.png", doc, 0, MID, "Mid");
        doc = over_layer(dir, "top.png", doc, 1, HIGH, "Top");
        doc = set_style(doc, 2, &knockout_style("deep", 0.0));
        doc
    };
    // The control: entry 0 qualifies, so the hole shows it.
    let control = build(&dir);
    assert_eq!(probe(control, PX.0, PX.1), BG, "the qualifying Background");
    unsafe { dr_doc_free(control) };

    /// One way to disqualify entry 0, named so a failure says which.
    type Disqualifier = Box<dyn Fn(*mut DrDocument) -> *mut DrDocument>;
    let mut cases: Vec<(&str, Disqualifier)> = Vec::new();
    cases.push(("hidden", Box::new(|d| set_visible(d, 0, false))));
    cases.push(("opacity < 1", Box::new(|d| set_opacity(d, 0, 0.5))));
    cases.push(("a blend mode", Box::new(|d| set_blend(d, 0, MULTIPLY))));
    cases.push((
        "an offset",
        Box::new(|d| apply(d, |x| unsafe { dr_doc_with_layer_offset(x, 0, 1, 0) })),
    ));
    cases.push((
        "a style",
        Box::new(|d| set_style(d, 0, "{\"fill_opacity\":0.5}")),
    ));
    cases.push((
        "an enabled mask",
        Box::new(|d| {
            apply(d, |x| unsafe {
                dr_doc_adding_layer_mask(x, 0, MASK_REVEAL_ALL, ptr::null(), 0, 0)
            })
        }),
    ));
    cases.push(("a clipped entry 1", Box::new(|d| set_clipped(d, 1))));
    for (what, disqualify) in cases {
        let doc = disqualify(build(&dir));
        let got = probe(doc, PX.0, PX.1);
        assert_eq!(got[3], 0, "{what} disqualifies the Background: {got:?}");
        unsafe { dr_doc_free(doc) };
    }

    // "smaller than the canvas": entry 0 keeps its pixels while the canvas
    // grows around it.
    let doc = apply(build(&dir), |d| unsafe {
        dr_doc_canvas_resize(d, W + 2, H + 2, 0, 0)
    });
    let flat = flat_pixels(doc);
    let got = pixel(&flat, W + 2, PX.0, PX.1);
    assert_eq!(got[3], 0, "a Background smaller than the canvas: {got:?}");
    unsafe { dr_doc_free(doc) };
}

// ---------------------------------------------- D4: groups and boundaries --

/// `[BG, MID, GROUP(mode){ INNER, TOP(knockout, fill 0) }]` — the fixture
/// every boundary case shares. The group's mode decides pass-through vs
/// isolated; `gate` sets its opacity.
fn group_fixture(
    dir: &TempDir,
    knockout: &str,
    group_mode: i32,
    gate: Option<f32>,
) -> *mut DrDocument {
    let mut doc = scene(dir, BG);
    doc = over_layer(dir, "mid.png", doc, 0, MID, "Mid");
    doc = over_layer(dir, "inner.png", doc, 1, EXTRA, "Inner");
    doc = over_layer(dir, "top.png", doc, 2, HIGH, "Top");
    doc = set_style(doc, 3, &knockout_style(knockout, 0.0));
    let (mut doc, group) = group_many(doc, &[2, 3], "G");
    doc = set_blend(doc, group, group_mode);
    if let Some(opacity) = gate {
        doc = set_opacity(doc, group, opacity);
    }
    doc
}

#[test]
fn a_pass_through_group_gives_shallow_its_own_entry_state_and_deep_the_background() {
    let dir = TempDir::new().unwrap();
    // Shallow: the accumulator the group's children began with — MID over
    // BG, i.e. MID — so the hole erases `Inner` and stops at the group.
    let doc = group_fixture(&dir, "shallow", PASS_THROUGH, None);
    assert_eq!(probe(doc, PX.0, PX.1), MID, "Shallow stops at the group");
    unsafe { dr_doc_free(doc) };
    // Deep walks straight through the pass-through level to the Background.
    let doc = group_fixture(&dir, "deep", PASS_THROUGH, None);
    assert_eq!(probe(doc, PX.0, PX.1), BG, "Deep reaches the Background");
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_gated_pass_through_group_reads_the_untouched_accumulator_and_still_carries_deep() {
    let dir = TempDir::new().unwrap();
    // The children composite into a COPY, so `acc` still holds what they
    // began with: Shallow punches to MID and the gate then lerps MID onto
    // MID, which is MID exactly.
    let doc = group_fixture(&dir, "shallow", PASS_THROUGH, Some(0.5));
    assert_eq!(probe(doc, PX.0, PX.1), MID, "gated Shallow");
    unsafe { dr_doc_free(doc) };
    // Deep punches to the Background inside the copy; the gate then mixes
    // that hole back over the untouched accumulator at 0.5.
    let doc = group_fixture(&dir, "deep", PASS_THROUGH, Some(0.5));
    let want = mix(u(MID), u(BG), 0.5);
    assert_t1(probe(doc, PX.0, PX.1), want, "gated Deep");
    unsafe { dr_doc_free(doc) };
    // A MASKED pass-through group takes the same path for the same reason.
    let mut doc = group_fixture(&dir, "deep", PASS_THROUGH, None);
    let count = unsafe { dr_doc_layer_count(doc) };
    let mask: Vec<u8> = vec![255; (W * H) as usize];
    doc = apply(doc, |d| unsafe {
        dr_doc_adding_layer_mask(d, count - 1, MASK_FROM_SELECTION, mask.as_ptr(), W, H)
    });
    assert_eq!(
        probe(doc, PX.0, PX.1),
        BG,
        "an all-white mask gates without changing the answer"
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn an_isolated_group_is_a_hard_stop_for_both_modes() {
    let dir = TempDir::new().unwrap();
    for knockout in ["shallow", "deep"] {
        // A Normal group isolates; its buffer starts transparent, so the hole
        // reaches transparency and the group contributes nothing there —
        // what shows is the layer beneath the GROUP.
        let doc = group_fixture(&dir, knockout, NORMAL, None);
        assert_eq!(
            probe(doc, PX.0, PX.1),
            MID,
            "{knockout} stops at the isolated group's buffer"
        );
        unsafe { dr_doc_free(doc) };
    }
}

#[test]
fn a_pass_through_group_nested_in_an_isolated_one_keeps_both_boundaries() {
    let dir = TempDir::new().unwrap();
    // [BG, MID, ISO(Normal){ Inner, PASS(pass-through){ Top(knockout, fill 0) } }]
    let build = |knockout: &str| {
        let mut doc = scene(&dir, BG);
        doc = over_layer(&dir, "mid.png", doc, 0, MID, "Mid");
        doc = over_layer(&dir, "inner.png", doc, 1, EXTRA, "Inner");
        doc = over_layer(&dir, "top.png", doc, 2, HIGH, "Top");
        doc = set_style(doc, 3, &knockout_style(knockout, 0.0));
        let (doc, inner_group) = group_one(doc, 3, "Pass");
        let doc = set_blend(doc, inner_group, PASS_THROUGH);
        let (doc, outer) = group_many(doc, &[2, inner_group], "Iso");
        set_blend(doc, outer, NORMAL)
    };
    // Shallow reads the accumulator the pass-through level began with, which
    // INSIDE the isolated buffer is `Inner` alone.
    let doc = build("shallow");
    assert_eq!(
        probe(doc, PX.0, PX.1),
        EXTRA,
        "Shallow inside the isolation"
    );
    unsafe { dr_doc_free(doc) };
    // Deep walks through the pass-through level and meets the isolated
    // buffer's hard stop: transparent, so the whole group drops out.
    let doc = build("deep");
    assert_eq!(probe(doc, PX.0, PX.1), MID, "Deep inside the isolation");
    unsafe { dr_doc_free(doc) };
}

// --------------------------------------------------- D4: clipped members --

/// `[BG, MID, Base(style?), M1(clipped), M2(clipped, knockout, fill 0)]`.
fn clip_fixture(dir: &TempDir, knockout: &str, base_style: Option<&str>) -> *mut DrDocument {
    let mut doc = scene(dir, BG);
    doc = over_layer(dir, "mid.png", doc, 0, MID, "Mid");
    doc = over_layer(dir, "base.png", doc, 1, EXTRA, "Base");
    doc = over_layer(dir, "m1.png", doc, 2, HIGH, "M1");
    doc = set_clipped(doc, 3);
    doc = over_layer(dir, "m2.png", doc, 3, MID, "M2");
    doc = set_clipped(doc, 4);
    doc = set_style(doc, 4, &knockout_style(knockout, 0.0));
    if let Some(style) = base_style {
        doc = set_style(doc, 2, style);
    }
    doc
}

/// A drop shadow whose shift lands entirely under a canvas-sized opaque
/// layer: `layer_knocks_out` (the default) then zeroes it inside the shape,
/// so it never reaches the probe and the base is still a STYLED base.
const BASE_SHADOW: &str =
    "{\"effects\":[{\"type\":\"drop_shadow\",\"distance\":2,\"size\":0,\"spread\":0}]}";

#[test]
fn a_clipped_member_punches_to_its_base_alone_over_an_unstyled_base() {
    let dir = TempDir::new().unwrap();
    for knockout in ["shallow", "deep"] {
        let doc = clip_fixture(&dir, knockout, None);
        assert_eq!(
            probe(doc, PX.0, PX.1),
            EXTRA,
            "{knockout} member reveals the BASE, through the lower member"
        );
        unsafe { dr_doc_free(doc) };
    }
}

#[test]
fn a_clipped_member_punches_to_its_base_alone_over_a_styled_base_at_both_opacities() {
    let dir = TempDir::new().unwrap();
    for knockout in ["shallow", "deep"] {
        // Opacity 1: the DIRECT styled path.
        let doc = clip_fixture(&dir, knockout, Some(BASE_SHADOW));
        assert_eq!(
            probe(doc, PX.0, PX.1),
            EXTRA,
            "{knockout} over a styled base at opacity 1 — the BASE, never the Background"
        );
        unsafe { dr_doc_free(doc) };
        // Opacity 0.5: the PACKAGE-STRIP path, whose backdrop windows must
        // line up with the strip.
        let doc = set_opacity(clip_fixture(&dir, knockout, Some(BASE_SHADOW)), 2, 0.5);
        let want = mix(u(MID), u(EXTRA), 0.5);
        assert_t1(
            probe(doc, PX.0, PX.1),
            want,
            "over a styled base at opacity 0.5 (the package strip)",
        );
        unsafe { dr_doc_free(doc) };
    }
}

#[test]
fn a_knockout_base_evaluates_its_members_against_its_own_base_only_state() {
    let dir = TempDir::new().unwrap();
    // The BASE itself knocks out (Deep, fill 1, so its own unit is its
    // pixels), and its upper member knocks out too (Shallow, fill 0): the
    // member must reveal the base's own pixels, NOT the outer backdrop, and
    // the finished unit then meets the outer `B0`.
    let mut doc = scene(&dir, BG);
    doc = over_layer(&dir, "mid.png", doc, 0, MID, "Mid");
    doc = over_layer(&dir, "base.png", doc, 1, EXTRA, "Base");
    doc = set_style(doc, 2, &knockout_style("deep", 1.0));
    doc = over_layer(&dir, "m1.png", doc, 2, HIGH, "M1");
    doc = set_clipped(doc, 3);
    doc = over_layer(&dir, "m2.png", doc, 3, MID, "M2");
    doc = set_clipped(doc, 4);
    doc = set_style(doc, 4, &knockout_style("shallow", 0.0));
    assert_eq!(
        probe(doc, PX.0, PX.1),
        EXTRA,
        "the member reveals the unit's base-only state"
    );
    unsafe { dr_doc_free(doc) };

    // With the BASE at fill 0 its base-only state is EMPTY, so the member's
    // own hole empties the unit buffer the lower member had filled — and the
    // finished unit, now a hole itself, meets the outer `B0`.
    let mut doc = scene(&dir, BG);
    doc = over_layer(&dir, "mid.png", doc, 0, MID, "Mid");
    doc = over_layer(&dir, "base.png", doc, 1, EXTRA, "Base");
    doc = set_style(doc, 2, &knockout_style("deep", 0.0));
    doc = over_layer(&dir, "m1.png", doc, 2, HIGH, "M1");
    doc = set_clipped(doc, 3);
    doc = over_layer(&dir, "m2.png", doc, 3, MID, "M2");
    doc = set_clipped(doc, 4);
    doc = set_style(doc, 4, &knockout_style("shallow", 0.0));
    assert_eq!(
        probe(doc, PX.0, PX.1),
        BG,
        "the unit then meets the outer backdrop"
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_knockout_group_is_one_unit() {
    let dir = TempDir::new().unwrap();
    // A style isolates a group, so a knockout on the GROUP punches once with
    // the group's own projection as its shape.
    let mut doc = scene(&dir, BG);
    doc = over_layer(&dir, "mid.png", doc, 0, MID, "Mid");
    doc = over_layer(&dir, "inner.png", doc, 1, EXTRA, "Inner");
    let (mut doc, group) = group_one(doc, 2, "G");
    doc = set_style(doc, group, &knockout_style("deep", 0.0));
    assert_eq!(probe(doc, PX.0, PX.1), BG, "a knockout GROUP");
    unsafe { dr_doc_free(doc) };
}

// ---------------------------------------- D3: soft edges, Blend If, Below --

#[test]
fn a_partial_alpha_shape_gets_no_fringe_at_q_one() {
    let dir = TempDir::new().unwrap();
    // D3: with `q = 1` under Normal the unit equals the plain render for ANY
    // backdrop, so a half-transparent layer keeps exactly the edge it had.
    let half = solid(W, H, [34, 34, 204, 128]);
    let plain = {
        let mut doc = scene(&dir, BG);
        doc = over_layer(&dir, "mid.png", doc, 0, MID, "Mid");
        add_layer(&dir, "half.png", doc, 1, &half, "Half")
    };
    let plain_pixels = flat_pixels(plain);
    unsafe { dr_doc_free(plain) };
    let knocked = {
        let mut doc = scene(&dir, BG);
        doc = over_layer(&dir, "mid.png", doc, 0, MID, "Mid");
        doc = add_layer(&dir, "half.png", doc, 1, &half, "Half");
        set_style(doc, 2, &knockout_style("deep", 1.0))
    };
    let knocked_pixels = flat_pixels(knocked);
    unsafe { dr_doc_free(knocked) };
    assert_close(
        &knocked_pixels,
        &plain_pixels,
        "a Normal knockout at fill 1 is the plain render",
    );
    // And the oracle agrees with both, so the equality is not two wrongs.
    let acc = over(u(BG), [u(MID)[0], u(MID)[1], u(MID)[2]], 1.0, NORMAL);
    let want = unit_pixel(
        acc,
        u(BG),
        [34.0 / 255.0, 34.0 / 255.0, 204.0 / 255.0],
        1.0,
        NORMAL,
        128.0 / 255.0,
        1.0,
    );
    assert_t1(
        pixel(&knocked_pixels, W, PX.0, PX.1),
        want,
        "the partial-alpha unit",
    );
}

#[test]
fn blend_if_is_opacity_and_never_shape_and_its_underlying_is_the_accumulated_composite() {
    let dir = TempDir::new().unwrap();
    // `this_layer` hides the layer's own dark pixels entirely: the weight is
    // 0, so `q` is 0 — and D3 says a hidden pixel STILL punches.
    let hidden = "{\"knockout\":\"deep\",\"blend_if\":{\"channel\":\"gray\",\
                  \"this_layer\":[200,200,255,255]}}";
    let mut doc = scene(&dir, BG);
    doc = over_layer(&dir, "mid.png", doc, 0, MID, "Mid");
    doc = over_layer(&dir, "top.png", doc, 1, [8, 8, 8, 255], "Top");
    doc = set_style(doc, 2, hidden);
    assert_eq!(
        probe(doc, PX.0, PX.1),
        BG,
        "a pixel Blend If hides still punches"
    );
    unsafe { dr_doc_free(doc) };

    // `underlying` is keyed so that BLACK passes and WHITE does not. The
    // layer beneath is black and the knockout backdrop is white: the layer
    // paints, which can only happen if Underlying read the ACCUMULATED
    // composite rather than the backdrop.
    let ranged = "{\"knockout\":\"deep\",\"blend_if\":{\"channel\":\"gray\",\
                  \"underlying\":[0,0,0,0]}}";
    let mut doc = scene(&dir, WHITE);
    doc = over_layer(&dir, "black.png", doc, 0, [0, 0, 0, 255], "Black");
    doc = over_layer(&dir, "top.png", doc, 1, HIGH, "Top");
    doc = set_style(doc, 2, ranged);
    assert_eq!(
        probe(doc, PX.0, PX.1),
        HIGH,
        "Underlying is the composite beneath, not the knockout backdrop"
    );
    unsafe { dr_doc_free(doc) };
}

#[test]
fn below_effects_and_the_opacity_package_are_untouched_by_the_knockout() {
    let dir = TempDir::new().unwrap();
    // A 4x4 layer at (2, 2) with a hard drop shadow, at layer opacity 0.5 so
    // the PACKAGE path runs. The shadow lands outside the shape, where the
    // knockout cannot reach.
    let build = |knockout: &str| {
        let mut doc = scene(&dir, BG);
        doc = over_layer(&dir, "mid.png", doc, 0, MID, "Mid");
        doc = add_layer(&dir, "rect.png", doc, 1, &solid(4, 4, HIGH), "Rect");
        doc = apply_or_keep(doc, |d| unsafe { dr_doc_with_layer_offset(d, 2, 2, 2) });
        doc = set_opacity(doc, 2, 0.5);
        set_style(
            doc,
            2,
            &format!(
                "{{\"knockout\":\"{knockout}\",\"fill_opacity\":0.0,\
                 \"effects\":[{{\"type\":\"drop_shadow\",\"distance\":2,\"size\":0,\
                 \"spread\":0}}]}}"
            ),
        )
    };
    let none = build("none");
    let deep = build("deep");
    let (a, b) = (flat_pixels(none), flat_pixels(deep));
    // The shadow's own pixels: identical, because Below never sees a knockout.
    for (x, y) in [(3u32, 6u32), (4, 6), (5, 6), (6, 5)] {
        assert_eq!(
            pixel(&a, W, x, y),
            pixel(&b, W, x, y),
            "the Below group at ({x}, {y}) is untouched by the knockout"
        );
    }
    // Inside the shape, the knockout has punched — otherwise the test proves
    // nothing about the pixel pass.
    assert_ne!(
        pixel(&a, W, 3, 3),
        pixel(&b, W, 3, 3),
        "the interior DID change"
    );
    // The package model: the strip runs at opacity 1 and mixes back at 0.5,
    // so the hole is half applied.
    let acc = over(u(BG), [u(MID)[0], u(MID)[1], u(MID)[2]], 1.0, NORMAL);
    let want = mix(acc, u(BG), 0.5);
    assert_t1(pixel(&b, W, 3, 3), want, "the knockout inside the package");
    unsafe { dr_doc_free(none) };
    unsafe { dr_doc_free(deep) };
}

// ------------------------------------------- D6: blend interior as group --

/// A grey canvas under a WHITE Multiply layer carrying a GREEN colour
/// overlay — the shape D6's claim is sharpest on.
fn interior_fixture(dir: &TempDir, grouped: bool, fill: f32) -> *mut DrDocument {
    let mut doc = scene(dir, [128, 128, 128, 255]);
    doc = over_layer(dir, "top.png", doc, 0, WHITE, "Top");
    doc = set_blend(doc, 1, MULTIPLY);
    let style = format!(
        "{{\"blend_interior_as_group\":{grouped},\"fill_opacity\":{fill},\
         \"effects\":[{{\"type\":\"color_overlay\",\"color\":\"#00ff00\",\"opacity\":1.0}}]}}"
    );
    set_style(doc, 1, &style)
}

#[test]
fn blend_interior_as_group_moves_the_four_grouped_contributions_into_the_unit() {
    let dir = TempDir::new().unwrap();
    let grey = u([128, 128, 128, 255]);
    let green = [0.0, 1.0, 0.0];

    // OFF (the default): the overlay composites onto the accumulator
    // with its OWN Normal mode, so the layer's Multiply never touches it.
    let doc = interior_fixture(&dir, false, 1.0);
    let got = probe(doc, PX.0, PX.1);
    assert_eq!(got, [0, 255, 0, 255], "off: the overlay shows as itself");
    unsafe { dr_doc_free(doc) };

    // ON at fill 1: the overlay recolours the unit, which then meets the
    // accumulator ONCE with the LAYER's Multiply at q = fill.
    for fill in [1.0f32, 0.5] {
        let doc = interior_fixture(&dir, true, fill);
        let got = probe(doc, PX.0, PX.1);
        let want = unit_pixel(grey, grey, green, f64::from(fill), MULTIPLY, 1.0, 1.0);
        assert_t1(got, want, &format!("grouped interior at fill {fill}"));
        unsafe { dr_doc_free(doc) };
    }
}

#[test]
fn an_inner_shadow_stays_outside_the_grouped_unit() {
    let dir = TempDir::new().unwrap();
    // With the flag ON the inner shadow must STILL be drawn, and still AFTER
    // the unit: the far interior matches the no-shadow scene exactly while
    // the edge the shadow reaches does not.
    let build = |shadow: bool| {
        let mut doc = scene(&dir, [128, 128, 128, 255]);
        doc = over_layer(&dir, "top.png", doc, 0, WHITE, "Top");
        doc = set_blend(doc, 1, MULTIPLY);
        let extra = if shadow {
            ",{\"type\":\"inner_shadow\",\"distance\":3,\"size\":0,\"choke\":0}"
        } else {
            ""
        };
        set_style(
            doc,
            1,
            &format!(
                "{{\"blend_interior_as_group\":true,\"effects\":[\
                 {{\"type\":\"color_overlay\",\"color\":\"#00ff00\",\"opacity\":1.0}}{extra}]}}"
            ),
        )
    };
    let plain = build(false);
    let shadowed = build(true);
    let (a, b) = (flat_pixels(plain), flat_pixels(shadowed));
    // The default 120° light at distance 3 shifts the inverted shape by
    // (2, 3), so the shadow darkens x < 2 or y < 3 and nothing else.
    assert_eq!(
        pixel(&a, W, 7, 7),
        pixel(&b, W, 7, 7),
        "the inner shadow does not reach the far interior"
    );
    assert_ne!(
        pixel(&a, W, 0, 0),
        pixel(&b, W, 0, 0),
        "the inner shadow is still drawn, outside the grouped unit"
    );
    unsafe { dr_doc_free(plain) };
    unsafe { dr_doc_free(shadowed) };
}

/// P7-39: on a clip base with visible members the flag evaluates ON. The
/// unit is the base's pixels at fill 1, then the member (clamped to the
/// footprint), then the grouped overlay (the unit's alpha restored), and it
/// meets the backdrop ONCE with the base's Multiply at `q = fill`. The
/// member covers the left half only, so both halves of the footprint are
/// probed; the overlay at half opacity lets the member show through.
#[test]
fn the_grouped_flag_evaluates_on_on_a_clip_base_with_visible_members() {
    let dir = TempDir::new().unwrap();
    let grey = u([128, 128, 128, 255]);
    let red = [1.0, 0.0, 0.0];
    let white = [1.0, 1.0, 1.0];
    let green = [0.0, 1.0, 0.0];
    let build = |grouped: bool, fill: f32| {
        let mut doc = scene(&dir, [128, 128, 128, 255]);
        doc = over_layer(&dir, "base.png", doc, 0, WHITE, "Base");
        doc = set_blend(doc, 1, MULTIPLY);
        let half = image::RgbaImage::from_fn(W, H, |x, _| {
            image::Rgba(if x < W / 2 { [255, 0, 0, 255] } else { [0; 4] })
        });
        doc = add_layer(&dir, "m.png", doc, 1, &half, "M");
        doc = set_clipped(doc, 2);
        set_style(
            doc,
            1,
            &format!(
                "{{\"blend_interior_as_group\":{grouped},\"fill_opacity\":{fill},\"effects\":[\
                 {{\"type\":\"color_overlay\",\"color\":\"#00ff00\",\"opacity\":0.5}}]}}"
            ),
        )
    };
    // U after the member, then the overlay at 0.5 over it (Normal).
    let unit = |under: [f64; 3]| -> [f64; 3] {
        let u = over([under[0], under[1], under[2], 1.0], green, 0.5, NORMAL);
        [u[0], u[1], u[2]]
    };
    for fill in [1.0f32, 0.5] {
        let doc = build(true, fill);
        let q = f64::from(fill);
        let inside = unit_pixel(grey, grey, unit(red), q, MULTIPLY, 1.0, 1.0);
        let outside = unit_pixel(grey, grey, unit(white), q, MULTIPLY, 1.0, 1.0);
        assert_t1(
            probe(doc, 1, 4),
            inside,
            &format!("member half at fill {fill}"),
        );
        assert_t1(
            probe(doc, 6, 4),
            outside,
            &format!("bare half at fill {fill}"),
        );
        unsafe { dr_doc_free(doc) };
    }
    // Non-vacuity: OFF differs.
    let off = build(false, 1.0);
    let on = build(true, 1.0);
    assert_ne!(
        flat_pixels(off),
        flat_pixels(on),
        "the flag changes the picture"
    );
    unsafe { dr_doc_free(off) };
    unsafe { dr_doc_free(on) };
}

/// A cut is not a structure change, and since P7-39 the flag no longer asks
/// about members at all: the CURRENT-AND-BELOW plate of the base (which cuts
/// its one clipped member) draws the grouped base exactly as the document
/// with that member HIDDEN does.
#[test]
fn the_grouped_flag_is_on_under_a_cut_at_8_bits() {
    let dir = TempDir::new().unwrap();
    let build = |grouped: bool| {
        let mut doc = scene(&dir, BG);
        doc = over_layer(&dir, "base.png", doc, 0, WHITE, "Base");
        doc = set_blend(doc, 1, MULTIPLY);
        doc = over_layer(&dir, "m.png", doc, 1, [255, 0, 0, 255], "M");
        doc = set_clipped(doc, 2);
        set_style(
            doc,
            1,
            &format!(
                "{{\"blend_interior_as_group\":{grouped},\"effects\":[\
                 {{\"type\":\"color_overlay\",\"color\":\"#00ff00\",\"opacity\":1.0}}]}}"
            ),
        )
    };
    let on = build(true);
    let hidden = set_visible(build(true), 2, false);
    assert_eq!(
        cut_plate(on, 1),
        flat_pixels(hidden),
        "the plate draws the grouped base as the member-less document does"
    );
    let off = set_visible(build(false), 2, false);
    assert_ne!(
        flat_pixels(off),
        flat_pixels(hidden),
        "non-vacuity: the flag changes the member-less picture"
    );
    unsafe { dr_doc_free(on) };
    unsafe { dr_doc_free(hidden) };
    unsafe { dr_doc_free(off) };
}

/// The CURRENT-AND-BELOW sampling plate for `idx`, as bytes.
fn cut_plate(doc: *const DrDocument, idx: usize) -> Vec<u8> {
    let mut err: *mut c_char = ptr::null_mut();
    let img = unsafe {
        doctored_core::ffi_heal::dr_doc_retouch_source(doc, idx, 1, false, ptr::null(), &mut err)
    };
    assert!(err.is_null(), "unexpected message");
    assert!(!img.is_null(), "the plate refused");
    let out = img_pixels(img);
    unsafe { doctored_core::ffi::dr_image_free(img) };
    out
}

// ------------------------------------------------------------ D7: merges --

/// `[BG, MidA, MidB, Top(knockout deep, fill 0)]`, whose projection is the
/// Background: the scene D7's disclosure is about, because something sits
/// between the merged pair and the backdrop.
fn merge_fixture(dir: &TempDir) -> *mut DrDocument {
    let mut doc = scene(dir, BG);
    doc = over_layer(dir, "a.png", doc, 0, MID, "MidA");
    doc = over_layer(dir, "b.png", doc, 1, EXTRA, "MidB");
    doc = over_layer(dir, "top.png", doc, 2, HIGH, "Top");
    set_style(doc, 3, &knockout_style("deep", 0.0))
}

#[test]
fn flatten_stamp_and_merge_visible_bake_knockout_exactly() {
    let dir = TempDir::new().unwrap();
    let before = merge_fixture(&dir);
    assert_eq!(
        probe(before, PX.0, PX.1),
        BG,
        "the projection punches to BG"
    );
    unsafe { dr_doc_free(before) };

    // Merge Down and Merge Layers over the same scene are 12E-3b's EXACT
    // merges and live in `knockout_merge.rs`, which pins them against the
    // projection at both depths. This test keeps the three ops that were
    // exact from the start.

    // MERGE VISIBLE bakes EXACTLY, which is D7's rule for it:
    // `doc_structure::merge_visible` bakes `self.flattened()` — the
    // document's OWN projection, the one accumulator that carries the
    // Background snapshot — not a private transparent one like the other two
    // merges. So at 8 bits it behaves as Flatten and Stamp Visible do, and
    // neither its reply nor its catalog entry carries the narrowing note
    // (the drive's G8 asserts that absence). 12E-3b makes Merge Down and
    // Merge Layers exact too and replaces the blanket note with the RESIDUAL
    // one (decision xi).
    let doc = apply(merge_fixture(&dir), |d| unsafe { dr_doc_merge_visible(d) });
    assert_eq!(
        probe(doc, PX.0, PX.1),
        BG,
        "merge_visible renders the projection, so knockout bakes exactly"
    );
    unsafe { dr_doc_free(doc) };

    // Flatten and Stamp Visible render the PROJECTION, so both bake exactly.
    let doc = apply(merge_fixture(&dir), |d| unsafe { dr_doc_flattening(d) });
    assert_eq!(probe(doc, PX.0, PX.1), BG, "flatten bakes knockout exactly");
    unsafe { dr_doc_free(doc) };
    let name = std::ffi::CString::new("Stamp").unwrap();
    let doc = apply(merge_fixture(&dir), |d| unsafe {
        dr_doc_stamp_visible(d, 3, name.as_ptr())
    });
    assert_eq!(probe(doc, PX.0, PX.1), BG, "stamp bakes knockout exactly");
    unsafe { dr_doc_free(doc) };
}

// --------------------------------------------------- policy, cost, identity --

#[test]
fn a_linear_light_document_evaluates_a_knockout_style() {
    let dir = TempDir::new().unwrap();
    let mut doc = scene(&dir, BG);
    doc = over_layer(&dir, "top.png", doc, 0, HIGH, "Top");
    doc = set_style(doc, 1, &knockout_style("deep", 0.0));
    let encoded = flat_pixels(doc);
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_set_evaluation(doc, 1, &mut err) };
    assert!(
        !out.is_null() && err.is_null(),
        "linear light admits a knockout style"
    );
    // A deep knockout at fill 0 punches the layer through to its backdrop,
    // whose lattice codes round-trip exactly under either policy.
    assert_eq!(flat_pixels(out), encoded);
    unsafe {
        dr_doc_free(out);
        dr_doc_free(doc);
    }
}

#[test]
fn a_document_with_neither_key_projects_byte_identically() {
    let dir = TempDir::new().unwrap();
    // A styled + grouped + clipped scene, so "identical" means the whole
    // compositing path agrees and not merely the plain kernel.
    let build = |explicit: bool| {
        let defaults = if explicit {
            "\"knockout\":\"none\",\"blend_interior_as_group\":false,"
        } else {
            ""
        };
        let mut doc = scene(&dir, BG);
        doc = over_layer(&dir, "base.png", doc, 0, MID, "Base");
        doc = set_style(
            doc,
            1,
            &format!(
                "{{{defaults}\"fill_opacity\":0.6,\"effects\":[\
                 {{\"type\":\"drop_shadow\",\"distance\":2,\"size\":1}},\
                 {{\"type\":\"color_overlay\",\"color\":\"#00ff00\",\"opacity\":0.4}}]}}"
            ),
        );
        doc = over_layer(&dir, "m.png", doc, 1, HIGH, "M");
        doc = set_clipped(doc, 2);
        doc = over_layer(&dir, "inner.png", doc, 2, EXTRA, "Inner");
        let (doc, group) = group_one(doc, 3, "G");
        set_blend(doc, group, PASS_THROUGH)
    };
    let absent = build(false);
    let explicit = build(true);
    assert_eq!(
        flat_pixels(absent),
        flat_pixels(explicit),
        "the keys at their defaults change no pixel"
    );
    // The canonical JSON is identical too: neither key is emitted at default.
    assert_eq!(ffi_style(absent, 1), ffi_style(explicit, 1));
    unsafe { dr_doc_free(absent) };
    unsafe { dr_doc_free(explicit) };
}

#[test]
fn the_projection_scratch_estimate_grows_by_the_snapshots_the_design_charges() {
    let dir = TempDir::new().unwrap();
    let estimate = |doc: *const DrDocument| -> u64 {
        let ptrs = [doc];
        resource_report::report(&ptrs, &[], &[], &[], 0, 0).expect("a resource report")
            ["projection_scratch_estimate_bytes"]
            .as_u64()
            .unwrap()
    };
    // No knockout anywhere: the estimate is exactly what it was.
    let plain = clip_fixture(&dir, "none", Some(BASE_SHADOW));
    let base = estimate(plain);
    unsafe { dr_doc_free(plain) };
    // A STYLED-base clip unit with a knockout member: the Background
    // snapshot plus the clip unit's own.
    let styled = clip_fixture(&dir, "deep", Some(BASE_SHADOW));
    let with_clip = estimate(styled);
    unsafe { dr_doc_free(styled) };
    assert!(
        with_clip >= base + u64::from(W) * u64::from(H) * 16 * 2,
        "the styled-base clip unit's snapshots are charged: {base} -> {with_clip}"
    );
    // A Shallow child of a pass-through group charges a level snapshot too.
    let shallow = group_fixture(&dir, "shallow", PASS_THROUGH, None);
    let with_level = estimate(shallow);
    unsafe { dr_doc_free(shallow) };
    let plain_group = group_fixture(&dir, "none", PASS_THROUGH, None);
    let group_base = estimate(plain_group);
    unsafe { dr_doc_free(plain_group) };
    assert!(
        with_level > group_base,
        "the pass-through level's snapshot is charged: {group_base} -> {with_level}"
    );
}
