//! The filtered whole-canvas turn (8-bit Rotate Canvas and Straighten)
//! charges its destination once, a measured figure (`doc_source_geometry`'s
//! `# Admission: a filtered turn's destination`), so a 12 MP photo turns by
//! 17° under the working limit. HEAD charged six destination copies and
//! refused the rotate. A straighten's saved channels are charged apart, at
//! `(canvas + output) × C`. The layer and canvas terms are measured too (its
//! `# Admission: a filtered turn's layer and canvas terms`), and the saved
//! channels of both turns take their own 5/4.
//! The typed filtered turn's work is bounded by `MAX_DESTRUCTIVE_WORK`,
//! so a 24 MP 16-bit photo turns at Catmull-Rom; an `interactive` straighten
//! (the Crop tool's preview) keeps `MAX_WORK_UNITS`.
//! A typed transform commit (`affine`, `perspective`, `affine_set`,
//! `perspective_set`) takes the same one-shot budget with a measured 5/4
//! headroom; an `interactive` transform (every ⌘T preview) keeps the cap.
//! A placed image (a retained source) under those commits and the
//! direct source `transform` takes the same one-shot budget; its previews
//! keep the cap.
//! The typed native reopen envelope's decode term is the measured
//! `NATIVE_DECODE_BYTES_PER_PIXEL` (27, was 32), so a 5961×3974 placed image
//! (then the largest 3:2 size) commits.
//! The native RGBA16 PNG bound is the exact stored length, so a
//! 6000×4000 placed image commits (6144×4096 was then the largest 3:2 size).
//! The banded reopen decodes PNG rows straight into tiles, so the
//! envelope is the file, the planes and one band plus a quarter.
//! The reopen envelope charges header bytes and a RAW payload twice
//! because the reader holds them twice; a RAW-backed turn reopens within its
//! envelope.
//! A re-rendered placed layer's never-run raster plane adds no work units,
//! and its scratch is charged again for previews and commits alike; an 8-bit
//! placed transform commit takes the one-shot budget too.
//! An exact edit of a placed layer (quarter turn, flip, integer move,
//! an exact 8-bit Rotate Canvas) counts only the planes it runs: its
//! never-run source render adds no work units.

use doctored_core::doc::{DrDocument, MaskKind};
use doctored_core::doc_rotate::rotated_canvas;
use doctored_core::ffi_raw_source::dr_precision_doc_attach_raw_source;
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::{Metadata, Resolution};
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::precision_render::MAX_WORK_UNITS;
use doctored_core::retained_source::SourceEdit;
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::json;
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use crate::common;

/// The working limit of the assumed minimum machine, a 16 GB Mac.
const MINIMUM_MAC_LIMIT: u64 = 8 << 30;

/// A turn by `degrees` about (`cx`, `cy`) in `CGAffineTransform` order — the
/// matrix the Crop tool's straighten builds (`byte_canvas_geometry_cancel.rs`'s
/// `turn`).
fn turn_about(degrees: f64, cx: f64, cy: f64) -> [f64; 6] {
    let (s, c) = degrees.to_radians().sin_cos();
    [c, s, -s, c, cx - cx * c + cy * s, cy - cx * s - cy * c]
}

/// A 17° turn about (2000, 1500).
fn turn() -> [f64; 6] {
    turn_about(17., 2000., 1500.)
}

fn rotate() -> GeometryEdit {
    GeometryEdit::parse(&json!({"op":"rotate_canvas","degrees":17,"filter":"bilinear"}).to_string())
        .unwrap()
}

fn straighten() -> GeometryEdit {
    GeometryEdit::parse(
        &json!({"op":"straighten","matrix":turn(),"filter":"bilinear",
            "x":200,"y":150,"width":3600,"height":2700})
        .to_string(),
    )
    .unwrap()
}

fn photo() -> DrDocument {
    common::doc_of(4000, 3000, common::plasma)
}

