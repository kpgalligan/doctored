```json screen
{"id": "show-clipping", "title": "Show Clipping over a backlit photo", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "set_view_chrome", "args": {"option": "show_clipping", "value": true}},
           {"ui": "show_panel_tab", "args": {"tab": "info"}},
           {"ui": "wait", "args": {"seconds": 2}}],
 "captures": [{"name": "window", "of": "window"}]}
```

The editor with View ▸ Show Clipping on. The black rooftops, and a few specks of the darkest sky at
the top left, are painted solid blue where the photo clips to black, and a ring of red specks
around the sun marks the pixels that clip to white; the very centre of the sun is not marked,
because there the photo stops just short of white. The Info tab on the right shows the photo's
histogram with a clipping wedge at each end and, under it, the line counting the clipped shadow
and highlight pixels.
