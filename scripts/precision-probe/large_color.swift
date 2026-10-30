import AppKit

private struct LargeColorReference: Decodable {
    struct Sample: Decodable {
        let index: Int
        let source: [UInt16]
        let live: [Double]
        let destructive: [UInt16]
        let converted: [UInt16]
    }
    let width: Int
    let height: Int
    let samples: [Sample]
}

/// A separate fresh-process 24 MP workload. Stage-local projections and temporary
/// captures are released before the next stage. An old source necessarily stays
/// alive through a destructive/ICC call until its new owner has been returned.
func verifyLargeColorWorkflow() throws {
    let files = try root()
    let reference = try JSONDecoder().decode(LargeColorReference.self,
        from: Data(contentsOf: files.appendingPathComponent("large-color-reference.json")))
    try require(reference.width == 6000 && reference.height == 4000 && reference.samples.count == 257,
                "Fixed native 24 MP reference geometry and sample count")
    let destinationProfile = try Data(contentsOf: files.appendingPathComponent("destination-profile.icc"))
    var stages: [[String: Any]] = []
    var current: RasterPrecisionDocument?
    var savedLayers: Data?

    func stage(_ name: String, scope: String, _ operation: () throws -> Void) {
        let start = Date()
        var record: [String: Any] = ["stage": name, "retained_scope": scope,
                                     "before": (try? RasterResources.shared.report()) ?? [:]]
        do {
            try autoreleasepool { try operation() }
            record["status"] = "completed"
        } catch let error as ProbeFailure {
            record["status"] = "verification_failed"
            record["reason"] = error.description
        } catch {
            record["status"] = "refused"
            record["reason"] = error.localizedDescription
        }
        record["seconds"] = Date().timeIntervalSince(start)
        record["after"] = (try? RasterResources.shared.report()) ?? [:]
        stages.append(record)
        print("LARGE COLOR: \(name): \(record["status"] ?? "unknown") \(record["reason"] ?? "")")
        fflush(stdout)
    }

    func checkSource(_ document: RasterPrecisionDocument,
                     field: KeyPath<LargeColorReference.Sample, [UInt16]>, maximumError: Int) throws {
        guard let image = document.layerSource(0) else {
            throw ProbeFailure(description: "Missing native 24 MP raster source")
        }
        try require(image.width == 6000 && image.height == 4000, "Native24MP source dimensions")
        try image.withRGBA16 { pixels in
            for sample in reference.samples {
                let expected = sample[keyPath: field]
                try require(expected.count == 4, "Four independent native sample channels")
                for c in 0..<4 {
                    let difference = abs(Int(pixels[sample.index * 4 + c]) - Int(expected[c]))
                    try require(difference <= (c == 3 ? 0 : maximumError),
                                "24MP source sample \(sample.index)/\(c) error \(difference)")
                }
            }
        }
    }

    func layers(_ document: RasterPrecisionDocument) throws -> Data {
        try JSONSerialization.data(withJSONObject: document.info()["layers"] ?? [], options: .sortedKeys)
    }

    stage("preserving PNG16 import", scope: "one 192 MB source; codec input/scratch stage-local") {
        current = try openTyped(files.appendingPathComponent("large-source16.png"))
        guard let current else { throw ProbeFailure(description: "Input unavailable") }
        try checkSource(current, field: \.source, maximumError: 0)
    }
    stage("live Curves and calibrated Exposure in both group modes",
          scope: "all edit revisions share one 192 MB pixel payload; prepared descriptions and ICC contexts retained") {
        guard let source = current else { throw ProbeFailure(description: "Input unavailable") }
        let curve = try PrecisionAdjustment(op: .curves, params: ["rgb": [[0.0, 0.0], [255.0, 204.0]]])
        let exposure = try PrecisionAdjustment(op: .exposure, params: ["exposure": 0.25])
        guard let adjusted = try PrecisionCapturedEdit(source,
            operation: .add(name: "Continuous Curve", adjustment: curve, opacity: 0.5, anchor: 0, position: .top)).run(),
            let isolated = try adjusted.groupingRoots(opacity: 0.8),
            let exposed = try PrecisionCapturedEdit(isolated,
            operation: .add(name: "Calibrated Exposure", adjustment: exposure, opacity: 0.75, anchor: 0, position: .top)).run(),
            let passThrough = try exposed.groupingRoots(opacity: 0.9, passThrough: true) else {
            throw ProbeFailure(description: "Live stack did not produce the requested owners")
        }
        current = passThrough
        savedLayers = try layers(passThrough)
        try checkSource(passThrough, field: \.source, maximumError: 0)
    }
    stage("authoritative full-size live projection",
          scope: "current source and live stack plus one 192 MB output; renderer scratch and output released at stage end") {
        guard let current else { throw ProbeFailure(description: "Live stack unavailable") }
        let budget = try PrecisionWorkContext(source: current)
        let image = try current.render(workingLimit: budget.limit, otherRetained: budget.otherRetained)
        try require(image.width == 6000 && image.height == 4000, "Native24MP projection dimensions")
        try image.withRGBA16 { pixels in
            for sample in reference.samples {
                for c in 0..<4 {
                    let actual = Double(pixels[sample.index * 4 + c]) / 65535
                    let bound = c == 3 ? 2e-6 + 0.5 / 65535 : 2.0 / 65535
                    try require(abs(actual - sample.live[c]) <= bound,
                                "24MP live scalar reference error at \(sample.index)/\(c): \(abs(actual - sample.live[c]))")
                }
            }
        }
    }
    let native = files.appendingPathComponent("large-color-live.rz")
    stage("captured live native save", scope: "current source and captured alias share pixels; native encoding scratch only") {
        guard let current else { throw ProbeFailure(description: "Live stack unavailable") }
        try PrecisionCapturedSave(current, destination: .native).save(to: native)
    }
    stage("live native reopen", scope: "current source plus independent reopened 192 MB source; reopened owner released after checks") {
        let reopened = try openTyped(native)
        try checkSource(reopened, field: \.source, maximumError: 0)
        try require(try layers(reopened) == savedLayers, "Native preserves live operator/group summaries")
    }
    let archive = files.appendingPathComponent("large-color-archive")
    stage("captured live archive save", scope: "current source and captured alias; native encoding scratch and bounded manifest") {
        guard let current else { throw ProbeFailure(description: "Live stack unavailable") }
        try PrecisionStateArchive.save(PrecisionStateArchive.capture(current, name: "Native24MP live color"),
                                       to: archive)
    }
    stage("live archive reopen", scope: "current source plus independent reopened 192 MB source; reopened owner released after checks") {
        let loaded = try PrecisionStateArchive.load(from: archive)
        try checkSource(loaded.document, field: \.source, maximumError: 0)
        try require(try layers(loaded.document) == savedLayers, "Archive preserves live operator/group summaries")
    }

    current = nil
    stage("fresh source for destructive operations", scope: "prior live owners released; one newly imported 192 MB source") {
        current = try openTyped(files.appendingPathComponent("large-source16.png"))
    }
    stage("destructive Levels gamma0.8", scope: "old 192 MB source retained during creation of new 192 MB source; old owner then released") {
        guard let source = current else { throw ProbeFailure(description: "Destructive input unavailable") }
        let adjustment = try PrecisionAdjustment(op: .levels, params: ["gamma": 0.8])
        guard let result = try PrecisionCapturedEdit(source,
            operation: .apply(index: 0, adjustment: adjustment)).run() else {
            throw ProbeFailure(description: "Destructive Levels unexpectedly made no change")
        }
        current = result
        try checkSource(result, field: \.destructive, maximumError: 1)
    }
    stage("stored-source P3 to sRGB conversion", scope: "old 192 MB adjusted source retained during new 192 MB conversion; ICC tables/scratch stage-local") {
        guard let source = current else { throw ProbeFailure(description: "Adjusted input unavailable") }
        guard let result = try PrecisionCapturedEdit(source, operation: .convertProfile(destinationProfile)).run() else {
            throw ProbeFailure(description: "Different-profile conversion unexpectedly made no change")
        }
        current = result
        try checkSource(result, field: \.converted, maximumError: 2)
        try require(result.layerSource(0)?.profileData == destinationProfile, "Converted source carries exact destination ICC")
    }
    let convertedNative = files.appendingPathComponent("large-color-converted.rz")
    stage("converted captured native save", scope: "current converted 192 MB source; native encoding scratch only") {
        guard let current else { throw ProbeFailure(description: "Converted input unavailable") }
        try PrecisionCapturedSave(current, destination: .native).save(to: convertedNative)
    }
    stage("converted native reopen", scope: "current converted source plus independent reopened 192 MB source, then released") {
        let reopened = try openTyped(convertedNative)
        try checkSource(reopened, field: \.converted, maximumError: 2)
        try require(reopened.layerSource(0)?.profileData == destinationProfile, "Native retains converted ICC")
    }
    current = nil
    let report: [String: Any] = [
        "width": 6000, "height": 4000, "storage": "rgba16_unorm",
        "working_limit_bytes": RasterResources.shared.workingLimit,
        "complete_workflow": stages.allSatisfy { $0["status"] as? String == "completed" },
        "refusals_fail_size_target": true, "resizing_permitted": false,
        "live_structure": "source -> Curves0..204 atopacity0.5 -> isolated0.8 -> Exposure0.25stops atopacity0.75 -> pass-through0.9",
        "numeric_checks": "257 independent f64 samples; calibrated live RGB and ICC <=2u16codes; live alpha <=2e-6+0.5/65535; destructive <=1code, stored alpha exact",
        "retention": "one raster source; previous source remains through each destructive/ICC call only; temporary projections and reopened sources released between stages",
        "memory_measurement": "Live payload and dependency estimates, plus separately measured process RSS; working limit is not an OS memory ceiling",
        "stages": stages, "after_releasing_sources": try RasterResources.shared.report()
    ]
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        .write(to: files.appendingPathComponent("large-color-report.json"))
}
