//! Filters ▸ Lens Correction: the `lens_correction` `SpatialKernel` arm
//! behind `dr_image_filter_op` at 8 bits and `selected_edit` at 16. The
//! geometry oracle is the documented inverse map, written here from the
//! header's formula, over a linear opaque ramp: Catmull-Rom reproduces a linear
//! function exactly wherever all 16 taps are inside, so an interior pixel is
//! the ramp at its mapped source point (±1 level for the two roundings). The
//! vignette oracle is an independent IEC 61966-2-1 sRGB transfer pair. The
//! fold threshold is the test's own, r² = 1/(3·0.08) for Distortion 100. The
//! cancel case parks the worker at a checkpoint taken from a fresh twin's own
//! report log, so nothing depends on timing.
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::{dr_image_filter_op, dr_image_filter_op_scratch_bytes};
use doctored_core::ffi_jobs::{dr_job_control_create, dr_job_control_free};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::selected_adjustment::SelectedEdit;
use doctored_core::DrImage;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::common;
use common::probe::{cancelled_parked_at, reports};

fn image(w: u32, h: u32, pixels: &[u8]) -> *mut DrImage {
    let p = unsafe { dr_image_from_rgba(pixels.as_ptr(), w, h) };
    assert!(!p.is_null());
    p
}

/// `(pixels, message)`: `pixels` is None for a NULL result, `message` empty
/// when no error was written.
fn run(
    img: *const DrImage,
    params: &str,
    control: *const DrJobControl,
    count: usize,
) -> (Option<Vec<u8>>, String) {
    let op = CString::new("lens_correction").unwrap();
    let params = CString::new(params).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), control, &mut err) };
    let pixels = (!out.is_null()).then(|| {
        let v =
            unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), count * 4) }.to_vec();
        unsafe { dr_image_free(out) };
        v
    });
    let message = if err.is_null() {
        String::new()
    } else {
        let m = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(err) };
        m
    };
    (pixels, message)
}

fn filter(w: u32, h: u32, pixels: &[u8], params: &Value) -> Vec<u8> {
    let img = image(w, h, pixels);
    let (out, message) = run(img, &params.to_string(), ptr::null(), (w * h) as usize);
    unsafe { dr_image_free(img) };
    assert!(message.is_empty(), "{params}: {message}");
    out.unwrap_or_else(|| panic!("{params} answered the bypass"))
}

/// The ramp's three channels at pixel index `(x, y)`: each linear in the
/// pixel coordinates and inside 0…255 for plates up to 100 pixels.
fn ramp_at(x: f64, y: f64) -> [f64; 3] {
    [40.0 + 2.0 * x, 30.0 + 2.0 * y, 20.0 + x + y]
}

fn ramp(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let v = ramp_at(f64::from(i % w), f64::from(i / w));
            [v[0] as u8, v[1] as u8, v[2] as u8, 255]
        })
        .collect()
}

/// Structure and mixed alpha (opaque, partial, a transparent pixel with RGB 0).
fn plate(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            let value = |offset: u32| ((x * 23 + y * 41 + offset + (i % 3) * 17) % 256) as u8;
            if i % 13 == 5 {
                return [0, 0, 0, 0];
            }
            [
                value(0),
                value(80),
                value(160),
                if i % 4 == 1 {
                    128 + (i % 64) as u8
                } else {
                    255
                },
            ]
        })
        .collect()
}

/// The parameters as the header states them, with their defaults.
#[derive(Clone, Copy)]
struct Lens {
    distortion: f64,
    red_cyan: f64,
    blue_yellow: f64,
    vertical: f64,
    horizontal: f64,
    angle: f64,
    scale: f64,
}

impl Lens {
    fn of(params: &Value) -> Self {
        let get =
            |key: &str, default: f64| params.get(key).and_then(Value::as_f64).unwrap_or(default);
        Lens {
            distortion: get("distortion", 0.0),
            red_cyan: get("chromatic_red_cyan", 0.0),
            blue_yellow: get("chromatic_blue_yellow", 0.0),
            vertical: get("vertical_perspective", 0.0),
            horizontal: get("horizontal_perspective", 0.0),
            angle: get("angle", 0.0),
            scale: get("scale", 100.0),
        }
    }

