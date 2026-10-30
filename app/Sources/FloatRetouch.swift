import AppKit

/// Retouching in a 32-bit float document: the rules the Clone Stamp,
/// Healing Brush, Spot Healing Brush, Patch, Remove and Content-Aware Move
/// tools, Edit ▸ Content-Aware Fill… and their MCP twins share. There is no
/// pixel math here: every gesture sends the 16-bit typed request unchanged
/// (`retouch_clone`, `retouch_heal`, `spot_heal`, `content_aware_fill`) and
/// the core's `float_retouch` runs it on f32.
///
/// - The Clone Stamp copies LINEAR light, unclamped, in one of
///   `NativeLayerBlending.floatModes`: a cloned 8.0 reads 8.0.
/// - The healing family (heal, spot heal, patch, remove, fill, move) compares
///   and solves in a working encoding and decodes once at the write, so a
///   pixel it does not change keeps its bits; it blends in Normal only.
/// - The Blur, Sharpen, Sponge and Smudge brushes and the History Brush
///   (`brushTools`, the core's `float_retouch_brush`) rework a layer's own
///   samples in linear light with the range kept: a blur spreads a value
///   above white, a sharpened value below zero is stored, the Sponge
///   saturates or desaturates about the luma with no clamp (R5 of
///   `display_encoding`), Smudge carries above-white values along the
///   stroke, and the History Brush restores a float state's exact samples.
///   They take the 16-bit requests unchanged too.
/// - A float retouch edits a layer's own pixels. A mask, a channel, one
///   colour channel and Quick Mask refuse by name, as at 16 bits; the Sponge
///   on a mask or channel says why (`spongeCoverageRefusal`, every depth).
/// - The Red Eye tool and Filter ▸ Remove Red Eye run: the
///   core corrects each rect on the layer's display-encoded copy, so the red
///   test reads display values and a value above white keeps its light ratio
///   (`red_eye` / `red_eye_auto`, `.floatTool`).
/// - What a float retouch still refuses, the core's
///   operator table says why.
enum FloatRetouch {
    /// The MCP tools this gate answers for.
    static let tools: Set<String> = [
        "clone_stamp", "heal_stroke", "spot_heal_stroke", "patch_region", "remove_stroke",
        "content_aware_fill", "content_aware_move",
    ]

    /// The retouch-brush tools this gate answers for.
    static let brushTools: Set<String> = [
        "blur_stroke", "sharpen_stroke", "smudge_stroke", "history_brush_stroke", "sponge_stroke",
    ]

    /// Why the Sponge refuses a mask or an alpha channel, at every depth
    /// (`display_encoding`'s R5).
    static let spongeCoverageRefusal: String =
        "The Sponge changes saturation, and a mask or alpha channel is one gray plane with none; "
        + "Dodge and Burn work there."

    /// `FloatDocument.limitation`'s clause for the retouch tools.
    static let limitationClause: String =
        "the Clone Stamp (linear light, in the same modes), the Healing Brush, Spot Healing "
        + "Brush, Patch, Remove and Content-Aware Move tools and Content-Aware Fill on a "
        + "layer's pixels, the Blur, Sharpen, Sponge and Smudge brushes and the History Brush "
        + "on a layer's pixels (linear light, colour never clamped to 0…1), "
        + "the Red Eye tool and Filter ▸ Remove Red Eye on a layer's pixels (on the display "
        + "encoding), "
        + "Stroke Path with the Brush or the Eraser, "

    static let normalOnly: String =
        "32-bit float retouch supports Normal blending only; clone_stamp alone takes blend_mode."

    /// The pre-dispatch gate (`DocumentPrecision.refusal`); nil for any tool
    /// outside `tools` and `brushTools` and outside a float document. In
    /// order: a `content_aware_fill` that commits a stored preview passes (its
    /// preview was asked already); a `target` other than the layer; Quick
    /// Mask; the core's `clone` / `heal` / `retouch_brush` capability for the
    /// entry's kind; the named `blend_mode` (a brush tool's handler refuses
    /// one itself, in its own sentence).
    static func refusal(tool: String, arguments: [String: Any], document: ImageDocument) -> String? {
        let brush = brushTools.contains(tool)
        guard tools.contains(tool) || brush, let doc = document.doc, doc.isFloat else { return nil }
        if tool == "content_aware_fill", arguments["preview_id"] != nil { return nil }
        if let target = arguments["target"] as? String, target != "layer" {
            if tool == "sponge_stroke", target == "mask" || target.hasPrefix("channel:") {
                return spongeCoverageRefusal
            }
            if let refusal = AgentServer.sampledTargetRefusal(arguments) { return refusal }
            return DocumentPrecision.scalarTargetRefusal(tool)
        }
        if document.quickMaskActive { return OperationScope.quickMaskRefusal }
        let index = DocumentPrecision.integer(arguments["layer"])
            ?? DocumentPrecision.integer(arguments["index"]) ?? document.activeLayerIndex
        let kind = doc.layerIsGroup(index) ? "group" : doc.layerIsAdjustment(index) ? "adjustment" : "raster"
        do {
            let report = try RasterDocument.precisionCapability(
                operation: brush ? "retouch_brush" : tool == "clone_stamp" ? "clone" : "heal",
                target: kind, depth: UInt32(32))
            guard report["supported"] as? Bool == true else {
                return report["reason"] as? String ?? FloatDocument.refusal(tool: tool)
            }
        } catch {
            return "Could not validate the 32-bit float operation capability: "
                + error.localizedDescription
        }
        guard !brush, let name = arguments["blend_mode"] as? String else { return nil }
        if tool == "clone_stamp" {
            return NativeLayerBlending.mode(named: name).flatMap(FloatPaint.modeRefusal)
        }
        return name.caseInsensitiveCompare("Normal") == .orderedSame ? nil : normalOnly
    }
}
