import Foundation

extension AgentServer {
    /// `super_zoom`, `jpeg_artifacts_removal` and `neural_colorize` (handlers in
    /// AgentServer+NeuralFilters.swift, the runs in SuperZoom.swift, JpegArtifactsRemoval.swift and
    /// Colorize.swift).
    func neuralFiltersCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let slider: (String) -> [String: Any] = { text in
            ["type": "number", "minimum": 0, "maximum": 100, "description": text + " (0 to 100, default 0)."]
        }
        // An array of literals joined, never a `+` chain (see `adjustmentOps`).
        let zoomDescription = [
            "Enlarges a layer into a NEW document ",
            "\u{2014} the app's Filter \u{25B8} Neural Filters \u{25B8} Super Zoom\u{2026}. The layer's ",
            "pixels on its canvas are zoomed 2x or 4x (zoom) by a local super-resolution model ",
            "(Real-ESRGAN x4plus, Core ML) run tile by tile and stitched seamlessly; ",
            "enhance_details false uses a classical Catmull-Rom resample instead. ",
            "remove_jpeg_artifacts runs the JPEG artifacts model (FBCNN, Medium) on the source first. ",
            "noise_reduction (Reduce Noise on the source) and sharpen (Unsharp Mask on the result) ",
            "are classical. The new document has one layer, \"Background\", the source's profile and ",
            "resolution, and the title \"<name> Super Zoom \u{00D7}N\"; the source is not changed and the ",
            "selection is ignored. The reply carries the new document_id, its width and height, the ",
            "zoom, tiles, planned_bytes and the models used. 8- and 16-bit RGB documents; 32-bit and ",
            "CMYK documents are refused by name. A result past 100 megapixels is refused by its size ",
            "(choose a smaller zoom). Refused by name when a needed model is not installed (the user ",
            "downloads it in Settings \u{25B8} Models: Real-ESRGAN always, FBCNN for remove_jpeg_artifacts).",
        ].joined()
        let zoomProperties: [String: Any] = [
            "zoom": ["type": "integer", "enum": [2, 4], "description": "The enlargement: 2 (default) or 4."],
            "enhance_details": ["type": "boolean",
                                "description": "Run the super-resolution model (default true); false resamples."],
            "remove_jpeg_artifacts": ["type": "boolean",
                                      "description": "Remove JPEG artifacts from the source first (default false)."],
            "noise_reduction": slider("Noise reduction on the source"),
            "sharpen": slider("Sharpening of the result"),
            "layer": schema.index, "document_id": schema.docID,
        ]
        let jpegDescription = [
            "Removes JPEG compression artifacts (blocking, ringing) from a layer ",
            "\u{2014} the app's Filter \u{25B8} Neural ",
            "Filters \u{25B8} JPEG Artifacts Removal\u{2026}. A local model (FBCNN colour, Core ML) runs tile by ",
            "tile over the layer's own pixels; strength low, medium (default) or high is the model's ",
            "quality factor 70, 40 or 10. output layer (default) rewrites the layer; new_layer writes ",
            "the result to a new layer \"<name> JPEG Artifacts Removal\" directly above it. Alpha is ",
            "kept exactly; the selection is ignored. One undo step. The reply carries changed, ",
            "output_layer, strength, quality_factor, tiles, planned_bytes and the model. 8- and 16-bit ",
            "RGB documents; 32-bit and CMYK documents are refused by name. Refused by name when the ",
            "model is not installed (the user downloads FBCNN in Settings \u{25B8} Models). A re-editable ",
            "text or shape layer drops its description with output layer (undo restores it).",
        ].joined()
        let jpegProperties: [String: Any] = [
            "strength": ["type": "string", "enum": JpegArtifactsStrength.allCases.map(\.rawValue),
                         "description": "low, medium (default) or high."],
            "output": ["type": "string", "enum": ["layer", "new_layer"],
                       "description": "The layer itself (default) or a new layer above it."],
            "layer": schema.index, "document_id": schema.docID,
        ]
        let colorizeDescription = [
            "Colors a layer from its luminance ",
            "\u{2014} the app's Filter \u{25B8} Neural Filters \u{25B8} Colorize\u{2026}. A local ",
            "model (DDColor, Core ML) predicts the colour (Lab a*, b*) from the layer's lightness once ",
            "at 512\u{00B2}; the colour is upscaled onto the layer's own full-resolution lightness in ",
            "the document's profile, so luminance and detail are kept exactly and only chroma is ",
            "added (colours past the profile's gamut lose chroma, never hue or lightness). The ",
            "classical sliders: saturation scales the colour (-50 gray, +50 double); cyan_red, ",
            "magenta_green and yellow_blue push it towards red, green or blue (positive) or cyan, ",
            "magenta or yellow (negative); color_artifact_reduction blurs the predicted colour to ",
            "remove blotches. output layer (default) rewrites the layer, new_layer writes a new ",
            "layer \"<name> Colorize\" directly above it (one undo step either way), new_document ",
            "opens the layer's canvas colorized as a NEW document \"<name> Colorize\" (the reply ",
            "carries its document_id, width and height). Alpha is kept exactly; the selection is ",
            "ignored. The reply carries changed, output, planned_bytes and the model, plus layer ",
            "and output_layer for the layer outputs. 8- and 16-bit RGB documents; 32-bit and CMYK ",
            "documents are refused by name. Refused by name when the model is not installed (the ",
            "user downloads DDColor in Settings \u{25B8} Models). A re-editable text or shape layer ",
            "drops its description with output layer (undo restores it).",
        ].joined()
        let balance: (String) -> [String: Any] = { text in
            ["type": "number", "minimum": -50, "maximum": 50, "description": text + " (-50 to 50, default 0)."]
        }
        let colorizeProperties: [String: Any] = [
            "saturation": balance("Colour saturation"),
            "cyan_red": balance("Cyan (negative) to red (positive)"),
            "magenta_green": balance("Magenta (negative) to green (positive)"),
            "yellow_blue": balance("Yellow (negative) to blue (positive)"),
            "color_artifact_reduction": slider("Smoothing of the predicted colour"),
            "output": ["type": "string", "enum": ColorizeOutput.allCases.map(\.rawValue),
                       "description": "The layer itself (default), a new layer above it, or a new document."],
            "layer": schema.index, "document_id": schema.docID,
        ]
        return [
            tool("super_zoom", zoomDescription, zoomProperties),
            tool("jpeg_artifacts_removal", jpegDescription, jpegProperties),
            tool("neural_colorize", colorizeDescription, colorizeProperties),
        ]
    }
}
