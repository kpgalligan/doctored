//! A LAYER clipboard pasted into a 32-bit float document
//! (`layer_clipboard_float`, `PrecisionDocument::paste_layers`), through
//! `dr_precision_doc_paste_layers`.
//!
//! Oracles, none read off the implementation: a promoted sample is the f64
//! chain over the profile's own tag data (`common::icc_chain`: the `curv`
//! table, then `inv(dst.to_pcs) · src.to_pcs`), within 1e-5 (the tolerance
//! `precision_clipboard_promote.rs` fixed for the same kernel) with alpha
//! exactly `code / 65535`; the SAME pixels through the pixel paste
//! (`clipboard_paste`) must agree bit for bit, because both call one kernel;
//! the report and every refusal sentence are written out here; the admission
//! threshold is exact.

use crate::common;
use crate::icc_spelling;
use crate::live_filter_support;

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::dr_precision_doc_artwork_metadata;
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_precision::dr_precision_image_storage_identity;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_clipboard::{ClipboardPayload, PasteProfile, PasteRequest};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::retained_source::{SourceEdit, SourceInput};
use doctored_core::style::{BlendIf, BlendIfChannel, LayerStyle};
use doctored_core::warp::WarpEdit;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use common::icc_chain::{apply, inverse, Chain};
use live_filter_support::{plasma, Doc};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const ABOVE: u32 = 0;
const EMPTY_REPORT: &str = r#"{"blend_reset":[],"effect_blend_reset":[],"rasterized":[]}"#;
const EXPOSURE: &str =
    r#"{"type":"adjust","evaluation":"precision_v1","op":"exposure","params":{"exposure":0.25}}"#;

// ------------------------------------------------------------- plumbing --

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// `dr_precision_doc_paste_layers` above record 0, its refusal as `Err`.
fn paste_at(
    doc: &PrecisionDocument,
    clip: &PrecisionDocument,
    offset: (i32, i32),
    mode: PasteProfile,
    limit: u64,
) -> Result<(PrecisionDocument, Value), String> {
    let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
    let out = unsafe {
        dr_precision_doc_paste_layers(
            doc,
            clip,
            0,
            ABOVE,
            offset.0,
            offset.1,
            mode == PasteProfile::Preserve,
            limit,
            0,
            &mut report,
            &mut err,
        )
    };
    if let Some(message) = owned(err) {
        assert!(out.is_null());
        assert!(report.is_null(), "no report on a refusal");
        return Err(message);
    }
    assert!(!out.is_null(), "a paste is never a silent no-op");
    let report = serde_json::from_str(&owned(report).expect("a report")).unwrap();
    Ok((*unsafe { Box::from_raw(out) }, report))
}

fn paste(
    doc: &PrecisionDocument,
    clip: &PrecisionDocument,
    mode: PasteProfile,
) -> (PrecisionDocument, Value) {
    let (out, report) = paste_at(doc, clip, (0, 0), mode, LIMIT).unwrap();
    assert!(out.is_float());
    assert_eq!(out.float_refusal(), None, "the float renderer accepts it");
    (out, report)
}

fn refused(doc: &PrecisionDocument, clip: &PrecisionDocument) -> String {
    let before = names(doc);
    let message = paste_at(doc, clip, (0, 0), PasteProfile::Convert, LIMIT)
        .err()
        .expect("a refusal");
    assert_eq!(names(doc), before, "the destination is untouched");
    message
}

fn refusal_sentence(layer: &str, feature: &str) -> String {
    format!(
        "the copied layer “{layer}” cannot be pasted into a 32-bit float document: {feature} is unavailable in a 32-bit float document"
    )
}

enum Samples {
    U16(Vec<[u16; 4]>),
    F32(Vec<[f32; 4]>),
}

/// The test's own RZCB v3 writer (`precision_clipboard_promote.rs`'s): no
/// coverage, no editable section, offset (0, 0).
fn drcb(w: u32, h: u32, profile: &[u8], samples: &Samples) -> Vec<u8> {
    let mut out = b"RZCB".to_vec();
    out.extend(3u32.to_le_bytes());
    out.extend(
        match samples {
            Samples::U16(_) => 1u32,
            Samples::F32(_) => 2u32,
        }
        .to_le_bytes(),
    );
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    out.extend(0i32.to_le_bytes());
    out.extend(0i32.to_le_bytes());
    out.extend(72f32.to_le_bytes());
    out.extend(72f32.to_le_bytes());
    out.extend((profile.len() as u32).to_le_bytes());
    out.extend(profile);
    out.push(0);
    out.extend(0u32.to_le_bytes());
    out.extend(4f32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    match samples {
        Samples::U16(p) => p.iter().flatten().for_each(|v| out.extend(v.to_le_bytes())),
        Samples::F32(p) => p.iter().flatten().for_each(|v| out.extend(v.to_le_bytes())),
    }
    out
}

fn payload(w: u32, h: u32, profile: &[u8], samples: &Samples) -> ClipboardPayload {
    ClipboardPayload::decode(&drcb(w, h, profile, samples), LIMIT, 0).expect("a payload")
}

/// A float document whose one "Background" raster holds `pixels`.
fn float_doc(w: u32, h: u32, profile: &[u8], pixels: Vec<[f32; 4]>) -> PrecisionDocument {
    let doc = PrecisionDocument::from_clipboard(
        &payload(w, h, profile, &Samples::F32(pixels)),
        LIMIT,
        0,
        None,
    )
    .expect("a float document");
    assert!(doc.is_float());
    doc
}

fn float_base(w: u32, h: u32, profile: &Arc<IccProfile>) -> PrecisionDocument {
    float_doc(
        w,
        h,
        profile.bytes(),
        vec![[0.25, 0.5, 0.75, 1.0]; (w * h) as usize],
    )
}

/// Float samples with a value above white and a negative one.
fn f32_pattern(n: u32) -> Vec<[f32; 4]> {
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            [t * 1.5, 1.0 - t, -0.125 + t * 0.5, 0.25 + 0.75 * t]
        })
        .collect()
}

