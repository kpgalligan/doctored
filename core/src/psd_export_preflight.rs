//! The depth-agnostic PSD export preflight: the pixel-free inventory both
//! document types fill in, the worst-case file arithmetic, the fixed refusal
//! order and the loss/observation inventory.
//!
//! The contract is "never silent": anything the chosen container cannot hold
//! either refuses the export with a reason, or appears in the report's
//! `losses`/`observations` before a single byte is written. Nothing here reads
//! a pixel, so a host can build it while a save panel is open. `psd_export`
//! and `psd_export16` own the record walks that turn an admitted document into
//! bytes.
//!
//! The same picture is written into one of three containers
//! (`psd_framing::LayeredContainer`: PSD, PSB, a layered TIFF). They share the
//! records and so every loss and observation; only the side limit, the file
//! limit, the row-table width in the worst-case size and the sentences that
//! name the format differ, and a PSD refused for a side or for size names PSB.
//!
//! An adjustment layer never refuses. One with a Photoshop record
//! (`psd_adjustment::encode`) is written as that record; any other is BAKED:
//! written as a canvas-sized pixel layer holding the document's picture up to
//! and including it (`psd_export_bake`), reported as a loss per layer, with
//! `bake_scratch_bytes` the working memory the stamp needs: three canvas-sized
//! RGBA planes at the document's depth, a multiplier MEASURED at 24 MP (the
//! stamp's accumulator, its result and the planes split from it; 2 missed the
//! 1.25 margin on the peak footprint at both depths and 3 holds it).
//! The preflight stays pixel-free: a baked entry's planes are the canvas.
//!
//! `psd_export_preflight` is the ONE depth-agnostic preflight (`PsdExportMode`, `PsdExportReport`,
//! `ExportInventory`/`ExportEntry`, refusal order, loss/observation inventory, worst-case size).
//! Only the first general-limitation sentence and the `sample_bytes` multiplier differ by depth; a
//! float document is written at 32 bits through the same preflight (`sample_bytes` 4), a
//! layered TIFF included (a 32-bit Gray one refused), with the layered mode's fill,
//! baked-style and unrecorded-adjustment records refused by name (`psd_export_float`), report
//! `depth` 32. The per-mode preflight, the record walk that mirrors the importer's
//! (each group's closing divider before its contents, its folder record after them, a fifth -2
//! channel for masks; both writers emit a locked record's `lspf` immediately before its `lclr`),
//! and the byte serializer (PackBits with a proven `w + ceil(w/128)` row bound, image resources
//! 1039/1005, the white-matted composite preview). `psd_group_records::PSD_BLEND_KEYS` is the ONE
//! key table, read forwards by import and backwards by `psd_export_preflight::blend_key`. Anything
//! the classic RGB format cannot hold at that depth is refused with a reason or reported — never
//! dropped silently; a document past Doctored's own reopen envelope is a reported loss, never a
//! refusal.

use serde_json::{json, Value};

use crate::blend::BlendMode;
use crate::metadata::Resolution;
use crate::psd_adjustment::Encoded;
use crate::psd_export_encode::plane_worst_case;
use crate::psd_framing::LayeredContainer;
use crate::psd_group_records::PSD_BLEND_KEYS;
use crate::psd_layer_effects::{channel_list, knockout_clause, knockout_keys};
use crate::psd_layer_pixels::{peak_forecast, PeakInputs};

pub(crate) const UNKNOWN_MODE: &str = "unknown PSD export mode; use 0 (layers) or 1 (flattened)";

/// The name a closing section divider carries in every classic writer.
pub(crate) const CLOSER_NAME: &str = "</Layer group>";

/// The limits of this export, stated once. They describe what the writer does,
/// not what this document contains. The first sentence follows the document's
/// own depth and the chosen container: there is no cross-depth export.
fn general_limitations(container: LayeredContainer, depth: u16) -> Vec<&'static str> {
    use LayeredContainer::{Psb, Psd, Tiff};
    let mut out = vec![match (depth, container) {
        (16, Psd) => "Only 16-bit RGB classic PSD is written for a 16-bit document, with PackBits (RLE) channel compression. ZIP, grayscale and CMYK output are not available, and a 16-bit document is never written as 8-bit.",
        (16, Psb) => "Only 16-bit RGB large-document PSB is written for a 16-bit document, with PackBits (RLE) channel compression. ZIP, grayscale and CMYK output are not available, and a 16-bit document is never written as 8-bit.",
        (16, Tiff) => "Only a 16-bit RGB TIFF is written for a 16-bit document: an uncompressed flat image with Photoshop's layer block beside it, its channels PackBits (RLE) compressed. Grayscale and CMYK output are not available, and a 16-bit document is never written as 8-bit.",
        (8, Psd) => "Only 8-bit RGB classic PSD is written, with PackBits (RLE) channel compression. 16-bit, ZIP, grayscale and CMYK output are not available.",
        (8, Psb) => "Only 8-bit RGB large-document PSB is written, with PackBits (RLE) channel compression. 16-bit, ZIP, grayscale and CMYK output are not available.",
        (8, Tiff) => "Only an 8-bit RGB TIFF is written: an uncompressed flat image with Photoshop's layer block beside it, its channels PackBits (RLE) compressed. 16-bit, grayscale and CMYK output are not available.",
        (32, Psd) => "Only 32-bit RGB classic PSD is written for a 32-bit float document: linear-light float samples in the document's own primaries under its profile's linear (gamma 1.0) twin, uncompressed (Raw) channels and layer records in Photoshop's Lr32 block. A style or adjustment layer that 8 and 16 bits would rasterize refuses the layered mode instead, and a 32-bit document is never written at a lower depth.",
        (32, Psb) => "Only 32-bit RGB large-document PSB is written for a 32-bit float document: linear-light float samples in the document's own primaries under its profile's linear (gamma 1.0) twin, uncompressed (Raw) channels and layer records in Photoshop's Lr32 block. A style or adjustment layer that 8 and 16 bits would rasterize refuses the layered mode instead, and a 32-bit document is never written at a lower depth.",
        _ => "Only a 32-bit RGB TIFF is written for a 32-bit float document: a flat image of linear-light float samples with Photoshop's layer block beside it, layer records in Photoshop's Lr32 block, uncompressed (Raw) channels, Macintosh byte order, under the profile's linear (gamma 1.0) twin. A style or adjustment layer that 8 and 16 bits would rasterize refuses the layered mode instead, and a 32-bit document is never written at a lower depth.",
    }];
    // Index 1 must stay the styles sentence: the Lab report replaces
    // `general_limitations[1]` by position (`psd_export_lab::lab_psd_report_json`).
    out.extend_from_slice(&[
        "Layer styles are rasterized into their layer's pixels only when Photoshop's own dialog or a PSD mask cannot hold them, and are otherwise written as Photoshop layer effects and blending options (lfx2, lfxs on a group, iOpa, blending ranges) with the document's global light; either way they are reported per layer, and the effect setting Doctored does not model (the glows' technique) is not written. Invert, Threshold, Posterize, Levels, Curves, Color Balance, Hue/Saturation, Photo Filter, Exposure, Vibrance, Black & White, Channel Mixer, Selective Color and Brightness/Contrast (without saturation) adjustment layers are written as Photoshop adjustment records with their parameters mapped onto Photoshop's own arithmetic, and every other adjustment layer is written as a pixel layer holding the picture up to that layer, reported per layer; editable text, shapes and RAW layers are written as pixels only. No Photoshop type descriptor is written. Knockout and Blend Interior Effects as Group are written as Photoshop's own knko and infx records whenever the style itself is written, and only when they are not at their defaults.",
        "Alpha channels, guides, the ruler origin, layer comps, link groups, EXIF/XMP/IPTC packets and capture packets are not written; spot channels are written to a PSD or PSB in RGB, CMYK, Grayscale or Duotone at 8 or 16 bits, and to a TIFF in RGB or CMYK at 8 or 16 bits.",
        "Layer and group masks are written as ordinary absolute 20-byte masks with their samples, outside default and enabled state. Mask density, feather and PSD link state are not written.",
        "Persisted warp fields and their pre-warp source planes are not written; PSD receives the current rendered planes only.",
        match container {
            Psd => "The classic format limits every side to 30000 pixels and the file to 2 GiB; the large-document format (PSB) holds up to 300000 pixels per side and has no such file limit.",
            Psb => "The large-document format limits every side to 300000 pixels.",
            Tiff => "A layered TIFF limits every side to 30000 pixels and the file to 4 GiB; the large-document format (PSB) holds more.",
        },
        "Doctored's own layered reopen has a decode envelope, Doctored's working-memory limit, which can be smaller than what this format and Photoshop allow; a file past it reopens here as its flattened composite only.",
    ]);
    out
}

