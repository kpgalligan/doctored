//! Phase10D typed clipping: clip units in the f32 compositor against independent
//! f64 clip-group equations, exact bit-identity for documents the flag does not
//! change, the `layer_clipped` edit, conditional native32/`LCLP` persistence,
//! preservation through edits, PSD16 round trips and contribution-only cuts.
use doctored_core::doc::BlendMode;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_precision::dr_precision_doc_info;
use doctored_core::ffi_precision_edit::{dr_precision_doc_edit, dr_precision_doc_edit_plan};
use doctored_core::ffi_psd::{
    dr_precision_doc_open_psd, dr_precision_doc_save_psd, dr_psd_import_report,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::precision_render::MAX_GROUP_DEPTH;
use doctored_core::scoped_statistics::AnalysisRequest;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;
use tempfile::TempDir;

use crate::common;
use crate::identity;
use crate::psd16_fixture as fixture;
use common::{cpath, take_err_string};
use identity::{drdc_word, prior_body, IDENTITY_TYPED};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

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
fn clip(doc: &PrecisionDocument, layer: usize, clipped: bool) -> PrecisionDocument {
    changed(
        doc,
        json!({"op":"layer_clipped","layer":layer,"clipped":clipped}),
    )
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
/// Independent straight source-over with the W3C separable formula; mode
/// 0 Normal, 1 Multiply, 2 Screen. `weight` is opacity × coverage.
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
            _ => panic!("oracle mode"),
        };
        out[c] = ((1. - src[3]) * base[3] * base[c]
            + src[3] * ((1. - base[3]) * src[c] + base[3] * b))
            / a;
    }
    out
}
/// The clip rule in f64 (#2): the base at full opacity in Normal
/// into a transparent buffer, its alpha (the footprint) recorded, each member
/// composited onto the base with that alpha FROZEN — the W3C step against the
/// base colour seen at alpha 1, `lerp(Cb, B(Cb, Cs), sa)` — then the buffer
/// once at the footprint with the base's mode and opacity.
fn clip_unit(
    backdrop: [f64; 4],
    base: ([f64; 4], f64),
    (opacity, mode): (f64, i32),
    members: &[([f64; 4], f64, i32)],
) -> [f64; 4] {
    let mut buf = over([0.; 4], base.0, base.1, 0);
    let footprint = buf[3];
    for &(pixel, weight, mode) in members {
        buf[3] = 1.;
        buf = over(buf, pixel, weight, mode);
    }
    buf[3] = footprint;
    if footprint == 0. {
        return backdrop;
    }
    over(backdrop, buf, opacity, mode)
}
fn close(actual: [u16; 4], expected: [f64; 4]) {
    for c in 0..4 {
        assert!(
            (actual[c] as f64 - expected[c] * 65535.).abs() <= 0.65,
            "channel{c}: {actual:?} vs {expected:?}"
        );
    }
}
fn coverage(density: f64, sample: f64) -> f64 {
    1. - density + density * sample / 65535.
}

// ------------------------------------------------------------ (a) kernel --

#[test]
fn a_raster_base_with_two_members_matches_the_independent_clip_group_equation() {
    let backdrop = [30001, 20002, 10003, 65535];
    let base_px = [
        [50000, 10000, 20000, 0],
        [50001, 10001, 20001, 3],
        [50002, 10002, 20002, 32768],
        [50003, 10003, 20003, 65535],
    ];
    let m1_px = [
        [1000, 60000, 30000, 65535],
        [1001, 60001, 30001, 32768],
        [1002, 60002, 30002, 1],
        [1003, 60003, 30003, 65535],
    ];
    let m2_px = [
        [40000, 40001, 5000, 65535],
        [40002, 40003, 5001, 65535],
        [40004, 40005, 5002, 40000],
        [40006, 40007, 5003, 65535],
    ];
    let base_mask = [65535u16, 32769, 40000];
    let m2_mask = [0u16, 20000, 65535, 65535];
    let doc = document(
        4,
        vec![
            layer("B", vec![backdrop; 4], 0, 1.),
            layer("Base", base_px.to_vec(), 0, 0.625)
                .with_blend_mode(BlendMode::Screen)
                .unwrap()
                .with_mask(Some(mask(base_mask.to_vec(), 1, 0.75))),
            layer("M1", m1_px.to_vec(), 0, 1.).with_clipped(true),
            layer("M2", m2_px.to_vec(), 0, 0.5)
                .with_blend_mode(BlendMode::Multiply)
                .unwrap()
                .with_mask(Some(mask(m2_mask.to_vec(), 0, 1.)))
                .with_clipped(true),
        ],
    );
    let rendered = render(&doc);
    let alone = {
        let mut layers: Vec<Layer> = (0..2).map(|i| doc.layer(i).unwrap().clone()).collect();
        layers[1] = layers[1].clone();
        render(&document(4, layers))
    };
    for x in 0..4 {
        let base_cov = if x == 0 {
            coverage(0.75, 0.)
        } else {
            coverage(0.75, base_mask[x - 1] as f64)
        };
        let expected = clip_unit(
            normalized(backdrop),
            (normalized(base_px[x]), base_cov),
            // Every opacity at its byte (#2).
            (crate::common::byte_unit(0.625), 2),
            &[
                (normalized(m1_px[x]), 1., 0),
                (
                    normalized(m2_px[x]),
                    crate::common::byte_unit(0.5) * coverage(1., m2_mask[x] as f64),
                    1,
                ),
            ],
        );
        close(rendered[x], expected);
        assert_eq!(
            rendered[x][3], alone[x][3],
            "the base's gated alpha exactly"
        );
    }
}

