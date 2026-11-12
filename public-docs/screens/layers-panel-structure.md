```json screen
{"id": "layers-panel-structure", "title": "The Layers panel with a group, a clipped adjustment, labels, locks and links",
 "fixture": "sky", "window": "editor",
 "steps": [{"tool": "duplicate_layer", "args": {"index": 0}},
           {"tool": "set_layer_properties", "args": {"index": 1, "name": "Sky"}},
           {"tool": "select_rect", "args": {"x": 0, "y": 0, "width": 2048, "height": 760}},
           {"tool": "add_layer_mask", "args": {"layer": 1, "kind": "from_selection"}},
           {"tool": "deselect", "args": {}},
           {"tool": "add_adjustment_layer", "args": {"op": "bcs", "params": {"contrast": 0.25},
            "name": "Sky contrast", "layer": 1}},
           {"tool": "group_layers", "args": {"layers": [1, 2], "name": "Sky treatment"}},
           {"tool": "set_layer_clipped", "args": {"layer": 2, "clipped": true}},
           {"tool": "set_layer_mask_enabled", "args": {"layer": 2, "enabled": false}},
           {"tool": "add_shape_layer", "args": {"kind": "rect", "x": 120, "y": 96, "w": 1010, "h": 300,
            "fill": "#1d3557cc", "radius": 36, "layer": 3, "position": "above"}},
           {"tool": "set_layer_properties", "args": {"index": 4, "name": "Badge"}},
           {"tool": "add_text_layer", "args": {"text": "ROOFTOPS", "x": 170, "y": 150, "size": 180,
            "color": "#ffffff", "layer": 4, "position": "above"}},
           {"tool": "set_layer_style", "args": {"layer": 5, "style": {"effects": [{"type": "drop_shadow",
            "distance": 6, "size": 10, "opacity": 0.5}]}}},
           {"tool": "link_layers", "args": {"layers": [4, 5]}},
           {"tool": "set_layer_label", "args": {"layers": [4, 5], "label": "red"}},
           {"tool": "set_layer_label", "args": {"layer": 3, "label": "blue"}},
           {"tool": "set_layer_lock", "args": {"layer": 4, "locks": ["position"]}},
           {"tool": "set_active_layer", "args": {"index": 3}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "panel", "of": "view", "view": "layers_panel"}]}
```

The Layers panel with six entries. At the top, ROOFTOPS is a text layer with a layer style: its
thumbnail carries fx and T marks and its row reads text · fx. Badge, below it, is a shape layer.
Both have a red label tint behind their eye and a link badge at the right end of the row, and
Badge also shows an open padlock for its position lock. Next comes the group Sky treatment, open
and selected: its eye sits on a blue label tint, its row reads group · 2 items, and the blend mode
menu at the top of the panel reads Pass Through. Inside the group, set in under a guide line, Sky
contrast is an adjustment layer clipped to the layer below it, shown by a bent arrow before its
thumbnail, and its mask thumbnail is struck through because the mask is turned off. Sky, under it,
shows the photo beside its mask, white over black. Background is at the bottom, and the count
reads 5 layers.
