import AppKit

/// Filters ▸ Liquify…: a modal workspace on the active raster layer.
/// Drags in the preview push, twirl, pucker, bloat, reconstruct, freeze or
/// thaw through `LiquifySession`, which previews on a proxy; the document is
/// untouched until Apply, which runs the same strokes at full size as one
/// undo step (`EditorViewController.commitLiquify`). Settings are not kept
/// between openings.
final class LiquifySheetController: NSViewController {
    /// The popup's tools, in order. Twirl is one entry (clockwise); ⌥ at
    /// mouse-down turns it, and every other tool, the other way.
    static let tools = LiquifyTool.allCases.filter { $0 != .twirlCounterclockwise }

    private weak var editor: EditorViewController?
    /// The edit's captured inputs: one of the two, by the document's depth.
    private let byte: OperationCapture?
    private let precision: PrecisionEditorCapture?
    private let colorSpace: CGColorSpace
    private(set) var session: LiquifySession
    let canvasView = LiquifyCanvasView(frame: .zero)
    let status = NSTextField(wrappingLabelWithString: "")
    private let scroll = NSScrollView()
    private let zoomLabel = NSTextField(labelWithString: "")
    private let toolPopup = NSPopUpButton()
    private let showMesh = NSButton(checkboxWithTitle: "Show Mesh", target: nil, action: nil)
    private let showMask = NSButton(checkboxWithTitle: "Show Mask", target: nil, action: nil)
    /// Size, Pressure, Density, in that order (a control's tag is its index).
    private var fields: [NSTextField] = []
    private var sliders: [NSSlider] = []
    private var undoButton: NSButton!
    private var restoreButton: NSButton!
    private var applyButton: NSButton!
    /// Runs while the mouse is down with a tool that dwells.
    private var dwellTimer: Timer?
    private var movedSinceTick = false
    /// The pointer in proxy pixels, for the brush outline.
    private var pointer: CGPoint?

    /// Captures the editor's document, active layer and selection. Throws
    /// when the layer cannot be liquified or the workspace does not fit.
    init(editor: EditorViewController) throws {
        guard let document = editor.document, let doc = document.doc else {
            throw DoctoredCoreError(message: "Liquify needs an open document.")
        }
        if doc.isHighDepth {
            precision = try PrecisionEditorCapture(document)
            byte = nil
        } else {
            guard let capture = document.captureOperation(), capture.target == .layer else {
                throw DoctoredCoreError(message: "Liquify edits layer pixels only; select the layer's pixels first.")
            }
            byte = capture
            precision = nil
        }
        colorSpace = doc.colorSpace
        session = try LiquifySession(document: document)
        self.editor = editor
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("LiquifySheetController does not support NSCoder") }

