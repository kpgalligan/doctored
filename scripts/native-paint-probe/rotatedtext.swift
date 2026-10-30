import AppKit

// A rotated or scaled text layer is edited IN PLACE. The on-canvas
// editor sits in a host view turned and scaled by the layer's map, so the
// text view's own space lands on the canvas exactly where the committed
// glyphs do; the text tool's hit test uses the transformed quad; and the
// commit keeps the layer's transform and exact anchor. Mirrored, off-canvas
// and untransformed layers keep the upright editor at the earlier clamp.

/// The line fragments a layout manager has laid out.
private func rotatedLineCount(_ manager: NSLayoutManager) -> Int {
    var count = 0
    var glyph = 0
    while glyph < manager.numberOfGlyphs {
        var range = NSRange(location: 0, length: 0)
        _ = manager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: &range)
        count += 1
        glyph = max(NSMaxRange(range), glyph + 1)
    }
    return count
}

/// The committed layer's line count: its description laid out in its own
/// box, the way the commit lays it out (`TextLayer.wrapWidth`).
private func rotatedCommittedLines(_ payload: TextLayerPayload) throws -> Int {
    guard let wrap = TextLayer.wrapWidth(payload.box) else {
        throw DoctoredCoreError(message: "Committed box is unresolved")
    }
    let storage = NSTextStorage(attributedString: payload.attributedString(style: payload.style))
    let manager = NSLayoutManager()
    let container = NSTextContainer(size: NSSize(width: wrap, height: TextLayer.unboundedWidth))
    container.lineFragmentPadding = 0
    manager.addTextContainer(container)
    storage.addLayoutManager(manager)
    manager.ensureLayout(for: container)
    return withExtendedLifetime(storage) { rotatedLineCount(manager) }
}

/// The adversary's example: a 120 × 36 editor at 45° with
/// its anchor at (250, −80) grazes a 240 × 180 canvas with its bounding box
/// only, so it counts as off the canvas.
private func rotatedPlacementPure(canvas: ImageCanvasView) throws -> [String: Any] {
    let turn = Double.pi / 4
    let map = LinearMap(a: cos(turn), b: sin(turn), c: -sin(turn), d: cos(turn))
    let placement = TextSessionPlacement(map: map)
    try require(placement.rotationDegrees == 45 && placement.scale == 1,
        "45° map placed at \(placement.rotationDegrees)° × \(placement.scale)")
    let quad = placement.quad(origin: CGPoint(x: 250, y: -80), size: CGSize(width: 120, height: 36))
    let canvasRect = CGRect(x: 0, y: 0, width: 240, height: 180)
    let xs = quad.map(\.x), ys = quad.map(\.y)
    let box = CGRect(x: xs.min() ?? 0, y: ys.min() ?? 0,
                     width: (xs.max() ?? 0) - (xs.min() ?? 0), height: (ys.max() ?? 0) - (ys.min() ?? 0))
    try require(box.intersects(canvasRect), "The example's bounding box misses the canvas")
    try require(!TextSessionPlacement.convexQuad(quad, overlaps: canvasRect),
        "A quad entirely off the canvas counted as overlapping")
    let inside = placement.quad(origin: CGPoint(x: 100, y: 50), size: CGSize(width: 120, height: 36))
    try require(TextSessionPlacement.convexQuad(inside, overlaps: canvasRect),
        "A quad on the canvas counted as off it")
    try require(canvas.bounds.size == canvasRect.size, "Canvas is \(canvas.bounds.size)")
    let presented = canvas.textSessionPresentation(
        at: CGPoint(x: 250, y: -80), size: CGSize(width: 120, height: 36), placement: placement,
        editing: true)
    try require(presented.placement == .identity && presented.origin == CGPoint(x: 120, y: 0),
        "Off-canvas quad presented at \(presented.origin) under \(presented.placement)")
    let mirrored = TextSessionPlacement(map: LinearMap(a: -1, b: 0, c: 0, d: 1))
    try require(mirrored == .identity, "A mirrored map did not fall back to the identity")
    return ["off_canvas_quad_upright": true, "bbox_intersects": true]
}

