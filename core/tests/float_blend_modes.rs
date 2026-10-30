//! The nine blend modes a 32-bit float document now
//! composites — Dissolve, Darker Color, Linear Dodge (Add),
//! Lighter Color, Subtract, Hue, Saturation, Color and Luminosity — on a raster
//! layer, an adjustment layer, an isolated group and as Fill… ink, through the
//! request API (`layer_blend`, `paint_blend`), `dr_precision_doc_set_layer_opacity`
//! and `dr_precision_doc_paste_layers`; the native round trip. Linear Burn,
//! Linear Light, Pin Light and Divide are added on the same paths, and
//! Screen and Overlay stand for the nine modes still refused.
//!
//! The oracle is written here in binary64 from Photoshop 26.10.0's MEASURED
//! 32-bit formulas (`scripts/photoshop-probe/float_blend.py`, receipt
//! `doctored-fixtures/photoshop/float-blend-473/take3`), never read off the
//! implementation: Linear Dodge `cb + cs` and Subtract `cb − cs` unclamped;
//! Darker / Lighter Color the whole-triple pick by the W3C weights
//! (0.3, 0.59, 0.11), a tie keeping the backdrop; Hue, Saturation, Color and
//! Luminosity the W3C SetSat / SetLum with those weights and NO ClipColor;
//! Dissolve all-or-nothing, a shown pixel the source at alpha 1. The rest
//! are the W3C formulas with no clamp, chosen rather than measured: Linear
//! Burn `cb + cs − 1`, Linear Light `cb + 2·cs − 1`, Pin Light `min(cb, 2·cs)`
//! for `cs ≤ 0.5` else `max(cb, 2·cs − 1)`, and Divide
//! `clamp(cb / max(cs, 1/65504), −65504, 65504)`. Each is composited by the
//! W3C source-over step. Tolerance `1e-5 · max(1, |want|)`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_precision::dr_precision_doc_set_layer_opacity;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{ClipboardPayload, PasteRequest};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const EMPTY_REPORT: &str = r#"{"blend_reset":[],"effect_blend_reset":[],"rasterized":[]}"#;
const EXPOSURE: &str =
    r#"{"type":"adjust","evaluation":"precision_v1","op":"exposure","params":{"exposure":1.0}}"#;
/// The built modes other than Dissolve, by C code: Linear Dodge, Subtract,
/// Darker Color, Lighter Color, Hue, Saturation, Color, Luminosity, and
/// Linear Burn, Linear Light, Pin Light and Divide.
const BUILT: [i32; 12] = [12, 13, 16, 17, 23, 24, 25, 26, 15, 19, 20, 22];
const DISSOLVE: i32 = 14;
const OVERLAY: i32 = 3;
const SCREEN: i32 = 2;

/// Backdrop triples: zero, neutrals below and above white, a uniform
/// negative, primaries, mixed colours and a negative component.
const BACKDROPS: [[f32; 3]; 12] = [
    [0.0, 0.0, 0.0],
    [0.18, 0.18, 0.18],
    [1.0, 1.0, 1.0],
    [2.5, 2.5, 2.5],
    [8.0, 8.0, 8.0],
    [2.0, 0.0, 0.0],
    [0.5, 0.2, 0.1],
    [1.5, 0.5, 0.25],
    [0.9, 0.1, 0.6],
    [-0.25, 0.5, 1.0],
    [-0.5, -0.5, -0.5],
    [3.0, -1.0, 0.5],
];
/// Source triples: the backdrops plus two more colours.
const SOURCES: [[f32; 3]; 14] = [
    [0.0, 0.0, 0.0],
    [0.18, 0.18, 0.18],
    [1.0, 1.0, 1.0],
    [2.5, 2.5, 2.5],
    [8.0, 8.0, 8.0],
    [2.0, 0.0, 0.0],
    [0.5, 0.2, 0.1],
    [1.5, 0.5, 0.25],
    [0.9, 0.1, 0.6],
    [-0.25, 0.5, 1.0],
    [-0.5, -0.5, -0.5],
    [3.0, -1.0, 0.5],
    [0.0, 2.0, 0.0],
    [4.0, 0.5, 0.0],
];
/// Every source against every backdrop, at source alpha 1 and 0.5 (the
/// right half) and backdrop alpha 1 and 0.5 (the bottom half).
const W: u32 = 2 * SOURCES.len() as u32;
const H: u32 = 2 * BACKDROPS.len() as u32;

