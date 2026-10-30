//! The ONE CMYK row compositor (#1 phases 1, 2 and 4a, and the styles
//! batch): a CMYK document's visible record TREE, bottom first, composited
//! into one row of straight f32 ink. Every projection (`cmyk_render`), the
//! flatten (`cmyk_convert`), the savers (`cmyk_io`), the sampler and the
//! merges (`cmyk_merge`) read rows through it. Rows are addressed in CANVAS
//! coordinates as `i64`, so a merge reads a frame that reaches past the
//! canvas.
//!
//! * A RASTER record composites through its blend mode
//!   (`cmyk_composite::over_mode`; Normal is `over`) at
//!   `sa = alpha × opacity × mask(x, y) × fill`, the mask read through
//!   `PreparedMask::at` (the RGB stack's own mask coverage, feather and
//!   density included) at canvas coordinates; `fill` is a fill-only style's
//!   Fill Opacity (1 without one), multiplied in exactly as
//!   `precision_style_render::composite_pixels_into` does.
//! * An ADJUSTMENT record (`cmyk_adjust_layer`) moves every composited
//!   sample below it with any alpha toward its tone by
//!   `k = opacity × mask(x, y)` through its mode
//!   (`cmyk_composite::tone_mix_mode`; Normal is `tone_mix`), alpha kept; a
//!   sample with no alpha is left alone, as the RGB stack leaves a
//!   noncontributing backdrop (`precision_render::adjust_backdrop_sample16`).
//!   The tone is `cmyk_adjust::CmykAdjustment::apply` on the ink at the
//!   document's depth (Levels, Curves, drawn curves and Hue/Saturation alike).
//! * An ISOLATED (Normal) group composites its children into a row buffer
//!   of its own, starting transparent, then that buffer through its mode at
//!   `alpha × opacity × fill × mask`. A PASS THROUGH group composites its
//!   children over a copy of the row and gates the result back by
//!   `opacity × mask` (`cmyk_composite::gate`) — `precision_render`'s two
//!   group arms. A Pass Through group carrying a style or Blend If
//!   composites as an isolated Normal group, its Fill Opacity included
//!   (`PrecisionDocumentLayer::isolates`).
//! * A CLIP UNIT — an unclipped record with the visible clipped records
//!   directly above it in its level (`precision_document::render_layers`'
//!   grouping): the base is drawn into a transparent buffer at Normal and
//!   full opacity with `alpha × mask × fill`, its FOOTPRINT is
//!   `alpha × mask` without the fill, each member composites onto the
//!   buffer with the base's alpha frozen relative to the footprint
//!   (`clip_unit`'s Open / Settle / Close, #2), and the buffer composites
//!   once through the base's mode at the base's opacity — the styled clip
//!   base's rule in `precision_style_render`, so the base's Fill stays
//!   relative and a member shows inside the base's unfilled alpha. A Pass
//!   Through base with a visible member, and a clipped Pass Through member,
//!   composite as isolated groups; a clipped record with no base below it in
//!   its level composites as if unclipped; a hidden base drops its whole
//!   unit.
//! * A STYLED record (the styles batch, `cmyk_style::needs_styled`: a raster
//!   or a group of either kind with an enabled layer effect, a Blend If or a
//!   knockout) composites through the row kernel `cmyk_style_row` — D3's
//!   order, its planes rendered once when the stack is built
//!   (`cmyk_style::styled_ink`); a styled GROUP's children are rendered into
//!   an ink frame first, over its extent within the stack's bounds. A styled
//!   clip BASE takes its members into the kernel's unit.
//! * A STYLED ADJUSTMENT (any style or Blend If on a CMYK
//!   adjustment record) is a styled record too: its shape is opaque over the
//!   stack's bounds under its mask (S-MASK) and the kernel applies its tone
//!   after its Below effects (POST), `cmyk_style_row`'s adjustment interior.
//!   An unstyled adjustment keeps the plain path above.
//!
//! # Knockout (D4, D5)
//!
//! `style_knockout` owns the rules; on the row they read: an ISOLATED
//! buffer's children (a group, a styled group's frame, a clip unit's base)
//! start with a transparent backdrop; a clip MEMBER punches to the clip
//! buffer after the base alone drew; SHALLOW inside a Pass Through group to
//! the row as the children began, Deep riding through; at the ROOT, when a
//! visible record asks for a knockout and record 0 qualifies as the
//! Background (`precision_style_knockout::background_record`, a CMYK raster
//! included), both to the row right after record 0, else transparent.
//! [`CmykStack::row_seeded`] instead hands the root a seed (a merge's
//! members, D7: never D5), and [`CmykStack::capture_at`] records the
//! backdrop the level holding one record reads, row by row — the merge's
//! capture, `precision_style_knockout::render_projection` its master.
//!
//! A disabled mask is skipped. The masks are prepared ONCE per stack
//! (`PrecisionMask::prepare`: a feathered mask blurs then), so a caller
//! builds one stack and reads every row through it; [`stack_scratch`]
//! charges the preparation, `PrecisionMask::plan`'s figure summed over the
//! contributing masks, the row buffers of every group and clip unit, every
//! styled record's `cmyk_style::styled_scratch` and a 20 B row for each
//! knockout backdrop copy (the Background row, a clip unit's base-only row),
//! which every plan that builds a stack adds.

