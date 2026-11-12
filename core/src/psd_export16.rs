//! The 16-bit document's PSD export: the inventory it hands the shared
//! preflight, and the record walk that turns its nested typed stack into
//! classic PSD layer records at 16 bits. Every refusal, loss and observation
//! comes from `psd_export_preflight`, the bytes from `psd_export_encode`; a
//! typed document is never written as 8-bit.
//!
//! `psd_export16` writes the ranges as PSD blending ranges (since 10C-2 slice 3, `fc70079`).
//!
//! `psd_export16` bakes a styled raster through `bake_style16` exactly as the 8-bit export does and
//! builds its `ExportStyle` directly so the Blend If clause reads the layer's field.
//!
//! A pattern fill layer whose style is not baked is written as Photoshop's `PtFl` record
//! (`psd_fill_layer`), exactly as the 8-bit walk writes it: its rendered u16
//! pixels over `psd_fill_layer::record_rect`, its `-2` mask, flags `0x18`, its tile in `Pat2`;
//! a solid color fill layer is Photoshop's `SoCo` the same way, its colour the document's
//! fractional numbers. A gradient fill layer is Photoshop's `GdFl` in Photoshop's
//! 16-bit form: PIXEL-LESS (bounds 0, channels -1, 0, 1, 2 empty, its `-2` mask), flags `0x18`,
//! its stops in the document's fractional numbers.
//!
//! A 32-bit float document takes the SAME inventory (at 4 bytes a
//! sample) and the SAME record walk (`Walk::float`): its rasters and masks are
//! written as Raw f32 planes by `psd_export_float`, which owns its save. A
//! solid color fill is Photoshop's 32-bit `SoCo`: the same
//! fill arm at 32 bits, PIXEL-LESS (`Exported::pixel_less(32)`: bounds 0,
//! channels -1, 0, 1, 2 empty Raw, its `-2` mask as Raw f32), flags `0x18`,
//! its colour and every effect colour the document's linear light × 255
//! (`Walk::colours`' `FxColours::Float`). A gradient or pattern fill, a stamp
//! and a bake refuse at float with that module's `layers_refusal` sentence
//! (the report refuses first, so they are never reached).
//!
//! A Grayscale document (#1 phase 3b) takes the SAME inventory (`gray`: one
//! composite plane, its Gray profile) and the SAME record walk
//! (`Walk::gray`): every raster, stamp and bake is written from its R plane
//! (R = G = B) and alpha, channels `-1` and `0` (`psd_export_gray::
//! channels_gray`), its masks and placeholders at the document's DECLARED
//! depth, its effect colours as `Grsc` (`FxColours::Gray`); a fill arm
//! refuses. `psd_export_gray` owns its report and save, and moves the walk's
//! records to Photoshop's own gray form. A 32-bit Gray walk (`float` and
//! `gray`, phase 3c) writes R and alpha as Raw f32
//! (`psd_export_gray32::channels_gray32`) and its masks as the float walk's.
//!
//! A Lab Color document (#1 Lab phase 3b) takes the SAME inventory (`lab`:
//! three composite planes, no profile) and the SAME record walk, decided by
//! the walk's document (`walk.document.lab_policy()`, no `Walk` field): every
//! raster and bake is written as L, a, b and alpha at the document's DECLARED
//! depth (`psd_export_lab::channels_lab`, ids `-1`, `0`, `1`, `2`, by
//! `lab_encode::psd_sample`), its masks at that depth, its effect colours as
//! `LbCl` (`FxColours::Lab`). An adjustment layer is written as its STAMP
//! (`entries` decides by `lab_document::is_lab_twin`): no Lab adjustment
//! record was ever observed (`psd_import_lab`'s H-L). A fill layer (#1
//! Lab phase 6a-i) is written as its rendered codes, an ordinary raster:
//! [`exported_fill16`] answers `None` on the twin, so the inventory, the
//! pattern table and the walk agree, and preflight reports the description's
//! loss (Lab `SoCo`/`GdFl`/`PtFl` are unmeasured).
//! `psd_export_lab` owns its report and save, and moves the walk's records
//! to Photoshop's own Lab form.

use std::sync::Arc;

use crate::icc::IccProfile;
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::{admit, PrecisionImage, PrecisionStorage};
use crate::precision_style_render::{bake_style16, style_extent16_of, StyleEnv};
use crate::psd_adjustment;
use crate::psd_export_bake::inexact_group16;
use crate::psd_export_cmyk::Depth;
use crate::psd_export_encode::{
    composite_planes16, encode_plane, image_resources, layer_info, write_psd, MaskBody, Record,
};
use crate::psd_export_preflight::{
    bake_inexact, blend_key, mask_bake_reason, preflight, stamps, ExportAdjustment, ExportEntry,
    ExportInventory, ExportMask, ExportStyle, PsdExportMode, PsdExportReport, CLOSER_NAME,
    UNKNOWN_MODE,
};
use crate::psd_export_rasterize::style_block_bytes;
use crate::psd_framing::LayeredContainer;
use crate::psd_layer_effects::{
    bake_reason, effect_blocks, encode_fill, encode_fill_for, encode_ranges, knockout_blocks,
};
use crate::psd_layer_pixels::import_envelope_bytes;
use crate::style::GlobalLight;
use crate::style::LayerStyle;
use crate::style_cache::RenderCache;
use serde_json::{json, Value};

/// The one `LayerStyle` a typed layer's two fields describe: the effects and
/// fill opacity of its style, with its own Blend If put back — the shape
/// `psd_layer_effects` writes and the byte document already holds.
pub(crate) fn merged_style(layer: &PrecisionDocumentLayer) -> Option<LayerStyle> {
    if layer.style().is_none() && layer.blend_if().is_none() {
        return None;
    }
    Some(LayerStyle {
        fill_opacity: layer.style().map_or(1.0, |style| style.fill_opacity),
        blend_if: layer.blend_if(),
        // Both knockout keys are CARRIED since 12E-3b (`precision_style`) and
        // WRITTEN since 12E-3c, so the typed export puts them in the merged
        // style the one codec reads.
        knockout: layer
            .style()
            .map_or(crate::style::Knockout::None, |style| style.knockout),
        blend_interior_as_group: layer
            .style()
            .is_some_and(|style| style.blend_interior_as_group),
        effects: layer
            .style()
            .map(|style| style.effects.clone())
            .unwrap_or_default(),
        cache: RenderCache::default(),
    })
}

/// Whether this writer writes `layer`'s knockout: its style is written, not
/// baked (a BAKED style writes no `knko`, so its file punches nothing in
/// Photoshop). The typed writers' answer to `psd_background`'s "a written
/// knockout reaches the bottom", every typed writer's but CMYK's.
pub(crate) fn knockout_written16(layer: &PrecisionDocumentLayer) -> bool {
    export_style16(layer, false).is_some_and(|style| style.bake.is_none())
}

