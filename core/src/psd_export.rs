//! Basic layered PSD export for the 8-bit document: the export inventory it
//! hands the shared preflight, and the record walk that turns its flat
//! bottom-first stack into classic PSD layer records. The depth-agnostic
//! preflight lives in `psd_export_preflight`, byte serialization in
//! `psd_export_encode`, and the 16-bit document's own walk in `psd_export16`.
//!
//! `psd_export` fills that inventory from `DrDocument` and owns the 8-bit record walk,
//! `psd_export16` fills it from `PrecisionDocument` and owns the 16-bit one (planes are big-endian
//! u16 over `width * 2` byte rows; the host's authoritative projection is required, never rendered
//! here).
//!
//! Both walks write one of three containers (`psd_framing::LayeredContainer`): a classic PSD, a
//! PSB (the same records at the large widths) or a TIFF whose flat strips are the composite and
//! whose tags carry `psd_export_encode::layer_info` and `image_resources`. An adjustment layer
//! with no Photoshop record is written as its stamp (`psd_export_bake`): a canvas-sized Normal
//! pixel layer at 100 % with no mask, keeping its name, visibility, clipping, locks and label —
//! the same answer the inventory gave the preflight, so the report and the bytes never disagree.
//! A pattern fill layer whose style is not baked is written as Photoshop's `PtFl` record
//! (`psd_fill_layer`): its rendered pixels over `psd_fill_layer::record_rect`,
//! its `-2` mask, flags `0x18`, `fxrp` = its reference and its tile in the pattern table; a
//! baked one keeps its pixels and the description loss. A solid color fill layer is written the
//! same way as Photoshop's `SoCo` record, its colour the document's numbers and its `fxrp`
//! the ordinary reference rule, and a gradient fill layer as its `GdFl`, with
//! pixels at 8 bits as Photoshop writes it, its stops in the document's numbers.

use image::RgbaImage;
use serde_json::json;

use crate::authored_color::AuthoredColors;
use crate::doc::{DrDocument, Layer, LayerKind};
use crate::doc_group;
use crate::dr_image::DrImage;
use crate::job_control::{DrJobControl, JobContext};
use crate::psd_adjustment;
use crate::psd_export_encode::{
    composite_planes8, encode_plane, image_resources, layer_info, write_psd, MaskBody, Record,
};
use crate::psd_export_preflight::{
    bake_inexact, blend_key, preflight, stamps, ExportAdjustment, ExportEntry, ExportInventory,
    ExportMask, ExportStyle, CLOSER_NAME,
};
use crate::psd_export_rasterize::export_style;
use crate::psd_framing::LayeredContainer;
use crate::psd_layer_effects::{
    effect_blocks, encode_fill, encode_fill_for, encode_ranges, knockout_blocks,
};
use crate::psd_layer_pixels::import_envelope_bytes;
use crate::style_model::GlobalLight;

pub(crate) use crate::psd_export_preflight::{PsdExportMode, PsdExportReport, UNKNOWN_MODE};

fn mask_geometry(layer: &Layer) -> Option<([i32; 4], u32, u32)> {
    let mask = layer.mask.as_ref()?;
    let (mw, mh) = mask.dimensions();
    let (mx, my) = layer.mask_properties.offset;
    Some((
        [
            my,
            mx,
            my.saturating_add(mh as i32),
            mx.saturating_add(mw as i32),
        ],
        mw,
        mh,
    ))
}

impl DrDocument {
    /// Layer `index`'s written `PtFl` record rectangle (top, left, bottom,
    /// right): `psd_fill_layer::record_rect` over its mask.
    fn fill_rect(&self, index: usize) -> [i32; 4] {
        let layer = &self.layers[index];
        crate::psd_fill_layer::record_rect(
            (self.width, self.height),
            mask_geometry(layer).map(|(bounds, _, _)| {
                (
                    bounds,
                    layer.mask_properties.outside_coverage,
                    layer.mask_enabled,
                )
            }),
        )
    }

    /// Layer `index`'s fill when it is written as a `PtFl`, `SoCo` or `GdFl` record
    /// (module doc): a raster whose meta is a pattern or solid fill
    /// description and whose style is not baked.
    fn exported_fill(&self, index: usize) -> Option<crate::psd_fill_layer::Exported> {
        let layer = self.layers.get(index)?;
        if layer.kind != LayerKind::Raster
            || export_style(layer, false).is_some_and(|style| style.bake.is_some())
        {
            return None;
        }
        crate::psd_fill_layer::exported(layer.meta.as_deref()?, false)
    }

