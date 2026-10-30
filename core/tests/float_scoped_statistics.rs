//! The 32-bit float arm of the schema2 scoped analysis. A float
//! document's Composite, raw Layer and Backdrop sources, with or without
//! selection coverage, answer the stop-binned histogram and the weighted
//! linear sample through the same request, plan, admission and source walk
//! as 8/16 bits.
//!
//! Oracles are independent of the implementation: every expected answer is
//! computed HERE from the row-major samples a fixture was built from, with
//! the rules written out (the stop bin `floor((log2(max(v, 2^-12)) + 12) /
//! 18 · 256)` clamped to `[0, 255]`, Rec.709 luma over `max(v, 0)`, RGB
//! eligibility alpha > 0, alpha counting every positively weighted
//! position, coverage as a rational weight over 65535). Float documents are
//! authored as float TIFFs and opened with `open_preserving`, so a raw leaf
//! holds exactly the samples the oracle reads.

use std::ffi::{c_char, CStr, CString};
use std::ptr;

use doctored_core::ffi::dr_string_free;
use doctored_core::ffi_scoped_statistics::{
    dr_precision_doc_scoped_analysis, dr_precision_image_scoped_analysis,
};
use doctored_core::icc::IccProfile;
use doctored_core::job_control::DrJobControl;
use doctored_core::metadata::Metadata;
use doctored_core::precision_document::PrecisionDocument;
use doctored_core::precision_geometry::GeometryEdit;
use doctored_core::precision_image::{PrecisionImage, REFERENCE_WORKING_BYTES};
use doctored_core::scoped_statistics::{AnalysisRequest, STATISTICS_SCRATCH};
use doctored_core::tiff_float::FloatTiff;
use serde_json::{json, Value};

const LIMIT: u64 = REFERENCE_WORKING_BYTES;
const W: u32 = 19;
const H: u32 = 13;
const EMPTY_SAMPLE: &str = "no positively weighted pixels in the sample region";

// ------------------------------------------------------------- fixtures --

/// Extended-range straight samples: exact zeros, negatives, above-white RGB,
/// alpha 0 (hidden RGB) and alpha 1. No −0.0, so ±0 ordering never matters.
fn extended(width: u32, height: u32, seed: u32) -> Vec<[f32; 4]> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            let v = ((x * 37 + y * 101 + seed * 13) % 1000) as f32 / 1000.0;
            let r = if (x + y) % 9 == 0 { 0.0 } else { v * 6.0 - 1.0 };
            let g = if (x * y) % 7 == 3 { -0.25 } else { 1.0 - v };
            let b = ((x ^ y) % 97) as f32 / 24.0;
            let a = match (x + 2 * y) % 11 {
                0 => 0.0,
                1 | 2 => 1.0,
                k => k as f32 / 11.0,
            };
            [r, g, b, a]
        })
        .collect()
}

/// The same range with alpha 1 everywhere, so Normal over transparent is
/// exact and a group or backdrop render reproduces its child's samples.
fn opaque(width: u32, height: u32, seed: u32) -> Vec<[f32; 4]> {
    extended(width, height, seed)
        .into_iter()
        .map(|[r, g, b, _]| [r, g, b, 1.0])
        .collect()
}

/// Fractional coverage holding 0, 65535 and intermediate weights.
fn coverage(width: u32, height: u32) -> Vec<u16> {
    (0..width * height)
        .map(|i| match i % 5 {
            0 => 0,
            1 => 65535,
            k => (k * 9001 + i) as u16,
        })
        .collect()
}

fn image(width: u32, height: u32, samples: &[[f32; 4]]) -> PrecisionImage {
    PrecisionImage::from_rgba32_extended(width, height, samples.to_vec(), IccProfile::srgb(), None)
        .expect("an extended float image")
}

