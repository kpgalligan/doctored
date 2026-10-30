import AppKit

// Free Transform a Copy with no selection (⌥⌘T duplicates
// the target layers and transforms the copies, one "Transform Copy" step,
// against its MCP twin `transform_layers duplicate`), a duplicate's drag and
// box snapping to their source, and the geometry commit waits revealing their
// panel only after `DocumentJobs.panelDelay`. Every case calls methods
// directly, so none needs a key window.

private func layerOffset(_ document: ImageDocument, _ layer: Int) -> [Int]? {
    document.doc?.layerInfo(layer).map { [$0.offsetX, $0.offsetY] }
}

private func pixelSamples(_ document: ImageDocument, _ points: [(Int, Int)]) throws -> [NSDictionary] {
    try points.map { NSDictionary(dictionary: try agentOK("sample_pixel", document,
                                                          ["x": $0.0, "y": $0.1])) }
}

private func matrixArgument(_ matrix: CGAffineTransform) -> [Double] {
    [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty].map(Double.init)
}

/// What a committed copy looks like: pixels plus offset at 8 bits, six
/// `sample_pixel` readings at 16.
private func copyState(_ document: ImageDocument, depth: Int) throws -> [String] {
    if depth == 8 {
        let pixels = document.doc?.layerPixels(1) ?? []
        let checksum = pixels.reduce(0) { ($0 &* 31) &+ Int($1) }
        return ["\(pixels.count):\(checksum)",
                "\(layerOffset(document, 1) ?? [])", "\(document.doc?.layerCount ?? 0)"]
    }
    let points = [(24, 20), (30, 24), (35, 30), (20, 16), (40, 28), (28, 34)]
    return try pixelSamples(document, points).map { "\($0)" }
}

/// A described copy's pixels, offset and description.
private func describedCopyState(_ document: ImageDocument, _ layer: Int) -> [String] {
    let pixels = document.doc?.layerPixels(layer) ?? []
    let checksum = pixels.reduce(0) { ($0 &* 31) &+ Int($1) }
    return ["\(pixels.count):\(checksum)", "\(layerOffset(document, layer) ?? [])",
            document.doc?.layerMeta(layer) ?? "-"]
}

