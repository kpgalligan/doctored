//! Editing a CMYK document in ink (#547 phase 2): the Brush/Fill
//! (`paint_blend`), Gradient, Paint Bucket and Eraser through
//! `dr_precision_doc_edit`, each against this file's own straight CMYKA
//! source-over at a coverage, frozen-alpha over, erase and flood written
//! from the module docs' formulas (never read off the implementation); New
//! Layer; layer masks through the composite; the one-colour conversion
//! `dr_cmyk_color` against `common/cmyk_fixture.rs`'s closed-form oracle;
//! and every new refusal by its sentence. No test here sets or clears the
//! process working CMYK profile.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::{
    dr_cmyk_color, dr_precision_doc_cmyk_sample, dr_precision_doc_convert_to_cmyk,
};
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::ffi_precision_edit::{
    dr_precision_doc_edit, dr_precision_doc_edit_plan, dr_precision_doc_read_plane,
};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;

use crate::cmyk_fixture::{ink_to_srgb, press_profile, srgb_to_ink_bpc};
use crate::common::{classic_ramp, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

type Ink = [f64; 5];

fn free(document: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(document) };
}

/// A `w × h` CMYK document converted from u16 sRGB `pixels` (alpha kept)
/// into the synthetic press profile.
fn ink_doc(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    ink_doc_with(w, h, pixels, c"{}")
}

/// [`ink_doc`] with the conversion's `options` (`{"depth":8}`).
fn ink_doc_with(
    w: u32,
    h: u32,
    pixels: Vec<[u16; 4]>,
    options: &std::ffi::CStr,
) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    let rgb = Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ));
    let profile = press_profile("Press Test");
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

/// A varied picture with every alpha class: opaque, partial and zero.
fn varied(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let alpha = match i % 4 {
                0 => 65535,
                1 => 40000,
                2 => 0,
                _ => 12000,
            };
            [
                (i * 7919 % 65536) as u16,
                (i * 104_729 % 65536) as u16,
                (i * 31_337 % 65536) as u16,
                alpha,
            ]
        })
        .collect()
}

fn edit(
    doc: *const PrecisionDocument,
    request: &str,
    samples: &[u16],
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        if err.is_null() {
            Ok(ptr::null_mut())
        } else {
            Err(take_err_string(err))
        }
    } else {
        Ok(out)
    }
}

fn plan(doc: *const PrecisionDocument, request: &str) -> Result<Value, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

fn samples(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    unsafe { &*doc }
        .cmyk_layer_samples(index)
        .expect("a CMYK record")
}

fn unit(s: [u16; 5]) -> Ink {
    s.map(|v| f64::from(v) / 65535.0)
}

fn code(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn codes(p: Ink) -> [u16; 5] {
    p.map(code)
}

/// Normal straight CMYKA over at effective alpha `sa` (the W3C source-over
/// on the four inks).
fn over(dst: Ink, src: [f64; 4], sa: f64) -> Ink {
    if sa <= 0.0 {
        return dst;
    }
    let ab = dst[4];
    let ao = sa + ab * (1.0 - sa);
    let mut out = [0.0; 5];
    for c in 0..4 {
        out[c] = (sa * src[c] + (1.0 - sa) * ab * dst[c]) / ao;
    }
    out[4] = ao;
    out
}

/// The over with the backdrop's alpha frozen: transparent stays, opaque is
/// the plain over, partial mixes `lerp(Cb, Cs, sa)` at its own alpha.
fn frozen(dst: Ink, src: [f64; 4], sa: f64) -> Ink {
    if dst[4] <= 0.0 || sa <= 0.0 {
        return dst;
    }
    if dst[4] >= 1.0 {
        return over(dst, src, sa);
    }
    let mut out = dst;
    for c in 0..4 {
        out[c] = dst[c] + (src[c] - dst[c]) * sa;
    }
    out
}

fn assert_close(got: &[[u16; 5]], want: &[[u16; 5]], what: &str) {
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..5 {
            assert!(
                g[c].abs_diff(w[c]) <= 1,
                "{what}: pixel {i} component {c}: {} vs oracle {} ({g:?} vs {w:?})",
                g[c],
                w[c]
            );
        }
    }
}

