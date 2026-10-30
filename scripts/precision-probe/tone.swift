import Foundation

private let toneErrorBound = 2e-6 + 0.5 / 65535.0

private func changed(_ source: RasterPrecisionDocument, _ operation: PrecisionCapturedEdit.Operation)
    throws -> RasterPrecisionDocument {
    guard let result = try PrecisionCapturedEdit(source, operation: operation).run() else {
        throw ProbeFailure(description: "Expected a changed typed owner")
    }
    return result
}

private func projected(_ document: RasterPrecisionDocument) throws -> RasterPrecisionImage {
    let context = try PrecisionWorkContext(source: document)
    return try document.render(workingLimit: context.limit, otherRetained: context.otherRetained)
}

private func rampCode(_ pixel: Int, _ channel: Int) -> Int {
    [pixel, (pixel * 13) % 65536, (pixel * 173) % 65536][channel]
}

private func checkRamp(_ image: RasterPrecisionImage, _ label: String,
                       expected: (Double) -> Double) throws {
    try require(image.width == 65536 && image.height == 1, "\(label) native ramp geometry")
    try image.withRGBA16 { output in
        var maximum = 0.0
        var unique = Set<UInt16>()
        for i in 0..<65536 {
            try require(output[i * 4 + 3] == 65535, "\(label) exact alpha")
            unique.insert(output[i * 4])
            for c in 0..<3 {
                let reference = expected(Double(rampCode(i, c)) / 65535)
                maximum = max(maximum, abs(Double(output[i * 4 + c]) / 65535 - reference))
            }
        }
        try require(maximum <= toneErrorBound,
                    "\(label) f64 error \(maximum) exceeds frozen \(toneErrorBound)")
        try require(unique.count > 1024, "\(label) retains more than byte-sized tonal steps")
        print("MEASURE: \(label), maximum_error=\(maximum), unique_red_codes=\(unique.count)")
    }
}

func verifyPrecisionTone() throws {
    let source = try openTyped(root().appendingPathComponent("tone-source16.png"))
    let levels = try PrecisionAdjustment(op: .levels, params: ["gamma": 0.1])
    let curved = try PrecisionAdjustment(op: .curves, params: ["rgb": [[0.0, 0.0], [255.0, 127.5]]])
    let severe = try changed(source, .apply(index: 0, adjustment: levels))
    guard let severePixels = severe.layerSource(0) else {
        throw ProbeFailure(description: "Missing destructively adjusted source")
    }
    try checkRamp(severePixels, "Severe Levels gamma0.1") { pow($0, 10) }
    let curve = try changed(source, .add(name: "Continuous Curves", adjustment: curved, opacity: 1, anchor: 0, position: .top))
    try checkRamp(projected(curve), "Continuous two-point Curves") { $0 * 0.5 }
    let replacement = try changed(curve, .set(index: 1, adjustment: levels))
    try checkRamp(projected(replacement), "Replaced live Levels") { pow($0, 10) }
    try require(try PrecisionCapturedEdit(replacement, operation: .set(index: 1, adjustment: levels)).run() == nil,
                "Setting identical typed adjustment is a no-op")

    let tiny = try PrecisionAdjustment(op: .bcs, params: ["brightness": 0.000001])
    var stack = source
    for i in 0..<64 {
        stack = try changed(stack, .add(name: "Sub-code step \(i)", adjustment: tiny, opacity: 1, anchor: 0, position: .top))
    }
    try checkRamp(projected(stack), "64 live sub-code brightness steps") { min(1, $0 + 64e-6) }
    let baked = try PrecisionCapturedEdit(source, operation: .apply(index: 0, adjustment: tiny)).run() ?? source
    guard let bakedSource = baked.layerSource(0) else {
        throw ProbeFailure(description: "Missing baked tiny-step source")
    }
    try checkRamp(bakedSource, "One destructive sub-code step rounds once") { $0 }
    try require(try value(source.info(), "layer_count") == 1, "Original ramp remains immutable")
    print("PASS: severe ramp, continuous curves, live float stack, exact no-op and destructive rounding")
}

