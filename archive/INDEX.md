# Archive index

Historical design and status documents, moved here on 2026-09-26. Each was condensed into an ADR at the same basename in [`designs/`](../designs/); read the ADR first. Come here only when asked, or when you need the full reasoning, measurements, receipts or run logs behind an earlier phase's decision. Their verified open items are GitHub issues, linked from each ADR's "Open items" section.

A record that grew again after 2026-09-26 and was condensed a second time has its second original under [`2026-10/`](2026-10/), at the same basename: the doc as it stood on 2026-10-07, with the measurement tables and issue records added since September. The first original stays where it was. Two records condensed a third time on 2026-10-30 have that original under [`2026-10-30/`](2026-10-30/). They are listed in the last section.

Links inside these files are historical: sibling links point at other archived originals, and code line numbers may have drifted.

## Roadmaps and gap catalogs

- [gimp-image-manipulation-capabilities.md](gimp-image-manipulation-capabilities.md) — GIMP 3.3 architecture study (Aug 2026) — lessons adopted/rejected and a 14-area capability catalog with verified shipped status. ([ADR](../designs/gimp-image-manipulation-capabilities.md))
- [high-leverage-roadmap.md](high-leverage-roadmap.md) — Four GIMP-derived features (selection algebra, layer masks, meta-based text layers, unified transform), all shipped 2026-08-10/11; rewrite-scale deferrals later taken up by the plan. ([ADR](../designs/high-leverage-roadmap.md))
- [next-features.md](next-features.md) — Feature survey choosing the meta-parametric-layer direction; suggested phase and second wave shipped. ADR carries a verified 20-row catalog. ([ADR](../designs/next-features.md))
- [photo-editor-gap-review-2026-09.md](photo-editor-gap-review-2026-09.md) — September re-review (F01–F16, P0–P3) reprioritising around integrity, precision, sources and scale; became the plan's Phases 1–13. Mostly done. ([ADR](../designs/photo-editor-gap-review-2026-09.md))
- [photo-editor-gap-review.md](photo-editor-gap-review.md) — First gap catalog, driving Phases 1–8 (styles, symbolic transforms, channels, ICC/metadata, adjustments, healing/CAF, groups/locks, guides, RAW, Actions) — all shipped; the §3 breadth catalog is partly open. ([ADR](../designs/photo-editor-gap-review.md))
- [professional-photo-editor-implementation-plan.md](professional-photo-editor-implementation-plan.md) — The thirteen-phase plan implementing the September gap review, with every slice narrative, completion gate, boundary, the follow-on tracks table and the original execution sections, to 2026-10-30 (Phases 1–12 shipped, 13 partly open). ([ADR](../designs/professional-photo-editor-implementation-plan.md))

## Phases 1–5 — integrity, selection, colour/metadata, jobs, recovery

- [phase-1-integrity-status.md](phase-1-integrity-status.md) — Phase 1A–1D (2026-09-09) — native capacity preflight, preservation-checked Save routing, render failure as an explicit state, premultiplied sampling. ([ADR](../designs/phase-1-integrity-status.md))
- [phase-2-selection-status.md](phase-2-selection-status.md) — Phase 2A–2D (2026-09-09) — OperationScope target policy, coverage-confined destructive edits, transient selection undo, Quick Mask replay from recorded coverage. ([ADR](../designs/phase-2-selection-status.md))
- [phase-3-color-metadata-status.md](phase-3-color-metadata-status.md) — Phase 3A–3D (2026-09-09) — typed TIFF writer, bounded EXIF relocation, ImageIO carrier metadata, `document_v2` calibrated evaluation, PSD compatibility notice with source fingerprint. ([ADR](../designs/phase-3-color-metadata-status.md))
- [phase-4-jobs-resource-status.md](phase-4-jobs-resource-status.md) — Phase 4A–4D (2026-09-09) — serial worker seam, stale-result refusal, `RzJobControl` cancellation, deduplicated memory accounting/admission, measured save/RAW/transform migration. ([ADR](../designs/phase-4-jobs-resource-status.md))
- [phase-4-rendering-contract.md](phase-4-rendering-contract.md) — Phase 4C/4D kernel contract — `RenderRequest` region/divisor semantics, 4096-pixel cancellation cadence, halo and full-frame-fallback rules, 4D migration measurements. ([ADR](../designs/phase-4-rendering-contract.md))
- [phase-5-recovery-history-status.md](phase-5-recovery-history-status.md) — Phase 5A–5D (2026-09-09) — own autosave coordinator, two-generation atomic recovery, enumerable budgeted history, named snapshots / `.rzsnapshot`. ([ADR](../designs/phase-5-recovery-history-status.md))
- [working-limit-ram-scaled.md](working-limit-ram-scaled.md) — The RAM-scaled working limit (2026-10-04): curve, precedence, test pin, the batches 163–168 and 45 MP filter fit record at 2 GiB, 2,862,612,480 and 8 GiB, ceilings. ([ADR](../designs/working-limit-ram-scaled.md))

## Phase 6 — selection, masks and matting

