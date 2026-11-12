//! A CMYK document's profile and depth edits (#1 phase 5): Edit ▸ Assign
//! Profile and Convert to Profile between CMYK profiles, Image ▸ Mode ▸
//! 8/16 Bits/Channel, and File ▸ New in CMYK — their request, plans and
//! refusals.
//!
//! * **Assign** gives the document a new [`CmykPolicy`] from the profile at
//!   the same depth and touches no sample: the ink numbers stay and their
//!   colour changes, which is what Assign means — a layer style's inks too,
//!   whose hex is re-canonicalised under the new profile
//!   (`cmyk_style::canonical_hex`), and a layer comp's recorded ones
//!   (`cmyk_comps`). The same bytes is `None`.
//! * **Convert** runs every raster record through
//!   `cmyk_color::CmykToCmyk` (the request's intent and BPC; relative with
//!   BPC when omitted), memoized per
//!   distinct ink (`ink_key`), and keeps alpha, masks, adjustment records,
//!   alpha channels, vector masks, ids, names, metadata and the chrome;
//!   every layer style's ink, and every ink a layer comp recorded, goes
//!   through the same leg, its hex the new profile's; a
//!   converted raster's type, shape or fill description is dropped (the host
//!   re-renders it in the new profile), while Assign and Depth keep it.
//!   `flatten: true` flattens onto
//!   paper first (`cmyk_flatten`), "Flatten Image to Preserve
//!   Appearance": a stack converted record by record composites the
//!   converted numbers, not the converted composite. The declared depth is
//!   kept, so an 8-bit document rounds each converted ink to the 8-bit grid
//!   (×257). Converting into the bytes the document already holds changes
//!   nothing and is `None`.
//! * **Depth** 16 → 8 rounds every raster sample (ink and alpha), every
//!   mask sample and every alpha-channel sample to `round(v / 257) · 257`,
//!   what an 8-bit file holds, and re-sets each raster-less vector mask from
//!   its shape (`seeding_vector_masks`); 8 → 16
//!   changes only the declared depth (the stored codes already are 16-bit).
//!   The same depth is `None`.
//! * **New** ([`PrecisionDocument::new_cmyk`]) is one `CmykImage::filled`
//!   record at the request's `fill` (straight `[c, m, y, k, a]` in 0…1,
//!   8-bit rounded at depth 8), named `layer_name`, at `ppi`.
//!
//! The profile is the caller's `icc`, else the working CMYK profile
//! (`working_cmyk`). A profile that is not CMYK or has no `A2B` refuses with
//! [`CmykPolicy::new`]'s sentences; Convert also refuses a destination with
//! no `B2A` with `cmyk_color::require_encodable`'s. None of this is a format
//! change: the `CMYK` table already holds the profile and the depth.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::cmyk_adjust_layer::DISPLAY_BYTES_PER_PIXEL;
use crate::cmyk_color::{ink_key, require_encodable, CmykToCmyk, ConversionOptions};
use crate::cmyk_composite::unit;
use crate::cmyk_convert_tree::{carry, leaf_count};
use crate::cmyk_document::{CmykDepth, CmykPolicy};
use crate::cmyk_image::CmykImage;
use crate::icc_intent::RenderingIntent;
use crate::job_control::{DrJobControl, JobContext};
use crate::metadata::{Metadata, Resolution};
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};
use crate::precision_mask::{PrecisionChannel, PrecisionMask, PrecisionPlane};
use crate::soft_proof::memo_peak_bytes_for;

/// Bytes a converted raster pixel charges beside the ink memo, the profile
/// and the view: the converted plane (10 B/px) and the host's display
/// preparation of the result. Fitted to `scripts/cmyk-admission/measure.py`
/// (`cmyk-convert-16`); a miss raises this term.
pub(crate) const CONVERT_BYTES_PER_PIXEL: u64 = 30;

/// Bytes a raster pixel a 16 → 8 depth change charges: the rounded plane
/// (10 B/px) and the display preparation (`cmyk-depth-8`); a miss raises
/// this term.
pub(crate) const DEPTH_BYTES_PER_PIXEL: u64 = 30;