func verifyFreeTransformCopyLayers() throws {
    defer { _ = ActionRecorder.shared.stop() }
    for depth in [8, 16] {
        let tag = "[\(depth)]"
        let document = depth == 8
            ? try fixture(width: 64, height: 64) : try precisionFixture(width: 64, height: 48)
        defer { document.close() }
        let controller = try editor(document)
        guard let base = document.doc else { throw ProbeFailure(description: "\(tag) fixture") }
        let sampler = EditorViewController.samplerAgentName(controller.transformSampler)

        // Open: the session is over the duplicate; the document is untouched.
        try require(controller.validateFreeTransformCopyItem(),
                    "\(tag) ⌥⌘T validates with no selection")
        controller.freeTransformCopy(nil)
        try require(controller.transformSession?.duplicate != nil,
                    "\(tag) ⌥⌘T opened a duplicate session")
        try require(controller.transformSession?.layers == [1],
                    "\(tag) the session's layers are the copy: "
                    + "\(String(describing: controller.transformSession?.layers))")
        try require(document.doc === base, "\(tag) opening left the document untouched")
        try require(controller.transformStatusText.hasPrefix("Free Transform · copy"),
                    "\(tag) status: \(controller.transformStatusText)")

        // A translation, recorded as ONE transform_layers duplicate step.
        controller.transformSession?.transform.translation = CGVector(dx: 7, dy: 0)
        guard let pivot = controller.transformSession?.transform.pivot else {
            throw ProbeFailure(description: "\(tag) no session pivot")
        }
        ActionRecorder.shared.start()
        let committed = controller.commitTransformSession()
        let steps = ActionRecorder.shared.stop()
        try require(committed, "\(tag) the commit answered true")
        try require(document.doc?.layerCount == 2, "\(tag) the commit added the copy")
        try require(document.doc?.layerInfo(1)?.name.hasSuffix("copy") == true,
                    "\(tag) layer 1 is the copy: \(document.doc?.layerInfo(1)?.name ?? "-")")
        try require(layerOffset(document, 1) == [7, 0] && layerOffset(document, 0) == [0, 0],
                    "\(tag) the copy moved (7,0), the original stayed: "
                    + "\(String(describing: layerOffset(document, 1))) "
                    + "\(String(describing: layerOffset(document, 0)))")
        if depth == 8 {
            try require(document.doc?.layerPixels(0) == base.layerPixels(0),
                        "\(tag) the original's pixels are the base's")
        }
        try require(document.layerSelection.all == [1] && document.layerSelection.primary == 1,
                    "\(tag) the copy is selected: \(document.layerSelection.all)")
        try require(document.undoManager?.undoActionName == "Transform Copy",
                    "\(tag) one \"Transform Copy\" step: \(document.undoManager?.undoActionName ?? "-")")
        try require(steps.count == 1 && steps.first?.tool == "transform_layers"
                    && steps.first?.arguments["duplicate"] as? Bool == true,
                    "\(tag) one transform_layers duplicate step: "
                    + "\(steps.map { "\($0.tool) \($0.arguments)" })")
        // The matrix step records the session's pivot, read before the
        // commit, so a relative replay keeps the transform about it.
        let recordedPivot = (steps.first?.arguments["pivot"] as? [Any])?.compactMap { $0 as? Double }
        try require(recordedPivot == [Double(pivot.x), Double(pivot.y)],
                    "\(tag) the step records the session pivot \(pivot): "
                    + "\(String(describing: steps.first?.arguments["pivot"]))")
        document.undoManager?.undo()
        try require(document.doc === base && document.layerSelection.all == [0],
                    "\(tag) one undo gives the base and selection [0]")

        // The MCP twin.
        let reply = try agentOK("transform_layers", document,
                                ["layers": [0], "matrix": [1, 0, 0, 1, 7, 0],
                                 "duplicate": true, "sampler": sampler])
        try require((reply["copies"] as? [Int]) == [1] && reply["duplicate"] as? Bool == true,
                    "\(tag) transform_layers duplicate answers copies [1]: \(reply)")
        try require(document.doc?.layerCount == 2 && layerOffset(document, 1) == [7, 0]
                    && layerOffset(document, 0) == [0, 0] && document.layerSelection.all == [1]
                    && document.undoManager?.undoActionName == "Transform Copy",
                    "\(tag) MCP gives the same state")
        document.undoManager?.undo()
        try require(document.doc === base, "\(tag) undo after MCP")

        // A rotation: the commit equals MCP given the session's matrix.
        controller.freeTransformCopy(nil)
        controller.transformDegrees = 30
        guard let matrix = controller.transformSession?.transform.matrix else {
            throw ProbeFailure(description: "\(tag) no rotation session")
        }
        try require(controller.commitTransformSession(), "\(tag) the rotation committed")
        let ui = try copyState(document, depth: depth)
        document.undoManager?.undo()
        try require(document.doc === base, "\(tag) undo after the rotation")
        _ = try agentOK("transform_layers", document,
                        ["layers": [0], "matrix": matrixArgument(matrix),
                         "duplicate": true, "sampler": sampler])
        let agent = try copyState(document, depth: depth)
        try require(ui == agent, "\(tag) the rotation equals MCP: \(ui) vs \(agent)")
        document.undoManager?.undo()
        try require(document.doc === base, "\(tag) undo after the MCP rotation")

        // Esc and an identity Return change nothing.
        let canUndo = document.undoManager?.canUndo == true
        controller.freeTransformCopy(nil)
        controller.endTransformSession()
        try require(document.doc === base && controller.transformSession == nil
                    && (document.undoManager?.canUndo == true) == canUndo,
                    "\(tag) Esc leaves the base and no step")
        controller.freeTransformCopy(nil)
        try require(controller.commitTransformSession(), "\(tag) an identity Return answers true")
        try require(document.doc === base && controller.transformSession == nil
                    && (document.undoManager?.canUndo == true) == canUndo,
                    "\(tag) an identity Return leaves the base and no step")
        let identity = try agentOK("transform_layers", document,
                                   ["layers": [0], "matrix": [1, 0, 0, 1, 0, 0], "duplicate": true])
        try require(identity["changed"] as? Bool == false && (identity["copies"] as? [Int]) == []
                    && document.doc === base,
                    "\(tag) an identity MCP duplicate changes nothing: \(identity)")

        // A Position lock refuses both.
        _ = try agentOK("set_layer_lock", document, ["layer": 0, "locks": ["position"]])
        guard let locked = document.doc else { throw ProbeFailure(description: "\(tag) lock") }
        let alerts = AutomationAlerts.recorded.count
        controller.freeTransformCopy(nil)
        try require(controller.transformSession == nil && document.doc === locked,
                    "\(tag) a Position lock opens nothing")
        try require(AutomationAlerts.recorded.count == alerts + 1,
                    "\(tag) the lock refusal is one alert: \(AutomationAlerts.recorded.suffix(2))")
        let refused = try agentCall("transform_layers", document,
                                    ["layers": [0], "matrix": [1, 0, 0, 1, 7, 0], "duplicate": true])
        try require(refused.isError && refused.text.lowercased().contains("position"),
                    "\(tag) MCP duplicate refuses naming Position: \(refused.text)")
        document.undoManager?.undo()
        try require(document.doc === base, "\(tag) undo the lock")

        if depth == 8 {
            // With two layers selected and layer 0 active, the ACTIVE
            // layer's copy (index 1, directly above it) stays the primary, in
            // the UI and over MCP.
            _ = try agentOK("new_layer", document, [:])
            guard let pair = document.doc, pair.layerCount == 2 else {
                throw ProbeFailure(description: "\(tag) new_layer for the pair")
            }
            controller.setSelectedLayers(LayerSelection(primary: 0, others: [0, 1]))
            try require(document.activeLayerIndex == 0, "\(tag) layer 0 is active")
            controller.freeTransformCopy(nil)
            controller.transformSession?.transform.translation = CGVector(dx: 7, dy: 0)
            try require(controller.commitTransformSession(), "\(tag) the pair's ⌥⌘T committed")
            try require(document.doc?.layerCount == 4 && document.layerSelection.primary == 1,
                        "\(tag) UI: the active layer's copy is the primary: "
                        + "\(document.layerSelection.primary) of \(document.layerSelection.all)")
            document.undoManager?.undo()
            try require(document.doc === pair, "\(tag) undo the pair's ⌥⌘T")
            controller.setSelectedLayers(LayerSelection(primary: 0, others: [0, 1]))
            _ = try agentOK("transform_layers", document,
                            ["layers": [0, 1], "matrix": [1, 0, 0, 1, 7, 0], "duplicate": true])
            try require(document.doc?.layerCount == 4 && document.layerSelection.primary == 1,
                        "\(tag) MCP: the active layer's copy is the primary: "
                        + "\(document.layerSelection.primary) of \(document.layerSelection.all)")
            document.undoManager?.undo()
            try require(document.doc === pair, "\(tag) undo the pair's MCP copy")
            document.undoManager?.undo()
            try require(document.doc === base, "\(tag) undo new_layer")

            // A text layer's ⌥⌘T keeps the copy editable with no alert,
            // and ⌥⇧⌘T then adds a second described copy.
            _ = try agentOK("add_text_layer", document, ["text": "Probe", "x": 4, "y": 4])
            guard let withText = document.doc,
                  let text = (0..<withText.layerCount).first(where: {
                      document.layerDescribesSource($0) }) else {
                throw ProbeFailure(description: "\(tag) add_text_layer made no text layer")
            }
            controller.setSelectedLayers(.single(text))
            let alerts = AutomationAlerts.recorded.count
            controller.freeTransformCopy(nil)
            try require(controller.transformSession?.duplicate != nil,
                        "\(tag) ⌥⌘T opens over a text layer")
            controller.transformSession?.transform.translation = CGVector(dx: 7, dy: 0)
            try require(controller.commitTransformSession(), "\(tag) the text copy committed")
            try require(document.doc?.layerCount == withText.layerCount + 1
                        && document.layerDescribesSource(text + 1)
                        && document.layerDescribesSource(text)
                        && document.layerSelection.primary == text + 1
                        && AutomationAlerts.recorded.count == alerts,
                        "\(tag) ⌥⌘T keeps the text copy described with no alert: "
                        + "\(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
            guard let once = document.doc else { throw ProbeFailure(description: "\(tag) copy") }
            controller.transformAgainWithCopy(nil)
            try require(document.doc?.layerCount == withText.layerCount + 2
                        && document.layerDescribesSource(text + 2)
                        && document.undoManager?.undoActionName
                            == RepeatableTransform.actionName(duplicate: true)
                        && AutomationAlerts.recorded.count == alerts,
                        "\(tag) ⌥⇧⌘T adds a second described copy with no alert: "
                        + "\(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
            document.undoManager?.undo()
            try require(document.doc === once, "\(tag) undo the ⌥⇧⌘T copy")
            document.undoManager?.undo()
            try require(document.doc === withText, "\(tag) undo the text ⌥⌘T")

            // A recorded 15° ⌥⌘T replayed over MCP keeps the copy's description
            // and lands the UI's pixels (the replayed step composes too).
            controller.setSelectedLayers(.single(text))
            controller.freeTransformCopy(nil)
            controller.transformDegrees = 15
            ActionRecorder.shared.start()
            let turned = controller.commitTransformSession()
            let recorded = ActionRecorder.shared.stop()
            let uiTurn = describedCopyState(document, text + 1)
            try require(turned && recorded.count == 1 && recorded.first?.tool == "transform_layers"
                        && document.layerDescribesSource(text + 1)
                        && AutomationAlerts.recorded.count == alerts,
                        "\(tag) the 15° text ⌥⌘T keeps the copy described: "
                        + "\(recorded.map { "\($0.tool) \($0.arguments)" })")
            document.undoManager?.undo()
            try require(document.doc === withText, "\(tag) undo the 15° text ⌥⌘T")
            controller.setSelectedLayers(.single(text))
            let replay = ActionPlayer.runSingle(recorded[0], on: document, undoName: "Transform Replay")
            try require(replay.ok && document.layerDescribesSource(text + 1)
                        && describedCopyState(document, text + 1) == uiTurn,
                        "\(tag) the replayed ⌥⌘T keeps the description and the UI's pixels: "
                        + "\(describedCopyState(document, text + 1)) vs \(uiTurn)")
            document.undoManager?.undo()
            try require(document.doc === withText, "\(tag) undo the replay")

            // MCP: a matrix keeps the copy described; a true perspective needs
            // rasterize_artwork, which reports the copy.
            controller.setSelectedLayers(.single(text))
            let kept = try agentOK("transform_layers", document,
                                   ["layers": [text], "matrix": [1, 0, 0, 1, 7, 0],
                                    "duplicate": true])
            try require((kept["rasterized_layers"] as? [Int]) == []
                        && document.layerDescribesSource(text + 1),
                        "\(tag) MCP duplicate keeps the text copy described: \(kept)")
            document.undoManager?.undo()
            try require(document.doc === withText, "\(tag) undo the MCP text copy")
            guard let info = withText.layerInfo(text) else {
                throw ProbeFailure(description: "\(tag) text layer info")
            }
            let (x, y) = (Double(info.offsetX), Double(info.offsetY))
            let (w, h) = (Double(info.width), Double(info.height))
            let trapezoid: [String: Any] = [
                "layers": [text], "duplicate": true,
                "source_rect": [x, y, w, h],
                "corners": [[x, y], [x + w, y], [x + w - 2, y + h], [x + 2, y + h]],
            ]
            let perspectiveRefused = try agentCall("transform_layers", document, trapezoid)
            try require(perspectiveRefused.isError
                        && perspectiveRefused.text.contains(AgentServer.truePerspectiveRefusal)
                        && document.doc === withText,
                        "\(tag) a true-perspective text copy needs rasterize_artwork: "
                        + "\(perspectiveRefused.text)")
            let authorized = try agentOK("transform_layers", document,
                                         trapezoid.merging(["rasterize_artwork": true]) { $1 })
            try require((authorized["rasterized_layers"] as? [Int]) == [text + 1]
                        && document.doc?.layerMeta(text + 1) == nil
                        && document.layerDescribesSource(text),
                        "\(tag) rasterize_artwork rasterizes only the copy: \(authorized)")
            document.undoManager?.undo()
            try require(document.doc === withText, "\(tag) undo the authorized copy")
        } else {
            // Two selected layers: the 16-bit gate opens nothing.
            _ = try agentOK("new_layer", document, [:])
            guard let doc = document.doc, doc.layerCount == 2 else {
                throw ProbeFailure(description: "\(tag) new_layer")
            }
            controller.setSelectedLayers(LayerSelection(primary: 1, others: [0, 1]))
            controller.freeTransformCopy(nil)
            try require(controller.transformSession == nil && document.doc === doc,
                        "\(tag) two selected layers open nothing at 16 bits")

            // A lone text layer's ⌥⌘T at 15° keeps the copy's artwork.
            _ = try agentOK("add_text_layer", document, ["text": "Probe", "x": 4, "y": 4])
            guard let withText = document.doc,
                  let text = (0..<withText.layerCount).first(where: {
                      withText.hasArtworkDescription($0) }) else {
                throw ProbeFailure(description: "\(tag) add_text_layer made no text layer")
            }
            controller.setSelectedLayers(.single(text))
            let alerts = AutomationAlerts.recorded.count
            controller.freeTransformCopy(nil)
            controller.transformDegrees = 15
            try require(controller.commitTransformSession(), "\(tag) the text copy committed")
            try require(document.doc?.layerCount == withText.layerCount + 1
                        && document.doc?.hasArtworkDescription(text + 1) == true
                        && document.doc?.hasArtworkDescription(text) == true
                        && AutomationAlerts.recorded.count == alerts,
                        "\(tag) ⌥⌘T at 15° keeps the text copy's artwork: "
                        + "\(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
            document.undoManager?.undo()
            try require(document.doc === withText, "\(tag) undo the text ⌥⌘T")
        }
    }
    print("PASS: free transform copy layers — ⌥⌘T with no selection opens over the duplicate, "
        + "commits one \"Transform Copy\" step (one recorded transform_layers duplicate) with the "
        + "copy selected, equals MCP for a translation and a rotation, Esc and identity change "
        + "nothing, a Position lock refuses, at 8 and 16 bits; with two layers selected "
        + "the active layer's copy stays the primary in the UI and over MCP; a text "
        + "layer's copy stays editable — ⌥⌘T and ⌥⇧⌘T with no alert, a recorded ⌥⌘T replayed "
        + "over MCP with the UI's pixels, MCP needing rasterize_artwork only for a true "
        + "perspective at 8 bits, and the 16-bit copy keeping its artwork")
}

/// A 64×64 document opaque only in (20, 20, 10, 10), at 8 or 16 bits.
private func sourceBoxFixture(depth: Int) throws -> ImageDocument {
    let document: ImageDocument
    if depth == 8 {
        var bytes = [UInt8](repeating: 0, count: 64 * 64 * 4)
        for y in 20..<30 {
            for x in 20..<30 {
                let p = (y * 64 + x) * 4
                bytes[p] = 200
                bytes[p + 1] = 100
                bytes[p + 2] = 50
                bytes[p + 3] = 255
            }
        }
        guard let image = RasterImage.from(rgba: bytes, width: 64, height: 64),
              let made = ImageDocument.makeUntitled(with: image) else {
            throw ProbeFailure(description: "Source-box fixture")
        }
        document = made
        document.updateChangeCount(.changeCleared)
    } else {
        var samples = [UInt16](repeating: 0, count: 64 * 64 * 4)
        for y in 20..<30 {
            for x in 20..<30 {
                let p = (y * 64 + x) * 4
                samples[p] = 51400
                samples[p + 1] = 25700
                samples[p + 2] = 12850
                samples[p + 3] = 65535
            }
        }
        let source = try RasterDocument.fromRawRGBA16(samples, width: 64, height: 64,
                                                      profile: RasterProfile.builtin(.sRGB))
        document = ImageDocument()
        document.fileType = ImageDocument.nativeTypeIdentifier
        document.installEditingState(EditingState(document: source, selection: SelectionHistoryState(),
            layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: 90, embedColorProfile: true,
            stripMetadata: false, exportBitDepth: 16, exportDither: "none"), markSaved: true)
    }
    document.undoManager?.groupsByEvent = false
    NSDocumentController.shared.addDocument(document)
    document.makeWindowControllers()
    if let window = document.windowControllers.first?.window {
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderFront(nil)
    }
    document.undoManager?.removeAllActions()
    return document
}

func verifyDuplicateSnapsToSource() throws {
    let store = ToolOptionsStore.shared
    let savedView = store.view
    let savedMove = store.move
    defer {
        store.view = savedView
        store.move = savedMove
    }
    store.view.snapEnabled = true
    store.view.snapTargets = SnapTarget.layers.rawValue
    store.move.snapToLayers = true
    let source = CGRect(x: 20, y: 20, width: 10, height: 10)
    for depth in [8, 16] {
        let tag = "[\(depth)]"
        let document = try sourceBoxFixture(depth: depth)
        defer { document.close() }
        let controller = try editor(document)
        controller.selectTool(.move)
        controller.canvas.enclosingScrollView?.magnification = 1
        guard let base = document.doc else { throw ProbeFailure(description: "\(tag) fixture") }
        // A typed document answers the same content box.
        let rows = try agentOK("get_document", document)["layers"] as? [[String: Any]]
        let row = rows?.first
        let content = ["content_x", "content_y", "content_width", "content_height"]
            .map { (row?[$0] as? NSNumber)?.intValue ?? -1 }
        try require(content == [20, 20, 10, 10],
                    "\(tag) get_document row 0 reports the content box: \(content)")

        controller.moveDidBegin(at: CGPoint(x: 25, y: 25), modifiers: [.option])
        let pressRects = controller.makeSnapEngine(for: .move).layerRects
        try require(pressRects == [source],
                    "\(tag) at the press the source is a target: \(pressRects)")
        // 2 px: every nearest candidate (edges 20/30, centre 25) pulls back
        // by 2, so the box lands on the source.
        let pull = controller.snapMoveDelta(2, 0, [])
        try require(pull == (0, 0), "\(tag) a 2 px drag snaps back onto the source: \(pull)")
        controller.moveDidUpdate(12, 0, [])
        let tickRects = controller.makeSnapEngine(for: .move).layerRects
        try require(tickRects == [source],
                    "\(tag) mid-drag the source stays the only target (the copy is excluded): \(tickRects)")
        controller.moveDidFinish()
        let landed = [10, 0]
        try require(document.doc?.layerCount == 2 && layerOffset(document, 1) == landed,
                    "\(tag) the copy landed at \(landed): "
                    + "\(String(describing: layerOffset(document, 1)))")
        document.undoManager?.undo()
        try require(document.doc === base, "\(tag) undo the dragged copy")

        guard let duplicated = try base.duplicateLayers([0]) else {
            throw ProbeFailure(description: "\(tag) duplicateLayers")
        }
        let live = MoveDuplicateLayers.snapExclusion(originals: [0], copies: [1], base: base,
                                                     in: duplicated)
        let pressed = MoveDuplicateLayers.snapExclusion(originals: [0], copies: [1], base: base,
                                                        in: base)
        try require(live == [1] && pressed.isEmpty,
                    "\(tag) snapExclusion: live \(live), at the base \(pressed)")

        controller.freeTransformCopy(nil)
        try require(controller.transformSession?.duplicate != nil, "\(tag) ⌥⌘T opened")
        let boxRects = controller.makeSnapEngine(for: .transform).layerRects
        try require(boxRects == [source],
                    "\(tag) the ⌥⌘T box snaps to its source: \(boxRects)")
        controller.endTransformSession()
    }
    print("PASS: duplicate snaps to source — get_document reports the content box, an ⌥-drag "
        + "and the ⌥⌘T box keep the source as a target, exclude the live copy and snap back "
        + "onto it, at 8 and 16 bits")
}

/// The largest alpha of a visible geometry-commit panel seen by a 10 ms poll
/// while `body` runs; -1 when none is seen. The worker queue is held for
/// 0.1 s first (a synthetic duration), so the wait spans several polls while
/// staying well under `DocumentJobs.panelDelay`.
private func revealReading(_ body: () throws -> Void) rethrows -> CGFloat {
    let titles: Set<String> = ["Move Layer", "Transform Layer", "Transform Copy", "Crop"]
    var largest: CGFloat = -1
    let timer = Timer(timeInterval: 0.01, repeats: true) { _ in
        for window in NSApp.windows where window.isVisible && titles.contains(window.title) {
            largest = max(largest, window.alphaValue)
        }
    }
    RunLoop.main.add(timer, forMode: .common)
    RunLoop.main.add(timer, forMode: .modalPanel)
    defer { timer.invalidate() }
    BackgroundWork.queue.async { Thread.sleep(forTimeInterval: 0.1) }
    try body()
    return largest
}

func verifyGeometryCommitReveal() throws {
    var readings: [String: CGFloat] = [:]
    // 16 bits: Move, ⌘T and Crop.
    do {
        let document = try precisionFixture(width: 64, height: 48)
        defer { document.close() }
        let controller = try editor(document)
        controller.selectTool(.move)
        guard var before = document.doc else { throw ProbeFailure(description: "16-bit fixture") }
        readings["16 Move Layer"] = revealReading {
            controller.moveDidBegin(at: CGPoint(x: 10, y: 10), modifiers: [])
            controller.moveDidUpdate(3, 0, [.control])
            controller.moveDidFinish()
        }
        try require(document.doc !== before, "[16] the Move published")
        guard let moved = document.doc else { throw ProbeFailure(description: "16-bit move") }
        before = moved
        controller.freeTransform(nil)
        controller.transformDegrees = 10
        guard let gesture = controller.transformSession?.precisionGesture else {
            throw ProbeFailure(description: "16-bit ⌘T has no gesture")
        }
        try pumpUntil({ !gesture.isPending }, seconds: 30)
        readings["16 Transform Layer"] = revealReading { controller.commitPendingTransform() }
        try require(document.doc !== before && !controller.isTransforming, "[16] the ⌘T published")
        guard let transformed = document.doc else { throw ProbeFailure(description: "16-bit ⌘T") }
        before = transformed
        controller.beginCropSession()
        controller.cropSession?.angle = 3
        guard let crop = controller.cropSession?.precisionGesture else {
            throw ProbeFailure(description: "16-bit crop has no gesture")
        }
        try pumpUntil({ !crop.isPending }, seconds: 30)
        readings["16 Crop"] = revealReading { controller.commitCropSession() }
        try require(document.doc !== before, "[16] the Crop published")
        controller.endCropSession()
    }
    // 8 bits: ⌘T and ⌥⌘T with no selection (TransformWork.wait).
    do {
        let document = try fixture(width: 64, height: 64)
        defer { document.close() }
        let controller = try editor(document)
        guard let base = document.doc else { throw ProbeFailure(description: "8-bit fixture") }
        controller.freeTransform(nil)
        controller.transformDegrees = 10
        readings["8 Transform Layer"] = revealReading { controller.commitPendingTransform() }
        try require(document.doc !== base && !controller.isTransforming, "[8] the ⌘T published")
        guard let transformed = document.doc else { throw ProbeFailure(description: "8-bit ⌘T") }
        controller.freeTransformCopy(nil)
        controller.transformDegrees = 10
        readings["8 Transform Copy"] = revealReading { controller.commitPendingTransform() }
        try require(document.doc !== transformed && !controller.isTransforming,
                    "[8] the ⌥⌘T published")
    }
    for (name, value) in readings.sorted(by: { $0.key < $1.key }) {
        try require(value == 0, "[\(name)] the panel was seen and stayed hidden (0): \(value)")
    }
    print("PASS: geometry commit reveal — the 16-bit Move, ⌘T and Crop and the 8-bit ⌘T and "
        + "⌥⌘T commit waits keep their panel hidden for a 0.1 s job: "
        + readings.sorted(by: { $0.key < $1.key }).map { "\($0.key)=\($0.value)" }
            .joined(separator: ", "))
}
