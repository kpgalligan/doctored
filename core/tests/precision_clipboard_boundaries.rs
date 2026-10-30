//! Phase 12G-2: the clipboard's BOUNDARIES — RZCB v2 (flags, byte origin,
//! the carried tone map), `from_rgba8`/`to_rgba8`, the convert arm
//! (`ClipboardPayload::converted`), Paste's arms, the float Cut and New from
//! Clipboard, plus the three new FFI exports.
//!
//! Every reference is written here and never read off the implementation:
//! an independent v2 encoder, the profile's own `rXYZ`/`gXYZ`/`bXYZ` and
//! `curv` tag DATA driven through the test's f64 matrix inverse, linear
//! interpolation and exact piecewise-linear inversion (`common::icc_chain`,
//! shared with the promotion tests), the tone-map formulas
//! written out from their definitions, and `a · (1 − c/65535)` for the erase.
//!
//! Tolerances, fixed before anything was measured: the codec, `from_rgba8`,
//! `to_rgba8`, Paste and New from Clipboard are EXACT; every conversion is
//! within 1 u16 code of its f64 chain (the core computes in f32); the float
//! erase is within 1e-6 (relative to the source alpha) of f64 and bit-exact
//! against the test's own f32 loop.

use crate::common;

use std::ffi::{c_char, CStr};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi::{
    dr_image_free, dr_image_height, dr_image_pixels_rgba, dr_image_width, dr_string_free,
};
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_free, dr_clipboard_payload_from_rgba8, dr_clipboard_payload_info,
    dr_clipboard_payload_rgba8, dr_precision_doc_from_clipboard, DrClipboardPayload,
};
use doctored_core::ffi_precision::dr_precision_doc_free;
use doctored_core::float_document::ToneMap;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{ClipboardPayload, ClipboardRead, PasteRequest};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use serde_json::{json, Value};

use common::icc_chain::{apply, inverse, Chain};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const BOOKKEEPING: u64 = 1024 * 1024;
const LOCK_TRANSPARENCY: u32 = 1;
const LOCK_PIXELS: u32 = 2;
const COVERAGE: [u16; 5] = [0, 1, 32768, 65534, 65535];

// ------------------------------------------------------------- plumbing --

fn take_error(error: *mut c_char) -> Option<String> {
    if error.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(error) };
    Some(text)
}

enum Samples {
    U16(Vec<[u16; 4]>),
    F32(Vec<[f32; 4]>),
}

/// The test's own RZCB writer. `version` 1 writes the 12G-1 layout (the
/// flag byte and no tone-map words); 2 appends `operator` and `white`.
#[allow(clippy::too_many_arguments)]
fn drcb(
    version: u32,
    w: u32,
    h: u32,
    offset: (i32, i32),
    ppi: (f32, f32),
    profile: &[u8],
    samples: &Samples,
    flags: u8,
    tone: (u32, f32),
    coverage: Option<&[u16]>,
) -> Vec<u8> {
    let mut out = b"RZCB".to_vec();
    out.extend(version.to_le_bytes());
    out.extend(
        match samples {
            Samples::U16(_) => 1u32,
            Samples::F32(_) => 2u32,
        }
        .to_le_bytes(),
    );
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    out.extend(offset.0.to_le_bytes());
    out.extend(offset.1.to_le_bytes());
    out.extend(ppi.0.to_le_bytes());
    out.extend(ppi.1.to_le_bytes());
    out.extend((profile.len() as u32).to_le_bytes());
    out.extend(profile);
    out.push(flags);
    if version >= 2 {
        out.extend(tone.0.to_le_bytes());
        out.extend(tone.1.to_le_bytes());
    }
    if version == 3 {
        // No editable section: a zero length word.
        out.extend(0u32.to_le_bytes());
    }
    match samples {
        Samples::U16(p) => p.iter().flatten().for_each(|v| out.extend(v.to_le_bytes())),
        Samples::F32(p) => p.iter().flatten().for_each(|v| out.extend(v.to_le_bytes())),
    }
    if let Some(c) = coverage {
        c.iter().for_each(|v| out.extend(v.to_le_bytes()));
    }
    out
}

/// A v2 stream with the defaults a u16 payload writes (operator 0, white 4).
fn u16_stream(w: u32, h: u32, profile: &[u8], pixels: Vec<[u16; 4]>, flags: u8) -> Vec<u8> {
    drcb(
        2,
        w,
        h,
        (0, 0),
        (72.0, 72.0),
        profile,
        &Samples::U16(pixels),
        flags,
        (0, 4.0),
        None,
    )
}

fn f32_stream(w: u32, h: u32, profile: &[u8], pixels: Vec<[f32; 4]>, tone: (u32, f32)) -> Vec<u8> {
    drcb(
        2,
        w,
        h,
        (0, 0),
        (72.0, 72.0),
        profile,
        &Samples::F32(pixels),
        0,
        tone,
        None,
    )
}

