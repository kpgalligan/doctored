```json screen
{"id": "batch", "title": "The Batch window", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "save_action", "args": {"name": "Web Export Prep", "overwrite": true, "steps": [
            {"tool": "image_size", "arguments": {"width": 1600, "height": 1067}},
            {"tool": "apply_filter", "arguments": {"filter": "unsharp_mask", "radius": 1.0, "amount": 0.8}}]}},
           {"ui": "run_modal_card", "args": {"card": "batch"}}],
 "captures": [{"name": "card", "of": "card"}]}
```

The Batch window. A hint says it runs the action on every image in the input folder, its top level
only, not sub-folders, and writes each result into the output folder. Action is Web Export Prep, and
RAW develop shows a collapsed File defaults row. Input folder and Output folder each have a Choose…
button and read None chosen. Format is PNG, with a greyed Quality slider at 90, Embed colour profile
checked and Strip metadata off, above a note that every file is written flattened in one of those
formats. Deliver with reads Flat copy (format belo…, cut short, and the Metadata policy menu is
greyed at Preset's own, above a note on what an export preset supplies. Name as holds {name}, with
a note on the {name} and {n} tokens. Overwrite existing files is off and Stop the whole run is on,
above a note on what a failed step does. Cancel and Run close the window.