    /// Every layer's [`Self::exported_fill`]; the pattern table takes their
    /// `Exported::pattern`s.
    fn exported_fills(&self) -> Vec<crate::psd_fill_layer::Exported> {
        (0..self.layers.len())
            .filter_map(|index| self.exported_fill(index))
            .collect()
    }

    /// Entry `index` as the layer comps' writer reads it; its style and the
    /// writer's decisions are read only when the document has a comp to
    /// write.
    fn comp_owner(&self, index: usize) -> crate::psd_comps::Owner {
        let layer = &self.layers[index];
        let comps = !self.comps.is_empty();
        // The record walk's own answers (`entry_record`): a style that is
        // baked, and an adjustment layer written as its stamp.
        let baked = comps && export_style(layer, false).is_some_and(|style| style.bake.is_some());
        let stamped = comps
            && crate::retouch_sampling::is_adjustment(layer)
            && (baked
                || psd_adjustment::encode(layer.meta.as_deref().unwrap_or_default()).is_none());
        crate::psd_comps::Owner {
            id: layer.id,
            visible: layer.visible,
            offset: layer.offset,
            pixel: layer.kind == LayerKind::Raster,
            opacity: layer.opacity,
            blend: layer.blend,
            style: layer.style.as_ref().filter(|_| comps).map(|s| s.to_json()),
            baked,
            stamped,
        }
    }

    /// Every entry's [`Self::comp_owner`]: what the preflight and the
    /// pattern table read the comps against (`psd_comp_limits`).
    fn comp_owners(&self) -> Vec<Option<crate::psd_comps::Owner>> {
        (0..self.layers.len())
            .map(|index| Some(self.comp_owner(index)))
            .collect()
    }

    /// True when a visible SIBLING in the contiguous clipped run directly
    /// above `index`, at the same depth, is clipped to it — the run a styled
    /// base's rasterized effects end up underneath in the written file.
    ///
    /// C1's one clip-run question. The stack is flat and a group's children
    /// PRECEDE their folder record, so a deeper entry is skipped rather than
    /// ending the run: a clipped GROUP above the base is a sibling and counts,
    /// and only an entry at the base's own level — unclipped, or a shallower
    /// one, which is the base's own parent — ends it.
    fn clip_base(&self, index: usize) -> bool {
        let depth = self.layers[index].depth;
        for above in &self.layers[index + 1..] {
            if above.depth > depth {
                continue;
            }
            if above.depth < depth || !above.clipped {
                return false;
            }
            if above.visible {
                return true;
            }
        }
        false
    }

