# Phase 2 — selection and mask targeting

Implementation record for the [professional photo editor plan](professional-photo-editor-implementation-plan.md), September 9, 2026. **Phase 2 (2A–2D) is implemented and passes its completion gate**, qualified with the release Rust core and an Xcode development app.

## Operation and target policy

| Command | Layer pixels | Layer/group mask | Color plane | Saved channel |
| --- | --- | --- | --- | --- |
| Destructive filters and adjustments | Selected coverage | Selected coverage | Selected coverage | Selected coverage |
| Auto Tone / Contrast / Color | Whole-source analysis, selected writes | Same, scalar | Same, scalar | Same, scalar |
| Fill and gradient | Selected coverage | Selected coverage | Selected coverage | Selected coverage |
| Pixel-only retouch, text, clear and transforms | Existing operation | Explicit refusal | Explicit refusal | Explicit refusal |

The shared `OperationScope` policy validates targets and locks. Groups and adjustment layers have no editable pixel target; their existing masks and document channels remain supported. A missing mask, read-only derived plane or unavailable channel refuses explicitly. Quick Mask permits coverage strokes; exit it before filtering, filling or drawing a gradient on a document target. MCP arguments default to `target: "layer"` and may explicitly name `mask`, a color plane, or `channel:<name>`. UI commands use the indicated target.

Filters read full target storage so their neighborhoods include unselected pixels. Their results write through canvas selection coverage mapped by the layer or group offset. With a selection, off-canvas storage and every zero-coverage byte are unchanged. Without one, the whole stored target is eligible, including off-canvas pixels. Scalar targets run the existing filter on opaque grayscale and retain its Rec. 709 luma. RGBA result blending uses encoded-color premultiplied interpolation; scalar results interpolate their coverage byte. The alpha-plane writer retains its existing convention of clearing RGB when a changed alpha becomes zero.

Automatic tonal commands analyze the whole target independently of the selected write region. UI previews, commits and MCP use the same immutable operation capture and result writers. Destructive sheets refuse changed document revisions, active targets/layers or selection coverage instead of applying different inputs from those previewed. Cached mask/channel overlay tints are hidden during a sheet preview so old coverage is not drawn over the new result. Incoming MCP mutations refuse during an active live-edit gesture, whose next tick or cancellation otherwise could overwrite the incoming edit.

## Selection history and Actions

`ImageDocument` owns the selection, previous selection and Quick Mask state. Replace, combine, modify, load, deselect, Reselect, Quick Mask mode changes and individual strokes register transient undo entries without counting a file edit. No-op selection commands do not open empty undo groups; no-op Quick Mask strokes preserve geometric selections and Reselect state. Persisted edits retain the corresponding selection snapshot. Canvas geometry commands clear the current and previous selection in their own undo entry; undo restores their exact pre-edit coverage. An Action containing only selection operations has one transient undo entry, while a mixed Action counts one persisted edit. Selection tools record feathering before combination, matching the UI's additive and subtractive gestures.

Quick Mask records the actual premultiplied gray/alpha coverage overlay after brush opacity, pressure, smoothing and timed dabs. Its canonical representation uses little-endian UInt32 run lengths followed by gray and alpha bytes, encoded as base64. Dimensions, premultiplication and every run are validated before applying. Preview and replay use the same Rust source-over kernel. Coordinates are absolute to the recorded canvas; mismatched canvas dimensions refuse. The recorder's existing storage budget also accounts for coverage payloads.

Selection state remains transient and is not written to `.rz`; saved channels are the persistent coverage workflow. Native format version and supported document capacity are unchanged. Freehand selection, independent mask transforms, refinement, broader history budgets and recovery remain later phases.

## Verification

The full Rust suite, Swift checks, built-app MCP workflows and AppKit source probe pass.

| Check | Result |
| --- | --- |
| `make test` | 768 passed, 0 failed, 1 ignored; 55 result groups. Localhost enabled for existing transport tests. |
| `cargo fmt --check`, `make lint` | Passed. |
| `make typecheck` | Passed, 0 errors; 6 existing warnings. Clang cache redirected to temporary storage. |
| Isolated app build | Xcode Debug build passed with the release Rust core and Swift `-Onone`, whole-module compilation. Optimized Swift output is not qualified by this phase. |
| Phase 2 built-app MCP | All four suites passed: scoped filters/scalar edits, selection history, Actions parity, Quick Mask stroke replay. |
| Phase 1 built-app regression | All three existing save/render/alpha suites passed against the Phase 2 app. |
| AppKit source probe | Passed: selection and geometry history, live-gesture refusal, Quick Mask no-ops/strokes/replay, feather-before-combine, five concrete filter targets, preview equivalence, stale captures, described masks, scalar edits, locks and translated group masks. |

- `core/tests/operation_scope.rs`: scoped result writers, offsets, off-canvas preservation, feathered and alpha-changing output, mask/channel isolation, locks, refusal/no-op and FFI null handling.
- `core/tests/quick_mask.rs`: independent coverage math, raw-preview/RLE-replay equivalence and refusal before mutation for malformed runs/overlays.
- `scripts/verify_phase2.py`: built-app MCP checks for feathered invert/blur on all four target types, scalar fills/gradients, unsupported targets, selection-only dirty state, crop undo, direct-operation/Actions equivalence and individual Quick Mask stroke replay.
- `scripts/selection-history-probe/`: AppKit source probe for real document/UI mutation paths, history and captured preview/commit semantics.

Built-app verification used `RZ_VERIFY_APP=/tmp/rz-phase2-build/Build/Products/Debug/Rasterize.app/Contents/MacOS/Rasterize`. The runners used temporary generated fixtures and Actions directories, dummy credentials, private ports and only processes they launched. Tests cover numerical and workflow correctness on small fixtures; they make no large-document performance or tablet-event timing claim.
