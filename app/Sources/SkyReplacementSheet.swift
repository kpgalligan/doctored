import AppKit
import UniformTypeIdentifiers

/// The dialog's preview: the proxy with its sky replaced, drawn aspect-fit. Dragging it moves the
/// sky; `onDrag` hands the move over in canvas fractions.
final class SkyPreviewView: NSView {
    var image: CGImage? { didSet { needsDisplay = true } }
    var onDrag: ((CGFloat, CGFloat) -> Void)?
    private var last: NSPoint?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.backgroundColor = DS.canvasVoid.cgColor
        setAccessibilityIdentifier("sky-replacement-preview")
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SkyPreviewView does not support NSCoder")
    }

    /// Where the image is drawn: aspect-fit, centred.
    var imageRect: NSRect {
        guard let image, image.width > 0, image.height > 0 else { return .zero }
        let scale = min(bounds.width / CGFloat(image.width), bounds.height / CGFloat(image.height))
        let size = NSSize(width: CGFloat(image.width) * scale, height: CGFloat(image.height) * scale)
        return NSRect(x: bounds.midX - size.width / 2, y: bounds.midY - size.height / 2,
                      width: size.width, height: size.height)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let image, let context = NSGraphicsContext.current?.cgContext else { return }
        context.interpolationQuality = .medium
        context.draw(image, in: imageRect)
    }

    override func mouseDown(with event: NSEvent) { last = convert(event.locationInWindow, from: nil) }

    override func mouseDragged(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let rect = imageRect
        defer { last = point }
        guard let last, rect.width > 0, rect.height > 0 else { return }
        // View y grows up; the canvas's grows down.
        onDrag?((point.x - last.x) / rect.width, -(point.y - last.y) / rect.height)
    }

    override func mouseUp(with event: NSEvent) { last = nil }
}

/// Edit ▸ Sky Replacement…: the sky model runs once when the sheet opens
/// (`SkyReplacementRun.mask`); the proxy is the layer's 8-bit display at most 1024 px. Each sky
/// pick makes one reduced copy (`SkyLibrary.previewImage`), and every change renders the proxy
/// through `RasterImage.skyReplaced` with that copy, synchronously, into `SkyPreviewView`. OK
/// runs `SkyReplacementRun.apply` with the full sky and records `sky_replacement`.
final class SkyReplacementSheetController: NSViewController {
    /// The longest side of the preview's proxy.
    static let proxyLongSide = 1024

    private let document: ImageDocument
    private let layer: Int
    private let coverage: [UInt16]
    private let entry: ModelCatalogEntry
    private let canvasSize: (width: Int, height: Int)
    private let colorSpace: CGColorSpace
    /// The layer's 8-bit display, reduced to `proxyLongSide`.
    let proxy: RasterImage
    private(set) var settings = SkyReplacementSettings()
    /// The picked sky, and the reduced copy the preview renders with.
    private(set) var choice: SkyChoice?
    private(set) var previewSky: RasterImage?
    let preview = SkyPreviewView(frame: NSRect(x: 0, y: 0, width: 420, height: 260))
    private let skyPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    /// One row per popup item: a sky, Add from File, Remove, or nil for a header or separator.
    private var popupRows: [PopupRow?] = []
    private let downloadNote = NSTextField(labelWithString: "Download the sky library in Settings ▸ Models")
    private let status = NSTextField(labelWithString: "")
    private var sliders: [NSSlider] = []
    private var valueLabels: [NSTextField] = []
    private let flip = NSButton(checkboxWithTitle: "Flip", target: nil, action: nil)
    private let decontaminate = NSButton(checkboxWithTitle: "Decontaminate Edge", target: nil, action: nil)
    private let lightingMode = NSPopUpButton(frame: .zero, pullsDown: false)
    private let output = NSPopUpButton(frame: .zero, pullsDown: false)
    private var okButton: NSButton?

    private enum PopupRow {
        case sky(SkyChoice)
        case addFile
        case removeUser
    }