/// What the profile edit does (module doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProfileOp {
    Assign,
    Convert,
    Depth(CmykDepth),
}

impl ProfileOp {
    /// The menu item the op mirrors, for refusals that name it.
    fn menu_name(self) -> &'static str {
        match self {
            ProfileOp::Assign => "Assign Profile",
            ProfileOp::Convert => "Convert to Profile",
            ProfileOp::Depth(CmykDepth::Eight) => "8 Bits/Channel",
            ProfileOp::Depth(CmykDepth::Sixteen) => "16 Bits/Channel",
        }
    }
}

/// One parsed request.
struct ProfileRequest {
    op: ProfileOp,
    conversion: ConversionOptions,
    flatten: bool,
}

fn depth_of(value: &Value, what: &str) -> Result<CmykDepth, String> {
    value
        .as_u64()
        .and_then(|bits| CmykDepth::from_bits(bits as u32))
        .ok_or_else(|| format!("{what} depth must be 8 or 16"))
}

fn object_of<'a>(
    text: &str,
    what: &str,
    value: &'a mut Value,
) -> Result<&'a Map<String, Value>, String> {
    *value = if text.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(text).map_err(|e| format!("{what} is not valid JSON: {e}"))?
    };
    value
        .as_object()
        .ok_or_else(|| format!("{what} must be a JSON object"))
}

impl ProfileRequest {
    /// `{"op":"assign"|"convert"|"depth","intent","black_point_compensation",
    /// "flatten","depth"}`: `op` required, `depth` only (and always) with
    /// `op` depth, `flatten` only with convert; an unknown key refuses.
    fn parse(text: &str) -> Result<Self, String> {
        let mut value = Value::Null;
        let object = object_of(text, "the CMYK profile request", &mut value)?;
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "op" | "intent" | "black_point_compensation" | "flatten" | "depth"
            ) {
                return Err(format!(
                    "the CMYK profile request has an unknown key \"{key}\""
                ));
            }
        }
        let mut conversion = ConversionOptions::default();
        if let Some(intent) = object.get("intent").filter(|v| !v.is_null()) {
            let text = intent.as_str().ok_or("intent must be a string")?;
            conversion.intent = RenderingIntent::parse(text).ok_or_else(|| {
                format!(
                    "intent \"{text}\" is not one of perceptual, relative, saturation or absolute"
                )
            })?;
        }
        let flag = |key: &str| -> Result<Option<bool>, String> {
            match object.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::Bool(v)) => Ok(Some(*v)),
                Some(_) => Err(format!("{key} must be a boolean")),
            }
        };
        if let Some(bpc) = flag("black_point_compensation")? {
            conversion.black_point_compensation = bpc;
        }
        let flatten = flag("flatten")?;
        let depth = object.get("depth").filter(|v| !v.is_null());
        let op = match object.get("op").and_then(Value::as_str) {
            Some("assign") => ProfileOp::Assign,
            Some("convert") => ProfileOp::Convert,
            Some("depth") => ProfileOp::Depth(depth_of(
                depth.ok_or("a depth change needs \"depth\": 8 or 16")?,
                "the CMYK",
            )?),
            Some(other) => {
                return Err(format!(
                    "the CMYK profile op \"{other}\" is not one of assign, convert or depth"
                ))
            }
            None => return Err("the CMYK profile request needs an \"op\"".into()),
        };
        if depth.is_some() && !matches!(op, ProfileOp::Depth(_)) {
            return Err("\"depth\" belongs to the depth op only".into());
        }
        if flatten.is_some() && op != ProfileOp::Convert {
            return Err("\"flatten\" belongs to the convert op only".into());
        }
        Ok(Self {
            op,
            conversion,
            flatten: flatten.unwrap_or(false),
        })
    }
}

