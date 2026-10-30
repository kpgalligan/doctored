import AppKit

// Edit ▸ Stroke… with nothing selected strokes the layer's own alpha
// edge in layer space. (a) A trimmed text layer still gets the rasterize
// prompt, grows inside the canvas, and the UI equals MCP; (b) an oversized
// layer's off-canvas edge is stroked, its canvas pixels untouched. Every case
// calls methods directly, so none needs a key window.

private func strokeSettings(_ location: StrokeLocation, width: Double) -> StrokeSettings {
    StrokeSettings(
        width: width, color: NSColor(srgbRed: 0, green: 0, blue: 1, alpha: 1),
        location: location, mode: DR_BLEND_NORMAL, opacity: 100, preserveTransparency: false)
}

private func rect(_ doc: RasterDocument?, _ layer: Int) -> [Int] {
    doc?.layerInfo(layer).map { [$0.offsetX, $0.offsetY, $0.width, $0.height] } ?? []
}

private func verifyTextLayerEdge() throws {
    let tag = "[stroke text]"
    let document = try fixture(width: 200, height: 140)
    defer { document.close() }
    let controller = try editor(document)
    _ = try agentOK("add_text_layer", document, ["text": "Edge", "x": 60, "y": 50, "size": 18])
    guard let withText = document.doc,
          let text = (0..<withText.layerCount).first(where: { withText.hasArtworkDescription($0) }),
          let info = withText.layerInfo(text) else {
        throw ProbeFailure(description: "\(tag) add_text_layer made no text layer")
    }
    try require(info.offsetX >= 30 && info.offsetY >= 30
                && info.offsetX + info.width <= 170 && info.offsetY + info.height <= 110
                && info.width < 200 && info.height < 140,
                "\(tag) the text sits at least 30 px from every edge: \(rect(withText, text))")

    controller.setSelectedLayers(.single(text))
    let alerts = AutomationAlerts.recorded.count
    let steps = document.history.entries.count
    // A text layer's buffer keeps 4–8 px of transparent padding around its
    // glyphs, so the Outside band is 10 px: wide enough to leave the buffer
    // on every side (a 4 px band fits inside it and grows nothing).
    controller.applyStroke(strokeSettings(.outside, width: 10))
    try require(AutomationAlerts.recorded.count == alerts + 1
                && AutomationAlerts.recorded.last?.messageText == "Rasterize text layer?",
                "\(tag) exactly one \"Rasterize text layer?\" alert: "
                + "\(AutomationAlerts.recorded.suffix(2).map(\.messageText))")
    try require(document.doc !== withText && document.history.entries.count == steps + 1
                && document.undoManager?.undoActionName == "Stroke",
                "\(tag) one \"Stroke\" step: \(document.undoManager?.undoActionName ?? "-")")
    try require(document.doc?.layerMeta(text) == nil, "\(tag) the text layer was rasterized")
    let ui = rect(document.doc, text)
    try require(ui.count == 4 && ui[0] < info.offsetX && ui[1] < info.offsetY
                && ui[0] + ui[2] > info.offsetX + info.width
                && ui[1] + ui[3] > info.offsetY + info.height
                && ui[0] >= 0 && ui[1] >= 0 && ui[0] + ui[2] <= 200 && ui[1] + ui[3] <= 140,
                "\(tag) the rect strictly contains the old one inside the canvas: "
                + "\(ui) vs \(rect(withText, text))")
    let uiPixels = document.doc?.layerPixels(text)

    document.undoManager?.undo()
    try require(document.doc === withText, "\(tag) undo the stroke")
    let agent = try agentOK("stroke_selection", document,
                            ["layer": text, "width": 10, "location": "outside",
                             "color": "#0000FF"])
    try require(agent["outline"] as? String == "layer_edge" && agent["changed"] as? Bool == true,
                "\(tag) MCP strokes the layer edge: \(agent)")
    try require(rect(document.doc, text) == ui && document.doc?.layerPixels(text) == uiPixels,
                "\(tag) MCP lands the UI's rect and pixels: \(rect(document.doc, text)) vs \(ui)")
    document.undoManager?.undo()
    try require(document.doc === withText, "\(tag) undo the MCP stroke")
}

