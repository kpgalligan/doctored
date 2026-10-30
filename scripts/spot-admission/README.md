# #547 spot channels 1c-1 and 1c-2a admission drive

Every memory plan term spot channels 1c-1 added — Merge Spot Channel at 16 bits, in Lab and in
CMYK, and the overprint of a flattened typed export (RGB16 PNG, 32-bit OpenEXR, a kept-CMYK JPEG)
— and every term 1c-2a added for a TIFF that keeps them (`open-tiff-rgb16`, `open-tiff-cmyk16`,
`save-tiff-rgb8`, `save-tiff-rgb16`, `save-tiff-cmyk16`; their open fixtures are written by a setup
app's own `save_copy tif`) is held to the measured-admission rule on a built Release app: plan ≥ ⌈1.25 × max E⌉, where E is
`ri_interval_max_phys_footprint` − pre around ONE MCP call, read once the footprint has been still
for 2 s.

```sh
make app
DR_VERIFY_APP=build/Build/Products/Release/Doctored.app \
  python3 -B scripts/spot-admission/measure.py --runs 3 [--output <folder outside the repo>] [--cases a,b]
```

Each case runs in its own fresh owned app (`scripts/cmyk-admission/measure.py`'s harness,
`DR_BACKGROUND=1`, a dummy API key, private preference folders) at `DR_WORKING_LIMIT_BYTES` 8 GiB
with `MallocLargeCache=0`, beside a baseline twin that calls `resource_usage` instead, `--runs`
times. The fixtures are 24 MP (6000 × 4000): a 16-bit RGB PNG from
`scripts/host-still-admission/author.swift` and the CMYK drive's 16-bit Generic CMYK TIFF; every
setup adds one spot channel at full coverage, the worst case for tile sharing.

The cases and the plan each is held to are listed in `measure.py`'s docstring. A miss raises the
arm's own per-pixel term in `core/src/spot_typed.rs` (`RGB16_BYTES_PER_PIXEL`,
`FLOAT_BYTES_PER_PIXEL`, `LAB_BYTES_PER_PIXEL`) or, in CMYK, the existing plan that charges the
on-paper plane the overprint fills in place (the merge's `cmyk_flatten_scratch_bytes`, the kept
JPEG's staged plan), never the bound and never a shared constant; a 1c-2a miss enlarges only the
spot term it measured (`with_tiff_spots`' admit term, `tiff_spot_scratch_bytes`,
`cmyk_save_scratch_of(Tiff)`'s spot term, `SpotChannels.tiffSaveScratch`). The receipt (`receipt.json`) records every run, the
baselines, the plan, the need and the ratio; the run's measurement record stays in the handoff
directory, not in the repository.
