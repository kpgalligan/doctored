# Stack admission drive

`StackLoad.plan` admits `StackLoad.planBytes` (every layer at the stack's
depth, the place charge of that depth per layer pixel — an 8-bit stack's
contiguous views, a 16-bit or float stack's margin on its layers — the largest
single decode charge and the canvas's open) before any file is decoded, and
MCP `load_files_into_stack` reports it as `working_memory_plan_bytes`.
`AutoAlignWork` admits `AutoAlign.registrationPlan` (two analysis frames as
Vision's working copies, plus the thumbnail transient of the largest analysed
layer in a 16-bit or float document) before it registers anything, and MCP
`auto_align_layers` reports it as `registration_plan_bytes`.
`StackModeWork` admits the core's `stack_mode` plan (`stack_stat::plan_scratch`:
the output plane `output_plane_copies` times — 7 at 8 bits, 5 at 16 and 4 at
float — a row per layer read plus one,
the structure term and 1 MiB) before the statistic runs, and MCP `stack_mode`
and `reduce_noise_from_stack` report the largest charge they admitted as
`working_memory_plan_bytes`. Merge to HDR (`MergeHDR.merge`) admits its own
plan before its one job — the core's output and scratch, the result's open at
60 bytes a canvas pixel, and with Align the registration plan and the
non-reference layers' bytes at the stack's depth — and MCP `merge_to_hdr` /
`merge_layers_to_hdr` report that plan plus the stack's as
`working_memory_plan_bytes`. This drive measures all four and pins them, on a
built Release app.

```sh
make app
python3 -B scripts/stack-admission/measure.py \
  --app build/Build/Products/Release/Doctored.app [--output <folder outside the repo>] \
  [--runs 3] [--cases load-8-jpeg8,register-16]
```

Every case runs in its own fresh owned app at `DR_WORKING_LIMIT_BYTES` 8 GiB
with `MallocLargeCache=0`, beside a baseline twin (the same setup, then
`resource_usage`), `--runs` times. The fixtures are `jpeg8` and `png16` at
4032x3024 and at 8064x6048 (`jpeg8-48`, `png16-48`) and `png16` at 2016x1512
(`png16-3`), authored into the run's
private folder by `scripts/host-still-admission/author.swift`, and the
repository's 48x32 float `core/tests/fixtures/float_11f/scene.tiff`. E is
`ri_interval_max_phys_footprint` − pre around the one measured call, read once
the footprint has been still for 2 s, so the new document's window and
projection are inside the interval.

Load cases: `load-8-jpeg8` (jpeg8 x 8), `load-4-png16` (png16 x 4, a 16-bit
stack), `load-png16-3-jpeg8` (png16 then jpeg8 x 3, a 16-bit stack with three
converted files), `load-16-jpeg8` / `load-16-png16` (sixteen layers, so
the plan is held at a second layer count), `load-40-png16-3mp` (png16-3 x 40:
a 16-bit stack past the 34 layers where a plan that charged a 16-bit layer
only its own 8 bytes a pixel fell under 1.25 x), `load-6-float` (the float
scene then jpeg8 x 5, a float stack with five converted layers), `load-2-png16`
/ `load-8-png16` (with `load-16-png16`, the 16-bit per-layer slope at three
layer counts) and `load-2-float` / `load-8-float` / `load-16-float` (the float
scene then jpeg8 x 1, x 7 and x 15: the float slope). A 16-bit layer is charged
3 bytes a pixel beside its own 8 and a float layer 5 beside its own 16
(`StackLoad.typedLayerCharge16` / `typedLayerCharge32`); the summary prints each
depth's slope, (max E at 16 layers - max E at 8) / (8 x 4032 x 3024).