// ------------------------------------------------------- (b) identity --

#[test]
fn baseless_hidden_member_and_invisible_base_are_bit_identical_to_plain_documents() {
    let a = [12345, 34567, 56789, 32123];
    let b = [51234, 22223, 34567, 43210];
    let plain = document(
        2,
        vec![
            layer("A", vec![a; 2], 0, 0.75),
            layer("B", vec![b; 1], 1, 1.),
        ],
    );
    let baseless = document(
        2,
        vec![
            layer("A", vec![a; 2], 0, 0.75).with_clipped(true),
            layer("B", vec![b; 1], 1, 1.),
        ],
    );
    assert_eq!(render(&baseless), render(&plain));
    // The same at the bottom of a nested level.
    let plain_group = document(
        2,
        vec![Layer::pass_through(
            "G",
            vec![
                layer("A", vec![a; 2], 0, 0.75),
                layer("B", vec![b; 1], 1, 1.),
            ],
            0.875,
            true,
        )
        .unwrap()],
    );
    let baseless_group = document(
        2,
        vec![Layer::pass_through(
            "G",
            vec![
                layer("A", vec![a; 2], 0, 0.75).with_clipped(true),
                layer("B", vec![b; 1], 1, 1.),
            ],
            0.875,
            true,
        )
        .unwrap()],
    );
    assert_eq!(render(&baseless_group), render(&plain_group));

    let hidden_member = document(
        2,
        vec![
            layer("A", vec![a; 2], 0, 0.75),
            layer("B", vec![b; 1], 1, 1.),
            Layer::raster("M", image(vec![b; 2]), (0, 0), 1., false)
                .unwrap()
                .with_clipped(true),
        ],
    );
    assert_eq!(render(&hidden_member), render(&plain));

    let invisible_base = document(
        2,
        vec![
            layer("A", vec![a; 2], 0, 0.75),
            Layer::raster("Base", image(vec![b; 2]), (0, 0), 1., false).unwrap(),
            layer("M", vec![b; 2], 0, 1.).with_clipped(true),
        ],
    );
    assert_eq!(
        render(&invisible_base),
        render(&document(2, vec![layer("A", vec![a; 2], 0, 0.75)]))
    );

    // A visible clip base at opacity 0 over a transparent canvas: the unit's
    // gated alpha is 0 everywhere, so nothing composites and the render stays
    // finite and fully transparent.
    let zero_opacity_base = document(
        2,
        vec![
            layer("Base", vec![b; 2], 0, 0.),
            layer("M", vec![a; 2], 0, 1.).with_clipped(true),
        ],
    );
    assert_eq!(render(&zero_opacity_base), vec![[0u16; 4]; 2]);
}

// ------------------------------------------------------ (c) isolation --

fn pass_through_pair(isolated: bool) -> PrecisionDocument {
    let b = [30001, 20002, 10003, 65535];
    let r = [40000, 20000, 10000, 32768];
    let m = [1000, 60000, 30000, 50000];
    let children = vec![adjustment("Invert"), layer("R", vec![r; 2], 0, 0.75)];
    let group = if isolated {
        Layer::isolated_normal("G", children, 0.875, true)
    } else {
        Layer::pass_through("G", children, 0.875, true)
    }
    .unwrap();
    document(
        2,
        vec![
            layer("B", vec![b; 2], 0, 1.),
            group,
            layer("M", vec![m; 2], 0, 1.).with_clipped(true),
        ],
    )
}

