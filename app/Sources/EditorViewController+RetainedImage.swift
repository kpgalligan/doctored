import AppKit

extension EditorViewController {
    @objc func placeEmbeddedImage(_ sender: Any?) { chooseImageSource(.embedded) }

    /// Layer ▸ Rasterize RAW Layer: drops the camera original and its
    /// development, keeping the layer's pixels exactly. One undo step.
    @objc func rasterizeEmbeddedImage(_ sender: Any?) {
        guard let document else { NSSound.beep(); return }
        guard commitPendingSessions() else { return }
        let index = document.activeLayerIndex
        guard !refuseLockedEdit(layer: index, kind: DR_EDIT_PIXELS) else { return }
        do {
            try RetainedImageWork.rasterize(in: document, layer: index, record: .rasterizeImageSource())
        } catch { document.presentError(error) }
    }
}
