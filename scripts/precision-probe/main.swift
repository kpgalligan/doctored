import AppKit
import Darwin

struct ProbeFailure: Error, CustomStringConvertible { let description: String }
func require(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
    if try !condition() { throw ProbeFailure(description: message) }
}
func refuses(_ message: String, _ operation: () throws -> Void) throws {
    var refused = false
    do { try operation() } catch { refused = true }
    try require(refused, message)
}
func root() throws -> URL {
    guard let path = ProcessInfo.processInfo.environment["DR_PRECISION_PROBE_DIR"] else {
        throw ProbeFailure(description: "Missing private probe directory")
    }
    return URL(fileURLWithPath: path, isDirectory: true)
}
func openTyped(_ url: URL) throws -> RasterPrecisionDocument {
    let budget = try PrecisionWorkContext()
    return try RasterPrecisionDocument.openPreserving(url: url, workingLimit: budget.limit,
                                                       otherRetained: budget.otherRetained)
}
func value(_ info: [String: Any], _ key: String) throws -> UInt64 {
    try PrecisionDocumentDescriptor.integer(info, key)
}
/// The byte a unit layer Opacity composites at below 32 bits (`opacity_byte`):
/// `floor(255 v + 0.5 + 1e-3) / 255`, written here from the integer rule, never from the core.
func byteUnit(_ value: Double) -> Double {
    (min(max(value, 0), 1) * 255 + 0.5 + 1e-3).rounded(.down) / 255
}
func expected(_ index: Int) -> [UInt16] {
    let alpha: [UInt16] = [0, 1, 2, 255, 256, 257, 32767, 65535]
    return [UInt16((30000 + index) % 65536), UInt16((12345 + index) % 65536),
            UInt16((40000 + index) % 65536), alpha[index % alpha.count]]
}
func verifySource(_ document: RasterPrecisionDocument, at index: Int = 0,
                  width: Int = 32, height: Int = 12, allPixels: Bool = true) throws {
    guard let source = document.layerSource(index) else {
        throw ProbeFailure(description: "Missing typed raster source")
    }
    try require(source.width == width && source.height == height, "Typed source dimensions")
    let profile = try Data(contentsOf: root().appendingPathComponent("source-profile.icc"))
    try require(source.profileData == profile, "Original P3 ICC retained without working-space adoption")
    try source.withRGBA16 { samples in
        try require(samples.count == width * height * 4, "Native UInt16 element count")
        let indices = allPixels ? Array(0..<(width * height))
            : (0...256).map { $0 * (width * height - 1) / 256 }
        for i in indices {
            try require(Array(samples[(i * 4)..<(i * 4 + 4)]) == expected(i),
                        "Exact native source including low alpha/hidden RGB at pixel \(i)")
        }
    }
}

/// The app's 16-bit PNG/TIFF save over a bare typed document: the 16-bit branch of
/// `PrecisionExportCapture.write`, run on the bounded worker after the core's projection plan is
/// admitted. `projection` is the authoritative projection the caller captured with the source;
/// nil renders it here. Answers the DR_CARRIES_* bits written.
@discardableResult
func saveCapturedProjection16(_ source: RasterPrecisionDocument,
                              projection captured: RasterPrecisionImage? = nil,
                              format: PrecisionExportFormat, to url: URL) throws -> UInt32 {
    let projection: RasterPrecisionImage
    if let captured {
        projection = captured
    } else {
        let budget = try PrecisionWorkContext(source: source)
        projection = try source.render(workingLimit: budget.limit, otherRetained: budget.otherRetained)
    }
    let scratch = try value(source.projectionExportPlan(projection), "host_scratch_bytes")
    _ = try RasterResources.shared.admit(scratch: scratch)
    let task = try WorkerTask(scratchEstimate: scratch) { () throws -> UInt32 in
        let context = try PrecisionWorkContext(source: source, image: projection)
        return try source.saveProjection16(projection, to: url, format: format, embedProfile: true,
                                           stripMetadata: false, workingLimit: context.limit,
                                           otherRetained: context.otherRetained)
    }
    return try BackgroundWork.wait(task, title: "Saving Captured High-Depth Projection")
}

