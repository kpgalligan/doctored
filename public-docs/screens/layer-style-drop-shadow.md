```json screen
{"id": "layer-style-drop-shadow", "title": "Layer Style: two stacked Drop Shadows", "fixture": "sky",
 "window": "editor",
 "steps": [{"tool": "add_shape_layer", "args": {"kind": "rect", "x": 524, "y": 383, "w": 1000, "h": 600,
            "fill": "#f6f1e7", "radius": 48}},
           {"tool": "set_layer_style", "args": {"layer": 1, "style": {"effects": [
            {"type": "drop_shadow", "opacity": 0.35, "distance": 4, "size": 6},
            {"type": "drop_shadow", "opacity": 0.3, "distance": 28, "size": 60, "spread": 0.05,
             "contour": {"preset": "gaussian"}}]}}},
           {"ui": "open_layer_style", "args": {"effect": "drop_shadow"}},
           {"ui": "wait", "args": {"seconds": 1.5}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Layer Style dialog on a shape layer with two drop shadows. In the list down the left, two
checked Drop Shadow rows sit at the bottom, and the upper one, the top shadow of the two, is
selected. Above the pane, the Instance bar holds up and down arrows and a trash button for moving
or removing the selected shadow. The pane shows Blend Mode Multiply, a black Color, Opacity 30%,
Angle 120° with Use Global Light checked, Distance 28 px, Spread 5%, Size 60 px, Contour Gaussian
with an anti-aliasing checkbox beside it, Noise 0% and Layer Knocks Out Drop Shadow checked. Reset,
Cancel and Apply run along the bottom.
