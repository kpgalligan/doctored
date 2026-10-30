import AppKit

/// A short controls document stays at the top of the scroll viewport; a tall
/// document starts with its first control visible and scrolls downward.
private final class RawDevelopControlsDocument: NSView {
    override var isFlipped: Bool { true }

    override init(frame frameRect: NSRect) { super.init(frame: frameRect) }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
}

extension RawDevelopWindowController {
    var workflowCardWidth: CGFloat {
        guard descriptor != nil, snapshot != nil else { return 560 }
        return min(1050, max(760, (NSScreen.main?.visibleFrame.width ?? 1280) - 40))
    }

    func workflowContent(controls globalControls: NSStackView, legacyPreview: NSView) -> NSView {
        let controls = localSidebar(global: globalControls)
        guard descriptor != nil, snapshot != nil else {
            let stack = NSStackView(views: [legacyPreview, controls])
            stack.orientation = .vertical
            stack.alignment = .leading
            stack.spacing = 16
            return stack
        }
        let height = min(660, max(400, (NSScreen.main?.visibleFrame.height ?? 900) - 220))
        let controlsWidth: CGFloat = 400
        let panel = RawDevelopPreviewPanel(width: workflowCardWidth - 44 - 20 - controlsWidth, height: height)
        workflowPanel = panel
        let scroll = NSScrollView()
        controlsScroll = scroll
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.widthAnchor.constraint(equalToConstant: controlsWidth).isActive = true
        scroll.heightAnchor.constraint(equalToConstant: height).isActive = true
        controls.translatesAutoresizingMaskIntoConstraints = false
        controls.widthAnchor.constraint(equalToConstant: controlsWidth - 18).isActive = true
        let document = RawDevelopControlsDocument(frame: NSRect(x: 0, y: 0,
            width: controlsWidth - 18, height: controls.fittingSize.height))
        document.addSubview(controls)
        NSLayoutConstraint.activate([
            controls.leadingAnchor.constraint(equalTo: document.leadingAnchor),
            controls.trailingAnchor.constraint(equalTo: document.trailingAnchor),
            controls.topAnchor.constraint(equalTo: document.topAnchor),
            controls.bottomAnchor.constraint(equalTo: document.bottomAnchor)
        ])
        scroll.documentView = document
        let content = NSStackView(views: [panel, scroll])
        content.orientation = .horizontal
        content.alignment = .top
        content.spacing = 20
        return content
    }

    /// Resizes the controls' scroll document to its controls. `workflowContent`
    /// frame-sizes that document ONCE, from the controls' fitting height at
    /// build, so every later change of a row's `isHidden` (the output note, the
    /// Global/Local section switch) comes through here: the document's one
    /// controls subview (the sidebar in a local session, the global stack
    /// otherwise) is laid out and the document takes its fitting height at the
    /// document's own width. The scroll position is kept; nothing happens when
    /// there is no scroll view (the legacy card, whose stack sizes itself).
    func refitControlsDocument() {
        guard let scroll = controlsScroll, let document = scroll.documentView,
              let controls = document.subviews.first else { return }
        controls.layoutSubtreeIfNeeded()
        document.setFrameSize(NSSize(width: document.bounds.width,
                                     height: controls.fittingSize.height))
        scroll.reflectScrolledClipView(scroll.contentView)
    }

    func configureWorkflowSession() {
        if configureLocalSession() { return }
        guard let descriptor, let snapshot, let panel = workflowPanel else { return }
        do {
            let session = try RawDevelopPreviewSession(probe: probe, descriptor: descriptor,
                snapshot: snapshot, beforeSource: beforeSource)
            workflowSession = session
            session.onFrame = { [weak panel] frame in panel?.present(frame) }
            session.onStatus = { [weak self, weak panel] status, ready in
                panel?.setStatus(status, ready: ready)
                self?.previewIsReady = ready
                self?.applyButton?.isEnabled = ready
            }
            panel.onChange = { [weak self] in self?.requestWorkflowPreview() }
        } catch {
            panel.setStatus(error.localizedDescription, ready: false)
            applyButton?.isEnabled = false
        }
    }

    /// The patch the card returns. It names an encoding only when the user
    /// moved the card off the one it opened on: a place into a 32-bit
    /// float document that kept the default records none, so a replay follows
    /// its own document's depth. At the float encoding the card opened on,
    /// grain is returned as at 16 bits (#680: it develops at float on the
    /// display encoding), and the tone curve is returned
    /// only when the user moved it from the value the card opened on (0 when
    /// the card started it at 0, else the saved curve): the recipe
    /// starts an unnamed curve at 0 for a float development, and a replay in
    /// an 8- or 16-bit document develops the file's own tone curve.
    func pendingSettings() -> RawDevelopSettings {
        var settings = developSettings()
        settings.outputEncoding = encoding == initialEncoding ? nil : encoding
        if settings.outputEncoding == nil, encoding == .linearFloat {
            let opening = descriptor?.recipe.outputEncoding == .linearFloat ? initialValues.toneCurve : 0
            settings.toneCurve = values.toneCurve != opening ? values.toneCurve : nil
        }
        return settings
    }

