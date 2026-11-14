//! Frequency separation beyond the encoded RGB split, through the FFI: each
//! form's detail layer over its low layer, set to its blend mode and clipped,
//! recomposes the layer within the bound declared here before measuring
//! (`frequency_separation.rs` covers 8- and 16-bit RGB).
//!
//! - **32-bit float** (RGB and Grayscale): `layer_detail` writes `t − s`
//!   unclamped, negatives kept, and Linear Dodge (Add) rebuilds the layer
//!   within `2^-22 · max(|t|, |s|)` per component. Oracle: the difference and
//!   the sum in f64. Control: the same pair under Linear Light is far off.
//! - **Grayscale and Lab at 8 and 16 bits**: Apply Image's Subtract at scale 2
//!   and offset ½ on the stored codes (`dr_selection_u16`, one plane for Gray
//!   written by `scalar_edit` target `gray`; L, a and b as the red, green and
//!   blue planes for Lab), Linear Light, within ONE u16 code of every stored
//!   channel (so within one code of the 8-bit readout). Lab's a and b span both
//!   signs about 128. Control: a pivot of 32768 reaches two codes.
//! - **16 bits evaluated in linear light**: `layer_detail`'s decoded form,
//!   Linear Light, within 4 codes of 65535 in DECODED components (the
//!   detail's rounding doubled at its decode slope, ≤ 2.275 codes per code
//!   under sRGB, plus the render's own half code there) under sRGB and under
//!   Adobe RGB's pure power; in STORED codes within 24 under sRGB (its 12.92
//!   toe times the detail's error at black) and 600 under Adobe RGB (the power
//!   curve's unbounded encode slope at black: ≈ 510 by the same arithmetic,
//!   plus its encoder's own error). Oracle: the published curves in f64.
//!   Control: Apply Image's stored-code detail on the same document is far
//!   off.
//!
//! Each low is a real Gaussian Blur of the layer and, separately, unrelated
//! noise: the bound holds for ANY low, because each detail is computed from
//! what the low layer stores.
use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use crate::common::{take_err_string, BLEND_ADDITION, BLEND_LINEAR_LIGHT, BLEND_SUBTRACT};
use crate::float_fixture::float_doc;
use crate::native_scalar::{text, LIMIT};
use doctored_core::ffi_evaluation::dr_precision_doc_set_evaluation;
use doctored_core::ffi_gray::dr_precision_doc_convert_to_gray;
use doctored_core::ffi_lab::dr_precision_doc_convert_to_lab;
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::ffi_selection_u16::dr_selection_u16;
use doctored_core::icc::IccProfile;
use doctored_core::precision_capability::{capability, require_gray};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

const W: u32 = 48;
const H: u32 = 36;

/// A deterministic xorshift.
struct Noise(u64);
impl Noise {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// A unit value on a 1/4096 lattice.
    fn unit(&mut self) -> f64 {
        (self.next() % 4097) as f64 / 4096.0
    }
}

fn owned(
    out: *mut PrecisionDocument,
    err: *mut c_char,
) -> Result<Option<PrecisionDocument>, String> {
    if !out.is_null() {
        Ok(Some(*unsafe { Box::from_raw(out) }))
    } else if err.is_null() {
        Ok(None)
    } else {
        Err(take_err_string(err))
    }
}

fn edit(
    doc: &PrecisionDocument,
    request: Value,
    input: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    let request = text(request);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
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
    owned(out, err)
}

