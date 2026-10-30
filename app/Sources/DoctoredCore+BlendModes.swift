import AppKit

extension DrBlendMode {
    /// The full blend-mode set in menu order, grouped the way the menu draws
    /// its separators. The layers panel builds its popup from these groups;
    /// display titles differ from the C constant names only for ADDITION,
    /// which shows as "Linear Dodge (Add)".
    static let blendModeGroups: [[(DrBlendMode, String)]] = [
        [
            (DR_BLEND_NORMAL, "Normal"),
            (DR_BLEND_DISSOLVE, "Dissolve"),
        ],
        [
            (DR_BLEND_DARKEN, "Darken"),
            (DR_BLEND_MULTIPLY, "Multiply"),
            (DR_BLEND_COLOR_BURN, "Color Burn"),
            (DR_BLEND_LINEAR_BURN, "Linear Burn"),
            (DR_BLEND_DARKER_COLOR, "Darker Color"),
        ],
        [
            (DR_BLEND_LIGHTEN, "Lighten"),
            (DR_BLEND_SCREEN, "Screen"),
            (DR_BLEND_COLOR_DODGE, "Color Dodge"),
            (DR_BLEND_ADDITION, "Linear Dodge (Add)"),
            (DR_BLEND_LIGHTER_COLOR, "Lighter Color"),
        ],
        [
            (DR_BLEND_OVERLAY, "Overlay"),
            (DR_BLEND_SOFT_LIGHT, "Soft Light"),
            (DR_BLEND_HARD_LIGHT, "Hard Light"),
            (DR_BLEND_VIVID_LIGHT, "Vivid Light"),
            (DR_BLEND_LINEAR_LIGHT, "Linear Light"),
            (DR_BLEND_PIN_LIGHT, "Pin Light"),
            (DR_BLEND_HARD_MIX, "Hard Mix"),
        ],
        [
            (DR_BLEND_DIFFERENCE, "Difference"),
            (DR_BLEND_EXCLUSION, "Exclusion"),
            (DR_BLEND_SUBTRACT, "Subtract"),
            (DR_BLEND_DIVIDE, "Divide"),
        ],
        [
            (DR_BLEND_HUE, "Hue"),
            (DR_BLEND_SATURATION, "Saturation"),
            (DR_BLEND_COLOR, "Color"),
            (DR_BLEND_LUMINOSITY, "Luminosity"),
        ],
    ]

    /// Flat menu-order list for callers that don't care about grouping.
    /// Deliberately WITHOUT Pass Through: this list feeds the brush, clone
    /// and Apply Image menus and the agent catalog's shared blend vocabulary,
    /// where a group's declaration has no meaning (the inverse of the
    /// gray-degenerate rule below, which subtracts from this list instead).
    static let allBlendModes: [(DrBlendMode, String)] = blendModeGroups.flatMap { $0 }

    /// The vocabulary a GROUP entry offers: Pass Through — the
    /// default for a new group, and the mode that lets an adjustment layer
    /// inside the group reach the layers below it — ahead of every ordinary
    /// mode. The layers-panel header popup and `set_layer_properties` use
    /// this when the entry is a group; nothing else does, because the core
    /// refuses Pass Through on a raster layer.
    static let groupBlendModes: [(DrBlendMode, String)] =
        [(DR_BLEND_PASS_THROUGH, "Pass Through")] + allBlendModes

    /// `groupBlendModes`' names alone, in the same order.
    static let groupBlendNames: [String] = groupBlendModes.map { $0.1 }

    /// The four HSL modes, which carry NO information on a SINGLE 8-bit
    /// plane: the W3C defines them over an RGB triple, and a gray triple has
    /// zero saturation, so Hue, Saturation and Color answer the base verbatim
    /// while Luminosity collapses to Normal. `dr_blend_planes` refuses them
    /// for that reason (see the header's "Channels" section), so anything
    /// offering blend modes for one plane — Calculations, and Apply Image
    /// onto a colour plane or an alpha channel — leaves them out. Three
    /// planes blended as one colour (`RasterPlaneMath.blendRGB`) accept them:
    /// that is the form in which they mean something.
    static let grayDegenerateModes: [DrBlendMode] = [
        DR_BLEND_HUE, DR_BLEND_SATURATION, DR_BLEND_COLOR, DR_BLEND_LUMINOSITY,
    ]

    /// True for a mode `dr_blend_planes` refuses on one plane.
    static func degeneratesOnGray(_ mode: DrBlendMode) -> Bool {
        grayDegenerateModes.contains { $0 == mode }
    }

    /// Display name for a mode (status bar, layer-row meta lines).
    ///
    /// Pass Through is answered AHEAD of the table lookup: it is absent from
    /// `allBlendModes` on purpose, and the lookup's `?? "Normal"` fallback
    /// would otherwise report a pass-through group as Normal — which is the
    /// one thing it is not, and would make `get_document` contradict the
    /// core.
    static func displayName(for mode: DrBlendMode) -> String {
        if mode == DR_BLEND_PASS_THROUGH { return "Pass Through" }
        return allBlendModes.first { $0.0 == mode }?.1 ?? "Normal"
    }
}