/// A float document holding one raster of `samples`, through a float TIFF
/// opened with `open_preserving`; its raster holds exactly the samples.
fn float_document(width: u32, height: u32, samples: &[[f32; 4]]) -> PrecisionDocument {
    let tiff = FloatTiff {
        width,
        height,
        channels: 4,
        alpha: Some(false),
        samples: samples.iter().flatten().copied().collect(),
        icc: None,
        resolution: None,
        metadata: Metadata::default(),
        non_finite: 0,
    };
    let bytes = doctored_core::tiff_float::encode_to_vec(&tiff, false).expect("float TIFF");
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("scoped.tiff");
    std::fs::write(&path, &bytes).unwrap();
    let doc = PrecisionDocument::open_preserving(path.to_str().unwrap(), LIMIT, 0, None)
        .expect("the float document opens");
    assert!(doc.is_float());
    let held: Vec<[u32; 4]> = doc
        .layer(0)
        .and_then(|layer| layer.source())
        .expect("one float raster")
        .to_rgba32()
        .unwrap()
        .into_iter()
        .map(|p| p.map(f32::to_bits))
        .collect();
    let wanted: Vec<[u32; 4]> = samples.iter().map(|p| p.map(f32::to_bits)).collect();
    assert_eq!(held, wanted, "the raster holds the TIFF samples");
    doc
}

fn parse(value: &Value) -> AnalysisRequest {
    AnalysisRequest::parse(&value.to_string()).expect("a scoped request")
}

fn histogram(stride: u32) -> Value {
    json!({"operation":"histogram","source":"composite","stride":stride})
}

fn sample(x: i32, y: i32, reach: u32) -> Value {
    json!({"operation":"sample","source":"composite","x":x,"y":y,"reach":reach})
}

fn pick(value: &Value, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .map(|k| ((*k).to_string(), value[*k].clone()))
            .collect(),
    )
}

// -------------------------------------------------------------- oracles --

fn stop_bin(v: f32) -> usize {
    let ev = f64::from(v).max(2.0_f64.powf(-12.0)).log2();
    ((ev + 12.0) / 18.0 * 256.0).floor().clamp(0.0, 255.0) as usize
}

fn stops(v: f64) -> f64 {
    v.max(2.0_f64.powi(-16)).log2()
}

fn weight_at(coverage: &[u16], i: usize) -> u64 {
    if coverage.is_empty() {
        65535
    } else {
        u64::from(coverage[i])
    }
}

const HISTOGRAM_KEYS: [&str; 24] = [
    "bins",
    "bin_weight_units",
    "alpha_bin_weight_units",
    "stride",
    "total",
    "sampled_pixels",
    "sampled_rgb_weight_units",
    "full_rgb_weight_units",
    "full_alpha_weight_units",
    "full_rgb_pixel_count",
    "weighted_linear_sums_rgba",
    "exact_zero_rgba",
    "below_zero_rgba",
    "above_white_rgba",
    "exact_zero_weight_units_rgba",
    "below_zero_weight_units_rgba",
    "above_white_weight_units_rgba",
    "exact_one_rgba",
    "exact_one_weight_units_rgba",
    "min_rgba",
    "max_rgba",
    "storage",
    "schema_version",
    "selection",
];

/// The keys every stride must agree on: everything but the plot.
const COMPLETE_KEYS: [&str; 16] = [
    "full_rgb_weight_units",
    "full_alpha_weight_units",
    "full_rgb_pixel_count",
    "weighted_linear_sums_rgba",
    "exact_zero_rgba",
    "below_zero_rgba",
    "above_white_rgba",
    "exact_zero_weight_units_rgba",
    "below_zero_weight_units_rgba",
    "above_white_weight_units_rgba",
    "exact_one_rgba",
    "exact_one_weight_units_rgba",
    "min_rgba",
    "max_rgba",
    "storage",
    "selection",
];

