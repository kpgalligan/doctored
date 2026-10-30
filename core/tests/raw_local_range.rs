//! Luminance and colour ranges of a RAW local against an independent f64 oracle.
//!
//! The range's coverage is read back through `dr_precision_doc_develop_locals`:
//! every variant differs from its base by at least 32768 codes in the red
//! channel and nowhere else, so the blend weight of a pixel is
//! `(out − base) / (variant − base)` in that channel, known to `0.5 / 32768`.
//!
//! The oracle is written here and shares nothing with the core: it reads the
//! profile's `rXYZ`/`gXYZ`/`bXYZ` and TRC tags from `profile.bytes()`, takes
//! D50 = (0.96422, 1, 0.82521), and clamps L* to 0…100 as the law states.
//!
//! Bounds, fixed before the first run: a smooth case (falloff ≥ 0.10 l or
//! ≥ 10 ΔE) agrees within `1e-4 + 0.5/65535`; a sample at least 0.0025 l or
//! 0.25 ΔE from a hard bound is exactly 0 or 1; a product with a radial shape
//! adds `1/65535` for the shape's own quantization. The three clamp cases sit
//! ON a hard bound, so their expectations are constants, not oracle values.
use crate::raw_local_numeric::{
    develop, icc_be32, image, mask_edit, radial_reference, rgba, success, transfer_decode,
    transfer_tag, Mask,
};
use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_precision_color::dr_precision_doc_assign_profile;
use doctored_core::ffi_raw_local::*;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::raw_local_mask::RawLocalMask;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

const SMOOTH_BOUND: f64 = 1e-4 + 0.5 / 65535.0;
const RADIAL_BOUND: f64 = SMOOTH_BOUND + 1.0 / 65535.0;
const HARD_LUMINANCE_MARGIN: f64 = 0.0025;
const HARD_COLOR_MARGIN: f64 = 0.25;
const D50: [f64; 3] = [0.96422, 1.0, 0.82521];

// ---- the oracle -----------------------------------------------------------

fn xyz_tag(bytes: &[u8], signature: &[u8; 4]) -> [f64; 3] {
    for n in 0..icc_be32(bytes, 128) as usize {
        let record = 132 + n * 12;
        if &bytes[record..record + 4] == signature {
            let start = icc_be32(bytes, record + 4) as usize;
            assert_eq!(&bytes[start..start + 4], b"XYZ ");
            return std::array::from_fn(|k| {
                f64::from(icc_be32(bytes, start + 8 + k * 4) as i32) / 65536.0
            });
        }
    }
    panic!("independent ICC fixture lacks a colorant");
}
fn oracle_lab(profile: &IccProfile, rgb: [u16; 3]) -> [f64; 3] {
    let bytes = profile.bytes();
    let mut xyz = [0.0; 3];
    for (c, signature) in [b"rXYZ", b"gXYZ", b"bXYZ"].into_iter().enumerate() {
        let linear = transfer_decode(transfer_tag(bytes, c), f64::from(rgb[c]) / 65535.0);
        let column = xyz_tag(bytes, signature);
        for k in 0..3 {
            xyz[k] += column[k] * linear;
        }
    }
    let f = |t: f64| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(xyz[0] / D50[0]), f(xyz[1] / D50[1]), f(xyz[2] / D50[2]));
    [
        (116.0 * fy - 16.0).clamp(0.0, 100.0),
        500.0 * (fx - fy),
        200.0 * (fy - fz),
    ]
}
fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}
#[derive(Clone, Copy)]
struct Luminance {
    low: f64,
    high: f64,
    low_falloff: f64,
    high_falloff: f64,
    invert: bool,
}
impl Luminance {
    fn json(&self) -> Value {
        json!({"low":self.low,"high":self.high,"low_falloff":self.low_falloff,
               "high_falloff":self.high_falloff,"invert":self.invert})
    }
    fn reference(&self, lab: [f64; 3]) -> f64 {
        let l = lab[0] / 100.0;
        let value = if l < self.low {
            if self.low_falloff == 0.0 {
                0.0
            } else {
                smooth(((l - self.low + self.low_falloff) / self.low_falloff).clamp(0.0, 1.0))
            }
        } else if l > self.high {
            if self.high_falloff == 0.0 {
                0.0
            } else {
                1.0 - smooth(((l - self.high) / self.high_falloff).clamp(0.0, 1.0))
            }
        } else {
            1.0
        };
        if self.invert {
            1.0 - value
        } else {
            value
        }
    }
    /// True when `lab` is far enough from both bounds for a hard range to be
    /// exactly 0 or 1 there.
    fn clear(&self, lab: [f64; 3]) -> bool {
        let l = lab[0] / 100.0;
        (l - self.low).abs() >= HARD_LUMINANCE_MARGIN
            && (l - self.high).abs() >= HARD_LUMINANCE_MARGIN
    }
}
#[derive(Clone, Copy)]
struct Colour {
    lab: [f64; 3],
    amount: f64,
    falloff: f64,
    invert: bool,
}
impl Colour {
    fn json(&self) -> Value {
        json!({"lab":self.lab,"amount":self.amount,"falloff":self.falloff,"invert":self.invert})
    }
    fn distance(&self, lab: [f64; 3]) -> f64 {
        (0..3)
            .map(|c| (lab[c] - self.lab[c]).powi(2))
            .sum::<f64>()
            .sqrt()
    }
    fn reference(&self, lab: [f64; 3]) -> f64 {
        let d = self.distance(lab);
        let value = if d <= self.amount {
            1.0
        } else if self.falloff == 0.0 || d >= self.amount + self.falloff {
            0.0
        } else {
            1.0 - smooth((d - self.amount) / self.falloff)
        };
        if self.invert {
            1.0 - value
        } else {
            value
        }
    }
    fn clear(&self, lab: [f64; 3]) -> bool {
        (self.distance(lab) - self.amount).abs() >= HARD_COLOR_MARGIN
    }
}

