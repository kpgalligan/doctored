```json screen
{"id": "mask-edit-overlay", "title": "Painting a layer mask", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "duplicate_layer", "args": {"index": 0}},
           {"tool": "apply_filter", "args": {"layer": 1, "filter": "grayscale"}},
           {"tool": "set_layer_properties", "args": {"index": 1, "name": "Black & white"}},
           {"tool": "select_ellipse", "args": {"x": 624, "y": 283, "width": 800, "height": 800}},
           {"tool": "modify_selection", "args": {"operation": "invert"}},
           {"tool": "add_layer_mask", "args": {"layer": 1, "kind": "from_selection"}},
           {"tool": "deselect", "args": {}},
           {"tool": "set_active_layer", "args": {"index": 1}},
           {"ui": "select_tool", "args": {"tool": "brush"}},
           {"ui": "set_paint_target", "args": {"target": "mask"}},
           {"ui": "set_channel_visibility", "args": {"rgb": true, "mask": true}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "panel", "of": "view", "view": "layers_panel"},
              {"name": "status", "of": "view", "view": "status_bar"}]}
```

## window

The editor with two layers: a black-and-white copy of the photo above the original, with a layer
mask that hides the copy inside a large circle in the middle. The mask is the edit target and is
shown, so a translucent red wash covers the circle, the area the mask hides. The Brush is chosen,
its options bar showing Hard Round, a Size of 24 px, Hardness, Opacity and Flow at 100% and the
Normal mode, and the status bar ends with Brush · B · Mask.

## panel

The Layers panel with two layers. Black & white, selected, shows its picture thumbnail beside its
mask thumbnail, a black circle on white, and the blue target ring sits around the mask thumbnail
rather than the picture. Background is below it, and the count at the bottom reads 2 layers.

## status

The status bar along the bottom of the window: 2048 × 1366 px · 72 ppi, RGB · 8-bit · sRGB
IEC61966-2.1 and Selection: none at the left, and Brush · B · Mask at the right end, naming the
tool, its key and the edit target.