/// The policy `icc` (else the working CMYK profile) gives at `depth`.
fn policy_of(icc: Option<&[u8]>, depth: CmykDepth) -> Result<CmykPolicy, String> {
    match icc {
        Some(bytes) => CmykPolicy::new(bytes, depth),
        None => {
            let bytes = crate::working_cmyk::working_cmyk_bytes()
                .ok_or("no working CMYK profile is set, so a CMYK profile must be named")?;
            CmykPolicy::new(&bytes, depth)
        }
    }
}

/// File ▸ New in CMYK's working memory over `pixels` in `policy`: the new
/// document's display preparation (`DISPLAY_BYTES_PER_PIXEL`; the filled
/// record itself is at most four shared tiles), the parsed profile and its
/// bytes, the view link and bookkeeping (`new-cmyk-16`).
pub(crate) fn new_cmyk_scratch(pixels: u64, policy: &CmykPolicy) -> u64 {
    pixels.saturating_mul(DISPLAY_BYTES_PER_PIXEL)
        + policy.profile.bytes()
        + policy.profile_bytes().len() as u64
        + CmykPolicy::view_bytes()
        + BOOKKEEPING_BYTES
}

/// An 8-bit grid code: `round(v / 257) · 257` (shared with the Gray
/// document's depth and convert edits, `gray_profile`).
pub(crate) fn eight(v: u16) -> u16 {
    ((u32::from(v) + 128) / 257 * 257) as u16
}

/// `mask` with every sample on the 8-bit grid.
/// `layers` with every leaf, at any depth, replaced by `leaf(layer)` and every
/// group rebuilt around its mapped children (`cmyk_convert_tree::carry`), so
/// a 4a group's members convert with the root's; `round_masks` puts every
/// record's mask, groups' included, on the 8-bit grid. The Gray document's
/// 16 → 8 edit walks its tree through it too (`gray_profile`).
pub(crate) fn map_cmyk_tree(
    layers: &[PrecisionDocumentLayer],
    round_masks: bool,
    leaf: &mut dyn FnMut(&PrecisionDocumentLayer) -> Result<PrecisionDocumentLayer, String>,
) -> Result<Vec<PrecisionDocumentLayer>, String> {
    let mut out = Vec::with_capacity(layers.len());
    for layer in layers {
        let mut next = match layer.children() {
            Some(children) => carry(
                layer,
                PrecisionDocumentLayer::group(
                    &layer.name,
                    map_cmyk_tree(children, round_masks, leaf)?,
                    layer.opacity,
                    layer.visible,
                    layer.is_pass_through(),
                )?,
            ),
            None => leaf(layer)?,
        };
        if round_masks {
            if let Some(mask) = layer.mask() {
                next.mask = Some(mask_on_grid(mask)?);
            }
        }
        out.push(next);
    }
    Ok(out)
}

/// Every record's style that carries an ink, at any depth, rewritten by `f`
/// (#1: Convert maps each ink through the conversion's leg, Assign keeps
/// the inks and re-canonicalises their hex under the new profile,
/// `cmyk_style`).
fn map_inked_styles(
    layers: &mut [PrecisionDocumentLayer],
    f: &mut dyn FnMut(&crate::style::LayerStyle) -> Result<crate::style::LayerStyle, String>,
) -> Result<(), String> {
    for layer in layers.iter_mut() {
        if let Some(style) = layer.style.as_deref().filter(|s| s.has_inks()) {
            layer.style = Some(Arc::new(f(style)?));
        }
        if let Content::Group(children) | Content::PassThrough(children) = &mut layer.content {
            map_inked_styles(Arc::make_mut(children).as_mut_slice(), f)?;
        }
    }
    Ok(())
}

/// Every inked style of `layers` with each ink through `leg` (four-decimal
/// quantized, as a parsed ink is) and its hex the ink's sRGB under `policy`:
/// Convert to Profile and a converting layers paste (#1, `cmyk_style`).
pub(crate) fn convert_style_inks(
    layers: &mut [PrecisionDocumentLayer],
    leg: &mut CmykToCmyk,
    policy: &CmykPolicy,
) -> Result<(), String> {
    map_inked_styles(layers, &mut |s| converted_inks(s, leg, policy))
}

