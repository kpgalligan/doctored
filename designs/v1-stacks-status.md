# Merge and stacks (ADR)

- **Status:** Accepted, closed 2026-10-19.
- **Phase / dates:** first wave 2026-10-11 to 2026-10-12, phases 1 to 4c 2026-10-16 to 2026-10-19.
- **Originals:** [archive/v1-stacks-status.md](../archive/v1-stacks-status.md) (the working record at close) and
  [archive/v1-stacks-shipped.md](../archive/v1-stacks-shipped.md) (its shipped companion).
- **Takes:** `~/devel/doctored-fixtures/photoshop/auto-align-564/take1`, `panorama-564/take1`–`take3`,
  `hdr-toning-564/selftest-1`, `selftest-2` and `take3`; the runners are in `scripts/photoshop-probe/`.
- **Key commits:** given in each Shipped and Phase section.

## Context

This phase brought the merge and stack commands to 8, 16 and 32 bits, from the UI and over MCP. Every item in
the phase plan shipped. Live Stack Modes is not built, by decision: smart objects were removed on 2026-10-10;
EDR display shipped in v1.x.

## Decisions in force

- **A Stack Mode is a one-shot statistic into ONE new raster layer**, not a live mode (smart objects were
  removed, [ADR](convert-to-smart-object-status.md)); the layers read are kept and hidden.
- **No `.rz` change.** A stack is ordinary raster layers; alignment is the core's `perspective_set`, the
  statistic is one core request (`stack_mode`, `stack_stat.rs`), a merged HDR document is an ordinary 32-bit
  float document. DRDC word 79 stays unused and is the one to take if a later item needs it. Merge to HDR is
  the kernel `merge_hdr.rs`, the adapters `doc_merge_hdr.rs` and six `dr_*merge_hdr*` exports.
