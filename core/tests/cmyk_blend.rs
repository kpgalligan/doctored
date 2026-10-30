//! Blend modes and Fill Opacity on a CMYK document (#1 phase 4a): every
//! offered mode over a 14 × 14 grid of inks (the Photoshop take's
//! backdrop rows × source columns) at opacity 1 and 0.5 and at Fill 0.5,
//! and on an adjustment record over a half-transparent backdrop, against
//! `common/cmyk_structure_fixture.rs`'s f64 W3C oracle on `1 − ink` (never
//! `blend.rs`); Normal bit-identical to phase 2's f32 source-over; Dissolve
//! showing whole samples; every refused mode and style part by its sentence.

use doctored_core::doc::BlendMode;

use crate::cmyk_structure_fixture::cmyk_layers;
use crate::cmyk_structure_fixture::{
    assert_composite, composite, free, refusal, then, try_edit, with_fill, with_levels, with_mode,
    with_opacity, RgbLayer,
};
use doctored_core::precision_document::PrecisionDocument;

/// The modes a CMYK document offers (the set
/// `scripts/photoshop-probe/cmyk_blend_take.py` take1 recorded).
const OFFERED: &[BlendMode] = &[
    BlendMode::Normal,
    BlendMode::Dissolve,
    BlendMode::Darken,
    BlendMode::Multiply,
    BlendMode::ColorBurn,
    BlendMode::LinearBurn,
    BlendMode::DarkerColor,
    BlendMode::Lighten,
    BlendMode::Screen,
    BlendMode::ColorDodge,
    BlendMode::Addition,
    BlendMode::LighterColor,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::HardLight,
    BlendMode::VividLight,
    BlendMode::LinearLight,
    BlendMode::PinLight,
    BlendMode::HardMix,
    BlendMode::Difference,
    BlendMode::Exclusion,
    BlendMode::Subtract,
    BlendMode::Divide,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
];

/// Every colour mode, by its name.
const ALL: &[(BlendMode, &str)] = &[
    (BlendMode::Normal, "Normal"),
    (BlendMode::Dissolve, "Dissolve"),
    (BlendMode::Darken, "Darken"),
    (BlendMode::Multiply, "Multiply"),
    (BlendMode::ColorBurn, "Color Burn"),
    (BlendMode::LinearBurn, "Linear Burn"),
    (BlendMode::DarkerColor, "Darker Color"),
    (BlendMode::Lighten, "Lighten"),
    (BlendMode::Screen, "Screen"),
    (BlendMode::ColorDodge, "Color Dodge"),
    (BlendMode::Addition, "Linear Dodge (Add)"),
    (BlendMode::LighterColor, "Lighter Color"),
    (BlendMode::Overlay, "Overlay"),
    (BlendMode::SoftLight, "Soft Light"),
    (BlendMode::HardLight, "Hard Light"),
    (BlendMode::VividLight, "Vivid Light"),
    (BlendMode::LinearLight, "Linear Light"),
    (BlendMode::PinLight, "Pin Light"),
    (BlendMode::HardMix, "Hard Mix"),
    (BlendMode::Difference, "Difference"),
    (BlendMode::Exclusion, "Exclusion"),
    (BlendMode::Subtract, "Subtract"),
    (BlendMode::Divide, "Divide"),
    (BlendMode::Hue, "Hue"),
    (BlendMode::Saturation, "Saturation"),
    (BlendMode::Color, "Color"),
    (BlendMode::Luminosity, "Luminosity"),
];

/// The take's fourteen inks, in percent: paper, K50, K100, rich black, the
/// three primaries, a neutral, four mixes and three fixed random ones.
const INKS: [[f64; 4]; 14] = [
    [0.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 50.0],
    [0.0, 0.0, 0.0, 100.0],
    [75.0, 68.0, 67.0, 90.0],
    [100.0, 0.0, 0.0, 0.0],
    [0.0, 100.0, 0.0, 0.0],
    [0.0, 0.0, 100.0, 0.0],
    [50.0, 50.0, 50.0, 0.0],
    [20.0, 40.0, 60.0, 10.0],
    [100.0, 100.0, 0.0, 0.0],
    [0.0, 60.0, 60.0, 20.0],
    [37.0, 81.0, 12.0, 44.0],
    [63.0, 7.0, 95.0, 18.0],
    [8.0, 52.0, 29.0, 57.0],
];

