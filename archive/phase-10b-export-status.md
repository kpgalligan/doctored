# Phase 10B — basic layered PSD export status

**10B is qualified and closed.** Phase 1 shipped the writer, its preflight, the
UI path and the MCP twins; phase 2 reopened seven Rasterize-written PSDs in an
independent application — GIMP 3.2.6 — under gates fixed before the run, and
every one of them passed. The receipts are at the end of this document.

The contract below is what the export CLAIMS. Everything outside it is either
refused with a reason before a byte is written, or listed in the report the
user and the model both see. "Never silent" is the whole design rule.

## Accepted export contract

| Carried | How it is written | Round-trips through Rasterize's own importer |
| --- | --- | --- |
| Canvas size, 8-bit RGB | Header, mode 3, four channels with merged alpha declared | Yes |
| Raster layers | One record per layer, four PackBits planes (`-1` alpha, `0`/`1`/`2` RGB), straight samples, exclusive signed bounds | Yes, byte-exact, including hidden RGB under alpha 0 and off-canvas rectangles |
| Layer/group masks | Ordinary absolute 20-byte mask body plus a fifth `-2` PackBits plane; outside default 0/255 and the disabled flag | Yes: samples, size, offset, outside default and enabled state |
| Groups | `lsct` 1 (open) / 2 (closed) folder records, each with its `lsct` 3 closing divider, four empty channel slots | Yes: nesting, order, open/closed, empty groups |
| Opacity | One byte per record, `round(opacity * 255)` | Yes, to the stored byte |
| Blend modes | `psd_group_records::PSD_BLEND_KEYS`, read backwards; Pass Through on folders | Yes for every key the importer recognizes |
| Visibility | Record flag bit 1 | Yes |
| Layer clipping | Clipping byte on raster records | Yes |
| Names | Truncated Pascal name plus the full name in `8BIM luni` (UTF-16BE) | Yes, including names past 255 UTF-8 bytes |
| ICC profile | Image resource 1039 | Yes, byte-identical |
| Print resolution | Image resource 1005 (16.16 PPI per axis) | Yes |
| Composite preview | Image data section, PackBits, RGB matted against white | Alpha exactly; colour exactly at alpha 0 and 255, within `ceil(127.5/a)` codes elsewhere |

## Explicit exclusions

Refused (the mode is unavailable and says why):

- A canvas, raster or mask side past 30000 pixels; a worst-case file past 2 GiB.
- Adjustment layers — merge them down or export the flattened composite.
- Layer styles.
- Masks carrying density or feather.
- 16-bit (typed) documents: the menu item is disabled and both MCP tools refuse.

Written but reported as a loss or an observation:

- **Group clipping.** The flag is written for Photoshop, but Rasterize's own
  layered import drops it, so a clipped group does not round-trip through this
  application. Reported per clipped group by the export, and again by the
  importer's own `verified_losses`.
- **Rasterize's 2 GiB layered decode envelope.** The export evaluates the same
  `psd_layer_pixels::peak_forecast` that import admission uses, on the file it
  is about to write. Past it, the report says the file will reopen HERE as its
  flattened composite only; Photoshop is not affected. This is a report entry,
  never a refusal — a document Photoshop can open must still export.
- Blend modes beyond Normal, Multiply, Screen and Pass Through: written with
  Photoshop keys, appearance in other applications not qualified (the 10D
  boundary).
- Re-editable descriptions (text, shape, live photo), embedded sources and
  live filters: exported as pixels only.
- Layer locks, link groups, saved channels, guides, the ruler origin,
  EXIF/XMP/IPTC packets, the global light direction, PSD mask link state.

Out of scope for this phase entirely: PSB, ZIP, grayscale/CMYK output, 16-bit
PSD, editable styles/adjustments/text/smart objects (10C), thumbnails, and
Rasterize-private payloads. PSD is not a `save_copy`, Batch or Action output
format, and `export_psd` is `nonRecordable` and not a step tool.

## Verification receipts

Phase 1, all passing at the commit that introduced this document:

