//! Healing on the layered document: the Poisson write-back that turns a
//! cloned patch into a seamless one, and the healing brush that drives it.
//!
//! The stroke arrives as the SAME canvas-frame premultiplied RGBA8 overlay
//! `dr_doc_painting_layer` takes — the Clone Stamp's overlay, literally: its
//! ALPHA is the footprint's coverage and its RGB the already-aligned source
//! pixels, premultiplied by that coverage. The healing brush IS the clone
//! stamp with a different op at commit, which is why no offset parameter
//! crosses the FFI: the alignment happened when the overlay was drawn.
//!
//! # What the solve does
//!
//! Covered pixels (alpha ≥ 128) are handed to `poisson::solve_membrane`,
//! which returns the harmonic correction `ũ` matching `destination − source`
//! on the covered set's contour. The healed pixel is `source + ũ`: the
//! source's texture, the destination's illumination. Each of R, G and B is
//! solved independently on STRAIGHT colour, in the document's own numbers —
//! healing samples the document's own pixels, so no colour conversion
//! happens anywhere here. Layer alpha is never touched, and a fully
//! transparent destination pixel is excluded from the solve and never
//! written (its RGB is latent garbage, not colour — `doc_retouch`'s rule).
//!
//! # The write-back weight is a HARD cut, and why
//!
//! `out = dst + w·(f − dst)` with **`w = covered ? strength : 0`** — a hard
//! cut at the α = 128 contour, NOT the overlay's soft alpha. Outside the
//! covered set the solver leaves `ũ = 0`, so weighting by a soft alpha there
//! would blend in `f = g`: the UNCORRECTED clone, carrying exactly the
//! illumination mismatch the op exists to remove, as a bright or dark fringe
//! around every stroke. No feather is needed to hide the join, because
//! `f == dst` exactly on the contour. Content-Aware Fill is the one caller
//! that passes a soft `write` plane, and it can because it also takes its Ω
//! out to the far edge of the selection's ramp: every pixel it fades is one
//! the inpaint actually re-estimated, so the weight rises from 0 INSIDE Ω
//! rather than stepping at its contour. Spot healing is NOT: its coverage is
//! a brush dab exactly like this one's, so it takes the hard cut too, which
//! is all `doc_inpaint::Caller` decides.
//!
//! Two consequences worth stating plainly:
//!
//! - **Hardness shrinks the footprint; it does not fade the heal.** A soft
//!   brush's rim falls below 128 and is simply not healed. The heal itself
//!   is always at full strength inside the contour.
//! - **Flow must be 1 for a healing brush.** Flow multiplies every dab's
//!   alpha, so at flow ≤ 0.5 a click deposits α < 128 everywhere, the
//!   covered set is empty and the op refuses. Strength belongs in Opacity,
//!   which the callers pass as `strength` — the dodge/burn precedent.
//!
//! # Quantization at the rim
//!
//! At a pixel ON the contour the un-premultiply error cancels exactly:
//! `f = g + (f* − g) = f*`. It does NOT cancel in the interior — `b` is read
//! at the contour from the un-premultiplied overlay and, by the maximum
//! principle, an error there propagates inward with weight up to 1. At the
//! α = 128 contour the un-premultiply error is up to 127/128 ≈ 1 code value,
//! so a CG-rasterized (anti-aliased) footprint carries up to ±1 code of DC
//! error across the healed patch. A hand-built, integer-aligned,
//! full-coverage footprint carries none. Relatedly, the overlay conflates
//! the source's own alpha with coverage: cloning from a semi-transparent
//! area heals at reduced strength, exactly as the Clone Stamp paints it at
//! reduced opacity. Both are limits, not bugs.
//!
//! # "Replace" mode is not shipped
//!
//! A Replace mode skips the blend at the footprint's edge, which
//! in this build is precisely the Clone Stamp — already its own tool with
//! its own overlay path. Shipping it would be a second name for an existing
//! tool, so `heal_layer` has no mode parameter.
//!
//! # Measured cost of a stroke commit
//!
//! A whole `heal_layer` call on a 2000×1500 document (`heal_tests`, this
//! machine, `--release`) — the canvas-sized coverage scan and the component
//! walk included, which is most of the bill for a small dab:
//!
//! | footprint | covered px | whole op, 3 channels |
//! |---|---|---|
//! | a 40 px brush dab | 1.3 k | 4.4 ms |
//! | a 120 px dab | 11 k | 8.4 ms |
//! | a patch-tool region, 340 px across | 91 k | 54 ms |
//!
//! So the solve runs once at mouse-up, on the whole stroke's footprint, and
//! costs a frame or two — which is why the healing brush previews the raw
//! clone during the drag and heals on release, rather than solving per dab
//! (`k` solves instead of one, each pinning its boundary to a destination
//! the previous dab already moved, which shows as ridges along the stroke).
//!
//! # What is capped, and why there are two caps
//!
//! [`MAX_SOLVE_WINDOW_PIXELS`] bounds ONE component's window and is about
//! MEMORY. It says nothing about a call with many components, and the cost
//! of those is real and was unbounded: every component pays a full pass over
//! its own window — the coverage, usable, `b` and `u` planes, plus
//! `poisson`'s classification — even when it covers three pixels, so the
//! bill is `parts × window area`. Measured on a 5000 × 4000 document whose
//! coverage is N disjoint three-pixel diagonal runs, each with a box
//! spanning nearly the whole canvas (the shape a scratch, a wire or a hair
//! makes): 10 parts 5.2 s, 40 parts 20.2 s, linear in the count and nothing
//! refusing. [`MAX_HEAL_PLAN_PIXELS`] bounds that sum, before any component
//! is healed — the same quantity `doc_inpaint`'s plan cap bounds for the
//! sibling ops, which refused the identical selection in milliseconds while
//! this one accepted it and blocked.
//!
//! # Cancellation between the solves
//!
//! Building a component's window planes polls every `EVERY_INTERVAL`
//! component pixels and every `EVERY_INTERVAL` window pixels, so a cancel
//! there returns within 4096 pixels of either pass; the read of the union
//! rect (`layer_rect::rect_image_with_job`) and its final write
//! (`LayerRaster::with_rect_with_job`) poll once per visited tile, so a
//! cancel there returns within one tile's copy (or compare and copy).
//!
//! # Admission
//!
//! The reserve before the component walk charges one `COMPONENT_BYTES`
//! record per start `heal_components::component_count_bound` counts (a
//! proven upper bound on the components; the proof is that module's doc),
//! not one per covered pixel: a dense 16 MP Patch used to reserve 2,276,388,608 bytes
//! before its walk, over the 2 GiB reference limit it was measured against.
//! The post-plan reserve takes the walk's own
//! count. With the per-covered-pixel records no longer covering any shortfall
//! in the two per-pixel constants, both were measured: one heal per process
//! (`dr_doc_heal_layer`, NULL control) on `heal_job`'s gradient under its
//! checker overlay, `/usr/bin/time -l` "peak memory footprint", the footprint
//! read in-process (`proc_pid_rusage` `ri_phys_footprint`) just before the
//! call, `E = peak − pre-edit`, three runs per case. E also counts the result
//! document's rewritten tiles, held when the peak is read.
//!
//! - **M1**, the walk's worst canvas term: 8192×4097, all covered
//!   (n = 33,562,624, one component; the pixel list doubles just past 2^25),
//!   refused over-window after the walk. Figure `10n + 128 + 8 MiB` =
//!   344,014,976. Peaks 611,894,064 / 611,337,008 / 609,780,528; pre-edit
//!   404,882,032 / 404,832,880 / 404,849,264; E 207,012,032 / 206,504,128 /
//!   204,931,264. Baseline peaks (no call) 404,898,440 / 404,931,208 /
//!   404,931,208.
//! - **M2**, the 16 MP Patch: 5000×4400, covered (500, 200)–(4500, 4200),
//!   window = union = 4002² = 16,016,004; the heal completes. Figure at
//!   38 B/px 901,060,904. Peaks 1,074,414,720 / 1,074,365,568 / 1,074,365,568;
//!   pre-edit 257,982,968 / 257,950,200 / 258,015,736; E 816,431,752 /
//!   816,415,368 / 816,349,832. Baseline peaks 258,032,144 / 258,048,528 /
//!   258,048,528.
//! - **M3**, the same Patch where canvas = union = window: 4002×4002,
//!   covered (1, 1)–(4001, 4001); completes. Figure at 38 B/px
//!   `52n + 128 + 8 MiB` = 841,220,944. Peaks 979,616,824 / 979,584,056 /
//!   983,073,848; pre-edit 194,314,672 / 194,249,136 / 194,281,904; E
//!   785,302,152 / 785,334,920 / 788,791,944. Baseline peaks 194,380,232 /
//!   194,380,232 / 194,331,080.
//!
//! The rule: a constant is only ever raised, each to the smallest integer
//! keeping every case's figure at or above 1.25·E. At 10 and 38 the largest
//! E was 60 % of M1's figure, 91 % of M2's and 94 % of M3's, so `HEAL_WINDOW_BYTES_PER_PIXEL`
//! is raised to 48: M3 needs `(14 + v)·16,016,004 + 8,388,736 ≥ 1.25 ×
//! 788,791,944 = 985,989,930`, so `v ≥ 47.04`, and M2 needs `v ≥ 45.46`.
//! Under 10 and 48 M3's figure is `62n + 128 + 8 MiB` = 1,001,380,984 (largest
//! E 0.788 of it) and M2's 1,061,220,944 (0.769). M1's figure does not
//! involve the window term and 1.25 × 207,012,032 = 258,765,040 is under
//! 344,014,976 (0.602), so `HEAL_CANVAS_BYTES_PER_PIXEL` stays 10. M3 is the
//! tight case: with canvas ≈ window the canvas term has the least slack to
//! lend the window term.
//!
//! `heal_layer` (the Healing Brush and the Patch tool, driven by the SAME premultiplied overlay
//! `dr_doc_painting_layer` takes; `pub(crate)`, it takes a `JobContext`, reserves
//! `heal_scratch_bound` before the plan and again sized to the measured union and largest window
//! before the layer read, reports stage 2 at `HEAL_PLAN_PROGRESS` and stage 4 per component over a
//! share weighted by padded window area, and `dr_doc_heal_layer` / `dr_doc_heal_to_new_layer` take
//! a nullable `DrJobControl`), `heal_window_into_pixels_with_context` — the one write-back the
//! inpainters share, writing into a single layer buffer so a many-part edit pays one clone — and
//! `heal_window_into_layer`, the whole-layer form kept for `core/tests/heal_tests.rs:429`'s
//! window-contract test (since 13B-2c-2a every production caller — heal, spot heal and
//! content-aware fill — runs `heal_window_into_pixels_with_context` over a
//! `layer_rect::rect_image`; it takes a bare `&DrDocument`, reads `layers.get` and is NOT
//! lock-gated — the locks are enforced at `heal_layer` / `spot_heal_layer_job` /
//! `content_aware_fill_source_job`). Owns the 4-connected `components_with_context` walk the whole
//! healing family splits its coverage with, and solves each component in its own window; the write
//! weight is a hard cut at the coverage contour (Content-Aware Fill's soft selection bytes are the
//! one exception), so no pixel ever receives an unsolved source. Two caps, both measured before
//! anything is healed: `MAX_SOLVE_WINDOW_PIXELS` on one component's window (memory) and
//! `MAX_HEAL_PLAN_PIXELS` on every component's window summed (the CALL's time — a stroke landing in
//! many places pays a full pass per place). `window_buffers` polls every
//! `EVERY_INTERVAL` component and window pixels and the final union-rect write goes through
//! `LayerRaster::with_rect_with_job` (one check per visited tile; the byte inpaint's final write
//! too); `heal_job`'s bench probe is in `make cancel-bench`. The union read polls per
//! visited tile too (`layer_rect::rect_image_with_job`). `heal_scratch_bound` charges
//! one `COMPONENT_BYTES` record per component: before the walk by
//! `heal_components::component_count_bound` (a proven upper bound, one pass over `covered`), after
//! it by the walk's own count (heal has no hole cap); this module doc's `# Admission` holds
//! the M1–M3 footprint measurements that keep 10 B/px and raised the window term to 48 B/px (1.25×
//! headroom). `heal_job`'s cancel-bench also probes the heal and spot-heal union reads
//! and each step of the new-layer deposit.

