//! Solid Color and Gradient fill layers: the plane-less `fill` and
//! `gradient` artwork components through `dr_artwork_render` /
//! `dr_artwork_render_float`, their publication with a `fill` description,
//! and the `.rz` round trip of pixels and description at 8, 16 and 32 bits.
//!
//! Oracles are closed forms, none read off the implementation: a solid fill
//! is its colour at every pixel; a gradient's parameter is written out for
//! the three boxes tested (left to right, bottom to top reversed, radial
//! from the centre; Linear and Radial sample the pixel's top-left corner)
//! and its opacity is piecewise linear between the two stops that
//! bracket it, its colour the Classic ramp for both, held to the
//! bracketing stops (`common::bracket_ramp`). The documents are sRGB, so an authored
//! colour is the document's own number at 8 and 16 bits and linear light as
//! given at 32 bits. A `fill`'s wide `linear` colour and a
//! `gradient`'s per-stop `linear` are held to the Display P3
//! oracle of `common/p3_oracle.rs` at all three depths.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{
    dr_artwork_render, dr_artwork_render_float, dr_doc_artwork, dr_precision_doc_artwork,
    dr_precision_doc_artwork_description,
};
use doctored_core::ffi_doc::{dr_doc_layer_meta, dr_doc_open, dr_doc_save_native};
use doctored_core::ffi_precision::{dr_precision_doc_open, dr_precision_doc_save_native};
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::common::{bracket_ramp, layer_pixels};
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 16;
const H: u32 = 8;

/// Three stops: position, RGB, opacity.
const STOPS: [(f64, [f64; 3], f64); 3] = [
    (0.0, [1.0, 0.0, 0.25], 1.0),
    (0.4, [0.0, 1.0, 0.5], 0.5),
    (1.0, [0.25, 0.5, 1.0], 0.25),
];

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// The `GradientFill` object a fill layer stores, as the style parser reads
/// it. The stops' `#rrggbb` are deliberately wrong: the component's `colors`
/// carry the numbers.
fn gradient(style: &str, angle: f64, reverse: bool) -> Value {
    let stops: Vec<Value> = STOPS
        .iter()
        .map(|(position, _, opacity)| json!({"position": position, "color": "#808080", "opacity": opacity}))
        .collect();
    json!({"stops": stops, "style": style, "angle": angle, "scale": 1.0,
        "reverse": reverse, "align_with_layer": false})
}

fn gradient_component(gradient: &Value, scale: f64) -> Value {
    let colors: Vec<[f64; 3]> = STOPS.iter().map(|s| s.1.map(|v| v * scale)).collect();
    json!([{"kind": "gradient", "gradient": gradient, "colors": colors}])
}

