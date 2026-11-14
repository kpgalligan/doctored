import AppKit

// The Paths panel's Convert to Shape Layer and Combine with Path… (EditorViewController+
// PathCommands.swift), run off screen through the editor's own methods: their validation, their
// one undo step, the step each records and its replay, and a path shape's re-edit session.

/// A curved blob with a square hole, even-odd: the path the conversion is checked on.
private let pathCommandBlob: [[String: Any]] = [
    ["closed": true, "anchors": [
        ["x": 60.25, "y": 30.5, "in": [30.25, 30.5], "out": [90.25, 30.5], "smooth": true],
        ["x": 130.75, "y": 80.25, "in": [130.75, 50.25], "out": [130.75, 110.25], "smooth": true],
        ["x": 60.5, "y": 130.125, "in": [90.5, 130.125], "out": [30.5, 130.125], "smooth": true],
        [20.5, 80.75],
    ]],
    ["closed": true, "anchors": [[50, 60], [90, 60], [90, 100], [50, 100]]],
]

/// The pixels, placement and description of `layer`.
private func pathCommandLayerState(_ document: ImageDocument, _ layer: Int) -> [String] {
    let info = document.doc?.layerInfo(layer)
    let pixels = document.doc?.layerPixels(layer) ?? []
    return [document.doc?.layerMeta(layer) ?? "-",
            "\(info.map { [$0.offsetX, $0.offsetY, $0.width, $0.height] } ?? [])",
            "\(pixels.count):\(pixels.reduce(0) { ($0 &* 31) &+ Int($1) })"]
}

