//! Image ▸ Mode ▸ Bitmap and Grayscale back (#1 Bitmap phases 1a and 1b):
//! the options, the plans and both conversions
//! (`designs/bitmap-mode.md`).
//!
//! * **The options** are `method` (`threshold`, `pattern`, `diffusion`, the
//!   default, `halftone` or `custom`), `resolution`, `flatten`, Halftone
//!   Screen's `frequency` (lines per inch, 1…999.999, 53), `angle` (−180…180°,
//!   45) and `shape` (`round`, the default, `ellipse`, `line`, `square`,
//!   `cross`, `diamond`), and Custom Pattern's `pattern`. A method's key
//!   beside another method refuses. The screen is laid over the OUTPUT
//!   resolution. The pattern store is `style_pattern_fill::parse_pattern`'s
//!   pattern object, the one every Pattern Overlay, Pattern stroke and
//!   pattern fill layer takes: `{"preset"}` of `pattern_tile::PRESETS` or an
//!   inline `{"name"?, "width", "height", "rgba"}` tile (checkerboard when
//!   omitted); the app holds no other pattern library. Its gray is
//!   [`threshold_tile`]'s.
//!
//! * **Gray → Bitmap** converts an 8-bit Grayscale document of ONE plain
//!   layer (`indexed_convert::plain_image`, Indexed's rule); any other
//!   answers `flatten_required` and `flatten` runs Flatten Image first.
//!   Bitmap itself, every other mode and a 16- or 32-bit Gray document
//!   refuse by name, naming the Grayscale or 8 Bits/Channel step
//!   ([`PrecisionDocument::to_bitmap_hard_refusal`]). The Output resolution
//!   `r` decides each axis on its own: omitted, or equal to that axis's
//!   stored ppi (compared as `f32`), the axis keeps its pixels and ppi;
//!   otherwise it becomes `max(1, round(n·r/ppi))` pixels at `r` ppi, and a
//!   changed canvas is resampled by Image Size's own Bicubic
//!   (`precision_geometry`'s `Resize`, which moves the guides, samplers and
//!   paths). Each pixel is then read as 8-bit codes
//!   (`indexed_palette::code8`), put over white (`indexed_palette::prepare`
//!   without Transparency), and the gray goes through the method
//!   (`bitmap_method::Binarizer`) into the packed plane. The result is one
//!   "Bitmap" layer in the source's Gray profile and twin, no alpha
//!   channel, with the guides, samplers, paths, light and metadata carried.
//! * **Bitmap → Gray** at Size Ratio 1 is the Gray document the storage
//!   already is (`without_bitmap`): every sample shared, exact. At a ratio
//!   `r` > 1 each `r × r` block is averaged (`bitmap_method`) onto a
//!   `ceil(w/r) × ceil(h/r)` canvas at `ppi / r`, the chrome moved as Image
//!   Size moves it.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::bitmap_document::{BitmapPolicy, LAYER_NAME};
use crate::bitmap_method::{
    block_average_row, pack_row, ratio_dimensions, stride, Binarizer, Method, Screen, SpotShape,
    ThresholdTile, MAX_FREQUENCY, MAX_SIZE_RATIO, MIN_FREQUENCY,
};
use crate::indexed_convert::plain_image;
use crate::indexed_palette::{code8, prepare};
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::{Resolution, MAX_PPI, MIN_PPI};
use crate::precision_document::{PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_geometry::GeometryEdit;
use crate::precision_image::{admit, PrecisionImage, PrecisionRun, BOOKKEEPING_BYTES};
use crate::precision_sampling::GeometryFilter;
use crate::style::Strictness;
use crate::style_pattern::Pattern;
use crate::style_pattern_fill::parse_pattern;
use crate::tile_store::TilePlane;

/// Bytes an output pixel of either conversion charges: the packed plane, the
/// "Bitmap" (or Gray) layer's RGBA16 tiles (8 B/px), one row's grays and
/// bits and the result's display preparation the host runs right after;
/// Gray's measured conversion shape (`gray_convert::TO_GRAY_BYTES_PER_PIXEL`,
/// as Indexed's `TO_INDEXED_BYTES_PER_PIXEL`), beside the method's exact
/// scratch (`bitmap_method::Method::scratch_bytes`). Held by
/// `scripts/bitmap-admission/measure.py` to ⌈1.25 × its
/// measured peak⌉; a miss raises this term.
pub(crate) const TO_BITMAP_BYTES_PER_PIXEL: u64 = 41;

/// The sentence beside the host's "Flatten layers?" choice.
pub(crate) const FLATTEN_SENTENCE: &str = "Bitmap holds one layer: flatten the image.";

/// Halftone Screen's defaults: 53 lines per inch at 45°, Round.
const DEFAULT_SCREEN: Screen = Screen {
    frequency: 53.0,
    angle: 45.0,
    shape: SpotShape::Round,
    ppi: (72.0, 72.0),
};

/// The `method` names.
const METHODS: [&str; 5] = ["threshold", "pattern", "diffusion", "halftone", "custom"];

/// The keys only Halftone Screen reads.
const HALFTONE_KEYS: [&str; 3] = ["frequency", "angle", "shape"];

/// The Mode ▸ Bitmap options (`dr_precision_doc_convert_to_bitmap`).
struct BitmapOptions {
    /// The method as parsed; a Halftone screen's `ppi` is set at the output
    /// resolution by [`BitmapOptions::method_at`].
    method: Method,
    /// The Output resolution in ppi; `None` keeps both axes.
    resolution: Option<f64>,
    flatten: bool,
}

/// `text` as a JSON object (empty is `{}`).
fn object_of(text: &str) -> Result<Map<String, Value>, String> {
    let value: Value = serde_json::from_str(if text.trim().is_empty() { "{}" } else { text })
        .map_err(|e| format!("mode options are not valid JSON: {e}"))?;
    match value {
        Value::Object(object) => Ok(object),
        _ => Err("mode options must be a JSON object".into()),
    }
}

/// Refuses a key outside `known`.
fn known_keys(object: &Map<String, Value>, known: &[&str]) -> Result<(), String> {
    match object.keys().find(|key| !known.contains(&key.as_str())) {
        Some(key) => Err(format!("mode options have an unknown key \"{key}\"")),
        None => Ok(()),
    }
}

/// `object[key]` unless absent or null.
fn present<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    object.get(key).filter(|v| !v.is_null())
}

