//! Perspective Crop on the byte document, through
//! `dr_doc_geometry_edit[_plan]`'s `perspective_crop` request.
//!
//! The oracle is this file's own: a homography solved by Gaussian
//! elimination over the eight direct-linear-transform equations (not the
//! core's closed form), and a NEAREST resample that floors each destination
//! centre's source point — so the nearest results are compared sample for
//! sample. Filtered results are pinned the other way: an integer axis
//! rectangle at its own size must flatten to exactly what `dr_doc_crop`
//! flattens to, nearest and bilinear.
//!
//! The admission record (`admission_measure`, ignored; `plans_cover_the_
//! measured_peaks`, default) covers the byte, 16-bit and float plans.

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::doc_guide::GuideOrientation;
use doctored_core::doc_sampler::ColorSampler;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::*;
use doctored_core::ffi_geometry::{dr_doc_geometry_edit, dr_doc_geometry_edit_plan};
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::perspective_crop::{CORNERS_INVALID, QUAD_INVALID, SIZE_INVALID, SOURCE_LAYERS};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::common::*;
use crate::perspective_oracle::{dlt, nearest_index, project, solve};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CW: u32 = 32;
const CH: u32 = 24;

// ----------------------------------------------------------- the requests --

pub fn request(quad: [f64; 8], w: u32, h: u32, filter: &str) -> Value {
    json!({"op":"perspective_crop","quad":quad,"width":w,"height":h,"filter":filter})
}

fn run(doc: *const DrDocument, req: &Value) -> (Option<DrDocument>, Option<String>) {
    let (out, err) = byte_geometry_edit(doc, req);
    let doc = (!out.is_null()).then(|| *unsafe { Box::from_raw(out) });
    (doc, err)
}

