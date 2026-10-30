# Phase 10A — relative placement of ordinary raster masks

This is a historical slice record. Current 10A status, qualification and explicit
exclusions are maintained in the [bounded closeout](phase-10a-qualification.md).

This bounded follow-up admits relative placement for ordinary PSD masks on
raster records. Relative folder masks remain unqualified. It extends the
[ordinary raster-mask slice](phase-10a-masks-status.md), uses the existing
[outside-coverage model](phase-10a-mask-foundation-status.md), and does not complete
10A, high-depth PSD or Photoshop exchange qualification.

## Coordinate contract and evidence

The [Adobe PSD specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)
identifies the layer record's top/left/bottom/right rectangle and defines bit 0
of the mask flags as “position relative to layer.” It separately defines bit 1
as disabled, outside defaults 0/255 and the 20-byte body. These declarations are
the basis for interpreting an ordinary raster mask's relative rectangle in its
owning raster's coordinates. They do not establish a native content link.

For an owning raster at `(layer_left, layer_top)`, normalize exactly once:

```text
canvas_left   = layer_left + relative_left
canvas_right  = layer_left + relative_right
canvas_top    = layer_top  + relative_top
canvas_bottom = layer_top  + relative_bottom
```

The coordinate basis is the owning raster's original signed layer record.
Ancestor folder bounds, descendant unions, canvas origin, names and current
visible bounds do not supply this origin. Every addition uses checked widened
integer arithmetic. All four normalized edges must fit signed 32-bit; this bounded
reader refuses endpoint overflow even when a smaller origin alone would fit.
There is no wrapping, saturation or clipping to the canvas. Existing positive
extent, 30000-pixel side and 100 MP plane limits still apply.

