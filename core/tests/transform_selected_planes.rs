//! Free Transform of the selected area of a targeted LAYER MASK or
//! ALPHA CHANNEL, through both FFI seams (`dr_doc_selected_pixels_edit` and
//! `dr_precision_doc_edit`) at 8 and 16 bits, and in a 32-bit float document.
//!
//! The oracle is written independently in f64: the plane's selected coverage
//! floats as premultiplied `(v·c, c)`, resamples (nearest or bilinear,
//! transparent taps off the selection), unpremultiplies and clamps; the hole
//! is `lerp(v, bg, c)` (none for a duplicate); the float lands source-over an
//! opaque gray, `lerp(holed, v_f, a_f)`. Off-canvas landing is discarded.
//!
//! `move_selected` with a plane target, checked against the transform's
//! integer translation bit for bit and against an integer oracle.

use crate::common;

use std::ffi::CString;
use std::ptr;
use std::sync::Arc;

use common::selected_pixels::*;
use doctored_core::doc::DrDocument;
use doctored_core::doc_lock::{LOCK_ALL, LOCK_PIXELS};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_retained_source::{dr_doc_mixed_source_edit, dr_precision_doc_source_edit};
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_capability::capability;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use image::{GrayImage, RgbaImage};
use serde_json::{json, Value};

const EMPTY: &str = "The selection holds no pixels to transform.";
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

// --------------------------------------------------------------- requests --

#[derive(Clone, Copy, Debug)]
enum Target {
    Mask(usize),
    Channel(usize),
}

impl Target {
    fn json(self) -> (usize, Value) {
        match self {
            Target::Mask(layer) => (layer, json!("mask")),
            Target::Channel(i) => (0, json!({ "channel": i })),
        }
    }
}

/// A plane transform request; `background` is the u16 gray (None omits it).
fn request(target: Target, m: [f64; 6], filter: &str, background: Option<u16>) -> String {
    let (layer, target) = target.json();
    let mut value = json!({"op":"transform_selected","layer":layer,"matrix":m,
                           "filter":filter,"target":target});
    if let Some(bg) = background {
        value["background"] = json!(bg);
    }
    value.to_string()
}

fn with(text: &str, key: &str, v: Value) -> String {
    let mut value: Value = serde_json::from_str(text).unwrap();
    value[key] = v;
    value.to_string()
}

fn translate(dx: f64, dy: f64) -> [f64; 6] {
    [1.0, 0.0, 0.0, 1.0, dx, dy]
}

/// Rotate by `degrees` and scale by `s` about `(cx, cy)`.
fn rotate_scale(degrees: f64, s: f64, cx: f64, cy: f64) -> [f64; 6] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (a, b, c, d) = (s * cos, s * sin, -s * sin, s * cos);
    [a, b, c, d, cx - a * cx - c * cy, cy - b * cx - d * cy]
}

// ----------------------------------------------------------------- planes --

/// A gray plane at its canvas rect, depth-coded samples.
#[derive(Clone, Debug, PartialEq)]
struct Gray {
    offset: (i32, i32),
    size: (u32, u32),
    px: Vec<u16>,
}

impl Gray {
    fn of(depth: Depth, offset: (i32, i32), size: (u32, u32), f: impl Fn(u32, u32) -> u8) -> Self {
        let mut px = Vec::new();
        for y in 0..size.1 {
            for x in 0..size.0 {
                px.push(depth.code(f(x, y), (x * 13 + y * 7) as u16));
            }
        }
        Gray { offset, size, px }
    }
    fn at(&self, x: i64, y: i64) -> Option<u16> {
        let (lx, ly) = (x - i64::from(self.offset.0), y - i64::from(self.offset.1));
        if lx < 0 || ly < 0 || lx >= i64::from(self.size.0) || ly >= i64::from(self.size.1) {
            return None;
        }
        Some(self.px[ly as usize * self.size.0 as usize + lx as usize])
    }
    fn bytes(&self) -> Vec<u8> {
        self.px.iter().map(|v| *v as u8).collect()
    }
}

fn ramp(x: u32, y: u32) -> u8 {
    (x * 23 + y * 37 + 11) as u8
}

/// Mask properties with every field off its default, so a test can see each
/// one survive.
fn props(offset: (i32, i32), outside: u8) -> MaskProperties {
    let mut p = MaskProperties::at(offset);
    p.linked = false;
    p.density = 0.75;
    p.feather = 2.0;
    p.outside_coverage = outside;
    p
}

fn byte_mask(doc: &mut DrDocument, layer: usize, mask: &Gray, p: MaskProperties) {
    let image = GrayImage::from_raw(mask.size.0, mask.size.1, mask.bytes()).unwrap();
    doc.layers[layer].mask = Some(Arc::new(image));
    doc.layers[layer].mask_enabled = true;
    doc.layers[layer].mask_properties = p;
}

fn typed_mask(mask: &Gray, p: MaskProperties) -> PrecisionMask {
    PrecisionMask::new(
        PrecisionPlane::from_u16(mask.size.0, mask.size.1, mask.px.clone(), None).unwrap(),
        p,
        true,
    )
    .unwrap()
}

fn layer_plane(depth: Depth, canvas: (u32, u32)) -> Plane {
    Plane::of(depth, (0, 0), canvas, pattern)
}

/// One raster layer carrying `mask`.
fn mask_doc(depth: Depth, canvas: (u32, u32), mask: &Gray, p: MaskProperties) -> Doc {
    let plane = layer_plane(depth, canvas);
    match depth {
        Depth::D8 => {
            let mut d = byte_doc(canvas, &plane);
            byte_mask(&mut d, 0, mask, p);
            Doc::Byte(d)
        }
        Depth::D16 => Doc::Typed(typed_doc(
            canvas,
            vec![typed_layer(&plane).with_mask(Some(typed_mask(mask, p)))],
        )),
    }
}