/// What one typed entry's style costs the export, or `None` for an unstyled
/// layer with no Blend If. Built DIRECTLY rather than through
/// `psd_export_rasterize::export_style`, whose Blend If reads `style.blend_if`
/// — always `None` on a typed style, whose Blend If lives on the layer.
/// `clip_base` is the caller's structural answer.
pub(crate) fn export_style16(
    layer: &PrecisionDocumentLayer,
    clip_base: bool,
) -> Option<ExportStyle> {
    let merged = merged_style(layer)?;
    let masked = layer.mask().is_some_and(|m| m.enabled());
    let (density, feather) = layer.mask().map_or((1.0, 0.0), |mask| {
        (mask.properties().density, mask.properties().feather)
    });
    Some(ExportStyle {
        effects: merged
            .enabled_effects()
            .map(|effect| effect.kind().type_name().to_string())
            .collect(),
        fill_opacity: merged.fill_opacity,
        // A typed Blend If is already `None` for an identity, so this is the
        // byte `filter(|r| !r.is_identity())`.
        blend_if: layer.blend_if().map_or(Vec::new(), |ranges| {
            ranges.ranges().map(|r| r.channel).collect()
        }),
        knockout: merged.knockout,
        blend_interior: merged.blend_interior_as_group,
        masked,
        clip_base,
        // Only EFFECTS are ever baked; a mask beside a Blend If alone shapes
        // nothing and stays an ordinary written mask.
        bake: layer.style().and_then(|style| {
            bake_reason(style).or_else(|| mask_bake_reason(masked, density, feather))
        }),
        writes_effects: merged.has_enabled_effects(),
        block_bytes: style_block_bytes(&merged),
    })
}

/// The layer's fill when it is written as a `PtFl` or `SoCo` record (module
/// doc): a raster whose description is a pattern or solid fill and whose
/// style is not baked. `None` on a Lab document's twin (module doc: its fill
/// is written as its rendered codes).
pub(crate) fn exported_fill16(
    layer: &PrecisionDocumentLayer,
) -> Option<crate::psd_fill_layer::Exported> {
    if layer.children().is_some()
        || !matches!(layer.content, Content::Raster { .. })
        || layer
            .source()
            .is_some_and(|source| crate::lab_document::is_lab_twin(source.profile()))
        || export_style16(layer, false).is_some_and(|style| style.bake.is_some())
    {
        return None;
    }
    crate::psd_fill_layer::exported(layer.description.as_deref()?, false)
}

/// A written fill record's rectangle (top, left, bottom, right):
/// `psd_fill_layer::record_rect` over the layer's mask, or `[0; 4]` for a
/// record written pixel-less at `depth` bits (`fill.pixel_less(depth)`: a
/// 16-bit gradient, every 32-bit fill); the CMYK walk's too
/// (`psd_export_cmyk`).
pub(crate) fn fill_rect16(
    layer: &PrecisionDocumentLayer,
    canvas: (u32, u32),
    fill: &crate::psd_fill_layer::Exported,
    depth: u32,
) -> [i32; 4] {
    if fill.pixel_less(depth) {
        return [0; 4];
    }
    crate::psd_fill_layer::record_rect(
        canvas,
        layer.mask().map(|mask| {
            let (w, h) = mask.plane().dimensions();
            let (mx, my) = mask.properties().offset;
            (
                [
                    my,
                    mx,
                    my.saturating_add(h as i32),
                    mx.saturating_add(w as i32),
                ],
                mask.properties().outside_coverage,
                mask.enabled(),
            )
        }),
    )
}

/// Every record's [`exported_fill16`] in the tree: the pattern table takes
/// their `Exported::pattern`s, and a wide solid charges an encoder.
pub(crate) fn exported_fills16(
    layers: &[PrecisionDocumentLayer],
    out: &mut Vec<crate::psd_fill_layer::Exported>,
) {
    for layer in layers {
        out.extend(exported_fill16(layer));
        if let Some(children) = layer.children() {
            exported_fills16(children, out);
        }
    }
}