#[test]
fn a_photo_rotates_and_straightens_by_17_degrees_at_8_bits() {
    let doc = common::doc_of(400, 300, common::plasma);
    let rotated = doc
        .geometry_edit(&rotate(), REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .expect("a 17° rotate is admitted");
    let canvas = rotated_canvas(400, 300, 17.).unwrap();
    assert_eq!(
        (rotated.width, rotated.height),
        (canvas.width, canvas.height)
    );
    drop(rotated);
    let straighten = GeometryEdit::parse(
        &json!({"op":"straighten","matrix":turn_about(17., 200., 150.),"filter":"bilinear",
            "x":20,"y":15,"width":360,"height":270})
        .to_string(),
    )
    .unwrap();
    let straightened = doc
        .geometry_edit(&straighten, REFERENCE_WORKING_BYTES, 0, None)
        .unwrap()
        .expect("a 17° straighten is admitted");
    assert_eq!((straightened.width, straightened.height), (360, 270));
}

#[test]
fn the_turn_plans_pin_their_measured_figures() {
    let doc = photo();
    // Rotate max E 200,786,112 (1.25 × = 250,982,640), straighten
    // 200,802,496 (1.25 × = 251,003,120). Each figure is 48 B below its
    // value before 2026-10-10: the layer record lost its live-filter cache,
    // live-filter stack and Stack Mode fields, and the plan counts the
    // record's cloned structure. Each is 24 B above that since the layer
    // reference point (the record's `reference` and its plan entry's
    // `reference_shift`), and 168 B above that since the record gained its
    // 56 B Generative Layer binding (`generated`), counted three times. The
    // rotated canvas became ceil(bw) × ceil(bh), 4703 × 4039 where the
    // symmetric box was 4704 × 4040: the rotate's plan charges 12 B per
    // output canvas pixel (growth × 8 plus the canvas-sized layer's 4 B
    // scaled by growth), so it fell by 12 × (19,004,160 − 18,995,417) =
    // 104,916 from 404,262,482. Each is 120 B above that since each of the
    // record's five inline rasters (its pixels and the source, warp and
    // generated renders) gained its 8 B alpha-fact cell, counted three times.
    for (edit, figure, floor) in [
        (rotate(), 404_157_686, 250_982_640),
        (straighten(), 320_212_682, 251_003_120),
    ] {
        let plan = doc.geometry_edit_plan(&edit).unwrap();
        assert_eq!(plan.scratch_bytes, figure);
        assert!(plan.scratch_bytes >= floor);
    }
}

fn solid(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba([90, 120, 150, 255]))
}

/// One case of `doc_source_geometry`'s
/// `# Admission: a filtered turn's layer and canvas terms`: the plan covers
/// 1.25 × its measured max E.
fn covers(doc: &DrDocument, edit: &GeometryEdit, floor: u64) {
    let plan = doc.geometry_edit_plan(edit).unwrap();
    assert!(
        plan.scratch_bytes >= floor,
        "{} < {floor}",
        plan.scratch_bytes
    );
}

#[test]
fn every_measured_turn_plan_covers_its_peak() {
    // T3 / T4: four full-canvas layers, each with a reveal-all mask.
    let mut masked = DrDocument::from_pixels(solid(4000, 3000));
    for i in 0..3 {
        masked = masked
            .adding_image_layer(i, solid(4000, 3000), "Layer")
            .unwrap();
    }
    for i in 0..4 {
        masked = masked.add_mask(i, MaskKind::RevealAll).unwrap();
    }
    covers(&masked, &rotate(), 633_037_280); // max E 506,429,824
    covers(&masked, &straighten(), 633_057_760); // max E 506,446,208
    drop(masked);
    // T5: an 8000×6000 photo. HEAD planned 2,755,900,992 bytes of scratch
    // (2,947,916,955 with its retained figure) and refused the rotate.
    let large = DrDocument::from_pixels(solid(8000, 6000));
    covers(&large, &rotate(), 1_002_271_470); // max E 801,817,176
    drop(large);
    // T7: 32 saved channels.
    let plane = GrayImage::from_pixel(4000, 3000, image::Luma([128]));
    let mut channels = DrDocument::from_pixels(solid(4000, 3000));
    for _ in 0..32 {
        channels = channels
            .add_channel("Saved", plane.as_raw(), 4000, 3000, [255, 0, 0], 0.5)
            .unwrap();
    }
    covers(&channels, &rotate(), 1_012_204_330); // max E 809,763,464
    drop(channels);
    // T8 / T9: one 8000×6000 layer overhanging the 4000×3000 canvas.
    let overhang = DrDocument::from_pixels(solid(4000, 3000))
        .adding_image_layer(0, solid(8000, 6000), "Big")
        .unwrap()
        .with_layer_offset(1, -2000, -1500)
        .unwrap()
        .removing_layer(0)
        .unwrap();
    covers(&overhang, &straighten(), 943_084_180); // max E 754,467,344
    covers(&overhang, &rotate(), 942_285_490); // max E 753,828,392
}

#[test]
fn a_channel_heavy_12_mp_straighten_charges_both_channel_planes() {
    // Each saved channel is resampled on the full canvas and then cropped,
    // both live: 5/4 × 40 × (12,000,000 + 9,720,000) on top of the plain
    // plan. The measured max E is 1,060,193,144; 1.25 × max E =
    // 1,325,241,430. The edit is admitted, consistently with that
    // measurement.
    let plane = GrayImage::from_fn(4000, 3000, |x, y| {
        image::Luma([((x * 3 + y * 5) % 256) as u8])
    });
    let mut doc = photo();
    for _ in 0..40 {
        doc = doc
            .add_channel("Saved", plane.as_raw(), 4000, 3000, [255, 0, 0], 0.5)
            .unwrap();
    }
    let plan = doc.geometry_edit_plan(&straighten()).unwrap();
    assert_eq!(
        plan.scratch_bytes,
        // + the 40 channel records' cloned structure (93 bytes each: the
        // record, its spot ink slot included, and the five-byte name).
        320_212_682 + 40 * (12_000_000 + 9_720_000) * 5 / 4 + 40 * 93
    );
    assert_eq!(plan.scratch_bytes, 1_406_216_402);
    assert!(plan.scratch_bytes >= 1_325_241_430);
}