- **The alpha rule** (`stack_stat.rs`'s module doc is the statement). The extent is the union of the layers'
  pixel rectangles; a layer is transparent outside its own. The layers with alpha above 0 are present; none
  leaves the pixel transparent. The result's alpha is the largest alpha present, in every mode. Mean is
  alpha-weighted per colour channel; every other mode reads the layers at least half covered (all present
  ones when none is), unweighted. Statistics are of the stored values: 8- and 16-bit codes in the document's
  encoding, rounded half up; linear values at float, unclamped; a Range past 65504 is stored as 65504.
  Masks, opacity, blend modes and styles are not read.
- **HDR Toning flattens, in the core (phase 4a).** `hdr_toning.rs` (kernel: the request, the three maps,
  `analyse`'s plane statistics) and `doc_hdr_toning.rs` (a u16 render through `promoted_image` under Preserve,
  the map at RGBA16, `flattened_u16_document`). The maps route through `adjust_tone::Exposure::apply_linear`,
  `ToneMap::Reinhard` and `equalize::Histogram`; the encode is the profile's `FloatPolicy` under Clip. An 8-bit
  document is promoted flattened and narrowed back (dither none) by `HdrToningRun`; a float one narrows to 8
  bits only when asked. The plan is the largest stage with the outputs held before it plus
  `HDR_TONING_MARGIN_BYTES_PER_PIXEL`.
  Local Adaptation (4b) is `hdr_local_adaptation.rs` behind `HdrRequest::Local`: log luminance from the 0.1 %
  alpha-weighted quantile (`equalize::Histogram::quantile`), the Toning Curve through `CurveLuts`, an edge-aware
  base from `sampling_presence::base_delta_in` (Radius above 192 runs at `RADIUS_LIMIT`), the base compressed
  into eight stops, Detail on the rest, then `adjust_color::Vibrance` and `ShadowsHighlights::sliders`.
- **HDR Toning measured against Photoshop.**
  Photoshop 26.10.0 takes the method only as `CnvM` with `hdrOptions` {`Vrsn` 6, `method` typed
  `hdrToningMethodType`} (form C1; the `hdrToning` event is refused), members LA `hdrtype4`, EG `hdrtype2`, HC
  `hdrtype1`, EH `hdrtype3` (`hdr-toning-564/selftest-2`). Take `hdr-toning-564/take3`, gate max 2 / mean 0.5
  codes at 8-bit scale, chart / merged bracket: only EG at (0, 1) PASSES; every other EG row, HC, EH and LA at its
  defaults MISS (worst EG 117.1 max; per-row numbers in `fixtures/photoshop_hdr_toning/rows.json`, pinned by
  `float_hdr_toning_photoshop.rs`). HC-LUM fitted the chart but missed the held-out merged row, so nothing
  swapped and every member ships as declared, its numbers disclosed. Gamma: CLAMPED (Photoshop tones LA gamma
  3.25 as 2); a preset named in the descriptor (`Nm  `) is SELECTED.
- **HDR Toning's built-in preset is Default (phase 4c).** HDR Toning's built-in preset is Default; user presets
  are saved from the Preset popup. A recorded call keeps the preset's explicit values, never its name
  (`HdrToningPreset.recordedArguments` in `ActionRecorder.recordAgentCall`).
- **One implementation per algorithm.** Registration is Vision, then the core's feature fallback
  (`feature_points.rs`, `feature_register.rs`, `dr_feature_register`), which also cross-checks Vision; the warp
  is the core's, solving its own homography from `source_rect` and `quad`. Swift's `Homography` converts
  Vision's matrix and projects four corners; it resamples nothing. A later item calls `AutoAlign.register`.
- **Never warp on an estimate nothing vouches for.** `AutoAlign.register` answers nil unless an estimate reaches
  `AutoAlign.agreementThreshold` (0.6); the layer is left untouched and named. Merge to HDR leaves an
  unregistered frame out by name and refuses below two frames; Auto-Blend with "Align first" refuses the whole
  blend, naming the layer, and never merges a frame unaligned in silence.
- **The feature step and its gates (phase 3a, `AutoAlignFeatures.swift`).** Once a call: Harris corners,
  oriented 8 × 8 descriptors, RANSAC homography F (≥ 24 inliers at 3.0 px, fixed seed). When F exists every
  homographic or similarity answer must lie within `crossCheckBound` 2.0 px of F's inliers; Vision first, else F
  itself when its inliers cover more than half the overlap's 4 × 4 cells with an RMS within 1.5 px, it is
  `isUsable` over its overlap (`Homography.overlap(in:)`) and it reaches the agreement score. A frame under
  512 px is detected on its 2× upsampled level first. Collage and Reposition take the higher-scoring
  vouched one of Vision's answer and the inliers' own reduction (`inlierReduction`). THE LENS TEST: when
  `align_distortion_pair::pair_coefficient`'s H + k departs from H by 1.0 px or more at the inliers, Geometric
  Distortion off refuses the layer (`AutoAlign.lensRefusal`); on, the bound is 8.0 px. The departure is
  measured at the inliers. Collage and Reposition with Geometric Distortion register as Perspective
  and reduce after the fit
  (`AutoAlignPlanarLens.swift`). The plan charges `featureScratch`.
- **The reach table** (`python3 scripts/reach-probe/run.py --output <dir> --before <HEAD's reach.json>`,
  real-photo views of `core/tests/fixtures/registration_photo/sky.jpg` at f 600, yaw 5–40°, overlap 15–70 %,
  with and without k −0.05): 27 / 27 plain rows within 2.0 px, every lens row refused with Geometric
  Distortion off, 26 / 27 lens rows within bound with it on, 0 / 138 false accepts. Its four limits have
  since moved: B-20-15 and B-30-15 register (the upsampled level), B-40-50 registers 0.37 px (usable over its
  overlap), B-20-30-lens refuses for the lens (its inliers on the upsampled level depart 1.42 px);
  `run.py` names no residual or no-estimate row any more.
- **A merge is tested against the radiance that made its frames**, not against another editor: frames are
  authored from a known scene through the sRGB curve, bounds fixed before measuring (1e-3 at 16 bits, 0.06 at
  8 bits in the core; 5e-3 at 16 bits through the app).
- **There is no conversion into 32-bit float.** Merge to HDR builds a NEW float document
  (`raster_float`, `from_parts_float`); it never converts the source.
- **Vision's conventions are measured, not assumed** (`AutoAlign.swift`'s header): both requests map the
  targeted image to the handler's, in pixels, bottom-left origin; `warpTransform` acts on a column vector.
