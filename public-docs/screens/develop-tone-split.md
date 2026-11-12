```json screen
{"id": "develop-tone-split", "title": "Develop RAW: tone controls with a Split comparison", "fixture": "z6-nef",
 "window": "develop",
 "steps": [{"ui": "set_control", "args": {"control": "Exposure:", "value": 0.7}},
           {"ui": "set_control", "args": {"control": "White balance:", "value": "Custom"}},
           {"ui": "set_control", "args": {"control": "Temperature:", "value": 5600}},
           {"ui": "set_control", "args": {"control": "Tint:", "value": 12}},
           {"ui": "set_control", "args": {"control": "raw-develop-comparison", "value": "Split"}},
           {"ui": "develop_scroll", "args": {"row": "Exposure:"}},
           {"ui": "develop_wait_ready", "args": {}}],
 "captures": [{"name": "window", "of": "window"}]}
```

The Develop RAW window for a Nikon Z 6 file with the preview split down the middle: Before on the
left and, on the right, an After that is brighter and much warmer, the white curtain turned yellow.
Split is chosen among After, Before and Split above the preview, beside Fit, 100% and Clipping. The
controls column is scrolled to the tone controls: Exposure +0.70 EV, White balance Custom with
Temperature 5600 K and Tint 12, then Tone curve, Shadows, Local contrast, Sharpness, Detail,
Luminance noise and Colour noise, followed by the Presence and Grain groups, Enable Profile
Corrections and Recover clipped highlights. Under the preview are the X and Y position sliders, an
RGB histogram and the status line Native 16-bit preview ready; Reset, Cancel and Open run along the
bottom.
