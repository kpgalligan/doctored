//! Byte affine and perspective SET geometry under a job control: the
//! Free Transform / Distort / `transform_layers` commits reach
//! `dr_doc_geometry_edit` with a control, and a cancel must stop the kernel —
//! the raster resample, a linked layer mask's, a group mask's and a retained
//! member's re-render — and answer "operation cancelled" with no scratch
//! left, never a partial document and never the refusal `Ok(None)`. Every
//! cancel PARKS the worker at a counted checkpoint (`cancelled_parked_at`)
//! taken from a fresh twin's own report log; nothing here races a poller or
//! judges a wall clock. A controlled run that is not cancelled must equal
//! the uncontrolled one exactly.

use doctored_core::doc::{DrDocument, LayerRaster, MaskKind};
use doctored_core::ffi_jobs::*;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use doctored_core::retained_source::SourceEdit;
use image::{Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use common::probe::*;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// A 17° rotation about the canvas origin: every destination pixel resamples.
fn rotation() -> [f64; 6] {
    let (s, c) = 17f64.to_radians().sin_cos();
    [c, s, -s, c, 40., -20.]
}

fn affine_set(layers: &[usize], matrix: [f64; 6], filter: &str) -> GeometryEdit {
    GeometryEdit::parse(
        &json!({"op":"affine_set","layers":layers,"matrix":matrix,"filter":filter}).to_string(),
    )
    .unwrap()
}

fn perspective_set(layers: &[usize], rect: [f64; 4], quad: [f64; 8], filter: &str) -> GeometryEdit {
    GeometryEdit::parse(
        &json!({"op":"perspective_set","layers":layers,"source_rect":rect,
            "quad":quad,"filter":filter})
        .to_string(),
    )
    .unwrap()
}

fn pattern(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8, 255])
    })
}

/// One 1200×800 raster layer.
fn plain() -> DrDocument {
    DrDocument::from_pixels(pattern(1200, 800))
}

/// A byte document with a retained-source member on top of a plain raster
/// (the `tests/retained_geometry.rs` fixture, larger so its re-render polls).
fn retained(base: (u32, u32), source: (u32, u32)) -> DrDocument {
    let doc = DrDocument::from_pixels(RgbaImage::from_pixel(base.0, base.1, Rgba([2, 4, 6, 255])));
    let original = DrDocument::from_pixels(RgbaImage::from_fn(source.0, source.1, |x, y| {
        Rgba([
            (x * 31 + y * 7) as u8,
            (x * 17 + y * 19) as u8,
            221,
            if x == 0 { 0 } else { 255 },
        ])
    }));
    let place = SourceEdit::parse(
        &json!({"op":"place","source_id":"cancel-master",
            "revision_id":"first","name":"Source","x":1,"y":1})
        .to_string(),
    )
    .unwrap();
    doc.source_edit(Some(&original), &place, LIMIT, 0, None)
        .unwrap()
        .unwrap()
}

/// The render every probe drives: `geometry_edit` under the probe's control.
/// Completes only with an actual edit; a refusal is `(false, "no edit")`.
fn render<'a>(
    doc: &'a DrDocument,
    edit: &'a GeometryEdit,
) -> impl FnOnce(usize) -> (bool, String) + Send + 'a {
    move |token| {
        let control = unsafe { &*(token as *const DrJobControl) };
        match doc.geometry_edit(edit, LIMIT, 0, Some(control)) {
            Ok(Some(_)) => (true, String::new()),
            Ok(None) => (false, "no edit".into()),
            Err(e) => (false, e),
        }
    }
}

/// The checkpoint count of a completed controlled run.
fn total_checkpoints(doc: &DrDocument, edit: &GeometryEdit) -> u64 {
    let control = DrJobControl::default();
    assert!(doc
        .geometry_edit(edit, LIMIT, 0, Some(&control))
        .unwrap()
        .is_some());
    control.checkpoints()
}

fn at_member(from: f64) -> impl Fn(f64, u32, u64) -> bool {
    move |progress, stage, _| stage == 6 && progress >= from - 1e-6
}

