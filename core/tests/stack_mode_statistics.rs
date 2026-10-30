//! Six Stack Modes (Summation, Variance, Entropy,
//! Kurtosis, Skewness, Majority) against hand-computed closed forms at 8
//! bits (`dr_doc_stack_mode`), 16 bits and float (`dr_precision_doc_edit`).
//!
//! Every case is one opaque pixel per member, so each member is fully
//! covered and reads unweighted. F is the full value (255, 65535, 1.0). The
//! expected numbers are derived in the comments from the rule
//! (`stack_moments`' module doc): Variance m₂ / F; Skewness
//! F/2·(1 + s/s_max), s_max = (n − 2)/√(n − 1); Kurtosis
//! F·(k − 1)/(k_max − 1), k_max = (n² − 3n + 3)/(n − 1); Entropy H₂ over 256
//! equal bins / log₂ min(n, 256) × F; Majority the most frequent value, a tie
//! the median of the tied values. Integers round half up.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc::DrDocument;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_stack::dr_doc_stack_mode;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common::{layer_bytes, take_doc, take_err_string};
use crate::float_fixture::float_doc;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

fn request(n: usize, mode: &str) -> Value {
    let layers: Vec<usize> = (0..n).collect();
    json!({"op": "stack_mode", "layers": layers, "above": n - 1, "mode": mode, "name": "Stat"})
}

