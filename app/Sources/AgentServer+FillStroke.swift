import AppKit

/// `fill_selection`: the MCP twin of Edit ▸ Fill… and of Edit ▸ Fill
/// with Foreground/Background Color (⌥⌫ / ⌘⌫, and ⇧ to preserve
/// transparency) — EditorViewController+FillStroke.swift's `applyFill`,
/// with an explicit colour where the UI reads the sheet or the swatches.
/// Same capture, same core op (`dr_doc_fill_selection`, or the gray of
/// `PlaneAlgebra.fillColor` through `PlaneScratchOp.fillSelection` on a mask,
/// colour plane or alpha channel: the colour's 30/59/11 luma on a colour
/// plane, its Rec. 709 gray on a mask or alpha channel), same
/// refusals, one undo step named "Fill". In Quick Mask,
/// or with `target: "quick_mask"`, it fills the Quick Mask coverage instead
/// (`fillQuickMask`), at every depth.
///
/// `stroke_selection`: the MCP twin of Edit ▸ Stroke…
/// (`applyStroke`). The selection becomes a canvas-space band
/// (`StrokeSettings.band`), filled through the same argument parsing and
/// commit as `fill_selection`; with nothing selected on a layer
/// (`StrokeSettings.outline`) the core strokes the layer's own alpha edge in
/// layer space (`strokingLayerEdge`, the UI's `commitLayerEdgeStroke`). One
/// undo step named "Stroke".
///
/// On a 16-bit or 32-bit float document both tools branch to
/// `AgentServer+PrecisionFillStroke.swift` after their Quick Mask check
/// and argument parsing; a colour-plane target writes the colour's
/// 30/59/11 luma into that one channel at every depth (rounded to a
/// code at 8 bits, unrounded at 16 and 32), and at float `rgba32` is linear ink
/// up to 65504 (`linearInkArgument`).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let fillStrokeHandlers: [String: ToolHandler] = [
        "fill_selection": { $0.fillSelection },
        "stroke_selection": { $0.strokeSelection },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let fillStrokeToolFacts: [String: ToolFacts] = [
        // Edit ▸ Fill… / Stroke… through the typed `paint_blend`,
        // scalar `paint` and `stroke_layer_edge` (AgentServer+PrecisionFillStroke.swift).
        // A layer, mask, colour plane or channel fill/stroke.
        // Fill… and Stroke… paint a layer (linear ink through the
        // core's float `paint_blend`, in the float layer modes), its mask
        // or an alpha channel; the Red/Green/Blue/Alpha planes
        // through the same `paint_blend`'s `plane`. In Quick Mask `fill_selection`
        // fills coverage, and `DocumentPrecision.refusal` admits it there
        // before this table is asked.
        "fill_selection": [.selectionSensitive, .typedTool, .rasterTool, .floatTool, .cmykTool],
        "stroke_selection": [.selectionSensitive, .typedTool, .rasterTool, .floatTool, .cmykTool],
    ]

    func fillSelection(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        if document.quickMaskActive || stringArg(a, "target") == "quick_mask" {
            return try fillQuickMask(a, document)
        }
        if document.doc?.isHighDepth == true { return try precisionFillSelection(a, document) }
        let index = try structuralLayerIndex(a, document)
        let planeTarget = try paintTarget(a, document)
        let paint = try selectionPaintArguments(
            a, document, coverage: planeTarget.isCoverage, tool: "fill_selection")
        let capture = try operationCapture(
            document, operation: .fill, target: planeTarget, layer: index)
        // A fill reaching past a trimmed layer grows it, as applyFill does.
        let base: RasterDocument
        do {
            base = try capture.paintDestination(mask: capture.selection, preserve: paint.preserve)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try commitSelectionPaintResult(
            document, capture: capture, rgba: paint.rgba, mode: paint.mode,
            opacity: paint.opacity, preserve: paint.preserve, mask: capture.selection,
            actionName: "Fill", noOp: FillSettings.transparentNoOp, extra: [:], base: base)
    }

    /// Edit ▸ Fill…, ⌥⌫ and ⌘⌫ in Quick Mask
    /// (`EditorViewController.applyQuickMaskFill`): the whole Quick Mask
    /// coverage filled with the colour's coverage gray, one file-clean "Fill"
    /// step, at every depth — 32-bit float included, whose native u16
    /// coverage fills as 16 bits' does. `target: "quick_mask"` pins it there
    /// and refuses by name when the document is not in Quick Mask; `layer`,
    /// another target and `preserve_transparency: true` are refused. The
    /// dispatcher records the call, so this commit records nothing itself.
    private func fillQuickMask(_ a: [String: Any], _ document: ImageDocument) throws -> String {
        guard document.quickMaskActive else {
            throw ToolError(
                message: "target \"quick_mask\" fills the Quick Mask, and this document is not "
                    + "in Quick Mask. Enter it with set_quick_mask first, or pass another target.")
        }
        let named = stringArg(a, "target")
        if a["layer"] != nil || a["index"] != nil || (a["target"] != nil && named != "quick_mask")
            || boolArg(a, "preserve_transparency") == true {
            throw ToolError(message: FillSettings.quickMaskArguments)
        }
        let paint = try selectionPaintArguments(a, document, coverage: true, tool: "fill_selection")
        let changed: Bool
        do {
            let fill = try QuickMaskFill(color: paint.color, mode: paint.mode, opacity: paint.opacity)
            changed = try document.applyQuickMaskFill(fill, record: .notACommand)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        var result = selectionHistorySummary(document)
        result["changed"] = changed
        result["target"] = "quick_mask"
        if !changed { result["note"] = "No Quick Mask coverage changed. No undo step was added." }
        return try jsonResult(result)
    }

    /// Edit ▸ Stroke…: the captured outline stroked (width, location) with
    /// `color`. A selection's band is filled through `fill_selection`'s
    /// commit, and a band that reaches past a trimmed or offset layer's
    /// pixels grows the layer (`OperationCapture.paintDestination`); with nothing
    /// selected on a layer the core strokes the layer's own
    /// alpha edge in layer space, growing it past the canvas if need be. The
    /// reply's `outline` says which.
    func strokeSelection(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let index = try structuralLayerIndex(a, document)
        let planeTarget = try paintTarget(a, document)
        guard a["width"] != nil else {
            throw ToolError(message: "stroke_selection requires width (pixels, 1–250).")
        }
        guard let width = doubleArg(a, "width"), width.isFinite,
              StrokeSettings.widthRange.contains(width)
        else {
            throw ToolError(message: "width must be a number from 1 to 250 (pixels).")
        }
        var location = StrokeLocation.center
        if let name = stringArg(a, "location") {
            guard let parsed = StrokeLocation(agentName: name) else {
                throw ToolError(message: "location must be inside, center or outside.")
            }
            location = parsed
        } else if a["location"] != nil {
            throw ToolError(message: "location must be inside, center or outside.")
        }
        if document.doc?.isHighDepth == true {
            return try precisionStrokeSelection(a, document, width: width, location: location)
        }
        let paint = try selectionPaintArguments(
            a, document, coverage: planeTarget.isCoverage, tool: "stroke_selection")
        let capture = try operationCapture(
            document, operation: .fill, target: planeTarget, layer: index)
        guard let outline = StrokeSettings.outline(of: capture) else {
            throw ToolError(message: StrokeSettings.noSelection)
        }
        let outlineName = outline.agentName
        let emptyLayer = {
            try self.jsonResult([
                "ok": true, "changed": false, "layer": index,
                "target": capture.target.agentName(in: capture.doc),
                "outline": outlineName, "note": StrokeSettings.emptyLayer,
            ])
        }
        let extra: [String: Any] = [
            "width": width, "location": location.agentName, "outline": outlineName,
        ]
        guard case .selection(let selection) = outline else {
            guard StrokeSettings.layerHasOutline(capture) else { return try emptyLayer() }
            let updated: RasterDocument?
            do {
                updated = try capture.doc.strokingLayerEdge(
                    index, widthPx: width, location: location.dr, rgba: paint.rgba,
                    mode: paint.mode, opacity: paint.opacity,
                    preserveTransparency: paint.preserve)
            } catch {
                throw ToolError(message: error.localizedDescription)
            }
            return try selectionPaintReply(
                document, capture: capture, updated: updated, preserve: paint.preserve,
                actionName: "Stroke", noOp: StrokeSettings.transparentNoOp, extra: extra,
                clippedNoOp: StrokeSettings.outsideLayerNoOp)
        }
        guard selection.contains(where: { $0 != 0 }) else { return try emptyLayer() }
        guard let band = StrokeSettings.band(
            of: selection, canvasWidth: capture.doc.width, canvasHeight: capture.doc.height,
            widthPx: width, location: location)
        else {
            throw ToolError(message: "Could not compute the stroke band")
        }
        guard band.contains(where: { $0 != 0 }) else {
            return try jsonResult([
                "ok": true, "changed": false, "layer": index,
                "target": capture.target.agentName(in: capture.doc),
                "outline": outlineName, "note": StrokeSettings.emptyBand,
            ])
        }
        return try commitPaintBand(
            document, capture: capture, rgba: paint.rgba, mode: paint.mode,
            opacity: paint.opacity, preserve: paint.preserve, band: band,
            actionName: "Stroke", noOp: StrokeSettings.transparentNoOp,
            clippedNoOp: StrokeSettings.outsideLayerNoOp, extra: extra)
    }

    /// The band branch `stroke_selection` and `fill_path`
    /// (AgentServer+Paths.swift) share: `rgba` through `mode` at `opacity`
    /// over the canvas-sized coverage `band` on the captured target, one undo
    /// step named `actionName`. A band that reaches past a trimmed or offset
    /// layer's pixels grows the layer first (`paintDestination`), except
    /// under `preserve`.
    func commitPaintBand(
        _ document: ImageDocument, capture: OperationCapture, rgba: [UInt8],
        mode: DrBlendMode, opacity: Double, preserve: Bool, band: [UInt8],
        actionName: String, noOp: String, clippedNoOp: String?, extra: [String: Any]
    ) throws -> String {
        let base: RasterDocument
        do {
            base = try capture.paintDestination(mask: band, preserve: preserve)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try commitSelectionPaintResult(
            document, capture: capture, rgba: rgba, mode: mode,
            opacity: opacity, preserve: preserve, mask: band,
            actionName: actionName, noOp: noOp, extra: extra,
            clippedNoOp: clippedNoOp, base: base)
    }

    /// `fill_selection` / `stroke_selection`'s shared arguments: the colour
    /// (`color`, or a float document's `rgba32`; one is required and
    /// `tool` names the caller in the refusal) as the qualified `NSColor`
    /// and its document bytes, the blend mode (HSL refuses on a `coverage`
    /// target, the Quick Mask included), the 0…1 opacity,
    /// `preserve_transparency` (layer target only) and `rgba32`'s numbers
    /// as `linear` (nil with `color`).
    func selectionPaintArguments(
        _ a: [String: Any], _ document: ImageDocument, coverage: Bool, tool: String
    ) throws -> (color: NSColor, rgba: [UInt8], mode: DrBlendMode, opacity: Double,
                 preserve: Bool, linear: [Double]?) {
        let linear = try linearInkArgument(a, document)
        let color: NSColor
        if let linear {
            guard let doc = document.doc,
                  let built = ExtendedRangeColor.linearColor(linear, in: doc)
            else { throw ToolError(message: "Could not convert the color") }
            color = built
        } else {
            guard stringArg(a, "color") != nil else {
                throw ToolError(
                    message: "\(tool) requires color (#RRGGBB or #RRGGBBAA). "
                        + "Pass the foreground or background colour explicitly.")
            }
            // A coverage target takes `coverageColor` and reads it in
            // `coverageSpace`: white stays 255 on a LUT-mode document.
            // #547: a Lab layer's sRGB hex stays unquantized (`labSrgbInk`).
            color = try coverage
                ? coverageColor(a, "color", in: document, fallback: .black)
                : labSrgbInk(a, document, coverage: false)
                    ? parseColor(a, "color", fallback: .black)
                    : qualifiedColor(a, "color", in: document, fallback: .black)
        }
        let rgba = try coverage
            ? coverageRGBA(color, in: document) : colorRGBA(color, in: document)
        let mode = try blendModeArg(a) ?? DR_BLEND_NORMAL
        if coverage, DrBlendMode.degeneratesOnGray(mode) {
            throw ToolError(message: FillSettings.coverageBlendRefusal)
        }
        var opacity = 1.0
        if let value = doubleArg(a, "opacity") {
            guard value.isFinite else {
                throw ToolError(message: "opacity must be a finite number from 0 to 1")
            }
            opacity = min(max(value, 0), 1)
        }
        let preserve = boolArg(a, "preserve_transparency") ?? false
        if preserve, coverage {
            throw ToolError(
                message: "preserve_transparency applies to layer pixels only; a mask, colour "
                    + "plane or alpha channel has no transparency of its own to keep. "
                    + "Omit it, or pass target \"layer\".")
        }
        return (color, rgba, mode, opacity, preserve, linear)
    }

    /// #547 Lab phase 4a: whether a Lab layer's `color` is authored sRGB ink, read unquantized
    /// (`parseColor`) and sent as `srgb` so the core converts it by Image ▸ Mode ▸ Lab Color's
    /// rule (`PrecisionFillStroke.Request.srgb`), as the Brush's `precisionInk` and `crop`'s
    /// `background_color` are, not rounded to the document's 8-bit L*a*b* codes at 16 bits.
    func labSrgbInk(_ a: [String: Any], _ document: ImageDocument, coverage: Bool) throws -> Bool {
        guard !coverage, document.doc?.isLab == true else { return false }
        return try colorSpaceArgument(a) == .srgb
    }

    /// The shared commit: `rgba` through `mode` at `opacity` weighted by the
    /// canvas-sized `mask` on the captured target, answered by
    /// `selectionPaintReply`. A layer fill reads `base` (Stroke…'s grown
    /// layer) when given, else the capture's document.
    private func commitSelectionPaintResult(
        _ document: ImageDocument, capture: OperationCapture, rgba: [UInt8],
        mode: DrBlendMode, opacity: Double, preserve: Bool, mask: [UInt8]?,
        actionName: String, noOp: String, extra: [String: Any],
        clippedNoOp: String? = nil, base: RasterDocument? = nil
    ) throws -> String {
        let index = capture.layerIndex
        let coverage = capture.target.isCoverage
        let updated: RasterDocument?
        if coverage {
            guard let gray = PlaneAlgebra.fillColor(rgba, target: capture.target) else {
                throw ToolError(message: "Could not convert the color")
            }
            updated = capture.applyingScratch(
                .fillSelection(rgba: gray, mode: mode, opacity: opacity), mask: mask)
        } else {
            updated = (base ?? capture.doc).selectionFilled(
                index, mask: mask, rgba: rgba, mode: mode, opacity: opacity,
                preserveTransparency: preserve)
        }
        return try selectionPaintReply(
            document, capture: capture, updated: updated, preserve: preserve,
            actionName: actionName, noOp: noOp, extra: extra, clippedNoOp: clippedNoOp)
    }

    /// The reply half of the shared commit, which the layer-edge stroke
    /// shares too. A LAYER edit that changed nothing (`updated` nil) answers
    /// `changed:false` with `noOp` when the layer's alpha was kept (preserve
    /// or a transparency lock), else with `clippedNoOp` when given (Stroke…'s
    /// colour, opacity and mode can leave every pixel under its band
    /// unchanged); every other outcome is `commitScopedResult`'s, with
    /// `extra` merged in.
    private func selectionPaintReply(
        _ document: ImageDocument, capture: OperationCapture, updated: RasterDocument?,
        preserve: Bool, actionName: String, noOp: String, extra: [String: Any],
        clippedNoOp: String? = nil
    ) throws -> String {
        let index = capture.layerIndex
        if updated == nil, !capture.target.isCoverage {
            let kept = preserve || capture.doc.transparencyLockOwner(index) != nil
            if let note = kept ? noOp : clippedNoOp {
                return try jsonResult(extra.merging([
                    "ok": true, "changed": false, "layer": index,
                    "target": capture.target.agentName(in: capture.doc),
                    "note": note,
                ]) { _, shared in shared })
            }
        }
        return try commitScopedResult(
            document, capture: capture, updated: updated, actionName: actionName, extra: extra)
    }
}
