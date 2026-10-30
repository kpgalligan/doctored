import AppKit

// Image ▸ Adjustments ▸ Equalize's menu action (#547): the prompt when a selection exists,
// then `Equalize.apply` on the active layer's current target, recorded as an Action step.

extension EditorViewController {
    @objc func equalize(_ sender: Any?) {
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        if document.quickMaskActive { NSSound.beep(); return }
        var scope: EqualizeScope?
        if document.canvasSelection != nil {
            guard let answer = Equalize.prompt() else { return }
            scope = answer
        }
        let target = document.planeEditTarget
        let layer = document.activeLayerIndex
        if target == .layer || target.isPlane,
           !document.confirmRasterize(layer: layer) { return }
        guard doc === document.doc else { NSSound.beep(); return }
        do {
            _ = try Equalize.apply(document, layer: layer, target: target, scope: scope, record: true)
        } catch {
            document.presentError(error)
        }
    }
}
