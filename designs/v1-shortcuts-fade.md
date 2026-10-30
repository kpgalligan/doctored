# v1 shortcuts and Edit ▸ Fade (ADR)

- **Status:** Accepted, 2026-10-13.
- **Supersedes:** Phase 12C's "⌘N stays New from Clipboard" and every "a shortcut already in the user's fingers is
  never renegotiated" comment ([phase-12c-workspace-status.md](phase-12c-workspace-status.md)).

## Shortcuts

### Decisions in force

- **The rule:** the audit below is the default set: an item takes its New chord, and a row marked "same" keeps its
  old one.
- **The defaults are the menu builders' rows.** There is no central table: `AppDelegate.buildMainMenu()` is what
  the build ships and `ShortcutMenu.snapshotDefaults` snapshots it; the rows were changed in place.
- **Overrides always win** (`ShortcutMenu.apply`). A stored override (`DR_SHORTCUTS_DIR`, the Shortcuts sheet) is
  applied unchanged. A DEFAULT chord — a customizable item's snapshot default, or the builder chord of any other item
  (the app menu, the system selectors, hidden twins), remembered per `NSMenuItem` on first sight — that an override
  assigns to a different identity is withheld (the item gets no chord) and comes back on a later `apply` once no
  override holds it. So a user who had bound ⌘E to Export keeps it, and Merge Down shows no chord until they drop it.
- **Window ▸ Minimize is `minimizeWindow:`**, which sends `performMiniaturize:` to the key window. macOS 26 forces
  every item whose action is `performMiniaturize:` to a bare ⌘ mask (measured: ⌃⌘M, ⇧⌘M, ⌥⌘M and ⌃M all read back
  ⌘M; `miniaturize:` and other actions keep theirs), which would have put Minimize on Curves…'s ⌘M. The new selector
  takes `performMiniaturize:`'s place in `ShortcutMenu.systemSelectors`.
- **Tool letters:** Paint Bucket moved from K to G, sharing Gradient's key and cycling with it in rail order.
- **The regression test** is the jobs-probe case "default shortcuts" (`scripts/jobs-probe/shortcut_defaults.swift`):
  the whole menu's identity → chord table below, no chord held twice, the tool letters, and the withhold rule.

### Audit (every chorded item, and every item that lost one)

