//! Black-box contracts of the Magnetic Lasso's live-wire tracer
//! (`dr_selection_trace_edge`): synthetic edges followed within a pixel,
//! Width, Contrast and the snap end honoured, Frequency's fasten rule, the
//! corridor count against a brute-force oracle, determinism and refusals.
//! Oracles are geometric (distances to the drawn shapes), never golden paths;
//! each exact vertex is derived from the cost rule in a comment beside it
//! (the rule's own oracle is `edge_trace_cost.rs`).
//! The `#[ignore]`d probes time budget-sized traces for `make cancel-bench`:
//! one with no edge map (a map's first trace), and five through a warm map
//! (the session's hover trace); their bounds are declared before measuring
//! and never widened.

use crate::edge_trace_support::{
    as_f64, image, raw_call, refusal, samples, segment_distance, trace, trace_through, Image, Map,
    Traced,
};
use std::time::Instant;

const LIGHT: [u8; 4] = [220, 220, 220, 255];
const DARK: [u8; 4] = [30, 30, 30, 255];
const WORK_BUDGET: u64 = 131_072;

/// Distance from `p` to the outline of the rectangle `[x0,x1]×[y0,y1]`.
fn rect_outline_distance(p: [f64; 2], r: [f64; 4]) -> f64 {
    let [x0, y0, x1, y1] = r;
    let inside = p[0] >= x0 && p[0] <= x1 && p[1] >= y0 && p[1] <= y1;
    if inside {
        (p[0] - x0).min(x1 - p[0]).min(p[1] - y0).min(y1 - p[1])
    } else {
        let dx = (x0 - p[0]).max(p[0] - x1).max(0.0);
        let dy = (y0 - p[1]).max(p[1] - y1).max(0.0);
        (dx * dx + dy * dy).sqrt()
    }
}

fn rect_image() -> Image {
    // A dark rectangle covering pixels x 40..120, y 30..90: edges at x = 40,
    // x = 120, y = 30, y = 90.
    image(160, 120, |x, y| {
        if (40..120).contains(&x) && (30..90).contains(&y) {
            DARK
        } else {
            LIGHT
        }
    })
}

const RECT: [f64; 4] = [40.0, 30.0, 120.0, 90.0];
const RECT_TRAIL: &str = r#"{"from":[40,40],"trail":[[35,50],[35,70],[35,85],[45,95],[70,95]],"end":"snap","frequency":0}"#;

#[test]
fn a_rectangle_edge_and_its_corner_are_followed_within_a_pixel() {
    let img = rect_image();
    let t = trace(&img, RECT_TRAIL).unwrap();
    assert!(t.points.len() >= 3, "{:?}", t.points);
    assert_eq!(t.fasten, None, "frequency 0 never fastens");
    for &p in &t.points {
        let d = rect_outline_distance(as_f64(p), RECT);
        assert!(d <= 1.0, "vertex {p:?} is {d} px off the rectangle");
    }
    for p in samples(&t.points) {
        let d = rect_outline_distance(p, RECT);
        assert!(d <= 1.5, "sample {p:?} is {d} px off the rectangle");
    }
    let last = t.points[t.points.len() - 1];
    assert!(
        (f64::from(last[1]) - 90.0).abs() <= 1.0 && last[0] > 60.0,
        "the trace ends on the bottom edge near the pointer: {last:?}"
    );
}

#[test]
fn a_disc_edge_is_followed_within_a_pixel() {
    let (cx, cy, r) = (60.0f64, 60.0f64, 30.0f64);
    let img = image(120, 120, |x, y| {
        let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
        if d < r {
            DARK
        } else {
            LIGHT
        }
    });
    let trail: Vec<String> = (1..=9)
        .map(|k| {
            let a = (k * 10) as f64 * std::f64::consts::PI / 180.0;
            format!("[{},{}]", cx + 35.0 * a.cos(), cy + 35.0 * a.sin())
        })
        .collect();
    let json = format!(
        r#"{{"from":[90,60],"trail":[{}],"end":"snap","frequency":0}}"#,
        trail.join(",")
    );
    let t = trace(&img, &json).unwrap();
    for &p in &t.points {
        let d = ((f64::from(p[0]) - cx).powi(2) + (f64::from(p[1]) - cy).powi(2)).sqrt();
        assert!((d - r).abs() <= 1.0, "vertex {p:?} is {d} from the centre");
    }
    let last = t.points[t.points.len() - 1];
    assert!(
        f64::from(last[1]) > 85.0,
        "the arc reaches the bottom: {last:?}"
    );
}

