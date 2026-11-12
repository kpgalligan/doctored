```json screen
{"id": "redevelop-local-tools", "title": "Redevelop RAW: mask tools, range and local actions", "fixture": "z6-nef",
 "window": "editor_raw",
 "steps": [{"tool": "edit_raw_development", "args": {"layer": 0, "operations": [{"op": "create", "id": "globe",
            "name": "Globe", "mask": {"op": "radial", "center": [4350, 2470], "radius": [900, 880], "hardness": 0.5},
            "feather": 40, "range": {"luminance": {"low": 0.2, "high": 0.9, "low_falloff": 0.1, "high_falloff": 0.1}},
            "tone": {"exposure": 0.6, "saturation": 30}}]}},
           {"ui": "run_modal_card", "args": {"card": "redevelop_raw"}},
           {"ui": "set_control", "args": {"control": "raw-develop-section", "value": 1}},
           {"ui": "develop_wait_ready", "args": {}},
           {"ui": "set_control", "args": {"control": "raw-local-tool", "value": "Brush Add"}},
           {"ui": "develop_scroll", "args": {"row": "raw-local-tool"}},
           {"ui": "wait", "args": {"seconds": 0.5}}],
 "captures": [{"name": "card", "of": "card"}]}
```

The Local Masks sidebar of Redevelop RAW scrolled to its end. The local's tone rows end with
Saturation 30, Clarity, Texture, Sharpness and Noise Reduction. Luminance has Limit to a luminance
range checked, with Low 0.20, High 0.90 and both falloffs 0.10, and Invert the luminance range
unchecked. Limit to a sampled colour is off, so its Sampled Lab, Amount ΔE, Falloff ΔE and Invert
rows are greyed out. Mask tool reads Brush Add, with Radius px 24.00, Hardness 0.70 and Flow 1.00,
and Show effective mask overlay is checked. The local's actions follow: Invert, Remap…, Earlier,
Later and Remove, Recompute (greyed out), and Copy Settings and Paste Settings. In the preview the
mask overlay tints the globe.
