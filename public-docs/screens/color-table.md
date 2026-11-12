```json screen
{"id": "color-table", "title": "The Color Table dialog", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "convert_mode", "args": {"mode": "indexed", "palette": "adaptive", "colors": 32}},
           {"ui": "open_sheet", "args": {"sheet": "color_table"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Color Table dialog of an Indexed Color document converted with a 32-colour Adaptive palette. A
hint says it edits the document's palette: every pixel keeps its index and takes its entry's new
colour, and Load and Save read and write Adobe Color Table (.act) files. Two rows of sixteen swatches
in black, dark slate blues, greys and near whites, the first one, black, selected. Below are a Color
well showing that black entry, Transparent set to None, Load… and Save…, a line reading 32
colours, a one undo step footnote, Cancel and OK.
