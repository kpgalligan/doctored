```json screen
{"id": "history-window", "title": "The History panel", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_adjustment_layer", "args": {"op": "curves",
            "params": {"rgb": [[0, 0], [70, 52], [190, 205], [255, 255]]}}},
           {"tool": "add_adjustment_layer", "args": {"op": "vibrance", "params": {"vibrance": 0.25}}},
           {"tool": "new_layer", "args": {"name": "Retouch"}},
           {"tool": "select_rect", "args": {"x": 682, "y": 455, "width": 682, "height": 455}},
           {"tool": "undo", "args": {}},
           {"ui": "show_window", "args": {"window": "history"}}],
 "captures": [{"name": "history", "of": "panel", "panel": "history"}]}
```

The History panel for an untitled document. A hint says that choosing a state undoes or redoes to
it. The list starts with Initial State, marked Saved and with a pencil mark for the History
Brush's source, then New Curves Layer, New Vibrance Layer and New Layer, highlighted with a dot as
the current state; Selection, the step just undone, waits below it. Under the list, a memory line
reads 3 KB additional / 2 GB · 0 older states removed, above the History budget menu set to
2 GiB.
