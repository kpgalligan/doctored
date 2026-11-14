//! A CMYK document's PSD, PSB and layered-TIFF export (#1 phase 3): the
//! inventory it hands the shared preflight, the record walk that writes its
//! tree as colour-mode-4 layer records, the composite planes and the plan.
//!
//! Every refusal, loss and observation comes from `psd_export_preflight`
//! (its `cmyk` inventory: five composite planes, the CMYK limitations), the
//! bytes from `psd_export_encode` and `tiff_layers`. The walk mirrors
//! `psd_export16::records` and the importer (`psd_import_cmyk`):
//!
//! * a raster → channels `-1` (alpha), `0`–`3` (C, M, Y, K stored `max −
//!   ink`) at the document's DECLARED depth (8-bit: `max − round(v / 257)`,
//!   exact for unedited samples) and `-2` for its mask, PackBits rows;
//! * a CMYK adjustment record → a pixel-less record with placeholder
//!   channels `-1`, `0`–`3` and the Photoshop body
//!   (`psd_adjustment::encode_cmyk`, Brightness/Contrast's `CgEd` after it)
//!   and its style blocks and Blend If ranges — never
//!   baked: a kind with no record refuses the Layers mode
//!   (`psd_export_preflight`);
//! * a group → its closing divider, its contents, then its folder record
//!   (`lsct` 1/2 with its key: `pass` for Pass Through), placeholder
//!   channels and its mask;
//! * on every record: the blend key (`blend_key`), opacity, clipping,
//!   visibility, the layer style — `lfx2` (`lfxs` on a group record, Pass
//!   Through included) with every colour as a `CMYC` ink
//!   (`cmyk_style::with_all_inks`, `FxColours::Cmyk`), Fill (`iOpa`), `infx`
//!   and `knko`, and the Blend If ranges in the file's five CMYK slots
//!   (`encode_ranges_for(.., true)`: Gray, Cyan, Magenta, Yellow, Black) — then
//!   locks (`lspf`) and label (`lclr`). A style Photoshop's dialog cannot
//!   hold refuses the Layers mode (`psd_export_preflight`), and the
//!   document's light (1037/1049) is declared when an `lfx2`, `lfxs` or
//!   `lmfx` is written;
//! * a pattern fill layer → Photoshop's `PtFl` record
//!   (`psd_fill_layer`), as `psd_export16` writes it: its rendered ink over
//!   `psd_fill_layer::record_rect`, channels `-1`, `0`–`3` and its `-2`
//!   mask, flags `0x18`, `fxrp` = its reference, and its tile in the
//!   pattern table as an RGB pattern record; a solid one → `SoCo`
//!   the same way, its `CMYC` the stored ink and its `fxrp` the usual rule;
//!   a gradient one → `GdFl`, its stops' `CMYC`, pixel-less at 16 bits;
//! * then, as the RGB writer: the comps' `lyid`/`shmd` and resource 1065,
//!   each raster-less vector mask as its `vmsk`, and record 0 as
//!   Photoshop's Background record when it is one (`psd_typed_exchange`).
//!
//! The LAYERED file's composite is five planes — the stack's composite
//! flattened onto paper (`on_paper`), stored inverted, and its alpha — under
//! a negative record count (merged alpha declared); a layered TIFF's strip
//! is the straight composite ink and alpha (Photometric 5, InkSet 1). The
//! FLATTENED mode is the CMYK save's own bytes: a zero-record colour-mode-4
//! PSD (`cmyk_io::cmyk_flat_psd`; a PSB at version 2) or the plain CMYK TIFF
//! (`cmyk_io::cmyk_encoded`).

use serde_json::{json, Value};

use crate::cmyk_composite::{codes, on_paper};
use crate::cmyk_document::CmykDepth;
use crate::cmyk_io::SaveFormat;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, pixel_count, BOOKKEEPING_BYTES};
use crate::psd_adjustment::encode_cmyk;
use crate::psd_export_encode::{
    encode_plane, image_resources, layer_info, write_psd, MaskBody, Record,
};
use crate::psd_export_preflight::{
    blend_key, cmyk_general_limitations, preflight, ExportAdjustment, ExportEntry, ExportInventory,
    ExportMask, ExportStyle, PsdExportMode, CLOSER_NAME,
};
use crate::psd_fill_layer::Exported;
use crate::psd_framing::LayeredContainer;
use crate::psd_layer_effects::{
    bake_reason, effect_block_bytes, effect_blocks, encode_fill, encode_fill_for,
    encode_ranges_for, knockout_blocks, mask_bake_reason, FxColours,
};
use crate::style::{GlobalLight, LayerStyle};

