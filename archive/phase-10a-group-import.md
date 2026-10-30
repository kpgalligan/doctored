# Phase 10A: bounded PSD folder-property import

This is a historical slice record. Current 10A status, qualification and explicit
exclusions are maintained in the [bounded closeout](phase-10a-qualification.md).

This slice preserves folder opacity, visibility, recognized blend modes and
open/closed state in supported 8-bit RGB/grayscale PSD layered imports. It fixes
the prior substitution of visible, fully opaque, Pass Through groups. The
existing layer tree and raster decoder remain in use. This does not complete
10A, Phase 10, Photoshop interchange, native-depth exchange, or Phase 9I.

This record describes the original folder-property implementation and its
qualification. The subsequent [ordinary mask slice](phase-10a-masks-status.md)
adds admitted independent raster/folder masks; mask limitations below belong to
this original qualification. The [full raster-rectangle slice](phase-10a-pixels-status.md)
retains complete admitted Raw/PackBits 8-bit RGB/grayscale raster samples and
signed offsets, including off-canvas pixels and hidden RGB. The decoder
placement and off-canvas limitations below describe the earlier implementation;
its receipts remain the evidence for that original slice. Masks, advanced
constructs, native-depth PSD and independent exchange qualification remain open.

## Contract and source audit

The [Adobe file format specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)
places opacity and flags on each layer record. Its `lsct` block identifies an
open folder, closed folder, bounding divider or ordinary layer, and optionally
supplies a blend key and scene subtype. The optional blend key takes precedence;
when absent, the explicit ordinary record blend key is used. A Normal folder
therefore stays isolated and a Pass Through folder stays Pass Through.

The locally installed `psd` 0.3.5 source, especially
`sections/layer_and_mask_information_section/mod.rs`, reverses file records and
assigns group IDs when entering folders. It keeps the name and parent from that
folder but constructs the eventual `PsdGroup` from the closing divider. Its
`lsct` reader discards the optional blend key. This slice retains those folder
properties before decoder entry and verifies the resulting group IDs/parents
and raster parent sequence. Visibility uses the existing import convention:
record flag bit 1 set means hidden. Closing-divider properties never substitute
for folder properties; matching duplicate names is unnecessary.

`psd_group_records.rs` performs only bounded framing and property interpretation.
It does not decompress pixels or implement another compositor. Recognized keys
use the existing blend-mode conversion and the existing group compositor;
unknown folder keys refuse layered import. Existing layer locks, group clipping,
mask payloads, Blend If ranges and other Photoshop blending options remain
unsupported. Detected folder masks, clipping and scene subtype produce specific
losses; uninterpreted styles and other live constructs remain general limits.

## Framing, limits and fallback

Both preflight and layered open use the same record walk before the third-party
parser. Classic PSD section/extra-data/channel lengths, signatures, divider
shapes and nesting must be coherent. Duplicate/unknown divider records,
unsupported `lsdk`, unknown folder blend keys and malformed Unicode-name counts
refuse. The Unicode count must stay within its own `luni` block, because the
dependency otherwise reads ahead and then seeks back, potentially manufacturing
names from later records or channel bytes.

The walk admits at most 2,048 raw records, 1,024 imported rows (the existing
native row capacity), 56 channels per record, 64 MiB aggregate extra data,
65,536 additional blocks and 128 nested folder records. Counts, checked lengths
and group structure are validated before decoder entry. This bounds this
metadata slice; it does not introduce a whole-file streaming decoder or a new
application-wide import memory reservation. Existing canvas and codec limits
continue to apply. Additional blocks must use the even declared lengths the
specification describes and the decoder consumes verbatim.

The existing ten-level editing limit still dissolves deeper folders and reports
that loss; their opacity/blend/visibility cannot survive removing those rows.
Pathological nesting above 128 refuses before the legacy recursive emitter.
Rejected layer records do not invalidate the independent saved-composite path.
No failed layered import silently substitutes composite pixels. Both choices
continue to retain the existing supported RGB ICC/PPI metadata.

