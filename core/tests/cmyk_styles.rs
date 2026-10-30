//! Layer effects on a CMYK document's ink (#547, the styles batch): every
//! effect at Normal 100 and 50, Multiply and Screen against the coverage its
//! RGB16 twin renders (`common/cmyk_style_fixture.rs`) meeting the ink
//! through the f64 W3C step on `L = 1 − ink`
//! (`common/cmyk_structure_fixture.rs`'s `over`, never the implementation);
//! the gradient overlay's ink interpolated at the twin's parameter; the
//! layer-opacity package, Fill, the inside band, a styled group and a styled
//! clip base by their closed forms; a slot with no ink through the
//! document's profile; an 8-bit document's ink on its grid; the canonical
//! hex. Bound: 2 16-bit codes a plate unless stated.

use std::ffi::CStr;
use std::ptr;

use doctored_core::doc::BlendMode;
use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_cmyk::dr_cmyk_color;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{free, then, Ink};
use crate::cmyk_style_fixture::{
    assert_flat_all, backdrop, flat, hard_rect, layer, mix, over_to, percent, soft_rect, styled,
    twin, twin_effect,
};

const W: u32 = 48;
const H: u32 = 40;
const COLUMNS: [[f64; 4]; 4] = [
    [0.0, 0.0, 0.0, 0.0],
    [50.0, 0.0, 0.0, 0.0],
    [20.0, 40.0, 60.0, 10.0],
    [0.0, 0.0, 0.0, 50.0],
];
const SOURCE: [f64; 4] = [10.0, 60.0, 30.0, 0.0];
const E1: [f64; 4] = [75.0, 68.0, 67.0, 90.0];
const E2: [f64; 4] = [100.0, 0.0, 0.0, 0.0];
const RECT: (u32, u32, u32, u32) = (12, 10, 36, 30);

fn column(x: u32) -> Ink {
    let c = percent(COLUMNS[(x as usize * COLUMNS.len()) / W as usize]);
    [c[0], c[1], c[2], c[3], 1.0]
}

fn fractions(ink: [f64; 4]) -> Value {
    json!(percent(ink))
}

/// The backdrop and one "Shape" layer of SOURCE at `coverage`.
fn shaped(coverage: &[u16]) -> *mut PrecisionDocument {
    layer(backdrop(W, H, 16, &COLUMNS), "Shape", SOURCE, coverage)
}

fn mode_of(name: &str) -> BlendMode {
    match name {
        "normal" => BlendMode::Normal,
        "multiply" => BlendMode::Multiply,
        "screen" => BlendMode::Screen,
        other => panic!("{other}"),
    }
}

/// The effects under test, each with whether it composites Below the
/// layer's pixels.
fn effects() -> Vec<(&'static str, Value, bool)> {
    vec![
        (
            "drop_shadow",
            json!({"type":"drop_shadow","angle":120,"use_global_light":false,"distance":5,"spread":0.2,"size":4}),
            true,
        ),
        (
            "outer_glow",
            json!({"type":"outer_glow","spread":0.3,"size":5}),
            true,
        ),
        (
            "stroke",
            json!({"type":"stroke","size":3,"position":"outside"}),
            true,
        ),
        (
            "inner_shadow",
            json!({"type":"inner_shadow","angle":45,"use_global_light":false,"distance":4,"choke":0,"size":3}),
            false,
        ),
        (
            "inner_glow",
            json!({"type":"inner_glow","choke":0.2,"size":4}),
            false,
        ),
        (
            "inner_glow_center",
            json!({"type":"inner_glow","source":"center","choke":0.5,"size":4,"range":0.75}),
            false,
        ),
        (
            "inner_glow_hard",
            json!({"type":"inner_glow","choke":1,"size":4}),
            false,
        ),
        (
            "inner_glow_hard_center",
            json!({"type":"inner_glow","source":"center","choke":1,"size":4,"range":1}),
            false,
        ),
        ("color_overlay", json!({"type":"color_overlay"}), false),
        (
            "satin",
            json!({"type":"satin","angle":19,"distance":6,"size":5}),
            false,
        ),
    ]
}

const VARIANTS: [(&str, f64); 4] = [
    ("normal", 1.0),
    ("normal", 0.5),
    ("multiply", 1.0),
    ("screen", 1.0),
];

