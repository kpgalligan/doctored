//! Layered PSD import, plus the shared "8-bit RGB and grayscale only" gate
//! that the flat open path (`DrImage::open_bytes`) applies too. Import
//! quirks and fallbacks are documented on [`open_psd`].
//!
//! Clipping is retained on rasters and folders at both depths (`with_clipped`); unrepresentable
//! blend keys become Normal and are reported. The per-construct rule table is
//! `designs/phase-10c-constructs-status.md`, summarized in `GENERAL_LIMITATIONS`: a pixel-less
//! adjustment/fill record is OMITTED with a per-record loss (`GroupRecords::omitted`;
//! `retains_nothing` sends a file of nothing but those to the stored composite), a carried
//! Pattern Fill (`PtFl`), Solid Color Fill (`SoCo`) or Gradient Fill (`GdFl`) becomes its rendered
//! canvas raster with its fill description (`psd_fill_layer`), any other
//! pixel-less record becomes an empty canvas-sized transparent raster with an observation, and
//! text/shape/smart/retained-source records keep their pixels plus their marker loss.

use std::collections::HashMap;
use std::sync::Arc;

use image::RgbaImage;

use crate::blend::BlendMode;
use crate::doc::{DrDocument, Layer, LayerKind, LayerRaster};
use crate::doc_group::{validate_structure, MAX_GROUP_DEPTH};
use crate::icc::IccProfile;
use crate::metadata::{Metadata, Resolution};
use crate::psd_layer_effects::StyleBody;
use crate::style::GlobalLight;

/// The psd crate silently mis-decodes anything but 8-bit RGB or grayscale
/// (CMYK channels land in RGB slots, 16-bit data is read byte-interleaved),
/// so both open paths — the flat `DrImage::open_bytes` composite and the layered
/// import — reject those up front with the same message.
pub(crate) fn check_supported(psd: &psd::Psd, path: &str) -> Result<(), String> {
    if psd.depth() != psd::PsdDepth::Eight
        || !matches!(
            psd.color_mode(),
            psd::ColorMode::Rgb | psd::ColorMode::Grayscale
        )
    {
        return Err(format!(
            "PSD {path}: unsupported {:?} color at depth {:?}; only 8-bit RGB and grayscale PSDs are supported",
            psd.color_mode(),
            psd.depth()
        ));
    }
    Ok(())
}

/// PSD -> DrDocument layer blend mapping, also shared by the admitted folder
/// key mapping. PassThrough on a raster layer becomes Normal. The argument
/// is psd 0.3.5's private `BlendMode` discriminant (the enum is
/// not re-exported, so it cannot be named here — but its values are C-like
/// and cast losslessly): 0 PassThrough, 1 Normal, 2 Dissolve, 3 Darken,
/// 4 Multiply, 5 ColorBurn, 6 LinearBurn, 7 DarkerColor, 8 Lighten,
/// 9 Screen, 10 ColorDodge, 11 LinearDodge, 12 LighterColor, 13 Overlay,
/// 14 SoftLight, 15 HardLight, 16 VividLight, 17 LinearLight, 18 PinLight,
/// 19 HardMix, 20 Difference, 21 Exclusion, 22 Subtract, 23 Divide, 24 Hue,
/// 25 Saturation, 26 Color, 27 Luminosity.
pub(crate) fn map_psd_blend(mode: i32) -> BlendMode {
    match mode {
        2 => BlendMode::Dissolve,
        3 => BlendMode::Darken,
        4 => BlendMode::Multiply,
        5 => BlendMode::ColorBurn,
        6 => BlendMode::LinearBurn,
        7 => BlendMode::DarkerColor,
        8 => BlendMode::Lighten,
        9 => BlendMode::Screen,
        10 => BlendMode::ColorDodge,
        11 => BlendMode::Addition, // LinearDodge
        12 => BlendMode::LighterColor,
        13 => BlendMode::Overlay,
        14 => BlendMode::SoftLight,
        15 => BlendMode::HardLight,
        16 => BlendMode::VividLight,
        17 => BlendMode::LinearLight,
        18 => BlendMode::PinLight,
        19 => BlendMode::HardMix,
        20 => BlendMode::Difference,
        21 => BlendMode::Exclusion,
        22 => BlendMode::Subtract,
        23 => BlendMode::Divide,
        24 => BlendMode::Hue,
        25 => BlendMode::Saturation,
        26 => BlendMode::Color,
        27 => BlendMode::Luminosity,
        _ => BlendMode::Normal,
    }
}

