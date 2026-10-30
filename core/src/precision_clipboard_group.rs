//! Copy of a GROUP at 16 bits and 32-bit float — the typed twin of the
//! byte Copy of a group, which copies `DrDocument::group_projection`
//! (`doc_group_ops.rs`, the master rule).
//!
//! # The rule
//!
//! A Copy with a group as its layer copies the group's RAW projection under
//! the selection, at the document's own depth: the children composited
//! among themselves with their own blend, opacity, mask, style and
//! clipping, but WITHOUT the group's own opacity, blend mode, mask, style
//! or Blend If, with its visibility forced on. A Pass Through group is
//! re-expressed as an isolated group: over transparency there is no
//! backdrop for it to pass through to. The projection is rendered by the
//! ordinary typed compositor over a one-root document
//! ([`PrecisionDocument::raw_group_document`]) and the window is read out of
//! it by Copy Merged's own render-then-window read
//! (`precision_clipboard::read_rendered`), so no pixel, window or coverage
//! arithmetic is written here. Cut of a group refuses at every depth, as
//! the byte Cut does.
//!
//! # Admission
//!
//! The read admits Copy Merged's charges with the raw document as the
//! source: the canvas-sized render plus `2 · payload` before the renderer
//! runs, then the renderer's own plan with the payload counted as other
//! retained bytes. [`PrecisionDocument::group_copy_plan`] is that largest
//! single admission (the exact threshold), and
//! [`PrecisionDocument::group_copy_allocation`] the NEW memory the Copy
//! allocates — the same figure without the children's pixels, which are
//! already resident — the quantity the measurement gate compares with the
//! measured footprint. Both add [`GROUP_COPY_MARGIN_PER_PIXEL`] × canvas
//! to the staged charge, the margin the measurement below fitted, and both
//! take the max with the read's host stage (`precision_clipboard_host`),
//! which every read admits too.
//!
//! # Measurement record (batch 170, 2026-10-05)
//!
//! Fixture: a Release app on port 4997 with `DR_WORKING_LIMIT_BYTES =
//! 8589934592`, `MallocLargeCache=0`, `DR_BACKGROUND=1`, private `DR_*_DIR`s
//! and a private pasteboard. Over MCP: a 6000×4000 16-bit PNG opened as the
//! Background, duplicated three times; two copies given `add_noise`; the
//! top child Multiply at 70 %, the middle one clipped to the bottom one;
//! the three grouped and the group set to Screen at 50 %; `select_all`;
//! saved as `.rz`. One measured MCP `copy` with `layer` = the group per
//! fresh instance (the saved file reopened, `select_all` again), E =
//! `ri_interval_max_phys_footprint` − the footprint before the call.
//!
//! - Without a margin: E = 1,451,558,088 / 1,433,666,760 / 1,458,472,112
//!   bytes; the `list_documents` twin E = 0 / 0 / 0. A =
//!   `group_copy_allocation(4, 6000, 4000, true)` = 1,729,048,576 and P =
//!   `group_copy_plan` = 2,305,071,447 (the render plan's scratch,
//!   1,249,048,576, plus `2 · payload`, 480,000,000; P adds the children's
//!   resident pixels). A / max E = 1.186, below the 1.25 gate.
//! - Fitted: `max E · 1.25 − (canvas · 8 + 2 · payload)` over 24,000,000
//!   pixels is 47.96 B/px, so the margin is 48 B/px (47 gives 1.80 GB).
//! - With the margin (re-measured on a rebuilt app): E = 1,459,127,472 /
//!   1,458,472,112 / 1,433,584,816; twin E = 0 / 0 / 0. A = 1,824,000,000
//!   (the staged term now dominates), P = 2,305,071,447 (unchanged; the
//!   render admission still dominates it). A / max E = 1.2501 ≥ 1.25:
//!   ⌈1.25 × 1,459,127,472⌉ = 1,823,909,340 ≤ A.
//! - Diagnostic: one `copy_merged` of the same document measured E_merged =
//!   1,452,393,672 (1,448,723,632 on the rebuilt app), within 0.7 % of the
//!   group Copy's E; the core difference (the subtree's render plan against
//!   the whole stack's) is 0 bytes of scratch here, since both render plans
//!   charge 1,249,048,576. Most of E beyond the core's terms is the host's
//!   public rendering and its TIFF/PNG encode, which this module does not
//!   charge.
//!
//! `group_copy_plan` is a group Copy's exact threshold and `group_copy_allocation` its new memory,
//! both with the fitted `GROUP_COPY_MARGIN_PER_PIXEL` (48) × canvas in the staged charge (the
//! measurement record is the GROUP module's doc). A group Copy needs no RZCB change: the
//! clipboard carries pixels plus a text, shape or adjustment record and never a group's open
//! state or the global light — a Copy of a group copies its raw projection as pixels at every
//! depth — and a Copy Merged bakes the source document's global light into its pixels at both
//! depths (the typed one renders the projection, which reads the light).

use crate::blend::BlendMode;
use crate::precision_clipboard_codec::payload_bytes;
use crate::precision_clipboard_host::host_stage_bytes;
use crate::precision_document::{Content, PrecisionDocument};
use crate::precision_image::pixel_count;