fn inside_member(from: f64, to: f64) -> impl Fn(f64, u32) -> bool {
    move |progress, stage| stage == 6 && progress >= from - 1e-6 && progress < to - 1e-6
}

fn assert_unchanged(before: &DrDocument, after: &DrDocument) {
    assert_eq!(before.layers.len(), after.layers.len());
    for (a, b) in before.layers.iter().zip(&after.layers) {
        assert!(LayerRaster::ptr_eq(&a.pixels, &b.pixels));
        assert_eq!(a.offset, b.offset);
        assert_eq!(a.mask_properties.offset, b.mask_properties.offset);
    }
}

#[test]
fn a_cancel_parks_inside_the_affine_set_resample() {
    let doc = plain();
    let snapshot = doc.clone();
    let edit = affine_set(&[0], rotation(), "lanczos3");
    let n0 = entry_count(&at_member(0.), render(&doc, &edit));
    let p = cancelled_parked_at(n0 + 1 + 40, &inside_member(0., 1.), render(&doc, &edit));
    eprintln!("affine set resample: {}", parked_line(&p));
    assert_unchanged(&snapshot, &doc);
}

#[test]
fn b_a_cancel_parks_inside_a_linked_layer_mask_and_a_group_mask_resample() {
    // The linked layer mask resamples after the raster: park past the
    // unmasked twin's last kernel checkpoint.
    let bare = plain();
    let masked = bare.add_mask(0, MaskKind::RevealAll).unwrap();
    let snapshot = masked.clone();
    let edit = affine_set(&[0], rotation(), "lanczos3");
    let raster_only = total_checkpoints(&bare, &edit);
    assert!(total_checkpoints(&masked, &edit) > raster_only + 40);
    let n0 = entry_count(&at_member(0.), render(&masked, &edit));
    let k = raster_only - n0 + 20;
    let p = cancelled_parked_at(n0 + 1 + k, &inside_member(0., 1.), render(&masked, &edit));
    eprintln!("linked layer mask resample: {}", parked_line(&p));
    assert_unchanged(&snapshot, &masked);

    // A group's mask is its own member, after its raster child (postorder).
    let (grouped, group, _, _) = plain().group_layers(&[0], "Group").unwrap();
    let grouped = grouped.add_mask(group, MaskKind::RevealAll).unwrap();
    let snapshot = grouped.clone();
    let edit = affine_set(&[group], rotation(), "lanczos3");
    let n0 = entry_count(&at_member(0.5), render(&grouped, &edit));
    let p = cancelled_parked_at(
        n0 + 1 + 40,
        &inside_member(0.5, 1.),
        render(&grouped, &edit),
    );
    eprintln!("group mask resample: {}", parked_line(&p));
    assert_unchanged(&snapshot, &grouped);
}

/// The stage-6 progress log of one uncancelled run.
fn stage6(doc: &DrDocument, edit: &GeometryEdit) -> Vec<f64> {
    reports(render(doc, edit))
        .iter()
        .filter(|r| r.stage == 6)
        .map(|r| r.progress)
        .collect()
}

fn assert_non_decreasing(log: &[f64]) {
    assert!(
        log.windows(2).all(|pair| pair[0] <= pair[1] + 1e-12),
        "{log:?}"
    );
}

fn max_of(log: &[f64]) -> f64 {
    log.iter().copied().fold(0., f64::max)
}