/// The CMYK export's general limits (#1 phase 3 and the styles batch,
/// `psd_export_cmyk`): its own first two sentences, then the RGB list's
/// channel, mask, container and reopen sentences (nothing is baked and there
/// is no warp to lose).
pub(crate) fn cmyk_general_limitations(container: LayeredContainer) -> Vec<&'static str> {
    use LayeredContainer::{Psb, Psd, Tiff};
    let shared = general_limitations(container, 16);
    let mut out = vec![
        match container {
            Psd => "A CMYK document is written as a CMYK classic PSD at its declared depth (8 or 16 bits), its inks stored as Photoshop stores them, with PackBits (RLE) channel compression; ZIP is not written.",
            Psb => "A CMYK document is written as a CMYK large-document PSB at its declared depth (8 or 16 bits), its inks stored as Photoshop stores them, with PackBits (RLE) channel compression; ZIP is not written.",
            Tiff => "A CMYK document is written as a CMYK TIFF at its declared depth (8 or 16 bits): an uncompressed flat image of straight ink and alpha with Photoshop's layer block beside it, its channels PackBits (RLE) compressed.",
        },
        "Adjustment layers are written as Photoshop's own CMYK adjustment records (levl, curv, hue2, brit, blnc, selc, mixr, phfl, nvrt, post, thrs, grdm), and layer styles as lfx2 (lfxs on a group) with their colours in ink, iOpa, knko and infx; a kind or a style Photoshop cannot hold refuses the layered export by name, since a CMYK document's export bakes nothing.",
    ];
    out.extend([shared[2], shared[3], shared[5], shared[6]]);
    out
}

/// Which picture the file carries: the editable stack, or one flattened
/// composite raster.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PsdExportMode {
    Layers = 0,
    Flattened = 1,
}

impl PsdExportMode {
    pub(crate) fn from_c(mode: i32) -> Option<Self> {
        match mode {
            0 => Some(Self::Layers),
            1 => Some(Self::Flattened),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Layers => "layers",
            Self::Flattened => "flattened",
        }
    }
}

/// The preflight answer for ONE mode: whether it can be written at all, what
/// will not travel with the file, and how large the file can get.
pub(crate) struct PsdExportReport {
    pub refusal: Option<String>,
    pub losses: Vec<String>,
    pub observations: Vec<String>,
    pub layer_count: usize,
    pub group_count: usize,
    pub worst_case_bytes: u64,
    /// The working memory the adjustment stamps of this export need beyond
    /// the file itself: three canvas-sized RGBA planes at the document's
    /// depth when any entry bakes, else 0.
    pub bake_scratch_bytes: u64,
}

impl PsdExportReport {
    pub(crate) fn json(
        &self,
        container: LayeredContainer,
        mode: PsdExportMode,
        depth: u16,
    ) -> Value {
        json!({
            "format": container.name(),
            "container": container.name(),
            "bake_scratch_bytes": self.bake_scratch_bytes,
            "mode": mode.name(),
            "depth": depth,
            "available": self.refusal.is_none(),
            "refusal": self.refusal,
            "losses": self.losses,
            "observations": self.observations,
            "general_limitations": general_limitations(container, depth),
            "layer_count": self.layer_count,
            "group_count": self.group_count,
            "worst_case_bytes": self.worst_case_bytes,
        })
    }
}

/// The PSD file key for one native blend mode, read backwards out of the ONE
/// key table the importer uses. Pass Through is the folder-only key; a mode
/// with no classic key falls back to Normal, which is exactly what the
/// importer would have produced for an unrecognized key anyway.
pub(crate) fn blend_key(mode: BlendMode) -> [u8; 4] {
    if mode == BlendMode::PassThrough {
        return *b"pass";
    }
    PSD_BLEND_KEYS
        .iter()
        .enumerate()
        .find(|(index, _)| crate::psd::map_psd_blend(*index as i32 + 1) == mode)
        .map(|(_, key)| *key)
        .unwrap_or(*b"norm")
}

/// One mask as the preflight sees it: its extent and the properties a PSD mask
/// cannot hold. `vector` is a live vector mask, which the record walks write
/// as its rendered raster (no `vmsk`).
pub(crate) struct ExportMask {
    pub width: u32,
    pub height: u32,
    pub density: f32,
    pub feather: f32,
    pub linked: bool,
    pub vector: bool,
}