/// Every u16 code family: the ends, the midpoint and a spread, varied alpha.
fn u16_pattern(n: u32) -> Vec<[u16; 4]> {
    let alphas = [0u16, 1, 32768, 65534, 65535];
    (0..n)
        .map(|i| {
            let k = i as usize;
            [
                ((k * 7919) % 65536) as u16,
                ((k * 104_729 + 3) % 65536) as u16,
                [0u16, 65535, 32768, 1, 65534][k % 5],
                alphas[k % 5],
            ]
        })
        .collect()
}

fn u16_image(w: u32, h: u32, profile: &Arc<IccProfile>) -> PrecisionImage {
    PrecisionImage::from_rgba16(w, h, u16_pattern(w * h), profile.clone(), None).unwrap()
}

fn raster(name: &str, w: u32, h: u32, profile: &Arc<IccProfile>) -> Layer {
    Layer::raster(name, u16_image(w, h, profile), (0, 0), 1.0, true).unwrap()
}

fn doc16(w: u32, h: u32, profile: &Arc<IccProfile>, layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        w,
        h,
        profile.clone(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn edit(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    let text = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
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
    if let Some(message) = owned(err) {
        panic!("{request}: {message}");
    }
    assert!(!out.is_null(), "{request} changed nothing");
    *unsafe { Box::from_raw(out) }
}

fn names(doc: &PrecisionDocument) -> Vec<String> {
    (0..doc.layer_count())
        .map(|i| doc.layer(i).unwrap().name().to_string())
        .collect()
}

fn floats(doc: &PrecisionDocument, index: usize) -> Vec<[f32; 4]> {
    doc.layer_source(index)
        .expect("a raster record")
        .to_rgba32()
        .expect("float samples")
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

fn address(doc: &PrecisionDocument, index: usize) -> usize {
    let source = doc.layer_source(index).expect("a raster record");
    unsafe { dr_precision_image_storage_identity(source) as usize }
}

fn request(mode: PasteProfile) -> PasteRequest {
    let profile = match mode {
        PasteProfile::Convert => "convert",
        PasteProfile::Preserve => "preserve",
    };
    serde_json::from_value(json!({
        "name": "Pasted", "anchor": 0, "position": "above",
        "placement": "in_place", "profile": profile,
    }))
    .unwrap()
}

/// The pixel paste of the same samples: the landed record's floats.
fn pixel_paste(
    dest: &PrecisionDocument,
    payload: &ClipboardPayload,
    mode: PasteProfile,
) -> Vec<[f32; 4]> {
    let out = dest
        .clipboard_paste(payload, &request(mode), LIMIT, 0, None)
        .expect("a pixel paste");
    floats(&out, 1)
}

fn combined(src: &Chain, dst: &Chain) -> [[f64; 3]; 3] {
    let inv = inverse(dst.to_pcs);
    std::array::from_fn(|r| {
        std::array::from_fn(|c| (0..3).map(|k| inv[r][k] * src.to_pcs[k][c]).sum())
    })
}

/// The f64 chain: `code` decoded by `decode`'s curve, then `matrix`.
fn promoted_oracle(decode: &Chain, matrix: Option<[[f64; 3]; 3]>, code: [u16; 4]) -> [f64; 4] {
    let linear: [f64; 3] = std::array::from_fn(|c| decode.decode(f64::from(code[c]) / 65535.0));
    let rgb = matrix.map_or(linear, |m| apply(m, linear));
    [rgb[0], rgb[1], rgb[2], f64::from(code[3]) / 65535.0]
}

fn assert_promoted(got: &[[f32; 4]], codes: &[[u16; 4]], want: impl Fn([u16; 4]) -> [f64; 4]) {
    assert_eq!(got.len(), codes.len());
    for (i, (g, code)) in got.iter().zip(codes).enumerate() {
        let w = want(*code);
        for c in 0..3 {
            let d = (f64::from(g[c]) - w[c]).abs();
            assert!(d <= 1e-5, "pixel {i} channel {c}: {} vs {}", g[c], w[c]);
        }
        assert_eq!(g[3], f32::from(code[3]) / 65535.0, "pixel {i}: alpha");
    }
}

// ------------------------------------------------------------ promotion --

#[test]
fn sixteen_bit_and_byte_layers_promote_like_the_pixel_paste() {
    let srgb = IccProfile::srgb();
    let s = Chain::of(&srgb);
    let dest = float_base(9, 10, &srgb);
    let codes = u16_pattern(90);
    let clip = doc16(9, 10, &srgb, vec![raster("Photo", 9, 10, &srgb)]);
    let (out, report) = paste(&dest, &clip, PasteProfile::Convert);
    assert_eq!(report.to_string(), EMPTY_REPORT);
    assert_eq!(names(&out), ["Background", "Photo"]);
    assert_eq!(out.tone_map(), dest.tone_map());
    assert!(Arc::ptr_eq(
        out.layer_source(1).unwrap().profile(),
        out.profile()
    ));
    let landed = floats(&out, 1);
    assert_promoted(&landed, &codes, |c| promoted_oracle(&s, None, c));
    let same = pixel_paste(
        &dest,
        &payload(9, 10, srgb.bytes(), &Samples::U16(codes)),
        PasteProfile::Convert,
    );
    assert_eq!(bits(&landed), bits(&same), "one kernel, bit for bit");

    // 8 bits: ×257, then the same promotion.
    let image = RgbaImage::from_fn(8, 8, |x, y| {
        Rgba([
            (x * 31 + y) as u8,
            255 - (y * 29) as u8,
            (x * y * 7) as u8,
            if (x + y) % 3 == 0 { 255 } else { 120 },
        ])
    });
    let byte = DrDocument::from_pixels(image.clone());
    let clip = byte.promote_structure(LIMIT, 0, None).unwrap();
    let dest = float_base(8, 8, &srgb);
    let (out, _) = paste(&dest, &clip, PasteProfile::Convert);
    let widened: Vec<[u16; 4]> = image
        .pixels()
        .map(|p| p.0.map(|v| u16::from(v) * 257))
        .collect();
    let landed = floats(&out, 1);
    assert_promoted(&landed, &widened, |c| promoted_oracle(&s, None, c));
    let bytes = ClipboardPayload::from_rgba8(
        image.as_raw(),
        8,
        8,
        srgb.bytes(),
        (0, 0),
        Resolution::default(),
        LIMIT,
        0,
    )
    .unwrap();
    assert_eq!(
        bits(&landed),
        bits(&pixel_paste(&dest, &bytes, PasteProfile::Convert))
    );
}

#[test]
fn another_profile_converts_or_keeps_its_numbers() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let (s, d) = (Chain::of(&srgb), Chain::of(&p3));
    let dest = float_base(9, 10, &srgb);
    let codes = u16_pattern(90);
    let clip = doc16(9, 10, &p3, vec![raster("P3", 9, 10, &p3)]);
    // Convert: the P3 curve, then P3 → sRGB on linear light, negatives kept.
    let (out, _) = paste(&dest, &clip, PasteProfile::Convert);
    let converted = floats(&out, 1);
    let m = combined(&d, &s);
    assert_promoted(&converted, &codes, |c| promoted_oracle(&d, Some(m), c));
    assert!(converted.iter().any(|p| p[..3].iter().any(|v| *v < 0.0)));
    // Preserve: the numbers read as the destination's encoding, no matrix.
    let (out, _) = paste(&dest, &clip, PasteProfile::Preserve);
    assert_promoted(&floats(&out, 1), &codes, |c| promoted_oracle(&s, None, c));

    // A FLOAT clip in Display P3.
    let pixels = f32_pattern(90);
    let float_clip = float_doc(9, 10, p3.bytes(), pixels.clone());
    let (out, report) = paste(&dest, &float_clip, PasteProfile::Convert);
    assert_eq!(report.to_string(), EMPTY_REPORT);
    for (i, (got, source)) in floats(&out, 1).iter().zip(&pixels).enumerate() {
        let want = apply(m, std::array::from_fn(|c| f64::from(source[c])));
        for c in 0..3 {
            let bound = 1e-5 * want[c].abs().max(1.0);
            assert!((f64::from(got[c]) - want[c]).abs() <= bound, "pixel {i}");
        }
        assert_eq!(got[3], source[3]);
    }
    let (out, _) = paste(&dest, &float_clip, PasteProfile::Preserve);
    assert_eq!(
        address(&out, 1),
        address(&float_clip, 0),
        "Don't Convert shares the float samples"
    );
    assert_eq!(bits(&floats(&out, 1)), bits(&pixels));
}

#[test]
fn a_float_clip_in_the_documents_own_profile_is_todays_paste() {
    let srgb = IccProfile::srgb();
    let dest = float_base(10, 8, &srgb);
    let clip = float_doc(10, 8, srgb.bytes(), f32_pattern(80));
    // 4·BOOKKEEPING + 2·names + 4096·records + 65·canvas (`doc_paste_layers`).
    let today = 4 * (1 << 20) + 2 * (2 * "Background".len() as u64) + 4096 * 2 + 65 * 80;
    for mode in [PasteProfile::Convert, PasteProfile::Preserve] {
        assert_eq!(dest.paste_layers_allocation(&clip, mode).unwrap(), today);
        assert_eq!(
            dest.paste_layers_plan(&clip, mode).unwrap(),
            dest.retained_bytes() + today
        );
        let (out, report) = paste(&dest, &clip, mode);
        assert_eq!(report.to_string(), EMPTY_REPORT);
        assert_eq!(address(&out, 1), address(&clip, 0));
    }
}

#[test]
fn a_second_spelling_of_the_profile_never_refuses() {
    let srgb = IccProfile::srgb();
    let bytes = icc_spelling::srgb_second_spelling();
    assert_ne!(&bytes[..], &srgb.bytes()[..]);
    let spelled = Arc::new(IccProfile::parse(&bytes).expect("a matrix/TRC profile"));
    let dest = float_base(9, 10, &srgb);
    let codes = u16_pattern(90);
    let pixels = f32_pattern(90);
    let clips = [
        (
            doc16(9, 10, &spelled, vec![raster("Spelled", 9, 10, &spelled)]),
            payload(9, 10, &bytes, &Samples::U16(codes)),
        ),
        (
            float_doc(9, 10, &bytes, pixels.clone()),
            payload(9, 10, &bytes, &Samples::F32(pixels)),
        ),
    ];
    for (clip, same) in &clips {
        for mode in [PasteProfile::Convert, PasteProfile::Preserve] {
            let (out, _) = paste(&dest, clip, mode);
            assert_eq!(out.profile().bytes(), srgb.bytes());
            assert_eq!(
                bits(&floats(&out, 1)),
                bits(&pixel_paste(&dest, same, mode)),
                "equal to the pixel paste of the same pixels"
            );
            let plan = dest.paste_layers_plan(clip, mode).unwrap();
            let err = paste_at(&dest, clip, (0, 0), mode, plan - 1)
                .err()
                .expect("a refusal one byte under the plan");
            assert!(err.contains("working limit"), "{err}");
            assert!(paste_at(&dest, clip, (0, 0), mode, plan).is_ok());
        }
    }
    // The float clip under Convert is charged as a conversion; under
    // Preserve nothing converts.
    let float_clip = &clips[1].0;
    assert!(
        dest.paste_layers_allocation(float_clip, PasteProfile::Convert)
            .unwrap()
            > dest
                .paste_layers_allocation(float_clip, PasteProfile::Preserve)
                .unwrap()
    );
}

// ----------------------------------------------- what lands as its pixels --

fn typed(doc: Doc) -> PrecisionDocument {
    match doc {
        Doc::Typed(document) => document,
        Doc::Byte(_) => panic!("a typed document"),
    }
}

fn warp_field(layer: usize) -> WarpEdit {
    let values: Vec<[f64; 2]> = (0..9)
        .map(|i| {
            let (fx, fy) = (f64::from(i % 3) / 2.0, f64::from(i / 3) / 2.0);
            [0.35 + 0.58 * fx - 0.31 * fy, -0.27 + 0.33 * fy + 0.19 * fx]
        })
        .collect();
    WarpEdit::parse(
        &json!({"op":"set","layer":layer,"target":"pixels","columns":3,"rows":3,
            "displacements":values,"filter":"bicubic"})
        .to_string(),
    )
    .unwrap()
}

/// A 16-bit Display P3 document: `Background` (white), `Placed` (a placed
/// image), `Text` (described) and `Warped`.
fn held_clip() -> PrecisionDocument {
    let p3 = IccProfile::display_p3();
    let (w, h) = (12, 9);
    let placed = Doc::placed_over_white(
        true,
        true,
        w,
        h,
        &plasma(w, h, 0),
        IccProfile::srgb(),
        p3.clone(),
    );
    let doc = typed(placed);
    let doc = edit(&doc, json!({"op":"rename_layer","layer":1,"name":"Placed"}));
    // Two plain rasters above it, through the pixel paste.
    let plain = payload(w, h, p3.bytes(), &Samples::U16(u16_pattern(w * h)));
    let add = |doc: &PrecisionDocument, anchor: usize, name: &str| {
        let request: PasteRequest = serde_json::from_value(json!({
            "name": name, "anchor": anchor, "position": "above", "placement": "in_place",
        }))
        .unwrap();
        doc.clipboard_paste(&plain, &request, LIMIT, 0, None)
            .unwrap()
    };
    let doc = add(&doc, 1, "Text");
    let doc = add(&doc, 2, "Warped");
    let text = CString::new(json!({"type":"text","version":1,"payload":"hi"}).to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_artwork_metadata(&doc, 2, text.as_ptr(), LIMIT, 0, &mut err) };
    assert!(owned(err).is_none() && !out.is_null());
    let doc = *unsafe { Box::from_raw(out) };
    let doc = doc
        .warp_edit(&warp_field(3), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(names(&doc), ["Background", "Placed", "Text", "Warped"]);
    assert!(doc.layer(1).unwrap().effective_source().is_some());
    assert!(doc.artwork_description(2).is_some());
    assert!(doc.warp_info(3).unwrap()["field"].is_object());
    doc
}

#[test]
fn text_and_warps_land_as_their_pixels_beside_a_kept_placed_image() {
    let clip = held_clip();
    let srgb = IccProfile::srgb();
    let p3 = Chain::of(&IccProfile::display_p3());
    let s = Chain::of(&srgb);
    let dest = float_base(12, 9, &srgb);
    let (out, report) = paste(&dest, &clip, PasteProfile::Convert);
    assert_eq!(
        names(&out),
        ["Background", "Background", "Placed", "Text", "Warped"]
    );
    assert_eq!(
        report,
        json!({
            "rasterized": [
                {"layer": "Warped", "reason": "warp"},
            ],
            "blend_reset": [],
            "effect_blend_reset": [],
        })
    );
    for i in 1..5 {
        assert_eq!(out.layer(i).unwrap().effective_source().is_some(), i == 2);
        assert!(out.warp_info(i).unwrap()["field"].is_null());
    }
    // Phase 187b: the Text record lands described under Convert, its bytes
    // verbatim, as measured against Photoshop 26.10.0 (`float_artwork.py`'s
    // PT paste).
    assert_eq!(out.artwork_description(3), clip.artwork_description(2));
    for i in [1, 2, 4] {
        assert!(out.artwork_description(i).is_none());
    }
    // The kept pixels are each layer's own render, promoted (the placed
    // image re-renders from its master instead: `an_unfiltered_placed_image_lands_placed`).
    let m = combined(&p3, &s);
    for i in 2..4 {
        let codes = clip.layer_source(i).unwrap().to_rgba16().unwrap();
        assert_promoted(&floats(&out, i + 1), &codes, |c| {
            promoted_oracle(&p3, Some(m), c)
        });
    }
    // Don't Convert (decision 6): every layer keeps its
    // numbers, the placed image's included — decoded under the destination's
    // curve with no matrix.
    let (kept, kept_report) = paste(&dest, &clip, PasteProfile::Preserve);
    // A Display P3 record relabelled into sRGB under Preserve no longer shows
    // what its description renders: the description drops with its row.
    assert!(kept_report["rasterized"]
        .as_array()
        .unwrap()
        .contains(&json!({"layer": "Text", "reason": "text or shape"})));
    assert!(kept.artwork_description(3).is_none());
    let codes = clip.layer_source(1).unwrap().to_rgba16().unwrap();
    assert_promoted(&floats(&kept, 2), &codes, |c| promoted_oracle(&s, None, c));
    assert_ne!(bits(&floats(&kept, 2)), bits(&floats(&out, 2)), "Placed");
    let codes = clip.layer_source(2).unwrap().to_rgba16().unwrap();
    assert_promoted(&floats(&kept, 3), &codes, |c| promoted_oracle(&s, None, c));
    assert_ne!(bits(&floats(&kept, 3)), bits(&floats(&out, 3)), "Text");
}

/// A placed image lands PLACED. Photoshop
/// 26.10.0 keeps a 16-bit placed image bound to its original when it is pasted
/// into a 32-bit document (`float_placed.py`, paste verdict `kept`).
#[test]
fn an_unfiltered_placed_image_lands_placed() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let s = Chain::of(&srgb);
    let (w, h) = (12, 9);
    let pixels = plasma(w, h, 0);
    let clip = typed(Doc::placed_over_white(
        true,
        true,
        w,
        h,
        &pixels,
        srgb.clone(),
        p3,
    ));
    assert!(clip.layer(1).unwrap().effective_source().is_some());
    let dest = float_base(w, h, &srgb);

    // Convert: the record is re-rendered from its master, exactly the render
    // a fresh place of the same master gives.
    let (out, report) = paste(&dest, &clip, PasteProfile::Convert);
    assert_eq!(report.to_string(), EMPTY_REPORT);
    assert!(out.layer(2).unwrap().effective_source().is_some());
    let master = live_filter_support::source(w, h, &pixels, srgb.clone());
    let placed = dest
        .source_edit_input(
            Some(SourceInput::Typed(&master)),
            &SourceEdit::parse(
                &json!({"op":"place","source_id":"fresh","name":"fresh","x":0,"y":0}).to_string(),
            )
            .unwrap(),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(bits(&floats(&out, 2)), bits(&floats(&placed, 1)));

    // Preserve: the numbers are kept, and so is the binding, over them.
    let (kept, report) = paste(&dest, &clip, PasteProfile::Preserve);
    assert_eq!(report.to_string(), EMPTY_REPORT);
    let layer = kept.layer(2).unwrap();
    assert!(layer.effective_source().is_some());
    let codes = clip.layer_source(1).unwrap().to_rgba16().unwrap();
    assert_promoted(&floats(&kept, 2), &codes, |c| promoted_oracle(&s, None, c));
}

// ------------------------------------------------- groups and blend modes --

fn pass_through(name: &str, children: Vec<Layer>) -> Layer {
    Layer::pass_through(name, children, 0.75, true).unwrap()
}

#[test]
fn pass_through_groups_land_as_pass_through_with_blended_and_adjustment_children() {
    // The float render composites Pass Through (measured
    // against Photoshop 26.10.0 by `float_pass_through.py`), so every Pass
    // Through group lands as one, whatever its children, and nothing is
    // reported.
    let srgb = IccProfile::srgb();
    let dest = float_base(6, 4, &srgb);
    let leaf = |name: &str| raster(name, 6, 4, &srgb);
    let landed_pass_through = |out: &PrecisionDocument, i: usize| {
        let group = out.layer(i).unwrap();
        assert!(group.children().is_some() && group.is_pass_through(), "{i}");
        assert_eq!(group.blend_mode(), BlendMode::PassThrough);
        assert_eq!(group.opacity(), 0.75);
    };
    // Every direct child Normal, a nested Pass Through group included.
    let normal = doc16(
        6,
        4,
        &srgb,
        vec![pass_through(
            "Outer",
            vec![leaf("A"), pass_through("Inner", vec![leaf("B")])],
        )],
    );
    let (out, report) = paste(&dest, &normal, PasteProfile::Convert);
    assert_eq!(names(&out), ["Background", "A", "B", "Inner", "Outer"]);
    assert_eq!(report.to_string(), EMPTY_REPORT);
    for i in [3, 4] {
        landed_pass_through(&out, i);
    }
    // A blended child, an adjustment child and a blended grandchild.
    let blended = leaf("Multiplied")
        .with_blend_mode(BlendMode::Multiply)
        .unwrap();
    let adjusting = Layer::adjustment("Exposure", EXPOSURE, 1.0, true, &srgb).unwrap();
    let inner = pass_through(
        "Inner",
        vec![leaf("B").with_blend_mode(BlendMode::Darken).unwrap()],
    );
    let clip = doc16(
        6,
        4,
        &srgb,
        vec![pass_through(
            "Outer",
            vec![leaf("A"), blended, adjusting, inner],
        )],
    );
    let (out, report) = paste(&dest, &clip, PasteProfile::Convert);
    assert_eq!(
        names(&out),
        [
            "Background",
            "A",
            "Multiplied",
            "Exposure",
            "B",
            "Inner",
            "Outer"
        ]
    );
    assert_eq!(report.to_string(), EMPTY_REPORT);
    landed_pass_through(&out, 5);
    landed_pass_through(&out, 6);
    assert_eq!(out.layer(2).unwrap().blend_mode(), BlendMode::Multiply);
    assert_eq!(out.layer(4).unwrap().blend_mode(), BlendMode::Darken);
    out.render(LIMIT, 0, None).unwrap();
    // A child whose mode the 32-bit paste resets (Overlay) lands Normal
    // inside a group that stays Pass Through.
    let reset = leaf("Overlaid")
        .with_blend_mode(BlendMode::Overlay)
        .unwrap();
    let clip = doc16(
        6,
        4,
        &srgb,
        vec![pass_through("Reset", vec![leaf("A"), reset])],
    );
    let (out, report) = paste(&dest, &clip, PasteProfile::Convert);
    landed_pass_through(&out, 3);
    assert_eq!(
        report["blend_reset"],
        json!([{"layer": "Overlaid", "from": "overlay"}])
    );
}

#[test]
fn refused_modes_land_normal_and_admitted_ones_ride() {
    let srgb = IccProfile::srgb();
    let dest = float_base(6, 4, &srgb);
    let moded =
        |name: &str, mode: BlendMode| raster(name, 6, 4, &srgb).with_blend_mode(mode).unwrap();
    // The nine of the twelve modes `paste_layers.py` measured as landing
    // Normal that a float document still refuses (decision B).
    let reset = [
        (BlendMode::ColorBurn, "color_burn"),
        (BlendMode::Screen, "screen"),
        (BlendMode::ColorDodge, "color_dodge"),
        (BlendMode::Overlay, "overlay"),
        (BlendMode::SoftLight, "soft_light"),
        (BlendMode::HardLight, "hard_light"),
        (BlendMode::VividLight, "vivid_light"),
        (BlendMode::HardMix, "hard_mix"),
        (BlendMode::Exclusion, "exclusion"),
    ];
    let clip = doc16(
        6,
        4,
        &srgb,
        reset
            .iter()
            .map(|(mode, name)| moded(name, *mode))
            .collect(),
    );
    let (out, report) = paste(&dest, &clip, PasteProfile::Convert);
    let rows: Vec<Value> = reset
        .iter()
        .map(|(_, name)| json!({"layer": name, "from": name}))
        .collect();
    assert_eq!(report["blend_reset"], Value::Array(rows));
    for i in 1..out.layer_count() {
        assert_eq!(out.layer(i).unwrap().blend_mode(), BlendMode::Normal);
    }
    // The four other admitted modes ride, and so do the nine 32-bit modes
    // built first and the three admitted after them.
    let admitted = [
        BlendMode::LinearBurn,
        BlendMode::LinearLight,
        BlendMode::PinLight,
        BlendMode::Multiply,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Difference,
        BlendMode::Dissolve,
        BlendMode::DarkerColor,
        BlendMode::Addition,
        BlendMode::LighterColor,
        BlendMode::Subtract,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];
    let clip = doc16(
        6,
        4,
        &srgb,
        admitted.iter().map(|mode| moded("Kept", *mode)).collect(),
    );
    let (out, report) = paste(&dest, &clip, PasteProfile::Convert);
    assert_eq!(report.to_string(), EMPTY_REPORT);
    for (i, mode) in admitted.iter().enumerate() {
        assert_eq!(out.layer(i + 1).unwrap().blend_mode(), *mode);
    }
    // Divide is admitted (decision B) and rides too.
    let clip = doc16(
        6,
        4,
        &srgb,
        vec![
            raster("Fine", 6, 4, &srgb),
            moded("Moded", BlendMode::Divide),
        ],
    );
    let (out, report) = paste(&dest, &clip, PasteProfile::Convert);
    assert_eq!(report.to_string(), EMPTY_REPORT);
    assert_eq!(out.layer(2).unwrap().blend_mode(), BlendMode::Divide);
}

#[test]
fn styled_and_ranged_layers_land_with_the_float_effect_mode_resets() {
    // A style and Blend If ride verbatim, except the effect
    // modes reset on the way into 32 bits (`FLOAT_EFFECT_RESET`: Screen on an
    // Outer Glow, an Inner Glow or a Bevel's highlight lands Linear Dodge, as
    // measured against Photoshop 26.10.0, `float_styles.py`'s paste verdict).
    let srgb = IccProfile::srgb();
    let dest = float_base(10, 8, &srgb);
    let shadow = LayerStyle::from_json(
        r##"{"effects":[{"type":"drop_shadow","use_global_light":true,"distance":2,"size":1,
            "spread":0,"opacity":0.5,"color":"#000000"}],"knockout":"shallow",
            "fill_opacity":0.5}"##,
    )
    .unwrap();
    let styled = raster("Shadowed", 10, 8, &srgb)
        .with_layer_style(Some(&shadow), (10, 8))
        .unwrap();
    let ranged = raster("Ranged", 10, 8, &srgb)
        .with_blend_if(Some(BlendIf {
            channel: BlendIfChannel::Gray,
            this_layer: [0, 0, 200, 200],
            underlying: [0, 0, 255, 255],
            ..BlendIf::default()
        }))
        .unwrap();
    let glows = LayerStyle::from_json(
        r##"{"effects":[{"type":"outer_glow"},{"type":"inner_glow"},
            {"type":"bevel_emboss"},{"type":"satin"}]}"##,
    )
    .unwrap();
    let glowing = raster("Glowing", 10, 8, &srgb)
        .with_layer_style(Some(&glows), (10, 8))
        .unwrap();
    let clip = doc16(
        10,
        8,
        &srgb,
        vec![styled.clone(), ranged.clone(), glowing.clone()],
    );
    let (out, report) = paste(&dest, &clip, PasteProfile::Convert);
    assert_eq!(names(&out), ["Background", "Shadowed", "Ranged", "Glowing"]);
    assert_eq!(
        report["effect_blend_reset"],
        json!([
            {"layer": "Glowing", "effect": "outer_glow", "from": "screen", "to": "linear_dodge"},
            {"layer": "Glowing", "effect": "inner_glow", "from": "screen", "to": "linear_dodge"},
            {"layer": "Glowing", "effect": "bevel_emboss", "from": "screen",
             "to": "linear_dodge"},
        ])
    );
    assert_eq!(report["blend_reset"], json!([]));
    // The shadow and the range land verbatim.
    assert_eq!(
        out.layer(1).unwrap().reported_style(),
        styled.reported_style()
    );
    assert_eq!(
        out.layer(2).unwrap().reported_style(),
        ranged.reported_style()
    );
    // The glows land in Linear Dodge, the satin's and bevel shadow's
    // Multiply kept; everything else verbatim.
    let landed: Value =
        serde_json::from_str(&out.layer(3).unwrap().reported_style().unwrap()).unwrap();
    let mut want: Value = serde_json::from_str(&glowing.reported_style().unwrap()).unwrap();
    for effect in want["effects"].as_array_mut().unwrap() {
        match effect["type"].as_str().unwrap() {
            "outer_glow" | "inner_glow" => effect["blend"] = json!("linear_dodge"),
            "bevel_emboss" => effect["highlight_blend"] = json!("linear_dodge"),
            _ => {}
        }
    }
    assert_eq!(landed, want);
    let satin = landed["effects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "satin")
        .unwrap();
    assert_eq!(satin["blend"], "multiply");
}