/// The canvas term of a LAYERED CMYK export's plan beside the file's worst
/// case: the composite's accumulator rows and five stored planes, the
/// layered TIFF's strip, each layer's contiguous samples and the file's
/// assembly copies. Declared before measuring;
/// `scripts/cmyk-admission/measure.py`'s `export-psd-layers-16` holds the
/// plan to ⌈1.25 × its measured peak⌉, and a miss raises this term: declared
/// 42, the first Release measurement (24 MP, max E 2.10 GB) missed at 0.964,
/// so it is 68, the smallest whole term that clears ⌈1.25 × max E⌉.
pub(crate) const LAYERED_SAVE_BYTES_PER_PIXEL: u64 = 68;

/// A placeholder record's channels: alpha and the four inks, each an empty
/// raw plane.
fn placeholder_channels() -> Vec<(i16, Vec<u8>)> {
    [-1i16, 0, 1, 2, 3]
        .into_iter()
        .map(|id| (id, vec![0, 0]))
        .collect()
}

/// The style one record writes (`psd_export16::merged_style`: the layer's
/// style and its Blend If as one), every slot given its ink under `policy`
/// (`cmyk_style::with_all_inks`); `None` for an unstyled record.
fn written_style(
    layer: &PrecisionDocumentLayer,
    policy: &crate::cmyk_document::CmykPolicy,
) -> Result<Option<LayerStyle>, String> {
    let Some(merged) = crate::psd_export16::merged_style(layer) else {
        return Ok(None);
    };
    crate::cmyk_style::with_all_inks(&merged, policy).map(Some)
}

/// Whether this writer writes `layer`'s knockout: its style is written, not
/// baked — the CMYK twin of `psd_export16::knockout_written16`, which
/// `psd_background` asks.
fn knockout_written(
    layer: &PrecisionDocumentLayer,
    policy: &crate::cmyk_document::CmykPolicy,
    light: GlobalLight,
) -> bool {
    export_style(layer, false, policy, light).is_some_and(|style| style.bake.is_none())
}

/// The record's style as the preflight sees it (`psd_export16`'s
/// `export_style16` on ink): its effects, Fill, Blend If channel, knockout
/// and interior flag, why it would need baking, and what its blocks cost.
/// A style whose inks cannot be resolved (`with_all_inks`' error, which
/// `blocks` returns at save time) takes that error as its bake reason, so the
/// report refuses as the save would.
fn export_style(
    layer: &PrecisionDocumentLayer,
    clip_base: bool,
    policy: &crate::cmyk_document::CmykPolicy,
    light: GlobalLight,
) -> Option<ExportStyle> {
    let (merged, ink_error) = match written_style(layer, policy) {
        Ok(style) => (style?, None),
        Err(error) => (crate::psd_export16::merged_style(layer)?, Some(error)),
    };
    let masked = layer.mask().is_some_and(|m| m.enabled());
    let (density, feather) = layer.mask().map_or((1.0, 0.0), |mask| {
        (mask.properties().density, mask.properties().feather)
    });
    // Only a style that writes effects or knockout makes the mask a reason
    // to bake; a Fill-only style leaves an unstorable mask to the preflight's
    // own density-and-feather sentence.
    let writes = merged.has_enabled_effects() || merged.knockout != crate::style::Knockout::None;
    let bake = ink_error.or_else(|| {
        layer.style().and_then(|style| {
            bake_reason(style).or_else(|| {
                writes
                    .then(|| mask_bake_reason(masked, density, feather))
                    .flatten()
            })
        })
    });
    let effects_bytes = if merged.has_enabled_effects() {
        effect_block_bytes(&merged, light, FxColours::Cmyk)
    } else {
        0
    };
    Some(ExportStyle {
        effects: merged
            .enabled_effects()
            .map(|effect| effect.kind().type_name().to_string())
            .collect(),
        fill_opacity: merged.fill_opacity,
        blend_if: merged.blend_if.map_or(Vec::new(), |ranges| {
            ranges.ranges().map(|r| r.channel).collect()
        }),
        knockout: merged.knockout,
        blend_interior: merged.blend_interior_as_group,
        masked,
        clip_base,
        bake,
        writes_effects: merged.has_enabled_effects(),
        block_bytes: effects_bytes + 12 + 4 + 2 * (12 + 4) + 40,
    })
}