    /// The normalised point before the radial step, or None behind the
    /// perspective's horizon.
    fn pre_radial(&self, w: u32, h: u32, x: u32, y: u32) -> Option<(f64, f64)> {
        let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
        let r = f64::from(w.max(h)) / 2.0;
        let s = self.scale / 100.0;
        let (nx, ny) = (
            (f64::from(x) + 0.5 - cx) / r / s,
            (f64::from(y) + 0.5 - cy) / r / s,
        );
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let (nx, ny) = (nx * cos - ny * sin, nx * sin + ny * cos);
        let d = 1.0 + 0.004 * (self.horizontal * nx + self.vertical * ny);
        (d > 0.0).then(|| (nx / d, ny / d))
    }

    /// The source point (pixel centres at half-integers) of channel `c`, or
    /// None outside the picture.
    fn source(&self, w: u32, h: u32, x: u32, y: u32, c: usize) -> Option<(f64, f64)> {
        let (nx, ny) = self.pre_radial(w, h, x, y)?;
        let k1 = -0.0008 * self.distortion;
        let r2 = nx * nx + ny * ny;
        if k1 < 0.0 && r2 >= 1.0 / (3.0 * -k1) {
            return None;
        }
        let lateral = match c {
            0 => 1.0 + 0.00002 * self.red_cyan,
            2 => 1.0 + 0.00002 * self.blue_yellow,
            _ => 1.0,
        };
        let f = (1.0 + k1 * r2) * lateral;
        let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
        let r = f64::from(w.max(h)) / 2.0;
        Some((cx + r * nx * f, cy + r * ny * f))
    }
}

/// Whether a source coordinate's 4×4 Catmull-Rom taps are all inside.
fn interior(s: f64, size: u32) -> bool {
    s >= 2.0 && s <= f64::from(size) - 3.0
}

/// Every pixel of a ramp plate whose taps are inside is the ramp at its source
/// point, each channel at its own; returns how many pixels were checked.
fn assert_ramp_map(w: u32, h: u32, params: &Value) -> usize {
    let lens = Lens::of(params);
    let out = filter(w, h, &ramp(w, h), params);
    let mut checked = 0;
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let sources: Vec<Option<(f64, f64)>> =
                (0..3).map(|c| lens.source(w, h, x, y, c)).collect();
            if !sources
                .iter()
                .all(|s| s.is_some_and(|(sx, sy)| interior(sx, w) && interior(sy, h)))
            {
                continue;
            }
            for c in 0..3 {
                let (sx, sy) = sources[c].unwrap();
                let expected = ramp_at(sx - 0.5, sy - 0.5)[c];
                let got = f64::from(out[i + c]);
                assert!(
                    (got - expected).abs() <= 1.0,
                    "{params} pixel {x},{y} channel {c}: {got} vs {expected}"
                );
            }
            assert_eq!(out[i + 3], 255, "{params} pixel {x},{y} alpha");
            checked += 1;
        }
    }
    checked
}

#[test]
fn defaults_are_the_exact_bypass_and_plan_nothing() {
    let (w, h) = (24_u32, 18_u32);
    let img = image(w, h, &plate(w, h));
    for params in [
        "{}",
        r#"{"distortion":0,"scale":100,"vignette_midpoint":20}"#,
    ] {
        let (out, message) = run(img, params, ptr::null(), (w * h) as usize);
        assert!(out.is_none() && message.is_empty(), "{params}: {message}");
    }
    let plan = |params: &str| {
        let (name, text) = (
            CString::new("lens_correction").unwrap(),
            CString::new(params).unwrap(),
        );
        let mut err: *mut c_char = ptr::null_mut();
        let bytes = unsafe {
            dr_image_filter_op_scratch_bytes(img, name.as_ptr(), text.as_ptr(), &mut err)
        };
        assert!(err.is_null(), "{params}");
        bytes
    };
    assert_eq!(plan("{}"), 0);
    for params in [
        r#"{"distortion":10}"#,
        r#"{"vignette_amount":-20}"#,
        r#"{"chromatic_red_cyan":30}"#,
        r#"{"angle":1}"#,
    ] {
        // At least the n×16 plane the kernel returns.
        assert!(plan(params) >= u64::from(w * h) * 16, "{params}");
    }
    unsafe { dr_image_free(img) };
}