    /// This document as the depth-agnostic preflight sees it: one entry per
    /// layer, bottom-first, without reading a single pixel.
    fn export_inventory(&self) -> ExportInventory {
        let owners = self.comp_owners();
        let comp_styles = crate::psd_comp_limits::recorded_styles(&self.comps, &owners);
        let fills = self.exported_fills();
        let fills: Vec<_> = fills
            .iter()
            .filter_map(crate::psd_fill_layer::Exported::pattern)
            .collect();
        let spots = crate::psd_spot::spot_inventory(
            self.channels
                .iter()
                .filter(|c| c.spot.is_some())
                .map(|c| c.name.as_str()),
        );
        ExportInventory {
            width: self.width,
            height: self.height,
            sample_bytes: 1,
            cmyk: false,
            gray: false,
            lab: false,
            profile_bytes: self.profile.bytes().len() as u64,
            tail_bytes: crate::psd_patterns::tail_bytes_with(
                self.layers
                    .iter()
                    .filter_map(|l| l.renders_any_style())
                    .chain(comp_styles.iter()),
                &fills,
            ),
            profile_name: self.profile.name().to_string(),
            resolution: self.resolution,
            channels: self.channels.len(),
            spot_channels: spots.0,
            spot_name_units: spots.1,
            has_guides: !self.guides.is_empty() || self.ruler_origin != (0.0, 0.0),
            sampler_sizes_lost: self.color_samplers.iter().any(|s| s.sample_size != 1),
            comps: self.comps.len(),
            // The layered mode writes every comp (`psd_comps`).
            comps_written: self.comps.len(),
            comps_narrowed: crate::psd_comp_limits::narrowed(&self.comps, &owners, &|style| {
                crate::psd_layer_effects::bake_reason(style).is_some()
            }),
            path_losses: crate::psd_paths::export_losses(
                &self.paths.entries,
                (self.width, self.height),
            ),
            has_metadata: !self.metadata.is_empty(),
            nondefault_global_light: self.global_light != GlobalLight::default(),
            // D13: this writer stores a qualifying bottom as Photoshop's
            // Background record (`psd_background`), so only a bottom that
            // cannot be one is said.
            background_unwritable: self.bottom_export()
                == crate::psd_background::BottomExport::Unwritable,
            entries: self
                .layers
                .iter()
                .enumerate()
                .map(|(index, layer)| {
                    let group = layer.kind == LayerKind::Group;
                    // An adjustment layer's style travels on its record
                    // (`style_adjustment`).
                    let style = export_style(layer, self.clip_base(index));
                    let baked = style.as_ref().is_some_and(|s| s.bake.is_some());
                    // An adjustment layer's record has no pixels; its
                    // (ignored) pixels do not travel.
                    let adjustment = crate::retouch_sampling::is_adjustment(layer).then(|| {
                        let meta = layer.meta.as_deref().unwrap_or_default();
                        let encoded = psd_adjustment::encode(meta);
                        ExportAdjustment {
                            op: psd_adjustment::op(meta).unwrap_or_default(),
                            inexact: (encoded.is_none() || baked)
                                .then(|| {
                                    bake_inexact(layer.clipped, self.bake_inexact_group(index))
                                })
                                .flatten(),
                            encoded,
                        }
                    });
                    // An adjustment with no record, or with a style Photoshop
                    // cannot hold, is BAKED: a canvas-sized Normal pixel layer
                    // at 100 % with no mask of its own.
                    let stamped = stamps(adjustment.as_ref(), style.as_ref());
                    // A fill written as its `PtFl`, `SoCo` or `GdFl` record keeps its
                    // description; its planes are its record rectangle's
                    // (module doc).
                    let fill_record = self.exported_fill(index).is_some();
                    let fill_rect = fill_record.then(|| self.fill_rect(index));
                    // A BAKED style's record is its bake, so the planes and
                    // offset the preflight forecasts are the grown
                    // rectangle's; a PRESERVED one keeps the layer's own.
                    let extent = (!group && adjustment.is_none() && baked)
                        .then(|| self.style_extent(index))
                        .flatten();
                    let planes = match (group || adjustment.is_some(), extent) {
                        _ if fill_rect.is_some() => {
                            let [top, left, bottom, right] = fill_rect.unwrap_or_default();
                            vec![((right - left) as u64, (bottom - top) as u64); 4]
                        }
                        (true, _) if stamped => {
                            vec![(u64::from(self.width), u64::from(self.height)); 4]
                        }
                        (true, _) => vec![(0, 0); 4],
                        (false, Some((x0, y0, x1, y1))) => {
                            vec![((x1 - x0).max(0) as u64, (y1 - y0).max(0) as u64); 4]
                        }
                        (false, None) => {
                            let (w, h) = layer.pixels.dimensions();
                            vec![(u64::from(w), u64::from(h)); 4]
                        }
                    };
                    let offset = match extent {
                        _ if fill_rect.is_some() => {
                            let [top, left, _, _] = fill_rect.unwrap_or_default();
                            (left, top)
                        }
                        Some((x0, y0, _, _)) => (
                            x0.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
                            y0.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
                        ),
                        None => layer.offset,
                    };
                    // An enabled mask shapes the effects and is inside the
                    // bake; a disabled one — and every mask beside a
                    // PRESERVED style — is written exactly as it is today.
                    let baked_mask =
                        baked && style.as_ref().is_some_and(|style| style.masked) && !group;
                    ExportEntry {
                        name: layer.name.clone(),
                        group,
                        visible: layer.visible,
                        clipped: layer.clipped,
                        blend: if stamped {
                            crate::blend::BlendMode::Normal
                        } else {
                            layer.blend
                        },
                        opacity: if stamped { 1.0 } else { layer.opacity },
                        offset: if stamped { (0, 0) } else { offset },
                        planes,
                        mask: mask_geometry(layer)
                            .filter(|_| !baked_mask && !stamped)
                            .map(|(_, mw, mh)| ExportMask {
                                width: mw,
                                height: mh,
                                density: layer.mask_properties.density,
                                feather: layer.mask_properties.feather,
                                linked: layer.mask_properties.linked,
                                // Written as pixels beside a raster part,
                                // or when a PSD path cannot hold its shape;
                                // any other is its `vmsk` (`psd_paths`).
                                vector: self.live_vector_mask(index).is_some_and(|record| {
                                    record.raster.is_some()
                                        || crate::psd_paths::vmsk_body(
                                            &record.shape,
                                            record.enabled,
                                            record.linked,
                                            (self.width, self.height),
                                        )
                                        .is_none()
                                }),
                            }),
                        described: layer.meta.is_some() && adjustment.is_none() && !fill_record,
                        adjustment,
                        style,
                        retained_source: layer.retained_source.is_some(),
                        warp: layer.effective_warp(&self.profile).is_some(),
                        locked: layer.locks != 0,
                        link_group: layer.link != 0,
                    }
                })
                .collect(),
        }
    }

