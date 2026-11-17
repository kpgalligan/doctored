//! 13B-9b (13A's item 16): the byte sampling plate
//! (`inpaint_source::sampling_source`) reports its progress once per tile,
//! inside the band its caller hands it. Its Tiles arm (depth-0 rasters) and
//! its Tree arm (`doc_regional_plate::regional_cut_plate`, a group) each
//! report `k / tiles` at stage 1 at the head of tile `k`, so a plate on a
//! large document moves the bar instead of holding it at its band's start.
//!
//! The canvas is 2100×150: three tile columns (1024, 1024, 52) by three tile
//! rows (64, 64, 22), nine tiles. A count of reports is load-independent:
//! every case runs to completion under `DrJobControl::record_reports`.
//!
//! * `dr_doc_retouch_source`, sample All, owns the whole band: at least 8
//!   reports at stage 1 with progress strictly inside (0, 1), in
//!   non-decreasing order, for each arm.
//! * `dr_doc_spot_heal_layer_job`, sample All, hands the plate the band
//!   between its own 0.03 and 0.06 reports: at least 8 reports strictly
//!   between those two values come between them, and none outside them.
//!
//! 13A's item 14: the plate's predicate pass (`plate_arm`, one metadata
//! parse per described layer, each polled) runs inside `base +
//! context_scratch`, reserved before it. Under a matrix-free profile a
//! scratch limit above `base` but below `base + context_scratch` refuses
//! the plate before the pass parses anything, so the refusal lands at the
//! same checkpoint count for one described layer as for six.

use std::ffi::c_char;
use std::ptr;
use std::sync::Arc;

use doctored_core::doc::DrDocument;
use doctored_core::ffi::dr_image_free;
use doctored_core::ffi_doc::dr_doc_free;
use doctored_core::ffi_heal::dr_doc_retouch_source;
use doctored_core::ffi_jobs::{dr_doc_spot_heal_layer_job, dr_job_control_set_scratch_limit};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::{DrJobControl, Report};
use image::{Rgba, RgbaImage};

use crate::common::{opaque_pattern, take_err_string};
use crate::icc_lut_builders::{build, tag_desc, tag_mft2, tag_wtpt};

const W: u32 = 2100;
const H: u32 = 150;
/// `DR_RETOUCH_SAMPLE_ALL`.
const ALL: u32 = 2;
/// The tiles past the first: one report each strictly inside the band.
const INNER_TILES: usize = 8;

/// Two depth-0 rasters: the opaque background and a varied layer above it
/// (the Tiles arm).
fn rasters() -> DrDocument {
    let doc = DrDocument::from_pixels(opaque_pattern(W, H));
    let above = RgbaImage::from_fn(W, H, |x, y| {
        Rgba([
            (x * 5 % 256) as u8,
            (y * 11 % 256) as u8,
            ((x + y) % 256) as u8,
            if (x / 7 + y / 5) % 3 == 0 { 120 } else { 255 },
        ])
    });
    doc.adding_image_layer(0, above, "Raster")
        .expect("the raster above")
}

/// [`rasters`] with its upper raster in a group (the Tree arm).
fn grouped() -> DrDocument {
    let (doc, ..) = rasters().group_layers(&[1], "Group").expect("the group");
    doc
}

/// Every report of one `dr_doc_retouch_source` Sample All plate of `doc`.
fn plate_reports(doc: &DrDocument) -> Vec<Report> {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let control = DrJobControl::default();
    control.record_reports();
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_doc_retouch_source(handle, 0, ALL, false, &control, &mut err) };
    unsafe { dr_doc_free(handle) };
    assert!(!image.is_null(), "the plate: {}", take_err_string(err));
    unsafe { dr_image_free(image) };
    control.reports()
}

