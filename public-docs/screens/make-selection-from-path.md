```json screen
{"id": "make-selection-from-path", "title": "The Make Selection dialog for a path", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "set_path", "args": {"path": "Sun", "subpaths": [{"closed": true, "anchors": [
            {"x": 1055, "y": 413, "in": [972, 413], "out": [1138, 413], "smooth": true},
            {"x": 1205, "y": 563, "in": [1205, 480], "out": [1205, 646], "smooth": true},
            {"x": 1055, "y": 713, "in": [1138, 713], "out": [972, 713], "smooth": true},
            {"x": 905, "y": 563, "in": [905, 646], "out": [905, 480], "smooth": true}]}]}},
           {"ui": "set_current_path", "args": {"name": "Sun"}},
           {"ui": "open_sheet", "args": {"sheet": "make_selection_from_path"}},
           {"ui": "wait", "args": {"seconds": 0.5}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Make selection dialog for the current path. The hint says the path's fill becomes the selection
and that an open subpath is closed by a straight line. Feather Radius is 0 px, Anti-aliased is
checked, and Operation reads New Selection, the only choice while nothing is selected. A note gives
the feather range, 0 to 250 px, beside Cancel and Apply.
