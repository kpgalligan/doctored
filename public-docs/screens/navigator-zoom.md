```json screen
{"id": "navigator-zoom", "title": "Zoomed in, with the Navigator and the Zoom tool", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "select_tool", "args": {"tool": "zoom"}},
           {"tool": "set_view", "args": {"zoom": 2, "center": {"x": 1055, "y": 700}}},
           {"ui": "show_panel_tab", "args": {"tab": "navigator"}},
           {"ui": "wait", "args": {"seconds": 1}}],
 "captures": [{"name": "window", "of": "window"},
              {"name": "navigator", "of": "view", "view": "navigator_panel"},
              {"name": "options", "of": "view", "view": "options_bar"}]}
```

## window

The editor zoomed to 200% with the Zoom tool chosen. The canvas shows the glow of the sun in the
clouds, with a chimney and the dark slope of a roof at the bottom, and the zoom pill at the lower
left reads 200%. The options bar holds Fit, 100% and Fill buttons, a Zoom field at 200%, Scrubby
zoom, off, Rulers and Guides, both on, and the pixel grid pop-up set to Auto. On the right, the
Navigator tab is open, with a red-orange rectangle on its thumbnail marking the part of the photo
in view.

## navigator

The Navigator tab: a thumbnail of the whole photo, dark rooftops against a cloudy sky with the sun
near the middle, and a red-orange rectangle around the sun and the roof below it marking the part
shown in the window. Under the thumbnail are a zoom slider, a field reading 200% and a Fit button,
then the document's size, 2048 × 1366 px.

## options

The Zoom tool's options bar: the tool's icon and name, Fit, 100% and Fill buttons, a Zoom field
reading 200% with a chevron, the Scrubby zoom checkbox, off, the Rulers and Guides checkboxes, both
on, a pop-up for the pixel grid set to Auto, and More at the right end.