- [phase-6-closed-form-matting-candidate.md](phase-6-closed-form-matting-candidate.md) — Closed-form (Levin) matting-Laplacian alpha solver, experimental; 10/24 analytic, 9/24 crops, 15/42 full, 0 joint alpha/color passes. Objective reused by the 9I tiled kernel. ([ADR](../designs/phase-6-closed-form-matting-candidate.md))
- [phase-6-cutout-output-contract.md](phase-6-cutout-output-contract.md) — Internal `rz_doc_add_refinement_cutout` — opaque foreground layer + linked mask, hides old roots, one Undo, cancellable. Not exposed; gate open. ([ADR](../designs/phase-6-cutout-output-contract.md))
- [phase-6-foreground-evidence.md](phase-6-foreground-evidence.md) — Shows opaque vs translucent foreground is underdetermined from RGB+prior+labels; feasibility-projection correction measured useless, not integrated. ([ADR](../designs/phase-6-foreground-evidence.md))
- [phase-6-full-image-reference-protocol.md](phase-6-full-image-reference-protocol.md) — Seven full-size photographic references (42 cases, 2 GiB admission envelope); sampled candidate 15/42 alpha, 0/42 end-to-end. ([ADR](../designs/phase-6-full-image-reference-protocol.md))
- [phase-6-matting-reference-protocol.md](phase-6-matting-reference-protocol.md) — Freezes PhotoMatte85 crop references, automatic trimap and gate-v1 color thresholds (still binding under gate v2); first run 0/24 end-to-end. ([ADR](../designs/phase-6-matting-reference-protocol.md))
- [phase-6-sampled-matting-candidate.md](phase-6-sampled-matting-candidate.md) — Trimap colour-sampling (He et al.) solver, bounded 256-pair search with evidence refusal; 10/24 analytic, 8/24 crops, 0 joint. ([ADR](../designs/phase-6-sampled-matting-candidate.md))
- [phase-6-selection-mask-status.md](phase-6-selection-mask-status.md) — Phase 6 parent status — 6A essentials, 6B independent masks (native v10), 6C workspace, 6D assisted selection shipped; advanced cutout gate open under 9I/P7-12. ([ADR](../designs/phase-6-selection-mask-status.md))
- [phase-6-selection-quality.md](phase-6-selection-quality.md) — Contracts for Color Range, Quick Selection, selection transform, refine and foreground-estimate FFI; guided refine-edge candidate failed its 24-case gate (12/24), unexposed. ([ADR](../designs/phase-6-selection-quality.md))
- [phase-6-vitmatte-spike.md](phase-6-vitmatte-spike.md) — Offline ViTMatte-S spike (pinned checkpoint); 8–9/24 analytic, 0 end-to-end; 0/42 full images admitted. Nothing shipped. ([ADR](../designs/phase-6-vitmatte-spike.md))

## Phase 7 — high-depth (16-bit) documents

- [phase-7-carryovers.md](phase-7-carryovers.md) — Carryover ledger P7-01–P7-39 — each 7A–7E gap's receiving slice and close criterion, with every row's verification evidence; closed 2026-10-30, open rows filed as issues. ([ADR](../designs/phase-7-carryovers.md))
- [phase-7-conversion-readout.md](phase-7-conversion-readout.md) — 7E contract — flattened ×257 promotion/downconversion, one nearest/Bayer quantizer with undithered alpha, captured-projection PNG/TIFF16 export, u16 readouts (12H addendum). ([ADR](../designs/phase-7-conversion-readout.md))
- [phase-7-geometry-paint-status.md](phase-7-geometry-paint-status.md) — 7D batch outcome — masks/channels, geometry, paint/clone, artwork, healing on the typed document; 21/21 native 24 MP stages. ([ADR](../designs/phase-7-geometry-paint-status.md))
- [phase-7-geometry.md](phase-7-geometry.md) — 7D typed geometry contract — `GeometryEdit` requests (extended by 12F), mask-link rules, exact vs filtered sampling, f64 weights, plan-before-allocate. Closed. ([ADR](../designs/phase-7-geometry.md))
- [phase-7-high-depth-decision.md](phase-7-high-depth-decision.md) — 7A RGBA16 vs RGBA32F prototype — chose u16 storage with f32 processing, 2e-6 error gates, 2 GiB tracked limit; 48 MP grouped scenes refused. ([ADR](../designs/phase-7-high-depth-decision.md))
- [phase-7-high-depth-status.md](phase-7-high-depth-status.md) — Phase 7 hub — 7A–7E slice table, contract locations, shared invariants, 24 MP results; gaps moved to the carryover ledger. ([ADR](../designs/phase-7-high-depth-status.md))
- [phase-7-neutral-path.md](phase-7-neutral-path.md) — 7B preserving workflow — separate `PrecisionDocument`/`RzPrecision*` owners, genuine PNG/TIFF16 import (no ImageIO fallback), pass-through groups, native v11, captured saves/archives. ([ADR](../designs/phase-7-neutral-path.md))
- [phase-7-operation-support.md](phase-7-operation-support.md) — Operation × target × depth support matrix; the 7A RGBA8 boundary audit became the public 16-bit/float gate. ADR keeps a checked compact matrix. ([ADR](../designs/phase-7-operation-support.md))
- [phase-7-pixel-editing.md](phase-7-pixel-editing.md) — 7D contract — u16 masks/channels, continuous-colour paint/erase/clone, TextKit/shape coverage (256 levels), membrane healing (1/16-code gate), three typed edit FFI exports, native v13. ([ADR](../designs/phase-7-pixel-editing.md))
- [phase-7-public-mode-status.md](phase-7-public-mode-status.md) — 7E public 16-bit mode — one owner in existing history/recovery, explicit flattened conversion, one capability gate, captured-projection export, 9/9 public 24 MP stages. ([ADR](../designs/phase-7-public-mode-status.md))
- [phase-7-tone-color.md](phase-7-tone-color.md) — 7C — all 21 adjustments on shared kernels with required `precision_v1` tag, continuous Curves/ICC encoder, profile assign/convert semantics, native v12. ([ADR](../designs/phase-7-tone-color.md))

## Phase 8 — retained sources