use crate::job_control::JobContext;
use image::RgbaImage;

use crate::doc::DrDocument;
use crate::doc_lock::EditKind;
use crate::layer_raster::ALLOCATION;
use crate::layer_rect::{canvas_rect_on_layer, rect_image_with_job, rect_offset};
use crate::poisson::COVERED_THRESHOLD;

#[path = "heal_components.rs"]
mod heal_components;
pub(crate) use heal_components::{
    component_count_bound, component_count_bound_where, components_with_context, Component,
};

/// Memory bound on ONE component's solve window. The window carries dense
/// f32 `u` and `b` planes, the classification and coverage byte planes, the
/// source RGB and the multigrid hierarchy's own 4/3 of all that — about 38
/// bytes a pixel measured across the buffers this file and `poisson`
/// allocate, so 32 megapixels is a ~1.2 GB peak and, at `poisson`'s measured
/// 137–166 ns an unknown, ~13–16 s for three channels. It is a bound on
/// MEMORY, not on the unknown count: the schedule in `poisson` costs a
/// constant per unknown, so a big region is slow in proportion to its area
/// and nothing worse.
///
/// The window is a bounding BOX, so the cap is deliberately generous: a
/// three-pixel scratch running corner to corner across a 24-megapixel photo
/// has a box of 24 megapixels and a few thousand covered pixels, and
/// refusing that — the single most common content-aware fill — would be
/// worse than the transient.
///
/// This is the one home for the number; `doc_inpaint` reuses it rather than
/// declaring a second copy.
pub const MAX_SOLVE_WINDOW_PIXELS: u64 = 32_000_000;

