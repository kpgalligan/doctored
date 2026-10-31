# Phase 9I feasibility evidence and acquisition audit (ADR)

- **Status:** Accepted — closed (inventory locations superseded by [gate v2](phase-9i-gate-v2-status.md) decision 6)
- **Phase / dates:** 9I work package 1, 2026-09-13 (starting HEAD `19b7928`)
- **Original:** [archive/phase-9i-feasibility-evidence-audit.md](../archive/phase-9i-feasibility-evidence-audit.md) — inventory tables, manifest/report hashes, dataset survey, recovery commands
- **Key commits:** `195efe7`

## Context
Before any new comparison, work package 1 needed to know which fixtures, reports, binaries and runtimes survived,
which photographic evidence and permissions exist, and what source-only isolation a new run requires. No dataset,
checkpoint or package was acquired, no annotation invented, and no inference or scoring run.

## Decisions
- **Inventory tool:** [`scripts/phase9i_feasibility_inventory.py`](../scripts/phase9i_feasibility_inventory.py)
  (stdlib only) runs both fixture verifiers, hashes reports/model files, inspects PNG headers, and recomputes V3/V4
  gate decisions from serialized measurements. Its exit code only means "inventory written"; require both verifier
  exits 0, all pins matching, 90 unique cases per baseline and every gate comparison agreeing. Its
  `original90-manifest.json` is **scorer-only data** (truth-derived counts) and is ineligible as an annotation bundle.
- **Corpus facts (unchanged recipe):** 24 analytic (320×256; light/dark/ambiguous), 24 PhotoMatte crops (1280×1280 at
  `(512,384)`), full cat 763×764, bottle 800×600, Arduboy 944×1338, portraits 2304×3456; each with radius-3 feather and
  threshold-128 binary starts; all arithmetic in encoded RGB8. No reference PNG has ICC or `sRGB`; cat carries
  `gAMA`/`cHRM`, bottle `gAMA`. Do not apply new gamma/ICC transforms or label untagged data as a verified profile.
- **Memory headroom:** the largest image (7,962,624 px) uses `256·N + 64 MiB` = 2,105,540,608 B, leaving 40 MiB
  under 2 GiB — a planning reservation, not room for a model.
- **Measured binaries are not retained:** the V3/V4 executable path now holds a different build and four recorded
  sources changed; a reproduction must be a new identity compared by legacy parity.
- **Evidence and permission are separate prerequisites:** PhotoMatte85 permits local use (no redistribution, no
  training). Transparent-460, AM-2k, alphamatting.com, withoutBG100 and TOM-Net each lack established permission,
  independent foreground/transmission semantics, or both. The bottle (452 fractional vs 190,338 opaque pixels) is
  silhouette-only. A software license never implies dataset or weight rights; no forms, contacts or purchases.
- **Isolation requirements for any new run:** an exporter emits only source RGB, the permitted prior, geometry and
  declared color metadata; no truth plane, count, feasibility bound, filename map, metric or preview enters the
  bundle; resolve symlinks, allowlist types/metadata, and deny reference-directory/network access to annotator and
  inference worker. Agents that reviewed truth cannot author "fresh" guidance. Freeze guidance, replay bytes, input
  hashes and candidate identity before scoring. If shared scoring changes, replay legacy guided/sampled/closed-form parity.
- **Recovery:** refetch fixtures only with the pinned procedures (`scripts/phase6_reference_fixtures.py`,
  `scripts/phase6_full_reference_fixtures.py`, each then `--verify-only`) into a new directory; preserve mismatches,
  never refresh pins. Lost reports/binaries cannot be reconstructed from hashes — retain the historical record and
  create a separately identified reproduction.

## Consequences
- The `/private/tmp` locations inventoried here were purged; canonical locations are under `~/devel/doctored-fixtures/`
  (gate v2 decision 6). The script's `--temporary-root` default is still `/private/tmp`.
- The isolation rules above were implemented by the assisted workspace exporter ([phase-9i-assisted-workspace-status.md](phase-9i-assisted-workspace-status.md)).

## Open items (verified 2026-09-26)
- `DONE` — Source-only exporter with isolation audit (`scripts/phase9i_assisted_workspace.py`, `de70228`).
- `DONE` — Fixture recovery after purge: `~/devel/doctored-fixtures/phase6/{references,photos,full-references}` present (gate v2 decision 6).
- `DROPPED` — glass/transmission research ([gate v2](phase-9i-gate-v2-status.md)).
- `DROPPED` — Qualifying a new model runtime/environment for an established pipeline (gate v2 decision 5).
