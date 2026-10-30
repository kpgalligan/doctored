import AppKit

/// Actual controllers and controls, compiled with an immutable app-source snapshot.
/// The fixture does not replace rendering or editing-state implementations.
func runArtworkUIWorkflowProbe(directory: URL) throws -> [String: Any] {
    let oldPolicy = NSApp.activationPolicy()
    _ = NSApp.setActivationPolicy(.accessory)
    let oldText = ToolOptionsStore.shared.text
    let oldShape = ToolOptionsStore.shared.shape
    defer {
        ToolOptionsStore.shared.text = oldText
        ToolOptionsStore.shared.shape = oldShape
        _ = NSApp.setActivationPolicy(oldPolicy)
    }
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    var groups: [String] = []
    for depth in [8, 16] {
        groups += try ArtworkUIProbe.controllers(depth: depth, directory: directory)
    }
    groups += try ArtworkUIProbe.cancellation()
    let report: [String: Any] = ["passed": true, "groups": groups, "group_count": groups.count]
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        .write(to: directory.appendingPathComponent("artwork-ui-workflow.json"))
    return report
}

/// Records the `document:shouldClose:contextInfo:` answer `canClose` gives.
private final class ShouldCloseRecorder: NSObject {
    var answer: Bool?
    @objc func document(_ document: NSDocument, shouldClose: Bool,
                        contextInfo: UnsafeMutableRawPointer?) {
        answer = shouldClose
    }
}