/// The histogram written out over canvas-sized samples (`canvas[i]` is the
/// straight sample at canvas index `i`, zero outside the source).
fn histogram_oracle(canvas: &[[f32; 4]], coverage: &[u16], stride: usize) -> Value {
    let mut bins = vec![vec![0u64; 256]; 4];
    let mut alpha_bins = vec![0u64; 256];
    let (mut zero, mut below, mut above) = ([0u64; 4], [0u64; 4], [0u64; 4]);
    let (mut zero_w, mut below_w, mut above_w) = ([0u64; 4], [0u64; 4], [0u64; 4]);
    let (mut one, mut one_w) = ([0u64; 4], [0u64; 4]);
    let mut sums = [0f64; 4];
    let mut minimum: [Option<f32>; 4] = [None; 4];
    let mut maximum: [Option<f32>; 4] = [None; 4];
    let (mut rgb_w, mut alpha_w, mut sampled_w, mut sampled, mut measured) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    for (i, p) in canvas.iter().enumerate() {
        let w = weight_at(coverage, i);
        if w == 0 {
            continue;
        }
        let in_plot = i % stride == 0;
        let mut count = |c: usize| {
            let v = p[c];
            sums[c] += f64::from(v) * w as f64;
            if minimum[c].is_none_or(|m| v < m) {
                minimum[c] = Some(v);
            }
            if maximum[c].is_none_or(|m| v > m) {
                maximum[c] = Some(v);
            }
            if v == 0.0 {
                zero[c] += 1;
                zero_w[c] += w;
            }
            if v == 1.0 {
                one[c] += 1;
                one_w[c] += w;
            }
            if v < 0.0 {
                below[c] += 1;
                below_w[c] += w;
            }
            if v > 1.0 {
                above[c] += 1;
                above_w[c] += w;
            }
        };
        alpha_w += w;
        count(3);
        if in_plot {
            alpha_bins[(f64::from(p[3].clamp(0.0, 1.0)) * 255.0).round() as usize] += w;
            sampled += 1;
        }
        if p[3] == 0.0 {
            continue;
        }
        measured += 1;
        rgb_w += w;
        for c in 0..3 {
            count(c);
        }
        if in_plot {
            sampled_w += w;
            for c in 0..3 {
                bins[c][stop_bin(p[c])] += w;
            }
            let luma = 0.2126_f32 * p[0].max(0.0)
                + 0.7152_f32 * p[1].max(0.0)
                + 0.0722_f32 * p[2].max(0.0);
            bins[3][stop_bin(luma)] += w;
        }
    }
    let shown: Vec<Vec<f64>> = bins
        .iter()
        .map(|row| row.iter().map(|v| *v as f64 / 65535.0).collect())
        .collect();
    json!({"bins":shown,"bin_weight_units":bins,"alpha_bin_weight_units":alpha_bins,
        "stride":stride,"total":sampled_w as f64 / 65535.0,"sampled_pixels":sampled,
        "sampled_rgb_weight_units":sampled_w,"full_rgb_weight_units":rgb_w,
        "full_alpha_weight_units":alpha_w,"full_rgb_pixel_count":measured,
        "weighted_linear_sums_rgba":sums,
        "exact_zero_rgba":zero,"below_zero_rgba":below,"above_white_rgba":above,
        "exact_zero_weight_units_rgba":zero_w,"below_zero_weight_units_rgba":below_w,
        "above_white_weight_units_rgba":above_w,
        "exact_one_rgba":one,"exact_one_weight_units_rgba":one_w,
        "min_rgba":minimum.map(|v| v.unwrap_or(0.0)),"max_rgba":maximum.map(|v| v.unwrap_or(0.0)),
        "storage":"rgba32_float","schema_version":2,"selection":!coverage.is_empty()})
}

const SAMPLE_KEYS: [&str; 12] = [
    "sample_count",
    "center_rgba32",
    "weighted_sum_rgba32",
    "weight_units",
    "mean_rgba32",
    "above_white",
    "below_zero",
    "stops",
    "storage",
    "schema_version",
    "selection",
    "reach",
];

