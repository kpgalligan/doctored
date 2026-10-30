//! Experimental alpha and foreground-color qualification. Original24-case alpha
//! corpus/gate stay unchanged. Photo references must first pass the pinned
//! phase6_reference_fixtures.py --verify-only check. All RGB math is encoded.
use doctored_core::{
    ffi::*,
    ffi_doc::*,
    ffi_foreground::dr_image_estimate_foreground,
    sampled_matting::{estimate, MattingParams},
};
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::{json, Value};
use std::{
    ffi::{CStr, CString},
    path::Path,
    ptr,
    time::Instant,
};
#[path = "../tests/common/selection_quality.rs"]
mod corpus;
#[path = "../tests/common/matting_quality.rs"]
pub(crate) mod quality;

pub(crate) fn prior(truth: &GrayImage, kind: &str) -> GrayImage {
    let mut result = truth.clone();
    if kind == "blurred" {
        unsafe {
            assert!(dr_selection_feather(
                result.as_mut_ptr(),
                truth.width(),
                truth.height(),
                3.0
            ));
        }
    } else {
        for a in result.as_mut() {
            *a = if *a >= 128 { 255 } else { 0 };
        }
    }
    result
}

fn foreground(source: &RgbaImage, alpha: &GrayImage) -> Result<(RgbaImage, f64), String> {
    unsafe {
        let image = dr_image_from_rgba(source.as_raw().as_ptr(), source.width(), source.height());
        let document = dr_doc_from_image(image);
        let params = CString::new(r#"{"color_space":"encoded"}"#).unwrap();
        let mut error = ptr::null_mut();
        let start = Instant::now();
        let output = dr_image_estimate_foreground(
            document,
            image,
            alpha.as_raw().as_ptr(),
            params.as_ptr(),
            ptr::null(),
            &mut error,
        );
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let result = if !error.is_null() {
            let message = CStr::from_ptr(error).to_string_lossy().into_owned();
            dr_string_free(error);
            Err(message)
        } else if output.is_null() {
            Ok((source.clone(), ms))
        } else {
            Ok((
                RgbaImage::from_raw(
                    source.width(),
                    source.height(),
                    std::slice::from_raw_parts(dr_image_pixels_rgba(output), source.as_raw().len())
                        .to_vec(),
                )
                .unwrap(),
                ms,
            ))
        };
        if !output.is_null() {
            dr_image_free(output);
        }
        dr_doc_free(document);
        dr_image_free(image);
        result
    }
}

fn composite_colors(alpha: &GrayImage, colors: &[[f32; 3]], background: &str) -> RgbaImage {
    RgbaImage::from_fn(alpha.width(), alpha.height(), |x, y| {
        let i = (y * alpha.width() + x) as usize;
        let a = f64::from(alpha.as_raw()[i]) / 255.0;
        let bg = quality::replacement_background(background, x, y);
        let mut pixel = [0, 0, 0, 255];
        for c in 0..3 {
            pixel[c] = ((a * f64::from(colors[i][c]) + (1.0 - a) * bg[c]) * 255.0).round() as u8;
        }
        Rgba(pixel)
    })
}

fn panels(
    root: &Path,
    name: &str,
    source: &RgbaImage,
    alphas: (&GrayImage, &GrayImage, &GrayImage),
    truth_f: &[[f32; 3]],
    estimated: &RgbaImage,
    alpha_label: &str,
) {
    let (truth, initial, after) = alphas;
    // Fixed top-left512x512native crop chosen before scoring, plus full-size
    // individual masks/source/foreground so any edge can be inspected at1:1.
    source
        .save(root.join(format!("{name}-source.png")))
        .unwrap();
    truth
        .save(root.join(format!("{name}-truth-alpha.png")))
        .unwrap();
    initial
        .save(root.join(format!("{name}-prior.png")))
        .unwrap();
    after
        .save(root.join(format!("{name}-{alpha_label}-alpha.png")))
        .unwrap();
    estimated
        .save(root.join(format!("{name}-estimated-foreground.png")))
        .unwrap();
    let observed = quality::colors(source);
    let fg = quality::colors(estimated);
    for background in ["white", "dark", "checker"] {
        let images = [
            composite_colors(truth, truth_f, background),
            composite_colors(initial, &observed, background),
            composite_colors(after, &observed, background),
            composite_colors(after, &fg, background),
        ];
        for (label, image) in ["reference", "prior", "source-mask", "estimated"]
            .iter()
            .zip(&images)
        {
            image
                .save(root.join(format!("{name}-{background}-{label}.png")))
                .unwrap();
        }
        let w = source.width().min(512);
        let h = source.height().min(512);
        let mut panel = RgbaImage::new(w * 4, h);
        for (i, image) in images.iter().enumerate() {
            let cropped = image::imageops::crop_imm(image, 0, 0, w, h).to_image();
            image::imageops::replace(&mut panel, &cropped, i as i64 * i64::from(w), 0);
        }
        panel
            .save(root.join(format!("{name}-{background}-native-panel.png")))
            .unwrap();
    }
}

fn color_record(
    source: &RgbaImage,
    truth: &GrayImage,
    alpha: &GrayImage,
    truth_f: &[[f32; 3]],
    protection: Option<(&GrayImage, &[u8])>,
    joint_foreground: Option<&[[f32; 3]]>,
) -> (Value, Option<RgbaImage>) {
    let before =
        quality::color_metrics(truth, alpha, truth_f, &quality::colors(source), source).unwrap();
    let initial_estimate = if let Some(colors) = joint_foreground {
        let result = RgbaImage::from_fn(source.width(), source.height(), |x, y| {
            let rgb = colors[y as usize * source.width() as usize + x as usize];
            Rgba([
                (rgb[0] * 255.0).round() as u8,
                (rgb[1] * 255.0).round() as u8,
                (rgb[2] * 255.0).round() as u8,
                source.get_pixel(x, y)[3],
            ])
        });
        Ok((result, 0.0)) // Joint inference time is counted in candidate_ms.
    } else {
        foreground(source, alpha)
    };
    let estimated = initial_estimate.and_then(|(result, ms)| {
        if let Some((prior, user)) = protection {
            let timer = Instant::now();
            let protected = doctored_core::foreground_protection::protect(source, &result, alpha, prior, user, None)?;
            let diagnostics = json!({"policy":"source_protection_v1","eligible_pixels":protected.eligible_pixels,
                "protected_by_source_color":protected.protected_by_source_color,"corrected_pixels":protected.corrected_pixels});
            Ok((protected.image.unwrap_or_else(||source.clone()), ms + timer.elapsed().as_secs_f64()*1000.0, Some(diagnostics)))
        } else { Ok((result, ms, None)) }
    });
    match estimated {
        Ok((result, ms, diagnostics)) => {
            let after =
                quality::color_metrics(truth, alpha, truth_f, &quality::colors(&result), source)
                    .unwrap();
            let eligible = after["fractional_pixels"].as_u64().unwrap() > 0;
            let passes = eligible && quality::color_gate(&before, &after);
            let mut record = json!({"before":before,"after":after,"passes_color_gate":passes,"has_fractional_reference":eligible,"candidate_ms":ms});
            if let Some(diagnostics) = diagnostics {
                record["protection"] = diagnostics;
            }
            (record, Some(result))
        }
        Err(error) => (
            json!({"before":before,"refusal":error,"passes_color_gate":false}),
            None,
        ),
    }
}

/// Solver-independent qualification seam. Candidates receive only source RGB,
/// categorical trimap and degraded prior, never reference foreground or alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputPolicy {
    LegacyAutomaticHardLabels,
    AutomaticSoftPriorV1,
    AssistedHumanLabelsV1,
}

