//! The CMYK document mode (#547 phase 1): Image ▸ Mode's two conversions
//! against the synthetic press profile's closed-form oracle
//! (`common/cmyk_fixture.rs`: the profile's own affine tables, the CIE
//! formulas and the built-in sRGB's own tags through `common::icc_chain`),
//! the view link's accuracy at every 16⁴ cell centre, the structure edits
//! and composite against an independent source-over, the readouts, every
//! refusal by its sentence and the null-safety sweep. No test here sets or
//! clears the process working CMYK profile (`untagged_cmyk_working` does):
//! this binary runs with none set.

use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

use doctored_core::cmyk_color::view_link_deviation;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::*;
use doctored_core::ffi_precision::{
    dr_precision_doc_descriptor, dr_precision_doc_duplicate_layer, dr_precision_doc_free,
    dr_precision_doc_info, dr_precision_doc_layer_canvas_image, dr_precision_doc_open,
    dr_precision_doc_render, dr_precision_doc_set_layer_opacity, dr_precision_image_free,
};
use doctored_core::ffi_precision_conversion::dr_precision_doc_promote_to_float;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_regional::dr_precision_doc_render_region;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_capability::require_cmyk;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use doctored_core::DrImage;
use serde_json::Value;
use tempfile::TempDir;

use crate::cmyk_fixture::{ink_to_srgb, press_profile, srgb_to_ink};
use crate::cmyk_tiff_builder::cmyk_tiff;
use crate::common::{cpath, img_dims, img_pixels, take_err_string};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const GENERIC_CMYK: &str = "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc";

