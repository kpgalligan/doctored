//! Image ▸ Mode ▸ Indexed Color and RGB Color back (#1 Indexed phase
//! 1a): the options, the plan and both conversions
//! (`designs/indexed-mode.md`).
//!
//! * **RGB / Gray → Indexed** converts an 8-bit RGB (the host promotes it
//!   first) or 8-bit Grayscale document of ONE plain layer — full canvas, at
//!   the origin, opacity 1, visible, Normal, with no mask, style, clipping,
//!   Blend If, description or binding; any other answers `flatten_required`
//!   and `flatten` runs Flatten Image first. Each
//!   pixel is read as 8-bit codes (`indexed_palette::code8`), put through
//!   Transparency and Matte (`indexed_palette::prepare`), and mapped onto
//!   the palette with no dither, Floyd–Steinberg diffusion, or phase 3c's
//!   Pattern or Noise (`indexed_dither`, declared candidates, phase 3d's
//!   verdict disclosed there; with the Exact palette both run the None arm).
//!   Forced Custom takes the options' `forced_table`; Local and Master
//!   Perceptual and Selective refuse for good (`UNPUBLISHED_PALETTES`: no
//!   candidate matched phase 3d's take). A Gray source's
//!   R = G = B is indexed as it is, and its twin becomes the Indexed
//!   document's profile. Alpha channels, guides, samplers, paths, the light,
//!   metadata and resolution carry; the result is one "Index" layer.
//!   Phase 1c's Custom palette is the options' `table` as it is
//!   (`indexed_act::custom`), the transparent entry appended as for the
//!   others.
//! * **Indexed → RGB** is the RGB document the storage already is
//!   (`without_indexed`): every sample shared, exact.
//!
//! Indexed → Grayscale needs no arm here: `gray_convert` converts each
//! pixel, a function of its palette entry alone.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::blend::BlendMode;
use crate::indexed_dither::Ordered;
use crate::indexed_document::{IndexedPolicy, LAYER_NAME};
use crate::indexed_palette::{
    ColorHistogram, Diffusion, ExactColors, Forced, NearestMap, Palette, Rgb, ADAPTIVE_BYTES,
    ADAPTIVE_COLOR_BYTES, EXACT_BYTES, MAX_COLORS, MEMO_BYTES, MIN_COLORS,
};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage, PrecisionRun, BOOKKEEPING_BYTES};

/// Bytes a canvas pixel RGB/Gray → Indexed charges: the index plane (1
/// B/px), the "Index" layer's RGBA16 tiles (8 B/px), one row's prepared
/// colours and the result's display preparation the host runs right after;
/// Gray's measured conversion shape (`gray_convert::TO_GRAY_BYTES_PER_PIXEL`),
/// beside the exact [`MEMO_BYTES`]. Held by
/// `scripts/indexed-admission/measure.py` (`to-indexed-8`,
/// `to-indexed-gray8`, `to-rgb-from-indexed`) to ⌈1.25 × its measured
/// peak⌉; a miss raises this term.
pub(crate) const TO_INDEXED_BYTES_PER_PIXEL: u64 = 41;

/// The sentence beside the host's "Flatten layers?" choice.
pub(crate) const FLATTEN_SENTENCE: &str = "Indexed Color holds one layer: flatten the image.";

/// The dialog's Dither (phase 3c adds Pattern and Noise, `indexed_dither`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dither {
    None,
    Diffusion,
    Pattern,
    Noise,
}

/// The palettes Adobe has not published and phase 3d's Photoshop take could
/// not match (`designs/indexed-mode.md`, "Photoshop's measured facts"):
/// Local Perceptual and Selective missed every declared and
/// fitted candidate, and Photoshop refused both Master palettes through the
/// action descriptor. Each refuses for good by name, pointing to Adaptive.
const UNPUBLISHED_PALETTES: [(&str, &str); 4] = [
    ("local_perceptual", "Local Perceptual"),
    ("local_selective", "Local Selective"),
    ("master_perceptual", "Master Perceptual"),
    ("master_selective", "Master Selective"),
];

/// The Mode ▸ Indexed Color options (`dr_precision_doc_convert_to_indexed`).
struct IndexedOptions {
    palette: Palette,
    /// The Custom palette's table (phase 1c); `None` for every other palette.
    table: Option<Vec<Rgb>>,
    colors: usize,
    forced: Forced,
    transparency: bool,
    matte: Option<Rgb>,
    dither: Dither,
    amount: u32,
    preserve_exact: bool,
    flatten: bool,
}

