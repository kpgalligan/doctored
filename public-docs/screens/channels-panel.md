```json screen
{"id": "channels-panel", "title": "The Channels panel with alpha channels and a rubylith", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "select_magic_wand", "args": {"x": 1024, "y": 1250, "tolerance": 32}},
           {"tool": "save_selection", "args": {"name": "Rooftops"}},
           {"tool": "new_layer", "args": {"name": "Retouch"}},
           {"tool": "add_layer_mask", "args": {"layer": 1, "kind": "from_selection"}},
           {"tool": "modify_selection", "args": {"operation": "invert"}},
           {"ui": "show_panel_tab", "args": {"tab": "channels"}},
           {"ui": "set_channel_target", "args": {"name": "Rooftops"}},
           {"tool": "save_selection", "args": {"name": "Sky"}},
           {"tool": "deselect", "args": {}},
           {"ui": "set_alpha_channel_visibility", "args": {"rgb": true, "channels": ["Rooftops"]}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "panel", "of": "view", "view": "channels_panel"}]}
```

## window

The editor with the alpha channel Rooftops shown over the photo as a rubylith: a translucent red
covers the sky, where the channel is black, and the rooftops, where it is white, stay clear. The
Channels tab is open on the right with Rooftops as the edit target, and the status bar ends with
Channel: Rooftops.

## panel

The Channels tab, top to bottom: RGB, the composite, then Red, Green and Blue, each with its eye on;
Retouch Mask, the active layer's mask, with its eye off; then the saved alpha channels Rooftops and
Sky, each marked 50% masked. Rooftops is the edit target: its row is selected, its name is blue and
bold, a blue ring surrounds its thumbnail of a black sky over white rooftops, and its eye is on.
Sky's thumbnail is the reverse, and its eye is off. Along the bottom are buttons to load a channel
as a selection, save the selection as a channel (dimmed), add a channel and delete one, and the
count reads 2 channels.