func verifyConvertPathToShape() throws {
    let tag = "[convert path to shape]"
    let document = try fixture(width: 200, height: 160)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let selector = "convertPathToShape:"
    try require(!uiValidates(controller, selector), "\(tag) the item is off with no current path")
    _ = try agentOK("set_path", document, ["subpaths": pathCommandBlob, "fill_rule": "evenodd"])
    guard let work = document.doc?.paths.workPath else {
        throw ProbeFailure(description: "\(tag) no work path")
    }
    controller.setCurrentPath(.path(work.id))
    try require(uiValidates(controller, selector), "\(tag) the item is on with a drawn path")
    document.undoManager?.removeAllActions()

    let before = document.doc
    let layers = before?.layerCount ?? 0
    let steps = uiRecorded { controller.convertPathToShape(nil) }
    guard let after = document.doc, after !== before, after.layerCount == layers + 1 else {
        throw ProbeFailure(description: "\(tag) the command added no layer: \(uiAlerts())")
    }
    let index = document.activeLayerIndex
    guard let payload = after.shapePayload(index), let outline = payload.path,
          let anchor = after.describedAnchor(index) else {
        throw ProbeFailure(description: "\(tag) the new layer is not a path shape")
    }
    try require(payload.kind == "path" && outline.fillRule == "evenodd",
                "\(tag) kind path with the path's rule: \(payload.kind) \(outline.fillRule)")
    // The outline placed at its anchor is the path itself (the stored path is already quantized).
    let placed = outline.shape(origin: anchor)
    let close = { (a: CGPoint, b: CGPoint) in abs(a.x - b.x) < 1e-9 && abs(a.y - b.y) < 1e-9 }
    let same = placed.subpaths.count == work.shape.subpaths.count
        && zip(placed.subpaths, work.shape.subpaths).allSatisfy { mine, theirs in
            mine.closed == theirs.closed && mine.anchors.count == theirs.anchors.count
                && zip(mine.anchors, theirs.anchors).allSatisfy {
                    close($0.point, $1.point) && close($0.handleIn, $1.handleIn)
                        && close($0.handleOut, $1.handleOut) && $0.smooth == $1.smooth
                }
        }
    try require(same, "\(tag) the outline at its anchor is the path")
    let recorded = steps.filter { $0.tool == "add_shape_layer" }
    try require(steps.count == 1 && recorded.count == 1, "\(tag) one add_shape_layer step: "
                + "\(steps.map(\.tool))")
    let arguments = recorded[0].arguments
    try require(arguments["kind"] as? String == "path" && arguments["subpaths"] != nil
                && arguments["fill_rule"] as? String == "evenodd"
                && ["x", "y", "w", "h", "flipped", "radius"].allSatisfy { arguments[$0] == nil },
                "\(tag) the step records the outline and no box: \(arguments.keys.sorted())")

    // One undo step; the recorded step replayed is the same layer.
    let converted = pathCommandLayerState(document, index)
    document.undoManager?.undo()
    try require(document.doc === before, "\(tag) one undo step removes the layer")
    let replay = ActionPlayer.runSingle(recorded[0], on: document, undoName: "Replay")
    try require(replay.ok && pathCommandLayerState(document, index) == converted,
                "\(tag) the recorded step replays to the same layer: \(replay.failures)")

    // The re-edit session carries the outline; a handle drag scales it with the box.
    controller.editShapeLayer(index)
    guard let session = controller.shapeEditSession,
          controller.canvas.shapeEditOverlay?.outline == outline else {
        throw ProbeFailure(description: "\(tag) the re-edit session has no outline")
    }
    let quad = session.quad
    let right = CGPoint(x: (quad[1].x + quad[2].x) / 2, y: (quad[1].y + quad[2].y) / 2)
    controller.shapeEditMouseDown(right)
    let width = session.size.width
    controller.shapeEditMouseDragged(CGPoint(x: right.x + width, y: right.y), [])
    try require(controller.commitShapeEditSession(), "\(tag) the re-box commits: \(uiAlerts())")
    guard let reboxed = document.doc?.shapePayload(index), let scaled = reboxed.path else {
        throw ProbeFailure(description: "\(tag) the re-boxed layer lost its outline")
    }
    let factor = reboxed.w / payload.w
    try require(reboxed.kind == "path" && abs(factor - 2) < 1e-6 && reboxed.h == payload.h,
                "\(tag) the box doubled its width: \(payload.w) → \(reboxed.w)")
    let scaledRight = zip(outline.subpaths, scaled.subpaths).allSatisfy { old, new in
        zip(old.anchors, new.anchors).allSatisfy { a, b in
            abs(b[0] - a[0] * factor) < 1e-9 && abs(b[2] - a[2] * factor) < 1e-9
                && abs(b[4] - a[4] * factor) < 1e-9 && b[1] == a[1] && b[3] == a[3] && b[5] == a[5]
        }
    }
    try require(scaledRight, "\(tag) every outline x scaled with the box and no y moved")

    // A straight vertical path at x = 100.5 sits 0.5 px into a box 0.5 px wide (the box's origin
    // floors to a whole pixel): it spans no width, so neither the bottom edge nor a corner dragged
    // straight down scales its x. A 0.4 px sliver spans a width, which the bottom edge leaves alone.
    let line: [String: Any] = ["closed": false, "anchors": [[100.5, 20], [100.5, 60]]]
    let sliver: [String: Any] = [
        "closed": true, "anchors": [[100.3, 20], [100.7, 20], [100.7, 60], [100.3, 60]],
    ]
    var wrong: [String] = []
    for (name, subpath, handle) in [("line", line, 6), ("line", line, 2), ("sliver", sliver, 6)] {
        let (opened, committed) = try reboxPathShapeDown(document, controller, tag, subpath, handle)
        guard let before = opened.path, let after = committed.path else {
            throw ProbeFailure(description: "\(tag) the \(name)'s layer lost its outline")
        }
        let kept = zip(before.subpaths, after.subpaths).allSatisfy { old, new in
            zip(old.anchors, new.anchors).allSatisfy { a, b in
                b[0] == a[0] && b[2] == a[2] && b[4] == a[4] && abs(b[1] - 2 * a[1]) < 1e-9
            }
        }
        if committed.w != opened.w || committed.h != 2 * opened.h || !kept {
            wrong.append("the \(name) by handle \(handle): \(opened.w) × \(opened.h) → "
                         + "\(committed.w) × \(committed.h), x kept and y doubled: \(kept)")
        }
    }
    try require(wrong.isEmpty, "\(tag) dragging down keeps the width and every x and doubles "
                + "the height and every y: \(wrong.joined(separator: "; "))")
}

