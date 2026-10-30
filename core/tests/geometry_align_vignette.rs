//! Auto-Align Layers' Vignette Removal: `remove_vignette`
//! at 8 bits, 16 bits and 32-bit float through the FFI
//! (`dr_doc_remove_vignette(_plan)`, `dr_precision_doc_remove_vignette(_plan)`).
//!
//! # The oracle
//!
//! Take2's geometry, written here: three 600 × 400 pinhole views at f 600,
//! yawed −20, 0 and +20°, each sampling one smooth LINEAR picture over the
//! directions, every layer at the canvas origin (as a stack is before it is
//! aligned), the bottom one the reference. Each layer's map is the test's
//! own T(c)·K·R·K⁻¹·T(−c). A known falloff V = 1 − 0.4ρ² (ρ over the
//! farthest pixel centre) is applied in each depth's linear light and
//! encoded; the removal must find the coefficient within 0.02 and the gain
//! within 0.01 of the closed-form mean of V, and every opaque output pixel
//! must be the picture times the reported gain: mean |Δ| ≤ 1.5 codes at
//! 8 bits, 1.5 × 257 at 16, and 1% relative at float. Take2's own
//! construction (1 − 0.3ρ² on 8-bit CODES) must reproduce Photoshop's ring
//! ratios on the reference (`scripts/photoshop-probe/panorama_lens_reread.py`)
//! within 0.03. No golden images.
#![allow(clippy::needless_range_loop)]

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use crate::cmyk_fixture::press_profile;
use crate::common::selected_pixels::take_err;
use crate::float_fixture::float_doc;
use doctored_core::doc::{DrDocument, LayerRaster};
use doctored_core::doc_lock::LOCK_PIXELS;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_align_vignette::{
    dr_doc_remove_vignette, dr_doc_remove_vignette_plan, dr_precision_doc_remove_vignette,
    dr_precision_doc_remove_vignette_plan,
};
use doctored_core::ffi_cmyk::dr_precision_doc_convert_to_cmyk;
use doctored_core::icc::IccProfile;
use doctored_core::icc_transform::PrecisionToneEncoder;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{GrayImage, RgbaImage};
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const SIZE: (u32, u32) = (600, 400);
const FOCAL: f64 = 600.0;
const YAWS: [f64; 3] = [-20.0, 0.0, 20.0];
/// Photoshop's left-frame ring ratios on take2 (vig ÷ rot over source ρ,
/// tenths; `panorama_lens_reread.py`).
const PS_LEFT_RINGS: [f64; 10] = [
    0.909, 0.910, 0.911, 0.911, 0.912, 0.913, 0.913, 0.914, 0.915, 0.917,
];
/// The documented plan terms: `PLAN_BOOKKEEPING` and `FIT_SCRATCH_BYTES`
/// (2^18 samples × a 32-byte record).
const BOOKKEEPING: u64 = 2 * 1024 * 1024;
const FIT_SCRATCH: u64 = (1 << 18) * 32;

/// The smooth linear picture over the unrolled directions, per channel.
fn picture(u: f64, v: f64, channel: usize) -> f64 {
    let base = 0.35
        + 0.22 * (u / 41.0 + 0.7).sin() * (v / 29.0 - 0.3).cos()
        + 0.1 * (u / 13.0 - v / 19.0).sin();
    base * [1.0, 0.92, 1.06][channel]
}

/// The view yawed `yaw` degrees at its pixel centre (fx, fy), linear.
fn view(yaw: f64, fx: f64, fy: f64, channel: usize) -> f64 {
    let (xc, yc) = (fx - f64::from(SIZE.0) / 2.0, fy - f64::from(SIZE.1) / 2.0);
    let theta = yaw.to_radians() + xc.atan2(FOCAL);
    picture(FOCAL * theta, FOCAL * yc / xc.hypot(FOCAL), channel)
}

