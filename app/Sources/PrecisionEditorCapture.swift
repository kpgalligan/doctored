import AppKit

// `PrecisionEditorCapture.requireTopInsertion` is GONE — the typed create path keeps only the Quick
// Mask guard, and the mask write and the `onCommitted` index read the slot's LANDING index, never
// `layer_count - 1`.

/// One captured editor revision, including the target/selection that survives
/// a modal worker wait. Every typed UI/MCP edit still uses ImageDocument's one
/// mutation/history path; this struct owns no additional history.
struct PrecisionEditorCapture {
    let facade: RasterDocument
    let source: RasterPrecisionDocument
    let revision: UInt64
    let layer: Int
    let selectedLayers: [Int]
    let target: PaintTarget
    let selection: CanvasSelection?
    let selectionState: SelectionHistoryState
    let quickMask: Bool
    let channelVisibility: ChannelVisibility

    init(_ document: ImageDocument) throws {
        let depth = document.doc?.isFloat == true ? "32-bit float" : "16-bit"
        guard let facade = document.doc, let source = facade.precision, !document.isLiveEditing, !document.hasClosed else {
            throw DoctoredCoreError(message: "Finish the current gesture before editing a \(depth) document.")
        }
        self.facade = facade
        self.source = source
        revision = document.revision
        layer = document.activeLayerIndex
        selectedLayers = document.selectedLayerIndices
        target = document.planeEditTarget
        selection = document.canvasSelection
        selectionState = document.selectionHistory
        quickMask = document.quickMaskActive
        channelVisibility = document.channelVisibility
    }

    func requireCurrent(_ document: ImageDocument) throws {
        guard document.doc === facade, document.revision == revision, !document.isLiveEditing, !document.hasClosed,
              document.activeLayerIndex == layer, document.selectedLayerIndices == selectedLayers,
              document.planeEditTarget == target, document.selectionHistory == selectionState,
              document.quickMaskActive == quickMask, document.channelVisibility == channelVisibility else {
            let depth = facade.isFloat ? "32-bit float" : "16-bit"
            throw DoctoredCoreError(message: "The document, target or selection changed while the \(depth) edit was running. No result was applied.")
        }
    }

    @discardableResult
    func publish(_ result: RasterPrecisionDocument?, to document: ImageDocument,
                 name: String, record: [ActionStep] = .notACommand,
                 changesCanvasGeometry: Bool = false,
                 resultingSelection: SelectionHistoryState? = nil) throws -> Bool {
        try requireCurrent(document)
        guard let result else { return false }
        // The evaluation policy's refusal reaches the CALLER here rather than
        // `ImageDocument.applyEdit`'s alert. An MCP tool runs on the main
        // thread, so a modal alert inside a publish would deadlock the call;
        // the UI path presents this thrown error in the same alert it would
        // have shown anyway (`DocumentEvaluation.swift` / `FloatDocument.swift`).
        if let reason = result.linearLightRefusalText() {
            throw DoctoredCoreError(message: reason)
        }
        try document.applyPreparedPrecision(result, captured: facade, revision: revision,
            actionName: name, record: record, changesCanvasGeometry: changesCanvasGeometry,
            resultingSelection: resultingSelection)
        return document.doc !== facade
    }

    /// `publish` for a result whose 16-bit display the caller already
    /// prepared inside its own job (`PrecisionGeometryGesture.finish`):
    /// the same currency and evaluation-policy checks, then
    /// `ImageDocument.applyPrepared` — no second preparation wait. nil =
    /// `.unchanged`; a publication `applyEdit` refused is `.refused`.
    @discardableResult
    func publish(prepared: PreparedDocumentEdit?, to document: ImageDocument, name: String,
                 record: [ActionStep], changesCanvasGeometry: Bool,
                 resultingSelection: SelectionHistoryState?) throws -> PublishOutcome {
        try requireCurrent(document)
        guard let prepared else { return .unchanged }
        if let reason = prepared.document.precision?.linearLightRefusalText() {
            throw DoctoredCoreError(message: reason)
        }
        return try document.applyPrepared(prepared, captured: facade, revision: revision,
            actionName: name, record: record, changesCanvasGeometry: changesCanvasGeometry,
            resultingSelection: resultingSelection)
    }
}

extension PrecisionInk {
    /// Authored controls use their continuous NSColor components. Byte hex
    /// controls retain their declared parameter granularity; neither route
    /// samples a displayed RGBA8 image as authoritative source RGB. Each
    /// converted component is clamped to 0…1, so an above-white swatch
    /// counts as white per channel here; a non-finite one still refuses.
    init(color: NSColor, space: NSColorSpace = .sRGB) throws {
        guard let color = ColorProfile.converted(color, into: space) else {
            throw DoctoredCoreError(message: "Could not interpret the authored color in the selected space.")
        }
        let unit: (CGFloat) -> Double = {
            let value = Double($0)
            return value.isFinite ? min(max(value, 0), 1) : value
        }
        try self.init(red: unit(color.redComponent), green: unit(color.greenComponent),
                      blue: unit(color.blueComponent), alpha: unit(color.alphaComponent))
    }
}