/// The largest total of working WINDOWS one heal call will build, summed over
/// its components. It bounds the quantity [`MAX_SOLVE_WINDOW_PIXELS`]
/// deliberately does not — the whole CALL — and it is checked before a single
/// component is healed, so a refusal costs a component walk and not a heal.
///
/// The number is the same 80 megapixels as
/// `doc_inpaint::MAX_INPAINT_PLAN_PIXELS`, and for the same measured reason:
/// a megapixel of window costs about 26 ms of plane building here (5.2 s for
/// ten canvas-spanning scratches on a 5000 × 4000 document, 20.2 s for
/// forty), so 80 megapixels is ~2 s of window work, which leaves the rest of
/// the 15 s a blocking commit is allowed for the solve itself. It is a
/// separate constant rather than a shared one because the two ops bound
/// different work and either may move on its own measurement.
pub const MAX_HEAL_PLAN_PIXELS: u64 = 80_000_000;

/// Scratch per CANVAS pixel a heal call holds before any window exists: the
/// `covered` and component-walk `seen` planes (a byte each) plus the
/// component pixel lists and BFS queue (a `u32` each, at most one entry per
/// covered pixel), rounded up to 10 — `doc_inpaint::inpaint_scratch_bound`'s
/// canvas term for the same planes. Measured (the module doc's
/// `# Admission`, M1): a whole-canvas walk's largest E was
/// 207,012,032 bytes against its 344,014,976 figure.
const HEAL_CANVAS_BYTES_PER_PIXEL: u64 = 10;