- [phase-8-retained-source-status.md](phase-8-retained-source-status.md) — 8A–8D retained sources (embedded, linked, retained RAW, Develop) — immutable masters, native v14–v19, reviewed decoder migration. Closed P7-01/02/35. ([ADR](../designs/phase-8-retained-source-status.md))
- [phase-8e-artwork-status.md](phase-8e-artwork-status.md) — 8E retained text/shape artwork at both depths — description v3, native area coverage, typed native v20 `ADSC`, fail-closed metadata. Verified P7-04/P7-05. ([ADR](../designs/phase-8e-artwork-status.md))
- [phase-8f-host-source-status.md](phase-8f-host-source-status.md) — 8F retained host stills and video frames — exact encoded originals, actual decoder samples, rational frame identity, native v21/v22 `RBIN` pool. Verified P7-03; ProRes cross-decoder unqualified. ([ADR](../designs/phase-8f-host-source-status.md))
- [phase-8g-raw-enhancements-status.md](phase-8g-raw-enhancements-status.md) — 8G camera-relative controls and up to four local RAW developments with u16 `RZLM` source-coordinate masks, recipe 3, frozen numeric gates. Completed Phase 8. ([ADR](../designs/phase-8g-raw-enhancements-status.md))
- [phase-8g-raw-enhancements-shipped.md](phase-8g-raw-enhancements-shipped.md) — 8G's shipped companion as of 2026-10-30 — the decisions in force for locals, range/generated/Sky masks, the 32-bit float development, the Develop Filter and its masks, Presence, local tone, syncing and bounds; condensed into the ADR. ([ADR](../designs/phase-8g-raw-enhancements-status.md))

## Phase 9 — live filters, retouch, selections, paint

- [phase-9-live-filters-status.md](phase-9-live-filters-status.md) — 9A–9D live filter stack (five operators, per-filter masks, native v23/v24 `LFLT`, caches, destructive twins) and native/transformed retouch; 9E–9I summarized. ([ADR](../designs/phase-9-live-filters-status.md))
- [phase-9d-synthesis-qualification.md](phase-9d-synthesis-qualification.md) — Qualification of 16-bit Spot Heal synthesis (typed f32 PatchMatch/membrane, byte path bit-identical, photographic small-spot gates); large periodic holes not claimed. ([ADR](../designs/phase-9d-synthesis-qualification.md))
- [phase-9e-content-aware-status.md](phase-9e-content-aware-status.md) — Content-Aware Fill at both depths — source_mask v1 eligibility recipe, new-layer output, preview/apply ticket MCP flow, native Patch. Verifies P7-18. ([ADR](../designs/phase-9e-content-aware-status.md))
- [phase-9f-selected-tone-status.md](phase-9f-selected-tone-status.md) — Selection-limited native tone/filters, `analysis_scope` for Auto commands, fine Levels/Threshold/Curves entry, schema-2 scoped histogram/sample statistics. Verifies P7-06/P7-07. ([ADR](../designs/phase-9f-selected-tone-status.md))
- [phase-9g-selections-masks-status.md](phase-9g-selections-masks-status.md) — Authoritative u16 selection/Quick Mask, scalar plane/mask/channel editing, native source-derived selections, auxiliary mattes, editing-state v3, native v25. Verifies P7-08–P7-11. ([ADR](../designs/phase-9g-selections-masks-status.md))
- [phase-9h-paint-fill-status.md](phase-9h-paint-fill-status.md) — Native pressure/airbrush strokes with RZBR v1 canonical dabs, 27 paint modes, eraser, bucket, gradients, dodge/burn; 50,000-dab / 256M-work caps. Verifies P7-13/P7-14. ([ADR](../designs/phase-9h-paint-fill-status.md))

## Phase 9I — cutout quality gate