Register cases (a stack of two identical layers is the setup; the measured
call is `auto_align_layers` over them): `register-8` and `register-16` at
12 MP, `register-float` (the float scene then jpeg8 x 2: a float stack whose
layers 1 and 2 are aligned), and `register-8-48mp` / `register-16-48mp` at
48 MP, so the plan is held at a second layer size. `register-4-jpeg8-24mp`
is the one register case that warps: the first four jpeg8 plane
frames (6000x4000) as a stack, all four named, so three layers are warped to
the bottom one and the canvas grows to their union inside the interval; its
plan is the reply's `working_memory_plan_bytes`. The projection cases
reproject: `project-3-jpeg8-24mp` and `project-3-png16-24mp` are
three 6000x4000 YAWED views of the plane scene (`author.swift` with a trailing
`yaw <degrees> 6000`, at -2.5, 0 and +2.5 degrees: take2's geometry x 10 at an
eighth of its yaw step; on this picture `AutoAlign.register` reached a 3-degree
step and not a 4-degree one) as a stack, and
`project-float-24mp` is the float scene then the three jpeg8 views (layers
1-3); the measured call is `auto_align_layers` with
`projection: "cylindrical"`, so every layer, the reference too, is reprojected
by the core and the canvas is set to the projected union inside the interval.
The Vignette Removal cases, `vignette-3-jpeg8-24mp`,
`vignette-3-png16-24mp` and `vignette-float-24mp`, load the projection cases'
stacks; the measured call is `auto_align_layers` with
`projection: "perspective", vignette: true`, so the core's `remove_vignette`
fits one falloff and rewrites every layer, and every layer is then reprojected
onto the sphere; the plan is the reply's `working_memory_plan_bytes`, and the
reply must answer `surface` "spherical", three `vignette.layers` and a nonzero
coefficient (kind `register-vignette`, gate M-register).
The Geometric Distortion cases, `distortion-3-jpeg8-24mp`,
`distortion-3-png16-24mp` and `distortion-float-24mp`, load the same stacks;
the measured call is `auto_align_layers` with
`projection: "perspective", distortion: true`, so the core's `fit_distortion`
matches the registered overlaps and fits one lens coefficient, and every layer
is then reprojected onto the sphere with it; the plan is the reply's
`working_memory_plan_bytes`, and the reply must answer `surface` "spherical",
three `distortion.layers` and `distortion.matches` above 0 (kind
`register-distortion`, gate M-register).
The planar lens cases, `planar-3-jpeg8-24mp`,
`planar-3-png16-24mp` and `planar-float-24mp`, load the first three plane
frames (`plane_frames(...)[:3]`; the float case is the float scene then the
three jpeg8 frames, layers 1-3); the measured call is `auto_align_layers` with
`projection: "reposition", distortion: true`, so the core's `fit_distortion`
fits one lens coefficient and every layer, the reference too, is corrected
within its own frame and placed (`project_layer`'s plane); the plan is the
reply's `working_memory_plan_bytes`, and the reply must answer `changed` true,
no `surface`, three `distortion.layers` and `distortion.matches` above 0 (kind
`register-planar`, gate M-register).
The reach cases hold the feature step's term of the plan
(`AutoAlign.featureScratch`): `reach-unrelated-8-24mp` and
`reach-unrelated-16-24mp` are two 6000x4000 yawed views at f 6000, yaw 0 and
yaw 60 (no shared pixel), as jpeg8 and as png16; the reply must answer layer 1
`unregistered` with `changed` false, so the interval holds Vision's requests and
the feature step and no warp (kind `register-unrelated`, plan the reply's
`registration_plan_bytes`). `reach-2-jpeg8-24mp` is yaw 0 and yaw 20 as jpeg8,
a step only the feature step follows; the reply must align layer 1 with
`method` homography and grow the canvas (kind `register-warp`).

Stack cases (a stack of identical files is the setup; the measured call is
`stack_mode` with `mode: "median"` over its layers): `stack-2-jpeg8`,
`stack-8-jpeg8`, `stack-16-jpeg8`, `stack-2-png16`, `stack-8-png16`,
`stack-16-png16`, and `stack-2-float` / `stack-8-float` / `stack-16-float` (the
float scene then jpeg8 x 2, x 8 and x 16; the call reads the converted layers,
1...N), so each depth's factor is held at three layer counts.
`stack-8-jpeg8-24mp` is a median over eight 6000x4000 jpeg8
plane frames (the five, then the first three again), and `stack-4-cmyk` a
median over jpeg8 x 4 converted with `convert_mode cmyk` before the interval:
the CMYK arm, held to the 8-bit factor at 10 bytes a pixel.
`denoise-16-jpeg8` measures `reduce_noise_from_stack`
(`mode: "mean"`) over jpeg8 x 16. The interval holds the whole call: the
statistic, the display preparation and the published edit.

CMYK cases (each stack converted with `convert_mode cmyk` before the
interval, then its RGB kind's call and checks):
- `register-cmyk`: jpeg8 x 2, `auto_align_layers` answering `unchanged` (kind
  `register-cmyk`, gate M-register).
- `register-4-cmyk-24mp`: the first four jpeg8 plane frames, three warped in
  all four inks and the canvas grown (kind `register-warp-cmyk`, M-register).
- `project-3-cmyk-24mp`: the three jpeg8 yawed views, Cylindrical (kind
  `register-project-cmyk`, M-register).
- `denoise-3-cmyk-24mp`: the first three jpeg8 plane frames,
  `reduce_noise_from_stack` (`mode: "median"`), every floating layer `aligned`,
  none `excluded`, all three read (kind `denoise-cmyk`, M-stack).

Merge cases: `merge-5-jpeg8-24mp` and `merge-5-png16-24mp` (`merge_to_hdr` of
five 6000x4000 frames, so the interval holds the stack's build, four warps and
the merge) and `merge-layers-5-png16-24mp` (the five png16 frames as a stack is
the setup; the measured call is `merge_layers_to_hdr` over its five layers).
Their frames are the PLANE scene — `author.swift` with a trailing `<dx> <dy>`:
one picture of value noise at the shifts (24, -16), (-32, 12), (0, 0),
(16, 28) and (-12, -20) — because the ramp scene has no features to register
and identical frames would answer `unchanged` and warp nothing. Each call
passes `evs: [-4, -2, 0, 2, 4]` and `align: true`: the given EVs make frame 2
the reference and keep the estimate and the identical-exposure refusal out of
it (the frames are one exposure, which the memory does not see).

Tone cases: `tone-jpeg8-24mp` and `tone-png16-24mp` open the
middle plane frame (6000x4000, jpeg8 or png16) as the setup, and
`tone-float-24mp` merges the five jpeg8 plane frames with `merge_to_hdr`
(`align: false`) into a 32-bit float document; the measured call is
`hdr_toning` with `method: "equalize_histogram"`, so the interval holds the
promotion (8 bits), the render, the decode to linear light, the toning, the
narrowing back (8 bits), the display preparation and the published edit.
The Local Adaptation cases, `tone-la-jpeg8-24mp` and
`tone-la-png16-24mp` (kind `tone-la`) and `tone-la-float-24mp` (kind
`tone-la-float`), take the same three setups and call `hdr_toning` with
`method: "local_adaptation"` at its defaults, so the interval adds the range
passes, the (t′, α) plane and the guided pass at radius 16.

A row whose other checks passed, whose runs all reported one plan, and whose
max E is not above its baseline twins' max E measured no rise (the footprint
never went above where the call started): it prints `SKIP (no rise)` in place
of PASS or FAIL and does not count toward its gate. A row that failed another
check or reported differing plans is a FAIL even with no rise. A gate fails when
at least one of its cases was selected and every selected row of it was
skipped, so a run cannot pass on rows that allocate nothing; a gate none of
whose cases was selected keeps its PASS.

- **M-load**: for the load cases, `working_memory_plan_bytes` ≥
  ⌈1.25 × max E⌉.
- **M-register**: for the register cases, `registration_plan_bytes` ≥
  ⌈1.25 × max E⌉, and every reply answers the floating layer `unchanged`
  (identical layers need no warp, so the interval is the registration alone);
  for `register-4-jpeg8-24mp`, `working_memory_plan_bytes` ≥ ⌈1.25 × max E⌉,
  with all three floating layers `aligned`, `changed` true and a `canvas`;
  for the `project-*` cases, `working_memory_plan_bytes` (the registration or
  the largest `project_layer_plan` scratch) ≥ ⌈1.25 × max E⌉, with every
  floating layer `aligned`, `changed` true and `focal` and `canvas` present.
  A project miss raises `doc_align_projection::PROJECTION_MARGIN_BYTES_PER_PIXEL`.
- **M-stack**: for the stack cases and the denoise case,
  `working_memory_plan_bytes` ≥ ⌈1.25 × max E⌉, and the denoise reply answers
  every floating layer `unchanged` with nothing `excluded`. A miss raises that
  depth's factor (`stack_stat::OUTPUT_PLANE_COPIES_8` / `_16` / `_32`; the CMYK
  case `_8`).
