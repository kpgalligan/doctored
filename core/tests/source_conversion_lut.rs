//! Convert to Profile to or from a LUT (A2B/B2A) profile on an 8-bit
//! document holding a retained source.
//!
//! The rule under test: relative colorimetric with BPC off converts, keeps
//! every binding, and re-renders each source through the same relative/no-BPC
//! pipeline the raster pixels use; perceptual, saturation, absolute and BPC
//! with a LUT side refuse with `settings_sentence`'s text; a typed document
//! still refuses a LUT destination at its colour plan. The LUT is the
//! synthetic identity profile `conversion_lut_destination.rs` builds (never an
//! installed profile, except in the ignored cost probe).
use crate::live_filter_support;
use doctored_core::doc::DrDocument;
use doctored_core::ffi_output_color::*;
use doctored_core::icc::IccProfile;
use doctored_core::icc_intent::{ColorPipeline, RenderingIntent};
use doctored_core::icc_lut::{ProofProfile, D50};
use image::RgbaImage;
use live_filter_support::*;
use serde_json::{json, Value};
use std::{ffi::CString, ptr, sync::Arc};

use crate::icc_lut_builders as builders;
use builders::{build, tag_desc, tag_mft2, tag_wtpt};

const W: u32 = 8;
const H: u32 = 6;
const SETTINGS_SOURCE: &str = "converting with these settings cannot keep 1 image source bound; convert at relative colorimetric with black point compensation off";
const HIGH_DEPTH: &str = "high-depth conversion requires an RGB matrix/TRC destination profile";
const INSTALLED_LUT: &str = "/Library/Application Support/Adobe/Color/Profiles/VideoHD16-235.icc";

/// The LUT-only profile (`conversion_lut_destination.rs`'s `lut_profile`).
fn lut_profile() -> Vec<u8> {
    build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic LUT RGB")),
            (b"wtpt", tag_wtpt(D50)),
            (b"A2B0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
            (b"B2A0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
        ],
    )
}

fn lut_arc() -> Arc<IccProfile> {
    Arc::new(IccProfile::parse(&lut_profile()).unwrap())
}

fn opaque(w: u32, h: u32) -> Vec<[u16; 4]> {
    (0..w * h)
        .map(|i| {
            let v = (i * 65535 / (w * h)) as u16;
            [v, 40000, 65535 - v, 65535]
        })
        .collect()
}

fn to8(v: u16) -> u8 {
    ((u32::from(v) + 128) / 257) as u8
}

/// The 8-bit fallback bytes `Doc::retained` captures an 8-bit asset from.
fn asset8() -> Vec<[u8; 4]> {
    opaque(W, H).iter().map(|p| p.map(to8)).collect()
}

fn source_doc(actual16: bool) -> Doc {
    Doc::retained(
        false,
        actual16,
        W,
        H,
        &opaque(W, H),
        IccProfile::srgb(),
        IccProfile::srgb(),
    )
}

fn typed_source_doc() -> Doc {
    Doc::retained(
        true,
        true,
        W,
        H,
        &opaque(W, H),
        IccProfile::srgb(),
        IccProfile::srgb(),
    )
}

/// The byte document's layer 0 as straight RGBA8.
fn bytes(doc: &Doc) -> Vec<[u8; 4]> {
    let Doc::Byte(d) = doc else {
        panic!("byte document expected")
    };
    d.layers[0]
        .pixels
        .contiguous_image()
        .pixels()
        .map(|p| p.0)
        .collect()
}

fn offset(doc: &Doc) -> (i32, i32) {
    let Doc::Byte(d) = doc else {
        panic!("byte document expected")
    };
    d.layers[0].offset
}