// --------------------------------------------------------------- oracle --

fn lum(c: [f64; 3]) -> f64 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn sat(c: [f64; 3]) -> f64 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// W3C SetSat: min channel to 0, max to `s`, the middle in proportion.
fn set_sat(c: [f64; 3], s: f64) -> [f64; 3] {
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| c[a].total_cmp(&c[b]));
    let [lo, mid, hi] = order;
    let mut out = [0.0; 3];
    if c[hi] > c[lo] {
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo]);
        out[hi] = s;
    }
    out
}

/// SetLum as Photoshop measures it at 32 bits: the shift, with no ClipColor.
fn set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
    let d = l - lum(c);
    [c[0] + d, c[1] + d, c[2] + d]
}

/// `B(Cb, Cs)` for a built non-Dissolve mode. `None` for Darker / Lighter
/// Color when the two luminosities are too close for f32 and f64 to agree on
/// the pick; the caller then accepts either triple.
fn blend(mode: i32, cb: [f64; 3], cs: [f64; 3]) -> Option<[f64; 3]> {
    Some(match mode {
        12 => [cb[0] + cs[0], cb[1] + cs[1], cb[2] + cs[2]],
        13 => [cb[0] - cs[0], cb[1] - cs[1], cb[2] - cs[2]],
        16 | 17 => {
            let (ls, lb) = (lum(cs), lum(cb));
            if (ls - lb).abs() <= 1e-5 * lb.abs().max(1.0) && cs != cb {
                return None;
            }
            let pick = if mode == 16 { ls < lb } else { ls > lb };
            if pick {
                cs
            } else {
                cb
            }
        }
        15 => std::array::from_fn(|c| cb[c] + cs[c] - 1.0),
        19 => std::array::from_fn(|c| cb[c] + 2.0 * cs[c] - 1.0),
        20 => std::array::from_fn(|c| {
            if cs[c] <= 0.5 {
                cb[c].min(2.0 * cs[c])
            } else {
                cb[c].max(2.0 * cs[c] - 1.0)
            }
        }),
        22 => std::array::from_fn(|c| (cb[c] / cs[c].max(1.0 / 65504.0)).clamp(-65504.0, 65504.0)),
        23 => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        24 => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        25 => set_lum(cs, lum(cb)),
        26 => set_lum(cb, lum(cs)),
        _ => unreachable!("mode {mode} is not a built non-Dissolve mode"),
    })
}

/// One W3C source-over step of straight `cs` at `sa` onto straight `bg`.
fn composite(bg: [f64; 4], cs: [f64; 3], sa: f64, b: [f64; 3]) -> [f64; 4] {
    let ab = bg[3];
    let ao = sa + ab * (1.0 - sa);
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = (sa * (1.0 - ab) * cs[c] + sa * ab * b[c] + (1.0 - sa) * ab * bg[c]) / ao;
    }
    out[3] = ao;
    out
}

fn close(got: f32, want: f64) -> bool {
    (f64::from(got) - want).abs() <= 1e-5 * want.abs().max(1.0)
}

fn wide(p: [f32; 4]) -> [f64; 4] {
    p.map(f64::from)
}

fn rgb(p: [f64; 4]) -> [f64; 3] {
    [p[0], p[1], p[2]]
}

/// Checks `got` against the composite of `cs` at `sa` over `bg` under `mode`,
/// accepting either pick when the oracle declines to choose.
fn check(mode: i32, bg: [f64; 4], cs: [f64; 3], sa: f64, got: [f32; 4], what: &str) {
    assert!(got.iter().all(|v| v.is_finite()), "{what}: {got:?}");
    let picks = match blend(mode, rgb(bg), cs) {
        Some(b) => vec![b],
        None => vec![cs, rgb(bg)],
    };
    let ok = picks.iter().any(|b| {
        let want = composite(bg, cs, sa, *b);
        (0..4).all(|c| close(got[c], want[c]))
    });
    assert!(
        ok,
        "{what}: mode {mode} got {got:?}, want {:?} (cb {bg:?}, cs {cs:?}, sa {sa})",
        picks
            .iter()
            .map(|b| composite(bg, cs, sa, *b))
            .collect::<Vec<_>>()
    );
}

// ------------------------------------------------------------- fixtures --

