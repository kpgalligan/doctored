//! Reproducible analytic quality measurements and licensed-photo inspection
//! artifacts. Run with an output root; optional second argument is the photo
//! folder prepared by phase6_quality_fixtures.py + phase6_initial_masks.py.
use doctored_core::{ffi::*, ffi_doc::dr_selection_feather, ffi_selection_tools::*};
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::json;
use std::{
    ffi::{CStr, CString},
    path::Path,
    ptr,
    time::Instant,
};
#[path = "../tests/common/selection_quality.rs"]
mod corpus;
#[path = "../tests/common/matting_quality.rs"]
mod qualification;
use qualification::alpha_metrics as metrics;

/// Select and Mask's refine with no area plane: every pixel automatic, as with
/// no Refine Edge strokes.
fn refine(source: &RgbaImage, mask: &GrayImage, params: &str) -> (GrayImage, f64) {
    unsafe {
        let source = dr_image_from_rgba(source.as_raw().as_ptr(), source.width(), source.height());
        let mut out = vec![0; mask.as_raw().len()];
        let mut err = ptr::null_mut();
        let start = Instant::now();
        let ok = dr_selection_refine(
            source,
            mask.as_raw().as_ptr(),
            ptr::null(),
            CString::new(params).unwrap().as_ptr(),
            out.as_mut_ptr(),
            ptr::null(),
            &mut err,
        );
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        dr_image_free(source);
        if !ok {
            panic!(
                "refinement: {}",
                if err.is_null() {
                    "unknown".into()
                } else {
                    let s = CStr::from_ptr(err).to_string_lossy().into_owned();
                    dr_string_free(err);
                    s
                }
            );
        }
        (
            GrayImage::from_raw(mask.width(), mask.height(), out).unwrap(),
            elapsed,
        )
    }
}

fn panel(source: &RgbaImage, mask: &GrayImage, background: u8) -> RgbaImage {
    RgbaImage::from_fn(source.width(), source.height(), |x, y| {
        let p = source.get_pixel(x, y);
        let a = f32::from(mask.get_pixel(x, y)[0]) / 255.;
        let b = if background == 127 {
            if (x / 12 + y / 12) % 2 == 0 {
                190.
            } else {
                235.
            }
        } else {
            f32::from(background)
        };
        Rgba([
            (f32::from(p[0]) * a + b * (1. - a)).round() as u8,
            (f32::from(p[1]) * a + b * (1. - a)).round() as u8,
            (f32::from(p[2]) * a + b * (1. - a)).round() as u8,
            255,
        ])
    })
}

