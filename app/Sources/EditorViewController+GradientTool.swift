import AppKit

/// The Gradient tool's ramp and its commit on an 8-bit layer's pixels.
/// The drag's other landings read the same ramp: a colour plane, mask or
/// channel (`EditorViewController+PlanePaint`), a 16-bit document
/// (`PrecisionColorTools`) and Quick Mask
/// (`EditorViewController+QuickMaskEdits`).
extension EditorViewController {
    /// The ramp the options bar describes right now (`GradientRamp`'s rules).
    var gradientToolRamp: GradientRamp {
        GradientRamp(
            options: ToolOptionsStore.shared.gradient,
            foreground: paintColor, background: backgroundColor)
    }

    /// The options bar's Mode: the stored raw `DrBlendMode`, Normal
    /// for anything unrecognized.
    var gradientToolMode: DrBlendMode {
        // A stored mode outside the document's list (`toolBlendChoices`, Lab's since #1 phase
        // 2f) paints as Normal; the store is kept.
        let stored = Self.blendMode(fromStored: ToolOptionsStore.shared.gradient.blendIndex)
        return Self.toolBlendChoices(for: document?.doc).contains { $0.0 == stored }
            ? stored : DR_BLEND_NORMAL
    }

    /// A drag with a non-Normal Mode onto a mask, colour plane, channel or the
    /// Quick Mask: says why and answers true (`refuseFillStyleOnCoverage`'s
    /// shape). A blend mode composites a colour into a layer's pixels;
    /// coverage has none to blend with.
    func refuseGradientModeOnCoverage() -> Bool {
        guard gradientToolMode != DR_BLEND_NORMAL,
              paintTarget.isCoverage || document?.quickMaskActive == true
        else { return false }
        NSSound.beep()
        let alert = NSAlert()
        alert.messageText = "The Gradient tool's Mode needs a layer target."
        alert.informativeText = GradientRamp.coverageTargetRefusal
        if let window = view.window { alert.beginSheetModal(for: window) } else { alert.runModal() }
        return true
    }

    /// The recorded step of a Gradient drag from `ramp`: the two-colour
    /// `gradient` step for a legacy ramp, `stops` and `shape` otherwise, and
    /// `blend_mode` when `mode` is not Normal.
    func gradientRecord(
        from a: CGPoint, to b: CGPoint, ramp: GradientRamp, target: String,
        mode: DrBlendMode = DR_BLEND_NORMAL
    ) -> [ActionStep] {
        guard ramp.isLegacyTwoStop else {
            return .gradientStops(
                from: a, to: b, ramp: ramp, target: target, space: document?.nsColorSpace,
                mode: mode)
        }
        return .gradient(
            from: a, to: b, start: ramp.stops[0].color, end: ramp.stops[1].color,
            radial: ramp.style == DR_GRADIENT_RADIAL, target: target,
            space: document?.nsColorSpace, mode: mode)
    }

    /// The Gradient tool's drag on an 8-bit layer's own pixels. A two-colour
    /// linear or radial ramp in Normal is the two-colour core op it has always
    /// been; any other ramp, and any ramp under another Mode (where a
    /// two-colour ramp becomes its two stops), is `gradientedStops`.
    func gradientLayer(from a: CGPoint, to b: CGPoint) {
        guard let document = document else { return }
        // Same rule as fillClicked: a gradient drag ends on the canvas,
        // outside menu validation's reach.
        guard !refuseAdjustmentPixelEdit(), !refuseGroupPixelEdit() else { return }
        guard !refuseLockedEdit(layer: document.activeLayerIndex, kind: DR_EDIT_PIXELS)
        else { return }
        let idx = document.activeLayerIndex
        let ramp = gradientToolRamp
        let mode = gradientToolMode
        let record = gradientRecord(
            from: a, to: b, ramp: ramp,
            target: paintTarget.layerEditAgentName(in: document.doc), mode: mode)
        guard let capture = document.captureOperation() else { NSSound.beep(); return }
        if ramp.isLegacyTwoStop, mode == DR_BLEND_NORMAL {
            // Foreground → background (the rail's swatches); Reverse swaps
            // them and the bar's opacity rides in both colours' alpha.
            let start = colorBytes(ramp.stops[0].color)
            let end = colorBytes(ramp.stops[1].color)
            document.applyCapturedRewrite(capture, "Gradient", record: record) {
                capture.doc.gradiented(
                    idx, from: a, to: b, start: start, end: end, kind: ramp.style,
                    mask: capture.selection)
            }
            return
        }
        guard let stops = ramp.flattened({ [weak self] in self?.colorBytes($0) }) else {
            NSSound.beep()
            return
        }
        document.applyCapturedRewrite(capture, "Gradient", record: record) {
            capture.doc.gradientedStops(
                idx, from: a, to: b, stops: stops, kind: ramp.style, mode: mode,
                smoothness: ramp.smoothness, mask: capture.selection)
        }
    }

    /// The options bar's Edit… button: the stops sheet over the tool's ramp.
    func showGradientStops() {
        presentAsSheet(GradientStopsSheetController(
            foreground: paintColor, background: backgroundColor
        ) { [weak self] in self?.optionsBar.refreshValues() })
    }
}