#[derive(Clone, Copy)]
pub(crate) struct Candidate {
    pub protect_foreground: bool,
    pub input_policy: InputPolicy,
    pub name: &'static str,
    pub estimate: EstimateAlpha,
}

pub(crate) type EstimateAlpha =
    fn(&str, &[[f32; 3]], u32, u32, &[u8], &[u8]) -> Result<AlphaResult, String>;

pub(crate) struct AlphaResult {
    pub alpha: GrayImage,
    /// Joint candidates may supply source-only F; legacy candidates retain the
    /// unchanged multilevel estimator. Reference F never enters inference.
    pub foreground: Option<Vec<[f32; 3]>>,
    pub diagnostics: Value,
    pub confidence: Option<GrayImage>,
}

fn sampled_candidate(
    _case: &str,
    colors: &[[f32; 3]],
    width: u32,
    height: u32,
    trimap: &[u8],
    prior: &[u8],
) -> Result<AlphaResult, String> {
    let result = estimate(
        colors,
        width,
        height,
        trimap,
        prior,
        &MattingParams::default(),
        None,
    )?;
    Ok(AlphaResult {
        foreground: None,
        alpha: result.alpha,
        diagnostics: json!({"unknown_pixels":result.unknown_pixels,"unresolved_pixels":result.unresolved_pixels,"foreground_samples":result.foreground_samples,"background_samples":result.background_samples}),
        confidence: Some(result.confidence),
    })
}

