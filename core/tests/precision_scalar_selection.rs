//! Native scalar/source/selection qualification through additive C contracts.
//! Numerical expectations use original codes, direct integer/f64 arithmetic,
//! brute-force distances and a separately constructed Gaussian kernel.
use crate::common;
use crate::native_scalar as native;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};
use doctored_core::ffi_precision_edit::*;
use doctored_core::ffi_selection_u16::*;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use doctored_core::precision_image::PrecisionImage;
use native::*;
use serde_json::json;
use std::ptr;

#[test]
fn scalar_rgba_fill_preserves_other_components_and_tiny_outside_codes() {
    let original = vec![
        [1, 7, 12345, 0],
        [123, 257, 32769, 1],
        [32001, 33002, 34003, 65535],
        [11, 12, 13, 17],
    ];
    let d = doc(4, 1, original.clone());
    let coverage = [1, 32769, 65535, 0];
    for (channel, target) in ["red", "green", "blue", "alpha"].iter().enumerate() {
        let next = scalar(
            &d,
            target,
            json!({"kind":"fill","value":50001}),
            true,
            &coverage,
        )
        .unwrap();
        let output = next.layer_source(0).unwrap().to_rgba16().ok().unwrap();
        for i in 0..4 {
            for c in 0..4 {
                let expected = if c == channel {
                    q((f64::from(original[i][c]) * f64::from(65535 - coverage[i])
                        + 50001. * f64::from(coverage[i]))
                        / 65535.)
                } else {
                    original[i][c]
                };
                assert_eq!(output[i][c], expected, "{target} pixel{i} component{c}");
            }
        }
    }
    assert_eq!(
        d.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        original
    );
}
#[test]
fn selected_mask_paint_preserves_raw_extent_placement_and_properties() {
    let d = doc(3, 1, vec![[11001, 22002, 33003, 40004]; 3]);
    let d=edit(&d,json!({"op":"set_mask","layer":0,"width":5,"height":1,"x":-1,"y":0,"density":0.7,"feather":2.25,"linked":false,"enabled":false}),&[1,3,10001,50001,65534]).unwrap();
    let original = plane(&d, "mask", false);
    assert_eq!(plane(&d, "mask", true), [3, 10001, 50001]);
    let next = scalar(
        &d,
        "mask",
        json!({"kind":"paint","value":65535,"opacity":0.73}),
        true,
        &[1, 32769, 65535, 65535, 12345, 0],
    )
    .unwrap();
    let mask = next.layer(0).unwrap().mask().unwrap();
    let props = mask.properties();
    assert_eq!(props.offset, (-1, 0));
    assert!(!props.linked);
    assert!(!mask.enabled());
    assert_eq!(props.feather, 2.25);
    assert_eq!(props.density, 0.7);
    let mut expected = original.clone();
    for x in 0..3 {
        let t = f64::from([1, 32769, 65535][x]) / 65535. * f64::from([65535, 12345, 0][x]) / 65535.
            * 0.73;
        expected[x + 1] = q(f64::from(original[x + 1]) * (1. - t) + 65535. * t);
    }
    assert_eq!(mask.plane().samples(), expected);
    assert_eq!(plane(&d, "mask", false), original);
    assert_eq!(
        next.layer_source(0).unwrap().to_rgba16().ok(),
        d.layer_source(0).unwrap().to_rgba16().ok()
    );
}
#[test]
fn scalar_filter_evaluates_hidden_rgb_and_quantizes_after_selection() {
    let pixels = vec![
        [10001, 20002, 30003, 0],
        [10002, 20003, 30004, 1],
        [65533, 1, 9, 12345],
    ];
    let d = doc(3, 1, pixels.clone());
    let description=json!({"type":"adjust","evaluation":"precision_v1","op":"bcs","params":{"brightness":2./65535.}}).to_string();
    let operation =
        json!({"kind":"filter","edit":{"layer":0,"op":"adjustment","description":description}});
    let next = scalar(&d, "red", operation, true, &[65535, 32769, 0]).unwrap();
    let out = next.layer_source(0).unwrap().to_rgba16().ok().unwrap();
    assert_eq!(out[0], [10003, 20002, 30003, 0]);
    assert_eq!(out[1], [10003, 20003, 30004, 1]);
    assert_eq!(out[2], pixels[2]);
}
#[test]
fn scalar_spatial_filter_reads_full_offset_storage_only_writes_selected_sample() {
    let source = PrecisionImage::from_rgba16(
        4,
        1,
        vec![
            [1, 2, 3, 0],
            [10001, 11, 13, 1],
            [20001, 21, 23, 123],
            [30001, 31, 33, 65535],
        ],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let layer = PrecisionDocumentLayer::raster("offset", source, (-1, 0), 0.73, true).unwrap();
    let d = PrecisionDocument::from_parts(
        2,
        1,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        vec![layer],
    )
    .unwrap();
    let next = scalar(
        &d,
        "red",
        json!({"kind":"filter","edit":{"layer":0,"op":"pixelate","params":{"block":4}}}),
        true,
        &[65535, 0],
    )
    .unwrap();
    let expected = vec![
        [1, 2, 3, 0],
        [15001, 11, 13, 1],
        [20001, 21, 23, 123],
        [30001, 31, 33, 65535],
    ];
    assert_eq!(
        next.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        expected
    );
    assert_eq!(next.layer(0).unwrap().offset(), (-1, 0));
}
#[test]
fn native_channel_replace_and_noop_retain_identity_and_exact_precision() {
    let d = doc(5, 1, vec![[11, 12, 13, 1]; 5]);
    let d = edit(
        &d,
        json!({"op":"set_channel","name":"fine"}),
        &[0, 1, 257, 32769, 65535],
    )
    .unwrap();
    let id = d.channels()[0].id();
    let next = scalar(
        &d,
        "channel",
        json!({"kind":"replace"}),
        true,
        &[123, 124, 125, 126, 127, 0, 1, 65535, 32769, 0],
    )
    .unwrap();
    assert_eq!(next.channels()[0].id(), id);
    assert_eq!(next.channels()[0].name(), "fine");
    assert_eq!(plane(&next, "channel", false), [0, 1, 125, 16447, 65535]);
    assert!(scalar(
        &next,
        "channel",
        json!({"kind":"fill","value":5555}),
        true,
        &[0; 5]
    )
    .is_none());
    assert!(scalar(
        &next,
        "channel",
        json!({"kind":"replace"}),
        false,
        &plane(&next, "channel", false)
    )
    .is_none());
}
#[test]
fn authoritative_selection_combine_invert_and_quick_mask_keep_native_codes() {
    let base = [0, 1, 257, 32769, 65535];
    let incoming = [1, 0, 258, 49151, 2];
    for mode in ["replace", "add", "subtract", "intersect"] {
        let input = [base.as_slice(), incoming.as_slice()].concat();
        let out = selection(5, 1, json!({"kind":"combine","mode":mode}), &input);
        let expected: Vec<u16> = base
            .iter()
            .zip(incoming)
            .map(|(&a, b)| match mode {
                "replace" => b,
                "add" => a.max(b),
                "subtract" => a.min(65535 - b),
                _ => a.min(b),
            })
            .collect();
        assert_eq!(out, expected);
    }
    let inverted = selection(5, 1, json!({"kind":"invert"}), &base);
    assert_eq!(selection(5, 1, json!({"kind":"invert"}), &inverted), base);
    let gray = [0, 1, 123, 12345, 65535];
    let alpha = [0, 1, 257, 32769, 65535];
    let input = [base.as_slice(), gray.as_slice(), alpha.as_slice()].concat();
    let out = selection(5, 1, json!({"kind":"paint"}), &input);
    for i in 0..5 {
        assert_eq!(
            out[i],
            q(f64::from(gray[i]) + f64::from(base[i]) * f64::from(65535 - alpha[i]) / 65535.)
        );
    }
}
#[test]
fn native_feather_matches_independent_separable_gaussian_and_identity() {
    let (w, h) = (7_usize, 5_usize);
    let values: Vec<u16> = (0..w * h)
        .map(|i| ((i * 1877 + 1) % 65536) as u16)
        .collect();
    for radius in [0_f64, 0.25, 2., 4.] {
        let out = selection(
            w as u32,
            h as u32,
            json!({"kind":"feather","radius":radius}),
            &values,
        );
        if radius == 0. {
            assert_eq!(out, values);
            continue;
        }
        let size = 2. * radius + 1.;
        let mut taps = size.round().max(1.) as usize;
        if taps.is_multiple_of(2) {
            taps = if size - (taps - 1) as f64 <= (taps + 1) as f64 - size {
                taps - 1
            } else {
                taps + 1
            };
        }
        taps = taps.max(3);
        let half = (taps / 2) as isize;
        let sigma = 0.3 * (radius - 1.) + 0.8;
        let weights: Vec<f64> = (0..taps)
            .map(|i| (-0.5 * ((i as isize - half) as f64 / sigma).powi(2)).exp())
            .collect();
        let sum: f64 = weights.iter().sum();
        for y in 0..h {
            for x in 0..w {
                let mut expected = 0.;
                for (dy, wy) in weights.iter().enumerate() {
                    for (dx, wx) in weights.iter().enumerate() {
                        let sx =
                            (x as isize + dx as isize - half).clamp(0, w as isize - 1) as usize;
                        let sy =
                            (y as isize + dy as isize - half).clamp(0, h as isize - 1) as usize;
                        expected += f64::from(values[sy * w + sx]) * wx * wy / (sum * sum);
                    }
                }
                assert!(out[y * w + x].abs_diff(q(expected)) <= 1);
            }
        }
    }
}
#[test]
fn native_morphology_matches_bruteforce_contours_below_byte_resolution() {
    let (w, h) = (7_usize, 5_usize);
    let input: Vec<u16> = (0..w * h)
        .map(|i| {
            if (2..=4).contains(&(i % w)) && (1..=3).contains(&(i / w)) {
                32768
            } else {
                32767
            }
        })
        .collect();
    for mode in ["grow", "shrink", "border"] {
        let radius = 0.731;
        let out = selection(
            w as u32,
            h as u32,
            json!({"kind":mode,"radius":radius}),
            &input,
        );
        for i in 0..input.len() {
            let inside = input[i] >= 32768;
            let mut distance = f64::INFINITY;
            for (j, &value) in input.iter().enumerate() {
                if (value >= 32768) != inside {
                    let dx = (i % w) as f64 - (j % w) as f64;
                    let dy = (i / w) as f64 - (j / w) as f64;
                    distance = distance.min(dx.hypot(dy));
                }
            }
            let s = if inside {
                distance - 0.5
            } else {
                0.5 - distance
            };
            let d = match mode {
                "grow" => s + radius,
                "shrink" => s - radius,
                _ => radius * 0.5 - s.abs(),
            };
            assert!(out[i].abs_diff(q((d + 0.5).clamp(0., 1.) * 65535.)) <= 1);
        }
    }
}
#[test]
fn native_affine_selection_preserves_tiny_codes_and_fractional_edge() {
    let input = [1, 32769, 65535, 257, 12345, 55555];
    let out = selection(
        3,
        2,
        json!({"kind":"transform","matrix":[1,0,0,1,1,0],"filter":"nearest"}),
        &input,
    );
    assert_eq!(out, [0, 1, 32769, 0, 257, 12345]);
    let half = selection(
        3,
        2,
        json!({"kind":"transform","matrix":[1,0,0,1,0.5,0],"filter":"bilinear"}),
        &input,
    );
    assert_eq!(half, [1, 16385, 49152, 129, 6301, 33950]);
}
#[test]
fn typed_source_alpha_luminance_color_wand_range_quick_distinguish_native_codes() {
    let input = vec![
        [30000, 30000, 30000, 1],
        [30001, 30001, 30001, 257],
        [30003, 30003, 30003, 32769],
        [30000, 30000, 30000, 0],
    ];
    let d = doc(4, 1, input.clone());
    assert_eq!(select(&d, json!({"kind":"alpha"})), [1, 257, 32769, 0]);
    assert_eq!(
        select(&d, json!({"kind":"color","channel":"red"})),
        [30000, 30001, 30003, 30000]
    );
    assert_eq!(
        select(&d, json!({"kind":"luminance"})),
        [30000, 30001, 30003, 30000]
    );
    assert_eq!(
        select(
            &d,
            json!({"kind":"color_range","params":{"mode":"sampled","samples":[[30001,30001,30001]],"tolerance":0,"softness":0}})
        ),
        [0, 257, 0, 0]
    );
    assert_eq!(
        select(
            &d,
            json!({"kind":"quick","params":{"seeds":[[1,0]],"tolerance":0,"edge":0}})
        ),
        [0, 257, 0, 0]
    );
    let w = doc(
        4,
        1,
        vec![
            [30000, 0, 0, 65535],
            [30001, 0, 0, 65535],
            [30003, 0, 0, 65535],
            [30000, 0, 0, 65535],
        ],
    );
    assert_eq!(
        select(
            &w,
            json!({"kind":"magic_wand","x":0,"y":0,"tolerance":1,"contiguous":true})
        ),
        [65535, 65535, 0, 0]
    );
    assert_eq!(
        select(
            &w,
            json!({"kind":"magic_wand","x":0,"y":0,"tolerance":0,"contiguous":false})
        ),
        [65535, 0, 0, 65535]
    );
}
#[test]
fn source_and_raw_scalar_canvas_placement_excludes_offcanvas_storage() {
    let image = PrecisionImage::from_rgba16(
        4,
        1,
        vec![
            [1, 2, 3, 4],
            [101, 102, 103, 104],
            [201, 202, 203, 204],
            [301, 302, 303, 304],
        ],
        IccProfile::srgb(),
        None,
    )
    .unwrap();
    let layer = PrecisionDocumentLayer::raster("placed", image, (-1, 1), 0.2, false).unwrap();
    let d = PrecisionDocument::from_parts(
        4,
        3,
        IccProfile::srgb(),
        Default::default(),
        Default::default(),
        vec![layer],
    )
    .unwrap();
    let expected = [0, 0, 0, 0, 104, 204, 304, 0, 0, 0, 0, 0];
    assert_eq!(select(&d, json!({"kind":"alpha"})), expected);
    assert_eq!(plane(&d, "alpha", true), expected);
    assert_eq!(plane(&d, "alpha", false), [4, 104, 204, 304]);
}
#[test]
fn nulls_invalid_lengths_parameters_budget_and_cancellation_never_publish() {
    unsafe {
        let mut e = ptr::null_mut();
        assert!(dr_selection_u16_plan(0, 0, ptr::null(), LIMIT, 0, &mut e).is_null());
        assert!(!e.is_null());
        drop(error(e));
        assert!(!dr_selection_u16(
            0,
            0,
            ptr::null(),
            ptr::null(),
            0,
            ptr::null_mut(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        ));
        assert!(!dr_precision_doc_select(
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            ptr::null_mut(),
            0,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        ));
        let input = [1, 32769, 65535];
        let mut out = [555; 3];
        let request = text(json!({"kind":"invert"}));
        let token = DrJobControl::default();
        token.cancel();
        assert!(!dr_selection_u16(
            3,
            1,
            request.as_ptr(),
            input.as_ptr(),
            3,
            out.as_mut_ptr(),
            3,
            LIMIT,
            0,
            &token,
            ptr::null_mut()
        ));
        assert_eq!(out, [555; 3]);
        assert!(!dr_selection_u16(
            3,
            1,
            request.as_ptr(),
            input.as_ptr(),
            2,
            out.as_mut_ptr(),
            3,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, [555; 3]);
        for req in [
            json!({"kind":"invert","unknown":true}),
            json!({"kind":"feather","radius":251}),
            json!({"kind":"transform","matrix":[0,0,0,0,0,0],"filter":"nearest"}),
        ] {
            let t = text(req);
            assert!(!dr_selection_u16(
                3,
                1,
                t.as_ptr(),
                input.as_ptr(),
                3,
                out.as_mut_ptr(),
                3,
                LIMIT,
                0,
                ptr::null(),
                ptr::null_mut()
            ));
            assert_eq!(out, [555; 3]);
        }
        assert!(!dr_selection_u16(
            3,
            1,
            request.as_ptr(),
            input.as_ptr(),
            3,
            out.as_mut_ptr(),
            3,
            1,
            0,
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, [555; 3]);
        let d = doc(3, 1, vec![[1, 2, 3, 4]; 3]);
        let req = text(
            json!({"op":"selection","source":"layer","layer":0,"operation":{"kind":"quick","params":{"seeds":[[3,0]]}}}),
        );
        assert!(!dr_precision_doc_select(
            &d,
            req.as_ptr(),
            ptr::null(),
            0,
            out.as_mut_ptr(),
            3,
            LIMIT,
            0,
            ptr::null(),
            ptr::null_mut()
        ));
        assert_eq!(out, [555; 3]);
    }
}

#[test]
fn scalar_bucket_matches_native_seed_codes_and_selection_independently() {
    let d = doc(
        5,
        1,
        vec![
            [30000, 1, 2, 0],
            [30001, 3, 4, 1],
            [30003, 5, 6, 7],
            [30000, 8, 9, 10],
            [30001, 11, 12, 13],
        ],
    );
    let selected = [65535, 32769, 65535, 65535, 0];
    for contiguous in [true, false] {
        let result=scalar(&d,"red",json!({"kind":"bucket_fill","x":0,"y":0,"tolerance":1,"contiguous":contiguous,"value":65535,"opacity":0.7}),true,&selected).unwrap();
        let original = d.layer_source(0).unwrap().to_rgba16().ok().unwrap();
        let actual = result.layer_source(0).unwrap().to_rgba16().ok().unwrap();
        for i in 0..5 {
            let mut expected = original[i];
            if (i < 2 || (!contiguous && i == 3)) && selected[i] > 0 {
                let t = f64::from(selected[i]) / 65535. * 0.7;
                expected[0] = q(f64::from(expected[0]) * (1. - t) + 65535. * t);
            }
            assert_eq!(actual[i], expected);
        }
    }
    assert!(scalar(
        &d,
        "red",
        json!({"kind":"bucket_fill","x":-1,"y":0,"tolerance":1,"value":1}),
        false,
        &[]
    )
    .is_none());
}

#[test]
fn active_native_feather_cancellation_after_gaussian_row_leaves_output_and_drains_scratch() {
    let written = std::sync::Mutex::new(Vec::new());
    let render = |token: usize| {
        let input: Vec<u16> = (0..512 * 512)
            .map(|i| ((i * 127 + 1) % 65536) as u16)
            .collect();
        let mut out = vec![54321; input.len()];
        let req = text(json!({"kind":"feather","radius":100}));
        let mut e = ptr::null_mut();
        let ok = unsafe {
            dr_selection_u16(
                512,
                512,
                req.as_ptr(),
                input.as_ptr(),
                input.len(),
                out.as_mut_ptr(),
                out.len(),
                LIMIT,
                0,
                token as *const DrJobControl,
                &mut e,
            )
        };
        let message = unsafe { error(e) };
        *written.lock().unwrap() = out;
        (ok, message)
    };
    // k = 1: the Gaussian polls twice between its row reports (the next report is at n0 + 3).
    let (entry, next) = entry_then_cancel(&|progress, _, _| progress > 0., render);
    let n0 = entry.checkpoints;
    assert!(
        next.expect("the entry's kernel never reported again")
            .checkpoints
            > n0 + 2,
        "the entry's kernel polls fewer than two checkpoints before its next report"
    );
    let parked = cancelled_parked_at(
        n0 + 2,
        &|progress, stage| stage == entry.stage && progress == entry.progress,
        render,
    );
    eprintln!("native feather cancellation: {}", parked_line(&parked));
    assert!(
        parked.progress > 0. && parked.progress < 1.,
        "must observe a completed Gaussian row before cancellation"
    );
    assert!(parked.scratch > 0);
    assert!(written.lock().unwrap().iter().all(|v| *v == 54321));
}

#[test]
fn authored_scalar_ink_retains_subbyte_values_and_refuses_invalid_without_output() {
    unsafe {
        let mut out = 999;
        let mut e = ptr::null_mut();
        assert!(dr_scalar_ink_u16(
            12345. / 65535.,
            12345. / 65535.,
            12345. / 65535.,
            &mut out,
            &mut e
        ));
        assert_eq!(out, 12345);
        assert!(dr_scalar_ink_u16(1., 0., 0., &mut out, &mut e));
        assert!(out.abs_diff(q(0.2126 * 65535.)) <= 1);
        out = 999;
        assert!(!dr_scalar_ink_u16(f64::NAN, 0., 0., &mut out, &mut e));
        assert_eq!(out, 999);
        drop(error(e));
        assert!(!dr_scalar_ink_u16(
            0.,
            0.,
            0.,
            ptr::null_mut(),
            ptr::null_mut()
        ));
    }
}

#[test]
fn colorizing_scalar_filters_extract_luminance_instead_of_one_rgb_component() {
    let d = doc(
        3,
        1,
        vec![
            [10001, 20002, 30003, 0],
            [10001, 20002, 30003, 1],
            [10001, 20002, 30003, 65535],
        ],
    );
    let constant = |v| json!({"r":0,"g":0,"b":0,"constant":v});
    let desc=json!({"type":"adjust","evaluation":"precision_v1","op":"channel_mixer","params":{"red":constant(0.6),"green":constant(0.2),"blue":constant(0.8)}}).to_string();
    let op = json!({"kind":"filter","edit":{"layer":0,"op":"adjustment","description":desc}});
    let result = scalar(&d, "red", op, true, &[65535, 32769, 0]).unwrap();
    let pixels = result.layer_source(0).unwrap().to_rgba16().ok().unwrap();
    // The constants as Photoshop stores them, whole 1/1024 steps toward
    // zero: 0.6 -> 614, 0.2 -> 204, 0.8 -> 819.
    let expected = (0.2126 * 614. + 0.7152 * 204. + 0.0722 * 819.) / 1024. * 65535.;
    assert!(pixels[0][0].abs_diff(q(expected)) <= 1);
    assert!(
        pixels[1][0].abs_diff(q(
            10001. * (1. - 32769. / 65535.) + expected * 32769. / 65535.
        )) <= 1
    );
    assert_eq!(pixels[2][0], 10001);
    for (i, p) in pixels.iter().enumerate() {
        assert_eq!(p[1..], [20002, 30003, [0, 1, 65535][i]]);
    }
}

#[test]
fn scalar_box_surface_and_grain_filter_the_plane_and_write_only_selected_samples() {
    let code = |i: usize| (4000 + (i * i * 911) % 50000) as u16;
    let original: Vec<[u16; 4]> = (0..15).map(|i| [code(i), 22002, 33003, 65535]).collect();
    let d = doc(5, 3, original.clone());
    let mut coverage = vec![0; 15];
    coverage[7] = 65535;
    // The plane is opaque gray; the centre's 3x3 window is samples 1-3, 6-8, 11-13.
    let window: Vec<f64> = [1, 2, 3, 6, 7, 8, 11, 12, 13]
        .iter()
        .map(|&i| f64::from(code(i)))
        .collect();
    let mean = window.iter().sum::<f64>() / 9.;
    let centre = f64::from(code(7));
    let weight = |v: f64| (1. - ((v - centre) / 257.).abs() / (2.5 * 60.)).max(0.);
    let tent = window.iter().map(|&v| weight(v) * v).sum::<f64>()
        / window.iter().map(|&v| weight(v)).sum::<f64>();
    for (op, params) in [
        ("box_blur", json!({"radius":1})),
        ("surface_blur", json!({"radius":1,"threshold":60})),
        ("add_grain", json!({"amount":100,"roughness":100,"seed":5})),
        (
            "radial_blur",
            json!({"amount":100,"method":"zoom","center_x":0}),
        ),
        ("clouds", json!({"seed":5})),
        ("difference_clouds", json!({"seed":5})),
        (
            "offset",
            json!({"horizontal":1,"undefined_areas":"fill","fill":0.25}),
        ),
    ] {
        let operation = json!({"kind":"filter","edit":{"layer":0,"op":op,"params":params}});
        let next = scalar(&d, "red", operation, true, &coverage).unwrap();
        let out = next.layer_source(0).unwrap().to_rgba16().ok().unwrap();
        for i in 0..15 {
            assert_eq!(out[i][1..], original[i][1..], "{op} other components");
            if coverage[i] == 0 {
                assert_eq!(out[i][0], original[i][0], "{op} unselected sample{i}");
            }
        }
        match op {
            "box_blur" => assert!(out[7][0].abs_diff(q(mean)) <= 1, "{op}"),
            "surface_blur" => assert!(out[7][0].abs_diff(q(tent)) <= 1, "{op}"),
            _ => assert_ne!(
                out[7][0], original[7][0],
                "{op} changed the selected sample"
            ),
        }
    }
}

mod bench {
    use super::*;

    #[test]
    #[ignore = "make cancel-bench: a wall-clock verdict"]
    fn active_native_feather_cancellation_after_gaussian_row_leaves_output_and_drains_scratch() {
        use std::sync::Arc;
        use std::time::{Duration, Instant};
        let control = Arc::new(DrJobControl::default());
        let worker_control = Arc::clone(&control);
        let worker = std::thread::spawn(move || {
            let input: Vec<u16> = (0..512 * 512)
                .map(|i| ((i * 127 + 1) % 65536) as u16)
                .collect();
            let mut out = vec![54321; input.len()];
            let req = text(json!({"kind":"feather","radius":100}));
            let mut e = ptr::null_mut();
            let ok = unsafe {
                dr_selection_u16(
                    512,
                    512,
                    req.as_ptr(),
                    input.as_ptr(),
                    input.len(),
                    out.as_mut_ptr(),
                    out.len(),
                    LIMIT,
                    0,
                    Arc::as_ptr(&worker_control),
                    &mut e,
                )
            };
            let message = unsafe { error(e) };
            (ok, message, out)
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while control.progress() == 0. && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(
            control.progress() > 0. && control.progress() < 1.,
            "must observe a completed Gaussian row before cancellation"
        );
        assert!(control.scratch_bytes() > 0);
        let start = Instant::now();
        control.cancel();
        let (ok, message, out) = worker.join().unwrap();
        assert!(!ok);
        assert!(message.contains("cancelled"), "{message}");
        assert!(out.iter().all(|v| *v == 54321));
        assert_eq!(control.scratch_bytes(), 0);
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
