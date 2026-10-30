import AppKit

// An 8-bit true-perspective ⌘T / ⇧⌘T / ⌥⌘T over a set asks ONE prompt
// over its described members (text, shape); a declined set
// prompt keeps the session at 8 and 16 bits, and the name list truncates
// after five. An 8-bit ⌥⌘T parallelogram over a text layer replays to
// the UI's copy through the recorded step and `transform_layers`, and the
// 16-bit ⌥⌘T preview tick's duplicate is measured against ⌘T's. Every case
// calls methods directly, so none needs a key window.

private let perspectiveCorners = [CGVector.zero, .zero, CGVector(dx: 6, dy: 4), .zero]

/// The document's text layers, bottom first.
private func textLayers(_ doc: RasterDocument?) -> [Int] {
    guard let doc else { return [] }
    return (0..<doc.layerCount).filter { doc.hasArtworkDescription($0) }
}

/// Open ⌘T (or ⌥⌘T) over `selection`, drag one corner and commit.
private func perspectiveCommit(_ controller: EditorViewController, _ selection: LayerSelection,
                               copy: Bool = false) -> Bool {
    controller.setSelectedLayers(selection)
    if copy { controller.freeTransformCopy(nil) } else { controller.freeTransform(nil) }
    controller.transformSession?.transform.cornerOffsets = perspectiveCorners
    return controller.commitTransformSession()
}

/// The alerts recorded since `count`, as message texts.
private func alertsSince(_ count: Int) -> [String] {
    AutomationAlerts.recorded.dropFirst(count).map(\.messageText)
}

func verifyByteSetPerspectivePrompt() throws {
    let tag = "[8 set prompt]"
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    let controller = try editor(document)
    _ = try agentOK("add_text_layer", document, ["text": "A", "x": 4, "y": 4])
    _ = try agentOK("add_text_layer", document, ["text": "B", "x": 30, "y": 20])
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) fixture") }
    let artwork = textLayers(base)
    guard artwork.count == 2 else {
        throw ProbeFailure(description: "\(tag) two text layers: \(artwork)")
    }
    let (a, b) = (artwork[0], artwork[1])
    let pair = LayerSelection(primary: a, others: [a, b])
    let setMessage = "Rasterize 2 editable artwork layers?"

    // (a) ⌘T over the pair asks once and rasterizes both.
    var alerts = AutomationAlerts.recorded.count
    try require(perspectiveCommit(controller, pair) && document.doc !== base,
                "\(tag) (a) the perspective ⌘T published")
    try require(alertsSince(alerts) == [setMessage],
                "\(tag) (a) exactly one set prompt: \(alertsSince(alerts))")
    try require(document.doc?.layerMeta(a) == nil && document.doc?.layerMeta(b) == nil,
                "\(tag) (a) both text layers were rasterized")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) (a) undo")

    // (b) ⇧⌘T over the pair: the perspective memory survives the undo.
    controller.setSelectedLayers(pair)
    alerts = AutomationAlerts.recorded.count
    controller.transformAgain(nil)
    try require(document.doc !== base, "\(tag) (b) ⇧⌘T published")
    try require(alertsSince(alerts) == [setMessage],
                "\(tag) (b) exactly one set prompt: \(alertsSince(alerts))")
    try require(document.doc?.layerMeta(a) == nil && document.doc?.layerMeta(b) == nil,
                "\(tag) (b) both text layers were rasterized")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) (b) undo")

    // (c) ⌥⌘T over the pair: one prompt about the copies; the originals keep
    // their descriptions.
    alerts = AutomationAlerts.recorded.count
    try require(perspectiveCommit(controller, pair, copy: true) && document.doc !== base,
                "\(tag) (c) the perspective ⌥⌘T published")
    try require(alertsSince(alerts) == [setMessage],
                "\(tag) (c) exactly one set prompt: \(alertsSince(alerts))")
    let copies = document.layerSelection.all
    let described = textLayers(document.doc)
    try require(copies.count == 2 && copies.allSatisfy({ document.doc?.layerMeta($0) == nil })
                && described.count == 2 && Set(described).isDisjoint(with: copies),
                "\(tag) (c) both copies rasterized, both originals described: copies "
                + "\(copies.sorted()), described \(described)")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) (c) undo")

    // (d) One described member beside a plain layer: today's lone prompt,
    // with the set reason.
    alerts = AutomationAlerts.recorded.count
    try require(perspectiveCommit(controller, LayerSelection(primary: a, others: [0, a]))
                && document.doc !== base, "\(tag) (d) the perspective ⌘T published")
    try require(alertsSince(alerts) == ["Rasterize text layer?"]
                && AutomationAlerts.recorded.last?.informativeText
                    .contains(EditorViewController.setRasterizeReason) == true,
                "\(tag) (d) one \"Rasterize text layer?\" with the set reason: "
                + "\(AutomationAlerts.recorded.dropFirst(alerts).map(\.informativeText))")
    try require(document.doc?.layerMeta(a) == nil, "\(tag) (d) the text layer was rasterized")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) (d) undo")
    print("PASS: byte set perspective prompt — an 8-bit true-perspective ⌘T, ⇧⌘T and ⌥⌘T "
        + "over two text layers each ask one \"Rasterize 2 editable artwork layers?\" prompt; "
        + "one described member keeps its lone prompt with the set reason")
}

