# RAW decode footprint (phases 1 and 2)

`RawSourceWork.scratch` is the measured RAW decode charge,
`scratchFixed + scratchPerPixel·px + scratchPerEncodedByte·encoded`. Its
measurement record (every case, the fit, the margins, the residency before
and after) sits in the `///` block beside the constants in
`app/Sources/RawSourceWork.swift`. Phase 2 measured the menu path:
`RawLocalWork.restoreScratch` (Layer ▸ Redevelop RAW…'s restore admit) and
`RawLocalWork.measuredScratch` (local development: the Develop preview,
`edit_raw_development`, enhanced `redevelop_raw`), whose record sits beside
their constants in `app/Sources/RawLocalWork.swift`. This folder is the
harness that produced both.

```sh
(cd core && cargo build --release)
R=~/devel/doctored-fixtures/raw-pixls
python3 -I -B scripts/raw-decode-probe/measure.py --output <folder outside the repo> \
  --camera ~/devel/doctored-fixtures/camera/camera.NEF \
  --real Z6=$R/nikon-z6/Z6.NEF --real A73=$R/sony-a7m3/A73.ARW --real K1D=$R/pentax-k1m2/K1D.DNG \
  --real A7R3=$R/sony-a7rm3/A7R3.ARW --real R5=$R/canon-r5/R5.CR3 --real D850=$R/nikon-d850/D850.NEF \
  --real A7R4=$R/sony-a7rm4/A7R4.ARW
```

`measure.py` builds `main.swift` with every app source but `main.swift`
(`swiftc -O -whole-module-optimization`, the release core, the app's
`Info.plist` under a private bundle id) through `scripts/probe_build.py`, which
caches the linked probe under `build/probe-cache`, so a build on unchanged
inputs compiles nothing; it writes the synthetic DNGs with
`scripts/phase3_fixtures.synthetic_dng` into `<output>/fx`, records each
fixture's size and SHA-256, and runs every case. The camera NEF
(`scripts/raw-probe/camera.py` verifies it) is a 6 MP Nikon D70s file.

`--real LABEL=PATH` (repeatable; LABEL matches `[A-Z0-9]+` and is neither a
synthetic name nor `NEF`) adds a real camera RAW. The seven real-camera files
are CC0 samples from raw.pixls.us, each in its own folder under
`~/devel/doctored-fixtures/raw-pixls/`, whose README records each file's
source URL, listing id, byte size and SHA-256 (never committed; nothing
there is executed, and Python that reads them runs with `-I`): Z6 (Nikon Z 6
lossless NEF, 24.5 MP), A73 (Sony ILCE-7M3 ARW, 24.3 MP), K1D (Pentax K-1
Mark II compressed DNG, 36.6 MP), A7R3 (Sony ILCE-7RM3 ARW, 42.6 MP), R5
(Canon EOS R5 CR3, 44.8 MP), D850 (Nikon D850 lossless NEF, 45.75 MP) and
A7R4 (Sony ILCE-7RM4 ARW, 61.2 MP). `--only <labels>` runs a subset, `--runs`
changes the run count and `--skip-build` reuses the binary. It writes
`measure.json` and exits 1 when a STOP fired. Nothing it writes is committed.

## Method

The method of `HostStillDecoder.swift`'s record. One case per process
with `MallocLargeCache=0`, three runs plus a baseline twin (the same setup and
no measured call), `DR_BACKGROUND=1`, `DR_AUTOMATION_ALERTS=record`, a dummy
`ANTHROPIC_API_KEY`, private `DR_*_DIR` folders and
`DR_WORKING_LIMIT_BYTES=34359738368` (32 GiB), so no admission refuses inside
the harness. The harness touches `RasterResources.shared` right after
`NSApplication.shared`, so the core takes that limit.

Each setup step drains its own autorelease pool (the app's main run loop does
that after every event; a harness top level never does), and the harness then
waits 1 s (`settle()`): Core Image and Metal return a finished render's memory
asynchronously, within about 250 ms, and a setup step's release must not land
inside the measured window. Then `proc_reset_footprint_interval`, pre =
`ri_phys_footprint`, the call, and E = `ri_interval_max_phys_footprint` − pre
(`RUSAGE_INFO_V4`), read while the call's results are alive. Each process
also runs under `/usr/bin/time -l`, whose peak memory footprint − pre is
printed beside E. `calibrate` (once per session) waits 0.5 s after launch,
touches and frees 1 GiB, which must return within 65,536 B, and reads E for a
touched 256 MiB, which must lie within 256 MiB + 1 MiB.

Every mode sets `RawImportRequest.mode = .headless(RawDevelopSettings())` and
`PrecisionImportRequest.mode` `.preserve` (16) or `.rgba8` (8).

- `open <raw> <16|8>` (O): setup `RawImage.inspect(url:)`; measured
  `ImageDocument.openRetainedRaw` (what `openDocument(at:)` runs for a RAW).
- `warm <raw> <16|8>` (W): setup opens the RAW the same way; measured
  `RawSourceWork.prepare`'s worker body, `RawSourceSnapshot.restore` then
  `RetainedRawDecoder.develop(…, maximumScratch: .max)` with exposure 0.5;
  both results stay alive until E is read.
- `cold <rz> <16|8>` (K): the same measured call, after opening a `.rz`
  written by `save`, so no CIRAWFilter has run in the process.
- `preview <raw> <16|8>` (V): setup `inspect`, `RawSourceSnapshot.capture`
  and `RetainedRawDecoder.capture`; measured, the
  `RawDevelopPreviewSession` worker body verbatim: develop the captured
  descriptor, then `RawDevelopInspectionSource`; develop exposure 0.5, then
  `RawDevelopInspectionSource`; then
  `RawDevelopPreviewFrame(viewport: RawDevelopViewport())`. Everything stays
  alive until E is read.
- `resident <raw> <16|8>` (RES, no twin): P0 = `ri_phys_footprint` after app
  setup and before any RAW call; then `openRetainedRaw`, then
  `RawSourceWork.prepare` (exposure 0.5), each in its own pool, with both
  results alive. It prints residual = footprint − P0 −
  `unique_retained_bytes` right after each step (`…_immediate`) and after
  `settle()`; the residency gate is the settled reading, ≤ 128 MiB.
- `prep <raw> <16|8>` (P, information only): `RawSourceWork.prepare` whole,
  then the stage 2 figures: `retainedSourcePlan`'s `scratch_bytes` for a
  decoded asset and `unique_retained_bytes` with that asset alive.
- `save <raw> <16|8> <out.rz>`: opens the RAW and writes it with
  `RasterDocument.saveNative(to:)`, the writer `save_copy` uses. Not measured.
- `baseline <mode> <file> <16|8>`: the twin.
- `open <raw> <16|8> OP` and `preview <raw> 16 VP`: the same measured
  calls with Develop Presence (texture, clarity and dehaze 50, so every guided
  pass runs): OP opens with them, VP's second develop (exposure 0.5) carries
  them, as the import preview does once a slider moved.
- `open <raw> <16|8> OL` and `preview <raw> 16 VL` (v1-lensfun): the same
  measured calls with Profile Corrections on, so a file whose decoder carries
  no lens profile and whose camera and lens Lensfun matches develops with the
  Lensfun stage (`lens_correction` with the matched profile, before the source
  geometry); VL's before and after both carry it. Each run reports `lensfun`.

Cases: O16/O8 at D0, D3, D12, D24, D26, D45 and the NEF; W16/W8 and K16/K8 at
D0, D3, D12, D24, D26 and the NEF, plus K16-D45; V16 at D3, D12, D24 and the
NEF; P16/P8 at D24 (information); RES16 at D24 and the NEF, RES8 at D24. Each
`--real` label adds O16, O8, W16, W8, K16, K8 and V16, all fitted, and
RES16-D850 (under the residency gate, not fitted) when D850 is given. The NEF
and each `--real` label also add the check-only Presence variants OP16, OP8 and
VP16 (not fitted, run them with `--only`, e.g. `--only OP16-NEF OP8-NEF
VP16-NEF OP16-D850 OP8-D850 VP16-D850`): each STOPs unless the committed
decode charge is ≥ ⌈1.25 × max E⌉, and the run prints a PRESENCE table.
They add the check-only Lensfun variants OL16, OL8 and VL16 (v1-lensfun) the
same way (e.g. `--only OL16-NEF OL8-NEF VL16-NEF OL16-D850 OL8-D850
VL16-D850`), under the same check plus a STOP for a run whose development did
not run the Lensfun stage; the run prints a LENSFUN table.

STOPs (exit 1): the calibration misses; a twin reads E > 16 MiB; a fitted case
is refused; the residency gate misses; the committed constants charge a fitted
case below its max E; an `[AutomationAlerts]` line.

## Fit

Deterministic, nothing tuned. Over the fitted cases (O, W, K, V):
`need = ⌈1.25 × max E⌉` and c = 3 per encoded byte (`encoded` is the RAW's
file size). `p_ref` is the largest px among the fitted cases, and `a_floor`
= ⌈max over the fitted cases at `p_ref` of (need − 3·encoded) / p_ref⌉, the
largest measured file's whole cost per pixel. `a_hi` is the previous fit's slope, the
largest ⌈(need − 3·encoded) / px⌉ over cases with px ≥ 2,000,000. For each
integer a in [a_floor, max(a_floor, a_hi)], F(a) is the smallest multiple of
1 MiB ≥ max(0, max over every case of need − a·px − 3·encoded); the fit takes
the a that minimises F(a) + a·p_ref (the larger a on a tie). So every case is
charged at least its need, and above `p_ref` the charge grows at no less
than the largest file's whole cost per pixel, assuming no fixed excess beyond
the measured sizes. The output lists the binding cases of F, each case's
need, charge, margin, charge ÷ max E and the previous charge (173 B/px +
9 MiB + 3·encoded), the ceilings (open: the largest px an empty process
admits at 8 GiB with the captured snapshot, encoded + 4096 B, counted, for an
uncompressed DNG at 2 B/px and each camera file's bytes per pixel; redevelop
adds O-D24's retained bytes per pixel), and, once the constants are in
`RawSourceWork.swift`, the committed charge ÷ max E per case.

```sh
python3 -I -B scripts/raw-decode-probe/measure.py --fit-from <run1>/measure.json [<run2>/measure.json …]
python3 -I -B scripts/raw-decode-probe/measure.py --admit 8589934592 --output <folder> --skip-build \
  --camera ~/devel/doctored-fixtures/camera/camera.NEF --real … (as above)
```

`--fit-from` starts no process: it takes each fitted case's max E over every
given `measure.json`, runs the fit, prints the table, the committed coverage
and the 8 GiB open ceilings for each camera file's bytes per pixel, and exits
1 when the committed constants differ from the fit or charge a case below its
need. `--admit LIMIT` runs `open` and `prep` once each, at 16 and 8 bits, for
the NEF and every `--real` fixture at `DR_WORKING_LIMIT_BYTES=LIMIT`, prints
ok or the sentence, the committed charge, the open's admission figure (charge
+ encoded + 4096, the snapshot `RawSourceSnapshot` keeps admitted across the
open's second admit and `develop`'s) and the px, and writes
`admit-<LIMIT>.json`. Its STOPs: a refusal or setup failure that is not one of
`RasterResources.admit`'s sentences, an `[AutomationAlerts]` line, and an O
case of a real fixture ≤ 46 MP refused although its admission figure is
below LIMIT.

## Predicting an admission

```sh
python3 -B scripts/raw-decode-probe/measure.py --predict <limit> --file <raw> \
  [--local [--locals K] [--effects] | --restore] \
  [--retained R --cache C --transient T] [--charge-only]
```

It mirrors `RasterResources.admit` (no caller of `scratch` passes `own`) with
the constants read from `RawSourceWork.swift`. S is the `scratch` formula on
the file's dimensions (`sips`, or `--width`/`--height`) and size; with
`--local` it is `RawLocalWork.measuredScratch` for K camera locals (0…4,
default 0, no tone term) and `--effects` (grain, vignette or authored lens geometry), with the
constants read from `RawLocalWork.swift`; with `--restore` it is
`RawLocalWork.restoreScratch`. R, C and T are
`unique_retained_bytes`, `cache_bytes` and `transient_estimate_bytes` from the
`resource_usage` taken just before the call. The verdict is the first rule
that matches: `refuse` (with the figure) if S ≥ limit; `admit` if S + T <
limit − R; `refuse` if S + T ≥ limit − (R − C); otherwise `indeterminate`,
because cache eviction decides. `--charge-only` prints S and the previous
charge (`previous_charge`: 173·px + 9 MiB + 3·encoded, plus 56·px +
4 MiB with `--local`) and `above_previous`, and with
`--local` or `--restore` also `old_local_charge`, the unchanged
`RawLocalWork.scratch` (decoder migration's envelope). The Develop preview
adds the session's 64 MiB display term to the local charge; MCP
`edit_raw_development` does not. It does not
model the replace stage's forecast that `RawLocalWork.request` charges when it
is larger (`replaceScratch`: the core's replace plan + 32 MiB +
8·(locals + 2)·px, which binds for a scaled layer or a 32-bit float document).

## Phase 2: the menu path

```sh
python3 -I -B scripts/raw-decode-probe/measure.py --phase2 --output <folder> \
  --camera ~/devel/doctored-fixtures/camera/camera.NEF --real … (the seven files, as above)
python3 -I -B scripts/raw-decode-probe/measure.py --menu 8589934592 --output <folder> --skip-build \
  --camera ~/devel/doctored-fixtures/camera/camera.NEF --real … (as above)
```

Both take the `--real` camera files, so the local charge is fitted
on 24–61 MP camera RAWs as the decode charge is.
`--phase2` runs only the phase-2 cases and fits (writes `measure-phase2.json`);
without it the behaviour above is unchanged. `--recheck-phase2 JSON` starts no
process: it reprints a saved `measure-phase2.json`'s committed-coverage table,
STOP and migration-envelope note, and its ceilings (from the file's
`ceilings.retained_per_px`), under the constants committed now, and exits 1
on a STOP; it is how a decode refit checks the local charge. Same method: one case per
process, three runs plus a baseline twin, `calibrate` once, each setup step in
`step{}`, `settle()` before measuring. Every setup opens with
`openRetainedRaw`, or a cold `.rz` (written by `save`) through
`ImageDocument.openPrecisionDocumentIfRequested`, so no decode or CIRAWFilter
has run before the stage it measures. The shape is the 4th argument:
`probe [baseline] <mode> <file> <16|8> <shape>`.

Shapes (W, H the source size; settings are exposure 0.5 plus the neutral
manual lens `pendingSettings` sends, unless stated):

- L0: no locals. L1: create `l1`, radial centre [W/2, H/2], radius
  [W/3, H/3], hardness 0.4, `exposure_offset` −1.
- L1B: create `l1` with an empty mask, then one brush `mask` command of 2048
  dabs on a 64×32 grid of integer centres, hardness 0.5, opacity 1, at the
  largest radius ≤ 180 whose evaluated pixels (each dab's clipped bounding
  box, `raw_local_coverage.rs`) stay within the core's 256,000,000 cap — the
  worst brush the core admits. The run prints `brush_radius` and
  `brush_evaluated_px`.
- L1C: the source is `RawLocalWork.prepare(exposure 0.5, [L1])`'s result;
  then exposure 0.2 with no operations.
- L4: creates `l1`…`l4` at the four quarter points, radius [W/5, H/5],
  hardness 0.4, offsets −1, +0.5, −0.5, +1.
- FX: L1 plus grain {50, 1.5, 50}, vignette {−1.25, [0.31, 0.67], 1.7} and
  lens {"fx", [0.43, 0.59], [0.012, −0.001, 0.0001], 0.985, 1.015, bilinear}.
- PX: FX plus Develop Presence (texture, clarity and dehaze 50); k 1
  with effects, the same charge as FX. Check-only (not fitted): the run STOPs
  unless the committed `measuredScratch` is ≥ ⌈1.25 × max E⌉ and prints a
  PRESENCE table. Each `--real` label adds PX-16 and PX-8; run them with
  `--phase2 --real D850=… --only PX-16-D850 PX-8-D850`.
- LS, LP: create `l1` with `mask: {op: subject}` (LS) or `{op: people}`
  (LP), `exposure_offset` 1; settings as L1. Vision makes the mask from the
  base development inside the measured request, within the local charge
  (k 1). A request whose status is the shape's own "nothing found" sentence
  (`No subject was found in the image.` / `No people were found in this
  image.`) is recorded as `no_result`, not `refused`, so the run keeps its E.
  Both shapes also print `operations_after`, `session.operations.count` read
  right after the request returns: 0 after a `no_result` (the session dropped
  the generating create on its first run), 1 when a mask was made.
- LT1, LT16, LTS (the local tone path; no camera variant): LT1 creates
  `l1`, radial as L1, with `tone` {exposure 0.5, contrast 40, whites 20,
  blacks −20, saturation 20} and no `exposure_offset`; LT16 creates `l1`…`l16`
  with that tone at the centres of a 4×4 grid, radius [W/6, H/6]; LTS creates
  `l1` as LT1 with `tone` {highlights 50, shadows 50, clarity 50, texture 50},
  which builds the Shadows/Highlights guide and the Presence plane; LTD
  is LTS with `sharpness` 60 and `noise_reduction` 60 added, so the
  detail stage (Reduce Noise, then Smart Sharpen) runs over the Presence plane.
  The run prints the charge's `tone_locals` and `tone_stage` beside `locals`.

Modes:

- `restore <raw|.rz> <16|8> <none|L4>` (RS, RS4, RSK): measured
  `RawLocalWork.restoreForRedevelop`, then `sourceMaster16(0)` and
  `RawLocalDevelopSession(source:layer:)`, all alive at E — what the menu
  holds when the window opens. RS4's source carries the L4 locals.
- `local <raw|.rz> <16|8> <shape>` (L0, L1, L1B, L1C, L4, FX, PX, LS, LP, LT1, LT16, LTS, LTD; L0K/L1K cold):
  setup holds `sourceMaster16(0)` as `runRetained` does, creates a
  `RawLocalDevelopSession` and `append`s the shape's operations (a cold case
  first runs `restoreForRedevelop`, the process's first CIRAWFilter, with no
  render). Measured: the session's `request(…, viewport: RawDevelopViewport())`
  ((nil, false, false) for L0, ("l1", true, true) otherwise), then the main
  run loop until `onStatus` reports ready or anything but the updating line;
  a failure status is the refusal. The run prints the charge
  (`measuredScratch`), the replace stage's forecast (`replaceScratch`;
  `request` charges the larger, which is the printed `budget`), the decode
  charge and the old `RawLocalWork.scratch`.
