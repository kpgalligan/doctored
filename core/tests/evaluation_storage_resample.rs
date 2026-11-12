//! The resampling edits that write a layer's stored pixels, on a
//! linear-light document: Puppet Warp, Perspective Warp, Free Transform /
//! Puppet Warp / Perspective Warp of selected pixels and Liquify mix layer
//! colour in premultiplied LINEAR light (11E ADR, D7) at 8 and 16 bits,
//! against `linear_oracle`. An encoded document is untouched.
//!
//! Two oracles, neither calling the crate's colour or sampling code:
//!
//! * A half-pixel move of `halves` to the right has closed-form taps
//!   (Catmull-Rom at t = ½: −1/16, 9/16, 9/16, −1/16 over x−2…x+1; bilinear
//!   ½, ½ over x−1, x), so each interior column is
//!   `encode(clamp(Σ wᵢ·lin(tapᵢ)))`. The same sum over the CODES must match
//!   the encoded document first, which proves the taps without the change.
//! * Liquify's field has no closed-form taps, so its check is FEASIBILITY:
//!   every pixel of an opaque two-colour picture is `w·A + (1−w)·B` with one
//!   w for all three channels, in whichever domain the mix ran. Each
//!   channel's stored code ± (the gate + half a code) bounds w to an
//!   interval in a model's domain (a code at either end also stands for
//!   every value the clamp folds onto it), and the model holds where the
//!   three intervals meet. The linear model must hold at every pixel of the
//!   linear document and the code model fail at some; the reverse on the
//!   encoded one. The bounds below are declared before any run.
//!
//! What still copies codes — the move, an integer translation, Nearest, a
//! quarter turn, a pixel Liquify leaves in place — must, bit for bit.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{
    dr_image_free, dr_image_from_rgba, dr_image_height, dr_image_pixels_rgba, dr_image_width,
};
use doctored_core::ffi_filters::{dr_doc_filter_op, dr_image_filter_op};
use doctored_core::ffi_perspective_warp::{
    dr_doc_perspective_warp, dr_precision_doc_perspective_warp,
};
use doctored_core::ffi_puppet::{dr_doc_puppet_warp, dr_precision_doc_puppet_warp};
use doctored_core::ffi_selected_adjustment::dr_precision_doc_selected_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::DrImage;
use image::RgbaImage;
use serde_json::{json, Value};

use crate::common::selected_pixels::{edit16, edit8, take_err};
use crate::linear_oracle::{decode, encode, lin, linear, typed_encode, typed_linear, TYPED_GATE};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const A: [u8; 3] = [30, 60, 200];
const B: [u8; 3] = [220, 180, 40];
/// The fixture the closed-form cases move: `halves(W, H)`, seam at W/2.
const W: u32 = 40;
const H: u32 = 16;

/// Catmull-Rom at t = ½, taps x−2…x+1.
const CUBIC: [(i64, f64); 4] = [(-2, -0.0625), (-1, 0.5625), (0, 0.5625), (1, -0.0625)];
/// Bilinear at t = ½, taps x−1, x.
const BILINEAR: [(i64, f64); 2] = [(-1, 0.5), (0, 0.5)];

/// Liquify's declared floors: pixels mixed from both colours, and pixels at
/// which the wrong domain's model fails.
const MIN_MIXED: usize = 40;
const MIN_TELLING: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Depth {
    D8,
    D16,
}

const DEPTHS: [Depth; 2] = [Depth::D8, Depth::D16];

impl Depth {
    fn max(self) -> f64 {
        match self {
            Depth::D8 => 255.0,
            Depth::D16 => 65535.0,
        }
    }

    /// Byte value `v` as this depth's stored code (the lattice `257·v`).
    fn code(self, v: u8) -> u16 {
        match self {
            Depth::D8 => u16::from(v),
            Depth::D16 => u16::from(v) * 257,
        }
    }

    /// `linear_oracle`'s gate, in this depth's codes.
    fn gate(self) -> f64 {
        match self {
            Depth::D8 => 1.0,
            Depth::D16 => TYPED_GATE as f64,
        }
    }

