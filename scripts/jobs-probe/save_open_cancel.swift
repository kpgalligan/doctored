import AppKit
import CryptoKit
import UniformTypeIdentifiers

// Save and Open show Cancel, and the source snapshot and fingerprint stages stop at it.
//
// Every row cancels from the job's own progress panel (`PanelCanceller`) while a `QueueHold`
// keeps the serial worker queue blocked until the click has landed, so the job's worker starts
// with its control already cancelled however fast it would run: the verdict never depends on load.
// A cancelled save completes with `NSUserCancelledError`, keeps the previous file byte for byte,
// leaves no staged name beside it and the document still edited; a cancelled open throws
// `JobCancelled` and opens nothing; an MCP call answers `JobCancelled.message` in-band. No row
// records an alert, and each leaves the worker slot free. The same call uncancelled then succeeds.
// On an app whose Save and Open take no control the panels have no Cancel, and every row fails
// with "the panel's Cancel was never clicked".

/// Holds `BackgroundWork.queue` until `canceller` has clicked (capped at 10 s).
private final class QueueHold {
    private let gate = ProbeGate()
    private var timer: Timer?

    init(until canceller: PanelCanceller) {
        let gate = gate
        BackgroundWork.queue.async {
            let deadline = Date().addingTimeInterval(10)
            while !gate.isOpen && Date() < deadline { usleep(1000) }
        }
        let timer = Timer(timeInterval: 0.005, repeats: true) { [weak canceller] _ in
            if canceller?.clickedAt != nil { gate.open() }
        }
        RunLoop.main.add(timer, forMode: .common)
        RunLoop.main.add(timer, forMode: .modalPanel)
        self.timer = timer
    }

    func stop() {
        gate.open()
        timer?.invalidate()
        timer = nil
    }
}

/// `body` with the panel titled `title` cancelled under a queue hold.
private func cancelling<T>(_ title: String, _ body: () throws -> T) rethrows -> (T, PanelCanceller) {
    let canceller = PanelCanceller(title: title)
    let hold = QueueHold(until: canceller)
    defer {
        hold.stop()
        canceller.stop()
    }
    return (try body(), canceller)
}

private func requireClicked(_ name: String, _ canceller: PanelCanceller) throws {
    try require(canceller.clickedAt != nil,
                "[\(name)] the panel's Cancel was never clicked (worker seen: \(canceller.sawOccupied))")
}

private func requireClean(_ name: String, alerts: Int) throws {
    try require(AutomationAlerts.recorded.count == alerts,
                "[\(name)] Cancel presented no alert: \(AutomationAlerts.recorded.suffix(2))")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "[\(name)] Cancel released the worker slot and its scratch")
}

private func names(_ folder: URL) -> [String] {
    ((try? FileManager.default.contentsOfDirectory(atPath: folder.path)) ?? []).sorted()
}

/// One NSDocument save, waited for; its completion error.
private func saveWaiting(_ document: ImageDocument, to url: URL, type: String,
                         _ operation: NSDocument.SaveOperationType) throws -> Error? {
    var done = false
    var failure: Error?
    document.save(to: url, ofType: type, for: operation) { error in
        failure = error
        done = true
    }
    try pumpUntil({ done }, seconds: 60)
    return failure
}

private final class SaveReply: NSObject {
    var didSave: Bool?
    @objc func document(_ document: NSDocument, didSave: Bool, contextInfo: UnsafeMutableRawPointer?) {
        self.didSave = didSave
    }
}