fn plan(doc: *const DrDocument, req: &Value) -> Result<Value, String> {
    let text = CString::new(req.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let p = unsafe { dr_doc_geometry_edit_plan(doc, text.as_ptr(), &mut err) };
    if p.is_null() {
        return Err(take_err_string(err));
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Ok(serde_json::from_str(&s).unwrap())
}

/// A keystone: the far edge narrower, every corner fractional.
const KEYSTONE: [f64; 8] = [5.3, 3.7, 26.1, 2.2, 29.4, 21.6, 2.8, 19.9];

/// A patterned 32×24 base; a translucent 10×8 layer at (6, 5) with a
/// checkerboard mask whose outside coverage is 255; one saved channel.
fn fixture() -> DrDocument {
    let base = RgbaImage::from_fn(CW, CH, |x, y| {
        Rgba([(x * 7) as u8, (y * 11) as u8, ((x * y) % 251) as u8, 255])
    });
    let top = RgbaImage::from_fn(10, 8, |x, y| {
        Rgba([(200 - x * 9) as u8, (y * 23) as u8, 77, (90 + x * 13) as u8])
    });
    let doc = DrDocument::from_pixels(base)
        .adding_image_layer(0, top, "Top")
        .unwrap()
        .with_layer_offset(1, 6, 5)
        .unwrap();
    let sel = selection(CW, CH, |x, y| if (x + y) % 2 == 0 { 255 } else { 40 });
    let mut doc = doc.add_mask(1, MaskKind::FromSelection(&sel)).unwrap();
    doc.layers[1].mask_properties.outside_coverage = 255;
    let plane: Vec<u8> = (0..CW * CH).map(|i| ((i * 13) % 256) as u8).collect();
    doc.add_channel("Saved", &plane, CW, CH, [255, 0, 0], 0.5)
        .unwrap()
}

#[test]
fn nearest_matches_the_independent_homography_on_every_plane() {
    let doc = fixture();
    let (w, h) = (20u32, 15u32);
    let (out, err) = run(&doc, &request(KEYSTONE, w, h, "nearest"));
    assert_eq!(err, None);
    let out = out.expect("the crop changes the document");
    assert_eq!((out.width, out.height), (w, h));
    let hm = dlt(f64::from(w), f64::from(h), KEYSTONE);
    let mut compared = 0;
    for idx in 0..2 {
        let src = &doc.layers[idx];
        let srcpx = src.pixels.to_contiguous().unwrap();
        let got = layer_bytes(&out, idx);
        assert_eq!(out.layers[idx].pixels.dimensions(), (w, h));
        assert_eq!(out.layers[idx].offset, (0, 0));
        for j in 0..h {
            for i in 0..w {
                let Some(at) = nearest_index(&hm, srcpx.dimensions(), src.offset, i, j) else {
                    continue;
                };
                let want = at.map_or([0; 4], |k| {
                    let p = &srcpx.as_raw()[k * 4..k * 4 + 4];
                    [p[0], p[1], p[2], p[3]]
                });
                let k = ((j * w + i) * 4) as usize;
                assert_eq!(&got[k..k + 4], &want, "layer {idx} at ({i}, {j})");
                compared += 1;
            }
        }
    }
    // The mask reads 255 where the quad leaves it; the channel reads 0.
    let mask = doc.layers[1].mask.as_ref().unwrap();
    let new_mask = out.layers[1].mask.as_ref().unwrap();
    assert_eq!(new_mask.dimensions(), (w, h));
    assert_eq!(out.layers[1].mask_properties.offset, (0, 0));
    let channel = &doc.channels[0].data;
    let new_channel = &out.channels[0].data;
    for j in 0..h {
        for i in 0..w {
            if let Some(at) = nearest_index(
                &hm,
                mask.dimensions(),
                doc.layers[1].mask_properties.offset,
                i,
                j,
            ) {
                let want = at.map_or(255, |k| mask.as_raw()[k]);
                assert_eq!(new_mask.get_pixel(i, j).0[0], want, "mask ({i}, {j})");
            }
            if let Some(at) = nearest_index(&hm, (CW, CH), (0, 0), i, j) {
                let want = at.map_or(0, |k| channel.as_raw()[k]);
                assert_eq!(new_channel.get_pixel(i, j).0[0], want, "channel ({i}, {j})");
            }
        }
    }
    assert!(compared > 500, "only {compared} samples compared");
}

#[test]
fn an_axis_rectangle_at_its_own_size_flattens_like_crop() {
    let doc = fixture();
    let (x, y, w, h) = (4u32, 3u32, 20u32, 15u32);
    let (fx, fy, fw, fh) = (f64::from(x), f64::from(y), f64::from(w), f64::from(h));
    let quad = [fx, fy, fx + fw, fy, fx + fw, fy + fh, fx, fy + fh];
    let cropped = unsafe { dr_doc_crop(&doc, x, y, w, h) };
    assert!(!cropped.is_null());
    let want = flat_pixels(cropped);
    for filter in ["nearest", "bilinear"] {
        let (out, err) = run(&doc, &request(quad, w, h, filter));
        assert_eq!(err, None);
        let out = out.unwrap();
        assert_eq!(out.layers.len(), unsafe { dr_doc_layer_count(cropped) });
        assert_eq!(flat_pixels(&out), want, "{filter}");
    }
    unsafe { dr_doc_free(cropped) };
}

#[test]
fn the_whole_canvas_at_its_own_size_is_unchanged_with_no_error() {
    let doc = fixture();
    let quad = [0.0, 0.0, 32.0, 0.0, 32.0, 24.0, 0.0, 24.0];
    let (out, err) = run(&doc, &request(quad, CW, CH, "bilinear"));
    assert!(out.is_none());
    assert_eq!(err, None);

    // Decided before admission: a limit no plan fits still answers
    // nothing-to-do, not the working-limit refusal.
    let text = CString::new(request(quad, CW, CH, "bilinear").to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe { dr_doc_geometry_edit(&doc, text.as_ptr(), 1, 0, ptr::null(), &mut err) };
    assert!(out.is_null());
    assert!(err.is_null());

    // Decided before the source refusal: a document holding a placed layer
    // (9×7) plans and answers nothing-to-do for its own whole canvas.
    let placed = crate::source_refusal_fixture::byte_document();
    let whole = request([0.0, 0.0, 9.0, 0.0, 9.0, 7.0, 0.0, 7.0], 9, 7, "bilinear");
    assert!(plan(&placed, &whole).is_ok());
    let (out, err) = run(&placed, &whole);
    assert!(out.is_none());
    assert_eq!(err, None);
}

#[test]
fn refusals_name_their_reason_in_plan_and_edit() {
    let doc = fixture();
    let concave = [0.0, 0.0, 20.0, 0.0, 6.0, 6.0, 0.0, 20.0];
    let crossed = [0.0, 0.0, 20.0, 0.0, 0.0, 20.0, 20.0, 20.0];
    let collapsed = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let far = [0.0, 0.0, 2e7, 0.0, 20.0, 20.0, 0.0, 20.0];
    for (req, want) in [
        (request(concave, 10, 10, "nearest"), QUAD_INVALID),
        (request(crossed, 10, 10, "nearest"), QUAD_INVALID),
        (request(collapsed, 10, 10, "nearest"), QUAD_INVALID),
        (request(far, 10, 10, "nearest"), CORNERS_INVALID),
        (request(KEYSTONE, 0, 10, "nearest"), SIZE_INVALID),
        (request(KEYSTONE, 20_000, 10_000, "nearest"), SIZE_INVALID),
    ] {
        assert_eq!(plan(&doc, &req).unwrap_err(), want, "{req}");
        let (out, err) = run(&doc, &req);
        assert!(out.is_none());
        assert_eq!(err.as_deref(), Some(want), "{req}");
    }
    let bad = json!({"op":"perspective_crop","quad":KEYSTONE,"width":4,"height":4,
                     "filter":"nearest","extra":1});
    assert!(run(&doc, &bad)
        .1
        .unwrap()
        .contains("invalid perspective crop request"));
}

#[test]
fn a_layer_with_a_retained_source_refuses_by_name() {
    let doc = crate::source_refusal_fixture::byte_document();
    let req = request(KEYSTONE, 8, 6, "bilinear");
    assert_eq!(plan(&doc, &req).unwrap_err(), SOURCE_LAYERS);
    assert_eq!(run(&doc, &req).1.as_deref(), Some(SOURCE_LAYERS));
}

#[test]
fn a_limit_below_the_plan_refuses_before_any_work() {
    let doc = fixture();
    let req = request(KEYSTONE, 20, 15, "bilinear");
    let scratch = plan(&doc, &req).unwrap()["scratch_bytes"].as_u64().unwrap();
    let text = CString::new(req.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out =
        unsafe { dr_doc_geometry_edit(&doc, text.as_ptr(), scratch / 2, 0, ptr::null(), &mut err) };
    assert!(out.is_null());
    assert!(take_err_string(err).contains("working limit"));
}

#[test]
fn chrome_clears_samplers_follow_and_styles_scale() {
    let doc = fixture()
        .add_guide(GuideOrientation::Vertical, 9.0)
        .unwrap()
        .with_color_samplers(&[
            ColorSampler {
                x: 15,
                y: 11,
                sample_size: 3,
            },
            ColorSampler {
                x: 0,
                y: 0,
                sample_size: 1,
            },
        ])
        .unwrap();
    let mut doc = doc;
    doc.ruler_origin = (4.0, 6.0);
    let doc = styled(
        &doc,
        1,
        "{\"effects\":[{\"type\":\"stroke\",\"size\":2,\"color\":\"#FF0000\"}]}",
    );
    let (w, h) = (40u32, 30u32);
    let quad = [6.0, 4.0, 26.0, 4.0, 26.0, 19.0, 6.0, 19.0];
    let (out, err) = run(&doc, &request(quad, w, h, "bilinear"));
    assert_eq!(err, None);
    let out = out.unwrap();
    assert!(out.guides.is_empty());
    assert_eq!(out.ruler_origin, (0.0, 0.0));
    // (15.5, 11.5) maps to ((15.5 − 6) × 2, (11.5 − 4) × 2) = (19, 15); the
    // corner sampler leaves the quad and is dropped.
    let back = solve(
        [
            (quad[0], quad[1]),
            (quad[2], quad[3]),
            (quad[4], quad[5]),
            (quad[6], quad[7]),
        ],
        [(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (0.0, 30.0)],
    );
    let (sx, sy) = project(&back, 15.5, 11.5);
    assert_eq!(
        out.color_samplers(),
        &[ColorSampler {
            x: sx.floor() as u32,
            y: sy.floor() as u32,
            sample_size: 3
        }]
    );
    let style: Value = serde_json::from_str(&ffi_style(&out, 1).unwrap()).unwrap();
    assert_eq!(style["effects"][0]["size"].as_f64(), Some(4.0), "{style}");
}

#[test]
fn the_exports_tolerate_null() {
    let req = CString::new(request(KEYSTONE, 8, 6, "nearest").to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let out =
        unsafe { dr_doc_geometry_edit(ptr::null(), req.as_ptr(), LIMIT, 0, ptr::null(), &mut err) };
    assert!(out.is_null());
    let _ = take_err_string(err);
    let doc = fixture();
    let out =
        unsafe { dr_doc_geometry_edit(&doc, ptr::null(), LIMIT, 0, ptr::null(), ptr::null_mut()) };
    assert!(out.is_null());
    let p = unsafe { dr_doc_geometry_edit_plan(ptr::null(), req.as_ptr(), ptr::null_mut()) };
    assert!(p.is_null());
}

// -------------------------------------------------------------- admission --

/// The measured fixture: a 4000×3000 canvas, two full-canvas layers (the
/// upper one masked), two saved channels.
const MW: u32 = 4000;
const MH: u32 = 3000;
/// A keystone quad onto 3600×2600, bilinear.
const MQUAD: [f64; 8] = [180.0, 260.0, 3790.0, 140.0, 3880.0, 2860.0, 90.0, 2700.0];
const MOUT: (u32, u32) = (3600, 2600);

fn measure_request() -> Value {
    request(MQUAD, MOUT.0, MOUT.1, "bilinear")
}

fn plasma16(i: u32) -> [u16; 4] {
    let (x, y) = (i % MW, i / MW);
    [
        (x * 13) as u16,
        (y * 17) as u16,
        ((x ^ y) * 7) as u16,
        65535,
    ]
}

fn byte_measure_fixture() -> DrDocument {
    let base = RgbaImage::from_fn(MW, MH, |x, y| Rgba([x as u8, y as u8, (x ^ y) as u8, 255]));
    let top = RgbaImage::from_fn(MW, MH, |x, y| Rgba([y as u8, x as u8, 90, 200]));
    let doc = DrDocument::from_pixels(base)
        .adding_image_layer(0, top, "Top")
        .unwrap();
    let sel = selection(MW, MH, |x, y| ((x / 7 + y / 5) % 256) as u8);
    let doc = doc.add_mask(1, MaskKind::FromSelection(&sel)).unwrap();
    let plane: Vec<u8> = (0..MW * MH).map(|i| (i % 253) as u8).collect();
    doc.add_channel("A", &plane, MW, MH, [255, 0, 0], 0.5)
        .unwrap()
        .add_channel("B", &plane, MW, MH, [0, 255, 0], 0.5)
        .unwrap()
}

fn typed_mask(w: u32, h: u32) -> PrecisionMask {
    let samples: Vec<u16> = (0..w * h).map(|i| ((i * 37) % 65536) as u16).collect();
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, samples, None).unwrap(),
        doctored_core::doc_mask::MaskProperties::at((0, 0)),
        true,
    )
    .unwrap()
}

fn typed_channels(w: u32, h: u32) -> Vec<PrecisionChannel> {
    ["A", "B"]
        .iter()
        .map(|name| {
            let samples: Vec<u16> = (0..w * h).map(|i| ((i * 29) % 65536) as u16).collect();
            PrecisionChannel::new(name, PrecisionPlane::from_u16(w, h, samples, None).unwrap())
                .unwrap()
        })
        .collect()
}

fn u16_measure_fixture() -> PrecisionDocument {
    let image = |seed: u32| {
        let px: Vec<[u16; 4]> = (0..MW * MH).map(|i| plasma16(i + seed)).collect();
        PrecisionImage::from_rgba16(MW, MH, px, IccProfile::srgb(), None).unwrap()
    };
    let base = PrecisionDocumentLayer::raster("Base", image(0), (0, 0), 1.0, true).unwrap();
    let top = PrecisionDocumentLayer::raster("Top", image(5), (0, 0), 1.0, true)
        .unwrap()
        .with_mask(Some(typed_mask(MW, MH)));
    PrecisionDocument::from_parts_and_channels(
        MW,
        MH,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![base, top],
        typed_channels(MW, MH),
    )
    .unwrap()
}

fn float_measure_fixture() -> PrecisionDocument {
    let record = |seed: u32| -> Vec<[f32; 4]> {
        (0..MW * MH)
            .map(|i| {
                let p = plasma16(i + seed);
                [
                    f32::from(p[0]) / 40000.0,
                    f32::from(p[1]) / 65535.0 - 0.1,
                    f32::from(p[2]) / 65535.0,
                    1.0,
                ]
            })
            .collect()
    };
    let doc = crate::float_fixture::float_doc(MW, MH, &IccProfile::srgb(), &[record(0), record(5)]);
    let doc = doc
        .set_layer_mask(1, Some(typed_mask(MW, MH)), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let samples: Vec<u16> = (0..MW * MH).map(|i| ((i * 29) % 65536) as u16).collect();
    let mut doc = doc;
    for name in ["A", "B"] {
        doc = crate::native_scalar::edit(&doc, json!({"op":"set_channel","name":name}), &samples)
            .unwrap();
    }
    doc
}

fn typed_plan(doc: &PrecisionDocument, req: &Value) -> Value {
    let text = CString::new(req.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), u64::MAX / 4, 0, &mut err) };
    assert!(!p.is_null(), "{}", take_err_string(err));
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    serde_json::from_str(&s).unwrap()
}

/// Max E per case, bytes, from `admission_measure` (module doc of
/// `perspective_crop.rs` holds the full record).
const MEASURED_MAX_E: [(&str, u64); 3] = [
    ("byte", 123_273_312),
    ("u16", 321_437_936),
    ("float", 642_466_272),
];

#[test]
fn plans_cover_the_measured_peaks() {
    let req = measure_request();
    for (case, max_e) in MEASURED_MAX_E {
        let scratch = match case {
            "byte" => plan(&byte_measure_fixture(), &req).unwrap()["scratch_bytes"]
                .as_u64()
                .unwrap(),
            "u16" => typed_plan(&u16_measure_fixture(), &req)["scratch_bytes"]
                .as_u64()
                .unwrap(),
            _ => typed_plan(&float_measure_fixture(), &req)["scratch_bytes"]
                .as_u64()
                .unwrap(),
        };
        let floor = (max_e * 5).div_ceil(4);
        assert!(
            scratch >= floor,
            "{case}: plan {scratch} under ⌈1.25 × {max_e}⌉ = {floor}"
        );
    }
}

#[repr(C)]
struct RusageInfoV4 {
    uuid: [u8; 16],
    fields: [u64; 35],
}
const PHYS_FOOTPRINT: usize = 7;
const INTERVAL_MAX_PHYS_FOOTPRINT: usize = 33;

extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV4) -> i32;
    fn proc_reset_footprint_interval(pid: i32) -> i32;
}

fn rusage() -> RusageInfoV4 {
    let mut info = RusageInfoV4 {
        uuid: [0; 16],
        fields: [0; 35],
    };
    let rc = unsafe { proc_pid_rusage(std::process::id() as i32, 4, &mut info) };
    assert_eq!(rc, 0, "proc_pid_rusage");
    info
}

/// One case per process, selected by `DR_PC_CASE` (`baseline`, `byte`,
/// `u16`, `float`): build the fixture, read the footprint, reset the
/// interval peak, run the edit through the FFI with the plan's own
/// admission, and print E = interval peak − pre-edit footprint beside the
/// plan's scratch. `baseline` builds the byte fixture and runs nothing.
#[test]
#[ignore = "measurement: DR_PC_CASE=baseline|byte|u16|float, Release, MallocLargeCache=0"]
fn admission_measure() {
    let case = std::env::var("DR_PC_CASE").expect("DR_PC_CASE");
    let req = measure_request();
    let text = CString::new(req.to_string()).unwrap();
    let limit = 64 * 1024 * 1024 * 1024u64;
    let mut err: *mut c_char = ptr::null_mut();
    let (scratch, pre, peak) = match case.as_str() {
        "baseline" | "byte" => {
            let doc = byte_measure_fixture();
            let scratch = plan(&doc, &req).unwrap()["scratch_bytes"].as_u64().unwrap();
            let pre = rusage().fields[PHYS_FOOTPRINT];
            unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
            let out = if case == "byte" {
                unsafe {
                    dr_doc_geometry_edit(&doc, text.as_ptr(), limit, 0, ptr::null(), &mut err)
                }
            } else {
                ptr::null_mut()
            };
            let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
            assert!(err.is_null(), "{}", take_err_string(err));
            assert_eq!(out.is_null(), case == "baseline");
            if !out.is_null() {
                unsafe { dr_doc_free(out) };
            }
            (scratch, pre, peak)
        }
        "u16" | "float" => {
            let doc = if case == "u16" {
                u16_measure_fixture()
            } else {
                float_measure_fixture()
            };
            let scratch = typed_plan(&doc, &req)["scratch_bytes"].as_u64().unwrap();
            let pre = rusage().fields[PHYS_FOOTPRINT];
            unsafe { proc_reset_footprint_interval(std::process::id() as i32) };
            let out = unsafe {
                dr_precision_doc_edit(
                    &doc,
                    text.as_ptr(),
                    ptr::null(),
                    0,
                    limit,
                    0,
                    ptr::null(),
                    &mut err,
                )
            };
            let peak = rusage().fields[INTERVAL_MAX_PHYS_FOOTPRINT];
            assert!(err.is_null(), "{}", take_err_string(err));
            assert!(!out.is_null());
            drop(unsafe { Box::from_raw(out) });
            (scratch, pre, peak)
        }
        other => panic!("unknown DR_PC_CASE {other}"),
    };
    println!(
        "PC_MEASURE case={case} scratch={scratch} pre={pre} peak={peak} E={}",
        peak.saturating_sub(pre)
    );
}
