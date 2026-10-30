//! Phase 13A-3c: the 16-bit regional render over KNOCKOUT (Shallow / Deep)
//! and "Blend Interior Effects as Group" — the typed twin of
//! `regional_byte_knockout.rs`: D5's Background (the scene's own
//! `KnockoutSeed`) and every one of its disqualifiers, typed Blend If
//! included; Blend If, Dissolve, masked and off-canvas knockout layers, the
//! package, D4's pass-through (plain, gated, masked, nested), isolated and
//! materialized hard stops, knockout members under every kind of base, a
//! knockout clip base, D6 on and off; the plan's materialized-subtree
//! scratch restated with `collect`'s knockout term; the float and
//! linear-light policies. The oracle is INDEPENDENT of the regional code:
//! `dr_precision_doc_render`'s projection read back through
//! `dr_precision_image_pixels_rgba16`, cropped and top-left-cell sampled
//! here, compared as u16 codes. (The concurrent-cancel u16 twin lives in
//! `regional_byte_knockout.rs`, so one command measures both depths.)

use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::BlendMode;
use doctored_core::doc_evaluation::Evaluation;
use doctored_core::doc_mask::MaskProperties;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_jobs::*;
use doctored_core::ffi_precision::*;
use doctored_core::ffi_precision_edit::dr_precision_doc_edit;
use doctored_core::ffi_regional::*;
use doctored_core::icc::IccProfile;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::{PrecisionDocument, PrecisionDocumentLayer as Layer};
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_mask::{PrecisionMask, PrecisionPlane};
use doctored_core::style::{BlendIf, BlendIfChannel, LayerStyle};
use serde_json::{json, Value};

use crate::region_plan;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 41;
const H: u32 = 29;
const FLOAT_FIXTURE: &str = "tests/fixtures/float_11f/scene.tiff";

// ------------------------------------------------------------- plumbing --

fn take(p: *mut c_char) -> String {
    if p.is_null() {
        return "<no error message>".into();
    }
    let text = unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned();
    unsafe { dr_string_free(p) };
    text
}

fn samples(image: *mut PrecisionImage) -> (u32, u32, Vec<u32>) {
    let (w, h) = unsafe {
        (
            dr_precision_image_width(image),
            dr_precision_image_height(image),
        )
    };
    let n = (w * h * 4) as usize;
    let p16 = unsafe { dr_precision_image_pixels_rgba16(image) };
    let out = if !p16.is_null() {
        unsafe { std::slice::from_raw_parts(p16, n) }
            .iter()
            .map(|&v| u32::from(v))
            .collect()
    } else {
        unsafe { &*image }
            .to_rgba32()
            .expect("an image with samples")
            .as_flattened()
            .iter()
            .map(|v| v.to_bits())
            .collect()
    };
    unsafe { dr_precision_image_free(image) };
    (w, h, out)
}

fn full(doc: &PrecisionDocument) -> (u32, u32, Vec<u32>) {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_precision_doc_render(doc, LIMIT, 0, ptr::null(), &mut err) };
    assert!(!image.is_null(), "render: {}", take(err));
    samples(image)
}

