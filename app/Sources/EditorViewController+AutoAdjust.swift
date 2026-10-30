import AppKit

/// Image ▸ Auto Tone / Auto Contrast / Auto Color: Levels parameters derived
/// from the active layer's own histogram, then applied through the SAME
/// levels math every other Levels path runs. On a CMYK document (#547) they
/// are the CMYK tone analysis's Auto (`CmykAdjustFilter.autoLevelsParams`:
/// per channel for Tone and Color, monochromatic for Contrast) applied as a
/// destructive CMYK Levels. One undo step; a layer already
/// at full range writes nothing (the core returns no parameters, and
/// `performLayerEdit`'s nil beeps rather than registering a phantom step).
///
/// The three are commands, not adjustment layers and not dialogs: each is a
/// measurement of the pixels in front of it, so there is nothing to re-edit
/// later and nothing to preview. Their agent mirrors are `auto_tone`,
/// `auto_contrast` and `auto_color` (AgentServer+Adjustments.swift), which
/// take the same clipping fraction as an argument and report the nine
/// numbers each one derived.
/// All three are disabled on a 32-bit float document.
extension EditorViewController {
    /// The share of counted pixels dropped at EACH end before the black and
    /// white points are read off the histogram. 0.1 % is the
    /// default in Auto Color Correction Options, for both ends, and it is
    /// what keeps a handful of hot pixels or a single blown specular from
    /// deciding where white is. The menu commands carry no dialog — the
    /// agent tools take `clip` when a picture needs a different one.
    private static let autoClip = LevelsAuto.defaultClip

    /// Image ▸ Auto Tone (⇧⌘L): each channel's own histogram stretched to
    /// the full range, which corrects a cast as a side effect.
    @objc func autoTone(_ sender: Any?) {
        applyAutoLevels(DR_AUTO_TONE, "Auto Tone", tool: "auto_tone")
    }

    /// Image ▸ Auto Contrast (⌥⇧⌘L): ONE black and white point read off the
    /// luma histogram and applied to all three channels, so the colour
    /// balance the photographer chose survives.
    @objc func autoContrast(_ sender: Any?) {
        applyAutoLevels(DR_AUTO_CONTRAST, "Auto Contrast", tool: "auto_contrast")
    }

    /// Image ▸ Auto Color (⇧⌘B): Auto Tone's per-channel stretch plus a
    /// per-channel gamma that snaps the midtones neutral.
    @objc func autoColor(_ sender: Any?) {
        applyAutoLevels(DR_AUTO_COLOR, "Auto Color", tool: "auto_color")
    }

    /// The three share one body. Both halves run inside the SAME
    /// `performLayerEdit` closure, on the image that edit is handed, for two
    /// reasons: the numbers must be derived from exactly the pixels they are
    /// applied to (with a colour plane or an alpha channel targeted, that is
    /// the plane, not the layer), and the pair is then one undo step.
    ///
    /// A nil from either half beeps and registers nothing. For
    /// `autoLevels` that is the honest "already at full range" case — the
    /// core refuses rather than writing an identity — and it is why the
    /// commands need no separate "nothing to do" alert. Like the app's other
    /// one-shot filters, analysis reads the whole target source. The result
    /// writes only through the captured selection, on UI and MCP alike.
    private func applyAutoLevels(_ mode: DrAutoMode, _ actionName: String, tool: String) {
        guard let document, !refuseOperationScope(.filter) else { return }
        let typed: PrecisionEditorCapture?
        let byte: OperationCapture?
        do {
            typed = document.doc?.isHighDepth == true ? try PrecisionEditorCapture(document) : nil
            byte = typed == nil ? document.captureOperation() : nil
        } catch { document.presentError(error); return }
        let target = paintTarget.agentName(in: document.doc)
        let apply: (ToneAnalysisScope) -> Void = { scope in
            do {
                let record: [ActionStep] = .autoAdjust(tool, target: target, analysisScope: scope.rawValue)
                if let capture = typed {
                    try capture.requireCurrent(document)
                    // #547: a CMYK document runs Auto on a layer's ink, or on one ink plate or an
                    // alpha channel through the scalar Auto below.
                    if capture.source.isCmyk, capture.target == .mask {
                        throw DoctoredCoreError(message: CmykAdjustFilter.autoTargetRefusal(target))
                    }
                    if capture.target == .layer || capture.target.isPlane {
                        guard document.confirmRasterize(layer: capture.layer) else { return }
                    }
                    try capture.requireCurrent(document)
                    // #547: on ink, the CMYK tone analysis's Auto as a destructive CMYK Levels.
                    if capture.source.isCmyk, capture.target == .layer {
                        let coverage = scope == .selection
                            ? document.canvasSelection?.maskSamples16() ?? [] : []
                        guard let params = try CmykAdjustFilter.autoLevelsParams(
                            capture.source, layer: capture.layer, coverage: coverage,
                            form: CmykAdjustFilter.autoForm(tool))
                        else { NSSound.beep(); return }
                        let edit = try SelectedAdjustment.edit(capture, layer: capture.layer, filter: "levels",
                                                               params: params, target: .layer)
                        let result = try edit.run(title: actionName)
                        _ = try capture.publish(result, to: document, name: actionName, record: record)
                        return
                    }
                    if capture.target.isCoverage {
                        let edit = try PrecisionScalarAuto(capture: capture, layer: capture.layer, target: capture.target,
                            mode: mode, scope: scope, clip: Self.autoClip)
                        let result = try edit.run(title: actionName)
                        _ = try capture.publish(result.document, to: document, name: actionName, record: record)
                        return
                    }
                    let edit = try SelectedAutoAdjustment(capture: capture, layer: capture.layer, mode: mode, scope: scope, clip: Self.autoClip)
                    let result = try edit.run(title: actionName)
                    _ = try capture.publish(result.document, to: document, name: actionName, record: record)
                } else if let capture = byte {
                    guard !document.hasClosed, capture.isCurrent(in: document) else {
                        throw DoctoredCoreError(message: "The document, target or selection changed while Auto options were open.")
                    }
                    let mask = try capture.analysisCoverage(scope)
                    let derived = try capture.source.scopedAutoLevels(mask: mask, mode: mode, clip: Self.autoClip)
                    guard let derived else { return }
                    document.applyCapturedOperation(capture, actionName, record: record) {
                        $0.levelsChannels(black: derived.black, white: derived.white, gamma: derived.gamma)
                    }
                }
            } catch { document.presentError(error) }
        }
        guard document.canvasSelection != nil, let window = view.window else { apply(.wholeTarget); return }
        let alert = NSAlert()
        alert.messageText = "\(actionName) analysis"
        alert.informativeText = "Measure the whole target or weight measurements by the selection. The result writes through the selection in either case."
        alert.addButton(withTitle: "Whole Target")
        alert.addButton(withTitle: "Selection")
        alert.addButton(withTitle: "Cancel")
        alert.beginSheetModal(for: window) { response in
            if response == .alertFirstButtonReturn { apply(.wholeTarget) }
            else if response == .alertSecondButtonReturn { apply(.selection) }
        }
    }
}
