import AppKit

/// Layer ▸ Replace Missing Fonts…: the action and its validation. The sheet and the op are
/// ReplaceFontsSheetController.swift and MissingFonts.swift; the MCP twin is
/// `replace_missing_fonts` (AgentServer+MissingFonts.swift).
extension EditorViewController {
    @objc func replaceMissingFonts(_ sender: Any?) {
        guard let document else { return }
        // Never leave a canvas session hanging behind the sheet.
        guard commitPendingSessions() else { return }
        do {
            let sheet = try ReplaceFontsSheetController(document: document, preferred: fontFamily)
            presentAsSheet(sheet)
        } catch { document.presentError(error) }
    }

    /// Enabled while a text layer names a font family this Mac lacks.
    func validateReplaceFontsItem() -> Bool {
        document?.doc?.missingFontLayers().isEmpty == false
    }
}