fn region(
    doc: &PrecisionDocument,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> Result<(u32, u32, Vec<u32>), String> {
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe {
        dr_precision_doc_render_region(doc, x, y, w, h, d, LIMIT, 0, ptr::null(), &mut err)
    };
    if image.is_null() {
        return Err(take(err));
    }
    assert!(err.is_null(), "err_out set on success");
    Ok(samples(image))
}

fn plan(
    doc: &PrecisionDocument,
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> Result<Value, String> {
    region_plan::typed(doc, (x, y, w, h), d)
}

/// The independent oracle: sample (x + i·d, y + j·d) of the full render.
fn expected(
    (cw, _, reference): &(u32, u32, Vec<u32>),
    (x, y, w, h): (u32, u32, u32, u32),
    d: u32,
) -> (u32, u32, Vec<u32>) {
    let ow = w.div_ceil(d);
    let oh = h.div_ceil(d);
    let mut out = Vec::new();
    for j in 0..oh {
        for i in 0..ow {
            let cx = u64::from(x) + u64::from(i) * u64::from(d);
            let cy = u64::from(y) + u64::from(j) * u64::from(d);
            let at = ((cy * u64::from(*cw) + cx) * 4) as usize;
            out.extend_from_slice(&reference[at..at + 4]);
        }
    }
    (ow, oh, out)
}

fn assert_regions_match_with(doc: &PrecisionDocument, what: &str, extra: &[(u32, u32, u32, u32)]) {
    let reference = full(doc);
    let (cw, ch) = doc.dimensions();
    let mut windows = vec![
        (0, 0, cw, ch),
        (3, 2, 11, 7),
        (0, 0, 5, 4),
        (cw - 6, 0, 6, 5),
        (0, ch - 3, 7, 3),
        (cw - 4, ch - 5, 4, 5),
        (0, 4, cw, 3),
        (7, 0, 2, ch),
        (cw - 1, ch - 1, 1, 1),
        (0, 0, 1, 1),
        (19, 13, 1, 1),
    ];
    windows.extend_from_slice(extra);
    for window in windows {
        for d in [1, 2, 3, 7, u32::MAX] {
            let got =
                region(doc, window, d).unwrap_or_else(|e| panic!("{what}: {window:?}/{d}: {e}"));
            assert_eq!(
                got,
                expected(&reference, window, d),
                "{what}: window {window:?} divisor {d}"
            );
        }
    }
}

fn assert_regions_match(doc: &PrecisionDocument, what: &str) {
    assert_regions_match_with(doc, what, &[]);
}

/// The u16 sample at canvas (x, y) of a full render.
fn at((cw, _, pixels): &(u32, u32, Vec<u32>), x: u32, y: u32) -> [u32; 4] {
    let i = ((y * cw + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

// ------------------------------------------------------------- fixtures --

fn profile() -> Arc<IccProfile> {
    IccProfile::srgb()
}

fn image16(w: u32, h: u32, seed: u32, alpha: impl Fn(u32, u32) -> u16) -> PrecisionImage {
    let pixels = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            [
                ((x * 2711 + y * 911 + seed * 1234) % 65536) as u16,
                ((x * 523 + y * 3301 + seed * 77) % 65536) as u16,
                ((x ^ y) * 1999 % 65536 + seed) as u16,
                alpha(x, y),
            ]
        })
        .collect();
    PrecisionImage::from_rgba16(w, h, pixels, profile(), None).unwrap()
}

/// A shaped u16 layer: transparent outside an ellipse and in a hole, graded
/// alpha near the rim, opaque inside.
fn shape16(w: u32, h: u32, seed: u32) -> PrecisionImage {
    image16(w, h, seed, |x, y| {
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let dx = (x as f32 + 0.5 - cx) / cx;
        let dy = (y as f32 + 0.5 - cy) / cy;
        let r = dx * dx + dy * dy;
        let hole = x == w / 2 + 1 && (y == h / 2 || y == h / 2 + 1);
        if r > 1.0 || hole {
            0
        } else if r > 0.7 {
            (9000 + (x * 3001 + y * 1777 + seed * 97) % 50000) as u16
        } else {
            65535
        }
    })
}

/// The canvas-sized Background at (0, 0) — D5 qualifies it — with
/// translucent top rows.
fn base() -> Layer {
    let image = image16(W, H, 1, |x, y| {
        if y < 4 {
            (20000 + (x * 1291) % 30000) as u16
        } else {
            65535
        }
    });
    Layer::raster("Base", image, (0, 0), 1.0, true).unwrap()
}

fn shaped(name: &str, w: u32, h: u32, offset: (i32, i32), mode: BlendMode, opacity: f32) -> Layer {
    Layer::raster(name, shape16(w, h, 5), offset, opacity, true)
        .unwrap()
        .with_blend_mode(mode)
        .unwrap()
}

fn translucent(
    name: &str,
    w: u32,
    h: u32,
    offset: (i32, i32),
    mode: BlendMode,
    opacity: f32,
) -> Layer {
    let image = image16(w, h, 7, |x, y| {
        (9000 + (x * 3001 + y * 1777) % 56000) as u16
    });
    Layer::raster(name, image, offset, opacity, true)
        .unwrap()
        .with_blend_mode(mode)
        .unwrap()
}

fn style(json: &str) -> LayerStyle {
    LayerStyle::from_json(json).unwrap_or_else(|e| panic!("{json}: {e}"))
}

fn styled(layer: Layer, json: &str) -> Layer {
    layer.with_layer_style(Some(&style(json)), (W, H)).unwrap()
}

fn adjustment(name: &str, params: Value) -> Layer {
    let op = json!({"type":"adjust","op":name,"params":params,"evaluation":"precision_v1"});
    Layer::adjustment(name, &op.to_string(), 1.0, true, &profile()).unwrap()
}

fn doc(layers: Vec<Layer>) -> PrecisionDocument {
    PrecisionDocument::from_parts(
        W,
        H,
        profile(),
        Metadata::default(),
        Resolution::default(),
        layers,
    )
    .unwrap()
}

fn gradient(w: u32, h: u32) -> Vec<u16> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if x == w / 2 && y == h / 2 {
                0
            } else {
                ((x * 65535 / (w - 1).max(1)) ^ (y * 2311)) as u16
            }
        })
        .collect()
}

fn mask(w: u32, h: u32, offset: (i32, i32), density: f32, feather: f32) -> PrecisionMask {
    let mut properties = MaskProperties::at(offset);
    properties.density = density;
    properties.feather = feather;
    PrecisionMask::new(
        PrecisionPlane::from_u16(w, h, gradient(w, h), None).unwrap(),
        properties,
        true,
    )
    .unwrap()
}