/// `request`, which must change the document.
fn then(doc: &PrecisionDocument, request: Value, input: &[u16]) -> PrecisionDocument {
    edit(doc, request.clone(), input)
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

fn detail(doc: &PrecisionDocument) -> Result<Option<PrecisionDocument>, String> {
    edit(
        doc,
        json!({"op":"layer_detail","version":1,"layer":1,"source":0}),
        &[],
    )
}

/// The refusal an edit must answer.
fn refused(result: Result<Option<PrecisionDocument>, String>, what: &str) -> String {
    match result {
        Err(refusal) => refusal,
        Ok(_) => panic!("{what} was not refused"),
    }
}

/// Layer 1 in `mode`, clipped to layer 0.
fn paired(doc: &PrecisionDocument, mode: i32) -> PrecisionDocument {
    let moded = then(
        doc,
        json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":mode}),
        &[],
    );
    then(
        &moded,
        json!({"op":"layer_clipped","layer":1,"clipped":true}),
        &[],
    )
}

/// Layer 0's Gaussian Blur at `sigma`, the document's own filter.
fn blurred(doc: &PrecisionDocument, sigma: f64) -> PrecisionDocument {
    let request = text(json!({"layer":0,"op":"blur","params":{"sigma":sigma}}));
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_selected_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    owned(out, err)
        .unwrap()
        .expect("the blur changes the layer")
}

fn codes(doc: &PrecisionDocument, layer: usize) -> Vec<[u16; 4]> {
    doc.layer_source(layer).unwrap().to_rgba16().unwrap()
}

fn floats(doc: &PrecisionDocument, layer: usize) -> Vec<[f32; 4]> {
    doc.layer_source(layer).unwrap().to_rgba32().unwrap()
}

fn render16(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}

fn render32(doc: &PrecisionDocument) -> Vec<[f32; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba32().unwrap()
}

fn raster(name: &str, pixels: Vec<[u16; 4]>, profile: &Arc<IccProfile>) -> PrecisionDocumentLayer {
    let image = PrecisionImage::from_rgba16(W, H, pixels, profile.clone(), None).unwrap();
    PrecisionDocumentLayer::raster(name, image, (0, 0), 1.0, true).unwrap()
}

/// A 16-bit RGB document of `layers`, bottom first, in `profile`.
fn rgb16(profile: &Arc<IccProfile>, layers: Vec<(&str, Vec<[u16; 4]>)>) -> PrecisionDocument {
    let records = layers
        .into_iter()
        .map(|(name, p)| raster(name, p, profile))
        .collect();
    PrecisionDocument::from_parts(
        W,
        H,
        profile.clone(),
        Default::default(),
        Default::default(),
        records,
    )
    .unwrap()
}

/// Opaque colour noise over hard edges, every channel spanning the range.
fn picture16(seed: u64) -> Vec<[u16; 4]> {
    let mut noise = Noise(seed);
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let edge = if (x / 7 + y / 5) % 2 == 0 { 0.15 } else { 0.8 };
            let rgb: [u16; 3] = std::array::from_fn(|c| {
                let v = edge + 0.3 * (noise.unit() - 0.5) + 0.1 * c as f64 - 0.1;
                (v.clamp(0.0, 1.0) * 65535.0).round() as u16
            });
            [rgb[0], rgb[1], rgb[2], 65535]
        })
        .collect()
}

/// Opaque unrelated noise: a low no blur would give.
fn unrelated16(seed: u64) -> Vec<[u16; 4]> {
    let mut noise = Noise(seed);
    (0..W * H)
        .map(|_| {
            let rgb: [u16; 3] = std::array::from_fn(|_| (noise.next() % 65536) as u16);
            [rgb[0], rgb[1], rgb[2], 65535]
        })
        .collect()
}

fn free_handle(doc: *mut PrecisionDocument) -> PrecisionDocument {
    assert!(!doc.is_null());
    *unsafe { Box::from_raw(doc) }
}

fn promoted(doc: &PrecisionDocument) -> PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_promote_to_float(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    free_handle(out)
}