/// The weighted sample written out; `None` when nothing is positively
/// weighted (the shared refusal).
fn sample_oracle(
    canvas: &[[f32; 4]],
    (w, h): (u32, u32),
    coverage: &[u16],
    (x, y, reach): (i32, i32, u32),
) -> Option<Value> {
    let r = i64::from(reach);
    let inside = |px: i64, py: i64| px >= 0 && py >= 0 && px < i64::from(w) && py < i64::from(h);
    let (mut sums, mut units, mut count) = ([0f64; 4], 0u64, 0u64);
    let (mut above, mut below) = ([false; 3], [false; 3]);
    for py in i64::from(y) - r..=i64::from(y) + r {
        for px in i64::from(x) - r..=i64::from(x) + r {
            if !inside(px, py) {
                continue;
            }
            let i = py as usize * w as usize + px as usize;
            let weight = weight_at(coverage, i);
            if weight == 0 {
                continue;
            }
            let p = canvas[i];
            for c in 0..4 {
                sums[c] += f64::from(p[c]) * weight as f64;
            }
            for c in 0..3 {
                above[c] |= p[c] > 1.0;
                below[c] |= p[c] < 0.0;
            }
            units += weight;
            count += 1;
        }
    }
    if units == 0 {
        return None;
    }
    let center =
        inside(i64::from(x), i64::from(y)).then(|| canvas[y as usize * w as usize + x as usize]);
    let means = sums.map(|v| v / units as f64);
    Some(
        json!({"sample_count":count,"center_rgba32":center,"weighted_sum_rgba32":sums,
        "weight_units":units,"mean_rgba32":means,"above_white":above,"below_zero":below,
        "stops":[stops(means[0]),stops(means[1]),stops(means[2])],
        "storage":"rgba32_float","schema_version":2,"selection":!coverage.is_empty(),
        "reach":reach}),
    )
}

/// `samples` (W×H at `offset`) placed on a `canvas`-sized transparent plane.
fn placed(
    samples: &[[f32; 4]],
    size: (u32, u32),
    canvas: (u32, u32),
    offset: (i32, i32),
) -> Vec<[f32; 4]> {
    (0..canvas.0 * canvas.1)
        .map(|i| {
            let x = i64::from(i % canvas.0) - i64::from(offset.0);
            let y = i64::from(i / canvas.0) - i64::from(offset.1);
            if x < 0 || y < 0 || x >= i64::from(size.0) || y >= i64::from(size.1) {
                [0.0; 4]
            } else {
                samples[y as usize * size.0 as usize + x as usize]
            }
        })
        .collect()
}

const PROBES: [(i32, i32, u32); 7] = [
    (5, 4, 0),
    (5, 4, 1),
    (9, 6, 2),
    (0, 0, 2),
    (-1, -1, 1),
    (18, 12, 1),
    (3, 11, 2),
];

// ------------------------------------------------------------------ FFI --

fn answer(out: *mut c_char, error: *mut c_char) -> Result<Value, String> {
    unsafe {
        if out.is_null() {
            let text = CStr::from_ptr(error).to_string_lossy().into_owned();
            dr_string_free(error);
            return Err(text);
        }
        let text = CStr::from_ptr(out).to_string_lossy().into_owned();
        dr_string_free(out);
        Ok(serde_json::from_str(&text).unwrap())
    }
}

fn coverage_args(coverage: &[u16]) -> (*const u16, usize) {
    if coverage.is_empty() {
        (ptr::null(), 0)
    } else {
        (coverage.as_ptr(), coverage.len())
    }
}

fn image_ffi(image: &PrecisionImage, request: &Value, coverage: &[u16]) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let (pointer, length) = coverage_args(coverage);
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_image_scoped_analysis(
            image,
            text.as_ptr(),
            pointer,
            length,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    answer(out, error)
}

fn doc_ffi(doc: &PrecisionDocument, request: &Value, coverage: &[u16]) -> Result<Value, String> {
    let text = CString::new(request.to_string()).unwrap();
    let (pointer, length) = coverage_args(coverage);
    let mut error = ptr::null_mut();
    let out = unsafe {
        dr_precision_doc_scoped_analysis(
            doc,
            text.as_ptr(),
            pointer,
            length,
            LIMIT,
            0,
            ptr::null(),
            &mut error,
        )
    };
    answer(out, error)
}

/// The API answer through the same JSON text round trip the FFI takes.
fn round_trip(value: Value) -> Value {
    serde_json::from_str(&value.to_string()).unwrap()
}

// ---------------------------------------------------------------- tests --

