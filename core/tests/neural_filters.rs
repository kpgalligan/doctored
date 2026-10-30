//! Filter ▸ Neural Filters (M4) through the FFI: the tiled stage
//! (`dr_image_neural_stage`, `dr_precision_image_neural_stage`), its info,
//! Super Zoom's new document and JPEG Artifacts Removal's typed apply, at 8
//! and 16 bits, with STUB models in place of Real-ESRGAN and FBCNN.
//!
//! # The oracles
//!
//! Every figure is written here from the header's contract, in binary64:
//! the classical sample is Catmull-Rom at `s = (o + 0.5)/scale` with clamped
//! taps, alpha-weighted (premultiply, sum, divide by the summed alpha); the
//! model's RGB is the mean of the `k × k` cells an output pixel covers,
//! `k = model_scale/scale`, clamped and rounded; Noise Reduction and Sharpen
//! are the shipped Reduce Noise and Unsharp Mask, so the seam test runs them
//! in both arms rather than restating them.
//!
//! # Bounds, declared before the first run
//!
//! * A tiled stage against the same stage in one tile, NR and Sharpen on:
//!   bit-exact (the stubs are local, every core is written from its own
//!   tile's reads).
//! * Against the binary64 oracles: ±1 code at 8 and 16 bits (one f32
//!   accumulation, one rounding).
//! * A scale-1 pass's alpha: bit-exact.
#![allow(clippy::needless_range_loop)]

use std::ffi::{c_void, CStr, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::{
    dr_image_free, dr_image_from_rgba, dr_image_height, dr_image_pixels_rgba, dr_image_width,
};
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_jobs::{dr_job_control_cancel, dr_job_control_create, dr_job_control_free};
use doctored_core::ffi_neural::*;
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_image_free};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::PrecisionImage;
use doctored_core::DrImage;
use serde_json::{json, Value};

use crate::cmyk_fixture::press_profile;
use crate::common::{img_pixels, layer_name, layer_pixels, take_err_string};
use crate::float_fixture::float_doc;

const LIMIT: u64 = 2 << 30;

// ---------------------------------------------------------------- stubs --

/// What a stub model answers per cell.
#[derive(Clone, Copy)]
enum Model {
    /// `1 − 0.75·v` of the input pixel under the cell: local and pointwise.
    Point,
    /// `0.5·v + 0.1·(i + j)/6` with `(i, j)` the cell's place in its 4×4
    /// block, so a ×2 output's 2×2 mean differs from any one cell.
    Sub,
    /// NaN everywhere.
    NotFinite,
}

struct Stub {
    model: Model,
    model_scale: usize,
    calls: Vec<u64>,
    inputs: Vec<Vec<f32>>,
    fail_at: Option<u64>,
    lengths: Vec<(usize, usize)>,
}

impl Stub {
    fn new(model: Model, model_scale: usize) -> Self {
        Self {
            model,
            model_scale,
            calls: Vec::new(),
            inputs: Vec::new(),
            fail_at: None,
            lengths: Vec::new(),
        }
    }
}

unsafe extern "C" fn predict(
    context: *mut c_void,
    tile: u64,
    input: *const f32,
    input_len: usize,
    output: *mut f32,
    output_len: usize,
) -> bool {
    let stub = unsafe { &mut *(context as *mut Stub) };
    stub.calls.push(tile);
    stub.lengths.push((input_len, output_len));
    let input = unsafe { std::slice::from_raw_parts(input, input_len) };
    let output = unsafe { std::slice::from_raw_parts_mut(output, output_len) };
    if stub.inputs.len() < 4 {
        stub.inputs.push(input.to_vec());
    }
    if stub.fail_at == Some(tile) {
        return false;
    }
    let side = ((input_len / 3) as f64).sqrt() as usize;
    let ms = stub.model_scale;
    let big = side * ms;
    assert_eq!(output_len, 3 * big * big);
    for c in 0..3 {
        for y in 0..big {
            for x in 0..big {
                let v = input[c * side * side + (y / ms) * side + x / ms];
                output[c * big * big + y * big + x] = match stub.model {
                    Model::Point => 1.0 - 0.75 * v,
                    Model::Sub => 0.5 * v + 0.1 * (((x % ms) + (y % ms)) as f32) / 6.0,
                    Model::NotFinite => f32::NAN,
                };
            }
        }
    }
    true
}

