import AppKit
import ImageIO
import ObjectiveC.runtime
import UniformTypeIdentifiers

private func psdBE(_ value: UInt32) -> Data {
    var value = value.bigEndian
    return withUnsafeBytes(of: &value) { Data($0) }
}
private func psdShort(_ value: UInt16) -> Data {
    var value = value.bigEndian
    return withUnsafeBytes(of: &value) { Data($0) }
}

private func psdResource(_ id: UInt16, _ body: Data) -> Data {
    var resource = Data("8BIM".utf8)
    resource.append(psdShort(id)); resource.append(contentsOf: [0, 0])
    resource.append(psdBE(UInt32(body.count))); resource.append(body)
    if body.count % 2 != 0 { resource.append(0) }
    return resource
}

/// A 1x1 PSD with one gray raster layer and a deliberately different saved
/// RGB composite. Resources and layer bytes are authored independently of the
/// product's PSD reader; the profile comes from CoreGraphics, not Doctored.
/// A mask channel without a descriptor proves the report names verified losses.
private func psdFixture(profile: Data, depth: UInt16 = 8, color: UInt16 = 3) -> Data {
    var record = Data()
    for value: UInt32 in [0, 0, 1, 1] { record.append(psdBE(value)) }
    let channels: [Int16] = [0, 1, 2, -1, -2]
    record.append(psdShort(UInt16(channels.count)))
    for channel in channels { record.append(psdShort(UInt16(bitPattern: channel))); record.append(psdBE(3)) }
    record.append(Data("8BIMnorm".utf8)); record.append(contentsOf: [255, 0, 8, 0])
    let extra = Data([0,0,0,0, 0,0,0,0, 3,65,114,116])
    record.append(psdBE(UInt32(extra.count))); record.append(extra)
    for channel in channels { record.append(contentsOf: [0, 0, channel == -1 ? 255 : 200]) }
    var info = psdShort(1); info.append(record)
    if info.count % 2 != 0 { info.append(0) }
    var layers = psdBE(UInt32(info.count)); layers.append(info); layers.append(psdBE(0))
    var file = Data("8BPS".utf8); file.append(psdShort(1)); file.append(Data(repeating: 0, count: 6))
    file.append(psdShort(3)); file.append(psdBE(1)); file.append(psdBE(1))
    file.append(psdShort(depth)); file.append(psdShort(color))
    var resources = psdResource(1039, profile)
    var resolution = Data()
    for ppi: UInt32 in [300, 240] {
        resolution.append(psdBE(ppi << 16)); resolution.append(contentsOf: [0, 1, 0, 1])
    }
    resources.append(psdResource(1005, resolution))
    file.append(psdBE(0)); file.append(psdBE(UInt32(resources.count))); file.append(resources)
    file.append(psdBE(UInt32(layers.count)))
    file.append(layers); file.append(contentsOf: [0,0,10,20,30])
    return file
}

private func psdTool(_ tool: String, _ arguments: [String: Any]) throws -> (Bool, [String: Any]) {
    let bytes = try JSONSerialization.data(withJSONObject: arguments)
    let reply = AgentServer.shared.execute(tool: tool, argumentsJSON: String(decoding: bytes, as: UTF8.self))
    guard let envelope = try JSONSerialization.jsonObject(with: Data(reply.utf8)) as? [String: Any],
          let content = envelope["content"] as? [[String: Any]],
          let text = content.first?["text"] as? String else {
        throw ProbeFailure(description: "invalid PSD MCP reply")
    }
    let error = envelope["isError"] as? Bool ?? false
    let result = (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any]
    return (error, result ?? ["text": text])
}

private func psdMetadata(_ document: RasterDocument, profile: Data, name: String) throws {
    try require(document.iccProfile == profile, "\(name): exact PSD ICC bytes")
    try require(document.resolution.x == 300 && document.resolution.y == 240,
                "\(name): distinct PSD X/Y print resolution")
}

