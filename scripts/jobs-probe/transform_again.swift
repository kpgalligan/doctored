import AppKit

// Layer ▸ Transform Again (⇧⌘T) and Transform Again with Copy (⌥⇧⌘T)
// repeat the last committed Free Transform verbatim, as ONE step, against
// their MCP twin `transform_again`. The memory is the
// application's, and repeats only on a canvas the size it was committed on.
// Every case calls methods and the MCP dispatcher directly, so none needs a
// key window.

/// The steps a recording captured while `body` ran (heal_jobs.swift's helper).
private func recordedSteps(_ document: ImageDocument, _ body: () throws -> Void) throws -> [[String: Any]] {
    _ = try agentOK("start_recording", document, ["discard": true])
    try body()
    let stopped = try agentOK("stop_recording", document)
    guard let steps = stopped["steps"] as? [[String: Any]] else {
        throw ProbeFailure(description: "stop_recording returned no steps: \(stopped)")
    }
    return steps
}

private func againMatrix(_ matrix: CGAffineTransform) -> [Double] {
    [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty].map(Double.init)
}

private func degrees(_ matrix: CGAffineTransform) -> Double {
    Double(atan2(matrix.b, matrix.a)) * 180 / .pi
}

private func fixes(_ matrix: CGAffineTransform, _ point: CGPoint) -> Bool {
    let moved = point.applying(matrix)
    return abs(moved.x - point.x) < 1e-9 && abs(moved.y - point.y) < 1e-9
}

/// R: layer `layer`'s pixels and offset at 8 bits; six `sample_pixel`
/// readings at 16.
private func againState(_ document: ImageDocument, depth: Int, layer: Int = 0) throws -> [String] {
    if depth == 8 {
        let pixels = document.doc?.layerPixels(layer) ?? []
        let checksum = pixels.reduce(0) { ($0 &* 31) &+ Int($1) }
        let offset = document.doc?.layerInfo(layer).map { [$0.offsetX, $0.offsetY] } ?? []
        return ["\(pixels.count):\(checksum)", "\(offset)"]
    }
    let points = [(32, 24), (28, 20), (36, 28), (30, 26), (34, 22), (32, 18)]
    return try points.map {
        "\(NSDictionary(dictionary: try agentOK("sample_pixel", document, ["x": $0.0, "y": $0.1])))"
    }
}

private func selectionBounds(_ document: ImageDocument) -> CGRect? {
    document.canvasSelection?.bounds
}

private func holes(_ document: ImageDocument) -> Int {
    let pixels = document.doc?.layerPixels(0) ?? []
    return stride(from: 3, to: pixels.count, by: 4).filter { pixels[$0] < 255 }.count
}

private func againFixture(depth: Int) throws -> ImageDocument {
    depth == 8 ? try fixture(width: 64, height: 64) : try precisionFixture(width: 64, height: 48)
}

