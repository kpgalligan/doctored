//! Perspective Crop at 16 bits and in a 32-bit float document,
//! through `dr_precision_doc_edit[_plan]`'s `perspective_crop` request.
//!
//! The nearest results are compared with `common/perspective_oracle.rs`'s
//! independent homography sample for sample (u16 codes and f32 bits); the
//! filtered contract is pinned by an integer axis rectangle at its own size
//! rendering exactly what the typed `crop` renders, nearest and bilinear.

use std::ffi::{c_char, CString};
use std::ptr;

use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::perspective_crop::{QUAD_INVALID, SIZE_INVALID, SOURCE_LAYERS};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use serde_json::{json, Value};

use crate::native_scalar::{edit, error};
use crate::perspective_oracle::{dlt, nearest_index};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const CW: u32 = 32;
const CH: u32 = 24;
const TOP_AT: (i32, i32) = (6, 5);
const TOP: (u32, u32) = (10, 8);
const KEYSTONE: [f64; 8] = [5.3, 3.7, 26.1, 2.2, 29.4, 21.6, 2.8, 19.9];

fn request(quad: [f64; 8], w: u32, h: u32, filter: &str) -> Value {
    json!({"op":"perspective_crop","quad":quad,"width":w,"height":h,"filter":filter})
}

fn try_run(doc: &PrecisionDocument, req: &Value) -> Result<Option<PrecisionDocument>, String> {
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
    if !err.is_null() {
        return Err(unsafe { error(err) });
    }
    Ok((!out.is_null()).then(|| *unsafe { Box::from_raw(out) }))
}

fn plan_error(doc: &PrecisionDocument, req: &Value) -> String {
    let text = CString::new(req.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let p = unsafe { dr_precision_doc_edit_plan(doc, text.as_ptr(), LIMIT, 0, &mut err) };
    assert!(p.is_null(), "{req} must be refused");
    unsafe { error(err) }
}

fn mask_plane(w: u32, h: u32, at: (i32, i32)) -> PrecisionMask {
    let samples: Vec<u16> = (0..w * h)
        .map(|i| if i % 2 == 0 { 65535 } else { 9000 + i as u16 })
        .collect();
    let mut properties = MaskProperties::at(at);
    properties.outside_coverage = 255;
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, samples, None).unwrap(),
        properties,
        true,
    )
    .unwrap()
}

fn channel() -> PrecisionChannel {
    let samples: Vec<u16> = (0..CW * CH).map(|i| (i * 211) as u16).collect();
    PrecisionChannel::new(
        "Saved",
        PrecisionPlane::from_u16(CW, CH, samples, None).unwrap(),
    )
    .unwrap()
}

/// The 16-bit fixture: a patterned base, a translucent 10×8 layer at (6, 5)
/// with a mask whose outside coverage is 255, one saved channel.
fn u16_fixture() -> PrecisionDocument {
    let base: Vec<[u16; 4]> = (0..CW * CH)
        .map(|i| [(i * 97) as u16, (i * 331) as u16, (i * 7) as u16, 65535])
        .collect();
    let top: Vec<[u16; 4]> = (0..TOP.0 * TOP.1)
        .map(|i| {
            [
                (i * 1009) as u16,
                40000,
                (i * 3) as u16,
                20000 + (i * 500) as u16,
            ]
        })
        .collect();
    let base = PrecisionImage::from_rgba16(CW, CH, base, IccProfile::srgb(), None).unwrap();
    let top = PrecisionImage::from_rgba16(TOP.0, TOP.1, top, IccProfile::srgb(), None).unwrap();
    PrecisionDocument::from_parts_and_channels(
        CW,
        CH,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![
            Layer::raster("Base", base, (0, 0), 1.0, true).unwrap(),
            Layer::raster("Top", top, TOP_AT, 1.0, true)
                .unwrap()
                .with_mask(Some(mask_plane(TOP.0, TOP.1, TOP_AT))),
        ],
        vec![channel()],
    )
    .unwrap()
}