fn add_channel(doc: Doc, channel: &Gray) -> Doc {
    match doc {
        Doc::Byte(d) => {
            let (w, h) = (d.width, d.height);
            Doc::Byte(
                d.add_channel("A", &channel.bytes(), w, h, [255, 0, 0], 0.5)
                    .unwrap(),
            )
        }
        Doc::Typed(d) => {
            let text = json!({"op":"set_channel","index":null,"name":"A"}).to_string();
            Doc::Typed(edit16(&d, &text, &channel.px, None).unwrap().unwrap())
        }
    }
}

/// One raster layer and one canvas-sized channel.
fn channel_doc(depth: Depth, canvas: (u32, u32), channel: &Gray) -> Doc {
    let plane = layer_plane(depth, canvas);
    add_channel(doc(depth, canvas, &plane), channel)
}

/// What a stored mask looks like after an edit.
struct Stored {
    gray: Gray,
    linked: bool,
    density: f32,
    feather: f32,
    outside: u8,
    enabled: bool,
}

trait Planes {
    fn mask(&self, layer: usize) -> Stored;
    fn channel(&self, i: usize, canvas: (u32, u32)) -> Gray;
}

impl Planes for Doc {
    fn mask(&self, layer: usize) -> Stored {
        match self {
            Doc::Byte(d) => {
                let entry = &d.layers[layer];
                let mask = entry.mask.as_deref().unwrap();
                let p = &entry.mask_properties;
                Stored {
                    gray: Gray {
                        offset: p.offset,
                        size: mask.dimensions(),
                        px: mask.as_raw().iter().map(|v| u16::from(*v)).collect(),
                    },
                    linked: p.linked,
                    density: p.density,
                    feather: p.feather,
                    outside: p.outside_coverage,
                    enabled: entry.mask_enabled,
                }
            }
            Doc::Typed(d) => {
                let mask = d.layer(layer).unwrap().mask().unwrap();
                let p = mask.properties();
                Stored {
                    gray: Gray {
                        offset: p.offset,
                        size: mask.plane().dimensions(),
                        px: mask.plane().samples().to_vec(),
                    },
                    linked: p.linked,
                    density: p.density,
                    feather: p.feather,
                    outside: p.outside_coverage,
                    enabled: mask.enabled(),
                }
            }
        }
    }
    fn channel(&self, i: usize, canvas: (u32, u32)) -> Gray {
        let px = match self {
            Doc::Byte(d) => d.channels[i]
                .data
                .as_raw()
                .iter()
                .map(|v| u16::from(*v))
                .collect(),
            Doc::Typed(d) => d.channels()[i].plane().samples().to_vec(),
        };
        Gray {
            offset: (0, 0),
            size: canvas,
            px,
        }
    }
}

fn run(doc: &Doc, text: &str, cov: &[u16]) -> Doc {
    doc.edit(text, cov)
        .expect("the plane transform runs")
        .expect("the plane transform changes something")
}

/// u16 background for a byte gray at this depth (the host sends gray·257).
fn bg(gray: u8) -> u16 {
    u16::from(gray) * 257
}

// ----------------------------------------------------------------- oracle --

#[derive(Clone, Copy, PartialEq)]
enum Kernel {
    Nearest,
    Bilinear,
}

struct Oracle<'a> {
    depth: Depth,
    canvas: (u32, u32),
    plane: &'a Gray,
    /// Unit coverage outside the plane's stored rect.
    outside: f64,
    coverage: &'a [u16],
    /// Unit background gray; `None` is the duplicate (no hole).
    background: Option<f64>,
    m: [f64; 6],
    kernel: Kernel,
}

impl Oracle<'_> {
    fn on(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < i64::from(self.canvas.0) && y < i64::from(self.canvas.1)
    }
    fn cov(&self, x: i64, y: i64) -> f64 {
        if !self.on(x, y) {
            return 0.0;
        }
        f64::from(self.coverage[y as usize * self.canvas.0 as usize + x as usize]) / 65535.0
    }
    fn v(&self, x: i64, y: i64) -> f64 {
        self.plane
            .at(x, y)
            .map_or(self.outside, |s| f64::from(s) / self.depth.max())
    }
    /// The premultiplied float `(v·c, c)` at a source pixel.
    fn float(&self, x: i64, y: i64) -> [f64; 2] {
        let c = self.cov(x, y);
        [self.v(x, y) * c, c]
    }
    fn landed(&self, x: i64, y: i64) -> Option<(f64, f64)> {
        let m = self.m;
        let det = m[0] * m[3] - m[1] * m[2];
        let (dx, dy) = (x as f64 + 0.5 - m[4], y as f64 + 0.5 - m[5]);
        let u = (m[3] * dx - m[2] * dy) / det;
        let v = (-m[1] * dx + m[0] * dy) / det;
        let acc = match self.kernel {
            Kernel::Nearest => self.float(u.floor() as i64, v.floor() as i64),
            Kernel::Bilinear => {
                let (kx, ky) = ((u - 0.5).floor(), (v - 0.5).floor());
                let (fx, fy) = (u - 0.5 - kx, v - 0.5 - ky);
                let mut acc = [0.0; 2];
                for (oy, wy) in [(0, 1.0 - fy), (1, fy)] {
                    for (ox, wx) in [(0, 1.0 - fx), (1, fx)] {
                        let s = self.float(kx as i64 + ox, ky as i64 + oy);
                        acc[0] += s[0] * wx * wy;
                        acc[1] += s[1] * wx * wy;
                    }
                }
                acc
            }
        };
        (acc[1] > 0.0).then(|| ((acc[0] / acc[1]).clamp(0.0, 1.0), acc[1].clamp(0.0, 1.0)))
    }
    /// The expected unit value at canvas pixel (x, y); `None` keeps it.
    fn expect(&self, x: i64, y: i64) -> Option<f64> {
        if !self.on(x, y) {
            return None;
        }
        let c = self.cov(x, y);
        let landing = self.landed(x, y);
        let holed = match self.background {
            Some(b) if c > 0.0 => {
                let v = self.v(x, y);
                v + (b - v) * c
            }
            _ if landing.is_none() => return None,
            _ => self.v(x, y),
        };
        Some(match landing {
            Some((vf, af)) => holed + (vf - holed) * af,
            None => holed,
        })
    }
    /// Every sample of `got` against the oracle; `slack` in codes. Pixels the
    /// oracle keeps must equal `before` (or the outside coverage code).
    fn check(&self, before: &Gray, outside_code: u16, got: &Gray, slack: f64, what: &str) {
        let max = self.depth.max();
        assert_eq!(
            got.px.len(),
            got.size.0 as usize * got.size.1 as usize,
            "{what}"
        );
        for (i, value) in got.px.iter().enumerate() {
            let x = i64::from(got.offset.0) + (i as i64 % i64::from(got.size.0));
            let y = i64::from(got.offset.1) + (i as i64 / i64::from(got.size.0));
            match self.expect(x, y) {
                None => assert_eq!(
                    *value,
                    before.at(x, y).unwrap_or(outside_code),
                    "{what} {:?}: untouched {x},{y}",
                    self.depth
                ),
                Some(want) => {
                    let want = want.clamp(0.0, 1.0) * max;
                    assert!(
                        (f64::from(*value) - want).abs() <= slack,
                        "{what} {:?}: {x},{y} got {value} want {want}",
                        self.depth
                    );
                }
            }
        }
    }
}

