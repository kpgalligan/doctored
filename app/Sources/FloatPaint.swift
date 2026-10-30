import AppKit

/// The Brush, Eraser, Gradient, Paint Bucket and Dodge/Burn in a 32-bit float
/// document: the rules the canvas tools
/// (`EditorViewController+PrecisionPaint.swift`, `PrecisionColorTools.swift`)
/// and their MCP twins `brush_stroke`, `eraser_stroke`, `gradient`, `fill` and
/// `dodge_burn` (`AgentServer+PrecisionPaint.swift`) share. There is no pixel
/// math here: the core's `float_paint` composites in linear light with no
/// clamp on colour, and `display_encoding` states the display-defined
/// rules (the bucket compares R1 display codes; Dodge/Burn is R4 on a layer
/// and R4m on a mask or alpha channel).
///
/// - A LAYER takes the 16-bit stroke, gradient and bucket requests unchanged
///   (`paint_blend`, `erase_native`, `gradient`, `bucket_fill`, `dodge_burn`).
///   The ink is the authored colour decoded to linear light by the core; a
///   foreground above white (the Color Picker's Intensity row, an eyedropped
///   highlight) or MCP's `rgba32` / `start_rgba32` + `end_rgba32` is `linear`
///   ink, used as it is. The blend mode is one of
///   `NativeLayerBlending.floatModes`; Dodge/Burn has none.
/// - A MASK or an ALPHA CHANNEL is 0…1 u16 coverage at every depth and keeps
///   the 16-bit scalar route (Dodge/Burn's is the scalar `dodge_burn`, R4m).
///   A linear-ink argument aimed at one is refused by name
///   (`coverageInkRefusal`), never ignored.
/// - One COLOUR CHANNEL of a float layer refuses (`planeRefusal`): the core's
///   scalar colour plane is u16-only, and Edit ▸ Fill… is the float route.
enum FloatPaint {
    /// The MCP tools this gate answers for.
    static let tools: Set<String> = ["brush_stroke", "eraser_stroke", "gradient", "fill", "dodge_burn"]

    /// The arguments that carry linear ink.
    static let linearInkKeys = ["rgba32", "start_rgba32", "end_rgba32"]

    /// `FloatDocument.limitation`'s clause for the five tools.
    static let limitationClause: String =
        "the Brush, Eraser, Gradient, Paint Bucket, Dodge and Burn tools on a layer, its mask "
        + "or an alpha channel (a colour above white paints as it is on a layer; the bucket's "
        + "tolerance compares the display-encoded copy, a value above white as white; Dodge "
        + "and Burn classify on that copy and change linear light, scaling a value above "
        + "white by what they do to white), "

    static let planeRefusal: String =
        "Painting one colour channel is unavailable in a 32-bit float document. "
        + "Edit ▸ Fill… fills one; paint the layer, its mask or an alpha channel."

    static let coverageInkRefusal: String =
        "rgba32, start_rgba32 and end_rgba32 colour a float layer's pixels. A mask, a channel "
        + "or an adjustment layer's mask is 0–1 coverage: leave them out."

    /// The linear-ink arguments `tool` itself takes: the Brush's one colour,
    /// the Gradient's two ends; the Eraser has no colour.
    static func ownInkKeys(_ tool: String) -> [String] {
        switch tool {
        case "brush_stroke", "fill": return ["rgba32"]
        case "gradient": return ["start_rgba32", "end_rgba32"]
        default: return []
        }
    }

    /// The authored-colour argument a depth refusal points `tool`'s caller to.
    static func colourArgument(_ tool: String) -> String {
        tool == "gradient" ? "start_color and end_color" : "color"
    }

    /// The sentence a linear-ink argument carries outside a float document's
    /// pixels (an 8- or 16-bit document, Quick Mask), naming the argument
    /// passed and the authored colour to pass instead. The master copy:
    /// `AgentServer.linearInkArgument` reads it for `rgba32` and `color`.
    static func depthInkRefusal(key: String, colour: String) -> String {
        "\(key) is a 32-bit float document's colour. Pass \(colour)."
    }

    /// The sentence a linear-ink argument of ANOTHER tool carries, at every
    /// depth: `eraser_stroke rgba32`, `brush_stroke start_rgba32`,
    /// `gradient rgba32`.
    static func foreignInkRefusal(key: String, tool: String) -> String {
        let own = ownInkKeys(tool)
        return "\(tool) takes no \(key)."
            + (own.isEmpty ? " It has no colour."
                : " Its linear ink is " + own.joined(separator: " and ") + ".")
    }

