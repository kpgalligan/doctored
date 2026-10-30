import AppKit

// `deleteChordAllowed` is THE ⌫-chord guard — Clear, the hidden twin and the four fills all ask it;
// the command palette's own field does not count as editing text.

/// Edit ▸ Fill… and the four ⌥⌫ / ⌘⌫ fills: one commit,
/// `applyFill`, for the sheet, the menu items and their key chords. The MCP
/// twin is `fill_selection` (AgentServer+FillStroke.swift), and a fill
/// records that tool's step.
///
/// In Quick Mask, Fill… and the chords fill the Quick Mask coverage
/// instead (`applyQuickMaskFill`, at every depth), one file-clean "Fill"
/// step recorded as `fill_selection` with `target: "quick_mask"`.
///
/// On a 16-bit or 32-bit float document Fill…, Stroke… and the chords branch
/// on their first lines (after the Quick Mask branch) to
/// `EditorViewController+PrecisionFillStroke.swift`.
///
/// Edit ▸ Stroke… (8 bits): `applyStroke` turns the captured
/// selection into a canvas-space band (`StrokeSettings.band`) and fills it
/// through the SAME private commit, `commitSelectionPaint`, as one "Stroke"
/// undo step. With nothing selected on a layer
/// (`StrokeSettings.outline`) the core strokes the layer's own alpha edge in
/// layer space instead (`commitLayerEdgeStroke`), growing the layer past the
/// canvas if the band reaches there. Its MCP twin is `stroke_selection`, and
/// it records that tool's step.
///
/// `deleteChordAllowed` is THE ⌫-chord guard: Edit ▸ Clear's bare ⌫, the
/// hidden ⇧⌫ twin of Fill… and the four fill chords all resolve as key
/// equivalents AHEAD of the first responder, so each stands down while text
/// is being edited, a guide is grabbed or a selection gesture is pending —
/// otherwise the chord would reach the menu instead of the field or the
/// canvas that owns it.
extension EditorViewController {
    // MARK: - Menu actions

    /// Edit ▸ Fill… (⇧F5). Captures the edit target now, so the fill lands
    /// on the layer, plane or channel the sheet was opened over. In Quick
    /// Mask it captures the Quick Mask state instead and fills its coverage.
    @objc func fillSelection(_ sender: Any?) {
        if let document = document, document.quickMaskActive {
            let expected = document.selectionHistory
            let sheet = FillSheetController(
                coverage: true, quickMask: true, options: ToolOptionsStore.shared.fillDialog
            ) { [weak self] in
                self?.applyQuickMaskFill($0, expected: expected, note: "Edit ▸ Fill…")
            }
            presentAsSheet(sheet)
            return
        }
        if let doc = document?.doc, doc.isHighDepth {
            presentPrecisionFill()
            return
        }
        if refuseOperationScope(.fill) { return }
        guard let document = document, let captured = document.captureOperation() else {
            NSSound.beep()
            return
        }
        let sheet = FillSheetController(
            coverage: captured.target.isCoverage, quickMask: false,
            options: ToolOptionsStore.shared.fillDialog
        ) { [weak self] in self?.applyFill($0, capture: captured) }
        presentAsSheet(sheet)
    }

    /// The hidden ⇧⌫ twin of Fill… (its second chord).
    @objc func fillSelectionFromDeleteKey(_ sender: Any?) {
        fillSelection(sender)
    }

    /// Edit ▸ Stroke…. Captures the edit target and the selection now, so
    /// the stroke lands on what the sheet was opened over. With nothing
    /// selected it outlines a layer's own pixels; a mask, colour plane
    /// or alpha channel still needs a selection.
    @objc func strokeSelection(_ sender: Any?) {
        if let doc = document?.doc, doc.isHighDepth {
            presentPrecisionStroke()
            return
        }
        if refuseOperationScope(.fill) { return }
        guard let document = document, let captured = document.captureOperation() else {
            NSSound.beep()
            return
        }
        guard let outline = StrokeSettings.outline(of: captured) else {
            showFillAlert(StrokeSettings.noSelection)
            return
        }
        guard hasOutline(outline, capture: captured) else {
            showFillAlert(StrokeSettings.emptyLayer)
            return
        }
        let sheet = StrokeSheetController(
            target: captured.target, color: paintColor,
            options: ToolOptionsStore.shared.strokeDialog
        ) { [weak self] in self?.applyStroke($0, capture: captured) }
        presentAsSheet(sheet)
    }