/// `object[key]` as a finite number in `range`, else `sentence`.
fn number_in(
    object: &Map<String, Value>,
    key: &str,
    range: std::ops::RangeInclusive<f64>,
    sentence: &str,
) -> Result<Option<f64>, String> {
    match present(object, key) {
        None => Ok(None),
        Some(value) => Ok(Some(
            value
                .as_f64()
                .filter(|v| v.is_finite() && range.contains(v))
                .ok_or(sentence)?,
        )),
    }
}

/// Halftone Screen's `frequency`, `angle` and `shape` over the defaults.
fn screen(object: &Map<String, Value>) -> Result<Screen, String> {
    let mut screen = DEFAULT_SCREEN;
    if let Some(f) = number_in(
        object,
        "frequency",
        MIN_FREQUENCY..=MAX_FREQUENCY,
        "mode option frequency must be 1 to 999.999 lines per inch",
    )? {
        screen.frequency = f;
    }
    if let Some(a) = number_in(
        object,
        "angle",
        -180.0..=180.0,
        "mode option angle must be -180 to 180 degrees",
    )? {
        screen.angle = a;
    }
    if let Some(value) = present(object, "shape") {
        screen.shape = value
            .as_str()
            .and_then(SpotShape::from_name)
            .ok_or("mode option shape must be round, ellipse, line, square, cross or diamond")?;
    }
    Ok(screen)
}

