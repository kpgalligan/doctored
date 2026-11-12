```json screen
{"id": "raw-layer-editor", "title": "A camera RAW document in the editor", "fixture": "z6-nef", "window": "editor_raw",
 "steps": [{"tool": "new_layer", "args": {"name": "Retouch"}},
           {"tool": "set_active_layer", "args": {"index": 0}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "panel", "of": "view", "view": "layers_panel"}]}
```

## window

The Nikon Z 6 file opened in the editor: a photograph of an origami star, a toy monkey, a globe and
a yellow toy wagon on a wooden shelf. The title reads Z6.NEF – 6048 × 4024 px and the status bar
reads 300 ppi, RGB, 16-bit. The Layers panel lists an empty Retouch layer above Background, which
is selected.

## panel

The Layers panel for an opened camera file: Retouch, an empty layer of 6048 × 4024, above the
selected Background, whose thumbnail carries a diamond badge and whose row reads camera RAW. The
count reads 2 layers.