func verifyCapturedWrites() throws {
    let files = try root()
    let original = try openTyped(files.appendingPathComponent("source16.png"))
    try verifySource(original)
    let info = try original.info()
    let resolution = info["resolution"] as? [String: Any] ?? [:]
    try require(abs((resolution["x_ppi"] as? Double ?? 0) - 300) < 0.02,
                "Input horizontal PPI retained")
    try require(abs((resolution["y_ppi"] as? Double ?? 0) - 150) < 0.02,
                "Input vertical PPI retained")
    let capture = try PrecisionCapturedSave(original)
    var current = original
    guard let duplicate = try current.duplicateLayer(0),
          let edited = try duplicate.settingLayerOpacity(1, opacity: 0.5),
          let group = try edited.groupingRoots(opacity: 0.75) else {
        throw ProbeFailure(description: "Neutral structural edit did not produce a typed owner")
    }
    current = group
    try require(current !== capture.source, "Later state is independent of the captured revision")
    try capture.save(to: files.appendingPathComponent("captured.rz"))
    let reopened = try openTyped(files.appendingPathComponent("captured.rz"))
    try require(try value(reopened.info(), "layer_count") == 1, "Save encoded captured old state")
    try verifySource(reopened)
    let groupedSave = try PrecisionCapturedSave(current)
    try groupedSave.save(to: files.appendingPathComponent("grouped.rz"))
    let grouped = try openTyped(files.appendingPathComponent("grouped.rz"))
    try require(try value(grouped.info(), "layer_count") == 3, "Typed group and children persist")
    try verifySource(grouped, at: 0)
    try verifySource(grouped, at: 1)
    guard let passThrough = try edited.groupingRoots(opacity: 0.625, passThrough: true) else {
        throw ProbeFailure(description: "Pass-through group was not created")
    }
    let passURL = files.appendingPathComponent("pass-through.rz")
    try PrecisionCapturedSave(passThrough).save(to: passURL)
    let passReopened = try openTyped(passURL)
    let passInfo = try passThrough.info()
    let passReopenedInfo = try passReopened.info()
    let beforeLayers = try JSONSerialization.data(withJSONObject: passInfo["layers"] ?? [], options: .sortedKeys)
    let afterLayers = try JSONSerialization.data(withJSONObject: passReopenedInfo["layers"] ?? [], options: .sortedKeys)
    try require(beforeLayers == afterLayers, "Pass-through group semantics and children survive native reopen")
    let passBudget = try PrecisionWorkContext(source: passReopened)
    let passProjection = try passReopened.render(workingLimit: passBudget.limit,
                                                otherRetained: passBudget.otherRetained)
    try passProjection.withRGBA16 { output in
        for i in 0..<(32 * 12) {
            let input = expected(i)
            let alpha = Double(input[3]) / 65535
            // Independent premultiplied Normal source-over followed by group
            // backdrop/result interpolation, each opacity at its byte. Both
            // children use this RGB value.
            let childrenAlpha = alpha + alpha * byteUnit(0.5) * (1 - alpha)
            let wantedAlpha = Int((childrenAlpha * byteUnit(0.625) * 65535).rounded())
            try require(abs(Int(output[i * 4 + 3]) - wantedAlpha) <= 1,
                        "Pass-through alpha agrees with independent scalar reference")
            for channel in 0..<3 {
                let wanted = alpha == 0 ? 0 : Int(input[channel])
                try require(abs(Int(output[i * 4 + channel]) - wanted) <= 1,
                            "Pass-through RGB retains source precision")
            }
        }
    }
    for (format, filename) in [(PrecisionExportFormat.png16, "captured-projection16.png"),
                               (.tiff16, "captured-projection16.tiff")] {
        let expectedCarried = UInt32(DR_CARRIES_PROFILE | DR_CARRIES_XMP | DR_CARRIES_RESOLUTION)
        let carried = try saveCapturedProjection16(original, format: format,
                                                   to: files.appendingPathComponent(filename))
        try require(carried == expectedCarried, "ICC/PPI/XMP export reports every carried field")
    }
    try require(try original.settingLayerOpacity(0, opacity: 1) == nil, "Neutral opacity no-op")
    try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0,
                "Completed typed captured writes release the shared worker reservation")
    print("PASS: captured immutable native/PNG16/TIFF16 writes, later independent edits, group sources, P3/PPI and no-op")
}