/// Layered PSD import. One document layer per PSD raster layer; if the file
/// has no raster layers, reads the saved composite. A failed layer decode
/// refuses and requires an explicit stored-composite choice.
///
/// Layer groups retain structure/names from the decoder, with folder opacity,
/// visibility, open/closed state and recognized blend keys from the independently
/// bounded `psd_group_records` walk. Decoder parent IDs must match that walk.
/// Ordinary absolute raster masks and group clipping are retained
/// independently. Adjustment and fill records carry no pixels and are omitted
/// unless carried (an adjustment kind, or a Pattern or Solid Color Fill,
/// `psd_fill_layer`);
/// any other pixel-less record becomes an empty canvas-sized raster. Layer
/// locks (`lspf`) are carried through `psd_locks`. Advanced masks and effects
/// remain disclosed by the compatibility report; saved composites always
/// require an explicit choice.
///
/// psd 0.3.5 quirks this code compensates for (verified against the crate
/// sources and real files):
/// - `Psd::layers()` is ordered TOP-to-bottom (its `layer_by_idx` doc comment
///   claims the opposite, but the crate's own renderer consumes it top-down),
///   so the iteration is reversed for our bottom-first stack.
/// - `PsdLayer::visible()` actually returns the record's HIDDEN flag (bit 1
///   of the flags byte set means hidden in real files), so it is negated.
/// - `PsdLayer::is_clipping_mask()` is INVERTED: the crate stores the PSD
///   spec's "Clipping: 0 = base, 1 = non-base" byte as `byte == 0`, so a true
///   there means the layer is a clipping BASE. Our flag is its negation.
/// - `Psd::group_ids_in_order()` is CLOSE (post) order despite its doc
///   comment, and `get_group_sub_layers` returns DESCENDANTS rather than
///   direct children, so neither is used for ordering; the tree is rebuilt
///   from `parent_id()` and the flat layer indices, which are exact.
/// - `PsdLayer::rgba()` clips to a canvas and loses wholly off-canvas pixels.
///   Exact raw channel framing now feeds the separate bounded layer codec seam;
///   the dependency receives a metadata-only view for names/properties/tree.
///
/// A PSD nested deeper than [`MAX_GROUP_DEPTH`] has its over-deep GROUPS
/// dissolved: their layers arrive at the tenth level as siblings, keeping
/// their pixels, names and properties, and only the extra folder rows are
/// dropped, as explicitly disclosed in the report. A dissolved folder's locks
/// are added to each layer it held, so dissolving never unlocks. The
/// bounded record walk refuses pathological nesting above 128 before this
/// recursion. The finished stack goes through `validate_structure` — this is
/// a trust boundary, and a decoder whose ordering surprised us must refuse
/// rather than hand back a stack whose entries would silently vanish.
pub(crate) fn open_psd(bytes: &[u8], path: &str) -> Result<DrDocument, String> {
    crate::psd_import::open(bytes, path, 0)
}