#[test]
fn a_pass_through_base_or_member_with_a_base_is_isolated_and_an_adjustment_base_hides_members() {
    assert_eq!(
        render(&pass_through_pair(false)),
        render(&pass_through_pair(true))
    );
    // Unclipped, the same Pass Through group applies its adjustment to B.
    let unclipped = clip(&pass_through_pair(false), 4, false);
    assert_ne!(render(&unclipped), render(&pass_through_pair(true)));

    let b = [30001, 20002, 10003, 65535];
    let base = [40000, 20000, 10000, 40000];
    let r = [1000, 60000, 30000, 50000];
    let member = |isolated: bool| {
        let children = vec![adjustment("Invert"), layer("R", vec![r; 2], 0, 0.5)];
        if isolated {
            Layer::isolated_normal("G", children, 0.875, true)
        } else {
            Layer::pass_through("G", children, 0.875, true)
        }
        .unwrap()
        .with_clipped(true)
    };
    let with = |isolated| {
        document(
            2,
            vec![
                layer("B", vec![b; 2], 0, 1.),
                layer("Base", vec![base; 2], 0, 0.75),
                member(isolated),
            ],
        )
    };
    assert_eq!(render(&with(false)), render(&with(true)));
    for x in 0..2 {
        // Hand-computed: the adjustment sees a transparent buffer, so the
        // member is R alone at opacity 0.5 inside a group at 0.875.
        // Every opacity at its byte (#2).
        let inner = over([0.; 4], normalized(r), crate::common::byte_unit(0.5), 0);
        let expected = clip_unit(
            normalized(b),
            (normalized(base), 1.),
            (crate::common::byte_unit(0.75), 0),
            &[(inner, crate::common::byte_unit(0.875), 0)],
        );
        close(render(&with(false))[x], expected);
    }

    let adjustment_base = document(
        2,
        vec![
            layer("B", vec![b; 2], 0, 1.),
            adjustment("Invert"),
            layer("M", vec![r; 2], 0, 1.).with_clipped(true),
        ],
    );
    assert_eq!(
        render(&adjustment_base),
        render(&document(2, vec![layer("B", vec![b; 2], 0, 1.)]))
    );
}

// --------------------------------------------------------- (d) nesting --

#[test]
fn clip_units_nest_inside_groups_and_a_clipped_group_member_composites_as_a_unit() {
    let b = [30001, 20002, 10003, 65535];
    let base = [40000, 20000, 10000, 40000];
    let r1 = [1000, 60000, 30000, 50000];
    let r2 = [20000, 5000, 45000, 30000];
    let unit = |clipped: bool| {
        vec![
            layer("Base", vec![base; 2], 0, 0.75)
                .with_blend_mode(BlendMode::Multiply)
                .unwrap(),
            Layer::isolated_normal(
                "GM",
                vec![
                    layer("R1", vec![r1; 2], 0, 1.),
                    layer("R2", vec![r2; 2], 0, 0.5),
                ],
                0.875,
                true,
            )
            .unwrap()
            .with_clipped(clipped),
        ]
    };
    // Every opacity at its byte (#2).
    let member_px = over(
        over([0.; 4], normalized(r1), 1., 0),
        normalized(r2),
        crate::common::byte_unit(0.5),
        0,
    );
    let expected_unit = |backdrop| {
        clip_unit(
            backdrop,
            (normalized(base), 1.),
            (crate::common::byte_unit(0.75), 1),
            &[(member_px, crate::common::byte_unit(0.875), 0)],
        )
    };
    // Directly at root.
    let root = document(2, {
        let mut v = vec![layer("B", vec![b; 2], 0, 1.)];
        v.extend(unit(true));
        v
    });
    for x in 0..2 {
        close(render(&root)[x], expected_unit(normalized(b)));
    }
    assert_ne!(
        render(&root),
        render(&document(2, {
            let mut v = vec![layer("B", vec![b; 2], 0, 1.)];
            v.extend(unit(false));
            v
        }))
    );
    // Inside an isolated group at opacity 0.625.
    let isolated = document(
        2,
        vec![
            layer("B", vec![b; 2], 0, 1.),
            Layer::isolated_normal("G", unit(true), 0.625, true).unwrap(),
        ],
    );
    for x in 0..2 {
        close(
            render(&isolated)[x],
            over(
                normalized(b),
                expected_unit([0.; 4]),
                crate::common::byte_unit(0.625),
                0,
            ),
        );
    }
    // Inside a pass-through group at full opacity.
    let pass = document(
        2,
        vec![
            layer("B", vec![b; 2], 0, 1.),
            Layer::pass_through("G", unit(true), 1., true).unwrap(),
        ],
    );
    for x in 0..2 {
        close(render(&pass)[x], expected_unit(normalized(b)));
    }
    assert_eq!(root.render_plan().unwrap().group_depth, 2);
}

