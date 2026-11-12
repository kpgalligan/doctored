```json screen
{"id": "transform-selection", "title": "The Transform Selection dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "select_ellipse", "args": {"x": 724, "y": 240, "width": 600, "height": 400}},
           {"ui": "open_sheet", "args": {"sheet": "transform_selection"}},
           {"ui": "set_control", "args": {"control": "selectionTransform.angle", "value": "20"}},
           {"ui": "set_control", "args": {"control": "selectionTransform.scale_x", "value": "130"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Transform Selection dialog for an elliptical selection over the sun. The captured photo sits on
the left and the coverage preview on the right, where the ellipse, white on black, is now wider and
turned 20° clockwise. The fields read Horizontal move and Vertical move 0, Horizontal scale 130%,
Vertical scale 100% and Clockwise angle 20°, with Interpolation set to Bilinear. A note gives the
pivot, 1024, 440 px, and says a negative scale flips the coverage, above Cancel and Apply. The hint
at the top says the layer pixels stay in place.
