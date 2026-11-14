//! Type, shape and fill layers on a CMYK document (#1): the adapter over
//! `cmyk_artwork_render` that publishes a CMYK raster WITH its description,
//! and `artwork_colors`, the host's one colour leg for a description's
//! numbers across the CMYK boundary.
//!
//! # Publication
//!
//! [`PrecisionDocument::cmyk_artwork`] assembles the ink components into a
//! straight CMYKA16 raster (`assemble_cmyk`) and publishes it exactly as the
//! typed RGB path publishes its image (`artwork_document`'s
//! `artwork_unlocked`): REPLACING rewrites record `index`'s pixels and
//! description under its `Pixels` lock, keeping its mask and every panel
//! property, and answers `None` when the samples, the offset and the
//! description are already those; a NEW record is a `raster_cmyk` leaf at
//! the slot `position` names relative to the anchor (`layer_insert`), or the
//! top root when there is none. The description is the host's opaque bytes
//! (`artwork_document::validate`), stored in the record's typed description
//! slot and persisted as DRDC 87 (`cmyk_native`).
//!
//! An intrinsic colour glyph's RGBA8 pixels are sRGB platform image data,
//! not stored numbers: each distinct sRGB converts once to ink through the
//! document's profile at relative colorimetric with black-point compensation
//! (`cmyk_color::RgbToCmyk`, the Color Picker's and the mode conversion's
//! leg), memoized by `ink_key`.
//!
//! A pattern fill layer's tile (the `pattern` component) is
//! separated at render by the Pattern Overlay's own leg
//! (`cmyk_style::TexelInk`: sRGB → ink, then the document's grid), so the
//! description stays the RGB one with no ink; that leg is built only when a
//! pattern component is listed.
//!
//! # Colours across the boundary
//!
//! [`artwork_colors`] converts a list of colours between the built-in sRGB
//! (`[r, g, b]`) and a CMYK profile's ink (`[c, m, y, k]`), or between two
//! CMYK profiles, every value 0…1, through `cmyk_color`'s legs and no other
//! arithmetic: sRGB → ink is `srgb_to_ink`'s leg, ink → sRGB `ink_to_srgb`'s,
//! ink → ink `CmykToCmyk` (identical profile bytes answer the values as they
//! are). The intent and black-point compensation are `dr_cmyk_color`'s keys
//! with its defaults (relative colorimetric, BPC on).
//!
//! # Admission
//!
//! The publication charges `8·BOOKKEEPING + n·10 + width·20 + profile·64 +
//! description·2 + pattern_scratch` beside the document's retained bytes: the
//! assembled samples (10 B a pixel), a row, the sRGB → ink leg's parsed state
//! (64 × the profile's bytes, the ICC parsers' rule), the description and
//! each pattern component's ink level, exactly (`cmyk_artwork_render::
//! pattern_scratch`). The host's
//! plan for the whole render (`NativeArtworkEvaluation.scratchBytes` plus
//! `cmykMarginPerPixel` a pixel and the profile × 64) is the measured
//! figure; its record is below.
//!
//! # Measurement record (#1, 2026-10-15)
//!
//! `scripts/cmyk-admission/measure.py` (Release, `MallocLargeCache=0`, one
//! case per process, three runs, each beside a `resource_usage` baseline
//! twin) on the 6000 × 4000 16-bit CMYK TIFF fixture (Generic CMYK, 55,280
//! profile bytes): `artwork-fill-16` (`add_fill_layer` solid, n = 24,000,000),
//! `artwork-shape-16` (`add_shape_layer` rect over the canvas with a fill and
//! an 8 px stroke: a 6012 × 4012 raster, n = 24,120,144, two coverage planes)
//! and `artwork-edit-16` (`edit_fill_layer` recolouring the solid fill). The
//! plan is the host's admitted figure each reply echoes as
//! `working_memory_plan_bytes`: `scratchBytes` + K·n + 64 × the profile, K =
//! `NativeArtworkEvaluation.cmykMarginPerPixel`. E does not depend on K (K
//! only sizes the reservation), so the first build's (K = 40) runs hold for
//! the shipped K = 10, the assembled CMYKA16 samples alone; the K = 40 plans
//! were 1,617,511,040 / 1,721,440,832 / 1,617,511,040 (3.8–5.3 × the need),
//! an over-charge.
//!
//! | case | plan (K = 10) | E (three runs) | need ⌈1.25 × max E⌉ | plan / need |
//! | --- | --- | --- | --- | --- |
//! | artwork-fill-16 | 897,511,040 | 336,544,224 / 336,462,328 / 336,593,400 | 420,741,750 | 2.133 |
//! | artwork-shape-16 | 997,836,512 | 367,182,112 / 367,149,320 / 367,182,088 | 458,977,640 | 2.174 |
//! | artwork-edit-16 | 897,511,040 | 243,581,096 / 243,581,096 / 243,548,328 | 304,476,370 | 2.948 |
//!
//! Baseline twins read 0–49,152 bytes. The K = 10 build was measured again
//! (`admission-2` in the batch's handoff) to confirm the echoed plans.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::artwork_document::{validate, ArtworkPublication};
use crate::artwork_window::totals;
use crate::cmyk_artwork_render::{
    assemble_cmyk, cmyk_components, cmyk_windows, pattern_scratch, CmykComponent,
};
use crate::cmyk_color::{ink_key, CmykToCmyk, CmykToRgb, ConversionOptions, RgbToCmyk};
use crate::cmyk_image::CmykImage;
use crate::cmyk_style::TexelInk;
use crate::icc::IccProfile;
use crate::icc_lut::{DeviceSpace, ProofProfile};
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};