    /// A linear value as the code the linear-light store makes.
    fn encode(self, linear: f64) -> f64 {
        match self {
            Depth::D8 => f64::from(encode(linear)),
            Depth::D16 => f64::from(typed_encode(linear)),
        }
    }
}

/// A layer read back: its canvas rect and straight samples in stored codes.
struct Layer {
    rect: (i64, i64, u32, u32),
    px: Vec<[u16; 4]>,
}

impl Layer {
    fn at(&self, x: i64, y: i64) -> Option<[u16; 4]> {
        let (lx, ly) = (x - self.rect.0, y - self.rect.1);
        if lx < 0 || ly < 0 || lx >= i64::from(self.rect.2) || ly >= i64::from(self.rect.3) {
            return None;
        }
        Some(self.px[ly as usize * self.rect.2 as usize + lx as usize])
    }
}

enum Doc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}

/// One opaque-or-not layer covering a `size` canvas, `f` in byte values.
fn doc(depth: Depth, size: (u32, u32), f: impl Fn(u32, u32) -> [u8; 4]) -> Doc {
    match depth {
        Depth::D8 => Doc::Byte(DrDocument::from_pixels(RgbaImage::from_fn(
            size.0,
            size.1,
            |x, y| image::Rgba(f(x, y)),
        ))),
        Depth::D16 => doc16(size, |x, y| f(x, y).map(|v| Depth::D16.code(v))),
    }
}

/// A 16-bit document whose layer holds `f`'s codes as given.
fn doc16(size: (u32, u32), f: impl Fn(u32, u32) -> [u16; 4]) -> Doc {
    let samples = (0..size.0 * size.1)
        .map(|i| f(i % size.0, i / size.0))
        .collect();
    let image = PrecisionImage::from_rgba16(size.0, size.1, samples, IccProfile::srgb(), None);
    Doc::Typed(
        PrecisionDocument::from_image(image.unwrap(), Metadata::default(), Resolution::default())
            .unwrap(),
    )
}

/// A byte fixture value as 16-bit codes off the 257 lattice (RGB moved by
/// 1…120 codes toward mid-grey; alpha kept), so a decode and re-encode of
/// an untouched sample would show.
fn off_lattice(v: [u8; 4], x: u32, y: u32) -> [u16; 4] {
    let off = (x * 7 + y * 5) % 120 + 1;
    let rgb = [0, 1, 2].map(|c| {
        let code = u32::from(v[c]) * 257;
        (if code > 32768 { code - off } else { code + off }) as u16
    });
    [rgb[0], rgb[1], rgb[2], u16::from(v[3]) * 257]
}

/// Left half `A`, right half `B`, opaque.
fn halves(w: u32) -> impl Fn(u32, u32) -> [u8; 4] {
    move |x, _| {
        let c = if x < w / 2 { A } else { B };
        [c[0], c[1], c[2], 255]
    }
}

fn text(request: &Value) -> CString {
    CString::new(request.to_string()).unwrap()
}

impl Doc {
    fn depth(&self) -> Depth {
        match self {
            Doc::Byte(_) => Depth::D8,
            Doc::Typed(_) => Depth::D16,
        }
    }

    fn linear(&self) -> Doc {
        match self {
            Doc::Byte(d) => Doc::Byte(linear(d)),
            Doc::Typed(d) => Doc::Typed(typed_linear(d)),
        }
    }

    fn layer(&self) -> Layer {
        match self {
            Doc::Byte(d) => {
                let layer = &d.layers[0];
                let (w, h) = layer.pixels.dimensions();
                let px = layer.pixels.read_rect(0, 0, w, h).unwrap();
                Layer {
                    rect: (i64::from(layer.offset.0), i64::from(layer.offset.1), w, h),
                    px: px.iter().map(|p| p.map(u16::from)).collect(),
                }
            }
            Doc::Typed(d) => {
                let layer = d.layer(0).unwrap();
                let image = layer.source().unwrap();
                let (w, h) = image.dimensions();
                let (x, y) = layer.offset();
                Layer {
                    rect: (i64::from(x), i64::from(y), w, h),
                    px: image.to_rgba16().unwrap(),
                }
            }
        }
    }