/// The layer style ONE entry carries, as the preflight sees it: which effects
/// render, the fill opacity and Blend If channel beside them, whether the
/// layer's enabled mask shapes those effects, whether other layers clip to
/// this one, why the style must be BAKED instead of written (`None` = it is
/// written as `lfx2`/`iOpa`/blending ranges), whether an `lfx2` descriptor is
/// written at all (the record walks' own `style.has_enabled_effects()`, which
/// a Blend If-only, fill-opacity-only or all-disabled style fails), and what
/// those blocks cost.
pub(crate) struct ExportStyle {
    pub effects: Vec<String>,
    pub fill_opacity: f32,
    /// Every Blend If range's channel, primary first; empty = none.
    pub blend_if: Vec<crate::style::BlendIfChannel>,
    /// The style's `knockout`, written as Photoshop's own `knko` record
    /// whenever the style itself is written and the value is not `None`
    /// (12E-3c). A BAKED style writes neither key and reports the narrowed
    /// per-layer loss instead, because the bake clears both
    /// (`psd_export_rasterize`).
    pub knockout: crate::style::Knockout,
    /// The style's `blend_interior_as_group`, written as Photoshop's own
    /// `infx` record under the same rule.
    pub blend_interior: bool,
    pub masked: bool,
    pub clip_base: bool,
    pub bake: Option<String>,
    pub writes_effects: bool,
    pub block_bytes: u64,
}

impl ExportStyle {
    /// The style's contents as both sentences name them: the effects, then
    /// the fill opacity, then the Blend If channel, each only when it is
    /// there. `join` separates the last clause.
    fn clause(&self, join: &str) -> String {
        let mut out = self.effects.join(", ");
        if self.fill_opacity < 1.0 {
            if !out.is_empty() {
                out.push_str(", ");
            }
            out.push_str(&format!("fill opacity {:.0} %", self.fill_opacity * 100.0));
        }
        if !self.blend_if.is_empty() {
            if !out.is_empty() {
                out.push_str(join);
            }
            out.push_str(&format!("Blend If on {}", channel_list(&self.blend_if)));
        }
        out
    }
}

/// Why an ENABLED mask cannot travel beside a preserved style; re-exported
/// from the ONE codec so both depths ask it the same way.
pub(crate) use crate::psd_layer_effects::mask_bake_reason;

/// The adjustment ONE entry is: its Doctored op, and the Photoshop record
/// `psd_adjustment` writes for it — `None` when this build writes no record
/// for it, in which case the layer is BAKED into a pixel layer. `inexact` is
/// the structural reason a baked layer cannot be followed exactly
/// ([`bake_inexact`]); it is `None` for a written record.
pub(crate) struct ExportAdjustment {
    pub op: String,
    pub encoded: Option<Encoded>,
    pub inexact: Option<String>,
}

/// Whether an entry's record is its adjustment STAMP (`psd_export_bake`): an
/// adjustment with no Photoshop record (`encoded` is `None`), or one whose
/// layer style Photoshop cannot hold (its `bake` is `Some`) — a
/// pixel-less record has no pixels to rasterize a style into. The ONE
/// predicate both record walks, both inventories and `bake_scratch_bytes`
/// ask, so the preflight forecasts the record the writer produces.
pub(crate) fn stamps(adjustment: Option<&ExportAdjustment>, style: Option<&ExportStyle>) -> bool {
    adjustment.is_some_and(|a| a.encoded.is_none() || style.is_some_and(|s| s.bake.is_some()))
}

/// Why a baked adjustment's stamp cannot reproduce the document exactly, in
/// the words that follow "It is": the layer is clipped, or `group` names the
/// nearest enclosing group that is not Normal or Pass Through at 100 % with no
/// enabled mask, no rendered style and unclipped. `None` when neither holds.
pub(crate) fn bake_inexact(clipped: bool, group: Option<&str>) -> Option<String> {
    let inside = group.map(|name| {
        format!(
            "inside group “{name}”, which has its own blend mode, opacity, mask, style or clipping"
        )
    });
    match (clipped, inside) {
        (true, Some(inside)) => Some(format!("clipped to the layer beneath it and {inside}")),
        (true, None) => Some("clipped to the layer beneath it".into()),
        (false, inside) => inside,
    }
}

/// The loss a baked adjustment layer reports: why it is a pixel layer, what
/// that layer holds, and — when `inexact` — where the file can differ.
fn bake_loss(name: &str, adjustment: &ExportAdjustment) -> String {
    let op = adjustment.op.as_str();
    let mut sentence = match op {
        // A written record whose STYLE forced the stamp ([`stamps`]).
        _ if adjustment.encoded.is_some() => format!("Adjustment layer “{name}” ({op}) has a layer style Photoshop cannot hold on an adjustment record, so it is written as a pixel layer with the style rasterized."),
        "gradient_map" => format!("Adjustment layer “{name}” (gradient_map) is written as a pixel layer, not as Photoshop's Gradient Map record (grdm): the published format gives that record's field sizes without their units, so this build does not write it."),
        "color_lookup" => format!("Adjustment layer “{name}” (color_lookup) is written as a pixel layer, not as Photoshop's Color Lookup record (clrL): the published format does not describe that record's contents, so this build does not write it."),
        _ => format!("Adjustment layer “{name}” ({op}) has no Photoshop record and is written as a pixel layer."),
    };
    sentence.push_str(" It holds this document's picture up to and including that layer. The layers beneath it stay in the file but no longer show through it, and where they are partly transparent the baked layer adds its own coverage.");
    if let Some(reason) = &adjustment.inexact {
        sentence.push_str(&format!(" It is {reason}, which a baked layer cannot follow exactly, so this file can look different there; the flattened mode is exact."));
    }
    sentence
}

/// One record of the document, without a single pixel: what it is, what it
/// carries, and every property the classic format would drop.
pub(crate) struct ExportEntry {
    pub name: String,
    pub group: bool,
    pub visible: bool,
    pub clipped: bool,
    pub blend: BlendMode,
    pub opacity: f32,
    pub offset: (i32, i32),
    /// The four colour/alpha planes, in PIXELS. A folder's are empty.
    pub planes: Vec<(u64, u64)>,
    pub mask: Option<ExportMask>,
    pub adjustment: Option<ExportAdjustment>,
    pub style: Option<ExportStyle>,
    pub described: bool,
    pub retained_source: bool,
    pub warp: bool,
    /// The entry carries a lock. Layered export writes it as `lspf`, so it is
    /// read only by [`is_single_plain_raster`]: the flattened mode writes no
    /// `lspf`, and flattening a locked layer loses that lock.
    pub locked: bool,
    pub link_group: bool,
}