    // MARK: - The one commit

    /// Fills the captured target (or the current one when `capture` is nil)
    /// with `settings`, as one undo step named "Fill".
    ///
    /// `settings.opacity` is the sheet's 0…100; the core takes 0…1, so it is
    /// divided HERE, on both paths. A coverage target (mask, colour plane,
    /// alpha channel) fills with the colour's gray
    /// (`PlaneAlgebra.fillColor`), never keeps transparency, and refuses
    /// the four HSL modes by name. The recorded step carries the AUTHORED
    /// colour and the effective Preserve Transparency.
    func applyFill(
        _ settings: FillSettings, capture: OperationCapture? = nil,
        note: String = "Edit ▸ Fill…"
    ) {
        if capture == nil, document?.quickMaskActive == true {
            applyQuickMaskFill(settings, expected: nil, note: note)
            return
        }
        if let doc = document?.doc, doc.isHighDepth {
            applyPrecisionFill(settings, capture: nil, note: note)
            return
        }
        if refuseOperationScope(.fill) { return }
        guard let document = document,
              let capture = capture ?? document.captureOperation()
        else {
            NSSound.beep()
            return
        }
        if capture.target == .layer {
            guard !refuseAdjustmentPixelEdit(), !refuseGroupPixelEdit() else { return }
            guard !refuseLockedEdit(layer: capture.layerIndex, kind: DR_EDIT_PIXELS)
            else { return }
        }
        guard let color = settings.contents.color(
            foreground: paintColor, background: backgroundColor,
            custom: settings.customColor, in: document.nsColorSpace)
        else {
            NSSound.beep()
            return
        }
        let opacity = min(max(settings.opacity, 0), 100) / 100
        let coverage = capture.target.isCoverage
        let preserve = settings.preserveTransparency && !coverage
        let record: [ActionStep] = .fillSelection(
            color: color, blendMode: settings.mode, opacity: opacity * 100,
            preserveTransparency: preserve,
            target: capture.target.layerEditAgentName(in: capture.doc),
            space: document.nsColorSpace, note: note)
        // A fill reaching past a trimmed layer grows it, as Stroke… does.
        let base: RasterDocument
        do {
            base = try capture.paintDestination(mask: capture.selection, preserve: preserve)
        } catch {
            document.presentError(error)
            NSSound.beep()
            return
        }
        commitSelectionPaint(
            color: color, mode: settings.mode, opacity: opacity, preserve: preserve,
            mask: capture.selection, capture: capture, document: document, actionName: "Fill",
            record: record, noOp: FillSettings.transparentNoOp, base: base)
    }

    /// Fill… / ⌥⌫ / ⌘⌫ in Quick Mask: the whole Quick Mask coverage
    /// filled with the colour's coverage gray as one file-clean "Fill" step.
    /// `expected` is the Quick Mask state the sheet opened over (nil for a
    /// chord); a different one refuses by name. The colour is quantized to
    /// the document's bytes, so the recorded step replays exactly.
    func applyQuickMaskFill(
        _ settings: FillSettings, expected: SelectionHistoryState?, note: String
    ) {
        guard let document = document else {
            NSSound.beep()
            return
        }
        guard document.quickMaskActive,
              expected.map({ $0 == document.selectionHistory }) ?? true
        else {
            showFillAlert(FillSettings.quickMaskChanged)
            return
        }
        guard !canvas.isPaintingStroke else {
            NSSound.beep()
            return
        }
        let space = document.nsColorSpace
        guard let authored = settings.contents.color(
            foreground: paintColor, background: backgroundColor,
            custom: settings.customColor, in: space),
            let color = ColorProfile.quantized(authored, in: space)
        else {
            NSSound.beep()
            return
        }
        guard !DrBlendMode.degeneratesOnGray(settings.mode) else {
            showFillAlert(FillSettings.coverageBlendRefusal)
            return
        }
        let opacity = min(max(settings.opacity, 0), 100)
        do {
            let fill = try QuickMaskFill(color: color, mode: settings.mode, opacity: opacity / 100)
            try document.applyQuickMaskFill(
                fill,
                record: .quickMaskFill(
                    color: color, blendMode: settings.mode, opacity: opacity, space: space,
                    note: note))
        } catch {
            document.presentError(error)
            NSSound.beep()
        }
    }

