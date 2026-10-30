# Phase 9F — selected tone and scoped analysis

**Phase 9F is complete within the qualified scope below**, verifying P7-06 and
P7-07. Subsequent [9G](phase-9g-selections-masks-status.md) and
[9H](phase-9h-paint-fill-status.md) qualification brings the current Phase 7
ledger to 18 verified assignments and 18 open. P7-12/9I and Phase 9 remain open.

## Selected native edits

All currently exposed destructive filters have a native raster path: the shared
tone/color suite, legacy Blur and Sharpen, Unsharp Mask, High Pass, Reduce Noise,
Dust & Scratches, Edge Detect, Emboss, Pixelate and Add Noise. Sources remain
straight RGBA16 in the document profile. Canvas u16 coverage crosses the new
selected-edit ABI separately. At the 9F milestone, UI selections promoted their
existing u8 coverage by exactly ×257, without creating new selection precision.
Phase 9G adds authoritative native selection coverage through the same edit path.

Selection zero copies original codes exactly, including hidden RGB and tiny
alpha. Full coverage receives the operator's native result; fractional coverage
mixes in premultiplied space when alpha changes. Tone and the photographic
colour-only operators preserve source alpha. Gaussian Blur and Pixelate filter
alpha; legacy Edge Detect/Emboss retain their opaque gray output semantics and
hidden-RGB luminance reads. Noise keeps its row-major RGB random sequence,
including transparent pixels. Legacy Sharpen's amount remains its historical
Gaussian scale with the existing gain; it is not reinterpreted as Unsharp Mask.
The image-crate Gaussian is evaluated in bounded output tiles with sufficient
halo, through the original sampling algorithm. The photographic operators reuse
the live-filter neighborhoods and combine functions.

Every neighborhood reads the original target storage outside the selected write
region. Automatic tone analysis is a separate choice: `analysis_scope` is
`whole_target` by default, preserving old Actions, or `selection` to weight the
histogram and neutral candidates by captured coverage. Auto Tone/Contrast/Color
share endpoint, neutral-candidate and gamma helpers. Typed analysis has 65,536
native-code bins; selected byte analysis uses weighted native byte bins, while
omitted/whole-target byte analysis retains its historical result exactly.

UI sheets and MCP capture source, target, selection and revision before work.
Native previews and commits use the same selected command. Final publication
checks the capture and owner lifetime, passes through central history once,
and preserves no-op behavior. Retained image/RAW/host masters still require
explicit source rasterization; destructive artwork rasterization remains one
central edit. Native scalar targets and Quick Mask refuse; 9G owns those paths.

## Fine parameter entry

Levels and Threshold have editable normalized/native-code fields in both depths;
the native display scale is 255 or 65,535. Gamma remains dimensionless. Curves
adds fine input/output coordinates and retains fractional coordinates through
reopen. Unit changes leave stored values alone. Existing normalized Levels and
Threshold arguments and Curves' historical 0–255 coordinate meaning remain
unchanged; new Actions retain full numeric values instead of rounding fine
values to four decimal places. Preview and Apply use the same resolved numbers.

## Scoped statistics

Info's source popup selects Composite, Active layer or Below active layer, and
Weight by selection controls histogram and RGB readout coverage. Levels/Curves
plots read the captured target or adjustment backdrop at native depth. Cursor
readouts share the existing debounced statistics worker; source/selection changes
invalidate queued results even when the document pixels and selection bounds
stay unchanged. Native readout text and its normalized tooltip are derived from
the requested source, never from a display image.

`histogram` and `sample_pixel` expose `source: composite|layer|backdrop`. Layer
scope uses canvas coordinates, omits offcanvas storage and includes transparent
black canvas padding. A backdrop requires `below` in 0…layer_count: contributors
with bottom-first record index at or above the boundary are cut, while enclosing
group visibility, opacity, mask and isolation stay in effect. This is a visible
document contribution cut, not the isolated group's internal unmasked backdrop.
Raw group scope renders its children over transparent and excludes its own gates.
Adjustment entries have no raw pixels and refuse layer scope in both depths;
choose their backdrop instead.