private enum ArtworkUIProbe {
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) throws {
        guard condition() else { throw DoctoredCoreError(message: "Artwork UI probe: \(message)") }
    }
    static func pump() { RunLoop.main.run(until: Date().addingTimeInterval(0.01)) }
    static func descendants(_ view: NSView) -> [NSView] {
        [view] + view.subviews.flatMap(descendants)
    }
    static func field(_ sheet: ArtworkColorsSheetController, _ index: Int) throws -> NSTextField {
        guard let result = descendants(sheet.view).compactMap({ $0 as? NSTextField })
            .first(where: { $0.identifier?.rawValue == "artworkColor.value\(index)" }) else {
            throw DoctoredCoreError(message: "Missing actual Artwork Colors numeric field \(index)")
        }
        return result
    }
    static func setColor(_ sheet: ArtworkColorsSheetController, _ rgba: [Double]) throws {
        for (index, value) in rgba.enumerated() { try field(sheet, index).stringValue = String(value) }
    }
    static func button(_ sheet: ArtworkColorsSheetController, action: String) throws -> NSButton {
        guard let result = descendants(sheet.view).compactMap({ $0 as? NSButton })
            .first(where: { $0.action.map(NSStringFromSelector) == action }) else {
            throw DoctoredCoreError(message: "Missing actual Artwork Colors \(action) button")
        }
        return result
    }
    static func present(_ document: ImageDocument, _ editor: EditorViewController)
        throws -> ArtworkColorsSheetController {
        let sheet = try ArtworkColorsSheetController(document: document)
        editor.presentAsSheet(sheet)
        pump()
        sheet.view.layoutSubtreeIfNeeded()
        return sheet
    }
    static func apply(_ sheet: ArtworkColorsSheetController, document: ImageDocument) throws {
        let control = try button(sheet, action: "apply:")
        withUndoGroup(document.undoManager) { control.performClick(nil) }
        pump()
    }
    static func cancel(_ sheet: ArtworkColorsSheetController) throws {
        try button(sheet, action: "cancel:").performClick(nil)
        pump()
    }
    static func saveView(_ view: NSView, to url: URL) throws {
        guard let image = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
            throw DoctoredCoreError(message: "Could not capture owned Artwork Colors view")
        }
        view.cacheDisplay(in: view.bounds, to: image)
        guard let data = image.representation(using: .png, properties: [:]) else {
            throw DoctoredCoreError(message: "Could not encode owned Artwork Colors view")
        }
        try data.write(to: url)
    }
    static func document(depth: Int) throws -> (ImageDocument, EditorViewController) {
        let width = 320, height = 220
        guard let image = RasterImage.from(rgba: [UInt8](repeating: 0, count: width * height * 4),
            width: width, height: height), let byte = RasterDocument.from(image: image),
            let profile = byte.iccProfile else {
            throw DoctoredCoreError(message: "Could not create actual artwork fixture document")
        }
        let source = depth == 8 ? byte : try RasterDocument.fromRawRGBA16(
            [UInt16](repeating: 0, count: width * height * 4),
            width: width, height: height, profile: profile)
        let document = ImageDocument()
        document.fileType = ImageDocument.nativeTypeIdentifier
        document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
            layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90,
            embedColorProfile: true, stripMetadata: false, exportBitDepth: depth,
            exportDither: "none"), markSaved: true)
        NSDocumentController.shared.addDocument(document)
        document.makeWindowControllers()
        document.showWindows()
        guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
            document.close()
            throw DoctoredCoreError(message: "Missing actual artwork editor controller")
        }
        _ = editor.view
        return (document, editor)
    }
    static func controllers(depth: Int, directory: URL) throws -> [String] {
        let (document, editor) = try document(depth: depth)
        defer { document.close() }
        var groups: [String] = []
        let prefix = "rgba\(depth)_"
        let original = document.doc
        let rgba = [0.1234567, 0.3456789, 0.5678912, 0.6123456]
        guard let color = ArtworkColor.color(rgba) else {
            throw DoctoredCoreError(message: "Invalid independent artwork fixture color")
        }
        var payload = TextLayerPayload(string: "A😀B",
            style: TextStyle(family: "Helvetica Neue", size: 28),
            color: color, box: .point)
        let run = ArtworkColorRun(location: 3, length: 1,
                                  rgba: [0.7654321, 0.2345678, 0.4567891, 0.8765432])
        payload.artwork?.runs = [run]
        withUndoGroup(document.undoManager) {
            editor.commitTextLayer(payload, origin: CGPoint(x: 25, y: 25), editing: nil)
        }
        try check(document.doc !== original && document.doc.layerCount == 2,
                  "\(prefix) actual text controller adds one layer")
        let textIndex = document.activeLayerIndex
        try check(document.doc.textPayload(textIndex)?.artwork?.rgba == rgba,
                  "\(prefix) text controller preserves continuous default ink")
        try check(document.doc.textPayload(textIndex)?.artwork?.runs == [run],
                  "\(prefix) text controller preserves attributed ink")
        let added = document.doc
        document.undoManager?.undo()
        try check(document.doc === original && !document.isDocumentEdited,
                  "\(prefix) one Undo returns the original saved document")
        document.undoManager?.redo()
        try check(document.doc === added, "\(prefix) Redo restores exact retained owner")
        groups.append(prefix + "controller_text_add_undo_redo")

        editor.setActiveLayer(0)
        let beforeLanding = document.doc
        var landingPayload = payload
        landingPayload.string = "Landing"
        landingPayload.artwork?.runs = []
        let expectedLanding = 1
        withUndoGroup(document.undoManager) {
            editor.commitTextLayer(landingPayload, origin: CGPoint(x: 25, y: 130), editing: nil)
        }
        try check(document.doc.layerCount == 3 && document.activeLayerIndex == expectedLanding
            && document.doc.textPayload(document.activeLayerIndex)?.string == "Landing"
            && document.doc.textPayload(2)?.string == "A😀B",
            "\(prefix) inserting from a lower active layer selects the actual new layer")
        document.undoManager?.undo()
        try check(document.doc === beforeLanding, "\(prefix) lower-layer insertion has one Undo")
        editor.setActiveLayer(textIndex)
        groups.append(prefix + "controller_insertion_selection")

        payload.string = "C😀D"
        payload.size = 34
        withUndoGroup(document.undoManager) {
            editor.commitTextLayer(payload, origin: CGPoint(x: 25, y: 25), editing: textIndex)
        }
        try check(document.doc.textPayload(textIndex)?.string == "C😀D"
            && document.doc.textPayload(textIndex)?.size == 34
            && document.doc.textPayload(textIndex)?.artwork?.runs == [run],
            "\(prefix) text controller edits content and size without dropping runs")
        groups.append(prefix + "controller_text_edit")

        editor.setActiveLayer(textIndex)
        let sheet = try present(document, editor)
        let newColor = [0.1234667, 0.3456889, 0.5678812, 0.6123556]
        try setColor(sheet, newColor)
        try saveView(sheet.view, to: directory.appendingPathComponent(prefix + "artwork-colors.png"))
        let beforeColor = document.doc
        try apply(sheet, document: document)
        try check(document.doc !== beforeColor
            && document.doc.textPayload(textIndex)?.artwork?.rgba == newColor
            && document.doc.textPayload(textIndex)?.artwork?.runs == [run],
            "\(prefix) actual color sheet preserves numeric color and existing runs")
        document.undoManager?.undo()
        try check(document.doc === beforeColor, "\(prefix) color sheet has one Undo transition")
        document.undoManager?.redo()
        groups.append(prefix + "numeric_color_sheet_undo")

        let rangeSheet = try present(document, editor)
        guard let popup = descendants(rangeSheet.view).compactMap({ $0 as? NSPopUpButton }).first else {
            throw DoctoredCoreError(message: "Missing actual color range popup")
        }
        popup.selectItem(at: 1)
        if let action = popup.action { _ = popup.sendAction(action, to: popup.target) }
        try field(rangeSheet, 4).stringValue = "2"
        try field(rangeSheet, 5).stringValue = "1"
        let emojiInk = [0.3216549, 0.6549873, 0.4327651, 0.3712345]
        try setColor(rangeSheet, emojiInk)
        try apply(rangeSheet, document: document)
        try check(document.doc.textPayload(textIndex)?.artwork?.runs.contains(
            ArtworkColorRun(location: 1, length: 2, rgba: emojiInk)) == true,
            "\(prefix) one complete emoji maps to its exact UTF-16 range")
        groups.append(prefix + "character_range_sheet")

        let staleTarget = try present(document, editor)
        try setColor(staleTarget, rgba)
        let beforeStale = document.doc
        editor.setActiveLayer(0)
        try apply(staleTarget, document: document)
        try check(document.doc === beforeStale, "\(prefix) changed target refuses sheet publication")
        try cancel(staleTarget)
        editor.setActiveLayer(textIndex)
        groups.append(prefix + "sheet_stale_target")

        let staleSelection = try present(document, editor)
        try setColor(staleSelection, rgba)
        let beforeSelection = document.doc
        editor.agentSetSelection(CanvasSelection(shape: .rect(CGRect(x: 5, y: 6, width: 20, height: 18)),
            canvasWidth: document.doc.width, canvasHeight: document.doc.height))
        try apply(staleSelection, document: document)
        try check(document.doc === beforeSelection, "\(prefix) changed selection refuses captured sheet")
        try cancel(staleSelection)
        editor.agentSetSelection(nil)
        groups.append(prefix + "sheet_stale_selection")

        let shape = ShapeLayerPayload(kind: "ellipse", w: 63.375, h: 48.625,
            flipped: false, fill: color, stroke: ArtworkColor.color(run.rgba),
            strokeWidth: 1.375, radius: 0)
        let shapeBefore = document.doc
        var preparationError: Error?
        withUndoGroup(document.undoManager) {
            document.applyEdit("UI probe shape fixture", record: .unrecorded("UI probe shape fixture")) { source in
                do {
                    return try source.nativeArtwork(.shape(shape), anchor: CGPoint(x: 130.25, y: 80.375),
                        layer: textIndex, replacing: false, name: "Native Shape")
                } catch { preparationError = error; return nil }
            }
        }
        if let preparationError { throw preparationError }
        try check(document.doc !== shapeBefore, "\(prefix) shape fixture was admitted")
        let shapeIndex = document.doc.layerCount - 1
        editor.setActiveLayer(shapeIndex)
        editor.editShapeLayer(shapeIndex)
        guard var session = editor.shapeEditSession else {
            throw DoctoredCoreError(message: "Actual shape controller did not open an edit session")
        }
        session.size.width += 7.125
        session.dirty = true
        editor.shapeEditSession = session
        let beforeShapeEdit = document.doc
        withUndoGroup(document.undoManager) { editor.commitShapeEditSession() }
        try check(document.doc.shapePayload(shapeIndex)?.w == shape.w + 7.125
            && document.doc.shapePayload(shapeIndex)?.artwork?.fill == rgba
            && document.doc.shapePayload(shapeIndex)?.artwork?.stroke == run.rgba,
            "\(prefix) actual shape resize preserves continuous fill and stroke")
        document.undoManager?.undo()
        try check(document.doc === beforeShapeEdit, "\(prefix) shape controller commits one Undo")
        document.undoManager?.redo()
        groups.append(prefix + "controller_shape_resize_color_undo")
        try refusedShapeCommitStops(
            document: document, editor: editor, shapeIndex: shapeIndex,
            directory: directory, prefix: prefix)
        groups.append(prefix + "controller_shape_refusal_stops_pending_commits")
        try check(!BackgroundWork.occupied, "\(prefix) artwork worker releases after controller edits")
        return groups
    }

    /// A pixel-locked shape whose reopened session is dirty refuses its
    /// commit, and every pending-commit caller stops — the editor and the
    /// document answer false, Close answers NO without a review, and a Save
    /// cancels without writing. Controller-level, so no key window is needed.
    static func refusedShapeCommitStops(
        document: ImageDocument, editor: EditorViewController, shapeIndex: Int,
        directory: URL, prefix: String
    ) throws {
        var lockError: Error?
        withUndoGroup(document.undoManager) {
            document.applyEdit("UI probe pixel lock", record: .unrecorded("UI probe pixel lock")) {
                do { return try $0.withLayerLocks(shapeIndex, DR_LOCK_PIXELS.rawValue) }
                catch { lockError = error; return nil }
            }
        }
        if let lockError { throw lockError }
        editor.editShapeLayer(shapeIndex)
        guard var session = editor.shapeEditSession else {
            throw DoctoredCoreError(message: "Locked shape did not open an edit session")
        }
        session.size.width += 3
        session.dirty = true
        editor.shapeEditSession = session
        let lockedDoc = document.doc
        func stopped(_ what: String, _ call: () throws -> Void) throws {
            let before = AutomationAlerts.recorded.count
            try call()
            let added = AutomationAlerts.recorded.dropFirst(before)
            try check(editor.shapeEditSession != nil, "\(prefix) \(what) leaves the shape session open")
            try check(document.doc === lockedDoc, "\(prefix) \(what) changes no document")
            try check(added.count == 1 && added.first?.kind == .alertModal,
                      "\(prefix) \(what) shows exactly the one lock alert")
            try check(!added.contains { $0.kind == .documentCloseReview },
                      "\(prefix) \(what) never reaches the close review")
        }
        try stopped("editor commit") {
            try check(editor.commitPendingSessions() == false,
                      "\(prefix) editor commitPendingSessions answers false")
        }
        try stopped("document commit") {
            try check(document.commitPendingCanvasSessions() == false,
                      "\(prefix) document commitPendingCanvasSessions answers false")
        }
        document.updateChangeCount(.changeCleared)
        let recorder = ShouldCloseRecorder()
        try stopped("close") {
            document.canClose(
                withDelegate: recorder,
                shouldClose: #selector(ShouldCloseRecorder.document(_:shouldClose:contextInfo:)),
                contextInfo: nil)
            try check(recorder.answer == false, "\(prefix) a refused commit answers Close with NO")
        }
        let target = directory.appendingPathComponent(prefix + "refused.rz")
        try? FileManager.default.removeItem(at: target)
        try stopped("save") {
            var answer: Error?
            var answered = false
            document.save(to: target, ofType: ImageDocument.nativeTypeIdentifier, for: .saveToOperation) {
                answer = $0
                answered = true
            }
            try check(answered && (answer as? CocoaError)?.code == .userCancelled,
                      "\(prefix) a refused commit cancels the save")
            try check(!FileManager.default.fileExists(atPath: target.path),
                      "\(prefix) a cancelled save writes no file")
        }
        // An MCP writer commits the session first, as File ▸ Save does,
        // and a refused commit answers with the refusal's own sentence.
        let agentTarget = directory.appendingPathComponent(prefix + "agent-refused.rz")
        try? FileManager.default.removeItem(at: agentTarget)
        try stopped("agent save_copy") {
            let before = AutomationAlerts.recorded.count
            let reply = try agentCall("save_copy", document: document, ["path": agentTarget.path])
            let alert = AutomationAlerts.recorded.dropFirst(before).first?.messageText ?? ""
            try check(reply.isError && !alert.isEmpty && reply.text.contains(alert),
                      "\(prefix) a refused commit answers save_copy with the lock sentence: \(reply.text)")
            try check(!FileManager.default.fileExists(atPath: agentTarget.path),
                      "\(prefix) a refused agent commit writes no file")
        }
        // contact_sheet_from_sources reads its sources and never commits
        // them: an open session refuses by name, with no alert and no edit.
        let alertsBeforeSheet = AutomationAlerts.recorded.count
        let sheetReply = try agentCall(
            "contact_sheet_from_sources", document: document,
            ["documents": [AgentServer.shared.summary(document)["id"] ?? 0]])
        try check(sheetReply.isError && sheetReply.text.contains("uncommitted")
                  && sheetReply.text.contains(document.displayName ?? "Untitled"),
                  "\(prefix) contact_sheet_from_sources refuses an open session by name: \(sheetReply.text)")
        try check(AutomationAlerts.recorded.count == alertsBeforeSheet && document.doc === lockedDoc
                  && editor.shapeEditSession != nil,
                  "\(prefix) the source refusal shows nothing and changes nothing")
        let captured = SessionCommitRefusal.capturing {
            document.presentError(DoctoredCoreError(message: "probe sentence"))
            return false
        }
        try check(captured.committed == false && captured.sentence == "probe sentence",
                  "\(prefix) a presented document error is captured as the refusal sentence")
        editor.cancelShapeEditSession()
        try check(editor.commitPendingSessions() == true,
                  "\(prefix) with the session cancelled nothing is pending")
        var unlockError: Error?
        withUndoGroup(document.undoManager) {
            document.applyEdit("UI probe pixel unlock", record: .unrecorded("UI probe pixel unlock")) {
                do { return try $0.withLayerLocks(shapeIndex, 0) }
                catch { unlockError = error; return nil }
            }
        }
        if let unlockError { throw unlockError }
        editor.editShapeLayer(shapeIndex)
        guard var reopened = editor.shapeEditSession else {
            throw DoctoredCoreError(message: "Unlocked shape did not open an edit session")
        }
        reopened.size.width += 3
        reopened.dirty = true
        editor.shapeEditSession = reopened
        let uncommitted = document.doc
        let committedTarget = directory.appendingPathComponent(prefix + "agent-committed.rz")
        try? FileManager.default.removeItem(at: committedTarget)
        let saved = try agentCall("save_copy", document: document, ["path": committedTarget.path])
        try check(!saved.isError && FileManager.default.fileExists(atPath: committedTarget.path),
                  "\(prefix) save_copy commits the open session and writes: \(saved.text)")
        try check(editor.shapeEditSession == nil && document.doc !== uncommitted,
                  "\(prefix) save_copy's commit closes the session and edits the document")
        document.undoManager?.undo()
        try check(document.doc === uncommitted, "\(prefix) save_copy's commit is one Undo step")
    }

    /// One MCP call through the real dispatcher, as the HTTP server makes it.
    static func agentCall(_ tool: String, document: ImageDocument,
                          _ arguments: [String: Any]) throws -> (isError: Bool, text: String) {
        var arguments = arguments
        arguments["document_id"] = AgentServer.shared.summary(document)["id"]
        let request = try JSONSerialization.data(withJSONObject: arguments)
        let response = AgentServer.shared.execute(
            tool: tool, argumentsJSON: String(decoding: request, as: UTF8.self))
        guard let envelope = try JSONSerialization.jsonObject(with: Data(response.utf8)) as? [String: Any],
              let content = envelope["content"] as? [[String: Any]],
              let text = content.first?["text"] as? String else {
            throw DoctoredCoreError(message: "Malformed MCP result for \(tool)")
        }
        return (envelope["isError"] as? Bool == true, text)
    }

    static func cancellation() throws -> [String] {
        let profile = RasterProfile.builtin(.sRGB)
        let control = try RasterJobControl()
        try check(control.setScratchLimit(128 * 1024 * 1024), "compound token configures scratch limit")
        let commands: [UInt8] = [0, 1, 1, 1, 4]
        let points: [Double] = [0.25, 0.125, 3.75, 0.125, 3.75, 3.875, 0.25, 3.875]
        let coverage = try control.withChildOperation(from: 0, to: 0.5) {
            try RasterArtworkCore.coverage(commands: commands, points: points, width: 4, height: 4, control: $0)
        }
        try check(control.progress >= 0.5 && control.scratchBytes == 0,
                  "compound progress retains completed stage and releases scratch")
        control.cancel()
        var refused = false
        do {
            _ = try control.withChildOperation(from: 0.5, to: 1) {
                try RasterArtworkCore.render(width: 4, height: 4,
                    components: "[{\"kind\":\"coverage\",\"color\":[0.123456,0.234567,0.345678,0.456789]}]",
                    samples: coverage, intrinsic: [], profile: profile, control: $0)
            }
        } catch { refused = true }
        try check(refused && control.isCancelled && control.scratchBytes == 0,
                  "Cancel between stages refuses assembly and releases child")

        let active = try RasterJobControl()
        try check(active.setScratchLimit(128 * 1024 * 1024), "active token configures scratch")
        var verbs: [UInt8] = [], coordinates: [Double] = []
        for index in 0..<256 {
            let x = Double(index * 3) + 0.125
            verbs += commands
            coordinates += [x, 0.125, x + 1.5, 0.125, x + 1.5, 1023.875, x, 1023.875]
        }
        let capturedVerbs = verbs, capturedCoordinates = coordinates
        let task = try WorkerTask(scratchEstimate: 64 * 1024 * 1024) {
            try active.withChildOperation {
                try RasterArtworkCore.coverage(commands: capturedVerbs, points: capturedCoordinates,
                    width: 1024, height: 1024, control: $0)
            }
        }
        let deadline = Date().addingTimeInterval(30)
        var peak: UInt64 = 0
        var result: Result<[UInt16], Error>?
        while result == nil && Date() < deadline {
            peak = max(peak, active.scratchBytes)
            if peak > 0 { active.cancel() }
            result = task.take()
            if result == nil { pump() }
        }
        if result == nil {
            active.cancel()
            let cleanup = Date().addingTimeInterval(10)
            while result == nil && Date() < cleanup { result = task.take(); if result == nil { pump() } }
        }
        guard let result else { throw DoctoredCoreError(message: "Owned artwork worker did not drain after cancellation") }
        if case .success = result { throw DoctoredCoreError(message: "In-flight cancellation fixture finished before Cancel") }
        try check(peak > 0 && active.isCancelled && active.scratchBytes == 0 && !BackgroundWork.occupied,
                  "active child scratch is observable and cancellation drains the worker")
        return ["compound_between_stage_cancellation", "compound_active_child_scratch_and_cancellation"]
    }
}