/// One style with each ink through `leg` (four-decimal quantized) and its hex
/// the ink's sRGB under `policy`: [`convert_style_inks`]' rewrite, which
/// Convert to Profile also gives a comp's recorded style (`cmyk_comps`).
fn converted_inks(
    style: &crate::style::LayerStyle,
    leg: &mut CmykToCmyk,
    policy: &CmykPolicy,
) -> Result<crate::style::LayerStyle, String> {
    style.map_inks(&mut |ink| {
        let key = ink_key(ink.map(crate::cmyk_composite::code));
        let mapped = leg.ink(key, ink.map(f64::from)).map(crate::style_json::q4);
        Ok((mapped, crate::cmyk_style::hex_of(mapped, policy)?))
    })
}

/// `channels` with every sample on the 8-bit grid: a 16 → 8 depth change's
/// alpha channels (CMYK's and Gray's, `gray_profile`).
pub(crate) fn channels_on_grid(
    channels: &[PrecisionChannel],
) -> Result<Vec<PrecisionChannel>, String> {
    channels
        .iter()
        .map(|channel| {
            let (w, h) = channel.plane().dimensions();
            let mut pixels = crate::precision_image::reserved_vec(pixel_count(w, h)?)?;
            pixels.extend(channel.plane().samples().iter().map(|v| eight(*v)));
            let mut next = channel.clone();
            next.plane = PrecisionPlane::adopt(w, h, pixels)?;
            Ok(next)
        })
        .collect()
}

fn mask_on_grid(mask: &PrecisionMask) -> Result<PrecisionMask, String> {
    let (w, h) = mask.plane().dimensions();
    let mut pixels = crate::precision_image::reserved_vec(pixel_count(w, h)?)?;
    pixels.extend(mask.plane().samples().iter().map(|v| eight(*v)));
    Ok(PrecisionMask {
        plane: PrecisionPlane::adopt(w, h, pixels)?,
        properties: mask.properties().clone(),
        enabled: mask.enabled(),
    })
}

impl PrecisionDocument {
    /// Every CMYK raster leaf's pixels, at any depth (4a groups included).
    fn cmyk_raster_pixels(&self) -> u64 {
        fn walk(layers: &[PrecisionDocumentLayer]) -> u64 {
            layers
                .iter()
                .map(|layer| match layer.children() {
                    Some(children) => walk(children),
                    None => layer.cmyk_source().map_or(0, |image| {
                        let (w, h) = image.dimensions();
                        u64::from(w) * u64::from(h)
                    }),
                })
                .sum()
        }
        walk(self.root_layers())
    }

    /// Every record's mask samples, at any depth (groups' masks included).
    pub(crate) fn cmyk_mask_samples(&self) -> u64 {
        fn walk(layers: &[PrecisionDocumentLayer]) -> u64 {
            layers
                .iter()
                .map(|layer| {
                    let own = layer.mask().map_or(0, |mask| {
                        let (w, h) = mask.plane().dimensions();
                        u64::from(w) * u64::from(h)
                    });
                    own + layer.children().map_or(0, walk)
                })
                .sum()
        }
        walk(self.root_layers())
    }