use crate::blend::BlendMode;
use crate::clip_unit::{clip_pass, ClipStep};
use crate::cmyk_adjust::CmykAdjustment;
use crate::cmyk_composite::{gate, over, over_mode, tone_mix_mode, unit, InkPixel};
use crate::cmyk_document::CmykPolicy;
use crate::cmyk_image::CmykImage;
use crate::cmyk_style::{needs_styled, styled_ink, styled_scratch, StyledSource};
use crate::cmyk_style_row::{composite_styled_row, StyledInk};
use crate::job_control::JobContext;
use crate::precision_document::{Content, PrecisionDocument, PrecisionDocumentLayer};
use crate::precision_mask::PreparedMask;
use crate::style::{GlobalLight, Knockout};
use crate::style_composite::Cut;

/// The backdrop rows a knockout punches to (D4), each aligned with the row
/// in play; `None` is transparent.
#[derive(Clone, Copy)]
pub(crate) struct KnockoutSeed<'s> {
    pub shallow: Option<&'s [InkPixel]>,
    pub deep: Option<&'s [InkPixel]>,
}

impl KnockoutSeed<'_> {
    /// Both transparent: every isolated buffer and every stack with no
    /// Background.
    pub(crate) const NONE: KnockoutSeed<'static> = KnockoutSeed {
        shallow: None,
        deep: None,
    };
}

/// One contributing record (or clip unit) as the compositor reads it.
enum Entry<'a> {
    Raster {
        id: u64,
        image: &'a CmykImage,
        offset: (i32, i32),
        opacity: f32,
        fill: f32,
        mode: BlendMode,
        mask: Option<PreparedMask<'a>>,
    },
    Tone {
        id: u64,
        tone: &'a CmykAdjustment,
        depth_bits: u32,
        opacity: f32,
        mode: BlendMode,
        mask: Option<PreparedMask<'a>>,
    },
    Group {
        id: u64,
        children: Vec<Entry<'a>>,
        opacity: f32,
        fill: f32,
        mode: BlendMode,
        mask: Option<PreparedMask<'a>>,
        buf: Vec<InkPixel>,
    },
    Pass {
        id: u64,
        children: Vec<Entry<'a>>,
        opacity: f32,
        mask: Option<PreparedMask<'a>>,
        buf: Vec<InkPixel>,
    },
    Clip {
        base: Box<Entry<'a>>,
        members: Vec<Entry<'a>>,
        buf: Vec<InkPixel>,
        footprint: Vec<f32>,
        /// D4: some member punches, so the base-only buffer is kept.
        members_want_base: bool,
        base_only: Vec<InkPixel>,
    },
    Styled {
        id: u64,
        styled: Box<StyledInk>,
        source: StyledSource<'a>,
        mask: Option<PreparedMask<'a>>,
        members: Vec<Entry<'a>>,
        members_want_base: bool,
        base_only: Vec<InkPixel>,
        scratch: Vec<InkPixel>,
    },
}

/// The merge capture (module doc): the backdrop rows the level holding the
/// record `id` reads, first visit wins.
struct Capture {
    id: u64,
    filled: bool,
    shallow: Option<Vec<InkPixel>>,
    deep: Option<Vec<InkPixel>>,
}

impl Capture {
    fn offer(&mut self, entry: &Entry<'_>, env: KnockoutSeed<'_>) {
        if self.filled || !holds(entry, self.id) {
            return;
        }
        self.filled = true;
        self.shallow = env.shallow.map(<[InkPixel]>::to_vec);
        self.deep = env.deep.map(<[InkPixel]>::to_vec);
    }
}

/// Whether `entry` is the record `id`, or a clip unit holding it.
fn holds(entry: &Entry<'_>, id: u64) -> bool {
    match entry {
        Entry::Raster { id: own, .. }
        | Entry::Tone { id: own, .. }
        | Entry::Group { id: own, .. }
        | Entry::Pass { id: own, .. } => *own == id,
        Entry::Styled {
            id: own, members, ..
        } => *own == id || members.iter().any(|m| holds(m, id)),
        Entry::Clip { base, members, .. } => {
            holds(base, id) || members.iter().any(|m| holds(m, id))
        }
    }
}

