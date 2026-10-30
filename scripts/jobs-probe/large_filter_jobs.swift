import AppKit

// Opt-in (name "large filter jobs" in DR_JOBS_PROBE_CASES, with
// DR_WORKING_LIMIT_BYTES=8589934592): a 45 MP filter runs as a cancellable
// job with progress at 8 and 16 bits. On an 8192 × 5504 document built by the
// core and filled by Clouds through the same filter job, Dust & Scratches r2,
// Surface Blur r5 T15 and Radial Blur (Spin, Good) each
//   (cancel)   stop at the progress panel's Cancel and leave the document, its
//              revision, its undo stack and its dirty state as they were, and
//   (complete) run to the end as ONE undo step with changed pixels.
// Every call is the MCP mirror `apply_filter`, the path the sheets' Apply,
// replayed Actions and ⌃F share (`ByteFilterAdmission.run` at 8 bits,
// `SelectedAdjustment.edit(...).run` at 16). Twelve rows, one PASS line each.
// No row has a time verdict: the elapsed seconds printed are information.

private let largeWidth = 8192
private let largeHeight = 5504

private struct LargeFilter {
    let name: String
    let arguments: [String: Any]
}

private let largeFilters = [
    LargeFilter(name: "dust_and_scratches", arguments: ["radius": 2, "threshold": 0]),
    LargeFilter(name: "surface_blur", arguments: ["radius": 5, "threshold": 15]),
    LargeFilter(name: "radial_blur", arguments: ["amount": 10, "method": "spin", "quality": "good"]),
]

/// A blank 8192 × 5504 document at `depth` bits with its window off-screen,
/// filled by Clouds through `apply_filter`, saved-clean and with no undo.
private func largeDocument(depth: Int) throws -> ImageDocument {
    let source: RasterDocument
    if depth == 16 {
        source = try RasterDocument.blankRGBA16(
            width: largeWidth, height: largeHeight, fill: [65535, 65535, 65535, 65535],
            profile: RasterProfile.builtin(.sRGB), layerName: "Background")
    } else {
        guard let image = RasterImage.from(
                rgba: [UInt8](repeating: 255, count: largeWidth * largeHeight * 4),
                width: largeWidth, height: largeHeight),
              let blank = RasterDocument.from(image: image) else {
            throw ProbeFailure(description: "[large filter jobs] no blank 8-bit document")
        }
        source = blank
    }
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: depth, exportDither: "none"), markSaved: true)
    document.undoManager?.groupsByEvent = false
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    // Both colours are passed, as Filters ▸ Render ▸ Clouds passes its swatches.
    _ = try agentOK("apply_filter", document, [
        "filter": "clouds", "seed": 606, "foreground_color": "#000000",
        "background_color": "#ffffff"])
    document.undoManager?.removeAllActions()
    document.updateChangeCount(.changeCleared)
    guard let filled = document.doc, filled.isHighDepth == (depth == 16),
          filled.width == largeWidth, filled.height == largeHeight else {
        throw ProbeFailure(description: "[large filter jobs] the \(depth)-bit document is not "
            + "\(largeWidth) x \(largeHeight)")
    }
    return document
}

/// The current state's position in the document's history. A step taken
/// after an undo replaces the redo tail, so the entry COUNT can stay the same
/// while the position moves on by one.
private func historyPosition(_ document: ImageDocument) throws -> Int {
    guard let id = document.history.currentID,
          let index = document.history.entryIndex(id: id) else {
        throw ProbeFailure(description: "[large filter jobs] the document has no current history state")
    }
    return index
}

/// The composite at a fixed 12 × 8 grid of interior points, as `sample_pixel`
/// reports it: `hex` at 8 bits, `normalized_rgba` (the 16-bit codes over
/// 65535) at 16.
private func sampleGrid(_ document: ImageDocument) throws -> [String] {
    var out: [String] = []
    for row in 1...8 {
        for column in 1...12 {
            let reply = try agentOK("sample_pixel", document,
                ["x": column * largeWidth / 13, "y": row * largeHeight / 9])
            if let hex = reply["hex"] as? String {
                out.append(hex)
            } else if let rgba = reply["normalized_rgba"] as? [Double] {
                out.append(rgba.map { String($0) }.joined(separator: ","))
            } else {
                throw ProbeFailure(description: "[large filter jobs] sample_pixel reported no "
                    + "colour: \(reply.keys.sorted())")
            }
        }
    }
    return out
}