    /// The working memory of `request` (module doc's plans) and its refusal.
    fn cmyk_profile_scratch(
        &self,
        icc: Option<&[u8]>,
        request: &ProfileRequest,
    ) -> (u64, Option<String>) {
        let (w, h) = self.dimensions();
        let canvas = u64::from(w) * u64::from(h);
        let display = canvas.saturating_mul(DISPLAY_BYTES_PER_PIXEL);
        let Some(current) = self.cmyk_policy() else {
            return (BOOKKEEPING_BYTES, Some("the document is not CMYK".into()));
        };
        let destination = match request.op {
            ProfileOp::Depth(depth) => CmykPolicy::new(current.profile_bytes(), depth),
            _ => policy_of(icc, current.depth()),
        };
        let profile = destination
            .as_ref()
            .map_or(0, |p| p.profile.bytes() + p.profile_bytes().len() as u64);
        let view = CmykPolicy::view_bytes();
        // Assign and Convert rewrite each recorded inked style of the comps
        // (`cmyk_comps`); Convert's flatten clears them first.
        let comps = crate::gray_comps::carried_bytes(&self.comps);
        let mut refusal = destination.as_ref().err().cloned();
        let scratch = match request.op {
            ProfileOp::Assign => display + profile + view + comps,
            ProfileOp::Convert => {
                if refusal.is_none() {
                    refusal = destination.as_ref().ok().and_then(|p| {
                        require_encodable(&p.profile)
                            .and_then(|()| {
                                CmykToCmyk::new(&current.profile, &p.profile, request.conversion)
                                    .map(|_| ())
                            })
                            .err()
                    });
                }
                let pixels = if request.flatten {
                    canvas
                } else {
                    self.cmyk_raster_pixels()
                };
                let flatten = if request.flatten {
                    self.cmyk_flatten_scratch_bytes()
                } else {
                    comps
                };
                flatten
                    + pixels.saturating_mul(CONVERT_BYTES_PER_PIXEL)
                    + memo_peak_bytes_for::<[f64; 4]>(pixels)
                    + profile
                    + view
            }
            ProfileOp::Depth(depth) => {
                let rounding = depth == CmykDepth::Eight && current.depth() == CmykDepth::Sixteen;
                let rounded = if rounding {
                    self.cmyk_raster_pixels()
                        .saturating_mul(DEPTH_BYTES_PER_PIXEL)
                        + self.cmyk_mask_samples() * 2
                        + self.channels().len() as u64 * canvas * 2
                } else {
                    0
                };
                rounded + display + profile + view
            }
        };
        (scratch + BOOKKEEPING_BYTES, refusal)
    }

    /// `dr_precision_doc_cmyk_profile_plan`'s answer:
    /// `{"working_memory_plan_bytes","refusal","flatten_required"}`.
    /// `flatten_required` is always false: Convert never needs a flatten,
    /// it is offered to preserve the appearance (module doc).
    pub fn cmyk_profile_plan(&self, icc: Option<&[u8]>, request: &str) -> Result<Value, String> {
        let request = ProfileRequest::parse(request)?;
        let (bytes, refusal) = self.cmyk_profile_scratch(icc, &request);
        Ok(json!({"working_memory_plan_bytes":bytes,"refusal":refusal,"flatten_required":false}))
    }

    /// Assign, Convert or Depth (module doc); `None` when it would change
    /// nothing.
    pub fn cmyk_profile_edit(
        &self,
        icc: Option<&[u8]>,
        request: &str,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<Option<Self>, String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let request = ProfileRequest::parse(request)?;
        let current = self.require_cmyk_policy(request.op.menu_name())?.clone();
        let (scratch, refusal) = self.cmyk_profile_scratch(icc, &request);
        if let Some(reason) = refusal {
            return Err(reason);
        }
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let out = match request.op {
            ProfileOp::Assign => {
                let policy = policy_of(icc, current.depth())?;
                if policy.profile_bytes() == current.profile_bytes() {
                    None
                } else {
                    let mut next = self.clone();
                    // The inks are kept, their hex is the new profile's
                    // (`cmyk_style::canonical_hex`), a comp's recorded ones
                    // too (`cmyk_comps`).
                    let mut canonical =
                        |s: &crate::style::LayerStyle| crate::cmyk_style::canonical_hex(s, &policy);
                    map_inked_styles(
                        Arc::make_mut(&mut next.layers).as_mut_slice(),
                        &mut canonical,
                    )?;
                    next.comps = crate::cmyk_comps::inks_mapped(&self.comps, &mut canonical)?;
                    next.cmyk = Some(Arc::new(policy));
                    Some(next)
                }
            }
            ProfileOp::Convert => {
                self.cmyk_converted(icc, &current, &request, (limit, other), job)?
            }
            ProfileOp::Depth(depth) => {
                self.cmyk_depth_changed(&current, depth, (limit, other), job)?
            }
        };
        job.report(1.0, 5)?;
        Ok(out)
    }