fn radius() -> f64 {
    (f64::from(SIZE.0) / 2.0 - 0.5).hypot(f64::from(SIZE.1) / 2.0 - 0.5)
}

fn rho2(fx: f64, fy: f64) -> f64 {
    let (dx, dy) = (fx - f64::from(SIZE.0) / 2.0, fy - f64::from(SIZE.1) / 2.0);
    (dx * dx + dy * dy) / (radius() * radius())
}

/// The closed-form mean of 1 + a·ρ² over the frame's pixel centres.
fn mean_falloff(a: f64) -> f64 {
    let (w, h) = (f64::from(SIZE.0), f64::from(SIZE.1));
    1.0 + a * ((w * w - 1.0) / 12.0 + (h * h - 1.0) / 12.0) / (radius() * radius())
}

fn encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// The map of the layer yawed `yaw` onto the reference yawed `YAWS[0]`:
/// T(c)·K·R_y(yaw − yaw₀)·K⁻¹·T(−c), row-major.
fn map(yaw: f64) -> [f64; 9] {
    let a = (yaw - YAWS[0]).to_radians();
    let (c, s) = (a.cos(), a.sin());
    let (cx, cy) = (f64::from(SIZE.0) / 2.0, f64::from(SIZE.1) / 2.0);
    // K·R·K⁻¹ with R = [c 0 s; 0 1 0; −s 0 c].
    let m = [c, 0.0, s * FOCAL, 0.0, 1.0, 0.0, -s / FOCAL, 0.0, c];
    let t = [1.0, 0.0, cx, 0.0, 1.0, cy, 0.0, 0.0, 1.0];
    let back = [1.0, 0.0, -cx, 0.0, 1.0, -cy, 0.0, 0.0, 1.0];
    product(&t, &product(&m, &back))
}

fn product(a: &[f64; 9], b: &[f64; 9]) -> [f64; 9] {
    let mut out = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            out[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
        }
    }
    out
}

fn request() -> Value {
    json!({
        "op": "remove_vignette",
        "layers": YAWS.iter().enumerate()
            .map(|(layer, yaw)| json!({"layer": layer, "map": map(*yaw).to_vec()}))
            .collect::<Vec<_>>(),
    })
}

/// Each frame's samples in 0…1 of its depth's encoding: `value(yaw, x, y,
/// channel)`.
fn frames(value: impl Fn(f64, f64, f64, usize) -> f64) -> Vec<Vec<[f64; 3]>> {
    YAWS.iter()
        .map(|yaw| {
            (0..SIZE.0 * SIZE.1)
                .map(|i| {
                    let (fx, fy) = (f64::from(i % SIZE.0) + 0.5, f64::from(i / SIZE.0) + 0.5);
                    std::array::from_fn(|c| value(*yaw, fx, fy, c))
                })
                .collect()
        })
        .collect()
}

/// The linear picture with V = 1 + a·ρ² applied.
fn vignetted(a: f64) -> impl Fn(f64, f64, f64, usize) -> f64 {
    move |yaw, fx, fy, c| view(yaw, fx, fy, c) * (1.0 + a * rho2(fx, fy))
}

fn byte_doc(frames: &[Vec<[f64; 3]>]) -> DrDocument {
    let image = |samples: &Vec<[f64; 3]>| {
        RgbaImage::from_fn(SIZE.0, SIZE.1, |x, y| {
            let p = samples[(y * SIZE.0 + x) as usize];
            image::Rgba([
                (p[0] * 255.0).round() as u8,
                (p[1] * 255.0).round() as u8,
                (p[2] * 255.0).round() as u8,
                255,
            ])
        })
    };
    let mut doc = DrDocument::from_pixels(image(&frames[0]));
    let base = doc.layers[0].clone();
    for samples in &frames[1..] {
        let mut layer = base.clone();
        layer.pixels = LayerRaster::new(image(samples));
        doc.layers.push(layer);
    }
    doc
}

