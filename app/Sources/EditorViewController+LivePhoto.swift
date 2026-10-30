import AppKit
import UniformTypeIdentifiers

/// Layer ▸ Place Live Photo…: the pair's still photo (or a bare clip's first
/// frame) lands as an ordinary layer above the active one, as any placed
/// image does. Mirrored for the agent by `add_live_photo_layer`.
extension EditorViewController {
    @objc func placeLivePhoto(_ sender: Any?) {
        guard let document = document, document.doc != nil, let window = view.window else {
            NSSound.beep()
            return
        }
        // Never leave a canvas session hanging behind a modal panel.
        guard commitPendingSessions() else { return }
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.message = "Choose a Live Photo — its photo or its video. Its photo is placed as a layer."
        panel.prompt = "Place"
        panel.allowedContentTypes = Self.livePhotoContentTypes
        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let url = panel.url else { return }
            self?.placeLivePhoto(from: url)
        }
    }

    /// The file types the Place panel offers: both halves of a pair, since
    /// either one identifies the Live Photo.
    private static let livePhotoContentTypes: [UTType] = {
        let names = LivePhoto.stillExtensions + LivePhoto.videoExtensions
        return names.compactMap { UTType(filenameExtension: $0) }
    }()

    /// The picked file's still, as a new layer. Refuses a file that is not
    /// half of a Live Photo or a movie: a lone photo is Place Embedded…'s.
    private func placeLivePhoto(from url: URL) {
        guard let document else { return }
        do {
            guard LivePhoto.locate(url) != nil else {
                throw DoctoredCoreError(message: "Choose a movie, or either half of a Live Photo pair sharing one basename.")
            }
            let name = url.deletingPathExtension().lastPathComponent
            let notes = try HostSourceWork.place(in: document, url: url, name: name,
                record: .placeImage(path: url.path, name: name))
            if !notes.isEmpty { presentSourceNotice(notes.joined(separator: " ")) }
        } catch { document.presentError(error) }
    }
}
