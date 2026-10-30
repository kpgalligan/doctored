# Phase 9I execution continuation

**Internal preparation improvement; no alpha/foreground quality qualification.**
The original 9I quality gate and P7-12 remain open, and advanced controls remain
disabled. This change closes the native final-display cancellation gap and
adds controlled byte cutout record copying/projection without changing an
estimator, pixel arithmetic, public tool, Actions schema or native format.

## Captured operation and cancellation

`RefinementCutoutPreparation` claims one single-use parent before dispatch and
observes the owning document's actual close notification throughout preparation.
Every worker retains immutable inputs and tokens. The main thread drains each
worker before removing the observer. Cancel after preparation still refuses
publication through the original final guard.

Cutout stages now create the actual progress panel and modal session before
constructing their WorkerTask. This makes Cancel available when work begins,
including the fast byte projection, rather than allowing a worker to finish
while AppKit is still creating its panel. `BackgroundWork.wait` adds a synchronous
main-thread task-factory overload; existing-task callers retain their original
scheduling. Factories return their newly created task immediately and never
abandon a started mailbox by throwing afterward. Cleanup restores the previous
`isWaiting` value, so a nested busy refusal cannot clear an outer wait's state
or replace its occupied worker/scratch ownership.

The additive `rz_job_control_begin_compound` uses the existing core atomic begin
operation. It owns no parent scratch and does not permit a parent to be reused by
another preparation or ordinary core kernel. Actual stages receive fresh child
tokens with independent scratch ceilings. Existing unmodified callers of
`PreparedDocumentEdit` retain their default path.

The native cutout uses child ranges 0–0.25 for assembly and 0.25–1 for display.
Within display, 0–0.85 is authoritative native rendering and 0.85–0.98 is the
explicit byte-preview quantizer. Those stages use the already shared native
renderer and quantizer's cooperative checkpoints. CoreGraphics image creation
has checks before and after its call; its internal work remains noncancellable.
Progress describes pipeline work, not a time estimate. The existing child helper
advances its range floor during cleanup even on error, so cancellation/error
state—not a progress value of 1—determines whether preparation succeeded.

Byte foreground/matte copying retain their existing chunk checkpoints. The
shared image-layer insertion now accepts an internal optional job. Ordinary
callers still use ordinary `Clone`; cutout assembly uses `clone_with_job`, which
copies owned layer/channel strings in UTF-8-safe chunks of at most 64 KiB and
checks between records/guides. Pixels, masks, styles, retained sources, profiles
and document metadata blobs keep their original Arc sharing. Group insertion
and validation are the same shared path as before.

The internal byte output has exactly one visible plain canvas root with Normal
blend, opacity 1 and an enabled default canvas mask; every previous root is
hidden. `doc_cutout_render` checks that shape and drives the existing
`composite_layer_into` and `quantize` in chunks of at most 4096 pixels, passing
each chunk's canvas origin. The identity mask uses `Layer::active_mask`'s raw-Arc
fast path, so chunks do not create full-canvas mask derivatives. The output
allocates capacity once and copies bounded chunks, avoiding full-canvas zeroing.
Its plan includes final RGBA8 output, f32/quantized chunk storage, bookkeeping
and any context-parser workspace. The host separately admits the retained
borrowed alpha input and later CGImage storage, using the already-rendered byte
projection without a second render. Other document shapes refuse this internal
shortcut; general byte renderer behavior and defaults are unchanged.

The metadata-only plan does **not** parse JSON. Matrix/TRC profiles skip hidden
metadata classification entirely, matching ordinary `flattened`. For an
unmodelled profile, the render admits the largest leaf-metadata parse forecast
before invoking the shared `validate_context_with_job`: 256 times that slot's
UTF-8 byte length, plus fixed bookkeeping. This conservative working estimate
covers serde Value arrays, capacity growth, mostly-empty BTree nodes for small
nested objects, copied strings and adjustment-parser temporaries; it is not an
allocator/RSS limit. The maximum native slot is 16 MiB. Validation preserves the
ordinary renderer's refusal for hidden calibrated adjustments and checks before
and after each metadata parse. An individual bounded serde parse remains
noncancellable internally. CoreGraphics creation and bounded structural/ICC
validation retain their own noncancellable calls. No universal cancellation
latency or interruption inside those calls is claimed.