pub(crate) const SAMPLED: Candidate = Candidate {
    protect_foreground: false,
    input_policy: InputPolicy::LegacyAutomaticHardLabels,
    name: "sampled",
    estimate: sampled_candidate,
};

/// The seven-argument evaluator every automatic candidate uses: no human
/// label plane exists, so the policy arms that need one cannot be selected.
pub(crate) fn evaluate_candidate(
    location: (&Path, &str),
    source: &RgbaImage,
    truth: &GrayImage,
    truth_f: &[[f32; 3]],
    start: &str,
    artifacts: bool,
    candidate: Candidate,
) -> Value {
    evaluate_candidate_with_labels(
        location, source, truth, truth_f, start, artifacts, candidate, None,
    )
}

// 8 params; make lint is --all-targets -D warnings, no clippy.toml
// (precedent: ffi_doc.rs:779). The assisted policy needs the human label
// plane beside the candidate, and the call site reads as one evaluator call.
#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate_candidate_with_labels(
    location: (&Path, &str),
    source: &RgbaImage,
    truth: &GrayImage,
    truth_f: &[[f32; 3]],
    start: &str,
    artifacts: bool,
    candidate: Candidate,
    labels: Option<&GrayImage>,
) -> Value {
    let (root, name) = location;
    let initial = prior(truth, start);
    let trimap = quality::trimap_from_prior(&initial);
    let before = quality::alpha_metrics(truth, &initial);
    let automatic_constraint_audit = quality::constraint_audit(truth, &trimap, &before);
    let constraints = match candidate.input_policy {
        InputPolicy::LegacyAutomaticHardLabels => trimap.clone(),
        InputPolicy::AutomaticSoftPriorV1 => {
            GrayImage::from_pixel(source.width(), source.height(), image::Luma([128]))
        }
        InputPolicy::AssistedHumanLabelsV1 => {
            let labels = labels.expect("assisted policy needs a label plane");
            assert_eq!(
                labels.dimensions(),
                (source.width(), source.height()),
                "the human label plane is the case extent"
            );
            assert!(
                labels
                    .as_raw()
                    .iter()
                    .all(|value| matches!(value, 0 | 128 | 255)),
                "the human label plane is categorical 0/128/255"
            );
            labels.clone()
        }
    };
    let constraint_audit = quality::constraint_audit(truth, &constraints, &before);
    let colors = quality::colors(source);
    let timer = Instant::now();
    let sampled = (candidate.estimate)(
        name,
        &colors,
        source.width(),
        source.height(),
        constraints.as_raw(),
        initial.as_raw(),
    )
    .and_then(|result| {
        if result.alpha.dimensions() != (source.width(), source.height())
            || result
                .confidence
                .as_ref()
                .is_some_and(|plane| plane.dimensions() != result.alpha.dimensions())
        {
            return Err(
                "alpha candidate returned mismatched alpha or confidence dimensions".into(),
            );
        }
        if result.foreground.as_ref().is_some_and(|values| {
            values.len() != colors.len()
                || values
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        }) {
            return Err(
                "joint candidate returned invalid foreground dimensions or RGB values".into(),
            );
        }
        if result
            .alpha
            .as_raw()
            .iter()
            .zip(constraints.as_raw())
            .any(|(&value, &label)| label != 128 && value != label)
        {
            return Err("alpha candidate changed known categorical trimap constraints".into());
        }
        Ok(result)
    });
    let ms = timer.elapsed().as_secs_f64() * 1000.0;
    let (after, sampling, joint_foreground) = match sampled {
        Ok(result) => {
            if artifacts {
                if let Some(confidence) = result.confidence {
                    confidence
                        .save(root.join(format!("{name}-confidence.png")))
                        .unwrap();
                }
            }
            (result.alpha, result.diagnostics, result.foreground)
        }
        Err(error) => (
            initial.clone(),
            json!({"refusal":error,"reported_alpha":"unchanged prior for diagnostic metrics only"}),
            None,
        ),
    };
    let measured = quality::alpha_metrics(truth, &after);
    let eligible = truth.as_raw().iter().any(|&a| a > 0 && a < 255);
    let pass = sampling.get("refusal").is_none() && quality::alpha_gate(&before, &measured);
    let (oracle, _) = color_record(source, truth, truth, truth_f, None, None);
    // Preserve the original unprotected estimator diagnostic. This oracle row
    // does not measure the protected v3/v4 RGB pipeline with reference alpha.
    let (mut coupled, estimated) = color_record(
        source,
        truth,
        &after,
        truth_f,
        candidate
            .protect_foreground
            .then_some((&initial, constraints.as_raw().as_slice())),
        joint_foreground.as_deref(),
    );
    // A refused alpha operation cannot qualify an end-to-end color pipeline.
    coupled["alpha_candidate_refused"] = json!(sampling.get("refusal").is_some());
    if sampling.get("refusal").is_some() {
        coupled["passes_color_gate"] = json!(false);
    }
    if artifacts {
        trimap
            .save(root.join(format!("{name}-trimap.png")))
            .unwrap();
        panels(
            root,
            name,
            source,
            (truth, &initial, &after),
            truth_f,
            estimated.as_ref().unwrap_or(source),
            if candidate.name == "sampled" {
                "sampled"
            } else {
                "estimated"
            },
        );
    }
    let end_to_end = pass && eligible && coupled["passes_color_gate"] == true;
    let mut record = json!({"passes_end_to_end_gate": end_to_end,"case":name,"domain":"encoded RGB","dimensions":[truth.width(),truth.height()],"reference_classes":{"background_pixels":truth.as_raw().iter().filter(|&&a|a==0).count(),"foreground_pixels":truth.as_raw().iter().filter(|&&a|a==255).count(),"fractional_pixels":truth.as_raw().iter().filter(|&&a|a>0&&a<255).count()},"has_fractional_reference":eligible,"before":before,"after":measured,"sampling":sampling,"candidate_ms":ms,"passes_analytic_gate":pass,"oracle_alpha_color":oracle,"sampled_alpha_color":coupled});
    println!(
        "{name}: alpha={} oracle_color={} coupled_color={} refusal={}",
        pass,
        record["oracle_alpha_color"]["passes_color_gate"],
        record["sampled_alpha_color"]["passes_color_gate"],
        record["sampling"]["refusal"]
    );
    record["constraint_audit"] = constraint_audit;
    if joint_foreground.is_some() {
        record["joint_foreground_pipeline"] = json!(
            "source-only candidate alpha and foreground; no post-hoc multilevel reconstruction"
        );
        record["oracle_alpha_color_pipeline"] = json!(
            "legacy unprotected multilevel estimator; joint reference-alpha pipeline not measured"
        );
    }
    if candidate.protect_foreground {
        record["oracle_alpha_color_pipeline"] = json!("legacy unprotected multilevel estimator; protected reference-alpha pipeline not measured");
    }
    if candidate.input_policy == InputPolicy::AutomaticSoftPriorV1 {
        record["input_policy"] = json!({"version":1,"automatic_prior":"soft","user_constraints":"none","automatic_labels_used_as_hard_constraints":false});
        record["legacy_automatic_constraint_audit"] = automatic_constraint_audit;
    } else if candidate.input_policy == InputPolicy::AssistedHumanLabelsV1 {
        let mut counts = [0_usize; 3];
        for &label in constraints.as_raw() {
            counts[usize::from(label == 128) + 2 * usize::from(label == 255)] += 1;
        }
        record["input_policy"] = json!({"version":1,"automatic_prior":"soft","user_constraints":"assisted_human_labels_v1","automatic_labels_used_as_hard_constraints":false,"human_labels_used_as_hard_constraints":true,"labeled_foreground_pixels":counts[2],"labeled_background_pixels":counts[0],"unknown_pixels":counts[1]});
        record["legacy_automatic_constraint_audit"] = automatic_constraint_audit;
    }
    if candidate.name != "sampled" {
        let fields = record.as_object_mut().unwrap();
        let metadata = fields.remove("sampling").unwrap();
        let color = fields.remove("sampled_alpha_color").unwrap();
        fields.insert("alpha_candidate".into(), metadata);
        fields.insert("estimated_alpha_color".into(), color);
        fields.insert("solver".into(), json!(candidate.name));
    }
    record
}