/// Light above y = 40, a weak step to mid-grey for y 40..60, then a strong
/// step to black from y = 60.
fn two_edges() -> Image {
    image(200, 100, |_, y| {
        if y < 40 {
            [200, 200, 200, 255]
        } else if y < 60 {
            [160, 160, 160, 255]
        } else {
            [0, 0, 0, 255]
        }
    })
}

#[test]
fn a_stronger_edge_outside_the_width_is_ignored() {
    let img = two_edges();
    let json = |width: u32| {
        format!(
            r#"{{"from":[10,40],"trail":[[100,35],[190,35]],"end":[190,40],"width":{width},"frequency":0}}"#
        )
    };
    let narrow = trace(&img, &json(8)).unwrap();
    for &p in &narrow.points {
        assert!(
            (f64::from(p[1]) - 40.0).abs() <= 1.0,
            "width 8 follows the weak edge: {p:?}"
        );
    }
    let wide = trace(&img, &json(30)).unwrap();
    assert!(
        wide.points
            .iter()
            .any(|p| (f64::from(p[1]) - 60.0).abs() <= 1.0),
        "width 30 reaches the strong edge: {:?}",
        wide.points
    );
}

#[test]
fn contrast_decides_whether_a_weak_edge_attracts() {
    let img = two_edges();
    let json = |contrast: u32| {
        format!(
            r#"{{"from":[10,35.5],"trail":[[100,35.5],[190,35.5]],"end":"snap","width":10,"contrast":{contrast},"frequency":0}}"#
        )
    };
    // The weak step is 40/255 ≈ 0.157 strong.
    let follows = trace(&img, &json(10)).unwrap();
    let last = follows.points[follows.points.len() - 1];
    assert!((f64::from(last[1]) - 40.0).abs() <= 1.0, "{last:?}");
    for p in samples(&follows.points) {
        if (60.0..=140.0).contains(&p[0]) {
            assert!(
                (p[1] - 40.0).abs() <= 1.0,
                "contrast 10 follows the edge: {p:?}"
            );
        }
    }
    let ignores = trace(&img, &json(30)).unwrap();
    for &p in &ignores.points {
        assert!(
            (f64::from(p[1]) - 35.5).abs() <= 2.0,
            "contrast 30 follows the trail: {p:?}"
        );
    }
}

#[test]
fn a_flat_image_is_traced_along_the_trail() {
    let img = image(128, 64, |_, _| LIGHT);
    let t = trace(
        &img,
        r#"{"from":[0,0],"trail":[[100,30]],"end":"snap","width":10,"frequency":0}"#,
    )
    .unwrap();
    for &p in &t.points {
        let d = segment_distance(as_f64(p), [0.0, 0.0], [100.0, 30.0]);
        assert!(d <= 2.0, "vertex {p:?} is {d} px off the trail");
    }
    // No pixel is an edge, so the snap falls back to the trail's last point,
    // and flat ground keeps its pixel's centre.
    assert_eq!(t.points[t.points.len() - 1], [100.5, 30.5]);
}

