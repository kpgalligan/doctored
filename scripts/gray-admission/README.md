# Grayscale admission drive

Every memory plan the Grayscale document mode added (phases 1a, 1b, 2e, 4 and 5a) is held to the
measured-admission rule on a built Release app: plan ≥ ⌈1.25 × max E⌉, where E
is `ri_interval_max_phys_footprint` − pre around ONE MCP call, read once the
footprint has been still for 2 s, so the host's display preparation of the
result is inside the interval.

```sh
make app
DR_VERIFY_APP=build/Build/Products/Release/Doctored.app \
  python3 -B scripts/gray-admission/measure.py --runs 3 [--output <folder outside the repo>] [--cases a,b]
```

Each case runs in its own fresh owned app (`scripts/host-source-probe/support.py`'s
`HostApp` through `scripts/cmyk-admission/measure.py`'s harness, `DR_BACKGROUND=1`,
a dummy API key, private preference folders) at `DR_WORKING_LIMIT_BYTES` 8 GiB with
`MallocLargeCache=0`, beside a baseline twin that calls `resource_usage` instead,
`--runs` times. The fixtures are 24 MP (6000 × 4000): 16- and 8-bit gray PNGs, a
16-bit BlackIsZero TIFF and a one-component JPEG (`magick`; the case is skipped and
said so when it is absent), all a gray gradient with per-column noise; the CMYK
drive's 16-bit Generic CMYK TIFF; and a 16-bit RGB PNG from
`scripts/host-still-admission/author.swift`.

The cases and the plan each is held to are listed in `measure.py`'s docstring.
The figures behind the plans are the per-pixel terms beside the code they charge:
`gray_io::OPEN_BYTES_PER_PIXEL` (the opens and New Grayscale) and
`gray_convert::TO_GRAY_BYTES_PER_PIXEL` (Mode ▸ Grayscale) and
`gray_export::EXPORT_BYTES_PER_PIXEL` (the one-channel export's buffers, phase 1b,
beside the typed export plan's own terms). Phase 4's two plans are the clipboard's:
`ClipboardPayload::converted_to_gray`'s (bookkeeping + twice the Gray payload, held
through the paste beside the converting arm's insertion and display term,
`doc_paste_layers::LAYER_PASTE_MARGIN_PER_PIXEL` over the canvas as a CMYK paste charges it;
`paste-rgb-into-gray-16`; the conversion term alone was 0.389 × the measured peak) and `GrayClipboardPayload::gray_tiff`'s
(bookkeeping + 8 B a window pixel, beside the typed copy's host stage; `copy-gray-16`).
Those cases run on their own private pasteboard (`DR_PASTEBOARD_NAME`). RGB Color from a Gray
document is Convert to Profile's own plan (with the prepared pipeline's scratch).
Phase 5a's six plans are `gray_profile`'s and HDR Toning's 32-bit Gray arm: Assign's colour
plan, Convert's conversion plan, 16 → 8's rounded planes and 8/16 → 32's promotion, each beside
the result's display preparation (`TO_GRAY_BYTES_PER_PIXEL`, doubled for a 32-bit result) and
the destination policy's bytes (`assign-gray-16`, `convert-gray-16`, `depth-gray-8`,
`depth-gray-32`), and the tone stage of a 32-bit one-raster Gray document, at 8 bits also the
host's depth-8 narrow inside the same job (`gray_profile::depth_eight_bytes`;
`hdr-gray-32-16`, `hdr-gray-32-8`).
A miss raises the term that charges it, never the bound and never a shared
constant. The receipt (`receipt.json`) records every run, the baselines, the
plan, the need and the ratio; the run's measurement record stays in the handoff
directory, not in the repository.