// -------------------------------------------------------------- fixtures --

/// A hashed RGBA texture with transparent, partial and opaque alpha.
fn texture(w: u32, h: u32) -> Vec<[f64; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let hash = |k: u32| {
                let mut v = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663) ^ k)
                    .wrapping_mul(2_654_435_761);
                v ^= v >> 13;
                f64::from(v % 1000) / 999.0
            };
            let alpha = if (x + y) % 11 == 0 {
                0.0
            } else if x % 5 == 0 {
                0.4 + 0.5 * hash(9)
            } else {
                1.0
            };
            [hash(1), hash(2), hash(3), alpha]
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Depth {
    Byte,
    Sixteen,
}

impl Depth {
    fn max(self) -> f64 {
        match self {
            Depth::Byte => 255.0,
            Depth::Sixteen => 65535.0,
        }
    }
    fn bps(self) -> u32 {
        match self {
            Depth::Byte => 1,
            Depth::Sixteen => 2,
        }
    }
}

/// Codes of `pixels` at `depth`.
fn codes(depth: Depth, pixels: &[[f64; 4]]) -> Vec<[u32; 4]> {
    pixels
        .iter()
        .map(|p| p.map(|v| (v * depth.max()).round() as u32))
        .collect()
}

enum Source {
    Byte(*mut DrImage),
    Typed(*mut PrecisionImage),
}

