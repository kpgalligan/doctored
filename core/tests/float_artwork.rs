//! Text and shape layers in a 32-bit float document — the f32
//! artwork render (`artwork_render::render_float`) through
//! `dr_artwork_render_float`, its publication, persistence, paste and
//! narrowing.
//!
//! Oracles follow Photoshop 26.10.0's measured verdicts (receipt
//! `doctored-fixtures/photoshop/float-artwork-473/take2`), none read off the
//! implementation: a coverage colour's numbers ARE linear light (H-raw),
//! carried into the document's primaries by `inv(dst.to_pcs) · src.to_pcs`
//! over the profiles' own tag data (`common::icc_chain`); alpha is
//! `coverage / 65535` (C-same); inks mix by the W3C `over` on linear values
//! (H-lin); an intrinsic RGBA8 pixel decodes through sRGB's `curv` table
//! first. Tolerance 1e-6 · max(1, |want|). Description BYTES are compared
//! verbatim: the core never rewrites them (the host reads them in the
//! destination's ink).

use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{
    dr_artwork_render, dr_artwork_render_float, dr_precision_doc_artwork,
    dr_precision_doc_artwork_at, dr_precision_doc_artwork_description,
    dr_precision_doc_artwork_metadata,
};
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::float_document::float_operators_json;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_capability::capability;
use doctored_core::precision_conversion_narrow::NarrowTarget;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_quantize::PrecisionDither;
use serde_json::{json, Value};

use crate::common;
use crate::float_fixture::float_doc;
use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_wtpt};
use common::icc_chain::{apply, inverse, Chain};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const TOLERANCE: f64 = 1e-6;
/// `precision_image::BOOKKEEPING_BYTES`, the FFI's per-term allowance.
const BOOKKEEPING: u64 = 1024 * 1024;
const STORAGE: &str =
    "artwork pixels require the exact document profile and the document's own storage";
const SHAPE: &str = r#"{"type":"shape","version":4,"artwork":{"ink":"linear_srgb"}}"#;
const TEXT: &str = r#"{"type":"text","version":3,"payload":"HI"}"#;
/// Take2's ink F = (46, 128, 200) as the stored numbers.
const F: [f64; 3] = [46.0 / 255.0, 128.0 / 255.0, 200.0 / 255.0];

// ------------------------------------------------------------- plumbing --

fn owned(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}

/// `dr_artwork_render_float` (or `dr_artwork_render` when `!float`).
#[allow(clippy::too_many_arguments)]
fn render_at(
    float: bool,
    w: u32,
    h: u32,
    components: &Value,
    samples: &[u16],
    intrinsic: &[u8],
    profile: &IccProfile,
    limit: u64,
) -> Result<PrecisionImage, String> {
    let description = CString::new(components.to_string()).unwrap();
    let call = if float {
        dr_artwork_render_float
    } else {
        dr_artwork_render
    };
    let mut err = ptr::null_mut();
    let out = unsafe {
        call(
            w,
            h,
            description.as_ptr(),
            if samples.is_empty() {
                ptr::null()
            } else {
                samples.as_ptr()
            },
            samples.len(),
            if intrinsic.is_empty() {
                ptr::null()
            } else {
                intrinsic.as_ptr()
            },
            intrinsic.len(),
            profile.bytes().as_ptr(),
            profile.bytes().len(),
            limit,
            0,
            ptr::null(),
            &mut err,
        )
    };
    match owned(err) {
        Some(message) => {
            assert!(out.is_null());
            Err(message)
        }
        None => {
            assert!(!out.is_null());
            Ok(*unsafe { Box::from_raw(out) })
        }
    }
}

fn render_float(
    w: u32,
    h: u32,
    components: &Value,
    samples: &[u16],
    intrinsic: &[u8],
    profile: &IccProfile,
) -> PrecisionImage {
    let image = render_at(true, w, h, components, samples, intrinsic, profile, LIMIT)
        .expect("the float render");
    assert_eq!(image.storage(), PrecisionStorage::Rgba32Float);
    image
}

