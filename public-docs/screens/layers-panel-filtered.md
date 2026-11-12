```json screen
{"id": "layers-panel-filtered", "title": "The Layers panel filtered by a colour label", "fixture": "sky",
 "window": "editor",
 "steps": [{"tool": "duplicate_layer", "args": {"index": 0}},
           {"tool": "set_layer_properties", "args": {"index": 1, "name": "Sky"}},
           {"tool": "select_rect", "args": {"x": 0, "y": 0, "width": 2048, "height": 760}},
           {"tool": "add_layer_mask", "args": {"layer": 1, "kind": "from_selection"}},
           {"tool": "deselect", "args": {}},
           {"tool": "add_adjustment_layer", "args": {"op": "bcs", "params": {"contrast": 0.25},
            "name": "Sky contrast", "layer": 1}},
           {"tool": "group_layers", "args": {"layers": [1, 2], "name": "Sky treatment"}},
           {"tool": "set_layer_clipped", "args": {"layer": 2, "clipped": true}},
           {"tool": "add_shape_layer", "args": {"kind": "rect", "x": 120, "y": 96, "w": 1010, "h": 300,
            "fill": "#1d3557cc", "radius": 36, "layer": 3, "position": "above"}},
           {"tool": "set_layer_properties", "args": {"index": 4, "name": "Badge"}},
           {"tool": "add_text_layer", "args": {"text": "ROOFTOPS", "x": 170, "y": 150, "size": 180,
            "color": "#ffffff", "layer": 4, "position": "above"}},
           {"tool": "set_layer_label", "args": {"layers": [4, 5], "label": "red"}},
           {"tool": "set_layer_label", "args": {"layer": 3, "label": "blue"}},
           {"tool": "set_active_layer", "args": {"index": 5}},
           {"ui": "set_layers_filter", "args": {"label": "red"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "panel", "of": "view", "view": "layers_panel"}]}
```

The Layers panel with its label filter set to Red: the filter bar under the Opacity slider reads
All Kinds and Red. Only the two red-labelled layers are listed, ROOFTOPS, the selected text layer,
and Badge, a shape layer, each with a red tint behind its eye. The group, the adjustment and the
photo layers are left out of the list, and the count at the bottom reads Filtered 2 of 6 instead
of a layer count.