/// The float fixture: two canvas-sized records with extended values, the
/// upper one translucent and masked, one saved channel.
fn float_fixture() -> PrecisionDocument {
    let base: Vec<[f32; 4]> = (0..CW * CH)
        .map(|i| [i as f32 / 300.0, -0.25 + (i % 7) as f32 * 0.1, 0.5, 1.0])
        .collect();
    let top: Vec<[f32; 4]> = (0..CW * CH)
        .map(|i| {
            [
                1.75 - (i % 11) as f32 * 0.2,
                0.3,
                (i % 5) as f32 * 0.25,
                0.25 + (i % 3) as f32 * 0.25,
            ]
        })
        .collect();
    let doc = crate::float_fixture::float_doc(CW, CH, &IccProfile::srgb(), &[base, top]);
    let at = doc.layer(1).unwrap().offset();
    let dims = doc.layer_source(1).unwrap().dimensions();
    let doc = doc
        .set_layer_mask(1, Some(mask_plane(dims.0, dims.1, at)), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let samples: Vec<u16> = (0..CW * CH).map(|i| (i * 211) as u16).collect();
    edit(&doc, json!({"op":"set_channel","name":"Saved"}), &samples).unwrap()
}

/// Compares every raster, the mask and the channel of `out` with the oracle;
/// `bits` reads a source or result sample as comparable bits.
fn assert_nearest(doc: &PrecisionDocument, out: &PrecisionDocument, size: (u32, u32)) {
    let hm = dlt(f64::from(size.0), f64::from(size.1), KEYSTONE);
    let bits = |image: &PrecisionImage, x: u32, y: u32| -> [u32; 4] {
        match image.rgba16_at(x, y) {
            Some(p) => p.map(u32::from),
            None => image.sample_at(x, y).unwrap().map(f32::to_bits),
        }
    };
    let mut compared = 0;
    for idx in 0..2 {
        let (src, at) = (
            doc.layer_source(idx).unwrap(),
            doc.layer(idx).unwrap().offset(),
        );
        let dst = out.layer_source(idx).unwrap();
        assert_eq!(dst.dimensions(), size);
        assert_eq!(out.layer(idx).unwrap().offset(), (0, 0));
        let zero = bits(dst, 0, 0).map(|_| 0u32);
        for j in 0..size.1 {
            for i in 0..size.0 {
                let Some(hit) = nearest_index(&hm, src.dimensions(), at, i, j) else {
                    continue;
                };
                let want = hit.map_or(zero, |k| {
                    let w = src.dimensions().0 as usize;
                    bits(src, (k % w) as u32, (k / w) as u32)
                });
                assert_eq!(bits(dst, i, j), want, "record {idx} ({i}, {j})");
                compared += 1;
            }
        }
    }
    let (mask, new_mask) = (
        doc.layer(1).unwrap().mask().unwrap(),
        out.layer(1).unwrap().mask().unwrap(),
    );
    let mdims = mask.plane().dimensions();
    let mat = doc.layer(1).unwrap().offset();
    assert_eq!(new_mask.plane().dimensions(), size);
    let (channel, new_channel) = (doc.channels()[0].plane(), out.channels()[0].plane());
    for j in 0..size.1 {
        for i in 0..size.0 {
            let k = (j * size.0 + i) as usize;
            if let Some(hit) = nearest_index(&hm, mdims, mat, i, j) {
                let want = hit.map_or(65535, |s| mask.plane().samples()[s]);
                assert_eq!(new_mask.plane().samples()[k], want, "mask ({i}, {j})");
            }
            if let Some(hit) = nearest_index(&hm, (CW, CH), (0, 0), i, j) {
                let want = hit.map_or(0, |s| channel.samples()[s]);
                assert_eq!(new_channel.samples()[k], want, "channel ({i}, {j})");
            }
        }
    }
    assert!(compared > 500, "only {compared} samples compared");
}

#[test]
fn sixteen_bit_nearest_matches_the_independent_homography() {
    let doc = u16_fixture();
    let out = try_run(&doc, &request(KEYSTONE, 20, 15, "nearest"))
        .unwrap()
        .unwrap();
    assert_eq!(out.dimensions(), (20, 15));
    assert_nearest(&doc, &out, (20, 15));
}

#[test]
fn float_nearest_matches_the_independent_homography_bit_for_bit() {
    let doc = float_fixture();
    let out = try_run(&doc, &request(KEYSTONE, 20, 15, "nearest"))
        .unwrap()
        .unwrap();
    assert!(out.is_float());
    assert_nearest(&doc, &out, (20, 15));
}

fn rendered(doc: &PrecisionDocument) -> Vec<u32> {
    let image = doc.render(LIMIT, 0, None).unwrap();
    let out: Vec<u32> = match image.runs_rgba16() {
        Some(runs) => runs.flatten().flatten().map(|&v| u32::from(v)).collect(),
        None => image
            .runs_rgba32()
            .unwrap()
            .flatten()
            .flatten()
            .map(|v| v.to_bits())
            .collect(),
    };
    out
}

#[test]
fn an_axis_rectangle_at_its_own_size_renders_like_crop_at_both_depths() {
    let (x, y, w, h) = (4u32, 3u32, 20u32, 15u32);
    let (fx, fy, fw, fh) = (f64::from(x), f64::from(y), f64::from(w), f64::from(h));
    let quad = [fx, fy, fx + fw, fy, fx + fw, fy + fh, fx, fy + fh];
    for doc in [u16_fixture(), float_fixture()] {
        let crop = json!({"op":"crop","x":x,"y":y,"width":w,"height":h});
        let cropped = try_run(&doc, &crop).unwrap().unwrap();
        let want = rendered(&cropped);
        for filter in ["nearest", "bilinear"] {
            let out = try_run(&doc, &request(quad, w, h, filter))
                .unwrap()
                .unwrap();
            assert_eq!(out.layer_count(), cropped.layer_count());
            let got = rendered(&out);
            assert_eq!(got.len(), want.len());
            // Exact, except a float kernel: its premultiplied f32 accumulator
            // divides alpha back out (`straight_extended`), and `c·a/a` may
            // land one ULP from `c` on a translucent record. u16 quantizes
            // that away; nearest copies bits.
            let ulps = if doc.is_float() && filter != "nearest" {
                1
            } else {
                0
            };
            let off = got
                .iter()
                .zip(&want)
                .filter(|(g, w)| g.abs_diff(**w) > ulps)
                .count();
            assert_eq!(off, 0, "{filter}: {off} samples differ by more than {ulps}");
        }
    }
}

#[test]
fn refusals_identity_descriptions_and_null() {
    for doc in [u16_fixture(), float_fixture()] {
        let concave = [0.0, 0.0, 20.0, 0.0, 6.0, 6.0, 0.0, 20.0];
        for (req, want) in [
            (request(concave, 10, 10, "nearest"), QUAD_INVALID),
            (request(KEYSTONE, 10, 0, "nearest"), SIZE_INVALID),
        ] {
            assert_eq!(plan_error(&doc, &req), want);
            assert_eq!(try_run(&doc, &req).err().as_deref(), Some(want));
        }
        let whole = [0.0, 0.0, 32.0, 0.0, 32.0, 24.0, 0.0, 24.0];
        assert!(try_run(&doc, &request(whole, CW, CH, "bilinear"))
            .unwrap()
            .is_none());
    }
    let placed = crate::source_refusal_fixture::typed_document();
    let req = request(KEYSTONE, 8, 6, "bilinear");
    assert_eq!(plan_error(&placed, &req), SOURCE_LAYERS);
    // The identity is decided before the source refusal: the placed
    // document's own whole canvas (9×7) is nothing-to-do with no error.
    let whole = request([0.0, 0.0, 9.0, 0.0, 9.0, 7.0, 0.0, 7.0], 9, 7, "bilinear");
    assert!(try_run(&placed, &whole).unwrap().is_none());

    // A description is dropped with the pixels it described.
    let doc = u16_fixture();
    let description = CString::new(r#"{"type":"shape","version":3,"probe":"crop"}"#).unwrap();
    let name = CString::new("Base").unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let described = unsafe {
        doctored_core::ffi_artwork::dr_precision_doc_artwork(
            &doc,
            0,
            true,
            doc.layer_source(0).unwrap(),
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
    assert!(!described.is_null(), "{}", unsafe { error(err) });
    let described = *unsafe { Box::from_raw(described) };
    assert!(described.artwork_description(0).is_some());
    let out = try_run(&described, &request(KEYSTONE, 12, 9, "bilinear"))
        .unwrap()
        .unwrap();
    assert!(out.artwork_description(0).is_none());

    let text = CString::new(req.to_string()).unwrap();
    let mut err: *mut c_char = ptr::null_mut();
    let null = unsafe {
        dr_precision_doc_edit(
            ptr::null(),
            text.as_ptr(),
            ptr::null(),
            0,
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(null.is_null());
    let _ = unsafe { error(err) };
}