- **M-merge**: for the merge cases, every frame `used`, frame 2
  `alignment: "reference"` and the other four `"aligned"`. A reply with an
  `unchanged` or `unregistered` frame fails its case; the answer to that is
  the fixture (cell sizes, shifts), never the gate. The plan:
  `merge_layers_to_hdr`'s `working_memory_plan_bytes` is the merge's plan
  alone and must be ≥ ⌈1.25 × max E⌉. `merge_to_hdr`'s is the stack's plan
  plus the merge's, admitted one after the other, so the sum is only
  recorded: the baseline twin loads the same five files with
  `load_files_into_stack` after its interval is read, that reply's figure is
  the stack's plan, the rest of the sum is the merge's, and each of the two
  must be ≥ ⌈1.25 × max E⌉.
- **M-tone**: for the tone cases, `working_memory_plan_bytes` (the core's
  `hdr_toning_plan`) ≥ ⌈1.25 × max E⌉, and every reply answers `ok`, one layer
  and bit depth 8, 16 and 16 (the same for the three `tone-la-*` cases). A
  miss raises
  `hdr_toning::HDR_TONING_MARGIN_BYTES_PER_PIXEL`.
- **R**: at 1 GiB, a jpeg8 x 12 load is refused with "…on its own…" and the
  `list_documents` count is unchanged.