- `core/tests/psd_export.rs` — seven cases through the C API:
  - PackBits at widths 1/128/129/257 over pseudo-random, all-equal,
    alternating and long-run rows: every row decodes exactly through a
    decoder written independently in the test, never emits the `-128` control,
    and stays inside `w + ceil(w/128)` bytes.
  - A full round trip whose SOURCE is a hand-built PSD imported through the
    existing reader: nested open/closed groups (Pass Through and Multiply), an
    empty group, a clipped group, a clipped layer, a hidden layer, a negative
    offset, hidden RGB under partial alpha, a 360-byte Unicode name, absolute
    raster masks with both outside defaults and a disabled one, a group mask,
    Display P3 and 300 × 240 PPI. The written bytes are read back by an
    independent reader in the test, parsed again by the third-party `psd`
    crate, and finally reopened through `rz_psd_import_report` +
    `rz_doc_open_psd`, comparing kind, depth, name, opacity byte, blend,
    visibility, clipped, open, offset, pixels, mask samples/size/offset/outside,
    profile bytes and resolution exactly. `verified_losses` is exactly one
    sentence — the clipped group — and nothing else.
  - Flattened mode: one raster named "Composite"; a mode-0 open of that file
    equals `flattened()` byte-exactly (the layer plane is raw, not matted), and
    the mode-1 stored composite is exact at alpha 0/255 and within
    `ceil(127.5/a)` per channel elsewhere, over a fixture containing alphas 0,
    1, 64, 127, 128, 200 and 255.
  - Each refusal (adjustment, style, mask density/feather, a 30001-pixel side,
    and a reduced `max_file_bytes`) is reported AND leaves a pre-existing
    destination byte-identical.
  - A reduced `import_envelope_bytes` produces the envelope loss while leaving
    `available` true, and the save still succeeds.
  - A NULL sweep over both exports: NULL document, NULL path, bad mode, NULL
    `err_out`.
- `make test`, `make lint`, `make typecheck`, `make app` — clean.
- Driven over MCP against the built app: `open_document` of a layered `.rz`,
  `inspect_psd_export`, `export_psd`, then `inspect_psd` and `open_document`
  with `psd_mode: layers` on the written file, comparing `get_document`
  structure; on a 16-bit document both MCP export tools refuse with the
  precision refusal and `PSDExportCapture` throws its 8-bit-only message.
  The greyed-out File > Export as PSD… item on a 16-bit document is
  covered by the same `doc?.precision == nil` validation case but was not
  enumerated (no accessibility permission for the menu bar) — pending human
  confirmation.

Set `RZ_PSD_EXPORT_FIXTURES=<dir>` to have the round-trip test also write its
PSDs for external review:

```sh
python3 -c "import pathlib,sys; sys.path.insert(0,'scripts'); import psd_qualification_decode as p; print(p.decode(pathlib.Path(sys.argv[1]))['layers'])" <dir>/nested.psd
```

## Phase 2 — independent-application reopen receipts (closed)

The corpus is seven documents written by the production writer through the C
API (`core/tests/psd_export_corpus.rs`, `rz_doc_save_psd` and
`rz_precision_doc_save_psd`) and committed with a manifest built from the SOURCE
document's own fields and its own projection — never from the importer:
`core/tests/fixtures/psd_10b/` (49 files, 404,078 bytes). Each case carries
`source.psd`, `manifest.json` and five files copied unmodified from GIMP:
`gimp.json`, `reopened.png`, `reopened.icc`, `merged.png`, `merged.icc`.

| Case | What it qualifies |
| --- | --- |
| `layers_basic` | Signed and wholly off-canvas rectangles, a hidden layer, a fractional opacity byte, a 200-character Unicode name, alphas 0/1/64/127/128/200/255 and hidden RGB under alpha 0. |
| `groups_masks` | Nested open/closed folders, Normal/Multiply/Screen/Pass Through, folder opacity, folder and raster masks with both outside defaults, a disabled mask, an empty folder, 300 × 240 PPI. |
| `flattened` | The same document written in the flattened mode: one raster named "Composite". |
| `p3_ppi` | Exact Display P3 profile bytes and print resolution. |
| `clipping` | Two clipping runs: a raster base with two clipped rasters, and a base with a clipped FOLDER above it. |
| `styled_effects` | A styled raster written as Photoshop's own constructs (since [10C-2 slice 3](phase-10c-constructs-status.md)): the layer's own rectangle and `-2` mask, an `lfx2` effects descriptor, an `iOpa` fill-opacity block and the 40-byte blending ranges, with the global light in image resources 1037/1049. Renamed from `styled_bake`, which held the rasterized form. |
| `depth16` | A genuine 16-bit RGB file: big-endian u16 planes, a u16 mask with outside 65535, a disabled u16 mask, a pass-through folder, Display P3, 300 × 240 PPI. |

