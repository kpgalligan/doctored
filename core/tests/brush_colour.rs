//! Brush dynamics' colour (`dr_brush_colour_create`, `dr_brush_colour_ink`,
//! `dr_brush_u16_coloured`, `dr_brush_overlay_rgba8_coloured`;
//! `brush_colour`, `brush_coverage` THE COLOURED STROKE), black-box through
//! the C ABI against an oracle written here from the rule alone: its own
//! SplitMix64 over the positional stream `(seed, stamp, dab, 3)`, its own
//! HSL (the standard hexcone written in binary64), its own Lab rule (the mix
//! on all three codes, brightness on the first alone), and its own
//! premultiplied accumulation `P += c·(ink − P)` beside `F += c·(1 − F)` over
//! its own dab coverage (soft: the smoothstep at the pixel centre; hard: the
//! mean of 8×8 samples).
use crate::native_scalar::{error, text, LIMIT};
use doctored_core::ffi_brush::*;
use doctored_core::ffi_brush_colour::*;
use doctored_core::ffi_brush_dual::*;
use doctored_core::ffi_brush_texture::*;
use serde_json::{json, Value};
use std::ptr;

const W: u32 = 40;
const H: u32 = 30;
const N: usize = (W * H) as usize;

const RANGE: &str = "native brush colour holds a foreground and a background of three values from 0 to 1 and a mix, hue, saturation and brightness from 0 to 1";
const LAB: &str =
    "native brush colour in the lab model takes a mix and a brightness; its hue and saturation are 0";
const INKS: &str = "native brush inks hold three values from 0 to 1 per dab";

/// The colour handle, freed on drop.
struct Colour(*mut DrBrushColour);

impl Drop for Colour {
    fn drop(&mut self) {
        unsafe { dr_brush_colour_free(self.0) }
    }
}

fn create(request: Value) -> Result<Colour, String> {
    let t = text(request);
    let mut e = ptr::null_mut();
    let c = unsafe { dr_brush_colour_create(t.as_ptr(), &mut e) };
    if c.is_null() {
        Err(unsafe { error(e) })
    } else {
        assert!(e.is_null());
        Ok(Colour(c))
    }
}

fn ink(c: &Colour, index: u64, dab: u32) -> [f64; 3] {
    let mut out = [-7.; 3];
    assert!(unsafe { dr_brush_colour_ink(c.0, index, dab, out.as_mut_ptr()) });
    out
}

// ---- the oracle ---------------------------------------------------------

/// SplitMix64 at the positional seed `seed ^ a·γ ^ b·m1 ^ c·m2`, unit values
/// from the top 53 bits.
struct Draws(u64);
impl Draws {
    fn at(seed: u64, a: u64, b: u64, c: u64) -> Self {
        Draws(
            seed ^ a.wrapping_mul(0x9E3779B97F4A7C15)
                ^ b.wrapping_mul(0xBF58476D1CE4E5B9)
                ^ c.wrapping_mul(0x94D049BB133111EB),
        )
    }
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        (z >> 11) as f64 / 9007199254740992.0
    }
}