/// `doc` as a Gray Gamma 2.2 document at `depth` (32 promotes first).
fn grayed(doc: &PrecisionDocument, depth: u32) -> PrecisionDocument {
    let float;
    let (source, options) = if depth == 32 {
        float = promoted(doc);
        (&float, r#"{"space":"gray_gamma_22"}"#.to_string())
    } else {
        (
            doc,
            format!(r#"{{"space":"gray_gamma_22","depth":{depth}}}"#),
        )
    };
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_gray(
            source,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to gray: {}", take_err_string(err));
    free_handle(out)
}

fn labbed(doc: &PrecisionDocument, depth: u32) -> PrecisionDocument {
    let options = CString::new(format!(r#"{{"depth":{depth}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_lab(doc, options.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!out.is_null(), "to Lab: {}", take_err_string(err));
    free_handle(out)
}

fn linear_light(doc: &PrecisionDocument) -> PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_set_evaluation(doc, 1, &mut err) };
    assert!(!out.is_null(), "linear light: {}", take_err_string(err));
    free_handle(out)
}

/// Apply Image's Subtract at scale 2 and `offset` of `low` from `src`, over
/// `channels` planes of canvas codes (the typed `"kind":"blend"`).
fn plane_detail(src: &[Vec<u16>], low: &[Vec<u16>], offset: f64) -> Vec<Vec<u16>> {
    let n = (W * H) as usize;
    let channels = src.len();
    let mut input: Vec<u16> = Vec::with_capacity(2 * channels * n);
    for plane in src.iter().chain(low) {
        input.extend_from_slice(plane);
    }
    let request = text(
        json!({"kind":"blend","channels":channels,"mode":BLEND_SUBTRACT,
        "opacity":1,"scale":2,"offset":offset}),
    );
    let mut out = vec![0u16; channels * n];
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_selection_u16(
            W,
            H,
            request.as_ptr(),
            input.as_ptr(),
            input.len(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(ok, "{}", take_err_string(err));
    out.chunks(n).map(<[u16]>::to_vec).collect()
}

fn plane(pixels: &[[u16; 4]], c: usize) -> Vec<u16> {
    pixels.iter().map(|p| p[c]).collect()
}

/// The largest |a − b| over the colour channels.
fn worst16(a: &[[u16; 4]], b: &[[u16; 4]]) -> u16 {
    a.iter()
        .zip(b)
        .flat_map(|(p, q)| (0..3).map(move |c| p[c].abs_diff(q[c])))
        .max()
        .unwrap_or(0)
}

// ---- 32-bit float -------------------------------------------------------

/// Opaque linear samples from −0.5 to about 6: hard edges above white and
/// below black, with noise on a 1/4096 lattice.
fn float_picture(seed: u64) -> Vec<[f32; 4]> {
    let mut noise = Noise(seed);
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let edge = match (x / 6 + y / 4) % 3 {
                0 => -0.4,
                1 => 0.35,
                _ => 5.5,
            };
            let rgb: [f32; 3] =
                std::array::from_fn(|c| (edge + 0.5 * noise.unit() - 0.1 * c as f64) as f32);
            [rgb[0], rgb[1], rgb[2], 1.0]
        })
        .collect()
}

fn float_unrelated(seed: u64) -> Vec<[f32; 4]> {
    let mut noise = Noise(seed);
    (0..W * H)
        .map(|_| {
            let rgb: [f32; 3] = std::array::from_fn(|_| (7.0 * noise.unit() - 1.0) as f32);
            [rgb[0], rgb[1], rgb[2], 1.0]
        })
        .collect()
}

/// The float form's checks on a two-layer float document (low, source):
/// the detail is f32(t − s) exactly with negatives kept, the pair under
/// Linear Dodge (Add) rebuilds the source within the bound, and under Linear
/// Light it does not. Answers the worst error as a fraction of the bound.
fn float_case(doc: &PrecisionDocument, label: &str, gray: bool) -> f64 {
    let low = floats(doc, 0);
    let src = floats(doc, 1);
    let split = detail(doc)
        .unwrap()
        .unwrap_or_else(|| panic!("{label}: no detail"));
    let high = floats(&split, 1);
    let mut negatives = 0;
    for i in 0..src.len() {
        for c in 0..3 {
            let want = (f64::from(src[i][c]) - f64::from(low[i][c])) as f32;
            assert_eq!(
                high[i][c].to_bits(),
                want.to_bits(),
                "{label}: pixel {i} channel {c}: detail {} for {} − {}",
                high[i][c],
                src[i][c],
                low[i][c]
            );
            negatives += usize::from(high[i][c] < 0.0);
        }
        assert_eq!(
            high[i][3].to_bits(),
            src[i][3].to_bits(),
            "{label}: alpha kept"
        );
        if gray {
            assert!(
                high[i][0] == high[i][1] && high[i][1] == high[i][2],
                "{label}: R = G = B"
            );
        }
    }
    assert!(negatives > 0, "{label}: the detail holds negative samples");
    let out = render32(&paired(&split, BLEND_ADDITION));
    let mut worst: f64 = 0.0;
    for i in 0..src.len() {
        for c in 0..3 {
            let bound = 2f64.powi(-22) * f64::from(src[i][c]).abs().max(f64::from(low[i][c]).abs());
            let error = (f64::from(out[i][c]) - f64::from(src[i][c])).abs();
            assert!(
                error <= bound,
                "{label}: pixel {i} channel {c}: rebuilt {} for {} (bound {bound})",
                out[i][c],
                src[i][c]
            );
            if bound > 0.0 {
                worst = worst.max(error / bound);
            }
        }
        if gray {
            assert!(
                out[i][0] == out[i][1] && out[i][1] == out[i][2],
                "{label}: render R = G = B"
            );
        }
    }
    // The control: Linear Light's pivot is no part of the float form.
    let control = render32(&paired(&split, BLEND_LINEAR_LIGHT));
    let off = control
        .iter()
        .zip(&src)
        .flat_map(|(p, q)| (0..3).map(move |c| (f64::from(p[c]) - f64::from(q[c])).abs()))
        .fold(0.0, f64::max);
    assert!(
        off > 0.25,
        "{label}: under Linear Light the pair is only {off} off"
    );
    println!("{label}: worst {worst:.3} of the bound; under Linear Light {off:.3} off");
    worst
}

#[test]
fn float_detail_is_the_unclamped_difference_and_linear_dodge_rebuilds_the_layer() {
    let profile = IccProfile::srgb();
    let src = float_picture(0x9e37_79b9_7f4a_7c15);
    let unrelated = float_doc(W, H, &profile, &[float_unrelated(0x1234_5678), src.clone()]);
    float_case(&unrelated, "float unrelated", false);
    for sigma in [0.7, 2.0, 9.0] {
        let doc = blurred(
            &float_doc(W, H, &profile, &[src.clone(), src.clone()]),
            sigma,
        );
        float_case(&doc, &format!("float blur {sigma}"), false);
    }
}

#[test]
fn float_gray_detail_keeps_r_equal_g_equal_b() {
    let rgb = rgb16(
        &IccProfile::srgb(),
        vec![("Low", picture16(7)), ("Source", picture16(7))],
    );
    let doc = blurred(&grayed(&rgb, 32), 2.5);
    float_case(&doc, "gray 32 blur", true);
    let rgb = rgb16(
        &IccProfile::srgb(),
        vec![("Low", unrelated16(11)), ("Source", picture16(7))],
    );
    float_case(&grayed(&rgb, 32), "gray 32 unrelated", true);
}

// ---- Grayscale and Lab at 8 and 16 bits ---------------------------------

/// The encoded mode split: the detail of one stored plane per target by
/// Apply Image's plane blend at `offset`, written to layer 1 by each
/// target's `scalar_edit` `replace_canvas`, Linear Light, clipped; answers
/// the rendered worst error in u16 codes and the render.
fn mode_split(doc: &PrecisionDocument, targets: &[&str], offset: f64) -> (u16, Vec<[u16; 4]>) {
    let low = codes(doc, 0);
    let src = codes(doc, 1);
    let planes: Vec<usize> = (0..targets.len()).collect();
    let high = plane_detail(
        &planes.iter().map(|&c| plane(&src, c)).collect::<Vec<_>>(),
        &planes.iter().map(|&c| plane(&low, c)).collect::<Vec<_>>(),
        offset,
    );
    let mut current = doc.clone();
    for (target, samples) in targets.iter().zip(&high) {
        if let Some(next) = edit(
            &current,
            json!({"op":"scalar_edit","target":target,"index":1,"selection":false,
                "operation":{"kind":"replace_canvas"}}),
            samples,
        )
        .unwrap_or_else(|e| panic!("{target}: {e}"))
        {
            current = next;
        }
    }
    let out = render16(&paired(&current, BLEND_LINEAR_LIGHT));
    (worst16(&out, &src), out)
}

fn assert_mode_case(doc: &PrecisionDocument, targets: &[&str], label: &str) {
    let (error, out) = mode_split(doc, targets, 0.5);
    assert!(error <= 1, "{label}: recomposed within {error} codes");
    if targets.len() == 1 {
        assert!(
            out.iter().all(|p| p[0] == p[1] && p[1] == p[2]),
            "{label}: R = G = B"
        );
    }
    let (control, _) = mode_split(doc, targets, 32768.0 / 65535.0);
    assert_eq!(control, 2, "{label}: pivot 32768");
    println!("{label}: worst {error} codes; pivot 32768 {control}");
}

#[test]
fn gray_split_recomposes_within_one_code_at_8_and_16_bits() {
    let profile = IccProfile::srgb();
    for depth in [8, 16] {
        let same = rgb16(
            &profile,
            vec![("Low", picture16(3)), ("Source", picture16(3))],
        );
        for sigma in [1.0, 4.0] {
            let doc = blurred(&grayed(&same, depth), sigma);
            assert_mode_case(&doc, &["gray"], &format!("gray {depth} blur {sigma}"));
        }
        let other = rgb16(
            &profile,
            vec![("Low", unrelated16(5)), ("Source", picture16(3))],
        );
        assert_mode_case(
            &grayed(&other, depth),
            &["gray"],
            &format!("gray {depth} unrelated"),
        );
    }
}

/// Saturated sRGB colours over hard edges, so a* and b* span both signs.
fn colourful16(seed: u64) -> Vec<[u16; 4]> {
    const HUES: [[f64; 3]; 6] = [
        [0.9, 0.1, 0.1],
        [0.1, 0.8, 0.2],
        [0.15, 0.2, 0.9],
        [0.95, 0.9, 0.1],
        [0.8, 0.2, 0.85],
        [0.1, 0.85, 0.9],
    ];
    let mut noise = Noise(seed);
    (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            let hue = HUES[((x / 6 + y / 5) % 6) as usize];
            let rgb: [u16; 3] = std::array::from_fn(|c| {
                ((hue[c] + 0.2 * (noise.unit() - 0.5)).clamp(0.0, 1.0) * 65535.0).round() as u16
            });
            [rgb[0], rgb[1], rgb[2], 65535]
        })
        .collect()
}

#[test]
fn lab_split_recomposes_within_one_code_per_channel_at_8_and_16_bits() {
    let profile = IccProfile::srgb();
    for depth in [8, 16] {
        let same = rgb16(
            &profile,
            vec![("Low", colourful16(3)), ("Source", colourful16(3))],
        );
        let lab = labbed(&same, depth);
        let src = codes(&lab, 1);
        for c in [1, 2] {
            assert!(
                src.iter().any(|p| p[c] < 32768 - 4000),
                "Lab {depth}: channel {c} below 128"
            );
            assert!(
                src.iter().any(|p| p[c] > 32768 + 4000),
                "Lab {depth}: channel {c} above 128"
            );
        }
        for sigma in [1.0, 4.0] {
            let doc = blurred(&lab, sigma);
            assert_mode_case(
                &doc,
                &["red", "green", "blue"],
                &format!("Lab {depth} blur {sigma}"),
            );
        }
        let other = rgb16(
            &profile,
            vec![("Low", unrelated16(9)), ("Source", colourful16(3))],
        );
        assert_mode_case(
            &labbed(&other, depth),
            &["red", "green", "blue"],
            &format!("Lab {depth} unrelated"),
        );
    }
}

// ---- 16 bits evaluated in linear light -----------------------------------

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Adobe RGB (1998)'s published curve: a pure power of 563/256.
fn adobe_decode(v: f64) -> f64 {
    v.powf(563.0 / 256.0)
}

/// A 3×3 box mean of the codes: a smooth low.
fn box_low(src: &[[u16; 4]]) -> Vec<[u16; 4]> {
    (0..W * H)
        .map(|i| {
            let (x, y) = ((i % W) as i64, (i / W) as i64);
            let mut sum = [0u64; 3];
            let mut n = 0u64;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (sx, sy) = (x + dx, y + dy);
                    if sx < 0 || sy < 0 || sx >= W as i64 || sy >= H as i64 {
                        continue;
                    }
                    let p = src[(sy * W as i64 + sx) as usize];
                    for c in 0..3 {
                        sum[c] += u64::from(p[c]);
                    }
                    n += 1;
                }
            }
            let rgb: [u16; 3] = std::array::from_fn(|c| ((sum[c] + n / 2) / n) as u16);
            [rgb[0], rgb[1], rgb[2], 65535]
        })
        .collect()
}