/// The 8-bit result of `mode` over members holding `members[i]` (RGB, opaque).
fn byte(members: &[[u8; 3]], mode: &str) -> [u8; 4] {
    let pixel = |m: [u8; 3]| RgbaImage::from_pixel(1, 1, Rgba([m[0], m[1], m[2], 255]));
    let mut doc = DrDocument::from_pixels(pixel(members[0]));
    for (i, m) in members.iter().enumerate().skip(1) {
        doc = doc.adding_image_layer(i - 1, pixel(*m), "M").unwrap();
    }
    let text = CString::new(request(members.len(), mode).to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_stack_mode(&doc, text.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    let (out, err) = take_doc(out, err);
    let out = out.unwrap_or_else(|| panic!("{mode}: {err:?}"));
    let bytes = layer_bytes(&out, members.len());
    [bytes[0], bytes[1], bytes[2], bytes[3]]
}

fn edited(doc: &PrecisionDocument, req: &Value) -> Box<PrecisionDocument> {
    let text = CString::new(req.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_edit(
            doc,
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{req}: {}", take_err_string(err));
    unsafe { Box::from_raw(out) }
}

/// The 16-bit result of `mode` over members holding `members[i]`.
fn wide(members: &[[u16; 3]], mode: &str) -> [u16; 4] {
    let layers = members
        .iter()
        .map(|m| {
            let image = PrecisionImage::from_rgba16(
                1,
                1,
                vec![[m[0], m[1], m[2], 65535]],
                IccProfile::srgb(),
                None,
            )
            .unwrap();
            PrecisionDocumentLayer::raster("M", image, (0, 0), 1.0, true).unwrap()
        })
        .collect();
    let doc = PrecisionDocument::from_parts(
        1,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap();
    let out = edited(&doc, &request(members.len(), mode));
    out.layer(members.len())
        .unwrap()
        .source()
        .unwrap()
        .to_rgba16()
        .unwrap()[0]
}

/// The float result of `mode` over members holding `members[i]`.
fn real(members: &[[f32; 3]], mode: &str) -> [f32; 4] {
    let records: Vec<Vec<[f32; 4]>> = members
        .iter()
        .map(|m| vec![[m[0], m[1], m[2], 1.0]])
        .collect();
    let doc = float_doc(1, 1, &IccProfile::srgb(), &records);
    let out = edited(&doc, &request(members.len(), mode));
    out.layer(members.len())
        .unwrap()
        .source()
        .unwrap()
        .to_rgba32()
        .unwrap()[0]
}

/// Members whose channels are the three columns.
fn columns<T: Copy>(r: &[T], g: &[T], b: &[T]) -> Vec<[T; 3]> {
    (0..r.len()).map(|i| [r[i], g[i], b[i]]).collect()
}

fn close(got: [f32; 4], want: [f64; 3], what: &str) {
    for c in 0..3 {
        let bound = 1e-6 * want[c].abs().max(1.0);
        assert!(
            (f64::from(got[c]) - want[c]).abs() <= bound,
            "{what}, channel {c}: {} against {}",
            got[c],
            want[c]
        );
    }
    assert_eq!(got[3], 1.0, "{what}: alpha");
}

// ------------------------------------------------------------------ 8 bits --

/// R {0,0,0,255}: one member apart, high. G {0,255,255,255}: one apart, low.
/// B {0,0,255,255}: two equal clusters.
#[test]
fn four_members_at_8_bits() {
    let members = columns(&[0, 0, 0, 255], &[0, 255, 255, 255], &[0, 0, 255, 255]);
    // Σv: 255; 765 and 510 clamp to 255.
    assert_eq!(byte(&members, "summation"), [255, 255, 255, 255]);
    // (n·Σv² − (Σv)²)/(n²·F): R (4·65025 − 65025)/4080 = 47.8125 → 48, G
    // the same, B (4·130050 − 260100)/4080 = 63.75 → 64.
    assert_eq!(byte(&members, "variance"), [48, 48, 64, 255]);
    // One member apart is the bound: s = +s_max → F, s = −s_max → 0 (below
    // F/2); two equal clusters are symmetric, s = 0 → 127.5 → 128.
    assert_eq!(byte(&members, "skewness"), [255, 0, 128, 255]);
    // One member apart is k_max(4) = 7/3 exactly → F for R and G; two equal
    // clusters have k = 1 → 0.
    assert_eq!(byte(&members, "kurtosis"), [255, 255, 0, 255]);
    // R, G: H = −(¾·log₂¾ + ¼·log₂¼) = 0.811278, / log₂4 → 0.405639 × 255 =
    // 103.44 → 103. B: H = 1, / 2 → 127.5 → 128.
    assert_eq!(byte(&members, "entropy"), [103, 103, 128, 255]);
    // R 0 (three), G 255 (three), B the tie {0, 255} → (0 + 255)/2 half up 128.
    assert_eq!(byte(&members, "majority"), [0, 255, 128, 255]);
}

/// R {0,0,0,255,255}, G {10,20,20,30,30}, B {1,2,3,4,5}: the kurtosis of R
/// and of G is strictly between 0 and F.
#[test]
fn five_members_at_8_bits() {
    let members = columns(
        &[0, 0, 0, 255, 255],
        &[10, 20, 20, 30, 30],
        &[1, 2, 3, 4, 5],
    );
    // 510 clamps to 255; 110; 15.
    assert_eq!(byte(&members, "summation"), [255, 110, 15, 255]);
    // R (5·130050 − 260100)/6375 = 61.2 → 61; G (5·2700 − 12100)/6375 =
    // 0.22 → 0; B (5·55 − 225)/6375 → 0.
    assert_eq!(byte(&members, "variance"), [61, 0, 0, 255]);
    // R: p = 2/5 high, s = (1 − 2p)/√(p(1 − p)) = 0.408248, s_max(5) = 1.5,
    // 127.5·(1 + 0.272166) = 162.2 → 162. G: deviations −12,−2,−2,8,8, m₂ 56,
    // m₃ −144, s = −144/56^1.5 = −0.343622, 127.5·(1 − 0.229081) = 98.29 →
    // 98. B symmetric → 128.
    assert_eq!(byte(&members, "skewness"), [162, 98, 128, 255]);
    // R: k = (1 − 3p + 3p²)/(p(1 − p)) = 0.28/0.24 = 7/6, k_max(5) = 13/4,
    // (1/6)/(9/4) = 0.074074 × 255 = 18.89 → 19. G: m₄ 5792, k = 5792/3136 =
    // 1.846939, 0.376417 × 255 = 95.99 → 96. B: m₂ 2, m₄ 6.8, k = 1.7,
    // 0.311111 × 255 = 79.33 → 79.
    assert_eq!(byte(&members, "kurtosis"), [19, 96, 79, 255]);
    // R: H(⅗, ⅖) = 0.970951 / log₂5 (2.321928) = 0.418166 × 255 = 106.63 →
    // 107. G: H(⅕, ⅖, ⅖) = 1.521928 / 2.321928 = 0.655459 × 255 = 167.14 →
    // 167. B: five distinct values, H = log₂5 → F.
    assert_eq!(byte(&members, "entropy"), [107, 167, 255, 255]);
    // R 0 (three); G the tie {20, 30} → 25; B a five-way tie → its median 3.
    assert_eq!(byte(&members, "majority"), [0, 25, 3, 255]);
}

/// n = 2: s_max = 0 and k ≡ 1, so Skewness reads F/2 and Kurtosis 0.
#[test]
fn two_members_at_8_bits() {
    let members = columns(&[0, 255], &[40, 40], &[100, 101]);
    assert_eq!(byte(&members, "summation"), [255, 80, 201, 255]);
    // R (2·65025 − 65025)/1020 = 63.75 → 64; G 0; B (2·20201 − 40401)/1020 → 0.
    assert_eq!(byte(&members, "variance"), [64, 0, 0, 255]);
    assert_eq!(byte(&members, "skewness"), [128, 128, 128, 255]);
    assert_eq!(byte(&members, "kurtosis"), [0, 0, 0, 255]);
    // Two bins → H = 1 = log₂2 → F; one bin → 0; 100 and 101 are two bins.
    assert_eq!(byte(&members, "entropy"), [255, 0, 255, 255]);
    // Ties: (0 + 255)/2 → 128; 40; (100 + 101)/2 = 100.5 → 101.
    assert_eq!(byte(&members, "majority"), [128, 40, 101, 255]);
}

// ----------------------------------------------------------------- 16 bits --

#[test]
fn the_closed_forms_at_16_bits() {
    // R {0,0,0,F}, G {0,F,F,F}, B {0,0,0,F}.
    let members = columns(
        &[0, 0, 0, 65535],
        &[0, 65535, 65535, 65535],
        &[0, 0, 0, 65535],
    );
    // 3·F² / (16·F) = 3·65535/16 = 12287.8125 → 12288.
    assert_eq!(wide(&members, "variance"), [12288, 12288, 12288, 65535]);
    assert_eq!(wide(&members, "skewness"), [65535, 0, 65535, 65535]);
    assert_eq!(wide(&members, "kurtosis"), [65535, 65535, 65535, 65535]);
    // 0.405639 × 65535 = 26583.56 → 26584 (bins 0 and 255, `v >> 8`).
    assert_eq!(wide(&members, "entropy"), [26584, 26584, 26584, 65535]);
    assert_eq!(wide(&members, "majority"), [0, 65535, 0, 65535]);

    // R {0,0,0,F,F}: kurtosis 0.074074 × 65535 = 4854.4 → 4854, skewness
    // 0.636083 × 65535 = 41685.7 → 41686.
    let five = columns(
        &[0, 0, 0, 65535, 65535],
        &[0, 0, 0, 65535, 65535],
        &[0, 0, 0, 65535, 65535],
    );
    assert_eq!(wide(&five, "kurtosis"), [4854, 4854, 4854, 65535]);
    assert_eq!(wide(&five, "skewness"), [41686, 41686, 41686, 65535]);
    assert_eq!(wide(&five, "summation"), [65535, 65535, 65535, 65535]);
}

/// Members that differ within one bin read no entropy: 256, 300 and 511 are
/// all bin 1 (`v >> 8`); 255 and 256 straddle a bin edge and read F.
#[test]
fn entropy_bins_at_16_bits() {
    let within = columns(&[256, 300, 511], &[256, 300, 511], &[256, 300, 511]);
    assert_eq!(wide(&within, "entropy"), [0, 0, 0, 65535]);
    // Σ 1067; the three-way tie's median 300.
    assert_eq!(wide(&within, "summation"), [1067, 1067, 1067, 65535]);
    assert_eq!(wide(&within, "majority"), [300, 300, 300, 65535]);
    let straddle = columns(&[255, 256], &[255, 256], &[255, 256]);
    assert_eq!(wide(&straddle, "entropy"), [65535, 65535, 65535, 65535]);
}

// ------------------------------------------------------------------- float --

#[test]
fn the_closed_forms_at_float() {
    // R {0,0,0,1}, G {0,1,1,1}, B {0,0,0,1}.
    let members = columns(
        &[0.0, 0.0, 0.0, 1.0],
        &[0.0, 1.0, 1.0, 1.0],
        &[0.0, 0.0, 0.0, 1.0],
    );
    close(
        real(&members, "summation"),
        [1.0, 3.0, 1.0],
        "summation, 3 above white",
    );
    // m₂ of one member apart: p(1 − p) = 3/16.
    close(real(&members, "variance"), [0.1875; 3], "variance");
    close(real(&members, "skewness"), [1.0, 0.0, 1.0], "skewness");
    close(real(&members, "kurtosis"), [1.0; 3], "kurtosis");
    close(
        real(&members, "entropy"),
        [0.405_639_062_229_566_4; 3],
        "entropy",
    );
    close(real(&members, "majority"), [0.0, 1.0, 0.0], "majority");

    // Above white: Σ and m₂ are stored unclamped, the entropy bins clamp
    // (2.0 and 5.0 are both bin 255), the tie's median is 3.5.
    let bright = columns(&[2.0, 5.0], &[2.0, 5.0], &[2.0, 5.0]);
    close(
        real(&bright, "summation"),
        [7.0; 3],
        "summation above white",
    );
    close(real(&bright, "variance"), [2.25; 3], "variance above white");
    close(real(&bright, "entropy"), [0.0; 3], "entropy above white");
    close(real(&bright, "majority"), [3.5; 3], "majority above white");
}

/// 0.5, 0.501 and 0.502 are all bin 128 (⌊v·256⌋); 0.49 is bin 125.
#[test]
fn entropy_bins_at_float() {
    let within = columns(
        &[0.5, 0.501, 0.502],
        &[0.5, 0.501, 0.502],
        &[0.5, 0.501, 0.502],
    );
    close(real(&within, "entropy"), [0.0; 3], "within one bin");
    // A three-way tie's median is the member's own value, bit for bit.
    let majority = real(&within, "majority");
    assert_eq!(majority[0].to_bits(), 0.501_f32.to_bits());
    let apart = columns(&[0.5, 0.49], &[0.5, 0.49], &[0.5, 0.49]);
    close(real(&apart, "entropy"), [1.0; 3], "two bins");
}
