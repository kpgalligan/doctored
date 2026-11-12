```json screen
{"id": "spot-healing-brush", "title": "Spot Healing Brush and its options", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "select_tool", "args": {"tool": "spotHeal"}},
           {"ui": "set_tool_options", "args": {"options": "paint:spotHeal", "values": {"size": 70, "hardness": 80,
            "outputToNewLayer": true}}},
           {"ui": "zoom_to_rect", "args": {"rect": [700, 500, 700, 466]}},
           {"ui": "hover", "args": {"point": [1043, 800]}},
           {"ui": "open_overflow", "args": {}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "more", "of": "panel", "panel": "overflow"}]}
```

## window

The Spot Healing Brush zoomed to 130% on the small chimney, its round 70 px outline circling the
chimney, with Preset Custom, Size 70 px, Hardness 80% and Opacity 100% in the options bar and the
More panel open at the right of the bar.

## more

The Spot Healing Brush's More panel: Type Content-Aware, Sample Current Layer, Ignore adjustment
layers (greyed out), Retouch to New Layer and Expand destination, Flow and Blend greyed out, then
Spacing 25, Angle 0°, Roundness 100%, Smoothing 20%, Pressure size (checked), Airbrush and
Pressure opacity.
