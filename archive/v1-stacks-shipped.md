# Merge and stacks: shipped runs

The settled record of the merge and stacks items that shipped: each item's few lines, added by the batch that
ships it. [v1-stacks-status.md](v1-stacks-status.md) keeps the head, the decisions in
force, the contract of each item not yet shipped and `## Open items`. Takes live under
`rasterize-fixtures/photoshop/`, runners in `scripts/photoshop-probe/`.

## Shipped

### Load Files into Stack
- File ▸ Load Files into Stack… and MCP `load_files_into_stack` (`LoadFilesIntoStack.swift`,
  `AgentServer+Stacks.swift`). Each file becomes one raster layer named after its file, bottom-first; the
  menu sorts the chosen files by name, MCP keeps `paths` order. The canvas is the largest width and height;
  a smaller file sits at the top-left, unscaled.
- The document is what File ▸ Open gives the FIRST file (depth, profile, the working-space adoption of an
  8-bit open). Each later file goes through the core's `place` + `rasterize`, so it is converted exactly as
  Place converts. The reply and the UI's notice say which files were converted.
- A camera RAW takes its default development, a PSD or layered TIFF its composite, a PDF or SVG its first
  page at the default resolution. A native `.rz` is refused by name: nothing sizes it before decoding.
- `StackLoad.plan` sizes every file from its header and admits the whole stack before any decode, so a
  refusal opens nothing. Beside its own bytes, every layer pixel is charged 10 bytes at 8 bits
  (`ImageSourceFile.scratchPerPixel8`: the contiguous view the first projection makes of every layer), 3 at
  16 bits and 5 at float (`StackLoad.typedLayerCharge16` / `typedLayerCharge32`, since 2026-10-12, in place
  of the 19 and 60 of the place figure). One more layer measured 8.07 bytes a pixel at 16 bits and 15.92 at
  float, so the 11 and 21 charged keep 1.36 × and about 1.3 × at any layer count (gate M-load of
  `scripts/stack-admission/measure.py`). The margin is on the layers: charged only its own bytes, a 16-bit
  stack's plan fell under 1.25 × its peak past about 34 layers.
- A file that decodes at another size than its header promised — a host still (HEIC, AVIF, JPEG XL, JPEG
  2000) stored turned, whose header reports the stored width and height — re-plans the stack on the real
  sizes and resizes the canvas, larger or smaller, before the file lands.
- The new document has an empty history, is unsaved, and its top layer is active. Not recorded into Actions.

### Auto-Align Layers
- Edit ▸ Auto-Align Layers… (since phase 1; it was in the Layer menu) and MCP `auto_align_layers`
  (`AutoAlign.swift`, `AutoAlignProjection.swift`, `AutoAlignSheetController.swift`,
  `EditorViewController+AutoAlign.swift`). Raster layers only (not a group, an adjustment, or a text, shape
  or fill layer), none linked. The reference is the given layer, else the one position-locked layer, else
  the lowest index. One undo step, one cancellable job; the reference and the layer order do not change, and
  the canvas grows to the union (the decision above). Works at 8, 16 and 32 bits; refused on CMYK.
- Registration runs on frames at most 2048 px on the longest side, in two passes: the translational request,
  then the homographic request on the frame shifted by it. The homographic request alone follows only about
  2% of the frame's width and is wrong by hundreds of pixels past that. The fallbacks, in order: the
  translation alone, then the homographic request alone (which follows a rotation the translation cannot).