fn blend_if() -> BlendIf {
    BlendIf {
        channel: BlendIfChannel::Gray,
        this_layer: [10, 60, 180, 240],
        underlying: [0, 40, 200, 255],
        ..BlendIf::default()
    }
}

fn around(offset: (i32, i32), w: u32, h: u32, grow: i64) -> Vec<(u32, u32, u32, u32)> {
    let clamp = |v: i64, hi: u32| v.clamp(0, i64::from(hi) - 1) as u32;
    let (x0, y0) = (i64::from(offset.0) - grow, i64::from(offset.1) - grow);
    let (x1, y1) = (
        i64::from(offset.0) + i64::from(w) + grow,
        i64::from(offset.1) + i64::from(h) + grow,
    );
    let mut out = Vec::new();
    for (cx, cy) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
        let x = clamp(cx - 3, W);
        let y = clamp(cy - 2, H);
        out.push((x, y, (W - x).min(6), (H - y).min(5)));
    }
    out
}

fn fx(effects: &[&str]) -> String {
    format!("{{\"effects\":[{}]}}", effects.join(","))
}

/// The byte file's knockout styles, one depth up.
const KNOCKOUTS: [(&str, &str); 5] = [
    (
        "shallow 0.5",
        "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}",
    ),
    (
        "shallow hole",
        "{\"fill_opacity\":0,\"knockout\":\"shallow\"}",
    ),
    ("deep 0.5", "{\"fill_opacity\":0.5,\"knockout\":\"deep\"}"),
    ("deep hole", "{\"fill_opacity\":0,\"knockout\":\"deep\"}"),
    ("deep full", "{\"knockout\":\"deep\"}"),
];

const TOP_AT: (i32, i32) = (6, 5);

fn screen() -> Layer {
    translucent("Screen", 30, 14, (4, 9), BlendMode::Screen, 0.8)
}

fn top(json: &str, mode: BlendMode, opacity: f32) -> Layer {
    styled(shaped("Top", 17, 12, TOP_AT, mode, opacity), json)
}

/// Background, a Screen raster, then the shaped knockout raster.
fn over_background(json: &str, mode: BlendMode, opacity: f32) -> PrecisionDocument {
    doc(vec![base(), screen(), top(json, mode, opacity)])
}

// ----------------------------------------------------------------- D5 --

#[test]
fn a_lone_knockout_over_the_background_matches_at_u16() {
    for (label, json) in KNOCKOUTS {
        for opacity in [1.0, 0.7] {
            let d = over_background(json, BlendMode::Normal, opacity);
            assert_regions_match_with(
                &d,
                &format!("{label} at {opacity}"),
                &around(TOP_AT, 17, 12, 1),
            );
        }
    }
    // The hole at the shape's opaque centre is the Background's alpha.
    let hole = full(&over_background(KNOCKOUTS[3].1, BlendMode::Normal, 1.0));
    assert_eq!(at(&hole, 14, 11)[3], 65535);
}

#[test]
fn each_background_disqualifier_punches_to_transparency_at_u16() {
    let deep = KNOCKOUTS[3].1;
    let top = || top(deep, BlendMode::Normal, 1.0);
    let white = PrecisionMask::new(
        PrecisionPlane::from_u16(W, H, vec![65535; (W * H) as usize], None).unwrap(),
        MaskProperties::at((0, 0)),
        true,
    )
    .unwrap();
    let cases: Vec<(&str, Vec<Layer>)> = vec![
        (
            "offset",
            vec![
                Layer::raster("Base", image16(W, H, 1, |_, _| 65535), (1, 0), 1.0, true).unwrap(),
                screen(),
                top(),
            ],
        ),
        (
            "opacity 0.9",
            vec![
                Layer::raster("Base", image16(W, H, 1, |_, _| 65535), (0, 0), 0.9, true).unwrap(),
                screen(),
                top(),
            ],
        ),
        (
            "enabled mask",
            vec![base().with_mask(Some(white)), screen(), top()],
        ),
        (
            "entry 1 clipped",
            vec![base(), screen().with_clipped(true), top()],
        ),
        (
            "hidden",
            vec![
                Layer::raster("Base", image16(W, H, 1, |_, _| 65535), (0, 0), 1.0, false).unwrap(),
                screen(),
                top(),
            ],
        ),
        (
            "multiply",
            vec![
                base().with_blend_mode(BlendMode::Multiply).unwrap(),
                screen(),
                top(),
            ],
        ),
        (
            "a style",
            vec![styled(base(), "{\"fill_opacity\":0.9}"), screen(), top()],
        ),
        (
            "a typed Blend If",
            vec![
                base().with_blend_if(Some(blend_if())).unwrap(),
                screen(),
                top(),
            ],
        ),
    ];
    for (label, layers) in cases {
        let d = doc(layers.clone());
        assert_eq!(
            at(&full(&d), 14, 11)[3],
            0,
            "{label}: the hole is transparent"
        );
        assert_regions_match_with(&d, label, &around(TOP_AT, 17, 12, 0));
        let mut shallow = layers;
        shallow[2] = styled(
            shaped("Top", 17, 12, TOP_AT, BlendMode::Normal, 1.0),
            KNOCKOUTS[0].1,
        );
        assert_regions_match(&doc(shallow), &format!("{label}, shallow"));
    }
}