/// Custom Pattern's threshold map (`bitmap_method`): each texel over white
/// (`indexed_palette::prepare` without Transparency), its Rec. 709 luma
/// (`blend::luma_code`) as an 8-bit code (`indexed_palette::code8`).
fn threshold_tile(pattern: &Pattern) -> ThresholdTile {
    let tile = &pattern.tile;
    let gray: Vec<u8> = tile
        .rgba
        .chunks_exact(4)
        .map(|t| {
            let [r, g, b] = prepare([t[0], t[1], t[2], t[3]], false, None).unwrap_or([255; 3]);
            let wide = |c: u8| u16::from(c) * 257;
            code8(crate::blend::luma_code([wide(r), wide(g), wide(b), 65535]))
        })
        .collect();
    ThresholdTile {
        width: tile.width,
        height: tile.height,
        gray: gray.into(),
    }
}

impl BitmapOptions {
    /// `{"method"?,"resolution"?,"flatten"?,"frequency"?,"angle"?,"shape"?,
    /// "pattern"?}`; an unknown key, or a method's key beside another
    /// method, refuses.
    fn parse(text: &str) -> Result<Self, String> {
        let object = object_of(text)?;
        known_keys(
            &object,
            &[
                "method",
                "resolution",
                "flatten",
                "frequency",
                "angle",
                "shape",
                "pattern",
            ],
        )?;
        let name = match present(&object, "method") {
            None => "diffusion",
            Some(value) => value.as_str().unwrap_or(""),
        };
        if !METHODS.contains(&name) {
            return Err(
                "mode option method must be threshold, diffusion, pattern, halftone or custom"
                    .into(),
            );
        }
        if name != "halftone" {
            if let Some(key) = HALFTONE_KEYS.iter().find(|k| present(&object, k).is_some()) {
                return Err(format!("mode option {key} applies to method halftone"));
            }
        }
        if name != "custom" && present(&object, "pattern").is_some() {
            return Err("mode option pattern applies to method custom".into());
        }
        let method = match name {
            "threshold" => Method::Threshold,
            "pattern" => Method::Pattern,
            "diffusion" => Method::Diffusion,
            "halftone" => Method::Halftone(screen(&object)?),
            _ => Method::Custom(threshold_tile(&parse_pattern(
                &object,
                "",
                Strictness::Strict,
            )?)),
        };
        let resolution = match object.get("resolution").filter(|v| !v.is_null()) {
            None => None,
            Some(value) => Some(
                value
                    .as_f64()
                    .filter(|r| r.is_finite() && (MIN_PPI..=MAX_PPI).contains(r))
                    .ok_or("mode option resolution must be 1 to 30000 pixels per inch")?,
            ),
        };
        let flatten = match object.get("flatten") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(v)) => *v,
            Some(_) => return Err("mode option flatten must be a boolean".into()),
        };
        Ok(Self {
            method,
            resolution,
            flatten,
        })
    }

    /// The method sampled at the output `resolution`: a Halftone screen's
    /// lines per inch are laid over the output pixels.
    fn method_at(&self, resolution: Resolution) -> Method {
        match &self.method {
            Method::Halftone(screen) => Method::Halftone(Screen {
                ppi: (f64::from(resolution.x), f64::from(resolution.y)),
                ..*screen
            }),
            other => other.clone(),
        }
    }
}

/// Bitmap → Gray's `{"size_ratio"?}` (1 to [`MAX_SIZE_RATIO`], default 1).
fn size_ratio(text: &str) -> Result<u32, String> {
    let object = object_of(text)?;
    known_keys(&object, &["size_ratio"])?;
    match object.get("size_ratio").filter(|v| !v.is_null()) {
        None => Ok(1),
        Some(value) => value
            .as_u64()
            .filter(|r| (1..=u64::from(MAX_SIZE_RATIO)).contains(r))
            .map(|r| r as u32)
            .ok_or_else(|| "mode option size_ratio must be an integer 1 to 16".into()),
    }
}

