# samples

Six generated test images, committed with the first two commits. None is a
photograph, and none is third-party or personal material.

- `plasma.jpg`: a synthetic plasma fractal, 1024×768
  (`scripts/verify_v1_develop_filter.py` uses it as its fixture).
- `portrait-exif6.jpg`: the same synthetic plasma fractal with an EXIF
  orientation tag; not a portrait or any other photograph
  (`designs/phase-9-live-filters-status.md`, `core/tests/precision_inpaint.rs`).
- `layered.psd`: a layered PSD written by ImageMagick (`core/src/psd_locks.rs`).
- `gradient.png`, `gradient.psd`, `shapes-alpha.png`: generated; the tree
  records no generator command.
