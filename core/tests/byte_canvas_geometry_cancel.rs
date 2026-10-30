//! Byte whole-canvas geometry under a job control: the 8-bit Crop
//! straighten and the filtered Rotate Canvas reach `dr_doc_geometry_edit`'s
//! `straighten` and `rotate_canvas` requests with a control, and a cancel must
//! stop the layer resample or the channel resample and answer "operation
//! cancelled" with no scratch left, never a partial document and never the
//! refusal `Ok(None)`. Every cancel PARKS the worker at a counted checkpoint
//! (`cancelled_parked_at`) taken from a fresh twin's own report log; nothing
//! here races a poller or judges a wall clock. A controlled run that is not
//! cancelled must equal the uncontrolled one exactly, and the full-window
//! straighten is the straighten alone (the pub `straighten_layers` then
//! `transform_channels` oracle).

use doctored_core::doc::{DrDocument, LayerKind, LayerRaster};
use doctored_core::doc_transform::Affine;
use doctored_core::ffi_jobs::*;
use doctored_core::job_control::DrJobControl;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::REFERENCE_WORKING_BYTES;
use image::imageops::FilterType;
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::{json, Value};

use crate::common;
use common::probe::*;

const LIMIT: u64 = REFERENCE_WORKING_BYTES;

/// A turn of `degrees` about the centre of a `w`×`h` canvas, in
/// `CGAffineTransform` order — the matrix the Crop tool's straighten builds.
fn turn(degrees: f64, w: u32, h: u32) -> [f64; 6] {
    let (s, c) = degrees.to_radians().sin_cos();
    let (cx, cy) = (f64::from(w) / 2., f64::from(h) / 2.);
    [c, s, -s, c, cx - cx * c + cy * s, cy - cx * s - cy * c]
}

fn straighten(matrix: [f64; 6], window: (u32, u32, u32, u32), filter: &str) -> GeometryEdit {
    let (x, y, width, height) = window;
    GeometryEdit::parse(
        &json!({"op":"straighten","matrix":matrix,"filter":filter,
            "x":x,"y":y,"width":width,"height":height})
        .to_string(),
    )
    .unwrap()
}

fn rotate_canvas(degrees: f64, filter: &str) -> GeometryEdit {
    GeometryEdit::parse(
        &json!({"op":"rotate_canvas","degrees":degrees,"filter":filter}).to_string(),
    )
    .unwrap()
}

fn pattern(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8, 255])
    })
}

/// A canvas-sized coverage ramp for a saved channel.
fn with_channel(doc: DrDocument) -> DrDocument {
    let (w, h) = (doc.width, doc.height);
    let plane = GrayImage::from_fn(w, h, |x, y| image::Luma([((x * 3 + y * 5) % 256) as u8]));
    doc.add_channel("Saved", plane.as_raw(), w, h, [255, 0, 0], 0.5)
        .unwrap()
}

