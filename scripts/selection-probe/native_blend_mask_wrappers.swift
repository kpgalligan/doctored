import AppKit

func verifyNativeBlendMaskWrappers() throws {
    guard let input = ProcessInfo.processInfo.environment["DR_MASK_FOUNDATION_FIXTURES"],
          let output = ProcessInfo.processInfo.environment["DR_SELECTION_PROBE_DIR"] else {
        throw ProbeFailure(description: "Combined blend/mask probe needs independent mask fixtures")
    }
    let original = try RasterDocument.open(url: URL(fileURLWithPath: input).appendingPathComponent("16-outside-white.rz"))
    guard let native = original.precision, let image = native.layerSource(0) else {
        throw ProbeFailure(description: "Combined native fixture owner")
    }
    let pixels = try image.withRGBA16 { Array($0) }
    let mask = try PrecisionScalarOperations.read(original, target: "mask", index: 0).samples
    let document = try fixture(width: 4, height: 3)
    defer { document.close() }
    withUndoGroup(document.undoManager) {
        document.applyEdit("Combined mask/blend fixture", record: .notACommand,
                           intentionalDepthConversion: true) { _ in original }
    }
    _ = try call(document, "duplicate_layer", ["index": 0])
    _ = try call(document, "set_active_layer", ["index": 1])
    document.updateChangeCount(.changeCleared)
    document.undoManager?.removeAllActions()
    guard let baseline = document.doc else { throw ProbeFailure(description: "Combined baseline") }
    let panel = try editor(document).layersPanel
    guard let popup = allViews(panel?.view ?? NSView()).compactMap({ $0 as? NSPopUpButton })
        .first(where: { $0.itemTitles.contains("Color Dodge") && $0.itemTitles.contains("Linear Burn") }) else {
        throw ProbeFailure(description: "Combined native Layers menu")
    }
    func check(_ owner: RasterDocument, mode: Int) throws {
        try require(owner.layerInfo(1)?.blendMode.rawValue == UInt32(mode), "Combined wrapper preserves blend mode")
        for index in 0...1 {
            guard let image = owner.precision?.layerSource(index), let properties = owner.layerMaskProperties(index) else {
                throw ProbeFailure(description: "Combined archive source/mask")
            }
            let actualPixels = try image.withRGBA16 { Array($0) }
            let actualMask = try PrecisionScalarOperations.read(owner, target: "mask", index: index).samples
            try require(actualPixels == pixels && actualMask == mask && properties.outsideCoverage == 255,
                        "Combined archive preserves exact raw RGB/alpha/mask and outside255")
        }
    }
    let directory = URL(fileURLWithPath: output)
    for mode in [10, 11, 12, 15, 13, 18, 19, 20, 22] {
        let version: UInt8 = [10, 11, 12, 15].contains(mode) ? 30 : 31
        popup.selectItem(withTag: mode)
        let errors = try capturedPresentationError { _ = popup.sendAction(popup.action, to: popup.target) }
        try require(errors.isEmpty, "Combined popup errors: \(errors)")
        guard let changed = document.doc else { throw ProbeFailure(description: "Combined popup result") }
        try check(changed, mode: mode)
        let path = directory.appendingPathComponent("outside-blend-\(mode).rz")
        try changed.saveNative(to: path)
        let data = try Data(contentsOf: path)
        // Since 12E-1 the typed identity word 46 leads the chain; the
        // canonical mask29 -> blend30/31 -> prior13 run follows it.
        let words = leadingVersionWords(data)
        let ordered = versionChain(words, contains: [29, UInt32(version), 13])
        try require(words.first == 46 && ordered,
                    "Canonical mask29 -> blend30/31 -> prior13 wrapper order: \(words)")
        try check(RasterDocument.open(url: path), mode: mode)
        if mode == 11 || mode == 18 {
            let snapshot = try document.namedSnapshots.create(name: "Combined outside blend")
            let url = directory.appendingPathComponent("outside-blend-\(mode).drsnapshot")
            try document.namedSnapshots.save(id: snapshot.id, to: url)
            let restored = try document.namedSnapshots.load(from: url)
            try check(restored.state.document, mode: mode)
        }
        document.undoManager?.undo()
        try require(document.doc === baseline && !document.isDocumentEdited,
                    "One combined popup Undo restores exact original owner and clean state")
    }
    print("PASS: outside255 plus nine actual blend controls, canonical29/30/31 native reopen, exact pixels/masks, snapshot and owner Undo")
}