func verifyArchiveAndRefusals() throws {
    let files = try root()
    let original = try openTyped(files.appendingPathComponent("source16.png"))
    let captured = try PrecisionStateArchive.capture(original, name: "Pinned high-depth source")
    let destination = files.appendingPathComponent("typed-archive", isDirectory: true)
    try PrecisionStateArchive.save(captured, to: destination)
    let loaded = try PrecisionStateArchive.load(from: destination)
    try require(loaded.name == "Pinned high-depth source", "Archive plain value survives")
    try verifySource(loaded.document)
    try refuses("Public editing-state archive must not accept the private proof package") {
        _ = try EditingStateArchive.read(from: destination)
    }
    let sentinel = destination.appendingPathComponent("sentinel")
    try Data("keep".utf8).write(to: sentinel)
    try refuses("Direct archive write must not remove an existing directory") {
        try PrecisionStateArchive.write(captured, to: destination)
    }
    try require(try Data(contentsOf: sentinel) == Data("keep".utf8), "Existing archive remains intact")
    let manifestURL = destination.appendingPathComponent("precision-state.json")
    let manifest = try Data(contentsOf: manifestURL)
    let invalid: [(String, Any)] = [
        ("version", 99), ("kind", "legacy"), ("storage", "rgba8"),
        ("evaluation", "linear"), ("documentFile", "../captured.rz"), ("width", 33)
    ]
    for (key, bad) in invalid {
        guard var object = try JSONSerialization.jsonObject(with: manifest) as? [String: Any] else {
            throw ProbeFailure(description: "Archive manifest is not JSON")
        }
        object[key] = bad
        try JSONSerialization.data(withJSONObject: object).write(to: manifestURL)
        try refuses("Archive rejects invalid \(key)") { _ = try PrecisionStateArchive.read(from: destination) }
        try manifest.write(to: manifestURL)
    }
    let fake = files.appendingPathComponent("fake-archive", isDirectory: true)
    try FileManager.default.copyItem(at: destination, to: fake)
    let fakeNative = fake.appendingPathComponent("image.rz")
    try FileManager.default.removeItem(at: fakeNative)
    try FileManager.default.copyItem(at: files.appendingPathComponent("source16.png"), to: fakeNative)
    try refuses("A PNG renamed image.rz is not a native archive source") {
        _ = try PrecisionStateArchive.read(from: fake)
    }
    try refuses("Typed import must reject byte input without fallback") {
        _ = try openTyped(files.appendingPathComponent("source8.png"))
    }
    let misleading = files.appendingPathComponent("typed-native.png")
    try FileManager.default.copyItem(at: files.appendingPathComponent("captured.rz"), to: misleading)
    try require(ImageDocument.hasNativeSignature(at: misleading), "Native guard uses content, not extension")
    let legacy = ImageDocument()
    try legacy.read(from: files.appendingPathComponent("source8.png"), ofType: "public.png")
    let before = legacy.doc
    let revision = legacy.revision
    try legacy.read(from: misleading, ofType: "public.png")
    guard let typed = legacy.doc?.precision else { throw ProbeFailure(description: "Native signature did not select typed owner") }
    try verifySource(typed)
    try require(legacy.doc !== before && legacy.revision > revision && legacy.projection == nil,
                "Native signature selects exact typed state and never a byte master")
    legacy.close()
    print("PASS: separate typed archive, strict manifest/native refusal, existing-package preservation, and public typed-native dispatch")
}

