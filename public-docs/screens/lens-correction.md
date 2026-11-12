```json screen
{"id": "lens-correction", "title": "Lens Correction with its alignment grid", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "lens_correction"}},
           {"ui": "wait", "args": {"seconds": 2}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "sheet", "of": "sheet"}]}
```

## window

The Lens Correction dialog open over the editor, which is dimmed behind it. A grid of thin lines
in square cells is drawn across the photo, plainest over the sky: the alignment grid Show Grid
draws on the canvas while the dialog is open.

## sheet

The Lens Correction dialog: Distortion, Red/Cyan, Blue/Yellow and Vignette at +0, Midpoint at 50,
Vertical and Horizontal at +0, Angle 0° and Scale 100%, each a slider with its value. Lens Profile
reads None, with the line No lens profile. under it, and Show Grid is checked. The note one undo
step sits beside Cancel and Apply.
