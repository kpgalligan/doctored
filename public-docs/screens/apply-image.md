```json screen
{"id": "apply-image", "title": "The Apply Image dialog", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "apply_image"}},
           {"ui": "choose_popup_item", "args": {"title": "Multiply"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Apply image dialog. The hint says it blends one source onto the current target: the active
layer, one of its colour planes, or an alpha channel. Source is Merged, Channel is RGB, Invert is
unchecked, Blending is set to Multiply and Opacity is 100%, a slider beside its field. The footnote
reads Target: Background, the layer the result lands on, beside Cancel and Apply.