/// Scratch per pixel of ONE component's solve window: the ~38 bytes
/// [`MAX_SOLVE_WINDOW_PIXELS`]'s doc measures across the window planes and
/// `poisson`'s multigrid hierarchy, raised to 48 for 1.25× headroom over the
/// measured footprint. Components solve one after another, so only the
/// largest window is live at once. Measured (M2, M3): a 16 MP
/// Patch's largest E was 816,431,752 bytes against 1,061,220,944, and
/// 788,791,944 against 1,001,380,984 where canvas = window.
const HEAL_WINDOW_BYTES_PER_PIXEL: u64 = 48;

/// Fixed headroom for bookkeeping the per-pixel terms do not model, the same
/// 8 MiB `doc_inpaint::inpaint_scratch_bound` adds.
const HEAL_SCRATCH_SLACK: u64 = 8 * 1024 * 1024;

/// The share of a controlled heal's progress the plan (coverage scan and
/// component walk) is reported as; the solves share the rest.
const HEAL_PLAN_PROGRESS: f64 = 0.05;

/// The working memory one heal call reserves against its job token:
/// `canvas·10 + components·COMPONENT_BYTES + union·4 + window·48 + slack` —
/// the canvas planes, one component record per component
/// (`precision_inpaint::COMPONENT_BYTES`; heal has no hole cap), the one
/// RGBA8 read of the windows' union on the layer, and the largest single
/// component window. Reserved three times: with every measured term 0 before
/// the coverage scan, with `heal_components::component_count_bound` — a
/// proven upper bound on the component count — before the component
/// walk, and with the walk's own count and every other term once the plan is
/// known, before the layer read.
pub(crate) fn heal_scratch_bound(canvas: u64, components: u64, union: u64, window: u64) -> u64 {
    canvas
        .saturating_mul(HEAL_CANVAS_BYTES_PER_PIXEL)
        .saturating_add(components.saturating_mul(crate::precision_inpaint::COMPONENT_BYTES))
        .saturating_add(union.saturating_mul(4))
        .saturating_add(
            window
                .min(MAX_SOLVE_WINDOW_PIXELS)
                .saturating_mul(HEAL_WINDOW_BYTES_PER_PIXEL),
        )
        .saturating_add(HEAL_SCRATCH_SLACK)
}

