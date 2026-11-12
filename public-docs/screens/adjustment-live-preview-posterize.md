```json screen
{"id": "adjustment-live-preview-posterize", "title": "Posterize previewing on the canvas", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "posterize"}},
           {"ui": "wait", "args": {"seconds": 2}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "sheet", "of": "sheet"}]}
```

## window

The Posterize dialog open over the editor, which is dimmed behind it. The photo on the canvas
already shows what Apply will commit: the cloudy sky has broken into a few flat colours with hard
edges between them, and the rooftops are one flat dark shape. The Layers panel lists a single
layer, Background.

## sheet

The Posterize dialog: one Levels slider set to 4, and the note one undo step beside Cancel and
Apply.
