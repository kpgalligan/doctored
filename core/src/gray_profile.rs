//! A Grayscale document's profile and depth edits (#547 phase 5a): Edit ▸
//! Assign Profile and Convert to Profile between Gray profiles and Image ▸
//! Mode ▸ 8/16/32 Bits/Channel — their request, plan and refusals. CMYK's
//! `cmyk_profile` is the template; nothing here converts a pixel itself.
//!
//! The storage stays the RGB precision document with R = G = B in the
//! policy's twin (`gray_document`'s module doc), so every edit is the RGB
//! one on [`PrecisionDocument::without_gray`], its result taken back by
//! `with_gray` under the new policy:
//!
//! * **Assign** is the RGB Assign into the destination's twin, which keeps
//!   every sample bit for bit at 8, 16 and 32 bits; the twin's bytes (and so
//!   the Gray profile's) already held is `None`.
//! * **Convert** is the RGB `convert_to_profile_with` from the twin into the
//!   destination's twin (the request's intent and BPC, Color Settings'
//!   relative without BPC when omitted). At 8 and 16 bits every raster leaf
//!   then takes the neutral-grid pass: `[r', r', r', a']` with `r' = R`
//!   (`eight(R)` and `eight(a)` at 8 bits), because the two twins share
//!   sRGB's primaries and R = G = B in, so R is the gray the conversion
//!   gave, and the pass keeps the equality and the 8-bit grid exact. At 32
//!   bits linear `Y` is the same under every twin (both are sRGB's primaries
//!   on linear light), so the conversion runs at its default options, intent
//!   and BPC not applying, and no pass follows. A conversion that changes no
//!   sample relabels as Assign does (`convert_gray_to_rgb`'s precedent).
//! * **Depth** 16 → 8 puts every raster sample (gray and alpha), every mask
//!   sample and every alpha-channel sample on the 8-bit grid
//!   (`cmyk_profile::eight`, CMYK's own order: `map_cmyk_tree`,
//!   `channels_on_grid`, `rebuilt`, `seeding_vector_masks`); 8 → 16 changes
//!   only the declared depth (the codes already are 16-bit); 8/16 → 32 is
//!   `promote_to_float` of the twin, refusing with its plan's first blocked
//!   row; 32 → 8/16 refuses ([`FLOAT_DEPTH_REFUSAL`]): it goes through HDR
//!   Toning, as Doctored's RGB 32 Bits/Channel does. The same
//!   depth is `None`.
//!
//! The profile is the caller's `icc`, else the request's `space` (a Gray
//! working space). An RGB or CMYK profile refuses by name
//! ([`non_gray_refusal`]), then the policy's own sentences
//! (`GrayPolicy::new`). None of this is a format change: the `GRAY` table
//! already holds the profile and the depth.
//!
//! A Duotone document (#547, `designs/duotone-mode.md`) refuses Assign
//! Profile, Convert to Profile, 8 → 16 and 32 Bits/Channel by name
//! (`duotone_document`'s `ASSIGN_REFUSAL`, `CONVERT_REFUSAL`,
//! `DEPTH16_REFUSAL` and `DEPTH32_REFUSAL`). 16 → 8 runs as above and the
//! policy keeps the inks (`with_depth`).
//!
//! # Memory
//!
//! The plan is the larger of the edit's stage and the result's display
//! preparation (`gray_convert::TO_GRAY_BYTES_PER_PIXEL` a raster pixel, twice
//! that for a 32-bit result, `new_gray`'s rule), plus the destination
//! policy's ICC, twin and encoder, and bookkeeping. The stage is Assign's
//! colour plan, Convert's conversion plan, 16 → 8's rounded planes (8 B a
//! raster pixel, 2 B a mask or channel sample), nothing for 8 → 16, and the
//! promotion's scratch and output for → 32. Held by
//! `scripts/gray-admission/measure.py` (`assign-gray-16`, `convert-gray-16`,
//! `depth-gray-8`, `depth-gray-32`); a miss raises `TO_GRAY_BYTES_PER_PIXEL`.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::cmyk_profile::{channels_on_grid, eight, map_cmyk_tree};
use crate::doc_color_policy::ConversionOptions;
use crate::gray_convert::TO_GRAY_BYTES_PER_PIXEL;
use crate::gray_document::{GrayDepth, GrayPolicy};
use crate::gray_working::GrayWorkingSpace;
use crate::icc_lut::{DeviceSpace, ProofProfile};
use crate::job_control::{DrJobControl, JobContext};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, PrecisionImage, PrecisionRun, BOOKKEEPING_BYTES};
use crate::tile_store::TilePlane;