## Independent checks and reproduction

`core/tests/psd_group_import.rs` constructs small PSD byte streams independently,
then exercises the existing report/open/render/native-save/native-open C APIs.
It checks hidden, zero and fractional folder opacity; conflicting closing
properties; Multiply, Normal isolation and Pass Through/backdrop behavior;
nested and empty/sibling folders; duplicate names; minimal/extended dividers;
Unicode names; native reopen; explicit unsupported losses; malformed framing;
record/block/metadata/depth limits; and captured-state/source-file preservation
on refusal. Expected pixels come from independently written source-over and
multiply arithmetic with one final byte rounding, not importer output.

The existing `psd_tests` regression that intentionally asserted the former loss
now asserts the retained folder properties. Existing `psd_import_notice` tests
continue to cover RGB ICC/PPI, masks/off-canvas reporting, raw/PackBits saved
composites, unsupported color/depth and stale fingerprints.

```sh
RZ_PHASE10A_FIXTURES=/private/tmp/rz-phase10a-group-fixtures \
  cargo test --release --manifest-path core/Cargo.toml \
  --test psd_group_import --test psd_tests --test psd_import_notice
cargo fmt --manifest-path core/Cargo.toml --check
cargo clippy --manifest-path core/Cargo.toml --all-targets -- -D warnings
```

Local verification: all 26 focused PSD tests passed (13 new group tests,
8 compatibility tests and 5 existing PSD regressions); formatting, whitespace
checks and all-target Clippy with warnings denied passed. The ten-file handoff
matches the integrated source hashes in `/private/tmp/rz-phase10a-source-freeze.json`.

The optional export writes seven 1×1 PSDs with adjacent `.psd.json` manifests
containing authored expected group properties, independent layered RGBA values
and deliberately distinct saved-composite pixels. No external image data,
licenses or dependency fork are introduced. Independent Photoshop
reopen and masked/off-canvas/native-depth fidelity remain separate open gates.

## Integrated qualification

- Full Rust regression: **1,423 passed, 0 failed, 2 existing ignored**, across
  146 result groups. Log: `/private/tmp/rz-phase10a-root-rust-qualified.log`.
  The first sandboxed invocation stopped at the loopback transport test; the
  complete rerun with local socket access passes. No test was removed or weakened.
- Authoritative `make typecheck` and normal-LTO `make app` pass. Logs:
  `/private/tmp/rz-phase10a-root-typecheck.log` and
  `/private/tmp/rz-phase10a-root-app.log`. All 858 captured core/app/build/test
  inputs remained unchanged through the build; the separately reviewed app
  verification helper completes the final 859-input capture.
- The immutable Release application passes **seven fixtures / eight check
  groups** through [the external verifier](../scripts/verify_phase10a_groups.py):
  explicit import preflight, independent pixel expectations, exact folder
  metadata and row topology, one-edit Undo/Redo, byte-identical native round
  trips, unknown-blend refusal with the expected reason, explicit composite
  pixels and stale-fingerprint refusal. Receipt:
  `/private/tmp/rz-phase10a-release-verification-qualified/result.json`.
- Executable SHA-256:
  `771c15a3fa6f79e040cb66d8605b73df470f860f30838861506259dba3c1d4b4`;
  normal production archive SHA-256:
  `812f463c7e564f17ca435d45456d9a8b716c28ce62c885754654c2b03de07bec`.
  App: `/private/tmp/rz-phase10a-release-candidate/Rasterize.app`.
  All 859 final inputs match `/private/tmp/rz-phase10a-final-inputs.json`.

Independent review caught and verified the Unicode block containment fix.
Review of the app verifier added exact row topology and specific refusal
reasons. Its initial sandboxed launch could not bind a port and launched no
app; the qualified run used an isolated local port, dummy credentials and
private Actions/recovery/preset stores, and its owned process exited.

No FFI signature, native format or MCP tool-count change. This completes the
folder-property slice; Phase 10A and the Phase 9 cutout gate remain open.