The additive core statistics ABI borrows native u16 canvas weights with denominator
65,535. Schema 2 labels `maximum_code` (255 or 65,535), storage and profile. Exact
integer `bin_weight_units` and `weighted_code_sums_rgba` coexist with fractional
display `bins`. Histogram RGB eligibility is alpha greater than zero; alpha is
not an additional weight. Alpha statistics include all positively weighted
positions. Sample means deliberately include hidden RGB at alpha zero, preserving
the established straight-mean readout convention.

`stride` affects display bins only. `exact_zero_rgba`/`exact_max_rgba` count full
scope endpoint pixels with positive coverage; their `*_weight_units_rgba` twins
sum exact coverage. `full_rgb_weight_units` and `full_alpha_weight_units` provide
the corresponding complete denominators. Clipping wedges use complete weighted
endpoints and complete RGB mass, never end display buckets or sampled totals.
Sample windows return `weighted_sum_rgba_codes`, `weight_units`, exact center
codes, means and normalized values; an empty weighted window refuses.

`histogram.selection` defaults true and `sample_pixel.selection` defaults false.
Unscoped byte `sample_pixel` retains its existing byte/hex/HSB/Lab response;
explicit scoped reads and all typed reads return schema 2. Image-only schema-1
RAW inspection remains unchanged and keeps its existing declared stride policy.
Read-only source/toggle changes neither dirty the document nor add Undo.

## Resource and qualification boundaries

Plans admit original owners, captured coverage, output, operator scratch and
parameter parsing before computation. Native dense scans, writeback and filter
tiles check cancellation. Scoped histogram/readout previews register concurrent
scratch reservations; core admission subtracts only the calling preview's own
reservation. Byte group/style/spatial projection retains its existing declared
full-frame noncancellable rendering stage, checked before and after; full scoped
statistical scans themselves check cancellation. Full-frame analysis and large
multilayer interaction remain subject to the existing working-memory limits.
Native legacy Blur is bounded to sigma 64; neighborhood/work plans may refuse
earlier. No document format version or persistent adjustment meaning changes.

## Qualification

- Final `make typecheck` completes with **zero errors**, and production `make app`
  succeeds with normal release LTO. Logs are
  `/private/tmp/rz-phase9f-host-typecheck-final5.txt` and
  `/private/tmp/rz-phase9f-make-app-final5.txt` (on this host `/tmp` resolves to
  `/private/tmp`). Final executable SHA-256:
  `0495bc7007f1822d40cd41445e1062476fe5f045e37d6e669241c8f90611a28b`;
  core archive:
  `866953f047a6a41cfad93a59a682fb158a293f6e4d42da175b7f671a868b258f`.
- Full `make test` passes **1,321 tests across 133 suites**, with zero failures
  and two explicitly ignored cases. The run uses the pinned real retouch-photo
  corpus, isolated release artifacts and disabled test LTO; production app builds
  retain normal LTO. `cargo fmt`, `make lint` with `-D warnings`, and diff checks
  pass. Logs: `/private/tmp/rz-phase9f-full-rust-final.log` and
  `/private/tmp/rz-phase9f-lint.log`.
- `core/tests/selected_adjustments.rs` has **15 passing tests**, including genuine native ramps, exact
  outside/alpha, translated nested targets, every spatial operator, independent
  Gaussian references, legacy Gaussian tile agreement, full LUT33 payloads,
  native weighted Auto, no-op/admission/refusal and active cancellation.
- `core/tests/scoped_statistics.rs` has **seven passing tests** and independently sums fractional native coverage
  including 1/65535, hidden-RGB sample means, translated canvas padding and group
  backdrop contributions. Full endpoint counts/sums remain exact at strides 1/3.
