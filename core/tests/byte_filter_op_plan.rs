//! The 8-bit `dr_image_filter_op` filters' core plan
//! (`dr_image_filter_op_scratch_bytes`, `ops_spatial::filter_op_scratch_bytes`)
//! and the byte outputs it must leave unchanged. Every expectation is
//! black-box through the C boundary.
use crate::common;
use common::sha256;
use doctored_core::ffi::{dr_image_free, dr_image_from_rgba, dr_image_pixels_rgba, dr_string_free};
use doctored_core::ffi_filters::{dr_image_filter_op, dr_image_filter_op_scratch_bytes};
use doctored_core::DrImage;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Splitmix colour whose alpha is 0 for a quarter of the pixels, a partial
/// value for another quarter and opaque otherwise, so every op meets the
/// alpha-0 and partial-alpha branches.
fn fixture(w: u32, h: u32) -> Vec<u8> {
    let mut state = 0x448_u64;
    (0..u64::from(w) * u64::from(h))
        .flat_map(|_| {
            let v = splitmix(&mut state);
            let alpha = match (v >> 40) % 4 {
                0 => 0,
                1 => 1 + ((v >> 48) as u8 % 254),
                _ => 255,
            };
            [v as u8, (v >> 8) as u8, (v >> 16) as u8, alpha]
        })
        .collect()
}

unsafe fn text(p: *mut c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    s
}

/// Byte `dr_image_filter_op`, uncontrolled: the output bytes, `Ok(None)` for
/// a NULL with no error (a bypass), or the error.
fn byte_op(
    w: u32,
    h: u32,
    bytes: &[u8],
    op: &str,
    params: &str,
) -> Result<Option<Vec<u8>>, String> {
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), w, h) };
    assert!(!img.is_null());
    let op = CString::new(op).unwrap();
    let params = CString::new(params).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out =
        unsafe { dr_image_filter_op(img, op.as_ptr(), params.as_ptr(), ptr::null(), &mut err) };
    let error = unsafe { text(err) };
    let result = if out.is_null() {
        if error.is_empty() {
            Ok(None)
        } else {
            Err(error)
        }
    } else {
        assert!(error.is_empty(), "{error}");
        let v =
            unsafe { std::slice::from_raw_parts(dr_image_pixels_rgba(out), bytes.len()) }.to_vec();
        unsafe { dr_image_free(out) };
        Ok(Some(v))
    };
    unsafe { dr_image_free(img) };
    result
}

