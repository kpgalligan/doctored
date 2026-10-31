# Bitmap admission drive

Every memory plan the Bitmap document mode added (phases 1a to 2a-ii) is held to the measured-admission
rule on a built Release app: plan ≥ ⌈1.25 × max E⌉, where E is
`ri_interval_max_phys_footprint` − pre around ONE MCP call, read once the footprint has been
still for 2 s, so the host's display preparation of the result is inside the interval.

```sh
make app
DR_VERIFY_APP=build/Build/Products/Release/Doctored.app \
  python3 -B scripts/bitmap-admission/measure.py --runs 3 [--output <folder outside the repo>] [--cases a,b]
```

Each case runs in its own fresh owned app (`scripts/host-source-probe/support.py`'s `HostApp`
through `scripts/cmyk-admission/measure.py`'s harness, `DR_BACKGROUND=1`, a dummy API key,
private preference folders) at `DR_WORKING_LIMIT_BYTES` 8 GiB with `MallocLargeCache=0`, beside
a baseline twin that calls `resource_usage` instead, `--runs` times. The fixture is the gray
drive's 24 MP (6000 × 4000) 8-bit gray PNG (`scripts/gray-admission/measure.py`'s `gray_png`).

The cases and the plan each is held to are listed in `measure.py`'s docstring:
`to-bitmap-threshold`, `to-bitmap-diffusion`, `to-bitmap-resample` (Output resolution 1.25 × the
document's), `to-bitmap-halftone`, `to-bitmap-custom`, `to-gray-from-bitmap` (Size Ratio 1) and
`to-gray-ratio-4`, and (phase 1c) `open-bitmap-png`, `open-bitmap-tiff`, `open-bitmap-psd`,
`export-bitmap-tiff` and `export-bitmap-psd`. Every conversion plan is the core's
(`bitmap_convert`): output pixels × `TO_BITMAP_BYTES_PER_PIXEL` (Gray's measured conversion
shape) beside Diffusion's error rows (phase 2b's Sierra Lite; 1b's Floyd–Steinberg also held a
32 MiB nearest-colour memo), the resample's own geometry plan and
Flatten Image's render when they run, and bookkeeping. Phase 1c's 24 MP 1-bit PNG, TIFF and PSD
are written by the drive; an open's plan is pixels × `bitmap_io::OPEN_BYTES_PER_PIXEL` (42) +
the file + bookkeeping, the kept TIFF export's the reply's `working_memory_plan_bytes`
(`bitmap_export::EXPORT_BYTES_PER_PIXEL`), and the PSD export's `inspect_psd_export`'s. Phase
2a-i's edits, `brush-bitmap`, `rotate90-bitmap` and `crop-extend-bitmap` (a crop past the
canvas with Delete Cropped off, whose inner plan is near zero), each name their tool, their
arguments and the reply key their plan is read from (`working_memory_plan_bytes`, or `crop`'s
`planned_bytes`); each plan is the inner op's on the Gray storage plus the output canvas ×
`bitmap_edit::BITMAP_EDIT_BYTES_PER_PIXEL` (34; 31 as declared missed on `crop-extend-bitmap`)
and bookkeeping. Phase 2a-ii's resampled-geometry cases, `rotate5-bitmap` (`rotate` 5°
bicubic) and `transform-bitmap`, went with phase 2b, which refuses arbitrary rotation and
`transform_layers` on a Bitmap document (their record stays in the ADR's table). A miss
raises the case's own per-pixel term, never the bound and never a shared constant. The receipt
(`receipt.json`) records every run, the baselines, the plan, the need and the ratio; the run's
measurement record stays in the handoff directory, not in the repository.