    init(document: ImageDocument, layer: Int, coverage: [UInt16], entry: ModelCatalogEntry) throws {
        guard let doc = document.doc else { throw DoctoredCoreError(message: "Document has no image") }
        self.document = document
        self.layer = layer
        self.coverage = coverage
        self.entry = entry
        canvasSize = (doc.width, doc.height)
        colorSpace = doc.colorSpace
        let display: RasterImage = try autoreleasepool {
            if doc.isHighDepth { return try doc.nativeDisplay(layer: layer) }
            guard let image = doc.layerCanvasImage(layer) else {
                throw DoctoredCoreError(message: "The layer could not be prepared for Sky Replacement.")
            }
            return image
        }
        let scale = min(1, Double(Self.proxyLongSide) / Double(max(display.width, display.height)))
        if scale < 1 {
            let w = max(1, Int((Double(display.width) * scale).rounded()))
            let h = max(1, Int((Double(display.height) * scale).rounded()))
            proxy = display.resized(w: w, h: h, filter: DR_FILTER_LANCZOS3) ?? display
        } else {
            proxy = display
        }
        super.init(nibName: nil, bundle: nil)
        preview.onDrag = { [weak self] dx, dy in self?.dragged(dx, dy) }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("SkyReplacementSheetController does not support NSCoder")
    }

    override func loadView() {
        skyPopup.target = self
        skyPopup.action = #selector(skyChanged(_:))
        skyPopup.setAccessibilityIdentifier("sky-replacement-sky")
        downloadNote.textColor = DS.textMuted
        downloadNote.font = DS.sans(11)
        let skyRow = NSStackView(views: [fieldLabel("Sky:"), skyPopup, downloadNote])
        skyRow.orientation = .horizontal
        skyRow.spacing = 12

        var rows: [[NSView]] = []
        for slider in SkyReplacementSettings.sliders {
            let control = NSSlider(value: slider.initial, minValue: slider.range.lowerBound,
                                   maxValue: slider.range.upperBound, target: self,
                                   action: #selector(sliderChanged(_:)))
            control.isContinuous = true
            control.widthAnchor.constraint(equalToConstant: 180).isActive = true
            control.setAccessibilityIdentifier("sky-replacement-\(slider.key)")
            let label = NSTextField(labelWithString: String(format: "%.0f", slider.initial))
            label.font = DS.mono(12)
            label.textColor = DS.textMuted
            label.alignment = .right
            label.widthAnchor.constraint(equalToConstant: 52).isActive = true
            sliders.append(control)
            valueLabels.append(label)
            rows.append([fieldLabel(slider.label), control, label])
        }
        let grid = NSGridView(views: rows)
        grid.rowSpacing = 8
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 140

        flip.target = self
        flip.action = #selector(flipChanged(_:))
        flip.setAccessibilityIdentifier("sky-replacement-flip")
        lightingMode.addItems(withTitles: ["Multiply", "Screen"])
        lightingMode.target = self
        lightingMode.action = #selector(lightingModeChanged(_:))
        lightingMode.setAccessibilityIdentifier("sky-replacement-lighting-mode")
        output.addItems(withTitles: SkyReplacementOutput.allCases.map(\.title))
        output.selectItem(at: SkyReplacementOutput.allCases.firstIndex(of: settings.output) ?? 0)
        output.target = self
        output.action = #selector(outputChanged(_:))
        output.setAccessibilityIdentifier("sky-replacement-output")
        let modeRow = NSStackView(views: [flip, fieldLabel("Lighting Mode:"), lightingMode])
        modeRow.orientation = .horizontal
        modeRow.spacing = 12
        decontaminate.state = settings.decontaminate ? .on : .off
        decontaminate.target = self
        decontaminate.action = #selector(decontaminateChanged(_:))
        decontaminate.setAccessibilityIdentifier("sky-replacement-decontaminate")
        let outputRow = NSStackView(views: [fieldLabel("Output To:"), output])
        outputRow.orientation = .horizontal
        outputRow.spacing = 12

        preview.translatesAutoresizingMaskIntoConstraints = false
        preview.widthAnchor.constraint(equalToConstant: 420).isActive = true
        preview.heightAnchor.constraint(equalToConstant: 260).isActive = true
        status.textColor = .systemRed
        status.font = DS.sans(11)
        status.lineBreakMode = .byWordWrapping
        status.maximumNumberOfLines = 3
        status.isHidden = true
        status.setAccessibilityIdentifier("sky-replacement-status")

        let content = NSStackView(views: [skyRow, preview, grid, modeRow, decontaminate, outputRow,
                                          status])
        content.orientation = .vertical
        content.alignment = .leading
        content.spacing = 10
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let apply = sheetApplyButton(target: self, action: #selector(applyClicked(_:)))
        apply.title = "OK"
        okButton = apply
        view = makeSheetView(
            title: "Sky Replacement",
            hint: "Pick a sky; drag the preview to move it.",
            content: content,
            buttonRow: makeButtonRow(cancel: cancel, apply: apply,
                                     leading: [sheetFootnote("one undo step")]),
            width: 520)
        rebuildPopup(selecting: nil)
        if case .sky(let first)? = popupRows.compactMap({ $0 }).first { select(first) }
        updateOK()
    }

    // MARK: - The sky popup

    /// The popup: the library by category, the user's skies, then Add Sky from File… and Remove
    /// User Sky. With no sky at all it offers Add from File and shows the download note.
    private func rebuildPopup(selecting selected: SkyChoice?) {
        skyPopup.removeAllItems()
        popupRows = []
        let menu = skyPopup.menu ?? NSMenu()
        menu.autoenablesItems = false
        func header(_ title: String) {
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            item.isEnabled = false
            menu.addItem(item)
            popupRows.append(nil)
        }
        func add(_ title: String, _ row: PopupRow, enabled: Bool = true) {
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            item.isEnabled = enabled
            menu.addItem(item)
            popupRows.append(row)
        }
        let presets = SkyLibrary.presets()
        let users = (try? SkyLibrary.userSkies()) ?? []
        for category in SkyLibrary.categories {
            let members = presets.filter { $0.category == category }
            guard !members.isEmpty else { continue }
            header(category.prefix(1).uppercased() + category.dropFirst())
            for preset in members { add("   " + preset.name, .sky(.library(preset))) }
        }
        if !users.isEmpty {
            header("Your Skies")
            for sky in users { add("   " + sky.name, .sky(.user(sky))) }
        }
        if !popupRows.isEmpty {
            menu.addItem(.separator())
            popupRows.append(nil)
        }
        add("Add Sky from File…", .addFile)
        if !users.isEmpty { add("Remove User Sky", .removeUser) }
        downloadNote.isHidden = !presets.isEmpty
        let target = selected ?? choice
        if let target, let index = popupRows.firstIndex(where: { row in
            if case .sky(let c)? = row { return c == target }
            return false
        }) {
            skyPopup.selectItem(at: index)
        } else if let index = popupRows.firstIndex(where: { if case .sky? = $0 { return true }; return false }) {
            skyPopup.selectItem(at: index)
        }
    }

    @objc private func skyChanged(_ sender: Any?) {
        let index = skyPopup.indexOfSelectedItem
        guard popupRows.indices.contains(index), let row = popupRows[index] else { return }
        switch row {
        case .sky(let picked): select(picked)
        case .addFile: addFromFile()
        case .removeUser: removeUserSky()
        }
    }

    private func addFromFile() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.image]
        panel.allowsMultipleSelection = false
        panel.message = "Choose a sky image"
        guard panel.runModal() == .OK, let url = panel.url else {
            rebuildPopup(selecting: choice)
            return
        }
        do {
            let sky = try SkyLibrary.addUserSky(from: url)
            rebuildPopup(selecting: .user(sky))
            select(.user(sky))
        } catch {
            rebuildPopup(selecting: choice)
            showStatus(error.localizedDescription)
        }
    }

