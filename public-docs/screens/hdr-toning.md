```json screen
{"id": "hdr-toning", "title": "The HDR Toning dialog on a 32-bit document", "fixture": "sky", "window": "editor",
 "steps": [{"tool": "convert_precision", "args": {"mode": "rgba32", "structure": "preserve"}},
           {"ui": "open_sheet", "args": {"sheet": "hdr_toning"}},
           {"ui": "wait", "args": {"seconds": 4}}],
 "captures": [{"name": "sheet", "of": "sheet"}]}
```

The HDR Toning dialog for a 32-bit document. A paragraph at the top says that it tone maps the
document into an 8- or 16-bit image and flattens it, and what each method does. Preset reads
Default and Method reads Local Adaptation. Under Edge Glow are Radius 16 px, Strength 0.52 and
Smooth Edges, unchecked; under Tone and Detail, Gamma 1.00, Exposure +0.00 and Detail +30%; under
Advanced, Shadow, Highlight and Vibrance at +0% and Saturation at +20%. Toning Curve and Histogram
shows a straight diagonal curve over the image's histogram. Mode reads 16 Bits/Channel, above the
line The 32-bit document becomes 16 bits per channel, and the note flattens · one undo step sits
beside Cancel and OK.
