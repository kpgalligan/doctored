```json screen
{"id": "black-and-white", "title": "The Black & White dialog", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "black_and_white"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Black & White dialog. Under a Preset menu reading Default are six colour sliders at their
starting weights: Reds 40%, Yellows 60%, Greens 40%, Cyans 60%, Blues 20% and Magentas 80%. Below
them are a Tint checkbox, off, and a Tint color swatch, then a paragraph explaining that each
weight sets how much of its hue reaches the grey and that Tint is what colours a neutral pixel. The
note greys stay grey · one undo step sits beside Cancel and Apply.
