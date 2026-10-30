import AppKit

private func nativeCutoutRefuses(_ body: () throws -> Void) throws {
    var refused = false
    // A user cancel surfaces as JobCancelled (7790467); every other refusal is a DoctoredCoreError.
    do { try body() } catch is DoctoredCoreError { refused = true } catch is JobCancelled { refused = true }
    try require(refused, "Native cutout must refuse stale/cancelled input")
}

func verifyNativeRefinementCutout() throws {
    let directory = URL(fileURLWithPath: ProcessInfo.processInfo.environment["DR_SELECTION_PROBE_DIR"] ?? NSTemporaryDirectory())
    let limit = RasterResources.shared.workingLimit
    let input = try RasterPrecisionDocument.openPreserving(url: directory.appendingPathComponent("native-cutout-original.png"), workingLimit: limit, otherRetained: 0)
    let foregroundDocument = try RasterPrecisionDocument.openPreserving(url: directory.appendingPathComponent("native-cutout-foreground.png"), workingLimit: limit, otherRetained: 0)
    guard let foreground = foregroundDocument.layerSource(0) else { throw ProbeFailure(description: "Missing native foreground") }
    let exact = try foreground.withRGBA16 { Array($0) }
    try require(exact[3] == 1 && exact[7] == 7 && exact[11] == 32769, "Fixture has genuine tiny and partial native alpha")
    let document = try fixture(width: 3, height: 2)
    defer { document.close() }
    let original = try RasterDocument(precision: input)
    withUndoGroup(document.undoManager) { document.applyEdit("Native fixture", record: .notACommand, intentionalDepthConversion: true) { _ in original } }
    let selected: [UInt16] = [1, 7, 257, 32769, 65534, 65535]
    document.setCanvasSelection(CanvasSelection(shape: .mask16(selected), canvasWidth: 3, canvasHeight: 2))
    let originalSelection = document.selectionHistory
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    let session = try RefinementTrimapSession(width: 3, height: 2)
    let capture = try RefinementCutoutCapture(document: document, session: session)
    try require(capture.selection.source == nil, "Native capture never stores a byte display source")
    let matte: [UInt16] = [1,257,32771,65534,123,65535]
    let beforePayload = try PrecisionDocumentDescriptor.integer(RasterResources.shared.report(),
                                                               "precision_capture_payload_bytes")
    let nativeMatte = try PrecisionScalarBuffer(samples: matte, width: 3, height: 2)
    let registeredPayload = try PrecisionDocumentDescriptor.integer(RasterResources.shared.report(),
                                                                   "precision_capture_payload_bytes")
    try require(registeredPayload == beforePayload + UInt64(matte.count) * 2 + 512,
                "Native coverage has one retained sample/overhead lease before assembly")
    // Unregistered raw coverage stays live through every display stage; a
    // registered scalar payload is already included in the resource union.
    let ordinaryContext = try PrecisionWorkContext(source: input)
    let borrowedContext = try PrecisionWorkContext(source: input, additionalRetainedBytes: 12)
    try require(borrowedContext.otherRetained == ordinaryContext.otherRetained + 12,
                "Native context accounts a borrowed six-sample u16 matte exactly once")
    let refusalToken = try RasterJobControl()
    try nativeCutoutRefuses {
        _ = try original.renderPrecision(control: refusalToken, additionalRetainedBytes: limit)
    }
    try require(refusalToken.scratchBytes == 0,
                "Native render receives borrowed retention and refuses excess budget without scratch")
    let refusedDisplay = PreparedDocumentEdit(original, borrowedInputBytes: limit)
    try require(refusedDisplay.error != nil && refusedDisplay.preview == nil
                && refusedDisplay.precisionProjection == nil,
                "Prepared native display includes borrowed retention in host admission")
    guard let prepared = try capture.prepare(foreground: foreground, matte: nativeMatte, in: document, session: session) else {
        throw ProbeFailure(description: "Tiny positive native product must survive assembly")
    }
    try require(document.doc === original && !document.isDocumentEdited && document.selectionHistory == originalSelection,
                "Native preparation leaves source, exact native selection and dirty state untouched")
    let preparedPayload = try PrecisionDocumentDescriptor.integer(RasterResources.shared.report(),
                                                                 "precision_capture_payload_bytes")
    guard let metadataPayload = prepared.document.precisionMetadata?.payload.bytes else {
        throw ProbeFailure(description: "Native output has no registered inspection metadata")
    }
    try require(preparedPayload == registeredPayload + metadataPayload,
                "Assembly retains the original coverage lease plus the new document metadata, without a second matte payload")
    try prepared.commit(in: document, session: session)
    guard let committed = document.doc, let native = committed.precision, let stored = native.layerSource(1) else {
        throw ProbeFailure(description: "Missing committed native cutout")
    }
    let source = try stored.withRGBA16 { Array($0) }
    let coverage = try native.readingPlane(request: #"{"op":"read_plane","target":"mask","index":1}"#, workingLimit: limit, otherRetained: 0)
    try require(source == exact && coverage == matte, "Native RGBA and mask remain separate and exact")
    try require(document.selectionHistory == originalSelection, "Cutout publication preserves the native selection")
    try require(committed.layerInfo(0)?.visible == false && document.activeLayerIndex == 1 && document.isDocumentEdited, "Commit hides the original and selects the new root")
    let path = directory.appendingPathComponent("native-refined-cutout.rz")
    try committed.saveNative(to: path)
    let reopened = try RasterPrecisionDocument.openPreserving(url: path, workingLimit: limit, otherRetained: 0)
    guard let reopenedImage = reopened.layerSource(1) else { throw ProbeFailure(description: "Missing reopened foreground") }
    let reopenedSource = try reopenedImage.withRGBA16 { Array($0) }
    let reopenedMask = try reopened.readingPlane(request: #"{"op":"read_plane","target":"mask","index":1}"#, workingLimit: limit, otherRetained: 0)
    try require(reopenedSource == exact && reopenedMask == matte, "Native reopen preserves independent tiny alpha and coverage")
    try verifyNativeCutoutPersistence(document, directory: directory, exact: exact, matte: matte,
                                     selection: originalSelection)
    document.undoManager?.undo()
    try require(document.doc === original && !document.isDocumentEdited && document.undoManager?.canUndo != true, "One native Undo restores the exact clean original")
    try require(document.selectionHistory == originalSelection, "Native Undo retains exact selection codes")
    document.undoManager?.redo()
    try require(document.doc === committed && document.activeLayerIndex == 1, "Native Redo restores cutout and target")
    try require(document.selectionHistory == originalSelection, "Native Redo retains exact selection codes")
    document.undoManager?.undo()
    try nativeCutoutRefuses { try prepared.commit(in: document, session: session) }
    let fresh = try RefinementCutoutCapture(document: document, session: session)
    let token = try RasterJobControl()
    guard let cancelled = try fresh.prepare(foreground: foreground, matte: matte, in: document, session: session, control: token) else {
        throw ProbeFailure(description: "Missing native cancellation fixture")
    }
    try require(!token.setScratchLimit(1), "Native preparation consumes its compound parent exactly once")
    try nativeCutoutRefuses {
        _ = try fresh.prepare(foreground: foreground, matte: matte, in: document,
                              session: session, control: token)
    }
    token.cancel()
    try nativeCutoutRefuses { try cancelled.commit(in: document, session: session) }
    let precancelled = try RasterJobControl(); precancelled.cancel()
    try nativeCutoutRefuses {
        _ = try fresh.prepare(foreground: foreground, matte: matte, in: document,
                              session: session, control: precancelled)
    }
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0
                && precancelled.scratchBytes == 0, "Native precancellation dispatches no worker or scratch")
    let empty = try fresh.prepare(foreground: foreground, matte: [UInt16](repeating: 0, count: 6), in: document, session: session)
    try require(empty == nil && document.doc === original && !document.isDocumentEdited && !BackgroundWork.occupied, "Native empty/stale/cancel output remains atomic")
    guard let closing = try fresh.prepare(foreground: foreground, matte: matte, in: document, session: session) else {
        throw ProbeFailure(description: "Missing native close fixture")
    }
    document.close()
    try nativeCutoutRefuses { try closing.commit(in: document, session: session) }
    print("PASS: internal native RGBA/matte separation, tiny alpha, before/after preparation, central Undo/Redo, stale/cancel/no-op and native reopen")
}