impl Source {
    fn new(depth: Depth, w: u32, h: u32, codes: &[[u32; 4]]) -> Self {
        match depth {
            Depth::Byte => {
                let bytes: Vec<u8> = codes.iter().flat_map(|p| p.map(|v| v as u8)).collect();
                Source::Byte(unsafe { dr_image_from_rgba(bytes.as_ptr(), w, h) })
            }
            Depth::Sixteen => {
                let pixels: Vec<[u16; 4]> = codes.iter().map(|p| p.map(|v| v as u16)).collect();
                let image =
                    PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap();
                Source::Typed(Box::into_raw(Box::new(image)))
            }
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        unsafe {
            match self {
                Source::Byte(image) => dr_image_free(*image),
                Source::Typed(image) => dr_precision_image_free(*image),
            }
        }
    }
}

fn stage_json(v: &Value) -> CString {
    CString::new(v.to_string()).unwrap()
}

/// A stage's output: its size and codes.
type Output = (u32, u32, Vec<[u32; 4]>);

/// One stage call: `Ok(None)` is NULL with no error.
fn run(
    source: &Source,
    stage: &Value,
    stub: Option<&mut Stub>,
    limit: u64,
    control: *const doctored_core::job_control::DrJobControl,
) -> Result<Option<Output>, String> {
    let json = stage_json(stage);
    let (callback, context): (DrNeuralPredict, *mut c_void) = match stub {
        Some(stub) => (Some(predict), stub as *mut Stub as *mut c_void),
        None => (None, ptr::null_mut()),
    };
    let mut err = ptr::null_mut();
    unsafe {
        match source {
            Source::Byte(image) => {
                let out = dr_image_neural_stage(
                    *image,
                    json.as_ptr(),
                    callback,
                    context,
                    limit,
                    0,
                    control,
                    &mut err,
                );
                if out.is_null() {
                    return if err.is_null() {
                        Ok(None)
                    } else {
                        Err(take_err_string(err))
                    };
                }
                let (w, h) = (dr_image_width(out), dr_image_height(out));
                let bytes =
                    std::slice::from_raw_parts(dr_image_pixels_rgba(out), (w * h * 4) as usize);
                let pixels = bytes
                    .chunks(4)
                    .map(|p| [p[0], p[1], p[2], p[3]].map(u32::from))
                    .collect();
                dr_image_free(out);
                Ok(Some((w, h, pixels)))
            }
            Source::Typed(image) => {
                let out = dr_precision_image_neural_stage(
                    *image,
                    json.as_ptr(),
                    callback,
                    context,
                    limit,
                    0,
                    control,
                    &mut err,
                );
                if out.is_null() {
                    return if err.is_null() {
                        Ok(None)
                    } else {
                        Err(take_err_string(err))
                    };
                }
                let (w, h) = (*out).dimensions();
                let pixels = (*out)
                    .to_rgba16()
                    .unwrap()
                    .iter()
                    .map(|p| p.map(u32::from))
                    .collect();
                dr_precision_image_free(out);
                Ok(Some((w, h, pixels)))
            }
        }
    }
}

fn stage(tile: u32, pad: u32, scale: u32, model_scale: u32, model: bool) -> Value {
    json!({"tile": tile, "pad": pad, "scale": scale, "model_scale": model_scale, "model": model})
}

fn with_sliders(mut stage: Value, noise_reduction: f64, sharpen: f64) -> Value {
    stage["noise_reduction"] = json!(noise_reduction);
    stage["sharpen"] = json!(sharpen);
    stage
}

// --------------------------------------------------------------- oracles --

fn catmull_rom(x: f64) -> f64 {
    let x = x.abs();
    if x < 1.0 {
        (1.5 * x - 2.5) * x * x + 1.0
    } else if x < 2.0 {
        ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0
    } else {
        0.0
    }
}

/// The classical sample of output `(ox, oy)` in code units (module doc).
fn classical(
    codes: &[[u32; 4]],
    w: u32,
    h: u32,
    scale: u32,
    max: f64,
    ox: u32,
    oy: u32,
) -> [f64; 4] {
    let taps = |o: u32| {
        let p = (f64::from(o) + 0.5) / f64::from(scale) - 0.5;
        let base = p.floor() as i64;
        (base - 1..=base + 2)
            .map(|t| (t, catmull_rom(p - t as f64)))
            .collect::<Vec<_>>()
    };
    let mut acc = [0.0_f64; 4];
    for (ty, wy) in taps(oy) {
        for (tx, wx) in taps(ox) {
            let x = tx.clamp(0, i64::from(w) - 1) as usize;
            let y = ty.clamp(0, i64::from(h) - 1) as usize;
            let p = codes[y * w as usize + x];
            let weight = wx * wy;
            let a = f64::from(p[3]);
            for c in 0..3 {
                acc[c] += weight * a / max * f64::from(p[c]);
            }
            acc[3] += weight * a;
        }
    }
    let alpha = acc[3].clamp(0.0, max).round();
    if alpha == 0.0 {
        return [0.0; 4];
    }
    [
        (acc[0] * max / acc[3]).clamp(0.0, max).round(),
        (acc[1] * max / acc[3]).clamp(0.0, max).round(),
        (acc[2] * max / acc[3]).clamp(0.0, max).round(),
        alpha,
    ]
}

fn within(got: &[[u32; 4]], want: &[[f64; 4]], bound: f64, what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: length");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..4 {
            let d = (f64::from(g[c]) - w[c]).abs();
            assert!(
                d <= bound,
                "{what}: pixel {i} channel {c}: got {} want {}",
                g[c],
                w[c]
            );
        }
    }
}

// ----------------------------------------------------------------- tests --

