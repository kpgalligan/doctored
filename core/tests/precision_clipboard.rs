//! Phase 12G-1: the TYPED clipboard — `precision_clipboard` and its FFI.
//!
//! The oracles are written here and never read off the implementation: an
//! independent RZCB encoder/reader, a per-pixel window loop for Copy, the
//! test's own f64 straight source-over for Copy Merged, and `paint(Erase)`
//! for Cut's document half.
//!
//! Tolerances, fixed before anything was measured: the codec, Copy, Cut and
//! Paste are EXACT (bit for bit); Copy Merged is within 1 code at u16 and
//! 1e-5 relative at float, because the projection is computed in f32 and the
//! oracle in f64.

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_clipboard::{
    dr_clipboard_payload_bytes, dr_clipboard_payload_decode, dr_clipboard_payload_free,
    dr_clipboard_payload_image, dr_clipboard_payload_info, dr_precision_doc_clipboard_copy,
    dr_precision_doc_clipboard_cut, dr_precision_doc_clipboard_paste, DrClipboardPayload,
};
use doctored_core::ffi_precision::{dr_precision_doc_free, dr_precision_image_free};
use doctored_core::icc::IccProfile;
use doctored_core::layer_insert::InsertPosition;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{
    host_stage_bytes, ClipboardPayload, ClipboardRead, PastePlacement, PasteRequest,
};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_paint::PaintEdit;
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";
const BOOKKEEPING: u64 = 1024 * 1024;

const LOCK_TRANSPARENCY: u32 = 1;
const LOCK_PIXELS: u32 = 2;

/// The five coverage codes the spec names: the endpoints, one code either
/// side of them, and the midpoint.
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

fn u16_image(w: u32, h: u32, pixels: Vec<[u16; 4]>, profile: Arc<IccProfile>) -> PrecisionImage {
    PrecisionImage::from_rgba16(w, h, pixels, profile, None).expect("image")
}

/// A deterministic u16 pattern with sub-byte codes and alpha-0 latent RGB.
fn pattern(w: u32, h: u32, seed: u32) -> Vec<[u16; 4]> {
    let codes = [0x0101u16, 0x0100, 1, 65534, 0, 65535, 12345, 40000];
    (0..w * h)
        .map(|i| {
            let k = (i + seed) as usize;
            [
                codes[k % 8],
                codes[(k + 3) % 8],
                codes[(k + 5) % 8],
                codes[(k * 7 + seed as usize) % 8],
            ]
        })
        .collect()
}

fn doc_with(
    w: u32,
    h: u32,
    layers: Vec<Layer>,
    resolution: Resolution,
    profile: Arc<IccProfile>,
) -> PrecisionDocument {
    PrecisionDocument::from_parts(w, h, profile, Metadata::default(), resolution, layers)
        .expect("document")
}

fn coverage_plane(w: u32, h: u32) -> Vec<u16> {
    (0..w * h).map(|i| COVERAGE[i as usize % 5]).collect()
}

fn copy(layer: usize, x: u32, y: u32, width: u32, height: u32) -> ClipboardRead {
    serde_json::from_value(
        json!({"op":"copy","layer":layer,"x":x,"y":y,"width":width,"height":height}),
    )
    .unwrap()
}

fn copy_merged(x: u32, y: u32, width: u32, height: u32) -> ClipboardRead {
    serde_json::from_value(json!({"op":"copy_merged","x":x,"y":y,"width":width,"height":height}))
        .unwrap()
}

fn paste_request(anchor: usize, position: &str, placement: &str) -> PasteRequest {
    serde_json::from_value(
        json!({"name":"Pasted","anchor":anchor,"position":position,"placement":placement}),
    )
    .unwrap()
}

fn float_fixture() -> Option<PrecisionDocument> {
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return None;
    }
    Some(PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).expect("fixture"))
}

// ------------------------------------------------ the independent codec --

enum Samples {
    U16(Vec<[u16; 4]>),
    F32(Vec<[f32; 4]>),
}

#[allow(clippy::too_many_arguments)]
fn drcb(
    w: u32,
    h: u32,
    offset: (i32, i32),
    ppi: (f32, f32),
    profile: &[u8],
    samples: &Samples,
    coverage: Option<&[u16]>,
) -> Vec<u8> {
    let mut out = b"RZCB".to_vec();
    out.extend(1u32.to_le_bytes());
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
    out.push(u8::from(coverage.is_some()));
    match samples {
        Samples::U16(p) => p.iter().flatten().for_each(|v| out.extend(v.to_le_bytes())),
        Samples::F32(p) => p.iter().flatten().for_each(|v| out.extend(v.to_le_bytes())),
    }
    if let Some(c) = coverage {
        c.iter().for_each(|v| out.extend(v.to_le_bytes()));
    }
    out
}

fn decode(bytes: &[u8]) -> Result<ClipboardPayload, String> {
    ClipboardPayload::decode(bytes, LIMIT, 0)
}

// ----------------------------------------------------------------- codec --