#[test]
fn the_plate_reports_once_a_tile_through_both_arms() {
    let mut failures = Vec::new();
    for (arm, doc) in [("tiles", rasters()), ("tree", grouped())] {
        let reports = plate_reports(&doc);
        let inner = reports
            .iter()
            .filter(|r| r.stage == 1 && r.progress > 0.0 && r.progress < 1.0)
            .count();
        if inner < INNER_TILES {
            failures.push(format!(
                "{arm}: {inner} reports inside (0, 1) at stage 1: {reports:?}"
            ));
        }
        if !reports.windows(2).all(|p| p[0].progress <= p[1].progress) {
            failures.push(format!("{arm}: progress went back: {reports:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The first report at `stage` whose progress is `value`.
fn position(reports: &[Report], stage: u32, value: f64) -> usize {
    reports
        .iter()
        .position(|r| r.stage == stage && (r.progress - value).abs() < 1e-9)
        .unwrap_or_else(|| panic!("no report of {value} at stage {stage}: {reports:?}"))
}

#[test]
fn a_spot_heal_plate_reports_inside_its_band() {
    let doc = Box::into_raw(Box::new(rasters()));
    // A 12×12 dab in the middle tile.
    let overlay: Vec<u8> = (0..W * H)
        .flat_map(|i| {
            let (x, y) = (i % W, i / W);
            let a = if (1000..1012).contains(&x) && (70..82).contains(&y) {
                255
            } else {
                0
            };
            [0, 0, 0, a]
        })
        .collect();
    let control = DrJobControl::default();
    control.record_reports();
    let mut err: *mut c_char = ptr::null_mut();
    let out = unsafe {
        dr_doc_spot_heal_layer_job(
            doc,
            1,
            overlay.as_ptr(),
            W,
            H,
            1.0,
            0,
            7,
            0,
            ALL,
            false,
            &control,
            &mut err,
        )
    };
    unsafe { dr_doc_free(doc) };
    assert!(!out.is_null(), "the spot heal: {}", take_err_string(err));
    unsafe { dr_doc_free(out) };
    let reports = control.reports();
    let (start, end) = (position(&reports, 1, 0.03), position(&reports, 2, 0.06));
    let band = &reports[start + 1..end];
    assert!(
        band.iter().all(|r| (0.03..=0.06).contains(&r.progress)),
        "a report between 0.03 and 0.06 left the band: {band:?}"
    );
    let inner = band
        .iter()
        .filter(|r| r.progress > 0.03 + 1e-9 && r.progress < 0.06 - 1e-9)
        .count();
    assert!(
        inner >= INNER_TILES,
        "{inner} reports strictly inside the plate's band: {band:?}"
    );
}

/// [`described`]'s canvas side.
const SIDE: u32 = 64;

/// A matrix-free RGB profile (`A2B0`/`B2A0` tables only), under which
/// `context_scratch` charges the description parse.
fn lut_profile() -> Arc<IccProfile> {
    let bytes = build(
        [0x02, 0x20, 0, 0],
        b"mntr",
        b"RGB ",
        b"XYZ ",
        &[
            (b"desc", tag_desc("Synthetic LUT RGB")),
            (b"wtpt", tag_wtpt([0.9642, 1.0, 0.8249])),
            (b"A2B0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
            (b"B2A0", tag_mft2(3, 3, 2, &|c| c.to_vec())),
        ],
    );
    let profile = Arc::new(IccProfile::parse(&bytes).expect("the LUT profile parses"));
    assert!(
        profile.model().is_none(),
        "the profile has no matrix/TRC model"
    );
    profile
}

/// The one description every [`described`] raster carries: opaque metadata
/// (no adjustment), so every arm admits it and only its length matters.
fn description() -> Arc<str> {
    format!(r#"{{"type":"note","text":"{}"}}"#, "x".repeat(4096)).into()
}

/// The background and `count` rasters above it, each carrying
/// [`description`], under [`lut_profile`]: the predicate pass parses `count`
/// metas, while `context_scratch` (the longest meta × 256) is the same for
/// every `count`.
fn described(count: usize) -> DrDocument {
    let mut doc = DrDocument::from_pixels(opaque_pattern(SIDE, SIDE));
    for i in 0..count {
        doc = doc
            .adding_image_layer(i, opaque_pattern(SIDE, SIDE), "Described")
            .expect("a described raster");
    }
    let meta = description();
    for layer in doc.layers.iter_mut().skip(1) {
        layer.meta = Some(meta.clone());
    }
    doc.profile = lut_profile();
    doc
}

/// The checkpoint count and the error of one `dr_doc_retouch_source` Sample
/// All plate of `doc` under the scratch `limit`, which must refuse it.
fn refused_at(doc: &DrDocument, limit: u64) -> (u64, String) {
    let handle = Box::into_raw(Box::new(doc.clone()));
    let control = DrJobControl::default();
    assert!(unsafe { dr_job_control_set_scratch_limit(&control, limit) });
    let mut err: *mut c_char = ptr::null_mut();
    let image = unsafe { dr_doc_retouch_source(handle, 0, ALL, false, &control, &mut err) };
    unsafe { dr_doc_free(handle) };
    if !image.is_null() {
        unsafe { dr_image_free(image) };
        panic!("a {limit}-byte limit admitted the plate");
    }
    (control.checkpoints(), take_err_string(err))
}

#[test]
fn the_predicate_pass_runs_inside_its_reservation() {
    // `dr_doc_retouch_source`'s base, restated: the plate it will hold plus
    // `precision_image::BOOKKEEPING_BYTES` (1 MiB).
    let base = u64::from(SIDE * SIDE) * 4 + 1024 * 1024;
    let context = description().len() as u64 * 256;
    // Above the base and below `base + context`: only the context term
    // can refuse.
    let limit = base + context / 2;
    let (one, one_err) = refused_at(&described(1), limit);
    let (six, six_err) = refused_at(&described(6), limit);
    for err in [&one_err, &six_err] {
        assert!(
            err.contains("working memory"),
            "the refusal is the scratch limit's: {err}"
        );
    }
    assert_eq!(
        one, six,
        "the refusal must land before the predicate pass polls any described layer"
    );
}