#[test]
fn every_effect_meets_its_coverage_on_one_minus_ink() {
    let coverage = soft_rect(W, H, RECT);
    for (name, base, below) in effects() {
        let cov = twin(W, H, &coverage, &twin_effect(&base, None), (120.0, 30.0));
        assert!(
            cov.iter().any(|&v| v > 0.2),
            "{name}: the twin draws nothing"
        );
        for (ink, label) in [(E1, "E1"), (E2, "E2")] {
            for (mode, opacity) in VARIANTS {
                let mut effect = base.clone();
                effect["ink"] = fractions(ink);
                effect["blend"] = json!(mode);
                effect["opacity"] = json!(opacity);
                let doc = styled(shaped(&coverage), 1, &json!({ "effects": [effect] }));
                let e = percent(ink);
                let src = percent(SOURCE);
                assert_flat_all(
                    doc,
                    2,
                    &format!("{name} {label} {mode} {opacity}"),
                    |x, y| {
                        let i = (y * W + x) as usize;
                        let a = f64::from(coverage[i]) / 65535.0;
                        let mut sa = cov[i] * opacity;
                        // An outside stroke band lands under the layer as
                        // the disjoint union (#578): at `sa / (1 − a)` under
                        // a pixel the layer covers at `a` (Fill 1); so does
                        // an outer glow, whose plane is `g (1 − a)` (#578,
                        // the measured composite `g (1 − a)`), and since #578
                        // part 4 (DK1) a knocked-out drop shadow, whose twin
                        // already reads `m (1 − a)`.
                        let under = matches!(name, "stroke" | "outer_glow" | "drop_shadow");
                        if under && a > 0.0 && a < 1.0 {
                            sa = (sa / (1.0 - a)).min(1.0);
                        }
                        if below {
                            let acc = over_to(column(x), e, sa, mode_of(mode));
                            over_to(acc, src, a, BlendMode::Normal)
                        } else {
                            let acc = over_to(column(x), src, a, BlendMode::Normal);
                            over_to(acc, e, sa, mode_of(mode))
                        }
                    },
                );
                free(doc);
            }
        }
    }
}

#[test]
fn the_bevel_lights_each_side_with_its_own_ink() {
    // The second base is overhead and softened: the Smooth law's NORMALIZED
    // shading (#578, `style_bevel_smooth`) shades every slope and lights none
    // at altitude 90. The Emboss and Pillow Emboss bases (the law at half the
    // Size, #578) draw a Below half and an Interior half; on a hard edge
    // every pixel is in one band, so the oracle's one pass over the layer is
    // the kernel's order there.
    let bases = [
        (
            json!({"type":"bevel_emboss","size":5,"soften":0,"angle":120,"use_global_light":false,"altitude":30}),
            soft_rect(W, H, RECT),
        ),
        (
            json!({"type":"bevel_emboss","size":5,"soften":2,"angle":120,"use_global_light":false,"altitude":90}),
            soft_rect(W, H, RECT),
        ),
        (
            json!({"type":"bevel_emboss","style":"emboss","size":5,"soften":0,"angle":120,"use_global_light":false,"altitude":30}),
            hard_rect(W, H, RECT),
        ),
        (
            json!({"type":"bevel_emboss","style":"pillow_emboss","size":5,"soften":0,"angle":120,"use_global_light":false,"altitude":30}),
            hard_rect(W, H, RECT),
        ),
    ];
    for (base, coverage) in bases {
        let overhead = base["altitude"] == 90;
        let light = (
            base["angle"].as_f64().expect("angle") as f32,
            base["altitude"].as_f64().expect("altitude") as f32,
        );
        let lit = twin(
            W,
            H,
            &coverage,
            &twin_effect(&base, Some("highlight")),
            light,
        );
        let shaded = twin(W, H, &coverage, &twin_effect(&base, Some("shadow")), light);
        let peak = |v: &[f64]| v.iter().copied().fold(0.0, f64::max);
        if overhead {
            assert!(
                lit.iter().all(|&v| v == 0.0) && peak(&shaded) > 0.1,
                "the overhead twin: highlight peak {} (want 0), shadow peak {}",
                peak(&lit),
                peak(&shaded)
            );
        } else {
            assert!(
                peak(&lit) > 0.1 && peak(&shaded) > 0.1,
                "the twin lights nothing: {} / {}",
                peak(&lit),
                peak(&shaded)
            );
        }
        for (hmode, smode) in [("screen", "multiply"), ("normal", "normal")] {
            let mut effect = base.clone();
            effect["highlight_ink"] = fractions(E2);
            effect["shadow_ink"] = fractions(E1);
            effect["highlight_blend"] = json!(hmode);
            effect["shadow_blend"] = json!(smode);
            effect["highlight_opacity"] = json!(0.8);
            effect["shadow_opacity"] = json!(0.6);
            let doc = styled(shaped(&coverage), 1, &json!({ "effects": [effect] }));
            let what = format!(
                "bevel {} {hmode}/{smode} at altitude {}",
                base["style"], base["altitude"]
            );
            assert_flat_all(doc, 2, &what, |x, y| {
                let i = (y * W + x) as usize;
                let a = f64::from(coverage[i]) / 65535.0;
                let acc = over_to(column(x), percent(SOURCE), a, BlendMode::Normal);
                let acc = over_to(acc, percent(E2), lit[i] * 0.8, mode_of(hmode));
                over_to(acc, percent(E1), shaded[i] * 0.6, mode_of(smode))
            });
            free(doc);
        }
    }
}