#[test]
fn typed_blend_if_beside_a_knockout_matches() {
    for knockout in ["shallow", "deep"] {
        let json = format!("{{\"fill_opacity\":0.6,\"knockout\":\"{knockout}\"}}");
        for (mode, opacity) in [(BlendMode::Normal, 1.0), (BlendMode::Multiply, 0.8)] {
            let layer = top(&json, mode, opacity)
                .with_blend_if(Some(blend_if()))
                .unwrap();
            let d = doc(vec![base(), screen(), layer]);
            assert_regions_match_with(
                &d,
                &format!("blend if {knockout} {mode:?} {opacity}"),
                &around(TOP_AT, 17, 12, 0),
            );
        }
        let json = format!(
            "{{\"knockout\":\"{knockout}\",\"effects\":[\
             {{\"type\":\"drop_shadow\",\"distance\":4,\"size\":2}}]}}"
        );
        let layer = top(&json, BlendMode::Normal, 1.0)
            .with_blend_if(Some(blend_if()))
            .unwrap();
        assert_regions_match(
            &doc(vec![base(), screen(), layer]),
            &format!("blend if + shadow {knockout}"),
        );
    }
}

#[test]
fn a_dissolve_knockout_matches_at_u16() {
    for (label, json) in [
        KNOCKOUTS[0],
        KNOCKOUTS[3],
        (
            "deep + effects",
            "{\"fill_opacity\":0.5,\"knockout\":\"deep\",\"effects\":[\
             {\"type\":\"drop_shadow\",\"distance\":3,\"size\":2},{\"type\":\"color_overlay\"}]}",
        ),
    ] {
        let d = over_background(json, BlendMode::Dissolve, 0.6);
        assert_regions_match_with(&d, &format!("dissolve {label}"), &around(TOP_AT, 17, 12, 2));
    }
}

#[test]
fn masked_and_off_canvas_knockout_layers_match_at_u16() {
    for feather in [0.0, 6.0] {
        for (label, json) in [KNOCKOUTS[0], KNOCKOUTS[3]] {
            let layer = top(json, BlendMode::Normal, 1.0).with_mask(Some(mask(
                14,
                10,
                (8, 6),
                0.8,
                feather,
            )));
            assert_regions_match_with(
                &doc(vec![base(), screen(), layer]),
                &format!("masked {label} feather {feather}"),
                &around((8, 6), 14, 10, 0),
            );
        }
    }
    for offset in [(-6, -4), (30, 20)] {
        for (label, json) in [KNOCKOUTS[1], KNOCKOUTS[2]] {
            let layer = styled(shaped("Top", 17, 12, offset, BlendMode::Normal, 1.0), json);
            assert_regions_match_with(
                &doc(vec![base(), screen(), layer]),
                &format!("off canvas {offset:?} {label}"),
                &around(offset, 17, 12, 0),
            );
        }
    }
}

#[test]
fn the_package_with_a_knockout_matches_at_u16() {
    for knockout in ["shallow", "deep"] {
        let json = format!(
            "{{\"fill_opacity\":0.5,\"knockout\":\"{knockout}\",\"effects\":[\
             {{\"type\":\"drop_shadow\",\"distance\":4,\"size\":2}},\
             {{\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#3080c0\"}},\
             {{\"type\":\"inner_shadow\",\"distance\":2,\"size\":2}}]}}"
        );
        for mode in [BlendMode::Normal, BlendMode::Multiply] {
            let d = over_background(&json, mode, 0.6);
            assert_regions_match_with(
                &d,
                &format!("package {knockout} {mode:?}"),
                &around(TOP_AT, 17, 12, 5),
            );
        }
    }
}

// ----------------------------------------------------------------- D4 --

/// A Multiply raster and the shaped knockout child `json`.
fn pass_children(json: &str) -> Vec<Layer> {
    vec![
        translucent("Multiply", 20, 16, (10, 2), BlendMode::Multiply, 1.0),
        top(json, BlendMode::Normal, 1.0),
    ]
}