func verifyTransformAgainLayers() throws {
    defer { _ = ActionRecorder.shared.stop() }
    for depth in [8, 16] {
        let tag = "[\(depth)]"
        // The memory is the application's: each depth starts with none.
        TransformAgainMemory.last = nil
        let document = try againFixture(depth: depth)
        defer { document.close() }
        let controller = try editor(document)
        let sampler = EditorViewController.samplerAgentName(controller.transformSampler)

        // No memory: nothing validates, MCP refuses, the item does nothing.
        try require(!controller.validateTransformAgainItem(), "\(tag) no memory validates false")
        let empty = try agentCall("transform_again", document)
        try require(empty.isError && empty.text.contains(RepeatableTransform.emptyRefusal),
                    "\(tag) MCP answers the empty refusal: \(empty.text)")
        guard let base = document.doc else { throw ProbeFailure(description: "\(tag) fixture") }
        controller.transformAgain(nil)
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "\(tag) no memory adds no step")

        // ⌘T at 15° stores its matrix, which fixes the pivot.
        controller.freeTransform(nil)
        controller.transformDegrees = 15
        guard let session = controller.transformSession else {
            throw ProbeFailure(description: "\(tag) ⌘T opened no session")
        }
        let matrix = session.transform.matrix
        let pivot = session.transform.pivotInCanvas
        try require(controller.commitTransformSession(), "\(tag) the ⌘T committed")
        guard let once = document.doc, once !== base else {
            throw ProbeFailure(description: "\(tag) the ⌘T did not publish")
        }
        let stored = RepeatableTransform(matrix: matrix, sampler: controller.transformSampler)
        try require(document.lastFreeTransform == stored,
                    "\(tag) ⌘T stored its matrix: \(String(describing: document.lastFreeTransform))")
        try require(fixes(matrix, pivot) && abs(degrees(matrix) - 15) < 1e-9,
                    "\(tag) the matrix turns 15° about the pivot: \(matrix) \(pivot)")
        document.undoManager?.undo()
        document.undoManager?.redo()
        try require(document.doc === once && document.lastFreeTransform == stored,
                    "\(tag) undo and redo keep the memory")

        // ⇧⌘T: one "Transform Again" step, no session.
        try require(controller.validateTransformAgainItem(), "\(tag) the memory validates")
        controller.transformAgain(nil)
        try require(controller.transformSession == nil, "\(tag) no session opened")
        try require(document.doc !== once
                    && document.undoManager?.undoActionName == RepeatableTransform.actionName,
                    "\(tag) one \"Transform Again\" step: "
                    + "\(document.undoManager?.undoActionName ?? "-")")
        try require(document.lastFreeTransform == stored, "\(tag) the memory is kept")
        let ui = try againState(document, depth: depth)
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) one undo returns to the ⌘T result")
        _ = try agentOK("transform_layers", document,
                        ["layers": [0], "matrix": againMatrix(matrix), "sampler": sampler])
        let layers = try againState(document, depth: depth)
        try require(ui == layers, "\(tag) ⇧⌘T equals transform_layers: \(ui) vs \(layers)")
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) undo after transform_layers")
        let reply = try agentOK("transform_again", document)
        let agent = try againState(document, depth: depth)
        try require(reply["target"] as? String == "layers" && reply["changed"] as? Bool == true
                    && document.undoManager?.undoActionName == RepeatableTransform.actionName,
                    "\(tag) MCP transform_again over layers: \(reply)")
        try require(ui == agent, "\(tag) ⇧⌘T equals MCP: \(ui) vs \(agent)")
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) undo after MCP")

        // ⌥⇧⌘T: the copy transforms and is selected.
        controller.transformAgainWithCopy(nil)
        try require(document.doc?.layerCount == 2
                    && document.doc?.layerInfo(1)?.name.hasSuffix("copy") == true
                    && document.layerSelection.all == [1]
                    && document.undoManager?.undoActionName == RepeatableTransform.copyActionName,
                    "\(tag) ⌥⇧⌘T: two layers, the copy selected, one step: "
                    + "\(document.layerSelection.all) \(document.undoManager?.undoActionName ?? "-")")
        if depth == 8 {
            try require(document.doc?.layerPixels(0) == once.layerPixels(0)
                        && document.doc?.layerInfo(0)?.offsetX == once.layerInfo(0)?.offsetX
                        && document.doc?.layerInfo(0)?.offsetY == once.layerInfo(0)?.offsetY,
                        "\(tag) the original is unchanged")
        }
        let copyUI = try againState(document, depth: depth, layer: 1)
        try require(copyUI == ui, "\(tag) the copy gives R: \(copyUI) vs \(ui)")
        document.undoManager?.undo()
        try require(document.doc === once && document.doc?.layerCount == 1
                    && document.layerSelection.all == [0], "\(tag) undo leaves one layer and [0]")
        let copyReply = try agentOK("transform_again", document, ["duplicate": true])
        let copyAgent = try againState(document, depth: depth, layer: 1)
        try require((copyReply["copies"] as? [Int]) == [1] && document.doc?.layerCount == 2
                    && document.layerSelection.all == [1]
                    && document.undoManager?.undoActionName == RepeatableTransform.copyActionName,
                    "\(tag) MCP duplicate answers copies [1]: \(copyReply)")
        try require(copyAgent == copyUI, "\(tag) the MCP copy equals ⌥⇧⌘T: \(copyAgent)")
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) undo after the MCP copy")

        // Both UI items record transform_again.
        let steps = try recordedSteps(document) {
            controller.transformAgain(nil)
            controller.transformAgainWithCopy(nil)
        }
        let summary = steps.map { "\($0["tool"] ?? "-") \($0["arguments"] ?? [:])" }
        try require(steps.count == 2
                    && steps.allSatisfy { $0["tool"] as? String == "transform_again" }
                    && (steps[0]["arguments"] as? [String: Any])?.isEmpty != false
                    && (steps[1]["arguments"] as? [String: Any])?["duplicate"] as? Bool == true
                    && (steps[1]["arguments"] as? [String: Any])?.count == 1,
                    "\(tag) recorded [transform_again {}, transform_again {duplicate:true}]: "
                    + "\(summary)")
    }
    print("PASS: transform again layers — no memory refuses; ⌘T stores its matrix (pivot fixed, "
        + "kept by undo/redo); ⇧⌘T is one \"Transform Again\" step equal to transform_layers "
        + "and MCP transform_again; ⌥⇧⌘T copies, selects the copy and equals MCP duplicate; "
        + "both record transform_again, at 8 and 16 bits")
}

func verifyTransformAgainSelectedPixels() throws {
    for depth in [8, 16] {
        let tag = "[\(depth)]"
        let document = try againFixture(depth: depth)
        defer { document.close() }
        let controller = try editor(document)
        _ = try select(document, side: 16)

        controller.freeTransform(nil)
        controller.transformDegrees = 15
        guard let session = controller.transformSession, session.floating != nil else {
            throw ProbeFailure(description: "\(tag) ⌘T opened no floating session")
        }
        let matrix = session.transform.matrix
        let box = session.sourceRect
        try require(controller.commitTransformSession(), "\(tag) the ⌘T committed")
        try require(document.undoManager?.undoActionName == TransformSelectedPixels.actionName,
                    "\(tag) the ⌘T is \"Transform Selected Pixels\"")
        try require(document.lastFreeTransform
                    == RepeatableTransform(matrix: matrix, sampler: controller.transformSampler)
                    && fixes(matrix, CGPoint(x: box.midX, y: box.midY)),
                    "\(tag) the memory is the matrix, fixing the box centre")
        guard let once = document.doc, let onceBounds = selectionBounds(document) else {
            throw ProbeFailure(description: "\(tag) no ⌘T result")
        }
        let hole = holes(document)

        controller.transformAgain(nil)
        try require(document.doc !== once
                    && document.undoManager?.undoActionName == RepeatableTransform.actionName,
                    "\(tag) one \"Transform Again\" step: "
                    + "\(document.undoManager?.undoActionName ?? "-")")
        let uiBounds = selectionBounds(document)
        try require(uiBounds != nil && uiBounds != onceBounds,
                    "\(tag) the selection moved: \(String(describing: uiBounds)) \(onceBounds)")
        let ui = try againState(document, depth: depth)
        if depth == 8 {
            try require(holes(document) > hole,
                        "\(tag) the repeat leaves a new hole: \(holes(document)) vs \(hole)")
        }
        document.undoManager?.undo()
        try require(document.doc === once && selectionBounds(document) == onceBounds,
                    "\(tag) one undo returns to the ⌘T result")
        let reply = try agentOK("transform_again", document)
        try require(reply["target"] as? String == "selected_pixels"
                    && document.undoManager?.undoActionName == RepeatableTransform.actionName,
                    "\(tag) MCP over selected pixels: \(reply)")
        let agent = try againState(document, depth: depth)
        try require(ui == agent && selectionBounds(document) == uiBounds,
                    "\(tag) ⇧⌘T equals MCP: \(ui) vs \(agent)")
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) undo after MCP")

        controller.transformAgainWithCopy(nil)
        try require(document.doc !== once
                    && document.undoManager?.undoActionName == RepeatableTransform.copyActionName,
                    "\(tag) one \"Transform Again with Copy\" step")
        if depth == 8 {
            try require(holes(document) <= hole,
                        "\(tag) the copy leaves no new hole: \(holes(document)) vs \(hole)")
        }
        let copyUI = try againState(document, depth: depth)
        let copyBounds = selectionBounds(document)
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) undo after the copy")
        _ = try agentOK("transform_again", document, ["duplicate": true])
        let copyAgent = try againState(document, depth: depth)
        try require(copyUI == copyAgent && selectionBounds(document) == copyBounds,
                    "\(tag) ⌥⇧⌘T equals MCP duplicate: \(copyUI) vs \(copyAgent)")
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) undo after the MCP copy")

        // A Pixels lock refuses both.
        _ = try agentOK("set_layer_lock", document, ["layer": 0, "locks": ["pixels"]])
        guard let locked = document.doc else { throw ProbeFailure(description: "\(tag) lock") }
        let alerts = AutomationAlerts.recorded.count
        controller.transformAgain(nil)
        try require(document.doc === locked, "\(tag) a Pixels lock adds no step")
        try require(AutomationAlerts.recorded.count == alerts + 1,
                    "\(tag) the lock refusal is one alert: \(AutomationAlerts.recorded.suffix(2))")
        let refused = try agentCall("transform_again", document)
        try require(refused.isError && refused.text.lowercased().contains("pixels"),
                    "\(tag) MCP refuses naming the lock: \(refused.text)")
    }
    print("PASS: transform again selected pixels — ⌘T over a selection stores its matrix (the box "
        + "centre fixed); ⇧⌘T is one \"Transform Again\" step that moves the selection and "
        + "equals MCP; ⌥⇧⌘T leaves no new hole and equals MCP duplicate; a Pixels lock refuses "
        + "both, at 8 and 16 bits")
}

