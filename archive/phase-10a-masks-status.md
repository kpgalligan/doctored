# Phase 10A: ordinary independent PSD raster masks

This is a historical slice record. Current 10A status, qualification and explicit
exclusions are maintained in the [bounded closeout](phase-10a-qualification.md).

This bounded slice retains ordinary 8-bit PSD raster masks on raster layers and
retained folder records. It depends on the separately qualified mask outside
coverage foundation. It does not complete 10A, Photoshop exchange, high-depth
PSD, vector masks or advanced mask behavior.

The later [relative raster-mask continuation](phase-10a-relative-masks-status.md)
adds ordinary raster-relative placement with checked one-time normalization.
Relative folder masks remain unqualified. Absolute-only restrictions and the
original 53-case receipts below describe this earlier qualification; the later
corpus replaces its former raster-relative loss with a relative-folder loss.

## Admitted meaning

The exact mask subset is one `-2` channel paired with a 20-byte mask metadata
body, absolute signed bounds, outside default0 or255, and a flags byte containing
only the optional disabled bit. Raw and PackBits channels use the mask's own
nonempty rectangle, independently of the layer rectangle and canvas. Every raw
coverage byte is retained, including off-canvas coverage; raster RGB/source alpha
remain unchanged. Disabled masks remain editable and persistent while their
contribution is bypassed. Density1 and feather0 are the only imported parameters.

The [Adobe PSD specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)
distinguishes position relativity from a content link. The first slice therefore
leaves relative mask placement unqualified. Every retained mask is explicitly
independent (`linked=false`), and the file report discloses that PSD linkage was
not inferred or preserved. Later raster or group placement changes leave that
mask in its own canvas location. A group's native raw offset records its mask
anchor, following the existing native group convention; visible group placement
and movement deltas still use the descendants' buffer union.

The original `psd_group_records` walk remains the only layer-record parser. It
lends mask metadata and channel slices to `psd_raster_masks`; no channel/sample
allocation occurs during classification. Folder masks follow the verified
folder IDs, including duplicate names and empty groups. Masks on folder rows
that the existing depth limit dissolves are explicitly reported as losses.
A mask on a discarded bounding divider still refuses the layered choice.

## Unsupported forms and malformed admitted data

Missing descriptor/channel pairs, non-20-byte or extended bodies, nonendpoint
outside defaults, relative/rendered/obsolete-invert/unknown flags, mask parameters,
`-3` channels, vector masks and advanced mask blocks (`vmsk`, `vsms`, `lmgm`,
`vmgm`) are specific per-file losses. Unsupported mask compression, including
ZIP, also remains a disclosed loss. These payloads stay borrowed and are not
sent to a mask pixel decoder. This preserves the earlier explicit layered
subset choice for unsupported masks; it does not claim their appearance.

Once an ordinary descriptor and Raw/RLE codec are admitted, invalid rectangles,
Raw lengths, row tables, PackBits packets or trailing channel data refuse the
layered choice. They never become an implicit dropped mask or a stored-composite
fallback. Every retained channel is validated before any output plane is allocated.
The independently stored composite remains a separate explicit choice after a
layered refusal. The compatibility report distinguishes retained observations,
actual mask losses and general unsupported features.

The two trailing bytes in the old 20-byte body are consumed as padding, with no
semantic interpretation. Extended real-mask records and parameter flags do not
silently acquire ordinary-mask meaning.

## Admission and implementation

Raw copies and the existing `psd_container::decode_composite` PackBits seam own
sample decoding. Raster and mask channels share `psd_layer_pixels::validate_channel`
and the existing `validate_packbits` framing-only validator. No second decompressor
or interpretation through the metadata-only PSD decoder is added.

Every retained mask admits widened signed rectangle arithmetic, the codec's
30000-pixel side limit and100MP plane limit, plus the shared native aggregate
mask limit. These checks run before channel validation and pixel allocation,
including masks on folders and disabled masks. The layered decode forecast becomes
`4I + 8M + 4P + A + 4L + 2E + 4C + 64MiB`: `A` is aggregate retained mask samples,
`L` is the largest raster or mask plane, and `E` includes retained mask channels.
The other quantities retain the preceding raster slice's definitions. The
2GiB envelope uses saturating arithmetic. Tiny rasters with enormous compressed
masks cannot bypass it. Imported documents also pass the native save preflight.

This bounds layered decode work after the source file has already been read;
it does not claim admission of the FFI source read or unrelated host documents.
The independent saved-composite choice keeps its separate existing limits.

## Qualification

`core/tests/psd_raster_masks.rs` exercises report/open, mask property inspection,
mask enabling, independent placement changes, projection, native save/reopen,
exact raw mask equality and refusal purity through the existing FFI. The fixture
builder and expected scalar/source-over calculations are independent of product
mask/compositor helpers. Distinct saved-composite sentinel pixels prove that
layer import never silently consumes that alternative.

Fixtures cover Raw, literal/repeat/no-op PackBits, different mask/raster heights,
signed/off-canvas/extreme coordinates, both outside defaults, disabled masks,
grayscale, fractional source alpha and hidden RGB. Folder cases distinguish
isolated from Pass Through evaluation, preserve closed disclosure and duplicate
folder identity, and pin descendant-origin moves with an independent mask.
Unsupported-form fixtures pair deliberately malformed samples with their
unqualified descriptors to prove those samples never enter decoding. Admitted
malformed rows, extents and memory-pressure cases refuse before pixels decode.

