# Indexed Color admission drive

Every memory plan the Indexed Color document mode added (phases 1a, 1b, 1c, 2b, 2c, 2d and 3a) is held to the
measured-admission rule on a built Release app: plan ≥ ⌈1.25 × max E⌉, where E is
`ri_interval_max_phys_footprint` − pre around ONE MCP call, read once the footprint has
been still for 2 s, so the host's display preparation of the result is inside the
interval.

```sh
make app
DR_VERIFY_APP=build/Build/Products/Release/Doctored.app \
  python3 -B scripts/indexed-admission/measure.py --runs 3 [--output <folder outside the repo>] [--cases a,b]
```

Each case runs in its own fresh owned app (`scripts/host-source-probe/support.py`'s
`HostApp` through `scripts/cmyk-admission/measure.py`'s harness, `DR_BACKGROUND=1`, a
dummy API key, private preference folders) at `DR_WORKING_LIMIT_BYTES` 8 GiB with
`MallocLargeCache=0`, beside a baseline twin that calls `resource_usage` instead,
`--runs` times. The fixtures are 24 MP (6000 × 4000): a 16-bit RGB PNG from
`scripts/host-still-admission/author.swift`, the gray drive's 8-bit gray PNG, and an
8-bit palette PNG, its twin with a `tRNS` transparent index, and an uncompressed 8-bit
palette TIFF written by `measure.py`.

The cases and the plan each is held to are listed in `measure.py`'s docstring. The
figures behind the plans are the per-pixel terms beside the code they charge:
`indexed_io::OPEN_BYTES_PER_PIXEL` (`open-indexed-png`, and `open-indexed-tiff` beside
`indexed_tiff::open_plan`'s one byte a sample), `indexed_convert::TO_INDEXED_BYTES_PER_PIXEL`
(`to-indexed-gray8`, and phase 1c's `set-color-table`; the host's 8-bit RGB source and
Indexed → RGB terms in
`EditorViewController+CmykMode.swift` and `+IndexedMode.swift`), beside the exact 32 MiB
nearest-colour memo, the Adaptive palette's exact 64 MiB histogram and 8 B a distinct colour
(`to-indexed-adaptive`), and `indexed_export::EXPORT_BYTES_PER_PIXEL` (`export-indexed-png`,
`export-indexed-gif`, `export-indexed-tiff` and their `-alpha` twins). Phase 1c's
`to-cmyk-from-indexed` and `to-lab-from-indexed` are the RGB → CMYK and RGB → Lab plans of the
expanded palette, whose samples are shared. Phase 2b's `brush-indexed` is the Brush's stroke job
plan, whose edit adds `indexed_paint::INDEXED_PAINT_BYTES_PER_PIXEL` and the nearest-colour memo
to the native paint's. Phase 2c's `rotate90-indexed` (the exact path, a carrier of the index
plane) and `rotate-indexed` (the resample path, re-indexed to the nearest entry) are the
geometry plan, whose edit adds (old + new canvas) × `indexed_geometry::GEOMETRY_BYTES_PER_PIXEL`
and the memo to the RGB geometry's. Phase 2d's `transform-nearest-indexed` (a layer
translation on the carrier) and `transform-indexed` (a bicubic layer rotation, re-indexed) are
the layer transform's plan, the RGB `affine_set` plan plus the same `transform_scratch` term;
`paste-indexed` (a full-canvas RGB copy pasted centered on the case's own private pasteboard,
`DR_PASTEBOARD_NAME`) is `indexed_clipboard::paste_plan`: the RGB paste's term, (canvas +
payload) × `indexed_clipboard::PASTE_MERGE_BYTES_PER_PIXEL` for Merge Down and the re-index's
bytes. Phase 3a's `convert-profile-indexed` (`convert_profile` display_p3 of the converted
24 MP document: the colour table converted, the storage assigned, the one layer re-rendered) is
`indexed_profile::plan`: the storage's assign plan plus the Color Table edit's re-render,
`TO_INDEXED_BYTES_PER_PIXEL`; a miss raises that term and re-measures every case that uses it.
The Move tool's whole-pixel moves reuse `transform-nearest-indexed`'s exact path. The Indexed
Color PSD's `open-indexed-psd` (`open_document` psd_mode composite of a 24 MP colour-mode-2 PSD
written by `measure.py`) is `indexed_io::open_scratch`, `OPEN_BYTES_PER_PIXEL`; its
`export-indexed-psd` (`export_psd` of the transparent-index palette PNG's document) is
`indexed_psd_worst_case` plus `EXPORT_BYTES_PER_PIXEL`. A miss raises the new
term that charges it, never the bound and never a shared constant. The receipt
(`receipt.json`) records every run, the baselines, the plan, the need and the ratio; the
run's measurement record stays in the handoff directory, not in the repository.
