//! Phase10D basic and seven added separable native modes: independent f64 source-over equations,
//! genuine adjacent u16 codes, actual JSON FFI edits and corrupt native framing.
use crate::group_roots;
use doctored_core::doc::BlendMode;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use group_roots::GroupRoots;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::common;
use common::probe::{cancelled_parked_at, entry_then_cancel, parked_line};

use crate::identity;

/// Every native encode below goes through here. Phase 12E-1 made the
/// always-written identity wrapper the OUTERMOST `.rz` word and its `LIDS`
/// table the file's last block; this file's subject is an EARLIER wrapper,
/// so the outer one is asserted and then peeled, leaving byte-for-byte what
/// the writer these tests were written against produced.
fn encoded(doc: &PrecisionDocument) -> Vec<u8> {
    identity::typed_prior_body(&doc.encode_native(LIMIT, 0, None).unwrap())
}

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
fn error(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { dr_string_free(p) };
    Some(text)
}
fn request(layer: usize, mode: i32) -> Value {
    json!({"op":"layer_blend","version":1,"layer":layer,"blend_mode":mode})
}
fn edit(
    doc: &PrecisionDocument,
    request: Value,
    limit: u64,
    control: Option<&DrJobControl>,
) -> Result<Option<PrecisionDocument>, String> {
    let request = CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let result = unsafe {
        dr_precision_doc_edit(
            doc,
            request.as_ptr(),
            ptr::null(),
            0,
            limit,
            0,
            control.map_or(ptr::null(), |c| c),
            &mut err,
        )
    };
    if let Some(message) = error(err) {
        assert!(result.is_null());
        return Err(message);
    }
    Ok((!result.is_null()).then(|| *unsafe { Box::from_raw(result) }))
}
fn changed(doc: &PrecisionDocument, layer: usize, mode: i32) -> PrecisionDocument {
    edit(doc, request(layer, mode), LIMIT, None)
        .unwrap()
        .unwrap()
}
fn layer(name: &str, pixels: Vec<[u16; 4]>, offset: i32, opacity: f32) -> Layer {
    Layer::raster(
        name,
        PrecisionImage::from_rgba16(pixels.len() as u32, 1, pixels, IccProfile::srgb(), None)
            .unwrap(),
        (offset, 0),
        opacity,
        true,
    )
    .unwrap()
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
fn plane(samples: Vec<u16>) -> PrecisionPlane {
    PrecisionPlane::from_u16(samples.len() as u32, 1, samples, None).unwrap()
}
fn mask(samples: Vec<u16>, offset: i32, density: f32) -> PrecisionMask {
    let mut props = MaskProperties::at((offset, 0));
    props.density = density;
    PrecisionMask::new(plane(samples), props, true).unwrap()
}
fn normalized(p: [u16; 4]) -> [f64; 4] {
    p.map(|v| v as f64 / 65535.)
}
fn over(base: [f64; 4], mut src: [f64; 4], weight: f64, mode: i32) -> [f64; 4] {
    src[3] *= weight;
    let a = src[3] + base[3] * (1. - src[3]);
    if a == 0. {
        return [0.; 4];
    }
    let mut out = [0.; 4];
    out[3] = a;
    for c in 0..3 {
        let b = match mode {
            0 => src[c],
            1 => base[c] * src[c],
            2 => 1. - (1. - base[c]) * (1. - src[c]),
            3 => {
                if base[c] <= 0.5 {
                    2. * base[c] * src[c]
                } else {
                    1. - 2. * (1. - base[c]) * (1. - src[c])
                }
            }
            4 => {
                if src[c] <= 0.5 {
                    base[c] - (1. - 2. * src[c]) * base[c] * (1. - base[c])
                } else {
                    let d = if base[c] <= 0.25 {
                        ((16. * base[c] - 12.) * base[c] + 4.) * base[c]
                    } else {
                        base[c].sqrt()
                    };
                    base[c] + (2. * src[c] - 1.) * (d - base[c])
                }
            }
            5 => {
                if src[c] <= 0.5 {
                    2. * base[c] * src[c]
                } else {
                    1. - 2. * (1. - base[c]) * (1. - src[c])
                }
            }
            6 => base[c].min(src[c]),
            7 => base[c].max(src[c]),
            8 => (base[c] - src[c]).abs(),
            9 => base[c] + src[c] - 2. * base[c] * src[c],
            _ => panic!("oracle mode"),
        };
        out[c] = ((1. - src[3]) * base[3] * base[c]
            + src[3] * ((1. - base[3]) * src[c] + base[3] * b))
            / a;
    }
    out
}
fn close(actual: [u16; 4], expected: [f64; 4]) {
    for c in 0..4 {
        assert!(
            (actual[c] as f64 - expected[c] * 65535.).abs() <= 0.65,
            "channel{c}: {actual:?}, {expected:?}"
        );
    }
}

#[test]
fn all_native_coverage_codes_partial_alpha_and_masks_match_independent_f64() {
    let base: Vec<_> = (0..=u16::MAX)
        .map(|i| {
            [
                i,
                i.wrapping_add(1),
                65535 - i,
                [0, 1, 2, 32769, 65535][i as usize % 5],
            ]
        })
        .collect();
    let src: Vec<_> = (0..=u16::MAX)
        .map(|i| {
            [
                65535 - i,
                i.wrapping_add(2),
                i.wrapping_add(3),
                [1, 32768, 65535, 0][i as usize % 4],
            ]
        })
        .collect();
    let coverage: Vec<_> = (0..=u16::MAX).collect();
    let doc = document(
        65536,
        vec![
            layer("Base", base.clone(), 0, 1.),
            layer("Ink", src.clone(), 0, 0.625).with_mask(Some(mask(coverage, 0, 0.75))),
        ],
    );
    for mode in 1..=9 {
        let edited = changed(&doc, 1, mode);
        assert!(PrecisionImage::ptr_eq(
            edited.layer_source(1).unwrap(),
            doc.layer_source(1).unwrap()
        ));
        for (i, p) in render(&edited).into_iter().enumerate() {
            close(
                p,
                over(
                    normalized(base[i]),
                    normalized(src[i]),
                    // The layer's 0.625 at its byte (#578).
                    common::byte_unit(0.625) * (0.25 + 0.75 * i as f64 / 65535.),
                    mode,
                ),
            );
        }
        assert_eq!(doc.layer(1).unwrap().blend_mode(), BlendMode::Normal);
    }
}

#[test]
fn nested_isolation_fractional_masks_offsets_and_pass_through_are_distinct() {
    let b = [12345, 34567, 56789, 32123];
    let a = [51234, 22223, 34567, 43210];
    let z = [22444, 44555, 11666, 54321];
    let inner = Layer::isolated_normal(
        "Inner",
        vec![
            layer("A", vec![a; 3], -1, 0.75),
            layer("Z", vec![z; 2], 1, 0.5)
                .with_blend_mode(BlendMode::Screen)
                .unwrap(),
        ],
        0.625,
        true,
    )
    .unwrap()
    .with_mask(Some(mask(vec![1, 32769, 65535], 0, 1.)));
    let outer = Layer::pass_through("Outer", vec![inner], 0.875, true).unwrap();
    let doc = document(3, vec![layer("B", vec![b; 3], 0, 1.), outer]);
    // Every opacity at its byte (#578).
    let q = common::byte_unit;
    let t = q(0.875);
    for mode in 1..=9 {
        let result = render(&changed(&doc, 3, mode));
        for x in 0..3 {
            let base = normalized(b);
            let first = if x < 2 {
                over([0.; 4], normalized(a), q(0.75), 0)
            } else {
                [0.; 4]
            };
            let group = if x >= 1 {
                over(first, normalized(z), q(0.5), 2)
            } else {
                first
            };
            let after = over(
                base,
                group,
                q(0.625) * [1., 32769., 65535.][x] / 65535.,
                mode,
            );
            let alpha = base[3] * (1. - t) + after[3] * t;
            let mut expected = [0.; 4];
            expected[3] = alpha;
            for c in 0..3 {
                expected[c] = (base[c] * base[3] * (1. - t) + after[c] * after[3] * t) / alpha;
            }
            close(result[x], expected);
        }
    }
    let multiply = changed(&doc, 3, 1);
    let result = render(&multiply);
    let passing = changed(&multiply, 3, 27);
    assert!(passing.layer(3).unwrap().is_pass_through());
    assert_ne!(render(&passing), result);
    let restored = changed(&passing, 3, 1);
    assert_eq!(render(&restored), result);
    let renamed = edit(
        &restored,
        json!({"op":"rename_layer","layer":1,"name":"Nested"}),
        LIMIT,
        None,
    )
    .unwrap()
    .unwrap();
    let opacity = renamed.set_layer_opacity(1, 0.25).unwrap().unwrap();
    let duplicate = opacity.duplicate_layer(2).unwrap().unwrap();
    assert_eq!(
        duplicate.layer(4).unwrap().blend_mode(),
        BlendMode::Multiply
    );
    assert_eq!(
        duplicate.layer(5).unwrap().blend_mode(),
        BlendMode::PassThrough
    );
    assert_eq!(duplicate.layer(2).unwrap().blend_mode(), BlendMode::Screen);
    assert_eq!(duplicate.layer(3).unwrap().blend_mode(), BlendMode::Screen);
}

#[test]
fn strict_ffi_plans_noops_refusals_and_admission_leave_input_exact() {
    let adjustment = Layer::adjustment(
        "Invert",
        r#"{"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}"#,
        1.,
        true,
        &IccProfile::srgb(),
    )
    .unwrap();
    let doc = document(
        1,
        vec![
            layer("Fine", vec![[12345, 23456, 34567, 1]], 0, 1.),
            adjustment,
        ],
    );
    let before = encoded(&doc);
    assert!(edit(&doc, request(0, 0), LIMIT, None).unwrap().is_none());
    for bad in [
        request(0, 27),
        request(5, 1),
        request(0, -1),
        json!({"op":"layer_blend","layer":0,"blend_mode":1}),
        json!({"op":"layer_blend","version":2,"layer":0,"blend_mode":1}),
        json!({"op":"layer_blend","version":1,"layer":0,"blend_mode":1,"unknown":true}),
    ] {
        assert!(edit(&doc, bad.clone(), LIMIT, None).is_err(), "{bad}");
        let json = CString::new(bad.to_string()).unwrap();
        let mut err = ptr::null_mut();
        let out = unsafe { dr_precision_doc_edit_plan(&doc, json.as_ptr(), LIMIT, 0, &mut err) };
        assert!(out.is_null());
        assert!(error(err).is_some());
    }
    // An adjustment carries the same twenty-six modes as a raster (v37 /
    // LBLD schema7); only Pass Through (27) is still group-only.
    for mode in 1..=26 {
        assert!(edit(&doc, request(1, mode), LIMIT, None).unwrap().is_some());
    }
    assert!(edit(&doc, request(1, 27), LIMIT, None).is_err());
    for mode in 1..=9 {
        let changed = changed(&doc, 0, mode);
        assert!(edit(&changed, request(0, mode), LIMIT, None)
            .unwrap()
            .is_none());
        assert_eq!(
            changed.layer_source(0).unwrap().to_rgba16().ok(),
            doc.layer_source(0).unwrap().to_rgba16().ok()
        );
    }
    assert!(edit(&doc, request(0, 1), 1, None).is_err());
    let control = DrJobControl::default();
    control.cancel();
    assert!(edit(&doc, request(0, 1), LIMIT, Some(&control)).is_err());
    assert_eq!(encoded(&doc), before);
}

#[test]
fn conditional_v26_wraps_channels_and_preserves_every_raw_code() {
    let raw = vec![
        [1, 2, 3, 0],
        [30001, 30002, 30003, 1],
        [65535, 54321, 12345, 65535],
    ];
    let doc = document(
        2,
        vec![
            layer("Hidden off canvas", raw.clone(), -1, 0.75).with_mask(Some(mask(
                vec![1, 32769, 65535, 0],
                -2,
                0.5,
            ))),
        ],
    );
    assert_eq!(
        u32::from_le_bytes(encoded(&doc)[4..8].try_into().unwrap()),
        13
    );
    let doc = PrecisionDocument::from_parts_and_channels(
        2,
        1,
        IccProfile::srgb(),
        Metadata::default(),
        Resolution::default(),
        vec![doc.layer(0).unwrap().clone()],
        vec![PrecisionChannel::new("Matte", plane(vec![1, 32769])).unwrap()],
    )
    .unwrap();
    let doc=edit(&doc,json!({"op":"channel_properties","index":0,"overlay_color":[17,83,149],"overlay_opacity":0.375}),LIMIT,None).unwrap().unwrap();
    let old = encoded(&doc);
    assert_eq!(u32::from_le_bytes(old[4..8].try_into().unwrap()), 25);
    let doc = changed(&doc, 0, 2);
    let bytes = encoded(&doc);
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 26);
    assert_eq!(&bytes[8..12], &25u32.to_le_bytes());
    assert!(bytes.len() as u64 <= doc.native_plan().unwrap().encoded_bound);
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
        raw
    );
    assert_eq!(reopened.layer(0).unwrap().offset(), (-1, 0));
    assert_eq!(
        reopened.layer(0).unwrap().mask().unwrap().plane().samples(),
        [1, 32769, 65535, 0]
    );
    assert_eq!(
        reopened.channels()[0].metadata().overlay_color,
        [17, 83, 149]
    );
    assert_eq!(reopened.channels()[0].plane().samples(), [1, 32769]);
    assert_eq!(render(&reopened), render(&doc));
    assert_eq!(encoded(&reopened), bytes);
    for mode in (3..=9).chain([10, 11, 12, 15]) {
        let added = changed(&reopened, 0, mode);
        let saved = encoded(&added);
        assert_eq!(
            &saved[4..12],
            &[
                (if mode <= 9 { 27u32 } else { 30u32 }).to_le_bytes(),
                25u32.to_le_bytes()
            ]
            .concat()
        );
        let again = PrecisionDocument::parse_native(&saved, LIMIT, 0, None).unwrap();
        assert_eq!(
            again.layer_source(0).unwrap().to_rgba16().ok().unwrap(),
            raw
        );
        assert_eq!(
            again.layer(0).unwrap().mask().unwrap().plane().samples(),
            [1, 32769, 65535, 0]
        );
        assert_eq!(
            again.channels()[0].metadata(),
            reopened.channels()[0].metadata()
        );
        assert_eq!(again.channels()[0].plane().samples(), [1, 32769]);
        assert_eq!(encoded(&again), saved);
        assert_eq!(encoded(&changed(&again, 0, 2)), bytes);
    }
    assert_eq!(encoded(&changed(&reopened, 0, 0)), old);
}

