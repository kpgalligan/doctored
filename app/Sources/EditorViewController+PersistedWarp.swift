import AppKit

extension EditorViewController {
    @objc func showPersistedWarp(_ sender: Any?) {
        guard let sheet = PersistedWarpSheetController(editor: self) else {
            document?.presentError(DoctoredCoreError(message: "Warp is unavailable without a document."))
            return
        }
        presentAsSheet(sheet)
    }

    func commitPersistedWarp(_ request: PersistedWarpRequest, source: RasterDocument) {
        guard let document else { NSSound.beep(); return }
        do {
            let work = PersistedWarpWork(source: source, request: request,
                                         indexedBackground: backgroundColor)
            guard let prepared = try work.wait(in: document, title: "Warp Layer") else { return }
            document.preparedEdit = prepared
            document.applyEdit("Warp Layer", record: .persistedWarp(request),
                               allowWarpChange: true) { _ in prepared.document }
        } catch is JobCancelled {
        } catch { document.presentError(error) }
    }

    func validatePersistedWarpItem() -> Bool {
        guard let document, let source = document.doc else { return false }
        return source.layerInfo(document.activeLayerIndex) != nil
    }
}