/// The oracle: one sRGB triple through the sRGB model to PCS XYZ, then the
/// LUT profile's own PCS→device table, rounded once.
fn oracle(lut: &ProofProfile, rgb: [f64; 3]) -> [u8; 3] {
    let srgb = IccProfile::srgb();
    let xyz = srgb.model().unwrap().to_pcs_xyz64(rgb);
    let (device, n) = lut
        .from_xyz(xyz, RenderingIntent::RelativeColorimetric)
        .expect("the LUT encodes");
    assert_eq!(n, 3);
    std::array::from_fn(|c| (device[c].clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn within_one(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1)
}

fn preflight(doc: &Doc, profile: &[u8], options: &str) -> Result<Value, String> {
    let options = CString::new(options).unwrap();
    let mut e = ptr::null_mut();
    let p = unsafe {
        match doc {
            Doc::Byte(d) => dr_doc_conversion_preflight(
                d,
                profile.as_ptr(),
                profile.len(),
                options.as_ptr(),
                std::ptr::null(),
                &mut e,
            ),
            Doc::Typed(d) => dr_precision_doc_conversion_preflight(
                d,
                profile.as_ptr(),
                profile.len(),
                options.as_ptr(),
                LIMIT,
                0,
                std::ptr::null(),
                &mut e,
            ),
        }
    };
    unsafe { json_result(p, e) }.map(|v| v["adjustments"].clone())
}

/// A conversion's refusal sentence; panics when it converted.
fn refusal(result: Result<Option<Doc>, String>) -> String {
    match result {
        Err(error) => error,
        Ok(converted) => panic!("expected a refusal, got changed={}", converted.is_some()),
    }
}

/// The converted document, `Ok(None)` for the no-op answer.
fn convert(doc: &Doc, profile: &[u8], options: &str) -> Result<Option<Doc>, String> {
    let options = CString::new(options).unwrap();
    let mut report = ptr::null_mut();
    let mut e = ptr::null_mut();
    let result = unsafe {
        match doc {
            Doc::Byte(d) => {
                let p = dr_doc_convert_to_profile_with(
                    d,
                    profile.as_ptr(),
                    profile.len(),
                    options.as_ptr(),
                    ptr::null(),
                    &mut report,
                    &mut e,
                );
                (!p.is_null()).then(|| Doc::Byte(*Box::from_raw(p)))
            }
            Doc::Typed(d) => {
                let p = dr_precision_doc_convert_to_profile_with(
                    d,
                    profile.as_ptr(),
                    profile.len(),
                    options.as_ptr(),
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut report,
                    &mut e,
                );
                (!p.is_null()).then(|| Doc::Typed(*Box::from_raw(p)))
            }
        }
    };
    let error = unsafe { text(e) };
    unsafe { text(report) };
    if !error.is_empty() {
        assert!(result.is_none());
        return Err(error);
    }
    Ok(result)
}

fn converted(doc: &Doc, profile: &[u8]) -> Doc {
    convert(doc, profile, "{}")
        .unwrap()
        .expect("the conversion changes the document")
}

/// Whether layer 0 still holds its retained source.
fn binding(doc: &Doc) -> bool {
    match doc {
        Doc::Byte(d) => d.layers[0].effective_source(&d.profile).is_some(),
        Doc::Typed(d) => d.layer(0).and_then(|l| l.effective_source()).is_some(),
    }
}

fn profile_bytes(doc: &Doc) -> Vec<u8> {
    match doc {
        Doc::Byte(d) => d.profile.bytes().to_vec(),
        Doc::Typed(d) => d.profile().bytes().to_vec(),
    }
}

/// (a) Matrix → LUT keeps a placed source of either asset depth, and its
/// render is the oracle within one code.
#[test]
fn matrix_to_lut_keeps_a_placed_source() {
    let lut = lut_profile();
    let model = ProofProfile::parse(&lut).unwrap();
    for actual16 in [false, true] {
        let doc = source_doc(actual16);
        assert_eq!(preflight(&doc, &lut, "{}"), Ok(Value::Array(vec![])));
        let result = converted(&doc, &lut);
        assert!(binding(&result), "actual16={actual16}");
        assert_eq!(profile_bytes(&result), lut);
        for (i, (got, source)) in bytes(&result).into_iter().zip(opaque(W, H)).enumerate() {
            let input: [f64; 3] = if actual16 {
                std::array::from_fn(|c| f64::from(source[c]) / 65535.0)
            } else {
                std::array::from_fn(|c| f64::from(to8(source[c])) / 255.0)
            };
            let [r, g, b] = oracle(&model, input);
            assert!(
                within_one(got, [r, g, b, 255]),
                "actual16={actual16} pixel {i}: {got:?} vs {:?}",
                [r, g, b]
            );
        }
    }
}

/// (c) An integral nudge on the converted document re-renders through the
/// same pipeline: identical pixels, offset +1 x.
#[test]
fn nudge_after_lut_conversion_keeps_pixels() {
    let lut = lut_profile();
    for actual16 in [false, true] {
        let result = converted(&source_doc(actual16), &lut);
        let moved = result
            .source_edit(
                json!({"op":"transform","layer":0,"matrix":[1,0,0,1,1,0],"filter":"bilinear"}),
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(bytes(&moved), bytes(&result), "actual16={actual16}");
        let (x, y) = offset(&result);
        assert_eq!(offset(&moved), (x + 1, y), "actual16={actual16}");
    }
}

/// (d) LUT → matrix: a document working in the LUT converts back to the
/// asset's own sRGB, keeping the binding and the asset's bytes exactly.
#[test]
fn lut_to_matrix_restores_the_asset_bytes() {
    let doc = Doc::retained(
        false,
        false,
        W,
        H,
        &opaque(W, H),
        IccProfile::srgb(),
        lut_arc(),
    );
    let srgb = IccProfile::srgb().bytes().to_vec();
    assert_eq!(preflight(&doc, &srgb, "{}"), Ok(Value::Array(vec![])));
    let result = converted(&doc, &srgb);
    assert!(binding(&result));
    assert_eq!(profile_bytes(&result), srgb);
    assert_eq!(bytes(&result), asset8());
}

/// (e) A LUT-tagged asset renders into an sRGB document at both depths: the
/// byte render is the asset's own LUT document converted, the typed one
/// within one code of it.
#[test]
fn lut_tagged_asset_renders_at_both_depths() {
    let mut own = DrDocument::from_pixels(RgbaImage::from_raw(W, H, asset8().concat()).unwrap());
    own.profile = lut_arc();
    let srgb = IccProfile::srgb().bytes().to_vec();
    let expected = bytes(&converted(&Doc::Byte(own), &srgb));
    let byte = Doc::retained(
        false,
        false,
        W,
        H,
        &opaque(W, H),
        lut_arc(),
        IccProfile::srgb(),
    );
    assert!(binding(&byte));
    assert_eq!(bytes(&byte), expected);
    let typed = Doc::retained(
        true,
        false,
        W,
        H,
        &opaque(W, H),
        lut_arc(),
        IccProfile::srgb(),
    );
    assert!(binding(&typed));
    for (i, (got, want)) in typed.pixels().into_iter().zip(&expected).enumerate() {
        assert!(
            within_one(got.map(to8), *want),
            "pixel {i}: {got:?} vs {want:?}"
        );
    }
}

/// (f) Every setting a source render cannot follow refuses into the LUT with
/// the one settings sentence; a typed document refuses a LUT destination at
/// its colour plan.
#[test]
fn lut_refusals_name_the_keeping_setting() {
    let lut = lut_profile();
    let documents = [
        (source_doc(true), SETTINGS_SOURCE),
        (typed_source_doc(), SETTINGS_SOURCE),
    ];

    for (doc, sentence) in &documents {
        for options in [
            "{\"intent\":\"perceptual\"}",
            "{\"intent\":\"saturation\"}",
            "{\"intent\":\"absolute\"}",
            "{\"black_point_compensation\":true}",
        ] {
            let what = format!("{options} typed={}", doc.typed());
            assert_eq!(
                preflight(doc, &lut, options).unwrap_err(),
                *sentence,
                "{what}"
            );
            let refused = refusal(convert(doc, &lut, options));
            assert_eq!(refused, *sentence, "{what}");
        }
    }
    let typed = typed_source_doc();
    let refused = refusal(convert(&typed, &lut, "{}"));
    assert!(refused.contains(HIGH_DEPTH), "{refused}");
}

/// The per-pixel cost of `ColorPipeline::color8` through a LUT side over the
/// matrix fast path, measured over one deterministic sweep. Release only;
/// sets `retained_source_render::LUT_COLOR_WORK_FACTOR`.
#[test]
#[ignore]
fn lut_color_cost_ratio() {
    let (lut, name) = match std::fs::read(INSTALLED_LUT) {
        Ok(bytes) => (bytes, "VideoHD16-235.icc"),
        Err(_) => (lut_profile(), "synthetic identity LUT"),
    };
    let lut = IccProfile::parse(&lut).unwrap();
    let srgb = IccProfile::srgb();
    let p3 = IccProfile::display_p3();
    let relative = RenderingIntent::RelativeColorimetric;
    let time = |from: &IccProfile, to: &IccProfile| {
        let pipeline = ColorPipeline::new(from, to, relative, false).unwrap();
        let start = std::time::Instant::now();
        let mut sum = 0u64;
        for i in 0u32..1 << 20 {
            let rgb = [
                (i.wrapping_mul(2_654_435_761) >> 24) as u8,
                (i.wrapping_mul(40_503) >> 8) as u8,
                (i ^ (i >> 7)) as u8,
            ];
            let out = std::hint::black_box(pipeline.color8(std::hint::black_box(rgb)));
            sum += u64::from(out[0]) + u64::from(out[1]) + u64::from(out[2]);
        }
        std::hint::black_box(sum);
        start.elapsed().as_secs_f64()
    };
    let fast = time(&srgb, &p3);
    let into = time(&srgb, &lut);
    let from = time(&lut, &srgb);
    println!(
        "profile {name}: sRGB->P3 {fast:.4}s, sRGB->LUT {into:.4}s ({:.2}x), LUT->sRGB {from:.4}s ({:.2}x)",
        into / fast,
        from / fast
    );
}
