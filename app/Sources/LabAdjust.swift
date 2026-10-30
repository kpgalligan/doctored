import AppKit

// Image ▸ Adjustments, Layer ▸ New Adjustment Layer, Image ▸ Auto and the Filter menu on a Lab
// Color document (#1 Lab phases 2b to 2f and 4a-ii): which ops run — the RGB kernels in their Lab form,
// bound by the core at parse (Levels and Curves on Lightness, their r, g and b on Lightness, a and
// b; Brightness/Contrast on Lightness, its saturation on a* and b*; Invert and Posterize on every
// channel; Threshold black and white; Desaturate keeping L*; since phase 2d Hue/Saturation, Color
// Balance, Photo Filter, Shadows/Highlights and Replace Color on the sample's 16-bit ProPhoto RGB
// colour, Color Lookup and Gradient Map on its sRGB colour) — and the sentence each refused op
// carries. The Hue/Saturation sheet's sampler reads the colour that kernel reads
// (`CanvasSample.kernelRGB`); Replace Color's colours are the document's own `#LLaabb` codes. The
// core's twins are `lab_adjust::LAB_ADJUSTMENTS` and `lab_filter::LAB_FILTERS`, in the same words.
// Since phase 2c the
// Filter menu runs (`allowsFilterAction`): every filter on the L*a*b* codes through its kernel,
// each channel alone, but the `filters` table's (the disabled ones, two Develop Filter
// controls and Depth Blur), and since phase 4a-ii Liquify on the layer; since phase 2f the five
// luma filters (Emboss,
// Edge Detect, Lens Blur, Reduce Noise, Smart Sharpen) run too, in the core's one-channel passes
// (`lab_channel_filter`). Since phase 2d a layer mask or an alpha channel, and since phase 2e the Lightness, a, b
// and alpha planes, run every filter as on RGB but the rows that refuse for good and Liquify,
// which edits layer pixels only, as on RGB (`scalarFilterRefusal`, `allowsScalarFilterAction`).

enum LabAdjust {
    /// What a Lab document does with one op.
    enum Availability: Equatable {
        /// Runs on the L*a*b* codes; `layer` = also as an adjustment layer.
        case runs(layer: Bool)
        /// Unavailable in a Lab Color document: refused for good.
        case unavailable
        /// A Doctored colour adjustment or filter offered on RGB documents only.
        case rgbOnly
        /// A Develop Filter control, unavailable in Lab with the Develop Filter.
        case developFilter
    }

    /// The core's table, plus Image ▸ Auto, Equalize and HDR Toning.
    static let adjustments: [String: Availability] = [
        "bcs": .runs(layer: true),
        "levels": .runs(layer: true),
        "curves": .runs(layer: true),
        "invert": .runs(layer: true),
        "posterize": .runs(layer: true),
        "threshold": .runs(layer: true),
        "hue_saturation": .runs(layer: true),
        "color_balance": .runs(layer: true),
        "photo_filter": .runs(layer: true),
        "color_lookup": .runs(layer: true),
        "gradient_map": .runs(layer: true),
        "shadows_highlights": .runs(layer: false),
        "replace_color": .runs(layer: false),
        "auto_tone": .runs(layer: false),
        "auto_contrast": .runs(layer: false),
        "equalize": .runs(layer: false),
        "exposure": .unavailable,
        "vibrance": .unavailable,
        "black_and_white": .unavailable,
        "channel_mixer": .unavailable,
        "selective_color": .unavailable,
        "auto_color": .unavailable,
        "hdr_toning": .unavailable,
        "hue_rotate": .rgbOnly,
        "grayscale": .rgbOnly,
        "sepia": .rgbOnly,
        "white_balance": .rgbOnly,
    ]

    /// The core's filter table (`lab_filter::LAB_FILTERS`); a filter outside it runs.
    static let filters: [String: Availability] = [
        "lens_correction": .unavailable,
        "develop_filter": .unavailable,
        "difference_clouds": .unavailable,
        "presence": .developFilter,
        "add_grain": .developFilter,
        "depth_blur": .rgbOnly,
    ]

    /// The ops applied to pixels only (the core's `Pixels` rows): their layer refuses.
    static let pixelsOnly: Set<String> = ["shadows_highlights", "replace_color"]

    /// Whether `op` runs on a Lab document, as an adjustment layer when `layer`.
    static func allows(_ op: String, layer: Bool) -> Bool {
        guard case .runs(let hasLayer) = adjustments[op] else { return false }
        return !layer || hasLayer
    }

    /// Whether the Filter menu item `action` (one of `FloatDocument.filterActions`) runs on a Lab
    /// document: every filter but the `filters` table's (Liquify on the layer since phase 4a-ii).
    static func allowsFilterAction(_ action: String) -> Bool {
        guard let op = FloatDocument.filterActions[action] else { return false }
        return filters[op] == nil
    }

    /// The same rule for a layer mask or an alpha channel target (`scalarFilterRefusal`).
    static func allowsScalarFilterAction(_ action: String) -> Bool {
        guard let op = FloatDocument.filterActions[action] else { return false }
        return scalarFilterRefusal(op: op) == nil
    }

