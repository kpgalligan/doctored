```json screen
{"id": "sky-replacement", "title": "The Sky Replacement dialog", "fixture": "sky", "window": "editor", "needs": ["models"],
 "steps": [{"ui": "open_sheet", "args": {"sheet": "sky_replacement"}},
           {"ui": "wait", "args": {"seconds": 4}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Sky Replacement dialog. A hint says to pick a sky and drag the preview to move it. Sky is set to
Kloofendal 38d Partly Cloudy, and the large preview shows the rooftops under that sky, a pale blue
with scattered white clouds. Sliders follow: Shift Edge 0, Fade Edge 3, Brightness 0, Temperature 0,
Scale 100, Lighting Adjustment 50, Color Adjustment 50 and Edge Lighting 0. Flip is off and Lighting
Mode is Multiply, Decontaminate Edge is checked, and Output To is New Layers. A one undo step
footnote sits beside Cancel and OK.