#[test]
fn snap_ends_on_the_edge_and_a_point_ends_on_the_pointer() {
    let img = rect_image();
    // The snap end is the cost rule's untouched part: the strongest pixel
    // within width of (35, 70), columns 39 and 40 tying at full strength.
    // Both are crests of the step at x = 40 (δ = +½ and −½ along x), so the
    // vertex is on x = 40, column 39's written as the f32 below it.
    // An explicit end and `from` are their own pixels whatever the costs.
    let snapped = trace(
        &img,
        r#"{"from":[40,40],"trail":[[35,70]],"end":"snap","frequency":0}"#,
    )
    .unwrap();
    let last = snapped.points[snapped.points.len() - 1];
    assert!(
        last[0] == 40.0 || last[0] == 40f32.next_down(),
        "snap lands on the edge: {last:?}"
    );
    let pointed = trace(
        &img,
        r#"{"from":[40,40],"trail":[[35,70]],"end":[35,70],"frequency":0}"#,
    )
    .unwrap();
    // (35, 70) is flat ground, five columns off the edge: its centre.
    assert_eq!(pointed.points[pointed.points.len() - 1], [35.5, 70.5]);
    // (40, 40) is the step's right crest column: δ = −½ puts it on x = 40.
    assert_eq!(pointed.points[0], [40.0, 40.5]);
}

#[test]
fn an_alpha_only_edge_attracts() {
    let img = image(100, 60, |x, _| {
        if x < 50 {
            [128, 128, 128, 255]
        } else {
            [128, 128, 128, 0]
        }
    });
    let t = trace(
        &img,
        r#"{"from":[50,5],"trail":[[55,30],[55,55]],"end":"snap","frequency":0}"#,
    )
    .unwrap();
    for &p in &t.points {
        assert!((f64::from(p[0]) - 50.0).abs() <= 1.0, "{p:?}");
    }
    assert!(f64::from(t.points[t.points.len() - 1][1]) > 45.0);
}

/// Dark above y = 20, light from y = 20, 1200 px wide.
///
/// Under the cost rule (the phase-6 ADR) rows 19 and 20 are the step's two
/// crest rows, each half a pixel off it (`o = 1`), so a horizontal step
/// along either costs `LAMBDA + SUBPIXEL` = 0.15 plus the trail pull, which
/// is 0 on row 20, the trail's own row, and 0.02/width on row 19. Any step
/// off row 20 is vertical or diagonal, at least 45° off the edge's
/// horizontal tangent, so it gains no attraction and costs flat ground's
/// 1.05 or more. The path from `(5, 20)` to `(end_x, 20)` is therefore row 20
/// itself, and where it fastens depends on Frequency and the tail alone.
/// Row 20 is the step's lower crest row (δ = −½), so every vertex is placed
/// on the step, `y = 20`, at its pixel's centre `x`.
fn long_edge() -> Image {
    image(1200, 40, |_, y| if y < 20 { DARK } else { LIGHT })
}

fn fasten_at(img: &Image, end_x: f64, frequency: f64) -> Traced {
    trace(
        img,
        &format!(
            r#"{{"from":[5,20],"trail":[[{end_x},20]],"end":[{end_x},20],"width":10,"frequency":{frequency}}}"#
        ),
    )
    .unwrap()
}

#[test]
fn frequency_sets_the_fasten_distance() {
    let img = long_edge();
    assert_eq!(fasten_at(&img, 1195.5, 0.0).fasten, None);
    let mut previous = f64::INFINITY;
    for frequency in [1.0, 10.0, 30.0, 57.0, 80.0, 100.0] {
        let t = fasten_at(&img, 1195.5, frequency);
        // The path runs straight along row 20 (`long_edge`), so the fasten
        // pixel is the first whose length from (5, 20) reaches
        // `fasten_distance`: 800 px at 1, 40 at 57, 8 at 100.
        assert!(t.points.iter().all(|p| p[1] == 20.0), "{:?}", t.points);
        let k = t.fasten.expect("a long path fastens");
        let along = f64::from(t.points[k][0]) - 5.5;
        assert!(along > 0.0 && along <= previous, "{frequency}: {along}");
        previous = along;
        match frequency as u32 {
            1 => assert_eq!(t.points[k], [805.5, 20.0]),
            57 => assert_eq!(t.points[k], [45.5, 20.0]),
            100 => assert_eq!(t.points[k], [13.5, 20.0]),
            _ => {}
        }
    }
}