// ------------------------------------------------------------------ cases --

#[test]
fn a_channel_integer_translate_is_exact_and_leaves_the_background() {
    let canvas = (14, 10);
    let cov = coverage(canvas, rect(2, 2, 4, 3));
    let m = translate(5.0, 3.0);
    for depth in DEPTHS {
        let channel = Gray::of(depth, (0, 0), canvas, ramp);
        let base = channel_doc(depth, canvas, &channel);
        let layer_before = base.plane(0);
        let got = run(
            &base,
            &request(Target::Channel(0), m, "bilinear", Some(bg(200))),
            &cov,
        );
        let after = got.channel(0, canvas);
        let code = |g: u8| depth.code(g, 0);
        for y in 0..canvas.1 as i64 {
            for x in 0..canvas.0 as i64 {
                let source = (x - 5, y - 3);
                let want = if (2..6).contains(&source.0) && (2..5).contains(&source.1) {
                    channel.at(source.0, source.1).unwrap()
                } else if (2..6).contains(&x) && (2..5).contains(&y) {
                    if depth == Depth::D8 {
                        200
                    } else {
                        code(200)
                    }
                } else {
                    channel.at(x, y).unwrap()
                };
                assert_eq!(after.at(x, y).unwrap(), want, "{depth:?} {x},{y}");
            }
        }
        assert_eq!(
            got.plane(0),
            layer_before,
            "{depth:?}: layer pixels untouched"
        );
    }
}

#[test]
fn a_mask_grows_keeps_its_properties_and_moves_its_offset_only_on_growth() {
    let canvas = (16, 12);
    let cov = coverage(canvas, rect(4, 3, 3, 3));
    for depth in DEPTHS {
        for outside in [0u8, 255] {
            let mask = Gray::of(depth, (4, 3), (6, 5), ramp);
            let base = mask_doc(depth, canvas, &mask, props((4, 3), outside));
            let layer_before = base.plane(0);
            // Up-left by (3, 2): the landing (1, 1, 3, 3) leaves R.
            let m = translate(-3.0, -2.0);
            let got = run(
                &base,
                &request(Target::Mask(0), m, "nearest", Some(bg(90))),
                &cov,
            );
            let stored = got.mask(0);
            assert_eq!(stored.gray.offset, (1, 1), "{depth:?} {outside}");
            assert_eq!(stored.gray.size, (9, 7), "{depth:?} {outside}");
            assert!(!stored.linked && stored.enabled, "{depth:?}");
            assert_eq!(stored.density, 0.75);
            assert_eq!(stored.feather, 2.0);
            assert_eq!(stored.outside, outside);
            let outside_code = if outside == 255 {
                depth.max() as u16
            } else {
                0
            };
            let oracle = Oracle {
                depth,
                canvas,
                plane: &mask,
                outside: f64::from(outside) / 255.0,
                coverage: &cov,
                background: Some(90.0 / 255.0),
                m,
                kernel: Kernel::Nearest,
            };
            oracle.check(&mask, outside_code, &stored.gray, 0.5, "grown mask");
            assert_eq!(
                got.plane(0),
                layer_before,
                "{depth:?}: layer pixels untouched"
            );

            // A move that stays inside R keeps the offset and the storage.
            let cov_in = coverage(canvas, rect(5, 4, 2, 2));
            let inside = run(
                &base,
                &request(
                    Target::Mask(0),
                    translate(2.0, 1.0),
                    "nearest",
                    Some(bg(90)),
                ),
                &cov_in,
            );
            let kept = inside.mask(0);
            assert_eq!(kept.gray.offset, (4, 3));
            assert_eq!(kept.gray.size, (6, 5));
        }
    }
}

#[test]
fn a_feathered_hole_is_the_background_lerp() {
    let canvas = (18, 12);
    let cov = coverage(canvas, |x, y| {
        let (dx, dy) = (f64::from(x) + 0.5 - 6.0, f64::from(y) + 0.5 - 6.0);
        let r = (dx * dx + dy * dy).sqrt();
        ((1.0 - ((r - 2.0) / 3.0).clamp(0.0, 1.0)) * 65535.0).round() as u16
    });
    let m = translate(7.0, 0.0);
    for depth in DEPTHS {
        let channel = Gray::of(depth, (0, 0), canvas, ramp);
        let base = channel_doc(depth, canvas, &channel);
        let got = run(
            &base,
            &request(Target::Channel(0), m, "bicubic", Some(bg(30))),
            &cov,
        );
        let oracle = Oracle {
            depth,
            canvas,
            plane: &channel,
            outside: 0.0,
            coverage: &cov,
            background: Some(30.0 / 255.0),
            m,
            kernel: Kernel::Nearest,
        };
        oracle.check(&channel, 0, &got.channel(0, canvas), 0.55, "feathered");
    }
}