#[test]
fn interior_pixels_are_the_ramp_at_the_mapped_point() {
    let (w, h) = (96_u32, 80_u32);
    for params in [
        json!({"scale":50}),
        json!({"scale":150}),
        json!({"distortion":100}),
        json!({"distortion":-100}),
        json!({"vertical_perspective":100}),
        json!({"vertical_perspective":-100}),
        json!({"horizontal_perspective":100}),
        json!({"horizontal_perspective":-100}),
        json!({"angle":30}),
        json!({"scale":120,"distortion":60,"vertical_perspective":-40,
            "horizontal_perspective":30,"angle":30}),
        json!({"scale":80,"distortion":-50,"vertical_perspective":70,
            "horizontal_perspective":-60,"angle":30}),
    ] {
        let checked = assert_ramp_map(w, h, &params);
        assert!(checked > 500, "{params}: only {checked} interior pixels");
    }
}

#[test]
fn chromatic_aberration_scales_red_and_blue_and_leaves_green() {
    let (w, h) = (96_u32, 80_u32);
    for params in [
        json!({"chromatic_red_cyan":100}),
        json!({"chromatic_red_cyan":-100,"chromatic_blue_yellow":100}),
        json!({"chromatic_blue_yellow":-100}),
    ] {
        assert!(assert_ramp_map(w, h, &params) > 4000, "{params}");
        let src = ramp(w, h);
        let out = filter(w, h, &src, &params);
        for y in 2..h - 2 {
            for x in 2..w - 2 {
                let i = ((y * w + x) * 4) as usize;
                assert_eq!(out[i + 1], src[i + 1], "{params} green at {x},{y}");
                assert_eq!(out[i + 3], 255, "{params} alpha at {x},{y}");
            }
        }
    }
}

#[test]
fn angle_90_on_a_square_plate_is_an_exact_rotation() {
    let w = 30_u32;
    let src = plate(w, w);
    let out = filter(w, w, &src, &json!({"angle":90}));
    for y in 0..w {
        for x in 0..w {
            let o = ((y * w + x) * 4) as usize;
            let s = ((x * w + (w - 1 - y)) * 4) as usize;
            assert_eq!(out[o..o + 4], src[s..s + 4], "{x},{y}");
        }
    }
    // Counterclockwise as seen: the top centre shows the source's right centre.
    let top = ((w / 2) * 4) as usize;
    let right = (((w / 2) * w + w - 1) * 4) as usize;
    assert_eq!(out[top..top + 4], src[right..right + 4]);
}

#[test]
fn scale_50_leaves_the_corners_transparent() {
    let (w, h) = (40_u32, 30_u32);
    let out = filter(w, h, &ramp(w, h), &json!({"scale":50}));
    for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
        let i = ((y * w + x) * 4) as usize;
        assert_eq!(out[i..i + 4], [0, 0, 0, 0], "corner {x},{y}");
    }
}