Qualification uses independently authored equivalent absolute/relative PSD
pairs and independent raw/source-over expectations. Since 2026-09-16 it ALSO
uses a Photoshop-authored fixture and Photoshop's own renders in both
directions — see [Photoshop appearance agreement](#photoshop-appearance-agreement-run-3-2026-09-16),
which is where the reading of bit 0 is settled as a recorded decision.

## Implementation and persistent behavior

The existing `psd_group_records` walk supplies the original raster origin to
`MaskRecord::classify`. It supplies no raster origin for a folder or bounding
divider. The classifier accepts bit 0 only for an otherwise admitted ordinary
raster mask, normalizes its rectangle before channel validation, metadata-wrapper
allocation or pixel decoding, and records a specific retained observation.
All mask samples still use the existing shared Raw/PackBits framing and decoder.
No parallel record parser, decompressor, pixel resampler, FFI entry point or
native format is added.

The resulting mask remains independent (`linked=false`), with exact raw samples,
outside coverage, enabled state and canvas placement. Density 1 and feather 0
remain the only imported parameters. The report discloses the normalization and
that PSD linkage was not inferred or preserved. Moving the raster later leaves
the mask at its normalized canvas position; ordinary mask property edits remain
available through existing UI/MCP/Actions and central Undo.

Equivalent absolute and relative files therefore produce exactly the same native
bytes, including the existing conditional byte 28 outside-255 wrapper or byte 10
writer. Original source alpha and hidden RGB are untouched. Reopening or editing
the native document never applies the original relative offset a second time.

Relative folder masks remain a specific reported loss because folder bounds and
content unions have different meanings. Extended bodies, vector/-3 combinations,
rendered/inverted/unknown flags, parameters and unsupported mask codecs retain
the established unsupported-form distinction: disclose the dropped mask without
decoding its payload. Once an ordinary relative descriptor and codec are admitted,
invalid normalized coordinates, dimensions, rows or admission limits refuse the
layered choice. The saved composite remains a separate explicit choice.

## Qualification and fixture reproduction

`core/tests/psd_relative_masks.rs` adds five tests. The 13 independently authored
absolute/relative pairs cover Raw/RLE, both outside defaults, disabled masks,
grayscale, zero origin and zero ending edges, opposite signs, wholly off-canvas
storage, signed extremes, fractional source alpha/hidden RGB, and nested raster
children whose folder bounds deliberately suggest different origins. Both files
in every pair retain exact pixels/masks, independently expected projections,
identical native bytes and independent mask placement after raster moves.

Separate fixtures cover six signed-coordinate overflow/underflow directions,
empty/reversed/overwide/over-cap rectangles, malformed Raw/RLE samples, aggregate
mask and decode-envelope refusal before deliberately bad payloads, and relative
parameter/vector/-3/ZIP losses. The new corpus has 44 files:26 paired inputs,
14 layered refusals and4 explicit unsupported-form losses.

The former 53-case regression corpus changes one case: the now-admitted raster
`loss_relative` case is replaced by `loss_relative_group`, an explicitly relative
folder with malformed samples that must remain an undecoded loss. Its counts
remain 14 ordinary imports, 21 losses and 18 refusals. These are new corpus hashes;
historical 53-case receipts remain records of their earlier absolute-only scope.

```sh
RZ_PSD_RELATIVE_MASK_FIXTURES=/private/tmp/rz-relative-new-corpus \
RZ_PSD_MASK_FIXTURES=/private/tmp/rz-relative-prior-corpus \
CARGO_BUILD_JOBS=2 cargo test --release --manifest-path core/Cargo.toml \
  --test psd_relative_masks --test psd_raster_masks --test psd_layer_pixels \
  --test psd_group_import --test psd_import_notice --test psd_tests

python3 scripts/verify_phase10a_relative_masks.py \
  --app /path/to/Rasterize.app --fixtures /private/tmp/rz-relative-new-corpus \
  --output /private/tmp/rz-relative-app-verification
python3 scripts/verify_phase10a_masks.py \
  --app /path/to/Rasterize.app --fixtures /private/tmp/rz-relative-prior-corpus \
  --output /private/tmp/rz-relative-prior-verification
```

Use fresh private directories for every qualification run. Each corpus contains
binary `.psd` files and adjacent independently authored `.psd.json` expectations;
no compiler executable or product-native encoder output is a fixture input.
The new production helper pins every pair/case category before launching an
owned app, checks exact absolute/relative native and rendered equality, raster
move Undo/Redo, mask-property Actions and persistence, and retains the explicit
saved-composite choice and all refusal/loss checks. Before the corpus workflow,
all 14 failed layered opens must leave an edited sentinel’s native bytes, history,
dirty state, selection, selected layers and open-document IDs exactly unchanged,
checked immediately around each failure before any composite is opened. These
14 sentinel refusals are separate from the 14 refusal cases in the ordinary
corpus workflow: the helper performs **28 failed layered opens in total**.
The helper uses the existing
private-state transport and independent native/PNG decoders.

Final isolated and root integration receipts follow after their checks complete.
The carryover ledger and the separate 9I prerequisite decision remain unchanged.

## Photoshop appearance agreement (Run 3, 2026-09-16)

[Run 3 of the closeout](phase-10c-constructs-status.md#photoshop-appearance-agreement-run-3-2026-09-16)
took A5 with `scripts/verify_phase10_photoshop_appearance.py --receipt A5`
against **Adobe Photoshop 2025, 26.10.0** (executable SHA-256
`a502d3b62279299a77f85d69a3d8f3d5379e0fb406d40d746a00ee49031bfa04`), app
SHA-256 `a135ef31389df8c3d94ab078adcd296c674b60478611d3b1e17897e2b9906f8c`,
receipt `~/devel/rasterize-fixtures/photoshop/masks/receipt.json`. All five
structural gates passed and every case met its tolerance.

### Direction 1 — Photoshop's own masks, read by Rasterize

`core/tests/fixtures/psd_photoshop/masks/` (120 KB, six files) is
Photoshop-authored: a 64 x 64 base plus the SAME 40 x 40 overlay at (12, 8)
three times, masked from a selection — `hard` at rect (28, 24, 32, 36), `soft`
the same rect ramped black to white, `wide` at (0, 0, 40, 64). Photoshop
rendered Base plus each; Rasterize imported the file and rendered the same.

| case | tolerance | max / mean / differing | |
|---|---|---|---|
| `hard` | 1 code | 0 / 0 / 0 % | **pass** |
| `soft` | 2 codes | 0 / 0 / 0 % | **pass** |
| `wide` | 1 code | 0 / 0 / 0 % | **pass** |

Exact, all three, with the base render also exact at 0 codes. What the record
bytes say (gate M1, recorded rather than assumed):

| record | bounds | mask body | rect | flags | default |
|---|---|---|---|---|---|
| `hard` | `[8, 12, 48, 52]` | 20 bytes | `[24, 28, 60, 60]` | **0** | 0 |
| `soft` | `[8, 12, 48, 52]` | 20 bytes | `[24, 29, 60, 60]` | **0** | 0 |
| `wide` | `[8, 12, 48, 52]` | 20 bytes | `[0, 0, 64, 40]` | **0** | 0 |

Two facts in that table. Photoshop writes every one of these masks ABSOLUTE —
bit 0 clear — so it never authors the construct this slice admits; and it
TRIMS the stored rectangle to the mask's nonzero extent, which is why `soft`,
whose ramp starts at black, is one column narrower than `hard` from the same
selection.

### The relative twin, and what each side makes of bit 0

`source_relative.psd` is the runner's byte-patched twin of that same file:
each masked record's body patched in place (`flags |= 1`, rectangle made
relative to the record's own top/left, through `psd_records`' `mask_body_at`).
Its provenance is in the fixture's `description.json` note. Because Photoshop
never writes one, patching a file it DID write is the only way the construct
exists at all.

- **Rasterize** normalizes it once and renders the twin identically to the
  absolute file — **0 codes** on all three cases (gate M3), with the three
  *layer-relative placement normalized once against the owning raster top/left*
  observations and no loss. `core/tests/psd_photoshop_constructs.rs` pins the
  same equality through the C API, down to identical mask samples and an
  identical flattened projection.
- **Photoshop IGNORES bit 0.** Its renders of the twin equal, at 0 codes, a
  control file carrying the same moved rectangle with the flag left CLEAR —
  that is, it reads the relative rectangle as absolute — and differ from the
  original placement by 192 / 122 / 167 codes on 15.6 % / 26.6 % / 11.7 % of
  pixels. Its DOM bounds move with it (`hard` `[16, 16, 48, 48]`, `soft`
  `[17, 16, 48, 48]`, `wide` `[12, 8, 28, 48]`), and re-saving the twin keeps
  the flag set and the rectangle unchanged. The receipt records this as
  `photoshop_relative_reading: "absolute"`.

**Recorded decision:** Rasterize keeps Adobe's documented reading (bit 0 =
"position relative to layer", normalized once). Matching Photoshop would mean
ignoring a flag the published format defines, on files Photoshop itself never
produces, and would break every absolute/relative pair in this slice's corpus.
The divergence is carried here, in this section, as a recorded follow-up
("relative-mask semantics vs Photoshop") rather than as a defect: whether to
keep the documented reading, follow Photoshop or make it a preference is a
separate decision, and any change would break the absolute/relative pair
corpus in `core/tests/psd_relative_masks.rs`.