pub(crate) fn import_layers(
    psd: &psd::Psd,
    records: &crate::psd_group_records::GroupRecords<'_>,
    container: &crate::psd_container::Container<'_>,
    path: &str,
) -> Result<DrDocument, String> {
    check_supported(psd, path)?;
    let (cw, ch) = (container.width, container.height);
    let tree = PsdTree::build(psd);
    // The record's carried `lfx2`/`iOpa`/blending ranges as ONE layer style,
    // a carried adjustment record's included (its effects take the
    // mask's shape and the adjustment applies after its Below effects,
    // `style_adjustment`); an omitted record is not built at all.
    let carried_style = |body: &StyleBody| match body {
        StyleBody::Carried(carried) => Some(Arc::new(carried.style.clone())),
        _ => None,
    };
    // A carried solid renders in the file's own profile (sRGB when it has
    // none): its colour is converted into the document's numbers there, as
    // the host draws it. A pattern's texels are document values and take
    // no conversion, so it renders in sRGB, an exact identity.
    let solid_profile = container
        .profile()
        .ok()
        .flatten()
        .unwrap_or_else(IccProfile::srgb);
    let mut layers = Vec::with_capacity(psd.layers().len() + psd.groups().len());
    let mut sources = Vec::with_capacity(layers.capacity());
    let mut failure = None;
    let group_locks = |id: u32| records.groups[id as usize - 1].features.locks();
    tree.emit(None, 0, 0, &group_locks, &mut |node, depth, dissolved| {
        if failure.is_some() {
            return;
        }
        match node {
            Node::Group(id) => layers.push(Layer {
                // Minted on import, like every other reader's entries
                // (`doc_identity`); a PSD carries no Doctored identity.
                id: crate::doc_identity::mint_layer_id(),
                retained_source: None,
                warp: None,
                generated: None,
                // A group has no pixels of its own; the 1x1 transparent buffer
                // is the placeholder `doc_group` documents, and it is private
                // per entry so the style plane cache cannot confuse two of
                // them.
                pixels: LayerRaster::new(RgbaImage::new(1, 1)),
                offset: (0, 0),
                name: psd.groups()[&id].name().to_string(),
                opacity: records.groups[id as usize - 1].opacity,
                blend: records.groups[id as usize - 1].blend,
                visible: records.groups[id as usize - 1].visible,
                mask: None,
                mask_enabled: true,
                mask_properties: Default::default(),
                meta: None,
                // An 8-bit document evaluates group clipping (clip to the entry
                // below), so the folder's own clipping byte is retained.
                clipped: records.groups[id as usize - 1].clipped,
                style: carried_style(&records.group_styles[id as usize - 1]),
                kind: LayerKind::Group,
                depth,
                locks: records.groups[id as usize - 1].features.locks() | dissolved,
                link: 0,
                open: records.groups[id as usize - 1].open,
                // Photoshop's sheet colour travels verbatim (`doc_label`).
                label: records.groups[id as usize - 1].label,
                // Photoshop's `fxrp` (`Layer::reference`).
                reference: records.groups[id as usize - 1].reference,
            }),
            Node::Layer(i) => {
                let l = &psd.layers()[i];
                let raw = &records.layers[i];
                // An adjustment or fill record carries no pixels and no
                // interpretable state: it is omitted here, with its mask, and
                // reported per record by `psd_import::report`.
                if records.omitted(i) {
                    return;
                }
                // A carried adjustment body becomes a canvas-sized adjustment
                // layer: its meta is what the compositor evaluates, and its
                // (transparent) pixels are ignored.
                let fill = records.fill(i);
                let described = fill
                    .map(|fill| crate::psd_fill_layer::description(fill, (cw, ch), None))
                    .transpose();
                let described = match described {
                    Ok(described) => described,
                    Err(reason) => {
                        failure = Some(reason);
                        return;
                    }
                };
                let meta = records
                    .carried(i)
                    .map(|carried| carried.meta.clone())
                    .or(described);
                let style = carried_style(&records.layer_styles[i]);
                // A carried Pattern, Solid Color or Gradient Fill is its
                // rendered canvas raster at the origin (a pattern in sRGB, the
                // others in the file's profile), quantized as the byte
                // artwork publication does.
                // Any other pixel-less record becomes an empty canvas-sized
                // transparent raster at the origin: byte painting never grows
                // a layer, so a smaller rectangle would be unpaintable.
                let (pixels, offset) = if let Some(fill) = fill {
                    let profile = match fill.kind {
                        crate::psd_fill_layer::FillKind::Pattern { .. } => IccProfile::srgb(),
                        _ => solid_profile.clone(),
                    };
                    // An aligned gradient spans its mask's coverage bounds,
                    // the plane decoded as `MaskRecord::attach` decodes it.
                    let rendered =
                        crate::psd_fill_layer::align_frame(fill, records.masks[i], (cw, ch), 1)
                            .and_then(|frame| {
                                crate::psd_fill_layer::render(
                                    fill,
                                    records.layer_references[i],
                                    frame,
                                    (cw, ch),
                                    profile,
                                    crate::job_control::JobContext::NONE,
                                )
                            })
                            .and_then(|image| {
                                image.quantize_with_context(
                                    crate::precision_quantize::PrecisionDither::None,
                                    crate::job_control::JobContext::NONE,
                                )
                            });
                    match rendered {
                        Ok(pixels) => (pixels, (0, 0)),
                        Err(reason) => {
                            failure = Some(reason);
                            return;
                        }
                    }
                } else if raw.is_empty() {
                    (RgbaImage::new(cw, ch), (0, 0))
                } else {
                    match raw.decode(container.color) {
                        Ok(pixels) => (pixels, (raw.bounds[1], raw.bounds[0])),
                        Err(reason) => {
                            failure = Some(reason);
                            return;
                        }
                    }
                };
                layers.push(Layer {
                    id: crate::doc_identity::mint_layer_id(),
                    retained_source: None,
                    warp: None,
                    generated: None,
                    pixels: LayerRaster::new(pixels),
                    offset,
                    name: l.name().to_string(),
                    opacity: f32::from(l.opacity()) / 255.0,
                    blend: map_psd_blend(l.blend_mode() as i32),
                    visible: !l.visible(),
                    // The admitted descriptor attaches the independent mask
                    // after construction. Advanced masks and effects remain
                    // explicit losses. Raster clipping retains its old meaning.
                    mask: None,
                    mask_enabled: true,
                    mask_properties: Default::default(),
                    meta,
                    clipped: !l.is_clipping_mask(),
                    style,
                    kind: LayerKind::Raster,
                    depth,
                    // Photoshop's `lspf`, decoded by `psd_locks` through the
                    // record walk, plus any dissolved folder's bits;
                    // nothing in a PSD corresponds to our link groups.
                    locks: records.layer_features[i].locks() | dissolved,
                    link: 0,
                    open: true,
                    label: records.layer_labels[i],
                    reference: records.layer_references[i],
                });
            }
        }
        // The record this entry was built from, for its layer comp settings
        // (`psd_comps`).
        sources.push(match node {
            Node::Group(id) => crate::psd_comps::CompSource::Group(id),
            Node::Layer(index) => crate::psd_comps::CompSource::Layer(index),
        });
        let mask = match node {
            Node::Group(id) => records.groups[id as usize - 1].mask,
            Node::Layer(index) => records.masks[index],
        };
        if let Some(layer) = layers.last_mut() {
            if let Err(reason) = mask.attach(layer) {
                failure = Some(reason);
            }
        }
    });
    // A decode failure refuses the layer choice. The caller can offer the
    // saved composite explicitly, but must never substitute it silently.
    if let Some(reason) = failure {
        return Err(format!(
            "PSD {path}: {reason}; explicitly choose the stored composite if available"
        ));
    }
    if layers.is_empty() {
        return Err(format!("PSD {path}: no layer pixels could be imported; explicitly choose the stored composite if available"));
    }
    if !validate_structure(&layers) {
        return Err(format!("PSD {path}: malformed layer group structure"));
    }
    let document = DrDocument {
        width: cw,
        height: ch,
        layers,
        global_light: GlobalLight::default(),
        // Saved alpha channels remain unsupported. Container::tag applies
        // the independently retained RGB ICC and print resolution after import.
        channels: Vec::new(),
        // PSD guides and the ruler origin live in image resource 1032, which
        // `psd` 0.3.5 does not expose either, so an imported document arrives
        // with no guides and its ruler zero at the canvas's top-left.
        guides: Vec::new(),
        ruler_origin: (0.0, 0.0),
        // Image resource 1073's samplers are set by `Container::tag`.
        color_samplers: Vec::new(),
        // A PSD's path resources (2000–2997, and 1025 for the work path) are
        // set by `Container::tag` (`psd_paths`); the vector masks below.
        paths: crate::doc_path::DocPaths::default(),
        profile: IccProfile::srgb(),
        metadata: Metadata::default(),
        resolution: Resolution::default(),
        // An imported PSD is an ENCODED document; the policy is only ever
        // chosen deliberately (`doc_evaluation`).
        linear_light: None,
        // Set below from resource 1065 and each record's `cmls`
        // (`psd_comps`).
        comps: Vec::new(),
        // Set below from record 0 (`psd_background`).
        background: None,
    };
    let mut document = document;
    // The vector masks the walk carried (`psd_paths`), each on the entry
    // built from its record. The walk counted them against the file's paths,
    // which `Container::tag` sets afterwards (`psd_paths::MaskRoom`), so the
    // two together fit the document's anchor cap.
    for (k, &source) in sources.iter().enumerate() {
        if let Some(body) = crate::psd_paths::source_vmsk(records, source) {
            document = document.importing_vector_mask(k, body);
        }
    }
    if container.color == 3 {
        document.comps = crate::psd_comps::imported(container, records, &sources, |k| {
            document.layers.get(k).map(|l| (l.id, l.name.clone()))
        });
    }
    // FILE record 0, when kept, is entry 0 (`psd_background::import_role`).
    if let Some(role) = crate::psd_background::import_role(records) {
        document.mark_background(0, role);
    }
    document.native_save_preflight()?;
    Ok(document)
}

