//! The typed rendered-plane cache: the 16-bit and 32-bit float twin of
//! `style_cache`'s pointer-keyed entries, held beside them in each
//! `Arc<LayerStyle>`'s `RenderCache` (its second half, [`TypedCache`]). The
//! typed renderer produces the same `RenderedEffects` over the same u8
//! `Shape` (`precision_style_render`'s shape-quantized policy), so a typed
//! entry holds exactly what a byte entry holds, and an edit that leaves a
//! layer's style alone keeps its `Arc` and so its entries.
//!
//! # Key
//! The byte key's rule, one depth up:
//! - **Pixels.** A raster's `PrecisionImage::identity_key()` (a `Weak` of its
//!   identity token: one identity is one set of samples) with its storage and
//!   dimensions; a styled adjustment's opaque frame (dimensions and origin).
//! - **Mask.** The plane's `Weak<Vec<u16>>` with the enabled flag, density,
//!   feather, outside coverage and the plane's offset relative to the pixels.
//! - **Context.** The global light only when the style reads it, the canvas
//!   and the layer offset only under a canvas-aligned gradient or unlinked
//!   pattern tile (`CacheKey::rekeyed`'s rule), and `lab_tiles`.
//!
//! `Weak`s compare by address and never pin samples (`style_cache`'s module
//! doc says why the address cannot be reused while the entry lives).
//!
//! # Lookup, bound and eviction
//! [`rendered`] is the one entry both typed sites call
//! (`precision_style_render::composite_styled_into`,
//! `precision_regional_tree::prepare_style`). A hit moves its entry to the
//! front and builds no shape. A miss builds the shape and renders OUTSIDE
//! the lock, then stores the planes, so a cancelled render (`Err`) caches
//! nothing. `style_cache::CACHE_CAPACITY` entries, most recent first; on
//! every store the entries whose pixels or mask plane have died are pruned,
//! and so is any entry the new key matches.
//!
//! # Not cached
//! Each of these renders in full on every composite, as before:
//! - a styled GROUP (its pixels are a rendered f32 buffer with no identity);
//! - a CMYK layer (`cmyk_style`, which keys on a CMYK image);
//! - the byte cache's brush-dab patch path and shape-equality refresh, which
//!   need a typed shape before the lookup;
//! - plane inheritance across a style edit: the typed `layer_style` edit
//!   DRAINS the replaced style's cache instead (`LayerStyleEdit::apply`).
//!
//! # Accounting
//! The planes are disposable cache: `RenderCache::resource_snapshot` reports
//! them under `style_cache_planes` / `style_cache_structure`, and
//! `resources::evict_caches` clears them, for every style [`styles_of`] finds
//! in the supplied typed documents.
use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_image::PrecisionStorage;
use crate::precision_mask::PreparedMask;
use crate::precision_style_render::{typed_shape, StyledPixels};
use crate::style::{GlobalLight, LayerStyle};
use crate::style_cache::CACHE_CAPACITY;
use crate::style_render::{render_effects_with_job, RenderContext, RenderedEffects};

/// The sentence a plane past `MAX_PIXELS` refuses with (both typed sites).
pub(crate) const PLANE_LIMIT: &str = "typed layer style plane exceeds the pixel limit";

/// The pixels' part of the key (module doc, `# Key`).
enum PixelsKey {
    Image {
        identity: Weak<()>,
        storage: PrecisionStorage,
        dimensions: (u32, u32),
    },
    Opaque {
        dimensions: (u32, u32),
        origin: (i32, i32),
    },
}

/// The mask's part of the key.
struct MaskKey {
    plane: Weak<Vec<u16>>,
    enabled: bool,
    density: u32,
    feather: u32,
    outside: u8,
    relative_offset: (i64, i64),
}

struct TypedKey {
    pixels: PixelsKey,
    mask: Option<MaskKey>,
    light: Option<GlobalLight>,
    canvas: Option<(u32, u32)>,
    offset: Option<(i32, i32)>,
    lab_tiles: bool,
}

impl TypedKey {
    /// `None` for pixels with no identity (a group's buffer: module doc).
    fn of(
        style: &LayerStyle,
        pixels: &StyledPixels<'_>,
        mask: Option<&PreparedMask<'_>>,
        ctx: &RenderContext,
    ) -> Option<Self> {
        let origin = pixels.offset();
        let pixels = match pixels {
            StyledPixels::Image { image, .. } => PixelsKey::Image {
                identity: image.identity_key(),
                storage: image.storage(),
                dimensions: image.dimensions(),
            },
            StyledPixels::Opaque { width, height, .. } => PixelsKey::Opaque {
                dimensions: (*width, *height),
                origin,
            },
            StyledPixels::Buffer { .. } => return None,
        };
        let mask = mask.map(|prepared| {
            let source = prepared.source();
            let properties = source.properties();
            MaskKey {
                plane: Arc::downgrade(&source.plane().pixels),
                enabled: source.enabled(),
                density: properties.density.to_bits(),
                feather: properties.feather.to_bits(),
                outside: properties.outside_coverage,
                relative_offset: (
                    i64::from(properties.offset.0) - i64::from(origin.0),
                    i64::from(properties.offset.1) - i64::from(origin.1),
                ),
            }
        });
        let aligned = style.needs_canvas_alignment();
        Some(TypedKey {
            pixels,
            mask,
            light: style.uses_global_light().then_some(ctx.light),
            canvas: aligned.then_some(ctx.canvas),
            offset: aligned.then_some(ctx.layer_offset),
            lab_tiles: ctx.lab_tiles,
        })
    }