/// A perspective ⌘T over two text layers whose prompt is declined: nothing
/// published, the session open, the decline note as the sentence.
private func verifySetPromptDecline(depth: Int) throws {
    let tag = "[\(depth) decline]"
    let document = depth == 8
        ? try fixture(width: 64, height: 64) : try precisionFixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    _ = try agentOK("add_text_layer", document, ["text": "A", "x": 4, "y": 4])
    _ = try agentOK("add_text_layer", document, ["text": "B", "x": 30, "y": 20])
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) fixture") }
    let artwork = textLayers(base)
    guard artwork.count == 2 else {
        throw ProbeFailure(description: "\(tag) two text layers: \(artwork)")
    }
    controller.setSelectedLayers(LayerSelection(primary: artwork[0], others: Set(artwork)))
    controller.freeTransform(nil)
    controller.transformSession?.transform.cornerOffsets = perspectiveCorners
    guard let method = class_getInstanceMethod(NSAlert.self, #selector(NSAlert.runModal)) else {
        throw ProbeFailure(description: "\(tag) NSAlert.runModal missing")
    }
    var intercepted: [String] = []
    let decline: @convention(block) (AnyObject) -> Int = { alert in
        intercepted.append((alert as? NSAlert)?.messageText ?? "-")
        return NSApplication.ModalResponse.alertSecondButtonReturn.rawValue
    }
    let outcome: (committed: Bool, sentence: String?)
    do {
        let replacement = imp_implementationWithBlock(decline)
        let original = method_setImplementation(method, replacement)
        defer {
            method_setImplementation(method, original)
            imp_removeBlock(replacement)
        }
        outcome = SessionCommitRefusal.capturing { controller.commitTransformSession() }
    }
    try require(!outcome.committed, "\(tag) the commit answered false")
    try require(intercepted == ["Rasterize 2 editable artwork layers?"],
                "\(tag) exactly one set prompt: \(intercepted)")
    try require(outcome.sentence == "The prompt to rasterize 2 editable artwork layers was "
                    + "cancelled in the document window.",
                "\(tag) the decline note: \(outcome.sentence ?? "nil")")
    try require(document.doc === base && controller.isTransforming,
                "\(tag) nothing published and the session stays open")
    controller.endTransformSession()
}

func verifySetPromptDeclineAndTruncation() throws {
    try verifySetPromptDecline(depth: 8)
    try verifySetPromptDecline(depth: 16)

    let tag = "[8 truncation]"
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    let controller = try editor(document)
    for (index, point) in [(2, 2), (22, 2), (42, 2), (2, 32), (22, 32), (42, 32)].enumerated() {
        _ = try agentOK("add_text_layer", document,
                        ["text": "T\(index)", "x": point.0, "y": point.1])
    }
    guard let base = document.doc else { throw ProbeFailure(description: "\(tag) fixture") }
    let artwork = textLayers(base)
    guard artwork.count == 6 else {
        throw ProbeFailure(description: "\(tag) six text layers: \(artwork)")
    }
    let alerts = AutomationAlerts.recorded.count
    try require(perspectiveCommit(controller,
                                  LayerSelection(primary: artwork[0], others: Set(artwork)))
                && document.doc !== base, "\(tag) the perspective ⌘T published")
    let informative = AutomationAlerts.recorded.last?.informativeText ?? ""
    try require(alertsSince(alerts) == ["Rasterize 6 editable artwork layers?"],
                "\(tag) exactly one set prompt: \(alertsSince(alerts))")
    try require(informative.filter({ $0 == "“" }).count == 5
                && informative.contains(" and 1 more"),
                "\(tag) five names and \" and 1 more\": \(informative)")
    try require(artwork.allSatisfy { document.doc?.layerMeta($0) == nil },
                "\(tag) all six were rasterized")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) undo")
    print("PASS: set prompt decline and truncation — a declined set prompt at 8 and 16 bits "
        + "publishes nothing, keeps the session and names its decline note; six text layers "
        + "list five names and \" and 1 more\"")
}