#[test]
fn unknown_blending_preflights_before_corrupt_png_and_hidden_record_decode() {
    let doc = document(
        1,
        vec![layer("Hidden", vec![[12345, 23456, 34567, 65535]], 0, 1.)],
    );
    let doc = changed(&doc, 0, 1);
    let hidden = edit(
        &doc,
        json!({"op":"layer_visible","layer":0,"visible":false}),
        LIMIT,
        None,
    )
    .unwrap()
    .unwrap();
    let bytes = encoded(&hidden);
    let tag = bytes.windows(4).rposition(|x| x == b"LBLD").unwrap();
    let idat = bytes.windows(4).position(|x| x == b"IDAT").unwrap();
    for (offset, value, word) in [
        (tag + 4, 2, "schema"),
        (tag + 8, 0, "count"),
        (tag + 12, 1, "index"),
        (tag + 16, 14, "mode"),
    ] {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(value));
        bad[idat + 4] ^= 255;
        let error = PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
            .err()
            .unwrap();
        assert!(error.contains(word), "{error}");
    }
    let mut bad = bytes.clone();
    bad.extend_from_slice(&[0]);
    assert!(PrecisionDocument::parse_native(&bad, LIMIT, 0, None).is_err());
    // A v26 table cannot assign Multiply to a pass-through group.
    let group = document(
        1,
        vec![Layer::pass_through(
            "Group",
            vec![layer("Leaf", vec![[1, 2, 3, 4]], 0, 1.)],
            1.,
            true,
        )
        .unwrap()],
    );
    let group = changed(&group, 0, 1);
    let mut bad = encoded(&group);
    let tag = bad.windows(4).rposition(|x| x == b"LBLD").unwrap();
    bad[tag + 12..tag + 16].copy_from_slice(&1u32.to_le_bytes());
    assert!(PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
        .err()
        .unwrap()
        .contains("isolated group"));
}

