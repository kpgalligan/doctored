import Foundation

extension AgentServer {
    /// `depth_blur` and `list_models` (handlers in AgentServer+DepthBlur.swift, the
    /// settings and run in DepthBlur.swift, the store in ModelStore.swift).
    func depthBlurCatalog(_ schema: CatalogSchema) -> [[String: Any]] {
        let slider: (Double, Double, Double, String) -> [String: Any] = { low, high, initial, text in
            ["type": "number", "minimum": low, "maximum": high,
             "description": text + " (\(Int(low)) to \(Int(high)), default \(Int(initial)))."]
        }
        // An array of literals joined, never a `+` chain (see `adjustmentOps`).
        let description = [
            "Blurs a layer by depth ",
            "\u{2014} the app's Filter \u{25B8} Neural Filters \u{25B8} Depth Blur\u{2026}. A local depth model ",
            "(Depth Anything V2 Small, Core ML) estimates the depth of the layer, or of the active ",
            "selection's bounding box on it, and pixels away from the focus are blurred with a lens ",
            "disc whose radius grows with their distance from the focal plane. The focus is the ",
            "subject (focus_subject, Vision's subject mask, the default), a focal point (focal_x, ",
            "focal_y in canvas px, which turns focus_subject off unless it is given), or ",
            "focal_distance. focal_range keeps a band of depth sharp; haze lightens far pixels; ",
            "warmth, brightness and saturation grade the result; grain adds film grain; ",
            "depth_map_only writes the depth map as grey (near is white) instead. output new_layer ",
            "writes a copy of the layer above it, named \"<name> Depth Blur\". One undo step; writes ",
            "only inside the active selection. The reply carries changed, output_layer, ",
            "focal_distance_used (the focus in percent) and the model's id and version. 8-, 16- and ",
            "32-bit documents: a float layer is blurred in linear light. Refused by name when the ",
            "model is not installed (the user downloads it in Settings \u{25B8} Models; downloads stay a ",
            "user action, because they fetch tens of MB from a third-party host under a licence ",
            "the user must accept), on an empty layer, and when Vision finds no subject for ",
            "focus_subject. A re-editable text or shape layer drops its description (undo restores it).",
        ].joined()
        var properties: [String: Any] = [
            "focus_subject": ["type": "boolean",
                              "description": "Keep Vision's subject in focus (default true without a focal point)."],
            "focal_x": ["type": "number", "description": "Focal point x in canvas px (with focal_y)."],
            "focal_y": ["type": "number", "description": "Focal point y in canvas px (with focal_x)."],
            "depth_map_only": ["type": "boolean", "description": "Write the depth map instead (default false)."],
            "output": ["type": "string", "enum": ["layer", "new_layer"],
                       "description": "The layer itself (default) or a new layer above it."],
            "layer": schema.index, "document_id": schema.docID,
        ]
        let notes: [String: String] = [
            "focal_distance": "The focal depth when there is no subject or point, 0 nearest",
            "focal_range": "The band of depth around the focus kept sharp",
            "blur_strength": "How strong the blur is",
            "haze": "Lightens pixels by their depth",
            "warmth": "Warms (positive) or cools the result",
            "brightness": "Brightens or darkens the result",
            "saturation": "Saturates or desaturates the result",
            "grain": "Film grain amount",
        ]
        for entry in DepthBlurSettings.sliders {
            properties[entry.key] = slider(entry.range.lowerBound, entry.range.upperBound, entry.initial,
                                           notes[entry.key] ?? entry.key)
        }
        let listDescription = [
            "Lists the local models of Settings \u{25B8} Models with their state (not_downloaded, ",
            "downloading with progress 0 to 1, ready, failed with failure), bytes on disk, whether ",
            "the user accepted the licence of this version, the licence (name, url, tier) and the ",
            "training-data note. Read only: downloading is a user action in Settings \u{25B8} Models, ",
            "because it fetches tens of MB from a third-party host under a licence the user must accept.",
        ].joined()
        return [
            tool("depth_blur", description, properties),
            tool("list_models", listDescription, [:]),
        ]
    }
}
