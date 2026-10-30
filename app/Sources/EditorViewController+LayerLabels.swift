import AppKit

// Layer ▸ Label and the row menu's Label ▸ share one `@objc setLayerLabel(_:)` whose sender TAG is
// the code, applied over the whole selection in one undo step. A label is lock-EXEMPT, like a
// rename.

/// Layer ▸ Label and the layers panel's row menu ▸ Label. One handler for
/// both, the lock submenu's shape exactly: the sender's TAG is the label's
/// code, the item applies it to the whole selection in ONE undo step, and the
/// checkmark shows the PRIMARY entry's label.
///
/// A label is EXEMPT from the layer locks, like a rename: it marks a row in
/// the panel and changes no pixel, so a locked layer can still be labelled.
extension EditorViewController {
    @objc func setLayerLabel(_ sender: Any?) {
        guard let document, document.doc != nil,
              let tag = (sender as? NSMenuItem)?.tag,
              let label = LayerLabel(rawValue: UInt8(clamping: tag)) else {
            NSSound.beep()
            return
        }
        let name = label == .none ? "Clear Label" : "Label \(label.displayName)"
        let record: [ActionStep] = .setLayerLabel(
            label.name, layers: document.selectedLayerIndices.count > 1)
        document.applyToSelectedLayers(name, record: record, renderNeutral: true) { doc, idx in
            try doc.withLayerLabel(idx, label)
        }
    }

    /// The primary entry's label — what the menu's checkmarks show.
    var activeLayerLabel: LayerLabel {
        guard let document, let doc = document.doc else { return .none }
        return doc.layerLabel(document.activeLayerIndex)
    }

    /// The validation `EditorViewController.validateUserInterfaceItem` wires
    /// one line to: enabled whenever there is a document, with the radio mark
    /// on the primary entry's current label.
    func validateLayerLabelItem(_ item: NSValidatedUserInterfaceItem) -> Bool {
        if let menuItem = item as? NSMenuItem {
            menuItem.state = Int(activeLayerLabel.rawValue) == menuItem.tag ? .on : .off
        }
        return document?.doc != nil
    }
}