func verifyTypedResources() throws {
    let files = try root()
    let baseline = try RasterResources.shared.report()
    weak var releasedDocument: RasterPrecisionDocument?
    weak var releasedImage: RasterPrecisionImage?
    try autoreleasepool {
        let source = try openTyped(files.appendingPathComponent("source16.png"))
        releasedDocument = source
        guard let pixels = source.layerSource(0), let duplicate = try source.duplicateLayer(0) else {
            throw ProbeFailure(description: "Typed resource fixture")
        }
        releasedImage = pixels
        let one = try RasterPrecisionDocument.resourceReport(documents: [source], images: [])
        let aliases = try RasterPrecisionDocument.resourceReport(documents: [source, source], images: [pixels])
        try require(try value(one, "unique_retained_bytes") == value(aliases, "unique_retained_bytes"),
                    "Source aliases and borrowed-image owners share one native payload")
        let union = try RasterPrecisionDocument.resourceReport(documents: [source, duplicate], images: [pixels])
        try require(try value(union, "unique_retained_bytes") - value(one, "unique_retained_bytes") < 32 * 12 * 8,
                    "Duplicating a layer shares its typed pixel allocation")
        let global = try RasterResources.shared.report()
        try require(try value(global, "unique_retained_bytes") >= value(union, "unique_retained_bytes"),
                    "Global admission includes typed captures")
        let context = try PrecisionWorkContext(image: pixels)
        let preview = try pixels.previewRGBA8(workingLimit: context.limit, otherRetained: context.otherRetained)
        try require(preview.pixelRGBA(x: 1, y: 0)?.a == 0, "Explicit byte preview may quantize tiny alpha")
        try verifySource(source)
        try refuses("Captured native budget rejects insufficient available memory") {
            try source.saveNative(to: files.appendingPathComponent("refused.rz"),
                                  workingLimit: 1, otherRetained: 0)
        }
        try require(!FileManager.default.fileExists(atPath: files.appendingPathComponent("refused.rz").path),
                    "Refused save publishes no file")
    }
    try require(releasedDocument == nil && releasedImage == nil, "Weak registry releases unused typed owners")
    let final = try RasterResources.shared.report()
    try require(try value(final, "host_precision_document_handles") == value(baseline, "host_precision_document_handles")
        && value(final, "host_precision_image_handles") == value(baseline, "host_precision_image_handles"),
        "All typed handles leave the live registry after capture scope")
    print("PASS: typed allocation deduplication, weak owner release, explicit lossy preview and save admission")
}

let app = NSApplication.shared
AutomationAlerts.installIfEnabled()
app.setActivationPolicy(.accessory)
app.finishLaunching()
if ProcessInfo.processInfo.environment["DR_PRECISION_LARGE_PUBLIC"] == "1" {
    do { try verifyLargePublicWorkflow(); exit(0) }
    catch { print("FAIL: large public report: \(error)"); exit(1) }
}
if ProcessInfo.processInfo.environment["DR_PRECISION_LARGE_PIXELS"] == "1" {
    do { try verifyLargePixelWorkflow(); exit(0) }
    catch { print("FAIL: large pixel report: \(error)"); exit(1) }
}
if ProcessInfo.processInfo.environment["DR_PRECISION_LARGE_COLOR"] == "1" {
    do { try verifyLargeColorWorkflow(); exit(0) }
    catch { print("FAIL: large color report: \(error)"); exit(1) }
}
if ProcessInfo.processInfo.environment["DR_PRECISION_LARGE"] == "1" {
    do { try verifyLargeWorkflow(); exit(0) }
    catch { print("FAIL: large workflow report: \(error)"); exit(1) }
}
var failures = 0
for (name, test) in [("captured writes", verifyCapturedWrites), ("archive and guards", verifyArchiveAndRefusals),
                     ("typed resources", verifyTypedResources), ("typed tone", verifyPrecisionTone),
                     ("typed profiles", verifyPrecisionProfiles), ("tone persistence", verifyPrecisionTonePersistence),
                     ("typed compatibility", verifyPrecisionCompatibility),
                     ("capture resources", verifyPrecisionCaptureResources),
                     ("scalar storage", verifyPrecisionScalarStorage),
                     ("typed geometry", verifyPrecisionGeometry),
                     ("typed paint tools", verifyPrecisionPaintTools),
                     ("typed healing", verifyPrecisionHealing),
                     ("typed artwork", verifyPrecisionArtwork),
                     ("public typed ownership", verifyPublicPrecisionOwnership),
                     ("public typed history/conversion", verifyPublicPrecisionHistory),
                     ("public typed archive/recovery", verifyPublicPrecisionArchive),
                     ("public typed stale/save", verifyPublicPrecisionStaleSave),
                     ("public mixed resources", verifyPublicPrecisionResources)] {
    do { try autoreleasepool { try test() } }
    catch { failures += 1; print("FAIL: \(name): \(error)") }
    fflush(stdout)
}
exit(failures == 0 ? 0 : 1)