    /// Why `mode` cannot composite ink in a float document; nil when
    /// `NativeLayerBlending.floatModes` contains it.
    static func modeRefusal(_ mode: DrBlendMode) -> String? {
        guard !NativeLayerBlending.floatModes.contains(mode) else { return nil }
        return "The \(DrBlendMode.displayName(for: mode)) blend mode is unavailable in a "
            + "32-bit float document. Paint there in "
            + NativeLayerBlending.floatModeNames.joined(separator: ", ") + "."
    }

    /// The pre-dispatch gate (`DocumentPrecision.refusal`); nil for any tool
    /// outside `tools`, and for every call the handlers themselves answer.
    /// In order: another tool's linear-ink argument, at every depth; the
    /// tool's own outside a float document's pixels; any
    /// other call on an 8- or 16-bit document passes; a `gradient` that edits
    /// the Quick Mask passes; a colour-plane target; a mask or channel target
    /// passes; then, for a layer: Quick Mask, the core's `paint` / `erase`
    /// capability for the entry's kind (a brush or eraser stroke on an
    /// adjustment layer with a mask paints that mask and passes), and the
    /// named `blend_mode` (Dodge/Burn has none).
    static func refusal(tool: String, arguments: [String: Any], document: ImageDocument) -> String? {
        guard tools.contains(tool), let doc = document.doc else { return nil }
        let target = arguments["target"] as? String
        let quickMaskTarget = target == "quick_mask"
        let own = ownInkKeys(tool)
        if let foreign = linearInkKeys.first(where: { arguments[$0] != nil && !own.contains($0) }) {
            return foreignInkRefusal(key: foreign, tool: tool)
        }
        if let key = own.first(where: { arguments[$0] != nil }),
           !doc.isFloat || document.quickMaskActive || quickMaskTarget {
            return depthInkRefusal(key: key, colour: colourArgument(tool))
        }
        guard doc.isFloat else { return nil }
        if tool == "gradient", quickMaskTarget || (document.quickMaskActive && target == nil) {
            return nil
        }
        if let target, let plane = RasterPlane.named(target),
           PaintTarget.paintablePlanes.contains(plane) {
            return planeRefusal
        }
        // A mask or a channel is the 16-bit coverage route; an unknown target
        // is the handler's to name.
        if let target, target != "layer" { return nil }
        if document.quickMaskActive { return OperationScope.quickMaskRefusal }
        let index = DocumentPrecision.integer(arguments["layer"])
            ?? DocumentPrecision.integer(arguments["index"]) ?? document.activeLayerIndex
        if ["brush_stroke", "eraser_stroke"].contains(tool), doc.layerIsAdjustment(index),
           doc.layerHasMask(index) { return nil }
        let kind = doc.layerIsGroup(index) ? "group" : doc.layerIsAdjustment(index) ? "adjustment" : "raster"
        do {
            let report = try RasterDocument.precisionCapability(
                operation: tool == "eraser_stroke" ? "erase" : "paint", target: kind,
                depth: UInt32(32))
            guard report["supported"] as? Bool == true else {
                return report["reason"] as? String ?? FloatDocument.refusal(tool: tool)
            }
        } catch {
            return "Could not validate the 32-bit float operation capability: "
                + error.localizedDescription
        }
        // The Eraser and Dodge/Burn have no blend mode at any depth; their
        // handlers say so.
        if tool != "eraser_stroke", tool != "dodge_burn",
           let name = arguments["blend_mode"] as? String,
           let mode = NativeLayerBlending.mode(named: name) {
            return modeRefusal(mode)
        }
        return nil
    }

    /// The Gradient tool's Foreground → Background ramp as linear ink: both
    /// endpoints as linear light in the document's primaries when `facade` is
    /// a float document and either colour is above white; else nil, and the
    /// ramp takes the document route.
    static func linearEndpoints(
        start: NSColor, end: NSColor, facade: RasterDocument
    ) -> (start: [Double], end: [Double])? {
        guard facade.isFloat,
              ExtendedRangeColor.aboveWhite(start, in: facade) != nil
                  || ExtendedRangeColor.aboveWhite(end, in: facade) != nil,
              let first = ExtendedRangeColor.linearNumbers(start, in: facade),
              let last = ExtendedRangeColor.linearNumbers(end, in: facade)
        else { return nil }
        return (first, last)
    }
}
