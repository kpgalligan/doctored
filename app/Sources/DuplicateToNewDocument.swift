import AppKit

// `ImageDocument.makeDuplicate` builds the new document from ONE core call at each depth
// (`RasterDocument.duplicatingToNewDocument`: `dr_doc_duplicate_to_new_document`, or the typed
// `duplicate_to_new_document` structure request) — native `fileType`, dirty through
// `countEditChange`, active layer the topmost root, never `adoptWorkingSpace()` (`makeNew`'s
// precedent) — and `openDuplicate` shows it and tells `ActionRecorder.noteDuplicatedDocument`. The
// source gets no undo step and no dirty change; the UI records `duplicate_to_new_document` with
// `{"$layers":"selected"}` against the source after success.

/// Layer ▸ Duplicate to New Document…: the selected layers, with their
/// subtrees and every per-layer property, copied into a NEW untitled document
/// at the source's depth, canvas, profile and resolution. The core builds it
/// in ONE call at each depth (`RasterDocument.duplicatingToNewDocument`); this
/// file only turns that handle into an open window.
///
/// The fifth document-creating path beside open, New from Clipboard,
/// File ▸ New… and recovery. Like `makeNew` it never calls
/// `adoptWorkingSpace()`: the new document's profile IS the source's.
extension ImageDocument {
    /// The new document, not yet shown. Dirty through `countEditChange`, its
    /// history "Initial State" with no undo step, its active layer the
    /// topmost root. The source is not touched.
    static func makeDuplicate(of source: RasterDocument, layers: [Int], name: String?) throws
        -> ImageDocument
    {
        guard let made = try source.duplicatingToNewDocument(layers) else {
            throw DoctoredCoreError(
                message: "The selected layers could not be duplicated into a new document.")
        }
        let document = ImageDocument()
        document.doc = made
        document.activeLayerIndex = max(0, made.layerCount - 1)
        document.refreshProjection()
        document.fileType = Self.nativeTypeIdentifier
        if let name = name?.trimmingCharacters(in: .whitespacesAndNewlines), !name.isEmpty {
            document.displayName = name
        }
        document.countEditChange(.changeDone)
        return document
    }

    /// `makeDuplicate`, then the window — and, while a recording runs, the
    /// note that keeps steps made in the new document out of the recording
    /// (`ActionRecorder.noteDuplicatedDocument`).
    @discardableResult
    static func openDuplicate(of source: ImageDocument, layers: [Int], name: String?) throws
        -> ImageDocument
    {
        guard let doc = source.doc else {
            throw DoctoredCoreError(message: "Document has no image.")
        }
        let document = try makeDuplicate(of: doc, layers: layers, name: name)
        NSDocumentController.shared.addDocument(document)
        document.makeWindowControllers()
        document.showWindows()
        ActionRecorder.shared.noteDuplicatedDocument(document, from: source)
        return document
    }
}
