//! Auto-Align Layers and Reduce Noise from Stack on a CMYK document,
//! through the FFI.
//!
//! The host registers a CMYK layer on its Display P3 VIEW (the ink through
//! the document's profile, `dr_precision_doc_cmyk_layer_thumbnail_rgba8`),
//! warps it with `perspective_set` or `project_layer`, and takes Reduce
//! Noise's statistic with `stack_mode`. These tests pin each step on a CMYK
//! document built from sRGB layers converted into the synthetic press
//! profile (`cmyk_structure_fixture::cmyk_layers`).
//!
//! # The oracles
//!
//! - Registration: two crops of the committed photo
//!   (`tests/fixtures/registration_photo/sky.jpg`, CC0) whose offset is
//!   known; the map must lie within `feature_register.rs`'s plain bound of
//!   that translation. The grey is the test's own luma of the view.
//! - Warps: the inks are read back from the document's own records
//!   (`cmyk_layer_samples`, `offset`), never from the core's arithmetic. An
//!   integer shift must land every ink on the reference's exactly (both inks
//!   came from the same sRGB pixel); a fractional one is the test's own
//!   bilinear of the stored ink within one code.
//! - Reduce Noise: the median of three is the middle stored ink.
//! - Cylindrical: the CMYKA16 mesh arm against the RGBA16 arm on twins that
//!   hold `65535 − ink` (`geometry_align_projection.rs` pins that arm to the
//!   analytic inverse).
#![allow(clippy::needless_range_loop)]

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::{dr_image_free, dr_string_free};
use doctored_core::ffi_align_projection::{
    dr_precision_doc_project_layer, dr_precision_doc_project_layer_plan,
};
use doctored_core::ffi_cmyk::dr_precision_doc_cmyk_layer_thumbnail_rgba8;
use doctored_core::ffi_feature_register::dr_feature_register;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{cmyk_layers, free, then, RgbLayer, LIMIT};
use crate::common::{img_dims, img_pixels, take_err_string};

/// The crops' size and the canvas's.
const W: u32 = 480;
const H: u32 = 320;
/// The reference crop's top-left in the photo.
const ORIGIN: (u32, u32) = (600, 400);
/// The worst distance from the truth a plain registration may have, frame
/// pixels: `feature_register.rs`'s `BOUND`.
const BOUND: f64 = 2.0;

// ---------------------------------------------------------------- fixtures --

fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^ (x >> 12)
}

/// The photo as straight 8-bit RGB.
fn photo() -> image::RgbImage {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/registration_photo/sky.jpg"
    );
    image::open(path)
        .expect("the fixture photo decodes")
        .to_rgb8()
}

/// The `W × H` crop at photo `(x, y)`, opaque u16 sRGB; `noise` seeds an
/// RGB noise of ±(hash % 9000) codes per channel added before conversion.
fn crop(photo: &image::RgbImage, x: u32, y: u32, noise: Option<u32>) -> Vec<[u16; 4]> {
    let mut out = Vec::with_capacity((W * H) as usize);
    for j in 0..H {
        for i in 0..W {
            let p = photo.get_pixel(x + i, y + j).0;
            let index = j * W + i;
            let px: [u16; 3] = std::array::from_fn(|c| {
                let base = i32::from(p[c]) * 257;
                let delta = noise.map_or(0, |seed| {
                    let n = hash(index.wrapping_mul(3).wrapping_add(c as u32) ^ seed);
                    let magnitude = (n % 9000) as i32;
                    if n & (1 << 20) != 0 {
                        magnitude
                    } else {
                        -magnitude
                    }
                });
                (base + delta).clamp(0, 65535) as u16
            });
            out.push([px[0], px[1], px[2], 65535]);
        }
    }
    out
}

fn layer(name: &'static str, pixels: Vec<[u16; 4]>) -> RgbLayer {
    RgbLayer {
        name,
        w: W,
        h: H,
        offset: (0, 0),
        pixels,
    }
}

fn doc<'a>(handle: *const PrecisionDocument) -> &'a PrecisionDocument {
    unsafe { &*handle }
}

