//! The Inner Shadow composites INSIDE the layer on its partial-alpha ring
//! (`style_inside_layer`, #2's IK1) at 8 bits (`dr_doc_set_layer_style` +
//! `dr_doc_flattened`), 16 bits and 32 bits (`dr_precision_doc_edit`
//! `layer_style`).
//!
//! The oracle is the rule written here in f64, never read off the
//! implementation. Per pixel, with `a` the layer's shape / 255, `cov` the
//! shadow plane's byte, `T0` the stack after the layer's Below group and
//! before its pixels (never a knockout's backdrop) and `acc` the pixel as the
//! layer left it, in premultiplied space:
//!
//! - `X = T0 + (acc − T0) / a`, the layer at full coverage (colour divided by
//!   the unclamped alpha, then alpha and colour clamped to [0, 1], colour
//!   left unclamped at 32 bits);
//! - `X' = over(X, ink, min(1, cov / shape) · opacity)` with the shadow's
//!   blend;
//! - the pixel becomes `acc + a · (X' − X)`.
//!
//! Size 0, Choke 0 and Distance 0 make the plane the closed form `cov =
//! round((255 − A) · A / 255)` for a column of alpha `A`, so every column
//! below is its own case: transparent, opaque and fourteen partial alphas,
//! every one over an opaque backdrop. Over-the-composite, the rule before
//! this one, lands the shadow at `cov / 255` over `acc` and misses by about
//! 25 codes at `A = 128`.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 16;
const H: u32 = 3;
/// The layer's alpha per column.
const ALPHAS: [u8; W as usize] = [
    0, 255, 128, 64, 192, 32, 224, 16, 240, 100, 160, 200, 50, 8, 247, 170,
];
const BACK: [u8; 3] = [40, 160, 220];
const MIDDLE: [u8; 3] = [90, 200, 60];
const SOURCE: [u8; 3] = [220, 60, 30];
const LIGHT: ([u8; 3], &str) = ([250, 230, 20], "#fae614");
const DARK: ([u8; 3], &str) = ([30, 20, 140], "#1e148c");

// ------------------------------------------------------------ the oracle --

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Normal,
    Multiply,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Multiply => "multiply",
        }
    }

    fn blend(self) -> BlendMode {
        match self {
            Mode::Normal => BlendMode::Normal,
            Mode::Multiply => BlendMode::Multiply,
        }
    }

    /// `B(cb, cs)`.
    fn apply(self, cb: f64, cs: f64) -> f64 {
        match self {
            Mode::Normal => cs,
            Mode::Multiply => cb * cs,
        }
    }
}

#[derive(Clone, Copy)]
struct Shadow {
    ink: ([u8; 3], &'static str),
    mode: Mode,
    opacity: f64,
}

struct Case {
    name: &'static str,
    shadows: &'static [Shadow],
    fill: f64,
    layer: Mode,
    opacity: f64,
    knockout: bool,
}

const NORMAL: Shadow = Shadow {
    ink: LIGHT,
    mode: Mode::Normal,
    opacity: 1.0,
};
const MULTIPLY: Shadow = Shadow {
    ink: DARK,
    mode: Mode::Multiply,
    opacity: 1.0,
};
const SOFT: Shadow = Shadow {
    ink: LIGHT,
    mode: Mode::Normal,
    opacity: 0.6,
};

const fn case(name: &'static str, shadows: &'static [Shadow]) -> Case {
    Case {
        name,
        shadows,
        fill: 1.0,
        layer: Mode::Normal,
        opacity: 1.0,
        knockout: false,
    }
}

const CASES: [Case; 8] = [
    case("normal", &[NORMAL]),
    case("multiply shadow", &[MULTIPLY]),
    Case {
        fill: 0.5,
        ..case("fill 50", &[NORMAL])
    },
    Case {
        fill: 0.0,
        ..case("fill 0", &[NORMAL])
    },
    Case {
        layer: Mode::Multiply,
        ..case("multiply layer", &[NORMAL])
    },
    // 0.6 is a byte (153 / 255), so the package mixes at it at every depth.
    Case {
        opacity: 0.6,
        ..case("package", &[NORMAL])
    },
    Case {
        fill: 0.5,
        knockout: true,
        ..case("shallow knockout", &[NORMAL])
    },
    // Array order is render order: the soft Normal shadow, then Multiply.
    case("stacked", &[SOFT, MULTIPLY]),
];

type Px = [f64; 4];

fn unit(c: [u8; 3]) -> [f64; 3] {
    c.map(|v| f64::from(v) / 255.0)
}

fn opaque(c: [f64; 3]) -> Px {
    [c[0], c[1], c[2], 1.0]
}

/// IEC 61966-2.1 decode: an authored colour in a 32-bit document's light.
fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// A Fill below 32 bits composites at its byte, `round(255 · v) / 255`.
fn byte_unit(v: f64) -> f64 {
    (v * 255.0 + 0.5 + 1e-6).floor() / 255.0
}

/// The shadow plane at Size 0, Choke 0, Distance 0: the inverted shape times
/// the shape, rounded to a byte.
fn cov(alpha: u8) -> f64 {
    let a = u32::from(alpha);
    f64::from(((255 - a) * a + 127) / 255)
}

/// The W3C source-over of straight `cs` at `sa` onto straight `b` with
/// `mode`.
fn over(b: Px, cs: [f64; 3], sa: f64, mode: Mode) -> Px {
    let ab = b[3];
    let ao = sa + ab * (1.0 - sa);
    if ao <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] =
            (sa * (1.0 - ab) * cs[c] + sa * ab * mode.apply(b[c], cs[c]) + (1.0 - sa) * ab * b[c])
                / ao;
    }
    out[3] = ao;
    out
}