    override func loadView() {
        let proxy = session.proxy
        canvasView.frame = NSRect(x: 0, y: 0, width: proxy.width, height: proxy.height)
        canvasView.onStrokeBegin = { [weak self] in self?.strokeBegan(at: $0, modifiers: $1) ?? false }
        canvasView.onStrokeMove = { [weak self] in self?.strokeMoved(to: $0) }
        canvasView.onStrokeEnd = { [weak self] in self?.strokeEnded() }
        canvasView.onHover = { [weak self] in self?.pointerMoved(to: $0) }
        if let mask = session.clipMask {
            canvasView.baseImage = proxy.makeCGImage(in: colorSpace)
            canvasView.clipMask = CanvasSelection.grayImage(mask, proxy.width, proxy.height)
        }
        scroll.documentView = canvasView
        scroll.hasHorizontalScroller = true
        scroll.hasVerticalScroller = true
        scroll.allowsMagnification = true
        scroll.minMagnification = 0.01
        scroll.maxMagnification = 8
        scroll.widthAnchor.constraint(equalToConstant: 560).isActive = true
        scroll.heightAnchor.constraint(equalToConstant: 380).isActive = true
        let fit = StickerButton(title: "Fit", style: .secondary, target: self, action: #selector(fitClicked(_:)))
        fit.identifier = NSUserInterfaceItemIdentifier("liquify.fit")
        let actual = StickerButton(title: "100%", style: .secondary, target: self, action: #selector(actualClicked(_:)))
        actual.identifier = NSUserInterfaceItemIdentifier("liquify.actual")
        zoomLabel.font = DS.mono(11)
        let toolbar = NSStackView(views: [fit, actual, zoomLabel])
        toolbar.spacing = 8
        let preview = NSStackView(views: [scroll, toolbar])
        preview.orientation = .vertical; preview.alignment = .leading; preview.spacing = 10

        toolPopup.addItems(withTitles: Self.tools.map { $0 == .twirlClockwise ? "Twirl" : $0.title })
        toolPopup.identifier = NSUserInterfaceItemIdentifier("liquify.tool")
        toolPopup.target = self; toolPopup.action = #selector(toolChanged(_:))
        toolPopup.toolTip = "Drag in the preview. Hold ⌥ as you press to reverse the tool."
        let toolRow = NSStackView(views: [fieldLabel("Tool"), toolPopup])
        toolRow.distribution = .fill; toolRow.spacing = 8
        var controls: [NSView] = [toolRow]
        // The slider's reach follows the layer; the field takes any size to 5,000 px.
        let longest = Double(max(session.width, session.height))
        let rows: [(name: String, id: String, value: Double, low: Double, high: Double)] = [
            ("Size (px)", "size", session.size, 1, min(5000, max(100, longest))),
            ("Pressure (%)", "pressure", session.pressure, 0, 100),
            ("Density (%)", "density", session.density, 0, 100),
        ]
        for (index, row) in rows.enumerated() {
            let field = selectionSheetField(row.value, identifier: "liquify.\(row.id)",
                                            target: self, action: #selector(fieldChanged(_:)))
            field.tag = index
            field.stringValue = Self.text(row.value)
            let slider = NSSlider(value: row.value, minValue: row.low, maxValue: row.high,
                                  target: self, action: #selector(sliderChanged(_:)))
            slider.tag = index; slider.isContinuous = true
            slider.widthAnchor.constraint(equalToConstant: 210).isActive = true
            fields.append(field); sliders.append(slider)
            let line = NSStackView(views: [fieldLabel(row.name), field])
            line.distribution = .fill; line.spacing = 8
            controls.append(line); controls.append(slider)
        }
        showMesh.identifier = NSUserInterfaceItemIdentifier("liquify.mesh")
        showMesh.target = self; showMesh.action = #selector(overlayChanged(_:))
        showMask.identifier = NSUserInterfaceItemIdentifier("liquify.mask")
        showMask.state = .on
        showMask.target = self; showMask.action = #selector(overlayChanged(_:))
        showMask.toolTip = "Tint the areas Freeze Mask protects."
        // No key equivalent on either: ⌘Z belongs to Edit ▸ Undo.
        undoButton = StickerButton(title: "Undo Stroke", style: .secondary, target: self,
                                   action: #selector(undoStrokeClicked(_:)))
        undoButton.identifier = NSUserInterfaceItemIdentifier("liquify.undo")
        restoreButton = StickerButton(title: "Restore All", style: .secondary, target: self,
                                      action: #selector(restoreAllClicked(_:)))
        restoreButton.identifier = NSUserInterfaceItemIdentifier("liquify.restore")
        let buttons = NSStackView(views: [undoButton, restoreButton])
        buttons.spacing = 8
        controls += [showMesh, showMask, buttons]
        let options = NSStackView(views: controls)
        options.orientation = .vertical; options.alignment = .leading; options.spacing = 6
        options.widthAnchor.constraint(equalToConstant: 220).isActive = true
        let row = NSStackView(views: [preview, options])
        row.orientation = .horizontal; row.alignment = .top; row.spacing = 24
        status.identifier = NSUserInterfaceItemIdentifier("liquify.status")
        status.font = DS.sans(11); status.textColor = DS.textMuted
        status.preferredMaxLayoutWidth = 800
        let content = NSStackView(views: [row, status])
        content.orientation = .vertical; content.alignment = .leading; content.spacing = 12
        applyButton = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        applyButton.identifier = NSUserInterfaceItemIdentifier("liquify.apply")
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        cancel.identifier = NSUserInterfaceItemIdentifier("liquify.cancel")
        view = makeSheetView(title: "Liquify",
            hint: "Drag in the preview to reshape the layer. Freeze Mask protects what you paint over. Apply creates one reversible edit.",
            content: content, buttonRow: makeButtonRow(cancel: cancel, apply: applyButton), width: 856)
        refresh()
    }

    override func viewDidAppear() {
        super.viewDidAppear()
        fitClicked(nil)
    }

    override func viewWillDisappear() {
        super.viewWillDisappear()
        stopDwelling()
    }

    // MARK: - Controls

    private static func text(_ value: Double) -> String {
        value == value.rounded() ? String(Int(value)) : String(value)
    }

    @objc private func toolChanged(_ sender: Any?) {
        let index = toolPopup.indexOfSelectedItem
        if Self.tools.indices.contains(index) { session.tool = Self.tools[index] }
    }

    @objc private func sliderChanged(_ sender: NSSlider) { setAmount(sender.tag, sender.doubleValue.rounded()) }
    @objc private func fieldChanged(_ sender: NSTextField) { setAmount(sender.tag, Double(sender.stringValue) ?? .nan) }

    /// Size (1…5,000 px), Pressure or Density (0…100 %), clamped; a value that
    /// is not a number puts the current one back in the field.
    private func setAmount(_ index: Int, _ value: Double) {
        guard fields.indices.contains(index) else { return }
        if value.isFinite {
            switch index {
            case 0: session.size = min(5000, max(1, value))
            case 1: session.pressure = min(100, max(0, value))
            default: session.density = min(100, max(0, value))
            }
        }
        let current = [session.size, session.pressure, session.density][index]
        fields[index].stringValue = Self.text(current)
        sliders[index].doubleValue = current
        showBrush()
    }

    @objc private func overlayChanged(_ sender: Any?) { showOverlays() }

    @objc func fitClicked(_ sender: Any?) {
        let size = scroll.contentSize
        let scale = min(size.width / canvasView.bounds.width, size.height / canvasView.bounds.height, 1)
        scroll.magnification = max(scroll.minMagnification, scale)
        zoomLabel.stringValue = "\(Int(scroll.magnification * 100))%"
    }

    @objc func actualClicked(_ sender: Any?) {
        scroll.magnification = 1
        zoomLabel.stringValue = "100%"
    }

    // MARK: - Display

    private func showPreview() { canvasView.image = session.preview.makeCGImage(in: colorSpace) }

    /// The freeze coverage, tinted by the view.
    private func showFreeze() {
        let proxy = session.proxy
        canvasView.freezeImage = showMask.state == .on
            ? session.freezeMask().flatMap { CanvasSelection.grayImage($0, proxy.width, proxy.height) } : nil
    }

    /// The mesh and the freeze overlay. The mesh is a whole-proxy render, so
    /// it follows a stroke when the stroke ends, not as it is dragged.
    private func showOverlays() {
        canvasView.meshImage = showMesh.state == .on
            ? (try? session.meshImage())?.makeCGImage(in: ColorProfile.sRGB) : nil
        showFreeze()
    }

    private func showBrush() {
        canvasView.brush = pointer.map { (center: $0, radius: session.proxyBrushRadius) }
    }

    /// The stroke list changed: the picture, its overlays and the buttons.
    private func refresh() {
        showPreview()
        showOverlays()
        let any = !session.strokes.isEmpty
        undoButton.isEnabled = any
        restoreButton.isEnabled = any
        applyButton.isEnabled = any
    }

    /// One beep and the sentence on the status line.
    private func refuse(_ error: Error) {
        NSSound.beep()
        status.stringValue = error.localizedDescription
    }

    // MARK: - Drags (forwarded by `LiquifyCanvasView`, in proxy pixels)

    func pointerMoved(to point: CGPoint?) {
        pointer = point
        showBrush()
    }

    func strokeBegan(at point: CGPoint, modifiers: NSEvent.ModifierFlags) -> Bool {
        do {
            try session.begin(at: session.canvasPoint(proxy: point), reverse: modifiers.contains(.option))
        } catch {
            refuse(error)
            return false
        }
        status.stringValue = ""
        movedSinceTick = true
        if session.draft?.tool.dwells == true { startDwelling() }
        strokeChanged()
        return true
    }

    func strokeMoved(to point: CGPoint) {
        movedSinceTick = true
        extendStroke(to: session.canvasPoint(proxy: point), dwell: false)
    }

    func strokeEnded() {
        stopDwelling()
        guard session.finish() else { return }
        refresh()
    }

    /// A limit or the core's work refusal ends the stroke at its last
    /// accepted point.
    private func extendStroke(to point: CGPoint, dwell: Bool) {
        do {
            if try session.append(point, dwell: dwell) { strokeChanged() }
        } catch {
            refuse(error)
            strokeEnded()
        }
    }

    /// During a drag: the picture, and the mask while it is being painted.
    private func strokeChanged() {
        showPreview()
        if let tool = session.draft?.tool, tool == .freeze || tool == .thaw { showFreeze() }
    }

    /// A tool that dwells keeps working while the pointer rests: fifteen times
    /// a second, a tick that saw no drag since the one before repeats the last
    /// point. `.common`, so it also fires while AppKit tracks the mouse.
    private func startDwelling() {
        stopDwelling()
        let timer = Timer(timeInterval: 1.0 / 15, repeats: true) { [weak self] _ in self?.dwellTick() }
        RunLoop.main.add(timer, forMode: .common)
        dwellTimer = timer
    }

    private func stopDwelling() {
        dwellTimer?.invalidate()
        dwellTimer = nil
    }

    private func dwellTick() {
        if !movedSinceTick, let last = session.draft?.points.last { extendStroke(to: last, dwell: true) }
        movedSinceTick = false
    }

    // MARK: - Buttons

    @objc func undoStrokeClicked(_ sender: Any?) {
        do {
            guard try session.undoStroke() else { NSSound.beep(); return }
        } catch {
            refuse(error)
            return
        }
        status.stringValue = ""
        refresh()
    }

    @objc func restoreAllClicked(_ sender: Any?) {
        do {
            guard try session.restoreAll() else { NSSound.beep(); return }
        } catch {
            refuse(error)
            return
        }
        status.stringValue = ""
        refresh()
    }

    /// The session lets go of its fields, preview and reservation before the
    /// full-size job is admitted; the view keeps its last frame, so the
    /// picture does not snap back while the job runs. A job that changed
    /// nothing (or did not complete) rebuilds the session and keeps the sheet.
    @objc func applyClicked(_ sender: Any?) {
        strokeEnded()
        guard let editor, !session.strokes.isEmpty else { NSSound.beep(); return }
        guard BackgroundWork.awaitSlotForSheetApply(status: status) else { return }
        session.suspend()
        canvasView.meshImage = nil
        canvasView.freezeImage = nil
        if editor.commitLiquify(session.strokes, byte: byte, precision: precision, record: .liquify(session.strokes)) {
            dismiss(self)
            return
        }
        do {
            try session.resume()
            refresh()
            status.stringValue = "Liquify changed no pixel, or the edit did not complete."
        } catch { status.stringValue = error.localizedDescription }
    }

    @objc func cancelClicked(_ sender: Any?) {
        stopDwelling()
        session.suspend()
        dismiss(self)
    }
}
