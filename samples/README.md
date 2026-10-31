# samples

Six generated test images. None is a photograph, and none is third-party or
personal material.

- `plasma.jpg`: a synthetic plasma fractal, 1024×768
  (`scripts/verify_v1_develop_filter.py` uses it as its fixture).
- `portrait-exif6.jpg`: the same synthetic plasma fractal with an EXIF
  orientation tag; not a portrait or any other photograph. No test reads it;
  `core/tests/common/metadata_fixtures.rs`'s EXIF writer was checked against it.
- `layered.psd`: a layered PSD written by ImageMagick (`core/tests/psd_locks.rs`,
  `core/tests/psd_tests.rs`).
- `gradient.psd`: generated (`core/tests/psd_tests.rs`).
- `gradient.png`, `shapes-alpha.png`: generated; read by nothing in the tree.

The tree records no generator command for the generated files.
