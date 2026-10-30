# Experimental cutout output contract

The advanced refinement output path now builds a complete reversible edit from an opaque foreground RGB image and a separate alpha mask. It is internal plumbing: no new menu, brush, MCP command or Actions entry is enabled. Assembly correctness does not qualify the input matte or foreground colors.

## Document result

`rz_doc_add_refinement_cutout` adds a top-level **Refined Cutout** raster layer, at canvas origin and canvas size, using the existing layer insertion and native capacity rules. Its RGB is copied exactly; its pixel alpha stays 255. The supplied coverage becomes a separate enabled, linked mask with density 1 and feather 0. This avoids applying alpha twice and keeps later mask painting, geometry and properties independent of the foreground pixels.

The previous top-level entries become hidden so the new cutout can be inspected over transparency. All original pixels, masks, descriptions, grouping, child visibility and other properties survive. Old pixel and mask buffers remain shared. Undo restores the complete previous editing state, including top-level visibility and active layer. Native v10 already represents the result, so no format change is needed.

Empty alpha is a no-op. Canvas mismatch, nonopaque foreground, native capacity refusal, cancellation and a reused job token cannot publish a document. An image handle has no profile: supplying foreground RGB in the captured document's profile is explicitly a caller precondition, not a detectable foreign-profile assertion.

## Host and resource boundaries

`RefinementCutoutCapture` captures the existing `SelectionCapture` and the categorical brush session snapshot. Preparation and publication validate both document/selection identity and revision and local session identity and revision. A draft, a later stroke, a foreign same-sized session, document edits, or Undo back to the same document handle cannot revive an old result. Cancellation is checked after work and immediately before publication. The caller gets one prepared document and commits it through the existing central mutation/history path in one explicit Undo group.

The core scratch query includes 5 bytes per canvas pixel for new RGBA/mask storage, cloned document records and strings, and 1 MiB of bookkeeping allowance. Existing profile, pixel, mask, style and capture-metadata allocations are shared. The host additionally admits its alpha array; image handles are already in live resource accounting. Validation and transfer poll the job token every 4,096 pixels; copies touch bounded chunks instead of performing an uncancellable full-image clone.

Display preparation is a separate admitted pass using `PreparedDocumentEdit`. The [9I execution continuation](phase-9i-execution-status.md) gives assembly, byte projection, native composition and native-to-byte preview conversion separate single-use child controls beneath one claimed parent. The internal byte cutout shape renders through the existing compositor and quantizer in at most 4,096-pixel chunks. Cancel and actual document close reach the active child; all workers drain before preparation returns. Each admitted metadata JSON parse and CoreGraphics image construction remain noncancellable internally, with checks at their boundaries. This does not qualify a future public solver-to-display workflow or promise universal cancellation latency.

Cutout insertion now uses the shared insertion path with a controlled clone: records and guides have checkpoints, and owned UTF-8 strings copy in at most 64 KiB chunks. Ordinary insertion retains its previous clone behavior. Non-matrix/TRC profile validation preserves the original hidden-adjustment refusal, with a conservative metadata-parser allowance admitted before parsing and checks between metadata slots. Publication requires both the core projection and its display image; missing display preparation refuses the edit. After commit, the existing editor retargeting helper updates canvas and panels as well as the document's active-layer/history model.

## Verification

- Three core FFI tests cover independently known unbaked RGB and coverage, grouped source preservation, exact native reopen, empty no-op, malformed/nonopaque input, scratch refusal, single-use tokens and live cancellation during a 4096×4096 copy. All pass in Release; live cancellation returns within the test's 500 ms limit with zero retained scratch.
- The authoritative app `make typecheck` passes with zero errors.
- The AppKit source probe has a `refinement cutout output` check for preparation without mutation, the RGB/mask split, native reopen, one clean Undo/Redo, document/session staleness, cancellation and no-op/refusal cleanup. Its final run is recorded in the Phase 6 status handoff.

Relevant files: `core/src/doc_cutout.rs`, `core/src/ffi_cutout.rs`, `app/Sources/RefinementCutout.swift`, `core/tests/refinement_cutout.rs`, and `scripts/selection-probe/cutout.swift`.