    fn cmyk_converted(
        &self,
        icc: Option<&[u8]>,
        current: &CmykPolicy,
        request: &ProfileRequest,
        (limit, other): (u64, u64),
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let policy = policy_of(icc, current.depth())?;
        if policy.profile_bytes() == current.profile_bytes() {
            return Ok(None);
        }
        let depth = policy.depth();
        let flattened;
        let source = if request.flatten {
            flattened = self.cmyk_flatten(job.range(0.0, 0.3))?;
            &flattened
        } else {
            self
        };
        let mut leg = CmykToCmyk::new(&current.profile, &policy.profile, request.conversion)?;
        let quantize = |v: f64| -> u16 {
            let c = crate::cmyk_composite::code(v as f32);
            match depth {
                CmykDepth::Sixteen => c,
                CmykDepth::Eight => eight(c),
            }
        };
        let count = leaf_count(source.root_layers()).max(1) as f64;
        let mut index = 0usize;
        // Every CMYK raster leaf at any depth (4a groups included), groups
        // rebuilt around their converted children.
        let layers = map_cmyk_tree(source.root_layers(), false, &mut |layer| {
            let Content::CmykRaster { image, offset } = &layer.content else {
                return Ok(layer.clone());
            };
            let (w, h) = image.dimensions();
            let part = job.range(
                0.3 + 0.7 * index as f64 / count,
                0.3 + 0.7 * (index + 1) as f64 / count,
            );
            index += 1;
            let converted = CmykImage::from_rows_with_job(
                w,
                h,
                |y, row| {
                    let mut x = 0usize;
                    for (_, run) in image.row_segments(y, 0, w) {
                        for sample in run {
                            let ink4 = [sample[0], sample[1], sample[2], sample[3]];
                            let ink = leg.ink(ink_key(ink4), ink4.map(|v| f64::from(unit(v))));
                            row[x] = [
                                quantize(ink[0]),
                                quantize(ink[1]),
                                quantize(ink[2]),
                                quantize(ink[3]),
                                sample[4],
                            ];
                            x += 1;
                        }
                    }
                    part.every(y as usize * w as usize)
                },
                part,
            )?;
            let mut next = layer.clone();
            next.content = Content::CmykRaster {
                image: converted,
                offset: *offset,
            };
            // The converted ink is no longer the description's rendering;
            // the host re-renders it in the new profile (#1).
            next.description = None;
            Ok(next)
        })?;
        // Every ink of every style through the same leg (`cmyk_style`), its
        // hex the new profile's; a comp's recorded style too (`cmyk_comps`),
        // so applying it sets the style its layer now holds.
        let mut layers = layers;
        convert_style_inks(&mut layers, &mut leg, &policy)?;
        let mut base = source.clone();
        base.comps = crate::cmyk_comps::inks_mapped(&source.comps, &mut |s| {
            converted_inks(s, &mut leg, &policy)
        })?;
        base.cmyk = Some(Arc::new(policy));
        // Alpha channels carry verbatim (they hold no ink); a raster-less
        // vector mask is re-set from its shape where the rebuild left it
        // dead (`precision_vector_mask_carry`).
        let next = base.rebuilt(
            source.width,
            source.height,
            layers,
            source.channels().to_vec(),
        )?;
        Ok(Some(next.seeding_vector_masks(
            source.vector_mask_seeds(Clone::clone),
            limit,
            other,
        )))
    }