/// A prepared stack (module doc).
pub(crate) struct CmykStack<'a> {
    entries: Vec<Entry<'a>>,
    /// D5: record 0 is the Background a knockout punches to.
    background: bool,
    background_row: Vec<InkPixel>,
    capture: Option<Capture>,
}

/// What building a stack needs beside the layers.
struct Build<'a> {
    document: &'a PrecisionDocument,
    policy: &'a CmykPolicy,
    depth_bits: u32,
    light: GlobalLight,
    canvas: (u32, u32),
    /// The canvas rect the stack's rows are read over: a styled group's
    /// frame is rendered within it.
    bounds: (i64, i64, i64, i64),
}

fn prepared<'a>(
    layer: &'a PrecisionDocumentLayer,
    job: JobContext<'_>,
) -> Result<Option<PreparedMask<'a>>, String> {
    match layer.mask() {
        Some(mask) if mask.enabled() => Ok(Some(mask.prepare(job)?)),
        _ => Ok(None),
    }
}

/// A record's Fill Opacity: its fill-only style's, else 1.
fn fill_of(layer: &PrecisionDocumentLayer) -> f32 {
    layer.style().map_or(1.0, |style| {
        crate::opacity_byte::quantized(style.fill_opacity)
    })
}

/// Whether some visible member of a clip unit punches (D4).
fn wants_base(members: &[&PrecisionDocumentLayer]) -> bool {
    members
        .iter()
        .any(|m| m.visible() && m.style().is_some_and(|s| s.knockout != Knockout::None))
}

/// A styled record (module doc) with `members` its clip members.
fn styled<'a>(
    layer: &'a PrecisionDocumentLayer,
    members: Vec<Entry<'a>>,
    members_want_base: bool,
    ctx: &Build<'_>,
    job: JobContext<'_>,
) -> Result<Entry<'a>, String> {
    let mask = prepared(layer, job)?;
    let source = match &layer.content {
        Content::CmykRaster { image, offset } => StyledSource::Raster {
            image,
            offset: *offset,
        },
        Content::Group(children) | Content::PassThrough(children) => {
            group_frame(layer, children, ctx, job)?
        }
        Content::CmykAdjustment(_) => opaque_over(ctx.bounds)?,
        _ => return Err(crate::cmyk_document::refused_sentence("A non-CMYK record")),
    };
    let ink = styled_ink(
        layer,
        &source,
        mask.as_ref(),
        ctx.policy,
        ctx.light,
        ctx.canvas,
        job,
    )?;
    Ok(Entry::Styled {
        id: layer.id(),
        styled: Box::new(ink),
        source,
        mask,
        members,
        members_want_base,
        base_only: Vec::new(),
        scratch: Vec::new(),
    })
}

/// A styled adjustment's shape (S-MASK): opaque over the stack's read rect
/// `bounds` (`Build::bounds`: the canvas, or a merge's frame).
fn opaque_over(bounds: (i64, i64, i64, i64)) -> Result<StyledSource<'static>, String> {
    let range = || "a styled CMYK adjustment lies outside the coordinate range".to_string();
    let (x0, y0, x1, y1) = bounds;
    Ok(StyledSource::Opaque {
        origin: (
            i32::try_from(x0).map_err(|_| range())?,
            i32::try_from(y0).map_err(|_| range())?,
        ),
        dims: (
            u32::try_from((x1 - x0).max(0)).map_err(|_| range())?,
            u32::try_from((y1 - y0).max(0)).map_err(|_| range())?,
        ),
    })
}

