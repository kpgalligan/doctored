```json screen
{"id": "cmyk-channels", "title": "The Channels panel of a CMYK document", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "convert_mode", "args": {"mode": "cmyk"}},
           {"ui": "show_panel_tab", "args": {"tab": "channels"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "panel", "of": "view", "view": "channels_panel"}]}
```

The Channels tab of a CMYK document converted from the rooftops photo, under the tab row (Layers,
Channels, Assistant, Info, Navigator, Paths). The CMYK composite row is selected, its thumbnail in
colour. Cyan, Magenta, Yellow and Black follow, each marked active layer's ink, with a grey
thumbnail of that plate: the more ink, the darker, so the rooftops are dark on every plate and the
sun is white. Every row has its eye on. The bar at the bottom holds the channel buttons and reads
0 channels.
