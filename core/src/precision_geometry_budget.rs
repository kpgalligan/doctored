//! The typed geometry plan's WORK LIMIT and SCRATCH HEADROOM: every edit but
//! a preview (`interactive`) is bounded by its
//! caller's cap rather than the interactive `MAX_WORK_UNITS`, and the
//! one-shot jobs take a measured headroom over their summed scratch. `precision_geometry_plan::build` asks
//! `with_headroom` and then refuses above `work_limit`. The byte placed-image
//! plans reuse `one_shot` and `source_transform_scratch`
//! (`doc_source_geometry`, `doc_retained_source`).
//!
//! A one-shot commit's cap comes from its caller. A plan (no job) uses
//! `CONTROLLED_JOB_WORK`, so it carries no work refusal; a run uses
//! `sampling_gaussian::destructive_work_cap(job)`: no ceiling under a host job
//! control, `MAX_DESTRUCTIVE_WORK` without one. That holds for EVERY
//! edit that is not a preview (Resize, Crop, Canvas Size, the exact turns and
//! flips as well as the one-shot jobs), so no geometry the app runs under a
//! job control is refused for work. Only a preview (`interactive`) keeps
//! `MAX_WORK_UNITS`, and the host answers that refusal with its reduced proxy
//! or the commit request as a cancellable preview job. The paragraphs below
//! record what each batch measured under the bounds it had then.
//!
//! A filtered canvas turn takes the 1.25× headroom over its measured
//! peak (12 MP 16-bit at 17°: max E 401,129,832, unscaled plan 404,282,288).
//!
//! A filtered turn is bounded by the one-shot `MAX_DESTRUCTIVE_WORK`
//! (only when uncontrolled: a controlled commit has none), so
//! a 24 MP 16-bit photo straightens at the default Catmull-Rom sampler (the
//! 2e9 interactive cap refused it above ~21 MP). The turn runs once as a
//! cancellable job with progress (`performingCanvasGeometry`,
//! `PrecisionGeometryGesture.finishOutcome`). The 16-bit Crop tool's PREVIEW
//! sends `interactive` and keeps `MAX_WORK_UNITS`, so a 24 MP Catmull-Rom
//! preview stays unavailable while the commit succeeds. (The memory refusal
//! batch 81 saw on the next preview was the host counting the preview's own
//! reservation twice, since fixed.) The byte turn has no work cap.
//! Measured at 6000×4000 RGBA16, bicubic, 3 runs each, one case per
//! process, E = `/usr/bin/time -l` peak − pre-edit `ri_phys_footprint`
//! (bytes): S7 straighten 7°, window (300, 200, 5400, 3600), peaks
//! 1,064,830,032 / 1,064,862,800 / 1,064,862,800 over pre 386,335,320 /
//! 386,351,704 / 386,351,704, max E 678,511,096, plan 851,278,620 (1.2546×),
//! 2,181,577,536 work units; S17 the same at 17° max E 810,943,064, plan
//! 1,016,225,660 (1.2531×); R17 Rotate Canvas 17° (6908×5580) peaks
//! 1,197,327,536 / 1,197,278,384 / 1,197,278,384 over pre 386,400,856 /
//! 386,351,704 / 386,368,088, max E 810,926,680, plan 1,016,225,660
//! (1.2532×), 2,775,358,080 work units. The baseline twin's peaks
//! (386,384,496 / 386,368,112 / 386,384,496) sit at pre. Retained
//! 192,012,955, with the 5/4 headroom above unchanged.
//!
//! A typed transform COMMIT (`affine`, `perspective`, `affine_set`,
//! `perspective_set` without `interactive`) is bounded by
//! `MAX_DESTRUCTIVE_WORK` too (only when uncontrolled), so 16-bit
//! ⌘T, Distort, Transform Again and the set transforms of a 24 MP layer
//! commit at Catmull-Rom; every preview sends
//! `interactive` and keeps `MAX_WORK_UNITS`. The commit runs once as a
//! cancellable job with progress (`PrecisionGeometryWork`'s `.transform`,
//! `.transformSet` and `.duplicateTransform` arms pass the job's control to
//! the core). It takes the same 5/4 headroom as a turn: the unscaled plan
//! covered only ~1.003× its measured peak. Measured with the turn's method
//! (6000×4000 RGBA16 sRGB opaque photo, bicubic, 3 runs each, one case per
//! process, E = `/usr/bin/time -l` peak − pre-edit `ri_phys_footprint`,
//! bytes): A17 `affine` layer 0 17° about (3000, 2000), peaks 1,197,147,312 /
//! 1,197,180,080 / 1,197,180,080 over pre 386,171,480 / 386,220,632 /
//! 386,237,016, max E 810,975,832, unscaled plan 812,980,528, plan
//! 1,016,225,660 (1.2531×), 2,775,358,080 work units; S17 `affine_set` [0]
//! the same matrix, peaks 1,197,147,312 / 1,197,163,696 / 1,197,147,312 over
//! pre 386,171,480 / 386,204,248 / 386,171,480, max E 810,975,832, the same
//! plan (1.2531×) and units; P `perspective` quad (0, 0), (6000, 0),
//! (6900, 4600), (−900, 4600), peaks 1,154,024,624 / 1,154,008,240 /
//! 1,153,991,856 over pre 386,171,480 / 386,155,096 / 386,138,712, max E
//! 767,853,144, unscaled plan 770,311,984, plan 962,889,980 (1.2540×),
//! 2,583,360,000 work units; PS `perspective_set` [0] source rect (0, 0,
//! 6000, 4000), the same quad, peaks 1,153,991,832 / 1,154,008,240 /
//! 1,154,008,240 over pre 386,187,864 / 386,171,480 / 386,220,632, max E
//! 767,836,760, the same plan (1.2540×) and units. The baseline twin's peaks
//! (386,204,272 / 386,220,656 / 386,220,656) sit at pre. Retained
//! 192,012,955.
//!
//! A PLACED image (a retained source) follows. Each re-rendered source
//! layer's own render plan (`retained_source_render::work_plan_within`) takes
//! the edit's `work_limit`, and the direct source `transform` the 16-bit ⌘T
//! of a single placed image sends (`retained_affine`, the host's
//! `retainedAffineRequest`) takes its caller's transform cap and
//! `source_transform_scratch` unless it carries `interactive`, which the
//! preview forwards from its `affine`. Under a one-shot edit the geometry
//! plan no longer charges such a layer's raster plane, which never executes
//! (`Execution::layers` renders only the source); its work units stayed, as
//! progress ranges divided by them (a later change, below, drops them). The
//! raster plane's scratch is now charged in every edit again (work
//! units still exclude it), so the placed-image plans recorded here and
//! below are lower than today's, and each ratio is a lower bound. Measured with
//! the turn's method (bicubic, 3 runs each, one case per process, E =
//! `/usr/bin/time -l` peak − pre-edit `ri_phys_footprint`, bytes) on a
//! 5700×3800 RGBA16 sRGB opaque photo bound by `retain`: at 6000×4000 every
//! case was refused first by the typed native reopen envelope
//! (`precision_native_plan`, 2,367,746,912 / P 2,234,414,912 bytes for the
//! result's raster beside its 24 MP original), so 5700×3800 was the largest
//! 3:2 size that reached this budget (5961×3974 after the measured decode
//! term, and 6144×4096 with the exact PNG bound, both below). A17 source
//! `transform` layer 0, 17° about (2850, 1900), peaks 1,080,820,840 / 1,080,788,072 /
//! 1,080,755,304 over pre 348,848,704 / 348,865,112 / 348,799,552, max E
//! 731,972,136, unscaled plan 734,141,888, plan 917,677,360 (1.2537×),
//! 2,591,644,128 work units; S17 `affine_set` [0] the same matrix, peaks
//! 1,080,804,456 / 1,080,804,456 / 1,080,788,072 over pre 348,897,880 /
//! 348,848,704 / 348,832,320, max E 731,955,752, plan 922,922,700
//! (1.2609×), 5,096,648,256 work units; P `perspective` quad (0, 0),
//! (5700, 0), (6555, 4370), (−855, 4370), peaks 1,042,334,800 /
//! 1,042,318,416 / 1,042,351,184 over pre 348,832,320 / 348,848,704 /
//! 348,881,496, max E 693,502,480, plan 874,722,220 (1.2613×),
//! 4,749,604,800 work units; R17 Rotate Canvas 17°, peaks 1,080,804,456 /
//! 1,080,804,456 / 1,080,788,072 over pre 348,832,320 / 348,897,880 /
//! 348,832,320, max E 731,972,136, plan 922,922,700 (1.2609×),
//! 5,096,648,256 work units. The baseline twin's peaks (348,881,520 /
//! 348,865,112 / 348,865,112) sit at pre. Retained 173,293,163, with the
//! 5/4 headroom above unchanged.
//!
//! With the measured 27 B/px reopen decode term (`precision_png`'s
//! record) the largest 3:2 placed image the reopen envelope admitted was
//! 5961×3974 (6144×4096 with the exact PNG bound, below). Its four commits,
//! measured with the turn's method (bicubic, 3 runs
//! each, one case per process, E = `/usr/bin/time -l` peak − pre-edit
//! `ri_phys_footprint`, pre read after the document is built, bytes): A17
//! source `transform` layer 0, 17° about the centre, peaks 1,181,844,632 /
//! 1,181,861,016 / 1,181,861,016 over pre 381,780,568 / 381,796,952 /
//! 381,780,568, max E 800,080,448, plan 1,003,102,460 (1.2538×),
//! 2,834,246,040 work units; S17 `affine_set` [0] the same matrix, peaks
//! 1,181,844,632 / 1,181,861,016 / 1,181,877,400 over pre 381,764,184 /
//! 381,796,952 / 381,813,336, max E 800,080,448, plan 1,008,347,800
//! (1.2603×), 2,834,246,040 work units; P `perspective` quad (0, 0),
//! (5961, 0), (6855.15, 4570.1), (−894.15, 4570.1), peaks 1,140,180,072 /
//! 1,140,163,688 / 1,140,147,328 over pre 381,829,720 / 381,813,336 /
//! 381,796,952, max E 758,350,376, plan 955,974,780 (1.2606×),
//! 2,645,703,168 work units; R17 Rotate Canvas 17°, peaks 1,181,844,632 /
//! 1,181,828,248 / 1,181,861,016 over pre 381,780,568 / 381,764,184 /
//! 381,780,568, max E 800,080,448, plan 1,008,347,800 (1.2603×),
//! 2,834,246,040 work units. The baseline twin's peaks (381,813,336 /
//! 381,796,952 / 381,862,488) sit at pre (381,796,952 / 381,780,568 /
//! 381,846,104). Retained 189,525,587, with the 5/4 headroom above
//! unchanged.
//!
//! A re-rendered placed layer's raster plane, which never executes, adds no
//! work units at all (its progress range is the source render's units), and
//! its scratch is skipped for a preview (`interactive`) as for a one-shot
//! commit (`skips_unexecuted_raster`, since removed: that scratch is
//! charged in every edit again while the work units still exclude it, so
//! the plans below are lower than today's and each ratio is a lower bound);
//! Resize still charges it. So the 5700×3800 S17 counts 2,591,644,128
//! units, the direct `transform`'s (5,096,648,256 before), and a 16-bit
//! placed preview runs on the placed image's unscaled 1.003–1.009× coverage
//! above, as a raster preview does. An 8-bit placed transform commit takes
//! the one-shot budget too: the byte geometry plan bounds each placed
//! layer's source render by `one_shot_work_limit`, and a byte source
//! `transform` without `interactive` takes it with
//! `source_transform_scratch`'s 5/4. Measured with the method above on a
//! 5700×3800 8-bit plasma photo placed as a retained source (its original
//! layer removed; pre 175.6 MB): S17 `affine_set` [0] peaks 542,048,976 /
//! 542,081,744 / 542,098,128 over pre 175,604,144 / 175,587,760 /
//! 175,636,912, max E 366,493,984, plan 1,901,483,776 (5.19×); R17 peaks
//! 542,081,744 / 542,032,592 / 542,081,744 over pre 175,604,144 /
//! 175,587,760 / 175,604,144, max E 366,477,600, plan 1,242,478,352
//! (3.39×); PS `perspective_set` [0] source rect (0, 0, 5700, 3800), P's
//! quad, peaks 522,437,328 / 522,453,712 / 522,486,480 over pre 175,554,992
//! / 175,571,376 / 175,604,144, max E 346,882,336, plan 1,824,363,008
//! (5.26×); A17src source `transform` layer 0, peaks 542,065,360 /
//! 542,032,592 / 541,999,824 over pre 175,620,528 / 175,554,992 /
//! 175,554,992, max E 366,477,600, unscaled plan 369,169,568 (1.0073×), so it
//! takes the 5/4: plan 461,461,960 (1.2592×), 2,591,644,128 work units. The
//! baseline twin's peaks (175,604,144 / 175,636,912 / 175,587,760) sit at pre
//! (175,587,760 / 175,620,528 / 175,571,376). Retained 86,653,601. At
//! 6000×4000 the byte S17 plan is 2,105,881,088 (with retained 96,013,913,
//! 2,201,895,001), dominated by the plan's ×6 unexecuted raster terms.
//!
//! With the exact native PNG bound (`precision_png`'s `# Encoded bound`)
//! the reopen envelope admits a 6000×4000 placed image's commits.
//! Measured with the decode term's method above (bicubic, 3 runs each, one
//! case per process, E = `/usr/bin/time -l` peak − pre-edit
//! `ri_phys_footprint`, pre read after the document is built, bytes) on a
//! 6000×4000 RGBA16 sRGB opaque photo bound by `retain`: A17 source
//! `transform` layer 0, 17° about the centre, peaks 1,197,327,512 /
//! 1,197,294,744 / 1,197,343,896 over pre 386,417,240 / 386,417,240 /
//! 386,466,392, max E 810,910,272, plan 1,016,175,680 (1.2531×),
//! 2,871,358,080 work units; S17 `affine_set` [0] the same matrix, peaks
//! 1,197,343,896 / 1,197,376,664 / 1,197,376,664 over pre 386,466,392 /
//! 386,499,160 / 386,499,160, max E 810,877,504, plan 1,021,421,020
//! (1.2596×), 2,871,358,080 work units; P `perspective` quad (0, 0),
//! (6000, 0), (6900, 4600), (−900, 4600), peaks 1,154,188,440 /
//! 1,154,188,440 / 1,154,204,824 over pre 386,417,240 / 386,450,008 /
//! 386,482,776, max E 767,771,200, plan 968,088,220 (1.2609×),
//! 2,679,360,000 work units; R17 Rotate Canvas 17°, peaks 1,197,376,664 /
//! 1,197,343,896 / 1,197,343,896 over pre 386,499,160 / 386,466,392 /
//! 386,433,624, max E 810,910,272, plan 1,021,421,020 (1.2596×),
//! 2,871,358,080 work units. The baseline twin's peaks (386,466,392 /
//! 386,466,392 / 386,499,160 and 386,515,544 / 386,499,160 / 386,482,776)
//! sit at pre (386,450,008 / 386,450,008 / 386,482,776 and 386,499,160 /
//! 386,482,776 / 386,466,392). Retained 192,013,499, with the 5/4 headroom
//! above unchanged: floors ⌈1.25 × max E⌉ 1,013,637,840 (turns) and 959,714,000
//! (perspective). With the banded writer the result saves at 16 bits
//! (`precision_png`'s O1: 1,132,553,784 for the placed R17).
//!
//! The two-layer turn: a uniform u16 raster (File ▸ New's background)
//! resamples into a tile band (`precision_sampling::uniform_source`) and the
//! plan charges that band (`uniform_band_bytes`) instead of its source copy
//! and adoption copy; `TRANSFORM_HEADROOM` and the turn's 5/4 are unchanged.
//! Measured with `precision_png_rows`' method (`MallocLargeCache=0`, 3 runs
//! each, one case per process, pre = `ri_phys_footprint` after the document
//! is built and `proc_reset_footprint_interval`, the measured call
//! `geometry(&edit, REFERENCE_WORKING_BYTES, 0, None)`, E =
//! `ri_interval_max_phys_footprint` − pre, `RUSAGE_INFO_V4`, bytes), Rotate
//! Canvas 17° bicubic: (U) `create_blank` 6000×4000 white, E 21,332,016 ×3
//! over pre 2,474,272 ×3, plan retained 1,237,659, scratch 408,512,840
//! (19.15× max E), 2,775,358,080 work units; the result retains 50,699,441
//! while the footprint after the edit is 19,136,560 above pre, so E may
//! undercount by up to 31,562,881 (scratch ≥ 7.72× E plus that). (2L) (U)
//! plus a placed 6000×4000 plasma photo, E 821,101,168 / 821,084,784 /
//! 821,101,168 over pre 387,613,248 / 387,629,632 / 387,629,632, plan
//! retained 193,252,395, scratch 1,424,691,000 (1.7351× max E),
//! 5,646,716,160 work units, total 1,617,943,395 (HEAD refused: "needs
//! 2416422387 bytes"); the result retains 551,091,499, 357,839,104 beyond
//! the input's 193,252,395, while the footprint after the edit is 320,438,536
//! above pre, so E may undercount by up to 37,400,568 (scratch ≥ 1.6595× E
//! plus that). The baseline twins read E 0 in all six runs. Floors ⌈1.25 ×
//! max E⌉: 26,665,020 (U) and 1,026,376,460 (2L). The (2L) result reopens at
//! its plan envelope (2,030,964,413: reopen retained 809,815,670, decode
//! scratch 1,221,148,743), its reopened background retaining 308,386,993
//! (the banded reopen keeps fresh tiles); a second Rotate Canvas 17° of it
//! is refused by the native reopen envelope at the 2 GiB test pin and
//! admitted at 8 GiB.
use super::GeometryEdit;
use crate::precision_render::MAX_WORK_UNITS;

