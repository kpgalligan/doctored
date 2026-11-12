```json screen
{"id": "color-settings", "title": "Color Settings", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "run_modal_card", "args": {"card": "color_settings"}}],
 "captures": [{"name": "card", "of": "card"}]}
```

The Color Settings window. A hint says it holds the working spaces new documents and conversions
use, what opening a file does with its profile, and every conversion's default intent and black point
compensation, and that changes affect future opens and conversions, never an open document's pixels.
Under Working Spaces, RGB is sRGB, CMYK is Automatic — U.S. Web Coated (SWOP) v2 and Gray is Dot Gain
20%. Under Color Management, the RGB policy is Convert to Working and the CMYK policy is Preserve
Embedded Profiles, with Ask When Pasting off. Under Conversion Options, Engine reads Doctored ICC,
Intent is Relative Colorimetric and Use Black Point Compensation is checked. Cancel and OK close the
window.
