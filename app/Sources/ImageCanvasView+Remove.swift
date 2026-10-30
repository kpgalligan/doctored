import AppKit

/// The canvas half of the Remove tool's held strokes (RemoveSession.swift): with
/// "Remove after each stroke" off, Return applies the held strokes and Escape discards them.
extension ImageCanvasView {
    /// Return or Enter applies, Escape cancels, while the Remove tool holds strokes. Inert in
    /// Quick Mask, like the Crop tool's keys: an Apply is a document edit.
    func removeKeyDown(_ event: NSEvent) -> Bool {
        guard tool == .remove, removeSession.hasPending, !quickMaskActive else { return false }
        switch event.keyCode {
        case 53:
            onRemoveCancel?()
            return true
        case 36, 76:
            onRemoveApply?()
            return true
        default:
            return false
        }
    }
}