| Item | Old | New |
|---|---|---|
| Doctored ▸ Settings… | ⌘, | ⌘, + hidden ⌘K |
| Doctored ▸ Hide Doctored / Hide Others / Quit | ⌘H / ⌥⌘H / ⌘Q | same |
| File ▸ New… | ⌥⌘N | ⌘N |
| File ▸ New from Clipboard | ⌘N | ⌥⌘N |
| File ▸ Open… / Close / Save / Save As… | ⌘O / ⌘W / ⌘S / ⇧⌘S | same |
| File ▸ Automate ▸ Actions… | ⌃⌘T | ⌥F9 |
| File ▸ Revert to Saved | — | F12 |
| File ▸ Export… | ⌘E | ⌥⇧⌘W |
| File ▸ Quick Export | ⌥⌘E | same |
| File ▸ File Info… | ⌥⇧⌘I | same |
| File ▸ Page Setup… / Print… | ⇧⌘P / ⌘P | same |
| Edit ▸ Undo / Redo | ⌘Z / ⇧⌘Z | same |
| Edit ▸ Fade… | new | ⇧⌘F |
| Edit ▸ Cut / Copy / Copy Merged / Paste | ⌘X / ⌘C / ⇧⌘C / ⌘V | same |
| Edit ▸ Paste Special ▸ Paste in Place / Paste Into | ⇧⌘V / ⌥⇧⌘V | same |
| Edit ▸ Clear | ⌫ | same |
| Edit ▸ Fill… (+ hidden ⇧⌫) | ⇧F5 | same |
| Edit ▸ the four colour fills | ⌥⌫ / ⌘⌫ / ⌥⇧⌫ / ⇧⌘⌫ | same |
| Edit ▸ Color Settings… (#547, after this audit) | — | ⇧⌘K |
| Image ▸ Rotate 90° CW / CCW | ⌘R / ⇧⌘R | — / — |
| Image ▸ Crop | ⌘K | — |
| Image ▸ Image Size… / Canvas Size… | ⌥⌘I / ⌥⌘C | same |
| Image ▸ Adjustments ▸ Brightness/Contrast/Saturation… | ⌥⌘A | same |
| Image ▸ Adjustments ▸ Levels… | ⌘L | same |
| Image ▸ Adjustments ▸ Curves… | — | ⌘M |
| Image ▸ Adjustments ▸ Hue/Saturation… / Color Balance… | — / — | ⌘U / ⌘B |
| Image ▸ Adjustments ▸ Black & White… | — | ⌥⇧⌘B |
| Image ▸ Adjustments ▸ Invert / Desaturate | ⌘I / ⇧⌘U | same |
| Image ▸ Auto Tone / Auto Contrast / Auto Color | ⇧⌘L / ⌥⇧⌘L / ⇧⌘B | same |
| Layer ▸ New Layer / Group / Ungroup | ⇧⌘N / ⌘G / ⇧⌘G | same |
| Layer ▸ Layer Via Copy / Via Cut | ⌘J / ⇧⌘J | same |
| Layer ▸ Free Transform / with copy / Again / Again with copy | ⌘T / ⌥⌘T / ⇧⌘T / ⌥⇧⌘T | same |
| Layer ▸ Create Clipping Mask | ⌥⌘G | same |
| Layer ▸ Merge Down | ⇧⌘E | ⌘E |
| Layer ▸ Merge Visible | — | ⇧⌘E |
| Layer ▸ Stamp Visible | ⌥⇧⌘E | same |
| Layer ▸ Arrange (Front / Forward / Backward / Back) | ⇧⌘] / ⌘] / ⌘[ / ⇧⌘[ | same |
| Layer ▸ Lock ▸ Lock All | ⌘/ | same |
| Select ▸ All / Deselect / Reselect / Inverse | ⌘A / ⌘D / ⇧⌘D / ⇧⌘I | same |
| Select ▸ Select and Mask… | — | ⌥⌘R |
| Select ▸ Modify ▸ Feather… | — | ⇧F6 |
| Filters ▸ Repeat Last Filter | ⌃F | same |
| Filters ▸ Liquify… / Develop Filter… | ⇧⌘X / ⇧⌘A | same |
| Filters ▸ Lens Correction… | — | ⇧⌘R |
| Tools ▸ Command Palette… | ⇧⌘K | ⌘F |
| View ▸ Zoom In (+ hidden ⌘=) / Zoom Out | ⌘+ / ⌘- | same |
| View ▸ Actual Size | ⌘0 | ⌘1 + hidden ⌥⌘0 |
| View ▸ Zoom to Fit | ⌘9 | ⌘0 |
| View ▸ Rotate View… | ⌥⌘R | — |
| View ▸ Reset View Rotation | ⌃⌥⌘R | same |
| View ▸ Proof Colors / Gamut Warning | ⌘Y / ⇧⌘Y | same |
| View ▸ Show Clipping | ⌥⌘J | same |
| View ▸ Hide Layers | ⌥⌘L | F7 |
| View ▸ Channels / Assistant / Navigator | ⌃⌘C / ⌃⌘A / ⌃⌘N | same |
| View ▸ Info | ⌃⌘I | F8 |
| View ▸ Rulers | ⌃⌘R | ⌘R |
| View ▸ Show Guides / Lock Guides / Show Grid / Snap | ⌘; / ⌥⌘; / ⌘' / ⇧⌘; | same |
| Window ▸ Minimize | ⌘M | ⌃⌘M |
| Tool letter: Paint Bucket | K | G |

Kept on purpose: ⌘H, ⌘, (macOS's own); ⌥⌘A Brightness/Contrast/Saturation; Quick Export ⌥⌘E, Page Setup ⇧⌘P,
Show Clipping ⌥⌘J, Channels ⌃⌘C, Assistant ⌃⌘A, Navigator ⌃⌘N, Reset View Rotation ⌃⌥⌘R; the focus tools' R; ⌥⇧⌘K
left free (the shortcut probe's test chord).

## Edit ▸ Fade

### Decisions in force

- **What fades.** The last edit, when its one recorded step (or the hint an agent call, a job or a stroke leaves,
  `ImageDocument.fadeHint`) is in `FadeStep.regionTools` (apply_filter, auto_tone, auto_contrast, auto_color, liquify,
  fill_selection, gradient) or `FadeStep.strokeTools` (stroke_selection, fill and the paint and retouch strokes) with
  `target` absent or "layer". `applyEdit` and `endLiveEdit` note the candidate after registering their history.
- **Unavailable after anything else** (`currentFadeCandidate`): the document must still be the step's result, with
  the same selection, layer selection, the layer as paint target and no Quick Mask; undo, redo or a History jump
  clears it. A step that changed the layer count (a stroke onto a new layer) is not fadeable.
- **The blend** (`core/src/fade.rs`, a kernel over `blend.rs`): where both sides are transparent the pixel stays;
  E is the result for Normal, else the result composited over the state before it in the mode
  (`composite_source_into`, the float variants by `blend_kind_at`) — the state before where the result is
  transparent; E′ is the result moved toward E by the step's weight; the output is the state before moved toward E′
  by Opacity (both `gate_source_into`). Normal at 100 % is the result, 0 % the state before.
- **The weight:** a region step's selection coverage (1 with none); a stroke's changed pixels (its Outside/Center
  band paints where the selection is 0, so `stroke_selection` is a stroke; the Paint Bucket's flood reaches only part
  of the selection, so `fill` is one too).
- **One step, replacing the faded one** (`commitFade`): the step is undone and "Fade <step>" applied over the state
  before it, so History keeps its entry count. In an Action replay (history suspended) the fade applies over the
  result as an edit of its own; the replay's one step covers both.
- **8, 16 and 32 bits:** `dr_doc_fade` (byte), `dr_precision_doc_fade` (u16 rounded as the History Brush, f32
  unclamped to ±65504). A 32-bit document lists and admits only `float_document::admitted_blend`'s modes. The typed
  plan is the History Brush's measured term over the same walk; no new memory plan.
- **Out of scope:** Fade after Patch, Red Eye, Remove, Content-Aware Move/Fill or Apply Image, and on mask, plane,
  channel or Quick Mask targets.

### Verification

`make t F=fade` (W3C oracle, transparent pixels at every depth, refusals, null safety); jobs-probe "fade sheet"
and "curves sheet"; `scripts/verify_v1_fade.py` (MCP at 8/16/32 bits, refusals, undo, Actions replay).