impl IndexedOptions {
    /// `{"palette","table"?,"colors"?,"forced"?,"forced_table"?,
    /// "transparency"?,"matte"?,"dither"?,"amount"?,"preserve_exact"?,
    /// "flatten"?}`; an unknown key refuses. `table` is the Custom
    /// palette's, required with it and refused elsewhere; `colors` does not
    /// apply to Custom. `forced_table` is Forced Custom's, required with it
    /// and refused elsewhere; `amount` and `preserve_exact` are Diffusion's.
    fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(if text.trim().is_empty() { "{}" } else { text })
            .map_err(|e| format!("mode options are not valid JSON: {e}"))?;
        let object: &Map<String, Value> = value
            .as_object()
            .ok_or("mode options must be a JSON object")?;
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "palette"
                    | "table"
                    | "colors"
                    | "forced"
                    | "forced_table"
                    | "transparency"
                    | "matte"
                    | "dither"
                    | "amount"
                    | "preserve_exact"
                    | "flatten"
            ) {
                return Err(format!("mode options have an unknown key \"{key}\""));
            }
        }
        let name = |key: &str| object.get(key).filter(|v| !v.is_null()).map(Value::as_str);
        if let Some(Some(wire)) = name("palette") {
            if let Some((_, title)) = UNPUBLISHED_PALETTES.iter().find(|(w, _)| *w == wire) {
                return Err(format!(
                    "The {title} palette is not available: its algorithm is unpublished. Use \
                     Adaptive (median cut) instead."
                ));
            }
        }
        let palette = match name("palette") {
            Some(Some(name)) => Palette::from_name(name),
            _ => None,
        }
        .ok_or(
            "mode option palette must be exact, web, uniform, adaptive, system_mac, system_windows \
             or custom",
        )?;
        let table = match (palette, object.get("table").filter(|v| !v.is_null())) {
            (Palette::Custom, Some(value)) => Some(crate::indexed_act::colors_from_json(
                value,
                "mode option table",
            )?),
            (Palette::Custom, None) => {
                return Err("the Custom palette needs mode option table".into())
            }
            (_, Some(_)) => {
                return Err("mode option table applies to the Custom palette only".into())
            }
            (_, None) => None,
        };
        if palette == Palette::Custom && object.get("colors").is_some_and(|v| !v.is_null()) {
            return Err(
                "mode option colors does not apply to the Custom palette: its table sets \
                        the count"
                    .into(),
            );
        }
        let colors = match object.get("colors").filter(|v| !v.is_null()) {
            None => MAX_COLORS,
            Some(v) => v
                .as_u64()
                .filter(|n| (MIN_COLORS as u64..=MAX_COLORS as u64).contains(n))
                .ok_or("mode option colors must be 2 to 256")? as usize,
        };
        let forced_table = object.get("forced_table").filter(|v| !v.is_null());
        let forced = match (name("forced"), forced_table) {
            (Some(Some("custom")), Some(value)) => Forced::Custom(
                crate::indexed_act::colors_from_json(value, "mode option forced_table")?,
            ),
            (Some(Some("custom")), None) => {
                return Err("forced custom needs mode option forced_table".into())
            }
            (_, Some(_)) => {
                return Err("mode option forced_table applies to forced custom only".into())
            }
            (None, None) => Forced::None,
            (Some(Some(name)), None) => Forced::from_name(name)
                .ok_or("mode option forced must be none, black_white, primaries, web or custom")?,
            (Some(None), None) => return Err("mode option forced must be a string".into()),
        };
        if forced != Forced::None && !matches!(palette, Palette::Uniform | Palette::Adaptive) {
            return Err("Forced colours apply to the Uniform and Adaptive palettes only.".into());
        }
        let flag = |key: &str, default: bool| -> Result<bool, String> {
            match object.get(key) {
                None | Some(Value::Null) => Ok(default),
                Some(Value::Bool(v)) => Ok(*v),
                Some(_) => Err(format!("mode option {key} must be a boolean")),
            }
        };
        let matte = match object.get("matte") {
            None | Some(Value::Null) => None,
            Some(Value::Array(items)) if items.len() == 3 => {
                let mut rgb = [0u8; 3];
                for (slot, item) in rgb.iter_mut().zip(items) {
                    *slot = item
                        .as_u64()
                        .filter(|v| *v <= 255)
                        .ok_or("mode option matte must be three codes 0 to 255")?
                        as u8;
                }
                Some(rgb)
            }
            Some(_) => return Err("mode option matte must be [r, g, b] or null".into()),
        };
        let dither = match name("dither") {
            None | Some(Some("diffusion")) => Dither::Diffusion,
            Some(Some("none")) => Dither::None,
            Some(Some("pattern")) => Dither::Pattern,
            Some(Some("noise")) => Dither::Noise,
            _ => return Err("mode option dither must be none, diffusion, pattern or noise".into()),
        };
        let preserve_exact = flag("preserve_exact", false)?;
        if dither != Dither::Diffusion
            && (preserve_exact || object.get("amount").is_some_and(|v| !v.is_null()))
        {
            return Err("mode options amount and preserve_exact apply to Diffusion only".into());
        }
        let amount = match object.get("amount").filter(|v| !v.is_null()) {
            None => 75,
            Some(v) => v
                .as_u64()
                .filter(|n| (1..=100).contains(n))
                .ok_or("mode option amount must be 1 to 100")? as u32,
        };
        Ok(Self {
            palette,
            table,
            colors,
            forced,
            transparency: flag("transparency", true)?,
            matte,
            dither,
            amount,
            preserve_exact,
            flatten: flag("flatten", false)?,
        })
    }
}