pub(crate) fn photo_source(foreground: &RgbaImage, background: &str) -> RgbaImage {
    RgbaImage::from_fn(foreground.width(), foreground.height(), |x, y| {
        let p = foreground.get_pixel(x, y);
        let a = f64::from(p[3]) / 255.0;
        let bg = match background {
            "light" => [0.88, 0.91, 0.96],
            "dark" => [0.02, 0.03, 0.06],
            "checker" => quality::replacement_background("checker", x, y),
            _ => unreachable!(),
        };
        let mut result = [0, 0, 0, 255];
        for c in 0..3 {
            result[c] = (a * f64::from(p[c]) + (1.0 - a) * bg[c] * 255.0).round() as u8;
        }
        Rgba(result)
    })
}

/// Serialized copy of the frozen color thresholds for standalone reports.
pub(crate) fn color_gate_contract() -> Value {
    json!({"fractional_foreground_mad_max":0.05,"visible_fringe_initial_mad":0.02,"visible_fringe_min_improvement":0.2,"recomposite_boundary_mad_max":0.02,"opaque_changed_pixels_max":0,"weighted_mad_regression_allowance":1.0/255.0})
}

pub(crate) fn summary(records: &[Value]) -> Value {
    json!({"cases":records.len(),"end_to_end_passes":records.iter().filter(|r|r["passes_end_to_end_gate"]==true).count(),"alpha_passes":records.iter().filter(|r|r["passes_analytic_gate"]==true).count(),"alpha_refusals":records.iter().filter(|r|r["sampling"].get("refusal").is_some()).count(),"oracle_alpha_color_passes":records.iter().filter(|r|r["oracle_alpha_color"]["passes_color_gate"]==true).count(),"sampled_alpha_color_passes":records.iter().filter(|r|r["sampled_alpha_color"]["passes_color_gate"]==true).count()})
}

