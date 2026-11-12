```json screen
{"id": "convert-to-profile", "title": "The Convert to Profile dialog", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "convert_to_profile"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Convert to profile dialog on an sRGB photo. A hint says Convert transforms every layer's pixels
into another space, so each layer keeps its appearance while its numbers change, and that layer
masks, alpha channels and layer descriptions are coverage, not colour, and are left exactly as they
are. Destination reads sRGB, Intent is Relative Colorimetric, Black point compensation is off, and
Adjustment layers is set to Keep parameters. A note says the document is already in sRGB
IEC61966-2.1, so there is nothing to convert, and Apply is greyed beside Cancel, with a one undo step
footnote.