#[test]
fn a_bilinear_scale_and_rotate_matches_the_oracle_within_one_code() {
    let canvas = (24, 20);
    let cov = coverage(canvas, rect(8, 7, 7, 6));
    let m = rotate_scale(20.0, 1.5, 11.5, 10.0);
    for depth in DEPTHS {
        let mask = Gray::of(depth, (0, 0), canvas, ramp);
        let base = mask_doc(depth, canvas, &mask, props((0, 0), 0));
        let got = run(
            &base,
            &request(Target::Mask(0), m, "bilinear", Some(bg(64))),
            &cov,
        );
        let stored = got.mask(0);
        assert_eq!(stored.gray.offset, (0, 0));
        assert_eq!(stored.gray.size, canvas);
        let oracle = Oracle {
            depth,
            canvas,
            plane: &mask,
            outside: 0.0,
            coverage: &cov,
            background: Some(64.0 / 255.0),
            m,
            kernel: Kernel::Bilinear,
        };
        oracle.check(&mask, 0, &stored.gray, 1.0, "bilinear");
    }
}

#[test]
fn a_duplicate_leaves_no_hole() {
    let canvas = (14, 10);
    let cov = coverage(canvas, rect(2, 2, 4, 3));
    let m = translate(6.0, 4.0);
    for depth in DEPTHS {
        let channel = Gray::of(depth, (0, 0), canvas, ramp);
        let base = channel_doc(depth, canvas, &channel);
        let text = with(
            &request(Target::Channel(0), m, "bilinear", Some(bg(200))),
            "duplicate",
            json!(true),
        );
        let got = run(&base, &text, &cov);
        let oracle = Oracle {
            depth,
            canvas,
            plane: &channel,
            outside: 0.0,
            coverage: &cov,
            background: None,
            m,
            kernel: Kernel::Nearest,
        };
        oracle.check(&channel, 0, &got.channel(0, canvas), 0.5, "duplicate");
        let after = got.channel(0, canvas);
        assert_eq!(
            after.at(3, 3),
            channel.at(3, 3),
            "{depth:?}: the source stays"
        );
    }
}

#[test]
fn a_group_mask_is_clipped_to_its_storage_not_grown() {
    let canvas = (16, 12);
    let cov = coverage(canvas, rect(3, 3, 3, 3));
    let m = translate(6.0, 4.0);
    for depth in DEPTHS {
        let mask = Gray::of(depth, (2, 2), (6, 5), ramp);
        let plane = layer_plane(depth, canvas);
        let (base, group) = match depth {
            Depth::D8 => {
                let (mut d, group, _, _) =
                    byte_doc(canvas, &plane).group_layers(&[0], "G").unwrap();
                byte_mask(&mut d, group, &mask, props((2, 2), 0));
                (Doc::Byte(d), group)
            }
            Depth::D16 => {
                let group = PrecisionDocumentLayer::isolated_normal(
                    "G",
                    vec![typed_layer(&plane)],
                    1.0,
                    true,
                )
                .unwrap()
                .with_mask(Some(typed_mask(&mask, props((2, 2), 0))));
                (Doc::Typed(typed_doc(canvas, vec![group])), 1)
            }
        };
        let got = run(
            &base,
            &request(Target::Mask(group), m, "nearest", Some(bg(10))),
            &cov,
        );
        let stored = got.mask(group);
        assert_eq!(stored.gray.offset, (2, 2), "{depth:?}");
        assert_eq!(stored.gray.size, (6, 5), "{depth:?}");
        let oracle = Oracle {
            depth,
            canvas,
            plane: &mask,
            outside: 0.0,
            coverage: &cov,
            background: Some(10.0 / 255.0),
            m,
            kernel: Kernel::Nearest,
        };
        oracle.check(&mask, 0, &stored.gray, 0.5, "group mask");
    }
}

