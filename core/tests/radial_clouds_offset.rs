//! Radial Blur, Clouds / Difference Clouds and Offset at the
//! C boundary and through the 16-bit selected-edit and scalar paths. Every
//! expectation is an independent oracle: a direct binary64 path mean with its
//! own bilinear read, a direct source mapping for each Offset edge, and the
//! clouds' contract (determinism, range, the difference law, the contrast
//! spread). The frame — the selection's bounding box in plane pixels — is
//! checked at both depths, including under a layer offset.
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::dr_image_filter_op;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionPlane};
use doctored_core::selected_adjustment::SelectedEdit;
use doctored_core::DrImage;
use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::ptr;

use crate::gaussian_reference;

fn image(w: u32, h: u32, pixels: &[u8]) -> *mut DrImage {
    let p = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!p.is_null());
    p
}
fn samples(image: *const DrImage, count: usize) -> Vec<u8> {
    let p = unsafe { dr_image_pixels_rgba(image) };
    assert!(!p.is_null());
    unsafe { std::slice::from_raw_parts(p, count * 4) }.to_vec()
}
/// `Ok(null)` is a NULL result with no error.
fn raw_filter(img: *const DrImage, op: &str, params: &str) -> Result<*mut DrImage, String> {
    let op = CString::new(op).unwrap();
    let params = CString::new(params).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), ptr::null(), &mut err) };
    if err.is_null() {
        Ok(out)
    } else {
        assert!(out.is_null());
        let message = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        Err(message)
    }
}
fn filter(w: u32, h: u32, pixels: &[u8], op: &str, params: Value) -> Vec<u8> {
    let img = image(w, h, pixels);
    let out = raw_filter(img, op, &params.to_string()).unwrap();
    assert!(!out.is_null(), "{op} answered the bypass");
    let result = samples(out, (w * h) as usize);
    unsafe {
        dr_image_free(out);
        dr_image_free(img);
    }
    result
}
fn refusal(op: &str, params: Value) -> String {
    let img = image(4, 3, &[128; 48]);
    let err = raw_filter(img, op, &params.to_string()).unwrap_err();
    unsafe { dr_image_free(img) };
    err
}
fn doc16(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}
/// A `cw`×`ch` canvas holding one `w`×`h` raster layer at `offset`.
fn offset_doc(
    (cw, ch): (u32, u32),
    (w, h): (u32, u32),
    offset: (i32, i32),
    pixels: Vec<[u16; 4]>,
) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("Layer", image, offset, 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        cw,
        ch,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
}
fn edit16(d: &PrecisionDocument, op: &str, params: Value, coverage: &[u16]) -> Vec<[u16; 4]> {
    let e = SelectedEdit::parse(&json!({"layer":0,"op":op,"params":params}).to_string()).unwrap();
    let out = d
        .selected_edit(&e, coverage, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    out.as_ref()
        .unwrap_or(d)
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .unwrap()
}
fn widen(pixels: &[u8]) -> Vec<[u16; 4]> {
    pixels
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect()
}
/// Structure and mixed alpha (opaque, partial, a transparent pixel with RGB 0).
fn plate(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let value = |offset: u32| ((x * 23 + y * 41 + offset + (i % 3) * 17) % 256) as u8;
            if i % 13 == 5 {
                return [0, 0, 0, 0];
            }
            [
                value(0),
                value(80),
                value(160),
                if i % 4 == 1 {
                    128 + (i % 64) as u8
                } else {
                    255
                },
            ]
        })
        .collect()
}
/// A canvas-sized rectangle of full coverage.
fn rect_coverage(cw: u32, ch: u32, x: u32, y: u32, w: u32, h: u32) -> Vec<u16> {
    (0..cw * ch)
        .map(|i| {
            let (px, py) = (i % cw, i / cw);
            if px >= x && px < x + w && py >= y && py < y + h {
                65535
            } else {
                0
            }
        })
        .collect()
}

// ---- Radial Blur oracle -------------------------------------------------