#[test]
fn unreset_effect_modes_and_other_adjustments_refuse_by_layer_and_feature() {
    let srgb = IccProfile::srgb();
    let dest = float_base(10, 8, &srgb);
    let overlaid =
        LayerStyle::from_json(r##"{"effects":[{"type":"outer_glow","blend":"overlay"}]}"##)
            .unwrap();
    let overlaid = raster("Overlaid", 10, 8, &srgb)
        .with_layer_style(Some(&overlaid), (10, 8))
        .unwrap();
    let burned =
        LayerStyle::from_json(r##"{"effects":[{"type":"drop_shadow","blend":"color_burn"}]}"##)
            .unwrap();
    let burned = raster("Burned", 10, 8, &srgb)
        .with_layer_style(Some(&burned), (10, 8))
        .unwrap();
    let adjustment = |name: &str, op: &str, params: Value| {
        let description =
            json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params});
        Layer::adjustment(name, &description.to_string(), 1.0, true, &srgb).unwrap()
    };
    let cases = [
        (
            overlaid,
            "Overlaid",
            "the overlay blend mode on the outer_glow layer effect".to_string(),
        ),
        (
            burned,
            "Burned",
            "the color_burn blend mode on the drop_shadow layer effect".to_string(),
        ),
        (
            adjustment("Inverted", "invert", json!({})),
            "Inverted",
            "the invert adjustment".to_string(),
        ),
        (
            adjustment(
                "Legacy",
                "white_balance",
                json!({"temperature": 5000, "evaluation": "legacy_srgb"}),
            ),
            "Legacy",
            "the white_balance adjustment with legacy evaluation".to_string(),
        ),
    ];
    for (layer, name, feature) in cases {
        // A landing record first: the refusal names the refused one.
        let clip = doc16(10, 8, &srgb, vec![raster("Fine", 10, 8, &srgb), layer]);
        assert_eq!(refused(&dest, &clip), refusal_sentence(name, &feature));
    }
}