/// Record `index`'s stored ink, offset and size.
struct Stored {
    samples: Vec<[u16; 5]>,
    offset: (i32, i32),
    dims: (u32, u32),
}

impl Stored {
    fn read(handle: *const PrecisionDocument, index: usize) -> Stored {
        let record = doc(handle).layer(index).unwrap();
        let dims = record.cmyk_source().expect("a CMYK raster").dimensions();
        Stored {
            samples: doc(handle).cmyk_layer_samples(index).unwrap(),
            offset: record.offset(),
            dims,
        }
    }

    /// The stored sample at canvas `(x, y)`, or `None` outside the record.
    fn at(&self, x: i32, y: i32) -> Option<[u16; 5]> {
        let (px, py) = (x - self.offset.0, y - self.offset.1);
        if px < 0 || py < 0 || px >= self.dims.0 as i32 || py >= self.dims.1 as i32 {
            return None;
        }
        Some(self.samples[(py as u32 * self.dims.0 + px as u32) as usize])
    }

    /// The canvas rectangle `(x0, y0, x1, y1)`, exclusive.
    fn rect(&self) -> (i32, i32, i32, i32) {
        (
            self.offset.0,
            self.offset.1,
            self.offset.0 + self.dims.0 as i32,
            self.offset.1 + self.dims.1 as i32,
        )
    }
}

/// `perspective_set` moving layer `index` (a `W × H` record at the origin)
/// by `(dx, dy)`, bilinear, the input freed.
fn shifted(
    handle: *mut PrecisionDocument,
    index: usize,
    dx: f64,
    dy: f64,
) -> *mut PrecisionDocument {
    let (w, h) = (f64::from(W), f64::from(H));
    let quad = [dx, dy, w + dx, dy, w + dx, h + dy, dx, h + dy];
    let request = json!({
        "op": "perspective_set", "layers": [index], "source_rect": [0, 0, W, H],
        "quad": quad, "filter": "bilinear",
    });
    then(handle, &request.to_string(), &[])
}

fn apply(m: &[f64; 9], x: f64, y: f64) -> (f64, f64) {
    let w = m[6] * x + m[7] * y + m[8];
    (
        (m[0] * x + m[1] * y + m[2]) / w,
        (m[3] * x + m[4] * y + m[5]) / w,
    )
}

/// Layer `index`'s view thumbnail at full size, as the test's own grey
/// (0.299 R + 0.587 G + 0.114 B, rounded).
fn view_grey(handle: *const PrecisionDocument, index: usize) -> Vec<u8> {
    let mut err = ptr::null_mut();
    let thumb = unsafe {
        dr_precision_doc_cmyk_layer_thumbnail_rgba8(
            handle,
            index,
            W,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!thumb.is_null(), "thumbnail: {}", take_err_string(err));
    assert_eq!(
        img_dims(thumb),
        (W, H),
        "the view is the layer at full size"
    );
    let rgba = img_pixels(thumb);
    unsafe { dr_image_free(thumb) };
    rgba.chunks_exact(4)
        .map(|p| {
            let y = 0.299 * f64::from(p[0]) + 0.587 * f64::from(p[1]) + 0.114 * f64::from(p[2]);
            y.round().clamp(0.0, 255.0) as u8
        })
        .collect()
}

// ------------------------------------------------------------------ tests --

#[test]
fn the_view_plane_registers_a_shifted_cmyk_pair() {
    let photo = photo();
    let handle = cmyk_layers(
        W,
        H,
        vec![
            layer("Reference", crop(&photo, ORIGIN.0, ORIGIN.1, None)),
            layer("Floating", crop(&photo, ORIGIN.0 + 12, ORIGIN.1 + 8, None)),
        ],
    );
    let reference = view_grey(handle, 0);
    let floating = view_grey(handle, 1);
    let mut map = [0.0f64; 9];
    let mut inliers = 0u32;
    let mut err = ptr::null_mut();
    let ok = unsafe {
        dr_feature_register(
            floating.as_ptr(),
            reference.as_ptr(),
            W,
            H,
            map.as_mut_ptr(),
            ptr::null_mut(),
            0,
            &mut inliers,
            ptr::null_mut(),
            &mut err,
        )
    };
    assert!(ok, "the view registers: {}", take_err_string(err));
    // Floating pixel (x, y) shows photo (612 + x, 408 + y), the reference's
    // pixel (x + 12, y + 8).
    let (w, h) = (f64::from(W), f64::from(H));
    for (x, y) in [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h), (w / 2.0, h / 2.0)] {
        let (gx, gy) = apply(&map, x, y);
        let distance = ((gx - (x + 12.0)).powi(2) + (gy - (y + 8.0)).powi(2)).sqrt();
        assert!(
            distance <= BOUND,
            "at ({x}, {y}) the map lands {distance} px from translation(12, 8): {map:?}"
        );
    }
    free(handle);
}