func verifyTransformAgainMemory() throws {
    let document = try fixture(width: 64, height: 64)
    defer { document.close() }
    let controller = try editor(document)
    let sampler = controller.transformSampler

    // A 15° commit sets it; an Esc'd 30° session and an untouched ⌘T keep it.
    controller.freeTransform(nil)
    controller.transformDegrees = 15
    guard let turned = controller.transformSession?.transform.matrix else {
        throw ProbeFailure(description: "⌘T opened no session")
    }
    try require(controller.commitTransformSession(), "the 15° ⌘T committed")
    let stored = RepeatableTransform(matrix: turned, sampler: sampler)
    try require(document.lastFreeTransform == stored, "a 15° commit sets the memory")
    controller.freeTransform(nil)
    controller.transformDegrees = 30
    controller.endTransformSession()
    try require(document.lastFreeTransform == stored, "an Esc'd 30° session keeps it")
    controller.freeTransform(nil)
    try require(controller.commitTransformSession() && controller.transformSession == nil,
                "an untouched ⌘T closes")
    try require(document.lastFreeTransform == stored, "an untouched ⌘T commit keeps it")

    // A 5° ⌥⌘T over a selection stores its matrix.
    _ = try select(document, side: 16)
    controller.freeTransformCopy(nil)
    controller.transformDegrees = 5
    guard let copied = controller.transformSession?.transform.matrix else {
        throw ProbeFailure(description: "⌥⌘T opened no session")
    }
    try require(controller.commitTransformSession(), "the 5° ⌥⌘T committed")
    try require(document.lastFreeTransform == RepeatableTransform(matrix: copied, sampler: sampler),
                "a 5° ⌥⌘T stores its matrix")
    _ = try agentOK("deselect", document)

    // A parallelogram stores its affine; a true perspective its corners.
    controller.freeTransform(nil)
    controller.transformSession?.transform.cornerOffsets = [
        CGVector(dx: 4, dy: 0), CGVector(dx: 4, dy: 0), .zero, .zero]
    guard let sheared = controller.transformSession else {
        throw ProbeFailure(description: "⌘T opened no session for the shear")
    }
    let shearRect = sheared.sourceRect
    let shearQuad = sheared.transform.warpedQuad(of: shearRect)
    try require(controller.commitTransformSession(), "the shear committed")
    guard let shear = LayerTransform.parallelogramAffine(of: shearRect, onto: shearQuad) else {
        throw ProbeFailure(description: "the shear is not a parallelogram")
    }
    try require(document.lastFreeTransform == RepeatableTransform(matrix: shear, sampler: sampler),
                "a parallelogram stores its affine: "
                + "\(String(describing: document.lastFreeTransform)) vs \(shear)")
    controller.freeTransform(nil)
    controller.transformSession?.transform.cornerOffsets = [
        .zero, .zero, CGVector(dx: 6, dy: 4), .zero]
    guard let warped = controller.transformSession else {
        throw ProbeFailure(description: "⌘T opened no session for the perspective")
    }
    let perspective = RepeatableTransform(
        perspective: .init(sourceRect: warped.sourceRect,
                           quad: warped.transform.warpedQuad(of: warped.sourceRect)),
        sampler: sampler)
    try require(controller.commitTransformSession(), "the perspective committed")
    try require(document.lastFreeTransform == perspective && controller.validateTransformAgainItem(),
                "a true perspective stores its corners and validates: "
                + "\(String(describing: document.lastFreeTransform))")
    document.undoManager?.undo()
    try require(document.lastFreeTransform == perspective, "undo keeps the memory")

    // The MCP writers.
    _ = try agentOK("transform_layer", document, ["layer": 0, "rotate": 10, "pivot_x": 0, "pivot_y": 0])
    guard let ten = document.lastFreeTransform else {
        throw ProbeFailure(description: "transform_layer stored nothing")
    }
    try require(abs(degrees(ten.matrix) - 10) < 1e-9 && fixes(ten.matrix, .zero),
                "transform_layer stores 10° about (0,0): \(ten.matrix)")
    _ = try agentOK("transform_layers", document, ["layers": [0], "matrix": [1, 0, 0, 1, 3, 0]])
    try require(document.lastFreeTransform?.matrix == CGAffineTransform(translationX: 3, y: 0),
                "transform_layers stores its translation")
    guard let info = document.doc?.layerInfo(0) else { throw ProbeFailure(description: "layer 0") }
    let rect = CGRect(x: info.offsetX, y: info.offsetY, width: info.width, height: info.height)
    let corners = [CGPoint(x: rect.minX + 4, y: rect.minY), CGPoint(x: rect.maxX + 4, y: rect.minY),
                   CGPoint(x: rect.maxX, y: rect.maxY), CGPoint(x: rect.minX, y: rect.maxY)]
    _ = try agentOK("distort_layer", document,
                    ["layer": 0, "corners": corners.map { [Double($0.x), Double($0.y)] }])
    guard let distortAffine = LayerTransform.parallelogramAffine(of: rect, onto: corners),
          let distorted = document.lastFreeTransform else {
        throw ProbeFailure(description: "the sheared distort stored nothing")
    }
    try require(distorted.matrix == distortAffine,
                "a parallelogram distort_layer stores its affine: \(distorted.matrix) vs \(distortAffine)")
    guard let after = document.doc?.layerInfo(0) else { throw ProbeFailure(description: "layer 0") }
    let afterRect = CGRect(x: after.offsetX, y: after.offsetY, width: after.width, height: after.height)
    let perspectiveCorners = [
        CGPoint(x: afterRect.minX, y: afterRect.minY), CGPoint(x: afterRect.maxX, y: afterRect.minY),
        CGPoint(x: afterRect.maxX + 6, y: afterRect.maxY + 4),
        CGPoint(x: afterRect.minX, y: afterRect.maxY)]
    _ = try agentOK("distort_layer", document, ["layer": 0, "corners": perspectiveCorners.map {
        [Double($0.x), Double($0.y)] }])
    try require(document.lastFreeTransform?.perspective
                == .init(sourceRect: afterRect, quad: perspectiveCorners),
                "a true-perspective distort_layer stores its corners: "
                + "\(String(describing: document.lastFreeTransform))")
    guard let beforeAgain = document.doc else { throw ProbeFailure(description: "distorted") }
    let repeated = try agentOK("transform_again", document)
    try require(repeated["changed"] as? Bool == true && repeated["corners"] != nil
                && repeated["source_rect"] != nil && repeated["matrix"] == nil
                && document.doc !== beforeAgain,
                "transform_again repeats the perspective: \(repeated)")
    document.undoManager?.undo()
    try require(document.doc === beforeAgain, "undo after the perspective repeat")
    _ = try select(document, side: 16)
    let selected = try agentOK("transform_selected_pixels", document, ["rotate": 20])
    guard let boxReply = selected["box"] as? [String: Any],
          let bx = (boxReply["x"] as? NSNumber)?.doubleValue,
          let by = (boxReply["y"] as? NSNumber)?.doubleValue,
          let bw = (boxReply["width"] as? NSNumber)?.doubleValue,
          let bh = (boxReply["height"] as? NSNumber)?.doubleValue,
          let twenty = document.lastFreeTransform else {
        throw ProbeFailure(description: "transform_selected_pixels: \(selected)")
    }
    try require(abs(degrees(twenty.matrix) - 20) < 1e-9
                && fixes(twenty.matrix, CGPoint(x: bx + bw / 2, y: by + bh / 2)),
                "transform_selected_pixels stores 20° about the box centre")
    _ = try agentOK("deselect", document)

    // Other edits keep it. The memory is the application's (measured):
    // a same-size second document repeats it, a document of another size
    // refuses naming both sizes, a transform in any document replaces it, and
    // closing the document it was committed in keeps it.
    _ = try agentOK("set_layer_properties", document, ["layer": 0, "opacity": 0.5])
    try require(document.lastFreeTransform == twenty, "set_layer_properties keeps the memory")
    let second = try fixture(width: 64, height: 64)
    defer { second.close() }
    let secondController = try editor(second)
    try requireAgain(second, secondController, twenty, "a same-size second document has it")
    guard let secondBase = second.doc else { throw ProbeFailure(description: "second") }
    let secondAgain = try agentOK("transform_again", second)
    try require(secondAgain["changed"] as? Bool == true && second.doc !== secondBase,
                "transform_again repeats it in the second document: \(secondAgain)")
    second.undoManager?.undo()
    let other = try fixture(width: 64, height: 48)
    defer { other.close() }
    let otherController = try editor(other)
    try requireAgain(other, otherController, nil,
                     refusal: RepeatableTransform.sizeRefusal(
                        committed: CGSize(width: 64, height: 64), canvas: CGSize(width: 64, height: 48)),
                     "a 64×48 document cannot repeat a 64×64 memory")
    _ = try agentOK("transform_layers", other, ["layers": [0], "matrix": [1, 0, 0, 1, 0, 2]])
    try require(other.lastFreeTransform?.matrix == CGAffineTransform(translationX: 0, y: 2)
                && document.lastFreeTransform == nil && second.lastFreeTransform == nil,
                "a transform in the 64×48 document replaces the memory everywhere")
    let closing = try fixture(width: 64, height: 64)
    _ = try agentOK("transform_layers", closing, ["layers": [0], "matrix": [1, 0, 0, 1, 3, 0]])
    let closed = closing.lastFreeTransform
    closing.close()
    try require(closed?.matrix == CGAffineTransform(translationX: 3, y: 0)
                && document.lastFreeTransform == closed && second.lastFreeTransform == closed
                && other.lastFreeTransform == nil,
                "closing the document it was committed in keeps the memory")

    // 16 bits: the multi-layer gate.
    let precision = try precisionFixture(width: 64, height: 48)
    defer { precision.close() }
    let precisionController = try editor(precision)
    _ = try agentOK("transform_layers", precision, ["layers": [0], "matrix": [1, 0, 0, 1, 3, 0]])
    _ = try agentOK("new_layer", precision)
    let item = NSMenuItem(title: RepeatableTransform.actionName,
                          action: #selector(EditorViewController.transformAgain(_:)),
                          keyEquivalent: "")
    _ = try agentOK("set_selected_layers", precision, ["layers": [0, 1]])
    try require(!precisionController.validateUserInterfaceItem(item),
                "[16] two selected layers do not validate")
    _ = try agentOK("set_selected_layers", precision, ["layers": [0]])
    try require(precisionController.validateUserInterfaceItem(item), "[16] one layer validates")
    try verifyTransformAgainCanvasEdits()
    print("PASS: transform again memory — ⌘T, ⌥⌘T, a parallelogram, a true perspective (its "
        + "corners) and the MCP writers set it, Esc, an untouched ⌘T, undo and other edits keep "
        + "it, transform_again repeats a distort_layer perspective, the 16-bit item needs one "
        + "layer; the memory is the application's: a same-size document repeats "
        + "it, another size refuses naming both, a transform anywhere replaces it and closing "
        + "keeps it; undo, redo and History jumps never change it; Canvas Size, Image Size, "
        + "Crop and Rotate 90° make it unavailable and undoing them, or sizing back, restores "
        + "it; flip, Rotate 180° and a square quarter turn keep it")
}