- [phase-9i-assisted-annotation-runbook.md](phase-9i-assisted-annotation-runbook.md) — Operational runbook for the blinded 36-case human annotation session and scoring (build, export, annotate, score, close out); session deferred — ADR kept operationally complete. ([ADR](../designs/phase-9i-assisted-annotation-runbook.md))
- [phase-9i-assisted-protocol-v1.md](phase-9i-assisted-protocol-v1.md) — Assisted edge-refinement protocol contract — inputs/outputs, guidance planes, blinding, budget, tooling/scoring pins, pass rule. Tooling built; no human data yet. **Live:** the pinned `protocol_sha256` input for runs; amended in place (gate v2). ([ADR](../designs/phase-9i-assisted-protocol-v1.md))
- [phase-9i-assisted-scoring-status.md](phase-9i-assisted-scoring-status.md) — Scorer for assisted replays — one frozen tiled soft-prior kernel with human labels as hard constraints, refusals in the denominator. Synthetic dry run only. ([ADR](../designs/phase-9i-assisted-scoring-status.md))
- [phase-9i-assisted-workspace-status.md](phase-9i-assisted-workspace-status.md) — Source-only anonymous workspace exporter, replay format and gated blinded annotation window for protocol v1. Built and drive-verified. ([ADR](../designs/phase-9i-assisted-workspace-status.md))
- [phase-9i-bayesian-v5-status.md](phase-9i-bayesian-v5-status.md) — Joint Bayesian alpha/foreground candidate V5 — failed (2/90 combined passes, damaged interiors, 24 budget refusals); kept as a diagnostic. ([ADR](../designs/phase-9i-bayesian-v5-status.md))
- [phase-9i-cutout-status.md](phase-9i-cutout-status.md) — The five predeclared automatic cutout candidates (V1–V5) and the internal RGBA16 + u16-mask cutout foundation; none pass, advanced controls stay disabled. ([ADR](../designs/phase-9i-cutout-status.md))
- [phase-9i-established-pipeline-decision.md](phase-9i-established-pipeline-decision.md) — Review of RVM, FBA and BGMv2 as an established matting pipeline; RVM preferred conditionally, then dropped by gate v2. ([ADR](../designs/phase-9i-established-pipeline-decision.md))
- [phase-9i-established-pipeline-manifest.json](phase-9i-established-pipeline-manifest.json) — RVM acquisition/runtime proposal for the established-pipeline review; never a frozen candidate, unused since gate v2 dropped RVM. ([ADR](../designs/phase-9i-established-pipeline-decision.md))
- [phase-9i-execution-status.md](phase-9i-execution-status.md) — Cancellable cutout preparation — compound single-use job control, owner-close forwarding, chunked byte copy/render, borrowed-matte accounting. Closed. ([ADR](../designs/phase-9i-execution-status.md))
- [phase-9i-feasibility-evidence-audit.md](phase-9i-feasibility-evidence-audit.md) — Audit of preserved fixtures, reports, binaries, dataset permissions and source-only isolation; `/private/tmp` locations later purged and replaced. ([ADR](../designs/phase-9i-feasibility-evidence-audit.md))
- [phase-9i-feasibility-status.md](phase-9i-feasibility-status.md) — Work-package-1 prerequisite decision — stop at missing rights/evidence. Superseded by gate v2. ([ADR](../designs/phase-9i-feasibility-status.md))
- [phase-9i-gate-v2-status.md](phase-9i-gate-v2-status.md) — One-time rescope of the cutout gate — 12-case analytic pass rule, bounded opaque rule, 36-case assisted subset, RVM dropped, V1–V5 re-scored (none pass). ([ADR](../designs/phase-9i-gate-v2-status.md))
- [phase-9i-next-steps.md](phase-9i-next-steps.md) — Restart plan after automatic failures — assisted-refinement contract, separate protocols, work packages, decision table; automatic line stopped. ([ADR](../designs/phase-9i-next-steps.md))
- [phase-9i-typed-cutouts-status.md](phase-9i-typed-cutouts-status.md) — Typed half of P7-12 — basic Refine Selection natively at u16 with a reusable canvas-sized u16 mask. Verified; gate half open. ([ADR](../designs/phase-9i-typed-cutouts-status.md))

## Phase 10 — PSD exchange and typed blending

- [phase-10a-group-import.md](phase-10a-group-import.md) — 8-bit PSD folder opacity/visibility/blend/disclosure kept on import via one bounded record walk (`psd_group_records`) ahead of the `psd` crate. Closed. ([ADR](../designs/phase-10a-group-import.md))
- [phase-10a-mask-foundation-status.md](phase-10a-mask-foundation-status.md) — Mask outside coverage 0/255 in byte and 16-bit mask models, feather/cache semantics, conditional native RZDC28/29 `MOUT` wrapper. Closed. ([ADR](../designs/phase-10a-mask-foundation-status.md))
- [phase-10a-masks-status.md](phase-10a-masks-status.md) — Ordinary absolute 8-bit PSD raster and folder masks imported unlinked; unsupported forms are reported losses, malformed data refuses. Closed. ([ADR](../designs/phase-10a-masks-status.md))
- [phase-10a-pixels-status.md](phase-10a-pixels-status.md) — Full signed PSD raster rectangles with exact straight RGBA8, metadata-only crate view, working-limit `peak_forecast` admission. Closed. ([ADR](../designs/phase-10a-pixels-status.md))
- [phase-10a-qualification.md](phase-10a-qualification.md) — 10A closeout — bounded PSD import contract qualified against a GIMP 3.2.6 corpus, 2-code appearance gate, profile-retention and white-matte fixes. Closed. ([ADR](../designs/phase-10a-qualification.md))
- [phase-10a-relative-masks-status.md](phase-10a-relative-masks-status.md) — Raster-relative PSD masks normalized from the owning raster's origin; Photoshop Run 3 shows Photoshop ignores bit 0 — divergence recorded. ([ADR](../designs/phase-10a-relative-masks-status.md))
- [phase-10b-export-status.md](phase-10b-export-status.md) — 8-bit classic PSD writer (layers or flattened composite) under "never silent" preflight, qualified by GIMP 3.2.6 reopen under five gates. Closed. ([ADR](../designs/phase-10b-export-status.md))
- [phase-10c-constructs-status.md](phase-10c-constructs-status.md) — 10C-2 per-construct PSD exchange rules (adjustments, lfx2/iOpa/Blend If, clipping, omissions) both directions at both depths; closed 2026-09-16, its agreement with Photoshop measured. ([ADR](../designs/phase-10c-constructs-status.md))
- [phase-10c-constructs-shipped.md](phase-10c-constructs-shipped.md) — 10C-2's shipped companion as of 2026-10-30 — Pattern Overlay, reference point, Stroke Fill Type, Contour, Noise, Texture, gradient glows, Styles panel and `.asl`, Levels/Curves carry and the measured effect-kernel and gradient members, each with its take record. ([ADR](../designs/phase-10c-constructs-status.md))
- [phase-10c-depth-status.md](phase-10c-depth-status.md) — 16-bit RGB PSD import/export following document depth; `Lr16` tail blocks, ZIP decode, Photoshop-authored 16-bit fixture. ([ADR](../designs/phase-10c-depth-status.md))
- [phase-10d-adjustment-blending-status.md](phase-10d-adjustment-blending-status.md) — 26 non-Normal blend modes on typed 16-bit adjustment layers via the byte composite_adjustment_into rule; native v37 / LBLD schema 7, 16-bit PSD keys; verified P7-19. ([ADR](../designs/phase-10d-adjustment-blending-status.md))
- [phase-10d-basic-blending.md](phase-10d-basic-blending.md) — First native blend slice — Normal/Multiply/Screen + Pass Through, `layer_blend` v1 edit, atomic compound properties, conditional RZDC26 / LBLD schema 1. ([ADR](../designs/phase-10d-basic-blending.md))
- [phase-10d-blend-if-status.md](phase-10d-blend-if-status.md) — Typed per-layer Blend If on 16-bit codes (knee 257·k), band policy, native v35 / LBIF, clip-unit footprint rules; agreement with Photoshop measured (red exact, gray differs). ([ADR](../designs/phase-10d-blend-if-status.md))
- [phase-10d-clipping-status.md](phase-10d-clipping-status.md) — Typed clipping chains on every 16-bit record kind via the byte clip-group rule; native v32 / LCLP, shared UI/MCP/Actions path, 16-bit PSD both directions. Closed. ([ADR](../designs/phase-10d-clipping-status.md))
- [phase-10d-five-modes-status.md](phase-10d-five-modes-status.md) — Subtract, Vivid Light, Linear Light, Pin Light, Divide (v31 / schema 4, 1.6-code Vivid bound); Hard Mix refused here, admitted later by the threshold slice. ([ADR](../designs/phase-10d-five-modes-status.md))
- [phase-10d-four-modes-status.md](phase-10d-four-modes-status.md) — Color Dodge, Color Burn, Linear Dodge (Add), Linear Burn (v30 / schema 3) with the component-classified 0.65/1.6-code numerical policy. ([ADR](../designs/phase-10d-four-modes-status.md))
- [phase-10d-hsl-modes-status.md](phase-10d-hsl-modes-status.md) — Hue, Saturation, Color, Luminosity via the unchanged f32 NonSeparable kernel (v33 / schema 5, 16-bit PSD keys), SetSat near-gray policy `Δ ≥ 6830·s`. ([ADR](../designs/phase-10d-hsl-modes-status.md))
- [phase-10d-layer-styles-status.md](phase-10d-layer-styles-status.md) — Typed fill opacity and nine effects under shape-quantized byte-plane policy (≤129 codes vs byte), native v36 / LSTY, 100 MP plane gate; verified P7-20. ([ADR](../designs/phase-10d-layer-styles-status.md))
- [phase-10d-separable-status.md](phase-10d-separable-status.md) — Overlay, Soft Light, Hard Light, Darken, Lighten, Difference, Exclusion via W3C kernels; conditional RZDC27 / LBLD schema 2 (modes 1–9). Closed. ([ADR](../designs/phase-10d-separable-status.md))
- [phase-10d-threshold-modes-status.md](phase-10d-threshold-modes-status.md) — Hard Mix, Darker/Lighter Color, Dissolve (v34 / schema 6) under the versioned strong/weak band policy, documented Dissolve hash, 16-bit PSD keys; verifier + GIMP R6 receipt taken. ([ADR](../designs/phase-10d-threshold-modes-status.md))