/// One save row: save, edit, save again and cancel, then save uncancelled.
private func verifySaveRow(_ name: String, _ document: ImageDocument, file: String, type: String,
                           _ operation: NSDocument.SaveOperationType) throws {
    let folder = try uiCaseFolder("save-cancel")
    let url = folder.appendingPathComponent(file)
    if let error = try saveWaiting(document, to: url, type: type, operation) {
        throw ProbeFailure(description: "[\(name)] the first save failed: \(error)")
    }
    let first = try Data(contentsOf: url)
    _ = try agentOK("duplicate_layer", document, ["layer": 0])
    try require(document.isDocumentEdited, "[\(name)] the edit dirtied the document")
    let alerts = AutomationAlerts.recorded.count
    let (error, canceller) = try cancelling("Saving “\(file)”") {
        try saveWaiting(document, to: url, type: type, operation)
    }
    try requireClicked(name, canceller)
    let failure = error as NSError?
    try require(failure?.domain == NSCocoaErrorDomain && failure?.code == NSUserCancelledError,
                "[\(name)] the cancelled save completes with NSUserCancelledError: \(String(describing: error))")
    let kept = try Data(contentsOf: url)
    try require(kept == first, "[\(name)] the cancelled save kept the previous file")
    try require(names(folder) == [file], "[\(name)] no staged file is left: \(names(folder))")
    try require(document.isDocumentEdited, "[\(name)] the document is still edited")
    try requireClean(name, alerts: alerts)
    if operation == .saveAsOperation {
        // The close-time path: a cancelled save answers didSave false, which stops the close.
        let reply = SaveReply()
        let (_, closing) = try cancelling("Saving “\(file)”") {
            document.save(withDelegate: reply,
                          didSave: #selector(SaveReply.document(_:didSave:contextInfo:)), contextInfo: nil)
            try pumpUntil({ reply.didSave != nil }, seconds: 60)
        }
        try requireClicked("\(name) close-time", closing)
        try require(reply.didSave == false && document.isDocumentEdited,
                    "[\(name)] a cancelled close-time save answers didSave false and stays edited")
        let still = try Data(contentsOf: url)
        try require(still == first, "[\(name)] the close-time cancel kept the file")
        try requireClean("\(name) close-time", alerts: alerts)
    }
    if let error = try saveWaiting(document, to: url, type: type, operation) {
        throw ProbeFailure(description: "[\(name)] the uncancelled save failed: \(error)")
    }
    try require(names(folder) == [file], "[\(name)] the uncancelled save left one file: \(names(folder))")
    if operation == .saveAsOperation {
        let saved = try Data(contentsOf: url)
        try require(saved != first && !document.isDocumentEdited,
                    "[\(name)] the uncancelled save wrote the edit and cleared the dirty state")
    }
    print("PASS: [\(name)] Save's panel Cancel keeps the previous file and the edit, and a retry saves")
}

/// One open row: cancelled, then opened.
private func verifyOpenRow(_ name: String, url: URL, title: String) throws {
    let alerts = AutomationAlerts.recorded.count
    let before = NSDocumentController.shared.documents
    var thrown: Error?
    let (_, canceller) = cancelling(title) {
        do { _ = try ImageDocument.openDocument(at: url) } catch { thrown = error }
    }
    try requireClicked(name, canceller)
    try require(thrown is JobCancelled, "[\(name)] a cancelled open throws JobCancelled: \(String(describing: thrown))")
    try require(uiNewDocuments(since: before).isEmpty, "[\(name)] a cancelled open opens nothing")
    try requireClean(name, alerts: alerts)
    let opened = try ImageDocument.openDocument(at: url)
    try require(opened.doc.width > 0, "[\(name)] the uncancelled open opens the file")
    print("PASS: [\(name)] Open's panel Cancel opens nothing, and a retry opens the file")
}

func verifySaveOpenCancel() throws {
    let previous = PrecisionImportRequest.mode
    PrecisionImportRequest.mode = .preserve
    defer { PrecisionImportRequest.mode = previous }
    let eight = try fixture(width: 1200, height: 1000)
    defer { eight.close() }
    try verifySaveRow("8-bit native", eight, file: "out.doctored",
                      type: ImageDocument.nativeTypeIdentifier, .saveAsOperation)
    try verifySaveRow("8-bit JPEG", eight, file: "out.jpg", type: UTType.jpeg.identifier, .saveToOperation)
    let sixteen = try precisionFixture(width: 800, height: 600)
    defer { sixteen.close() }
    try verifySaveRow("16-bit native", sixteen, file: "out16.doctored",
                      type: ImageDocument.nativeTypeIdentifier, .saveAsOperation)
    try verifySaveRow("16-bit TIFF", sixteen, file: "out16.tif", type: UTType.tiff.identifier,
                      .saveToOperation)
    // Save a Copy at 8 bits from a 16-bit document: downconvert, then encode, each a child stage.
    sixteen.exportBitDepth = 8
    try verifySaveRow("16-bit JPEG", sixteen, file: "out16.jpg", type: UTType.jpeg.identifier,
                      .saveToOperation)
    sixteen.exportBitDepth = 16

    let folder = try uiCaseFolder("open-cancel")
    let png = folder.appendingPathComponent("open.png")
    var rgba = [UInt8](repeating: 255, count: 640 * 480 * 4)
    for i in stride(from: 0, to: rgba.count, by: 4) { rgba[i] = UInt8(i / 4 % 251) }
    try uiWritePNG(rgba, width: 640, height: 480, to: png)
    try verifyOpenRow("8-bit PNG open", url: png, title: "Opening “open.png”")
    let native = folder.appendingPathComponent("open.doctored")
    if let error = try saveWaiting(eight, to: native, type: ImageDocument.nativeTypeIdentifier, .saveToOperation) {
        throw ProbeFailure(description: "[open] the native fixture was not written: \(error)")
    }
    try verifyOpenRow("8-bit native open", url: native, title: "Opening “open.doctored”")
    let tiff = folder.appendingPathComponent("open16.tif")
    if let error = try saveWaiting(sixteen, to: tiff, type: UTType.tiff.identifier, .saveToOperation) {
        throw ProbeFailure(description: "[open] the 16-bit fixture was not written: \(error)")
    }
    try verifyOpenRow("16-bit TIFF open", url: tiff, title: "Opening 16-bit Document")

    // MCP save_copy, cancelled from its panel, answers JobCancelled in-band and writes nothing.
    let copy = folder.appendingPathComponent("copy.png")
    let alerts = AutomationAlerts.recorded.count
    let (reply, canceller) = try cancelling("Exporting Copy") {
        try agentCall("save_copy", eight, ["path": copy.path, "format": "png"])
    }
    try requireClicked("save_copy", canceller)
    try require(reply.isError && reply.text.contains(JobCancelled.message),
                "[save_copy] a cancelled save_copy answers JobCancelled in-band: \(reply.text)")
    try require(!FileManager.default.fileExists(atPath: copy.path), "[save_copy] Cancel wrote nothing")
    try requireClean("save_copy", alerts: alerts)
    _ = try agentOK("save_copy", eight, ["path": copy.path, "format": "png"])
    try require(FileManager.default.fileExists(atPath: copy.path), "[save_copy] a retry writes the copy")
    print("PASS: [save_copy] its panel Cancel answers \"\(JobCancelled.message)\" and writes nothing")
}

/// The camera fixture `raw_fixtures.py` built (`DR_JOBS_PROBE_RAW_DIR`).
private func cameraFixture() throws -> URL {
    guard let folder = ProcessInfo.processInfo.environment["DR_JOBS_PROBE_RAW_DIR"], !folder.isEmpty else {
        throw ProbeFailure(description: "DR_JOBS_PROBE_RAW_DIR is not set (scripts/jobs-probe/verify.py sets it)")
    }
    let url = URL(fileURLWithPath: folder).appendingPathComponent("develop.dng")
    try require(FileManager.default.fileExists(atPath: url.path),
                "develop.dng is missing from \(folder); raw-fixtures.log says why")
    return url
}

func verifySnapshotFingerprintCancel() throws {
    let name = "snapshot fingerprint"
    let folder = try uiCaseFolder("fingerprint-cancel")
    let file = folder.appendingPathComponent("source.bin")
    var bytes = [UInt8](repeating: 0, count: 3 * 1024 * 1024 + 17)
    for i in bytes.indices { bytes[i] = UInt8(truncatingIfNeeded: i &* 2654435761 >> 13) }
    try Data(bytes).write(to: file)
    let cancelled = try RasterJobControl()
    cancelled.cancel()
    let copy = folder.appendingPathComponent("copy.bin")
    var thrown: Error?
    do { _ = try ImageSourceFile.fingerprint(file, copyingTo: copy, control: cancelled) } catch { thrown = error }
    try require(thrown is JobCancelled, "[\(name)] a cancelled fingerprint throws JobCancelled: \(String(describing: thrown))")
    let expected = SHA256.hash(data: Data(bytes)).map { String(format: "%02x", $0) }.joined()
    let hash = try ImageSourceFile.fingerprint(file, control: try RasterJobControl())
    try require(hash == expected, "[\(name)] an uncancelled fingerprint is the file's SHA-256")
    thrown = nil
    do { _ = try RawSourceSnapshot.capture(file, maximumBytes: UInt64(bytes.count), control: cancelled) }
    catch { thrown = error }
    try require(thrown is JobCancelled, "[\(name)] a cancelled RAW capture throws JobCancelled: \(String(describing: thrown))")
    print("PASS: [\(name)] the fingerprint and the RAW snapshot stop at Cancel; the hash is the file's")

    // The waits around them, from MCP: a camera RAW's capture, a still's read and a linked
    // source's check each answer JobCancelled in-band, and then run.
    let document = try fixture(width: 800, height: 600)
    defer { document.close() }
    let camera = try cameraFixture()
    let png = folder.appendingPathComponent("place.png")
    try uiWritePNG([UInt8](repeating: 128, count: 320 * 240 * 4), width: 320, height: 240, to: png)
    for (row, title, arguments) in [
        ("place RAW", "Capturing Camera RAW", ["path": camera.path, "storage": "linked"]),
        ("place image", "Reading Image", ["path": png.path]),
    ] as [(String, String, [String: Any])] {
        let alerts = AutomationAlerts.recorded.count
        let (reply, canceller) = try cancelling(title) { try agentCall("place_image", document, arguments) }
        try requireClicked(row, canceller)
        try require(reply.isError && reply.text.contains(JobCancelled.message),
                    "[\(row)] a cancelled place answers JobCancelled in-band: \(reply.text)")
        try requireClean(row, alerts: alerts)
        _ = try agentOK("place_image", document, arguments)
    }
    let layers = document.doc?.layerCount ?? 0
    guard let linked = (0..<layers).first(where: {
        ((try? document.doc?.retainedSourceInfo($0)) ?? nil)?["link"] != nil
    }) else {
        throw ProbeFailure(description: "[linked check] the linked RAW place left no linked layer")
    }
    let alerts = AutomationAlerts.recorded.count
    let (reply, canceller) = try cancelling("Checking Linked Image") {
        try agentCall("get_image_source", document, ["layer": linked])
    }
    try requireClicked("linked check", canceller)
    try require(reply.isError && reply.text.contains(JobCancelled.message),
                "[linked check] a cancelled check answers JobCancelled, not a link status: \(reply.text)")
    try requireClean("linked check", alerts: alerts)
    let source = try agentOK("get_image_source", document, ["layer": linked])["source"] as? [String: Any]
    try require(source?["link_status"] as? String == "current",
                "[linked check] an uncancelled check reads the link current: \(source ?? [:])")
    print("PASS: [snapshot waits] Capturing Camera RAW, Reading Image and Checking Linked Image stop at Cancel")
}
