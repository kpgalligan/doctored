import AppKit

/// Layer ▸ Generative Variations…: "Variation k of N" with Previous and Next, each
/// a select job without progress while the sheet stays open; the prompt (prefilled from the
/// selected variation's tag) and the provider for a new set; Generate closes the sheet, runs
/// `GenerativeFillRun.gate` and `GenerativeVariationsRun.startMore` with progress; Done closes
/// it. MCP twins `generative_variations`, `select_generative_variation` and
/// `generate_variations`.
final class GenerativeVariationsSheetController: NSViewController {
    private let status = NSTextField(labelWithString: "")
    private let promptField: NSTextField
    private let providerPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private var previousButton: NSButton!
    private var nextButton: NSButton!
    private let provider: CloudProviderID?
    private let current: () -> GenerativeVariationsInfo?
    private let onSelect: (Int) -> DocumentJob?
    private let onGenerate: (GenerativeFillSettings) -> Void

    init(prompt: String, provider: CloudProviderID?, current: @escaping () -> GenerativeVariationsInfo?,
         onSelect: @escaping (Int) -> DocumentJob?,
         onGenerate: @escaping (GenerativeFillSettings) -> Void) {
        promptField = NSTextField(string: prompt)
        self.provider = provider
        self.current = current
        self.onSelect = onSelect
        self.onGenerate = onGenerate
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("GenerativeVariationsSheetController does not support NSCoder")
    }

    override func loadView() {
        status.font = DS.sans(13)
        status.textColor = DS.textStrong
        status.setAccessibilityIdentifier("generative-variations-status")
        previousButton = StickerButton(title: "Previous", style: .secondary, target: self,
                                       action: #selector(previousClicked(_:)))
        previousButton.setAccessibilityIdentifier("generative-variations-previous")
        nextButton = StickerButton(title: "Next", style: .secondary, target: self,
                                   action: #selector(nextClicked(_:)))
        nextButton.setAccessibilityIdentifier("generative-variations-next")
        let stepper = NSStackView(views: [previousButton, status, nextButton])
        stepper.spacing = 10
        promptField.widthAnchor.constraint(equalToConstant: 260).isActive = true
        promptField.placeholderString = "Describe what to add, or leave empty"
        DSField.style(promptField)
        promptField.setAccessibilityIdentifier("generative-variations-prompt")
        for item in CloudProviders.all { providerPopup.addItem(withTitle: item.displayName) }
        if let provider, let at = CloudProviders.all.firstIndex(where: { $0.id == provider }) {
            providerPopup.selectItem(at: at)
        }
        providerPopup.setAccessibilityIdentifier("generative-variations-provider")
        let grid = NSGridView(views: [[fieldLabel("Variation:"), stepper],
                                      [fieldLabel("Prompt:"), promptField],
                                      [fieldLabel("Provider:"), providerPopup]])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let done = StickerButton(title: "Done", style: .secondary, target: self,
                                 action: #selector(doneClicked(_:)))
        let generate = StickerButton(title: "Generate", style: .primary, target: self,
                                     action: #selector(generateClicked(_:)))
        generate.setAccessibilityIdentifier("generative-variations-generate")
        view = makeSheetView(
            title: "Generative Variations",
            hint: "Switch the Generative Layer between the pictures its fills kept, or generate "
                + "three more from the same area with your own API key, billed to you.",
            content: grid,
            buttonRow: makeButtonRow(cancel: done, apply: generate))
        refresh()
    }

    /// The status line and the arrows from the layer's current variations.
    func refresh() {
        guard let info = current() else {
            status.stringValue = "No variations"
            previousButton.isEnabled = false
            nextButton.isEnabled = false
            return
        }
        status.stringValue = "Variation \(info.selected + 1) of \(info.count)"
        previousButton.isEnabled = info.selected > 0
        nextButton.isEnabled = info.selected < info.count - 1
    }

    @objc private func previousClicked(_ sender: Any?) { step(-1) }

    @objc private func nextClicked(_ sender: Any?) { step(1) }

    private func step(_ delta: Int) {
        guard let info = current(), (0..<info.count).contains(info.selected + delta),
              let job = onSelect(info.selected + delta) else { return }
        let prior = job.completion
        job.completion = { [weak self] finished in
            prior?(finished)
            self?.refresh()
        }
    }

    @objc private func generateClicked(_ sender: Any?) {
        var settings = GenerativeFillSettings()
        settings.prompt = String(promptField.stringValue.prefix(GenerativeFillSettings.maximumPromptLength))
        let index = max(0, providerPopup.indexOfSelectedItem)
        settings.provider = CloudProviders.all[min(index, CloudProviders.all.count - 1)].id
        dismiss(self)
        onGenerate(settings)
    }

    @objc private func doneClicked(_ sender: Any?) { dismiss(self) }
}

extension EditorViewController {
    /// Layer ▸ Generative Variations…: the sheet over the active Generative Layer.
    @objc func showGenerativeVariations(_ sender: Any?) {
        guard let document, validateGenerativeVariationsItem() else {
            NSSound.beep()
            return
        }
        let layer = document.activeLayerIndex
        let doc = document.doc
        presentAsSheet(GenerativeVariationsSheetController(
            prompt: GenerativeVariationsRun.prompt(doc, layer: layer),
            provider: GenerativeVariationsRun.provider(doc, layer: layer),
            current: { [weak self] in GenerativeVariationsRun.info(self?.document?.doc, layer: layer) },
            onSelect: { [weak self] index in self?.selectGenerativeVariation(layer: layer, index: index) },
            onGenerate: { [weak self] settings in self?.runGenerativeVariations(settings, layer: layer) }))
    }

    private func selectGenerativeVariation(layer: Int, index: Int) -> DocumentJob? {
        guard let document else { return nil }
        do {
            return try GenerativeVariationsRun.select(document: document, layer: layer, index: index,
                                                      showProgress: false)
        } catch {
            NSSound.beep()
            return nil
        }
    }

    private func runGenerativeVariations(_ settings: GenerativeFillSettings, layer: Int) {
        guard let document else { NSSound.beep(); return }
        let title = "Generative Variations did not run"
        do {
            try GenerativeFillRun.gate(CloudProviders.provider(settings.provider), agent: false)
            let job = try GenerativeVariationsRun.startMore(settings, document: document, layer: layer,
                                                            showProgress: true)
            let prior = job.completion
            job.completion = { [weak self] completed in
                prior?(completed)
                if completed.state == "completed" { self?.presentGenerativeShortfall(completed) }
                if ["failed", "stale"].contains(completed.state), let message = completed.message {
                    self?.presentGenerativeFillFailure(message, title: title)
                }
            }
        } catch is JobCancelled {
        } catch {
            presentGenerativeFillFailure(error.localizedDescription, title: title)
        }
    }

    /// The active layer holds live variations, on a layer plane, without Quick Mask or a
    /// transform, at 8 and 16 bits and on 8-bit CMYK.
    func validateGenerativeVariationsItem() -> Bool {
        guard let document, let doc = document.doc, !document.quickMaskActive,
              document.planeEditTarget == .layer, !isTransforming,
              GenerativeFillRun.refusal(doc) == nil else { return false }
        return GenerativeVariationsRun.info(doc, layer: document.activeLayerIndex) != nil
    }
}
