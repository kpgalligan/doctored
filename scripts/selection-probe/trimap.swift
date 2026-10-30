import AppKit

private func trimapMustRefuse(_ body: () throws -> Void, _ message: String) throws {
    var refused = false
    do { try body() } catch { refused = true }
    try require(refused, message)
}

func verifyTrimapModel() throws {
    let document = try fixture(width: 11, height: 9)
    defer { document.close() }
    let native = document.doc, initial = document.selectionHistory
    let capture = try SelectionCapture(document: document, needsSource: false)
    let session = try RefinementTrimapSession(width: 11, height: 9)
    let empty = try session.snapshot()
    try session.begin(label: .foreground, radius: 2, at: CGPoint(x: 2.5, y: 4.5))
    try session.append(CGPoint(x: 8.5, y: 4.5))
    try require(!session.isCurrent(empty) && !session.canUndo, "Draft invalidates previews and disables local undo")
    try trimapMustRefuse({ _ = try session.snapshot() }, "Unfinished draft cannot enter replay or Apply")
    try session.finish()
    let first = try session.snapshot()
    try session.begin(label: .background, radius: 1, at: CGPoint(x: 5.5, y: 4.5))
    try session.finish()
    try session.begin(label: .unknown, radius: 0.5, at: CGPoint(x: 5.5, y: 4.5))
    try session.finish()
    let complete = try session.snapshot()
    let replay = try RefinementTrimap(data: complete.trimap.encodedData())
    try require(replay == complete.trimap, "Categorical replay preserves labels, ordering, radius and fractional coordinates")
    let base = [UInt8](repeating: 128, count: 99)
    let token = try RasterJobControl()
    let coreScratch = try replay.coreScratchBytes()
    try require(token.setScratchLimit(coreScratch), "Bounded trimap token admission")
    let painted = try RasterImage.applyingTrimapStrokes(base, replay: replay, control: token)
    try require(painted[4 * 11 + 2] == 255 && painted[4 * 11 + 4] == 0
                && painted[4 * 11 + 5] == 128 && painted[0] == 128,
                "Core bridge paints known constraints, explicitly relaxes Unknown, and preserves untouched base")
    try require(token.scratchBytes == 0, "Completed constructor releases scratch")
    try require(session.undo() && session.undo(), "Local stroke Undo traverses two edits")
    let undone = try session.snapshot()
    try require(undone.trimap == first.trimap && !session.isCurrent(first),
                "Undo restores replay geometry but cannot revive a stale preview revision")
    try require(session.redo() && session.redo(), "Local Redo restores categorical paint order")
    let redone = try session.snapshot()
    try require(redone.trimap == replay, "Redo restores complete replay")
    _ = session.undo()
    try session.begin(label: .foreground, radius: 0.5, at: CGPoint(x: 0.5, y: 0.5))
    session.cancel()
    try require(session.canRedo, "Cancelling draft preserves local redo branch")
    try session.begin(label: .foreground, radius: 0.5, at: CGPoint(x: 0.5, y: 0.5))
    try session.finish()
    try require(!session.canRedo && !session.isCurrent(complete), "New committed stroke discards redo and invalidates preview")
    let other = try RefinementTrimapSession(width: 11, height: 9)
    try require(!other.isCurrent(empty), "Separate sessions never accept each other's preview tokens")
    try require(document.doc === native && document.selectionHistory == initial
                && !document.isDocumentEdited && document.undoManager?.canUndo != true,
                "Local categorical history never changes document pixels, dirty state or global Undo")
    document.setCanvasSelection(CanvasSelection(shape: .rect(CGRect(x: 1, y: 1, width: 4, height: 4)),
        canvasWidth: 11, canvasHeight: 9))
    try require(!capture.isCurrent(in: document), "Existing selection capture independently rejects stale coverage")
    print("PASS: categorical replay bridge, explicit Unknown, bounded local Undo/Redo, session revision and captured document isolation")

    var object = try JSONSerialization.jsonObject(with: replay.encodedData()) as! [String: Any]
    for (key, value) in [("version", 2 as Any), ("unknown_key", 0), ("width", true)] {
        var invalid = object; invalid[key] = value
        let data = try JSONSerialization.data(withJSONObject: invalid)
        try trimapMustRefuse({ _ = try RefinementTrimap(data: data) }, "Invalid replay metadata refuses: \(key)")
    }
    object["strokes"] = [["label": "foreground", "radius": true, "points": [[0, 0]]]]
    try trimapMustRefuse({ _ = try RefinementTrimap(data: JSONSerialization.data(withJSONObject: object)) },
                        "Boolean radius cannot masquerade as a numeric brush value")
    try trimapMustRefuse({ _ = try RefinementTrimap(width: Int.max, height: Int.max) }, "Canvas overflow refuses")
    try trimapMustRefuse({ _ = try RefinementTrimapStroke(label: .unknown, radius: .nan, points: [.zero]) },
                        "Nonfinite brush radius refuses")
    let cancelled = try RasterJobControl(); cancelled.cancel()
    try trimapMustRefuse({ _ = try RasterImage.applyingTrimapStrokes(base, replay: replay, control: cancelled) },
                        "Cancelled constructor never succeeds")
    try require(cancelled.scratchBytes == 0 && base.allSatisfy { $0 == 128 }, "Refusal releases scratch and preserves base")

    let bounded = try RefinementTrimapSession(width: 1, height: 1)
    for _ in 0..<8 {
        try bounded.begin(label: .unknown, radius: 0.5, at: .zero)
        for _ in 1..<8192 { try bounded.append(.zero) }
        try bounded.finish()
    }
    try require(bounded.retainedPointCount == 65_536, "Committed geometry reaches its exact point cap")
    try bounded.begin(label: .foreground, radius: 1, at: .zero)
    try trimapMustRefuse({ try bounded.finish() }, "Single excess committed point refuses atomically")
    bounded.cancel()
    try require(bounded.retainedPointCount == 65_536, "Rejected draft does not change committed geometry")
    _ = bounded.undo()
    try bounded.begin(label: .background, radius: 1, at: .zero); try bounded.finish()
    try require(bounded.retainedPointCount == 7 * 8192 + 1 && !bounded.canRedo,
                "Branching releases old redo points before admitting replacement")
    print("PASS: typed replay validation, cancellation release and exact 65536-point retention/redo branch bounds")
}

