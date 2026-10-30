# Phase 2 AppKit regression probe

Run from the repository root on Apple Silicon macOS:

```sh
make rust project
python3 scripts/selection-history-probe/verify.py
```

The runner compiles the current app sources with a dedicated probe entry point,
release Rust static library and Swift development settings. It creates a private
preferences domain, dummy API credential, temporary Actions directory, build
products and logs. It opens owned editor windows and calls the real model,
controller callbacks and Actions/MCP dispatch without starting a network server.
Scratch output is retained on failure or with `--keep`.

Coverage includes:

- Selection replace/feather/deselect/Reselect and exact undo/redo coverage, without
  incrementing file-dirty state.
- Crop and same-size rotation snapshots restoring corresponding canvas selection,
  and live document edits retaining unchanged coverage.
- Quick Mask mode and per-stroke undo, independent integer source-over reference,
  raw preview versus canonical RLE commit, real UI callback recording and Actions
  replay, with one clean undo for a selection-only Action.
- Feather-before-combine parity for a recorded additive selection.
- Supported raster/mask/color-plane/channel filter targets, independent weighted
  pixel references, matching preview and commit, stale capture refusal predicates,
  scalar fill/gradient coverage, locks, and translated group-mask storage.

The probe checks shared UI commit paths and actual AppKit document/undo behavior.
It does not synthesize tablet pressure or mouse-event timing; canonical Quick Mask
replay is checked using the resulting premultiplied overlay those events produce.
`scripts/verify_phase2.py` separately drives the built app over HTTP MCP.