/// The source's one layer image, when the document is one plain layer
/// (module doc).
pub(crate) fn plain_image(document: &PrecisionDocument) -> Option<&PrecisionImage> {
    let [layer] = document.root_layers() else {
        return None;
    };
    let Content::Raster {
        image,
        offset: (0, 0),
    } = &layer.content
    else {
        return None;
    };
    let plain = image.dimensions() == document.dimensions()
        && layer.opacity == 1.0
        && layer.visible
        && layer.blend_mode == BlendMode::Normal
        && layer.mask.is_none()
        && layer.style.is_none()
        && !layer.clipped
        && layer.blend_if.is_none()
        && layer.description.is_none()
        && layer.retained_source.is_none()
        && layer.warp.is_none()
        && layer.generated.is_none();
    plain.then_some(image)
}

/// One row of `image`, prepared (`indexed_palette::prepare`).
fn prepared_row(
    image: &PrecisionImage,
    y: u32,
    options: &IndexedOptions,
    out: &mut Vec<Option<Rgb>>,
) -> Result<(), String> {
    out.clear();
    for (_, run) in image.row_segments(y, 0, image.dimensions().0) {
        let PrecisionRun::Rgba16(run) = run else {
            return Err("Indexed Color converts an 8-bit document".into());
        };
        out.extend(run.iter().map(|p| {
            let rgba = p.map(crate::indexed_palette::code8);
            crate::indexed_palette::prepare(rgba, options.transparency, options.matte)
        }));
    }
    Ok(())
}

