```json screen
{"id": "channel-mixer", "title": "The Channel Mixer dialog", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "channel_mixer"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Channel Mixer dialog, editing the Red output. Preset reads Default, Output reads Red and
Monochrome is unchecked. The Red slider is at 100% and Green and Blue are at 0%, a Total readout
shows +100%, and Constant is at 0%. A paragraph explains that each output channel is a weighted sum
of the source channels plus a constant, and that a total above 100% can clip the output. The note
total is advisory · one undo step sits beside Cancel and Apply.
