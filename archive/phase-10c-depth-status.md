# Phase 10C-1 — genuine 16-bit RGB PSD import and export status

Slice 1 of 10C. A 16-bit RGB classic PSD now opens as a 16-bit
(`PrecisionDocument`) document, and a 16-bit document exports as a 16-bit PSD.
Depth follows the DOCUMENT on both sides: an 8-bit document still writes 8-bit,
a 16-bit one writes 16-bit, and there is no cross-depth conversion in either
direction. Slice 2 — preserve/rasterize/refuse rules per construct (styles,
adjustments, editable type/shapes, smart objects, Blend If, group clipping) —
is untouched and keeps 10C open.

Everything in the 10A import contract and the 10B export contract applies
unchanged at 16 bits: the same container framing, the same Raw/PackBits
channels, the same ordinary 20-byte masks, the same folder records, the same
refusal order, the same "never silent" rule.

## Accepted contract at 16 bits

| Carried | How | Round-trips through Rasterize itself |
| --- | --- | --- |
| Canvas, 16-bit RGB | Header depth 16, mode 3; every plane is big-endian u16, Raw or PackBits over `width * 2` byte rows | Yes |
| Raster layers | One typed raster layer per record, straight RGBA16, hidden RGB under alpha 0 and off-canvas rectangles retained | Yes, sample-exact |
| Layer/group masks | Ordinary absolute (or raster-relative, normalized once) 20-byte body plus one `-2` plane, as a `PrecisionMask` with u16 coverage, outside default 0/255, enabled state, independent (unlinked) placement | Yes: samples, size, offset, outside default, enabled |
| Groups | `lsct` folders become typed pass-through or isolated groups; nesting, order and empty groups retained | Yes, except open/closed state (see exclusions) |
| Opacity / visibility | Record opacity byte / flag bit 1 | Yes, to the stored byte |
| Blend keys | `psd_group_records::PSD_BLEND_KEYS` forwards, `psd_export_preflight::blend_key` backwards, filtered through `precision_layer_blend::COLOR_MODES` | Yes for every mode the typed model represents |
| ICC profile | Image resource 1039, parsed ONCE into the single profile handle every typed plane shares | Yes, byte-identical |
| Print resolution | Image resource 1005 | Yes |
| Stored composite | Decoded at 16 bits, merged transparency reconstructed from white with the exact widened-integer twin of the byte formula | Alpha exactly; colour exactly at alpha 0 and 65535, within `ceil(32767.5/a)` codes elsewhere |

The export is written from the HOST's authoritative projection of that exact
revision (`rz_precision_doc_save_psd` requires it, as `save_projection16`
does); nothing is rendered inside the writer.

## Explicit exclusions

Refused (with a reason, before a byte is written or read):

- 16-bit grayscale, 32-bit float, CMYK/Lab, PSB, `Lr32`. ZIP and
  ZIP-with-prediction are no longer refused everywhere: since Run 4 they are
  DECODED for 16-bit layer and mask planes (`psd_zip`), because that is the
  only form Photoshop's own 16-bit files come in. An 8-bit ZIP channel and a
  ZIP stored composite still refuse with their existing sentences.
- A 16-bit file through the byte importer or the flat image open: both refuse
  and name the 16-bit path rather than converting a depth silently.
- An 8-bit file through `rz_precision_doc_open_psd`.
- Cross-depth export: an 8-bit PSD from a 16-bit document requires the
  existing deliberate conversion first, and vice versa.
- The 10B refusal set, unchanged: a side past 30000 pixels, a worst-case file
  past 2 GiB, typed adjustment layers, mask density or feather.

Reported, not silent:

