import AppKit

/// Edit ▸ Fill… / Stroke… and the ⌥⌫ / ⌘⌫ fills on a 16-bit or 32-bit float
/// document: the same sheets, guards, records and alerts as the 8-bit commits
/// in `EditorViewController+FillStroke.swift`, which branch here on their
/// first lines. A sheet captures a `PrecisionEditorCapture` when it opens,
/// and Apply publishes through it, so a document, target or selection that
/// changed meanwhile refuses by name. The work is `PrecisionFillStroke`'s;
/// the MCP twins are `AgentServer+PrecisionFillStroke.swift`. At float a
/// colour-plane target follows the 16-bit one's order (scope refusal, HSL
/// alert, the rasterize question, one undo step, the record's `target`) into
/// the core's `paint_blend` `plane`, and an above-white colour is
/// recorded as the step's `rgba32`.
extension EditorViewController {
    /// Edit ▸ Fill… at 16 and 32 bits.
    func presentPrecisionFill() {
        guard let document else {
            NSSound.beep()
            return
        }
        if refuseOperationScope(.fill) { return }
        let capture: PrecisionEditorCapture
        do {
            capture = try PrecisionEditorCapture(document)
        } catch {
            presentPrecisionError(error)
            return
        }
        let sheet = FillSheetController(
            coverage: capture.target.isCoverage, quickMask: false,
            options: ToolOptionsStore.shared.fillDialog
        ) { [weak self] in self?.applyPrecisionFill($0, capture: capture, note: "Edit ▸ Fill…") }
        presentAsSheet(sheet)
    }

    /// Edit ▸ Stroke… at 16 and 32 bits. With nothing selected it outlines a
    /// layer's own pixels; a mask, colour plane or alpha channel still needs
    /// a selection.
    func presentPrecisionStroke() {
        guard let document else {
            NSSound.beep()
            return
        }
        if refuseOperationScope(.fill) { return }
        let capture: PrecisionEditorCapture
        do {
            capture = try PrecisionEditorCapture(document)
        } catch {
            presentPrecisionError(error)
            return
        }
        guard capture.selection != nil || capture.target == .layer else {
            showFillAlert(StrokeSettings.noSelection)
            return
        }
        if capture.selection == nil {
            guard !refusePrecisionLayerFill(capture) else { return }
            do {
                guard try PrecisionFillStroke.layerHasOutline(capture, layer: capture.layer) else {
                    showFillAlert(StrokeSettings.emptyLayer)
                    return
                }
            } catch {
                presentPrecisionError(error)
                return
            }
        }
        let sheet = StrokeSheetController(
            target: capture.target, color: paintColor,
            options: ToolOptionsStore.shared.strokeDialog
        ) { [weak self] in self?.applyPrecisionStroke($0, capture: capture) }
        presentAsSheet(sheet)
    }

    /// The 16/32-bit Fill commit: `applyFill`'s rules, one "Fill" step recorded
    /// as `fill_selection`. `capture` is the sheet's (nil for a chord, which
    /// captures now).
    func applyPrecisionFill(_ s: FillSettings, capture: PrecisionEditorCapture?, note: String) {
        guard let document, let capture = currentPrecisionCapture(capture, document) else { return }
        if capture.target == .layer, refusePrecisionLayerFill(capture) { return }
        guard let picked = s.contents.color(
            foreground: paintColor, background: backgroundColor,
            custom: s.customColor, in: document.nsColorSpace)
        else {
            NSSound.beep()
            return
        }
        let opacity = min(max(s.opacity, 0), 100) / 100
        let coverage = capture.target.isCoverage
        // #547 Lab phase 2e: a Lab layer fills Foreground / Background Color with the swatch's
        // Lab ink when it has one; a coverage target keeps the sRGB swatch.
        let labInks = ToolOptionsStore.shared.sharedState
        let labInk = coverage ? nil : s.contents == .foreground ? labInks.foregroundLab
            : s.contents == .background ? labInks.backgroundLab : nil
        let color = LabPaint.color(picked, stored: labInk, in: document.doc)
        let preserve = s.preserveTransparency && !coverage
        if coverage, DrBlendMode.degeneratesOnGray(s.mode) {
            showFillAlert(FillSettings.coverageBlendRefusal)
            return
        }
        guard confirmPrecisionRasterize(capture, document) else { return }
        var request = PrecisionFillStroke.Request(
            layer: capture.layer, target: capture.target, color: color, mode: s.mode,
            opacity: opacity, preserve: preserve)
        // #547: the swatch's stored CMYK ink for Foreground / Background Color.
        let shared = ToolOptionsStore.shared.sharedState
        request.cmykInk = s.contents == .foreground ? shared.foregroundInk
            : s.contents == .background ? shared.backgroundInk : nil
        let record: [ActionStep] = .fillSelection(
            color: color, blendMode: s.mode, opacity: opacity * 100,
            preserveTransparency: preserve,
            target: capture.target.layerEditAgentName(in: capture.facade),
            space: document.nsColorSpace, note: note,
            rgba32: PrecisionFillStroke.recordedLinear(request, capture: capture))
        do {
            let result = try PrecisionFillStroke.paint(
                request, coverage: capture.selection?.maskSamples16(), capture: capture,
                title: "Fill")
            try capture.publish(result, to: document, name: "Fill", record: record)
            if result == nil, !coverage {
                showUnchangedAlert(
                    preserve: preserve, doc: capture.facade, layer: capture.layer,
                    noOp: FillSettings.transparentNoOp, clippedNoOp: nil)
            }
        } catch {
            presentPrecisionError(error)
        }
    }

