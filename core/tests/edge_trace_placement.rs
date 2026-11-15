//! Vertex placement through `dr_selection_trace_edge` (the phase-6 ADR's
//! "Vertex placement"): every vertex sits on the edge its pixel locates, and
//! inside that pixel's half-open square. The oracles are the drawn geometry
//! and the ADR's arithmetic, never a traced path: a hard pixel-aligned step
//! is outlined on the step from either crest row; a box-filtered step (one
//! row covered `c` by the far side) on its boundary `r + 1 − c`; a step under
//! Contrast keeps its centres; and `floor` of a returned vertex is its pixel,
//! so a vertex passed back as `from` starts the next trace on that pixel.

use crate::edge_trace_support::{as_f64, image, samples, segment_distance, trace, Image};

const LIGHT: [u8; 4] = [220, 220, 220, 255];
const DARK: [u8; 4] = [30, 30, 30, 255];

/// The largest `f32` below `v`: how a vertex that would reach the next
/// pixel's edge is written inside its own.
fn below(v: f32) -> f32 {
    v.next_down()
}

/// A trace along row `row` (a y coordinate) from x = 5 to x = 190.
fn along_row(img: &Image, row: f64) -> Vec<[f32; 2]> {
    trace(
        img,
        &format!(
            r#"{{"from":[5,{row}],"trail":[[190,{row}]],"end":[190,{row}],"width":10,"frequency":0}}"#
        ),
    )
    .unwrap()
    .points
}

/// Dark above y = 20, light from it, 200 × 40.
fn hard_step() -> Image {
    image(200, 40, |_, y| if y < 20 { DARK } else { LIGHT })
}

#[test]
fn a_hard_step_is_outlined_on_the_step_from_either_crest_row() {
    // Rows 19 and 20 are the step's two crest rows: their Sobel strengths
    // are equal and the rows beyond have none (0, S, S, 0), so each is a
    // crest with k = −S, row 19 at δ = +½ and row 20 at δ = −½ along the
    // vertical normal. Both put their vertices on y = 20; row 19 writes it as
    // the f32 below 20, inside its own pixel. The trail's own row is the
    // cheaper of the two (the trail pull), so each trace stays on it.
    let img = hard_step();
    let on_20 = along_row(&img, 20.5);
    assert!(on_20.len() >= 2, "{on_20:?}");
    assert!(on_20.iter().all(|p| p[1] == 20.0), "row 20: {on_20:?}");
    let on_19 = along_row(&img, 19.5);
    assert!(
        on_19.iter().all(|p| p[1] == below(20.0)),
        "row 19: {on_19:?}"
    );
    // The normal is vertical, so x stays on the pixel centres.
    assert_eq!((on_20[0][0], on_19[0][0]), (5.5, 5.5));
    // The same step turned on its side: dark left of x = 20.
    let img = image(40, 200, |x, _| if x < 20 { DARK } else { LIGHT });
    let column = |x: f64| {
        trace(
            &img,
            &format!(
                r#"{{"from":[{x},5],"trail":[[{x},190]],"end":[{x},190],"width":10,"frequency":0}}"#
            ),
        )
        .unwrap()
        .points
    };
    let on_20 = column(20.5);
    assert!(on_20.iter().all(|p| p[0] == 20.0), "column 20: {on_20:?}");
    let on_19 = column(19.5);
    assert!(
        on_19.iter().all(|p| p[0] == below(20.0)),
        "column 19: {on_19:?}"
    );
}

/// 40 above row 20 and 200 below it, row 20 covered `c` by the light side:
/// 40 + 160·c, exact in u8 for quarters.
fn box_step(c: f64) -> Image {
    let mid = (40.0 + 160.0 * c) as u8;
    image(200, 40, move |_, y| match y {
        0..=19 => [40, 40, 40, 255],
        20 => [mid, mid, mid, 255],
        _ => [200, 200, 200, 255],
    })
}

#[test]
fn a_box_filtered_step_is_located_on_its_drawn_boundary() {
    // Row r = 20 covered c from below puts the boundary at y = r + 1 − c.
    // A row's vertical Sobel strength is proportional to the difference of
    // the rows either side, so rows 19, 20, 21 read c, 1, 1 − c: k = −1 and
    // the parabola's vertex lies (1 − c − c)/2 = ½ − c below row 20's
    // centre, at 20.5 + ½ − c, the boundary. Row 20 is the crest the path
    // runs along (rows 19 and 21 are slopes). At c = ½ the boundary is the
    // centre itself, which the placement keeps.
    for c in [0.25, 0.5, 0.75] {
        let points = along_row(&box_step(c), 20.5);
        let boundary = 21.0 - c;
        for p in &points {
            assert!(
                (f64::from(p[1]) - boundary).abs() <= 1e-4,
                "c {c}: {p:?} is not on y = {boundary}: {points:?}"
            );
        }
    }
}