/// The depth op's refusal from 32 bits (module doc).
pub(crate) const FLOAT_DEPTH_REFUSAL: &str = "A 32-bit Grayscale document converts to 8 or 16 Bits/Channel through HDR Toning (hdr_toning with bit_depth 8 or 16).";

/// `Some` for a parsable RGB or CMYK profile: a Gray document takes Gray
/// profiles only.
/// A matrix/TRC RGB profile is `IccProfile`'s; a LUT profile (RGB or CMYK)
/// is `ProofProfile`'s.
pub(crate) fn non_gray_refusal(icc: &[u8]) -> Option<String> {
    let (name, kind) = match crate::icc::IccProfile::parse(icc) {
        Some(profile) => (profile.name().to_string(), "an RGB"),
        None => {
            let profile = ProofProfile::parse(icc).ok()?;
            let kind = match profile.device() {
                DeviceSpace::Rgb => "an RGB",
                DeviceSpace::Cmyk => "a CMYK",
                DeviceSpace::Gray => return None,
            };
            (profile.name().to_string(), kind)
        }
    };
    Some(format!(
        "“{name}” is {kind} profile: a Grayscale document is assigned and converted to Gray profiles only."
    ))
}

/// The larger of a 16 → 8 edit's stage (`pixels` raster pixels at 8 B and
/// `planes` mask and channel samples at 2 B) and the result's display
/// preparation (module doc, "Memory").
fn eight_terms(pixels: u64, planes: u64) -> Option<u64> {
    let stage = pixels.checked_mul(8)?.checked_add(planes.checked_mul(2)?)?;
    Some(stage.max(pixels.checked_mul(TO_GRAY_BYTES_PER_PIXEL)?))
}

/// The 16 → 8 plan's stage and display terms of a one-raster Gray document
/// of `width × height` with no masks or channels: the depth op's raster term
/// and the 8-bit arm of HDR Toning a 32-bit Gray document
/// (`doc_hdr_toning`), whose host narrows the toned result by this edit.
pub(crate) fn depth_eight_bytes(width: u32, height: u32) -> Option<u64> {
    eight_terms(u64::from(width).checked_mul(u64::from(height))?, 0)
}

/// The destination policy's ICC, twin and encoder bytes (module doc).
pub(crate) fn policy_bytes(policy: &GrayPolicy) -> u64 {
    policy.twin().bytes().len() as u64
        + policy.bytes().len() as u64
        + crate::icc_transform::PrecisionToneEncoder::scratch_bytes()
}

/// What the edit does (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GrayOp {
    Assign,
    Convert,
    Depth(GrayDepth),
}

impl GrayOp {
    /// The menu item the op mirrors, for refusals that name it.
    fn menu_name(self) -> &'static str {
        match self {
            GrayOp::Assign => "Assign Profile",
            GrayOp::Convert => "Convert to Profile",
            GrayOp::Depth(GrayDepth::Eight) => "8 Bits/Channel",
            GrayOp::Depth(GrayDepth::Sixteen) => "16 Bits/Channel",
            GrayOp::Depth(GrayDepth::ThirtyTwo) => "32 Bits/Channel",
        }
    }
}

/// One parsed request.
struct GrayRequest {
    op: GrayOp,
    space: Option<GrayWorkingSpace>,
    conversion: ConversionOptions,
}