#[test]
fn codec_round_trips_u16_and_f32_bit_exactly() {
    let srgb = IccProfile::srgb();
    let profile = srgb.bytes().to_vec();
    let (w, h) = (3, 2);
    let u16s = vec![
        [0x0101, 0x0100, 1, 65534],
        [65535, 0, 0x0101, 0],
        [7, 0x0100, 65534, 1],
        [0, 0, 0, 0],
        [1, 2, 3, 4],
        [65535, 65535, 65535, 65535],
    ];
    let f32s = vec![
        [-0.25, 1.5, 65504.0, 1.0],
        [-65504.0, 0.0, 0.5, 0.0],
        [3.0e-8, -1.0e-3, 12.0, 0.25],
        [0.0, 0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0, 1.0],
        [-2.0, 4.0, -8.0, 0.75],
    ];
    let coverage = [0u16, 1, 32768, 65534, 65535, 0x0101];
    for samples in [Samples::U16(u16s.clone()), Samples::F32(f32s.clone())] {
        for cov in [None, Some(&coverage[..])] {
            for (offset, ppi) in [((-5, -7), (300.0, 72.0)), ((4, 0), (72.0, 300.0))] {
                let bytes = drcb(w, h, offset, ppi, &profile, &samples, cov);
                let payload = decode(&bytes).expect("decodes");
                assert_eq!(payload.dimensions(), (w, h));
                assert_eq!(payload.offset(), offset);
                assert_eq!(payload.resolution().x, ppi.0);
                assert_eq!(payload.resolution().y, ppi.1);
                assert_eq!(payload.coverage(), cov);
                assert_eq!(&**payload.image().profile().bytes(), &profile[..]);
                match &samples {
                    Samples::U16(p) => {
                        assert_eq!(payload.storage(), PrecisionStorage::Rgba16Unorm);
                        assert_eq!(payload.image().to_rgba16().ok().unwrap(), &p[..]);
                    }
                    Samples::F32(p) => {
                        assert_eq!(payload.storage(), PrecisionStorage::Rgba32Float);
                        let got = payload.image().to_rgba32().ok().unwrap();
                        for (a, b) in got.iter().zip(p) {
                            for c in 0..4 {
                                assert_eq!(a[c].to_bits(), b[c].to_bits());
                            }
                        }
                    }
                }
                let bps = if matches!(samples, Samples::U16(_)) {
                    8
                } else {
                    16
                };
                assert_eq!(
                    payload.payload_bytes(),
                    6 * bps + if cov.is_some() { 12 } else { 0 }
                );
                // The encoder writes v3: the v1 stream with version 3, the
                // two tone-map words (clip; white 4) after the flag byte and
                // a zero editable-length word after them.
                let mut v3 = bytes.clone();
                v3[4] = 3;
                let at = 40 + profile.len() + 1;
                let mut words = 0u32.to_le_bytes().to_vec();
                words.extend(4.0f32.to_le_bytes());
                words.extend(0u32.to_le_bytes());
                v3.splice(at..at, words);
                assert_eq!(payload.encode().unwrap(), v3, "re-encode is the v3 twin");
                let info = payload.info();
                assert_eq!(info["width"], 3);
                assert_eq!(info["offset"], json!([offset.0, offset.1]));
                assert_eq!(info["has_coverage"], cov.is_some());
            }
        }
    }
}

#[test]
fn codec_refuses_malformed_streams() {
    let profile = IccProfile::srgb().bytes().to_vec();
    let good = drcb(
        2,
        1,
        (0, 0),
        (72.0, 72.0),
        &profile,
        &Samples::U16(vec![[1, 2, 3, 4], [5, 6, 7, 8]]),
        None,
    );
    decode(&good).expect("the control decodes");
    let mut magic = good.clone();
    magic[0] = b'X';
    assert!(decode(&magic).err().unwrap().contains("RZCB"));
    let mut version = good.clone();
    version[4] = 4;
    assert!(decode(&version).err().unwrap().contains("version"));
    let mut storage = good.clone();
    storage[8] = 3;
    assert!(decode(&storage).err().unwrap().contains("storage"));
    let mut empty = good.clone();
    empty[12..16].copy_from_slice(&0u32.to_le_bytes());
    assert!(decode(&empty).is_err());
    assert!(decode(&good[..good.len() - 1]).is_err(), "truncated");
    let mut long = good.clone();
    long.push(0);
    assert!(decode(&long).err().unwrap().contains("declares"));
    let nan = drcb(
        1,
        1,
        (0, 0),
        (72.0, 72.0),
        &profile,
        &Samples::F32(vec![[f32::NAN, 0.0, 0.0, 1.0]]),
        None,
    );
    assert!(decode(&nan).err().unwrap().contains("float sample"));
    let alpha = drcb(
        1,
        1,
        (0, 0),
        (72.0, 72.0),
        &profile,
        &Samples::F32(vec![[0.0, 0.0, 0.0, 1.5]]),
        None,
    );
    assert!(decode(&alpha).is_err());
    let garbage = vec![0x55u8; 200];
    let bad_profile = drcb(
        1,
        1,
        (0, 0),
        (72.0, 72.0),
        &garbage,
        &Samples::U16(vec![[0; 4]]),
        None,
    );
    assert!(decode(&bad_profile).err().unwrap().contains("profile"));
    let bad_ppi = drcb(
        1,
        1,
        (0, 0),
        (f32::NAN, 72.0),
        &profile,
        &Samples::U16(vec![[0; 4]]),
        None,
    );
    assert!(decode(&bad_ppi).err().unwrap().contains("resolution"));
    // Admission before the samples: a limit below the payload refuses.
    assert!(ClipboardPayload::decode(&good, BOOKKEEPING, 0)
        .err()
        .unwrap()
        .contains("working limit"));
}