fn typed16(frames: &[Vec<[f64; 3]>]) -> PrecisionDocument {
    let records = frames
        .iter()
        .enumerate()
        .map(|(k, samples)| {
            let px = samples
                .iter()
                .map(|p| {
                    let c = |v: f64| (v * 65535.0).round() as u16;
                    [c(p[0]), c(p[1]), c(p[2]), 65535]
                })
                .collect();
            let image =
                PrecisionImage::from_rgba16(SIZE.0, SIZE.1, px, IccProfile::srgb(), None).unwrap();
            PrecisionDocumentLayer::raster(&format!("L{k}"), image, (0, 0), 1.0, true).unwrap()
        })
        .collect();
    PrecisionDocument::from_parts(
        SIZE.0,
        SIZE.1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        records,
    )
    .unwrap()
}

fn typed32(frames: &[Vec<[f64; 3]>]) -> PrecisionDocument {
    let records: Vec<Vec<[f32; 4]>> = frames
        .iter()
        .map(|samples| {
            samples
                .iter()
                .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32, 1.0])
                .collect()
        })
        .collect();
    float_doc(SIZE.0, SIZE.1, &IccProfile::srgb(), &records)
}

enum TestDoc {
    Byte(DrDocument),
    Typed(PrecisionDocument),
}

fn string(text: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(text) };
    out
}

impl TestDoc {
    /// Layer `index`'s straight samples in storage units.
    fn layer(&self, index: usize) -> Vec<[f64; 4]> {
        match self {
            TestDoc::Byte(d) => {
                let (w, h) = d.layers[index].pixels.dimensions();
                let px = d.layers[index].pixels.read_rect(0, 0, w, h).unwrap();
                px.iter().map(|p| p.map(f64::from)).collect()
            }
            TestDoc::Typed(d) => {
                let image = d.layer(index).unwrap().source().unwrap();
                if d.is_float() {
                    let samples = image.to_rgba32().unwrap();
                    samples.iter().map(|p| p.map(f64::from)).collect()
                } else {
                    let samples = image.to_rgba16().unwrap();
                    samples.iter().map(|p| p.map(f64::from)).collect()
                }
            }
        }
    }

