# Screens

The documentation's screenshots, one Markdown file per screen. Starlight never routes this folder;
`scripts/screens-probe/run.py` captures the screens in the light and dark looks, and
`scripts/import-screens.mjs` turns its output into `src/assets/screens/`, which the `Shot`
component shows.

## Format

A screen file opens with a ```` ```json screen ```` line, one JSON object and a closing ```` ``` ````
line; the prose after it is the capture's alt text (with more than one capture, each capture's alt
is the section under a `## ` heading that names it).

- `id` is the file stem; `title` names the screen; `fixture` is an id in `fixtures.json`, the only
  pictures allowed, or `null` for a window that opens none; `window` is a window kind: `editor` (a
  fresh document of the fixture, 1280 × 800 pt), `editor_raw` (a camera file opened in the editor,
  developed at the default settings), `develop` (the Develop RAW card of a camera file) or `welcome`
  (the welcome window, no fixture). `needs` is optional: `["models"]` runs the screen
  with the fixtures store's models and skips it without them.
- `steps` run in order: `{"tool": <agent tool>, "args": {…}}` calls the MCP tool on the screen's
  document (the probe adds `document_id`); `{"ui": <action>, "args": {…}}` calls the app's own code.
  `{"ui": "run_modal_card", "args": {"card": <name>}}` opens a modal card from the editor; the steps
  after it run inside the card, and the captures are taken there before it is cancelled. Rects are
  `[x, y, w, h]` and points `[x, y]`, in canvas pixels.
- `captures` are `{"name", "of": "window"}`, `{"name", "of": "view", "view": <name>}` (cut from the
  editor window), `{"name", "of": "sheet"}` (the editor's sheet, cut from its window, which grows
  to hold a tall sheet), `{"name", "of": "panel", "panel": <name>}` (a separate window; the
  options bar's More panel, a child of the editor window, is cut from the editor window's image),
  `{"name", "of": "card"}` with an optional `"tab"` (the running card) or `{"name", "of": "alert"}`
  (the first alert the steps put up, answered Cancel).
- The names are the screens probe's, each one entry in a table, and the probe checks every one
  before it opens a window: window kinds, `cards` and `panels` in
  `scripts/screens-probe/windows.swift`; `sheetOpeners` (`open_sheet`) and `editorViews` in
  `editor_tables.swift`; the UI actions, their scope (editor, the open sheet or card, a card only,
  or anywhere) and their argument keys in `actions.swift`.
- Each capture becomes `<id>.<name>` in `src/assets/screens/screens.json` and `Shot`'s `name`.
- To try a screen before it is published, put its file in a folder outside the repository and run
  `run.py --screens <folder>`.

## Screens