#[test]
fn a_point_past_the_fold_radius_is_transparent() {
    let threshold = 1.0 / (3.0 * 0.08);
    let w = 80_u32;
    // Scale 50 doubles every radius: the corners are past the fold.
    let params = json!({"scale":50,"distortion":100});
    let lens = Lens::of(&params);
    let out = filter(w, w, &ramp(w, w), &params);
    for y in 0..w {
        for x in 0..w {
            let (nx, ny) = lens.pre_radial(w, w, x, y).unwrap();
            if nx * nx + ny * ny >= threshold {
                let i = ((y * w + x) * 4) as usize;
                assert_eq!(out[i + 3], 0, "{x},{y} is past the fold");
            }
        }
    }
    for (x, y) in [(0, 0), (w - 1, 0), (0, w - 1), (w - 1, w - 1)] {
        assert_eq!(out[((y * w + x) * 4 + 3) as usize], 0, "corner {x},{y}");
    }
    // Along the main diagonal outward from the centre, the opaque pixels'
    // source radius (the red ramp grows with the source x) never decreases.
    let mut last = 0_u8;
    for t in w / 2..w {
        let i = ((t * w + t) * 4) as usize;
        if out[i + 3] == 255 {
            assert!(out[i] >= last, "diagonal {t}: {} after {last}", out[i]);
            last = out[i];
        }
    }
    assert!(last > 0, "the diagonal kept some opaque pixels");

    // A strong perspective carries the bottom corners past the fold too.
    let params = json!({"vertical_perspective":-100,"distortion":100});
    let lens = Lens::of(&params);
    let out = filter(w, w, &ramp(w, w), &params);
    for (x, y) in [(0, w - 1), (w - 1, w - 1)] {
        let (nx, ny) = lens.pre_radial(w, w, x, y).unwrap();
        assert!(nx * nx + ny * ny >= threshold, "{x},{y} is past the fold");
        assert_eq!(
            out[((y * w + x) * 4 + 3) as usize],
            0,
            "bottom corner {x},{y}"
        );
    }
    for y in 0..w {
        for x in 0..w {
            let (nx, ny) = lens.pre_radial(w, w, x, y).unwrap();
            if nx * nx + ny * ny >= threshold {
                assert_eq!(out[((y * w + x) * 4 + 3) as usize], 0, "{x},{y}");
            }
        }
    }
}

/// IEC 61966-2-1 sRGB, written here independently of the core.
fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn encode(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    if u <= 0.003_130_8 {
        12.92 * u
    } else {
        1.055 * u.powf(1.0 / 2.4) - 0.055
    }
}

#[test]
fn the_vignette_is_a_light_linear_gain_falling_off_from_the_centre() {
    // Odd sides, so one pixel centre is the plate's centre.
    let (w, h) = (41_u32, 31_u32);
    let grey: Vec<u8> = (0..w * h).flat_map(|_| [128, 128, 128, 255]).collect();
    let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
    let far = (cx - 0.5).hypot(cy - 0.5);
    for amount in [-100.0, 100.0] {
        for midpoint in [0.0, 50.0, 100.0] {
            let params = json!({"vignette_amount":amount,"vignette_midpoint":midpoint});
            let out = filter(w, h, &grey, &params);
            let falloff = 4.0_f64.powf((midpoint - 50.0) / 50.0);
            for y in 0..h {
                for x in 0..w {
                    let fraction = (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy) / far;
                    let stops = amount * 0.02 * fraction.powf(falloff);
                    let expected = encode(decode(128.0 / 255.0) * stops.exp2()) * 255.0;
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..3 {
                        let got = f64::from(out[i + c]);
                        assert!(
                            (got - expected).abs() <= 1.0,
                            "{params} {x},{y}: {got} vs {expected}"
                        );
                    }
                    assert_eq!(out[i + 3], 255);
                }
            }
            let centre = (((h / 2) * w + w / 2) * 4) as usize;
            assert_eq!(out[centre..centre + 4], [128, 128, 128, 255], "{params}");
            let corner = out[0];
            if amount > 0.0 {
                assert!(corner > 128, "{params}: a positive amount brightens");
            } else {
                assert!(corner < 128, "{params}: a negative amount darkens");
            }
        }
    }
}

fn doc16(w: u32, h: u32, pixels: Vec<[u16; 4]>) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}