// ---- the weight readout ---------------------------------------------------

/// The base with red moved to the far end of the scale: at least 32768 codes
/// away, the other channels and alpha untouched.
fn far_variant(base: &[[u16; 4]]) -> Vec<[u16; 4]> {
    base.iter()
        .map(|p| [if p[0] < 32768 { 65535 } else { 0 }, p[1], p[2], p[3]])
        .collect()
}
struct Reading {
    base: Vec<[u16; 4]>,
    variant: Vec<[u16; 4]>,
    out: Vec<[u16; 4]>,
}
impl Reading {
    fn weight(&self, i: usize) -> f64 {
        (f64::from(self.out[i][0]) - f64::from(self.base[i][0]))
            / (f64::from(self.variant[i][0]) - f64::from(self.base[i][0]))
    }
    /// The oracle says 0 or 1 exactly: the output is the base's pixel or the
    /// variant's, bit for bit.
    fn assert_exact(&self, i: usize, expected: f64, label: &str) {
        let wanted = if expected == 0.0 {
            self.base[i]
        } else {
            assert_eq!(expected, 1.0, "{label} pixel{i}: not a hard expectation");
            self.variant[i]
        };
        assert_eq!(
            self.out[i], wanted,
            "{label} pixel{i}: expected exactly {expected}"
        );
    }
    fn assert_near(&self, i: usize, expected: f64, bound: f64, label: &str) {
        let actual = self.weight(i);
        assert!(
            (actual - expected).abs() <= bound,
            "{label} pixel{i}: weight {actual} expected {expected} bound {bound}"
        );
    }
}
fn read(
    profile: &IccProfile,
    w: u32,
    h: u32,
    base: &[[u16; 4]],
    mask: &Mask,
    setting: Value,
) -> Reading {
    let variant = far_variant(base);
    let base_doc = image(w, h, base, profile);
    let variant_doc = image(w, h, &variant, profile);
    let out = develop(&base_doc, &[&variant_doc], &[mask], json!([setting]), None)
        .map_or_else(|| base.to_vec(), |d| rgba(&d));
    Reading {
        base: base.to_vec(),
        variant,
        out,
    }
}
fn setting(range: Value) -> Value {
    json!({"enabled":true,"density":1,"feather":0,"range":range})
}
fn full(w: u32, h: u32) -> Mask {
    mask_edit(None, w, h, json!({"op":"full"})).unwrap()
}
/// Row 0 is a neutral ramp over the whole scale, row 1 a coloured one, so L*
/// runs from 0 to 100 and also depends on the profile's matrix.
fn ramp(w: u32) -> Vec<[u16; 4]> {
    let v = |i: u32| (u64::from(i) * 65535 / u64::from(w - 1)) as u16;
    (0..w)
        .map(|i| [v(i), v(i), v(i), 65535])
        .chain((0..w).map(|i| [v(i), 65535 - v(i), v(i) / 2 + 9000, 65535]))
        .collect()
}
fn patches() -> Vec<[u16; 4]> {
    let steps = [0_u16, 16384, 32768, 49151, 65535];
    let mut out = Vec::new();
    for r in steps {
        for g in steps {
            for b in steps {
                out.push([r, g, b, 65535]);
            }
        }
    }
    // Neighbours of the centre colour, so the smooth band holds samples.
    for k in 0..25_u16 {
        out.push([40000 + k * 700, 30000 - k * 500, 20000 + k * 300, 65535]);
    }
    out
}
fn rgb(p: [u16; 4]) -> [u16; 3] {
    [p[0], p[1], p[2]]
}
fn try_develop(
    base: &PrecisionDocument,
    variant: &PrecisionDocument,
    mask: &Mask,
    settings: Value,
) -> Result<bool, String> {
    let variants = [variant as *const PrecisionDocument];
    let masks = [mask.0 as *const RawLocalMask];
    let settings = CString::new(settings.to_string()).unwrap();
    let mut error: *mut c_char = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_develop_locals(
            base,
            variants.as_ptr(),
            masks.as_ptr(),
            1,
            settings.as_ptr(),
            ptr::null(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if !error.is_null() {
        assert!(output.is_null());
        let text = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        return Err(text);
    }
    let changed = !output.is_null();
    if changed {
        drop(unsafe { Box::from_raw(output) });
    }
    Ok(changed)
}
fn sample(base: &PrecisionDocument, x: f64, y: f64) -> Result<[f64; 3], String> {
    let mut lab = [-7.0_f64; 3];
    let mut error: *mut c_char = ptr::null_mut();
    let ok = unsafe { dr_precision_doc_raw_range_sample(base, x, y, lab.as_mut_ptr(), &mut error) };
    if ok {
        assert!(error.is_null());
        return Ok(lab);
    }
    assert!(!error.is_null());
    assert_eq!(lab, [-7.0; 3], "a refused sample must not write its output");
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    Err(text)
}

// ---- luminance --------------------------------------------------------------

#[test]
fn hard_luminance_range_is_exactly_zero_or_one_on_a_ramp() {
    let w = 512;
    let base = ramp(w);
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        for invert in [false, true] {
            let range = Luminance {
                low: 0.3,
                high: 0.7,
                low_falloff: 0.0,
                high_falloff: 0.0,
                invert,
            };
            let reading = read(
                &profile,
                w,
                2,
                &base,
                &full(w, 2),
                setting(json!({"luminance":range.json()})),
            );
            let mut seen = [0, 0];
            for (i, p) in base.iter().enumerate() {
                let lab = oracle_lab(&profile, rgb(*p));
                if !range.clear(lab) {
                    continue;
                }
                let expected = range.reference(lab);
                reading.assert_exact(i, expected, "hard luminance");
                seen[expected as usize] += 1;
            }
            assert!(seen[0] > 100 && seen[1] > 100, "{seen:?}");
        }
    }
}
#[test]
fn smooth_luminance_range_matches_the_oracle_on_a_ramp() {
    let w = 512;
    let base = ramp(w);
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        for invert in [false, true] {
            let range = Luminance {
                low: 0.3,
                high: 0.6,
                low_falloff: 0.1,
                high_falloff: 0.25,
                invert,
            };
            let reading = read(
                &profile,
                w,
                2,
                &base,
                &full(w, 2),
                setting(json!({"luminance":range.json()})),
            );
            let mut partial = 0;
            for (i, p) in base.iter().enumerate() {
                let expected = range.reference(oracle_lab(&profile, rgb(*p)));
                reading.assert_near(i, expected, SMOOTH_BOUND, "smooth luminance");
                partial += usize::from(expected > 0.05 && expected < 0.95);
            }
            assert!(partial > 50, "{partial}");
        }
    }
}

