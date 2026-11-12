```json screen
{"id": "new-document-card", "title": "New Document with a saved preset", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "save_document_preset", "args": {"name": "Letter 300 ppi CMYK", "width": 8.5, "height": 11,
            "unit": "in", "ppi": 300, "color_mode": "cmyk", "depth": 8, "background": "white", "replace": true}},
           {"ui": "run_modal_card", "args": {"card": "new_document"}},
           {"ui": "choose_popup_item", "args": {"title": "Letter 300 ppi CMYK"}}],
 "captures": [{"name": "card", "of": "card"}]}
```

The New Document dialog with a saved preset, Letter 300 ppi CMYK, chosen. Under a hint about
resolution and about 32-bit and CMYK documents, the Preset menu sits above Save Preset… and Delete
Preset, then an empty Name field, Width 8.5 and Height 11 in Inches, Resolution 300 ppi, Color Mode
CMYK Color, Profile Working CMYK — U.S. Web Coated (SWOP) v2, Depth 8 bits/channel and Background
White. The size in pixels, 2550 × 3300 px, sits below, with Cancel and Create at the bottom.