#[test]
fn byte_and_16_bit_agree_within_a_level_and_coverage_confines_the_result() {
    let (w, h) = (36_u32, 28_u32);
    let src = plate(w, h);
    let wide: Vec<[u16; 4]> = src
        .chunks_exact(4)
        .map(|p| [0, 1, 2, 3].map(|c| u16::from(p[c]) * 257))
        .collect();
    let d = doc16(w, h, wide.clone());
    let inside = |x: u32, y: u32| (5..30).contains(&x) && (4..22).contains(&y);
    let coverage: Vec<u16> = (0..w * h)
        .map(|i| if inside(i % w, i / w) { 65535 } else { 0 })
        .collect();
    for params in [
        json!({"distortion":40,"angle":12}),
        json!({"chromatic_red_cyan":80,"chromatic_blue_yellow":-60,"scale":110}),
        json!({"vignette_amount":-70,"vignette_midpoint":30,"vertical_perspective":50}),
    ] {
        let edit = SelectedEdit::parse(
            &json!({"layer":0,"op":"lens_correction","params":params.clone()}).to_string(),
        )
        .unwrap();
        let out16 = d
            .selected_edit(&edit, &coverage, REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .unwrap_or_else(|| panic!("{params}: no change at 16 bits"))
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .unwrap();
        let out8 = filter(w, h, &src, &params);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if !inside(x, y) {
                    assert_eq!(out16[i], wide[i], "{params} outside {x},{y}");
                    continue;
                }
                for c in 0..4 {
                    if c < 3 && out16[i][3] == 0 {
                        continue;
                    }
                    let got = f64::from(out16[i][c]) / 257.0;
                    assert!(
                        (got - f64::from(out8[i * 4 + c])).abs() <= 1.0,
                        "{params} {x},{y} {c}: {got} vs {}",
                        out8[i * 4 + c]
                    );
                }
            }
        }
    }
}

#[test]
fn parameters_refuse_by_name() {
    let (w, h) = (4_u32, 3_u32);
    let img = image(w, h, &[128; 48]);
    for (params, wanted) in [
        (
            json!({"distortion":101}),
            "distortion must be finite in [-100,100]",
        ),
        (
            json!({"chromatic_red_cyan":-101}),
            "chromatic_red_cyan must be finite in [-100,100]",
        ),
        (
            json!({"chromatic_blue_yellow":200}),
            "chromatic_blue_yellow must be finite in [-100,100]",
        ),
        (
            json!({"vignette_amount":-100.5}),
            "vignette_amount must be finite in [-100,100]",
        ),
        (
            json!({"vignette_midpoint":-1}),
            "vignette_midpoint must be finite in [0,100]",
        ),
        (
            json!({"vertical_perspective":101}),
            "vertical_perspective must be finite in [-100,100]",
        ),
        (
            json!({"horizontal_perspective":-101}),
            "horizontal_perspective must be finite in [-100,100]",
        ),
        (json!({"angle":181}), "angle must be finite in [-180,180]"),
        (json!({"scale":49}), "scale must be finite in [50,150]"),
        (json!({"scale":151}), "scale must be finite in [50,150]"),
        (json!({"distortion":"a"}), "distortion must be numeric"),
        (
            json!({"radius":3}),
            "unexpected lens_correction parameter radius",
        ),
    ] {
        let (out, message) = run(img, &params.to_string(), ptr::null(), 12);
        assert!(out.is_none(), "{params} was accepted");
        assert_eq!(message, wanted, "{params}");
    }
    unsafe { dr_image_free(img) };
}

#[test]
fn null_arguments_are_tolerated() {
    // A NULL image is NULL with no error; NULL params are the defaults, the
    // exact bypass, so NULL with no error too.
    let (out, message) = run(ptr::null(), r#"{"distortion":20}"#, ptr::null(), 0);
    assert!(out.is_none() && message.is_empty(), "{message}");
    let (w, h) = (20_u32, 20_u32);
    let img = image(w, h, &plate(w, h));
    let name = CString::new("lens_correction").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_image_filter_op(img, name.as_ptr(), ptr::null(), ptr::null(), &mut err) };
    assert!(out.is_null() && err.is_null());
    let bytes = unsafe {
        dr_image_filter_op_scratch_bytes(ptr::null(), name.as_ptr(), ptr::null(), ptr::null_mut())
    };
    assert_eq!(bytes, 0);
    unsafe { dr_image_free(img) };
}

