```json screen
{"id": "color-range", "title": "The Color Range dialog with a sampled sky colour", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "color_range"}},
           {"ui": "color_range_sample", "args": {"point": [1353, 157]}},
           {"ui": "wait", "args": {"seconds": 2}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Color Range dialog after one click on a blue gap between the clouds. The captured photo is on
the left and the coverage preview on the right, where white is selected: the darker blue sky at the
left and top and many of the gaps between the clouds are white, while the bright clouds around the
sun and the rooftops stay black. The readout says 1 color(s) and gives the sampled RGB values 73,
83, 95, beside Clear Samples. Below are Select set to Sampled Colors and Combine set to Replace,
Tolerance ΔE 20 and Softness 10, Lower and Upper L* (dimmed), the Invert and Localized Color
Clusters checkboxes, Range % and Fuzziness (dimmed), a note that white selects fully and gray
partly, and Cancel and Apply. A long hint at the top explains sampling, the presets and how
Tolerance is measured.