Native foreground RGBA16/source alpha and u16 refinement matte remain independent.
Existing source/profile sharing, registered matte leases, captured document,
selection and trimap revision guards, exact mask16 selection, central one-edit
Undo/Redo, snapshot/recovery and no-op behavior are retained.

Raw unregistered u16 matte bytes remain in both host admission and the native
render/preview `PrecisionWorkContext.additionalRetainedBytes` allowance through
display preparation. A registered `PrecisionScalarBuffer` passes zero extra
bytes because its existing lease already participates in the retained union.
No second payload lease is created; core child scratch ceilings remain the
operation's own estimate. The provided byte projection similarly retains its
unregistered alpha allowance through CGImage admission.

## Initial native-stage verification

The receipts in this section qualify the native cancellation implementation
before the subsequent byte continuation. Integrated receipts follow below.

Focused FFI tests cover NULL, fresh claim, precancellation, no retained scratch,
compound-to-compound reuse, compound-to-ordinary reuse, ordinary-to-compound
reuse, and the existing byte/native cutout assembly and persistence contracts.
The native source probe additionally checks a used parent and a precancelled
parent before dispatch. The live cancellation probe drives the actual captured
AppKit preparation using a genuine 4096 × 4096 RGBA16 source and separately
registered u16 matte. It invokes the actual progress panel Cancel action or
closes the owner during disjoint native-render and byte-preview progress ranges.
It requires live stage 6, positive scratch and interior progress, the core's
`operation cancelled` checkpoint error (not just late parent discard), unchanged
editing state and fully drained worker/modal/scratch ownership.

The focused core suites passed **17 tests**: 10 job-control, 4 native cutout and
3 byte cutout tests. `cargo fmt` and `git diff --check` passed. Authoritative
`make typecheck` passed with zero errors and the existing media deprecation
warnings. The actual source probe passed all four selected checks (10 printed
assertion groups), including the four active-kernel cancellation outcomes below.

| Trigger | Active native stage | Observed parent progress | Child scratch released |
|---|---|---:|---:|
| Actual Cancel action | Authoritative render | 0.4697111875 | 403,701,760 → 0 bytes |
| Actual Cancel action | Byte-preview quantization | 0.90180598 | 68,157,440 → 0 bytes |
| Actual owner close | Authoritative render | 0.47360185 | 403,701,760 → 0 bytes |
| Actual owner close | Byte-preview quantization | 0.891975055 | 68,157,440 → 0 bytes |

Every row observed core stage 6 and returned the exact core checkpoint error
`operation cancelled`. No row accepts cancellation merely after output finished.
The probe invokes the active visible panel's actual Cancel action synchronously,
avoiding `performClick` animation delay. A 2048² preliminary fixture could not
reliably expose the short preview interval to the modal timer, so the final probe
uses an admitted 4096² fixture with unchanged product kernels and polling.
It selects a visible progress window because AppKit can retain closed panels.
These are witness conditions, not a minimum supported canvas or latency promise.

Receipts:

- Focused tests: `/private/tmp/rz-phase9i-cancellation-rust.log`.
- Final typecheck: `/private/tmp/rz-phase9i-cancellation-typecheck-final.log`.
- Final source probe: `/private/tmp/rz-phase9i-cancellation-source-probe-qualified.log`.
- Full source/helper and artifact hashes: `/private/tmp/rz-phase9i-cancellation-qualified-source-freeze.json`.
  All **686** captured inputs and the linked archive remained unchanged during
  qualification. Afterward, the Python runner received a hygiene-only change to
  disable fixture-import bytecode writes; its before/after hashes and successful
  no-cache import check are recorded separately. No compiled product or Swift
  probe source changed for that runner cleanup.
- Probe directory: `$TMPDIR/rasterize-selection-probe-na49r3r7`.
- Probe executable SHA-256: `66063d9d88b34852f607a343eb1eb76503c2366020c3ce981667a483a1c88271`.
- Linked isolated no-LTO archive SHA-256: `36804d2a48a61801745b95ba21734af3e58e80ce883440e449920032f9630ed9`.
  This is a source probe, separate from the normal-LTO production app.

