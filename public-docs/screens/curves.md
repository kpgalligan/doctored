```json screen
{"id": "curves", "title": "The Curves dialog with an S-curve", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "curves"}},
           {"ui": "set_curve_points", "args": {"points": [[0, 0], [64, 44], [192, 218], [255, 255]]}},
           {"ui": "wait", "args": {"seconds": 1.5}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Curves dialog with an S-curve. Under a hint on adding, removing and drawing points, the Preset
menu reads Custom, Channel reads RGB and Edit is set to Points rather than Pencil. The curve bends
through two added points, darkening the shadows and lifting the highlights, over the photo's
histogram, with the straight diagonal behind it. An After histogram follows, then a Point 1 menu
with Fine entry Input and Output fields, Reset, a Targeted Adjustment checkbox, a note that the
dialog makes one undo step, and Cancel and Apply.