### Direction 2 — Rasterize's own unlinked masks, read by Photoshop

At both depths: a 40 x 40 shape at (12, 8), `select_rect(28, 24, 32, 36)`,
`add_layer_mask kind: from_selection`, `set_mask_properties linked: false`,
then `set_layer_properties offset_x: 18, offset_y: 12` — the layer moves, the
unlinked mask must not. `export_psd mode layers`, and Photoshop renders Base
plus each.

| case | depth | tolerance | max / mean / differing | |
|---|---|---|---|---|
| `Masked` | 8 | 1 code | 0 / 0 / 0 % | **pass** |
| `Soft` | 8 | 2 codes | 0 / 0 / 0 % | **pass** |
| `Masked` | 16 | 1 code | 0 / 0 / 0 % | **pass** |
| `Soft` | 16 | 2 codes | 0.01 / 0.00 / 26.8 % | **pass** |

Gate M5 reads the exported bytes rather than trusting the render: at BOTH
depths each record's `bounds[:2]` is `[12, 18]` (the layer moved) while its
mask rectangle is `[0, 0, 64, 64]`, equal to the `get_mask_properties`
geometry taken BEFORE the move (the mask did not), with bit 0 CLEAR; the
export reports no mask loss and `inspect_psd` shows both masks. Measured, and
worth recording because the plan expected otherwise: `add_layer_mask
from_selection` produces a CANVAS-sized mask at (0, 0) in byte mode as well as
in the typed one, so the two depths agree exactly and there is no per-depth
mask extent to reconcile.

