#!/usr/bin/env python3
"""The stack admission drive, on a built Release app.

`StackLoad.plan` admits `StackLoad.planBytes` (every layer at the stack's
depth, the place charge of that depth per layer pixel — an 8-bit stack's
contiguous views, a 16-bit or float stack's margin on its layers — the largest
single decode charge and the canvas's open) before any file is decoded, and
`load_files_into_stack` reports it as `working_memory_plan_bytes`.
A 16-bit layer is charged 3 bytes a pixel beside its own 8, and a float layer
5 beside its own 16 (`StackLoad.typedLayerCharge16` / `typedLayerCharge32`).
`AutoAlignWork` admits `AutoAlign.registrationPlan` (two analysis frames as
Vision's working copies, plus the thumbnail transient of the largest analysed layer
in a 16-bit or float document) before it registers anything, and `auto_align_layers`
reports it as `registration_plan_bytes`. `StackModeWork` admits the core's
`stack_mode` plan (`stack_stat::plan_scratch`: the output plane
`output_plane_copies` times (a factor per depth: 7, 5 and 4 at 8, 16 and 32
bits), a row per layer read plus one, the structure term
and 1 MiB) before the statistic runs, and `stack_mode` and
`reduce_noise_from_stack` report the largest charge they admitted as
`working_memory_plan_bytes`. This drive measures what each really costs and pins
the plan against it. The gates are FIXED here before the drive runs; a miss is
recorded in `receipt.json` and fails its gate, and nothing is widened after the
fact.

Every case runs in its own fresh owned `HostApp` at `DR_WORKING_LIMIT_BYTES`
8 GiB with `MallocLargeCache=0`, beside a baseline twin (a fresh app with the
same setup, then `resource_usage` instead of the measured call), `--runs`
times. Fixtures are `jpeg8` and `png16` at 4032x3024 and at 8064x6048, and
`png16` at 2016x1512 (`png16-3`), authored into this run's private folder by
`scripts/host-still-admission/author.swift`. E is
`ri_interval_max_phys_footprint` - pre around the one measured call, read once
the footprint has been still for 2 s, so the new document's window and
projection are inside the interval.

Cases:
- `load-8-jpeg8`: `load_files_into_stack` of jpeg8 x 8;
- `load-4-png16`: png16 x 4 (a 16-bit stack);
- `load-2-png16` and `load-8-png16`: two and eight 16-bit layers, which with
  `load-16-png16` give the 16-bit per-layer slope at three layer counts;
- `load-png16-3-jpeg8`: png16 then jpeg8 x 3 (a 16-bit stack, three converted);
- `load-16-jpeg8` and `load-16-png16`: sixteen layers at 8 and at 16 bits, so
  the plan is held at a second layer count and its per-layer slope is seen;
- `load-40-png16-3mp`: png16-3 x 40, a 16-bit stack past the 34 layers where a
  plan that charged a 16-bit layer only its own 8 bytes a pixel fell under
  1.25 x (2016x1512 layers, so the forty fit the 8 GiB limit);
- `load-6-float`: the float scene then jpeg8 x 5, a float stack on the
  4032x3024 canvas with five converted layers;
- `load-2-float`, `load-8-float` and `load-16-float`: the float scene then
  jpeg8 x 1, x 7 and x 15, the float per-layer slope at three layer counts;
- `register-8`: a stack of jpeg8 x 2 is the setup; the measured call is
  `auto_align_layers` over its two identical layers;
- `register-16`: the same over png16 x 2;
- `register-float`: the stack is `core/tests/fixtures/float_11f/scene.tiff`
  (48x32, 32-bit float) then jpeg8 x 2, a float stack on the 4032x3024 canvas;
  the measured call aligns its two identical converted layers, 1 and 2;
- `register-8-48mp` and `register-16-48mp`: the 8- and 16-bit cases over
  8064x6048 layers (`jpeg8-48`, `png16-48`), so the plan is held at a second
  layer size;
- `stack-2-jpeg8`, `stack-8-jpeg8`, `stack-16-jpeg8`, `stack-2-png16`,
  `stack-8-png16`, `stack-16-png16`: a stack of that many identical files is
  the setup; the measured call is `stack_mode` (median) over all of its
  layers;
- `stack-2-float`, `stack-8-float` and `stack-16-float`: the float scene then
  jpeg8 x 2, x 8 and x 16; the measured call is `stack_mode` (median) over the
  converted layers, 1...N;
- `denoise-16-jpeg8`: a stack of jpeg8 x 16 is the setup; the measured call is
  `reduce_noise_from_stack` (mean) over all sixteen;
- `merge-5-jpeg8-24mp` and `merge-5-png16-24mp`: `merge_to_hdr` of five
  6000x4000 frames of the PLANE scene (`author.swift` with `<dx> <dy>`: one
  picture of value noise at the shifts (24, -16), (-32, 12), (0, 0), (16, 28)
  and (-12, -20), so every frame registers to the middle one), with
  `evs: [-4, -2, 0, 2, 4]` and `align: true`. The interval holds the stack's
  build, four warps and the merge. The EVs are given so that frame 2 is the
  reference and neither the estimate nor the identical-exposure refusal is
  involved; the frames are all one exposure, which the memory does not see.
- `merge-layers-5-png16-24mp`: the five png16 frames as a stack
  (`load_files_into_stack`) is the setup; the measured call is
  `merge_layers_to_hdr` over its five layers with the same `evs` and
  `align: true`.
- `register-4-jpeg8-24mp`: the first four jpeg8 plane frames
  (6000x4000, shifts (24, -16), (-32, 12), (0, 0), (16, 28)) as a stack is the
  setup; the measured call is `auto_align_layers` over all four, so three
  layers are warped to the bottom one and the canvas grows to their union
  inside the interval.
- `stack-8-jpeg8-24mp`: eight 6000x4000 jpeg8 plane frames (the
  five, then the first three again) as a stack; the measured call is
  `stack_mode` (median) over all eight.
- `stack-4-cmyk`: jpeg8 x 4 as a stack, converted with
  `convert_mode cmyk` (the working CMYK, 8 bits) before the interval; the
  measured call is `stack_mode` (median) over the four CMYK layers, the CMYK
  arm (`cmyk_stack_mode`, 10 bytes a pixel at the 8-bit factor).
- `register-cmyk`, `register-4-cmyk-24mp`, `project-3-cmyk-24mp` and
  `denoise-3-cmyk-24mp`: the `register-8`, `register-4-jpeg8-24mp` and
  `project-3-jpeg8-24mp` stacks and the first three jpeg8 plane frames,
  each converted with `convert_mode cmyk` (the working CMYK, 8 bits) before
  the interval, as `stack-4-cmyk` is. A `-cmyk` kind runs its RGB kind's
  call, record and checks (`register-cmyk` answers `unchanged`; the warp case
  aligns all three floating layers and grows the canvas; the projection case
  reprojects every layer), so the interval holds the registration on the
  layers' views, the warps of all four inks and the canvas step. The
  denoise case's measured call is `reduce_noise_from_stack` (median); its
  frames are shifted, so the reply must align every floating layer, exclude
  none and read all three.
- `project-3-jpeg8-24mp`, `project-3-png16-24mp` and `project-float-24mp`:
  three 6000x4000 YAWED views of the plane scene
  (`author.swift` with `yaw <degrees> 6000`, at -2.5, 0 and +2.5 degrees:
  take2's geometry x 10 at an eighth of its yaw step; on this finer picture
  `AutoAlign.register` reached a 3-degree step and not a 4-degree one) as
  a stack, as jpeg8 and as png16; the float case is the float scene then the
  three jpeg8 views, aligning layers 1-3. The measured call is
  `auto_align_layers` with `projection: "cylindrical"`, so every layer, the
  reference too, is reprojected by the core (`project_layer`) and the canvas is
  set to the projected union inside the interval.
- `vignette-3-jpeg8-24mp`, `vignette-3-png16-24mp` and `vignette-float-24mp`:
  the `project-*` cases' stacks; the measured call is
  `auto_align_layers` with `projection: "perspective", vignette: true`, so
  the core's `remove_vignette` fits and rewrites every layer, then every
  layer is reprojected onto the sphere and the canvas set to their union,
  inside the interval.
- `distortion-3-jpeg8-24mp`, `distortion-3-png16-24mp` and
  `distortion-float-24mp`: the `project-*` cases' stacks;
  the measured call is `auto_align_layers` with `projection: "perspective",
  distortion: true`, so the core's `fit_distortion` matches the registered
  overlaps (two passes) and fits the lens, then every layer is reprojected
  onto the sphere with the coefficient and the canvas set to their union,
  inside the interval.
- `planar-3-jpeg8-24mp`, `planar-3-png16-24mp` and `planar-float-24mp`:
  the first three plane frames (`plane_frames(...)[:3]`, shifts
  (24, -16), (-32, 12) and (0, 0)) as a stack, as jpeg8 and as png16; the
  float case is the float scene then the three jpeg8 frames, aligning layers
  1-3. The measured call is `auto_align_layers` with `projection:
  "reposition", distortion: true`, so the core's `fit_distortion` fits the
  lens over the registered overlaps, then every layer, the reference too, is
  corrected within its own frame and placed (`project_layer`'s plane) and the
  canvas set to their union, inside the interval.
- `reach-unrelated-8-24mp` and `reach-unrelated-16-24mp`:
  two 6000x4000 yawed views at f 6000, yaw 0 and yaw 60 (they share no
  pixels: the view spans 53.13 degrees), as jpeg8 and as png16. The measured
  call is `auto_align_layers` over both; the reply must answer layer 1
  `unregistered` with `changed` false, so the interval holds Vision's
  requests and the feature step (with its lens test) and no warp.
- `tone-jpeg8-24mp` and `tone-png16-24mp`: the middle plane
  frame (6000x4000, jpeg8 or png16) opened with `open_document` is the setup;
  the measured call is `hdr_toning` (equalize_histogram), so the interval holds
  the flattened promotion (8 bits), the render, the decode to linear light,
  the toning, the narrowing back (8 bits), the display preparation and the
  published edit.
- `tone-float-24mp`: `merge_to_hdr` of the five jpeg8 plane
  frames (`evs: [-4, -2, 0, 2, 4]`, `align: false`) is the setup, a 6000x4000
  32-bit float document; the measured call is `hdr_toning`
  (equalize_histogram) into 16 bits.
- `tone-la-jpeg8-24mp`, `tone-la-png16-24mp` (kind `tone-la`) and
  `tone-la-float-24mp` (kind `tone-la-float`): the tone cases'
  setups, the measured call `hdr_toning` with `local_adaptation` at its
  defaults, so the interval adds the range passes, the (t', alpha) plane and
  the guided pass at radius 16 to the render, the decode and the map.
- `reach-2-jpeg8-24mp`: yaw 0 and yaw 20 at f 6000 as jpeg8,
  a step only the feature step follows. The measured call is
  `auto_align_layers` over both; the reply must align layer 1 with `method`
  homography, `changed` true and the grown `canvas` (the `register-warp`
  checks).

- **M-load** for the twelve load cases, the reply's `working_memory_plan_bytes`
  >= ceil(1.25 x max E) over its runs.
- **M-register** for the register cases, the reply's
  `registration_plan_bytes` >= ceil(1.25 x max E), and every reply answers the
  floating layer `unchanged` with `changed` false (identical layers need no
  warp, so the interval holds the registration alone); the two
  `reach-unrelated-*` cases instead answer it `unregistered`. For
  `register-4-jpeg8-24mp` the plan is the reply's `working_memory_plan_bytes`
  (the largest charge the job admitted: the registration or a warp's geometry
  plan), and the reply must align all three floating layers with `changed`
  true and report the grown `canvas`. For the three `project-*` cases the
  plan is the reply's `working_memory_plan_bytes` (the largest charge: the
  registration or a layer's `project_layer_plan` scratch), and the reply must
  answer `changed` true, every floating layer `aligned`, and `focal` and
  `canvas` present. For the three `vignette-*` cases the plan is the reply's
  `working_memory_plan_bytes` (the largest charge: the registration, the
  `remove_vignette` plan's scratch or a layer's projection charge), and the
  reply must answer `changed` true, `surface` "spherical", three
  `vignette.layers` and a nonzero `vignette.coefficient`. For the three
  `distortion-*` cases the plan is the reply's `working_memory_plan_bytes`
  (the largest charge: the registration, the `fit_distortion` plan's scratch
  or a layer's projection charge), and the reply must answer `changed` true,
  `surface` "spherical", three `distortion.layers` and `distortion.matches`
  above 0. For the three `planar-*` cases the plan is the reply's
  `working_memory_plan_bytes` (the largest charge: the registration, the
  `fit_distortion` plan's scratch, a layer's plane charge or the canvas
  step's), and the reply must answer `changed` true, no `surface`, three
  `distortion.layers` and `distortion.matches` above 0.
- **M-stack** for the eleven stack cases (the CMYK one included) and the denoise cases, the reply's
  `working_memory_plan_bytes` >= ceil(1.25 x max E) over its runs; the interval
  holds the statistic, the display preparation and the published edit. The
  `denoise-16-jpeg8` reply must answer every floating layer `unchanged` with
  nothing `excluded` (identical layers need no warp); the CMYK denoise reply
  answers every floating layer `aligned`.
- **M-merge** for the three merge cases, every frame `used`, frame 2
  `alignment: "reference"` and the other four `"aligned"` (so four layers were
  warped inside the interval); a reply with an `unchanged` or `unregistered`
  frame fails its case. The plan: `merge_layers_to_hdr`'s
  `working_memory_plan_bytes` is the merge's plan alone and must be
  >= ceil(1.25 x max E). `merge_to_hdr`'s is the stack's plan plus the
  merge's, admitted one after the other, so the sum is only recorded: the
  baseline twin loads the same five files with `load_files_into_stack` after
  its interval is read, that reply's `working_memory_plan_bytes` is the stack's
  plan, the rest of the sum is the merge's, and EACH must be
  >= ceil(1.25 x max E).
- **M-tone** for the six tone cases, the reply's `working_memory_plan_bytes`
  (`HdrToningRun`'s plan: the core's `hdr_toning_plan`) >= ceil(1.25 x max E),
  and every reply answers `ok`, one layer and the depth the case expects (8,
  16, 16); a miss raises `hdr_toning::HDR_TONING_MARGIN_BYTES_PER_PIXEL`.
- **No rise.** A row whose max E is not above its baseline twins' max E (the
  call ran, and the footprint never went above where it started) holds no
  floor under its plan: it prints `SKIP (no rise)` in place of PASS / FAIL and
  does not count toward its gate. A gate FAILS when at least one of its cases
  was selected and every selected row of it was skipped; a gate with no
  selected case keeps its PASS.
- **R** in a fresh app at `DR_WORKING_LIMIT_BYTES` 1 GiB, a jpeg8 x 12 load is
  refused with `RasterResources.admit`'s "...on its own..." sentence, and the
  `list_documents` count is unchanged. (Its plan: 12 x 48,771,072 of layers +
  12 x 121,927,680 for their views + 34 x 12,192,768 for the canvas +
  10 x 12,192,768 + 16 MiB of decode = 2,601,644,032 before the 3 x encoded
  term, over 1,073,741,824.)

Usage: python3 -B scripts/stack-admission/measure.py --app <Doctored.app>
[--output <folder outside the repository>] [--runs 3] [--cases a,b]. Exit 0
only when all six gates pass. The summary also prints each depth's per-layer
slope, (max E at 16 layers - max E at 8) / (8 x 4032 x 3024), in bytes a pixel.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
AUTHOR = ROOT / 'scripts/host-still-admission/author.swift'
sys.path.insert(0, str(ROOT / 'scripts'))
sys.path.insert(0, str(ROOT / 'scripts/host-still-admission'))
from verify import call, footprint, owned  # noqa: E402

MINIMUM_MAC_LIMIT = 8589934592
REFUSAL_LIMIT = 1073741824
SIZE = (4032, 3024)
LARGE_SIZE = (8064, 6048)
SMALL_SIZE = (2016, 1512)
FLOAT_SCENE = ROOT / 'core/tests/fixtures/float_11f/scene.tiff'
# The merge cases' frames: 6000x4000, the plane scene at these shifts.
MERGE_SIZE = (6000, 4000)
MERGE_SHIFTS = [(24, -16), (-32, 12), (0, 0), (16, 28), (-12, -20)]
MERGE_EVS = [-4, -2, 0, 2, 4]
MERGE_REFERENCE = 2
MERGE_KINDS = {'jpeg8': 'jpg', 'png16': 'png'}
# The projection cases' frames: 6000x4000 yawed views at f 6000 (take2's
# geometry x 10). Take2's 20-degree step is past Auto-Align's reach; on the
# plane scene the three-frame stack registered at steps of 2, 2.5 and 3
# degrees and not at 4, 5 or 7.5 (a Debug-app probe of the 2b-2 batch).
YAW_DEGREES = [-2.5, 0, 2.5]
YAW_FOCAL = 6000
# The phase 3a reach cases' views: (kind, yaw in degrees) at YAW_FOCAL.
REACH_VIEWS = {'reach-jpeg8-0': ('jpeg8', 0), 'reach-jpeg8-20': ('jpeg8', 20),
               'reach-jpeg8-60': ('jpeg8', 60), 'reach-png16-0': ('png16', 0),
               'reach-png16-60': ('png16', 60)}


def plane_frames(kind):
    return [f'plane-{kind}-{index}' for index in range(len(MERGE_SHIFTS))]


def yaw_frames(kind):
    return [f'yaw-{kind}-{index}' for index in range(len(YAW_DEGREES))]


# name: (kind, the stack's files, the layers the measured call names)
CASES = {
    'load-8-jpeg8': ('load', ['jpeg8'] * 8, None),
    'load-4-png16': ('load', ['png16'] * 4, None),
    'load-png16-3-jpeg8': ('load', ['png16'] + ['jpeg8'] * 3, None),
    'load-16-jpeg8': ('load', ['jpeg8'] * 16, None),
    'load-16-png16': ('load', ['png16'] * 16, None),
    'load-40-png16-3mp': ('load', ['png16-3'] * 40, None),
    'load-6-float': ('load', ['scene'] + ['jpeg8'] * 5, None),
    'load-2-png16': ('load', ['png16'] * 2, None),
    'load-8-png16': ('load', ['png16'] * 8, None),
    'load-2-float': ('load', ['scene'] + ['jpeg8'] * 1, None),
    'load-8-float': ('load', ['scene'] + ['jpeg8'] * 7, None),
    'load-16-float': ('load', ['scene'] + ['jpeg8'] * 15, None),
    'register-8': ('register', ['jpeg8'] * 2, [0, 1]),
    'register-16': ('register', ['png16'] * 2, [0, 1]),
    'register-float': ('register', ['scene', 'jpeg8', 'jpeg8'], [1, 2]),
    'register-8-48mp': ('register', ['jpeg8-48'] * 2, [0, 1]),
    'register-16-48mp': ('register', ['png16-48'] * 2, [0, 1]),
    'stack-2-jpeg8': ('stack', ['jpeg8'] * 2, [0, 1]),
    'stack-8-jpeg8': ('stack', ['jpeg8'] * 8, list(range(8))),
    'stack-16-jpeg8': ('stack', ['jpeg8'] * 16, list(range(16))),
    'stack-2-png16': ('stack', ['png16'] * 2, [0, 1]),
    'stack-8-png16': ('stack', ['png16'] * 8, list(range(8))),
    'stack-16-png16': ('stack', ['png16'] * 16, list(range(16))),
    'stack-2-float': ('stack', ['scene'] + ['jpeg8'] * 2, [1, 2]),
    'stack-8-float': ('stack', ['scene'] + ['jpeg8'] * 8, list(range(1, 9))),
    'stack-16-float': ('stack', ['scene'] + ['jpeg8'] * 16, list(range(1, 17))),
    'denoise-16-jpeg8': ('denoise', ['jpeg8'] * 16, list(range(16))),
    'merge-5-jpeg8-24mp': ('merge', plane_frames('jpeg8'), None),
    'merge-5-png16-24mp': ('merge', plane_frames('png16'), None),
    'merge-layers-5-png16-24mp': ('merge-layers', plane_frames('png16'), [0, 1, 2, 3, 4]),
    # Warps and the union canvas, a 24 MP median, the CMYK arm.
    'register-4-jpeg8-24mp': ('register-warp', plane_frames('jpeg8')[:4], [0, 1, 2, 3]),
    'stack-8-jpeg8-24mp': ('stack', plane_frames('jpeg8') + plane_frames('jpeg8')[:3],
                           list(range(8))),
    'stack-4-cmyk': ('stack-cmyk', ['jpeg8'] * 4, [0, 1, 2, 3]),
    # Auto-Align and Reduce Noise from Stack on CMYK stacks.
    'register-cmyk': ('register-cmyk', ['jpeg8'] * 2, [0, 1]),
    'register-4-cmyk-24mp': ('register-warp-cmyk', plane_frames('jpeg8')[:4], [0, 1, 2, 3]),
    'project-3-cmyk-24mp': ('register-project-cmyk', yaw_frames('jpeg8'), [0, 1, 2]),
    'denoise-3-cmyk-24mp': ('denoise-cmyk', plane_frames('jpeg8')[:3], [0, 1, 2]),
    # Cylindrical reprojection of three yawed views.
    'project-3-jpeg8-24mp': ('register-project', yaw_frames('jpeg8'), [0, 1, 2]),
    'project-3-png16-24mp': ('register-project', yaw_frames('png16'), [0, 1, 2]),
    'project-float-24mp': ('register-project', ['scene'] + yaw_frames('jpeg8'), [1, 2, 3]),
    # Vignette Removal, then the Spherical reprojection.
    'vignette-3-jpeg8-24mp': ('register-vignette', yaw_frames('jpeg8'), [0, 1, 2]),
    'vignette-3-png16-24mp': ('register-vignette', yaw_frames('png16'), [0, 1, 2]),
    'vignette-float-24mp': ('register-vignette', ['scene'] + yaw_frames('jpeg8'), [1, 2, 3]),
    # Geometric Distortion's fit, then the Spherical reprojection.
    'distortion-3-jpeg8-24mp': ('register-distortion', yaw_frames('jpeg8'), [0, 1, 2]),
    'distortion-3-png16-24mp': ('register-distortion', yaw_frames('png16'), [0, 1, 2]),
    'distortion-float-24mp': ('register-distortion', ['scene'] + yaw_frames('jpeg8'),
                              [1, 2, 3]),
    # Collage and Reposition with Geometric Distortion.
    'planar-3-jpeg8-24mp': ('register-planar', plane_frames('jpeg8')[:3], [0, 1, 2]),
    'planar-3-png16-24mp': ('register-planar', plane_frames('png16')[:3], [0, 1, 2]),
    'planar-float-24mp': ('register-planar', ['scene'] + plane_frames('jpeg8')[:3], [1, 2, 3]),
    # The feature step's reach and its refusal of unrelated views.
    'reach-unrelated-8-24mp': ('register-unrelated', ['reach-jpeg8-0', 'reach-jpeg8-60'], [0, 1]),
    'reach-unrelated-16-24mp': ('register-unrelated', ['reach-png16-0', 'reach-png16-60'],
                                [0, 1]),
    'reach-2-jpeg8-24mp': ('register-warp', ['reach-jpeg8-0', 'reach-jpeg8-20'], [0, 1]),
    # HDR Toning of one 24 MP document at each depth.
    'tone-jpeg8-24mp': ('tone', [plane_frames('jpeg8')[MERGE_REFERENCE]], 8),
    'tone-png16-24mp': ('tone', [plane_frames('png16')[MERGE_REFERENCE]], 16),
    'tone-float-24mp': ('tone-float', plane_frames('jpeg8'), 16),
    # Local Adaptation at its defaults, the same three setups.
    'tone-la-jpeg8-24mp': ('tone-la', [plane_frames('jpeg8')[MERGE_REFERENCE]], 8),
    'tone-la-png16-24mp': ('tone-la', [plane_frames('png16')[MERGE_REFERENCE]], 16),
    'tone-la-float-24mp': ('tone-la-float', plane_frames('jpeg8'), 16),
}
# The gate each kind of case feeds.
GATES = {'load': 'load', 'register': 'register', 'register-unrelated': 'register',
         'register-warp': 'register',
         'register-project': 'register', 'register-vignette': 'register',
         'register-distortion': 'register', 'register-planar': 'register', 'stack': 'stack',
         'stack-cmyk': 'stack', 'denoise': 'stack', 'merge': 'merge', 'merge-layers': 'merge',
         'register-cmyk': 'register', 'register-warp-cmyk': 'register',
         'register-project-cmyk': 'register', 'denoise-cmyk': 'stack',
         'tone': 'tone', 'tone-float': 'tone', 'tone-la': 'tone', 'tone-la-float': 'tone'}
# The tone kinds and the method each measures.
TONE_METHODS = {'tone': 'equalize_histogram', 'tone-float': 'equalize_histogram',
                'tone-la': 'local_adaptation', 'tone-la-float': 'local_adaptation'}
# The pairs a per-layer slope is read from: eight more 4032x3024 layers.
SLOPES = {'8-bit': ('load-8-jpeg8', 'load-16-jpeg8'), '16-bit': ('load-8-png16', 'load-16-png16'),
          'float': ('load-8-float', 'load-16-float')}
REFUSAL_FILES = ['jpeg8'] * 12
REFUSAL_SENTENCE = 'on its own'


def digest(path):
    sha = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            sha.update(block)
    return sha.hexdigest()


def author(output, merge, yaw, reach=False):
    """Build the author, then write the fixtures this drive loads; with
    `merge`, the merge cases' plane-scene frames too, with `yaw` the
    projection cases' yawed views, and with `reach` the reach cases'."""
    folder = output / 'fixtures'
    made = {'jpeg8': (folder / 'jpeg8-12.jpg', SIZE, None),
            'png16': (folder / 'png16-12.png', SIZE, None),
            'jpeg8-48': (folder / 'jpeg8-48.jpg', LARGE_SIZE, None),
            'png16-48': (folder / 'png16-48.png', LARGE_SIZE, None),
            'png16-3': (folder / 'png16-3.png', SMALL_SIZE, None)}
    if merge:
        for kind, extension in MERGE_KINDS.items():
            for name, shift in zip(plane_frames(kind), MERGE_SHIFTS):
                made[name] = (folder / f'{name}.{extension}', MERGE_SIZE, (kind, shift))
    if yaw:
        for kind, extension in MERGE_KINDS.items():
            for name, degrees in zip(yaw_frames(kind), YAW_DEGREES):
                made[name] = (folder / f'{name}.{extension}', MERGE_SIZE,
                              (kind, ('yaw', degrees, YAW_FOCAL)))
    if reach:
        for name, (kind, degrees) in REACH_VIEWS.items():
            made[name] = (folder / f'{name}.{MERGE_KINDS[kind]}', MERGE_SIZE,
                          (kind, ('yaw', degrees, YAW_FOCAL)))
    if not all(path.is_file() for path, _, _ in made.values()):
        binary = output / 'author'
        subprocess.run(['xcrun', 'swiftc', '-O', '-target', 'arm64-apple-macos15.0', str(AUTHOR),
                        '-o', str(binary)], check=True, capture_output=True, timeout=600)
        folder.mkdir(parents=True, exist_ok=True)
        for name, (path, size, plane) in made.items():
            if path.is_file():
                continue
            # The ramp scene takes five arguments; the plane scene adds its
            # shift, the yawed scene `yaw <degrees> <focal>`.
            kind, shift = plane if plane else (name.split('-')[0], ())
            subprocess.run([str(binary), kind, str(size[0]), str(size[1]), str(path)]
                           + [str(value) for value in shift],
                           check=True, capture_output=True, timeout=600)
    return dict({name: path for name, (path, _, _) in made.items()}, scene=FLOAT_SCENE)


def settle(pid, timeout=120):
    """Waits until the footprint has been still for 2 s; answers it."""
    last, still_since, deadline = None, time.monotonic(), time.monotonic() + timeout
    while time.monotonic() < deadline and time.monotonic() - still_since < 2:
        time.sleep(0.25)
        now, _ = footprint(pid)
        if now != last:
            last, still_since = now, time.monotonic()
    return last


def paths(fixtures, files):
    return [str(fixtures[name]) for name in files]


def measure_case(output, executable, fixtures, label, case, baseline):
    """One case (or its baseline twin) in a fresh app at 8 GiB."""
    kind, files, layers = CASES[case]
    # A `-cmyk` kind converts the stack, then runs its RGB kind's call and checks.
    base = kind.removesuffix('-cmyk')
    record = dict(case=case, label=label, baseline=baseline, kind=kind)
    with owned(output, label, executable) as app:
        ok, usage = call(app, 'resource_usage')
        record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
        document = None
        if kind in ('tone', 'tone-la'):
            ok, opened = call(app, 'open_document', path=paths(fixtures, files)[0])
            assert ok, opened
            document = opened['document']['id']
        elif kind in ('tone-float', 'tone-la-float'):
            ok, merged = call(app, 'merge_to_hdr', paths=paths(fixtures, files), evs=MERGE_EVS,
                              align=False)
            assert ok and merged['document']['precision'] == 'rgba32_float', merged
            document = merged['document']['id']
        elif kind not in ('load', 'merge'):
            ok, stack = call(app, 'load_files_into_stack', paths=paths(fixtures, files))
            assert ok, stack
            document = stack['document']['id']
            record['depth'] = stack['depth']
            if kind.endswith('-cmyk'):
                ok, converted = call(app, 'convert_mode', document_id=document, mode='cmyk')
                assert ok and converted.get('color_mode') == 'cmyk', converted
                record['depth'] = 'cmyk'
        pid = app.process.pid
        settle(pid)
        pre, _ = footprint(pid, reset=True)
        if baseline:
            ok, reply = call(app, 'resource_usage')
        elif kind == 'load':
            ok, reply = call(app, 'load_files_into_stack', paths=paths(fixtures, files))
        elif base == 'stack':
            ok, reply = call(app, 'stack_mode', document_id=document, layers=layers, mode='median')
        elif base == 'denoise':
            ok, reply = call(app, 'reduce_noise_from_stack', document_id=document, layers=layers,
                             mode='median' if kind == 'denoise-cmyk' else 'mean')
        elif kind == 'merge':
            ok, reply = call(app, 'merge_to_hdr', paths=paths(fixtures, files), evs=MERGE_EVS,
                             align=True)
        elif kind == 'merge-layers':
            ok, reply = call(app, 'merge_layers_to_hdr', document_id=document, layers=layers,
                             evs=MERGE_EVS, align=True)
        elif kind in TONE_METHODS:
            ok, reply = call(app, 'hdr_toning', document_id=document, method=TONE_METHODS[kind])
        elif base == 'register-project':
            ok, reply = call(app, 'auto_align_layers', document_id=document, layers=layers,
                             projection='cylindrical')
        elif kind == 'register-vignette':
            ok, reply = call(app, 'auto_align_layers', document_id=document, layers=layers,
                             projection='perspective', vignette=True)
        elif kind == 'register-distortion':
            ok, reply = call(app, 'auto_align_layers', document_id=document, layers=layers,
                             projection='perspective', distortion=True)
        elif kind == 'register-planar':
            ok, reply = call(app, 'auto_align_layers', document_id=document, layers=layers,
                             projection='reposition', distortion=True)
        else:
            ok, reply = call(app, 'auto_align_layers', document_id=document, layers=layers)
        settle(pid)
        post, peak = footprint(pid)
        record.update(ok=ok, pre=pre, peak=peak, post=post, E=peak - pre)
        if baseline and kind == 'merge':
            # The stack's own plan for the same files, read after the twin's
            # interval in an app with nothing else open: `merge_to_hdr` admits
            # it before any decode and the merge's plan before its job, so
            # M-merge holds each of the two to the peak.
            loaded, stack = call(app, 'load_files_into_stack', paths=paths(fixtures, files))
            if loaded:
                record['stack_plan'] = stack.get('working_memory_plan_bytes')
            else:
                record.update(stack_plan=None, stack_refusal=stack[:800])
        if not baseline:
            if not ok:
                record['refusal'] = reply[:800]
            elif kind in TONE_METHODS:
                summary = reply.get('document') or {}
                record.update(plan=reply.get('working_memory_plan_bytes'),
                              bit_depth=reply.get('bit_depth'), flattened=reply.get('flattened'),
                              layer_count=summary.get('layer_count'))
                # One flattened layer at the depth the case expects (`layers`).
                record['toned'] = (reply.get('ok') is True and summary.get('layer_count') == 1
                                   and reply.get('bit_depth') == layers)
            elif kind == 'load':
                record.update(plan=reply.get('working_memory_plan_bytes'), depth=reply.get('depth'),
                              canvas=reply.get('canvas'), layers=len(reply.get('layers', [])))
            elif base in ('stack', 'denoise'):
                record.update(plan=reply.get('working_memory_plan_bytes'), layer=reply.get('layer'),
                              read=reply.get('layers'), hidden=reply.get('hidden'))
                if base == 'denoise':
                    record.update(
                        registration_plan_bytes=reply.get('registration_plan_bytes'),
                        aligned=[entry.get('layer') for entry in reply.get('aligned', [])],
                        unchanged=[entry.get('layer') for entry in reply.get('unchanged', [])],
                        excluded=reply.get('excluded'))
                    # Identical layers need no warp: the statistic reads all of them.
                    record['unchanged_only'] = (record['unchanged'] == layers[1:]
                                                and not record['aligned']
                                                and not reply.get('excluded')
                                                and reply.get('layers') == layers)
                    # Shifted frames: every floating layer warped, the statistic reads all.
                    record['aligned_all'] = (record['aligned'] == layers[1:]
                                             and not reply.get('excluded')
                                             and reply.get('layers') == layers)
            elif kind in ('merge', 'merge-layers'):
                frames = reply.get('frames', [])
                alignments = [frame.get('alignment') for frame in frames]
                wanted = ['reference' if index == MERGE_REFERENCE else 'aligned'
                          for index in range(len(MERGE_SHIFTS))]
                record.update(plan=reply.get('working_memory_plan_bytes'), depth=reply.get('depth'),
                              canvas=reply.get('canvas'), reference=reply.get('reference'),
                              alignments=alignments,
                              used=[frame.get('used') for frame in frames],
                              reasons=[frame.get('reason') for frame in frames
                                       if frame.get('reason')])
                # Four layers warped inside the interval, every frame merged.
                record['merged_aligned'] = (reply.get('aligned') is True
                                            and alignments == wanted
                                            and all(frame.get('used') is True for frame in frames)
                                            and reply.get('depth') == 'rgba32_float')
            elif base == 'register-warp':
                record.update(plan=reply.get('working_memory_plan_bytes'),
                              registration_plan_bytes=reply.get('registration_plan_bytes'),
                              changed=reply.get('changed'), canvas=reply.get('canvas'),
                              aligned=[entry.get('layer') for entry in reply.get('aligned', [])],
                              unchanged=[entry.get('layer') for entry in reply.get('unchanged', [])],
                              unregistered=reply.get('unregistered'),
                              methods=[entry.get('method') for entry in reply.get('aligned', [])])
                # Every floating layer warped, and the canvas grew to the union;
                # a reach case's layer as a homography (the feature step's).
                record['warped_and_grown'] = (reply.get('changed') is True
                                              and record['aligned'] == layers[1:]
                                              and reply.get('canvas') is not None
                                              and (not case.startswith('reach-')
                                                   or record['methods'] == ['homography']))
            elif kind == 'register-unrelated':
                record.update(plan=reply.get('registration_plan_bytes'),
                              working_memory_plan_bytes=reply.get('working_memory_plan_bytes'),
                              changed=reply.get('changed'),
                              aligned=[entry.get('layer') for entry in reply.get('aligned', [])],
                              unchanged=[entry.get('layer') for entry in reply.get('unchanged', [])],
                              unregistered=[entry.get('layer')
                                            for entry in reply.get('unregistered', [])])
                # Nothing registers: the interval is Vision and the feature step.
                record['refused_only'] = (reply.get('changed') is False
                                          and record['unregistered'] == layers[1:]
                                          and not record['aligned']
                                          and not record['unchanged'])
            elif base == 'register-project':
                record.update(plan=reply.get('working_memory_plan_bytes'),
                              registration_plan_bytes=reply.get('registration_plan_bytes'),
                              changed=reply.get('changed'), canvas=reply.get('canvas'),
                              focal=reply.get('focal'),
                              aligned=[entry.get('layer') for entry in reply.get('aligned', [])],
                              unregistered=reply.get('unregistered'))
                # Every layer reprojected, and the canvas set to their union.
                record['projected'] = (reply.get('changed') is True
                                       and record['aligned'] == layers[1:]
                                       and reply.get('focal') is not None
                                       and reply.get('canvas') is not None)
            elif kind == 'register-vignette':
                fit = reply.get('vignette') or {}
                record.update(plan=reply.get('working_memory_plan_bytes'),
                              registration_plan_bytes=reply.get('registration_plan_bytes'),
                              changed=reply.get('changed'), canvas=reply.get('canvas'),
                              surface=reply.get('surface'), vignette=fit,
                              aligned=[entry.get('layer') for entry in reply.get('aligned', [])],
                              unregistered=reply.get('unregistered'))
                # The vignette fitted over all three layers, then the sphere.
                record['vignetted'] = (reply.get('changed') is True
                                       and reply.get('surface') == 'spherical'
                                       and len(fit.get('layers') or []) == 3
                                       and bool(fit.get('coefficient')))
            elif kind == 'register-distortion':
                fit = reply.get('distortion') or {}
                record.update(plan=reply.get('working_memory_plan_bytes'),
                              registration_plan_bytes=reply.get('registration_plan_bytes'),
                              changed=reply.get('changed'), canvas=reply.get('canvas'),
                              surface=reply.get('surface'), distortion=fit,
                              aligned=[entry.get('layer') for entry in reply.get('aligned', [])],
                              unregistered=reply.get('unregistered'))
                # The lens fitted over all three layers, then the sphere.
                record['distorted'] = (reply.get('changed') is True
                                       and reply.get('surface') == 'spherical'
                                       and len(fit.get('layers') or []) == 3
                                       and (fit.get('matches') or 0) > 0)
            elif kind == 'register-planar':
                fit = reply.get('distortion') or {}
                record.update(plan=reply.get('working_memory_plan_bytes'),
                              registration_plan_bytes=reply.get('registration_plan_bytes'),
                              changed=reply.get('changed'), canvas=reply.get('canvas'),
                              surface=reply.get('surface'), distortion=fit,
                              aligned=[entry.get('layer') for entry in reply.get('aligned', [])],
                              unregistered=reply.get('unregistered'))
                # The lens fitted over all three layers, each corrected in its frame.
                record['planar'] = (reply.get('changed') is True
                                    and 'surface' not in reply
                                    and len(fit.get('layers') or []) == 3
                                    and (fit.get('matches') or 0) > 0)
            else:
                record.update(plan=reply.get('registration_plan_bytes'),
                              working_memory_plan_bytes=reply.get('working_memory_plan_bytes'),
                              changed=reply.get('changed'),
                              aligned=[entry.get('layer') for entry in reply.get('aligned', [])],
                              unchanged=[entry.get('layer') for entry in reply.get('unchanged', [])],
                              unregistered=reply.get('unregistered'))
                # Identical layers need no warp: the interval is the registration alone.
                record['registered_only'] = (reply.get('changed') is False
                                             and record['unchanged'] == layers[1:]
                                             and not record['aligned']
                                             and not reply.get('unregistered'))
    return record


def gate_r(output, executable, fixtures):
    """jpeg8 x 12 at 1 GiB: refused on its own, no new document."""
    previous = os.environ.get('DR_WORKING_LIMIT_BYTES')
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(REFUSAL_LIMIT)
    record = {}
    try:
        with owned(output, 'gate-r', executable) as app:
            ok, usage = call(app, 'resource_usage')
            record['working_limit_bytes'] = usage.get('working_limit_bytes') if ok else usage
            ok, before = call(app, 'list_documents')
            assert ok, before
            ok, reply = call(app, 'load_files_into_stack', paths=paths(fixtures, REFUSAL_FILES))
            ok_after, after = call(app, 'list_documents')
            assert ok_after, after
            record.update(refused=not ok, reply=reply if not ok else None,
                          documents_before=len(before['documents']),
                          documents_after=len(after['documents']))
            record['passed'] = (
                record['working_limit_bytes'] == REFUSAL_LIMIT and not ok
                and REFUSAL_SENTENCE in reply
                and record['documents_before'] == record['documents_after'])
    except Exception as error:                      # Recorded, never repaired.
        record.update(passed=False, error=f'{type(error).__name__}: {error}'[:800])
    finally:
        if previous is None:
            os.environ.pop('DR_WORKING_LIMIT_BYTES', None)
        else:
            os.environ['DR_WORKING_LIMIT_BYTES'] = previous
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--cases', default=','.join(CASES),
                        help='comma list of cases, from ' + ','.join(CASES))
    arguments = parser.parse_args()
    selected = [case.strip() for case in arguments.cases.split(',') if case.strip()]
    unknown = [case for case in selected if case not in CASES]
    if unknown:
        parser.error(f'unknown case(s): {",".join(unknown)}')
    output = (arguments.output or Path(tempfile.mkdtemp(prefix='rz-stack-admission-')))
    output = output.expanduser().resolve()
    if output.is_relative_to(ROOT):
        parser.error('output must be outside the repository')
    output.mkdir(parents=True, exist_ok=True)
    os.environ['DR_BACKGROUND'] = '1'
    os.environ['MallocLargeCache'] = '0'
    os.environ['DR_WORKING_LIMIT_BYTES'] = str(MINIMUM_MAC_LIMIT)
    executable = (arguments.app / 'Contents/MacOS/Doctored'
                  if arguments.app.suffix == '.app' else arguments.app).resolve()
    fixtures = author(output, any(name.startswith('plane-') for case in selected
                                  for name in CASES[case][1]),
                      any(name.startswith('yaw-') for case in selected
                          for name in CASES[case][1]),
                      any(name.startswith('reach-') for case in selected
                          for name in CASES[case][1]))
    report = dict(app=str(executable), app_sha256=digest(executable),
                  script_sha256=digest(Path(__file__)), author_sha256=digest(AUTHOR),
                  runs=arguments.runs, working_limit_bytes=MINIMUM_MAC_LIMIT,
                  fixtures={name: dict(file=str(path), bytes=path.stat().st_size,
                                       sha256=digest(path)) for name, path in fixtures.items()},
                  records=[], gates={})

    for run in range(1, arguments.runs + 1):
        for case in selected:
            for baseline in (True, False):
                label = f'{case}-{"baseline" if baseline else "measured"}-{run}'
                try:
                    record = measure_case(output, executable, fixtures, label, case, baseline)
                except Exception as error:          # Recorded, never repaired.
                    record = dict(case=case, label=label, baseline=baseline, ok=False,
                                  kind=CASES[case][0],
                                  error=f'{type(error).__name__}: {error}'[:800])
                record['run'] = run
                report['records'].append(record)
                print(json.dumps(record), flush=True)

    table = {}
    passed = dict(load=True, register=True, stack=True, merge=True, tone=True)
    # Per gate: the rows selected, and the ones that measured a rise.
    rows = dict(load=0, register=0, stack=0, merge=0, tone=0)
    counted = dict(load=0, register=0, stack=0, merge=0, tone=0)
    for case in selected:
        kind = CASES[case][0]
        base = kind.removesuffix('-cmyk')
        measured = [r for r in report['records'] if r['case'] == case and not r['baseline']]
        baselines = [r for r in report['records'] if r['case'] == case and r['baseline']]
        ok = bool(measured) and all(r.get('ok') for r in measured)
        if base == 'register':
            ok = ok and all(r.get('registered_only') for r in measured)
        if kind == 'register-unrelated':
            ok = ok and all(r.get('refused_only') for r in measured)
        if base == 'register-warp':
            ok = ok and all(r.get('warped_and_grown') for r in measured)
        if base == 'register-project':
            ok = ok and all(r.get('projected') for r in measured)
        if kind == 'register-vignette':
            ok = ok and all(r.get('vignetted') for r in measured)
        if kind == 'register-distortion':
            ok = ok and all(r.get('distorted') for r in measured)
        if kind == 'register-planar':
            ok = ok and all(r.get('planar') for r in measured)
        if kind == 'denoise':
            ok = ok and all(r.get('unchanged_only') for r in measured)
        if kind == 'denoise-cmyk':
            ok = ok and all(r.get('aligned_all') for r in measured)
        if GATES[kind] == 'merge':
            ok = ok and all(r.get('merged_aligned') for r in measured)
        if GATES[kind] == 'tone':
            ok = ok and all(r.get('toned') for r in measured)
        plans = {r.get('plan') for r in measured}
        max_e = max((r.get('E', 0) for r in measured), default=0)
        baseline_e = max((r.get('E') or 0 for r in baselines), default=0)
        plan = next(iter(plans)) if len(plans) == 1 else None
        need = math.ceil(1.25 * max_e)
        # A call that ran and whose footprint never rose above what its
        # baseline twins read holds no floor under its plan: the row is named
        # SKIP and its gate rests on the rows that allocate.
        skipped = ok and plan is not None and max_e <= baseline_e
        row = dict(kind=kind, E=[r.get('E') for r in measured],
                   baseline_E=[r.get('E') for r in baselines], plan=plan, need=need,
                   ratio=(plan / max_e) if plan and max_e else None, skipped=skipped,
                   passed=ok and plan is not None and plan >= need)
        if kind == 'merge':
            # The reply's figure is two plans admitted one after the other,
            # so the sum proves nothing: each must hold the peak on its own.
            stack_plans = {r.get('stack_plan') for r in baselines}
            stack_plan = next(iter(stack_plans)) if len(stack_plans) == 1 else None
            merge_plan = (plan - stack_plan
                          if plan is not None and stack_plan is not None else None)
            row.update(stack_plan=stack_plan, merge_plan=merge_plan,
                       stack_ratio=(stack_plan / max_e) if stack_plan and max_e else None,
                       merge_ratio=(merge_plan / max_e) if merge_plan and max_e else None,
                       passed=(ok and stack_plan is not None and merge_plan is not None
                               and min(stack_plan, merge_plan) >= need))
        rows[GATES[kind]] += 1
        if not skipped:
            counted[GATES[kind]] += 1
            passed[GATES[kind]] &= row['passed']
        table[case] = row
    # A gate whose every selected row was skipped measured nothing: it fails.
    # One with no case selected keeps its result.
    for gate in passed:
        if rows[gate] and not counted[gate]:
            passed[gate] = False
    report['table'] = table
    slopes = {}
    for depth, (fewer, more) in SLOPES.items():
        if fewer in table and more in table and all(table[c]['E'] and None not in table[c]['E']
                                                    for c in (fewer, more)):
            slopes[depth] = (max(table[more]['E']) - max(table[fewer]['E'])) / (8 * SIZE[0] * SIZE[1])
    report['slopes_bytes_per_layer_pixel'] = slopes
    report['gates']['M-load'] = dict(passed=passed['load'])
    report['gates']['M-register'] = dict(passed=passed['register'])
    report['gates']['M-stack'] = dict(passed=passed['stack'])
    report['gates']['M-merge'] = dict(passed=passed['merge'])
    report['gates']['M-tone'] = dict(passed=passed['tone'])
    report['gates']['R'] = gate_r(output, executable, fixtures)
    everything = (passed['load'] and passed['register'] and passed['stack'] and passed['merge']
                  and passed['tone'] and report['gates']['R'].get('passed', False))
    report['passed'] = everything
    (output / 'receipt.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    for case, row in table.items():
        verdict = 'SKIP (no rise)' if row['skipped'] else 'PASS' if row['passed'] else 'FAIL'
        split = (f'stack {row["stack_plan"]} merge {row["merge_plan"]} '
                 if row['kind'] == 'merge' else '')
        print(f'{case:20} plan {row["plan"]} {split}need {row["need"]} ratio '
              f'{row["ratio"] and round(row["ratio"], 3)} E {row["E"]} '
              f'baseline {row["baseline_E"]} {verdict}', flush=True)
    print(f'gate M-load: {"PASS" if passed["load"] else "FAIL"}', flush=True)
    print(f'gate M-register: {"PASS" if passed["register"] else "FAIL"}', flush=True)
    print(f'gate M-stack: {"PASS" if passed["stack"] else "FAIL"}', flush=True)
    print(f'gate M-merge: {"PASS" if passed["merge"] else "FAIL"}', flush=True)
    print(f'gate M-tone: {"PASS" if passed["tone"] else "FAIL"}', flush=True)
    for depth, slope in slopes.items():
        print(f'slope {depth}: {slope:.3f} bytes a layer pixel', flush=True)
    print(f'gate R: {"PASS" if report["gates"]["R"].get("passed") else "FAIL"} '
          f'{json.dumps(report["gates"]["R"])[:600]}', flush=True)
    print(output / 'receipt.json', flush=True)
    sys.exit(0 if everything else 1)


if __name__ == '__main__':
    main()