/// A styled group's children rendered into an ink frame over their extent
/// within the stack's bounds (module doc), isolated: a transparent backdrop.
fn group_frame<'a>(
    layer: &PrecisionDocumentLayer,
    children: &'a [PrecisionDocumentLayer],
    ctx: &Build<'_>,
    job: JobContext<'_>,
) -> Result<StyledSource<'a>, String> {
    let extent = children
        .iter()
        .filter_map(|child| crate::precision_merge_extent::record_extent16(child, ctx.document))
        .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)));
    let (x0, y0, x1, y1) = match extent {
        Some(e) => (
            e.0.max(ctx.bounds.0),
            e.1.max(ctx.bounds.1),
            e.2.min(ctx.bounds.2),
            e.3.min(ctx.bounds.3),
        ),
        None => (0, 0, 0, 0),
    };
    let (w, h) = ((x1 - x0).max(0) as u64, (y1 - y0).max(0) as u64);
    if w.saturating_mul(h) > crate::doc::MAX_PIXELS {
        return Err(format!(
            "Layer “{}” with its layer style covers {w}×{h} pixels, over the pixel limit",
            layer.name()
        ));
    }
    let mut entries = level(children, ctx, job)?;
    let mut pixels: Vec<InkPixel> = crate::precision_image::reserved_vec((w * h) as usize)?;
    let mut row = vec![[0f32; 5]; w as usize];
    for y in y0..y0 + h as i64 {
        job.check()?;
        row.fill([0.0; 5]);
        let span = Span { x0, step: 1 };
        composite_level(
            &mut entries,
            y,
            span,
            &mut row,
            KnockoutSeed::NONE,
            &mut None,
        );
        pixels.extend_from_slice(&row);
    }
    let origin = if w == 0 || h == 0 {
        (0, 0)
    } else {
        (
            i32::try_from(x0)
                .map_err(|_| "a styled CMYK group lies outside the coordinate range")?,
            i32::try_from(y0)
                .map_err(|_| "a styled CMYK group lies outside the coordinate range")?,
        )
    };
    Ok(StyledSource::Frame {
        pixels,
        origin,
        dims: (w as u32, h as u32),
    })
}

/// One record as an entry; `isolate` composites a Pass Through group as an
/// isolated Normal one (a clip unit's base or member, module doc), as does a
/// Pass Through group's own style (`PrecisionDocumentLayer::isolates`).
fn entry<'a>(
    layer: &'a PrecisionDocumentLayer,
    isolate: bool,
    ctx: &Build<'_>,
    job: JobContext<'_>,
) -> Result<Entry<'a>, String> {
    if needs_styled(layer) {
        return styled(layer, Vec::new(), false, ctx, job);
    }
    let id = layer.id();
    Ok(match &layer.content {
        Content::CmykRaster { image, offset } => Entry::Raster {
            id,
            image,
            offset: *offset,
            opacity: crate::opacity_byte::quantized(layer.opacity()),
            fill: fill_of(layer),
            mode: layer.blend_mode,
            mask: prepared(layer, job)?,
        },
        Content::CmykAdjustment(state) => Entry::Tone {
            id,
            tone: state.kernel(),
            depth_bits: ctx.depth_bits,
            opacity: crate::opacity_byte::quantized(layer.opacity()),
            mode: layer.blend_mode,
            mask: prepared(layer, job)?,
        },
        Content::Group(children) => Entry::Group {
            id,
            children: level(children, ctx, job)?,
            opacity: crate::opacity_byte::quantized(layer.opacity()),
            fill: fill_of(layer),
            mode: layer.blend_mode,
            mask: prepared(layer, job)?,
            buf: Vec::new(),
        },
        Content::PassThrough(children) if isolate || layer.isolates() => Entry::Group {
            id,
            children: level(children, ctx, job)?,
            opacity: crate::opacity_byte::quantized(layer.opacity()),
            fill: fill_of(layer),
            mode: BlendMode::Normal,
            mask: prepared(layer, job)?,
            buf: Vec::new(),
        },
        Content::PassThrough(children) => Entry::Pass {
            id,
            children: level(children, ctx, job)?,
            opacity: crate::opacity_byte::quantized(layer.opacity()),
            mask: prepared(layer, job)?,
            buf: Vec::new(),
        },
        _ => return Err(crate::cmyk_document::refused_sentence("A non-CMYK record")),
    })
}

/// One level's visible records as entries, bottom first, with its clip
/// units (module doc).
fn level<'a>(
    layers: &'a [PrecisionDocumentLayer],
    ctx: &Build<'_>,
    job: JobContext<'_>,
) -> Result<Vec<Entry<'a>>, String> {
    let mut out = Vec::with_capacity(layers.len());
    let mut i = 0;
    while i < layers.len() {
        let layer = &layers[i];
        if layer.clipped() {
            // Baseless: only reachable at the bottom of a level.
            if layer.visible() {
                out.push(entry(layer, false, ctx, job)?);
            }
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < layers.len() && layers[end].clipped() {
            end += 1;
        }
        if layer.visible() {
            let members: Vec<&PrecisionDocumentLayer> = layers[i + 1..end]
                .iter()
                .filter(|member| member.visible())
                .collect();
            if members.is_empty() {
                out.push(entry(layer, false, ctx, job)?);
            } else {
                let want = wants_base(&members);
                let built = members
                    .into_iter()
                    .map(|member| entry(member, true, ctx, job))
                    .collect::<Result<Vec<_>, String>>()?;
                if needs_styled(layer) {
                    out.push(styled(layer, built, want, ctx, job)?);
                } else {
                    let base = entry(layer, true, ctx, job)?;
                    out.push(Entry::Clip {
                        base: Box::new(base),
                        members: built,
                        buf: Vec::new(),
                        footprint: Vec::new(),
                        members_want_base: want,
                        base_only: Vec::new(),
                    });
                }
            }
        }
        i = end;
    }
    Ok(out)
}

