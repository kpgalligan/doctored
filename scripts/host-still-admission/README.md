# Host-still admission drive

Host-still admission (`HostStillDecoder.scratchFixed8` + `scratchPerPixel8`·px and
the `Wide` pair, an affine figure per depth class used through
`HostSourceInput.scratch`) and plain PNG/JPEG/TIFF placement
(`ImageSourceFile.scratchPerPixel8` / `scratchPerPixel16`) are per-depth
figures measured from the real capture, open and place peaks. Their
measurement records (method, every case, floors and fit checks) sit in the
`///` blocks beside those constants. This drive pins what they admit, at the limit
each gate names (below), on a built Release app.

```sh
make app
python3 -B scripts/host-still-admission/verify.py \
  --app build/Build/Products/Release/Doctored.app [--output <folder outside the repo>] \
  [--measure RUNS]
```

`--measure RUNS` runs no gate. It records the wide-open footprint table (the 96
and 100 MP rows are whole opens; taken again on the open that lands one raster
layer) behind `HostStillDecoder.scratchFixedWide` / `scratchPerPixelWide`: heic10,
avif10 and png16 at 12, 48, 67, 96 and 100 MP, each in `RUNS` fresh apps at
8 GiB with `MallocLargeCache=0`, plus a baseline twin per run. E is
`ri_interval_max_phys_footprint` − pre around the MCP `open_document`, read
once the footprint has been still for 2 s. The table goes in `receipt.json`'s
`measurements`.

Run it outside the sandbox: it launches owned app instances
(`scripts/host-source-probe/support.py`'s `HostApp`, with `DR_BACKGROUND=1`, a
dummy `ANTHROPIC_API_KEY`, a free port and private `DR_*_DIR` folders) and
stops only those. It writes `receipt.json` to the output folder (a fresh
temporary folder when `--output` is omitted) and exits 0 only when every
must-pass gate passed.

## Fixtures

`author.swift` is a standalone ImageIO / Core Image author that imports no
Doctored code and self-checks every file it writes (dimensions, container
depth, gain-map presence). The drive builds it and writes `heic8`,
`heic8-gainmap`, `jpeg8` and `png8` fixtures at 12 MP (4032x3024) and 48 MP
(8064x6048) into its private folder, `heic10` and `png16` at 12, 48, 67
(9456x7096, the largest 4:3 16-bit image the core once kept), 96
(12000x8000) and 100 MP (12500x8000, the document limit), plus AVIF through
`avifenc -s 8 -y 420 [-d 10]` over the PNGs. Nothing is committed and no
personal photo is used; the receipt records each fixture's bytes and SHA-256.
`author.swift`'s `heic8-pair` kind writes a two-image HEIC whose primary image
is the second one. `/System/Library/Desktop Pictures/iMac Blue.heic` and
`Sonoma.heic` (both 6016x6016; Sonoma holds two images) are copied into the
run folder's `system/` and opened from there, because the path guard refuses
`/System`; the receipt records both SHA-256s of each.

## Gates

Each gate runs in its own fresh app, because there is no `close_document`
tool, and names its `document_id` on every call.

Each gate also runs at an explicit working limit, set through
`DR_WORKING_LIMIT_BYTES` for its own app and checked first
(`resource_usage.working_limit_bytes`): `MINIMUM_MAC_LIMIT` (8,589,934,592,
what the 16 GB minimum Mac gets automatically) for A1–A3, A7, A8, A10–A12, N1 and R;
the 2 GiB test pin for A0, A4–A6 and A9; and `PLAN_PROBE_LIMIT` (1 GiB) for
A7-plan. A single 8-bit still's whole open is charged up front
(`HostStillDecoder.openScratch`); a 48 MP HEIC's charge is 2,180,368,909
(gain map 2,176,287,400).

- **A0** `resource_usage.working_limit_bytes` is 2,147,483,648, the test pin
  the runner sets.
- **A1** at 8 GiB: a 48 MP 8-bit HEIC opens as an 8064x6048 8-bit document
  whose host source decoded 8064x6048 at `storage_depth` 8.
- **A2** at 8 GiB: the same for a 48 MP gain-map HEIC, which also reports
  `decode_request: "sdr_base"`.
- **A3** at 8 GiB: the same for a 48 MP 8-bit AVIF (open charge
  2,103,027,151).
- **A4** the iMac Blue HEIC, which was once refused, opens as 6016x6016.
- **A5** in a 12 MP 8-bit document: place the 48 MP HEIC embedded, place it
  linked from a private copy, relink to a second copy elsewhere, then
  replace the linked layer's contents with the 48 MP gain-map HEIC.
- **A6** 12 MP regression: heic8, avif8 and heic10 open; heic10 opens at 16
  bits with `precision: preserve`.
- **A7** at 8 GiB: a plain 48 MP JPEG and PNG each open. The core's
  flat JPEG/PNG open is charged whole up front
  (`FlatOpenAdmission.openScratch`, 34 B/px + 3·encoded).
- **A7-plan** at `PLAN_PROBE_LIMIT` (1 GiB): the 48 MP JPEG's open is
  refused with "…of working memory on its own…", so the flat open records
  a plan.
- **A8** at 8 GiB: a 48 MP JPEG places into a 48 MP 8-bit document — the
  measured `ImageSourceFile` place figure, with the place's display
  preparation charged with `ProjectionMargin`.
- **A9** a two-image HEIC opens its primary image: the `heic8-pair` fixture
  (primary index 1, 4032x3024 at 240 ppi, over a 2016x1512 index 0 at 72 ppi)
  opens at 4032x3024 with the primary image's 240 ppi, and Sonoma.heic opens.
- **A10** at 8 GiB: the 48, 67, 96 and 100 MP wide stills (heic10, avif10)
  open at 16 bits with `precision: preserve` — the measured wide fit
  (`WIDE_FIT_PASSED`; 45 MiB + 42 B/px, refit on the open that lands one
  raster layer). After the heic10-100 open, `add_layer_mask` on layer 0 is
  recorded, not gated: it succeeds (one 16-bit raster layer is 8 B/px, 800 MB
  at 100 MP of the typed decoded cap, then 1.6 GB and now 8 GiB, so a
  canvas-sized mask fits). Earlier, a wide still kept its 16-bit master
  beside the canvas raster (16 B/px), which filled the 1.6 GB cap, and the
  mask was refused. A refused mask must leave `get_document` unchanged (gated).
- **A11** at 8 GiB: the 96 MP wide stills open at 16 bits. They were once refused
  before the decode (one 16-bit image held at most 512 MiB); one image now
  holds the 100 MP document limit.
- **A12** at 8 GiB: the 96 and 100 MP 16-bit PNGs open with `precision:
  preserve` (the core's own 16-bit open), and `add_layer_mask` on layer 0 of
  the 100 MP one succeeds (an 8 B/px PNG is 800 MB of the typed decoded
  cap, now 8 GiB).
- **N1** at 8 GiB: `new_document` 10000x10000 at depth 16, `save_copy` to a
  `.rz` and `open_document` of it succeed; the reopened document is
  10000x10000 at 16 bits.
- **R** recorded only, at 8 GiB: the 48 and 67 MP 16-bit PNG opens, which are
  the core's own precision admission rather than the host estimate.