/// One node of a PSD's layer tree: a raster layer at its flat `layers()`
/// index, or a group by its crate id.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Node {
    Layer(usize),
    Group(u32),
}

/// The PSD's tree, rebuilt from `parent_id()` alone: each parent's children in
/// BOTTOM-FIRST order, which is the order our stack wants.
///
/// The ordering key is each node's position in the crate's TOP-DOWN
/// `layers()` list — a layer is at its own flat index, and a group's folder
/// row sits immediately above its contents, so a group takes the flat index
/// its descendants begin at, with groups before the layer at the same index
/// and nested groups outermost first (ids are assigned in top-down OPEN
/// order, so the outer folder gets the smaller id). Reversing that key gives
/// bottom-first.
pub(crate) struct PsdTree {
    children: HashMap<Option<u32>, Vec<Node>>,
}

/// One node with its TOP-DOWN sort key: `(position, groups-first, group id)`.
type Keyed = (usize, u8, u32, Node);

impl PsdTree {
    pub(crate) fn build(psd: &psd::Psd) -> Self {
        let mut keyed: HashMap<Option<u32>, Vec<Keyed>> = HashMap::new();
        for (i, layer) in psd.layers().iter().enumerate() {
            keyed
                .entry(layer.parent_id())
                .or_default()
                // 1 = "after a group at the same index": the folder row comes
                // first in top-down order.
                .push((i, 1, 0, Node::Layer(i)));
        }
        for (&id, group) in psd.groups() {
            let start = group_start(psd, id);
            keyed
                .entry(group.parent_id())
                .or_default()
                .push((start, 0, id, Node::Group(id)));
        }
        let children = keyed
            .into_iter()
            .map(|(parent, mut nodes)| {
                nodes.sort_by_key(|&(start, group_last, id, _)| (start, group_last, id));
                // Top-down -> bottom-first.
                nodes.reverse();
                (parent, nodes.into_iter().map(|(_, _, _, n)| n).collect())
            })
            .collect();
        PsdTree { children }
    }

