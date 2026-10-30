import AppKit
import ImageIO

// `RetainedImageWork.edit`'s `selectNewLayer: Bool` became `landing: Int?`: the "select what I just
// made" answer is the slot rule's, never `layerCount - 1`.
//
// Only a camera RAW layer keeps a retained source (2026-10-10): every other placed file lands an
// ordinary raster layer at the document's depth, through the core's own `place` (so its colour
// conversion and depth rule are the retained render's) followed by `rasterize` of the landing
// index, as ONE source edit and one undo step. Import snapshots and hashes a regular file before
// decoding. Source edits capture revision/target/selection and use the shared worker. Placement
// lands at the slot the `position`/`layer` pair names (12E-2b; `.above` the active entry is the
// UI's default and an ABSENT core pair is still the legacy push).
//
// Scope and qualification live in `designs/phase-8-retained-source-status.md`.

/// Place, and the source edits a RAW layer still takes. Placement lands at the
/// CORE's one slot rule (`core/src/layer_insert.rs`) — `.above` the active
/// entry unless a caller names another slot.
enum RetainedImageWork {
    /// Returns the core open's profile refusal for the placed file.
    @discardableResult
    static func place(in document: ImageDocument, url: URL, name: String,
                      x: Int = 0, y: Int = 0,
                      storage: ImageSourceStorage = .embedded,
                      insertion: LayerInsertion? = nil,
                      raw: RawImportMode = .headless(RawDevelopSettings()),
                      record: [ActionStep] = .notACommand) throws -> String? {
        try LinkedImageWork.importImage(in: document, url: url, name: name, x: x, y: y,
            storage: storage, insertion: insertion, raw: raw, record: record)
    }

    static func rasterize(in document: ImageDocument, layer: Int,
                          record: [ActionStep] = .notACommand) throws {
        guard let source = document.doc, try source.retainedSourceInfo(layer) != nil else {
            throw failure("Choose a camera RAW layer.")
        }
        try edit(in: document, source: source,
                 request: PrecisionPixelCommand(["op": "rasterize", "layer": layer]),
                 title: "Rasterize", record: record)
    }

    /// `landing` is the index the new entry occupies once the edit lands —
    /// the slot rule's own answer (12E-2b), replacing the old
    /// `selectNewLayer` flag, which could only mean `layerCount - 1`.
    @discardableResult
    static func edit(in document: ImageDocument, source: RasterDocument,
                     asset: RasterDocument? = nil, request: PrecisionPixelCommand,
                     title: String, record: [ActionStep] = .notACommand,
                     landing: Int? = nil) throws -> Bool {
        try edit(in: document, source: source, steps: [(asset: asset, request: request)],
                 title: title, record: record, landing: landing)
    }

    /// Several source edits chained as ONE undo step (a place, then the
    /// rasterization of what it placed). Each step is planned on the intermediate it
    /// runs on and runs in its own `PrecisionCapturedWork.run` with its own
    /// job control, because a control starts once and its reservation is
    /// the latest figure, not a sum. Everything else live (unconsumed inputs,
    /// the intermediate, the captured document) is charged through the
    /// registry. A nil step result keeps the intermediate; a cancel or a
    /// throw at any step publishes nothing.
    @discardableResult
    static func edit(in document: ImageDocument, source: RasterDocument,
                     steps: [(asset: RasterDocument?, request: PrecisionPixelCommand)],
                     title: String, record: [ActionStep] = .notACommand,
                     landing: Int? = nil) throws -> Bool {
        guard !document.hasClosed, document.doc === source, !document.isLiveEditing, !document.isPublishingEditingState else {
            throw failure("The document changed before the image source edit began.")
        }
        let revision = document.revision
        guard let state = document.captureEditingState() else { throw failure("Could not capture the document state.") }
        var current = source
        for step in steps {
            let input = current
            let asset = step.asset
            let request = step.request.request
            let plan = try input.retainedSourcePlan(asset: asset, request: request)
            let scratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
            let next = try PrecisionCapturedWork.run(scratchEstimate: scratch, title: title) { control in
                try input.editingRetainedSource(asset: asset, request: request, control: control)
            }
            try requireCurrent(document, source: source, revision: revision, state: state)
            if let next { current = next }
        }
        guard current !== source else { return false }
        let result = current
        let prepared = try BackgroundWork.wait(try WorkerTask { PreparedDocumentEdit(result) },
                                                title: "Preparing Image Source Display")
        try requireCurrent(document, source: source, revision: revision, state: state)
        if let error = prepared.error { throw failure(error) }
        document.preparedEdit = prepared
        defer { document.preparedEdit = nil }
        try document.preflightPublication(result, record: record)
        withUndoGroup(document.undoManager) {
            document.applyEdit(title, record: record) { current in current === source ? result : nil }
            if let landing, document.doc === result {
                document.activeLayerIndex = min(max(landing, 0), result.layerCount - 1)
            }
        }
        return document.doc === result
    }