func verifyPrecisionProfiles() throws {
    let files = try root()
    let source = try openTyped(files.appendingPathComponent("source16.png"))
    let originalProfile = try Data(contentsOf: files.appendingPathComponent("source-profile.icc"))
    let destination = try Data(contentsOf: files.appendingPathComponent("destination-profile.icc"))
    let expectedBytes = try Data(contentsOf: files.appendingPathComponent("expected-converted16.bin"))
    try require(expectedBytes.count == 32 * 12 * 8, "Independent ICC reference dimensions")
    let assigned = try changed(source, .assignProfile(destination))
    guard let assignedPixels = assigned.layerSource(0) else {
        throw ProbeFailure(description: "Missing assigned source")
    }
    try require(assignedPixels.profileData == destination, "Assignment installs exact destination ICC")
    try assignedPixels.withRGBA16 { samples in
        for i in 0..<(32 * 12) {
            try require(Array(samples[i * 4..<i * 4 + 4]) == expected(i),
                        "Assignment preserves every sample including hidden RGB")
        }
    }
    let converted = try changed(source, .convertProfile(destination))
    guard let convertedPixels = converted.layerSource(0) else {
        throw ProbeFailure(description: "Missing converted source")
    }
    try require(convertedPixels.profileData == destination, "Conversion installs exact destination ICC")
    var maximum = 0
    try convertedPixels.withRGBA16 { output in
        for i in 0..<(32 * 12) {
            try require(output[i * 4 + 3] == expected(i)[3], "ICC conversion preserves exact alpha")
            for c in 0..<3 {
                let offset = (i * 4 + c) * 2
                let wanted = Int(expectedBytes[offset]) | Int(expectedBytes[offset + 1]) << 8
                maximum = max(maximum, abs(Int(output[i * 4 + c]) - wanted))
            }
        }
        try require(output[0] != expected(0)[0], "ICC conversion includes hidden RGB")
    }
    try require(maximum <= 2, "Independent f64 matrix/TRC error exceeds predeclared two u16 codes: \(maximum)")
    try require(try PrecisionCapturedEdit(source, operation: .assignProfile(originalProfile)).run() == nil,
                "Identical profile assignment is a no-op")
    try require(try PrecisionCapturedEdit(converted, operation: .convertProfile(destination)).run() == nil,
                "Identical profile conversion is a no-op")
    let exposure = try PrecisionAdjustment(op: .exposure, params: ["exposure": 0.75])
    let live = try changed(source, .add(name: "Profile-aware Exposure", adjustment: exposure, opacity: 0.6, anchor: 0, position: .top))
    let liveAssigned = try changed(live, .assignProfile(destination))
    let liveConverted = try changed(live, .convertProfile(destination))
    let originalLayers = try JSONSerialization.data(withJSONObject: live.info()["layers"] ?? [], options: .sortedKeys)
    for updated in [liveAssigned, liveConverted] {
        let layers = try JSONSerialization.data(withJSONObject: updated.info()["layers"] ?? [], options: .sortedKeys)
        try require(layers == originalLayers, "Profile edits preserve live operator summaries and grouping")
    }
    let convertedThenAdjusted = try changed(converted,
        .add(name: "Profile-aware Exposure", adjustment: exposure, opacity: 0.6, anchor: 0, position: .top))
    let convertedLivePixels = try projected(liveConverted)
    let preparedDestinationPixels = try projected(convertedThenAdjusted)
    try convertedLivePixels.withRGBA16 { first in
        try preparedDestinationPixels.withRGBA16 { second in
            try require(first.elementsEqual(second),
                        "Converted live parameters are prepared in the destination profile")
        }
    }
    let token = try RasterJobControl()
    token.cancel()
    let capture = try PrecisionCapturedEdit(source, operation: .convertProfile(destination))
    try refuses("Cancelled captured ICC conversion refuses") { _ = try capture.compute(control: token) }
    try require(token.scratchBytes == 0, "Cancelled conversion releases core reservation")
    try refuses("Insufficient conversion budget refuses without a byte fallback") {
        _ = try source.convertingToProfile(destination, workingLimit: 1, otherRetained: 0)
    }
    try refuses("ICC planning accounts other retained owners before parsing") {
        _ = try source.colorPlan(profile: destination, convert: true,
                                 workingLimit: RasterResources.shared.workingLimit,
                                 otherRetained: RasterResources.shared.workingLimit)
    }
    try refuses("Malformed ICC refuses without profile adoption") {
        _ = try PrecisionCapturedEdit(source, operation: .assignProfile(Data([0, 1, 2, 3]))).run()
    }
    try verifySource(source)
    print("PASS: exact profile assignment, scalar ICC conversion (max \(maximum) codes), alpha, cancellation and admission")
}