#[test]
fn a_shallow_or_deep_child_in_plain_gated_and_masked_pass_through_groups_matches_at_u16() {
    for (label, json) in KNOCKOUTS {
        for opacity in [1.0, 0.5] {
            let pass = Layer::pass_through("Pass", pass_children(json), opacity, true).unwrap();
            assert_regions_match_with(
                &doc(vec![base(), screen(), pass]),
                &format!("pass-through {opacity} {label}"),
                &around(TOP_AT, 17, 12, 0),
            );
        }
        let pass = Layer::pass_through("Pass", pass_children(json), 1.0, true)
            .unwrap()
            .with_mask(Some(mask(22, 15, (5, 4), 1.0, 0.0)));
        assert_regions_match(
            &doc(vec![base(), screen(), pass]),
            &format!("masked pass-through {label}"),
        );
    }
    // The Shallow hole shows the sample as the children began.
    let pass = Layer::pass_through("Pass", pass_children(KNOCKOUTS[1].1), 1.0, true).unwrap();
    assert_eq!(
        at(&full(&doc(vec![base(), screen(), pass])), 14, 11),
        at(&full(&doc(vec![base(), screen()])), 14, 11)
    );
}

#[test]
fn nested_pass_through_groups_snapshot_their_own_level_at_u16() {
    for (label, json) in KNOCKOUTS {
        let inner = |opacity: f32| {
            Layer::pass_through(
                "Inner",
                vec![
                    translucent("Screen2", 12, 12, (3, 6), BlendMode::Screen, 0.7),
                    top(json, BlendMode::Normal, 1.0),
                ],
                opacity,
                true,
            )
            .unwrap()
        };
        for opacity in [1.0, 0.6] {
            let outer = Layer::pass_through(
                "Outer",
                vec![
                    translucent("Multiply", 20, 16, (10, 2), BlendMode::Multiply, 1.0),
                    inner(opacity),
                ],
                1.0,
                true,
            )
            .unwrap();
            assert_regions_match(
                &doc(vec![base(), screen(), outer]),
                &format!("nested, inner {opacity} {label}"),
            );
        }
    }
}

#[test]
fn a_pass_through_group_without_a_shallow_child_under_a_level_with_one_matches_at_u16() {
    let inner = Layer::pass_through(
        "Inner",
        vec![
            translucent("Multiply", 12, 12, (20, 6), BlendMode::Multiply, 1.0),
            styled(
                shaped("Deep", 15, 11, (18, 12), BlendMode::Normal, 1.0),
                KNOCKOUTS[2].1,
            ),
        ],
        1.0,
        true,
    )
    .unwrap();
    let outer = Layer::pass_through(
        "Outer",
        vec![top(KNOCKOUTS[0].1, BlendMode::Normal, 1.0), inner],
        1.0,
        true,
    )
    .unwrap();
    assert_regions_match(
        &doc(vec![base(), screen(), outer]),
        "no Shallow child in the inner level",
    );
}

#[test]
fn knockouts_inside_isolated_and_materialized_groups_stop_at_the_buffer_at_u16() {
    for (label, json) in KNOCKOUTS {
        for (mode, opacity) in [(BlendMode::Multiply, 1.0), (BlendMode::Normal, 0.8)] {
            let group = Layer::isolated_normal("Group", pass_children(json), opacity, true)
                .unwrap()
                .with_blend_mode(mode)
                .unwrap();
            assert_regions_match(
                &doc(vec![base(), screen(), group]),
                &format!("isolated {mode:?} {opacity} {label}"),
            );
        }
        let materialized = styled(
            Layer::isolated_normal("Group", pass_children(json), 1.0, true).unwrap(),
            &fx(&["{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}"]),
        );
        assert_regions_match(
            &doc(vec![base(), screen(), materialized]),
            &format!("materialized {label}"),
        );
    }
    // The Deep hole inside an isolated group shows what lies beneath it.
    let group = Layer::isolated_normal("Group", pass_children(KNOCKOUTS[3].1), 1.0, true).unwrap();
    assert_eq!(
        at(&full(&doc(vec![base(), screen(), group])), 14, 11),
        at(&full(&doc(vec![base(), screen()])), 14, 11)
    );
}

// ---------------------------------------------------------- clip members --

fn clip_base() -> Layer {
    shaped("ClipBase", 19, 13, (4, 4), BlendMode::Normal, 0.9)
}

fn member(json: &str) -> Layer {
    styled(
        shaped("Member", 17, 12, (12, 9), BlendMode::Normal, 1.0),
        json,
    )
    .with_clipped(true)
}