const N: u32 = 14;

fn ink_json(ink: [f64; 4]) -> String {
    format!(
        "[{},{},{},{},1]",
        ink[0] / 100.0,
        ink[1] / 100.0,
        ink[2] / 100.0,
        ink[3] / 100.0
    )
}

/// Paints layer `layer` with ink `i` wherever `covered(x, y)`.
fn paint_band(
    doc: *mut PrecisionDocument,
    layer: usize,
    opacity: f64,
    covered: impl Fn(u32, u32) -> bool,
    ink: [f64; 4],
) -> *mut PrecisionDocument {
    let stroke: Vec<u16> = (0..N * N)
        .map(|i| if covered(i % N, i / N) { 65535 } else { 0 })
        .collect();
    then(
        doc,
        &format!(
            r#"{{"op":"paint_blend","layer":{layer},"ink":{},"opacity":{opacity}}}"#,
            ink_json(ink)
        ),
        &stroke,
    )
}

/// The grid: layer 0 the backdrop ROWS (ink `y`), layer 1 the source
/// COLUMNS (ink `x`), both opaque.
fn grid() -> *mut PrecisionDocument {
    grid_of(&INKS)
}

/// [`grid`] over `inks`.
fn grid_of(inks: &[[f64; 4]; N as usize]) -> *mut PrecisionDocument {
    let mut doc = cmyk_layers(
        N,
        N,
        vec![RgbLayer {
            name: "Backdrop",
            w: N,
            h: N,
            offset: (0, 0),
            pixels: vec![[65535, 65535, 65535, 65535]; (N * N) as usize],
        }],
    );
    for (row, ink) in inks.iter().enumerate() {
        doc = paint_band(doc, 0, 1.0, |_, y| y as usize == row, *ink);
    }
    doc = then(
        doc,
        r#"{"op":"add_layer","name":"Source","anchor":0,"position":"above"}"#,
        &[],
    );
    for (column, ink) in inks.iter().enumerate() {
        doc = paint_band(doc, 1, 1.0, |x, _| x as usize == column, *ink);
    }
    doc
}

