```json screen
{"id": "develop-presence-grain", "title": "Develop RAW: Presence, Grain and 100% detail", "fixture": "z6-nef",
 "window": "develop",
 "steps": [{"ui": "set_control", "args": {"control": "raw-develop-texture", "value": 25}},
           {"ui": "set_control", "args": {"control": "raw-develop-clarity", "value": 35}},
           {"ui": "set_control", "args": {"control": "raw-develop-grain-amount", "value": 30}},
           {"ui": "set_control", "args": {"control": "raw-develop-zoom", "value": "100%"}},
           {"ui": "develop_scroll", "args": {"row": "Sharpness:"}},
           {"ui": "develop_wait_ready", "args": {}}],
 "captures": [{"name": "window", "of": "window"}]}
```

The Develop RAW window with the preview at 100% on the toy monkey's fur beside a point of the
origami star, close enough to judge the added texture and grain. The line under the X and Y
sliders reads 100%, x 2512, y 1559, 1024 × 906 native pixels. The controls column shows Sharpness,
Detail, Luminance noise and Colour noise, then Presence with Texture +25, Clarity +35 and Dehaze
+0, Grain with Amount 30, Size 1.00 px and Roughness 50, Enable Profile Corrections with the lens
the file names, and Recover clipped highlights.