/// `dr_precision_doc_artwork` (`position` None) or `_at` (`Some(code)`):
/// `Ok(None)` for a no-op.
fn publish(
    doc: &PrecisionDocument,
    layer: usize,
    replacing: bool,
    position: Option<u32>,
    image: &PrecisionImage,
    description: &str,
) -> Result<Option<PrecisionDocument>, String> {
    let name = CString::new("Artwork").unwrap();
    let description = CString::new(description).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        match position {
            None => dr_precision_doc_artwork(
                doc,
                layer,
                replacing,
                image,
                1,
                2,
                name.as_ptr(),
                description.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            ),
            Some(position) => dr_precision_doc_artwork_at(
                doc,
                layer,
                position,
                image,
                1,
                2,
                name.as_ptr(),
                description.as_ptr(),
                LIMIT,
                0,
                ptr::null(),
                &mut err,
            ),
        }
    };
    match owned(err) {
        Some(message) => Err(message),
        None => Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) })),
    }
}

/// `dr_precision_doc_artwork_metadata`; NULL `description` clears.
fn metadata(doc: &PrecisionDocument, layer: usize, description: Option<&str>) -> PrecisionDocument {
    let text = description.map(|d| CString::new(d).unwrap());
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork_metadata(
            doc,
            layer,
            text.as_ref().map_or(ptr::null(), |t| t.as_ptr()),
            LIMIT,
            0,
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null(), "the metadata changes");
    *unsafe { Box::from_raw(out) }
}

/// `dr_precision_doc_artwork_description`.
fn description(doc: &PrecisionDocument, layer: usize) -> Option<String> {
    owned(unsafe { dr_precision_doc_artwork_description(doc, layer) })
}

/// `dr_precision_doc_paste_layers` above record 0 at (0, 0).
fn paste(
    doc: &PrecisionDocument,
    clip: &PrecisionDocument,
    preserve: bool,
) -> (PrecisionDocument, Value) {
    let (mut report, mut err) = (ptr::null_mut(), ptr::null_mut());
    let out = unsafe {
        dr_precision_doc_paste_layers(
            doc,
            clip,
            0,
            0,
            0,
            0,
            preserve,
            LIMIT,
            0,
            &mut report,
            &mut err,
        )
    };
    assert_eq!(owned(err), None);
    assert!(!out.is_null());
    let report = serde_json::from_str(&owned(report).expect("a report")).unwrap();
    let out = *unsafe { Box::from_raw(out) };
    assert!(out.is_float());
    assert_eq!(out.float_refusal(), None);
    (out, report)
}

/// A `w × h` float document of one opaque record in `profile`.
fn base(w: u32, h: u32, profile: &Arc<IccProfile>) -> PrecisionDocument {
    float_doc(
        w,
        h,
        profile,
        &[vec![[0.25, 0.5, 0.75, 1.0]; (w * h) as usize]],
    )
}

fn floats(image: &PrecisionImage) -> Vec<[f32; 4]> {
    image.to_rgba32().expect("float samples")
}

fn bits(pixels: &[[f32; 4]]) -> Vec<[u32; 4]> {
    pixels.iter().map(|p| p.map(f32::to_bits)).collect()
}

fn layer_bits(doc: &PrecisionDocument, index: usize) -> Vec<[u32; 4]> {
    bits(&floats(doc.layer_source(index).expect("a raster record")))
}

/// `inv(dst.to_pcs) · src.to_pcs`.
fn combined(src: &Chain, dst: &Chain) -> [[f64; 3]; 3] {
    let inv = inverse(dst.to_pcs);
    std::array::from_fn(|r| {
        std::array::from_fn(|c| (0..3).map(|k| inv[r][k] * src.to_pcs[k][c]).sum())
    })
}

fn assert_near(got: [f32; 4], want: [f64; 4], what: &str) {
    for c in 0..4 {
        let bound = TOLERANCE * want[c].abs().max(1.0);
        let d = (f64::from(got[c]) - want[c]).abs();
        assert!(d <= bound, "{what} channel {c}: {} vs {}", got[c], want[c]);
    }
}

/// A coverage ramp touching 0, 1, the midpoint and 65535.
fn ramp(n: usize) -> Vec<u16> {
    (0..n)
        .map(|i| match i % 7 {
            0 => 0,
            1 => 65535,
            2 => 32768,
            3 => 1,
            _ => ((i * 7919) % 65536) as u16,
        })
        .collect()
}