/// The working memory of building and reading a stack over `layers`
/// (module doc): `PrecisionMask::plan`'s scratch summed over the visible
/// records' masks, a `row`-wide buffer (ink and footprint, 24 B a pixel)
/// for every visible group and clipped record, every styled record's
/// `styled_scratch` over `frame` (the larger of the canvas and a merge's
/// frame) and a 20 B row for a clip unit's base-only copy.
fn stack_scratch(
    layers: &[PrecisionDocumentLayer],
    frame: (u32, u32),
    row: u64,
) -> Result<u64, String> {
    let overflow = || "CMYK mask preparation calculation overflow".to_string();
    let mut total = 0u64;
    for layer in layers.iter().filter(|layer| layer.visible()) {
        if let Some(mask) = layer.mask() {
            total = total.checked_add(mask.plan()?.0).ok_or_else(overflow)?;
        }
        if let Content::Group(children) | Content::PassThrough(children) = &layer.content {
            let inner = stack_scratch(children, frame, row)?;
            total = total
                .checked_add(row * 24)
                .and_then(|t| t.checked_add(inner))
                .ok_or_else(overflow)?;
        }
        if layer.clipped() {
            total = total.checked_add(row * 24).ok_or_else(overflow)?;
            if layer.style().is_some_and(|s| s.knockout != Knockout::None) {
                total = total.checked_add(row * 20).ok_or_else(overflow)?;
            }
        }
        if needs_styled(layer) {
            total = total
                .checked_add(styled_scratch(layer, frame, row)?)
                .ok_or_else(overflow)?;
        }
    }
    Ok(total)
}

impl PrecisionDocument {
    /// The visible record tree bottom-first (module doc), or the one raster
    /// `only` at full opacity, Normal, whatever its visibility and with no
    /// mask — a layer's own view. `only` naming an adjustment record or a
    /// group is an empty stack (it has no pixels of its own).
    pub(crate) fn cmyk_stack(
        &self,
        only: Option<usize>,
        job: JobContext<'_>,
    ) -> Result<CmykStack<'_>, String> {
        if let Some(index) = only {
            self.require_cmyk_policy("the CMYK stack")?;
            let layer = self
                .layer(index)
                .ok_or_else(|| format!("CMYK layer index {index} is out of range"))?;
            let mut entries = Vec::new();
            if let Content::CmykRaster { image, offset } = &layer.content {
                entries.push(Entry::Raster {
                    id: layer.id(),
                    image,
                    offset: *offset,
                    opacity: 1.0,
                    fill: 1.0,
                    mode: BlendMode::Normal,
                    mask: None,
                });
            }
            return Ok(CmykStack {
                entries,
                background: false,
                background_row: Vec::new(),
                capture: None,
            });
        }
        let (w, h) = self.dimensions();
        self.cmyk_stack_over((0, 0, i64::from(w), i64::from(h)), job)
    }

    /// The whole stack, its rows read over `bounds` (canvas `(x0, y0, x1,
    /// y1)`): the canvas for a projection, a merge's frame for a merge.
    pub(crate) fn cmyk_stack_over(
        &self,
        bounds: (i64, i64, i64, i64),
        job: JobContext<'_>,
    ) -> Result<CmykStack<'_>, String> {
        let policy = self.require_cmyk_policy("the CMYK stack")?;
        let ctx = Build {
            document: self,
            policy,
            depth_bits: policy.depth.bits(),
            light: self.global_light,
            canvas: self.dimensions(),
            bounds,
        };
        let roots = self.root_layers();
        let background =
            crate::precision_style_knockout::document_wants_background16(roots, 0, Cut::NONE)
                && crate::precision_style_knockout::background_record(
                    roots,
                    self.dimensions(),
                    Cut::NONE,
                    // CMYK quantizes always (`opacity_byte`).
                    false,
                );
        Ok(CmykStack {
            entries: level(roots, &ctx, job)?,
            background,
            background_row: Vec::new(),
            capture: None,
        })
    }

    /// What building [`Self::cmyk_stack`] holds beside its rows (module doc).
    pub(crate) fn cmyk_stack_scratch_bytes(&self) -> Result<u64, String> {
        self.cmyk_stack_scratch_bytes_over(self.dimensions().0)
    }

    /// [`Self::cmyk_stack_scratch_bytes`] for rows `width` wide (a merge's
    /// frame): the styled records' frames are charged over the larger of the
    /// canvas and a `width`-wide frame of the canvas height.
    pub(crate) fn cmyk_stack_scratch_bytes_over(&self, width: u32) -> Result<u64, String> {
        let (w, h) = self.dimensions();
        let row = u64::from(width);
        let background = row * 20;
        Ok(stack_scratch(self.root_layers(), (w.max(width), h), row)? + background)
    }
}