Re-runnable (drop `--author` to render the committed `source.psd` and the
committed twin instead of re-authoring them):

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B scripts/verify_phase10_photoshop_appearance.py \
  --receipt A5 --app build/Build/Products/Release/Rasterize.app \
  --output ~/devel/rasterize-fixtures/photoshop/masks --author
```

## Isolated implementation receipt — 2026-09-13

- Worktree: `/private/tmp/rz-psd-relative-masks-worktree`, base `b6c1fd7`.
  The isolated agent made no commits or root source edits.
- All **58 normal-LTO Release tests pass**, with zero failures/ignored, across
  `psd_relative_masks` (5), `psd_raster_masks` (12) and the four prior PSD suites
  (41). Log: `/private/tmp/rz-psd-relative-mask-qualified.log`.
  All **448 captured core inputs** remained unchanged; their hashes are in
  `/private/tmp/rz-psd-relative-mask-qualified-inputs.json`.
- Formatting, all-target Clippy with warnings denied and whitespace checks pass.
  Clippy logs: `/private/tmp/rz-psd-relative-mask-lint.log` and
  `/private/tmp/rz-psd-relative-mask-final-lint.log`.
- New fixtures: `/private/tmp/rz-psd-relative-mask-fixtures`, **44 PSDs and44
  manifests**, with88 unchanged hashes in
  `/private/tmp/rz-psd-relative-mask-fixtures-hashes.json`.
  Regenerated prior fixtures: `/private/tmp/rz-psd-relative-updated-mask-fixtures`,
  **53 PSDs and53 manifests**, with106 unchanged hashes in
  `/private/tmp/rz-psd-relative-updated-mask-fixtures-hashes.json`.
  Independent review confirmed that104 prior files are byte-identical to the
  historical corpus; only the relative-mask loss fixture/manifest pair changed.
- Both production-helper modules pass syntax/import checks and their exact44/53
  corpus guards. Independent binary fixture review verified each pair differs
  only in the mask rectangle/relative flag and matches its expected samples.
  The new helper's14-refusal sentinel guard was independently reviewed after
  addition; no product or Rust test change was required.

Root integration owns the production app/MCP execution, full Rust suite,
current Swift/app checks, final immutable artifacts and commit receipts below.
The isolated receipt does not claim that the production helper has already run.

### Root integration and Release-app checks (2026-09-13)

The integrated normal-LTO Release app and authoritative Swift typecheck pass,
as do formatting and all-target Clippy. The final app is
`/private/tmp/rz-relative-root-release-candidate/Rasterize.app`.
Its executable SHA-256 is
`87a8e8dd5e02283b8e90b2b92ca6cf4f168cb5042aa044200ee660b71b203b1f`;
the linked normal-LTO archive SHA-256 is
`c1adf7f70e04fd824915a05cb0d8e3858a23cc15dc245d6b528c1a9f67efeea5`.
The root product, Rust-test and combined input manifests capture 683, 450 and
1,073 files respectively. Their content hashes remain unchanged through the
qualified build and application checks.

The initial root full-suite attempt exposed an integration cache issue: copying
files from the isolated worktree had retained modification times earlier than
the previous Cargo build. The new tests linked the previous importer and failed
all five new cases, reporting its old absolute-only mask policy. That failed log
is retained at `/private/tmp/rz-relative-root-rust.log`, together with its
partially generated corpora under the `-stale-cache-run` directory suffix.
The three integrated Rust source timestamps were refreshed without changing
content, then the core, app and full suite were rebuilt. Subsequent integration
copies use fresh timestamps; source hash agreement alone does not prove a cache
compiled the current contents. No product semantics, assertion or threshold was
changed to address the failed run.

The final production helper passes **44 corpus cases**: 13 independently authored
absolute/relative pairs, 14 refusals and four disclosed losses. Every pair has
identical native bytes and independently expected rendering, exact raw pixels,
alpha/hidden RGB, mask samples/properties and placement. Independent raster
moves, mask-property Actions, Undo/Redo, native reopen and the explicit saved
composite choice pass. Results are in `/private/tmp/rz-relative-root-mcp/`.

A separate initial pass over all 14 refused imports checks an existing dirty
document with a meaningful Undo history and selection. Each failed layered open
preserves its exact native bytes, history tuple, dirty flag, selection, active
and selected layers, and the complete set of open-document IDs. These checks
precede any explicit composite opens. The helper therefore performs 28 failed
layered opens in total; the second set belongs to the original 44-case flow.

The production inputs are private copies of the independently authored,
qualified 44/53 corpora at `/private/tmp/rz-relative-root-mcp-fixtures` and
`/private/tmp/rz-relative-root-mcp-prior-fixtures`. All 194 fixture/manifest files
are captured in `/private/tmp/rz-relative-root-mcp-fixture-hashes.json`.
The full root suite independently regenerates those corpora for a byte-for-byte
comparison. This preserves the separate authored-fixture and product-decoder
roles; no product-native encoder supplies the PSD fixture bytes.

The final combined source probe links the same production archive and passes
`native layer blending, mask outside coverage`. All **1,075** physically copied
source/test/example/helper/build/archive/executable inputs remain unchanged.
Actual controls, Actions, exact-owner Undo/Redo, group transitions, snapshots
and private recovery pass. Independent decoding verifies five standalone31
saves, five outer29→31 saves, byte28/native29 mask saves, and 11 snapshot/recovery
`image.rz` payloads with exact raw sources, masks, all mask properties, ICC and
matching package bytes. This probe qualifies the shared mask/blend UI and
archive paths; the 44-case production helper separately qualifies PSD intake.

The receipt is `/private/tmp/rz-relative-root-source-probe/result.json`;
its SHA-256 is
`1864e861de823136917173f5379fce9550d7537afab9d4036ff8073b261b837a`.
The source-probe executable SHA-256 is
`7eacb8cc26283e458ca9531bb7ea9d06b2a3fe9f91996fb1420cd5b84d307095`.
The owned probe exited with dummy credentials and private Actions, recovery,
RAW-preset and filter-preset stores; no Rust rebuild occurred during the probe.

### Final integrated qualification

The complete fresh root Rust run passes **1,490 tests, zero failures and two
pre-existing ignored tests across 154 result groups**. The log is
`/private/tmp/rz-relative-root-rust-qualified.log`. It uses the isolated no-LTO
Release test cache with serialized execution; the shipping app and source probe
use the separately qualified normal-LTO archive. The regenerated 44-case and
53-case corpora match all **194** authored fixture/expectation files byte for
byte, with no extra files. Their hashes are recorded in
`/private/tmp/rz-relative-root-regenerated-fixture-hashes.json`.

The same final Release app passes the revised 53-case mask corpus, the prior
13 raster-rectangle cases, the five-mode workflows including outside255 masks,
and the mask foundation's ten cases plus eight nested-wrapper checks. Receipts
are `/private/tmp/rz-relative-root-{masks,pixels,five,foundation}/result.json`.
All 683 product, 450 Rust and 1,073 combined source/helper/build inputs remain
unchanged after the full run. Formatting, Clippy, typecheck, actual UI/MCP,
Actions, exact Undo/Redo, native reopen, snapshots and recovery checks pass.

Independent implementation, fixture, helper and integration review found no
remaining blocker. All owned app and probe processes exited. Relative folder
masks, extended/vector/parameter forms, broader depth/compression and independent
application exchange remain open. Full 10A/10D and the separate 9I prerequisite
gate are unchanged; the carryover ledger remains 18 verified and 18 open.
