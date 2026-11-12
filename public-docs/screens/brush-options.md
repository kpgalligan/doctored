```json screen
{"id": "brush-options", "title": "The Brush tip, its options and the More panel", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "new_layer", "args": {"name": "Paint"}},
           {"tool": "brush_stroke", "args": {"points": [[260, 330], [620, 470], [980, 300], [1340, 440]], "size": 70,
            "color": "#E8A33D", "hardness": 95, "angle": -45, "roundness": 30, "spacing": 5}},
           {"ui": "select_tool", "args": {"tool": "brush"}},
           {"ui": "set_tool_options", "args": {"options": "paint:brush", "values": {"size": 220, "hardness": 95,
            "flow": 100, "spacing": 15, "angle": -45, "roundness": 30, "airbrush": false, "smoothing": 40}}},
           {"ui": "set_colors", "args": {"foreground": "#E8A33D", "background": "#1F3A5F"}},
           {"ui": "hover", "args": {"point": [1560, 560]}},
           {"ui": "open_overflow", "args": {}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "more", "of": "panel", "panel": "overflow"}]}
```

## window

An orange calligraphic stroke zigzags across the clouds on a Paint layer, its width changing with
its direction as a flat tip's does, and the Brush's flattened, tilted outline hovers in the sky to
the right. The options bar reads Preset Calligraphy, Size 220 px, Hardness 95%, Opacity 100%, Flow
100% and the blend mode Normal, and the More panel is open at its right. The colour swatches at
the foot of the tool rail are orange over dark blue.

## more

The Brush's More panel: Spacing 15 (selected for editing), Angle -45°, Roundness 30%, Smoothing
40%, Pressure size (checked), and Airbrush and Pressure opacity, both unchecked.
