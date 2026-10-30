import AppKit
import CryptoKit

struct LocalProbeFailure: Error, CustomStringConvertible { let description: String }
func localCheck(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
    guard try condition() else { throw LocalProbeFailure(description: message) }
}
func localRefuses(_ name: String, _ body: () throws -> Void) throws {
    do { try body() } catch { return }
    throw LocalProbeFailure(description: "Expected refusal: " + name)
}
func localHash(_ data: Data) -> String { SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() }
func localWrite(_ object: Any, _ path: URL) throws {
    try JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys]).write(to: path)
}
func localProgress(_ value: String) { FileHandle.standardOutput.write(Data((value + "\n").utf8)) }
func localPixels(_ source: RasterDocument, retained: Bool = true) throws -> Data {
    let image: RasterPrecisionImage
    if retained { image = try source.sourceMaster16(0) }
    else {
        guard let value = source.precision?.layerSource(0) else { throw LocalProbeFailure(description: "Missing true16 source") }
        image = value
    }
    return try image.withRGBA16 { Data(UnsafeRawBufferPointer($0)) }
}
func localOpen(_ url: URL, depth: Int, settings: RawDevelopSettings = .init()) throws -> RasterDocument {
    let snapshot = try RawSourceSnapshot.capture(url, maximumBytes: 256 * 1024 * 1024)
    let captured = try RetainedRawDecoder.capture(url: snapshot.url, filename: url.lastPathComponent, sha256: snapshot.sha256)
    var descriptor = captured
    descriptor.recipe = try captured.recipe.updating(settings)
    let desired = descriptor
    return try BackgroundWork.wait(try WorkerTask {
        let decoded = try RetainedRawDecoder.develop(url: snapshot.url, descriptor: desired,
            whiteBalanceChanged: desired.recipe.values.customWhiteBalance)
        return try RawSourceWork.initialDocument(asset: decoded.0, descriptor: decoded.1,
            bytes: snapshot.bytes, highDepth: depth == 16)
    }, title: "Opening independent RAW fixture")
}
func localEvaluate(_ source: RasterDocument, settings: RawDevelopSettings = .init(),
                   operations: [[String: Any]] = [], cache: RawLocalVariantCache) throws -> RasterDocument? {
    let request = try RawLocalWork.request(source: source, layer: 0, settings: settings, operations: operations, cache: cache)
    let control = try RasterJobControl()
    try localCheck(control.setScratchLimit(request.scratchEstimate), "Configure exact shared RAW pipeline budget")
    return try BackgroundWork.wait(try WorkerTask(scratchEstimate: request.scratchEstimate) {
        try request.evaluate(control: control)
    }, title: "Qualifying original-derived RAW locals", control: control)
}
func localDocument(_ source: RasterDocument) -> ImageDocument {
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: source.isHighDepth ? 16 : 8, exportDither: "none"), markSaved: true)
    NSDocumentController.shared.addDocument(document)
    return document
}