#[test]
fn a_tiled_stage_equals_the_same_stage_in_one_tile_at_both_depths() {
    let (w, h) = (40, 28);
    let pixels = texture(w, h);
    for depth in [Depth::Byte, Depth::Sixteen] {
        let source = Source::new(depth, w, h, &codes(depth, &pixels));
        for (scale, model_scale, nr, sharpen) in [
            (2, 4, 50.0, 50.0),
            (4, 4, 0.0, 30.0),
            (1, 1, 40.0, 0.0),
            (2, 4, 0.0, 0.0),
        ] {
            let tiled = with_sliders(stage(32, 8, scale, model_scale, true), nr, sharpen);
            let whole = with_sliders(stage(64, 8, scale, model_scale, true), nr, sharpen);
            let mut a = Stub::new(Model::Point, model_scale as usize);
            let mut b = Stub::new(Model::Point, model_scale as usize);
            let got = run(&source, &tiled, Some(&mut a), LIMIT, ptr::null())
                .unwrap()
                .unwrap();
            let want = run(&source, &whole, Some(&mut b), LIMIT, ptr::null())
                .unwrap()
                .unwrap();
            assert_eq!(a.calls.len(), 6, "{depth:?}: 40×28 at core 16 is 3×2 tiles");
            assert_eq!(b.calls.len(), 1, "{depth:?}: core 48 holds the image");
            assert_eq!((got.0, got.1), (w * scale, h * scale));
            assert!(
                got == want,
                "{depth:?} ×{scale} NR {nr} sharpen {sharpen}: the stitch is not seamless"
            );
        }
    }
}

#[test]
fn the_model_stitch_matches_its_oracle() {
    let (w, h) = (36, 20);
    let pixels = texture(w, h);
    for depth in [Depth::Byte, Depth::Sixteen] {
        let max = depth.max();
        let codes = codes(depth, &pixels);
        let source = Source::new(depth, w, h, &codes);
        for (scale, model, model_scale) in
            [(2, Model::Sub, 4), (4, Model::Sub, 4), (1, Model::Point, 1)]
        {
            let mut stub = Stub::new(model, model_scale as usize);
            let (ow, oh, got) = run(
                &source,
                &stage(32, 8, scale, model_scale, true),
                Some(&mut stub),
                LIMIT,
                ptr::null(),
            )
            .unwrap()
            .unwrap();
            let k = (model_scale / scale) as usize;
            let mut want = Vec::new();
            for oy in 0..oh {
                for ox in 0..ow {
                    let alpha = classical(&codes, w, h, scale, max, ox, oy)[3];
                    let p = codes[(oy / scale * w + ox / scale) as usize];
                    let mut pixel = [0.0; 4];
                    for c in 0..3 {
                        // The input as the core hands it over (f32), then the
                        // stub's cells under this output pixel, averaged.
                        let v = f64::from(p[c] as f32 / max as f32);
                        let mut sum = 0.0;
                        for j in 0..k {
                            for i in 0..k {
                                // The cell's place in its model_scale block.
                                let cx = (ox as usize % scale as usize) * k + i;
                                let cy = (oy as usize % scale as usize) * k + j;
                                sum += match model {
                                    Model::Point => 1.0 - 0.75 * v,
                                    Model::Sub => {
                                        0.5 * v
                                            + 0.1
                                                * ((cx % model_scale as usize
                                                    + cy % model_scale as usize)
                                                    as f64)
                                                / 6.0
                                    }
                                    Model::NotFinite => unreachable!(),
                                };
                            }
                        }
                        pixel[c] = (sum / (k * k) as f64).clamp(0.0, 1.0) * max;
                        pixel[c] = pixel[c].round();
                    }
                    pixel[3] = alpha;
                    want.push(pixel);
                }
            }
            within(&got, &want, 1.0, &format!("{depth:?} ×{scale}"));
            if scale == 1 {
                for (g, c) in got.iter().zip(&codes) {
                    assert_eq!(g[3], c[3], "{depth:?}: a scale-1 pass keeps alpha exactly");
                }
            }
        }
    }
}

