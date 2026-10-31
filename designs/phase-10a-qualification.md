# Bounded PSD import qualification — 10A closeout (ADR)

- **Status:** Accepted — closed
- **Phase / dates:** 10A closeout, 2026-09-13 (baseline `1992d43`); Photoshop mask evidence added 2026-09-16
- **Original:** [archive/phase-10a-qualification.md](../archive/phase-10a-qualification.md) — full history, measurements, receipts
- **Key commits:** `352cb50` (closeout + two fidelity fixes)

## Context
The 10A slices — [folders](phase-10a-group-import.md), [raster rectangles](phase-10a-pixels-status.md),
[mask outside coverage](phase-10a-mask-foundation-status.md), [ordinary masks](phase-10a-masks-status.md)
and [relative masks](phase-10a-relative-masks-status.md) — were qualified only against synthetic fixtures.
This record closes 10A as a **defined import subset**, qualified against files authored and rendered by an
independent application (GIMP 3.2.6), with explicit exclusions rather than an open-ended "Photoshop
compatible" claim. This record owns 10A status; the slice ADRs are history.

## Decisions
- **Accepted import contract** (anything else refuses or is reported):
  - Container: classic PSD, 8-bit RGB or grayscale, Raw/PackBits channels, saved composite. Grayscale →
    equal RGB.
  - Color/resolution: exact supported embedded RGB ICC bytes and declared PPI; absent ICC = sRGB;
    invalid/unsupported profile or resolution refuses (a gray ICC refuses both choices).
  - Raster storage: full signed rectangles incl. off-canvas, straight RGB under zero alpha, partial alpha,
    absent-alpha opacity.
  - Folders: order, names, visibility, opacity, recognized blend keys, Normal isolation / Pass Through,
    disclosure, within ten retained levels.
  - Ordinary masks: one `-2` Raw/PackBits channel, 20-byte body, outside 0/255, enabled; the rectangle
    is canvas coordinates on rasters and folders, with the position-relative flag (bit 0) recorded and
    not applied, as Photoshop reads it (since 2026-10-11; the contract first normalized a
    raster-relative rectangle once). Imported masks are unlinked, density 1, feather 0.
  - Compatibility choice: report before editing; the user explicitly selects layers or the saved
    composite; **a layer failure never silently chooses the composite**. Observed excluded markers name the
    affected record; general limitations stay distinguishable from observed losses.
  - Editing/persistence: imports are editable through central Undo/Actions; native save/reopen keeps
    exact samples, properties, profile, PPI.
  - Admission: canvas, raw-plane, aggregate, record, metadata, nesting and the layered-decode envelope
    (`psd_layer_pixels::import_envelope_bytes()` = `process_working_limit()`, 2 GiB at the test pin)
    limits are enforced before expensive decode (not the initial file read).
- **Appearance gate** (fixed before measurement; never widen after seeing results): for finite layered RGB
  examples using Normal, Multiply, Screen and ordinary group/mask opacity, **max abs error ≤ 2 byte codes per
  RGBA component** vs the independent reference; alpha exactly 255 over opaque backdrops. Raw
  source/mask samples, geometry, metadata and native persistence have **exact** gates. Compare file RGB
  codes without display conversion.
- **Joint-zero-alpha convention:** appearance comparisons canonicalize RGB only where *both* rendered
  alphas are zero (GIMP keeps RGB there; Doctored's compositor emits 0). The production compositor was
  not changed; source/native samples keep exact gates.
- **Fix 1 — PSD profile retention:** explicit PSD imports (single-layer and saved-composite) keep their
  source ICC and samples; generic flat-image working-space adoption no longer applies to them
  (`app/Sources/ImageDocument.swift`).
- **Fix 2 — white-matted saved composite:** merged color is stored against white. On verified merged
  transparency only, decoded RGB = clamped nearest `255 * (stored + A - 255) / A` (half-up) for A > 0;
  alpha exact; opaque unchanged; zero-alpha RGB = 0 (`core/src/psd_container.rs`). Layer/mask channels
  stay straight. The saved preview is therefore compared against GIMP's *merged reader*, then over
  black/white against the layered reference, both at 2 codes.
- **Finite compatibility reporting** (`core/src/psd_feature_markers.rs`): the bounded walk reports a fixed
  set of style, adjustment/fill, type, smart/placed, vector and blending markers on verified identities;
  descriptor payloads are not evaluated. Reporting only — not editable support.
- Recognized blend keys are preserved, but no universal Photoshop appearance is promised beyond the
  qualified combinations.

## Rejected alternatives
- Rewriting GIMP's exported PSDs to fit the contract — the gray-ICC file stays a refusal case.
- Comparing white-matted plane bytes directly to straight RGB — different representations.
- Changing the compositor to emit GIMP's zero-alpha RGB — those codes contribute nothing on any backdrop.

## Consequences
- Corpus: `core/tests/fixtures/psd_10a/` (GIMP layered) and `psd_10a_merged/` (merged reader); provenance
  in [`scripts/psd-exchange-probe/README.md`](../scripts/psd-exchange-probe/README.md). App verifier
  `scripts/verify_phase10a_external.py` enforces the gate above; markers via
  `scripts/verify_phase10a_markers.py`.
- Doctored's PSD/native ICC preservation has **no** timestamp exception (only GIMP-regenerated
  reference ICCs may differ in bytes 24–35).
- Future mask/construct extensions need their own contract and independent evidence.

## Open items (verified 2026-09-26)
Exclusions assigned elsewhere or declared non-goals — none keeps 10A open:
- `DONE` — PSD export / reopen of Doctored-written PSD (10B): [phase-10b-export-status.md](phase-10b-export-status.md).
- `DONE` — Genuine 16-bit PSD (10C): [phase-10c-depth-status.md](phase-10c-depth-status.md).
- `DONE` — Styles, adjustments, group clipping, Blend If, fill opacity, knockout on import: 10C constructs slices (`psd_adjustment.rs`, `psd_layer_effects.rs`, `psd_knockout` tests; carryovers P7-20/P7-21 verified); native compositing 10D (P7-19).
- `DONE` — PSD layer locks (`lspf`) on import and export at both depths: `psd_locks.rs`, `psd_locks` tests.
- `DONE` — Photoshop-authored mask evidence: Run 3 (`d1b6c27`), see [relative-mask agreement](phase-10a-relative-masks-status.md#photoshop-appearance-agreement-run-3-2026-09-16).
- `DONE` — 8-bit ZIP channels and ZIP composites (`22ea5a9f`, `core/tests/psd_zip8.rs`).
- `DONE` — PSB read and write (`22ea5a9f`).
- `DROPPED` — Smart Object interchange: smart objects were removed 2026-10-10.
- `DROPPED` — Extended / vector masks, density/feather, rendered/inverted flags, PSD mask linkage; guides, ruler origin, saved auxiliary channels, capture metadata; >10 folder levels: explicit non-goals of this contract (plan §10A).