impl DrDocument {
    /// Poisson-blends the source patch `overlay` carries into layer `idx`:
    /// the source's texture with the layer's own illumination.
    ///
    /// `overlay` is the canvas-frame PREMULTIPLIED RGBA8 buffer
    /// `dr_doc_painting_layer` takes (`w`/`h` must equal the canvas size).
    /// Its alpha is the footprint's coverage; its RGB, un-premultiplied, is
    /// the already-aligned source. `strength` is clamped to [0, 1] and
    /// scales the write-back. The overlay is mapped through the layer's
    /// offset exactly as `painting_layer`: overlay outside the layer's
    /// extent is ignored and the layer never grows.
    ///
    /// The covered set is split into 4-connected COMPONENTS, each solved in
    /// its own bounding box padded by one pixel — so a scatter of dabs costs
    /// the sum of their own boxes rather than one box around all of them,
    /// and each blemish's boundary condition comes from its own
    /// neighbourhood.
    ///
    /// `Err` (a message for the user) when one component's box exceeds
    /// [`MAX_SOLVE_WINDOW_PIXELS`], or when every component's box together
    /// exceeds [`MAX_HEAL_PLAN_PIXELS`] — both are measured before anything
    /// is healed. `Ok(None)` — a plain refusal — for an
    /// out-of-range `idx`, `w`/`h` not the canvas size, an `overlay` shorter
    /// than `w*h*4`, a non-finite `strength`, an empty covered set, a layer
    /// extent that misses the canvas, or no pixel actually changing (a
    /// covered set with no interior is the common case: every covered pixel
    /// is then on the join contour, where the heal IS the destination).
    ///
    /// `control` is the cancellation context (`JobContext::NONE` for none):
    /// the call reserves [`heal_scratch_bound`] against it before the plan,
    /// with the component-count bound before the component walk and again,
    /// sized to the real components and windows, before the layer read;
    /// reports stage
    /// 2 at [`HEAL_PLAN_PROGRESS`] once the plan is measured, then stage 4 for
    /// each component over a range weighted by its padded window area. A
    /// cancel returns `Err("operation cancelled")`, never a partial result.
    pub(crate) fn heal_layer(
        &self,
        idx: usize,
        overlay: &[u8],
        w: u32,
        h: u32,
        strength: f32,
        control: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        self.under_locks_fallible(idx, EditKind::Pixels, |doc| {
            doc.heal_layer_unlocked(idx, overlay, w, h, strength, control)
        })
    }