/// The whole document as the preflight sees it — the ONE shape both the byte
/// and the typed document fill in, so neither depth can drift from the other.
pub(crate) struct ExportInventory {
    pub width: u32,
    pub height: u32,
    /// 1 for an 8-bit document, 2 for a 16-bit one, 4 for a 32-bit float
    /// one.
    pub sample_bytes: u64,
    /// A CMYK document (#1 phase 3, `psd_export_cmyk`): every raster and
    /// the composite carry five planes (four inks and alpha), and the
    /// general limitations are the CMYK ones.
    pub cmyk: bool,
    /// A Grayscale document (#1 phase 3b, `psd_export_gray`): the
    /// composite carries ONE plane (the white-matted gray, as Photoshop
    /// writes it) and the embedded profile is its Gray one.
    pub gray: bool,
    /// A Lab Color document (#1 Lab phase 3b, `psd_export_lab`): the
    /// composite carries THREE planes (L, a, b matted over Lab white, as
    /// Photoshop writes them) and no profile is embedded (`profile_bytes`
    /// 0): L\*a\*b\* values are CIE D50 by definition.
    pub lab: bool,
    pub profile_bytes: u64,
    /// The layered mode's document tail (the pattern table,
    /// `psd_patterns::tail_bytes_with`): an upper bound, 0 without a pattern.
    pub tail_bytes: u64,
    pub profile_name: String,
    pub resolution: Resolution,
    /// Every channel, alpha and spot.
    pub channels: usize,
    /// The spot channels among them (`psd_spot`), and their names' UTF-16
    /// units in total.
    pub spot_channels: u64,
    pub spot_name_units: u64,
    pub has_guides: bool,
    /// Some colour sampler's sample size is not 1: PSD 1073 stores no
    /// per-sampler size, so Photoshop reads every sampler at its own Sample
    /// Size (`psd_samplers`). The samplers themselves are written.
    pub sampler_sizes_lost: bool,
    /// The document's layer comps. PSD `cmls` is not written (plan §10C-3), so
    /// every comp is a disclosed loss in both modes.
    pub comps: usize,
    pub has_metadata: bool,
    pub nondefault_global_light: bool,
    /// D13: this document's bottom entry satisfies D5 and a WRITTEN knockout
    /// REACHES it (a baked one writes no `knko`, so its file punches nothing
    /// in Photoshop). Doctored never writes a true Photoshop Background
    /// record, so the hole this file opens shows the bottom layer here and
    /// transparency in Photoshop — disclosed once per file, never silently.
    pub structural_background_knockout: bool,
    /// Bottom-first, in the layered mode's own order.
    pub entries: Vec<ExportEntry>,
}

/// One plane's dimensions in pixels, used by the pixel-free worst-case
/// arithmetic.
type Plane = (u64, u64);

/// What one record costs before any pixels are read: its name in UTF-16 units
/// and the planes it will carry.
struct RecordCost {
    name_units: u64,
    planes: Vec<Plane>,
    /// The adjustment block a carried entry adds: signature, key, length
    /// and body.
    block_bytes: u64,
}

fn name_units(name: &str) -> u64 {
    name.encode_utf16().count() as u64
}

/// The record costs of one export, in file order.
fn export_costs(inventory: &ExportInventory, mode: PsdExportMode) -> Vec<RecordCost> {
    if mode == PsdExportMode::Flattened {
        return vec![RecordCost {
            name_units: name_units("Composite"),
            planes: vec![
                (u64::from(inventory.width), u64::from(inventory.height));
                composite_planes(inventory)
            ],
            block_bytes: 0,
        }];
    }
    let mut out = Vec::new();
    for entry in &inventory.entries {
        if entry.group {
            out.push(RecordCost {
                name_units: name_units(CLOSER_NAME),
                planes: vec![(0, 0); 4],
                block_bytes: 0,
            });
        }
        let mut planes = entry.planes.clone();
        if let Some(mask) = &entry.mask {
            planes.push((u64::from(mask.width), u64::from(mask.height)));
        }
        out.push(RecordCost {
            name_units: name_units(&entry.name),
            planes,
            block_bytes: entry
                .adjustment
                .as_ref()
                .and_then(|adjustment| adjustment.encoded.as_ref())
                .map_or(0, |encoded| {
                    12 + encoded.body.len() as u64
                        + encoded
                            .companion
                            .as_ref()
                            .map_or(0, |(_, body)| 12 + body.len() as u64)
                })
                + entry.style.as_ref().map_or(0, |style| style.block_bytes),
        });
    }
    out
}

/// The composite's planes: R, G, B, A — or C, M, Y, K, A for CMYK, the
/// one white-matted Gray for Grayscale, or L, a, b over Lab white for Lab.
fn composite_planes(inventory: &ExportInventory) -> usize {
    if inventory.cmyk {
        5
    } else if inventory.gray {
        1
    } else if inventory.lab {
        3
    } else {
        4
    }
}

/// True when this export writes the document's spot channels (`psd_spot`):
/// a PSD or PSB of an RGB, CMYK, Grayscale or Duotone document at 8 or 16
/// bits, a layered TIFF of an RGB or CMYK one (`tiff_spot`).
fn spots_written(inventory: &ExportInventory, container: LayeredContainer) -> bool {
    spots_written_for(
        inventory.lab,
        inventory.gray,
        inventory.sample_bytes,
        SpotFile::Layered(container),
    )
}

/// The file a spot-channel save writes: a layered container (PSD, PSB, the
/// layered TIFF) or the flat TIFF (#1 1c-2a, `tiff_spot`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpotFile {
    Layered(LayeredContainer),
    FlatTiff,
}

/// [`spots_written`]'s one rule, by its inputs: the preflight and the save
/// refusals for spot channels elsewhere (`spot_channel::PSD_REFUSAL`,
/// `spot_channel::TIFF_REFUSAL`) all ask it, so a file the writer can carry
/// is never refused. A PSD or PSB at 8 or 16 bits, not Lab; a TIFF, flat or
/// layered (#1 1c-2b: the layered file's composite carries them as the
/// flat file's trailing extra samples), of an RGB or CMYK document (not
/// Gray, Duotone or Lab) at 8 or 16 bits.
pub(crate) fn spots_written_for(lab: bool, gray: bool, sample_bytes: u64, file: SpotFile) -> bool {
    match file {
        SpotFile::Layered(LayeredContainer::Tiff) | SpotFile::FlatTiff => {
            !lab && !gray && sample_bytes <= 2
        }
        SpotFile::Layered(_) => !lab && sample_bytes <= 2,
    }
}

/// The sentence a layered export of a document whose spot channels
/// [`spots_written_for`] does not carry refuses with: the TIFF's
/// (`spot_channel::TIFF_REFUSAL`) for the layered TIFF, else the PSD's.
pub(crate) fn spot_refusal(container: LayeredContainer) -> &'static str {
    match container {
        LayeredContainer::Tiff => crate::spot_channel::TIFF_REFUSAL,
        _ => crate::spot_channel::PSD_REFUSAL,
    }
}

/// How many spot channels a file whose picture takes `composite_planes`
/// planes can declare: Adobe's 56-channel cap less those planes.
pub(crate) fn spots_capacity(composite_planes: usize) -> usize {
    crate::psd_framing::MAX_PSD_CHANNELS.saturating_sub(composite_planes)
}