fn json(text: *mut std::ffi::c_char, err: *mut std::ffi::c_char) -> Value {
    assert!(!text.is_null(), "{}", take_err_string(err));
    let value = serde_json::from_str(unsafe { CStr::from_ptr(text) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(text) };
    value
}

fn boxed(document: PrecisionDocument) -> *mut PrecisionDocument {
    Box::into_raw(Box::new(document))
}

fn free(document: *mut PrecisionDocument) {
    unsafe { dr_precision_doc_free(document) };
}

/// A 16-bit CMYK TIFF of `samples` (ink, one row) carrying `icc`, opened as
/// a CMYK document through the typed open.
fn cmyk_doc(
    dir: &TempDir,
    name: &str,
    w: u32,
    h: u32,
    samples: &[[u16; 4]],
    icc: &[u8],
) -> *mut PrecisionDocument {
    let data: Vec<u8> = samples
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let path = dir.path().join(name);
    std::fs::write(&path, cmyk_tiff(w, h, 16, 4, 0, &data, Some(icc))).unwrap();
    open(&path)
}

fn open(path: &Path) -> *mut PrecisionDocument {
    let c = cpath(path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(!doc.is_null(), "open {path:?}: {}", take_err_string(err));
    doc
}

/// A u16 sRGB document of one row of `pixels`.
fn rgb_doc(pixels: Vec<[u16; 4]>) -> *mut PrecisionDocument {
    let w = pixels.len() as u32;
    let image = PrecisionImage::from_rgba16(w, 1, pixels, IccProfile::srgb(), None).unwrap();
    boxed(PrecisionDocument::from_image(image, Metadata::default(), Resolution::default()).unwrap())
}

fn to_cmyk(doc: *const PrecisionDocument, icc: &[u8], options: &str) -> *mut PrecisionDocument {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            doc,
            icc.as_ptr(),
            icc.len(),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!out.is_null(), "to CMYK: {}", take_err_string(err));
    out
}

fn to_cmyk_err(doc: *const PrecisionDocument, icc: Option<&[u8]>, options: &str) -> String {
    let options = CString::new(options).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_convert_to_cmyk(
            doc,
            icc.map_or(ptr::null(), |b| b.as_ptr()),
            icc.map_or(0, |b| b.len()),
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(out.is_null());
    take_err_string(err)
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
    assert!(!out.is_null(), "to RGB: {}", take_err_string(err));
    out
}

fn samples(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 5]> {
    unsafe { &*doc }
        .cmyk_layer_samples(index)
        .expect("a CMYK record")
}

fn rgb_samples(doc: *const PrecisionDocument, index: usize) -> Vec<[u16; 4]> {
    unsafe { &*doc }
        .layer_source(index)
        .expect("an RGB raster")
        .to_rgba16()
        .unwrap()
}

fn edit(doc: *const PrecisionDocument, request: &str) -> Result<*mut PrecisionDocument, String> {
    let text = CString::new(request).unwrap();
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
    if out.is_null() {
        Err(take_err_string(err))
    } else {
        Ok(out)
    }
}

fn code(v: f64) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn within(a: u16, b: u16, bound: u16) -> bool {
    a.abs_diff(b) <= bound
}

/// A 6³ grid of sRGB colours.
fn rgb_grid() -> Vec<[u16; 4]> {
    let mut out = Vec::new();
    for r in 0..6u16 {
        for g in 0..6u16 {
            for b in 0..6u16 {
                out.push([r * 13107, g * 13107, b * 13107, 65535]);
            }
        }
    }
    out
}

/// A 5⁴ grid of inks.
fn ink_grid() -> Vec<[u16; 4]> {
    let level = |i: u16| i * 16383 + i / 4 * 3;
    let mut out = Vec::new();
    for c in 0..5u16 {
        for m in 0..5u16 {
            for y in 0..5u16 {
                for k in 0..5u16 {
                    out.push([level(c), level(m), level(y), level(k)]);
                }
            }
        }
    }
    out
}

#[test]
fn rgb_to_cmyk_lands_on_the_profiles_own_tables() {
    let profile = press_profile("Press Test");
    let grid = rgb_grid();
    let doc = rgb_doc(grid.clone());
    let cmyk = to_cmyk(doc, &profile, r#"{"black_point_compensation":false}"#);
    assert!(unsafe { dr_precision_doc_is_cmyk(cmyk) });
    for (pixel, sample) in grid.iter().zip(samples(cmyk, 0)) {
        let ink = srgb_to_ink(std::array::from_fn(|c| f64::from(pixel[c]) / 65535.0));
        for c in 0..4 {
            assert!(
                within(sample[c], code(ink[c]), 1),
                "{pixel:?}: ink {c} {} vs oracle {}",
                sample[c],
                code(ink[c])
            );
        }
        assert_eq!(sample[4], 65535, "alpha is carried");
    }
    // The kept profile is the one handed over, byte for byte.
    let mut length = 0usize;
    let bytes = unsafe { dr_precision_doc_cmyk_profile(cmyk, &mut length) };
    assert_eq!(
        unsafe { std::slice::from_raw_parts(bytes, length) },
        &profile[..]
    );
    free(cmyk);
    free(doc);
}

#[test]
fn cmyk_to_rgb_lands_on_the_oracle_with_and_without_bpc() {
    let dir = TempDir::new().unwrap();
    let grid = ink_grid();
    let doc = cmyk_doc(
        &dir,
        "grid.tif",
        grid.len() as u32,
        1,
        &grid,
        &press_profile("Press Test"),
    );
    for bpc in [false, true] {
        let rgb = to_rgb(doc, &format!(r#"{{"black_point_compensation":{bpc}}}"#));
        assert!(!unsafe { dr_precision_doc_is_cmyk(rgb) });
        for (ink, pixel) in grid.iter().zip(rgb_samples(rgb, 0)) {
            let expected = ink_to_srgb(ink.map(|v| f64::from(v) / 65535.0), bpc);
            for c in 0..3 {
                assert!(
                    within(pixel[c], code(expected[c]), 1),
                    "bpc {bpc} {ink:?}: channel {c} {} vs oracle {}",
                    pixel[c],
                    code(expected[c])
                );
            }
            assert_eq!(pixel[3], 65535);
        }
        free(rgb);
    }
    free(doc);
}

#[test]
fn the_view_link_is_within_one_code_of_the_exact_leg_at_every_cell_centre() {
    let synthetic = view_link_deviation(&press_profile("Press Test")).unwrap();
    assert!(synthetic <= 1.0 / 255.0, "synthetic profile: {synthetic}");
    if let Ok(generic) = std::fs::read(GENERIC_CMYK) {
        let deviation = view_link_deviation(&generic).unwrap();
        assert!(deviation <= 1.0 / 255.0, "Generic CMYK: {deviation}");
    }
}

#[test]
fn the_view_renders_the_composite_and_regions_match_it() {
    let dir = TempDir::new().unwrap();
    let grid = ink_grid();
    let doc = cmyk_doc(
        &dir,
        "view.tif",
        25,
        25,
        &grid,
        &press_profile("Press Test"),
    );
    let mut err = ptr::null_mut();
    let full = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!full.is_null(), "{}", take_err_string(err));
    let full_pixels = unsafe { &*full }.to_rgba16().unwrap();
    assert_eq!(
        unsafe { &*full }.profile().name(),
        IccProfile::display_p3().name()
    );
    let region = unsafe {
        dr_precision_doc_render_region(doc, 3, 4, 10, 7, 1, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!region.is_null(), "{}", take_err_string(err));
    let window = unsafe { &*region }.to_rgba16().unwrap();
    for y in 0..7usize {
        for x in 0..10usize {
            assert_eq!(window[y * 10 + x], full_pixels[(y + 4) * 25 + x + 3]);
        }
    }
    // A reduced preview is the full view sampled at (x + i·d, y + j·d).
    assert_sampled(doc, &full_pixels, 25, 25);
    unsafe {
        dr_precision_image_free(full);
        dr_precision_image_free(region);
    }
    free(doc);
}

/// Every divisor 2, 3 and 7 region of `doc`, whole canvas and offset, is
/// the full view `full` (`w × h`) sampled at `(x + i·d, y + j·d)`, its
/// output `ceil(rw/d) × ceil(rh/d)` (`regional`'s SAMPLING rule).
fn assert_sampled(doc: *const PrecisionDocument, full: &[[u16; 4]], w: u32, h: u32) {
    for d in [2u32, 3, 7] {
        for (x, y, rw, rh) in [(0, 0, w, h), (1, 2, w - 3, h - 2)] {
            let mut err = ptr::null_mut();
            let region = unsafe {
                dr_precision_doc_render_region(
                    doc,
                    x,
                    y,
                    rw,
                    rh,
                    d,
                    LIMIT,
                    0,
                    ptr::null(),
                    &mut err,
                )
            };
            assert!(!region.is_null(), "divisor {d}: {}", take_err_string(err));
            let (ow, oh) = unsafe { &*region }.dimensions();
            assert_eq!((ow, oh), (rw.div_ceil(d), rh.div_ceil(d)), "divisor {d}");
            let pixels = unsafe { &*region }.to_rgba16().unwrap();
            for j in 0..oh {
                for i in 0..ow {
                    let (cx, cy) = (x + i * d, y + j * d);
                    assert_eq!(
                        pixels[(j * ow + i) as usize],
                        full[(cy * w + cx) as usize],
                        "divisor {d} region ({x},{y}) output ({i},{j})"
                    );
                }
            }
            unsafe { dr_precision_image_free(region) };
        }
    }
}

#[test]
fn a_reduced_region_samples_masked_offset_and_grouped_records() {
    // Offset layers with every alpha class, a feathered-free ramp mask and a
    // group: the strided stack must read every record, mask and group buffer
    // at the full-resolution column.
    use crate::cmyk_structure_fixture::{cmyk_layers, ramp, then, varied, with_mask, RgbLayer};
    let (w, h) = (23u32, 19u32);
    let doc = cmyk_layers(
        w,
        h,
        vec![
            RgbLayer {
                name: "A",
                w,
                h,
                offset: (0, 0),
                pixels: varied(w, h, 1),
            },
            RgbLayer {
                name: "B",
                w: 15,
                h: 11,
                offset: (5, 3),
                pixels: varied(15, 11, 2),
            },
            RgbLayer {
                name: "C",
                w: 9,
                h: 13,
                offset: (-2, 4),
                pixels: varied(9, 13, 3),
            },
        ],
    );
    let doc = with_mask(doc, 1, &ramp(w, h, 4));
    let doc = then(
        doc,
        r#"{"op":"group_layers","layers":[1,2],"name":"G","pass_through":false}"#,
        &[],
    );
    let mut err = ptr::null_mut();
    let full = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!full.is_null(), "{}", take_err_string(err));
    let full_pixels = unsafe { &*full }.to_rgba16().unwrap();
    assert_sampled(doc, &full_pixels, w, h);
    unsafe { dr_precision_image_free(full) };
    crate::cmyk_structure_fixture::free(doc);
}

/// The independent source-over, in f64 (W3C Normal on ink).
fn over(dst: [f64; 5], src: [f64; 5], opacity: f64) -> [f64; 5] {
    let sa = src[4] * opacity;
    let ab = dst[4];
    let ao = sa + ab * (1.0 - sa);
    if ao == 0.0 {
        return [0.0; 5];
    }
    let mut out = [0.0; 5];
    for c in 0..4 {
        out[c] = (sa * src[c] + (1.0 - sa) * ab * dst[c]) / ao;
    }
    out[4] = ao;
    out
}

#[test]
fn structure_edits_keep_the_ink_and_the_composite_is_source_over() {
    // Two RGB records — an opaque bottom and a half-transparent top at 60 %
    // layer opacity — converted to CMYK keep both records, their alpha and
    // their opacity; the composite is then checked against the independent
    // source-over of the stored ink.
    let profile = press_profile("Press Test");
    let srgb = IccProfile::srgb();
    let bottom = PrecisionImage::from_rgba16(
        2,
        2,
        vec![[60000, 5000, 9000, 65535]; 4],
        srgb.clone(),
        None,
    )
    .unwrap();
    let top = PrecisionImage::from_rgba16(
        2,
        2,
        vec![[3000, 20000, 61000, 32768]; 4],
        srgb.clone(),
        None,
    )
    .unwrap();
    let rgb = boxed(
        PrecisionDocument::from_parts(
            2,
            2,
            srgb,
            Metadata::default(),
            Resolution::default(),
            vec![
                PrecisionDocumentLayer::raster("Bottom", bottom, (0, 0), 1.0, true).unwrap(),
                PrecisionDocumentLayer::raster("Top", top, (0, 0), 0.6, true).unwrap(),
            ],
        )
        .unwrap(),
    );
    let doc = to_cmyk(rgb, &profile, "");
    let mut err = ptr::null_mut();
    let info = json(unsafe { dr_precision_doc_info(doc, &mut err) }, err);
    assert_eq!(info["color_mode"], "cmyk");
    assert_eq!(info["layers"][1]["name"], "Top");
    assert_eq!(info["layers"][1]["kind"], "raster");
    assert!((info["layers"][1]["opacity"].as_f64().unwrap() - 0.6).abs() < 1e-6);
    let (low, high) = (samples(doc, 0)[0], samples(doc, 1)[0]);
    assert_eq!(high[4], 32768, "alpha is carried");
    let unit = |s: [u16; 5]| s.map(|v| f64::from(v) / 65535.0);
    let expected = over(over([0.0; 5], unit(low), 1.0), unit(high), 0.6);
    // The unflattened composite's readout.
    let sample = json(
        unsafe { dr_precision_doc_cmyk_sample(doc, 0, 0, 0, LIMIT, 0, &mut err) },
        err,
    );
    for c in 0..4 {
        let got = sample["ink16"][c].as_u64().unwrap() as u16;
        assert!(
            within(got, code(expected[c]), 1),
            "ink {c}: {got} vs {}",
            code(expected[c])
        );
    }
    // Rename, label, duplicate: the ink is shared and unchanged.
    let named = edit(doc, r#"{"op":"rename_layer","layer":1,"name":"Ink"}"#).unwrap();
    let labelled = edit(named, r#"{"op":"layer_label","layer":1,"label":3}"#).unwrap();
    let copied = unsafe { dr_precision_doc_duplicate_layer(labelled, 1, &mut err) };
    assert!(!copied.is_null(), "{}", take_err_string(err));
    assert_eq!(samples(copied, 2), samples(copied, 1));
    let info = json(unsafe { dr_precision_doc_info(copied, &mut err) }, err);
    assert_eq!(info["layers"][1]["label"], 3);
    assert_eq!(info["layers"][2]["name"], "Ink copy");
    let removed = edit(copied, r#"{"op":"remove_layers","layers":[2]}"#).unwrap();
    // Opacity 1 on the top record, then Flatten onto paper.
    let opaque = unsafe { dr_precision_doc_set_layer_opacity(removed, 1, 1.0, &mut err) };
    assert!(!opaque.is_null(), "{}", take_err_string(err));
    // The flatten's edit plan, which the host reserves for the job, covers
    // the CMYK flatten's own admission.
    let plan = json(
        unsafe {
            dr_precision_doc_edit_plan(opaque, c"{\"op\":\"flatten\"}".as_ptr(), LIMIT, 0, &mut err)
        },
        err,
    );
    let descriptor = json(
        unsafe { dr_precision_doc_descriptor(opaque, &mut err) },
        err,
    );
    let own = descriptor["cmyk_flatten_scratch_bytes"].as_u64().unwrap();
    assert!(
        plan["scratch_bytes"].as_u64().unwrap() >= own,
        "{plan} < {own}"
    );
    let flat = edit(opaque, r#"{"op":"flatten"}"#).unwrap();
    let expected = over(over([0.0; 5], unit(low), 1.0), unit(high), 1.0);
    for sample in samples(flat, 0) {
        for c in 0..4 {
            assert!(within(sample[c], code(expected[c] * expected[4]), 1));
        }
        assert_eq!(sample[4], 65535, "flattened onto paper");
    }
    for d in [rgb, doc, named, labelled, copied, removed, opaque, flat] {
        free(d);
    }
}

#[test]
fn ink_planes_show_ink_dark_and_the_sample_reads_percent() {
    let dir = TempDir::new().unwrap();
    let ink = [[65535u16, 32768, 0, 13107]; 4];
    let doc = cmyk_doc(&dir, "planes.tif", 2, 2, &ink, &press_profile("Press Test"));
    let mut err = ptr::null_mut();
    for (plane, value) in [(0u32, 65535u16), (1, 32768), (2, 0), (3, 13107)] {
        let image = unsafe {
            dr_precision_doc_cmyk_plane_view(doc, plane, 0, LIMIT, 0, ptr::null(), &mut err)
        };
        assert!(!image.is_null(), "{}", take_err_string(err));
        let gray = 255 - (f64::from(value) / 65535.0 * 255.0).round() as u8;
        let pixels = img_pixels(image);
        assert!(
            pixels.chunks(4).all(|p| p == [gray, gray, gray, 255]),
            "plane {plane}"
        );
        unsafe { doctored_core::ffi::dr_image_free(image) };
    }
    let bad =
        unsafe { dr_precision_doc_cmyk_plane_view(doc, 4, 0, LIMIT, 0, ptr::null(), &mut err) };
    assert!(bad.is_null());
    assert_eq!(
        take_err_string(err),
        "a CMYK ink plane is 0 (cyan), 1 (magenta), 2 (yellow) or 3 (black)"
    );
    let sample = json(
        unsafe { dr_precision_doc_cmyk_sample(doc, 1, 1, 1, LIMIT, 0, &mut err) },
        err,
    );
    assert_eq!(
        sample["ink_percent"],
        serde_json::json!([100.0, 50.0, 0.0, 20.0])
    );
    assert_eq!(sample["sample_count"], 4);
    // The sample is admitted: with the whole limit held elsewhere it refuses.
    let refused = unsafe { dr_precision_doc_cmyk_sample(doc, 1, 1, 1, LIMIT, LIMIT, &mut err) };
    assert!(refused.is_null());
    assert!(
        take_err_string(err).contains("working limit is"),
        "the sample is admitted against the working limit"
    );
    let thumb = unsafe {
        dr_precision_doc_cmyk_layer_thumbnail_rgba8(doc, 0, 1, LIMIT, 0, ptr::null(), &mut err)
    };
    assert!(!thumb.is_null(), "{}", take_err_string(err));
    assert_eq!(img_dims(thumb), (1, 1));
    unsafe { doctored_core::ffi::dr_image_free(thumb) };
    assert_ne!(unsafe { dr_precision_doc_cmyk_layer_identity(doc, 0) }, 0);
    assert_eq!(unsafe { dr_precision_doc_cmyk_layer_identity(doc, 7) }, 0);
    let canvas =
        unsafe { dr_precision_doc_layer_canvas_image(doc, 0, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!canvas.is_null(), "{}", take_err_string(err));
    unsafe { dr_precision_image_free(canvas) };
    free(doc);
}

#[test]
fn every_refusal_names_itself() {
    let profile = press_profile("Press Test");
    let dir = TempDir::new().unwrap();
    let rgb = rgb_doc(vec![[1000, 2000, 3000, 65535]]);
    // No working CMYK profile is set in this binary.
    assert_eq!(
        to_cmyk_err(rgb, None, ""),
        "no working CMYK profile is set, so CMYK Color needs a CMYK profile to convert into"
    );
    // A float document.
    let mut err = ptr::null_mut();
    let float = unsafe { dr_precision_doc_promote_to_float(rgb, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!float.is_null(), "{}", take_err_string(err));
    assert_eq!(
        to_cmyk_err(float, Some(&profile), ""),
        "CMYK Color is unavailable in a 32-bit float document; convert to 16 Bits/Channel first"
    );
    // A non-CMYK profile.
    let srgb = IccProfile::srgb().bytes().to_vec();
    assert!(to_cmyk_err(rgb, Some(&srgb), "").starts_with("cannot use the CMYK profile"));
    // An unknown option.
    assert_eq!(
        to_cmyk_err(rgb, Some(&profile), r#"{"dither":true}"#),
        "mode options have an unknown key \"dither\""
    );
    // A group CARRIES since phase 4a: the CMYK document keeps it.
    let grouped = edit(rgb, r#"{"op":"group_layers","layers":[0],"name":"G"}"#).unwrap();
    let carried = to_cmyk(grouped, &profile, "");
    assert_eq!(unsafe { &*carried }.layer_count(), 2);
    let group = unsafe { &*carried }.layer(1).unwrap();
    assert_eq!(group.name(), "G");
    assert_eq!(group.children().map(<[_]>::len), Some(1));
    // An enabled layer effect CARRIES since the styles batch (#547).
    let shadowed = edit(
        rgb,
        r#"{"op":"layer_style","version":1,"layer":0,"style":{"effects":[{"type":"drop_shadow"}]}}"#,
    )
    .unwrap();
    let carried = to_cmyk(shadowed, &profile, "");
    assert!(
        unsafe { &*carried }
            .layer(0)
            .unwrap()
            .style()
            .is_some_and(|s| s.has_enabled_effects()),
        "the drop shadow carries"
    );
    // A Gray Blend If carries (the Blend If batch); one on Red needs a
    // flatten; the plan says so and flatten:true converts.
    let gray = edit(
        rgb,
        r#"{"op":"layer_blend_if","version":1,"layer":0,"blend_if":{"channel":"gray","this_layer":[0,0,200,255]}}"#,
    )
    .unwrap();
    let carried = to_cmyk(gray, &profile, "");
    assert_eq!(
        unsafe { &*carried }.layer(0).unwrap().blend_if(),
        unsafe { &*gray }.layer(0).unwrap().blend_if(),
        "the Gray Blend If carries"
    );
    let styled = edit(
        rgb,
        r#"{"op":"layer_blend_if","version":1,"layer":0,"blend_if":{"channel":"red","this_layer":[0,0,200,255]}}"#,
    )
    .unwrap();
    let blocker = "“Background” is a layer with Blend If on the Red channel, which a CMYK document cannot hold yet; flatten the image to convert it";
    // A plate channel names the mode it needs on an RGB document (#547).
    for (edit_text, channel) in [
        (
            r#"{"op":"layer_blend_if","version":1,"layer":0,"blend_if":{"channel":"cyan","this_layer":[0,0,200,255]}}"#,
            "Cyan",
        ),
        (
            r#"{"op":"layer_style","version":1,"layer":0,"style":{"blend_if":{"channel":"black","this_layer":[0,0,200,255]}}}"#,
            "Black",
        ),
    ] {
        assert_eq!(
            edit(rgb, edit_text).unwrap_err(),
            format!("Blend If on the {channel} channel needs a CMYK document")
        );
    }
    assert_eq!(to_cmyk_err(styled, Some(&profile), ""), blocker);
    let plan = json(
        unsafe {
            dr_precision_doc_convert_to_cmyk_plan(
                styled,
                profile.as_ptr(),
                profile.len(),
                ptr::null(),
                &mut err,
            )
        },
        err,
    );
    assert_eq!(plan["flatten_required"], true);
    assert_eq!(plan["refusal"], blocker);
    let flattened = to_cmyk(styled, &profile, r#"{"flatten":true}"#);
    assert_eq!(unsafe { &*flattened }.layer_count(), 1);
    // Already CMYK.
    assert_eq!(
        to_cmyk_err(flattened, Some(&profile), ""),
        "the document is already CMYK"
    );
    // Everything outside the CMYK table, by name.
    assert_eq!(
        edit(flattened, r#"{"op":"paint","layer":0}"#).unwrap_err(),
        "The \"paint\" edit is unavailable in a CMYK document"
    );
    // Phase 2 admits painting in ink; #547's retouch batch the Clone Stamp,
    // the healing family and the retouch brushes; the channels batch alpha
    // channels. An RGB-only family refuses.
    assert!(require_cmyk("paint", "raster").is_ok());
    assert!(require_cmyk("clone", "raster").is_ok());
    assert!(require_cmyk("heal", "raster").is_ok());
    assert!(require_cmyk("retouch_brush", "raster").is_ok());
    assert!(require_cmyk("channel_remove", "channel").is_ok());
    assert!(require_cmyk("layer_blend_if", "raster").is_ok());
    assert_eq!(
        require_cmyk("paint", "channel").unwrap_err(),
        "CMYK paint does not support target channel"
    );
    assert!(require_cmyk("layer_flatten", "document").is_ok());
    // Phase 4a: a family's targets are every line's.
    assert!(require_cmyk("layer_duplicate", "raster").is_ok());
    assert!(require_cmyk("layer_duplicate", "group").is_ok());
    assert!(require_cmyk("layer_group", "adjustment").is_ok());
    assert!(require_cmyk("layer_style", "group").is_ok());
    // A style and a Blend If on an adjustment too.
    assert!(require_cmyk("layer_style", "adjustment").is_ok());
    // The styles batch: the global light; the Blend If batch: Blend If on a
    // raster or a group, and an adjustment.
    assert!(require_cmyk("global_light", "document").is_ok());
    assert!(require_cmyk("layer_blend_if", "group").is_ok());
    assert!(require_cmyk("layer_blend_if", "adjustment").is_ok());
    // Phase 4b: geometry, the warps, Transform Mask and the selected pixels
    // of a layer or its mask; a colour plane and a mesh warp of a mask refuse.
    for (family, target) in [
        ("geometry_resize", "document"),
        ("geometry_crop", "document"),
        ("geometry_rotate", "document"),
        ("geometry_affine", "raster"),
        ("geometry_warp", "mask"),
        ("geometry_puppet", "raster"),
        ("geometry_perspective_warp", "raster"),
        ("geometry_affine_set", "group"),
        ("mask_transform", "mask"),
        ("selected_pixels_move", "raster"),
        ("selected_pixels_transform", "mask"),
    ] {
        assert!(require_cmyk(family, target).is_ok(), "{family} {target}");
    }
    assert_eq!(
        require_cmyk("selected_pixels_move", "red").unwrap_err(),
        "CMYK selected_pixels_move does not support target red"
    );
    assert_eq!(
        require_cmyk("geometry_puppet", "mask").unwrap_err(),
        "CMYK geometry_puppet does not support target mask"
    );
    // Phase 4c: copy and paste, Layer Via and Stamp Selected.
    assert!(require_cmyk("clipboard_copy", "group").is_ok());
    assert!(require_cmyk("clipboard_copy_merged", "document").is_ok());
    assert!(require_cmyk("clipboard_paste", "document").is_ok());
    assert!(require_cmyk("layer_via", "raster").is_ok());
    assert!(require_cmyk("layer_stamp_selected", "group").is_ok());
    assert_eq!(
        require_cmyk("clipboard_cut", "group").unwrap_err(),
        "CMYK clipboard_cut does not support target group"
    );
    // Stack Modes per ink, on rasters only.
    assert!(require_cmyk("layer_stack_mode", "raster").is_ok());
    // A save format the mode does not write.
    let plan = json(
        unsafe {
            dr_precision_doc_save_cmyk_plan(flattened, c"{\"format\":\"png\"}".as_ptr(), &mut err)
        },
        err,
    );
    assert_eq!(
        plan["refusal"],
        "Saving as png is unavailable in a CMYK document"
    );
    // The RGB depth's own exports refuse a CMYK document.
    let descriptor = json(
        unsafe { dr_precision_doc_descriptor(flattened, &mut err) },
        err,
    );
    assert_eq!(descriptor["storage"], "cmyka16_unorm");
    assert_eq!(descriptor["cmyk_depth"], 16);
    // RGB Color of an RGB document.
    let options = c"";
    let wrong = unsafe {
        dr_precision_doc_convert_cmyk_to_rgb(
            rgb,
            ptr::null(),
            0,
            options.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(wrong.is_null());
    assert_eq!(take_err_string(err), "RGB Color requires a CMYK document");
    let _ = dir;
    for d in [rgb, float, grouped, carried, styled, flattened] {
        free(d);
    }
}

#[test]
fn an_untagged_cmyk_file_keeps_todays_path_with_no_working_profile() {
    let dir = TempDir::new().unwrap();
    let data: Vec<u8> = [10u8, 20, 30, 40].repeat(4);
    let path = dir.path().join("untagged.tif");
    std::fs::write(&path, cmyk_tiff(2, 2, 8, 4, 0, &data, None)).unwrap();
    let c = cpath(&path);
    assert_eq!(unsafe { dr_path_cmyk_source(c.as_ptr()) }, 0);
    let tagged = dir.path().join("tagged.tif");
    std::fs::write(
        &tagged,
        cmyk_tiff(2, 2, 8, 4, 0, &data, Some(&press_profile("P"))),
    )
    .unwrap();
    let c = cpath(&tagged);
    assert_eq!(unsafe { dr_path_cmyk_source(c.as_ptr()) }, 1);
    // The typed open of the untagged file is not a CMYK document.
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let doc = unsafe { dr_precision_doc_open(c.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    if !doc.is_null() {
        assert!(!unsafe { dr_precision_doc_is_cmyk(doc) });
        free(doc);
    } else {
        let _ = take_err_string(err);
    }
}

#[test]
fn every_new_export_tolerates_null() {
    let mut err = ptr::null_mut();
    let mut length = 7usize;
    unsafe {
        assert_eq!(dr_path_cmyk_source(ptr::null()), 0);
        assert!(!dr_precision_doc_is_cmyk(ptr::null()));
        assert!(dr_precision_doc_cmyk_info(ptr::null()).is_null());
        assert!(dr_precision_doc_cmyk_profile(ptr::null(), &mut length).is_null());
        assert_eq!(length, 0);
        assert!(dr_precision_doc_cmyk_profile(ptr::null(), ptr::null_mut()).is_null());
        assert!(dr_precision_doc_convert_to_cmyk_plan(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_convert_to_cmyk(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert_eq!(take_err_string(err), "required handle is NULL");
        assert!(dr_precision_doc_convert_cmyk_to_rgb_plan(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_convert_cmyk_to_rgb(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(
            dr_precision_doc_cmyk_sample(ptr::null(), 0, 0, 0, LIMIT, 0, ptr::null_mut()).is_null()
        );
        let image: *mut DrImage = dr_precision_doc_cmyk_plane_view(
            ptr::null(),
            0,
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
        );
        assert!(image.is_null());
        assert!(dr_precision_doc_cmyk_layer_thumbnail_rgba8(
            ptr::null(),
            0,
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert_eq!(dr_precision_doc_cmyk_layer_identity(ptr::null(), 0), 0);
        assert!(
            dr_precision_doc_save_cmyk_plan(ptr::null(), ptr::null(), ptr::null_mut()).is_null()
        );
        assert!(!dr_precision_doc_save_cmyk(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        ));
        assert!(dr_cmyk_color(ptr::null(), ptr::null(), ptr::null_mut()).is_null());
        assert!(dr_cmyk_color(ptr::null(), ptr::null(), &mut err).is_null());
        assert_eq!(take_err_string(err), "request is NULL");
        // Phase 5 (`ffi_cmyk_colour`).
        use doctored_core::ffi_cmyk_colour::*;
        assert!(dr_precision_doc_cmyk_profile_plan(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_cmyk_profile_edit(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert_eq!(take_err_string(err), "required handle is NULL");
        assert!(dr_precision_doc_new_cmyk(
            1,
            1,
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_cmyk_proof_plan(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        let mut info = ptr::null_mut();
        let proof: *mut DrImage = dr_precision_doc_cmyk_proof(
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut info,
            ptr::null_mut(),
        );
        assert!(proof.is_null() && info.is_null());
        assert!(dr_precision_doc_cmyk_tone_analysis_plan(
            ptr::null(),
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_cmyk_tone_analysis(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        let clip: *mut DrImage = dr_precision_doc_cmyk_clip_display(
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut(),
        );
        assert!(clip.is_null());
        assert!(dr_cmyk_separate(ptr::null(), ptr::null(), 0, ptr::null(), &mut err).is_null());
        assert_eq!(take_err_string(err), "request is NULL");
        assert!(
            dr_adjustment_preset_file_decode_cmyk(ptr::null(), 0, ptr::null(), &mut err).is_null()
        );
        assert_eq!(take_err_string(err), "extension is NULL");
        // The artwork batch (`ffi_cmyk_artwork`).
        use doctored_core::ffi_cmyk_artwork::*;
        assert!(dr_precision_doc_cmyk_artwork(
            ptr::null(),
            0,
            false,
            0,
            1,
            1,
            ptr::null(),
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            0,
            ptr::null(),
            ptr::null(),
            LIMIT,
            0,
            ptr::null(),
            &mut err
        )
        .is_null());
        assert_eq!(take_err_string(err), "required handle is NULL");
        assert!(dr_cmyk_artwork_colors(
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(
            dr_cmyk_artwork_colors(ptr::null(), 0, ptr::null(), 0, ptr::null(), &mut err).is_null()
        );
        assert_eq!(take_err_string(err), "request is NULL");
    }
    // An RGB document answers the CMYK queries with their empty values.
    let rgb = rgb_doc(vec![[0, 0, 0, 65535]]);
    unsafe {
        assert!(!dr_precision_doc_is_cmyk(rgb));
        assert!(dr_precision_doc_cmyk_info(rgb).is_null());
        assert!(dr_precision_doc_cmyk_profile(rgb, &mut length).is_null());
        assert_eq!(dr_precision_doc_cmyk_layer_identity(rgb, 0), 0);
    }
    free(rgb);
}

#[test]
fn a_reduced_region_samples_styled_records() {
    // A drop shadow and an outer glow on an offset layer, and a styled
    // isolated group: the styled-row kernel must read its planes at the
    // full-resolution column too (`cmyk_style_row`'s step).
    use crate::cmyk_structure_fixture::{cmyk_layers, then, varied, RgbLayer};
    let (w, h) = (29u32, 21u32);
    let doc = cmyk_layers(
        w,
        h,
        vec![
            RgbLayer {
                name: "A",
                w,
                h,
                offset: (0, 0),
                pixels: varied(w, h, 1),
            },
            RgbLayer {
                name: "B",
                w: 13,
                h: 9,
                offset: (6, 5),
                pixels: varied(13, 9, 2),
            },
            RgbLayer {
                name: "C",
                w: 7,
                h: 8,
                offset: (17, 9),
                pixels: varied(7, 8, 3),
            },
        ],
    );
    let doc = then(
        doc,
        r#"{"op":"layer_style","version":1,"layer":1,"style":{"effects":[{"type":"drop_shadow","angle":120,"use_global_light":false,"distance":3,"size":2},{"type":"outer_glow","size":3}]}}"#,
        &[],
    );
    let doc = then(
        doc,
        r#"{"op":"group_layers","layers":[2],"name":"G","pass_through":false}"#,
        &[],
    );
    let doc = then(
        doc,
        r#"{"op":"layer_style","version":1,"layer":3,"style":{"effects":[{"type":"stroke","size":2,"position":"outside"}]}}"#,
        &[],
    );
    let mut err = ptr::null_mut();
    let full = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!full.is_null(), "{}", take_err_string(err));
    let full_pixels = unsafe { &*full }.to_rgba16().unwrap();
    assert_sampled(doc, &full_pixels, w, h);
    unsafe { dr_precision_image_free(full) };
    crate::cmyk_structure_fixture::free(doc);
}
