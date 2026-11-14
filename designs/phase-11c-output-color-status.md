# Output color control — LUT profiles, intents, soft proofing, conversion policy (ADR)

- **Status:** Accepted — open items remain ([#1](https://github.com/kpgalligan/doctored/issues/1): PDF/X)
- **Phase / dates:** 11C, receipt 2026-09-17
- **Originals:** [archive/phase-11c-output-color-status.md](../archive/phase-11c-output-color-status.md) — the phase's full history, to 2026-09-26; [archive/2026-10/phase-11c-output-color-status.md](../archive/2026-10/phase-11c-output-color-status.md) — this doc as of 2026-10-07, with every measurement table and issue record
- **Key commits:** `54d2547`
- **Carryover:** closes **P7-22**
- **Changed 2026-10-10:** live filters and placed (non-RAW) sources were removed
  ([removal ADR](convert-to-smart-object-status.md)), so every rule below about a retained source now applies
  to camera RAW layers only, and every rule about a live filter (`live_filter_profile::plan`,
  `LIVE_FILTER_ROUTE`, the checked live-filter conversion) is gone. Convert to Profile gained
  `rasterize_sources`: where a RAW layer makes the conversion refuse (anything but relative colorimetric
  without black point compensation), the Convert sheet's "Rasterize N RAW layers, then convert", MCP
  `convert_profile` and the recorded Action rasterize the bound layers and convert as one step;
  a bound source therefore has no intent to persist.

## Context
The plan asked for broader ICC transforms, rendering intents and black-point compensation, soft proofing and gamut
warnings verified against a reference CMS; a defined policy for adjustment layers during profile conversion (keep
the existing keep-parameters default, qualify an appearance-preserving alternative without flattening the master);
and an explicit Assign workflow consistent with 8A's rule that retained sources are never silently reinterpreted.
The overriding constraint: default conversions and every 11A/11B default export stay byte-identical.

## Decisions
- **LUT engine** (`core/src/icc_lut.rs`): v2 `mft1`/`mft2`, v4 `mAB `/`mBA `, for RGB, Gray and CMYK devices with a
  Lab or XYZ PCS, parsed into one staged `LutPipeline` (curves → 3×3+offset → CLUT). PCS encodings are per tag:
  `mft2` Lab uses the **v2 legacy** encoding (L\*=100 at 0xFF00); Lab encoding is the tag's question, XYZ's the
  header's. Tetrahedral interpolation for 3-input CLUTs (weights are the sorted fractions themselves, applied to
  corner deltas — the difference form is not exact at corners), multilinear otherwise.
- **Refused, naming what the profile is** (`dr_icc_describe`'s `refusal`), checked class → data space → PCS →
  tables: named-colour (`nmcl`, a list of colours), device-link / abstract (a transform) and any other
  non-pixel class; an `nCLR` space by its colourant count (`4CLR` included) or any space but RGB/Gray/CMYK; an
  `mpet`-typed `A2Bx`/`B2Ax`; a `D2B0..3`/`B2D0..3`-only profile (the first found is named); CLUT > 64 MiB decoded;
  truncated tables. Each family needs its own fixtures and reference comparison before it can be believed. A Gray profile is its `kTRC` (D50); Gray with only A2B tables refuses.
- **`IccProfile` gains `lut`, `can_decode()`, `can_encode()`**; an RGB LUT-only profile inspects as `Inspect::RgbLut`.
  `model()` still means matrix/TRC. **`dr_doc_profile_is_convertible` keeps its matrix/TRC meaning** (widening it
  would admit LUT documents to `document_v2` Exposure/WB, which need a matrix model); the wider question is the new
  `dr_doc_profile_can_decode`, used only by the Convert sheet's and `convert_profile`'s source gates.
- **Intents** (`core/src/icc_intent.rs`): perceptual A2B0/B2A0; relative A2B1/B2A1 and saturation A2B2/B2A2, each
  **falling back to table 0** (ICC reader requirement, what every CMM does); absolute = relative tables +
  componentwise media-white scaling. Matrix/TRC profiles: every intent is the matrix path.
- **BPC** is ISO 18619's affine XYZ map (BP_src ↦ BP_dst, D50 fixed). Black points live in
  `core/src/icc_black_point.rs`. A SOURCE black follows Little CMS's `cmsDetectBlackPoint`: matrix/Gray = device
  zero pushed forward (no curve inverter exists), made neutral with L* ≤ 50 and zero above L* 95, like every darker
  colorant (a well-formed matrix black stays exactly zero); absolute is asked as relative; LUT v4
  perceptual/saturation = lcms's fixed perceptual black (Y = 0.0034731); an output-class (`prtr`) CMYK profile at
  relative = its perceptual black (Lab 0 through `B2A0`, back through `A2B1`), which discounts ink limiting, made
  neutral with L* ≤ 50; otherwise the darker colorant through the intent's own `A2Bx`, made neutral with L* ≤ 50 but
  zero when its L* is above 95 or below 0 (lcms's `BlackPointAsDarkerColorant`); a missing table is lcms's zero
  black, so BPC onto a zero destination black is the identity. A DESTINATION black — the `ColorPipeline`
  destination, both soft-proof legs' destinations and the flat open's sRGB — follows
  `cmsDetectDestinationBlackPoint`: a matrix profile answers its source black; a LUT profile's Lab → device → Lab
  round trip at the intent is ramped, and a round trip straight above its shadows keeps the source black while any
  other is fitted with lcms's least-squares quadratic (Gray `kTRC`, v4 perceptual/saturation and a profile with no
  B2A answer the source black).
- **`ColorPipeline`'s fast path IS the shipped path**: two matrix/TRC profiles, non-absolute intent, BPC off →
  holds the existing `Transform` / `PrecisionTransform`. This is the byte-identity guarantee, measured over a 32³
  cube (`color_intent_tests::the_default_matrix_pipeline_is_the_shipped_transform_byte_for_byte`).
- **A failing matrix profile is a valid SOURCE**: `PrecisionTransform::new` and the typed convert plan validate only
  the DESTINATION they encode into (an exact identity validates nothing), so a document tagged with a matrix/TRC
  profile whose curves fail `validate_precision` converts, adopts and exports out of it through `Curve::eval`'s
  repairs — assign anything, convert out of anything. An 8-bit file embedding one is therefore CONVERTED on open.
  Export and Convert INTO one are refused by the CORE's own preflights, before any work, with `dr_icc_describe`'s
  `export_refusal` / `convert_refusal` sentence (`icc_encode_refusal.rs`): `ExportRequest::destination_refusal` in
  every byte and typed export entry point after the bit-depth check, and both `conversion_preflight`s right
  after the own-bytes rule. The document's own bytes under a non-absolute intent
  with BPC off are exempt, and `"profile":"document"` never converts, so it never refuses. The host keeps only the
  gates that must answer before the core is reached: Export Layers' whole-run check, contact sheets and preset save;
  a batch run decides per file through the core's export refusal, own-bytes exemption included. Every other encode
  into such a document's OWN profile (16-bit painting and artwork, linear-light at both depths, soft proof, a
  RAW layer's 16-bit render, a converted paste, an abstract Color Lookup look; a new or re-edited calibrated
  Exposure / White Balance is refused first through the core's `calibratedRefusal` query, which the host asks and
  `dr_doc_adjust_image` backs up, with this same sentence at both depths) answers one capitalised sentence naming
  the profile and pointing at Image ▸ Mode ▸ Convert to Profile… (no fallback encoder). ASSIGN of such a profile is
  admitted at both depths, rasterizing or not (a relabel encodes nothing; the 16-bit rasterizing Assign is planned
  as an assign), except to a linear-light document, one holding text/shape artwork, or one holding a document_v2
  Exposure/White Balance layer, each refused up front in its own `icc_encode_refusal` Assign sentence by the byte
  `assign_profile_refusal` and the typed `color_plan(convert: false)`, in that order, ahead of any re-render or
  rebind. The calibrated layer refuses at both depths, naming it: at 16 bits it can only evaluate through the
  validated encoder, and there is no fallback encoder. The Export sheet dims intent and BPC for "Document profile",
  which the MCP export tools ignore there. A preset naming such a destination file is refused at save with the same
  `export_refusal` sentence — a preset has no document, so the own-profile exception does not apply to it
  (`ExportPresetLibrary.save` and `save_export_preset`).
- **Soft proof** (`core/src/soft_proof.rs`, `dr_proof_render` / `dr_proof_gamut_mask` + precision twins): doc →
  PCS (relative) → proof device (setup intent, BPC flag) → PCS (relative; absolute when `simulate_paper_white`) →
  output RGB (document profile if encodable, else sRGB; relative, BPC **unless** `simulate_ink_black`). The result
  is tagged and drawn by CoreGraphics — **no monitor compensation in the core**.
- **Gamut rule**: out of gamut iff CIE76 ΔE\*ab (D50) between a pixel's Lab and its proof round trip > **5.0**, the
  round trip being **relative colorimetric always** (lcms's rule) regardless of setup intent — otherwise perceptual
  flags printable colours. Overlay = opaque `gamut_color` (default 50 % grey); alpha copied.
- **Proofing is view state only**: `ProofDisplay` on `EditorViewController`, never `EditingState`, the
  `.doctored` file, undo, dirty flag or an Action. Drawn through the display-only `ImageCanvasView.proofImage` slot
  (`previewImage ?? proofImage ?? image`), never assigned to `canvas.image` (whose `didSet` resets selection etc.).
  A sheet's live preview wins over the proof and is unproofed.
- **16-bit proof = the display codes**: at 16 bits the canvas and the default `render_proof` proof the RETAINED
  projection's 8-bit display codes (the nearest-rounding preview the canvas already draws) through the byte proof,
  so proofed and unproofed views compare like with like. `render_proof exact:true` proofs the full 16-bit composite
  through the typed twin on `BackgroundWork` with Cancel; the reply's `source` says which (`display` / `composite`).
  Tradeoff: against the exact chain the display-code proof is a few codes off on noisy content — the fidelity 8-bit
  documents already get, well inside T3's bounds below. The byte proof of a 24 MP document's display codes is
  bounded at 2.0 s (`display_code_proof_throughput_at_24_mp`, `make cancel-bench`); the timing table is in the
  archived original. At 8 bits the canvas proof and `render_proof` proof the retained projection too. A 32-bit float
  document has no soft proof: the three tools refuse, the canvas slot clears and the proof state is kept so Redo
  back into 16 bits proofs again. A proof that fails turns Proof Colors and Gamut Warning off, beeps, names the
  reason in the status bar and in `get_proof_setup`'s `failure`; a cancelled render is not a failure and delivers
  nothing. Exact canvas proof via parallel core bands is not built.
- **Conversion policy / P7-22** (`core/src/doc_color_policy.rs`): `ConversionOptions {intent,
  black_point_compensation, adjustments}`, all optional, `is_default()` keys byte identity
  (`dr_doc_convert_to_profile` delegates to `dr_doc_convert_to_profile_with`). **Keep parameters stays default.**
  `adjustments: "preserve_appearance"` keeps `exposure`/`white_balance` at `document_v2` (light-linear through the
  profile) and **bakes** every other op in place into one raster layer `"<name> (baked)"` (cut projection up to and
  including it; Normal, 100 %, visible). Layers below stay put. Refuses by layer name: a to-be-baked entry over any
  non-opaque backdrop, inside a group, or clipped. Hidden adjustments left alone; bakes run bottom-up. Into a
  destination with NO matrix/TRC model (`core/src/doc_color_destination.rs`, whose sentences the 16-bit
  `precision_color_policy` reads): keep refuses a
  `document_v2` Exposure/WB layer by name (offering the bake only when it would succeed), preserve appearance bakes a
  visible one under the same rules and refuses a hidden one, and a `linear_light` document refuses the destination
  (`core/tests/conversion_lut_destination.rs`).
- **Assign + 8A**: `assign_profile` refuses (naming the count) while RAW layers exist;
  `assign_profile_rasterizing` / `rasterize_sources: true` rasterizes via `SourceEdit::Rasterize` then assigns as
  ONE undo step. Both also refuse a profile with no matrix/TRC model while a `document_v2`
  Exposure/WB layer exists or the document is `linear_light` (`dr_doc_assign_profile_refusal`, shown per choice by the
  Assign sheet, thrown first by `assign_profile`). On documents with RAW layers BPC, absolute, and
  perceptual or saturation with a LUT side refuse, naming the count and the setting that keeps the sources bound
  (relative colorimetric, BPC off); relative to or from a LUT profile converts at 8 and 16 bits under any
  adjustment policy, every source re-rendering through `retained_source_render::lut_pipeline` (relative, no BPC),
  so the 9A cache key needs no intent.
- **Export**: `intent`, `black_point_compensation`, `profile: "file"` + `profile_path` join the 11A request; the host
  resolves `profile: "installed"` + `profile_name` to a path first so recordings carry a value. `info_json` gains
  `profile_name`, `intent`, `black_point_compensation`.
- Surfaces: Convert/Assign sheet additions; View ▸ Proof Setup ▸ Custom… (and saved presets), Proof Colors ⌘Y,
  Gamut Warning ⇧⌘Y; MCP
  `get_proof_setup`, `set_proof_setup`, `render_proof`, `describe_profile`, `list_installed_profiles` (all
  `nonRecordable`), plus new args on `convert_profile` / `assign_profile` and `can_decode` on `get_color_profile`.

## Rejected alternatives
- Refusing single-table profiles for relative/saturation — makes them unusable for intents they plainly mean.
- Gamut round trip at the setup intent — flags in-gamut colours under perceptual.
- A general per-op retarget for preserve-appearance — curve points and saturation amounts have no meaning in
  another encoding.
- Silently detaching retained sources on Assign (the pre-11C behaviour) — the 8A violation the plan asked to fix.
- Widening the tolerance after T3 missed — tolerances are fixed before measurement. T3 was re-derived instead from
  the written stage-by-stage finding (Open items), the ending its acceptance named.

## Consequences
Default conversion/export byte identity is load-bearing (`conversion_policy_tests`, `export_pipeline`). Tests:
`core/tests/{icc_lut,color_intent,soft_proof,conversion_policy}_tests.rs`; drive `verify_phase11c_color.py`
(T1–T6 tolerances fixed in-file; in the private archive `kpgalligan/doctored-dev`). **T3 result (2026-09-17, FOGRA39 vs
Adobe ACE, tolerance mean ≤ 2.0 / p95 ≤ 4.0 / max ≤ 8.0 ΔE76):** perceptual+BPC mean 1.87, relative 3.52,
relative+BPC 4.15, absolute 5.59 — a miss; the gamut-mask half of C6 passes exactly (8 249 / 17 152 under both
intents). T2 (vs ACE, matrix) is within 1–2 codes; T5 preserve-appearance mean 0.126 vs keep-parameters max 65.
**T3 re-derived (2026-11-14, Open items):** the gate is the proof against Little CMS's same chain; ACE is
recorded at Little CMS's own distance from it.

## Open items (verified 2026-10-30)
- `DONE` (2026-11-14) — T3, a written finding, no defect. Compared stage by stage with Little CMS 2.19.1
  (Homebrew build, double precision, `cmsFLAGS_NOOPTIMIZE`; never vendored, no committed test uses it) on the 11C
  plate and FOGRA39, over the public `dr_proof_render` and the chain's public pieces (the batch handoff's
  `t3/run.sh`). ACE's references return to sRGB at relative with BPC off, which is the proof with
  `simulate_ink_black` on and paper white off; the default proof maps the proof's black onto the display's, another
  chain. On that chain every stage agrees with Little CMS (B2A mean ≤ 0.14 % ink, every other stage mean ≤ 0.06
  ΔE76), the proof is within mean 0.17, p95 0.70 and in-gamut max 2.0 ΔE76 of it at all four settings, and Little
  CMS misses ACE as Doctored does (mean,
  Little CMS / Doctored: perceptual+BPC 0.77 / 0.80, relative 3.51 / 3.52, relative+BPC 4.13 / 4.13, absolute
  5.58 / 5.59). The miss is ACE's distance from both CMMs. **T3 re-derived:** the gate is the proof
  (`simulate_ink_black` on) against Little CMS's same chain, mean ≤ 0.5 and in-gamut max ≤ 2.0 ΔE76 (the stage
  rule fixed before measuring); against ACE each setting's bound is Little CMS's own figure, recorded, not gated —
  in-gamut / out-of-gamut mean, p95, max: perceptual+BPC 0.85 1.53 2.49 / 0.68 1.37 2.29; relative 4.36 8.79 12.52 /
  2.59 7.43 14.31; relative+BPC 4.29 8.80 13.57 / 3.95 9.17 15.26; absolute 5.64 11.40 16.19 / 5.53 12.86 25.96.
