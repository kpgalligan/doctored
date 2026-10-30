# Phase 10A: complete admitted PSD raster rectangles

This is a historical slice record. Current 10A status, qualification and explicit
exclusions are maintained in the [bounded closeout](phase-10a-qualification.md).

The later [ordinary raster-mask qualification](phase-10a-masks-status.md) adds
admitted independent masks to this raster foundation. Mask-loss statements below
describe the original raster-only slice and its frozen fixtures.

This slice preserves the complete stored rectangle, signed offset and exact
straight RGBA8 samples of admitted RGB/grayscale PSD raster layers. Pixels may
be partly or wholly outside the canvas; RGB beneath zero alpha remains exact.
Missing transparency means alpha255 inside the raster rectangle. Ordinary RGB
requires its three color channels; grayscale requires channel0, copied into
RGB. Missing/partial color data refuses, rather than inferring grayscale or
manufacturing transparent storage.

The qualified folder-property behavior, names/tree order, clipping flags,
supported RGB ICC/PPI and explicit stored-composite choice remain in place.
This does not finish 10A, Phase10, Photoshop exchange, mask import, native-depth
PSD, ZIP/PSB support or independent Photoshop reopen qualification.

## Bounded seam and primary reference

The [Adobe Photoshop file format specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)
defines signed layer rectangles with exclusive bottom/right edges, per-channel
IDs and lengths including the compression word, and separately compressed RLE
rows with their own byte counts. The existing bounded `psd_group_records` walk
now retains original borrowed channel slices and rectangles along with folder
metadata. It remains the sole layer-record framing implementation.

The installed `psd`0.3.5 library's channel fields/getters are private. Its layer
RGBA renderer places pixels into a canvas, clips off-canvas samples, and floods
absent alpha over that canvas. Its record reader also narrows heights to u16,
discards RLE row tables, treats a zero bottom/right differently when converting
to inclusive bounds, and cannot correctly frame a mask with a different height.
Using its original-channel parser would therefore undermine exact signed
storage before the renderer even runs.

A bounded metadata-only PSD view instead carries each original record's
blend/name/extra bytes in its original ordinal position, with empty rectangles,
zero channels and a tiny RGB canvas/composite. The existing decoder still
interprets names, raster properties and the tree. Its parent/ordinal sequence
must agree with the bounded walk; names never serve as correlation keys. True
canvas dimensions and color/profile/PPI come from the original `Container`,
and original borrowed descriptors own every raster rectangle/channel.

`psd_layer_pixels` copies Raw scalar samples directly. Each admitted PackBits
channel passes through the existing synthetic RGB composite codec seam, shared
with saved-composite import, and its red scalar output is placed in the correct
raw RGBA component. This supports mixed Raw/RLE channels and grayscale alpha
without a dependency fork, second decompressor, general parser replacement,
premultiplication or canvas clipping. `validate_packbits` remains the one
framing-only packet/decoded-width validator; it produces no decoded samples.

## Admission, refusal and disclosed losses

The complete record/channel walk precedes any metadata-wrapper, codec-channel
copy or output-plane allocation. Raster extents use widened signed arithmetic.
Each raster must be nonempty, positive, at most30000 pixels per side and100MP;
aggregate raw layer pixels, including group placeholders, must fit the native
400MP limit. Empty raster records explicitly refuse the layer choice because
native raster storage has no empty-rectangle representation. They never become
transparent1×1 substitutes. The original classic-PSD canvas limit also remains.

Channel IDs must be recognized and unique. Every retained raster color/alpha
channel must contain exactly its rectangle's samples: Raw lengths are exact;
RLE row tables stay inside their channel, each row decodes to exactly its width,
and the channel has no unclaimed data. ZIP/unknown compression on non-mask
channels refuses even on otherwise discarded folder/divider records. Existing
record count, metadata byte/block count, nesting and native row limits remain.

Masks and style/advanced payloads remain unsupported. Mask channels are bounded
slices, disclosed and dropped without decoding their bytes or assuming the
raster's height. This includes arbitrary or ZIP mask payloads, which cannot
influence raster samples. Folder mask presence remains a per-file loss. A mask
on a discarded bounding divider refuses, so it cannot disappear without an
explanation. No mask dimensions, density/feather, links or coverage are imported.

Layer admission additionally bounds a conservative peak at2GiB. In bytes its
forecast is `4I + 8M + 4P + 4L + 2E + 4C + 64MiB`, where `I` is the already-read
source length, `M` the exact metadata spans, `P` aggregate retained raster/group
pixels, `L` the largest raster plane, `E` the largest encoded layer channel and
`C` original canvas pixels. It covers the input, metadata/name/parser copies,
all retained RGBA, one channel's wrapper/codec copies and RGBA scratch, plus a
conservative independent report-composite allowance and record/row overhead.
Saturating arithmetic refuses overflow. Structural and memory checks happen
before expensive channel validation/decode; compressed bytes stay borrowed.

This is admission of the new layered decode work after the FFI has read the
file, not an admission of that source read or unrelated host-retained documents.
After layered refusal the independent saved-composite path retains its existing
separate limits; the new envelope is not a whole-report or application-wide
memory guarantee. A valid stored composite remains explicitly selectable after
any layer failure. Layer failure never silently opens the saved composite.

## Independent checks and reproduction

