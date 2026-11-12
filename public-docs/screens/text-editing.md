```json screen
{"id": "text-editing", "title": "Re-editing a text layer on the canvas", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_text_layer", "args": {"text": "Rooftops under a mackerel sky", "font": "Avenir Next",
            "size": 120, "color": "#FFFFFF", "x": 160, "y": 990, "wrap_width": 1150, "tracking": 10}},
           {"ui": "reopen_text", "args": {"layer": 1}},
           {"ui": "wait", "args": {"seconds": 0.5}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "bar", "of": "view", "view": "options_bar"}]}
```

## window

A text layer reopened for editing on the canvas: white text in Avenir Next, Rooftops under a
mackerel sky, wraps onto two lines inside a dashed box over the dark rooftops. The options bar
shows the Type tool's settings taken from the layer, and the Layers panel lists the text layer,
Rooftops under a mac…, above Background.

## bar

The Type tool's options bar for the reopened layer: font Avenir Next, weight Regular, Italic,
Underline and Strikethrough buttons, Size 120 pt, Tracking 10, left alignment chosen among left,
centre and right, a white colour well, Leading 0 pt and Baseline 0, with More at the right end.
