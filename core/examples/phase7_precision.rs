//! Phase 7A storage/neutral-compositing comparison, with independent f64 oracles.
//! Run `cargo run --release --example phase7_precision > /tmp/rz-phase7a.json`.
//! `--accuracy-only` omits the 24/48 MP measurements. No file codec, app editing,
//! HDR, color conversion or process-memory qualification is implied by this probe.

use std::sync::Arc;
use std::time::Instant;

use doctored_core::icc::IccProfile;
use doctored_core::precision_image::{PrecisionImage, PrecisionStorage};
use doctored_core::precision_render::{PrecisionLayer, PrecisionScene};
use serde_json::{json, Value};

const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const FORMATS: [PrecisionStorage; 2] =
    [PrecisionStorage::Rgba16Unorm, PrecisionStorage::Rgba32Float];

fn name(storage: PrecisionStorage) -> &'static str {
    match storage {
        PrecisionStorage::Rgba16Unorm => "rgba16_unorm",
        PrecisionStorage::Rgba32Float => "rgba32_float_normalized",
    }
}

fn pixel(index: u32, plane: u32) -> [u16; 4] {
    // Actual sixteen-bit source codes, including adjacent values and fractional
    // alpha. The modulus uses u64 so this remains defined at 48 MP.
    let n = u64::from(index);
    let p = u64::from(plane);
    [
        ((n + 30000 + p * 101) % 65536) as u16,
        ((n * 17 + p * 503) % 65536) as u16,
        ((n * 47 + p * 11) % 65536) as u16,
        if plane == 0 {
            65535
        } else {
            ((n * 31 + 1) % 65536) as u16
        },
    ]
}

fn image(
    width: u32,
    height: u32,
    plane: u32,
    storage: PrecisionStorage,
    profile: &Arc<IccProfile>,
) -> Result<PrecisionImage, String> {
    match storage {
        PrecisionStorage::Rgba16Unorm => PrecisionImage::from_rgba16(
            width,
            height,
            (0..width * height).map(|i| pixel(i, plane)).collect(),
            profile.clone(),
            None,
        ),
        PrecisionStorage::Rgba32Float => PrecisionImage::from_rgba32(
            width,
            height,
            (0..width * height)
                .map(|i| pixel(i, plane).map(|v| f32::from(v) / 65535.0))
                .collect(),
            profile.clone(),
            None,
        ),
    }
}

fn scene(
    width: u32,
    height: u32,
    images: &[PrecisionImage; 2],
    grouped: bool,
    profile: &Arc<IccProfile>,
) -> Result<PrecisionScene, String> {
    let base = PrecisionLayer::raster(images[0].clone(), (0, 0), 1.0)?;
    let top = PrecisionLayer::raster(images[1].clone(), (0, 0), 0.625)?;
    let top = if grouped {
        PrecisionLayer::isolated_normal(vec![top], 0.75)?
    } else {
        top
    };
    PrecisionScene::new(width, height, profile.clone(), vec![base, top], None, false)
}

fn decoded(image: &PrecisionImage, index: usize) -> [f64; 4] {
    let width = image.dimensions().0 as usize;
    let (x, y) = ((index % width) as u32, (index / width) as u32);
    if let Some(pixel) = image.rgba16_at(x, y) {
        pixel.map(|v| f64::from(v) / 65535.0)
    } else {
        image
            .sample_at(x, y)
            .expect("one typed payload")
            .map(f64::from)
    }
}

// Independently expressed premultiplied source-over, not the production W3C
// blend function or its straight-alpha three-term expansion.
fn over(background: [f64; 4], foreground: [f64; 4], opacity: f64) -> [f64; 4] {
    let source_alpha = foreground[3] * opacity;
    let destination_alpha = background[3] * (1.0 - source_alpha);
    let alpha = source_alpha + destination_alpha;
    if alpha == 0.0 {
        return [0.0; 4];
    }
    [
        (foreground[0] * source_alpha + background[0] * destination_alpha) / alpha,
        (foreground[1] * source_alpha + background[1] * destination_alpha) / alpha,
        (foreground[2] * source_alpha + background[2] * destination_alpha) / alpha,
        alpha,
    ]
}