fn backdrop_pixels() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let y = i / W;
            let c = BACKDROPS[(y as usize) % BACKDROPS.len()];
            let a = if y as usize >= BACKDROPS.len() {
                0.5
            } else {
                1.0
            };
            [c[0], c[1], c[2], a]
        })
        .collect()
}

fn source_pixels() -> Vec<[f32; 4]> {
    (0..W * H)
        .map(|i| {
            let x = i % W;
            let c = SOURCES[(x as usize) % SOURCES.len()];
            let a = if x as usize >= SOURCES.len() {
                0.5
            } else {
                1.0
            };
            [c[0], c[1], c[2], a]
        })
        .collect()
}

/// The test's own RZCB v3 writer (`layer_clipboard_float.rs`'s): float
/// samples, no coverage, no editable section, offset (0, 0).
fn drcb(w: u32, h: u32, profile: &[u8], pixels: &[[f32; 4]]) -> Vec<u8> {
    let mut out = b"RZCB".to_vec();
    out.extend(3u32.to_le_bytes());
    out.extend(2u32.to_le_bytes());
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    out.extend(0i32.to_le_bytes());
    out.extend(0i32.to_le_bytes());
    out.extend(72f32.to_le_bytes());
    out.extend(72f32.to_le_bytes());
    out.extend((profile.len() as u32).to_le_bytes());
    out.extend(profile);
    out.push(0);
    out.extend(0u32.to_le_bytes());
    out.extend(4f32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    pixels
        .iter()
        .flatten()
        .for_each(|v| out.extend(v.to_le_bytes()));
    out
}

fn payload(pixels: &[[f32; 4]]) -> ClipboardPayload {
    let bytes = drcb(W, H, IccProfile::srgb().bytes(), pixels);
    ClipboardPayload::decode(&bytes, LIMIT, 0).expect("a payload")
}

fn floats(doc: &PrecisionDocument, index: usize) -> Vec<[f32; 4]> {
    doc.layer_source(index).unwrap().to_rgba32().unwrap()
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

/// A float document whose one raster holds the backdrop samples.
fn backdrop_doc() -> PrecisionDocument {
    let samples = backdrop_pixels();
    let doc = PrecisionDocument::from_clipboard(&payload(&samples), LIMIT, 0, None)
        .expect("a float document");
    assert!(doc.is_float());
    assert_eq!(bits(&floats(&doc, 0)), bits(&samples));
    doc
}

/// [`backdrop_doc`] with the source samples pasted above it as record 1,
/// bit for bit (same storage, same profile: nothing converts).
fn two_layer_doc() -> PrecisionDocument {
    let samples = source_pixels();
    let request: PasteRequest = serde_json::from_value(json!({
        "name": "Source", "anchor": 0, "position": "above",
        "placement": "in_place", "profile": "preserve",
    }))
    .unwrap();
    let doc = backdrop_doc()
        .clipboard_paste(&payload(&samples), &request, LIMIT, 0, None)
        .expect("the source pastes");
    assert_eq!(doc.layer_count(), 2);
    assert_eq!(bits(&floats(&doc, 1)), bits(&samples));
    doc
}

fn take(err: *mut c_char) -> String {
    if err.is_null() {
        return String::new();
    }
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    text
}

/// One request through `dr_precision_doc_edit`; `Err` is its refusal.
fn edit_with(
    doc: &PrecisionDocument,
    request: Value,
    input: &[u16],
) -> Result<PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if input.is_empty() {
                ptr::null()
            } else {
                input.as_ptr()
            },
            input.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if !err.is_null() {
        assert!(out.is_null());
        return Err(take(err));
    }
    assert!(!out.is_null(), "{request} changed nothing");
    Ok(*unsafe { Box::from_raw(out) })
}

fn with_mode(doc: &PrecisionDocument, layer: usize, mode: i32) -> PrecisionDocument {
    edit_with(
        doc,
        json!({"op":"layer_blend","version":1,"layer":layer,"blend_mode":mode}),
        &[],
    )
    .expect("the mode is set")
}

fn with_opacity(doc: &PrecisionDocument, layer: usize, opacity: f32) -> PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_layer_opacity(doc, layer, opacity, &mut err) };
    assert!(err.is_null(), "{}", take(err));
    if out.is_null() {
        // An unchanged opacity is no edit.
        assert_eq!(doc.layer(layer).unwrap().opacity(), opacity);
        return doc.clone();
    }
    *unsafe { Box::from_raw(out) }
}

