import AppKit
import CryptoKit

struct ProbeFailure: Error, CustomStringConvertible { let description: String }
func require(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
    if try !condition() { throw ProbeFailure(description: message) }
}
func pump() {
    RunLoop.main.run(until: Date().addingTimeInterval(0.01))
    while let event = NSApp.nextEvent(matching: .any, until: Date(), inMode: .default, dequeue: true) {
        NSApp.sendEvent(event)
    }
    NSApp.updateWindows()
}
@discardableResult
func call(_ tool: String, _ document: ImageDocument? = nil, _ arguments: [String: Any] = [:],
          refuses: Bool = false) throws -> [String: Any] {
    var arguments = arguments
    if let document { arguments["document_id"] = AgentServer.shared.documentID(for: document) }
    let data = try JSONSerialization.data(withJSONObject: arguments)
    let response = AgentServer.shared.execute(tool: tool, argumentsJSON: String(decoding: data, as: UTF8.self))
    guard let envelope = try JSONSerialization.jsonObject(with: Data(response.utf8)) as? [String: Any],
          let content = envelope["content"] as? [[String: Any]], let text = content.first?["text"] as? String else {
        throw ProbeFailure(description: "Malformed \(tool) response")
    }
    try require((envelope["isError"] as? Bool ?? false) == refuses, "\(tool): \(text)")
    if refuses { return ["refusal": text] }
    return (try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any]) ?? [:]
}
func opened(_ path: URL, depth: Int? = nil) throws -> ImageDocument {
    var arguments: [String: Any] = ["path": path.path]
    if let depth { arguments["precision"] = depth == 8 ? "rgba8" : "preserve" }
    let result = try call("open_document", nil, arguments)
    guard let info = result["document"] as? [String: Any], let id = info["id"] as? Int,
          let document = NSDocumentController.shared.documents.compactMap({ $0 as? ImageDocument })
            .first(where: { AgentServer.shared.documentID(for: $0) == id }) else {
        throw ProbeFailure(description: "Opened RAW document not registered: \(result)")
    }
    try require(document.previewImage != nil && document.projectionError == nil, "Actual editor has current projection")
    return document
}
func layerBytes(_ document: ImageDocument, layer: Int) throws -> Data {
    guard let source = document.doc else { throw ProbeFailure(description: "Missing source") }
    if let typed = source.precision {
        guard let image = typed.layerSource(layer) else { throw ProbeFailure(description: "Missing typed layer") }
        return try image.withRGBA16 { Data(UnsafeRawBufferPointer($0)) }
    }
    guard let bytes = source.layerPixels(layer) else { throw ProbeFailure(description: "Missing byte layer") }
    return Data(bytes)
}
func hash(_ bytes: Data) -> String { SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined() }
func sourceInfo(_ document: ImageDocument) throws -> [String: Any] {
    let response = try call("get_raw_source", document, ["layer": 0])
    return (response["source"] as? [String: Any]) ?? response
}
func save(_ document: ImageDocument, _ path: URL) throws {
    var done = false
    var failure: Error?
    document.save(to: path, ofType: ImageDocument.nativeTypeIdentifier, for: .saveAsOperation) {
        failure = $0; done = true
    }
    let deadline = Date().addingTimeInterval(30)
    while !done && Date() < deadline { pump() }
    try require(done, "NSDocument Save completes")
    if let failure { throw failure }
    try require(!document.isDocumentEdited, "Actual Save marks captured RAW state clean")
}
func privateRecovery(_ document: ImageDocument, directory: URL) throws -> [String: Any] {
    guard let state = document.captureEditingState() else { throw ProbeFailure(description: "Missing editing state") }
    let captured = try EditingStateArchive.capture(state)
    let id = UUID().uuidString
    let destination = try RecoveryStore.directory(root: directory, id: id)
    guard let lease = try RecoveryLease.acquire(directory: destination, create: true) else {
        throw ProbeFailure(description: "Private recovery lease unavailable")
    }
    defer { lease.release() }
    let request = RecoveryStore.Captured(id: id, title: "Retained RAW probe", sourceURL: nil,
        sourceType: nil, generation: .init(folder: UUID().uuidString, revision: document.revision,
            serial: 1, captured: Date()), previous: nil, state: captured,
        snapshots: try document.namedSnapshots.captureArchive())
    _ = try RecoveryCapacity.validate(request)
    let gate = RecoveryPublicationGate()
    let manifest = try BackgroundWork.wait(WorkerTask(scratchEstimate: request.scratchEstimate) {
        try RecoveryStore.write(request, directory: destination, gate: gate)
    }, title: "Writing Private RAW Recovery")
    let candidate = RecoveryStore.Candidate(id: id, directory: destination, manifest: manifest, failure: nil)
    let loaded = try BackgroundWork.wait(WorkerTask { try RecoveryStore.read(candidate) }, title: "Reading Private RAW Recovery")
    let restored = ImageDocument()
    NSDocumentController.shared.addDocument(restored)
    defer { restored.close() }
    restored.installEditingState(try loaded.state.editingState(), markSaved: false)
    try restored.namedSnapshots.installArchive(loaded.snapshots)
    try require(try layerBytes(restored, layer: 0) == layerBytes(document, layer: 0), "Recovery RAW fallback exact")
    try require(try layerBytes(restored, layer: 1) == layerBytes(document, layer: 1), "Recovery retouch layer exact")
    try require(restored.namedSnapshots.entries.count == 1, "Recovery retains named RAW snapshot")
    let info = try sourceInfo(restored)
    return ["directory": destination.path, "source": info]
}
func verifyUICancel(_ document: ImageDocument) throws {
    try call("set_active_layer", document, ["index": 0])
    guard let editor = document.windowControllers.first?.contentViewController as? EditorViewController else {
        throw ProbeFailure(description: "Missing real RAW editor")
    }
    for selector in [#selector(EditorViewController.redevelopRaw(_:)), #selector(EditorViewController.showRawSource(_:))] {
        let item = NSMenuItem(title: "RAW", action: selector, keyEquivalent: "")
        try require(editor.validateUserInterfaceItem(item), "RAW UI selector is enabled at the retained source's depth")
    }
    let owner = document.doc, revision = document.revision
    var clicked = false
    var tries = 0
    func cancelButton(_ view: NSView) -> NSButton? {
        if let button = view as? NSButton, button.title == "Cancel" { return button }
        return view.subviews.lazy.compactMap(cancelButton).first
    }
    let timer = Timer(timeInterval: 0.1, repeats: true) { timer in
        tries += 1
        if NSApp.modalWindow?.title == "Develop RAW", let root = NSApp.modalWindow?.contentView, let cancel = cancelButton(root) {
            clicked = true
            cancel.performClick(nil)
            timer.invalidate()
        } else if tries >= 50 { NSApp.abortModal(); timer.invalidate() }
    }
    RunLoop.main.add(timer, forMode: .common)
    editor.redevelopRaw(nil)
    timer.invalidate()
    try require(clicked && document.doc === owner && document.revision == revision,
                "Actual Layer > Redevelop RAW Cancel button preserves owner and history")
}
func verifyInitialUIDepth(_ root: URL) throws {
    let document = ImageDocument()
    defer { document.close() }
    let previousMode = PrecisionImportRequest.mode, previousRAW = RawImportRequest.mode
    PrecisionImportRequest.mode = .ask
    RawImportRequest.mode = .ask
    defer { PrecisionImportRequest.mode = previousMode; RawImportRequest.mode = previousRAW }
    var clicked = false
    var tries = 0
    func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(descendants) }
    let timer = Timer(timeInterval: 0.1, repeats: true) { timer in
        tries += 1
        if NSApp.modalWindow?.title == "Develop RAW", let view = NSApp.modalWindow?.contentView {
            let views = descendants(view)
            // Open stays disabled until the first preview is ready (Phase 8D,
            // `RawDevelopWindowController+Workflow.swift`); a click before
            // that does nothing and would leave the modal session open.
            if let popup = views.compactMap({ $0 as? NSPopUpButton }).first(where: { $0.itemTitles.contains(where: { $0.hasPrefix("8-bit") }) }),
               let open = views.compactMap({ $0 as? NSButton }).first(where: { $0.title == "Open" }), open.isEnabled {
                popup.selectItem(at: 1)
                if let action = popup.action { NSApp.sendAction(action, to: popup.target, from: popup) }
                // The depth change may disable Open again; keep polling (and
                // keep the 100-tick bound) until it is enabled after it.
                if open.isEnabled {
                    clicked = true
                    timer.invalidate()
                    open.performClick(nil)
                }
            }
        }
        if tries >= 100 { timer.invalidate(); NSApp.abortModal() }
    }
    RunLoop.main.add(timer, forMode: .common)
    defer { timer.invalidate() }
    try document.read(from: root.appendingPathComponent("synthetic.dng"), ofType: "com.adobe.raw-image")
    try require(clicked && document.doc?.isHighDepth == false, "Actual RAW Open depth popup creates requested byte document")
    guard let info = try document.doc?.retainedSourceInfo(0) else { throw ProbeFailure(description: "UI RAW import lost source") }
    try require(info["depth"] as? Int == 16 && info["raw"] is [String: Any], "Byte UI import retains genuine UInt16 and original RAW")
    print("PASS: actual initial RAW UI selects byte fallback while retaining 16-bit source")
}
func verifyRenamedHighDepthContainers(_ root: URL) throws {
    let original = try opened(root.appendingPathComponent("ordinary16.png"))
    defer { original.close() }
    let expected = try layerBytes(original, layer: 0)
    let tiff = root.appendingPathComponent("ordinary16.tiff")
    try call("save_copy", original, ["path": tiff.path])
    for filename in ["ordinary16.png", "ordinary16.tiff"] {
        let renamed = root.appendingPathComponent(filename + ".dng")
        try FileManager.default.copyItem(at: root.appendingPathComponent(filename), to: renamed)
        let opened = try opened(renamed)
        defer { opened.close() }
        try require(opened.doc?.isHighDepth == true && opened.rawDevelop == nil,
                    "Ordinary high-depth container under RAW extension preserves depth without camera development")
        try require(try layerBytes(opened, layer: 0) == expected,
                    "Misnamed PNG/TIFF16 retains every original UInt16 code")
    }
    print("PASS: ordinary PNG16/TIFF16 under .dng extensions preserve every original code")
}
func scenario(root: URL, depth: Int) throws -> [String: Any] {
    let folder = root.appendingPathComponent("host-\(depth)")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    let input = folder.appendingPathComponent("original.dng")
    try FileManager.default.copyItem(at: root.appendingPathComponent("synthetic.dng"), to: input)
    let document = try opened(input, depth: depth)
    defer { document.close() }
    try require(document.doc?.isHighDepth == (depth == 16), "Requested fallback depth opens")
    let first = try sourceInfo(document)
    let initial = hash(try layerBytes(document, layer: 0))
    try verifyUICancel(document)
    if depth == 16 {
        try call("add_shape_layer", document, ["kind": "rect", "x": 12, "y": 12, "w": 42, "h": 30, "fill": "#00000000"])
    } else { try call("new_layer", document, ["name": "Independent retouch"]) }
    var brush: [String: Any] = ["layer": 1, "points": [[20, 20], [35, 25]], "size": 9, "color": "#4379ae", "opacity": 0.63]
    if depth == 16 { brush["rgba"] = [0.26237, 0.47361, 0.68219, 1.0] }
    try call("brush_stroke", document, brush)
    let retouch = hash(try layerBytes(document, layer: 1))
    let snapshot = try call("create_snapshot", document, ["name": "Initial RAW under retouch"])
    guard let snapshotID = snapshot["snapshot_id"] as? String else { throw ProbeFailure(description: "Missing snapshot") }
    let archive = folder.appendingPathComponent("initial.drsnapshot")
    try call("save_snapshot", document, ["snapshot_id": snapshotID, "path": archive.path])
    let native = folder.appendingPathComponent("developed.rz")
    try save(document, native)
    let saved = document.doc
    let revision = document.revision
    try call("redevelop_raw", document, ["layer": 0, "raw": [:]])
    try require(document.doc === saved && document.revision == revision && !document.isDocumentEdited,
                "Empty redevelopment leaves exact saved owner and Undo unchanged")
    try call("start_recording", document)
    try call("redevelop_raw", document, ["layer": 0, "raw": ["temperature": 7000, "tint": 17, "exposure": 0.35]])
    let recorded = try call("stop_recording", document, ["name": "RAW WB recipe \(depth)"])
    let steps = recorded["steps"] as? [[String: Any]] ?? []
    try require(steps.count == 1 && steps.first?["tool"] as? String == "redevelop_raw", "One RAW development step recorded")
    let developed = try sourceInfo(document)
    let changed = hash(try layerBytes(document, layer: 0))
    try require(changed != initial && hash(try layerBytes(document, layer: 1)) == retouch,
                "Development changes only RAW fallback below exact retouch")
    try call("undo", document)
    try require(document.doc === saved && !document.isDocumentEdited && hash(try layerBytes(document, layer: 0)) == initial,
                "One-step Undo restores exact saved RAW owner and clean marker")
    try call("redo", document)
    try require(hash(try layerBytes(document, layer: 0)) == changed && hash(try layerBytes(document, layer: 1)) == retouch,
                "Redo restores exact developed RAW and separate retouch")
    try call("undo", document)
    try call("run_action", document, ["name": "RAW WB recipe \(depth)"])
    try require(hash(try layerBytes(document, layer: 0)) == changed && hash(try layerBytes(document, layer: 1)) == retouch,
                "Recorded RAW action replays saved recipe without touching retouch")
    try call("redevelop_raw", document, ["layer": 0, "raw": ["as_shot_white_balance": true]])
    try require(hash(try layerBytes(document, layer: 0)) != changed, "As-shot reset changes captured custom white balance")
    try call("undo", document)
    let recipeBeforeExposure = try sourceInfo(document)["raw"] as? [String: Any] ?? [:]
    let whiteBalanceBefore = recipeBeforeExposure["recipe"] as? [String: Any] ?? [:]
    try call("redevelop_raw", document, ["layer": 0, "raw": ["exposure": 0.6]])
    let recipeAfterExposure = try sourceInfo(document)["raw"] as? [String: Any] ?? [:]
    let whiteBalanceAfter = recipeAfterExposure["recipe"] as? [String: Any] ?? [:]
    for key in ["chromaticityX", "chromaticityY"] {
        try require(whiteBalanceBefore[key] as? Double == whiteBalanceAfter[key] as? Double,
                    "Exposure-only redevelopment retains exact saved white balance chromaticity")
    }
    try call("redevelop_raw", document, ["layer": 0, "raw": ["exposure": 0.35]])
    try require(hash(try layerBytes(document, layer: 0)) == changed, "Exposure away and back restores every RAW code without WB drift")
    try FileManager.default.removeItem(at: input)
    try save(document, native)
    let recovery = try privateRecovery(document, directory: folder.appendingPathComponent("recovery"))
    let package = folder.appendingPathComponent("portable.rz")
    try save(document, package)
    let reopenPath = folder.appendingPathComponent("reopened.rz")
    try FileManager.default.copyItem(at: native, to: reopenPath)
    let reopened = try opened(reopenPath)
    try require(reopened !== document, "Native reopening materializes a new document owner")
    defer { reopened.close() }
    try require(hash(try layerBytes(reopened, layer: 0)) == changed && hash(try layerBytes(reopened, layer: 1)) == retouch,
                "Native reopening after original deletion retains exact RAW and retouch")
    try call("redevelop_raw", reopened, ["layer": 0, "raw": ["exposure": 0.6]])
    try require(hash(try layerBytes(reopened, layer: 1)) == retouch, "Embedded RAW redevelops without source file")
    try call("undo", reopened)
    let before = reopened.doc
    try call("redevelop_raw", reopened, ["layer": 1, "raw": ["exposure": 1]], refuses: true)
    try call("redevelop_raw", reopened, ["layer": 0, "raw": ["exposure": 100]], refuses: true)
    try require(reopened.doc === before, "Invalid target/recipe refuses without publication")
    let output = folder.appendingPathComponent("developed.png")
    try call("save_copy", reopened, ["path": output.path])
    return ["passed": true, "depth": depth, "first": first, "developed": developed,
        "initial_hash": initial, "developed_hash": changed, "retouch_hash": retouch,
        "native": native.path, "snapshot": archive.path, "package": package.path, "recovery": recovery,
        "groups": ["actual_open_depth", "actual_redevelop_ui_cancel", "retouch_above_raw", "actual_save_and_empty_noop", "recorded_redevelopment",
            "one_step_undo_redo_saved_identity", "recorded_action_replay", "as_shot_white_balance_reset", "exposure_preserves_white_balance", "snapshot_and_private_recovery", "portable_embedded_native",
            "original_deleted_reopen_and_redevelop", "strict_refusals", "flat_output"]]
}
let application = NSApplication.shared
AutomationAlerts.installIfEnabled()
application.setActivationPolicy(.accessory)
application.finishLaunching()
do {
    guard let directory = ProcessInfo.processInfo.environment["DR_RAW_PROBE_DIR"] else { throw ProbeFailure(description: "Missing private probe directory") }
    let root = URL(fileURLWithPath: directory, isDirectory: true)
    try verifyInitialUIDepth(root)
    try verifyRenamedHighDepthContainers(root)
    var cases = [[String: Any]]()
    for depth in [8, 16] {
        try autoreleasepool { cases.append(try scenario(root: root, depth: depth)) }
        for _ in 0..<10 { pump() }
        print("PASS: retained RAW host \(depth)-bit fallback, development/history/recovery/portability")
        fflush(stdout)
    }
    try JSONSerialization.data(withJSONObject: ["passed": true, "initial_ui_depth_choice": true, "renamed_high_depth_containers": true, "cases": cases], options: [.prettyPrinted, .sortedKeys])
        .write(to: root.appendingPathComponent("host-report.json"))
    exit(0)
} catch { print("FAIL: retained RAW host: \(error)"); exit(1) }
