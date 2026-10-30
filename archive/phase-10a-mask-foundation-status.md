# Phase 10A — mask outside coverage foundation

This is a historical slice record. Current 10A status, qualification and explicit
exclusions are maintained in the [bounded closeout](phase-10a-qualification.md).

The later [four-mode qualification](phase-10d-four-modes-status.md) explicitly
adds blend30 as an inner body of mask29. The version list below records this
foundation’s original qualification; its outside-coverage semantics remain intact.

This is the model prerequisite for ordinary PSD raster masks. It does not itself
import PSD masks or close Phase 10A. Phase 9I and the Phase 7 carryover decisions
remain unchanged.

## Qualified contract

`MaskProperties.outside_coverage` declares raw coverage outside the independently
stored mask rectangle. Its admitted values are 0 and 255, with 0 as the historical
default; native u16 masks promote those endpoints to 0 and 65535. Other values
refuse. Stored pixels, raw mask dimensions, signed placement, enabled state,
linking, density and feather retain their separate meanings.

Byte and native rendering, effective-mask reads, raw mask reads on the canvas,
group gates, mask thumbnails and Apply Mask use this declared background. Density
still computes `1 - density * (1 - raw coverage)`. Feather evaluates the existing
Gaussian/multiresolution kernel on the finite complement for a white background,
then complements the derived result. This avoids a dark boundary where the shared
blur expands its workspace with zeros. It adds no raw-plane edits or second blur
algorithm. Native planning counts the two additional complement passes.

Derived-mask, aligned-mask, styled-shape and content-bounds caches include the
background in their semantic keys, even when cloned properties share raw buffers
and cache owners. Independent mask affine and perspective resampling, including
linked retained-source transforms, use the existing spatial walk with a constant
background. Nearest samples remain exact; filtered samples accumulate deviations
from that background. Resize, crop, canvas permutations and placement changes
retain the background alongside the existing numeric feather policy.

Existing scalar editing operates on the finite raw mask storage. In particular,
its Invert command changes those stored samples and retains the outside default;
it has not been redefined as an operation on an infinite plane. Replacing an
entire mask through the established creation/replacement APIs deliberately makes
a new default-zero mask. Ordinary property edits, transformed masks and edits of
existing raw samples preserve the stored background. No new UI control or MCP
mutation is introduced. The existing byte properties JSON, typed inspection and
host `get_mask_properties` report additive `outside_coverage: 0|255`.

## Native persistence

RZDC28 is a conditional byte wrapper; RZDC29 is its typed counterpart. The wrapper
inserts one little-endian u32 version immediately after `RZDC`, before the exact
prior version and body. After all prior tails it appends:

- ASCII `MOUT`, u32 schema 1, u32 nonzero count;
- that many strictly ascending u32 bottom-first/postorder record indexes.

Each listed record must own a mask and receives outside coverage 255. Unlisted
masks receive 0. Disabled masks remain eligible. Empty tables, duplicate or
unordered indexes, missing-mask references, invalid indexes, unsupported schemas,
invalid inner versions and trailing bytes refuse during borrowed preflight before
PNG decoding or raw mask allocation. The sparse table adds `16 + 4 * count` bytes,
including its outer version word, to the writer's prior capacity plan.

The foundation's byte wrapper accepts only versions 10, 14, 16, 18, 21 and 23.
Its typed wrapper accepts only 13, 15, 17, 19, 20, 22, 24, 25, 26 and 27. A typed
blend table remains inside the mask wrapper, with `LBLD` preceding `MOUT`; its
schema does not change. Future blend additions must explicitly admit any new
inner version rather than widen an old reader's meaning. A document with no
outside-255 masks uses the exact prior writer and bytes. Sources, raw resources,
artwork descriptions and channel metadata retain their existing inner framing.

## Verification

`core/tests/mask_outside_coverage.rs` uses independent Gaussian, scalar and
bilinear references and explicit wire assertions. It covers exact byte/native
raw codes, hidden source RGB, finite and far-field feather, cache aliasing,
ordinary property edits, disabled/off-canvas masks, cancellation, exact native
wrapper fallback, malformed-table refusal before deliberately corrupt PNG
inflation, and group postorder references. The existing independent-mask suite's
legacy migration JSON expectation now explicitly asserts outside coverage 0.

`scripts/selection-probe/mask_outside.swift` exercises the actual existing Mask
Properties controls with independently authored byte28 and typed29 documents. It
checks transient preview, exact raw/source retention, the existing recorded
Action, exact-owner Undo/Redo, native reopen, snapshots and private recovery.
The runner generates fixtures through `scripts/mask-probe/fixtures.py`, or accepts
an explicit `RZ_MASK_FOUNDATION_FIXTURES` directory. The root production verifier
uses those independent fixtures to qualify the built application over MCP.

Detailed final run receipts are recorded below after integration verification.

### Isolated implementation qualification — 2026-09-13

- Worktree: `/private/tmp/rz-mask-outside-worktree`, based on `50cfea6`.
  No commits were made in that worktree. Root owns integration and commits.
- `cargo fmt --check`, all-target Clippy with warnings denied, and `make typecheck`
  passed. Typecheck had zero errors and retained existing host warnings.
  Logs: `/private/tmp/rz-mask-foundation-lint.log` and
  `/private/tmp/rz-mask-foundation-typecheck.log`.