    fn cmyk_depth_changed(
        &self,
        current: &CmykPolicy,
        depth: CmykDepth,
        (limit, other): (u64, u64),
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        if depth == current.depth() {
            return Ok(None);
        }
        let policy = CmykPolicy::new(current.profile_bytes(), depth)?;
        let eight_bits = depth == CmykDepth::Eight;
        // Every record at any depth (4a groups included); 16 → 8 rounds each
        // raster leaf and every record's mask to the 8-bit grid.
        let layers = map_cmyk_tree(self.root_layers(), eight_bits, &mut |layer| {
            let mut next = layer.clone();
            if eight_bits {
                if let Content::CmykRaster { image, offset } = &layer.content {
                    let (w, h) = image.dimensions();
                    let rounded = CmykImage::from_rows_with_job(
                        w,
                        h,
                        |y, row| {
                            let mut x = 0usize;
                            for (_, run) in image.row_segments(y, 0, w) {
                                for sample in run {
                                    row[x] = sample.map(eight);
                                    x += 1;
                                }
                            }
                            job.every(y as usize * w as usize)
                        },
                        job,
                    )?;
                    next.content = Content::CmykRaster {
                        image: rounded,
                        offset: *offset,
                    };
                }
            }
            Ok(next)
        })?;
        // 16 → 8 puts every alpha channel on the 8-bit grid, as it does
        // every mask.
        let channels = if eight_bits {
            channels_on_grid(self.channels())?
        } else {
            self.channels().to_vec()
        };
        let mut base = self.clone();
        base.cmyk = Some(Arc::new(policy));
        let next = base.rebuilt(self.width, self.height, layers, channels)?;
        Ok(Some(next.seeding_vector_masks(
            self.vector_mask_seeds(Clone::clone),
            limit,
            other,
        )))
    }

    /// File ▸ New in CMYK (module doc): request `{"depth":8|16,"fill":[c,m,y,
    /// k,a],"layer_name","ppi"}`, every key optional (16 bits, transparent
    /// paper, "Background", 72 ppi).
    pub fn new_cmyk(
        width: u32,
        height: u32,
        icc: Option<&[u8]>,
        request: &str,
        limit: u64,
        other: u64,
    ) -> Result<Self, String> {
        let mut value = Value::Null;
        let object = object_of(request, "the new CMYK document request", &mut value)?;
        for key in object.keys() {
            if !matches!(key.as_str(), "depth" | "fill" | "layer_name" | "ppi") {
                return Err(format!(
                    "the new CMYK document request has an unknown key \"{key}\""
                ));
            }
        }
        let depth = match object.get("depth").filter(|v| !v.is_null()) {
            Some(value) => depth_of(value, "a new CMYK document's")?,
            None => CmykDepth::Sixteen,
        };
        let fill: [f64; 5] = match object.get("fill").filter(|v| !v.is_null()) {
            None => [0.0; 5],
            Some(value) => {
                let parts: Vec<f64> = value
                    .as_array()
                    .filter(|a| a.len() == 5)
                    .and_then(|a| {
                        a.iter()
                            .map(|v| v.as_f64().filter(|v| (0.0..=1.0).contains(v)))
                            .collect()
                    })
                    .ok_or("a new CMYK document's fill is [c, m, y, k, a], each 0…1")?;
                [parts[0], parts[1], parts[2], parts[3], parts[4]]
            }
        };
        let name = match object.get("layer_name").filter(|v| !v.is_null()) {
            Some(value) => value
                .as_str()
                .ok_or("a new CMYK document's layer_name must be a string")?
                .to_string(),
            None => "Background".to_string(),
        };
        let resolution = match object.get("ppi").filter(|v| !v.is_null()) {
            Some(value) => {
                let ppi = value
                    .as_f64()
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .ok_or("a new CMYK document's ppi must be a positive number")?
                    as f32;
                Resolution { x: ppi, y: ppi }
            }
            None => Resolution::default(),
        };
        let pixels = pixel_count(width, height)? as u64;
        let policy = policy_of(icc, depth)?;
        let scratch = new_cmyk_scratch(pixels, &policy);
        admit(0, scratch, other, limit)?;
        let sample: [u16; 5] = std::array::from_fn(|c| {
            let code = crate::cmyk_composite::code(fill[c] as f32);
            match depth {
                CmykDepth::Sixteen => code,
                CmykDepth::Eight => eight(code),
            }
        });
        let image = CmykImage::filled(width, height, sample)?;
        let leaf = PrecisionDocumentLayer::raster_cmyk(&name, image, (0, 0), 1.0, true)?;
        PrecisionDocument::build_cmyk(
            width,
            height,
            Metadata::default(),
            resolution,
            vec![leaf],
            Vec::new(),
            Arc::new(policy),
        )
    }
}