/// The layer's fill when it is written as a `PtFl`, `SoCo` or `GdFl` record (module
/// doc): a CMYK raster whose description is a pattern or ink solid fill
/// (`psd_fill_layer::exported`). A style Photoshop cannot hold refuses the
/// Layers mode, so a written record's style is never baked.
fn exported_fill(layer: &PrecisionDocumentLayer) -> Option<Exported> {
    if layer.children().is_some() || !matches!(layer.content, Content::CmykRaster { .. }) {
        return None;
    }
    crate::psd_fill_layer::exported(layer.description.as_deref()?, true)
}

/// Every record's [`exported_fill`]; the pattern table takes the patterns.
fn exported_fills(layers: &[PrecisionDocumentLayer], out: &mut Vec<Exported>) {
    for layer in layers {
        out.extend(exported_fill(layer));
        if let Some(children) = layer.children() {
            exported_fills(children, out);
        }
    }
}

/// One entry per record, bottom-first, a folder's contents before it — the
/// `psd_export16::entries` order. `vector` holds the ids of the layers whose
/// live vector mask is written as pixels.
fn entries(
    layers: &[PrecisionDocumentLayer],
    vector: &[u64],
    policy: &crate::cmyk_document::CmykPolicy,
    light: GlobalLight,
    canvas: (u32, u32),
    out: &mut Vec<ExportEntry>,
) {
    for (index, layer) in layers.iter().enumerate() {
        if let Some(children) = layer.children() {
            entries(children, vector, policy, light, canvas, out);
        }
        let clip_base = layers[index + 1..]
            .iter()
            .take_while(|above| above.clipped())
            .any(|above| above.visible());
        // A pattern fill written as its `PtFl` record keeps its description;
        // its planes and offset are its record rectangle's (module doc).
        let bits = if policy.depth() == CmykDepth::Sixteen {
            16
        } else {
            8
        };
        let fill_rect = exported_fill(layer)
            .map(|fill| crate::psd_export16::fill_rect16(layer, canvas, &fill, bits));
        let planes = match (layer.cmyk_source(), fill_rect) {
            (_, Some([top, left, bottom, right])) => {
                vec![((right - left) as u64, (bottom - top) as u64); 5]
            }
            (Some(image), None) => {
                let (w, h) = image.dimensions();
                vec![(u64::from(w), u64::from(h)); 5]
            }
            (None, None) => vec![(0, 0); 5],
        };
        let adjustment = match &layer.content {
            Content::CmykAdjustment(state) => Some(ExportAdjustment {
                op: state.operation().to_string(),
                encoded: encode_cmyk(state.description()),
                inexact: None,
            }),
            _ => None,
        };
        out.push(ExportEntry {
            name: layer.name().to_string(),
            group: layer.children().is_some(),
            visible: layer.visible(),
            clipped: layer.clipped(),
            blend: layer.blend_mode(),
            opacity: layer.opacity(),
            offset: fill_rect.map_or(layer.offset(), |[top, left, _, _]| (left, top)),
            planes,
            mask: layer.mask().map(|mask| {
                let (w, h) = mask.plane().dimensions();
                ExportMask {
                    width: w,
                    height: h,
                    density: mask.properties().density,
                    feather: mask.properties().feather,
                    linked: mask.properties().linked,
                    vector: vector.contains(&layer.id),
                }
            }),
            adjustment,
            style: export_style(layer, clip_base, policy, light),
            // A type, shape or solid or gradient fill layer exports its
            // pixels, named on the loss line as in RGB (#1's artwork
            // batch); a pattern fill is its `PtFl` record.
            described: layer.description.is_some() && fill_rect.is_none(),
            retained_source: false,
            warp: false,
            locked: layer.locks() != 0,
            link_group: layer.link() != 0,
        });
    }
}

/// The record walk's per-depth sample writer: one u16 code as the file
/// stores it, at 8 bits `round(v / 257)`.
#[derive(Clone, Copy)]
pub(crate) struct Depth {
    pub(crate) eight: bool,
}

impl Depth {
    pub(crate) fn bytes(self) -> u32 {
        if self.eight {
            1
        } else {
            2
        }
    }

    pub(crate) fn push(self, out: &mut Vec<u8>, value: u16) {
        if self.eight {
            out.push(((u32::from(value) + 128) / 257) as u8);
        } else {
            out.extend_from_slice(&value.to_be_bytes());
        }
    }

