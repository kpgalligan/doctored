//! Type, shape and fill layers on a CMYK document (#547's artwork batch,
//! `cmyk_artwork` / `cmyk_artwork_render`), through the FFI.
//!
//! The oracles are independent of the implementation: a fill and a shape's
//! interior are the stored ink exactly (`round(v · 65535)`); an edge's alpha
//! is the RGB render's own alpha on the same coverage pixel
//! (`dr_artwork_render`, C-same); an intrinsic glyph pixel's ink is
//! `dr_cmyk_color`'s for its sRGB; a two-stop Linear gradient is the
//! closed-form Classic ramp (`common::classic_ramp`) at each pixel
//! corner's parameter. Structure paths are checked by the
//! description they leave (`artwork_description`), the `.rz` word by its
//! bytes, and the Mode plans' `flattens` against the conversion's own result.

use std::ffi::{CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_artwork::{
    dr_artwork_render, dr_precision_doc_artwork_at, dr_precision_doc_artwork_metadata,
};
use doctored_core::ffi_cmyk::{
    dr_cmyk_color, dr_precision_doc_convert_cmyk_to_rgb, dr_precision_doc_convert_cmyk_to_rgb_plan,
    dr_precision_doc_convert_to_cmyk, dr_precision_doc_convert_to_cmyk_plan,
};
use doctored_core::ffi_cmyk_artwork::{dr_cmyk_artwork_colors, dr_precision_doc_cmyk_artwork};
use doctored_core::ffi_cmyk_colour::dr_precision_doc_cmyk_profile_edit;
use doctored_core::ffi_layer_clipboard::dr_precision_doc_paste_layers;
use doctored_core::ffi_precision::{dr_precision_doc_duplicate_layer, dr_precision_image_free};
use doctored_core::ffi_psd::dr_precision_doc_psd_export_report;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::press_profile;
use crate::cmyk_paste_fixture::{cmyk_layers_in, second_press_profile};
use crate::cmyk_structure_fixture::{
    cmyk_layers, cmyk_word, free, json, open, ramp, save, then, try_edit, varied, with_levels,
    with_mask, RgbLayer, LIMIT,
};
use crate::common::{classic_ramp, take_err_string};

const FILL: &str = r#"{"type":"fill","version":2,"ink":"cmyk"}"#;
const SHAPE: &str = r#"{"type":"shape","version":6}"#;
const TEXT: &str = r#"{"type":"text","version":5}"#;

/// `cmyk_composite::code`'s rule on an f32 ink.
fn code(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// A `w × h` CMYK document (the press profile, 16 bits) of one varied layer.
fn base(w: u32, h: u32) -> *mut PrecisionDocument {
    cmyk_layers(
        w,
        h,
        vec![RgbLayer {
            name: "Bottom",
            w,
            h,
            offset: (0, 0),
            pixels: varied(w, h, 1),
        }],
    )
}

/// `dr_precision_doc_cmyk_artwork`; `Ok(null)` for a no-op.
#[allow(clippy::too_many_arguments)]
fn publish(
    doc: *const PrecisionDocument,
    layer: usize,
    replacing: bool,
    size: (u32, u32),
    components: &str,
    samples: &[u16],
    intrinsic: &[u8],
    name: &str,
    description: &str,
) -> Result<*mut PrecisionDocument, String> {
    let components = CString::new(components).unwrap();
    let name = CString::new(name).unwrap();
    let description = CString::new(description).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_cmyk_artwork(
            doc,
            layer,
            replacing,
            0,
            size.0,
            size.1,
            components.as_ptr(),
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
            0,
            0,
            name.as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    if out.is_null() && !err.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn fill_components(ink: [f32; 5]) -> String {
    format!(r#"[{{"kind":"fill","ink":{ink:?}}}]"#)
}

/// `doc` with a canvas-sized fill layer of `ink` above layer 0.
fn with_fill(doc: *const PrecisionDocument, ink: [f32; 5]) -> *mut PrecisionDocument {
    let (w, h) = unsafe { &*doc }.dimensions();
    let out = publish(
        doc,
        0,
        false,
        (w, h),
        &fill_components(ink),
        &[],
        &[],
        "Color Fill",
        FILL,
    )
    .unwrap();
    assert!(!out.is_null());
    out
}

fn samples_of(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    unsafe { &*doc }.cmyk_layer_samples(index).unwrap()
}

fn described(doc: *const PrecisionDocument, index: usize) -> Option<String> {
    unsafe { &*doc }
        .artwork_description(index)
        .map(str::to_string)
}

fn take_json(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    json(text, err)
}

/// `dr_cmyk_artwork_colors` with profile ends (empty = sRGB).
fn colors(source: &[u8], destination: &[u8], request: &str) -> Result<Value, String> {
    let request = CString::new(request).unwrap();
    let mut err = ptr::null_mut();
    let text = unsafe {
        dr_cmyk_artwork_colors(
            if source.is_empty() {
                ptr::null()
            } else {
                source.as_ptr()
            },
            source.len(),
            if destination.is_empty() {
                ptr::null()
            } else {
                destination.as_ptr()
            },
            destination.len(),
            request.as_ptr(),
            &mut err,
        )
    };
    if text.is_null() {
        return Err(take_err_string(err));
    }
    Ok(take_json(text, err))
}

fn numbers(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

/// A coverage plane: opaque inside `[1, w−1) × [1, h−1)`, partial edges, 0
/// in the corners.
fn shape_coverage(w: u32, h: u32) -> Vec<u16> {
    (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                let inside_x = x >= 1 && x + 1 < w;
                let inside_y = y >= 1 && y + 1 < h;
                match (inside_x, inside_y) {
                    (true, true) => 65535,
                    (true, false) | (false, true) => 20000 + 3001 * (x + y),
                    (false, false) => 0,
                }
                .min(65535) as u16
            })
        })
        .collect()
}

#[test]
fn a_fill_and_a_shape_interior_are_the_stored_ink_exactly() {
    let (w, h) = (6u32, 4u32);
    let doc = base(w, h);
    let ink = [0.2f32, 0.4, 0.6, 0.8, 1.0];
    let filled = with_fill(doc, ink);
    assert_eq!(unsafe { &*filled }.layer_count(), 2);
    assert_eq!(described(filled, 1).as_deref(), Some(FILL));
    assert!(samples_of(filled, 1).iter().all(|s| *s == ink.map(code)));
    // A shape: one coverage component at alpha 0.6, the interior exact.
    let shape_ink = [0.8f32, 0.2, 0.0, 0.4, 0.6];
    let coverage = shape_coverage(w, h);
    let components = format!(r#"[{{"kind":"coverage","ink":{shape_ink:?}}}]"#);
    let shaped = publish(
        doc,
        0,
        false,
        (w, h),
        &components,
        &coverage,
        &[],
        "Shape",
        SHAPE,
    )
    .unwrap();
    let got = samples_of(shaped, 1);
    for (i, sample) in got.iter().enumerate() {
        if coverage[i] == 65535 {
            assert_eq!(*sample, shape_ink.map(code), "interior pixel {i}");
        }
        if coverage[i] == 0 {
            assert_eq!(sample[4], 0, "outside pixel {i}");
        }
    }
    free(shaped);
    free(filled);
    free(doc);
}

#[test]
fn edge_alpha_is_the_rgb_renders_alpha_on_the_same_coverage() {
    let (w, h) = (9u32, 7u32);
    let n = (w * h) as usize;
    let coverage: Vec<u16> = (0..n as u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) % 65536) as u16)
        .collect();
    let alpha = 0.7f32;
    let rgb_components = CString::new(format!(
        r#"[{{"kind":"coverage","color":[0.3,0.5,0.7,{alpha}]}}]"#
    ))
    .unwrap();
    let srgb = IccProfile::srgb().bytes().to_vec();
    let mut err = ptr::null_mut();
    let image = unsafe {
        dr_artwork_render(
            w,
            h,
            rgb_components.as_ptr(),
            coverage.as_ptr(),
            n,
            ptr::null(),
            0,
            srgb.as_ptr(),
            srgb.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!image.is_null(), "{}", take_err_string(err));
    let rgb = unsafe { &*image }.to_rgba16().unwrap();
    let doc = base(w, h);
    let components = format!(r#"[{{"kind":"coverage","ink":[0.1,0.2,0.3,0.4,{alpha}]}}]"#);
    let out = publish(
        doc,
        0,
        false,
        (w, h),
        &components,
        &coverage,
        &[],
        "Shape",
        SHAPE,
    )
    .unwrap();
    for (i, (ink, rgb)) in samples_of(out, 1).iter().zip(&rgb).enumerate() {
        assert_eq!(
            ink[4], rgb[3],
            "alpha at pixel {i} (coverage {})",
            coverage[i]
        );
    }
    unsafe { dr_precision_image_free(image) };
    free(out);
    free(doc);
}

#[test]
fn an_intrinsic_glyph_pixel_is_its_srgbs_ink_through_the_profile() {
    let (w, h) = (4u32, 1u32);
    let pixels: [[u8; 4]; 4] = [
        [255, 0, 0, 255],
        [10, 200, 90, 255],
        [40, 40, 40, 128],
        [0, 0, 0, 0],
    ];
    let intrinsic: Vec<u8> = pixels.iter().flatten().copied().collect();
    let doc = base(w, h);
    let out = publish(
        doc,
        0,
        false,
        (w, h),
        r#"[{"kind":"intrinsic_rgba8","opacity":1}]"#,
        &[],
        &intrinsic,
        "Emoji",
        TEXT,
    )
    .unwrap();
    let got = samples_of(out, 1);
    for (pixel, sample) in pixels.iter().zip(&got) {
        if pixel[3] == 0 {
            assert_eq!(*sample, [0; 5]);
            continue;
        }
        let request = CString::new(format!(
            r#"{{"from":"srgb","values":[{},{},{}]}}"#,
            f64::from(pixel[0]) / 255.0,
            f64::from(pixel[1]) / 255.0,
            f64::from(pixel[2]) / 255.0
        ))
        .unwrap();
        let mut err = ptr::null_mut();
        let answer = take_json(
            unsafe { dr_cmyk_color(doc, request.as_ptr(), &mut err) },
            err,
        );
        let ink = numbers(&answer["ink"]);
        let want: Vec<u16> = ink.iter().map(|v| code(*v as f32)).collect();
        assert_eq!(&sample[..4], &want[..], "{pixel:?}");
        assert_eq!(sample[4], code(f32::from(pixel[3]) / 255.0));
    }
    free(out);
    free(doc);
}

#[test]
fn a_two_stop_gradient_is_the_closed_form_lerp_within_one_code() {
    let (w, h) = (50u32, 2u32);
    let doc = base(w, h);
    let a = [0.9f64, 0.1, 0.0, 0.0];
    let b = [0.0f64, 0.3, 0.6, 1.0];
    let gradient = r##"{"stops":[{"position":0,"color":"#000000","opacity":1},{"position":1,"color":"#000000","opacity":1}],"style":"linear","angle":0,"scale":1,"reverse":false,"align_with_layer":false}"##;
    let components =
        format!(r#"[{{"kind":"gradient","gradient":{gradient},"inks":[{a:?},{b:?}]}}]"#);
    let out = publish(
        doc,
        0,
        false,
        (w, h),
        &components,
        &[],
        &[],
        "Gradient Fill",
        FILL,
    )
    .unwrap();
    for (i, sample) in samples_of(out, 1).iter().enumerate() {
        // The top-left corner, Linear's sample point.
        let t = f64::from(i as u32 % w) / f64::from(w);
        for c in 0..4 {
            let want = (classic_ramp(&[(0.0, a[c]), (1.0, b[c])], t) * 65535.0).round();
            assert!(
                (f64::from(sample[c]) - want).abs() <= 1.0,
                "pixel {i} plate {c}: {} vs {want}",
                sample[c]
            );
        }
        assert_eq!(sample[4], 65535);
    }
    // One ink per stop, as the RGB render's one colour per stop.
    let short = format!(r#"[{{"kind":"gradient","gradient":{gradient},"inks":[{a:?}]}}]"#);
    assert_eq!(
        publish(doc, 0, false, (w, h), &short, &[], &[], "G", FILL).unwrap_err(),
        "artwork gradient requires one color per stop"
    );
    free(out);
    free(doc);
}

#[test]
fn a_replacement_keeps_the_mask_a_no_op_is_null_and_a_lock_refuses() {
    let (w, h) = (5u32, 4u32);
    let doc = base(w, h);
    let filled = with_fill(doc, [0.2, 0.2, 0.2, 0.2, 1.0]);
    let mask = ramp(w, h, 3);
    let masked = with_mask(filled, 1, &mask);
    let ink = [0.6f32, 0.0, 0.4, 0.2, 1.0];
    let replaced = publish(
        masked,
        1,
        true,
        (w, h),
        &fill_components(ink),
        &[],
        &[],
        "x",
        TEXT,
    )
    .unwrap();
    assert!(!replaced.is_null());
    let layer = unsafe { &*replaced }.layer(1).unwrap().clone();
    assert_eq!(layer.mask().unwrap().plane().samples(), &mask[..]);
    assert_eq!(
        layer.name(),
        "Color Fill",
        "a replacement keeps the record's name"
    );
    assert_eq!(described(replaced, 1).as_deref(), Some(TEXT));
    assert!(samples_of(replaced, 1).iter().all(|s| *s == ink.map(code)));
    // The same pixels, offset and description again change nothing.
    let again = publish(
        replaced,
        1,
        true,
        (w, h),
        &fill_components(ink),
        &[],
        &[],
        "x",
        TEXT,
    );
    assert!(again.unwrap().is_null());
    // Lock Pixels refuses the replacement by name; a new record is ungated.
    let locked = then(replaced, r#"{"op":"layer_lock","layer":1,"locks":2}"#, &[]);
    let refused = publish(
        locked,
        1,
        true,
        (w, h),
        &fill_components(ink),
        &[],
        &[],
        "x",
        FILL,
    )
    .unwrap_err();
    assert!(refused.contains("lock"), "{refused}");
    let added = publish(
        locked,
        1,
        false,
        (w, h),
        &fill_components(ink),
        &[],
        &[],
        "y",
        FILL,
    )
    .unwrap();
    assert_eq!(unsafe { &*added }.layer_count(), 3);
    free(added);
    free(locked);
    free(masked);
    free(doc);
}

#[test]
fn duplicate_keeps_the_description_and_merge_flatten_and_paint_drop_it() {
    let (w, h) = (4u32, 3u32);
    let n = (w * h) as usize;
    let doc = base(w, h);
    let filled = with_fill(doc, [0.4, 0.2, 0.0, 0.0, 0.8]);
    let mut err = ptr::null_mut();
    let duplicated = unsafe { dr_precision_doc_duplicate_layer(filled, 1, &mut err) };
    assert!(!duplicated.is_null(), "{}", take_err_string(err));
    assert_eq!(described(duplicated, 2).as_deref(), Some(FILL));
    assert_eq!(described(duplicated, 1).as_deref(), Some(FILL));
    let merged = try_edit(filled, r#"{"op":"merge_down","layer":1}"#, &[]).unwrap();
    assert_eq!(unsafe { &*merged }.layer_count(), 1);
    assert_eq!(described(merged, 0), None);
    let flat = try_edit(filled, r#"{"op":"flatten"}"#, &[]).unwrap();
    assert_eq!(described(flat, 0), None);
    let stroke = vec![65535u16; n];
    let painted = try_edit(
        filled,
        r#"{"op":"paint_blend","layer":1,"ink":[0.0,0.0,0.0,1.0,1.0],"opacity":1}"#,
        &stroke,
    )
    .unwrap();
    assert!(!painted.is_null());
    assert_eq!(described(painted, 1), None);
    for d in [duplicated, merged, flat, painted, filled, doc] {
        free(d);
    }
}

#[test]
fn drdc_87_round_trips_the_description_and_the_words_claim_both_ways() {
    let dir = TempDir::new().unwrap();
    let (w, h) = (6u32, 4u32);
    let doc = base(w, h);
    let filled = with_fill(doc, [0.2, 0.4, 0.6, 0.8, 1.0]);
    let masked = with_mask(filled, 1, &ramp(w, h, 5));
    let path = dir.path().join("artwork.rz");
    let bytes = save(masked, &path);
    assert_eq!(cmyk_word(&bytes), 87, "a description writes DRDC 87");
    let reopened = open(&path).unwrap();
    assert_eq!(described(reopened, 1).as_deref(), Some(FILL));
    assert_eq!(samples_of(reopened, 1), samples_of(masked, 1));
    assert!(unsafe { &*reopened }.layer(1).unwrap().mask().is_some());
    // A pre-87 word holding a description refuses by name.
    let mut doctored = bytes.clone();
    doctored[8..12].copy_from_slice(&85u32.to_le_bytes());
    let at = dir.path().join("doctored85.rz");
    std::fs::write(&at, &doctored).unwrap();
    assert_eq!(
        open(&at).unwrap_err(),
        "a CMYK text, shape or fill layer requires DRDC 87"
    );
    // An 87 holding none refuses as an invalid word.
    let plain_path = dir.path().join("plain.rz");
    let mut plain = save(doc, &plain_path);
    assert_eq!(cmyk_word(&plain), 82);
    plain[8..12].copy_from_slice(&87u32.to_le_bytes());
    let at = dir.path().join("doctored87.rz");
    std::fs::write(&at, &plain).unwrap();
    assert_eq!(
        open(&at).unwrap_err(),
        "invalid native CMYK description word"
    );
    free(reopened);
    free(masked);
    free(doc);
}

/// An RGB document of `layers` (u16 sRGB), bottom first.
fn rgb_doc(w: u32, h: u32, names: &[&str]) -> *mut PrecisionDocument {
    let srgb = IccProfile::srgb();
    let records = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let image =
                PrecisionImage::from_rgba16(w, h, varied(w, h, i as u32 + 7), srgb.clone(), None)
                    .unwrap();
            PrecisionDocumentLayer::raster(name, image, (0, 0), 1.0, true).unwrap()
        })
        .collect();
    Box::into_raw(Box::new(
        PrecisionDocument::from_parts(
            w,
            h,
            srgb,
            Metadata::default(),
            Resolution::default(),
            records,
        )
        .unwrap(),
    ))
}

fn describe(doc: *mut PrecisionDocument, index: usize, text: &str) -> *mut PrecisionDocument {
    let text = CString::new(text).unwrap();
    let mut err = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_artwork_metadata(doc, index, text.as_ptr(), LIMIT, 0, &mut err) };
    assert!(!out.is_null(), "{}", take_err_string(err));
    free(doc);
    out
}

fn to_cmyk_plan(doc: *const PrecisionDocument, options: &str) -> Value {
    let profile = press_profile("Press Test");
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    take_json(
        unsafe {
            dr_precision_doc_convert_to_cmyk_plan(
                doc,
                profile.as_ptr(),
                profile.len(),
                options.as_ptr(),
                &mut err,
            )
        },
        err,
    )
}

fn to_cmyk(doc: *const PrecisionDocument, options: &str) -> *mut PrecisionDocument {
    let profile = press_profile("Press Test");
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            doc,
            profile.as_ptr(),
            profile.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

fn to_rgb_plan(doc: *const PrecisionDocument, options: &str) -> Value {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    take_json(
        unsafe {
            dr_precision_doc_convert_cmyk_to_rgb_plan(
                doc,
                ptr::null(),
                0,
                options.as_ptr(),
                &mut err,
            )
        },
        err,
    )
}

fn to_rgb(doc: *const PrecisionDocument, options: &str) -> *mut PrecisionDocument {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_cmyk_to_rgb(
            doc,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "{}", take_err_string(err));
    out
}

#[test]
fn a_description_no_longer_blocks_cmyk_color_and_flattens_is_what_happens() {
    let (w, h) = (4u32, 3u32);
    let text = r#"{"type":"text","version":3}"#;
    // Text alone: no blocker, so nothing flattens even under Flatten; the
    // layers carry and the description is dropped (the host re-renders it).
    let plain = describe(rgb_doc(w, h, &["Bottom", "Title"]), 1, text);
    let plan = to_cmyk_plan(plain, r#"{"flatten":true}"#);
    assert_eq!(plan["refusal"], Value::Null);
    assert_eq!(plan["flatten_required"], false);
    assert_eq!(plan["flattens"], false);
    let converted = to_cmyk(plain, r#"{"flatten":true}"#);
    assert_eq!(unsafe { &*converted }.layer_count(), 2);
    assert_eq!(described(converted, 1), None);
    // Text (index 1 of 3) and Levels: Levels is the blocker.
    let levelled = with_levels(describe(rgb_doc(w, h, &["Bottom", "Title"]), 1, text), 1.0);
    let refused = to_cmyk_plan(levelled, "{}");
    assert_eq!(refused["flatten_required"], true);
    assert_eq!(refused["flattens"], false);
    assert!(refused["refusal"]
        .as_str()
        .unwrap()
        .contains("adjustment layer"));
    let flattening = to_cmyk_plan(levelled, r#"{"flatten":true}"#);
    assert_eq!(flattening["flattens"], true);
    let flat = to_cmyk(levelled, r#"{"flatten":true}"#);
    assert_eq!(unsafe { &*flat }.layer_count(), 1);
    // Toward RGB: one root never flattens, two do under Flatten.
    let one = base(w, h);
    assert_eq!(to_rgb_plan(one, r#"{"flatten":true}"#)["flattens"], false);
    let rgb_one = to_rgb(one, r#"{"flatten":true}"#);
    assert_eq!(unsafe { &*rgb_one }.layer(0).unwrap().name(), "Bottom");
    let two = with_fill(one, [0.1, 0.1, 0.1, 0.1, 0.5]);
    assert_eq!(to_rgb_plan(two, "{}")["flattens"], false);
    assert_eq!(to_rgb_plan(two, r#"{"flatten":true}"#)["flattens"], true);
    let rgb_two = to_rgb(two, r#"{"flatten":true}"#);
    assert_eq!(unsafe { &*rgb_two }.layer_count(), 1);
    let kept = to_rgb(two, "{}");
    assert_eq!(unsafe { &*kept }.layer_count(), 2);
    assert_eq!(
        described(kept, 1),
        None,
        "CMYK -> RGB drops the description"
    );
    for d in [
        plain, converted, levelled, flat, one, rgb_one, two, rgb_two, kept,
    ] {
        free(d);
    }
}

#[test]
fn artwork_colors_agree_with_dr_cmyk_color_and_convert_to_profile() {
    let press = press_profile("Press Test");
    let doc = base(2, 1);
    // sRGB -> ink and ink -> sRGB: dr_cmyk_color's legs exactly.
    let rgb = [0.2f64, 0.5, 0.9];
    let to_ink = colors(&[], &press, &format!(r#"{{"values":[{rgb:?},[1,1,1]]}}"#)).unwrap();
    let request = CString::new(format!(r#"{{"from":"srgb","values":{rgb:?}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let single = take_json(
        unsafe { dr_cmyk_color(doc, request.as_ptr(), &mut err) },
        err,
    );
    assert_eq!(numbers(&to_ink["values"][0]), numbers(&single["ink"]));
    let ink = [0.1f64, 0.2, 0.3, 0.4];
    let to_srgb = colors(&press, &[], &format!(r#"{{"values":[{ink:?}]}}"#)).unwrap();
    let request = CString::new(format!(r#"{{"from":"ink","values":{ink:?}}}"#)).unwrap();
    let mut err = ptr::null_mut();
    let single = take_json(
        unsafe { dr_cmyk_color(doc, request.as_ptr(), &mut err) },
        err,
    );
    assert_eq!(numbers(&to_srgb["values"][0]), numbers(&single["srgb"]));
    // ink -> ink: Convert to Profile's own leg on a 1×1 layer.
    let second = second_press_profile("Second Press");
    let one = cmyk_layers_in(
        &press,
        16,
        1,
        1,
        vec![RgbLayer {
            name: "Dot",
            w: 1,
            h: 1,
            offset: (0, 0),
            pixels: vec![[20000, 40000, 9000, 65535]],
        }],
    );
    let before = samples_of(one, 0)[0];
    let request = c"{\"op\":\"convert\"}";
    let mut err = ptr::null_mut();
    let converted = unsafe {
        dr_precision_doc_cmyk_profile_edit(
            one,
            second.as_ptr(),
            second.len(),
            request.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!converted.is_null(), "{}", take_err_string(err));
    let after = samples_of(converted, 0)[0];
    let units: Vec<f64> = before[..4]
        .iter()
        .map(|v| f64::from(f32::from(*v) / 65535.0))
        .collect();
    let crossed = colors(&press, &second, &format!(r#"{{"values":[{units:?}]}}"#)).unwrap();
    let want: Vec<u16> = numbers(&crossed["values"][0])
        .iter()
        .map(|v| code(*v as f32))
        .collect();
    assert_eq!(&after[..4], &want[..]);
    // Identical profile bytes answer the values as they are.
    let same = colors(&press, &press, r#"{"values":[[0.25,0.5,0.75,1]]}"#).unwrap();
    assert_eq!(numbers(&same["values"][0]), vec![0.25, 0.5, 0.75, 1.0]);
    // Refusals by name.
    assert_eq!(
        colors(&[], &[], r#"{"values":[]}"#).unwrap_err(),
        "artwork colours convert to or from CMYK ink: both ends are sRGB"
    );
    let srgb = IccProfile::srgb().bytes().to_vec();
    assert!(colors(&srgb, &[], r#"{"values":[]}"#)
        .unwrap_err()
        .ends_with("is not a CMYK profile"));
    assert_eq!(
        colors(&[], &press, r#"{"values":[[0.1,0.2]]}"#).unwrap_err(),
        "each artwork colour must be 3 finite numbers"
    );
    assert_eq!(
        colors(&[], &press, r#"{"values":[],"gamma":1}"#).unwrap_err(),
        "artwork colour request has an unknown key \"gamma\""
    );
    for d in [doc, one, converted] {
        free(d);
    }
}

#[test]
fn every_refusal_names_itself() {
    let (w, h) = (3u32, 2u32);
    let n = (w * h) as usize;
    let doc = base(w, h);
    // RGB artwork has no place in a CMYK document.
    let srgb = IccProfile::srgb().bytes().to_vec();
    let rgb_components = c"[{\"kind\":\"fill\",\"color\":[0.5,0.5,0.5,1]}]";
    let mut err = ptr::null_mut();
    let image = unsafe {
        dr_artwork_render(
            w,
            h,
            rgb_components.as_ptr(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            srgb.as_ptr(),
            srgb.len(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!image.is_null(), "{}", take_err_string(err));
    let description = CString::new(FILL).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_artwork_at(
            doc,
            0,
            0,
            image,
            0,
            0,
            c"Fill".as_ptr(),
            description.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    assert_eq!(
        take_err_string(err),
        "RGB artwork is unavailable in a CMYK document"
    );
    unsafe { dr_precision_image_free(image) };
    // CMYK artwork has no place in an RGB document.
    let rgb = rgb_doc(w, h, &["Bottom"]);
    let ink = [0.5f32, 0.5, 0.5, 0.5, 1.0];
    assert_eq!(
        publish(
            rgb,
            0,
            false,
            (w, h),
            &fill_components(ink),
            &[],
            &[],
            "F",
            FILL
        )
        .unwrap_err(),
        "CMYK artwork requires a CMYK document"
    );
    assert_eq!(
        publish(
            doc,
            0,
            false,
            (w, h),
            &fill_components([1.5, 0.0, 0.0, 0.0, 1.0]),
            &[],
            &[],
            "F",
            FILL
        )
        .unwrap_err(),
        "CMYK artwork ink must be finite with every ink and alpha in 0…1"
    );
    let coverage = vec![65535u16; n - 1];
    assert_eq!(
        publish(
            doc,
            0,
            false,
            (w, h),
            r#"[{"kind":"coverage","ink":[0,0,0,1,1]}]"#,
            &coverage,
            &[],
            "S",
            SHAPE
        )
        .unwrap_err(),
        "CMYK artwork samples do not match their component layout"
    );
    assert_eq!(
        publish(
            doc,
            0,
            false,
            (w, h),
            &fill_components(ink),
            &[],
            &[],
            "F",
            r#"{"type":"photo","version":1}"#
        )
        .unwrap_err(),
        "artwork description requires text, shape, fill or generated with a positive version"
    );
    assert_eq!(
        publish(doc, 0, true, (w, h), "[]", &[], &[], "F", FILL).unwrap_err(),
        "artwork requires between 1 and 256 ordered components"
    );
    free(rgb);
    free(doc);
}

#[test]
fn psd_export_names_the_described_layer_on_its_loss_line() {
    let doc = base(4, 3);
    let filled = with_fill(doc, [0.0, 0.5, 0.5, 0.0, 1.0]);
    let mut err = ptr::null_mut();
    let text = unsafe { dr_precision_doc_psd_export_report(filled, 0, 0, &mut err) };
    assert!(!text.is_null(), "{}", take_err_string(err));
    let report: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    let losses: Vec<&str> = report["losses"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        losses.contains(
            &"Layer “Color Fill” has a re-editable description (text or shape); it is exported as pixels only."
        ),
        "{losses:?}"
    );
    free(filled);
    free(doc);
}

#[test]
fn a_layers_paste_that_converts_the_ink_drops_the_description() {
    let press = press_profile("Press Test");
    let second = second_press_profile("Second Press");
    let layer = |name| RgbLayer {
        name,
        w: 4,
        h: 3,
        offset: (0, 0),
        pixels: varied(4, 3, 2),
    };
    let clip_base = cmyk_layers_in(&press, 16, 4, 3, vec![layer("Clip")]);
    let clip = with_fill(clip_base, [0.3, 0.3, 0.0, 0.0, 1.0]);
    let target = cmyk_layers_in(&second, 16, 4, 3, vec![layer("Target")]);
    let paste = |preserve: bool| {
        let mut report = ptr::null_mut();
        let mut err = ptr::null_mut();
        let out = unsafe {
            dr_precision_doc_paste_layers(
                target,
                clip,
                0,
                1,
                0,
                0,
                preserve,
                LIMIT,
                0,
                &mut report,
                &mut err,
            )
        };
        if !report.is_null() {
            unsafe { dr_string_free(report) };
        }
        assert!(!out.is_null(), "{}", take_err_string(err));
        out
    };
    let converted = paste(false);
    let count = unsafe { &*converted }.layer_count();
    assert_eq!(count, 3);
    assert_eq!(
        described(converted, 2),
        None,
        "converted ink is no longer the rendering"
    );
    let kept = paste(true);
    assert_eq!(
        described(kept, 2).as_deref(),
        Some(FILL),
        "Don't Convert keeps the ink"
    );
    for d in [clip_base, clip, target, converted, kept] {
        free(d);
    }
}