#[test]
fn mixed_retained_master_and_artwork_descriptions_survive_blends_and_ancestor_edits() {
    use doctored_core::retained_source::{SourceEdit, SourceInput};
    let doc = document(
        2,
        vec![layer(
            "Artwork",
            vec![[12345, 23456, 34567, 1], [54321, 32123, 11111, 65535]],
            0,
            1.,
        )],
    );
    let source = document(
        3,
        vec![layer("Original", vec![[30001, 30002, 30003, 0]; 3], 0, 1.)],
    );
    let place=SourceEdit::parse(&json!({"op":"place","source_id":"blend-master","revision_id":"v1","name":"Retained","x":-1,"y":0}).to_string()).unwrap();
    let placed = doc
        .source_edit_input(Some(SourceInput::Typed(&source)), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap();
    let description =
        CString::new(r#"{"type":"shape","version":3,"probe":"native blend preservation"}"#)
            .unwrap();
    let name = CString::new("Artwork").unwrap();
    let mut err = ptr::null_mut();
    let ptr = unsafe {
        doctored_core::ffi_artwork::dr_precision_doc_artwork(
            &placed,
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
    assert!(error(err).is_none());
    assert!(!ptr.is_null());
    let artwork = *unsafe { Box::from_raw(ptr) };
    let first = changed(&artwork, 0, 2);
    let blended = changed(&first, 1, 1);
    let grouped = blended.group_all_pass_through(1.).unwrap().unwrap();
    let grouped = changed(&grouped, 2, 1);
    let renamed = edit(
        &grouped,
        json!({"op":"rename_layer","layer":1,"name":"Renamed"}),
        LIMIT,
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(renamed.layer(2).unwrap().blend_mode(), BlendMode::Multiply);
    assert_eq!(
        renamed.source_info(1).unwrap(),
        placed.source_info(1).unwrap()
    );
    assert_eq!(
        renamed.artwork_description(0).unwrap(),
        description.to_str().unwrap()
    );
    let bytes = encoded(&renamed);
    assert_eq!(
        &bytes[4..12],
        &[26u32.to_le_bytes(), 20u32.to_le_bytes()].concat()
    );
    let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
    assert_eq!(
        reopened.source_info(1).unwrap(),
        placed.source_info(1).unwrap()
    );
    assert_eq!(
        reopened.artwork_description(0),
        renamed.artwork_description(0)
    );
    assert_eq!(render(&reopened), render(&renamed));
    for mode in (3..=9).chain([10, 11, 12, 15]) {
        let added = changed(&renamed, 2, mode);
        let saved = encoded(&added);
        assert_eq!(
            &saved[4..12],
            &[
                (if mode <= 9 { 27u32 } else { 30u32 }).to_le_bytes(),
                20u32.to_le_bytes()
            ]
            .concat()
        );
        let again = PrecisionDocument::parse_native(&saved, LIMIT, 0, None).unwrap();
        assert_eq!(again.source_info(1), placed.source_info(1));
        assert_eq!(again.artwork_description(0), renamed.artwork_description(0));
        assert_eq!(encoded(&again), saved);
        assert_eq!(encoded(&changed(&again, 2, 1)), bytes);
        for i in 0..2 {
            assert_eq!(
                again.layer_source(i).unwrap().to_rgba16().ok(),
                renamed.layer_source(i).unwrap().to_rgba16().ok()
            );
        }
    }
    for i in 0..2 {
        assert_eq!(
            reopened.layer_source(i).unwrap().to_rgba16().ok(),
            renamed.layer_source(i).unwrap().to_rgba16().ok()
        );
    }
}

#[test]
fn blended_render_cancellation_releases_scratch_without_changing_sources() {
    // MID-WORK: the default test parks the render once, on rung 0 (a
    // 65 536-wide canvas), past a tenth of the work, and cancels it there.
    let width = 65536usize;
    let fixture = || {
        document(
            width as u32,
            vec![
                layer("Base", vec![[12345, 23456, 34567, 32769]; width], 0, 1.),
                layer(
                    "Soft Light",
                    vec![[33333, 44444, 55555, 44444]; width],
                    0,
                    1.,
                )
                .with_blend_mode(BlendMode::SoftLight)
                .unwrap(),
            ],
        )
    };
    let render = |doc: &PrecisionDocument, token: usize| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match doc.render(LIMIT, 0, Some(control)) {
            Ok(_) => (true, String::new()),
            Err(error) => (false, error),
        }
    };
    let twin = fixture();
    let (entry, _) = entry_then_cancel(&|progress, _, _| progress >= 0.1, |token| {
        render(&twin, token)
    });
    // k = 0: the blended render checks only at its reports (the next report's
    // check is n0 + 1), so the park is that report's own check.
    let n0 = entry.checkpoints;
    let doc = fixture();
    let parked = cancelled_parked_at(n0 + 1, &|_, stage| stage == entry.stage, |token| {
        render(&doc, token)
    });
    println!("blended render cancellation: {}", parked_line(&parked));
    assert!(parked.progress >= 0.1);
    assert_eq!(
        doc.layer_source(1).unwrap().to_rgba16().ok().unwrap()[40000],
        [33333, 44444, 55555, 44444]
    );
    assert!(doc.render(1, 0, None).is_err());
}

// Oracle follows W3C Compositing and Blending Level 1 sections10.1.4–12:
// https://www.w3.org/TR/2024/CRD-compositing-1-20240321/#blendingseparable
// Test data crosses both Soft Light branches and reverses asymmetric colors;
// native .25 and .5 have neighboring codes, not exact integer encodings.
#[test]
fn separable_branches_endpoints_asymmetric_colors_and_full_alpha() {
    let codes = [
        0, 1, 16383, 16384, 16385, 32767, 32768, 32769, 49151, 65534, 65535,
    ];
    let mut base = Vec::new();
    let mut source = Vec::new();
    for &b in &codes {
        for &s in &codes {
            base.push([b, s, 65535 - b, 65535]);
            source.push([s, b, 65535 - s, 65535]);
        }
    }
    let doc = document(
        base.len() as u32,
        vec![
            layer("Backdrop", base.clone(), 0, 1.),
            layer("Source", source.clone(), 0, 1.),
        ],
    );
    for mode in 3..=9 {
        for (i, actual) in render(&changed(&doc, 1, mode)).into_iter().enumerate() {
            close(
                actual,
                over(normalized(base[i]), normalized(source[i]), 1., mode),
            );
        }
    }
    assert_ne!(
        render(&changed(&doc, 1, 3)),
        render(&changed(&doc, 1, 5)),
        "Overlay and Hard Light use opposite branches"
    );
}

#[test]
fn separable_nested_groups_keep_subcode_alpha_until_final_projection() {
    let b = [50000, 30000, 5000, 1];
    let s = [10000, 15000, 60000, 1];
    for mode in 3..=9 {
        let m = BlendMode::from_c(mode).unwrap();
        let group = Layer::isolated_normal(
            "Inner",
            vec![layer("Translated", vec![s; 2], -1, 0.25)],
            0.625,
            true,
        )
        .unwrap()
        .with_blend_mode(m)
        .unwrap()
        .with_mask(Some(mask(vec![32769], 0, 1.)));
        let outer = Layer::pass_through("Outer", vec![group], 0.875, true).unwrap();
        let doc = document(1, vec![layer("B", vec![b], 0, 1.), outer]);
        // Every opacity at its byte (#578).
        let (q, t) = (common::byte_unit, common::byte_unit(0.875));
        let inner = over([0.; 4], normalized(s), q(0.25), 0);
        assert!(inner[3] * 65535. < 0.5);
        let expected_after = over(normalized(b), inner, q(0.625) * 32769. / 65535., mode);
        let alpha = normalized(b)[3] * (1. - t) + expected_after[3] * t;
        let mut expected = [0.; 4];
        expected[3] = alpha;
        for c in 0..3 {
            expected[c] = (normalized(b)[c] * normalized(b)[3] * (1. - t)
                + expected_after[c] * expected_after[3] * t)
                / alpha;
        }
        let actual = render(&doc)[0];
        close(actual, expected);
        assert!(
            (actual[2] as i32 - b[2] as i32).abs() > 1000,
            "An intermediate u16 alpha round erases the group and fails this witness"
        );
        let duplicate = doc.duplicate_layer(1).unwrap().unwrap();
        assert_eq!(duplicate.layer(3).unwrap().blend_mode(), m);
        let passing = changed(&doc, 2, 27);
        let restored = changed(&passing, 2, mode);
        assert_eq!(render(&restored), render(&doc));
    }
}

#[test]
fn conditional_v27_schema2_preserves_v26_schema1_and_prior_body_bytes() {
    let base = document(
        1,
        vec![
            layer("A", vec![[30001, 30002, 30003, 0]], -1, 0.5),
            layer("B", vec![[12345, 54321, 23456, 1]], 0, 1.),
        ],
    );
    let base = PrecisionDocument::from_parts(
        1,
        1,
        base.profile().clone(),
        Metadata {
            exif: Some(vec![1, 2, 3].into()),
            xmp: Some(b"<xmp>exact</xmp>".to_vec().into()),
            iptc: Some(vec![4, 5, 6].into()),
        },
        Resolution {
            x: 301.25,
            y: 299.75,
        },
        vec![
            base.layer(0).unwrap().clone(),
            base.layer(1).unwrap().clone(),
        ],
    )
    .unwrap();
    let old = changed(&base, 0, 1);
    let v26 = encoded(&old);
    // Independent old writer framing: untouched default body plus schema1.
    let original_body = encoded(&base);
    let mut expected_v26 = b"DRDC".to_vec();
    expected_v26.extend_from_slice(&26u32.to_le_bytes());
    expected_v26.extend_from_slice(&original_body[4..]);
    expected_v26.extend_from_slice(b"LBLD");
    for value in [1u32, 1, 0, 1] {
        expected_v26.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(v26, expected_v26);
    assert_eq!(&v26[4..8], &26u32.to_le_bytes());
    for mode in 3..=9 {
        let doc = changed(&old, 1, mode);
        let bytes = encoded(&doc);
        assert_eq!(&bytes[4..8], &27u32.to_le_bytes());
        let tag = bytes.windows(4).rposition(|x| x == b"LBLD").unwrap();
        assert_eq!(
            &bytes[tag + 4..tag + 12],
            &[2u32.to_le_bytes(), 2u32.to_le_bytes()].concat()
        );
        // Same older body and same capacity accounting, no added pixel plane.
        let old_tag = v26.windows(4).rposition(|x| x == b"LBLD").unwrap();
        assert_eq!(&bytes[8..tag], &v26[8..old_tag]);
        assert_eq!(bytes.len(), v26.len() + 8);
        assert_eq!(
            doc.native_plan().unwrap().encoded_bound,
            old.native_plan().unwrap().encoded_bound + 8
        );
        let reopened = PrecisionDocument::parse_native(&bytes, LIMIT, 0, None).unwrap();
        assert_eq!(encoded(&reopened), bytes);
        assert_eq!(reopened.metadata(), base.metadata());
        assert_eq!(reopened.profile().bytes(), base.profile().bytes());
        assert_eq!(reopened.resolution(), base.resolution());
        assert_eq!(encoded(&changed(&reopened, 1, 0)), v26);
        assert_eq!(
            encoded(&changed(&changed(&reopened, 1, 0), 0, 0)),
            encoded(&base)
        );
        for i in 0..2 {
            assert_eq!(
                reopened.layer_source(i).unwrap().to_rgba16().ok(),
                base.layer_source(i).unwrap().to_rgba16().ok()
            );
        }
    }
}

#[test]
fn separable_v27_hidden_preflight_rejects_malformed_tables_and_legacy_reinterpretation() {
    let doc = document(
        1,
        vec![
            layer("A", vec![[30001, 30002, 30003, 0]], 0, 1.),
            layer("B", vec![[1, 2, 3, 4]], 0, 1.),
        ],
    );
    let doc = changed(&changed(&doc, 0, 3), 1, 9);
    let doc = edit(
        &doc,
        json!({"op":"layer_visible","layer":1,"visible":false}),
        LIMIT,
        None,
    )
    .unwrap()
    .unwrap();
    let bytes = encoded(&doc);
    let tag = bytes.windows(4).rposition(|x| x == b"LBLD").unwrap();
    let idat = bytes.windows(4).position(|x| x == b"IDAT").unwrap();
    for (offset, value, word) in [
        (tag + 4, 1, "schema"),
        (tag + 4, 3, "schema"),
        (tag + 8, 0, "count"),
        (tag + 8, 3, "count"),
        (tag + 20, 0, "order"),
        (tag + 20, 2, "index"),
        (tag + 24, 10, "mode"),
        (tag + 24, 14, "mode"),
        (tag + 24, 23, "mode"),
        (tag + 24, 0, "mode"),
        (tag + 24, 27, "mode"),
        (8, 26, "body version"),
    ] {
        let mut bad = bytes.clone();
        bad[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(value));
        bad[idat + 4] ^= 255;
        let err = PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
            .err()
            .unwrap();
        assert!(err.contains(word), "{offset}:{value}: {err}");
    }
    let mut old = bytes.clone();
    old[4..8].copy_from_slice(&26u32.to_le_bytes());
    old[tag + 4..tag + 8].copy_from_slice(&1u32.to_le_bytes());
    old[idat + 4] ^= 255;
    assert!(PrecisionDocument::parse_native(&old, LIMIT, 0, None)
        .err()
        .unwrap()
        .contains("mode"));
    for end in tag..bytes.len() {
        assert!(PrecisionDocument::parse_native(&bytes[..end], LIMIT, 0, None).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(PrecisionDocument::parse_native(&trailing, LIMIT, 0, None).is_err());
    assert!(PrecisionDocument::parse_native(&bytes, 1, 0, None).is_err());
    let cancel = DrJobControl::default();
    cancel.cancel();
    assert!(PrecisionDocument::parse_native(&bytes, LIMIT, 0, Some(&cancel)).is_err());
    assert_eq!(encoded(&doc), bytes);
}

#[test]
fn separable_hidden_adjustment_and_pass_through_targets_refuse_before_pixels() {
    let raster = layer("Raster", vec![[1, 2, 3, 4]], 0, 1.);
    let adjustment = Layer::adjustment(
        "Hidden adjustment",
        r#"{"type":"adjust","op":"invert","params":{},"evaluation":"precision_v1"}"#,
        1.,
        false,
        &IccProfile::srgb(),
    )
    .unwrap();
    let group = Layer::pass_through("Hidden passing", vec![], 1., false).unwrap();
    let doc = changed(&document(1, vec![raster, adjustment, group]), 0, 4);
    let bytes = encoded(&doc);
    let tag = bytes.windows(4).rposition(|x| x == b"LBLD").unwrap();
    let idat = bytes.windows(4).position(|x| x == b"IDAT").unwrap();
    for index in [1u32, 2] {
        let mut bad = bytes.clone();
        bad[tag + 12..tag + 16].copy_from_slice(&index.to_le_bytes());
        bad[idat + 4] ^= 255;
        assert!(PrecisionDocument::parse_native(&bad, LIMIT, 0, None)
            .err()
            .unwrap()
            .contains("raster or isolated group"));
    }
}
