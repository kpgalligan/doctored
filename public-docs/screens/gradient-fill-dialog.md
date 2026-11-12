```json screen
{"id": "gradient-fill-dialog", "title": "The Gradient Fill dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_fill_layer", "args": {"kind": "gradient", "name": "Dusk", "gradient": {"stops": [
            {"position": 0, "color": "#1b2a49"}, {"position": 0.55, "color": "#c0587e"},
            {"position": 1, "color": "#ffb347"}], "style": "linear", "angle": 90, "smoothness": 0.8}}},
           {"ui": "open_sheet", "args": {"sheet": "layer_content_options"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Gradient Fill dialog, reopened on a gradient fill layer. A hint says the gradient spans the
whole canvas, or with Align with Layer the bounds of the layer's mask, and that painting on the
mask chooses where it shows. Below it, the stop editor shows the ramp from navy through rose to
amber, with three colour stops under it, the first one selected, and midpoint diamonds between
them. The selected stop's Position (0%), Opacity (100%) and Color follow, with the Midpoint field
empty because no diamond is selected. Then Style is Linear, Angle 90°, Scale 100% and Smoothness
80%, with Reverse and Align with Layer unchecked, above Cancel and Apply.