/// The premultiplied unit sample at continuous `(qx, qy)` (pixel centres at
/// half-integers), 2×2 tent weights, clamped edges.
fn bilinear(src: &[u8], w: u32, h: u32, qx: f64, qy: f64) -> [f64; 4] {
    let (sx, sy) = (qx - 0.5, qy - 0.5);
    let (x0, y0) = (sx.floor() as i64, sy.floor() as i64);
    let mut out = [0.0; 4];
    for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        let (px, py) = (x0 + dx, y0 + dy);
        let weight = (1.0 - (sx - px as f64).abs()) * (1.0 - (sy - py as f64).abs());
        let cx = px.clamp(0, i64::from(w) - 1) as usize;
        let cy = py.clamp(0, i64::from(h) - 1) as usize;
        let i = (cy * w as usize + cx) * 4;
        let a = f64::from(src[i + 3]) / 255.0;
        for c in 0..3 {
            out[c] += weight * f64::from(src[i + c]) / 255.0 * a;
        }
        out[3] += weight * a;
    }
    out
}
fn nearest(src: &[u8], w: u32, h: u32, qx: f64, qy: f64) -> [f64; 4] {
    let cx = (qx.floor() as i64).clamp(0, i64::from(w) - 1) as usize;
    let cy = (qy.floor() as i64).clamp(0, i64::from(h) - 1) as usize;
    let i = (cy * w as usize + cx) * 4;
    let a = f64::from(src[i + 3]) / 255.0;
    [
        f64::from(src[i]) / 255.0 * a,
        f64::from(src[i + 1]) / 255.0 * a,
        f64::from(src[i + 2]) / 255.0 * a,
        a,
    ]
}
struct Radial {
    amount: f64,
    zoom: bool,
    density: f64,
    cap: f64,
    frame: (f64, f64, f64, f64),
    center: (f64, f64),
}
/// The straight 0…255 mean along pixel `(x, y)`'s path at the given `t_k`.
fn radial_oracle(
    src: &[u8],
    w: u32,
    h: u32,
    r: &Radial,
    (x, y): (u32, u32),
    ts: impl Fn(usize) -> Vec<f64>,
    read: impl Fn(&[u8], u32, u32, f64, f64) -> [f64; 4],
) -> [f64; 4] {
    let (fx, fy, fw, fh) = r.frame;
    let (cx, cy) = (fx + r.center.0 * fw, fy + r.center.1 * fh);
    let (dx, dy) = (f64::from(x) + 0.5 - cx, f64::from(y) + 0.5 - cy);
    let span = if r.zoom {
        r.amount * 0.005
    } else {
        r.amount.to_radians()
    };
    let length = (dx * dx + dy * dy).sqrt() * span;
    let n = 1 + (length * r.density).ceil().min(r.cap) as usize;
    let mut acc = [0.0; 4];
    for t in ts(n) {
        let (qx, qy) = if r.zoom {
            let s = 1.0 + span * (t - 0.5);
            (cx + s * dx, cy + s * dy)
        } else {
            let angle = span * (t - 0.5);
            (
                cx + angle.cos() * dx - angle.sin() * dy,
                cy + angle.sin() * dx + angle.cos() * dy,
            )
        };
        let p = read(src, w, h, qx, qy);
        for c in 0..4 {
            acc[c] += p[c] / n as f64;
        }
    }
    if acc[3] < 0.5 / 65535.0 {
        return [0.0; 4];
    }
    [
        acc[0] / acc[3] * 255.0,
        acc[1] / acc[3] * 255.0,
        acc[2] / acc[3] * 255.0,
        acc[3] * 255.0,
    ]
}
fn even(n: usize) -> Vec<f64> {
    if n == 1 {
        vec![0.5]
    } else {
        (0..n).map(|k| k as f64 / (n - 1) as f64).collect()
    }
}
fn close(out: &[u8], expected: [f64; 4], i: usize, what: &str) {
    for c in 0..4 {
        let got = f64::from(out[i * 4 + c]);
        if expected[3] == 0.0 && c < 3 {
            continue;
        }
        assert!(
            (got - expected[c]).abs() <= 1.0,
            "{what} pixel{i} component{c}: {got} vs {}",
            expected[c]
        );
    }
}

#[test]
fn radial_spin_and_zoom_match_the_direct_path_mean_at_good_and_best() {
    let (w, h) = (23_u32, 17_u32);
    let src = plate(w, h);
    for (method, quality, amount, center) in [
        ("spin", "good", 30.0, (0.5, 0.5)),
        ("spin", "best", 60.0, (0.3, 0.7)),
        ("zoom", "good", 50.0, (0.5, 0.5)),
        ("zoom", "best", 100.0, (0.9, 0.1)),
    ] {
        let params = json!({"amount":amount,"method":method,"quality":quality,
            "center_x":center.0,"center_y":center.1});
        let out = filter(w, h, &src, "radial_blur", params);
        let r = Radial {
            amount,
            zoom: method == "zoom",
            density: if quality == "best" { 2.0 } else { 1.0 },
            cap: if quality == "best" { 128.0 } else { 64.0 },
            frame: (0.0, 0.0, f64::from(w), f64::from(h)),
            center,
        };
        for y in 0..h {
            for x in 0..w {
                let expected = radial_oracle(&src, w, h, &r, (x, y), even, bilinear);
                close(
                    &out,
                    expected,
                    (y * w + x) as usize,
                    &format!("{method} {quality}"),
                );
            }
        }
    }
}

#[test]
fn radial_centre_is_relative_to_the_region_and_outside_pixels_are_copied() {
    let (w, h) = (21_u32, 15_u32);
    let src = plate(w, h);
    let (rx, ry, rw, rh) = (4_u32, 3_u32, 10_u32, 8_u32);
    let params = json!({"amount":45,"quality":"good","center_x":0.25,"center_y":0.75,
        "region_x":rx,"region_y":ry,"region_width":rw,"region_height":rh});
    let out = filter(w, h, &src, "radial_blur", params);
    let r = Radial {
        amount: 45.0,
        zoom: false,
        density: 1.0,
        cap: 64.0,
        frame: (f64::from(rx), f64::from(ry), f64::from(rw), f64::from(rh)),
        center: (0.25, 0.75),
    };
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
                let expected = radial_oracle(&src, w, h, &r, (x, y), even, bilinear);
                close(&out, expected, i, "framed spin");
            } else if src[i * 4 + 3] > 0 {
                assert_eq!(
                    out[i * 4..i * 4 + 4],
                    src[i * 4..i * 4 + 4],
                    "outside {x},{y}"
                );
            }
        }
    }
}