/// One entry per record, in the layered mode's own bottom-first order:
/// a folder's contents precede the folder, exactly as the importer builds them.
/// `inexact` names the nearest enclosing group a baked adjustment cannot
/// follow exactly (`psd_export_bake::inexact_group16`); `depth` is the
/// file's bits per sample, which decides a fill record's rectangle.
#[allow(clippy::too_many_arguments)]
fn entries(
    layers: &[PrecisionDocumentLayer],
    profile: &std::sync::Arc<IccProfile>,
    canvas: (u32, u32),
    light: GlobalLight,
    inexact: Option<&str>,
    vector: &[u64],
    depth: u32,
    out: &mut Vec<ExportEntry>,
) {
    for (index, layer) in layers.iter().enumerate() {
        if let Some(children) = layer.children() {
            let inner = if inexact_group16(layer) {
                Some(layer.name())
            } else {
                inexact
            };
            entries(children, profile, canvas, light, inner, vector, depth, out);
        }
        // A visible entry directly above at the same depth, clipped to it —
        // the run a styled base's rasterized effects end up underneath.
        let clip_base = layers[index + 1..]
            .iter()
            .take_while(|above| above.clipped())
            .any(|above| above.visible());
        let group = layer.children().is_some();
        // An adjustment's style travels on its record.
        let style = export_style16(layer, clip_base);
        let baked = style.as_ref().is_some_and(|style| style.bake.is_some());
        let adjustment = match &layer.content {
            // `state.description()` carries the typed `evaluation` key,
            // which the codec ignores. A Lab document's has no record: its
            // stamp is written (module doc).
            Content::Adjustment(state) => {
                let encoded = if crate::lab_document::is_lab_twin(profile) {
                    None
                } else {
                    psd_adjustment::encode(state.description())
                };
                Some(ExportAdjustment {
                    op: state.kernel.operation().to_string(),
                    inexact: (encoded.is_none() || baked)
                        .then(|| bake_inexact(layer.clipped(), inexact))
                        .flatten(),
                    encoded,
                })
            }
            _ => None,
        };
        // A BAKED style's record is its bake, so the planes and offset the
        // preflight forecasts are the grown rectangle's; a PRESERVED one
        // keeps the layer's own.
        let extent = (!group && adjustment.is_none() && baked)
            .then(|| style_extent16_of(layer, profile, canvas, light))
            .flatten();
        // An adjustment with no record, or with a style Photoshop cannot
        // hold, is BAKED: a canvas-sized Normal pixel layer at 100 % with no
        // mask of its own.
        let stamped = stamps(adjustment.as_ref(), style.as_ref());
        // A pattern fill written as its `PtFl` record keeps its description;
        // its planes are its record rectangle's (module doc).
        let fill = exported_fill16(layer);
        let fill_record = fill.is_some();
        let fill_rect = fill
            .as_ref()
            .map(|fill| fill_rect16(layer, canvas, fill, depth));
        let planes = match (layer.source(), extent) {
            _ if stamped => vec![(u64::from(canvas.0), u64::from(canvas.1)); 4],
            _ if fill_rect.is_some() => {
                let [top, left, bottom, right] = fill_rect.unwrap_or_default();
                vec![((right - left) as u64, (bottom - top) as u64); 4]
            }
            (Some(_), Some((x0, y0, x1, y1))) => {
                vec![((x1 - x0).max(0) as u64, (y1 - y0).max(0) as u64); 4]
            }
            (Some(image), None) => {
                let (w, h) = image.dimensions();
                vec![(u64::from(w), u64::from(h)); 4]
            }
            (None, _) => vec![(0, 0); 4],
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
            None => layer.offset(),
        };
        // An enabled mask shapes the effects and is inside the bake; a
        // disabled one — and every mask beside a PRESERVED style — is written
        // exactly as it is today.
        let baked_mask = baked && style.as_ref().is_some_and(|style| style.masked) && !group;
        out.push(ExportEntry {
            name: layer.name().to_string(),
            group,
            visible: layer.visible(),
            clipped: layer.clipped(),
            blend: if stamped {
                crate::blend::BlendMode::Normal
            } else {
                layer.blend_mode()
            },
            opacity: if stamped { 1.0 } else { layer.opacity() },
            offset: if stamped { (0, 0) } else { offset },
            planes,
            mask: layer
                .mask()
                .filter(|_| !baked_mask && !stamped)
                .map(|mask| {
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
            style,
            described: layer.is_described() && !fill_record,
            retained_source: layer.has_retained_source(),
            warp: layer.effective_warp(profile).is_some(),
            locked: layer.locks() != 0,
            link_group: layer.link() != 0,
        });
    }
}

impl PrecisionDocumentLayer {
    fn is_described(&self) -> bool {
        self.description.is_some()
    }
    fn has_retained_source(&self) -> bool {
        self.retained_source.is_some()
    }
}

/// A folder or closing record has no pixels of its own, but Photoshop still
/// expects the four colour/alpha channel slots, each an empty raw plane — a
/// Grayscale file's two (`-1`, `0`).
fn placeholder_channels(gray: bool) -> Vec<(i16, Vec<u8>)> {
    let ids: &[i16] = if gray { &[-1, 0] } else { &[-1, 0, 1, 2] };
    ids.iter().map(|id| (*id, vec![0, 0])).collect()
}

/// An RGBA16 image's samples in plane order, read from its runs; `None` for
/// f32 storage.
fn samples16(image: &PrecisionImage) -> Option<impl Iterator<Item = &[u16; 4]> + '_> {
    image.runs_rgba16().map(Iterator::flatten)
}

/// An RGBA16 image's alpha, red, green and blue channels as [`plane16`]
/// encodes them, ids -1, 0, 1, 2 in that order, from ONE walk of its
/// samples; `None` for f32 storage.
pub(crate) fn channels16(image: &PrecisionImage, large: bool) -> Option<Vec<(i16, Vec<u8>)>> {
    let (width, height) = image.dimensions();
    let count = width as usize * height as usize * 2;
    let mut planes: [Vec<u8>; 4] = std::array::from_fn(|_| Vec::with_capacity(count));
    samples16(image)?.for_each(|pixel| {
        let [a, r, g, b] = &mut planes;
        a.extend_from_slice(&pixel[3].to_be_bytes());
        r.extend_from_slice(&pixel[0].to_be_bytes());
        g.extend_from_slice(&pixel[1].to_be_bytes());
        b.extend_from_slice(&pixel[2].to_be_bytes());
    });
    Some(
        [-1i16, 0, 1, 2]
            .into_iter()
            .zip(planes)
            .map(|(id, bytes)| (id, encode_plane(&bytes, width * 2, height, large)))
            .collect(),
    )
}

/// One 16-bit plane as PSD stores it: big-endian samples, PackBits rows of
/// `width * 2` BYTES.
fn plane16(samples: impl Iterator<Item = u16>, width: u32, height: u32, large: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(width as usize * height as usize * 2);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_be_bytes());
    }
    encode_plane(&bytes, width * 2, height, large)
}

/// The record's `-2` channel and mask body: a 16-bit PackBits plane, or at
/// `float` a Raw plane of f32 coverage (`psd_export_float::mask_plane32`),
/// or at a Grayscale file's `depth` (#1 phase 3b) a PackBits plane of its
/// samples at that depth.
fn mask_channel(
    layer: &PrecisionDocumentLayer,
    channels: &mut Vec<(i16, Vec<u8>)>,
    large: bool,
    float: bool,
    depth: Option<Depth>,
) -> Option<MaskBody> {
    let mask = layer.mask()?;
    let (w, h) = mask.plane().dimensions();
    let (mx, my) = mask.properties().offset;
    channels.push((
        -2,
        if float {
            crate::psd_export_float::mask_plane32(mask.plane().samples())
        } else if let Some(depth) = depth {
            let mut bytes = Vec::with_capacity(w as usize * h as usize * depth.bytes() as usize);
            for sample in mask.plane().samples() {
                depth.push(&mut bytes, *sample);
            }
            encode_plane(&bytes, w * depth.bytes(), h, large)
        } else {
            plane16(mask.plane().samples().iter().copied(), w, h, large)
        },
    ));
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

/// Every record's stored style in the tree, for the pattern table.
pub(crate) fn record_styles<'a>(
    layers: &'a [PrecisionDocumentLayer],
    out: &mut Vec<&'a LayerStyle>,
) {
    for layer in layers {
        if let Some(style) = layer.style() {
            out.push(&**style);
        }
        if let Some(children) = layer.children() {
            record_styles(children, out);
        }
    }
}

/// Photoshop's `lclr` block for a labelled record — u16 big-endian code then
/// six reserved zero bytes — and nothing for an unlabelled one, so an
/// unlabelled document's bytes are exactly what they were (`doc_label`).
fn label_block(layer: &PrecisionDocumentLayer) -> Option<([u8; 4], Vec<u8>)> {
    (layer.label() != 0).then(|| (*b"lclr", vec![0, layer.label(), 0, 0, 0, 0, 0, 0]))
}

/// One record's additional layer blocks and its blending-ranges section.
type StyleBlocks = (Vec<([u8; 4], Vec<u8>)>, Vec<u8>);

