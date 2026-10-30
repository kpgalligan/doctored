//! Phase10D typed Blend If: ramps evaluated on 16-bit codes in the f32
//! compositor against the independent f64 oracle under the strong/weak band
//! policy (designs/phase-10d-blend-if-status.md), byte-compositor parity for
//! promoted documents, clip units/groups/Dissolve, the `layer_blend_if` edit,
//! conditional native35/`LBIF` persistence, preservation through edits and
//! 16-bit PSD blending-range carriage (written since 10C-2 slice 3).
use doctored_core::doc::{BlendMode, DrDocument};
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_info;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_psd_export_report, dr_precision_doc_save_psd,
    dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::{BlendIf, BlendIfChannel};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;
use tempfile::TempDir;

use crate::common;
use crate::identity;
use crate::native_blend_oracle;
use crate::psd16_fixture as fixture;
use common::{cpath, styled, take_err_string};
use fixture::Record;
use native_blend_oracle::{
    blend_if_weight_16, dissolve_threshold, BlendIfOracle, BLEND_IF_BAND, DISSOLVE_BAND,
};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const FULL: [u8; 4] = [0, 0, 255, 255];

// ------------------------------------------------------------- plumbing --

fn error(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}
fn edit_samples(
    doc: &PrecisionDocument,
    request: Value,
    samples: &[u16],
) -> Result<Option<PrecisionDocument>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if let Some(message) = error(err) {
        assert!(result.is_null());
        return Err(message);
    }
    Ok((!result.is_null()).then(|| *unsafe { Box::from_raw(result) }))
}
fn edit(doc: &PrecisionDocument, request: Value) -> Result<Option<PrecisionDocument>, String> {
    edit_samples(doc, request, &[])
}
fn changed(doc: &PrecisionDocument, request: Value) -> PrecisionDocument {
    edit(doc, request).unwrap().unwrap()
}
fn plan(doc: &PrecisionDocument, request: Value) -> Result<Value, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_edit_plan(doc, request.as_ptr(), LIMIT, 0, &mut err) };
    if let Some(message) = error(err) {
        assert!(out.is_null());
        return Err(message);
    }
    Ok(serde_json::from_str(&error(out).unwrap()).unwrap())
}
fn info(doc: &PrecisionDocument) -> Value {
    let mut err = ptr::null_mut();
    let out = unsafe { dr_precision_doc_info(doc, &mut err) };
    assert!(!out.is_null(), "{:?}", error(err));
    serde_json::from_str(&error(out).unwrap()).unwrap()
}
fn blend_if_json(channel: &str, this: [u8; 4], under: [u8; 4]) -> Value {
    json!({"channel": channel, "this_layer": this, "underlying": under})
}
fn request(layer: usize, blend_if: Value) -> Value {
    json!({"op":"layer_blend_if","version":1,"layer":layer,"blend_if":blend_if})
}
fn set(doc: &PrecisionDocument, layer: usize, blend_if: Value) -> PrecisionDocument {
    changed(doc, request(layer, blend_if))
}
fn channel(name: &str) -> BlendIfChannel {
    match name {
        "gray" => BlendIfChannel::Gray,
        "red" => BlendIfChannel::Red,
        "green" => BlendIfChannel::Green,
        "blue" => BlendIfChannel::Blue,
        other => panic!("channel {other}"),
    }
}
fn blend_if(name: &str, this: [u8; 4], under: [u8; 4]) -> BlendIf {
    BlendIf {
        channel: channel(name),
        this_layer: this,
        underlying: under,
        ..BlendIf::default()
    }
}

