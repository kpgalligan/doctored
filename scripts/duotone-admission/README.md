# #1 Duotone admission drive

Every memory plan the Duotone document mode added is held to the measured-admission
rule on a built Release app: plan ≥ ⌈1.25 × max E⌉, where E is
`ri_interval_max_phys_footprint` − pre around ONE MCP call, read once the footprint has been
still for 2 s, so the host's display preparation of the result (the Duotone view, then its
preview) is inside the interval.

```sh
make app
DR_VERIFY_APP=build/Build/Products/Release/Doctored.app \
  python3 -B scripts/duotone-admission/measure.py --runs 3 [--output <folder outside the repo>] [--cases a,b]
```

Each case runs in its own fresh owned app (`scripts/host-source-probe/support.py`'s `HostApp`
through `scripts/cmyk-admission/measure.py`'s harness, `DR_BACKGROUND=1`, a dummy API key,
private preference folders) at `DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside a
baseline twin that calls `resource_usage` instead, `--runs` times. The fixture is a 24 MP
(6000 × 4000) 16-bit sGray PNG written by `measure.py` from the gray drive's row.

The cases (`to-duotone-gray16`, `set-duotone`, phase 1b's `to-rgb-from-duotone`,
`to-cmyk-from-duotone`, `to-lab-from-duotone`, `to-indexed-from-duotone8`, `export-duotone-png16`
and `export-duotone-psd16`, and the close's `to-gray-from-duotone` and `to-bitmap-from-duotone8`)
and the plan each is held to are listed in `measure.py`'s docstring; `to-indexed-from-duotone8`
and `to-bitmap-from-duotone8` run on a 24 MP 8-bit sGray PNG written the same way. The figure
behind phase 1a's plans is `duotone_document::TO_DUOTONE_BYTES_PER_PIXEL` (the measured display
preparation of the result, whose rationale is in that module's doc) and bookkeeping; phase 1b's
add the render (`duotone_convert::RENDER_BYTES_PER_PIXEL` and the view's table) or the view's
scratch to the route's own plan; the close's are `duotone_gray`'s (the Grayscale stage at
`RENDER_BYTES_PER_PIXEL` a raster pixel and its table, then the display term toward Grayscale,
or Gray → Bitmap's own plan toward Bitmap). A miss raises `RENDER_BYTES_PER_PIXEL` (or that
phase's term), never the bound and never a shared constant. The receipt (`receipt.json`) records every run, the baselines, the plan, the
need and the ratio; the run's measurement record stays in the handoff directory, not in the
repository.