    /// Pointer equality on the `Weak`s, value equality on the rest.
    fn matches(&self, other: &TypedKey) -> bool {
        let pixels = match (&self.pixels, &other.pixels) {
            (
                PixelsKey::Image {
                    identity: a,
                    storage: sa,
                    dimensions: da,
                },
                PixelsKey::Image {
                    identity: b,
                    storage: sb,
                    dimensions: db,
                },
            ) => Weak::ptr_eq(a, b) && sa == sb && da == db,
            (
                PixelsKey::Opaque {
                    dimensions: da,
                    origin: oa,
                },
                PixelsKey::Opaque {
                    dimensions: db,
                    origin: ob,
                },
            ) => da == db && oa == ob,
            _ => false,
        };
        let mask = match (&self.mask, &other.mask) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                Weak::ptr_eq(&a.plane, &b.plane)
                    && a.enabled == b.enabled
                    && a.density == b.density
                    && a.feather == b.feather
                    && a.outside == b.outside
                    && a.relative_offset == b.relative_offset
            }
            _ => false,
        };
        pixels
            && mask
            && self.light == other.light
            && self.canvas == other.canvas
            && self.offset == other.offset
            && self.lab_tiles == other.lab_tiles
    }

    /// Whether the pixels or the mask plane this entry was rendered for no
    /// longer exist anywhere, so nothing can match it again.
    fn is_dead(&self) -> bool {
        let pixels = match &self.pixels {
            PixelsKey::Image { identity, .. } => identity.strong_count() == 0,
            PixelsKey::Opaque { .. } => false,
        };
        pixels
            || self
                .mask
                .as_ref()
                .is_some_and(|m| m.plane.strong_count() == 0)
    }
}

struct TypedEntry {
    key: TypedKey,
    planes: Arc<RenderedEffects>,
}

/// The typed half of a style's `RenderCache` (module doc).
#[derive(Default)]
pub(crate) struct TypedCache(Mutex<Vec<TypedEntry>>);

impl TypedCache {
    fn lock(&self) -> MutexGuard<'_, Vec<TypedEntry>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every entry's planes, pinned for resource accounting.
    pub(crate) fn planes(&self) -> Vec<Arc<RenderedEffects>> {
        self.lock().iter().map(|e| Arc::clone(&e.planes)).collect()
    }

    pub(crate) fn clear(&self) {
        self.lock().clear();
    }

    /// The matching entry's planes, moved to the front.
    fn hit(&self, key: &TypedKey) -> Option<Arc<RenderedEffects>> {
        let mut entries = self.lock();
        let i = entries.iter().position(|e| key.matches(&e.key))?;
        let entry = entries.remove(i);
        let planes = Arc::clone(&entry.planes);
        entries.insert(0, entry);
        Some(planes)
    }

    /// Stores `planes` for `key` most recent first (module doc, `# Lookup`).
    fn store(&self, key: TypedKey, planes: Arc<RenderedEffects>) {
        let mut entries = self.lock();
        entries.retain(|e| !e.key.is_dead() && !e.key.matches(&key));
        entries.insert(0, TypedEntry { key, planes });
        entries.truncate(CACHE_CAPACITY);
    }
}

/// The planes `style` renders over `pixels` under `mask` and `ctx`: from the
/// style's typed cache when the key matches, else the typed shape rendered
/// under `job` and stored (module doc). Refuses with [`PLANE_LIMIT`] when the
/// padded plane would exceed `MAX_PIXELS`.
pub(crate) fn rendered(
    style: &LayerStyle,
    pixels: &StyledPixels<'_>,
    mask: Option<&PreparedMask<'_>>,
    ctx: &RenderContext,
    job: JobContext<'_>,
) -> Result<Arc<RenderedEffects>, String> {
    let key = TypedKey::of(style, pixels, mask, ctx);
    if let Some(planes) = key.as_ref().and_then(|key| style.cache.typed().hit(key)) {
        return Ok(planes);
    }
    let shape = typed_shape(pixels, mask, style.pad()).ok_or(PLANE_LIMIT)?;
    job.check()?;
    let planes = Arc::new(render_effects_with_job(style, shape, ctx, job)?);
    if let Some(key) = key {
        style.cache.typed().store(key, Arc::clone(&planes));
    }
    Ok(planes)
}

/// Every distinct style (by `Arc`) of `documents`' layers, groups' children
/// included: what `resources` reports and evicts.
pub(crate) fn styles_of<'a>(documents: &[&'a PrecisionDocument]) -> Vec<&'a LayerStyle> {
    fn walk<'a>(
        layers: &'a [PrecisionDocumentLayer],
        seen: &mut HashSet<usize>,
        out: &mut Vec<&'a LayerStyle>,
    ) {
        for layer in layers {
            if let Some(style) = &layer.style {
                if seen.insert(Arc::as_ptr(style) as usize) {
                    out.push(style);
                }
            }
            if let Content::Group(children) | Content::PassThrough(children) = &layer.content {
                walk(children, seen, out);
            }
        }
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for document in documents {
        walk(&document.layers, &mut seen, &mut out);
    }
    out
}
