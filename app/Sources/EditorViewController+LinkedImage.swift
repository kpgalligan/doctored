import AppKit
import UniformTypeIdentifiers

extension EditorViewController {
    @objc func placeLinkedImage(_ sender: Any?) { chooseImageSource(.linked) }

    /// Layer ▸ Place Embedded… / Place Linked…: one file, one new layer above
    /// the active one. A camera RAW file is developed and becomes a RAW layer
    /// (which keeps `storage`'s link); every other file lands as an ordinary
    /// layer at the document's depth.
    func chooseImageSource(_ storage: ImageSourceStorage) {
        guard let document, let window = view.window else { NSSound.beep(); return }
        guard commitPendingSessions() else { return }
        do {
            let capture = try ImageSourceCapture(document)
            let panel = NSOpenPanel()
            panel.allowsMultipleSelection = false
            panel.canChooseDirectories = false
            panel.allowedContentTypes = [.png, .jpeg, .tiff, .rawImage]
                + ["heic", "heif", "jxl", "avif", "jp2", "j2k", "jpf", "jpx", "mov", "m4v", "mp4"]
                    .compactMap { UTType(filenameExtension: $0) }
            panel.prompt = "Place"
            panel.message = "Place the image as a new layer. A camera RAW file is developed and "
                + "becomes a RAW layer that keeps the camera file"
                + (storage == .linked ? " and a link to it" : "")
                + "; any other image becomes an ordinary layer at the document's depth."
            panel.beginSheetModal(for: window) { [weak self] response in
                guard response == .OK, let url = panel.url, let self, let document = self.document else { return }
                do {
                    try capture.requireCurrent(document)
                    let name = url.deletingPathExtension().lastPathComponent
                    let refusal = try RetainedImageWork.place(in: document, url: url, name: name,
                        storage: storage, raw: .ask,
                        record: .placeImage(path: url.path, name: name, storage: storage))
                    if let refusal { self.presentSourceNotice(refusal) }
                } catch { document.presentError(error) }
            }
        } catch { document.presentError(error) }
    }

    /// The core open's profile refusal, or a host source's
    /// disclosures, for a committed Place, verbatim (MCP returns them in
    /// `disclosures`). Informational only — the edit has already landed and
    /// stays.
    func presentSourceNotice(_ sentence: String) {
        let alert = NSAlert()
        alert.alertStyle = .informational
        alert.messageText = "Image Source Notice"
        alert.informativeText = sentence
        if let window = view.window {
            alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }

    func validateLinkedImageItem(_ selector: Selector) -> Bool? {
        guard let document, let doc = document.doc else { return nil }
        switch NSStringFromSelector(selector) {
        case "placeLinkedImage:": return true
        case "redevelopRaw:", "showRawSource:", "migrateRawDecoder:":
            return doc.hasRetainedRawSource(document.activeLayerIndex)
        default: return nil
        }
    }
}
