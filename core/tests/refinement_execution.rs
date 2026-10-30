//! Shared byte cutout composition/record-copy qualification through the FFI.
use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use common::*;
use doctored_core::doc::LayerRaster;
use doctored_core::job_control::DrJobControl;
use doctored_core::{doc::DrDocument, ffi::*, ffi_cutout::*, ffi_jobs::*};
use image::{GrayImage, Rgba, RgbaImage};
use std::{ptr, sync::Arc};

fn assemble(document: &DrDocument, foreground: &RgbaImage, alpha: &[u8]) -> DrDocument {
    let image = unsafe {
        Box::from_raw(dr_image_from_rgba(
            foreground.as_raw().as_ptr(),
            foreground.width(),
            foreground.height(),
        ))
    };
    let mut error = ptr::null_mut();
    let output = unsafe {
        dr_doc_add_refinement_cutout(document, &*image, alpha.as_ptr(), ptr::null(), &mut error)
    };
    let (output, error) = take_doc(output, error);
    assert!(error.is_none(), "{error:?}");
    output.unwrap()
}

fn projected(document: &DrDocument) -> Result<RgbaImage, String> {
    let mut error = ptr::null_mut();
    let output = unsafe { dr_doc_render_refinement_cutout(document, ptr::null(), &mut error) };
    if output.is_null() {
        return Err(take_err_string(error));
    }
    assert!(error.is_null());
    let (width, height) = img_dims(output);
    let bytes = img_pixels(output);
    unsafe {
        dr_image_free(output);
    }
    Ok(RgbaImage::from_raw(width, height, bytes).unwrap())
}

#[test]
fn every_byte_foreground_and_coverage_pair_matches_the_full_compositor_and_oracle() {
    let foreground = RgbaImage::from_fn(256, 256, |x, _| {
        Rgba([x as u8, 255 - x as u8, (x as u8).wrapping_mul(73), 255])
    });
    let alpha: Vec<u8> = (0..256).flat_map(|y| vec![y as u8; 256]).collect();
    let document = doc_of(256, 256, |_, _| [17, 29, 91]);
    let cutout = assemble(&document, &foreground, &alpha);
    let output = projected(&cutout).unwrap();
    assert_eq!(output, cutout.flattened().unwrap());
    for ((pixel, source), &coverage) in output.pixels().zip(foreground.pixels()).zip(&alpha) {
        assert_eq!(
            *pixel,
            if coverage == 0 {
                Rgba([0; 4])
            } else {
                Rgba([source[0], source[1], source[2], coverage])
            }
        );
    }
}

/// A foreground that carries its own alpha (0, half, nearly opaque and opaque
/// pixels) keeps it on the cutout layer, and the bounded projection is the
/// full compositor's.
#[test]
fn a_foreground_with_its_own_alpha_projects_as_the_full_compositor_does() {
    let foreground = RgbaImage::from_fn(64, 48, |x, y| {
        let alpha = [0, 128, 254, 255][((x + y) % 4) as usize];
        Rgba([x as u8 * 3, 200 - y as u8, 77, alpha])
    });
    let alpha: Vec<u8> = (0..64 * 48).map(|i| (i * 5 % 256) as u8).collect();
    let document = doc_of(64, 48, |_, _| [17, 29, 91]);
    let cutout = assemble(&document, &foreground, &alpha);
    assert_eq!(cutout.layers[1].pixels.to_contiguous().unwrap(), foreground);
    assert_eq!(projected(&cutout).unwrap(), cutout.flattened().unwrap());
}