fn decode(bytes: &[u8]) -> Result<ClipboardPayload, String> {
    ClipboardPayload::decode(bytes, LIMIT, 0)
}

/// Byte codes ×257, so every sample is a legal byte-origin sample.
fn byte_pattern(n: u32) -> Vec<[u8; 4]> {
    let codes = [0u8, 1, 17, 128, 200, 254, 255, 77];
    (0..n as usize)
        .map(|k| {
            [
                codes[k % 8],
                codes[(k + 3) % 8],
                codes[(k + 5) % 8],
                codes[(k * 7 + 1) % 8],
            ]
        })
        .collect()
}

fn widened(bytes: &[[u8; 4]]) -> Vec<[u16; 4]> {
    bytes
        .iter()
        .map(|p| p.map(|v| u16::from(v) * 257))
        .collect()
}

/// A u16 pattern of sub-byte codes that stays inside both gamuts' reach.
fn u16_pattern(n: u32) -> Vec<[u16; 4]> {
    let codes = [0u16, 1, 0x0101, 1000, 12345, 32768, 40000, 65534, 65535];
    (0..n as usize)
        .map(|k| {
            [
                codes[k % 9],
                codes[(k + 4) % 9],
                codes[(k + 7) % 9],
                codes[(k * 5 + 2) % 9],
            ]
        })
        .collect()
}

/// Linear float samples with negatives, above-white and fractional alpha.
fn f32_pattern(n: u32) -> Vec<[f32; 4]> {
    let values = [-0.25f32, 0.0, 0.0031, 0.18, 0.5, 0.99, 1.0, 1.7, 4.0, 12.0];
    let alphas = [1.0f32, 0.25, 0.5, 0.0, 0.75, 1.0, 0.6];
    (0..n as usize)
        .map(|k| {
            [
                values[k % 10],
                values[(k + 3) % 10],
                values[(k + 7) % 10],
                alphas[k % 7],
            ]
        })
        .collect()
}

fn paste_request(position: &str, placement: &str, profile: &str) -> PasteRequest {
    serde_json::from_value(json!({
        "name": "Pasted", "anchor": 0, "position": position,
        "placement": placement, "profile": profile,
    }))
    .unwrap()
}

fn u16_doc(w: u32, h: u32, profile: Arc<IccProfile>) -> PrecisionDocument {
    let image = PrecisionImage::from_rgba16(w, h, u16_pattern(w * h), profile.clone(), None)
        .expect("image");
    PrecisionDocument::from_parts(
        w,
        h,
        profile,
        Metadata::default(),
        Resolution::default(),
        vec![Layer::raster("B", image, (0, 0), 1.0, true).unwrap()],
    )
    .expect("document")
}

/// A float document authored through New from Clipboard itself.
fn float_doc(w: u32, h: u32, profile: &[u8], map: ToneMap) -> PrecisionDocument {
    let (op, white) = match map {
        ToneMap::Clip => (0, 4.0),
        ToneMap::Reinhard { white } => (1, white),
    };
    let payload = decode(&f32_stream(w, h, profile, f32_pattern(w * h), (op, white))).unwrap();
    let doc = PrecisionDocument::from_clipboard(&payload, LIMIT, 0, None).expect("float doc");
    assert!(doc.is_float());
    doc
}

// -------------------------------------------- the independent colour chain --

fn code(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// One u16 RGB triple from `src`'s profile into `dst`'s, all in f64.
fn profile_oracle(src: &Chain, dst: &Chain, pixel: [u16; 4]) -> [u16; 4] {
    let linear = std::array::from_fn(|c| src.decode(f64::from(pixel[c]) / 65535.0));
    let xyz = apply(src.to_pcs, linear);
    let out = apply(inverse(dst.to_pcs), xyz);
    [
        code(dst.encode(out[0])),
        code(dst.encode(out[1])),
        code(dst.encode(out[2])),
        pixel[3],
    ]
}

/// The tone maps, from their definitions.
fn map_oracle(map: ToneMap, v: f64) -> f64 {
    match map {
        ToneMap::Clip => v.clamp(0.0, 1.0),
        ToneMap::Reinhard { white } => {
            let w = f64::from(white);
            let u = v.max(0.0);
            (u * (1.0 + u / (w * w)) / (1.0 + u)).min(1.0)
        }
    }
}

fn float_oracle(chain: &Chain, map: ToneMap, pixel: [f32; 4]) -> [u16; 4] {
    [
        code(chain.encode(map_oracle(map, f64::from(pixel[0])))),
        code(chain.encode(map_oracle(map, f64::from(pixel[1])))),
        code(chain.encode(map_oracle(map, f64::from(pixel[2])))),
        code(f64::from(pixel[3])),
    ]
}

fn assert_within_one(got: &[[u16; 4]], want: &[[u16; 4]], what: &str) {
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..3 {
            let d = (i32::from(g[c]) - i32::from(w[c])).abs();
            assert!(
                d <= 1,
                "{what}: pixel {i} channel {c}: {} vs {}",
                g[c],
                w[c]
            );
        }
        assert_eq!(g[3], w[3], "{what}: pixel {i} alpha is exact");
    }
}