#[test]
fn a_calibrated_exposure_lands_as_the_same_adjustment() {
    let srgb = IccProfile::srgb();
    let dest = float_base(9, 10, &srgb);
    let photo = raster("Photo", 9, 10, &srgb);
    let exposure = Layer::adjustment("Exposure", EXPOSURE, 1.0, true, &srgb).unwrap();
    let clip = doc16(9, 10, &srgb, vec![photo.clone(), exposure]);
    let (out, report) = paste(&dest, &clip, PasteProfile::Convert);
    assert_eq!(report.to_string(), EMPTY_REPORT);
    assert_eq!(names(&out), ["Background", "Photo", "Exposure"]);
    assert_eq!(
        out.adjustment_description(2),
        clip.adjustment_description(1)
    );
    // The same adjustment ADDED to the float document over the same pixels.
    let (alone, _) = paste(
        &dest,
        &doc16(9, 10, &srgb, vec![photo]),
        PasteProfile::Convert,
    );
    let added = alone
        .add_adjustment("Exposure", EXPOSURE, 1.0, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(names(&added), names(&out));
    assert!(out
        .render(LIMIT, 0, None)
        .unwrap()
        .same_samples(&added.render(LIMIT, 0, None).unwrap()));
}

/// Each adjustment the float mode now evaluates lands with an
/// empty report, its description verbatim, and renders as the same
/// adjustment ADDED to the float document over the same pixels.
#[test]
fn each_built_float_adjustment_lands_as_the_same_adjustment() {
    let srgb = IccProfile::srgb();
    let dest = float_base(9, 10, &srgb);
    let photo = raster("Photo", 9, 10, &srgb);
    let (alone, _) = paste(
        &dest,
        &doc16(9, 10, &srgb, vec![photo.clone()]),
        PasteProfile::Convert,
    );
    let built = [
        (
            "levels",
            json!({"black": 0.1, "gamma": 1.4, "output_white": 0.9}),
        ),
        ("black_and_white", json!({"reds": 1.2, "blues": -0.5})),
        (
            "channel_mixer",
            json!({"red": {"r": 1, "g": 0, "b": 0, "constant": 0.1}}),
        ),
        (
            "photo_filter",
            json!({"color": "#ec8a00", "density": 0.5, "preserve_luminosity": true}),
        ),
        ("bcs", json!({"brightness": 0.1, "contrast": 0.3})),
    ];
    for (op, params) in built {
        let description =
            json!({"type":"adjust","evaluation":"precision_v1","op":op,"params":params})
                .to_string();
        let layer = Layer::adjustment("Adjust", &description, 1.0, true, &srgb).unwrap();
        let clip = doc16(9, 10, &srgb, vec![photo.clone(), layer]);
        let (out, report) = paste(&dest, &clip, PasteProfile::Convert);
        assert_eq!(report.to_string(), EMPTY_REPORT, "{op}");
        assert_eq!(names(&out), ["Background", "Photo", "Adjust"], "{op}");
        assert_eq!(
            out.adjustment_description(2),
            clip.adjustment_description(1),
            "{op}"
        );
        let added = alone
            .add_adjustment("Adjust", &description, 1.0, LIMIT, 0, None)
            .unwrap()
            .unwrap();
        assert!(
            out.render(LIMIT, 0, None)
                .unwrap()
                .same_samples(&added.render(LIMIT, 0, None).unwrap()),
            "{op}"
        );
    }
}

// ------------------------------------------------ structure and identity --

#[test]
fn shared_planes_promote_once_and_structure_rides() {
    let srgb = IccProfile::srgb();
    let image = u16_image(5, 4, &srgb);
    let mask = PrecisionMask::new(
        PrecisionPlane::from_u16(5, 4, (0..20).map(|i| i * 3000).collect(), None).unwrap(),
        Default::default(),
        true,
    )
    .unwrap();
    let first = Layer::raster("First", image.clone(), (1, 2), 0.5, true)
        .unwrap()
        .with_mask(Some(mask))
        .with_label(2);
    let second = Layer::raster("Second", image, (3, -1), 1.0, false)
        .unwrap()
        .with_clipped(true);
    let group = Layer::isolated_normal("G", vec![first, second], 0.8, true).unwrap();
    let clip = doc16(10, 8, &srgb, vec![group, raster("Top", 10, 8, &srgb)]);
    let clip = edit(&clip, json!({"op":"link_layers","layers":[0,3]}));
    assert_eq!(names(&clip), ["First", "Second", "G", "Top"]);
    let dest = float_base(10, 8, &srgb);
    let dest = dest.duplicate_layer(0).unwrap().unwrap();
    let dest = edit(&dest, json!({"op":"rename_layer","layer":1,"name":"Other"}));
    let dest = edit(&dest, json!({"op":"link_layers","layers":[0,1]}));
    let (out, _) = paste(&dest, &clip, PasteProfile::Convert);
    assert_eq!(
        names(&out),
        ["Background", "First", "Second", "G", "Top", "Other"]
    );
    assert_eq!(address(&out, 1), address(&out, 2), "one promoted plane");
    assert_ne!(address(&out, 1), address(&clip, 0));
    // The promoted plane is charged once: a second record over the same
    // plane adds only its record and its name.
    let one = doc16(
        10,
        8,
        &srgb,
        vec![Layer::raster("First", u16_image(5, 4, &srgb), (0, 0), 1.0, true).unwrap()],
    );
    let two_image = u16_image(5, 4, &srgb);
    let two = doc16(
        10,
        8,
        &srgb,
        vec![
            Layer::raster("First", two_image.clone(), (0, 0), 1.0, true).unwrap(),
            Layer::raster("Second", two_image, (0, 0), 1.0, true).unwrap(),
        ],
    );
    let allocation = |clip: &PrecisionDocument| {
        dest.paste_layers_allocation(clip, PasteProfile::Convert)
            .unwrap()
    };
    assert_eq!(
        allocation(&two) - allocation(&one),
        4096 + 2 * "Second".len() as u64
    );

    let (a, b) = (clip.layer(0).unwrap(), out.layer(1).unwrap());
    assert_eq!((b.offset(), b.opacity(), b.label()), ((1, 2), 0.5, 2));
    assert_eq!(
        a.mask().unwrap().plane().samples(),
        b.mask().unwrap().plane().samples()
    );
    let second = out.layer(2).unwrap();
    assert!(second.clipped() && !second.visible());
    assert_eq!(second.offset(), (3, -1));
    assert_eq!(out.layer(3).unwrap().opacity(), 0.8);
    // Fresh ids, and the pasted link group takes an unused link id.
    let mut ids: Vec<u64> = (0..out.layer_count())
        .map(|i| out.layer(i).unwrap().id())
        .collect();
    let clip_ids: Vec<u64> = (0..clip.layer_count())
        .map(|i| clip.layer(i).unwrap().id())
        .collect();
    assert!(ids[1..5]
        .iter()
        .all(|id| *id != 0 && !clip_ids.contains(id)));
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), out.layer_count());
    let dest_link = out.layer(0).unwrap().link();
    assert_eq!(out.layer(5).unwrap().link(), dest_link);
    let pasted_link = out.layer(1).unwrap().link();
    assert_eq!(out.layer(4).unwrap().link(), pasted_link);
    assert!(pasted_link != 0 && pasted_link != dest_link);
}

