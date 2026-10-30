import AppKit

// Edit ▸ Puppet Warp's and Edit ▸ Perspective Warp's commit, at every depth and in both
// forms, shared by the canvas sessions and MCP `puppet_warp` / `perspective_warp`. The layer
// form is PersistedWarpWork's shape: the core plan, admission, one compound job (the warp, then
// the prepared display) under `BackgroundWork.wait` with Cancel, and the stale checks; the
// caller publishes once. The selection's float form is the selected-pixels edit
// (`puppet_selected` / `perspective_selected`): at 8 bits through `MoveSelectedPixels.prepared`,
// at 16 and 32 bits through `PrecisionGeometryGesture`'s `.transformSelected` publication.

/// Which mesh warp a `MeshWarpWork` commits: its name, its request JSON and
/// its core plan and edit.
enum MeshWarpKind {
    case puppet, perspective

    var name: String {
        switch self {
        case .puppet: return PuppetWarp.actionName
        case .perspective: return PerspectiveWarp.actionName
        }
    }

    func json(_ fields: [String: Any]) throws -> String {
        switch self {
        case .puppet: return try PuppetWarp.json(fields)
        case .perspective: return try PerspectiveWarp.json(fields)
        }
    }

    func plan(_ source: RasterDocument, request: String) throws -> [String: Any] {
        switch self {
        case .puppet: return try source.puppetWarpPlan(request: request)
        case .perspective: return try source.perspectiveWarpPlan(request: request)
        }
    }

    func edit(_ source: RasterDocument, request: String, budget: (limit: UInt64, other: UInt64),
              control: RasterJobControl?) throws -> RasterDocument? {
        switch self {
        case .puppet:
            return try source.editingPuppetWarp(request: request, budget: budget, control: control)
        case .perspective:
            return try source.editingPerspectiveWarp(request: request, budget: budget,
                                                     control: control)
        }
    }
}

struct MeshWarpWork {
    let kind: MeshWarpKind
    let document: ImageDocument
    /// The document's handle when the work was captured, and the one the
    /// warp runs on (the same, or the layer with its description dropped).
    let base: RasterDocument
    let source: RasterDocument
    let layer: Int
    /// The core request's fields: `PuppetWarp.fields` / `PerspectiveWarp.fields`
    /// (the canvas), or the agent's own values.
    let fields: [String: Any]
    /// The float form's canvas u16 coverage; nil for the layer form.
    let coverage: [UInt16]?
    let selection: SelectionHistoryState

    var isFloat: Bool { coverage != nil }

    func request() throws -> String { try kind.json(fields) }

    private var staleSentence: String {
        "The \(kind.name) target or editing state changed while work ran."
    }

    /// The layer form: nil for an identity request.
    func prepareLayer(title: String) throws -> PreparedDocumentEdit? {
        assert(Thread.isMainThread)
        guard document.doc === base, !document.isLiveEditing,
              let targetID = base.layerInfo(layer)?.id else {
            throw DoctoredCoreError(message: "The \(kind.name) target changed before work started.")
        }
        let revision = document.revision
        let state = document.captureEditingState()
        let text = try request()
        let plan = try kind.plan(source, request: text)
        let coreScratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        guard coreScratch > 0 else { return nil }
        let projection = UInt64(source.width) * UInt64(source.height) * 24
        let scratch = try PrecisionDocumentDescriptor.sum(coreScratch, projection)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let budget = try source.meshWarpBudget()
        let control = try RasterJobControl()
        let source = self.source
        let kind = self.kind
        let prepared = try BackgroundWork.wait(title: title, control: control) {
            try WorkerTask(scratchEstimate: scratch) {
                try control.beginCompoundOperation()
                guard let next = try control.withChildOperation(from: 0, to: 0.75, { child in
                    try kind.edit(source, request: text, budget: budget, control: child)
                }) else { return nil as PreparedDocumentEdit? }
                let prepared = try control.withChildOperation(from: 0.75, to: 1) { child in
                    PreparedDocumentEdit(next, scratch: BackgroundWork.scratch, control: child)
                }
                if let error = prepared.error { throw DoctoredCoreError(message: error) }
                return prepared
            }
        }
        guard !control.isCancelled else { throw JobCancelled() }
        guard document.doc === base, document.revision == revision, !document.isLiveEditing,
              document.isCurrentEditingState(state), base.layerInfo(layer)?.id == targetID else {
            throw DoctoredCoreError(message: staleSentence)
        }
        return prepared
    }

    /// The float form at 8 bits: nil when no pixel changed.
    func prepareFloat8(title: String) throws -> PreparedDocumentEdit? {
        guard let coverage else { return nil }
        return try MoveSelectedPixels.prepared(
            document, source: source, request: request(), coverage: coverage, title: title,
            base: base, selection: selection, stale: staleSentence)
    }

    /// The float form at 16 and 32 bits, published by the gesture's own
    /// commit: `.unchanged` when no pixel changed.
    func publishTypedFloat(name: String, record: [ActionStep]) throws -> PublishOutcome {
        guard let coverage else { return .unchanged }
        let gesture = try PrecisionGeometryGesture(document)
        defer { gesture.cancel() }
        let command = try PrecisionPixelCommand(fields)
        return try gesture.finishOutcome(.transformSelected(command: command, coverage: coverage),
                                         name: name, record: record)
    }

    /// The canvas commit: one undo step named `name` recorded as `record`.
    func publish(name: String, record: [ActionStep]) throws -> PublishOutcome {
        if isFloat, base.isHighDepth {
            return try publishTypedFloat(name: name, record: record)
        }
        let prepared = try isFloat ? prepareFloat8(title: name) : prepareLayer(title: name)
        guard let prepared else { return .unchanged }
        document.preparedEdit = prepared
        document.applyEdit(name, record: record, allowWarpChange: true) { _ in prepared.document }
        return document.doc === prepared.document ? .published : .refused
    }
}
