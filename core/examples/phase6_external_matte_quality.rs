//! Offline candidate adapter: inference sees only RGB, trimap and degraded prior.
//! Imported native L8 alpha is scored by the unchanged Rust qualification path.
//! No model/runtime is linked into the core or app. Use a separately pinned
//! inference script and record its model/environment/input/output provenance.
use image::{codecs::png::PngDecoder, ColorType, DynamicImage, GrayImage, ImageDecoder, RgbImage};
use serde_json::json;
use std::{
    fs::File,
    io::{BufReader, Read, Seek},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
#[allow(dead_code)]
#[path = "phase6_full_image_quality.rs"]
mod full;
use full::frozen as qualification;

struct Configuration {
    export: bool,
    inputs: PathBuf,
    outputs: PathBuf,
    cases: Mutex<Vec<serde_json::Value>>,
}
static CONFIGURATION: OnceLock<Configuration> = OnceLock::new();

/// Inspect actual PNG storage before decoder expansion or pixel allocation.
/// The same open file supplies inspection and decoding; no second path lookup.
pub(crate) fn decode_native_png(
    path: &Path,
    dimensions: (u32, u32),
    color: ColorType,
) -> Result<DynamicImage, String> {
    let failure = |error| format!("failed to read external PNG {}: {error}", path.display());
    let mut input = BufReader::new(File::open(path).map_err(failure)?);
    let mut header = [0_u8; 33];
    input.read_exact(&mut header).map_err(failure)?;
    if &header[..16] != b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR" {
        return Err(format!("external image {} must be a PNG", path.display()));
    }
    let actual = (
        u32::from_be_bytes(header[16..20].try_into().unwrap()),
        u32::from_be_bytes(header[20..24].try_into().unwrap()),
    );
    if actual != dimensions || actual.0 == 0 || actual.1 == 0 {
        return Err(format!(
            "external PNG {} dimensions differ from the native source",
            path.display()
        ));
    }
    let expected_mode = match color {
        ColorType::L8 => 0,
        ColorType::Rgb8 => 2,
        _ => return Err("unsupported external PNG storage contract".into()),
    };
    if header[24] != 8 || header[25] != expected_mode {
        return Err(format!(
            "external PNG {} must use native {color:?} storage",
            path.display()
        ));
    }
    input.rewind().map_err(failure)?;
    // Decoder metadata/scanlines and its output allowance remain inside the
    // harness's 256N+64MiB bound. PNG enforces geometry strictly; max_alloc is
    // the image library's conservative, best-effort decoder allocation limit.
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(dimensions.0);
    limits.max_image_height = Some(dimensions.1);
    limits.max_alloc = Some(
        u64::from(dimensions.0)
            .checked_mul(u64::from(dimensions.1))
            .and_then(|n| n.checked_mul(16))
            .and_then(|n| n.checked_add(1024 * 1024))
            .ok_or("external PNG decoder allocation overflow")?,
    );
    limits
        .reserve(
            u64::from(dimensions.0) * u64::from(dimensions.1) * u64::from(color.bytes_per_pixel()),
        )
        .map_err(|e| format!("external PNG output allocation refused: {e}"))?;
    let decoder = PngDecoder::with_limits(input, limits)
        .map_err(|e| format!("failed to inspect external PNG {}: {e}", path.display()))?;
    if decoder.dimensions() != dimensions || decoder.color_type() != color {
        return Err(format!(
            "external PNG {} decoder storage differs from the native contract",
            path.display()
        ));
    }
    DynamicImage::from_decoder(decoder)
        .map_err(|e| format!("failed to decode external PNG {}: {e}", path.display()))
}

/// Canonical parents prevent a symlink from placing qualification artifacts in
/// the repository. New destinations deliberately require an existing parent.
pub(crate) fn outside_repository(path: &Path) -> Result<PathBuf, String> {
    let resolve_error = |e| format!("failed to resolve artifact path {}: {e}", path.display());
    let resolved = if std::fs::symlink_metadata(path).is_ok() {
        path.canonicalize().map_err(resolve_error)?
    } else {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        parent
            .canonicalize()
            .map_err(resolve_error)?
            .join(path.file_name().ok_or("artifact path has no filename")?)
    };
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .canonicalize()
        .map_err(resolve_error)?;
    if resolved.starts_with(repository) {
        return Err("qualification artifacts must be outside the repository".into());
    }
    Ok(resolved)
}