#[test]
fn a_gradient_overlay_interpolates_its_stops_ink() {
    let coverage = soft_rect(W, H, RECT);
    let base = json!({"type":"gradient_overlay","gradient":{"stops":[
        {"position":0,"color":"#000000","opacity":1},{"position":1,"color":"#ffffff","opacity":1}],
        "style":"linear","angle":0}});
    let t = twin(W, H, &coverage, &twin_effect(&base, None), (120.0, 30.0));
    let cov = twin(
        W,
        H,
        &coverage,
        &json!({"type":"color_overlay","color":"#ffffff"}),
        (120.0, 30.0),
    );
    for (mode, opacity) in VARIANTS {
        let mut effect = base.clone();
        effect["gradient"]["stops"][0]["ink"] = fractions(E1);
        effect["gradient"]["stops"][1]["ink"] = fractions(E2);
        effect["blend"] = json!(mode);
        effect["opacity"] = json!(opacity);
        let doc = styled(shaped(&coverage), 1, &json!({ "effects": [effect] }));
        let (a1, a2) = (percent(E1), percent(E2));
        assert_flat_all(doc, 2, &format!("gradient {mode} {opacity}"), |x, y| {
            let i = (y * W + x) as usize;
            let a = f64::from(coverage[i]) / 65535.0;
            // `t` is the twin's black → white parameter; where the shape is
            // partial the twin's red is `t × coverage`.
            let tt = if cov[i] > 0.0 {
                (t[i] / cov[i]).min(1.0)
            } else {
                0.0
            };
            let ink: [f64; 4] = std::array::from_fn(|c| a1[c] + (a2[c] - a1[c]) * tt);
            let acc = over_to(column(x), percent(SOURCE), a, BlendMode::Normal);
            over_to(acc, ink, cov[i] * opacity, mode_of(mode))
        });
        free(doc);
    }
}

#[test]
fn layer_opacity_packages_the_layer_with_its_effects() {
    let coverage = soft_rect(W, H, RECT);
    let base = json!({"type":"drop_shadow","angle":120,"use_global_light":false,"distance":5,"spread":0.2,"size":4});
    let cov = twin(W, H, &coverage, &twin_effect(&base, None), (120.0, 30.0));
    let mut effect = base.clone();
    effect["ink"] = fractions(E1);
    effect["blend"] = json!("multiply");
    effect["opacity"] = json!(1.0);
    let doc = styled(shaped(&coverage), 1, &json!({ "effects": [effect] }));
    let doc = crate::cmyk_structure_fixture::with_opacity(doc, 1, 0.5);
    assert_flat_all(doc, 2, "package", |x, y| {
        let i = (y * W + x) as usize;
        let a = f64::from(coverage[i]) / 65535.0;
        // The knocked-out shadow lands under the layer (#578 part 4, DK1).
        let sa = if a > 0.0 && a < 1.0 {
            (cov[i] / (1.0 - a)).min(1.0)
        } else {
            cov[i]
        };
        let packaged = over_to(
            over_to(column(x), percent(E1), sa, BlendMode::Multiply),
            percent(SOURCE),
            a,
            BlendMode::Normal,
        );
        // The byte opacity (#578): 50 % composites at 128 / 255.
        mix(
            column(x),
            packaged,
            crate::cmyk_structure_fixture::byte_unit(0.5),
        )
    });
    free(doc);
}