    /// The document an edit export answered: `out` non-NULL, no message.
    fn adopt(&self, out: *mut u8, err: *mut c_char, what: &str) -> Doc {
        if let Some(message) = take_err(err) {
            panic!("{what} refused: {message}");
        }
        assert!(!out.is_null(), "{what} changed nothing");
        match self {
            Doc::Byte(_) => Doc::Byte(*unsafe { Box::from_raw(out.cast::<DrDocument>()) }),
            Doc::Typed(_) => Doc::Typed(*unsafe { Box::from_raw(out.cast::<PrecisionDocument>()) }),
        }
    }

    fn puppet(&self, request: &Value) -> Doc {
        let text = text(request);
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                Doc::Byte(d) => {
                    dr_doc_puppet_warp(d, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err).cast()
                }
                Doc::Typed(d) => {
                    dr_precision_doc_puppet_warp(d, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                        .cast()
                }
            }
        };
        self.adopt(out, err, "puppet warp")
    }

    fn perspective(&self, request: &Value) -> Doc {
        let text = text(request);
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                Doc::Byte(d) => {
                    dr_doc_perspective_warp(d, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                        .cast()
                }
                Doc::Typed(d) => dr_precision_doc_perspective_warp(
                    d,
                    text.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
                .cast(),
            }
        };
        self.adopt(out, err, "perspective warp")
    }

    /// A selected-pixels edit over `coverage` (canvas-sized u16).
    fn selected(&self, request: &Value, coverage: &[u16]) -> Doc {
        let text = request.to_string();
        match self {
            Doc::Byte(d) => Doc::Byte(edit8(d, &text, coverage, None).unwrap().expect("an edit")),
            Doc::Typed(d) => {
                Doc::Typed(edit16(d, &text, coverage, None).unwrap().expect("an edit"))
            }
        }
    }

    /// Filters ▸ Liquify on the whole layer: at 8 bits `dr_doc_filter_op`
    /// over the layer's pixels, at 16 the selected edit.
    fn liquify(&self, strokes: &Value) -> Layer {
        match self {
            Doc::Byte(d) => {
                let layer = self.layer();
                let img = byte_image(&layer);
                let pixels = filter(Some(d), img, "liquify", strokes).expect("liquify ran");
                unsafe { dr_image_free(img) };
                Layer {
                    rect: layer.rect,
                    px: pixels
                        .chunks_exact(4)
                        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c])))
                        .collect(),
                }
            }
            Doc::Typed(d) => {
                let request = json!({"layer": 0, "op": "liquify", "params": strokes});
                let text = text(&request);
                let mut err = ptr::null_mut();
                let out = unsafe {
                    dr_precision_doc_selected_edit(
                        d,
                        text.as_ptr(),
                        ptr::null(),
                        0,
                        LIMIT,
                        0,
                        ptr::null(),
                        &mut err,
                    )
                };
                self.adopt(out.cast(), err, "liquify").layer()
            }
        }
    }
}

/// A byte layer as a `DrImage` (the host's capture of its pixels).
fn byte_image(layer: &Layer) -> *mut DrImage {
    let raw: Vec<u8> = layer.px.iter().flat_map(|p| p.map(|v| v as u8)).collect();
    let img = unsafe { dr_image_from_rgba(raw.as_ptr(), layer.rect.2, layer.rect.3) };
    assert!(!img.is_null());
    img
}

/// `dr_doc_filter_op` (with a document) or `dr_image_filter_op` (without):
/// the bytes, `None` for the NULL bypass. Panics on a refusal.
fn filter(
    doc: Option<&DrDocument>,
    img: *const DrImage,
    op: &str,
    params: &Value,
) -> Option<Vec<u8>> {
    let (op, params) = (CString::new(op).unwrap(), text(params));
    let mut err = ptr::null_mut();
    let out = unsafe {
        match doc {
            Some(d) => {
                dr_doc_filter_op(d, img, op.as_ptr(), params.as_ptr(), ptr::null(), &mut err)
            }
            None => dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), ptr::null(), &mut err),
        }
    };
    if let Some(message) = take_err(err) {
        panic!("the filter refused: {message}");
    }
    if out.is_null() {
        return None;
    }
    let (w, h) = unsafe { (dr_image_width(out), dr_image_height(out)) };
    let bytes =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), (w * h * 4) as usize) }
            .to_vec();
    unsafe { dr_image_free(out) };
    Some(bytes)
}

