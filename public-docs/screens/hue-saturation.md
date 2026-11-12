```json screen
{"id": "hue-saturation", "title": "The Hue/Saturation dialog", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "hue_saturation"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Hue/Saturation dialog. Preset reads Default and Range reads Master, above Hue at 0°, Saturation
at 0% and Lightness at 0%, each a slider with its value. Under them are the Colorize and Targeted
Adjustment checkboxes, both off, and the note Colorize replaces, not shifts sits beside Cancel and
Apply.