fn placed8(canvas: (u32, u32)) -> DrDocument {
    let base = DrDocument::from_pixels(RgbaImage::from_fn(canvas.0, canvas.1, |x, y| {
        image::Rgba(pattern(x, y))
    }));
    let source =
        DrDocument::from_pixels(RgbaImage::from_pixel(6, 6, image::Rgba([9, 99, 199, 255])));
    let text = CString::new(
        json!({"op":"place","source_id":"asset","name":"Placed","x":2,"y":2}).to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_mixed_source_edit(
            &base,
            &source,
            ptr::null(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(take_err(err).is_none());
    *unsafe { Box::from_raw(out) }
}

fn placed16(canvas: (u32, u32)) -> PrecisionDocument {
    let base = typed_doc(canvas, vec![typed_layer(&layer_plane(Depth::D16, canvas))]);
    let input = typed_doc(
        (6, 6),
        vec![typed_layer(&Plane::of(
            Depth::D16,
            (0, 0),
            (6, 6),
            |_, _| [9, 99, 199, 255],
        ))],
    );
    let text = CString::new(
        json!({"op":"place","source_id":"asset","name":"Placed","x":2,"y":2}).to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_source_edit(
            &base,
            &input,
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(take_err(err).is_none());
    *unsafe { Box::from_raw(out) }
}

#[test]
fn adjustment_and_retained_source_masks_are_admitted() {
    let canvas = (12, 10);
    let cov = coverage(canvas, rect(2, 2, 3, 3));
    let m = translate(4.0, 3.0);
    for depth in DEPTHS {
        let mask = Gray::of(depth, (0, 0), canvas, ramp);
        let plane = layer_plane(depth, canvas);
        let adjusted = match depth {
            Depth::D8 => {
                let mut d = byte_doc(canvas, &plane)
                    .adding_image_layer(0, RgbaImage::new(canvas.0, canvas.1), "Fx")
                    .unwrap();
                d.layers[1].meta = Some(adjustment_meta());
                byte_mask(&mut d, 1, &mask, props((0, 0), 255));
                Doc::Byte(d)
            }
            Depth::D16 => Doc::Typed(
                typed_doc(canvas, vec![typed_layer(&plane)])
                    .add_adjustment("Fx", &adjustment_meta(), 1.0, LIMIT, 0, None)
                    .unwrap()
                    .unwrap()
                    .set_layer_mask(
                        1,
                        Some(typed_mask(&mask, props((0, 0), 255))),
                        LIMIT,
                        0,
                        None,
                    )
                    .unwrap()
                    .unwrap(),
            ),
        };
        let got = run(
            &adjusted,
            &request(Target::Mask(1), m, "nearest", None),
            &cov,
        );
        assert_ne!(
            got.mask(1).gray,
            mask,
            "{depth:?}: the adjustment's mask moved"
        );

        let placed = match depth {
            Depth::D8 => {
                let mut d = placed8(canvas);
                let top = d.layers.len() - 1;
                byte_mask(&mut d, top, &mask, props((0, 0), 0));
                Doc::Byte(d)
            }
            Depth::D16 => {
                let d = placed16(canvas);
                let top = d.layer_count() - 1;
                Doc::Typed(
                    d.set_layer_mask(
                        top,
                        Some(typed_mask(&mask, props((0, 0), 0))),
                        LIMIT,
                        0,
                        None,
                    )
                    .unwrap()
                    .unwrap(),
                )
            }
        };
        let top = match &placed {
            Doc::Byte(d) => d.layers.len() - 1,
            Doc::Typed(d) => d.layer_count() - 1,
        };
        let before = placed.plane(top);
        let got = run(
            &placed,
            &request(Target::Mask(top), m, "nearest", None),
            &cov,
        );
        assert_ne!(
            got.mask(top).gray,
            mask,
            "{depth:?}: the placed layer's mask moved"
        );
        assert_eq!(got.plane(top), before, "{depth:?}: the placed pixels stay");
        match &got {
            Doc::Byte(d) => assert!(d.has_retained_source(top)),
            Doc::Typed(d) => assert!(d.has_retained_source(top)),
        }
    }
}

#[test]
fn lock_all_refuses_a_mask_lock_pixels_allows_and_a_channel_ignores_locks() {
    let canvas = (12, 10);
    let cov = coverage(canvas, rect(2, 2, 3, 3));
    let m = translate(4.0, 3.0);
    for depth in DEPTHS {
        let mask = Gray::of(depth, (0, 0), canvas, ramp);
        let locked = |locks: u32| -> Doc {
            match mask_doc(depth, canvas, &mask, props((0, 0), 0)) {
                Doc::Byte(mut d) => {
                    d.layers[0].locks = locks;
                    Doc::Byte(d)
                }
                Doc::Typed(d) => Doc::Typed(d.with_layer_locks(0, locks).unwrap().unwrap()),
            }
        };
        let text = request(Target::Mask(0), m, "nearest", None);
        let err = locked(LOCK_ALL).edit(&text, &cov).err().unwrap();
        assert!(err.contains("is locked (all)"), "{depth:?}: {err}");
        assert!(locked(LOCK_PIXELS).edit(&text, &cov).unwrap().is_some());
        let channel = Gray::of(depth, (0, 0), canvas, ramp);
        let with_channel = add_channel(locked(LOCK_ALL), &channel);
        let got = run(
            &with_channel,
            &request(Target::Channel(0), m, "nearest", None),
            &cov,
        );
        assert_ne!(got.channel(0, canvas), channel, "{depth:?}");
    }
}

#[test]
fn a_float_document_transforms_its_mask_its_channel_and_its_layer_pixels() {
    let float = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    assert!(float.is_float());
    let canvas = float.dimensions();
    let cov = coverage(canvas, rect(2, 2, 5, 4));
    let m = rotate_scale(15.0, 1.25, 4.5, 4.0);
    let mask = Gray::of(Depth::D16, (0, 0), canvas, ramp);
    let channel = Gray::of(Depth::D16, (0, 0), canvas, |x, y| ramp(y, x));
    let with_planes = |doc: PrecisionDocument| -> Doc {
        let doc = doc
            .set_layer_mask(0, Some(typed_mask(&mask, props((0, 0), 0))), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        add_channel(Doc::Typed(doc), &channel)
    };
    let float_doc = with_planes(float);
    let u16_doc = with_planes(typed_doc(
        canvas,
        vec![typed_layer(&layer_plane(Depth::D16, canvas))],
    ));
    for target in [Target::Mask(0), Target::Channel(0)] {
        let text = request(target, m, "bilinear", Some(bg(120)));
        let a = run(&float_doc, &text, &cov);
        let b = run(&u16_doc, &text, &cov);
        match target {
            Target::Mask(_) => assert_eq!(a.mask(0).gray, b.mask(0).gray),
            Target::Channel(_) => assert_eq!(a.channel(0, canvas), b.channel(0, canvas)),
        }
        let Doc::Typed(d) = &float_doc else {
            unreachable!()
        };
        assert!(float_doc.plan(&text).is_ok());
        assert!(d.is_float());
    }
    // The layer's own pixels are admitted too (`float_selected_pixels.rs`).
    let layer = json!({"op":"transform_selected","layer":0,"matrix":m,"filter":"bilinear"});
    assert!(float_doc.edit(&layer.to_string(), &cov).unwrap().is_some());
}

#[test]
fn an_identity_or_an_unchanged_plane_is_no_edit() {
    let canvas = (12, 10);
    let cov = coverage(canvas, rect(2, 2, 3, 3));
    for depth in DEPTHS {
        let channel = Gray::of(depth, (0, 0), canvas, ramp);
        let base = channel_doc(depth, canvas, &channel);
        let identity = request(Target::Channel(0), translate(0.0, 0.0), "bilinear", Some(1));
        assert!(base.edit(&identity, &cov).unwrap().is_none(), "{depth:?}");
        // An all-black channel moved with a black background stays black.
        let black = Gray::of(depth, (0, 0), canvas, |_, _| 0);
        let dark = channel_doc(depth, canvas, &black);
        let moved = request(Target::Channel(0), translate(3.0, 2.0), "bilinear", Some(0));
        assert!(dark.edit(&moved, &cov).unwrap().is_none(), "{depth:?}");
    }
}

// --------------------------------------------------------------- refusals --

#[test]
fn bad_targets_and_requests_refuse() {
    let canvas = (12, 10);
    let cov = coverage(canvas, rect(2, 2, 3, 3));
    let m = translate(4.0, 3.0);
    for depth in DEPTHS {
        let plane = layer_plane(depth, canvas);
        let bare = doc(depth, canvas, &plane);
        let layer_bg = json!({"op":"transform_selected","layer":0,"matrix":m,
                              "filter":"bilinear","background":5})
        .to_string();
        let sentence = "background applies to a mask or channel target only";
        assert_eq!(bare.edit(&layer_bg, &cov).err().as_deref(), Some(sentence));
        assert_eq!(bare.plan(&layer_bg).err().as_deref(), Some(sentence));
        let no_mask = request(Target::Mask(0), m, "nearest", None);
        assert_eq!(
            bare.edit(&no_mask, &cov).err().as_deref(),
            Some("layer 0 has no mask")
        );
        assert_eq!(
            bare.plan(&no_mask).err().as_deref(),
            Some("layer 0 has no mask")
        );
        let bad_channel = request(Target::Channel(3), m, "nearest", None);
        assert_eq!(
            bare.edit(&bad_channel, &cov).err().as_deref(),
            Some("channel 3 is out of range")
        );
        let out_of_range = request(Target::Mask(7), m, "nearest", None);
        assert!(bare
            .edit(&out_of_range, &cov)
            .err()
            .unwrap()
            .contains("out of range"));

        let channel = Gray::of(depth, (0, 0), canvas, ramp);
        let base = channel_doc(depth, canvas, &channel);
        let text = request(Target::Channel(0), m, "nearest", Some(9));
        let empty = vec![0u16; (canvas.0 * canvas.1) as usize];
        assert_eq!(base.edit(&text, &empty).err().as_deref(), Some(EMPTY));
        let short = &cov[1..];
        let err = base.edit(&text, short).err().unwrap();
        assert!(
            err.contains("canvas-sized") || err.contains("sample length"),
            "{depth:?}: {err}"
        );
        let promised = with(&text, "bounds", json!([5, 5, 2, 2]));
        assert_eq!(
            base.edit(&promised, &cov).err().as_deref(),
            Some("selected pixels coverage lies outside the request's bounds")
        );
        let degenerate = request(Target::Channel(0), [0.0; 6], "bilinear", None);
        assert_eq!(
            base.plan(&degenerate).err().as_deref(),
            Some("The transform matrix is degenerate or not finite.")
        );
        let plan = base.plan(&text).unwrap();
        let keys: Vec<&String> = plan.as_object().unwrap().keys().collect();
        assert!(keys.contains(&&"retained_bytes".to_string()), "{plan}");
        assert!(plan["scratch_bytes"].as_u64().unwrap() > 0, "{plan}");
        assert_eq!(
            plan["input_samples"].as_u64(),
            Some(u64::from(canvas.0 * canvas.1))
        );
    }
}

#[test]
fn the_byte_plan_answers_exactly_three_keys() {
    let canvas = (12, 10);
    let channel = Gray::of(Depth::D8, (0, 0), canvas, ramp);
    let Doc::Byte(d) = channel_doc(Depth::D8, canvas, &channel) else {
        unreachable!()
    };
    let text = request(
        Target::Channel(0),
        rotate_scale(10.0, 1.0, 5.0, 5.0),
        "bicubic",
        None,
    );
    let plan = plan8(&d, &text).unwrap();
    assert_eq!(plan.as_object().unwrap().len(), 3, "{plan}");
}

#[test]
fn a_landing_past_the_old_pixel_work_bound_is_planned_and_entered() {
    let canvas = (3800, 3800);
    let m = [500.0, 0.0, 0.0, 500.0, -499.0 * 1900.0, -499.0 * 1900.0];
    let cov = coverage(canvas, rect(1896, 1896, 8, 8));
    for depth in DEPTHS {
        let layer = Plane::of(depth, (1896, 1896), (8, 8), pattern);
        let mask = Gray::of(depth, (1896, 1896), (8, 8), ramp);
        let base = match depth {
            Depth::D8 => {
                let raw = layer.px.iter().flat_map(|p| p.map(|v| v as u8)).collect();
                let mut d = DrDocument::from_pixels(RgbaImage::from_raw(8, 8, raw).unwrap());
                d.width = canvas.0;
                d.height = canvas.1;
                d.layers[0].offset = layer.offset;
                byte_mask(&mut d, 0, &mask, props((1896, 1896), 0));
                Doc::Byte(d)
            }
            Depth::D16 => Doc::Typed(typed_doc(
                canvas,
                vec![typed_layer(&layer).with_mask(Some(typed_mask(&mask, props((1896, 1896), 0))))],
            )),
        };
        // 14.44M·36·4 > 2e9 once refused. The plan answers and the
        // run is entered: a cancelled control answers `operation cancelled`.
        let text = request(Target::Mask(0), m, "lanczos3", None);
        assert!(base.plan(&text).is_ok(), "{depth:?}");
        let control = DrJobControl::default();
        control.cancel();
        let err = match &base {
            Doc::Byte(d) => edit8(d, &text, &cov, Some(&control)).err(),
            Doc::Typed(d) => edit16(d, &text, &cov, Some(&control)).err(),
        };
        assert_eq!(err.as_deref(), Some("operation cancelled"), "{depth:?}");
    }
}

#[test]
fn a_companion_selection_past_the_old_work_bound_plans() {
    let canvas = (7500, 7500);
    let (sin, cos) = 30f64.to_radians().sin_cos();
    let m = [cos, sin, -sin, cos, 100.0, -50.0];
    for depth in DEPTHS {
        let layer = Plane::of(depth, (10, 10), (8, 8), pattern);
        let mask = Gray::of(depth, (10, 10), (8, 8), ramp);
        let base = match depth {
            Depth::D8 => {
                let raw = layer.px.iter().flat_map(|p| p.map(|v| v as u8)).collect();
                let mut d = DrDocument::from_pixels(RgbaImage::from_raw(8, 8, raw).unwrap());
                d.width = canvas.0;
                d.height = canvas.1;
                d.layers[0].offset = layer.offset;
                byte_mask(&mut d, 0, &mask, props((10, 10), 0));
                Doc::Byte(d)
            }
            Depth::D16 => Doc::Typed(typed_doc(
                canvas,
                vec![typed_layer(&layer).with_mask(Some(typed_mask(&mask, props((10, 10), 0))))],
            )),
        };
        // 56.25M·36 > 2e9 once refused the plan.
        let text = request(Target::Mask(0), m, "lanczos3", None);
        assert!(base.plan(&text).is_ok(), "{depth:?}");
    }
}

#[test]
fn the_capability_admits_planes_at_sixteen_and_thirty_two_bits() {
    for (target, depth, supported) in [
        ("raster", 16, true),
        ("mask", 16, true),
        ("channel", 16, true),
        ("red", 16, true),
        ("alpha", 32, true),
        ("group", 16, false),
        ("mask", 32, true),
        ("channel", 32, true),
        ("raster", 32, true),
    ] {
        assert_eq!(
            capability("selected_pixels_transform", target, depth)["supported"],
            supported,
            "{target}@{depth}"
        );
    }
}

#[test]
fn a_cancelled_control_returns_operation_cancelled() {
    let canvas = (12, 10);
    let cov = coverage(canvas, rect(2, 2, 3, 3));
    let text = request(Target::Channel(0), translate(4.0, 3.0), "bilinear", Some(9));
    for depth in DEPTHS {
        let channel = Gray::of(depth, (0, 0), canvas, ramp);
        let control = DrJobControl::default();
        control.cancel();
        let err = match channel_doc(depth, canvas, &channel) {
            Doc::Byte(d) => edit8(&d, &text, &cov, Some(&control)).err(),
            Doc::Typed(d) => edit16(&d, &text, &cov, Some(&control)).err(),
        };
        assert_eq!(err.as_deref(), Some("operation cancelled"), "{depth:?}");
    }
}

// ---------------------------------------------------------- move_selected --
//
// `move_selected` with the same `target`/`background` is the plane
// transform's integer translation through the same walk.

const MOVE_EMPTY: &str = "The selection holds no pixels to move.";

/// A plane move request; `background` is the u16 gray (None omits it).
fn move_request(target: Target, dx: i32, dy: i32, background: Option<u16>) -> String {
    let (layer, target) = target.json();
    let mut value = json!({"op":"move_selected","layer":layer,"dx":dx,"dy":dy,"target":target});
    if let Some(bg) = background {
        value["background"] = json!(bg);
    }
    value.to_string()
}

#[test]
fn move_equals_the_transform_translation_bit_for_bit() {
    let canvas = (16, 12);
    let hard = coverage(canvas, rect(3, 2, 5, 4));
    let feathered = coverage(canvas, |x, y| {
        let (dx, dy) = (f64::from(x) + 0.5 - 6.0, f64::from(y) + 0.5 - 5.0);
        let r = (dx * dx + dy * dy).sqrt();
        ((1.0 - ((r - 1.5) / 3.0).clamp(0.0, 1.0)) * 65535.0).round() as u16
    });
    let (dx, dy) = (4, -3);
    for depth in DEPTHS {
        let mask = Gray::of(depth, (2, 1), (9, 8), ramp);
        let channel = Gray::of(depth, (0, 0), canvas, |x, y| ramp(y, x));
        let base = add_channel(mask_doc(depth, canvas, &mask, props((2, 1), 255)), &channel);
        for target in [Target::Mask(0), Target::Channel(0)] {
            for duplicate in [false, true] {
                for (name, cov) in [("hard", &hard), ("feathered", &feathered)] {
                    let mut moved = move_request(target, dx, dy, Some(bg(170)));
                    let mut transformed = request(
                        target,
                        translate(f64::from(dx), f64::from(dy)),
                        "bilinear",
                        Some(bg(170)),
                    );
                    if duplicate {
                        moved = with(&moved, "duplicate", json!(true));
                        transformed = with(&transformed, "duplicate", json!(true));
                    }
                    let what = format!("{depth:?} {target:?} duplicate={duplicate} {name}");
                    let a = run(&base, &moved, cov);
                    let b = run(&base, &transformed, cov);
                    match target {
                        Target::Mask(_) => {
                            assert_eq!(a.mask(0).gray, b.mask(0).gray, "{what}");
                            assert_eq!(a.mask(0).outside, b.mask(0).outside, "{what}");
                        }
                        Target::Channel(_) => {
                            assert_eq!(a.channel(0, canvas), b.channel(0, canvas), "{what}")
                        }
                    }
                    assert_eq!(a.plane(0), b.plane(0), "{what}: layer pixels");
                    assert!(base.plan(&moved).is_ok(), "{what}: plan");
                }
            }
        }
    }
}

#[test]
fn move_a_hard_channel_rect_lands_at_the_offset_and_leaves_the_background() {
    let canvas = (14, 10);
    let cov = coverage(canvas, rect(2, 2, 4, 3));
    // The landing (12, 8, 4, 3) runs off the right and bottom edges.
    let (dx, dy) = (10i64, 6i64);
    for depth in DEPTHS {
        let channel = Gray::of(depth, (0, 0), canvas, ramp);
        let base = channel_doc(depth, canvas, &channel);
        let layer_before = base.plane(0);
        let got = run(
            &base,
            &move_request(Target::Channel(0), dx as i32, dy as i32, Some(bg(200))),
            &cov,
        );
        let after = got.channel(0, canvas);
        assert_eq!(
            after.size, canvas,
            "{depth:?}: a channel stays canvas-sized"
        );
        let selected = |x: i64, y: i64| (2..6).contains(&x) && (2..5).contains(&y);
        for y in 0..i64::from(canvas.1) {
            for x in 0..i64::from(canvas.0) {
                let want = if selected(x - dx, y - dy) {
                    channel.at(x - dx, y - dy).unwrap()
                } else if selected(x, y) {
                    depth.code(200, 0)
                } else {
                    channel.at(x, y).unwrap()
                };
                assert_eq!(after.at(x, y).unwrap(), want, "{depth:?} {x},{y}");
            }
        }
        assert_eq!(
            got.plane(0),
            layer_before,
            "{depth:?}: layer pixels untouched"
        );
    }
}

#[test]
fn move_a_mask_grows_and_moves_its_offset_only_on_growth() {
    let canvas = (16, 12);
    let cov = coverage(canvas, rect(4, 3, 3, 3));
    for depth in DEPTHS {
        let mask = Gray::of(depth, (4, 3), (6, 5), ramp);
        let base = mask_doc(depth, canvas, &mask, props((4, 3), 255));
        let grown = run(
            &base,
            &move_request(Target::Mask(0), -3, -2, Some(bg(90))),
            &cov,
        )
        .mask(0);
        assert_eq!(grown.gray.offset, (1, 1), "{depth:?}");
        assert_eq!(grown.gray.size, (9, 7), "{depth:?}");
        assert!(!grown.linked && grown.enabled, "{depth:?}");
        assert_eq!(
            (grown.density, grown.feather, grown.outside),
            (0.75, 2.0, 255)
        );
        // The moved selection's first pixel lands at (1, 1); the hole at
        // (4, 3) is the background.
        assert_eq!(grown.gray.at(1, 1), mask.at(4, 3), "{depth:?}");
        assert_eq!(grown.gray.at(4, 3), Some(depth.code(90, 0)), "{depth:?}");

        let kept = run(
            &base,
            &move_request(Target::Mask(0), 2, 1, Some(bg(90))),
            &coverage(canvas, rect(5, 4, 2, 2)),
        )
        .mask(0);
        assert_eq!(kept.gray.offset, (4, 3), "{depth:?}");
        assert_eq!(kept.gray.size, (6, 5), "{depth:?}");
    }
}

#[test]
fn move_on_a_float_document_equals_the_u16_document_and_moves_layer_pixels() {
    let float = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    assert!(float.is_float());
    let canvas = float.dimensions();
    let cov = coverage(canvas, rect(2, 2, 5, 4));
    let mask = Gray::of(Depth::D16, (0, 0), canvas, ramp);
    let channel = Gray::of(Depth::D16, (0, 0), canvas, |x, y| ramp(y, x));
    let with_planes = |doc: PrecisionDocument| -> Doc {
        let doc = doc
            .set_layer_mask(0, Some(typed_mask(&mask, props((0, 0), 0))), LIMIT, 0, None)
            .unwrap()
            .unwrap();
        add_channel(Doc::Typed(doc), &channel)
    };
    let float_doc = with_planes(float);
    let u16_doc = with_planes(typed_doc(
        canvas,
        vec![typed_layer(&layer_plane(Depth::D16, canvas))],
    ));
    for target in [Target::Mask(0), Target::Channel(0)] {
        let text = move_request(target, 3, 2, Some(bg(120)));
        let a = run(&float_doc, &text, &cov);
        let b = run(&u16_doc, &text, &cov);
        match target {
            Target::Mask(_) => assert_eq!(a.mask(0).gray, b.mask(0).gray),
            Target::Channel(_) => assert_eq!(a.channel(0, canvas), b.channel(0, canvas)),
        }
        assert!(float_doc.plan(&text).is_ok());
        let Doc::Typed(d) = &a else { unreachable!() };
        assert!(d.is_float());
    }
    // The layer's own pixels are admitted too (`float_selected_pixels.rs`).
    let layer = json!({"op":"move_selected","layer":0,"dx":3,"dy":2});
    assert!(float_doc.edit(&layer.to_string(), &cov).unwrap().is_some());
}

#[test]
fn move_refusals_name_the_plane() {
    let canvas = (12, 10);
    let cov = coverage(canvas, rect(2, 2, 3, 3));
    for depth in DEPTHS {
        let plane = layer_plane(depth, canvas);
        let bare = doc(depth, canvas, &plane);
        let layer_bg =
            json!({"op":"move_selected","layer":0,"dx":4,"dy":3,"background":5}).to_string();
        let sentence = "background applies to a mask or channel target only";
        assert_eq!(bare.edit(&layer_bg, &cov).err().as_deref(), Some(sentence));
        assert_eq!(bare.plan(&layer_bg).err().as_deref(), Some(sentence));
        let no_mask = move_request(Target::Mask(0), 4, 3, None);
        assert_eq!(
            bare.edit(&no_mask, &cov).err().as_deref(),
            Some("layer 0 has no mask")
        );
        assert_eq!(
            bare.plan(&no_mask).err().as_deref(),
            Some("layer 0 has no mask")
        );
        let bad_channel = move_request(Target::Channel(3), 4, 3, None);
        assert_eq!(
            bare.edit(&bad_channel, &cov).err().as_deref(),
            Some("channel 3 is out of range")
        );

        let mask = Gray::of(depth, (0, 0), canvas, ramp);
        let locked = |locks: u32| -> Doc {
            match mask_doc(depth, canvas, &mask, props((0, 0), 0)) {
                Doc::Byte(mut d) => {
                    d.layers[0].locks = locks;
                    Doc::Byte(d)
                }
                Doc::Typed(d) => Doc::Typed(d.with_layer_locks(0, locks).unwrap().unwrap()),
            }
        };
        let text = move_request(Target::Mask(0), 4, 3, None);
        let err = locked(LOCK_ALL).edit(&text, &cov).err().unwrap();
        assert!(err.contains("is locked (all)"), "{depth:?}: {err}");
        assert!(locked(LOCK_PIXELS).edit(&text, &cov).unwrap().is_some());

        let channel = Gray::of(depth, (0, 0), canvas, ramp);
        let base = channel_doc(depth, canvas, &channel);
        let empty = vec![0u16; (canvas.0 * canvas.1) as usize];
        let text = move_request(Target::Channel(0), 4, 3, Some(9));
        assert_eq!(base.edit(&text, &empty).err().as_deref(), Some(MOVE_EMPTY));
    }
}

#[test]
fn move_capability_admits_planes_at_sixteen_and_thirty_two_bits() {
    for (target, depth, supported) in [
        ("raster", 16, true),
        ("mask", 16, true),
        ("channel", 16, true),
        ("red", 16, true),
        ("alpha", 32, true),
        ("mask", 32, true),
        ("channel", 32, true),
        ("raster", 32, true),
    ] {
        assert_eq!(
            capability("selected_pixels_move", target, depth)["supported"],
            supported,
            "{target}@{depth}"
        );
    }
}