    /// `max − v` at the file's depth: an ink as PSD stores it.
    pub(crate) fn push_inverted(self, out: &mut Vec<u8>, value: u16) {
        if self.eight {
            out.push(255 - ((u32::from(value) + 128) / 257) as u8);
        } else {
            out.extend_from_slice(&(65535 - value).to_be_bytes());
        }
    }
}

fn mask_channel(
    layer: &PrecisionDocumentLayer,
    depth: Depth,
    channels: &mut Vec<(i16, Vec<u8>)>,
    large: bool,
) -> Option<MaskBody> {
    let mask = layer.mask()?;
    let (w, h) = mask.plane().dimensions();
    let (mx, my) = mask.properties().offset;
    let mut bytes = Vec::with_capacity(w as usize * h as usize * depth.bytes() as usize);
    for sample in mask.plane().samples() {
        depth.push(&mut bytes, *sample);
    }
    channels.push((-2, encode_plane(&bytes, w * depth.bytes(), h, large)));
    Some(MaskBody {
        bounds: [
            my,
            mx,
            my.saturating_add(h as i32),
            mx.saturating_add(w as i32),
        ],
        outside: mask.properties().outside_coverage,
        disabled: !mask.enabled(),
    })
}

/// Photoshop's `lclr` block for a labelled record (`psd_export16`'s rule).
fn label_block(layer: &PrecisionDocumentLayer) -> Option<([u8; 4], Vec<u8>)> {
    (layer.label() != 0).then(|| (*b"lclr", vec![0, layer.label(), 0, 0, 0, 0, 0, 0]))
}

/// One record's blocks in Photoshop's order — its style (`lfx2` when an
/// effect is enabled, `lfxs` on a group record, then the record's `fxrp`,
/// `iOpa`, `infx`/`knko`; an unstyled pixel record
/// in a legacy-Fill mode writes `iOpa` at 255 alone), the `fxrp` of a record
/// whose style wrote no effects, then `lspf`, then `lclr` — and its blending
/// ranges (`psd_export16`'s order).
fn blocks(layer: &PrecisionDocumentLayer, walk: &Walk<'_>) -> Result<(Blocks, Vec<u8>), String> {
    let mut out = Vec::new();
    let mut ranges = Vec::new();
    let style = written_style(layer, walk.policy)?;
    let section = layer.children().is_some();
    // A PIXEL record (not a group section, not an adjustment record): the
    // one kind whose legacy-Fill modes always write `iOpa` (`encode_fill_for`).
    // A fill record takes an adjustment record's `iOpa`; a pattern's writes
    // its reference as `fxrp`, always (`psd_export16`'s rule).
    let fill_record = exported_fill(layer);
    let pixel = layer.children().is_none()
        && !matches!(layer.content, Content::CmykAdjustment(_))
        && fill_record.is_none();
    let pattern = fill_record.as_ref().and_then(Exported::pattern).is_some();
    let mut reference = Some(if pattern {
        Some(crate::psd_fill_layer::reference_block(layer.reference()))
    } else {
        crate::psd_pattern_fill::record_reference(style.as_ref(), layer.offset(), layer.reference())
    });
    if let Some(style) = style.as_ref() {
        if style.has_enabled_effects() {
            // A group record writes `lfxs`, Photoshop's form.
            out.extend(effect_blocks(style, walk.light, FxColours::Cmyk, section)?);
            // The record's reference point (`fxrp`).
            out.extend(reference.take().flatten());
        }
        let fill = if pixel {
            encode_fill_for(layer.blend_mode(), style.fill_opacity)
        } else {
            encode_fill(style.fill_opacity)
        };
        if let Some(body) = fill {
            out.push((*b"iOpa", body.to_vec()));
        }
        out.extend(knockout_blocks(style));
        if let Some(blend_if) = style.blend_if.filter(|b| !b.is_identity()) {
            ranges = encode_ranges_for(&blend_if, true);
        }
    } else if pixel {
        // An unstyled pixel record in a legacy-Fill mode writes `iOpa` at
        // 255 too (#2), where a styled record without effects does.
        if let Some(body) = encode_fill_for(layer.blend_mode(), 1.0) {
            out.push((*b"iOpa", body.to_vec()));
        }
    }
    out.extend(reference.flatten());
    out.extend(crate::psd_locks::block(layer.locks()));
    out.extend(label_block(layer));
    Ok((out, ranges))
}

/// One record's tagged blocks, `(key, body)` in file order.
type Blocks = Vec<([u8; 4], Vec<u8>)>;

/// What the record walk needs beside the layers.
struct Walk<'a> {
    policy: &'a crate::cmyk_document::CmykPolicy,
    light: GlobalLight,
    canvas: (u32, u32),
}

