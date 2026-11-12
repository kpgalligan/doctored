```json screen
{"id": "auto-align-sheet", "title": "The Auto-Align Layers dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "duplicate_layer", "args": {"index": 0}},
           {"tool": "duplicate_layer", "args": {"index": 0}},
           {"tool": "set_selected_layers", "args": {"layers": [0, 1, 2], "primary": 2}},
           {"ui": "open_sheet", "args": {"sheet": "auto_align_layers"}},
           {"ui": "wait", "args": {"seconds": 0.5}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Auto-Align Layers dialog. A hint says the bottom selected layer, or the one position-locked
layer, stays put while the canvas grows to hold the rest, and explains how Cylindrical and
Spherical, Vignette Removal and Geometric Distortion treat the layers. Projection offers six radio
buttons, Auto (chosen), Perspective, Collage, Cylindrical, Spherical and Reposition, and Lens
Correction two checkboxes, Vignette Removal and Geometric Distortion, both off, above Cancel and
Apply.