- Measured reach: Vision alone follows a pure shift of a fifth of the frame's width within a pixel or two;
  at 35% it depends on the picture, and a rotation shortens it. Since phase 3a the core's feature step
  follows real-photo views sharing a quarter of the frame (yaw 40° at f 600) and pure shifts to 15 %
  (the status doc's reach table). Past the reach the layer is reported `unregistered` and its pixels are
  unchanged.
- A layer whose corners all land within 0.1 px is `unchanged` and not resampled. A reference with nothing
  opaque is refused before any registration.
- The dialog offers the projection (Auto, Perspective, Collage, Cylindrical, Spherical, Reposition; Auto
  preselected) and Lens Correction's Vignette Removal (phase 2b-3) and Geometric Distortion (phase 2b-4),
  both boxes always enabled since phase 2b-5 (take3's table). MCP takes `reference`,
  `projection` (`auto|perspective|collage|cylindrical|spherical|reposition`, default auto), the older
  `registration` (`auto|translation`, what recorded Actions hold; read only when `projection` is absent, both
  refused) and `sampler`. The reply adds `projection`, `method` `similarity` for Collage, and `canvas` {width,
  height, origin} when the canvas changed; `homography` and `quad` are in the old canvas's coordinates.
- **Cylindrical and Spherical** (phase 2b-2, 2026-10-16; the decision is in the status doc). Swift:
  `AutoAlignReprojection.swift` (`AutoAlignSphere.focal` / `rotation`, `reprojectedDocument`: the first pass, one
  chained retry against the nearest registered layer, every plan and the union bound before any warp, each warp
  under its own admitted scratch, then `projectedCanvas`), `RasterCore+AlignProjection.swift`. Core: the kernel
  `align_projection.rs` (inverse, forward, dest rect), `InverseMap` in `puppet_render.rs` (the mesh-warp tail is
  generic over it; Puppet and Perspective Warp output unchanged), the adapters `doc_align_projection.rs` /
  `precision_align_projection.rs` and four `rz_*project_layer*` exports. Refused: any position-locked layer (every
  layer moves), a placed image, a sampler other than bicubic, CMYK. The reply adds `focal` and each row's
  `rotation` (reference ray to layer ray, row-major). No `.rz` change.
- Plan: each layer's `project_layer_plan` (`mesh_warp_plan`: the source copy, the output and its margin), admitted
  one layer at a time plus the `output_bytes` of the layers reprojected before it (the canvas step: all of them);
  the reply's `working_memory_plan_bytes` is the largest charge or the registration. The admission cases
  (`project-*`) set the projection's own margin, `PROJECTION_MARGIN_BYTES_PER_PIXEL` 31 in
  `doc_align_projection.rs` (the mesh warps' shared margin stays 4); the rows are in the status doc's
  "Memory plans are measured".
- Verification: `make t F=geometry_align_projection`, `python3 scripts/verify_v1_stack_projection.py`.
- **Vignette Removal** (phase 2b-3, 2026-10-17; the decision is in the status doc). The sheet's "Lens
  Correction:" checkbox (since 2b-4 enabled for all six; Collage and Reposition ignore it, take3), MCP `vignette` (default
  false; the reply adds `surface` whenever the job reprojected and `vignette` {coefficient, gain, samples,
  layers}), Actions record `vignette: true` only when on. Swift: `AutoAlignVignette.swift` (`surface(for:
  vignette:)`, the request, `AutoAlignWork`'s plan and step), `RasterCore+AlignVignette.swift`; the step runs
  in `reprojectedDocument` after the registration, planned on the source before any pixel work, then the warps
  reproject its result (the source when it changed no sample). Core: the kernel `align_vignette.rs` (the
  overlap sampler, the fit, the stops), the adapters `doc_align_vignette.rs` / `precision_align_vignette.rs`
  (CMYK refused), four `rz_*remove_vignette*` exports; Lens Correction's per-pixel gain is shared as
  `sampling_lens::apply_vignette_stops` and the 16-bit profile gain as `selected_filter::profile_linear_gain`
  (Lens Correction's output bit-identical). The edit answers no change when no sample changed. No `.rz`
  change.
- Plan: `remove_vignette_plan` (2 MiB, the fit's 8 MiB of samples and its n × n system, the write's copy of
  the largest layer, at 16 bits the tiles' adoption copy and the encoder, at float its adoption copy), admitted
  alone before the warps, whose held bytes start at its `output_bytes`.
- Verification: `make t F=geometry_align_vignette`, `python3 scripts/verify_v1_stack_projection.py`.
- **Geometric Distortion and the lens table** (phase 2b-4, 2026-10-17; the decisions are in the status doc).
  The sheet's second "Lens Correction:" checkbox, MCP `distortion` (default false; refused with Collage and
  Reposition by `lensProjectionMessage` until phase 2b-5, "sampler must be bicubic with distortion true.";
  the reply adds `distortion` {coefficient, matches, residual, layers}), Actions record `distortion: true`
  only when on. Swift: `AutoAlignDistortion.swift` (`LensCorrection`, `Registration.lens`, the request,
  `AutoAlignWork.fitDistortion`), `RasterCore+AlignDistortion.swift`; `reprojectedDocument` records each
  layer's anchor (the reference, or the chained neighbour), fits after the registration under its own
  admitted scratch, swaps the Vision maps for the refined ones before the focal (keeping them for Vignette
  Removal) and sends k with every `project_layer`. Core: the kernels `align_distortion.rs` (the model, the
  two matching passes, the outlier cuts, the k search, `folds`) and `align_distortion_match.rs` (the NCC
  matcher on a Jacobian-warped patch, the DLT), `lens_model::radial_unmap`, the read-only adapter
  `doc_align_distortion.rs` (both depths, CMYK refused; Vignette Removal's layer checks and linear readers
  shared), four `rz_*fit_distortion*` exports, and `project_layer`'s optional `distortion` (absent or 0
  bit-identical). No `.rz` change.
- Plan: `fit_distortion_plan` (2 MiB, 4,096 match records, the patch and the level-1 window, and the level-2
  window of the largest layer's step), admitted alone before the warps and counted in
  `working_memory_plan_bytes`; measured at 24 MP it clears the whole call's peak by 2.33 × / 1.28 × / 1.52 ×
  (the status doc's rows), so no margin of its own.
- Verification: `make t F=geometry_align_distortion`, `python3 scripts/verify_v1_stack_projection.py`.
- **The projected-layer extent and the planar lens path** (phase 2b-5, 2026-10-17; the decisions are in the
  status doc). Core: `align_projection.rs` clips to the pixel-centre rect and sizes the rect by the
  pixel-span rule (`RECT_SLACK` 0.01; `puppet_mesh::outward_bounds` unchanged for the mesh warps), and
  gains `ProjectionKind::Plane` / `LayerProjection::plane` (map and its adjugate inverse, refused unless
  finite with a determinant above 0); `doc_align_projection.rs` reads the projection first and checks its
  own key set (`"plane"` takes `map`, the surfaces refuse it; `folds` on every kind). No FFI signature, no
  new export, no `.rz` change; `align_distortion_match`'s level-2 scores moved into `Buffers`. Swift:
  `AutoAlignPlanarLens.swift` (`planarMap`, `planarStillMessage`, `AutoAlignWork.planarLensDocument`),
  `Homography.translationFit(over:)`, steps 3–4 of `reprojectedDocument` extracted as `projectionCharges`
  and `projectedDocument` (both paths), the host's `origin` on a pixel centre, the lock and placed sentences'
  planar forms; the sheet's boxes always enabled. MCP: collage and reposition take `distortion` (no
  `surface`, no `focal` in the reply; `method` translation or similarity; `homography` maps corrected
  points). Plan: the surfaces' (the fit's scratch, then each plane charge plus the outputs held), measured
  by `planar-*` (the status doc's rows).
- Verification: `make t F=geometry_align_projection`, `make t F=geometry_align_distortion`, `make t
  F=geometry_align_planar`, `python3 scripts/verify_v1_stack_projection.py` (the planar cases).
- **The registration reach** (phase 3a, 2026-10-17; the decisions and the reach table are in the status
  doc). Core: the kernels `feature_points.rs` (grey pyramid, Harris, ANMS to 2000, oriented 8 × 8
  descriptors) and `feature_register.rs` (mutual ratio matching, RANSAC on `rng::SplitMix64`, the DLT of
  `align_distortion_match`, the lens test through `align_distortion_pair::pair_coefficient`), the export pair
  `rz_feature_register` (map, inliers, pairs, departure) and `rz_feature_register_scratch_bytes`. Swift:
  `RasterCore+FeatureRegister.swift`, `AutoAlignFeatures.swift`, `AutoAlign.registered` (the reason
  beside the map; `register(…, distortion:)`), the lens reason through `reprojectedDocument`, and the
  planar lens path registering as Perspective. Every caller of `register` (Auto-Align's six projections,
  Auto-Blend, Merge to HDR, Stack Mode) gains it and charges `featureScratch` in `registrationScratch`.
  No `.rz` change.
- Verification: `make t F=feature_register`, `python3 scripts/reach-probe/run.py` (the table),
  `python3 scripts/verify_v1_stack_reach.py`.

### Stack Modes
- Layer ▸ Stack Mode ▸ Entropy / Kurtosis / Majority / Maximum / Mean / Median / Minimum / Range / Skewness /
  Standard Deviation / Summation / Variance (alphabetical; the six phase-1 modes since 2026-10-16) and MCP
  `stack_mode` (`StackMode.swift`, `EditorViewController+StackMode.swift`; core `stack_stat.rs`,
  `stack_moments.rs`, `doc_stack_mode.rs`, `precision_stack_mode.rs`, `cmyk_stack_mode.rs`, `ffi_stack.rs`).
  Two or more raster layers, none a group or an adjustment layer. The result is one raster named after the
  mode, directly above the topmost layer given, at that layer's level; it becomes the active layer. One undo
  step, one cancellable job, at 8, 16 and 32 bits and on CMYK (since phase 1, plan at 10 bytes a pixel on the
  8-bit factor, two pass rows per member).
- The layers are not aligned first, and the menu items take no options.
- The core request carries `above`, the layer the result lands above: any index not below the highest layer
  read. The app always passes the topmost layer given.
- The plan (`stack_stat::plan_scratch`) is a per-depth number of output planes
  (`stack_stat::output_plane_copies`: 7 at 8 bits, 5 at 16, 4 at float), a row per layer read plus one, the
  structure term and 1 MiB. No layer is made contiguous. The whole call (statistic, display preparation,
  published edit) peaks at 4.98 output planes at 8 bits, 3.99 at 16 and 2.51 to 2.57 at float, because the
  display part does not grow with the sample width; each factor is the smallest that holds 1.25 × at 2, 8 and
  16 layers. The plans are 1.41 × the measured peak at 8 bits, 1.27 × at 16 and 1.57 to 1.60 × at float
  (12 MP layers; gate M-stack of `python3 -B scripts/stack-admission/measure.py --app
  build/Build/Products/Release/Rasterize.app`; the rows are beside each constant). One factor for every depth
  (7, until 2026-10-12) planned a 12 MP float stack at 1.37 GB against a 0.49 GB peak. A row whose footprint
  does not rise above its baseline twins' prints `SKIP (no rise)` and is no part of the gate.

### Reduce Noise from Stack
- Layer ▸ Reduce Noise from Stack… (a sheet: Median, the default, or Mean) and MCP
  `reduce_noise_from_stack`. It auto-aligns the layers with the menu's registration and sampler, through
  `AutoAlignWork`'s own halves (`registrationPlan`, `alignedDocument`), then takes the statistic across the
  reference and every layer that aligned. It adds no registration and no plan of its own.
- A layer no registration vouched for is listed (`excluded`), and is not warped, read, hidden or moved. Fewer
  than two usable layers is a refusal naming the layers left out. The result, "Noise Reduced", lands above
  the topmost layer GIVEN, so it is never under a layer it left out.
- The warps and the new layer are one undo step and one job.

### Merge to HDR
- File ▸ Merge to HDR… (files; works with no document open) and Layer ▸ Merge Layers to HDR… (the selected
  raster layers of an 8- or 16-bit document), MCP `merge_to_hdr` and `merge_layers_to_hdr` (`MergeHDR.swift`,
  `AgentServer+MergeHDR.swift`). The result is a NEW 32-bit float document with one raster layer, "Merged
  HDR", in the source's profile; the source document gets no history step. Not recorded into Actions.
- The merge (`merge_hdr.rs`): each frame is decoded to linear light through the document profile's typed
  transfer and weighted by `alpha · weight(level) · 2^e`, `level` the largest stored channel over full scale
  (no weight at or below 2% and at or above 98%, full weight on 10%…90%) and `e` its stops from the
  reference; the result is on the reference frame's scale. Where no frame has weight the shortest exposure
  stands alone if all are clipped, else the longest unclipped one; both are counted in the reply.
- The reference is the middle exposure (`MergeHDR.referenceFrame`). EVs come from the caller, else EXIF when
  every file has exposure time, f-number and ISO, else the core's estimate (medians of log ratios over a
  lattice of at most about 262,144 samples). Frames that share fewer than 64 well-exposed samples cannot be
  estimated: the card then asks for the values and MCP refuses with the reason unless `evs` is passed. Two
  frames within 0.1 EV are refused by name unless the values were entered.
- Align (default on) is `AutoAlignWork.alignedDocument`, the Auto-Align worker loop, inside the merge's one
  cancellable job; an `unregistered` frame is left out with its reason and an `unchanged` one is used. On
  the drive's ±2 EV textured bracket (`scripts/verify_v1_merge_hdr.py`, case `alignment`) the outer frames
  registered across the exposure difference and the mean |ln(merged / R)| fell from 0.206 to 0.002.
- Remove Ghosts (default off): a frame is judged against the reference only where both are fully weighted
  and opaque. A pixel whose log-radiance residual is farther than max(4 sigma, ln 1.1) from that frame's
  median residual takes the reference alone; sigma is the frame's robust noise on the lattice. Centring on
  the median makes a constant EV error harmless. Ghosts where the reference or the moving frame is outside
  its fully weighted band are not removed: the noise estimate does not hold there.
- A camera RAW frame is the 16-bit development with the tone curve and shadow boost off, clipped at the
  decoder's white; there is no float RAW development. A 32-bit float first file or source document is
  refused.
- Memory: the stack's plan is admitted before any decode (`StackLoad.plan`); the merge's plan is admitted
  before its job with the stack resident: the core's output and scratch, the result's open (60 bytes a
  canvas pixel), and with Align the registration plan and the non-reference layers' bytes. The reply's
  `working_memory_plan_bytes` is their sum, so it can exceed the working limit on a machine where each
  stage fits. The job drops the unaligned stack once the aligned one exists.

### Auto-Blend Layers
- Edit ▸ Auto-Blend Layers… (Layer ▸ until phase 2a) and MCP `auto_blend_layers` (`AutoBlend.swift`,
  `AgentServer+AutoBlend.swift`; core `focus_stack.rs`, `doc_auto_blend.rs`, `precision_auto_blend.rs`). The
  Stack Images method (focus stacking, `method: "stack_images"`, the MCP default): each selected raster
  layer takes a binary, canvas-sized layer mask revealing where it is the sharpest frame, and Seamless
  Tones and Colours adds one merged layer, a Laplacian-pyramid blend
  through those masks. One undo step, one cancellable job, at 8, 16 and 32 bits.
- Core code: four exports (`rz_doc_auto_blend*`, `rz_precision_doc_auto_blend*`). No `.rz` change: RZDC
  stays 75 and word 79 stays unused.
- Decisions: the masks are binary so they partition the covered area, and the stack itself shows one frame
  per pixel. A frame never wins where its alpha is 0, and the score is alpha-weighted, so a hole or a layer
  edge is not detail. The merged layer lands above the top selected layer's clip run and is unclipped
  (`insert_slot_above_clip_run`, which nothing else uses). The pixels no frame covers are one more member of
  the blend, so the result does not darken beside them. A float document scores and blends in
  `retouch_encoding` and keeps values above white. A layer that already has a mask is refused, not
  intersected.
- "Align first" (`align: true`) runs `AutoAlignWork.alignedDocument`, the Auto-Align worker loop, inside
  the same job. Any `unregistered` layer refuses the whole blend by name and nothing is published.
- The plan is `focus_stack`'s two per-pixel constants plus the new masks, each charged at 5/4
  (`doc_auto_blend::mask_charge`, a controller ruling on the merge and stacks review), and the merged layer. Each mask
  carries its own quarter and each constant is above 1.25 × the rest of its peak, so the plan holds 1.25 ×
  at any layer count. `scripts/auto-blend-admission/measure.py` (Release, one case per process) measured
  eight 24 MP layers at 8 and 16 bits and four 12 MP float layers, Seamless on and off, and 32 layers of
  3 MP: Seamless on at 8, 16 and float (1.321, 1.336 and 1.409 ×) and Seamless off at 16 bits (1.302 ×).
  The record sits in the comments on the two constants in `focus_stack.rs`.
- Verification: `make t F=layer_auto_blend`, `python3 scripts/verify_v1_auto_blend.py` (the MCP drive) and
  the admission drive.
- **Panorama** (phase 2a, `method: "panorama"`, the sheet's default; kernels `graph_cut.rs`,
  `panorama_seam.rs`, `panorama_gain.rs`; write-back `doc_panorama_blend.rs`, `precision_panorama_blend.rs`).
  Each frame takes a binary mask revealing its side of a seam; with Seamless Tones and Colors each frame's RGB
  is scaled by its gain, the seams are cut on the gained frames, and the frames are multi-band blended across
  the seams and written back into themselves. No layer is added. Align first grows the canvas to the union
  (`AutoAlign.request(growsCanvas: true)`), published with the selection carried as Auto-Align does, and the
  reply reports `aligned.canvas`. One undo step, one job, at 8, 16 and 32 bits; CMYK refuses by name.
- Panorama decisions. SEAMS: one Boykov–Kolmogorov min cut per frame against the composite of the frames
  below (bottom first), over the overlap's box on an analysis grid of at most 4 Mi blocks; links cost the
  colour difference plus the larger luma gradient plus ε = 1/255, so a seam runs where the frames agree and
  neither has an edge; a unary term of λ = 4/255 decides a flat overlap: `SeamPreference::Lower` since
  phase 2b-1 (take2's W), so the frame below keeps it and the seam runs along its edge (it was Centre,
  midway). GAINS: Brown–Lowe's per-channel least squares over the overlaps' means with σ_N = 10/255 and
  σ_g = 1 (0.1 leaves a 0.8 exposure step at ratio 0.851; 1 gives 0.8006). C-PIXELS: Seamless writes the
  blend into the frames (the take's default row; S was not answered, below). MULTI-BAND: `focus_stack::blend`
  is the one pyramid, no Poisson. A float document's Panorama reads retouch codes over the code of white
  (`precision_panorama_blend::float_white_code`), not over 65535, so ε, λ, σ_N and the clip bounds mean
  what they mean at 8 and 16 bits (over 65535 a 0.8 step solved to ratio 0.973). Seamless with a
  pixel-locked frame is refused ("is locked"); masks alone are not.
- Take1 (`panorama_blend_take.py`, `panorama-564/take1`) stopped after job 1 (a `-1712` while hashing
  outputs; the runner now sets `skip_file_hashes`): no layer added, masks binary (M); take2 answered the rest.
- Panorama's plan (`doc_auto_blend::kernel_plan`): the larger of the gains, the seams and
  `focus_stack::winners_plan`'s per-canvas-pixel charge (the masks-only peak outside any kernel), with
  Seamless the blend's too, plus the masks at 5/4 and the frames' written-back rasters. Admission gate
  M-blend PASS (`scripts/auto-blend-admission/measure.py`, the `panorama-` cases, 1.40–1.73 ×); no constant
  changed; the rows sit on `focus_stack.rs`' two constants.
- Verification: `make t F=layer_auto_blend_panorama`, `make t F=graph_cut_min_cut` (a brute-force oracle),
  `python3 scripts/verify_v1_auto_blend_panorama.py`, the admission drive's `panorama-` cases.
- **Content-Aware Fill Transparent Areas** (phase 2b-1, 2026-10-16; `AutoBlendFill.swift`, MCP
  `auto_blend_layers` `fill`, the sheet's checkbox `auto-blend-fill`, Actions). Panorama only (Stack Images
  refuses `AutoBlend.fillNeedsPanorama`). In the same job and undo step, after the blend: the coverage is
  Generative Expand's (`genExpandCoverage`, 65535 − the composite's alpha, now at 32 bits too); the visible
  picture is stamped above the top frame as "<top frame> (merged)" (take2's F-MERGED and N) and filled in place
  by the one Content-Aware Fill (`ContentAwareFillWork`, ring 0, seed 0, Current) with the core's opt-in
  `fill_transparent` pad (`precision_inpaint::opaque_hole`: the hole's alpha is set to full on the destination
  before the write-back, which leaves a transparent destination alone; refused on another op, with
  `new_layer` and in CMYK). Nothing transparent: `fill` = {layer: null, covered_pixels: 0}, no layer. Any
  coverage, admission or fill refusal refuses the whole call with `AutoBlendFill.refusalMessage` ("…Turn it off
  to blend without the fill."); a cancel publishes nothing. Each stage admits its own measured plan (gen
  expand's plate term, `fill_admission`); no new constant, no `.rz` change, no new export.
- Take2 (`scripts/photoshop-probe/panorama_fill_take.py`, `~/devel/rasterize-fixtures/photoshop/panorama-564/
  take2`; selftest-2 PASS, all nine jobs ran): S C-PIXELS (the default stands), G G-SPLIT (both frames' gains
  move; as built), W Lower (shipped above), M binary, F F-MERGED (one layer, opaque over the whole canvas, the
  frames and masks kept). K is unmeasured: Photoshop's composite PNG is flattened on white, so H was empty.
- Verification: `make t F=content_aware_fill_transparent`, `make t F=generative_expand`, `make t
  F=layer_auto_blend_panorama`, `python3 scripts/verify_v1_auto_blend_panorama.py` (the `fill …` cases).

### Merge Panorama

- Phase 3b (2026-10-17): File ▸ Automate ▸ Merge Panorama… (`MergePanorama.swift`,
  `MergePanoramaSheetController.swift`, `AppDelegate+MergePanorama.swift`), MCP `merge_panorama`
  (`AgentServer+MergePanorama.swift`, `documentFree`, recordable) and its Action step. The dialog holds
  the six layouts in order (Auto on), Use Files or Folder, Browse…, Remove, Add Open
  Files, Blend Images Together (on), Vignette Removal, Geometric Distortion Correction, Content-Aware Fill
  Transparent Areas (Fill disabled while Blend is off; the lens boxes disabled and cleared, Fill cleared, on
  Collage and Reposition). The result is a new document, "Untitled_Panorama<n>", whose history is its
  initial state; unregistered frames stay hidden and are named in the notice and `note`. `AutoBlendWork
  .blended` is the blend stage both jobs share; `stackLayerRows` and `autoAlignReplyFields` are the replies
  both tools share. `ActionRecorder.noteCreatedDocument`: a step on a Merge Panorama document records as ONE
  placeholder, "Edits in a document made by Merge Panorama".
- Verification: `python3 scripts/verify_v1_merge_panorama.py` (the reach rows, refusals by stage, pixel identity
  with the three tools run one by one, Actions), the jobs-probe case "merge panorama cancel".

### HDR Toning

- Phases 4a and 4b (2026-10-17): Image ▸ Adjustments ▸ HDR Toning… (`HdrToning.swift`,
  `HdrToningSheetController.swift`, `HdrToningLocalAdaptationPane.swift`, `HdrLocalAdaptation.swift`,
  `EditorViewController+HdrToning.swift`), opening on Local Adaptation (Edge Glow, Tone and Detail, Advanced,
  the Toning Curve over a histogram read once on open, before the first preview), with Equalize Histogram,
  Exposure and Gamma, Highlight Compression and Mode 8/16 for a 32-bit source; Image ▸ Mode ▸ 8/16
  Bits/Channel on a 32-bit RGB document opens it at that depth. MCP `hdr_toning` (recordable, 8/16/32 bits,
  CMYK refused). It asks "Using HDR Toning will flatten the document. Proceed?" when it flattens structure;
  MCP flattens silently. One job (`HdrToningRun`), one undo step "HDR Toning". Core: `hdr_toning.rs`,
  `hdr_local_adaptation.rs`, `doc_hdr_toning.rs`, four `rz_*hdr_toning*` exports (the histogram's included);
  `float_to_u16`'s carry is `flattened_u16_document`. Memory: M-tone (Release, 24 MP, three runs, the six tone
  cases) holds the plan at `HDR_TONING_MARGIN_BYTES_PER_PIXEL` 11; Local Adaptation's three cases at 1.32 ×
  (jpeg8), 1.26 × (png16) and 1.54 × (float) max E (`measure.py --cases tone-la-jpeg8-24mp,tone-la-png16-24mp,
  tone-la-float-24mp`), and `load-2-png16` with the two-job `StackLoad.placing` at 3.07 ×.
- Verification: `make t F=float_hdr_toning`, `make t F=float_hdr_local_adaptation`, `python3
  scripts/verify_v1_hdr_toning.py`, the jobs-probe case "hdr toning cancel", the admission drive's tone cases.
- Phase 4c (2026-10-19): the presets. The dialog's Preset popup (`HdrToningPresetControl.swift`): Default,
  the user's own (Save/Delete, `HdrToningPresetLibrary`, `RZ_HDR_TONING_PRESETS_DIR`, preferences), Custom;
  MCP `hdr_toning` `preset` (`method` then optional; explicit keys override). A recorded call keeps the
  preset's explicit values, never its name (`HdrToningPreset.recordedArguments` in
  `ActionRecorder.recordAgentCall`). Verification: `python3 scripts/verify_v1_hdr_toning.py` its presets
  case. The Photoshop take was not attempted.
  (2026-10-29) The eleven built-ins were removed: HDR Toning's built-in preset is Default; user presets are
  saved from the Preset popup (#7, #8).