private func verifyOversizedLayerEdge() throws {
    let tag = "[stroke oversized]"
    let (width, height) = (200, 140)
    let document = try fixture(width: width, height: height)
    defer { document.close() }
    let controller = try editor(document)
    document.applyEdit("Move", record: .notACommand) { try $0.withLayerOffset(0, -20, 0) }
    guard let moved = document.doc, rect(moved, 0) == [-20, 0, width, height],
          let before = moved.layerPixels(0) else {
        throw ProbeFailure(description: "\(tag) the background moved 20 px left: \(rect(document.doc, 0))")
    }
    let canvasBefore = try agentOK("sample_pixel", document, ["x": 0, "y": height / 2])
    let local = { (pixels: [UInt8]?, x: Int) -> [UInt8] in
        guard let pixels else { return [] }
        let at = ((height / 2) * width + x) * 4
        return Array(pixels[at..<at + 4])
    }

    controller.setSelectedLayers(.single(0))
    controller.applyStroke(strokeSettings(.inside, width: 6))
    try require(document.doc !== moved && document.undoManager?.undoActionName == "Stroke",
                "\(tag) one \"Stroke\" step")
    let canvasAfter = try agentOK("sample_pixel", document, ["x": 0, "y": height / 2])
    try require(NSDictionary(dictionary: canvasAfter).isEqual(to: canvasBefore),
                "\(tag) canvas (0, H/2) is unchanged: \(canvasAfter) vs \(canvasBefore)")
    let uiPixels = document.doc?.layerPixels(0)
    try require(local(uiPixels, 0) == [0, 0, 255, 255],
                "\(tag) layer-local (0, H/2) holds the stroke colour: \(local(uiPixels, 0))")
    try require(local(uiPixels, 20) == local(before, 20),
                "\(tag) layer-local (20, H/2) — canvas x 0 — is untouched")
    try require(rect(document.doc, 0) == [-20, 0, width, height], "\(tag) the rect is unchanged")

    document.undoManager?.undo()
    try require(document.doc === moved, "\(tag) undo the stroke")
    let agent = try agentOK("stroke_selection", document,
                            ["layer": 0, "width": 6, "location": "inside", "color": "#0000FF"])
    try require(agent["outline"] as? String == "layer_edge" && agent["changed"] as? Bool == true
                && document.doc?.layerPixels(0) == uiPixels
                && rect(document.doc, 0) == [-20, 0, width, height],
                "\(tag) MCP equals the UI: \(agent)")
    document.undoManager?.undo()
    try require(document.doc === moved, "\(tag) undo the MCP stroke")
}

private func milliseconds(_ body: () throws -> Void) rethrows -> Double {
    let start = DispatchTime.now().uptimeNanoseconds
    try body()
    return Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000
}

private func median(_ values: [Double]) -> Double {
    values.sorted()[values.count / 2]
}

/// INFO only: an Outside 4 px stroke of a sparse 6000 x 4000 layer (one
/// opaque 200 x 200 square at (2900, 1900)) — the host's scratch query plus
/// the job — timed three times, each on a freshly built layer so the
/// content-box memo never answers the scan. The band stays inside the
/// layer, so its rect never changes. No bound on the time.
private func timeLayerEdgeStroke() throws {
    let tag = "[stroke timing]"
    let (width, height) = (6000, 4000)
    var sparse = [UInt8](repeating: 0, count: width * height * 4)
    for y in 1900..<2100 {
        for x in 2900..<3100 {
            let at = (y * width + x) * 4
            sparse[at] = 200
            sparse[at + 1] = 80
            sparse[at + 2] = 40
            sparse[at + 3] = 255
        }
    }
    let white = [UInt8](repeating: 255, count: width * height * 4)
    var runs: [Double] = []
    for _ in 0..<3 {
        guard let background = RasterImage.from(rgba: white, width: width, height: height),
              let layer = RasterImage.from(rgba: sparse, width: width, height: height),
              let doc = RasterDocument.from(image: background)?
                  .addingImageLayer(above: 0, layer, name: "Sparse") else {
            throw ProbeFailure(description: "\(tag) could not build the 6000 x 4000 fixture")
        }
        var out: RasterDocument?
        runs.append(try milliseconds {
            out = try doc.strokingLayerEdge(
                1, widthPx: 4, location: DR_STROKE_OUTSIDE, rgba: [0, 0, 255, 255],
                mode: DR_BLEND_NORMAL, opacity: 1, preserveTransparency: false)
        })
        try require(out != nil, "\(tag) the stroke changed the layer")
        try require(rect(out, 1) == [0, 0, width, height],
                    "\(tag) the rect is unchanged: \(rect(out, 1))")
    }
    print(String(format: "INFO: stroke layer edge [8-bit 6000x4000 sparse 200x200, outside 4 px] "
                 + "median_ms %.1f (runs %.1f/%.1f/%.1f)", median(runs), runs[0], runs[1], runs[2]))
}

func verifyStrokeLayerEdge() throws {
    try verifyTextLayerEdge()
    try verifyOversizedLayerEdge()
    try timeLayerEdgeStroke()
    print("PASS: stroke layer edge — a text layer prompts once and grows inside the canvas, "
          + "an oversized layer's "
          + "off-canvas edge is stroked; UI equals MCP")
}