    /// The preflight for one container and mode at the production limits.
    pub(crate) fn psd_export_report(
        &self,
        container: LayeredContainer,
        mode: PsdExportMode,
    ) -> PsdExportReport {
        let inventory = self.export_inventory();
        let mut report = preflight(
            &inventory,
            container,
            mode,
            container.max_file_bytes(),
            import_envelope_bytes(),
        );
        // Spot channels reach PSD and PSB in #1 phase 1b; a container the
        // writer cannot carry them in (the layered TIFF) refuses by name
        // rather than dropping them.
        if self.has_spot_channel()
            && !crate::psd_export_preflight::spots_written_for(
                inventory.lab,
                inventory.gray,
                inventory.sample_bytes,
                crate::psd_export_preflight::SpotFile::Layered(container),
            )
        {
            report.refusal = Some(crate::psd_export_preflight::spot_refusal(container).into());
        }
        report
    }

    /// The same preflight for a classic PSD with reduced limits, so file-size
    /// and reopen-envelope behavior can be qualified without gigabyte
    /// fixtures. Cannot raise the production caps.
    #[doc(hidden)]
    pub fn psd_export_report_with_limits(
        &self,
        mode: i32,
        max_file_bytes: u64,
        envelope_bytes: u64,
    ) -> String {
        let container = LayeredContainer::Psd;
        match PsdExportMode::from_c(mode) {
            Some(mode) => preflight(
                &self.export_inventory(),
                container,
                mode,
                max_file_bytes.min(container.max_file_bytes()),
                envelope_bytes.min(import_envelope_bytes()),
            )
            .json(container, mode, 8)
            .to_string(),
            None => json!({ "error": UNKNOWN_MODE }).to_string(),
        }
    }

    /// The complete record list in FILE order for the layered mode: each
    /// group's closing divider comes before its contents, and its folder
    /// record after them, which is the mirror image of the importer's walk.
    fn layer_records(
        &self,
        colors: Option<&AuthoredColors>,
        large: bool,
    ) -> Result<Vec<Record>, String> {
        let mut out = Vec::new();
        // Each record's layer (`psd_comps::Owner`), `None` for a closer: what
        // the layer comps' `cmls` settings bind to (`psd_comps`).
        let mut owners = Vec::new();
        for index in 0..self.layers.len() {
            let mut closing: Vec<u16> = self
                .layers
                .iter()
                .enumerate()
                .filter(|(group, layer)| {
                    layer.kind == LayerKind::Group
                        && doc_group::subtree(&self.layers, *group).start == index
                })
                .map(|(_, layer)| layer.depth)
                .collect();
            closing.sort_unstable();
            for _ in &closing {
                out.push(Record {
                    bounds: [0; 4],
                    channels: placeholder_channels(),
                    blend: *b"norm",
                    opacity: 255,
                    clipping: 0,
                    flags: 0x18,
                    mask: None,
                    name: CLOSER_NAME.to_string(),
                    divider: Some(3),
                    adjustment: None,
                    blocks: Vec::new(),
                    ranges: Vec::new(),
                });
                owners.push(None);
            }
            out.push(self.entry_record(index, colors, large)?);
            owners.push(Some(self.comp_owner(index)));
        }
        // A comp's style through the records' own effects codec and colours.
        let light = self.global_light;
        crate::psd_comps::stamp_records(&mut out, &owners, &self.comps, &|style| {
            crate::psd_comps::effect_style_items(
                style,
                light,
                crate::psd_layer_effects::FxColours::Rgb(colors),
            )
        });
        // A live vector mask with no raster part is written as its `vmsk`
        // (`psd_paths`), in place of the baked mask the walk wrote.
        let masks: Vec<Option<crate::psd_paths::WrittenMask>> = owners
            .iter()
            .map(|owner| {
                let id = owner.as_ref()?.id;
                let index = self.layers.iter().position(|l| l.id == id)?;
                self.live_vector_mask(index)
                    .filter(|record| record.raster.is_none())
                    .map(|record| (record.shape, record.enabled, record.linked))
            })
            .collect();
        crate::psd_paths::stamp_vector_masks(&mut out, &masks, (self.width, self.height));
        // Entry 0 as Photoshop's Background record, when it is one: it is
        // FILE record 0, since a group's closer comes before its contents.
        if self.bottom_export() == crate::psd_background::BottomExport::Background {
            if let (Some(record), Some(layer)) = (out.first_mut(), self.layers.first()) {
                crate::psd_background::shape_record(record, layer.locks);
            }
        }
        Ok(out)
    }