M-merge's record (2026-10-12; a Release app at 8 GiB,
`MallocLargeCache=0`, three runs, every baseline twin E ≤ 65,536; every run
answered frame 2 `reference` and the other four `aligned`):

| case | reply's plan | stack's plan | merge's plan | E per run | stack's, merge's plan / max E |
|---|---|---|---|---|---|
| `merge-5-jpeg8-24mp` | 5,490,176,323 | 2,760,747,679 | 2,729,428,644 | 1,053,377,328 / 1,053,262,664 / 1,053,115,208 | 2.621, 2.591 |
| `merge-5-png16-24mp` | 9,071,664,763 | 5,502,236,119 | 3,569,428,644 | 2,123,990,600 / 2,122,810,976 / 2,123,187,808 | 2.591, 1.681 |
| `merge-layers-5-png16-24mp` | 3,569,428,644 | none (already open) | 3,569,428,644 | 1,527,923,840 / 1,527,956,608 / 1,527,956,608 | 2.336 (merge's) |

The files form's plan is the stack's plan plus the merge's, and the two are
admitted one after the other: the stack's before any file is decoded
(`StackLoad.plan`), the merge's before its job with the stack resident (the
built stack is passed to that admission as the job's own input). So the
16-bit files case reports 9.07 GB at an 8 GiB limit and is admitted; admitted
as one sum it would have been refused, against a peak of 2.12 GB. E of a files
case is the peak of the whole interval, both stages, so the gate holds each of
the two plans to it rather than their sum. The layers form's plan is the
merge's alone (3.57 GB here), and its interval starts with the five-layer
stack already open.

It imports `owned`, `call` and `footprint` from
`scripts/host-still-admission/verify.py`. Memory, not wall-clock: it may run
beside another lane's build. Run it outside the sandbox: it launches owned app
instances (`HostApp`: `DR_BACKGROUND=1`, a dummy `ANTHROPIC_API_KEY`, a free
port, private `DR_*_DIR` folders) and stops only those. It writes
`receipt.json` to the output folder (a fresh temporary folder when `--output`
is omitted) and exits 0 only when all six gates pass.