- Final external MCP qualification passes **31 filter cases, eight alpha/offset
  filter cases, three Auto analysis-scope comparisons and 13 independently
  computed statistics cases per depth**. All 30 pre-existing byte filter outputs
  match the copied committed-9E baseline bit for bit. Selected native gate
  comparisons stay within one native code after independent final rounding;
  selection-zero pixels and required unchanged alpha are exact. Actions and one
  Undo, native save/reopen, named snapshots and owned-process crash recovery
  preserve results in both depths. Adjustment-placeholder, scalar-target and
  invalid-parameter refusals pass. Final report:
  `/private/tmp/rz-phase9f-mcp-complete.json`; all 780 inputs remain unchanged with
  an exact file set, and every owned app exits.
- The public MCP reference is regenerated with the existing renderer from the
  final executable's live catalog and exactly compared with a second rendering:
  **180 tools**, unchanged. The final catalog receipt is
  `$TMPDIR/rasterize-phase9f-4z40ox9s/rgba8/catalog.json`.
  Precision annotations now agree with the scoped histogram/readout schemas.
- Actual UI qualification passes **12 groups** against the frozen production
  archive and actual app sources: Levels, Threshold, Blur, Curves, Info and Auto
  commands in both depths. It checks full fine values, synchronized slider labels,
  preview/commit/cancel, exact Actions/Undo, fractional Curves coordinates after
  native reopen, all three Auto menus with both analysis choices, and stationary
  readouts after source, layer or same-bounds coverage changes. All 386 captured
  inputs (381 compiled) remain unchanged with an exact file set; the owned probe
  exits. Final report: `/private/tmp/rz-phase9f-ui-complete.json`. The initial Info
  probe found a byte adjustment-placeholder read; the shared adjustment-kind guard
  and safe/FFI regressions now refuse it consistently in both depths.


## Reproduction

Build the ordinary production app before running the UI or external checks. The
isolated test target disables LTO only for the Rust regression binaries; it does
not replace the release archive linked into the app. macOS UI/service and
localhost-mock access must be available. The probes supply dummy credentials,
private stores and non-default ports and terminate only their own processes.

```sh
RZ_RETOUCH_PHOTOGRAPHS=/private/tmp/rz-phase9c-photographs \
CARGO_TARGET_DIR=/private/tmp/rz-phase9-tests \
CARGO_PROFILE_RELEASE_LTO=false CARGO_BUILD_JOBS=4 make test
make lint
CLANG_MODULE_CACHE_PATH=/tmp/rz-phase9f-clang-cache \
SWIFT_MODULECACHE_PATH=/tmp/rz-phase9f-swift-cache make typecheck
make app
python3 scripts/verify_phase9f_ui.py --require-current \
  --report /private/tmp/rz-phase9f-ui-complete.json
python3 scripts/verify_phase9f.py --require-current \
  --byte-baseline /private/tmp/rz-phase9f-byte-baseline \
  --report /private/tmp/rz-phase9f-mcp-complete.json
```

The byte-baseline directory is a **prior-build precondition**, not an output to
make from the new implementation: it contains the independently exported native
results and passing 30-case `report.json` captured from committed 9E (`694c39f`).
Its executable SHA-256 is
`5ad0d5417a0d721843c207f11e173d13cb8bec6b935f5971a1238d65be5bb76d` and byte-fixture
SHA-256 is `be69a5a4b045d3642f3e1a19581351b00579a4f13574166e504cec459ebb802c`.
Retain that receipt or recreate it using the prior commit and the same fixture
before asserting legacy bit equality. Omitting `--byte-baseline` exercises the
current selected workflow but does not establish the independent prior-build
comparison. The final receipt stores fixture/source/app hashes and scratch paths.
The qualified UI geometry here uses backing scale 1; the page makes no new
large-document latency, Retina-specific layout or HDR claim.