The five gates were fixed before any GIMP process ran and were never widened.
`scripts/verify_phase10b_external.py` runs them; the reference direction is the
reverse of 10A's — GIMP's loads are the reference and the Rasterize output is
the measured side.

1. **The bytes say what the document held.** The manifest against an
   independent decode of `source.psd` (`scripts/psd_qualification_decode.py`,
   extended to depth 16 here; its depth-8 output is unchanged and
   `verify_phase10a_external.py --oracle-only` still passes): name, kind, depth,
   visibility, opacity byte, blend key, clipping, folder open state, profile
   SHA-256 and PPI exact for every record, plus exact offset, size, straight
   RGBA and mask samples/size/offset/outside/enabled. The one baked record
   instead has NO mask channel, and its written rectangle `[2,2] 30 × 30`
   contains the source rectangle `[10,10] 12 × 12` and lies inside the canvas.
   The written preview's alpha is exact everywhere and its RGB error is 0 codes
   in all seven cases.
2. **What GIMP read back.** `file-psd-load`'s complete layer tree against that
   same decode, 1:1 in order after removing the folders GIMP synthesizes per
   clipping run: name (from `luni`), kind, depth, visibility,
   `round(opacity*255)`, blend nick, `clip-to-backdrop` exactly where the record
   is clipped, exact raster offset/size/samples, folder open state, mask
   coverage/enabled against the layer-sized plane GIMP builds from the stored
   rectangle and the outside default, profile bytes identical to resource 1039
   with no timestamp exception, exact resolution, and precision
   `u8-non-linear` / `u16-non-linear`. `clipping` produced two synthesized
   folders (`Group added by GIMP` and `Group added by GIMP #1`), matching its
   two clipping runs; every other case produced none.
3. **Appearance.** GIMP's reopened projection against the manifest's render:
   max abs error 0 codes per RGBA component in `layers_basic`, `groups_masks`,
   `flattened`, `p3_ppi` and `depth16` (threshold 2, and 514 at 16 bits).
   `clipping` also measured 0. The renamed `styled_effects` case measured
   151/128/130/0 on 2026-09-16: gate 2 applies to it in FULL (the record keeps
   its own rectangle and an ordinary `-2` mask — there is no bake carve-out any
   more), while gate 3 stays RECORDED, not gated, because GIMP 3.2.6 reads
   NONE of the `lfx2` — every snapshot row's `filters` list came back empty and
   its log carried no effects message — so its projection is the bare layer
   stack, not the styled render. Clipping semantics and the effect appearance
   the format hands to the reading application are outside this phase's
   appearance claim.
4. **The saved preview.** GIMP's separate `file-psd-load-merged` samples against
   the decoded composite: 0 codes in all seven cases. The black/white backdrop
   comparison against the layered render is 0 for the five qualified cases.
5. **The application.** Against the Release app
   (SHA-256 `e2905ff1b1b5f7ad0e3ae0c9233718be7b1368e03ae4372b741109f1d9fdde2a`):
   per case `inspect_psd` (`import_precision` `rgba16` for `depth16`),
   `open_document` with `psd_mode: layers`, `inspect_psd_export` (available,
   correct depth) and `export_psd`. Six of the seven re-exports are
   BYTE-IDENTICAL to the committed source; all seven decode identically.
   The style case is the one exception and the only declared one: its committed
   preview is the live styled projection while the re-export's is re-derived
   from the carried style, so `composite` and `composite_encoded` are excluded
   from that equality and their max abs error is recorded as the observation
   "layer style written as lfx2; reopened composite re-derived from the carried
   style" (1 code on 2026-09-16). Its re-export report has no rasterize loss
   and its `open_document` report has no style loss. The `save_copy` PNG
   against `reopened.png` (`projection_vs_reopened`) is 0 codes and GATED for
   four of the six 8-bit cases; it is RECORDED, never gated, for exactly
   `clipping` (0 codes) and `styled_effects` (151/128/130/0), because
   `save_copy` renders the whole style while GIMP's `reopened.png` renders none
   of it. Gate 4 stays gated for the style case (merged = stored composite,
   0 codes) with the backdrop comparison skipped, since it is not
   appearance-qualified.