/// The most colours one [`artwork_colors`] request converts.
pub(crate) const MAX_ARTWORK_COLORS: usize = 1024;

/// One CMYK artwork raster to assemble (module doc): its size, the ink
/// component list and the coverage and intrinsic planes it names.
pub(crate) struct CmykArtwork<'a> {
    pub width: u32,
    pub height: u32,
    pub components: &'a str,
    pub samples: &'a [u16],
    pub intrinsic: &'a [u8],
}

/// The coverage samples and intrinsic RGBA8 bytes `components` names over a
/// `width × height` raster, by their windows (`artwork_window`): what the FFI
/// recomputes its slice lengths from. Refuses a window the raster cannot hold.
pub(crate) fn component_lengths(
    components: &[CmykComponent],
    width: u32,
    height: u32,
) -> Result<(usize, usize), String> {
    Ok(totals(
        cmyk_windows(components, width, height)?
            .into_iter()
            .flatten(),
    ))
}

impl PrecisionDocument {
    /// Publishes `art` as a CMYK raster with `request`'s description (module
    /// doc). `Ok(None)` when a replacement would change nothing. Refuses an
    /// RGB document, a bad description or component list, a lock and a
    /// replacement target that is not a CMYK raster, by name.
    pub(crate) fn cmyk_artwork(
        &self,
        request: &ArtworkPublication<'_>,
        art: &CmykArtwork<'_>,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        job.check()?;
        let policy = self.require_cmyk_policy("CMYK artwork")?.clone();
        let n = pixel_count(art.width, art.height)? as u64;
        let components = cmyk_components(art.components)?;
        let scratch = 8 * BOOKKEEPING_BYTES
            + n * 10
            + u64::from(art.width) * 20
            + policy.profile_bytes().len() as u64 * 64
            + request.description.len() as u64 * 2
            + pattern_scratch(&components);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        validate(request.description)?;
        if request.replacing {
            self.lock_refusal(request.index, crate::doc_lock::EditKind::Pixels)?;
        }
        // The texel leg is built only when a pattern asks for it (module
        // doc), so a profile with no B2A table refuses only then.
        let mut texel = if components
            .iter()
            .any(|c| matches!(c, CmykComponent::Pattern { .. }))
        {
            Some(TexelInk::new(&policy)?)
        } else {
            None
        };
        let mut texel_ink =
            |rgb: [u8; 3]| -> [f32; 4] { texel.as_mut().map_or([0.0; 4], |t| t.ink(rgb)) };
        // The intrinsic leg is built only when a glyph asks for it, so a
        // profile with no B2A table refuses only artwork that needs one.
        let mut leg: Option<RgbToCmyk<'_>> = None;
        let mut refusal: Option<String> = None;
        // `RgbToCmyk::ink`'s own memo (bounded at `MEMO_CAPACITY`) holds
        // each distinct glyph colour once.
        let mut intrinsic_ink = |rgb: [u8; 3]| -> [f32; 4] {
            let key = ink_key([u16::from(rgb[0]), u16::from(rgb[1]), u16::from(rgb[2]), 0]);
            if leg.is_none() {
                match RgbToCmyk::new(
                    IccProfile::srgb(),
                    &policy.profile,
                    ConversionOptions::default(),
                ) {
                    Ok(built) => leg = Some(built),
                    Err(e) => {
                        refusal.get_or_insert(e);
                        return [0.0; 4];
                    }
                }
            }
            leg.as_mut().map_or([0.0; 4], |leg| {
                leg.ink(key, rgb.map(|v| f64::from(v) / 255.0))
                    .map(|v| v as f32)
            })
        };
        let samples = assemble_cmyk(
            art.width,
            art.height,
            &components,
            art.samples,
            art.intrinsic,
            &mut intrinsic_ink,
            &mut texel_ink,
            job,
        )?;
        if let Some(reason) = refusal {
            return Err(reason);
        }
        let row = art.width as usize;
        let image = CmykImage::from_rows_with_job(
            art.width,
            art.height,
            |y, out| {
                let at = y as usize * row;
                out.copy_from_slice(&samples[at..at + row]);
                Ok(())
            },
            job,
        )?;
        drop(samples);
        let next = if request.replacing {
            self.under_locks(request.index, crate::doc_lock::EditKind::Pixels, |doc| {
                doc.cmyk_artwork_unlocked(request, &image)
            })?
        } else {
            self.cmyk_artwork_unlocked(request, &image)?
        };
        job.check()?;
        Ok(next)
    }