private func psdNativeRoundtrip(_ document: RasterDocument, profile: Data, name: String) throws {
    try psdMetadata(document, profile: profile, name: name)
    let native = probeDirectory.appendingPathComponent("\(name).rz")
    try document.saveNative(to: native)
    let reopened = ImageDocument()
    defer { reopened.close() }
    try reopened.read(from: native, ofType: ImageDocument.nativeTypeIdentifier)
    guard let restored = reopened.doc else { throw ProbeFailure(description: "\(name): native reopen") }
    try psdMetadata(restored, profile: profile, name: "\(name) reopened")
    let second = probeDirectory.appendingPathComponent("\(name).reopened.rz")
    try restored.saveNative(to: second)
    let originalBytes = try Data(contentsOf: native)
    let reopenedBytes = try Data(contentsOf: second)
    try require(originalBytes == reopenedBytes, "\(name): exact native round trip")
}

private func psdFlatAdoptionControl(profile: Data, psdURL: URL, fingerprint: String) throws {
    let input = Data([197, 100, 43, 255])
    guard let space = CGColorSpace(iccData: profile as CFData),
          let provider = CGDataProvider(data: input as CFData),
          let image = CGImage(width: 1, height: 1, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: 4, space: space,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent) else {
        throw ProbeFailure(description: "Independent flat P3 control")
    }
    let url = probeDirectory.appendingPathComponent("psd-flat-control.png")
    guard let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil) else {
        throw ProbeFailure(description: "Flat PNG control writer")
    }
    CGImageDestinationAddImage(destination, image, nil)
    try require(CGImageDestinationFinalize(destination), "Flat PNG control finalize")
    let working = ColorSettings.workingSpace.profileData
    try working.write(to: probeDirectory.appendingPathComponent("psd-flat-working-profile.icc"))
    let control = ImageDocument()
    defer { control.close() }
    try control.read(from: url, ofType: UTType.png.identifier)
    try require(control.profileAdoption == .converted && control.psdImport == nil,
                "Ordinary flat P3 image still adopts the sRGB working space")
    guard let flat = control.doc, let pixel = flat.layerImage(0)?.sample(x: 0, y: 0, reach: 0) else {
        throw ProbeFailure(description: "Flat adoption control pixels")
    }
    try require(flat.iccProfile == working && flat.iccProfile != profile,
                "Flat adoption retains the converted working profile")
    try require([pixel.r, pixel.g, pixel.b] != [197, 100, 43] && pixel.a == 255,
                "Flat adoption must convert color samples and retain alpha")
    try flat.saveNative(to: probeDirectory.appendingPathComponent("psd-flat-control.rz"))

    // Reuse a document whose previous open was converted, exercising the read
    // path also used by Revert: its old diagnostic must not survive a PSD read.
    let previous = PSDImportRequest.mode
    defer { PSDImportRequest.mode = previous }
    PSDImportRequest.mode = .explicit(.layers, fingerprint: fingerprint)
    try control.read(from: psdURL, ofType: "com.adobe.photoshop-image")
    try require(control.profileAdoption == .unchanged, "PSD read cleared stale profile-adoption status")
    guard let restored = control.doc else { throw ProbeFailure(description: "Reused PSD control") }
    try psdNativeRoundtrip(restored, profile: profile, name: "psd-reused-layers")
    print("PASS PSD working-space policy: exact PSD source codes; ordinary flat P3 conversion retained; stale adoption status cleared")
}