// ----------------------------------------------------------------- codec --

#[test]
fn v2_round_trips_every_flag_and_the_carried_tone_map() {
    let profile = IccProfile::srgb().bytes().to_vec();
    let (w, h) = (3u32, 2u32);
    let pixels = widened(&byte_pattern(w * h));
    let coverage = [0u16, 1, 32768, 65534, 65535, 0x0101];
    for flags in 0u8..=3 {
        let cov = (flags & 1 != 0).then_some(&coverage[..]);
        let bytes = drcb(
            2,
            w,
            h,
            (-3, 4),
            (300.0, 150.0),
            &profile,
            &Samples::U16(pixels.clone()),
            flags,
            (0, 4.0),
            cov,
        );
        let payload = decode(&bytes).expect("u16 v2 decodes");
        assert_eq!(payload.coverage(), cov);
        assert_eq!(payload.byte_origin(), flags & 2 != 0);
        assert_eq!(payload.tone_map(), None);
        assert_eq!(payload.image().to_rgba16().ok().unwrap(), &pixels[..]);
        let v3 = drcb(
            3,
            w,
            h,
            (-3, 4),
            (300.0, 150.0),
            &profile,
            &Samples::U16(pixels.clone()),
            flags,
            (0, 4.0),
            cov,
        );
        assert_eq!(payload.encode().unwrap(), v3, "flags {flags} re-encode");
        let info = payload.info();
        assert_eq!(info["version"], 3);
        assert_eq!(info["depth"], 16);
        assert_eq!(info["origin_depth"], if flags & 2 != 0 { 8 } else { 16 });
        assert!(info.get("tone_map").is_none());
    }
    let floats = f32_pattern(w * h);
    for (map, tone, json) in [
        (ToneMap::Clip, (0u32, 4.0f32), json!({"operator": "clip"})),
        (
            ToneMap::Reinhard { white: 2.5 },
            (1, 2.5),
            json!({"operator": "reinhard", "white": 2.5}),
        ),
    ] {
        for flags in 0u8..=1 {
            let cov = (flags & 1 != 0).then_some(&coverage[..]);
            let bytes = drcb(
                2,
                w,
                h,
                (1, 1),
                (72.0, 72.0),
                &profile,
                &Samples::F32(floats.clone()),
                flags,
                tone,
                cov,
            );
            let payload = decode(&bytes).expect("f32 v2 decodes");
            assert_eq!(payload.tone_map(), Some(map));
            assert!(!payload.byte_origin());
            assert_eq!(payload.coverage(), cov);
            let v3 = drcb(
                3,
                w,
                h,
                (1, 1),
                (72.0, 72.0),
                &profile,
                &Samples::F32(floats.clone()),
                flags,
                tone,
                cov,
            );
            assert_eq!(payload.encode().unwrap(), v3);
            let info = payload.info();
            assert_eq!(info["tone_map"], json);
            assert_eq!(info["origin_depth"], 32);
        }
    }
    // A v1 stream still decodes; its float payload reads as Clip.
    let v1 = drcb(
        1,
        w,
        h,
        (0, 0),
        (72.0, 72.0),
        &profile,
        &Samples::F32(floats.clone()),
        0,
        (0, 0.0),
        None,
    );
    assert_eq!(decode(&v1).unwrap().tone_map(), Some(ToneMap::Clip));
    let v1 = drcb(
        1,
        w,
        h,
        (0, 0),
        (72.0, 72.0),
        &profile,
        &Samples::U16(pixels),
        1,
        (0, 0.0),
        Some(&coverage),
    );
    let payload = decode(&v1).unwrap();
    assert_eq!(payload.tone_map(), None);
    assert!(!payload.byte_origin());
    assert_eq!(payload.coverage(), Some(&coverage[..]));
}

#[test]
fn v2_refuses_every_bad_flag_and_tone_word_by_name() {
    let profile = IccProfile::srgb().bytes().to_vec();
    let bytes = widened(&byte_pattern(2));
    let refused = |stream: Vec<u8>, needle: &str| {
        let err = decode(&stream).err().expect("refused");
        assert!(err.contains(needle), "{needle}: {err}");
    };
    refused(u16_stream(2, 1, &profile, bytes.clone(), 4), "flags 4");
    let v1 = drcb(
        1,
        2,
        1,
        (0, 0),
        (72.0, 72.0),
        &profile,
        &Samples::U16(bytes.clone()),
        2,
        (0, 0.0),
        None,
    );
    refused(v1, "coverage flag");
    let floats = vec![[0.5f32, 0.5, 0.5, 1.0]; 2];
    let f32_bit1 = drcb(
        2,
        2,
        1,
        (0, 0),
        (72.0, 72.0),
        &profile,
        &Samples::F32(floats.clone()),
        2,
        (0, 4.0),
        None,
    );
    refused(f32_bit1, "byte-origin");
    let mut odd = bytes.clone();
    odd[1][3] = 256;
    refused(u16_stream(2, 1, &profile, odd, 2), "multiple of 257");
    let operator_on_u16 = drcb(
        2,
        2,
        1,
        (0, 0),
        (72.0, 72.0),
        &profile,
        &Samples::U16(bytes),
        0,
        (1, 2.0),
        None,
    );
    refused(operator_on_u16, "operator 1");
    refused(
        f32_stream(2, 1, &profile, floats.clone(), (2, 4.0)),
        "tone map",
    );
    refused(f32_stream(2, 1, &profile, floats, (1, 0.5)), "white point");
}