/// The f32 image of one coverage component of `rgb` over `ramp`.
fn one_coverage(w: u32, h: u32, rgb: [f64; 3], profile: &IccProfile) -> PrecisionImage {
    render_float(
        w,
        h,
        &json!([{"kind": "coverage", "color": [rgb[0], rgb[1], rgb[2], 1.0]}]),
        &ramp((w * h) as usize),
        &[],
        profile,
    )
}

// ------------------------------------------------------------ the render --

#[test]
fn a_coverage_colour_is_linear_light_through_the_document_matrix() {
    let s = Chain::of(&IccProfile::srgb());
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        let m = combined(&s, &Chain::of(&profile));
        let (w, h) = (9, 5);
        let image = one_coverage(w, h, F, &profile);
        assert_eq!(image.profile().bytes(), profile.bytes());
        let rgb = apply(m, F);
        for (i, (got, cov)) in floats(&image)
            .into_iter()
            .zip(ramp((w * h) as usize))
            .enumerate()
        {
            if cov == 0 {
                assert_eq!(got, [0.0; 4], "pixel {i}: no coverage");
                continue;
            }
            assert_near(
                got,
                [rgb[0], rgb[1], rgb[2], f64::from(cov) / 65535.0],
                &format!("pixel {i}"),
            );
        }
    }
}

#[test]
fn inks_mix_in_linear_light_and_a_glyph_pixel_decodes_through_srgb() {
    let s = Chain::of(&IccProfile::srgb());
    let a = [0.9, 0.1, 0.2];
    let b = [0.1, 0.8, 0.3];
    let c = [0.4, 0.2, 0.9];
    let glyph = [200u8, 30, 90, 128];
    let opacity = 0.75;
    for profile in [IccProfile::srgb(), IccProfile::display_p3()] {
        let m = combined(&s, &Chain::of(&profile));
        let (w, h) = (4, 2);
        let n = (w * h) as usize;
        let mut samples = vec![65535u16; n];
        samples.extend(vec![32768u16; n]);
        samples.extend(vec![32768u16; n]);
        let intrinsic: Vec<u8> = (0..n).flat_map(|_| glyph).collect();
        let image = render_float(
            w,
            h,
            &json!([
                {"kind": "coverage", "color": [a[0], a[1], a[2], 1.0]},
                {"kind": "coverage", "color": [b[0], b[1], b[2], 1.0]},
                {"kind": "coverage", "color": [c[0], c[1], c[2], 1.0]},
                {"kind": "intrinsic_rgba8", "opacity": opacity},
            ]),
            &samples,
            &intrinsic,
            &profile,
        );
        // The W3C `over` onto an opaque backdrop, every value linear.
        let over = |backdrop: [f64; 3], ink: [f64; 3], alpha: f64| -> [f64; 3] {
            std::array::from_fn(|k| ink[k] * alpha + backdrop[k] * (1.0 - alpha))
        };
        let half = 32768.0 / 65535.0;
        let mut want = apply(m, a);
        want = over(want, apply(m, b), half);
        want = over(want, apply(m, c), half);
        let decoded: [f64; 3] = std::array::from_fn(|k| s.decode(f64::from(glyph[k]) / 255.0));
        want = over(
            want,
            apply(m, decoded),
            opacity * f64::from(glyph[3]) / 255.0,
        );
        for (i, got) in floats(&image).into_iter().enumerate() {
            assert_near(got, [want[0], want[1], want[2], 1.0], &format!("pixel {i}"));
        }
    }
}

/// A LUT profile: the 16-bit render converts through the LUT pipeline
/// (`lut_authored_colors.rs` holds its oracle), and a float document is
/// matrix-only, so the float render still refuses.
#[test]
fn a_lut_profile_renders_at_sixteen_bits_and_refuses_at_float() {
    let lut = build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic LUT RGB")),
            (b"wtpt", tag_wtpt([0.9642, 1.0, 0.8249])),
            (b"A2B0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
            (b"B2A0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
        ],
    );
    let lut = IccProfile::parse(&lut).unwrap();
    let components = json!([{"kind": "coverage", "color": [0.5, 0.5, 0.5, 1.0]}]);
    let samples = vec![65535u16; 4];
    assert!(
        render_at(false, 2, 2, &components, &samples, &[], &lut, LIMIT).is_ok(),
        "the 16-bit render converts into the LUT"
    );
    let float = render_at(true, 2, 2, &components, &samples, &[], &lut, LIMIT)
        .err()
        .expect("the float render refuses");
    assert!(float.contains("matrix/TRC"), "{float}");
}