/// The preserved style's additional blocks and blending ranges for one
/// record: the same answer `export_style16` gave the preflight, so the report
/// and the bytes can never disagree. An `lfx2` (a group's `lfxs`) is written only when the style
/// holds an ENABLED effect — fill opacity and Blend If are their own
/// constructs, and a descriptor with no enabled effect would reopen as a bare
/// marker. Every record (raster or group) also carries its reference point
/// (`fxrp`, `psd_pattern_fill::record_reference`) when it has one: after the
/// effects, or after the other blocks when there are none. `colours` is how
/// an effect colour crosses (`Walk::colours`).
fn style_blocks(
    layer: &PrecisionDocumentLayer,
    env: &StyleEnv<'_>,
    colours: crate::psd_layer_effects::FxColours<'_>,
) -> Result<StyleBlocks, String> {
    // A pixel record in a legacy-Fill mode writes `iOpa` even at 255
    // (`encode_fill_for`, #2); a group section keeps `encode_fill`'s.
    // An adjustment record keeps `encode_fill`'s, as the 8-bit walk does.
    // A pattern or solid fill record (module doc) takes an adjustment
    // record's `iOpa`; a pattern's writes its reference as `fxrp`, always.
    let fill = exported_fill16(layer);
    let fill_record = fill.is_some();
    let pattern_record = fill
        .as_ref()
        .and_then(crate::psd_fill_layer::Exported::pattern)
        .is_some();
    let fill_block = |fill: f32| match (layer.children(), &layer.content) {
        (None, Content::Adjustment(_)) | (Some(_), _) => encode_fill(fill),
        (None, _) if fill_record => encode_fill(fill),
        (None, _) => encode_fill_for(layer.blend_mode(), fill),
    };
    let reference_of = |style: Option<&LayerStyle>| {
        if pattern_record {
            Some(crate::psd_fill_layer::reference_block(layer.reference()))
        } else {
            crate::psd_pattern_fill::record_reference(style, layer.offset(), layer.reference())
        }
    };
    let unstyled = || {
        let mut blocks: Vec<([u8; 4], Vec<u8>)> = fill_block(1.0)
            .map(|body| (*b"iOpa", body.to_vec()))
            .into_iter()
            .collect();
        blocks.extend(reference_of(None));
        Ok((blocks, Vec::new()))
    };
    let Some(export) = export_style16(layer, false) else {
        return unstyled();
    };
    if export.bake.is_some() {
        return unstyled();
    }
    let Some(merged) = merged_style(layer) else {
        return unstyled();
    };
    let mut blocks = Vec::new();
    let mut reference = Some(reference_of(Some(&merged)));
    if merged.has_enabled_effects() {
        // A group record writes `lfxs`, Photoshop's form.
        blocks.extend(effect_blocks(
            &merged,
            env.light,
            colours,
            layer.children().is_some(),
        )?);
        // The record's reference point (`fxrp`).
        blocks.extend(reference.take().flatten());
    }
    if let Some(body) = fill_block(merged.fill_opacity) {
        blocks.push((*b"iOpa", body.to_vec()));
    }
    // Photoshop's own blending-options records, after `iOpa` and before
    // `lclr`, and only when they are not at their defaults.
    blocks.extend(knockout_blocks(&merged));
    blocks.extend(reference.flatten());
    let ranges = merged
        .blend_if
        .filter(|ranges| !ranges.is_identity())
        .map_or(Vec::new(), |ranges| encode_ranges(&ranges).to_vec());
    Ok((blocks, ranges))
}

/// What the typed record walk needs beside the layers: the document the
/// adjustment stamps render from, with the caller's working limit and other
/// retained bytes for their admission, the container's row-count width,
/// whether this is a 32-bit float document's walk (module doc), and a
/// Grayscale document's declared depth and policy (#1 phase 3b).
pub(crate) struct Walk<'a> {
    pub document: &'a PrecisionDocument,
    pub limit: u64,
    pub other: u64,
    pub large: bool,
    pub float: bool,
    pub gray: Option<(Depth, &'a crate::gray_document::GrayPolicy)>,
}

impl Walk<'_> {
    /// A Lab Color document's declared depth (#1 Lab phase 3b, module
    /// doc), read off the walk's document; `None` for every other.
    fn lab_depth(&self) -> Option<Depth> {
        self.document.lab_policy().map(|policy| Depth {
            eight: policy.depth() == crate::lab_document::LabDepth::Eight,
        })
    }

    /// How this walk's effect and fill colours cross: `Grsc` on Gray, `LbCl`
    /// on Lab, a float RGB walk's linear light × 255 (`Float`, from
    /// the env's `float_style`), else the 16-bit RGB numbers.
    fn colours<'b>(&'b self, env: &StyleEnv<'b>) -> crate::psd_layer_effects::FxColours<'b> {
        match (self.gray, env.float_style) {
            (Some((_, policy)), _) => crate::psd_layer_effects::FxColours::Gray(policy),
            (None, _) if self.document.is_lab() => crate::psd_layer_effects::FxColours::Lab,
            (None, Some(style)) if self.float => crate::psd_layer_effects::FxColours::Float(style),
            (None, _) => crate::psd_layer_effects::FxColours::Rgb16(env.colors),
        }
    }

    /// One written image's channels: a Grayscale walk's R and alpha at its
    /// depth, a Lab walk's L, a, b and alpha at its depth, else RGBA16.
    fn channels(&self, image: &PrecisionImage) -> Option<Vec<(i16, Vec<u8>)>> {
        match (self.gray, self.lab_depth()) {
            (Some((depth, _)), _) => {
                crate::psd_export_gray::channels_gray(image, depth, self.large)
            }
            (None, Some(depth)) => crate::psd_export_lab::channels_lab(image, depth, self.large),
            (None, None) => channels16(image, self.large),
        }
    }
}

/// The sentence a float walk's fill, stamp or bake arm refuses with: the
/// record's own `layers_refusal` sentence, which the report already gave.
fn float_refusal(layer: &PrecisionDocumentLayer) -> String {
    crate::psd_export_float::layer_refusal(layer).unwrap_or_else(|| {
        format!(
            "the layer “{}” has no 32-bit PSD record in Doctored; export Flattened",
            layer.name()
        )
    })
}