#[test]
fn fill_opacity_scales_the_pixels_never_the_effects() {
    let coverage = soft_rect(W, H, RECT);
    let cov = twin(
        W,
        H,
        &coverage,
        &json!({"type":"color_overlay","color":"#ffffff"}),
        (120.0, 30.0),
    );
    let style = json!({"fill_opacity":0.3,"effects":[{"type":"color_overlay","ink":fractions(E2),"opacity":0.6}]});
    let doc = styled(shaped(&coverage), 1, &style);
    assert_flat_all(doc, 2, "fill", |x, y| {
        let i = (y * W + x) as usize;
        let a = f64::from(coverage[i]) / 65535.0;
        // The byte Fill (#578): 30 % composites at 77 / 255; the
        // effect's own opacity is not a byte.
        let fill = crate::cmyk_structure_fixture::byte_unit(0.3);
        let acc = over_to(column(x), percent(SOURCE), a * fill, BlendMode::Normal);
        over_to(acc, percent(E2), cov[i] * 0.6, BlendMode::Normal)
    });
    free(doc);
}

#[test]
fn an_inside_stroke_replaces_what_lies_under_it() {
    let coverage = hard_rect(W, H, RECT);
    let base = json!({"type":"stroke","size":3,"position":"inside"});
    let band = twin(W, H, &coverage, &twin_effect(&base, None), (120.0, 30.0));
    let mut effect = base.clone();
    effect["ink"] = fractions(E1);
    let doc = styled(shaped(&coverage), 1, &json!({ "effects": [effect] }));
    assert_flat_all(doc, 2, "inside band", |x, y| {
        let i = (y * W + x) as usize;
        let a = f64::from(coverage[i]) / 65535.0;
        let pixels = over_to(column(x), percent(SOURCE), a, BlendMode::Normal);
        if a == 0.0 {
            return pixels;
        }
        // `s = band / shape` with the shape full: the twin's red.
        let replaced = over_to(column(x), percent(E1), 1.0, BlendMode::Normal);
        mix(pixels, replaced, band[i])
    });
    free(doc);
}

#[test]
fn an_anti_aliased_inside_stroke_takes_the_anti_aliased_band_in_ink() {
    // #578: on a shape with partial pixels the Inside band is measured from
    // the anti-aliased edge (NA1, `inside_corner_band`), the one u8 kernel every
    // depth and CMYK share, so the twin's red (Fill 0 over black, a white
    // stroke: exactly the band) is this document's band too.
    let coverage = soft_rect(W, H, RECT);
    let base = json!({"type":"stroke","size":3,"position":"inside"});
    let band = twin(W, H, &coverage, &twin_effect(&base, None), (120.0, 30.0));
    let mut effect = base.clone();
    effect["ink"] = fractions(E1);
    let doc = styled(shaped(&coverage), 1, &json!({ "effects": [effect] }));
    assert_flat_all(doc, 2, "anti-aliased inside band", |x, y| {
        let i = (y * W + x) as usize;
        let a = f64::from(coverage[i]) / 65535.0;
        let pixels = over_to(column(x), percent(SOURCE), a, BlendMode::Normal);
        if a == 0.0 {
            return pixels;
        }
        // `s = band / shape` with the shape at its byte, and the
        // stroke over the stack beneath at that shape (`o · c`).
        let shape = (a * 255.0).round() / 255.0;
        let replaced = over_to(column(x), percent(E1), shape, BlendMode::Normal);
        mix(pixels, replaced, (band[i] / shape).min(1.0))
    });
    free(doc);
}