`core/tests/psd_layer_pixels.rs` constructs PSD bytes independently and calls the
existing report/open/render/native-save/native-open/offset C APIs. Expected raw
samples and canvas samples are authored directly, without using importer output
as an oracle. Coverage includes Raw, literal/repeat/no-op PackBits, mixed channel
compression, partial/absent alpha, hidden RGB, grayscale, a small opaque raster,
negative bounds, wholly off-canvas storage, rectangles ending at zero, signed
extremes, duplicate names and fractional-opacity closed folders. Native reopen
and moving an off-canvas layer into view preserve exact raw storage and captured
source handles/files.

Refusal cases cover missing/partial/duplicate/unknown channels, malformed Raw
lengths, truncated RLE tables/packets, row spill/underflow, trailing data,
unsupported compression, empty/reversed/oversized rectangles, aggregate pixels,
metadata-copy memory pressure and invalid canvas headers. Every layer-refusal
fixture preserves the independent saved composite and captured native bytes.
Differently sized/arbitrary mask payloads stay explicit losses and cannot shift
raster decoding. Existing PSD/group/ICC/PPI regressions remain in the focused run.

```sh
RZ_PHASE10A_PIXEL_FIXTURES=/private/tmp/rz-phase10a-pixel-fixtures \
  CARGO_BUILD_JOBS=2 cargo test --release --manifest-path core/Cargo.toml \
  --test psd_layer_pixels --test psd_group_import --test psd_import_notice --test psd_tests
cargo fmt --manifest-path core/Cargo.toml --check
CARGO_BUILD_JOBS=2 cargo clippy --manifest-path core/Cargo.toml --all-targets -- -D warnings
```

Optional export writes small PSDs plus adjacent `.psd.json` manifests. Admitted
manifests contain `canvas`, exact bottom-first `layers` (name/kind/depth/offset/
size/rgba/visible/opacity/blend/open), independent `composite_rgba`, the distinct
`stored_composite_rgba`, and optional `moved` expected output. The malformed RLE
fixture instead states `layered_available:false` and `layered_refusal_contains`,
along with its valid saved composite. RGB saved composites are `[13,27,41,255]`
per pixel; grayscale saved composites are `[13,13,13,255]`.

Local verification at the implementation source freeze: all41 focused tests
passed under normal-LTO Release (15 new pixel tests plus26 existing PSD/group
regressions), with zero failures. Formatting, whitespace checks and all-target
Clippy with warnings denied passed. Logs are
`/private/tmp/rz-phase10a-pixels-focused-release.log` and
`/private/tmp/rz-phase10a-pixels-clippy.log`. Export contains12 admitted fixtures
and one malformed-RLE refusal fixture, each with its independent JSON manifest.

No app was launched from the implementation worktree and no external image
data was introduced. The separate root qualification follows.

## Integrated qualification

- Full Rust regression: **1,445 passed, 0 failed, 2 existing ignored**, across
  148 result groups. Log: `/private/tmp/rz-phase10a-pixels-root-rust.log`.
  The full suite used the existing Release test cache with LTO disabled and
  local socket access; the 41 focused tests above used normal-LTO Release.
- Authoritative `make typecheck`, normal-LTO `make app`, all-target `make lint`
  with warnings denied, formatting and whitespace checks pass. Logs:
  `/private/tmp/rz-phase10a-pixels-root-typecheck.log`,
  `/private/tmp/rz-phase10a-pixels-root-app.log` and
  `/private/tmp/rz-phase10a-pixels-root-lint.log`.
- The immutable Release app passes all **13 fixtures** through the
  [pixel verifier](../scripts/verify_phase10a_pixels.py): 12 complete raster
  imports and one malformed-RLE layered refusal with explicit composite access.
  Every stored sample, signed offset and rectangle matches its authored
  manifest. Off-canvas moves, one-edit Undo/Redo and native reopen preserve
  exact bytes; canvas and saved-composite pixels match independent expectations.
  Receipt: `/private/tmp/rz-phase10a-pixels-root-mcp/result.json`.
- The same app passes all **seven existing folder fixtures**, including exact
  row topology/properties, independent composites, Undo/Redo, native reopen,
  unknown-blend refusal and stale-source refusal. Receipt:
  `/private/tmp/rz-phase10a-pixels-root-groups/result.json`.
- All 866 captured core/app/build/test/helper inputs remained unchanged through
  qualification, as did all 26 pixel fixture/manifest files. Input capture:
  `/private/tmp/rz-phase10a-pixels-root-inputs.json`. Executable SHA-256:
  `4bfed75987ad626ba67bba970e70c40bbe3e5e8e769d15fdd83b04b1fae2c4f8`;
  production archive SHA-256:
  `528e42baefa4b1f16012e42b2e02f7a1675999acdf772274932849c9c8cad459`.
  App: `/private/tmp/rz-phase10a-pixels-release-candidate/Rasterize.app`.

Independent review closed unsupported non-mask compression on discarded
folder/divider records and undisclosed divider masks. The external verifier
checks its fixture hashes before and after app work. Both owned app processes
exited; each used dummy credentials, a private local port and private
Actions/recovery/preset stores. Qualification documentation was updated after
the captured build without changing its compiled inputs.

No FFI signature, native format, dependency or MCP tool-count change. This
completes the admitted raster-rectangle slice; masks, other depths/compression,
independent Photoshop exchange, whole 10A and the 9I quality gate remain open.
