import AppKit

// Phase 12F-1b2: the UI-SOURCE half of typed STRAIGHTEN — the Crop tool at
// 16 bits, and the zero-angle Crop-tool commit at BOTH depths.
//
// An MCP drive can call `crop` with an `angle`; it proves the core op and the
// tool gate. What only the REAL UI can prove lives here:
//
//  * that `DocumentPrecision.allows(.crop, …)` and the `selectCropTool:` menu
//    item VALIDATE in a 16-bit document (they were false until this run) and
//    in a 32-bit float one;
//  * that the tool's OWN session state — the options bar's Straighten field,
//    the box rect and the shared Free Transform sampler — reaches the commit,
//    and that the commit's document is EXACTLY what the numeric
//    `RasterDocument.straightenedCrop` wrapper answers for the same numbers;
//  * that the commit records ONE `crop` step whose `angle` is a JSON NUMBER,
//    and that replaying it on a fresh twin is hash-equal;
//  * that the BYTE commit is still the three byte wrappers composed by hand
//    here — the refactor into `straightenedCrop` moved nothing;
//  * that an ANGLE-0 commit stays on `cropped` at both depths: it publishes,
//    it makes exactly one history step, it records no `angle` and no
//    `sampler`, and a TEXT layer keeps its artwork description.
//
// The zero-angle half reaches no modal at all.

/// A probe-built menu item, the shape AppKit hands `validateUserInterfaceItem`.
private func straightenMenuItem(_ selector: Selector) -> NSMenuItem {
    let item = NSMenuItem(title: "Item", action: selector, keyEquivalent: "")
    item.target = nil
    return item
}

/// One MCP call through the REAL server, refusing to continue on an error.
@discardableResult
private func straightenMCP(_ tool: String, _ arguments: [String: Any],
                           _ document: ImageDocument) throws -> [String: Any] {
    let response = try call(tool, arguments, document: document)
    try require(!(try responseIsError(response)), "\(tool) refused: \(response)")
    guard let envelope = try JSONSerialization.jsonObject(with: Data(response.utf8))
            as? [String: Any],
          let content = envelope["content"] as? [[String: Any]],
          let text = content.first?["text"] as? String else {
        throw DoctoredCoreError(message: "Invalid MCP response: \(response)")
    }
    return (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any] ?? [:]
}

private func straightenPlacements(_ doc: RasterDocument) -> [String] {
    (0..<doc.layerCount).compactMap { index in
        guard let info = doc.layerInfo(index) else { return nil }
        return "\(info.name)@\(info.offsetX),\(info.offsetY)"
    }
}

private func straightenGuides(_ doc: RasterDocument) -> [String] {
    (0..<doc.guideCount).compactMap { index in
        guard let guide = doc.guideInfo(index) else { return nil }
        return "\(guide.orientation.rawValue):\(guide.position)"
    }
}

/// The whole comparable state of a TYPED document: canvas, placements,
/// guides, ruler origin and the digest of its exact u16 samples.
private func typedState(_ doc: RasterDocument) throws -> [String] {
    [
        "canvas:\(doc.width)x\(doc.height)",
        "layers:" + straightenPlacements(doc).joined(separator: "|"),
        "guides:" + straightenGuides(doc).joined(separator: "|"),
        "ruler:\(doc.rulerOrigin.x),\(doc.rulerOrigin.y)",
        "raw:" + hashSamples(try raw(doc)),
    ]
}

/// The byte twin of `typedState`, over the flattened composite.
private func byteState(_ doc: RasterDocument) throws -> [String] {
    [
        "canvas:\(doc.width)x\(doc.height)",
        "layers:" + straightenPlacements(doc).joined(separator: "|"),
        "guides:" + straightenGuides(doc).joined(separator: "|"),
        "ruler:\(doc.rulerOrigin.x),\(doc.rulerOrigin.y)",
        "flat:" + hashBytes(try flatBytes(doc)),
    ]
}

/// The identical matrix `commitCropSession` builds, written out HERE so the
/// comparison is against this file's own arithmetic and not the editor's.
private func straightenMatrix(_ angle: Double, _ rect: CGRect) -> CGAffineTransform {
    let center = CGPoint(x: rect.midX, y: rect.midY)
    return CGAffineTransform(translationX: center.x, y: center.y)
        .rotated(by: CGFloat(-angle) * .pi / 180)
        .translatedBy(x: -center.x, y: -center.y)
}