## Phase 11 — output, colour, formats, linear light, float

- [phase-11a-export-workspace-status.md](phase-11a-export-workspace-status.md) — 11A export workspace — one pure resample→sharpen→convert→reduce→encode chain, preview bytes = written bytes, presets/naming, Quick Export. Closed. ([ADR](../designs/phase-11a-export-workspace-status.md))
- [phase-11b-production-outputs-status.md](phase-11b-production-outputs-status.md) — 11B production outputs — isolated layer/group exports, contact sheets, metadata-policy request key, preset-driven Batch delivery. Shipped. ([ADR](../designs/phase-11b-production-outputs-status.md))
- [phase-11c-output-color-status.md](phase-11c-output-color-status.md) — 11C output colour — LUT ICC engine, four intents + BPC, soft proof and gamut warning, preserve-appearance conversion (closes P7-22). T3 soft proof vs Adobe ACE is a recorded miss. ([ADR](../designs/phase-11c-output-color-status.md))
- [phase-11d-format-additions-status.md](phase-11d-format-additions-status.md) — 11D formats — host-encoded HEIC, Gray16→sRGB, typed packet/PPI bridge, float-TIFF seam, PDF/SVG import, PDF export, AVIF/JP2 in the 8F matrix (closes P7-23). ([ADR](../designs/phase-11d-format-additions-status.md))
- [phase-11e-linear-light-status.md](phase-11e-linear-light-status.md) — 11E phase 1 of P7-24 — per-document encoded/linear_light policy refused by name at switch/edit/render, native wrapper 38/39 `EVAL`, archive v4. ([ADR](../designs/phase-11e-linear-light-status.md))
- [phase-11f-hdr-float-status.md](phase-11f-hdr-float-status.md) — 11F 32-bit float document — linear-light RGBA32, finite-at-import rule, tone maps, native v40 `FLOT`, exact float TIFF I/O, bounded operator set (closes P7-25/P7-36). ([ADR](../designs/phase-11f-hdr-float-status.md))
- [phase-11f-hdr-float-shipped.md](phase-11f-hdr-float-shipped.md) — 11F's shipped companion as of 2026-10-30 — the float decisions in force and each shipped item (blend modes, styles, adjustments and their takes, paint and fill, selections, placed and RAW sources) with its take readings and admission ratios. ([ADR](../designs/phase-11f-hdr-float-status.md))

## Phase 12 — workflow and high-depth parity