func modelProbe(source: RasterDocument, directory: URL) throws -> [String: Any] {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    var groups: [String] = []
    let cache = RawLocalVariantCache()
    defer { cache.clear() }
    let original = try RawLocalWork.state(source, layer: 0)
    try localCheck(original.nodes.isEmpty && original.descriptor.recipe.version == 1, "Old recipe1 remains exact on normal import")
    try localCheck(try RawLocalWork.prepare(source: source, layer: 0) == nil, "Empty local work is a semantic no-op")
    var neutral = RawDevelopSettings(); neutral.baselineCompensation = 0; neutral.moire = 0; neutral.vignette = .neutral
    try localCheck(try RawSourceWork.prepare(source: source, layer: 0, settings: neutral) == nil, "Explicit neutral enhancements preserve old recipe identity")
    groups.append("recipe1_and_explicit_neutral_enhancements_remain_exact_noops")

    let command: [String: Any] = ["op": "create", "id": "native-local-a", "name": "Local A",
        "exposure_offset": -0.625, "white_balance": "custom", "temperature": 6200.0, "tint": 11.999950408935547,
        "mask": ["op": "radial", "center": [172.125, 238.625], "radius": [147.25, 171.5], "hardness": 0.27125]]
    guard let first = try localEvaluate(source, operations: [command], cache: cache) else { throw LocalProbeFailure(description: "Create local did not publish a candidate") }
    let beforePaint = cache.diagnostics
    let paint: [String: Any] = ["op": "mask", "id": "native-local-a", "command": ["op": "brush", "mode": "add",
        "dabs": [["center": [74.25, 80.75], "radius": 19.5, "hardness": 0.35, "opacity": 0.3125]]]]
    guard let painted = try localEvaluate(first, operations: [paint], cache: cache) else { throw LocalProbeFailure(description: "Native brush did not change coverage") }
    try localCheck(cache.diagnostics["misses"] == beforePaint["misses"], "Coverage-only edit after customWB reuses canonical native variants")
    try localCheck(cache.diagnostics["entries"] == 2 && (cache.diagnostics["aliases"] ?? 0) <= 4,
        "Requested/canonical cache aliases never double-count source images")
    let afterPaint = try RawLocalWork.state(painted, layer: 0)
    try localCheck(try afterPaint.masks[0].sha256() != RawLocalWork.state(first, layer: 0).masks[0].sha256(), "Native brush changes real coverage")
    try localCheck(try RawLocalWork.prepare(source: painted, layer: 0,
        operations: [["op": "update", "id": "native-local-a", "density": 1.0]]) == nil, "Repeated local controls add no edit")
    try localCheck(painted.sourceAttachmentKind(0) == .rawLocalMasks && painted.hasRetainedRawSource(0), "RAW and native masks coexist")
    let stackFields = RetainedSourceInspection.fields(painted, layer: 0)
    let rawSummary = (stackFields["image_source"] as? [String: Any])?["raw"] as? [String: Any]
    try localCheck(stackFields["has_raw_source"] as? Bool == true && rawSummary != nil && rawSummary?["recipe"] == nil,
        "Both depths expose a compact RAW stack summary without dense recipes")
    groups.append("native_radial_brush_custom_white_balance_and_canonical_variant_cache")

    for invalid: [[String: Any]] in [
        [["op": "remove", "id": "missing"]], [["op": "update", "id": "native-local-a", "density": true]],
        [["op": "reorder", "id": "native-local-a", "index": 0.5]],
        [["op": "mask", "id": "native-local-a", "command": ["op": "invert", "unknown": 1]]],
        [["op": "update", "id": "native-local-a", "white_balance": "custom", "temperature": NSNull()]],
        [["op": "create", "id": "native-local-a", "mask": ["op": "empty"]]]
    ] { try localRefuses("Strict local request: \(invalid)") { _ = try RawLocalWork.prepare(source: painted, layer: 0, operations: invalid) } }
    let cancelled = try RasterJobControl(); cancelled.cancel()
    let request = try RawLocalWork.request(source: painted, layer: 0, operations: [paint])
    try localRefuses("Precancelled local request") { _ = try request.evaluate(control: cancelled) }
    groups.append("strict_local_requests_and_precancel_refuse_without_replacing_source")

    let native = directory.appendingPathComponent("local.rz")
    try painted.saveNative(to: native)
    let reopened = try RasterDocument.open(url: native)
    let restored = try RawLocalWork.state(reopened, layer: 0)
    try localCheck(restored.descriptor == afterPaint.descriptor, "Dense local/RAW binary64 descriptor exact after reopen")
    try localCheck(try localPixels(reopened) == localPixels(painted), "Saved native rendering exact")
    try localCheck(try restored.masks[0].encodedBytes() == afterPaint.masks[0].encodedBytes(), "Native coverage bytes exact")
    groups.append("native_original_recipe_mask_and_master_roundtrip")

    var exposure = RawDevelopSettings(); exposure.exposure = 0.375
    guard let changed = try localEvaluate(reopened, settings: exposure, cache: cache) else { throw LocalProbeFailure(description: "Global development did not update locals") }
    exposure.exposure = afterPaint.descriptor.recipe.values.exposure
    guard let returned = try localEvaluate(changed, settings: exposure, cache: cache) else { throw LocalProbeFailure(description: "Global exposure did not return") }
    try localCheck(try localPixels(returned) == localPixels(reopened), "Global exposure roundtrip restores every local/master code")
    try localCheck(try RawLocalWork.state(returned, layer: 0).masks[0].sha256() == restored.masks[0].sha256(), "Global updates preserve exact native coverage")
    groups.append("global_edit_redevelops_locals_and_exact_exposure_roundtrip")

    let document = localDocument(reopened)
    defer { document.close() }
    let before = document.doc, revision = document.revision
    let available = try RasterResources.shared.admit(scratch: 0)
    var lease: PrecisionEditPayload? = try PrecisionEditPayload.admitted(bytes: available - 1)
    try withExtendedLifetime(lease) {
        try localCheck(reopened.hasRetainedRawSource(0) && reopened.sourceAttachmentKind(0) == .rawLocalMasks,
            "Cheap source/kind survive metadata refusal")
        let denied = RetainedSourceInspection.fields(reopened, layer: 0)
        try localCheck(denied["has_raw_source"] as? Bool == true && denied["source_inspection_error"] != nil
            && denied["image_source"] == nil, "Denied stack metadata preserves truthful cheap RAW presence")
        try localRefuses("Denied local edit") { _ = try RawLocalWork.prepare(source: reopened, layer: 0, operations: [paint]) }
    }
    lease = nil
    try localCheck(document.doc === before && document.revision == revision && !document.isDocumentEdited,
        "Denied work preserves exact owner, revision and dirty state")
    groups.append("admission_and_cheap_kind_routing_integrity")

    let maskHash = try restored.masks[0].sha256()
    var forged = restored.descriptor
    forged = RawSourceDescriptor(schema_version: forged.schema_version, original_filename: forged.original_filename,
        content_sha256: forged.content_sha256,
        decoder: .init(identifier: forged.decoder.identifier, version: forged.decoder.version, implementation: "unavailable-probe-implementation"),
        recipe: forged.recipe)
    let unavailable = try RawSourceWork.replacing(source: reopened, layer: 0,
        asset: reopened.rawLocalBaseSource(layer: 0), descriptor: forged)
    let migrationOwner = localDocument(unavailable)
    defer { migrationOwner.close() }
    let candidate = try RawDecoderMigration.prepare(in: migrationOwner, layer: 0)
    let reviewed = try localPixels(candidate.asset, retained: false)
    try candidate.apply(in: migrationOwner)
    guard let migrated = migrationOwner.doc else { throw LocalProbeFailure(description: "Migration lost owner") }
    try localCheck(try localPixels(migrated) == reviewed, "Migration publishes exact reviewed complete local source")
    try localCheck(try RawLocalWork.state(migrated, layer: 0).masks[0].sha256() == maskHash, "Migration retains exact native mask")
    migrationOwner.undoManager?.undo()
    try localCheck(migrationOwner.doc === unavailable, "Migration one Undo restores original source owner")
    groups.append("reviewed_same_geometry_decoder_migration_keeps_exact_local_coverage_and_undo")

    let snapshot = cache.snapshot()
    guard let entry = snapshot.first else { throw LocalProbeFailure(description: "Missing cache fixture") }
    let ticket = cache.ticket(); cache.clear()
    try cache.store(entry.0, descriptor: entry.1, key: entry.1.json(), ticket: ticket)
    try localCheck(cache.diagnostics["entries"] == 0, "Late worker cache store cannot repopulate after close/clear")
    groups.append("cache_clear_generation_blocks_late_store")
    return ["passed": true, "group_count": groups.count, "groups": groups, "native": native.path]
}