The owned probe exited, using private Actions/recovery/RAW/filter preset data
and dummy credentials. Production Release and broader integration qualification
are consolidated by the phase owner; this receipt does not claim those checks.

## Byte continuation and integrated verification

The byte-focused suites passed **15 tests**: seven new execution tests, three
existing byte cutout tests and five calibrated-adjustment tests. They cover all
65,536 byte foreground/coverage pairs against both an independent opaque-source
oracle and the ordinary full compositor; 5003-wide row/chunk origins; grouped
hidden originals; large multibyte layer/channel strings; exact channel/guide
state and shared pixel/profile/XMP/channel planes; shape/NULL/budget/precancel
refusals; active record/string-copy and shared-render cancellation; exact hidden
calibrated-profile refusal; and a large valid hidden JSON input whose parser
forecast refuses a low budget before the parser or pixel stages run. The same
large metadata input under a matrix profile retains the cheap plan and exact
ordinary projection.

Focused test receipt: `/private/tmp/rz-phase9i-byte-cutout-tests-qualified.log`.

The final integrated source probe passed **six checks / 15 assertion groups**
on the final byte, native, factory and borrowed-retention implementation. It
covers all six live UI cancellation outcomes below; factory throw cleanup;
nested busy refusal preserving the original modal/worker/reservation; successful
dispatch after the panel exists; exact +12-byte unregistered native context
accounting and bounded host/core memory refusal without allocating a giant
buffer; and the prior native/byte output, clean Undo/Redo, selection, snapshot,
recovery, stale/no-op/refusal and disabled quality-gate checks.

| Actual trigger | Active core stage | Observed parent progress | Child scratch released |
|---|---|---:|---:|
| Cancel action | Shared byte projection | 0.369508795 | 68,239,360 → 0 bytes |
| Owner close | Shared byte projection | 0.373815895 | 68,239,360 → 0 bytes |
| Cancel action | Authoritative native render | 0.3809444125 | 403,701,760 → 0 bytes |
| Cancel action | Native byte-preview quantizer | 0.9023772325 | 68,157,440 → 0 bytes |
| Owner close | Authoritative native render | 0.3952633 | 403,701,760 → 0 bytes |
| Owner close | Native byte-preview quantizer | 0.890570665 | 68,157,440 → 0 bytes |

All six rows observed core stage 6 and the exact core checkpoint error
`operation cancelled`. Each retained the captured editing state and drained
worker, modal and scratch ownership. Both large witness canvases are 4096² and
use unchanged memory admission. A proposed 8192² test was rejected by automatic
approval review for potential memory disruption and never ran. The completed
solution makes the progress UI exist before dispatch; it adds no artificial
work delay and requires no increased fixture size or memory limit.

Final receipts:

- Authoritative typecheck: `/private/tmp/rz-phase9i-execution-typecheck-final.log`,
  exit 0 with only existing warnings.
- Actual source probe: `/private/tmp/rz-phase9i-execution-final-qualified-probe.log`,
  exit 0; six checks / 15 groups.
- Full input/artifact receipt: `/private/tmp/rz-phase9i-execution-final-qualified-freeze.json`.
  All **691** captured product/helper inputs and the linked archive stayed
  unchanged through qualification; no generated workspace bytecode cache.
- Probe directory: `$TMPDIR/rasterize-selection-probe-gsb3tfbz`.
- Probe executable SHA-256: `afa91135ff4e4ee0e044bd2970f83b0ae818cc0702bacfba77cbabc20252f8c1`.
- Linked isolated no-LTO archive SHA-256: `2c3976ea638e90643f60057c2225079b362897796d95a6d36210fe98ff3fa7e0`.
- Run-log SHA-256: `38b69339951f6349e1aca401e34f8768b8c7dde6a262f7585155c3e6689a0664`.

The owned probe exited and retained only private test artifacts. This source
probe links the isolated no-LTO archive; the phase owner separately records
full Rust/Clippy and normal-LTO Release/MCP integration receipts.
