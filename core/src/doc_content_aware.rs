//! Exact Content-Aware Fill requests over the shared byte synthesis pipeline.
//! Source permission and output placement are orthogonal: a new layer matches
//! the current-and-below destination plate while sampling the requested source.
//!
//! # Admission
//!
//! [`DrDocument::content_aware_fill_scratch`] is stated in the BYTE kernel's
//! own terms. It used to borrow the typed kernel's
//! (`precision_inpaint::estimate`: 8 MiB + 2n + 128·covered + 320·window)
//! and add 40n + the layer + `projection_scratch_bytes` on top, while the
//! byte kernel reserves `doc_inpaint::inpaint_scratch_bound` — 136 bytes per
//! window pixel, not 320. On the reported 4000×3000 document with a
//! 900×900 box at (1550, 1050), ring 0 (window 1916² = 3,671,056) that
//! admitted 2,078,806,528 bytes, which the 2 GiB working limit it was
//! measured against, less the retained document, refused: the 1 MP limit
//! documented then (a hole over it is now lifted, not refused:
//! `doc_inpaint::inpaint_lift`) was unreachable on a 12 MP photograph. Measured (`/usr/bin/time -l`, `content_aware_tests
//! the_ring_does_not_drive_the_cost_at_the_cap`: a 1 MP hole, 3 MP window,
//! 2000×1500 canvas, both fills in one process) the peak footprint is
//! 330 MB INCLUDING the fixtures, against a kernel bound of 458 MB, this
//! admission's 586 MB and the old 1,294 MB. So the kernel does not need
//! what the old figure claimed; the estimate was wrong and the 1 MP limit
//! stood. The same repro now admits 779,332,224 bytes; a 1 MP hole
//! (1000×1000 at (1500, 1000)) 861,522,944 instead of 2,239,292,928. At the
//! reported size itself (a 4000×3000 plasma document, one fill per process,
//! ≈98 MB of fixtures included) the peak footprints were 397 MB for the
//! repro, 414 MB for the cap and 645 MB for the cap on a new layer sampling
//! current-and-below, whose admission was then 1,053,522,944.
//!
//! The figure (`fill_admission`, `FillAdmission::total(records)`) with `n`
//! canvas pixels, `win` = `precision_inpaint::estimate_window` (every planned
//! window is inside it): `inpaint_scratch_terms(n, layer, records, win)` —
//! whose layer term is `min(layer bytes, 4·win)` in place (the kernel
//! clones and rewrites only its windows' union rect on the layer, inside
//! `win` and on the layer) and `4n` on a new layer, and whose
//! component-record term is `min(records,
//! doc_inpaint::INPAINT_SEARCH_PIXELS)·COMPONENT_BYTES` (the kernel refuses a
//! call in more parts than that before it makes a record) — + the
//! plate term `inpaint_source::plate_scratch_bound`
//! (a new layer, or a mode that is not layer-only) + `n + 2·win` for a source
//! mask + `NEW_LAYER_CANVAS_BYTES·n` (9n) for a new layer. The
//! metadata export prices `records = area` (Swift admits by it); the edit,
//! once its coverage is in hand, re-admits at `records =
//! doc_heal::component_count_bound_where` over that coverage at
//! `Caller::Fill`'s hole threshold — exactly the kernel's own pre-walk
//! count, and the walk's count after it is no larger. Since that count is ≤
//! covered ≤ `area`, the edit's figure never exceeds the export's. It is
//! never below any nested reservation on the byte path: the FFI's 4 MiB,
//! the kernel's two `inpaint_scratch_bound` reserves and its
//! `sampling_source` extra, the new-layer plates, built on `total − plate`,
//! and the new-layer deposit's
//! `doc_retouch_output::fill_deposit_scratch`. Its refusals are the
//! fill's own words: `window_cap_message(Caller::Fill, …)` for the
//! window.
//!
//! With the in-place layer term no longer the whole layer, the
//! Current-mode canvas plate (`4n`) has to fit the kernel's 10 B canvas
//! term, so the floor was measured where the canvas dominates
//! (one call per process, NULL control, `/usr/bin/time -l` "peak memory
//! footprint", in-process `ri_phys_footprint` before the call, `E = peak −
//! pre-edit`): 6000×4000 opaque `plasma`, coverage 255 over the 100×100 box
//! at (2950, 1950), ring 0, seed 7, current, output layer; win = 214² =
//! 45,796. The edit's figure at its one start, `10n + min(4n, 4·win) + 128 +
//! 136·win + 8 MiB`, is 254,800,176 (the export's, at 10,000 records,
//! 256,080,048; both were 351,896,864 with the whole layer). Six runs: peaks
//! 367,952,496 / 367,247,984 / 366,412,400 / 366,379,632 / 366,215,792 /
//! 366,854,768; pre-edit 217,858,504 / 217,874,888 / 217,891,272 (each set);
//! E 150,093,992 / 149,373,096 / 148,521,128 / 148,521,128 / 148,340,904 /
//! 148,963,496. 1.25 × 150,093,992 = 187,617,490 ≤ 254,800,176 (largest E
//! 0.589 of it), so the canvas term stays 10; the spot heal's cases are in
//! `precision_inpaint`'s `# Admission`.
//!
//! The new-layer canvas term was 16n, never measured. It is now the
//! smallest `c` keeping each case's figure `fig16 − (16 − c)·n` at or above
//! 1.25·E, measured the way `doc_heal`'s `# Admission` measured: one
//! `dr_doc_content_aware_fill_edit` per process (NULL control, `plasma`
//! document, coverage 255 over the box, ring 0, seed 7, current-and-below,
//! new layer), `/usr/bin/time -l` "peak memory footprint", the footprint read
//! in-process (`proc_pid_rusage` `ri_phys_footprint`) just before the call,
//! `E = peak − pre-edit`, three runs per case.
//!
//! - **F1**, a 1 MP hole (the former cap): 4000×3000, the 1000×1000 box at
//!   (1500, 1000); figure at 16 1,053,522,944. Peaks 560,841,472 /
//!   558,596,864 / 564,052,736; pre-edit 109,724,032 in every run; E
//!   451,117,440 / 448,872,832 / 454,328,704; post-edit 560,825,088 /
//!   558,580,480 / 564,036,352. Baseline peaks (no call) 109,822,360 /
//!   109,773,208 / 109,805,976. Any `c ≥ 0` keeps it (0.527 of the figure
//!   at `c = 0`).
//! - **F2**, where the canvas terms dominate: 6000×4000, a 100×100 box at
//!   (2950, 1950), window 214² = 45,796; figure at 16 735,896,864. Peaks
//!   657,556,272 / 656,278,320 / 654,328,624; pre-edit 217,907,656 /
//!   217,907,656 / 217,858,504; E 439,648,616 / 438,370,664 / 436,470,120;
//!   post-edit 657,539,888 / 656,261,936 / 654,312,240. Baseline peaks
//!   217,907,680 / 217,940,448 / 217,924,064. It needs
//!   `735,896,864 − (16 − c)·24,000,000 ≥ 1.25 × 439,648,616 = 549,560,770`,
//!   so `c ≥ 8.24`: **9**. F2's figure is then 567,896,864 (largest E 0.774
//!   of it) and F1's 969,522,944 (0.469).
//!
//! The `layer` term (`n·4` for a new layer) stays: it is the temporary
//! plate document the arm holds, which is at least the kernel's own
//! `inpaint_scratch_bound` rect term over it, so F1 and F2 stand
//! unchanged, and the admission may never be below a nested reservation.
//! For any `c ≥ 0` every nested reservation stays ≤ the admission: the
//! kernel's `10n + 4R + records + 136·win + 8 MiB` (R the union rect, ≤ n;
//! records ≤ the admission's), the deposit's `14n + 4·w·h + 1 MiB` with
//! `w·h ≤ win`, and the plates' `total − plate + own ≤ total`.
//!
//! A hole over a megapixel is lifted (`doc_inpaint::inpaint_lift`),
//! not refused, and the figure above is unchanged, so one lifted case was
//! measured against it: `examples/inpaint_lift_admission.rs 8 fill`, a
//! 2600×2600 opaque position-hashed texture, coverage 255 over the 2000×2000
//! box at (300, 300) (4 MP, the most lift 1 takes), ring 0, seed 7, current,
//! output layer, NULL control; win = the canvas, 6,760,000. Release, one call
//! per process, `MallocLargeCache=0`, `/usr/bin/time -l` "peak memory
//! footprint"; the baseline is the `twin` mode (the same document, coverage,
//! request and scratch call, no edit). Figure `10n + 4·win + 1,000,000·128 +
//! 136·win + 8 MiB` = 1,150,388,608. Fill peaks 404,193,928 / 404,193,928 /
//! 404,226,696; twin peaks 62,374,248 / 62,374,248 / 62,390,632. Largest
//! fill less smallest twin E = 341,852,448; 1.25·E = 427,315,560 ≤ the
//! figure (E is 0.297 of it), so 136 stays. The typed case is in
//! `precision_inpaint`'s `# Admission`.
//!
//! That figure charged a lifted call's full-resolution window at 136 B/px, the lift-0
//! price of its pyramid, NNF and search planes, which a lifted call builds a level up: a 24 MP
//! Content-Aware crop (`scripts/crop-extend-admission/measure.py` fillcaf8, the whole new area of a
//! 4800×3200 picture cropped to 6000×4000, lift 2) was planned at 3.388× its peak and refused at
//! the 2 GiB test pin. The request now carries `covered`, the count of its coverage bytes at or
//! above `Caller::Fill`'s hole threshold (1, so a feathered ramp counts; the host's byte request
//! sends it), the admission takes `inpaint_lift(covered)` (0 without the key, so F1, F2 and
//! every request before it price as above), and a lifted call's window term is
//! `doc_inpaint::LIFTED_WINDOW_BYTES` (40); the edit refuses a `covered` that is not its
//! coverage's own count, so the admission's lift is the kernel's, and the kernel's post-plan
//! reserve passes the plan's lift. Measured: `inpaint_lift_admission 8 fill` ×3 beside `8 twin`
//! ×3 (Release, `MallocLargeCache=0`): fill peaks 404,439,688 / 404,374,152 / 404,406,920, twin
//! peaks 62,390,632 / 62,390,632 / 62,423,400, E 342,049,056, need 427,561,320 ≤ the figure
//! 501,428,608 (ratio 1.466); fillcaf8 (3 runs + twins, the Release app): P 1,528,401,587 ≥
//! need 1,414,575,130 (max E 1,131,660,104, ratio 1.351; it was 3832.4 MB).
//!
//! Selection coverage greater than zero defines the hole and weights writeback once; new output
//! deposits changed results at the same positive-coverage threshold. Native output placement is
//! shared in `precision_retouch_output.rs`. The byte edit/scratch exports and the typed request API
//! are its only entry points. `content_aware_workflow.rs` and
//! `content_aware_photographs.rs` test the FFI, independent soft-coverage arithmetic, source
//! exclusion, cancellation, output and pinned photographic quality. The byte admission
//! (`content_aware_fill_scratch` over `fill_admission`) is stated in the byte kernel's own terms —
//! `doc_inpaint::inpaint_scratch_terms` over `precision_inpaint::estimate_window` (in the fill's
//! words, `Caller::Fill`), component records, `inpaint_source::plate_scratch_bound` and
//! new-layer/source-mask terms — never below any nested byte reservation; this module doc's `#
//! Admission` holds the measurement. The new-layer arm lifts its result through
//! `doc_retouch_output`'s polled rect deposit (`content_aware_fill_deposit`) rather than a
//! canvas-sized lifted image and `with_layer_pixels`. Its canvas term
//! `NEW_LAYER_CANVAS_BYTES` is measured at 9 (was 16); the raw numbers are in `# Admission`.
//! `FillAdmission` keeps the terms and prices them by `total(records)`: the in-place layer
//! term is `min(layer bytes, 4·window)`, the metadata export charges the box's area in records, and
//! the edit re-admits at the coverage's `component_count_bound_where` (the kernel's own pre-walk
//! count) after its bounds check; `heal_job`'s cancel-bench also times the fill's new-layer
//! deposit.
use crate::doc::DrDocument;
use crate::job_control::JobContext;
use crate::layer_raster::LayerRaster;
use crate::precision_retouch::{RetouchEdit, RetouchOp, RetouchOutput};
use crate::retouch_sampling::RetouchSampling;