fn expected(images: &[PrecisionImage; 2], index: usize, grouped: bool) -> [f64; 4] {
    let base = decoded(&images[0], index);
    let top = decoded(&images[1], index);
    if grouped {
        over(base, over([0.0; 4], top, 0.625), 0.75)
    } else {
        over(base, top, 0.625)
    }
}

fn error_bound(storage: PrecisionStorage) -> f64 {
    // Fixed before measurement: 2e-6 arithmetic allowance (0.131 uint16 code),
    // plus exactly half a uint16 code when final storage quantizes.
    2e-6 + match storage {
        PrecisionStorage::Rgba16Unorm => 0.5 / 65535.0,
        PrecisionStorage::Rgba32Float => 0.0,
    }
}

fn accuracy(storage: PrecisionStorage) -> Result<Value, String> {
    let profile = IccProfile::display_p3();
    let images = [
        image(65536, 1, 0, storage, &profile)?,
        image(65536, 1, 1, storage, &profile)?,
    ];
    let mut records = Vec::new();
    for grouped in [false, true] {
        let source = scene(65536, 1, &images, grouped, &profile)?;
        let output = source.render(storage, LIMIT, 0, None)?;
        let mut maximum = 0.0_f64;
        let mut squared = 0.0;
        for i in 0..65536 {
            let actual = decoded(&output, i);
            let reference = expected(&images, i, grouped);
            for c in 0..4 {
                let error = (actual[c] - reference[c]).abs();
                maximum = maximum.max(error);
                squared += error * error;
            }
        }
        records.push(json!({
            "isolated_group": grouped, "max_absolute_error": maximum,
            "rms_error": (squared / (65536.0 * 4.0)).sqrt(),
            "error_limit": error_bound(storage), "pass": maximum <= error_bound(storage)
        }));
    }
    // Opaque input enumerates every uint16 value in red. Two isolated boundaries
    // must preserve every code, rather than merely create colors from 8-bit input.
    let layer = PrecisionLayer::raster(images[0].clone(), (0, 0), 1.0)?;
    let nested = PrecisionLayer::isolated_normal(
        vec![PrecisionLayer::isolated_normal(vec![layer], 1.0)?],
        1.0,
    )?;
    let output = PrecisionScene::new(65536, 1, profile, vec![nested], None, false)?
        .render(storage, LIMIT, 0, None)?;
    let mismatches = (0..65536)
        .filter(|&i| decoded(&images[0], i) != decoded(&output, i))
        .count();
    let pass = mismatches == 0 && records.iter().all(|r| r["pass"] == true);
    Ok(json!({"storage": name(storage), "cases": records,
        "opaque_two_group_exact_mismatches": mismatches, "pass": pass}))
}

fn storage_small_steps(storage: PrecisionStorage) -> Value {
    // This is a numerical storage experiment, NOT an enabled adjustment or
    // another production tone kernel. One hundred exact +1e-6 writes have a
    // known unclipped result. Each increment is below half one uint16 code.
    let mut maximum = 0.0_f64;
    let mut squared = 0.0;
    for code in 16384..49152 {
        let original = f64::from(code) / 65535.0;
        let mut value = original as f32;
        let store = |v: f32| match storage {
            PrecisionStorage::Rgba16Unorm => (v * 65535.0).round() / 65535.0,
            PrecisionStorage::Rgba32Float => v,
        };
        for _ in 0..100 {
            value = store(value + 1e-6);
        }
        let actual = match storage {
            PrecisionStorage::Rgba16Unorm => f64::from((value * 65535.0).round()) / 65535.0,
            PrecisionStorage::Rgba32Float => f64::from(value),
        };
        let error = (actual - (original + 100.0 * 1e-6)).abs();
        maximum = maximum.max(error);
        squared += error * error;
    }
    json!({"storage": name(storage), "writes":100, "increment":1e-6, "codes":32768,
        "max_absolute_error":maximum,"rms_error":(squared/32768.0).sqrt(),
        "scope":"hypothetical storage writes; no editor operation is qualified"})
}