**Closed 2026-09-16.** The `styled_effects` fixture's five gates were re-run
against the Release app SHA-256 `084b4be2b97b1c84b9749e861c066bec9b72abbefb627c37f7e7c9d7aa8052a6` with the restated expectation above,
and its regenerated `gimp.json`, `merged.*` and `reopened.*` are committed. The
other six cases were reopened in the same run: their fresh outputs were
byte-identical to the committed ones apart from the additive `filters` key the
snapshot now records, so they were not recommitted and their receipts stand.
Taken as receipt 4 of [Run 1 of the closeout](phase-10c-constructs-status.md#manual-receipts-taken-2026-09-16).
Photoshop's own reading of those effects — open when this phase closed, because
no local instance had been scripted for it — was taken on 2026-09-16 as Run 3's
A3: Photoshop 26.10.0 reopens a Rasterize export of sixteen styled cases at
both depths, parses every `lfx2` into `layerEffects` and the `iOpa` into
`fillOpacity` 128, and renders nine of them inside a tolerance fixed before the
run ([10C-2 Photoshop appearance agreement](phase-10c-constructs-status.md#photoshop-appearance-agreement-run-3-2026-09-16)).

Receipts, both outside the repository:

- GIMP run: `/private/tmp/rz-10b-gimp/state/receipt.json` — 7 cases, 35
  artifacts, exit code 0, no case error, pinned executable SHA-256
  `a771abf6241f43796dd843d332d968fe943da9e981c61adde7a0db1c21b79764`,
  private `GIMP3_DIRECTORY`/`GIMP3_CACHEDIR`/`GIMP3_TEMPDIR`.
- Application run: `/private/tmp/rz-10b-external-1789406628/result.json` — 7
  cases, every gate `true`, `fixtures_unchanged: true`, 49 fixture hashes and
  every helper hash re-verified after the run.
- The 2026-09-16 re-run for the renamed style case, preserved outside
  `/private/tmp`: GIMP run
  `~/devel/rasterize-fixtures/phase10-receipts/R4/gimp-state/receipt.json` — 7
  cases, 35 artifacts, exit code 0, no case error, the same pinned executable —
  and application run
  `~/devel/rasterize-fixtures/phase10-receipts/R4/external/result.json` — 7
  cases, every gated check `true`, `fixtures_unchanged: true`, app SHA-256
  `084b4be2b97b1c84b9749e861c066bec9b72abbefb627c37f7e7c9d7aa8052a6`.

Provenance, the case table and the GIMP loader conventions this run recorded —
its synthesized clipping folders, its content-derived folder bounds, its
resizing of a mask to the layer rectangle, and `union`/`auto` reported
interchangeably for an unclipped layer — are in
[`scripts/psd-exchange-probe/README.md`](../scripts/psd-exchange-probe/README.md).
No writer defect was found, so the 8-bit goldens in
`core/tests/fixtures/psd_10c/` were not regenerated.

Still outside the claim, and not reasons to reopen 10B: PSB, ZIP output,
grayscale/CMYK, 32-bit, appearance of clipping semantics and of blend keys
beyond Normal/Multiply/Screen/Pass Through (the 10D boundary), and masks
wholly outside their layer rectangle (GIMP drops them). Two items on this list
have since been answered elsewhere and are recorded here so the list stays
honest:

- **Photoshop itself** is scripted now (`scripts/photoshop-probe/`, pinned
  26.10.0). Runs 2-4 of the closeout
  ([the constructs status](phase-10c-constructs-status.md)) reopen
  Rasterize-written PSDs in it and author reference files with it.
- **GIMP-authored adjustment/fill records** still do not exist (its exporter
  writes none), so this document's own reopen gates keep their synthetic
  receipts for the omission rule — but the rule itself is no longer
  synthetic-only: Photoshop authored `brit`, `SoCo` and empty records in
  `core/tests/fixtures/psd_photoshop/depth16/`, and what Rasterize reports for
  each is measured in
  [the construct status](phase-10c-constructs-status.md#photoshop-16-bit-import-run-4-2026-09-16).
  That file is also the first third-party-authored 16-bit LAYERED import
  ([the depth status](phase-10c-depth-status.md)), so the IMPORT side of 16-bit
  is no longer a self-consistency claim; the 16-bit EXPORT side's receipts
  (GIMP 3.2.6 reading `psd_10b/depth16/`) are unchanged.
