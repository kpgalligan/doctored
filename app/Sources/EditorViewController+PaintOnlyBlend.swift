import AppKit

/// The UI's side of Behind and Clear (`PaintOnlyBlend`): the refusal Fill…,
/// Stroke… and the Brush ask before the core, at 8, 16 and 32 bits, and the
/// sheets' Mode popups. The MCP twins ask the same `PaintOnlyBlend.refusal`
/// (`selectionPaintArguments`, `resolvePaintTarget`, `precisionPaint`).
extension EditorViewController {
    /// Behind or Clear refused by name on `capture`'s target (a coverage
    /// target, Preserve Transparency, a transparency lock, a colour mode
    /// without them), with the fill alert; false for any other mode.
    func refusePaintOnlyBlend(_ mode: DrBlendMode, capture: OperationCapture, preserve: Bool) -> Bool {
        refusePaintOnlyBlend(mode, doc: capture.doc, layer: capture.layerIndex,
                             coverage: capture.target.isCoverage, preserve: preserve)
    }

    /// The same refusal for a typed capture or a Brush stroke's layer.
    func refusePaintOnlyBlend(
        _ mode: DrBlendMode, doc: RasterDocument?, layer: Int, coverage: Bool, preserve: Bool
    ) -> Bool {
        guard let reason = PaintOnlyBlend.refusal(
            mode, doc: doc, layer: layer, coverage: coverage, preserve: preserve)
        else { return false }
        showFillAlert(reason)
        return true
    }

    /// Whether the Fill… and Stroke… sheets list Behind and Clear: a layer
    /// target in a colour mode that offers them.
    static func sheetOffersPaintOnly(doc: RasterDocument?, coverage: Bool) -> Bool {
        !coverage && PaintOnlyBlend.offered(in: doc)
    }
}