// ---- colour -----------------------------------------------------------------

#[test]
fn hard_and_smooth_colour_ranges_match_the_oracle_in_srgb_and_display_p3() {
    let base = patches();
    let w = base.len() as u32;
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        let centre = oracle_lab(&profile, [40000, 30000, 20000]);
        for invert in [false, true] {
            let hard = Colour {
                lab: centre,
                amount: 30.0,
                falloff: 0.0,
                invert,
            };
            let reading = read(
                &profile,
                w,
                1,
                &base,
                &full(w, 1),
                setting(json!({"color":hard.json()})),
            );
            let mut seen = [0, 0];
            for (i, p) in base.iter().enumerate() {
                let lab = oracle_lab(&profile, rgb(*p));
                if !hard.clear(lab) {
                    continue;
                }
                let expected = hard.reference(lab);
                reading.assert_exact(i, expected, "hard colour");
                seen[expected as usize] += 1;
            }
            assert!(seen[0] > 5 && seen[1] > 5, "{seen:?}");

            let soft = Colour {
                lab: centre,
                amount: 15.0,
                falloff: 60.0,
                invert,
            };
            let reading = read(
                &profile,
                w,
                1,
                &base,
                &full(w, 1),
                setting(json!({"color":soft.json()})),
            );
            let mut partial = 0;
            for (i, p) in base.iter().enumerate() {
                let expected = soft.reference(oracle_lab(&profile, rgb(*p)));
                reading.assert_near(i, expected, SMOOTH_BOUND, "smooth colour");
                partial += usize::from(expected > 0.05 && expected < 0.95);
            }
            assert!(partial > 10, "{partial}");
        }
    }
}
#[test]
fn the_same_codes_select_differently_through_srgb_and_display_p3() {
    // The range reads Lab through the base's own profile, so one code triple
    // is a different colour, at a different distance, in each space.
    let p = [52000, 9000, 9000];
    let srgb = oracle_lab(&IccProfile::srgb(), p);
    let p3 = oracle_lab(&IccProfile::display_p3(), p);
    let range = Colour {
        lab: srgb,
        amount: 4.0,
        falloff: 0.0,
        invert: false,
    };
    assert!(range.distance(p3) > 8.0, "{}", range.distance(p3));
    let base = [[p[0], p[1], p[2], 65535]];
    for (profile, expected) in [(IccProfile::srgb(), 1.0), (IccProfile::display_p3(), 0.0)] {
        read(
            &profile,
            1,
            1,
            &base,
            &full(1, 1),
            setting(json!({"color":range.json()})),
        )
        .assert_exact(0, expected, "profile-aware colour");
    }
}
#[test]
fn luminance_and_colour_multiply() {
    let base = patches();
    let w = base.len() as u32;
    let profile = IccProfile::display_p3();
    let luminance = Luminance {
        low: 0.35,
        high: 0.55,
        low_falloff: 0.3,
        high_falloff: 0.4,
        invert: false,
    };
    let colour = Colour {
        lab: oracle_lab(&profile, [40000, 30000, 20000]),
        amount: 20.0,
        falloff: 80.0,
        invert: false,
    };
    let reading = read(
        &profile,
        w,
        1,
        &base,
        &full(w, 1),
        setting(json!({"luminance":luminance.json(),"color":colour.json()})),
    );
    let mut both = 0;
    for (i, p) in base.iter().enumerate() {
        let lab = oracle_lab(&profile, rgb(*p));
        let (a, b) = (luminance.reference(lab), colour.reference(lab));
        reading.assert_near(i, a * b, SMOOTH_BOUND, "luminance times colour");
        both += usize::from(a > 0.05 && a < 0.95 && b > 0.05 && b < 0.95);
    }
    assert!(both > 5, "{both}");
}