func verifyPrecisionTonePersistence() throws {
    let files = try root()
    let source = try openTyped(files.appendingPathComponent("source16.png"))
    let invert = try PrecisionAdjustment(op: .invert)
    let live = try changed(source, .add(name: "Captured Invert", adjustment: invert, opacity: 0.375, anchor: 0, position: .top))
    let oldNative = try PrecisionCapturedSave(live, destination: .native)
    let oldExport = try PrecisionCapturedSave(live, destination: .image(.png16))
    let archive = try PrecisionStateArchive.capture(live, name: "Captured live adjustment")
    let curved = try PrecisionAdjustment(op: .curves, params: ["rgb": [[0.0, 0.0], [255.0, 127.5]]])
    let latest = try changed(live, .set(index: 1, adjustment: curved))
    try require(latest !== oldNative.source, "Save/capture retains pre-replacement adjustment owner")
    let path = files.appendingPathComponent("adjusted-current.rz")
    try oldNative.save(to: path)
    let header = try Data(contentsOf: path).prefix(8)
    // Every typed save carries the layer-identity wrapper (`identity_native.rs`),
    // the outermost version word, so the current writer's version is 46.
    try require(Array(header.suffix(4)) == [46, 0, 0, 0], "Current writer uses outermost native version46")
    let reopened = try openTyped(path)
    let description = try JSONSerialization.data(withJSONObject: live.info()["layers"] ?? [], options: .sortedKeys)
    let recovered = try JSONSerialization.data(withJSONObject: reopened.info()["layers"] ?? [], options: .sortedKeys)
    try require(description == recovered, "Native persists bounded live operator summaries and structure")
    let compact = try reopened.descriptor()
    try require(compact["layers"] == nil && compact["import_warnings"] == nil,
                "Capture descriptor omits unbounded layer/detail lists")
    try verifySource(reopened)
    let oldPixels = try projected(live)
    let reopenedPixels = try projected(reopened)
    try oldPixels.withRGBA16 { before in
        try reopenedPixels.withRGBA16 { after in
            try require(before.elementsEqual(after), "Native live rendering is exactly reproducible")
        }
    }
    try oldExport.save(to: files.appendingPathComponent("captured-adjustment16.png"))
    let exported = try openTyped(files.appendingPathComponent("captured-adjustment16.png"))
    guard let exportPixels = exported.layerSource(0) else {
        throw ProbeFailure(description: "Missing captured export source")
    }
    try oldPixels.withRGBA16 { before in
        try exportPixels.withRGBA16 { after in
            try require(before.elementsEqual(after), "Flat export renders the captured old adjustment")
        }
    }
    let package = files.appendingPathComponent("adjusted-archive")
    try PrecisionStateArchive.save(archive, to: package)
    let loaded = try PrecisionStateArchive.load(from: package)
    try require(loaded.name == "Captured live adjustment", "Typed archive retains captured name")
    let loadedDescription = try JSONSerialization.data(withJSONObject: loaded.document.info()["layers"] ?? [],
                                                        options: .sortedKeys)
    try require(loadedDescription == description, "Typed archive preserves captured live operator summaries")
    try require(archive.manifest.version == 2 && archive.manifest.kind == "precision-document",
                "New archive explicitly versions support for typed adjustments")

    for passThrough in [false, true] {
        guard let group = try live.groupingRoots(opacity: 0.625, passThrough: passThrough) else {
            throw ProbeFailure(description: "Missing adjusted group")
        }
        let image = try projected(group)
        try image.withRGBA16 { output in
            for i in 0..<(32 * 12) {
                let input = expected(i)
                let alpha = Double(input[3]) / 65535
                try require(abs(Double(output[i * 4 + 3]) / 65535 - alpha * 0.625) <= toneErrorBound,
                            "Adjusted group exact alpha gate")
                for c in 0..<3 {
                    let x = Double(input[c]) / 65535
                    let reference = alpha == 0 ? 0 : x * 0.625 + (1 - x) * 0.375
                    try require(abs(Double(output[i * 4 + c]) / 65535 - reference) <= toneErrorBound,
                                "Adjusted group independent backdrop scalar reference")
                }
            }
        }
    }
    print("PASS: native13, immutable captured native/export/archive, and adjustment evaluation inside both group modes")
}