- [phase-12a-replay-status.md](phase-12a-replay-status.md) — 12A replay gaps — `move_layers` multi-layer Move, channel index fallback, byte canonical dabs (widened in 12F-5), `color_space` on brush/fill/gradient. Shipped 2026-09-18. ([ADR](../designs/phase-12a-replay-status.md))
- [phase-12b-actions-status.md](phase-12b-actions-status.md) — 12B reusable Actions — stable ids, schema 2, geometry modes, parameters, `.rzaction` bundles, headless batch, layer-set gap tools — plus per-entry RAW preset diagnosis/repair. Shipped 2026-09-18. ([ADR](../designs/phase-12b-actions-status.md))
- [phase-12c-workspace-status.md](phase-12c-workspace-status.md) — 12C — New Document, layer colour labels (`LLBL` 41/42, PSD `lclr`), shared `PreferenceLibrary`, Navigator, draggable panel, customizable shortcuts, saved workspaces. Shipped 2026-09-18/19. ([ADR](../designs/phase-12c-workspace-status.md))
- [phase-12d-comparison-views-status.md](phase-12d-comparison-views-status.md) — 12D — layer comps (`LCMP` 43/44, id-bound since 12E-1b), view-only rotation, synchronized second views on one `NSDocument`. Shipped 2026-09-19. ([ADR](../designs/phase-12d-comparison-views-status.md))
- [phase-12e-layer-ops-status.md](phase-12e-layer-ops-status.md) — 12E high-depth layer ops — persisted ids (RZDC 45/46), typed structure, merge, locks, guides, slot-rule insertion, stamp/flatten, knockout at both depths with PSD `knko`/`infx`. Closes P7-26/27/37. ([ADR](../designs/phase-12e-layer-ops-status.md))
- [phase-12f-geometry-status.md](phase-12f-geometry-status.md) — 12F high-depth geometry across ten runs — typed Canvas Size, straighten/arbitrary rotation, float exact ops, interactive geometry, set/retained transforms, warp, source geometry, paint replay. Shipped. ([ADR](../designs/phase-12f-geometry-status.md))
- [phase-12g-clipboard-status.md](phase-12g-clipboard-status.md) — 12G high-depth clipboard — RZCB v1–v3 typed payload, Cut/Copy/Paste at depth, convert/promote arms, New from Clipboard, recordable $asset Paste, editable payloads. Shipped. ([ADR](../designs/phase-12g-clipboard-status.md))
- [phase-12g-clipboard-shipped.md](phase-12g-clipboard-shipped.md) — 12G shipped companion — the full Part A–D records (paste conversion, recorded Paste, layers into float and across profiles, group copy and cut, what travels with copied layers, placement) with their Photoshop verdicts, receipts and coordinates; condensed into the ADR's Shipped parts 2026-10-30. ([ADR](../designs/phase-12g-clipboard-status.md))
- [phase-12h-conversion-status.md](phase-12h-conversion-status.md) — 12H structure-preserving precision conversion — per-structure table, 8↔16, float→16, float→8 with source/warp carriage; no conversion into float. Shipped; closes Phase 12. ([ADR](../designs/phase-12h-conversion-status.md))

## Build velocity — closed 2026-10-08

- [build-velocity-status.md](build-velocity-status.md) — the working record at close (2026-10-08) — Phase 9b's runs with its size table, the gate and oversize demos, and Phase 10's three issues with every timing table and guard-bite demo. ([ADR](../designs/build-velocity-status.md))
- [build-velocity-shipped.md](build-velocity-shipped.md) — the shipped companion at close — Phases 1 to 9a and 9c, each with its before-and-after table, script checks, scratch-edit demos and gate results. ([ADR](../designs/build-velocity-status.md))
- [build-velocity-plan.md](build-velocity-plan.md) — the implementation plan (2026-10-06) — the owner's decisions, every phase's changes, acceptance and scope, the owner actions and the phase order. ([ADR](../designs/build-velocity-status.md))
- [build-velocity-proposal.md](build-velocity-proposal.md) — the study of 2026-10-06 — the 70-batch profile, the Swift, app, Rust and test-run measurements, the merged-binary prototype and the case against a crate or module split. ([ADR](../designs/build-velocity-status.md))

## Smart objects — removed 2026-10-10

- [convert-to-smart-object-status.md](convert-to-smart-object-status.md) — the working record of Convert to Smart Object and its float phases as it stood at `3e05aea6`, the last build with smart objects — the Photoshop 26.10.0 verdict tables (take1 to take9-3 and the float takes), the phase plan, the sub-feature table and every open item. The feature was removed for v1 on 2026-10-10. ([ADR](../designs/convert-to-smart-object-status.md))
- [convert-to-smart-object-shipped.md](convert-to-smart-object-shipped.md) — its shipped companion — each sub-slice's record from the core model (2a) to 32-bit PSD contents (float phase 3c-3): modules, `.rz` words 65 to 73, measured admission and the rulings. ([ADR](../designs/convert-to-smart-object-status.md))

## v1 float editing — closed 2026-10-12

- [v1-float-editing.md](v1-float-editing.md) — the working record of the v1 float batches at close (2026-10-12) — the eleven linear filters, the display-referred filters with Presence, the Develop Filter and Red Eye, retouching and Stroke Path, the four retouch brushes, the Vision selections, New Document at 32 and 8/16 → float, and the display-defined tools, blend modes and adjustments, each with its decisions, admission figures and checks. ([ADR](../designs/v1-float-editing.md))

## Smart Sharpen — closed 2026-10-13

- [smart-sharpen-status.md](smart-sharpen-status.md) — the working record of Smart Sharpen before it was built (2026-10-09) — the Photoshop 26.10.0 take's verdict tables (linearity, legacy kernels, Reduce Noise as a smoother, the fade curves) and the phase plan the v1 batch replaced with its own algorithm. ([ADR](../designs/smart-sharpen-status.md))

## Merge and stacks — closed 2026-10-19