impl GrayRequest {
    /// `{"op":"assign"|"convert"|"depth","space"?,"intent"?,
    /// "black_point_compensation"?,"depth"?:8|16|32}`: `op` required, `depth`
    /// only (and always) with op depth, `space` only with assign or convert,
    /// intent and BPC only with convert; an unknown key refuses.
    fn parse(text: &str) -> Result<Self, String> {
        let value: Value = if text.trim().is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_str(text)
                .map_err(|e| format!("the Gray profile request is not valid JSON: {e}"))?
        };
        let object = value
            .as_object()
            .ok_or("the Gray profile request must be a JSON object")?;
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "op" | "space" | "intent" | "black_point_compensation" | "depth"
            ) {
                return Err(format!(
                    "the Gray profile request has an unknown key \"{key}\""
                ));
            }
        }
        let present = |key: &str| object.get(key).is_some_and(|v| !v.is_null());
        let op = match object.get("op").and_then(Value::as_str) {
            Some("assign") => GrayOp::Assign,
            Some("convert") => GrayOp::Convert,
            Some("depth") => GrayOp::Depth(
                object
                    .get("depth")
                    .filter(|v| !v.is_null())
                    .ok_or("a depth change needs \"depth\": 8, 16 or 32")?
                    .as_u64()
                    .and_then(|bits| u32::try_from(bits).ok())
                    .and_then(GrayDepth::from_bits)
                    .ok_or("the Gray depth must be 8, 16 or 32")?,
            ),
            Some(other) => {
                return Err(format!(
                    "the Gray profile op \"{other}\" is not one of assign, convert or depth"
                ))
            }
            None => return Err("the Gray profile request needs an \"op\"".into()),
        };
        if present("depth") && !matches!(op, GrayOp::Depth(_)) {
            return Err("\"depth\" belongs to the depth op only".into());
        }
        if present("space") && matches!(op, GrayOp::Depth(_)) {
            return Err("\"space\" belongs to the assign and convert ops only".into());
        }
        for key in ["intent", "black_point_compensation"] {
            if present(key) && op != GrayOp::Convert {
                return Err(format!("\"{key}\" belongs to the convert op only"));
            }
        }
        let space = match object.get("space").filter(|v| !v.is_null()) {
            Some(value) => {
                let name = value.as_str().ok_or("space must be a string")?;
                Some(
                    GrayWorkingSpace::from_name(name)
                        .ok_or_else(|| format!("\"{name}\" is not a Gray working space"))?,
                )
            }
            None => None,
        };
        let mut options = Map::new();
        for key in ["intent", "black_point_compensation"] {
            if let Some(v) = object.get(key) {
                options.insert(key.to_string(), v.clone());
            }
        }
        Ok(Self {
            op,
            space,
            conversion: ConversionOptions::from_value(&Value::Object(options))?,
        })
    }
}

impl PrecisionDocument {
    /// The Assign or Convert destination at the current depth: `icc` (an
    /// RGB or CMYK one refused by name first), else the request's space.
    fn gray_destination_policy(
        icc: Option<&[u8]>,
        request: &GrayRequest,
        current: &GrayPolicy,
    ) -> Result<GrayPolicy, String> {
        match (icc, request.space) {
            (Some(bytes), _) => {
                if let Some(reason) = non_gray_refusal(bytes) {
                    return Err(reason);
                }
                GrayPolicy::new(bytes, current.depth())
            }
            (None, Some(space)) => GrayPolicy::for_space(space, current.depth()),
            (None, None) => Err(format!(
                "{} needs a Gray profile or space",
                request.op.menu_name()
            )),
        }
    }

    /// The options a Convert runs at: the request's at 8 and 16 bits, the
    /// defaults at 32 (module doc).
    fn gray_conversion(&self, request: &GrayRequest) -> ConversionOptions {
        if self.is_float() {
            ConversionOptions::DEFAULT
        } else {
            request.conversion
        }
    }