    fn entry_record(
        &self,
        index: usize,
        colors: Option<&AuthoredColors>,
        large: bool,
    ) -> Result<Record, String> {
        let layer = &self.layers[index];
        let group = layer.kind == LayerKind::Group;
        let hidden = u8::from(!layer.visible) << 1;
        if crate::retouch_sampling::is_adjustment(layer) {
            let meta = layer.meta.as_deref().unwrap_or_default();
            let adjustment = ExportAdjustment {
                op: String::new(),
                encoded: psd_adjustment::encode(meta),
                inexact: None,
            };
            if stamps(Some(&adjustment), export_style(layer, false).as_ref()) {
                return self.stamped_record(index, large);
            }
        }
        // An adjustment layer with a Photoshop record is written as a
        // pixel-less record carrying its adjustment block; one without was
        // written as its stamp above.
        let adjustment = crate::retouch_sampling::is_adjustment(layer)
            .then(|| {
                psd_adjustment::encode(layer.meta.as_deref().unwrap_or_default())
                    .ok_or("this adjustment layer has no Photoshop adjustment record")
            })
            .transpose()?;
        // The same answer the preflight gave: a style Photoshop can hold is
        // WRITTEN as its own blocks, and only a style it cannot is baked —
        // the grown rectangle's pixels at the grown rectangle's origin.
        // An adjustment record that reaches here holds a PRESERVED style or
        // none (a baked one was its stamp above).
        let style: Option<ExportStyle> = export_style(layer, false);
        let baked = style.as_ref().is_some_and(|style| style.bake.is_some());
        // A pattern or solid fill whose style is not baked is Photoshop's
        // `PtFl`, `SoCo` or `GdFl` record (module doc).
        let fill = self.exported_fill(index);
        let fill_record = fill.is_some();
        let bake = (!group && adjustment.is_none() && baked)
            .then(|| self.rasterized_style(index, colors))
            .transpose()?;
        let (bounds, mut channels) = if group || adjustment.is_some() {
            ([0; 4], placeholder_channels())
        } else if fill.is_some() {
            // The rendered fill over its record rectangle (module doc).
            let rect = self.fill_rect(index);
            let ([w, h], planes) = crate::psd_fill_layer::planes(
                layer.pixels.runs().flatten().copied(),
                layer.pixels.dimensions(),
                layer.offset,
                rect,
                0u8,
            );
            let channels = [-1i16, 0, 1, 2]
                .into_iter()
                .zip(&planes)
                .map(|(id, samples)| (id, encode_plane(samples, w, h, large)))
                .collect();
            (rect, channels)
        } else {
            let ((w, h), samples) = match bake.as_ref() {
                Some(bake) => (
                    bake.pixels.dimensions(),
                    component_planes(
                        bake.pixels.width() as usize * bake.pixels.height() as usize,
                        std::iter::once(bake.pixels.as_raw().as_chunks::<4>().0),
                    ),
                ),
                None => (
                    layer.pixels.dimensions(),
                    component_planes(
                        layer.pixels.width() as usize * layer.pixels.height() as usize,
                        layer.pixels.runs(),
                    ),
                ),
            };
            let (ox, oy) = bake.as_ref().map_or(layer.offset, |bake| bake.origin);
            let mut planes = Vec::with_capacity(4);
            for (id, samples) in [-1i16, 0, 1, 2].into_iter().zip(&samples) {
                planes.push((id, encode_plane(samples, w, h, large)));
            }
            (
                [
                    oy,
                    ox,
                    oy.saturating_add(h as i32),
                    ox.saturating_add(w as i32),
                ],
                planes,
            )
        };
        // An ENABLED mask on a BAKED raster shaped the effects and is inside
        // the bake; a disabled one is untouched by it and written as today.
        let baked_mask = bake.is_some() && layer.mask_enabled;
        let mask = match (mask_geometry(layer), layer.mask.as_ref()) {
            (Some((bounds, mw, mh)), Some(plane)) if !baked_mask => {
                channels.push((-2, encode_plane(plane.as_raw(), mw, mh, large)));
                Some(MaskBody {
                    bounds,
                    outside: layer.mask_properties.outside_coverage,
                    disabled: !layer.mask_enabled,
                })
            }
            _ => None,
        };
        // A preserved style's three constructs: the effects descriptor, the
        // fill-opacity block and the record's blending ranges.
        let preserved = layer.renders_any_style().filter(|_| !baked);
        let pixel_record = !group && adjustment.is_none() && fill.is_none();
        let mut blocks = Vec::new();
        let mut ranges = Vec::new();
        // The record's reference point (`fxrp`): a pattern fill
        // record's is its reference, always (module doc); a solid's takes the
        // ordinary rule.
        let reference = |style: Option<&crate::style::LayerStyle>| match fill
            .as_ref()
            .and_then(crate::psd_fill_layer::Exported::pattern)
        {
            Some(_) => Some(crate::psd_fill_layer::reference_block(layer.reference)),
            None => crate::psd_pattern_fill::record_reference(style, layer.offset, layer.reference),
        };
        // Brightness/Contrast's `CgEd`, directly after its `brit`.
        let (adjustment, companion) = match adjustment {
            Some(encoded) => (Some((encoded.key, encoded.body)), encoded.companion),
            None => (
                fill.as_ref().map(|fill| {
                    crate::psd_fill_layer::encode(
                        fill,
                        crate::psd_layer_effects::FxColours::Rgb(colors),
                    )
                }),
                None,
            ),
        };
        blocks.extend(companion);
        let mut referenced = false;
        if let Some(style) = preserved {
            // An `lfx2` (a stacked style's `lmfx`) is written only when
            // the style holds an ENABLED effect; fill opacity and Blend If are
            // their own constructs, and a descriptor with no enabled effect
            // would reopen as a bare marker.
            if style.has_enabled_effects() {
                // A group record writes `lfxs`, Photoshop's form.
                blocks.extend(effect_blocks(
                    style,
                    self.global_light,
                    crate::psd_layer_effects::FxColours::Rgb(colors),
                    group,
                )?);
                // The record's reference point (`fxrp`).
                blocks.extend(reference(Some(style)));
                referenced = true;
            }
            // A pixel record in a legacy-Fill mode writes `iOpa` even at 255
            // (`encode_fill_for`); a group section keeps `encode_fill`'s.
            let fill = if pixel_record {
                encode_fill_for(layer.blend, style.fill_opacity)
            } else {
                encode_fill(style.fill_opacity)
            };
            if let Some(body) = fill {
                blocks.push((*b"iOpa", body.to_vec()));
            }
            // Photoshop's own blending-options records, after `iOpa` and
            // before `lclr`, and only when they are not at their defaults.
            blocks.extend(knockout_blocks(style));
            if let Some(blend_if) = style.blend_if.filter(|ranges| !ranges.is_identity()) {
                ranges = encode_ranges(&blend_if).to_vec();
            }
        }
        // An unstyled pixel record's `iOpa`: only a legacy-Fill mode writes
        // one, at 255 (#2), where a styled record without effects does.
        if preserved.is_none() && pixel_record {
            if let Some(body) = encode_fill_for(layer.blend, 1.0) {
                blocks.push((*b"iOpa", body.to_vec()));
            }
        }
        // Every other record's reference point (`fxrp`): only a moved
        // one writes it.
        if !referenced {
            blocks.extend(reference(preserved));
        }
        // Photoshop's `lspf`, immediately before `lclr`; an unlocked layer
        // writes none (`psd_locks`).
        blocks.extend(crate::psd_locks::block(layer.locks));
        // The colour LABEL is Photoshop's own `lclr`: u16 big-endian code
        // then six reserved zero bytes. Written only for a labelled record,
        // so an unlabelled document's bytes are exactly what they were
        // (`doc_label`).
        if layer.label != 0 {
            blocks.push((*b"lclr", vec![0, layer.label, 0, 0, 0, 0, 0, 0]));
        }
        Ok(Record {
            bounds,
            channels,
            blend: blend_key(layer.blend),
            opacity: crate::opacity_byte::code(layer.opacity),
            clipping: u8::from(layer.clipped),
            // A fill record's pixels are irrelevant (bit 4): Photoshop's
            // form (module doc).
            flags: if group || fill_record { 0x18 } else { 0x08 } | hidden,
            mask,
            name: layer.name.clone(),
            divider: group.then_some(if layer.open { 1 } else { 2 }),
            adjustment,
            blocks,
            ranges,
        })
    }