/// The one-shot transform commit's headroom over its summed scratch, as a
/// `(numerator, denominator)` factor (the transform commit's measurement above).
const TRANSFORM_HEADROOM: (u64, u64) = (5, 4);

/// A filtered canvas turn: Straighten, or a Rotate Canvas whose angle is not
/// an exact quarter turn.
fn filtered_turn(edit: &GeometryEdit) -> bool {
    match edit {
        GeometryEdit::Straighten { .. } => true,
        GeometryEdit::RotateCanvas { degrees, .. } => matches!(
            crate::doc_rotate::rotation_kind(*degrees),
            Some((crate::doc_rotate::RotationKind::Filtered, _))
        ),
        _ => false,
    }
}

/// A typed transform: a layer or set affine/perspective request.
fn transform(edit: &GeometryEdit) -> bool {
    matches!(
        edit,
        GeometryEdit::Affine { .. }
            | GeometryEdit::Perspective { .. }
            | GeometryEdit::AffineSet { .. }
            | GeometryEdit::PerspectiveSet { .. }
    )
}

/// A one-shot job: a filtered turn or transform commit, not a preview
/// (`interactive`).
pub(crate) fn one_shot(edit: &GeometryEdit) -> bool {
    !edit.interactive() && (filtered_turn(edit) || transform(edit))
}

