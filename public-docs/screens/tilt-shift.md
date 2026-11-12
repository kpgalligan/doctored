```json screen
{"id": "tilt-shift", "title": "Tilt-Shift with its live preview", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "tilt_shift"}},
           {"ui": "set_control", "args": {"control": "Blur:", "value": 40}},
           {"ui": "wait", "args": {"seconds": 3}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "sheet", "of": "sheet"}]}
```

## window

The Tilt-Shift dialog open over the editor, which is dimmed behind it. The canvas previews the
blur: the clouds along the top of the photo are smeared soft, while the clouds just above the
rooftops, on either side of the dialog, keep their sharp texture. The dark rooftops fill the lower
half of the photo.

## sheet

The Tilt-Shift dialog: Blur 40 px, Angle 0°, Focus 15% and Transition 25%, each a slider with its
value. The Blur Center box shows the whole photo with a cross at its centre, where the sharp band
runs. The note one undo step sits beside Cancel and Apply.