#[test]
fn from_rgba8_widens_exactly_and_is_admitted_first() {
    let srgb = IccProfile::srgb();
    let profile = srgb.bytes().to_vec();
    let (w, h) = (5u32, 3u32);
    let bytes = byte_pattern(w * h);
    let flat: Vec<u8> = bytes.iter().flatten().copied().collect();
    let payload = ClipboardPayload::from_rgba8(
        &flat,
        w,
        h,
        &profile,
        (7, -2),
        Resolution { x: 300.0, y: 300.0 },
        LIMIT,
        0,
    )
    .unwrap();
    assert!(payload.byte_origin());
    assert_eq!(payload.coverage(), None);
    assert_eq!(payload.offset(), (7, -2));
    assert_eq!(payload.resolution().x, 300.0);
    assert_eq!(&**payload.image().profile().bytes(), &profile[..]);
    let samples = payload.image().to_rgba16().ok().unwrap();
    for (s, b) in samples.iter().zip(&bytes) {
        for c in 0..4 {
            assert_eq!(u32::from(s[c]), u32::from(b[c]) * 257);
        }
    }
    // Its encoding round-trips with the flag set.
    let again = decode(&payload.encode().unwrap()).unwrap();
    assert!(again.byte_origin());
    assert_eq!(again.info()["origin_depth"], 8);
    // Admission: the input, 4·bookkeeping, 2·payload and the ICC scratch.
    let big = (400u32, 400u32);
    let big_pixels = vec![9u8; (big.0 * big.1 * 4) as usize];
    let payload_bytes = u64::from(big.0 * big.1) * 8;
    let need = big_pixels.len() as u64
        + profile.len() as u64
        + 4 * BOOKKEEPING
        + 2 * payload_bytes
        + profile.len() as u64 * 64;
    let make = |limit| {
        ClipboardPayload::from_rgba8(
            &big_pixels,
            big.0,
            big.1,
            &profile,
            (0, 0),
            Resolution::default(),
            limit,
            0,
        )
    };
    assert!(make(2 * payload_bytes - 1)
        .err()
        .unwrap()
        .contains("working limit"));
    assert!(make(need - 1).err().unwrap().contains("working limit"));
    make(need).expect("admitted at its need");
    // Refusals: a short buffer, a garbage profile, a bad PPI.
    let short = ClipboardPayload::from_rgba8(
        &flat[1..],
        w,
        h,
        &profile,
        (0, 0),
        Resolution::default(),
        LIMIT,
        0,
    );
    assert!(short.err().unwrap().contains("width times height"));
    let garbage = ClipboardPayload::from_rgba8(
        &flat,
        w,
        h,
        &[0x55; 200],
        (0, 0),
        Resolution::default(),
        LIMIT,
        0,
    );
    assert!(garbage.err().unwrap().contains("profile"));
    let ppi = ClipboardPayload::from_rgba8(
        &flat,
        w,
        h,
        &profile,
        (0, 0),
        Resolution {
            x: f32::NAN,
            y: 72.0,
        },
        LIMIT,
        0,
    );
    assert!(ppi.err().unwrap().contains("resolution"));
}