    /// What the card's previews develop: `pendingSettings` with the encoding
    /// named whenever it differs from the recipe's own, which a place that
    /// opened on the float development does without the user touching the row.
    /// While the float pin is in force the tone curve is always named, 0
    /// included: the recipe starts an unnamed curve at 0 when it enters the
    /// float encoding, so a slider moved back to the saved or file-default
    /// value must still develop that value, never the diff against
    /// `initialValues`.
    func developSettings() -> RawDevelopSettings {
        var settings = values.settings(against: initialValues)
        if encoding == .linearFloat, toneCurveBeforeFloat != nil { settings.toneCurve = values.toneCurve }
        if outputSpace != (descriptor?.recipe.outputColorSpace ?? .sRGB) { settings.outputSpace = outputSpace }
        if encoding != (descriptor?.recipe.outputEncoding ?? .normalized16) {
            settings.outputEncoding = encoding
        }
        if let enhancements = enhancementControls?.settings {
            settings.baselineCompensation = enhancements.baselineCompensation
            settings.moire = enhancements.moire
            settings.vignette = enhancements.vignette
            settings.grain = enhancements.grain
            settings.texture = enhancements.texture
            settings.clarity = enhancements.clarity
            settings.dehaze = enhancements.dehaze
            settings.sourceGeometry = enhancements.sourceGeometry
        }
        return settings
    }

    func requestWorkflowPreview() {
        if requestLocalPreview() { return }
        guard let descriptor, let panel = workflowPanel, let session = workflowSession else { return }
        do {
            var requested = descriptor
            requested.recipe = try descriptor.recipe.updating(developSettings())
            let whiteBalanceChanged = requested.recipe.values.customWhiteBalance
                && (requested.recipe.values.temperature != descriptor.recipe.values.temperature
                    || requested.recipe.values.tint != descriptor.recipe.values.tint)
            session.request(descriptor: requested, whiteBalanceChanged: whiteBalanceChanged,
                viewport: panel.currentViewport())
        } catch {
            panel.setStatus(error.localizedDescription, ready: false)
            previewIsReady = false
            applyButton?.isEnabled = false
        }
    }

    func presetControls() -> NSView {
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        popup.target = self
        popup.action = #selector(presetSelected(_:))
        popup.setAccessibilityIdentifier("raw-develop-preset")
        popup.widthAnchor.constraint(equalToConstant: 250).isActive = true
        presetPopup = popup
        let save = NSButton(title: "Save Preset…", target: self, action: #selector(savePreset(_:)))
        save.bezelStyle = .rounded
        save.setAccessibilityIdentifier("raw-develop-save-preset")
        let row = NSStackView(views: [popup, save])
        row.orientation = .horizontal
        row.spacing = 8
        reloadPresets()
        return row
    }

    private func reloadPresets() {
        do {
            presets = try RawPresetLibrary.entries()
            presetPopup?.removeAllItems()
            presetPopup?.addItem(withTitle: "Apply saved preset…")
            presetPopup?.addItems(withTitles: presets.map(\.name))
        } catch {
            presets = []
            presetPopup?.removeAllItems()
            presetPopup?.addItem(withTitle: "Presets unavailable")
            presetPopup?.toolTip = error.localizedDescription
        }
    }

    @objc private func presetSelected(_ sender: NSPopUpButton) {
        let index = sender.indexOfSelectedItem - 1
        guard presets.indices.contains(index) else { return }
        do {
            try applySettings(presets[index].settings)
        } catch {
            let alert = NSAlert(error: error)
            alert.runModal()
        }
        sender.selectItem(at: 0)
    }

    func applySettings(_ settings: RawDevelopSettings) throws {
        guard var requested = descriptor else { return }
        requested.recipe = try requested.recipe.updating(developSettings())
        let patched = try requested.recipe.updating(settings)
        guard let output = patched.outputColorSpace else {
            throw DoctoredCoreError(message: "The preset output color space is unavailable.")
        }
        values = patched.values
        outputSpace = output
        // A preset never carries the encoding. Its tone curve applies at
        // float; with the float pin in force it stays named, and it is
        // what going back to the 16-bit master restores. A preset that names
        // no tone curve keeps the curve the pin saved.
        if toneCurveBeforeFloat != nil, settings.toneCurve != nil {
            toneCurveBeforeFloat = values.toneCurve
        }
        enhancementControls?.apply(recipe: patched)
        for (index, knob) in knobs.enumerated() { knobRows[index].setValue(knob.read(values)) }
        lensRow?.setOn(values.lensCorrection)
        highlightRow?.setOn(values.highlightRecovery)
        whiteBalanceRow?.setSelected(values.customWhiteBalance ? 1 : 0)
        outputRow?.setSelected(RawOutputSpace.allCases.firstIndex(of: outputSpace) ?? 0)
        refreshOutputNote()
        refreshEncodingControls()
        requestPreview()
    }

    @objc private func savePreset(_ sender: Any?) {
        do {
            let recipe = try descriptor?.recipe.updating(developSettings())
            if RawPresetSaveSheet.run(values: values, capabilities: probe.capabilities,
                outputSpace: outputSpace, recipe: recipe) { reloadPresets() }
        } catch { workflowPanel?.setStatus(error.localizedDescription, ready: false); NSSound.beep() }
    }
}
