//! Knockout and Blend Interior Effects as Group on a CMYK document's ink
//! (#547, the styles batch): D3–D6 of `style_knockout` on ink, each case
//! against the D3 statement written out here — `K = over(B0, C, q, M)`,
//! `out = mix(acc, K, s·opacity)` on `L = 1 − ink` through the fixture's f64
//! W3C step (`common/cmyk_structure_fixture.rs`'s `over`, never the
//! implementation) — with the knockout backdrop `B0` of D4: transparent at an
//! isolated group's start, the accumulator as a Pass Through group's children
//! began, the Background (D5) for Deep or a root Shallow, transparent over a
//! bottom that does not qualify, and the clip buffer after the base alone for
//! a clipped member. D6's grouped interior on a raster and on a clip base.
//! Bound: 2 16-bit codes a plate.

use doctored_core::doc::BlendMode;
use doctored_core::precision_document::PrecisionDocument;
use serde_json::{json, Value};

use crate::cmyk_structure_fixture::{free, then, with_opacity, Ink};
use crate::cmyk_style_fixture::{
    assert_each, assert_flat_all, backdrop, hard_rect, layer, mix, over_to, percent, soft_rect,
    styled,
};

const W: u32 = 48;
const H: u32 = 40;
const COLUMNS: [[f64; 4]; 4] = [
    [0.0, 0.0, 0.0, 0.0],
    [50.0, 0.0, 0.0, 0.0],
    [20.0, 40.0, 60.0, 10.0],
    [0.0, 0.0, 0.0, 50.0],
];
/// The layer the hole cuts through.
const UNDER: [f64; 4] = [0.0, 70.0, 20.0, 5.0];
const UNDER_RECT: (u32, u32, u32, u32) = (4, 6, 44, 34);
/// The knockout layer's own ink.
const E1: [f64; 4] = [75.0, 68.0, 67.0, 90.0];
const RECT: (u32, u32, u32, u32) = (12, 10, 36, 30);
const TRANSPARENT: Ink = [0.0; 5];

fn column(x: u32) -> Ink {
    let c = percent(COLUMNS[(x as usize * COLUMNS.len()) / W as usize]);
    [c[0], c[1], c[2], c[3], 1.0]
}

fn unit(coverage: &[u16], x: u32, y: u32) -> f64 {
    f64::from(coverage[(y * W + x) as usize]) / 65535.0
}