impl CmykStack<'_> {
    /// Canvas row `y`, columns `x0..x0 + acc.len()`, composited into `acc`.
    /// `&mut` because every group keeps its row buffer.
    pub(crate) fn row(&mut self, y: i64, x0: i64, acc: &mut [InkPixel]) {
        self.row_strided(y, x0, 1, acc);
    }

    /// Canvas row `y` at columns `x0 + k·step` for `k` in `0..acc.len()`,
    /// composited into `acc`: every record, mask and Dissolve position read
    /// at the full-resolution canvas column, so `acc[k]` is exactly the
    /// full row's pixel at that column (a reduced-resolution region render's
    /// SAMPLING rule). `step` 1 is [`Self::row`].
    pub(crate) fn row_strided(&mut self, y: i64, x0: i64, step: u32, acc: &mut [InkPixel]) {
        let span = Span {
            x0,
            step: i64::from(step.max(1)),
        };
        acc.fill([0.0; 5]);
        if let Some(capture) = self.capture.as_mut() {
            capture.filled = false;
        }
        let CmykStack {
            entries,
            background,
            background_row,
            capture,
        } = self;
        if *background && !entries.is_empty() {
            // D5: record 0 draws with NO capture, then the row is the
            // Background both knockout modes punch to (Shallow at the root
            // is Deep).
            composite_entry(&mut entries[0], y, span, acc, KnockoutSeed::NONE, &mut None);
            background_row.clear();
            background_row.extend_from_slice(acc);
            let env = KnockoutSeed {
                shallow: Some(&background_row[..]),
                deep: Some(&background_row[..]),
            };
            // A merge INTO the Background reads this backdrop too.
            if let Some(capture) = capture.as_mut() {
                capture.offer(&entries[0], env);
            }
            composite_level(&mut entries[1..], y, span, acc, env, capture);
            return;
        }
        composite_level(entries, y, span, acc, KnockoutSeed::NONE, capture);
    }

    /// [`Self::row`] with the root handed `seed` instead of D5's Background
    /// (a merge's members document, module doc).
    pub(crate) fn row_seeded(
        &mut self,
        y: i64,
        x0: i64,
        acc: &mut [InkPixel],
        seed: KnockoutSeed<'_>,
    ) {
        acc.fill([0.0; 5]);
        if let Some(capture) = self.capture.as_mut() {
            capture.filled = false;
        }
        let span = Span { x0, step: 1 };
        composite_level(&mut self.entries, y, span, acc, seed, &mut self.capture);
    }

    /// From the next row on, records the backdrop the level holding record
    /// `record_id` reads (module doc), each row afresh.
    pub(crate) fn capture_at(&mut self, record_id: u64) {
        self.capture = Some(Capture {
            id: record_id,
            filled: false,
            shallow: None,
            deep: None,
        });
    }

    /// The last row's capture; transparent when that row never reached the
    /// record's level (D4's private-accumulator rule).
    pub(crate) fn captured(&self) -> KnockoutSeed<'_> {
        match self.capture.as_ref() {
            Some(capture) if capture.filled => KnockoutSeed {
                shallow: capture.shallow.as_deref(),
                deep: capture.deep.as_deref(),
            },
            _ => KnockoutSeed::NONE,
        }
    }
}

/// The canvas columns a row buffer holds: `x0 + k·step`.
#[derive(Clone, Copy)]
struct Span {
    x0: i64,
    step: i64,
}

impl Span {
    /// The canvas column of buffer index `k`.
    fn at(self, k: usize) -> i64 {
        self.x0 + k as i64 * self.step
    }
}