/// The bytes `written` spot channels can take: each plane's RLE worst case
/// and 512 bytes of its resource entries, and the 1045 names at two bytes a
/// UTF-16 unit.
pub(crate) fn spot_bytes(
    written: u64,
    name_units: u64,
    row_bytes: u64,
    height: u64,
    large: bool,
) -> u64 {
    written
        .saturating_mul(plane_worst_case(row_bytes, height, large).saturating_add(512))
        .saturating_add(name_units.saturating_mul(2))
}

/// The spot channels this export writes (the first [`spots_capacity`]).
fn spots_in_file(inventory: &ExportInventory, container: LayeredContainer) -> u64 {
    if !spots_written(inventory, container) {
        return 0;
    }
    inventory
        .spot_channels
        .min(spots_capacity(composite_planes(inventory)) as u64)
}

/// The preflight for one container and mode at the supplied limits.
pub(crate) fn preflight(
    inventory: &ExportInventory,
    container: LayeredContainer,
    mode: PsdExportMode,
    max_file_bytes: u64,
    import_envelope_bytes: u64,
) -> PsdExportReport {
    let layers = mode == PsdExportMode::Layers;
    let large = container.large();
    let sample_bytes = inventory.sample_bytes;
    let row = |width: u64| width.saturating_mul(sample_bytes);
    let costs = export_costs(inventory, mode);
    let mut worst_case = inventory.profile_bytes.saturating_add(256);
    // The flat picture: a PSD's RLE composite preview, whose worst case also
    // bounds a TIFF's uncompressed strip of the same planes (four, or five
    // for CMYK).
    worst_case = worst_case.saturating_add(
        plane_worst_case(
            row(u64::from(inventory.width)),
            u64::from(inventory.height),
            large,
        )
        .saturating_mul(composite_planes(inventory) as u64),
    );
    if container == LayeredContainer::Tiff {
        // The directory, the block header and the out-of-line tag values, and
        // the profile a second time: a TIFF carries it in tag 34675 as well
        // as in resource 1039 inside tag 34377.
        worst_case = worst_case
            .saturating_add(512)
            .saturating_add(inventory.profile_bytes);
    }
    if layers {
        // The pattern table after the global layer mask (or beside the TIFF's
        // `Layr`/`Lr16` block): only the layered mode writes one.
        worst_case = worst_case.saturating_add(inventory.tail_bytes);
    }
    // The spot channels' planes after the composite, and their resources.
    worst_case = worst_case.saturating_add(spot_bytes(
        spots_in_file(inventory, container),
        inventory.spot_name_units,
        row(u64::from(inventory.width)),
        u64::from(inventory.height),
        large,
    ));
    let mut metadata_bytes = 0u64;
    let mut encoded = 0u64;
    for cost in &costs {
        // 28: a record's `fxrp` (12 framing + two f64, `psd_pattern_fill`),
        // which any record may carry.
        let record = (512 + 28 + 2 * cost.name_units).saturating_add(cost.block_bytes);
        metadata_bytes = metadata_bytes.saturating_add(record);
        worst_case = worst_case.saturating_add(record);
        for &(w, h) in &cost.planes {
            let plane = plane_worst_case(row(w), h, large);
            encoded = encoded.max(plane);
            worst_case = worst_case.saturating_add(plane);
        }
    }

    let group_count = if layers {
        inventory.entries.iter().filter(|e| e.group).count()
    } else {
        0
    };
    let layer_count = if layers {
        inventory.entries.len() - group_count
    } else {
        1
    };

    let mut report = PsdExportReport {
        refusal: export_refusal(inventory, container, mode, worst_case, max_file_bytes),
        losses: Vec::new(),
        observations: Vec::new(),
        layer_count,
        group_count,
        worst_case_bytes: worst_case,
        // Three canvases of four samples. The multiplier is measured, not
        // derived: at 24 MP the bake's peak footprint exceeds its no-bake
        // twin's by 292 MB (8-bit) and 585 MB (16-bit), and with the baked
        // record's own planes already in `worst_case` this term has to be
        // at least 2.8 canvases for the plan to hold 1.25x that. One baked
        // layer is stamped at a time, so the term does not grow with the
        // number of baked layers (`core/examples/psd_bake_admission.rs`).
        bake_scratch_bytes: if layers
            && inventory
                .entries
                .iter()
                .any(|entry| stamps(entry.adjustment.as_ref(), entry.style.as_ref()))
        {
            (3 * 4 * sample_bytes)
                .saturating_mul(u64::from(inventory.width))
                .saturating_mul(u64::from(inventory.height))
        } else {
            0
        },
    };
    if layers {
        layer_losses(inventory, container, &mut report.losses);
    } else {
        flattened_losses(inventory, container, &mut report.losses);
    }
    export_observations(inventory, container, mode, &mut report.observations);

    let mut pixels = group_count as u64;
    let mut mask_pixels = 0u64;
    let mut largest = 0u64;
    if layers {
        for entry in &inventory.entries {
            if !entry.group {
                let (w, h) = entry.planes.first().copied().unwrap_or((0, 0));
                let count = w.saturating_mul(h);
                pixels = pixels.saturating_add(count);
                largest = largest.max(count);
            }
            if let Some(mask) = &entry.mask {
                let count = u64::from(mask.width) * u64::from(mask.height);
                mask_pixels = mask_pixels.saturating_add(count);
                largest = largest.max(count);
            }
        }
    } else {
        let count = u64::from(inventory.width) * u64::from(inventory.height);
        pixels = count;
        largest = count;
    }
    let peak = peak_forecast(PeakInputs {
        source_bytes: worst_case,
        metadata_bytes,
        pixels,
        mask_pixels,
        largest,
        encoded,
        canvas_pixels: u64::from(inventory.width) * u64::from(inventory.height),
        sample_bytes,
        planes: if inventory.cmyk { 5 } else { 4 },
    });
    if peak > import_envelope_bytes {
        report.losses.push(format!(
            "This file will exceed Doctored's own {} GiB layered import envelope; Doctored will reopen only its flattened composite. Photoshop is not affected.",
            crate::precision_image::gib_text(crate::psd_layer_pixels::import_envelope_bytes())
        ));
    }
    report
}

