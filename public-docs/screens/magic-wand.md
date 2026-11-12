```json screen
{"id": "magic-wand", "title": "The Magic Wand and its options bar", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "select_tool", "args": {"tool": "wand"}},
           {"tool": "select_magic_wand", "args": {"x": 1024, "y": 1250, "tolerance": 32}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "bar", "of": "view", "view": "options_bar"}]}
```

## window

The editor after one Magic Wand click in the dark foreground. Marching ants run along the whole
roofline, around the chimneys and up the building on the right, and along the edges of the picture:
the rooftops and walls are selected as one connected shape, and the sky outside the selection is
dimmed. The Magic Wand is chosen in the tool rail, and the status bar reads Selection: 2048 × 1366
px.

## bar

The Magic Wand's options bar: the four selection mode buttons (new, add, subtract and intersect)
with the first chosen, Feather 0 px, Sample Size set to Point Sample, Tolerance 32, Contiguous
checked, Sample set to All layers, and More at the right end.