func cameraProbe(url: URL, depth: Int, space: RawOutputSpace, directory: URL) throws -> [String: Any] {
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let start = Date()
    var calibration: [[String: Any]] = []
    if depth == 16 {
        for enhanced in [false, true] {
            var settings = RawDevelopSettings(); settings.outputSpace = space
            if enhanced { settings.exposure = 0.35; settings.baselineCompensation = 0.5; settings.moire = 1 }
            let document = try localOpen(url, depth: depth, settings: settings)
            let name = enhanced ? "enhanced" : "initial"
            let path = directory.appendingPathComponent(name + ".rgba16le")
            try localPixels(document).write(to: path)
            let descriptor = try RawLocalWork.state(document, layer: 0).descriptor
            calibration.append(["name": name + "-" + space.rawValue, "values": path.path,
                "descriptor": try JSONSerialization.jsonObject(with: Data(descriptor.json().utf8))])
        }
    }
    var initial = RawDevelopSettings(); initial.outputSpace = space; initial.exposure = 4
    let source = try localOpen(url, depth: depth, settings: initial)
    let cache = RawLocalVariantCache()
    defer { cache.clear() }
    let operations: [[String: Any]] = [
        ["op": "create", "id": "recovered", "name": "Recovered", "exposure_offset": -4.0,
         "mask": ["op": "radial", "center": [1504.0, 1000.0], "radius": [1400.0, 950.0], "hardness": 0.4]],
        ["op": "create", "id": "warm", "name": "Warm", "exposure_offset": -3.0,
         "white_balance": "custom", "temperature": 7000.0, "tint": 17.0, "density": 0.35,
         "mask": ["op": "linear", "from": [200.0, 150.0], "to": [2800.0, 1850.0]]],
        ["op": "create", "id": "cool", "name": "Cool", "exposure_offset": -2.0,
         "white_balance": "custom", "temperature": 5000.0, "tint": -15.0, "density": 0.6,
         "mask": ["op": "radial", "center": [1700.0, 700.0], "radius": [650.0, 500.0], "hardness": 0.0]],
        ["op": "create", "id": "neutral", "name": "Neutral", "exposure_offset": -1.0, "white_balance": "as_shot",
         "mask": ["op": "empty"]],
        ["op": "mask", "id": "neutral", "command": ["op": "brush", "mode": "add", "dabs": [
            ["center": [800.0, 1300.0], "radius": 250.0, "hardness": 0.5, "opacity": 0.72],
            ["center": [1000.0, 1350.0], "radius": 300.0, "hardness": 0.3, "opacity": 0.51]]]]]
    guard let mixed = try localEvaluate(source, operations: operations, cache: cache) else { throw LocalProbeFailure(description: "Four camera locals did not render") }
    let mixedPath = directory.appendingPathComponent("mixed.rz")
    try mixed.saveNative(to: mixedPath)
    let mixedValues = directory.appendingPathComponent("mixed.rgba16le")
    try localPixels(mixed).write(to: mixedValues)
    let state = try RawLocalWork.state(mixed, layer: 0)
    try localCheck(state.nodes.count == 4 && cache.diagnostics["entries"] == 5,
        "Four full-size camera locals use five unique admitted variants")
    var inputs: [[String: Any]] = []
    for (index, entry) in cache.snapshot().enumerated() {
        let values = directory.appendingPathComponent("input-\(index).rgba16le")
        let profile = directory.appendingPathComponent("input-\(index).icc")
        try localPixels(entry.0, retained: false).write(to: values)
        guard let icc = entry.0.iccProfile else { throw LocalProbeFailure(description: "Camera variant lost ICC") }
        try icc.write(to: profile)
        inputs.append(["values": values.path, "profile": profile.path,
            "descriptor": try JSONSerialization.jsonObject(with: Data(entry.1.json().utf8))])
    }
    var maskRecords: [[String: Any]] = []
    for (node, mask) in zip(state.nodes, state.masks) {
        let path = directory.appendingPathComponent(node.id + ".drmask")
        try mask.encodedBytes().write(to: path)
        maskRecords.append(["id": node.id, "path": path.path, "density": node.density,
            "feather": node.feather, "enabled": node.enabled])
    }
    var vignette = RawDevelopSettings()
    vignette.vignette = RawVignetteSettings(edgeExposure: 0.5, centerX: 0.45, centerY: 0.55, falloff: 2)
    let beforeVignette = cache.diagnostics
    guard let final = try localEvaluate(mixed, settings: vignette, cache: cache) else { throw LocalProbeFailure(description: "Manual vignette did not update source") }
    try localCheck(cache.diagnostics["misses"] == beforeVignette["misses"], "Manual vignette reuses every camera variant")
    let finalPath = directory.appendingPathComponent("vignette.rz")
    try final.saveNative(to: finalPath)
    let finalValues = directory.appendingPathComponent("vignette.rgba16le")
    try localPixels(final).write(to: finalValues)
    var optics = RawDevelopSettings()
    optics.sourceGeometry = RawLensGeometry(profileID: "pinned-nikon-d70s-explicit-v1",
        center: [0.43, 0.57], radial: [0.012, -0.001, 0.0001],
        redScale: 0.985, blueScale: 1.015, filter: .bilinear,
        referenceSize: nil, referenceFromSource: nil)
    guard let optical = try localEvaluate(final, settings: optics, cache: cache) else {
        throw LocalProbeFailure(description: "Genuine Nikon explicit radial/TCA profile did not develop")
    }
    let opticalValues = directory.appendingPathComponent("optical.rgba16le")
    try localPixels(optical).write(to: opticalValues)
    try localCheck((try RawLocalWork.state(optical, layer: 0)).descriptor.recipe.sourceGeometry
        == optics.sourceGeometry, "Genuine Nikon exact optical profile is persisted")
    let reopened = try RasterDocument.open(url: finalPath)
    try localCheck(try localPixels(reopened) == localPixels(final), "Full camera native reopened master exact")
    let restored = try RawLocalWork.state(reopened, layer: 0)
    try localCheck(try restored.descriptor == RawLocalWork.state(final, layer: 0).descriptor,
        "Full camera native dense recipe exact")
    for (a, b) in zip(restored.masks, state.masks) { try localCheck(try a.sha256() == b.sha256(), "Full camera masks exact") }
    let migration = depth == 16 && space == .sRGB
        ? try runRawLocalMigrationProbe(source: final, directory: directory) : [:]
    return ["passed": true, "group_count": 4 + (migration["group_count"] as? Int ?? 0), "migration": migration, "depth": depth, "space": space.rawValue,
        "width": state.width, "height": state.height, "inputs": inputs, "masks": maskRecords, "calibration": calibration,
        "operations": operations, "mixed_values": mixedValues.path, "mixed_native": mixedPath.path,
        "vignette_values": finalValues.path, "vignette_native": finalPath.path,
        "optical_values": opticalValues.path, "optical_profile": optics.sourceGeometry?.jsonObject ?? [:],
        "vignette": vignette.vignette?.json ?? [:], "elapsed_seconds": Date().timeIntervalSince(start)]
}