    /// The core's sentence for `op` (as a layer when `layer`).
    static func refusal(op: String, layer: Bool = false) -> String {
        let name = op == "hdr_toning" ? HdrToning.title : CmykAdjustFilter.displayName(op)
        switch adjustments[op] {
        case .unavailable?:
            return unavailableSentence(name)
        case .rgbOnly?:
            return rgbOnlySentence(name)
        case .runs? where layer && pixelsOnly.contains(op):
            return "A \(name) adjustment layer is unavailable in a Lab Color document: \(name) "
                + "applies to pixels only."
        case .runs?, .developFilter?, nil:
            return LabDocument.refusal(tool: "The \"\(op)\" adjustment")
        }
    }

    /// The core's sentence for a filter of `filters`; nil for one that runs.
    static func filterRefusal(op: String) -> String? {
        guard let entry = filters[op] else { return nil }
        return rowSentence(op: op, entry)
    }

    /// The core's sentence for a filter on a layer mask, an alpha channel or (phase 2e) a plane
    /// (`lab_filter::scalar_refusal`): `filterRefusal`'s rows, and Liquify's in the core's scalar
    /// wording (`precision_scalar_filter::LIQUIFY_REFUSAL`, as on RGB); nil for one that runs.
    static func scalarFilterRefusal(op: String) -> String? {
        if op == "liquify" { return "liquify edits layer pixels only" }
        guard let entry = filters[op] else { return nil }
        return rowSentence(op: op, entry)
    }

    /// One `filters` row's sentence, shared by `filterRefusal` and `scalarFilterRefusal`.
    private static func rowSentence(op: String, _ entry: Availability) -> String {
        let name = FloatDocument.filterNames[op] ?? op
        switch entry {
        case .unavailable:
            return unavailableSentence(name)
        case .developFilter:
            return "\(name) is unavailable in a Lab Color document: it is a Develop Filter "
                + "control, and the Develop Filter is unavailable in Lab."
        case .rgbOnly:
            return rgbOnlySentence(name)
        case .runs:
            return LabDocument.refusal(tool: name)
        }
    }

    private static func unavailableSentence(_ name: String) -> String {
        "\(name) is unavailable in a Lab Color document."
    }

    private static func rgbOnlySentence(_ name: String) -> String {
        "\(name) is unavailable in a Lab Color document: Doctored offers it on RGB documents only."
    }

    /// The answer for an adjustment menu item — tagged (`showAdjustmentSheet:`,
    /// `newAdjustmentLayerOp:`) or one of `CmykAdjustFilter.legacySelectors` — or nil for any
    /// other item.
    static func allowsAction(_ action: String, tag: Int) -> Bool? {
        if ["showAdjustmentSheet:", "newAdjustmentLayerOp:"].contains(action) {
            guard let op = AdjustmentMenuOrder.op(for: tag) else { return false }
            return allows(op.rawValue, layer: action == "newAdjustmentLayerOp:")
        }
        return CmykAdjustFilter.legacySelectors[action].map { allows($0.op, layer: $0.layer) }
    }

    /// An MCP adjustment or filter call's refusal by the tables (the disabled ops, the
    /// RGB-only ones, the Develop Filter controls, a layer of a pixels-only op), asked before
    /// `LabDocument.tools`. Any other filter is admitted here (Liquify runs on the layer since
    /// phase 4a-ii). On a mask or
    /// `channel:<name>` target (phase 2d) and on a `lightness`, `a`, `b` or `alpha` plane (phase
    /// 2e) a filter answers `scalarFilterRefusal`, the menu's scalar rule, so the menu, MCP and
    /// Repeat Last Filter agree; every adjustment runs there as on RGB (the core's scalar path
    /// parses it unbound).
    static func toolRefusal(tool: String, arguments: [String: Any]) -> String? {
        switch tool {
        case "apply_filter":
            // A missing filter is the handler's own argument error, not a Lab refusal.
            guard let filter = arguments["filter"] as? String else { return nil }
            if let target = arguments["target"] as? String,
               ["mask", "lightness", "a", "b", "alpha"].contains(target)
                   || target.hasPrefix("channel:") {
                return scalarFilterRefusal(op: filter)
            }
            if let reason = filterRefusal(op: filter) { return reason }
            let op = filter == "adjust" ? "bcs" : filter
            guard let entry = adjustments[op] else { return nil }
            if case .runs = entry { return nil }
            return refusal(op: op, layer: false)
        case "add_adjustment_layer", "edit_adjustment_layer":
            guard let op = arguments["op"] as? String, let entry = adjustments[op] else { return nil }
            if case .runs(true) = entry { return nil }
            // Not a layer op on any document: the handler's own error answers, as on RGB.
            if ["auto_tone", "auto_contrast", "equalize"].contains(op) { return nil }
            return refusal(op: op, layer: true)
        case "depth_blur":
            return filterRefusal(op: "depth_blur")
        case "auto_color", "hdr_toning":
            return refusal(op: tool, layer: false)
        default:
            return nil
        }
    }
}
