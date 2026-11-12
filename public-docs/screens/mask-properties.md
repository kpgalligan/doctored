```json screen
{"id": "mask-properties", "title": "The Mask Properties dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "new_layer", "args": {"name": "Warm light"}},
           {"tool": "fill_selection", "args": {"layer": 1, "color": "#ff9a3d"}},
           {"tool": "select_ellipse", "args": {"x": 150, "y": 160, "width": 500, "height": 500}},
           {"tool": "add_layer_mask", "args": {"layer": 1, "kind": "from_selection"}},
           {"tool": "deselect", "args": {}},
           {"tool": "set_active_layer", "args": {"index": 1}},
           {"tool": "set_mask_properties", "args": {"layer": 1, "feather": 60, "density": 0.9}},
           {"ui": "open_sheet", "args": {"sheet": "mask_properties"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"},
              {"name": "window", "of": "window"}]}
```

## sheet

The Mask Properties dialog. A hint says the original mask stays editable, that a lower density
reveals more of the layer and that Feather softens the edge. Density is at 90.0%, Feather at
60.00 px, and Move mask with layer content is checked, above Cancel and Apply.

## window

The editor behind the open dialog, dimmed while it is open. An orange layer, Warm light, sits over
the photo with a mask
that shows it only inside a circle at the upper left. With a 60 px feather the circle's edge fades
softly into the sky, and at 90% density a faint orange cast also covers the rest of the picture.
The Layers panel shows the orange layer beside its mask thumbnail, a small white spot on a dark
ground, above Background.
