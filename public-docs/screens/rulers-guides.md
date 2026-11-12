```json screen
{"id": "rulers-guides", "title": "Rulers and guides", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_guide", "args": {"orientation": "vertical", "position": 683}},
           {"tool": "add_guide", "args": {"orientation": "vertical", "position": 1365}},
           {"tool": "add_guide", "args": {"orientation": "horizontal", "position": 455}},
           {"tool": "add_guide", "args": {"orientation": "horizontal", "position": 911}},
           {"ui": "move_cursor", "args": {"point": [1702, 1093]}}],
 "captures": [{"name": "window", "of": "window"}]}
```

The editor with rulers along the top and left edges, labelled in pixels every 200, and four cyan
guides dividing the photo into thirds, two across and two down. A small wedge marks the corner box
where the two rulers meet, and a thin line on each ruler, at 1700 across and near 1100 down, shows
where the pointer is.