/// The complete record list in FILE order (module doc).
fn records(
    layers: &[PrecisionDocumentLayer],
    depth: Depth,
    large: bool,
    walk: &Walk<'_>,
    job: JobContext<'_>,
    out: &mut Vec<Record>,
) -> Result<(), String> {
    for layer in layers {
        job.check()?;
        let hidden = u8::from(!layer.visible()) << 1;
        let opacity = crate::opacity_byte::code(layer.opacity());
        if let Some(children) = layer.children() {
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
            records(children, depth, large, walk, job, out)?;
            let mut channels = placeholder_channels();
            let mask = mask_channel(layer, depth, &mut channels, large);
            let (blocks, ranges) = blocks(layer, walk)?;
            out.push(Record {
                bounds: [0; 4],
                channels,
                blend: blend_key(layer.blend_mode()),
                opacity,
                clipping: u8::from(layer.clipped()),
                flags: 0x18 | hidden,
                mask,
                name: layer.name().to_string(),
                divider: Some(if layer.open() { 1 } else { 2 }),
                adjustment: None,
                blocks,
                ranges,
            });
            continue;
        }
        if let Content::CmykAdjustment(state) = &layer.content {
            // Unreachable: the preflight refuses a kind with no record.
            let encoded = encode_cmyk(state.description())
                .ok_or("this CMYK adjustment layer has no Photoshop record")?;
            let mut channels = placeholder_channels();
            let mask = mask_channel(layer, depth, &mut channels, large);
            // Brightness/Contrast's `CgEd` directly after its `brit`.
            let mut written: Vec<_> = encoded.companion.into_iter().collect();
            let (own, ranges) = blocks(layer, walk)?;
            written.extend(own);
            out.push(Record {
                bounds: [0; 4],
                channels,
                blend: blend_key(layer.blend_mode()),
                opacity,
                clipping: u8::from(layer.clipped()),
                flags: 0x08 | hidden,
                mask,
                name: layer.name().to_string(),
                divider: None,
                adjustment: Some((encoded.key, encoded.body)),
                blocks: written,
                ranges,
            });
            continue;
        }
        let image = layer
            .cmyk_source()
            .ok_or("a CMYK document's layer holds no CMYK raster to write")?;
        let (w, h) = image.dimensions();
        let (ox, oy) = layer.offset();
        let samples = image.to_contiguous()?;
        let fill = exported_fill(layer);
        // Alpha, then the four inks inverted: ids -1, 0, 1, 2, 3. A pattern
        // fill's `PtFl` record (module doc) covers its record rectangle.
        let (bounds, [w, h], planes) = if let Some(fill) = &fill {
            let bits = if depth.eight { 8 } else { 16 };
            let rect = crate::psd_export16::fill_rect16(layer, walk.canvas, fill, bits);
            let (size, [a, c, m, y, k]) =
                crate::psd_fill_layer::planes(samples.into_iter(), (w, h), (ox, oy), rect, 0u16);
            let bytes = |plane: Vec<u16>, inverted: bool| {
                let mut out = Vec::with_capacity(plane.len() * depth.bytes() as usize);
                for v in plane {
                    if inverted {
                        depth.push_inverted(&mut out, v);
                    } else {
                        depth.push(&mut out, v);
                    }
                }
                out
            };
            let planes = [
                bytes(a, false),
                bytes(c, true),
                bytes(m, true),
                bytes(y, true),
                bytes(k, true),
            ];
            (rect, size, planes)
        } else {
            let count = samples.len() * depth.bytes() as usize;
            let mut planes: [Vec<u8>; 5] = std::array::from_fn(|_| Vec::with_capacity(count));
            for sample in &samples {
                depth.push(&mut planes[0], sample[4]);
                for c in 0..4 {
                    depth.push_inverted(&mut planes[c + 1], sample[c]);
                }
            }
            drop(samples);
            let bounds = [
                oy,
                ox,
                oy.saturating_add(h as i32),
                ox.saturating_add(w as i32),
            ];
            (bounds, [w, h], planes)
        };
        let mut channels: Vec<(i16, Vec<u8>)> = [-1i16, 0, 1, 2, 3]
            .into_iter()
            .zip(planes)
            .map(|(id, bytes)| (id, encode_plane(&bytes, w * depth.bytes(), h, large)))
            .collect();
        let mask = mask_channel(layer, depth, &mut channels, large);
        let (blocks, ranges) = blocks(layer, walk)?;
        out.push(Record {
            bounds,
            channels,
            blend: blend_key(layer.blend_mode()),
            opacity,
            clipping: u8::from(layer.clipped()),
            // Bit 4 on a fill record: its pixels do not decide the
            // appearance (`psd_fill_layer`).
            flags: if fill.is_some() { 0x18 } else { 0x08 } | hidden,
            mask,
            name: layer.name().to_string(),
            divider: None,
            adjustment: fill.map(|fill| crate::psd_fill_layer::encode(&fill, FxColours::Cmyk)),
            blocks,
            ranges,
        });
    }
    Ok(())
}