/// The first reason this mode cannot be written at all, in a fixed order so a
/// host always sees the most fundamental one. Every sentence names the
/// container, and a PSD or TIFF refused for a side or for size names PSB.
fn export_refusal(
    inventory: &ExportInventory,
    container: LayeredContainer,
    mode: PsdExportMode,
    worst_case: u64,
    max_file_bytes: u64,
) -> Option<String> {
    let max_side = container.max_side();
    // "the classic PSD format stores at most 30000 pixels per side", and the
    // route past it when there is one.
    let (stores, route) = match container {
        LayeredContainer::Psd => (
            format!("the classic PSD format stores at most {max_side} pixels per side"),
            " Export it as a Large Document (PSB), which stores up to 300000.",
        ),
        LayeredContainer::Psb => (
            format!("the PSB format stores at most {max_side} pixels per side"),
            "",
        ),
        LayeredContainer::Tiff => (
            format!("a layered TIFF stores at most {max_side} pixels per side"),
            " Export it as a Large Document (PSB), which stores up to 300000.",
        ),
    };
    if inventory.width > max_side || inventory.height > max_side {
        return Some(format!(
            "This canvas is {} x {} pixels; {stores}.{route}",
            inventory.width, inventory.height
        ));
    }
    if worst_case > max_file_bytes {
        return Some(match container {
            LayeredContainer::Psd => format!(
                "This document could need up to {worst_case} bytes as a PSD, past the format's {max_file_bytes}-byte limit. Export the flattened composite, or save a native .doctored copy. A Large Document (PSB) has no such limit."
            ),
            LayeredContainer::Psb => format!(
                "This document could need up to {worst_case} bytes as a PSB, past the {max_file_bytes}-byte limit. Export the flattened composite, or save a native .doctored copy."
            ),
            LayeredContainer::Tiff => format!(
                "This document could need up to {worst_case} bytes as a layered TIFF, past the format's {max_file_bytes}-byte limit. Export the flattened composite, or save a native .doctored copy. A Large Document (PSB) has no such limit."
            ),
        });
    }
    if mode != PsdExportMode::Layers {
        return None;
    }
    // A CMYK document (#1, `psd_export_cmyk`) bakes nothing: an adjustment
    // kind with no Photoshop record, or a style Photoshop's dialog cannot
    // hold, refuses the Layers mode by name. The report and the save read
    // this one answer.
    if inventory.cmyk {
        for entry in &inventory.entries {
            if let Some(adjustment) = entry.adjustment.as_ref().filter(|a| a.encoded.is_none()) {
                return Some(format!(
                    "Adjustment layer “{}” ({}) has no Photoshop record in a CMYK file; export Flattened",
                    entry.name, adjustment.op
                ));
            }
            if let Some(reason) = entry.style.as_ref().and_then(|style| style.bake.as_ref()) {
                return Some(format!(
                    "“{}”'s layer style needs baking ({reason}), which Export as PSD does not do from a CMYK document; export Flattened",
                    entry.name
                ));
            }
        }
    }
    let side = u64::from(max_side);
    for entry in &inventory.entries {
        let (w, h) = entry.planes.first().copied().unwrap_or((0, 0));
        if !entry.group && (w > side || h > side) {
            return Some(format!(
                "Layer “{}” is {w} x {h} pixels; {stores}.{route}",
                entry.name
            ));
        }
        if let Some(mask) = &entry.mask {
            let (mw, mh) = (mask.width, mask.height);
            if mw > max_side || mh > max_side {
                return Some(format!(
                    "The mask on “{}” is {mw} x {mh} pixels; {stores}.{route}",
                    entry.name
                ));
            }
        }
    }
    for entry in &inventory.entries {
        // Only a BAKED style grows a rectangle; a preserved one keeps the
        // layer's own planes.
        if entry.group || entry.style.as_ref().is_none_or(|s| s.bake.is_none()) {
            continue;
        }
        let (w, h) = entry.planes.first().copied().unwrap_or((0, 0));
        if w.saturating_mul(h) > crate::doc::MAX_PIXELS {
            return Some(crate::psd_export_rasterize::oversize_refusal(
                &entry.name,
                w,
                h,
            ));
        }
    }
    for entry in &inventory.entries {
        // A styled folder's effects are WRITTEN; only a style Photoshop
        // cannot hold would need a bake, and a group's cannot be baked
        // without flattening its contents.
        let Some(reason) = entry
            .style
            .as_ref()
            .filter(|_| entry.group)
            .and_then(|style| style.bake.clone())
        else {
            continue;
        };
        return Some(format!(
            "Group “{}” has a layer style whose {reason}; a group's effects cannot be rasterized without flattening its contents. Clear the style, merge the group, or export the flattened composite.",
            entry.name
        ));
    }
    for entry in &inventory.entries {
        if let Some(mask) = &entry.mask {
            if mask.density != 1.0 || mask.feather != 0.0 {
                return Some(format!(
                    "The mask on “{}” has density or feather; a PSD mask stores neither. Apply those properties or export the flattened composite.",
                    entry.name
                ));
            }
        }
    }
    None
}

fn layer_losses(
    inventory: &ExportInventory,
    container: LayeredContainer,
    losses: &mut Vec<String>,
) {
    for entry in &inventory.entries {
        let name = &entry.name;
        if let Some((style, reason)) = entry
            .style
            .as_ref()
            .filter(|_| !entry.group)
            .and_then(|style| style.bake.as_ref().map(|reason| (style, reason)))
        {
            let mut sentence = format!(
                "Layer “{name}” has a layer style ({}) whose {reason}; its effects are rasterized into the layer's pixels for this file. Effect blend modes are evaluated against transparency, so effects that blend with the layers beneath can look different.",
                style.effects.join(", ")
            );
            if !style.blend_if.is_empty() {
                sentence.push_str(" Its Blend If ranges are not written and were not applied.");
            }
            if style.masked {
                sentence.push_str(" Its mask shapes the effects, so it is baked into those pixels and not written as a separate mask.");
            }
            if style.clip_base {
                sentence
                    .push_str(" Layers clipped to it are written above the rasterized effects.");
            }
            losses.push(sentence);
        }
        // A BAKED style writes neither key: the rasterized pixels already
        // hold what a knockout would have punched with, so writing them
        // would punch a second time (12E-3c, E2).
        if let Some(what) = entry
            .style
            .as_ref()
            .filter(|style| style.bake.is_some())
            .and_then(|style| knockout_clause(style.knockout, style.blend_interior))
        {
            losses.push(format!(
                "Layer “{name}” uses {what}, but its style is rasterized for this file, so it is not written: the rasterized pixels already hold the effects a knockout would punch with."
            ));
        }
        if let Some(adjustment) = entry
            .adjustment
            .as_ref()
            .filter(|_| stamps(entry.adjustment.as_ref(), entry.style.as_ref()))
        {
            losses.push(bake_loss(name, adjustment));
        }
        if let Some((op, clause)) = entry.adjustment.as_ref().and_then(|adjustment| {
            let clause = adjustment.encoded.as_ref()?.unwritten?;
            Some((&adjustment.op, clause))
        }) {
            losses.push(format!("Adjustment layer “{name}” ({op}) {clause}"));
        }
        if entry.described {
            losses.push(format!(
                "Layer “{name}” has a re-editable description (text or shape); it is exported as pixels only."
            ));
        }
        if entry.retained_source {
            losses.push(format!(
                "Layer “{name}” has an embedded RAW source; it is exported as rendered pixels only."
            ));
        }
        if entry.warp {
            losses.push(format!(
                "Layer “{name}” has a persisted warp field and pre-warp source; only the rendered planes are exported."
            ));
        }
        if entry.link_group {
            losses.push(format!(
                "Layer “{name}” belongs to a link group; PSD has no equivalent and it is not written."
            ));
        }
        if entry.mask.as_ref().is_some_and(|mask| mask.vector) {
            losses.push(format!(
                "The vector mask on “{name}” is written as pixels, an ordinary layer mask of its rendered shape; its path is not written."
            ));
        }
        if entry.mask.as_ref().is_some_and(|mask| mask.linked) {
            losses.push(format!(
                "The mask on “{name}” is linked to its layer; PSD link state not written."
            ));
        }
    }
    // D13, once per file: the layered file stores Doctored's Background as
    // an ordinary layer, because Doctored never writes a true Photoshop
    // Background record. The FLATTENED mode carries no knockout at all — the
    // composite already holds Doctored's own answer — so the sentence is
    // stated here and not in `document_losses`.
    if inventory.structural_background_knockout {
        losses.push("A layer uses knockout over this document's bottom layer, which Doctored treats as the Background. A PSD written by Doctored stores it as an ordinary layer, and Photoshop punches a Deep knockout (or a Shallow one outside every group) through to transparency when a file has no Background layer, so the hole can show transparency in Photoshop where it shows the bottom layer here.".into());
    }
    document_losses(inventory, container, preserves_a_style(inventory), losses);
}

