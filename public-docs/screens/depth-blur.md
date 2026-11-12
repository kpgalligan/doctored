```json screen
{"id": "depth-blur", "title": "The Depth Blur dialog", "fixture": "sky", "window": "editor", "needs": ["models"],
 "steps": [{"ui": "open_sheet", "args": {"sheet": "depth_blur"}},
           {"ui": "wait", "args": {"seconds": 4}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "sheet", "of": "sheet"}]}
```

## window

The editor behind the Depth Blur dialog, dimmed while the dialog is open. The canvas shows the
dialog's preview at its starting settings, with the focus on the nearest depth: the cloud pattern of
the far sky is blurred into soft grey shapes, while the near rooftops stay dark silhouettes.

## sheet

The Depth Blur dialog. A hint says to pick a focal point on the image, or let Focus Subject keep the
subject sharp. Focus Subject is greyed, since the photo has no subject to find. A thumbnail of the
photo labelled Blur Center holds a cross at its middle. Sliders follow: Focal Distance 0, Focal Range
20, Blur Strength 50, Haze 0, Warmth 0, Brightness 0, Saturation 0 and Grain 0. Output Depth Map
Only is off and Output is set to Current Layer, above a one undo step footnote, Cancel and Apply.
