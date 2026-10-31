# Phase 8D RAW workflow qualification

Run from the repository root after `make app-debug`:

```sh
python3 -B scripts/verify_phase8d.py --keep --report /tmp/rz-phase8d-workflow.json
```

`--host-only` exercises shared product code and actual AppKit Develop windows.
`--mcp-only` drives the app under test (the Debug build; `DR_VERIFY_APP` selects
another) through its external MCP server. The
runner generates small DNG fixtures with independently framed, different camera
white-balance defaults. It uses private preference-library folders (every
`scripts/host-source-probe/support.py` `PRESET_DIRECTORIES` entry), Actions and
recovery folders, dummy API credentials, a scratch MCP socket per instance and owned
processes. Normal macOS graphics-service access is required; a sandbox-denied
decode is a failure.

The host checks strict bounded preset storage, explicit control copying,
target-local As Shot white balance, multi-layer/document atomic preparation,
stale source rejection, no-op history, stable recorded settings, native full-source
histograms and exact detail, decoder migration/refusal/Undo/Actions, and actual
Develop Fit/100%, pan, comparison, clipping, output-space controls and cancellation.
The external app checks the same public tools, persisted presets, both document
depths, reviewed migration pixels and exact descriptors, and fresh-process native
reopen. The host saves captures of its own content views for layout review.

Every report records its temporary artifact directory. Host compilation uses an
immutable source/header snapshot and private static archive. Source manifests also
include Cargo manifests, build configuration, fixtures and Python helpers. Final
qualification must use the final app/archive and unchanged source inputs. After a
Python-only change, `--reuse-host /path/to/prior/scratch` can rerun an existing
binary only when its entire compiled Swift/header file set and archive match the
current checkout. Any Swift/header/archive change requires a new host build.

The separate real-camera gate uses the CC0 Nikon D70s NEF fixture documented in
[`../raw-probe/README.md`](../raw-probe/README.md), and an independently compiled
Core Image reference to check normalized Display P3 master samples. It also checks
explicit output-space round trips, native reopening and typed PNG output:

```sh
python3 -B scripts/verify_phase8d_color.py --help
```

One malformed, oversized or unknown-version preset file is listed as a diagnosed
`broken` row beside the good presets. The manager's Delete File or Repair clears
it (over MCP, `delete_raw_preset {file}` or `repair_raw_preset`), and a
future-schema file is never rewritten. Only a library-level fault makes
`entries()` throw: the folder cannot be enumerated, it holds more than 256
entries or more than 128 `.json` files, or the lock is busy. Develop then shows
“Presets unavailable” while editing stays available. Presets are separate
preferences, while recorded Actions retain resolved settings independently of
later preset replacement or deletion. Synchronization publishes only after all
targets are prepared; Undo remains independent for each changed document.