impl PrecisionDocument {
    /// This CMYK document as the preflight sees it, without a pixel.
    fn cmyk_export_inventory(&self) -> Result<ExportInventory, String> {
        let policy = self.require_cmyk_policy("A CMYK PSD export")?;
        let mut out = Vec::new();
        // The vector masks written as pixels: a raster-less one a PSD path
        // can hold is written as its `vmsk` (`psd_typed_exchange`).
        let vector = crate::psd_typed_exchange::baked_vector_masks(self);
        entries(
            self.root_layers(),
            &vector,
            policy,
            self.global_light,
            self.dimensions(),
            &mut out,
        );
        // The comps' recorded styles after the layers' own: their patterns
        // ride in the same table (`psd_comp_limits::recorded_styles`).
        let comp_styles = crate::psd_typed_exchange::comp_styles(self, &|layer| {
            knockout_written(layer, policy, self.global_light)
        });
        let mut styles = Vec::new();
        crate::psd_export16::record_styles(self.root_layers(), &mut styles);
        styles.extend(comp_styles.iter());
        let mut fills = Vec::new();
        exported_fills(self.root_layers(), &mut fills);
        let fills: Vec<_> = fills.iter().filter_map(Exported::pattern).collect();
        let spots = crate::psd_spot::spot_inventory(
            self.channels()
                .iter()
                .filter(|c| c.metadata().spot.is_some())
                .map(|c| c.name()),
        );
        Ok(ExportInventory {
            width: self.dimensions().0,
            height: self.dimensions().1,
            sample_bytes: u64::from(policy.depth().bits() / 8),
            cmyk: true,
            gray: false,
            lab: false,
            profile_bytes: policy.profile_bytes().len() as u64,
            // The pattern table (RGB pattern records, `psd_patterns`).
            tail_bytes: crate::psd_patterns::tail_bytes_with(styles, &fills),
            profile_name: policy.name().to_string(),
            resolution: self.resolution(),
            channels: self.channels().len(),
            spot_channels: spots.0,
            spot_name_units: spots.1,
            has_guides: !self.guides().is_empty() || self.ruler_origin() != (0.0, 0.0),
            sampler_sizes_lost: false,
            comps: self.layer_comps().len(),
            // The layered mode writes every comp (`psd_comps`). A comp's
            // style crosses as ink, as a record's own does, so one whose
            // inks cannot be resolved is refused with the codec's.
            comps_written: self.layer_comps().len(),
            comps_narrowed: crate::psd_typed_exchange::comps_narrowed(
                self,
                &|layer| knockout_written(layer, policy, self.global_light),
                &|style| {
                    crate::cmyk_style::with_all_inks(style, policy)
                        .map_or(true, |inked| bake_reason(&inked).is_some())
                },
            ),
            path_losses: crate::psd_paths::export_losses(self.paths(), self.dimensions()),
            has_metadata: !self.metadata().is_empty(),
            nondefault_global_light: self.global_light != GlobalLight::default(),
            // D13 on ink: a qualifying bottom is written as Photoshop's
            // Background record (`psd_background`), so only one that cannot
            // be one is said.
            background_unwritable: self
                .bottom_export(&|layer| knockout_written(layer, policy, self.global_light))
                == crate::psd_background::BottomExport::Unwritable,
            entries: out,
        })
    }

    /// The working memory of this export (`working_memory_plan_bytes`): the
    /// flattened mode is the CMYK save's own plan (a PSD's, or a TIFF's); the
    /// layered mode the file's worst case, [`LAYERED_SAVE_BYTES_PER_PIXEL`] a
    /// canvas pixel, the composite stack's scratch and bookkeeping.
    fn cmyk_psd_plan(
        &self,
        container: LayeredContainer,
        mode: PsdExportMode,
        worst_case: u64,
    ) -> Result<u64, String> {
        match (mode, container) {
            (PsdExportMode::Flattened, LayeredContainer::Tiff) => {
                self.cmyk_save_scratch_of(SaveFormat::Tiff)
            }
            (PsdExportMode::Flattened, _) => self.cmyk_psd_save_scratch_bytes(),
            (PsdExportMode::Layers, _) => {
                let (w, h) = self.dimensions();
                Ok(worst_case
                    .saturating_add(pixel_count(w, h)? as u64 * LAYERED_SAVE_BYTES_PER_PIXEL)
                    .saturating_add(self.cmyk_stack_scratch_bytes()?)
                    .saturating_add(BOOKKEEPING_BYTES))
            }
        }
    }

