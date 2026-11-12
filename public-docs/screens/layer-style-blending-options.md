```json screen
{"id": "layer-style-blending-options", "title": "Layer Style: Blending Options with Blend If", "fixture": "sky",
 "window": "editor",
 "steps": [{"tool": "new_layer", "args": {"name": "Warm wash"}},
           {"tool": "fill_selection", "args": {"layer": 1, "color": "#ff8a3d"}},
           {"tool": "set_layer_properties", "args": {"index": 1, "blend_mode": "Multiply"}},
           {"tool": "set_layer_style", "args": {"layer": 1, "style": {"fill_opacity": 0.8, "blend_if": {
            "channel": "gray", "this_layer": [0, 0, 255, 255], "underlying": [90, 160, 255, 255]}}}},
           {"ui": "open_layer_style", "args": {"effect": "blending_options"}},
           {"ui": "wait", "args": {"seconds": 1.5}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Layer Style dialog open on Blending Options, selected at the top of the list down the left.
Below it the ten effects are listed, all unchecked, and the five that can be added more than once
carry a + button. A hint says effects render from the layer's shape every time the canvas
composites, and that fill opacity scales the pixels but not the effects. On the right, Fill
Opacity is at 80%, Knockout is None and Blend Interior Effects as Group is unchecked. Under Blend
If, Channel is Gray. This Layer's ramp keeps its handles at the ends, 0 / 0 and 255 / 255, while
Underlying's black handle is split in two, reading 90 / 160: the layer is hidden over darker
underlying tones and fades in between those two levels. A note explains the ranges and that
Option-dragging a joined handle splits it. Reset sits at the bottom left beside the words one undo
step, with Cancel and Apply at the right.