    /// The plan of `request` (module doc, "Memory"): its bytes, its
    /// refusal and whether it would change the document.
    fn gray_profile_scratch(
        &self,
        icc: Option<&[u8]>,
        request: &GrayRequest,
    ) -> (u64, Option<String>, bool) {
        let Some(current) = self.gray_policy() else {
            return (
                BOOKKEEPING_BYTES,
                Some("the document is not Grayscale".into()),
                false,
            );
        };
        // #547 Duotone: Assign, Convert, 8 → 16 and 32 Bits/Channel are
        // unavailable for good (`duotone_document`); 16 → 8 runs below and
        // keeps the description (module doc).
        if current.duotone().is_some() {
            let refusal = match request.op {
                GrayOp::Assign => Some(crate::duotone_document::ASSIGN_REFUSAL),
                GrayOp::Convert => Some(crate::duotone_document::CONVERT_REFUSAL),
                GrayOp::Depth(GrayDepth::ThirtyTwo) => {
                    Some(crate::duotone_document::DEPTH32_REFUSAL)
                }
                GrayOp::Depth(GrayDepth::Sixteen) if current.depth() == GrayDepth::Eight => {
                    Some(crate::duotone_document::DEPTH16_REFUSAL)
                }
                GrayOp::Depth(_) => None,
            };
            if let Some(reason) = refusal {
                return (BOOKKEEPING_BYTES, Some(reason.into()), false);
            }
        }
        let pixels = Self::gray_raster_pixels(self.root_layers());
        let rgb = self.without_gray();
        let mut refusal = None;
        let mut fail = |reason: String| {
            refusal.get_or_insert(reason);
            0
        };
        let (stage, destination, changes, float_result) = match request.op {
            GrayOp::Assign | GrayOp::Convert => {
                match Self::gray_destination_policy(icc, request, current) {
                    Err(reason) => (fail(reason), None, false, self.is_float()),
                    Ok(policy) => {
                        let plan = if request.op == GrayOp::Assign {
                            match rgb.assign_refusal_for(policy.twin()) {
                                Some(reason) => Err(reason),
                                None => rgb.color_plan(policy.twin(), false),
                            }
                        } else {
                            rgb.conversion_plan(policy.twin(), &self.gray_conversion(request))
                        };
                        let stage = plan.map_or_else(fail, |p| p.scratch_bytes);
                        let changes = policy.bytes() != current.bytes();
                        (stage, Some(policy), changes, self.is_float())
                    }
                }
            }
            GrayOp::Depth(depth) => {
                let from = current.depth();
                let stage = match (from, depth) {
                    _ if from == depth => 0,
                    (GrayDepth::ThirtyTwo, _) => fail(FLOAT_DEPTH_REFUSAL.into()),
                    (_, GrayDepth::ThirtyTwo) => match rgb.promote_to_float_plan() {
                        Ok(plan) => match plan.refusal() {
                            Some(reason) => fail(reason),
                            None => plan.scratch_bytes.saturating_add(plan.output_bytes),
                        },
                        Err(reason) => fail(reason),
                    },
                    (GrayDepth::Sixteen, GrayDepth::Eight) => {
                        let (w, h) = self.dimensions();
                        let planes = self.cmyk_mask_samples().saturating_add(
                            (self.channels().len() as u64)
                                .saturating_mul(u64::from(w) * u64::from(h)),
                        );
                        eight_terms(pixels, planes).unwrap_or(u64::MAX)
                    }
                    _ => 0,
                };
                (
                    stage,
                    Some(current.with_depth(depth)),
                    from != depth,
                    depth == GrayDepth::ThirtyTwo,
                )
            }
        };
        let per_pixel = if float_result {
            TO_GRAY_BYTES_PER_PIXEL * 2
        } else {
            TO_GRAY_BYTES_PER_PIXEL
        };
        let display = pixels.saturating_mul(per_pixel);
        let profile = destination.as_ref().map_or(0, policy_bytes);
        let bytes = stage
            .max(display)
            .saturating_add(profile)
            .saturating_add(BOOKKEEPING_BYTES);
        (bytes, refusal, changes)
    }

    /// `dr_precision_doc_gray_profile_plan`'s answer:
    /// `{"working_memory_plan_bytes","refusal","changes"}` (`changes` false
    /// = the edit would answer `None`).
    pub fn gray_profile_plan(&self, icc: Option<&[u8]>, request: &str) -> Result<Value, String> {
        let request = GrayRequest::parse(request)?;
        let (bytes, refusal, changes) = self.gray_profile_scratch(icc, &request);
        Ok(json!({"working_memory_plan_bytes":bytes,"refusal":refusal,"changes":changes}))
    }