func verifyAdvancedRefinementGate() throws {
    let document = try fixture()
    defer { document.close() }
    document.setCanvasSelection(CanvasSelection(shape: .rect(CGRect(x: 8, y: 8, width: 21, height: 25)),
        canvasWidth: 90, canvasHeight: 60))
    document.undoManager?.removeAllActions()
    let original = document.selectionHistory, native = document.doc
    // `true` is no valid value for any of these: `brush` must be an array of strokes,
    // `decontaminate` needs a New Layer output (the default is selection), and the rest are not
    // parameters of the tool at all.
    for key in ["trimap", "trimap_strokes", "foreground", "color_space", "brush", "decontaminate", "typo"] {
        let arguments: [String: Any] = ["document_id": AgentServer.shared.documentID(for: document), key: true]
        let data = try JSONSerialization.data(withJSONObject: arguments)
        let raw = AgentServer.shared.execute(tool: "refine_selection", argumentsJSON: String(decoding: data, as: UTF8.self))
        let result = try JSONSerialization.jsonObject(with: Data(raw.utf8)) as! [String: Any]
        try require(result["isError"] as? Bool == true, "Malformed/unknown refinement parameter must refuse: \(key)")
    }
    try require(document.selectionHistory == original && document.doc === native
                && document.undoManager?.canUndo != true, "Gate refusal is atomically clean")
    _ = try call(document, "refine_selection", ["feather": 2])
    try require(document.selectionHistory != original && document.doc === native && !document.isDocumentEdited,
                "Existing basic refinement still changes only transient coverage")
    document.undoManager?.undo()
    try require(document.selectionHistory == original && document.undoManager?.canUndo != true,
                "Basic MCP refinement retains one-step Undo")
    print("PASS: malformed brush, decontaminate without a layer output and unknown MCP parameters refuse; basic refinement remains one clean Undo")
}