#[test]
fn histogram_counts_weighted_stop_bins_and_complete_endpoints() {
    let samples = extended(W, H, 2);
    let image = image(W, H, &samples);
    let weights = coverage(W, H);
    for selection in [&[][..], &weights[..]] {
        let mut complete = Vec::new();
        for stride in [1u32, 3] {
            let value = image
                .scoped_analysis(&parse(&histogram(stride)), selection, LIMIT, 0, None)
                .unwrap();
            assert_eq!(
                pick(&value, &HISTOGRAM_KEYS),
                histogram_oracle(&samples, selection, stride as usize),
                "stride {stride}, selection {}",
                !selection.is_empty()
            );
            assert_eq!(
                value["channels"],
                json!(["red", "green", "blue", "linear_rec709_luma"])
            );
            assert_eq!(value["range"], json!({"min_ev":-12.0,"max_ev":6.0}));
            assert_eq!(value["coverage_denominator"], 65535);
            assert_eq!(value["source"], "composite");
            assert!(value.get("maximum_code").is_none());
            assert!(value["display_bin_rule"]
                .as_str()
                .unwrap()
                .contains("luma before binning"));
            complete.push(pick(&value, &COMPLETE_KEYS));
        }
        assert_eq!(
            complete[0], complete[1],
            "stride never changes complete keys"
        );
    }
    // The fixture really holds what the endpoints are for.
    let whole = image
        .scoped_analysis(&parse(&histogram(1)), &[], LIMIT, 0, None)
        .unwrap();
    for key in ["below_zero_rgba", "above_white_rgba", "exact_zero_rgba"] {
        assert!(whole[key][0].as_u64().unwrap() > 0, "{key}");
    }
}

#[test]
fn sample_is_weighted_and_counts_hidden_rgb() {
    let samples = extended(W, H, 3);
    let image = image(W, H, &samples);
    let weights = coverage(W, H);
    for selection in [&[][..], &weights[..]] {
        for (x, y, reach) in PROBES {
            let answer =
                image.scoped_analysis(&parse(&sample(x, y, reach)), selection, LIMIT, 0, None);
            match sample_oracle(&samples, (W, H), selection, (x, y, reach)) {
                Some(expected) => assert_eq!(
                    pick(&answer.unwrap(), &SAMPLE_KEYS),
                    expected,
                    "({x}, {y}) reach {reach}"
                ),
                None => assert_eq!(
                    answer.unwrap_err(),
                    EMPTY_SAMPLE,
                    "({x}, {y}) reach {reach}"
                ),
            }
        }
    }
    // Alpha-zero RGB is counted by the sample: (0, 0) has alpha 0 here.
    assert_eq!(samples[0][3], 0.0);
    let hidden = image
        .scoped_analysis(&parse(&sample(0, 0, 0)), &[], LIMIT, 0, None)
        .unwrap();
    assert_eq!(hidden["weight_units"], 65535);
    assert_eq!(hidden["mean_rgba32"][2], json!(f64::from(samples[0][2])));
    // Zero coverage over the whole region refuses with the shared sentence.
    let none = vec![0u16; (W * H) as usize];
    assert_eq!(
        image
            .scoped_analysis(&parse(&sample(4, 4, 2)), &none, LIMIT, 0, None)
            .unwrap_err(),
        EMPTY_SAMPLE
    );
}