// ---- shape × range ----------------------------------------------------------

fn field(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let v = (u64::from(x) * 65535 / u64::from(w - 1)) as u16;
            let t = (u64::from(y) * 20000 / u64::from(h - 1)) as u16;
            [v, v / 2 + t, 65535 - v, 65535]
        })
        .collect()
}
#[test]
fn a_range_multiplies_a_soft_radial_shape() {
    let (w, h) = (96, 40);
    let base = field(w, h);
    let profile = IccProfile::srgb();
    let (center, radius, hardness) = ([47.25, 19.5], [44.0, 18.5], 0.2);
    let mask = mask_edit(
        None,
        w,
        h,
        json!({"op":"radial","center":center,"radius":radius,"hardness":hardness}),
    )
    .unwrap();
    let range = Luminance {
        low: 0.4,
        high: 0.6,
        low_falloff: 0.2,
        high_falloff: 0.2,
        invert: false,
    };
    let reading = read(
        &profile,
        w,
        h,
        &base,
        &mask,
        setting(json!({"luminance":range.json()})),
    );
    let mut both = 0;
    for (i, p) in base.iter().enumerate() {
        let point = [(i as u32 % w) as f64 + 0.5, (i as u32 / w) as f64 + 0.5];
        let shape = radial_reference(point, center, radius, hardness);
        let tone = range.reference(oracle_lab(&profile, rgb(*p)));
        reading.assert_near(i, shape * tone, RADIAL_BOUND, "radial times range");
        if shape == 0.0 {
            assert_eq!(reading.out[i], reading.base[i], "outside the ellipse");
        }
        both += usize::from(shape > 0.05 && shape < 0.95 && tone > 0.05 && tone < 0.95);
    }
    assert!(both > 100, "{both}");
}
#[test]
fn a_range_multiplies_a_shape_inverted_by_the_invert_command() {
    let (w, h) = (64, 24);
    let base = field(w, h);
    let profile = IccProfile::display_p3();
    let (center, radius) = ([20.5, 11.5], [14.0, 9.0]);
    let radial = mask_edit(
        None,
        w,
        h,
        json!({"op":"radial","center":center,"radius":radius,"hardness":1}),
    )
    .unwrap();
    let mask = mask_edit(Some(&radial), w, h, json!({"op":"invert"})).unwrap();
    let range = Luminance {
        low: 0.25,
        high: 0.75,
        low_falloff: 0.0,
        high_falloff: 0.0,
        invert: false,
    };
    let reading = read(
        &profile,
        w,
        h,
        &base,
        &mask,
        setting(json!({"luminance":range.json()})),
    );
    let mut seen = [0, 0];
    for (i, p) in base.iter().enumerate() {
        let point = [(i as u32 % w) as f64 + 0.5, (i as u32 / w) as f64 + 0.5];
        let lab = oracle_lab(&profile, rgb(*p));
        if !range.clear(lab) {
            continue;
        }
        let expected = (1.0 - radial_reference(point, center, radius, 1.0)) * range.reference(lab);
        reading.assert_exact(i, expected, "inverted shape times range");
        seen[expected as usize] += 1;
    }
    assert!(seen[0] > 100 && seen[1] > 100, "{seen:?}");
}
#[test]
fn full_alone_covers_every_pixel_and_repeats_as_a_no_op() {
    let (w, h) = (7, 5);
    let mask = full(w, h);
    let mut count = 0;
    let mut error = ptr::null_mut();
    let pointer = unsafe { dr_raw_local_mask_bytes(mask.0, &mut count, &mut error) };
    success(error);
    let bytes = unsafe { std::slice::from_raw_parts(pointer, count) };
    assert_eq!(count, 16 + 35 * 2);
    assert!(bytes[16..].iter().all(|b| *b == 255));
    assert!(mask_edit(Some(&mask), w, h, json!({"op":"full"})).is_none());
    let base = field(w, h);
    let reading = read(
        &IccProfile::srgb(),
        w,
        h,
        &base,
        &mask,
        json!({"enabled":true,"density":1,"feather":0}),
    );
    assert_eq!(reading.out, reading.variant);
}