    /// A BAKED adjustment layer's record: its stamp as a canvas-sized Normal
    /// pixel layer at 100 % with no mask, keeping the layer's name,
    /// visibility, clipping, locks and label (`psd_export_bake`).
    fn stamped_record(&self, index: usize, large: bool) -> Result<Record, String> {
        let layer = &self.layers[index];
        let stamp = self.adjustment_stamp(index)?;
        let (w, h) = stamp.dimensions();
        let samples = component_planes(
            w as usize * h as usize,
            std::iter::once(stamp.as_raw().as_chunks::<4>().0),
        );
        let channels = [-1i16, 0, 1, 2]
            .into_iter()
            .zip(&samples)
            .map(|(id, samples)| (id, encode_plane(samples, w, h, large)))
            .collect();
        let mut blocks: Vec<_> = crate::psd_locks::block(layer.locks).into_iter().collect();
        if layer.label != 0 {
            blocks.push((*b"lclr", vec![0, layer.label, 0, 0, 0, 0, 0, 0]));
        }
        Ok(Record {
            bounds: [0, 0, h as i32, w as i32],
            channels,
            blend: *b"norm",
            opacity: 255,
            clipping: u8::from(layer.clipped),
            flags: 0x08 | (u8::from(!layer.visible) << 1),
            mask: None,
            name: layer.name.clone(),
            divider: None,
            adjustment: None,
            blocks,
            ranges: Vec::new(),
        })
    }