// ------------------------------------------------------- closed-form taps --

/// The column a half-pixel move of `halves(W)` to the right writes at
/// canvas `x`: `taps`' sum per channel in linear light (`linear`) or over the
/// stored codes, clamped and stored.
fn column(depth: Depth, x: i64, taps: &[(i64, f64)], linear: bool) -> [f64; 3] {
    let colour = |sx: i64| if sx < i64::from(W / 2) { A } else { B };
    std::array::from_fn(|c| {
        let sum: f64 = taps
            .iter()
            .map(|&(k, weight)| {
                let v = colour(x + k)[c];
                weight
                    * if linear {
                        lin(v)
                    } else {
                        f64::from(depth.code(v))
                    }
            })
            .sum();
        if linear {
            depth.encode(sum.clamp(0.0, 1.0))
        } else {
            sum.clamp(0.0, depth.max()).round()
        }
    })
}

/// The two oracles disagree by more than twice the gate somewhere on the
/// seam, so the closed-form cases can tell the domains apart.
fn assert_discriminates(depth: Depth, taps: &[(i64, f64)]) {
    let apart = (0..i64::from(W)).any(|x| {
        let (l, c) = (column(depth, x, taps, true), column(depth, x, taps, false));
        (0..3).any(|k| (l[k] - c[k]).abs() > 2.0 * depth.gate())
    });
    assert!(
        apart,
        "{depth:?}: the fixture cannot tell linear from codes"
    );
}

/// Row `y` of `moved` (a half-pixel move of `halves(W)` to the right)
/// against [`column`] at every column whose taps lie inside the source.
fn assert_columns(moved: &Layer, depth: Depth, taps: &[(i64, f64)], linear: bool, what: &str) {
    let y = i64::from(H / 2);
    let reach = taps.iter().map(|t| -t.0).max().unwrap();
    let last = i64::from(W) - 1 - taps.iter().map(|t| t.0).max().unwrap();
    for x in reach..=last {
        let want = column(depth, x, taps, linear);
        let have = moved
            .at(x, y)
            .unwrap_or_else(|| panic!("{what}: ({x}, {y}) is outside {:?}", moved.rect));
        assert_eq!(
            f64::from(have[3]),
            depth.max(),
            "{what}: alpha at ({x}, {y})"
        );
        for c in 0..3 {
            assert!(
                (f64::from(have[c]) - want[c]).abs() <= depth.gate(),
                "{what} ({}): channel {c} at ({x}, {y}) is {} for {}",
                if linear { "linear light" } else { "codes" },
                have[c],
                want[c]
            );
        }
    }
}

/// `run` on the encoded and the linear-light `halves(W)` at `depth`, each
/// against its own domain's columns.
fn check_half_pixel(depth: Depth, taps: &[(i64, f64)], what: &str, run: impl Fn(&Doc) -> Layer) {
    assert_discriminates(depth, taps);
    let encoded = doc(depth, (W, H), halves(W));
    assert_columns(
        &run(&encoded),
        depth,
        taps,
        false,
        &format!("{what} {depth:?} encoded"),
    );
    let linear = encoded.linear();
    assert_columns(
        &run(&linear),
        depth,
        taps,
        true,
        &format!("{what} {depth:?} linear"),
    );
}

/// Five pins over the layer, each moved half a pixel to the right.
fn half_pins() -> Value {
    let pins: Vec<Value> = [
        [8.0, 4.0],
        [32.0, 4.0],
        [8.0, 12.0],
        [32.0, 12.0],
        [20.0, 8.0],
    ]
    .iter()
    .map(|p| json!({"from": p, "to": [p[0] + 0.5, p[1]]}))
    .collect();
    json!(pins)
}