fn render(float: bool, components: &Value) -> Result<PrecisionImage, String> {
    let description = CString::new(components.to_string()).unwrap();
    let profile = IccProfile::srgb();
    let call = if float {
        dr_artwork_render_float
    } else {
        dr_artwork_render
    };
    let mut err = ptr::null_mut();
    let out = unsafe {
        call(
            W,
            H,
            description.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    match owned(err) {
        Some(message) => {
            assert!(out.is_null());
            Err(message)
        }
        None => {
            assert!(!out.is_null());
            Ok(*unsafe { Box::from_raw(out) })
        }
    }
}

/// The straight colour and opacity of `STOPS` at `t`, colours times `scale`:
/// the opacity piecewise linear, the colour too unless `classic`, when it is
/// the Classic ramp (a Linear, Radial, Angle or Reflected gradient)
/// held to its bracketing stops (a fill layer's bound).
fn ramp(t: f64, scale: f64, classic: bool) -> ([f64; 3], f64) {
    let (mut rgb, opacity) = linear_ramp(t, scale);
    if classic {
        for (c, v) in rgb.iter_mut().enumerate() {
            let channel: Vec<(f64, f64)> = STOPS.iter().map(|s| (s.0, s.1[c] * scale)).collect();
            *v = bracket_ramp(&channel, t, 1.0);
        }
    }
    (rgb, opacity)
}

/// The straight colour and opacity of `STOPS` at `t`, colours times `scale`,
/// both piecewise linear.
fn linear_ramp(t: f64, scale: f64) -> ([f64; 3], f64) {
    if t <= STOPS[0].0 {
        return (STOPS[0].1.map(|v| v * scale), STOPS[0].2);
    }
    for pair in STOPS.windows(2) {
        let ((p0, c0, o0), (p1, c1, o1)) = (pair[0], pair[1]);
        if t < p1 {
            let f = (t - p0) / (p1 - p0);
            return (
                std::array::from_fn(|i| (c0[i] + (c1[i] - c0[i]) * f) * scale),
                o0 + (o1 - o0) * f,
            );
        }
    }
    (STOPS[2].1.map(|v| v * scale), STOPS[2].2)
}

/// A gradient's parameter at pixel `(x, y)`.
type Parameter = fn(u32, u32) -> f64;

/// The three gradients tested, the parameter of each at pixel `(x, y)` and
/// whether its colour is the Classic ramp (Linear and Radial).
fn cases() -> Vec<(Value, Parameter, bool)> {
    vec![
        // Angle 0: left to right across the box, at the pixel's corner.
        (
            gradient("linear", 0.0, false),
            |x, _| f64::from(x) / f64::from(W),
            true,
        ),
        // Angle 90 runs bottom to top (1 - y / H at the corner); reversed,
        // top to bottom.
        (
            gradient("linear", 90.0, true),
            |_, y| f64::from(y) / f64::from(H),
            true,
        ),
        // Radial at angle 0: the corner's distance from the integer centre
        // (⌊W/2⌋, ⌊H/2⌋) over the whole-pixel radius ⌊W/2⌋.
        (
            gradient("radial", 0.0, false),
            |x, y| {
                let dx = f64::from(x) - f64::from(W / 2);
                let dy = f64::from(y) - f64::from(H / 2);
                (dx.hypot(dy) / f64::from(W / 2)).clamp(0., 1.)
            },
            true,
        ),
    ]
}

fn fill_description(body: Value) -> String {
    let mut description = json!({"type": "fill", "version": 1, "canvas": [W, H]});
    for (key, value) in body.as_object().unwrap() {
        description[key] = value.clone();
    }
    description.to_string()
}

fn typed_doc() -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            W,
            H,
            vec![[1000, 2000, 3000, 65535]; (W * H) as usize],
            IccProfile::srgb(),
            None,
        )
        .unwrap(),
        Default::default(),
        Default::default(),
    )
    .unwrap()
}