- [v1-stacks-status.md](v1-stacks-status.md) — the working record of merge and stacks at close (2026-10-19) — the phase head, every decision with its Photoshop take (take1 to take3, the offline re-reads), the reach table, the memory-plan rows of phases 1 to 3a and the verification list. ([ADR](../designs/v1-stacks-status.md))
- [v1-stacks-shipped.md](v1-stacks-shipped.md) — its shipped companion — Load Files into Stack, Auto-Align Layers with its projection, lens and reach runs, Stack Modes, Reduce Noise from Stack, Merge to HDR, Auto-Blend Layers, Merge Panorama and HDR Toning, each with its plan, admission figures and checks. ([ADR](../designs/v1-stacks-status.md))

## Grayscale mode — closed 2026-10-21

- [grayscale-mode-status.md](grayscale-mode-status.md) — the working record of #1's Grayscale mode at close (2026-10-21) — the status head, the closing batch's contract and the list of tools no phase named. ([ADR](../designs/grayscale-mode.md))
- [grayscale-mode-shipped.md](grayscale-mode-shipped.md) — its shipped companion — every decision in force with its Photoshop take (gray-luminance, gray-mode-547 and gray-files-547 take1 to take4) and phases 1a to 6b, each with its tests, drive gates and admission figures. ([ADR](../designs/grayscale-mode.md))

## Lab Color mode — closed 2026-10-22

- [lab-mode-status.md](lab-mode-status.md) — the working record of #1's Lab Color mode at close (2026-10-22) — the status head with every phase's commit, the phase 6 contract and the close's list, Photoshop in Lab (measured) and the open items. ([ADR](../designs/lab-mode.md))
- [lab-mode-shipped.md](lab-mode-shipped.md) — its shipped companion — every decision in force with its Photoshop take (lab-mode-547, lab-blend-681, lab-files-547, lab-export-547, lab-select-547, lab-gray-547, lab-cmyk-547) and phases 1 to 6b-iii, each with its tests, drive gates and admission figures. ([ADR](../designs/lab-mode.md))

## Indexed Color mode — closed 2026-10-23

- [indexed-mode-status.md](indexed-mode-status.md) — the working record of #1's Indexed Color mode at close (2026-10-23) — the status head with every phase's commit, the 3d take and the close's plan, Photoshop in Indexed Color (measured) and the open item. ([ADR](../designs/indexed-mode.md))
- [indexed-mode-shipped.md](indexed-mode-shipped.md) — its shipped companion — every decision in force (the palettes, the edge rule, the ink and background rules, the exact geometry, paste, profiles, tool classes) and phases 1a to 3c, each with its tests, drive gates and admission figures. ([ADR](../designs/indexed-mode.md))

## Duotone mode — closed 2026-10-23

- [duotone-mode-status.md](duotone-mode-status.md) — the working record of #1's Duotone mode at close (2026-10-23) — the status head with phases 1a to 2b and their commits, the decisions in force with take1's numbers and menu walk, "What remains for the close" (the Grayscale question, Bitmap, the walk's 25 paths, the unreachable arms) and the mode's scope. ([ADR](../designs/duotone-mode.md))

## Bitmap mode — closed 2026-10-23

