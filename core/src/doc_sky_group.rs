//! Sky Replacement's "New Layers" output (M2b): the adapters over
//! `sky_group`. `doc_sky_replace`'s `sky_replace` dispatches here after its
//! layer, coverage and sky-size checks, so both outputs refuse their inputs
//! identically.
//!
//! The op adds a Sky Replacement Group directly above the clip run
//! that contains or sits on the source (`insert_slot_above_clip_run`, Auto-
//! Blend's rule), so the group is never clipped: a Pass Through group named
//! `params.name`, opacity 1, open, holding bottom-first the "Foreground
//! Color" Curves adjustment (Normal, opacity `color_adjustment/100`), the
//! "Foreground Lighting" Curves adjustment (Multiply or Screen, opacity
//! `lighting/100`) — each with a white canvas mask, as an adjustment layer is
//! made — and the canvas-sized "Sky" raster at (0, 0), Normal, opacity 1,
//! masked by `sky_group::sky_mask`. The children are unclipped. With
//! `params.decontaminate` on, a fourth child comes first, bottom of
//! the group: the canvas-sized "Foreground Edge" raster at (0, 0), Normal,
//! opacity 1, from `sky_group::edge_planes`, with a binary mask of the edge
//! band; both ops then keep `m0` (and the byte op its canvas copy) until
//! that layer is drawn, after the sky's.
//!
//! Each op owns its whole run: the slot, a capacity preflight over every
//! added entry and mask, the plan (`sky_harmonize::scratch_bytes` with
//! `sky_group::new_layers_bytes`, the value the FFI's plan returns),
//! then one job begin, admission and reservation. The byte op checks the
//! result against the `.rz` writer's contract (`native_save_preflight`), so
//! it never publishes a document the writer refuses.

use std::sync::Arc;

use image::{GrayImage, Luma, RgbaImage};
use serde_json::json;

use crate::blend::BlendMode;
use crate::doc::{DrDocument, Layer, LayerRaster};
use crate::doc_mask::MaskProperties;
use crate::doc_sky_replace::{byte, named, not_raster};
use crate::job_control::{DrJobControl, JobContext};
use crate::layer_insert::InsertSlot;
use crate::precision_document::{scalar_and_name, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, reserved_vec, unit_to_u16, PrecisionImage};
use crate::precision_mask::{PrecisionMask, PrecisionPlane};
use crate::sky_group::{
    edge_planes, foreground_curves, new_layers_bytes, sky_planes, COLOR_NAME, EDGE_NAME,
    LIGHTING_NAME, SKY_NAME,
};
use crate::sky_harmonize::{coverage_at, prepare, scratch_bytes, LightingMode, SkyParams};

/// The refusal for a source nested so deep that its group's children would
/// pass the depth's ceiling.
const NESTING: &str = "sky replacement cannot nest its group that deep";

/// The straight RGBA8 canvas `source` read as 0…1 samples.
fn byte_read(source: &[u8]) -> impl Fn(usize) -> [f32; 4] + '_ {
    move |i: usize| {
        let at = i * 4;
        [
            f32::from(source[at]) / 255.0,
            f32::from(source[at + 1]) / 255.0,
            f32::from(source[at + 2]) / 255.0,
            f32::from(source[at + 3]) / 255.0,
        ]
    }
}

/// One Curves adjustment description with per-channel point lists (R, G, B),
/// the meta an adjustment layer carries (`adjust`'s `"curves"` schema).
pub(crate) fn curves_description(points: &[[[f64; 2]; 2]; 3]) -> String {
    json!({
        "type": "adjust",
        "op": "curves",
        "params": {"r": points[0], "g": points[1], "b": points[2]},
    })
    .to_string()
}

/// [`curves_description`] under the typed envelope (`evaluation:
/// precision_v1`), which a high-depth adjustment record requires.
fn precision_curves_description(points: &[[[f64; 2]; 2]; 3]) -> String {
    json!({
        "type": "adjust",
        "evaluation": "precision_v1",
        "op": "curves",
        "params": {"r": points[0], "g": points[1], "b": points[2]},
    })
    .to_string()
}

/// The lighting child's blend mode.
fn lighting_blend(p: &SkyParams) -> BlendMode {
    match p.lighting_mode {
        LightingMode::Multiply => BlendMode::Multiply,
        LightingMode::Screen => BlendMode::Screen,
    }
}