/// One quad over the layer, every corner moved half a pixel to the right.
fn half_quads() -> Value {
    let from = [8.0, 4.0, 32.0, 4.0, 32.0, 12.0, 8.0, 12.0];
    let mut to = from;
    for x in to.iter_mut().step_by(2) {
        *x += 0.5;
    }
    json!([{"from": from, "to": to}])
}

fn full_coverage() -> Vec<u16> {
    vec![65535; (W * H) as usize]
}

#[test]
fn puppet_warp_mixes_a_half_pixel_seam_in_linear_light() {
    let request = json!({"op":"puppet_warp","layer":0,"mode":"normal","pins":half_pins()});
    for depth in DEPTHS {
        check_half_pixel(depth, &CUBIC, "puppet warp", |d| d.puppet(&request).layer());
    }
}

#[test]
fn perspective_warp_mixes_a_half_pixel_seam_in_linear_light() {
    let request = json!({"op":"perspective_warp","layer":0,"quads":half_quads()});
    for depth in DEPTHS {
        check_half_pixel(depth, &CUBIC, "perspective warp", |d| {
            d.perspective(&request).layer()
        });
    }
}

#[test]
fn a_transform_of_selected_pixels_mixes_a_half_pixel_seam_in_linear_light() {
    let request =
        json!({"op":"transform_selected","layer":0,"matrix":[1,0,0,1,0.5,0],"filter":"bilinear"});
    for depth in DEPTHS {
        check_half_pixel(depth, &BILINEAR, "transform_selected", |d| {
            d.selected(&request, &full_coverage()).layer()
        });
    }
}

#[test]
fn puppet_and_perspective_of_selected_pixels_mix_in_linear_light() {
    let puppet = json!({"op":"puppet_selected","layer":0,"mode":"normal","pins":half_pins()});
    let perspective = json!({"op":"perspective_selected","layer":0,"quads":half_quads()});
    for depth in DEPTHS {
        for (what, request) in [
            ("perspective_selected", &perspective),
            ("puppet_selected", &puppet),
        ] {
            check_half_pixel(depth, &CUBIC, what, |d| {
                d.selected(request, &full_coverage()).layer()
            });
        }
    }
}

/// A transparent white pixel column beside opaque dark ones: moved half a
/// pixel, every visible pixel is the dark colour — the hidden white never
/// mixes in, in either domain.
#[test]
fn a_transparent_neighbour_leaks_nothing_through_a_puppet_move() {
    let dark = [20u8, 30, 40];
    let request = json!({"op":"puppet_warp","layer":0,"mode":"normal","pins":half_pins()});
    for depth in DEPTHS {
        let source = doc(depth, (W, H), |x, _| {
            if x < 6 {
                [255, 255, 255, 0]
            } else {
                [dark[0], dark[1], dark[2], 255]
            }
        });
        let moved = source.linear().puppet(&request).layer();
        let mut partial = 0;
        for p in moved.px.iter().filter(|p| p[3] > 0) {
            for c in 0..3 {
                let want = f64::from(depth.code(dark[c]));
                assert!(
                    (f64::from(p[c]) - want).abs() <= depth.gate(),
                    "{depth:?}: {p:?} carries the hidden white"
                );
            }
            partial += usize::from(f64::from(p[3]) < depth.max());
        }
        assert!(
            partial > 0,
            "{depth:?}: no edge pixel was mixed with the transparent column"
        );
    }
}

