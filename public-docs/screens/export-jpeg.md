```json screen
{"id": "export-jpeg", "title": "The Export sheet with JPEG", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "open_sheet", "args": {"sheet": "export"}},
           {"ui": "set_control", "args": {"control": "export.format", "value": "JPEG"}},
           {"ui": "wait", "args": {"seconds": 2}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The Export sheet with JPEG chosen. A hint says the preview is decoded from the bytes that will be
written, so the file is exactly what you see, and that the document is not changed. On the left is
the preview of the photo with a status line under it: 2048×1366 px · 8-bit · 269 KB encoded. On the
right, in order: Format JPEG; JPEG quality 90; Size Original size, with an empty Value field and
Resample Lanczos3 greyed; Profile Document profile, with Intent Relative Colorimetric and Black point
compensation greyed; Output sharpening off, with its Method, Radius, Amount and Threshold greyed;
Embed colour profile on; Strip capture metadata off; Metadata policy Custom; Preset Custom; then Save
Preset… and Manage…, above Cancel and Export….
