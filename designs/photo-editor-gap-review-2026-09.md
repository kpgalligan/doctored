# Professional photo editor gap review, September 2026 (ADR)

- **Status:** Accepted — live inventory (rows re-verified 2026-10-30). Its recommendations became the thirteen phases of the [implementation plan](professional-photo-editor-implementation-plan.md); the [archived plan](../archive/professional-photo-editor-implementation-plan.md) cites the finding IDs F01–F16 below; a row whose work is open links its issue.
- **Phase / dates:** reviewed 9 September 2026 at commit `3db6609`
- **Original:** [archive/photo-editor-gap-review-2026-09.md](../archive/photo-editor-gap-review-2026-09.md) — probes, evidence tables, per-item reconciliation, footnoted sources
- **Key commits:** `3db6609` (reviewed tree)

## Context
By September the [first gap review](photo-editor-gap-review.md) had shipped its phases 1–8. Doctored had layers,
groups, masks, channels, styles, adjustments, healing, RAW import and Actions, but all of it ran on 8-bit storage.
Its §4 had concluded that "everything else is breadth". This review rejected that. For a professional photographer,
the remaining gaps were preserving work (saves, recovery, history), preserving image information (bit depth, sources,
file exchange) and responsiveness on large documents. It also found concrete correctness defects, confirmed with
probes: a save the reader refused to reopen, loss when saving flat, dark fringes on alpha edges, and TIFF round trips.