/// One 1200×800 raster layer.
fn plain() -> DrDocument {
    DrDocument::from_pixels(pattern(1200, 800))
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

fn at_member(from: f64) -> impl Fn(f64, u32, u64) -> bool {
    move |progress, stage, _| stage == 6 && progress >= from - 1e-6
}

fn inside_member(from: f64, to: f64) -> impl Fn(f64, u32) -> bool {
    move |progress, stage| stage == 6 && progress >= from - 1e-6 && progress < to - 1e-6
}

fn assert_unchanged(before: &DrDocument, after: &DrDocument) {
    assert_eq!((before.width, before.height), (after.width, after.height));
    assert_eq!(before.layers.len(), after.layers.len());
    for (a, b) in before.layers.iter().zip(&after.layers) {
        assert!(LayerRaster::ptr_eq(&a.pixels, &b.pixels));
        assert_eq!(a.offset, b.offset);
    }
    assert_eq!(before.channels.len(), after.channels.len());
    for (a, b) in before.channels.iter().zip(&after.channels) {
        assert!(std::sync::Arc::ptr_eq(&a.data, &b.data));
    }
}

#[test]
fn a_a_cancel_parks_inside_the_straighten_layer_resample() {
    let doc = plain();
    let snapshot = doc.clone();
    let edit = straighten(turn(7.5, 1200, 800), (100, 80, 900, 600), "lanczos3");
    let n0 = entry_count(&at_member(0.), render(&doc, &edit));
    let p = cancelled_parked_at(n0 + 1 + 40, &inside_member(0., 1.), render(&doc, &edit));
    eprintln!("straighten layer resample: {}", parked_line(&p));
    assert_unchanged(&snapshot, &doc);
}

#[test]
fn b_a_cancel_parks_inside_the_rotate_canvas_channel_resample() {
    // One layer and one channel: s = 1/2, so the channel runs in 0.5 … 1.
    let doc = with_channel(plain());
    let snapshot = doc.clone();
    let edit = rotate_canvas(17.0, "bilinear");
    let n0 = entry_count(&at_member(0.5), render(&doc, &edit));
    let p = cancelled_parked_at(n0 + 1 + 40, &inside_member(0.5, 1.), render(&doc, &edit));
    eprintln!("rotate canvas channel resample: {}", parked_line(&p));
    assert!(p.progress >= 0.5 - 1e-6 && p.stage == 6);
    assert_unchanged(&snapshot, &doc);
}

#[test]
fn c_a_straighten_reports_rows_per_layer_and_once_per_channel() {
    // Each layer's resample reports its rows inside the layer's
    // third; the channel still reports once, at its start.
    let doc = DrDocument::from_pixels(pattern(300, 200))
        .adding_image_layer(0, pattern(120, 80), "B")
        .unwrap();
    let doc = with_channel(doc);
    let edit = straighten(turn(7.5, 300, 200), (20, 15, 260, 170), "bilinear");
    let log = reports(render(&doc, &edit));
    let members: Vec<f64> = log
        .iter()
        .filter(|r| r.stage == 6)
        .map(|r| r.progress)
        .collect();
    assert!(
        members.windows(2).all(|pair| pair[0] <= pair[1] + 1e-12),
        "{members:?}"
    );
    for want in [0., 1. / 3., 2. / 3.] {
        assert!(
            members.iter().any(|got| (got - want).abs() <= 1e-6),
            "{want} missing: {members:?}"
        );
    }
    for (from, to) in [(0., 1. / 3.), (1. / 3., 2. / 3.)] {
        assert!(
            members
                .iter()
                .any(|&got| got > from + 1e-6 && got < to - 1e-6),
            "no row inside ({from}, {to}): {members:?}"
        );
    }
    let max = members.iter().copied().fold(0., f64::max);
    assert!(max <= 2. / 3. + 1e-6, "{members:?}");
}

fn assert_same(a: &DrDocument, b: &DrDocument, what: &str) {
    assert_eq!((a.width, a.height), (b.width, b.height), "{what}: canvas");
    assert_eq!(a.layers.len(), b.layers.len(), "{what}");
    for (x, y) in a.layers.iter().zip(&b.layers) {
        assert_eq!(x.offset, y.offset, "{what}: offset");
        assert!(
            x.pixels.to_contiguous().unwrap() == y.pixels.to_contiguous().unwrap(),
            "{what}: pixels"
        );
        assert_eq!(x.mask.as_deref(), y.mask.as_deref(), "{what}: mask");
    }
    assert_eq!(a.channels.len(), b.channels.len(), "{what}: channels");
    for (x, y) in a.channels.iter().zip(&b.channels) {
        assert_eq!(x.data.as_raw(), y.data.as_raw(), "{what}: channel");
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
fn d_a_controlled_run_equals_the_uncontrolled_one() {
    let doc = with_channel(DrDocument::from_pixels(pattern(300, 200)));
    let m = turn(7.5, 300, 200);
    let cases = [
        (
            "straighten inner",
            straighten(m, (20, 15, 260, 170), "bicubic"),
        ),
        (
            "straighten full",
            straighten(m, (0, 0, 300, 200), "bilinear"),
        ),
        ("rotate canvas 17", rotate_canvas(17.0, "lanczos3")),
        ("rotate canvas 90", rotate_canvas(90.0, "bilinear")),
    ];
    for (what, edit) in &cases {
        let (free, controlled) = both(&doc, edit);
        assert_same(&free, &controlled, what);
    }
}

#[test]
fn e_the_full_window_straighten_is_the_straighten_alone() {
    let doc = with_channel(DrDocument::from_pixels(pattern(300, 200)));
    let m = turn(5.0, 300, 200);
    let got = doc
        .geometry_edit(&straighten(m, (0, 0, 300, 200), "bilinear"), LIMIT, 0, None)
        .unwrap()
        .expect("a full-window straighten is an edit");
    let oracle = doc
        .straighten_layers(Affine::from_array(m), FilterType::Triangle)
        .unwrap();
    let oracle = oracle
        .transform_channels(Affine::from_array(m), FilterType::Triangle)
        .unwrap();
    assert_same(&got, &oracle, "full window");
    assert_eq!((got.width, got.height), (300, 200));

    // The identity at the full window moves nothing: NULL, no error.
    let identity = [1., 0., 0., 1., 0., 0.];
    assert!(doc
        .geometry_edit(
            &straighten(identity, (0, 0, 300, 200), "bilinear"),
            LIMIT,
            0,
            None
        )
        .unwrap()
        .is_none());
}

#[test]
fn f_an_identity_over_an_inner_window_is_the_crop_bit_for_bit() {
    let doc = with_channel(DrDocument::from_pixels(pattern(300, 200)));
    let identity = [1., 0., 0., 1., 0., 0.];
    let got = doc
        .geometry_edit(
            &straighten(identity, (20, 15, 260, 170), "lanczos3"),
            LIMIT,
            0,
            None,
        )
        .unwrap()
        .expect("an identity over an inner window still crops");
    let oracle = doc.crop(20, 15, 260, 170).unwrap();
    assert_same(&got, &oracle, "identity inner window");
}

#[test]
fn g_a_stack_with_nothing_to_write_is_unchanged_layers_not_a_refusal() {
    // The only entry is a mask-less empty group (the construction PSD import
    // and a crafted `.rz` both produce): `straighten_layers` writes nothing.
    let mut bare = DrDocument::from_pixels(pattern(300, 200));
    bare.layers[0].kind = LayerKind::Group;
    bare.layers[0].pixels = LayerRaster::new(RgbaImage::new(1, 1));
    let saved = with_channel(bare.clone());
    let m = turn(5.0, 300, 200);

    for (what, doc) in [("bare", &bare), ("channel", &saved)] {
        let inner = doc
            .geometry_edit(
                &straighten(m, (20, 15, 260, 170), "bilinear"),
                LIMIT,
                0,
                None,
            )
            .unwrap()
            .unwrap_or_else(|| panic!("{what}: an inner window still crops"));
        assert_eq!((inner.width, inner.height), (260, 170), "{what}");
    }
    let full = (0, 0, 300, 200);
    assert!(
        saved
            .geometry_edit(&straighten(m, full, "bilinear"), LIMIT, 0, None)
            .unwrap()
            .is_some(),
        "the saved channel turns"
    );
    assert!(
        bare.geometry_edit(&straighten(m, full, "bilinear"), LIMIT, 0, None)
            .unwrap()
            .is_none(),
        "nothing to write over the whole canvas is unchanged"
    );
}

#[test]
fn h_a_window_past_the_pixel_limit_refuses_before_any_work() {
    // A window past the canvas is a crop past the canvas; one past
    // the pixel limit is refused by the typed op's sentence.
    let doc = plain();
    let edit = straighten(turn(7.0, 1200, 800), (0, 0, 20_000, 6_000), "bilinear");
    let sentence = "precision prototype dimensions exceed the supported pixel limit";
    assert_eq!(
        doc.geometry_edit_plan(&edit).err().as_deref(),
        Some(sentence)
    );
    let control = unsafe { dr_job_control_create() };
    assert_eq!(
        doc.geometry_edit(&edit, LIMIT, 0, Some(unsafe { &*control }))
            .err()
            .as_deref(),
        Some(sentence)
    );
    assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
    unsafe { dr_job_control_free(control) };
    let identity = straighten([1., 0., 0., 1., 0., 0.], (1, 0, 20_000, 6_000), "bilinear");
    assert_eq!(
        doc.geometry_edit(&identity, LIMIT, 0, None)
            .err()
            .as_deref(),
        Some(sentence)
    );
}

#[test]
fn i_a_zero_size_window_refuses_before_any_work() {
    let doc = plain();
    let sentence = "precision prototype dimensions exceed the supported pixel limit";
    for window in [(10, 10, 0, 100), (10, 10, 100, 0), (0, 0, 0, 0)] {
        for matrix in [turn(7.0, 1200, 800), [1., 0., 0., 1., 0., 0.]] {
            let edit = straighten(matrix, window, "bilinear");
            assert_eq!(
                doc.geometry_edit_plan(&edit).err().as_deref(),
                Some(sentence),
                "plan, window {window:?}"
            );
            let control = unsafe { dr_job_control_create() };
            assert_eq!(
                doc.geometry_edit(&edit, LIMIT, 0, Some(unsafe { &*control }))
                    .err()
                    .as_deref(),
                Some(sentence),
                "edit, window {window:?}"
            );
            assert_eq!(unsafe { dr_job_control_scratch_bytes(control) }, 0);
            unsafe { dr_job_control_free(control) };
        }
    }
}
