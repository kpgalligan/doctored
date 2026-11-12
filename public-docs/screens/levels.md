```json screen
{"id": "levels", "title": "The Levels dialog", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "levels"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Levels dialog. Preset and Channel menus sit above two histograms of the image, the second
labelled After, each with markers at its black and white ends. Below them are sliders for Black,
White, Gamma, Output black and Output white, a Fine entry section with the same five values as
numbers, and Reset, Cancel and Apply buttons.
