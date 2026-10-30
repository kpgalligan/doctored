# Phase 1 — save and rendering integrity

Implementation record for the [professional photo editor plan](professional-photo-editor-implementation-plan.md), September 9, 2026. **Phase 1 (1A–1D) is implemented and passes its integrity gate**, qualified with the release Rust core and a Swift development app. Subsequent phase progress is tracked in the implementation plan.

## Compatibility and behavior

| Slice | UI | MCP / Actions | Persistence and deliberate boundaries |
| --- | --- | --- | --- |
| 1A Native capacity | Refuse unsupported growth and native saves; keep existing work available for reduction/export | Same core growth checks and atomic `save_copy` refusal | Shared writer/reader preflight before PNG encoding/decoding. 400 million aggregate layer pixels and existing count/plane/blob/group limits remain unchanged. Version 8 writer, versions 1–8 reader. Invalid masks fail instead of disappearing. |
| 1B Save preservation | Save, Save As and close-time Save route through the document-versus-format policy; unsupported flat master writes refuse | `get_document.save_preservation` reports the UI policy; explicit flat `save_copy` reports losses and keeps the master dirty | Inventory covers masks, descriptions, styles, names/properties/locks/links, layer bounds, channels, guides/origin, global light, profile, density, packet refusal and transparency. Export remains intentional flattening. JPEG compression/GIF palette quantization remain explicit format choices. |
| 1C Render failures | Preserve last valid display preview with failure status; refuse rendered output for the failed revision | Render/sample/flat save refuse; native save remains available; undo/redo restore renderability with document state | Group-buffer budget is a render failure, never an empty group. Reduced per-call budgets qualify failures using small fixtures; production capacity is unchanged. No persistent rendering-policy change. |
| 1D Alpha-correct sampling | Resize, Gaussian blur and sharpening's blur use premultiplied sampling | Existing tools and recorded operations use the same corrected core | Straight RGBA8 storage and encoded-color evaluation remain unchanged. Nearest/identity operations retain latent RGB; masks/channels/styles remain scalar. |

Flat formats retain their existing container limitations. TIFF profile/PPI round trips are still Phase 3A; the Save check accounts for the current reader's limitations. JPEG/PNG density is checked against the emitted container and normalized EXIF, allowing 0.013 PPI for PNG's integer pixels-per-metre quantization. Authored profile bytes and nonzero RGB stored under zero alpha require native preservation when flat output would discard them.

Native capacity and render capacity are separate: a document can be natively saveable while its projection fails. AppKit safe writes stage the encoded file before replacing the destination, retaining existing file attributes; a format, capacity, render or encode refusal therefore occurs before destination replacement. Error-safe replacement does not claim power-loss durability via `fsync`.

## Verification

- `core/tests/native_capacity.rs`: aggregate and count boundaries, refusal before any pixel decode/encode, previous-file preservation, malformed masks and reduction/export from oversized memory state. Large logical fixtures share allocations; malformed PNG streams demonstrate preflight order without gigabyte decodes.
- `core/tests/preservation_tests.rs`: one-property-at-a-time single-layer fixtures, format/packet/option losses, native rich-state reopen and FFI NULL handling.
- `core/tests/render_failure_tests.rs`: reduced budgets for isolated, gated pass-through, nested and clipped groups; sibling peak-budget release; native save/reopen after render failure.
- `core/tests/alpha_sampling.rs`: independent double-precision two-dimensional sampling references, alpha edges, low alpha, negative-lobe kernels, document resize/thumbnails, and unchanged encoded-color behavior.
- `python3 scripts/verify_phase1.py`: launches only owned test instances with dummy credentials, private temporary fixtures/actions, non-default ports and argv-only defaults. Exercises Save policy, mask/guide undo-redo, rich native reopen, explicit export/dirty state, alpha-correct editing and failed-render save refusal/native rescue.
- `python3 scripts/save-preservation-probe/verify.py`: compiles the app sources with an AppKit probe entry point. Exercises direct safe writes, Save completion, inherited autosave refusal, delegated Close → Save callbacks, capacity/render refusals, preserved preview identity and undo, native reopen, dirty state, destination bytes and file attributes. This probe exposed the default AppKit Save As cleanup deleting an existing destination on refusal; the staged writer regression now passes.

| Final check | Result |
| --- | --- |
| `make test` | 756 passed, 0 failed, 1 ignored; 53 result groups. Transport tests ran with localhost access. |
| `cargo fmt --check`, `make lint` | Passed. |
| `make typecheck` | Passed, 0 errors; 14 existing warnings. Clang module cache redirected to temporary storage. |
| AppKit Save probe | Passed against the final app sources, including staged-path diagnostic mapping, destination retention, file attributes, callback routing and preview identity. |
| Built-app MCP workflow | Passed all three suites in `verify_phase1.py`, using a newly compiled/ad-hoc-signed app with release Rust and `swiftc -Onone -whole-module-optimization`. `RZ_VERIFY_APP` selected its isolated executable. |
| Optimized app build | Not qualified: the final optimized Swift compilation was stopped after more than 12 minutes. No successful optimized build or performance claim is made. |

The app verification used generated small pixel fixtures and independent numerical references, not a real-photograph perceptual or large-document performance corpus. Native capacity was tested without allocating its full supported aggregate size. Test instances were stopped by their owning runners; the user's app instances were untouched.

## Recovery assessment for Phase 5

Local source and the installed AppKit SDK show `autosavesInPlace = false`, no configured `NSDocumentController.autosavingDelay` (the SDK default is zero), and inherited `autosavingFileType` selecting the current file type. There is no verified periodic native recovery path for imported or untitled documents. The new safe-writing guard refuses a lossy flat autosave rather than dropping editable state; that refusal is not native recovery.

Phase 5A/5B must establish a native recovery destination and captured revision, periodic scheduling, restart discovery, recover/discard behavior and lifecycle cleanup. Controlled crash/restart recovery remains outstanding. The existing 24-level undo policy is unchanged in Phase 1.
