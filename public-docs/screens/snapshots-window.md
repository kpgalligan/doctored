```json screen
{"id": "snapshots-window", "title": "The Snapshots panel", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "apply_filter", "args": {"filter": "unsharp_mask", "radius": 1.5, "amount": 0.8}},
           {"tool": "create_snapshot", "args": {"name": "Sharpened"}},
           {"tool": "add_adjustment_layer", "args": {"op": "curves",
            "params": {"rgb": [[0, 0], [70, 52], [190, 205], [255, 255]]}}},
           {"tool": "create_snapshot", "args": {"name": "Contrast curve"}},
           {"ui": "show_window", "args": {"window": "snapshots"}}],
 "captures": [{"name": "snapshots", "of": "panel", "panel": "snapshots"}]}
```

The Snapshots panel for an untitled document. A hint says snapshots hold the full image, selection
and edit target, that saving a .drsnapshot file keeps one after closing, and that ordinary .doctored
saves do not include the list. Two session snapshots are listed, Sharpened and Contrast curve, each
2048 × 1366. A line under the list reads 0 MiB additional / 2048 MiB budget, above the New…,
Restore, Rename…, Remove, Save… and Load… buttons.