func verifyPrecisionCompatibility() throws {
    let files = try root()
    let native = try openTyped(files.appendingPathComponent("independent-v11.rz"))
    try verifySource(native)
    let oldArchive = try PrecisionStateArchive.load(from: files.appendingPathComponent("independent-v1-archive"))
    try verifySource(oldArchive.document)
    try require(oldArchive.name == "Phase7B captured source", "Independent archive v1 remains readable")
    let old12 = try openTyped(files.appendingPathComponent("independent-v12.rz"))
    try verifySource(old12)
    try require(try value(old12.descriptor(), "layer_count") == 2,
                "Independent native12 preserves its tagged adjustment")
    let old12Archive = try PrecisionStateArchive.load(from: files.appendingPathComponent("independent-v2-archive"))
    try verifySource(old12Archive.document)
    try require(try value(old12Archive.document.descriptor(), "layer_count") == 2,
                "Independent archive v2/native12 remains readable")
    let context = try PrecisionWorkContext(source: native)
    for malformed in [
        "{\"type\":\"adjust\",\"op\":\"invert\"}",
        "{\"type\":\"adjust\",\"evaluation\":\"future_v99\",\"op\":\"invert\"}",
        "{\"type\":\"adjust\",\"evaluation\":\"precision_v1\",\"op\":\"unknown\"}",
        "{\"type\":\"adjust\",\"evaluation\":\"precision_v1\",\"op\":\"invert\"}\0ignored"
    ] {
        try refuses("Unknown/missing typed evaluation, operation or embedded NUL refuses") {
            _ = try native.addingAdjustment(name: "Refused", description: malformed,
                workingLimit: context.limit, otherRetained: context.otherRetained)
        }
    }
    let adjustment = try PrecisionAdjustment(op: .invert)
    try refuses("Live adjustment plan validates its target kind") {
        _ = try PrecisionCapturedEdit(native, operation: .set(index: 0, adjustment: adjustment)).run()
    }
    let legacy = ImageDocument()
    try legacy.read(from: files.appendingPathComponent("source8.png"), ofType: "public.png")
    for name in ["independent-v11.rz", "independent-v12.rz", "adjusted-current.rz"] {
        try legacy.read(from: files.appendingPathComponent(name), ofType: "public.png")
        guard let typed = legacy.doc?.precision else { throw ProbeFailure(description: "Public typed-native owner missing") }
        try verifySource(typed)
        try require(legacy.projection == nil, "Public typed-native open preserves depth across all readable versions")
    }
    legacy.close()
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "Typed worker terminal paths release host reservation")
    print("PASS: independent v11/native-v1 archive, strict evaluation/target guards and public typed-native ownership")
}

func verifyPrecisionCaptureResources() throws {
    let source = try openTyped(root().appendingPathComponent("source16.png"))
    let profile = try Data(contentsOf: root().appendingPathComponent("destination-profile.icc"))
    let baseline = try RasterResources.shared.report()
    let blocker = try WorkerTask { 7 }
    var refusedCompetingPreparation = false
    do { _ = try PrecisionCapturedEdit(source, operation: .convertProfile(profile)) }
    catch { refusedCompetingPreparation = true }
    let completed = try BackgroundWork.wait(blocker, title: "Verifying Exclusive Typed Preparation")
    try require(refusedCompetingPreparation, "A pending final job prevents competing typed preparation")
    try require(completed == 7,
                "Owned worker remains consumable after competing preparation refused")
    weak var firstLease: PrecisionEditPayload?
    weak var secondLease: PrecisionEditPayload?
    try autoreleasepool {
        let first = try PrecisionCapturedEdit(source, operation: .convertProfile(profile))
        firstLease = first.retainedPayload
        let one = try RasterResources.shared.report()
        let copy = first
        let copied = try RasterResources.shared.report()
        try require(try value(one, "precision_capture_payload_bytes")
                    == value(copied, "precision_capture_payload_bytes"),
                    "Copying a captured value shares one payload lease")
        let second = try PrecisionCapturedEdit(source, operation: .convertProfile(profile))
        secondLease = second.retainedPayload
        let two = try RasterResources.shared.report()
        let increase = try value(two, "unique_retained_bytes") - value(baseline, "unique_retained_bytes")
        try require(increase == first.retainedPayload.bytes + second.retainedPayload.bytes,
                    "Independent captured Data is conservatively charged in global retained bytes")
        let context = try PrecisionWorkContext(source: source)
        try require(context.otherRetained >= increase,
                    "Core caller allowance includes outstanding capture payloads")
        withExtendedLifetime((first, copy, second)) {}
    }
    try require(firstLease == nil && secondLease == nil, "Weak registry releases discarded payload leases")
    let after = try RasterResources.shared.report()
    try require(try value(after, "precision_capture_payload_bytes") == value(baseline, "precision_capture_payload_bytes"),
                "Captured payload retention returns to baseline")
    print("PASS: copied/independent capture payload accounting and release")
}
