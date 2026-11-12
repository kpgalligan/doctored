```json screen
{"id": "selective-color", "title": "The Selective Color dialog", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "selective_color"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Selective Color dialog. Preset reads Default and Colors reads Reds, above Cyan, Magenta,
Yellow and Black sliders, all at 0%. Method is set to Relative, with Absolute beside it, and a
paragraph explains the difference between the two and that the colour ranges overlap. The note
±100 % ink per range · 1 undo sits beside Cancel and Apply.
