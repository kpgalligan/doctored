```json screen
{"id": "super-zoom", "title": "The Super Zoom dialog", "fixture": "sky", "window": "editor", "needs": ["models"],
 "steps": [{"tool": "select_rect", "args": {"x": 994, "y": 750, "width": 100, "height": 100}},
           {"ui": "open_sheet", "args": {"sheet": "super_zoom"}},
           {"ui": "wait", "args": {"seconds": 5}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Super Zoom dialog on a selection around the small chimney. A hint says it enlarges the layer into
a new document and that the preview shows the result at 100 %. The preview shows the chimney and its
roof against the clouds enlarged two times, with scroll bars to pan the enlarged picture. Zoom is set
to ×2, beside ×4; Noise Reduction and Sharpen are at 0; Enhance Image Details is checked and Remove
JPEG Artifacts is off. A new document footnote sits beside Cancel and OK.
