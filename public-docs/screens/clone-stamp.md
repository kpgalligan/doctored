```json screen
{"id": "clone-stamp", "title": "Clone Stamp with its source, overlay and options", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "new_layer", "args": {"name": "Retouch"}},
           {"ui": "select_tool", "args": {"tool": "clone"}},
           {"ui": "set_tool_options", "args": {"options": "paint:clone", "values": {"size": 90, "hardness": 60,
            "sample": "current_and_below", "outputToNewLayer": false, "sourceOverlay": true}}},
           {"ui": "zoom_to_rect", "args": {"rect": [700, 500, 700, 466]}},
           {"ui": "set_clone_source", "args": {"point": [960, 700]}},
           {"ui": "hover", "args": {"point": [1043, 812]}},
           {"ui": "open_overflow", "args": {}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "more", "of": "panel", "panel": "overflow"}]}
```

## window

The Clone Stamp zoomed to 130% on the small chimney, with Preset Custom, Size 90 px, Hardness 60%
and Opacity 100% in the options bar. A crosshair in the clouds up and to the left marks the source
point, and the round brush outline over the chimney shows the sky from that source at half
strength, lightening the chimney and the roof edge inside it. The More panel hangs under the More
button at the right of the bar, over the Layers panel.

## more

The Clone Stamp's More panel: Sample Current & Below, Ignore adjustment layers, Retouch to This
Layer, Expand destination, Source overlay (checked), Source scale 100.0 (selected for editing),
Source rotation 0.0°, Flip source horizontally and Flip source vertically, then Flow 100%, Blend
Normal, Spacing 25%, Angle 0°, Roundness 100%, Smoothing 20%, Pressure size (checked), Airbrush and
Pressure opacity.