// ------------------------------------------------------------------ copy --

#[test]
fn copy_reads_raw_samples_through_the_offset_under_coverage() {
    let (cw, ch) = (8u32, 6u32);
    let (lw, lh) = (7u32, 5u32);
    let offset = (3i32, -2i32);
    let srgb = IccProfile::srgb();
    let samples = pattern(lw, lh, 1);
    let layer = Layer::raster(
        "L",
        u16_image(lw, lh, samples.clone(), srgb.clone()),
        offset,
        0.5,
        true,
    )
    .unwrap();
    let doc = doc_with(cw, ch, vec![layer], Resolution { x: 300.0, y: 72.0 }, srgb);
    let coverage = coverage_plane(cw, ch);
    for (x, y, w, h) in [(0, 0, cw, ch), (2, 1, 5, 4), (7, 5, 1, 1)] {
        for cov in [None, Some(&coverage[..])] {
            let payload = doc
                .clipboard_copy(&copy(0, x, y, w, h), cov, LIMIT, 0, None)
                .expect("copies");
            assert_eq!(payload.offset(), (x as i32, y as i32));
            assert_eq!(payload.resolution().x, 300.0);
            assert_eq!(payload.resolution().y, 72.0);
            let got = payload.image().to_rgba16().ok().unwrap();
            let mut expected_cov = Vec::new();
            for py in 0..h {
                for px in 0..w {
                    let (cx, cy) = (i64::from(x + px), i64::from(y + py));
                    let (lx, ly) = (cx - i64::from(offset.0), cy - i64::from(offset.1));
                    let mut want = if lx >= 0 && ly >= 0 && lx < i64::from(lw) && ly < i64::from(lh)
                    {
                        samples[(ly * i64::from(lw) + lx) as usize]
                    } else {
                        [0; 4]
                    };
                    let c = cov.map_or(65535u16, |c| c[(cy * i64::from(cw) + cx) as usize]);
                    expected_cov.push(c);
                    // Independent rounding: nearest integer of a·c/65535 in f64.
                    want[3] = ((f64::from(want[3]) * f64::from(c)) / 65535.0).round() as u16;
                    assert_eq!(
                        got[(py * w + px) as usize],
                        want,
                        "({px},{py}) window {x},{y}"
                    );
                }
            }
            assert_eq!(
                payload.coverage().map(<[u16]>::to_vec),
                cov.map(|_| expected_cov)
            );
        }
    }
    // Latent RGB at alpha 0 is retained exactly.
    let latent = doc_with(
        2,
        1,
        vec![Layer::raster(
            "L",
            u16_image(
                2,
                1,
                vec![[0x0101, 0x0100, 1, 0], [9, 8, 7, 65535]],
                IccProfile::srgb(),
            ),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()],
        Resolution::default(),
        IccProfile::srgb(),
    );
    let payload = latent
        .clipboard_copy(&copy(0, 0, 0, 2, 1), None, LIMIT, 0, None)
        .unwrap();
    assert_eq!(
        payload.image().to_rgba16().ok().unwrap()[0],
        [0x0101, 0x0100, 1, 0]
    );
    assert!(payload.coverage().is_none());
}

#[test]
fn copy_refuses_adjustments_and_bad_windows() {
    let srgb = IccProfile::srgb();
    let leaf = Layer::raster(
        "L",
        u16_image(2, 2, pattern(2, 2, 0), srgb.clone()),
        (0, 0),
        1.0,
        true,
    )
    .unwrap();
    let group = Layer::isolated_normal("G", vec![leaf], 1.0, true).unwrap();
    let doc = doc_with(2, 2, vec![group], Resolution::default(), srgb);
    let description =
        json!({"type":"adjust","evaluation":"precision_v1","op":"invert","params":{}}).to_string();
    let doc = doc
        .add_adjustment("Fx", &description, 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let err = doc
        .clipboard_copy(&copy(2, 0, 0, 2, 2), None, LIMIT, 0, None)
        .err()
        .unwrap();
    assert!(err.contains("adjustment"), "{err}");
    // Copy Merged of the same stack is admitted.
    doc.clipboard_copy(&copy_merged(0, 0, 2, 2), None, LIMIT, 0, None)
        .expect("copy merged reads the projection");
    for (x, y, w, h) in [(0, 0, 0, 2), (1, 0, 2, 2), (0, 2, 1, 1)] {
        assert!(doc
            .clipboard_copy(&copy(0, x, y, w, h), None, LIMIT, 0, None)
            .is_err());
    }
    assert!(doc
        .clipboard_copy(&copy(0, 0, 0, 2, 2), Some(&[0; 3]), LIMIT, 0, None)
        .is_err());
}

// ----------------------------------------------------------- copy merged --

/// Straight source-over in f64: top (colour, alpha·opacity) over bottom.
fn over(top: [f64; 4], top_opacity: f64, bottom: [f64; 4]) -> [f64; 4] {
    let at = top[3] * top_opacity;
    let ab = bottom[3];
    let ao = at + ab * (1.0 - at);
    if ao == 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = (top[c] * at + bottom[c] * ab * (1.0 - at)) / ao;
    }
    out[3] = ao;
    out
}

#[test]
fn copy_merged_matches_source_over_at_u16() {
    let (w, h) = (6u32, 4u32);
    let srgb = IccProfile::srgb();
    let bottom = pattern(w, h, 2);
    let top = pattern(w, h, 5);
    let doc = doc_with(
        w,
        h,
        vec![
            Layer::raster(
                "B",
                u16_image(w, h, bottom.clone(), srgb.clone()),
                (0, 0),
                1.0,
                true,
            )
            .unwrap(),
            Layer::raster(
                "T",
                u16_image(w, h, top.clone(), srgb.clone()),
                (0, 0),
                0.6,
                true,
            )
            .unwrap(),
        ],
        Resolution::default(),
        srgb,
    );
    let coverage = coverage_plane(w, h);
    let payload = doc
        .clipboard_copy(&copy_merged(1, 1, 4, 3), Some(&coverage), LIMIT, 0, None)
        .unwrap();
    let got = payload.image().to_rgba16().ok().unwrap();
    let f = |p: [u16; 4]| p.map(|v| f64::from(v) / 65535.0);
    for py in 0..3u32 {
        for px in 0..4u32 {
            let i = ((py + 1) * w + px + 1) as usize;
            let want = over(f(top[i]), 0.6, f(bottom[i]));
            let pixel = got[(py * 4 + px) as usize];
            let c = f64::from(coverage[i]) / 65535.0;
            let alpha = want[3] * c * 65535.0;
            assert!(
                (f64::from(pixel[3]) - alpha).abs() <= 1.0,
                "alpha {} vs {alpha}",
                pixel[3]
            );
            if want[3] > 0.0 {
                for ch in 0..3 {
                    let v = want[ch] * 65535.0;
                    assert!(
                        (f64::from(pixel[ch]) - v).abs() <= 1.0,
                        "({px},{py}) ch{ch} {} vs {v}",
                        pixel[ch]
                    );
                }
            }
        }
    }
}

#[test]
fn copy_merged_matches_source_over_at_float() {
    let Some(fixture) = float_fixture() else {
        return;
    };
    let (w, h) = fixture.dimensions();
    let profile = fixture.profile().bytes().to_vec();
    let top: Vec<[f32; 4]> = (0..w * h)
        .map(|i| {
            let k = i as f32;
            [
                (k * 0.37).sin() * 3.0,
                -0.5 + (k * 0.11).cos(),
                2.5 + (k * 0.05).sin(),
                ((i % 7) as f32) / 6.0,
            ]
        })
        .collect();
    let bytes = drcb(
        w,
        h,
        (0, 0),
        (72.0, 72.0),
        &profile,
        &Samples::F32(top.clone()),
        None,
    );
    let payload = decode(&bytes).unwrap();
    let anchor = fixture.layer_count() - 1;
    let doc = fixture
        .clipboard_paste(
            &payload,
            &paste_request(anchor, "above", "in_place"),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    let doc = doc
        .set_layer_opacity(doc.layer_count() - 1, 0.6)
        .unwrap()
        .unwrap();
    assert_eq!(fixture.layer_count(), 1, "the fixture is one raster");
    let bottom = fixture.layer_source(0).unwrap().to_rgba32().ok().unwrap();
    assert_eq!(fixture.layer(0).unwrap().offset(), (0, 0));
    let merged = doc
        .clipboard_copy(&copy_merged(0, 0, w, h), None, LIMIT, 0, None)
        .unwrap();
    let got = merged.image().to_rgba32().ok().unwrap();
    let f = |p: [f32; 4]| p.map(f64::from);
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-5 * b.abs().max(1.0);
    for i in 0..(w * h) as usize {
        let want = over(f(top[i]), 0.6, f(bottom[i]));
        assert!(close(f64::from(got[i][3]), want[3]), "alpha at {i}");
        if want[3] > 0.0 {
            for c in 0..3 {
                assert!(
                    close(f64::from(got[i][c]), want[c]),
                    "{i} ch{c}: {} vs {}",
                    got[i][c],
                    want[c]
                );
            }
        }
    }
}

// ------------------------------------------------------------- admission --

#[test]
fn every_read_is_admitted_before_it_allocates() {
    // 400×400 u16: payload 1.28 MB, so 2·payload is above the bookkeeping.
    let (w, h) = (400u32, 400u32);
    let srgb = IccProfile::srgb();
    let doc = doc_with(
        w,
        h,
        vec![Layer::raster(
            "L",
            u16_image(w, h, pattern(w, h, 3), srgb.clone()),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()],
        Resolution::default(),
        srgb,
    );
    let payload = u64::from(w * h) * 8;
    let need = 2 * payload;
    assert!(need > BOOKKEEPING);
    // Every read also admits the host's public-rendering stage.
    let host = host_stage_bytes(w, h, PrecisionStorage::Rgba16Unorm, false).unwrap();
    assert!(host >= 3 * payload, "the host stage holds three payloads");
    let copy_need = need.max(host);
    let read = copy(0, 0, 0, w, h);
    let err = doc
        .clipboard_copy(&read, None, copy_need - 1, 0, None)
        .err()
        .unwrap();
    assert!(err.contains("working limit"), "{err}");
    doc.clipboard_copy(&read, None, copy_need, 0, None)
        .expect("copy at max(2·payload, host stage)");

    // Copy Merged: the host stage, the render plus 2·payload, and the
    // renderer's own plan with the payload counted as other.
    let plan = doc.render_plan().unwrap();
    let render = u64::from(w * h) * 8;
    let merged_need = (render + need)
        .max(plan.unique_retained_bytes + plan.scratch_bytes + need)
        .max(host);
    let merged = copy_merged(0, 0, w, h);
    assert!(doc
        .clipboard_copy(&merged, None, (render + need).max(host) - 1, 0, None)
        .err()
        .unwrap()
        .contains("working limit"));
    assert!(doc
        .clipboard_copy(&merged, None, merged_need - 1, 0, None)
        .is_err());
    doc.clipboard_copy(&merged, None, merged_need, 0, None)
        .expect("copy merged at its need");

    // Cut: the copy's 2·payload, then the erase with 2·payload + the
    // full-coverage plane as other.
    let paint = doc
        .paint_plan(&PaintEdit::Erase {
            layer: 0,
            opacity: 1.0,
        })
        .unwrap();
    let plane = u64::from(w * h) * 2;
    let erase_need = paint.retained_bytes + paint.scratch_bytes + need + plane;
    let cut_need = copy_need.max(erase_need);
    assert!(doc
        .clipboard_cut(0, 0, 0, w, h, None, copy_need - 1, 0, None)
        .err()
        .unwrap()
        .contains("working limit"));
    assert!(doc
        .clipboard_cut(0, 0, 0, w, h, None, cut_need - 1, 0, None)
        .is_err());
    doc.clipboard_cut(0, 0, 0, w, h, None, cut_need, 0, None)
        .expect("cut at its need");
}

#[test]
fn copy_merged_needs_the_host_stage_and_names_it_in_its_allocation() {
    // A small canvas with a large covered window, so the host stage —
    // three covered payloads plus the public rendering — dominates the
    // render plus 2·payload, and Copy Merged's threshold is the stage.
    let (w, h) = (300u32, 200u32);
    let srgb = IccProfile::srgb();
    let doc = doc_with(
        w,
        h,
        vec![Layer::raster(
            "L",
            u16_image(w, h, pattern(w, h, 5), srgb.clone()),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()],
        Resolution::default(),
        srgb,
    );
    let coverage = vec![40000u16; (w * h) as usize];
    let pixels = u64::from(w * h);
    let payload = pixels * 8 + pixels * 2;
    let host = host_stage_bytes(w, h, PrecisionStorage::Rgba16Unorm, true).unwrap();
    assert!(host >= 3 * payload);
    let plan = doc.render_plan().unwrap();
    let core = (pixels * 8 + 2 * payload)
        .max(plan.unique_retained_bytes + plan.scratch_bytes + 2 * payload);
    let need = core.max(host);
    let merged = copy_merged(0, 0, w, h);
    assert!(doc
        .clipboard_copy(&merged, Some(&coverage), need - 1, 0, None)
        .err()
        .unwrap()
        .contains("working limit"));
    doc.clipboard_copy(&merged, Some(&coverage), need, 0, None)
        .expect("copy merged at max(core, host stage)");
    // The read's new memory names the host stage too.
    let allocation = doc.clipboard_read_allocation(&merged, true).unwrap();
    assert_eq!(
        allocation,
        host.max((pixels * 8 + 2 * payload).max(plan.scratch_bytes + 2 * payload))
    );
    assert!(allocation >= host);
}

#[test]
fn decode_is_admitted_at_twice_the_payload() {
    // 400×400 u16: 2·payload is above the bookkeeping, so the threshold is
    // the payload's own charge plus the ICC parse scratch.
    let (w, h) = (400u32, 400u32);
    let profile = IccProfile::srgb().bytes().to_vec();
    let bytes = drcb(
        w,
        h,
        (0, 0),
        (72.0, 72.0),
        &profile,
        &Samples::U16(pattern(w, h, 2)),
        None,
    );
    let need = 2 * u64::from(w * h) * 8 + BOOKKEEPING + profile.len() as u64 * 64;
    let err = ClipboardPayload::decode(&bytes, need - 1, 0).err().unwrap();
    assert!(err.contains("working limit"), "{err}");
    ClipboardPayload::decode(&bytes, need, 0).expect("decode at 2·payload");
}

// ------------------------------------------------------------------- cut --

#[test]
fn cut_is_copy_then_clear_and_honours_locks() {
    let (w, h) = (5u32, 4u32);
    let srgb = IccProfile::srgb();
    let samples: Vec<[u16; 4]> = pattern(w, h, 4)
        .into_iter()
        .map(|mut p| {
            p[3] = p[3].max(1000);
            p
        })
        .collect();
    let doc = doc_with(
        w,
        h,
        vec![Layer::raster(
            "L",
            u16_image(w, h, samples, srgb.clone()),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()],
        Resolution::default(),
        srgb,
    );
    let coverage = coverage_plane(w, h);
    let window = (1, 1, 3, 2);
    // The clear half is exactly paint(Erase) under the SAME coverage — the
    // window the host sends is the selection's bounds, and coverage outside
    // it is zero; here it is not, which proves the erase is Clear's.
    let (payload, next) = doc
        .clipboard_cut(
            0,
            window.0,
            window.1,
            window.2,
            window.3,
            Some(&coverage),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    let copied = doc
        .clipboard_copy(
            &copy(0, window.0, window.1, window.2, window.3),
            Some(&coverage),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    assert_eq!(payload.encode().unwrap(), copied.encode().unwrap());
    let erased = doc
        .paint(
            &PaintEdit::Erase {
                layer: 0,
                opacity: 1.0,
            },
            &coverage,
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        next.unwrap()
            .layer_source(0)
            .unwrap()
            .to_rgba16()
            .ok()
            .unwrap(),
        erased.layer_source(0).unwrap().to_rgba16().ok().unwrap()
    );
    // No coverage: a full-coverage erase.
    let (_, next) = doc
        .clipboard_cut(0, 0, 0, w, h, None, LIMIT, 0, None)
        .unwrap();
    assert!(next
        .unwrap()
        .layer_source(0)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap()
        .iter()
        .all(|p| p[3] == 0));

    // Pixels lock: refused by name before any payload exists.
    let locked = doc.with_layer_locks(0, LOCK_PIXELS).unwrap().unwrap();
    let err = locked
        .clipboard_cut(0, 1, 1, 3, 2, Some(&coverage), LIMIT, 0, None)
        .err()
        .unwrap();
    assert!(err.contains("locked (pixels)"), "{err}");

    // Transparency lock: the payload, and no document.
    let frozen = doc.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let (payload, next) = frozen
        .clipboard_cut(0, 1, 1, 3, 2, Some(&coverage), LIMIT, 0, None)
        .unwrap();
    assert!(next.is_none());
    let copied = frozen
        .clipboard_copy(&copy(0, 1, 1, 3, 2), Some(&coverage), LIMIT, 0, None)
        .unwrap();
    assert_eq!(payload.encode().unwrap(), copied.encode().unwrap());

    // Float Cut is admitted since 12G-2 (its oracle is in
    // `precision_clipboard_boundaries.rs`); the operator table says so.
    if let Some(fixture) = float_fixture() {
        fixture
            .clipboard_cut(0, 0, 0, 1, 1, None, LIMIT, 0, None)
            .expect("float cut");
        let table: Value =
            serde_json::from_str(&doctored_core::float_document::float_operators_json()).unwrap();
        assert!(!table["refused"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["operator"] == "clipboard_cut"));
        let admitted = table["admitted"].as_array().unwrap();
        assert!(admitted.contains(&json!("clipboard_paste")));
        assert!(admitted.contains(&json!("clipboard_cut")));
    }
}

// ----------------------------------------------------------------- paste --

#[test]
fn paste_places_the_payload_exactly_and_converts_mismatches() {
    let srgb = IccProfile::srgb();
    let (w, h) = (9u32, 7u32);
    let doc = doc_with(
        w,
        h,
        vec![Layer::raster(
            "B",
            u16_image(w, h, pattern(w, h, 0), srgb.clone()),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()],
        Resolution::default(),
        srgb.clone(),
    );
    let samples = pattern(4, 3, 6);
    let cov = [7u16; 12];
    let bytes = drcb(
        4,
        3,
        (-2, 5),
        (300.0, 72.0),
        srgb.bytes(),
        &Samples::U16(samples.clone()),
        Some(&cov),
    );
    let payload = decode(&bytes).unwrap();
    let placed = doc
        .clipboard_paste(
            &payload,
            &paste_request(0, "above", "in_place"),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    assert_eq!(placed.layer_count(), 2);
    let record = placed.layer(1).unwrap();
    assert_eq!(record.offset(), (-2, 5));
    assert_eq!(record.name(), "Pasted");
    assert_eq!(record.opacity(), 1.0);
    assert_eq!(record.layer_source_samples(), samples);
    let centered = doc
        .clipboard_paste(
            &payload,
            &paste_request(0, "top", "centered"),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    // Independent: floor((9-4)/2) = 2, floor((7-3)/2) = 2.
    assert_eq!(centered.layer(1).unwrap().offset(), (2, 2));
    // A payload larger than the canvas centres to a negative offset.
    let big = decode(&drcb(
        12,
        10,
        (0, 0),
        (72.0, 72.0),
        srgb.bytes(),
        &Samples::U16(pattern(12, 10, 1)),
        None,
    ))
    .unwrap();
    let placed = doc
        .clipboard_paste(&big, &paste_request(0, "above", "centered"), LIMIT, 0, None)
        .unwrap();
    assert_eq!(placed.layer(1).unwrap().offset(), (-2, -2));
    // Refusals.
    let err = doc
        .clipboard_paste(
            &payload,
            &paste_request(0, "into_group", "centered"),
            LIMIT,
            0,
            None,
        )
        .err()
        .unwrap();
    assert!(!err.is_empty());
    let p3 = doc_with(
        w,
        h,
        vec![Layer::raster(
            "B",
            u16_image(w, h, pattern(w, h, 0), IccProfile::display_p3()),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()],
        Resolution::default(),
        IccProfile::display_p3(),
    );
    // A mismatch converts instead of refusing — another profile, a
    // float payload into 16 bits, and a u16 payload into float.
    let converted = p3
        .clipboard_paste(
            &payload,
            &paste_request(0, "above", "centered"),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    assert_eq!(converted.layer_count(), 2);
    assert_eq!(
        &**converted.layer_source(1).unwrap().profile().bytes(),
        &IccProfile::display_p3().bytes()[..]
    );
    let float_payload = decode(&drcb(
        1,
        1,
        (0, 0),
        (72.0, 72.0),
        srgb.bytes(),
        &Samples::F32(vec![[2.0, -1.0, 0.5, 1.0]]),
        None,
    ))
    .unwrap();
    let mapped = doc
        .clipboard_paste(
            &float_payload,
            &paste_request(0, "above", "centered"),
            LIMIT,
            0,
            None,
        )
        .unwrap();
    assert_eq!(
        mapped.layer_source(1).unwrap().storage(),
        PrecisionStorage::Rgba16Unorm
    );
    if let Some(fixture) = float_fixture() {
        let payload16 = decode(&drcb(
            1,
            1,
            (0, 0),
            (72.0, 72.0),
            fixture.profile().bytes(),
            &Samples::U16(vec![[1, 2, 3, 4]]),
            None,
        ))
        .unwrap();
        let promoted = fixture
            .clipboard_paste(
                &payload16,
                &paste_request(0, "above", "centered"),
                LIMIT,
                0,
                None,
            )
            .unwrap();
        assert_eq!(
            promoted.layer_source(1).unwrap().storage(),
            PrecisionStorage::Rgba32Float
        );
        // Float → float is exact, negatives and above-white included.
        let payload32 = decode(&drcb(
            2,
            1,
            (1, 1),
            (72.0, 72.0),
            fixture.profile().bytes(),
            &Samples::F32(vec![[-3.5, 70.0, 0.125, 0.5], [1.0, 2.0, 3.0, 1.0]]),
            None,
        ))
        .unwrap();
        let placed = fixture
            .clipboard_paste(
                &payload32,
                &paste_request(0, "above", "in_place"),
                LIMIT,
                0,
                None,
            )
            .unwrap();
        assert_eq!(placed.layer(1).unwrap().offset(), (1, 1));
        assert_eq!(
            placed.layer_source(1).unwrap().to_rgba32().ok().unwrap(),
            &[[-3.5, 70.0, 0.125, 0.5], [1.0, 2.0, 3.0, 1.0]]
        );
    }
}

trait Samples16 {
    fn layer_source_samples(&self) -> Vec<[u16; 4]>;
}
impl Samples16 for Layer {
    fn layer_source_samples(&self) -> Vec<[u16; 4]> {
        self.source().unwrap().to_rgba16().unwrap()
    }
}

// ------------------------------------------------------------------- FFI --

fn ffi_copy(
    doc: &PrecisionDocument,
    request: Value,
    coverage: Option<&[u16]>,
) -> Result<*mut DrClipboardPayload, String> {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let (p, n) = coverage.map_or((ptr::null(), 0), |c| (c.as_ptr(), c.len()));
    let out = unsafe {
        dr_precision_doc_clipboard_copy(doc, text.as_ptr(), p, n, LIMIT, 0, ptr::null(), &mut err)
    };
    match take_error(err) {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

#[test]
fn ffi_round_trip_through_every_export() {
    let srgb = IccProfile::srgb();
    let (w, h) = (4u32, 3u32);
    let samples = pattern(w, h, 7);
    let doc = doc_with(
        w,
        h,
        vec![Layer::raster(
            "L",
            u16_image(w, h, samples.clone(), srgb.clone()),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()],
        Resolution { x: 300.0, y: 72.0 },
        srgb,
    );
    let coverage = coverage_plane(w, h);
    let handle = ffi_copy(
        &doc,
        json!({"op":"copy","layer":0,"x":1,"y":0,"width":3,"height":2}),
        Some(&coverage),
    )
    .unwrap();
    assert!(!handle.is_null());
    let mut length = 0usize;
    let bytes_ptr = unsafe { dr_clipboard_payload_bytes(handle, &mut length) };
    let bytes = unsafe { std::slice::from_raw_parts(bytes_ptr, length) }.to_vec();
    assert_eq!(&bytes[..4], b"RZCB");
    let mut err = ptr::null_mut();
    let decoded =
        unsafe { dr_clipboard_payload_decode(bytes.as_ptr(), bytes.len(), LIMIT, 0, &mut err) };
    assert!(take_error(err).is_none());
    let mut length2 = 0usize;
    let again = unsafe { dr_clipboard_payload_bytes(decoded, &mut length2) };
    assert_eq!(
        unsafe { std::slice::from_raw_parts(again, length2) },
        &bytes[..]
    );
    let mut err = ptr::null_mut();
    let info = unsafe { dr_clipboard_payload_info(decoded, &mut err) };
    assert!(take_error(err).is_none());
    let text = unsafe { CStr::from_ptr(info) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(info) };
    let info: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(info["storage"], "rgba16_unorm");
    assert_eq!(info["depth"], 16);
    assert_eq!(info["offset"], json!([1, 0]));
    assert_eq!(info["ppi"], json!([300.0, 72.0]));
    assert_eq!(info["has_coverage"], true);
    let image = unsafe { dr_clipboard_payload_image(decoded) };
    assert!(!image.is_null());
    assert_eq!(unsafe { &*image }.dimensions(), (3, 2));
    unsafe { dr_precision_image_free(image) };
    let request = CString::new(
        json!({"name":"P","anchor":0,"position":"above","placement":"in_place"}).to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let pasted = unsafe {
        dr_precision_doc_clipboard_paste(
            &doc,
            decoded,
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(take_error(err).is_none());
    assert!(!pasted.is_null());
    assert_eq!(unsafe { &*pasted }.layer(1).unwrap().offset(), (1, 0));
    unsafe { dr_precision_doc_free(pasted) };

    // The transparency-locked cut: NULL document, no error, payload set.
    let frozen = doc.with_layer_locks(0, LOCK_TRANSPARENCY).unwrap().unwrap();
    let cut =
        CString::new(json!({"op":"cut","layer":0,"x":0,"y":0,"width":4,"height":3}).to_string())
            .unwrap();
    let mut payload_out: *mut DrClipboardPayload = ptr::null_mut();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_clipboard_cut(
            &frozen,
            cut.as_ptr(),
            coverage.as_ptr(),
            coverage.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut payload_out,
            &mut err,
        )
    };
    assert!(result.is_null());
    assert!(take_error(err).is_none());
    assert!(!payload_out.is_null());
    unsafe { dr_clipboard_payload_free(payload_out) };
    // An unlocked cut answers both.
    let mut payload_out: *mut DrClipboardPayload = ptr::null_mut();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_clipboard_cut(
            &doc,
            cut.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut payload_out,
            &mut err,
        )
    };
    assert!(take_error(err).is_none());
    assert!(!result.is_null() && !payload_out.is_null());
    unsafe {
        dr_precision_doc_free(result);
        dr_clipboard_payload_free(payload_out);
    }
    // A refused cut leaves payload_out NULL.
    let locked = doc.with_layer_locks(0, LOCK_PIXELS).unwrap().unwrap();
    let mut payload_out: *mut DrClipboardPayload = ptr::null_mut();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_clipboard_cut(
            &locked,
            cut.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut payload_out,
            &mut err,
        )
    };
    assert!(result.is_null() && payload_out.is_null());
    assert!(take_error(err).unwrap().contains("locked"));
    // Coverage length must be the canvas.
    assert!(ffi_copy(
        &doc,
        json!({"op":"copy","layer":0,"x":0,"y":0,"width":1,"height":1}),
        Some(&[0; 5])
    )
    .is_err());
    assert!(ffi_copy(
        &doc,
        json!({"op":"copy","layer":0,"x":0,"y":0,"width":1,"height":1,"extra":1}),
        None
    )
    .is_err());
    unsafe {
        dr_clipboard_payload_free(decoded);
        dr_clipboard_payload_free(handle);
    }
}

#[test]
fn ffi_tolerates_null_inputs() {
    unsafe {
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_clipboard_copy(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(take_error(err).is_some());
        let mut out: *mut DrClipboardPayload = ptr::null_mut();
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_clipboard_cut(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut out,
            &mut err
        )
        .is_null());
        assert!(out.is_null());
        assert!(take_error(err).is_some());
        assert!(dr_precision_doc_clipboard_cut(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut()
        )
        .is_null());
        let mut err = ptr::null_mut();
        assert!(dr_precision_doc_clipboard_paste(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert!(take_error(err).is_some());
        let mut err = ptr::null_mut();
        assert!(dr_clipboard_payload_decode(ptr::null(), 0, LIMIT, 0, &mut err).is_null());
        assert!(take_error(err).is_some());
        let mut length = 99usize;
        assert!(dr_clipboard_payload_bytes(ptr::null(), &mut length).is_null());
        assert_eq!(length, 0);
        assert!(dr_clipboard_payload_bytes(ptr::null(), ptr::null_mut()).is_null());
        assert!(dr_clipboard_payload_info(ptr::null(), ptr::null_mut()).is_null());
        assert!(dr_clipboard_payload_image(ptr::null()).is_null());
        let mut length = 99usize;
        assert!(doctored_core::ffi_cmyk_clipboard::dr_clipboard_payload_icc(
            ptr::null(),
            &mut length
        )
        .is_null());
        assert_eq!(length, 0);
        let mut err = ptr::null_mut();
        let mut assigned = ptr::null_mut();
        assert!(
            doctored_core::ffi_cmyk_clipboard::dr_clipboard_payload_from_cmyk_image(
                ptr::null(),
                0,
                ptr::null(),
                0,
                LIMIT,
                0,
                &mut assigned,
                &mut err
            )
            .is_null()
        );
        assert!(take_error(err).is_some());
        assert!(assigned.is_null());
        dr_clipboard_payload_free(ptr::null_mut());
    }
    // A position the core does not know is refused by the parser.
    let _ = InsertPosition::Above;
    let _ = PastePlacement::Centered;
}
