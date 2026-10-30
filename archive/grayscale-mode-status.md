# Grayscale document mode (#1): status

- **Status:** Open — phases 1a (the plan, the model, New, open, Image ▸ Mode both ways, the view, `.rz`),
  1b (layer structure, Fill, the one-channel saves and exports), 1c (the panels, `get_precision`,
  `render`'s gray channel, Export As), phase 2's Photoshop take, 2a-ii (painting), 2b (adjustments), 2c
  (filters, Fade, Actions, retouch), 2d (layer effects, Blend If, alpha-channel targets), 3a (gray PSD,
  PSB and layered TIFF open), 3b's import fix, 3b-ii (readback fix; the gray writer in core), 3b (the
  Grayscale PSD export, after take4 PASS), 3c (32-bit gray files), 2e (the rest of phase 2), 4
  (geometry, selections, the clipboard), 5a (profiles, Bits/Channel, 32-bit HDR Toning), 5b (proof, Gray → CMYK)
  and 6 (6a-i: type, shape and fill layers, paths, vector masks; 6a-ii: comps, links, snapshots, the legacy ops,
  the artwork carry; 6b: the analysis, multi-image, neural and generative tools) shipped; the closing batch next
- **Issue:** [#1](https://github.com/kpgalligan/doctored/issues/1) (the colour-mode umbrella; CMYK is
  `designs/cmyk-mode-status.md`)
- **Code:** `core/src/gray_document.rs`, `gray_adjust.rs`, `gray_filter.rs`, `gray_style.rs`, `gray_io.rs`, `gray_convert.rs`, `gray_native.rs`, `psd_import_gray.rs`, `psd_import_gray32.rs`, `psd_export_gray.rs`, `psd_export_gray32.rs`,
  `ffi_gray.rs`, `gray_encode.rs`, `gray_export.rs`, `gray_working.rs` (sGray), `icc_builtin.rs` (`gray_blob`,
  `twin_blob`), `gray_clipboard.rs`, `ffi_gray_clipboard.rs`, `gray_profile.rs`, `gray_artwork.rs`, `gray_comps.rs`;
  `app/Sources/GrayDocument.swift`, `GrayAdjust.swift`, `GrayClipboard.swift`, `GrayProfileEdit.swift`,
  `GrayArtwork.swift`, `GrayProfileSheetController.swift`,
  `AgentServer+GrayColour.swift`, `RasterCore+Gray.swift`, `AgentCatalog+Gray.swift`,
  `EditorViewController+GrayMode.swift`, `EditorViewController+CmykMode.swift`'s and `NativeLayerBlending.swift`'s
  gray arms
- **Template:** CMYK document mode, phase 1 (`ad8db093`): the same seams, gate shape and phase numbering
- **Next mode:** Lab Color, with its own status doc when it starts.

## Phases

### The closing batch (contract)

Phase 6 (6a-i, 6a-ii and 6b) shipped; see the companion. Grayscale's feature work is complete. What remains is one
small batch:

- condense this doc and the companion into the ADR `designs/grayscale-mode.md` (decisions in force, what shipped with
  commits, the measured Photoshop facts), `git mv` both into `archive/`, add their `archive/INDEX.md` lines and update
  every link to the old paths;
- decide the document tools no phase names, which still refuse "until #1 phase 6" (`set_resolution`,
  `get_metadata`, `distort_layer`, `arrange_layer`, `auto_select_layer`, `export_layers`, `contact_sheet*`,
  `set_history_budget`, `recover_document`, `discard_recovery`, `get_mask_properties`, `place_image`, the image-source
  and RAW tools, `export_pdf`, `get/set_evaluation`, `find_layers`, the view, workspace and second-view tools,
  `get/set_color_samplers`, `detect_horizon`, `get/set_file_info`, `set_ruler_origin`, `separate_color`,
  `save_proof_preset`, `get/set_tone_map`), and RGB → Gray's Generative Layer blocker
  (`gray_convert::first_gray_blocker` still names one "cannot hold yet", though 6b's Gray document holds one);
- file an `ai-drafted` issue for whatever Grayscale still lacks, linked from the ADR.

## Open items

- None outside the phase contracts above.

## Shipped runs

[grayscale-mode-shipped.md](grayscale-mode-shipped.md) — the decisions in force and phases 1a, 1b, 1c, phase 2's take, 2a-ii, 2b, 2c, 2d, 3a,
3b and their takes, 3c, 2e, 4, 5a, 5b, 6a-i, 6a-ii and 6b.
