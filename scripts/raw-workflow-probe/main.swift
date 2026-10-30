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
          let content = envelope["content"] as? [[String: Any]], let text = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String else {
        throw ProbeFailure(description: "Malformed \(tool) response")
    }
    try require((envelope["isError"] as? Bool ?? false) == refuses, "\(tool): \(text)")
    if refuses { return ["refusal": text] }
    var result = (try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any]) ?? [:]
    result["_images"] = content.filter { $0["type"] as? String == "image" }
    return result
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
func descriptor(_ document: ImageDocument, layer: Int = 0) throws -> RawSourceDescriptor {
    guard let source = document.doc else { throw ProbeFailure(description: "Missing document") }
    return try RawSourceDescriptor.read(RawSourceWork.info(source: source, layer: layer))
}
func value(_ document: ImageDocument, layer: Int = 0) throws -> RawDevelopValues {
    try descriptor(document, layer: layer).recipe.values
}
func makeDocument(_ source: RasterDocument) -> ImageDocument {
    let document = ImageDocument()
    document.fileType = ImageDocument.nativeTypeIdentifier
    document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
        layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
        stripMetadata: false, exportBitDepth: source.isHighDepth ? 16 : 8, exportDither: "none"), markSaved: true)
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers(); document.showWindows()
    return document
}
func forged(_ document: ImageDocument, mutate: (inout [String: Any]) -> Void) throws -> ImageDocument {
    guard let source = document.doc else { throw ProbeFailure(description: "Missing forging source") }
    var raw = try JSONSerialization.jsonObject(with: Data(descriptor(document).json().utf8)) as? [String: Any] ?? [:]
    mutate(&raw)
    let json = try JSONSerialization.data(withJSONObject: raw, options: .sortedKeys)
    let description = try JSONDecoder().decode(RawSourceDescriptor.self, from: json)
    let master = try source.sourceMaster16(0)
    let asset = try master.withRGBA16 { samples in
        try RasterDocument.fromRawRGBA16(Array(samples), width: master.width, height: master.height, profile: master.profileData)
    }
    let result = try RawSourceWork.replacing(source: source, layer: 0, asset: asset, descriptor: description)
    return makeDocument(result)
}
func refusal(_ title: String, _ operation: () throws -> Void) throws {
    var failed = false
    do { try operation() } catch { failed = true }
    try require(failed, title)
}
func testLibrary() throws -> [String] {
    func requireOneBrokenRow(_ fault: RawPresetLibrary.Fault) throws {
        let listing = try RawPresetLibrary.listing()
        try require(listing.entries.map { $0.preset.name } == ["Exposure"], "One bad preset file keeps the good entries listed")
        try require(listing.broken.count == 1, "One bad preset file is exactly one broken row")
        let row = listing.broken[0]
        try require(row.file == "unsupported.json" && row.fault == fault && !row.repairable,
                    "Broken row names the file, its \(fault.rawValue) fault and is not repairable")
        try require(try RawPresetLibrary.named("Exposure").settings.exposure == 0.5, "Good presets still resolve beside a broken row")
    }
    try require(try RawPresetLibrary.entries().isEmpty, "Private preset library starts empty")
    try RawPresetLibrary.save(name: "Exposure", settings: .init(exposure: 0.25))
    try require(try RawPresetLibrary.named("exposure").settings.exposure == 0.25, "Case-insensitive name resolves exact stored value")
    try refusal("Duplicate names require explicit replace") { _ = try RawPresetLibrary.save(name: "EXPOSURE", settings: .init(exposure: 1)) }
    try RawPresetLibrary.save(name: "Exposure", settings: .init(exposure: 0.5), replace: true)
    try require(try RawPresetLibrary.named("Exposure").settings.exposure == 0.5, "Explicit replacement is atomic")
    for raw: [String: Any] in [["decoder": "future"], ["exposure": true], ["exposure": 99],
                              ["exposure": NSNull()], ["as_shot_white_balance": true, "temperature": 7000]] {
        try refusal("Unknown, invalid and conflicting preset settings refuse") { _ = try RawSettingsPatch.decode(raw) }
    }
    let bad = RawPresetLibrary.directory.appendingPathComponent("unsupported.json")
    try Data("{\"schema_version\":99,\"name\":\"Future\",\"raw\":{\"exposure\":1}}".utf8).write(to: bad)
    try requireOneBrokenRow(.unknownVersion)
    try FileManager.default.removeItem(at: bad)
    try Data(repeating: 32, count: RawPresetLibrary.maximumFileBytes + 1).write(to: bad)
    try requireOneBrokenRow(.oversize)
    try FileManager.default.removeItem(at: bad)
    for index in 1..<RawPresetLibrary.maximumPresets {
        let record: [String: Any] = ["schema_version": 1, "name": "Limit \(index)", "raw": ["exposure": 0.2]]
        try JSONSerialization.data(withJSONObject: record).write(to: RawPresetLibrary.directory.appendingPathComponent("limit-\(index).json"))
    }
    try refusal("Library cap refuses a 129th preset") { _ = try RawPresetLibrary.save(name: "Too Many", settings: .init(exposure: 1)) }
    for index in 1..<RawPresetLibrary.maximumPresets {
        try FileManager.default.removeItem(at: RawPresetLibrary.directory.appendingPathComponent("limit-\(index).json"))
    }
    return ["versioned_sparse_library", "explicit_replacement", "strict_types_fields_and_conflicts", "per_entry_broken_rows", "bounded_file_and_library"]
}
func testSettings(_ root: URL) throws -> [String] {
    let source = try opened(root.appendingPathComponent("source.dng"), depth: 16)
    let byte = try opened(root.appendingPathComponent("target.dng"), depth: 8)
    let typed = try opened(root.appendingPathComponent("second.dng"), depth: 16)
    defer { source.close(); byte.close(); typed.close() }
    try call("duplicate_layer", typed, ["index": 0])
    let asShotSource = try descriptor(source).recipe
    let asShotTarget = try descriptor(byte).recipe
    try require(asShotSource.asShotChromaticityX != asShotTarget.asShotChromaticityX
        || asShotSource.asShotChromaticityY != asShotTarget.asShotChromaticityY,
        "Independent camera metadata fixtures have different as-shot white balance")
    try call("redevelop_raw", source, ["layer": 0, "raw": ["exposure": 0.45, "temperature": 7000, "tint": 17, "output_space": "display_p3"]])
    let saved = try call("save_raw_preset", source, ["name": "Copied Controls", "fields": ["exposure", "white_balance", "output_space"]])
    let before = typed.doc
    try call("start_recording")
    try call("apply_raw_preset", typed, ["name": "Copied Controls", "layers": [0, 1]])
    let recording = try call("stop_recording", nil, ["name": "Resolved RAW preset"])
    let steps = recording["steps"] as? [[String: Any]] ?? []
    try require(steps.count == 1 && steps.first?["tool"] as? String == "apply_raw_settings", "Preset records one resolved document-local Action step")
    let changed = typed.doc
    try require(before !== changed, "Multi-layer preset publishes one new document")
    try require(try descriptor(typed, layer: 0).recipe.outputSpace.contains("display-p3")
        && descriptor(typed, layer: 1).recipe.outputSpace.contains("display-p3"), "Preset includes explicit output gamut")
    try call("undo", typed)
    try require(typed.doc === before, "One Undo restores both RAW layers")
    try call("redo", typed)
    try require(typed.doc === changed, "One Redo restores both RAW layers")
    let revision = typed.revision
    try call("apply_raw_preset", typed, ["name": "Copied Controls", "layers": [0, 1]])
    try require(typed.doc === changed && typed.revision == revision, "Repeated preset does not create Undo")
    try call("save_raw_preset", nil, ["name": "Copied Controls", "replace": true, "raw": ["exposure": 2.0]])
    try call("undo", typed)
    try call("run_action", typed, ["name": "Resolved RAW preset"])
    try require(abs(try value(typed).exposure - 0.45) < 1e-6, "Action replay retains original values after library replacement")
    try call("delete_raw_preset", nil, ["name": "Copied Controls"])
    try call("undo", typed)
    try call("run_action", typed, ["name": "Resolved RAW preset"])
    try require(abs(try value(typed, layer: 1).exposure - 0.45) < 1e-6, "Action replay survives preset deletion")
    let byteBefore = byte.doc, typedBefore = typed.doc
    let targets: [[String: Any]] = [["document_id": AgentServer.shared.documentID(for: byte), "layer": 0],
                                  ["document_id": AgentServer.shared.documentID(for: typed), "layer": 0],
                                  ["document_id": AgentServer.shared.documentID(for: typed), "layer": 1]]
    try call("sync_raw_settings", source, ["layer": 0, "fields": ["exposure", "white_balance"], "targets": targets])
    try require(byte.doc !== byteBefore && typed.doc === typedBefore, "Cross-depth sync changes only targets requiring an edit")
    let byteAfter = byte.doc
    try call("undo", byte)
    try require(byte.doc === byteBefore, "Synced document has its own independent Undo")
    try call("redo", byte)
    try require(byte.doc === byteAfter, "Sync Redo restores its exact result")
    try call("redevelop_raw", source, ["layer": 0, "raw": ["as_shot_white_balance": true]])
    try call("sync_raw_settings", source, ["fields": ["white_balance"], "targets": [targets[0]]])
    let reset = try descriptor(byte).recipe
    try require(!reset.values.customWhiteBalance && reset.chromaticityX == asShotTarget.asShotChromaticityX
        && reset.chromaticityY == asShotTarget.asShotChromaticityY, "As-shot sync uses receiving camera's own captured chromaticity")
    let stableByte = byte.doc, stableTyped = typed.doc
    for target: [String: Any] in [["document_id": AgentServer.shared.documentID(for: typed), "layer": 999],
                                  ["document_id": AgentServer.shared.documentID(for: typed), "layer": 0.5],
                                  ["document_id": AgentServer.shared.documentID(for: typed), "layer": true],
                                  ["document_id": AgentServer.shared.documentID(for: typed), "layer": NSNull()],
                                  ["document_id": AgentServer.shared.documentID(for: typed), "layer": 0, "unknown": 1]] {
        try call("sync_raw_settings", source, ["fields": ["exposure"], "targets": [targets[0], target]], refuses: true)
        try require(byte.doc === stableByte && typed.doc === stableTyped, "Invalid final target leaves every document untouched")
    }
    try call("redevelop_raw", source, ["raw": ["exposure": 0.85]])
    let unsupported = try forged(source) { raw in
        var recipe = raw["recipe"] as? [String: Any] ?? [:]
        var capabilities = recipe["capabilities"] as? [String: Any] ?? [:]
        capabilities["colorNoise"] = false; recipe["capabilities"] = capabilities; raw["recipe"] = recipe
    }
    defer { unsupported.close() }
    let unsupportedID = AgentServer.shared.documentID(for: unsupported)
    try call("sync_raw_settings", source, ["fields": ["color_noise"], "targets": [targets[0], ["document_id": unsupportedID, "layer": 0]]], refuses: true)
    try require(byte.doc === stableByte && typed.doc === stableTyped, "Unsupported final target refuses before any publication")
    var changedSource = false
    let timer = Timer(timeInterval: 0.01, repeats: false) { _ in
        changedSource = (try? call("select_rect", source, ["x": 3, "y": 4, "width": 10, "height": 11])) != nil
    }
    RunLoop.main.add(timer, forMode: .common)
    RunLoop.main.add(timer, forMode: .modalPanel)
    try call("sync_raw_settings", source, ["fields": ["exposure"], "targets": [targets[1], targets[2]]], refuses: true)
    timer.invalidate()
    try require(changedSource && source.canvasSelection != nil && typed.doc === stableTyped, "Source selection changes during workers refuse publication to all targets")
    _ = saved
    return ["preset_capture_explicit_fields", "same_document_multi_raw_one_undo", "canonical_action_recording",
        "preset_noop", "mutable_library_independent_replay", "cross_depth_multi_document_sync",
        "target_local_as_shot", "unsupported_target_preflight", "all_target_preflight", "strict_target_identifiers", "source_stale_refusal"]
}
func testInspectionMigration(_ root: URL) throws -> [String] {
    let original = try opened(root.appendingPathComponent("inspect.dng"), depth: 16)
    defer { original.close() }
    let before = original.doc, revision = original.revision
    let read = try call("inspect_raw_development", original, ["region": ["x": 10, "y": 20, "width": 32, "height": 24]])
    try require((read["_images"] as? [[String: Any]])?.count == 2 && read["preview_divisor"] as? Int == 1,
                "Inspection returns before/after at 100% source-pixel scale")
    let oldHistogram = read["before_histogram"] as? [String: Any] ?? [:]
    guard let master = try original.doc?.sourceMaster16(0) else { throw ProbeFailure(description: "Missing inspected source master") }
    let rows = oldHistogram["bins"] as? [[NSNumber]] ?? []
    try require(rows.count == 4 && oldHistogram["total"] as? Int == master.width * master.height
        && oldHistogram["sampled_pixels"] as? Int == master.width * master.height, "Histogram visits all source pixels")
    try master.withRGBA16 { samples in
        var reference = Array(repeating: Array(repeating: 0, count: 256), count: 3)
        var zero = [0, 0, 0, 0], maximum = [0, 0, 0, 0]
        for pixel in 0..<(master.width * master.height) {
            for channel in 0..<4 {
                let value = Int(samples[pixel * 4 + channel])
                if value == 0 { zero[channel] += 1 }
                if value == 65535 { maximum[channel] += 1 }
                if channel < 3 { reference[channel][(value + 128) / 257] += 1 }
            }
        }
        for channel in 0..<3 { try require(rows[channel].map(\.intValue) == reference[channel], "Independent histogram RGB bins exact") }
        try require((oldHistogram["exact_zero_rgba"] as? [NSNumber])?.map(\.intValue) == zero
            && (oldHistogram["exact_max_rgba"] as? [NSNumber])?.map(\.intValue) == maximum, "Independent exact endpoint counts")
        let detail = try master.developDetail(x: 10, y: 20, width: 32, height: 24)
        try detail.withRGBA16 { crop in
            for y in 0..<24 { for x in 0..<32 { for channel in 0..<4 {
                try require(crop[(y * 32 + x) * 4 + channel] == samples[((y + 20) * master.width + x + 10) * 4 + channel], "100% detail preserves every original source code")
            } } }
        }
    }
    try JSONSerialization.data(withJSONObject: oldHistogram, options: [.prettyPrinted, .sortedKeys]).write(to: root.appendingPathComponent("histogram.json"))
    let proposed = try call("inspect_raw_development", original, ["raw": ["exposure": 1, "output_space": "display_p3"], "region": ["x": 10, "y": 20, "width": 32, "height": 24]])
    try require(original.doc === before && original.revision == revision && proposed["committed"] as? Bool == false,
                "What-if RAW histogram/detail never publishes settings")
    var authored = try descriptor(original)
    try require(authored.recipe.capabilities.colorNoise, "Synthetic fixture supports colour noise for migration-loss test")
    authored.recipe.values.colorNoise = authored.recipe.capabilities.defaults.colorNoise == 0.25 ? 0.75 : 0.25
    var currentObject = try JSONSerialization.jsonObject(with: Data(descriptor(original).json().utf8)) as? [String: Any] ?? [:]
    var currentRecipe = currentObject["recipe"] as? [String: Any] ?? [:]
    var currentCapabilities = currentRecipe["capabilities"] as? [String: Any] ?? [:]
    currentCapabilities["colorNoise"] = false; currentRecipe["capabilities"] = currentCapabilities; currentObject["recipe"] = currentRecipe
    let unsupportedCurrent = try JSONDecoder().decode(RawSourceDescriptor.self, from: JSONSerialization.data(withJSONObject: currentObject))
    try refusal("Migration refuses loss of an authored control") { _ = try RawDecoderMigration.recipe(previous: authored, current: unsupportedCurrent) }
    let unavailable = try forged(original) { raw in
        var decoder = raw["decoder"] as? [String: Any] ?? [:]
        decoder["implementation"] = "Previous independent decoder implementation"
        raw["decoder"] = decoder
    }
    defer { unavailable.close() }
    let savedPixels = hash(try layerBytes(unavailable, layer: 0))
    try call("inspect_raw_development", unavailable, ["region": ["x": 1, "y": 2, "width": 8, "height": 6]])
    let preview = try call("preview_raw_decoder_migration", unavailable)
    guard let previewID = preview["preview_id"] as? String else { throw ProbeFailure(description: "Migration lacks preview ticket") }
    try require(hash(try layerBytes(unavailable, layer: 0)) == savedPixels, "Migration preview retains saved rendering")
    try call("set_layer_properties", unavailable, ["index": 0, "name": "Changed during review"])
    let altered = unavailable.doc
    try call("migrate_raw_decoder", unavailable, ["preview_id": previewID], refuses: true)
    try require(unavailable.doc === altered, "Stale migration ticket cannot publish")
    let fresh = try call("preview_raw_decoder_migration", unavailable)
    guard let freshID = fresh["preview_id"] as? String else { throw ProbeFailure(description: "Missing renewed ticket") }
    try call("start_recording")
    try call("migrate_raw_decoder", unavailable, ["preview_id": freshID])
    let action = try call("stop_recording", nil, ["name": "Reviewed migration"])
    let steps = action["steps"] as? [[String: Any]] ?? []
    try require(steps.count == 1 && steps.first?["tool"] as? String == "migrate_raw_decoder", "Migration records stable captured recipes")
    let migrated = unavailable.doc
    try call("undo", unavailable)
    try require(unavailable.doc === altered, "Migration Undo restores saved implementation and pixels")
    try call("run_action", unavailable, ["name": "Reviewed migration"])
    try require(try descriptor(unavailable).compatible, "Migration Action replay restores reviewed current decoder")
    try call("migrate_raw_decoder", unavailable, ["preview_id": freshID], refuses: true)
    let unknown = try forged(original) { raw in
        var recipe = raw["recipe"] as? [String: Any] ?? [:]
        recipe["version"] = 99; raw["recipe"] = recipe
    }
    defer { unknown.close() }
    let unknownOwner = unknown.doc
    try call("inspect_raw_development", unknown)
    for tool in ["inspect_raw_development", "redevelop_raw", "preview_raw_decoder_migration"] {
        var arguments: [String: Any] = [:]
        if tool != "preview_raw_decoder_migration" { arguments["raw"] = ["output_space": "display_p3"] }
        try call(tool, unknown, arguments, refuses: true)
    }
    try require(unknown.doc === unknownOwner, "Unknown recipes keep saved inspection and refuse all reinterpretation")
    let migrationNative = root.appendingPathComponent("migrated.rz")
    try call("save_copy", unavailable, ["path": migrationNative.path])
    _ = migrated
    return ["source_100_percent_inspection", "independent_full_histogram_bins_endpoints", "what_if_histogram_and_gamut_no_mutation", "saved_inspection_without_decoder",
        "reviewed_migration", "stale_preview_refusal", "migration_undo_and_stable_action", "consumed_ticket_refusal", "unknown_recipe_preservation", "authored_control_migration_refusal"]
}
func testPresetSheets(_ original: ImageDocument) throws -> [String] {
    func descendants(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(descendants) }
    func modal<T>(_ title: String, run: () -> T, drive: @escaping ([NSView], NSView) throws -> Void) throws -> T {
        var failure: Error?, visited = false
        let deadline = Date().addingTimeInterval(20)
        let timer = Timer(timeInterval: 0.03, repeats: true) { _ in
            do {
                try require(Date() < deadline, "Preset modal timed out: \(title)")
                guard !visited, let window = NSApp.windows.first(where: { $0.title == title && $0.isVisible }),
                      let content = window.contentView else { return }
                visited = true
                content.layoutSubtreeIfNeeded()
                let screen = window.screen?.visibleFrame ?? .zero
                try require(screen.contains(window.frame), "Preset modal fits the visible screen: \(title)")
                try drive(descendants(content), content)
            } catch { failure = error; NSApp.stopModal(withCode: .cancel) }
        }
        RunLoop.main.add(timer, forMode: .common); RunLoop.main.add(timer, forMode: .modalPanel)
        let result = run()
        timer.invalidate()
        if let failure { throw failure }
        try require(visited, "Actual preset modal was driven: \(title)")
        return result
    }
    func button(_ views: [NSView], title: String) throws -> NSButton {
        guard let value = views.compactMap({ $0 as? NSButton }).first(where: { $0.title == title }) else {
            throw ProbeFailure(description: "Missing preset button \(title)")
        }
        return value
    }
    let raw = try descriptor(original)
    let saved = try modal("Save RAW Preset", run: {
        RawPresetSaveSheet.run(values: raw.recipe.values, capabilities: raw.recipe.capabilities, outputSpace: .sRGB)
    }) { views, content in
        guard let name = views.first(where: { $0.accessibilityIdentifier() == "raw-preset-name" }) as? NSTextField else {
            throw ProbeFailure(description: "Missing preset name control")
        }
        name.stringValue = "Actual UI exposure"
        for control in views.compactMap({ $0 as? NSButton }) {
            let id = control.accessibilityIdentifier()
            if id.hasPrefix("raw-field-") {
                control.state = id == "raw-field-exposure" ? .on : .off
            }
        }
        let save = try button(views, title: "Save Preset")
        try require(content.bounds.contains(save.convert(save.bounds, to: content)), "Save button remains visible")
        save.performClick(nil)
    }
    try require(saved, "Actual Save Preset accepts explicit controls")
    let written = try RawPresetLibrary.named("Actual UI exposure")
    try require(Set(written.settings.jsonObject().keys) == ["exposure"], "Actual preset picker saves only chosen fields")
    let selected = try modal("RAW Presets", run: { RawPresetManagerSheet.run(descriptor: raw) }) { views, _ in
        guard let popup = views.first(where: { $0.accessibilityIdentifier() == "raw-preset-manager-list" }) as? NSPopUpButton else {
            throw ProbeFailure(description: "Missing preset manager selection")
        }
        popup.selectItem(withTitle: "Actual UI exposure")
        // A clean library shows no broken-file section: its popup is either
        // detached from the stack or behind a hidden ancestor.
        if let brokenList = views.first(where: { $0.accessibilityIdentifier() == "raw-preset-broken-list" }) {
            try require(brokenList.isHiddenOrHasHiddenAncestor, "Clean library hides the broken-file section")
        }
        try button(views, title: "Apply").performClick(nil)
    }
    try require(selected?.name == written.name && selected?.settings.jsonObject()["exposure"] as? Double == raw.recipe.values.exposure,
                "Actual manager returns captured saved settings")
    // Two files the reader cannot list, both unknown_version: the manager
    // lists both, Repair exists but is disabled (neither is repairable), and
    // Delete File removes exactly the selected one.
    let brokenFiles = ["broken-empty.json", "broken-future.json"]
    try FileManager.default.createDirectory(at: RawPresetLibrary.directory, withIntermediateDirectories: true)
    try Data("{}".utf8).write(to: RawPresetLibrary.directory.appendingPathComponent(brokenFiles[0]))
    try Data("{\"schema_version\": 99, \"name\": \"x\"}".utf8)
        .write(to: RawPresetLibrary.directory.appendingPathComponent(brokenFiles[1]))
    defer {
        for file in brokenFiles {
            try? FileManager.default.removeItem(at: RawPresetLibrary.directory.appendingPathComponent(file))
        }
    }
    var brokenRows = 0, repairEnabled = true, remainingBroken = -1
    _ = try modal("RAW Presets", run: { RawPresetManagerSheet.run(descriptor: raw) }) { views, content in
        guard let list = views.first(where: { $0.accessibilityIdentifier() == "raw-preset-broken-list" }) as? NSPopUpButton,
              !list.isHiddenOrHasHiddenAncestor else {
            throw ProbeFailure(description: "Broken RAW preset files are not listed")
        }
        brokenRows = list.numberOfItems
        guard let repair = views.first(where: { $0.accessibilityIdentifier() == "raw-preset-repair" }) as? NSButton,
              let deleteFile = views.first(where: { $0.accessibilityIdentifier() == "raw-preset-delete-file" }) as? NSButton else {
            throw ProbeFailure(description: "Missing RAW broken-file Repair or Delete File")
        }
        repairEnabled = repair.isEnabled
        deleteFile.performClick(nil)
        remainingBroken = try RawPresetLibrary.listing().broken.count
        try button(descendants(content), title: "Cancel").performClick(nil)
    }
    try require(brokenRows == 2, "Manager lists both broken RAW preset files")
    try require(!repairEnabled, "Repair is disabled for unknown_version files")
    try require(remainingBroken == 1, "Delete File removes exactly one broken RAW preset file")
    guard let source = original.doc else { throw ProbeFailure(description: "Missing source for sync sheet") }
    let receiving = makeDocument(source)
    defer { receiving.close() }
    let target = RawSettingsTarget(document: receiving, layer: 0)
    let selection = try modal("Synchronize RAW Settings", run: {
        RawSyncSheet.run(targets: [target], capabilities: raw.recipe.capabilities)
    }) { views, content in
        guard let check = views.first(where: { $0.accessibilityIdentifier() == "raw-sync-target-0" }) as? NSButton else {
            throw ProbeFailure(description: "Missing synchronization target checkbox")
        }
        let rectangle = check.convert(check.bounds, to: content)
        try require(rectangle.width > 0 && rectangle.height > 0 && content.bounds.contains(rectangle), "Synchronization target is visible and has a usable frame")
        check.state = .on
        try button(views, title: "Apply").performClick(nil)
    }
    try require(selection?.0 == [.exposure, .whiteBalance] && selection?.2.count == 1 && selection?.2.first?.document === receiving,
                "Actual synchronization sheet resolves explicit target and selected fields")
    try RawPresetLibrary.delete(name: written.name)
    return ["actual_preset_save_explicit_controls", "actual_preset_manager_captured_settings", "actual_preset_manager_broken_rows",
            "actual_sync_sheet_target_and_field_selection"]
}
let app = NSApplication.shared
AutomationAlerts.installIfEnabled()
app.setActivationPolicy(.accessory); app.finishLaunching()
do {
    guard let directory = ProcessInfo.processInfo.environment["DR_RAW_WORKFLOW_PROBE_DIR"] else { throw ProbeFailure(description: "Missing private probe directory") }
    let root = URL(fileURLWithPath: directory)
    var groups = try testLibrary()
    groups += try testSettings(root)
    groups += try testInspectionMigration(root)
    let original = try opened(root.appendingPathComponent("ui.dng"), depth: 16)
    guard let source = original.doc, let probe = RawImage.inspect(url: root.appendingPathComponent("ui.dng")) else {
        throw ProbeFailure(description: "Missing UI RAW source")
    }
    groups += try testPresetSheets(original)
    let snapshot = try RawSourceSnapshot.capture(root.appendingPathComponent("ui.dng"), maximumBytes: 1024 * 1024)
    let ui = try runDevelopUIProbe(probe: probe, descriptor: descriptor(original), snapshot: snapshot,
        beforeSource: source.sourceMaster16(0), directory: root)
    original.close()
    let report: [String: Any] = ["passed": true, "groups": groups, "ui": ui]
    try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]).write(to: root.appendingPathComponent("workflow-report.json"))
    print("PASS: RAW workflow host \(groups.count) groups plus actual Develop UI")
    exit(0)
} catch { print("FAIL: RAW workflow host: \(error)"); exit(1) }