fn render(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    assert_eq!(doc.float_refusal(), None);
    doc.render(LIMIT, 0, None)
        .expect("the float scene renders")
        .to_rgba32()
        .unwrap()
}

/// The composite of the source record over the backdrop for every built
/// non-Dissolve mode at opacity 1 and 0.5, against the oracle.
fn assert_layer_composites(doc: &PrecisionDocument, layer: usize, what: &str) {
    let (back, source) = (backdrop_pixels(), source_pixels());
    for mode in BUILT {
        for opacity in [1.0f32, 0.5] {
            let moded = with_opacity(&with_mode(doc, layer, mode), layer, opacity);
            let out = render(&moded);
            for (i, got) in out.iter().enumerate() {
                let sa = f64::from(source[i][3]) * f64::from(opacity);
                let cs = rgb(wide(source[i]));
                check(
                    mode,
                    wide(back[i]),
                    cs,
                    sa,
                    *got,
                    &format!("{what} opacity {opacity} pixel {i}"),
                );
            }
        }
    }
}

// ---------------------------------------------------------------- tests --

#[test]
fn every_built_mode_composites_a_raster_layer_by_the_measured_formula() {
    assert_layer_composites(&two_layer_doc(), 1, "raster");
}

#[test]
fn an_isolated_group_carrying_the_mode_composites_by_the_same_formula() {
    let grouped = edit_with(
        &two_layer_doc(),
        json!({"op":"group_layers","layers":[1],"name":"G","pass_through":false}),
        &[],
    )
    .expect("grouped");
    // Postorder, bottom-first: the backdrop, the source inside, the group.
    assert_eq!(grouped.layer_count(), 3);
    assert_layer_composites(&grouped, 2, "group");
}

#[test]
fn an_exposure_layer_carrying_the_mode_blends_its_output_over_the_backdrop() {
    let back = backdrop_pixels();
    let base = backdrop_doc()
        .add_adjustment("Exposure", EXPOSURE, 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    for mode in BUILT {
        for opacity in [1.0f32, 0.5] {
            let out = render(&with_opacity(&with_mode(&base, 1, mode), 1, opacity));
            for (i, got) in out.iter().enumerate() {
                let cb = rgb(wide(back[i]));
                // +1 EV in linear light is a gain of exactly two, pushed
                // through the mode as the SOURCE and mixed in by `opacity`.
                let adjusted = cb.map(|v| 2.0 * v);
                let picks = match blend(mode, cb, adjusted) {
                    Some(b) => vec![b],
                    None => vec![adjusted, cb],
                };
                let k = f64::from(opacity);
                let ok = picks.iter().any(|b| {
                    (0..3).all(|c| close(got[c], cb[c] + (b[c] - cb[c]) * k))
                        && got[3] == back[i][3]
                });
                assert!(
                    ok,
                    "mode {mode} opacity {opacity} pixel {i}: {got:?} over {cb:?}"
                );
            }
        }
    }
}

#[test]
fn dissolve_shows_the_source_or_keeps_the_backdrop_bit_for_bit() {
    let reference = render(&backdrop_doc());
    let source = source_pixels();
    for opacity in [1.0f32, 0.5] {
        let out = render(&with_opacity(
            &with_mode(&two_layer_doc(), 1, DISSOLVE),
            1,
            opacity,
        ));
        let (mut shown, mut kept) = (0, 0);
        for (i, got) in out.iter().enumerate() {
            let s = source[i];
            if got.map(f32::to_bits) == [s[0], s[1], s[2], 1.0].map(f32::to_bits) {
                shown += 1;
            } else {
                assert_eq!(
                    got.map(f32::to_bits),
                    reference[i].map(f32::to_bits),
                    "opacity {opacity} pixel {i}: neither the source nor the backdrop"
                );
                kept += 1;
            }
        }
        // A source above white survives unclamped wherever it shows (4.0
        // in column 13), and both outcomes occur where the effective alpha
        // is below 1.
        assert!(
            shown > 0 && kept > 0,
            "opacity {opacity}: {shown} shown, {kept} kept"
        );
        let bright = (0..W * H).find(|&i| {
            let i = i as usize;
            source[i][0] == 4.0 && out[i][0] == 4.0
        });
        assert!(bright.is_some(), "a 4.0 source pixel shows unclamped");
    }
}

#[test]
fn negative_and_zero_triples_stay_finite_in_every_built_mode() {
    let doc = two_layer_doc();
    for mode in BUILT.iter().copied().chain([DISSOLVE]) {
        let out = render(&with_mode(&doc, 1, mode));
        assert!(
            out.iter().flatten().all(|v| v.is_finite()),
            "mode {mode} produced a non-finite sample"
        );
    }
}

#[test]
fn the_mode_survives_a_native_round_trip_bit_for_bit() {
    for mode in BUILT.iter().copied().chain([DISSOLVE]) {
        let doc = with_opacity(&with_mode(&two_layer_doc(), 1, mode), 1, 0.5);
        let bytes = doc.encode_native(LIMIT, 0, None).expect("it saves");
        let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).expect("it reopens");
        assert!(reopened.is_float());
        assert_eq!(
            reopened.layer(1).unwrap().blend_mode(),
            BlendMode::from_c(mode).unwrap()
        );
        assert_eq!(bits(&floats(&reopened, 1)), bits(&floats(&doc, 1)));
        assert_eq!(bits(&render(&reopened)), bits(&render(&doc)), "mode {mode}");
        assert_eq!(
            reopened.encode_native(LIMIT, 0, None).unwrap(),
            bytes,
            "mode {mode}: re-encoding is byte-identical"
        );
    }
}