/// `MAX_WORK_UNITS` for a preview (`interactive`); `one_shot_cap`, the
/// caller's cap, for every other edit (widened from the one-shot jobs to
/// all of them: `CONTROLLED_JOB_WORK` for a plan or a
/// controlled run, `MAX_DESTRUCTIVE_WORK` for an uncontrolled run —
/// `sampling_gaussian::destructive_work_cap`).
pub(super) fn work_limit(edit: &GeometryEdit, one_shot_cap: u64) -> u64 {
    if edit.interactive() {
        MAX_WORK_UNITS
    } else {
        one_shot_cap
    }
}

/// A direct source `transform`'s scratch: a one-shot commit takes
/// `TRANSFORM_HEADROOM` over the source render's summed plan, as the
/// geometry plan's transform commit does; a preview's figure is unchanged.
pub(crate) fn source_transform_scratch(one_shot: bool, scratch: u64) -> Result<u64, String> {
    if !one_shot {
        return Ok(scratch);
    }
    let (numerator, denominator) = TRANSFORM_HEADROOM;
    scratch
        .checked_mul(numerator)
        .map(|scaled| scaled / denominator)
        .ok_or_else(|| "typed retained source scratch overflow".into())
}

/// `scratch` with the measured headroom: a filtered turn's 5/4
/// (interactive or not), a one-shot transform commit's `TRANSFORM_HEADROOM`,
/// and (#547 phase 4b) a CMYK document's Image Size the same
/// `TRANSFORM_HEADROOM`: `scripts/cmyk-admission`'s `resize-16` (24 MP
/// CMYKA16 to 9000×6000 bicubic, Release, 3 runs) measured max E
/// 1,501,430,840 against the bare summed plan 1,504,292,256 (1.002×), a
/// miss, which the headroom answers. Every other edit's figure unchanged.
pub(super) fn with_headroom(edit: &GeometryEdit, cmyk: bool, scratch: u64) -> Result<u64, String> {
    const OVERFLOW: &str = "typed geometry scratch overflow";
    let cmyk_resize = cmyk && matches!(edit, GeometryEdit::Resize { .. });
    if filtered_turn(edit) {
        scratch
            .checked_add(scratch / 4)
            .ok_or_else(|| OVERFLOW.into())
    } else if transform(edit) && !edit.interactive() || cmyk_resize {
        let (numerator, denominator) = TRANSFORM_HEADROOM;
        scratch
            .checked_mul(numerator)
            .map(|scaled| scaled / denominator)
            .ok_or_else(|| OVERFLOW.into())
    } else {
        Ok(scratch)
    }
}