    /// Writes this document as an 8-bit RGB PSD, PSB or layered TIFF. Pure:
    /// nothing about the document changes, and a refused export never touches
    /// an existing destination, because the whole file is built in memory
    /// first and lands through the shared atomic temp-file rename.
    /// `control` is optional and single-use, begun once for the whole save:
    /// checked after the composite and the records are built, polled as the
    /// file is written (`cancel_io::write_file`) and checked before the
    /// rename; a cancel answers exactly `operation cancelled`.
    pub(crate) fn save_psd(
        &self,
        flat: Option<&DrImage>,
        path: &str,
        container: LayeredContainer,
        mode: PsdExportMode,
        control: Option<&DrJobControl>,
    ) -> Result<(), String> {
        let _run = control.map(DrJobControl::begin).transpose()?;
        let job = control.map_or(JobContext::NONE, JobContext::new);
        let report = self.psd_export_report(container, mode);
        if let Some(refusal) = report.refusal {
            return Err(refusal);
        }
        let large = container.large();
        let composite: RgbaImage = match flat {
            Some(image) => {
                if image.pixels.dimensions() != (self.width, self.height) {
                    return Err(
                        "the supplied composite does not match this document's canvas".into(),
                    );
                }
                image.pixels.clone()
            }
            None => self.flattened()?,
        };
        job.report(0.2, 1)?;
        // ONE colour transform for the whole export, the way a projection
        // builds one for however many styled layers it holds.
        let colors = self.style_colors();
        let records = match mode {
            PsdExportMode::Layers => self.layer_records(colors.as_ref(), large)?,
            PsdExportMode::Flattened => vec![Record {
                bounds: [0, 0, self.height as i32, self.width as i32],
                channels: composite_channels(&composite, large),
                blend: *b"norm",
                opacity: 255,
                clipping: 0,
                flags: 0x08,
                mask: None,
                name: "Composite".to_string(),
                divider: None,
                adjustment: None,
                blocks: Vec::new(),
                ranges: Vec::new(),
            }],
        };
        job.report(0.6, 1)?;
        // Photoshop stores the global light beside the effects that read it.
        let light = records
            .iter()
            .any(|record| {
                record
                    .blocks
                    .iter()
                    .any(|(key, _)| matches!(key, b"lfx2" | b"lfxs" | b"lmfx"))
            })
            .then_some(self.global_light);
        // The pattern table every written pattern overlay resolves through
        // (`psd_patterns`); empty — no block — without one.
        let fills = self.exported_fills();
        let fills: Vec<_> = fills
            .iter()
            .filter_map(crate::psd_fill_layer::Exported::pattern)
            .collect();
        // The comps' recorded styles after the layers' own: their patterns
        // ride in the same table (`psd_comp_limits::recorded_styles`).
        let comp_styles = crate::psd_comp_limits::recorded_styles(&self.comps, &self.comp_owners());
        let tail = match mode {
            PsdExportMode::Layers => crate::psd_patterns::tail_blocks_with(
                self.layers
                    .iter()
                    .filter_map(|l| l.renders_any_style())
                    .chain(comp_styles.iter()),
                &fills,
                false,
            ),
            PsdExportMode::Flattened => Vec::new(),
        };
        // Image resource 1065, the layer comps the records' `cmls` name
        // (`psd_comps`); the flattened mode writes no layer, so none.
        // …and, in both modes, the paths' resources (`psd_paths`).
        let mut extra = crate::psd_paths::resources(&self.paths.entries, (self.width, self.height));
        if matches!(mode, PsdExportMode::Layers) {
            extra.extend(crate::psd_comps::resource(&self.comps));
        }
        let bytes = if container == LayeredContainer::Tiff {
            // The flat picture is the TIFF's own strip, straight RGBA; the
            // records and resources ride in Photoshop's tags.
            let spots = self.layered_tiff_spots();
            let mut resources = image_resources(
                self.profile.bytes(),
                self.resolution,
                light,
                &self.color_samplers,
                &extra,
            );
            resources.extend_from_slice(&spots.resources);
            crate::tiff_layers::write_spots(
                self.width,
                self.height,
                8,
                3,
                self.profile.bytes(),
                self.resolution,
                &resources,
                &layer_info(&records, false)?,
                composite.as_raw(),
                &tail,
                &spots.planes,
            )?
        } else {
            write_psd(
                self.width,
                self.height,
                8,
                3,
                self.profile.bytes(),
                self.resolution,
                light,
                &self.color_samplers,
                &records,
                &composite_planes8(&composite),
                large,
                &tail,
                &self.psd_spot_planes(),
                &extra,
            )?
        };
        crate::dr_image::save_bytes_atomically(path, &bytes, job.range(0.8, 1.0))?;
        if let Some(control) = control {
            control.report(1.0, 5);
        }
        Ok(())
    }
}