    /// The CMYK export report (`dr_precision_doc_psd_export_report`'s CMYK
    /// arm): the shared preflight's report with `"depth":"cmyk"`, the
    /// declared `bit_depth`, the CMYK limitations and the plan.
    pub(crate) fn cmyk_psd_report_json(
        &self,
        container: LayeredContainer,
        mode: PsdExportMode,
        file: u64,
        reopen: u64,
    ) -> Value {
        let inventory = match self.cmyk_export_inventory() {
            Ok(inventory) => inventory,
            Err(refusal) => {
                return json!({"format": container.name(), "container": container.name(),
                    "mode": mode.name(), "depth": "cmyk", "available": false,
                    "refusal": refusal, "losses": [], "observations": [],
                    "general_limitations": cmyk_general_limitations(container),
                    "layer_count": 0, "group_count": 0, "worst_case_bytes": 0,
                    "bake_scratch_bytes": 0})
            }
        };
        let report = preflight(&inventory, container, mode, file, reopen);
        let plan = self
            .cmyk_psd_plan(container, mode, report.worst_case_bytes)
            .ok();
        json!({
            "format": container.name(),
            "container": container.name(),
            "bake_scratch_bytes": 0,
            "mode": mode.name(),
            "depth": "cmyk",
            "bit_depth": inventory.sample_bytes * 8,
            "available": report.refusal.is_none(),
            "refusal": report.refusal,
            "losses": report.losses,
            "observations": report.observations,
            "general_limitations": cmyk_general_limitations(container),
            "layer_count": report.layer_count,
            "group_count": report.group_count,
            "worst_case_bytes": report.worst_case_bytes,
            "working_memory_plan_bytes": plan,
        })
    }

    /// The composite's planes (module doc): `layered` = a PSD's five stored
    /// planes (C, M, Y, K inverted on paper, then alpha); else a layered
    /// TIFF's interleaved straight strip of five big-endian samples.
    fn cmyk_composite_planes(
        &self,
        depth: Depth,
        tiff: bool,
        job: JobContext<'_>,
    ) -> Result<Vec<Vec<u8>>, String> {
        let (w, h) = self.dimensions();
        let count = pixel_count(w, h)? * depth.bytes() as usize;
        let mut planes: Vec<Vec<u8>> = if tiff {
            vec![crate::precision_image::reserved_vec(count * 5)?]
        } else {
            (0..5)
                .map(|_| crate::precision_image::reserved_vec(count))
                .collect::<Result<_, _>>()?
        };
        let mut stack = self.cmyk_stack(None, job)?;
        let mut acc = vec![[0f32; 5]; w as usize];
        for y in 0..h {
            job.check()?;
            stack.row(i64::from(y), 0, &mut acc);
            for ink in &acc {
                if tiff {
                    for v in codes(*ink) {
                        depth.push(&mut planes[0], v);
                    }
                    continue;
                }
                let paper = on_paper(*ink);
                let flat = codes([paper[0], paper[1], paper[2], paper[3], ink[4]]);
                for c in 0..4 {
                    depth.push_inverted(&mut planes[c], flat[c]);
                }
                depth.push(&mut planes[4], flat[4]);
            }
        }
        Ok(planes)
    }

