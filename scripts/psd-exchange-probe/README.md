# Independent application corpus for bounded 10A import

The 60 files in `core/tests/fixtures/psd_10a/` were generated locally by
**GIMP 3.2.6**, using its public Python API, its PSD exporter and its layered PSD
loader. The source artwork consists of small color/alpha arrays, masks and
folders authored in `author_gimp.py`. No Doctored encoder, compositor or PSD
fixture writer generated these PSDs or reference PNGs. No personal document was
opened, and exported PSD bytes were never patched to fit the importer.

This is independent **GIMP** interoperability evidence for the declared corpus.
It is not a Photoshop-authored corpus, universal Photoshop appearance claim,
layered PSD export qualification, or high-depth PSD qualification. The finite
scope and fixed appearance gate are in
[`designs/phase-10a-qualification.md`](../../designs/phase-10a-qualification.md).

## Provenance

- Distribution: the official [GIMP macOS release directory](https://download.gimp.org/gimp/v3.2/macos/),
  `gimp-3.2.6-arm64.dmg`. Its published SHA-256 was checked before mounting:
  `854573aec2be6a185aa021109c3c0ca94f210379204b704231bdd191b4060294`.
- Executable SHA-256:
  `a771abf6241f43796dd843d332d968fe943da9e981c61adde7a0db1c21b79764`.
  The original DMG stays outside the repository. It was mounted read-only;
  nothing was installed into `/Applications`.
- The authoring script SHA-256 is
  `88977926e9cdec4cb0a5d7e86e99736a4a83f782f2115383b0e9fa9fdba1d533`.
  Every case manifest records GIMP's reported version and all six artifact
  roles: source PSD, authored and reopened PNGs, authored and reopened profiles,
  and the manifest itself. Its `files` table hashes the five companion files.
- The P3 case uses the macOS `Display P3.icc` file, SHA-256
  `0ff6958f98684c61f6bbdce1368ddeaf3873baf84545baba482e920d92a914c0`.
  Exact copies of the profile embedded by the application accompany the case.
  Other RGB cases use GIMP's builtin sRGB profile.
- Initial qualification ran on 2026-09-13. Private application state was selected
  through `GIMP3_DIRECTORY`, `GIMP3_CACHEDIR` and `GIMP3_TEMPDIR`; every process
  used `--new-instance --no-interface --no-data --no-fonts`. Only owned processes
  and new documents were used. The already-running Photoshop process was not
  scripted or inspected for document contents.

The [GIMP command-line reference](https://www.gimp.org/man/gimp.html) documents
the separate-instance, batch and private-state mechanisms. The batch uses
`python-fu-eval` and the application's ordinary procedure registry. All source
arrays and authoring code are included here; no externally sourced artwork or
private download is needed to reproduce them.

## Cases and independent expectations

| Case | What the original exported PSD qualifies |
| --- | --- |
| `rgb_mask` | Partial-alpha raster plus enabled ordinary mask over an opaque backdrop. |
| `rgb_mask_disabled` | The same mask retained with its disabled state. |
| `rgb_partial_alpha` | Exact straight sources including hidden RGB and alpha0/1/127/255. |
| `rgb_offcanvas` | Negative full raster/mask bounds and a hidden raster wholly outside the canvas. |
| `rgb_group_isolated` | Normal isolated folder, byte-exact fractional opacity, closed state, Multiply/Screen children and an ordinary folder mask. |
| `rgb_group_pass` | The corresponding Pass Through folder with its own mask and opacity. |
| `rgb_nested_hidden` | Nested folder structure, open/closed state, fractional opacity and a hidden folder. |
| `rgb_p3_ppi` | Exact Display P3 source encoding/profile and 300×240 PPI metadata. |
| `rgb_saved_channel` | Supported raster plus a real saved auxiliary channel; the channel must be specifically reported as discarded. |
| `gray_mask` | Real grayscale mask document with a gray ICC; both Doctored choices must explicitly refuse the unsupported embedded profile. |

All layer and mask channels written by this GIMP exporter are PackBits/RLE.
Its admitted masks have 20-byte bodies, absolute coordinates, outside0 and
ordinary `-2` channels. Existing independent Rust/production fixtures retain the
Raw codec, outside255, relative coordinates, signed endpoints and malformed-input
coverage. These properties are not falsely attributed to the GIMP files.

`author_gimp.py` captures the actual application state before export, deletes
that document, then independently loads the original exported PSD with
`file-psd-load`. It captures the complete reopened layer tree and raw planes and
renders again through `Gimp.Layer.new_from_visible`. The reopened reference is
therefore a layer projection, not a copy of the PSD's saved merged preview.
The production verifier compares that saved composite separately.

Manifest layers use bottom-first postorder. Group bounds are GIMP's actual group
bounds; they are not substituted for Doctored's native placeholder geometry.
Raw reads request the drawable's own buffer format, preserving its actual
profile encoding. Requesting a generic sRGB buffer format would incorrectly
convert P3 source codes. Scalar masks are authored as linear `Y u8` coverage.
The independent PSD decoder checks those application observations against the
original file's actual geometry, samples, flags, profiles and resource records.

GIMP's PSD writer always embeds an effective ICC for these images, even when its
generic `include-color-profile` option is false. This is explicit in the official
[`psd-export.c` at `GIMP_3_2_6`](https://github.com/GNOME/gimp/blob/GIMP_3_2_6/plug-ins/file-psd/psd-export.c#L1085).
Consequently the gray case is an existing unsupported-profile refusal, not an
admitted grayscale rendering pass. Its unmodified file and independent metadata
remain in the corpus. Admitted untagged grayscale remains covered by the existing
exact codec/import fixtures; this corpus adds no broader gray-ICC support claim.

## Reproduction

Download and verify the official DMG using the checksum above. Mount it read-only
at a private path. With fresh output and state paths, run:

```sh
python3 -B scripts/psd-exchange-probe/run_author.py \
  --gimp /private/tmp/gimp-mount/GIMP.app/Contents/MacOS/gimp \
  --output /private/tmp/rz-10a-independent-reproduction \
  --state /private/tmp/rz-10a-independent-state

python3 -B scripts/psd-exchange-probe/compare_authoring.py \
  core/tests/fixtures/psd_10a /private/tmp/rz-10a-independent-reproduction \
  --receipt /private/tmp/rz-10a-authoring-comparison.json
```

The launcher pins the application executable and source P3 profile, requires
fresh private state, captures input hashes and process ownership, verifies each
manifest, and records its receipt beside the GIMP log. Its version assertion
prevents an accidental change of authoring application.

GIMP creates timestamps in the headers of its builtin sRGB/gray ICC profiles.
Fresh authoring processes therefore produce different embedded profile and file
hashes. `compare_authoring.py` requires exactly matching application observations,
decoded PSD state and PNG samples; the only permitted ICC difference is its
creation-time field at bytes24–35. It does not rewrite any artifact. **This
authoring-only allowance does not apply to Doctored:** imported/native profile
bytes must exactly equal the particular original PSD's profile.

The frozen corpus is 111,832 bytes across 60 files. A fresh private-state
regeneration reproduced all raw sources, masks, structure, metadata and rendered
samples exactly; 52 artifact hashes differed only through those builtin ICC
creation timestamps and their dependent encodings/hashes. The P3 case was
byte-identical. Final Doctored
application measurements and closeout receipts belong to the qualification
record linked above.

## Separate saved-composite references

`core/tests/fixtures/psd_10a_merged/` adds three files per original case without
changing any of the original 60 artifacts: `merged.png`, `merged.icc` and a
separate `manifest.json`. These were captured by `capture_merged_gimp.py` using
GIMP's actual **`file-psd-load-merged`** procedure. Its one decoded raster, native
pixel codes, source PSD hash, profile and resolution are recorded before an
independent application projection supplies the PNG. No inverse-color formula
or Doctored output creates this evidence.

The original application run found a real distinction between PSD encoded
merged planes and straight layer samples. GIMP's exporter first blends merged
RGB against white while retaining its alpha channel. Its merged reader removes
that white contribution for positive alpha; see
[`psd-export.c:2240`](https://github.com/GNOME/gimp/blob/GIMP_3_2_6/plug-ins/file-psd/psd-export.c#L2240)
and [`psd-load.c:3377`](https://github.com/GNOME/gimp/blob/GIMP_3_2_6/plug-ins/file-psd/psd-load.c#L3377).
Directly using the encoded merged RGB as straight RGB produces a white halo.
The original failing comparison remains recorded; these new references qualify
the corrected interpretation, rather than changing the source PSD to conceal it.

Quantization makes tiny-alpha original color unrecoverable. In the unchanged
partial-alpha fixture, source RGB `[63,137,201]` at alpha1 is stored in the merged
planes as `[254,255,255]`; GIMP's merged reader reconstructs `[0,255,255]` at
alpha1. At alpha127, source `[101,211,67]` becomes stored `[178,233,161]` and GIMP
reconstructs `[100,211,66]`. The original editable layer samples remain exact.
Therefore a merged reconstruction must be compared with the independent merged
reference, and its appearance on opaque black/white backgrounds with the
original layered reference. The fixed two-code appearance limit is unchanged;
it cannot require reconstructing information the saved merged planes discarded.

The new manifests name both the original ICC SHA-256 and captured merged ICC
SHA-256, plus `icc_creation_time_only_difference`. GIMP reloads builtin sRGB/gray
profiles with a fresh creation time. All other profile bytes must match the
original, and the merged PNG profile must exactly match `merged.icc`. The P3
profile must match the original in full. This independent-application timestamp
allowance never applies to Doctored's exact original-profile persistence gate.

To reproduce the capture from an existing, unchanged original corpus:

```sh
python3 -B scripts/psd-exchange-probe/run_merged_capture.py \
  --gimp /private/tmp/gimp-mount/GIMP.app/Contents/MacOS/gimp \
  --source core/tests/fixtures/psd_10a \
  --output /private/tmp/rz-10a-merged-references \
  --state /private/tmp/rz-10a-merged-state
```

The runner requires fresh output/state, pins the executable, captures every
source artifact and helper hash, records the owned process and verifies that all
original inputs remain unchanged. It does not re-export or rewrite any PSD.

## Reopening Doctored exports

The 10B phase-2 corpus reverses the direction of everything above. The seven
cases in `core/tests/fixtures/psd_10b/` are written by **Doctored's own PSD
exporter** (`core/tests/psd_export_corpus.rs`, through `dr_doc_save_psd` and
`dr_precision_doc_save_psd`), and **GIMP 3.2.6** is the independent reader whose
observations are the reference. `reopen_gimp.py` loads each `source.psd` twice —
`file-psd-load` and `file-psd-load-merged` — snapshots the layer tree, renders
both projections and writes `gimp.json`; the PSD bytes are hashed before and
after and never modified. No Doctored decoder, importer or expectation appears
in the GIMP-side script, and `scripts/verify_phase10b_external.py` derives every
expectation from the committed manifest (built from the SOURCE document's own
fields) and from `scripts/psd_qualification_decode.py`.

Provenance is the same pinned application as the 10A corpus: the official
`gimp-3.2.6-arm64.dmg` (published SHA-256 above), mounted read-only, executable
SHA-256 `a771abf6241f43796dd843d332d968fe943da9e981c61adde7a0db1c21b79764`,
private `GIMP3_DIRECTORY`/`GIMP3_CACHEDIR`/`GIMP3_TEMPDIR`, and
`--new-instance --no-interface --no-data --no-fonts`. Each case folder holds
`source.psd` and `manifest.json` (Doctored-written) plus `gimp.json`,
`reopened.png`, `reopened.icc`, `merged.png` and `merged.icc` copied unmodified
from the runner's output.

| Case | What the Doctored-written PSD qualifies |
| --- | --- |
| `layers_basic` | Signed and wholly off-canvas rectangles, a hidden layer, a fractional opacity byte, a 200-character Unicode name, alphas 0/1/64/127/128/200/255 and hidden RGB under alpha 0. |
| `groups_masks` | Nested open/closed folders, Normal / Multiply / Screen / Pass Through, folder opacity, folder and raster masks with both outside defaults, a disabled mask, an empty folder and 300 × 240 PPI. |
| `flattened` | The same document in the flattened mode: one raster named "Composite". |
| `p3_ppi` | Exact Display P3 profile bytes and print resolution. |
| `clipping` | Two clipping runs — a raster base with two clipped rasters, and a base with a clipped FOLDER above it. |
| `styled_effects` | A styled raster's PRESERVED record: its own rectangle and ordinary `-2` mask, the effects written as `lfx2`, fill opacity as `iOpa` and Blend If as the 40-byte blending ranges. |
| `depth16` | A genuine 16-bit RGB file: big-endian u16 planes, a u16 mask with outside 65535, a disabled u16 mask, a pass-through folder, Display P3 and 300 × 240 PPI. |

GIMP conventions this run recorded, none of which the writer should chase:

- Its loader **synthesizes one folder per clipping run**, named
  `Group added by GIMP`, and uniquifies a duplicate on insert
  (`Group added by GIMP #1` for the second run). The verifier removes those
  folders and decrements their descendants' depth, then asserts that the number
  removed equals the number of clipping runs in the file.
- A folder's reported size and offset are GIMP's own **content bounds**, not
  anything stored in the file (an empty folder reports 1 × 1). They are recorded
  and never gated.
- A mask is **resized to its layer's rectangle** on load: the stored rectangle
  is pasted over its intersection with the layer and the rest is filled with the
  mask's outside default. A mask wholly outside its layer is dropped, so this
  corpus declares none.
- An unclipped layer's composite mode is reported as `union` or `auto`
  interchangeably; a clipped one is always `clip-to-backdrop`.
- Its PSD writer emits no adjustment or fill records, so the omission rule in
  `archive/phase-10c-constructs-status.md` keeps its synthetic receipts.

Conventions the Phase 10 receipt runs (R4-R9) added, none of which the writer
should chase either:

- It attaches **no filter at all** from a record's `lfx2`: every snapshot row's
  `filters` list (PDB `gimp-drawable-get-filters`, recorded since R4) was empty
  on the written-style case, and its log carried no effects message. Its
  reopened projection is therefore the BARE layer stack, not the styled
  appearance, which is why that case's appearance and `projection_vs_reopened`
  are RECORDED and never gated.
- It **ignores the record's blending ranges**: given a layer Doctored's own
  preview excludes entirely, GIMP renders the unrestricted source-over exactly
  (0 codes in R7).
- It **keeps a pixel-less adjustment record** rather than dropping it: the
  `nvrt` record of R8 arrived as an empty canvas-sized raster at (0, 0) with
  its name, opacity and `multiply` nick intact and no adjustment applied, so
  its projection differs from the stored composite (56779 codes, recorded).
  Its geometry is recorded, never gated.
- Its layer-mode nicks for the four threshold keys are `hMix` -> `hard-mix`,
  `dkCl` -> `luma-darken-only`, `lgCl` -> `luma-lighten-only` and
  `diss` -> `dissolve` (R6). Their projections differ from Doctored's by up to
  8133 codes — different blend arithmetic, and a stochastic Dissolve — while
  the merged preview agrees at 0 codes.
- Its layer-mode nicks for the four HSL keys are `hue ` -> `lch-hue`,
  `sat ` -> `lch-chroma`, `colr` -> `lch-color` and `lum ` -> `lch-lightness`
  (R9): GIMP's LCH modes, so its projection differs from Doctored's by up to
  19082 codes (recorded) while the merged preview agrees at 0 codes.
- A 16-bit clipping run is wrapped in the same one synthesized folder as an
  8-bit one, and its layered projection matched the stored composite exactly
  (R5).

### Reproduction

```sh
python3 -B scripts/psd-exchange-probe/run_reopen.py \
  --gimp /Applications/GIMP.app/Contents/MacOS/gimp-3.2 \
  --source core/tests/fixtures/psd_10b \
  --output ~/devel/doctored-fixtures/phase10-receipts/R4/corpus \
  --state ~/devel/doctored-fixtures/phase10-receipts/R4/gimp-state
```

The launcher pins the executable, requires fresh output and private state,
hashes every input, verifies each `gimp.json` against the files beside it, and
exits non-zero if any case recorded an `error` — a failed read is kept as the
receipt rather than repaired. `--cases` names the case folders to expect
(comma-separated; the default is the seven-case 10B corpus), so a smaller
authored set can use the same launcher. Then:

```sh
python3 -B scripts/verify_phase10b_external.py --oracle-only
python3 -B scripts/verify_phase10b_external.py \
  --app build/Build/Products/Release/Doctored.app --output <new dir outside the repo>
```

## Phase 10 receipts

`scripts/verify_phase10_receipts.py` takes the eight closeout receipts that are
not the 10B corpus: R1-R3 drive the built app over MCP (the eight binary-body
adjustment kinds, the three descriptor kinds, and the `lfx2`/`iOpa`/Blend If
carry with its bake case) and R5-R9 author a 16-bit PSD in the app and hand it
to GIMP through `run_reopen.py --cases`, gating a clipping chain, the four
threshold-mode blend keys, Blend If, an adjustment record's blend key and the
four HSL-mode blend keys. It
writes one `receipt.json` per receipt, recording the app and GIMP SHA-256, the
call count, every check and any error.

```sh
python3 -B scripts/verify_phase10_receipts.py \
  --app build/Build/Products/Release/Doctored.app \
  --gimp /Applications/GIMP.app/Contents/MacOS/gimp-3.2 \
  --output ~/devel/doctored-fixtures/phase10-receipts
```

Its `--output` may pre-exist (R4 creates it); each selected `<out>/<receipt>`
must be fresh, and no sibling receipt folder is read or written.
