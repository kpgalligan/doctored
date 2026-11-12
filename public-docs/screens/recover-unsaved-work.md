```json screen
{"id": "recover-unsaved-work", "title": "Recover Unsaved Work", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "new_layer", "args": {"name": "Retouch"}},
           {"ui": "seed_recovery_copy", "args": {"title": "Rooftops"}},
           {"ui": "show_window", "args": {"window": "recovery"}}],
 "captures": [{"name": "recovery", "of": "panel", "panel": "recovery"}]}
```

The Recover Unsaved Work window. A line says recovery copies keep unsaved editing state, that
Recover opens a new copy, and that original files stay unchanged. One copy is listed and chosen:
Rooftops, with the date and time it was taken. Refresh and Save Recovery Now sit at the bottom left,
Discard… and Recover Copy at the right.
