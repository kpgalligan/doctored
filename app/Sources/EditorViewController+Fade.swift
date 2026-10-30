import AppKit

// Edit ▸ Fade… (⇧⌘F, v1): the menu item and its validation. The candidate rules are
// `ImageDocument+Fade.swift`; the dialog is `FadeSheetController`; the MCP twin `fade`.

extension EditorViewController {
    /// Edit ▸ Fade… — the last filter, adjustment, stroke or fill blended
    /// with the state before it. MCP twin `fade`.
    @objc func showFade(_ sender: Any?) {
        guard let candidate = document?.currentFadeCandidate, !canvas.hasActiveTextSession,
              !isTransforming, shapeEditSession == nil else {
            NSSound.beep()
            return
        }
        presentAsSheet(FadeSheetController(editor: self, candidate: candidate))
    }

    /// Enabled only while the last edit can still be faded, and retitled
    /// "Fade <step>…" after it; "Fade…" otherwise.
    func validateFadeItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        let candidate = document?.currentFadeCandidate
        if let menuItem = item as? NSMenuItem {
            menuItem.title = candidate.map { "Fade \($0.name)…" } ?? "Fade…"
            menuItem.toolTip = candidate == nil ? FadeStep.nothingToFade : nil
        }
        return candidate != nil && !canvas.hasActiveTextSession && !isTransforming
            && shapeEditSession == nil
    }
}
