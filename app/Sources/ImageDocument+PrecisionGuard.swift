import Foundation

extension ImageDocument {
    /// Undo may restore a typed state while another captured job owns the
    /// serial worker. Keep its authoritative projection empty until the worker
    /// is available, then prepare only the latest document on that same queue.
    func schedulePrecisionProjectionRefresh() {
        guard precisionProjectionTimer == nil else { return }
        let timer = Timer(timeInterval: 0.05, repeats: true) { [weak self] timer in
            guard let self else { timer.invalidate(); return }
            guard !BackgroundWork.occupied, !self.isPublishingEditingState else { return }
            timer.invalidate()
            self.precisionProjectionTimer = nil
            guard self.doc?.precision != nil else { return }
            self.refreshProjection()
            NotificationCenter.default.post(name: .imageDocumentImageDidChange, object: self,
                                            userInfo: ["isLive": false])
        }
        precisionProjectionTimer = timer
        RunLoop.main.add(timer, forMode: .common)
    }

    /// Native precision is authoritative regardless of extension or import
    /// choice. A preserving PNG/TIFF decode failure cannot reach ImageIO.
    static func openPrecisionDocumentIfRequested(at url: URL) throws -> RasterDocument? {
        let native = hasNativeSignature(at: url)
        let typedNative = try native && RasterDocument.isPrecisionNative(at: url)
        if native && !typedNative { return try RasterDocument.open(url: url) }
        // #547: a CMYK TIFF or JPEG the core opens as a CMYK document is
        // decided here, by the core's own header test, before any 16-bit
        // prompt (`PrecisionImportRequest.resolve` answers it first).
        // A CMYK PSD is the PSD importer's (`import_precision` "cmyk").
        let cmyk = !typedNative && [.tiff, .jpeg].contains(RasterPrecisionDocument.cmykSource(at: url))
        // #547 phase 1a: a gray PNG, TIFF or JPEG the core opens as a Grayscale document, by the
        // core's own header test (`dr_path_gray_source`, which claims only what the gray open
        // accepts), routed like a CMYK file: no 16-bit prompt, and only rgba8 keeps today's open.
        let gray = !typedNative && !cmyk && RasterPrecisionDocument.graySource(at: url) != .none
        // #547 Lab phase 3a: a flat Lab TIFF the core opens as a Lab Color document, by the core's
        // own header test (`dr_path_lab_source`), routed like a gray file.
        let lab = !typedNative && !cmyk && !gray && RasterPrecisionDocument.labSource(at: url) != .none
        // #547 Indexed phase 1a: an indexed PNG or GIF the core opens as an Indexed Color document, by
        // the core's own header test (`dr_path_indexed_source`), asked after Lab and routed the same.
        let indexed = !typedNative && !cmyk && !gray && !lab
            && RasterPrecisionDocument.indexedSource(at: url) != .none
        // #547 Bitmap phase 1c: a 1-bit PNG, TIFF or BMP the core opens as a Bitmap document, by the
        // core's own header test (`dr_path_bitmap_source`), asked after Indexed and routed the same:
        // `precision` rgba8 keeps the old 8-bit RGB open.
        let bitmap = !typedNative && !cmyk && !gray && !lab && !indexed
            && RasterPrecisionDocument.bitmapSource(at: url) != .none
        if !typedNative {
            // OpenEXR and Radiance are float containers the platform reports
            // no PNG/TIFF depth for, so they are asked for by signature.
            guard PrecisionImportRequest.isHighDepthImage(url)
                    || PrecisionImportRequest.isFloatImage(url) || cmyk || gray || lab || indexed
                    || bitmap
            else { return nil }
            let mode = try PrecisionImportRequest.resolve(
                for: url, cmyk: cmyk || gray || lab || indexed || bitmap)
            if case .rgba8 = mode { return nil }
        }
        let context = try PrecisionWorkContext()
        // Edit ▸ Color Settings' CMYK policy and working Gray, read here on the main thread
        // (#547 phases 5 and 1a).
        let policy = CmykProfileEdit.OpenPolicy.current
        let workingGray = ColorSettings.workingGray.rawValue
        let task = try WorkerTask {
            var source = gray
                ? try RasterPrecisionDocument.openGray(url: url, workingGray: workingGray,
                                                       workingLimit: context.limit,
                                                       otherRetained: context.otherRetained)
                : lab
                ? try RasterPrecisionDocument.openLab(url: url, workingLimit: context.limit,
                                                      otherRetained: context.otherRetained)
                : indexed
                ? try RasterPrecisionDocument.openIndexed(url: url, workingLimit: context.limit,
                                                          otherRetained: context.otherRetained)
                : bitmap
                ? try RasterPrecisionDocument.openBitmap(url: url, workingGray: workingGray,
                                                         workingLimit: context.limit,
                                                         otherRetained: context.otherRetained)
                : try RasterPrecisionDocument.openPreserving(url: url, workingLimit: context.limit,
                                                             otherRetained: context.otherRetained)
            if cmyk { source = try policy.applied(to: source, context: context) }
            let document = try RasterDocument(precision: source)
            return try typedNative ? document : NativeAuxiliaryMattes.attaching(to: document, from: url)
        }
        let title = cmyk ? "Opening CMYK Document" : gray ? "Opening Grayscale Document"
            : lab ? "Opening Lab Color Document" : indexed ? "Opening Indexed Color Document"
            : bitmap ? "Opening Bitmap Document" : "Opening 16-bit Document"
        return try BackgroundWork.wait(task, title: title)
    }