#[test]
fn chunk_origins_wide_rows_grouped_originals_and_record_strings_remain_exact() {
    let mut original = doc_of(5003, 3, |x, y| [x as u8, y as u8, 19]);
    let text = "a🧭界".repeat(30_000); // str boundaries cross a 64 KiB copy chunk.
    original.layers[0].name = "海🧭".repeat(6000);
    original.layers[0].meta = Some(text.clone());
    original.metadata.xmp = Some(Arc::from([7_u8; 1000]));
    original.channels.push(doctored_core::doc_channel::Channel {
        id: 77,
        name: "Ch🧭".repeat(15_000),
        data: Arc::new(GrayImage::from_pixel(5003, 3, image::Luma([117]))),
        overlay_color: [3, 79, 211],
        overlay_opacity: 0.625,
        color_indicates_selected: true,
        spot: None,
    });
    original.guides.push(doctored_core::doc_guide::Guide {
        id: 91,
        orientation: doctored_core::doc_guide::GuideOrientation::Vertical,
        position: 1.25,
    });
    original = original.group_layers(&[0], "Original group").unwrap().0;
    let foreground = RgbaImage::from_fn(5003, 3, |x, y| {
        Rgba([x as u8, (x >> 8) as u8, y as u8, 255])
    });
    let alpha: Vec<u8> = (0..5003 * 3).map(|i| (i * 37) as u8).collect();
    let cutout = assemble(&original, &foreground, &alpha);
    assert_eq!(projected(&cutout).unwrap(), cutout.flattened().unwrap());
    assert_eq!(cutout.layers[0].name, original.layers[0].name);
    assert_eq!(cutout.channels[0].id, 77);
    assert_eq!(cutout.channels[0].name, original.channels[0].name);
    assert_eq!(cutout.channels[0].overlay_color, [3, 79, 211]);
    assert_eq!(cutout.channels[0].overlay_opacity, 0.625);
    assert!(cutout.channels[0].color_indicates_selected);
    assert!(Arc::ptr_eq(
        &cutout.channels[0].data,
        &original.channels[0].data
    ));
    assert_eq!(cutout.guides.len(), original.guides.len());
    assert_eq!(cutout.guides[0].id, original.guides[0].id);
    assert_eq!(cutout.guides[0].orientation, original.guides[0].orientation);
    assert_eq!(
        cutout.guides[0].position.to_bits(),
        original.guides[0].position.to_bits()
    );
    assert_eq!(cutout.layers[0].meta.as_deref(), Some(text.as_str()));
    assert_eq!(cutout.layers[0].depth, original.layers[0].depth);
    assert!(cutout.layers[0].visible); // Child flag survives hidden enclosing root.
    assert!(!cutout.layers[1].visible);
    assert_eq!(cutout.layers[2].depth, 0);
    assert!(LayerRaster::ptr_eq(
        &cutout.layers[0].pixels,
        &original.layers[0].pixels
    ));
    assert!(Arc::ptr_eq(&cutout.profile, &original.profile));
    assert!(Arc::ptr_eq(
        cutout.metadata.xmp.as_ref().unwrap(),
        original.metadata.xmp.as_ref().unwrap()
    ));
}

#[test]
fn cutout_projection_refuses_changed_shape_nulls_budget_and_precancel_without_output() {
    let original = doc_of(3, 2, |_, _| [7, 13, 19]);
    let foreground = RgbaImage::from_pixel(3, 2, Rgba([17, 73, 209, 255]));
    let cutout = assemble(&original, &foreground, &[127; 6]);
    for change in 0..10 {
        let mut bad = cutout.clone();
        match change {
            0 => bad.layers[0].visible = true,
            1 => bad.layers[1].visible = false,
            2 => bad.layers[1].opacity = 0.5,
            3 => bad.layers[1].offset = (1, 0),
            4 => bad.layers[1].mask_enabled = false,
            5 => bad.layers[1].mask_properties.density = 0.5,
            6 => bad.layers[1].mask_properties.feather = 1.0,
            7 => bad.layers[1].mask_properties.offset = (1, 0),
            8 => bad.layers[1].meta = Some("{}".into()),
            9 => bad.layers[1].mask = Some(Arc::new(GrayImage::new(1, 1))),
            _ => unreachable!(),
        }
        assert!(projected(&bad).is_err(), "shape change {change}");
    }
    unsafe {
        assert_eq!(
            dr_doc_refinement_cutout_render_scratch_bytes(ptr::null(), ptr::null_mut()),
            0
        );
        assert!(
            dr_doc_render_refinement_cutout(ptr::null(), ptr::null(), ptr::null_mut()).is_null()
        );
        let scratch = dr_doc_refinement_cutout_render_scratch_bytes(&cutout, ptr::null_mut());
        assert!(scratch >= 4 * 6 + 4096 * 20);
        for precancel in [false, true] {
            let token = dr_job_control_create();
            assert!(dr_job_control_set_scratch_limit(
                token,
                if precancel { scratch } else { scratch - 1 }
            ));
            if precancel {
                dr_job_control_cancel(token);
            }
            let mut error = ptr::null_mut();
            assert!(dr_doc_render_refinement_cutout(&cutout, token, &mut error).is_null());
            let error = take_err_string(error);
            assert!(
                error.contains(if precancel {
                    "cancelled"
                } else {
                    "working memory"
                }),
                "{error}"
            );
            assert_eq!(dr_job_control_scratch_bytes(token), 0);
            dr_job_control_free(token);
        }
    }
}