| id | fixture | captures | shows |
| :--- | :--- | :--- | :--- |
| `editor-window` | `sky` | `window` | The editor with two layers and a rectangle selection |
| `layers-panel` | `sky` | `panel` | The Layers panel with two layers |
| `crop` | `sky` | `window` | The Crop tool with a box inside the image |
| `crop-past-edge` | `sky` | `window` | The Crop tool with a box past every edge |
| `levels` | `sky` | `sheet` | The Levels dialog |
| `develop` | `z6-nef` | `window` | Develop RAW on a Nikon Z 6 file |
| `welcome-window` | none | `window` | The welcome window, with nothing open |
| `info-panel` | `sky` | `panel` | The Info tab reading a pixel in the sun |
| `new-document-card` | `sky` | `card` | New Document with a saved CMYK preset chosen |
| `history-window` | `sky` | `history` | The History panel with an undone step |
| `curves` | `sky` | `sheet` | The Curves dialog with an S-curve |
| `tool-rail` | `sky` | `rail` | The tool rail with the Brush chosen and a group showing its last-used tool |
| `options-bar` | `sky` | `bar` | The options bar for the Gradient tool |
| `command-palette` | `sky` | `palette` | The Command Palette with levels typed |
| `navigator-zoom` | `sky` | `window`, `navigator`, `options` | The editor at 200%, the Navigator tab and the Zoom tool's bar |
| `color-samplers` | `sky` | `window` | Three color samplers on the photo and their Info readings |
| `show-clipping` | `sky` | `window` | Show Clipping on the backlit photo |
| `rulers-guides` | `sky` | `window` | Rulers and four guides on the thirds |
| `ruler-tool` | `sky` | `window`, `options` | The Ruler tool measuring a roof line, and its readouts |
| `mask-edit-overlay` | `sky` | `window`, `panel`, `status` | A layer mask as the edit target: the red wash, the target ring, the status bar |
| `image-size` | `sky` | `sheet` | The Image Size dialog at 300 ppi, with Resample on |
| `canvas-size` | `sky` | `sheet` | The Canvas Size dialog and its anchor grid |
| `crop-straighten` | `sky` | `window`, `options` | The Crop tool straightening by 4°, and its options bar |
| `snapshots-window` | `sky` | `snapshots` | The Snapshots panel with two session snapshots |
| `recover-unsaved-work` | `sky` | `recovery` | Recover Unsaved Work listing one recovery copy |
| `export-jpeg` | `sky` | `sheet` | The Export sheet with JPEG chosen |
| `export-16-bit-sharpening` | `sky` | `sheet` | The Export sheet on a 16-bit document, with output sharpening on |
| `contact-sheet` | `sky` | `sheet` | The Contact Sheet dialog for six layers |
| `psd-open-report` | `sky` | `notice` | The compatibility notice that opening a PSD puts up |
| `layers-panel-structure` | `sky` | `panel` | The Layers panel with an open group, a clipped adjustment, labels, a lock and links |
| `layers-panel-filtered` | `sky` | `panel` | The Layers panel filtered to the red label |
| `layers-select-and-align` | `sky` | `window`, `options` | Three shape layers selected, and the Move tool's align and distribute buttons |
| `gradient-fill-dialog` | `sky` | `sheet` | The Gradient Fill dialog reopened on a fill layer |
| `layer-comps-window` | `sky` | `comps` | The Layer Comps window with three comps |
| `mask-properties` | `sky` | `sheet`, `window` | Mask Properties at 90% density and a 60 px feather, and the feathered mask behind it |
| `layer-style-blending-options` | `sky` | `sheet` | Layer Style's Blending Options with a split Blend If handle |
| `layer-style-drop-shadow` | `sky` | `sheet` | Layer Style with two stacked Drop Shadows |
| `layer-style-bevel-emboss` | `sky` | `sheet` | Layer Style's Bevel & Emboss with Contour and Texture on |
| `styles-panel` | `sky` | `panel` | The Styles panel with the built-in styles and a saved group |
| `auto-align-sheet` | `sky` | `sheet` | The Auto-Align Layers dialog |
| `select-and-mask` | `sky` | `sheet` | Select and Mask on the sky, with Edge Radius 8 and Smart Radius on |
| `color-range` | `sky` | `sheet` | Color Range after sampling a blue gap between the clouds |
| `quick-mask` | `sky` | `window` | Quick Mask over a feathered ellipse, with the Brush chosen |
| `magic-wand` | `sky` | `window`, `bar` | One Magic Wand click selecting the rooftops, and the wand's options bar |
| `transform-selection` | `sky` | `sheet` | Transform Selection widening and turning an ellipse |
| `load-selection` | `sky` | `sheet` | Load Selection with a saved channel as its source |
| `paths-panel` | `sky` | `window`, `panel` | A smooth path around the sun under the Pen, and the Paths tab |
| `make-selection-from-path` | `sky` | `sheet` | The Make Selection dialog for a path |
| `channels-panel` | `sky` | `window`, `panel` | An alpha channel shown as a rubylith, and the Channels tab |
| `apply-image` | `sky` | `sheet` | Apply Image with Multiply blending |
| `free-transform` | `sky` | `window`, `bar` | Free Transform turning and scaling a text layer, and its options bar |
| `puppet-warp` | `sky` | `window`, `bar` | Puppet Warp bending a bar with three pins, and its options bar |
| `warp-dialog` | `sky` | `sheet` | The Warp dialog reopened on a stored warp |
| `adjustment-layers-dodge-burn` | `sky` | `panel`, `window` | Curves layers for dodge and burn with painted masks, and their result |
| `adjustment-live-preview-posterize` | `sky` | `window`, `sheet` | Posterize previewing on the canvas behind its dialog |
| `hue-saturation` | `sky` | `sheet` | The Hue/Saturation dialog |
| `color-balance` | `sky` | `sheet` | The Color Balance dialog on Midtones |
| `black-and-white` | `sky` | `sheet` | The Black & White dialog at its default weights |
| `channel-mixer` | `sky` | `sheet` | The Channel Mixer dialog on the Red output |
| `selective-color` | `sky` | `sheet` | The Selective Color dialog on Reds |
| `gradient-map` | `sky` | `sheet` | The Gradient Map dialog with the black-to-white ramp |
| `hdr-toning` | `sky` | `sheet` | The HDR Toning dialog on a 32-bit document |
| `filter-in-selection-radial-blur` | `sky` | `window`, `sheet` | Radial Blur confined to an elliptical selection |
| `tilt-shift` | `sky` | `window`, `sheet` | Tilt-Shift at 40 px, blurring the top of the sky |
| `lens-correction` | `sky` | `window`, `sheet` | Lens Correction and its alignment grid on the canvas |
| `liquify` | `sky` | `sheet` | Liquify after one Forward Warp stroke, with the mesh, at 100% |
| `develop-tone-split` | `z6-nef` | `window` | Develop RAW with Exposure, a custom white balance and the Split comparison |
| `develop-presence-grain` | `z6-nef` | `window` | Develop RAW at 100% with Texture, Clarity and Grain set |
| `raw-layer-editor` | `z6-nef` | `window`, `panel` | A camera file opened as a 16-bit document, and its camera RAW layer |
| `redevelop-local-masks` | `z6-nef` | `card` | Redevelop RAW on its Local Masks tab with a radial local on the globe |
| `redevelop-local-tools` | `z6-nef` | `card` | The Local Masks sidebar's range rows, Mask tool and local actions |
| `content-aware-fill` | `sky` | `sheet` | Content-Aware Fill on the tall chimney, ready to apply |
| `clone-stamp` | `sky` | `window`, `more` | The Clone Stamp's source, its overlay in the brush ring, and the More panel |
| `spot-healing-brush` | `sky` | `window`, `more` | The Spot Healing Brush over the small chimney, and the More panel |
| `remove-tool` | `sky` | `window` | The Remove tool holding a stroke down the tall chimney |
| `dodge-burn` | `sky` | `bar` | The Dodge / Burn options bar |
| `brush-options` | `sky` | `window`, `more` | A Calligraphy brush tip beside its stroke, and the More panel |
| `fill-command` | `sky` | `sheet` | The Fill dialog |
| `gradient-stops` | `sky` | `sheet` | Gradient Stops with three stops, one of them translucent |
| `text-editing` | `sky` | `window`, `bar` | A wrapping text layer reopened on the canvas, and the Type bar |
| `shape-edit` | `sky` | `window`, `bar`, `panel` | A rounded rectangle reopened with its handles, the shape bar and the Layers panel |
| `artwork-colors` | `sky` | `sheet` | Artwork Colors setting the first four characters of a text layer |
| `color-settings` | `sky` | `card` | Color Settings: working spaces, policies and conversion options |
| `proof-setup` | `sky` | `sheet` | Proof Setup simulating the working CMYK profile |
| `gamut-warning` | `sky` | `window` | Gamut Warning over the colours the press cannot print, and the proof in the status bar |
| `convert-to-profile` | `sky` | `sheet` | Convert to Profile with Apply greyed on the document's own profile |
| `duotone` | `sky` | `sheet` | Duotone Options with a black and a warm brown ink |
| `indexed-color` | `sky` | `sheet` | The Indexed Color dialog at its defaults |
| `color-table` | `sky` | `sheet` | The Color Table of a 32-colour Adaptive palette |
| `bitmap-mode` | `sky` | `sheet` | The Bitmap dialog on a Grayscale document, with Diffusion Dither |
| `cmyk-channels` | `sky` | `panel` | The Channels tab of a CMYK document: the composite and four plates |
| `convert-precision` | `sky` | `sheet` | Convert Precision to 16-bit, preserving two layers |
| `batch` | `sky` | `card` | The Batch window with an action chosen and no folders yet |
| `assistant-panel` | `sky` | `window`, `panel` | The Assistant tab beside the photo, and the tab itself |
| `depth-blur` | `sky` | `window`, `sheet` | Depth Blur blurring the far sky on the canvas, and its dialog (needs the models) |
| `sky-replacement` | `sky` | `sheet` | Sky Replacement previewing the rooftops under a library sky (needs the models) |
| `super-zoom` | `sky` | `sheet` | Super Zoom previewing the small chimney at ×2 (needs the models) |
| `generative-fill` | `sky` | `sheet` | The Generative Fill dialog on a selection |
| `cloud-consent` | `sky` | `card` | The consent card before the first send to OpenAI |

## Blocked

- `editor-32`: the editor on a 32-bit float document. Its status bar says "RGB · 16-bit" for a
  32-bit document ([#27](https://github.com/kpgalligan/doctored/issues/27)).
- `settings`: the Settings card. It is 2240 pt tall, taller than a laptop screen, because
  `resizeTabs` sizes every tab to the tallest pane ([#28](https://github.com/kpgalligan/doctored/issues/28)).
- Settings ▸ Models, when `settings` is unblocked: its intro prints the models folder's full path, which
  under isolation is the runner's scratch folder, so the screen needs a cut or a neutral `DR_MODELS_DIR`.
