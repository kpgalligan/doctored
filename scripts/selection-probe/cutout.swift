import AppKit

private func cutoutMustRefuse(_ body: () throws -> Void, _ message: String) throws {
    var refused = false
    // A user cancel surfaces as JobCancelled (7790467); every other refusal is a DoctoredCoreError.
    do { try body() } catch is DoctoredCoreError { refused = true } catch is JobCancelled { refused = true }
    try require(refused, message)
}

func verifyRefinementCutout() throws {
    let document = try fixture(width: 31, height: 19)
    defer { document.close() }
    withUndoGroup(document.undoManager) {
        document.applyEdit("Fixture mask", record: .notACommand) {
            $0.addingLayerMask(0, kind: DR_MASK_REVEAL_ALL)
        }
    }
    document.updateChangeCount(.changeCleared)
    let controller = try editor(document)
    controller.setPaintTarget(.mask)
    guard let original = document.doc, let originalPixels = original.layerPixels(0) else {
        throw ProbeFailure(description: "Missing cutout source")
    }
    let selected = CanvasSelection(shape: .rect(CGRect(x: 2, y: 2, width: 20, height: 12)),
                                   canvasWidth: 31, canvasHeight: 19)
    document.setCanvasSelection(selected)
    document.undoManager?.removeAllActions()
    let initialSelection = document.selectionHistory
    let initialEntries = document.history.entries.count
    let session = try RefinementTrimapSession(width: 31, height: 19)
    let alpha: [UInt8] = (0..<589).map { (i: Int) -> UInt8 in
        let x = i % 31
        return UInt8(x * 255 / 30)
    }
    let foregroundPixel: [UInt8] = [51, 102, 153, 255]
    let foregroundBytes = Array(repeating: foregroundPixel, count: 31 * 19).flatMap { $0 }
    guard let foreground = RasterImage.from(rgba: foregroundBytes, width: 31, height: 19) else {
        throw ProbeFailure(description: "Missing cutout foreground")
    }
    let capture = try RefinementCutoutCapture(document: document, session: session)
    let token = try RasterJobControl()
    guard let prepared = try capture.prepare(foreground: foreground, alpha: alpha,
                                            in: document, session: session, control: token) else {
        throw ProbeFailure(description: "Nonempty output unexpectedly became a no-op")
    }
    try require(document.doc === original && document.selectionHistory == initialSelection
                && document.history.entries.count == initialEntries && !document.isDocumentEdited,
                "Preparing a complete cutout leaves editing state untouched")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0 && token.scratchBytes == 0,
                "Assembly and projection release all temporary work reservations")
    try prepared.commit(in: document, session: session)
    let committed = document.doc
    try require(committed?.layerCount == 2 && document.isDocumentEdited && document.activeLayerIndex == 1,
                "Commit adds and selects a new cutout layer")
    try require(controller.paintTarget == .layer && controller.canvas.paintTarget == .layer
                && document.planeEditTarget == .layer,
                "Canvas and panels follow the new pixel target after the edit notification")
    try require(committed?.layerPixels(0) == originalPixels && committed?.layerPixels(1) == foregroundBytes,
                "Original pixels and unbaked foreground RGB remain exact")
    try require(committed?.layerInfo(0)?.visible == false && original.layerInfo(0)?.visible == true,
                "New result hides the original while the captured original retains its visibility")
    try require(committed?.layerMaskCoverage(1) == alpha && document.selectionHistory == initialSelection,
                "Alpha is an independent editable mask and selection remains reusable")
    let properties = committed?.layerMaskProperties(1)
    try require(properties?.linked == true && properties?.density == 1 && properties?.feather == 0,
                "New raw mask uses explicit default editable properties")
    let folder = ProcessInfo.processInfo.environment["DR_SELECTION_PROBE_DIR"] ?? NSTemporaryDirectory()
    let path = URL(fileURLWithPath: folder).appendingPathComponent("refined-cutout.rz")
    try committed?.saveNative(to: path)
    let reopened = try RasterDocument.open(url: path)
    try require(reopened.layerPixels(0) == originalPixels && reopened.layerPixels(1) == foregroundBytes
                && reopened.layerMaskCoverage(1) == alpha && reopened.layerMaskProperties(1) == properties,
                "Native reopen retains originals, foreground RGB and mask properties")
    document.undoManager?.undo()
    try require(document.doc === original && !document.isDocumentEdited
                && document.activeLayerIndex == 0 && document.undoManager?.canUndo != true,
                "One Undo restores the original handle, active layer, visibility and clean state")
    try require(controller.paintTarget == .mask && controller.canvas.paintTarget == .mask,
                "Undo restores the original mask target in the actual editor")
    document.undoManager?.redo()
    try require(document.doc === committed && document.activeLayerIndex == 1,
                "One Redo restores the full cutout editing state")
    document.undoManager?.undo()
    try cutoutMustRefuse({ try prepared.commit(in: document, session: session) },
                        "Undo to the same native handle cannot revive an older document revision")
    print("PASS: cutout preparation, separate RGB/mask output, native round trip and one clean Undo/Redo")

    let current = try RefinementCutoutCapture(document: document, session: session)
    guard let stale = try current.prepare(foreground: foreground, alpha: alpha,
                                         in: document, session: session) else {
        throw ProbeFailure(description: "Missing stale cutout fixture")
    }
    let otherSession = try RefinementTrimapSession(width: 31, height: 19)
    try cutoutMustRefuse({ try stale.commit(in: document, session: otherSession) },
                        "Same-size foreign session cannot publish another session's output")
    try session.begin(label: .unknown, radius: 1, at: CGPoint(x: 3, y: 3))
    try cutoutMustRefuse({ try stale.commit(in: document, session: session) },
                        "Drawing a draft invalidates prepared output")
    session.cancel()
    try cutoutMustRefuse({ try stale.commit(in: document, session: session) },
                        "Cancelling a draft does not revive the previous session revision")
    let fresh = try RefinementCutoutCapture(document: document, session: session)
    let cancel = try RasterJobControl()
    guard let cancelled = try fresh.prepare(foreground: foreground, alpha: alpha,
        in: document, session: session, control: cancel) else {
        throw ProbeFailure(description: "Missing cancellation fixture")
    }
    cancel.cancel()
    try cutoutMustRefuse({ try cancelled.commit(in: document, session: session) },
                        "Cancel after preparation still prevents publication")
    try require(document.doc === original && !document.isDocumentEdited
                && document.undoManager?.canUndo != true, "All stale/cancel refusals remain atomically clean")
    print("PASS: document and local session revision, foreign session, draft and post-preparation cancellation guards")

    let empty = try fresh.prepare(foreground: foreground, alpha: [UInt8](repeating: 0, count: alpha.count),
                                 in: document, session: session)
    try require(empty == nil, "Empty cutout coverage creates no layer or undo entry")
    try cutoutMustRefuse({ _ = try fresh.prepare(foreground: foreground, alpha: [255],
        in: document, session: session) }, "Mismatched alpha buffer refuses before dispatch")
    let precancel = try RasterJobControl(); precancel.cancel()
    try cutoutMustRefuse({ _ = try fresh.prepare(foreground: foreground, alpha: alpha,
        in: document, session: session, control: precancel) }, "Precancelled worker refuses")
    guard let nonopaque = RasterImage.from(rgba: originalPixels, width: 31, height: 19) else {
        throw ProbeFailure(description: "Missing nonopaque fixture")
    }
    // A foreground with its own alpha (the fixture's bottom rows are transparent) is a valid
    // cutout: the layer keeps those bytes, alpha included, and the coverage stays a separate mask.
    guard let translucent = try fresh.prepare(foreground: nonopaque, alpha: alpha,
        in: document, session: session) else {
        throw ProbeFailure(description: "A foreground with its own alpha became a no-op")
    }
    try require(translucent.document.layerPixels(1) == originalPixels
                && translucent.document.layerMaskCoverage(1) == alpha,
                "A foreground's own alpha is kept on the cutout layer, never multiplied into the mask")
    try require(document.doc === original && !document.isDocumentEdited
                && document.undoManager?.canUndo != true && !BackgroundWork.occupied,
                "No-op, invalid and uncommitted inputs preserve state and release the worker")
    print("PASS: empty-output no-op, invalid coverage, a foreground with its own alpha, cancellation and reservation cleanup")
}