- **Auto-Align follows take1** (`auto_align_stack_take.py`). R1: the reference is the BOTTOM layer (the lowest
  selected index, or the one position-locked layer). R2: the canvas grows to the union of the canvas, the
  reference and every aligned layer, through Reveal All's `canvasResized`, the selection carried
  (`AutoAlign.alignedCanvas`); Auto-Align and Auto-Blend's Panorama grow it, Reduce Noise, Stack Images and
  Merge to HDR keep it. R3: Auto IS Perspective. Collage is the Perspective answer reduced to its least-squares
  similarity (`Homography.similarityFit`), else the translation; Reposition is the translation alone.
- **The union canvas is bounded once for every projection**: at most 100 MP (`RasterImage.maxResizePixels`)
  and within the channel budget, else `AutoAlign.tooBigMessage` and nothing changes. Cylindrical and Spherical
  hold their projected union to the same bound before any warp. No admission for the growth (Reveal All admits
  none either).
- **Cylindrical and Spherical follow take2's C** (`AutoAlignReprojection.swift`, `align_projection.rs`). Each
  layer is registered as Perspective; only homography answers count; one focal is fitted (the f that makes
  K⁻¹·N·K closest to a rotation), each rotation is the polar factor. Every layer, the reference too, is
  reprojected onto the reference camera's surface, clipped at the frame edge; the canvas is the projected
  union. No homography or no focal refuses with `AutoAlign.focalRefusal` and nothing changes (take1's Spherical
  on a pure shift is refused, as take1's Cylindrical did).
- **Vignette Removal follows take2's V, re-read offline** (`panorama_lens_reread.py`). ONE coefficient for the
  stack, V = 1 + a·ρ² (`lens_model::pa_falloff`), fitted in linear light to the registered overlaps' ratios
  with a per-layer exposure as a nuisance (never applied); every layer multiplied by gain / V, gain the mean of
  V (`align_vignette.rs`). Vignette Removal REPROJECTS: Perspective and Auto switch to Spherical; Cylindrical
  and Spherical keep their surface; Collage and Reposition IGNORE it. The fit refuses below 1,000 ratios.
- **Lens availability** (checked against `panorama-564/take3`, `auto_align_lens_take.py`). Vignette Removal
  applies on Auto, Perspective, Cylindrical and Spherical and is ignored on Collage and Reposition; Geometric
  Distortion applies on all six (`AutoAlign.Registration.lens`, `AutoAlignDistortion.swift`: `.applies` or
  `.ignored`, no `.refused`). Both sheet boxes are always enabled.
- **Geometric Distortion follows take2's D.** ONE k for the stack, Lens Correction's model: p =
  `lens_model::radial_map(c, R, u, [k, 0, 0], 1)`, c the rect's centre, R half its longer side, k in ±0.25
  (`align_distortion.rs`). Fitted after the registration, before the focal, from two passes of NCC patch
  matches per pair; its refined maps replace Vision's for the focal and rotations (Vignette Removal keeps
  Vision's). Every `project_layer` carries k (`distortion`), so lens and projection are one resample.
  `align_distortion::folds` is `project_layer`'s refusal. `geometry_align_distortion` holds the absolute ±2
  bound against take2's D rows.
- **The projected-layer extent is an index-space clip** (phase 2b-5). A destination pixel has content
  iff its centre's source lies in the source's PIXEL-CENTRE rect [x0 + ½, x0 + w − ½] × [y0 + ½, y0 + h − ½],
  the reference's axis on a pixel centre (the host's `origin` [⌊midX⌋ + ½, ⌊midY⌋ + ½]). The rect
  (`align_projection.rs`, its one home) runs from ⌈lo − ½ − 0.01⌉ to ⌊hi − ½ + 0.01⌋ + 1 and reproduces all
  twelve take rows. The Rotate Canvas rule (ceil over the edge extent, 557) is rejected, as is 2b-2's
  outward rounding (556/557/556).
