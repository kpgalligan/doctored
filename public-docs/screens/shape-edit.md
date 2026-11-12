```json screen
{"id": "shape-edit", "title": "Re-editing a shape layer", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_shape_layer", "args": {"kind": "rect", "x": 240, "y": 960, "w": 700, "h": 280,
            "fill": "#E8A33D", "stroke": "#FFFFFF", "stroke_width": 10, "radius": 36}},
           {"tool": "add_shape_layer", "args": {"kind": "ellipse", "x": 1300, "y": 160, "w": 420, "h": 420,
            "fill": "#3D7DE8B3", "stroke": "#FFFFFF", "stroke_width": 6, "stroke_dash": "dashed"}},
           {"ui": "reopen_shape", "args": {"layer": 1}},
           {"ui": "wait", "args": {"seconds": 0.5}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "bar", "of": "view", "view": "options_bar"},
              {"name": "panel", "of": "view", "view": "layers_panel"}]}
```

## window

A shape layer reopened for editing: an orange rounded rectangle with a white outline over the dark
rooftops sits in a box with eight square handles, and a translucent blue circle with a dashed white
outline floats in the sky. The options bar shows the rectangle's own settings, and the Layers panel
lists Ellipse and the selected Rectangle, both shape layers, above Background.

## bar

The shape options bar for the reopened rectangle: an orange Fill well, a white Stroke well, a
stroke weight of 10 px, Radius 36 px, and three pop-ups reading Miter, None and Center for the
corner style, the dash pattern and the stroke's alignment, with More at the right end.

## panel

The Layers panel with two shape layers above Background: Ellipse, with a round blue thumbnail, and
the selected Rectangle, with an orange bar for a thumbnail, each reading shape. The count reads 3
layers.