// ---- exact bypasses ---------------------------------------------------------

#[test]
fn pixels_a_range_excludes_keep_the_base_sample_bit_for_bit() {
    let profile = IccProfile::display_p3();
    // Hidden colour under alpha 0 and odd codes: nothing may be rewritten.
    let base = vec![
        [12345, 23457, 34569, 0],
        [60001, 60003, 60005, 65535],
        [101, 103, 107, 40001],
        [30001, 31003, 29007, 65535],
    ];
    let range = Luminance {
        low: 0.45,
        high: 0.6,
        low_falloff: 0.0,
        high_falloff: 0.0,
        invert: false,
    };
    let expected: Vec<f64> = base
        .iter()
        .map(|p| {
            let lab = oracle_lab(&profile, rgb(*p));
            assert!(range.clear(lab));
            range.reference(lab)
        })
        .collect();
    assert_eq!(expected, [0.0, 0.0, 0.0, 1.0]);
    let reading = read(
        &profile,
        4,
        1,
        &base,
        &full(4, 1),
        setting(json!({"luminance":range.json()})),
    );
    assert_eq!(reading.out[..3], reading.base[..3]);
    assert_eq!(reading.out[3], reading.variant[3]);

    // A range that excludes every pixel changes nothing: NULL without error.
    let none = Luminance {
        low: 0.999,
        high: 1.0,
        ..range
    };
    let variant = far_variant(&base);
    let base_doc = image(4, 1, &base, &profile);
    let variant_doc = image(4, 1, &variant, &profile);
    assert_eq!(
        try_develop(
            &base_doc,
            &variant_doc,
            &full(4, 1),
            json!([setting(json!({"luminance":none.json()}))])
        ),
        Ok(false)
    );
    // A disabled local and density 0 stay exact with a range present.
    for (enabled, density) in [(false, 1.0), (true, 0.0)] {
        assert_eq!(
            try_develop(
                &base_doc,
                &variant_doc,
                &full(4, 1),
                json!([{"enabled":enabled,"density":density,"feather":0,
                        "range":{"luminance":range.json()}}])
            ),
            Ok(false)
        );
    }
    assert_eq!(rgba(&base_doc), base);
}