#[test]
fn a_knockout_member_punches_to_the_base_only_sample_under_every_base_at_u16() {
    for (label, json) in KNOCKOUTS {
        assert_regions_match_with(
            &doc(vec![base(), screen(), clip_base(), member(json)]),
            &format!("unstyled base {label}"),
            &around((12, 9), 17, 12, 0),
        );
        let blend_if_base = clip_base().with_blend_if(Some(blend_if())).unwrap();
        assert_regions_match(
            &doc(vec![base(), screen(), blend_if_base, member(json)]),
            &format!("typed blend if base {label}"),
        );
        for (base_label, base_style) in [
            ("fill", "{\"fill_opacity\":0.6}".to_string()),
            (
                "effects",
                fx(&[
                    "{\"type\":\"stroke\",\"size\":2,\"color\":\"#ffffff\"}",
                    "{\"type\":\"drop_shadow\",\"distance\":3,\"size\":2}",
                ]),
            ),
        ] {
            for opacity in [1.0, 0.6] {
                let b = styled(
                    shaped("ClipBase", 19, 13, (4, 4), BlendMode::Normal, opacity),
                    &base_style,
                );
                assert_regions_match(
                    &doc(vec![base(), screen(), b, member(json)]),
                    &format!("{base_label} base {opacity} {label}"),
                );
            }
        }
        let masked_base = clip_base().with_mask(Some(mask(16, 12, (5, 5), 0.9, 6.0)));
        assert_regions_match(
            &doc(vec![base(), screen(), masked_base, member(json)]),
            &format!("masked base {label}"),
        );
    }
}

#[test]
fn a_second_knockout_member_ignores_what_the_first_member_drew_at_u16() {
    for (label, json) in KNOCKOUTS {
        let layers = vec![
            base(),
            screen(),
            shaped("ClipBase", 19, 13, (4, 4), BlendMode::Normal, 1.0),
            translucent("First", 20, 14, (8, 6), BlendMode::Multiply, 1.0).with_clipped(true),
            styled(
                shaped("Second", 17, 12, (10, 5), BlendMode::Normal, 1.0),
                json,
            )
            .with_clipped(true),
        ];
        assert_regions_match(&doc(layers), &format!("second member {label}"));
    }
}

#[test]
fn a_knockout_group_member_and_an_adjustment_base_match_at_u16() {
    let children = || {
        vec![
            shaped("A", 17, 12, (12, 9), BlendMode::Normal, 1.0),
            translucent("B", 9, 9, (14, 6), BlendMode::Screen, 0.8),
        ]
    };
    for (label, json) in KNOCKOUTS {
        let group = styled(
            Layer::isolated_normal("Member", children(), 1.0, true).unwrap(),
            json,
        )
        .with_clipped(true);
        assert_regions_match(
            &doc(vec![base(), screen(), clip_base(), group]),
            &format!("group member {label}"),
        );
    }
    let effects = "{\"fill_opacity\":0.5,\"knockout\":\"deep\",\"effects\":[\
                   {\"type\":\"stroke\",\"size\":2},{\"type\":\"color_overlay\",\"opacity\":0.5}]}";
    let group = styled(
        Layer::isolated_normal("Member", children(), 1.0, true).unwrap(),
        effects,
    )
    .with_clipped(true);
    assert_regions_match(
        &doc(vec![base(), screen(), clip_base(), group]),
        "materialized group member",
    );
    for (label, json) in KNOCKOUTS {
        let adjustment_base = adjustment("hue_saturation", json!({"hue":-60,"saturation":0.2}));
        assert_regions_match(
            &doc(vec![base(), screen(), adjustment_base, member(json)]),
            &format!("adjustment base {label}"),
        );
    }
}

#[test]
fn a_knockout_clip_base_with_visible_and_clamp_only_members_matches_at_u16() {
    for (label, json) in KNOCKOUTS {
        let multiply =
            translucent("Multiply", 15, 15, (14, 8), BlendMode::Multiply, 0.6).with_clipped(true);
        let zero = translucent("Zero", 9, 9, (2, 2), BlendMode::Normal, 0.0).with_clipped(true);
        let layers = vec![
            base(),
            screen(),
            top(json, BlendMode::Normal, 1.0),
            multiply,
            zero.clone(),
        ];
        assert_regions_match_with(
            &doc(layers),
            &format!("knockout base {label}"),
            &around((14, 8), 15, 15, 1),
        );
        assert_regions_match(
            &doc(vec![
                base(),
                screen(),
                top(json, BlendMode::Normal, 1.0),
                zero.clone(),
            ]),
            &format!("knockout base, clamp-only {label}"),
        );
        let package = styled(
            shaped("Top", 17, 12, TOP_AT, BlendMode::Normal, 0.7),
            &json.replace('}', ",\"effects\":[{\"type\":\"stroke\",\"size\":2}]}"),
        );
        assert_regions_match(
            &doc(vec![base(), screen(), package, zero]),
            &format!("knockout base package {label}"),
        );
    }
}

// ----------------------------------------------------------------- D6 --

const GROUPED: &str = "\"blend_interior_as_group\":true,\"fill_opacity\":0.5,\"effects\":[\
     {\"type\":\"color_overlay\",\"blend\":\"multiply\",\"color\":\"#3080c0\",\"opacity\":0.8},\
     {\"type\":\"gradient_overlay\",\"opacity\":0.6},\
     {\"type\":\"satin\",\"distance\":4,\"size\":3,\"color\":\"#602000\"},\
     {\"type\":\"inner_glow\",\"size\":3},\
     {\"type\":\"inner_shadow\",\"distance\":2,\"size\":2},\
     {\"type\":\"stroke\",\"size\":2,\"position\":\"inside\",\"color\":\"#20ff00\"},\
     {\"type\":\"bevel_emboss\",\"size\":3,\"depth\":2}]";

