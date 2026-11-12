```json screen
{"id": "quick-mask", "title": "Quick Mask Mode over a feathered selection", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "select_ellipse", "args": {"x": 624, "y": 140, "width": 800, "height": 560, "feather": 40}},
           {"tool": "set_quick_mask", "args": {"active": true}},
           {"ui": "select_tool", "args": {"tool": "brush"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "window", "of": "window"}]}
```

The editor in Quick Mask mode. A red tint covers the whole photo except a wide ellipse in the upper
middle, around the sun, where the clouds show clear; the tint fades in gradually across the
ellipse's soft, feathered edge. The Brush is chosen, its options bar showing Hard Round, Size 24 px,
Hardness, Opacity and Flow at 100% and the Normal mode, black is the foreground colour, and the
status bar reads Quick Mask.
