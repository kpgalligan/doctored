```json screen
{"id": "paths-panel", "title": "The Paths panel and a path under the Pen", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "set_path", "args": {"subpaths": [{"closed": false,
            "anchors": [[988, 883], [1034, 829], [1167, 827], [1219, 881]]}]}},
           {"tool": "set_path", "args": {"path": "Sun", "subpaths": [{"closed": true, "anchors": [
            {"x": 1055, "y": 413, "in": [972, 413], "out": [1138, 413], "smooth": true},
            {"x": 1205, "y": 563, "in": [1205, 480], "out": [1205, 646], "smooth": true},
            {"x": 1055, "y": 713, "in": [1138, 713], "out": [972, 713], "smooth": true},
            {"x": 905, "y": 563, "in": [905, 646], "out": [905, 480], "smooth": true}]}]}},
           {"ui": "select_tool", "args": {"tool": "pen"}},
           {"ui": "show_panel_tab", "args": {"tab": "paths"}},
           {"ui": "set_current_path", "args": {"name": "Sun"}},
           {"ui": "zoom_to_rect", "args": {"rect": [555, 196, 1000, 735]}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "panel", "of": "view", "view": "paths_panel"}]}
```

## window

The editor zoomed to 91% on the sun, with the Pen chosen and its options bar showing Rubber Band
checked. The path Sun circles the sun: four square anchors at its top, bottom, left and right, each
with a straight handle line running out to a small round handle on both sides, the mark of a smooth
anchor. The Paths tab is open on the right, and the status bar ends with Pen · P.

## panel

The Paths tab with two paths. Work Path, the unsaved scratch path, reads work path · 4 anchors; Sun,
highlighted as the current path, reads path · 4 anchors. Each has a small outline thumbnail. Along
the bottom are buttons to fill the path, stroke it, make a selection from it, make a work path from
the selection (dimmed), add a vector mask from it and delete it, and the count reads 2 paths.
