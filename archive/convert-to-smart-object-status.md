# Convert to Smart Object — status

- **Phase:** 1 of 8 (the Photoshop measurement) done 2026-10-09; 2a (core model and convert) and 2b (native
  contents) done 2026-10-09; 3 (app, MCP, Actions) done 2026-10-09; 4 (Edit Contents) done 2026-10-09; 5a
  (via Copy, Reset Transform, Embed All Linked, the submenu move) done 2026-10-09; 5b (Convert to Layers, with
  take2 and take2b) done 2026-10-09; 5c (Export Contents, Update All Modified Content) done 2026-10-09; 6a
  (a layered `.rz` or PSD placed, replaced and updated as a smart object) done 2026-10-10; 6b0 (take3) and 6b1
  (Convert to Linked) done 2026-10-10; 6b2-0 (take4) and 6b2 (Edit Contents of linked contents) done 2026-10-10;
  float phase 2a (Convert to Smart Object at 32 bits, float
  contents in `.rz`) done 2026-10-10; float phase 2b (Edit Contents and its commit at 32 bits) done 2026-10-10;
  7a-0 (take5, take6) and 7a (Stack Mode in the core) done 2026-10-10; 7b (Stack Mode in the app) done
  2026-10-10; float phase 2c (Convert to Layers, Export Contents and Convert to Linked at 32 bits) done
  2026-10-10; float phase 2d (a layered `.rz` or PSD placed, replaced, updated and relinked, and linked Edit
  Contents, at 32 bits) done 2026-10-10; 8a (PSD smart objects read at 8 bits) done 2026-10-10; float phase 3a (a
  32-bit float master in 8- and 16-bit documents) done 2026-10-10; float phase 3b-1 (32-bit float contents in 8- and
  16-bit documents) done 2026-10-10; float phase 3b-2 (their Convert to Layers) done 2026-10-10; 8b-1 (the 8-bit PSD smart-object writer) and
  8b-2 (take7: Photoshop opens the writer's files, and authors 8a's corpus) done 2026-10-10 in lane C, merged
  together; 8c-1 (PSD smart objects read at 16 bits) done 2026-10-10; 8c-2a (the writer at 8 and 16 bits) and
  8c-2b (take8: Photoshop opens the 16-bit files) done 2026-10-10 in lane C, merged together; float phase 3c-1 (a PSD
  smart object's 32-bit contents read in 8- and 16-bit files) done 2026-10-10 in lane C; float phase 3c-2 (those
  contents written, take9-3) done 2026-10-10 in lane C; float phase 3c-3 (their masks, layer effects, fill opacity,
  Blend If and adjustment layers, read and written) done 2026-10-10 in lane C.