    private func removeUserSky() {
        guard case .user(let sky)? = choice else {
            NSSound.beep()
            rebuildPopup(selecting: choice)
            return
        }
        do { try SkyLibrary.deleteUserSky(named: sky.name) } catch {
            showStatus(error.localizedDescription)
        }
        choice = nil
        previewSky = nil
        rebuildPopup(selecting: nil)
        if let index = popupRows.firstIndex(where: { if case .sky? = $0 { return true }; return false }),
           case .sky(let next)? = popupRows[index] {
            select(next)
        } else {
            preview.image = nil
        }
        updateOK()
    }

    /// Picks `picked`: its full image is read once and reduced for the preview, then dropped.
    func select(_ picked: SkyChoice) {
        do {
            let full = try SkyLibrary.image(picked)
            previewSky = SkyLibrary.previewImage(full, proxyWidth: proxy.width, proxyHeight: proxy.height)
            choice = picked
            showStatus(nil)
            render()
        } catch {
            // No re-render: the preview still shows the previous pick, and a successful render
            // clears the status, which would hide this refusal.
            showStatus(error.localizedDescription)
        }
        updateOK()
    }

    // MARK: - Controls

    @objc private func sliderChanged(_ sender: Any?) {
        guard let slider = sender as? NSSlider,
              let index = sliders.firstIndex(where: { $0 === slider }) else { return }
        slider.doubleValue = slider.doubleValue.rounded()
        settings.set(SkyReplacementSettings.sliders[index].key, slider.doubleValue)
        valueLabels[index].stringValue = String(format: "%.0f", slider.doubleValue)
        render()
    }