#[test]
fn c_perspective_sets_poll_the_kernel_and_report_rows_per_member() {
    let doc = plain();
    let snapshot = doc.clone();
    // q0 − q2 + q4 − q6 = 0 on both axes: the parallelogram branch.
    let edit = perspective_set(
        &[0],
        [0., 0., 1200., 800.],
        [0., 0., 1180., 20., 1200., 820., 20., 800.],
        "lanczos3",
    );
    let n0 = entry_count(&at_member(0.), render(&doc, &edit));
    let p = cancelled_parked_at(n0 + 1 + 40, &inside_member(0., 1.), render(&doc, &edit));
    eprintln!("parallelogram perspective set: {}", parked_line(&p));
    assert_unchanged(&snapshot, &doc);

    // A true perspective over two members: each member's rows report inside
    // its own half.
    let two = doc.adding_image_layer(0, pattern(300, 200), "B").unwrap();
    let edit = perspective_set(
        &[0, 1],
        [0., 0., 1200., 800.],
        [0., 0., 1180., 0., 1170., 790., 0., 800.],
        "bilinear",
    );
    let members = stage6(&two, &edit);
    assert_non_decreasing(&members);
    for want in [0., 0.5] {
        assert!(
            members.iter().any(|got| (got - want).abs() <= 1e-6),
            "{want} missing: {members:?}"
        );
    }
    for (from, to) in [(0., 0.5), (0.5, 1.)] {
        assert!(
            members
                .iter()
                .any(|&got| got > from + 1e-6 && got < to - 1e-6),
            "no row inside ({from}, {to}): {members:?}"
        );
    }
    assert!((max_of(&members) - 1.).abs() <= 1e-6, "{members:?}");
}

#[test]
fn d_a_cancel_inside_a_retained_member_is_a_cancel_not_a_refusal() {
    let doc = retained((400, 300), (320, 240));
    assert!(doc.layers[1].retained_source.is_some());
    let snapshot = doc.clone();
    let edit = affine_set(&[0, 1], rotation(), "lanczos3");
    let n0 = entry_count(&at_member(0.5), render(&doc, &edit));
    let p = cancelled_parked_at(n0 + 1 + 20, &inside_member(0.5, 1.), render(&doc, &edit));
    eprintln!("retained member: {}", parked_line(&p));
    assert!(p.progress < 1. && p.stage == 6);
    assert_unchanged(&snapshot, &doc);
}

fn assert_same(a: &DrDocument, b: &DrDocument, what: &str) {
    assert_eq!(a.layers.len(), b.layers.len(), "{what}");
    for (x, y) in a.layers.iter().zip(&b.layers) {
        assert_eq!(x.offset, y.offset, "{what}: offset");
        assert!(
            x.pixels.to_contiguous().unwrap() == y.pixels.to_contiguous().unwrap(),
            "{what}: pixels"
        );
        assert_eq!(x.mask.as_deref(), y.mask.as_deref(), "{what}: mask");
        assert_eq!(
            x.mask_properties.offset, y.mask_properties.offset,
            "{what}: mask offset"
        );
    }
}

/// The uncontrolled and the controlled result (the latter under the plan's
/// own scratch limit); both must be edits.
fn both(doc: &DrDocument, edit: &GeometryEdit) -> (DrDocument, DrDocument) {
    let plan: Value = doc.geometry_edit_plan(edit).unwrap().json();
    let scratch = plan["scratch_bytes"].as_u64().unwrap();
    let free = doc.geometry_edit(edit, LIMIT, 0, None).unwrap().unwrap();
    let control = unsafe { dr_job_control_create() };
    assert!(unsafe { dr_job_control_set_scratch_limit(control, scratch) });
    let controlled = doc
        .geometry_edit(edit, LIMIT, 0, Some(unsafe { &*control }))
        .unwrap()
        .unwrap();
    assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
    unsafe { dr_job_control_free(control) };
    (free, controlled)
}

