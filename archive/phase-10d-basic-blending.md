# Phase 10D: basic native layer blending

This bounded slice adds Normal, Multiply and Screen for native RGBA16 raster
layers and isolated groups, plus transitions between isolated and Pass Through
groups. Adjustment layers remain Normal-only. It does not complete Phase 10D,
P7-19 or P7-20 and does not add clipping, styles, Blend If, knockout or exchange
compatibility claims. Phase 9I remains independent and open.

Adjustment layers are no longer Normal-only: the
[adjustment-blending slice](phase-10d-adjustment-blending-status.md) has since given a typed
adjustment layer the same twenty-six non-Normal modes under the byte
`doc.rs::composite_adjustment_into` rule, with conditional native v37/`LBLD` schema7. The
v26/schema1 contract below is unchanged, and knockout is tracked separately as P7-37.

This record describes the original basic blending implementation and its
qualification. The subsequent [seven-mode separable slice](phase-10d-separable-status.md)
adds Overlay, Soft Light, Hard Light, Darken, Lighten, Difference and Exclusion
with conditional native v27/schema2 persistence. The v26/schema1 contract and
receipts below retain their original meaning; earlier statements about the
remaining mode set describe this basic slice. Both slices leave the broader
10D, P7-19/P7-20 and exchange gates open.

## Meaning and implementation

The captured profile's encoded RGB uses the existing `blend.rs` W3C f32
source-over kernel. Raster placement, native masks and opacity gate contribution
once. Isolated groups retain straight f32 color/alpha across every nested boundary;
only the final requested projection stores u16. Pass Through evaluates children
against the enclosing backdrop, then gates their combined premultiplied change
through the established group kernel. No byte preview, transfer-curve conversion,
new blend formula or intermediate group quantization enters the native master.

The metadata-only JSON edit is
`{op:"layer_blend",version:1,layer:<postorder index>,blend_mode:<RzBlendMode>}`.
Modes 0/1/2 are Normal/Multiply/Screen; mode27 is group-only Pass Through.
Unknown versions, fields, modes and incompatible targets refuse during planning
and editing. No input samples are consumed. Exact no-ops return no new owner.
The existing bounded metadata admission and single-use cancellation tokens apply.

Raster/source/artwork data and masks stay shared. Descendant edits, duplication
and native retouch ancestor rebuilding preserve the parent mode. The existing
Layers control, MCP `set_layer_properties` and Actions share the same mode set.
A typed compound property request prepares every named property through throwing
APIs before one captured publication: a later refusal cannot publish an earlier
blend change. Unsupported collapsed native group disclosure refuses explicitly;
`open:true` is a known no-op. Undo remains owned by the central document path.

## Native format

RZDC26 prepends its version to the prior compatible typed version/body, including
version25's channel-properties wrapper when present. After the complete older
body and tails it appends `LBLD`, u32 schema1, u32 count and ordered pairs of
u32 postorder layer index plus u32 blend mode. Only nondefault Multiply/Screen
records appear. Existing Normal and Pass Through record meaning remains intact.
Schema1 explicitly declares the encoded-RGB/f32 blend interpretation above.

The added encoded capacity is exactly `16 + 8 * record_count` bytes: four bytes
for the outer version and twelve bytes for the table header. Default-only
documents retain their prior writers; setting the final new mode back to Normal
or Pass Through removes the wrapper. Every older reader remains in place.
All layer references, supported modes/schema, unique ascending indices and target
kinds, including hidden records, validate before raster/source PNG decoding.
Earlier retained-source, resource/live-filter, artwork and channel tails retain
their existing framing and accounting.

## Qualification and limits

`core/tests/precision_layer_blending.rs` exercises actual JSON FFI requests,
independent f64 source-over across every u16 coverage code, partial/tiny alpha,
mask density, translated sources, nested groups and Pass Through. It checks
ancestor preservation, exact raw/hidden samples, no-ops/refusals/admission,
active render cancellation, conditional v26 over v25, retained-master/artwork
bodies and corrupt hidden-record blend preflight before PNG decoding.

The independent Python reader in `scripts/live-filter-probe/native.py` parses
v26 and its strict LBLD table without calling the product. Its synthetic decoder
qualification covers a valid table and eleven malformed/atomic-refusal cases.

The AppKit source probe (`scripts/selection-probe/verify.py --checks 'native layer
blending' --keep`) uses the actual Layers popup and MCP handler, records/replays
an Action, verifies one Undo and exact native owner restoration, refuses both
unsupported-mode and later-property failures atomically, and checks native
reopen, snapshots and private recovery. It reuses a genuine 3×2 RGBA16 fixture.

