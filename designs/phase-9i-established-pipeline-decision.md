# Phase 9I established matting pipeline decision (ADR)

- **Status:** Superseded by [phase-9i-gate-v2-status.md](phase-9i-gate-v2-status.md) (decision 5: RVM dropped, no pipeline selected)
- **Phase / dates:** 9I work package 1, 2026-09-13; closed 2026-09-15
- **Original:** [archive/phase-9i-established-pipeline-decision.md](../archive/phase-9i-established-pipeline-decision.md) — adapter policy, resource forecast table, source hashes, primary-source links
- **Key commits:** `195efe7` (decision), `d5c7490` (closing decision)

## Context
[The restart plan](phase-9i-next-steps.md) asked for at most three established complete alpha+foreground
pipelines to be reviewed and one selected for a single bounded comparison against the original 90 cases, after
five custom candidates failed. The [ViTMatte spike](phase-6-vitmatte-spike.md) had already failed (0/24 combined
analytic, 0/42 full admitted under 2 GiB), so repeating it was ruled out.

## Decisions
- **Closing decision (2026-09-15): RVM is dropped and no pipeline is selected. No neural matting model is added;
  Apple Vision stays the automatic prior.** *Why:* RVM issue #281 (exact pretrained-weight terms) is unanswered;
  its GPL-3.0 code cannot link into all-rights-reserved Doctored regardless of a weight grant; it is human-video
  trained and trimap-free, so it cannot consume brush guidance — the input the assisted protocol adds.
- Survey outcome that stands as history:
  - **RVM MobileNetv3 + deep guided filter** — RGB-only joint F/α; was the preferred *conditional* pipeline, pending weight rights.
  - **FBA Matting** — not selected: needs a trimap (none source-only exists; automatic endpoints as certain labels
    repeat the known input defect), author restricts Adobe-trained weights to noncommercial use, and ~11 GB GPU guidance.
  - **Background Matting V2** — not selected: needs an independently captured empty background, which the corpus lacks;
    a background reconstructed from reference planes would be contaminated input.
- The [acquisition/runtime proposal](phase-9i-established-pipeline-manifest.json) is explicitly **not a frozen
  candidate declaration**. No weights were downloaded, packages installed, model loaded or publisher contacted.
- Standing rules for any future external model (from the conditional RVM contract): no host double-normalization,
  no external resize/padding/profile transform, validate native shapes/finiteness/range before conversion,
  quantize with `floor(255x+0.5)`, one model child at a time within the 2 GiB harness with a combined forecast,
  planning estimates are not allocation bounds, and a software license never substitutes for weight/dataset rights.

## Rejected alternatives
- Repeating ViTMatte with changed normalization/caps or truth-derived labels — not a new experiment.
- A custom V5 successor in place of an established pipeline — outside this work package.

## Consequences
- Adding any neural matting model later is a new decision that must resolve code *and* weight rights first.
- **Superseded 2026-10-13:** the owner allows neural models; licence tiers and the survey are in [neural-models.md](neural-models.md).
- An RGB-only model's output cannot qualify the assisted contract (no label/protection/local-write interface).

## Open items (verified 2026-09-26)
- `DROPPED` — Resolve RVM checkpoint terms, acquire/pin the TorchScript export, build the joint-output adapter and run the 90-case comparison ([phase-9i-gate-v2-status.md](phase-9i-gate-v2-status.md) decision 5).
- `DROPPED` — New complete 90-case source-only export for the automatic comparison (same).
- `TRACKED` — Accepted blinded human guidance → P7-12 / plan §9I assisted run.
- `DROPPED` — Glass/transmission research: out of scope (dropped 2026-09-28); the six analytic glass cases stay reported diagnostics under frozen gate v2 and the bottle stays silhouette-only.
