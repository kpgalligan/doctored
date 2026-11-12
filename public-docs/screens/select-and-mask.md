```json screen
{"id": "select-and-mask", "title": "The Select and Mask workspace", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "select_magic_wand", "args": {"x": 1024, "y": 1250, "tolerance": 32}},
           {"tool": "modify_selection", "args": {"operation": "invert"}},
           {"ui": "open_sheet", "args": {"sheet": "select_and_mask"}},
           {"ui": "set_control", "args": {"control": "refine.edgeRadius", "value": "8"}},
           {"ui": "set_control", "args": {"control": "refine.smartRadius", "value": true}},
           {"ui": "set_control", "args": {"control": "refine.output", "value": 1}},
           {"ui": "wait", "args": {"seconds": 3}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Select and Mask dialog with the sky selected. A large preview on the left shows the cloudy sky
over a checkerboard where the rooftops were left out, with the hard roofline along the bottom of the
sky. Under the preview are a Checkerboard background menu, a Before checkbox, Fit and 100% buttons
and the zoom, 26%. The column on the right holds Edge Radius at 8 px with Smart Radius checked, then
Smooth, Feather, Shift and Contrast, each a field and a slider at zero; the Refine Edge Brush menu
set to Off, Brush Size 30 px and the Undo Stroke and Clear buttons; Decontaminate Colors unchecked
with Amount 100%; and Output set to New Layer Mask, naming Background as the layer it masks. A note
under the preview suggests 100% for checking single edges, above Cancel and Apply.