    /// Walks `parent`'s children bottom-first, emitting each node — a group's
    /// descendants first, then the group's own record, which is exactly
    /// `doc_group`'s layout. A group that would push its children past
    /// [`MAX_GROUP_DEPTH`] is DISSOLVED: its children take the cap's depth as
    /// siblings and its folder row is dropped, so the invariant holds and only
    /// the container is lost. A dissolved folder's lock bits (`group_locks`)
    /// are not lost with it: `out`'s third argument is the OR of every
    /// dissolved ancestor's bits, which the caller adds to each node's own.
    /// A kept group is never inside a dissolved one, so only layers
    /// ever receive a nonzero value.
    pub(crate) fn emit(
        &self,
        parent: Option<u32>,
        depth: u16,
        dissolved_locks: u32,
        group_locks: &impl Fn(u32) -> u32,
        out: &mut impl FnMut(Node, u16, u32),
    ) {
        let Some(nodes) = self.children.get(&parent) else {
            return;
        };
        for &node in nodes {
            match node {
                Node::Layer(_) => out(node, depth.min(MAX_GROUP_DEPTH), dissolved_locks),
                Node::Group(id) if depth >= MAX_GROUP_DEPTH => self.emit(
                    Some(id),
                    MAX_GROUP_DEPTH,
                    dissolved_locks | group_locks(id),
                    group_locks,
                    out,
                ),
                Node::Group(id) => {
                    self.emit(Some(id), depth + 1, dissolved_locks, group_locks, out);
                    out(node, depth, dissolved_locks);
                }
            }
        }
    }
}

/// The flat `layers()` index group `id`'s contents begin at — the position its
/// folder row occupies in the crate's TOP-DOWN ordering, since a folder row
/// sits immediately above its contents.
///
/// `get_group_sub_layers` returns `&layers[range]`, so even for an EMPTY group
/// (length 0) the slice's ADDRESS is `layers.as_ptr() + range.start`, which is
/// the only way this crate exposes an empty group's position at all — it has
/// no descendants to be anchored by. The offset is computed in `usize` (so a
/// slice from anywhere else wraps instead of misbehaving) and clamped to the
/// layer count, which parks an unplaceable group at the bottom of its parent;
/// it has no pixels, so nothing about the picture depends on it.
fn group_start(psd: &psd::Psd, id: u32) -> usize {
    let layers = psd.layers();
    let Some(first) = layers.first() else {
        return 0;
    };
    let Some(sub) = psd.get_group_sub_layers(&id) else {
        return layers.len();
    };
    let stride = std::mem::size_of_val(first);
    let base = layers.as_ptr() as usize;
    ((sub.as_ptr() as usize).wrapping_sub(base) / stride.max(1)).min(layers.len())
}
