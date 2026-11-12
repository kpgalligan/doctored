```json screen
{"id": "crop-past-edge", "title": "A crop box past the image edge", "fixture": "sky", "window": "editor",
 "steps": [{"ui": "select_tool", "args": {"tool": "crop"}},
           {"ui": "set_crop_box", "args": {"rect": [-205, -137, 2458, 1640]}},
           {"ui": "zoom_to_rect", "args": {"rect": [-307, -205, 2662, 1776]}}],
 "captures": [{"name": "window", "of": "window"}]}
```

The Crop tool with a box drawn larger than the photo. The box's handles and thirds grid extend past
every edge of the image, the area between the image and the box is shaded, and the options bar shows
the box as 2458 by 1640, larger than the 2048 × 1366 image.