- `DONE` — CMYK and Grayscale document modes and CMYK, Gray and spot files (`cmyk_document.rs`, `cmyk_export.rs`,
  `gray_convert.rs`, `gray_export.rs`, `tiff_spot.rs`, `psd_spot.rs`; [cmyk-mode-status.md](cmyk-mode-status.md),
  [grayscale-mode.md](grayscale-mode.md), [spot-channels.md](spot-channels.md)), Gray as a convert/export
  destination included.
- `OPEN` [#1](https://github.com/kpgalligan/doctored/issues/1) — PDF/X output.
- `DROPPED` (2026-10-30) — the physical print-sample runbook: a manual print session that gates nothing; the
  profile path is covered by the automated proof and export tests.
- `DROPPED` — evaluating `document_v2` Exposure/White Balance through a two-way LUT, which needs
  `adjust_calibrated` to decode through `ColorPipeline`; the gate stays matrix/TRC (`dr_doc_profile_is_convertible`).
- `DROPPED` — exact 16-bit proof on the canvas through parallel core row bands (Decisions, **16-bit proof**).
- `DONE` — `rasterize_sources` on Convert for BPC and absolute on source documents (head).
- `DROPPED` — proof targets through `D2Bx`/`B2Dx`; refused today (Decisions, **Refused**).
- `DROPPED` — `nCLR` proof targets; refused today by colourant count.
- `DONE` (2026-11-14) — the C6 ΔE figures re-measured against ACE after the lcms destination black: with
  `simulate_ink_black` on, relative 3.52, relative+BPC 4.13, absolute 5.59 and perceptual+BPC 0.80 mean (T3 above).
- `DONE` — every refusal names what the profile really is; the refused set is unchanged (Decisions, **Refused**;
  `core/tests/icc_lut_tests.rs`).
- `DROPPED` — named-colour support: such a profile is a list of colours, not a pixel transform.
- `DONE` — one `ConversionOptions::source_refusal` for both depths; preserve appearance is admitted beside a RAW
  layer (`core/tests/source_conversion_policy.rs`).
- `DONE` — the LUT conversion that keeps sources bound (Decisions, **Assign + 8A**), at 8 and 16 bits. The work
  gate charges the LUT colour visit at `LUT_COLOR_WORK_FACTOR`; 32 bits refuse a LUT side
  (`core/tests/source_conversion_lut.rs`).
- `DONE` — the no-matrix/TRC destination rules (Decisions, **Conversion policy** and **Assign + 8A**;
  `core/src/doc_color_destination.rs`).
- `DONE` — the source black is lcms's `cmsDetectBlackPoint` (Decisions, **BPC**), for the soft proof and the flat
  open's CMYK conversion alike; `proof_black_point` answers zero, not `None`, when a table is missing.
- `DONE` — every BPC destination black is lcms's `cmsDetectDestinationBlackPoint`, ported from lcms master
  `src/cmssamp.c` (Decisions, **BPC**).
- `DONE` — the matrix/TRC and Gray `kTRC` black rule (Decisions, **BPC**); sRGB/P3 blacks stay exactly zero.
- `DONE` — the encode refusals live in the core, the preset save refusal and the own-profile sentence (Decisions,
  **A failing matrix profile**). The Swift-side export and Convert checks are deleted
  (`core/tests/encode_refusals.rs`); there is no fallback encoder.
- `DONE` — authored colours on an 8/16-bit LUT-tagged document convert into it once (measured against Photoshop,
  take `lut-style-colours-100/take1`, B-APP): styles, lfx2/`phfl`, artwork, `srgb` ink and the host through
  `authored_color` / `dr_authored_color` (relative, no BPC); a LUT that cannot convert both ways keeps the numbers
  (`core/tests/lut_authored_colors.rs`).
- `DONE` (2026-11-14) — Convert to Profile into or out of a LUT at 16 bits: the default and explicit-options
  conversions run the relative/no-BPC pipeline (`lut_pipeline`), each component clamped to 0…1 before its one
  rounding (`precision_color::U16Conversion`, the one rule the matrix kernel and the options walk share); the
  colour plan charges the pipeline; a Lab document, a `linear_light` document into a model-less destination and a
  decode-only side refuse; the calibrated layer follows the byte rule; Gray → RGB Color and an Indexed table take
  the same pipeline (`core/tests/precision_lut_conversion.rs`, 2 codes of 65535 of a closed-form oracle). Against
  Little CMS 2.19.1 on Adobe's `VideoHD16-235.icc` (729-triple grid): within 2 u16 codes into it and 3 out of it
  inside its 16–235 range; at its floor and at clipped super-black/white the two CMMs part by up to 13 / 82 codes.
  On a LUT-tagged document `sample_color`'s `srgb_rgba` is the core's decode clamped to 0…1, in gamut when it
  encodes back within one code, and a `color_space: "document"` hex is the document's own numbers
  (`scripts/verify_lut_document_colours.py`). (A 32-bit float document requires a matrix/TRC profile by design.)
- `DONE` — the 8-bit canvas proof and `render_proof` proof the RETAINED projection
  (`ImageDocument.retainedByteProjection`, `ProofSource.flat(doc, retained)`) and render the document only when none
  is retained; pixels and `out_of_gamut` are unchanged.
- `DONE` — proof presets on disk (`ProofPreset.swift`: `ProofPresetLibrary`, `DR_PROOF_PRESETS_DIR`, 64 × 4 KiB), a
  lazy default for windows with no setup, and MCP `list_proof_presets` / `save_proof_preset` / `delete_proof_preset`
  / `set_default_proof_preset` plus `preset` on `set_proof_setup` / `render_proof`
  (`scripts/verify_issue32_proof_presets.py`). A preset stores the profile's name, path and SHA-256, never its
  bytes, and resolves through the saved path or the installed profile of the same name only when the digest matches.
- `DROPPED` — per-layer proof: a lone layer is not what prints, and Gamut Warning on the composite already
  covers the diagnostic need.
- `DONE` — soft proof (menu + MCP) at 16 bits (Decisions, **16-bit proof**; `soft_proof_tests.rs`, 9H
  `soft-proof-menu-ui`).
- `DONE` — MCP-level 16-bit soft proof check: `verify_phase11c_softproof16.py` (G1–G5; private archive). The canvas
  cancel-on-new-request check lives in 9H `soft-proof-menu-ui`, because the canvas proof slot has no MCP surface.
- `DONE` — the 16-bit preserve-appearance Convert plans with its own `dr_precision_doc_conversion_plan`
  (`PrecisionDocument::conversion_plan`), which counts the bake renders and the baked layers; formula and 24 MP
  measurement in `precision_color_policy.rs`' `# Admission`.
- `DONE` — RAW wider-gamut output choices shared with 8D (Display P3 output, plan §8 8D "Completed").
- `DROPPED` — proofed sheet live previews (decision: preview wins, unproofed) and monitor compensation in the core
  (decision: CoreGraphics applies the display profile).