#[test]
fn cancel_during_owned_record_metadata_copy_drops_partial_clone() {
    let fixture = || {
        let mut original = doc_of(1, 1, |_, _| [17, 73, 209]);
        for i in 0..12 {
            if i > 0 {
                original.layers.push(original.layers[0].clone());
            }
            original.layers[i].meta = Some("🧭".repeat(4 * 1024 * 1024));
        }
        original
    };
    let foreground =
        unsafe { Box::from_raw(dr_image_from_rgba([17, 73, 209, 255].as_ptr(), 1, 1)) };
    let cut = |original: &DrDocument, t: usize| unsafe {
        let budget = dr_doc_refinement_cutout_scratch_bytes(original);
        assert!(dr_job_control_set_scratch_limit(
            t as *const DrJobControl,
            budget
        ));
        let mut error = ptr::null_mut();
        let output = dr_doc_add_refinement_cutout(
            original,
            &*foreground,
            [127].as_ptr(),
            t as _,
            &mut error,
        );
        let (output, error) = take_doc(output, error);
        (output.is_some(), error.unwrap_or_default())
    };
    let (entry, _) = {
        let twin = fixture();
        entry_then_cancel(&|progress, stage, _| stage == 2 && progress >= 0.91, |t| {
            cut(&twin, t)
        })
    };
    // k = 0: the record copy checks only at its reports (the next report's
    // check is n0 + 1), so the park is that report's own check, stage 2.
    let n0 = entry.checkpoints;
    let original = fixture();
    let parked = cancelled_parked_at(n0 + 1, &|_, stage| stage == entry.stage, |t| {
        cut(&original, t)
    });
    println!("metadata copy cancellation: {}", parked_line(&parked));
    assert_eq!(parked.stage, 2);
    assert!(parked.progress >= 0.91 && parked.progress < 0.98);
    assert!(parked.scratch > 0);
    assert!(original
        .layers
        .iter()
        .all(|layer| layer.visible && layer.meta.as_ref().unwrap().len() == 16 * 1024 * 1024));
}

#[test]
fn cancel_inside_shared_compositor_chunks_drops_partial_projection() {
    let fixture = || {
        let original = doc_of(2048, 2048, |_, _| [17, 73, 209]);
        let foreground = RgbaImage::from_pixel(2048, 2048, Rgba([29, 79, 211, 255]));
        assemble(&original, &foreground, &vec![127; 2048 * 2048])
    };
    let render = |cutout: &DrDocument, t: usize| unsafe {
        let budget = dr_doc_refinement_cutout_render_scratch_bytes(cutout, ptr::null_mut());
        assert!(dr_job_control_set_scratch_limit(
            t as *const DrJobControl,
            budget
        ));
        let mut error = ptr::null_mut();
        let output = dr_doc_render_refinement_cutout(cutout, t as _, &mut error);
        if !output.is_null() {
            dr_image_free(output);
        }
        (
            !output.is_null(),
            if error.is_null() {
                String::new()
            } else {
                take_err_string(error)
            },
        )
    };
    let (entry, _) = {
        let twin = fixture();
        entry_then_cancel(&|progress, _, _| progress >= 0.10, |t| render(&twin, t))
    };
    // k = 0: the compositor's chunk loop (`render_refinement_cutout`) checks
    // only at its per-chunk report, so the park is the next chunk's report
    // check: between two chunks of the shared compositor, still stage 6.
    let n0 = entry.checkpoints;
    let cutout = fixture();
    let budget = unsafe { dr_doc_refinement_cutout_render_scratch_bytes(&cutout, ptr::null_mut()) };
    let parked = cancelled_parked_at(n0 + 1, &|_, stage| stage == entry.stage, |t| {
        render(&cutout, t)
    });
    println!("compositor chunk cancellation: {}", parked_line(&parked));
    assert_eq!(parked.stage, 6);
    assert!(parked.progress >= 0.1 && parked.progress < 1.0);
    assert_eq!(parked.scratch, budget);
}