fn measurements(storage: PrecisionStorage, width: u32, height: u32) -> Result<Vec<Value>, String> {
    let profile = IccProfile::srgb();
    let start = Instant::now();
    let images = [
        image(width, height, 0, storage, &profile)?,
        image(width, height, 1, storage, &profile)?,
    ];
    let construction_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut records = Vec::new();
    for grouped in [false, true] {
        let source = scene(width, height, &images, grouped, &profile)?;
        let plan = source.plan(storage)?;
        let mut times = Vec::new();
        let mut refusal = None;
        let mut maximum = 0.0_f64;
        for _ in 0..3 {
            let start = Instant::now();
            match source.render(storage, LIMIT, 0, None) {
                Ok(output) => {
                    times.push(start.elapsed().as_secs_f64() * 1000.0);
                    // Full accuracy is measured separately above. These 257
                    // distributed samples catch wrong/missing large-frame work.
                    for sample in 0..257_u64 {
                        let i =
                            (sample * (u64::from(width) * u64::from(height) - 1) / 256) as usize;
                        let actual = decoded(&output, i);
                        let reference = expected(&images, i, grouped);
                        for c in 0..4 {
                            maximum = maximum.max((actual[c] - reference[c]).abs());
                        }
                    }
                }
                Err(error) => {
                    refusal = Some(error);
                    break;
                }
            }
        }
        let mut sorted = times.clone();
        sorted.sort_by(f64::total_cmp);
        let completed = refusal.is_none() && times.len() == 3;
        let median = if completed {
            sorted.get(1).copied()
        } else {
            None
        };
        let target_ms = if width == 6000 { 2000.0 } else { 4000.0 };
        records.push(
            json!({"storage":name(storage),"width":width,"height":height,
            "isolated_group":grouped,"construction_ms_two_images":construction_ms,
            "unique_retained_bytes":plan.unique_retained_bytes,
            "scratch_bytes":plan.scratch_bytes,"output_bytes":plan.output_bytes,
            "work_units":plan.work_units,"group_depth":plan.group_depth,
            "working_limit_bytes":LIMIT,"runs_ms":times,"median_ms":median,
            "latency_target_ms":target_ms,
            "latency_pass":completed && median.is_some_and(|v| v <= target_ms),
            "sample_max_absolute_error":maximum,
            "sample_accuracy_pass":refusal.is_none() && maximum <= error_bound(storage),
            "refusal":refusal}),
        );
    }
    Ok(records)
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a != "--accuracy-only") || args.len() > 1 {
        return Err("usage: phase7_precision [--accuracy-only]".into());
    }
    let mut numeric = Vec::new();
    let mut small_steps = Vec::new();
    let mut performance = Vec::new();
    for storage in FORMATS {
        numeric.push(accuracy(storage)?);
        small_steps.push(storage_small_steps(storage));
        if args.is_empty() {
            for (width, height) in [(6000, 4000), (8000, 6000)] {
                performance.extend(measurements(storage, width, height)?);
            }
        }
    }
    let pass = numeric.iter().all(|record| record["pass"] == true);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "contract_version":1, "accuracy_pass":pass,"accuracy":numeric,
            "storage_small_step_diagnostic":small_steps,"performance":performance,
            "scope":"Phase 7A safe Rust prototype; no document mode or persistence",
            "memory_semantics":"tracked payload and conservative scratch, not process RSS",
            "capacity_refusals_are_failures_of_that_size_target":true
        }))
        .map_err(|e| e.to_string())?
    );
    if !pass {
        return Err("phase 7a numerical precision gate failed".into());
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