#[test]
fn grouped_interiors_on_a_lone_raster_match_at_u16() {
    for (mode, opacity) in [
        (BlendMode::Multiply, 1.0),
        (BlendMode::Normal, 1.0),
        (BlendMode::Multiply, 0.6),
    ] {
        let d = over_background(&format!("{{{GROUPED}}}"), mode, opacity);
        assert_regions_match_with(
            &d,
            &format!("D6 {mode:?} {opacity}"),
            &around(TOP_AT, 17, 12, 3),
        );
        let d = over_background(
            &format!("{{\"knockout\":\"deep\",{GROUPED}}}"),
            mode,
            opacity,
        );
        assert_regions_match(&d, &format!("D6 + deep {mode:?} {opacity}"));
    }
}

#[test]
fn grouped_interiors_on_bases_members_and_groups_match_at_u16() {
    let grouped = format!("{{{GROUPED}}}");
    let d6_base = || top(&grouped, BlendMode::Multiply, 1.0);
    let multiply =
        || translucent("Multiply", 15, 15, (14, 8), BlendMode::Multiply, 0.6).with_clipped(true);
    assert_regions_match(
        &doc(vec![base(), screen(), d6_base(), multiply()]),
        "D6 base with a visible member",
    );
    let hidden = Layer::raster("Hidden", shape16(15, 15, 9), (14, 8), 0.6, false)
        .unwrap()
        .with_clipped(true);
    assert_regions_match(
        &doc(vec![base(), screen(), d6_base(), hidden]),
        "D6 base with hidden-only members",
    );
    let d6_member = styled(
        translucent("Member", 15, 15, (14, 8), BlendMode::Multiply, 0.6),
        &grouped,
    )
    .with_clipped(true);
    assert_regions_match(
        &doc(vec![base(), screen(), d6_base(), d6_member]),
        "D6 on a member",
    );
    let pair = || {
        vec![
            shaped("A", 17, 12, TOP_AT, BlendMode::Normal, 1.0),
            translucent("B", 13, 10, (-4, 15), BlendMode::Screen, 0.8),
        ]
    };
    let group = |json: &str| {
        styled(
            Layer::isolated_normal("Group", pair(), 1.0, true)
                .unwrap()
                .with_blend_mode(BlendMode::Multiply)
                .unwrap(),
            json,
        )
    };
    assert_regions_match(&doc(vec![base(), group(&grouped)]), "D6 group");
    assert_regions_match(
        &doc(vec![
            base(),
            group(&format!("{{\"knockout\":\"shallow\",{GROUPED}}}")),
        ]),
        "D6 + shallow group",
    );
}

// ------------------------------------------------------------------ plan --

#[test]
fn the_materialized_subtree_scratch_carries_collects_knockout_term() {
    // Inside a materialized group: a raster child `a` (restyled below) and a
    // clip unit (base `b` + member `m`). Each document differs from `plain`
    // in ONE style only, so each difference is that row's `style_scratch`
    // difference (none: all are pad-0, effect-less) plus `knockout_scratch`.
    let canvas = u64::from(W) * u64::from(H);
    let fill = "{\"fill_opacity\":0.5}";
    let build = |a: &str, b: &str, m: &str| {
        let children = vec![
            styled(shaped("a", 13, 9, (4, 4), BlendMode::Normal, 1.0), a),
            styled(shaped("b", 13, 9, (20, 12), BlendMode::Normal, 1.0), b),
            styled(translucent("m", 9, 9, (22, 13), BlendMode::Normal, 1.0), m).with_clipped(true),
        ];
        doc(vec![
            base(),
            styled(
                Layer::isolated_normal("Styled", children, 1.0, true).unwrap(),
                &fx(&["{\"type\":\"stroke\",\"size\":3}"]),
            ),
        ])
    };
    let window = (4, 3, 9, 7);
    let scratch = |d: &PrecisionDocument| -> u64 {
        plan(d, window, 1).unwrap()["scratch_heap_bytes"]
            .as_u64()
            .unwrap()
    };
    let plain = build(fill, fill, fill);
    let reference = scratch(&plain);
    let cases = [
        (
            "a Deep child: the routed unit buffer",
            build("{\"fill_opacity\":0.5,\"knockout\":\"deep\"}", fill, fill),
            16 * canvas,
        ),
        (
            "a grouped child: the unit buffer and the alpha record",
            build(
                "{\"fill_opacity\":0.5,\"blend_interior_as_group\":true}",
                fill,
                fill,
            ),
            20 * canvas,
        ),
        (
            "a grouped clip base with a visible member (P7-39): the unit buffer and the alpha record",
            build(
                fill,
                "{\"fill_opacity\":0.5,\"blend_interior_as_group\":true}",
                fill,
            ),
            20 * canvas,
        ),
        (
            "a Shallow member: its unit buffer and the base's snapshot",
            build(
                fill,
                fill,
                "{\"fill_opacity\":0.5,\"knockout\":\"shallow\"}",
            ),
            32 * canvas,
        ),
    ];
    let rows = |d: &PrecisionDocument| -> Vec<(u64, Option<u64>, Option<u64>)> {
        plan(d, window, 1).unwrap()["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| {
                (
                    l["index"].as_u64().unwrap(),
                    l["clip_base"].as_u64(),
                    l["group"].as_u64(),
                )
            })
            .collect()
    };
    for (label, d, term) in cases {
        assert_eq!(scratch(&d), reference + term, "{label}");
        assert_eq!(rows(&d), rows(&plain), "{label}: the rows keep their shape");
        assert_regions_match(&d, label);
    }
    // Outside a materialized subtree the regional path allocates nothing
    // for a knockout: no term.
    let lone = over_background(KNOCKOUTS[0].1, BlendMode::Normal, 1.0);
    let unstyled = doc(vec![
        base(),
        screen(),
        shaped("Top", 17, 12, TOP_AT, BlendMode::Normal, 1.0),
    ]);
    assert_eq!(scratch(&lone), scratch(&unstyled));
    let p = plan(&lone, window, 1).unwrap();
    assert_eq!(p["contract_version"], 1);
    assert_eq!(p["layers"][2]["style"], "fill");
}