#[test]
fn it_reports_progress_and_cancels_mid_run() {
    let (w, h) = (120_u32, 80_u32);
    let n = (w * h) as usize;
    let img = image(w, h, &plate(w, h)) as usize;
    for params in [
        r#"{"distortion":30,"angle":5}"#,
        r#"{"chromatic_red_cyan":50,"vignette_amount":-40,"scale":90}"#,
    ] {
        let (plain, message) = run(img as *const DrImage, params, ptr::null(), n);
        assert!(message.is_empty(), "{params}: {message}");
        assert!(plain.is_some(), "{params}: a bypass");
        let control = unsafe { dr_job_control_create() };
        let (controlled, message) = run(img as *const DrImage, params, control, n);
        unsafe { dr_job_control_free(control) };
        assert!(message.is_empty(), "{params}: {message}");
        assert!(plain == controlled, "{params}: controlled bytes differ");

        let render = |token: usize| {
            let (out, message) = run(
                img as *const DrImage,
                params,
                token as *const DrJobControl,
                n,
            );
            (out.is_some(), message)
        };
        let log = reports(render);
        assert!(
            log.iter().all(|r| (0.0..=1.0).contains(&r.progress)),
            "{params}: a progress outside [0, 1]"
        );
        let last = log.last().expect("a report");
        assert!(
            last.progress == 1.0 && last.stage == 5,
            "{params}: the log ends at ({}, stage {})",
            last.progress,
            last.stage
        );
        let inside: Vec<u64> = log
            .iter()
            .filter(|r| r.stage == 6 && r.progress > 0.0 && r.progress < 1.0)
            .map(|r| r.checkpoints)
            .collect();
        assert!(
            inside.len() >= 2,
            "{params}: fewer than two stage-6 reports inside (0, 1)"
        );
        // `checkpoint` parks before `report` stores its stage, so the second
        // stage-6 report parks with stage 6 already stored by the first.
        let count = inside[1];
        let parked = cancelled_parked_at(count, &|_, stage| stage == 6, render);
        assert_eq!(parked.count, count, "{params}");
    }
    unsafe { dr_image_free(img as *mut DrImage) };
}

#[test]
fn the_vignette_reports_progress_row_by_row() {
    // The vignette is the last of `stages` equal shares of the run: alone it
    // is the whole of it, after the one shared sampling pass of a distortion
    // its second half. It reports (y + 1)/h after each row, so at least h/2
    // stage-6 reports land strictly inside its share.
    let (w, h) = (120_u32, 80_u32);
    let n = (w * h) as usize;
    let img = image(w, h, &plate(w, h)) as usize;
    for (params, low) in [
        (r#"{"vignette_amount":-40}"#, 0.0),
        (r#"{"distortion":30,"vignette_amount":-40}"#, 0.5),
    ] {
        let (plain, message) = run(img as *const DrImage, params, ptr::null(), n);
        assert!(message.is_empty(), "{params}: {message}");
        assert!(plain.is_some(), "{params}: a bypass");
        let mut controlled = None;
        let log = reports(|token: usize| {
            let (out, message) = run(
                img as *const DrImage,
                params,
                token as *const DrJobControl,
                n,
            );
            let ok = out.is_some();
            controlled = out;
            (ok, message)
        });
        assert!(plain == controlled, "{params}: controlled bytes differ");
        assert!(
            log.windows(2)
                .all(|pair| pair[0].progress <= pair[1].progress),
            "{params}: progress went backwards"
        );
        let rows = log
            .iter()
            .filter(|r| r.stage == 6 && r.progress > low && r.progress < 1.0)
            .count();
        assert!(
            rows >= (h / 2) as usize,
            "{params}: {rows} stage-6 reports inside the vignette's ({low}, 1)"
        );
    }
    unsafe { dr_image_free(img as *mut DrImage) };
}