#[test]
fn radial_draft_is_deterministic_jittered_and_unbiased() {
    let (w, h) = (40_u32, 30_u32);
    let src = plate(w, h);
    let params = json!({"amount":80,"quality":"draft","method":"spin"});
    let first = filter(w, h, &src, "radial_blur", params.clone());
    assert_eq!(first, filter(w, h, &src, "radial_blur", params), "rerun");
    // The unjittered stratified oracle (t_k = (k + ½)/N, nearest) differs
    // somewhere: the jitter is real.
    let r = Radial {
        amount: 80.0,
        zoom: false,
        density: 0.25,
        cap: 16.0,
        frame: (0.0, 0.0, f64::from(w), f64::from(h)),
        center: (0.5, 0.5),
    };
    let centred = |n: usize| (0..n).map(|k| (k as f64 + 0.5) / n as f64).collect();
    let mut differs = 0;
    for y in 0..h {
        for x in 0..w {
            let expected = radial_oracle(&src, w, h, &r, (x, y), centred, nearest);
            let i = (y * w + x) as usize;
            if (0..4).any(|c| (f64::from(first[i * 4 + c]) - expected[c]).abs() > 1.0) {
                differs += 1;
            }
        }
    }
    assert!(differs > 0, "draft is jittered");
    // On a smooth opaque ramp Draft's mean is Good's within two levels.
    let ramp: Vec<u8> = (0..w * h)
        .flat_map(|i| {
            let v = (i % w * 255 / (w - 1)) as u8;
            [v, 255 - v, (i / w * 255 / (h - 1)) as u8, 255]
        })
        .collect();
    let mean = |out: &[u8]| {
        let mut sums = [0.0; 3];
        for p in out.chunks_exact(4) {
            for c in 0..3 {
                sums[c] += f64::from(p[c]);
            }
        }
        sums.map(|s| s / f64::from(w * h))
    };
    for method in ["spin", "zoom"] {
        let draft = mean(&filter(
            w,
            h,
            &ramp,
            "radial_blur",
            json!({"amount":60,"quality":"draft","method":method}),
        ));
        let good = mean(&filter(
            w,
            h,
            &ramp,
            "radial_blur",
            json!({"amount":60,"quality":"good","method":method}),
        ));
        for c in 0..3 {
            assert!(
                (draft[c] - good[c]).abs() <= 2.0,
                "{method} {c}: {draft:?} {good:?}"
            );
        }
    }
}

#[test]
fn radial_alpha_law_transparent_stays_transparent_opaque_stays_opaque() {
    let (w, h) = (9_u32, 7_u32);
    let clear = vec![0_u8; (w * h * 4) as usize];
    let out = filter(w, h, &clear, "radial_blur", json!({"amount":50}));
    assert!(out.chunks_exact(4).all(|p| p[3] == 0));
    let opaque: Vec<u8> = plate(w, h)
        .chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2], 255])
        .collect();
    let out = filter(
        w,
        h,
        &opaque,
        "radial_blur",
        json!({"amount":50,"method":"zoom"}),
    );
    assert!(out.chunks_exact(4).all(|p| p[3] == 255));
}

// ---- Offset -------------------------------------------------------------

/// The expected source of `(x, y)` for an edge, frame `(fx, fy, fw, fh)`.
fn offset_oracle(
    x: i64,
    y: i64,
    (h_off, v_off): (i64, i64),
    edge: &str,
    (fx, fy, fw, fh): (i64, i64, i64, i64),
) -> Option<(i64, i64)> {
    if x < fx || y < fy || x >= fx + fw || y >= fy + fh {
        return Some((x, y));
    }
    let (sx, sy) = (x - h_off, y - v_off);
    let inside = sx >= fx && sy >= fy && sx < fx + fw && sy < fy + fh;
    match edge {
        _ if inside => Some((sx, sy)),
        "repeat" => Some((sx.clamp(fx, fx + fw - 1), sy.clamp(fy, fy + fh - 1))),
        "wrap" => {
            let wrap = |v: i64, o: i64, n: i64| o + ((v - o) % n + n) % n;
            Some((wrap(sx, fx, fw), wrap(sy, fy, fh)))
        }
        _ => None,
    }
}