/// `picture16` with a column of pure black and one of pure white.
fn with_extremes(mut pixels: Vec<[u16; 4]>) -> Vec<[u16; 4]> {
    for y in 0..H {
        pixels[(y * W + 3) as usize] = [0, 0, 0, 65535];
        pixels[(y * W + 4) as usize] = [65535, 65535, 65535, 65535];
    }
    pixels
}

/// The decoded form's checks (module doc) on `profile`'s document of
/// (low, src) in linear light. Answers (worst decoded, worst stored).
fn linear_case(
    profile: &Arc<IccProfile>,
    decode: fn(f64) -> f64,
    low: Vec<[u16; 4]>,
    src: Vec<[u16; 4]>,
    stored_bound: u16,
    label: &str,
) -> (f64, u16) {
    let doc = linear_light(&rgb16(
        profile,
        vec![("Low", low.clone()), ("Source", src.clone())],
    ));
    let split = detail(&doc)
        .unwrap()
        .unwrap_or_else(|| panic!("{label}: no detail"));
    assert_eq!(codes(&split, 0), low, "{label}: the low layer is untouched");
    let out = render16(&paired(&split, BLEND_LINEAR_LIGHT));
    let mut decoded: f64 = 0.0;
    for (i, p) in out.iter().enumerate() {
        for c in 0..3 {
            let error =
                (decode(f64::from(p[c]) / 65535.0) - decode(f64::from(src[i][c]) / 65535.0)).abs()
                    * 65535.0;
            assert!(
                error <= 4.0,
                "{label}: pixel {i} channel {c}: {} for {}: {error} decoded codes",
                p[c],
                src[i][c]
            );
            decoded = decoded.max(error);
        }
        assert_eq!(p[3], 65535, "{label}: opaque");
    }
    let stored = worst16(&out, &src);
    assert!(stored <= stored_bound, "{label}: {stored} stored codes");
    // The control: Apply Image's stored-code detail, which the same
    // compositor does not undo in linear light.
    let high: Vec<[u16; 4]> = src
        .iter()
        .zip(&low)
        .map(|(t, s)| {
            let rgb: [u16; 3] = std::array::from_fn(|c| {
                ((f64::from(t[c]) - f64::from(s[c]) + 65535.0) / 2.0).round() as u16
            });
            [rgb[0], rgb[1], rgb[2], 65535]
        })
        .collect();
    let control = linear_light(&rgb16(profile, vec![("Low", low), ("High", high)]));
    let off = worst16(&render16(&paired(&control, BLEND_LINEAR_LIGHT)), &src);
    assert!(
        off > 1000,
        "{label}: the stored-code detail is only {off} off"
    );
    println!(
        "{label}: worst {decoded:.3} decoded codes, {stored} stored; stored-code detail {off} off"
    );
    (decoded, stored)
}