    /// The body of [`Self::heal_layer`], outside the lock gate. Split out
    /// only so the gate is one line; that method is its only caller. It takes
    /// `under_locks_fallible` rather than `under_locks` because it reports
    /// through `Result`, and a lock refusal comes back as `Ok(None)` — a
    /// domain refusal, not an error string.
    fn heal_layer_unlocked(
        &self,
        idx: usize,
        overlay: &[u8],
        w: u32,
        h: u32,
        strength: f32,
        control: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        control.check()?;
        let Some(layer) = self.raster_layer(idx) else {
            return Ok(None);
        };
        if w != self.width || h != self.height || !strength.is_finite() {
            return Ok(None);
        }
        let canvas = match (w as usize).checked_mul(h as usize) {
            Some(n) if n > 0 && overlay.len() >= n * 4 => n,
            _ => return Ok(None),
        };
        // The canvas-frame overlay must be able to reach a layer pixel at
        // all — the same intersection `DrDocument::painting_layer`
        // (`doc.rs`, the master copy) makes before touching anything.
        let (lw, lh) = layer.pixels.dimensions();
        let (off_x, off_y) = (i64::from(layer.offset.0), i64::from(layer.offset.1));
        let reach_x = (-off_x).max(0) < (i64::from(self.width) - off_x).min(i64::from(lw));
        let reach_y = (-off_y).max(0) < (i64::from(self.height) - off_y).min(i64::from(lh));
        if !reach_x || !reach_y {
            return Ok(None);
        }
        let n = canvas as u64;
        control.reserve(heal_scratch_bound(n, 0, 0, 0))?;

        let covered: Vec<bool> = (0..canvas)
            .map(|i| {
                control.every(i)?;
                Ok(overlay[i * 4 + 3] >= COVERED_THRESHOLD)
            })
            .collect::<Result<_, String>>()?;
        let starts = component_count_bound(&covered, w as usize, h as usize, control)?;
        if starts == 0 {
            return Ok(None);
        }
        control.reserve(heal_scratch_bound(n, starts, 0, 0))?;
        let parts = components_with_context(&covered, w as usize, h as usize, control)?;
        // The WHOLE call's working area, measured before any of it is healed:
        // a refusal must cost a component walk and not a heal, and the
        // per-component memory bound below says nothing about how many
        // components there are. See the module doc's cap section.
        if let Some(message) = over_plan_limit(&parts, w, h) {
            return Err(message);
        }
        // Check every component's memory bound BEFORE building any window
        // buffers, since they are most of what the bound is about — and
        // before the rect below, so an over-limit part whose window misses
        // the layer still refuses. The same walk takes the union of the
        // windows, which bounds the one layer read.
        let mut union: Option<(i64, i64, i64, i64)> = None;
        let (mut max_window, mut total_window) = (0u64, 0u64);
        for component in &parts {
            let (x0, y0, x1, y1) = component.box_padded(w, h);
            let (ww, wh) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
            if let Some(message) = over_window_limit(ww, wh) {
                return Err(message);
            }
            let area = ww as u64 * wh as u64;
            max_window = max_window.max(area);
            total_window = total_window.saturating_add(area);
            let window = (
                i64::from(x0),
                i64::from(y0),
                i64::from(x1) + 1,
                i64::from(y1) + 1,
            );
            union = Some(match union {
                None => window,
                Some(u) => (
                    u.0.min(window.0),
                    u.1.min(window.1),
                    u.2.max(window.2),
                    u.3.max(window.3),
                ),
            });
        }
        // ONE read of the windows' union on the layer (13B-2c-2a), threaded
        // through every component: a window pixel is reachable through the
        // rect exactly when it is on-canvas and on-layer.
        let Some(rect) =
            union.and_then(|u| canvas_rect_on_layer((self.width, self.height), layer, u))
        else {
            return Ok(None);
        };
        let union_pixels = union.map_or(0, |u| ((u.2 - u.0) * (u.3 - u.1)) as u64);
        control.report(HEAL_PLAN_PROGRESS, 2)?;
        control.reserve(heal_scratch_bound(
            n,
            parts.len() as u64,
            union_pixels,
            max_window,
        ))?;
        let mut pixels = rect_image_with_job(&layer.pixels, rect, control)?
            .ok_or_else(|| ALLOCATION.to_string())?;
        let offset = rect_offset(layer, rect);
        let mut changed = false;
        // Each component's stage-4 reports land in a share of the post-plan
        // range weighted by its padded window area — the quantity its planes
        // and solve cost in proportion to (`precision_retouch_heal`'s rule).
        let solve = control.range(HEAL_PLAN_PROGRESS, 1.0);
        let total = total_window.max(1) as f64;
        let mut done = 0u64;
        for component in parts {
            control.check()?;
            let (x0, y0, x1, y1) = component.box_padded(w, h);
            let (ww, wh) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
            let area = ww as u64 * wh as u64;
            let (region, write, source) =
                component.window_buffers(overlay, w as usize, (x0, y0), (ww, wh), control)?;
            changed |= heal_window_into_pixels_with_context(
                (self.width, self.height),
                &mut pixels,
                offset,
                (i64::from(x0), i64::from(y0), ww, wh),
                &source,
                &region,
                &write,
                strength,
                solve.range(done as f64 / total, (done + area) as f64 / total),
            )?;
            done += area;
        }
        if !changed {
            // Identity result: refuse rather than mint an unchanged copy.
            return Ok(None);
        }
        let (x, y, rw, rh) = rect;
        let raw = pixels.into_raw();
        let Some(raster) =
            layer
                .pixels
                .with_rect_with_job(x, y, rw, rh, raw.as_chunks::<4>().0, control)?
        else {
            return Ok(None);
        };
        Ok(self.with_layer_raster(idx, raster))
    }
}

