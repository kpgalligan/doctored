```json screen
{"id": "layer-comps-window", "title": "The Layer Comps window", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_shape_layer", "args": {"kind": "rect", "x": 120, "y": 96, "w": 1010, "h": 300,
            "fill": "#1d3557cc", "radius": 36}},
           {"tool": "add_text_layer", "args": {"text": "ROOFTOPS", "x": 170, "y": 150, "size": 180,
            "color": "#ffffff"}},
           {"tool": "save_layer_comp", "args": {"name": "Title card", "comment": "Client review, round 2"}},
           {"tool": "set_layer_properties", "args": {"index": 1, "visible": false}},
           {"tool": "set_layer_properties", "args": {"index": 2, "visible": false}},
           {"tool": "save_layer_comp", "args": {"name": "Clean plate"}},
           {"tool": "set_layer_properties", "args": {"index": 1, "visible": true, "opacity": 0.5}},
           {"tool": "set_layer_properties", "args": {"index": 2, "visible": true}},
           {"tool": "save_layer_comp", "args": {"name": "Soft badge", "position": false}},
           {"tool": "apply_layer_comp", "args": {"name": "Title card"}},
           {"ui": "show_window", "args": {"window": "layer_comps"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "comps", "of": "panel", "panel": "layer_comps"}]}
```

The Layer Comps window for an untitled document. A hint says a comp records each layer's
visibility, position and appearance, never pixels, the selection or the history, and that V P A
shows what each comp captures. Three comps are listed: Title card and Clean plate, each followed
by V P A, and Soft badge, followed by V – A because it leaves positions out. A status line reads
3 comps · 0 stale entries · 0 locked, above the New…, Apply, ◀, ▶, Update, Options… and Delete
buttons.
