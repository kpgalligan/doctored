```json screen
{"id": "color-samplers", "title": "Color samplers on the canvas and in Info", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "set_color_samplers", "args": {"samplers": [{"x": 154, "y": 307, "sample_size": 3},
            {"x": 1126, "y": 461, "sample_size": 3}, {"x": 1024, "y": 1126, "sample_size": 1}]}},
           {"ui": "select_tool", "args": {"tool": "colorSampler"}},
           {"ui": "show_panel_tab", "args": {"tab": "info"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "window", "of": "window"}]}
```

The editor with the Color Sampler tool chosen and three numbered sampler markers on the photo:
#1 on the dark blue sky at the left, #2 on the bright clouds beside the sun and #3 on the black
rooftops. The options bar shows a Sample pop-up set to Point and a Clear All button. In the Info
tab on the right, under Color Samplers with an RGB pop-up, there is one reading for each marker:
#1 3×3 11, 30, 47; #2 3×3 253, 252, 250; and #3 1×1 0, 0, 0. The pointer's own readings above them
are dashes, because the pointer is not over the canvas.
