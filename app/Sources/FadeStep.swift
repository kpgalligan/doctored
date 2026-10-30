import Foundation

// Edit ▸ Fade's model (v1, `designs/v1-shortcuts-fade.md`): which steps can be faded, the hint a
// step leaves for `ImageDocument.noteFadeCandidate`, and the candidate it becomes. The candidate's
// validity, the faded result and the commit are `ImageDocument+Fade.swift`; the dialog is
// `FadeSheetController`, the menu item `EditorViewController+Fade.swift`, MCP `AgentServer+Fade`.

/// How a step's fade is weighed: a REGION step (a filter, an adjustment, a
/// selection fill or gradient) by the selection it ran under, a STROKE (and
/// the Paint Bucket's flood) by the pixels it changed — a stroke's coverage is
/// not the selection, and `stroke_selection`'s Outside and Center bands paint
/// where the selection's coverage is 0.
enum FadeKind {
    case region, stroke
}

/// What a fadeable step tells `noteFadeCandidate`: its kind, and the layer it
/// edited when it names one (nil: the active layer).
struct FadeHint {
    let kind: FadeKind
    let layer: Int?
}

/// The step Edit ▸ Fade can blend back: the document before and after it,
/// and the selection, layer selection and history entry it left, any change
/// of which makes it unavailable (`ImageDocument.currentFadeCandidate`).
struct FadeCandidate {
    let name: String
    let kind: FadeKind
    let layer: Int
    let before: RasterDocument
    let after: RasterDocument
    let selection: SelectionHistoryState
    let layerSelection: LayerSelection
    let historyID: String?
}

enum FadeStep {
    /// The tools whose step fades by the selection's coverage.
    static let regionTools: Set<String> = [
        "apply_filter", "auto_tone", "auto_contrast", "auto_color", "liquify",
        "fill_selection", "gradient",
    ]

    /// The tools whose step fades by the pixels it changed. The Paint Bucket
    /// (`fill`) is one: its flood reaches only part of the selection.
    static let strokeTools: Set<String> = [
        "stroke_selection", "fill", "brush_stroke", "eraser_stroke", "clone_stamp", "heal_stroke",
        "spot_heal_stroke", "dodge_burn", "blur_stroke", "sharpen_stroke", "smudge_stroke",
        "sponge_stroke", "history_brush_stroke", "art_history_brush_stroke",
    ]

    /// The refusal for a Fade with nothing to fade — the MCP `fade` tool's
    /// in-band error and the menu item's tooltip.
    static let nothingToFade =
        "There is nothing to fade: Edit ▸ Fade follows a filter, an adjustment, a paint or "
        + "retouch stroke or a fill on a layer, before any other change."

    /// The hint a call of `tool` with `arguments` leaves: nil for a tool that
    /// is not fadeable and for one aimed at a mask, a plane, a channel or the
    /// Quick Mask (`target` other than "layer").
    static func hint(tool: String, arguments: [String: Any]) -> FadeHint? {
        let kind: FadeKind
        if regionTools.contains(tool) {
            kind = .region
        } else if strokeTools.contains(tool) {
            kind = .stroke
        } else {
            return nil
        }
        if let target = arguments["target"], (target as? String) != "layer" { return nil }
        return FadeHint(kind: kind, layer: layerIndex(arguments["layer"])
            ?? layerIndex(arguments["index"]))
    }

    /// The hint of a recorded edit: exactly one step, which must be fadeable.
    static func hint(for record: [ActionStep]) -> FadeHint? {
        guard record.count == 1, let step = record.first else { return nil }
        return hint(tool: step.tool, arguments: step.arguments)
    }

    /// An integer layer argument (a number, or its string form); nil for the
    /// recorded `{"$layer": …}` reference and anything else, which means the
    /// active layer.
    private static func layerIndex(_ value: Any?) -> Int? {
        if let text = value as? String { return Int(text) }
        guard let number = value as? NSNumber,
              CFGetTypeID(number) != CFBooleanGetTypeID() else { return nil }
        return number.intValue
    }
}