/// The complete record list in FILE order: each group's closing divider before
/// its contents, its folder record after them — the mirror of the importer.
/// A style Photoshop cannot hold is BAKED (`bake_style16`): the grown
/// rectangle's u16 pixels at the grown rectangle's origin, an ENABLED mask
/// inside it and no separate `-2` channel. `cursor` is the postorder index of
/// the next record, which is how an adjustment with no Photoshop record finds
/// its stamp (`psd_export_bake`).
pub(crate) fn records(
    layers: &[PrecisionDocumentLayer],
    profile: &Arc<IccProfile>,
    env: &StyleEnv<'_>,
    walk: &Walk<'_>,
    cursor: &mut usize,
    out: &mut Vec<Record>,
) -> Result<(), String> {
    let large = walk.large;
    let float = walk.float;
    let gray = walk.gray.is_some();
    // A Gray or Lab file's masks are written at its declared depth.
    let depth = walk
        .gray
        .map(|(depth, _)| depth)
        .or_else(|| walk.lab_depth());
    let lab = walk.document.is_lab();
    let colours = walk.colours(env);
    for layer in layers {
        let hidden = u8::from(!layer.visible()) << 1;
        let opacity = crate::opacity_byte::code(layer.opacity());
        if let Some(children) = layer.children() {
            out.push(Record {
                bounds: [0; 4],
                channels: placeholder_channels(gray),
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
            records(children, profile, env, walk, cursor, out)?;
            *cursor += 1;
            let mut channels = placeholder_channels(gray);
            let mask = mask_channel(layer, &mut channels, large, float, depth);
            let (mut blocks, ranges) = style_blocks(layer, env, colours)?;
            blocks.extend(crate::psd_locks::block(layer.locks()));
            blocks.extend(label_block(layer));
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
        let postorder = *cursor;
        *cursor += 1;
        if let Content::Adjustment(state) = &layer.content {
            // Photoshop's `lspf` immediately before `lclr` (`psd_locks`).
            let mut blocks: Vec<_> = crate::psd_locks::block(layer.locks()).into_iter().collect();
            blocks.extend(label_block(layer));
            let export = export_style16(layer, false);
            // A Lab document's adjustment is its stamp (module doc).
            let adjustment = ExportAdjustment {
                op: String::new(),
                encoded: if lab {
                    None
                } else {
                    psd_adjustment::encode(state.description())
                },
                inexact: None,
            };
            let stamped = stamps(Some(&adjustment), export.as_ref());
            let Some(encoded) = adjustment.encoded.filter(|_| !stamped) else {
                if float {
                    return Err(float_refusal(layer));
                }
                // No Photoshop record: the stamp, as a canvas-sized Normal
                // pixel layer at 100 % with no mask, keeping the layer's name,
                // visibility, clipping, locks and label.
                let stamp = walk
                    .document
                    .adjustment_stamp16(postorder, walk.limit, walk.other)?;
                let (w, h) = stamp.dimensions();
                out.push(Record {
                    bounds: [0, 0, h as i32, w as i32],
                    channels: walk
                        .channels(&stamp)
                        .ok_or("16-bit PSD export requires RGBA16 UNORM sources")?,
                    blend: *b"norm",
                    opacity: 255,
                    clipping: u8::from(layer.clipped()),
                    flags: 0x08 | hidden,
                    mask: None,
                    name: layer.name().to_string(),
                    divider: None,
                    adjustment: None,
                    blocks,
                    ranges: Vec::new(),
                });
                continue;
            };
            // A pixel-less record carrying the Photoshop adjustment block,
            // and Brightness/Contrast's `CgEd` directly after its `brit`, then
            // its preserved style's blocks and ranges.
            let mut channels = placeholder_channels(gray);
            let mask = mask_channel(layer, &mut channels, large, float, depth);
            let mut written: Vec<_> = encoded.companion.into_iter().collect();
            let ranges = match export {
                Some(_) => {
                    let (style, ranges) = style_blocks(layer, env, colours)?;
                    written.extend(style);
                    ranges
                }
                None => Vec::new(),
            };
            written.extend(blocks);
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
        let (mut blocks, ranges) = style_blocks(layer, env, colours)?;
        blocks.extend(crate::psd_locks::block(layer.locks()));
        blocks.extend(label_block(layer));
        if let Some(fill) = exported_fill16(layer) {
            if gray {
                // A Gray document holds no fill layer (`gray_document`).
                return Err(format!(
                    "the layer “{}” is a fill layer, which a Grayscale PSD export does not write",
                    layer.name()
                ));
            }
            // At 32 bits only Photoshop's shown `SoCo` is written (module doc).
            if float && !fill.written_at_32() {
                return Err(float_refusal(layer));
            }
            // Photoshop's `PtFl`, `SoCo` or `GdFl` record (module doc): the
            // rendered fill over its record rectangle (none for a pixel-less
            // record, whose four planes are then empty), its mask, bit 4.
            let source = layer
                .source()
                .ok_or("this 16-bit layer has no raster pixels to write")?;
            let bits = if float { 32 } else { 16 };
            let rect = fill_rect16(layer, walk.document.dimensions(), &fill, bits);
            let ids = [-1i16, 0, 1, 2].into_iter();
            let mut channels: Vec<(i16, Vec<u8>)> = if float {
                // Raw big-endian f32 planes, as `psd_export_float::channels32`.
                let runs = source
                    .runs_rgba32()
                    .ok_or("32-bit PSD export requires RGBA32 float sources")?;
                let (_, planes) = crate::psd_fill_layer::planes(
                    runs.flatten().copied(),
                    source.dimensions(),
                    layer.offset(),
                    rect,
                    0f32,
                );
                ids.zip(planes)
                    .map(|(id, samples)| {
                        let bytes: Vec<u8> = samples.iter().flat_map(|v| v.to_be_bytes()).collect();
                        (id, crate::psd_export_encode::raw_plane(&bytes))
                    })
                    .collect()
            } else {
                let samples =
                    samples16(source).ok_or("16-bit PSD export requires RGBA16 UNORM sources")?;
                let ([w, h], planes) = crate::psd_fill_layer::planes(
                    samples.copied(),
                    source.dimensions(),
                    layer.offset(),
                    rect,
                    0u16,
                );
                ids.zip(planes)
                    .map(|(id, samples)| (id, plane16(samples.into_iter(), w, h, large)))
                    .collect()
            };
            let mask = mask_channel(layer, &mut channels, large, float, depth);
            out.push(Record {
                bounds: rect,
                channels,
                blend: blend_key(layer.blend_mode()),
                opacity,
                clipping: u8::from(layer.clipped()),
                flags: 0x18 | hidden,
                mask,
                name: layer.name().to_string(),
                divider: None,
                adjustment: Some(crate::psd_fill_layer::encode(&fill, colours)),
                blocks,
                ranges,
            });
            continue;
        }
        let baked = export_style16(layer, false).is_some_and(|style| style.bake.is_some());
        if baked && float {
            return Err(float_refusal(layer));
        }
        let bake = layer
            .style()
            .filter(|_| baked)
            .map(|_| bake_style16(layer, profile, env, JobContext::NONE))
            .transpose()?;
        let source = layer
            .source()
            .ok_or("this 16-bit layer has no raster pixels to write")?;
        let image = bake.as_ref().map_or(source, |(_, image)| image);
        let (w, h) = image.dimensions();
        let (ox, oy) = bake.as_ref().map_or(layer.offset(), |(origin, _)| *origin);
        let mut channels = if float && gray {
            // A 32-bit Gray walk (#1 phase 3c): R and alpha, Raw f32.
            crate::psd_export_gray32::channels_gray32(image)
                .ok_or("32-bit PSD export requires RGBA32 float sources")?
        } else if float {
            crate::psd_export_float::channels32(image)
                .ok_or("32-bit PSD export requires RGBA32 float sources")?
        } else {
            if image.storage() != PrecisionStorage::Rgba16Unorm {
                return Err("16-bit PSD export requires RGBA16 UNORM sources".into());
            }
            walk.channels(image)
                .ok_or("16-bit PSD export requires RGBA16 UNORM sources")?
        };
        // An ENABLED mask on a styled raster shaped the effects and is inside
        // the bake; a disabled one is untouched by it and written as today.
        let baked_mask = bake.is_some() && layer.mask().is_some_and(|m| m.enabled());
        let mask = if baked_mask {
            None
        } else {
            mask_channel(layer, &mut channels, large, float, depth)
        };
        out.push(Record {
            bounds: [
                oy,
                ox,
                oy.saturating_add(h as i32),
                ox.saturating_add(w as i32),
            ],
            channels,
            blend: blend_key(layer.blend_mode()),
            opacity,
            clipping: u8::from(layer.clipped()),
            flags: 0x08 | hidden,
            mask,
            name: layer.name().to_string(),
            divider: None,
            adjustment: None,
            blocks,
            ranges,
        });
    }
    Ok(())
}

impl PrecisionDocument {
    /// This typed document as the depth-agnostic preflight sees it, without
    /// reading a single pixel: `sample_bytes` 2, or 4 for a float document
    /// (1 for an 8-bit Gray or Lab one).
    fn export_inventory(&self, sample_bytes: u64) -> ExportInventory {
        let mut out = Vec::new();
        // The vector masks written as pixels: a raster-less one a PSD path
        // can hold is written as its `vmsk` (`psd_typed_exchange`).
        let vector = crate::psd_typed_exchange::baked_vector_masks(self);
        entries(
            self.root_layers(),
            self.profile(),
            self.dimensions(),
            self.global_light(),
            None,
            &vector,
            // The depth `records` writes a fill at: 32 for float, else 16
            // (an 8-bit Lab or Gray document's fill record is the 16-bit one).
            if sample_bytes == 4 { 32 } else { 16 },
            &mut out,
        );
        let mut styles = Vec::new();
        record_styles(self.root_layers(), &mut styles);
        let mut fills = Vec::new();
        exported_fills16(self.root_layers(), &mut fills);
        let fills: Vec<_> = fills
            .iter()
            .filter_map(crate::psd_fill_layer::Exported::pattern)
            .collect();
        // At 32 bits the file embeds the profile's linear twin and carries
        // Photoshop's 110-byte color-mode data (`psd_export_encode::write_psd32`);
        // a Gray document's file embeds its Gray profile (#1 phase 3b), at
        // 32 bits its Gray linear twin (phase 3c); a Lab document's file
        // embeds none (#1 Lab phase 3b, L2).
        let (profile_bytes, profile_name) = if self.is_lab() {
            (0, String::new())
        } else if let Some(policy) = self.gray_policy() {
            if sample_bytes == 4 {
                let twin = crate::icc_builtin::gray_linear_twin(policy.bytes());
                let name = crate::icc::IccBytes::new(&twin)
                    .map_or_else(|| policy.name().to_string(), |view| view.name());
                (twin.len() as u64 + 110, name)
            } else {
                (policy.bytes().len() as u64, policy.name().to_string())
            }
        } else if sample_bytes == 4 {
            let twin = crate::icc_builtin::linear_twin(self.profile());
            let name = crate::icc::IccProfile::parse(&twin).map_or_else(
                || self.profile().name().to_string(),
                |p| p.name().to_string(),
            );
            (twin.len() as u64 + 110, name)
        } else {
            (
                self.profile().bytes().len() as u64,
                self.profile().name().to_string(),
            )
        };
        let spots = crate::psd_spot::spot_inventory(
            self.channels()
                .iter()
                .filter(|c| c.metadata().spot.is_some())
                .map(|c| c.name()),
        );
        ExportInventory {
            width: self.dimensions().0,
            height: self.dimensions().1,
            sample_bytes,
            cmyk: false,
            gray: self.is_gray(),
            lab: self.is_lab(),
            profile_bytes,
            tail_bytes: crate::psd_patterns::tail_bytes_with(styles, &fills),
            profile_name,
            resolution: self.resolution(),
            channels: self.channels().len(),
            spot_channels: spots.0,
            spot_name_units: spots.1,
            has_guides: !self.guides().is_empty() || self.ruler_origin() != (0.0, 0.0),
            sampler_sizes_lost: self.color_samplers().iter().any(|s| s.sample_size != 1),
            comps: self.layer_comps().len(),
            // The visibility comps the layered mode writes (`psd_comps`).
            comps_written: crate::psd_comps::counts(self.layer_comps()).0,
            comps_narrowed: crate::psd_comps::counts(self.layer_comps()).1,
            path_losses: crate::psd_paths::export_losses(self.paths(), self.dimensions()),
            has_metadata: !self.metadata().is_empty(),
            nondefault_global_light: self.global_light() != GlobalLight::default(),
            // D13: a qualifying bottom is written as Photoshop's Background
            // record (`psd_background`), so only one that cannot be one is
            // said.
            background_unwritable: self.bottom_export(&knockout_written16)
                == crate::psd_background::BottomExport::Unwritable,
            entries: out,
        }
    }

    /// The preflight for one container and mode at the given limits. A float
    /// document is preflighted at 4 bytes a sample: a 32-bit
    /// Grayscale document's layered TIFF is refused by name before any
    /// inventory is built (`psd_export_gray32::TIFF_REFUSAL`; an RGB one is
    /// written), and in the layered mode a record the 32-bit writer has
    /// no form for refuses with `psd_export_float::layers_refusal` when
    /// nothing more fundamental did.
    /// An 8-bit Gray document (#1 phase 3b) or Lab document (#1 Lab
    /// phase 3b) is preflighted at 1 byte a sample, its declared depth.
    pub(crate) fn limited_psd_report(
        &self,
        container: LayeredContainer,
        mode: PsdExportMode,
        file: u64,
        reopen: u64,
    ) -> PsdExportReport {
        if !self.is_float() {
            let eight = self
                .gray_policy()
                .is_some_and(|p| p.depth() == crate::gray_document::GrayDepth::Eight)
                || self
                    .lab_policy()
                    .is_some_and(|p| p.depth() == crate::lab_document::LabDepth::Eight);
            let sample_bytes = if eight { 1 } else { 2 };
            return preflight(
                &self.export_inventory(sample_bytes),
                container,
                mode,
                file,
                reopen,
            );
        }
        if container == LayeredContainer::Tiff && self.is_gray() {
            return PsdExportReport {
                refusal: Some(crate::psd_export_gray32::TIFF_REFUSAL.into()),
                losses: Vec::new(),
                observations: Vec::new(),
                layer_count: 0,
                group_count: 0,
                worst_case_bytes: 0,
                bake_scratch_bytes: 0,
            };
        }
        let mut report = preflight(&self.export_inventory(4), container, mode, file, reopen);
        if mode == PsdExportMode::Layers && report.refusal.is_none() {
            report.refusal = crate::psd_export_float::layers_refusal(self.root_layers());
        }
        report
    }

    /// The preflight for one container and mode at the production limits.
    pub(crate) fn psd_export_report(
        &self,
        container: LayeredContainer,
        mode: PsdExportMode,
    ) -> PsdExportReport {
        self.limited_psd_report(
            container,
            mode,
            container.max_file_bytes(),
            import_envelope_bytes(),
        )
    }

    /// The report JSON at limits that can only lower the production caps:
    /// depth 16, or 32 for a float document. A document holding a
    /// spot channel the writer cannot carry (`spots_refused`) is unavailable
    /// at every mode with `psd_export_preflight::spot_refusal`'s sentence.
    pub(crate) fn psd_report_json(
        &self,
        container: LayeredContainer,
        mode: PsdExportMode,
        file: u64,
        reopen: u64,
    ) -> Value {
        let mut report = self.mode_psd_report_json(container, mode, file, reopen);
        if self.spots_refused(container) {
            report["available"] = json!(false);
            report["refusal"] = json!(crate::psd_export_preflight::spot_refusal(container));
        }
        report
    }

    /// Whether this document's spot channels refuse a save to `container`:
    /// it holds one and the writer cannot carry it there (a Lab or 32-bit
    /// document, or a Gray or Duotone one to the TIFF; `spots_written_for`, asked with the
    /// document's real depth, [`Self::spot_sample_bytes`]).
    fn spots_refused(&self, container: LayeredContainer) -> bool {
        self.has_spot_channel()
            && !crate::psd_export_preflight::spots_written_for(
                self.is_lab(),
                self.is_gray(),
                self.spot_sample_bytes(),
                crate::psd_export_preflight::SpotFile::Layered(container),
            )
    }

    /// This document's sample bytes as the spot-channel rule asks about
    /// them, derived as the export inventories derive theirs: a CMYK
    /// document's policy depth; 4 for a float document; 1 for an 8-bit Gray,
    /// Duotone or Lab policy; else 2.
    fn spot_sample_bytes(&self) -> u64 {
        let eight = self
            .gray_policy()
            .is_some_and(|p| p.depth() == crate::gray_document::GrayDepth::Eight)
            || self
                .lab_policy()
                .is_some_and(|p| p.depth() == crate::lab_document::LabDepth::Eight);
        if let Some(policy) = self.cmyk_policy() {
            u64::from(policy.depth().bits() / 8)
        } else if self.is_float() {
            4
        } else if eight {
            1
        } else {
            2
        }
    }

    /// [`Self::psd_report_json`] by colour mode, before the spot override.
    fn mode_psd_report_json(
        &self,
        container: LayeredContainer,
        mode: PsdExportMode,
        file: u64,
        reopen: u64,
    ) -> Value {
        let (file, reopen) = (
            file.min(container.max_file_bytes()),
            reopen.min(import_envelope_bytes()),
        );
        // A Bitmap document (#1 Bitmap phase 1c), Gray storage underneath,
        // writes a colour-mode-0 file (`bitmap_psd`), asked before Gray.
        if self.is_bitmap() {
            return self.bitmap_psd_report_json(container, mode);
        }
        // An Indexed Color document (#1), RGB storage underneath, writes a
        // colour-mode-2 file (`indexed_psd`).
        if self.is_indexed() {
            return self.indexed_psd_report_json(container, mode);
        }
        // A Grayscale document (#1 phase 3b) writes a colour-mode-1 file at
        // its declared depth (`psd_export_gray`).
        if self.is_gray() {
            return self.gray_psd_report_json(container, mode, file, reopen);
        }
        // A Lab document (#1 Lab phase 3b) writes a colour-mode-9 file at
        // its declared depth (`psd_export_lab`).
        if self.is_lab() {
            return self.lab_psd_report_json(container, mode, file, reopen);
        }
        // A CMYK document (#1 phase 3) has its own export: every container,
        // both modes, at its declared depth (`psd_export_cmyk`).
        if self.is_cmyk() {
            return self.cmyk_psd_report_json(container, mode, file, reopen);
        }
        let depth = if self.is_float() { 32 } else { 16 };
        self.limited_psd_report(container, mode, file, reopen)
            .json(container, mode, depth)
    }

    /// The same report for a classic PSD with reduced limits, so file-size and
    /// reopen-envelope behavior can be qualified without gigabyte fixtures.
    /// Cannot raise the production caps.
    #[doc(hidden)]
    pub fn psd_export_report_with_limits(
        &self,
        mode: i32,
        max_file_bytes: u64,
        import_envelope_bytes: u64,
    ) -> String {
        match PsdExportMode::from_c(mode) {
            Some(mode) => self
                .psd_report_json(
                    LayeredContainer::Psd,
                    mode,
                    max_file_bytes,
                    import_envelope_bytes,
                )
                .to_string(),
            None => json!({ "error": UNKNOWN_MODE }).to_string(),
        }
    }

    /// Writes this document as a 16-bit RGB PSD, PSB or layered TIFF. Pure:
    /// nothing about the document changes, and a refused export never touches
    /// an existing destination, because the whole file is built in memory
    /// first and lands through the shared atomic temp-file rename.
    /// `projection` is the host's own authoritative rendering of THIS
    /// revision, required for an RGB document; the only rendering done here
    /// is an adjustment stamp (`psd_export_bake`), admitted against `limit`
    /// and `other`. A CMYK document takes none and is written by
    /// `psd_export_cmyk` (#1 phase 3), before any projection check; a float
    /// document is written at 32 bits by `psd_export_float`.
    pub(crate) fn save_psd(
        &self,
        projection: Option<&PrecisionImage>,
        path: &str,
        container: LayeredContainer,
        mode: PsdExportMode,
        limit: u64,
        other: u64,
    ) -> Result<(), String> {
        // Spot channels (#1): written to PSD and PSB at 8 and 16 bits since
        // phase 1b; refused where the writer cannot carry them.
        if self.spots_refused(container) {
            return Err(crate::psd_export_preflight::spot_refusal(container).into());
        }
        // A Bitmap document (#1 Bitmap phase 1c): `bitmap_psd`, before Gray.
        if self.is_bitmap() {
            return self.save_bitmap_psd(path, container, mode, limit, other);
        }
        // An Indexed Color document (#1): `indexed_psd`, no projection.
        if self.is_indexed() {
            return self.save_indexed_psd(path, container, mode, limit, other);
        }
        // A Grayscale document (#1 phase 3b): `psd_export_gray`.
        if self.is_gray() {
            return self.save_gray_psd(projection, path, container, mode, limit, other);
        }
        // A Lab document (#1 Lab phase 3b): `psd_export_lab`.
        if self.is_lab() {
            return self.save_lab_psd(projection, path, container, mode, limit, other);
        }
        if self.is_cmyk() {
            return self.save_cmyk_psd(path, container, mode, limit, other);
        }
        // A float document is written at 32 bits (`psd_export_float`).
        if self.is_float() {
            return self.save_psd_float(projection, path, container, mode, limit, other);
        }
        let report = self.psd_export_report(container, mode);
        if let Some(refusal) = report.refusal {
            return Err(refusal);
        }
        let projection = projection.ok_or(
            "an RGB document's PSD export requires its projection (required handle is NULL)",
        )?;
        let large = container.large();
        let (width, height) = self.dimensions();
        if projection.dimensions() != (width, height) {
            return Err("the supplied projection does not match this document's canvas".into());
        }
        if projection.storage() != PrecisionStorage::Rgba16Unorm {
            return Err("16-bit PSD export requires an RGBA16 UNORM projection".into());
        }
        if projection.profile().bytes() != self.profile().bytes() {
            return Err("the supplied projection does not carry this document's ICC bytes".into());
        }
        let canvas_bytes = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(8))
            .ok_or("16-bit PSD export size calculation overflow")?;
        // A wide style colour's `RGBC` builds one high-depth encoder
        // (`authored_color::AuthoredColors::wide`), and so does a wide solid
        // fill's `SoCo` or gradient fill's `GdFl`.
        let mut fills = Vec::new();
        exported_fills16(self.root_layers(), &mut fills);
        let wide_fill = fills.iter().any(crate::psd_fill_layer::Exported::is_wide);
        let wide_bytes = if crate::wide_color_native::styles_wide16(self) || wide_fill {
            crate::authored_color::wide_scratch_bytes()
        } else {
            0
        };
        admit(
            self.retained_bytes(),
            report
                .worst_case_bytes
                .saturating_add(canvas_bytes)
                .saturating_add(report.bake_scratch_bytes)
                .saturating_add(wide_bytes),
            other,
            limit,
        )?;
        let mut out = Vec::new();
        // ONE colour transform for the whole export, the way a render builds
        // one for however many styled layers it holds.
        let colors = StyleEnv::colors_for(self.profile());
        let env = StyleEnv {
            light: self.global_light(),
            canvas: (width, height),
            colors: colors.as_ref(),
            // PSD export writes the STORED planes; a style BAKE is the layer
            // as the canvas shows it, so under `linear_light` it composites
            // in linear light and `bake_style16` encodes it back.
            transfer: self.linear_light.as_deref(),
            // A float document is written by `psd_export_float`, so this
            // walk only sees u16 records.
            float: false,
            // ... and so carries no float style statements.
            float_style: None,
            // A BAKE composites over a transparent accumulator, so `B0` is
            // absent by D4 and `bake_style16` clears both keys anyway.
            knockout: crate::style_knockout::KnockoutBackdrop::NONE,
            capture: None,
            // A Lab document's bakes take its arms.
            lab: self.lab_grid(),
        };
        match mode {
            PsdExportMode::Layers => records(
                self.root_layers(),
                self.profile(),
                &env,
                &Walk {
                    document: self,
                    limit,
                    other,
                    large,
                    float: false,
                    gray: None,
                },
                &mut 0,
                &mut out,
            )?,
            PsdExportMode::Flattened => out.push(Record {
                bounds: [0, 0, height as i32, width as i32],
                channels: channels16(projection, large)
                    .ok_or("16-bit PSD export requires an RGBA16 UNORM projection")?,
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
            }),
        }
        // The comps' `lyid`/`shmd`, the raster-less vector masks' `vmsk` and
        // the Background record (`psd_typed_exchange`).
        if matches!(mode, PsdExportMode::Layers) {
            crate::psd_typed_exchange::stamp(self, &mut out, &knockout_written16);
        }
        // The document's light, declared only when some record writes
        // effects — the byte `psd_export` rule.
        let light = out
            .iter()
            .any(|record| {
                record
                    .blocks
                    .iter()
                    .any(|(key, _)| matches!(key, b"lfx2" | b"lfxs" | b"lmfx"))
            })
            .then_some(self.global_light());
        // The pattern table every written pattern overlay resolves through
        // (`psd_patterns`); no block without one.
        let tail = match mode {
            PsdExportMode::Layers => {
                let mut styles = Vec::new();
                record_styles(self.root_layers(), &mut styles);
                let mut fills = Vec::new();
                exported_fills16(self.root_layers(), &mut fills);
                let fills: Vec<_> = fills
                    .iter()
                    .filter_map(crate::psd_fill_layer::Exported::pattern)
                    .collect();
                crate::psd_patterns::tail_blocks_with(styles, &fills, true)
            }
            PsdExportMode::Flattened => Vec::new(),
        };
        // The paths' resources, and the layered mode's 1065
        // (`psd_typed_exchange`).
        let extra =
            crate::psd_typed_exchange::resources(self, matches!(mode, PsdExportMode::Layers));
        let bytes = if container == LayeredContainer::Tiff {
            // The flat picture is the TIFF's own strip: the projection's
            // straight RGBA16, big-endian. The records and resources ride in
            // Photoshop's tags.
            let runs = projection
                .runs_rgba16()
                .ok_or("16-bit PSD export requires an RGBA16 UNORM projection")?;
            let mut flat = Vec::with_capacity(width as usize * height as usize * 8);
            for run in runs {
                for pixel in run {
                    for sample in pixel {
                        flat.extend_from_slice(&sample.to_be_bytes());
                    }
                }
            }
            let spots = self.layered_tiff_spots();
            let mut resources = image_resources(
                self.profile().bytes(),
                self.resolution(),
                light,
                self.color_samplers(),
                &extra,
            );
            resources.extend_from_slice(&spots.resources);
            crate::tiff_layers::write_spots(
                width,
                height,
                16,
                3,
                self.profile().bytes(),
                self.resolution(),
                &resources,
                &layer_info(&out, false)?,
                &flat,
                &tail,
                &spots.planes,
            )?
        } else {
            write_psd(
                width,
                height,
                16,
                3,
                self.profile().bytes(),
                self.resolution(),
                light,
                self.color_samplers(),
                &out,
                &composite_planes16(projection, width, height)?,
                large,
                &tail,
                &self.psd_spot_planes(false),
                &extra,
            )?
        };
        crate::dr_image::save_atomically(path, |tmp| {
            std::fs::write(tmp, &bytes).map_err(|e| format!("failed to write {path}: {e}"))
        })
    }
}