/// A copy's pixels, offset and description.
private func copyState(_ document: ImageDocument, _ layer: Int) -> [String] {
    let pixels = document.doc?.layerPixels(layer) ?? []
    let checksum = pixels.reduce(0) { ($0 &* 31) &+ Int($1) }
    let offset = document.doc?.layerInfo(layer).map { [$0.offsetX, $0.offsetY] } ?? []
    return ["\(pixels.count):\(checksum)", "\(offset)", document.doc?.layerMeta(layer) ?? "-"]
}

func verifyParallelogramCopyReplay() throws {
    defer { _ = ActionRecorder.shared.stop() }
    let tag = "[8 parallelogram copy]"
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    let controller = try editor(document)
    _ = try agentOK("add_text_layer", document, ["text": "Probe", "x": 8, "y": 8])
    guard let base = document.doc, let t = textLayers(base).first else {
        throw ProbeFailure(description: "\(tag) add_text_layer made no text layer")
    }
    controller.setSelectedLayers(.single(t))
    let alerts = AutomationAlerts.recorded.count
    controller.freeTransformCopy(nil)
    controller.transformSession?.transform.cornerOffsets = [
        CGVector(dx: 4, dy: 0), CGVector(dx: 4, dy: 0), .zero, .zero]
    ActionRecorder.shared.start()
    let committed = controller.commitTransformSession()
    let recorded = ActionRecorder.shared.stop()
    try require(committed && document.doc !== base, "\(tag) the ⌥⌘T published")
    try require(AutomationAlerts.recorded.count == alerts, "\(tag) no alert: \(alertsSince(alerts))")
    let arguments = recorded.first?.arguments ?? [:]
    try require(recorded.count == 1 && recorded.first?.tool == "transform_layers"
                && arguments["corners"] != nil && arguments["source_rect"] != nil
                && arguments["duplicate"] as? Bool == true,
                "\(tag) one transform_layers step with corners, source_rect and duplicate: "
                + "\(recorded.map { "\($0.tool) \($0.arguments)" })")
    try require(document.layerDescribesSource(t + 1), "\(tag) the copy is still described")
    let ui = copyState(document, t + 1)
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) undo the ⌥⌘T")

    controller.setSelectedLayers(.single(t))
    let replay = ActionPlayer.runSingle(recorded[0], on: document, undoName: "Transform Replay")
    let replayed = copyState(document, t + 1)
    try require(replay.ok && replayed == ui,
                "\(tag) the replayed step gives the UI's copy: ok \(replay.ok), "
                + "\(replayed) vs \(ui)")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) undo the replay")

    var direct: [String: Any] = ["layers": [t], "duplicate": true]
    for key in ["source_rect", "corners", "sampler"] { direct[key] = arguments[key] }
    let reply = try agentOK("transform_layers", document, direct)
    let agent = copyState(document, t + 1)
    try require((reply["rasterized_layers"] as? [Int]) == [] && agent == ui,
                "\(tag) transform_layers gives the UI's copy: \(reply["rasterized_layers"] ?? "-"), "
                + "\(agent) vs \(ui)")
    document.undoManager?.undo()
    try require(document.doc === base, "\(tag) undo transform_layers")
    print("PASS: parallelogram copy replay — an 8-bit ⌥⌘T parallelogram over a text layer "
        + "asks nothing, keeps the copy described, and its recorded step and transform_layers "
        + "land the UI's copy")
}

/// Median (sorted[5]) and max of eleven millisecond samples.
private func summary(_ samples: [Double]) -> (median: Double, max: Double) {
    let sorted = samples.sorted()
    return (sorted[sorted.count / 2], sorted.last ?? 0)
}

