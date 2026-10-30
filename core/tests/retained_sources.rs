//! Embedded-source behavior through the public ABI. Exact-code and analytic
//! sampling/ICC oracles are independent of the retained-source implementation.
use doctored_core::doc::DrDocument;
use doctored_core::doc::LayerRaster;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_doc::dr_doc_canvas_resize;
use doctored_core::ffi_output_color::dr_precision_doc_assign_refusal;
use doctored_core::ffi_precision_color::dr_precision_doc_assign_profile;
use doctored_core::ffi_retained_source::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::retained_source::SourceEdit;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::Arc;
const LIMIT: u64 = REFERENCE_WORKING_BYTES;

unsafe fn text(pointer: *mut c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let value = unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .unwrap()
        .to_string();
    unsafe { dr_string_free(pointer) };
    value
}
fn request(value: Value) -> CString {
    CString::new(value.to_string()).unwrap()
}
fn edit8(doc: &DrDocument, input: Option<&DrDocument>, value: Value) -> DrDocument {
    let mut err = ptr::null_mut();
    let value = request(value);
    let result = unsafe {
        dr_doc_mixed_source_edit(
            doc,
            input.map_or(ptr::null(), |d| d),
            ptr::null(),
            value.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!result.is_null(), "{}", unsafe { text(err) });
    assert!(err.is_null());
    *unsafe { Box::from_raw(result) }
}
fn edit16(
    doc: &PrecisionDocument,
    input: Option<&PrecisionDocument>,
    value: Value,
) -> PrecisionDocument {
    let mut err = ptr::null_mut();
    let value = request(value);
    let result = unsafe {
        dr_precision_doc_source_edit(
            doc,
            input.map_or(ptr::null(), |d| d),
            value.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(!result.is_null(), "{}", unsafe { text(err) });
    assert!(err.is_null());
    *unsafe { Box::from_raw(result) }
}
fn place() -> Value {
    json!({"op":"place","source_id":"fixture-a","name":"Placed","x":0,"y":0})
}
fn transform(matrix: [f64; 6], filter: &str) -> Value {
    json!({"op":"transform","layer":1,"matrix":matrix,"filter":filter})
}
fn byte(w: u32, h: u32) -> DrDocument {
    DrDocument::from_pixels(RgbaImage::from_fn(w, h, |x, y| {
        Rgba([
            (x * 17 + y * 7) as u8,
            (x * 3 + y * 43) as u8,
            (x * 53 + y * 13) as u8,
            if (x + y) % 13 == 0 {
                0
            } else {
                (x * 29 + y * 11 + 1) as u8
            },
        ])
    }))
}
fn typed(w: u32, h: u32) -> PrecisionDocument {
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(
            w,
            h,
            (0..w * h)
                .map(|i| {
                    [
                        i.wrapping_mul(1103) as u16,
                        i.wrapping_mul(7109) as u16,
                        i.wrapping_mul(9973) as u16,
                        if i % 11 == 0 {
                            0
                        } else {
                            i.wrapping_mul(47).wrapping_add(1) as u16
                        },
                    ]
                })
                .collect(),
            IccProfile::display_p3(),
            None,
        )
        .unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap()
}
#[test]
fn byte_and_typed_down_up_restore_master_codes_and_original_profile() {
    let source = byte(32, 24);
    let placed = edit8(&source, Some(&source), place());
    let down = edit8(
        &placed,
        None,
        transform([0.25, 0., 0., 0.25, 0., 0.], "lanczos3"),
    );
    assert_eq!(down.layers[1].pixels.dimensions(), (8, 6));
    let restored = edit8(&down, None, transform([4., 0., 0., 4., 0., 0.], "lanczos3"));
    assert_eq!(
        restored.layers[1].pixels.contiguous_image().as_raw(),
        source.layers[0].pixels.contiguous_image().as_raw()
    );
    assert_eq!(
        placed.layers[1].pixels.contiguous_image().as_raw(),
        source.layers[0].pixels.contiguous_image().as_raw()
    );
    let source = typed(32, 24);
    let placed = edit16(&source, Some(&source), place());
    let down = edit16(
        &placed,
        None,
        transform([0.25, 0., 0., 0.25, 0., 0.], "lanczos3"),
    );
    let restored = edit16(&down, None, transform([4., 0., 0., 4., 0., 0.], "lanczos3"));
    assert_eq!(
        restored.layer_source(1).unwrap().to_rgba16().ok(),
        source.layer_source(0).unwrap().to_rgba16().ok()
    );
    assert!(Arc::ptr_eq(source.profile(), restored.profile()));
    let mut error = ptr::null_mut();
    let info: Value = serde_json::from_str(&unsafe {
        text(dr_precision_doc_source_info(&restored, 1, &mut error))
    })
    .unwrap();
    assert!(error.is_null());
    assert_eq!(info["source_id"], "fixture-a");
    assert_eq!(info["depth"], 16);
    assert_eq!(info["transform"], json!([1., 0., 0., 1., 0., 0.]));
}

#[test]
fn incremental_canvas_matrix_composition_is_noncommutative_and_translation_keeps_master() {
    let source = byte(16, 12);
    let placed = edit8(
        &source,
        Some(&source),
        json!({"op":"place","source_id":"offset","name":"Offset","x":7,"y":-3}),
    );
    let transformed = edit8(
        &placed,
        None,
        transform([2., 0., 0., 3., 5., 11.], "nearest"),
    );
    let mut err = ptr::null_mut();
    let info: Value =
        serde_json::from_str(&unsafe { text(dr_doc_source_info(&transformed, 1, &mut err)) })
            .unwrap();
    assert_eq!(info["transform"], json!([2., 0., 0., 3., 19., 2.]));
    let moved = transformed.with_layer_offset(1, 23, 8).unwrap();
    let info = moved.source_info(1).unwrap();
    assert_eq!(info["transform"], json!([2., 0., 0., 3., 23., 8.]));
}

#[test]
fn byte_bilinear_sampling_matches_independent_premultiplied_oracle() {
    let source = byte(8, 6);
    let placed = edit8(&source, Some(&source), place());
    let out = edit8(
        &placed,
        None,
        transform([1., 0., 0., 1., 0.25, 0.5], "bilinear"),
    );
    let image = out.layers[1].pixels.contiguous_image();
    for y in 0..image.height() {
        for x in 0..image.width() {
            // inverse mapped center minus half a pixel gives top-left tap coordinates.
            let u = f64::from(x) - 0.25;
            let v = f64::from(y) - 0.5;
            let ix = u.floor() as i32;
            let iy = v.floor() as i32;
            let mut premul = [0.; 4];
            for dy in 0..2 {
                for dx in 0..2 {
                    let sx = ix + dx;
                    let sy = iy + dy;
                    if sx < 0 || sy < 0 || sx >= 8 || sy >= 6 {
                        continue;
                    }
                    let weight = (if dx == 0 {
                        1. - (u - f64::from(ix))
                    } else {
                        u - f64::from(ix)
                    }) * (if dy == 0 {
                        1. - (v - f64::from(iy))
                    } else {
                        v - f64::from(iy)
                    });
                    let p_image = source.layers[0].pixels.contiguous_image();
                    let p = p_image.get_pixel(sx as u32, sy as u32);
                    let alpha = f64::from(p[3]) / 255.;
                    for c in 0..3 {
                        premul[c] += weight * f64::from(p[c]) * alpha;
                    }
                    premul[3] += weight * alpha;
                }
            }
            let mut expected = [0; 4];
            if premul[3] > 0. {
                for c in 0..3 {
                    expected[c] = (premul[c] / premul[3]).round() as u8;
                }
            }
            expected[3] = (premul[3] * 255.).round() as u8;
            if expected[3] == 0 {
                expected = [0; 4];
            }
            let actual = image.get_pixel(x, y).0;
            for c in 0..4 {
                assert!(
                    actual[c].abs_diff(expected[c]) <= 1,
                    "at {x},{y} channel {c}: {actual:?} versus {expected:?}"
                );
            }
        }
    }
}

#[test]
fn explicit_rasterize_is_exact_and_destructive_pixels_cannot_resurrect_master() {
    let source = byte(16, 12);
    let placed = edit8(&source, Some(&source), place());
    let raster = edit8(&placed, None, json!({"op":"rasterize","layer":1}));
    assert!(LayerRaster::ptr_eq(
        &raster.layers[1].pixels,
        &placed.layers[1].pixels
    ));
    assert_eq!(raster.source_info(1).unwrap()["valid"], false);
    let mut changed = placed.clone();
    changed.layers[1].pixels = LayerRaster::new(RgbaImage::from_pixel(16, 12, Rgba([1, 2, 3, 4])));
    assert_eq!(changed.source_info(1).unwrap()["valid"], false);
    let source = typed(16, 12);
    let placed = edit16(&source, Some(&source), place());
    let raster = edit16(&placed, None, json!({"op":"rasterize","layer":1}));
    assert!(PrecisionImage::ptr_eq(
        raster.layer_source(1).unwrap(),
        placed.layer_source(1).unwrap()
    ));
    assert_eq!(raster.source_info(1).unwrap()["valid"], false);
    // Phase 11C: Assign no longer silently detaches a retained source. It
    // REFUSES with a sentence naming the count, and the disclosed way through
    // rasterizes first — as one result.
    let Err(refusal) = placed.assign_profile(&IccProfile::srgb(), LIMIT, 0, None) else {
        panic!("assign must refuse while a retained source is present");
    };
    assert_eq!(refusal, placed.assign_refusal().unwrap());
    assert!(refusal.contains("1 image source"), "{refusal}");
    let assigned = placed
        .assign_profile_rasterizing(&IccProfile::srgb(), LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(assigned.source_info(1).unwrap()["valid"], false);
    assert_eq!(placed.source_info(1).unwrap()["valid"], true);
    // And the document that refused still holds its original master.
    assert!(placed.layer_source(1).unwrap().to_rgba16().ok().is_some());
}

#[test]
fn typed_assign_refuses_a_retained_source_through_the_ffi() {
    // The typed Assign export refuses with the typed refusal export's
    // sentence while a layer holds a retained source; its own bytes stay the
    // no-op (NULL, no message).
    let src = typed(16, 12);
    let placed = edit16(&src, Some(&src), place());
    let report: Value =
        serde_json::from_str(&unsafe { text(dr_precision_doc_assign_refusal(&placed)) }).unwrap();
    let refusal = report["refusal"].as_str().unwrap().to_string();
    assert!(refusal.contains("1 image source"), "{refusal}");
    let srgb = IccProfile::srgb();
    let mut e = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_assign_profile(
            &placed,
            srgb.bytes().as_ptr(),
            srgb.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(result.is_null());
    assert_eq!(unsafe { text(e) }, refusal);
    let own = IccProfile::display_p3();
    let mut e = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_assign_profile(
            &placed,
            own.bytes().as_ptr(),
            own.bytes().len(),
            LIMIT,
            0,
            ptr::null(),
            &mut e,
        )
    };
    assert!(result.is_null());
    assert!(e.is_null(), "{}", unsafe { text(e) });
}

#[test]
fn source_plans_include_original_pixels_and_enforce_limit_without_publication() {
    let source = typed(64, 48);
    let req = request(place());
    let mut err = ptr::null_mut();
    let plan: Value = serde_json::from_str(&unsafe {
        text(dr_precision_doc_source_plan(
            &source,
            &source,
            req.as_ptr(),
            &mut err,
        ))
    })
    .unwrap();
    assert!(err.is_null());
    let total = plan["retained_bytes"].as_u64().unwrap() + plan["scratch_bytes"].as_u64().unwrap();
    let token = DrJobControl::default();
    let failed = unsafe {
        dr_precision_doc_source_edit(
            &source,
            &source,
            req.as_ptr(),
            total - 1,
            0,
            &token,
            &mut err,
        )
    };
    assert!(failed.is_null());
    assert!(unsafe { text(err) }.contains("limit"));
    assert_eq!(token.scratch_bytes(), 0);
    let source = byte(64, 48);
    let mut err = ptr::null_mut();
    let plan: Value = serde_json::from_str(&unsafe {
        text(dr_doc_mixed_source_plan(
            &source,
            &source,
            ptr::null(),
            req.as_ptr(),
            &mut err,
        ))
    })
    .unwrap();
    assert!(err.is_null());
    let total = plan["retained_bytes"].as_u64().unwrap() + plan["scratch_bytes"].as_u64().unwrap();
    let failed = unsafe {
        dr_doc_mixed_source_edit(
            &source,
            &source,
            ptr::null(),
            req.as_ptr(),
            total - 1,
            0,
            ptr::null(),
            &mut err,
        )
    };
    assert!(failed.is_null());
    assert!(!unsafe { text(err) }.is_empty());
}

#[test]
fn malformed_requests_null_sweep_and_cancelled_edits_fail_atomically() {
    let source = byte(8, 8);
    let req = request(place());
    unsafe {
        assert!(dr_doc_mixed_source_plan(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            req.as_ptr(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_doc_source_info(ptr::null(), 0, ptr::null_mut()).is_null());
        assert!(dr_doc_mixed_source_edit(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            req.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_source_plan(
            ptr::null(),
            ptr::null(),
            req.as_ptr(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_precision_doc_source_info(ptr::null(), 0, ptr::null_mut()).is_null());
        assert!(dr_precision_doc_source_edit(
            ptr::null(),
            ptr::null(),
            req.as_ptr(),
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
        assert!(dr_doc_mixed_source_plan(
            &source,
            &source,
            ptr::null(),
            ptr::null(),
            ptr::null_mut()
        )
        .is_null());
    }
    for value in [
        json!({"op":"place","source_id":"../invalid","name":"x","x":0,"y":0}),
        json!({"op":"rasterize","layer":0,"unexpected":true}),
        transform([0.; 6], "nearest"),
    ] {
        assert!(SourceEdit::parse(&value.to_string()).is_err());
    }
    let token = DrJobControl::default();
    token.cancel();
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_doc_mixed_source_edit(
            &source,
            &source,
            ptr::null(),
            req.as_ptr(),
            LIMIT,
            0,
            &token,
            &mut error,
        )
    };
    assert!(output.is_null());
    assert!(unsafe { text(error) }.contains("cancel"));
    assert_eq!(token.scratch_bytes(), 0);
}

// Independent matrix/TRC fixture: identity primaries and an exact gamma curve
// isolate a conversion with the analytic oracle destination = source^gamma.
fn gamma_profile(gamma: u16) -> Arc<IccProfile> {
    let mut tags = Vec::new();
    for (axis, key) in [*b"rXYZ", *b"gXYZ", *b"bXYZ"].into_iter().enumerate() {
        let mut data = b"XYZ \0\0\0\0".to_vec();
        for c in 0..3 {
            data.extend_from_slice(&(if c == axis { 65536i32 } else { 0 }).to_be_bytes());
        }
        tags.push((key, data));
    }
    let mut curve = b"curv\0\0\0\0".to_vec();
    curve.extend_from_slice(&1u32.to_be_bytes());
    curve.extend_from_slice(&gamma.to_be_bytes());
    for key in [*b"rTRC", *b"gTRC", *b"bTRC"] {
        tags.push((key, curve.clone()));
    }
    let mut bytes = vec![0; 132 + 12 * tags.len()];
    bytes[8..12].copy_from_slice(&0x0210_0000u32.to_be_bytes());
    bytes[12..16].copy_from_slice(b"mntr");
    bytes[16..20].copy_from_slice(b"RGB ");
    bytes[20..24].copy_from_slice(b"XYZ ");
    bytes[36..40].copy_from_slice(b"acsp");
    bytes[128..132].copy_from_slice(&(tags.len() as u32).to_be_bytes());
    for (i, (key, data)) in tags.into_iter().enumerate() {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        let at = 132 + 12 * i;
        let offset = bytes.len() as u32;
        bytes[at..at + 4].copy_from_slice(&key);
        bytes[at + 4..at + 8].copy_from_slice(&offset.to_be_bytes());
        bytes[at + 8..at + 12].copy_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend(data);
    }
    let len = bytes.len() as u32;
    bytes[..4].copy_from_slice(&len.to_be_bytes());
    Arc::new(IccProfile::parse(&bytes).unwrap())
}

#[test]
fn profile_conversion_before_retained_sampling_preserves_original_master() {
    let from = gamma_profile(512);
    let to = gamma_profile(256);
    let mut source = byte(32, 24);
    source.profile = from.clone();
    let placed = edit8(&source, Some(&source), place());
    let down = edit8(
        &placed,
        None,
        transform([0.25, 0., 0., 0.25, 0., 0.], "bilinear"),
    );
    let converted = down.convert_to_profile(&to).unwrap().unwrap();
    let restored = edit8(
        &converted,
        None,
        transform([4., 0., 0., 4., 0., 0.], "bilinear"),
    );
    for (actual, original) in restored.layers[1]
        .pixels
        .contiguous_image()
        .pixels()
        .zip(source.layers[0].pixels.contiguous_image().pixels())
    {
        for c in 0..3 {
            let expected = (f64::from(original[c]).powi(2) / 255.).round() as u8;
            assert!(actual[c].abs_diff(expected) <= 1);
        }
        assert_eq!(actual[3], original[3]);
    }
    assert_eq!(
        restored.layers[1]
            .effective_source(&to)
            .unwrap()
            .asset
            .profile()
            .bytes(),
        from.bytes()
    );
    let pixels = typed(32, 24).layer_source(0).unwrap().to_rgba16().unwrap();
    let source = PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(32, 24, pixels.clone(), from.clone(), None).unwrap(),
        Metadata::default(),
        Resolution::default(),
    )
    .unwrap();
    let placed = edit16(&source, Some(&source), place());
    let down = edit16(
        &placed,
        None,
        transform([0.25, 0., 0., 0.25, 0., 0.], "bilinear"),
    );
    let converted = down
        .convert_to_profile(&to, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let restored = edit16(
        &converted,
        None,
        transform([4., 0., 0., 4., 0., 0.], "bilinear"),
    );
    for (actual, original) in restored
        .layer_source(1)
        .unwrap()
        .to_rgba16()
        .ok()
        .unwrap()
        .iter()
        .zip(&pixels)
    {
        for c in 0..3 {
            let expected = (f64::from(original[c]).powi(2) / 65535.).round() as u16;
            assert!(
                actual[c].abs_diff(expected) <= 1,
                "{actual:?} vs {original:?}"
            );
        }
        assert_eq!(actual[3], original[3]);
    }
    assert_eq!(
        restored
            .layer(1)
            .unwrap()
            .effective_source()
            .unwrap()
            .asset
            .profile()
            .bytes(),
        from.bytes()
    );
}

#[test]
fn existing_affine_and_typed_geometry_entry_points_retain_sources() {
    let source = byte(32, 24);
    let placed = edit8(&source, Some(&source), place());
    let down = placed
        .transform_layer(
            1,
            doctored_core::doc_transform::Affine::from_array([0.25, 0., 0., 0.25, 0., 0.]),
            image::imageops::FilterType::Lanczos3,
        )
        .unwrap();
    let restored = down
        .transform_layer(
            1,
            doctored_core::doc_transform::Affine::from_array([4., 0., 0., 4., 0., 0.]),
            image::imageops::FilterType::Lanczos3,
        )
        .unwrap();
    assert_eq!(
        restored.layers[1].pixels.contiguous_image().as_raw(),
        source.layers[0].pixels.contiguous_image().as_raw()
    );
    let source = typed(32, 24);
    let placed = edit16(&source, Some(&source), place());
    let edit=doctored_core::precision_geometry::GeometryEdit::parse(&json!({"op":"affine","layer":1,"target":"pixels","matrix":[0.25,0.,0.,0.25,0.,0.],"filter":"bilinear"}).to_string()).unwrap();
    let down = placed.geometry(&edit, LIMIT, 0, None).unwrap().unwrap();
    assert_eq!(down.source_info(1).unwrap()["valid"], true);
    for request in [
        json!({"op":"rotate90"}),
        json!({"op":"resize","width":16,"height":12,"filter":"nearest"}),
        json!({"op":"perspective","layer":1,"target":"pixels","quad":[0,0,31,0,32,24,0,24],"filter":"nearest"}),
    ] {
        let edit =
            doctored_core::precision_geometry::GeometryEdit::parse(&request.to_string()).unwrap();
        placed.geometry_plan(&edit).expect("source geometry plans");
        let changed = placed
            .geometry(&edit, LIMIT, 0, None)
            .expect("source geometry runs")
            .expect("source geometry changes the document");
        assert_eq!(changed.source_info(1).unwrap()["valid"], true);
    }
}

#[test]
fn near_native_capacity_plans_use_final_fallback_extent_instead_of_temporary_master() {
    // Shared 8 MiB samples make a document near its serialized reopen cap cheap
    // to construct. Every native raster still counts separately, as on reopen.
    let source = typed(1024, 1024);
    let placed = edit16(&source, Some(&source), place());
    let reduced = edit16(
        &placed,
        None,
        transform([1. / 16., 0., 0., 1. / 16., 0., 0.], "bilinear"),
    );
    let master = source.root_layers()[0].clone();
    let small = reduced.root_layers()[1].clone();
    let mut largest = None;
    for count in 80..170 {
        let mut layers = vec![master.clone(); count];
        layers.push(small.clone());
        match PrecisionDocument::from_parts(
            1024,
            1024,
            source.profile().clone(),
            Metadata::default(),
            Resolution::default(),
            layers,
        ) {
            Ok(doc) => largest = Some(doc),
            Err(_) => break,
        }
    }
    let doc = largest.expect("bounded shared fixture must fit below the native cap");
    let layer = doc.layer_count() - 1;
    let request = SourceEdit::parse(
        &json!({"op":"transform","layer":layer,"matrix":[1,0,0,1,1,2],"filter":"bilinear"})
            .to_string(),
    )
    .unwrap();
    assert!(doc.source_plan(None, &request).is_ok());
    let output = doc
        .source_edit(None, &request, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(output.layer_source(layer).unwrap().dimensions(), (64, 64));
    assert_eq!(output.layer(layer).unwrap().offset(), (1, 2));
}

#[test]
fn metadata_assignment_detaches_ordinary_source_without_changing_pixels_or_description() {
    use doctored_core::ffi_doc::dr_doc_with_layer_meta;
    let source = byte(32, 24);
    let placed = edit8(&source, Some(&source), place());
    for description in [
        r#"{"type":"adjust","op":"invert","params":{}}"#,
        r#"{"type":"text","version":1,"text":"Caption"}"#,
        r#"{"type":"shape","version":1}"#,
        r#"{"type":"live_photo","version":1}"#,
        r#"{"type":"future_source","version":999}"#,
        r#"{"annotation":"opaque metadata stays verbatim"}"#,
    ] {
        let value = CString::new(description).unwrap();
        let changed = unsafe { dr_doc_with_layer_meta(&placed, 1, value.as_ptr()) };
        assert!(!changed.is_null());
        let changed = unsafe { Box::from_raw(changed) };
        assert!(changed.layers[1].retained_source.is_none());
        assert_eq!(changed.layers[1].meta.as_deref(), Some(description));
        assert!(LayerRaster::ptr_eq(
            &changed.layers[1].pixels,
            &placed.layers[1].pixels
        ));
        assert_eq!(changed.source_info(1).unwrap()["valid"], false);
        let cleared = unsafe { dr_doc_with_layer_meta(&*changed, 1, ptr::null()) };
        assert!(!cleared.is_null());
        let cleared = unsafe { Box::from_raw(cleared) };
        assert!(cleared.layers[1].meta.is_none());
        assert!(cleared.layers[1].retained_source.is_none());
        assert_eq!(cleared.source_info(1).unwrap()["valid"], false);
        // A prior immutable revision still owns its original asset for Undo.
        assert_eq!(placed.source_info(1).unwrap()["valid"], true);
    }
}

// ------------------------------------------- G5 (12F-1): Canvas Size --
//
// Canvas Size is the ONE whole-document op a retained source survives, at 8
// and at 16 bits. The evidence is the FOLD: `effective_source` composes
// `offset − rendered_offset` into the source transform on every read, the
// resize keeps each layer's pixel allocation (so that binding guard still
// holds), and the NEXT rerender therefore lands at the new position. Every
// other whole-document op keeps its refusal.

/// The test's OWN pad/crop of one RGBA8 canvas: the old canvas's top-left
/// corner lands at `origin`, everything outside is transparent. Written here
/// so the comparison is against an independent map, not against the core's.
fn pad_crop8(src: &RgbaImage, w: u32, h: u32, origin: (i32, i32)) -> RgbaImage {
    let mut out = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
    for y in 0..src.height() {
        for x in 0..src.width() {
            let (dx, dy) = (
                i64::from(x) + i64::from(origin.0),
                i64::from(y) + i64::from(origin.1),
            );
            if dx >= 0 && dy >= 0 && dx < i64::from(w) && dy < i64::from(h) {
                out.put_pixel(dx as u32, dy as u32, *src.get_pixel(x, y));
            }
        }
    }
    out
}

/// The same map over a typed canvas's u16 samples.
fn pad_crop16(
    src: &[[u16; 4]],
    (sw, sh): (u32, u32),
    (w, h): (u32, u32),
    origin: (i32, i32),
) -> Vec<[u16; 4]> {
    let mut out = vec![[0u16; 4]; (w as usize) * (h as usize)];
    for y in 0..sh {
        for x in 0..sw {
            let (dx, dy) = (
                i64::from(x) + i64::from(origin.0),
                i64::from(y) + i64::from(origin.1),
            );
            if dx >= 0 && dy >= 0 && dx < i64::from(w) && dy < i64::from(h) {
                out[dy as usize * w as usize + dx as usize] =
                    src[y as usize * sw as usize + x as usize];
            }
        }
    }
    out
}

fn g5_place(x: i32, y: i32) -> Value {
    json!({"op":"place","source_id":"g5","name":"Placed","x":x,"y":y})
}

/// The dyadic sub-pixel translation the rerender uses: it COMMUTES with an
/// integer origin shift and every f64 sum in it is exact, so "the two
/// rerenders are equal bit for bit" is a real equality and not a tolerance.
const RERENDER: [f64; 6] = [1., 0., 0., 1., 0.25, 0.5];

#[test]
fn a_byte_canvas_size_carries_an_embedded_source_and_the_rerender_lands_at_the_new_origin() {
    // The placed picture is SMALLER than the canvas and sits wholly inside
    // it, so growing the canvas reveals nothing that was clipped before and
    // the pad/crop comparison below is an exact one.
    let canvas = byte(16, 12);
    let source = byte(6, 4);
    let mut placed = edit8(&canvas, Some(&source), g5_place(6, 4));
    // A layer mask beside the source; it rides the canvas like any other.
    placed.layers[1].mask = Some(Arc::new(image::GrayImage::from_fn(6, 4, |x, y| {
        image::Luma([((x * 5 + y * 3) % 251) as u8])
    })));
    placed.layers[1].mask_properties = doctored_core::doc_mask::MaskProperties::at((6, 4));
    let before = placed.source_info(1).unwrap();
    assert_eq!(before["valid"], true);
    let before_transform: Vec<f64> = serde_json::from_value(before["transform"].clone()).unwrap();
    let before_pixels = placed.layers[1].pixels.clone();
    let placed_flat = placed.flattened().unwrap();

    for (w, h, ox, oy, through_export, what) in [
        (
            26u32,
            20u32,
            5i32,
            3i32,
            true,
            "a grow with a non-zero origin",
        ),
        (12, 9, -3, -2, false, "a shrink with a negative origin"),
    ] {
        let moved = if through_export {
            // The EXPORT, so `doc_op`'s `detach_invalid_sources` — which
            // STORES the fold rather than recomputing it on every read — is
            // the thing under test for this case.
            let p = unsafe { dr_doc_canvas_resize(&placed, w, h, ox, oy) };
            assert!(!p.is_null(), "{what}: the export answers a document");
            let out = unsafe { &*p }.clone();
            unsafe { doctored_core::ffi_doc::dr_doc_free(p) };
            out
        } else {
            placed
                .canvas_resize(w, h, (ox, oy))
                .expect("the byte resize")
        };

        let info = moved.source_info(1).unwrap();
        assert_eq!(info["valid"], true, "{what}: the source is still valid");
        let after: Vec<f64> = serde_json::from_value(info["transform"].clone()).unwrap();
        let mut expected = before_transform.clone();
        expected[4] += f64::from(ox);
        expected[5] += f64::from(oy);
        assert_eq!(after, expected, "{what}: the transform gained the origin");
        assert!(
            LayerRaster::ptr_eq(&moved.layers[1].pixels, &before_pixels),
            "{what}: the allocation the binding guard reads is preserved"
        );
        assert_eq!(
            moved.layers[1].offset,
            (
                placed.layers[1].offset.0 + ox,
                placed.layers[1].offset.1 + oy
            ),
            "{what}: the layer moved by the origin"
        );
        assert_eq!(
            moved.layers[1].mask_properties.offset,
            (
                placed.layers[1].mask_properties.offset.0 + ox,
                placed.layers[1].mask_properties.offset.1 + oy
            ),
            "{what}: the mask rode with it"
        );

        // THE RERENDER: the same source edit on both documents.
        let placed_again = edit8(&placed, None, transform(RERENDER, "bilinear"));
        let moved_again = edit8(&moved, None, transform(RERENDER, "bilinear"));
        assert_eq!(
            moved_again.layers[1].pixels.contiguous_image().as_raw(),
            placed_again.layers[1].pixels.contiguous_image().as_raw(),
            "{what}: a translation commutes with the origin shift, bit for bit"
        );
        assert_eq!(
            moved_again.layers[1].offset,
            (
                placed_again.layers[1].offset.0 + ox,
                placed_again.layers[1].offset.1 + oy
            ),
            "{what}: and the rerender landed at the NEW position"
        );
        assert_eq!(
            moved_again.layers[1].mask_properties.offset,
            (
                placed_again.layers[1].mask_properties.offset.0 + ox,
                placed_again.layers[1].mask_properties.offset.1 + oy
            ),
            "{what}: the mask is still where the origin put it"
        );
        let expected_canvas = pad_crop8(&placed_again.flattened().unwrap(), w, h, (ox, oy));
        assert_eq!(
            moved_again.flattened().unwrap().as_raw(),
            expected_canvas.as_raw(),
            "{what}: the whole canvas is the test's own pad/crop of the un-resized one"
        );
    }

    // UNDO IS PURITY: nothing above touched `placed`.
    assert_eq!(placed.source_info(1).unwrap(), before);
    assert!(LayerRaster::ptr_eq(
        &placed.layers[1].pixels,
        &before_pixels
    ));
    assert_eq!(placed.flattened().unwrap().as_raw(), placed_flat.as_raw());
}

#[test]
fn a_typed_canvas_size_and_other_whole_ops_carry_an_embedded_source() {
    let canvas = typed(16, 12);
    let source = typed(6, 4);
    let placed = edit16(&canvas, Some(&source), g5_place(6, 4));
    let mask = doctored_core::precision_mask::PrecisionMask::new(
        doctored_core::precision_mask::PrecisionPlane::from_u16(
            6,
            4,
            (0..6 * 4).map(|i| (i * 211) as u16).collect(),
            None,
        )
        .unwrap(),
        doctored_core::doc_mask::MaskProperties::at((6, 4)),
        true,
    )
    .unwrap();
    let placed = placed
        .set_layer_mask(1, Some(mask), LIMIT, 0, None)
        .expect("a mask beside a source")
        .expect("attaching a mask is a change");

    let info = |doc: &PrecisionDocument| -> Value {
        let mut err = ptr::null_mut();
        let out: Value =
            serde_json::from_str(&unsafe { text(dr_precision_doc_source_info(doc, 1, &mut err)) })
                .unwrap();
        assert!(err.is_null());
        out
    };
    let before = info(&placed);
    assert_eq!(before["valid"], true);
    let before_transform: Vec<f64> = serde_json::from_value(before["transform"].clone()).unwrap();
    let before_pixels = placed.layer_source(1).unwrap().clone();
    let placed_flat = placed.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap();

    for (w, h, ox, oy, what) in [
        (26u32, 20u32, 5i32, 3i32, "a grow with a non-zero origin"),
        (12, 9, -3, -2, "a shrink with a negative origin"),
    ] {
        let edit = doctored_core::precision_geometry::GeometryEdit::parse(
            &json!({"op":"canvas_size","width":w,"height":h,"x":ox,"y":oy}).to_string(),
        )
        .unwrap();
        let moved = placed
            .geometry(&edit, LIMIT, 0, None)
            .expect("Canvas Size is LIFTED at 16 bits")
            .expect("the canvas changed");

        let after_info = info(&moved);
        assert_eq!(after_info["valid"], true, "{what}: still valid");
        let after: Vec<f64> = serde_json::from_value(after_info["transform"].clone()).unwrap();
        let mut expected = before_transform.clone();
        expected[4] += f64::from(ox);
        expected[5] += f64::from(oy);
        assert_eq!(after, expected, "{what}: the transform gained the origin");
        assert!(
            PrecisionImage::ptr_eq(moved.layer_source(1).unwrap(), &before_pixels),
            "{what}: the allocation the binding guard reads is preserved"
        );
        assert_eq!(
            moved.layer(1).unwrap().offset(),
            (
                placed.layer(1).unwrap().offset().0 + ox,
                placed.layer(1).unwrap().offset().1 + oy
            ),
            "{what}: the layer moved by the origin"
        );
        assert_eq!(
            moved.layer(1).unwrap().mask().unwrap().properties().offset,
            (
                placed
                    .layer(1)
                    .unwrap()
                    .mask()
                    .unwrap()
                    .properties()
                    .offset
                    .0
                    + ox,
                placed
                    .layer(1)
                    .unwrap()
                    .mask()
                    .unwrap()
                    .properties()
                    .offset
                    .1
                    + oy
            ),
            "{what}: the mask rode with it"
        );

        let placed_again = edit16(&placed, None, transform(RERENDER, "bilinear"));
        let moved_again = edit16(&moved, None, transform(RERENDER, "bilinear"));
        assert_eq!(
            moved_again.layer_source(1).unwrap().to_rgba16().ok(),
            placed_again.layer_source(1).unwrap().to_rgba16().ok(),
            "{what}: the two rerenders are equal bit for bit"
        );
        assert_eq!(
            moved_again.layer(1).unwrap().offset(),
            (
                placed_again.layer(1).unwrap().offset().0 + ox,
                placed_again.layer(1).unwrap().offset().1 + oy
            ),
            "{what}: and the rerender landed at the NEW position"
        );
        let expected_canvas = pad_crop16(
            &placed_again
                .render(LIMIT, 0, None)
                .unwrap()
                .to_rgba16()
                .ok()
                .unwrap(),
            placed_again.dimensions(),
            (w, h),
            (ox, oy),
        );
        assert_eq!(
            moved_again
                .render(LIMIT, 0, None)
                .unwrap()
                .to_rgba16()
                .ok()
                .unwrap(),
            expected_canvas.as_slice(),
            "{what}: the whole canvas is the test's own pad/crop of the un-resized one"
        );
    }

    // UNDO IS PURITY.
    assert_eq!(info(&placed), before);
    assert!(PrecisionImage::ptr_eq(
        placed.layer_source(1).unwrap(),
        &before_pixels
    ));
    assert_eq!(
        placed.render(LIMIT, 0, None).unwrap().to_rgba16().unwrap(),
        placed_flat
    );

    // 12F-2: the other whole-document ops compose against the same master.
    for request in [
        json!({"op":"crop","x":1,"y":1,"width":8,"height":6}),
        json!({"op":"rotate90"}),
        json!({"op":"resize","width":8,"height":6,"filter":"nearest"}),
    ] {
        let edit =
            doctored_core::precision_geometry::GeometryEdit::parse(&request.to_string()).unwrap();
        placed
            .geometry_plan(&edit)
            .expect("retained whole geometry plans");
        let changed = placed
            .geometry(&edit, LIMIT, 0, None)
            .expect("retained whole geometry runs")
            .expect("retained whole geometry changes the document");
        assert_eq!(info(&changed)["source_id"], before["source_id"]);
        assert_eq!(info(&changed)["valid"], true);
    }
}