/// `p + (q − p) · t` in premultiplied space, straight out.
fn mix(p: Px, q: Px, t: f64) -> Px {
    let alpha = p[3] + (q[3] - p[3]) * t;
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = (p[c] * p[3] + (q[c] * q[3] - p[c] * p[3]) * t) / alpha;
    }
    out[3] = alpha;
    out
}

/// The rule (module doc) for one shadow at one pixel of shape byte `shape`
/// and plane byte `cov`.
#[allow(clippy::too_many_arguments)]
fn inside(
    t0: Px,
    now: Px,
    shape: f64,
    cov: f64,
    ink: [f64; 3],
    opacity: f64,
    mode: Mode,
    clamp: bool,
) -> Px {
    let a = shape / 255.0;
    let limit = |v: f64| if clamp { v.clamp(0.0, 1.0) } else { v };
    let xa = t0[3] + (now[3] - t0[3]) / a;
    let mut x = [0.0; 4];
    if xa > 0.0 {
        for c in 0..3 {
            x[c] = limit((t0[c] * t0[3] + (now[c] * now[3] - t0[c] * t0[3]) / a) / xa);
        }
        x[3] = xa.min(1.0);
    }
    let shadowed = over(x, ink, (cov / shape).min(1.0) * opacity, mode);
    let alpha = now[3] + a * (shadowed[3] - x[3]);
    if alpha <= 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = limit((now[c] * now[3] + a * (shadowed[c] * shadowed[3] - x[c] * x[3])) / alpha);
    }
    out[3] = alpha.min(1.0);
    out
}

/// The pixel of a column of alpha `alpha` at `depth`.
fn want(case: &Case, alpha: u8, depth: u32) -> Px {
    let float = depth == 32;
    let ink = |c: [u8; 3]| {
        if float {
            unit(c).map(decode)
        } else {
            unit(c)
        }
    };
    let fill = if float {
        case.fill
    } else {
        byte_unit(case.fill)
    };
    let a = f64::from(alpha) / 255.0;
    let back = opaque(unit(BACK));
    let t0 = if case.knockout {
        opaque(unit(MIDDLE))
    } else {
        back
    };
    // The layer's pixels at the package's opacity 1. A Shallow knockout at
    // the root punches to the Background: `K = over(Background, C, fill)`
    // mixed in by the shape.
    let mut now = if case.knockout {
        mix(t0, over(back, unit(SOURCE), fill, case.layer), a)
    } else if fill > 0.0 {
        over(t0, unit(SOURCE), a * fill, case.layer)
    } else {
        t0
    };
    let c = cov(alpha);
    for s in case.shadows.iter().filter(|_| c > 0.0) {
        now = inside(
            t0,
            now,
            f64::from(alpha),
            c,
            ink(s.ink.0),
            s.opacity,
            s.mode,
            !float,
        );
    }
    if case.opacity < 1.0 {
        mix(t0, now, case.opacity)
    } else {
        now
    }
}

fn shadow_json(s: &Shadow) -> Value {
    json!({"type":"inner_shadow","blend":s.mode.name(),"color":s.ink.1,"opacity":s.opacity,
           "use_global_light":false,"angle":120,"distance":0,"size":0,"choke":0,"noise":0})
}