/// Adds a path shape of `subpath` (stroked, 2 px), reopens it and drags handle `handle` straight
/// down by the box's height with snapping suspended (⌃), then commits: the payload as opened and
/// as committed. The press is put on the handle itself, as at a zoom where a box under 1 px wide
/// keeps its handles apart (at 100 % one press there reaches a corner before an edge).
private func reboxPathShapeDown(
    _ document: ImageDocument, _ controller: EditorViewController, _ tag: String,
    _ subpath: [String: Any], _ handle: Int
) throws -> (opened: ShapeLayerPayload, committed: ShapeLayerPayload) {
    let made = try agentOK("add_shape_layer", document, [
        "kind": "path", "subpaths": [subpath], "stroke": "#000000", "stroke_width": 2,
    ])
    guard let index = made["layer"] as? Int, let opened = document.doc?.shapePayload(index) else {
        throw ProbeFailure(description: "\(tag) no path shape for handle \(handle): \(made)")
    }
    controller.editShapeLayer(index)
    guard var session = controller.shapeEditSession else {
        throw ProbeFailure(description: "\(tag) no re-edit session for handle \(handle)")
    }
    let press = ImageCanvasView.transformHandlePoints(session.quad)[handle]
    session.drag = .handle(handle, start: (session.anchor, session.size))
    session.pressPoint = press
    controller.shapeEditSession = session
    controller.shapeEditMouseDragged(
        CGPoint(x: press.x, y: press.y + session.size.height), [.control])
    try require(controller.commitShapeEditSession(), "\(tag) the re-box commits: \(uiAlerts())")
    guard let committed = document.doc?.shapePayload(index) else {
        throw ProbeFailure(description: "\(tag) the re-boxed layer by handle \(handle) is gone")
    }
    return (opened, committed)
}

func verifyCombinePaths() throws {
    let tag = "[combine paths]"
    let document = try fixture(width: 120, height: 80)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close()
    }
    let controller = try editor(document)
    let selector = "combinePaths:"
    let left: [[Any]] = [[5, 5], [20.5, 5], [20.5, 30], [5, 30]]
    let right: [[Any]] = [[20.5, 5], [35, 5], [35, 30], [20.5, 30]]
    _ = try agentOK("set_path", document, ["name": "A", "subpaths": [["closed": true, "anchors": left]]])
    guard let first = document.doc?.paths.entry(named: "A") else {
        throw ProbeFailure(description: "\(tag) no path A")
    }
    controller.setCurrentPath(.path(first.id))
    try require(!uiValidates(controller, selector), "\(tag) the item is off with one path")
    _ = try agentOK("set_path", document, ["name": "B", "subpaths": [["closed": true, "anchors": right]]])
    try require(uiValidates(controller, selector), "\(tag) the item is on with two")
    document.undoManager?.removeAllActions()

    let steps = uiRecorded { controller.commitCombinePaths(with: "B", operation: .unite) }
    guard let united = document.doc?.paths.entry(named: "A")?.shape else {
        throw ProbeFailure(description: "\(tag) path A is gone")
    }
    let corners = Set(united.subpaths.first?.anchors.map { "\($0.point.x),\($0.point.y)" } ?? [])
    try require(united.subpaths.count == 1 && united.anchorCount == 4
                && corners == ["5.0,5.0", "35.0,5.0", "35.0,30.0", "5.0,30.0"],
                "\(tag) A is the union, one subpath of four anchors: \(united)")
    try require(steps.count == 1 && steps[0].tool == "combine_paths"
                && steps[0].arguments["path"] as? String == "A"
                && steps[0].arguments["with"] as? String == "B"
                && steps[0].arguments["operation"] as? String == "unite",
                "\(tag) one combine_paths step: \(steps.map { "\($0.tool) \($0.arguments)" })")
    document.undoManager?.undo()
    try require(document.doc?.paths.entry(named: "A")?.shape == first.shape,
                "\(tag) one undo step restores A exactly")
}