    /// The publication itself (module doc), `artwork_unlocked`'s CMYK twin.
    fn cmyk_artwork_unlocked(
        &self,
        request: &ArtworkPublication<'_>,
        image: &CmykImage,
    ) -> Result<Option<Self>, String> {
        let ArtworkPublication {
            index,
            replacing,
            offset,
            name,
            description,
            position,
        } = *request;
        let described = Some(Arc::new(description.to_string()));
        if replacing {
            let old = self
                .layer(index)
                .ok_or("artwork layer index is out of range")?;
            let old_image = old
                .cmyk_source()
                .ok_or("CMYK artwork replacement requires a CMYK raster layer")?;
            if old_image.dimensions() == image.dimensions()
                && old_image.same_samples(image)
                && old.offset() == offset
                && self.artwork_description(index) == Some(description)
            {
                return Ok(None);
            }
            let mut layer = old.clone();
            layer.content = Content::CmykRaster {
                image: image.clone(),
                offset,
            };
            layer.description = described;
            return self.replacing_layer(index, layer);
        }
        self.preflight_raster_addition(image.dimensions(), name)?;
        let mut layer =
            PrecisionDocumentLayer::raster_cmyk(name, image.clone(), offset, 1.0, true)?;
        layer.description = described;
        if let Some(position) = position {
            let slot = self.insert_slot(index, position)?;
            return Ok(Some(self.inserting_record(layer, slot)?));
        }
        let mut layers = self.layers.as_ref().clone();
        layers.push(layer);
        Ok(Some(self.rebuilt(
            self.width,
            self.height,
            layers,
            self.channels.as_ref().clone(),
        )?))
    }
}

/// One end of an [`artwork_colors`] conversion: the built-in sRGB or a CMYK
/// profile's ink.
enum End {
    Srgb,
    Cmyk(ProofProfile, Vec<u8>),
}

impl End {
    fn of(bytes: Option<&[u8]>) -> Result<Self, String> {
        let Some(bytes) = bytes.filter(|b| !b.is_empty()) else {
            return Ok(End::Srgb);
        };
        if bytes.len() > crate::drdc::MAX_DRDC_BLOB_LEN as usize {
            return Err("the CMYK profile exceeds the native byte limit".into());
        }
        let not_cmyk = |name: &str| {
            format!("artwork colours convert to or from CMYK ink: “{name}” is not a CMYK profile")
        };
        let profile = match ProofProfile::parse(bytes) {
            Ok(profile) => profile,
            // An RGB profile this parser does not model is still named.
            Err(e) => {
                return Err(match IccProfile::parse(bytes) {
                    Some(rgb) => not_cmyk(rgb.name()),
                    None => format!("cannot use the CMYK profile: {e}"),
                })
            }
        };
        if profile.device() != DeviceSpace::Cmyk {
            return Err(not_cmyk(profile.name()));
        }
        Ok(End::Cmyk(profile, bytes.to_vec()))
    }