#[test]
fn the_group_depth_limit_bounds_clip_nesting_exactly_as_it_bounds_groups() {
    let px = [1000, 2000, 3000, 65535];
    let nested = |depth: usize| {
        let mut inner = layer("Leaf", vec![px; 1], 0, 1.);
        for level in 0..depth {
            inner = Layer::isolated_normal(&format!("G{level}"), vec![inner], 1., true).unwrap();
        }
        inner
    };
    let at = |depth: usize| {
        document(
            1,
            vec![
                nested(depth),
                layer("M", vec![px; 1], 0, 0.5).with_clipped(true),
            ],
        )
    };
    let full = at(MAX_GROUP_DEPTH);
    assert_eq!(
        full.layer(MAX_GROUP_DEPTH)
            .unwrap()
            .children()
            .unwrap()
            .len(),
        1
    );
    let refusal = full.render_plan().expect_err("depth refusal");
    assert!(refusal.contains("group-depth limit"), "{refusal}");
    assert!(full.render(LIMIT, 0, None).is_err());
    let below = at(MAX_GROUP_DEPTH - 1);
    let plan = below.render_plan().unwrap();
    assert_eq!(plan.group_depth, MAX_GROUP_DEPTH, "base depth + 1");
    assert_eq!(render(&below).len(), 1);
    // Unclipped, the same stack renders at the base's own depth.
    let released = clip(&below, MAX_GROUP_DEPTH, false);
    assert_eq!(
        released.render_plan().unwrap().group_depth,
        MAX_GROUP_DEPTH - 1
    );
}

// ------------------------------------------------------------ (e) edit --

#[test]
fn layer_clipped_plans_edits_reports_and_shares_storage() {
    let px = [1000, 2000, 3000, 65535];
    let doc = document(
        2,
        vec![
            layer("B", vec![px; 2], 0, 1.),
            layer("T", vec![px; 2], 0, 0.5).with_mask(Some(mask(vec![1, 65535], 0, 1.))),
            Layer::isolated_normal("G", vec![adjustment("A")], 1., true).unwrap(),
        ],
    );
    let request = json!({"op":"layer_clipped","layer":1,"clipped":true});
    let planned = plan(&doc, request.clone()).unwrap();
    assert_eq!(planned["input_samples"], json!(0));
    assert!(planned["scratch_bytes"].as_u64().unwrap() > 0);
    let set = edit(&doc, request.clone()).unwrap().unwrap();
    assert!(set.layer(1).unwrap().clipped());
    assert!(!doc.layer(1).unwrap().clipped(), "pure");
    assert!(
        edit(&set, request).unwrap().is_none(),
        "exact no-op is NULL"
    );
    assert!(edit(
        &doc,
        json!({"op":"layer_clipped","layer":1,"clipped":false})
    )
    .unwrap()
    .is_none());
    let cleared = clip(&set, 1, false);
    assert!(!cleared.layer(1).unwrap().clipped());
    for target in [2usize, 3] {
        let out = clip(&doc, target, true);
        assert!(
            out.layer(target).unwrap().clipped(),
            "any kind may carry it"
        );
    }
    assert!(edit(&doc, json!({"op":"layer_clipped","layer":4,"clipped":true})).is_err());
    assert!(plan(&doc, json!({"op":"layer_clipped","layer":4,"clipped":true})).is_err());
    assert!(edit(
        &doc,
        json!({"op":"layer_clipped","layer":1,"clipped":true,"version":1})
    )
    .is_err());
    let layers = info(&set)["layers"].as_array().unwrap().clone();
    assert_eq!(layers[1]["clipped"], Value::Bool(true));
    assert_eq!(layers[0]["clipped"], Value::Bool(false));
    assert_eq!(layers[3]["clipped"], Value::Bool(false));
    for index in 0..2 {
        assert!(PrecisionImage::ptr_eq(
            set.layer_source(index).unwrap(),
            doc.layer_source(index).unwrap()
        ));
        assert_eq!(
            set.layer(index).unwrap().name().as_ptr(),
            doc.layer(index).unwrap().name().as_ptr()
        );
    }
    assert_eq!(
        set.layer(1)
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
        info(&set)["supported_operations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v == &&json!("clipping"))
            .count(),
        1
    );
}

// ---------------------------------------------------------- (f) native --

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn bytes(doc: &PrecisionDocument) -> Vec<u8> {
    doc.encode_native(LIMIT, 0, None).unwrap()
}
fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}
/// An in-test reader of the `LCLP` tail: the last `12 + 4n` bytes.
fn read_lclp(data: &[u8]) -> Vec<u32> {
    let at = data.windows(4).rposition(|v| v == b"LCLP").unwrap();
    assert_eq!(word(data, at + 4), 1, "schema");
    let count = word(data, at + 8) as usize;
    assert_eq!(
        data.len(),
        at + 12 + 4 * count,
        "the tail ends the prior body"
    );
    (0..count).map(|i| word(data, at + 12 + 4 * i)).collect()
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
            layer("Top", px, 0, 0.5),
        ],
    )
}