pub(crate) fn request(text: &str) -> Result<RetouchEdit, String> {
    let edit = RetouchEdit::parse(text)?;
    if edit.op != RetouchOp::ContentAwareFill {
        return Err("content-aware request requires op=content_aware_fill".into());
    }
    Ok(edit)
}
/// Bytes per canvas pixel only the new-layer arm charges beside the kernel:
/// its source and destination plates, the healed result's tiles and the
/// polled rect deposit (`doc_retouch_output::fill_deposit_scratch`, which
/// stays under this admission's total). Measured (the module doc's
/// `# Admission`): the smallest value keeping F1 and F2 at or above 1.25×
/// their largest footprint, F2 binding (was 16, unmeasured). The temporary
/// document's tiled plate is the kernel's own layer term.
const NEW_LAYER_CANVAS_BYTES: u64 = 9;

/// The admission's terms (the module doc's `# Admission`). `total(records)`
/// prices them with `records` component records: the metadata export's
/// `area`, or the edit's coverage-aware `component_count_bound`. The
/// new-layer arm's two `sampling_source` calls reserve on top of
/// `total − plate` as `base`.
struct FillAdmission {
    /// Canvas pixels `n`.
    canvas: u64,
    /// The kernel's destination term: `min(layer bytes, 4·window)` in place,
    /// `4n` on a new layer.
    layer: u64,
    /// `precision_inpaint::estimate_window`'s window pixels.
    window: u64,
    /// The request box's pixels, the metadata-only record count.
    area: u64,
    /// `inpaint_source::plate_scratch_bound`, or 0.
    plate: u64,
    /// The source-mask and `NEW_LAYER_CANVAS_BYTES` terms.
    extra: u64,
    /// `doc_inpaint::inpaint_lift` of the request's `covered`, or 0 without
    /// one: the kernel's own lift once the edit has checked the count.
    lift: u32,
}