/// The move, an integer translation, Nearest and a quarter turn permute
/// stored samples: the linear-light document gives the encoded document's
/// result bit for bit, partial alpha and partial coverage included.
#[test]
fn copies_of_selected_pixels_stay_copies() {
    let side = 24u32;
    let varied = |x: u32, y: u32| {
        [
            (x * 29 + 13) as u8,
            (y * 41 + 7) as u8,
            ((x + y) * 17 + 101) as u8,
            if (x + y).is_multiple_of(5) { 140 } else { 255 },
        ]
    };
    let coverage: Vec<u16> = (0..side * side)
        .map(|i| {
            let (x, y) = (i % side, i / side);
            match (x, y) {
                (4..=11, 6..=13) if x == 4 || y == 13 => 30000,
                (4..=11, 6..=13) => 65535,
                _ => 0,
            }
        })
        .collect();
    let requests = [
        json!({"op":"move_selected","layer":0,"dx":3,"dy":1}),
        json!({"op":"transform_selected","layer":0,"matrix":[1,0,0,1,3,1],"filter":"bilinear"}),
        json!({"op":"transform_selected","layer":0,"matrix":[0,1,-1,0,24,0],"filter":"bilinear"}),
        json!({"op":"transform_selected","layer":0,"matrix":[1.5,0,0,1.25,0.3,0.2],
               "filter":"nearest"}),
    ];
    // Off the 257 lattice, so a decode and encode would show.
    let typed = doc16((side, side), |x, y| off_lattice(varied(x, y), x, y));
    for encoded in [doc(Depth::D8, (side, side), varied), typed] {
        let switched = encoded.linear();
        for request in &requests {
            let (a, b) = (
                encoded.selected(request, &coverage).layer(),
                switched.selected(request, &coverage).layer(),
            );
            assert_eq!(a.rect, b.rect, "{request}");
            assert!(
                a.px == b.px,
                "{:?} {request}: the copy moved codes",
                encoded.depth()
            );
        }
    }
}

// ----------------------------------------------------------------- Liquify --

/// Liquify's picture: 4-pixel bands of `A` and `B`, opaque, so a brush
/// crosses several seams.
fn stripes(x: u32, _: u32) -> [u8; 4] {
    let c = if (x / 4).is_multiple_of(2) { A } else { B };
    [c[0], c[1], c[2], 255]
}

/// A push across the bands of a 64 × 24 [`stripes`], at partial strength so
/// the field lands between pixels.
fn push() -> Value {
    json!({"strokes": [{"tool": "forward_warp", "size": 30, "strength": 0.5, "density": 1,
                        "points": [[16, 12], [44, 12]]}]})
}

/// The interval of w a stored code `p` allows when the mix runs through `f`
/// (the encoded value's image in the model's domain), A at w = 1.
fn w_interval(depth: Depth, p: u16, a: u8, b: u8, f: impl Fn(f64) -> f64) -> (f64, f64) {
    let slack = depth.gate() + 0.5;
    let (low, high) = (f64::from(p) - slack, f64::from(p) + slack);
    let low = if low <= 0.0 {
        f64::NEG_INFINITY
    } else {
        f(low / depth.max())
    };
    let high = if high >= depth.max() {
        f64::INFINITY
    } else {
        f(high / depth.max())
    };
    let (fa, fb) = (f(f64::from(a) / 255.0), f(f64::from(b) / 255.0));
    let (w0, w1) = ((low - fb) / (fa - fb), (high - fb) / (fa - fb));
    if fa > fb {
        (w0, w1)
    } else {
        (w1, w0)
    }
}

/// Whether one w explains all three channels of `p` under `f`.
fn feasible(depth: Depth, p: [u16; 4], f: impl Fn(f64) -> f64 + Copy) -> bool {
    let (mut low, mut high) = (f64::NEG_INFINITY, f64::INFINITY);
    for c in 0..3 {
        let (l, h) = w_interval(depth, p[c], A[c], B[c], f);
        low = low.max(l);
        high = high.min(h);
    }
    low <= high
}

