import AppKit

/// One bounded public-mode workflow. Unlike the earlier internal measurements,
/// ImageDocument retains its real projection, history, saved owner and window.
func verifyLargePublicWorkflow() throws {
    let directory = try root()
    let width = 6000, height = 4000, count = 24_000_000
    let painted = [1: UInt16(1), 7: UInt16(32768), 2000 * width + 3007: UInt16(65535)]
    let indices = Array(Set((0...256).map { $0 * (count - 1) / 256 } + Array(painted.keys))).sorted()
    let profile = try Data(contentsOf: directory.appendingPathComponent("source-profile.icc"))
    var current: ImageDocument?
    weak var primary: ImageDocument?
    weak var closedReopen: ImageDocument?
    var records: [[String: Any]] = []
    var originalIdentity: ObjectIdentifier?
    var paintedIdentity: ObjectIdentifier?
    var committedSamples: [Int: [UInt16]] = [:]
    var exportAdmission: [String: UInt64] = [:]
    let ink = try PrecisionInk(red: 0.234567, green: 0.456789, blue: 0.678912, alpha: 0.75)
    func requireDocument() throws -> ImageDocument {
        guard let current else { throw ProbeFailure(description: "Public document is unavailable") }
        return current
    }
    func source(_ document: ImageDocument) throws -> RasterPrecisionImage {
        guard let facade = document.doc, let image = facade.precision?.layerSource(0),
              facade.width == width && facade.height == height, image.width == width && image.height == height else {
            throw ProbeFailure(description: "Public document changed the requested native24MP extent or precision")
        }
        try require(facade.ptr == nil && document.projection == nil, "Public typed mode has no byte master")
        try require(image.profileData == profile, "Exact source profile survives the public workflow")
        return image
    }
    func stage(_ name: String, _ scope: String, settled: (() throws -> Bool)? = nil,
               _ work: () throws -> Void) {
        let started = Date()
        var row: [String: Any] = ["stage": name, "retained_scope": scope]
        do {
            row["before"] = try RasterResources.shared.report()
            try autoreleasepool { try work() }
            if let settled {
                row["before_settling"] = try RasterResources.shared.report()
                let settlingStart = Date()
                var turns = 0
                while try !settled(), Date().timeIntervalSince(settlingStart) < 2 {
                    pumpPublicProbeEvents()
                    turns += 1
                }
                row["settling_seconds"] = Date().timeIntervalSince(settlingStart)
                row["settling_event_turns"] = turns
                try require(try settled(), "Closed public owners did not release within the two-second lifecycle bound")
            }
            row["status"] = "completed"
            if let current {
                let history = current.history.report()
                try require(history["accounting_error"] == nil, "History accounting must succeed")
                row["history"] = history
            }
        } catch let error as ProbeFailure { row["status"] = "verification_failed"; row["reason"] = error.description }
        catch { row["status"] = "refused"; row["reason"] = error.localizedDescription }
        row["seconds"] = Date().timeIntervalSince(started)
        do { row["after"] = try RasterResources.shared.report() }
        catch { row["status"] = "verification_failed"; row["resource_report_error"] = String(describing: error) }
        records.append(row)
        print("LARGE PUBLIC: \(name): \(row["status"] ?? "unknown") \(row["reason"] ?? "")")
        fflush(stdout)
    }
    func originalSamples(_ image: RasterPrecisionImage) throws {
        try image.withRGBA16 { pixels in
            for i in indices { try require(Array(pixels[i * 4..<i * 4 + 4]) == expected(i), "Exact native24MP source sample") }
        }
    }
    func storedSamples(_ image: RasterPrecisionImage, exported: Bool = false) throws {
        try image.withRGBA16 { pixels in
            for i in indices {
                var wanted = committedSamples[i] ?? expected(i)
                if exported && wanted[3] == 0 { wanted = [0, 0, 0, 0] }
                for c in 0..<4 {
                    let difference = abs(Int(pixels[i * 4 + c]) - Int(wanted[c]))
                    try require(difference <= (exported && c < 3 ? 1 : 0),
                                "Stored/exported u16 samples retain exact alpha and source RGB precision")
                }
            }
        }
    }
    func awaitDisplay(_ document: ImageDocument) throws {
        let deadline = Date().addingTimeInterval(30)
        while Date() < deadline && (document.precisionProjectionTimer != nil || BackgroundWork.occupied || document.precisionProjection == nil) {
            RunLoop.main.run(until: Date().addingTimeInterval(0.005))
        }
        try require(document.precisionProjectionTimer == nil && !BackgroundWork.occupied
            && document.precisionProjection != nil && document.previewImage != nil && document.projectionError == nil,
            "Current authoritative projection and public display finish within bound")
    }
    stage("public preserving open", "real ImageDocument/window with192MB typed source,192MB projection, display and saved/history owners") {
        let oldMode = PrecisionImportRequest.mode
        PrecisionImportRequest.mode = .preserve
        defer { PrecisionImportRequest.mode = oldMode }
        let document = try openPublicProbeDocument(directory.appendingPathComponent("large-source16.png"), show: true)
        current = document
        primary = document
        try awaitDisplay(document)
        try originalSamples(source(document))
        if let facade = document.doc { originalIdentity = ObjectIdentifier(facade) }
        try require(!document.isDocumentEdited, "Public preserving open starts clean")
    }
    stage("one captured typed paint", "real old source/projection and Undo retained;48MB u16 coverage and new source/projection overlap during publication") {
        let document = try requireDocument()
        let capture = try PrecisionEditorCapture(document)
        _ = try RasterResources.shared.admit(scratch: UInt64(count) * 2 + 512)
        let coverage = try PrecisionScalarBuffer(samples: (0..<count).map { painted[$0] ?? 0 }, width: width, height: height)
        let edit = try PrecisionPixelEdit(capture.source, command: .paint(layer: 0, ink: ink, space: .document, opacity: 0.625), input: coverage)
        let result = try edit.run()
        try require(try capture.publish(result, to: document, name: "Public Native16 Paint"), "Public paint must change its typed owner")
        try awaitDisplay(document)
        let image = try source(document)
        try image.withRGBA16 { pixels in
            for i in indices {
                let actual = Array(pixels[i * 4..<i * 4 + 4])
                if let coverage = painted[i] {
                    let old = expected(i).map { Double($0) / 65535.0 }
                    let sa = 0.75 * 0.625 * Double(coverage) / 65535.0
                    let alpha = sa + old[3] * (1 - sa)
                    let reference = (0..<3).map { (ink.rgba[$0] * sa + old[$0] * old[3] * (1 - sa)) / alpha } + [alpha]
                    for c in 0..<4 { try require(abs(Double(actual[c]) / 65535.0 - reference[c]) <= 2e-6 + 0.5 / 65535.0,
                                                "Independent f64 public paint reference") }
                    try require(actual[3] == expected(i)[3], "Chosen paint samples preserve exact tiny/opaque alpha")
                } else { try require(actual == expected(i), "Public paint preserves unselected raw samples") }
                committedSamples[i] = actual
            }
        }
        if let facade = document.doc { paintedIdentity = ObjectIdentifier(facade) }
        try require(document.isDocumentEdited && document.undoManager?.canUndo == true
            && document.history.entries.count == 2 && document.history.cursor == 1,
                    "Public paint creates exactly one central dirty/Undo transition")
    }
    stage("public Undo", "current original source/projection plus painted source retained by Redo; no full buffers dropped from history") {
        let document = try requireDocument()
        document.undoManager?.undo()
        try awaitDisplay(document)
        if let facade = document.doc { try require(ObjectIdentifier(facade) == originalIdentity, "One Undo restores original facade") }
        try originalSamples(source(document))
        try require(!document.isDocumentEdited && document.history.cursor == 0
            && document.undoManager?.canUndo == false && document.undoManager?.canRedo == true,
                    "Undo to exact saved owner is clean with one reversible transition")
    }
    stage("public Redo", "current painted source/projection; original source remains the saved/Undo owner") {
        let document = try requireDocument()
        document.undoManager?.redo()
        try awaitDisplay(document)
        if let facade = document.doc { try require(ObjectIdentifier(facade) == paintedIdentity, "One Redo restores painted facade") }
        try storedSamples(source(document))
        try require(document.isDocumentEdited && document.history.cursor == 1
            && document.undoManager?.canRedo == false, "Redo restores dirty state exactly once")
    }
    let native = directory.appendingPathComponent("large-public-native.rz")
    stage("public native Save", "captured facade aliases current source; real history/projection remain retained during native encoding") {
        let document = try requireDocument()
        var complete = false
        var error: Error?
        document.save(to: native, ofType: ImageDocument.nativeTypeIdentifier, for: .saveAsOperation) {
            error = $0; complete = true
        }
        let deadline = Date().addingTimeInterval(60)
        while !complete && Date() < deadline { RunLoop.main.run(until: Date().addingTimeInterval(0.005)) }
        try require(complete, "Actual public native Save completes")
        if let error { throw error }
        try require(!document.isDocumentEdited && document.historySavedDocument === document.doc,
                    "Native Save marks only current captured owner clean")
    }
    stage("public native reopen", "original window/history/projection coexist with second public source/projection only through checks; bounded actual event turns then release the closed temporary", settled: {
        let report = try RasterResources.shared.report()
        guard closedReopen == nil && current?.history.entries.count == 2
            && current?.history.cursor == 1 else { return false }
        return try value(report, "host_document_handles") == 2
            && value(report, "host_precision_document_handles") == 2
            && value(report, "host_precision_image_handles") == 1
            && value(report, "host_image_handles") == 1
    }) {
        let reopened = try openPublicProbeDocument(native, show: false)
        closedReopen = reopened
        defer { reopened.close() }
        try awaitDisplay(reopened)
        try storedSamples(source(reopened))
    }
    let png = directory.appendingPathComponent("large-public-export16.png")
    stage("public PNG16 export", "real window/history/projection remain live; explicit captured16bit export plans its full codec peak") {
        let document = try requireDocument()
        guard let facade = document.doc else { throw ProbeFailure(description: "Missing export source") }
        let capture = try PrecisionExportSource(document)
        try require(capture.source === facade && capture.projection === document.precisionProjection,
                    "Public export reuses the current authoritative projection and exact source capture")
        let export = try PrecisionExportCapture(source: capture, format: .png, bitDepth: 16, dither: "none")
        let inventory = try RasterResources.shared.report()
        let retained = try value(inventory, "unique_retained_bytes")
        let transient = try value(inventory, "transient_estimate_bytes")
        exportAdmission = ["retained_bytes": retained, "scratch_bytes": export.scratchEstimate,
            "other_transient_bytes": transient, "working_limit_bytes": RasterResources.shared.workingLimit,
            "estimated_peak_bytes": try PrecisionDocumentDescriptor.sum(
                PrecisionDocumentDescriptor.sum(retained, transient), export.scratchEstimate)]
        let report = try export.run(to: png)
        try require(report.carries(DR_CARRIES_PROFILE), "Public PNG16 export carries original profile")
        try require(document.doc === facade && !document.isDocumentEdited, "Explicit export leaves master and saved identity unchanged")
    }
    stage("PNG16 preserving verification", "public window/history retained; exported flat typed source is reopened only for distributed checks") {
        let image = try openTyped(png)
        guard let plane = image.layerSource(0) else { throw ProbeFailure(description: "PNG16 exported source missing") }
        try require(plane.width == width && plane.height == height && plane.profileData == profile,
                    "PNG16 stays full-size with original exact profile")
        try storedSamples(plane, exported: true)
    }
    let zeroFields = ["host_document_handles", "host_image_handles", "host_precision_document_handles", "host_precision_image_handles",
                      "host_precision_capture_payloads", "precision_capture_payload_bytes", "unique_retained_bytes", "transient_estimate_bytes"]
    stage("close public window and release", "close actual document/window; bounded actual event turns must release all history, projection and metadata leases", settled: {
        let report = try RasterResources.shared.report()
        guard primary == nil else { return false }
        return try zeroFields.allSatisfy { try value(report, $0) == 0 }
    }) {
        current?.close()
        current = nil
    }
    let final = try RasterResources.shared.report()
    let released = try zeroFields.allSatisfy { try value(final, $0) == 0 }
    let report: [String: Any] = ["width": width, "height": height,
        "working_limit_bytes": RasterResources.shared.workingLimit,
        "complete_workflow": released && records.allSatisfy { $0["status"] as? String == "completed" },
        "refusals_fail_size_target": true, "resizing_permitted": false,
        "public_document_window": true, "all_resources_released": released,
        "export_admission": exportAdmission,
        "numeric_checks": "257distributed+3paint samples; exact original/uncovered/native codes and profile; independentf64 paint bound2e-6+0.5/65535; chosen tiny/opaque alpha exact; PNG16 source RGB<=1code and alpha exact after flattening transparent pixels.",
        "timing_scope": "Swift-Onone source-linked host plus ReleaseRust; includes actual window/projection/history and host coverage construction, not optimizedUI latency",
        "stages": records, "after_releasing_sources": final]
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        .write(to: directory.appendingPathComponent("large-public-report.json"))
}