/// Publishes `image` as a new typed record with `description`.
fn publish(
    doc: &PrecisionDocument,
    image: &PrecisionImage,
    description: &str,
) -> PrecisionDocument {
    let name = CString::new("Fill").unwrap();
    let text = CString::new(description).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork(
            doc,
            0,
            false,
            image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    *unsafe { Box::from_raw(out) }
}

/// `doc` saved as `.rz` and opened again.
fn reopened(doc: &PrecisionDocument) -> PrecisionDocument {
    let dir = TempDir::new().unwrap();
    let path = CString::new(dir.path().join("fill.rz").to_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let saved = unsafe {
        dr_precision_doc_save_native(doc, path.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(saved, "{:?}", owned(err));
    let out = unsafe { dr_precision_doc_open(path.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!out.is_null(), "{:?}", owned(err));
    *unsafe { Box::from_raw(out) }
}

fn typed_description(doc: &PrecisionDocument, layer: usize) -> Option<String> {
    owned(unsafe { dr_precision_doc_artwork_description(doc, layer) })
}

#[test]
fn sixteen_bit_fill_layers_render_and_round_trip() {
    let doc = typed_doc();
    // Solid: the colour at every pixel, alpha included.
    let solid = [0.2, 0.4, 0.8, 0.5];
    let image = render(false, &json!([{"kind": "fill", "color": solid}])).expect("a solid fill");
    assert_eq!(image.storage(), PrecisionStorage::Rgba16Unorm);
    let want = solid.map(|v: f64| (v * 65535.).round() as u16);
    for pixel in image.to_rgba16().ok().unwrap() {
        for c in 0..4 {
            assert!(
                pixel[c].abs_diff(want[c]) <= 1,
                "{pixel:?} against {want:?}"
            );
        }
    }
    let description = fill_description(json!({"fill": "solid", "color": solid}));
    let published = publish(&doc, &image, &description);
    let back = reopened(&published);
    assert_eq!(
        typed_description(&back, 1).as_deref(),
        Some(description.as_str())
    );
    assert_eq!(
        back.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
        image.to_rgba16().ok().unwrap()
    );

    for (gradient, parameter, classic) in cases() {
        let image = render(false, &gradient_component(&gradient, 1.0)).expect("a gradient fill");
        let pixels = image.to_rgba16().ok().unwrap();
        for y in 0..H {
            for x in 0..W {
                // The Classic ramp may overshoot a stop; a stored sample is
                // clamped.
                let (rgb, opacity) = ramp(parameter(x, y), 1.0, classic);
                let rgb = rgb.map(|v| v.clamp(0., 1.));
                let got = pixels[(y * W + x) as usize];
                for c in 0..3 {
                    assert!(
                        (f64::from(got[c]) - rgb[c] * 65535.).abs() <= 2.0,
                        "{gradient} ({x},{y}) channel {c}: {} against {}",
                        got[c],
                        rgb[c] * 65535.
                    );
                }
                assert!(
                    (f64::from(got[3]) - opacity * 65535.).abs() <= 2.0,
                    "{gradient} ({x},{y}) alpha: {} against {}",
                    got[3],
                    opacity * 65535.
                );
            }
        }
        let description = fill_description(json!({"fill": "gradient", "gradient": gradient}));
        let published = publish(&doc, &image, &description);
        let back = reopened(&published);
        assert_eq!(
            typed_description(&back, 1).as_deref(),
            Some(description.as_str())
        );
        assert_eq!(
            back.layer_source(1).unwrap().to_rgba16().ok().unwrap(),
            pixels
        );
    }
}

#[test]
fn eight_bit_fill_layers_render_and_round_trip() {
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(W, H, Rgba([10, 20, 30, 255])));
    for (gradient, parameter, classic) in cases() {
        let image = render(false, &gradient_component(&gradient, 1.0)).expect("a gradient fill");
        let description = fill_description(json!({"fill": "gradient", "gradient": gradient}));
        let name = CString::new("Fill").unwrap();
        let text = CString::new(description.as_str()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_doc_artwork(
                &doc,
                0,
                false,
                &image,
                0,
                0,
                name.as_ptr(),
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert_eq!(owned(err), None);
        assert!(!out.is_null());
        let published = unsafe { Box::from_raw(out) };
        let pixels = layer_pixels(&*published, 1);
        for y in 0..H {
            for x in 0..W {
                // The Classic ramp may overshoot a stop; a stored sample is
                // clamped.
                let (rgb, opacity) = ramp(parameter(x, y), 1.0, classic);
                let rgb = rgb.map(|v| v.clamp(0., 1.));
                let at = ((y * W + x) * 4) as usize;
                for c in 0..3 {
                    assert!(
                        (f64::from(pixels[at + c]) - rgb[c] * 255.).abs() <= 1.0,
                        "{gradient} ({x},{y}) channel {c}"
                    );
                }
                assert!((f64::from(pixels[at + 3]) - opacity * 255.).abs() <= 1.0);
            }
        }
        // Save, reopen: the same bytes and the same description.
        let dir = TempDir::new().unwrap();
        let path = CString::new(dir.path().join("fill.rz").to_str().unwrap()).unwrap();
        assert!(unsafe {
            dr_doc_save_native(&*published, path.as_ptr(), std::ptr::null(), &mut err)
        });
        let back = unsafe { dr_doc_open(path.as_ptr(), std::ptr::null(), &mut err) };
        assert!(!back.is_null(), "{:?}", owned(err));
        let back = unsafe { Box::from_raw(back) };
        assert_eq!(layer_pixels(&*back, 1), pixels);
        assert_eq!(
            owned(unsafe { dr_doc_layer_meta(&*back, 1) }).as_deref(),
            Some(description.as_str())
        );
    }
}

#[test]
fn float_fill_layers_render_linear_light_and_round_trip() {
    let profile = IccProfile::srgb();
    let doc = float_doc(
        W,
        H,
        &profile,
        &[vec![[0.5, 0.25, 0.125, 1.0]; (W * H) as usize]],
    );
    // Solid, above white: the numbers are linear light, taken as they are.
    let solid = [4.0, 0.5, 0.25, 1.0];
    let image = render(true, &json!([{"kind": "fill", "color": solid}])).expect("a solid fill");
    assert_eq!(image.storage(), PrecisionStorage::Rgba32Float);
    for pixel in image.to_rgba32().expect("float samples") {
        for c in 0..4 {
            assert!((f64::from(pixel[c]) - solid[c]).abs() <= 1e-5, "{pixel:?}");
        }
    }
    // A gradient whose stop colours are doubled reaches above white too.
    for (gradient, parameter, classic) in cases() {
        let image = render(true, &gradient_component(&gradient, 2.0)).expect("a gradient fill");
        let pixels = image.to_rgba32().expect("float samples");
        for y in 0..H {
            for x in 0..W {
                let (rgb, opacity) = ramp(parameter(x, y), 2.0, classic);
                let got = pixels[(y * W + x) as usize];
                for c in 0..3 {
                    assert!(
                        (f64::from(got[c]) - rgb[c]).abs() <= 1e-4,
                        "{gradient} ({x},{y}) channel {c}: {} against {}",
                        got[c],
                        rgb[c]
                    );
                }
                assert!((f64::from(got[3]) - opacity).abs() <= 1e-4);
            }
        }
        let description = fill_description(json!({"fill": "gradient", "gradient": gradient}));
        let published = publish(&doc, &image, &description);
        let back = reopened(&published);
        assert!(back.is_float());
        assert_eq!(
            typed_description(&back, 1).as_deref(),
            Some(description.as_str())
        );
        assert_eq!(back.layer_source(1).unwrap().to_rgba32().unwrap(), pixels);
    }
}

#[test]
fn component_and_description_refusals() {
    let good = gradient("linear", 0.0, false);
    // One colour per stop.
    let short =
        json!([{"kind": "gradient", "gradient": good, "colors": [[0., 0., 0.], [1., 1., 1.]]}]);
    assert!(render(false, &short).is_err());
    // Stops listed out of order would be re-sorted away from their colours.
    let mut unordered = good.clone();
    unordered["stops"][0]["position"] = json!(0.9);
    assert!(render(false, &gradient_component(&unordered, 1.0)).is_err());
    // An unknown style is refused, as the style setter refuses it.
    let mut unknown = good.clone();
    unknown["style"] = json!("spiral");
    assert!(render(false, &gradient_component(&unknown, 1.0)).is_err());
    // Above white is a 32-bit colour only.
    assert!(render(false, &gradient_component(&good, 2.0)).is_err());
    assert!(render(true, &gradient_component(&good, 2.0)).is_ok());
    assert!(render(
        false,
        &json!([{"kind": "fill", "color": [1.5, 0., 0., 1.]}])
    )
    .is_err());
    assert!(render(
        false,
        &json!([{"kind": "fill", "color": [0.5, 0., 0., 1.5]}])
    )
    .is_err());
    assert!(render(
        true,
        &json!([{"kind": "fill", "color": [0.5, 0., 0., 1.5]}])
    )
    .is_err());
    // A fill component takes no sample plane: one supplied is a layout error.
    let description =
        CString::new(json!([{"kind": "fill", "color": [0., 0., 0., 1.]}]).to_string()).unwrap();
    let profile = IccProfile::srgb();
    let samples = vec![65535u16; (W * H) as usize];
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_artwork_render(
            W,
            H,
            description.as_ptr(),
            samples.as_ptr(),
            samples.len(),
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert!(owned(err).is_some());

    // The description slot admits `fill` beside text and shape, with a
    // positive version, and still refuses another type.
    let doc = typed_doc();
    let image = render(false, &json!([{"kind": "fill", "color": [0., 0., 0., 1.]}])).unwrap();
    for (description, admitted) in [
        (
            r#"{"type":"fill","version":1,"fill":"solid","color":[0,0,0,1],"canvas":[16,8]}"#,
            true,
        ),
        (
            r#"{"type":"fill","version":0,"fill":"solid","color":[0,0,0,1],"canvas":[16,8]}"#,
            false,
        ),
        (r#"{"type":"pattern","version":1}"#, false),
    ] {
        let name = CString::new("Fill").unwrap();
        let text = CString::new(description).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_artwork(
                &doc,
                0,
                false,
                &image,
                0,
                0,
                name.as_ptr(),
                text.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            )
        };
        assert_eq!(!out.is_null(), admitted, "{description}");
        assert_eq!(owned(err).is_none(), admitted, "{description}");
        if !out.is_null() {
            drop(unsafe { Box::from_raw(out) });
        }
    }
}

/// `dr_artwork_render` / `_float` of `components` in `profile`.
fn render_in(
    profile: &std::sync::Arc<IccProfile>,
    float: bool,
    components: &Value,
) -> Result<PrecisionImage, String> {
    let description = CString::new(components.to_string()).unwrap();
    let call = if float {
        dr_artwork_render_float
    } else {
        dr_artwork_render
    };
    let mut err = ptr::null_mut();
    let out = unsafe {
        call(
            W,
            H,
            description.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    match owned(err) {
        Some(message) => Err(message),
        None => Ok(*unsafe { Box::from_raw(out) }),
    }
}

#[test]
fn a_wide_fill_colour_lands_on_its_display_p3_numbers_at_every_depth() {
    use crate::common::p3_oracle::{linear_srgb_to_linear_p3, p3_to_linear_srgb};
    let p3 = IccProfile::display_p3();
    // A Display P3 colour outside sRGB, carried as linear sRGB components.
    let numbers = [0.2, 0.9, 0.1];
    let linear = p3_to_linear_srgb(numbers);
    assert!(linear[0] < 0.0, "outside sRGB: {linear:?}");
    // `color` is its clipped sRGB stand-in, which `linear` overrides.
    let component = json!([{"kind": "fill", "color": [0.0, 1.0, 0.0, 1.0], "linear": linear}]);

    // 16 bits: the document's numbers within half an 8-bit code (the
    // profile stores its colorants on s15Fixed16; the oracle uses the
    // published primaries).
    let image = render_in(&p3, false, &component).expect("a wide fill");
    let samples = image.to_rgba16().unwrap();
    for pixel in &samples {
        for c in 0..3 {
            let got = f64::from(pixel[c]) / 65535.0;
            assert!((got - numbers[c]).abs() <= 1.0 / 510.0, "16: {pixel:?}");
        }
        assert_eq!(pixel[3], 65535);
    }

    // 8 bits: the same render published into a Display P3 byte document.
    let mut doc = DrDocument::from_pixels(crate::common::solid(W, H, [255; 4]));
    doc.profile = p3.clone();
    let name = CString::new("Fill").unwrap();
    let text = CString::new(
        json!({"type": "fill", "version": 1, "fill": "solid", "color": [0.0, 1.0, 0.0, 1.0],
               "linear": linear, "canvas": [W, H]})
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_artwork(
            &doc,
            0,
            false,
            &image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    let published = unsafe { Box::from_raw(out) };
    for pixel in published.layers[1].pixels.to_contiguous().unwrap().pixels() {
        for (got, n) in pixel.0.iter().zip(numbers) {
            let want = (n * 255.0).round();
            assert!((f64::from(*got) - want).abs() <= 1.0, "8: {pixel:?}");
        }
    }

    // 32 bits: linear light in the document's primaries, no encoding.
    let image = render_in(&p3, true, &component).expect("a wide float fill");
    let want = linear_srgb_to_linear_p3(linear);
    for pixel in image.to_rgba32().unwrap() {
        for c in 0..3 {
            assert!(
                (f64::from(pixel[c]) - want[c]).abs() <= 1e-4,
                "32: {pixel:?} vs {want:?}"
            );
        }
    }

    // `linear` must be finite within ±4 at every depth.
    for float in [false, true] {
        let wide = json!([{"kind": "fill", "color": [0.0, 1.0, 0.0, 1.0],
                           "linear": [-4.5, 0.5, 0.5]}]);
        assert!(render_in(&p3, float, &wide).is_err(), "float {float}");
    }
}

#[test]
fn a_wide_gradient_stop_lands_on_its_display_p3_numbers_at_every_depth() {
    use crate::common::p3_oracle::{linear_srgb_to_linear_p3, p3_to_linear_srgb};
    let p3 = IccProfile::display_p3();
    // Two Display P3 green stops (outside sRGB) then a grey: every pixel left
    // of the middle is on the Classic ramp of the green, the green and
    // the grey, which bends toward the grey before the middle stop.
    let numbers = [0.2, 0.9, 0.1];
    let green = p3_to_linear_srgb(numbers);
    assert!(green[0] < 0.0, "outside sRGB: {green:?}");
    let grey = p3_to_linear_srgb([0.5; 3]);
    let g = json!({"stops": [
            {"position": 0.0, "color": "#00ff00", "opacity": 1.0},
            {"position": 0.5, "color": "#00ff00", "opacity": 1.0},
            {"position": 1.0, "color": "#808080", "opacity": 1.0}],
        "style": "linear", "angle": 0.0, "scale": 1.0, "reverse": false,
        "align_with_layer": false});
    let component = |linear: Value| {
        json!([{"kind": "gradient", "gradient": g,
                "colors": [[0.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.5, 0.5, 0.5]],
                "linear": linear}])
    };
    let wide = component(json!([green, green, grey]));
    let left = |i: usize| (i % W as usize) < 7;
    // Stop values per channel at 0, 0.5 and 1, ramped at pixel `i`'s corner.
    let ramped = |i: usize, values: [[f64; 3]; 3]| -> [f64; 3] {
        let t = (i % W as usize) as f64 / f64::from(W);
        std::array::from_fn(|c| {
            bracket_ramp(
                &[
                    (0.0, values[0][c]),
                    (0.5, values[1][c]),
                    (1.0, values[2][c]),
                ],
                t,
                1.0,
            )
        })
    };

    // 16 bits: the document's numbers within half an 8-bit code.
    let image = render_in(&p3, false, &wide).expect("a wide gradient");
    for (i, pixel) in image
        .to_rgba16()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(i, _)| left(*i))
    {
        let want = ramped(i, [numbers, numbers, [0.5; 3]]);
        for c in 0..3 {
            let got = f64::from(pixel[c]) / 65535.0;
            assert!(
                (got - want[c]).abs() <= 1.0 / 510.0,
                "16 at {i}: {pixel:?} vs {want:?}"
            );
        }
    }

    // 8 bits: the same render published into a Display P3 byte document.
    let mut doc = DrDocument::from_pixels(crate::common::solid(W, H, [255; 4]));
    doc.profile = p3.clone();
    let name = CString::new("Fill").unwrap();
    let text = CString::new(
        json!({"type": "fill", "version": 1, "fill": "gradient", "gradient": g,
               "stop_linears": [green, green, grey], "canvas": [W, H]})
        .to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_doc_artwork(
            &doc,
            0,
            false,
            &image,
            0,
            0,
            name.as_ptr(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    let published = unsafe { Box::from_raw(out) };
    let pixels = published.layers[1].pixels.to_contiguous().unwrap();
    for (i, pixel) in pixels.pixels().enumerate().filter(|(i, _)| left(*i)) {
        for (got, n) in pixel.0.iter().zip(ramped(i, [numbers, numbers, [0.5; 3]])) {
            let want = (n * 255.0).round();
            assert!((f64::from(*got) - want).abs() <= 1.0, "8 at {i}: {pixel:?}");
        }
    }

    // 32 bits: linear light in the document's primaries, no encoding.
    let image = render_in(&p3, true, &wide).expect("a wide float gradient");
    let (green_p3, grey_p3) = (
        linear_srgb_to_linear_p3(green),
        linear_srgb_to_linear_p3(grey),
    );
    for (i, pixel) in image
        .to_rgba32()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(i, _)| left(*i))
    {
        let want = ramped(i, [green_p3, green_p3, grey_p3]);
        for c in 0..3 {
            assert!(
                (f64::from(pixel[c]) - want[c]).abs() <= 1e-4,
                "32 at {i}: {pixel:?} vs {want:?}"
            );
        }
    }

    // `linear` must match the stops and be finite within ±4 at every depth.
    let message = "artwork gradient linear colours must match its stops and be finite within ±4";
    for float in [false, true] {
        for bad in [json!([green]), json!([green, green, [-4.5, 0.5, 0.5]])] {
            let got = render_in(&p3, float, &component(bad.clone())).map(|_| ());
            assert!(
                got.as_ref().is_err_and(|e| e.contains(message)),
                "float {float}, {bad}: {got:?}"
            );
        }
    }
}
