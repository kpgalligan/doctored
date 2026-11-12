```json screen
{"id": "canvas-size", "title": "The Canvas Size dialog", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "canvas_size"}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Canvas size dialog. A hint says it grows or trims the canvas without scaling, and that layers
keep their pixels and can be revealed again later. Current size reads 2048 × 1366 px, and Width and
Height hold the same numbers. Below them, Anchor is a 3 × 3 grid of buttons: the centre one is
chosen, and arrows on the other eight point the way the canvas grows away from it. The ⌥⌘C shortcut
sits at the bottom left, beside Cancel and Apply.
