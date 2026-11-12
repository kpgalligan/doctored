```json screen
{"id": "redevelop-local-masks", "title": "Redevelop RAW: the Local Masks tab", "fixture": "z6-nef", "window": "editor_raw",
 "steps": [{"tool": "edit_raw_development", "args": {"layer": 0, "operations": [{"op": "create", "id": "globe",
            "name": "Globe", "mask": {"op": "radial", "center": [4350, 2470], "radius": [900, 880], "hardness": 0.5},
            "feather": 40, "tone": {"exposure": 0.6, "saturation": 30}}]}},
           {"ui": "run_modal_card", "args": {"card": "redevelop_raw"}},
           {"ui": "set_control", "args": {"control": "raw-develop-section", "value": 1}},
           {"ui": "develop_wait_ready", "args": {}}],
 "captures": [{"name": "card", "of": "card"}]}
```

Develop RAW opened from Redevelop RAW on a camera RAW layer, on its Local Masks tab. In the
preview the red overlay of the chosen local tints the globe. The sidebar's switch reads Global
(final) and Local Masks (canonical, before lens), with Local Masks chosen; below a note on canonical
masks are the local menu reading Globe, the New Radial, New Linear, New Empty, New Range, New
Subject, New People and New Sky buttons, and the local's name, Globe. The local's rows follow:
Enabled with Apply this local checked, Density 1.00, Feather px 40.00, Camera exposure 0.00, White
balance Inherit Global with Temperature and Tint greyed out, a note on how tone is applied inside
the mask, and the tone sliders Exposure EV 0.60, Contrast, Highlights, Shadows, Whites and Blacks.
The status line reads Native development ready, Apply uses this exact result, and Reset, Cancel
and Apply run along the bottom.