#[test]
fn a_rectangle_is_outlined_on_its_sides_away_from_the_corners() {
    // A dark rectangle over pixels x 40..120, y 30..90. Away from a corner
    // every path pixel's 3×3 neighbourhood sees one straight side, a hard
    // step whose two crest pixels put their vertices on it; within 2 px of a
    // corner the neighbourhood sees the corner, which three samples along
    // one normal cannot locate (the ADR: a corner is cut, its pixels keep
    // their centres). The whole wire, not only its vertices, lies on the
    // sides away from the corners: a chord from a corner's centre to a
    // vertex further along a side would leave the side.
    let img = image(160, 120, |x, y| {
        if (40..120).contains(&x) && (30..90).contains(&y) {
            DARK
        } else {
            LIGHT
        }
    });
    let t = trace(
        &img,
        r#"{"from":[40,40],"trail":[[35,50],[35,70],[35,85],[45,95],[70,95]],"end":"snap","frequency":0}"#,
    )
    .unwrap();
    let corners = [[40.0, 30.0], [120.0, 30.0], [120.0, 90.0], [40.0, 90.0]];
    let mut on_a_side = 0;
    for q in samples(&t.points) {
        let near_corner = corners
            .iter()
            .any(|c| ((q[0] - c[0]).powi(2) + (q[1] - c[1]).powi(2)).sqrt() <= 2.0);
        if near_corner {
            continue;
        }
        let side = (0..4)
            .map(|k| segment_distance(q, corners[k], corners[(k + 1) % 4]))
            .fold(f64::INFINITY, f64::min);
        // One f32 step below 128 is 2⁻¹⁷ ≈ 7.6e-6.
        assert!(
            side <= 1e-5,
            "{q:?} is {side} px off the rectangle: {:?}",
            t.points
        );
        on_a_side += 1;
    }
    assert!(on_a_side >= 2, "{:?}", t.points);
}

#[test]
fn an_edge_under_contrast_keeps_its_centres() {
    // 200 above y = 40, 160 from it: a step 40/255 ≈ 0.157 strong, its
    // crest rows 39 and 40. At Contrast 30 it is no edge, so row 40 keeps its
    // centres; at Contrast 10 row 40 locates it at y = 40 (δ = −½).
    let img = image(200, 100, |_, y| {
        if y < 40 {
            [200, 200, 200, 255]
        } else {
            [160, 160, 160, 255]
        }
    });
    let at = |contrast: u32| {
        trace(
            &img,
            &format!(
                r#"{{"from":[5,40.5],"trail":[[190,40.5]],"end":[190,40.5],"width":10,"contrast":{contrast},"frequency":0}}"#
            ),
        )
        .unwrap()
        .points
    };
    let ignored = at(30);
    assert!(
        ignored.iter().all(|p| p[1] == 40.5),
        "contrast 30: {ignored:?}"
    );
    let located = at(10);
    assert!(
        located.iter().all(|p| p[1] == 40.0),
        "contrast 10: {located:?}"
    );
}

#[test]
fn a_vertex_names_its_pixel_and_starts_the_next_trace_there() {
    // The hard step's row-19 trace: every vertex floors to row 19, and its
    // last, passed back as `from` the way the host does, is the next trace's
    // first vertex bit for bit.
    let img = hard_step();
    let first = along_row(&img, 19.5);
    assert!(
        first.iter().all(|p| p[1].floor() == 19.0),
        "a row-19 vertex left its pixel: {first:?}"
    );
    let last = first[first.len() - 1];
    let back = trace(
        &img,
        &format!(
            r#"{{"from":[{},{}],"trail":[[100,19.5]],"end":[100,19.5],"width":10,"frequency":0}}"#,
            f64::from(last[0]),
            f64::from(last[1])
        ),
    )
    .unwrap()
    .points;
    assert_eq!(back[0], last);
    // Round a binary disc: explicit ends at 32 angles, alternately a quarter
    // pixel inside and outside the circle so both crest pixels of a tie are
    // ends somewhere, each trace starting from the last one's last vertex.
    let (c, r) = (60.0f64, 30.0f64);
    let disc = image(120, 120, |x, y| {
        let d = ((x as f64 + 0.5 - c).powi(2) + (y as f64 + 0.5 - c).powi(2)).sqrt();
        if d < r {
            DARK
        } else {
            LIGHT
        }
    });
    let at = |turn: f64, radius: f64| {
        let a = turn * std::f64::consts::TAU / 32.0;
        [c + radius * a.cos(), c + radius * a.sin()]
    };
    let floor = |p: [f64; 2]| [p[0].floor(), p[1].floor()];
    let mut from = at(0.0, r - 0.25);
    let mut previous: Option<[f32; 2]> = None;
    let mut written_below = 0;
    for k in 1..=32 {
        let end = at(k as f64, if k % 2 == 0 { r - 0.25 } else { r + 0.25 });
        let mid = at(k as f64 - 0.5, r + 3.0);
        let t = trace(
            &disc,
            &format!(
                r#"{{"from":[{},{}],"trail":[[{},{}],[{},{}]],"end":[{},{}],"width":6,"frequency":0}}"#,
                from[0], from[1], mid[0], mid[1], end[0], end[1], end[0], end[1]
            ),
        )
        .unwrap();
        let (head, tail) = (t.points[0], t.points[t.points.len() - 1]);
        assert_eq!(floor(as_f64(head)), floor(from), "{k}: from {from:?}");
        assert_eq!(floor(as_f64(tail)), floor(end), "{k}: end {end:?}");
        if let Some(previous) = previous {
            assert_eq!(head, previous, "{k}: the vertex passed back moved");
        }
        written_below += t
            .points
            .iter()
            .flatten()
            .filter(|v| v.fract() != 0.0 && v.next_up().fract() == 0.0)
            .count();
        previous = Some(tail);
        from = as_f64(tail);
    }
    assert!(
        written_below > 0,
        "no vertex round the disc needed the half-open write"
    );
}