/// The fitted margin a group's Copy charges per CANVAS pixel on top of the
/// staged render plus `2 · payload`, before the renderer runs. Why: the
/// measured peak of a 24 MP 16-bit group Copy (the module doc's record) is
/// above what the core's own terms charge, and the house rule asks the
/// allocation figure to be at least 1.25× that peak; 48 is the smallest
/// integer that passes (47 does not). It is charged × canvas through
/// `read_rendered`'s `margin`, and in [`PrecisionDocument::group_copy_plan`]
/// and [`PrecisionDocument::group_copy_allocation`], so the plan stays the
/// exact admission threshold.
pub(crate) const GROUP_COPY_MARGIN_PER_PIXEL: u64 = 48;

impl PrecisionDocument {
    /// Record `layer` (a Group or PassThrough record, postorder index) as the
    /// only root of a document built by `self.rebuilt(w, h, vec![root],
    /// Vec::new())`. The root is a clone with opacity 1.0, visible true, mask
    /// None, warp None, blend_mode `BlendMode::Normal`, style None, blend_if
    /// None, clipped false, and PassThrough(children) re-expressed as
    /// Group(children). Children are untouched. Err for an out-of-range index
    /// or a non-group record. The typed twin of `DrDocument::group_projection`
    /// (`doc_group_ops.rs`), the master rule. Its other caller is
    /// `analysis_source`'s Layer arm, so `layer_canvas_image`,
    /// layer-scoped analysis and a group selection source read the same
    /// projection Copy does.
    pub(crate) fn raw_group_document(&self, layer: usize) -> Result<PrecisionDocument, String> {
        let record = self
            .layer(layer)
            .ok_or("clipboard layer index is out of range")?;
        let children = match &record.content {
            Content::Group(children) | Content::PassThrough(children) => children.clone(),
            _ => return Err(format!("layer {layer} is not a group")),
        };
        let mut root = record.clone();
        root.content = Content::Group(children);
        root.opacity = 1.0;
        root.visible = true;
        root.mask = None;
        root.warp = None;
        root.blend_mode = BlendMode::Normal;
        root.style = None;
        root.blend_if = None;
        root.clipped = false;
        let (w, h) = self.dimensions();
        self.rebuilt(w, h, vec![root], Vec::new())
    }

    /// The largest single admission a Copy of group `layer` over a
    /// `width`×`height` window makes (coverage present iff `covered`), i.e.
    /// the exact threshold:
    /// `max(canvas·render_bpp + 2·payload + margin, iso.unique_retained_bytes
    /// + iso.scratch_bytes + 2·payload, host)`, where `iso` is the raw group
    /// document's `render_plan`, `payload` is
    /// `precision_clipboard_codec::payload_bytes` in the render storage,
    /// `margin` is [`GROUP_COPY_MARGIN_PER_PIXEL`] · canvas and `host` is the
    /// read's host stage (`precision_clipboard_host::host_stage_bytes`).
    pub fn group_copy_plan(
        &self,
        layer: usize,
        width: u32,
        height: u32,
        covered: bool,
    ) -> Result<u64, String> {
        let (staged, held, iso) = self.group_copy_terms(layer, width, height, covered)?;
        let render = iso
            .unique_retained_bytes
            .checked_add(iso.scratch_bytes)
            .and_then(|n| n.checked_add(held))
            .ok_or("clipboard size overflow")?;
        let host = host_stage_bytes(width, height, self.render_storage(), covered)?;
        Ok(staged.max(render).max(host))
    }

    /// The NEW memory the same Copy allocates. It excludes the children's
    /// pixels, which are already resident:
    /// `max(canvas·render_bpp + 2·payload + margin, iso.scratch_bytes +
    /// 2·payload, host)`.
    pub fn group_copy_allocation(
        &self,
        layer: usize,
        width: u32,
        height: u32,
        covered: bool,
    ) -> Result<u64, String> {
        let (staged, held, iso) = self.group_copy_terms(layer, width, height, covered)?;
        let render = iso
            .scratch_bytes
            .checked_add(held)
            .ok_or("clipboard size overflow")?;
        let host = host_stage_bytes(width, height, self.render_storage(), covered)?;
        Ok(staged.max(render).max(host))
    }

    /// The terms both figures share: the staged charge
    /// (`canvas·render_bpp + 2·payload + margin`), the held payload
    /// (`2·payload`) and the raw group document's render plan.
    fn group_copy_terms(
        &self,
        layer: usize,
        width: u32,
        height: u32,
        covered: bool,
    ) -> Result<(u64, u64, crate::precision_render::PrecisionRenderPlan), String> {
        let storage = self.render_storage();
        let (w, h) = self.dimensions();
        let canvas = pixel_count(w, h)? as u64;
        let held = 2 * payload_bytes(width, height, storage, covered);
        let staged = (canvas * storage.bytes_per_pixel())
            .checked_add(held)
            .and_then(|n| n.checked_add(GROUP_COPY_MARGIN_PER_PIXEL.checked_mul(canvas)?))
            .ok_or("clipboard size overflow")?;
        let iso = self.raw_group_document(layer)?.render_plan()?;
        Ok((staged, held, iso))
    }
}