private func verifyLargeFilter(_ filter: LargeFilter, depth: Int, document: ImageDocument,
                               grid: [String]) throws {
    let tag = "[large filter jobs, \(depth)-bit \(filter.name)]"
    var arguments = filter.arguments
    arguments["filter"] = filter.name
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) no document") }
    let revision = document.revision
    let history = document.history.entries.count
    let position = try historyPosition(document)
    let canUndo = document.undoManager?.canUndo == true
    let undoName = document.undoManager?.undoActionName
    let dirty = document.isDocumentEdited
    let alerts = AutomationAlerts.recorded.count

    // Cancel mid-way, through the panel's own button.
    let canceller = PanelCanceller(title: "Applying \(filter.name)")
    var started = Date()
    let cancelled = try agentCall("apply_filter", document, arguments)
    canceller.stop()
    try require(canceller.clickedAt != nil,
                "\(tag) the panel's Cancel was never clicked (worker seen: \(canceller.sawOccupied)): "
                + "\(cancelled.isError ? cancelled.text : "the call completed")")
    try require(cancelled.isError && cancelled.text.contains(JobCancelled.message),
                "\(tag) a cancelled call answers JobCancelled in-band: \(cancelled.text)")
    try require(document.doc === base && document.revision == revision,
                "\(tag) Cancel preserved the document and its revision")
    let afterCancel = try historyPosition(document)
    try require(document.history.entries.count == history && afterCancel == position
                && (document.undoManager?.canUndo == true) == canUndo
                && document.undoManager?.undoActionName == undoName
                && document.isDocumentEdited == dirty,
                "\(tag) Cancel left undo and the dirty state as they were")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "\(tag) Cancel released the worker slot and its scratch")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) Cancel presented no alert: \(AutomationAlerts.recorded.suffix(2))")
    print(String(format: "PASS: large filter jobs — %d-bit %@ cancelled mid-way after %.1f s "
                 + "leaves the document unchanged (%@)", depth, filter.name,
                 Date().timeIntervalSince(started), canceller.latency("panel")))

    // Complete.
    started = Date()
    let reply = try agentCall("apply_filter", document, arguments)
    try require(!reply.isError, "\(tag) the filter refused: \(reply.text)")
    let elapsed = Date().timeIntervalSince(started)
    guard let filtered = document.doc, filtered !== base else {
        throw ProbeFailure(description: "\(tag) the completed filter published nothing: \(reply.text)")
    }
    let landed = try historyPosition(document)
    try require(landed == position + 1,
                "\(tag) one history step: position \(landed) after \(position)")
    let after = try sampleGrid(document)
    let changed = zip(grid, after).filter { $0 != $1 }.count
    try require(changed > 0, "\(tag) no sampled pixel changed")
    document.undoManager?.undo()
    let restored = try historyPosition(document)
    try require(document.doc === base && restored == position,
                "\(tag) one undo restores the base")
    try require(AutomationAlerts.recorded.count == alerts,
                "\(tag) the completed filter presented no alert: \(AutomationAlerts.recorded.suffix(2))")
    print(String(format: "PASS: large filter jobs — %d-bit %@ completes in %.1f s as one undo "
                 + "step, %d of %d sampled pixels changed", depth, filter.name, elapsed, changed,
                 grid.count))
}

func verifyLargeFilterJobs() throws {
    let limit = RasterResources.shared.workingLimit
    try require(limit >= 8 << 30,
                "[large filter jobs] run with DR_WORKING_LIMIT_BYTES=8589934592 (the 16 GB Mac's "
                + "limit); the limit is \(limit) bytes")
    for depth in [8, 16] {
        let document = try largeDocument(depth: depth)
        defer { document.close() }
        _ = try editor(document)
        let grid = try sampleGrid(document)
        try require(Set(grid).count > 1,
                    "[large filter jobs] Clouds filled the \(depth)-bit document: "
                    + "\(Set(grid).count) distinct of \(grid.count) samples, first \(grid.first ?? "-")")
        for filter in largeFilters {
            try verifyLargeFilter(filter, depth: depth, document: document, grid: grid)
        }
    }
}