// ---- the clamp --------------------------------------------------------------

/// What the far variant of Display P3 white and of black must come out as
/// under `full` and a hard 0…1 luminance range: the variant itself.
const WHITE_OUT: [u16; 4] = [0, 65535, 65535, 65535];
const BLACK_OUT: [u16; 4] = [65535, 0, 0, 65535];

#[test]
fn display_p3_white_and_black_are_inside_a_hard_zero_to_one_luminance_range() {
    let profile = IccProfile::display_p3();
    let range =
        json!({"luminance":{"low":0,"high":1,"low_falloff":0,"high_falloff":0,"invert":false}});
    for (code, expected) in [(65535, WHITE_OUT), (0, BLACK_OUT)] {
        let base = [[code, code, code, 65535]];
        let reading = read(&profile, 1, 1, &base, &full(1, 1), setting(range.clone()));
        assert_eq!(reading.out, [expected], "code {code}");
    }
}
#[test]
fn a_colour_sampled_on_display_p3_white_validates_and_develops() {
    let profile = IccProfile::display_p3();
    let base = vec![[65535; 4]; 9];
    let base_doc = image(3, 3, &base, &profile);
    let lab = sample(&base_doc, 1.5, 1.5).unwrap();
    assert!(lab[0] <= 100.0 && lab[0] >= 99.999, "{lab:?}");
    let reading = read(
        &profile,
        3,
        3,
        &base,
        &full(3, 3),
        setting(json!({"color":{"lab":lab,"amount":0.5,"falloff":0,"invert":false}})),
    );
    assert_eq!(reading.out, vec![WHITE_OUT; 9]);
}

// ---- the sample ---------------------------------------------------------------

#[test]
fn raw_range_sample_is_the_window_mean_of_the_oracle() {
    let (w, h) = (9_u32, 7_u32);
    let base: Vec<[u16; 4]> = (0..w * h)
        .map(|i| {
            [
                (997 + i * 977 % 64000) as u16,
                (2199 + i * 1237 % 63000) as u16,
                (317 + i * 599 % 65000) as u16,
                65535,
            ]
        })
        .collect();
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        let document = image(w, h, &base, &profile);
        for (x, y) in [
            (0.0, 0.0),
            (9.0, 7.0),
            (4.5, 3.5),
            (8.99, 0.01),
            (1.2, 5.9),
            (2.0, 2.0),
        ] {
            let (cx, cy) = ((x as i64).min(8), (y as i64).min(6));
            let mut sum = [0.0; 3];
            let mut count = 0.0;
            for py in (cy - 2).max(0)..=(cy + 2).min(6) {
                for px in (cx - 2).max(0)..=(cx + 2).min(8) {
                    let lab = oracle_lab(&profile, rgb(base[(py * 9 + px) as usize]));
                    for k in 0..3 {
                        sum[k] += lab[k];
                    }
                    count += 1.0;
                }
            }
            let actual = sample(&document, x, y).unwrap();
            for k in 0..3 {
                assert!(
                    (actual[k] - sum[k] / count).abs() <= 1e-3,
                    "sample ({x},{y}) component{k}: {} expected {}",
                    actual[k],
                    sum[k] / count
                );
            }
        }
    }
}

// ---- refusals -----------------------------------------------------------------