/// The standard RGB → HSL (hue in degrees).
fn hsl(rgb: [f64; 3]) -> [f64; 3] {
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    let min = rgb[0].min(rgb[1]).min(rgb[2]);
    let l = (max + min) / 2.;
    if max == min {
        return [0., 0., l];
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2. - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == rgb[0] {
        (rgb[1] - rgb[2]) / d + if rgb[1] < rgb[2] { 6. } else { 0. }
    } else if max == rgb[1] {
        (rgb[2] - rgb[0]) / d + 2.
    } else {
        (rgb[0] - rgb[1]) / d + 4.
    };
    [h * 60., s, l]
}

/// The standard HSL → RGB through the `hue2rgb` helper.
fn rgb(hsl: [f64; 3]) -> [f64; 3] {
    let h = hsl[0].rem_euclid(360.) / 360.;
    let (s, l) = (hsl[1].clamp(0., 1.), hsl[2].clamp(0., 1.));
    if s == 0. {
        return [l; 3];
    }
    let q = if l < 0.5 { l * (1. + s) } else { l + s - l * s };
    let p = 2. * l - q;
    let channel = |t: f64| {
        let t = t.rem_euclid(1.);
        if t < 1. / 6. {
            p + (q - p) * 6. * t
        } else if t < 0.5 {
            q
        } else if t < 2. / 3. {
            p + (q - p) * (2. / 3. - t) * 6.
        } else {
            p
        }
    };
    [channel(h + 1. / 3.), channel(h), channel(h - 1. / 3.)]
}

#[derive(Clone, Copy)]
struct Amounts {
    mix: f64,
    hue: f64,
    saturation: f64,
    brightness: f64,
}

/// The rule's ink of dab `k` of stamp `i`.
fn oracle_ink(
    seed: u32,
    i: u64,
    k: u32,
    fg: [f64; 3],
    bg: [f64; 3],
    a: Amounts,
    lab: bool,
) -> [f64; 3] {
    let mut d = Draws::at(u64::from(seed), i, u64::from(k), 3);
    let v = [d.unit(), d.unit(), d.unit(), d.unit()];
    let c: [f64; 3] = std::array::from_fn(|ch| fg[ch] + (bg[ch] - fg[ch]) * a.mix * v[0]);
    let out = if lab {
        [c[0] * (1. - a.brightness * v[3]), c[1], c[2]]
    } else if a.hue == 0. && a.saturation == 0. && a.brightness == 0. {
        c
    } else {
        let [h, s, l] = hsl(c);
        rgb([
            h + 180. * a.hue * (2. * v[1] - 1.),
            s * (1. - a.saturation * v[2]),
            l * (1. - a.brightness * v[3]),
        ])
    };
    out.map(|v| v.clamp(0., 1.))
}

fn request_json(seed: u32, fg: [f64; 3], bg: [f64; 3], a: Amounts, lab: bool) -> Value {
    json!({"version":1,"model":if lab {"lab"} else {"rgb"},"seed":seed,"foreground":fg,
           "background":bg,"mix":a.mix,"hue":a.hue,"saturation":a.saturation,
           "brightness":a.brightness})
}

// ---- the ink rule ---------------------------------------------------------

#[test]
fn the_rgb_ink_follows_the_seeded_mix_and_hsl_jitter() {
    let (fg, bg) = ([0.85, 0.2, 0.1], [0.1, 0.3, 0.9]);
    let cases = [
        Amounts {
            mix: 0.7,
            hue: 0.,
            saturation: 0.,
            brightness: 0.,
        },
        Amounts {
            mix: 0.,
            hue: 0.4,
            saturation: 0.,
            brightness: 0.,
        },
        Amounts {
            mix: 0.,
            hue: 0.,
            saturation: 0.9,
            brightness: 0.,
        },
        Amounts {
            mix: 0.,
            hue: 0.,
            saturation: 0.,
            brightness: 0.6,
        },
        Amounts {
            mix: 0.5,
            hue: 0.25,
            saturation: 0.5,
            brightness: 0.35,
        },
    ];
    for (n, a) in cases.into_iter().enumerate() {
        for seed in [0_u32, 7, 4_000_000_000] {
            let c = create(request_json(seed, fg, bg, a, false)).unwrap();
            let mut moved = false;
            for i in [0_u64, 1, 2, 17, 1 << 40] {
                for k in 0..4 {
                    let got = ink(&c, i, k);
                    let want = oracle_ink(seed, i, k, fg, bg, a, false);
                    for ch in 0..3 {
                        assert!(
                            (got[ch] - want[ch]).abs() < 2e-6,
                            "case {n} seed {seed} stamp {i} dab {k}: {got:?} against {want:?}"
                        );
                    }
                    moved |= got != fg;
                }
            }
            assert!(moved, "case {n}: every dab kept the foreground");
        }
    }
}

#[test]
fn zero_amounts_give_the_foreground_bit_for_bit() {
    let fg = [0.123456789, 0.5, 0.987654321];
    let none = Amounts {
        mix: 0.,
        hue: 0.,
        saturation: 0.,
        brightness: 0.,
    };
    for lab in [false, true] {
        let c = create(request_json(11, fg, [1., 0., 0.], none, lab)).unwrap();
        for i in 0..6 {
            for k in 0..3 {
                assert_eq!(ink(&c, i, k), fg);
            }
        }
    }
    // Absent amounts and model are the defaults.
    let c = create(json!({"version":1,"foreground":fg,"background":[0.,0.,0.]})).unwrap();
    assert_eq!(ink(&c, 3, 1), fg);
}

#[test]
fn the_lab_model_mixes_the_codes_and_scales_the_l_code_alone() {
    let (fg, bg) = ([0.54, 0.81, 0.77], [0.2, 0.3, 0.6]);
    let a = Amounts {
        mix: 0.8,
        hue: 0.,
        saturation: 0.,
        brightness: 0.5,
    };
    let mix_only = Amounts {
        brightness: 0.,
        ..a
    };
    for seed in [0_u32, 99] {
        let c = create(request_json(seed, fg, bg, a, true)).unwrap();
        let mixed = create(request_json(seed, fg, bg, mix_only, true)).unwrap();
        for i in [0_u64, 3, 1000] {
            for k in 0..3 {
                let got = ink(&c, i, k);
                assert_eq!(got, oracle_ink(seed, i, k, fg, bg, a, true));
                // a* and b* are the mix's alone, for every draw.
                let m = ink(&mixed, i, k);
                assert_eq!([got[1], got[2]], [m[1], m[2]]);
                assert!(got[0] <= m[0]);
            }
        }
    }
    // A neutral grey under brightness alone keeps a* and b* exactly.
    let grey = [0.5, 128. / 255., 128. / 255.];
    let bright = Amounts {
        mix: 0.,
        hue: 0.,
        saturation: 0.,
        brightness: 0.9,
    };
    let c = create(request_json(5, grey, grey, bright, true)).unwrap();
    let mut darker = false;
    for i in 0..8 {
        let got = ink(&c, i, 0);
        assert_eq!([got[1], got[2]], [grey[1], grey[2]]);
        darker |= got[0] < grey[0];
    }
    assert!(darker);
}

#[test]
fn the_lab_model_refuses_hue_and_saturation() {
    let grey = [0.5; 3];
    for (hue, saturation) in [(0.1, 0.), (0., 0.2), (1., 1.)] {
        let a = Amounts {
            mix: 0.5,
            hue,
            saturation,
            brightness: 0.5,
        };
        assert_eq!(
            create(request_json(1, grey, grey, a, true)).err().unwrap(),
            LAB
        );
        assert!(create(request_json(1, grey, grey, a, false)).is_ok());
    }
}

#[test]
fn a_request_out_of_range_or_malformed_is_refused() {
    let ok = json!({"version":1,"foreground":[0.,0.,0.],"background":[1.,1.,1.]});
    assert!(create(ok.clone()).is_ok());
    for (key, value) in [
        ("foreground", json!([1.2, 0., 0.])),
        ("background", json!([0., -0.1, 0.])),
        ("mix", json!(1.5)),
        ("hue", json!(-0.5)),
        ("saturation", json!(2)),
        ("brightness", json!(1.0001)),
    ] {
        let mut r = ok.clone();
        r[key] = value;
        assert_eq!(create(r).err().unwrap(), RANGE, "{key}");
    }
    let mut r = ok.clone();
    r["version"] = json!(2);
    assert_eq!(
        create(r).err().unwrap(),
        "native brush colour requires version 1"
    );
    let mut r = ok.clone();
    r["jitter"] = json!(0.5);
    assert!(create(r)
        .err()
        .unwrap()
        .starts_with("invalid native brush colour request"));
    let mut r = ok.clone();
    r["model"] = json!("cmyk");
    assert!(create(r).is_err());
    let mut r = ok.clone();
    r["pad"] = json!("x".repeat(1100));
    assert!(create(r).is_err());
    let mut e = ptr::null_mut();
    assert!(unsafe { dr_brush_colour_create(ptr::null(), &mut e) }.is_null());
    assert!(!unsafe { error(e) }.is_empty());
}

// ---- the coloured stroke ------------------------------------------------

fn brush(hardness: f64, opacity: f64) -> Value {
    json!({"version":1,"hardness":hardness,"roundness":1.,"angle_degrees":0.,"pixel":false,
           "opacity":opacity})
}

/// `dr_brush_u16_coloured`: the coverage and the three ink planes, or its
/// refusal (which must leave the output untouched).
fn coloured(
    request: &Value,
    dabs: &[f64],
    shapes: Option<&[f64]>,
    texture: *const DrBrushTexture,
    dual: Option<(&Value, &[f64])>,
    inks: Option<&[f64]>,
) -> Result<(Vec<u16>, [Vec<u16>; 3]), String> {
    let t = text(request.clone());
    let d = dual.map(|(r, _)| text(r.clone()));
    let mut out = vec![777_u16; 4 * N];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_u16_coloured(
            W,
            H,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            shapes.map_or(ptr::null(), <[f64]>::as_ptr),
            texture,
            0,
            0,
            d.as_ref().map_or(ptr::null(), |d| d.as_ptr()),
            dual.map_or(ptr::null(), |(_, dabs)| dabs.as_ptr()),
            dual.map_or(0, |(_, dabs)| dabs.len() / 4),
            inks.map_or(ptr::null(), <[f64]>::as_ptr),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    if !ok {
        assert!(out.iter().all(|&v| v == 777), "a refusal wrote output");
        return Err(unsafe { error(e) });
    }
    let plane = |p: usize| out[p * N..(p + 1) * N].to_vec();
    Ok((plane(0), [plane(1), plane(2), plane(3)]))
}

fn plain(request: &Value, dabs: &[f64]) -> Vec<u16> {
    let t = text(request.clone());
    let mut out = vec![777; N];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_u16(
            W,
            H,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(ok, "{}", unsafe { error(e) });
    out
}

fn textured(
    request: &Value,
    dabs: &[f64],
    shapes: &[f64],
    texture: *const DrBrushTexture,
) -> Vec<u16> {
    let t = text(request.clone());
    let mut out = vec![777; N];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_u16_textured(
            W,
            H,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            shapes.as_ptr(),
            texture,
            0,
            0,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(ok, "{}", unsafe { error(e) });
    out
}

fn dual_plain(request: &Value, dabs: &[f64], second: &Value, second_dabs: &[f64]) -> Vec<u16> {
    let t = text(request.clone());
    let s = text(second.clone());
    let mut out = vec![777; N];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_u16_dual(
            W,
            H,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            ptr::null(),
            ptr::null(),
            0,
            0,
            s.as_ptr(),
            second_dabs.as_ptr(),
            second_dabs.len() / 4,
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(ok, "{}", unsafe { error(e) });
    out
}

/// A 5×3 tile, the texture of the coverage check.
fn texture() -> *mut DrBrushTexture {
    let texels: [[u8; 4]; 15] = [
        [0, 0, 0, 255],
        [255, 255, 255, 255],
        [128, 128, 128, 255],
        [255, 0, 0, 255],
        [0, 0, 255, 255],
        [64, 200, 30, 255],
        [255, 255, 255, 255],
        [10, 10, 10, 255],
        [200, 200, 200, 255],
        [0, 255, 0, 255],
        [90, 90, 90, 64],
        [255, 255, 255, 255],
        [30, 60, 90, 255],
        [250, 240, 230, 255],
        [5, 5, 5, 255],
    ];
    let rgba = texels.concat();
    let mut e = ptr::null_mut();
    let t = unsafe { dr_brush_texture_create(rgba.as_ptr(), 5, 3, 1., 0.7, &mut e) };
    assert!(!t.is_null());
    t
}

const DABS: [f64; 12] = [
    12.3, 14.7, 15., 1., //
    18.8, 16.2, 12.5, 0.65, //
    26.4, 15.9, 17., 0.9,
];

const INK3: [f64; 9] = [1., 0., 0.2, 0., 0.5, 1., 0.3, 0.9, 0.1];

#[test]
fn the_coverage_plane_is_the_uncoloured_strokes_bit_for_bit() {
    let t = texture();
    let shapes = [30., 0.4, -75., 0.9, 111.5, 0.6];
    let second = brush(0.5, 1.);
    let second_dabs = [11.6, 13.1, 6., 1., 19.9, 17.4, 5., 1., 29.7, 16.2, 8., 1.];
    for (hardness, opacity) in [(1., 1.), (0.4, 0.8), (0., 0.55)] {
        let r = brush(hardness, opacity);
        let (cov, _) = coloured(&r, &DABS, None, ptr::null(), None, Some(&INK3)).unwrap();
        assert_eq!(cov, plain(&r, &DABS), "plain {hardness}");
        let (cov, _) = coloured(&r, &DABS, Some(&shapes), t, None, Some(&INK3)).unwrap();
        assert_eq!(cov, textured(&r, &DABS, &shapes, t), "textured {hardness}");
        let (cov, _) = coloured(
            &r,
            &DABS,
            None,
            ptr::null(),
            Some((&second, &second_dabs)),
            Some(&INK3),
        )
        .unwrap();
        assert_eq!(
            cov,
            dual_plain(&r, &DABS, &second, &second_dabs),
            "dual {hardness}"
        );
    }
    unsafe { dr_brush_texture_free(t) };
}

/// One dab's coverage of pixel (x, y) before its deposit (round tip, angle
/// 0): soft, the smoothstep at the pixel centre; hard, the mean of 8×8
/// samples inside.
fn oracle_pixel(dab: &[f64], hardness: f64, x: u32, y: u32) -> f64 {
    let (cx, cy, r) = (dab[0], dab[1], dab[2] / 2.);
    let distance = |px: f64, py: f64| (px - cx).hypot(py - cy) / r;
    if hardness < 1. {
        let t = distance(f64::from(x) + 0.5, f64::from(y) + 0.5);
        if t <= hardness {
            1.
        } else if t >= 1. {
            0.
        } else {
            let k = (t - hardness) / (1. - hardness);
            1. - k * k * (3. - 2. * k)
        }
    } else {
        let mut inside = 0;
        for sy in 0..8 {
            for sx in 0..8 {
                let px = f64::from(x) + f64::from(2 * sx + 1) / 16.;
                let py = f64::from(y) + f64::from(2 * sy + 1) / 16.;
                if distance(px, py) <= 1. {
                    inside += 1;
                }
            }
        }
        f64::from(inside) / 64.
    }
}

/// The rule's field `F` and straight ink `S` per pixel.
fn oracle_stroke(hardness: f64, dabs: &[f64], inks: &[f64]) -> (Vec<f64>, Vec<[f64; 3]>) {
    let mut f = vec![0_f64; N];
    let mut p = vec![[0_f64; 3]; N];
    for (i, dab) in dabs.chunks_exact(4).enumerate() {
        for y in 0..H {
            for x in 0..W {
                let c = oracle_pixel(dab, hardness, x, y) * dab[3];
                let at = (y * W + x) as usize;
                f[at] += c * (1. - f[at]);
                for ch in 0..3 {
                    p[at][ch] += c * (inks[3 * i + ch] - p[at][ch]);
                }
            }
        }
    }
    let s = f
        .iter()
        .zip(&p)
        .map(|(f, p)| {
            if *f > 0. {
                p.map(|v| (v / f).clamp(0., 1.))
            } else {
                [0.; 3]
            }
        })
        .collect();
    (f, s)
}

#[test]
fn overlapping_dabs_of_different_inks_accumulate_by_coverage() {
    let dabs = [14., 15., 16., 1., 21.5, 15.5, 16., 0.8];
    let inks = [1., 0., 0.2, 0., 0.5, 1.];
    for hardness in [1., 0.3] {
        let r = brush(hardness, 1.);
        let (cov, planes) = coloured(&r, &dabs, None, ptr::null(), None, Some(&inks)).unwrap();
        let (f, s) = oracle_stroke(hardness, &dabs, &inks);
        let mut overlap = 0;
        for at in 0..N {
            if f[at] < 0.02 {
                continue;
            }
            assert!(cov[at] > 0);
            for ch in 0..3 {
                let want = (s[at][ch] * 65535.).round();
                let got = f64::from(planes[ch][at]);
                assert!(
                    (got - want).abs() <= 2.,
                    "hardness {hardness} pixel {at} channel {ch}: {got} against {want}"
                );
            }
            // Both inks show where both dabs laid paint.
            if s[at][0] > 0.05 && s[at][2] > 0.25 {
                overlap += 1;
            }
        }
        assert!(overlap > 10, "hardness {hardness}: no mixed overlap");
    }
}

#[test]
fn a_uniform_ink_reads_back_exactly_wherever_coverage_lands() {
    for value in [0., 1.] {
        let inks = [value; 9];
        for hardness in [1., 0.5] {
            let r = brush(hardness, 0.7);
            let (cov, planes) = coloured(&r, &DABS, None, ptr::null(), None, Some(&inks)).unwrap();
            let code = (value * 65535.) as u16;
            let mut painted = 0;
            for at in 0..N {
                if cov[at] == 0 {
                    continue;
                }
                painted += 1;
                for plane in &planes {
                    assert_eq!(plane[at], code, "value {value} hardness {hardness}");
                }
            }
            assert!(painted > 100);
        }
    }
    // Outside the footprint the ink planes are 0.
    let (_, planes) = coloured(
        &brush(1., 1.),
        &DABS,
        None,
        ptr::null(),
        None,
        Some(&[1.; 9]),
    )
    .unwrap();
    assert_eq!(planes[0][0], 0);
    assert_eq!(planes[2][N - 1], 0);
}

fn overlay(
    request: &Value,
    dabs: &[f64],
    inks: &[f64],
    selection: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let t = text(request.clone());
    let mut out = vec![77_u8; 4 * N];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_overlay_rgba8_coloured(
            W,
            H,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null(),
            0,
            inks.as_ptr(),
            selection.map_or(ptr::null(), <[u8]>::as_ptr),
            selection.map_or(0, <[u8]>::len),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    if ok {
        Ok(out)
    } else {
        assert!(out.iter().all(|&v| v == 77), "a refusal wrote output");
        Err(unsafe { error(e) })
    }
}

fn plain_overlay(request: &Value, dabs: &[f64], rgb: [u8; 3], selection: Option<&[u8]>) -> Vec<u8> {
    let t = text(request.clone());
    let mut out = vec![77_u8; 4 * N];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_overlay_rgba8(
            W,
            H,
            t.as_ptr(),
            dabs.as_ptr(),
            dabs.len() / 4,
            rgb.as_ptr(),
            selection.map_or(ptr::null(), <[u8]>::as_ptr),
            selection.map_or(0, <[u8]>::len),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(ok, "{}", unsafe { error(e) });
    out
}

#[test]
fn the_overlay_takes_each_pixels_ink_through_the_overlay_rule() {
    let selection: Vec<u8> = (0..N).map(|i| ((i * 37) % 256) as u8).collect();
    let r = brush(0.6, 0.9);
    // A uniform ink is the one-ink overlay of its bytes, selection or none.
    let inks = [0.2, 0.6, 1.0].repeat(3);
    for sel in [None, Some(selection.as_slice())] {
        assert_eq!(
            overlay(&r, &DABS, &inks, sel).unwrap(),
            plain_overlay(&r, &DABS, [51, 153, 255], sel)
        );
    }
    // Mixed inks: alpha is the coverage's (times the selection), each colour
    // byte the rule's premultiplication of that pixel's ink.
    let got = overlay(&r, &DABS, &INK3, Some(&selection)).unwrap();
    let (cov, planes) = coloured(&r, &DABS, None, ptr::null(), None, Some(&INK3)).unwrap();
    let round = |x: u32, d: u32| (x + (d - 1) / 2) / d;
    for at in 0..N {
        let mut alpha = round(u32::from(cov[at]) * 255, 65535);
        alpha = round(alpha * u32::from(selection[at]), 255);
        assert_eq!(u32::from(got[4 * at + 3]), alpha, "alpha {at}");
        for ch in 0..3 {
            let ink = round(u32::from(planes[ch][at]) * 255, 65535);
            let want = round(ink * alpha, 255);
            assert!(
                u32::from(got[4 * at + ch]).abs_diff(want) <= 1,
                "pixel {at} channel {ch}"
            );
        }
    }
}

#[test]
fn an_inks_list_that_is_missing_or_out_of_range_is_refused() {
    let r = brush(1., 1.);
    assert_eq!(
        coloured(&r, &DABS, None, ptr::null(), None, None).unwrap_err(),
        INKS
    );
    for bad in [1.5, -0.25, f64::NAN, f64::INFINITY] {
        let mut inks = INK3;
        inks[4] = bad;
        assert_eq!(
            coloured(&r, &DABS, None, ptr::null(), None, Some(&inks)).unwrap_err(),
            INKS
        );
        assert_eq!(overlay(&r, &DABS, &inks, None).unwrap_err(), INKS);
    }
    // An empty stroke needs no inks.
    let (cov, _) = coloured(&r, &[], None, ptr::null(), None, None).unwrap();
    assert!(cov.iter().all(|&v| v == 0));
    // A texture on a pixel tip refuses as the textured call does.
    let t = texture();
    let pixel = json!({"version":1,"hardness":1.,"roundness":1.,"angle_degrees":0.,
                       "pixel":true});
    assert_eq!(
        coloured(&pixel, &DABS, None, t, None, Some(&INK3)).unwrap_err(),
        "native brush texture requires a stamped tip that is not aliased"
    );
    unsafe { dr_brush_texture_free(t) };
    // The output's length is the four planes'.
    let tx = text(r.clone());
    let mut out = vec![5_u16; N];
    let mut e = ptr::null_mut();
    let ok = unsafe {
        dr_brush_u16_coloured(
            W,
            H,
            tx.as_ptr(),
            DABS.as_ptr(),
            3,
            ptr::null(),
            ptr::null(),
            0,
            0,
            ptr::null(),
            ptr::null(),
            0,
            INK3.as_ptr(),
            out.as_mut_ptr(),
            out.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(!ok);
    assert_eq!(
        unsafe { error(e) },
        "native brush output length differs from canvas"
    );
    assert!(out.iter().all(|&v| v == 5));
}