#[test]
fn linear_light_detail_recomposes_within_its_decoded_bound() {
    for (profile, decode, stored_bound, name) in [
        (
            IccProfile::srgb(),
            srgb_decode as fn(f64) -> f64,
            24,
            "sRGB",
        ),
        (
            IccProfile::adobe_rgb(),
            adobe_decode as fn(f64) -> f64,
            600,
            "Adobe RGB",
        ),
    ] {
        let src = with_extremes(picture16(21));
        linear_case(
            &profile,
            decode,
            box_low(&src),
            src.clone(),
            stored_bound,
            &format!("{name} box"),
        );
        linear_case(
            &profile,
            decode,
            unrelated16(23),
            src,
            stored_bound,
            &format!("{name} unrelated"),
        );
    }
}

// ---- the gates -----------------------------------------------------------

#[test]
fn layer_detail_refuses_every_other_document_and_request() {
    let profile = IccProfile::srgb();
    let encoded = rgb16(
        &profile,
        vec![("Low", picture16(1)), ("Source", picture16(2))],
    );
    let gray = grayed(&encoded, 16);
    let lab = labbed(&encoded, 16);
    for (doc, what) in [
        (&encoded, "encoded 16-bit RGB"),
        (&gray, "16-bit Gray"),
        (&lab, "Lab"),
    ] {
        let refusal = refused(detail(doc), what);
        assert!(
            refusal.starts_with("layer_detail takes a 32-bit float document"),
            "{what}: {refusal}"
        );
    }
    let float = float_doc(
        W,
        H,
        &profile,
        &[float_picture(1), float_picture(2), float_picture(3)],
    );
    let refuse = |request: Value, needle: &str| {
        let refusal = refused(edit(&float, request.clone(), &[]), &request.to_string());
        assert!(refusal.contains(needle), "{request}: {refusal}");
    };
    refuse(
        json!({"op":"layer_detail","version":1,"layer":1,"source":1}),
        "another layer",
    );
    refuse(
        json!({"op":"layer_detail","version":1,"layer":5,"source":0}),
        "layer index is out of range",
    );
    refuse(
        json!({"op":"layer_detail","version":1,"layer":1,"source":9}),
        "source index is out of range",
    );
    refuse(
        json!({"op":"layer_detail","version":2,"layer":1,"source":0}),
        "version",
    );
    refuse(
        json!({"op":"layer_detail","version":1,"layer":1,"source":0,"mode":1}),
        "unknown field",
    );
    let grouped = then(
        &float,
        json!({"op":"group_layers","layers":[0],"name":"Group"}),
        &[],
    );
    let group = (0..grouped.layer_count())
        .find(|&i| grouped.layer(i).unwrap().children().is_some())
        .unwrap();
    let refusal = refused(
        edit(
            &grouped,
            json!({"op":"layer_detail","version":1,"layer":group,"source":group + 1}),
            &[],
        ),
        "a group",
    );
    assert!(refusal.contains("has no pixels of its own"), "{refusal}");
    let locked = then(&float, json!({"op":"layer_lock","layer":1,"locks":2}), &[]);
    refused(detail(&locked), "a pixel-locked layer");
    // A source of zeros leaves every difference as it was: no document.
    let zero = vec![[0.0, 0.0, 0.0, 1.0]; (W * H) as usize];
    let unchanged = float_doc(W, H, &profile, &[zero, float_picture(4)]);
    assert!(
        detail(&unchanged).unwrap().is_none(),
        "a zero source changes nothing"
    );
    // The family table admits it where the op answers.
    for depth in [16, 32] {
        assert_eq!(
            capability("layer_detail", "raster", depth)["supported"],
            true,
            "{depth}"
        );
    }
    assert!(require_gray("layer_detail", "raster").is_ok());
}