/// An opaque `w`×`h` RGBA16 sRGB photo: `common::plasma` × 257.
fn photo16(w: u32, h: u32) -> PrecisionDocument {
    let pixels = (0..w * h)
        .map(|i| {
            let c = common::plasma(i % w, i / w);
            [
                u16::from(c[0]) * 257,
                u16::from(c[1]) * 257,
                u16::from(c[2]) * 257,
                65535,
            ]
        })
        .collect();
    PrecisionDocument::from_image(
        PrecisionImage::from_rgba16(w, h, pixels, IccProfile::srgb(), None).unwrap(),
        Metadata::default(),
        Resolution { x: 72., y: 72. },
    )
    .unwrap()
}

#[test]
fn a_16_bit_12_mp_turn_plan_covers_its_peak() {
    let doc = photo16(4000, 3000);
    // The typed filtered turn takes 1.25 × its measured max E (401,129,832).
    for edit in [rotate(), straighten()] {
        let plan = doc.geometry_plan(&edit).unwrap();
        assert!(plan.scratch_bytes >= 501_412_290);
    }
}

#[test]
fn a_24_mp_16_bit_photo_turns_with_catmull_rom() {
    // The typed filtered turn's work is bounded by the one-shot
    // `MAX_DESTRUCTIVE_WORK`, so a 6000×4000 16-bit photo straightens and
    // rotates at the default Catmull-Rom (`bicubic`) sampler. Floors are
    // 1.25 × the measured max E (`precision_geometry_budget`'s record):
    // S7 678,511,096, R17 810,926,680.
    let doc = photo16(6000, 4000);
    let s7 = GeometryEdit::parse(
        &json!({"op":"straighten","matrix":turn_about(-7., 3000., 2000.),
            "filter":"bicubic","x":300,"y":200,"width":5400,"height":3600})
        .to_string(),
    )
    .unwrap();
    let r17 = GeometryEdit::parse(
        &json!({"op":"rotate_canvas","degrees":17,"filter":"bicubic"}).to_string(),
    )
    .unwrap();
    for (edit, floor) in [(s7, 848_138_870), (r17, 1_013_658_350)] {
        let plan = doc.geometry_plan(&edit).unwrap();
        // The interactive cap would refuse it.
        assert!(plan.work_units > MAX_WORK_UNITS, "{}", plan.work_units);
        assert!(
            plan.scratch_bytes >= floor,
            "{} < {floor}",
            plan.scratch_bytes
        );
    }
    // The Crop tool's preview sends `interactive` and keeps the interactive
    // cap (a preview reruns on every angle change), so a Catmull-Rom preview
    // stays unavailable there while the commit above is admitted.
    let preview = GeometryEdit::parse(
        &json!({"op":"straighten","matrix":turn_about(-7., 3000., 2000.),
            "filter":"bicubic","x":300,"y":200,"width":5400,"height":3600,
            "interactive":true})
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        doc.geometry_plan(&preview).err().as_deref(),
        Some("typed geometry exceeds the bounded pixel-work limit")
    );
}

/// The four 24 MP transform cases (`precision_geometry_budget`'s record): the
/// request, then the floor 1.25 × its measured max E.
fn transform_cases(interactive: bool) -> Vec<(&'static str, GeometryEdit, u64)> {
    let matrix = turn_about(17., 3000., 2000.);
    let quad = [0., 0., 6000., 0., 6900., 4600., -900., 4600.];
    let mut requests = [
        (
            "A17",
            json!({"op":"affine","layer":0,"target":"pixels","matrix":matrix,
                "filter":"bicubic"}),
            1_013_719_790, // max E 810,975,832
        ),
        (
            "S17",
            json!({"op":"affine_set","layers":[0],"matrix":matrix,"filter":"bicubic"}),
            1_013_719_790, // max E 810,975,832
        ),
        (
            "P",
            json!({"op":"perspective","layer":0,"target":"pixels","quad":quad,
                "filter":"bicubic"}),
            959_816_430, // max E 767,853,144
        ),
        (
            "PS",
            json!({"op":"perspective_set","layers":[0],"source_rect":[0,0,6000,4000],
                "quad":quad,"filter":"bicubic"}),
            959_795_950, // max E 767,836,760
        ),
    ];
    requests
        .iter_mut()
        .map(|(name, request, floor)| {
            if interactive {
                request["interactive"] = json!(true);
            }
            (
                *name,
                GeometryEdit::parse(&request.to_string()).unwrap(),
                *floor,
            )
        })
        .collect()
}

