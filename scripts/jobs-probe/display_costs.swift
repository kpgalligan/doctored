import AppKit

/// Display costs. A render-neutral 8-bit edit carries the current
/// projection, a closed view's panels stop observing the document, the 8-bit
/// proof reads the retained projection, and opening or closing a group keeps
/// the retained projection a proof uses.

private func displayCall(_ tool: String, _ document: ImageDocument,
                         _ arguments: [String: Any] = [:]) throws -> [String: Any] {
    let (response, raw) = try transformResponse(tool, document: document, arguments: arguments)
    try require(response["isError"] as? Bool != true, "\(tool) refused: \(raw)")
    guard let content = response["content"] as? [[String: Any]],
          let text = content.first(where: { $0["type"] as? String == "text" })?["text"] as? String,
          let result = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else {
        throw ProbeFailure(description: "Malformed MCP response to \(tool): \(raw)")
    }
    return result
}

private func genericCMYK() throws -> ProofSetup {
    let path = "/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc"
    let data = try Data(contentsOf: URL(fileURLWithPath: path))
    return ProofSetup(profile: data, profileName: "Generic CMYK Profile", path: path)
}

private func proofBytes(_ image: CGImage) -> Data? {
    image.dataProvider?.data as Data?
}

/// A straight RGBA8 copy of an image's pixels, or nil for an empty handle.
private func displayPlate(_ image: RasterImage) -> [UInt8]? {
    image.withPlateBytes { Array(UnsafeBufferPointer(start: $0, count: image.width * image.height * 4)) }
}

/// "byte render-neutral carry": a rename, its Undo and Redo, and MCP's
/// name-only `set_layer_properties` keep the projection and preview objects.
func verifyByteRenderNeutralCarry() throws {
    let document = try fixture()
    defer { document.close() }
    _ = try editor(document)
    guard let p0 = document.projection, let c0 = document.previewImage,
          let original = document.doc?.layerInfo(0)?.name else {
        throw ProbeFailure(description: "No retained 8-bit projection")
    }
    func carried(_ step: String, name: String) throws {
        try require(document.projection === p0 && document.previewImage === c0,
                    "[\(step)] carried the projection and preview")
        let shown = document.projection.flatMap(displayPlate)
        let fresh = displayPlate(try document.doc.render())
        try require(shown != nil && shown == fresh,
                    "[\(step)] the carried projection shows the document's pixels")
        try require(document.doc?.layerInfo(0)?.name == name,
                    "[\(step)] layer name is \(name): \(document.doc?.layerInfo(0)?.name ?? "nil")")
    }
    withUndoGroup(document.undoManager) {
        document.applyEdit("Rename Layer", record: .notACommand, renderNeutral: true) {
            try $0.withLayerName(0, "Renamed")
        }
    }
    try carried("rename", name: "Renamed")
    document.undoManager?.undo()
    try carried("undo", name: original)
    document.undoManager?.redo()
    try carried("redo", name: "Renamed")
    _ = try displayCall("set_layer_properties", document, ["index": 0, "name": "Agent"])
    try carried("MCP name-only", name: "Agent")

    _ = try displayCall("set_layer_properties", document, ["index": 0, "opacity": 0.5])
    let afterOpacity = document.projection
    try require(afterOpacity != nil && afterOpacity !== p0, "An opacity change renders a new projection")
    _ = try displayCall("set_layer_properties", document,
                        ["index": 0, "name": "Mixed", "opacity": 0.25])
    try require(document.projection != nil && document.projection !== afterOpacity,
                "A name plus opacity change renders a new projection")

    // Negative control: a pixel edit falsely declared render-neutral keeps the
    // projection object, and the pixel comparison must catch it.
    let beforeStale = document.projection
    withUndoGroup(document.undoManager) {
        document.applyEdit("Probe Stale Carry", record: .notACommand, renderNeutral: true) { doc in
            doc.layerImage(0)?.inverted().flatMap { doc.withLayerPixels(0, $0) }
        }
    }
    try require(beforeStale != nil && document.projection === beforeStale,
                "A false render-neutral edit keeps the projection object")
    let staleFresh = displayPlate(try document.doc.render())
    try require(document.projection.flatMap(displayPlate) != staleFresh,
                "The pixel comparison catches a stale carried projection")
    print("PASS: byte render-neutral carry — rename, undo, redo and MCP name-only keep the "
        + "projection and its pixels; opacity and mixed calls render; a stale carry is caught")
}

