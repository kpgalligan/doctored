//! #1 phase 3: a Lab Color document composites every mode it offers
//! by its Lab arm (`lab_blend`, take2 of
//! `scripts/photoshop-probe/lab_blend_take.py`), at 8 and 16 bits, through
//! every path that blends: a layer's render, Merge Down, Flatten, a layer at
//! Fill 40 %, a Brush stroke (`paint_blend`), Edit ▸ Fade, the Clone Stamp, a
//! Color Overlay and a drop shadow (both effect inks). Each is checked
//! against the independent f64 oracle `common/lab_blend_oracle.rs`, written
//! from the runner's declaration. Bounds, declared before measuring: 2/65535
//! per channel at 16 bits; one 8-bit code at 8 (|code/65535 − oracle|·255 ≤
//! 1).
//!
//! `take2_reads_back_in_place` reads take2's files where they lie
//! (`~/devel/doctored-fixtures/photoshop/lab-blend-681/take2`; never
//! committed; SKIPS when absent): each mode row rebuilt from the layered
//! PSD's own Base and row-layer pixels at the cell centres, rendered, and
//! held to the take's gate against Photoshop's composite TIFF — at 8 bits
//! the render's 8-bit code within one code, at 16 bits within 257/65535.

use std::ffi::CString;
use std::path::PathBuf;
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi_fade::dr_precision_doc_fade;
use doctored_core::ffi_lab::dr_precision_doc_new_lab;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::common::take_err_string;
use crate::gray_ops::{edit, free, plan_samples, result, LIMIT};
use crate::lab_blend_oracle as oracle;
use crate::lab_fixture::srgb_to_lab;
use crate::native_blend_oracle::dissolve_threshold;

/// Every mode a Lab document offers bar Normal, as C codes, menu order.
const MODES: [i32; 18] = [
    14, 1, 15, 16, 2, 12, 17, 3, 4, 5, 18, 19, 20, 21, 23, 24, 25, 26,
];

/// The take's (backdrop, source) pairs, sRGB bytes.
const PAIRS: [([u8; 3], [u8; 3]); 12] = [
    ([200, 40, 40], [40, 160, 220]),
    ([40, 160, 60], [220, 60, 160]),
    ([230, 200, 30], [60, 80, 220]),
    ([128, 126, 130], [230, 40, 40]),
    ([40, 120, 200], [140, 142, 138]),
    ([20, 30, 25], [200, 120, 40]),
    ([245, 240, 230], [60, 30, 90]),
    ([30, 20, 60], [250, 245, 200]),
    ([180, 80, 200], [90, 200, 120]),
    ([255, 0, 0], [0, 255, 0]),
    ([90, 200, 120], [230, 230, 228]),
    ([60, 60, 64], [20, 40, 60]),
];

const W: u32 = PAIRS.len() as u32;