#[test]
fn a_fasten_needs_a_tail_of_the_width() {
    let img = long_edge();
    // 45 px of path at the default 40-px fasten leaves 5 < width 10.
    assert_eq!(fasten_at(&img, 50.5, 57.0).fasten, None);
    // 50 px leaves exactly 10.
    let t = fasten_at(&img, 55.5, 57.0);
    let k = t.fasten.expect("the tail is long enough");
    assert_eq!(t.points[k], [45.5, 20.0]);
    assert_eq!(t.points, vec![[5.5, 20.0], [45.5, 20.0], [55.5, 20.0]]);
}

#[test]
fn a_closed_loop_at_frequency_0_fastens_and_keeps_the_whole_outline() {
    // A dark square covering pixels 50..150 on both axes.
    const SQUARE: [f64; 4] = [50.0, 50.0, 150.0, 150.0];
    let img = image(200, 200, |x, y| {
        if (50..150).contains(&x) && (50..150).contains(&y) {
            DARK
        } else {
            LIGHT
        }
    });
    // Clockwise 5 px outside the square, 2.5 px apart, from above the top
    // edge's middle round to 10 px short of it.
    let mut trail = Vec::new();
    let mut push_run = |a: [f64; 2], b: [f64; 2]| {
        let steps = (((b[0] - a[0]).abs() + (b[1] - a[1]).abs()) / 2.5) as usize;
        for k in 1..=steps {
            let t = k as f64 / steps as f64;
            trail.push(format!(
                "[{},{}]",
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t
            ));
        }
    };
    push_run([100.0, 45.0], [155.0, 45.0]);
    push_run([155.0, 45.0], [155.0, 155.0]);
    push_run([155.0, 155.0], [45.0, 155.0]);
    push_run([45.0, 155.0], [45.0, 45.0]);
    push_run([45.0, 45.0], [90.0, 45.0]);
    let json = format!(
        r#"{{"from":[100,50],"trail":[{}],"end":"snap","width":10,"frequency":0}}"#,
        trail.join(",")
    );
    let t = trace(&img, &json).unwrap();
    let k = t.fasten.expect("a loop back to the anchor fastens");
    let at = as_f64(t.points[k]);
    assert!(
        ((at[0] - 100.0).powi(2) + (at[1] - 50.0).powi(2)).sqrt() > 20.0,
        "the junction is out at the far side: {at:?}"
    );
    for &p in &t.points {
        let d = rect_outline_distance(as_f64(p), SQUARE);
        assert!(d <= 1.0, "vertex {p:?} is {d} px off the square");
    }
    // Every side is followed end to end, not cut short: (the axis the side
    // is fixed on, its coordinate, the axis it runs along, its name).
    let along = samples(&t.points);
    for (fixed, at, run, name) in [
        (1, 50.0, 0, "top"),
        (0, 150.0, 1, "right"),
        (1, 150.0, 0, "bottom"),
        (0, 50.0, 1, "left"),
    ] {
        let on: Vec<f64> = along
            .iter()
            .filter(|p| (p[fixed] - at).abs() <= 1.0)
            .map(|p| p[run])
            .collect();
        let covered = on.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - on.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(covered >= 98.0, "the {name} side spans only {covered} px");
    }
}

#[test]
fn an_overshoot_backed_up_along_the_edge_is_not_a_loop() {
    let img = rect_image();
    // Along the top edge 3 px above it, 2.5 px apart: out 55 px from the
    // anchor (more than 2·width), then back to 10 px from it (within).
    let mut trail: Vec<String> = (1..=22)
        .map(|k| format!("[{},27]", 50.0 + 2.5 * k as f64))
        .collect();
    trail.extend((1..=18).map(|k| format!("[{},27]", 105.0 - 2.5 * k as f64)));
    for frequency in [57.0, 0.0] {
        let json = format!(
            r#"{{"from":[50,30],"trail":[{}],"end":"snap","width":10,"frequency":{frequency}}}"#,
            trail.join(",")
        );
        let t = trace(&img, &json).unwrap();
        assert_eq!(t.fasten, None, "frequency {frequency}: {:?}", t.points);
        let far = t
            .points
            .iter()
            .map(|p| f64::from(p[0]))
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            far <= 72.0,
            "frequency {frequency}: the wire reaches x = {far}"
        );
    }
}

