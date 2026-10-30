import AppKit

/// Supplemental resource qualification. A refusal fails the24MP workflow target
/// and remains in the report; it is never replaced with a smaller image.
func verifyLargeWorkflow() throws {
    let directory = try root()
    var records: [[String: Any]] = []
    func stage(_ name: String, _ operation: () throws -> Void) {
        let started = Date()
        let before = try? RasterResources.shared.report()
        var row: [String: Any] = ["stage": name, "before": before ?? [:]]
        do {
            try autoreleasepool { try operation() }
            row["status"] = "completed"
        } catch {
            row["status"] = "refused"
            row["reason"] = error.localizedDescription
        }
        row["seconds"] = Date().timeIntervalSince(started)
        row["after"] = (try? RasterResources.shared.report()) ?? [:]
        records.append(row)
        print("LARGE: \(name): \(row["status"] ?? "unknown") \(row["reason"] ?? "")")
        fflush(stdout)
    }
    var source: RasterPrecisionDocument?
    var grouped: RasterPrecisionDocument?
    stage("native24MP PNG16 import") {
        source = try openTyped(directory.appendingPathComponent("large-source16.png"))
        guard let source else { throw ProbeFailure(description: "Missing large source") }
        try verifySource(source, width: 6000, height: 4000, allPixels: false)
    }
    stage("duplicate and isolated group") {
        guard let source, let duplicate = try source.duplicateLayer(0),
              let changed = try duplicate.settingLayerOpacity(1, opacity: 0.5),
              let result = try changed.groupingRoots(opacity: 0.75) else {
            throw ProbeFailure(description: "Prior typed source/structural result unavailable")
        }
        grouped = result
        try verifySource(result, at: 0, width: 6000, height: 4000, allPixels: false)
        try verifySource(result, at: 1, width: 6000, height: 4000, allPixels: false)
    }
    stage("authoritative grouped projection and explicit display copy") {
        guard let grouped else { throw ProbeFailure(description: "Prior group unavailable") }
        let budget = try PrecisionWorkContext(source: grouped)
        let projection = try grouped.render(workingLimit: budget.limit, otherRetained: budget.otherRetained)
        try require(projection.width == 6000 && projection.height == 4000, "Native projection dimensions")
        let previewBudget = try PrecisionWorkContext(image: projection)
        let preview = try projection.previewRGBA8(workingLimit: previewBudget.limit,
                                                  otherRetained: previewBudget.otherRetained)
        try require(preview.width == 6000 && preview.height == 4000, "Explicit full-size display copy")
        try projection.withRGBA16 { samples in
            for sample in 0...256 {
                let i = sample * (6000 * 4000 - 1) / 256
                let original = expected(i)
                let alpha = Double(original[3]) / 65535
                let childrenAlpha = alpha + alpha * 0.5 * (1 - alpha)
                let reference = (0..<3).map { alpha == 0 ? 0 : Double(original[$0]) / 65535 }
                    + [childrenAlpha * 0.75]
                for channel in 0..<4 {
                    let actual = Double(samples[i * 4 + channel]) / 65535
                    try require(abs(actual - reference[channel]) <= 2e-6 + 0.5 / 65535,
                                "Native 24 MP projection agrees with independent f64 reference")
                }
                guard let bytes = preview.pixelRGBA(x: i % 6000, y: i / 6000) else {
                    throw ProbeFailure(description: "Missing native-size preview sample")
                }
                let actualBytes = [bytes.r, bytes.g, bytes.b, bytes.a]
                for channel in 0..<4 {
                    let nearest = UInt8((UInt32(samples[i * 4 + channel]) + 128) / 257)
                    try require(actualBytes[channel] == nearest,
                                "Explicit byte preview uses nearest rounding from typed projection")
                }
            }
        }
    }
    let nativeURL = directory.appendingPathComponent("large-grouped.rz")
    stage("captured grouped native save") {
        guard let grouped else { throw ProbeFailure(description: "Prior group unavailable") }
        try PrecisionCapturedSave(grouped, destination: .native).save(to: nativeURL)
    }
    stage("native reopen and source precision") {
        let reopened = try openTyped(nativeURL)
        try verifySource(reopened, at: 0, width: 6000, height: 4000, allPixels: false)
        try verifySource(reopened, at: 1, width: 6000, height: 4000, allPixels: false)
    }
    for (format, filename) in [(PrecisionExportFormat.png16, "large-export16.png"), (.tiff16, "large-export16.tiff")] {
        stage("captured \(filename)") {
            guard let grouped else { throw ProbeFailure(description: "Prior group unavailable") }
            try PrecisionCapturedSave(grouped, destination: .image(format))
                .save(to: directory.appendingPathComponent(filename))
        }
    }
    let archiveURL = directory.appendingPathComponent("large-archive", isDirectory: true)
    stage("captured host archive save") {
        guard let grouped else { throw ProbeFailure(description: "Prior group unavailable") }
        try PrecisionStateArchive.save(PrecisionStateArchive.capture(grouped, name: "Native24MP"), to: archiveURL)
    }
    stage("captured host archive reopen") {
        let reopened = try PrecisionStateArchive.load(from: archiveURL)
        try verifySource(reopened.document, at: 0, width: 6000, height: 4000, allPixels: false)
        try verifySource(reopened.document, at: 1, width: 6000, height: 4000, allPixels: false)
    }
    source = nil
    grouped = nil
    let report: [String: Any] = [
        "width": 6000, "height": 4000, "storage": "rgba16_unorm",
        "working_limit_bytes": RasterResources.shared.workingLimit,
        "complete_workflow": records.allSatisfy { $0["status"] as? String == "completed" },
        "refusals_fail_size_target": true, "resizing_permitted": false,
        "source_checks": "257 distributed native UInt16 source pixels; full small fixture checked separately",
        "projection_checks": "257 distributed samples against independent f64 source-over; error <=2e-6+0.5/65535; explicit preview nearest byte rounding",
        "memory_measurement": "Live payload reports plus separate process RSS/time; not an OS memory ceiling",
        "stages": records, "after_releasing_sources": try RasterResources.shared.report()
    ]
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        .write(to: directory.appendingPathComponent("large-report.json"))
}