fn flattened_losses(
    inventory: &ExportInventory,
    container: LayeredContainer,
    losses: &mut Vec<String>,
) {
    if !is_single_plain_raster(inventory) {
        losses.push(
            "The flattened mode writes one composited raster layer: every layer, group, mask, clipping relationship and blend mode is baked into it and cannot be separated again."
                .into(),
        );
    }
    document_losses(inventory, container, false, losses);
}

/// True when the document already IS the picture one raster layer would carry,
/// so flattening loses nothing.
fn is_single_plain_raster(inventory: &ExportInventory) -> bool {
    let [entry] = &inventory.entries[..] else {
        return false;
    };
    !entry.group
        && entry.mask.is_none()
        && entry.style.is_none()
        && !entry.described
        && entry.adjustment.is_none()
        && entry.visible
        && !entry.clipped
        && entry.opacity == 1.0
        && entry.blend == BlendMode::Normal
        && entry.offset == (0, 0)
        && !entry.locked
        && entry.planes.first().copied()
            == Some((u64::from(inventory.width), u64::from(inventory.height)))
}

/// True when at least one entry writes an `lfx2` descriptor — the only case
/// in which this file carries image resources 1037/1049, so it is the record
/// walks' own `Some(light)` condition (a baked style, and a preserved style
/// that is Blend If-only, fill-opacity-only or all-disabled, write no
/// `lfx2`).
fn preserves_a_style(inventory: &ExportInventory) -> bool {
    inventory.entries.iter().any(|entry| {
        entry
            .style
            .as_ref()
            .is_some_and(|s| s.bake.is_none() && s.writes_effects)
    })
}

fn document_losses(
    inventory: &ExportInventory,
    container: LayeredContainer,
    preserved: bool,
    losses: &mut Vec<String>,
) {
    let spots = inventory.spot_channels;
    let alpha = (inventory.channels as u64).saturating_sub(spots);
    if alpha != 0 {
        losses.push(format!(
            "{alpha} saved channels are not written; this export stores no PSD alpha channels."
        ));
    }
    let unwritten = spots - spots_in_file(inventory, container);
    if unwritten != 0 && spots_written(inventory, container) {
        losses.push(format!(
            "{unwritten} spot channels are not written; a PSD holds at most 56 channels, and the picture's own planes take {}.",
            composite_planes(inventory)
        ));
    } else if unwritten != 0 && container == LayeredContainer::Tiff {
        losses.push(format!(
            "{unwritten} spot channels are not written; Doctored writes TIFF spot channels only in RGB and CMYK at 8 and 16 bits."
        ));
    } else if unwritten != 0 {
        losses.push(format!(
            "{unwritten} spot channels are not written; Doctored writes PSD spot channels only in RGB, CMYK, Grayscale and Duotone at 8 and 16 bits."
        ));
    }
    if inventory.has_guides {
        losses.push("Guides and the ruler origin are not written.".into());
    }
    if inventory.sampler_sizes_lost {
        losses.push("Color sampler sample sizes are not written; Photoshop reads every sampler at its own Sample Size.".into());
    }
    match inventory.comps {
        0 => {}
        1 => losses.push(
            "1 layer comp is not written; this export stores no Photoshop layer comps.".into(),
        ),
        n => losses.push(format!(
            "{n} layer comps are not written; this export stores no Photoshop layer comps."
        )),
    }
    if inventory.has_metadata {
        losses.push("EXIF/XMP/IPTC packets are not written.".into());
    }
    if inventory.nondefault_global_light && !preserved {
        losses.push(
            "The document's global light direction is not written; PSD stores it only alongside the layer effects this export omits."
                .into(),
        );
    }
}

/// The blend modes whose appearance in Photoshop is claimed for an RGB file
/// (#2, `scripts/photoshop-probe/psd_appearance_578_take.py`, `--rejudge`
/// of take1-3): every judged tile of the mode (opacity, Fill, inside a
/// Normal, Pass Through or moded group, a clipped member, a clipping base)
/// within 2 codes max and 0.5 mean of Photoshop 26.10.0's own render at 8
/// and 16 bits. Normal, Multiply, Screen and Pass Through are the claim's
/// floor whatever a take says.
const QUALIFIED_BLEND: &[BlendMode] = &[
    BlendMode::Normal,
    BlendMode::Darken,
    BlendMode::Multiply,
    BlendMode::ColorBurn,
    BlendMode::LinearBurn,
    BlendMode::Lighten,
    BlendMode::Screen,
    BlendMode::ColorDodge,
    BlendMode::Addition,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::HardLight,
    BlendMode::PinLight,
    BlendMode::Exclusion,
    BlendMode::Subtract,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
    BlendMode::PassThrough,
];

/// The modes claimed only at 100 % Fill: every tile but the Fill ones passes
/// (#2: Lighter Color misses on a luminosity tie, Hard Mix and Difference
/// on Photoshop's own Fill arithmetic, which no declared candidate met).
const QUALIFIED_AT_FULL_FILL: &[BlendMode] = &[
    BlendMode::Difference,
    BlendMode::LighterColor,
    BlendMode::HardMix,
];