func verifyPSDImports() throws {
    try require(ColorSettings.workingSpace == .sRGB, "PSD probe requires the distinct sRGB working space")
    let url = probeDirectory.appendingPathComponent("compatibility.psd")
    guard let space = CGColorSpace(name: CGColorSpace.displayP3),
          let profile = space.copyICCData() as Data?, !profile.isEmpty else {
        throw ProbeFailure(description: "Independent Display P3 ICC fixture")
    }
    try profile.write(to: probeDirectory.appendingPathComponent("psd-source-profile.icc"))
    let bytes = psdFixture(profile: profile)
    try bytes.write(to: url)
    try bytes.write(to: probeDirectory.appendingPathComponent("psd-profile-source.psd"))
    let report = try RasterDocument.psdImportReport(at: url)
    try require(report["layered_available"] as? Bool == true, "PSD layers available")
    try require(report["stored_composite_available"] as? Bool == true, "PSD saved composite available")
    let losses = report["verified_losses"] as? [String] ?? []
    try require(losses.contains { $0.contains("raster mask") }, "PSD mask detection")
    try require(!losses.contains { $0.contains("styles") }, "PSD general limits are not invented per-file losses")
    let resolution = report["resolution"] as? [String: Any]
    try require((resolution?["x"] as? NSNumber)?.doubleValue == 300
        && (resolution?["y"] as? NSNumber)?.doubleValue == 240, "PSD report print resolution")
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
        .write(to: probeDirectory.appendingPathComponent("psd-import-report.json"))
    guard let fingerprint = report["source_fingerprint"] as? String,
          let method = class_getInstanceMethod(NSAlert.self, #selector(NSAlert.runModal)) else {
        throw ProbeFailure(description: "PSD report or alert interception unavailable")
    }
    let previousMode = PSDImportRequest.mode
    defer { PSDImportRequest.mode = previousMode }
    var alertCount = 0
    var selectedTitle = "Cancel"
    var observedReport = ""
    var observedUnopened = true
    var observedDocument: ImageDocument?
    let interceptor: @convention(block) (AnyObject) -> Int = { object in
        alertCount += 1
        if let document = observedDocument { observedUnopened = observedUnopened && document.doc == nil }
        guard let alert = object as? NSAlert else { return NSApplication.ModalResponse.abort.rawValue }
        if let scroll = alert.accessoryView as? NSScrollView,
           let text = scroll.documentView as? NSTextView { observedReport = text.string }
        let index = alert.buttons.firstIndex { $0.title == selectedTitle } ?? alert.buttons.count - 1
        return NSApplication.ModalResponse.alertFirstButtonReturn.rawValue + index
    }
    let replacement = imp_implementationWithBlock(interceptor)
    let original = method_setImplementation(method, replacement)
    defer { method_setImplementation(method, original); imp_removeBlock(replacement) }

    for (title, mode) in [("Open Stored Composite", PSDImportChoice.composite), ("Import Layers", .layers)] {
        let document = ImageDocument()
        defer { document.close() }
        observedDocument = document
        selectedTitle = title
        PSDImportRequest.mode = .ask
        try document.read(from: url, ofType: "com.adobe.photoshop-image")
        try require(document.psdImport?.choice == mode, "PSD UI choice retained")
        try require(document.profileAdoption == .unchanged, "PSD UI must retain source profile")
        try require(observedUnopened, "PSD became editable before compatibility notice")
        try require(observedReport.contains("Verified losses in this file")
            && observedReport.contains("General importer limitations"), "PSD notice report separation")
        guard let image = document.doc?.layerImage(0), let pixel = image.sample(x: 0, y: 0, reach: 0) else {
            throw ProbeFailure(description: "PSD UI imported no pixels")
        }
        let expected: (UInt8, UInt8, UInt8, UInt8) = mode == .composite ? (10, 20, 30, 255) : (200, 200, 200, 255)
        try require(pixel.r == expected.0 && pixel.g == expected.1
                    && pixel.b == expected.2 && pixel.a == expected.3,
                    "PSD UI substituted imported layers for saved composite or vice versa")
        guard let captured = document.doc else { throw ProbeFailure(description: "PSD UI lost source") }
        try psdNativeRoundtrip(captured, profile: profile,
                               name: "psd-ui-\(mode == .composite ? "composite" : "layers")")
    }
    selectedTitle = "Cancel"
    let cancelled = ImageDocument(); observedDocument = cancelled
    do { try cancelled.read(from: url, ofType: "com.adobe.photoshop-image"); throw ProbeFailure(description: "PSD cancel opened") }
    catch let failure as ProbeFailure { throw failure }
    catch { try require(cancelled.doc == nil, "PSD cancel published a document") }
    print("PASS PSD UI: full compatibility report before editing; distinct layers/composite choices and cancel")

    observedDocument = nil
    let alertsBefore = alertCount
    let noChoice = try psdTool("open_document", ["path": url.path])
    try require(noChoice.0, "MCP PSD opened without preflight/choice")
    let inspect = try psdTool("inspect_psd", ["path": url.path])
    try require(!inspect.0 && inspect.1["source_fingerprint"] as? String == fingerprint, "MCP PSD preflight")
    try JSONSerialization.data(withJSONObject: inspect.1, options: [.prettyPrinted, .sortedKeys])
        .write(to: probeDirectory.appendingPathComponent("psd-mcp-inspect.json"))
    for (name, mode) in [("composite", PSDImportChoice.composite), ("layers", .layers)] {
        let opened = try psdTool("open_document", ["path": url.path,
            "psd_mode": name, "psd_fingerprint": fingerprint])
        try require(!opened.0, "MCP PSD explicit \(name) choice failed: \(opened.1)")
        guard let mcpDocument = NSDocumentController.shared.document(for: url) as? ImageDocument else {
            throw ProbeFailure(description: "MCP did not register the PSD document")
        }
        defer { mcpDocument.close() }
        try require(mcpDocument.psdImport?.choice == mode, "MCP PSD wrong import mode")
        try mcpDocument.revert(toContentsOf: url, ofType: "com.adobe.photoshop-image")
        try require(mcpDocument.psdImport?.choice == mode, "PSD Revert changed import mode")
        try require(mcpDocument.profileAdoption == .unchanged, "PSD Revert must retain source profile")
        guard let captured = mcpDocument.doc else { throw ProbeFailure(description: "PSD MCP lost source") }
        try psdNativeRoundtrip(captured, profile: profile, name: "psd-mcp-\(name)")
    }
    try require(alertCount == alertsBefore, "MCP or Revert opened a PSD modal")
    print("PASS PSD MCP: inspect/explicit choice, retained report and modal-free Revert")
    print("PASS PSD metadata: exact independent Display P3 ICC and 300/240 PPI through UI/MCP layers/composite and native reopen")
    try psdFlatAdoptionControl(profile: profile, psdURL: url, fingerprint: fingerprint)

    PSDImportRequest.mode = .refuse
    do { _ = try ImageDocument.openPSD(at: url); throw ProbeFailure(description: "PSD Batch accepted unapproved import") }
    catch let failure as ProbeFailure { throw failure }
    catch { }
    var changed = bytes; changed[changed.count - 1] = 31
    try changed.write(to: url)
    PSDImportRequest.mode = .explicit(.composite, fingerprint: fingerprint)
    do { _ = try ImageDocument.openPSD(at: url); throw ProbeFailure(description: "PSD stale report accepted") }
    catch let failure as ProbeFailure { throw failure }
    catch { }
    for (depth, color): (UInt16, UInt16) in [(16,3), (8,4)] {
        try psdFixture(profile: profile, depth: depth, color: color).write(to: url)
        PSDImportRequest.mode = .explicit(.composite, fingerprint: nil)
        let unsupported = ImageDocument()
        do { try unsupported.read(from: url, ofType: "com.adobe.photoshop-image"); throw ProbeFailure(description: "PSD unsupported mode fell back") }
        catch let failure as ProbeFailure { throw failure }
        catch { try require(unsupported.doc == nil, "PSD unsupported file published pixels") }
    }
    try require(alertCount == alertsBefore, "Headless PSD refusal opened modal")
    print("PASS PSD refusal: unsupported color/depth, changed source and unattended Batch never silently fall back")
}