#[test]
fn an_integer_warp_lands_every_ink_on_the_reference() {
    let photo = photo();
    let handle = cmyk_layers(
        W,
        H,
        vec![
            layer("Reference", crop(&photo, ORIGIN.0, ORIGIN.1, None)),
            layer("Floating", crop(&photo, ORIGIN.0 + 12, ORIGIN.1 + 8, None)),
        ],
    );
    let handle = shifted(handle, 1, 12.0, 8.0);
    let reference = Stored::read(handle, 0);
    let moved = Stored::read(handle, 1);
    assert_eq!(moved.offset, (12, 8), "the warped layer sits at the shift");
    let (a, b) = (reference.rect(), moved.rect());
    let mut compared = 0usize;
    for y in a.1.max(b.1)..a.3.min(b.3) {
        for x in a.0.max(b.0)..a.2.min(b.2) {
            let (want, got) = (reference.at(x, y).unwrap(), moved.at(x, y).unwrap());
            assert_eq!(got, want, "five samples at canvas ({x}, {y})");
            compared += 1;
        }
    }
    assert!(compared >= 460 * 300, "compared {compared} overlap pixels");
    free(handle);
}

#[test]
fn a_fractional_warp_is_bilinear_per_ink() {
    let photo = photo();
    let handle = cmyk_layers(
        W,
        H,
        vec![
            layer("Reference", crop(&photo, ORIGIN.0, ORIGIN.1, None)),
            layer("Floating", crop(&photo, ORIGIN.0 + 12, ORIGIN.1 + 8, None)),
        ],
    );
    let source = Stored::read(handle, 1);
    let (dx, dy) = (12.5, 8.25);
    let handle = shifted(handle, 1, dx, dy);
    let moved = Stored::read(handle, 1);
    let (w, h) = (W as i64, H as i64);
    let mut compared = 0usize;
    for j in 0..moved.dims.1 as i32 {
        for i in 0..moved.dims.0 as i32 {
            // The destination centre, back into the source plane (the
            // source record sits at the origin), then bilinear on centres.
            let cx = f64::from(moved.offset.0 + i) + 0.5 - dx;
            let cy = f64::from(moved.offset.1 + j) + 0.5 - dy;
            let (px, py) = (cx - 0.5, cy - 0.5);
            let (x0, y0) = (px.floor() as i64, py.floor() as i64);
            if x0 < 0 || y0 < 0 || x0 + 1 >= w || y0 + 1 >= h {
                continue;
            }
            let (tx, ty) = (px - x0 as f64, py - y0 as f64);
            let at = |x: i64, y: i64| source.samples[(y * w + x) as usize];
            let got = moved.samples[(j as u32 * moved.dims.0 + i as u32) as usize];
            assert_eq!(got[4], 65535, "opaque inside at ({i}, {j})");
            for c in 0..4 {
                let v = |x: i64, y: i64| f64::from(at(x, y)[c]);
                let top = v(x0, y0) * (1.0 - tx) + v(x0 + 1, y0) * tx;
                let bottom = v(x0, y0 + 1) * (1.0 - tx) + v(x0 + 1, y0 + 1) * tx;
                let want = top * (1.0 - ty) + bottom * ty;
                assert!(
                    (f64::from(got[c]) - want).abs() <= 1.0,
                    "ink {c} at ({i}, {j}): {} against {want}",
                    got[c]
                );
            }
            compared += 1;
        }
    }
    assert!(compared >= 470 * 310, "compared {compared} pixels");
    free(handle);
}