fn u16_layer(name: &str, mode: BlendMode, profile: &Arc<IccProfile>) -> Layer {
    let pixels: Vec<[u16; 4]> = (0..W * H)
        .map(|i| [(i * 977 % 65536) as u16, 40000, 1234, 65535])
        .collect();
    let image = PrecisionImage::from_rgba16(W, H, pixels, profile.clone(), None).unwrap();
    Layer::raster(name, image, (0, 0), 1.0, true)
        .unwrap()
        .with_blend_mode(mode)
        .unwrap()
}

/// `dr_precision_doc_paste_layers` above record 0; `Err` is its refusal.
fn paste_layers(
    doc: &PrecisionDocument,
    clip: &PrecisionDocument,
) -> Result<(PrecisionDocument, String), String> {
    let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
    let out = unsafe {
        dr_precision_doc_paste_layers(
            doc,
            clip,
            0,
            0,
            0,
            0,
            false,
            LIMIT,
            0,
            &mut report,
            &mut err,
        )
    };
    if !err.is_null() {
        assert!(out.is_null());
        return Err(take(err));
    }
    assert!(!out.is_null());
    let report: Value = serde_json::from_str(&take(report)).expect("a JSON report");
    Ok((*unsafe { Box::from_raw(out) }, report.to_string()))
}