/// One axis under the Output resolution (module doc): its pixels and ppi.
fn output_axis(pixels: u32, ppi: f32, resolution: Option<f64>) -> (u32, f32) {
    match resolution {
        Some(r) if r as f32 != ppi => {
            let scaled = (f64::from(pixels) * r / f64::from(ppi)).round();
            (scaled.clamp(1.0, f64::from(u32::MAX)) as u32, r as f32)
        }
        _ => (pixels, ppi),
    }
}

/// The 8-bit grays of row `y` of `image`, over white.
fn gray_row(image: &PrecisionImage, y: u32, out: &mut Vec<u8>) -> Result<(), String> {
    out.clear();
    for (_, run) in image.row_segments(y, 0, image.dimensions().0) {
        let PrecisionRun::Rgba16(run) = run else {
            return Err("Bitmap converts an 8-bit Grayscale document".into());
        };
        out.extend(run.iter().map(|p| {
            let rgba = p.map(crate::indexed_palette::code8);
            crate::indexed_palette::prepare(rgba, false, None).map_or(255, |rgb| rgb[0])
        }));
    }
    Ok(())
}

/// The resample edit to `(width, height)`.
fn resize(width: u32, height: u32) -> GeometryEdit {
    GeometryEdit::Resize {
        width,
        height,
        filter: GeometryFilter::Bicubic,
    }
}

impl PrecisionDocument {
    /// Why this document cannot become Bitmap even with a flatten, or `None`.
    fn to_bitmap_hard_refusal(&self) -> Option<String> {
        if self.is_bitmap() {
            return Some("The document is already in Bitmap mode.".into());
        }
        // A Duotone document is Gray storage with inks; this direct route
        // refuses it, and Duotone → Bitmap goes through
        // `duotone_gray::convert_duotone_to_bitmap` (its Grayscale rule first).
        let Some(gray) = self.gray_policy().filter(|_| !self.is_duotone()) else {
            return Some(
                "Bitmap converts an 8-bit Grayscale document; convert to \
                 Grayscale first."
                    .into(),
            );
        };
        if self.is_float() || gray.depth() != crate::gray_document::GrayDepth::Eight {
            return Some(
                "Bitmap converts an 8-bit Grayscale document; convert to 8 \
                 Bits/Channel first."
                    .into(),
            );
        }
        if self.has_spot_channel() {
            return Some(crate::spot_channel::TO_BITMAP_REFUSAL.into());
        }
        None
    }

    /// The output canvas and resolution under `options` (module doc).
    fn bitmap_output(&self, options: &BitmapOptions) -> (u32, u32, Resolution) {
        let (w, h) = self.dimensions();
        let (nw, x) = output_axis(w, self.resolution.x, options.resolution);
        let (nh, y) = output_axis(h, self.resolution.y, options.resolution);
        (nw, nh, Resolution { x, y }.sane())
    }

    /// The to-Bitmap working memory: Flatten Image's render when it
    /// flattens, then the larger of the resample's own plan (when it
    /// resamples) and [`TO_BITMAP_BYTES_PER_PIXEL`] an output pixel, then
    /// the method's scratch and bookkeeping. The larger, not the sum: the
    /// resampled Gray document IS the output-sized source the per-pixel
    /// shape already counts (1a's measured peak was 32 B an output pixel
    /// with or without a resample). A resample the geometry plan refuses is
    /// the refusal.
    fn to_bitmap_scratch(&self, options: &BitmapOptions, flattens: bool) -> Result<u64, String> {
        let (nw, nh, _) = self.bitmap_output(options);
        let flattening = if flattens {
            self.render_plan().map(|p| p.scratch_bytes).unwrap_or(0)
        } else {
            0
        };
        let resampling = if (nw, nh) != self.dimensions() {
            self.geometry_plan(&resize(nw, nh))?.scratch_bytes
        } else {
            0
        };
        let per_pixel = (u64::from(nw) * u64::from(nh)).saturating_mul(TO_BITMAP_BYTES_PER_PIXEL);
        Ok(flattening
            .saturating_add(resampling.max(per_pixel))
            .saturating_add(options.method.scratch_bytes(nw))
            .saturating_add(BOOKKEEPING_BYTES))
    }