    /// Value-only workers return a prepared immutable owner. The same central
    /// edit path owns dirty state, Actions and Undo for both precisions.
    func applyPreparedPrecision(_ result: RasterPrecisionDocument, captured: RasterDocument,
                                revision capturedRevision: UInt64, actionName: String,
                                record: [ActionStep], changesCanvasGeometry: Bool = false,
                                resultingSelection: SelectionHistoryState? = nil) throws {
        guard revision == capturedRevision, doc === captured, !isLiveEditing, !hasClosed else {
            throw DoctoredCoreError(message: "The document changed while this edit was being prepared.")
        }
        let state = captureEditingState()
        let task = try WorkerTask {
            PreparedDocumentEdit(try RasterDocument(precision: result))
        }
        let prepared = try BackgroundWork.wait(task, title: "Preparing 16-bit Display")
        guard revision == capturedRevision, doc === captured, !isLiveEditing, !hasClosed,
              isCurrentEditingState(state) else {
            throw DoctoredCoreError(message: "The document, target or selection changed while this edit was being prepared.")
        }
        try applyPrepared(prepared, captured: captured, revision: capturedRevision,
                          actionName: actionName, record: record,
                          changesCanvasGeometry: changesCanvasGeometry,
                          resultingSelection: resultingSelection)
    }

    /// The publication half of `applyPreparedPrecision`, for a caller that
    /// prepared the display in its own job: the same guards and ONE
    /// undo group, no worker. The outcome tells a refused publication from
    /// a published one (`publishPrepared`).
    @discardableResult
    func applyPrepared(_ prepared: PreparedDocumentEdit, captured: RasterDocument,
                       revision capturedRevision: UInt64, actionName: String,
                       record: [ActionStep], changesCanvasGeometry: Bool,
                       resultingSelection: SelectionHistoryState?) throws -> PublishOutcome {
        guard revision == capturedRevision, doc === captured, !isLiveEditing, !hasClosed else {
            throw DoctoredCoreError(message: "The document changed while this edit was being prepared.")
        }
        preparedEdit = prepared
        defer { preparedEdit = nil }
        try preflightPublication(prepared.document, record: record)
        return publishPrepared(prepared, name: actionName, record: record,
                               changesCanvasGeometry: changesCanvasGeometry,
                               resultingSelection: resultingSelection, captured: captured)
    }

    /// Native files are never platform image fallbacks, including a typed
    /// native file with a misleading extension.
    static func hasNativeSignature(at url: URL) -> Bool {
        guard let file = try? FileHandle(forReadingFrom: url) else { return false }
        defer { try? file.close() }
        guard let head = try? file.read(upToCount: 4) else { return false }
        return isNativeMagic(head)
    }

    /// True when `head` starts with a native magic: `DRDC`, which this build
    /// writes, or `RZDC`, which every file written before the rename starts
    /// with and which stays readable forever (`core/src/native_magic.rs`).
    static func isNativeMagic(_ head: Data) -> Bool {
        let magic = head.prefix(4)
        return magic == Data("DRDC".utf8) || magic == Data("RZDC".utf8)
    }
}