/// `document`'s view of the memory is `want`, the item validates exactly when
/// there is one, and with none MCP `transform_again` refuses in band (with
/// `refusal` when given), changing nothing.
private func requireAgain(_ document: ImageDocument, _ controller: EditorViewController,
                          _ want: RepeatableTransform?, refusal: String? = nil,
                          _ label: String) throws {
    try require(document.lastFreeTransform == want
                && controller.validateTransformAgainItem() == (want != nil),
                "\(label): \(String(describing: document.lastFreeTransform)), validates "
                + "\(controller.validateTransformAgainItem())")
    guard want == nil else { return }
    guard let base = document.doc else { throw ProbeFailure(description: "\(label): no doc") }
    let refused = try agentCall("transform_again", document)
    try require(refused.isError && document.doc === base
                && (refusal.map { refused.text.contains($0) } ?? true),
                "\(label): MCP refuses in band: \(refused.text)")
}

/// Undo, redo and History navigation never change the memory, and it repeats
/// only while the canvas is the size it was committed on
/// (`transform_again_after_edits.py`, takes 1–3).
private func verifyTransformAgainCanvasEdits() throws {
    let document = try fixture(width: 64, height: 48)
    defer { document.close() }
    let controller = try editor(document)
    func sized(_ width: Int, _ height: Int) -> String {
        RepeatableTransform.sizeRefusal(committed: CGSize(width: 64, height: 48),
                                        canvas: CGSize(width: width, height: height))
    }
    _ = try agentOK("transform_layers", document, ["layers": [0], "matrix": [1, 0, 0, 1, 3, 0]])
    let first = document.lastFreeTransform
    _ = try agentOK("transform_layers", document, ["layers": [0], "matrix": [1, 0, 0, 1, 0, 2]])
    guard let memory = document.lastFreeTransform, memory != first,
          memory.matrix == CGAffineTransform(translationX: 0, y: 2) else {
        throw ProbeFailure(description: "the second transform_layers stored nothing")
    }

    // History never touches it (Transform Again's rule, take2).
    let cursor = document.history.cursor
    guard cursor >= 2 else { throw ProbeFailure(description: "history cursor \(cursor)") }
    let beforeFirst = document.history.entries[cursor - 2].id
    let latest = document.history.entries[cursor].id
    document.undoManager?.undo()
    try requireAgain(document, controller, memory, "undo of the second transform keeps it")
    document.undoManager?.redo()
    try requireAgain(document, controller, memory, "redo keeps it")
    document.undoManager?.undo()
    document.undoManager?.undo()
    try requireAgain(document, controller, memory, "undo of both transforms keeps it")
    document.undoManager?.redo()
    document.undoManager?.redo()
    try requireAgain(document, controller, memory, "redo of both keeps it")
    try document.navigateHistory(toID: beforeFirst)
    try require(document.history.cursor == cursor - 2, "the History jump landed before both")
    try requireAgain(document, controller, memory, "a History jump before both keeps it")
    try document.navigateHistory(toID: latest)
    try requireAgain(document, controller, memory, "a History jump back keeps it")

    // Canvas edits: a size change makes it unavailable, the size back restores it.
    _ = try agentOK("canvas_size", document, ["width": 80, "height": 48, "anchor": "top-left"])
    try requireAgain(document, controller, nil, refusal: sized(80, 48), "Canvas Size 80×48")
    document.undoManager?.undo()
    try requireAgain(document, controller, memory, "undo of Canvas Size restores it")
    document.undoManager?.redo()
    try requireAgain(document, controller, nil, refusal: sized(80, 48), "redo of Canvas Size")
    document.undoManager?.undo()
    let kept: [(String, [String: Any])] = [("flip", ["axis": "horizontal"]),
                                           ("rotate", ["degrees": 180])]
    for (tool, arguments) in kept {
        _ = try agentOK(tool, document, arguments)
        try requireAgain(document, controller, memory, "\(tool) \(arguments) keeps it")
        document.undoManager?.undo()
    }
    let resized: [(String, [String: Any], Int, Int)] = [
        ("image_size", ["width": 128, "height": 96], 128, 96),
        ("crop", ["x": 8, "y": 8, "width": 32, "height": 24], 32, 24),
    ]
    for (tool, arguments, width, height) in resized {
        _ = try agentOK(tool, document, arguments)
        try requireAgain(document, controller, nil, refusal: sized(width, height), tool)
        document.undoManager?.undo()
        try requireAgain(document, controller, memory, "undo of \(tool) restores it")
    }
    controller.rotateCW(nil)
    try requireAgain(document, controller, nil, refusal: sized(48, 64), "Rotate 90° CW")
    document.undoManager?.undo()
    try requireAgain(document, controller, memory, "undo of Rotate 90° restores it")
    _ = try agentOK("canvas_size", document, ["width": 80, "height": 48, "anchor": "top-left"])
    _ = try agentOK("canvas_size", document, ["width": 64, "height": 48, "anchor": "top-left"])
    try requireAgain(document, controller, memory, "Canvas Size 80×48 and back keeps it")
    guard let back = document.doc else { throw ProbeFailure(description: "sized back") }
    let repeated = try agentOK("transform_again", document)
    try require(repeated["changed"] as? Bool == true && document.doc !== back,
                "transform_again repeats it after the canvas is sized back: \(repeated)")

    // A quarter turn of a square canvas keeps it.
    let square = try fixture(width: 64, height: 64)
    defer { square.close() }
    let squareController = try editor(square)
    _ = try agentOK("transform_layers", square, ["layers": [0], "matrix": [1, 0, 0, 1, 3, 0]])
    guard let squared = square.lastFreeTransform else {
        throw ProbeFailure(description: "the square transform stored nothing")
    }
    _ = try agentOK("rotate", square, ["degrees": 90])
    try requireAgain(square, squareController, squared, "Rotate 90° of a square canvas keeps it")
}

