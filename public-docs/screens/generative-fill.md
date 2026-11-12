```json screen
{"id": "generative-fill", "title": "The Generative Fill dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "select_rect", "args": {"x": 1200, "y": 700, "width": 500, "height": 400}},
           {"ui": "open_sheet", "args": {"sheet": "generative_fill"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Generative Fill dialog, opened on a selection. A hint says it sends the area around the selection
to the provider with your own API key, billed to you, and puts the answer on a new layer masked by
the selection, keeping three variations to switch between in Layer ▸ Generative Variations…, and that
pixels outside the selection do not change. The empty Prompt field shows its placeholder, "Describe
what to add, or leave em", cut off mid-word at the field's edge, and Provider is OpenAI. Cancel and
Generate close it.
