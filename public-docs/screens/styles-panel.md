```json screen
{"id": "styles-panel", "title": "The Styles panel", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_shape_layer", "args": {"kind": "rect", "x": 524, "y": 383, "w": 1000, "h": 600,
            "fill": "#f6f1e7", "radius": 48}},
           {"tool": "set_layer_style", "args": {"layer": 1, "style": {"effects": [
            {"type": "drop_shadow", "distance": 12, "size": 30, "opacity": 0.4},
            {"type": "stroke", "size": 4, "position": "inside", "color": "#1d3557"}]}}},
           {"tool": "save_style_preset", "args": {"layer": 1, "name": "Client card", "group": "Client",
            "replace": true}},
           {"tool": "save_style_preset", "args": {"layer": 1, "name": "Client card with blending",
            "group": "Client", "include_blending": true, "replace": true}},
           {"ui": "show_window", "args": {"window": "styles"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "panel", "of": "panel", "panel": "styles"}]}
```

The Styles panel. Under Default Styles, a grid of twelve swatches previews the built-in styles,
each on a rounded square: soft and lifted drop shadows, a thin dark frame, inset and pressed
bevels, a green neon edge, a warm orange glow, an embossed edge, a brushed-metal finish, a glossy
blue fill, a thick white outline and an orange-to-red gradient. Under Client, two swatches preview the
styles saved from a card with a soft shadow and a navy inner stroke. New Style…, Import Styles…
and Export Styles… run along the bottom.