/// The FFI admits `input + scratch` where scratch is `8 · bookkeeping +
/// render_float_scratch(n) (= n · 16) + profile · 64 + description · 32`.
#[test]
fn the_float_render_is_admitted_at_its_declared_scratch_and_not_a_byte_below() {
    let profile = IccProfile::srgb();
    let (w, h) = (64u32, 48u32);
    let n = u64::from(w * h);
    let components = json!([{"kind": "coverage", "color": [F[0], F[1], F[2], 1.0]}]);
    let text = components.to_string();
    let samples = ramp(n as usize);
    let input = n * 2 + profile.bytes().len() as u64 + text.len() as u64;
    let scratch =
        8 * BOOKKEEPING + n * 16 + profile.bytes().len() as u64 * 64 + text.len() as u64 * 32;
    let at = input + scratch;
    assert!(render_at(true, w, h, &components, &samples, &[], &profile, at).is_ok());
    let refused = render_at(true, w, h, &components, &samples, &[], &profile, at - 1)
        .err()
        .expect("one byte below refuses");
    assert_eq!(
        refused,
        format!(
            "precision operation needs {at} bytes including retained inputs; working limit is {} bytes",
            at - 1
        )
    );
}

// ------------------------------------------------------- publication --

#[test]
fn a_float_document_publishes_replaces_and_clears_f32_artwork() {
    let srgb = IccProfile::srgb();
    let doc = base(12, 8, &srgb);
    let image = one_coverage(6, 4, F, &srgb);
    let published = publish(&doc, 0, false, None, &image, SHAPE)
        .unwrap()
        .expect("a new record");
    assert!(published.is_float());
    assert_eq!(published.float_refusal(), None);
    assert_eq!(description(&published, 1).as_deref(), Some(SHAPE));
    assert_eq!(published.layer(1).unwrap().offset(), (1, 2));
    assert_eq!(layer_bits(&published, 1), bits(&floats(&image)));

    // The positional twin lands above the anchor the same way.
    let at = publish(&doc, 0, false, Some(0), &image, TEXT)
        .unwrap()
        .expect("a new record");
    assert_eq!(at.float_refusal(), None);
    assert_eq!(description(&at, 1).as_deref(), Some(TEXT));

    // A u16 image refuses in a float document.
    let sixteen = render_at(
        false,
        6,
        4,
        &json!([{"kind": "coverage", "color": [0.5, 0.5, 0.5, 1.0]}]),
        &ramp(24),
        &[],
        &srgb,
        LIMIT,
    )
    .unwrap();
    assert_eq!(
        publish(&doc, 0, false, None, &sixteen, SHAPE).err(),
        Some(STORAGE.to_string())
    );
    // So does an f32 image in another profile.
    let p3 = one_coverage(6, 4, F, &IccProfile::display_p3());
    assert_eq!(
        publish(&doc, 0, false, None, &p3, SHAPE).err(),
        Some(STORAGE.to_string())
    );

    // Replacing re-renders the record's pixels.
    let recoloured = one_coverage(6, 4, [0.7, 0.2, 0.1], &srgb);
    let replaced = publish(&published, 1, true, None, &recoloured, SHAPE)
        .unwrap()
        .expect("the replacement changes the record");
    assert_eq!(layer_bits(&replaced, 1), bits(&floats(&recoloured)));
    assert_eq!(replaced.float_refusal(), None);
    // The same pixels and description again are a no-op.
    assert!(publish(&replaced, 1, true, None, &recoloured, SHAPE)
        .unwrap()
        .is_none());

    // Rasterize Artwork: NULL clears the description, the samples kept.
    let cleared = metadata(&replaced, 1, None);
    assert_eq!(description(&cleared, 1), None);
    assert_eq!(layer_bits(&cleared, 1), layer_bits(&replaced, 1));
    assert_eq!(cleared.float_refusal(), None);
}

