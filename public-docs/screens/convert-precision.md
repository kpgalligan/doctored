```json screen
{"id": "convert-precision", "title": "The Convert Precision dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "new_layer", "args": {"name": "Retouch"}},
           {"ui": "open_sheet", "args": {"sheet": "convert_precision"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Convert Precision dialog on an 8-bit document with two layers. A hint says Flatten renders the
canvas into one raster, Preserve Layers carries every layer to the target precision, and the previews
are displayed at 8 bits. Mode is To 16-bit, Structure is set to Preserve Layers rather than Flatten,
and Ordered RGB dithering is greyed. A scrolling text lists what the conversion carries, every layer,
group, adjustment, text and shape description, mask, saved channel and style among them, and says
that widening to 16 bits multiplies each 8-bit value by 257. Below it, a Before thumbnail of the
photo sits beside an empty Converted area. Prepare Preview and Cancel sit beside a greyed Apply
Conversion.
