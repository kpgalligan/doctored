# Phase 8G — camera controls and local RAW development (ADR)

- **Status:** Accepted — open items remain
- **Phase / dates:** 8G, 2026-09-11 (completes Phase 8)
- **Originals:** [archive/phase-8g-raw-enhancements-status.md](../archive/phase-8g-raw-enhancements-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-8g-raw-enhancements-status.md](../archive/2026-10/phase-8g-raw-enhancements-status.md) — this doc as of 2026-10-07, with every measurement table and issue record
- **Key commits:** `e386448` (follows 8F `9b3f65c`)

## Context
8C/8D ([phase-8-retained-source-status.md](phase-8-retained-source-status.md)) retained RAW originals with a
global recipe only. Photographers need local exposure/white-balance adjustments that re-develop the *camera
data* (recovering highlight detail a darkened byte preview cannot), plus camera-relative controls — without
introducing a linear/HDR document mode, and with persisted masks that survive migration, presets and sync.

## Shipped runs

[phase-8g-raw-enhancements-shipped.md](phase-8g-raw-enhancements-shipped.md) — the decisions in force: evaluation
order, locals, range, generated and Sky masks, the 32-bit float development, the Develop Filter and its masks,
Develop Presence, the local tone path and detail, syncing locals, mask storage, transfer rules and bounds.

## Rejected alternatives
- Darkening/brightening a developed byte or u16 preview for locals — cannot reveal clipped-but-recoverable camera detail.
- Mask samples inside recipe JSON — capped, slow, and duplicates across revisions.
- A linear or HDR document mode for local blending — out of scope for the 16-bit development, which keeps the encoded SDR policy; the 32-bit float development blends in float.

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

## Open items (verified 2026-10-07)
- `TRACKED` — Broader camera/decoder corpus, more camera/lens controls and calibration datasets, larger mask/gesture and multi-source/history workloads → plan §13D (explicitly lists 8G expansion), P7-33.
- `TRACKED` — Tighter codec cancellation / source-heavy scale → P7-32 (§13C).
- `DONE` — the 32-bit float (linear) RAW development, generated and Sky locals, the Develop Filter and its masks and syncing locals across pictures (Decisions in the shipped companion).
- `TRACKED` — Expanded ProRes cross-decoder comparison (from 8F) → §13D.
- `OPEN` — decoder migration's `RawLocalWork.scratch` envelope is unmeasured as its own stage but covers every
  measured four-local case at ≥ 1.25×.
- `OPEN` — `scripts/verify_phase8c.py` fails: its host probe hangs.
- `DONE` — Lens distortion / transverse CA as source-stage geometry and geometry-changing local-plane remap (12F-4, `6400648`; `raw_local_remap.rs`, `RawLocalRemapSheetController.swift`).
- `DONE` — Typed capture packets/PPI for RAW (11D typed metadata bridge); typed auxiliary mattes (9G, P7-08).
- `DONE` — Per-entry RAW preset repair (12B: `broken` rows, `repair_raw_preset`, schema 2 on write).
- `DONE` — the measured decode charge: `RawSourceWork.scratch` = **528 MiB + 82 B/px + 3 × the file** (518 MiB plus
  10 MiB for Develop Presence) (`RawSourceWork.swift`, record beside the constants; need = ⌈1.25 × max E⌉, measured
  by `scripts/raw-decode-probe`). `develop` renders through its own `CIContext`, cleared and dropped. A RAW opens
  on a 16 GB Mac (8 GiB) up to about 90 MP.
- `DONE` — the decode charge refit on seven real 24–61 MP CC0 camera files from raw.pixls.us (`--real`), beside the
  synthetic DNGs and the 6 MP NEF: a ≥ the largest file's whole cost per pixel (`a_floor`), F the least fixed term
  for it (`measure.py`'s `fit`). A 45 MP NEF is charged 4.45 GB (4.43 at the fit; was 8.04) and opens and prepares
  at 8 GiB (`--admit`). The local charge was refit the same way (below).
- `DONE` — the menu path's own terms (`RawLocalWork.swift`, record beside the constants): `restoreScratch` =
  22 MiB + 2·px + 3·encoded admits Layer ▸ Redevelop RAW…'s restore; `measuredScratch` = max(`RawSourceWork.scratch`,
  522 MiB + (73 + 13·k + 16·e)·px + 3·encoded), k = captured locals + creates (≤ 4), e = authored lens, grain or
  vignette; `RawLocalWork.request` charges max(`measuredScratch`, `replaceScratch`) (the Develop preview + 64 MiB
  display; MCP `edit_raw_development` / copy-local; enhanced `redevelop_raw`); decoder migration keeps
  `RawLocalWork.scratch` (decode + 56 B/px + 4 MiB). The preview drops a superseded candidate before the next
  admission; stages are sequential.
- `DONE` — the local charge refit on the seven real 24–61 MP camera files by the decode refit's rule
  (`measure.py`'s `fit_local`: the k = 0 rows through `fit()`, then the per-local and effects terms): the 6 MP
  NEF's 800 MB preview peak is a fixed term, not a 170 B/px slope. A 45 MP NEF's preview with four locals is
  charged 6.39 GB (was 9.89), so Layer ▸ Redevelop RAW… (restore, session, previews, publish) and
  `edit_raw_development` with one and with four locals pass at 8 GiB; the 61 MP file passes with one.
  `replaceScratch` = the core's replace plan + 32 MiB + 8·(k + 2)·px is the floor of the charge, because the
  request's budget is the job's scratch limit and the replace reserves its exact plan (it binds for a scaled layer
  or a float document). Core caps unchanged: a local mask to 64 MP, a RAW's file and masks within 512 MiB (four
  full-size masks to about 59 MP).
- `DONE` — the publish projection estimate adds `ProjectionMargin`'s 6/16 at 16 bits and 7/16 at 8.
