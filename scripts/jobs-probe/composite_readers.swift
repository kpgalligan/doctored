import AppKit

/// The 8-bit composite readers. Apply Image / Calculations from Merged,
/// their sheet previews, the Evaluation readout and Remove Red Eye read the
/// retained projection when it is the handle's own, otherwise render, and a
/// render failure answers the core's sentence (an alert in the UI, `isError`
/// over MCP) instead of a false no-op.

private func compositeResponse(_ tool: String, _ document: ImageDocument,
                               _ arguments: [String: Any]) throws -> (isError: Bool, raw: String) {
    let (response, raw) = try transformResponse(tool, document: document, arguments: arguments)
    return (response["isError"] as? Bool == true, raw)
}

private func white(_ width: Int, _ height: Int) throws -> RasterImage {
    guard let image = RasterImage.from(
        rgba: [UInt8](repeating: 255, count: width * height * 4), width: width, height: height)
    else { throw ProbeFailure(description: "White image allocation") }
    return image
}

private func stored(_ subject: Any, _ label: String) -> Any? {
    Mirror(reflecting: subject).children.first(where: { $0.label == label })?.value
}

private func maxAbs(_ difference: [String: Any]) -> Int? {
    (difference["max_abs"] as? NSNumber)?.intValue
}

/// "composite readers": (a) the retained read, (b) ChannelMath reads the
/// Merged it is handed only for its own handle, (c) the sheets capture the
/// retained projection and `projectionDifference` reads its `before`, (d) a
/// render failure is the core's sentence on every path, (e) a stale stored
/// error does not refuse a render that now succeeds.
func verifyCompositeReaders() throws {
    let document = try fixture()
    let domain = UserDefaults.argumentDomain
    let savedArguments = UserDefaults.standard.volatileDomain(forName: domain)
    defer {
        UserDefaults.standard.setVolatileDomain(savedArguments, forName: domain)
        document.close()
    }
    let controller = try editor(document)
    guard let doc = document.doc, let projection = document.projection else {
        throw ProbeFailure(description: "No retained 8-bit projection")
    }

    // (a) The retained projection is what an 8-bit reader of `doc` gets; any
    // other handle renders its own.
    let composite = try document.byteComposite(of: doc)
    try require(composite === projection && composite === document.retainedByteProjection,
                "(a) byteComposite(of: doc) is the retained projection")
    guard let other = try doc.withLayerName(0, "Other") else {
        throw ProbeFailure(description: "(a) rename produced no handle")
    }
    let otherComposite = try document.byteComposite(of: other)
    let otherDifference = try RasterDocument.compare(otherComposite, projection).maxAbs
    try require(otherComposite !== projection && otherDifference == 0,
                "(a) another handle renders its own, equal composite")

    // (b) ChannelMath: the retained Merged, none, and a mismatched handle's
    // agree; a matching all-white Merged is actually read.
    let allWhite = try white(doc.width, doc.height)
    var apply = ApplyImageParameters()
    apply.blend = DR_BLEND_MULTIPLY
    apply.opacity = 0.6
    var calc = CalculationsParameters()
    calc.source1 = CalculationsParameters.Source(layer: nil, plane: .plane(.red))
    calc.source2 = CalculationsParameters.Source(layer: nil, plane: .plane(.blue))
    calc.blend = DR_BLEND_SCREEN
    calc.opacity = 0.7
    try require(apply.readsMerged && calc.readsMerged, "(b) both parameter sets read Merged")
    func applied(_ merged: MergedComposite?) -> [UInt8]? {
        (ChannelMath.applyImage(doc, apply, merged: merged) ?? doc).layerPixels(0)
    }
    let original = doc.layerPixels(0)
    let applyRetained = applied(MergedComposite(doc: doc, image: projection))
    let applyNone = applied(nil)
    let applyMismatched = applied(MergedComposite(doc: other, image: allWhite))
    let applyWhite = applied(MergedComposite(doc: doc, image: allWhite))
    try require(applyNone != nil && applyRetained == applyNone && applyMismatched == applyNone,
                "(b) Apply Image: retained, nil and mismatched Merged agree")
    try require(applyNone != original, "(b) Apply Image changed the layer")
    // Multiply by white is the identity, so an all-white Merged leaves the layer as it was.
    try require(applyWhite != applyNone && applyWhite == original,
                "(b) Apply Image read the matching all-white Merged")
    let calcRetained = ChannelMath.calculated(
        doc, calc, merged: MergedComposite(doc: doc, image: projection))
    let calcNone = ChannelMath.calculated(doc, calc, merged: nil)
    let calcMismatched = ChannelMath.calculated(
        doc, calc, merged: MergedComposite(doc: other, image: allWhite))
    let calcWhite = ChannelMath.calculated(
        doc, calc, merged: MergedComposite(doc: doc, image: allWhite))
    try require(calcNone != nil && calcRetained == calcNone && calcMismatched == calcNone,
                "(b) Calculations: retained, nil and mismatched Merged agree")
    // Screen of white over white is white.
    try require(calcWhite != calcNone
                    && calcWhite == [UInt8](repeating: 255, count: doc.width * doc.height),
                "(b) Calculations read the matching all-white Merged")

    // (c) The sheets capture the retained projection; the MCP readout reads
    // the `before` it is handed.
    let applySheet = ApplyImageSheetController(editor: controller)
    let calcSheet = CalculationsSheetController(editor: controller)
    try require((stored(applySheet, "mergedPreview") as? MergedComposite)?.image === projection,
                "(c) Apply Image sheet captured the retained projection")
    try require((stored(calcSheet, "mergedPreview") as? MergedComposite)?.image === projection,
                "(c) Calculations sheet captured the retained projection")
    guard let evaluationSheet = EvaluationSheetController(editor: controller) else {
        throw ProbeFailure(description: "(c) Evaluation sheet did not open")
    }
    try require((stored(evaluationSheet, "retainedBefore") as? RasterImage) === projection,
                "(c) Evaluation sheet captured the retained projection")
    let whiteBefore = try AgentServer.projectionDifference(doc, doc, beforeProjection: allWhite)
    let renderedBefore = try AgentServer.projectionDifference(doc, doc, beforeProjection: nil)
    try require((maxAbs(whiteBefore) ?? 0) > 0 && maxAbs(renderedBefore) == 0,
                "(c) projectionDifference reads beforeProjection: "
                    + "\(whiteBefore), \(renderedBefore)")

    // (d) A render failure is the core's sentence on every path.
    var arguments = savedArguments
    arguments["RenderGroupPixelBudget"] = 1
    UserDefaults.standard.setVolatileDomain(arguments, forName: domain)
    for (tool, args) in [("group_layers", ["layers": [0]] as [String: Any]),
                         ("set_layer_properties", ["index": 1, "blend_mode": "normal"])] {
        let (isError, raw) = try compositeResponse(tool, document, args)
        try require(!isError, "(d) \(tool) refused: \(raw)")
    }
    document.setLayerSelection(.single(0))
    try require(document.projectionError?.contains("budget") == true
                    && document.retainedByteProjection == nil,
                "(d) the projection failed on the budget: \(document.projectionError ?? "nil")")
    guard let failing = document.doc else { throw ProbeFailure(description: "(d) no document") }
    let canUndo = document.undoManager?.canUndo
    let uiPaths: [(String, () -> Void)] = [
        ("Apply Image", { controller.applyImage(apply) }),
        ("Calculations", { controller.calculations(calc, result: .selection) }),
        ("Remove Red Eye", { controller.removeRedEye(nil) }),
    ]
    for (name, run) in uiPaths {
        let alerts = AutomationAlerts.recorded.count
        run()
        let added = AutomationAlerts.recorded.suffix(from: alerts)
        try require(added.count == 1 && added.first?.messageText.contains("budget") == true,
                    "(d) UI \(name) presented the core's sentence once: \(Array(added))")
        try require(document.doc === failing && document.undoManager?.canUndo == canUndo,
                    "(d) UI \(name) edited nothing")
    }
    let mcpPaths: [(String, [String: Any])] = [
        ("apply_image", ["layer": 0, "source": "merged", "blend_mode": "multiply"]),
        ("calculations", ["source1": "merged", "source2": "merged", "result": "selection"]),
        ("red_eye_auto", ["layer": 0]),
    ]
    for (tool, args) in mcpPaths {
        let alerts = AutomationAlerts.recorded.count
        let (isError, raw) = try compositeResponse(tool, document, args)
        try require(isError && raw.contains("budget"),
                    "(d) MCP \(tool) answered the core's sentence: \(raw)")
        try require(AutomationAlerts.recorded.count == alerts, "(d) MCP \(tool) presented no alert")
        try require(document.doc === failing, "(d) MCP \(tool) edited nothing")
    }

    // (e) The stored error is stale once the budget is lifted: a reader
    // renders instead of repeating it.
    UserDefaults.standard.setVolatileDomain(savedArguments, forName: domain)
    try require(document.projectionError != nil, "(e) the stored error is still set")
    _ = try document.byteComposite(of: failing)
    let (isError, raw) = try compositeResponse("apply_image", document, mcpPaths[0].1)
    try require(!isError, "(e) MCP apply_image renders past the stale stored error: \(raw)")
    print("PASS: composite readers — retained read, ChannelMath reads its own Merged only, "
        + "sheets and readout capture it, budget failures answer the core's sentence, "
        + "stale stored error ignored")
}