- **Clipping.** Now retained: since [the 10D clipping slice](phase-10d-clipping-status.md)
  a 16-bit document evaluates clipping, so a clipped PSD raster or folder
  imports clipped with no loss sentence, and the export writes the clipping
  byte back. (At this slice's handoff it was imported unclipped and reported.)
- **Unrepresentable blend keys.** A key outside the typed model's frozen mode
  set is imported as Normal, reported per record. Since the
  [threshold slice](phase-10d-threshold-modes-status.md) no such raster or
  folder key remains (the [HSL slice](phase-10d-hsl-modes-status.md) had left
  only `diss`, `dkCl`, `lgCl` and `hMix`). Since the
  [adjustment-blending slice](phase-10d-adjustment-blending-status.md) an
  adjustment record carries its key too, at both depths.
- **Folder open/closed state.** Typed groups have no disclosure state, so the
  import does not retain it and the export writes every folder open.
- The 10B loss/observation set otherwise applies unchanged, including the
  2 GiB reopen-envelope loss, the white-matted preview observation and the
  1/255 opacity-step observation.

## How it is built

- `psd_container` admits depth 16 for colour 3 only, exposes `sample_bytes()`
  and decodes the saved composite at 16 bits (`composite16`).
- `Container::layer_info` is the ONE place that decides WHERE a file's layer
  records are: the ordinary layer-information sub-section when it is
  non-empty, else — at depth 16 only — the first `Lr16` tail block after the
  global-mask section, walked as Adobe's `8BIM` + key + `u32` length with each
  block padded to a MULTIPLE of four. Photoshop's own 16-bit files have an
  EMPTY layer-information section and keep their records there; before Run 4
  such a file framed no layer at all and its layered choice resolved to the
  stored composite. `merged_alpha`'s sign comes from the same body, so it is
  read wherever the records live. `8B64` or a bad tail signature refuses the
  LAYERED choice only.
- `psd_zip::unpack_zip16` is the ONE inflate seam in the crate (`miniz_oxide`,
  pure Rust, already in the lock file through `png`/`tiff`): it validates the
  zlib header, inflates with the ADMITTED plane size as the limit, refuses a
  stream that decodes to any other length, and undoes the per-row big-endian
  u16 prediction of compression word 3. Words 2 and 3 are admitted for layer
  and mask planes at `sample_bytes == 2` only; `validate_channel` frames the
  header, `decode16`/`attach16` decode. The inflated plane is
  `largest x sample_bytes`, already inside `peak_forecast`'s
  `largest x 4 x sample` term, so no budget changed.
- `unpack_packbits` is the ONE in-crate PackBits DECODER, the twin of the
  master `validate_packbits`, used ONLY by 16-bit planes. It exists because the
  `psd` crate is 8-bit only and caps a header dimension at 30000, so mapping a
  16-bit row onto a doubled-width 8-bit plane fails past 15000 pixels. 8-bit
  planes still decode through that crate (`decode_composite`).
- `psd_export_preflight` is the depth-agnostic preflight both document types
  fill in through `ExportInventory`. The refusal order, every sentence and the
  worst-case arithmetic are shared; only the first general-limitation sentence
  and the `sample_bytes` multiplier differ by depth.
- `psd_layer_pixels::peak_forecast` gained `sample_bytes`, so import admission
  and the export preflight still evaluate ONE forecast at either depth.
- The import job control is begun ONCE in `psd_import16::open16` and polled
  between layer planes; every `PrecisionImage::from_rgba16` receives `None`,
  because a single-use token cannot be begun twice.

## Verification receipts

All passing at the commit that introduced this document:

- `core/tests/psd_depth16.rs` — nine cases, with `tests/common/psd16_fixture.rs`
  (an independent 16-bit builder/reader with its own PackBits packer/unpacker
  and its own matte arithmetic) as the only oracle:
  - `unpack_packbits` inverts that independent packer at row widths 2, 256,
    258 and 514 bytes, including runs spanning big-endian sample pairs, and
    refuses a row that decodes to the wrong width.
  - A hand-built fixture (nested Pass Through/Multiply folders, an empty
    group, Raw and RLE rasters, a negative offset, hidden RGB under alpha 0,
    samples one code apart, an RLE mask outside 255, a disabled Raw mask
    outside 0, a group mask, a clipped layer, a `hue ` layer, Display P3,
    300 × 240 PPI, a merged-alpha composite) reports `depth` 16,
    `import_precision` `rgba16`, both choices available and exactly two
    `verified_losses` — the clipped layer and the `hue ` key. `rz_doc_open_psd`
    refuses it.
  - `rz_precision_doc_open_psd` mode 0, opened with a LIVE
    `rz_job_control_create()` token, matches names, tree, depths, opacity,
    visibility, blend, offsets, samples, mask samples/size/offset/outside/
    enabled, profile bytes and PPI exactly; the token's scratch is 0
    afterwards, and both that consumed token and a pre-cancelled one refuse.
  - Mode 1 equals the fixture's own rational un-matte oracle exactly; a second
    fixture with a zero-length layer section reports `layered_available: true`
    and opens mode 0 as one raster equal to mode 1.
  - A Rust-built typed document (groups, Multiply, a disabled mask, hidden RGB,
    a 360-byte name) exports, is read by the independent reader (record order,
    dividers, bounds, opacity byte, flags, mask body, planes, ICC, PPI) and
    reopens through `rz_precision_doc_open_psd` exactly. Flattened mode equals
    the supplied projection. The preview equals the independent matte exactly
    and recovers within `ceil(32767.5/a)`.
  - Each refusal (typed adjustment, mask density/feather, a 30001-pixel side)
    is reported AND leaves a pre-existing destination byte-identical; a reduced
    `max_file_bytes` refuses while a reduced envelope is a loss.
  - A NULL sweep over the three new exports.
- `core/tests/psd_zip16.rs` — six cases for the Run 4 reader, with
  `psd16_fixture`'s OWN stored-block zlib writer, Adler-32, delta encoder and
  delta decoder as the only oracle:
  - `unpack_zip16` inverts that encoder at widths 1, 64 and 300 px, plain and
    predicted, over samples holding long equal runs, a wrap across 0 and
    one-code neighbours, and a predicted row never predicts into the next.
  - A short plane, an over-limit stream (the inflater's limit IS the admitted
    plane size), a corrupted Adler-32 and four malformed zlib headers all
    refuse.
  - A `fixture_lr16` file — empty layer-information section, a leading
    zero-length `Patt` tail block, a NEGATIVE record count, Raw + ZIP +
    ZIP-predicted rasters, a predicted `-2` mask, a folder, and a pixel-less
    `SoCo` record whose zero-rectangle mask body sits beside a canvas-sized
    compression-3 plane that is never decoded — imports layered at `rgba16`
    with no spurious mask loss and opens sample-exact through
    `rz_precision_doc_open_psd`, stored composite included.
  - An ordinary non-empty layer-information section beside a nonsense `Lr16`
    wins; a bad tail signature refuses LAYERS only and leaves the stored
    composite available; an 8-bit ZIP channel and a ZIP composite keep their
    existing sentences.
- `core/tests/psd_photoshop_depth16.rs` — the committed Photoshop-authored
  family (below), which is where REAL compressed ZIP streams are read: the
  report facts, the typed open (six roots, the set's Multiply member at
  opacity byte 153, the untrimmed 64 x 64 mask, the enabled drop shadow among
  Photoshop's four disabled slots, the carried `curv` meta, the canvas-sized
  transparent empty layer) and Base's decoded plane against
  `ps16(v) = round(round(v * 32768 / 255) * 65535 / 32768)` of `base.png` for
  every sample — the independent oracle for the ZIP-with-prediction decode.
  The 8-bit twin pins the divergences: `SoCo` as a canvas-sized RLE raster
  with its marker and irrelevant-pixels losses, and the mask TRIMMED to
  32 x 36 at (28, 24).
- `core/tests/psd_export.rs::eight_bit_writer_bytes_match_the_committed_goldens`
  — the 8-bit writer's exact FILE BYTES against
  `tests/fixtures/psd_10c/nested.psd` and `flat.psd`, committed goldens
  generated by the UNMODIFIED writer before this work began. Regenerate them
  only with an intentional 8-bit writer change, with
  `RZ_PSD_EXPORT_FIXTURES=tests/fixtures/psd_10c cargo test --release --test psd_export`.
- The whole existing PSD suite (`psd_export`, `psd_tests`, `psd_group_import`,
  `psd_raster_masks`, `psd_relative_masks`, `psd_feature_markers`,
  `psd_import_notice`, `psd_layer_pixels`, `psd_composite_transparency`) passes
  untouched, as do `make test`, `make lint`, `make typecheck` and `make app`.
- Driven over MCP against the built app: `inspect_psd` on a written 16-bit
  file, `open_document psd_mode: layers`, `get_document`, `inspect_psd_export`
  (`depth` 16), `export_psd` (`bit_depth` 16), then `inspect_psd` and
  `open_document` on the result; `export_psd` on an 8-bit `.rz` still reports
  `bit_depth` 8.

## Open

- 10C-2: per-construct preserve/rasterize/refuse rules — styles, adjustment
  layers, editable type and shapes, smart objects, Blend If, group clipping —
  on either depth. No `lfx2`/`lrFX`/`TySh`/adjustment descriptor is read or
  written here. Slice 1 of 10C-2 has since fixed one rule per construct and
  shipped the ones that need no descriptor codec
  ([the 10C construct status](phase-10c-constructs-status.md)): group clipping
  is retained at 8 bits, pixel-less adjustment/fill records are omitted with a
  report instead of refusing, empty records import empty, and an 8-bit export
  rasterizes a styled layer. Slice 2 phases 1–2 carry eleven adjustment
  kinds — the eight binary bodies (`nvrt`, `thrs`, `post`, `levl`, `curv`,
  `blnc`, `hue2`, `phfl`), the binary `expA`, and the descriptor bodies
  `vibA`/`blwh` — as typed adjustment layers on import and write the matching
  Rasterize ops as those records on export, at 16 bits too (an imported
  `expA` carries `document_v2`, so it evaluates the same at both depths).
  Slice 3 carries layer effects (`lfx2`), fill opacity (`iOpa`) and Blend If
  (the record's blending ranges) in BOTH directions at 16 bits as well: an
  authored record imports as a typed style through `with_layer_style` (a style
  a Pass Through folder or an adjustment record cannot hold is reported and
  discarded, and the file's global light resolves into each effect, since a
  16-bit document has none), and a typed style is written as those three
  constructs with the default light in image resources 1037/1049; the bake is
  the fallback for what Photoshop cannot hold. **Phase 12E-3c** puts the two
  remaining blending options on that same style at this depth too: `knko`
  (shallow/deep) and `infx` are read and written on rasters and on OPENING
  folder records, after `iOpa` and before `lclr`, only when the style is
  written rather than baked and only when the key is not at its default, so a
  key-free 16-bit export is byte-for-byte the previous writer's. A file whose
  bottom record is Rasterize's structural Background but NOT a true Photoshop
  Background carries one disclosure sentence at this depth as well (D13,
  carryover P7-38), and **12E-3d** measured that divergence at this depth:
  inside the hole Photoshop is white exactly and Rasterize is the bottom
  layer exactly, the two differ by the full 65535 codes there, they agree to
  the code outside it, and one reveal-all mask on the bottom record makes the
  whole frame agree to the code. `SoCo` (no Rasterize fill
  adjustment) remains open.
- 16-bit grayscale, ZIP, PSB, 32-bit float, CMYK/Lab.
- Typed clipping (10D) and PSD clipping in a 16-bit document are now CLOSED by
  [the 10D clipping slice](phase-10d-clipping-status.md); its GIMP reopen
  receipt for a 16-bit clipped export was taken there on 2026-09-16
  (`~/devel/rasterize-fixtures/phase10-receipts/R5/receipt.json`, app SHA-256 `084b4be2b97b1c84b9749e861c066bec9b72abbefb627c37f7e7c9d7aa8052a6`: one synthesized folder, 0-code
  merged and layered agreement). Three further Rasterize-written 16-bit exports
  reopened in the same application that day — the four threshold-mode blend
  keys (R6), Blend If (R7) and an adjustment record's blend key (R8) — so this
  slice's export side now has independent 16-bit receipts for clipping, blend
  keys, blending ranges and adjustment records.
- The IMPORT side is now third-party-authored as well (Run 4 F1, taken
  2026-09-16). `core/tests/fixtures/psd_photoshop/depth16/` (148 KB, eight
  files) is a document **Adobe Photoshop 2025 26.10.0 wrote itself** and saved
  at both depths: a 64 x 64 base raster, a 24 x 24 raster with a drop shadow,
  a 40 x 40 raster with a reveal-selection mask, a layer set holding one
  Multiply member at 60 %, a `curv` adjustment, a `brit` adjustment, a `SoCo`
  solid-colour fill and an empty layer. `source16.psd` has an EMPTY
  layer-information section, its ten records in an `Lr16` tail block and
  compression 3 on every raster and mask channel, so before this run it
  imported as 0 layers; it now imports LAYERED at `rgba16` with seven rows and
  exactly the two omitted records' sentences.
  Runner `scripts/verify_phase10_photoshop_depth16.py`, receipt
  `~/devel/rasterize-fixtures/photoshop/depth16/receipt.json`, app SHA-256
  `809b16289e8c0ae0da65087ef60edff3bf23c6101132ccdc9129a722e4c46512`,
  Photoshop executable SHA-256
  `a502d3b62279299a77f85d69a3d8f3d5379e0fb406d40d746a00ee49031bfa04`.
  Gates D1 (framing), D2 (import) and D3 (base exact at 0 codes) all pass, and
  a second authoring run reproduced the same record names, mask bodies and
  adjustment/fill block bodies (`authoring_agrees_with_fixture` true,
  `~/devel/rasterize-fixtures/photoshop/depth16-reauthor/receipt.json`).

  | case | tolerance (8-bit codes) | max | mean | pass |
  | --- | --- | --- | --- | --- |
  | `base` | 0 (gate D3) | 0 | 0 | yes |
  | `empty` | 0 | 0 | 0 | yes |
  | `masked` | 2 | 0 | 0 | yes |
  | `group` | 2 | 0.0078 | 0.0002 | yes |
  | `shadowed` | 4 | 0.249 | 0.006 | yes |
  | `curves` | 8 | 5.0233 | 2.3219 | yes |
  | `carried` (base+shadowed+masked+group+curves+empty) | 8 | 5.0233 | 2.0245 | yes |
  | `brightness` | none by construction | 33.8054 | 16.3724 | recorded |
  | `fill` | none by construction | 107.9222 | 51.4657 | recorded |
  | `all` | none by construction | 107.9222 | 46.7521 | recorded |

  The last three hold a record Rasterize omits (`brit`, `SoCo`), so the gap is
  the omitted adjustment's own appearance, not an arithmetic error; no
  tolerance was edited after the run. Fixture SHA-256s: `description.json`
  `a02367fc…`, `base.png` `34bf9c57…`, `shadowed.png` `4d43ccae…`,
  `masked.png` `87cd4912…`, `inner.png` `de6cc1a3…`, `source.psd`
  `b7511ff3…`, `source16.psd` `4c5ee34a…`, `photoshop.json` `8bb4977c…`.
  Photoshop's own 16-bit renders (the reference pixels) are under
  `~/devel/rasterize-fixtures/photoshop/depth16/photoshop-depth16/`:
  `base_16.png` `9645be467755ff3286cfa9e4e06e752973f57cbb421b1304fd48086571b51d5b`,
  `carried_16.png` `bfc4fefb4c49897cbf5484a0557e0766c4966c1ce9b54f8ccd2ca7f7bc99c312`,
  `all_16.png` `b78e1bb72183e442149aac2a31fe56a2dc8068f75edefe8a1cc75fc569f00969`;
  Rasterize's own `rasterize_base_16.png`
  `a1d353187abfe447f19783b5ff00767697860ac3a216a6b754600502b604eb86` and
  `rasterize_carried_16.png`
  `22104b2ff96dd8678e991df4aed951e67bf2d6369575fcb2d486b066de3aaf14`.
- Independent-application reopen receipts are now CLOSED for this slice's
  export side. `scripts/psd-exchange-probe/reopen_gimp.py` reads the
  Rasterize-written 16-bit case `core/tests/fixtures/psd_10b/depth16/` in GIMP
  3.2.6, which loads it at `u16-non-linear` precision, and
  `scripts/verify_phase10b_external.py` gates its structure, big-endian u16
  planes, u16 mask coverage with outside 65535, the disabled u16 mask, the
  pass-through folder, Display P3 bytes and 300 × 240 PPI against an
  independent decode, plus a 0-code appearance and merged-preview agreement
  (threshold 514). The 16-bit IMPORT side is no longer a self-consistency
  claim either: GIMP's exporter writes no 16-bit PSD, but Photoshop's own
  16-bit file is the `depth16` family above.