    /// `dr_precision_doc_convert_to_bitmap_plan`'s answer:
    /// `{"working_memory_plan_bytes","refusal","flatten_required","flattens",
    /// "width","height"}`.
    pub fn convert_to_bitmap_plan(&self, options: &str) -> Result<Value, String> {
        let options = BitmapOptions::parse(options)?;
        let hard = self.to_bitmap_hard_refusal();
        let blocked = plain_image(self).is_none();
        let flattens = blocked && options.flatten;
        let (width, height, _) = self.bitmap_output(&options);
        let (scratch, resample) = match self.to_bitmap_scratch(&options, flattens) {
            Ok(bytes) => (bytes, None),
            Err(e) => (0, Some(e)),
        };
        let refusal = hard
            .or_else(|| (blocked && !options.flatten).then(|| FLATTEN_SENTENCE.to_string()))
            .or(resample);
        Ok(json!({
            "working_memory_plan_bytes": scratch,
            "refusal": refusal,
            "flatten_required": blocked,
            "flattens": flattens,
            "width": width,
            "height": height,
        }))
    }

    /// Gray → Bitmap (module doc).
    pub fn convert_to_bitmap(
        &self,
        options: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let options = BitmapOptions::parse(options)?;
        if let Some(reason) = self.to_bitmap_hard_refusal() {
            return Err(reason);
        }
        let blocked = plain_image(self).is_none();
        if blocked && !options.flatten {
            return Err(FLATTEN_SENTENCE.into());
        }
        let scratch = self.to_bitmap_scratch(&options, blocked)?;
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let (width, height, resolution) = self.bitmap_output(&options);
        let flattened;
        let source = if blocked {
            flattened = self
                .flatten_image(limit, other, job.range(0.0, 0.2))?
                .ok_or("flattening the image changed nothing")?;
            &flattened
        } else {
            self
        };
        let resampled;
        let source = if (width, height) != source.dimensions() {
            resampled = source.geometry_with_context(
                &resize(width, height),
                limit,
                other,
                job.range(0.2, 0.4),
            )?;
            resampled.as_ref().unwrap_or(source)
        } else {
            source
        };
        let image = plain_image(source).ok_or(FLATTEN_SENTENCE)?;
        let gray_policy = source
            .gray_policy()
            .ok_or("Bitmap converts an 8-bit Grayscale document")?
            .clone();
        let line = stride(width);
        let mut packed = crate::precision_image::reserved_vec(line * height as usize)?;
        packed.resize(line * height as usize, 0u8);
        let mut binarizer = Binarizer::new(options.method_at(resolution), width)?;
        let (mut grays, mut bits) = (Vec::new(), vec![0u8; width as usize]);
        let stage = job.range(0.4, 0.9);
        if line > 0 {
            for (y, out) in packed.chunks_exact_mut(line).enumerate() {
                gray_row(image, y as u32, &mut grays)?;
                binarizer.row(y as u32, &grays, &mut bits);
                pack_row(&bits, out);
                // Once a row (as `indexed_convert`).
                stage.report((y + 1) as f64 / f64::from(height), 5)?;
            }
        }
        drop(binarizer);
        let policy = BitmapPolicy::new(packed, width, height)?;
        let profile = source.profile().clone();
        let rendered = policy.render(&profile, job.range(0.9, 1.0))?;
        let layer = PrecisionDocumentLayer::raster(LAYER_NAME, rendered, (0, 0), 1.0, true)?;
        let mut out = PrecisionDocument::build(
            width,
            height,
            profile,
            source.metadata.clone(),
            resolution,
            vec![layer],
            Vec::new(),
            None,
        )?;
        out.guides.clone_from(&source.guides);
        out.ruler_origin = source.ruler_origin;
        out.color_samplers.clone_from(&source.color_samplers);
        out.global_light = source.global_light;
        out.paths.set_entries(source.paths.entries.to_vec());
        let out = out.with_gray(gray_policy)?.with_bitmap(Arc::new(policy))?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The Bitmap → Gray canvas at `ratio`.
    fn gray_output(&self, ratio: u32) -> (u32, u32) {
        let (w, h) = self.dimensions();
        if ratio <= 1 {
            (w, h)
        } else {
            ratio_dimensions(w, h, ratio)
        }
    }

    /// `dr_precision_doc_convert_bitmap_to_gray_plan`'s answer:
    /// `{"working_memory_plan_bytes","refusal","width","height"}`.
    pub fn convert_bitmap_to_gray_plan(&self, options: &str) -> Result<Value, String> {
        let ratio = size_ratio(options)?;
        let refusal = (!self.is_bitmap()).then_some("the document is not a Bitmap document");
        let (width, height) = self.gray_output(ratio);
        let scratch = (u64::from(width) * u64::from(height))
            .saturating_mul(TO_BITMAP_BYTES_PER_PIXEL)
            .saturating_add(BOOKKEEPING_BYTES);
        Ok(json!({
            "working_memory_plan_bytes": scratch,
            "refusal": refusal,
            "width": width,
            "height": height,
        }))
    }

    /// Bitmap → Gray (module doc).
    pub fn convert_bitmap_to_gray(
        &self,
        options: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let ratio = size_ratio(options)?;
        let Some(policy) = self.bitmap_policy() else {
            return Err("the document is not a Bitmap document".into());
        };
        if ratio == 1 {
            return Ok(self.without_bitmap());
        }
        let (width, height) = self.gray_output(ratio);
        let scratch = (u64::from(width) * u64::from(height))
            .saturating_mul(TO_BITMAP_BYTES_PER_PIXEL)
            .saturating_add(BOOKKEEPING_BYTES);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let gray_policy = self
            .gray_policy()
            .ok_or("the document is not a Bitmap document")?
            .clone();
        let (w, h) = policy.dimensions();
        let mut grays = vec![0u8; width as usize];
        let stage = job.range(0.0, 0.9);
        let plane = TilePlane::<[u16; 4]>::from_rows_with_job(
            width,
            height,
            |y, row| {
                block_average_row(policy.packed(), w, h, ratio, y, &mut grays);
                for (pixel, &g) in row.iter_mut().zip(&grays) {
                    let v = u16::from(g) * 257;
                    *pixel = [v, v, v, 65535];
                }
                stage.report(f64::from(y + 1) / f64::from(height), 5)
            },
            job,
        )?;
        let profile = self.profile().clone();
        let image = PrecisionImage::from_rgba16_tiles_unseeded(plane, profile.clone())?;
        let layer = PrecisionDocumentLayer::raster(LAYER_NAME, image, (0, 0), 1.0, true)?;
        let edit = resize(width, height);
        let (guides, ruler_origin) = self.geometry_chrome(&edit)?;
        let color_samplers =
            crate::doc_sampler::edited_samplers(&self.color_samplers, &edit, w, h)?;
        let paths = crate::precision_path::edited_paths(&self.paths.entries, &edit, w, h)?;
        let r = ratio as f32;
        let resolution = Resolution {
            x: self.resolution.x / r,
            y: self.resolution.y / r,
        }
        .sane();
        let mut out = PrecisionDocument::build(
            width,
            height,
            profile,
            self.metadata.clone(),
            resolution,
            vec![layer],
            Vec::new(),
            None,
        )?;
        out.guides = guides;
        out.ruler_origin = ruler_origin;
        out.color_samplers = color_samplers;
        out.global_light = self.global_light;
        out.paths.set_entries(paths);
        let out = out.with_gray(gray_policy)?;
        job.report(1.0, 5)?;
        Ok(out)
    }
}
