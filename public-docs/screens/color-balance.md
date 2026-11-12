```json screen
{"id": "color-balance", "title": "The Color Balance dialog", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "color_balance"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Color Balance dialog. Preset reads Default and Tone reads Midtones, above three sliders at
0%: Cyan / Red, Magenta / Green and Yellow / Blue. Preserve Luminosity is checked. A note that
begins Preserve renormalises each tone's sits beside Cancel and Apply.