private func milliseconds(_ body: () throws -> Void) rethrows -> Double {
    let start = DispatchTime.now().uptimeNanoseconds
    try body()
    return Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000
}

private func rotation(_ degrees: Double, about rect: CGRect) -> CGAffineTransform {
    CGAffineTransform(translationX: rect.midX, y: rect.midY)
        .rotated(by: degrees * .pi / 180)
        .translatedBy(x: -rect.midX, y: -rect.midY)
}

private func format(_ value: (median: Double, max: Double)) -> String {
    String(format: "median %.2f ms, max %.2f ms", value.median, value.max)
}

/// Eleven whole preview ticks of the open session, each pumped to rest.
private func tickSamples(_ controller: EditorViewController,
                         _ gesture: PrecisionGeometryGesture) throws -> [Double] {
    var samples: [Double] = []
    for k in 1...11 {
        samples.append(milliseconds { controller.transformDegrees = Double(k) })
        try pumpUntil({ !gesture.isPending }, seconds: 120)
    }
    return samples
}

func verifyCopyPreviewTickCost() throws {
    let tag = "[16 copy preview tick]"
    let document = try precisionFixture(width: 6000, height: 4000)
    defer { document.close() }
    let controller = try editor(document)
    _ = try agentOK("add_text_layer", document,
                    ["text": "Probe", "x": 400, "y": 400, "size": 200])
    guard let base = document.doc, let t = textLayers(base).first else {
        throw ProbeFailure(description: "\(tag) add_text_layer made no text layer")
    }

    // ⌥⌘T: the duplicate, the per-tick work of both requests, whole ticks.
    controller.setSelectedLayers(.single(t))
    controller.freeTransformCopy(nil)
    guard let session = controller.transformSession, session.duplicate != nil,
          let gesture = session.precisionGesture else {
        throw ProbeFailure(description: "\(tag) ⌥⌘T opened no typed duplicate session")
    }
    try pumpUntil({ !gesture.isPending }, seconds: 120)
    let facade = gesture.capture.facade
    let rect = session.sourceRect
    let sampler = session.sampler
    var duplicate: [Double] = []
    var copyWork: [Double] = []
    var setWork: [Double] = []
    for k in 1...11 {
        let matrix = rotation(Double(k), about: rect)
        duplicate.append(try milliseconds { _ = try facade.duplicateLayers([t]) })
        copyWork.append(try milliseconds {
            _ = try PrecisionGeometryWork(source: facade, request: .duplicateTransform(
                layers: [t], sourceRect: rect, matrix: matrix, quad: nil, sampler: sampler))
        })
        setWork.append(try milliseconds {
            _ = try PrecisionGeometryWork(source: facade, request: .transformSet(
                layers: [t], sourceRect: rect, matrix: matrix, quad: nil, sampler: sampler))
        })
    }
    let copyTicks = try tickSamples(controller, gesture)
    controller.endTransformSession()
    try require(document.doc === base, "\(tag) Esc on ⌥⌘T left the document unchanged")

    // ⌘T over the same layer.
    controller.setSelectedLayers(.single(t))
    controller.freeTransform(nil)
    guard let setGesture = controller.transformSession?.precisionGesture else {
        throw ProbeFailure(description: "\(tag) ⌘T opened no typed session")
    }
    try pumpUntil({ !setGesture.isPending }, seconds: 120)
    let setTicks = try tickSamples(controller, setGesture)
    controller.endTransformSession()
    try require(document.doc === base, "\(tag) Esc on ⌘T left the document unchanged")

    let budget = 16.7
    let duplicated = summary(duplicate), copyTick = summary(copyTicks)
    let setTick = summary(setTicks)
    let stall = duplicated.median > budget
        || (copyTick.median > budget && setTick.median <= budget)
    print("24 MP 16-bit text ⌥⌘T preview tick on main (Swift -Onone, release core): "
        + "duplicate \(format(duplicated)); ⌥⌘T work \(format(summary(copyWork))); "
        + "⌘T work \(format(summary(setWork))); ⌥⌘T tick \(format(copyTick)); "
        + "⌘T tick \(format(setTick)); frame budget 16.7 ms; verdict \(stall ? "stall" : "ok")")
    print("PASS: copy preview tick cost — the 24 MP 16-bit ⌥⌘T preview's duplicate, per-tick "
        + "work and whole ticks measured against ⌘T's; Esc left the document unchanged")
}
