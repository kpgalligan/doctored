# #547 Lab Color admission drive

Every memory plan the Lab Color document mode added (phases 1, 1b, 2f and 3a) is held to the
measured-admission rule on a built Release app: plan ≥ ⌈1.25 × max E⌉, where E
is `ri_interval_max_phys_footprint` − pre around ONE MCP call, read once the
footprint has been still for 2 s, so the host's display preparation of the
result — the core's Lab view (`lab_convert::lab_display`, Display P3 at u16)
and its narrowing — is inside the interval.

```sh
make app
DR_VERIFY_APP=build/Build/Products/Release/Doctored.app \
  python3 -B scripts/lab-admission/measure.py --runs 3 [--output <folder outside the repo>] [--cases a,b]
```

Each case runs in its own fresh owned app (`scripts/host-source-probe/support.py`'s
`HostApp` through `scripts/cmyk-admission/measure.py`'s harness, `DR_BACKGROUND=1`,
a dummy API key, private preference folders) at `DR_WORKING_LIMIT_BYTES` 8 GiB with
`MallocLargeCache=0`, beside a baseline twin that calls `resource_usage` instead,
`--runs` times. The fixtures are 24 MP (6000 × 4000): a 16-bit RGB PNG from
`scripts/host-still-admission/author.swift`, the gray drive's 16-bit gray PNG and
the CMYK drive's 16-bit Generic CMYK TIFF.

The cases and the plan each is held to are listed in `measure.py`'s docstring.
The figures behind the plans are the per-pixel terms beside the code they charge:
`lab_convert::TO_LAB_BYTES_PER_PIXEL` (Image ▸ Mode ▸ Lab Color and RGB Color on a
Lab document), `gray_io::OPEN_BYTES_PER_PIXEL` (New Lab Color takes New
Grayscale's 16-bit charge, `gray_io::open_scratch`), and since phase 1b
`lab_rgb::CONVERT_BYTES_PER_PIXEL` (`export-lab-png16`, `export-lab-jpeg8`: the
export's Lab → RGB stage) and `lab_save::SAVE_BYTES_PER_PIXEL` (`save-lab-tiff16`:
the Lab TIFF), and since phase 2f `lab_channel_filter::PASS_BYTES_PER_PIXEL` and
`VIEW_BYTES_PER_PIXEL` (`filter-emboss-16`, `filter-edge-detect-16`,
`filter-lens-blur-16`, `filter-reduce-noise-16`, `filter-smart-sharpen-16`: the
luma filters' one-channel passes, each plan read from `get_precision`'s
`filter_plans` before the call), and since phase 3a `lab_tiff_read::open_plan`
(`open-lab-tiff16`, `open-lab-tiff8`: the flat Lab TIFF open, the computed `extra`
term for the wider decoded samples plus, since phase 3b, the Lab open's own
measured `lab_limits::OPEN_BYTES_PER_PIXEL` in place of Gray's 41; a miss raises
that Lab term). A miss raises the term that charges it, never the bound and never a shared
constant. The receipt
(`receipt.json`) records every run, the baselines, the plan, the need and the
ratio; the run's measurement record stays in the handoff directory, not in the
repository.