/// The Lab twin, from a new Lab document.
fn twin() -> Arc<IccProfile> {
    let request = CString::new(r#"{"depth":16,"fill_srgb":[1,1,1,1]}"#).unwrap();
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_new_lab(1, 1, request.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!doc.is_null(), "{}", take_err_string(err));
    let profile = unsafe { &*doc }.profile().clone();
    free(doc);
    profile
}

/// A unit triple as stored codes at `bits` (8 bits on the ×257 grid).
fn codes_of(u: [f64; 3], bits: u32) -> [u16; 4] {
    let c = |v: f64| {
        let v = v.clamp(0.0, 1.0);
        if bits == 8 {
            (v * 255.0).round() as u16 * 257
        } else {
            (v * 65535.0).round() as u16
        }
    };
    [c(u[0]), c(u[1]), c(u[2]), 65535]
}

fn unit_of(p: [u16; 4]) -> [f64; 3] {
    [0, 1, 2].map(|c| f64::from(p[c]) / 65535.0)
}

/// sRGB bytes as Lab unit codes (the hand-computed CIE L\*a\*b\*).
fn lab_unit(rgb: [u8; 3]) -> [f64; 3] {
    let lab = srgb_to_lab(rgb.map(|v| f64::from(v) / 255.0));
    [lab[0] / 100.0, oracle::unit(lab[1]), oracle::unit(lab[2])]
}

/// The pairs' backdrops (`source` false) or sources as codes at `bits`.
fn row(bits: u32, source: bool) -> Vec<[u16; 4]> {
    PAIRS
        .iter()
        .map(|p| codes_of(lab_unit(if source { p.1 } else { p.0 }), bits))
        .collect()
}

fn backdrops(bits: u32) -> Vec<[u16; 4]> {
    row(bits, false)
}

fn sources(bits: u32) -> Vec<[u16; 4]> {
    row(bits, true)
}

/// A Lab document at `bits` of one row per layer, bottom-first.
fn lab_doc(bits: u32, width: u32, layers: &[(&str, Vec<[u16; 4]>, f32)]) -> *mut PrecisionDocument {
    let twin = twin();
    let records = layers
        .iter()
        .map(|(name, pixels, opacity)| {
            let image =
                PrecisionImage::from_rgba16(width, 1, pixels.clone(), twin.clone(), None).unwrap();
            Layer::raster(name, image, (0, 0), *opacity, true).unwrap()
        })
        .collect();
    let doc = PrecisionDocument::from_parts(
        width,
        1,
        twin,
        Metadata::default(),
        Resolution::default(),
        records,
    )
    .unwrap()
    .with_lab_depth(bits)
    .unwrap();
    Box::into_raw(Box::new(doc))
}

fn then(doc: *mut PrecisionDocument, request: Value) -> *mut PrecisionDocument {
    let out = edit(doc, &request, &[])
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"));
    free(doc);
    out
}

fn with_mode(doc: *mut PrecisionDocument, layer: usize, mode: i32) -> *mut PrecisionDocument {
    if mode == 0 {
        return doc;
    }
    then(
        doc,
        json!({"op":"layer_blend","version":1,"layer":layer,"blend_mode":mode}),
    )
}

fn with_fill(doc: *mut PrecisionDocument, layer: usize, fill: f64) -> *mut PrecisionDocument {
    then(
        doc,
        json!({"op":"layer_style","version":1,"layer":layer,"style":{"fill_opacity":fill}}),
    )
}

/// A two-layer document: the backdrops under the sources at `opacity` in
/// `mode`.
fn pair_doc(bits: u32, mode: i32, opacity: f32) -> *mut PrecisionDocument {
    let doc = lab_doc(
        bits,
        W,
        &[
            ("Backdrop", backdrops(bits), 1.0),
            ("Source", sources(bits), opacity),
        ],
    );
    with_mode(doc, 1, mode)
}

fn render(doc: *const PrecisionDocument) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn layer_codes(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .unwrap()
        .to_rgba16()
        .unwrap()
}

/// The module doc's bound.
fn assert_near(got: [u16; 4], want: [f64; 3], bits: u32, what: &str) {
    for c in 0..3 {
        let g = f64::from(got[c]);
        let ok = if bits == 16 {
            (g - want[c] * 65535.0).abs() <= 2.0
        } else {
            (g / 65535.0 - want[c]).abs() * 255.0 <= 1.0 + 1e-9
        };
        assert!(
            ok,
            "{what} channel {c}: {g} against {} ({bits} bits)",
            want[c] * 65535.0
        );
    }
}

/// Hard Mix away from its threshold: the oracle's f64 and the kernel's f32
/// agree only where a channel's sum is clear of 1 (or an exact 8-bit tie).
fn assert_clear_of_hard_mix_tie(bits: u32, cb: [f64; 3], cs: [f64; 3], what: &str) {
    if bits == 8 {
        return;
    }
    let y = |u: f64| 0.5 + oracle::star(u) / 255.0;
    let margins = [
        (cb[0] + cs[0] - 1.0).abs(),
        (y(cb[1]) + y(cs[1]) - 1.0).abs(),
        (y(cb[2]) + y(cs[2]) - 1.0).abs(),
    ];
    assert!(
        margins.iter().all(|m| *m > 1e-5),
        "{what} on the tie {margins:?}"
    );
}

/// Checks `out` (the composite of `doc_kind` per pixel) against the oracle
/// for `mode` at `opacity` and Fill `fill`.
fn assert_row(out: &[[u16; 4]], bits: u32, mode: i32, opacity: f64, fill: f64, what: &str) {
    let (base, top) = (backdrops(bits), sources(bits));
    for x in 0..W as usize {
        let what = format!("{what} mode {mode} opacity {opacity} fill {fill} x {x}");
        assert_eq!(out[x][3], 65535, "{what} alpha");
        if mode == 14 {
            // Dissolve: the source whole, or the backdrop untouched.
            let shown = out[x][..3] == top[x][..3];
            assert!(shown || out[x][..3] == base[x][..3], "{what}");
            let t = dissolve_threshold(x as i64, 0);
            if (opacity * fill - t).abs() >= 1e-4 {
                assert_eq!(shown, t < opacity * fill, "{what} t {t}");
            }
            continue;
        }
        let (cb, cs) = (unit_of(base[x]), unit_of(top[x]));
        if mode == 21 {
            assert_clear_of_hard_mix_tie(bits, cb, cs, &what);
        }
        let want = oracle::composite(mode, bits == 8, cb, cs, opacity, fill);
        assert_near(out[x], want, bits, &what);
    }
}

#[test]
fn every_lab_mode_composites_a_layer_by_its_arm_at_8_and_16_bits() {
    for bits in [8, 16] {
        for mode in MODES {
            for opacity in [1.0, 0.6] {
                let doc = pair_doc(bits, mode, opacity as f32);
                assert_row(&render(doc), bits, mode, opacity, 1.0, "render");
                assert!(unsafe { &*doc }.is_lab());
                free(doc);
            }
        }
    }
}

#[test]
fn merge_down_and_flatten_composite_by_the_lab_arms() {
    for bits in [8, 16] {
        for mode in MODES {
            let doc = pair_doc(bits, mode, 0.6);
            let merged = edit(doc, &json!({"op":"merge_down","layer":1}), &[])
                .unwrap()
                .unwrap();
            assert_row(&layer_codes(merged, 0), bits, mode, 0.6, 1.0, "merge down");
            let flat = edit(doc, &json!({"op":"flatten"}), &[]).unwrap().unwrap();
            assert_row(&layer_codes(flat, 0), bits, mode, 0.6, 1.0, "flatten");
            for d in [flat, merged, doc] {
                free(d);
            }
        }
    }
}

#[test]
fn a_layer_at_fill_40_folds_linear_burn_dodge_and_vivid_light() {
    for bits in [8, 16] {
        for mode in [15, 12, 18, 4, 1, 19, 21, 23] {
            let doc = with_fill(pair_doc(bits, mode, 1.0), 1, 0.4);
            assert_row(&render(doc), bits, mode, 1.0, 0.4, "fill 40");
            free(doc);
        }
    }
}

/// `request` on `doc` with full coverage of every planned input.
fn covered(doc: *const PrecisionDocument, request: &Value) -> *mut PrecisionDocument {
    let n = plan_samples(doc, request).unwrap_or_else(|e| panic!("{request}: {e}"));
    edit(doc, request, &vec![65535; n])
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap_or_else(|| panic!("{request} changed nothing"))
}

#[test]
fn a_brush_stroke_composites_by_the_lab_arm() {
    let colour = [0.2, 0.55, 0.85, 1.0];
    let brush = |mode: i32| {
        json!({"op":"paint_blend","layer":0,"color":colour,"color_space":"srgb",
            "opacity":1.0,"blend_mode":mode})
    };
    for bits in [8, 16] {
        let doc = lab_doc(bits, W, &[("Backdrop", backdrops(bits), 1.0)]);
        let ink = covered(doc, &brush(0));
        let ink_codes = layer_codes(ink, 0);
        for mode in [1, 2, 15, 12, 18, 16, 17, 23, 24, 25, 26] {
            let painted = covered(doc, &brush(mode));
            let out = layer_codes(painted, 0);
            for x in 0..W as usize {
                let (cb, cs) = (unit_of(backdrops(bits)[x]), unit_of(ink_codes[x]));
                let want = oracle::composite(mode, bits == 8, cb, cs, 1.0, 1.0);
                assert_near(out[x], want, bits, &format!("brush mode {mode} x {x}"));
            }
            free(painted);
        }
        free(ink);
        free(doc);
    }
}

fn fade(
    after: *const PrecisionDocument,
    before: *const PrecisionDocument,
    mode: i32,
) -> *mut PrecisionDocument {
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_fade(
            after,
            0,
            before,
            mode,
            1.0,
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    result(out, err)
        .unwrap_or_else(|e| panic!("fade {mode}: {e}"))
        .unwrap_or_else(|| panic!("fade {mode} changed nothing"))
}

#[test]
fn fade_composites_by_the_lab_arm() {
    for bits in [8, 16] {
        let before = lab_doc(bits, W, &[("Layer", backdrops(bits), 1.0)]);
        let after = lab_doc(bits, W, &[("Layer", sources(bits), 1.0)]);
        for mode in [1, 2, 15, 12, 18, 21, 16, 17, 23, 24, 25, 26] {
            let faded = fade(after, before, mode);
            assert_row(&layer_codes(faded, 0), bits, mode, 1.0, 1.0, "fade");
            free(faded);
        }
        free(after);
        free(before);
    }
}

#[test]
fn the_clone_stamp_composites_by_the_lab_arm() {
    // The left half is the backdrops, the right half the sources; the clone
    // maps each left pixel to the source W to its right (the matrix maps
    // destination centres to source centres) under coverage of the left half.
    for bits in [8, 16] {
        let mut pixels = backdrops(bits);
        pixels.extend(sources(bits));
        let doc = lab_doc(bits, 2 * W, &[("Layer", pixels, 1.0)]);
        for mode in [1, 2, 15, 12, 18, 21, 16, 17, 23, 24, 25, 26] {
            let request = json!({"op":"retouch_clone","layer":0,
                "matrix":[1,0,0,1,f64::from(W),0],"opacity":1.0,"blend_mode":mode});
            let n = plan_samples(doc, &request).unwrap_or_else(|e| panic!("{request}: {e}"));
            let mut input: Vec<u16> = (0..2 * W).map(|x| if x < W { 65535 } else { 0 }).collect();
            input.resize(n, 65535);
            let cloned = edit(doc, &request, &input)
                .unwrap_or_else(|e| panic!("{request}: {e}"))
                .unwrap_or_else(|| panic!("clone {mode} changed nothing"));
            assert_row(
                &layer_codes(cloned, 0)[..W as usize],
                bits,
                mode,
                1.0,
                1.0,
                "clone",
            );
            free(cloned);
        }
        free(doc);
    }
}

/// `doc` with one effect on layer `layer`.
fn styled(doc: *const PrecisionDocument, layer: usize, effect: Value) -> *mut PrecisionDocument {
    let request = json!({"op":"layer_style","version":1,"layer":layer,
        "style":{"effects":[effect]}});
    edit(doc, &request, &[])
        .unwrap_or_else(|e| panic!("{request}: {e}"))
        .unwrap()
}

fn effect_name(mode: i32) -> &'static str {
    match mode {
        0 => "normal",
        1 => "multiply",
        2 => "screen",
        12 => "linear_dodge",
        15 => "linear_burn",
        16 => "darker_color",
        17 => "lighter_color",
        18 => "vivid_light",
        21 => "hard_mix",
        23 => "hue",
        24 => "saturation",
        25 => "color",
        26 => "luminosity",
        _ => panic!("mode {mode}"),
    }
}

#[test]
fn a_color_overlay_composites_by_the_lab_arm() {
    for bits in [8, 16] {
        let doc = lab_doc(bits, W, &[("Layer", backdrops(bits), 1.0)]);
        let overlay = |mode: i32| {
            json!({"type":"color_overlay","color":"#3a8fd0","blend":effect_name(mode),
                "opacity":1.0})
        };
        let normal = styled(doc, 0, overlay(0));
        let ink = render(normal);
        for mode in [1, 2, 15, 12, 18, 21, 16, 17, 23, 24, 25, 26] {
            let moded = styled(doc, 0, overlay(mode));
            let out = render(moded);
            for x in 0..W as usize {
                let (cb, cs) = (unit_of(backdrops(bits)[x]), unit_of(ink[x]));
                let want = oracle::composite(mode, bits == 8, cb, cs, 1.0, 1.0);
                assert_near(out[x], want, bits, &format!("overlay mode {mode} x {x}"));
            }
            free(moded);
        }
        free(normal);
        free(doc);
    }
}

#[test]
fn a_drop_shadow_composites_by_the_lab_arm() {
    // A hard shadow (size 0) 3 px to the right of a 4 px top layer: where the
    // top layer is transparent and the Normal shadow lands, the shadow's
    // colour is that Normal composite, and a moded shadow is the oracle over
    // the backdrop.
    let width = W;
    for bits in [8, 16] {
        let top: Vec<[u16; 4]> = (0..width)
            .map(|x| {
                if x < 4 {
                    [30000, 40000, 20000, 65535]
                } else {
                    [0; 4]
                }
            })
            .collect();
        let doc = lab_doc(
            bits,
            width,
            &[
                ("Backdrop", backdrops(bits), 1.0),
                ("Top", top.clone(), 1.0),
            ],
        );
        let shadow = |mode: i32| {
            json!({"type":"drop_shadow","blend":effect_name(mode),"color":"#204080",
                "opacity":1.0,"angle":180,"use_global_light":false,"distance":3,"spread":0,
                "size":0})
        };
        let bare = render(doc);
        let normal = styled(doc, 1, shadow(0));
        let under = render(normal);
        let shaded: Vec<usize> = (0..width as usize)
            .filter(|&x| top[x][3] == 0 && under[x] != bare[x])
            .collect();
        assert_eq!(shaded.len(), 3, "the shadow's visible pixels {shaded:?}");
        for mode in [1, 2, 15, 12, 18, 21, 16, 23, 25, 26] {
            let moded = styled(doc, 1, shadow(mode));
            let out = render(moded);
            for &x in &shaded {
                let (cb, cs) = (unit_of(backdrops(bits)[x]), unit_of(under[x]));
                let want = oracle::composite(mode, bits == 8, cb, cs, 1.0, 1.0);
                assert_near(out[x], want, bits, &format!("shadow mode {mode} x {x}"));
            }
            free(moded);
        }
        free(normal);
        free(doc);
    }
}

// ------------------------------------------------------------- take2 --

/// The take's modes: (menu name, C code).
const TAKE_MODES: [(&str, i32); 18] = [
    ("Dissolve", 14),
    ("Multiply", 1),
    ("Linear Burn", 15),
    ("Screen", 2),
    ("Linear Dodge (Add)", 12),
    ("Overlay", 3),
    ("Soft Light", 4),
    ("Hard Light", 5),
    ("Vivid Light", 18),
    ("Linear Light", 19),
    ("Pin Light", 20),
    ("Hard Mix", 21),
    ("Darker Color", 16),
    ("Lighter Color", 17),
    ("Hue", 23),
    ("Saturation", 24),
    ("Color", 25),
    ("Luminosity", 26),
];
const FILL_MODES: [&str; 4] = [
    "Linear Burn",
    "Linear Dodge (Add)",
    "Vivid Light",
    "Soft Light",
];

#[test]
fn take2_reads_back_in_place() {
    let folder = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("devel/doctored-fixtures/photoshop/lab-blend-681/take2/jobs/blend");
    if !folder.join("blend16_composite.tif").is_file() {
        eprintln!(
            "SKIP take2_reads_back_in_place: {} is absent",
            folder.display()
        );
        return;
    }
    let mut cells = 0;
    for bits in [8u32, 16] {
        let psd = crate::cmyk_psd_fixture::read(
            &std::fs::read(folder.join(format!("blend{bits}.psd"))).unwrap(),
        );
        assert_eq!(u32::from(psd.depth), bits);
        let tiff = oracle::read_cielab_tiff(
            &std::fs::read(folder.join(format!("blend{bits}_composite.tif"))).unwrap(),
        );
        assert_eq!(u32::from(tiff.bits), bits);
        // A record's unit code at canvas (x, y).
        let pixel = |name: &str, x: i32, y: i32| -> [u16; 4] {
            let r = psd.by_name(name);
            let [top, left, _, right] = r.bounds;
            let i = ((y - top) * (right - left) + x - left) as usize;
            let u = [0usize, 1, 2]
                .map(|c| oracle::psd_unit(psd.depth, c, r.plane(c as i16).unwrap()[i]));
            codes_of(u, bits)
        };
        let photoshop = |x: i32, y: i32| -> [f64; 3] {
            let i = (y as u32 * tiff.width + x as u32) as usize;
            [0, 1, 2].map(|c| oracle::cielab_unit(tiff.bits, c, tiff.planes[c][i]))
        };
        for (k, (name, mode)) in TAKE_MODES.iter().enumerate() {
            let mut rows = vec![(2 + 2 * k as i32, 100, 100), (3 + 2 * k as i32, 60, 100)];
            if let Some(f) = FILL_MODES.iter().position(|m| m == name) {
                rows.push((2 + 2 * TAKE_MODES.len() as i32 + f as i32, 100, 40));
            }
            for (r, opacity, fill) in rows {
                let layer = if fill < 100 {
                    format!("{name} Fill {fill}")
                } else {
                    format!("{name} {opacity}")
                };
                let y = 8 * r + 4;
                let xs: Vec<i32> = (0..12).map(|c| 8 * c + 4).collect();
                let base: Vec<[u16; 4]> = xs.iter().map(|&x| pixel("Base", x, y)).collect();
                let source: Vec<[u16; 4]> = xs.iter().map(|&x| pixel(&layer, x, y)).collect();
                let doc = lab_doc(
                    bits,
                    12,
                    &[
                        ("Base", base.clone(), 1.0),
                        ("Source", source.clone(), opacity as f32 / 100.0),
                    ],
                );
                let mut doc = with_mode(doc, 1, *mode);
                if fill < 100 {
                    doc = with_fill(doc, 1, f64::from(fill) / 100.0);
                }
                let out = render(doc);
                for (c, &x) in xs.iter().enumerate() {
                    let what = format!("take2 {bits} bits {layer} column {c}");
                    let ps = photoshop(x, y);
                    let near = |p: [u16; 4]| -> bool {
                        (0..3).all(|k| {
                            if bits == 8 {
                                ((f64::from(p[k]) / 257.0).round() - ps[k] * 255.0).abs()
                                    <= 1.0 + 1e-6
                            } else {
                                (f64::from(p[k]) - ps[k] * 65535.0).abs() <= 257.0
                            }
                        })
                    };
                    if *mode == 14 && opacity < 100 {
                        // Dissolve's dither is its own: ours and Photoshop's
                        // each show the backdrop or the source.
                        assert!(out[c][..3] == base[c][..3] || out[c][..3] == source[c][..3]);
                        assert!(near(base[c]) || near(source[c]), "{what}: {ps:?}");
                    } else {
                        assert!(near(out[c]), "{what}: {:?} against {ps:?}", out[c]);
                    }
                    cells += 1;
                }
                free(doc);
            }
        }
    }
    assert_eq!(cells, 2 * 12 * (2 * TAKE_MODES.len() + FILL_MODES.len()));
}