/// rotated-text-editor-ui.
func rotatedTextEditorUI() throws -> [String: Any] {
    var trials: [[String: Any]] = []
    var pure: [String: Any] = [:]
    let wire: [Double] = [1.5 * cos(Double.pi / 6), -1.5 * sin(Double.pi / 6),
                          1.5 * sin(Double.pi / 6), 1.5 * cos(Double.pi / 6)]
    guard let map = LinearMap(array: wire) else {
        throw DoctoredCoreError(message: "Probe map invalid")
    }
    for typed in [false, true] {
        for point in [false, true] {
            let (document, editor, _) = typed
                ? try openDocument(width: 240, height: 180)
                : try openByteDocument(width: 240, height: 180)
            defer {
                editor.canvas.cancelTextSession()
                editor.setViewRotation(0)
                document.close(); pump()
            }
            let canvas = editor.canvas
            if pure.isEmpty { pure = try rotatedPlacementPure(canvas: canvas) }
            let label = "\(typed ? "16" : "8")-bit \(point ? "point" : "box")"
            let added = try geometryCall("add_text_layer",
                ["text": point ? "Turn me" : "Tilted", "x": 60, "y": 40, "size": 16,
                 "wrap_width": point ? 0 : 120, "transform": wire], document)
            guard let doc = document.doc else { throw DoctoredCoreError(message: "Document missing") }
            let idx = added["layer"] as? Int ?? document.activeLayerIndex
            guard let payload = doc.textPayload(idx), let anchor = doc.describedAnchor(idx) else {
                throw DoctoredCoreError(message: "\(label): no text layer at \(idx)")
            }
            let stored = payload.transform
            try require(zip(stored.array, map.array).allSatisfy { abs($0 - $1) <= 1e-12 },
                "\(label): stored map \(stored.array)")

            // (2) the text view's space lands on the canvas through the map.
            func mapCheck(_ step: String) throws {
                guard let textView = canvas.activeTextView else {
                    throw DoctoredCoreError(message: "\(label) \(step): no session")
                }
                let b = textView.bounds
                for p in [CGPoint(x: 0, y: 0), CGPoint(x: b.width, y: 0), CGPoint(x: 0, y: b.height)] {
                    let got = canvas.convert(p, from: textView)
                    let q = stored.apply(p)
                    let want = CGPoint(x: anchor.x + q.x, y: anchor.y + q.y)
                    try require(abs(got.x - want.x) <= 1e-6 && abs(got.y - want.y) <= 1e-6,
                        "\(label) \(step): \(p) → \(got), want \(want)")
                }
            }
            editor.editTextLayer(idx); pump()
            try require(canvas.textSessionHost?.placement == TextSessionPlacement(map: stored)
                && TextSessionPlacement(map: stored).rotationDegrees == 30
                && TextSessionPlacement(map: stored).scale == 1.5,
                "\(label): session opened upright")
            try require(canvas.textSessionOrigin == anchor, "\(label): session not at the anchor")
            try mapCheck("open")

            // (3) a turned view changes neither the map nor the hit.
            if !typed {
                editor.setViewRotation(-30); editor.view.layoutSubtreeIfNeeded(); pump()
                try mapCheck("view -30°")
                guard let textView = canvas.activeTextView, let window = textView.window,
                      let content = window.contentView else {
                    throw DoctoredCoreError(message: "\(label): window missing")
                }
                let centre = textView.convert(
                    CGPoint(x: textView.bounds.midX, y: textView.bounds.midY), to: nil)
                let hit = content.hitTest(content.superview?.convert(centre, from: nil) ?? centre)
                try require(hit.map { $0 === textView || $0.isDescendant(of: textView) } == true,
                    "\(label): centre hit \(String(describing: hit))")
                editor.setViewRotation(0); editor.view.layoutSubtreeIfNeeded(); pump()
            }

            // (4) an unchanged ⌘Return registers nothing.
            let before = document.doc
            let steps = document.history.entries.count
            try require(canvas.commitTextSession(), "\(label): unchanged commit refused")
            pump()
            try require(document.doc === before && document.history.entries.count == steps,
                "\(label): an unchanged commit edited the document")

            // (5) a real edit re-syncs the host and keeps the map and anchor.
            editor.editTextLayer(idx); pump()
            guard let textView = canvas.activeTextView else {
                throw DoctoredCoreError(message: "\(label): reopen failed")
            }
            let length = (textView.string as NSString).length
            textView.setSelectedRange(NSRange(location: length, length: 0))
            textView.insertText("\nOK", replacementRange: NSRange(location: NSNotFound, length: 0))
            pump()
            try mapCheck("after typing")
            try require(canvas.commitTextSession(), "\(label): edit refused")
            pump()
            try require(document.history.entries.count == steps + 1
                && document.history.entries.last?.name == "Edit Text Layer",
                "\(label): edit history \(document.history.entries.map(\.name))")
            guard let edited = document.doc, let after = edited.textPayload(idx) else {
                throw DoctoredCoreError(message: "\(label): edited layer missing")
            }
            try require(after.transform == payload.transform
                && edited.describedAnchor(idx) == anchor && after.string.hasSuffix("\nOK"),
                "\(label): edit moved the map or anchor")

            // Objection 1: append to the longest line; the editor never wraps
            // what the commit lays out on one line.
            editor.editTextLayer(idx); pump()
            guard let longer = canvas.activeTextView, let manager = longer.layoutManager,
                  let container = longer.textContainer else {
                throw DoctoredCoreError(message: "\(label): reopen for append failed")
            }
            let firstBreak = (longer.string as NSString).range(of: "\n").location
            longer.setSelectedRange(NSRange(location: firstBreak, length: 0))
            longer.insertText(" wider", replacementRange: NSRange(location: NSNotFound, length: 0))
            pump()
            manager.ensureLayout(for: container)
            let editorLines = rotatedLineCount(manager)
            try mapCheck("after append")
            try require(canvas.commitTextSession(), "\(label): append refused")
            pump()
            guard let appended = document.doc?.textPayload(idx) else {
                throw DoctoredCoreError(message: "\(label): appended layer missing")
            }
            let committedLines = try rotatedCommittedLines(appended)
            try require(editorLines == committedLines,
                "\(label): editor shows \(editorLines) lines, commit \(committedLines)")
            if point {
                try require(committedLines == 2, "\(label): point text wrapped to \(committedLines)")
            }

            var trial: [String: Any] = ["label": label, "editor_lines": editorLines,
                                        "committed_lines": committedLines]
            if !typed {
                // (6) the hit test uses the quad, not the bounding box.
                guard let current = document.doc, let info = current.layerInfo(idx),
                      let description = current.layerDescription(idx),
                      let source = description.sourceRect(),
                      let currentAnchor = current.describedAnchor(idx) else {
                    throw DoctoredCoreError(message: "\(label): layer geometry missing")
                }
                let corner = CGPoint(x: CGFloat(info.offsetX) + 1.5, y: CGFloat(info.offsetY) + 1.5)
                try require(editor.topmostTextLayer(at: corner, in: current) == nil,
                    "\(label): the bounding box's corner hit the layer")
                let quad = description.transform.quad(of: source, anchor: currentAnchor)
                let centroid = CGPoint(x: quad.map(\.x).reduce(0, +) / 4,
                                       y: quad.map(\.y).reduce(0, +) / 4)
                try require(editor.topmostTextLayer(at: centroid, in: current) == idx,
                    "\(label): the quad's centroid missed the layer")

                // (7) a mirrored and (8) an untransformed layer open upright at
                // the clamp.
                for (name, transform) in [("mirrored", [-1.0, 0, 0, 1]), ("upright", [1.0, 0, 0, 1])] {
                    let extra = try geometryCall("add_text_layer",
                        ["text": "Flat", "x": 150, "y": 120, "size": 16, "wrap_width": 120,
                         "transform": transform], document)
                    guard let flatDoc = document.doc else { throw DoctoredCoreError(message: "Doc missing") }
                    let flat = extra["layer"] as? Int ?? document.activeLayerIndex
                    guard let flatAnchor = flatDoc.describedAnchor(flat) else {
                        throw DoctoredCoreError(message: "\(label) \(name): no anchor")
                    }
                    let flatSteps = document.history.entries.count
                    editor.editTextLayer(flat); pump()
                    guard let flatView = canvas.activeTextView else {
                        throw DoctoredCoreError(message: "\(label) \(name): no session")
                    }
                    let height = canvas.textStyle.nsFont.pointSize * 1.5
                    let clamp = CGPoint(
                        x: max(0, min(flatAnchor.x, canvas.bounds.width - 120)),
                        y: max(0, min(flatAnchor.y, canvas.bounds.height - height)))
                    try require(canvas.textSessionHost?.placement == .identity
                        && canvas.textSessionOrigin == clamp
                        && canvas.convert(CGPoint.zero, from: flatView) == clamp,
                        "\(label) \(name): opened at \(String(describing: canvas.textSessionOrigin)), "
                            + "want \(clamp)")
                    try require(canvas.commitTextSession() && document.history.entries.count == flatSteps,
                        "\(label) \(name): unchanged commit edited")
                    pump()
                }
                trial["quad_hit_test"] = true
                trial["upright_fallbacks"] = ["mirrored", "upright"]
            }
            trials.append(trial)
        }
    }
    return ["case": "rotated-text-editor-ui", "pure": pure, "trials": trials]
}