/// THE healing write-back, shared by [`DrDocument::heal_layer`] and by
/// `doc_inpaint`'s spot heal and content-aware fill. Everything but `doc`
/// and `idx` is WINDOW-sized.
///
/// `win` is `(x, y, w, h)` in canvas coordinates. `source` is straight RGB,
/// 3 bytes per window pixel. `region` is the solve domain (≥ 128 = in).
/// `write` is the write-back weight, and it is subordinate to `region` and
/// to the layer: `write[p]` is forced to 0 wherever `region[p] < 128` or the
/// destination pixel is outside the layer or fully transparent, so no pixel
/// can ever receive an unsolved source. `strength` scales it.
///
/// `Err` when the window exceeds [`MAX_SOLVE_WINDOW_PIXELS`]; `Ok(None)`
/// when the index is out of range, `strength` is not finite, a buffer is
/// short, the window is empty, or no byte moved.
///
/// The whole-layer form, kept for `core/tests/heal_tests.rs`'s
/// window-contract test: since 13B-2c-2a every production caller (heal, spot
/// heal and content-aware fill) runs
/// `heal_window_into_pixels_with_context` over a `layer_rect::rect_image`.
/// It takes a bare `&DrDocument`, reads `layers.get` rather than
/// `raster_layer` and is deliberately NOT lock-gated (`doc_lock`'s module doc
/// records this too, so nobody "fixes" it): the locks and the group guard are
/// enforced one level up, at `heal_layer` / `spot_heal_layer_job` /
/// `content_aware_fill_source_job`, which is where a caller's real layer index
/// enters.
pub fn heal_window_into_layer(
    doc: &DrDocument,
    idx: usize,
    win: (i64, i64, usize, usize),
    source: &[u8],
    region: &[u8],
    write: &[u8],
    strength: f32,
) -> Result<Option<DrDocument>, String> {
    let Some(layer) = doc.layers.get(idx) else {
        return Ok(None);
    };
    if !strength.is_finite() {
        return Ok(None);
    }
    let mut pixels = layer.pixels.to_contiguous()?;
    let changed = heal_window_into_pixels_with_context(
        (doc.width, doc.height),
        &mut pixels,
        layer.offset,
        win,
        source,
        region,
        write,
        strength,
        JobContext::NONE,
    )?;
    if !changed {
        return Ok(None);
    }
    Ok(doc.with_layer_pixels(idx, pixels))
}

