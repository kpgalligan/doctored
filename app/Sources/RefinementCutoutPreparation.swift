import AppKit

/// Main-thread lifetime of a captured cutout preparation. The observer cancels
/// the parent throughout assembly and projection, while workers capture only
/// immutable sources and tokens. Drain every worker before releasing the scope.
final class RefinementCutoutPreparation {
    let control: RasterJobControl
    private var closeObserver: NSObjectProtocol?

    init(document: ImageDocument, control: RasterJobControl?) throws {
        assert(Thread.isMainThread)
        guard !document.hasClosed else {
            throw DoctoredCoreError(message: "The cutout document is closed.")
        }
        let token = try control ?? RasterJobControl()
        try token.beginCompoundOperation()
        self.control = token
        closeObserver = NotificationCenter.default.addObserver(forName: .imageDocumentDidClose,
            object: document, queue: nil) { _ in token.cancel() }
    }

    func finish() {
        assert(Thread.isMainThread)
        if let closeObserver { NotificationCenter.default.removeObserver(closeObserver) }
        closeObserver = nil
    }

    deinit { if let closeObserver { NotificationCenter.default.removeObserver(closeObserver) } }
}