- Normal-LTO Release tests: 37 passed, zero failed across
  `independent_masks` (11), `mask_outside_coverage` (8), `precision_geometry` (8)
  and `precision_masks` (10). Log:
  `/private/tmp/rz-mask-foundation-release-tests-v2.log`.
  The strengthened feather test additionally reads across the derived-padding
  boundary on a 400×3 canvas; all eight final focused tests passed again in Release:
  `/private/tmp/rz-mask-foundation-strengthened-release.log`.
  The earlier Release attempt exposed only the expected additive JSON field in
  the old migration oracle; that expectation was corrected before the passing run.
- The actual mask-controls source probe passed using the integrated root's normal-LTO
  archive, SHA256 `ad95014653326015072e75ce0f9b3fe6a54c9a94719a82497bde6e56b3a9c1b5`.
  Before launch, 680 Rust/Swift/header/Cargo/build files matched root byte for byte.
  The probe used a dummy key, private preference domain and private Actions,
  presets and recovery directories. Its owned process exited successfully.
- `/private/tmp/rz-mask-foundation-source-probe-v1/result.json` records 897 physical
  input copies and no changed input hashes. `inputs.json` maps every copy to its
  source and hash. Independent post-check helpers are copied under `check-inputs/`.
  `artifact-hashes.json` records exact output and verification-helper hashes.
- Probe executable SHA256:
  `04975230f16a01901d86c03579fddfa3941ca1927d235d5063f3c0ca5453be9b`.
  Probe workspace:
  `$TMPDIR/rasterize-selection-probe-elb0uney`.
  An independent native/PNG decoder verified all six generated direct,
  snapshot and recovery archives: exact source RGBA, mask samples, ICC bytes,
  offset, outside coverage, density and feather. Byte archive SHA256:
  `877bfd73a961ec066950755a132ebf50c0bf57a15e19eb8901ecec96042a9681`;
  typed archive SHA256:
  `a2d989ff4b8b7fc3053cd8bf9404b0da8e0e406117c4b2a29333fa564e6a17cb`.

Root's full regression, production application/MCP receipts and final commit
belong to the integration record below. These implementation receipts do not
claim PSD mask import or complete Phase 10A.

## Integrated qualification (2026-09-13)

- The final complete Rust suite passes **1,458 tests, zero failures and two
  existing ignored tests**, across149 result groups. The final run uses Release
  with LTO disabled in the shared test cache and `RUST_TEST_THREADS=1`.
  Log: `/private/tmp/rz-mask-foundation-root-rust-qualified.log`.
- Authoritative `make typecheck`, normal-LTO production `make app`, all-target
  `make lint` with warnings denied, formatting and whitespace checks pass.
  Logs: `/private/tmp/rz-mask-foundation-root-{typecheck,app,lint}.log`.
- The immutable production app passes all ten independently authored native
  fixtures through `scripts/verify_phase10a_mask_foundation.py`: both depths,
  outside0/255, disabled masks, off-canvas rectangles and groups. Every original,
  density-edited and translated projection matches exactly (**zero code error**).
  Raw source/mask samples, hidden RGB, ICC, placement and outside metadata remain
  exact through property Actions, one-edit Undo/Redo and native reopen. Eight
  additional checks qualify29-over26/27 blend/mask composition and exact Undo.
  Ten malformed independent-decoder tables refuse atomically. Receipt:
  `/private/tmp/rz-mask-foundation-root-mcp-final/result.json`.
- The same production app passes all13 prior PSD raster fixtures, including
  off-canvas movement, exact stored pixels, native reopen and the explicit
  composite after a malformed layered refusal. Receipt:
  `/private/tmp/rz-mask-foundation-root-pixels-final/result.json`.
- All856 captured integrated app/core/header/build/test inputs match their final
  hashes. The final feather test strengthens sampling across the derived padding
  boundary; the complete qualified run includes it. Source capture:
  `/private/tmp/rz-mask-foundation-root-inputs.json`. Helper hashes:
  `/private/tmp/rz-mask-foundation-root-helper-hashes.json`.
- Production executable SHA-256:
  `c340a97b4fb894cb7d0528a24457c9873d62a8c46dcdab79a473d1e9f3a7d18f`;
  normal-LTO archive SHA-256:
  `ad95014653326015072e75ce0f9b3fe6a54c9a94719a82497bde6e56b3a9c1b5`.
  Immutable app: `/private/tmp/rz-mask-foundation-release-candidate/Rasterize.app`.

The initial complete run also passed. A repeat encountered the unchanged
Bayesian matting cancellation-witness test completing before cancellation could
be issued. Its transient scratch reservation can disappear while the controller
thread is descheduled; this was not a failed response to cancellation. The
unchanged target and final mask tests then passed separately, followed by the
complete serialized run above. The failed run remains recorded in
`/private/tmp/rz-mask-foundation-root-rust-final.log`; no assertion, cancellation
behavior or matting algorithm was weakened.

Independent review found no remaining blocking issue. It checked the background
in every relevant mask/style/content cache, complement feather work admission,
constant-background transforms, strict predecode persistence and exact prior
writers. All owned production and source-probe processes exited. Test launches
used dummy credentials, private local ports and private Actions/recovery/preset
stores. This completes the mask-model prerequisite; PSD import, full10A/10D and
the separate9I prerequisite decision remain open. The ledger stays18 verified
and18 open.