#[test]
fn a_described_float_document_reopens_bit_exact() {
    let srgb = IccProfile::srgb();
    let doc = publish(
        &base(10, 7, &srgb),
        0,
        false,
        None,
        &one_coverage(5, 3, F, &srgb),
        SHAPE,
    )
    .unwrap()
    .unwrap();
    let bytes = doc.encode_native(LIMIT, 0, None).unwrap();
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert!(reopened.is_float());
    assert_eq!(reopened.float_refusal(), None);
    assert_eq!(description(&reopened, 1).as_deref(), Some(SHAPE));
    for i in 0..2 {
        assert_eq!(layer_bits(&reopened, i), layer_bits(&doc, i), "layer {i}");
    }
    assert_eq!(reopened.encode_native(LIMIT, 0, None).unwrap(), bytes);
}

// --------------------------------------------------- the operator table --

#[test]
fn text_and_shape_layers_are_admitted_operators_and_rasterize_is_a_float_family() {
    let table: Value = serde_json::from_str(&float_operators_json()).unwrap();
    let admitted = table["admitted"].as_array().unwrap();
    assert!(admitted.contains(&json!("text_layer")));
    assert!(admitted.contains(&json!("shape_layer")));
    assert!(!table["refused"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["operator"].as_str().unwrap().contains("text or shape")));
    let row = capability("rasterize", "document", 32);
    assert_eq!(row["supported"], true, "{row}");
    assert_eq!(row["reason"], Value::Null);
    assert_eq!(capability("rasterize", "raster", 32)["supported"], false);
}

// ------------------------------------------------------------ narrowing --

fn described_float() -> PrecisionDocument {
    let srgb = IccProfile::srgb();
    let doc = publish(
        &base(10, 7, &srgb),
        0,
        false,
        None,
        &one_coverage(5, 3, F, &srgb),
        SHAPE,
    )
    .unwrap()
    .unwrap();
    publish(
        &doc,
        1,
        false,
        None,
        &one_coverage(4, 4, [0.737, 0.251, 0.063], &srgb),
        TEXT,
    )
    .unwrap()
    .unwrap()
}

fn notes(doc: &PrecisionDocument, target: NarrowTarget) -> Vec<(String, String)> {
    doc.narrow_structure_plan(target)
        .unwrap()
        .rendering_differences
        .into_iter()
        .map(|row| (row.name, row.note))
        .collect()
}

#[test]
fn narrowing_to_sixteen_bits_keeps_each_description_verbatim_with_its_note() {
    let doc = described_float();
    let sixteen = doc.narrow_structure_to_u16(LIMIT, 0, None).unwrap();
    assert!(!sixteen.is_float());
    assert_eq!(description(&sixteen, 1).as_deref(), Some(SHAPE));
    assert_eq!(description(&sixteen, 2).as_deref(), Some(TEXT));
    assert_eq!(description(&sixteen, 0), None);
    let rows = notes(&doc, NarrowTarget::Rgba16);
    let note = "text or shape keeps its description; it re-renders at 16 bits on the next edit";
    assert_eq!(
        rows.iter().filter(|(_, n)| n == note).count(),
        2,
        "{rows:?}"
    );
    assert!(rows.iter().any(|(name, n)| name == "Artwork" && n == note));
}

#[test]
fn narrowing_to_eight_bits_keeps_each_description_verbatim_with_its_note() {
    let doc = described_float();
    let eight = doc
        .narrow_structure_to_u8(PrecisionDither::None, LIMIT, 0, None)
        .unwrap();
    assert_eq!(eight.layers[1].meta.as_deref(), Some(SHAPE));
    assert_eq!(eight.layers[2].meta.as_deref(), Some(TEXT));
    assert_eq!(eight.layers[0].meta, None);
    let rows = notes(&doc, NarrowTarget::Rgba8);
    let note = "text or shape keeps its description; it re-renders at 8 bits on the next edit";
    assert_eq!(
        rows.iter().filter(|(_, n)| n == note).count(),
        2,
        "{rows:?}"
    );
}

// ---------------------------------------------------------------- paste --