#[test]
fn without_the_model_super_zoom_is_catmull_rom() {
    let (w, h) = (24, 18);
    let pixels = texture(w, h);
    for depth in [Depth::Byte, Depth::Sixteen] {
        let max = depth.max();
        let codes = codes(depth, &pixels);
        let source = Source::new(depth, w, h, &codes);
        for scale in [2, 4] {
            let (ow, oh, got) = run(
                &source,
                &stage(32, 8, scale, 4, false),
                None,
                LIMIT,
                ptr::null(),
            )
            .unwrap()
            .unwrap();
            let want: Vec<[f64; 4]> = (0..oh)
                .flat_map(|oy| (0..ow).map(move |ox| (ox, oy)))
                .map(|(ox, oy)| classical(&codes, w, h, scale, max, ox, oy))
                .collect();
            within(&got, &want, 1.0, &format!("{depth:?} classical ×{scale}"));
        }
        // A stage that cannot change a pixel answers NULL with no error.
        assert!(run(
            &source,
            &stage(32, 8, 1, 4, false),
            None,
            LIMIT,
            ptr::null()
        )
        .unwrap()
        .is_none());
    }
}

#[test]
fn the_model_runs_once_per_tile_in_order_on_clamped_reads() {
    let (w, h) = (40, 28);
    let pixels = texture(w, h);
    for depth in [Depth::Byte, Depth::Sixteen] {
        let max = depth.max() as f32;
        let codes = codes(depth, &pixels);
        let source = Source::new(depth, w, h, &codes);
        let mut stub = Stub::new(Model::Point, 4);
        run(
            &source,
            &stage(32, 8, 2, 4, true),
            Some(&mut stub),
            LIMIT,
            ptr::null(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(stub.calls, (0..6).collect::<Vec<u64>>());
        assert!(stub
            .lengths
            .iter()
            .all(|l| *l == (3 * 32 * 32, 3 * 128 * 128)));
        // Tile 0 reads from (−8, −8): every read clamped to the image.
        let input = &stub.inputs[0];
        for c in 0..3 {
            for y in 0..32_i64 {
                for x in 0..32_i64 {
                    let sx = (x - 8).clamp(0, i64::from(w) - 1) as usize;
                    let sy = (y - 8).clamp(0, i64::from(h) - 1) as usize;
                    let want = codes[sy * w as usize + sx][c] as f32 / max;
                    assert_eq!(input[c * 1024 + (y * 32 + x) as usize], want);
                }
            }
        }
        // Tile 1's core starts at x 16, so it reads from x 8.
        let input = &stub.inputs[1];
        assert_eq!(input[8 * 32], codes[8][0] as f32 / max);
    }
}

#[test]
fn refusals_name_their_cause() {
    let (w, h) = (40, 28);
    let pixels = texture(w, h);
    let source = Source::new(Depth::Byte, w, h, &codes(Depth::Byte, &pixels));
    let err = |stage: &Value, stub: Option<&mut Stub>| {
        run(&source, stage, stub, LIMIT, ptr::null()).unwrap_err()
    };
    assert_eq!(
        err(&stage(32, 8, 2, 4, true), None),
        "a neural stage with the model needs a predict callback"
    );
    let mut failing = Stub::new(Model::Point, 4);
    failing.fail_at = Some(2);
    assert_eq!(
        err(&stage(32, 8, 2, 4, true), Some(&mut failing)),
        "the model failed on tile 2"
    );
    assert_eq!(failing.calls, vec![0, 1, 2]);
    let mut nan = Stub::new(Model::NotFinite, 4);
    assert_eq!(
        err(&stage(32, 8, 2, 4, true), Some(&mut nan)),
        "the model failed on tile 0"
    );
    assert_eq!(
        err(&stage(8, 0, 2, 4, false), None),
        "neural tile must be 16 to 1024 pixels"
    );
    assert_eq!(
        err(&stage(32, 4, 2, 4, false), None),
        "neural tile pad must be a multiple of 8"
    );
    assert_eq!(
        err(&stage(32, 16, 2, 4, false), None),
        "neural tile core (tile − 2·pad) must be a multiple of 8, at least 8"
    );
    assert_eq!(
        err(&stage(32, 8, 2, 1, false), None),
        "neural zoom must divide the model's scale"
    );
    assert_eq!(
        err(&stage(32, 8, 3, 4, false), None),
        "neural zoom must be 1, 2 or 4"
    );
    assert_eq!(
        err(&stage(32, 8, 2, 2, false), None),
        "neural model scale must be 1 or 4"
    );
    assert_eq!(
        err(&with_sliders(stage(32, 8, 2, 4, false), 101.0, 0.0), None),
        "neural stage noise_reduction must be 0 to 100"
    );
    let mut unknown = stage(32, 8, 2, 4, false);
    unknown["face_enhancement"] = json!(true);
    assert!(err(&unknown, None).starts_with("invalid neural stage request: unknown field"));
    // The working limit, beside the retained source.
    let refused = run(&source, &stage(32, 8, 2, 4, false), None, 4096, ptr::null()).unwrap_err();
    assert!(refused.contains("working limit"), "{refused}");
    // A cancelled control.
    unsafe {
        let control = dr_job_control_create();
        dr_job_control_cancel(control);
        let cancelled = run(&source, &stage(32, 8, 2, 4, false), None, LIMIT, control).unwrap_err();
        assert_eq!(cancelled, "operation cancelled");
        dr_job_control_free(control);
    }
    // A float image refuses by name.
    let float =
        PrecisionImage::from_rgba32(4, 4, vec![[0.5; 4]; 16], IccProfile::srgb(), None).unwrap();
    let float = Source::Typed(Box::into_raw(Box::new(float)));
    assert_eq!(
        run(&float, &stage(32, 8, 2, 4, false), None, LIMIT, ptr::null()).unwrap_err(),
        "neural filters are not available in a 32-bit document"
    );
    // NULL image and stage.
    unsafe {
        let mut e = ptr::null_mut();
        let json = stage_json(&stage(32, 8, 2, 4, false));
        assert!(dr_image_neural_stage(
            ptr::null(),
            json.as_ptr(),
            None,
            ptr::null_mut(),
            LIMIT,
            0,
            ptr::null(),
            &mut e
        )
        .is_null());
        assert_eq!(take_err_string(e), "required handle is NULL");
        let Source::Byte(image) = &source else {
            unreachable!()
        };
        let mut e = ptr::null_mut();
        assert!(dr_image_neural_stage(
            *image,
            ptr::null(),
            None,
            ptr::null_mut(),
            LIMIT,
            0,
            ptr::null(),
            &mut e
        )
        .is_null());
        assert_eq!(take_err_string(e), "stage_json is NULL");
    }
}

fn info(w: u32, h: u32, stage: &Value, bps: u32) -> Result<Value, String> {
    let json = stage_json(stage);
    let mut err = ptr::null_mut();
    unsafe {
        let text = dr_neural_stage_info(w, h, json.as_ptr(), bps, &mut err);
        if text.is_null() {
            return Err(take_err_string(err));
        }
        let value = serde_json::from_str(CStr::from_ptr(text).to_str().unwrap()).unwrap();
        doctored_core::ffi::dr_string_free(text);
        Ok(value)
    }
}

#[test]
fn info_reports_the_grid_and_its_plan() {
    let zoom = stage(256, 16, 2, 4, true);
    let got = info(6000, 4000, &zoom, 1).unwrap();
    // Cores of 224: ⌈6000/224⌉ × ⌈4000/224⌉ = 27 × 18.
    assert_eq!(got["tiles"], json!(486));
    assert_eq!(got["output_width"], json!(12000));
    assert_eq!(got["output_height"], json!(8000));
    assert_eq!(got["input_len"], json!(3 * 256 * 256));
    assert_eq!(got["plane_len"], json!(3 * 1024 * 1024));
    let plain = got["planned_bytes"].as_u64().unwrap();
    // The output codes alone are 96 MP × 4 bytes.
    assert!(plain >= 96_000_000 * 4);
    let sliders = info(6000, 4000, &with_sliders(zoom.clone(), 50.0, 50.0), 1).unwrap();
    assert!(sliders["planned_bytes"].as_u64().unwrap() > plain);
    let wide = info(6000, 4000, &zoom, 2).unwrap();
    assert!(wide["planned_bytes"].as_u64().unwrap() > plain);
    // 12 MP ×4 is 192 MP: refused by its size.
    assert_eq!(
        info(4000, 3000, &stage(256, 16, 4, 4, true), 1).unwrap_err(),
        "the zoomed image would be 16000 × 12000 (192.0 megapixels), past the 100-megapixel \
         limit: choose a smaller zoom"
    );
    assert_eq!(
        info(64, 64, &zoom, 3).unwrap_err(),
        "bytes_per_sample must be 1 or 2"
    );
    for depth in [Depth::Byte, Depth::Sixteen] {
        assert!(info(64, 64, &zoom, depth.bps()).is_ok());
    }
}

#[test]
fn super_zoom_documents_carry_the_source_profile_and_resolution() {
    let (w, h) = (10, 6);
    let pixels = texture(w, h);
    let resolution = Resolution { x: 240.0, y: 180.0 };
    // 8 bits.
    let byte = codes(Depth::Byte, &pixels);
    let mut source = DrDocument::from_pixels(
        image::RgbaImage::from_raw(w, h, byte.iter().flat_map(|p| p.map(|v| v as u8)).collect())
            .unwrap(),
    );
    source.profile = IccProfile::adobe_rgb();
    source.resolution = resolution;
    let zoomed = Source::new(Depth::Byte, 20, 12, &codes(Depth::Byte, &texture(20, 12)));
    let Source::Byte(zoomed_image) = &zoomed else {
        unreachable!()
    };
    unsafe {
        let mut err = ptr::null_mut();
        let doc = dr_doc_super_zoom_document(&source, *zoomed_image, &mut err);
        assert!(!doc.is_null(), "{}", take_err_string(err));
        assert_eq!(((*doc).width, (*doc).height), (20, 12));
        assert_eq!((*doc).layers.len(), 1);
        assert_eq!(layer_name(doc, 0), "Background");
        assert_eq!((*doc).profile.bytes(), source.profile.bytes());
        assert_eq!((*doc).resolution, resolution);
        assert_eq!(layer_pixels(doc, 0), img_pixels(*zoomed_image));
        dr_doc_free(doc);
        let mut err = ptr::null_mut();
        assert!(dr_doc_super_zoom_document(ptr::null(), *zoomed_image, &mut err).is_null());
        assert_eq!(take_err_string(err), "required handle is NULL");
    }
    // 16 bits.
    let wide: Vec<[u16; 4]> = codes(Depth::Sixteen, &pixels)
        .iter()
        .map(|p| p.map(|v| v as u16))
        .collect();
    let typed = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, wide, IccProfile::prophoto_rgb(), None).unwrap(),
        Metadata::default(),
        resolution,
    )
    .unwrap();
    let zoomed_codes = codes(Depth::Sixteen, &texture(20, 12));
    let zoomed = Source::new(Depth::Sixteen, 20, 12, &zoomed_codes);
    let Source::Typed(zoomed_image) = &zoomed else {
        unreachable!()
    };
    unsafe {
        let mut err = ptr::null_mut();
        let doc = dr_precision_doc_super_zoom_document(
            &typed,
            *zoomed_image,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        assert!(!doc.is_null(), "{}", take_err_string(err));
        assert_eq!((*doc).dimensions(), (20, 12));
        assert_eq!((*doc).layer_count(), 1);
        assert_eq!((*doc).layer(0).unwrap().name(), "Background");
        assert_eq!((*doc).profile().bytes(), typed.profile().bytes());
        assert_eq!((*doc).resolution(), resolution);
        let layer = (*doc).layer_source(0).unwrap().to_rgba16().unwrap();
        assert_eq!(layer, (**zoomed_image).to_rgba16().unwrap());
        dr_precision_doc_free(doc);
    }
    // Float and CMYK refuse by name.
    let float = float_doc(4, 4, &IccProfile::srgb(), &[vec![[0.5, 0.5, 0.5, 1.0]; 16]]);
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_super_zoom_document(
            &float,
            *zoomed_image,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert_eq!(
            take_err_string(err),
            "neural filters are not available in a 32-bit document"
        );
        let profile = press_profile("Press Neural");
        let mut err = ptr::null_mut();
        let cmyk = dr_precision_doc_convert_to_cmyk(
            &typed,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        assert!(!cmyk.is_null(), "{}", take_err_string(err));
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_super_zoom_document(
            cmyk,
            *zoomed_image,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert_eq!(
            take_err_string(err),
            "neural filters are not available in a CMYK document"
        );
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_neural_layer_apply(
            cmyk,
            0,
            *zoomed_image,
            false,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert_eq!(
            take_err_string(err),
            "neural filters are not available in a CMYK document"
        );
        dr_precision_doc_free(cmyk);
    }
}

#[test]
fn the_typed_layer_apply_replaces_in_place_or_adds_above() {
    let (w, h) = (12, 8);
    let wide = |pixels: &[[f64; 4]]| -> Vec<[u16; 4]> {
        codes(Depth::Sixteen, pixels)
            .iter()
            .map(|p| p.map(|v| v as u16))
            .collect()
    };
    let original = wide(&texture(w, h));
    let doc = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, original.clone(), IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let mut restored = original.clone();
    for p in &mut restored {
        p[0] = 65535 - p[0];
    }
    let pixels = Box::into_raw(Box::new(
        PrecisionImage::from_rgba16(w, h, restored.clone(), IccProfile::srgb(), None).unwrap(),
    ));
    let same = Box::into_raw(Box::new(
        PrecisionImage::from_rgba16(w, h, original.clone(), IccProfile::srgb(), None).unwrap(),
    ));
    let small = Box::into_raw(Box::new(
        PrecisionImage::from_rgba16(4, 4, vec![[0; 4]; 16], IccProfile::srgb(), None).unwrap(),
    ));
    let name = CString::new("Background JPEG Artifacts Removal").unwrap();
    unsafe {
        let mut err = ptr::null_mut();
        let replaced = dr_precision_doc_neural_layer_apply(
            &doc,
            0,
            pixels,
            false,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        assert!(!replaced.is_null(), "{}", take_err_string(err));
        assert_eq!((*replaced).layer_count(), 1);
        assert_eq!(
            (*replaced).layer_source(0).unwrap().to_rgba16().unwrap(),
            restored
        );
        dr_precision_doc_free(replaced);

        let mut err = ptr::null_mut();
        let added = dr_precision_doc_neural_layer_apply(
            &doc,
            0,
            pixels,
            true,
            name.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        assert!(!added.is_null(), "{}", take_err_string(err));
        assert_eq!((*added).layer_count(), 2);
        assert_eq!(
            (*added).layer(1).unwrap().name(),
            "Background JPEG Artifacts Removal"
        );
        assert_eq!(
            (*added).layer(1).unwrap().offset(),
            (*added).layer(0).unwrap().offset()
        );
        assert_eq!(
            (*added).layer_source(1).unwrap().to_rgba16().unwrap(),
            restored
        );
        assert_eq!(
            (*added).layer_source(0).unwrap().to_rgba16().unwrap(),
            original
        );
        dr_precision_doc_free(added);

        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_neural_layer_apply(
            &doc,
            0,
            same,
            false,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(err.is_null(), "identical pixels change nothing");

        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_neural_layer_apply(
            &doc,
            0,
            small,
            false,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert_eq!(
            take_err_string(err),
            "the neural filter's pixels must be the layer's own size"
        );

        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_neural_layer_apply(
            &doc,
            3,
            pixels,
            false,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert_eq!(
            take_err_string(err),
            "neural filter layer 3 is out of range"
        );

        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_neural_layer_apply(
            &doc,
            0,
            pixels,
            true,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert_eq!(
            take_err_string(err),
            "a new neural filter layer needs a name"
        );

        dr_precision_image_free(pixels);
        dr_precision_image_free(same);
        dr_precision_image_free(small);
    }
}