/// The children's slots: consecutive at `slot`'s level, unclipped.
fn child_slot(slot: InsertSlot, k: usize) -> InsertSlot {
    InsertSlot {
        at: slot.at + k,
        level: slot.level,
        clipped: false,
    }
}

impl DrDocument {
    /// The New Layers output of Sky Replacement on `layer` (module doc). `Err`
    /// for a nesting past the depth's ceiling, growth past native capacity, a
    /// refused plan, no sky weight, a cancel and a result the `.rz` writer
    /// would refuse. The caller (`sky_replace`) has checked the layer, the
    /// coverage's length and the sky's size.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sky_replace_group(
        &self,
        layer: usize,
        sky: &RgbaImage,
        coverage: &[u16],
        p: &SkyParams,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<DrDocument, String> {
        let dims = (self.width, self.height);
        let slot = self.insert_slot_above_clip_run(layer)?;
        if slot.level + 1 > usize::from(crate::doc_group::MAX_GROUP_DEPTH) {
            return Err(NESTING.into());
        }
        // Sky, the two adjustments (canvas-sized transparent rasters, as
        // `addingAdjustmentLayer` makes them), the container's 1×1 and, with
        // `decontaminate` on, the Foreground Edge raster.
        let edge_dims = p.decontaminate.then_some(dims);
        let added = [dims, dims, dims, (1, 1)];
        if !self.native_layers_fit_dimensions(
            self.layers
                .iter()
                .map(|l| l.pixels.dimensions())
                .chain(added)
                .chain(edge_dims),
        ) || !self.native_masks_fit_dimensions(
            self.layers
                .iter()
                .filter_map(|l| l.mask.as_ref().map(|m| m.dimensions()))
                .chain([dims; 3])
                .chain(edge_dims),
        ) {
            return Err("the edited document exceeds native capacity".into());
        }
        let scratch = scratch_bytes(
            dims,
            sky.dimensions(),
            new_layers_bytes(false, p.decontaminate),
        );
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        admit(self.sky_retained(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let canvas = self
            .layer_canvas_image(layer)
            .ok_or_else(|| not_raster(layer))?;
        let m0 = coverage_at(coverage, dims, dims, job.checkpoints_only())?;
        let prepared = prepare(
            byte_read(canvas.as_raw()),
            dims,
            &m0,
            dims,
            sky.as_raw(),
            sky.dimensions(),
            p,
            job,
        )
        .map_err(|e| named(e, layer))?;
        // The edge layer reads the source and `m0` again after the sky's
        // planes; without it both go now.
        let held = if p.decontaminate {
            Some((m0, canvas))
        } else {
            drop(m0);
            drop(canvas);
            None
        };
        let n = dims.0 as usize * dims.1 as usize;
        let mut pixels: Vec<u8> = reserved_vec(n * 4)?;
        let mut mask: Vec<u8> = reserved_vec(n)?;
        let sky_range = if held.is_some() {
            job.range(0.75, 0.9)
        } else {
            job.range(0.75, 1.0)
        };
        sky_planes(&prepared, dims, p, sky_range, |_, k, coverage| {
            pixels.extend([byte(k[0]), byte(k[1]), byte(k[2]), 255]);
            mask.push(byte(coverage));
        })?;
        let edge = match held {
            Some((m0, canvas)) => {
                let mut pixels: Vec<u8> = reserved_vec(n * 4)?;
                let mut mask: Vec<u8> = reserved_vec(n)?;
                edge_planes(
                    byte_read(canvas.as_raw()),
                    &m0,
                    &prepared,
                    dims,
                    job.range(0.9, 1.0),
                    |_, f, inside| {
                        if inside {
                            pixels.extend(f.map(byte));
                            mask.push(255);
                        } else {
                            pixels.extend([0; 4]);
                            mask.push(0);
                        }
                    },
                )?;
                drop(m0);
                drop(canvas);
                let image = RgbaImage::from_raw(dims.0, dims.1, pixels)
                    .ok_or("sky replacement edge layer")?;
                let mask = GrayImage::from_raw(dims.0, dims.1, mask)
                    .ok_or("sky replacement edge layer mask")?;
                Some((image, mask))
            }
            None => None,
        };
        let curves = foreground_curves(&prepared.harmony);
        drop(prepared);
        let image =
            RgbaImage::from_raw(dims.0, dims.1, pixels).ok_or("sky replacement sky layer")?;
        let mask =
            GrayImage::from_raw(dims.0, dims.1, mask).ok_or("sky replacement sky layer mask")?;
        let white = Arc::new(GrayImage::from_pixel(dims.0, dims.1, Luma([255])));
        let masked = |mut entry: Layer, plane: Arc<GrayImage>| {
            entry.mask = Some(plane);
            entry.mask_enabled = true;
            entry.mask_properties = MaskProperties::at((0, 0));
            entry
        };
        let adjustment = |name: &str, points, opacity: f32, blend: BlendMode| {
            let mut entry = Layer::from_raster(LayerRaster::filled(dims.0, dims.1, [0; 4]), name);
            entry.meta = Some(curves_description(points));
            entry.opacity = opacity;
            entry.blend = blend;
            masked(entry, white.clone())
        };
        let color = adjustment(
            COLOR_NAME,
            &curves.color,
            p.color_adjustment / 100.0,
            BlendMode::Normal,
        );
        let lighting = adjustment(
            LIGHTING_NAME,
            &curves.lighting,
            p.lighting / 100.0,
            lighting_blend(p),
        );
        let sky_layer = masked(
            Layer::from_raster(LayerRaster::unseeded(image), SKY_NAME),
            Arc::new(mask),
        );
        let edge_layer = edge.map(|(image, mask)| {
            masked(
                Layer::from_raster(LayerRaster::unseeded(image), EDGE_NAME),
                Arc::new(mask),
            )
        });
        let children: Vec<Layer> = edge_layer
            .into_iter()
            .chain([color, lighting, sky_layer])
            .collect();
        let members: Vec<usize> = (0..children.len()).map(|k| slot.at + k).collect();
        let refused = "sky replacement could not insert its layers";
        let mut doc = self.clone();
        for (k, entry) in children.into_iter().enumerate() {
            doc = doc
                .inserting_layer(entry, child_slot(slot, k))
                .ok_or(refused)?;
        }
        let (doc, _, _, _) = doc.group_layers(&members, &p.name).ok_or(NESTING)?;
        doc.native_save_preflight()?;
        job.report(1.0, 5)?;
        Ok(doc)
    }
}

impl PrecisionDocument {
    /// The 16-bit New Layers output (module doc): the sky a u16 raster in the
    /// document's profile, every mask u16. `Err` as the byte op's, plus a
    /// document within 4 records (5 with the edge layer) of the record cap.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sky_replace_group(
        &self,
        layer: usize,
        sky: &RgbaImage,
        coverage: &[u16],
        p: &SkyParams,
        limit: u64,
        other: u64,
        control: Option<&DrJobControl>,
    ) -> Result<PrecisionDocument, String> {
        let record = self
            .layer(layer)
            .ok_or_else(|| format!("sky replacement layer {layer} is out of range"))?;
        let source = record.source().ok_or_else(|| not_raster(layer))?;
        let dims = self.dimensions();
        let slot = self.insert_slot_above_clip_run(layer)?;
        if slot.level + 1 > crate::precision_render::MAX_GROUP_DEPTH {
            return Err(NESTING.into());
        }
        for name in [p.name.as_str(), SKY_NAME, LIGHTING_NAME, COLOR_NAME] {
            scalar_and_name(name, 1.0)?;
        }
        if p.decontaminate {
            scalar_and_name(EDGE_NAME, 1.0)?;
        }
        let records = if p.decontaminate { 5 } else { 4 };
        if self.records + records > crate::precision_render::MAX_RECORDS {
            return Err("too many high-depth document records".into());
        }
        let (mut rasters, mut masks) = self.native_dimensions();
        rasters.push(dims);
        masks.extend([dims; 3]);
        if p.decontaminate {
            rasters.push(dims);
            masks.push(dims);
        }
        self.native_fits_geometry(dims, &rasters, &masks, self.channels.len())?;
        let scratch = scratch_bytes(
            dims,
            sky.dimensions(),
            new_layers_bytes(true, p.decontaminate),
        );
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        admit(self.retained_bytes(), scratch, other, limit)?;
        job.reserve(scratch)?;
        let m0 = coverage_at(coverage, dims, dims, job.checkpoints_only())?;
        let (offset, (sw, sh)) = (record.offset(), source.dimensions());
        let width = dims.0 as usize;
        let read = |i: usize| {
            let x = (i % width) as i64 - i64::from(offset.0);
            let y = (i / width) as i64 - i64::from(offset.1);
            if x < 0 || y < 0 || x >= i64::from(sw) || y >= i64::from(sh) {
                return [0.0; 4];
            }
            source.sample_at(x as u32, y as u32).unwrap_or([0.0; 4])
        };
        let prepared = prepare(
            read,
            dims,
            &m0,
            dims,
            sky.as_raw(),
            sky.dimensions(),
            p,
            job,
        )
        .map_err(|e| named(e, layer))?;
        // The edge layer reads `m0` again after the sky's planes; without it
        // `m0` goes now.
        let held = if p.decontaminate {
            Some(m0)
        } else {
            drop(m0);
            None
        };
        let n = width * dims.1 as usize;
        let mut codes: Vec<[u16; 4]> = reserved_vec(n)?;
        let mut mask: Vec<u16> = reserved_vec(n)?;
        let sky_range = if held.is_some() {
            job.range(0.75, 0.9)
        } else {
            job.range(0.75, 1.0)
        };
        sky_planes(&prepared, dims, p, sky_range, |_, k, coverage| {
            codes.push([
                unit_to_u16(k[0]),
                unit_to_u16(k[1]),
                unit_to_u16(k[2]),
                65535,
            ]);
            mask.push(unit_to_u16(coverage));
        })?;
        let edge = match held {
            Some(m0) => {
                let mut codes: Vec<[u16; 4]> = reserved_vec(n)?;
                let mut mask: Vec<u16> = reserved_vec(n)?;
                edge_planes(
                    read,
                    &m0,
                    &prepared,
                    dims,
                    job.range(0.9, 1.0),
                    |_, f, inside| {
                        if inside {
                            codes.push(f.map(unit_to_u16));
                            mask.push(65535);
                        } else {
                            codes.push([0; 4]);
                            mask.push(0);
                        }
                    },
                )?;
                drop(m0);
                Some((codes, mask))
            }
            None => None,
        };
        let curves = foreground_curves(&prepared.harmony);
        drop(prepared);
        let image = PrecisionImage::from_rgba16_unseeded(
            dims.0,
            dims.1,
            codes,
            self.profile().clone(),
            None,
        )?;
        let canvas_mask =
            |plane: PrecisionPlane| PrecisionMask::new(plane, MaskProperties::at((0, 0)), true);
        let white = canvas_mask(PrecisionPlane::adopt(dims.0, dims.1, vec![65535; n])?)?;
        let adjustment = |name: &str, points, opacity: f32, blend: BlendMode| {
            let entry = PrecisionDocumentLayer::adjustment(
                name,
                &precision_curves_description(points),
                opacity,
                true,
                self.profile(),
            )?
            .with_blend_mode(blend)?;
            Ok::<_, String>(entry.with_mask(Some(white.clone())))
        };
        let color = adjustment(
            COLOR_NAME,
            &curves.color,
            p.color_adjustment / 100.0,
            BlendMode::Normal,
        )?;
        let lighting = adjustment(
            LIGHTING_NAME,
            &curves.lighting,
            p.lighting / 100.0,
            lighting_blend(p),
        )?;
        let sky_layer = PrecisionDocumentLayer::raster(SKY_NAME, image, (0, 0), 1.0, true)?
            .with_mask(Some(canvas_mask(PrecisionPlane::adopt(
                dims.0, dims.1, mask,
            )?)?));
        let edge_layer = match edge {
            Some((codes, mask)) => {
                let image = PrecisionImage::from_rgba16_unseeded(
                    dims.0,
                    dims.1,
                    codes,
                    self.profile().clone(),
                    None,
                )?;
                Some(
                    PrecisionDocumentLayer::raster(EDGE_NAME, image, (0, 0), 1.0, true)?.with_mask(
                        Some(canvas_mask(PrecisionPlane::adopt(dims.0, dims.1, mask)?)?),
                    ),
                )
            }
            None => None,
        };
        let children: Vec<PrecisionDocumentLayer> = edge_layer
            .into_iter()
            .chain([color, lighting, sky_layer])
            .collect();
        let members: Vec<usize> = (0..children.len()).map(|k| slot.at + k).collect();
        let mut doc = self.clone();
        for (k, entry) in children.into_iter().enumerate() {
            doc = doc.inserting_record(entry, child_slot(slot, k))?;
        }
        let (doc, _) = doc.group_layers(&members, &p.name, true)?.ok_or(NESTING)?;
        job.report(1.0, 5)?;
        Ok(doc)
    }
}