#[test]
fn offset_edges_are_exact_at_both_depths_whole_and_framed() {
    let (w, h) = (13_u32, 9_u32);
    let src = plate(w, h);
    let wide = widen(&src);
    let d = doc16(w, h, wide.clone());
    for (edge, extra) in [
        ("transparent", json!({})),
        ("repeat", json!({})),
        ("wrap", json!({})),
        ("fill", json!({"fill":0.25})),
    ] {
        for (shift, region) in [
            ((3_i64, -2_i64), None),
            ((-40, 27), None),
            ((2, 1), Some((2_u32, 1_u32, 7_u32, 5_u32))),
            ((-9, 100), Some((5, 3, 4, 6))),
        ] {
            let mut params =
                json!({"horizontal":shift.0,"vertical":shift.1,"undefined_areas":edge});
            for (k, v) in extra.as_object().unwrap() {
                params[k] = v.clone();
            }
            let frame = region.map_or((0, 0, i64::from(w), i64::from(h)), |r| {
                (
                    i64::from(r.0),
                    i64::from(r.1),
                    i64::from(r.2),
                    i64::from(r.3),
                )
            });
            let mut byte_params = params.clone();
            if let Some(r) = region {
                byte_params["region_x"] = json!(r.0);
                byte_params["region_y"] = json!(r.1);
                byte_params["region_width"] = json!(r.2);
                byte_params["region_height"] = json!(r.3);
            }
            let out8 = filter(w, h, &src, "offset", byte_params);
            let coverage = match region {
                Some(r) => rect_coverage(w, h, r.0, r.1, r.2, r.3),
                None => Vec::new(),
            };
            let out16 = edit16(&d, "offset", params, &coverage);
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    let what = format!("{edge} {shift:?} {region:?} at {x},{y}");
                    let (e8, e16) =
                        match offset_oracle(i64::from(x), i64::from(y), shift, edge, frame) {
                            Some((sx, sy)) => {
                                let j = (sy * i64::from(w) + sx) as usize;
                                (
                                    [src[j * 4], src[j * 4 + 1], src[j * 4 + 2], src[j * 4 + 3]],
                                    wide[j],
                                )
                            }
                            None if edge == "fill" => {
                                ([64, 64, 64, 255], [16384, 16384, 16384, 65535])
                            }
                            None => ([0; 4], [0; 4]),
                        };
                    assert_eq!(out8[i * 4..i * 4 + 4], e8, "8-bit {what}");
                    // A transparent result carries no colour at 16 bits.
                    if e16[3] == 0 {
                        assert_eq!(out16[i][3], 0, "16-bit {what}");
                    } else {
                        assert_eq!(out16[i], e16, "16-bit {what}");
                    }
                }
            }
        }
    }
    // Both offsets 0 is the exact bypass.
    let img = image(w, h, &src);
    assert!(
        raw_filter(img, "offset", r#"{"horizontal":0,"vertical":0}"#)
            .unwrap()
            .is_null()
    );
    unsafe { dr_image_free(img) };
}

#[test]
fn offset_wrap_then_its_inverse_is_the_identity() {
    let (w, h) = (11_u32, 8_u32);
    let src = plate(w, h);
    let there = filter(w, h, &src, "offset", json!({"horizontal":37,"vertical":-5}));
    let back = filter(
        w,
        h,
        &there,
        "offset",
        json!({"horizontal":-37,"vertical":5}),
    );
    assert_eq!(back, src);
}

#[test]
fn the_16_bit_layer_frame_follows_the_layer_offset() {
    // A 6×5 layer at (4, 3) on a 12×10 canvas, under a canvas selection of
    // x 5..9, y 4..7: the frame on the layer's raster is (1, 1, 4, 3).
    let (lw, lh) = (6_u32, 5_u32);
    let src = plate(lw, lh);
    let d = offset_doc((12, 10), (lw, lh), (4, 3), widen(&src));
    let coverage = rect_coverage(12, 10, 5, 4, 4, 3);
    let out16 = edit16(
        &d,
        "offset",
        json!({"horizontal":1,"vertical":2}),
        &coverage,
    );
    let byte = filter(
        lw,
        lh,
        &src,
        "offset",
        json!({"horizontal":1,"vertical":2,
            "region_x":1,"region_y":1,"region_width":4,"region_height":3}),
    );
    for y in 0..lh {
        for x in 0..lw {
            let i = (y * lw + x) as usize;
            let (sx, sy) =
                offset_oracle(i64::from(x), i64::from(y), (1, 2), "wrap", (1, 1, 4, 3)).unwrap();
            let j = (sy * i64::from(lw) + sx) as usize;
            let expected = [0, 1, 2, 3].map(|c| u16::from(src[j * 4 + c]) * 257);
            if expected[3] == 0 {
                assert_eq!(out16[i][3], 0);
            } else {
                assert_eq!(out16[i], expected, "{x},{y}");
            }
            assert_eq!(
                byte[i * 4..i * 4 + 4],
                src[j * 4..j * 4 + 4],
                "8-bit {x},{y}"
            );
        }
    }
    // The radial centre follows the same frame: the 16-bit result inside the
    // selection agrees with the byte route given that region.
    let radial = json!({"amount":40,"center_x":0.25,"center_y":0.5});
    let out16 = edit16(&d, "radial_blur", radial.clone(), &coverage);
    let mut framed = radial;
    for (k, v) in [
        ("region_x", 1),
        ("region_y", 1),
        ("region_width", 4),
        ("region_height", 3),
    ] {
        framed[k] = json!(v);
    }
    let byte = filter(lw, lh, &src, "radial_blur", framed);
    for y in 1..4 {
        for x in 1..5 {
            let i = (y * lw + x) as usize;
            for c in 0..4 {
                let got = f64::from(out16[i][c]) / 257.0;
                if c < 3 && out16[i][3] == 0 {
                    continue;
                }
                assert!(
                    (got - f64::from(byte[i * 4 + c])).abs() <= 1.0,
                    "radial {x},{y} {c}"
                );
            }
        }
    }
}

