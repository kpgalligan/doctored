```json screen
{"id": "content-aware-fill", "title": "The Content-Aware Fill workspace", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "set_tool_options", "args": {"options": "contentAwareFill", "values": {"sample": "all"}}},
           {"tool": "select_rect", "args": {"x": 1615, "y": 600, "width": 110, "height": 200}},
           {"ui": "open_sheet", "args": {"sheet": "content_aware_fill"}},
           {"ui": "wait_enabled", "args": {"id": "caf.apply", "seconds": 90}},
           {"ui": "wait", "args": {"seconds": 0.5}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Content-aware fill dialog for a selection around the tall chimney on the right of the photo.
Under a hint on including and excluding source areas and on the size limits are Sample All
Layers, an Ignore adjustment layers checkbox and Output This Layer; Ring (px) 48, Seed 0 and
Prepare Final Result; then the source tool menu reading Exclude Brush, Radius (px) 20, Include All
and Exclude All, and the Source Area, Before and After and the Fit and 100% switches. The large
preview tints the whole photo green, the area the fill may sample, with the selection outlined
around the chimney. The status line reads Ready, Apply uses this exact result, above Cancel and
Apply.
