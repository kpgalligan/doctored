```json screen
{"id": "adjustment-layers-dodge-burn", "title": "Curves adjustment layers for dodge and burn", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_adjustment_layer", "args": {"op": "curves", "name": "Dodge", "params": {"rgb": [[0, 0], [128, 172], [255, 255]]}}},
           {"tool": "add_layer_mask", "args": {"layer": 1, "kind": "hide_all"}},
           {"tool": "brush_stroke", "args": {"layer": 1, "target": "mask", "color": "#ffffff", "size": 560, "hardness": 0, "points": [[240, 300], [520, 420], [800, 340]]}},
           {"tool": "add_adjustment_layer", "args": {"op": "curves", "name": "Burn", "params": {"rgb": [[0, 0], [128, 96], [255, 232]]}}},
           {"tool": "add_layer_mask", "args": {"layer": 2, "kind": "hide_all"}},
           {"tool": "brush_stroke", "args": {"layer": 2, "target": "mask", "color": "#ffffff", "size": 760, "hardness": 0, "points": [[1050, 580]]}},
           {"tool": "add_adjustment_layer", "args": {"op": "black_and_white", "name": "B&W helper"}},
           {"tool": "set_layer_properties", "args": {"index": 3, "visible": false}}],
 "captures": [{"name": "panel", "of": "view", "view": "layers_panel"},
              {"name": "window", "of": "window"}]}
```

## panel

The Layers panel with four layers. At the top, B&W helper, a Black & White adjustment layer, is
hidden: its eye is crossed out, and its row is the chosen one. Below it are two Curves adjustment
layers, Burn and Dodge, each with a black mask that holds one soft white patch: Burn's is a round
spot in the middle, Dodge's a smudge at the upper left. Background, the photo, is at the bottom,
and the footer counts 4 layers.

## window

The editor with the dodge-and-burn stack over a photo of clouds above dark rooftops. The deep blue
sky at the upper left is lifted lighter where Dodge's mask is white, and the bright sun in the
middle is toned down to a soft grey glow where Burn's mask is white; the rest of the photo is
unchanged. The Layers panel on the right lists B&W helper, hidden, then Burn, Dodge and Background.