#[test]
fn a_24_mp_16_bit_layer_transform_commits_with_catmull_rom() {
    let doc = photo16(6000, 4000);
    for (name, edit, floor) in transform_cases(false) {
        let plan = doc.geometry_plan(&edit).unwrap();
        // The interactive cap would refuse it.
        assert!(
            plan.work_units > MAX_WORK_UNITS,
            "{name} {}",
            plan.work_units
        );
        assert!(
            plan.scratch_bytes >= floor,
            "{name} {} < {floor}",
            plan.scratch_bytes
        );
    }
    // Every ⌘T preview sends `interactive` and keeps the interactive cap.
    for (name, edit, _) in transform_cases(true) {
        assert_eq!(
            doc.geometry_plan(&edit).err().as_deref(),
            Some("typed geometry exceeds the bounded pixel-work limit"),
            "{name}"
        );
    }
}

#[test]
fn an_interactive_transform_gives_the_commit_s_result() {
    let doc = photo16(64, 48);
    let matrix = turn_about(17., 32., 24.);
    let quad = [0., 0., 64., 0., 74., 52., -10., 52.];
    for request in [
        json!({"op":"affine","layer":0,"target":"pixels","matrix":matrix,"filter":"bicubic"}),
        json!({"op":"perspective_set","layers":[0],"source_rect":[0,0,64,48],
            "quad":quad,"filter":"bicubic"}),
    ] {
        let mut preview = request.clone();
        preview["interactive"] = json!(true);
        let [commit, preview] = [request, preview].map(|r| {
            doc.geometry(
                &GeometryEdit::parse(&r.to_string()).unwrap(),
                REFERENCE_WORKING_BYTES,
                0,
                None,
            )
            .unwrap()
            .unwrap()
        });
        assert_eq!(commit.dimensions(), preview.dimensions());
        assert_eq!(
            commit.layer(0).unwrap().offset(),
            preview.layer(0).unwrap().offset()
        );
        let commit_samples = commit.layer_source(0).map(|p| p.to_rgba16().unwrap());
        let preview_samples = preview.layer_source(0).map(|p| p.to_rgba16().unwrap());
        assert!(commit_samples.is_some());
        assert_eq!(commit_samples, preview_samples);
    }
}