    fn width(&self) -> usize {
        match self {
            End::Srgb => 3,
            End::Cmyk(..) => 4,
        }
    }
}

/// The request's intent and black-point compensation: `dr_cmyk_color`'s keys
/// and defaults.
fn options(object: &serde_json::Map<String, Value>) -> Result<ConversionOptions, String> {
    let mut options = ConversionOptions::default();
    if let Some(intent) = object.get("intent").filter(|v| !v.is_null()) {
        let text = intent.as_str().ok_or("intent must be a string")?;
        options.intent = crate::icc_intent::RenderingIntent::parse(text).ok_or_else(|| {
            format!("intent \"{text}\" is not one of perceptual, relative, saturation or absolute")
        })?;
    }
    match object.get("black_point_compensation") {
        None | Some(Value::Null) => {}
        Some(Value::Bool(v)) => options.black_point_compensation = *v,
        Some(_) => return Err("black_point_compensation must be a boolean".into()),
    }
    Ok(options)
}

/// `dr_cmyk_artwork_colors`' answer (module doc): `request` is
/// `{"values":[[…],…],"intent","black_point_compensation"}`, a NULL or empty
/// profile end is sRGB `[r, g, b]` and a CMYK profile end `[c, m, y, k]`,
/// every value 0…1 (clamped). Answers `{"values":[[…],…]}`.
pub(crate) fn artwork_colors(
    source: Option<&[u8]>,
    destination: Option<&[u8]>,
    request: &str,
) -> Result<String, String> {
    let value: Value = serde_json::from_str(request)
        .map_err(|e| format!("artwork colour request is not valid JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or("artwork colour request must be a JSON object")?;
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "values" | "intent" | "black_point_compensation"
        ) {
            return Err(format!(
                "artwork colour request has an unknown key \"{key}\""
            ));
        }
    }
    let options = options(object)?;
    let from = End::of(source)?;
    let to = End::of(destination)?;
    let listed = object
        .get("values")
        .and_then(Value::as_array)
        .ok_or("artwork colour request needs \"values\"")?;
    if listed.len() > MAX_ARTWORK_COLORS {
        return Err(format!(
            "artwork colour request converts at most {MAX_ARTWORK_COLORS} colours"
        ));
    }
    let width = from.width();
    let values = listed
        .iter()
        .map(|entry| {
            let numbers: Option<Vec<f64>> = entry.as_array().and_then(|a| {
                a.iter()
                    .map(|v| v.as_f64().filter(|v| v.is_finite()))
                    .collect()
            });
            numbers
                .filter(|n| n.len() == width)
                .map(|n| n.iter().map(|v| v.clamp(0.0, 1.0)).collect::<Vec<f64>>())
                .ok_or_else(|| format!("each artwork colour must be {width} finite numbers"))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let srgb = IccProfile::srgb();
    let out: Vec<Vec<f64>> = match (&from, &to) {
        (End::Srgb, End::Cmyk(profile, _)) => {
            let leg = RgbToCmyk::new(srgb, profile, options)?;
            values
                .iter()
                .map(|v| leg.evaluate([v[0], v[1], v[2]]).to_vec())
                .collect()
        }
        (End::Cmyk(profile, _), End::Srgb) => {
            let leg = CmykToRgb::new(profile, &srgb, options)?;
            values
                .iter()
                .map(|v| {
                    leg.evaluate([v[0], v[1], v[2], v[3]])
                        .map(|c| f64::from(c.clamp(0.0, 1.0)))
                        .to_vec()
                })
                .collect()
        }
        (End::Cmyk(a, a_bytes), End::Cmyk(b, b_bytes)) => {
            if a_bytes == b_bytes {
                values
            } else {
                let leg = CmykToCmyk::new(a, b, options)?;
                values
                    .iter()
                    .map(|v| leg.evaluate([v[0], v[1], v[2], v[3]]).to_vec())
                    .collect()
            }
        }
        (End::Srgb, End::Srgb) => {
            return Err("artwork colours convert to or from CMYK ink: both ends are sRGB".into())
        }
    };
    serde_json::to_string(&json!({ "values": out })).map_err(|e| e.to_string())
}