#[test]
fn a_styled_group_is_its_styled_child() {
    let coverage = soft_rect(W, H, RECT);
    let style = json!({"effects":[
        {"type":"drop_shadow","angle":120,"use_global_light":false,"distance":5,"size":4,"ink":fractions(E1),"blend":"multiply","opacity":0.75},
        {"type":"color_overlay","ink":fractions(E2),"blend":"screen","opacity":0.5}]});
    let raster = styled(shaped(&coverage), 1, &style);
    let grouped = then(
        shaped(&coverage),
        r#"{"op":"group_layers","layers":[1],"name":"G","pass_through":false}"#,
        &[],
    );
    let group = unsafe { &*grouped }.layer_count() - 1;
    let grouped = styled(grouped, group, &style);
    let (a, b) = (flat(raster), flat(grouped));
    for (i, (p, q)) in a.iter().zip(&b).enumerate() {
        for c in 0..4 {
            assert!(
                p[c].abs_diff(q[c]) <= 2,
                "pixel {i} plate {c}: raster {p:?} group {q:?}"
            );
        }
    }
    free(raster);
    free(grouped);
}

#[test]
fn a_styled_clip_base_confines_its_member_and_overlays_both() {
    let coverage = hard_rect(W, H, RECT);
    let member_cover = hard_rect(W, H, (24, 4, 44, 36));
    let member_ink = [0.0, 0.0, 80.0, 10.0];
    let style = json!({"effects":[{"type":"color_overlay","ink":fractions(E2),"opacity":0.4}]});
    let doc = styled(shaped(&coverage), 1, &style);
    let doc = layer(doc, "Member", member_ink, &member_cover);
    let doc = then(
        doc,
        r#"{"op":"layer_clipped","layer":2,"clipped":true}"#,
        &[],
    );
    assert_flat_all(doc, 2, "clip base", |x, y| {
        let i = (y * W + x) as usize;
        let a = f64::from(coverage[i]) / 65535.0;
        let m = f64::from(member_cover[i]) / 65535.0;
        let mut unit = over_to([0.0; 5], percent(SOURCE), a, BlendMode::Normal);
        unit = over_to(unit, percent(member_ink), m, BlendMode::Normal);
        unit[4] = unit[4].min(a);
        let acc = over_to(
            column(x),
            [unit[0], unit[1], unit[2], unit[3]],
            unit[4],
            BlendMode::Normal,
        );
        over_to(acc, percent(E2), a * 0.4, BlendMode::Normal)
    });
    free(doc);
}

fn cmyk_colour(doc: *const PrecisionDocument, request: Value) -> Value {
    let text = std::ffi::CString::new(request.to_string()).unwrap();
    let mut err = ptr::null_mut();
    let out = unsafe { dr_cmyk_color(doc, text.as_ptr(), &mut err) };
    assert!(!out.is_null(), "{}", crate::common::take_err_string(err));
    let value: Value =
        serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
    unsafe { dr_string_free(out) };
    value
}

#[test]
fn a_slot_with_no_ink_converts_its_hex_through_the_profile() {
    let coverage = hard_rect(W, H, RECT);
    let doc = styled(
        shaped(&coverage),
        1,
        &json!({"effects":[{"type":"color_overlay","color":"#3366cc"}]}),
    );
    let ink = cmyk_colour(doc, json!({"from":"srgb","values":[0.2, 0.4, 0.8]}))["ink"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect::<Vec<_>>();
    let ink = [ink[0], ink[1], ink[2], ink[3]];
    assert_flat_all(doc, 1, "no ink", |x, y| {
        let i = (y * W + x) as usize;
        if coverage[i] == 0 {
            column(x)
        } else {
            [ink[0], ink[1], ink[2], ink[3], 1.0]
        }
    });
    free(doc);
}

#[test]
fn an_eight_bit_document_holds_an_effect_ink_on_its_grid() {
    // 90 % is code 229 in Photoshop's 8-bit document (take1): round(255·ink).
    let coverage = hard_rect(W, H, RECT);
    let doc = layer(backdrop(W, H, 8, &COLUMNS), "Shape", SOURCE, &coverage);
    let doc = styled(
        doc,
        1,
        &json!({"effects":[{"type":"color_overlay","ink":fractions(E1)}]}),
    );
    let samples = flat(doc);
    let inside = samples[(20 * W + 20) as usize];
    assert_eq!(
        [inside[0], inside[1], inside[2], inside[3]],
        [191 * 257, 173 * 257, 171 * 257, 229 * 257],
        "E1 on the 8-bit grid"
    );
    free(doc);
}

#[test]
fn the_stored_hex_is_the_inks_srgb_under_the_profile() {
    let coverage = hard_rect(W, H, RECT);
    let doc = styled(
        shaped(&coverage),
        1,
        &json!({"effects":[{"type":"satin","color":"#ff0000","ink":fractions(E1)}]}),
    );
    let reported = unsafe { &*doc }.layer(1).unwrap().reported_style().unwrap();
    let style: Value = serde_json::from_str(&reported).unwrap();
    let satin = &style["effects"][0];
    assert_eq!(satin["ink"], json!([0.75, 0.68, 0.67, 0.9]));
    let srgb = cmyk_colour(doc, json!({"from":"ink","values":percent(E1)}))["srgb"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v.as_f64().unwrap() * 255.0).round() as u8)
        .collect::<Vec<_>>();
    assert_eq!(
        satin["color"],
        json!(format!("#{:02x}{:02x}{:02x}", srgb[0], srgb[1], srgb[2])),
        "the hex follows the ink"
    );
    // Setting the reported style again changes nothing.
    let again = crate::cmyk_style_fixture::edit(
        doc,
        &json!({"op":"layer_style","version":1,"layer":1,"style":style}),
    )
    .unwrap();
    assert!(again.is_null(), "an echoed canonical style is a no-op");
    free(doc);
}

