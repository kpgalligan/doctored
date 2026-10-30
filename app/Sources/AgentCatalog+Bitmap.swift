import Foundation

/// The Bitmap document's catalog notes (#547 Bitmap phase 1a): `convert_mode`'s `bitmap` mode and
/// the Bitmap sheet's options (`method`, `resolution`; since phase 1b `frequency`, `angle`,
/// `shape` and `pattern`), `gray` from Bitmap with `size_ratio`,
/// `get_document`'s mode, and (phase 1c) the 1-bit files: `save_copy`, the exports, `export_psd`
/// and `inspect_psd_export`, `open_document` and `inspect_psd`, (phase 2a-i) the painting tools
/// and the bit-exact geometry, (phase 2a-ii) the resampled geometry, Move, the selected pixels'
/// moves, Cut and the Magic Wand's refusal, and (phase 2b) the blend modes Fill takes there, the
/// refused transforms, Trim and arbitrary rotation, the filters' refusal and Lens Blur's,
/// and Halftone Screen as declared and unmeasured, and (phase 3) the Gradient, Clone Stamp,
/// History Brush, Liquify, Perspective Crop, Straighten Layer as the straightened crop, the render
/// channels, the layer row, the selection tools, Copy, and Copy Merged, Paste and Save Selection's
/// refusals (`BitmapDocument`'s gate, the core's `bitmap_convert`, `bitmap_document`,
/// `bitmap_io`, `bitmap_export`, `bitmap_psd`, `bitmap_edit` and `bitmap_tools`).
/// Behind an explicit non-inlining boundary, for `AgentCatalog.swift`'s reason.
extension AgentServer {
    @inline(never) func annotateBitmapCatalog(_ catalog: inout [[String: Any]]) {
        let properties: [String: Any] = [
            "method": ["type": "string", "enum": ["threshold", "diffusion", "pattern", "halftone", "custom"],
                       "description": "Toward Bitmap: 50% Threshold, Pattern Dither, Diffusion Dither "
                        + "(the default), Halftone Screen or Custom Pattern."],
            "frequency": ["type": "number", "minimum": 1, "maximum": 999.999,
                          "description": "Method halftone: the screen frequency in lines per inch "
                            + "of the output (53 by default)."],
            "angle": ["type": "number", "minimum": -180, "maximum": 180,
                      "description": "Method halftone: the screen angle in degrees (45 by default)."],
            "shape": ["type": "string",
                      "enum": ["round", "ellipse", "line", "square", "cross", "diamond"],
                      "description": "Method halftone: the spot shape (round by default)."],
            "pattern": ["type": "object",
                        "description": "Method custom: the pattern whose gray is the threshold, "
                            + "{\"preset\": \"checkerboard\"|\"stripes\"|\"dots\"|\"grid\"} or an "
                            + "inline tile {\"name\"?, \"width\", \"height\", \"rgba\"} (base64 of "
                            + "width × height × 4 straight RGBA8 bytes, each side 1–512), the "
                            + "pattern object Pattern Overlay takes; checkerboard by default."],
            "resolution": ["type": "number", "minimum": 1, "maximum": 30000,
                           "description": "Toward Bitmap: the Output resolution in pixels per inch; "
                            + "omitted keeps the document's pixels and resolution."],
            "size_ratio": ["type": "integer", "minimum": 1, "maximum": 16,
                           "description": "Mode gray on a Bitmap document: the Size Ratio (1 by "
                            + "default); r averages each r × r block into one gray pixel."],
        ]
        let note = " mode \"bitmap\" (#547 Bitmap phase 1a) converts an 8-bit Grayscale document to "
            + "Bitmap, as Image ▸ Mode ▸ Bitmap…: one layer of black and white pixels, its 1-bit "
            + "plane kept in the document. method threshold (gray 128 and above is white), pattern "
            + "(an 8 × 8 ordered dither from the canvas origin whose threshold table agrees with "
            + "Photoshop's on 99.2 % of a measured ramp), diffusion (the default: Sierra "
            + "Lite error diffusion onto black and white, serpentine; 70 % agreement with "
            + "Photoshop's, which varies between sessions), halftone (#547 Bitmap "
            + "phase 1b: a screen of frequency lines per inch of the output, 1–999.999, 53 by "
            + "default, at angle degrees, −180–180, 45 by default, of shape round, ellipse, line, "
            + "square, cross or diamond, Adobe's spot functions, round by default; Doctored's own "
            + "declared screen, not measured against Photoshop's, whose Halftone Screen conversion "
            + "hung both probe sessions that tried it) or custom (the "
            + "pattern's gray over white, tiled from the origin, is the threshold: a pixel is white "
            + "where its gray is above it). frequency, angle and shape apply to method halftone "
            + "only and pattern to method custom only. resolution (pixels per "
            + "inch, 1–30000): an axis whose resolution differs is resampled Bicubic to round(n × "
            + "resolution / ppi) pixels at that resolution first; omitted, or equal to an axis's "
            + "own, keeps that axis. A document of more than one plain layer must be flattened "
            + "(flatten true, else the refusal says so). Every other mode and a 16- or 32-bit "
            + "Grayscale document refuse by name, naming the Grayscale or 8 Bits/Channel step. On a "
            + "Bitmap document only mode \"gray\" converts, with size_ratio (1 to 16, 1 by default: "
            + "the gray storage as it is; r averages each r × r block, partial edge blocks over "
            + "their own pixels, onto a canvas 1/r the size at 1/r the resolution) and no "
            + "gray_space; every other mode refuses, naming the Grayscale step. A Bitmap result "
            + "replies depth 1."
        let paintNote = " On a Bitmap document (#547 Bitmap phase 2a-i) it paints in black and "
            + "white: the colour becomes black or white by its gray (128 and above is white), the "
            + "edit runs on the 8-bit gray storage at its opacity, and every pixel goes back to "
            + "white where its gray over the background at its own alpha reaches 128, else black, "
            + "so a soft edge lands hard where its coverage reaches about half and a stroke under "
            + "50 % opacity changes nothing. blend_mode Normal, Darken or Lighten only "
            + "(#547 Bitmap phase 2b; another refuses); "
            + "sample_all_layers is off (the one layer is the composite). An erased or cleared "
            + "pixel takes the background, white."
        let geometryNote = " On a Bitmap document (#547 Bitmap phase 2a-i) it is bit-exact: every "
            + "pixel keeps its bit."
        let newAreaNote = " New canvas area is the background, white."
        let rotateExactNote = " On a Bitmap document (#547 Bitmap phase 2a-i) the quarter turns "
            + "are bit-exact: every pixel keeps its bit."
        let cropExactNote = " On a Bitmap document (#547 Bitmap phase 2a-i) a crop at angle 0 is "
            + "bit-exact: every pixel keeps its bit."
        let cropNote = " A rectangle past the canvas fills with background_color thresholded to "
            + "black or white (white by default); fill other than background refuses (Generative "
            + "Expand and Content-Aware Fill are not available in Bitmap mode)."
        let resampleHead = "runs on the 8-bit gray storage with its sampler and every pixel goes "
            + "back white where its gray over the background at its alpha reaches 128, else black; "
        let resampleTail = "a whole-pixel move or nearest sampler keeps each bit."
        let resampleBody = resampleHead + "area it uncovers takes the background, white; "
            + resampleTail
        let resampleNote = " On a Bitmap document (#547 Bitmap phase 2a-ii) it " + resampleBody
        let rotateRefusalNote = " Any other angle refuses (#547 Bitmap phase 2b): "
            + BitmapDocument.arbitraryRotationRefusal
        // crop's cropNote alone says what fills new area (its background_color).
        let cropResampleNote = " A nonzero angle (#547 Bitmap phase 2a-ii) " + resampleHead
            + resampleTail
        let copyNote = " On a Bitmap document duplicate true refuses: "
            + BitmapDocument.copyRefusal
        // Phase 3: the tools built through the same adapter, and the ones refused for good.
        let phase3 = " A Bitmap document (#547 Bitmap phase 3): "
        let thresholdBody = "runs on the 8-bit gray storage and every pixel goes back white where "
            + "its gray over white at its own alpha reaches 128, else black"
        let blendNote = "; blend_mode Normal, Darken or Lighten only (another refuses)."
        let gradientNote = phase3 + "it " + thresholdBody + ", so the ramp lands hard where it "
            + "crosses 128; its colours are used as given, not snapped to black or white"
            + blendNote
        let cloneNote = phase3 + "the clone " + thresholdBody + blendNote
            + " output new_layer refuses: " + BitmapDocument.disabledSentence("add_layer")
        let historyNote = phase3 + "it paints the gray storage back toward the state's layer and "
            + thresholdBody + ", so full coverage gives the state's bits back."
        let liquifyNote = phase3 + "the warp " + thresholdBody + "."
        let perspectiveNote = phase3 + "it " + thresholdBody + "; area outside the corners takes "
            + "the background, white."
        let straightenNote = phase3 + "Straighten Layer is the straightened crop, as the one layer "
            + "cannot turn inside the canvas: crop at the Crop tool's fit of the whole canvas at "
            + "the line's angle (fit_crop's box for that line), so the canvas shrinks to the "
            + "window; the reply is crop's with rotation."
        let renderNote = phase3 + "channel composite or bitmap draws the composite, with no layer; "
            + "any other channel, or a channel with layer, refuses: " + BitmapDocument.renderRefusal
        let propertiesNote = phase3 + "the one layer takes name only; any other change refuses: "
            + BitmapDocument.layerPropertiesRefusal
        let selectionNote = phase3 + "it edits the selection only; the plane is unchanged."
        for index in catalog.indices {
            guard let name = catalog[index]["name"] as? String else { continue }
            let existing = (catalog[index]["description"] as? String) ?? ""
            switch name {
            case "convert_mode":
                catalog[index]["description"] = existing + note
                guard var schema = catalog[index]["inputSchema"] as? [String: Any],
                      var current = schema["properties"] as? [String: Any] else { continue }
                if var mode = current["mode"] as? [String: Any] {
                    var options = (mode["enum"] as? [String]) ?? []
                    if !options.contains("bitmap") { options.append("bitmap") }
                    mode["enum"] = options
                    current["mode"] = mode
                }
                current.merge(properties) { existing, _ in existing }
                schema["properties"] = current
                catalog[index]["inputSchema"] = schema
            case "get_document":
                catalog[index]["description"] = existing + " A Bitmap document (#547) reports "
                    + "color_mode bitmap."
            case "save_copy":
                catalog[index]["description"] = existing + " A Bitmap document (#547 Bitmap phase "
                    + "1c) saves doctored (or rz), a 1-bit png, tif or bmp keeping its plane (the reply's bit_depth "
                    + "is 1), or jpg through its 8-bit Grayscale render (bit_depth 8, the loss named "
                    + "in flat_save_losses); bit_depth defaults to 8 and no other value is taken for "
                    + "the 1-bit files. Every other format refuses; convert_mode gray first."
            case "export_image", "export_preview", "quick_export":
                catalog[index]["description"] = existing + " A Bitmap document (#547 Bitmap phase "
                    + "1c) with profile document exports png, tiff or bmp at 1 bit a pixel (bit_depth "
                    + "8 in the request, 1 in the reply; no size or sharpen) and jpeg through its "
                    + "8-bit Grayscale render; any other format refuses with profile document, and "
                    + "profile srgb, display-p3 or file exports RGB."
            case "export_psd", "inspect_psd_export":
                catalog[index]["description"] = existing + " A Bitmap document (#547 Bitmap phase "
                    + "1c) writes a 1-bit colour-mode-0 PSD or PSB (1 is black, no profile, the "
                    + "same file in both modes; bit_depth 1, the report's depth 1 and color_mode "
                    + "bitmap); its TIFF with layers, and a side past the container's limit, refuse."
            case "open_document":
                catalog[index]["description"] = existing + " A 1-bit gray PNG (no tRNS), a 1-bit "
                    + "TIFF or a 1 bpp black-and-white BMP opens as a Bitmap document (#547 Bitmap "
                    + "phase 1c); precision rgba8 keeps the 8-bit RGB open. A Bitmap PSD or PSB "
                    + "opens with psd_mode composite only (it holds no layers)."
            case "inspect_psd":
                catalog[index]["description"] = existing + " A Bitmap (colour mode 0) PSD or PSB "
                    + "answers import_precision \"bitmap\" (#547 Bitmap phase 1c): no layers "
                    + "(layered_available false), its stored composite opens as a Bitmap document."
            case "brush_stroke", "eraser_stroke", "fill", "fill_selection", "stroke_selection",
                 "clear_selection":
                catalog[index]["description"] = existing + paintNote
            case "canvas_size", "flip":
                catalog[index]["description"] = existing + geometryNote + newAreaNote
            case "rotate":
                catalog[index]["description"] = existing + rotateExactNote + rotateRefusalNote
            case "crop":
                // crop's own note alone says what fills new area.
                catalog[index]["description"] = existing + cropExactNote + cropNote
                    + cropResampleNote
            case "image_size", "move_selected_pixels":
                catalog[index]["description"] = existing + resampleNote
            case "move_layers":
                catalog[index]["description"] = existing + resampleNote + copyNote
            case "transform_layer", "transform_layers", "distort_layer", "transform_again",
                 "transform_selected_pixels", "trim":
                catalog[index]["description"] = existing + " A Bitmap document (#547 Bitmap phase "
                    + "2b): not available in Bitmap mode."
            case "cut":
                catalog[index]["description"] = existing + " A Bitmap document (#547 Bitmap phase "
                    + "2a-ii): Cut pixels take the background, white."
            case "select_magic_wand":
                catalog[index]["description"] = existing + " A Bitmap document (#547 Bitmap phase "
                    + "2a-ii): not available in Bitmap mode."
            case "apply_filter":
                catalog[index]["description"] = existing + " A Bitmap document (#547 Bitmap phase "
                    + "2b): not available in Bitmap mode; filter lens_blur "
                    + "refuses: "
                    + BitmapDocument.lensBlurRefusal
            case "depth_blur":
                catalog[index]["description"] = existing + " A Bitmap document (#547 Bitmap phase "
                    + "2b): not available in Bitmap mode."
            case "gradient":
                catalog[index]["description"] = existing + gradientNote
            case "clone_stamp":
                catalog[index]["description"] = existing + cloneNote
            case "history_brush_stroke":
                catalog[index]["description"] = existing + historyNote
            case "liquify":
                catalog[index]["description"] = existing + liquifyNote
            case "perspective_crop":
                catalog[index]["description"] = existing + perspectiveNote
            case "straighten_layers":
                catalog[index]["description"] = existing + straightenNote
            case "render":
                catalog[index]["description"] = existing + renderNote
            case "set_layer_properties":
                catalog[index]["description"] = existing + propertiesNote
            case "set_quick_mask", "quick_mask_stroke", "quick_select", "select_magnetic":
                catalog[index]["description"] = existing + selectionNote
            case "copy":
                catalog[index]["description"] = existing + phase3 + "the copy holds each bit as "
                    + "black or white gray."
            case "copy_merged", "paste":
                catalog[index]["description"] = existing + phase3 + "not available in Bitmap mode."
            case "save_selection":
                catalog[index]["description"] = existing + phase3
                    + BitmapDocument.saveSelectionRefusal
            default:
                break
            }
        }
    }
}
