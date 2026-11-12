```json screen
{"id": "psd-open-report", "title": "Opening a PSD: the compatibility notice", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_adjustment_layer", "args": {"op": "curves",
            "params": {"rgb": [[0, 0], [70, 52], [190, 205], [255, 255]]}}},
           {"tool": "add_shape_layer", "args": {"kind": "rect", "x": 1240, "y": 240, "w": 560, "h": 320,
            "radius": 36, "fill": "#f2c14e"}},
           {"tool": "set_layer_style", "args": {"layer": 2, "style": {"effects": [{"type": "drop_shadow",
            "distance": 12, "size": 24}]}}},
           {"tool": "add_text_layer", "args": {"text": "Rooftops", "x": 120, "y": 120, "size": 160,
            "color": "#ffffff"}},
           {"ui": "open_psd", "args": {"name": "rooftops.psd"}}],
 "captures": [{"name": "notice", "of": "alert"}]}
```

The notice that opening a PSD file puts up, here for a file named rooftops.psd. Under the app icon,
the title reads Open Photoshop document “rooftops.psd”, and a sentence says Stored Composite keeps the
file's saved appearance as one raster layer while Import Layers keeps the supported editable layers
and may change the appearance. A scrolling compatibility report follows: 8-bit RGB · 2048 × 1366
pixels; Verified losses in this file, with none detected; then Observed in this file, with notes on
the rectangle layer's drop shadow, the Curves layer's mask and the Curves adjustment itself, and more
below. The buttons are Open Stored Composite, Import Layers and Cancel.