/// A 64×64 8-bit document of two layers with transparent margins: layer 0
/// opaque only in (10, 12, 20, 16), layer 1 only in (36, 30, 14, 12).
private func marginFixture() throws -> ImageDocument {
    func layer(_ box: (Int, Int, Int, Int), _ rgb: (UInt8, UInt8, UInt8)) -> [UInt8] {
        var bytes = [UInt8](repeating: 0, count: 64 * 64 * 4)
        for y in box.1..<(box.1 + box.3) {
            for x in box.0..<(box.0 + box.2) {
                let p = (y * 64 + x) * 4
                bytes[p] = rgb.0 &+ UInt8(x)
                bytes[p + 1] = rgb.1 &+ UInt8(y)
                bytes[p + 2] = rgb.2
                bytes[p + 3] = 255
            }
        }
        return bytes
    }
    guard let image = RasterImage.from(rgba: layer((10, 12, 20, 16), (120, 60, 30)),
                                       width: 64, height: 64),
          let top = RasterImage.from(rgba: layer((36, 30, 14, 12), (20, 90, 200)),
                                     width: 64, height: 64),
          let document = ImageDocument.makeUntitled(with: image),
          let doc = document.doc,
          let two = doc.addingImageLayer(above: 0, top, name: "Top") else {
        throw ProbeFailure(description: "Margin fixture")
    }
    document.doc = two
    document.updateChangeCount(.changeCleared)
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

/// Both layers' pixels and offsets (8 bits).
private func pairState(_ document: ImageDocument) -> [String] {
    (0..<2).flatMap { layer -> [String] in
        let pixels = document.doc?.layerPixels(layer) ?? []
        let checksum = pixels.reduce(0) { ($0 &* 31) &+ Int($1) }
        let offset = document.doc?.layerInfo(layer).map { [$0.offsetX, $0.offsetY] } ?? []
        return ["\(pixels.count):\(checksum)", "\(offset)"]
    }
}

private func rectArgument(_ rect: CGRect) -> [Double] {
    [rect.minX, rect.minY, rect.width, rect.height].map(Double.init)
}

private func cornersArgument(_ quad: [CGPoint]) -> [[Double]] {
    quad.map { [Double($0.x), Double($0.y)] }
}

func verifyTransformAgainPerspective() throws {
    defer { _ = ActionRecorder.shared.stop() }
    let horizonPixels = "The perspective map folds the selected pixels past its horizon."
    // replayRefusal rewrites only the core's geometric refusals; a resource
    // or admission refusal keeps its own sentence.
    for geometric in [
        "byte geometry destination exceeds supported bounds",
        "perspective set raster crosses the horizon or exceeds supported bounds",
        "perspective set raster crosses the horizon",
        "typed perspective set crosses the horizon",
        "typed perspective destination crosses the horizon or exceeds supported bounds",
        "typed perspective quad exceeds supported bounds",
        "typed perspective set quad exceeds supported bounds",
    ] {
        try require(RepeatableTransform.isGeometricRefusal(geometric),
                    "geometric refusal is rewritten: \(geometric)")
    }
    for other in [
        "typed geometry exceeds the bounded pixel-work limit",
        "typed geometry scratch overflow",
        "typed geometry work overflow",
        "the layer style plane of “Layer 1” would exceed the pixel limit after this geometry; "
            + "reduce the effect sizes or clear its style first",
    ] {
        try require(!RepeatableTransform.isGeometricRefusal(other),
                    "non-geometric refusal keeps its own sentence: \(other)")
    }
    for depth in [8, 16] {
        let tag = "[\(depth)]"
        let document = try againFixture(depth: depth)
        defer { document.close() }
        let controller = try editor(document)
        let sampler = EditorViewController.samplerAgentName(controller.transformSampler)

        // ⌘T with one corner dragged stores Perspective(S, Q).
        controller.freeTransform(nil)
        controller.transformSession?.transform.cornerOffsets = [
            .zero, .zero, CGVector(dx: 6, dy: 4), .zero]
        guard let session = controller.transformSession else {
            throw ProbeFailure(description: "\(tag) ⌘T opened no session")
        }
        let rect = session.sourceRect
        let quad = session.transform.warpedQuad(of: rect)
        try require(controller.commitTransformSession(), "\(tag) the perspective ⌘T committed")
        guard let once = document.doc else { throw ProbeFailure(description: "\(tag) ⌘T") }
        let stored = RepeatableTransform(perspective: .init(sourceRect: rect, quad: quad),
                                         sampler: controller.transformSampler)
        try require(document.lastFreeTransform == stored,
                    "\(tag) ⌘T stored Perspective(S, Q): "
                    + "\(String(describing: document.lastFreeTransform))")

        // ⇧⌘T: one step, recorded with rasterize_artwork, equal to
        // transform_layers with the same S and Q and to MCP transform_again.
        let steps = try recordedSteps(document) { controller.transformAgain(nil) }
        try require(document.doc !== once
                    && document.undoManager?.undoActionName == RepeatableTransform.actionName,
                    "\(tag) one \"Transform Again\" step: "
                    + "\(document.undoManager?.undoActionName ?? "-")")
        let arguments = steps.first?["arguments"] as? [String: Any]
        try require(steps.count == 1 && steps.first?["tool"] as? String == "transform_again"
                    && arguments?["rasterize_artwork"] as? Bool == true && arguments?.count == 1,
                    "\(tag) recorded transform_again {rasterize_artwork: true}: \(steps)")
        try require(document.lastFreeTransform == stored, "\(tag) the memory is kept")
        let ui = try againState(document, depth: depth)
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) one undo returns to the ⌘T result")
        _ = try agentOK("transform_layers", document,
                        ["layers": [0], "source_rect": rectArgument(rect),
                         "corners": cornersArgument(quad), "sampler": sampler])
        let layers = try againState(document, depth: depth)
        try require(ui == layers, "\(tag) ⇧⌘T equals transform_layers: \(ui) vs \(layers)")
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) undo after transform_layers")
        let reply = try agentOK("transform_again", document)
        let agent = try againState(document, depth: depth)
        try require(reply["changed"] as? Bool == true && reply["corners"] != nil
                    && reply["matrix"] == nil,
                    "\(tag) MCP transform_again answers corners: \(reply)")
        try require(ui == agent, "\(tag) ⇧⌘T equals MCP: \(ui) vs \(agent)")
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) undo after MCP")

        // ⌥⇧⌘T equals MCP duplicate.
        controller.transformAgainWithCopy(nil)
        try require(document.doc?.layerCount == 2
                    && document.undoManager?.undoActionName == RepeatableTransform.copyActionName,
                    "\(tag) ⌥⇧⌘T made one copy step")
        let copyUI = try againState(document, depth: depth, layer: 1)
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) undo after ⌥⇧⌘T")
        let copyReply = try agentOK("transform_again", document, ["duplicate": true])
        let copyAgent = try againState(document, depth: depth, layer: 1)
        try require((copyReply["copies"] as? [Int]) == [1] && copyReply["corners"] != nil,
                    "\(tag) MCP duplicate answers copies and corners: \(copyReply)")
        try require(copyUI == copyAgent, "\(tag) ⌥⇧⌘T equals MCP duplicate: \(copyUI) vs \(copyAgent)")
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) undo after the MCP copy")

        // Over a 16 px selection, ⇧⌘T equals MCP.
        _ = try select(document, side: 16)
        controller.transformAgain(nil)
        try require(document.doc !== once, "\(tag) ⇧⌘T over the selection published")
        let selectedUI = try againState(document, depth: depth)
        let selectedBounds = selectionBounds(document)
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) undo after ⇧⌘T over the selection")
        let selectedReply = try agentOK("transform_again", document)
        let selectedAgent = try againState(document, depth: depth)
        try require(selectedReply["target"] as? String == "selected_pixels"
                    && selectedReply["corners"] != nil,
                    "\(tag) MCP over the selection answers corners: \(selectedReply)")
        try require(selectedUI == selectedAgent && selectionBounds(document) == selectedBounds,
                    "\(tag) ⇧⌘T over the selection equals MCP: \(selectedUI) vs \(selectedAgent)")
        document.undoManager?.undo()
        try require(document.doc === once, "\(tag) undo after MCP over the selection")
        _ = try agentOK("deselect", document)

        if depth == 8 {
            // A text layer: MCP needs rasterize_artwork; the UI prompts once.
            _ = try agentOK("add_text_layer", document, ["text": "Probe", "x": 8, "y": 8])
            guard let withText = document.doc,
                  let text = (0..<withText.layerCount).first(where: {
                      document.layerDescribesSource($0) }) else {
                throw ProbeFailure(description: "\(tag) add_text_layer made no text layer")
            }
            controller.setSelectedLayers(.single(text))
            let refused = try agentCall("transform_again", document)
            try require(refused.isError && refused.text.contains(AgentServer.truePerspectiveRefusal)
                        && document.doc === withText,
                        "\(tag) MCP refuses without rasterize_artwork: \(refused.text)")
            let authorized = try agentOK("transform_again", document, ["rasterize_artwork": true])
            try require(authorized["changed"] as? Bool == true
                        && (authorized["rasterized_layers"] as? [Int]) == [text]
                        && document.doc?.layerMeta(text) == nil,
                        "\(tag) rasterize_artwork rasterizes the text layer: \(authorized)")
            document.undoManager?.undo()
            try require(document.doc === withText, "\(tag) undo the authorized repeat")
            controller.setSelectedLayers(.single(text))
            let before = AutomationAlerts.recorded.count
            controller.transformAgain(nil)
            try require(AutomationAlerts.recorded.count == before + 1
                        && document.doc !== withText && document.doc?.layerMeta(text) == nil,
                        "\(tag) ⇧⌘T prompts once and rasterizes: "
                        + "\(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
            document.undoManager?.undo()
            try require(document.doc === withText, "\(tag) undo the UI repeat")
            document.undoManager?.undo()
            try require(document.doc === once, "\(tag) undo add_text_layer")
            controller.setSelectedLayers(.single(0))
        }

        // A memory whose horizon falls inside the canvas, replayed above it.
        _ = try agentOK("select_rect", document, ["x": 20, "y": 20, "width": 10, "height": 10])
        let horizonQuad = [CGPoint(x: 20, y: 20), CGPoint(x: 30, y: 20),
                           CGPoint(x: 26, y: 30), CGPoint(x: 24, y: 30)]
        _ = try agentOK("transform_selected_pixels", document,
                        ["corners": cornersArgument(horizonQuad)])
        try require(document.lastFreeTransform?.perspective
                    == .init(sourceRect: CGRect(x: 20, y: 20, width: 10, height: 10),
                             quad: horizonQuad),
                    "\(tag) the trapezoid is remembered: "
                    + "\(String(describing: document.lastFreeTransform))")
        guard let memory = document.lastFreeTransform else {
            throw ProbeFailure(description: "\(tag) no trapezoid memory")
        }
        _ = try agentOK("select_rect", document, ["x": 20, "y": 0, "width": 10, "height": 10])
        guard let folded = document.doc else { throw ProbeFailure(description: "\(tag) folded") }
        let foldedBounds = selectionBounds(document)
        let pixelRefusal = try agentCall("transform_again", document)
        try require(pixelRefusal.isError && pixelRefusal.text.contains(horizonPixels),
                    "\(tag) MCP refuses the folded selection in band: \(pixelRefusal.text)")
        try require(document.doc === folded && selectionBounds(document) == foldedBounds
                    && document.lastFreeTransform == memory,
                    "\(tag) the MCP refusal changed nothing")
        var alerts = AutomationAlerts.recorded.count
        controller.transformAgain(nil)
        try require(AutomationAlerts.recorded.count == alerts + 1
                    && AutomationAlerts.recorded.last?.messageText.contains(horizonPixels) == true,
                    "\(tag) ⇧⌘T over the folded selection is one alert: "
                    + "\(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
        try require(document.doc === folded && selectionBounds(document) == foldedBounds
                    && document.lastFreeTransform == memory,
                    "\(tag) the UI refusal changed nothing")
        _ = try agentOK("deselect", document)
        guard let layerBase = document.doc else { throw ProbeFailure(description: "\(tag) layer") }
        let layerRefusal = try agentCall("transform_again", document)
        try require(layerRefusal.isError
                    && layerRefusal.text.contains(RepeatableTransform.perspectiveRefusal),
                    "\(tag) MCP refuses the folded layer with one sentence: \(layerRefusal.text)")
        alerts = AutomationAlerts.recorded.count
        controller.transformAgain(nil)
        try require(AutomationAlerts.recorded.count == alerts + 1
                    && AutomationAlerts.recorded.last?.messageText
                        .contains(RepeatableTransform.perspectiveRefusal) == true,
                    "\(tag) ⇧⌘T over the folded layer is one alert with the sentence: "
                    + "\(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
        try require(document.doc === layerBase && document.lastFreeTransform == memory,
                    "\(tag) the layer refusals changed nothing")
    }

    // 8 bits: a two-layer ⌘T corner drag over layers with transparent
    // margins commits the map its box previewed, and the memory replays it.
    let pair = try marginFixture()
    defer { pair.close() }
    let pairController = try editor(pair)
    let sampler = EditorViewController.samplerAgentName(pairController.transformSampler)
    guard let pairBase = pair.doc else { throw ProbeFailure(description: "margin fixture") }
    pairController.setSelectedLayers(LayerSelection(primary: 1, others: [0, 1]))
    pairController.freeTransform(nil)
    pairController.transformSession?.transform.cornerOffsets = [
        .zero, .zero, CGVector(dx: 6, dy: 4), .zero]
    guard let pairSession = pairController.transformSession else {
        throw ProbeFailure(description: "[pair] ⌘T opened no session")
    }
    let box = pairSession.sourceRect
    let pairQuad = pairSession.transform.warpedQuad(of: box)
    try require(box == CGRect(x: 10, y: 12, width: 40, height: 30),
                "[pair] the box is the content union, not the buffers: \(box)")
    try require(pairController.commitTransformSession(), "[pair] the ⌘T committed")
    let pairUI = pairState(pair)
    try require(pair.lastFreeTransform?.perspective == .init(sourceRect: box, quad: pairQuad),
                "[pair] the memory is the box's perspective")
    pair.undoManager?.undo()
    try require(pair.doc === pairBase, "[pair] undo the ⌘T")
    pairController.setSelectedLayers(LayerSelection(primary: 1, others: [0, 1]))
    _ = try agentOK("transform_layers", pair,
                    ["layers": [0, 1], "source_rect": rectArgument(box),
                     "corners": cornersArgument(pairQuad), "sampler": sampler])
    let pairAgent = pairState(pair)
    try require(pairUI == pairAgent,
                "[pair] ⌘T equals transform_layers over the session box: \(pairUI) vs \(pairAgent)")
    pair.undoManager?.undo()
    try require(pair.doc === pairBase, "[pair] undo transform_layers")
    pairController.setSelectedLayers(LayerSelection(primary: 1, others: [0, 1]))
    _ = try agentOK("transform_again", pair)
    let pairAgain = pairState(pair)
    try require(pairAgain == pairUI, "[pair] the memory replays the committed map: \(pairAgain)")
    print("PASS: transform again perspective — a true-perspective ⌘T stores its corners; ⇧⌘T is "
        + "one step recorded with rasterize_artwork, equal to transform_layers with the same "
        + "rectangle and corners and to MCP; ⌥⇧⌘T and a selection equal MCP; a text layer needs "
        + "rasterize_artwork over MCP and prompts once in the UI; a folded selection and layer "
        + "refuse in band and with one alert, changing nothing, at 8 and 16 bits; a two-layer 8-bit "
        + "corner drag commits its box's map and the memory replays it")
}