#[test]
fn native32_wraps_the_exact_prior_writer_and_composes_with_mask29_and_blend31() {
    let plain = native_fixture();
    let clipped = clip(&clip(&plain, 2, true), 3, true);
    // 12E-1's identity wrapper is the OUTERMOST word at both depths and its
    // `LIDS` table closes every file; this test's subject is the wrapper
    // INSIDE it, so the outer one is peeled off and everything below is the
    // arithmetic this test was written with.
    let written = bytes(&clipped);
    assert_eq!(drdc_word(&written, 0), IDENTITY_TYPED);
    let data = prior_body(&written);
    assert_eq!(&data[4..12], words(&[32, 13]));
    assert_eq!(read_lclp(&data), [2, 3]);
    let prior = prior_body(&bytes(&plain));
    let tail = 12 + 8;
    assert_eq!(&data[12..data.len() - tail], &prior[8..]);
    assert_eq!(&data[data.len() - tail..], {
        let mut t = b"LCLP".to_vec();
        t.extend(words(&[1, 2, 2, 3]));
        t
    });
    let released = clip(&clip(&clipped, 2, false), 3, false);
    assert_eq!(
        prior_body(&bytes(&released)),
        prior,
        "releasing the last flag restores the prior writer"
    );
    let reopened = PrecisionDocument::parse_native(&written, LIMIT, 0, None).unwrap();
    assert!(reopened.layer(2).unwrap().clipped() && reopened.layer(3).unwrap().clipped());
    assert!(!reopened.layer(0).unwrap().clipped() && !reopened.layer(1).unwrap().clipped());
    assert_eq!(render(&reopened), render(&clipped));
    assert_eq!(bytes(&reopened), written);

    // 32 → 29 → 31 → body → LBLD → MOUT → LCLP.
    let composed = outside(
        &changed(
            &plain,
            json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":BlendMode::Subtract.to_c()}),
        ),
        1,
    );
    let composed_clipped = clip(&composed, 4, true);
    let written = bytes(&composed_clipped);
    let data = prior_body(&written);
    assert_eq!(&data[4..20], words(&[32, 29, 31, 13]));
    let at = |tag: &[u8]| data.windows(4).rposition(|v| v == tag).unwrap();
    assert!(at(b"LBLD") < at(b"MOUT") && at(b"MOUT") < at(b"LCLP"));
    assert_eq!(read_lclp(&data), [4]);
    let prior = prior_body(&bytes(&composed));
    assert_eq!(&prior[4..16], words(&[29, 31, 13]));
    assert_eq!(&data[8..data.len() - 16], &prior[4..]);
    assert_eq!(
        prior_body(&bytes(&clip(&composed_clipped, 4, false))),
        prior
    );
    let reopened = PrecisionDocument::parse_native(&written, LIMIT, 0, None).unwrap();
    assert!(reopened.layer(4).unwrap().clipped());
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
    assert_eq!(reopened.layer(1).unwrap().blend_mode(), BlendMode::Subtract);
    assert_eq!(bytes(&reopened), written);
}

