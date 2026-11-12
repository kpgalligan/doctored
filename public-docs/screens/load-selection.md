```json screen
{"id": "load-selection", "title": "The Load Selection dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "select_magic_wand", "args": {"x": 1024, "y": 1250, "tolerance": 32}},
           {"tool": "save_selection", "args": {"name": "Rooftops"}},
           {"tool": "deselect", "args": {}},
           {"ui": "open_sheet", "args": {"sheet": "load_selection"}},
           {"ui": "wait", "args": {"seconds": 0.5}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Load selection dialog. Source is set to Rooftops, a selection saved earlier as a channel; below
it are an unchecked Invert box and Operation set to Replace. The hint says a channel, a layer's
transparency or mask, or a colour plane can be loaded, and that an empty source simply deselects. A
note reads selection undo, beside Cancel and Load.
