import AppKit

/// Edit ▸ Fill… with Pattern contents at 8 bits: `applyFill` branches here once the layer's
/// refusals pass. The tile (`FillSettings.pattern`, resolved when the sheet applied) sits on the
/// canvas grid at 100 %, unturned, read per pixel by the core in place of one colour
/// (`dr_doc_fill_selection_pattern`), as one "Fill" undo step recorded as `fill_selection` with
/// the resolved `pattern`. A mask, colour plane or alpha channel refuses by name: it holds one
/// gray per pixel. The 16 and 32-bit and colour-mode commit is
/// `EditorViewController+PrecisionFillStroke.swift`'s; the MCP twin is `fill_selection`'s
/// `pattern` (AgentServer+FillStroke.swift).
extension EditorViewController {
    func applyPatternFill(
        _ settings: FillSettings, capture: OperationCapture, document: ImageDocument, note: String
    ) {
        guard !capture.target.isCoverage else {
            showFillAlert(FillSettings.patternCoverageRefusal)
            return
        }
        guard let pattern = settings.pattern, let tile = pattern.tileBytes() else {
            NSSound.beep()
            return
        }
        let opacity = min(max(settings.opacity, 0), 100) / 100
        let preserve = settings.preserveTransparency
        let record: [ActionStep] = .fillSelection(
            pattern: pattern, blendMode: settings.mode, opacity: opacity * 100,
            preserveTransparency: preserve,
            target: capture.target.layerEditAgentName(in: capture.doc), note: note)
        // A fill reaching past a trimmed layer grows it, as a colour fill does.
        let base: RasterDocument
        do {
            base = try capture.paintDestination(mask: capture.selection, preserve: preserve)
        } catch {
            document.presentError(error)
            NSSound.beep()
            return
        }
        var unchanged = false
        document.applyCapturedRewrite(capture, "Fill", record: record) {
            let updated = base.selectionFilledPattern(
                capture.layerIndex, mask: capture.selection, tile: tile.rgba,
                tileWidth: tile.width, tileHeight: tile.height, mode: settings.mode,
                opacity: opacity, preserveTransparency: preserve)
            unchanged = updated == nil
            return updated
        }
        guard unchanged else { return }
        showUnchangedAlert(
            preserve: preserve, doc: capture.doc, layer: capture.layerIndex,
            noOp: FillSettings.transparentNoOp, clippedNoOp: nil)
    }
}
