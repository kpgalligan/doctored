import AppKit

// Image ▸ Mode ▸ 8/16 Bits/Channel on a Lab Color document (#1 Lab phase 5a): the captured edit
// and its ONE undo step, `GrayProfileEdit`'s depth arm twin. The core decides everything
// (`lab_depth`): 16 → 8 puts every L*, a*, b*, alpha, mask and alpha-channel value on the 8-bit
// grid, 8 → 16 widens only the declared depth; there is no 32-bit Lab. The menu
// items are `EditorViewController+CmykProfile.swift`'s (`convertToBits` →
// `EditorViewController+LabMode.swift`'s `commitLabDepth`), the MCP route
// `AgentServer+LabColour.swift`; the recorded step replays as `convert_precision` mode lab8/lab16.

/// One captured Lab depth edit: the source revision and the target depth, planned by the core
/// before anything is computed.
struct LabDepthEdit {
    let source: RasterDocument
    let revision: UInt64
    let editingState: EditingState
    /// The target depth, 8 or 16.
    let bits: Int
    let activeLayer: Int
    /// The core's plan for the edit.
    let scratch: UInt64
    /// False when the document already declares `bits`.
    let changes: Bool

    init(_ document: ImageDocument, bits: Int) throws {
        guard let source = document.doc, !document.isLiveEditing, !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: "Finish the current operation before changing the depth.")
        }
        guard let typed = source.precision, source.isLab else {
            throw DoctoredCoreError(message: "\(bits) Bits/Channel of a Lab document requires a Lab Color "
                + "document.")
        }
        guard let state = document.captureEditingState() else {
            throw DoctoredCoreError(message: "Could not capture the document state.")
        }
        self.source = source
        revision = document.revision
        editingState = state
        self.bits = bits
        activeLayer = document.activeLayerIndex
        let plan = try typed.labDepthPlan(bits: bits)
        if let refusal = plan["refusal"] as? String { throw DoctoredCoreError(message: refusal) }
        scratch = try PrecisionDocumentDescriptor.integer(plan, "working_memory_plan_bytes")
        changes = plan["changes"] as? Bool ?? false
    }

    /// The menu and undo title.
    var title: String { "\(bits) Bits/Channel" }

    /// The edit, on the bounded worker with its own reservation, then the display preparation.
    func compute() throws -> PreparedDocumentEdit {
        guard let typed = source.precision else {
            throw DoctoredCoreError(message: "The document is not a Lab Color document.")
        }
        let bits = bits, title = title
        let result: RasterDocument = try PrecisionCapturedWork.run(
            scratchEstimate: scratch, title: title) { control in
            let context = try PrecisionWorkContext(source: typed)
            guard let edited = try typed.labDepthEdit(bits: bits, workingLimit: context.limit,
                                                      otherRetained: context.otherRetained,
                                                      control: control) else {
                throw DoctoredCoreError(message: "\(title) changed nothing.")
            }
            return try RasterDocument(precision: edited)
        }
        let task = try WorkerTask { PreparedDocumentEdit(result) }
        let prepared = try BackgroundWork.wait(task, title: "Preparing Lab Color Display")
        guard prepared.preview != nil, prepared.error == nil else {
            throw DoctoredCoreError(message: prepared.error ?? "Could not prepare the Lab Color display.")
        }
        return prepared
    }

    /// ONE undo step.
    func publish(_ prepared: PreparedDocumentEdit, to document: ImageDocument, record: [ActionStep]) throws {
        guard document.doc === source, document.revision == revision, !document.isLiveEditing,
              document.isCurrentEditingState(editingState) else {
            throw DoctoredCoreError(message: "The document changed while \(title) was prepared. "
                + "Nothing was changed.")
        }
        document.preparedEdit = prepared
        defer { document.preparedEdit = nil }
        try document.preflightPublication(prepared.document, record: record)
        withUndoGroup(document.undoManager) {
            document.applyEdit(title, record: record) { _ in prepared.document }
        }
        if document.doc === prepared.document, prepared.document.isValidIndex(activeLayer) {
            document.selectLayer(activeLayer)
        }
    }

    /// The recorded step: `convert_precision` mode lab8 or lab16.
    func step() -> ActionStep {
        ActionStep(tool: "convert_precision", arguments: ["mode": "lab\(bits)"], note: "\(bits) Bits/Channel")
    }
}
