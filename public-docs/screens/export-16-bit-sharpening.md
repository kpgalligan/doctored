```json screen
{"id": "export-16-bit-sharpening", "title": "Export at 16 bits with output sharpening", "fixture": "sky",
 "window": "editor",
 "steps": [{"tool": "convert_precision", "args": {"mode": "rgba16", "flatten": true}},
           {"ui": "open_sheet", "args": {"sheet": "export"}},
           {"ui": "choose_popup_item", "args": {"title": "16-bit"}},
           {"ui": "set_control", "args": {"control": "export.sharpen", "value": true}},
           {"ui": "wait", "args": {"seconds": 3}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Export sheet on a 16-bit document with PNG chosen. The status line under the preview reads
2048×1366 px · 16-bit · 22.4 MB encoded. Two rows appear that an 8-bit document does not have: Bit
depth, set to 16-bit, and Dither, greyed at No dither because it applies only at 8 bits. JPEG quality
is greyed for PNG. Output sharpening is on, so its Method, Unsharp Mask, and its Radius 1, Amount 0.8
and Threshold 0.02 fields can be changed.
