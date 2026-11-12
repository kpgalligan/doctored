```json screen
{"id": "indexed-color", "title": "The Indexed Color dialog", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "indexed_color"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Indexed Color dialog on an 8-bit RGB photo. A hint says it converts the image to one layer of at
most 256 palette colours, and that the palette, its transparent index and every pixel's index are kept
in the document. Palette is Uniform, with Table… greyed beside it; Colors is 256; Forced is None,
with Forced Table… greyed; Transparency is checked; Matte is None; Dither is Diffusion at an Amount
of 75 %; Preserve Exact Colors is off. A long note below describes each palette and dither method,
above a one undo step footnote, Cancel and Apply.
