```json screen
{"id": "remove-tool", "title": "The Remove tool with a held stroke", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "select_tool", "args": {"tool": "remove"}},
           {"ui": "set_tool_options", "args": {"options": "removeTool", "values": {"mode": "content_aware",
            "removeAfterEachStroke": false}}},
           {"ui": "set_tool_options", "args": {"options": "paint:remove", "values": {"size": 80}}},
           {"ui": "zoom_to_rect", "args": {"rect": [1430, 470, 540, 360]}},
           {"ui": "hold_remove_stroke", "args": {"points": [[1667, 630], [1667, 700], [1668, 775]], "size": 80}}],
 "captures": [{"name": "window", "of": "window"}]}
```

The Remove tool zoomed to 168% on the tall chimney, with one stroke held over it as a translucent
blue band from above its top down to the roof. The options bar reads Mode Content-Aware, Size 80
px, an unchecked Sample all layers, To Current Layer and an unchecked Remove after each stroke,
then a check mark and a cross that apply or drop the held stroke.