    /// Edit ▸ Stroke…'s commit: the captured outline stroked with
    /// `settings.color` as one undo step named "Stroke". A selection's band
    /// is computed in CANVAS space and handed to the fill as its mask; with
    /// nothing selected on a layer the core strokes the layer's
    /// own alpha edge in LAYER space (`commitLayerEdgeStroke`). The capture
    /// keeps the real selection, so its staleness guard is unchanged.
    func applyStroke(_ settings: StrokeSettings, capture: OperationCapture? = nil) {
        if let doc = document?.doc, doc.isHighDepth {
            applyPrecisionStroke(settings, capture: nil)
            return
        }
        if refuseOperationScope(.fill) { return }
        guard let document = document,
              let capture = capture ?? document.captureOperation()
        else {
            NSSound.beep()
            return
        }
        if capture.target == .layer {
            guard !refuseAdjustmentPixelEdit(), !refuseGroupPixelEdit() else { return }
            guard !refuseLockedEdit(layer: capture.layerIndex, kind: DR_EDIT_PIXELS)
            else { return }
        }
        guard let outline = StrokeSettings.outline(of: capture) else {
            showFillAlert(StrokeSettings.noSelection)
            return
        }
        guard hasOutline(outline, capture: capture) else {
            showFillAlert(StrokeSettings.emptyLayer)
            return
        }
        guard StrokeSettings.widthRange.contains(settings.width) else {
            NSSound.beep()
            return
        }
        let opacity = min(max(settings.opacity, 0), 100) / 100
        let coverage = capture.target.isCoverage
        let preserve = settings.preserveTransparency && !coverage
        let record: [ActionStep] = .strokeSelection(
            width: settings.width, location: settings.location, color: settings.color,
            blendMode: settings.mode, opacity: opacity * 100, preserveTransparency: preserve,
            target: capture.target.layerEditAgentName(in: capture.doc),
            space: document.nsColorSpace)
        guard case .selection(let selection) = outline else {
            commitLayerEdgeStroke(
                settings, opacity: opacity, preserve: preserve, capture: capture,
                document: document, record: record)
            return
        }
        guard let band = StrokeSettings.band(
            of: selection, canvasWidth: capture.doc.width, canvasHeight: capture.doc.height,
            widthPx: settings.width, location: settings.location)
        else {
            NSSound.beep()
            return
        }
        guard band.contains(where: { $0 != 0 }) else {
            showFillAlert(StrokeSettings.emptyBand)
            return
        }
        let base: RasterDocument
        do {
            base = try capture.paintDestination(mask: band, preserve: preserve)
        } catch {
            document.presentError(error)
            NSSound.beep()
            return
        }
        commitSelectionPaint(
            color: settings.color, mode: settings.mode, opacity: opacity, preserve: preserve,
            mask: band, capture: capture, document: document, actionName: "Stroke",
            record: record, noOp: StrokeSettings.transparentNoOp,
            clippedNoOp: StrokeSettings.outsideLayerNoOp, base: base)
    }