Integrated validation and final production Release checks are recorded below;
the source probe does not substitute for the production checks.
No external application's blend or exchange interpretation is asserted here.

### Isolated-slice receipt (2026-09-13)

- Seven dedicated release Rust tests passed, including the full native coverage
  sweep and active cancellation. `make lint` and `cargo fmt --check` passed.
- `make typecheck` passed with a private Clang module cache; only the existing
  AVFoundation deprecation warnings remain.
- The complete `native layer blending` AppKit source probe passed. Its retained
  directory is `$TMPDIR/rasterize-selection-probe-ovqllv0n/`.
- The independent Python decoder passed eleven malformed-tail cases and parsed
  the probe's actual native26 save, checking every raw u16 sample and property.
- An independent static review found a compound-property partial-publication
  bug during implementation. The final throwing preparation path and actual
  reverse-order name/offset/type/opacity refusal probes close that finding;
  the reviewer reported no remaining blocking finding.

Temporary receipts are `/private/tmp/rz10d-tests.log`, `rz10d-lint.log`,
`rz10d-typecheck.log`, `rz10d-probe.log` and `rz10d-independent-fixture.json`.
The duplicate broad Rust run was cancelled during compilation at the root
integrator's request; final integrated Rust and production Release checks remain
owned by that integrator and are completed below. These isolated receipts do not
close the full phase.

### Integrated qualification

- Full Rust regression: **1,430 passed, 0 failed, 2 existing ignored**, across
  147 result groups. Required all-target Clippy, formatting and diff checks pass.
  Logs: `/private/tmp/rz-phase10d-root-rust.log` and
  `/private/tmp/rz-phase10d-root-lint.log`.
- Authoritative `make typecheck` and normal-LTO `make app` pass. Logs:
  `/private/tmp/rz-phase10d-root-typecheck.log` and
  `/private/tmp/rz-phase10d-root-app.log`. All 879 captured implementation,
  build, test and probe inputs stayed unchanged; two additional captured
  guidance files received documentation updates only. The complete final
  881-file capture matches `/private/tmp/rz-phase10d-final-inputs.json`.
- A new integrated AppKit source probe links the **production Rust archive**.
  Actual Layers controls, MCP, Actions, Undo/Redo, native reopen, snapshots,
  private recovery, group edits and Pass Through transitions pass. All **701
  source/header/helper/build/library inputs** match before and after, with
  physical input snapshots retained. Receipt:
  `/private/tmp/rz10d-integrated-source-probe/result.json`; probe directory:
  `$TMPDIR/rasterize-selection-probe-qk6d49c7/`.
- The [production verifier](../scripts/verify_phase10d_blending.py) independently
  authors a 3×2 RGBA16 input, duplicates it and checks both blend modes with
  fractional opacity against f64 W3C arithmetic. Both quantized projections
  match exactly on these samples. It also verifies unchanged raw RGB/source
  alpha including hidden colors, requested names and opacity, exact v26 native
  round trips, one Undo, Redo and Actions replay. Six specific early/later
  property refusals preserve native bytes, history, selection and dirty state.
  Receipt: `/private/tmp/rz-phase10d-release-verification/result.json`.
- The **same final executable** also passes all seven byte PSD fixture cases
  from the preceding 10A slice, including property/Undo and native round trips.
  This checks the new shared host property dispatch against the byte workflow.
  Receipt: `/private/tmp/rz-phase10d-release-psd-regression/result.json`.
- Immutable app: `/private/tmp/rz-phase10d-release-candidate/Rasterize.app`.
  Executable SHA-256:
  `acaecf08a8eb16ecdf4f9c9491bd8dca9576c4e6a5f8327c060464f41025c040`;
  production archive SHA-256:
  `f680121c4ad85c8ee9080c97e8c42b4048696fa2a9ac3b77f20eafd1a48f6d12`.

All owned app/probe processes exited. App verification used dummy credentials,
private Actions/recovery/preset stores and isolated local ports; the source
probe also uses its own preferences domain. Initial isolated probe attempts
found helper lookup/argument/expected-error mistakes, corrected before the
qualified runs; they did not change the pixel or refusal requirements.

Documents using the new blend metadata require the native v26 reader. Existing
default-only writers and earlier format readers remain supported. The existing
MCP catalog and Actions schema are unchanged. This completes the limited native
Normal/Multiply/Screen and Pass Through transition slice, with 18 carryovers
verified and 18 still open. Remaining blend modes, clipping, Blend If, styles,
knockout, exchange validation and the original 9I gate remain open.