fn composite_level(
    entries: &mut [Entry<'_>],
    y: i64,
    span: Span,
    acc: &mut [InkPixel],
    env: KnockoutSeed<'_>,
    capture: &mut Option<Capture>,
) {
    for entry in entries {
        if let Some(capture) = capture.as_mut() {
            capture.offer(entry, env);
        }
        composite_entry(entry, y, span, acc, env, capture);
    }
}

/// `f(k, src, coverage)` for every buffer index `k` whose canvas column
/// `span.at(k)` of row `y` `image` at `offset` covers.
fn raster_span(
    image: &CmykImage,
    offset: (i32, i32),
    mask: Option<&PreparedMask<'_>>,
    y: i64,
    span: Span,
    len: usize,
    mut f: impl FnMut(usize, &[u16; 5], f32),
) {
    let ly = y - i64::from(offset.1);
    let (lw, lh) = image.dimensions();
    if ly < 0 || ly >= i64::from(lh) {
        return;
    }
    if span.step != 1 {
        for k in 0..len {
            let cx = span.at(k);
            let lx = cx - i64::from(offset.0);
            if lx < 0 || lx >= i64::from(lw) {
                continue;
            }
            if let Some(src) = image.sample_at(lx as u32, ly as u32) {
                let coverage = mask.map_or(1.0, |m| m.at(cx, y));
                f(k, &src, coverage);
            }
        }
        return;
    }
    let x0 = span.x0;
    // The canvas span this record covers on the row, in its own x.
    let start = (x0 - i64::from(offset.0)).max(0);
    let end = (x0 + len as i64 - i64::from(offset.0)).min(i64::from(lw));
    if start >= end {
        return;
    }
    for (lx, run) in image.row_segments(ly as u32, start as u32, end as u32) {
        let cx0 = i64::from(lx) + i64::from(offset.0);
        let at = (cx0 - x0) as usize;
        for (k, src) in run.iter().enumerate() {
            let coverage = mask.map_or(1.0, |m| m.at(cx0 + k as i64, y));
            f(at + k, src, coverage);
        }
    }
}

fn ink(src: &[u16; 5]) -> [f32; 4] {
    [unit(src[0]), unit(src[1]), unit(src[2]), unit(src[3])]
}

fn coverage_at(mask: &Option<PreparedMask<'_>>, x: i64, y: i64) -> f32 {
    mask.as_ref().map_or(1.0, |m| m.at(x, y))
}

fn composite_entry(
    entry: &mut Entry<'_>,
    y: i64,
    span: Span,
    acc: &mut [InkPixel],
    env: KnockoutSeed<'_>,
    capture: &mut Option<Capture>,
) {
    match entry {
        Entry::Raster {
            image,
            offset,
            opacity,
            fill,
            mode,
            mask,
            ..
        } => {
            // The Fill and the opacity in one product (`opacity_byte::joint`).
            let (joint, mode) = (crate::opacity_byte::joint(*fill, *opacity, false), *mode);
            raster_span(
                image,
                *offset,
                mask.as_ref(),
                y,
                span,
                acc.len(),
                |k, src, coverage| {
                    let sa = unit(src[4]) * joint * coverage;
                    over_mode(&mut acc[k], ink(src), sa, mode, (span.at(k), y));
                },
            );
        }
        Entry::Tone {
            tone,
            depth_bits,
            opacity,
            mode,
            mask,
            ..
        } => {
            for (k, dst) in acc.iter_mut().enumerate() {
                if dst[4] <= 0.0 {
                    continue;
                }
                let weight = *opacity * coverage_at(mask, span.at(k), y);
                if weight <= 0.0 {
                    continue;
                }
                let adjusted = tone.apply([dst[0], dst[1], dst[2], dst[3]], *depth_bits);
                tone_mix_mode(dst, adjusted, weight, *mode, (span.at(k), y));
            }
        }
        Entry::Group {
            children,
            opacity,
            fill,
            mode,
            mask,
            buf,
            ..
        } => {
            buf.resize(acc.len(), [0.0; 5]);
            buf.fill([0.0; 5]);
            // D4: an isolated buffer is a hard stop for both modes.
            composite_level(children, y, span, buf, KnockoutSeed::NONE, capture);
            let joint = crate::opacity_byte::joint(*fill, *opacity, false);
            for (k, (dst, src)) in acc.iter_mut().zip(buf.iter()).enumerate() {
                let sa = src[4] * joint * coverage_at(mask, span.at(k), y);
                over_mode(
                    dst,
                    [src[0], src[1], src[2], src[3]],
                    sa,
                    *mode,
                    (span.at(k), y),
                );
            }
        }
        Entry::Pass {
            children,
            opacity,
            mask,
            buf,
            ..
        } => {
            buf.clear();
            buf.extend_from_slice(acc);
            // D4: Shallow punches to the row as the children began — `acc`
            // itself, untouched until the gate; Deep rides through.
            let children_env = KnockoutSeed {
                shallow: Some(&*acc),
                deep: env.deep,
            };
            composite_level(children, y, span, buf, children_env, capture);
            for (k, (dst, evaluated)) in acc.iter_mut().zip(buf.iter()).enumerate() {
                gate(dst, *evaluated, *opacity * coverage_at(mask, span.at(k), y));
            }
        }
        Entry::Clip {
            base,
            members,
            buf,
            footprint,
            members_want_base,
            base_only,
        } => {
            buf.resize(acc.len(), [0.0; 5]);
            buf.fill([0.0; 5]);
            footprint.resize(acc.len(), 0.0);
            footprint.fill(0.0);
            let (opacity, mode) = draw_base(base, y, span, buf, footprint, capture);
            // `clip_unit`'s Open (#2), then D4: a member punches to the
            // buffer after the base alone drew (opened).
            clip_pass(buf, footprint, ClipStep::Open);
            if *members_want_base {
                base_only.clear();
                base_only.extend_from_slice(buf);
            }
            let member_env = if *members_want_base {
                KnockoutSeed {
                    shallow: Some(&base_only[..]),
                    deep: Some(&base_only[..]),
                }
            } else {
                KnockoutSeed::NONE
            };
            for member in members.iter_mut() {
                composite_entry(member, y, span, buf, member_env, capture);
                clip_pass(buf, footprint, ClipStep::Settle);
            }
            clip_pass(buf, footprint, ClipStep::Close);
            for (k, (dst, src)) in acc.iter_mut().zip(buf.iter()).enumerate() {
                over_mode(
                    dst,
                    [src[0], src[1], src[2], src[3]],
                    src[4] * opacity,
                    mode,
                    (span.at(k), y),
                );
            }
        }
        Entry::Styled {
            styled,
            source,
            mask,
            members,
            members_want_base,
            base_only,
            scratch,
            ..
        } => {
            let b0 = match styled.knockout {
                Knockout::None => None,
                Knockout::Shallow => env.shallow,
                Knockout::Deep => env.deep,
            };
            let origin = source.origin();
            let source = &*source;
            let mask = mask.as_ref();
            let pixel = |x: i64, y: i64| source.pixel(x, y, mask);
            if members.is_empty() {
                composite_styled_row(
                    acc, b0, y, span.x0, span.step, origin, styled, &pixel, None, scratch,
                );
                return;
            }
            let want = *members_want_base;
            let count = members.len();
            let mut render = |i: usize, buf: &mut [InkPixel]| {
                // D4: the members punch to the unit as it stands after the
                // base's pixel pass, before any member.
                if want && i == 0 {
                    base_only.clear();
                    base_only.extend_from_slice(buf);
                }
                let member_env = if want {
                    KnockoutSeed {
                        shallow: Some(&base_only[..]),
                        deep: Some(&base_only[..]),
                    }
                } else {
                    KnockoutSeed::NONE
                };
                composite_entry(&mut members[i], y, span, buf, member_env, capture);
            };
            composite_styled_row(
                acc,
                b0,
                y,
                span.x0,
                span.step,
                origin,
                styled,
                &pixel,
                Some((count, &mut render)),
                scratch,
            );
        }
    }
}

/// A clip unit's base into the transparent `buf` at Normal and full
/// opacity with its fill, its footprint (alpha × mask, no fill) into
/// `footprint` (module doc); answers the opacity and mode the finished
/// buffer composites with. An adjustment base has no pixels of its own, so
/// its footprint is empty and the unit shows nothing.
fn draw_base(
    base: &mut Entry<'_>,
    y: i64,
    span: Span,
    buf: &mut [InkPixel],
    footprint: &mut [f32],
    capture: &mut Option<Capture>,
) -> (f32, BlendMode) {
    match base {
        Entry::Raster {
            image,
            offset,
            opacity,
            fill,
            mode,
            mask,
            ..
        } => {
            let fill = *fill;
            raster_span(
                image,
                *offset,
                mask.as_ref(),
                y,
                span,
                buf.len(),
                |k, src, coverage| {
                    let shape = unit(src[4]) * coverage;
                    footprint[k] = shape;
                    over(&mut buf[k], ink(src), shape * fill);
                },
            );
            (*opacity, *mode)
        }
        Entry::Group {
            children,
            opacity,
            fill,
            mode,
            mask,
            buf: inner,
            ..
        } => {
            inner.resize(buf.len(), [0.0; 5]);
            inner.fill([0.0; 5]);
            composite_level(children, y, span, inner, KnockoutSeed::NONE, capture);
            for (k, src) in inner.iter().enumerate() {
                let shape = src[4] * coverage_at(mask, span.at(k), y);
                footprint[k] = shape;
                over(&mut buf[k], [src[0], src[1], src[2], src[3]], shape * *fill);
            }
            (*opacity, *mode)
        }
        Entry::Tone { opacity, mode, .. } => (*opacity, *mode),
        // `level` isolates a base Pass Through group, routes a styled base to
        // `Entry::Styled`, and a unit is never a base; none is reachable.
        Entry::Pass { opacity, .. } => (*opacity, BlendMode::Normal),
        Entry::Styled { styled, .. } => (styled.opacity, styled.mode),
        Entry::Clip { .. } => (0.0, BlendMode::Normal),
    }
}