#[test]
fn malformed_clipping_framing_refuses_before_any_png_decode() {
    let plain = native_fixture();
    let good = bytes(&clip(&plain, 3, true));
    // The `LCLP` table is located by its tag: `LIDS` closes the file now.
    let tail = good.windows(4).rposition(|v| v == b"LCLP").unwrap();
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
        (tail + 8, 0, "clipping count"),
        (tail + 8, 9, "clipping count"),
        (tail + 12, 5, "clipping reference"),
        (tail + 4, 2, "clipping schema"),
    ] {
        let mut bad = good.clone();
        bad[position..position + 4].copy_from_slice(&u32::to_le_bytes(value));
        corrupt(&mut bad);
        refuse(bad, needle);
    }
    let two = bytes(&clip(&clip(&plain, 2, true), 3, true));
    let tail2 = two.windows(4).rposition(|v| v == b"LCLP").unwrap();
    for (a, b) in [(3u32, 2u32), (2, 2)] {
        let mut bad = two.clone();
        bad[tail2 + 12..tail2 + 16].copy_from_slice(&a.to_le_bytes());
        bad[tail2 + 16..tail2 + 20].copy_from_slice(&b.to_le_bytes());
        corrupt(&mut bad);
        refuse(bad, "clipping reference");
    }
    let mut trailing = good.clone();
    trailing.push(0);
    corrupt(&mut trailing);
    refuse(trailing, "trailing");
    // LCLP under a non-32 outer.
    let blended = changed(
        &plain,
        json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":BlendMode::Subtract.to_c()}),
    );
    // The `LCLP` table is spliced in AHEAD of the identity tail, which must
    // stay last: `LIDS` closes every file.
    let blended_bytes = bytes(&blended);
    let records = identity::identity_records(&blended_bytes);
    let split = blended_bytes.len() - identity::identity_tail(records);
    assert_eq!(word(&blended_bytes, 8), 31);
    let mut under31 = blended_bytes[..split].to_vec();
    under31.extend_from_slice(&good[tail..good.len() - identity::identity_tail(records)]);
    under31.extend_from_slice(&blended_bytes[split..]);
    corrupt(&mut under31);
    // Under a non-32 outer the reader does not expect `LCLP` at all, so the
    // block it finds where the identity table belongs is what it names.
    refuse(under31, "identity");
    // 32 nested inside 29.
    let with_outside = clip(&outside(&plain, 1), 3, true);
    let data = bytes(&with_outside);
    assert_eq!(&data[8..16], words(&[32, 29]));
    let mut reversed = data.clone();
    reversed[8..16].copy_from_slice(&words(&[29, 32]));
    corrupt(&mut reversed);
    refuse(reversed, "mask outside typed body version");
    // The uncorrupted saves still open.
    assert!(PrecisionDocument::parse_native(&good, LIMIT, 0, None).is_ok());
    assert!(PrecisionDocument::parse_native(&data, LIMIT, 0, None).is_ok());
}

// ---------------------------------------------------- (g) preservation --