// ---- Clouds -------------------------------------------------------------

fn clouds(w: u32, h: u32, src: &[u8], params: Value) -> Vec<u8> {
    filter(w, h, src, "clouds", params)
}

#[test]
fn clouds_are_seeded_deterministic_in_range_and_high_contrast_spreads() {
    let (w, h) = (300_u32, 200_u32);
    let src = plate(w, h);
    let base = json!({"seed":7,"foreground_color":"#204060","background_color":"#e0c0a0"});
    let a = clouds(w, h, &src, base.clone());
    assert_eq!(a, clouds(w, h, &src, base.clone()), "deterministic");
    let mut other = base.clone();
    other["seed"] = json!(8);
    assert_ne!(a, clouds(w, h, &src, other), "the seed changes the clouds");
    // A seed crosses as its 64-bit pattern: -1 and u64::MAX are one seed.
    let mut negative = base.clone();
    negative["seed"] = json!(-1);
    let mut unsigned = base.clone();
    unsigned["seed"] = json!(u64::MAX);
    assert_eq!(
        clouds(w, h, &src, negative),
        clouds(w, h, &src, unsigned),
        "replayed seed"
    );
    let (fg, bg) = ([0x20, 0x40, 0x60], [0xe0, 0xc0, 0xa0]);
    for p in a.chunks_exact(4) {
        assert_eq!(p[3], 255, "clouds are opaque");
        for c in 0..3 {
            let (lo, hi) = (fg[c].min(bg[c]), fg[c].max(bg[c]));
            assert!(p[c] >= lo && p[c] <= hi, "{p:?} within fg/bg");
        }
    }
    let spread = |out: &[u8]| {
        let values: Vec<f64> = out.chunks_exact(4).map(|p| f64::from(p[0])).collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64).sqrt()
    };
    let mut contrast = base;
    contrast["high_contrast"] = json!(true);
    let b = clouds(w, h, &src, contrast);
    assert!(
        spread(&b) > spread(&a) * 1.3,
        "{} vs {}",
        spread(&b),
        spread(&a)
    );
    // Clouds use the whole range at the default colours.
    let grey = clouds(w, h, &src, json!({"seed":3}));
    let lo = grey.chunks_exact(4).map(|p| p[0]).min().unwrap();
    let hi = grey.chunks_exact(4).map(|p| p[0]).max().unwrap();
    assert!(lo < 64 && hi > 192, "range {lo}..{hi}");
}

#[test]
fn difference_clouds_is_the_absolute_difference_and_keeps_alpha() {
    let (w, h) = (64_u32, 48_u32);
    let src = plate(w, h);
    let params = json!({"seed":11,"foreground_color":"#102030","background_color":"#f0e0d0"});
    let cloud = clouds(w, h, &src, params.clone());
    let out = filter(w, h, &src, "difference_clouds", params);
    for i in 0..(w * h) as usize {
        if src[i * 4 + 3] == 0 {
            assert_eq!(
                out[i * 4..i * 4 + 4],
                src[i * 4..i * 4 + 4],
                "alpha-0 untouched"
            );
            continue;
        }
        assert_eq!(out[i * 4 + 3], src[i * 4 + 3], "alpha exact");
        for c in 0..3 {
            let expected = (i32::from(src[i * 4 + c]) - i32::from(cloud[i * 4 + c])).abs();
            assert!(
                (i32::from(out[i * 4 + c]) - expected).abs() <= 1,
                "pixel{i} {c}"
            );
        }
    }
}

#[test]
fn byte_and_16_bit_agree_within_a_level_under_a_rect_selection() {
    let (w, h) = (31_u32, 22_u32);
    let src = plate(w, h);
    let d = doc16(w, h, widen(&src));
    let (rx, ry, rw, rh) = (6_u32, 4_u32, 17_u32, 12_u32);
    let coverage = rect_coverage(w, h, rx, ry, rw, rh);
    for (op, params) in [
        (
            "radial_blur",
            json!({"amount":35,"quality":"good","center_x":0.3}),
        ),
        (
            "radial_blur",
            json!({"amount":70,"method":"zoom","quality":"best"}),
        ),
        ("radial_blur", json!({"amount":50,"quality":"draft"})),
        ("clouds", json!({"seed":99})),
        ("difference_clouds", json!({"seed":99,"high_contrast":true})),
        (
            "offset",
            json!({"horizontal":5,"vertical":-3,"undefined_areas":"repeat"}),
        ),
    ] {
        let out16 = edit16(&d, op, params.clone(), &coverage);
        let mut byte_params = params;
        if op != "clouds" && op != "difference_clouds" {
            byte_params["region_x"] = json!(rx);
            byte_params["region_y"] = json!(ry);
            byte_params["region_width"] = json!(rw);
            byte_params["region_height"] = json!(rh);
        }
        let out8 = filter(w, h, &src, op, byte_params);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let inside = x >= rx && x < rx + rw && y >= ry && y < ry + rh;
                if !inside {
                    assert_eq!(out16[i], widen(&src[i * 4..i * 4 + 4])[0], "{op} outside");
                    continue;
                }
                for c in 0..4 {
                    if c < 3 && out16[i][3] == 0 {
                        continue;
                    }
                    let got = f64::from(out16[i][c]) / 257.0;
                    assert!(
                        (got - f64::from(out8[i * 4 + c])).abs() <= 1.0,
                        "{op} {x},{y} {c}: {got} vs {}",
                        out8[i * 4 + c]
                    );
                }
            }
        }
    }
}

