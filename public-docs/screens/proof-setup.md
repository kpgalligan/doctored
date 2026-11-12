```json screen
{"id": "proof-setup", "title": "The Proof Setup dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "set_proof_setup", "args": {"proof_colors": true}},
           {"ui": "open_sheet", "args": {"sheet": "proof_setup"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Proof setup dialog. A hint says a proof shows what the document would look like on another
medium and changes nothing: the master, every render and every export are untouched, and there is no
undo step. Preset reads Custom, beside Save… and a greyed Delete, above a greyed Default for new
windows. Simulate is U.S. Web Coated (SWOP) v2 and Intent is Relative Colorimetric. Below them,
Black point compensation is checked, and Preserve CMYK Numbers, Simulate paper white and Simulate
black ink are off. Under Show, Proof Colors is checked and Gamut Warning is off, and the Warning
colour well holds a mid grey. A note says the canvas is drawn through U.S. Web Coated (SWOP) v2 at
Relative Colorimetric, which keeps in-gamut colours exactly and clips the rest, white mapping to
white. A view only footnote sits beside Cancel and Apply.