/// A folder or closing record has no pixels of its own, but Photoshop still
/// expects the four colour/alpha channel slots, each an empty raw plane.
fn placeholder_channels() -> Vec<(i16, Vec<u8>)> {
    [-1i16, 0, 1, 2]
        .into_iter()
        .map(|id| (id, vec![0, 0]))
        .collect()
}

/// One pass over `runs` (a layer's `pixels` samples in plane order), split
/// into the record's channel order `[alpha, r, g, b]`, each plane sized once.
fn component_planes<'a>(pixels: usize, runs: impl Iterator<Item = &'a [[u8; 4]]>) -> [Vec<u8>; 4] {
    let mut planes: [Vec<u8>; 4] = std::array::from_fn(|_| Vec::with_capacity(pixels));
    for run in runs {
        for &[r, g, b, a] in run {
            planes[0].push(a);
            planes[1].push(r);
            planes[2].push(g);
            planes[3].push(b);
        }
    }
    planes
}

fn composite_channels(composite: &RgbaImage, large: bool) -> Vec<(i16, Vec<u8>)> {
    let (w, h) = composite.dimensions();
    [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)]
        .into_iter()
        .map(|(id, component)| {
            let samples: Vec<u8> = composite
                .as_raw()
                .chunks_exact(4)
                .map(|p| p[component])
                .collect();
            (id, encode_plane(&samples, w, h, large))
        })
        .collect()
}