/// Opens the Crop tool and puts the box and the Straighten field where the
/// caller asks, through the tool's own session and options-bar bindings.
private func openCropBox(_ editor: EditorViewController, rect: CGRect, angle: Double) throws {
    editor.selectTool(.crop)
    pump()
    try require(editor.cropSession != nil, "Selecting the Crop tool opened no session")
    editor.cropSession?.rect = rect
    editor.cropStraightenDegrees = angle
    pump()
    try require(editor.cropSession?.rect == rect,
        "The crop box reads back \(String(describing: editor.cropSession?.rect))")
    try require(editor.cropStraightenDegrees == angle,
        "The Straighten field reads back \(editor.cropStraightenDegrees)")
}

/// The one recorded step a commit produced, or a failure naming what it got.
private func oneRecordedStep(_ steps: [ActionStep], _ what: String) throws -> ActionStep {
    try require(steps.count == 1,
        "\(what) recorded \(steps.count) steps: \(steps.map { $0.tool })")
    try require(steps[0].tool == "crop", "\(what) recorded a \(steps[0].tool) step")
    return steps[0]
}

// --------------------------------------------- the 16-bit straighten half --

private func typedStraightenHalf() throws -> [String: Any] {
    let (document, editor, _) = try openDocument(width: 40, height: 28)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    try require(DocumentPrecision.allows(.crop, document: document),
        "The Crop tool must be admitted in a 16-bit document")
    try require(editor.validateUserInterfaceItem(straightenMenuItem(
        #selector(EditorViewController.selectCropTool(_:)))),
        "selectCropTool: must validate in a 16-bit document")

    // Chrome and a SAVED CHANNEL through the real tools, so the commit has
    // guides, a ruler origin and a channel to carry as well as pixels.
    try straightenMCP("add_guide", ["orientation": "vertical", "position": 9.0], document)
    try straightenMCP("add_guide", ["orientation": "horizontal", "position": 6.0], document)
    try straightenMCP("set_ruler_origin", ["x": 7.0, "y": 5.0], document)
    try straightenMCP("select_rect", ["x": 3, "y": 2, "width": 12, "height": 9], document)
    try straightenMCP("save_selection", ["name": "Cover"], document)
    try straightenMCP("deselect", [:], document)

    let rect = CGRect(x: 5, y: 4, width: 28, height: 18)
    let angle = 7.5
    editor.transformSampler = DR_FILTER_BILINEAR
    try openCropBox(editor, rect: rect, angle: angle)

    guard let before = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    let beforeState = try typedState(before)
    guard let expected = try before.straightenedCrop(
        straightenMatrix(angle, rect), sampler: DR_FILTER_BILINEAR,
        x: 5, y: 4, w: 28, h: 18, described: [])?.document else {
        throw DoctoredCoreError(message: "The numeric straightenedCrop wrapper refused")
    }
    let expectedState = try typedState(expected)

    document.undoManager?.removeAllActions()
    let steps = document.history.entries.count
    ActionRecorder.shared.start()
    editor.commitCropSession()
    pump()
    let recorded = ActionRecorder.shared.stop()

    guard let after = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after the commit")
    }
    try require(after !== before, "The Crop tool commit published nothing at 16 bits")
    try require((try typedState(after)) == expectedState,
        "The committed document is \(try typedState(after)), the wrapper's is \(expectedState)")
    try require(document.history.entries.count == steps + 1,
        "The commit made \(document.history.entries.count - steps) history steps")
    try require(after.isHighDepth && !after.isFloat, "The result is no longer a 16-bit document")

    let step = try oneRecordedStep(recorded, "The 16-bit straighten commit")
    guard let angleValue = step.arguments["angle"] as? NSNumber else {
        throw DoctoredCoreError(
            message: "The recorded crop step's angle is not a number: "
                + String(describing: step.arguments["angle"]))
    }
    try require(!(step.arguments["angle"] is String),
        "The recorded crop step's angle is a string")
    try require(abs(angleValue.doubleValue - angle) < 1e-9,
        "The recorded angle is \(angleValue.doubleValue)")
    try require(step.arguments["sampler"] as? String == "bilinear",
        "The recorded sampler is \(String(describing: step.arguments["sampler"]))")

    document.undoManager?.undo()
    pump()
    guard let undone = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after Undo")
    }
    try require((try typedState(undone)) == beforeState,
        "One Undo did not restore the 16-bit document")

    // The RECORDED step replayed on a fresh twin is the same picture.
    let (twin, _, _) = try openDocument(width: 40, height: 28)
    defer { twin.close(); pump() }
    try straightenMCP("add_guide", ["orientation": "vertical", "position": 9.0], twin)
    try straightenMCP("add_guide", ["orientation": "horizontal", "position": 6.0], twin)
    try straightenMCP("set_ruler_origin", ["x": 7.0, "y": 5.0], twin)
    try straightenMCP("select_rect", ["x": 3, "y": 2, "width": 12, "height": 9], twin)
    try straightenMCP("save_selection", ["name": "Cover"], twin)
    try straightenMCP("deselect", [:], twin)
    let replayed = try replay(step, document: twin)
    try require(!(try responseIsError(replayed)), "The recorded crop step refused on replay")
    guard let twinResult = twin.doc else {
        throw DoctoredCoreError(message: "Owned twin handle missing after replay")
    }
    try require((try typedState(twinResult)) == expectedState,
        "The replayed step's document is \(try typedState(twinResult))")

    // A STRAIGHTEN-ONLY commit: the full box at -3 degrees keeps the canvas.
    guard let base = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing")
    }
    let full = CGRect(x: 0, y: 0, width: CGFloat(base.width), height: CGFloat(base.height))
    try openCropBox(editor, rect: full, angle: -3.0)
    let canvasBefore = (base.width, base.height)
    editor.commitCropSession()
    pump()
    guard let straightenedOnly = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after the straighten-only commit")
    }
    try require(straightenedOnly !== base, "The straighten-only commit published nothing")
    try require((straightenedOnly.width, straightenedOnly.height) == canvasBefore,
        "The straighten-only commit changed the canvas to "
        + "\(straightenedOnly.width)x\(straightenedOnly.height)")

    return ["canvas": [after.width, after.height],
            "equals_numeric_wrapper": true,
            "recorded_angle": angleValue.doubleValue,
            "recorded_sampler": "bilinear",
            "history_steps": 1, "undo_restored": true, "replay_equal": true,
            "straighten_only_canvas": [straightenedOnly.width, straightenedOnly.height]]
}

