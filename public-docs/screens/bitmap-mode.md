```json screen
{"id": "bitmap-mode", "title": "The Bitmap dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "convert_mode", "args": {"mode": "gray"}},
           {"ui": "open_sheet", "args": {"sheet": "bitmap"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Bitmap dialog on an 8-bit Grayscale document. A hint says it converts the image to one layer of
black and white pixels, and that the 1-bit plane is kept in the document. Input reads 72 Pixels/Inch,
and Output is 72 with a Pixels/Inch menu. Method is Diffusion Dither, so the Frequency (53
Lines/Inch), Angle (45°), Shape (Round) and Pattern (Checkerboard) rows are greyed. A note below
describes each method and says a different Output resolution resamples the gray first, above a one
undo step footnote, Cancel and Apply.