fn style(case: &Case) -> Value {
    let effects: Vec<Value> = case.shadows.iter().map(shadow_json).collect();
    let mut style = json!({"fill_opacity": case.fill, "effects": effects});
    if case.knockout {
        style["knockout"] = json!("shallow");
    }
    style
}

// ------------------------------------------------------------- renders --

fn render8(case: &Case) -> Vec<Px> {
    let solid = |c: [u8; 3]| RgbaImage::from_pixel(W, H, Rgba([c[0], c[1], c[2], 255]));
    let mut doc = DrDocument::from_pixels(solid(BACK));
    if case.knockout {
        doc = doc
            .adding_image_layer(0, solid(MIDDLE), "Middle")
            .expect("middle");
    }
    let top = doc.layers.len();
    let layer = RgbaImage::from_fn(W, H, |x, _| {
        Rgba([SOURCE[0], SOURCE[1], SOURCE[2], ALPHAS[x as usize]])
    });
    doc = doc
        .adding_image_layer(top - 1, layer, "Layer")
        .expect("layer");
    if case.layer != Mode::Normal {
        doc = doc
            .with_layer_blend_mode(top, case.layer.blend())
            .expect("blend");
    }
    if case.opacity < 1.0 {
        doc = doc
            .with_layer_opacity(top, case.opacity as f32)
            .expect("opacity");
    }
    let doc = common::styled(&doc, top, &style(case).to_string());
    let handle = Box::into_raw(Box::new(doc));
    let out = common::flat_pixels(handle);
    unsafe { doctored_core::ffi_doc::dr_doc_free(handle) };
    out.chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|k| f64::from(p[k]) / 255.0))
        .collect()
}

fn take(err: *mut c_char) -> String {
    let text = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(err) };
    text
}

fn edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
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
    assert!(err.is_null(), "{request}: {}", take(err));
    assert!(!out.is_null(), "{request} changed nothing");
    *unsafe { Box::from_raw(out) }
}

fn typed_styled(doc: &PrecisionDocument, layer: usize, style: Value) -> PrecisionDocument {
    edit(
        doc,
        json!({"op":"layer_style","version":1,"layer":layer,"style":style}),
    )
}

fn image16(f: impl Fn(u32) -> [u16; 4]) -> PrecisionImage {
    let pixels = (0..W * H).map(|i| f(i % W)).collect();
    PrecisionImage::from_rgba16(W, H, pixels, IccProfile::srgb(), None).unwrap()
}

fn promoted(c: [u8; 3], alpha: u16) -> [u16; 4] {
    [
        257 * u16::from(c[0]),
        257 * u16::from(c[1]),
        257 * u16::from(c[2]),
        alpha,
    ]
}

