# Phase 9I — typed refinement (the typed half of P7-12)

This slice closes the **typed** half of [P7-12](phase-7-carryovers.md#assignment-register):
the qualified basic Refine Selection path runs natively at u16 on a 16-bit document, and
its mask output is a first-class reusable u16 mask on a layer whose RGBA16 samples are the
cutout's unchanged input and output. It closes **nothing** of the original Phase 6 quality
gate; see [Remaining distance](#remaining-distance).

## Scope

In scope, on a 16-bit document only:

- Smooth, Feather, Shift Edge and Contrast, computed at u16 from the document's exact
  captured coverage rather than from an 8-bit projection, through all three entry points:
  Select ▸ Refine Selection, MCP `refine_selection` and Actions replay. The MCP path and
  the Actions replay are DRIVEN by `scripts/verify_phase9i_typed.py`; the sheet's own typed
  preview and apply share `SelectionRefinementCapture.nativeWork` and `applyNative` with
  them (`app/Sources/NativeSelectionRefinement.swift`, reached from
  `RefineSelectionSheetController`), and that shared branch is covered by `make typecheck`
  only — there is no UI automation for the sheet.
- Output to the selection (a `.mask16` canvas selection) or to a **canvas-sized u16 layer
  mask** at (0, 0) — enabled, linked, density 1, feather 0, replacing any existing mask,
  which is the byte contract's property reset expressed at typed depth.
- The refined coverage as a reusable output: `save_selection` into a u16 channel,
  `transform_mask`, snapshots, native `.rz` save and crash recovery.

Out of scope and unchanged: every experimental control (`edge_radius`, `edge_strength`,
`brush`, `decontaminate`, `trimap`, `foreground`), every estimator, `RefinementCutout.swift`
/ `NativeRefinementCutout.swift` / `precision_cutout.rs`, a typed *render* preview of the
mask output in the sheet, exposing `refine` through `modify_scalar` or Channels ▸ Modify
Plane, and any new FFI export, MCP tool, native record or `.rzsnapshot` schema.

## Contract

The kernel is `core/src/precision_selection_refine.rs`, reached as the `refine` kind of the
existing `rz_selection_u16` contract (`PlaneOperation::Refine { smooth, feather, shift,
contrast }`, every field defaulting to 0, unknown fields refused). It is a *composition*,
not a new family of kernels:

1. `Smooth { radius: smooth }`, skipped at 0.
2. `Grow { radius: shift }` when shift > 0, `Shrink { radius: −shift }` when shift < 0.
3. `Feather { radius: feather }`, skipped at 0.
4. Contrast, the only new arithmetic: `c == 1` is the plain threshold `v >= 32768 ?
   65535 : 0`; `0 < c < 1` evaluates in f64 in this exact order —
   `t = ((v / 65535) − 0.5) / (1 − c) + 0.5`, clamp `t` to [0, 1], `round(t · 65535)`
   (half away from zero); `c == 0` is skipped.

All four controls at zero copy the input codes exactly. Ranges are the byte kernel's:
smooth and feather ∈ [0, 64], shift ∈ [−64, 64], contrast ∈ [0, 1], all finite; anything
else refuses at `rz_selection_u16_plan`, before any allocation. The work forecast is the
sum of the stages' own per-kind expressions and the plan adds one inter-stage plane
(`2 · n`) to the shared scratch formula. The byte path (`selection_refine.rs`,
`rz_selection_refine`) is untouched and byte-identical.

On the Swift side `SelectionRefinementCapture` captures `coverage16` (the exact
`maskSamples16()`) whenever the document is high-depth; `isNative` selects
`NativeSelectionRefinement.swift`'s typed twin. The selection output commits `.mask16`; the
mask output goes through `PrecisionScalarBuffer` → `PrecisionPixelEdit(.setMask)` → the one
`PrecisionEditorCapture.publish`, exactly as `addPrecisionMask` does. The typed facade is
never routed to `rz_doc_adding_layer_mask` or `rz_selection_refine`. Sheet previews are
**explicit byte display derivatives** (`(v + 128) / 257`, the `maskBytes()` rule) and never
enter a commit — the 9G policy. MCP `refine_selection` keeps its arguments and adds
`"coverage_storage": "u16" | "u8"` to its result.

## Foundation evidence reused

This slice adds no new native assembly. It stands on evidence already recorded:

- `core/tests/precision_cutout.rs` — the four existing tests that pin exact RGBA16 source
  alpha beside an independent u16 refinement mask, mask-only edits that reveal tiny source
  alpha, FFI refusals/cancellation and the immutable-owner rules.
- [`designs/phase-9i-execution-status.md`](phase-9i-execution-status.md) — controlled
  byte/native output preparation, owner-close forwarding and single-use job controls.
- `scripts/verify_phase9i_native.py` — the owned native-assembly drive. It was **not**
  re-run for this slice and is unchanged.

## Verification receipts

- `core/tests/precision_selection_refine.rs` — six tests over `rz_selection_u16`:
  zero controls copy every odd code; contrast 1 is a plain threshold; the **depth receipt**
  is contrast 0.25 on 32770/32771 → 32771/32772, two distinct u16 codes that both project
  to displayed byte 128, plus a contrast-0.5 run over the 9G 7×5 pattern against an oracle
  written as the same f64 expression in the same order; each single control equals the
  standalone `smooth`/`feather`/`grow`/`shrink` kind exactly; all four equal the chained
  standalone kinds (which pins the order); out-of-range and unknown fields refuse at plan
  and a precancelled control leaves the 999 sentinel output untouched with
  `scratch_bytes() == 0`; and the typed cutout test — a refined coverage becomes a
  canvas-sized u16 mask whose layer's RGBA16 buffer stays **pointer-equal**, renders the
  expected alpha, survives `MaskProperties`/`affine` mask moves, becomes a saved channel and
  round-trips through `encode_native`/`parse_native` exactly.
- `scripts/verify_phase9i_typed.py` — an owned production MCP drive on a free port with a
  dummy API key and private Actions/recovery/preset directories. Thirteen checks, all
  passed:
  zero controls report `changed:false` with odd codes intact; contrast 1 equals the
  independent `>= 32768` oracle and `undo` restores exactly; contrast 0.25 separates
  32770/32771 inside one display byte; feather 2 equals `modify_selection feather 2`; a
  RECORDED `refine_selection` replays through `run_action` to the identical u16 coverage
  (the Actions entry point, added in Run 5 of the Phase 10 closeout);
  selection and mask outputs of the same parameters are identical coverage; the saved
  layer-0 PNG16 samples are byte-identical (tiny alpha 0/1 intact) while the mask data is
  the refined u16; `transform_mask` translate (1, 0) moves the mask and leaves the source
  exact; `save_selection` reuses the coverage as an exact u16 channel; snapshot
  save/restore and crash recovery return the exact mask, channel and source; the
  experimental and out-of-range controls refuse without touching history; and an 8-bit
  document still reports `coverage_storage == "u8"`.
- Logs. Receipt (Run 5 of the Phase 10 closeout, 2026-09-16; the take ran just
  after midnight, 2026-09-17):
  `~/devel/rasterize-fixtures/phase10-receipts/typed-refine/result.json`, app SHA-256
  `44edd1fde2b2f8741bbd9c735becb0278be4e25fe1b8a81b38ea4c87aa407858`, `passed: true` with all thirteen checks and
  `all_owned_apps_exited: true`. Per-run scratch (call traces
  `typed-calls.jsonl` / `recovery-calls.jsonl` / `byte-calls.jsonl`, the app logs under
  `app/`, `recovered-app/`, `byte-app/`, the published catalog and every intermediate
  `.rz`): the `scratch` path the report records — `/var/folders/.../
  rasterize-phase9i-typed-2mneuj0x` for the accepted run.
- Gates re-run for this slice: `cargo fmt --check`, `make lint`, the four named cargo test
  targets, `make test`, `make typecheck`, `make app`.

**Docs must not cite contrast 1 as a precision receipt.** Its threshold sits at 32768, which
is exactly the byte-128 boundary (`257 · 128 = 32896`, and `(32768 + 128) / 257 = 128`), so
a byte pipeline produces the same answer. The receipts for depth are the contrast-0.25 pair
and the exact odd-code identity.

## Remaining distance

The original Phase 6 gate stays open because 82 of 90 corpus cases have never passed
combined alpha/color, 12 analytic cases are blocked by fixture construction or missing
transmission evidence, the recorded feasibility decision forbids further custom variants
pending RVM checkpoint terms and blinded human guidance, and the photographic corpus is no
longer on disk. Nothing in this slice measures matte quality on a photograph: it qualifies
the *transport* of coverage at u16 and the mask it produces, not what an estimator would put
in that coverage. Advanced edge brushing and color decontamination stay refused by
`refineSelection`, and P7-12 stays open with 9I.

## Files

- `core/src/precision_selection_refine.rs` (new), `core/src/precision_selection_plane.rs`,
  `core/src/lib.rs`, `core/include/rasterize_core.h`.
- `core/tests/precision_selection_refine.rs` (new).
- `app/Sources/NativeSelectionRefinement.swift` (new), `app/Sources/SelectionRefinement.swift`,
  `app/Sources/RefineSelectionSheetController.swift`,
  `app/Sources/AgentServer+SelectionRefinement.swift`, `app/Sources/DocumentPrecision.swift`
  (`refine_selection` added to `typedTools`, without which the MCP tool refused in 16-bit mode).
- `scripts/verify_phase9i_typed.py` (new).
- Docs: this file, `core/CLAUDE.md`, `app/CLAUDE.md`, `designs/phase-7-carryovers.md`,
  `designs/professional-photo-editor-implementation-plan.md`,
  `designs/phase-6-selection-mask-status.md`, `designs/phase-7-operation-support.md`, `README.md`.