#[test]
fn e_a_controlled_run_equals_the_uncontrolled_one() {
    let doc = DrDocument::from_pixels(pattern(300, 200));
    for filter in ["nearest", "bilinear", "bicubic", "lanczos3"] {
        let (free, controlled) = both(&doc, &affine_set(&[0], rotation(), filter));
        assert_same(&free, &controlled, filter);
    }
    let (free, controlled) = both(
        &doc,
        &affine_set(&[0], [0., 1., -1., 0., 200., 0.], "bicubic"),
    );
    assert_same(&free, &controlled, "quarter turn");
    let (free, controlled) = both(
        &doc,
        &affine_set(&[0], [1., 0., 0., 1., 5., -3.], "lanczos3"),
    );
    assert_same(&free, &controlled, "integral translate");
    assert!(LayerRaster::ptr_eq(
        &controlled.layers[0].pixels,
        &doc.layers[0].pixels
    ));
    assert_eq!(controlled.layers[0].offset, (5, -3));
    let masked = doc.add_mask(0, MaskKind::RevealAll).unwrap();
    let (free, controlled) = both(&masked, &affine_set(&[0], rotation(), "lanczos3"));
    assert!(controlled.layers[0].mask.is_some());
    assert_same(&free, &controlled, "masked layer");
    let sourced = retained((64, 48), (40, 30));
    let (free, controlled) = both(&sourced, &affine_set(&[0, 1], rotation(), "bilinear"));
    assert!(controlled.layers[1].retained_source.is_some());
    assert_same(&free, &controlled, "retained member");
}

#[test]
fn f_byte_transforms_report_rows_inside_each_members_slice() {
    let doc = DrDocument::from_pixels(pattern(300, 200));
    let edit = affine_set(&[0], rotation(), "lanczos3");
    let rows = stage6(&doc, &edit);
    assert!(rows.len() > 2, "{rows:?}");
    assert_non_decreasing(&rows);
    assert!((max_of(&rows) - 1.).abs() <= 1e-6, "{rows:?}");

    // A linked mask resamples after the raster and reports no rows, so the
    // raster's rows end at LINKED_MASK_RASTER_SHARE (0.8) of the member.
    let masked = doc.add_mask(0, MaskKind::RevealAll).unwrap();
    let rows = stage6(&masked, &edit);
    assert!(rows.len() > 2, "{rows:?}");
    assert_non_decreasing(&rows);
    assert!((max_of(&rows) - 0.8).abs() <= 1e-6, "{rows:?}");
}

// ------------------------------------------------ bench (cancel-bench) --

#[test]
#[ignore = "make cancel-bench: a wall-clock verdict"]
fn g_a_24_mp_transform_cancel_returns_within_one_poll() {
    // A 6000 × 4000 raster. `c` is the longest gap between checkpoint-count
    // changes in the worker's own uncancelled timeline over the member; the
    // bound is `T_f + c`, fixed before the probe runs.
    const W: u32 = 6000;
    const H: u32 = 4000;
    let doc = DrDocument::from_pixels(pattern(W, H));
    let cases = [
        ("affine set", affine_set(&[0], rotation(), "lanczos3")),
        (
            "true-perspective set",
            perspective_set(
                &[0],
                [0., 0., f64::from(W), f64::from(H)],
                [0., 0., 5800., 200., 6000., 4000., 0., 4000.],
                "lanczos3",
            ),
        ),
    ];
    for (what, edit) in cases {
        let t = timeline(
            &at_member(0.),
            &|progress, stage, _| stage == 6 && progress >= 1. - 1e-6,
            render(&doc, &edit),
        );
        let (longest_at, longest) = t.longest().expect("a gap");
        let c = longest.as_secs_f64();
        let bound = RETURN_FLOOR_S + c;
        let first = t.0.first().expect("an entry").1;
        let last = t.0.last().expect("an exit").1;
        let span = (last - first).max(2);
        let p = probe(
            &at_member(0.),
            Trigger::After(span / 2),
            &inside_member(0., 1.),
            render(&doc, &edit),
        );
        eprintln!(
            "24 MP ({W}×{H}) {what} cancel inside the resample: {:.3} ms cancel-to-return \
             (issued at {:.1} ms; {}); c {:.3} ms (longest gap, after checkpoint {longest_at}); \
             bound T_f + c = {:.3} ms (T_f 1 ms); ≈{:.2} gaps past the floor; {}",
            p.latency.as_secs_f64() * 1e3,
            p.at.as_secs_f64() * 1e3,
            checkpoints_line(&p),
            c * 1e3,
            bound * 1e3,
            past(p.latency, c),
            verdict(p.latency, bound),
        );
    }
}
