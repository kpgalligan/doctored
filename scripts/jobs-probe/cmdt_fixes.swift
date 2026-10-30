import AppKit

// A lone 16-bit text layer keeps its description under a
// ⌘T affine or parallelogram (and a confirmed true perspective records
// `distort_layer` with `rasterize_artwork`, whose replay lands the UI's
// pixels at 8 and 16 bits), and a 16-bit true perspective asks once for the
// whole set.
// Every case calls methods directly, so none needs a key window.

private func pixelReadings(_ document: ImageDocument, _ points: [(Int, Int)]) throws -> [String] {
    try points.map { "\(NSDictionary(dictionary: try agentOK("sample_pixel", document, ["x": $0.0, "y": $0.1])))" }
}

/// Six points inside `info`'s box, clamped to the canvas.
private func boxPoints(_ info: RasterDocument.LayerInfo?, canvas: RasterDocument) -> [(Int, Int)] {
    guard let info else { return [] }
    let fractions: [(Double, Double)] = [(0.25, 0.25), (0.5, 0.5), (0.75, 0.75),
                                         (0.25, 0.75), (0.75, 0.25), (0.5, 0.25)]
    return fractions.map { fx, fy in
        (min(max(info.offsetX + Int(Double(info.width) * fx), 0), canvas.width - 1),
         min(max(info.offsetY + Int(Double(info.height) * fy), 0), canvas.height - 1))
    }
}

/// A layer's description, rect and six readings inside `points` (plus its
/// pixels at 8 bits).
private func layerState(_ document: ImageDocument, _ layer: Int,
                        _ points: [(Int, Int)]) throws -> [String] {
    let info = document.doc?.layerInfo(layer)
    var state = [document.doc?.layerMeta(layer) ?? "-",
                 "\(info.map { [$0.offsetX, $0.offsetY, $0.width, $0.height] } ?? [])"]
    if document.doc?.isHighDepth == false {
        let pixels = document.doc?.layerPixels(layer) ?? []
        state.append("\(pixels.count):\(pixels.reduce(0) { ($0 &* 31) &+ Int($1) })")
    }
    return state + (try pixelReadings(document, points))
}

private func textFixture(depth: Int) throws -> (ImageDocument, RasterDocument, Int) {
    let document = depth == 8
        ? try fixture(width: 64, height: 64) : try precisionFixture(width: 64, height: 48)
    _ = try agentOK("add_text_layer", document, ["text": "Probe", "x": 4, "y": 4])
    guard let withText = document.doc,
          let text = (0..<withText.layerCount).first(where: {
              withText.hasArtworkDescription($0) }) else {
        document.close()
        throw ProbeFailure(description: "[\(depth)] add_text_layer made no text layer")
    }
    return (document, withText, text)
}

