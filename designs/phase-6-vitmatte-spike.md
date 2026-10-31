# ViTMatte offline qualification spike (ADR)

- **Status:** Accepted — closed (learned matting not adopted; superseded for 9I by gate v2 decision 5)
- **Phase / dates:** 6C, 2026-09-10
- **Original:** [archive/phase-6-vitmatte-spike.md](../archive/phase-6-vitmatte-spike.md) — full history, measurements, receipts
- **Key commits:** 0155d41

## Context
[Foreground evidence](phase-6-foreground-evidence.md) concluded that color-only correction cannot resolve the coupled
cutout failure and named ViTMatte (image + trimap, MIT code) as a stronger alpha prior worth measuring. This spike ran
a pinned public checkpoint offline against the frozen corpus to see whether a learned prior changes the picture. It adds
no model, Python runtime, dependency, control or inference path to the app.

## Decisions
- **Checkpoint:** `hustvl/vitmatte-small-composition-1k` safetensors at revision
  `6a58ad7646403c1df626fbd746900aec7361ea1d` (Apache-2.0 card; `model.safetensors` SHA-256
  `bda9289d…d788cb`, 103,294,572 bytes). The original pickle checkpoint is never downloaded; no Composition-1k data.
- **Two declared normalization variants:** `packaged` (HF processor mean/std 0.5) and `author` (ImageNet constants from
  the author's model code). *Why:* the HF converter only renames keys and does not fold normalization into weights; the
  second variant follows published code, not observed scores. Both results retained.
- **Frozen inference policy:** CPU float32, 4 intra-op threads, one case per subprocess; only source RGB + categorical
  trimap enter the model; pad bottom/right to ×32 and crop back, never resize; quantize `floor(clamp(α)·255+0.5)`;
  then enforce known trimap `0`/`255` exactly, unknown keeps network output. Offline flags, local weights, no remote code.
- **Resource guard:** 120 s wall limit and a monitored 2 GiB RSS rejection threshold per child (plus post-exit
  `ru_maxrss`), with preflight `1 GiB + 256·padded_pixels + 6·attention_matrix_bytes`. The monitor is a termination
  threshold, not an OS ceiling. Refused outputs are deleted; **no resized or tiled substitute** counts as full-image.
- **Import hardening** (`../core/examples/phase6_external_matte_quality.rs`, tests `../core/tests/external_matte_inputs.rs`):
  header-checked PNGs (RGB8 source; L8 prior/trimap/alpha), exact native geometry, decoder allocation allowance
  `16·pixels + 1 MiB`, changed inputs and known-label violations refused. Scripts: `../scripts/phase6_vitmatte_spike.py`,
  `../scripts/test_phase6_vitmatte_spike.py`.

## Consequences
- Analytic 24: alpha 8/24 (packaged), 9/24 (author); **0 end-to-end** either way; peak RSS ~919 MiB.
- Full-image 42: **no admitted output** — 30 preflight refusals, 12 crossed 2 GiB RSS (peaks 2.29–2.85 GB). Global
  attention (~983 MB per matrix at 1280²) makes this model unfit for native photographic sizes as run.
- Gains on some blurred inputs (portrait light boundary MAD 0.142 → 0.018) coexist with severe regressions (ambiguous
  product binary 0.0017 → 0.404); a learned prior does not guarantee opaque-color preservation.

## Rejected alternatives
- Increasing the memory allowance or tiling/resizing after the fact — would be a new, separately declared experiment.

## Open items (verified 2026-09-26)
- `DROPPED` — MPS / Core ML deployment, a new resource policy, or any neural matting integration: gate v2 decision 5
  ("no neural matting model is added; Apple Vision stays the automatic prior"),
  [archive/phase-9i-gate-v2-status.md](../archive/phase-9i-gate-v2-status.md).
- `DROPPED` (2026-10-30) — Better automatic input policy / manual evidence: the gate is not a ship condition
  (2026-10-10) and gate v2's assisted run is dropped.
