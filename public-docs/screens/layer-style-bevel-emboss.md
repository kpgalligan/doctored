```json screen
{"id": "layer-style-bevel-emboss", "title": "Layer Style: Bevel & Emboss with Contour and Texture",
 "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_shape_layer", "args": {"kind": "rect", "x": 624, "y": 483, "w": 800, "h": 400,
            "fill": "#3a6ea5", "radius": 200}},
           {"tool": "set_layer_style", "args": {"layer": 1, "style": {"effects": [{"type": "bevel_emboss",
            "style": "inner_bevel", "depth": 1.5, "size": 40, "soften": 6, "contour_enabled": true,
            "contour": {"preset": "half_round"}, "texture_enabled": true,
            "texture": {"pattern": {"preset": "dots"}, "depth": 0.5}}]}}},
           {"ui": "open_layer_style", "args": {"effect": "bevel_emboss"}},
           {"ui": "wait", "args": {"seconds": 1.5}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Layer Style dialog on Bevel & Emboss, checked and selected in the list down the left. The
pane runs from Style Inner Bevel, Technique Smooth, Depth 150%, Direction Up, Size 40 px, Soften
6 px, Angle 120° and Altitude 30° with Use Global Light checked, through Gloss Contour Linear,
Highlight Mode Screen with a white colour at 75% opacity, and Shadow Mode Multiply with a black
colour at 75%. Below those, the Contour section is checked, with Contour Half Round and Range 50%,
and the Texture section is checked, with the Dots pattern, Scale 100%, Link with Layer checked, a
Snap to Origin button, Texture Depth +50% and Invert unchecked. Each of the two contour menus has
an anti-aliasing checkbox beside it. Reset, Cancel and Apply run along the bottom.
