import AppKit

/// `fill_selection` / `stroke_selection` on a 16-bit or 32-bit float document:
/// the MCP twins of the typed Edit ▸ Fill… / Stroke…
/// (`EditorViewController+PrecisionFillStroke.swift`, `applyPrecisionFill` /
/// `applyPrecisionStroke`), with the colour, mode, opacity and Preserve
/// Transparency parsed by the 8-bit `selectionPaintArguments`, so
/// `color_space` keeps its 8-bit meaning. The work is `PrecisionFillStroke`'s;
/// the reply carries the 8-bit keys plus `storage` (`"rgba32_float"` for a
/// float layer or one of its colour planes; `"u16"` for any other
/// coverage target; `"rgba16_unorm"` for a 16-bit layer). `rgba32`
/// (`linearInkArgument`) is linear ink for a float document. The dispatcher
/// records the call, so these commits record nothing themselves.
extension AgentServer {
    func precisionFillSelection(_ a: [String: Any], _ document: ImageDocument) throws -> String {
        let (index, planeTarget, capture) = try precisionSelectionPaintCapture(
            a, document, tool: "fill_selection")
        // Edit ▸ Fill…'s Pattern contents (`applyPrecisionPatternFill`): the tile replaces the
        // colour, whose arguments then refuse beside it.
        let pattern = try fillPatternArgument(a, coverage: planeTarget.isCoverage)
        // #1: a CMYK layer takes `ink` (C, M, Y, K percent) in place of `color`.
        let ink = capture.source.isCmyk && !planeTarget.isCoverage && pattern == nil
            ? try cmykInkArgument(a, "ink") : nil
        var arguments = a
        if ink != nil || pattern != nil, arguments["color"] == nil {
            arguments["color"] = "#000000"
        }
        let paint = try selectionPaintArguments(
            arguments, document, coverage: planeTarget.isCoverage, tool: "fill_selection")
        var request = PrecisionFillStroke.Request(
            layer: index, target: planeTarget, color: paint.color, mode: paint.mode,
            opacity: paint.opacity, preserve: paint.preserve, linear: paint.linear)
        request.cmykInk = ink
        request.srgb = try labSrgbInk(a, document, coverage: planeTarget.isCoverage)
        request.pattern = pattern
        let result = try precisionToolErrors {
            try PrecisionFillStroke.paint(
                request, coverage: capture.selection?.maskSamples16(), capture: capture,
                title: "Fill")
        }
        return try precisionSelectionPaintReply(
            document, capture: capture, request: request, result: result, actionName: "Fill",
            noOp: FillSettings.transparentNoOp,
            extra: pattern.map { ["pattern": $0.echo] } ?? [:])
    }

    func precisionStrokeSelection(
        _ a: [String: Any], _ document: ImageDocument, width: Double, location: StrokeLocation
    ) throws -> String {
        let (index, planeTarget, capture) = try precisionSelectionPaintCapture(
            a, document, tool: "stroke_selection")
        // #1: a CMYK layer takes `ink` (C, M, Y, K percent) in place of `color`, as
        // `fill_selection` does; mirrors `applyPrecisionStroke`'s CMYK record.
        let ink = capture.source.isCmyk && !planeTarget.isCoverage ? try cmykInkArgument(a, "ink") : nil
        var arguments = a
        if ink != nil, arguments["color"] == nil { arguments["color"] = "#000000" }
        let paint = try selectionPaintArguments(
            arguments, document, coverage: planeTarget.isCoverage, tool: "stroke_selection")
        guard capture.selection != nil || planeTarget == .layer else {
            throw ToolError(message: StrokeSettings.noSelection)
        }
        let outline = capture.selection == nil ? "layer_edge" : "selection"
        let extra: [String: Any] = [
            "width": width, "location": location.agentName, "outline": outline,
        ]
        let unchanged = { (note: String) in
            try self.jsonResult(extra.merging([
                "ok": true, "changed": false, "layer": index,
                "target": planeTarget.agentName(in: capture.facade), "note": note,
            ]) { _, shared in shared })
        }
        var request = PrecisionFillStroke.Request(
            layer: index, target: planeTarget, color: paint.color, mode: paint.mode,
            opacity: paint.opacity, preserve: paint.preserve, linear: paint.linear)
        request.cmykInk = ink
        request.srgb = try labSrgbInk(a, document, coverage: planeTarget.isCoverage)
        let result: RasterPrecisionDocument?
        if let selection = capture.selection {
            let band = try precisionToolErrors {
                try PrecisionFillStroke.band(of: selection, widthPx: width, location: location)
            }
            guard band.contains(where: { $0 != 0 }) else {
                return try unchanged(StrokeSettings.emptyBand)
            }
            return try commitPrecisionPaintBand(
                document, capture: capture, request: request, band: band,
                actionName: "Stroke", noOp: StrokeSettings.transparentNoOp,
                clippedNoOp: StrokeSettings.outsideLayerNoOp, extra: extra)
        } else {
            guard try precisionToolErrors({
                try PrecisionFillStroke.layerHasOutline(capture, layer: index)
            }) else { return try unchanged(StrokeSettings.emptyLayer) }
            result = try precisionToolErrors {
                try PrecisionFillStroke.strokeLayerEdge(
                    request, widthPx: width, location: location, capture: capture)
            }
        }
        return try precisionSelectionPaintReply(
            document, capture: capture, request: request, result: result,
            actionName: "Stroke", noOp: StrokeSettings.transparentNoOp, extra: extra,
            clippedNoOp: StrokeSettings.outsideLayerNoOp)
    }