#[test]
fn invalid_ranges_and_off_frame_samples_refuse() {
    let profile = IccProfile::srgb();
    let base = field(8, 4);
    let variant = far_variant(&base);
    let base_doc = image(8, 4, &base, &profile);
    let variant_doc = image(8, 4, &variant, &profile);
    let mask = full(8, 4);
    let luminance = |low: f64, high: f64, low_falloff: f64| {
        json!({"luminance":{"low":low,"high":high,"low_falloff":low_falloff,
                            "high_falloff":0,"invert":false}})
    };
    let colour = |lab: [f64; 3], amount: f64, falloff: f64| json!({"color":{"lab":lab,"amount":amount,"falloff":falloff,"invert":false}});
    for range in [
        luminance(0.7, 0.3, 0.0),
        luminance(-0.1, 0.3, 0.0),
        luminance(0.2, 1.1, 0.0),
        luminance(0.2, 0.3, 1.5),
        colour([101.0, 0.0, 0.0], 10.0, 0.0),
        colour([50.0, 257.0, 0.0], 10.0, 0.0),
        colour([50.0, 0.0, 0.0], 201.0, 0.0),
        colour([50.0, 0.0, 0.0], 10.0, -1.0),
        json!({}),
    ] {
        assert_eq!(
            try_develop(
                &base_doc,
                &variant_doc,
                &mask,
                json!([setting(range.clone())])
            ),
            Err("invalid local range bounds, falloff, amount or colour".into()),
            "{range}"
        );
    }
    // Unknown and missing fields are parse refusals, not silent defaults.
    for range in [
        json!({"luminance":{"low":0,"high":1,"low_falloff":0,"high_falloff":0,"invert":false,"x":1}}),
        json!({"luminance":{"low":0,"high":1}}),
        json!({"color":{"lab":[50,0,0],"amount":1,"falloff":0,"invert":false},"extra":true}),
    ] {
        let error = try_develop(&base_doc, &variant_doc, &mask, json!([setting(range)]))
            .expect_err("a malformed range was accepted");
        assert!(error.contains("invalid local blend settings"), "{error}");
    }
    for (x, y) in [
        (-0.1, 1.0),
        (8.1, 1.0),
        (1.0, -0.1),
        (1.0, 4.1),
        (f64::NAN, 1.0),
        (1.0, f64::INFINITY),
    ] {
        let error = sample(&base_doc, x, y).expect_err("an off-frame sample was accepted");
        assert!(error.contains("outside the source"), "{error}");
    }
    assert_eq!(rgba(&base_doc), base);
}

/// The refusal a range gives a base whose profile has no matrix/TRC model,
/// written out here so a reworded message fails the test.
const NO_MODEL: &str =
    "calibrated Exposure and White Balance require an RGB matrix/TRC document profile";

