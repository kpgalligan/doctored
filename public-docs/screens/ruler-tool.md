```json screen
{"id": "ruler-tool", "title": "The Ruler tool measuring a roof line", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "select_tool", "args": {"tool": "ruler"}},
           {"ui": "set_ruler_line", "args": {"from": [573, 906], "to": [988, 883]}},
           {"ui": "zoom_to_rect", "args": {"rect": [520, 700, 520, 260]}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "options", "of": "view", "view": "options_bar"}]}
```

## window

A close view, at 175%, of the rooftops under the clouds with the Ruler tool chosen. A measuring line
with a cross at each end runs along a long roof edge, from a gable at the left to the slope of a
taller building at the right, rising gently to the right. The options bar above reads the line's
measurements.

## options

The Ruler tool's options bar: X 573.0 and Y 906.0 for the start point, W 415.0 and H -23.0 for the
run to the end point, A 3.2° for the angle and L 415.6 for the length, then the Straighten Layer and
Clear buttons, with More at the right end.