/// A new handle on `doc` with layer 1 at `mode`, `opacity` and `fill`; the
/// input is kept.
fn variant(
    doc: *const PrecisionDocument,
    mode: BlendMode,
    opacity: f32,
    fill: f64,
) -> *mut PrecisionDocument {
    // A rename gives a new handle and leaves `doc` as it was.
    let mut out = try_edit(doc, r#"{"op":"rename_layer","layer":1,"name":"Src"}"#, &[]).unwrap();
    if mode != BlendMode::Normal {
        out = with_mode(out, 1, mode.to_c());
    }
    if opacity != 1.0 {
        out = with_opacity(out, 1, opacity);
    }
    if fill != 1.0 {
        out = with_fill(out, 1, fill);
    }
    out
}

#[test]
fn every_offered_mode_is_the_w3c_formula_on_one_minus_ink() {
    let doc = grid();
    for &mode in OFFERED.iter().filter(|m| **m != BlendMode::Dissolve) {
        for (opacity, fill) in [(1.0, 1.0), (0.5, 1.0), (1.0, 0.5)] {
            let d = variant(doc, mode, opacity, fill);
            assert_composite(d, 2, &format!("{mode:?} opacity {opacity} fill {fill}"));
            free(d);
        }
    }
    free(doc);
}

/// `gray_luminance_take.py`'s Hard Mix codes (equal on all four plates),
/// then 0, 127, 128 and 255: every pair summing to 255 is an exact tie on
/// `L = 1 − ink`.
const TIE_CODES: [u8; N as usize] = [
    3, 50, 77, 100, 126, 129, 155, 178, 205, 252, 0, 127, 128, 255,
];

#[test]
fn hard_mix_ties_take_the_measured_order_on_one_minus_ink() {
    // Photoshop 26.10.0 on CMYK 8 (take1, HM-ORD): an exact tie on L is 1
    // (no ink) where the backdrop's L is the larger — the source carries more
    // ink — else 0 (full ink); off the tie `L_b + L_s ≥ 1`.
    let inks = TIE_CODES.map(|c| [f64::from(c) * 100.0 / 255.0; 4]);
    let doc = grid_of(&inks);
    let d = variant(doc, BlendMode::HardMix, 1.0, 1.0);
    for (y, &b) in TIE_CODES.iter().enumerate() {
        for (x, &s) in TIE_CODES.iter().enumerate() {
            let (lb, ls) = (255 - u32::from(b), 255 - u32::from(s));
            let on = if lb + ls == 255 {
                lb > ls
            } else {
                lb + ls >= 255
            };
            let want = if on { 0 } else { 65535 };
            let got = composite(d, x as i32, y as i32);
            for (plate, value) in got[..4].iter().enumerate() {
                assert!(
                    value.abs_diff(want) <= 2,
                    "backdrop {b} source {s} plate {plate}: {value} vs {want}"
                );
            }
        }
    }
    assert_composite(d, 2, "Hard Mix ties");
    free(d);
    free(doc);
}

/// Phase 2's `over` in f32, statement for statement, from transparent.
fn phase_two(layers: &[([u16; 5], f32)]) -> [u16; 5] {
    let mut dst = [0f32; 5];
    for (src, opacity) in layers {
        let s: [f32; 5] = src.map(|v| f32::from(v) / 65535.0);
        let sa = s[4] * opacity * 1.0;
        if sa <= 0.0 {
            continue;
        }
        let ab = dst[4];
        let ao = sa + ab * (1.0 - sa);
        for c in 0..4 {
            dst[c] = (sa * s[c] + (1.0 - sa) * ab * dst[c]) / ao;
        }
        dst[4] = ao;
    }
    dst.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
}

#[test]
fn normal_is_bit_identical_to_phase_two() {
    let doc = grid();
    let d = variant(doc, BlendMode::Normal, 0.5, 1.0);
    let (back, source) = unsafe {
        (
            (*d).cmyk_layer_samples(0).unwrap(),
            (*d).cmyk_layer_samples(1).unwrap(),
        )
    };
    // The record's 50 % composites at its byte, 128 / 255 (#2,
    // `opacity_byte`).
    for y in 0..N as i32 {
        for x in 0..N as i32 {
            let i = (y as u32 * N + x as u32) as usize;
            assert_eq!(
                composite(d, x, y),
                phase_two(&[(back[i], 1.0), (source[i], 128.0 / 255.0)]),
                "({x}, {y})"
            );
        }
    }
    free(d);
    free(doc);
}

#[test]
fn a_moded_adjustment_record_keeps_the_backdrop_alpha() {
    // The backdrop rows at alpha 0.5 on a transparent layer, the paper
    // hidden, a Levels record on top.
    let mut doc = cmyk_layers(
        N,
        N,
        vec![RgbLayer {
            name: "Paper",
            w: N,
            h: N,
            offset: (0, 0),
            pixels: vec![[65535, 65535, 65535, 65535]; (N * N) as usize],
        }],
    );
    doc = then(
        doc,
        r#"{"op":"layer_visible","layer":0,"visible":false}"#,
        &[],
    );
    doc = then(
        doc,
        r#"{"op":"add_layer","name":"Rows","anchor":0,"position":"above"}"#,
        &[],
    );
    for (row, ink) in INKS.iter().enumerate() {
        doc = paint_band(doc, 1, 0.5, |_, y| y as usize == row, *ink);
    }
    let doc = with_levels(doc, 1.0);
    let rows = unsafe { &*doc }.cmyk_layer_samples(1).unwrap();
    for &mode in OFFERED.iter().filter(|m| **m != BlendMode::Dissolve) {
        for opacity in [1.0f32, 0.5] {
            let mut d =
                try_edit(doc, r#"{"op":"rename_layer","layer":2,"name":"Tone"}"#, &[]).unwrap();
            if mode != BlendMode::Normal {
                d = with_mode(d, 2, mode.to_c());
            }
            if opacity != 1.0 {
                d = with_opacity(d, 2, opacity);
            }
            assert_composite(d, 2, &format!("adjustment {mode:?} at {opacity}"));
            for y in 0..N as i32 {
                for x in 0..N as i32 {
                    let i = (y as u32 * N + x as u32) as usize;
                    assert_eq!(composite(d, x, y)[4], rows[i][4], "alpha is kept");
                }
            }
            free(d);
        }
    }
    free(doc);
}

#[test]
fn dissolve_shows_whole_samples_of_the_source_or_the_backdrop() {
    let doc = grid();
    let d = variant(doc, BlendMode::Dissolve, 0.5, 1.0);
    let (back, source) = unsafe {
        (
            (*d).cmyk_layer_samples(0).unwrap(),
            (*d).cmyk_layer_samples(1).unwrap(),
        )
    };
    let mut shown = 0;
    for y in 0..N as i32 {
        for x in 0..N as i32 {
            let i = (y as u32 * N + x as u32) as usize;
            let got = composite(d, x, y);
            if got == source[i] && got != back[i] {
                shown += 1;
            }
            assert!(
                got == source[i] || got == back[i],
                "({x}, {y}): {got:?} is neither {:?} nor {:?}",
                source[i],
                back[i]
            );
        }
    }
    let fraction = f64::from(shown) / f64::from(N * N);
    assert!((0.3..0.7).contains(&fraction), "shown {fraction}");
    free(d);
    free(doc);
}

#[test]
fn every_refused_mode_and_style_part_names_itself() {
    let doc = grid();
    for &(mode, name) in ALL.iter().filter(|(m, _)| !OFFERED.contains(m)) {
        assert_eq!(
            refusal(
                doc,
                &format!(
                    r#"{{"op":"layer_blend","version":1,"layer":1,"blend_mode":{}}}"#,
                    mode.to_c()
                ),
                &[]
            ),
            format!("The {name} blend mode is unavailable in a CMYK document")
        );
    }
    let style = |style: &str| {
        refusal(
            doc,
            &format!(r#"{{"op":"layer_style","version":1,"layer":1,"style":{style}}}"#),
            &[],
        )
    };
    // The styles batch admits effects and knockout (#1), the Blend If batch
    // Gray and the plates; a Blend If on Red, Green or Blue refuses by name.
    for admitted in [
        r#"{"fill_opacity":0.5,"effects":[{"type":"drop_shadow"}]}"#,
        r#"{"knockout":"shallow"}"#,
        r#"{"blend_if":{"channel":"gray","this_layer":[10,20,240,255]}}"#,
    ] {
        let out = try_edit(
            doc,
            &format!(r#"{{"op":"layer_style","version":1,"layer":1,"style":{admitted}}}"#),
            &[],
        )
        .unwrap_or_else(|e| panic!("{admitted}: {e}"));
        assert!(!out.is_null(), "{admitted} changed nothing");
        free(out);
    }
    assert_eq!(
        style(r#"{"blend_if":{"channel":"red","this_layer":[10,20,240,255]}}"#),
        "Blend If on the Red channel is unavailable in a CMYK document"
    );
    assert_eq!(
        refusal(
            doc,
            r#"{"op":"layer_blend_if","version":1,"layer":1,"blend_if":{"channel":"blue","this_layer":[10,20,240,255]}}"#,
            &[]
        ),
        "Blend If on the Blue channel is unavailable in a CMYK document"
    );
    // A fill-only style is admitted, and clearing it is too.
    let filled = with_fill(
        try_edit(doc, r#"{"op":"rename_layer","layer":1,"name":"Src"}"#, &[]).unwrap(),
        1,
        0.25,
    );
    let cleared = then(
        filled,
        r#"{"op":"layer_style","version":1,"layer":1,"style":null}"#,
        &[],
    );
    assert!(unsafe { &*cleared }.layer(1).unwrap().style().is_none());
    free(cleared);
    free(doc);
}
