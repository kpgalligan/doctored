//! Every adjustment the CMYK adjustments batch admits on ink (#1), each
//! against an independent oracle written here, through the FFI:
//!
//! * the RGB-bound ops (P4: Invert, Posterize, Brightness/Contrast; C3K:
//!   Color Balance, Photo Filter, Color Lookup) against the RGB16 FFI result
//!   of an sRGB document of the `L = 1 − ink` planes, per plate, with K
//!   bit-exact where held;
//! * Shadows/Highlights against an f64 statement of `adjust_tone`'s formula
//!   with the ink-tone guide, on uniform patches (where the guide is the
//!   patch's own tone), a K-only 90 % patch among them;
//! * the native ops (Channel Mixer on ink, Selective Color, Threshold,
//!   Gradient Map, Replace Color) against the formulas of
//!   `cmyk_adjust_ink`'s module doc, in f64; Threshold's black is K 100 %
//!   with C/M/Y 0, destructive and as a layer, at 8 and 16 bits;
//! * identity params answer NULL; a record at opacity 1 equals the
//!   destructive result; a document of five new records reopens bit-exact;
//!   every refusal names itself.

use std::ffi::CString;
use std::ptr;

use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_precision::{
    dr_precision_doc_free, dr_precision_doc_open, dr_precision_doc_render,
    dr_precision_doc_save_native, dr_precision_image_free,
};
use doctored_core::ffi_precision_color::{
    dr_precision_doc_add_adjustment, dr_precision_doc_apply_adjustment,
    dr_precision_doc_set_adjustment,
};
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::common::{cpath, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;

fn free(document: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(document) };
}

/// A CMYK document converted from u16 sRGB `pixels` into the press profile.
fn ink_doc(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    ink_doc_with(w, h, pixels, c"{}")
}

/// `ink_doc` at `depth` bits per plate (8 or 16).
fn ink_doc_at(w: u32, h: u32, pixels: Vec<[u16; 4]>, depth: u32) -> *mut PrecisionDocument {
    let options = CString::new(format!(r#"{{"depth":{depth}}}"#)).unwrap();
    ink_doc_with(w, h, pixels, &options)
}

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

/// An RGB16 sRGB document of `pixels`.
fn rgb_doc(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let image = PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
    Box::into_raw(Box::new(
        PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap(),
    ))
}

fn picture(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            [
                (i * 7919 % 65536) as u16,
                (i * 104_729 % 65536) as u16,
                (i * 31_337 % 65536) as u16,
                [65535, 30000, 0, 65535][(i % 4) as usize],
            ]
        })
        .collect()
}

fn samples(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    unsafe { &*doc }
        .cmyk_layer_samples(index)
        .expect("a CMYK record")
}

