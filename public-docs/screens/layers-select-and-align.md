```json screen
{"id": "layers-select-and-align", "title": "Several layers selected, with the Move tool's align buttons",
 "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_shape_layer", "args": {"kind": "ellipse", "x": 260, "y": 300, "w": 360, "h": 360,
            "fill": "#e63946"}},
           {"tool": "add_shape_layer", "args": {"kind": "ellipse", "x": 840, "y": 560, "w": 360, "h": 360,
            "fill": "#f4a261"}},
           {"tool": "add_shape_layer", "args": {"kind": "ellipse", "x": 1420, "y": 400, "w": 360, "h": 360,
            "fill": "#2a9d8f"}},
           {"tool": "set_selected_layers", "args": {"layers": [1, 2, 3], "primary": 2}},
           {"ui": "select_tool", "args": {"tool": "move"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "options", "of": "view", "view": "options_bar"}]}
```

## window

The editor with three coloured circles over the rooftop photo, each on its own shape layer: red
at the left, orange lower down in the middle and teal at the right. In the Layers panel all three
Ellipse rows are selected. The middle one, the orange circle, is the primary layer, with blue text
and a ring around its thumbnail; the other two have a lighter highlight. The Move tool is chosen,
and its options bar runs along the top of the window.

## options

The Move tool's options bar: Auto-select, off, with its Layer menu dimmed; Show transform
controls, off; six align buttons for left edges, horizontal centres, right edges, top edges,
vertical centres and bottom edges; two distribute buttons for horizontal and vertical spacing;
Snap to layers, on; Nudge set to 1 px; and More at the right end.