/// One window's solve and write-back, straight into a layer pixel buffer —
/// so a stroke (or a fill) with several components pays one buffer clone, not
/// one per component. [`heal_window_into_layer`] is the same call on a whole
/// layer, kept for the window-contract test; every production caller runs
/// this over a `layer_rect::rect_image`. Returns whether any byte moved.
// The parameter list is the window contract of `heal_window_into_layer`
// with the layer unpacked plus the job context; bundling them into a struct
// would only move the count somewhere else.
#[allow(clippy::too_many_arguments)]
pub(crate) fn heal_window_into_pixels_with_context(
    canvas: (u32, u32),
    pixels: &mut RgbaImage,
    offset: (i32, i32),
    win: (i64, i64, usize, usize),
    source: &[u8],
    region: &[u8],
    write: &[u8],
    strength: f32,
    control: JobContext<'_>,
) -> Result<bool, String> {
    let dimensions = pixels.dimensions();
    crate::heal_writeback::heal(
        canvas,
        dimensions,
        pixels.as_mut(),
        offset,
        win,
        source,
        region,
        write,
        strength,
        control,
    )
}

/// The refusal for a window over [`MAX_SOLVE_WINDOW_PIXELS`], or `None` when
/// it fits. Both the healing brush (before it builds a component's buffers)
/// and the shared seam (for every other caller) ask this, so the sentence
/// the user reads has one home.
pub(crate) fn over_window_limit(ww: usize, wh: usize) -> Option<String> {
    let pixels = (ww as u64).checked_mul(wh as u64)?;
    if pixels <= MAX_SOLVE_WINDOW_PIXELS {
        return None;
    }
    let megapixels = |v: u64| v as f64 / 1_000_000.0;
    Some(format!(
        "the healed region spans a {ww} x {wh} box ({:.1} megapixels); healing needs working \
         buffers over that box and is limited to {:.0} megapixels — heal it in shorter \
         strokes, or select a smaller region.",
        megapixels(pixels),
        megapixels(MAX_SOLVE_WINDOW_PIXELS)
    ))
}

/// The refusal for a call whose components' boxes together exceed
/// [`MAX_HEAL_PLAN_PIXELS`], or `None` when they fit. Worded for the two
/// gestures that reach it — a stroke and a dragged region — exactly as
/// [`over_window_limit`] words the per-component bound.
fn over_plan_limit(parts: &[Component], w: u32, h: u32) -> Option<String> {
    let mut total = 0u64;
    for part in parts {
        let (x0, y0, x1, y1) = part.box_padded(w, h);
        let box_pixels = u64::from(x1 - x0 + 1) * u64::from(y1 - y0 + 1);
        total = total.saturating_add(box_pixels);
    }
    if total <= MAX_HEAL_PLAN_PIXELS {
        return None;
    }
    let megapixels = |v: u64| v as f64 / 1_000_000.0;
    Some(format!(
        "the healed region falls in {} separate places whose working boxes come to {:.0} \
         megapixels; healing needs working buffers over every one of them and is limited to \
         {:.0} megapixels in one go — heal it in shorter strokes, or select a smaller region.",
        parts.len(),
        megapixels(total),
        megapixels(MAX_HEAL_PLAN_PIXELS)
    ))
}