/// A 16-bit document in `profile`: a white "Background" and a described
/// "Text" record above it.
fn described16(profile: &Arc<IccProfile>) -> PrecisionDocument {
    let (w, h) = (6, 5);
    let codes = |v: u16| vec![[v, v / 2, 65535 - v, 65535]; (w * h) as usize];
    let layer = |name: &str, v: u16| {
        Layer::raster(
            name,
            PrecisionImage::from_rgba16(w, h, codes(v), profile.clone(), None).unwrap(),
            (0, 0),
            1.0,
            true,
        )
        .unwrap()
    };
    let doc = PrecisionDocument::from_parts(
        w,
        h,
        profile.clone(),
        Metadata::default(),
        Resolution::default(),
        vec![layer("Background", 65535), layer("Text", 20000)],
    )
    .unwrap();
    metadata(&doc, 1, Some(TEXT))
}

#[test]
fn a_described_sixteen_bit_record_lands_described_with_its_bytes_verbatim() {
    let srgb = IccProfile::srgb();
    let dest = base(6, 5, &srgb);
    let clip = described16(&srgb);
    for preserve in [false, true] {
        let (out, report) = paste(&dest, &clip, preserve);
        assert_eq!(description(&out, 2).as_deref(), Some(TEXT), "{preserve}");
        assert_eq!(description(&out, 1), None);
        assert_eq!(report["rasterized"], json!([]), "{preserve}");
    }
    // Under Preserve a record of another profile is relabelled, so its
    // description no longer describes its pixels: it drops with its row.
    let p3 = described16(&IccProfile::display_p3());
    let (kept, report) = paste(&dest, &p3, true);
    assert_eq!(description(&kept, 2), None);
    assert_eq!(
        report["rasterized"],
        json!([{"layer": "Text", "reason": "text or shape"}])
    );
    // Under Convert it is converted, and lands described.
    let (converted, report) = paste(&dest, &p3, false);
    assert_eq!(description(&converted, 2).as_deref(), Some(TEXT));
    assert_eq!(report["rasterized"], json!([]));
}

// ------------------------------------------------------------ above white --

/// The float render's refusal for a colour outside its range.
const ABOVE_WHITE_REFUSAL: &str =
    "float artwork color must be finite with red, green and blue in 0..=65504 and alpha in 0..=1";

/// A 2 x 2 fully covered render of one coverage colour.
fn full_coverage(float: bool, color: Value) -> Result<PrecisionImage, String> {
    render_at(
        float,
        2,
        2,
        &json!([{"kind": "coverage", "color": color}]),
        &[65535; 4],
        &[],
        &IccProfile::srgb(),
        LIMIT,
    )
}

#[test]
fn an_above_white_colour_renders_its_linear_numbers_at_float() {
    // Photoshop 26.10.0 renders RGBFloatColor (4, 0.5, 0.25) as exactly
    // those linear values (hdr-colour-317-608 take2, row B1); on an sRGB
    // document the sRGB -> document matrix is the identity.
    let image = full_coverage(true, json!([4.0, 0.5, 0.25, 1.0])).expect("admitted at float");
    for (i, got) in floats(&image).into_iter().enumerate() {
        assert_near(got, [4.0, 0.5, 0.25, 1.0], &format!("pixel {i}"));
    }
    let top = full_coverage(true, json!([65504.0, 0.0, 1.0, 0.5])).expect("65504 is admitted");
    assert_near(floats(&top)[0], [65504.0, 0.0, 1.0, 0.5], "65504");
}

#[test]
fn a_float_colour_outside_its_range_refuses_and_sixteen_bits_stay_normalized() {
    for color in [
        json!([65505.0, 0.5, 0.25, 1.0]),
        json!([4.0, -0.01, 0.25, 1.0]),
        json!([4.0, 0.5, 1e39, 1.0]),
        json!([4.0, 0.5, 0.25, 1.01]),
    ] {
        let err = full_coverage(true, color.clone())
            .err()
            .expect("refused at float");
        assert!(err.contains(ABOVE_WHITE_REFUSAL), "{color}: {err}");
    }
    // NaN has no JSON form: it cannot reach the range check at all.
    assert!(full_coverage(true, json!([f64::NAN, 0.5, 0.25, 1.0])).is_err());
    let err = full_coverage(false, json!([1.5, 0.5, 0.25, 1.0]))
        .err()
        .expect("refused at 16");
    assert!(
        err.contains("artwork color must be finite normalized sRGB"),
        "{err}"
    );
}
