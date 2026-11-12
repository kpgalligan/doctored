```json screen
{"id": "warp-dialog", "title": "The Warp dialog reopened on a stored warp", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "warp_layer", "args": {"operation": "set", "layer": 0, "target": "pixels", "columns": 3, "rows": 3,
            "filter": "bicubic", "displacements": [[0, 0], [0, 0], [0, 0], [0, 0], [0, -160], [0, 0], [0, 0], [0, 0], [0, 0]]}},
           {"ui": "wait", "args": {"seconds": 2}},
           {"ui": "open_sheet", "args": {"sheet": "warp"}},
           {"ui": "set_control", "args": {"control": "warp-node", "value": 4}},
           {"ui": "wait", "args": {"seconds": 0.5}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Warp dialog reopened on a warp stored with the layer. The hint says it edits a displacement
field that can be reopened, on the entire layer or its mask. Target is Pixels, Columns 3 and Rows 3,
and Node is set to Row 2, Column 2, the centre of the grid, whose dx reads 0.0 and dy -160.0;
Sampler is Bicubic. Under them are Reset Field, Remove Warp and Bake Warp, a note that node
displacements are in original canvas pixels and that changing the dimensions resets the local field,
and Cancel and Apply.