    /// Assign, Convert or Depth (module doc); `None` when it would change
    /// nothing.
    pub fn gray_profile_edit(
        &self,
        icc: Option<&[u8]>,
        request: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let request = GrayRequest::parse(request)?;
        let current = self
            .gray_policy()
            .ok_or_else(|| format!("{} requires a Grayscale document", request.op.menu_name()))?
            .clone();
        let (scratch, refusal, _) = self.gray_profile_scratch(icc, &request);
        // Convert's RGB conversion begins the run itself (`convert_to_profile_with`),
        // so on Convert the reserve below, the neutral-grid pass and the final report
        // run outside a begun run, which the control accepts, as in
        // `convert_gray_to_rgb`.
        let _run = match request.op {
            GrayOp::Convert => None,
            _ => control.map(DrJobControl::begin).transpose()?,
        };
        let job = control.map_or(JobContext::NONE, JobContext::new);
        if let Some(reason) = refusal {
            return Err(reason);
        }
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let rgb = self.without_gray();
        let out = match request.op {
            GrayOp::Assign => {
                let policy = Self::gray_destination_policy(icc, &request, &current)?;
                if policy.bytes() == current.bytes() {
                    None
                } else {
                    if let Some(reason) = rgb.assign_refusal_for(policy.twin()) {
                        return Err(reason);
                    }
                    let assigned = rgb
                        .color_edit_with_context(policy.twin(), false, limit, other, job)?
                        .unwrap_or(rgb);
                    Some(assigned.with_gray(Arc::new(policy))?)
                }
            }
            GrayOp::Convert => {
                let policy = Self::gray_destination_policy(icc, &request, &current)?;
                if policy.bytes() == current.bytes() {
                    None
                } else {
                    let options = self.gray_conversion(&request);
                    let converted = match rgb.convert_to_profile_with(
                        policy.twin(),
                        &options,
                        limit,
                        other,
                        control,
                    )? {
                        Some((document, _)) => document,
                        // No sample changed: the document takes the
                        // destination by assignment, which keeps them all.
                        None => rgb
                            .assign_profile(policy.twin(), limit, other, None)?
                            .unwrap_or(rgb),
                    };
                    let converted = if converted.is_float() {
                        converted
                    } else {
                        converted.on_neutral_grid(
                            current.depth() == GrayDepth::Eight,
                            false,
                            job,
                        )?
                    };
                    Some(converted.with_gray(Arc::new(policy))?)
                }
            }
            GrayOp::Depth(depth) => {
                self.gray_depth_changed(&current, depth, (limit, other), job)?
            }
        };
        job.report(1.0, 5)?;
        Ok(out)
    }

    /// The depth op (module doc).
    fn gray_depth_changed(
        &self,
        current: &GrayPolicy,
        depth: GrayDepth,
        (limit, other): (u64, u64),
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let from = current.depth();
        if from == depth {
            return Ok(None);
        }
        let policy = Arc::new(current.with_depth(depth));
        let next = match (from, depth) {
            (GrayDepth::ThirtyTwo, _) => return Err(FLOAT_DEPTH_REFUSAL.into()),
            (_, GrayDepth::ThirtyTwo) => self
                .without_gray()
                .promote_to_float_with_context(limit, other, job)?,
            (GrayDepth::Sixteen, GrayDepth::Eight) => self
                .on_neutral_grid(true, true, job)?
                .seeding_vector_masks(self.vector_mask_seeds(Clone::clone), limit, other),
            _ => self.without_gray(),
        };
        Ok(Some(next.with_gray(policy)?))
    }

    /// This u16 document with every raster leaf through the neutral-grid
    /// pass (module doc: `[r', r', r', a']`, `eight` at 8 bits), groups
    /// rebuilt around their children; `whole` also puts every mask and alpha
    /// channel on the 8-bit grid (the 16 → 8 depth change).
    fn on_neutral_grid(
        &self,
        eight_bits: bool,
        whole: bool,
        job: JobContext<'_>,
    ) -> Result<Self, String> {
        let layers = map_cmyk_tree(self.root_layers(), whole, &mut |layer| {
            let Content::Raster { image, offset } = &layer.content else {
                return Ok(layer.clone());
            };
            let mut next: PrecisionDocumentLayer = layer.clone();
            next.content = Content::Raster {
                image: neutral_grid(image, eight_bits, job)?,
                offset: *offset,
            };
            Ok(next)
        })?;
        let channels = if whole {
            channels_on_grid(self.channels())?
        } else {
            self.channels().to_vec()
        };
        self.rebuilt(self.width, self.height, layers, channels)
    }
}

/// One u16 raster's `[r', r', r', a']` (module doc).
fn neutral_grid(
    image: &PrecisionImage,
    eight_bits: bool,
    job: JobContext<'_>,
) -> Result<PrecisionImage, String> {
    let (w, h) = image.dimensions();
    pixel_count(w, h)?;
    let grid = |v: u16| if eight_bits { eight(v) } else { v };
    let plane = TilePlane::<[u16; 4]>::from_rows_with_job(
        w,
        h,
        |y, row| {
            let mut x = 0usize;
            for (_, run) in image.row_segments(y, 0, w) {
                let PrecisionRun::Rgba16(run) = run else {
                    return Err("a Grayscale profile edit requires 16-bit storage".into());
                };
                for pixel in run {
                    let gray = grid(pixel[0]);
                    row[x] = [gray, gray, gray, grid(pixel[3])];
                    x += 1;
                }
            }
            job.every(y as usize * w as usize)
        },
        job,
    )?;
    PrecisionImage::from_rgba16_tiles_unseeded(plane, image.profile().clone())
}