    private static func requireCurrent(_ document: ImageDocument, source: RasterDocument,
                                       revision: UInt64, state: EditingState) throws {
        guard !document.hasClosed, document.doc === source, document.revision == revision, !document.isLiveEditing,
              !document.isPublishingEditingState,
              document.isCurrentEditingState(state) else {
            throw failure("The document, target or selection changed. No image source edit was applied.")
        }
    }

    private static func failure(_ message: String) -> DoctoredCoreError { DoctoredCoreError(message: message) }

    /// The native open's pass over a file written before 2026-10-10: every
    /// layer whose retained source has no camera RAW original (a placed or
    /// linked image, a host still or movie frame) is rasterized, keeping its
    /// pixels exactly, and a Live Photo layer's description is cleared. The
    /// core has already dropped smart-object, Stack Mode and live-filter
    /// records. Not an edit: nothing is recorded and no prompt is shown. A
    /// layer the core refuses to rasterize keeps its source, and Layer ▸
    /// Rasterize RAW Layer still rasterizes it.
    static func plainRasters(_ opened: RasterDocument) -> RasterDocument {
        var current = opened
        for layer in 0..<opened.layerCount {
            if current.hasRetainedSource(layer), !current.hasRetainedRawSource(layer),
               let raster = rasterized(current, layer: layer) {
                current = raster
            }
            if let meta = current.layerMeta(layer), LivePhoto.isLegacyLayerDescription(meta),
               let cleared = current.withLayerMeta(layer, nil) {
                current = cleared
            }
        }
        return current
    }

    /// `rasterize` of `layer`, through the layer's own lock: an open is not
    /// an edit, so a pixel lock is lifted for the rasterization and put back.
    private static func rasterized(_ source: RasterDocument, layer: Int) -> RasterDocument? {
        guard let request = try? PrecisionPixelCommand(["op": "rasterize", "layer": layer]).request else {
            return nil
        }
        if let raster = try? source.editingRetainedSource(request: request) { return raster }
        let locks = source.layerLocks(layer)
        guard locks != 0, let unlocked = try? source.withLayerLocks(layer, 0),
              let raster = try? unlocked.editingRetainedSource(request: request),
              let relocked = try? raster.withLayerLocks(layer, locks) else { return nil }
        return relocked
    }
}

extension RasterDocument {
    var retainedSourceIndices: [Int] { (0..<layerCount).filter { hasRetainedSource($0) } }
}

extension Array where Element == ActionStep {
    static func placeImage(path: String, name: String, x: Int = 0, y: Int = 0,
                           storage: ImageSourceStorage = .embedded) -> [ActionStep] {
        step("place_image", ["path": path, "name": name, "x": x, "y": y,
                             "storage": storage.rawValue],
             note: storage == .linked ? "Layer ▸ Place Linked…" : "Layer ▸ Place Embedded…")
    }
    static func rasterizeImageSource() -> [ActionStep] {
        step("rasterize_image_source", note: "Layer ▸ Rasterize RAW Layer")
    }
}