    /// Writes this CMYK document as a PSD, PSB or layered TIFF (module doc).
    /// Pure, and atomic through the shared temp-file rename; the preflight's
    /// refusal is this call's error.
    pub(crate) fn save_cmyk_psd(
        &self,
        path: &str,
        container: LayeredContainer,
        mode: PsdExportMode,
        limit: u64,
        other: u64,
        job: JobContext<'_>,
    ) -> Result<(), String> {
        let policy = self.require_cmyk_policy("A CMYK PSD export")?.clone();
        let inventory = self.cmyk_export_inventory()?;
        let report = preflight(
            &inventory,
            container,
            mode,
            container.max_file_bytes(),
            crate::psd_layer_pixels::import_envelope_bytes(),
        );
        if let Some(refusal) = report.refusal {
            return Err(refusal);
        }
        let plan = self.cmyk_psd_plan(container, mode, report.worst_case_bytes)?;
        admit(self.retained_bytes(), plan, other, limit)?;
        // The writer's own stages report inside the save's first 80 %; the
        // file's write is the rest.
        let write = job.range(0.8, 1.0);
        let job = job.range(0.0, 0.8);
        let large = container.large();
        let depth = Depth {
            eight: policy.depth() == CmykDepth::Eight,
        };
        let (w, h) = self.dimensions();
        let bytes = match (mode, container) {
            (PsdExportMode::Flattened, LayeredContainer::Tiff) => self.cmyk_encoded(
                &crate::cmyk_io::SaveRequest {
                    format: crate::cmyk_io::SaveFormat::Tiff,
                    embed_profile: true,
                    strip_metadata: false,
                },
                job,
            )?,
            (PsdExportMode::Flattened, _) => self.cmyk_flat_psd(large, job)?,
            (PsdExportMode::Layers, _) => {
                let mut out = Vec::new();
                let walk = Walk {
                    policy: &policy,
                    light: self.global_light,
                    canvas: (w, h),
                };
                records(self.root_layers(), depth, large, &walk, job, &mut out)?;
                // The comps' `lyid`/`shmd`, the raster-less vector masks'
                // `vmsk` and the Background record (`psd_typed_exchange`).
                crate::psd_typed_exchange::stamp(
                    self,
                    &mut out,
                    &|layer| knockout_written(layer, &policy, self.global_light),
                    // A comp's style with every colour as ink, as a record's
                    // own (`cmyk_style::with_all_inks`).
                    &|style| {
                        let inked = crate::cmyk_style::with_all_inks(style, &policy).ok()?;
                        crate::psd_comps::effect_style_items(
                            &inked,
                            self.global_light,
                            crate::psd_layer_effects::FxColours::Cmyk,
                        )
                    },
                );
                let extra = crate::psd_typed_exchange::resources(self, true);
                // The pattern table every written pattern overlay resolves
                // through (`psd_patterns`: RGB pattern records, `Patt` at 8
                // bits and `Pat2` at 16); no block without one.
                // The comps' recorded styles after the layers' own: their patterns
                // ride in the same table (`psd_comp_limits::recorded_styles`).
                let comp_styles = crate::psd_typed_exchange::comp_styles(self, &|layer| {
                    knockout_written(layer, &policy, self.global_light)
                });
                let mut styles = Vec::new();
                crate::psd_export16::record_styles(self.root_layers(), &mut styles);
                styles.extend(comp_styles.iter());
                let mut fills = Vec::new();
                exported_fills(self.root_layers(), &mut fills);
                let fills: Vec<_> = fills.iter().filter_map(Exported::pattern).collect();
                let tail = crate::psd_patterns::tail_blocks_with(styles, &fills, !depth.eight);
                // The document's light, declared only when some record
                // writes effects (`psd_export16`'s rule).
                let light = out
                    .iter()
                    .any(|record| {
                        record
                            .blocks
                            .iter()
                            .any(|(key, _)| matches!(key, b"lfx2" | b"lfxs" | b"lmfx"))
                    })
                    .then_some(self.global_light);
                let tiff = container == LayeredContainer::Tiff;
                let planes = self.cmyk_composite_planes(depth, tiff, job)?;
                let bits = (depth.bytes() * 8) as u16;
                if tiff {
                    let spots = self.layered_tiff_spots();
                    let mut resources = image_resources(
                        policy.profile_bytes(),
                        self.resolution(),
                        light,
                        &[],
                        &extra,
                    );
                    resources.extend_from_slice(&spots.resources);
                    crate::tiff_layers::write_spots(
                        w,
                        h,
                        bits,
                        4,
                        policy.profile_bytes(),
                        self.resolution(),
                        &resources,
                        &layer_info(&out, false)?,
                        &planes[0],
                        &tail,
                        &spots.planes,
                    )?
                } else {
                    write_psd(
                        w,
                        h,
                        bits,
                        4,
                        policy.profile_bytes(),
                        self.resolution(),
                        light,
                        &[],
                        &out,
                        &planes,
                        large,
                        &tail,
                        &self.psd_spot_planes(depth.eight),
                        &extra,
                    )?
                }
            }
        };
        job.check()?;
        crate::dr_image::save_bytes_atomically(path, &bytes, write)
    }
}
