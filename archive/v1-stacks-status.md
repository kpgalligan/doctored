# Merge and stacks — status

- **Shipped:** Load Files into Stack and Auto-Align Layers (2026-10-11), the first two items; Stack Modes and
  Reduce Noise from Stack (2026-10-12), items three and four; Merge to HDR with its ghost removal
  (2026-10-12); Auto-Blend Layers (2026-10-12); phase 1 (2026-10-16): Edit ▸ Auto-Align Layers… with the
  Auto, Perspective, Collage and Reposition projections, the bottom reference and the union canvas; Summation,
  Variance, Entropy, Kurtosis, Skewness and Majority; Stack Modes on CMYK. Phase 2a (2026-10-16): Edit ▸
  Auto-Blend Layers… (moved from Layer) gains the Panorama method with Seamless Tones
  and Colors. Phase 2b-1 (2026-10-16): Panorama's Content-Aware Fill Transparent Areas, and the seam preference
  Lower (take2's W). Phase 2b-2 (2026-10-16): Auto-Align's Cylindrical and Spherical projections. Phase 2b-3
  (2026-10-17): Auto-Align's Lens Correction ▸ Vignette Removal. Phase 2b-4 (2026-10-17): Lens Correction ▸
  Geometric Distortion, and take3's lens availability. Phase 2b-5 (2026-10-17): Geometric Distortion on
  Collage and Reposition, and the projected-layer extent of takes 2 and 3. Phase 3a (2026-10-17): Auto-Align's
  registration reach, the core's feature registration behind Vision. Phase 3b (2026-10-17): File ▸ Automate ▸
  Merge Panorama…, with MCP `merge_panorama` and its Action step. Phase 4a (2026-10-17): Image ▸ Adjustments ▸ HDR
  Toning… with Equalize Histogram, Exposure and Gamma and Highlight Compression (and 8/16 Bits/Channel on a
  32-bit document through it), MCP `hdr_toning`. Phase 4b (2026-10-17): HDR Toning's Local Adaptation, the
  default method, with its Toning Curve and Histogram; Load Files into Stack on tiny canvases. Phase 4c
  (2026-10-19): HDR Toning's presets (Default and the user's own; on 2026-10-29 the other
  built-ins were removed, #687, #688) and MCP `hdr_toning` `preset`.
- **Nothing in the phase plan is unbuilt.** The bounded Photoshop attempt was not made in 4c (with it the spec
  passed its length limit), so HDR Toning is not measured against Photoshop; that and the two other open items
  below are drafted as their own issues for the controller (the lane may not post to GitHub). Live Stack
  Modes is not built, by decision (smart objects were removed); EDR display shipped. The phase closes once
  those issues exist: the closing batch condenses this doc and its companion into the ADR.

## Decisions in force

- **A Stack Mode is a one-shot statistic into ONE new raster layer**, not a live mode: smart objects,
  placed-image sources and live filters were removed on 2026-10-10
  ([ADR](convert-to-smart-object-status.md)), so nothing here builds on them. Stack Mode is therefore a
  command on the selected layers that writes a new layer. The layers read are kept and hidden.
- **No `.rz` change.** A stack is ordinary raster layers; alignment is the core's existing `perspective_set`,
  the statistic is one new core request (`stack_mode`, `stack_stat.rs`), and a merged HDR document is an
  ordinary 32-bit float document. RZDC word 79 stays unused and is the one to take if a later item needs it.
  Merge to HDR's Rust and FFI are the kernel `merge_hdr.rs`, the adapters `doc_merge_hdr.rs` and six
  `rz_*merge_hdr*` exports.
- **The alpha rule** (`stack_stat.rs`'s module doc is the statement). The extent is the union of the layers'
  pixel rectangles; a layer is transparent outside its own. Per pixel, the layers with alpha above 0 are
  present, and none leaves the pixel transparent. The result's alpha is the largest alpha present, in every
  mode. Mean is alpha-weighted per colour channel over the layers present. Every other mode reads the layers
  at least half covered (all the present ones when none is), unweighted. Statistics are of the stored values:
  8- and 16-bit codes in the document's own encoding, rounded half up; linear values at float, unclamped.
  A Range past the storage bound 65504 is stored as the bound (layers at +65504 and −65504 read 65504).
  Masks, opacity, blend modes and styles are not read.
- **HDR Toning flattens, in the core (phase 4a).** `hdr_toning.rs` (kernel: the request, the three maps,
  `analyse`'s plane statistics over fresh passes, no sample buffer) and `doc_hdr_toning.rs` (render, a u16
  render decoded through `promoted_image` under Preserve, the map at RGBA16, `flattened_u16_document`, the
  carry `float_to_u16` shares). The maps route through `adjust_tone::Exposure::apply_linear`,
  `ToneMap::Reinhard` and `equalize::Histogram`; the encode is the profile's `FloatPolicy` under Clip. An 8-bit
  document is promoted flattened and narrowed back (dither none) by `HdrToningRun`; a float one narrows to 8
  bits only when asked. The plan is the largest stage with the outputs held before it plus
  `HDR_TONING_MARGIN_BYTES_PER_PIXEL`, the feature's own constant. Local Adaptation (phase 4b) is the kernel
  `hdr_local_adaptation.rs` behind `hdr_toning::HdrRequest::Local`: log luminance over a range whose low end
  is the 0.1 % alpha-weighted quantile (`equalize::Histogram::quantile`), the Toning Curve through
  `CurveLuts`, an edge-aware base from `sampling_presence::base_delta_in` (the guided filter Presence, Dehaze
  and Smart Sharpen share; epsilon a step of Strength stops, halved by Smooth Edges; Radius above 192 runs at
  `RADIUS_LIMIT`), the base compressed into eight stops, Detail on the rest, then `adjust_color::Vibrance`
  and `ShadowsHighlights::sliders` (the Develop Filter's mapping, held once). It is not measured against Photoshop.
- **One implementation per algorithm.** Registration is Vision, then the core's feature fallback
  (`feature_points.rs`, `feature_register.rs`, `rz_feature_register`; phase 3a), which also cross-checks
  Vision's answer; the warp is the core's, which solves its own homography from `source_rect` and `quad`.
  Swift's `Homography` converts Vision's matrix and projects four corners; it resamples nothing. A later
  item that needs a registration calls `AutoAlign.register` and does not write a second one.
- **Never warp on an estimate nothing vouches for.** `AutoAlign.register` answers nil unless an estimate
  reaches the agreement score (`AutoAlign.agreementThreshold`, 0.6), and the layer is then left untouched and
  named. The cut refuses a translation standing in for a perspective move (right at the centre, tens of
  pixels off at the corners; it scored 0.50 and 0.52). Merge to HDR leaves an unregistered frame out by name
  and refuses when fewer than two frames remain; Auto-Blend with "Align first" refuses the whole blend,
  naming the unregistered layer, and never merges a frame unaligned in silence.
- **The feature step and its gates (phase 3a, rulings 1–5 in the handoff;
  `AutoAlignFeatures.swift`).** Once a call, the core's Harris corners, oriented 8 × 8 descriptors and
  RANSAC homography F (≥ 24 inliers at 3.0 px, fixed seed). When F exists, every homographic or similarity
  answer must lie within `crossCheckBound` 2.0 px of F's inliers (worst 4 × 4 cell mean); Vision first, else
  F itself when its inliers cover more than half the overlap's 4 × 4 cells with an RMS within 1.5 px and it
  is vouched for. Collage judges a Vision similarity against the inliers' own similarity; otherwise Collage
  and Reposition take the higher-scoring vouched one of Vision's T and the inliers' own similarity or mean
  shift (`inlierReduction`): the agreement score is their judge (ruling 5). THE LENS TEST: when the H + k
  model fitted on the inliers (`align_distortion_pair::pair_coefficient`) departs from H by 1.0 px or more at
  the worst inlier, the frames carry a lens: Geometric Distortion off refuses the layer ("the frames carry
  lens distortion; turn on Geometric Distortion"; Cylindrical/Spherical with none registered,
  `AutoAlign.lensRefusal`); on, the cross-check bound is 8.0 px and the fit follows. Collage and Reposition
  with Geometric Distortion register as Perspective and reduce after the fit (`AutoAlignPlanarLens.swift`).
  The plan charges `featureScratch` (two grey frames, the inlier buffer, the core's peak).
- **The reach table** (`python3 scripts/reach-probe/run.py --output <dir> --before <HEAD's reach.json>`,
  real-photo views of `core/tests/fixtures/registration_photo/sky.jpg` at f 600, yaw 5–40°, overlap 15–70 %,
  with and without k −0.05; bounds 2.0 px plain, 8.0 px or the lens floor + 3.0 with the lens): at HEAD
  bd000f44 7 / 54 rows within bound and 6 wrong accepts (B-20-50-plain 16.02 px); after, 24 / 27 plain rows
  within 2.0 px (to yaw 40° at 25 % overlap on 600 × 400; a pure shift and Collage at 15 %), every lens row
  but B-20-30-lens refused with Geometric Distortion off (24 by the lens sentence; B-20-15 and B-30-15
  find no estimate), and 24 / 27 lens rows within bound with Geometric
  Distortion on (0.02–0.18 px, k −0.0494…−0.0506); 0 / 138 false accepts. B-20-30-lens (306 × 204, 36
  inliers, departure 0.70 px) is the one named residual: accepted 8.76 px off with Geometric Distortion off,
  pinned there by the probe's pass rule.
- **A merge is tested against the radiance that made its frames**, not against another editor: frames are
  authored from a known scene through the sRGB curve, and the bounds (1e-3 at 16 bits, 0.06 at 8 bits in
  the core; 5e-3 at 16 bits through the app) were fixed before measuring.
- **There is no conversion into 32-bit float.** Merge to HDR builds a NEW float document through
  `float_document`'s own gates (`raster_float`, `from_parts_float`); it never converts the source.
- **Vision's conventions are measured, not assumed**, and recorded in `AutoAlign.swift`'s header: both
  requests map a point of the targeted image to the handler's image, in pixels with a bottom-left origin, and
  `warpTransform` is applied to a column vector.
- **Auto-Align, measured against take1** (`~/devel/rasterize-fixtures/photoshop/auto-align-564/take1`,
  `scripts/photoshop-probe/auto_align_stack_take.py`). R1: the reference is the BOTTOM layer (the lowest
  selected index, or the one position-locked layer). R2: the canvas grows to the union of
  the canvas, the reference and every aligned layer, through Reveal All's `canvasResized`, the selection carried
  (`AutoAlign.alignedCanvas`); Auto-Align and Auto-Blend's Panorama grow it — Reduce Noise, Stack Images and
  Merge to HDR keep the canvas. R3: Auto IS Perspective (identical on the measured stack). Collage is the Perspective answer reduced to
  its least-squares similarity over the frame (`Homography.similarityFit`), else the translation; Reposition is
  the translation alone (since phase 3a each also weighs the feature inliers' own reduction, above); every
  projection warps only on a map the agreement score vouches for.
- **The union canvas is bounded once for every projection**: at most 100 MP (`RasterImage.maxResizePixels`)
  and within the channel budget, else `AutoAlign.tooBigMessage` ("…the result would be too big…") and nothing
  changes. Cylindrical and Spherical hold their projected union to the same
  bound before any warp. No admission is added for the growth: Reveal All admits none either, and the display is
  `PreparedDocumentEdit`'s.
- **Cylindrical and Spherical, measured against take2's C** (`AutoAlignReprojection.swift`, `align_projection.rs`).
  Each layer is registered as Perspective on the original frames, and only a homography answer counts; one focal is
  fitted to those maps (the f that makes K⁻¹·N·K closest to a rotation), and each layer's rotation is the polar
  factor. Every layer, the reference too, is reprojected onto the reference camera's cylinder or sphere, clipped
  crisply at the frame edge, and the canvas is the union of the projected layers' content (take2's 399 < 400).
  Take2's rows (974 × 399, 974 × 385, x 0/209/419) come out of the core from the take's own f 600 and ±20° yaw
  (`take2_rows_from_the_takes_focal_and_yaw`). Its 20° steps shift a frame about 36% of its width, past
  Vision's reach, so the projection drive uses ±10° (its own value noise registers there). The plane scene
  of the 6000 × 4000 admission frames registered three frames only at steps of 3° or less, so they use ±2.5°.
  Since phase 3a the feature step registers take2's own `rot_*` frames end to end
  (`verify_v1_stack_reach.py` (f): 974 × 399 and 974 × 385, x 0/208/419). No homography row, or no focal (a pure shift, identical layers), refuses with
  `AutoAlign.focalRefusal` ("…they do not turn about one point, or they overlap too little or too widely to
  align…") and nothing changes. Take1's Photoshop Spherical on a pure shift gave 1205 × 809; we refuse it.
- **Vignette Removal, fitted to take2's V re-read offline** (`scripts/photoshop-probe/panorama_lens_reread.py`;
  no new take). Photoshop's Perspective + Vignette Removal came back with exactly Spherical's bounds
  (974 × 385, left 0..555), so its output divided by its own Spherical output compares aligned pixels: the
  input's 1 − 0.3(r/R)² falloff is gone (left rings 0.909 → 0.917 over the source radius), every frame is
  scaled by about its mean falloff (0.9), and the three frames share one curve. So: ONE coefficient for the
  stack, V = 1 + a·ρ² (`lens_model::pa_falloff`, ρ over the farthest pixel centre), fitted in linear light to
  nearest-pixel ratios of the registered overlaps with a per-layer exposure as a nuisance (never applied), and
  every layer multiplied by gain / V, gain the mean of V over every listed pixel (`align_vignette.rs`). On
  take2's own construction the core reproduces Photoshop's rings within 0.014 (`geometry_align_vignette`,
  `take2_codes_reproduce_photoshops_rings`; a −0.563, gain 0.812). Vignette Removal REPROJECTS: Perspective
  and Auto switch to Spherical (take2, take3); Cylindrical and Spherical keep their surface; Collage and
  Reposition IGNORE it (take3, below). The fit's refusal below 1,000 overlap ratios and the focal refusal's
  lens sentence ("…Turn off Vignette Removal to align them with Perspective…") change nothing.
- **Lens availability, measured by take3** (`scripts/photoshop-probe/auto_align_lens_take.py`,
  `panorama-564/take3`, 2026-10-17; each lens's on case against its off twin, rules fixed in the runner's
  docstring before the take). Vignette Removal APPLIED on Auto, Perspective, Cylindrical and Spherical
  (~80% of the left layer's pixels apart, bounds equal) and was IGNORED on Collage and Reposition (bounds
  equal, 0.0% apart); Geometric Distortion APPLIED on all six. The table (`AutoAlign.Registration.lens`,
  `AutoAlignDistortion.swift`): applied → `.applies` (Vignette Removal on the four, Auto and Perspective
  onto the sphere; Geometric Distortion on all six since phase 2b-5, Collage and Reposition by the planar
  lens path below); ignored → `.ignored` (the request drops the lens). Both sheet boxes are always enabled;
  there is no `.refused`. Take3's D rows: Auto, Perspective and Spherical L/C/R 584/583/583 × 393 (union 1002 × 393),
  Cylindrical 584/583/583 × 407; Collage L (0, 1, 600, 401), Reposition L (0, 0, 600, 400) with its pixels
  changed.
- **Geometric Distortion, measured against take2's D** (`panorama_lens_reread.py`: Perspective + Geometric Distortion
  came back as the Spherical of the frame undistorted by its k 0.06, one correction on each of three frames
  of one lens). ONE coefficient k for the stack, Lens Correction's model and normalisation: a layer pixel p
  is the source of the corrected point u, p = `lens_model::radial_map(c, R, u, [k, 0, 0], 1)`, c the rect's
  centre, R half its longer side, k in ±0.25 (`align_distortion.rs`). It is fitted after the registration,
  before the focal: per pair (each layer and the layer it was registered against) NCC patch matches in the
  overlap (two passes: predicted by the Vision map, then by the first fit's lens and corrected maps), a
  homography between the corrected points per pair, and k minimising the summed squared transfer error. Its
  refined maps (corrected → corrected) replace the Vision maps for the focal and the rotations; Vignette
  Removal keeps the Vision maps (it pairs raw pixels); every `project_layer` carries k (`distortion`), the
  inverse reading the layer at `radial_map` of the pinhole point, so the lens and the projection are one
  resample. `align_distortion::folds` (`radial_folds` at the corner, or a corner with no corrected point) is
  `project_layer`'s refusal and the fit's +∞. On take2's construction, at the host's origin, the core fits
  k −0.0354 and lands Spherical 585/584/585 × 391 against Photoshop's D rows 584/583/583 × 393 (union
  1004 × 391 against 1002 × 393) and Cylindrical left 585 × 405 against 584 × 407: the ABSOLUTE ±2 bound
  is restored in `geometry_align_distortion` (phase 2b-5), with the growth check beside it. The focal
  refusal names the lenses that switched the surface ("…Turn off Vignette Removal and Geometric
  Distortion…").
- **The projected-layer extent is an index-space clip derived from Photoshop's rows** (phase 2b-5; offline
  from take2's `rot_*` and take3's `*_vig_*` rows, no take). A destination pixel has content iff its centre's
  source lies in the source's PIXEL-CENTRE rect [x0 + ½, x0 + w − ½] × [y0 + ½, y0 + h − ½], with the reference's axis on
  a pixel centre (the host sends `origin` [⌊midX⌋ + ½, ⌊midY⌋ + ½]; the principal points stay the rect
  centres). The rect (`align_projection.rs`, its one home) walks the pixel-centre perimeter and runs from
  ⌈lo − ½ − 0.01⌉ to ⌊hi − ½ + 0.01⌋ + 1. It reproduces all twelve rows (C rows L/C/R 555/556/555 at x
  0/209/419, Spherical 385, Cylindrical 399; `geometry_align_projection`'s take2 test lands each exactly),
  the smallest margin 0.09 px. The Rotate Canvas rule, ceil(bw) × ceil(bh) over the edge extent
  2f·atan(½) = 556.38, gives 557 and is rejected, as is 2b-2's outward rounding (556/557/556).
- **Geometric Distortion on Collage and Reposition is the planar lens path** (phase 2b-5,
  `AutoAlignPlanarLens.swift`). Take3: Photoshop APPLIES it and each layer keeps its own frame size
  (Reposition's D rows 600 × 400). Every layer is registered as the projection registers it,
  `fit_distortion` fits k over the reference and every registered layer anchored to it, each refined map is
  reduced to the projection's own map (`planarMap`: Collage's similarity, Reposition's translation), and
  every layer, the reference too, is corrected within its frame and placed in ONE core resample
  (`project_layer`'s `"projection":"plane"`: u = map⁻¹·p, then the lens; both u and the lens point clipped
  to the pixel-centre rect). The canvas is the union, as on the surfaces. A stack in which no registered
  layer moves a corner past 0.1 px says nothing about the lens and is refused before the fit
  (`planarStillMessage`); a position-locked layer and a placed image refuse by name (the planar forms of
  the lock and placed sentences). MCP: sampler must be bicubic with distortion on every projection.
- **The six phase-1 modes** (`stack_moments.rs`' module doc is the statement): Summation Σv; Variance m₂ / F
  in codes (exact at 8 and 16 bits); Skewness F/2·(1 + s/s_max) and Kurtosis F·(k − 1)/(k_max − 1), each
  scaled by its population bound for n layers so it spans [0, F] (n ≤ 2 or equal values: F/2 and 0); Entropy
  over 256 equal bins of [0, F] at every depth, / log₂ min(n, 256), × F; Majority the most frequent value, a
  tie the median of the tied values. They read S unweighted, as every mode but Mean does. Integers round half up
  and clamp; float is unclamped. Dimensionless results are a fraction of white, not measured against Photoshop.
- **Median and Mean round half up** (take1 R6: Median half up on all five patches, the three-layer Mean too).
  Residual, unexplained and not chased: stat4 Mean patch P3 {20, 21, 30, 40} = 27.75 read 27 in Photoshop where
  half up gives 28 (fixture `~/devel/rasterize-fixtures/photoshop/auto-align-564/take1`).
- **Stack Modes on CMYK run per ink** in `cmyk_passes`' `L = 65535 − ink` domain (`cmyk_stack_mode.rs`): each
  member's row is the C/M/Y pass and the K pass, `stack_row` runs on each, and `join_sample` maps them back on
  paper, so Minimum is the least ink and Maximum the most. Auto-Align and Reduce Noise from Stack refuse in a
  CMYK document by name (`CmykDocument.refusal(tool:)`).
- **Auto-Blend Panorama, measured against take2** (`~/devel/rasterize-fixtures/photoshop/panorama-564/take2`,
  `scripts/photoshop-probe/panorama_fill_take.py`): Seamless writes the frames' pixels (S C-PIXELS); each
  frame's gain moves (G-SPLIT, as built); a flat overlap stays with the frame below (W Lower,
  `panorama_seam::SEAM_PREFERENCE`); masks are binary (M). Content-Aware Fill Transparent Areas adds ONE layer,
  "<top frame> (merged)", the visible picture with the transparent area filled (F-MERGED); the fill is the one
  Content-Aware Fill over Generative Expand's coverage, and a fill that cannot run refuses the whole call.
- **Merge Panorama composes, it does not reimplement** (phase 3b, `MergePanorama.swift`): `StackLoad.build`, then
  one job over `AutoAlignWork.alignedDocument`, `AutoBlendWork.blended` (Panorama, Seamless on) and
  `AutoBlendFill.run`, then `makeStack`; each stage admits its own existing plan, no new plan or constant. The
  reference is the MIDDLE source; a frame that does not overlap it is unregistered
  (planar layouts register to the reference only), kept, hidden and named, and fewer than two registered
  frames refuses at Auto-Align. Auto is Perspective (R3); bicubic; layers named after their files, bottom-first
  in the dialog's list order (MCP: the caller's). Collage and Reposition refuse the lens boxes here
  (Auto-Align's own sheet keeps them, take3). A refusal starts "Merge Panorama stopped at
  <stage>:". `merge_panorama` is `documentFree` and recordable; edits on its document record as one placeholder.
- **Memory plans are measured** by `scripts/stack-admission/measure.py` (Release, one case per process).
  The measurement records sit beside `StackLoad.planBytes`, `AutoAlign.registrationPlan` and Merge to HDR's
  `mergeBytes` (`MergeHDR.swift`; gate M-merge), and in `stack_stat.rs`'s module doc. Phase 1's rows
  (2026-10-16, `python3 -B scripts/stack-admission/measure.py --app build/Build/Products/Release/Rasterize.app
  --cases register-4-jpeg8-24mp,stack-8-jpeg8-24mp,stack-4-cmyk`, gates M-register and M-stack PASS):
  `register-4-jpeg8-24mp` (three 24 MP layers warped, canvas grown to 6057 × 4045) plan 3,077,490,544 against
  max E 384,024,888, 8.014 ×; `stack-4-cmyk` plan 859,115,980 against max E 149,471,304, 5.748 ×;
  `stack-8-jpeg8-24mp` no rise (plan 673,277,544). No constant changed.
  Phase 2b-2's rows (2026-10-17, `python3 -B scripts/stack-admission/measure.py --app
  build/Build/Products/Release/Rasterize.app --cases project-3-jpeg8-24mp,project-3-png16-24mp,project-float-24mp`,
  gate M-register PASS; three 6000 × 4000 views yawed −2.5/0/+2.5° at f 6000, Cylindrical, E the whole call):
  at the mesh warps' shared margin, 4 B/px, they read 0.594 ×, 0.507 × and 0.712 ×, because the job holds
  every reprojected layer, then sets the canvas and publishes, while each warp was charged alone. So each warp
  (and the canvas step) is charged its own plan plus the `output_bytes` of the layers held before it, and the
  16-bit case still needed a margin of 31, which is the projection's own
  (`doc_align_projection::PROJECTION_MARGIN_BYTES_PER_PIXEL`, its record; the shared
  `puppet_render::MARGIN_BYTES_PER_PIXEL` stays 4, so Puppet and Perspective Warp plans are unchanged).
  At 31: `project-3-jpeg8-24mp` 1,055,666,226 against max E 464,896,360, 2.271 ×;
  `project-3-png16-24mp` 1,418,722,715 against 1,119,470,408, 1.267 ×; `project-float-24mp` 2,501,639,394
  against 1,668,302,072, 1.500 ×.
  Phase 2b-3's rows (2026-10-17, `python3 -B scripts/stack-admission/measure.py --app
  build/Build/Products/Release/Rasterize.app --cases
  vignette-3-jpeg8-24mp,vignette-3-png16-24mp,vignette-float-24mp`, gate M-register PASS; the same views,
  Perspective + Vignette Removal, so `remove_vignette` then the sphere, E the whole call):
  `vignette-3-jpeg8-24mp` plan 1,310,420,508 against max E 440,238,440, 2.977 ×; `vignette-3-png16-24mp`
  1,952,482,157 against 940,557,032, 2.076 ×; `vignette-float-24mp` 3,580,229,420 against 1,847,674,392,
  1.938 ×. No margin added; no constant changed.
  Phase 2b-4's rows (2026-10-17, `python3 -B scripts/stack-admission/measure.py --app
  build/Build/Products/Release/Rasterize.app --cases
  distortion-3-jpeg8-24mp,distortion-3-png16-24mp,distortion-float-24mp`, gate M-register PASS; the same
  views, Perspective + Geometric Distortion, so `fit_distortion` (4,070 matches, k within 2.3e-5 of 0) then
  the sphere, E the whole call): `distortion-3-jpeg8-24mp` plan 1,022,238,994 against max E 439,091,536,
  2.328 ×; `distortion-3-png16-24mp` 1,376,127,906 against 1,079,640,880, 1.275 ×; `distortion-float-24mp`
  2,427,800,738 against 1,602,847,968, 1.515 ×. No margin added; no constant changed.
  Phase 2b-5's rows (2026-10-17, `python3 -B scripts/stack-admission/measure.py --app
  build/Build/Products/Release/Rasterize.app --cases
  planar-3-jpeg8-24mp,planar-3-png16-24mp,planar-float-24mp`, gate M-register PASS; the first three plane
  frames, Reposition + Geometric Distortion, so `fit_distortion` (3,996 matches) then three plane
  projections and the canvas, E the whole call): `planar-3-jpeg8-24mp` plan 1,130,081,152 against max E
  491,290,984, 2.300 ×; `planar-3-png16-24mp` 1,514,097,152 against 1,166,607,200, 1.298 ×;
  `planar-float-24mp` 2,666,057,728 against 1,752,204,560, 1.522 ×. No margin added
  (`PLANE_MARGIN_BYTES_PER_PIXEL` not needed); no constant changed.
  Phase 3a's rows (2026-10-17, `python3 -B scripts/stack-admission/measure.py --app
  build/Build/Products/Release/Rasterize.app --cases reach-unrelated-8-24mp,reach-unrelated-16-24mp,
  reach-2-jpeg8-24mp,register-8,register-16,register-4-jpeg8-24mp`, gate M-register PASS; the plan now
  charges `featureScratch`): `reach-unrelated-8-24mp` plan 202,781,824 against max E 46,514,224, 4.36 ×;
  `reach-unrelated-16-24mp` 658,781,824 against 238,698,664, 2.76 ×; `reach-2-jpeg8-24mp` (yaw 0 and 20° at
  f 6000, aligned `homography`) 2,226,256,984 against 923,108,024, 2.41 ×; `register-8` 3.47 ×,
  `register-16` 2.81 ×, `register-4-jpeg8-24mp` 8.08 ×. No margin added (`featureMarginBytes` not needed).

## Shipped runs

[v1-stacks-shipped.md](v1-stacks-shipped.md) — Load Files into Stack, Auto-Align Layers, Stack Modes, Reduce Noise from
Stack, Merge to HDR, Auto-Blend Layers (Stack Images, Panorama, Content-Aware Fill Transparent Areas), Merge Panorama,
HDR Toning.

## Phase 2b-2: Cylindrical and Spherical (shipped)

Shipped 2026-10-16; the record is in [v1-stacks-shipped.md](v1-stacks-shipped.md) (Auto-Align Layers).

## Phase 2b-3: Vignette Removal (shipped)

Shipped 2026-10-17; the record is in [v1-stacks-shipped.md](v1-stacks-shipped.md) (Auto-Align Layers).

## Phase 2b-4: Geometric Distortion (shipped)

Shipped 2026-10-17; the record is in [v1-stacks-shipped.md](v1-stacks-shipped.md) (Auto-Align Layers).

## Phase 2b-5: the planar lens path and the extent (shipped)

Shipped 2026-10-17; the record is in [v1-stacks-shipped.md](v1-stacks-shipped.md) (Auto-Align Layers).

## Phase 3a: Auto-Align's registration reach (shipped)

Shipped 2026-10-17; the record is in [v1-stacks-shipped.md](v1-stacks-shipped.md) (Auto-Align Layers), the
decisions above ("The feature step and its gates", "The reach table").

## Phase 3b: Merge Panorama (shipped)

Shipped 2026-10-17; the record is in [v1-stacks-shipped.md](v1-stacks-shipped.md) (Merge Panorama), the decision
above ("Merge Panorama composes, it does not reimplement").

## Phase 4a: HDR Toning (shipped)

Shipped 2026-10-17; the record is in [v1-stacks-shipped.md](v1-stacks-shipped.md) (HDR Toning), the decision
above ("HDR Toning flattens, in the core").

## Phase 4b: Local Adaptation (shipped)

Shipped 2026-10-17; the record is in [v1-stacks-shipped.md](v1-stacks-shipped.md) (HDR Toning), the decision
above ("HDR Toning flattens, in the core"). `StackLoad.placing` now runs the place and the rasterize as two
jobs, each under its own plan, so a stack of 128 px or smaller canvases loads.

## Phase 4c: HDR Toning presets (shipped)

Shipped 2026-10-19; the record and the preset decision are in [v1-stacks-shipped.md](v1-stacks-shipped.md)
(HDR Toning). The bounded Photoshop attempt was not made (open items).

## Open items

- **CMYK availability of Auto-Align and Reduce Noise is unmeasured** (take1 job 4, a stack converted to CMYK,
  stalled Photoshop). Both refuse by name until a take settles it; that take authors its CMYK document
  directly (the runner's docstring).
- **The reach table's known limits**: B-40-50 (1007 × 671, 40° at 50 %) is
  refused by `isUsable`; B-20-15 and B-30-15 (250 × 167, 382 × 255 at 15 %) give the feature step no estimate,
  plain or with the lens; B-20-30-lens is the pinned 8.76 px residual (decisions above). Merge Panorama refuses
  the three by stage ("Merge Panorama stopped at Auto-Align:", naming the floating file;
  `scripts/verify_v1_merge_panorama.py` (a)).
- **HDR Toning is not measured against Photoshop.** Selftest-1
  (`~/devel/rasterize-fixtures/photoshop/hdr-toning-564/selftest-1`) answered STOP: `executeAction(CnvM)` with
  the runner's `hdrOptions` descriptor gave the plain clip conversion for all four members and every value
  (all twelve outputs identical), so the descriptor does not select a method. The three global methods ship
  their declared default members (EG-LIN, HC-RW, EH-LOGSUM), and Local Adaptation and the presets their
  declared forms, none measured against Photoshop. 4c's one bounded attempt was not made (spec length).
- These three are drafted as issues for the controller (CMYK availability: a separate take; the reach
  limits: batch-sized; HDR Toning not measured: a separate take), to be linked here when they exist.

## For the next item

- A warp leaves transparent borders; the Stack Modes' alpha rule above is what a later merge should follow.
- A merge of several frames in one job calls `AutoAlignWork.alignedDocument` (no display preparation, the
  caller's compound control) and reads rows through a kernel trait as `merge_hdr::FrameRows` does.
- Alignment across an exposure difference is measured only on authored brackets (the drive's ±2 EV value
  noise). The agreement score correlates gradients, which a gain between frames does not change, but clipped
  areas do. No bracket of a real scene exists under `~/devel/rasterize-fixtures`; frames whose highlights
  clip over large areas may come back `unregistered`. The permitted answer is a per-layer gain on
  `AutoAlign.analysisFrame`, never a second registration or a lower `agreementThreshold`.
- Verification: `python3 scripts/verify_v1_stack_align.py` (the MCP drive, union canvas, projections, the
  bottom reference and the too-big refusal included; its real-photograph and camera
  RAW cases read `~/devel/rasterize-fixtures` and print SKIP without it; its `stack turned` case drives the
  re-plan on a HEIC it writes with `/usr/bin/sips`, and passed on 2026-10-12), `python3
  scripts/verify_v1_stack_modes.py` (the six modes and CMYK too), `python3 scripts/verify_v1_merge_hdr.py` (its
  real-photograph and camera RAW cases SKIP the same way), `make t F=stack_mode_tests`, `make t
  F=stack_mode_statistics`, `make t F=cmyk_stack_mode`, `make t F=precision_stack_mode`,
  `make t F=geometry_align_warp`, `make t F=geometry_align_projection`, `make t F=geometry_align_vignette`,
  `make t F=geometry_align_distortion`, `python3 scripts/verify_v1_stack_projection.py` (Cylindrical,
  Spherical, Vignette Removal, Geometric Distortion, take3's availability, and a real photograph distorted
  at k −0.05), `make t F=feature_register`, `python3 scripts/reach-probe/run.py` (the reach table, above),
  `python3 scripts/verify_v1_stack_reach.py` (the reach end to end, lens and falloff cases), `python3 -I
  scripts/photoshop-probe/panorama_lens_reread.py` (take2's V and D, offline), `make t F=float_merge_hdr`,
  `make t F=float_filtered_geometry T=a_known_homography`, `python3 scripts/verify_v1_merge_panorama.py`
  (Merge Panorama over the reach table's plain rows, a lens row, fill, the unregistered frame, Actions), the
  jobs-probe cases "auto-align cancel", "stack mode cancel", "merge panorama cancel" and "hdr toning cancel", the
  admission drive above (gates M-load, M-register, M-stack, M-merge, M-tone, R), `make t F=float_hdr_toning`,
  `make t F=float_hdr_local_adaptation`,
  `python3 scripts/verify_v1_hdr_toning.py` and `python3 -I scripts/photoshop-probe/hdr_toning_564_take.py
  --selftest`.
- No drive case reaches `register`'s step 3 (the translation alone after the two-pass result is set aside);
  it ran only in the standalone Vision probe. Step 4 (the homographic request alone) is the drive's
  `rotation alone` case, a 5° rotation with no shift; the reply does not say which step answered.