/// Source-aware byte profile conversion stays on the existing worker. Its
/// temporary budget includes the full masters even when fallbacks are tiny.
/// Dependency/profile preparation currently has stage-only cancellation.
enum RetainedByteProfileWork {
    static func scratchEstimate(_ source: RasterDocument) throws -> UInt64 {
        var bytes: UInt64 = 16 * 1024 * 1024
        // Byte document clones copy layer descriptions and names. Inventory
        // reads their allocation sizes without copying large description blobs.
        let inventory = try RasterDocument.resourceReport(documents: [source], images: [], scratch: 0, limit: 0)
        let categories = inventory["categories"] as? [String: Any] ?? [:]
        for key in ["document_structure", "document_strings", "layer_metadata"] {
            if let category = categories[key] as? [String: Any] {
                bytes = try PrecisionDocumentDescriptor.sum(bytes,
                    PrecisionDocumentDescriptor.integer(category, "referenced_bytes"))
            }
        }
        for index in 0..<source.layerCount {
            guard let layer = source.layerInfo(index), !layer.isGroup else { continue }
            let info = try source.retainedSourceInfo(index)
            let typedMaster = info?["depth"] as? Int == 16
            let fallbackBytes: UInt64 = typedMaster ? 16 : 8
            bytes = try PrecisionDocumentDescriptor.sum(bytes,
                UInt64(layer.width) * UInt64(layer.height) * fallbackBytes)
            if let info {
                let width = try PrecisionDocumentDescriptor.integer(info, "width")
                let height = try PrecisionDocumentDescriptor.integer(info, "height")
                bytes = try PrecisionDocumentDescriptor.sum(bytes,
                    width * height * (typedMaster ? 24 : 8))
                if typedMaster {
                    // A supported ICC can reference one sampled TRC in all
                    // three channels. Typed preparation may clone all three.
                    let profileBytes = try PrecisionDocumentDescriptor.integer(info, "profile_bytes")
                    bytes = try PrecisionDocumentDescriptor.sum(bytes,
                        PrecisionDocumentDescriptor.sum(profileBytes,
                            PrecisionDocumentDescriptor.sum(profileBytes, profileBytes)))
                }
            }
        }
        return bytes
    }

    /// nil means no edit landed; otherwise the report of the one that did.
    @discardableResult
    static func run(in document: ImageDocument, profile: Data,
                    options: ConversionOptions = .default,
                    record: [ActionStep] = .notACommand) throws -> ConversionReport? {
        guard let source = document.doc, !source.isHighDepth, !document.isLiveEditing,
              let state = document.captureEditingState() else {
            throw DoctoredCoreError(message: "Finish the current gesture before converting the image profile.")
        }
        let revision = document.revision
        let scratch = try ProfileConversionWork.scratch(source, profile: profile, options: options)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let control = try RasterJobControl()
        guard control.setScratchLimit(scratch) else { throw DoctoredCoreError(message: "Could not configure source profile conversion.") }
        let closeObserver = NotificationCenter.default.addObserver(forName: .imageDocumentDidClose,
            object: document, queue: nil) { _ in control.cancel() }
        defer { NotificationCenter.default.removeObserver(closeObserver) }
        let task = try WorkerTask(scratchEstimate: scratch) {
            let converted = try source.convertingToProfile(profile, options: options, control: control)
            guard let result = converted, !control.isCancelled else {
                return nil as (edit: PreparedDocumentEdit, report: ConversionReport)?
            }
            return (PreparedDocumentEdit(result.document, scratch: BackgroundWork.scratch), result.report)
        }
        let prepared = try BackgroundWork.wait(task, title: "Converting Embedded Image Profiles", control: control)
        guard !document.hasClosed, document.doc === source, document.revision == revision,
              document.isCurrentEditingState(state), !document.isLiveEditing else {
            throw DoctoredCoreError(message: "The document changed during profile conversion. No result was applied.")
        }
        guard let prepared else { return nil }
        if let error = prepared.edit.error { throw DoctoredCoreError(message: error) }
        document.preparedEdit = prepared.edit
        defer { document.preparedEdit = nil }
        try document.preflightPublication(prepared.edit.document, record: record)
        withUndoGroup(document.undoManager) {
            document.applyEdit("Convert to Profile", record: record) { _ in prepared.edit.document }
        }
        return document.doc !== source ? prepared.report : nil
    }
}
