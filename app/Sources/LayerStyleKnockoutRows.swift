import AppKit

// The Blending Options pane SETS `knockout` and `blendInteriorAsGroup` since 12E-3b (this file: the
// Knockout popup, the Blend Interior Effects as Group checkbox and their
// `NSUserInterfaceItemIdentifier`s). The same rows appear at 8 and 16 bits; the flag evaluates ON
// on a clipping base with visible clipped layers too (P7-39), so no note
// qualifies it.

/// The Blending Options pane's KNOCKOUT rows — the two remaining blending
/// options, and the last controls the Layer Style sheet was missing
/// (Phase 12E-3b, contract e).
///
/// The CORE owns every rule (`core/src/style_knockout.rs`, D3–D8); this file
/// owns two things and nothing else: the two controls and the identifiers a
/// UI-source probe drives them by.
///
/// The rows are the same at 8 and 16 bits. There is no depth branch here: the
/// typed document carries both keys since 12E-3b, and the sheet's single
/// difference at depth — how it previews (`PrecisionStylePreview` at 16 bits,
/// nothing at 32-bit float) — belongs to `LayerStyleSheetController`, not to
/// a row.
enum LayerStyleKnockoutRows {
    /// The Knockout popup. A UI-source probe finds it with
    /// `NSView.withIdentifier`, so this string is a test contract.
    static let knockoutIdentifier = NSUserInterfaceItemIdentifier("layerStyle.knockout")
    /// The "Blend Interior Effects as Group" checkbox.
    static let blendInteriorIdentifier = NSUserInterfaceItemIdentifier("layerStyle.blendInterior")
    /// The sheet's Apply button, so a probe can commit without a mouse.
    static let applyIdentifier = NSUserInterfaceItemIdentifier("layerStyle.apply")

    /// The popup's titles and the JSON values behind them, in order. The values
    /// ARE the style JSON's strings (`Knockout`'s raw values), so nothing maps
    /// between two vocabularies.
    static let titles = ["None", "Shallow", "Deep"]
    static let values = [Knockout.none, .shallow, .deep].map { $0.rawValue }

    /// The rows the Blending Options pane appends under Fill Opacity: the
    /// popup and the checkbox.
    static func rows(
        knockout: EffectBinding<Knockout>, blendInterior: EffectBinding<Bool>
    ) -> [PaneRow] {
        let popup = PaneRows.popup(
            "Knockout:", titles: titles, values: values,
            get: { knockout.read().rawValue },
            set: { knockout.write(Knockout(rawValue: $0) ?? .none) })
        popup.views.last?.identifier = knockoutIdentifier
        let checkbox = PaneRows.checkbox(
            "Blend Interior Effects as Group",
            get: { blendInterior.read() }, set: { blendInterior.write($0) })
        checkbox.views.last?.identifier = blendInteriorIdentifier
        return [popup, checkbox]
    }
}

extension RasterDocument {
    /// Whether entry `idx` is a CLIPPING BASE whose run holds a layer the
    /// document marks visible — the isolation question, asked the way
    /// `doc_group::composite_unit_into` asks it: the consecutive run of
    /// CLIPPED siblings immediately above `idx` in its own level, and never
    /// across a group boundary. A clipped entry is never a base itself (a
    /// clipped entry at the bottom of its level is baseless and composites as
    /// if unclipped, with no members of its own).
    func isClipBaseWithVisibleMembers(_ idx: Int) -> Bool {
        guard idx >= 0, idx < layerCount, !layerClipped(idx) else { return false }
        let siblings = layerTree.siblings(of: idx)
        guard let position = siblings.firstIndex(of: idx) else { return false }
        for sibling in siblings[siblings.index(after: position)...] {
            guard layerClipped(sibling) else { break }
            if layerInfo(sibling)?.visible == true { return true }
        }
        return false
    }
}