#[test]
fn the_flag_survives_every_metadata_geometry_and_retouch_edit() {
    let px = [1000, 2000, 3000, 65535];
    let doc = clip(
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
        2,
        true,
    );
    let flags = |d: &PrecisionDocument| {
        (0..d.layer_count())
            .map(|i| d.layer(i).unwrap().clipped())
            .collect::<Vec<_>>()
    };
    assert_eq!(flags(&doc), [false, false, true, false]);
    let cases: Vec<(&str, PrecisionDocument, Vec<bool>)> = vec![
        (
            "duplicate",
            doc.duplicate_layer(2).unwrap().unwrap(),
            vec![false, false, true, true, false],
        ),
        (
            "rename",
            changed(
                &doc,
                json!({"op":"rename_layer","layer":2,"name":"Renamed"}),
            ),
            flags(&doc),
        ),
        (
            "visible",
            changed(
                &doc,
                json!({"op":"layer_visible","layer":2,"visible":false}),
            ),
            flags(&doc),
        ),
        (
            "opacity",
            doc.set_layer_opacity(2, 0.25).unwrap().unwrap(),
            flags(&doc),
        ),
        (
            "blend",
            changed(
                &doc,
                json!({"op":"layer_blend","version":1,"layer":2,"blend_mode":1}),
            ),
            flags(&doc),
        ),
        (
            "set_mask",
            edit_samples(
                &doc,
                json!({"op":"set_mask","layer":2,"width":2,"height":1,"x":0,"y":0}),
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
    ];
    for (name, out, expected) in &cases {
        assert_eq!(&flags(out), expected, "{name}");
        assert!(out.encode_native(LIMIT, 0, None).is_ok(), "{name}");
    }
    let masked = &cases[5].1;
    let removed = changed(masked, json!({"op":"remove_mask","layer":2}));
    assert_eq!(flags(&removed), flags(&doc));

    // Retouch onto a new layer inherits the target's flag …
    let inherited = doc
        .retouch(
            &doctored_core::precision_retouch::RetouchEdit::parse(
                &json!({"op":"retouch_clone","layer":2,"output":"new_layer"}).to_string(),
            )
            .unwrap(),
            &[65535, 65535, 65535],
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(flags(&inherited), [false, false, true, true, false]);

    // … and a clipped Pass Through ancestor refuses new-layer output.
    let gated = clip(&doc, 3, true);
    let new_layer = doctored_core::precision_retouch::RetouchEdit::parse(
        &json!({"op":"retouch_clone","layer":1,"output":"new_layer"}).to_string(),
    )
    .unwrap();
    let refusal = gated.retouch_plan(&new_layer).expect_err("refusal");
    assert_eq!(
        refusal,
        "new retouch layer inside a gated group would apply its opacity, mask or clip twice; use in-place output"
    );
    let in_place = doctored_core::precision_retouch::RetouchEdit::parse(
        &json!({"op":"retouch_clone","layer":1,"matrix":[1,0,0,1,1,0]}).to_string(),
    )
    .unwrap();
    let out = gated
        .retouch(&in_place, &[65535, 65535, 65535], LIMIT, 0, None)
        .unwrap()
        .unwrap();
    assert_eq!(flags(&out), flags(&gated));
    assert!(
        doc.retouch_plan(&new_layer).is_ok(),
        "unclipped ancestor admits"
    );
}

// ---------------------------------------------------------- (h) PSD16 --

fn report(path: &Path) -> Value {
    let mut err = ptr::null_mut();
    let json = unsafe { dr_psd_import_report(cpath(path).as_ptr(), &mut err) };
    assert!(!json.is_null(), "{}", take_err_string(err));
    serde_json::from_str(&take_err_string(json)).unwrap()
}

#[test]
fn a_clipped_raster_and_folder_round_trip_through_sixteen_bit_psd() {
    let directory = TempDir::new().unwrap();
    let b = [30001, 20002, 10003, 65535];
    let base = [40000, 20000, 10000, 40000];
    let r = [1000, 60000, 30000, 50000];
    let m = [5000, 6000, 7000, 65535];
    let source = document(
        2,
        vec![
            // Opacities the PSD's u8 byte holds exactly, so the projection
            // reopens identically.
            layer("B", vec![b; 2], 0, 1.),
            layer("Base", vec![base; 2], 0, 1.),
            layer("M", vec![m; 2], 0, 1.).with_clipped(true),
            Layer::pass_through("Folder", vec![layer("R", vec![r; 2], 0, 1.)], 1., true)
                .unwrap()
                .with_clipped(true),
        ],
    );
    let projection = source.render(LIMIT, 0, None).unwrap();
    let path = directory.path().join("clipped16.psd");
    let mut err = ptr::null_mut();
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
                std::ptr::null(),
                &mut err,
            )
        },
        "{}",
        take_err_string(err)
    );
    let file = fixture::read(&std::fs::read(&path).unwrap());
    let by_name = fixture::by_name(&file);
    assert_eq!(by_name["M"].clipping, 1);
    assert_eq!(by_name["Folder"].clipping, 1);
    assert_eq!(by_name["Base"].clipping, 0);
    assert_eq!(by_name["R"].clipping, 0);
    assert_eq!(by_name["B"].clipping, 0);
    assert_eq!(by_name["</Layer group>"].clipping, 0);

    let notice = report(&path);
    assert_eq!(notice["import_precision"], json!("rgba16"));
    assert!(!notice["verified_losses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l.as_str().unwrap().contains("is clipped")));
    let token = CString::new(notice["source_fingerprint"].as_str().unwrap()).unwrap();
    let raw = unsafe {
        dr_precision_doc_open_psd(
            cpath(&path).as_ptr(),
            0,
            token.as_ptr(),
            LIMIT,
            0,
            ptr::null::<DrJobControl>(),
            &mut err,
        )
    };
    assert!(!raw.is_null(), "{}", take_err_string(err));
    let reopened = unsafe { Box::from_raw(raw) };
    let layers = info(&reopened)["layers"].as_array().unwrap().clone();
    let names: Vec<&str> = layers.iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["B", "Base", "M", "R", "Folder"]);
    assert_eq!(layers[2]["clipped"], Value::Bool(true));
    assert_eq!(layers[4]["clipped"], Value::Bool(true));
    assert_eq!(layers[1]["clipped"], Value::Bool(false));
    assert_eq!(layers[3]["clipped"], Value::Bool(false));
    assert_eq!(render(&reopened), projection.to_rgba16().ok().unwrap());
}

// ------------------------------------------------------------ (i) cuts --

#[test]
fn typed_cuts_gate_contribution_and_keep_the_documents_isolation() {
    let b = [30001, 20002, 10003, 65535];
    let t = [40000, 20000, 10000, 32768];
    let m = [1000, 60000, 30000, 50000];
    let build = |isolated: bool, member_visible: bool| {
        let children = vec![adjustment("Invert"), layer("T", vec![t; 2], 0, 1.)];
        let group = if isolated {
            Layer::isolated_normal("G", children, 1., true)
        } else {
            Layer::pass_through("G", children, 1., true)
        }
        .unwrap();
        document(
            2,
            vec![
                layer("B", vec![b; 2], 0, 1.),
                group,
                Layer::raster("M", image(vec![m; 2]), (0, 0), 1., member_visible)
                    .unwrap()
                    .with_clipped(true),
            ],
        )
    };
    let doc = build(false, true);
    let expected = render(&build(true, false));
    assert_ne!(
        expected,
        render(&build(false, false)),
        "the adjustment reaches B when G passes through"
    );
    assert_eq!(render(&doc), render(&build(true, true)));

    // The current_and_below plate for T (index 2), as a new retouch layer.
    let plate = doc
        .retouch(
            &doctored_core::precision_retouch::RetouchEdit::parse(
                &json!({"op":"retouch_clone","layer":2,"sample":"current_and_below","output":"new_layer"}).to_string(),
            )
            .unwrap(),
            &[65535, 65535],
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let repair = plate.layer_source(3).unwrap().to_rgba16().ok().unwrap();
    for (a, e) in repair.iter().zip(&expected) {
        for c in 0..4 {
            assert!(a[c].abs_diff(e[c]) <= 1, "{a:?} vs {e:?}");
        }
    }
    // The backdrop below M (index 4) through scoped analysis.
    for x in 0..2 {
        let sample = doc
            .scoped_analysis(
                &AnalysisRequest::parse(
                    &json!({"operation":"sample","source":"backdrop","below":4,"x":x,"y":0})
                        .to_string(),
                )
                .unwrap(),
                &[],
                LIMIT,
                0,
                None,
            )
            .unwrap();
        assert_eq!(sample["center_rgba_codes"], json!(expected[x as usize]));
    }

    // The ignore cut on an adjustment member above T keeps its base isolated.
    let ignore_doc = document(
        2,
        vec![
            layer("B", vec![b; 2], 0, 1.),
            Layer::pass_through(
                "G",
                vec![adjustment("Inner"), layer("T", vec![t; 2], 0, 1.)],
                1.,
                true,
            )
            .unwrap(),
            adjustment("Above").with_clipped(true),
        ],
    );
    let expected = render(&document(
        2,
        vec![
            layer("B", vec![b; 2], 0, 1.),
            Layer::isolated_normal(
                "G",
                vec![adjustment("Inner"), layer("T", vec![t; 2], 0, 1.)],
                1.,
                true,
            )
            .unwrap(),
        ],
    ));
    let plate = ignore_doc
        .retouch(
            &doctored_core::precision_retouch::RetouchEdit::parse(
                &json!({"op":"retouch_clone","layer":2,"sample":"all","ignore_adjustments":true,"output":"new_layer"}).to_string(),
            )
            .unwrap(),
            &[65535, 65535],
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let repair = plate.layer_source(3).unwrap().to_rgba16().ok().unwrap();
    for (a, e) in repair.iter().zip(&expected) {
        for c in 0..4 {
            assert!(a[c].abs_diff(e[c]) <= 1, "{a:?} vs {e:?}");
        }
    }
}