#[test]
fn reduce_noise_median_is_the_middle_ink_per_plate() {
    let photo = photo();
    let handle = cmyk_layers(
        W,
        H,
        vec![
            layer("Reference", crop(&photo, ORIGIN.0, ORIGIN.1, None)),
            layer(
                "Noisy 1",
                crop(&photo, ORIGIN.0 + 12, ORIGIN.1 + 8, Some(0x1234_5678)),
            ),
            layer(
                "Noisy 2",
                crop(&photo, ORIGIN.0 - 6, ORIGIN.1 + 4, Some(0x0BAD_F00D)),
            ),
        ],
    );
    let handle = shifted(handle, 1, 12.0, 8.0);
    let handle = shifted(handle, 2, -6.0, 4.0);
    let request = json!({
        "op": "stack_mode", "layers": [0, 1, 2], "above": 2, "mode": "median",
        "name": "Noise Reduced",
    });
    let handle = then(handle, &request.to_string(), &[]);
    let members = [
        Stored::read(handle, 0),
        Stored::read(handle, 1),
        Stored::read(handle, 2),
    ];
    let result = Stored::read(handle, 3);
    let mut compared = 0usize;
    let mut result_error = [0.0f64; 4];
    let mut noisy_error = [0.0f64; 4];
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let (Some(a), Some(b), Some(c)) = (
                members[0].at(x, y),
                members[1].at(x, y),
                members[2].at(x, y),
            ) else {
                continue;
            };
            let got = result
                .at(x, y)
                .expect("the result covers every member pixel");
            for p in 0..4 {
                let mut three = [a[p], b[p], c[p]];
                three.sort_unstable();
                assert_eq!(got[p], three[1], "plate {p} at ({x}, {y})");
                result_error[p] += f64::from(got[p].abs_diff(a[p]));
                noisy_error[p] += f64::from(b[p].abs_diff(a[p]));
            }
            compared += 1;
        }
    }
    assert!(compared >= 460 * 300, "compared {compared} pixels");
    for p in 0..4 {
        assert!(
            result_error[p] < noisy_error[p],
            "plate {p}: the median's mean error {} is not below the noisy frame's {}",
            result_error[p] / compared as f64,
            noisy_error[p] / compared as f64
        );
    }
    free(handle);
}

// --------------------------------------------------------- cylindrical --

const P_CANVAS: (u32, u32) = (140, 100);
const P_OFFSET: (i32, i32) = (10, 6);
const P_SIZE: (u32, u32) = (120, 80);

/// The smooth picture, 0…1, at a layer-local continuous point.
fn picture(u: f64, v: f64, channel: usize) -> f64 {
    let c = channel as f64;
    0.5 + 0.35 * (u / 7.0 + c).sin() * (v / 9.0 - c / 2.0).cos()
}

/// An RGBA16 twin of the CMYK record's geometry holding `pixels`.
fn twin(pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    let image =
        PrecisionImage::from_rgba16(P_SIZE.0, P_SIZE.1, pixels, IccProfile::srgb(), None).unwrap();
    let layer = PrecisionDocumentLayer::raster("L", image, P_OFFSET, 1.0, true).unwrap();
    PrecisionDocument::from_parts(
        P_CANVAS.0,
        P_CANVAS.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![layer],
    )
    .unwrap()
}