fn image(pixels: Vec<[u16; 4]>) -> PrecisionImage {
    PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None).unwrap()
}
fn layer(name: &str, pixels: Vec<[u16; 4]>, offset: i32, opacity: f32) -> Layer {
    Layer::raster(name, image(pixels), (offset, 0), opacity, true).unwrap()
}
fn document(width: u32, layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        width,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}
fn render(doc: &PrecisionDocument) -> Vec<[u16; 4]> {
    doc.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap()
}
fn mask(samples: Vec<u16>, offset: i32, density: f32) -> PrecisionMask {
    let mut props = MaskProperties::at((offset, 0));
    props.density = density;
    PrecisionMask::new(
        PrecisionPlane::from_u16(samples.len() as u32, 1, samples, None).unwrap(),
        props,
        true,
    )
    .unwrap()
}
fn invert() -> String {
    json!({"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}).to_string()
}
fn adjustment(name: &str) -> Layer {
    Layer::adjustment(name, &invert(), 1.0, true, &IccProfile::srgb()).unwrap()
}

// ---------------------------------------------------- f64 reference --

fn normalized(p: [u16; 4]) -> [f64; 4] {
    p.map(|v| v as f64 / 65535.)
}
fn codes(p: [f64; 4]) -> [f64; 3] {
    [p[0] * 65535., p[1] * 65535., p[2] * 65535.]
}
/// Independent straight source-over (W3C Normal); `weight` is opacity ×
/// coverage × the Blend If weight.
fn over(base: [f64; 4], mut src: [f64; 4], weight: f64) -> [f64; 4] {
    src[3] *= weight;
    let a = src[3] + base[3] * (1. - src[3]);
    if a == 0. {
        return [0.; 4];
    }
    let mut out = [0.; 4];
    out[3] = a;
    for c in 0..3 {
        out[c] = ((1. - src[3]) * base[3] * base[c]
            + src[3] * ((1. - base[3]) * src[c] + base[3] * src[c]))
            / a;
    }
    out
}
fn deviation(actual: [u16; 4], expected: [f64; 4]) -> f64 {
    (0..4)
        .map(|c| (actual[c] as f64 - expected[c] * 65535.).abs())
        .fold(0., f64::max)
}
/// The band policy: `refs[0]` is the oracle's reference, the rest the
/// neighbouring-code references. Strong form (direct inputs, margin at or
/// above the band): within 0.65 codes of `refs[0]`. Weak form: within 0.65
/// codes of ONE reference, never a mixture. Returns the deviation from the
/// reference taken and whether that was the oracle's.
fn assert_policy(actual: [u16; 4], refs: &[[f64; 4]], margin: f64, direct: bool) -> (f64, bool) {
    if direct && margin >= BLEND_IF_BAND {
        let d = deviation(actual, refs[0]);
        assert!(d <= 0.65, "strong form: {actual:?} vs {:?} ({d})", refs[0]);
        return (d, true);
    }
    for (i, r) in refs.iter().enumerate() {
        let d = deviation(actual, *r);
        if d <= 0.65 {
            return (d, i == 0);
        }
    }
    panic!("weak form: {actual:?} matches none of {refs:?}");
}
fn references(bg: [f64; 4], src: [f64; 4], gate: f64, oracle: &BlendIfOracle) -> Vec<[f64; 4]> {
    let mut refs = vec![over(bg, src, gate * oracle.weight)];
    refs.extend(oracle.alternatives.iter().map(|w| over(bg, src, gate * w)));
    refs
}
fn coverage(density: f64, sample: f64) -> f64 {
    1. - density + density * sample / 65535.
}

// ------------------------------------------------------- (1) every code --

const BACKDROP: [u16; 4] = [30001, 20002, 10003, 65535];
const FILLER: (u16, u16) = (12345, 54321);

fn source_pixel(name: &str, x: u32) -> [u16; 4] {
    let x16 = x as u16;
    match name {
        "red" => [x16, FILLER.0, FILLER.1, 65535],
        "green" => [FILLER.0, x16, FILLER.1, 65535],
        "blue" => [FILLER.0, FILLER.1, x16, 65535],
        // Even codes are neutral (integer luminosity); odd codes carry a
        // +41 blue offset whose 0.11 * 41 = 4.51 luminosity fraction sits
        // inside the band, so gray's in-band weak form is exercised.
        "gray" => {
            if x.is_multiple_of(2) {
                [x16, x16, x16, 65535]
            } else {
                [x16, x16, x16.saturating_add(41), 65535]
            }
        }
        other => panic!("channel {other}"),
    }
}

#[test]
fn every_code_meets_the_ramp_policy_on_a_direct_backdrop() {
    let width = 65536u32;
    let ramps: [([u8; 4], [u8; 4]); 6] = [
        ([100, 100, 255, 255], FULL),
        ([10, 20, 240, 255], FULL),
        ([0, 0, 200, 200], FULL),
        (FULL, FULL),
        (FULL, [64, 96, 255, 255]),
        ([10, 20, 240, 255], [64, 96, 255, 255]),
    ];
    let bg = normalized(BACKDROP);
    let bg_codes = codes(bg);
    let mut max_strong = 0f64;
    let mut max_weak = 0f64;
    let mut in_band = 0usize;
    let mut oracle_taken = 0usize;
    for name in ["red", "green", "blue", "gray"] {
        let pixels: Vec<[u16; 4]> = (0..width).map(|x| source_pixel(name, x)).collect();
        for (this, under) in ramps {
            for (opacity, masked) in [(1.0f32, false), (0.625, false), (0.625, true)] {
                if masked && !(name == "red" && this == [10, 20, 240, 255] && under == FULL) {
                    continue;
                }
                let mut top = layer("Top", pixels.clone(), 0, opacity)
                    .with_blend_if(Some(blend_if(name, this, under)))
                    .unwrap();
                if masked {
                    top = top.with_mask(Some(mask(vec![40000; width as usize], 0, 0.75)));
                }
                let doc = document(
                    width,
                    vec![layer("B", vec![BACKDROP; width as usize], 0, 1.), top],
                );
                let rendered = render(&doc);
                let gate = crate::common::byte_unit(f64::from(opacity))
                    * if masked { coverage(0.75, 40000.) } else { 1. };
                for x in 0..width as usize {
                    let src = normalized(pixels[x]);
                    let oracle = blend_if_weight_16(name, this, under, codes(src), bg_codes);
                    let refs = references(bg, src, gate, &oracle);
                    let (d, taken) = assert_policy(rendered[x], &refs, oracle.margin, true);
                    if oracle.margin >= BLEND_IF_BAND {
                        max_strong = max_strong.max(d);
                    } else {
                        in_band += 1;
                        oracle_taken += usize::from(taken);
                        max_weak = max_weak.max(d);
                    }
                }
                if name == "red" && this == [100, 100, 255, 255] && opacity == 1.0 {
                    assert_eq!(rendered[25699], BACKDROP, "code 25699 hidden exactly");
                    assert_eq!(rendered[25700], pixels[25700], "code 25700 shown exactly");
                }
            }
        }
    }
    eprintln!(
        "direct backdrop: strong max {max_strong:.3} codes; gray in-band pixels {in_band} (weak max {max_weak:.3}, oracle code taken on {oracle_taken})"
    );
    assert!(in_band > 0, "the gray fixture must reach the band");
}

// ------------------------------------------------ (2) underlying value --

#[test]
fn underlying_reads_the_nearest_code_of_the_composite_and_transparent_reads_zero() {
    let width = 4096u32;
    let top_px = [50000u16, 10000, 60000, 65535];
    let under = [20u8, 60, 180, 220];
    // (a) Two partial-alpha layers beneath: a composited backdrop.
    let b1: Vec<[u16; 4]> = (0..width)
        .map(|x| [(x * 16) as u16, 65535 - (x * 16) as u16, 30000, 40000])
        .collect();
    let b2: Vec<[u16; 4]> = (0..width)
        .map(|x| {
            [
                60000 - (x * 8) as u16,
                (x * 4) as u16,
                20000 + x as u16,
                30000,
            ]
        })
        .collect();
    let mut taken = 0usize;
    let mut other = 0usize;
    for name in ["red", "gray"] {
        let doc = document(
            width,
            vec![
                layer("B1", b1.clone(), 0, 1.),
                layer("B2", b2.clone(), 0, 1.),
                layer("Top", vec![top_px; width as usize], 0, 0.8)
                    .with_blend_if(Some(blend_if(name, FULL, under)))
                    .unwrap(),
            ],
        );
        let rendered = render(&doc);
        for x in 0..width as usize {
            let bg = over(over([0.; 4], normalized(b1[x]), 1.), normalized(b2[x]), 1.);
            let src = normalized(top_px);
            let oracle = blend_if_weight_16(name, FULL, under, codes(src), codes(bg));
            let refs = references(bg, src, crate::common::byte_unit(0.8), &oracle);
            let (_, oracle_taken) = assert_policy(rendered[x], &refs, oracle.margin, false);
            if oracle_taken {
                taken += 1;
            } else {
                other += 1;
            }
        }
    }
    eprintln!("composited backdrop: oracle code taken {taken}, neighbouring code {other}");

    // (b) A masked backdrop: the underlying value is its COLOUR, not colour
    // times coverage.
    let b: Vec<[u16; 4]> = (0..width)
        .map(|x| [(x * 16) as u16, 20000, 40000, 65535])
        .collect();
    let samples: Vec<u16> = (0..width).map(|x| (x * 16) as u16).collect();
    let doc = document(
        width,
        vec![
            layer("B", b.clone(), 0, 1.).with_mask(Some(mask(samples.clone(), 0, 0.5))),
            layer("Top", vec![top_px; width as usize], 0, 1.)
                .with_blend_if(Some(blend_if("red", FULL, under)))
                .unwrap(),
        ],
    );
    let rendered = render(&doc);
    let mut partial = 0usize;
    for x in 0..width as usize {
        let bg = over([0.; 4], normalized(b[x]), coverage(0.5, samples[x] as f64));
        let src = normalized(top_px);
        let oracle = blend_if_weight_16("red", FULL, under, codes(src), codes(normalized(b[x])));
        let refs = references(bg, src, 1., &oracle);
        assert_policy(rendered[x], &refs, oracle.margin, false);
        partial += usize::from(oracle.weight > 0. && oracle.weight < 1.);
    }
    assert!(partial > 0, "the masked backdrop reaches the sloped rows");

    // (c) A transparent canvas reads 0: hidden unless lo0 == 0.
    let alone = |under: [u8; 4]| {
        render(&document(
            4,
            vec![layer("Top", vec![top_px; 4], 0, 1.)
                .with_blend_if(Some(blend_if("red", [0, 0, 200, 255], under)))
                .unwrap()],
        ))
    };
    assert_eq!(alone([1, 1, 255, 255]), vec![[0u16; 4]; 4]);
    assert_eq!(alone(FULL), vec![top_px; 4]);
}

// ------------------------------------------- (3) byte compositor parity --

fn byte_px(x: u32) -> [u8; 4] {
    [x as u8, 255 - x as u8, 128, 255]
}
fn promoted(p: [u8; 4]) -> [u16; 4] {
    p.map(|v| u16::from(v) * 257)
}
fn byte_style(name: &str, this: [u8; 4], under: [u8; 4]) -> String {
    json!({"blend_if": blend_if_json(name, this, under)}).to_string()
}
fn assert_promoted_close(typed: &[[u16; 4]], byte: &RgbaImage, what: &str) -> usize {
    let mut worst = 0u32;
    let mut disagreements = 0usize;
    for (x, t) in typed.iter().enumerate() {
        let b = byte.get_pixel(x as u32, 0).0;
        let gap = (0..3)
            .map(|c| (i32::from(t[c]) - i32::from(b[c]) * 257).unsigned_abs())
            .max()
            .unwrap();
        worst = worst.max(gap);
        disagreements += usize::from(gap > 129);
    }
    eprintln!(
        "{what}: worst |typed - 257 * byte| = {worst} codes, {disagreements} pixels past 129"
    );
    disagreements
}

#[test]
fn promoted_documents_match_the_byte_compositor() {
    let w = 256u32;
    let bg = [200u8, 150, 100, 255];
    let byte_bg = RgbaImage::from_pixel(w, 1, Rgba(bg));
    let byte_top = RgbaImage::from_fn(w, 1, |x, _| Rgba(byte_px(x)));
    let typed_bg = vec![promoted(bg); w as usize];
    let typed_top: Vec<[u16; 4]> = (0..w).map(|x| promoted(byte_px(x))).collect();
    let cases: [(&str, [u8; 4], [u8; 4]); 5] = [
        ("red", [100, 100, 255, 255], FULL),
        ("red", [30, 60, 200, 230], FULL),
        ("red", [0, 0, 255, 255], [150, 220, 255, 255]),
        ("gray", [30, 60, 200, 230], FULL),
        ("gray", [100, 100, 255, 255], FULL),
    ];
    let mut gray_disagreements = 0usize;
    for (name, this, under) in cases {
        for opacity in [1.0f32, 0.625] {
            let mut byte_doc = DrDocument::from_pixels(byte_bg.clone())
                .adding_image_layer(0, byte_top.clone(), "Top")
                .unwrap();
            if opacity != 1.0 {
                byte_doc = byte_doc.with_layer_opacity(1, opacity).unwrap();
            }
            let flat = styled(&byte_doc, 1, &byte_style(name, this, under))
                .flattened()
                .unwrap();
            let typed = render(&document(
                w,
                vec![
                    layer("B", typed_bg.clone(), 0, 1.),
                    layer("Top", typed_top.clone(), 0, opacity)
                        .with_blend_if(Some(blend_if(name, this, under)))
                        .unwrap(),
                ],
            ));
            let what = format!("{name} {this:?}/{under:?} opacity {opacity}");
            let disagreements = assert_promoted_close(&typed, &flat, &what);
            if name == "gray" {
                gray_disagreements += disagreements;
            } else {
                assert_eq!(disagreements, 0, "{what}");
            }
            if name == "red" && this == [100, 100, 255, 255] && opacity == 1.0 {
                assert_eq!(flat.get_pixel(99, 0).0, bg);
                assert_eq!(flat.get_pixel(100, 0).0, byte_px(100));
                assert_eq!(typed[99], promoted(bg));
                assert_eq!(typed[100], promoted(byte_px(100)));
            }
        }
    }
    eprintln!("gray luminosity disagreements past 129 codes: {gray_disagreements} (recorded, not matched)");

    // The clip unit: an UNMASKED partial-alpha base with Blend If and one
    // opaque clipped member; the member stays at the footprint alpha where
    // the base's ramp hides the base, in both compositors.
    let base_px = |x: u32| [x as u8, 40u8, 60, 160];
    let member_px = [30u8, 220, 90, 255];
    let byte_base = RgbaImage::from_fn(w, 1, |x, _| Rgba(base_px(x)));
    let byte_member = RgbaImage::from_pixel(w, 1, Rgba(member_px));
    for opacity in [1.0f32, 0.625] {
        let mut byte_doc = DrDocument::from_pixels(byte_bg.clone())
            .adding_image_layer(0, byte_base.clone(), "Base")
            .unwrap()
            .adding_image_layer(1, byte_member.clone(), "M")
            .unwrap()
            .with_layer_clipped(2, true)
            .unwrap();
        if opacity != 1.0 {
            byte_doc = byte_doc.with_layer_opacity(1, opacity).unwrap();
        }
        let flat = styled(&byte_doc, 1, &byte_style("red", [0, 0, 100, 100], FULL))
            .flattened()
            .unwrap();
        let typed = render(&document(
            w,
            vec![
                layer("B", typed_bg.clone(), 0, 1.),
                layer(
                    "Base",
                    (0..w).map(|x| promoted(base_px(x))).collect(),
                    0,
                    opacity,
                )
                .with_blend_if(Some(blend_if("red", [0, 0, 100, 100], FULL)))
                .unwrap(),
                layer("M", vec![promoted(member_px); w as usize], 0, 1.).with_clipped(true),
            ],
        ));
        let what = format!("clip unit opacity {opacity}");
        assert_eq!(assert_promoted_close(&typed, &flat, &what), 0, "{what}");
        for pixel in typed.iter().skip(101) {
            let expected = over(
                normalized(promoted(bg)),
                [
                    member_px[0] as f64 / 255.,
                    member_px[1] as f64 / 255.,
                    member_px[2] as f64 / 255.,
                    160. / 255.,
                ],
                crate::common::byte_unit(f64::from(opacity)),
            );
            assert!(
                deviation(*pixel, expected) <= 0.65,
                "member at the footprint alpha where the base is hidden: {pixel:?} vs {expected:?}"
            );
        }
    }
}

// ------------------------------- (4) groups, clip units, dissolve, refusals --

#[test]
fn groups_clip_units_dissolve_and_refusals() {
    let width = 512u32;
    let bg = [30001u16, 20002, 10003, 65535];
    let inner: Vec<[u16; 4]> = (0..width)
        .map(|x| [(x * 128) as u16, 40000, 20000, 45000])
        .collect();
    let this = [40u8, 80, 160, 220];
    // An isolated group with Blend If: the group's rendered colour is its
    // one raster's colour, weighted against the backdrop.
    let doc = document(
        width,
        vec![
            layer("B", vec![bg; width as usize], 0, 1.),
            Layer::isolated_normal("G", vec![layer("R", inner.clone(), 0, 1.)], 0.75, true)
                .unwrap()
                .with_blend_if(Some(blend_if("red", this, [64, 96, 255, 255])))
                .unwrap(),
        ],
    );
    let rendered = render(&doc);
    let mut partial = 0usize;
    for x in 0..width as usize {
        let group_px = over([0.; 4], normalized(inner[x]), 1.);
        let oracle = blend_if_weight_16(
            "red",
            this,
            [64, 96, 255, 255],
            codes(group_px),
            codes(normalized(bg)),
        );
        let refs = references(
            normalized(bg),
            group_px,
            crate::common::byte_unit(0.75),
            &oracle,
        );
        assert_policy(rendered[x], &refs, oracle.margin, false);
        partial += usize::from(oracle.weight > 0. && oracle.weight < 1.);
    }
    assert!(partial > 0);

    // A clip base with Blend If over a TRANSPARENT canvas: base drawn at
    // a * w, footprint a unweighted, the member onto the base with its alpha
    // frozen RELATIVE to the footprint (#2): r = min(a w / a, 1), the
    // member over (colour, r), alpha a r.
    let a = 0.5f64;
    let base_px = |red: u16| [red, 10000u16, 20000, (a * 65535.) as u16];
    let base = vec![
        base_px(200 * 257),
        base_px(200 * 257),
        base_px(50 * 257),
        base_px(50 * 257),
    ];
    let member_px = [5000u16, 60000, 30000, 65535];
    let member = vec![member_px, [0; 4], member_px, [0; 4]];
    for opacity in [1.0f32, 0.625] {
        let doc = document(
            4,
            vec![
                layer("Base", base.clone(), 0, opacity)
                    .with_blend_if(Some(blend_if("red", [0, 0, 100, 100], FULL)))
                    .unwrap(),
                layer("M", member.clone(), 0, 1.).with_clipped(true),
            ],
        );
        let rendered = render(&doc);
        for x in 0..4 {
            let w = if x < 2 { 0. } else { 1. };
            let footprint = normalized(base[x])[3];
            let mut buf = over([0.; 4], normalized(base[x]), w);
            buf[3] = (buf[3] / footprint).min(1.);
            buf = over(buf, normalized(member[x]), 1.);
            buf[3] *= footprint;
            let expected = over([0.; 4], buf, crate::common::byte_unit(f64::from(opacity)));
            assert!(
                deviation(rendered[x], expected) <= 0.65,
                "clip unit pixel {x}: {:?} vs {expected:?}",
                rendered[x]
            );
        }
        // Hidden base under an opaque member: the member at the footprint.
        let footprint = normalized(base[0])[3];
        assert!(
            (rendered[0][3] as f64
                - footprint * crate::common::byte_unit(f64::from(opacity)) * 65535.)
                .abs()
                <= 0.65
        );
        assert_eq!(rendered[0][..3], member_px[..3]);
        // Hidden base under no member: transparent, not black at a.
        assert_eq!(rendered[1], [0; 4]);
    }

    // A member with Blend If reads the clip buffer (the base's colour) as
    // its underlying.
    let base = vec![
        base_px(30 * 257),
        base_px(120 * 257),
        base_px(220 * 257),
        base_px(120 * 257),
    ];
    let doc = document(
        4,
        vec![
            layer("B", vec![bg; 4], 0, 1.),
            layer("Base", base.clone(), 0, 1.),
            layer("M", vec![member_px; 4], 0, 1.)
                .with_blend_if(Some(blend_if("red", FULL, [100, 100, 150, 150])))
                .unwrap()
                .with_clipped(true),
        ],
    );
    let rendered = render(&doc);
    for x in 0..4 {
        let w = if x == 1 || x == 3 { 1. } else { 0. };
        let footprint = normalized(base[x])[3];
        let mut buf = over([0.; 4], normalized(base[x]), 1.);
        buf[3] = 1.;
        buf = over(buf, normalized(member_px), w);
        buf[3] = footprint;
        let expected = over(normalized(bg), buf, 1.);
        assert!(
            deviation(rendered[x], expected) <= 0.65,
            "member reading the buffer, pixel {x}: {:?} vs {expected:?}",
            rendered[x]
        );
    }

    // Dissolve + Blend If: the threshold reads the WEIGHTED alpha.
    let width = 65536u32;
    let src: Vec<[u16; 4]> = (0..width).map(|x| [x as u16, 1000, 2000, 65535]).collect();
    let doc = document(
        width,
        vec![
            layer("B", vec![bg; width as usize], 0, 1.),
            layer("D", src.clone(), 0, 1.)
                .with_blend_mode(BlendMode::Dissolve)
                .unwrap()
                .with_blend_if(Some(blend_if("red", [10, 200, 255, 255], FULL)))
                .unwrap(),
        ],
    );
    let rendered = render(&doc);
    let (mut shown, mut hidden, mut skipped) = (0usize, 0usize, 0usize);
    for x in 0..width as usize {
        let oracle = blend_if_weight_16(
            "red",
            [10, 200, 255, 255],
            FULL,
            codes(normalized(src[x])),
            codes(normalized(bg)),
        );
        let threshold = dissolve_threshold(x as i64, 0);
        if (threshold - oracle.weight).abs() < DISSOLVE_BAND {
            skipped += 1;
            continue;
        }
        if threshold < oracle.weight {
            assert_eq!(rendered[x], src[x], "dissolve shows the source at {x}");
            shown += 1;
        } else {
            assert_eq!(rendered[x], bg, "dissolve keeps the backdrop at {x}");
            hidden += 1;
        }
    }
    eprintln!("dissolve: {shown} shown, {hidden} hidden, {skipped} within the band");
    assert!(shown > 0 && hidden > 0);

    // Refusals and the identity. An adjustment carries one.
    let some = Some(blend_if("red", this, FULL));
    assert_eq!(
        adjustment("A").with_blend_if(some).unwrap().blend_if(),
        some
    );
    // A Pass Through group carries one.
    let pass = Layer::pass_through("P", vec![layer("R", vec![bg; 4], 0, 1.)], 1., true)
        .unwrap()
        .with_blend_if(some)
        .expect("Pass Through carries Blend If");
    assert!(pass.is_pass_through());
    assert_eq!(pass.blend_if(), some);
    let group = Layer::isolated_normal("G", vec![layer("R", vec![bg; 4], 0, 1.)], 1., true)
        .unwrap()
        .with_blend_if(some)
        .unwrap();
    let passed = group
        .clone()
        .with_blend_mode(BlendMode::PassThrough)
        .expect("pass through on a Blend If group");
    assert!(passed.is_pass_through());
    assert_eq!(passed.blend_if(), some);
    assert!(group.with_blend_mode(BlendMode::Multiply).is_ok());
    assert_eq!(
        layer("R", vec![bg; 4], 0, 1.)
            .with_blend_if(Some(blend_if("blue", FULL, FULL)))
            .unwrap()
            .blend_if(),
        None
    );
}

// -------------------------------------------------------------- (5) edit --

#[test]
fn layer_blend_if_plans_edits_echoes_and_preserves_through_edits() {
    let px = [1000u16, 2000, 3000, 65535];
    let doc = document(
        2,
        vec![
            layer("B", vec![px; 2], 0, 1.),
            layer("T", vec![px; 2], 0, 0.5).with_mask(Some(mask(vec![1, 65535], 0, 1.))),
            Layer::isolated_normal("G", vec![adjustment("A")], 1., true).unwrap(),
            Layer::pass_through("P", vec![layer("R", vec![px; 2], 0, 1.)], 1., true).unwrap(),
        ],
    );
    let value = blend_if_json("red", [10, 20, 240, 255], [0, 0, 200, 220]);
    let req = request(1, value.clone());
    let planned = plan(&doc, req.clone()).unwrap();
    assert_eq!(planned["input_samples"], json!(0));
    assert!(planned["scratch_bytes"].as_u64().unwrap() > 0);
    let set_doc = edit(&doc, req.clone()).unwrap().unwrap();
    assert_eq!(
        set_doc.layer(1).unwrap().blend_if(),
        Some(blend_if("red", [10, 20, 240, 255], [0, 0, 200, 220]))
    );
    assert_eq!(doc.layer(1).unwrap().blend_if(), None, "pure");
    assert!(
        edit(&set_doc, req).unwrap().is_none(),
        "exact no-op is NULL"
    );
    assert!(edit(&doc, request(1, Value::Null)).unwrap().is_none());
    assert!(edit(&doc, request(1, blend_if_json("gray", FULL, FULL)))
        .unwrap()
        .is_none());
    let cleared = set(&set_doc, 1, blend_if_json("red", FULL, FULL));
    assert_eq!(
        cleared.layer(1).unwrap().blend_if(),
        None,
        "identity clears"
    );
    assert_eq!(
        set(&set_doc, 1, Value::Null).layer(1).unwrap().blend_if(),
        None
    );
    let on_group = set(&doc, 3, value.clone());
    assert!(on_group.layer(3).unwrap().blend_if().is_some());
    for (bad, needle) in [
        (
            json!({"op":"layer_blend_if","version":1,"layer":1,"blend_if":value.clone(),"extra":1}),
            "unknown field",
        ),
        (
            request(
                1,
                json!({"channel":"red","this_layer":[0,0,255,255],"underlying":[0,0,200,220],"note":1}),
            ),
            "unknown native blend-if field",
        ),
        (
            json!({"op":"layer_blend_if","version":2,"layer":1,"blend_if":value.clone()}),
            "version",
        ),
        (request(1, blend_if_json("cyan", FULL, FULL)), "channel"),
        (
            request(1, json!({"channel":"red","this_layer":[20,10,255,255]})),
            "non-decreasing",
        ),
        (
            request(1, json!({"channel":"red","this_layer":[0,0,255,255,255]})),
            "4 integers",
        ),
        (request(6, value.clone()), "out of range"),
    ] {
        let message = edit(&doc, bad.clone()).err().expect(needle);
        assert!(message.contains(needle), "{message}");
        assert!(plan(&doc, bad).is_err());
    }
    // A Pass Through group takes one (it then composites isolated).
    let on_pass_through = edit(&doc, request(5, value.clone())).unwrap().unwrap();
    assert!(on_pass_through.layer(5).unwrap().is_pass_through());
    assert!(on_pass_through.layer(5).unwrap().blend_if().is_some());
    // An adjustment record takes one (`precision_style_adjustment`).
    let on_adjustment = edit(&doc, request(2, value.clone())).unwrap().unwrap();
    assert_eq!(
        on_adjustment.layer(2).unwrap().blend_if(),
        Some(blend_if("red", [10, 20, 240, 255], [0, 0, 200, 220]))
    );
    let layers = info(&set_doc)["layers"].as_array().unwrap().clone();
    assert_eq!(layers[1]["blend_if"], value);
    assert_eq!(layers[0]["blend_if"], Value::Null);
    assert_eq!(layers[3]["blend_if"], Value::Null);
    for index in 0..2 {
        assert!(PrecisionImage::ptr_eq(
            set_doc.layer_source(index).unwrap(),
            doc.layer_source(index).unwrap()
        ));
        assert_eq!(
            set_doc.layer(index).unwrap().name().as_ptr(),
            doc.layer(index).unwrap().name().as_ptr()
        );
    }
    assert_eq!(
        set_doc
            .layer(1)
            .unwrap()
            .mask()
            .unwrap()
            .plane()
            .samples()
            .as_ptr(),
        doc.layer(1)
            .unwrap()
            .mask()
            .unwrap()
            .plane()
            .samples()
            .as_ptr()
    );
    assert_eq!(
        info(&set_doc)["supported_operations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v == &&json!("blend_if"))
            .count(),
        1
    );

    // Preservation through every metadata, geometry and retouch edit.
    let doc = set(
        &document(
            3,
            vec![
                layer("B", vec![px; 3], 0, 1.),
                Layer::pass_through(
                    "G",
                    vec![
                        layer("T", vec![px, [5, 6, 7, 65535], [8, 9, 10, 65535]], 0, 1.),
                        layer("M", vec![px; 3], 0, 0.5),
                    ],
                    1.,
                    true,
                )
                .unwrap(),
            ],
        ),
        1,
        value.clone(),
    );
    let flags = |d: &PrecisionDocument| {
        (0..d.layer_count())
            .map(|i| d.layer(i).unwrap().blend_if().is_some())
            .collect::<Vec<_>>()
    };
    assert_eq!(flags(&doc), [false, true, false, false]);
    let cases: Vec<(&str, PrecisionDocument, Vec<bool>)> = vec![
        (
            "duplicate",
            doc.duplicate_layer(1).unwrap().unwrap(),
            vec![false, true, true, false, false],
        ),
        (
            "rename",
            changed(
                &doc,
                json!({"op":"rename_layer","layer":1,"name":"Renamed"}),
            ),
            flags(&doc),
        ),
        (
            "visible",
            changed(
                &doc,
                json!({"op":"layer_visible","layer":1,"visible":false}),
            ),
            flags(&doc),
        ),
        (
            "opacity",
            doc.set_layer_opacity(1, 0.25).unwrap().unwrap(),
            flags(&doc),
        ),
        (
            "blend",
            changed(
                &doc,
                json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":1}),
            ),
            flags(&doc),
        ),
        (
            "set_mask",
            edit_samples(
                &doc,
                json!({"op":"set_mask","layer":1,"width":2,"height":1,"x":0,"y":0}),
                &[1, 65535],
            )
            .unwrap()
            .unwrap(),
            flags(&doc),
        ),
        (
            "crop",
            changed(&doc, json!({"op":"crop","x":1,"y":0,"width":2,"height":1})),
            flags(&doc),
        ),
        (
            "resize",
            changed(
                &doc,
                json!({"op":"resize","width":6,"height":2,"filter":"nearest"}),
            ),
            flags(&doc),
        ),
        (
            "clipped",
            changed(&doc, json!({"op":"layer_clipped","layer":2,"clipped":true})),
            flags(&doc),
        ),
    ];
    for (name, out, expected) in &cases {
        assert_eq!(&flags(out), expected, "{name}");
        assert_eq!(
            out.layer(1).unwrap().blend_if(),
            doc.layer(1).unwrap().blend_if(),
            "{name}"
        );
        assert!(out.encode_native(LIMIT, 0, None).is_ok(), "{name}");
    }
    let removed = changed(&cases[5].1, json!({"op":"remove_mask","layer":1}));
    assert_eq!(flags(&removed), flags(&doc));
    // Retouch onto a new layer: the target keeps its Blend If, the repair
    // has none.
    let repaired = doc
        .retouch(
            &doctored_core::precision_retouch::RetouchEdit::parse(
                &json!({"op":"retouch_clone","layer":1,"output":"new_layer"}).to_string(),
            )
            .unwrap(),
            &[65535, 65535, 65535],
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(flags(&repaired), [false, true, false, false, false]);
}

// ------------------------------------------------------------ (6) native --

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
/// The encoded file with 12E-1's always-written identity wrapper peeled off.
/// This file's subject is an EARLIER wrapper, so the outer word is asserted
/// and the `LIDS` tail removed, leaving byte-for-byte what the writer these
/// tests were written against produced.
fn bytes(doc: &PrecisionDocument) -> Vec<u8> {
    identity::typed_prior_body(&doc.encode_native(LIMIT, 0, None).unwrap())
}
fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}
/// An in-test reader of the `LBIF` tail: the last `12 + 16n` bytes.
fn read_lbif(data: &[u8]) -> Vec<(u32, u32, [u8; 4], [u8; 4])> {
    let at = data.windows(4).rposition(|v| v == b"LBIF").unwrap();
    assert_eq!(word(data, at + 4), 1, "schema");
    let count = word(data, at + 8) as usize;
    assert_eq!(data.len(), at + 12 + 16 * count, "the tail ends the file");
    (0..count)
        .map(|i| {
            let e = at + 12 + 16 * i;
            (
                word(data, e),
                word(data, e + 4),
                data[e + 8..e + 12].try_into().unwrap(),
                data[e + 12..e + 16].try_into().unwrap(),
            )
        })
        .collect()
}
fn outside(doc: &PrecisionDocument, index: usize) -> PrecisionDocument {
    let mask = doc.layer(index).unwrap().mask().unwrap();
    let mut p = mask.properties().clone();
    p.outside_coverage = 255;
    doc.set_layer_mask(
        index,
        Some(mask.with_properties(p, mask.enabled()).unwrap()),
        LIMIT,
        0,
        None,
    )
    .unwrap()
    .unwrap()
}
/// B(0), pass-through G{Inner masked (1), Member (2)}(3), Adj(4), Top(5).
fn native_fixture() -> PrecisionDocument {
    let px: Vec<_> = (0..4)
        .map(|i| {
            [
                1001 + i,
                2003 + i,
                3007 + i,
                [0, 3, 32768, 65535][i as usize],
            ]
        })
        .collect();
    document(
        4,
        vec![
            layer("B", px.clone(), 0, 1.),
            Layer::pass_through(
                "G",
                vec![
                    layer("Inner", px.clone(), 1, 0.625).with_mask(Some(mask(
                        vec![0, 123, 32769],
                        1,
                        0.75,
                    ))),
                    layer("Member", px.clone(), -1, 1.),
                ],
                0.875,
                true,
            )
            .unwrap(),
            adjustment("Adj"),
            layer("Top", px, 0, 0.5),
        ],
    )
}
const TOP_IF: ([u8; 4], [u8; 4]) = ([10, 20, 240, 255], [0, 0, 200, 220]);
const INNER_IF: ([u8; 4], [u8; 4]) = ([0, 0, 255, 255], [64, 96, 255, 255]);
fn styled_fixture(plain: &PrecisionDocument) -> PrecisionDocument {
    set(
        &set(plain, 5, blend_if_json("gray", TOP_IF.0, TOP_IF.1)),
        1,
        blend_if_json("blue", INNER_IF.0, INNER_IF.1),
    )
}
fn released(doc: &PrecisionDocument) -> PrecisionDocument {
    set(&set(doc, 5, Value::Null), 1, Value::Null)
}

#[test]
fn native35_wraps_the_exact_prior_writer_and_composes_with_32_29_34() {
    let plain = native_fixture();
    let with = styled_fixture(&plain);
    let data = bytes(&with);
    assert_eq!(&data[4..12], words(&[35, 13]));
    assert_eq!(
        read_lbif(&data),
        [(1, 3, INNER_IF.0, INNER_IF.1), (5, 0, TOP_IF.0, TOP_IF.1)]
    );
    let prior = bytes(&plain);
    let tail = 12 + 16 * 2;
    assert_eq!(&data[12..data.len() - tail], &prior[8..]);
    assert_eq!(&data[data.len() - tail..], {
        let mut t = b"LBIF".to_vec();
        t.extend(words(&[1, 2, 1, 3]));
        t.extend(INNER_IF.0);
        t.extend(INNER_IF.1);
        t.extend(words(&[5, 0]));
        t.extend(TOP_IF.0);
        t.extend(TOP_IF.1);
        t
    });
    assert_eq!(
        bytes(&released(&with)),
        prior,
        "releasing the last Blend If restores the prior writer"
    );
    let reopened = PrecisionDocument::parse_native(&data, LIMIT, 0, None).unwrap();
    for index in 0..6 {
        assert_eq!(
            reopened.layer(index).unwrap().blend_if(),
            with.layer(index).unwrap().blend_if(),
            "layer {index}"
        );
    }
    assert_eq!(render(&reopened), render(&with));
    assert_eq!(bytes(&reopened), data);

    // 35 → 32 → 29 → 34 → body → LBLD6 → MOUT → LCLP → LBIF.
    let composed = changed(
        &outside(
            &changed(
                &plain,
                json!({"op":"layer_blend","version":1,"layer":0,"blend_mode":BlendMode::Dissolve.to_c()}),
            ),
            1,
        ),
        json!({"op":"layer_clipped","layer":2,"clipped":true}),
    );
    let composed_styled = set(&composed, 5, blend_if_json("gray", TOP_IF.0, TOP_IF.1));
    let data = bytes(&composed_styled);
    assert_eq!(&data[4..24], words(&[35, 32, 29, 34, 13]));
    let at = |tag: &[u8]| data.windows(4).rposition(|v| v == tag).unwrap();
    assert!(at(b"LBLD") < at(b"MOUT") && at(b"MOUT") < at(b"LCLP") && at(b"LCLP") < at(b"LBIF"));
    assert_eq!(read_lbif(&data), [(5, 0, TOP_IF.0, TOP_IF.1)]);
    let prior = bytes(&composed);
    assert_eq!(&prior[4..20], words(&[32, 29, 34, 13]));
    assert_eq!(&data[8..data.len() - 28], &prior[4..]);
    assert_eq!(bytes(&set(&composed_styled, 5, Value::Null)), prior);
    let reopened = PrecisionDocument::parse_native(&data, LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.layer(5).unwrap().blend_if(),
        Some(blend_if("gray", TOP_IF.0, TOP_IF.1))
    );
    assert!(reopened.layer(2).unwrap().clipped());
    assert_eq!(
        reopened
            .layer(1)
            .unwrap()
            .mask()
            .unwrap()
            .properties()
            .outside_coverage,
        255
    );
    assert_eq!(reopened.layer(0).unwrap().blend_mode(), BlendMode::Dissolve);
    assert_eq!(bytes(&reopened), data);
}

#[test]
fn malformed_blend_if_framing_refuses_before_any_png_decode() {
    let plain = native_fixture();
    let good = bytes(&set(&plain, 5, blend_if_json("gray", TOP_IF.0, TOP_IF.1)));
    let tail = good.len() - 28;
    // Every corruption also corrupts a PNG so the framing error must win.
    let corrupt = |data: &mut Vec<u8>| {
        let idat = data.windows(4).position(|b| b == b"IDAT").unwrap();
        data[idat + 4] ^= 0xff;
    };
    let refuse = |data: Vec<u8>, needle: &str| {
        let control = DrJobControl::default();
        let error = PrecisionDocument::parse_native(&data, LIMIT, 0, Some(&control))
            .err()
            .expect("refusal");
        assert!(error.contains(needle), "{error}");
    };
    for (position, value, needle) in [
        (tail + 8, 0, "blend-if count"),
        (tail + 8, 9, "blend-if count"),
        (tail + 12, 7, "blend-if reference"),
        (tail + 16, 4, "blend-if channel"),
        (tail + 4, 2, "blend-if schema"),
    ] {
        let mut bad = good.clone();
        bad[position..position + 4].copy_from_slice(&u32::to_le_bytes(value));
        corrupt(&mut bad);
        refuse(bad, needle);
    }
    // Record 3 is the Pass Through group, which carries one.
    let mut on_pass_through = good.clone();
    on_pass_through[tail + 12..tail + 16].copy_from_slice(&3u32.to_le_bytes());
    let reopened = PrecisionDocument::parse_native(&on_pass_through, LIMIT, 0, None).unwrap();
    assert!(reopened.layer(3).unwrap().is_pass_through());
    assert_eq!(
        reopened.layer(3).unwrap().blend_if(),
        Some(blend_if("gray", TOP_IF.0, TOP_IF.1))
    );
    for (offset, ramp, needle) in [
        (20, [20u8, 10, 240, 255], "blend-if ramp order"),
        (24, [0, 0, 220, 200], "blend-if ramp order"),
    ] {
        let mut bad = good.clone();
        bad[tail + offset..tail + offset + 4].copy_from_slice(&ramp);
        corrupt(&mut bad);
        refuse(bad, needle);
    }
    // Record 4 is the adjustment, which carries one.
    let mut on_adjustment = good.clone();
    on_adjustment[tail + 12..tail + 16].copy_from_slice(&4u32.to_le_bytes());
    let reopened = PrecisionDocument::parse_native(&on_adjustment, LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.layer(4).unwrap().blend_if(),
        Some(blend_if("gray", TOP_IF.0, TOP_IF.1))
    );
    let mut identity = good.clone();
    identity[tail + 20..tail + 28].copy_from_slice(&[0, 0, 255, 255, 0, 0, 255, 255]);
    corrupt(&mut identity);
    refuse(identity, "blend-if identity");
    let two = bytes(&styled_fixture(&plain));
    let tail2 = two.len() - 44;
    for (a, b) in [(5u32, 1u32), (1, 1)] {
        let mut bad = two.clone();
        bad[tail2 + 12..tail2 + 16].copy_from_slice(&a.to_le_bytes());
        bad[tail2 + 28..tail2 + 32].copy_from_slice(&b.to_le_bytes());
        corrupt(&mut bad);
        refuse(bad, "blend-if reference");
    }
    let mut trailing = good.clone();
    trailing.push(0);
    corrupt(&mut trailing);
    refuse(trailing, "trailing");
    let mut truncated = good.clone();
    truncated.truncate(good.len() - 1);
    corrupt(&mut truncated);
    refuse(truncated, "");
    // LBIF under a 34 header.
    let blended = changed(
        &plain,
        json!({"op":"layer_blend","version":1,"layer":0,"blend_mode":BlendMode::Dissolve.to_c()}),
    );
    let mut under34 = bytes(&blended);
    assert_eq!(word(&under34, 4), 34);
    under34.extend_from_slice(&good[tail..]);
    corrupt(&mut under34);
    refuse(under34, "trailing");
    // 35 nested inside 32.
    let with_clip = set(
        &changed(
            &plain,
            json!({"op":"layer_clipped","layer":2,"clipped":true}),
        ),
        5,
        blend_if_json("gray", TOP_IF.0, TOP_IF.1),
    );
    let data = bytes(&with_clip);
    assert_eq!(&data[4..12], words(&[35, 32]));
    let mut reversed = data.clone();
    reversed[4..12].copy_from_slice(&words(&[32, 35]));
    corrupt(&mut reversed);
    refuse(reversed, "clipping typed body version");
    // The uncorrupted saves still open.
    assert!(PrecisionDocument::parse_native(&good, LIMIT, 0, None).is_ok());
    assert!(PrecisionDocument::parse_native(&data, LIMIT, 0, None).is_ok());
}

#[test]
fn a_cmyk_plate_channel_refuses_a_non_cmyk_native_file_by_name() {
    // Codes 4..=7 are the four CMYK plates (#1): a non-CMYK file naming one
    // refuses before any pixel decodes; 8 is past every channel.
    let good = bytes(&set(
        &native_fixture(),
        5,
        blend_if_json("gray", TOP_IF.0, TOP_IF.1),
    ));
    let channel_at = good.len() - 28 + 16;
    let idat = good.windows(4).position(|b| b == b"IDAT").unwrap();
    for (code, expected) in [
        (4u32, "invalid native blend-if channel"),
        (5, "invalid native blend-if channel"),
        (6, "invalid native blend-if channel"),
        (7, "invalid native blend-if channel"),
        (8, "unsupported native blend-if channel"),
    ] {
        let mut bad = good.clone();
        bad[channel_at..channel_at + 4].copy_from_slice(&code.to_le_bytes());
        bad[idat + 4] ^= 0xff;
        let error = PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
            .err()
            .expect("refusal");
        assert!(error.contains(expected), "code {code}: {error}");
    }
}

// ------------------------------------------------------------- (8) PSD16 --

fn report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).unwrap()
}
fn open16(path: &Path, notice: &Value) -> PrecisionDocument {
    let token = CString::new(notice["source_fingerprint"].as_str().unwrap()).unwrap();
    let mut err = ptr::null_mut();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(path).as_ptr(),
            0,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null::<DrJobControl>(),
            &mut err,
        )
    };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    *unsafe { Box::from_raw(raw) }
}
fn planes(rgba: &[[u16; 4]]) -> Vec<(i16, Vec<u8>)> {
    [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            let samples: Vec<u16> = rgba.iter().map(|p| p[component]).collect();
            (id, fixture::raw_plane(&samples))
        })
        .collect()
}