/// The postorder index of the record named `name`.
fn index(doc: *const PrecisionDocument, name: &str) -> usize {
    let doc = unsafe { &*doc };
    (0..doc.layer_count())
        .find(|&i| doc.layer(i).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no record {name}"))
}

fn group(
    doc: *mut PrecisionDocument,
    names: &[&str],
    name: &str,
    pass: bool,
) -> *mut PrecisionDocument {
    let layers: Vec<String> = names.iter().map(|n| index(doc, n).to_string()).collect();
    then(
        doc,
        &format!(
            r#"{{"op":"group_layers","layers":[{}],"name":"{name}","pass_through":{pass}}}"#,
            layers.join(",")
        ),
        &[],
    )
}

fn style_named(doc: *mut PrecisionDocument, name: &str, style: &Value) -> *mut PrecisionDocument {
    let i = index(doc, name);
    styled(doc, i, style)
}

fn clip(doc: *mut PrecisionDocument, name: &str) -> *mut PrecisionDocument {
    let i = index(doc, name);
    then(
        doc,
        &format!(r#"{{"op":"layer_clipped","layer":{i},"clipped":true}}"#),
        &[],
    )
}

/// D3 on ink: the knocked-out layer of ink `c` at shape `s`, fill `q` (at
/// its byte, #578) and mode `m` over `acc`, its hole stopping at `b0`.
fn d3(acc: Ink, b0: Ink, c: [f64; 4], s: f64, q: f64, m: BlendMode) -> Ink {
    mix(acc, over_to(b0, c, crate::common::byte_unit(q), m), s)
}

/// The backdrop, an "Under" layer, and a soft-edged "Knock" layer of E1 at
/// Fill `fill` with `knockout`.
fn knocked(knockout: &str, fill: f64) -> (*mut PrecisionDocument, Vec<u16>, Vec<u16>) {
    let under = hard_rect(W, H, UNDER_RECT);
    let shape = soft_rect(W, H, RECT);
    let doc = layer(backdrop(W, H, 16, &COLUMNS), "Under", UNDER, &under);
    let doc = layer(doc, "Knock", E1, &shape);
    let doc = style_named(
        doc,
        "Knock",
        &json!({"knockout": knockout, "fill_opacity": fill}),
    );
    (doc, under, shape)
}

#[test]
fn shallow_in_an_isolated_group_stops_at_the_groups_transparent_start() {
    let (doc, under, shape) = knocked("shallow", 0.25);
    let doc = group(doc, &["Under", "Knock"], "G", false);
    assert_flat_all(doc, 2, "shallow in isolated", |x, y| {
        let inner = over_to(
            TRANSPARENT,
            percent(UNDER),
            unit(&under, x, y),
            BlendMode::Normal,
        );
        let inner = d3(
            inner,
            TRANSPARENT,
            percent(E1),
            unit(&shape, x, y),
            0.25,
            BlendMode::Normal,
        );
        over_to(
            column(x),
            [inner[0], inner[1], inner[2], inner[3]],
            inner[4],
            BlendMode::Normal,
        )
    });
    free(doc);
}

#[test]
fn deep_at_the_root_shows_a_qualifying_background() {
    for knockout in ["deep", "shallow"] {
        let (doc, under, shape) = knocked(knockout, 0.0);
        assert_flat_all(doc, 2, knockout, |x, y| {
            let acc = over_to(
                column(x),
                percent(UNDER),
                unit(&under, x, y),
                BlendMode::Normal,
            );
            d3(
                acc,
                column(x),
                percent(E1),
                unit(&shape, x, y),
                0.0,
                BlendMode::Normal,
            )
        });
        free(doc);
    }
}

#[test]
fn deep_over_a_bottom_that_does_not_qualify_is_transparent() {
    let (doc, under, shape) = knocked("deep", 0.5);
    // Opacity below 1 disqualifies the bottom as the Background (D5).
    let doc = with_opacity(doc, 0, 0.9);
    assert_each(doc, 2, "deep, no Background", |x, y| {
        let bottom = mix(TRANSPARENT, column(x), crate::common::byte_unit(0.9));
        let acc = over_to(
            bottom,
            percent(UNDER),
            unit(&under, x, y),
            BlendMode::Normal,
        );
        d3(
            acc,
            TRANSPARENT,
            percent(E1),
            unit(&shape, x, y),
            0.5,
            BlendMode::Normal,
        )
    });
    free(doc);
}

#[test]
fn shallow_in_pass_through_stops_where_the_children_began() {
    let lower = hard_rect(W, H, (0, 0, W, 20));
    let under = hard_rect(W, H, UNDER_RECT);
    let shape = soft_rect(W, H, RECT);
    let doc = layer(
        backdrop(W, H, 16, &COLUMNS),
        "Lower",
        [0.0, 0.0, 90.0, 0.0],
        &lower,
    );
    let doc = layer(doc, "Under", UNDER, &under);
    let doc = layer(doc, "Knock", E1, &shape);
    let doc = style_named(
        doc,
        "Knock",
        &json!({"knockout": "shallow", "fill_opacity": 0.0}),
    );
    let doc = group(doc, &["Under", "Knock"], "P", true);
    assert_flat_all(doc, 2, "shallow in pass through", |x, y| {
        let began = over_to(
            column(x),
            [0.0, 0.0, 0.9, 0.0],
            unit(&lower, x, y),
            BlendMode::Normal,
        );
        let acc = over_to(began, percent(UNDER), unit(&under, x, y), BlendMode::Normal);
        d3(
            acc,
            began,
            percent(E1),
            unit(&shape, x, y),
            0.0,
            BlendMode::Normal,
        )
    });
    free(doc);
}

#[test]
fn a_knocked_out_clip_member_stops_at_the_base_alone() {
    let base_cover = hard_rect(W, H, (8, 8, 40, 32));
    let first_cover = hard_rect(W, H, (4, 4, 30, 36));
    let shape = soft_rect(W, H, RECT);
    let base = [10.0, 60.0, 30.0, 0.0];
    let first = [0.0, 0.0, 100.0, 0.0];
    let doc = layer(backdrop(W, H, 16, &COLUMNS), "Base", base, &base_cover);
    let doc = layer(doc, "First", first, &first_cover);
    let doc = layer(doc, "Knock", E1, &shape);
    let doc = clip(clip(doc, "First"), "Knock");
    let doc = style_named(
        doc,
        "Knock",
        &json!({"knockout": "deep", "fill_opacity": 0.4}),
    );
    assert_flat_all(doc, 2, "clip member", |x, y| {
        let b = unit(&base_cover, x, y);
        let base_alone = over_to(TRANSPARENT, percent(base), b, BlendMode::Normal);
        let mut u = over_to(
            base_alone,
            percent(first),
            unit(&first_cover, x, y),
            BlendMode::Normal,
        );
        u[4] = u[4].min(b);
        let mut u = d3(
            u,
            base_alone,
            percent(E1),
            unit(&shape, x, y),
            0.4,
            BlendMode::Normal,
        );
        u[4] = u[4].min(b);
        over_to(column(x), [u[0], u[1], u[2], u[3]], u[4], BlendMode::Normal)
    });
    free(doc);
}

/// A Multiply colour overlay of E2 at `opacity`, grouped into the unit.
fn grouped_interior(fill: f64) -> Value {
    json!({"fill_opacity": fill, "blend_interior_as_group": true, "effects":[
        {"type":"color_overlay","ink":percent([100.0, 0.0, 0.0, 0.0]),"blend":"multiply","opacity":0.8}]})
}

#[test]
fn grouped_interior_recolours_the_unit_before_it_meets_the_backdrop() {
    let shape = hard_rect(W, H, RECT);
    let source = [10.0, 60.0, 30.0, 0.0];
    let doc = layer(backdrop(W, H, 16, &COLUMNS), "Shape", source, &shape);
    let doc = style_named(doc, "Shape", &grouped_interior(0.5));
    assert_flat_all(doc, 2, "grouped interior", |x, y| {
        let s = unit(&shape, x, y);
        if s == 0.0 {
            return column(x);
        }
        let mut u = over_to(TRANSPARENT, percent(source), s, BlendMode::Normal);
        u = over_to(u, [1.0, 0.0, 0.0, 0.0], 0.8 * s, BlendMode::Multiply);
        u[4] = s;
        d3(
            column(x),
            column(x),
            [u[0], u[1], u[2], u[3]],
            s,
            0.5,
            BlendMode::Normal,
        )
    });
    free(doc);
}

#[test]
fn grouped_interior_on_a_clip_base_covers_its_members() {
    let base_cover = hard_rect(W, H, RECT);
    let member_cover = hard_rect(W, H, (24, 4, 44, 36));
    let source = [10.0, 60.0, 30.0, 0.0];
    let member = [0.0, 0.0, 80.0, 10.0];
    let doc = layer(backdrop(W, H, 16, &COLUMNS), "Base", source, &base_cover);
    let doc = layer(doc, "Member", member, &member_cover);
    let doc = clip(doc, "Member");
    let doc = style_named(doc, "Base", &grouped_interior(1.0));
    assert_flat_all(doc, 2, "grouped clip base", |x, y| {
        let s = unit(&base_cover, x, y);
        if s == 0.0 {
            return column(x);
        }
        let mut u = over_to(TRANSPARENT, percent(source), s, BlendMode::Normal);
        u = over_to(
            u,
            percent(member),
            unit(&member_cover, x, y),
            BlendMode::Normal,
        );
        u[4] = u[4].min(s);
        u = over_to(u, [1.0, 0.0, 0.0, 0.0], 0.8 * s, BlendMode::Multiply);
        u[4] = s;
        d3(
            column(x),
            column(x),
            [u[0], u[1], u[2], u[3]],
            s,
            1.0,
            BlendMode::Normal,
        )
    });
    free(doc);
}