#[test]
fn layer_scope_reads_the_raw_leaf_on_the_resized_canvas_and_groups_render_children() {
    let samples = extended(W, H, 4);
    let doc = float_document(W, H, &samples);
    let edit = GeometryEdit::parse(
        &json!({"op":"canvas_size","width":W + 4,"height":H + 3,"x":2,"y":1}).to_string(),
    )
    .unwrap();
    let resized = doc
        .geometry(&edit, LIMIT, 0, None)
        .unwrap()
        .expect("resized");
    let canvas = (W + 4, H + 3);
    let expected = placed(&samples, (W, H), canvas, (2, 1));
    let weights = coverage(canvas.0, canvas.1);
    for selection in [&[][..], &weights[..]] {
        let request = json!({"operation":"histogram","source":"layer","layer":0,"stride":2});
        let value = resized
            .scoped_analysis(&parse(&request), selection, LIMIT, 0, None)
            .unwrap();
        assert_eq!(
            pick(&value, &HISTOGRAM_KEYS),
            histogram_oracle(&expected, selection, 2)
        );
        assert_eq!(value["source"], "layer");
        for (x, y, reach) in [(0, 0, 1), (2, 1, 0), (10, 8, 2), (21, 15, 1)] {
            let request =
                json!({"operation":"sample","source":"layer","layer":0,"x":x,"y":y,"reach":reach});
            let answer = resized.scoped_analysis(&parse(&request), selection, LIMIT, 0, None);
            match sample_oracle(&expected, canvas, selection, (x, y, reach)) {
                Some(oracle) => assert_eq!(pick(&answer.unwrap(), &SAMPLE_KEYS), oracle),
                None => assert_eq!(answer.unwrap_err(), EMPTY_SAMPLE),
            }
        }
    }

    // A group over opaque samples renders its child's samples exactly.
    let solid = opaque(W, H, 5);
    let doc = float_document(W, H, &solid);
    let doubled = doc.duplicate_layer(0).unwrap().expect("duplicated");
    let (grouped, _) = doubled
        .group_layers(&[1], "Group", false)
        .unwrap()
        .expect("grouped");
    assert_eq!(grouped.layer_count(), 3);
    assert!(grouped.layer(2).and_then(|l| l.children()).is_some());
    let request = json!({"operation":"histogram","source":"layer","layer":2});
    let value = grouped
        .scoped_analysis(&parse(&request), &[], LIMIT, 0, None)
        .unwrap();
    assert_eq!(
        pick(&value, &HISTOGRAM_KEYS),
        histogram_oracle(&solid, &[], 1)
    );
}

#[test]
fn backdrop_cuts_above_the_boundary() {
    let solid = opaque(W, H, 6);
    let doc = float_document(W, H, &solid);
    let doubled = doc.duplicate_layer(0).unwrap().expect("duplicated");
    let weights = coverage(W, H);
    for selection in [&[][..], &weights[..]] {
        let below_one = json!({"operation":"histogram","source":"backdrop","below":1});
        let value = doubled
            .scoped_analysis(&parse(&below_one), selection, LIMIT, 0, None)
            .unwrap();
        assert_eq!(
            pick(&value, &HISTOGRAM_KEYS),
            histogram_oracle(&solid, selection, 1)
        );
        assert_eq!(value["source"], "backdrop");
        let below_zero = json!({"operation":"histogram","source":"backdrop","below":0});
        let empty = doubled
            .scoped_analysis(&parse(&below_zero), selection, LIMIT, 0, None)
            .unwrap();
        let transparent = vec![[0.0f32; 4]; (W * H) as usize];
        assert_eq!(
            pick(&empty, &HISTOGRAM_KEYS),
            histogram_oracle(&transparent, selection, 1)
        );
        assert_eq!(empty["full_rgb_weight_units"], 0);
        assert_eq!(empty["full_rgb_pixel_count"], 0);
    }
}