- **Geometric Distortion on Collage and Reposition is the planar lens path** (`AutoAlignPlanarLens.swift`).
  Each layer keeps its own frame size (take3). `fit_distortion` fits k over the reference and every registered
  layer; each refined map is reduced to the projection's map (`planarMap`); every layer is corrected and placed
  in ONE core resample (`project_layer`'s `"projection":"plane"`). A stack in which no layer moves a corner past
  0.1 px is refused before the fit (`planarStillMessage`). MCP: sampler must be bicubic with distortion.
- **The six phase-1 modes** (`stack_moments.rs`' module doc is the statement): Summation, Variance, Skewness and
  Kurtosis scaled by their population bounds to span [0, F], Entropy over 256 bins / log₂ min(n, 256) × F,
  Majority (a tie the median of the tied values). They read unweighted. Integers round half up and clamp; float
  is unclamped. Dimensionless results are a fraction of white, not measured against Photoshop.
- **Median and Mean round half up** (take1 R6). The one residual (stat4 Mean P3 = 27.75 read 27 in Photoshop)
  is unexplained and not chased.
- **Stack Modes on CMYK run per ink** in `cmyk_passes`' `L = 65535 − ink` domain (`cmyk_stack_mode.rs`), so
  Minimum is the least ink. Auto-Align and Reduce Noise from Stack run on CMYK
  (`takes_675_679.py` take1, A-EN): the registration estimates on each layer's Display P3 VIEW (its ink
  through the document's profile, `cmyk_layer_thumbnail_rgba8`), which every ink darkens; the warp moves all
  four inks (`perspective_set`, the CMYKA16 mesh arm); Reduce Noise's statistic is the per-ink Stack Mode.
  Vignette Removal and Geometric Distortion refuse on CMYK by the core's sentences
  (`CmykDocument.autoAlignLensRefusal`).
- **Auto-Blend Panorama follows take2** (`panorama_fill_take.py`): Seamless writes the frames' pixels
  (C-PIXELS); each frame's gain moves (G-SPLIT); a flat overlap stays with the frame below (W Lower,
  `panorama_seam::SEAM_PREFERENCE`); masks are binary. Content-Aware Fill Transparent Areas adds ONE layer,
  "<top frame> (merged)" (F-MERGED), filled by the one Content-Aware Fill over Generative Expand's coverage; a
  fill that cannot run refuses the whole call.
- **Merge Panorama composes, it does not reimplement** (`MergePanorama.swift`): `StackLoad.build`, then one job over
  `AutoAlignWork.alignedDocument`, `AutoBlendWork.blended` (Panorama, Seamless on) and `AutoBlendFill.run`,
  then `makeStack`; each stage admits its own plan. The reference is the MIDDLE source; a frame not overlapping
  it is unregistered, kept, hidden and named. Collage and Reposition refuse the lens boxes. A refusal starts
  "Merge Panorama stopped at <stage>:". `merge_panorama` is `documentFree` and recordable.
- **Memory plans are measured** by `scripts/stack-admission/measure.py` (Release, one case per process), each
  plan at least 1.25 × the measured peak. The records sit beside `StackLoad.planBytes`,
  `AutoAlign.registrationPlan`, `MergeHDR.swift`'s `mergeBytes`, in `stack_stat.rs`'s and `focus_stack.rs`'
  comments, beside `doc_align_projection::PROJECTION_MARGIN_BYTES_PER_PIXEL` (31; the shared
  `puppet_render::MARGIN_BYTES_PER_PIXEL` stays 4) and `HDR_TONING_MARGIN_BYTES_PER_PIXEL` (11). The per-phase
  rows (phases 1 to 3a) are in [the archived status doc](../archive/v1-stacks-status.md).

## Shipped

### Load Files into Stack

File ▸ Load Files into Stack… and MCP `load_files_into_stack` (`LoadFilesIntoStack.swift`,
`AgentServer+Stacks.swift`): one raster layer per file, bottom-first, the canvas the largest width and height;
later files go through the core's `place` + `rasterize`. `StackLoad.plan` admits the stack from headers before
any decode. A native `.rz` is refused. Not recorded into Actions. Commit `d92b438d`.

### Auto-Align Layers

Edit ▸ Auto-Align Layers… and MCP `auto_align_layers` (`AutoAlign.swift`, `AutoAlignProjection.swift`,
`AutoAlignSheetController.swift`, `EditorViewController+AutoAlign.swift`): six projections, Lens Correction's
Vignette Removal and Geometric Distortion, the bottom reference and the union canvas; one undo step, one
cancellable job, 8, 16 and 32 bits and CMYK. The projection and lens
homes are named in the decisions.
Commits `d92b438d`, `a0eca4e1` (already-aligned layers untouched), then phases 1, 2b-2 to 2b-5 and 3a.

### Stack Modes

Layer ▸ Stack Mode ▸ the twelve statistics and MCP `stack_mode` (`StackMode.swift`,
`EditorViewController+StackMode.swift`; core `stack_stat.rs`, `stack_moments.rs`, `doc_stack_mode.rs`,
`precision_stack_mode.rs`, `cmyk_stack_mode.rs`, `ffi_stack.rs`). One raster above the topmost layer given; one
undo step, one job, 8, 16 and 32 bits and CMYK. Commits `3bdef34a`, `097e3bbb` (the plan per depth), phase 1.

### Reduce Noise from Stack

Layer ▸ Reduce Noise from Stack… (Median or Mean) and MCP `reduce_noise_from_stack`: auto-aligns through
`AutoAlignWork`'s halves, then the statistic over the layers that aligned; layers left out are listed
(`excluded`). "Noise Reduced" lands above the topmost layer given. 8, 16 and 32 bits and CMYK. Commit
`3bdef34a`.

### Merge to HDR

File ▸ Merge to HDR… and Layer ▸ Merge Layers to HDR…, MCP `merge_to_hdr` and `merge_layers_to_hdr`
(`MergeHDR.swift`, `AgentServer+MergeHDR.swift`; core `merge_hdr.rs`): a NEW 32-bit float document, "Merged
HDR", from 8- and 16-bit frames; EVs from the caller, EXIF or the core's estimate; Align on, Remove Ghosts off
by default. Not recorded into Actions. Commit `d1684ad3`.

### Auto-Blend Layers

Edit ▸ Auto-Blend Layers… and MCP `auto_blend_layers` (`AutoBlend.swift`, `AgentServer+AutoBlend.swift`,
`AutoBlendFill.swift`; core `focus_stack.rs`, `doc_auto_blend.rs`, `graph_cut.rs`, `panorama_seam.rs`,
`panorama_gain.rs`): Stack Images, Panorama and Content-Aware Fill Transparent Areas. One undo step, one job,
8, 16 and 32 bits; CMYK refuses Panorama by name. Commits `6fe82db0`, phases 2a and 2b-1.

### Merge Panorama

File ▸ Automate ▸ Merge Panorama… (`MergePanorama.swift`, `MergePanoramaSheetController.swift`,
`AppDelegate+MergePanorama.swift`), MCP `merge_panorama` (`AgentServer+MergePanorama.swift`) and its Action step: a new
document "Untitled_Panorama<n>"; a step on it records as one placeholder (`ActionRecorder.noteCreatedDocument`).
Commit `05a9fbad`.

### HDR Toning

Image ▸ Adjustments ▸ HDR Toning… (`HdrToning.swift`, `HdrToningSheetController.swift`,
`HdrToningLocalAdaptationPane.swift`, `HdrLocalAdaptation.swift`, `HdrToningPresetControl.swift`,
`EditorViewController+HdrToning.swift`) and MCP `hdr_toning`: Local Adaptation, Equalize Histogram, Exposure and
Gamma, Highlight Compression, the presets; Image ▸ Mode ▸ 8/16 Bits/Channel on a 32-bit document opens it. 8, 16
and 32 bits; CMYK refused; one job, one undo step. Commits `2fc62152`, `dbcab40b`, `2fcb757b`.

## Shipped runs

The phase sections follow, in order; the first wave is under Shipped.

## Phase 1: Auto-Align's projections and six more Stack Modes (shipped)

`e9bab8c1`, 2026-10-16: Auto, Perspective, Collage and Reposition, the bottom reference and the union canvas;
Summation, Variance, Entropy, Kurtosis, Skewness and Majority; Stack Modes on CMYK.

## Phase 2a: Auto-Blend Panorama (shipped)

`b49b8c83`, 2026-10-16: Panorama with Seamless Tones and Colors; Auto-Blend moved to the Edit menu.

## Phase 2b-1: Content-Aware Fill Transparent Areas (shipped)

`a5c85bd7`, 2026-10-16: the fill, and the seam preference Lower (take2's W).

## Phase 2b-2: Cylindrical and Spherical (shipped)

`de5f3706`, 2026-10-16: each warp is charged its own plan plus the outputs held before it.

## Phase 2b-3: Vignette Removal (shipped)

`383e4890`, 2026-10-17: `remove_vignette_plan` is admitted alone before the warps.

## Phase 2b-4: Geometric Distortion (shipped)

`d30c3d1c`, 2026-10-17: `fit_distortion_plan` is admitted alone before the warps.

## Phase 2b-5: the planar lens path and the extent (shipped)

`bd000f44`, 2026-10-17: no new export, no `.rz` change; `RECT_SLACK` 0.01.

## Phase 3a: Auto-Align's registration reach (shipped)

`1902bc57`, 2026-10-17: every caller of `register` (Auto-Align, Auto-Blend, Merge to HDR, Stack Mode) gains the
feature step and charges `featureScratch` in `registrationScratch`.

## Phase 3b: Merge Panorama (shipped)

`05a9fbad`, 2026-10-17: `AutoBlendWork.blended`, `stackLayerRows` and `autoAlignReplyFields` are shared with
Auto-Blend and Auto-Align.

## Phase 4a: HDR Toning (shipped)

`2fc62152`, 2026-10-17: the three global methods.

## Phase 4b: Local Adaptation (shipped)

`dbcab40b`, 2026-10-17: `StackLoad.placing` runs the place and the rasterize as two jobs, each under its own
plan, so a stack of 128 px or smaller canvases loads.

## Phase 4c: HDR Toning presets (shipped)

`2fcb757b`, 2026-10-19: the Preset popup (Default, the user's own via `HdrToningPresetLibrary` and
`DR_HDR_TONING_PRESETS_DIR`) and MCP `hdr_toning` `preset`; explicit keys override.
On 2026-10-29 the eleven built-ins were removed; HDR Toning's built-in preset is Default (open items
[#687](https://github.com/kpgalligan/doctored-dev/issues/687), [#688](https://github.com/kpgalligan/doctored-dev/issues/688)).

## Open items

- Not built, by decision: Live Stack Modes.
- A built-in set of Doctored's own ([#687](https://github.com/kpgalligan/doctored-dev/issues/687)); importing a
  `.hdt` file the user owns ([#688](https://github.com/kpgalligan/doctored-dev/issues/688)).

## For the next item

- A warp leaves transparent borders; the Stack Modes' alpha rule above is what a later merge should follow.
- A merge of several frames in one job calls `AutoAlignWork.alignedDocument` (no display preparation, the
  caller's compound control) and reads rows through a kernel trait as `merge_hdr::FrameRows` does.
- Alignment across an exposure difference is measured only on authored brackets. The agreement score
  correlates gradients, which a gain does not change but clipped areas do, so frames whose highlights clip over
  large areas may come back `unregistered`. The permitted answer is a per-layer gain on
  `AutoAlign.analysisFrame`, never a second registration or a lower `agreementThreshold`.
- Checks: `make t F=` `stack_mode_tests`, `stack_mode_statistics`, `cmyk_stack_mode`, `precision_stack_mode`,
  `geometry_align_warp`, `geometry_align_projection`, `geometry_align_vignette`, `geometry_align_distortion`,
  `geometry_align_planar`, `feature_register`, `float_merge_hdr`, `layer_auto_blend`,
  `layer_auto_blend_panorama`, `graph_cut_min_cut`, `content_aware_fill_transparent`, `float_hdr_toning`,
  `float_hdr_local_adaptation`; the drives `scripts/verify_v1_stack_align.py`, `verify_v1_stack_modes.py`,
  `verify_v1_merge_hdr.py`, `verify_v1_stack_projection.py`, `verify_v1_stack_reach.py`,
  `verify_v1_auto_blend.py`, `verify_v1_auto_blend_panorama.py`, `verify_v1_merge_panorama.py`,
  `verify_v1_hdr_toning.py`; `scripts/reach-probe/run.py`; the admission drives; the jobs-probe cancel cases.
- No drive case reaches `register`'s step 3 (the translation alone after the two-pass result is set aside);
  it ran only in the standalone Vision probe. Step 4 (the homographic request alone) is the drive's
  `rotation alone` case, a 5° rotation with no shift; the reply does not say which step answered.