#[test]
fn to_rgba8_inverts_a_byte_origin_payload_and_maps_float_through_its_tone_map() {
    let profile = IccProfile::srgb().bytes().to_vec();
    let (w, h) = (4u32, 4u32);
    let flat: Vec<u8> = byte_pattern(w * h).into_iter().flatten().collect();
    let payload = ClipboardPayload::from_rgba8(
        &flat,
        w,
        h,
        &profile,
        (0, 0),
        Resolution::default(),
        LIMIT,
        0,
    )
    .unwrap();
    let back = payload.to_rgba8(LIMIT, 0, None).unwrap();
    assert_eq!(back.dimensions(), (w, h));
    assert_eq!(back.as_raw(), &flat);
    let float = decode(&f32_stream(
        1,
        1,
        &profile,
        vec![[2.0, 0.5, -1.0, 1.0]],
        (0, 4.0),
    ))
    .unwrap();
    // A float payload reads through its own tone map — its u16
    // conversion, then the u16 read.
    let mapped = float.to_rgba8(LIMIT, 0, None).unwrap();
    let want = float
        .converted(
            PrecisionStorage::Rgba16Unorm,
            &IccProfile::srgb(),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap()
        .to_rgba8(LIMIT, 0, None)
        .unwrap();
    assert_eq!(mapped.as_raw(), want.as_raw());
}

// ------------------------------------------------------------ conversion --

#[test]
fn converted_u16_srgb_to_p3_matches_the_f64_chain() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let pixels = u16_pattern(81);
    let payload = decode(&u16_stream(9, 9, srgb.bytes(), pixels.clone(), 0)).unwrap();
    let out = payload
        .converted(PrecisionStorage::Rgba16Unorm, &p3, LIMIT, 0, None)
        .unwrap()
        .expect("a real conversion");
    assert_eq!(&**out.image().profile().bytes(), &p3.bytes()[..]);
    assert!(!out.byte_origin());
    let (s, d) = (Chain::of(&srgb), Chain::of(&p3));
    let want: Vec<_> = pixels.iter().map(|p| profile_oracle(&s, &d, *p)).collect();
    assert_within_one(&out.image().to_rgba16().ok().unwrap(), &want, "sRGB → P3");
    // A byte-origin payload loses its flag in any conversion.
    let bytes = decode(&u16_stream(
        2,
        1,
        srgb.bytes(),
        widened(&byte_pattern(2)),
        2,
    ))
    .unwrap();
    assert!(bytes.byte_origin());
    let out = bytes
        .converted(PrecisionStorage::Rgba16Unorm, &p3, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert!(!out.byte_origin());
}

#[test]
fn converted_f32_to_u16_follows_the_carried_tone_map() {
    let srgb = IccProfile::srgb();
    let chain = Chain::of(&srgb);
    let floats = f32_pattern(70);
    for (map, tone) in [
        (ToneMap::Clip, (0u32, 4.0f32)),
        (ToneMap::Reinhard { white: 2.5 }, (1, 2.5)),
    ] {
        let payload = decode(&f32_stream(10, 7, srgb.bytes(), floats.clone(), tone)).unwrap();
        let out = payload
            .converted(PrecisionStorage::Rgba16Unorm, &srgb, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert_eq!(out.storage(), PrecisionStorage::Rgba16Unorm);
        assert_eq!(out.tone_map(), None);
        let want: Vec<_> = floats
            .iter()
            .map(|p| float_oracle(&chain, map, *p))
            .collect();
        assert_within_one(&out.image().to_rgba16().ok().unwrap(), &want, map.name());
    }
    // The map is the PAYLOAD's: Reinhard and Clip disagree above white.
    let clip = decode(&f32_stream(
        1,
        1,
        srgb.bytes(),
        vec![[1.7, 1.7, 1.7, 1.0]],
        (0, 4.0),
    ))
    .unwrap()
    .converted(PrecisionStorage::Rgba16Unorm, &srgb, LIMIT, 0, None)
    .unwrap()
    .unwrap();
    let reinhard = decode(&f32_stream(
        1,
        1,
        srgb.bytes(),
        vec![[1.7, 1.7, 1.7, 1.0]],
        (1, 2.5),
    ))
    .unwrap()
    .converted(PrecisionStorage::Rgba16Unorm, &srgb, LIMIT, 0, None)
    .unwrap()
    .unwrap();
    assert_eq!(clip.image().to_rgba16().ok().unwrap()[0][0], 65535);
    assert!(reinhard.image().to_rgba16().ok().unwrap()[0][0] < 65535);
}

#[test]
fn converted_f32_p3_to_u16_srgb_is_depth_then_profile() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let (s, d) = (Chain::of(&p3), Chain::of(&srgb));
    let map = ToneMap::Reinhard { white: 2.5 };
    let floats = f32_pattern(60);
    let payload = decode(&f32_stream(10, 6, p3.bytes(), floats.clone(), (1, 2.5))).unwrap();
    let out = payload
        .converted(PrecisionStorage::Rgba16Unorm, &srgb, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(&**out.image().profile().bytes(), &srgb.bytes()[..]);
    // The reference rounds to u16 BETWEEN the two stages, as the rule says.
    let want: Vec<_> = floats
        .iter()
        .map(|p| profile_oracle(&s, &d, float_oracle(&s, map, *p)))
        .collect();
    assert_within_one(
        &out.image().to_rgba16().ok().unwrap(),
        &want,
        "P3 f32 → sRGB u16",
    );
}

#[test]
fn converted_promotes_into_float_and_answers_none_when_equal() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    // A float target promotes a u16 payload and converts an f32 one
    // between profiles (the values are `precision_clipboard_promote`'s
    // tests; here only the resolution order).
    let u16s = decode(&u16_stream(1, 1, srgb.bytes(), vec![[1, 2, 3, 4]], 0)).unwrap();
    let promoted = u16s
        .converted(PrecisionStorage::Rgba32Float, &srgb, LIMIT, 0, None)
        .unwrap()
        .expect("a promotion");
    assert_eq!(promoted.storage(), PrecisionStorage::Rgba32Float);
    let floats = decode(&f32_stream(1, 1, p3.bytes(), vec![[0.5; 4]], (0, 4.0))).unwrap();
    let moved = floats
        .converted(PrecisionStorage::Rgba32Float, &srgb, LIMIT, 0, None)
        .unwrap()
        .expect("a float conversion between profiles");
    assert_eq!(&**moved.image().profile().bytes(), &srgb.bytes()[..]);
    assert!(u16s
        .converted(PrecisionStorage::Rgba16Unorm, &srgb, LIMIT, 0, None)
        .unwrap()
        .is_none());
    assert!(floats
        .converted(PrecisionStorage::Rgba32Float, &p3, LIMIT, 0, None)
        .unwrap()
        .is_none());
    // Admitted before it allocates: 2·payload of the target.
    let big = decode(&u16_stream(400, 400, srgb.bytes(), u16_pattern(160_000), 0)).unwrap();
    let err = big
        .converted(PrecisionStorage::Rgba16Unorm, &p3, 2 * 160_000 * 8, 0, None)
        .err()
        .unwrap();
    assert!(err.contains("working limit"), "{err}");
}

// ----------------------------------------------------------------- paste --

#[test]
fn paste_converts_every_mismatch_into_the_document() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let doc = u16_doc(6, 5, srgb.clone());
    let p3_payload = decode(&u16_stream(3, 3, p3.bytes(), u16_pattern(9), 0)).unwrap();
    // Another profile converts by default (no refusal, no arm).
    let landed = doc
        .clipboard_paste(
            &p3_payload,
            &paste_request("above", "centered", "convert"),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    let want = p3_payload
        .converted(PrecisionStorage::Rgba16Unorm, &srgb, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let record = landed.layer(1).unwrap();
    assert_eq!(
        record.source().unwrap().to_rgba16().ok().unwrap(),
        want.image().to_rgba16().ok().unwrap()
    );
    assert_eq!(record.offset(), (1, 1));
    // Preserve keeps the numbers and relabels them.
    let kept = doc
        .clipboard_paste(
            &p3_payload,
            &paste_request("above", "centered", "preserve"),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    let source = kept.layer(1).unwrap().source().unwrap();
    assert_eq!(&source.to_rgba16().ok().unwrap()[..], &u16_pattern(9)[..]);
    assert_eq!(&**source.profile().bytes(), &srgb.bytes()[..]);
    // A float payload into 16 bits converts under its tone map.
    let float = decode(&f32_stream(2, 2, srgb.bytes(), f32_pattern(4), (1, 2.5))).unwrap();
    let landed = doc
        .clipboard_paste(
            &float,
            &paste_request("top", "in_place", "convert"),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    let want = float
        .converted(PrecisionStorage::Rgba16Unorm, &srgb, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        landed
            .layer(1)
            .unwrap()
            .source()
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap(),
        want.image().to_rgba16().ok().unwrap()
    );
    // A byte-origin payload of the document's profile is exact.
    let flat: Vec<u8> = byte_pattern(4).into_iter().flatten().collect();
    let bytes = ClipboardPayload::from_rgba8(
        &flat,
        2,
        2,
        srgb.bytes(),
        (0, 0),
        Resolution::default(),
        LIMIT,
        0,
    )
    .unwrap();
    let landed = doc
        .clipboard_paste(
            &bytes,
            &paste_request("above", "in_place", "convert"),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    assert_eq!(
        landed
            .layer(1)
            .unwrap()
            .source()
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap(),
        &widened(&byte_pattern(4))[..]
    );
    // Float target: a u16 payload is promoted, a P3 float payload converts.
    let float_target = float_doc(4, 4, srgb.bytes(), ToneMap::Clip);
    let u16s = decode(&u16_stream(1, 1, srgb.bytes(), vec![[1, 2, 3, 4]], 0)).unwrap();
    let landed = float_target
        .clipboard_paste(
            &u16s,
            &paste_request("above", "centered", "convert"),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    assert_eq!(landed.layer_count(), 2);
    assert_eq!(
        landed.layer(1).unwrap().source().unwrap().storage(),
        PrecisionStorage::Rgba32Float
    );
    let p3_float = decode(&f32_stream(1, 1, p3.bytes(), vec![[0.5; 4]], (0, 4.0))).unwrap();
    let landed = float_target
        .clipboard_paste(
            &p3_float,
            &paste_request("above", "centered", "convert"),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    let want = p3_float
        .converted(PrecisionStorage::Rgba32Float, &srgb, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        landed.layer(1).unwrap().source().unwrap().to_rgba32(),
        want.image().to_rgba32()
    );
}

// ------------------------------------------------------------- float cut --

#[test]
fn float_cut_erases_alpha_against_its_own_oracle() {
    let srgb = IccProfile::srgb();
    let (w, h) = (7u32, 5u32);
    let map = ToneMap::Reinhard { white: 2.5 };
    let doc = float_doc(w, h, srgb.bytes(), map);
    assert_eq!(doc.tone_map(), Some(map));
    let before = doc.layer_source(0).unwrap().to_rgba32().unwrap();
    let coverage: Vec<u16> = (0..w * h).map(|i| COVERAGE[i as usize % 5]).collect();
    let (payload, next) = doc
        .clipboard_cut(0, 0, 0, w, h, Some(&coverage), LIMIT, 0, None)
        .unwrap();
    assert_eq!(payload.tone_map(), Some(map), "the payload carries the map");
    let copied = doc
        .clipboard_copy(
            &serde_json::from_value::<ClipboardRead>(
                json!({"op":"copy","layer":0,"x":0,"y":0,"width":w,"height":h}),
            )
            .unwrap(),
            Some(&coverage),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    assert_eq!(payload.encode().unwrap(), copied.encode().unwrap());
    let next = next.expect("the erase changed alpha");
    let after = next.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    for (i, (a, b)) in after.iter().zip(&before).enumerate() {
        for c in 0..3 {
            assert_eq!(a[c].to_bits(), b[c].to_bits(), "RGB {i}/{c} is retained");
        }
        let cover = coverage[i];
        let want = f64::from(b[3]) * (1.0 - f64::from(cover) / 65535.0);
        let got = f64::from(a[3]);
        // Relative to the SOURCE alpha: `1 − c/65535` cancels in f32 near
        // c = 65534, so a result-relative bound would measure that
        // cancellation rather than the erase.
        assert!(
            (got - want).abs() <= 1e-6 * f64::from(b[3]),
            "alpha {i}: {got} vs {want}"
        );
        // The test's own per-pixel loop, in f32.
        let loop_alpha = if cover == 0 {
            b[3]
        } else {
            b[3] * (1.0 - f32::from(cover) / 65535.0)
        };
        assert_eq!(
            a[3].to_bits(),
            loop_alpha.to_bits(),
            "alpha {i} per-pixel loop"
        );
    }
    assert!(next.is_float());
    assert_eq!(next.tone_map(), Some(map));
    // Full coverage when there is no selection.
    let (_, cleared) = doc
        .clipboard_cut(0, 0, 0, w, h, None, LIMIT, 0, None)
        .unwrap();
    assert!(cleared
        .unwrap()
        .layer_source(0)
        .unwrap()
        .to_rgba32()
        .ok()
        .unwrap()
        .iter()
        .all(|p| p[3] == 0.0));
    // Locks.
    let locked = doc.with_layer_locks(0, LOCK_PIXELS).unwrap().unwrap();
    let err = locked
        .clipboard_cut(0, 0, 0, w, h, Some(&coverage), LIMIT, 0, None)
        .err()
        .unwrap();
    assert!(err.contains("locked (pixels)"), "{err}");
    let frozen = doc.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let (payload, next) = frozen
        .clipboard_cut(0, 0, 0, w, h, Some(&coverage), LIMIT, 0, None)
        .unwrap();
    assert!(next.is_none());
    assert_eq!(payload.encode().unwrap(), copied.encode().unwrap());
}

// ------------------------------------------------------- new from clipboard --

#[test]
fn from_clipboard_builds_the_payloads_own_document() {
    let p3 = IccProfile::display_p3();
    let pixels = u16_pattern(12);
    let stream = drcb(
        2,
        4,
        3,
        (9, -4),
        (300.0, 300.0),
        p3.bytes(),
        &Samples::U16(pixels.clone()),
        0,
        (0, 4.0),
        None,
    );
    let doc = PrecisionDocument::from_clipboard(&decode(&stream).unwrap(), LIMIT, 0, None).unwrap();
    assert_eq!(doc.dimensions(), (4, 3));
    assert!(!doc.is_float());
    assert_eq!(doc.tone_map(), None);
    assert_eq!(&**doc.profile().bytes(), &p3.bytes()[..]);
    assert_eq!(doc.resolution().x, 300.0);
    assert_eq!(doc.resolution().y, 300.0);
    assert_eq!(doc.layer_count(), 1);
    let record = doc.layer(0).unwrap();
    assert_eq!(record.name(), "Background");
    assert_eq!(record.offset(), (0, 0), "the payload's offset is dropped");
    assert_eq!(
        record.source().unwrap().to_rgba16().ok().unwrap(),
        &pixels[..]
    );

    let floats = f32_pattern(6);
    let stream = drcb(
        2,
        3,
        2,
        (0, 0),
        (240.0, 120.0),
        p3.bytes(),
        &Samples::F32(floats.clone()),
        0,
        (1, 2.5),
        None,
    );
    let doc = PrecisionDocument::from_clipboard(&decode(&stream).unwrap(), LIMIT, 0, None).unwrap();
    assert!(doc.is_float());
    assert_eq!(doc.tone_map(), Some(ToneMap::Reinhard { white: 2.5 }));
    assert_eq!(doc.resolution().x, 240.0);
    assert_eq!(doc.resolution().y, 120.0);
    assert_eq!(&**doc.profile().bytes(), &p3.bytes()[..]);
    let got = doc.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    for (a, b) in got.iter().zip(&floats) {
        for c in 0..4 {
            assert_eq!(a[c].to_bits(), b[c].to_bits());
        }
    }
}

// ------------------------------------------------------------------- FFI --

#[test]
fn ffi_new_exports_round_trip_and_tolerate_null() {
    let srgb = IccProfile::srgb();
    let (w, h) = (3u32, 2u32);
    let flat: Vec<u8> = byte_pattern(w * h).into_iter().flatten().collect();
    unsafe {
        let mut err = ptr::null_mut();
        let handle = dr_clipboard_payload_from_rgba8(
            flat.as_ptr(),
            flat.len(),
            w,
            h,
            srgb.bytes().as_ptr(),
            srgb.bytes().len(),
            2,
            -1,
            300.0,
            300.0,
            LIMIT,
            0,
            &mut err,
        );
        assert_eq!(take_error(err), None);
        assert!(!handle.is_null());
        let mut err = ptr::null_mut();
        let info = dr_clipboard_payload_info(handle, &mut err);
        assert_eq!(take_error(err), None);
        let text = CStr::from_ptr(info).to_string_lossy().into_owned();
        dr_string_free(info);
        let info: Value = serde_json::from_str(&text).expect("info is JSON");
        assert_eq!(info["origin_depth"], 8);
        assert_eq!(info["offset"], json!([2, -1]));
        let mut err = ptr::null_mut();
        let image = dr_clipboard_payload_rgba8(
            handle,
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        assert_eq!(take_error(err), None);
        assert_eq!((dr_image_width(image), dr_image_height(image)), (w, h));
        let pixels = std::slice::from_raw_parts(dr_image_pixels_rgba(image), flat.len());
        assert_eq!(pixels, &flat[..]);
        dr_image_free(image);
        let mut err = ptr::null_mut();
        let doc = dr_precision_doc_from_clipboard(handle, LIMIT, 0, ptr::null(), &mut err);
        assert_eq!(take_error(err), None);
        assert!(!doc.is_null());
        assert_eq!((*doc).dimensions(), (w, h));
        assert_eq!((*doc).resolution().x, 300.0);
        dr_precision_doc_free(doc);
        dr_clipboard_payload_free(handle);

        // The NULL sweep: each answers NULL with a message.
        let mut err = ptr::null_mut();
        assert!(dr_clipboard_payload_from_rgba8(
            ptr::null(),
            0,
            w,
            h,
            srgb.bytes().as_ptr(),
            srgb.bytes().len(),
            0,
            0,
            72.0,
            72.0,
            LIMIT,
            0,
            &mut err
        )
        .is_null());
        assert!(take_error(err).is_some());
        let mut err = ptr::null_mut();
        assert!(dr_clipboard_payload_from_rgba8(
            flat.as_ptr(),
            flat.len(),
            w,
            h,
            ptr::null(),
            0,
            0,
            0,
            72.0,
            72.0,
            LIMIT,
            0,
            &mut err
        )
        .is_null());
        assert!(take_error(err).is_some());
        assert!(dr_clipboard_payload_from_rgba8(
            ptr::null(),
            0,
            0,
            0,
            ptr::null(),
            0,
            0,
            0,
            0.0,
            0.0,
            LIMIT,
            0,
            ptr::null_mut()
        )
        .is_null());
        let mut err = ptr::null_mut();
        assert!(dr_clipboard_payload_rgba8(
            ptr::null(),
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(take_error(err).is_some());
        assert!(dr_clipboard_payload_rgba8(
            ptr::null(),
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        let mut err = ptr::null_mut();
        assert!(
            dr_precision_doc_from_clipboard(ptr::null(), LIMIT, 0, ptr::null(), &mut err).is_null()
        );
        assert!(take_error(err).is_some());
        assert!(dr_precision_doc_from_clipboard(
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        // A float payload reads through its tone map across the FFI too, and
        // bytes that are not a profile refuse by name.
        let float: *const DrClipboardPayload = {
            let bytes = f32_stream(1, 1, srgb.bytes(), vec![[0.5; 4]], (0, 4.0));
            let mut err = ptr::null_mut();
            let handle = doctored_core::ffi_clipboard::dr_clipboard_payload_decode(
                bytes.as_ptr(),
                bytes.len(),
                LIMIT,
                0,
                &mut err,
            );
            assert_eq!(take_error(err), None);
            handle
        };
        let mut err = ptr::null_mut();
        let image = dr_clipboard_payload_rgba8(
            float,
            ptr::null(),
            0,
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        );
        assert_eq!(take_error(err), None);
        assert!(!image.is_null());
        dr_image_free(image);
        let junk = [1u8, 2, 3];
        let mut err = ptr::null_mut();
        assert!(dr_clipboard_payload_rgba8(
            float,
            junk.as_ptr(),
            junk.len(),
            false,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(take_error(err).unwrap().contains("profile"));
        dr_clipboard_payload_free(float as *mut _);
    }
}