#[test]
fn a_placement_offset_moves_the_promoted_records() {
    let srgb = IccProfile::srgb();
    let clip = doc16(
        10,
        8,
        &srgb,
        vec![Layer::raster("Moved", u16_image(4, 3, &srgb), (2, 1), 1.0, true).unwrap()],
    );
    let dest = float_base(10, 8, &srgb);
    let (still, _) = paste(&dest, &clip, PasteProfile::Convert);
    let (moved, _) = paste_at(&dest, &clip, (5, -3), PasteProfile::Convert, LIMIT).unwrap();
    assert_eq!(still.layer(1).unwrap().offset(), (2, 1));
    assert_eq!(moved.layer(1).unwrap().offset(), (7, -2));
    assert_eq!(bits(&floats(&moved, 1)), bits(&floats(&still, 1)));
}

// -------------------------------------------------------------- admission --

#[test]
fn the_plan_is_the_exact_threshold_small_and_large() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    for (w, h) in [(10, 8), (1024, 768)] {
        let dest = float_base(w, h, &srgb);
        let clip = doc16(
            w,
            h,
            &p3,
            vec![raster("One", w, h, &p3), raster("Two", w / 2, h / 2, &p3)],
        );
        for mode in [PasteProfile::Convert, PasteProfile::Preserve] {
            let plan = dest.paste_layers_plan(&clip, mode).unwrap();
            let allocation = dest.paste_layers_allocation(&clip, mode).unwrap();
            assert_eq!(plan, dest.retained_bytes() + allocation);
            // The two promoted planes are held as f32 (16 B per pixel) beside
            // today's insertion: 4·BOOKKEEPING + 2·names + 4096·records +
            // 65·canvas, and a promotion margin
            // (`LAYER_PROMOTE_MARGIN_PER_PIXEL`, 10) per promoted pixel. That
            // side decides here; the promotion's own working memory
            // (bookkeeping and each new plane twice, plus the decode table)
            // stays under it.
            let pixels = u64::from(w) * u64::from(h) + u64::from(w / 2) * u64::from(h / 2);
            let name_bytes = ("Background".len() + "One".len() + "Two".len()) as u64;
            let insertion = 4 * (1 << 20) + 2 * name_bytes + 4096 * 3;
            assert_eq!(
                allocation,
                16 * pixels + insertion + 65 * u64::from(w) * u64::from(h) + 10 * pixels
            );
            assert!(allocation >= (1 << 20) + 2 * 16 * pixels);
            let err = paste_at(&dest, &clip, (0, 0), mode, plan - 1)
                .err()
                .expect("a refusal one byte under the plan");
            assert!(err.contains("working limit"), "{err}");
            let (out, _) = paste_at(&dest, &clip, (0, 0), mode, plan).unwrap();
            assert_eq!(names(&out), ["Background", "One", "Two"]);
        }
    }
}