    /// The typed band branch `stroke_selection` and `fill_path`
    /// (AgentServer+Paths.swift) share: `request`'s colour through its mode
    /// and opacity over the canvas-sized u16 coverage `band`, published as
    /// one undo step named `actionName`.
    func commitPrecisionPaintBand(
        _ document: ImageDocument, capture: PrecisionEditorCapture,
        request: PrecisionFillStroke.Request, band: [UInt16], actionName: String,
        noOp: String, clippedNoOp: String?, extra: [String: Any]
    ) throws -> String {
        let result = try precisionToolErrors {
            try PrecisionFillStroke.paint(
                request, coverage: band, capture: capture, title: actionName)
        }
        return try precisionSelectionPaintReply(
            document, capture: capture, request: request, result: result,
            actionName: actionName, noOp: noOp, extra: extra, clippedNoOp: clippedNoOp)
    }

    /// The shared target, refusals and capture: the 8-bit `operationCapture`'s
    /// scope refusal, the pixel lock on a layer target, then a typed capture.
    /// Internal: `fill_path` takes the same capture.
    func precisionSelectionPaintCapture(
        _ a: [String: Any], _ document: ImageDocument, tool: String
    ) throws -> (Int, PaintTarget, PrecisionEditorCapture) {
        guard let doc = document.doc else {
            throw ToolError(message: "Document has no image")
        }
        guard !document.quickMaskActive else {
            throw ToolError(message: OperationScope.quickMaskRefusal)
        }
        let index = try structuralLayerIndex(a, document)
        let planeTarget = try paintTarget(a, document)
        if let reason = scopeRefusal(.fill, target: planeTarget, doc: doc, layer: index) {
            throw ToolError(message: reason)
        }
        if planeTarget == .layer { try rejectLockedEdit(document, index, DR_EDIT_PIXELS) }
        let capture = try precisionToolErrors { try PrecisionEditorCapture(document) }
        return (index, planeTarget, capture)
    }

    /// Publishes `result` as one undo step and answers the 8-bit reply's
    /// keys plus `storage`: an unchanged LAYER edit names `noOp` when the
    /// layer's alpha was kept (preserve or a transparency lock), else
    /// `clippedNoOp` when given.
    private func precisionSelectionPaintReply(
        _ document: ImageDocument, capture: PrecisionEditorCapture,
        request: PrecisionFillStroke.Request, result: RasterPrecisionDocument?,
        actionName: String, noOp: String, extra: [String: Any], clippedNoOp: String? = nil
    ) throws -> String {
        let coverage = request.target.isCoverage
        let changed = try precisionToolErrors {
            try capture.publish(result, to: document, name: actionName)
        }
        // A float layer's colour plane is one channel of its f32 pixels.
        let float = capture.facade.isFloat && (!coverage || request.target.isPlane)
        var fields = extra.merging([
            "ok": true, "changed": changed, "layer": request.layer,
            "target": request.target.agentName(in: capture.facade),
            "storage": float ? "rgba32_float" : coverage ? "u16"
                : capture.source.isCmyk ? "cmyka16_unorm" : "rgba16_unorm",
        ]) { _, shared in shared }
        if result == nil {
            let kept = !coverage && (request.preserve
                || capture.facade.transparencyLockOwner(request.layer) != nil)
            fields["note"] = kept ? noOp
                : (coverage ? nil : clippedNoOp) ?? "No covered pixels changed. No undo step was added."
        }
        return try jsonResult(fields)
    }

    /// `rgba32`: linear light in the document's primaries, as
    /// `sample_color`'s `center_rgba32` / `mean_rgba32` report it — four
    /// numbers, RGB 0…65504 and alpha 0…1 — for a 32-bit float document's
    /// Fill and Stroke; nil when the call passes none. It replaces `color`
    /// and takes no `color_space`; an 8- or 16-bit document and Quick Mask
    /// refuse it by name.
    func linearInkArgument(_ a: [String: Any], _ document: ImageDocument) throws -> [Double]? {
        guard let raw = a["rgba32"] else { return nil }
        guard a["color"] == nil, a["color_space"] == nil else {
            throw ToolError(message: "Pass rgba32 or color, not both; rgba32 takes no color_space.")
        }
        guard document.doc?.isFloat == true, !document.quickMaskActive,
              stringArg(a, "target") != "quick_mask"
        else {
            throw ToolError(message: FloatPaint.depthInkRefusal(key: "rgba32", colour: "color"))
        }
        let numbers = (raw as? [Any])?.compactMap {
            ($0 as? NSNumber)?.doubleValue ?? ($0 as? String).flatMap(Double.init)
        }
        guard let rgba = numbers, rgba.count == 4, (raw as? [Any])?.count == 4,
              rgba.allSatisfy(\.isFinite),
              rgba.prefix(3).allSatisfy({ (0...ExtendedRangeColor.maximum).contains($0) }),
              (0...1).contains(rgba[3])
        else {
            throw ToolError(
                message: "rgba32 requires four finite numbers: linear-light red, green and blue "
                    + "0–65504 in the document's primaries and alpha 0–1.")
        }
        return rgba
    }

    /// A core or capture refusal as an in-band tool error. Internal:
    /// `fill_path` reports its coverage and capture refusals the same way.
    func precisionToolErrors<T>(_ body: () throws -> T) throws -> T {
        do {
            return try body()
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: (error as? DoctoredCoreError)?.message ?? error.localizedDescription)
        }
    }
}