impl FillAdmission {
    fn total(&self, records: u64) -> u64 {
        crate::doc_inpaint::inpaint_scratch_terms(
            self.canvas,
            self.layer,
            records,
            self.window,
            self.lift,
        )
        .saturating_add(self.plate)
        .saturating_add(self.extra)
    }
}

impl DrDocument {
    /// Conservative metadata-only admission for an exact fill, including both
    /// captured plates, destination/lift, permission and worst component
    /// windows — the module doc's `# Admission`.
    pub fn content_aware_fill_scratch(&self, edit: &RetouchEdit) -> Result<u64, String> {
        let admission = self.fill_admission(edit)?;
        Ok(admission.total(admission.area))
    }

    fn fill_admission(&self, edit: &RetouchEdit) -> Result<FillAdmission, String> {
        if edit.op != RetouchOp::ContentAwareFill {
            return Err("expected content_aware_fill request".into());
        }
        let target = self.raster_layer(edit.layer).ok_or(
            "content-aware fill requires a raster pixel target; scalar targets remain unavailable",
        )?;
        if self.lock_block(edit.layer, crate::doc_lock::EditKind::Pixels) != 0 {
            return Err("content-aware fill target pixels are locked".into());
        }
        if target.retained_source.is_some() && edit.output == RetouchOutput::Layer {
            return Err("Rasterize the retained source before filling its pixels".into());
        }
        if edit.x < 0 || edit.y < 0 {
            return Err("content-aware selection bounds must be inside the canvas".into());
        }
        let bounds = (edit.x as u32, edit.y as u32, edit.width, edit.height);
        let area = u64::from(edit.width) * u64::from(edit.height);
        let window = crate::precision_inpaint::estimate_window(
            (self.width, self.height),
            bounds,
            area,
            edit.ring,
            crate::doc_inpaint::Caller::Fill,
            crate::spot_heal_type::SpotHealType::ContentAware,
        )?;
        let mode = edit.mode()?;
        if let Some(mask) = &edit.source_mask {
            mask.validate(self.width, self.height)?;
        }
        if edit.output == RetouchOutput::NewLayer {
            if let Some(reason) = self.retouch_layer_refusal(edit.layer) {
                return Err(reason);
            }
            if !self.native_layers_fit_dimensions(
                self.layers
                    .iter()
                    .map(|l| l.pixels.dimensions())
                    .chain(std::iter::once((self.width, self.height))),
            ) {
                return Err("content-aware output exceeds native layer capacity".into());
            }
        }
        let new_layer = edit.output == RetouchOutput::NewLayer;
        let n = u64::from(self.width) * u64::from(self.height);
        // In place the kernel clones and rewrites only the windows' union
        // rect on the layer, which lies inside the estimate window.
        let layer = if new_layer {
            n.saturating_mul(4)
        } else {
            target.pixels.byte_len().min(window.saturating_mul(4))
        };
        let plate = if new_layer || !mode.is_layer_only() {
            crate::inpaint_source::plate_scratch_bound(self)?
        } else {
            0
        };
        let mask = if edit.source_mask.is_some() {
            n.saturating_add(
                window
                    .min(crate::doc_heal::MAX_SOLVE_WINDOW_PIXELS)
                    .saturating_mul(2),
            )
        } else {
            0
        };
        let lift = if new_layer {
            n.saturating_mul(NEW_LAYER_CANVAS_BYTES)
        } else {
            0
        };
        Ok(FillAdmission {
            canvas: n,
            layer,
            window,
            area,
            plate,
            extra: mask.saturating_add(lift),
            lift: edit.covered.map_or(0, crate::doc_inpaint::inpaint_lift),
        })
    }
    /// Exact final candidate. No source mask runs the shared in-place kernel
    /// unchanged; new-layer output deposits the fully soft-weighted solve
    /// wherever coverage is nonzero and the destination changed. Partially
    /// transparent plate pixels retain their alpha and therefore compound
    /// alpha when composited above it.
    pub(crate) fn content_aware_fill_edit(
        &self,
        edit: &RetouchEdit,
        coverage: &[u8],
        job: JobContext<'_>,
    ) -> Result<Option<Self>, String> {
        let admission = self.fill_admission(edit)?;
        let n = self.width as usize * self.height as usize;
        if coverage.len() != n {
            return Err("content-aware selection extent mismatch".into());
        }
        let threshold = crate::doc_inpaint::Caller::Fill.hole_threshold();
        let mut covered = 0_u64;
        for (i, c) in coverage.iter().enumerate() {
            job.every(i)?;
            if *c == 0 {
                continue;
            }
            if *c >= threshold {
                covered += 1;
            }
            let (x, y) = (
                (i % self.width as usize) as u32,
                (i / self.width as usize) as u32,
            );
            if x < edit.x as u32
                || y < edit.y as u32
                || x >= edit.x as u32 + edit.width
                || y >= edit.y as u32 + edit.height
            {
                return Err("content-aware selection exceeds its admitted bounds".into());
            }
        }
        if let Some(claimed) = edit.covered.filter(|claimed| *claimed != covered) {
            return Err(format!(
                "content-aware request covered {claimed} does not match the hole's {covered} \
                 covered pixels"
            ));
        }
        if covered == 0 {
            return Ok(None);
        }
        // Re-admitted by the coverage's own component bound: the
        // kernel's pre-walk count over the same bytes at the same threshold.
        let starts = crate::doc_heal::component_count_bound_where(
            self.width as usize,
            self.height as usize,
            |i| coverage[i] >= threshold,
            job,
        )?;
        let scratch = admission.total(starts);
        job.reserve(scratch)?;
        let job = job.with_reservation_floor(scratch);
        let eligibility = edit
            .source_mask
            .as_ref()
            .map(|m| m.compile(self.width, self.height, job))
            .transpose()?;
        if edit.output == RetouchOutput::Layer {
            return self.content_aware_fill_source_job(
                edit.layer,
                coverage,
                self.width,
                self.height,
                edit.ring,
                edit.seed,
                edit.mode()?,
                edit.ignore_adjustments,
                eligibility.as_deref(),
                None,
                edit.fill_transparent,
                job,
            );
        }
        // Each sampling source may use what the admission holds beyond the plate.
        let base = scratch - admission.plate;
        let source = crate::inpaint_source::sampling_source(
            self,
            edit.layer,
            edit.mode()?,
            edit.ignore_adjustments,
            base,
            job.range(0., 0.05),
        )?
        .ok_or("content-aware sampling source is unavailable")?;
        let plate = crate::inpaint_source::sampling_source(
            self,
            edit.layer,
            RetouchSampling::CurrentAndBelow,
            false,
            base,
            job.range(0.05, 0.1),
        )?
        .ok_or("content-aware destination plate is unavailable")?;
        let mut temp = Self::from_raster(LayerRaster::unseeded(plate.clone()));
        temp.profile = self.profile.clone();
        let Some(healed) = temp.content_aware_fill_source_job(
            0,
            coverage,
            self.width,
            self.height,
            edit.ring,
            edit.seed,
            RetouchSampling::Current,
            false,
            eligibility.as_deref(),
            Some(&source),
            false,
            job.range(0.1, 0.95),
        )?
        else {
            return Ok(None);
        };
        drop(temp);
        drop(source);
        self.content_aware_fill_deposit(
            edit.layer,
            "Content-Aware Fill",
            coverage,
            &plate.as_raw()[..n * 4],
            &healed.layers[0].pixels,
            job,
        )
    }
}