/// A coverage plane with zeros, partial and full values.
fn coverage(n: usize, seed: u64) -> Vec<u16> {
    (0..n as u64)
        .map(|i| match (i * 31 + seed) % 5 {
            0 => 0,
            1 => 65535,
            k => ((i * 2654435761 + seed * 977) % 65536) as u16 / (k as u16),
        })
        .collect()
}

#[test]
fn paint_blend_is_straight_ink_over_at_stroke_and_selection_coverage() {
    let (w, h) = (6u32, 5u32);
    let n = (w * h) as usize;
    let doc = ink_doc(w, h, varied(w, h));
    let before = samples(doc, 0);
    let ink = [0.2, 0.45, 0.7, 0.1, 0.8];
    let opacity = 0.65;
    let stroke = coverage(n, 3);
    let selection = coverage(n, 11);
    let mut input = stroke.clone();
    input.extend_from_slice(&selection);
    let request = format!(
        r#"{{"op":"paint_blend","layer":0,"ink":{ink:?},"opacity":{opacity},"selection":true}}"#
    );
    let p = plan(doc, &request).unwrap();
    assert_eq!(p["input_samples"], n * 2);
    assert_eq!(p["input_planes"], 2);
    let out = edit(doc, &request, &input).unwrap();
    let want: Vec<[u16; 5]> = before
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let cover = f64::from(stroke[i]) / 65535.0 * f64::from(selection[i]) / 65535.0;
            let sa = ink[4] * opacity * cover;
            let result = codes(over(unit(*s), [ink[0], ink[1], ink[2], ink[3]], sa));
            if *s == [0; 5] && result[4] == 0 {
                *s
            } else {
                result
            }
        })
        .collect();
    assert_close(&samples(out, 0), &want, "paint_blend");
    // Outside every coverage the samples are untouched, bit for bit.
    for (i, (a, b)) in before.iter().zip(samples(out, 0)).enumerate() {
        if stroke[i] == 0 || selection[i] == 0 {
            assert_eq!(*a, b, "pixel {i} outside the coverage");
        }
    }
    // Opacity 0 and an all-zero stroke are exact no-ops.
    let none = format!(r#"{{"op":"paint_blend","layer":0,"ink":{ink:?},"opacity":0}}"#);
    assert!(edit(doc, &none, &stroke).unwrap().is_null());
    let zero = vec![0u16; n];
    let full = format!(r#"{{"op":"paint_blend","layer":0,"ink":{ink:?},"opacity":1}}"#);
    assert!(edit(doc, &full, &zero).unwrap().is_null());
    free(out);
    free(doc);
}

#[test]
fn preserve_transparency_and_the_transparency_lock_freeze_alpha() {
    let (w, h) = (4u32, 4u32);
    let n = (w * h) as usize;
    let doc = ink_doc(w, h, varied(w, h));
    let before = samples(doc, 0);
    let ink = [0.9, 0.1, 0.3, 0.5, 1.0];
    let stroke = coverage(n, 5);
    let want: Vec<[u16; 5]> = before
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let sa = 0.7 * f64::from(stroke[i]) / 65535.0;
            let mut out = codes(frozen(unit(*s), [ink[0], ink[1], ink[2], ink[3]], sa));
            out[4] = s[4];
            out
        })
        .collect();
    let preserve = format!(
        r#"{{"op":"paint_blend","layer":0,"ink":{ink:?},"opacity":0.7,"preserve_alpha":true}}"#
    );
    let painted = edit(doc, &preserve, &stroke).unwrap();
    assert_close(&samples(painted, 0), &want, "preserve transparency");
    for (a, b) in before.iter().zip(samples(painted, 0)) {
        assert_eq!(a[4], b[4], "alpha is frozen exactly");
    }
    // The transparency lock is the same rule, and an Eraser stroke on a
    // transparency-locked layer erases nothing.
    let locked = edit(doc, r#"{"op":"layer_lock","layer":0,"locks":1}"#, &[]).unwrap();
    let plain = format!(r#"{{"op":"paint_blend","layer":0,"ink":{ink:?},"opacity":0.7}}"#);
    let painted_locked = edit(locked, &plain, &stroke).unwrap();
    assert_eq!(samples(painted_locked, 0), samples(painted, 0));
    let erase = r#"{"op":"erase_native","layer":0,"opacity":1}"#;
    assert!(edit(locked, erase, &stroke).unwrap().is_null());
    // Lock Pixels refuses by its own sentence.
    let pixels = edit(doc, r#"{"op":"layer_lock","layer":0,"locks":2}"#, &[]).unwrap();
    assert!(edit(pixels, &plain, &stroke).is_err());
    for d in [painted, locked, painted_locked, pixels, doc] {
        free(d);
    }
}

#[test]
fn the_eraser_lowers_alpha_and_keeps_the_ink() {
    let (w, h) = (5u32, 3u32);
    let n = (w * h) as usize;
    let doc = ink_doc(w, h, varied(w, h));
    let before = samples(doc, 0);
    let stroke = coverage(n, 7);
    let out = edit(
        doc,
        r#"{"op":"erase_native","layer":0,"opacity":0.6}"#,
        &stroke,
    )
    .unwrap();
    for (i, (s, g)) in before.iter().zip(samples(out, 0)).enumerate() {
        let alpha = f64::from(s[4]) * (1.0 - f64::from(stroke[i]) / 65535.0 * 0.6);
        assert!(
            g[4].abs_diff(alpha.round() as u16) <= 1,
            "pixel {i}: {} vs {alpha}",
            g[4]
        );
        assert_eq!(&g[..4], &s[..4], "the ink is untouched");
    }
    free(out);
    free(doc);
}

#[test]
fn a_gradient_lerps_its_stops_along_the_drag() {
    let (w, h) = (9u32, 4u32);
    let n = (w * h) as usize;
    let doc = ink_doc(w, h, varied(w, h));
    let before = samples(doc, 0);
    let stops = [
        (0.0, [0.0, 0.2, 0.9, 0.0, 1.0]),
        (0.4, [0.5, 0.5, 0.0, 0.3, 0.5]),
        (1.0, [1.0, 0.0, 0.1, 0.8, 0.9]),
    ];
    let (from, to): ([f64; 2], [f64; 2]) = ([1.0, 0.5], [8.0, 3.5]);
    let stop_json: Vec<String> = stops
        .iter()
        .map(|(p, ink)| format!(r#"{{"position":{p},"ink":{ink:?}}}"#))
        .collect();
    let stroke = vec![65535u16; n];
    for style in [None, Some("radial")] {
        let style_key = style.map_or(String::new(), |s| format!(r#","style":"{s}""#));
        let request = format!(
            r#"{{"op":"gradient","layer":0,"opacity":0.9,"from":{from:?},"to":{to:?},"stops":[{}]{style_key}}}"#,
            stop_json.join(",")
        );
        let out = edit(doc, &request, &stroke).unwrap();
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        let length = (dx * dx + dy * dy).sqrt();
        let want: Vec<[u16; 5]> = before
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let (px, py) = ((i as u32 % w) as f64 + 0.5, (i as u32 / w) as f64 + 0.5);
                let t = match style {
                    None => ((px - from[0]) * dx + (py - from[1]) * dy) / (length * length),
                    _ => ((px - from[0]).powi(2) + (py - from[1]).powi(2)).sqrt() / length,
                }
                .clamp(0.0, 1.0);
                let k = stops.iter().rposition(|(p, _)| *p <= t).unwrap_or(0);
                let mut ink: Ink = if k + 1 >= stops.len() {
                    stops[k].1
                } else {
                    let (a, b) = (stops[k], stops[k + 1]);
                    let f = (t - a.0) / (b.0 - a.0);
                    std::array::from_fn(|c| a.1[c] + (b.1[c] - a.1[c]) * f)
                };
                if matches!(style, None | Some("radial")) {
                    // A Linear or Radial drag's four inks follow the Classic
                    // ramp; alpha stays linear.
                    for (c, v) in ink.iter_mut().take(4).enumerate() {
                        let channel: Vec<(f64, f64)> =
                            stops.iter().map(|(p, s)| (*p, s[c])).collect();
                        *v = classic_ramp(&channel, t);
                    }
                }
                let result = codes(over(
                    unit(*s),
                    [ink[0], ink[1], ink[2], ink[3]],
                    ink[4] * 0.9,
                ));
                if *s == [0; 5] && result[4] == 0 {
                    *s
                } else {
                    result
                }
            })
            .collect();
        assert_close(&samples(out, 0), &want, &format!("gradient {style:?}"));
        free(out);
    }
    free(doc);
}

/// The shipped ink projections (`selection_region`'s module doc, **Ink**,
/// PN7), written out here: the ink's naive RGB in 16-bit codes — rounded
/// through 8-bit codes on an 8-bit document — then R, G, B, A, R−G, G−B,
/// R−B.
fn ink_projections(p: [u16; 5], bits: u16) -> [i64; 7] {
    let [c, m, y, k, a] = p.map(f64::from);
    let plate = |v: f64| -> i64 {
        if bits == 8 {
            let (v8, k8) = ((v / 257.0).round(), (k / 257.0).round());
            257 * ((255.0 - v8) * (255.0 - k8) / 255.0).round() as i64
        } else {
            ((65535.0 - v) * (65535.0 - k) / 65535.0).round() as i64
        }
    };
    let (r, g, b) = (plate(c), plate(m), plate(y));
    [r, g, b, a as i64, r - g, g - b, r - b]
}

/// The test's own four-connected flood: every projection within ±
/// `tolerance` of the seed's.
fn flood(
    (w, h): (u32, u32),
    pixels: &[[u16; 5]],
    seed: usize,
    tolerance: u16,
    contiguous: bool,
    bits: u16,
) -> Vec<bool> {
    let reference = ink_projections(pixels[seed], bits);
    let near = |i: usize| {
        ink_projections(pixels[i], bits)
            .iter()
            .zip(reference)
            .all(|(a, b)| (a - b).abs() <= i64::from(tolerance))
    };
    if !contiguous {
        return (0..pixels.len()).map(near).collect();
    }
    let mut out = vec![false; pixels.len()];
    let mut stack = vec![seed];
    out[seed] = true;
    while let Some(i) = stack.pop() {
        let (x, y) = ((i as u32 % w) as i64, (i as u32 / w) as i64);
        for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
            if nx < 0 || ny < 0 || nx >= i64::from(w) || ny >= i64::from(h) {
                continue;
            }
            let j = (ny * i64::from(w) + nx) as usize;
            if !out[j] && near(j) {
                out[j] = true;
                stack.push(j);
            }
        }
    }
    out
}

#[test]
fn the_paint_bucket_fills_the_flood_from_its_seed() {
    let (w, h) = (6u32, 5u32);
    let n = (w * h) as usize;
    // Two inks in a pattern: a left block and a right block, joined by one
    // row, with an island of the left ink on the right.
    let a = [60000u16, 2000, 9000, 65535];
    let b = [1000u16, 50000, 3000, 65535];
    let pixels: Vec<[u16; 4]> = (0..n)
        .map(|i| {
            let (x, y) = (i as u32 % w, i as u32 / w);
            if (x < 3 && y != 4) || (x == 5 && y == 1) {
                a
            } else {
                b
            }
        })
        .collect();
    let ink = [0.1, 0.2, 0.3, 0.4, 1.0];
    let stroke = vec![65535u16; n];
    for bits in [8u16, 16] {
        let options = CString::new(format!(r#"{{"depth":{bits}}}"#)).unwrap();
        let doc = ink_doc_with(w, h, pixels.clone(), &options);
        let before = samples(doc, 0);
        for (contiguous, tolerance) in [(true, 0u16), (false, 0), (true, 65535)] {
            let request = format!(
                r#"{{"op":"bucket_fill","layer":0,"ink":{ink:?},"opacity":1,"x":0,"y":0,"tolerance":{tolerance},"contiguous":{contiguous}}}"#
            );
            let out = edit(doc, &request, &stroke).unwrap();
            let region = flood((w, h), &before, 0, tolerance, contiguous, bits);
            let want: Vec<[u16; 5]> = before
                .iter()
                .zip(&region)
                .map(|(s, inside)| {
                    if *inside {
                        codes(over(unit(*s), [ink[0], ink[1], ink[2], ink[3]], 1.0))
                    } else {
                        *s
                    }
                })
                .collect();
            assert_close(
                &samples(out, 0),
                &want,
                &format!("bucket contiguous {contiguous} tolerance {tolerance}"),
            );
            free(out);
        }
        // A seed outside the canvas refuses; a seed outside the selection is a no-op.
        let outside = format!(
            r#"{{"op":"bucket_fill","layer":0,"ink":{ink:?},"opacity":1,"x":9,"y":0,"tolerance":0,"contiguous":true}}"#
        );
        assert_eq!(
            edit(doc, &outside, &stroke).unwrap_err(),
            "native bucket seed is outside canvas"
        );
        let mut input = stroke.clone();
        input.extend(std::iter::repeat_n(0u16, n));
        let unselected = format!(
            r#"{{"op":"bucket_fill","layer":0,"ink":{ink:?},"opacity":1,"x":0,"y":0,"tolerance":0,"contiguous":true,"selection":true}}"#
        );
        assert!(edit(doc, &unselected, &input).unwrap().is_null());
        free(doc);
    }
}

#[test]
fn new_layer_is_bare_paper_and_takes_ink() {
    let (w, h) = (4u32, 3u32);
    let n = (w * h) as usize;
    let doc = ink_doc(w, h, varied(w, h));
    let p = plan(
        doc,
        r#"{"op":"add_layer","name":"Ink","anchor":0,"position":"above"}"#,
    )
    .unwrap();
    assert!(p["scratch_bytes"].as_u64().unwrap() >= u64::from(w * h) * 10);
    let added = edit(
        doc,
        r#"{"op":"add_layer","name":"Ink","anchor":0,"position":"above"}"#,
        &[],
    )
    .unwrap();
    assert_eq!(unsafe { &*added }.layer_count(), 2);
    assert!(samples(added, 1).iter().all(|s| *s == [0; 5]));
    assert_eq!(unsafe { &*added }.layer(1).unwrap().name(), "Ink");
    let ink = [0.0, 0.0, 0.0, 1.0, 1.0];
    let stroke = coverage(n, 2);
    let request = format!(r#"{{"op":"paint_blend","layer":1,"ink":{ink:?},"opacity":1}}"#);
    let painted = edit(added, &request, &stroke).unwrap();
    for (i, s) in samples(painted, 1).iter().enumerate() {
        let sa = f64::from(stroke[i]) / 65535.0;
        let want = codes(over([0.0; 5], [0.0, 0.0, 0.0, 1.0], sa));
        let want = if want[4] == 0 { [0; 5] } else { want };
        for c in 0..5 {
            assert!(s[c].abs_diff(want[c]) <= 1, "pixel {i}: {s:?} vs {want:?}");
        }
    }
    assert_eq!(
        samples(painted, 0),
        samples(doc, 0),
        "the layer below is untouched"
    );
    for d in [added, painted, doc] {
        free(d);
    }
}

fn sample_ink(doc: *const PrecisionDocument, x: i32, y: i32) -> [u16; 5] {
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_cmyk_sample(doc, x, y, 0, LIMIT, 0, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    let ink: Vec<u16> = value["ink16"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u16)
        .collect();
    [
        ink[0],
        ink[1],
        ink[2],
        ink[3],
        value["alpha16"].as_u64().unwrap() as u16,
    ]
}

#[test]
fn a_layer_mask_scales_the_layers_coverage_in_the_composite() {
    let (w, h) = (4u32, 3u32);
    let n = (w * h) as usize;
    let bottom: Vec<[u16; 4]> = (0..n)
        .map(|i| [(i * 4000) as u16, 30000, 5000, 65535])
        .collect();
    let doc = ink_doc(w, h, bottom);
    let added = edit(
        doc,
        r#"{"op":"add_layer","name":"Top","anchor":0,"position":"above"}"#,
        &[],
    )
    .unwrap();
    let ink = [0.8, 0.1, 0.2, 0.3, 1.0];
    let full = vec![65535u16; n];
    let request = format!(r#"{{"op":"paint_blend","layer":1,"ink":{ink:?},"opacity":1}}"#);
    let painted = edit(added, &request, &full).unwrap();
    let opaque = unsafe {
        doctored_core::ffi_precision::dr_precision_doc_set_layer_opacity(
            painted,
            1,
            0.75,
            ptr::null_mut(),
        )
    };
    assert!(!opaque.is_null());
    // A mask with every class of coverage, offset one pixel right: the
    // column left of it reads the outside coverage (0).
    let (mw, mh) = (3u32, 3u32);
    let plane: Vec<u16> = (0..mw * mh).map(|i| (i * 8191) as u16).collect();
    let set = format!(r#"{{"op":"set_mask","layer":1,"width":{mw},"height":{mh},"x":1,"y":0}}"#);
    let masked = edit(opaque, &set, &plane).unwrap();
    let low = samples(masked, 0);
    for y in 0..h {
        for x in 0..w {
            let m = if x >= 1 {
                f64::from(plane[(y * mw + x - 1) as usize]) / 65535.0
            } else {
                0.0
            };
            let i = (y * w + x) as usize;
            let want = codes(over(
                unit(low[i]),
                [ink[0], ink[1], ink[2], ink[3]],
                // The layer's 75 % at its byte (#578).
                crate::common::byte_unit(0.75) * m,
            ));
            let got = sample_ink(masked, x as i32, y as i32);
            for c in 0..5 {
                assert!(
                    got[c].abs_diff(want[c]) <= 1,
                    "({x},{y}): {got:?} vs {want:?}"
                );
            }
        }
    }
    // The raw mask plane reads back verbatim; disabling it shows the layer whole.
    let read = CString::new(r#"{"op":"read_plane","target":"mask","index":1}"#).unwrap();
    let mut out = vec![0u16; (mw * mh) as usize];
    let mut err = ptr::null_mut();
    assert!(
        unsafe {
            dr_precision_doc_read_plane(
                masked,
                read.as_ptr(),
                out.as_mut_ptr(),
                out.len(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    assert_eq!(out, plane);
    let disabled = edit(
        masked,
        r#"{"op":"mask_properties","layer":1,"x":1,"y":0,"linked":true,"density":1,"feather":0,"enabled":false}"#,
        &[],
    )
    .unwrap();
    let want = codes(over(
        unit(low[0]),
        [ink[0], ink[1], ink[2], ink[3]],
        crate::common::byte_unit(0.75),
    ));
    let got = sample_ink(disabled, 0, 0);
    for c in 0..5 {
        assert!(got[c].abs_diff(want[c]) <= 1, "{got:?} vs {want:?}");
    }
    let removed = edit(masked, r#"{"op":"remove_mask","layer":1}"#, &[]).unwrap();
    assert!(unsafe { &*removed }.layer(1).unwrap().mask().is_none());
    // Apply Layer Mask bakes the mask into alpha, the ink bit for bit.
    let applied = edit(masked, r#"{"op":"apply_mask","layer":1}"#, &[]).unwrap();
    assert!(unsafe { &*applied }.layer(1).unwrap().mask().is_none());
    let before = samples(masked, 1);
    for (i, (b, a)) in before.iter().zip(samples(applied, 1)).enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let m = if x >= 1 {
            f64::from(plane[(y * mw + x - 1) as usize])
        } else {
            0.0
        };
        assert_eq!(&a[..4], &b[..4], "pixel {i}: the ink is untouched");
        assert_eq!(
            a[4],
            (f64::from(b[4]) * m / 65535.0).round() as u16,
            "pixel {i}"
        );
    }
    for d in [
        added, painted, opaque, masked, disabled, removed, applied, doc,
    ] {
        free(d);
    }
}

#[test]
fn every_new_refusal_names_itself() {
    let (w, h) = (3u32, 2u32);
    let n = (w * h) as usize;
    let doc = ink_doc(w, h, varied(w, h));
    let stroke = vec![65535u16; n];
    let cases = [
        (
            r#"{"op":"paint_blend","layer":0,"ink":[0,0,0,1,1],"opacity":1,"blend_mode":27}"#,
            "The Pass Through blend mode is unavailable in a CMYK document",
        ),
        (
            r#"{"op":"paint_blend","layer":0,"ink":[0,0,0,1,1],"opacity":1,"plane":"red"}"#,
            "Painting one colour channel is unavailable in a CMYK document",
        ),
        (
            r#"{"op":"paint_blend","layer":0,"color":[0,0,0,1],"color_space":"srgb","opacity":1,"blend_mode":0}"#,
            "a CMYK document paints in ink: pass ink (or gradient stops with ink) as [c, m, y, k, alpha] in 0…1, not an RGB color",
        ),
        (
            r#"{"op":"paint_blend","layer":0,"ink":[0,0,0,1.5,1],"opacity":1}"#,
            "CMYK ink must be finite in [0,1]",
        ),
        (
            r#"{"op":"gradient","layer":0,"opacity":1,"from":[0,0],"to":[1,1],"stops":[{"position":0,"ink":[0,0,0,1,1]}]}"#,
            "native gradient stops must have 2 to 32 entries",
        ),
        (
            r#"{"op":"clone","layer":0}"#,
            "The \"clone\" edit is unavailable in a CMYK document",
        ),
        (
            r#"{"op":"scalar_edit","target":"red","index":0,"operation":{"kind":"fill","value":0}}"#,
            "The \"scalar_edit\" edit of anything but a layer mask, an alpha channel or an ink plate is unavailable in a CMYK document",
        ),
        (
            r#"{"op":"read_plane","target":"red","index":0}"#,
            "The \"read_plane\" edit of anything but a layer mask, an alpha channel or an ink plate is unavailable in a CMYK document",
        ),
    ];
    for (request, sentence) in cases {
        assert_eq!(
            edit(doc, request, &stroke).unwrap_err(),
            sentence,
            "{request}"
        );
    }
    free(doc);
}

fn color(doc: *const PrecisionDocument, request: &str) -> Result<Value, String> {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_cmyk_color(doc, text.as_ptr(), &mut err) };
    if out.is_null() {
        return Err(take_err_string(err));
    }
    let value = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    Ok(value)
}

#[test]
fn one_colour_crosses_the_boundary_through_the_documents_profile() {
    let doc = ink_doc(1, 1, vec![[0, 0, 0, 65535]]);
    let to16 = |v: &Value| (v.as_f64().unwrap() * 65535.0).round() as u16;
    for bpc in [false, true] {
        for rgb in [
            [0.2, 0.5, 0.8],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
            [0.9, 0.1, 0.3],
        ] {
            let reply = color(
                doc,
                &format!(r#"{{"from":"srgb","values":{rgb:?},"black_point_compensation":{bpc}}}"#),
            )
            .unwrap();
            assert_eq!(reply["profile"], "Press Test");
            let want = srgb_to_ink_bpc(rgb, bpc);
            for c in 0..4 {
                let got = to16(&reply["ink"][c]);
                assert!(
                    got.abs_diff(code(want[c])) <= 1,
                    "bpc {bpc} {rgb:?} ink {c}: {got} vs {}",
                    code(want[c])
                );
            }
        }
    }
    for bpc in [false, true] {
        for ink in [
            [0.0; 4],
            [1.0; 4],
            [0.3, 0.6, 0.1, 0.2],
            [0.0, 0.0, 0.0, 0.7],
        ] {
            let reply = color(
                doc,
                &format!(r#"{{"from":"ink","values":{ink:?},"black_point_compensation":{bpc}}}"#),
            )
            .unwrap();
            let want = ink_to_srgb(ink, bpc);
            for c in 0..3 {
                let got = to16(&reply["srgb"][c]);
                assert!(
                    got.abs_diff(code(want[c])) <= 1,
                    "bpc {bpc} {ink:?} channel {c}: {got} vs {}",
                    code(want[c])
                );
            }
        }
    }
    // With no CMYK document and no working profile the conversion refuses.
    assert_eq!(
        color(ptr::null(), r#"{"from":"srgb","values":[0,0,0]}"#).unwrap_err(),
        "no working CMYK profile is set, so a colour cannot be converted to or from CMYK ink"
    );
    assert_eq!(
        color(doc, r#"{"from":"lab","values":[0,0,0]}"#).unwrap_err(),
        "a CMYK colour request is {\"from\":\"srgb\",\"values\":[r,g,b]} or {\"from\":\"ink\",\"values\":[c,m,y,k]}"
    );
    assert_eq!(
        color(doc, r#"{"from":"ink","values":[0,0,0,0],"dither":1}"#).unwrap_err(),
        "CMYK colour request has an unknown key \"dither\""
    );
    free(doc);
}