/// "closed view panels": a closed second view's Layers, Channels and Info
/// panels stop answering the document's change notification.
func verifyClosedViewPanels() throws {
    let document = try fixture()
    defer { document.close() }
    let first = try editor(document)
    _ = try displayCall("open_second_view", document)
    guard let second = document.editor(ordinal: 2) else {
        throw ProbeFailure(description: "No second view")
    }
    second.view.window?.setFrameOrigin(NSPoint(x: -10000, y: -10000))
    second.showInfoTab()
    try pumpUntil({ second.infoPanel.precisionHistogramControl != nil }, seconds: 10)
    guard var control = second.infoPanel.precisionHistogramControl else {
        throw ProbeFailure(description: "View 2's Info panel started no histogram")
    }
    guard let channels = second.channelsPanel else {
        throw ProbeFailure(description: "View 2 has no Channels panel")
    }
    func channelsNeedReload() -> Bool? {
        Mirror(reflecting: channels).children
            .first(where: { $0.label == "needsReload" })?.value as? Bool
    }
    try require(channelsNeedReload() == false, "View 2's Channels panel starts current")
    try require(channels.isViewLoaded && channels.view.isHidden, "View 2's Channels panel is loaded and hidden")
    // Positive control: while view 2 is open, a rename reaches its Layers
    // panel, marks its hidden Channels panel for a reload and starts a new
    // Info histogram (the one the closed-view check then compares against).
    _ = try displayCall("set_layer_properties", document, ["index": 0, "name": "BeforeClose"])
    try? pumpUntil({ channelsNeedReload() == true }, seconds: 5)
    try require(channelsNeedReload() == true, "Open view 2's hidden Channels panel marked a reload")
    try require(second.layersPanel.rows.first?.info.name == "BeforeClose",
                "Open view 2's Layers panel follows the rename: "
                    + "\(second.layersPanel.rows.first?.info.name ?? "nil")")
    try pumpUntil({ second.infoPanel.precisionHistogramControl.map { $0 !== control } ?? false },
                  seconds: 10)
    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
    guard let current = second.infoPanel.precisionHistogramControl else {
        throw ProbeFailure(description: "Open view 2's Info panel started no new histogram")
    }
    control = current
    channels.setPanelVisible(true)
    try require(channelsNeedReload() == false, "View 2's Channels panel reloaded when shown")
    let secondName = second.layersPanel.rows.first?.info.name
    _ = try displayCall("close_second_view", document, ["view": 2])
    _ = try displayCall("set_layer_properties", document, ["index": 0, "name": "AfterClose"])
    RunLoop.main.run(until: Date().addingTimeInterval(0.3))
    try require(first.layersPanel.rows.first?.info.name == "AfterClose",
                "View 1's Layers panel follows the rename")
    try require(second.layersPanel.rows.first?.info.name == secondName,
                "Closed view 2's Layers panel did not reload: "
                    + "\(second.layersPanel.rows.first?.info.name ?? "nil")")
    try require(channels.view.isHidden && channelsNeedReload() == false,
                "Closed view 2's hidden Channels panel did not mark a reload")
    try require(second.infoPanel.precisionHistogramControl === control,
                "Closed view 2's Info panel started no new histogram")
    print("PASS: closed view panels — view 2's Layers, Channels and Info panels ignore edits "
        + "after close while view 1 follows")
}