#[test]
fn a_default_black_shadow_is_k100() {
    // take1 FITTED D-K: the default black in a CMYK document is K100.
    let coverage = hard_rect(W, H, RECT);
    let doc = styled(
        shaped(&coverage),
        1,
        &json!({"effects":[
            {"type":"drop_shadow"},
            {"type":"satin","ink":fractions(E2)},
            {"type":"outer_glow","color":"#000000"}]}),
    );
    let reported = unsafe { &*doc }.layer(1).unwrap().reported_style().unwrap();
    let style: Value = serde_json::from_str(&reported).unwrap();
    let of = |kind: &str| {
        style["effects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["type"] == kind)
            .unwrap()
            .clone()
    };
    assert_eq!(of("drop_shadow")["ink"], json!([0.0, 0.0, 0.0, 1.0]));
    assert_eq!(of("satin")["ink"], json!([1.0, 0.0, 0.0, 0.0]));
    assert!(
        of("outer_glow").get("ink").is_none(),
        "only the black-default slots"
    );
    free(doc);
}

// ------------------------------------------------- byte documents (#547) --

/// A byte document's style JSON as a value, `blend_if` read as absent when
/// null.
fn byte_style(doc: *const doctored_core::doc::DrDocument, idx: usize) -> Value {
    serde_json::from_str(&crate::common::ffi_style(doc, idx).expect("a style")).unwrap()
}

fn byte_save(doc: *const doctored_core::doc::DrDocument, path: &std::path::Path) -> Vec<u8> {
    use doctored_core::ffi_doc::dr_doc_save_native;
    let c = crate::common::cpath(path);
    let mut err = ptr::null_mut();
    let ok = unsafe { dr_doc_save_native(doc, c.as_ptr(), &mut err) };
    assert!(ok, "{}", crate::common::take_err_string(err));
    std::fs::read(path).unwrap()
}

#[test]
fn a_byte_document_refuses_a_plate_channel_and_drops_every_ink() {
    use crate::common::{add_layer, doc_from, ffi_style, set_style, solid, style_error};
    use doctored_core::ffi_doc::dr_doc_free;
    let dir = tempfile::TempDir::new().unwrap();
    let doc = doc_from(&dir, "bg.png", &solid(8, 6, [255, 255, 255, 255]));
    let doc = add_layer(
        &dir,
        "top.png",
        doc,
        0,
        &solid(4, 3, [255, 0, 0, 255]),
        "Top",
    );
    for plate in ["cyan", "magenta", "yellow", "black"] {
        let style = json!({"blend_if":{"channel":plate,
            "this_layer":[0,0,255,255],"underlying":[0,0,200,220]}});
        let error = style_error(doc, 1, &style.to_string()).expect("a named refusal");
        assert!(error.ends_with("needs a CMYK document"), "{plate}: {error}");
    }
    let doc = set_style(
        doc,
        1,
        &json!({"effects":[{"type":"color_overlay","color":"#00FF00","ink":[0.5,0.0,0.0,0.0]},
            {"type":"drop_shadow","ink":[0.0,0.0,0.0,1.0]}]})
        .to_string(),
    );
    let stored = ffi_style(doc, 1).unwrap();
    assert!(!stored.contains("ink"), "every ink dropped: {stored}");
    assert_eq!(byte_style(doc, 1)["effects"].as_array().unwrap().len(), 2);
    unsafe { dr_doc_free(doc) };
}

#[test]
fn a_byte_file_or_comp_holding_a_plate_channel_drops_it() {
    use crate::common::{add_layer, cpath, doc_from, set_style, solid, take_err_string};
    use doctored_core::ffi_comps::dr_doc_edit_layer_comps;
    use doctored_core::ffi_doc::{dr_doc_free, dr_doc_open};
    use doctored_core::ffi_style::dr_doc_set_layer_style;
    let dir = tempfile::TempDir::new().unwrap();
    let doc = doc_from(&dir, "bg.png", &solid(8, 6, [255, 255, 255, 255]));
    let doc = add_layer(
        &dir,
        "top.png",
        doc,
        0,
        &solid(4, 3, [255, 0, 0, 255]),
        "Top",
    );
    let doc = set_style(
        doc,
        1,
        &json!({"effects":[{"type":"drop_shadow"}],"blend_if":{"channel":"gray",
            "this_layer":[0,0,255,255],"underlying":[0,0,200,220]}})
        .to_string(),
    );
    // A comp records the style (LCMP) beside the layer's own (LSTY).
    let request = std::ffi::CString::new(
        json!({"op":"layer_comp","action":"save","name":"Styled"}).to_string(),
    )
    .unwrap();
    let mut err = ptr::null_mut();
    let comped = unsafe { dr_doc_edit_layer_comps(doc, request.as_ptr(), &mut err) };
    assert!(!comped.is_null(), "{}", take_err_string(err));
    unsafe { dr_doc_free(doc) };
    // A file this build's setter never writes: both records name the Cyan
    // plate (same length as "gray", no checksum to repair).
    let mut bytes = byte_save(comped, &dir.path().join("gray.drdc"));
    unsafe { dr_doc_free(comped) };
    let mut patched = 0;
    for at in 0..bytes.len().saturating_sub(4) {
        if &bytes[at..at + 5] == b"\"gray" {
            bytes[at + 1..at + 5].copy_from_slice(b"cyan");
            patched += 1;
        }
    }
    assert_eq!(patched, 2, "the layer's style and the comp's");
    let path = dir.path().join("cyan.drdc");
    std::fs::write(&path, &bytes).unwrap();
    let c = cpath(&path);
    let mut err = ptr::null_mut();
    let reopened = unsafe { dr_doc_open(c.as_ptr(), &mut err) };
    assert!(!reopened.is_null(), "{}", take_err_string(err));
    let style = byte_style(reopened, 1);
    assert!(style.get("blend_if").is_none_or(Value::is_null), "{style}");
    assert_eq!(style["effects"][0]["type"], "drop_shadow");
    // Clear the style, then apply the comp: its Cyan range is dropped too.
    let mut err = ptr::null_mut();
    let cleared = unsafe { dr_doc_set_layer_style(reopened, 1, ptr::null(), &mut err) };
    assert!(!cleared.is_null(), "{}", take_err_string(err));
    unsafe { dr_doc_free(reopened) };
    let apply =
        std::ffi::CString::new(json!({"op":"layer_comp","action":"apply","comp":0}).to_string())
            .unwrap();
    let mut err = ptr::null_mut();
    let applied = unsafe { dr_doc_edit_layer_comps(cleared, apply.as_ptr(), &mut err) };
    assert!(!applied.is_null(), "{}", take_err_string(err));
    unsafe { dr_doc_free(cleared) };
    let style = byte_style(applied, 1);
    assert!(style.get("blend_if").is_none_or(Value::is_null), "{style}");
    assert_eq!(style["effects"][0]["type"], "drop_shadow");
    unsafe { dr_doc_free(applied) };
}
