//! Independent f64 oracles for the frozen native RAW-local contracts.
//! All production evaluation enters public FFI; reference math uses only the
//! declared pixel-center laws and independently parsed ICC fixture bytes.
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_input::dr_precision_doc_from_rgba16;
use doctored_core::ffi_raw_local::*;
use doctored_core::icc::IccProfile;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::raw_local_mask::RawLocalMask;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

pub(crate) fn success(error: *mut c_char) {
    if !error.is_null() {
        let text = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { dr_string_free(error) };
        panic!("public FFI refused independent fixture: {text}");
    }
}
pub(crate) struct Mask(pub(crate) *mut RawLocalMask);
impl Drop for Mask {
    fn drop(&mut self) {
        unsafe { dr_raw_local_mask_free(self.0) };
    }
}
pub(crate) fn mask_edit(prior: Option<&Mask>, w: u32, h: u32, command: Value) -> Option<Mask> {
    let command = CString::new(command.to_string()).unwrap();
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_raw_local_mask_edit(
            prior.map_or(ptr::null(), |m| m.0),
            w,
            h,
            command.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    (!output.is_null()).then_some(Mask(output))
}
fn samples(mask: &Mask, w: u32, h: u32) -> Vec<u16> {
    let mut count = 0;
    let mut error = ptr::null_mut();
    let pointer = unsafe { dr_raw_local_mask_bytes(mask.0, &mut count, &mut error) };
    success(error);
    assert!(!pointer.is_null());
    let bytes = unsafe { std::slice::from_raw_parts(pointer, count) };
    assert_eq!(bytes.len(), 16 + w as usize * h as usize * 2);
    assert_eq!(&bytes[..4], b"RZLM");
    for (index, expected) in [1, w, h].into_iter().enumerate() {
        assert_eq!(
            u32::from_le_bytes(bytes[4 + index * 4..8 + index * 4].try_into().unwrap()),
            expected
        );
    }
    bytes[16..]
        .chunks_exact(2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .collect()
}
fn mask_from(w: u32, h: u32, values: &[u16]) -> Mask {
    assert_eq!(values.len(), w as usize * h as usize);
    let mut bytes = b"RZLM".to_vec();
    for value in [1, w, h] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut error = ptr::null_mut();
    let pointer = unsafe {
        dr_raw_local_mask_from_bytes(
            bytes.as_ptr(),
            bytes.len(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    assert!(!pointer.is_null());
    Mask(pointer)
}
fn assert_codes(actual: &[u16], expected: &[u16], bound: u16, label: &str) {
    assert_eq!(actual.len(), expected.len());
    for (index, (&a, &b)) in actual.iter().zip(expected).enumerate() {
        assert!(
            a.abs_diff(b) <= bound,
            "{label} component{index}: actual{a} expected{b} bound{bound}"
        );
    }
}
fn plane(w: u32, h: u32, evaluate: impl Fn([f64; 2]) -> f64) -> Vec<u16> {
    (0..w as usize * h as usize)
        .map(|i| {
            code(evaluate([
                (i % w as usize) as f64 + 0.5,
                (i / w as usize) as f64 + 0.5,
            ]))
        })
        .collect()
}
pub(crate) fn image(
    w: u32,
    h: u32,
    pixels: &[[u16; 4]],
    profile: &IccProfile,
) -> Box<PrecisionDocument> {
    assert_eq!(pixels.len(), w as usize * h as usize);
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_from_rgba16(
            pixels.as_ptr().cast(),
            pixels.len() * 4,
            w,
            h,
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            ptr::null(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    assert!(!output.is_null());
    unsafe { Box::from_raw(output) }
}
pub(crate) fn develop(
    base: &PrecisionDocument,
    variants: &[&PrecisionDocument],
    masks: &[&Mask],
    settings: Value,
    vignette: Option<Value>,
) -> Option<Box<PrecisionDocument>> {
    assert_eq!(variants.len(), masks.len());
    let variants: Vec<_> = variants
        .iter()
        .map(|d| *d as *const PrecisionDocument)
        .collect();
    let masks: Vec<_> = masks.iter().map(|m| m.0 as *const RawLocalMask).collect();
    let settings = CString::new(settings.to_string()).unwrap();
    let vignette = vignette.map(|v| CString::new(v.to_string()).unwrap());
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_precision_doc_develop_locals(
            base,
            variants.as_ptr(),
            masks.as_ptr(),
            variants.len(),
            settings.as_ptr(),
            vignette.as_ref().map_or(ptr::null(), |v| v.as_ptr()),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    success(error);
    (!output.is_null()).then(|| unsafe { Box::from_raw(output) })
}
pub(crate) fn rgba(document: &PrecisionDocument) -> Vec<[u16; 4]> {
    document
        .layer(0)
        .unwrap()
        .source()
        .unwrap()
        .to_rgba16()
        .unwrap()
}

fn code(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}
pub(crate) fn radial_reference(p: [f64; 2], center: [f64; 2], radius: [f64; 2], hard: f64) -> f64 {
    let x = (p[0] - center[0]) / radius[0];
    let y = (p[1] - center[1]) / radius[1];
    let distance = (x * x + y * y).sqrt();
    if distance <= hard {
        1.0
    } else if distance >= 1.0 {
        0.0
    } else {
        let t = (distance - hard) / (1.0 - hard);
        1.0 - t * t * (3.0 - 2.0 * t)
    }
}
fn linear_reference(p: [f64; 2], start: [f64; 2], end: [f64; 2]) -> f64 {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    (((p[0] - start[0]) * dx + (p[1] - start[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0)
}
#[derive(Clone, Copy)]
struct RefDab {
    center: [f64; 2],
    radius: f64,
    hardness: f64,
    opacity: f64,
}
fn brush_reference(prior: &[u16], width: usize, dabs: &[RefDab], add: bool) -> Vec<u16> {
    prior
        .iter()
        .enumerate()
        .map(|(i, &sample)| {
            let p = [(i % width) as f64 + 0.5, (i / width) as f64 + 0.5];
            let mut value = f64::from(sample) / 65535.0;
            for dab in dabs {
                let weight =
                    radial_reference(p, dab.center, [dab.radius; 2], dab.hardness) * dab.opacity;
                value = if add {
                    1.0 - (1.0 - value) * (1.0 - weight)
                } else {
                    value * (1.0 - weight)
                };
            }
            code(value)
        })
        .collect()
}
fn vignette_stops(
    p: [f64; 2],
    size: [usize; 2],
    center: [f64; 2],
    stops: f64,
    falloff: f64,
) -> f64 {
    let distance = |q: [f64; 2]| ((q[0] - center[0]).powi(2) + (q[1] - center[1]).powi(2)).sqrt();
    let mut farthest: f64 = 0.0;
    for x in [0.5, size[0] as f64 - 0.5] {
        for y in [0.5, size[1] as f64 - 0.5] {
            farthest = farthest.max(distance([x, y]));
        }
    }
    if stops == 0.0 || distance(p) == 0.0 {
        0.0
    } else {
        stops * (distance(p) / farthest).powf(falloff)
    }
}
pub(crate) fn icc_be32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
pub(crate) fn transfer_tag(bytes: &[u8], channel: usize) -> &[u8] {
    let signature = [b"rTRC", b"gTRC", b"bTRC"][channel];
    for n in 0..icc_be32(bytes, 128) as usize {
        let record = 132 + n * 12;
        if &bytes[record..record + 4] == signature {
            let start = icc_be32(bytes, record + 4) as usize;
            let length = icc_be32(bytes, record + 8) as usize;
            return &bytes[start..start + length];
        }
    }
    panic!("independent ICC fixture lacks TRC");
}
pub(crate) fn transfer_decode(tag: &[u8], encoded: f64) -> f64 {
    let x = encoded.clamp(0.0, 1.0);
    if &tag[..4] == b"curv" {
        let n = icc_be32(tag, 8) as usize;
        if n == 0 {
            return x;
        }
        if n == 1 {
            return x.powf(f64::from(u16::from_be_bytes([tag[12], tag[13]])) / 256.0);
        }
        let position = x * (n - 1) as f64;
        let lo = position.floor() as usize;
        let hi = (lo + 1).min(n - 1);
        let sample =
            |i: usize| f64::from(u16::from_be_bytes([tag[12 + i * 2], tag[13 + i * 2]])) / 65535.0;
        return sample(lo) + (sample(hi) - sample(lo)) * (position - lo as f64);
    }
    assert_eq!(&tag[..4], b"para");
    let kind = u16::from_be_bytes([tag[8], tag[9]]);
    let parameter = |i: usize| f64::from(icc_be32(tag, 12 + i * 4) as i32) / 65536.0;
    let gamma = parameter(0);
    if kind == 0 {
        return x.powf(gamma);
    }
    let a = parameter(1);
    let b = parameter(2);
    match kind {
        1 => {
            if x >= -b / a {
                (a * x + b).powf(gamma)
            } else {
                0.0
            }
        }
        2 => {
            if x >= -b / a {
                (a * x + b).powf(gamma) + parameter(3)
            } else {
                parameter(3)
            }
        }
        3 => {
            if x >= parameter(4) {
                (a * x + b).powf(gamma)
            } else {
                parameter(3) * x
            }
        }
        4 => {
            if x >= parameter(4) {
                (a * x + b).powf(gamma) + parameter(5)
            } else {
                parameter(3) * x + parameter(6)
            }
        }
        _ => panic!("unsupported independent fixture TRC"),
    }
}
fn transfer_encode(tag: &[u8], linear: f64) -> u16 {
    let target = linear.clamp(0.0, 1.0);
    if target <= transfer_decode(tag, 0.0) {
        return 0;
    }
    if target >= transfer_decode(tag, 1.0) {
        return 65535;
    }
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..64 {
        let middle = (low + high) * 0.5;
        if transfer_decode(tag, middle) < target {
            low = middle;
        } else {
            high = middle;
        }
    }
    code((low + high) * 0.5)
}

#[test]
fn fractional_radial_masks_match_f64_and_preserve_sub_byte_levels() {
    let (w, h) = (81, 63);
    for hardness in [0.0, 0.23125, 0.8125, 1.0] {
        let center = [39.125, 28.375];
        let radius = [37.625, 25.875];
        let mask = mask_edit(
            None,
            w,
            h,
            json!({"op":"radial","center":center,"radius":radius,"hardness":hardness}),
        )
        .unwrap();
        let actual = samples(&mask, w, h);
        let expected = plane(w, h, |p| radial_reference(p, center, radius, hardness));
        assert_codes(&actual, &expected, 1, "radial");
        if hardness == 0.0 {
            assert!(actual.iter().copied().collect::<HashSet<_>>().len() > 1000);
        }
    }
}
#[test]
fn hard_radial_pixel_center_boundary_is_exact() {
    let mask = mask_edit(
        None,
        9,
        7,
        json!({"op":"radial","center":[4.5,3.5],"radius":[3,2],"hardness":1}),
    )
    .unwrap();
    let expected = plane(9, 7, |p| radial_reference(p, [4.5, 3.5], [3.0, 2.0], 1.0));
    assert_eq!(samples(&mask, 9, 7), expected);
    assert_eq!(expected[3 * 9 + 7], 65535);
    assert_eq!(expected[3 * 9 + 8], 0);
}
#[test]
fn linear_reversed_and_diagonal_native_ramps_match_f64() {
    let (w, h) = (1025, 7);
    for (from, to) in [
        ([0.5, 0.5], [1024.5, 0.5]),
        ([1024.5, 0.5], [0.5, 0.5]),
        ([131.25, 0.125], [899.875, 6.5]),
    ] {
        let mask = mask_edit(None, w, h, json!({"op":"linear","from":from,"to":to})).unwrap();
        let actual = samples(&mask, w, h);
        assert_codes(
            &actual,
            &plane(w, h, |p| linear_reference(p, from, to)),
            1,
            "linear",
        );
        assert!(actual.iter().copied().collect::<HashSet<_>>().len() > 1000);
    }
}
#[test]
fn ordered_add_subtract_brush_matches_f64_once_per_command() {
    let (w, h) = (23, 17);
    let prior = mask_edit(
        None,
        w,
        h,
        json!({"op":"linear","from":[1.25,2.5],"to":[20.875,14.25]}),
    )
    .unwrap();
    let before = samples(&prior, w, h);
    let dabs = [
        RefDab {
            center: [7.125, 5.25],
            radius: 6.375,
            hardness: 0.2,
            opacity: 0.37125,
        },
        RefDab {
            center: [12.375, 8.125],
            radius: 7.875,
            hardness: 0.75,
            opacity: 0.823125,
        },
        RefDab {
            center: [9.625, 7.375],
            radius: 5.125,
            hardness: 0.0,
            opacity: 0.212345,
        },
    ];
    let encoded: Vec<_> = dabs.iter().map(|d| json!({"center":d.center,"radius":d.radius,"hardness":d.hardness,"opacity":d.opacity})).collect();
    for (mode, add) in [("add", true), ("subtract", false)] {
        let mask = mask_edit(
            Some(&prior),
            w,
            h,
            json!({"op":"brush","mode":mode,"dabs":encoded}),
        )
        .unwrap();
        assert_codes(
            &samples(&mask, w, h),
            &brush_reference(&before, w as usize, &dabs, add),
            1,
            mode,
        );
        assert_eq!(
            samples(&prior, w, h),
            before,
            "brush must not mutate its prior"
        );
    }
}
#[test]
fn one_code_brush_and_inversion_are_exact() {
    let empty = mask_edit(None, 5, 3, json!({"op":"empty"})).unwrap();
    let mask = mask_edit(Some(&empty), 5, 3, json!({"op":"brush","mode":"add","dabs":[{"center":[2.5,1.5],"radius":8,"hardness":1,"opacity":1.0/65535.0}]})).unwrap();
    assert_eq!(samples(&mask, 5, 3), vec![1; 15]);
    let inverted = mask_edit(Some(&mask), 5, 3, json!({"op":"invert"})).unwrap();
    assert_eq!(samples(&inverted, 5, 3), vec![65534; 15]);
    let restored = mask_edit(Some(&inverted), 5, 3, json!({"op":"invert"})).unwrap();
    assert_eq!(samples(&restored, 5, 3), vec![1; 15]);
}
#[test]
fn native_mask_roundtrip_keeps_every_code_and_noop_is_absent() {
    let values: Vec<u16> = (0..=65535).collect();
    let mask = mask_from(256, 256, &values);
    assert_eq!(samples(&mask, 256, 256), values);
    let zero = mask_edit(None, 3, 2, json!({"op":"empty"})).unwrap();
    assert!(mask_edit(Some(&zero), 3, 2, json!({"op":"empty"})).is_none());
    assert!(mask_edit(Some(&mask), 256, 256, json!({"op":"brush","mode":"add","dabs":[{"center":[3.5,4.5],"radius":2,"hardness":1,"opacity":0}]})).is_none());
}
fn mixture_reference(base: [u16; 4], layers: &[([u16; 4], f64)]) -> [f64; 4] {
    let mut current = base.map(|v| f64::from(v) / 65535.0);
    for &(pixel, weight) in layers {
        if weight == 0.0 {
            continue;
        }
        let source = pixel.map(|v| f64::from(v) / 65535.0);
        let alpha = current[3] * (1.0 - weight) + source[3] * weight;
        let mut next = [0.0; 4];
        next[3] = alpha;
        for c in 0..3 {
            next[c] = if alpha > 0.0 {
                (current[c] * current[3] * (1.0 - weight) + source[c] * source[3] * weight) / alpha
            } else {
                0.0
            };
        }
        current = next;
    }
    current
}
#[test]
fn ordered_premultiplied_blend_matches_independent_f64() {
    let profile = IccProfile::srgb();
    let n = 12;
    let base: Vec<_> = (0..n)
        .map(|i| {
            [
                1000 + i * 71,
                20000 + i * 113,
                50000 - i * 37,
                if i == 0 { 0 } else { 16000 + i * 997 },
            ]
        })
        .collect();
    let one: Vec<_> = (0..n)
        .map(|i| {
            [
                55000 - i * 313,
                5000 + i * 43,
                7000 + i * 173,
                60000 - i * 173,
            ]
        })
        .collect();
    let two: Vec<_> = (0..n)
        .map(|i| {
            [
                7000 + i * 163,
                48000 - i * 127,
                17000 + i * 191,
                9000 + i * 401,
            ]
        })
        .collect();
    let a = [
        0, 1, 327, 16000, 32768, 65534, 65535, 1701, 56789, 128, 4096, 60000,
    ];
    let b = [
        0, 16384, 65535, 1, 65534, 4096, 32000, 59000, 0, 400, 33000, 60000,
    ];
    let base_doc = image(4, 3, &base, &profile);
    let first = image(4, 3, &one, &profile);
    let second = image(4, 3, &two, &profile);
    let ma = mask_from(4, 3, &a);
    let mb = mask_from(4, 3, &b);
    let out=develop(&base_doc,&[&first,&second],&[&ma,&mb],json!([{"enabled":true,"density":0.73125,"feather":0},{"enabled":true,"density":0.3125,"feather":0}]),None).unwrap();
    let expected: Vec<_> = (0..n as usize)
        .map(|i| {
            mixture_reference(
                base[i],
                &[
                    (one[i], f64::from(a[i]) / 65535.0 * 0.73125),
                    (two[i], f64::from(b[i]) / 65535.0 * 0.3125),
                ],
            )
        })
        .collect();
    let actual = rgba(&out);
    for (index, (&actual, &expected)) in actual
        .as_flattened()
        .iter()
        .zip(expected.as_flattened())
        .enumerate()
    {
        let error = (f64::from(actual) / 65535.0 - expected).abs();
        assert!(
            error <= 2e-6 + 0.5 / 65535.0,
            "ordered blend component{index}: actual{actual} expected{expected} error{error}"
        );
    }
    assert_eq!(
        actual[0], base[0],
        "outside mask preserves even hidden RGB exactly"
    );
    assert_eq!(out.profile().bytes(), profile.bytes());
}
#[test]
fn disabled_zero_density_and_identical_variants_are_exact_noops() {
    let profile = IccProfile::display_p3();
    let pixels = vec![[17, 34567, 61999, 0], [12345, 33333, 59999, 65535]];
    let base = image(2, 1, &pixels, &profile);
    let other = image(2, 1, &[[60000, 1000, 4000, 65535]; 2], &profile);
    let full = mask_from(2, 1, &[65535; 2]);
    let zero = mask_from(2, 1, &[0; 2]);
    for (variant, mask, enabled, density) in [
        (&*other, &full, false, 1.0),
        (&*other, &full, true, 0.0),
        (&*other, &zero, true, 1.0),
        (&*base, &full, true, 0.37125),
    ] {
        assert!(develop(
            &base,
            &[variant],
            &[mask],
            json!([{"enabled":enabled,"density":density,"feather":0}]),
            None
        )
        .is_none());
    }
    assert_eq!(rgba(&base), pixels);
}
#[test]
fn four_sub_code_locals_accumulate_before_final_quantization() {
    let profile = IccProfile::srgb();
    let base = image(1, 1, &[[0, 0, 0, 65535]], &profile);
    let white = image(1, 1, &[[65535; 4]], &profile);
    let mask = mask_from(1, 1, &[1]);
    let settings = Value::Array(
        (0..4)
            .map(|_| json!({"enabled":true,"density":0.3,"feather":0}))
            .collect(),
    );
    let out = develop(
        &base,
        &[&white, &white, &white, &white],
        &[&mask, &mask, &mask, &mask],
        settings,
        None,
    )
    .unwrap();
    assert_eq!(
        rgba(&out),
        vec![[1, 1, 1, 65535]],
        "intermediate native rounding must not erase four sub-code edits"
    );
}
#[test]
fn feather_one_uses_transparent_extension_and_continuous_density() {
    let profile = IccProfile::srgb();
    let base = image(5, 5, &[[0, 0, 0, 65535]; 25], &profile);
    let white = image(5, 5, &[[65535; 4]; 25], &profile);
    let mut impulse = vec![0; 25];
    impulse[0] = 65535;
    let mask = mask_from(5, 5, &impulse);
    let out = develop(
        &base,
        &[&white],
        &[&mask],
        json!([{"enabled":true,"density":0.713,"feather":1}]),
        None,
    )
    .unwrap();
    let side = (-0.5_f64 / 0.8_f64.powi(2)).exp();
    let kernel = [side, 1.0, side];
    let sum = kernel.iter().sum::<f64>();
    let expected: Vec<_> = (0..25)
        .map(|i| {
            let x = i % 5;
            let y = i / 5;
            let weight = if x <= 1 && y <= 1 {
                kernel[x + 1] * kernel[y + 1] / sum.powi(2) * 0.713
            } else {
                0.0
            };
            [code(weight), code(weight), code(weight), 65535]
        })
        .collect();
    assert_codes(
        rgba(&out).as_flattened(),
        expected.as_flattened(),
        1,
        "transparent feather",
    );
}
#[test]
fn vignette_srgb_p3_profiles_centers_ranges_match_f64() {
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        for (w, h, center) in [(9, 5, [4.5, 2.5]), (4, 11, [0.0, 11.0]), (1, 1, [0.0, 0.0])] {
            let pixels: Vec<_> = (0..w * h)
                .map(|i| {
                    [
                        997 + (i * 791 % 49000) as u16,
                        2199 + (i * 1237 % 43000) as u16,
                        317 + (i * 997 % 59000) as u16,
                        65535,
                    ]
                })
                .collect();
            let base = image(w, h, &pixels, &profile);
            for stops in [-4.0, -0.3125, 0.73125, 4.0] {
                for falloff in [0.25, 1.0, 4.0] {
                    let out = develop(
                        &base,
                        &[],
                        &[],
                        json!([]),
                        Some(json!({"stops":stops,"center":center,"falloff":falloff})),
                    )
                    .unwrap();
                    let expected: Vec<_> = pixels
                        .iter()
                        .enumerate()
                        .map(|(i, pixel)| {
                            let gain = vignette_stops(
                                [(i % w as usize) as f64 + 0.5, (i / w as usize) as f64 + 0.5],
                                [w as usize, h as usize],
                                center,
                                stops,
                                falloff,
                            );
                            let mut result = *pixel;
                            if gain != 0.0 {
                                for c in 0..3 {
                                    let trc = transfer_tag(profile.bytes(), c);
                                    result[c] = transfer_encode(
                                        trc,
                                        transfer_decode(trc, f64::from(pixel[c]) / 65535.0)
                                            * gain.exp2(),
                                    );
                                }
                            }
                            result
                        })
                        .collect();
                    assert_codes(
                        rgba(&out).as_flattened(),
                        expected.as_flattened(),
                        3,
                        "profile vignette",
                    );
                    assert_eq!(out.profile().bytes(), profile.bytes());
                }
            }
        }
    }
}
#[test]
fn neutral_and_single_center_vignette_bypass_exactly() {
    let profile = IccProfile::display_p3();
    let pixels = vec![[101, 12345, 61999, 65535]];
    let base = image(1, 1, &pixels, &profile);
    for (stops, center) in [(0.0, [0.0, 0.0]), (-4.0, [0.5, 0.5]), (4.0, [0.5, 0.5])] {
        assert!(develop(
            &base,
            &[],
            &[],
            json!([]),
            Some(json!({"stops":stops,"center":center,"falloff":0.25}))
        )
        .is_none());
    }
    assert_eq!(rgba(&base), pixels);
}
#[test]
fn vignette_corrects_synthesized_illumination_falloff() {
    let (w, h) = (41, 27);
    let center = [13.5, 9.5];
    let strength = 2.0;
    let falloff = 1.7;
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        let target = [28000, 39000, 51000, 65535];
        let shaded: Vec<_> = (0..w * h)
            .map(|i| {
                let gain = vignette_stops(
                    [(i % w) as f64 + 0.5, (i / w) as f64 + 0.5],
                    [w, h],
                    center,
                    strength,
                    falloff,
                );
                let mut pixel = target;
                for c in 0..3 {
                    let trc = transfer_tag(profile.bytes(), c);
                    pixel[c] = transfer_encode(
                        trc,
                        transfer_decode(trc, f64::from(target[c]) / 65535.0) * (-gain).exp2(),
                    );
                }
                pixel
            })
            .collect();
        assert!(shaded.iter().any(|p| p[0] < target[0] - 5000));
        let base = image(w as u32, h as u32, &shaded, &profile);
        let out = develop(
            &base,
            &[],
            &[],
            json!([]),
            Some(json!({"stops":strength,"center":center,"falloff":falloff})),
        )
        .unwrap();
        assert_codes(
            rgba(&out).as_flattened(),
            vec![target; w * h].as_flattened(),
            3,
            "illumination correction",
        );
    }
}