fn artifacts(
    root: &Path,
    name: &str,
    source: &RgbaImage,
    initial: &GrayImage,
    after: &GrayImage,
    crop: (u32, u32, u32, u32),
) {
    source
        .save(root.join(format!("{name}-source.png")))
        .unwrap();
    initial
        .save(root.join(format!("{name}-initial.png")))
        .unwrap();
    after
        .save(root.join(format!("{name}-candidate.png")))
        .unwrap();
    for (bg, label) in [(255, "white"), (24, "dark"), (127, "checker")] {
        let before = panel(source, initial, bg);
        let after = panel(source, after, bg);
        let (x, y, w, h) = crop;
        let before = image::imageops::crop_imm(&before, x, y, w, h).to_image();
        let after = image::imageops::crop_imm(&after, x, y, w, h).to_image();
        let mut pair = RgbaImage::new(w * 2, h);
        image::imageops::replace(&mut pair, &before, 0, 0);
        image::imageops::replace(&mut pair, &after, i64::from(w), 0);
        pair.save(root.join(format!("{name}-{label}-100pct.png")))
            .unwrap();
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(args.get(1).expect("output root"));
    std::fs::create_dir_all(root).unwrap();
    let mut records = Vec::new();
    let mut passes = 0;
    for subject in corpus::SUBJECTS {
        let truth = corpus::alpha(subject);
        truth
            .save(root.join(format!("{subject}-truth.png")))
            .unwrap();
        for background in ["light", "dark", "ambiguous"] {
            let source = corpus::composite(&truth, background);
            for start in ["blurred", "binary"] {
                let mut initial = truth.clone();
                if start == "blurred" {
                    unsafe {
                        assert!(dr_selection_feather(
                            initial.as_mut_ptr(),
                            truth.width(),
                            truth.height(),
                            3.
                        ));
                    }
                } else {
                    for v in initial.as_mut() {
                        *v = if *v >= 128 { 255 } else { 0 };
                    }
                }
                let (after, ms) = refine(&source, &initial, r#"{"edge_radius":4}"#);
                let before_metrics = metrics(&truth, &initial);
                let after_metrics = metrics(&truth, &after);
                // Predeclared gate: no >1/255 boundary MAD regression and retain
                // at least95% of soft detail. Also requires real-photo inspection.
                let pass = after_metrics["boundary_mad"].as_f64().unwrap()
                    <= before_metrics["boundary_mad"].as_f64().unwrap() + 1. / 255.
                    && after_metrics["soft_detail_recall"].as_f64().unwrap() >= 0.95;
                if pass {
                    passes += 1;
                }
                let name = format!("{subject}-{background}-{start}");
                artifacts(
                    root,
                    &name,
                    &source,
                    &initial,
                    &after,
                    (0, 0, corpus::WIDTH, corpus::HEIGHT),
                );
                records.push(json!({"case":name,"before":before_metrics,"after":after_metrics,"candidate_ms":ms,"passes_analytic_gate":pass}));
            }
        }
    }
    let mut photographs = Vec::new();
    if let Some(directory) = args.get(2) {
        let directory = Path::new(directory);
        for (name, crop) in [
            ("portrait", (140, 8, 180, 160)),
            ("fur", (270, 50, 170, 230)),
            ("glass", (330, 190, 500, 400)),
            ("product", (340, 345, 500, 350)),
        ] {
            let source = image::open(directory.join(format!("{name}-source.png")))
                .unwrap()
                .to_rgba8();
            let initial_rgba = image::open(directory.join(format!("{name}-initial.png")))
                .unwrap()
                .to_rgba8();
            assert!(
                initial_rgba
                    .pixels()
                    .all(|p| p[0] == p[1] && p[1] == p[2] && p[3] == 255),
                "initial mask must be opaque grayscale"
            );
            let initial = image::DynamicImage::ImageRgba8(initial_rgba).to_luma8();
            assert!(
                initial.as_raw().contains(&0) && initial.as_raw().contains(&255),
                "initial mask must contain background and subject"
            );
            assert_eq!(source.dimensions(), initial.dimensions());
            let (after, ms) = refine(&source, &initial, r#"{"edge_radius":4}"#);
            let (manual, manual_ms) = refine(&source, &initial, r#"{"feather":1,"contrast":0.05}"#);
            artifacts(
                root,
                &format!("photo-{name}"),
                &source,
                &initial,
                &after,
                crop,
            );
            artifacts(
                root,
                &format!("photo-{name}-manual"),
                &source,
                &initial,
                &manual,
                crop,
            );
            photographs.push(json!({"case":name,"dimensions":[source.width(),source.height()],"initial":"macOS Vision foreground instance mask","alpha_ground_truth":false,"candidate_ms":ms,"manual_ms":manual_ms,"inspection_crop":crop,"candidate_mean_absolute_change":metrics(&initial,&after)["mad"],"manual_controls":{"feather":1,"contrast":0.05}}));
        }
    }
    let report = json!({"analytic_passes":passes,"analytic_cases":records.len(),"gate":"no boundary MAD regression above1/255,soft-detail recall>=0.95 on every analytic case plus photographic inspection","edge_stage":"edge_radius 4, automatic area (no Refine Edge strokes)","decontamination":false,"photographic_ground_truth_available":false,"analytic":records,"photographs":photographs});
    std::fs::write(
        root.join("metrics.json"),
        serde_json::to_string_pretty(&report).unwrap() + "\n",
    )
    .unwrap();
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