/// Every pixel of `out` is opaque and explained by `right`'s domain; at
/// least `MIN_MIXED` mix both colours and `MIN_TELLING` refuse `wrong`'s.
fn assert_domain(
    out: &Layer,
    depth: Depth,
    right: impl Fn(f64) -> f64 + Copy,
    wrong: impl Fn(f64) -> f64 + Copy,
    what: &str,
) {
    let (a, b) = (A.map(|v| depth.code(v)), B.map(|v| depth.code(v)));
    let near = |p: [u16; 4], c: [u16; 3]| {
        (0..3).all(|k| (f64::from(p[k]) - f64::from(c[k])).abs() <= depth.gate())
    };
    let (mut mixed, mut telling) = (0, 0);
    for (i, &p) in out.px.iter().enumerate() {
        assert_eq!(f64::from(p[3]), depth.max(), "{what}: pixel {i} lost alpha");
        assert!(
            feasible(depth, p, right),
            "{what}: pixel {i} {p:?} is not one mix"
        );
        if !near(p, a) && !near(p, b) {
            mixed += 1;
            telling += usize::from(!feasible(depth, p, wrong));
        }
    }
    assert!(mixed >= MIN_MIXED, "{what}: only {mixed} mixed pixels");
    assert!(
        telling >= MIN_TELLING,
        "{what}: only {telling} pixels tell the domains apart"
    );
}

#[test]
fn liquify_mixes_in_linear_light() {
    let codes = |v: f64| v;
    for depth in DEPTHS {
        let encoded = doc(depth, (64, 24), stripes);
        let what = format!("liquify {depth:?}");
        let out = encoded.liquify(&push());
        assert_domain(&out, depth, codes, decode, &format!("{what} encoded"));
        let out = encoded.linear().liquify(&push());
        assert_domain(&out, depth, decode, codes, &format!("{what} linear"));
    }
}

/// A pixel the field leaves in place is its stored sample, hidden colour
/// under alpha 0 and off-lattice codes included.
#[test]
fn liquify_leaves_untouched_pixels_bit_for_bit() {
    let (w, h) = (64u32, 48u32);
    let varied = |x: u32, y: u32| {
        let a = if (x * 3 + y).is_multiple_of(7) {
            0
        } else {
            255
        };
        [
            (x * 13 + 9) as u8,
            (y * 23 + 4) as u8,
            ((x ^ y) * 11) as u8,
            a,
        ]
    };
    let strokes = json!({"strokes": [{"tool": "forward_warp", "size": 16, "strength": 0.7,
                                      "density": 1, "points": [[16, 16], [24, 18]]}]});
    let typed = doc16((w, h), |x, y| off_lattice(varied(x, y), x, y));
    for source in [doc(Depth::D8, (w, h), varied), typed] {
        let before = source.layer();
        let out = source.linear().liquify(&strokes);
        let mut far = 0;
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let near = (fx - 20.0).hypot(fy - 17.0) < 8.0 + 8.0 + 4.0;
                let (xi, yi) = (i64::from(x), i64::from(y));
                if !near {
                    assert_eq!(out.at(xi, yi), before.at(xi, yi), "({x}, {y})");
                    far += 1;
                }
            }
        }
        assert!(far > 1500, "only {far} untouched pixels");
        assert!(out.px != before.px, "the stroke moved nothing");
    }
}

/// `dr_doc_filter_op` is `dr_image_filter_op` byte for byte for every op on
/// an encoded document and for every op but `liquify` on a linear-light one.
#[test]
fn only_liquify_reads_the_documents_evaluation() {
    let encoded = DrDocument::from_pixels(RgbaImage::from_fn(64, 24, |x, y| {
        image::Rgba(stripes(x, y))
    }));
    let switched = linear(&encoded);
    let img = byte_image(&Doc::Byte(encoded.clone()).layer());
    let blur = json!({"radius": 2});
    // Flags, not `assert_eq!`: the byte vectors are long, and the mix itself
    // is `liquify_mixes_in_linear_light`'s.
    let same = |doc: &DrDocument, op: &str, params: &Value| {
        let bytes = filter(Some(doc), img, op, params);
        assert!(bytes.is_some(), "{op} answered the bypass");
        bytes == filter(None, img, op, params)
    };
    for (op, params) in [("liquify", push()), ("box_blur", blur.clone())] {
        assert!(
            same(&encoded, op, &params),
            "{op} on an encoded document moved"
        );
    }
    assert!(
        same(&switched, "box_blur", &blur),
        "a linear-light document's box blur left the stored codes"
    );
    assert!(
        !same(&switched, "liquify", &push()),
        "a linear-light document's liquify gave the encoded bytes"
    );
    unsafe { dr_image_free(img) };
}