#[test]
fn a_sixteen_bit_destination_keeps_its_refusals_and_null_is_safe() {
    let (srgb, p3) = (IccProfile::srgb(), IccProfile::display_p3());
    let dest16 = doc16(6, 4, &srgb, vec![raster("Base", 6, 4, &srgb)]);
    let clip_p3 = doc16(6, 4, &p3, vec![raster("P3", 6, 4, &p3)]);
    // Don't Convert relabels; Convert still refuses.
    let (relabelled, _) =
        paste_at(&dest16, &clip_p3, (0, 0), PasteProfile::Preserve, LIMIT).unwrap();
    assert_eq!(
        relabelled.layer_source(1).unwrap().to_rgba16().unwrap(),
        clip_p3.layer_source(0).unwrap().to_rgba16().unwrap()
    );
    assert_eq!(
        paste_at(&dest16, &clip_p3, (0, 0), PasteProfile::Convert, LIMIT)
            .err()
            .as_deref(),
        Some("pasted layers must be in the document's profile; convert them first")
    );
    let float_clip = float_base(6, 4, &srgb);
    assert_eq!(
        paste_at(&dest16, &float_clip, (0, 0), PasteProfile::Convert, LIMIT)
            .err()
            .as_deref(),
        Some("pasted layers must be at the document's depth; convert them first")
    );
    // A 16-bit paste answers the empty report.
    let clip = doc16(6, 4, &srgb, vec![raster("Same", 6, 4, &srgb)]);
    let (out, report) = paste_at(&dest16, &clip, (0, 0), PasteProfile::Convert, LIMIT).unwrap();
    assert!(!out.is_float());
    assert_eq!(report.to_string(), EMPTY_REPORT);

    // NULL handles are named refusals; a NULL report_out is accepted.
    let dest = float_base(6, 4, &srgb);
    for (document, layers) in [
        (ptr::null(), &clip as *const PrecisionDocument),
        (&dest as *const PrecisionDocument, ptr::null()),
    ] {
        let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
        let out = unsafe {
            dr_precision_doc_paste_layers(
                document,
                layers,
                0,
                ABOVE,
                0,
                0,
                false,
                LIMIT,
                0,
                &mut report,
                &mut err,
            )
        };
        assert!(out.is_null() && report.is_null());
        assert!(owned(err).is_some());
    }
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_paste_layers(
            &dest,
            &clip,
            0,
            ABOVE,
            0,
            0,
            false,
            LIMIT,
            0,
            ptr::null_mut(),
            &mut err,
        )
    };
    assert!(owned(err).is_none() && !out.is_null());
    drop(unsafe { Box::from_raw(out) });
    let out = unsafe {
        dr_precision_doc_paste_layers(
            &dest,
            &clip,
            0,
            ABOVE,
            0,
            0,
            false,
            LIMIT,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    assert!(!out.is_null());
    drop(unsafe { Box::from_raw(out) });
    // An unknown position code refuses.
    let mut err = ptr::null_mut();
    assert!(unsafe {
        dr_precision_doc_paste_layers(
            &dest,
            &clip,
            0,
            9,
            0,
            0,
            false,
            LIMIT,
            0,
            ptr::null_mut(),
            &mut err,
        )
    }
    .is_null());
    assert!(owned(err).is_some());
}