#[test]
fn float_projection_answers_the_document_composite() {
    let samples = extended(W, H, 7);
    let doc = float_document(W, H, &samples);
    let projection = doc.render(LIMIT, 0, None).unwrap();
    let weights = coverage(W, H);
    let none = vec![0u16; (W * H) as usize];
    let mut requests = vec![histogram(1), histogram(3)];
    requests.extend(PROBES.iter().map(|(x, y, r)| sample(*x, *y, *r)));
    for selection in [&[][..], &weights[..], &none[..]] {
        for request in &requests {
            let parsed = parse(request);
            // A document's float `sample` adds `center_display_codes`,
            // the centre's R1 display code, which needs the document's policy
            // a bare projection does not carry; every other key agrees.
            let strip = |mut value: Value| {
                if let Some(object) = value.as_object_mut() {
                    object.remove("center_display_codes");
                }
                value
            };
            let from_doc = doc.scoped_analysis(&parsed, selection, LIMIT, 0, None);
            if let Ok(value) = &from_doc {
                if request["operation"] == "sample" {
                    assert!(value.get("center_display_codes").is_some(), "{request}");
                }
            }
            let from_doc = from_doc.map(strip);
            let from_projection = projection.scoped_analysis(&parsed, selection, LIMIT, 0, None);
            assert_eq!(from_doc, from_projection, "{request}");
            let through_doc = doc_ffi(&doc, request, selection).map(strip);
            let through_image = image_ffi(&projection, request, selection);
            match from_doc {
                Ok(value) => {
                    let value = round_trip(value);
                    assert_eq!(through_doc.as_ref().unwrap(), &value, "{request}");
                    assert_eq!(through_image.as_ref().unwrap(), &value, "{request}");
                }
                Err(message) => {
                    assert_eq!(message, EMPTY_SAMPLE, "{request}");
                    assert!(through_doc.unwrap_err().contains(EMPTY_SAMPLE));
                    assert!(through_image.unwrap_err().contains(EMPTY_SAMPLE));
                }
            }
        }
    }
}

#[test]
fn plan_charges_a_float_render_and_admission_and_cancel_hold() {
    let solid = opaque(W, H, 8);
    let doc = float_document(W, H, &solid);
    let doubled = doc.duplicate_layer(0).unwrap().expect("duplicated");
    let (grouped, _) = doubled
        .group_layers(&[1], "Group", false)
        .unwrap()
        .expect("grouped");
    let request = parse(&json!({"operation":"histogram","source":"layer","layer":2}));
    assert_eq!(
        grouped.scoped_analysis_plan(&request).unwrap(),
        STATISTICS_SCRATCH
            + 1024 * 1024
            + grouped.render_plan().unwrap().scratch_bytes
            + u64::from(W) * u64::from(H) * 16
    );
    assert!(grouped.scoped_analysis(&request, &[], 1, 0, None).is_err());
    let reference = grouped
        .scoped_analysis(&request, &[], LIMIT, 0, None)
        .unwrap();
    let token = DrJobControl::default();
    token.cancel();
    assert!(grouped
        .scoped_analysis(&request, &[], LIMIT, 0, Some(&token))
        .unwrap_err()
        .contains("cancelled"));
    assert_eq!(token.scratch_bytes(), 0);
    let fresh = DrJobControl::default();
    assert_eq!(
        grouped
            .scoped_analysis(&request, &[], LIMIT, 0, Some(&fresh))
            .unwrap(),
        reference
    );
}

// ------------------------------------------------------------ unit bins --

/// The unit bin written out: `round(clamp(v, 0, 1) · 255)` in binary64.
fn unit_bin(v: f32) -> usize {
    (f64::from(v).clamp(0.0, 1.0) * 255.0).round() as usize
}

/// The RGB and luma plot rows of a unit-binned histogram, written out.
fn unit_rows(canvas: &[[f32; 4]], coverage: &[u16], stride: usize) -> Vec<Vec<u64>> {
    let mut bins = vec![vec![0u64; 256]; 4];
    for (i, p) in canvas.iter().enumerate() {
        let w = weight_at(coverage, i);
        if w == 0 || i % stride != 0 || p[3] == 0.0 {
            continue;
        }
        for c in 0..3 {
            bins[c][unit_bin(p[c])] += w;
        }
        let luma =
            0.2126_f32 * p[0].max(0.0) + 0.7152_f32 * p[1].max(0.0) + 0.0722_f32 * p[2].max(0.0);
        bins[3][unit_bin(luma)] += w;
    }
    bins
}

fn unit_histogram(stride: u32) -> Value {
    json!({"operation":"histogram","source":"composite","stride":stride,"float_bins":"unit"})
}