/// `photo16` whose only layer is bound to a retained source (`retain`): a
/// placed image.
fn placed16(w: u32, h: u32) -> PrecisionDocument {
    photo16(w, h)
        .source_edit(
            None,
            &SourceEdit::parse(r#"{"op":"retain","source_id":"m373"}"#).unwrap(),
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap()
}

#[test]
fn a_21_mp_16_bit_placed_image_commits_with_catmull_rom() {
    // The placed-image record (`precision_geometry_budget`): a 5700×3800
    // placed image — the largest 3:2 16-bit size whose 17° result fit the
    // native reopen envelope before its decode term was measured (5961×3974
    // after that, 6144×4096 since the exact PNG bound, the test below) —
    // commits at Catmull-Rom by the one-shot `MAX_DESTRUCTIVE_WORK`. Floors
    // are 1.25 × the measured max E.
    let doc = placed16(5700, 3800);
    let matrix = turn_about(17., 2850., 1900.);
    let quad = [0., 0., 5700., 0., 6555., 4370., -855., 4370.];
    let source = |interactive: bool| {
        let mut request = json!({"op":"transform","layer":0,"matrix":matrix,"filter":"bicubic"});
        if interactive {
            request["interactive"] = json!(true);
        }
        SourceEdit::parse(&request.to_string()).unwrap()
    };
    let geometry = |mut request: serde_json::Value, interactive: bool| {
        if interactive {
            request["interactive"] = json!(true);
        }
        GeometryEdit::parse(&request.to_string()).unwrap()
    };
    let affine = json!({"op":"affine","layer":0,"target":"pixels","matrix":matrix,
        "filter":"bicubic"});
    let set = json!({"op":"affine_set","layers":[0],"matrix":matrix,"filter":"bicubic"});
    let perspective = json!({"op":"perspective","layer":0,"target":"pixels","quad":quad,
        "filter":"bicubic"});
    let rotate = json!({"op":"rotate_canvas","degrees":17,"filter":"bicubic"});
    let plans = [
        (
            "A17 source",
            doc.source_plan(None, &source(false)),
            914_965_170,
        ), // max E 731,972,136
        (
            "A17 affine",
            doc.geometry_plan(&geometry(affine.clone(), false))
                .map(|p| doctored_core::retained_source::SourcePlan {
                    retained_bytes: p.retained_bytes,
                    scratch_bytes: p.scratch_bytes,
                    work_units: p.work_units,
                }),
            914_965_170,
        ),
    ];
    for (name, plan, floor) in plans {
        let plan = plan.unwrap();
        assert!(
            plan.work_units > MAX_WORK_UNITS,
            "{name} {}",
            plan.work_units
        );
        assert!(
            plan.scratch_bytes >= floor,
            "{name} {} < {floor}",
            plan.scratch_bytes
        );
        let total = plan.retained_bytes + plan.scratch_bytes;
        println!("{name}: retained + scratch {total}");
        assert!(total <= MINIMUM_MAC_LIMIT, "{name}");
    }
    // The set's never-run raster plane adds no work units, so the set
    // counts exactly the source render the direct `transform` counts.
    assert_eq!(
        doc.geometry_plan(&geometry(set.clone(), false))
            .unwrap()
            .work_units,
        doc.source_plan(None, &source(false)).unwrap().work_units
    );
    for (name, edit, floor) in [
        ("S17", geometry(set.clone(), false), 914_944_690), // max E 731,955,752
        ("P", geometry(perspective.clone(), false), 866_878_100), // max E 693,502,480
        ("R17", geometry(rotate, false), 914_965_170),      // max E 731,972,136
    ] {
        let plan = doc.geometry_plan(&edit).unwrap();
        // The interactive cap would refuse it.
        assert!(
            plan.work_units > MAX_WORK_UNITS,
            "{name} {}",
            plan.work_units
        );
        assert!(
            plan.scratch_bytes >= floor,
            "{name} {} < {floor}",
            plan.scratch_bytes
        );
        let total = plan.retained_bytes + plan.scratch_bytes;
        println!("{name}: retained + scratch {total}");
        assert!(total <= MINIMUM_MAC_LIMIT, "{name}");
    }
    // Every ⌘T preview sends `interactive` and keeps the interactive cap.
    let refused = |name: &str, result: Result<(), String>| {
        assert!(
            result
                .as_ref()
                .err()
                .is_some_and(|e| e.contains("bounded sampling work limit")),
            "{name} {result:?}"
        );
    };
    refused(
        "A17 source",
        doc.source_plan(None, &source(true)).map(|_| ()),
    );
    for (name, request) in [("A17 affine", affine), ("S17", set), ("P", perspective)] {
        refused(
            name,
            doc.geometry_plan(&geometry(request, true)).map(|_| ()),
        );
    }
}

#[test]
fn an_interactive_source_transform_gives_the_commit_s_result() {
    let doc = placed16(64, 48);
    let mut request = json!({"op":"transform","layer":0,"matrix":turn_about(17., 32., 24.),
        "filter":"bicubic"});
    let commit = SourceEdit::parse(&request.to_string()).unwrap();
    request["interactive"] = json!(true);
    let preview = SourceEdit::parse(&request.to_string()).unwrap();
    let [commit, preview] = [commit, preview].map(|edit| {
        doc.source_edit(None, &edit, REFERENCE_WORKING_BYTES, 0, None)
            .unwrap()
            .unwrap()
    });
    assert_eq!(
        commit.layer(0).unwrap().offset(),
        preview.layer(0).unwrap().offset()
    );
    let commit_samples = commit.layer_source(0).map(|p| p.to_rgba16().unwrap());
    assert!(commit_samples.is_some());
    assert_eq!(
        commit_samples,
        preview.layer_source(0).map(|p| p.to_rgba16().unwrap())
    );
    assert_eq!(commit.source_info(0), preview.source_info(0));
}

/// The A17 source, A17 `affine`, S17, P and R17 commits of a `w`×`h` placed
/// image: the name, the plan and the floor index (0 = turn, 1 = perspective).
fn placed_commit_plans(
    doc: &PrecisionDocument,
    (w, h): (u32, u32),
) -> Vec<(
    &'static str,
    Result<doctored_core::retained_source::SourcePlan, String>,
    usize,
)> {
    let (fw, fh) = (f64::from(w), f64::from(h));
    let matrix = turn_about(17., fw / 2., fh / 2.);
    let quad = [0., 0., fw, 0., fw * 1.15, fh * 1.15, -fw * 0.15, fh * 1.15];
    let geometry = |request: serde_json::Value| {
        doc.geometry_plan(&GeometryEdit::parse(&request.to_string()).unwrap())
            .map(|p| doctored_core::retained_source::SourcePlan {
                retained_bytes: p.retained_bytes,
                scratch_bytes: p.scratch_bytes,
                work_units: p.work_units,
            })
    };
    vec![
        (
            "A17 source",
            doc.source_plan(
                None,
                &SourceEdit::parse(
                    &json!({"op":"transform","layer":0,"matrix":matrix,"filter":"bicubic"})
                        .to_string(),
                )
                .unwrap(),
            ),
            0,
        ),
        (
            "A17 affine",
            geometry(
                json!({"op":"affine","layer":0,"target":"pixels","matrix":matrix,
                "filter":"bicubic"}),
            ),
            0,
        ),
        (
            "S17",
            geometry(json!({"op":"affine_set","layers":[0],"matrix":matrix,"filter":"bicubic"})),
            0,
        ),
        (
            "P",
            geometry(
                json!({"op":"perspective","layer":0,"target":"pixels","quad":quad,
                "filter":"bicubic"}),
            ),
            1,
        ),
        (
            "R17",
            geometry(json!({"op":"rotate_canvas","degrees":17,"filter":"bicubic"})),
            0,
        ),
    ]
}

#[test]
fn a_24_mp_16_bit_placed_image_commits_with_catmull_rom() {
    // With the exact stored PNG length (`precision_png`'s `# Encoded bound`)
    // the native reopen envelope admits these commits of a 6000×4000 placed
    // image (5961×3974 was the largest before). Floors are 1.25 × the max E
    // measured at this size (`precision_geometry_budget`'s record): turns
    // 810,910,272, perspective 767,771,200.
    let floors = [1_013_637_840, 959_714_000];
    let doc = placed16(6000, 4000);
    for (name, plan, floor) in placed_commit_plans(&doc, (6000, 4000)) {
        let plan = plan.unwrap();
        assert!(
            plan.work_units > MAX_WORK_UNITS,
            "{name} {}",
            plan.work_units
        );
        assert!(
            plan.scratch_bytes >= floors[floor],
            "{name} {} < {}",
            plan.scratch_bytes,
            floors[floor]
        );
        let total = plan.retained_bytes + plan.scratch_bytes;
        println!("{name}: retained + scratch {total}");
        assert!(total <= MINIMUM_MAC_LIMIT, "{name}");
    }
}

/// A `len`-byte xorshift RAW original attached to layer 0
/// through the FFI, with an opaque descriptor.
fn with_raw(doc: &PrecisionDocument, len: usize) -> PrecisionDocument {
    let descriptor = CString::new(
        json!({"schema_version":1,"original_filename":"camera.NEF",
            "content_sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "decoder":{"identifier":"opaque","version":"99","implementation":"unavailable"},
            "recipe":{"version":99}})
        .to_string(),
    )
    .unwrap();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let bytes: Vec<u8> = (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    let mut error: *mut c_char = ptr::null_mut();
    let attached = unsafe {
        dr_precision_doc_attach_raw_source(
            doc,
            0,
            bytes.as_ptr(),
            bytes.len(),
            descriptor.as_ptr(),
            REFERENCE_WORKING_BYTES,
            0,
            ptr::null(),
            &mut error,
        )
    };
    assert!(!attached.is_null(), "{}", unsafe {
        CStr::from_ptr(error).to_string_lossy()
    });
    *unsafe { Box::from_raw(attached) }
}

#[test]
fn a_raw_backed_turned_placed_image_reopens_within_its_envelope() {
    let turned = with_raw(&placed16(320, 240), 1_000_000)
        .geometry(
            &GeometryEdit::parse(
                &json!({"op":"rotate_canvas","degrees":17,"filter":"bicubic"}).to_string(),
            )
            .unwrap(),
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let plan = turned.native_plan().unwrap();
    let bytes = turned
        .encode_native(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    let reopened = PrecisionDocument::parse_native(
        &bytes,
        plan.reopen_retained_bytes + plan.decode_scratch_bytes,
        0,
        None,
    )
    .unwrap();
    assert!(reopened.retained_bytes() <= plan.reopen_retained_bytes);
}

/// The stored length of a native RGBA16 PNG (`native_png_bound.rs`'s
/// formula): row bytes with a filter byte each, the zlib stream's stored
/// blocks and framing, then signature, IHDR, one IDAT and IEND.
fn png_len(w: u32, h: u32) -> u64 {
    let r = u64::from(h) * (8 * u64::from(w) + 1);
    let z = 2 + r + 5 * r.div_ceil(65_535) + 4;
    8 + (12 + 13) + z + 12 * z.div_ceil((1 << 31) - 1) + 12
}

#[test]
fn the_native_decode_scratch_has_no_whole_plane_cap() {
    // The banded reopen: base = retained + held + one band (16 rows of
    // 8·64 + 1 bytes beside the 4 MiB codec allowance) + the 1 MiB
    // bookkeeping, plus a quarter. `held` is the file less its two stored
    // PNGs (the 64×48 raster and its 64×48 source, read as spans) plus the
    // reader's 4 MiB window. There is no cap at the 27 B/px whole-plane
    // figure, which the quarter above base exceeds at so small a size.
    let plan = placed16(64, 48).native_plan().unwrap();
    let held = plan.encoded_bound - 2 * png_len(64, 48) + 4 * 1024 * 1024;
    let base =
        plan.reopen_retained_bytes + held + 16 * (8 * 64 + 1) + 4 * 1024 * 1024 + 1024 * 1024;
    assert_eq!(
        plan.decode_scratch_bytes,
        base + base.div_ceil(4) - plan.reopen_retained_bytes
    );
    assert!(plan.decode_scratch_bytes > held + 27 * 3072 + 4 * 1024 * 1024 + 1024 * 1024);
}

#[test]
fn a_turned_placed_image_reopens_within_its_envelope() {
    let turned = placed16(320, 240)
        .geometry(
            &GeometryEdit::parse(
                &json!({"op":"rotate_canvas","degrees":17,"filter":"bicubic"}).to_string(),
            )
            .unwrap(),
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap();
    let plan = turned.native_plan().unwrap();
    let bytes = turned
        .encode_native(REFERENCE_WORKING_BYTES, 0, None)
        .unwrap();
    let reopened = PrecisionDocument::parse_native(
        &bytes,
        plan.reopen_retained_bytes + plan.decode_scratch_bytes,
        0,
        None,
    )
    .unwrap();
    assert!(reopened.retained_bytes() <= plan.reopen_retained_bytes);
}

#[test]
fn a_placed_preview_charges_the_raster_plane() {
    // The preview again charges the raster plane it never runs, beside the
    // source render, so it exceeds an unplaced photo's preview; the commit is
    // exactly its 5/4 headroom over the preview.
    let doc = placed16(640, 480);
    let commit = json!({"op":"affine_set","layers":[0],"matrix":turn_about(17., 320., 240.),
            "filter":"bicubic"});
    let mut preview = commit.clone();
    preview["interactive"] = json!(true);
    let edit = |request: &serde_json::Value| GeometryEdit::parse(&request.to_string()).unwrap();
    let unplaced = photo16(640, 480)
        .geometry_plan(&edit(&preview))
        .unwrap()
        .scratch_bytes;
    let [commit, preview] =
        [commit, preview].map(|request| doc.geometry_plan(&edit(&request)).unwrap().scratch_bytes);
    assert_eq!(commit, preview * 5 / 4);
    assert!(preview > unplaced, "{preview} <= {unplaced}");
}

#[test]
fn a_placed_transform_reports_progress_to_the_end() {
    // A re-rendered layer's progress range is its source render's
    // units, so the render reaches the end of the job; HEAD's range stopped
    // at raster / (raster + source).
    let doc = placed16(512, 384);
    let edit = GeometryEdit::parse(
        &json!({"op":"affine_set","layers":[0],"matrix":turn_about(17., 256., 192.),
            "filter":"bicubic"})
        .to_string(),
    )
    .unwrap();
    let control = DrJobControl::default();
    control.record_reports();
    doc.geometry(&edit, REFERENCE_WORKING_BYTES, 0, Some(&control))
        .unwrap()
        .unwrap();
    let last = control
        .reports()
        .iter()
        .filter(|r| r.stage == 6)
        .map(|r| r.progress)
        .fold(0., f64::max);
    assert!(last >= 0.999, "{last}");
}

/// The five exact requests: quarter and half turns, flips, an integer move.
fn exact_edits() -> Vec<GeometryEdit> {
    [
        json!({"op":"rotate90"}),
        json!({"op":"rotate180"}),
        json!({"op":"flip_horizontal"}),
        json!({"op":"flip_vertical"}),
        json!({"op":"affine_set","layers":[0],"matrix":[1,0,0,1,7,-5],"filter":"bicubic"}),
    ]
    .iter()
    .map(|request| GeometryEdit::parse(&request.to_string()).unwrap())
    .collect()
}

#[test]
fn an_exact_placed_edit_counts_only_its_raster_work() {
    // An exact edit runs only the raster plane, so a placed layer's
    // plan counts the work of an unplaced photo's; HEAD added the never-run
    // source render's units too.
    let placed = placed16(640, 480);
    let photo = photo16(640, 480);
    for edit in exact_edits() {
        let placed = placed.geometry_plan(&edit).unwrap().work_units;
        let photo = photo.geometry_plan(&edit).unwrap().work_units;
        assert_eq!(placed, photo, "{edit:?}");
    }
}

#[test]
fn an_exact_placed_turn_reports_progress_to_the_end() {
    // An exact layer's progress range is its raster plane's units, and
    // the plan now counts only those, so the plane reaches the end of the
    // job; HEAD's stopped at raster / (raster + source).
    let doc = placed16(512, 384);
    let edit = GeometryEdit::parse(&json!({"op":"rotate90"}).to_string()).unwrap();
    let control = DrJobControl::default();
    control.record_reports();
    doc.geometry(&edit, REFERENCE_WORKING_BYTES, 0, Some(&control))
        .unwrap()
        .unwrap();
    let last = control
        .reports()
        .iter()
        .filter(|r| r.stage == 6)
        .map(|r| r.progress)
        .fold(0., f64::max);
    assert!(last >= 0.999, "{last}");
}

/// `common::doc_of` placed as a retained source (`retained_source_work.rs`'s
/// `place`), its original layer removed: a single 8-bit placed image.
fn placed8(w: u32, h: u32) -> DrDocument {
    let source = common::doc_of(w, h, common::plasma);
    source
        .source_edit(
            Some(&source),
            &SourceEdit::parse(
                r#"{"op":"place","source_id":"work-fixture","name":"Original","x":0,"y":0}"#,
            )
            .unwrap(),
            REFERENCE_WORKING_BYTES,
            0,
            None,
        )
        .unwrap()
        .unwrap()
        .remove_layers(&[0])
        .unwrap()
}

#[test]
fn a_21_mp_8_bit_placed_image_commits_with_catmull_rom() {
    // The M3 record (`precision_geometry_budget`): an 8-bit placed
    // transform commit takes the one-shot `MAX_DESTRUCTIVE_WORK`. Floors are
    // 1.25 × the measured max E: 366,493,984 for the geometry edits,
    // 366,477,600 for the direct source `transform`.
    let doc = placed8(5700, 3800);
    let matrix = turn_about(17., 2850., 1900.);
    let quad = [0., 0., 5700., 0., 6555., 4370., -855., 4370.];
    for (name, request) in [
        (
            "S17",
            json!({"op":"affine_set","layers":[0],"matrix":matrix,"filter":"bicubic"}),
        ),
        (
            "R17",
            json!({"op":"rotate_canvas","degrees":17,"filter":"bicubic"}),
        ),
        (
            "PS",
            json!({"op":"perspective_set","layers":[0],"source_rect":[0,0,5700,3800],
                "quad":quad,"filter":"bicubic"}),
        ),
    ] {
        let plan = doc
            .geometry_edit_plan(&GeometryEdit::parse(&request.to_string()).unwrap())
            .unwrap();
        assert!(
            plan.scratch_bytes >= 458_117_480,
            "{name} {}",
            plan.scratch_bytes
        );
        let total = plan.retained_bytes + plan.scratch_bytes;
        println!("{name}: retained + scratch {total}");
        assert!(total <= MINIMUM_MAC_LIMIT, "{name}");
    }
    let source = doc
        .source_plan(
            None,
            &SourceEdit::parse(
                &json!({"op":"transform","layer":0,"matrix":matrix,"filter":"bicubic"}).to_string(),
            )
            .unwrap(),
        )
        .unwrap();
    assert!(source.work_units > MAX_WORK_UNITS, "{}", source.work_units);
    assert!(
        source.scratch_bytes >= 458_097_000,
        "{}",
        source.scratch_bytes
    );
    // The ⌘T preview sends `interactive` and keeps the interactive cap.
    let preview = doc.geometry_edit_plan(
        &GeometryEdit::parse(
            &json!({"op":"affine_set","layers":[0],"matrix":matrix,"filter":"bicubic",
                "interactive":true})
            .to_string(),
        )
        .unwrap(),
    );
    assert!(
        preview
            .as_ref()
            .err()
            .is_some_and(|e| e.contains("bounded sampling work limit")),
        "{:?}",
        preview.map(|p| p.scratch_bytes)
    );
}

#[test]
fn an_exact_8_bit_rotate_canvas_charges_no_source_render() {
    // An exact Rotate Canvas runs `rotate90`/`rotate180`/`rotate270`
    // verbatim, so its work is the plain turn's, and a placed layer's source
    // adds no scratch over an unplaced photo's beyond the layer's strings.
    // HEAD charged the never-run render's work, scratch and plane.
    let doc = placed8(640, 480);
    let photo = common::doc_of(640, 480, common::plasma);
    let edit = |request: serde_json::Value| GeometryEdit::parse(&request.to_string()).unwrap();
    for (degrees, op) in [(90, "rotate90"), (180, "rotate180"), (270, "rotate270")] {
        let canvas = edit(json!({"op":"rotate_canvas","degrees":degrees,"filter":"bicubic"}));
        let turn = doc.geometry_edit_plan(&canvas).unwrap();
        let plain = doc.geometry_edit_plan(&edit(json!({"op": op}))).unwrap();
        assert_eq!(turn.work_units, plain.work_units, "{degrees}");
        let unplaced = photo.geometry_edit_plan(&canvas).unwrap().scratch_bytes;
        assert!(
            turn.scratch_bytes.abs_diff(unplaced) < 4096,
            "{degrees}: {} vs {unplaced}",
            turn.scratch_bytes
        );
    }
}