#[test]
fn a_sixteen_bit_layer_carrying_a_built_mode_pastes_with_an_empty_report() {
    let srgb = IccProfile::srgb();
    let dest = backdrop_doc();
    for mode in BUILT.iter().copied().chain([DISSOLVE]) {
        let mode = BlendMode::from_c(mode).unwrap();
        let clip = PrecisionDocument::from_parts(
            W,
            H,
            srgb.clone(),
            Metadata::default(),
            Resolution::default(),
            vec![u16_layer("Moded", mode, &srgb)],
        )
        .unwrap();
        let (out, report) = paste_layers(&dest, &clip).expect("the paste lands");
        assert_eq!(report, EMPTY_REPORT, "{mode:?}");
        assert_eq!(out.layer(1).unwrap().blend_mode(), mode);
        assert_eq!(out.float_refusal(), None);
        render(&out);
    }
    // A refused mode lands Normal with its `blend_reset` row, as before.
    let clip = PrecisionDocument::from_parts(
        W,
        H,
        srgb.clone(),
        Metadata::default(),
        Resolution::default(),
        vec![u16_layer("Screened", BlendMode::Screen, &srgb)],
    )
    .unwrap();
    let (out, report) = paste_layers(&dest, &clip).expect("the paste lands");
    assert_eq!(out.layer(1).unwrap().blend_mode(), BlendMode::Normal);
    assert!(report.contains(r#""from":"screen""#), "{report}");
}

#[test]
fn screen_and_overlay_still_refuse_on_a_layer() {
    let doc = two_layer_doc();
    for (mode, name) in [(SCREEN, "screen"), (OVERLAY, "overlay")] {
        let moded = with_mode(&doc, 1, mode);
        assert_eq!(
            moded.float_refusal().as_deref(),
            Some(
                format!("the {name} blend mode is unavailable in a 32-bit float document").as_str()
            )
        );
    }
}

/// The ink an opaque Normal fill at full coverage leaves: the decoded
/// document ink of `color`'s RGB.
fn decoded_ink(doc: &PrecisionDocument, color: [f64; 4]) -> [f64; 3] {
    let opaque = [color[0], color[1], color[2], 1.0];
    let out = edit_with(doc, fill(opaque, 1.0, 0, false), &[65535; (W * H) as usize]).unwrap();
    let p = floats(&out, 0)[0];
    [p[0], p[1], p[2]].map(f64::from)
}

fn fill(color: [f64; 4], opacity: f64, mode: i32, preserve: bool) -> Value {
    json!({"op":"paint_blend","layer":0,"color":color,"color_space":"document",
        "opacity":opacity,"blend_mode":mode,"selection":false,"preserve_alpha":preserve})
}

#[test]
fn fill_with_each_built_mode_matches_the_oracle_with_and_without_preserve_transparency() {
    let doc = backdrop_doc();
    let back = backdrop_pixels();
    let color = [0.9, 0.45, 0.15, 0.8];
    let ink = decoded_ink(&doc, color);
    let n = (W * H) as usize;
    // A coverage ramp: 0, full and partial weights.
    let coverage: Vec<u16> = (0..n)
        .map(|i| match i % 4 {
            0 => 0,
            1 => 65535,
            k => (k as u16) * 20000,
        })
        .collect();
    for mode in BUILT {
        for preserve in [false, true] {
            let out = edit_with(&doc, fill(color, 0.7, mode, preserve), &coverage)
                .expect("the fill changes the layer");
            for (i, got) in floats(&out, 0).iter().enumerate() {
                let sa = color[3] * 0.7 * f64::from(coverage[i]) / 65535.0;
                let bg = wide(back[i]);
                if sa <= 0.0 {
                    assert_eq!(got.map(f32::to_bits), back[i].map(f32::to_bits));
                    continue;
                }
                let what = format!("fill mode {mode} preserve {preserve} pixel {i}");
                if preserve {
                    // Frozen alpha: `lerp(Cb, B(Cb, Cs), sa)`, alpha kept.
                    let picks = match blend(mode, rgb(bg), ink) {
                        Some(b) => vec![b],
                        None => vec![ink, rgb(bg)],
                    };
                    let ok = picks.iter().any(|b| {
                        (0..3).all(|c| close(got[c], bg[c] + (b[c] - bg[c]) * sa))
                            && got[3] == back[i][3]
                    });
                    assert!(ok, "{what}: {got:?} over {bg:?}");
                } else {
                    check(mode, bg, ink, sa, *got, &what);
                }
            }
        }
    }
}

#[test]
fn a_dissolve_fill_shows_the_ink_or_keeps_the_layer() {
    let doc = backdrop_doc();
    let back = backdrop_pixels();
    let color = [0.9, 0.45, 0.15, 1.0];
    let ink = decoded_ink(&doc, color);
    let n = (W * H) as usize;
    let out = edit_with(&doc, fill(color, 0.5, DISSOLVE, false), &vec![65535; n]).unwrap();
    let (mut shown, mut kept) = (0, 0);
    for (i, got) in floats(&out, 0).iter().enumerate() {
        if got.map(f32::to_bits) == back[i].map(f32::to_bits) {
            kept += 1;
        } else {
            assert_eq!(
                got.map(f64::from),
                [ink[0], ink[1], ink[2], 1.0],
                "pixel {i}: neither the ink nor the layer"
            );
            shown += 1;
        }
    }
    assert!(shown > 0 && kept > 0, "{shown} shown, {kept} kept");
}

#[test]
fn an_overlay_fill_refuses_with_the_float_sentence() {
    let doc = backdrop_doc();
    let message = edit_with(
        &doc,
        fill([0.5, 0.5, 0.5, 1.0], 1.0, OVERLAY, false),
        &[65535; (W * H) as usize],
    )
    .err()
    .expect("Overlay refuses");
    assert_eq!(
        message,
        "the overlay blend mode is unavailable in a 32-bit float document"
    );
}
