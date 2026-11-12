# Save preservation probe

This AppKit regression probe calls the real `ImageDocument` save methods. It caught an existing-destination deletion when AppKit's default `writeSafely` received a Save As refusal from the lower write override. The staged writer must preserve that destination.

Run on an Apple Silicon Mac with Xcode and Rust installed:

```sh
make rust project
python3 scripts/save-preservation-probe/verify.py
```

`make rust project` supplies the current Rust release library and generated document-type registrations; it does not build or launch the app. The runner compiles all app Swift sources except their `main.swift`, linking this probe and a tiny Rust fixture. It does not replace the application binary or change application/core sources.

Fixtures, logs and the copy of the probe binary that runs go into a fresh temporary directory. The linked probe is built through `scripts/probe_build.py` and cached under `build/probe-cache`, with the Clang module cache in `build/probe-module-cache`, so a second run on unchanged sources compiles nothing. Successful runs remove the temporary directory; failures retain it and print its location. Pass `--keep` to retain a successful run. Run outside a restricted execution sandbox: AppKit file coordination can fail before it reaches the writing logic under test.

The process uses a private preferences domain, dummy API key, private Actions directory and argv defaults. It never starts `AppDelegate`, opens the user's documents, or signals other application instances.

The checks cover:

- Rich single-layer PNG refusal through `writeSafely` and save completion for Save, Save As and autosave elsewhere, preserving destination bytes and dirty state.
- Native save/reopen preserving descriptions, and native Save As establishing the master and clearing dirty state.
- Save To and explicit export preserving source bytes, URL, model and dirty state.
- Delegated Close → Save routing to native-leading Save As while retaining its delegate, selector and context.
- Oversized native and failed-render output preserving existing destinations; errors name the selected destination.
- Preserved preview `CGImage` identity on render failure, unavailable current projection, and undo clearing the failure.
- Replacement retaining POSIX permissions and creation date.
- A 32-bit float document (`core/tests/fixtures/float_11f/scene.tiff`, passed as `DR_SAVE_PROBE_FLOAT_FIXTURE`) whose remembered export depth is 32: a flat PNG Save To writes 16-bit RGBA, a JPEG Save To refuses with the explicit 8-bit sentence, and the remembered depth and dirty state are unchanged.
- A one-layer Grayscale PNG (written here through ImageIO and opened with `ImageDocument(contentsOf:ofType:)`) given a guide: `writeSafely` for Save refuses naming the guides and leaves the file and dirty flag alone, and Save routes to the native-leading Save As panel. `scripts/verify_mode_save_losses.py` covers every state and mode over MCP.
- A little-endian 8-bit TIFF whose layer block holds a record the core does not read (`scripts/layer_block_tiff.py`, written by `verify.py` and passed as `DR_SAVE_PROBE_LAYER_TIFF`): it opens flat, TIFF leaves Save and Save As, Save refuses and leaves the file; a Save a Copy and a Save As that fails keep that; a Save As to `.doctored` that succeeds ends it, and a TIFF then written by Save As saves in place.

`oversized.rs` creates an unsupported 1025-layer fixture by sharing one pixel buffer; its mutation is confined to this throwaway process. `main.swift` temporarily intercepts the save-panel entry point so delegated routing can be checked without opening a panel, then restores it. The one-pixel rendering budget exercises refusal with a two-pixel group.

This complements `scripts/verify_phase1.py`, which exercises the built application through MCP, and the independent numeric/capacity tests in `core/tests`.