    /// The one commit behind Fill… and Stroke…: `color` through `mode` at
    /// `opacity` (0…1) weighted by the canvas-sized `mask` (nil for the whole
    /// canvas). A coverage target fills with the colour's gray
    /// (`PlaneAlgebra.fillColor`: 30/59/11 on a colour plane, Rec. 709 on a
    /// mask or alpha channel) and
    /// refuses the four HSL modes by name. When nothing changed, `noOp` is
    /// shown if the layer's alpha was kept (Preserve Transparency or a
    /// transparency lock); otherwise `clippedNoOp`, when given (Stroke…'s
    /// colour, opacity and mode can leave every pixel under its band as it
    /// was). A layer fill reads `base` (the grown layer,
    /// `OperationCapture.paintDestination`) when given, else the capture's
    /// document.
    private func commitSelectionPaint(
        color: NSColor, mode: DrBlendMode, opacity: Double, preserve: Bool, mask: [UInt8]?,
        capture: OperationCapture, document: ImageDocument, actionName: String,
        record: [ActionStep], noOp: String, clippedNoOp: String? = nil,
        base: RasterDocument? = nil
    ) {
        if capture.target.isCoverage {
            guard !DrBlendMode.degeneratesOnGray(mode) else {
                showFillAlert(FillSettings.coverageBlendRefusal)
                return
            }
            // Read in `coverageSpace`: white stays 255 on a LUT-mode document.
            guard let bytes = ColorProfile.bytes(
                    color, in: ColorProfile.coverageSpace(document.nsColorSpace, for: color)),
                  let gray = PlaneAlgebra.fillColor(bytes, target: capture.target)
            else {
                NSSound.beep()
                return
            }
            let op = PlaneScratchOp.fillSelection(rgba: gray, mode: mode, opacity: opacity)
            document.applyCapturedRewrite(capture, actionName, record: record) {
                capture.applyingScratch(op, mask: mask)
            }
            return
        }
        let rgba = colorBytes(color)
        let source = base ?? capture.doc
        var unchanged = false
        document.applyCapturedRewrite(capture, actionName, record: record) {
            let updated = source.selectionFilled(
                capture.layerIndex, mask: mask, rgba: rgba, mode: mode,
                opacity: opacity, preserveTransparency: preserve)
            unchanged = updated == nil
            return updated
        }
        guard unchanged else { return }
        showUnchangedAlert(
            preserve: preserve, doc: capture.doc, layer: capture.layerIndex, noOp: noOp,
            clippedNoOp: clippedNoOp)
    }

    /// Stroke… with nothing selected on a layer: the core strokes the
    /// layer's own alpha edge in LAYER space (`strokingLayerEdge`) — growing
    /// the layer past the canvas when the band reaches there and its alpha
    /// is free to change — as one undo step named "Stroke". A refusal the
    /// core names is presented after the edit closes; an unchanged result
    /// shows the same alert `commitSelectionPaint` does.
    private func commitLayerEdgeStroke(
        _ settings: StrokeSettings, opacity: Double, preserve: Bool,
        capture: OperationCapture, document: ImageDocument, record: [ActionStep]
    ) {
        let rgba = colorBytes(settings.color)
        var unchanged = false
        var failure: Error?
        document.applyCapturedRewrite(capture, "Stroke", record: record) {
            do {
                let updated = try capture.doc.strokingLayerEdge(
                    capture.layerIndex, widthPx: settings.width, location: settings.location.dr,
                    rgba: rgba, mode: settings.mode, opacity: opacity,
                    preserveTransparency: preserve)
                unchanged = updated == nil
                return updated
            } catch {
                failure = error
                return nil
            }
        }
        if let failure {
            document.presentError(failure)
            NSSound.beep()
            return
        }
        guard unchanged else { return }
        showUnchangedAlert(
            preserve: preserve, doc: capture.doc, layer: capture.layerIndex,
            noOp: StrokeSettings.transparentNoOp, clippedNoOp: StrokeSettings.outsideLayerNoOp)
    }