#[test]
fn unit_bins_plot_the_linear_axis_and_keep_every_count() {
    let samples = extended(W, H, 9);
    let image = image(W, H, &samples);
    let weights = coverage(W, H);
    for selection in [&[][..], &weights[..]] {
        for stride in [1u32, 3] {
            let stops = image
                .scoped_analysis(&parse(&histogram(stride)), selection, LIMIT, 0, None)
                .unwrap();
            let unit = image
                .scoped_analysis(&parse(&unit_histogram(stride)), selection, LIMIT, 0, None)
                .unwrap();
            let rows = unit_rows(&samples, selection, stride as usize);
            assert_eq!(unit["bin_weight_units"], json!(rows), "stride {stride}");
            let shown: Vec<Vec<f64>> = rows
                .iter()
                .map(|r| r.iter().map(|v| *v as f64 / 65535.0).collect())
                .collect();
            assert_eq!(unit["bins"], json!(shown));
            assert_eq!(unit["float_bins"], "unit");
            assert_eq!(unit["range"], json!({"min":0.0,"max":1.0}));
            assert_eq!(
                unit["display_bin_rule"],
                "unit bins: round(clamp(v, 0, 1) * 255), below 0 in bin 0 and above 1 in bin 255; luma before binning"
            );
            // Every other key is the stops reply's.
            let strip = |value: &Value| {
                let mut object = value.as_object().unwrap().clone();
                for key in [
                    "bins",
                    "bin_weight_units",
                    "range",
                    "display_bin_rule",
                    "float_bins",
                ] {
                    object.remove(key);
                }
                object
            };
            assert_eq!(strip(&unit), strip(&stops));
            // The explicit "stops" request is today's reply, byte for byte.
            let explicit = json!({"operation":"histogram","source":"composite",
                "stride":stride,"float_bins":"stops"});
            let named = image
                .scoped_analysis(&parse(&explicit), selection, LIMIT, 0, None)
                .unwrap();
            assert_eq!(named.to_string(), stops.to_string());
            assert!(stops.get("float_bins").is_none());
        }
    }
}

#[test]
fn unit_bins_equal_the_u16_code_bins_of_the_same_values() {
    // Codes off the half-bin boundaries, so f32 rounding of c / 65535 cannot
    // move a value across one.
    let codes: Vec<[u16; 4]> = (0..W * H)
        .map(|i| {
            let code = |k: u32| {
                let c = ((i * 7919 + k * 104_729) % 65536) as u16;
                let scaled = f64::from(c) * 255.0 / 65535.0;
                if (scaled - scaled.floor() - 0.5).abs() < 1e-3 {
                    c.wrapping_add(3)
                } else {
                    c
                }
            };
            [code(0), code(1), code(2), 65535]
        })
        .collect();
    let floats: Vec<[f32; 4]> = codes
        .iter()
        .map(|p| p.map(|c| (f64::from(c) / 65535.0) as f32))
        .collect();
    let code_image =
        PrecisionImage::from_rgba16(W, H, codes.clone(), IccProfile::srgb(), None).unwrap();
    let float_image = image(W, H, &floats);
    let code_reply = code_image
        .scoped_analysis(&parse(&histogram(1)), &[], LIMIT, 0, None)
        .unwrap();
    let unit_reply = float_image
        .scoped_analysis(&parse(&unit_histogram(1)), &[], LIMIT, 0, None)
        .unwrap();
    for c in 0..3 {
        assert_eq!(
            unit_reply["bin_weight_units"][c], code_reply["bin_weight_units"][c],
            "channel {c}"
        );
    }
    // A u16 image ignores float_bins: the reply is its plain histogram.
    let ignored = code_image
        .scoped_analysis(&parse(&unit_histogram(1)), &[], LIMIT, 0, None)
        .unwrap();
    assert_eq!(ignored.to_string(), code_reply.to_string());
}

#[test]
fn float_bins_on_a_sample_refuses() {
    let request = json!({"operation":"sample","source":"composite","x":1,"y":1,
        "float_bins":"unit"});
    assert_eq!(
        AnalysisRequest::parse(&request.to_string()).unwrap_err(),
        "float_bins requires a histogram"
    );
    let bad = json!({"operation":"histogram","source":"composite","float_bins":"log"});
    assert!(AnalysisRequest::parse(&bad.to_string()).is_err());
}