/// The CMYK claim, unchanged: #2 measured RGB only.
const QUALIFIED_BLEND_CMYK: &[BlendMode] = &[
    BlendMode::Normal,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::PassThrough,
];

/// Each named entry's `{:?}` name once, in stack order.
fn mode_names<'a>(entries: impl Iterator<Item = &'a ExportEntry>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let name = format!("{:?}", entry.blend);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The layered mode's blend and clipping observations: the modes outside
/// the claim, the Fill-limited modes used below 100 % Fill, Dissolve's
/// pattern, a clipping base below 100 % Fill (#2 take2's `c_fill`: no
/// declared clip rule met Photoshop there, 128 codes apart) and a clipped
/// group, which Photoshop reads unclipped from every record form Doctored
/// can write and will not create itself (#2 take2 and take3).
fn blend_observations(inventory: &ExportInventory, observations: &mut Vec<String>) {
    let entries = &inventory.entries;
    if inventory.cmyk {
        let unqualified = mode_names(
            entries
                .iter()
                .filter(|e| !QUALIFIED_BLEND_CMYK.contains(&e.blend)),
        );
        if !unqualified.is_empty() {
            observations.push(format!(
                "Blend modes beyond Normal, Multiply, Screen and Pass Through are used ({}); they are written with Photoshop keys; appearance in other applications is not qualified.",
                unqualified.join(", ")
            ));
        }
    } else {
        let unqualified = mode_names(entries.iter().filter(|e| {
            !QUALIFIED_BLEND.contains(&e.blend) && !QUALIFIED_AT_FULL_FILL.contains(&e.blend)
        }));
        if !unqualified.is_empty() {
            observations.push(format!(
                "Blend modes outside the qualified set are used ({}); they are written with Photoshop keys; their appearance in Photoshop is not qualified.",
                unqualified.join(", ")
            ));
        }
        let below_full_fill = mode_names(entries.iter().filter(|e| {
            QUALIFIED_AT_FULL_FILL.contains(&e.blend)
                && e.style.as_ref().is_some_and(|s| s.fill_opacity < 1.0)
        }));
        if !below_full_fill.is_empty() {
            observations.push(format!(
                "Blend modes qualified in Photoshop only at 100 % Fill are used below it ({}); their appearance in Photoshop is not qualified there.",
                below_full_fill.join(", ")
            ));
        }
    }
    if entries.iter().any(|e| e.blend == BlendMode::Dissolve) {
        observations.push(
            "Dissolve's dither is Doctored's own; Photoshop draws its own pattern at the same density."
                .into(),
        );
    }
    for entry in entries {
        if entry
            .style
            .as_ref()
            .is_some_and(|s| s.clip_base && s.fill_opacity < 1.0)
        {
            observations.push(format!(
                "Layer “{}” is a clipping base below 100 % Fill; the layers clipped to it are not qualified in Photoshop.",
                entry.name
            ));
        }
    }
    for entry in entries.iter().filter(|e| e.group && e.clipped) {
        observations.push(format!(
            "Group “{}” is clipped; it reopens here clipped, but Photoshop reads a clipped group as unclipped and composites it over every layer beneath it.",
            entry.name
        ));
    }
}

fn export_observations(
    inventory: &ExportInventory,
    container: LayeredContainer,
    mode: PsdExportMode,
    observations: &mut Vec<String>,
) {
    if mode == PsdExportMode::Layers {
        for entry in &inventory.entries {
            let Some(adjustment) = &entry.adjustment else {
                continue;
            };
            if let Some(encoded) = &adjustment.encoded {
                observations.push(format!(
                    "Adjustment layer “{}” ({}) is written as a Photoshop {} adjustment ({}) with its mask, opacity, blend key and clipping; its parameters are mapped onto Photoshop's own arithmetic, so its appearance there is not qualified.",
                    entry.name,
                    adjustment.op,
                    encoded.kind,
                    String::from_utf8_lossy(&encoded.key)
                ));
            }
        }
        for entry in &inventory.entries {
            let Some(style) = entry.style.as_ref().filter(|s| s.bake.is_none()) else {
                continue;
            };
            observations.push(format!(
                "Layer “{}” has a layer style ({}); it is written as Photoshop layer effects and blending options (lfx2, iOpa, blending ranges), so Photoshop renders them itself; its appearance there is not qualified.",
                entry.name,
                style.clause(", ")
            ));
        }
        // The two blending-options keys, named per layer beside the style
        // sentence above — a WRITTEN style carries them as Photoshop's own
        // records (12E-3c, E1).
        for entry in &inventory.entries {
            let Some(style) = entry.style.as_ref().filter(|s| s.bake.is_none()) else {
                continue;
            };
            let Some(what) = knockout_clause(style.knockout, style.blend_interior) else {
                continue;
            };
            observations.push(format!(
                "Layer “{}” uses {what}; written as Photoshop's own {}.",
                entry.name,
                knockout_keys(style.knockout, style.blend_interior)
            ));
        }
        blend_observations(inventory, observations);
        observations.push(
            "Layer and group opacity is stored in 1/255 steps, so a value between two steps is rounded to the nearest one."
                .into(),
        );
    }
    observations.push(
        if container == LayeredContainer::Tiff && inventory.lab {
            "The file's flat image is an ordinary CIELab TIFF of the composite over Lab white, with no alpha, as Photoshop writes it; an application that does not read Photoshop's layer block opens that flat image."
        } else if container == LayeredContainer::Tiff {
            "The file's flat image is an ordinary TIFF with straight (unassociated) alpha; an application that does not read Photoshop's layer block opens that flat image."
        } else {
            "The file's composite preview is stored against white, the way every classic writer stores it; faint transparent colours reopened FROM that preview have less detail than the layers themselves."
        }
        .into(),
    );
    let spots = spots_in_file(inventory, container);
    if spots != 0 {
        observations.push(format!(
            "{spots} spot channels are written as Photoshop spot channels: name, D50 Lab ink (to 0.01), solidity and coverage."
        ));
    }
    if inventory.lab {
        observations.push(format!(
            "No profile is embedded, as in Photoshop's own Lab files: L*a*b* values are CIE D50 by definition. A print resolution of {} x {} PPI is written.",
            inventory.resolution.x, inventory.resolution.y
        ));
        return;
    }
    observations.push(format!(
        "The embedded {} profile “{}” and a print resolution of {} x {} PPI are written.",
        if inventory.cmyk {
            "CMYK"
        } else if inventory.gray {
            "Gray"
        } else {
            "RGB"
        },
        inventory.profile_name,
        inventory.resolution.x,
        inventory.resolution.y
    ));
}
