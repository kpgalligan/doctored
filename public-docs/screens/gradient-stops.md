```json screen
{"id": "gradient-stops", "title": "The Gradient Stops dialog", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "select_tool", "args": {"tool": "gradient"}},
           {"ui": "set_tool_options", "args": {"options": "gradient", "values": {"smoothness": 0.8, "stops": [
            {"position": 0, "color": "#e8a33d", "opacity": 1},
            {"position": 0.55, "color": "#ffffff", "opacity": 0.35, "midpoint": 0.35},
            {"position": 1, "color": "#1f3a5f", "opacity": 1}]}}},
           {"ui": "open_sheet", "args": {"sheet": "gradient_stops"}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Gradient Stops dialog. Under a hint on adding, moving and removing stops and moving midpoints,
the ramp runs from orange through a translucent white, where a checkerboard shows through, to dark
blue, with opacity midpoint diamonds above it. The stop bar below holds three stops, the orange
one selected, with colour midpoint diamonds between them; the second sits closer to the white
stop. The selected stop's Position 0%, Opacity 100% and Color, an orange well, follow, with the
Midpoint field empty until a diamond is chosen, then Smoothness 80%, and Use Foreground →
Background, Cancel and Apply.
