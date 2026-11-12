```json screen
{"id": "liquify", "title": "The Liquify workspace after a Forward Warp stroke", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "liquify"}},
           {"ui": "liquify_stroke", "args": {"from": [1040, 940], "to": [1040, 690]}},
           {"ui": "set_control", "args": {"control": "liquify.mesh", "value": true}},
           {"ui": "liquify_actual_size", "args": {"center": [1040, 800]}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Liquify workspace after one Forward Warp stroke, its preview at 100%. In the preview a roof
and its chimney have been pushed up into the clouds as a rounded hump, and the green mesh over the
picture bends upward with it, its lines straight again away from the stroke. Beside the preview are
Tool Forward Warp, Size 205 px, Pressure 50% and Density 50%, each a field above a slider, Show
Mesh and Show Mask, both checked, and the Undo Stroke and Restore All buttons. Under the preview
are Fit and 100% and the zoom, 100%; Cancel and Apply are at the bottom right.