// --------------------------------------------------------------- policy --

fn edit(doc: &PrecisionDocument, request: Value) -> Result<PrecisionDocument, String> {
    let text = CString::new(request.to_string()).unwrap();
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
    if out.is_null() {
        return Err(take(err));
    }
    Ok(*unsafe { Box::from_raw(out) })
}

#[test]
fn a_float_knockout_document_renders_regionally_as_in_full() {
    // A float document holds knockout and Blend Interior Effects
    // as Group (Photoshop 26.10.0 matched the shipped typed rule at 32 bits),
    // so the float regional render meets the routed unit pass — at the root,
    // where D5's Background is record 0, and inside an isolated group — and
    // equals the full render.
    let pair = crate::float_fixture::shaped_pair(W, H);
    let grouped = edit(
        &pair,
        json!({"op":"group_layers","layers":[1],"name":"G","pass_through":false}),
    )
    .expect("grouped");
    let multiplied = edit(
        &pair,
        json!({"op":"layer_blend","version":1,"layer":1,"blend_mode":1}),
    )
    .expect("multiply");
    for (doc, style, what) in [
        (
            &pair,
            json!({"knockout":"deep","fill_opacity":0.25}),
            "root deep",
        ),
        (
            &pair,
            json!({"knockout":"shallow","fill_opacity":0.5,"effects":[
                {"type":"drop_shadow","blend":"multiply","distance":2,"size":1,
                 "use_global_light":false,"angle":45}]}),
            "root shallow",
        ),
        (
            &multiplied,
            json!({"blend_interior_as_group":true,"effects":[
                {"type":"color_overlay","color":"#ff8000","opacity":0.5}]}),
            "blend interior",
        ),
        (
            &grouped,
            json!({"knockout":"deep","fill_opacity":0.0}),
            "group deep",
        ),
    ] {
        let d = edit(
            doc,
            json!({"op":"layer_style","version":1,"layer":1,"style":style}),
        )
        .expect("a float document holds the knockout");
        assert!(d.is_float());
        assert_regions_match(&d, what);
    }
    if !std::path::Path::new(FLOAT_FIXTURE).exists() {
        return;
    }
    let fixture = PrecisionDocument::open_preserving(FLOAT_FIXTURE, LIMIT, 0, None).unwrap();
    assert_regions_match(&fixture, "the float fixture");
}

#[test]
fn a_u16_knockout_document_refuses_linear_light() {
    let d = over_background(KNOCKOUTS[0].1, BlendMode::Normal, 1.0);
    assert!(d.with_evaluation(Evaluation::LinearLight).is_err());
    let d = over_background(&format!("{{{GROUPED}}}"), BlendMode::Normal, 1.0);
    assert!(d.with_evaluation(Evaluation::LinearLight).is_err());
}

#[test]
fn a_pre_cancelled_u16_knockout_render_returns_no_output() {
    let d = over_background(KNOCKOUTS[0].1, BlendMode::Normal, 0.7);
    let control = unsafe { dr_job_control_create() };
    unsafe { dr_job_control_cancel(control) };
    let mut err: *mut c_char = ptr::null_mut();
    let out =
        unsafe { dr_precision_doc_render_region(&d, 0, 0, W, H, 1, LIMIT, 0, control, &mut err) };
    assert!(out.is_null());
    assert_eq!(take(err), "operation cancelled");
    assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
    unsafe { dr_job_control_free(control) };
}