    fn plan(&self, request: &Value) -> Result<Value, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                TestDoc::Byte(d) => {
                    dr_doc_remove_vignette_plan(d, text.as_ptr(), LIMIT, 0, &mut err)
                }
                TestDoc::Typed(d) => {
                    dr_precision_doc_remove_vignette_plan(d, text.as_ptr(), LIMIT, 0, &mut err)
                }
            }
        };
        if let Some(message) = take_err(err) {
            assert!(out.is_null());
            return Err(message);
        }
        Ok(serde_json::from_str(&string(out)).unwrap())
    }

    /// The edit: `Ok(None)` for no change.
    fn edit(&self, request: &Value) -> Result<Option<TestDoc>, String> {
        let text = CString::new(request.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe {
            match self {
                TestDoc::Byte(d) => {
                    dr_doc_remove_vignette(d, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err)
                        .cast::<u8>()
                }
                TestDoc::Typed(d) => dr_precision_doc_remove_vignette(
                    d,
                    text.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
                .cast::<u8>(),
            }
        };
        if let Some(message) = take_err(err) {
            assert!(out.is_null());
            return Err(message);
        }
        if out.is_null() {
            return Ok(None);
        }
        Ok(Some(unsafe {
            match self {
                TestDoc::Byte(_) => TestDoc::Byte(*Box::from_raw(out.cast::<DrDocument>())),
                TestDoc::Typed(_) => {
                    TestDoc::Typed(*Box::from_raw(out.cast::<PrecisionDocument>()))
                }
            }
        }))
    }
}

#[derive(Clone, Copy)]
enum Depth {
    Byte,
    Sixteen,
    Float,
}

/// Case (a) at one depth: the coefficient, the gain and the pixels.
fn known_falloff(depth: Depth) {
    const A: f64 = -0.4;
    let doc = match depth {
        Depth::Byte => TestDoc::Byte(byte_doc(&frames(move |yaw, x, y, c| {
            encode(vignetted(A)(yaw, x, y, c))
        }))),
        Depth::Sixteen => TestDoc::Typed(typed16(&frames(move |yaw, x, y, c| {
            encode(vignetted(A)(yaw, x, y, c))
        }))),
        Depth::Float => TestDoc::Typed(typed32(&frames(vignetted(A)))),
    };
    let request = request();
    let plan = doc.plan(&request).unwrap();
    let coefficient = plan["coefficient"].as_f64().unwrap();
    let gain = plan["gain"].as_f64().unwrap();
    let samples = plan["samples"].as_u64().unwrap();
    let label = match depth {
        Depth::Byte => "8-bit",
        Depth::Sixteen => "16-bit",
        Depth::Float => "float",
    };
    println!("{label}: coefficient {coefficient:.4}, gain {gain:.4}, samples {samples}");
    assert!(
        (coefficient - A).abs() <= 0.02,
        "{label} coefficient {coefficient}"
    );
    assert!(
        (gain - mean_falloff(A)).abs() <= 0.01,
        "{label} gain {gain} against {}",
        mean_falloff(A)
    );
    assert_eq!(plan["layers"], json!([0, 1, 2]));
    let out = doc
        .edit(&request)
        .unwrap()
        .expect("the vignette changes the layers");
    let (mut sum, mut count) = (0.0, 0usize);
    for (layer, yaw) in YAWS.iter().enumerate() {
        let px = out.layer(layer);
        for (i, p) in px.iter().enumerate() {
            let (fx, fy) = (
                f64::from(i as u32 % SIZE.0) + 0.5,
                f64::from(i as u32 / SIZE.0) + 0.5,
            );
            for c in 0..3 {
                let truth = view(*yaw, fx, fy, c) * gain;
                sum += match depth {
                    Depth::Byte => (p[c] - encode(truth) * 255.0).abs(),
                    Depth::Sixteen => (p[c] - encode(truth) * 65535.0).abs(),
                    Depth::Float => (p[c] - truth).abs() / truth,
                };
                count += 1;
            }
            assert_eq!(p[3], doc_max(depth), "alpha stays");
        }
    }
    let mean = sum / count as f64;
    println!("{label}: mean |out - truth x gain| {mean:.5}");
    match depth {
        Depth::Byte => assert!(mean <= 1.5, "8-bit mean {mean} codes"),
        Depth::Sixteen => assert!(mean <= 1.5 * 257.0, "16-bit mean {mean} codes"),
        Depth::Float => assert!(mean <= 0.01, "float mean relative {mean}"),
    }
}

fn doc_max(depth: Depth) -> f64 {
    match depth {
        Depth::Byte => 255.0,
        Depth::Sixteen => 65535.0,
        Depth::Float => 1.0,
    }
}

#[test]
fn a_known_falloff_is_removed_at_8_bits() {
    known_falloff(Depth::Byte);
}

#[test]
fn a_known_falloff_is_removed_at_16_bits() {
    known_falloff(Depth::Sixteen);
}

#[test]
fn a_known_falloff_is_removed_at_float() {
    known_falloff(Depth::Float);
}

/// Case (b): take2's construction, 1 − 0.3ρ² on 8-bit codes, against
/// Photoshop's ring ratios on the reference.
#[test]
fn take2_codes_reproduce_photoshops_rings() {
    let truth = frames(|yaw, x, y, c| encode(view(yaw, x, y, c)));
    let input = frames(|yaw, x, y, c| encode(view(yaw, x, y, c)) * (1.0 - 0.3 * rho2(x, y)));
    let doc = TestDoc::Byte(byte_doc(&input));
    let plan = doc.plan(&request()).unwrap();
    let out = doc.edit(&request()).unwrap().expect("a change");
    let px = out.layer(0);
    let mut sums = [(0.0, 0.0); 10];
    for (i, p) in px.iter().enumerate() {
        let (fx, fy) = (
            f64::from(i as u32 % SIZE.0) + 0.5,
            f64::from(i as u32 / SIZE.0) + 0.5,
        );
        let ring = ((rho2(fx, fy).sqrt() * 10.0) as usize).min(9);
        for c in 0..3 {
            sums[ring].0 += p[c];
            sums[ring].1 += truth[0][i][c] * 255.0;
        }
    }
    let rings: Vec<f64> = sums.iter().map(|(o, t)| o / t).collect();
    let worst = rings
        .iter()
        .zip(PS_LEFT_RINGS)
        .map(|(ours, ps)| (ours - ps).abs())
        .fold(0.0, f64::max);
    println!(
        "take2 codes: coefficient {:.4}, gain {:.4}, rings {}, largest |ours - Photoshop| {worst:.4}",
        plan["coefficient"].as_f64().unwrap(),
        plan["gain"].as_f64().unwrap(),
        rings
            .iter()
            .map(|r| format!("{r:.3}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    assert!(worst <= 0.03, "rings {rings:?} against {PS_LEFT_RINGS:?}");
}

/// Case (c): alpha, transparent pixels, masks and offsets are kept.
#[test]
fn alpha_transparency_and_masks_are_kept() {
    let mut byte = byte_doc(&frames(move |yaw, x, y, c| {
        encode(vignetted(-0.4)(yaw, x, y, c))
    }));
    let mut image = byte.layers[1].pixels.to_contiguous().unwrap();
    for y in 0..SIZE.1 {
        for x in 0..SIZE.0 {
            let p = image.get_pixel_mut(x, y);
            if x < 40 && y < 40 {
                p.0 = [200, 100, 50, 0];
            } else if x >= 560 && y >= 360 {
                p.0[3] = 128;
            }
        }
    }
    byte.layers[1].pixels = LayerRaster::new(image);
    let mask = Arc::new(GrayImage::from_pixel(SIZE.0, SIZE.1, image::Luma([180])));
    byte.layers[1].mask = Some(mask.clone());
    byte.layers[1].mask_properties = MaskProperties::at((0, 0));
    let before = TestDoc::Byte(byte);
    let pixels = before.layer(1);
    let TestDoc::Byte(out) = before.edit(&request()).unwrap().expect("a change") else {
        unreachable!()
    };
    let TestDoc::Byte(source) = &before else {
        unreachable!()
    };
    let after = TestDoc::Byte(out.clone()).layer(1);
    for (p, q) in pixels.iter().zip(&after) {
        assert_eq!(p[3], q[3], "alpha is kept");
        if p[3] == 0.0 {
            assert_eq!(p, q, "a transparent pixel is untouched");
        }
    }
    assert!(pixels != after, "the opaque pixels change");
    assert!(Arc::ptr_eq(out.layers[1].mask.as_ref().unwrap(), &mask));
    for (a, b) in source.layers.iter().zip(&out.layers) {
        assert_eq!(a.offset, b.offset);
        assert_eq!(a.mask_properties.offset, b.mask_properties.offset);
    }
}

/// Case (c): a flat-colour stack changes no sample, so the edit answers no
/// change and the plan fits no falloff.
#[test]
fn a_flat_stack_changes_nothing() {
    let flat = RgbaImage::from_pixel(SIZE.0, SIZE.1, image::Rgba([128, 128, 128, 255]));
    let mut doc = DrDocument::from_pixels(flat);
    let base = doc.layers[0].clone();
    doc.layers.push(base.clone());
    doc.layers.push(base);
    let shift = |dx: f64| vec![1.0, 0.0, dx, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let request = json!({"op": "remove_vignette", "layers": [
        {"layer": 0, "map": shift(0.0)}, {"layer": 1, "map": shift(100.0)},
        {"layer": 2, "map": shift(200.0)},
    ]});
    let doc = TestDoc::Byte(doc);
    let plan = doc.plan(&request).unwrap();
    let coefficient = plan["coefficient"].as_f64().unwrap();
    assert!(coefficient.abs() <= 1e-6, "coefficient {coefficient}");
    assert!(doc.edit(&request).unwrap().is_none(), "no sample changes");
}

/// Case (d): the plan's documented formula at each depth.
#[test]
fn the_plan_is_the_documented_formula() {
    let n = u64::from(SIZE.0) * u64::from(SIZE.1);
    let tiles = u64::from(SIZE.0.div_ceil(256)) * u64::from(SIZE.1.div_ceil(256));
    // The exposure system for 3 layers: two 3 × 3 f64 matrices and three
    // vectors of 3.
    let system = 3 * 3 * 16 + 3 * 24;
    let picture = frames(move |yaw, x, y, c| encode(vignetted(-0.4)(yaw, x, y, c)));
    let linear = frames(vignetted(-0.4));
    for (doc, output, write) in [
        // 8 bits: the largest layer's contiguous copy.
        (TestDoc::Byte(byte_doc(&picture)), 3 * n * 4, 4 * n),
        // 16 bits: its copy, the tiles' adoption copy and the encoder.
        (
            TestDoc::Typed(typed16(&picture)),
            3 * n * 8,
            8 * n + (8 * n + 64 * tiles) + PrecisionToneEncoder::scratch_bytes(),
        ),
        // Float: its copy and the adoption copy.
        (
            TestDoc::Typed(typed32(&linear)),
            3 * n * 16,
            16 * n + (16 * n + 64 * tiles),
        ),
    ] {
        let plan = doc.plan(&request()).unwrap();
        assert_eq!(plan["output_bytes"], json!(output));
        assert_eq!(
            plan["scratch_bytes"],
            json!(BOOKKEEPING + FIT_SCRATCH + system + write)
        );
        assert!(plan["retained_bytes"].as_u64().unwrap() >= output);
        assert!(plan["samples"].as_u64().unwrap() >= 1_000);
    }
}

fn refusal(doc: &TestDoc, request: Value) -> String {
    let planned = doc.plan(&request).expect_err("the plan refuses");
    let edited = doc.edit(&request).err().expect("the edit refuses");
    assert_eq!(planned, edited);
    planned
}

/// Case (e): every refusal sentence.
#[test]
fn refusals_name_the_reason() {
    let picture = frames(move |yaw, x, y, c| encode(vignetted(-0.4)(yaw, x, y, c)));
    let doc = TestDoc::Byte(byte_doc(&picture));
    let good = request();
    let with = |key: &str, value: Value| {
        let mut r = good.clone();
        r[key] = value;
        r
    };
    let identity = vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    assert_eq!(
        refusal(&doc, with("op", json!("project_layer"))),
        "invalid remove vignette request: op must be remove_vignette"
    );
    assert_eq!(
        refusal(&doc, with("extra", json!(1))),
        "invalid remove vignette request: unknown field `extra`"
    );
    assert_eq!(
        refusal(&doc, with("layers", json!([{"layer": 0, "map": identity}]))),
        "invalid remove vignette request: layers must name at least 2 layers"
    );
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([{"layer": 0, "map": identity}, {"layer": 0, "map": identity}])
            )
        ),
        "invalid remove vignette request: layer 0 is listed twice"
    );
    let mirrored = vec![-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let map_refusal =
        "invalid remove vignette request: map must be 9 finite numbers with a positive determinant";
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([{"layer": 0, "map": identity}, {"layer": 1, "map": mirrored}])
            )
        ),
        map_refusal
    );
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([{"layer": 0, "map": identity}, {"layer": 1, "map": [1, 0, 0]}])
            )
        ),
        map_refusal
    );
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([{"layer": 0, "map": identity}, {"layer": 1, "map": identity, "x": 1}])
            )
        ),
        "invalid remove vignette request: unknown field `x`"
    );
    let far = vec![1.0, 0.0, 5000.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([{"layer": 0, "map": identity}, {"layer": 1, "map": far}])
            )
        ),
        "Vignette Removal found too little overlap between the layers to measure the \
         vignette. Nothing was changed."
    );
    assert_eq!(
        refusal(
            &doc,
            with(
                "layers",
                json!([{"layer": 0, "map": identity}, {"layer": 7, "map": identity}])
            )
        ),
        "Vignette Removal cannot change layer 7: there is no such layer."
    );
    let mut locked = byte_doc(&picture);
    locked.layers[2].locks = LOCK_PIXELS;
    assert_eq!(
        refusal(&TestDoc::Byte(locked), good.clone()),
        "Vignette Removal cannot change layer 2: it is locked (pixels)."
    );
    let mut described = byte_doc(&picture);
    described.layers[1].meta = Some(json!({"type":"text"}).to_string());
    assert_eq!(
        refusal(&TestDoc::Byte(described), good.clone()),
        "Vignette Removal cannot change layer 1: it holds artwork or a placed source; \
         rasterize it first."
    );
    // CMYK: a two-layer 16-bit stack converted into the press profile.
    let small: Vec<Vec<[f64; 3]>> = (0..2).map(|_| vec![[0.4, 0.5, 0.6]; 64]).collect();
    let rgb = |samples: &Vec<[f64; 3]>| {
        let px = samples
            .iter()
            .map(|p| p.map(|v| (v * 65535.0) as u16))
            .map(|p| [p[0], p[1], p[2], 65535])
            .collect();
        let image = PrecisionImage::from_rgba16(8, 8, px, IccProfile::srgb(), None).unwrap();
        PrecisionDocumentLayer::raster("L", image, (0, 0), 1.0, true).unwrap()
    };
    let source = Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            8,
            8,
            IccProfile::srgb(),
            Metadata::default(),
            Resolution::default(),
            small.iter().map(rgb).collect(),
        )
        .unwrap(),
    ));
    let profile = press_profile("Press Test");
    let mut err = ptr::null_mut();
    let cmyk = unsafe {
        dr_precision_doc_convert_to_cmyk(
            source,
            profile.as_ptr(),
            profile.len(),
            c"{}".as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!cmyk.is_null(), "to CMYK: {:?}", take_err(err));
    unsafe { drop(Box::from_raw(source)) };
    let cmyk = TestDoc::Typed(unsafe { *Box::from_raw(cmyk) });
    let pair = json!({"op": "remove_vignette", "layers": [
        {"layer": 0, "map": identity}, {"layer": 1, "map": identity},
    ]});
    assert_eq!(
        refusal(&cmyk, pair),
        "Vignette Removal does not work in CMYK."
    );
    let typed = TestDoc::Typed(typed16(&picture));
    assert_eq!(
        refusal(&typed, with("op", json!("x"))),
        "invalid remove vignette request: op must be remove_vignette"
    );
}

/// Case (f): NULL handles.
#[test]
fn null_handles_answer_null_with_no_error() {
    let text = CString::new("{}").unwrap();
    let mut err = ptr::null_mut();
    unsafe {
        assert!(
            dr_doc_remove_vignette_plan(ptr::null(), text.as_ptr(), LIMIT, 0, &mut err).is_null()
        );
        assert!(dr_doc_remove_vignette(
            ptr::null(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(dr_precision_doc_remove_vignette_plan(
            ptr::null(),
            text.as_ptr(),
            LIMIT,
            0,
            &mut err
        )
        .is_null());
        assert!(dr_precision_doc_remove_vignette(
            ptr::null(),
            text.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        let doc = DrDocument::from_pixels(RgbaImage::new(4, 4));
        assert!(
            dr_doc_remove_vignette_plan(&doc, ptr::null(), LIMIT, 0, ptr::null_mut()).is_null()
        );
    }
    assert!(err.is_null());
}