/// Whether `image` holds a pixel that `options` make transparent; only
/// Transparency makes one ([`crate::indexed_palette::prepare`]).
fn has_transparent_pixel(image: &PrecisionImage, options: &IndexedOptions) -> Result<bool, String> {
    if !options.transparency {
        return Ok(false);
    }
    let mut row = Vec::new();
    for y in 0..image.dimensions().1 {
        prepared_row(image, y, options, &mut row)?;
        if row.iter().any(Option::is_none) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The palette and transparent index of `image` under `options` (module
/// doc): one pass for a transparent pixel and, for Exact and Adaptive, the
/// colours.
fn palette_of(
    image: &PrecisionImage,
    options: &IndexedOptions,
    job: JobContext<'_>,
) -> Result<(Vec<Rgb>, Option<u8>), String> {
    let needs_scan =
        options.transparency || matches!(options.palette, Palette::Exact | Palette::Adaptive);
    let mut exact = if options.palette == Palette::Exact {
        Some(ExactColors::new(options.colors)?)
    } else {
        None
    };
    let mut histogram = if options.palette == Palette::Adaptive {
        Some(ColorHistogram::new()?)
    } else {
        None
    };
    let mut transparent_pixel = false;
    if needs_scan {
        let mut row = Vec::new();
        let (w, h) = image.dimensions();
        for y in 0..h {
            prepared_row(image, y, options, &mut row)?;
            for pixel in &row {
                match pixel {
                    None => transparent_pixel = true,
                    Some(rgb) => {
                        if let Some(exact) = exact.as_mut() {
                            exact.push(*rgb);
                        }
                        if let Some(histogram) = histogram.as_mut() {
                            histogram.push(*rgb);
                        }
                    }
                }
            }
            job.every(y as usize * w as usize)?;
        }
    }
    let t = usize::from(transparent_pixel);
    let mut palette = match options.palette {
        Palette::Exact => exact.map_or_else(|| Ok(Vec::new()), |e| e.finish(options.colors - t))?,
        Palette::Web => crate::indexed_palette::web(),
        Palette::Uniform => {
            crate::indexed_palette::uniform(options.colors, &options.forced, transparent_pixel)?
        }
        Palette::Adaptive => crate::indexed_palette::adaptive(
            histogram
                .as_ref()
                .ok_or("the Adaptive palette's histogram is missing")?,
            options.colors,
            &options.forced,
            transparent_pixel,
        )?,
        Palette::SystemMac => crate::indexed_palette::system_mac(),
        Palette::SystemWindows => crate::indexed_palette::system_windows(),
        Palette::Custom => crate::indexed_act::custom(
            options.table.as_deref().unwrap_or_default(),
            transparent_pixel,
        )?,
    };
    if !transparent_pixel {
        return Ok((palette, None));
    }
    if options.palette.is_system() {
        return Ok((palette, Some(255)));
    }
    palette.push(options.matte.unwrap_or([255, 255, 255]));
    let index = (palette.len() - 1) as u8;
    Ok((palette, Some(index)))
}

impl PrecisionDocument {
    /// Why this document cannot become Indexed even with a flatten, or `None`.
    fn to_indexed_hard_refusal(&self) -> Option<String> {
        if self.is_indexed() {
            return Some("the document is already Indexed Color".into());
        }
        if self.is_cmyk() || self.is_lab() {
            return Some(
                "Indexed Color converts an 8-bit RGB or Grayscale document; \
                 convert to RGB Color first."
                    .into(),
            );
        }
        let gray8 = self
            .gray_policy()
            .is_none_or(|p| p.depth() == crate::gray_document::GrayDepth::Eight);
        if self.is_float() || !gray8 {
            return Some(
                "Indexed Color converts an 8-bit RGB or Grayscale document; \
                 convert to 8 Bits/Channel first."
                    .into(),
            );
        }
        if self.has_spot_channel() {
            return Some(crate::spot_channel::TO_INDEXED_REFUSAL.into());
        }
        None
    }

    /// The conversion's working memory (module doc): the larger of Flatten
    /// Image's render and [`TO_INDEXED_BYTES_PER_PIXEL`] a canvas pixel, the
    /// exact memo, the Exact bit set, the diffusion rows and bookkeeping;
    /// Adaptive adds its histogram and 8 B a distinct colour, at most one a
    /// canvas pixel and 2²⁴.
    fn to_indexed_scratch(&self, flatten: bool, palette: Palette) -> u64 {
        let canvas = u64::from(self.width) * u64::from(self.height);
        let flattening = if flatten {
            self.render_plan().map(|p| p.scratch_bytes).unwrap_or(0)
        } else {
            0
        };
        let adaptive = if palette == Palette::Adaptive {
            ADAPTIVE_BYTES + ADAPTIVE_COLOR_BYTES * canvas.min(1 << 24)
        } else {
            0
        };
        flattening.max(canvas.saturating_mul(TO_INDEXED_BYTES_PER_PIXEL))
            + MEMO_BYTES
            + EXACT_BYTES
            + adaptive
            + Diffusion::scratch_bytes(self.width)
            + BOOKKEEPING_BYTES
    }

    /// `dr_precision_doc_convert_to_indexed_plan`'s answer (Gray's keys). A
    /// palette refusal (Exact past the budget, Uniform with no qualifying
    /// cube) is the plan's `refusal` and the conversion's `Err`, the same
    /// sentence.
    pub fn convert_to_indexed_plan(&self, options: &str) -> Result<Value, String> {
        let options = IndexedOptions::parse(options)?;
        let hard = self.to_indexed_hard_refusal();
        let image = plain_image(self);
        let blocked = image.is_none();
        // Only Exact, Uniform and Adaptive can refuse, so only they read the
        // pixels. Uniform and Adaptive read them only when a transparent
        // pixel would change the answer: a palette that fits with the
        // transparent entry fits without it.
        let refusable = match options.palette {
            Palette::Exact => true,
            Palette::Uniform => {
                crate::indexed_palette::uniform(options.colors, &options.forced, true).is_err()
            }
            Palette::Adaptive => {
                crate::indexed_palette::adaptive_budget(options.colors, &options.forced, true)
                    .is_err()
            }
            // Custom refuses only a full table beside a transparent pixel.
            Palette::Custom => options
                .table
                .as_ref()
                .is_some_and(|t| t.len() == MAX_COLORS),
            _ => false,
        };
        // Adaptive's refusal depends only on whether a transparent pixel
        // exists, so its plan scans for one and never builds the histogram
        // (the conversion's admitted scratch).
        let palette = match (&hard, image) {
            (None, Some(image)) if refusable && options.palette == Palette::Adaptive => {
                match has_transparent_pixel(image, &options) {
                    Ok(t) => {
                        crate::indexed_palette::adaptive_budget(options.colors, &options.forced, t)
                            .err()
                    }
                    Err(e) => Some(e),
                }
            }
            (None, Some(image)) if refusable => palette_of(image, &options, JobContext::NONE).err(),
            _ => None,
        };
        let refusal = hard
            .or_else(|| (blocked && !options.flatten).then(|| FLATTEN_SENTENCE.to_string()))
            .or(palette);
        let flattens = blocked && options.flatten;
        Ok(json!({
            "working_memory_plan_bytes": self.to_indexed_scratch(flattens, options.palette),
            "refusal": refusal,
            "flatten_required": blocked,
            "flattens": flattens,
        }))
    }

    /// RGB / Gray → Indexed (module doc).
    pub fn convert_to_indexed(
        &self,
        options: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Self, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let options = IndexedOptions::parse(options)?;
        if let Some(reason) = self.to_indexed_hard_refusal() {
            return Err(reason);
        }
        let blocked = plain_image(self).is_none();
        if blocked && !options.flatten {
            return Err(FLATTEN_SENTENCE.into());
        }
        let scratch = self.to_indexed_scratch(blocked, options.palette);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let flattened;
        let source = if blocked {
            flattened = self
                .flatten_image(limit, other, job.range(0.0, 0.3))?
                .ok_or("flattening the image changed nothing")?;
            &flattened
        } else {
            self
        };
        let image = plain_image(source).ok_or(FLATTEN_SENTENCE)?;
        let (width, height) = source.dimensions();
        let (palette, transparent) = palette_of(image, &options, job.range(0.3, 0.5))?;
        let mut map = NearestMap::new(palette.clone(), transparent.map(usize::from))?;
        let mut diffusion = if options.dither == Dither::Diffusion {
            Some(Diffusion::new(
                width,
                options.amount,
                options.preserve_exact,
            )?)
        } else {
            None
        };
        // With Exact every pixel already has its own entry, so Pattern and
        // Noise run the None arm (as Diffusion's zero error leaves it).
        let ordered = match options.dither {
            Dither::Pattern if options.palette != Palette::Exact => Some(Ordered::Pattern),
            Dither::Noise if options.palette != Palette::Exact => Some(Ordered::Noise),
            _ => None,
        };
        let pixels = crate::precision_image::pixel_count(width, height)?;
        let mut indices = crate::precision_image::reserved_vec(pixels)?;
        indices.resize(pixels, 0u8);
        let mut row = Vec::new();
        let stage = job.range(0.5, 0.9);
        let fill = transparent.unwrap_or(0);
        for (y, out) in indices.chunks_exact_mut(width as usize).enumerate() {
            prepared_row(image, y as u32, &options, &mut row)?;
            match (diffusion.as_mut(), ordered) {
                (Some(diffusion), _) => diffusion.row(&mut map, &row, fill, out),
                (None, Some(kind)) => {
                    crate::indexed_dither::ordered_row(&mut map, kind, y as u32, &row, fill, out)
                }
                (None, None) => crate::indexed_palette::plain_row(&mut map, &row, fill, out),
            }
            // Once a row (as `indexed_paint`'s re-index): `every` on a row
            // start would poll rarely on an odd width.
            stage.report((y + 1) as f64 / f64::from(height), 5)?;
        }
        drop(map);
        let policy = IndexedPolicy::new(palette, transparent, indices, width, height)?;
        let profile = source.profile().clone();
        let rendered = policy.render(&profile, job.range(0.9, 1.0))?;
        let layer = PrecisionDocumentLayer::raster(LAYER_NAME, rendered, (0, 0), 1.0, true)?;
        let mut out = PrecisionDocument::build(
            width,
            height,
            profile,
            source.metadata.clone(),
            source.resolution,
            vec![layer],
            source.channels().to_vec(),
            None,
        )?;
        out.guides.clone_from(&source.guides);
        out.ruler_origin = source.ruler_origin;
        out.color_samplers.clone_from(&source.color_samplers);
        out.global_light = source.global_light;
        out.paths.set_entries(source.paths.entries.to_vec());
        let out = out.with_indexed(Arc::new(policy))?;
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// Indexed → RGB (module doc): the RGB document the storage is.
    pub fn convert_indexed_to_rgb(&self) -> Result<Self, String> {
        if !self.is_indexed() {
            return Err("the document is not Indexed Color".into());
        }
        Ok(self.without_indexed())
    }
}