#[test]
fn a_radius_1000_disc_traced_in_four_quarters_stays_on_the_circle() {
    let (c, r) = (1050.0f64, 1000.0f64);
    let img = image(2100, 2100, |x, y| {
        let d = ((x as f64 + 0.5 - c).powi(2) + (y as f64 + 0.5 - c).powi(2)).sqrt();
        if d < r {
            DARK
        } else {
            LIGHT
        }
    });
    let at = |deg: f64, radius: f64| {
        let a = deg.to_radians();
        [c + radius * a.cos(), c + radius * a.sin()]
    };
    let mut from = at(0.0, r);
    let mut total = 0;
    for quarter in 0..4 {
        let trail: Vec<String> = (1..=90)
            .map(|k| {
                let p = at((quarter * 90 + k) as f64, r + 4.0);
                format!("[{},{}]", p[0], p[1])
            })
            .collect();
        let json = format!(
            r#"{{"from":[{},{}],"trail":[{}],"end":"snap","width":10,"frequency":0}}"#,
            from[0],
            from[1],
            trail.join(",")
        );
        let t = trace(&img, &json).unwrap();
        total += t.points.len();
        for p in samples(&t.points) {
            let d = ((p[0] - c).powi(2) + (p[1] - c).powi(2)).sqrt();
            assert!((d - r).abs() <= 1.5, "quarter {quarter}: {p:?} at {d}");
        }
        from = as_f64(t.points[t.points.len() - 1]);
    }
    assert!(total < 10_000, "{total} vertices");
}

#[test]
fn a_trace_is_deterministic() {
    let img = rect_image();
    let a = trace(&img, RECT_TRAIL).unwrap();
    let b = trace(&img, RECT_TRAIL).unwrap();
    assert_eq!(a.points, b.points);
    assert_eq!(a.fasten, b.fasten);
    assert_eq!(a.corridor, b.corridor);
}

#[test]
fn the_corridor_count_matches_a_brute_force_count() {
    let (w, h) = (120u32, 90u32);
    let img = image(w, h, |x, y| [(x * 2) as u8, (y * 2) as u8, 90, 255]);
    let poly = [[10.3, 12.7], [50.1, 40.6], [80.9, 15.2], [100.37, 70.21]];
    let width = 7.3;
    let t = trace(
        &img,
        r#"{"from":[10.3,12.7],"trail":[[50.1,40.6],[80.9,15.2]],"end":[100.37,70.21],"width":7.3}"#,
    )
    .unwrap();
    let mut count = 0u64;
    for y in 0..h {
        for x in 0..w {
            let p = [x as f64 + 0.5, y as f64 + 0.5];
            if poly
                .windows(2)
                .any(|s| segment_distance(p, s[0], s[1]) <= width)
            {
                count += 1;
            }
        }
    }
    assert_eq!(t.corridor, count);
}

