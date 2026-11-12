```json screen
{"id": "contact-sheet", "title": "The Contact Sheet dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "duplicate_layer", "args": {}},
           {"tool": "duplicate_layer", "args": {}},
           {"tool": "duplicate_layer", "args": {}},
           {"tool": "duplicate_layer", "args": {}},
           {"tool": "duplicate_layer", "args": {}},
           {"ui": "open_sheet", "args": {"sheet": "contact_sheet"}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Contact Sheet dialog for a document with six layers. A hint says cells are 8-bit sRGB and are
never upscaled, that a transparent item shows the background colour, and that the sources are not
changed. Items is All layers; Columns 4, Cell width 256 and Cell height 256; Gutter 16, Caption strip
24 and Background #ffffff; Caption each cell with its layer name is on; Output is New document, with
the Preset menu greyed. The summary reads 6 cells · 2 rows · 1104×608 px, above Cancel and Build
Sheet.
