```json screen
{"id": "artwork-colors", "title": "Layer ▸ Artwork Colors…", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "add_text_layer", "args": {"text": "Rooftops", "font": "Avenir Next", "size": 220,
            "color": "#FFFFFF", "x": 200, "y": 1020}},
           {"ui": "open_sheet", "args": {"sheet": "artwork_colors"}},
           {"ui": "choose_popup_item", "args": {"title": "Character Range"}},
           {"ui": "set_control", "args": {"control": "artworkColor.value0", "value": "0.91"}},
           {"ui": "set_control", "args": {"control": "artworkColor.value1", "value": "0.64"}},
           {"ui": "set_control", "args": {"control": "artworkColor.value2", "value": "0.24"}},
           {"ui": "set_control", "args": {"control": "artworkColor.value5", "value": "4"}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Artwork Colors dialog for a text layer, set to colour part of its text. A hint says the values
are continuous sRGB components from 0 to 1 and that character ranges preserve complete characters,
including emoji. The target menu reads Character Range, and the fields read Red 0.91, Green 0.64,
Blue 0.24, Alpha 1.0, First character (1-based) 1 and Character count 4. A note says the values
stay precise in the editable layer, above Cancel and Apply.
