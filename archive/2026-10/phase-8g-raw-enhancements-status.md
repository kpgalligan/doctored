# Phase 8G — camera controls and local RAW development (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 8G, 2026-09-11 (completes Phase 8)
- **Original:** [archive/phase-8g-raw-enhancements-status.md](../archive/phase-8g-raw-enhancements-status.md) — full history, measurements, receipts
- **Key commits:** `e386448` (follows 8F `9b3f65c`)

## Context
8C/8D ([phase-8-retained-source-status.md](phase-8-retained-source-status.md)) retained RAW originals with a
global recipe only. Photographers need local exposure/white-balance adjustments that re-develop the *camera
data* (recovering highlight detail a darkened byte preview cannot), plus camera-relative controls — without
introducing a linear/HDR document mode, and with persisted masks that survive migration, presets and sync.

## Decisions
- **Frozen evaluation order:** original camera bytes → pinned global camera/vendor lens development for the base
  and each enabled local variant → chosen sRGB/Display P3 integer u16 output → ordered native coverage blend
  (float accumulator) → calibrated manual vignette → u16 source quantization → explicit optics → develop
  grain → existing retained layer
  affine/profile/fallback → layer mask/retouch. Variants share decoder, orientation, dimensions and output ICC.
  *Why:* each local develops the original, never a byte preview; single quantization keeps the blend exact.
- **Exact bypasses:** zero coverage, disabled/identity locals and neutral vignette are bit-exact no-ops.
- **Locals:** at most four, ordered, in top-left, oriented, full-source pixel coordinates *before* layer
  geometry. Coverage is native u16 from radial/linear primitives and ordered brush add/subtract, with inversion,
  density and feather; never derived from a byte preview. Each local changes exposure offset and WB intent.
  Hard radial fields use point-center sampling (not area integration). Code: `raw_local_*.rs`,
  `RawLocal*.swift`, `RawDevelopWindowController+Locals.swift`.
- **Global controls exposed only where a capability matrix shows effect:** camera-relative baseline compensation,
  moiré reduction (where supported), manual profile-aware vignette. Unsupported controls (e.g. vendor lens
  correction and local tone map on the D70s) are not shown.
- **Recipe 3** stores every effective control plus captured effective variants; recipes 1/2 and strict binary64
  persistence keep their meaning. Unknown recipe or mask versions keep cached native rendering and refuse
  semantic edits.
- **Mask storage:** versioned `RZLM` blobs in the 8F native 21/22 `RBIN` pool under `raw_local_masks`; mask
  pixels never enter the capped recipe JSON. ≤64 MP per mask; originals + masks keep the 256 MiB per-resource /
  512 MiB aggregate caps. Unchanged masks/originals share storage across revisions; every recognized header,
  length and reference validates before pixel decode.
- **Transfer rules:** global presets/sync transfer camera-relative compensation, supported moiré and vignette
  while preserving each target's local IDs and masks; explicit local copy transfers exposure/WB intent only;
  never another camera's hidden recipe/decoder or a target's coverage.
- **Migration** at unchanged geometry preserves planes exactly and reviews the complete new development
  (12F-4 later added reviewed EXIF/crop/pad and filtered-affine remap of local planes).
- **Plumbing:** UI/MCP/Actions share bounded captured operations and one prepared publication. Cheap attachment
  kinds keep RAW masks out of host-frame summaries/menus/recording even when metadata reads are denied.
- **Bounds:** ≤4 locals, 5 cached camera developments / 256 MiB, 256 pending operations, 4,096 aggregate brush
  dabs, 256 M evaluated brush-box pixels per native op, 256 KiB operation JSON. Platform capture/render calls are
  noncancellable; native stages cooperate. The 64 MP cap is not a four-local 64 MP qualification.

## Rejected alternatives
- Darkening/brightening a developed byte or u16 preview for locals — cannot reveal clipped-but-recoverable camera detail.
- Mask samples inside recipe JSON — capped, slow, and duplicates across revisions.
- A linear or HDR document mode for local blending — out of scope; the encoded SDR policy is kept.

## Consequences
- Anything that changes source extent/orientation must remap `RZLM` planes through a registered inverse map or refuse.
- Local planes are source-local; they do not satisfy document-selection, scalar painting or backdrop-histogram requirements.
- Declared corpus is Nikon D70s (CIRAW decoder `8`) + oriented synthetic DNG (`8.dng`) on macOS 26.5.1; no all-camera claim.

## Qualification
Frozen before implementation: analytical mask coverage within **1 u16 code**; ordered blend vs a float64 oracle
within **`2e-6 + 0.5/65535`**; sRGB/P3 vignette and calibrated exposure within **3 u16 codes**; complete SDK
base/local camera develops within **3 u16 codes**; outside-mask, disabled, neutral and camera alpha exact; local
negative exposure must reveal original-derived detail absent from a clipped base. Results: SDK comparisons max 1
code over 577.5 M components; native masters exact over 192.5 M components; mask oracles max 1; blend max
7.75e-6; vignette max 1; real 2× Retina gesture geometry verified. Reproduction:
[`scripts/raw-local-probe`](../scripts/raw-local-probe/README.md), `scripts/verify_phase8g.py`,
[`camera-fixture.json`](../scripts/raw-probe/camera-fixture.json).