    @objc private func flipChanged(_ sender: Any?) {
        settings.flip = flip.state == .on
        render()
    }

    /// Decontaminate Edge: the preview shows the edge with and without the old sky's colour.
    @objc private func decontaminateChanged(_ sender: Any?) {
        settings.decontaminate = decontaminate.state == .on
        render()
    }

    @objc private func lightingModeChanged(_ sender: Any?) {
        settings.lightingMode = lightingMode.indexOfSelectedItem == 1 ? .screen : .multiply
        render()
    }

    /// Output To: the preview is the same picture either way, so nothing re-renders.
    @objc private func outputChanged(_ sender: Any?) {
        let cases = SkyReplacementOutput.allCases
        settings.output = cases.indices.contains(output.indexOfSelectedItem)
            ? cases[output.indexOfSelectedItem] : .newLayers
    }

    private func dragged(_ dx: CGFloat, _ dy: CGFloat) {
        settings.offset = CGPoint(x: min(1, max(-1, settings.offset.x + dx)),
                                  y: min(1, max(-1, settings.offset.y + dy)))
        render()
    }

    private func updateOK() { okButton?.isEnabled = choice != nil }

    private func showStatus(_ message: String?) {
        status.stringValue = message ?? ""
        status.isHidden = message == nil
    }

    /// One synchronous preview of the current settings with the reduced sky.
    private func render() {
        guard let previewSky else { return }
        do {
            let rendered = try proxy.skyReplaced(
                sky: previewSky, coverage: coverage, coverageWidth: canvasSize.width,
                coverageHeight: canvasSize.height, params: settings.coreParams, control: nil)
            preview.image = rendered.makeCGImage(in: colorSpace)
            showStatus(nil)
        } catch {
            showStatus(error.localizedDescription)
        }
    }

    // MARK: - OK and Cancel

    @objc private func applyClicked(_ sender: Any?) {
        guard let choice else { NSSound.beep(); return }
        let settings = settings
        dismiss(self)
        do {
            let sky = try SkyLibrary.image(choice)
            _ = try SkyReplacementRun.apply(
                document: document, layer: layer, settings: settings, sky: sky, coverage: coverage,
                record: .skyReplacement(settings, sky: choice,
                                        note: ModelGate.note(SkyReplacementRun.menuPath, entry)))
        } catch is JobCancelled {
        } catch {
            document.presentError(error)
        }
    }

    @objc private func cancelClicked(_ sender: Any?) { dismiss(self) }
}

extension EditorViewController {
    /// Edit ▸ Sky Replacement…: offers Settings ▸ Models when the sky model is not installed,
    /// else runs it once and presents the sheet. MCP twin `sky_replacement`.
    @objc func showSkyReplacement(_ sender: Any?) {
        guard let document, !isTransforming, !document.quickMaskActive,
              document.planeEditTarget == .layer else { NSSound.beep(); return }
        let layer = document.activeLayerIndex
        if let reason = SkyReplacementRun.refusal(document: document, layer: layer) {
            NSSound.beep()
            document.presentError(DoctoredCoreError(message: reason))
            return
        }
        guard ModelGate.ready("sky") != nil else {
            ModelGate.offerSettings(ModelGate.notInstalled("sky"))
            return
        }
        do {
            let mask = try SkyReplacementRun.mask(document: document)
            presentAsSheet(try SkyReplacementSheetController(document: document, layer: layer,
                                                             coverage: mask.coverage, entry: mask.entry))
        } catch is JobCancelled {
        } catch {
            document.presentError(error)
        }
    }

    /// The active layer can take Sky Replacement (`SkyReplacementRun.refusal`), with the layer
    /// itself targeted and no Quick Mask, transform or text session.
    func validateSkyReplacementItem() -> Bool {
        guard let document, !document.quickMaskActive, document.planeEditTarget == .layer,
              !isTransforming, !canvas.hasActiveTextSession else { return false }
        return SkyReplacementRun.refusal(document: document, layer: document.activeLayerIndex) == nil
    }
}