fn json_string(text: *mut std::ffi::c_char) -> Value {
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

/// The projection's plan rect and the projected document.
fn project(handle: *const PrecisionDocument, request: &Value) -> (Value, *mut PrecisionDocument) {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let plan =
        unsafe { dr_precision_doc_project_layer_plan(handle, text.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!plan.is_null(), "plan: {}", take_err_string(err));
    let plan = json_string(plan);
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_project_layer(handle, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!out.is_null(), "project: {}", take_err_string(err));
    (plan["rect"].clone(), out)
}

/// A twin's layer 0: its RGBA16 samples, offset and size.
type Twin16 = (Vec<[u16; 4]>, (i32, i32), (u32, u32));

/// Layer 0's RGBA16 samples, offset and size.
fn rgba16(handle: *const PrecisionDocument) -> Twin16 {
    let layer = doc(handle).layer(0).unwrap();
    let image = layer.source().unwrap();
    (
        image.to_rgba16().unwrap(),
        layer.offset(),
        image.dimensions(),
    )
}

#[test]
fn cylindrical_projects_each_ink_as_the_rgba16_arm_projects_its_codes() {
    let mut pixels = Vec::new();
    for y in 0..P_SIZE.1 {
        for x in 0..P_SIZE.0 {
            let (u, v) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let c = |k| (picture(u, v, k) * 65535.0).round() as u16;
            pixels.push([c(0), c(1), c(2), 65535]);
        }
    }
    let cmyk = cmyk_layers(
        P_CANVAS.0,
        P_CANVAS.1,
        vec![RgbLayer {
            name: "L",
            w: P_SIZE.0,
            h: P_SIZE.1,
            offset: P_OFFSET,
            pixels,
        }],
    );
    let stored = Stored::read(cmyk, 0);
    assert_eq!((stored.offset, stored.dims), (P_OFFSET, P_SIZE));
    let cmy = twin(
        stored
            .samples
            .iter()
            .map(|s| [65535 - s[0], 65535 - s[1], 65535 - s[2], s[4]])
            .collect(),
    );
    let k = twin(
        stored
            .samples
            .iter()
            .map(|s| [65535 - s[3], 65535 - s[3], 65535 - s[3], s[4]])
            .collect(),
    );
    let (a, b) = (5.0f64.to_radians().cos(), 5.0f64.to_radians().sin());
    let centre = [
        f64::from(P_OFFSET.0) + f64::from(P_SIZE.0) / 2.0,
        f64::from(P_OFFSET.1) + f64::from(P_SIZE.1) / 2.0,
    ];
    let request = json!({
        "op": "project_layer", "layer": 0, "projection": "cylindrical", "focal": 150.0,
        "rotation": [a, 0.0, b, 0.0, 1.0, 0.0, -b, 0.0, a],
        "centre": centre, "origin": [centre[0] + 7.0, centre[1] - 3.0],
    });
    let (rect, projected) = project(cmyk, &request);
    let (cmy_rect, cmy_projected) = project(&cmy, &request);
    let (k_rect, k_projected) = project(&k, &request);
    assert_eq!(rect, cmy_rect, "the CMYK plan's rect is the RGBA16 arm's");
    assert_eq!(rect, k_rect, "the CMYK plan's rect is the RGBA16 arm's");
    let ink = Stored::read(projected, 0);
    let (cmy_px, cmy_offset, cmy_dims) = rgba16(cmy_projected);
    let (k_px, k_offset, k_dims) = rgba16(k_projected);
    assert_eq!((ink.offset, ink.dims), (cmy_offset, cmy_dims));
    assert_eq!((ink.offset, ink.dims), (k_offset, k_dims));
    assert_eq!(
        rect,
        json!([ink.offset.0, ink.offset.1, ink.dims.0, ink.dims.1]),
        "the plan's rect is the edit's"
    );
    let mut compared = 0usize;
    for i in 0..ink.samples.len() {
        let (s, t, u) = (ink.samples[i], cmy_px[i], k_px[i]);
        if s[4] != 65535 || t[3] != 65535 || u[3] != 65535 {
            continue;
        }
        for c in 0..3 {
            assert!(
                s[c].abs_diff(65535 - t[c]) <= 1,
                "ink {c} at {i}: {} against 65535 − {}",
                s[c],
                t[c]
            );
        }
        assert!(
            s[3].abs_diff(65535 - u[0]) <= 1,
            "K at {i}: {} against 65535 − {}",
            s[3],
            u[0]
        );
        compared += 1;
    }
    assert!(compared > 2000, "compared {compared} opaque pixels");
    free(projected);
    free(cmyk);
    free(cmy_projected);
    free(k_projected);
}