fn code(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn unit(v: u16) -> f64 {
    f64::from(v) / 65535.0
}

fn description(op: &str, params: &str) -> String {
    format!(r#"{{"type":"adjust","evaluation":"precision_v1","op":"{op}","params":{params}}}"#)
}

fn apply(doc: *const PrecisionDocument, desc: &str) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(desc).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_apply_adjustment(doc, 0, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    if out.is_null() && !err.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn applied(doc: *const PrecisionDocument, desc: &str) -> *mut PrecisionDocument {
    let out = apply(doc, desc).unwrap_or_else(|e| panic!("{desc}: {e}"));
    assert!(!out.is_null(), "{desc} changed nothing");
    out
}

fn add(
    doc: *const PrecisionDocument,
    desc: &str,
    opacity: f32,
) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(desc).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_add_adjustment(
            doc,
            c"Tone".as_ptr(),
            text.as_ptr(),
            opacity,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn edit(doc: *const PrecisionDocument, request: &str) -> *mut PrecisionDocument {
    let text = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{request}: {}", take_err_string(err));
    out
}

/// The RGB16 result of `desc` on an sRGB document of `pixels` (the oracle
/// of the RGB-bound ops: the RGB kernel's own answer).
fn rgb_result(w: u32, h: u32, pixels: Vec<[u16; 4]>, desc: &str) -> Vec<[u16; 4]> {
    let doc = rgb_doc(w, h, pixels.clone());
    let out = apply(doc, desc).unwrap_or_else(|e| panic!("RGB {desc}: {e}"));
    let result = if out.is_null() {
        pixels
    } else {
        let px = unsafe { &*out }
            .layer(0)
            .unwrap()
            .source()
            .unwrap()
            .to_rgba16()
            .unwrap();
        free(out);
        px
    };
    free(doc);
    result
}

/// The two `L` plane documents of a CMYK layer: `[L_c, L_m, L_y, α]` and
/// `[L_k; 3, α]`.
fn l_planes(source: &[[u16; 5]]) -> (Vec<[u16; 4]>, Vec<[u16; 4]>) {
    (
        source
            .iter()
            .map(|s| [65535 - s[0], 65535 - s[1], 65535 - s[2], s[4]])
            .collect(),
        source
            .iter()
            .map(|s| [65535 - s[3], 65535 - s[3], 65535 - s[3], s[4]])
            .collect(),
    )
}

#[test]
fn rgb_bound_ops_equal_the_rgb_kernel_on_l() {
    let (w, h) = (9u32, 7u32);
    let doc = ink_doc(w, h, picture(w, h));
    let source = samples(doc, 0);
    let (cmy, k) = l_planes(&source);
    // (description, per plate) — per plate runs the kernel on K too.
    let lut: Vec<u8> = (0..8)
        .flat_map(|i: u32| {
            let rgb = [
                (i & 1) as f32 * 0.9,
                ((i >> 1) & 1) as f32,
                ((i >> 2) & 1) as f32 * 0.8,
            ];
            rgb.into_iter().flat_map(f32::to_le_bytes)
        })
        .collect();
    let table = crate::float_adjustments::base64(&lut);
    let cases = [
        (description("invert", "{}"), true),
        (description("posterize", r#"{"levels":5}"#), true),
        (
            description("bcs", r#"{"brightness":0.2,"contrast":0.3,"legacy":true}"#),
            true,
        ),
        (
            description(
                "bcs",
                r#"{"brightness":-0.3,"contrast":0.5,"legacy":false}"#,
            ),
            true,
        ),
        (
            description(
                "color_balance",
                r#"{"midtones":{"cyan_red":0.4,"magenta_green":-0.3},"shadows":{"yellow_blue":0.5},"preserve_luminosity":false}"#,
            ),
            false,
        ),
        (
            description(
                "color_lookup",
                &format!(r#"{{"kind":"3d","size":2,"table":"{table}"}}"#),
            ),
            false,
        ),
    ];
    for (desc, per_plate) in &cases {
        let out = applied(doc, desc);
        let got = samples(out, 0);
        let want_cmy = rgb_result(w, h, cmy.clone(), desc);
        let want_k = rgb_result(w, h, k.clone(), desc);
        for (i, ((g, s), (a, b))) in got
            .iter()
            .zip(&source)
            .zip(want_cmy.iter().zip(&want_k))
            .enumerate()
        {
            if s[4] == 0 {
                assert_eq!(g, s, "{desc}: pixel {i} at alpha 0 is untouched");
                continue;
            }
            for c in 0..3 {
                assert!(
                    g[c].abs_diff(65535 - a[c]) <= 1,
                    "{desc}: pixel {i} plate {c}: {} vs {}",
                    g[c],
                    65535 - a[c]
                );
            }
            if *per_plate {
                assert!(
                    g[3].abs_diff(65535 - b[0]) <= 1,
                    "{desc}: pixel {i} K: {} vs {}",
                    g[3],
                    65535 - b[0]
                );
            } else {
                assert_eq!(g[3], s[3], "{desc}: pixel {i} K is held bit for bit");
            }
            assert_eq!(g[4], s[4], "{desc}: alpha is kept");
        }
        free(out);
    }
    free(doc);
}

#[test]
fn photo_filter_takes_an_ink_and_holds_k() {
    let (w, h) = (6u32, 5u32);
    let doc = ink_doc(w, h, picture(w, h));
    let source = samples(doc, 0);
    let (cmy, _) = l_planes(&source);
    let ink: [f64; 4] = [0.1, 0.4, 0.9, 0.3];
    let hex = format!(
        "#{:02x}{:02x}{:02x}",
        (255.0 * (1.0 - ink[0])).round() as u8,
        (255.0 * (1.0 - ink[1])).round() as u8,
        (255.0 * (1.0 - ink[2])).round() as u8
    );
    for preserve in [true, false] {
        let cmyk = description(
            "photo_filter",
            &format!(
                r#"{{"ink":[{},{},{},{}],"density":0.6,"preserve_luminosity":{preserve}}}"#,
                ink[0], ink[1], ink[2], ink[3]
            ),
        );
        let rgb = description(
            "photo_filter",
            &format!(r#"{{"color":"{hex}","density":0.6,"preserve_luminosity":{preserve}}}"#),
        );
        let out = applied(doc, &cmyk);
        let want = rgb_result(w, h, cmy.clone(), &rgb);
        for (i, ((g, s), a)) in samples(out, 0).iter().zip(&source).zip(&want).enumerate() {
            if s[4] == 0 {
                continue;
            }
            for c in 0..3 {
                assert!(g[c].abs_diff(65535 - a[c]) <= 1, "pixel {i} plate {c}");
            }
            assert_eq!(g[3], s[3], "pixel {i}: K held");
        }
        free(out);
    }
    free(doc);
}

/// `adjust_tone`'s Shadows/Highlights on one RGB triple with guide `b`, in
/// f64 (its module doc's statement).
fn shadows_highlights(
    rgb: [f64; 3],
    b: f64,
    shadow: (f64, f64),
    highlight: (f64, f64),
    color: f64,
    mid: f64,
) -> [f64; 3] {
    let luma = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    let ws = (1.0 - b / shadow.1).clamp(0.0, 1.0).powi(2);
    let wh = (1.0 - (1.0 - b) / highlight.1).clamp(0.0, 1.0).powi(2);
    let e = (1.0 + highlight.0 * wh) / (1.0 + shadow.0 * ws);
    let mut l2 = luma.powf(e);
    l2 += mid * (3.0 * l2 * l2 - 2.0 * l2 * l2 * l2 - l2);
    let k = if luma > 0.0 { l2 / luma } else { 1.0 };
    let w = (shadow.0 * ws + highlight.0 * wh).clamp(0.0, 1.0);
    rgb.map(|v| (l2 + (v * k - l2) * (1.0 + color * w)).clamp(0.0, 1.0))
}

#[test]
fn shadows_highlights_reads_the_ink_tone_so_a_k_black_is_a_shadow() {
    // Uniform opaque patches: the blurred guide is the patch's own tone
    // `luma709(L_CMY)·L_K` everywhere.
    let patches: [[f64; 4]; 4] = [
        [0.0, 0.0, 0.0, 0.9],
        [0.75, 0.68, 0.67, 0.9],
        [0.3, 0.2, 0.1, 0.0],
        [0.1, 0.5, 0.2, 0.4],
    ];
    let shadow = (0.35, 0.5);
    let highlight = (0.25, 0.4);
    let (color, mid) = (0.2, 0.1);
    let desc = description(
        "shadows_highlights",
        r#"{"shadows":{"amount":0.35,"tone":0.5},"highlights":{"amount":0.25,"tone":0.4},"radius":3,"color":0.2,"midtone_contrast":0.1}"#,
    );
    for ink in patches {
        let (w, h) = (6u32, 6u32);
        let base = ink_doc(w, h, vec![[65535, 65535, 65535, 65535]; 36]);
        let codes = ink.map(code);
        let fill = format!(
            r#"{{"op":"paint_blend","layer":0,"ink":[{},{},{},{},1],"opacity":1}}"#,
            unit(codes[0]),
            unit(codes[1]),
            unit(codes[2]),
            unit(codes[3])
        );
        let painted = {
            let text = CString::new(fill.clone()).unwrap();
            let cover = [65535u16; 36];
            let mut err = ptr::null_mut();
            let out = unsafe {
                dr_precision_doc_edit(
                    base,
                    text.as_ptr(),
                    cover.as_ptr(),
                    cover.len(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
            };
            assert!(!out.is_null(), "{fill}: {}", take_err_string(err));
            out
        };
        let source = samples(painted, 0)[0];
        let l = [0, 1, 2, 3].map(|c| 1.0 - unit(source[c]));
        let tone = (0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2]) * l[3];
        let cmy = shadows_highlights([l[0], l[1], l[2]], tone, shadow, highlight, color, mid);
        let kk = shadows_highlights([l[3]; 3], tone, shadow, highlight, color, mid)[0];
        let want = [1.0 - cmy[0], 1.0 - cmy[1], 1.0 - cmy[2], 1.0 - kk].map(code);
        let out = applied(painted, &desc);
        for g in samples(out, 0) {
            for c in 0..4 {
                assert!(
                    g[c].abs_diff(want[c]) <= 2,
                    "ink {ink:?} plate {c}: {} vs {}",
                    g[c],
                    want[c]
                );
            }
        }
        if ink == [0.0, 0.0, 0.0, 0.9] {
            let g = samples(out, 0)[0];
            assert!(
                g[3] < source[3] - 2000,
                "Shadows lifts a K-only 90 % black: {g:?}"
            );
            assert_eq!(&g[..3], &[0, 0, 0], "a K-only black stays K-only");
        }
        for d in [base, painted, out] {
            free(d);
        }
    }
}

fn truncate_1024(v: f64) -> f64 {
    (1024.0 * v).trunc() / 1024.0
}

/// Selective Color's memberships of an RGB triple (`adjust_mix`'s module
/// doc statement): the two hue ranges between the largest channels, then
/// whites, neutrals, blacks.
fn memberships(rgb: [f64; 3]) -> [f64; 9] {
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| rgb[b].total_cmp(&rgb[a]).then(a.cmp(&b)));
    let (hi, mid, lo) = (order[0], order[1], order[2]);
    let mut w = [0.0; 9];
    let chroma = rgb[hi] - rgb[lo];
    if chroma > 0.0 {
        w[hi * 2] = (rgb[hi] - rgb[mid]) / chroma;
        let secondary = match (hi.min(mid), hi.max(mid)) {
            (0, 1) => 1,
            (1, 2) => 3,
            _ => 5,
        };
        w[secondary] = (rgb[mid] - rgb[lo]) / chroma;
    }
    w[6] = ((rgb[lo] - 0.5) * 2.0).clamp(0.0, 1.0);
    w[8] = ((0.5 - rgb[hi]) * 2.0).clamp(0.0, 1.0);
    w[7] = (1.0 - w[6] - w[8]).clamp(0.0, 1.0);
    w
}

#[test]
fn native_ops_match_their_f64_oracles() {
    let (w, h) = (11u32, 9u32);
    let doc = ink_doc(w, h, picture(w, h));
    let source = samples(doc, 0);
    let check = |desc: &str, oracle: &dyn Fn([f64; 4]) -> [f64; 4], tolerance: u16| {
        let out = applied(doc, desc);
        for (i, (g, s)) in samples(out, 0).iter().zip(&source).enumerate() {
            if s[4] == 0 {
                assert_eq!(g, s, "{desc}: pixel {i}");
                continue;
            }
            let want = oracle([0, 1, 2, 3].map(|c| unit(s[c]))).map(code);
            for c in 0..4 {
                assert!(
                    g[c].abs_diff(want[c]) <= tolerance,
                    "{desc}: pixel {i} plate {c}: {} vs {}",
                    g[c],
                    want[c]
                );
            }
            assert_eq!(g[4], s[4]);
        }
        free(out);
    };
    // Channel Mixer on ink: rows, a constant, a sum past 100 %.
    let rows = [
        [0.6, 0.4, 0.0, 0.0, 0.0],
        [0.0, 1.5, -0.3, 0.0, -0.2],
        [0.0, 0.0, 1.0, 0.0, 0.0],
        [0.2, 0.0, 0.0, 0.8, 0.1],
    ];
    check(
        &description(
            "channel_mixer",
            r#"{"cyan":{"c":0.6,"m":0.4},"magenta":{"m":1.5,"y":-0.3,"constant":-0.2},"black":{"c":0.2,"k":0.8,"constant":0.1}}"#,
        ),
        &|ink| {
            rows.map(|r| {
                (0..4).map(|j| truncate_1024(r[j]) * ink[j]).sum::<f64>() + truncate_1024(r[4])
            })
            .map(|v| v.clamp(0.0, 1.0))
        },
        1,
    );
    // Monochrome: the gray row to all four plates.
    let gray = [0.3, 0.3, 0.3, 1.0];
    check(
        &description(
            "channel_mixer",
            r#"{"monochrome":true,"gray":{"c":0.3,"m":0.3,"y":0.3,"k":1.0}}"#,
        ),
        &|ink| {
            let v = (0..4).map(|j| truncate_1024(gray[j]) * ink[j]).sum::<f64>();
            [v.clamp(0.0, 1.0); 4]
        },
        1,
    );
    // Selective Color, both methods: memberships of the L triple, nudges
    // on the stored ink.
    let ranges: [[f64; 4]; 9] = {
        let mut r = [[0.0; 4]; 9];
        r[0] = [-0.3, 0.2, 0.0, 0.1];
        r[4] = [0.0, 0.0, 0.25, 0.0];
        r[7] = [0.0, 0.0, 0.0, 0.15];
        r[8] = [0.0, 0.0, 0.0, -0.2];
        r
    };
    for method in ["relative", "absolute"] {
        check(
            &description(
                "selective_color",
                &format!(
                    r#"{{"method":"{method}","reds":{{"c":-0.3,"m":0.2,"k":0.1}},"blues":{{"y":0.25}},"neutrals":{{"k":0.15}},"blacks":{{"k":-0.2}}}}"#
                ),
            ),
            &|ink| {
                let m = memberships([1.0 - ink[0], 1.0 - ink[1], 1.0 - ink[2]]);
                std::array::from_fn(|p| {
                    let d: f64 = (0..9).map(|r| m[r] * ranges[r][p]).sum();
                    if method == "absolute" {
                        ink[p] + d
                    } else {
                        ink[p] * (1.0 + d)
                    }
                    .clamp(0.0, 1.0)
                })
            },
            1,
        );
    }
    let tone = |ink: [f64; 4]| {
        (0.2126 * (1.0 - ink[0]) + 0.7152 * (1.0 - ink[1]) + 0.0722 * (1.0 - ink[2]))
            * (1.0 - ink[3])
    };
    // Threshold: the ink tone against the level; paper or K 100 % only.
    check(
        &description("threshold", r#"{"level":0.3}"#),
        &|ink| {
            if tone(ink) >= 0.3 {
                [0.0; 4]
            } else {
                [0.0, 0.0, 0.0, 1.0]
            }
        },
        0,
    );
    // Gradient Map: three stops, a half-opaque middle, reversed.
    let stops = [
        (0.0, [0.0, 0.0, 0.0, 0.0], 1.0),
        (0.4, [0.0, 1.0, 0.0, 0.2], 0.5),
        (1.0, [1.0, 0.0, 1.0, 0.0], 1.0),
    ];
    check(
        &description(
            "gradient_map",
            r#"{"gradient":{"stops":[{"position":0,"ink":[0,0,0,0]},{"position":0.4,"ink":[0,1,0,0.2],"opacity":0.5},{"position":1,"ink":[1,0,1,0]}],"reverse":true},"dither":false}"#,
        ),
        &|ink| {
            let t = 1.0 - tone(ink).clamp(0.0, 1.0);
            let (a, b) = if t < 0.4 {
                (stops[0], stops[1])
            } else {
                (stops[1], stops[2])
            };
            let f = ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
            let opacity = a.2 + (b.2 - a.2) * f;
            std::array::from_fn(|p| {
                let mapped = a.1[p] + (b.1[p] - a.1[p]) * f;
                ink[p] + (mapped - ink[p]) * opacity
            })
        },
        2,
    );
    free(doc);
}

/// `doc` with layer 0 painted `ink` (unit CMYKA) at full coverage on the
/// pixels `at`, untouched elsewhere.
fn painted(
    doc: *mut PrecisionDocument,
    n: usize,
    ink: [f64; 5],
    at: &[usize],
) -> *mut PrecisionDocument {
    let mut coverage = vec![0u16; n];
    for &i in at {
        coverage[i] = 65535;
    }
    let request = CString::new(format!(
        r#"{{"op":"paint_blend","layer":0,"ink":{ink:?},"opacity":1}}"#
    ))
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "paint {ink:?}: {}", take_err_string(err));
    free(doc);
    out
}

#[test]
fn threshold_writes_black_as_k_only_at_8_and_16_bits() {
    let (w, h) = (9u32, 7u32);
    let level = 0.45;
    let tone = |s: &[u16; 5]| {
        let ink = [0, 1, 2, 3].map(|c| unit(s[c]));
        (0.2126 * (1.0 - ink[0]) + 0.7152 * (1.0 - ink[1]) + 0.0722 * (1.0 - ink[2]))
            * (1.0 - ink[3])
    };
    let desc = description("threshold", r#"{"level":0.45}"#);
    for depth in [8u32, 16] {
        let pixels: Vec<[u16; 4]> = picture(w, h)
            .into_iter()
            .map(|p| [p[0], p[1], p[2], 65535])
            .collect();
        let mut doc = ink_doc_at(w, h, pixels, depth);
        // Opaque paper, K-only 100 %, a rich black, and K-only tints either
        // side of the level.
        for (ink, at) in [
            ([0.0, 0.0, 0.0, 0.0, 1.0], 0),
            ([0.0, 0.0, 0.0, 1.0, 1.0], 1),
            ([0.75, 0.68, 0.67, 0.9, 1.0], 2),
            ([0.0, 0.0, 0.0, 0.53, 1.0], 3),
            ([0.0, 0.0, 0.0, 0.57, 1.0], 4),
        ] {
            doc = painted(doc, (w * h) as usize, ink, &[at]);
        }
        let source = samples(doc, 0);
        assert!(
            source.iter().all(|s| s[4] == 65535),
            "{depth}: every pixel opaque"
        );
        assert!(tone(&source[3]) >= level && tone(&source[4]) < level);
        let destructive = applied(doc, &desc);
        let layered = add(doc, &desc, 1.0).unwrap_or_else(|e| panic!("{desc}: {e}"));
        let flat = edit(layered, r#"{"op":"flatten"}"#);
        for (leg, out) in [("destructive", destructive), ("layer", flat)] {
            for (i, (g, s)) in samples(out, 0).iter().zip(&source).enumerate() {
                let want = if tone(s) >= level {
                    [0, 0, 0, 0, 65535]
                } else {
                    [0, 0, 0, 65535, 65535]
                };
                assert_eq!(*g, want, "{depth}-bit {leg}: pixel {i} from {s:?}");
            }
        }
        for d in [destructive, layered, flat, doc] {
            free(d);
        }
    }
}

#[test]
fn replace_color_weights_by_ink_distance_and_edits_the_cmy_triple() {
    let (w, h) = (10u32, 8u32);
    let doc = ink_doc(w, h, picture(w, h));
    let source = samples(doc, 0);
    let target = source[5];
    let target_ink = [0, 1, 2, 3].map(|c| unit(target[c]));
    let fuzziness = 120.0;
    let desc = description(
        "replace_color",
        &format!(
            r#"{{"inks":[[{},{},{},{}]],"fuzziness":{fuzziness},"hue":60,"saturation":0.3}}"#,
            target_ink[0], target_ink[1], target_ink[2], target_ink[3]
        ),
    );
    let (cmy, _) = l_planes(&source);
    // The edit itself is Hue/Saturation's master on the L triple: the RGB
    // kernel's own answer on an sRGB document of those planes.
    let edited = rgb_result(
        w,
        h,
        cmy,
        &description("hue_saturation", r#"{"hue":60,"saturation":0.3}"#),
    );
    let out = applied(doc, &desc);
    let mut moved = 0;
    for (i, ((g, s), e)) in samples(out, 0).iter().zip(&source).zip(&edited).enumerate() {
        if s[4] == 0 {
            continue;
        }
        let d = (0..4)
            .map(|c| (255.0 * (unit(s[c]) - target_ink[c])).powi(2))
            .sum::<f64>()
            .sqrt();
        let weight = if d <= fuzziness / 2.0 {
            1.0
        } else if d < fuzziness {
            2.0 * (fuzziness - d) / fuzziness
        } else {
            0.0
        };
        for c in 0..3 {
            let ink = unit(s[c]);
            let want = code(ink + weight * ((1.0 - unit(e[c])) - ink));
            assert!(
                g[c].abs_diff(want) <= 2,
                "pixel {i} plate {c}: {} vs {want}",
                g[c]
            );
        }
        assert_eq!(g[3], s[3], "pixel {i}: K held");
        moved += usize::from(weight > 0.0);
    }
    assert!(
        moved > 1,
        "the fuzziness reaches more than the sampled pixel"
    );
    free(out);
    free(doc);
}

#[test]
fn identity_params_change_nothing() {
    let doc = ink_doc(4, 4, picture(4, 4));
    for desc in [
        description("bcs", r#"{"brightness":0,"contrast":0}"#),
        description("color_balance", "{}"),
        description("photo_filter", r#"{"density":0}"#),
        description("channel_mixer", "{}"),
        description("selective_color", r#"{"method":"absolute"}"#),
        description(
            "gradient_map",
            r#"{"gradient":{"stops":[{"position":0,"ink":[0,0,0,1],"opacity":0},{"position":1,"ink":[1,0,0,0],"opacity":0}]}}"#,
        ),
        description("replace_color", r#"{"inks":[[0,0,0,0]]}"#),
        description(
            "shadows_highlights",
            r#"{"shadows":{"amount":0},"highlights":{"amount":0}}"#,
        ),
    ] {
        let out = apply(doc, &desc).unwrap_or_else(|e| panic!("{desc}: {e}"));
        assert!(out.is_null(), "{desc} is an identity");
    }
    free(doc);
}

#[test]
fn a_record_at_opacity_one_equals_the_destructive_apply() {
    let (w, h) = (7u32, 6u32);
    let pixels: Vec<[u16; 4]> = picture(w, h)
        .into_iter()
        .map(|p| [p[0], p[1], p[2], 65535])
        .collect();
    let doc = ink_doc(w, h, pixels);
    for desc in [
        description("invert", "{}"),
        description("posterize", r#"{"levels":3}"#),
        description("color_balance", r#"{"highlights":{"cyan_red":-0.4}}"#),
        description("photo_filter", r#"{"ink":[0,0.5,1,0],"density":0.4}"#),
        description("channel_mixer", r#"{"yellow":{"y":0.7,"c":0.3}}"#),
        description("selective_color", r#"{"neutrals":{"k":0.2}}"#),
        description("threshold", r#"{"level":0.45}"#),
        description(
            "gradient_map",
            r#"{"gradient":{"stops":[{"position":0,"ink":[0,0,0,1]},{"position":1,"ink":[0,0,0,0]}]}}"#,
        ),
    ] {
        let destructive = applied(doc, &desc);
        let layered = add(doc, &desc, 1.0).unwrap_or_else(|e| panic!("{desc}: {e}"));
        let flat = edit(layered, r#"{"op":"flatten"}"#);
        assert_eq!(
            samples(flat, 0),
            samples(destructive, 0),
            "{desc}: the record at opacity 1 is the destructive result"
        );
        for d in [destructive, layered, flat] {
            free(d);
        }
    }
    free(doc);
}

fn render(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    let mut err = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "{}", take_err_string(err));
    let pixels = unsafe { &*image }.to_rgba16().unwrap();
    unsafe { dr_precision_image_free(image) };
    pixels
}

#[test]
fn five_new_records_reopen_bit_exact() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (6u32, 5u32);
    let mut doc = ink_doc(w, h, picture(w, h));
    for (desc, opacity) in [
        (
            description("channel_mixer", r#"{"cyan":{"c":0.5,"k":0.5}}"#),
            0.8,
        ),
        (description("selective_color", r#"{"reds":{"m":0.3}}"#), 1.0),
        (
            description(
                "gradient_map",
                r#"{"gradient":{"stops":[{"position":0,"ink":[0.2,0,0,1]},{"position":1,"ink":[0,0,0.1,0]}]}}"#,
            ),
            0.5,
        ),
        (
            description("photo_filter", r#"{"ink":[0.1,0.5,0.9,0],"density":0.3}"#),
            1.0,
        ),
        (description("threshold", r#"{"level":0.6}"#), 0.4),
    ] {
        let next = add(doc, &desc, opacity).unwrap_or_else(|e| panic!("{desc}: {e}"));
        free(doc);
        doc = next;
    }
    let path = dir.path().join("records.rz");
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    assert!(
        unsafe { dr_precision_doc_save_native(doc, c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) },
        "{}",
        take_err_string(err)
    );
    let again = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!again.is_null(), "{}", take_err_string(err));
    let (a, b) = (unsafe { &*doc }, unsafe { &*again });
    assert_eq!(a.layer_count(), 6);
    assert_eq!(b.layer_count(), 6);
    for i in 0..6 {
        assert_eq!(
            a.cmyk_layer_samples(i),
            b.cmyk_layer_samples(i),
            "record {i}"
        );
        assert_eq!(
            a.adjustment_description(i),
            b.adjustment_description(i),
            "record {i}"
        );
        assert_eq!(a.layer(i).unwrap().opacity(), b.layer(i).unwrap().opacity());
    }
    assert_eq!(
        render(doc),
        render(again),
        "the composite reopens bit-exact"
    );
    free(doc);
    free(again);
}

#[test]
fn every_refusal_names_itself() {
    let doc = ink_doc(3, 3, picture(3, 3));
    let disabled = [
        ("exposure", r#"{"exposure":1}"#, "Exposure is unavailable in a CMYK document"),
        ("vibrance", r#"{"vibrance":0.5}"#, "Vibrance is unavailable in a CMYK document"),
        ("black_and_white", "{}", "Black & White is unavailable in a CMYK document"),
        ("hue_rotate", r#"{"degrees":30}"#, "Hue Rotate is unavailable in a CMYK document: Doctored offers it on RGB documents only"),
        ("grayscale", "{}", "Grayscale is unavailable in a CMYK document: Doctored offers it on RGB documents only"),
        ("sepia", "{}", "Sepia is unavailable in a CMYK document: Doctored offers it on RGB documents only"),
        ("white_balance", r#"{"temperature":5000}"#, "White Balance is unavailable in a CMYK document: Doctored offers it on RGB documents only"),
        ("bcs", r#"{"saturation":0.5}"#, "Saturation in Brightness/Contrast is unavailable in a CMYK document: use Hue/Saturation"),
        ("photo_filter", r##"{"color":"#ec8a00"}"##, "The RGB \"color\" key (give Photo Filter's \"ink\") is unavailable in a CMYK document"),
        ("channel_mixer", r#"{"red":{"r":1}}"#, "The RGB channel key \"red\" is unavailable in a CMYK document"),
        ("replace_color", r##"{"colors":["#ff0000"]}"##, "The RGB \"colors\" key (give Replace Color's \"inks\") is unavailable in a CMYK document"),
        (
            "gradient_map",
            r##"{"gradient":{"stops":[{"position":0,"color":"#000000"},{"position":1,"color":"#ffffff"}]}}"##,
            "A gradient stop \"color\" (give the stop's \"ink\") is unavailable in a CMYK document",
        ),
        ("equalize", "{}", "The \"equalize\" adjustment is unavailable in a CMYK document"),
    ];
    for (op, params, sentence) in disabled {
        let desc = description(op, params);
        assert_eq!(
            apply(doc, &desc).unwrap_err(),
            sentence,
            "{desc} destructive"
        );
        assert_eq!(
            add(doc, &desc, 1.0).unwrap_err(),
            sentence,
            "{desc} as a layer"
        );
    }
    // Pixels only: refused as a layer, admitted destructively.
    let layerless = [
        (
            description("shadows_highlights", "{}"),
            "A Shadows/Highlights adjustment layer is unavailable in a CMYK document: Shadows/Highlights applies to pixels only",
        ),
        (
            description("replace_color", r#"{"inks":[[0,1,1,0]],"hue":40}"#),
            "A Replace Color adjustment layer is unavailable in a CMYK document: Replace Color applies to pixels only",
        ),
    ];
    let levels = add(doc, &description("levels", r#"{"gamma":2}"#), 1.0).unwrap();
    for (desc, sentence) in &layerless {
        assert_eq!(add(doc, desc, 1.0).unwrap_err(), *sentence, "{desc}");
        let text = CString::new(desc.as_str()).unwrap();
        let mut err = ptr::null_mut();
        let set = unsafe {
            dr_precision_doc_set_adjustment(
                levels,
                1,
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert!(set.is_null());
        assert_eq!(take_err_string(err), *sentence, "{desc} set");
        assert!(apply(doc, desc).is_ok(), "{desc} applies to pixels");
    }
    free(levels);
    free(doc);
}
