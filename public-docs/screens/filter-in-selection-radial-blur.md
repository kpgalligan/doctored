```json screen
{"id": "filter-in-selection-radial-blur", "title": "Radial Blur confined to an elliptical selection", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "select_ellipse", "args": {"x": 80, "y": 60, "width": 640, "height": 520}},
           {"ui": "open_sheet", "args": {"sheet": "radial_blur"}},
           {"ui": "wait", "args": {"seconds": 3}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "sheet", "of": "sheet"}]}
```

## window

The Radial Blur dialog open over the editor, which is dimmed behind it. A dashed elliptical
selection sits in the sky at the upper left of the photo. Inside it the clouds are already swirled
around the ellipse's centre, while the clouds outside it stay sharp. The status bar reads
Selection: 640 × 520 px.

## sheet

The Radial Blur dialog: Amount 10, Blur Method Spin and Quality Good. The Blur Center box shows
only the selected part of the photo, dark sky and clouds, with a cross at its centre marking the
point the blur turns around. The note one undo step sits beside Cancel and Apply.