- [bitmap-mode-status.md](bitmap-mode-status.md) — the working record of #1's Bitmap mode at close (2026-10-23) — the status head with phases 1a to 3 and their commits, the close's contract, the known differences, the leaf rule, Photoshop in Bitmap (measured) and the admission table. ([ADR](../designs/bitmap-mode.md))
- [bitmap-mode-shipped.md](bitmap-mode-shipped.md) — its shipped companion — the 14 decisions in force (storage, methods, files, the gate's tiers, the edits through the adapter, plans) and phases 1a to 3. ([ADR](../designs/bitmap-mode.md))

## Spot channels — closed 2026-10-26

- [spot-channels-status.md](spot-channels-status.md) — the working record of #1's spot channels at close (2026-10-26) — the status head with 1a-1 to 1c-2a, the sources, the decisions in force (model, overprint, saves, PSD/PSB, flat TIFF, PDF), the plan, 1c-2b's contract, open items and the shipped runs. ([ADR](../designs/spot-channels.md))

## Retired assets

- [rasterize-app-icon/](rasterize-app-icon/) — The retired Rasterize "1a Overlap" app icon (three multiplying circles in a squircle): SVG master, appiconset and notes; replaced by the Doctored icon. ([current](../designs/new-icon/README.md))

## 2026-10 — originals of records condensed again

- [2026-10/phase-9f-selected-tone-status.md](2026-10/phase-9f-selected-tone-status.md) — 9F as of 2026-10-07 — the September ADR plus the full records of the additional spatial filters (Box/Surface Blur, Add Grain, Radial Blur, Clouds, Offset), the proxy previews, the 24 MP 16-bit previews, the 24 MP ceilings Parts C and D phase 1, the byte filter_op plan and its Apply as a job, with every calibration, footprint, latency and drive table. ([record](../designs/phase-9f-selected-tone-status.md))
- [2026-10/phase-4-jobs-resource-status.md](2026-10/phase-4-jobs-resource-status.md) — Phase 4 as of 2026-10-07 — the September ADR plus the full issue records of the geometry, placed-image, native save/reopen, recovery and retouch admission work, with gates M, R and R′, every footprint measurement and MCP drive. ([record](../designs/phase-4-jobs-resource-status.md))
- [2026-10/phase-9-live-filters-status.md](2026-10/phase-9-live-filters-status.md) — Phase 9 as of 2026-10-07 — the September ADR plus the full records of Parts B, D phase 2 and E (live filters at 24 and 45 MP, the banded last stage, gate S) and of the operations on a live-filtered layer, with every footprint, ceiling, probe and drive table. ([record](../designs/phase-9-live-filters-status.md))
- [2026-10/phase-12g-clipboard-status.md](2026-10/phase-12g-clipboard-status.md) — 12G as of 2026-10-07 — the September ADR plus the full clipboard issue records, with the Photoshop drive verdicts in full, the float gap table and the admission measurements. ([record](../designs/phase-12g-clipboard-status.md))
- [2026-10/build-velocity-status.md](2026-10/build-velocity-status.md) — Build velocity Phases 1–4 as first recorded (2026-10-06) — every timing row, the Debug-settings variants, the CLAUDE.md split's script output and transcript check, the 100-drive Debug/Release table and the gate probes. The phase closed 2026-10-08; its final originals are at the archive root. ([ADR](../designs/build-velocity-status.md))
- [2026-10/phase-11c-output-color-status.md](2026-10/phase-11c-output-color-status.md) — 11C as of 2026-10-07 — the September ADR plus the issue records added since, with the proof timing table. ([record](../designs/phase-11c-output-color-status.md))
- [2026-10/phase-11d-format-additions-status.md](2026-10/phase-11d-format-additions-status.md) — 11D as of 2026-10-07 — the September ADR plus the issue records added since, with the host-still admission measurements in full. ([record](../designs/phase-11d-format-additions-status.md))
- [2026-10/phase-11f-hdr-float-status.md](2026-10/phase-11f-hdr-float-status.md) — 11F as of 2026-10-07 — the September ADR plus the issue records added since, with the Photoshop patch table and readback calibration of the float selection rule. ([record](../designs/phase-11f-hdr-float-status.md))
- [2026-10/phase-12d-comparison-views-status.md](2026-10/phase-12d-comparison-views-status.md) — 12D as of 2026-10-07 — the September ADR plus the issue records added since, with the retained-source entries measurement in full. ([record](../designs/phase-12d-comparison-views-status.md))
- [2026-10/phase-10c-constructs-status.md](2026-10/phase-10c-constructs-status.md) — 10C-2 as of 2026-10-07 — the September ADR plus the issue records added since, with the measured `levl` / `curv` byte layouts and every render-parity figure. ([record](../designs/phase-10c-constructs-status.md))
- [2026-10/phase-8g-raw-enhancements-status.md](2026-10/phase-8g-raw-enhancements-status.md) — 8G as of 2026-10-07 — the September ADR plus the issue records added since, with every decode and local-work case table, fit, ceiling and drive. ([record](../designs/phase-8g-raw-enhancements-status.md))
- [2026-10/phase-12b-actions-status.md](2026-10/phase-12b-actions-status.md) — 12B as of 2026-10-07 — the September ADR plus the issue records added since (items 1–5). ([record](../designs/phase-12b-actions-status.md))
- [2026-10/phase-12c-workspace-status.md](2026-10/phase-12c-workspace-status.md) — 12C as of 2026-10-07 — the September ADR plus the issue records added since. ([record](../designs/phase-12c-workspace-status.md))
- [2026-10/phase-12a-replay-status.md](2026-10/phase-12a-replay-status.md) — 12A as of 2026-10-07 — the September ADR plus the issue record added since (`color_space` on the remaining tools, the 16-bit document hex, sampled sRGB numbers for text and shape colours). ([record](../designs/phase-12a-replay-status.md))
- [2026-10/phase-13a-regional-status.md](2026-10/phase-13a-regional-status.md) — 13A as of 2026-10-07, condensed here for the first time (the phase was open on 2026-09-26, so there is no September original) — the sub-slice plan with every split and every `Owns` / `Exit` cell, the 13A-1 contract and its 30 contract deltas, the 13A-6 cap survey, all 102 decisions, the 31 verification records with their receipts paths, cancel-to-return tables and run logs, the latency A/B and bisect tables, and the open-items table with its 65 struck rows ([record](../designs/phase-13a-regional-status.md))
- [2026-10/phase-13b-tiled-status.md](2026-10/phase-13b-tiled-status.md) — 13B as of 2026-10-07, condensed here for the first time (the phase was open on 2026-09-26, so there is no September original) — the survey of retained storage before 13B-1 with its line evidence, the sub-slice plan with every `Owns` / `Exit` cell, the 30 run contracts, all 198 decisions with their constructor-site classifications, the 30 verification records with their receipts paths, bound tables and run logs, and the open-items table with its 48 closed rows ([record](../designs/phase-13b-tiled-status.md))
- [2026-10-30/phase-10c-constructs-status.md](2026-10-30/phase-10c-constructs-status.md) — 10C-2 as of 2026-10-30, before its shipped companion was folded into the ADR — the decisions, open receipts and open items as they stood, with the take records. ([ADR](../designs/phase-10c-constructs-status.md))
- [2026-10-30/phase-11f-hdr-float-status.md](2026-10-30/phase-11f-hdr-float-status.md) — 11F as of 2026-10-30, before its shipped companion was folded into the ADR — the open items as they stood and the shipped runs, with their take records. ([ADR](../designs/phase-11f-hdr-float-status.md))
- [2026-10-30/phase-8g-raw-enhancements-status.md](2026-10-30/phase-8g-raw-enhancements-status.md) — 8G as of 2026-10-30, before its shipped companion was folded into the ADR — the open items as they stood. ([ADR](../designs/phase-8g-raw-enhancements-status.md))
- [2026-10-30/phase-12g-clipboard-status.md](2026-10-30/phase-12g-clipboard-status.md) — 12G as of 2026-10-30, before its shipped companion was folded into the ADR — the open items as they stood. ([ADR](../designs/phase-12g-clipboard-status.md))
