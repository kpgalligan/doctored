# Full-image matting reference protocol (ADR)

- **Status:** Accepted — closed (protocol and corpus in use; glass truth never obtained)
- **Phase / dates:** 6C, 2026-09-10 (corpus paths moved 2026-09-15)
- **Original:** [archive/phase-6-full-image-reference-protocol.md](../archive/phase-6-full-image-reference-protocol.md) — full history, measurements, receipts
- **Key commits:** 0155d41, d5c7490

## Context
The [frozen reference protocol](phase-6-matting-reference-protocol.md) only measured 1280×1280 portrait crops, so it
said nothing about fur, products or full-frame runtime. This extension adds seven complete photographic cutouts,
evaluated at full native size with no crop or resampling. It does not replace the 24-case analytic gate, change the
crops, or reinterpret earlier results. Source selection and the memory envelope were fixed before measuring.

## Decisions
- **References (all outside the repo, with `ATTRIBUTION.md` beside every fixture/output):** British Longhair fur cutout
  (763×764, CC BY-SA 4.0), Colorful bottle (800×600, attribution-only; **silhouette only, not transparent-interior or
  refractive truth**), Arduboy product (944×1338, CC0), and the four pinned PhotoMatte85 portraits at full 2304×3456.
  Supplied alpha/colors are annotations, not physical truth; existing annotation defects are kept and disclosed.
- **Refused sources:** Transparent-460 (noncommercial, terms-gated), AM-2k (release agreement; image/alpha pairs cannot
  supply independent foreground RGB), TOM-Net (attenuation/refraction targets, not scalar alpha). No contact, account
  or agreement was made. *Why:* rights must be clear and the target must match the benchmark's model.
- **Acquisition:** `../scripts/phase6_full_reference_fixtures.py` fetches only three pinned PNGs plus immutable
  description revisions and copies the PhotoMatte files from the verified acquisition; every asset is length+SHA-256
  checked before exclusive publication; mismatching files are preserved and refused; the manifest pins dimensions,
  RGBA8 storage, interlacing, revision and limitation (the bottle cannot be relabeled as transparency truth).
  Checks: `../scripts/test_phase6_full_reference_fixtures.py`.
- **Cases:** 42 = 7 references × light/dark/checker × blurred/binary starts. Encoded-RGB compositing and metrics.
  Automatic trimap from the degraded prior only; kernels never receive reference foreground/alpha. Oracle-alpha color is
  a separate diagnostic (duplicated across start labels — 21 distinct scenes, not 42).
- **Shared solver seam:** `Candidate` receives source RGB, dimensions, categorical trimap and degraded prior; returns
  alpha, metadata, optional confidence (`Candidate` / `AlphaResult` in `../core/examples/phase6_matting_quality.rs`,
  consumed by `../core/examples/phase6_full_image_quality.rs`; 9I later added an `InputPolicy` field). Non-default solvers report
  under `alpha_candidate` / `estimated_alpha_color` so they are never mislabeled as sampled. Refusal = failure.
- **Admission:** before decoding, reserve `256 × full_pixels + 64 MiB` against 2 GiB; `--working-limit-mib` may lower,
  never raise it. An image over the envelope records all six cases as refused with original dimensions — **no smaller
  crop or preview substitutes**. Kernel work limits are additional refusals.
- **Artifacts:** each source's light/blurred case exports native-size planes and composites; 512-px panels are
  inspection only. Runner: `../scripts/phase6_full_image_qualification.py` (new output dir outside the repo;
  `--require-pass` fails on any gate failure while keeping reports).

## Consequences
- Sampled result: alpha 15/42, oracle color 30/42, coupled/end-to-end 0/42; constraint audit shows 6/42 cannot pass
  under the frozen labels. Closed-form also 15/42 alpha, 0/42 end-to-end.
- The full 42 stay whole in the gate v2 automatic report; six non-bottle images contribute two checker cases each to
  the assisted 36-case completion subset; the bottle is excluded
  ([phase-9i-gate-v2-status.md](phase-9i-gate-v2-status.md)).
- The corpus now lives in `~/devel/doctored-fixtures/phase6/full-references`; `/tmp` receipts cited in the original
  were purged.

## Open items (verified 2026-09-26)
- `DROPPED` — Photographic physical-translucency reference: never obtained; glass moved out of the pass rule to a
  reported-only glass diagnostic (transmission research declined) (gate v2 decision 1, [archive/phase-9i-gate-v2-status.md](../archive/phase-9i-gate-v2-status.md)).
- `TRACKED` — Qualifying any candidate on these 42 → carryover P7-12 / 9I gate v2.