_ = NSApplication.shared
AutomationAlerts.installIfEnabled()
NSApp.setActivationPolicy(.prohibited)
do {
    guard let input = ProcessInfo.processInfo.environment["DR_RAW_LOCAL_FIXTURES"],
          let output = ProcessInfo.processInfo.environment["DR_RAW_LOCAL_RESULTS"] else { throw LocalProbeFailure(description: "Missing isolated probe paths") }
    let fixtures = URL(fileURLWithPath: input), directory = URL(fileURLWithPath: output)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    var models: [[String: Any]] = [], ui: [[String: Any]] = [], cameras: [[String: Any]] = []
    var geometry: [[String: Any]] = []
    var failures: [[String: String]] = []
    func attempt(_ name: String, _ body: () throws -> [String: Any]) -> [String: Any]? {
        localProgress("START " + name)
        do {
            let result = try autoreleasepool(invoking: body)
            localProgress("PASS " + name + ": \(result["group_count"] ?? 0) groups")
            return result
        } catch {
            failures.append(["stage": name, "error": String(describing: error)])
            localProgress("FAIL " + name + ": " + String(describing: error))
            try? localWrite(["failures": failures], directory.appendingPathComponent("partial-failures.json"))
            return nil
        }
    }
    for depth in [8, 16] {
        if let value = attempt("geometry-\(depth)", {
            try runRawGeometryProbe(fixtures: fixtures, depth: depth,
                directory: directory.appendingPathComponent("geometry-\(depth)"))
        }) { geometry.append(value) }
        if let value = attempt("model-\(depth)", {
            let source = try localOpen(fixtures.appendingPathComponent("source.dng"), depth: depth)
            return try modelProbe(source: source, directory: directory.appendingPathComponent("model-\(depth)"))
        }) { models.append(value) }
        if let value = attempt("ui-\(depth)", {
            let source = try localOpen(fixtures.appendingPathComponent("source.dng"), depth: depth)
            return try runRawLocalUIProbe(source: source, layer: 0, directory: directory.appendingPathComponent("ui-\(depth)"))
        }) { ui.append(value) }
    }
    for depth in [8, 16] {
        // The two spaces `reference.swift` and `fixtures.py` qualify.
        for space in [RawOutputSpace.sRGB, .displayP3] {
            if let value = attempt("camera-\(depth)-\(space.rawValue)", {
                try cameraProbe(url: fixtures.appendingPathComponent("camera.NEF"), depth: depth,
                    space: space, directory: directory.appendingPathComponent("camera-\(depth)-\(space.rawValue)"))
            }) { cameras.append(value) }
        }
    }
    let navigation = attempt("camera-navigation", {
        let source = try localOpen(fixtures.appendingPathComponent("camera.NEF"), depth: 16)
        return try runRawLocalNavigationUIProbe(source: source, layer: 0, directory: directory.appendingPathComponent("camera-navigation"))
    }) ?? [:]
    let report: [String: Any] = ["passed": failures.isEmpty, "models": models, "ui": ui, "cameras": cameras,
        "geometry": geometry,
        "navigation": navigation, "failures": failures,
        "group_count": (models + ui + cameras + geometry + [navigation]).reduce(0) { $0 + ($1["group_count"] as? Int ?? 0) }]
    try localWrite(report, directory.appendingPathComponent("host-report.json"))
    localProgress("RAW local host groups: \(report["group_count"] ?? 0), failed independent cases: \(failures.count)")
    if !failures.isEmpty { exit(1) }

} catch {
    FileHandle.standardError.write(Data("RAW local qualification failed: \(error)\n".utf8))
    exit(1)
}