#[test]
fn blend_if_is_written_as_blending_ranges_through_sixteen_bit_psd() {
    let directory = TempDir::new().unwrap();
    let b = [30001u16, 20002, 10003, 65535];
    let t: Vec<[u16; 4]> = vec![
        [10 * 257, 5000, 6000, 65535],
        [200 * 257, 5000, 6000, 65535],
    ];
    let plain = document(
        2,
        vec![layer("B", vec![b; 2], 0, 1.), layer("Top", t, 0, 1.)],
    );
    let source = set(&plain, 1, blend_if_json("red", [0, 0, 100, 100], FULL));
    assert_ne!(render(&source), render(&plain), "the ramp hides a pixel");
    let mut err = ptr::null_mut();
    let json = unsafe { dr_precision_doc_psd_export_report(&source, 0, 0, &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    let preflight: Value = serde_json::from_str(&take_err_string(json)).unwrap();
    assert_eq!(preflight["available"], Value::Bool(true));
    assert!(
        preflight["losses"]
            .as_array()
            .unwrap()
            .iter()
            .all(|l| !l.as_str().unwrap().contains("Blend If")),
        "Blend If is no longer a loss: {}",
        preflight["losses"]
    );
    let observations = preflight["observations"].as_array().unwrap();
    assert!(
        observations.iter().any(|o| o.as_str().unwrap()
            == "Layer “Top” has a layer style (Blend If on red); it is written as Photoshop layer effects and blending options (lfx2, iOpa, blending ranges), so Photoshop renders them itself; its appearance there is not qualified."),
        "{observations:?}"
    );
    let projection = source.render(LIMIT, 0, None).unwrap();
    let path = directory.path().join("blend_if16.psd");
    assert!(
        unsafe {
            dr_precision_doc_save_psd(
                &source,
                &projection,
                cpath(&path).as_ptr(),
                0,
                0,
                LIMIT,
                0,
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    let file = fixture::read(&std::fs::read(&path).unwrap());
    let by_name = fixture::by_name(&file);
    // The red pair in slot 1, the unrestricted default in the other four.
    let mut expected = [0u8; 40];
    for slot in expected.chunks_exact_mut(4) {
        slot.copy_from_slice(&[0, 0, 255, 255]);
    }
    expected[8..12].copy_from_slice(&[0, 0, 100, 100]);
    assert_eq!(by_name["Top"].ranges, expected);
    assert!(
        by_name["B"].ranges.is_empty(),
        "an unrestricted layer writes none"
    );
    assert!(
        by_name["Top"].blocks.iter().all(|(key, _)| key != b"lfx2"),
        "Blend If alone writes no effects descriptor"
    );
    let notice = report(&path);
    assert_eq!(notice["import_precision"], json!("rgba16"));
    assert!(!notice["verified_losses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l.as_str().unwrap().contains("Blend If")));
    let reopened = open16(&path, &notice);
    let layers = info(&reopened)["layers"].as_array().unwrap().clone();
    assert_eq!(
        layers[1]["blend_if"],
        json!({"channel":"red","this_layer":[0,0,100,100],"underlying":[0,0,255,255]})
    );
    assert_eq!(
        reopened.layer(1).unwrap().blend_if(),
        source.layer(1).unwrap().blend_if()
    );
    assert_eq!(
        render(&reopened),
        render(&source),
        "the file shows the ramps the source applied"
    );

    // A fixture-authored 16-bit file with ONE restricted gray pair is carried
    // the same way; two restricted pairs stay the shared marker loss.
    let px = vec![[4097u16, 51234, 60000, 65535], [4098, 51233, 60000, 32768]];
    let mut record = Record::raster("R", [0, 0, 1, 2], planes(&px));
    record.ranges = vec![0, 0, 255, 255, 0, 3, 250, 255];
    let bytes = fixture::fixture(
        (2, 1),
        &[record],
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &px,
        true,
        false,
    );
    let authored = directory.path().join("ranges16.psd");
    std::fs::write(&authored, bytes).unwrap();
    let notice = report(&authored);
    assert_eq!(notice["import_precision"], json!("rgba16"));
    assert!(
        !notice["verified_losses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l.as_str().unwrap().contains("Blend If")),
        "{}",
        notice["verified_losses"]
    );
    let opened = open16(&authored, &notice);
    assert_eq!(
        info(&opened)["layers"][0]["blend_if"],
        json!({"channel":"gray","this_layer":[0,0,255,255],"underlying":[0,3,250,255]})
    );

    let mut both = Record::raster("R", [0, 0, 1, 2], planes(&px));
    both.ranges = [0u8, 0, 255, 255, 0, 3, 250, 255].repeat(2);
    let bytes = fixture::fixture(
        (2, 1),
        &[both],
        IccProfile::srgb().bytes(),
        (72.0, 72.0),
        &px,
        true,
        false,
    );
    let authored = directory.path().join("ranges16-two.psd");
    std::fs::write(&authored, bytes).unwrap();
    let notice = report(&authored);
    // Two restricted channels are CARRIED: Gray the primary, Red its
    // extra, at 16 bits.
    assert!(
        !notice["verified_losses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l.as_str().unwrap().contains("Blend If")),
        "{}",
        notice["verified_losses"]
    );
    let opened = open16(&authored, &notice);
    assert_eq!(
        info(&opened)["layers"][0]["blend_if"],
        json!({"channel":"gray","this_layer":[0,0,255,255],"underlying":[0,3,250,255],
            "extra":[{"channel":"red","this_layer":[0,0,255,255],"underlying":[0,3,250,255]}]})
    );
}
