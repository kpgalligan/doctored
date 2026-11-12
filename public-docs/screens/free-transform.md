```json screen
{"id": "free-transform", "title": "Free Transform on a text layer", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_text_layer", "args": {"text": "Rooftops", "size": 240, "color": "#F2B134", "x": 560, "y": 220}},
           {"ui": "free_transform", "args": {}},
           {"ui": "set_transform", "args": {"angle": -8, "scale_x": 0.9, "scale_y": 0.9}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "bar", "of": "view", "view": "options_bar"}]}
```

## window

Free Transform on a text layer. The amber word Rooftops over the clouds sits inside a box turned 8°
counterclockwise and scaled to 90%, with square handles at its four corners and at the middle of
each side, and a small mark at its centre, the point it turns around. The Layers panel lists
Rooftops, a text layer, above Background, and the status bar ends with Free Transform.

## bar

The Free Transform options bar: Angle -8.00°, Scale X and Y at 90.00%, W 975 and H 344, and Sampler
set to Bicubic (Catmull-Rom), with More at the right end.