/// The pinned cases: op, params.
const CASES: [(&str, &str); 10] = [
    ("unsharp_mask", r#"{"radius":1.5}"#),
    (
        "unsharp_mask",
        r#"{"radius":6,"amount":2,"threshold":0.02}"#,
    ),
    ("high_pass", r#"{"radius":3}"#),
    ("dust_and_scratches", r#"{"radius":2}"#),
    ("dust_and_scratches", r#"{"radius":2,"threshold":0.04}"#),
    ("reduce_noise", r#"{}"#),
    (
        "reduce_noise",
        r#"{"radius":4,"luminance":0.05,"detail":0,"color":1}"#,
    ),
    ("box_blur", r#"{"radius":3}"#),
    ("surface_blur", r#"{"radius":5,"threshold":15}"#),
    ("add_grain", r#"{"seed":1}"#),
];

/// SHA-256 of each case's output, captured on the tree before
/// `ops_photographic::apply`'s straight copy was dropped: 131×300 then 97×1.
const PINS: [[&str; 10]; 2] = [
    [
        "5da7c5e93c1173d9b7be453ee89d7ad6d6f64a30705125d7a49c8e1d2419c090",
        "f78c5e56f0da518d5a590fe1af7bc60c8279a02654e3700d17c7495e66b0122e",
        "df5d8e987ad8f6eaabb992399da52b14a1ff81d4e2b7e61a32d3fd64dc11c65d",
        "48e98af763b7582e536b4d9a6bd32d478ecf1b50efe3e98cca90265b933b84a7",
        "4be4702835a4384cfd67491eea211953a4866ca4efc4465dc5250115c24d830c",
        "fc90fdcd87dac8c4d60c0c60e28469ed4f7f127f262a0237bb71b0e1e642f79d",
        "be0c0b64bb7737559236bdfb44b01bc4c5f9dd692fc6783adb65b445150b7dbf",
        "e35332603091e76664d81d508c4dcd891e64db25d0e4edd314ec8359e12ae074",
        "cd769b51e1919d1a127a25464861122885e850647f28f3c766749ca196bf1bdb",
        "4f482a64222614f3d5e9cbfd7fa5702bdc5746db08797f5d7d2bfaa16aeb69f3",
    ],
    [
        "e60903f5d5eaaa39f79a0e4b2138da22628155d51048cda754ccaa88a420984f",
        "107c9e99c667bba7f9bf0d92b0f508293074c44635c7f44394372ab3606c7ebb",
        "860d6deaa8896a0975dd9f6aa9e96741335550a104770849075931462f11d107",
        "a594deee43d797e963bbc0d7dd7ef21eab87fcb955c6e57406d898bd8b70dfea",
        "a8526893dbf79c0910db1ccbbffed4552df60bc01f633e3c9f6b633a9f97aebb",
        "1859140349ad7ceffcb43ad1af5e0f8cac8e8d366c6590bccc386007a2b9b4d4",
        "ad7f2a8c7ad93f4e67d49593beaacf8b7a06a13112bfa1c051d4d6536010c97d",
        "7d3d94f255c660635c7aaa3b1e326057a613d0f01f75d96bfdfe4b5e985b877a",
        "9d3b7c12f4fb7233a726ab40afdceb2828fc027e17e73fa2b8a2b963763a9d11",
        "13c108a2e13e36df9f56fd443c6707353bbb5131648844acf7a0e7afc3b46859",
    ],
];

/// (a) Every byte output is bit-identical to `PINS`.
#[test]
fn byte_outputs_are_pinned() {
    let mut mismatches = Vec::new();
    for (sizes, pins) in [(131_u32, 300_u32), (97, 1)].into_iter().zip(PINS) {
        let (w, h) = sizes;
        let bytes = fixture(w, h);
        for ((op, params), pin) in CASES.iter().zip(pins) {
            let out = byte_op(w, h, &bytes, op, params)
                .unwrap_or_else(|e| panic!("{op} {params} at {w}x{h}: {e}"))
                .unwrap_or_else(|| panic!("{op} {params} at {w}x{h} bypassed"));
            let digest = sha256::sha256_hex(&out);
            if digest != pin {
                mismatches.push(format!("{op} {params} at {w}x{h}: {digest}"));
            }
        }
    }
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}

/// `dr_image_filter_op_scratch_bytes` on a `w`×`h` image (zeroed: the plan
/// reads only the dimensions): the plan, or the error message.
fn plan(w: u32, h: u32, op: &str, params: &str) -> Result<u64, String> {
    let bytes = vec![0_u8; w as usize * h as usize * 4];
    let img = unsafe { dr_image_from_rgba(bytes.as_ptr(), w, h) };
    assert!(!img.is_null());
    drop(bytes);
    let op = CString::new(op).unwrap();
    let params = CString::new(params).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let scratch =
        unsafe { dr_image_filter_op_scratch_bytes(img, op.as_ptr(), params.as_ptr(), &mut err) };
    unsafe { dr_image_free(img) };
    let error = unsafe { text(err) };
    if error.is_empty() {
        Ok(scratch)
    } else {
        assert_eq!(scratch, 0, "a refusal answers 0");
        Err(error)
    }
}

/// (b) The plan refuses exactly as `dr_image_filter_op` does before it
/// allocates, in the same words: the op name, the JSON and a parameter.
#[test]
fn plan_refuses_in_the_filters_words() {
    let small = (131_u32, 300_u32);
    for (dims, op, params) in [
        (small, "bogus", r#"{}"#),
        (small, "gaussian_blur", r#"{"radius":2}"#),
        (small, "unsharp_mask", "{"),
        (small, "unsharp_mask", "[1]"),
        (small, "unsharp_mask", r#"{"radius":65}"#),
        (small, "box_blur", r#"{"radius":0}"#),
        (small, "high_pass", r#"{"sigma":2}"#),
    ] {
        let (w, h) = dims;
        let bytes = vec![0_u8; w as usize * h as usize * 4];
        let filtered = byte_op(w, h, &bytes, op, params)
            .err()
            .unwrap_or_else(|| panic!("{op} {params} was not refused"));
        drop(bytes);
        let planned = plan(w, h, op, params)
            .err()
            .unwrap_or_else(|| panic!("the plan for {op} {params} was not refused"));
        assert_eq!(planned, filtered, "{op} {params}");
        assert!(!planned.is_empty());
    }
}

/// (c) An exact bypass plans 0 with no error; so does a NULL image, with and
/// without `err_out`.
#[test]
fn bypass_and_null_plan_nothing() {
    for (op, params) in [
        ("unsharp_mask", r#"{"radius":0}"#),
        ("add_grain", r#"{"amount":0}"#),
        ("reduce_noise", r#"{"radius":0}"#),
        ("presence", "{}"),
        ("develop_filter", "{}"),
    ] {
        assert_eq!(plan(131, 300, op, params), Ok(0), "{op} {params}");
        let bytes = fixture(131, 300);
        assert_eq!(
            byte_op(131, 300, &bytes, op, params),
            Ok(None),
            "{op} bypasses"
        );
    }
    let null: *const DrImage = ptr::null();
    let op = CString::new("reduce_noise").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    assert_eq!(
        unsafe { dr_image_filter_op_scratch_bytes(null, op.as_ptr(), ptr::null(), &mut err) },
        0
    );
    assert!(err.is_null(), "a NULL image is not an error");
    assert_eq!(
        unsafe {
            dr_image_filter_op_scratch_bytes(null, op.as_ptr(), ptr::null(), ptr::null_mut())
        },
        0
    );
}

/// The gate F measurements (`designs/phase-9f-selected-tone-status.md`):
/// the largest of three `ri_interval_max_phys_footprint` − pre-call
/// `ri_phys_footprint` readings of an uncontrolled byte `dr_image_filter_op` on
/// opaque splitmix noise, one case per process, `MallocLargeCache=0`. Box
/// Blur r5 is the re-measurement after its one-sum-at-a-time change (the
/// record is `ops_spatial`'s `BOX_BLUR_BYTES_PER_PIXEL`). The 45 MP Dust &
/// Scratches r2 and Surface Blur r5/T15, which run under a job control, are
/// measured in `designs/working-limit-ram-scaled.md`; here their plan is
/// checked against its formula. The `blur` σ2 row (the record is
/// `sampling::blur_scratch_bytes`) is at the larger of its opaque and
/// partial-alpha readings (partial alpha, the image crate's float path); the
/// σ25 and σ64 rows are the re-measurement of the extended-box kernel on
/// opaque noise (the record is `gaussian_blur_filter::plan`; σ25 was
/// 140,738,656 on the recursive filter). The `lens_correction` row is
/// all three sampling passes, a perspective and a vignette: E 480,264,528 in
/// each of three runs (its 20n combine stage binds; the record is on
/// `sampling_lens`'s plan).
const MEASURED_24MP: [(&str, &str, u64); 14] = [
    ("reduce_noise", r#"{}"#, 480_346_472),
    ("reduce_noise", r#"{"radius":16}"#, 598_016_432),
    ("dust_and_scratches", r#"{"radius":2}"#, 480_264_528),
    ("dust_and_scratches", r#"{"radius":3}"#, 480_264_528),
    ("unsharp_mask", r#"{"radius":1.5}"#, 480_280_912),
    ("unsharp_mask", r#"{"radius":8}"#, 480_280_912),
    ("high_pass", r#"{"radius":3}"#, 480_280_912),
    ("box_blur", r#"{"radius":5}"#, 576_323_992),
    (
        "surface_blur",
        r#"{"radius":5,"threshold":15}"#,
        480_248_144,
    ),
    ("add_grain", r#"{"seed":1}"#, 576_340_376),
    ("blur", r#"{"sigma":25}"#, 136_773_704),
    ("blur", r#"{"sigma":64}"#, 162_906_208),
    ("blur", r#"{"sigma":2}"#, 139_952_224),
    (
        "lens_correction",
        r#"{"distortion":40,"chromatic_red_cyan":60,"chromatic_blue_yellow":-40,"vignette_amount":-30,"vertical_perspective":-30,"horizontal_perspective":20,"angle":3,"scale":105}"#,
        480_264_528,
    ),
];
const MEASURED_45MP: [(&str, &str, u64); 3] = [
    ("reduce_noise", r#"{}"#, 909_394_592),
    ("unsharp_mask", r#"{"radius":8}"#, 909_279_880),
    ("box_blur", r#"{"radius":5}"#, 1_091_158_808),
];

/// (d) One image at a time, every measured case's plan is at least 1.25× its
/// measured peak.
/// At 8256×5504 Dust & Scratches r2 and Surface Blur r5/T15 plan (a plan
/// carries no work refusal) at the combine stage, whose 20n is above either
/// kernel's own plan.
#[test]
fn plans_cover_the_measured_peaks() {
    for ((w, h), cases) in [
        ((6000_u32, 4000_u32), &MEASURED_24MP[..]),
        ((8256, 5504), &MEASURED_45MP[..]),
    ] {
        for &(op, params, measured) in cases {
            let scratch = plan(w, h, op, params).unwrap_or_else(|e| panic!("{op} {params}: {e}"));
            let floor = (measured * 5).div_ceil(4);
            assert!(
                scratch >= floor,
                "{op} {params} at {w}x{h}: {scratch} < {floor}"
            );
        }
    }
    let n = 8256_u64 * 5504;
    // (20n + 4·BK) + ⌈/4⌉, BK = 1 MiB: 913,014,784 + 228,253,696.
    let base = 20 * n + 4 * (1 << 20);
    let expected = base + base.div_ceil(4);
    assert_eq!(expected, 1_141_268_480);
    for (op, params) in [
        ("dust_and_scratches", r#"{"radius":2}"#),
        ("surface_blur", r#"{"radius":5,"threshold":15}"#),
    ] {
        assert_eq!(plan(8256, 5504, op, params), Ok(expected), "{op} at 45 MP");
    }
}