## Decisions
- **Priorities are product priorities, not size estimates.** P0 protects saved work or correct output. P1 closes a frequent professional blocker. P2 is efficiency or breadth. P3 is specialist. The old S/M/L sizes are historical sketches.
- **Fix correctness before adding breadth** (Stage A). The native writer must enforce every reader cap, including the 400 MP total-layer-pixel sum. Flat-save protection must compare document state against what the format can hold, not check `layerCount > 1`. Resize and blur must filter with premultiplied alpha. A group-budget overrun must be a render error, never a silently omitted group.
- **Promote high bit depth and retained sources from "deferred" to foundations.** An f32 compositor cannot recover precision lost at import. RAW and placed images need a retained source plus a versioned recipe. A backdrop filter layer and a source-backed layer are different concepts, and each gets its own model scope. (Placed-image and host retained sources were removed for v1 on 2026-10-10 ([removal ADR](convert-to-smart-object-status.md)); only a camera RAW layer keeps its original.)
- **Linear-light blending is an explicit, persisted evaluation choice**, not the old single toggle. Existing files must keep rendering as they did. (It shipped this way in 11E.)
- **Keep what works and extend it.** Implemented features (Actions, Batch, crop keeping off-canvas pixels, Free Transform, colour management, Load/Save Selection) are not to be re-built under new names. Crop already keeps layer pixels, so no second "nondestructive crop" is needed.
- **A command must affect the indicated target and region, or refuse.** This covers mask targets, selections and planes, and preview and commit must agree (became Phase 2).
- **Metadata is preserved per container and packet**, never as a blanket "keep metadata" claim. Web delivery must distinguish converting to sRGB from merely not embedding a profile.
- **PSD fidelity is an explicit contract.** "Opened without error" does not mean "preserved". A per-file loss report and a flattened fallback are always available. Private XMP payloads would not make Photoshop-editable text.
- **Action replay fidelity comes before authoring power.** Geometry remapping stays an explicit choice. Colours need an explicit colour-space argument.
- **Offline core editing is non-negotiable.** The assistant and MCP are opt-in, and the editor must be fully usable without a provider key.
- **Specialist workflows are chosen, not owed.** Prepress, animation, illustration and generative editing each need an explicit market decision. Multi-image photography waits for high-depth and scaling foundations.
- **Acceptance is by scenario, not unit count** (the original's §7 lists twelve scenarios). Perceptual quality (masks, retouch, tablet feel, accessibility) needs human evaluation. A passing unit suite cannot certify it.

## Rejected alternatives
- Raising undo from 24 to another fixed count. History needs a memory budget: it shipped as `levelsOfUndo = 0` plus a retention budget.
- A float preview buffer as "high-bit-depth support". It is not document precision.
- Equating subject-instance clicking with Quick Selection, luminosity channels with Color Range, or manual Align with photographic registration. Each is a different capability.

## Consequences
The plan's phases 1–13 are this review, and it treats the review as the complete feature inventory. That inventory
is now this ADR's finding table together with the gap catalog in [photo-editor-gap-review.md](photo-editor-gap-review.md).
New work must preserve the rendering semantics of existing native files (`.doctored`, and the legacy `.rz`). It
must also make every limit and loss explicit rather than silent, and ship UI, MCP and Actions parity together.

## Findings and added gaps (verified 2026-10-30)
Findings (IDs the archived plan cites). A row whose work is open links its issue; a gap marked "not built" with no
issue is inventory, with no owner yet.

| ID | Pri | Finding | Status |
|---|---|---|---|
| F01 | P0 | Writer accepts a `.rz` the reader refuses (401 MP probe) | DONE — plan §1A; `core/src/drdc_capacity.rs` |
| F02 | P0 | Flat save ignores single-layer native-only state | DONE — plan §1B; `ImageDocument+Preservation.swift` |
| F03 | P0 | No verified autosave/recovery; 24-level undo | DONE — plan §5A–5D; `RecoveryCoordinator.swift`, History…, Snapshots… |
| F04 | P0 | Straight-alpha resize/blur darkens edges | DONE — plan §1D; `ops::resize` → `sampling::resize` |
| F05 | P0/P1 | TIFF drops ICC/PPI on reopen; PSD import lossy without notice | DONE — plan §3A, §3D, §10A |
| F06 | P1 | 8-bit storage; uncalibrated Exposure/White Balance | DONE — Phase 7, 11F, plan §3C; the soft-proof ΔE miss OPEN → [#16](https://github.com/kpgalligan/doctored/issues/16) |
| F07 | P1 | RAW/placed sources not retained | DONE for camera RAW (8C, P7-02); placed-image and host retained sources (P7-01, P7-03, P7-35) were built and REMOVED 2026-10-10 ([removal ADR](convert-to-smart-object-status.md)) |
| F08 | P1 | Selection undo, Reselect, Color Range, refinement, independent masks, mask targeting | DONE — Phases 2, 6 and 9I; the matting quality gate stopped being a ship condition on 2026-10-10 ([selection and mask ADR](phase-6-selection-mask-status.md)) |
| F09 | P1 | Spatial filters; selection-scoped destructive edits | DONE for USM, High Pass, denoise, median, D&S and scoping (9B/9C/9F, 2B); remaining filters listed in the Filters rows of [photo-editor-gap-review.md](photo-editor-gap-review.md) |
| F10 | P0/P1 | Silent group omission; blocking heal/fill; memory | DONE — plan §1C, Phase 4; large-document scale OPEN → [#9](https://github.com/kpgalligan/doctored/issues/9), [#10](https://github.com/kpgalligan/doctored/issues/10), [#11](https://github.com/kpgalligan/doctored/issues/11) |
| F11 | P1 | Retouch sampling policy, CAF source exclusion/output | DONE — 9D/9E (P7-15–P7-18); retouch brushes DONE; frequency separation OPEN → [#17](https://github.com/kpgalligan/doctored/issues/17) |
| F12 | P1/P2 | Export workspace, PSD export, metadata, formats, proofing | DONE — 10B/10C, 11A–11D; linear light closed in 11E (2026-11-12) |
| F13 | P1/P2 | Replay gaps, pressure/timing, assets, relative geometry, colour-space args | DONE — 2D, 9H, 12A (`color_space`), 12B |
| F14 | P2 | History, snapshots, labels/search, comps, New Document, Navigator, view rotation, second view, shortcuts, workspaces | DONE — 5C/5D, 12C, 12D |
| F15 | P2 | Gradients, patterns, brushes, text runs, paths, styles, geometry | Partly DONE — see the lines below and the Paint, Styles, Geometry and Deferred rows of the older ADR's catalog; open → [#18](https://github.com/kpgalligan/doctored/issues/18), [#19](https://github.com/kpgalligan/doctored/issues/19) |
| F16 | P2/P3 | Specialist and multi-image workflows | Partly DONE — see the lines below; open → [#1](https://github.com/kpgalligan/doctored/issues/1) |

Gaps this review added beyond the first catalog:

- `DONE` — Registration/Auto-Align, focus stack, panorama, HDR merge/deghost and multi-frame denoise ([merge and stacks ADR](v1-stacks-status.md)). HDR Toning's built-in preset set is [#7](https://github.com/kpgalligan/doctored/issues/7) and its `.hdt` import [#8](https://github.com/kpgalligan/doctored/issues/8).
- `DONE` — Lab Color mode ([ADR](lab-mode.md); 8-bit Select Similar/Grow → [#5](https://github.com/kpgalligan/doctored/issues/5)), spot channels ([ADR](spot-channels.md); open items under [#1](https://github.com/kpgalligan/doctored/issues/1)) and optional generative editing through cloud providers (Edit ▸ Generative Fill…, Generative Expand…; [neural models](neural-models.md)); local models → [#21](https://github.com/kpgalligan/doctored/issues/21).
- `OPEN` [#1](https://github.com/kpgalligan/doctored/issues/1) — CMYK document mode shipped ([status](cmyk-mode-status.md)) and separations come from its CMYK and spot-channel output; PDF/X and CMYK's remaining items are open.
- Artboards — not built (a design-tool workflow that needs an explicit choice); templates are covered by New Document presets.
- `DONE` — Signed and notarized distribution: `.github/workflows/publish.yml` signs the DMG with Developer ID and notarizes it (`project.yml`'s `CODE_SIGN_IDENTITY: "-"` is the local build).
- In-app update checking and rollback, practical user guides, and keyboard, VoiceOver and tablet validation passes — not built: there is no updater, the public docs have a landing page and references but no task guides, and tablet pressure and timing ship and are tested.
- `DONE` — Pen paths, the Paths panel and vector masks ([ADR](v1-paths.md)); attributed text colours (8E, P7-05).
- `DONE` — Replacing a text layer's missing font family ([ADR](replace-missing-fonts.md)).
- `OPEN` [#19](https://github.com/kpgalligan/doctored/issues/19) — Text on a path.
- Attributed text runs with a per-run font, size and style — not built (runs carry colour only).
- `DONE` — Styles and Blend If on byte (8-bit RGB) adjustment layers (`style_adjustment.rs`), gradient glows (`style_glow_gradient.rs`), repeated effects (`style_instances.rs`) and style presets (`StylePreset.swift`, Window ▸ Styles).
- `DONE` — Float editing widened to a full editing depth ([ADR](v1-float-editing.md)); F16 TIFF and EXR export, OpenEXR and Radiance HDR; EDR display through View ▸ HDR Preview ([ADR](v1x-hdr-photos.md)). Luminance tone maps are not built; ACES-style tone maps `DROPPED` (2026-10-30) — the clip and Reinhard maps and HDR Toning cover SDR delivery. Float editing's open items: [#14](https://github.com/kpgalligan/doctored/issues/14) (float preview and plane edits) and [#4](https://github.com/kpgalligan/doctored/issues/4) (Develop at 32-bit float).
- `DONE` — Rendering intents, BPC, soft proof, gamut warning (11C; `icc_intent.rs`, `EditorViewController+SoftProof.swift`).
- `DONE` — Metadata editing and presets (11B metadata policies). Local RAW masks, lens controls (8G). Develop 100% view, presets, sync (8D).
- `REMOVED` — Live Photo portability via retained host frame sources (8F, P7-03). Removed for v1 on 2026-10-10 with retained host sources ([removal ADR](convert-to-smart-object-status.md)): a Live Photo now places its still as an ordinary raster layer, and an old file's `live_photo` layer opens as its pixels.
- `CLOSED` — MCP client authorization and document/file access scoping. [agent-socket.md](agent-socket.md) records the transport (a Unix socket in a `0700` folder, no TCP) and why client authorization was declined on 2026-10-28. C-lite is done: `../app/Sources/AgentPathGuard.swift` refuses, before any MCP or Assistant tool runs, a file argument that resolves into a stated list of sensitive locations, and writes into an `.app` or `/Applications`. C-full (folder grants) and D (per-client approval) are not planned.
- `DONE` — PSB read and write (`psd_container.rs`, `psd_framing.rs`; File ▸ Export as PSD… and MCP `export_psd` write a PSB container) for RGB at 8, 16 and 32 bits and for CMYK, Lab, Grayscale, Indexed Color, Duotone and Bitmap documents.
- `DONE` — Persistent colour sampler points: the Color Sampler tool, the Info panel's Color Samplers section and MCP `get_color_samplers` / `set_color_samplers` (`ColorSamplers.swift`, `ImageCanvasView+ColorSamplers.swift`, `EditorViewController+ColorSamplers.swift`, `ColorSamplerReadoutView.swift`, `AgentServer+ColorSamplers.swift`). `DONE`: the samplers are core document state at both depths, saved in the native file (words 59/60, `CSMP`) and PSD image resource 1073 at 8 and 16 bits, and moved by every canvas-geometry op (off-canvas samplers dropped) — `doc_sampler.rs`, `sampler_native.rs`, `psd_samplers.rs`. Photoshop 2025 writes 1073 at **version 3** (14-byte records, vertical position first, the pixel index; `tests/fixtures/psd_photoshop/color_samplers/`); Doctored writes version 2, which Photoshop reads back at the same pixels.
- `DONE` (Part A) — ⌥-drag clipping display: Levels Black/White (dialog and adjustment-layer pane) and a Curves end point show a threshold view of the edited composite, exact native 0/65535 at 16 bits, beside a live "After" histogram (`../core/src/clip_display.rs`, `../core/src/ffi_clip_display.rs`, `ClippingDisplay.swift`, `LevelsAdjustmentEditor.swift`, `CurvesAdjustmentSheetController.swift`, `CurveEditor.swift`, `PrecisionAdjustmentSession.swift`). `DONE` Part B — View ▸ Show Clipping (⌥⌘J), a persistent canvas overlay at 8 and 16 bits: blue where a channel sits at 0, red at the maximum (exactly 65535 at 16 bits), magenta for both, over a dialog's preview while adjusting and over the document otherwise; an app-wide view preference with no MCP twin, off in a 32-bit float document (clip-display mode 3, `EditorViewController+ClippingOverlay.swift`, `ImageCanvasView+ClippingOverlay.swift`).