fn render16(case: &Case) -> Vec<Px> {
    let solid = |c: [u8; 3]| image16(|_| promoted(c, 65535));
    let mut layers = vec![Layer::raster("Back", solid(BACK), (0, 0), 1.0, true).unwrap()];
    if case.knockout {
        layers.push(Layer::raster("Middle", solid(MIDDLE), (0, 0), 1.0, true).unwrap());
    }
    let pixels = image16(|x| promoted(SOURCE, 257 * u16::from(ALPHAS[x as usize])));
    let top = Layer::raster("Layer", pixels, (0, 0), case.opacity as f32, true)
        .unwrap()
        .with_blend_mode(case.layer.blend())
        .unwrap();
    layers.push(top);
    let index = layers.len() - 1;
    let doc = PrecisionDocument::from_parts(
        W,
        H,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    typed_styled(&doc, index, style(case))
        .render(LIMIT, 0, None)
        .unwrap()
        .to_rgba16()
        .unwrap()
        .into_iter()
        .map(|p| p.map(|v| f64::from(v) / 65535.0))
        .collect()
}

fn render32(case: &Case) -> Vec<Px> {
    let profile = IccProfile::srgb();
    let solid = |c: [u8; 3]| {
        let u = unit(c).map(|v| v as f32);
        vec![[u[0], u[1], u[2], 1.0]; (W * H) as usize]
    };
    let mut records = vec![solid(BACK)];
    if case.knockout {
        records.push(solid(MIDDLE));
    }
    let s = unit(SOURCE).map(|v| v as f32);
    records.push(
        (0..W * H)
            .map(|i| {
                [
                    s[0],
                    s[1],
                    s[2],
                    f32::from(ALPHAS[(i % W) as usize]) / 255.0,
                ]
            })
            .collect(),
    );
    let index = records.len() - 1;
    let mut doc = float_doc(W, H, &profile, &records);
    if case.layer != Mode::Normal {
        doc = edit(
            &doc,
            json!({"op":"layer_blend","version":1,"layer":index,"blend_mode":case.layer.blend().to_c()}),
        );
    }
    if case.opacity < 1.0 {
        doc = doc
            .set_layer_opacity(index, case.opacity as f32)
            .unwrap()
            .unwrap();
    }
    let doc = typed_styled(&doc, index, style(case));
    assert_eq!(doc.float_refusal(), None, "{}", case.name);
    doc.render(LIMIT, 0, None)
        .unwrap()
        .to_rgba32()
        .unwrap()
        .into_iter()
        .map(|p| p.map(f64::from))
        .collect()
}

/// Every pixel of `got` against the oracle within `bound` (unit values).
fn assert_case(case: &Case, depth: u32, got: &[Px], bound: f64) {
    assert_eq!(got.len(), (W * H) as usize);
    for (i, px) in got.iter().enumerate() {
        let x = i % W as usize;
        let want = want(case, ALPHAS[x], depth);
        for c in 0..4 {
            assert!(
                (px[c] - want[c]).abs() <= bound,
                "{} at {depth} bits, column {x} (alpha {}), channel {c}: got {}, want {} ({:.2} codes \
                 of 255 off)",
                case.name,
                ALPHAS[x],
                px[c],
                want[c],
                (px[c] - want[c]).abs() * 255.0
            );
        }
    }
}

#[test]
fn the_inner_shadow_composites_inside_the_layer_at_8_bits() {
    for case in &CASES {
        assert_case(case, 8, &render8(case), 1.0 / 255.0 + 1e-9);
    }
}

#[test]
fn the_inner_shadow_composites_inside_the_layer_at_16_bits() {
    for case in &CASES {
        assert_case(case, 16, &render16(case), 8.0 / 65535.0);
    }
}

#[test]
fn the_inner_shadow_composites_inside_the_layer_at_32_bits() {
    for case in &CASES {
        assert_case(case, 32, &render32(case), 1e-3);
    }
}

/// A 16-bit layer whose alpha (32996) is not a `× 257` code: the shadow reads
/// the byte shape (128) and its plane (64), while the layer's own alpha and
/// colour stay what its pixels made — the shadow lands as a delta on the
/// pixel as it stands, so the alpha is the shadow-off render's, sample for
/// sample, over a transparent canvas.
#[test]
fn a_16_bit_alpha_off_the_byte_grid_keeps_its_own_alpha() {
    const CODE: u16 = 32996;
    let render = |enabled: bool| {
        let doc = PrecisionDocument::from_parts(
            W,
            H,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            vec![Layer::raster(
                "Layer",
                image16(|_| promoted(SOURCE, CODE)),
                (0, 0),
                1.0,
                true,
            )
            .unwrap()],
        )
        .unwrap();
        // A style whose only effect is disabled is no style: the edit
        // changes nothing, so the shadow-off render is the plain document.
        let doc = if enabled {
            typed_styled(&doc, 0, json!({ "effects": [shadow_json(&NORMAL)] }))
        } else {
            doc
        };
        doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
    };
    let (on, off) = (render(true), render(false));
    let shape = (f64::from(CODE) / 257.0).round();
    assert_eq!(shape, 128.0);
    let s = unit(SOURCE);
    let now = [s[0], s[1], s[2], f64::from(CODE) / 65535.0];
    let want = inside(
        [0.0; 4],
        now,
        shape,
        cov(128),
        unit(LIGHT.0),
        1.0,
        Mode::Normal,
        true,
    );
    for (i, (got, plain)) in on.iter().zip(&off).enumerate() {
        assert_eq!(plain[3], CODE, "sample {i}: the shadow-off alpha");
        assert_eq!(got[3], plain[3], "sample {i}: the alpha is the layer's own");
        for c in 0..3 {
            let g = f64::from(got[c]) / 65535.0;
            assert!(
                (g - want[c]).abs() <= 8.0 / 65535.0,
                "sample {i} channel {c}: got {g}, want {} ({:.1} codes of 65535 off)",
                want[c],
                (g - want[c]).abs() * 65535.0
            );
        }
    }
}