fn external(
    name: &str,
    colors: &[[f32; 3]],
    width: u32,
    height: u32,
    trimap: &[u8],
    prior: &[u8],
) -> Result<qualification::AlphaResult, String> {
    let config = CONFIGURATION
        .get()
        .ok_or("missing external candidate configuration")?;
    if Path::new(name).file_name().and_then(|v| v.to_str()) != Some(name) {
        return Err("invalid external case name".into());
    }
    let input = config.inputs.join(name);
    let rgb: Vec<u8> = colors
        .iter()
        .flatten()
        .map(|v| (v * 255.0).round() as u8)
        .collect();
    let source = RgbImage::from_raw(width, height, rgb).ok_or("invalid external RGB dimensions")?;
    let labels = GrayImage::from_raw(width, height, trimap.to_vec())
        .ok_or("invalid external trimap dimensions")?;
    let initial = GrayImage::from_raw(width, height, prior.to_vec())
        .ok_or("invalid external prior dimensions")?;
    if config.export {
        std::fs::create_dir(&input)
            .map_err(|e| format!("failed to create external input {}: {e}", input.display()))?;
        source
            .save(input.join("source.png"))
            .map_err(|e| e.to_string())?;
        labels
            .save(input.join("trimap.png"))
            .map_err(|e| e.to_string())?;
        initial
            .save(input.join("prior.png"))
            .map_err(|e| e.to_string())?;
        config
            .cases
            .lock()
            .map_err(|_| "external input manifest lock failed")?
            .push(json!({
                "case":name,"width":width,"height":height,
                "source":format!("{name}/source.png"),"trimap":format!("{name}/trimap.png"),
                "prior":format!("{name}/prior.png"),"domain":"encoded RGB",
                "model_inputs":["source RGB","categorical trimap"],
                "prior_use":"degraded source of the frozen trimap; never reference alpha"
            }));
        return Err("input export only; no candidate was evaluated".into());
    }
    // Reconstruct the frozen inputs independently on this scoring run. A wrong
    // crop, preprocessing rewrite or stale/mismatched case refuses before any
    // imported alpha is eligible for measurement. Truth is never exported.
    let dimensions = (width, height);
    if decode_native_png(&input.join("source.png"), dimensions, ColorType::Rgb8)?.as_bytes()
        != source.as_raw()
        || decode_native_png(&input.join("trimap.png"), dimensions, ColorType::L8)?.as_bytes()
            != labels.as_raw()
        || decode_native_png(&input.join("prior.png"), dimensions, ColorType::L8)?.as_bytes()
            != initial.as_raw()
    {
        return Err(
            "external inference inputs differ from the frozen native source/trimap/prior".into(),
        );
    }
    let output = config.outputs.join(name).join("alpha.png");
    let DynamicImage::ImageLuma8(alpha) = decode_native_png(&output, dimensions, ColorType::L8)?
    else {
        return Err("external matte must be an 8-bit grayscale PNG".into());
    };
    Ok(qualification::AlphaResult {
        foreground: None,
        alpha,
        diagnostics: json!({"artifact":output,"input_pixels_verified":true,
            "solver":"external pinned inference; see companion provenance"}),
        confidence: None,
    })
}

fn main() {
    let mut args: Vec<_> = std::env::args().collect();
    let full_images = args.get(1).is_some_and(|value| value == "--full-images");
    if full_images {
        args.remove(1);
    }
    assert!(args.len() == 5 || args.len() == 6,
        "usage: phase6_external_matte_quality export|score report-directory input-directory output-directory [verified-photo-references]");
    let export = match args[1].as_str() {
        "export" => true,
        "score" => false,
        _ => panic!("expected export or score"),
    };
    // Resolve every artifact destination before creating any of them.
    let report = outside_repository(Path::new(&args[2])).expect("external report path");
    let inputs = outside_repository(Path::new(&args[3])).expect("external input path");
    let outputs = outside_repository(Path::new(&args[4])).expect("external output path");
    // New destinations keep the frozen reports and input archives untouched.
    std::fs::create_dir(&report).expect("new report directory");
    if export {
        std::fs::create_dir(&inputs).expect("new input directory");
    }
    CONFIGURATION
        .set(Configuration {
            export,
            inputs: inputs.clone(),
            outputs,
            cases: Mutex::new(Vec::new()),
        })
        .unwrap_or_else(|_| panic!("external candidate already configured"));
    let candidate = qualification::Candidate {
        protect_foreground: false,
        input_policy: qualification::InputPolicy::LegacyAutomaticHardLabels,
        name: if export {
            "external-input-export"
        } else {
            "external-matte"
        },
        estimate: external,
    };
    if full_images {
        full::run(
            &report,
            Path::new(args.get(5).expect("verified full-image references")),
            candidate,
            2 * 1024 * 1024 * 1024,
        );
    } else {
        qualification::run(&report, args.get(5).map(Path::new), candidate);
    }
    if export {
        let attribution = report.join("ATTRIBUTION.md");
        if attribution.is_file() {
            std::fs::copy(attribution, inputs.join("ATTRIBUTION.md"))
                .expect("copy reference attribution beside exported inputs");
        }
        let cases = CONFIGURATION.get().unwrap().cases.lock().unwrap();
        let manifest = json!({"schema_version":1,"reference_alpha_exported":false,
            "contract":"native RGB and frozen prior-only categorical trimap; no reference alpha or foreground supplied to inference",
            "cases":*cases});
        std::fs::write(
            inputs.join("inputs.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        println!(
            "exported {} input cases; export diagnostics are not candidate quality results",
            cases.len()
        );
    }
}
