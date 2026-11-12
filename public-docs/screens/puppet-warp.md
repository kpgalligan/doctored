```json screen
{"id": "puppet-warp", "title": "Puppet Warp with pins and the mesh", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "new_layer", "args": {"name": "Ribbon"}},
           {"tool": "select_rect", "args": {"x": 424, "y": 300, "width": 1200, "height": 120}},
           {"tool": "fill_selection", "args": {"layer": 1, "color": "#F2B134"}},
           {"tool": "deselect", "args": {}},
           {"ui": "puppet_warp", "args": {}},
           {"ui": "puppet_pin", "args": {"from": [500, 360], "to": [500, 360]}},
           {"ui": "puppet_pin", "args": {"from": [1024, 360], "to": [1024, 360]}},
           {"ui": "puppet_pin", "args": {"from": [1550, 360], "to": [1550, 180]}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "bar", "of": "view", "view": "options_bar"}]}
```

## window

Puppet Warp on a layer holding a long amber bar across the sky. A fine triangle mesh covers the bar.
Three pins sit along its middle: the ones at the left end and in the middle, drawn yellow, stay
where they were placed, and the one at the right end, drawn black because it is selected, has been
dragged upward, so the right half of the bar bends up in a smooth curve while the left half stays
straight. The Layers panel lists Ribbon above Background.

## bar

The Puppet Warp options bar: Mode Normal, Density Normal, Expansion 2 px, Show Mesh checked, the two
Pin Depth buttons, Rotate set to Auto at 0.0°, and More at the right end.
