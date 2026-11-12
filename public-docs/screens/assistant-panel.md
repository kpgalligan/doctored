```json screen
{"id": "assistant-panel", "title": "The Assistant tab", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "show_panel_tab", "args": {"tab": "assistant"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "panel", "of": "view", "view": "assistant_panel"}]}
```

## window

The editor with the Assistant tab chosen in the right panel, beside the rooftops photo. The tab holds
three small buttons at its top right, an empty conversation, and at its foot a message field reading
Ask about or edit this image… with Send beside it.

## panel

The Assistant tab under the tab row (Layers, Channels, Assistant, Info, Navigator, Paths). At the top
right are three buttons: Context usage, Clear the conversation and More. The conversation below is
empty, and at the bottom a message field reading Ask about or edit this image… sits beside Send.