#[test]
fn malformed_requests_refuse_and_leave_the_outputs_untouched() {
    let img = image(100, 60, |_, _| LIGHT);
    let base = |extra: &str| format!(r#"{{"from":[10,10],"trail":[[50,30]],"end":"snap"{extra}}}"#);
    assert!(refusal(&img, &base(r#","bogus":1"#)).contains("unknown selection parameter: bogus"));
    for (extra, key) in [
        (r#","width":0"#, "width"),
        (r#","width":257"#, "width"),
        (r#","contrast":0"#, "contrast"),
        (r#","contrast":101"#, "contrast"),
        (r#","frequency":-1"#, "frequency"),
        (r#","frequency":101"#, "frequency"),
    ] {
        let message = refusal(&img, &base(extra));
        assert!(
            message.contains(&format!("{key} must be between")),
            "{message}"
        );
    }
    assert!(refusal(&img, r#"{"from":[10,10],"trail":[],"end":"snap"}"#).contains("1 to 4096"));
    let long = vec!["[20,20]"; 4097].join(",");
    assert!(refusal(
        &img,
        &format!(r#"{{"from":[10,10],"trail":[{long}],"end":"snap"}}"#)
    )
    .contains("not 4097"));
    assert!(
        refusal(&img, r#"{"from":[-1,10],"trail":[[50,30]],"end":"snap"}"#).contains("outside")
    );
    assert!(
        refusal(&img, r#"{"from":[10,10],"trail":[[101,30]],"end":"snap"}"#).contains("outside")
    );
    assert!(
        refusal(&img, r#"{"from":[10,10],"trail":[[50,30]],"end":[50,61]}"#).contains("outside")
    );
    assert!(refusal(&img, r#"{"from":[10,10],"trail":[[50,30]],"end":"near"}"#).contains("end"));
    assert!(refusal(&img, r#"{"trail":[[50,30]],"end":"snap"}"#).contains("from"));
    // The edges of the image are inside the coordinate range.
    trace(&img, r#"{"from":[0,0],"trail":[[100,60]],"end":[100,60]}"#).unwrap();
}

#[test]
fn a_corridor_over_the_cap_refuses() {
    let img = image(2048, 2048, |_, _| LIGHT);
    let message = refusal(
        &img,
        r#"{"from":[0,512],"trail":[[2048,512],[2048,1536],[0,1536]],"end":"snap","width":256}"#,
    );
    assert!(message.contains("1048576-pixel limit"), "{message}");
}

#[test]
fn a_short_buffer_reports_the_count_needed() {
    let img = rect_image();
    let full = trace(&img, RECT_TRAIL).unwrap();
    let (ok, count, fasten, corridor, _, message) = raw_call(&img, RECT_TRAIL, 1);
    assert!(!ok);
    assert_eq!(count, full.points.len());
    assert_eq!((fasten, corridor), (i32::MIN, u64::MAX));
    assert!(
        message.contains(&format!("{} vertices", full.points.len())) && message.contains("holds 1"),
        "{message}"
    );
    // The size query: no buffer at all.
    let (ok, count, _, _, _, _) = raw_call(&img, RECT_TRAIL, 0);
    assert!(!ok);
    assert_eq!(count, full.points.len());
}

#[test]
fn the_header_states_the_work_budget() {
    let header = include_str!("../include/doctored_core.h");
    let line = header
        .lines()
        .find(|l| l.starts_with("#define DR_EDGE_TRACE_WORK_BUDGET"))
        .expect("the define");
    let value: u64 = line.split_whitespace().nth(2).unwrap().parse().unwrap();
    assert_eq!(value, WORK_BUDGET);
}

/// The side of the winding probes' square picture, in pixels.
const WINDING_SIZE: u32 = 2048;

/// The winding probes' picture: a deterministic texture, so edge strength
/// varies everywhere.
fn winding_texture() -> Image {
    image(WINDING_SIZE, WINDING_SIZE, |x, y| {
        let v = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) >> 24;
        let v = v as u8 / 4 + if (x / 37 + y / 23) % 2 == 0 { 40 } else { 160 };
        [v, v, v, 255]
    })
}

/// The winding probes' request at `width`: a sine wave of 4,000 vertices
/// 0.5 px apart along the curve, a snap end, Frequency 0.
fn winding_request(width: f64) -> String {
    let mut trail = Vec::with_capacity(4000);
    let (mut x, mut y) = (24.0f64, 1024.0f64);
    let wave = |x: f64| 1024.0 + 60.0 * (x / 40.0).sin();
    for _ in 0..4000 {
        trail.push(format!("[{x:.4},{y:.4}]"));
        let slope = 1.5 * (x / 40.0).cos();
        x += 0.5 / (1.0 + slope * slope).sqrt();
        y = wave(x);
    }
    format!(
        r#"{{"from":[24,1024],"trail":[{}],"end":"snap","width":{width},"frequency":0}}"#,
        trail.join(",")
    )
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict"]
fn a_budget_sized_winding_trace_fits_a_frame() {
    // A map's first trace (no edge map) at the work budget, bounded by one
    // 60 Hz frame.
    const BOUND_MS: f64 = 16.0;
    let img = winding_texture();
    let width = WIDTH_FOR_BUDGET;
    let json = winding_request(width);
    let first = trace(&img, &json).unwrap();
    let share = first.corridor as f64 / WORK_BUDGET as f64;
    assert!(
        (0.9..=1.0).contains(&share),
        "corridor {} is {share:.3} of the budget",
        first.corridor
    );
    let start = Instant::now();
    let timed = trace(&img, &json).unwrap();
    let ms = start.elapsed().as_secs_f64() * 1e3;
    assert_eq!(timed.points, first.points);
    eprintln!(
        "edge trace ({WINDING_SIZE}×{WINDING_SIZE} texture, 4000-vertex sine trail 0.5 px \
         apart, width {width}): corridor {} px ({:.1}% of the {WORK_BUDGET}-px budget), {} \
         vertices, {ms:.3} ms; bound {BOUND_MS} ms; {}",
        timed.corridor,
        share * 100.0,
        timed.points.len(),
        if ms <= BOUND_MS { "PASS" } else { "MISS" }
    );
}

/// Tuned so the winding probes' corridor is 90–100 % of the work budget.
const WIDTH_FOR_BUDGET: f64 = 30.0;

/// The traces each warm probe times through its filled map.
const WARM_TRACES: usize = 5;

/// Traces [`winding_request`] at `width` once through a fresh edge map of
/// `img`, which fills the map, then times [`WARM_TRACES`] more through it and
/// prints the slowest against `bound_ms`. The corridor must be 90–100 % of
/// `budget`, and each timed trace must return the fill's result and compute
/// no sample (the map's count is unchanged), so the timings are of the warm
/// path alone.
fn warm_probe(img: &Image, budget: u64, width: f64, label: &str, bound_ms: f64) {
    let json = winding_request(width);
    let map = Map::of(img);
    let fill = trace_through(img, map.0, &json).unwrap();
    let share = fill.corridor as f64 / budget as f64;
    assert!(
        (0.9..=1.0).contains(&share),
        "width {width}: corridor {} is {share:.3} of {budget} px",
        fill.corridor
    );
    let cached = map.cached();
    let mut times = Vec::with_capacity(WARM_TRACES);
    for _ in 0..WARM_TRACES {
        let start = Instant::now();
        let timed = trace_through(img, map.0, &json).unwrap();
        times.push(start.elapsed().as_secs_f64() * 1e3);
        assert_eq!(timed.points, fill.points);
        assert_eq!((timed.fasten, timed.corridor), (fill.fasten, fill.corridor));
        assert_eq!(map.cached(), cached, "a timed trace computed samples");
    }
    let min = times.iter().copied().fold(f64::INFINITY, f64::min);
    let max = times.iter().copied().fold(0.0, f64::max);
    eprintln!(
        "edge trace through a warm map ({WINDING_SIZE}×{WINDING_SIZE} texture, 4000-vertex sine \
         trail 0.5 px apart, width {width}, {label}): corridor {} px ({:.1}% of {budget} px), {} \
         vertices, {WARM_TRACES} traces {min:.3}–{max:.3} ms, slowest {max:.3} ms; bound \
         {bound_ms} ms; {}",
        fill.corridor,
        share * 100.0,
        fill.points.len(),
        if max <= bound_ms { "PASS" } else { "MISS" }
    );
}

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict"]
fn a_budget_sized_winding_trace_through_a_warm_map_fits_a_frame() {
    // A hover trace at the work budget through the session's edge map, once
    // the map holds the corridor's samples, must finish inside a 60 Hz frame.
    const BOUND_MS: f64 = 16.0;
    let img = winding_texture();
    warm_probe(&img, WORK_BUDGET, WIDTH_FOR_BUDGET, "the budget", BOUND_MS);
}