/// A confirmed lone true-perspective ⌘T on a text layer prompts once, records
/// `distort_layer` with `rasterize_artwork: true`, and replaying that step
/// succeeds with the UI's pixels.
private func verifyPerspectiveRecordReplay(depth: Int) throws {
    defer { _ = ActionRecorder.shared.stop() }
    let tag = "[\(depth) perspective replay]"
    let (document, withText, text) = try textFixture(depth: depth)
    defer { document.close() }
    let controller = try editor(document)
    let points = boxPoints(withText.layerInfo(text), canvas: withText)
    controller.setSelectedLayers(.single(text))
    let alerts = AutomationAlerts.recorded.count
    controller.freeTransform(nil)
    controller.transformSession?.transform.cornerOffsets = [
        .zero, .zero, CGVector(dx: 6, dy: 4), .zero]
    ActionRecorder.shared.start()
    let committed = controller.commitTransformSession()
    let recorded = ActionRecorder.shared.stop()
    try require(committed && document.doc !== withText, "\(tag) the ⌘T published")
    try require(AutomationAlerts.recorded.count == alerts + 1
                && AutomationAlerts.recorded.last?.messageText == "Rasterize text layer?",
                "\(tag) exactly one \"Rasterize text layer?\" alert: "
                + "\(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
    try require(document.doc?.layerMeta(text) == nil, "\(tag) the text layer was rasterized")
    try require(recorded.count == 1 && recorded.first?.tool == "distort_layer"
                && recorded.first?.arguments["rasterize_artwork"] as? Bool == true,
                "\(tag) one distort_layer step with rasterize_artwork: "
                + "\(recorded.map { "\($0.tool) \($0.arguments)" })")
    let ui = try layerState(document, text, points)
    document.undoManager?.undo()
    try require(document.doc === withText, "\(tag) undo the ⌘T")
    controller.setSelectedLayers(.single(text))
    let replay = ActionPlayer.runSingle(recorded[0], on: document, undoName: "Transform Replay")
    let replayed = try layerState(document, text, points)
    try require(replay.ok && document.doc?.layerMeta(text) == nil && replayed == ui,
                "\(tag) the replayed step succeeds with the UI's pixels: ok \(replay.ok), "
                + "\(replayed) vs \(ui)")
    document.undoManager?.undo()
    try require(document.doc === withText, "\(tag) undo the replay")
}

func verifyPrecisionArtworkTransform() throws {
    let tag = "[16]"
    let (document, withText, text) = try textFixture(depth: 16)
    defer { document.close() }
    let controller = try editor(document)
    let sampler = EditorViewController.samplerAgentName(controller.transformSampler)
    let points = boxPoints(withText.layerInfo(text), canvas: withText)
    guard let info = withText.layerInfo(text) else {
        throw ProbeFailure(description: "\(tag) text layer info")
    }

    // UI 15°: no alert, one "Transform Layer" step, the artwork kept, equal
    // to MCP transform_layers with the session's matrix.
    controller.setSelectedLayers(.single(text))
    var alerts = AutomationAlerts.recorded.count
    let steps = document.history.entries.count
    controller.freeTransform(nil)
    controller.transformDegrees = 15
    guard let matrix = controller.transformSession?.transform.matrix else {
        throw ProbeFailure(description: "\(tag) ⌘T opened no session")
    }
    try require(controller.commitTransformSession() && document.doc !== withText,
                "\(tag) the 15° ⌘T published")
    try require(AutomationAlerts.recorded.count == alerts
                && document.undoManager?.undoActionName == "Transform Layer"
                && document.history.entries.count == steps + 1
                && document.doc?.hasArtworkDescription(text) == true,
                "\(tag) the 15° ⌘T is one \"Transform Layer\" step with no alert and keeps the "
                + "artwork: \(document.undoManager?.undoActionName ?? "-") "
                + "\(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
    let ui = try layerState(document, text, points)
    document.undoManager?.undo()
    try require(document.doc === withText, "\(tag) undo the 15° ⌘T")
    controller.setSelectedLayers(.single(text))
    _ = try agentOK("transform_layers", document,
                    ["layers": [text],
                     "matrix": [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty]
                        .map(Double.init),
                     "sampler": sampler])
    let agent = try layerState(document, text, points)
    try require(ui == agent, "\(tag) the 15° ⌘T equals transform_layers: \(ui) vs \(agent)")
    document.undoManager?.undo()
    try require(document.doc === withText, "\(tag) undo transform_layers")

    // A translation and a shear keep the artwork with no alert.
    let cases: [(String, (inout LayerTransform) -> Void)] = [
        ("(7,0) translation", { $0.translation = CGVector(dx: 7, dy: 0) }),
        ("shear", { $0.cornerOffsets = [CGVector(dx: 4, dy: 0), CGVector(dx: 4, dy: 0),
                                        .zero, .zero] }),
    ]
    for (name, change) in cases {
        controller.setSelectedLayers(.single(text))
        alerts = AutomationAlerts.recorded.count
        controller.freeTransform(nil)
        guard var transform = controller.transformSession?.transform else {
            throw ProbeFailure(description: "\(tag) \(name): ⌘T opened no session")
        }
        change(&transform)
        controller.transformSession?.transform = transform
        try require(controller.commitTransformSession() && document.doc !== withText,
                    "\(tag) the \(name) ⌘T published")
        try require(AutomationAlerts.recorded.count == alerts
                    && document.undoManager?.undoActionName == "Transform Layer"
                    && document.doc?.hasArtworkDescription(text) == true,
                    "\(tag) the \(name) ⌘T keeps the artwork with no alert: "
                    + "\(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
        document.undoManager?.undo()
        try require(document.doc === withText, "\(tag) undo the \(name) ⌘T")
    }

    // A true perspective asks once, publishes and rasterizes; its recorded
    // step replays (and at 8 bits too).
    try verifyPerspectiveRecordReplay(depth: 16)
    try verifyPerspectiveRecordReplay(depth: 8)

    // MCP: these paths keep the description.
    controller.setSelectedLayers(.single(text))
    _ = try agentOK("transform_layer", document, ["layer": text, "rotate": 15])
    try require(document.doc?.hasArtworkDescription(text) == true,
                "\(tag) transform_layer rotate 15 keeps the artwork")
    document.undoManager?.undo()
    try require(document.doc === withText, "\(tag) undo transform_layer")
    let (x, y) = (Double(info.offsetX), Double(info.offsetY))
    let (w, h) = (Double(info.width), Double(info.height))
    let shear = try agentOK("distort_layer", document,
                            ["layer": text,
                             "corners": [[x + 4, y], [x + w + 4, y], [x + w, y + h], [x, y + h]]])
    try require(shear["rasterized"] as? Bool == false
                && document.doc?.hasArtworkDescription(text) == true,
                "\(tag) distort_layer's shear answers rasterized:false and keeps the artwork: "
                + "\(shear)")
    document.undoManager?.undo()
    try require(document.doc === withText, "\(tag) undo the distort_layer shear")
    let trapezoid = try agentCall("distort_layer", document,
                                  ["layer": text,
                                   "corners": [[x, y], [x + w, y], [x + w - 2, y + h],
                                               [x + 2, y + h]]])
    try require(trapezoid.isError && trapezoid.text.contains("rasterize_artwork")
                && document.doc === withText,
                "\(tag) a trapezoid without rasterize_artwork refuses and changes nothing: "
                + "\(trapezoid.text)")
    print("PASS: precision artwork transform — a lone 16-bit text layer's ⌘T at 15° is one "
        + "\"Transform Layer\" step with no alert that keeps the artwork and equals "
        + "transform_layers; a translation and a shear keep it too; a true perspective asks "
        + "once, rasterizes and records distort_layer with rasterize_artwork, whose replay lands "
        + "the UI's pixels at 8 and 16 bits; MCP transform_layer and a distort_layer shear keep "
        + "the artwork and a trapezoid needs rasterize_artwork")
}

func verifyPerspectiveArtworkSetPrompt() throws {
    let tag = "[16]"
    let document = try precisionFixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    _ = try agentOK("add_text_layer", document, ["text": "A", "x": 4, "y": 4])
    _ = try agentOK("add_text_layer", document, ["text": "B", "x": 30, "y": 20])
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) fixture") }
    let artwork = (0..<base.layerCount).filter { base.hasArtworkDescription($0) }
    guard artwork.count == 2 else {
        throw ProbeFailure(description: "\(tag) two text layers: \(artwork)")
    }
    let (a, b) = (artwork[0], artwork[1])

    // A true perspective asks ONE prompt for the pair.
    controller.setSelectedLayers(LayerSelection(primary: a, others: [a, b]))
    var alerts = AutomationAlerts.recorded.count
    controller.freeTransform(nil)
    controller.transformSession?.transform.cornerOffsets = [
        .zero, .zero, CGVector(dx: 6, dy: 4), .zero]
    try require(controller.commitTransformSession() && document.doc !== base,
                "\(tag) the perspective ⌘T published")
    try require(AutomationAlerts.recorded.count == alerts + 1
                && AutomationAlerts.recorded.last?.messageText
                    == "Rasterize 2 editable artwork layers?",
                "\(tag) exactly one set prompt: "
                + "\(AutomationAlerts.recorded.suffix(3).map(\.messageText))")
    try require(document.doc?.hasArtworkDescription(a) == false
                && document.doc?.hasArtworkDescription(b) == false,
                "\(tag) both text layers were rasterized")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) undo the perspective ⌘T")

    // A 15° ⌘T over the same pair asks nothing and keeps both.
    controller.setSelectedLayers(LayerSelection(primary: a, others: [a, b]))
    alerts = AutomationAlerts.recorded.count
    controller.freeTransform(nil)
    controller.transformDegrees = 15
    try require(controller.commitTransformSession() && document.doc !== base,
                "\(tag) the 15° ⌘T published")
    try require(AutomationAlerts.recorded.count == alerts
                && document.doc?.hasArtworkDescription(a) == true
                && document.doc?.hasArtworkDescription(b) == true,
                "\(tag) the 15° ⌘T asks nothing and keeps both: "
                + "\(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) undo the 15° ⌘T")
    print("PASS: perspective artwork set prompt — a 16-bit true-perspective ⌘T over two text "
        + "layers asks one \"Rasterize 2 editable artwork layers?\" prompt and rasterizes both; "
        + "a 15° ⌘T over the pair asks nothing and keeps both")
}
