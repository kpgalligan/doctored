import AppKit

// Image ▸ Adjustments, Layer ▸ New Adjustment Layer and Image ▸ Auto on a Grayscale document
// (#547 phase 2b): which ops run — on R = G = B through the RGB kernels, destructively and as a
// layer where the op has one — and the sentence each refused op carries. The core's twin is
// `gray_adjust::GRAY_ADJUSTMENTS`, in the same words. Since phase 2c the Filter menu runs too
// (`allowsFilterAction`): every filter (Liquify since phase 4), on R = G = B through the RGB
// kernels, a kernel's colour folded to one channel by the core (`gray_filter`).

enum GrayAdjust {
    /// What a Gray document does with one op.
    enum Availability: Equatable {
        /// Runs on the gray values; `layer` = also as an adjustment layer.
        case runs(layer: Bool)
        /// Unavailable in a Grayscale document: refused for good.
        case unavailable
        /// A Doctored colour adjustment offered on RGB documents only.
        case rgbOnly
    }

    /// The core's table, plus Image ▸ Auto and Equalize.
    static let adjustments: [String: Availability] = [
        "bcs": .runs(layer: true),
        "levels": .runs(layer: true),
        "curves": .runs(layer: true),
        "exposure": .runs(layer: true),
        "invert": .runs(layer: true),
        "posterize": .runs(layer: true),
        "threshold": .runs(layer: true),
        "gradient_map": .runs(layer: true),
        "shadows_highlights": .runs(layer: false),
        "replace_color": .runs(layer: false),
        "auto_tone": .runs(layer: false),
        "auto_contrast": .runs(layer: false),
        "equalize": .runs(layer: false),
        "vibrance": .unavailable,
        "hue_saturation": .unavailable,
        "color_balance": .unavailable,
        "black_and_white": .unavailable,
        "photo_filter": .unavailable,
        "channel_mixer": .unavailable,
        "color_lookup": .unavailable,
        "selective_color": .unavailable,
        "auto_color": .unavailable,
        "hue_rotate": .rgbOnly,
        "grayscale": .rgbOnly,
        "sepia": .rgbOnly,
        "white_balance": .rgbOnly,
    ]

    /// Whether `op` runs on a Gray document of `depth`, as an adjustment layer when `layer`. At
    /// 32 bits only what the float document evaluates: Auto refuses there, Equalize runs.
    static func allows(_ op: String, layer: Bool, depth: Int) -> Bool {
        guard case .runs(let hasLayer) = adjustments[op], !layer || hasLayer else { return false }
        guard depth == 32 else { return true }
        if op == "equalize" { return true }
        if op.hasPrefix("auto_") { return false }
        return layer ? FloatDocument.adjustmentOps.contains(op) : FloatDocument.destructiveOps.contains(op)
    }

    /// The core's sentence for `op` (as a layer when `layer`).
    static func refusal(op: String, layer: Bool) -> String {
        let name = CmykAdjustFilter.displayName(op)
        switch adjustments[op] {
        case .unavailable?:
            return "\(name) is unavailable in a Grayscale document."
        case .rgbOnly?:
            return "\(name) is unavailable in a Grayscale document: Doctored offers it on RGB "
                + "documents only."
        case .runs(let hasLayer)? where layer && !hasLayer:
            return "A \(name) adjustment layer is unavailable in a Grayscale document: \(name) "
                + "applies to pixels only."
        default:
            return GrayDocument.refusal(tool: "The \"\(op)\" adjustment")
        }
    }

    /// The answer for an adjustment menu item — tagged (`showAdjustmentSheet:`,
    /// `newAdjustmentLayerOp:`) or one of `CmykAdjustFilter.legacySelectors` — or nil for any
    /// other item.
    static func allowsAction(_ action: String, tag: Int, depth: Int) -> Bool? {
        if ["showAdjustmentSheet:", "newAdjustmentLayerOp:"].contains(action) {
            guard let op = AdjustmentMenuOrder.op(for: tag) else { return false }
            return allows(op.rawValue, layer: action == "newAdjustmentLayerOp:", depth: depth)
        }
        return CmykAdjustFilter.legacySelectors[action].map {
            allows($0.op, layer: $0.layer, depth: depth)
        }
    }

    /// Whether the Filter menu item `action` (one of `FloatDocument.filterActions`) runs on a Gray
    /// document of `depth`: every filter (Liquify since phase 4), at 32 bits only the float
    /// document's (`FloatDocument.filterOps`).
    static func allowsFilterAction(_ action: String, depth: Int) -> Bool {
        guard let op = FloatDocument.filterActions[action] else { return false }
        return depth != 32 || FloatDocument.filterOps.contains(op)
    }

    /// An MCP adjustment call's permanent refusal (the disabled ops, the RGB-only ones,
    /// a layer of a pixels-only op), asked before `GrayDocument.tools`. A filter (phase 2c) is
    /// admitted here, Liquify too (phase 4; `gray_filter::admit_filter`).
    static func toolRefusal(tool: String, arguments: [String: Any]) -> String? {
        switch tool {
        case "apply_filter":
            let filter = arguments["filter"] as? String ?? ""
            let op = filter == "adjust" ? "bcs" : filter
            guard let entry = adjustments[op] else { return nil }
            // Image ▸ Auto and Equalize run as their own tools; Auto Color refuses below.
            if op.hasPrefix("auto_") || op == "equalize", case .runs = entry {
                return "apply_filter \(filter) is unavailable in a Grayscale document: run the "
                    + "\(filter) tool (its Image menu item) instead."
            }
            if case .runs = entry { return nil }
            return refusal(op: op, layer: false)
        case "add_adjustment_layer", "edit_adjustment_layer":
            guard let op = arguments["op"] as? String, let entry = adjustments[op] else { return nil }
            if case .runs(true) = entry { return nil }
            return refusal(op: op, layer: true)
        case "auto_color":
            return refusal(op: "auto_color", layer: false)
        default:
            return nil
        }
    }
}