// ---- Scalar targets -----------------------------------------------------

fn scalar_edit(d: &PrecisionDocument, request: Value, input: &[u16]) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan = unsafe {
        dr_precision_doc_edit_plan(d, text.as_ptr(), REFERENCE_WORKING_BYTES, 0, &mut err)
    };
    assert!(!plan.is_null());
    unsafe { dr_string_free(plan) };
    let out = unsafe {
        dr_precision_doc_edit(
            d,
            text.as_ptr(),
            input.as_ptr(),
            input.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(err.is_null() && !out.is_null());
    *unsafe { Box::from_raw(out) }
}

#[test]
fn scalar_offset_fill_writes_the_fill_exactly_on_a_channel_within_the_frame() {
    let (w, h) = (8_u32, 6_u32);
    let samples: Vec<u16> = (0..w * h).map(|i| (1000 + i * 1031) as u16).collect();
    let channel = PrecisionChannel::new(
        "Alpha 1",
        PrecisionPlane::from_u16(w, h, samples.clone(), None).unwrap(),
    )
    .unwrap();
    let layer = PrecisionDocumentLayer::raster(
        "Layer",
        PrecisionImage::from_rgba16(
            w,
            h,
            vec![[0, 0, 0, 65535]; (w * h) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let d = PrecisionDocument::from_parts_and_channels(
        w,
        h,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
        vec![channel],
    )
    .unwrap();
    let coverage = rect_coverage(w, h, 2, 1, 4, 3);
    let fill = 0.3_f64;
    let request = json!({"op":"scalar_edit","target":"channel","index":0,"selection":true,
        "operation":{"kind":"filter","edit":{"layer":0,"op":"offset",
        "params":{"horizontal":2,"undefined_areas":"fill","fill":fill}}}});
    let next = scalar_edit(&d, request, &coverage);
    let out = next.channels()[0].plane().samples().to_vec();
    let code = ((fill as f32) * 65535.0).round() as u16;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let expected = if (2..6).contains(&x) && (1..4).contains(&y) {
                if x < 4 {
                    code
                } else {
                    samples[i - 2]
                }
            } else {
                samples[i]
            };
            assert_eq!(out[i], expected, "{x},{y}");
        }
    }
}

// ---- Refusals -----------------------------------------------------------

#[test]
fn parameters_refuse_by_name() {
    for (op, params, expected) in [
        (
            "radial_blur",
            json!({"amount":0}),
            "amount must be finite in [1,100]",
        ),
        (
            "radial_blur",
            json!({"amount":10.5}),
            "amount must be an integer",
        ),
        (
            "radial_blur",
            json!({"method":"twist"}),
            "method must be one of spin, zoom",
        ),
        (
            "radial_blur",
            json!({"quality":"fast"}),
            "quality must be one of draft, good, best",
        ),
        (
            "radial_blur",
            json!({"center_x":1.5}),
            "center_x must be finite in [0,1]",
        ),
        (
            "radial_blur",
            json!({"region_x":0,"region_y":0,"region_width":2}),
            "region needs all of region_x, region_y, region_width and region_height",
        ),
        (
            "radial_blur",
            json!({"region_x":2,"region_y":0,"region_width":3,"region_height":1}),
            "region must be a non-empty rectangle inside the image",
        ),
        (
            "offset",
            json!({"horizontal":1,"region_x":0,"region_y":0,"region_width":0,"region_height":1}),
            "region must be a non-empty rectangle inside the image",
        ),
        (
            "offset",
            json!({"horizontal":1,"region_x":-1,"region_y":0,"region_width":1,"region_height":1}),
            "region_x must be finite in",
        ),
        (
            "offset",
            json!({"horizontal":1,"undefined_areas":"fill"}),
            "undefined_areas fill needs fill",
        ),
        (
            "offset",
            json!({"horizontal":1,"fill":0.5}),
            "fill belongs only to undefined_areas fill",
        ),
        (
            "offset",
            json!({"horizontal":1,"undefined_areas":"background"}),
            "undefined_areas must be one of transparent, repeat, wrap, fill",
        ),
        (
            "offset",
            json!({"horizontal":1,"background_color":"#ffffff"}),
            "unexpected offset parameter background_color",
        ),
        (
            "offset",
            json!({"horizontal":1.5}),
            "horizontal must be an integer",
        ),
        (
            "offset",
            json!({"vertical":100_000_001}),
            "vertical must be finite in [-100000000,100000000]",
        ),
        (
            "offset",
            json!({"horizontal":1,"undefined_areas":"fill","fill":2}),
            "fill must be finite in [0,1]",
        ),
        (
            "clouds",
            json!({"high_contrast":"yes"}),
            "high_contrast must be true or false",
        ),
        (
            "clouds",
            json!({"foreground_color":"red"}),
            "foreground_color must be a #rrggbb",
        ),
        (
            "clouds",
            json!({"seed":1.5}),
            "seed must be a 64-bit integer",
        ),
        (
            "difference_clouds",
            json!({"background":true}),
            "unexpected difference_clouds parameter background",
        ),
    ] {
        let err = refusal(op, params.clone());
        assert!(err.contains(expected), "{op} {params}: {err}");
    }
    // Large offsets are defined: no refusal for an offset beyond the image.
    let (w, h) = (4_u32, 3_u32);
    let src = plate(w, h);
    let out = filter(
        w,
        h,
        &src,
        "offset",
        json!({"horizontal":100_000_000,"vertical":-7}),
    );
    assert_eq!(out.len(), src.len());
}

// ---- Calibration --------------------------------------------------------

/// SplitMix64, written independently of the core's for the probe's inputs.
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The explicit margin every shipped constant carries over the measured rate
/// (the Surface Blur precedent: the reference varies about 2% between runs).
const SAFETY_FACTOR: f64 = 1.15;

fn min_of_3(mut run: impl FnMut()) -> f64 {
    (0..3)
        .map(|_| {
            let start = std::time::Instant::now();
            run();
            start.elapsed().as_secs_f64()
        })
        .fold(f64::INFINITY, f64::min)
}

/// `radial_plan`'s sample count over the whole `w`×`h` plane, written from its
/// stated rule: per 64×64 tile, pixels × N at the farthest tile-corner to
/// plane-corner distance.
fn radial_samples(w: u32, h: u32, amount: f64, zoom: bool, density: f64, cap: f64) -> f64 {
    let span = if zoom {
        amount * 0.005
    } else {
        amount.to_radians()
    };
    let corners = [
        (0.0, 0.0),
        (f64::from(w), 0.0),
        (0.0, f64::from(h)),
        (f64::from(w), f64::from(h)),
    ];
    let mut total = 0.0;
    for ty in (0..h).step_by(64) {
        let th = 64.min(h - ty);
        for tx in (0..w).step_by(64) {
            let tw = 64.min(w - tx);
            let mut far = 0.0_f64;
            for (px, py) in [(tx, ty), (tx + tw, ty), (tx, ty + th), (tx + tw, ty + th)] {
                for (cx, cy) in corners {
                    far = far.max((f64::from(px) - cx).hypot(f64::from(py) - cy));
                }
            }
            let n = 1.0 + (far * span * density).ceil().min(cap);
            total += f64::from(tw) * f64::from(th) * n;
        }
    }
    total
}

/// Opaque splitmix noise as RGBA8, seeded for the probe's inputs.
fn probe_noise(w: u32, h: u32, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..w * h)
        .flat_map(|_| {
            let v = splitmix(&mut state);
            [v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]
        })
        .collect()
}

/// The calibration record of `RADIAL_NEAREST_WORK_PER_SAMPLE`,
/// `RADIAL_BILINEAR_WORK_PER_SAMPLE`, `CLOUDS_WORK_PER_PIXEL` and
/// `OFFSET_WORK_PER_PIXEL`. Times the Gaussian reference
/// (byte High Pass at σ3.9, converted to the radius-16 unit by
/// `R39_PER_R16`), then each op on opaque splitmix noise through the byte
/// `dr_image_filter_op` and the 16-bit `selected_edit` (the whole edit, so the
/// 16-bit read path and writeback are inside the charge). Prints the implied
/// units per planned sample or pixel at each depth, the larger times
/// `SAFETY_FACTOR`, and the 24 MP verdicts at those values. The constants were
/// frozen from the 6000×4000 rows, which ran before freezing; at the shipped
/// bilinear constant the 24 MP Good and Best rows are refused before they run,
/// so the probe reports the refusal and re-checks the bilinear constant at
/// 2400×1600, which it admits.
#[test]
#[ignore = "calibration probe: make t F=radial_clouds_offset A='--ignored --nocapture --test-threads=1'"]
fn radial_clouds_offset_calibration_probe() {
    let (w, h) = (6000_u32, 4000_u32);
    let n = f64::from(w) * f64::from(h);
    let noise = probe_noise(w, h, 0x5EED);
    let img = image(w, h, &noise);
    let t_gauss = min_of_3(|| {
        let out = raw_filter(img, "high_pass", r#"{"radius":3.9}"#).unwrap();
        assert!(!out.is_null());
        unsafe { dr_image_free(out) };
    });
    let work_gauss = n * (8.0 * 25.0 + 12.0);
    let rate = work_gauss / t_gauss / gaussian_reference::R39_PER_R16;
    println!(
        "reference: high_pass σ3.9 on {w}x{h}: {t_gauss:.3} s; r16-unit rate {rate:.4e} units/s"
    );
    let d = doc16(w, h, widen(&noise));
    let (sw, sh) = (2400_u32, 1600_u32);
    let small_noise = probe_noise(sw, sh, 0x5EED);
    let small = image(sw, sh, &small_noise);
    let small_doc = doc16(sw, sh, widen(&small_noise));
    let limit = 64 * 1024 * 1024 * 1024_u64;
    let samples = |big: bool, a: f64, zoom: bool, density: f64, cap: f64| {
        let (cw, ch) = if big { (w, h) } else { (sw, sh) };
        radial_samples(cw, ch, a, zoom, density, cap)
    };
    let cases: [(&str, bool, &str, Value, f64); 11] = [
        (
            "bilinear",
            true,
            "radial_blur",
            json!({"amount":10,"quality":"good"}),
            samples(true, 10.0, false, 1.0, 64.0),
        ),
        (
            "bilinear",
            true,
            "radial_blur",
            json!({"amount":10,"quality":"good","method":"zoom"}),
            samples(true, 10.0, true, 1.0, 64.0),
        ),
        (
            "bilinear",
            true,
            "radial_blur",
            json!({"amount":10,"quality":"best"}),
            samples(true, 10.0, false, 2.0, 128.0),
        ),
        (
            "bilinear",
            false,
            "radial_blur",
            json!({"amount":10,"quality":"good"}),
            samples(false, 10.0, false, 1.0, 64.0),
        ),
        (
            "bilinear",
            false,
            "radial_blur",
            json!({"amount":10,"quality":"best"}),
            samples(false, 10.0, false, 2.0, 128.0),
        ),
        (
            "nearest",
            true,
            "radial_blur",
            json!({"amount":10,"quality":"draft"}),
            samples(true, 10.0, false, 0.25, 16.0),
        ),
        (
            "nearest",
            true,
            "radial_blur",
            json!({"amount":100,"quality":"draft","method":"zoom"}),
            samples(true, 100.0, true, 0.25, 16.0),
        ),
        ("clouds", true, "clouds", json!({"seed":5}), n),
        (
            "clouds",
            true,
            "difference_clouds",
            json!({"seed":5,"high_contrast":true}),
            n,
        ),
        (
            "offset",
            true,
            "offset",
            json!({"horizontal":1234,"vertical":-567}),
            n,
        ),
        (
            "offset",
            true,
            "offset",
            json!({"horizontal":1234,"vertical":-567,"undefined_areas":"repeat"}),
            n,
        ),
    ];
    let mut implied: Vec<(&str, f64)> = Vec::new();
    for (family, big, op, params, count) in cases {
        let text = params.to_string();
        let (source, document, size) = if big {
            (img, &d, format!("{w}x{h}"))
        } else {
            (small, &small_doc, format!("{sw}x{sh}"))
        };
        if let Err(refusal) = raw_filter(source, op, &text).map(|out| unsafe { dr_image_free(out) })
        {
            println!("{op} {text} at {size}: refused at the shipped constants ({refusal})");
            continue;
        }
        let t8 = min_of_3(|| {
            let out = raw_filter(source, op, &text).unwrap();
            assert!(!out.is_null());
            unsafe { dr_image_free(out) };
        });
        let e =
            SelectedEdit::parse(&json!({"layer":0,"op":op,"params":params}).to_string()).unwrap();
        let t16 = min_of_3(|| {
            assert!(document
                .selected_edit(&e, &[], limit, 0, None)
                .unwrap()
                .is_some());
        });
        let (k8, k16) = (t8 * rate / count, t16 * rate / count);
        println!(
            "{op} {text} at {size}: 8-bit {t8:.3} s ({k8:.2} units each), 16-bit {t16:.3} s ({k16:.2} units each), count {count:.4e}"
        );
        implied.push((family, k8.max(k16)));
    }
    unsafe {
        dr_image_free(img);
        dr_image_free(small);
    }
    let mut shipped = std::collections::HashMap::new();
    for family in ["nearest", "bilinear", "clouds", "offset"] {
        let measured = implied
            .iter()
            .filter(|(f, _)| *f == family)
            .map(|(_, k)| *k)
            .fold(0.0_f64, f64::max);
        let constant = (measured * SAFETY_FACTOR).ceil();
        shipped.insert(family, constant);
        println!("{family}: measured {measured:.2} units; x{SAFETY_FACTOR} = {constant:.0}");
    }
    // `sampling_gaussian::MAX_DESTRUCTIVE_WORK`, which every plan is checked against.
    let cap = 24_000_000_000_f64;
    for (what, charge) in [
        (
            "radial spin good a10",
            shipped["bilinear"] * radial_samples(w, h, 10.0, false, 1.0, 64.0),
        ),
        (
            "radial spin best a10",
            shipped["bilinear"] * radial_samples(w, h, 10.0, false, 2.0, 128.0),
        ),
        (
            "radial spin draft a100",
            shipped["nearest"] * radial_samples(w, h, 100.0, false, 0.25, 16.0),
        ),
        ("clouds", shipped["clouds"] * n),
        ("offset", shipped["offset"] * n),
    ] {
        let verdict = if charge <= cap { "admitted" } else { "refused" };
        println!("24 MP {what}: charge {charge:.3e} of {cap:.3e}: {verdict}");
    }
}