```sh
RZ_PSD_MASK_FIXTURES=/private/tmp/rz-psd-mask-fixtures CARGO_BUILD_JOBS=2 \
  cargo test --release --manifest-path core/Cargo.toml \
  --test psd_raster_masks --test psd_layer_pixels --test psd_group_import \
  --test psd_import_notice --test psd_tests
```

Fixture export writes `.psd` plus adjacent `.psd.json` manifests. Expected layers
are bottom-first; each optional mask contains raw `coverage`, `size`, `offset`,
`outside_coverage`, `enabled`, `linked`, `density` and `feather`. Raster and raw
folder row properties, expected projection, distinct stored composite and optional
moved projection are independently authored. Refusal manifests instead declare
`layered_available:false` and a specific reason substring.

### Isolated qualification receipt (2026-09-13)

All53 focused normal-LTO Release tests pass:12 new ordinary-mask tests and41
existing PSD regressions. All-target Clippy with warnings denied, formatting
and whitespace checks pass. Final logs are
`/private/tmp/rz-psd-raster-masks-qualified.log`,
`/private/tmp/rz-psd-raster-masks-clippy-final.log`,
`/private/tmp/rz-psd-raster-masks-fmt.log` and
`/private/tmp/rz-psd-raster-masks-diff-check.log`.

The tests use the complete compile-ready mask-foundation preview, captured with
all446 core inputs in `/private/tmp/rz-psd-raster-masks-qualified-inputs.json`.
Those inputs remain unchanged through the final qualification. Fixture export
contains53 independently authored cases:14 ordinary imports,21 explicit losses
and18 layered refusals. All106 PSD/manifest hashes are captured in
`/private/tmp/rz-psd-mask-fixtures-inputs.json`.

An initial compilation ended without a completion receipt. A tracked run then
passed all53 tests; Clippy requested three test-only clone-to-slice cleanups.
The final unchanged-input run above passes after those cleanups. No application
was launched from this worktree, and no external image data was introduced.

The root integrator owns independent review and final combined-source regression,
production Release/UI/MCP, native recovery and Actions qualification. The isolated
receipt does not substitute for those checks or imply Photoshop reopen fidelity.

## Integrated qualification (2026-09-13)

- Complete Rust regression: **1,470 passed, zero failed, two existing ignored**,
  across150 result groups. The shared Release test cache uses LTO disabled and
  serialized tests, retaining every cancellation assertion. Log:
  `/private/tmp/rz-psd-masks-root-rust.log`.
- Authoritative `make typecheck`, normal-LTO production `make app`, all-target
  `make lint` with warnings denied, formatting and whitespace checks pass.
  Logs: `/private/tmp/rz-psd-masks-root-{typecheck,app,lint}.log`.
- The production [PSD mask verifier](../scripts/verify_phase10a_masks.py) passes
  the complete frozen53-case corpus:14 ordinary imports,21 specific per-file
  losses and18 layered refusals. Exact mask/source samples, native28 versus10
  selection, canonical MOUT indexes, original/moved projections, five independent
  movement witnesses and the separate stored composite match authored expectations.
  Fifteen mask-property workflows retain raw samples and outside state through
  recorded Actions, one-edit Undo/Redo and exact native reopen. Fixture hashes
  remain unchanged. Receipt: `/private/tmp/rz-psd-masks-root-mcp/result.json`.
- The same immutable app passes all ten preceding byte/native mask-foundation
  cases, including29-over26/27 nesting, and all13 original PSD raster fixtures.
  Receipts: `/private/tmp/rz-psd-masks-root-foundation/result.json` and
  `/private/tmp/rz-psd-masks-root-pixels/result.json`.
- The combined-root AppKit probe passes the actual mask density/feather controls,
  Actions, exact-owner Undo/Redo, native reopen, snapshots and private recovery.
  All902 physical inputs remain unchanged; independent readers verify all six
  direct/snapshot/recovery archives, including exact ICC, raw source/mask codes
  and outside state. Its authored native fixtures qualify the shared mask/archive
  paths; the53-case production run separately qualifies PSD intake. Receipt:
  `/private/tmp/rz-psd-masks-root-source-probe/result.json`. Probe executable SHA-256:
  `a76da0c7c1fdc646acc3df38f67a1b2f27e1c8abb1ccb1d9c6f55fbecba39e9d`.
- All859 captured integrated app/core/header/build/test inputs and all five
  captured verification helpers remain unchanged. Captures:
  `/private/tmp/rz-psd-masks-root-inputs.json` and
  `/private/tmp/rz-psd-masks-root-helper-hashes.json`.
  Production executable SHA-256:
  `9592691139631b98db79111b713377e3665a56bd87185783a34e46397454d96f`;
  normal-LTO archive SHA-256:
  `f8da6313452b621949860f6fb9d392740d3d3a6047a72bb07ddf92b751dfc507`.
  Immutable app: `/private/tmp/rz-psd-masks-release-candidate/Rasterize.app`.

Independent review found no remaining blocker in classification, record identity,
mask admission, shared decoding, native group anchors or the independent oracles.
The existing PSD dependency skips bounded mask metadata in the metadata-only
view; unsupported forms do not acquire an accidental second interpretation.
Every owned app/probe process exited, using dummy credentials and private local
ports, Actions, recovery and preset stores. No additional native version, FFI
signature or MCP tool was introduced by this importer slice.

This qualifies the stated ordinary-mask subset. Broader PSD masks, native-depth
PSD, independent-application exchange, full10A/10D and the separate9I prerequisite
decision remain open. The carryover ledger remains18 verified and18 open.