    /// Whether `outline` has anything to stroke: a selection with some
    /// coverage, or a layer with some alpha (`StrokeSettings.layerHasOutline`).
    private func hasOutline(_ outline: StrokeOutline, capture: OperationCapture) -> Bool {
        switch outline {
        case .selection(let selection): return selection.contains(where: { $0 != 0 })
        case .layerEdge: return StrokeSettings.layerHasOutline(capture)
        }
    }

    /// The alert for a Fill… or Stroke… that changed nothing: `noOp` when the
    /// layer's alpha was kept (Preserve Transparency or a transparency lock),
    /// otherwise `clippedNoOp` when given. The 16-bit commits share it.
    func showUnchangedAlert(
        preserve: Bool, doc: RasterDocument, layer: Int, noOp: String, clippedNoOp: String?
    ) {
        if preserve || doc.transparencyLockOwner(layer) != nil {
            showFillAlert(noOp)
        } else if let clippedNoOp = clippedNoOp {
            showFillAlert(clippedNoOp)
        }
    }

    func showFillAlert(_ message: String) {
        NSSound.beep()
        let alert = NSAlert()
        alert.messageText = message
        if let window = view.window { alert.beginSheetModal(for: window) }
        else { alert.runModal() }
    }

    // MARK: - The ⌫-chord guard and validation

    /// Whether a ⌫ key-equivalent chord may reach the menu: not while text
    /// is being edited in this window or in the key window (a field editor
    /// or the canvas text view — both NSText), not while a guide is grabbed
    /// (⌫ deletes it), not while a selection gesture is pending (⌫ pops a
    /// Polygonal Lasso vertex; a drag commits against its mouse-down
    /// capture), and not inside a text, transform or shape-edit session.
    /// Crop sessions are deliberately not excluded.
    ///
    /// The command palette's own search field does not count as "editing
    /// text": re-invoking ⌘F while the palette is key re-walks the menu with
    /// the palette key, and Clear and Fill must not drop out of its list.
    var deleteChordAllowed: Bool {
        !(view.window?.firstResponder is NSText) && !keyWindowIsEditingText
            && canvas.guideDrag == nil && !canvas.hasPendingSelectionGesture
            && !canvas.hasActiveTextSession && !isTransforming && shapeEditSession == nil
    }

    private var keyWindowIsEditingText: Bool {
        guard let key = NSApp.keyWindow,
              !(key.windowController is CommandPaletteWindowController)
        else { return false }
        return key.firstResponder is NSText
    }

    /// Fill… and its chords; nil for any other item. Fill… itself carries
    /// no ⌫ chord (⇧F5), so only its hidden ⇧⌫ twin and the four ⌥⌫ / ⌘⌫ fills
    /// ask `deleteChordAllowed`. In Quick Mask all six fill the Quick Mask
    /// and stand down only while a stroke is being painted; Stroke…
    /// stays disabled there. Stroke… with nothing selected is enabled on a
    /// layer target only.
    func validateFillItem(_ item: NSValidatedUserInterfaceItem) -> Bool? {
        if document?.quickMaskActive == true {
            switch item.action {
            case #selector(fillSelection(_:)):
                return !canvas.isPaintingStroke
            case #selector(fillSelectionFromDeleteKey(_:)),
                 #selector(fillWithForeground(_:)), #selector(fillWithBackground(_:)),
                 #selector(fillWithForegroundPreserving(_:)),
                 #selector(fillWithBackgroundPreserving(_:)):
                return deleteChordAllowed && !canvas.isPaintingStroke
            default:
                break
            }
        }
        switch item.action {
        case #selector(fillSelection(_:)):
            return supportsOperationScope(.fill)
        case #selector(strokeSelection(_:)):
            return (canvas.selection != nil || paintTarget == .layer)
                && supportsOperationScope(.fill)
        case #selector(fillSelectionFromDeleteKey(_:)),
             #selector(fillWithForeground(_:)), #selector(fillWithBackground(_:)),
             #selector(fillWithForegroundPreserving(_:)),
             #selector(fillWithBackgroundPreserving(_:)):
            return deleteChordAllowed && supportsOperationScope(.fill)
        default:
            return nil
        }
    }
}