## Open items (verified 2026-09-26)
- `TRACKED` — Broader camera/decoder corpus, more camera/lens controls and calibration datasets, >4 locals, larger mask/gesture and multi-source/history workloads → plan §13D (explicitly lists 8G expansion), P7-33.
- `TRACKED` — Tighter codec cancellation / source-heavy scale → P7-32 (§13C).
- `TRACKED` — Linear/HDR/float RAW policies → plan §11F open list (RAW-developed HDR fixture) and P7-24 (§11E).
- `TRACKED` — Expanded ProRes cross-decoder comparison (from 8F) → §13D.
- `DONE` — Lens distortion / transverse CA as source-stage geometry and geometry-changing local-plane remap (12F-4, `6400648`; `raw_local_remap.rs`, `RawLocalRemapSheetController.swift`).
- `DONE` — Typed capture packets/PPI for RAW (11D typed metadata bridge); typed auxiliary mattes (9G, P7-08).
- `DONE` — Per-entry RAW preset repair (12B: `broken` rows, `repair_raw_preset`, schema 2 on write).
- `DONE` — An honest, measured RAW decode charge.
  `RawSourceWork.scratch` = **173 B/px + 9 MiB + 3 × the file** (was 64 B/px + 32 MiB + 3 × the file, which
  charged the genuine D70s NEF 434,158,071 B against a measured ~807 MB peak). Method
  (`scripts/raw-decode-probe`, record beside the constants): one case per process, `MallocLargeCache=0`,
  3 runs plus a baseline twin, a 32 GiB harness limit, calibration exact (1 GiB returns 0 B, 256 MiB reads
  268,566,720), every twin 0. Cases: open (O), warm redevelop (W), cold first decode (K) and import preview
  (V) at 0.2–45 MP on synthetic DNGs and the NEF, 16 and 8 bits. Max E: NEF 796–843 MB in every mode (V16
  842,662,296); 24 MP 1.11–1.52 GB (V16-D24 1,523,647,232); D45 1.67–1.69 GB. Fit (deterministic):
  need = ⌈1.25 × max E⌉; a = the worst per-pixel whole cost at ≥ 2 MP (V16-NEF, 173), so an unmeasured
  format is charged at the compressed camera file's rate; F = 9 MiB (K8-D0). Charge ÷ max E ≥ 1.265 in
  every case (NEF 1.27–1.34; synthetic DNGs ≥ 3 MP 2.1–4.9). Residency: `develop` now renders through its
  own `CIContext`, cleared and dropped; footprint − P0 − `unique_retained_bytes` settles at ≤ 26 MB (D24/16,
  NEF/16, D24/8; the release is asynchronous: up to 0.78 GB right after the render in the harness, 0.31 GB
  above retained at +1 s in the app) against up to 0.5–0.7 GB with the shared context (`clearCaches()` alone left 0.7 GB); in the app
  R16 settles to 73 MB above retained within 6 s. Ceilings at the 2 GiB reference: open ≈ 11.9 MP
  (uncompressed DNG; 12.2 MP at the NEF's bytes per pixel), redevelop ≈ 10.6 MP (16-bit) / 10.9 MP (8-bit)
  beside the opened document; the drive's D12 (12 MP, 2,157,440,196) and D24 (4,305,440,196) opens are
  refused on their own, and D0, D3 and the NEF (1,065,784,823) open, redevelop and undo at both depths.
  At 8 GiB (a 16 GB Mac): R16/R8, RF16/RF8 (σ4 live filter), R26-16/R26-8 and C16/C8 open, redevelop,
  read back and undo; the D24 `edit_raw_development` (5,653,634,500 local) and the D45 open
  (8,143,383,492) are admitted; every `--predict` verdict matched. 2 GiB verify: `verify_phase8g`'s
  camera-16-srgb migration fixture now stops at its own "needs spare metadata capacity" precondition (the
  local budget is 1.41 GB); at 8 GiB it fails as HEAD does at 2 GiB ("Expected refusal"), and HEAD also
  fails model-8 and model-16, as this tree does at both limits; `verify_phase8c`'s host probe hangs at
  HEAD as well.
- `DONE` — the menu path's own terms.
  **Formulas.** `RawLocalWork.restoreScratch` = 28 MiB + 3·px + 3·encoded is Layer ▸ Redevelop RAW…'s
  restore admit (`restoreForRedevelop`: admit, restore on the worker, `RetainedRawDecoder.filter`,
  `RawImage.inspect`), which used to take the whole decode charge (4,305,440,196 at D24; now
  245,363,140). `RawLocalWork.measuredScratch` = max(`RawSourceWork.scratch`, 0 + (170 + 11·k + 0·e)·px
  + 3·encoded), k = captured locals + creates (≤ 4), e = authored lens, grain or vignette, is what
  `RawLocalWork.request` charges: the Develop preview (`RawLocalDevelopSession`, + 64 MiB display), MCP
  `edit_raw_development`/copy-local and enhanced `redevelop_raw`. Decoder migration keeps
  `RawLocalWork.scratch` (decode + 56 B/px + 4 MiB). The preview now drops a superseded candidate before
  the next request's admission. Stages stay sequential (max(), separate admissions).
  **Method** (`scripts/raw-decode-probe --phase2`, as for the RAW decode charge above): one case per process,
  3 runs + baseline twin (every twin E 0), calibration exact, 32 GiB harness limit, no `[AutomationAlerts]`. Modes: RS
  (`restoreForRedevelop` + master + session alive), L (the session's preview request run to ready), PB
  (`PreparedDocumentEdit` of the result), and the cold `.rz` K cases; shapes L0, L1, L1B (2048 dabs of
  radius 180 on a 64×32 grid, 255,568,481 / 255,998,475 evaluated px at D24 / D26 against the core's
  256,000,000 cap: the worst admitted brush), L1C, L4, FX. **Case tables** (max E, 16 / 8 bits): RS D24
  96,354,352 / 96,354,352, D26 104,202,312 / 104,120,392, NEF 10,436,608; RSK D24 96,714,824 /
  108,445,840; L0 D24 1,376,944,584 / 1,285,849,472, NEF 828,670,072 / 796,573,816; L1 D24 1,618,051,744
  / 1,526,907,480, D26 1,733,673,728 / 1,630,061,264, NEF 855,654,592 / 831,979,688; L1B D24
  1,425,162,744 / 1,334,084,016; L1C D24 1,377,665,528 / 1,467,253,312; L4 D24 2,211,366,232 /
  2,250,491,296, D26 2,516,108,896 / 2,412,086,808, NEF 1,037,468,104 / 1,019,117,976; FX D26
  2,389,952,240 / 2,286,044,864 (every case in `RawLocalWork.swift`'s record). **Fit** (deterministic,
  need = ⌈1.25 × max E⌉): restorePerPixel 3 (RSK8-NEF), restoreFixed 28 MiB (RSK8-D0); localPerPixel 170
  (L0-16-NEF), localPerLocalPixel 11 (L4-16-NEF), localPerEffectsPixel 0, localFixed 0. Charge ÷ max E
  ≥ 1.256 in every fitted case (L4-16-NEF; the NEF 1.26–1.34, D3 1.74–2.46, D12–D26 2.03–4.34); the
  verification's second full run fits 168 / 12 / 0 / 0 and 2 B/px + 23 MiB, and the committed constants
  cover all 84 of its fitted cases at ≥ 1.26×.
  **Publish check:** the structural publish projection estimate was 1.004× its measured peak at 16 bits and
  1.2× at 8 bits (D24 576,291,224 / 480,248,144 against 578,608,290 / 576,000,000; D26 likewise, 0 and 1 local
  alike). Since batch 165
  `projectionScratchEstimate()` adds `ProjectionMargin`'s 6/16 at both depths: 795,586,399 / 792,000,000 at
  D24, 1.380× / 1.649×, gated here at ≥ ⌈1.25 × max E⌉.
  **`menu`** (stage by stage, `--menu`): at 8 GiB M0, M1, M11 and MC pass every stage at D24 and D26,
  16 and 8 bits (preview charges 4,372,549,060–4,921,332,676 beside 0.52–1.13 GB retained; publish
  576,000,000–625,698,376); at 2 GiB M0 and M1 pass at D3 and on the NEF, 16 and 8, and M1-16-D12 is
  refused at the open ("This job needs 2.01 GB of working memory on its own…").
  **Drive** (owned instances, `RZ_WORKING_LIMIT_BYTES` 8 GiB on 4993 and 2 GiB on 4994): at 8 GiB E0, E1,
  EC, E4 and EFX (`edit_raw_development` with 0, 1, then exposure 0.2, 4 locals, and FX) pass at D24 and
  D26, 16 and 8 (charges 4,305,440,196–5,710,851,012 beside ~0.43–0.57 GB retained), with
  `get_raw_locals` and `undo`; `redevelop_raw` R16/R8 at D24 pass; at 2 GiB E0/E1 at D0, E1/E4 on the NEF
  (1,104,475,639 / 1,303,003,639) and E1 at D3 pass at both depths, and D12's `open_document` is
  refused. Every `--predict` verdict matched; no `[AutomationAlerts]`.
  **Ceilings** (preview charge + 64 MiB beside the opened document, M0-D24's 25.6 / 21.6 B/px retained):
  8 GiB ≈ 41.6 / 40.1 / 34.7 MP for 0 / 1 / 4 locals (uncompressed 16-bit DNG; 8-bit 42.4 / 40.9 /
  35.3; NEF bytes per pixel 42.3 / 40.7 / 35.2), so 24–26 MP has no 8 GiB ceiling up to four locals;
  2 GiB ≈ 10.1 / 9.8 / 8.5 MP (8-bit 10.3 / 10.0 / 8.6), below the 11.9 MP open ceiling. **Migration
  follow-up:** none needed by the data — the unchanged migration envelope (`scratch` + 8 B/px) covers
  every L4 case at ≥ 1.25× (≥ 1.12× need on the NEF); it stays unmeasured as its own stage.