#[test]
fn hidden_calibrated_originals_keep_the_full_projection_profile_refusal() {
    use doctored_core::icc::IccProfile;
    let mut original = doc_of(2, 2, |_, _| [17, 73, 209]);
    let mut profile = IccProfile::srgb().bytes()[..128].to_vec();
    profile.extend_from_slice(&0u32.to_be_bytes());
    profile[..4].copy_from_slice(&132u32.to_be_bytes());
    original.profile = Arc::new(IccProfile::parse(&profile).unwrap());
    original.layers[0].meta = Some(
        r#"{"type":"adjust","op":"exposure","params":{"evaluation":"document_v2","exposure":1}}"#
            .into(),
    );
    let foreground = RgbaImage::from_pixel(2, 2, Rgba([17, 73, 209, 255]));
    let cutout = assemble(&original, &foreground, &[127; 4]);
    assert!(!cutout.layers[0].visible);
    let standard = cutout.flattened().unwrap_err();
    let controlled = projected(&cutout).unwrap_err();
    assert!(standard.contains("matrix/TRC"));
    assert_eq!(controlled, standard);
}

#[test]
fn opaque_profile_large_hidden_json_is_forecast_before_parse_and_budget_refuses() {
    use doctored_core::icc::IccProfile;
    let mut original = doc_of(1, 1, |_, _| [17, 73, 209]);
    let json = format!("[{}0]", "0,".repeat(2 * 1024 * 1024));
    original.layers[0].meta = Some(json);
    let foreground = RgbaImage::from_pixel(1, 1, Rgba([17, 73, 209, 255]));
    let matrix = assemble(&original, &foreground, &[127]);
    let cheap = unsafe { dr_doc_refinement_cutout_render_scratch_bytes(&matrix, ptr::null_mut()) };
    assert!(cheap < 2 * 1024 * 1024);
    assert_eq!(projected(&matrix).unwrap(), matrix.flattened().unwrap());
    let mut profile = IccProfile::srgb().bytes()[..128].to_vec();
    profile.extend_from_slice(&0u32.to_be_bytes());
    profile[..4].copy_from_slice(&132u32.to_be_bytes());
    let mut opaque = matrix.clone();
    opaque.profile = Arc::new(IccProfile::parse(&profile).unwrap());
    let required =
        unsafe { dr_doc_refinement_cutout_render_scratch_bytes(&opaque, ptr::null_mut()) };
    assert_eq!(
        required,
        cheap + opaque.layers[0].meta.as_ref().unwrap().len() as u64 * 256
    );
    unsafe {
        let token = dr_job_control_create();
        assert!(dr_job_control_set_scratch_limit(token, cheap));
        let mut error = ptr::null_mut();
        assert!(dr_doc_render_refinement_cutout(&opaque, token, &mut error).is_null());
        assert!(take_err_string(error).contains("working memory"));
        assert_eq!(dr_job_control_scratch_bytes(token), 0);
        assert_eq!(dr_job_control_progress(token), 0.0);
        assert_eq!(dr_job_control_stage(token), 1); // Refused before context parsing/pixel stages.
        dr_job_control_free(token);
    }
}