// ------------------------------------------------- the byte straighten half --

private func byteStraightenHalf() throws -> [String: Any] {
    let (document, editor, _) = try openByteDocument(width: 40, height: 28)
    defer { document.close(); pump() }
    try straightenMCP("add_guide", ["orientation": "vertical", "position": 9.0], document)
    try straightenMCP("set_ruler_origin", ["x": 7.0, "y": 5.0], document)
    try straightenMCP("select_rect", ["x": 3, "y": 2, "width": 12, "height": 9], document)
    try straightenMCP("save_selection", ["name": "Cover"], document)
    try straightenMCP("deselect", [:], document)

    let rect = CGRect(x: 5, y: 4, width: 28, height: 18)
    let angle = 7.5
    editor.transformSampler = DR_FILTER_BILINEAR
    try openCropBox(editor, rect: rect, angle: angle)

    guard let before = document.doc else {
        throw DoctoredCoreError(message: "Owned byte document handle missing")
    }
    // THE BYTE `straighten` REQUEST, sent HERE directly (no job): this is
    // what the commit's background job must equal.
    let matrix = straightenMatrix(angle, rect)
    guard let cropped = try before.editingGeometry(request: PrecisionPixelCommand.straighten(
        matrix: matrix, filter: .bilinear, x: 5, y: 4, width: 28, height: 18).request)
    else {
        throw DoctoredCoreError(message: "The byte straighten request refused")
    }
    let expectedState = try byteState(cropped)
    let beforeState = try byteState(before)

    document.undoManager?.removeAllActions()
    editor.commitCropSession()
    pump()
    guard let after = document.doc else {
        throw DoctoredCoreError(message: "Owned byte document handle missing after the commit")
    }
    try require((try byteState(after)) == expectedState,
        "The byte commit is \(try byteState(after)), the direct request says \(expectedState)")
    document.undoManager?.undo()
    pump()
    guard let undone = document.doc else {
        throw DoctoredCoreError(message: "Owned byte document handle missing after Undo")
    }
    try require((try byteState(undone)) == beforeState, "One Undo did not restore the byte document")
    return ["canvas": [after.width, after.height], "equals_composed_wrappers": true,
            "undo_restored": true]
}

// -------------------------------------------- the zero-angle half, S3 --