- `publish <raw> <16|8> <L0|L1>` (PB): setup `RawLocalWork.prepare`;
  measured `PreparedDocumentEdit(result)`; prints
  `projectionScratchEstimate()`.
- `menu <raw> <16|8> <M0|M1|M11|MC>` (no E, `--menu <limit>` runs it at that
  `DR_WORKING_LIMIT_BYTES`, one run, writes `menu-<limit>.json`): an
  `ImageDocument` and `ImageSourceCapture`, then the stages restore →
  master + session → each preview request → hold `session.result`,
  `cancel()`, `RawLocalWork.publish` (`redevelopRaw`'s order). Per stage: ok
  or the sentence, the charge, and `unique_retained_bytes` /
  `transient_estimate_bytes` read just before it. M0 = L0, M1 = L1, M11 = M1
  then exposure 0.2, MC = on L1C's source.

Cases (16 and 8 bits each): RS at D0, D3, D12, D24, D26 and the NEF; RS4 at
D3, D24, D26; RSK at D0, D24, NEF; L0 and L1 at D0, D3, D12, D24, D26, NEF;
L0K and L1K at D0, D24, NEF; L1B and L1C at D24, D26; L4 and FX at D3, D24,
D26, NEF; PB0 and PB1 at D24, D26. Each `--real` label adds RS, L0, L1, L4
and FX, and D850 and A7R4 (the 45 and 61 MP files) also RSK, L0K, L1K, L1B
and L1C, all fitted. The `Z6` label (toys on a table) also adds
LS-16-Z6, LS-8-Z6, LP-16-Z6 and LP-8-Z6 (local mode, fitted, k 1); run
them alone with `--phase2 --real Z6=… --only LS-16-Z6 LS-8-Z6 LP-16-Z6
LP-8-Z6`. LT1, LT16, LTS and LTD run at D24 and, with `--real Z6=…`, at
Z6 (local mode, fitted, k = min(4, creates) as `chargeShape` counts them):
`--phase2 --camera … --real Z6=… --only LT1-16-D24 … LTS-8-Z6`. LTD is
check-only: `fit_tone` never reads it, and `phase2_coverage` STOPs when the
committed charge is below its ⌈1.25 × max E⌉ (`--phase2 --real Z6=… --only
LTD-16-D24 LTD-8-D24 LTD-16-Z6 LTD-8-Z6`).
`--menu 8589934592`: M0, M1, M11, MC at D24, D26 and every `--real` label.

Fit (deterministic; need = ⌈1.25 × max E⌉; rest = need − 3·enc; "≥ 2 MP"
means px ≥ 2,000,000):

- `restorePerPixel` = max(0, max over RS/RS4/RSK ≥ 2 MP of
  ⌈(need − 3·enc)/px⌉); `restoreFixed` = the smallest MiB multiple ≥
  max(0, need − a·px − 3·enc) over every RS case.
- The local charge follows the decode charge's rule. (a, F) = the decode
  fit's own `fit()` over the k = 0 rows (L0, L0K): a ≥ the largest file's
  whole cost per pixel, F the least MiB multiple covering every k = 0 row
  (`localPerPixel` = a). `localPerLocalPixel` b = max(0, max over the k > 0
  rows that are not FX, ≥ 2 MP, of ⌈(rest − F − a·px)/(k·px)⌉);
  `localPerEffectsPixel` c = max(0, max over FX ≥ 2 MP of
  ⌈(rest − F − (a + k·b)·px)/px⌉); `localFixed` = max(F, the least MiB
  multiple covering every row's rest − (a + k·b + e·c)·px). The tone shapes
  stay out of that fit.
- The tone terms, over the committed `localFixed` F, `localPerPixel` a
  and `localPerLocalPixel` b, with kt every local the charge counts:
  `localPerToneLocalPixel` t = max(0, max over LT1 and LT16 of
  ⌈(rest − F − (a + k·b)·px)/(kt·px)⌉); `localToneStagePixel` s = max(0,
  max over LTS of ⌈(rest − F − (a + k·b + kt·t)·px)/px⌉). LT16, LTS and LTD on
  a real file must admit at 8 GiB beside the opened document: `ADMIT 8GiB`
  prints the request's budget + 64 MiB + the setup's retained bytes, a STOP
  when it reaches 8 GiB.
- It prints each case's committed charge (`measuredScratch`, with the decode
  max) ÷ max E, the L4 migration-envelope check (`scratch` + 8 B/px), and the
  ceilings: the largest px whose preview charge + 64 MiB (or restore charge)
  fits beside the opened document, retained bytes per px from M0-D24's
  pre-preview reading (`menu-8589934592.json`, else L0-D24's setup), for
  k = 0, 1, 4 at 16/8 and 8 GiB, a DNG (2 B/px), the NEF and each real
  file's own bytes per pixel. The ceilings mirror `measuredScratch` only: they
  model neither the replace forecast (`replaceScratch`, which needs the
  core's plan) nor the core's own caps.

STOPs (exit 1): calibration misses; a twin reads E > 16 MiB; a fitted case is
refused; a PB case's `projectionScratchEstimate` is below ⌈1.25 × max E⌉
(the structural estimate plus `ProjectionMargin`); the committed charge
covers a fitted case below 1.25×; an `[AutomationAlerts]` line; an LS or LP
run whose `operations_after` is not 0 after a `no_result` or not 1 after a
mask, and an LS run on Z6 that made no mask (it is the case that allocates);
with `--menu`, a refusal that is not one of `RasterResources.admit`'s sentences.
One refusal is recorded instead (`AGGREGATE CEILING`, `aggregate_ceilings` in
the JSON) and leaves the fit: the core keeps a RAW's file and its local masks
(16 + 2 B/px each) within one aggregate at any working limit, `retained RAW and
source resources exceed the 1 GiB aggregate limit` (512 MiB before, when four
full-size masks stopped at about 59 MP, L4 on the 61 MP A7R4).
`RawLocalWork.request` refuses locals past that cap before any work
(`maximumLocals`: 16 up to about 32 MP, 11 on the D850 file, 8 on the A7R4).

## The 32-bit float (linear) development

```sh
python3 -I -B scripts/raw-decode-probe/measure.py --float --output <folder> \
  --camera ~/devel/doctored-fixtures/camera/camera.NEF --real D850=$R/nikon-d850/D850.NEF
```

Depth `32` is the float development: `setDepth(32)` opens headless with
`output_encoding: linear_float` (`PrecisionImportRequest.mode` `.preserve`),
so every later stage runs on the float recipe. `--float` runs, at depth 32 on
the NEF, D24 and every `--real` label, O32, W32, K32 and V32 (the modes
above), L0, L1, L4 and FX through `local` (FX with grain since #4), and
#4's grain and HDR-frame cases:

- OG32: `open` with grain {50, 1.5, 50} (shape `OG`).
- VG32: `preview` whose after develops that grain (shape `VG`). V32 and VG32
  draw the frame with `extendedRange` on, the window on a screen with
  headroom, so the f32 side's extended Fit image is in the interval.
- GR32: mode `grain`, the float open as setup outside the interval, then
  `applyingRawGrain` alone; gated on the core's `selectedEditPlan`
  `scratch_bytes` for the same request. A run whose grain changed nothing is
  a STOP.
- VF32F / VF32D: mode `frame` (shapes `F` / `D`): two float developments (the
  file's own and exposure 0.5), their inspection sources and a first frame
  (Clipping off) as setup, then one viewport-only `RawDevelopPreviewFrame`
  with `extendedRange` on and Clipping highlights, at Fit (F) or a 1024 × 1024
  100 % region (D); gated on the preview session's own charge, 48 MiB +
  `extendedFramePerPixel` × the f32 sides' output pixels. A frame that did not
  draw both sides extended is a STOP.

Same method, three runs plus a twin. It fits
nothing: each case is gated on the charge the app itself computes, which the
probe prints as `gate_charge` — `RawSourceWork.scratch(float: true)` for O,
W and K, plus the Develop session's 24 B/px of inspection copies for V, and
`RawLocalWork.measuredScratch(float: true)` plus those 24 B/px for the local
shapes. A case charged below ⌈1.25 × max E⌉, a refusal, a W/K/GR source that is
not a float development, a twin above 16 MiB or an `[AutomationAlerts]` line
is a STOP (exit 1). It writes `measure-float.json`. A miss raises
`RawSourceWork.scratchPerPixel` (decode) or `RawLocalWork.localPerPixel`
(local), after which `--fit-from` and `--recheck-phase2` confirm the 16-bit
rows. The LT shapes are 16- and 8-bit only: a local tone refuses at depth 32.