pub(crate) fn summary_for(records: &[Value], candidate: Candidate) -> Value {
    if candidate.name == "sampled" {
        return summary(records);
    }
    json!({"cases":records.len(),"end_to_end_passes":records.iter().filter(|r|r["passes_end_to_end_gate"]==true).count(),"alpha_passes":records.iter().filter(|r|r["passes_analytic_gate"]==true).count(),"alpha_refusals":records.iter().filter(|r|r["alpha_candidate"].get("refusal").is_some()).count(),"oracle_alpha_color_passes":records.iter().filter(|r|r["oracle_alpha_color"]["passes_color_gate"]==true).count(),"estimated_alpha_color_passes":records.iter().filter(|r|r["estimated_alpha_color"]["passes_color_gate"]==true).count()})
}

pub(crate) fn run(root: &Path, photo_directory: Option<&Path>, candidate: Candidate) {
    std::fs::create_dir_all(root).unwrap();
    let mut analytic = Vec::new();
    let mut photos = Vec::new();
    for subject in corpus::SUBJECTS {
        let truth = corpus::alpha(subject);
        let truth_f = (0..truth.height())
            .flat_map(|_| (0..truth.width()).map(corpus::foreground_color))
            .collect::<Vec<_>>();
        for background in ["light", "dark", "ambiguous"] {
            let source = corpus::composite(&truth, background);
            for start in ["blurred", "binary"] {
                let name = format!("{subject}-{background}-{start}");
                analytic.push(evaluate_candidate(
                    (root, &name),
                    &source,
                    &truth,
                    &truth_f,
                    start,
                    true,
                    candidate,
                ));
            }
        }
    }
    let mut provenance = Value::Null;
    if let Some(directory) = photo_directory {
        provenance =
            serde_json::from_slice(&std::fs::read(directory.join("references.json")).unwrap())
                .unwrap();
        assert_eq!(provenance["schema_version"], 1);
        for reference in provenance["references"]
            .as_array()
            .expect("reference array")
        {
            let name = reference["file"].as_str().unwrap();
            assert_eq!(
                Path::new(name).file_name().and_then(|s| s.to_str()),
                Some(name)
            );
            let full = image::open(directory.join(name)).unwrap().to_rgba8();
            assert_eq!(full.dimensions(), (2304, 3456));
            assert_eq!(reference["crop"], json!([512, 384, 1280, 1280]));
            let crop = image::imageops::crop_imm(&full, 512, 384, 1280, 1280).to_image();
            let truth = GrayImage::from_fn(crop.width(), crop.height(), |x, y| {
                image::Luma([crop.get_pixel(x, y)[3]])
            });
            let truth_f = quality::colors(&crop);
            for background in ["light", "dark", "checker"] {
                let source = photo_source(&crop, background);
                for start in ["blurred", "binary"] {
                    let case = format!(
                        "photo-{}-{background}-{start}",
                        reference["id"].as_str().unwrap()
                    );
                    let mut record = evaluate_candidate(
                        (root, &case),
                        &source,
                        &truth,
                        &truth_f,
                        start,
                        background == "light" && start == "blurred",
                        candidate,
                    );
                    // A reference crop with no soft population is diagnostic,
                    // not positive evidence for photographic edge quality.
                    if record["has_fractional_reference"] != true {
                        record["passes_analytic_gate"] = json!(false);
                    }
                    record["reference"] = reference.clone();
                    photos.push(record);
                }
            }
        }
    }
    let mut report = json!({"schema_version":1,"domain":"encoded RGB; controlled encoded composites","automatic_trimap":"prior<=8 BG / >=247 FG, clipped7x7 erosion, remaining128; never truth","analytic_summary":summary_for(&analytic,candidate),"photo_summary":summary_for(&photos,candidate),"original24_gate_unchanged":true,"color_gate":color_gate_contract(),"photo_limitations":["fixed native1280x1280 crops, not fullframe qualification","supplied green-screen-extracted portrait/hair references, not physical truth","no photographic fur/glass/product alpha+foreground references","panels left-to-right: reference / prior+source RGB / sampled alpha+source RGB / sampled alpha+estimated RGB"],"reference_provenance":provenance,"analytic":analytic,"photographic_recompositions":photos});
    if candidate.name != "sampled" {
        report["solver"] = json!(candidate.name);
        report["photo_limitations"][3] = json!("panels left-to-right: reference / prior+source RGB / estimated alpha+source RGB / estimated alpha+estimated RGB");
    }
    std::fs::write(
        root.join("metrics.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "analytic {}\nphotographic {}",
        report["analytic_summary"], report["photo_summary"]
    );
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    run(
        Path::new(args.get(1).expect("output directory")),
        args.get(2).map(Path::new),
        SAMPLED,
    );
}