    /// The 16/32-bit Stroke commit: `applyStroke`'s rules, one "Stroke" step
    /// recorded as `stroke_selection`. A selection's band is the u16
    /// `stroke` kind; with nothing selected on a layer the core strokes the
    /// layer's own alpha edge in layer space.
    func applyPrecisionStroke(_ s: StrokeSettings, capture: PrecisionEditorCapture?) {
        guard let document, let capture = currentPrecisionCapture(capture, document) else { return }
        if capture.target == .layer, refusePrecisionLayerFill(capture) { return }
        guard capture.selection != nil || capture.target == .layer else {
            showFillAlert(StrokeSettings.noSelection)
            return
        }
        guard StrokeSettings.widthRange.contains(s.width) else {
            NSSound.beep()
            return
        }
        let opacity = min(max(s.opacity, 0), 100) / 100
        let coverage = capture.target.isCoverage
        let preserve = s.preserveTransparency && !coverage
        if coverage, DrBlendMode.degeneratesOnGray(s.mode) {
            showFillAlert(FillSettings.coverageBlendRefusal)
            return
        }
        // #547 Lab phase 2f: a Lab layer strokes the foreground swatch's Lab ink when the sheet
        // kept its colour (Fill's 2e rule); a coverage target keeps the sheet's colour.
        let labInk = s.color == paintColor ? ToolOptionsStore.shared.sharedState.foregroundLab : nil
        let color = coverage ? s.color : LabPaint.color(s.color, stored: labInk, in: document.doc)
        var request = PrecisionFillStroke.Request(
            layer: capture.layer, target: capture.target, color: color, mode: s.mode,
            opacity: opacity, preserve: preserve)
        // #547: the foreground swatch's stored CMYK ink when the sheet kept its colour.
        if capture.source.isCmyk, s.color == paintColor {
            request.cmykInk = ToolOptionsStore.shared.sharedState.foregroundInk
        }
        var record: [ActionStep] = .strokeSelection(
            width: s.width, location: s.location, color: color, blendMode: s.mode,
            opacity: opacity * 100, preserveTransparency: preserve,
            target: capture.target.layerEditAgentName(in: capture.facade),
            space: document.nsColorSpace,
            rgba32: PrecisionFillStroke.recordedLinear(request, capture: capture))
        do {
            // #547: a CMYK layer's stroke records its ink (C, M, Y, K percent).
            if capture.source.isCmyk, !coverage {
                let ink = try CmykPaint.ink(s.color, stored: request.cmykInk, in: capture.source)
                for i in record.indices {
                    for key in ["color", "color_space"] { record[i].arguments.removeValue(forKey: key) }
                    record[i].arguments["ink"] = ink.prefix(4).map { $0 * 100 }
                }
            }
            let result: RasterPrecisionDocument?
            if let selection = capture.selection {
                let band = try PrecisionFillStroke.band(
                    of: selection, widthPx: s.width, location: s.location)
                guard band.contains(where: { $0 != 0 }) else {
                    showFillAlert(StrokeSettings.emptyBand)
                    return
                }
                guard confirmPrecisionRasterize(capture, document) else { return }
                result = try PrecisionFillStroke.paint(
                    request, coverage: band, capture: capture, title: "Stroke")
            } else {
                guard try PrecisionFillStroke.layerHasOutline(capture, layer: capture.layer) else {
                    showFillAlert(StrokeSettings.emptyLayer)
                    return
                }
                guard confirmPrecisionRasterize(capture, document) else { return }
                result = try PrecisionFillStroke.strokeLayerEdge(
                    request, widthPx: s.width, location: s.location, capture: capture)
            }
            try capture.publish(result, to: document, name: "Stroke", record: record)
            if result == nil, !coverage {
                showUnchangedAlert(
                    preserve: preserve, doc: capture.facade, layer: capture.layer,
                    noOp: StrokeSettings.transparentNoOp,
                    clippedNoOp: StrokeSettings.outsideLayerNoOp)
            }
        } catch {
            presentPrecisionError(error)
        }
    }

    /// The sheet's capture, still current, or a fresh one for a chord; nil
    /// (after the refusal is shown) when the operation scope refuses or the
    /// capture is stale.
    private func currentPrecisionCapture(
        _ given: PrecisionEditorCapture?, _ document: ImageDocument
    ) -> PrecisionEditorCapture? {
        if refuseOperationScope(.fill) { return nil }
        do {
            let capture = try given ?? PrecisionEditorCapture(document)
            try capture.requireCurrent(document)
            return capture
        } catch {
            presentPrecisionError(error)
            return nil
        }
    }

    /// The adjustment, group and pixel-lock refusals a layer fill gives.
    private func refusePrecisionLayerFill(_ capture: PrecisionEditorCapture) -> Bool {
        refuseAdjustmentPixelEdit() || refuseGroupPixelEdit()
            || refuseLockedEdit(layer: capture.layer, kind: DR_EDIT_PIXELS)
    }

    /// The described-layer question for the layer and colour-plane targets
    /// (a mask or a channel leaves the layer's pixels alone).
    private func confirmPrecisionRasterize(
        _ capture: PrecisionEditorCapture, _ document: ImageDocument
    ) -> Bool {
        switch capture.target {
        case .layer, .plane, .ink: return document.confirmRasterize(layer: capture.layer)
        case .mask, .channel: return true
        }
    }
}
