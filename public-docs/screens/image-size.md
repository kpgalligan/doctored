```json screen
{"id": "image-size", "title": "The Image Size dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "set_resolution", "args": {"ppi_x": 300}},
           {"ui": "open_sheet", "args": {"sheet": "image_size"}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Image size dialog for the 2048 × 1366 pixel photo, set to 300 pixels per inch. A hint says that
with Resample off the pixels stay unchanged and only the print resolution and size change. Current
size reads 2048 × 1366 px; Dimensions is Pixels, with Width 2,048 and Height 1,366, a Result size of
2048 × 1366 px and Lock aspect ratio on. Resolution is 300 pixels/inch, which gives a Print size of
6.827 × 4.553 inches, and Resample is on with the Lanczos3 filter. A max 100 MP note sits at the
bottom left, beside Cancel and Apply.
