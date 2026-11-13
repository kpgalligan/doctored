import Foundation

extension AgentServer {
    @inline(never)
    func documentCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let docID = schema.docID
        let index = schema.index
        let profileChoice = schema.profileChoice
        let profilePath = schema.profilePath
        return [
            tool(
                "list_documents",
                "Lists the images open in Doctored with their ids, sizes, and layer counts. Each "
                    + "document's color_mode names its mode and precision its mode and depth: rgba8, "
                    + "rgba16 or rgba32_float on an RGB document, gray8, gray16 or gray32 on a "
                    + "Grayscale or Duotone one, lab8 or lab16, cmyk8 or cmyk16, indexed8, or bitmap1.",
                [:]),
            tool(
                "inspect_psd",
                "Inspects a PSD, a Large Document PSB or a TIFF with Photoshop layers (format says "
                    + "which: PSD, PSB or TIFF) before editing: verified per-file losses, parser observations, "
                    + "general importer limits, embedded RGB profile/PPI, and saved-composite availability. "
                    + "import_precision says which document the file opens as: rgba8, or rgba16 for a 16-bit "
                    + "RGB PSD, which opens as a 16-bit document with either choice, or rgba32 for a "
                    + "32-bit RGB PSD or PSB, which opens as a 32-bit float document with either choice, "
                    + "or for a 32-bit float RGB TIFF with layers in either byte order, whose layers "
                    + "open as a 32-bit float document. "
                    + "Photoshop Invert, Threshold, Posterize, Levels, Curves, Color Balance, "
                    + "Hue/Saturation, Photo Filter, Exposure, Vibrance, Black & White, Channel "
                    + "Mixer, Selective Color and Brightness/Contrast layers are carried as Doctored "
                    + "adjustment layers "
                    + "(parameters mapped, appearance can differ), and a Pattern Fill layer (PtFl), RGB or "
                    + "CMYK, as a Doctored pattern fill layer with its tile from the file's Patt/Pat2 "
                    + "table, and a Solid Color Fill layer (SoCo), RGB or CMYK, as a Doctored solid "
                    + "color fill layer with its colour (one outside sRGB kept as a wide colour), "
                    + "and a Gradient Fill layer (GdFl), RGB or CMYK, as a Doctored gradient fill "
                    + "layer with its stops, style, angle, scale and reverse (settings it cannot hold "
                    + "named as losses); every "
                    + "other adjustment or fill layer "
                    + "carries no pixels and Import Layers omits it with a per-record loss. Empty layers "
                    + "import as empty canvas-sized layers, and both depths keep clipping. "
                    + "ZIP-compressed channels are read at both depths. A layered TIFF reports "
                    + "stored_composite_available false: its flat image is the ordinary TIFF open "
                    + "(open_document without psd_mode). "
                    + "Creates no document. Pass source_fingerprint to open_document as psd_fingerprint.",
                ["path": ["type": "string",
                          "description": "Absolute or ~ path to a PSD, PSB or layered TIFF file."]],
                required: ["path"]),
            tool(
                "open_document",
                "Opens an image file (PNG, JPEG, PSD, PSB, TIFF, BMP, GIF, WebP, HEIC/HEIF, AVIF, "
                    + "JPEG XL, JPEG 2000, OpenEXR, Radiance HDR, PDF, SVG, camera RAW, Doctored "
                    + "(.doctored, and legacy .rz)) in a new "
                    + "editor window and returns "
                    + "its document id. A camera RAW opens as a 16-bit document, or as a 32-bit "
                    + "float document holding its scene-linear development with raw.output_encoding "
                    + "linear_float (Develop > Document depth). "
                    + "An HDR photo — a gain-map HEIC/AVIF, or a PQ/HLG HEIC, AVIF, PNG (cICP) or "
                    + "JPEG XL — opens as a 32-bit float document in linear light (File > Open HDR "
                    + "Photos as HDR, on by default; hdr_photo overrides it for this open): 1.0 is "
                    + "the gain map's SDR base white, or 203 cd/m2 of a PQ/HLG signal, and the "
                    + "reply's hdr_open says {kind, rendition, reference_white}. hdr_photo \"sdr\" "
                    + "opens a gain-map photo as its SDR base (disclosed) and refuses a PQ/HLG still "
                    + "by name (it has no SDR rendition); the original file is not kept in the "
                    + "document. "
                    + "A movie (.mov, .mp4, .m4v: H.264, HEVC hvc1 at 8 or 10 bits 4:2:0, or ProRes "
                    + "4444) opens its first frame as one layer; an HLG or PQ HEVC movie's frame "
                    + "opens as a 32-bit float document by the PQ/HLG photo rule above (hdr_open "
                    + "adds source movie_frame), and hdr_photo \"sdr\" or precision rgba8 refuses it "
                    + "by name. A Dolby Vision movie refuses by name. "
                    + "A multipage TIFF opens each page that matches page 1's depth, sample format, "
                    + "colour model and profile as a layer \"Page n\" at its own size at the "
                    + "canvas's top-left, only Page 1 visible; the canvas, profile and depth are "
                    + "page 1's, and get_document's metadata.metadata_import.warnings names any page "
                    + "left out and why. A multipage TIFF whose page 1 opens as a Grayscale, a Lab "
                    + "Color or a CMYK document opens its pages the same way in that mode. One whose "
                    + "page 1 opens as an Indexed Color or a Bitmap document opens page 1 alone, and "
                    + "those warnings say so. "
                    + "A PDF or SVG is RASTERIZED on the way in: pass page "
                    + "(1-based) and ppi to choose which page and at what resolution, and call "
                    + "inspect_vector first for the page count and sizes. PSD requires "
                    + "inspect_psd first and an explicit layers/composite choice; unsupported color/depth "
                    + "never silently falls back to an 8-bit platform decode. A 16-bit RGB PSD "
                    + "(inspect_psd import_precision \"rgba16\") opens as a 16-bit document, and a 32-bit "
                    + "RGB PSD or PSB, or a 32-bit layered TIFF's layers (\"rgba32\"), as a 32-bit "
                    + "float document. The PSD layers "
                    + "choice carries Photoshop Invert/Threshold/Posterize/Levels/Curves/Color Balance/"
                    + "Hue-Saturation/Photo Filter/Exposure/Vibrance/Black & White/Channel Mixer/"
                    + "Selective Color/Brightness-Contrast layers "
                    + "as adjustment "
                    + "layers, Pattern Fill layers (PtFl, RGB or CMYK) as pattern fill layers, Solid "
                    + "Color Fill layers (SoCo) as solid color fill layers and Gradient Fill layers "
                    + "(GdFl) as gradient fill layers, omits every other "
                    + "pixel-less adjustment/fill layer, imports empty layers empty, and retains "
                    + "clipping at either depth. A PSB is a PSD here. A TIFF with Photoshop layers "
                    + "opens flat, as one layer, unless psd_mode is layers (with its inspect_psd "
                    + "fingerprint), which imports its layers at the file's depth.",
                [
                    "path": ["type": "string", "description": "Absolute or ~ path to the file."],
                    "raw": Self.rawDevelopProperty,
                    "hdr_photo": ["type": "string", "enum": ["hdr", "sdr"], "description": "HDR photos only (gain-map HEIC/AVIF, PQ/HLG stills): hdr opens a 32-bit float document at full headroom, sdr a gain map's SDR base (a PQ/HLG still refuses sdr). Absent = the File > Open HDR Photos as HDR preference (on by default). Revert keeps the rendition a document was opened with."],
                    "precision": ["type": "string", "enum": ["preserve", "rgba8"], "description": "PNG/TIFF16: preserve samples and profile (default), or deliberately open reduced 8-bit pixels. Native typed files always preserve depth. A floating-point TIFF, OpenEXR or Radiance file opens only as 32-bit float and refuses rgba8."],
                    "psd_mode": ["type": "string", "enum": ["layers", "composite"],
                        "description": "PSD, PSB and layered TIFF only: import supported layers or the file's saved composite as one raster layer (a layered TIFF's composite is its ordinary flat open)."],
                    "psd_fingerprint": ["type": "string",
                        "description": "PSD, PSB and layered TIFF only: source_fingerprint returned by inspect_psd. Changed sources refuse."],
                    "page": ["type": "integer",
                        "description": "PDF/SVG only: which page to rasterize, 1-based. Default 1."],
                    "ppi": ["type": "number",
                        "description": "PDF/SVG only: the resolution to rasterize at, 1…30000. Default 300."],
                ],
                required: ["path"]),
            tool(
                "get_document",
                "Full state of one document: canvas size and every layer's name, size, offset, "
                    + "opacity, blend mode, visibility, layer mask (has_mask, mask_enabled), "
                    + "and clipped flag (see set_layer_clipped). STRUCTURE is reported on "
                    + "every row: kind (\"layer\" or \"group\"), depth, parent, and — on a "
                    + "group — children and open. The array stays flat and bottom-first, "
                    + "with a group's children BEFORE it. content_x/y/width/height is the "
                    + "box of actually opaque pixels (what align_layers and "
                    + "distribute_layers act on); offset/width/height is the pixel BUFFER "
                    + "rect, which is usually a different rectangle, and on a group row is "
                    + "the union of its layers' buffers. locks and linked "
                    + "appear when set; a row inside a locked group also reports "
                    + "inherited_locks (the locks an enclosing group imposes that the row's "
                    + "own locks lack) and lock_owner (each of those names mapped to the "
                    + "index of the nearest group holding it, the layer to clear with "
                    + "set_layer_lock); pixel_layer_count counts leaves, layer_count counts "
                    + "entries. A STYLED layer reports a "
                    + "style object — its whole effect stack and blending options as the "
                    + "canonical JSON set_layer_style takes back — and the document reports "
                    + "global_light (angle, altitude). "
                    + "A re-editable TEXT layer also reports a text object (string, font, size, "
                    + "color, alignment, weight, italic, tracking, leading, baseline_shift, "
                    + "underline, strikethrough, box_width, transform, origin) — those are the "
                    + "layers edit_text_layer can change; font_missing: true when its font family "
                    + "is not installed here (replace_missing_fonts) — a SHAPE layer a shape "
                    + "object (kind, "
                    + "w, h, fill, stroke, stroke_width, radius, transform, origin); a layer "
                    + "without those "
                    + "keys is plain pixels. transform is the layer's 2×2 map [a, b, c, d], "
                    + "row-major — (x, y) maps to (a·x + b·y, c·x + d·y) — and "
                    + "origin the exact canvas position of its source's top-left (the text "
                    + "block's, the shape box's; fractional after a transform). "
                    + "An ADJUSTMENT layer reports "
                    + "is_adjustment true plus an adjustment object (op, params) — those are "
                    + "the layers edit_adjustment_layer can change. A document that carries "
                    + "ALPHA CHANNELS (saved selections, never part of the picture) also "
                    + "reports a channels array of {index, name, overlay_color, "
                    + "overlay_opacity, color_indicates} — list_channels returns the same "
                    + "list. A document that carries GUIDES also reports a guides array of "
                    + "{index, orientation, position} — non-printing lines in canvas pixels "
                    + "that tools snap to, never part of the picture; list_guides returns the "
                    + "same list. ruler is always present: origin_x/origin_y are the ruler's "
                    + "zero point in canvas pixels (document state, set with set_ruler_origin, "
                    + "affecting only what the on-screen rulers LABEL), and unit is the "
                    + "app-wide ruler-unit preference, not part of this document. "
                    + "operation_scope lists supported edit targets; edit_target is the UI target. "
                    + "Layer index 0 is the bottom "
                    + "layer; when it came from a PSD, its row reports background: "
                    + "\"background\" (the file's Background layer) or \"ordinary\" (an "
                    + "ordinary layer, which a Deep knockout punches through to "
                    + "transparency); without that key the bottom layer is the Background "
                    + "when it is visible, Normal, at 100 % opacity, unmasked, unstyled and "
                    + "canvas-sized at the origin. "
                    + "Offsets are measured from the canvas top-left corner, y "
                    + "increasing down. A 16-bit or 32-bit float document reports the same "
                    + "keys in the same shape plus typed extras (subtree_start, bounds, "
                    + "dimensions, link, mask, blend_if, offset, description_bytes, "
                    + "blend_evaluation): "
                    + "the same keys, shape and vocabulary "
                    + "(kind layer/group, opacity to two decimals, adjustment {op, params}), "
                    + "the adjustment object also carrying evaluation; each channel row "
                    + "also carries the typed id, dimensions and provenance.",
                ["document_id": docID]),
            tool(
                "render",
                "Renders the flattened canvas (or one layer) as a PNG image so you can see it. "
                    + "Use this to inspect the picture before and after edits. scale enlarges "
                    + "the render by an exact integer factor with nearest-neighbour sampling, "
                    + "so every pixel becomes a crisp scale×scale block (for pixel art).",
                [
                    "document_id": docID, "layer": index,
                    "max_side": [
                        "type": "integer",
                        "description": "Longest output side in px (64-4096, default 1024).",
                    ],
                    "scale": [
                        "type": "integer",
                        "description": "Integer >= 1: enlarges the render exactly scale× with "
                            + "nearest-neighbour sampling (each pixel becomes a scale×scale "
                            + "block). Replaces max_side and is refused together with it; the "
                            + "longest output side may not exceed 4096 px (use region or a "
                            + "smaller scale).",
                    ],
                    "channel": [
                        "type": "string",
                        "description": "Renders ONE plane as a grayscale PNG instead of the "
                            + "colour image: \"red\", \"green\", \"blue\", \"alpha\", "
                            + "\"luma\", or an alpha channel's name (list_channels shows "
                            + "them). With layer it is that layer's plane, canvas-sized "
                            + "with 0 outside the layer's rect; otherwise the flattened "
                            + "composite's. Use mask with a layer to inspect that layer's mask.",
                    ],
                    "region": [
                        "type": "object",
                        "description": "Renders only this canvas window {x, y, width, height} "
                            + "(integers, canvas pixels, inside the canvas) of the flattened "
                            + "canvas, exactly equal to that crop of the full render. Supported "
                            + "for layers, pointwise adjustments and groups (isolated or Pass "
                            + "Through, with their own opacity and mask, nested), with or without "
                            + "layer masks (density and feather included), and clipping masks "
                            + "over them; layer styles on layers (effects, fill opacity, Blend "
                            + "If, knockout, Blend Interior Effects as Group) and on groups "
                            + "(effects included) at 8 and 16 bits, and a Shadows/Highlights "
                            + "with a guide at the top level, inside isolated groups, directly "
                            + "inside Pass Through groups, as a clipping-mask member (of a "
                            + "styled base too) and inside groups with style effects. Any other "
                            + "stack is refused by name (at 8 bits a Shadows/Highlights with a "
                            + "guide clipped to a base whose style effects sit below opacity "
                            + "1 (not Dissolve)). Not with layer or channel. A region or "
                            + "preview render also returns a second text item, "
                            + "{\"regional_plan\": {...}}: the plan it ran, with "
                            + "global_analysis (false, or the whole-layer analyses that read the "
                            + "whole canvas), required_input_region, neighborhood, stages, "
                            + "preview_only, scratch_heap_bytes and work_units (no per-layer rows).",
                        "properties": [
                            "x": ["type": "integer"], "y": ["type": "integer"],
                            "width": ["type": "integer"], "height": ["type": "integer"],
                        ],
                    ],
                    "preview_divisor": [
                        "type": "integer",
                        "description": "Integer >= 1 (default 1). Above 1 renders a PREVIEW of "
                            + "the region (the whole canvas when region is absent) that samples "
                            + "the top-left pixel of every divisor×divisor cell. Same refusals "
                            + "as region; not with layer or channel.",
                    ],
                    "exposure": [
                        "type": "number",
                        "description": "32-bit float documents only: a view exposure in stops, "
                            + "−20…20 (default 0), applied as gain 2^stops to RGB before the "
                            + "document's tone map; alpha is untouched. A tone-mapped render "
                            + "shows everything above white as white, so render at a negative "
                            + "exposure to SEE the highlights and at a positive one to see the "
                            + "shadows. It changes only this render: the document, its tone map "
                            + "and the window's own exposure (set_view) are not read or changed. "
                            + "For the flattened canvas or a region; refused with layer or "
                            + "channel, outside ±20, and when non-zero on a document that is not "
                            + "32-bit float.",
                    ],
                ]),
            tool(
                "sample_color",
                "Reads the color of one pixel of the flattened composite (what you see "
                    + "in a render) — the eyedropper. Returns straight (non-premultiplied) "
                    + "RGBA components (0-255) and the hex string (#RRGGBB, or #RRGGBBAA "
                    + "when not fully opaque). The numbers are the DOCUMENT's, in its own "
                    + "colour space (returned as \"space\") — not sRGB — so \"hex\" is "
                    + "what the pixel actually holds. Every color ARGUMENT here is sRGB, "
                    + "so to paint that colour back pass \"paint_hex\", the CLOSEST sRGB "
                    + "spelling (the same string on an sRGB document). It reproduces the "
                    + "pixel exactly only when the colour is inside the sRGB gamut: when "
                    + "it is not, \"paint_hex_exact\" is false and a \"note\" says so — "
                    + "painting that hex back gives a duller colour, and there is no sRGB "
                    + "string that gives the sampled one. Also returns \"srgb_rgba\" "
                    + "(the pixel as continuous normalized sRGB numbers, for a text or shape "
                    + "layer's rgba/fill_rgba/stroke_rgba) and \"srgb_in_gamut\". Errors when "
                    + "the point is outside the canvas.",
                [
                    "x": [
                        "type": "integer",
                        "description": "Pixel x in canvas coordinates (0 = left edge).",
                    ],
                    "y": [
                        "type": "integer",
                        "description": "Pixel y in canvas coordinates (0 = top edge).",
                    ],
                    "document_id": docID,
                ], required: ["x", "y"]),
            tool(
                "histogram",
                "Info > Histogram: inspect authoritative native-code statistics in either depth. "
                    + "source is composite, raw layer canvas pixels, or backdrop below a bottom-first "
                    + "record boundary; backdrop retains enclosing group gates. Selection is rational "
                    + "coverage weight (default true), not a hard contour. RGB counts alpha>0 without "
                    + "alpha weighting; alpha counts every positively weighted canvas position. "
                    + "Schema2 returns fractional display bins plus exact integer bin_weight_units, "
                    + "coverage_denominator 65535, full weighted native-code sums and separate exact "
                    + "endpoint counts/weights. stride changes display bins only: endpoint and sum "
                    + "statistics always scan the complete scope. Raw layer scope includes transparent "
                    + "canvas padding and excludes offcanvas storage. In 16-bit mode, and for a mask or "
                    + "channel at 32-bit float, source scalar "
                    + "reads the complete raw target plane, including offcanvas storage when unselected; "
                    + "target is required. Canvas selection weights map into that raw extent. Its RGB/luma "
                    + "fields describe the scalar value and analysis alpha is opaque. Read-only; no Undo or dirty change.",
                [
                    "source": ["type":"string", "enum":["composite","layer","backdrop","scalar"],
                        "description":"Named analysis source; default composite. Scalar is a native u16 raw plane (at 32-bit float, a mask or channel only)."],
                    "target": ["type":"string", "description":"Required for scalar: mask, red, green, blue, alpha or channel:<name>. Other source modes do not use target. At 32-bit float: mask or channel:<name> only."],
                    "layer": index,
                    "below": ["type":"integer", "description":"Backdrop excludes records at/above this boundary, retaining enclosing groups. 0...layer_count; required only for backdrop."],
                    "selection": ["type":"boolean", "description":"Weight by current selection coverage when present; default true."],
                    "stride": ["type":"integer", "description":"Positive plot sampling stride, default 1. Exact endpoints/sums remain complete."],
                    "document_id": docID,
                ]),
            tool(
                "sample_pixel",
                "Reads one pixel off the flattened composite and reports it every way the "
                    + "Info panel does. r/g/b/a and hex are the DOCUMENT's own numbers, "
                    + "converted nowhere; hsb is derived from them (h in degrees, s and b in "
                    + "percent); lab is CIE L*a*b* against the D50 PCS white computed through "
                    + "the document's OWN profile, so the same bytes read differently in an "
                    + "sRGB and a Display P3 document \u{2014} it is null when this build cannot "
                    + "model the profile (a LUT profile), and lab_space names the profile it "
                    + "went through. paint_hex is the closest sRGB spelling for a colour "
                    + "argument, with paint_hex_exact false when the pixel is outside the "
                    + "sRGB gamut. sample_size 1, 3 or 5 averages that square about (x, y), "
                    + "the eyedropper's own rule \u{2014} including its habit of accepting a "
                    + "centre just outside the canvas as long as part of the square is "
                    + "inside. Read-only. The UI path is the Info panel's readout; "
                    + "sample_color is the paint-oriented twin. In 16-bit mode, or when source/selection "
                    + "is supplied, schema2 reports exact center_rgba_codes, integer "
                    + "weighted_sum_rgba_codes/weight_units, mean_rgba_codes and normalized_rgba. "
                    + "All positively weighted in-bounds samples include hidden RGB at alpha 0. "
                    + "Coordinates are always canvas coordinates; layer padding is transparent black. "
                    + "Selection defaults false; explicit true weights fractional coverage. "
                    + "Named backdrop uses the same contribution cut as histogram.",
                [
                    "source": ["type":"string", "enum":["composite","layer","backdrop"], "description":"Default composite; explicit scope returns schema2 exact weighted native-code readout."],
                    "layer": index,
                    "below": ["type":"integer", "description":"Required backdrop boundary 0...layer_count; enclosing group gates retained."],
                    "selection": ["type":"boolean", "description":"Weight sample by current selection coverage; default false."],
                    "x": [
                        "type": "integer",
                        "description": "Pixel x in canvas coordinates (0 = left edge).",
                    ],
                    "y": [
                        "type": "integer",
                        "description": "Pixel y in canvas coordinates (0 = top edge).",
                    ],
                    "sample_size": [
                        "type": "integer", "enum": [1, 3, 5],
                        "description": "Side of the square averaged about (x, y); 1 (the "
                            + "default) is the pixel itself.",
                    ],
                    "document_id": docID,
                ], required: ["x", "y"]),
            // Colour management and metadata — the document's ICC profile,
            // print resolution and preserved EXIF/XMP/IPTC (Image > Mode >
            // Assign Profile… / Convert to Profile…, Image Size's
            // Resolution, the Export panel's two checkboxes).
            tool(
                "get_color_profile",
                "The document's colour space: its profile name, whether Doctored can "
                    + "convert with it, the print resolution and the print size that "
                    + "implies. convertible false means Doctored can display the profile "
                    + "and re-embed it on export but cannot transform with it (a LUT-based "
                    + "profile) — assign_profile still works, convert_profile does not. "
                    + "on_open reports what the OPEN did: \"unchanged\", \"converted\" "
                    + "into the working space, or \"kept\" because it could not be "
                    + "converted from. on_open_refusal is null, or the sentence saying what the "
                    + "open did with an embedded profile a document cannot carry: a Gray or "
                    + "CMYK image converted into sRGB through it, or an image read into sRGB "
                    + "without it (\"unchanged\" then means nothing more was done). "
                    + "get_document's color_profile block carries the same name, convertible, "
                    + "can_decode, on_open and on_open_refusal keys. A file with no profile is assumed sRGB, and is converted when the "
                    + "working space differs. Every #RRGGBB argument in this surface is sRGB "
                    + "and converts into the document's space once, so on a non-sRGB document "
                    + "brush_stroke, fill, gradient and the shape/text tools all lay down the "
                    + "same colour — but the bytes a render or sample_color reports are the "
                    + "document's, not the hex you passed.",
                ["document_id": docID]),
            tool(
                "assign_profile",
                "RELABELS the document with another colour profile. Pixel numbers are "
                    + "NOT touched, so the picture changes appearance — this is the tool "
                    + "for a file that was tagged wrongly. One undo step. Answers "
                    + "changed:false when the document already carries that profile; "
                    + "errors on bytes that are not an RGB ICC profile. In a 32-bit float "
                    + "document the linear values are kept exactly and now mean the new "
                    + "profile's primaries; a profile with no matrix/TRC model, or one whose "
                    + "tone curves fail the precision check, is refused by name.",
                [
                    "profile": profileChoice, "path": profilePath, "document_id": docID,
                ], required: ["profile"]),
            tool(
                "convert_profile",
                "CONVERTS the document to another colour profile: every layer's pixels "
                    + "are transformed so the picture LOOKS the same and its numbers "
                    + "change. Layer masks, alpha channels and layer descriptions are "
                    + "coverage and are untouched, and so are layer-style and text-layer "
                    + "colours: those are authored in sRGB and convert where they are used, "
                    + "so they keep their appearance. One undo step. Answers changed:false "
                    + "when the two profiles describe the same space, or when this build "
                    + "cannot convert from or into one of them (get_color_profile's "
                    + "can_decode). In a 32-bit float document the conversion is the matrix "
                    + "between the two profiles' primaries on the linear values, with no "
                    + "clamp: a colour outside the destination gamut keeps a negative "
                    + "component and values above white stay above white. There it runs at "
                    + "relative, perceptual or saturation intent (all the same result) and "
                    + "refuses intent absolute, black_point_compensation and adjustments "
                    + "preserve_appearance by name, and a profile with no matrix/TRC model "
                    + "answers changed:false.",
                [
                    "profile": profileChoice, "path": profilePath, "document_id": docID,
                ], required: ["profile"]),
            tool(
                "set_resolution",
                "Sets the print resolution in pixels per inch. NO pixel is resampled — "
                    + "only the print size changes; use image_size to change the pixel "
                    + "dimensions. Mirrors Image Size…'s Resolution field with Resample "
                    + "off. One undo step; 1-30000 ppi. Answers at EVERY depth: at 16 "
                    + "bits and in 32-bit float every pixel buffer is the very allocation "
                    + "it was, which is what \"no pixel is resampled\" means there.",
                [
                    "ppi_x": ["type": "number", "minimum": 1, "maximum": 30000],
                    "ppi_y": [
                        "type": "number", "minimum": 1, "maximum": 30000,
                        "description": "Vertical resolution; defaults to ppi_x.",
                    ],
                    "document_id": docID,
                ], required: ["ppi_x"]),
            tool(
                "get_metadata",
                "Which metadata packets the document carries and how big they are — "
                    + "presence and byte sizes only, not a parsed EXIF dump — plus its "
                    + "print resolution, its profile name and what the open did (on_open, "
                    + "and on_open_refusal: null or the sentence get_color_profile reports). EXIF, "
                    + "XMP and IPTC are captured where supported in JPEG/PNG/TIFF; RAW/HEIC "
                    + "imports reconstruct standard ImageIO fields and XMP. metadata_import "
                    + "separates supported scope, packet representation and verified omissions. "
                    + "Packets are written on supported exports unless strip_metadata is set. "
                    + "metadata_not_captured: true means this document came from a file "
                    + "whose container was not inspected for capture data (such as a Live Photo frame); packets were "
                    + "never read — absent here does not mean the file had none.",
                ["document_id": docID]),
        ]
    }
}