- **Takes:** `~/devel/rasterize-fixtures/photoshop/convert-smart-object-158/take1` (`measured.json`, re-judged into
  `judged.json`), Photoshop 26.10.0. Runner `scripts/photoshop-probe/convert_smart_object.py`, job
  `convert_smart_object` in `scripts/photoshop-probe/photoshop.jsx` (schema and rule summary in
  `scripts/photoshop-probe/README.md`). Convert to Layers: `…/take2` (valid) and `…/take2b` (invalid), runner
  `scripts/photoshop-probe/convert_to_layers.py` (`--set take2b` for the second). Convert to Linked and linked
  Edit Contents: `…/take3` (valid), runner `scripts/photoshop-probe/convert_to_linked.py`. Edit Contents of a
  linked placed image: `…/take4` (valid), runner `scripts/photoshop-probe/edit_linked_contents.py`. Edit
  Contents at 32 bits: `~/devel/rasterize-fixtures/photoshop/float-edit-contents-600/take2` (valid; take1 invalid
  by a probe build defect), runner `scripts/photoshop-probe/float_edit_contents.py`. Stack Mode: `…/take5` (valid,
  29 STOP rows), runner `scripts/photoshop-probe/stack_mode.py`; `…/take6` (valid, fresh stimuli), runner
  `scripts/photoshop-probe/stack_mode_take6.py`. Convert to Layers, Export
  Contents and Convert to Linked at 32 bits: `~/devel/rasterize-fixtures/photoshop/float-smart-object-files-600/take1`
  (valid), runner `scripts/photoshop-probe/float_smart_object_files.py`. Layered files at 32 bits:
  `~/devel/rasterize-fixtures/photoshop/float-layered-place-600/take1` (valid), runner
  `scripts/photoshop-probe/float_layered_place.py`. Float masters in 8/16-bit documents:
  `~/devel/rasterize-fixtures/photoshop/float-low-depth-600/take1` (valid, one STOP row from the rule) and `…/take2`
  (SC16 alone under the corrected rule, valid), runner `scripts/photoshop-probe/float_low_depth.py`. Float contents
  in 8/16-bit documents: `~/devel/rasterize-fixtures/photoshop/float-contents-low-depth-600/take1` (valid), runner
  `scripts/photoshop-probe/float_contents_low_depth.py`. Their Convert to Layers:
  `~/devel/rasterize-fixtures/photoshop/float-contents-layers-600/take2` (valid; take1 valid with one STOP row,
  CLL8 DELTA, from Photoshop's 8-bit dither), runner `scripts/photoshop-probe/float_contents_layers.py`. PSD smart
  objects written, and the corpus: `~/devel/rasterize-fixtures/photoshop/convert-smart-object-158/take7` (valid)
  and `…/take7c` (valid), runner `scripts/photoshop-probe/psd_smart_object_write.py` (`--set corpus` for the
  second); the 16-bit files: `…/take8` (valid), the same runner's `--set write16`; 32-bit contents: `…/take9-3`
  (valid; take9 void, take9-2 a STOP), `--set write32`. Their layer kinds:
  `~/devel/rasterize-fixtures/photoshop/float-contents-kinds-600/take1` (valid, one STOP row shipped as a near
  match), `…/take2` (valid, two writer rows) and `…/take2-2` (Curves, valid), runner
  `scripts/photoshop-probe/psd_float_contents_kinds.py`.
- **Before this phase** nothing of the command existed (worktree `lane-c` at `6ab230b`: no
  `convert_to_smart_object` in the log or in `app/Sources`, `core/src`, `core/include`). What exists:
  `SourceEdit::Retain` (one-layer typed documents only, `core/src/precision_retained_source.rs`) and
  `CaptureRaster` (host media only). The ceilings work Convert to Smart Object was
  deferred behind has shipped its Parts B–E, so the command is unblocked.

## Decisions in force

- Each Photoshop 26.10.0 take is judged by the rule fixed in the runner's docstring before the take and never
  loosened after it.
- Ruling D1: a Layer ▸ Smart Objects submenu holds Convert to Smart Object and its siblings.
- Ruling D3: PSD smart-object interchange (PlLd/SoLd read and write, PSB where Photoshop uses it) is phase 8.
  take1 (P8, E1), take5 (KP) and take7c supply the read fixtures; take7, take8 and take9-3 showed Photoshop
  opening the writer's 8- and 16-bit files and its 32-bit contents as written.
- Each later phase is one implementer pass and one commit, with its own spec run, under the contract below. A
  verdict that contradicts a contract goes back to the orchestrator as a decision, not an issue.
- 32 bits is not cut: since float phase 2a Rasterize
  converts at 32 bits (float contents under a float master, `.rz` word 68 / `SOCT` kind 3), since phase 2b Edit
  Contents and its commit (the float edit-contents take below), since phase 2c Convert to Layers, Export
  Contents and Convert to Linked (the float files take below), and since phase 2d a layered `.rz` or PSD placed,
  replaced, updated and relinked, with linked Edit Contents of a layered `.rz` (the float layered place take
  below); smart filters at float shipped in phase 187c3.
- The backdrop filter layer stays deferred.

## Headline verdicts

Every row: `python3 scripts/photoshop-probe/convert_smart_object.py --judge ~/devel/rasterize-fixtures/photoshop/convert-smart-object-158/take1`
(the take itself: `python3 scripts/photoshop-probe/convert_smart_object.py --output <fresh dir>`). The judge
reproduces `measured.json` (`reproduced: true`). Deltas are max 8-bit codes over the flattened TIFFs.

| Statement | Verdict | Feeds |
|---|---|---|
| Validity (S0: one smart object in its member's place, one contents layer, exact, no leftover) | valid; 24 jobs, none failed, nothing `not_driven`, no `LEFTOVER` | — |
| Name (every case) | `top`: the smart object takes the topmost member's name (M1: `M1_upper`) | 2 |
| Slot (every case) | `top`: it lands where the topmost member was; M1's unselected middle layer ends up below it | 2 |
| Depth and profile (every case) | `same`: contents at the parent's depth and profile (D16 16-bit; D32 32-bit "sRGB IEC61966-2.1 (Linear RGB Profile)") | 2 |
| Contents canvas | `pixel_bounds`, off-canvas pixels kept (S5: 32 × 24, not the clipped 16 × 24); S3 `effect_bounds` (32 × 32: the shadow is inside); A1 (an adjustment alone) `canvas` | 2 |
| Delta, one member (S0–S6, G1, D16, D32) | `exact` (0 codes) | 2 |
| Delta, several members or an adjustment (M1 110, M2 255, M3 126, M4 181, G2 110, A1 67) | `changed`: the contents composite in isolation, so a skipped middle layer, an adjustment or Pass Through no longer reaches the layers below | 2 |
| Placement: blend | S1/D16/R1 `both` (the smart object is Multiply 100 %, the contents layer keeps Multiply); M3 `inside` (smart object Normal); G1 Normal group `both` (Normal is the smart object's default, so this does not show the blend moving outside); G2 Pass Through `inside` | 2 |
| Placement: opacity, fill | `inside` (the smart object is 100 / 100; contents keep 50.2 / 70.2) | 2 |
| Placement: mask, effects, visibility, locks, label | `inside` (S6: the smart object is visible and unlabelled, the hidden layer inside) | 2 |
| Placement: clipping | S4 `outside` (the smart object stays clipped, its contents layer is not); M4 `inside` (A + C2 selected: C2 stays clipped inside; C1 is left outside and unclipped) | 2 |
| Rasterize (R1) | `raster`; it keeps the smart object's Multiply 100 / 100; composite exact | 2 |
| Convert to Layers (C1) | `group` "`<name> - Smart Object Group`" holding the members, properties restored; composite equal to the converted one (0 codes, `C1_after.tif` vs `C1_event.tif`; command below the table) | 5 |
| Convert to Layers after a 50 % scale (C2) | `group`, members `unscaled` (width ratio 1.0): the transform is dropped (composite equal to the unscaled converted one, 0 codes, `C2_after.tif` vs `C2_event.tif`; command below the table) | 5 |
| Edit Contents, instances (E1) | updated `original+duplicate`; New Smart Object via Copy is independent; `documentID` reads empty (`shares` `unreadable`); `smartObjectMore.ID` is the same for the original and its duplicate, different for the copy | 4 |
| Edit Contents, Canvas Size +16 right (E2) | frame `grew_both` ([8, 8, 32, 32] → [0, 8, 40, 32]): the placed frame stays centred | 4 |
| Export Contents (X1) | converted → `psb`; placed PNG → `png`; written at the exact path given, no extension added | 5 |
| Reset Transform after a 50 % scale (T1) | `restored` (bounds back to [8, 8, 32, 32], composite exact) | 5 |
| Smart filter on a converted layer (P8) | `filtered` | 8 |

The C1 and C2 composite checks (`--judge` records only before → event), run from `scripts/photoshop-probe/` with
`T=~/devel/rasterize-fixtures/photoshop/convert-smart-object-158/take1/job` and `c` set to `C1` or `C2`:
`python3 -c "import sys; from convert_smart_object import max_code, read_tiff as r; print(max_code(r(sys.argv[1]), r(sys.argv[2])))" $T/$c/${c}_after.tif $T/$c/${c}_event.tif`
prints `{'max_code': 0.0, 'verdict': 'exact'}` for each.

## Take2 verdicts (Convert to Layers)

`python3 scripts/photoshop-probe/convert_to_layers.py --judge ~/devel/rasterize-fixtures/photoshop/convert-smart-object-158/take2`
(`reproduced: true`): valid, 31 jobs, no leftover; take2b (`…/take2b`) invalid, its one-member baseline made a group.
Verdicts and what 5b built: the probe README's `### take2`, `doc_smart_object_layers.rs`' build table, the companion's 5b entry.

## Take3 verdicts (Convert to Linked, linked Edit Contents)

`python3 scripts/photoshop-probe/convert_to_linked.py --judge ~/devel/rasterize-fixtures/photoshop/convert-smart-object-158/take3`
(`reproduced: true`): valid, CL1 driven and linked at the exact path, delta 0; 5 jobs, no leftover.
Verdicts: the probe README's `### take3`; what was built: the companion's 6b1 and 6b2 entries.

## Take4 verdicts (linked placed image Edit Contents)

`python3 scripts/photoshop-probe/edit_linked_contents.py --judge ~/devel/rasterize-fixtures/photoshop/convert-smart-object-158/take4`
(`reproduced: true`): valid, EP driven, both records linked to `ep_linked.png`; 1 job, no leftover, no STOP row.
Verdicts: the probe README's `### take4`; what was built: the companion's 6b2 entry.

## Float Edit Contents take (float phase 2b)

`python3 scripts/photoshop-probe/float_edit_contents.py --judge ~/devel/rasterize-fixtures/photoshop/float-edit-contents-600/take2`
(`reproduced: true`): valid, 5 jobs, no leftover, no STOP row. Verdicts: the probe README's `### float edit
contents`; what was built: the companion's float phase 2b entry (the history name).

## Take5 and take6 verdicts (Stack Mode)

take5: `python3 scripts/photoshop-probe/stack_mode.py --judge ~/devel/rasterize-fixtures/photoshop/convert-smart-object-158/take5`
(`reproduced: true`); valid, 20 jobs, no leftover, 29 STOP rows (six modes fit no candidate). take6, the
confirmation on fresh stimuli with its hypotheses fixed in the runner's docstring:
`python3 scripts/photoshop-probe/stack_mode_take6.py --judge ~/devel/rasterize-fixtures/photoshop/convert-smart-object-158/take6`
(`reproduced: true`; valid, 18 jobs, no leftover). For Skewness and Kurtosis the verdict of record is
`redecided.json` (`--redecide …/take6`): it re-applies the docstring's own pre-fixed rule (a pixel is undefined
when every member code is equal), which the judge code had implemented as a float test; `measured.json` and
`judged.json` are untouched.

| Reading | Verdict | Built (7a) |
|---|---|---|
| Mean, Maximum, Minimum, Range, Median (take5 K3/K4/K1/K16, take6 K5–K7) | on the codes; Median the middle pair's average | yes, 8 and 16 bits |
| Summation, Variance, Standard Deviation, Skewness, Kurtosis, Entropy (take6 a) | the statistic (Σv; s²; √s²; n−1 forms, negatives 0; bits capped at 1) clipped to [0,1] and encoded by the document's TRC (KT: Adobe RGB γ, not sRGB) | yes at 8 bits; 16 bits refuses by name, no candidate fit |
| Flat pixels, Skewness | Photoshop reads float noise that changes with n | Rasterize writes 0 (disclosed) |
| Members (take6 b) | hidden excluded; opacity, blend, style ignored; mask applied; adjustment skipped; clipped unclipped; a group gives its leaves, nested too | yes |
| Alpha (take6 c) | a member counts, unweighted, at alpha ≥ 128; with two or more members the result is opaque, black where none counts (one member: see K1) | yes (16 bits: 32896, unmeasured) |
| One-layer contents (K1) | Mean/Max/Min/Median/Summation the member; Range and the dispersion modes (0, 0, 0, 0) | yes (transparency since 7b) |
| None, history (take6 d) | `imageStackConvertSmartObject` restores the composite; "Rendering Image Stack <Mode>", "Stack Mode: None" | yes (core; history names 7b) |
| Instances (take6 e) | only the targeted instance changes; all instances keep ONE shared `documentID`; the mode lives on the layer (`smartObjectMore.import`) | per-record mode over shared contents |
| COPY, RE_EDIT, RASTERIZE, TO_LAYERS, PERSIST (take5) | `independent`, `restacked`, `stack`, `group`, `kept` | yes (PSD interchange is phase 8) |
| PLAIN / PLACED (take5) | a plain layer refuses ("The command “Set Stack Mode” is not currently available."); a placed PNG is `driven` | refuses both (no contents) |

## Float files take (float phase 2c)

`python3 scripts/photoshop-probe/float_smart_object_files.py --judge ~/devel/rasterize-fixtures/photoshop/float-smart-object-files-600/take1`
(`reproduced: true`): valid, 10 jobs, no leftover, no STOP row. Verdicts and what was built: the probe README's
`### float smart object files` and the companion's float phase 2c entry.

## Float layered place take (float phase 2d)

`python3 scripts/photoshop-probe/float_layered_place.py --judge ~/devel/rasterize-fixtures/photoshop/float-layered-place-600/take1`
(`reproduced: true`): valid, 8 jobs, no leftover, no STOP row. Verdicts and what was built: the probe README's
`### float layered place` and the companion's float phase 2d entry.

## Float contents Convert to Layers take (float phase 3b-2)

`python3 scripts/photoshop-probe/float_contents_layers.py --judge ~/devel/rasterize-fixtures/photoshop/float-contents-layers-600/take2`
(`reproduced: true`): valid, no STOP row (take1's CLL8 DELTA STOP was Photoshop's 8-bit dither, RULING-1). Verdicts:
the probe README's `### float contents layers`; what was built: the companion's float phase 3b-2 entry.

## Take7 verdicts (PSD smart objects written, phase 8b-2)

`python3 scripts/photoshop-probe/psd_smart_object_write.py --set write --judge ~/devel/rasterize-fixtures/photoshop/convert-smart-object-158/take7`
and `--set corpus --judge …/take7c` (`reproduced: true`): both valid, 7 jobs each, no STOP or CONTRADICTION row.
Verdicts, the writer choices they confirm, Photoshop's own form and 8a's reads: the probe README's `### take7`.

## Take8 verdicts (16-bit files, phase 8c-2b)

`python3 scripts/photoshop-probe/psd_smart_object_write.py --set write16 --judge ~/devel/rasterize-fixtures/photoshop/convert-smart-object-158/take8`
(`reproduced: true`): valid, 7 jobs (control C16), no STOP or CONTRADICTION row, so 8c-2a's writer stands.
Verdicts and the writer choices they confirm: the probe README's `### take8`.

## Phase-8 fixtures

In `take1/job/` and `take5/job/`, written by Photoshop 26.10.0 in the takes, and committed verbatim (with P8's
`P8_before.tif`, `P8_after.tif`, `P8_contents.tif`) in `core/tests/fixtures/psd_photoshop/smart_objects/`.
`P8_after.tif` is S0's converted state before the filter (its pixels equal `P8_before.tif`'s):

| File | What it is | SHA-256 |
|---|---|---|
| `P8/p8_smart_object.psd` | S0 converted (embedded smart object) | `8d66d2163e4e87dac86743c09b660d53470121b24eac5ee6e606f577776e6f87` |
| `P8/p8_smart_filter.psd` | the same with a 0.5 px Gaussian Blur smart filter | `ba9d9b76c8c6008c6ad5c94cb4ac2fc59fd71863f1d4aeaddaac8d6d054f25fb` |
| `P8/P8_before.tif` | S0's picture before the conversion | `1f4bfac3e16b38b849170a97a879d3551abbe32d5d6ddc290c086c8bb778f127` |
| `P8/P8_after.tif` | S0 converted, before the filter | `c495920ab2f5d302d225e94c129be3f9167aa27b089c5d8290283099d13619b6` |
| `P8/P8_contents.tif` | the smart object's contents (the master's oracle) | `3255494720080781966b257bbf3fe6120f2e8cfcd462731aded13e260abafb13` |
| `E1/p8_contents.psb` | E1's contents `.psb` as Photoshop wrote it when Edit Contents opened it | `61e7d95e089ed7e53a1be702612044013356b8b41babb678c701f5d9daeb9d8a` |
| `take5/job/KP/KP_saved.psd` | KP: a three-member smart object at Median, saved (`Impr` class `medn`, `Type` 3) | `d587ead61040509d23666c75ff9028d70a2d1e179bfc896094702cbbd2e1a084` |
| `take5/job/KP/KP_s0_KP_m3.tif` | KP's record isolated after Median, before the save | `122edac776b3f6a087099176d8508ef2cfc80c8945a2ff7a138a31cc72f0180b` |
| `take5/job/KP/KP_s1_KP_m3.tif` | the same after the save and reopen | `28448c22e15139e892649f95531b0de886ced25b8ff59b77801558d8e0c0c31b` |

take7's six `rz7_` inputs, take8's six `rz8_` inputs, take9's five `rz9_` inputs, take7c's `t7_` corpus, float phase
3c-1's six `f3_` files and take9-3's three `f9_` files are listed with their SHA-256s in `scripts/photoshop-probe/README.md`, `### The smart_objects family`.

## Phase plan

Items 1–5, 6a, 6b0, 6b1, 6b2, 7a-0, 7a, 7b, 8a, 8b-1, 8b-2, 8c-1, 8c-2a and 8c-2b have shipped; their record is in the shipped companion (`## Shipped runs`).

6. **Layered files as smart objects**, split in two.
   - **6a (shipped 2026-10-10)**: Place, Replace, Update Modified and Update All of a layered `.rz` or PSD.
   - **6b0 take3 and 6b1 Convert to Linked (shipped 2026-10-10)**: in the shipped companion.
   - **6b2-0 take4 and 6b2 Edit Contents of a linked smart object (shipped 2026-10-10)**: in the shipped
     companion.
7. **Stack Mode**, split in two.
   - **7a-0 take5/take6 and 7a the core (shipped 2026-10-10)**: in the shipped companion.
   - **7b the app (shipped 2026-10-10)**: in the shipped companion.
8. **PSD smart-object interchange** (D3), split in four. Type, shape and vector masks are separate work.
   - **8a, read at 8 bits (shipped 2026-10-10)**: in the shipped companion.
   - **8b-1, the writer at 8 bits, and 8b-2, take7 (shipped 2026-10-10)**: in the shipped companion.
   - **8c-1, read at 16 bits (shipped 2026-10-10)**: in the shipped companion.
   - **8c-2a, the writer at 8 and 16 bits, and 8c-2b, take8 (shipped 2026-10-10)**: in the shipped companion.
   - **8d, linked files, embedded rasters, smart filters and warps**, per take7 (take7c's CL, CP and CW, and
     Photoshop's own forms above).

- **The float phases at 32 bits** (2a, the convert and `.rz` float contents, 2b, Edit Contents and its commit, 2c,
  Convert to Layers, Export Contents and Convert to Linked, and 2d, a layered `.rz` or PSD placed, replaced,
  updated and relinked with linked Edit Contents, 3a, a float master in an 8- or 16-bit document, 3b-1, float
  CONTENTS in an 8- or 16-bit document, 3b-2, their Convert to Layers, 3c-1, a PSD smart object's 32-bit contents
  read in an 8- or 16-bit file, 3c-2, those contents written there as a depth-32 PSB (take9-3), and 3c-3, their
  masks, layer effects, fill opacity, Blend If and adjustment layers read and written (float-contents-kinds-600),
  shipped 2026-10-10). Left: Stack Mode over float contents.

## Sub-features

"yes" = shipped today.

| Sub-feature | Today | Plan |
|---|---|---|
| Convert to Smart Object, single/multi/group/any kind | yes, 8/16 bits (3) and 32 bits (float phase 2a); across groups refuses | — |
| Filter ▸ Convert for Smart Filters | yes, with an alert; Live Filters… on a plain layer offers it (3) | — |
| Smart filters with masks, per-filter opacity/blend | yes, 5 operators (`LiveFilter.swift`) | breadth stays with each filter's issue |
| Edit Contents; instances update together | yes, 8/16 bits (4) and 32 bits (float phase 2b), 32-bit contents in 8/16-bit documents (float phase 3b-1); not recorded into Actions | — |
| Rasterize | yes, Layer ▸ Smart Objects ▸ Rasterize (5a) | R1 verdict checked in 2 |
| Replace Contents / Relink to File / Embed Linked / Update Modified / Save Portable Image Copy | yes, in Layer ▸ Smart Objects (5a; Relink to File also resolves a broken link) | — |
| New Smart Object via Copy, Reset Transform, Embed All Linked | yes, 8/16/32 bits (5a); defaults to measure | — |
| Convert to Layers | yes, 8/16/32 bits (5b; 32 since float phase 2c); 32-bit contents in an 8/16-bit document convert at 32 bits into its profile, then narrow clipped (float phase 3b-2); a lone layer or placed image outside CI/D32L's state (a plain raster at default state, named as the record), warp, RAW refuse | — |
| Export Contents, Update All Modified Content | yes (5c); Export at 8/16/32 bits (32 since float phase 2c), contents as `.rz`, placed JPEG/TIFF as PNG and an f32 master as a float TIFF; RAW/media links skipped | — |
| Place a layered file (`.rz`, PSD) as a smart object; Replace / Update / Update All from one | yes, 8/16 bits (6a)
and 32 bits (float phase 2d: contents at the file's depth); a 32-bit layered file in an 8/16-bit document keeps 32-bit
contents under `convert_fallback` (float phase 3b-1); a layered Replace is "Replace Contents" and a place "Place Embedded/Linked Smart Object"
(float-layered-place-600/take1 RC16/RC32, PE8/PL32); depth prompt, frame, Update/Update All names unmeasured | — |
| Convert to Linked | yes, 8/16/32 bits (6b1; 32 since float phase 2c): the target alone, `.rz` contents; RAW/media, open contents, locks, Actions unmeasured | — |
| Edit Contents of a linked smart object | yes, 8/16/32 bits (6b2; 32 since float phase 2b, a linked layered `.rz` since 2d): the linked file opens; its save updates every linked record, "Update Smart Objects"; a PSD, >8 bits or a non-Save format refuses; other cases unmeasured | — |
| Stack Mode | yes (7a core, 7b Layer ▸ Smart Objects ▸ Stack Mode, MCP `set_smart_object_stack_mode`, Actions): every mode at 8 bits, Mean/Maximum/Minimum/Range/Median at 16, per instance, `.rz` words 69/70; 16-bit encoded modes and 32 bits refuse | — |
| 32-bit convert / smart filters at 32-bit | yes: float contents under a float master (float phase 2a); Edit Contents and its commit (float phase 2b); Convert to Layers, Export Contents, Convert to Linked (float phase 2c); a layered `.rz` or PSD placed, replaced, updated and relinked (float phase 2d); placed 8/16 and 32-bit float masters (float phase 1); a 32-bit float master in 8/16-bit documents, rendered clipped (float phase 3a); float contents in 8/16-bit documents, kept through narrowing, paste, Edit Contents and `.rz` (float phase 3b-1), and converted to layers (float phase 3b-2); a PSD smart object's 32-bit contents read in 8/16-bit files (float phase 3c-1) and written there (float phase 3c-2), with their masks, effects and adjustment layers (float phase 3c-3); smart filters (phase 187c3) | — |
| PSD smart-object read/write (PlLd/SoLd, PSB) | read at 8 and 16 bits (8a, 8c-1): embedded 8- or 16-bit PSD/PSB contents in an 8- or 16-bit file carry as a smart object with transform, instances and Stack Mode (16-bit contents typed; an encoded mode over them keeps its pixels); 32-bit contents carry as float contents under an `Rgba32F` master, with masks, layer effects, fill opacity, Blend If and adjustment layers (float phase 3c-3), when every layer is one a float document keeps (a fill layer and a folder past 10 levels refuse by name), else as Photoshop's stored composite where it saved a real one, ZIP or merged transparency included (float phase 3c-1, 3c-2), their own smart objects resolved (3c-2); every other smart object keeps its pixels, named per record. Write at 8 and 16 bits (8b-1, 8c-2a, float 3c-2; Photoshop opens them as written, take7, take8, take9-3): a smart object with 8-, 16- or 32-bit contents is written as `PlLd`/`SoLd` plus an `lnk2` `liFD` per instance group, contents at their own depth (32-bit as a depth-32 PSB with masks, styles and adjustments, take2 / take2-2); every other one is pixels only, named per record | 8d linked, raster, filtered and warped (D3) |

## Shipped runs

Phases 1–5, 6a, 6b0, 6b1, 6b2, 7a-0, 7a, 7b, 8a, 8b-1, 8b-2, 8c-1, 8c-2a and 8c-2b, and float phases 3a, 3b-1, 3b-2, 3c-1, 3c-2 and 3c-3: [convert-to-smart-object-shipped.md](convert-to-smart-object-shipped.md).

## Open items

- Stack Mode: the six encoded modes at 16 bits and every mode at 32 bits refuse, flat-pixel Skewness writes 0
  where Photoshop shows float noise, and the 16-bit alpha threshold, hidden and ancestor groups, one-member
  identity modes over transparent member pixels, zero members, the reset on rebinding (and Update Smart Objects
  of a linked layered file resetting to None), the TRC source, placed images without contents, the menu's
  separator after None, its checkmark, an enabled current-mode item that does nothing, the Actions step
  for Stack Mode and the per-asset `SOCT` contents are unmeasured or open.

- Edit Contents of a linked smart object: a save from a window Edit Contents did not open, other open documents
  linking the file, a separately placed record on the same file, a depth or profile change in the linked
  window and a missing link at Edit Contents are defaults until measured.
- Edit Contents refuses a linked PSD, a file above 8 bits per channel and a format Save does not write, because
  Save cannot write them in place.

- Convert to Linked of a camera RAW or host-media source, with the contents window open, its lock
  rule and its Actions recording are refusals or defaults until measured.

- Placing a layered file: the depth prompt, frame position, the history names of Update Modified
  Content, Update All Modified Content and a non-layered Replace, what it renders for a PSD whose layered import Rasterize refuses (placed from its stored composite here), and a PSD whose imported
  layers differ from its saved composite are defaults until measured.

- Reset Transform after a move, on a fitted placed image and with a mask, New Smart Object via Copy's name, the
  phase-5a and renamed history names and the Smart Objects menu order are defaults until measured.
- Edit Contents' window title (measured as the source's file name, not built), a placed PNG's commit after
  layers were added, and closing the parent while contents are open are defaults until measured.
- Edit Contents is not recorded into Actions.
- Convert across groups refuses until its placement is measured.
- Float contents (every float phase shipped with 3c-3): inside 32-bit contents a
  fill layer, an adjustment the PSD codec reads at no
  depth, Brightness/Contrast and Color Lookup among them,
  a blend a float document refuses (Divide) and folders
  past Photoshop's 10 levels keep the record's pixels (or Photoshop's real stored composite), named; a map-form
  Curves, a feathered mask, a style Photoshop's dialog cannot hold and an effect colour its 32-bit numbers cannot
  state export pixels only. Known differences, each under
  a quarter of an 8-bit code (shipped as near matches): effect colours travel at the codec's 8-bit numbers (take1's
  COLOUR read Photoshop's f32 component 7.6e-6 of a code off; renders within 0.5/255), and a Photo Filter's colour
  as 8-bit sRGB where Photoshop keeps Lab (take2: 0.102 of a code). The writer does not write a contents document's tone map (a reported loss) and writes
  Photoshop's blank composite into the PSB; a ZIP 32-bit stored composite is read with no Photoshop file to check it.
  A 32-bit PSD or PSB is not opened, placed or exported as a document. Stack Mode over float contents refuses at every depth. Convert to Layers of one-layer float contents
  and of float contents already in the document's profile are built by 3b-2's rule, unmeasured. A float
  convert whose result passes the native decoded cap (24 MP with a 24 MP Background below) is admitted by the
  plan and refused by the post-render preflight ("too large to save and reopen on its own"), today's rule for
  every typed edit over the cap.
- No event was `not_driven` in take1, so no post-convert event is left unmeasured.
- 8d: a linked file, an embedded raster, smart filters and a warp stay pixels only on read and write. Contents export and
  link as `.rz` where Photoshop writes `.psb`.
- Export Contents beyond X1 and the float phase 2c take (placed JPEG/TIFF, linked states, RAW and host stills, 16 bits, the proposed name,
  recording) and Update All Modified Content (history name, instances, missing links, locks, depth change, menu
  position), plus Update Modified Content's lock rule and instance update, are defaults until measured.
- A placed JPEG or TIFF exports as PNG because the encoded original is not retained.
- Linked RAW and host-media sources are skipped by Update All and refreshed one record at a time by Update
  Modified Content.
- Convert to Layers: a lone layer or placed image outside CI/D32L's state (record or member state, a renamed record) refuses until measured,
  and so do RAW and a warp; the hidden members' state and confirmation dialogs are open too. A 24 MP 16-bit convert fits at the 8 GiB limit of a
  16 GB Mac; only the 2 GiB test pin refuses it (the paste's 65 B/px margin).