/// A valid RGB profile with no transform tags (sRGB's header and a zero tag
/// count): kept as the base's interpretation, but with no model to reach Lab.
fn unmodelled() -> IccProfile {
    let mut bytes = IccProfile::srgb().bytes()[..128].to_vec();
    bytes.extend_from_slice(&0u32.to_be_bytes());
    bytes[..4].copy_from_slice(&132u32.to_be_bytes());
    let profile = IccProfile::parse(&bytes).unwrap();
    assert!(profile.model().is_none(), "the profile has no model");
    profile
}
/// `pixels` as a document on `profile`, made the way a document comes to hold
/// a profile with no model: `dr_precision_doc_from_rgba16` admits only a
/// matrix/TRC profile, so the samples enter as sRGB and are relabelled by the
/// public assign, which keeps them bit for bit.
fn assigned(w: u32, h: u32, pixels: &[[u16; 4]], profile: &IccProfile) -> Box<PrecisionDocument> {
    let source = image(w, h, pixels, &IccProfile::srgb());
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_assign_profile(
            &*source,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    assert!(!output.is_null());
    let output = unsafe { Box::from_raw(output) };
    assert_eq!(rgba(&output), pixels);
    output
}
#[test]
fn a_range_on_a_profile_with_no_matrix_trc_model_refuses_and_leaves_the_base() {
    let profile = unmodelled();
    let (w, h) = (8, 4);
    let base = field(w, h);
    let variant = far_variant(&base);
    let base_doc = assigned(w, h, &base, &profile);
    let variant_doc = assigned(w, h, &variant, &profile);
    let mask = full(w, h);
    let range =
        json!({"luminance":{"low":0.2,"high":0.8,"low_falloff":0,"high_falloff":0,"invert":false}});
    assert_eq!(
        try_develop(
            &base_doc,
            &variant_doc,
            &mask,
            json!([setting(range.clone())])
        ),
        Err(NO_MODEL.into())
    );
    // The refusal is the range's alone: the same local without one develops
    // on this profile, and a disabled local's range is never evaluated.
    assert_eq!(
        try_develop(
            &base_doc,
            &variant_doc,
            &mask,
            json!([{"enabled":true,"density":1,"feather":0}])
        ),
        Ok(true)
    );
    assert_eq!(
        try_develop(
            &base_doc,
            &variant_doc,
            &mask,
            json!([{"enabled":false,"density":1,"feather":0,"range":range.clone()}])
        ),
        Ok(false)
    );
    assert_eq!(sample(&base_doc, 1.5, 1.5), Err(NO_MODEL.into()));
    assert_eq!(
        preview(&mask, Some(&*base_doc), Some(&range), (w, h), 1, 1.0),
        Err(NO_MODEL.into())
    );
    assert_eq!(rgba(&base_doc), base);
}

// ---- the overlay ----------------------------------------------------------------

fn preview(
    mask: &Mask,
    base: Option<&PrecisionDocument>,
    range: Option<&Value>,
    size: (u32, u32),
    divisor: u32,
    density: f64,
) -> Result<(u32, u32, Vec<u8>), String> {
    let range = range.map(|v| CString::new(v.to_string()).unwrap());
    let mut error: *mut c_char = ptr::null_mut();
    let output = unsafe {
        dr_raw_local_mask_preview(
            mask.0,
            base.map_or(ptr::null(), |b| b),
            range.as_ref().map_or(ptr::null(), |r| r.as_ptr()),
            0,
            0,
            size.0,
            size.1,
            divisor,
            density,
            0.0,
            true,
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    if output.is_null() {
        assert!(!error.is_null());
        let text = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        return Err(text);
    }
    assert!(error.is_null());
    let (w, h) = unsafe { (dr_image_width(output), dr_image_height(output)) };
    let pixels =
        unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(output), (w * h * 4) as usize) }
            .to_vec();
    unsafe { dr_image_free(output) };
    Ok((w, h, pixels))
}
#[test]
fn preview_alpha_is_shape_times_range_and_base_and_range_come_together() {
    let (w, h) = (96_u32, 40_u32);
    let base = field(w, h);
    let profile = IccProfile::display_p3();
    let document = image(w, h, &base, &profile);
    let (center, radius, hardness) = ([47.25, 19.5], [44.0, 18.5], 0.2);
    let mask = mask_edit(
        None,
        w,
        h,
        json!({"op":"radial","center":center,"radius":radius,"hardness":hardness}),
    )
    .unwrap();
    let range = Luminance {
        low: 0.4,
        high: 0.6,
        low_falloff: 0.2,
        high_falloff: 0.2,
        invert: false,
    };
    let range_json = json!({"luminance":range.json()});
    let density = 0.75;
    for divisor in [1_u32, 2] {
        let (pw, ph, pixels) = preview(
            &mask,
            Some(&*document),
            Some(&range_json),
            (w, h),
            divisor,
            density,
        )
        .unwrap();
        assert_eq!((pw, ph), (w.div_ceil(divisor), h.div_ceil(divisor)));
        let mut partial = 0;
        for y in 0..ph {
            for x in 0..pw {
                let sx = (x * divisor + divisor / 2).min(w - 1);
                let sy = (y * divisor + divisor / 2).min(h - 1);
                let shape =
                    radial_reference([sx as f64 + 0.5, sy as f64 + 0.5], center, radius, hardness);
                let tone = range.reference(oracle_lab(&profile, rgb(base[(sy * w + sx) as usize])));
                let expected = shape * density * tone * 255.0;
                let pixel = &pixels[((y * pw + x) * 4) as usize..][..4];
                assert_eq!(pixel[..3], [255, 0, 0]);
                assert!(
                    (f64::from(pixel[3]) - expected).abs() <= 1.0,
                    "preview ({x},{y}) divisor{divisor}: alpha {} expected {expected}",
                    pixel[3]
                );
                partial += usize::from(tone > 0.05 && tone < 0.95 && shape > 0.05);
            }
        }
        assert!(partial > 50, "{partial}");
    }
    // No range: the shape alone, as before.
    let (_, _, plain) = preview(&mask, None, None, (w, h), 1, density).unwrap();
    let centre = ((19 * w + 47) * 4 + 3) as usize;
    assert_eq!(plain[centre], (density * 255.0_f64).round() as u8);

    for (base, range) in [(Some(&*document), None), (None, Some(&range_json))] {
        let error = preview(&mask, base, range, (w, h), 1, density)
            .expect_err("a base or a range alone was accepted");
        assert!(error.contains("together"), "{error}");
    }
    let invalid =
        json!({"luminance":{"low":0.7,"high":0.3,"low_falloff":0,"high_falloff":0,"invert":false}});
    assert_eq!(
        preview(&mask, Some(&*document), Some(&invalid), (w, h), 1, density),
        Err("invalid local range bounds, falloff, amount or colour".into())
    );
    let small = image(8, 4, &field(8, 4), &profile);
    assert!(preview(&mask, Some(&*small), Some(&range_json), (w, h), 1, density).is_err());
}
