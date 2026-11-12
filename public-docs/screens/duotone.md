```json screen
{"id": "duotone", "title": "The Duotone Options dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "convert_mode", "args": {"mode": "gray"}},
           {"tool": "convert_mode", "args": {"mode": "duotone", "inks": [
            {"name": "Black", "lab": [16, 0, 0],
             "curve": [0, null, null, null, null, null, 50, null, null, null, null, null, 100]},
            {"name": "Warm Brown", "lab": [48, 22, 38],
             "curve": [0, null, null, null, null, null, 35, null, null, null, null, null, 85]}]}},
           {"ui": "open_sheet", "args": {"sheet": "duotone"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Duotone Options dialog reopened on a Duotone document. A hint says it prints the gray channel
through one to four inks, each ink's curve mapping the gray to its coverage and the inks overprinting
on paper, while the pixels stay the Grayscale document's. Type is Duotone. Under a caption reading
Name, then L*, a*, b* (D50), Ink 1 is Black at 16, 0, 0 and Ink 2 is Warm Brown at 48, 22, 38, each
row ending in a swatch of the ink and a Curve… button. Overprint Colors…, Load…, Save… and a checked
Preview sit above a one undo step footnote, Cancel and Apply.