/// "byte proof retained": the 8-bit proof of the retained projection equals
/// the proof of a fresh render, and the canvas proof lands.
func verifyByteProofRetained() throws {
    let document = try fixture()
    defer { document.close() }
    let controller = try editor(document)
    guard let doc = document.doc, let retained = document.retainedByteProjection else {
        throw ProbeFailure(description: "No retained 8-bit projection")
    }
    try require(retained === document.projection, "The retained projection is the document's own")
    let setup = try genericCMYK()
    let renderer = ProofRenderer()
    func proof(_ source: ProofSource, gamutWarning: Bool = true) throws -> ProofResult {
        var outcome: ProofOutcome?
        renderer.request(source: source, setup: setup, gamutWarning: gamutWarning) { outcome = $0 }
        try pumpUntil({ outcome != nil }, seconds: 30)
        switch outcome {
        case .rendered(let result)?: return result
        case .failed(let reason)?: throw ProbeFailure(description: "Proof failed: \(reason)")
        case nil: throw ProbeFailure(description: "Proof never landed")
        }
    }
    let fromRetained = try proof(.flat(doc, retained))
    let fromRender = try proof(.flat(doc, nil))
    try require(fromRetained.outOfGamut == fromRender.outOfGamut && fromRetained.space == fromRender.space,
                "Retained and rendered proofs agree: \(fromRetained.outOfGamut)/\(fromRetained.space) "
                    + "vs \(fromRender.outOfGamut)/\(fromRender.space)")
    try require(proofBytes(fromRetained.image) != nil
                    && proofBytes(fromRetained.image) == proofBytes(fromRender.image),
                "Retained and rendered proofs have equal pixels")

    withUndoGroup(document.undoManager) {
        document.applyToActiveLayer("Invert", record: .notACommand) { $0.inverted() }
    }
    try require(document.retainedByteProjection != nil
                    && document.retainedByteProjection === document.projection
                    && document.projection !== retained,
                "After a pixel edit the retained projection is the new one")

    // A pixel edit falsely declared render-neutral carries the retained
    // projection past a document it no longer shows, so the canvas proof
    // can tell the retained projection from a fresh render.
    let inverted = document.projection
    withUndoGroup(document.undoManager) {
        document.applyEdit("Probe Stale Carry", record: .notACommand, renderNeutral: true) { doc in
            doc.layerImage(0)?.inverted().flatMap { doc.withLayerPixels(0, $0) }
        }
    }
    guard let stale = document.doc, let carried = document.retainedByteProjection else {
        throw ProbeFailure(description: "No retained 8-bit projection after the stale carry")
    }
    try require(inverted != nil && carried === inverted,
                "The stale carry kept the pre-edit retained projection")
    let staleRetained = try proof(.flat(stale, carried), gamutWarning: false)
    let staleFresh = try proof(.flat(stale, nil), gamutWarning: false)
    try require(proofBytes(staleRetained.image) != nil
                    && proofBytes(staleRetained.image) != proofBytes(staleFresh.image),
                "The retained and freshly rendered proofs differ after the stale carry")
    controller.applyProofSetup(setup, proofColors: true, gamutWarning: false)
    try pumpUntil({ controller.canvas.proofImage != nil }, seconds: 10)
    let canvasProof = controller.canvas.proofImage.flatMap(proofBytes)
    try require(canvasProof != nil && canvasProof == proofBytes(staleRetained.image)
                    && canvasProof != proofBytes(staleFresh.image),
                "The canvas proof is the proof of the retained projection, not a fresh render")
    print("PASS: byte proof retained — retained and rendered proofs equal "
        + "(out_of_gamut \(fromRetained.outOfGamut)); canvas proof lands and reads the retained "
        + "projection")
}

/// "group expansion keeps proof": opening or closing a group re-points the
/// retained projection at the new document, so a proof keeps using it (16
/// bits: stays on; 8 bits: no re-render). Both depths require the disclosure
/// to change (the typed `layer_open` edit at 16 bits).
func verifyGroupExpansionKeepsProof() throws {
    for depth in [8, 16] {
        let document = try depth == 8 ? fixture(width: 400, height: 300)
            : precisionFixture(width: 400, height: 300)
        defer { document.close() }
        let controller = try editor(document)
        let grouped = try displayCall("group_layers", document, ["layers": [0]])
        guard let group = grouped["group"] as? Int else {
            throw ProbeFailure(description: "[\(depth)] group_layers returned no group: \(grouped)")
        }
        func retained() -> AnyObject? {
            depth == 8 ? document.retainedByteProjection : document.retainedPrecisionProjection
        }
        try pumpUntil({ retained() != nil }, seconds: 20)
        controller.applyProofSetup(try genericCMYK(), proofColors: true, gamutWarning: false)
        try pumpUntil({ controller.canvas.proofImage != nil }, seconds: 20)
        guard let before = retained(), let open = document.doc?.layerInfo(group)?.open else {
            throw ProbeFailure(description: "[\(depth)] no retained projection or group")
        }
        for expanded in [!open, open] {
            document.setGroupExpanded(group, expanded, record: .notACommand)
            try require(document.doc?.layerInfo(group)?.open == expanded,
                        "[\(depth)] the group's disclosure changed")
            try require(retained() === before, "[\(depth)] the retained projection is still used")
            try require(controller.proofDisplay.isActive && controller.proofDisplay.failure == nil,
                        "[\(depth)] proofing stayed on: \(controller.proofDisplay.failure ?? "")")
        }
        RunLoop.main.run(until: Date().addingTimeInterval(0.3))
        try require(controller.proofDisplay.isActive && controller.canvas.proofImage != nil,
                    "[\(depth)] the proof is still drawn")
    }
    print("PASS: group expansion keeps proof — 8 and 16 bits change the disclosure and keep the "
        + "retained projection and the proof on")
}