/// The angle-0 commit on a document carrying a TEXT layer: it must publish,
/// make exactly one history step, record no `angle` and no `sampler`, keep the
/// text layer's artwork description, and land on exactly `cropped`'s pixels.
private func zeroAngleHalf(typed: Bool) throws -> [String: Any] {
    let (document, editor, _) = typed
        ? try openDocument(width: 40, height: 28)
        : try openByteDocument(width: 40, height: 28)
    defer {
        if ActionRecorder.shared.isRecording { _ = ActionRecorder.shared.stop() }
        document.close(); pump()
    }
    try straightenMCP("add_text_layer", ["text": "Zero", "x": 6, "y": 8, "size": 12], document)
    guard let seeded = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after add_text_layer")
    }
    guard let textIndex = (0..<seeded.layerCount).first(where: {
        document.layerDescribesSource($0)
    }) else {
        throw DoctoredCoreError(message: "No described layer after add_text_layer")
    }
    guard let description = seeded.layerMeta(textIndex) else {
        throw DoctoredCoreError(message: "The text layer carries no description")
    }
    guard let expected = try seeded.cropped(x: 5, y: 4, w: 28, h: 18) else {
        throw DoctoredCoreError(message: "The numeric cropped wrapper refused")
    }
    let expectedHash = typed ? hashSamples(try raw(expected)) : hashBytes(try flatBytes(expected))

    let rect = CGRect(x: 5, y: 4, width: 28, height: 18)
    try openCropBox(editor, rect: rect, angle: 0)
    let beforeHash = typed ? hashSamples(try raw(seeded)) : hashBytes(try flatBytes(seeded))
    document.undoManager?.removeAllActions()
    let steps = document.history.entries.count
    ActionRecorder.shared.start()
    editor.commitCropSession()
    pump()
    let recorded = ActionRecorder.shared.stop()

    guard let after = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after the zero-angle commit")
    }
    // The beep path is `document.doc === before`: a zero-angle straighten
    // answers nil and nothing is published.
    try require(after !== seeded, "The zero-angle commit published nothing (the beep path)")
    try require(document.history.entries.count == steps + 1,
        "The zero-angle commit made \(document.history.entries.count - steps) history steps")
    let hash = typed ? hashSamples(try raw(after)) : hashBytes(try flatBytes(after))
    try require(hash == expectedHash,
        "The zero-angle commit is not `cropped`'s picture")
    try require((after.width, after.height) == (28, 18),
        "The zero-angle commit left the canvas at \(after.width)x\(after.height)")
    try require(after.layerMeta(textIndex) == description,
        "The text layer's description changed: \(String(describing: after.layerMeta(textIndex)))")
    try require(document.layerDescribesSource(textIndex),
        "The text layer stopped describing its source")
    if typed {
        try require(after.isHighDepth && !after.isFloat, "The result is no longer 16-bit")
    } else {
        try require(!after.isHighDepth, "The 8-bit result changed depth")
    }

    let step = try oneRecordedStep(recorded, "The zero-angle commit")
    try require(step.arguments["angle"] == nil,
        "The zero-angle step recorded an angle: \(String(describing: step.arguments["angle"]))")
    try require(step.arguments["sampler"] == nil,
        "The zero-angle step recorded a sampler: \(String(describing: step.arguments["sampler"]))")

    document.undoManager?.undo()
    pump()
    guard let undone = document.doc else {
        throw DoctoredCoreError(message: "Owned document handle missing after Undo")
    }
    let undoneHash = typed ? hashSamples(try raw(undone)) : hashBytes(try flatBytes(undone))
    try require(undoneHash == beforeHash, "One Undo did not restore the zero-angle document")
    return ["depth": typed ? 16 : 8, "canvas": [after.width, after.height],
            "equals_cropped": true, "history_steps": 1, "description_kept": true,
            "recorded_angle_key": false, "recorded_sampler_key": false,
            "undo_restored": true]
}

// ------------------------------------------------------------- the case --

func straightenCropToolUI() throws -> [String: Any] {
    // The FLOAT document admits the tool too — its straighten resamples
    // premultiplied extended-range f32 in the core.
    let floatAnswers: [String: Any] = try {
        let (document, editor, _) = try openFloatDocument()
        defer { document.close(); pump() }
        try require(DocumentPrecision.allows(.crop, document: document),
            "The Crop tool must be admitted in a 32-bit float document")
        try require(editor.validateUserInterfaceItem(straightenMenuItem(
            #selector(EditorViewController.selectCropTool(_:)))),
            "selectCropTool: must validate in a 32-bit float document")
        return ["allows_crop": true, "select_crop_tool_validates": true]
    }()

    let typed = try typedStraightenHalf()
    let byte = try byteStraightenHalf()
    let zeroTyped = try zeroAngleHalf(typed: true)
    let zeroByte = try zeroAngleHalf(typed: false)

    return ["case": "straighten-crop-tool-ui",
            "float": floatAnswers,
            "typed_straighten": typed,
            "byte_straighten": byte,
            "zero_angle": [zeroTyped, zeroByte],
            "scope": "The REAL Crop tool driven at 16 bits and at 8: the straighten commit "
                + "is compared with RasterDocument.straightenedCrop (typed) and with the "
                + "three byte wrappers composed in this file (byte), and the zero-angle "
                + "commit is compared with `cropped` at both depths with a text layer "
                + "present so the description rule is measured, not assumed"]
}
