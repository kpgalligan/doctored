import AppKit

/// Edit ▸ Generative Fill…: the prompt (empty fills from the surrounding picture) and the
/// provider, then Generate. Generate closes the sheet and runs `GenerativeFillRun.gate` (the
/// consent card on first use; Cancel there sends nothing) and `GenerativeFillRun.start` on the
/// active layer with the selection's coverage. MCP twin `generative_fill`. Edit ▸ Generative
/// Expand… is the same sheet for `GenerativeFeature.expand`: no selection, the area
/// the picture leaves transparent is filled (`GenerativeCoverage.transparency`). MCP twin
/// `generative_expand`.
final class GenerativeFillSheetController: NSViewController {
    private let promptField = NSTextField(string: "")
    private let providerPopup = NSPopUpButton(frame: .zero, pullsDown: false)
    private let feature: GenerativeFeature
    private let onGenerate: (GenerativeFillSettings) -> Void

    init(feature: GenerativeFeature = .fill, onGenerate: @escaping (GenerativeFillSettings) -> Void) {
        self.feature = feature
        self.onGenerate = onGenerate
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("GenerativeFillSheetController does not support NSCoder")
    }

    override func loadView() {
        promptField.widthAnchor.constraint(equalToConstant: 260).isActive = true
        promptField.placeholderString = "Describe what to add, or leave empty"
        DSField.style(promptField)
        promptField.setAccessibilityIdentifier("generative-fill-prompt")
        for provider in CloudProviders.all { providerPopup.addItem(withTitle: provider.displayName) }
        providerPopup.setAccessibilityIdentifier("generative-fill-provider")
        let grid = NSGridView(views: [[fieldLabel("Prompt:"), promptField],
                                      [fieldLabel("Provider:"), providerPopup]])
        grid.rowSpacing = 10
        grid.columnSpacing = 12
        grid.column(at: 0).xPlacement = .trailing
        grid.column(at: 0).width = 106
        let cancel = sheetCancelButton(target: self, action: #selector(cancelClicked(_:)))
        let generate = StickerButton(title: "Generate", style: .primary, target: self,
                                     action: #selector(generateClicked(_:)))
        generate.setAccessibilityIdentifier("generative-fill-generate")
        view = makeSheetView(
            title: feature.title,
            hint: hint,
            content: grid,
            buttonRow: makeButtonRow(cancel: cancel, apply: generate))
    }

    private var hint: String {
        switch feature {
        case .fill:
            return "Sends the area around the selection to the provider with your own API key, "
                + "billed to you, and puts the answer on a new layer masked by the selection, "
                + "keeping three variations to switch between in Layer \u{25B8} Generative "
                + "Variations\u{2026}. Pixels outside the selection do not change."
        case .expand:
            return "Sends the picture with its transparent area (what Canvas Size or a crop past "
                + "the canvas added) to the "
                + "provider with your own API key, billed to you, and fills that area on a new "
                + "layer masked to it, keeping three variations to switch between in Layer "
                + "\u{25B8} Generative Variations\u{2026}. Pixels the picture already covers do "
                + "not change."
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

    @objc private func cancelClicked(_ sender: Any?) { dismiss(self) }
}

extension EditorViewController {
    /// Edit ▸ Generative Fill…: the sheet, then the gate and the job on the active layer.
    @objc func showGenerativeFill(_ sender: Any?) {
        guard let document, validateGenerativeFillItem(), document.canvasSelection != nil else {
            NSSound.beep()
            return
        }
        presentAsSheet(GenerativeFillSheetController { [weak self] settings in
            self?.runGenerativeFill(settings)
        })
    }

    private func runGenerativeFill(_ settings: GenerativeFillSettings) {
        guard let document, let selection = document.canvasSelection else { NSSound.beep(); return }
        let layer = document.activeLayerIndex
        do {
            try GenerativeFillRun.gate(CloudProviders.provider(settings.provider), agent: false)
            let job = try GenerativeFillRun.start(
                settings, coverage: .selection(selection.maskSamples16()), document: document,
                layer: layer, showProgress: true)
            let prior = job.completion
            job.completion = { [weak self] completed in
                prior?(completed)
                if completed.state == "completed" { self?.presentGenerativeShortfall(completed) }
                if ["failed", "stale"].contains(completed.state), let message = completed.message {
                    self?.presentGenerativeFillFailure(message)
                }
            }
        } catch is JobCancelled {
        } catch {
            presentGenerativeFillFailure(error.localizedDescription)
        }
    }

    func presentGenerativeFillFailure(_ message: String, title: String = "Generative Fill did not run") {
        let alert = NSAlert()
        alert.messageText = title
        // The core's sentences are lowercase, written to follow a lead-in.
        alert.informativeText = message.prefix(1).uppercased() + message.dropFirst()
        if let window = view.window {
            alert.beginSheetModal(for: window)
        } else {
            alert.runModal()
        }
    }

    /// A completed generative job that got fewer variations than it asked for says so, with the
    /// provider's reason (`GenerativeFillRun.describeCount`'s `short_reason`); nothing otherwise.
    func presentGenerativeShortfall(_ job: DocumentJob) {
        guard let reason = job.details["short_reason"] as? String else { return }
        let received = job.details["variations_received"] as? Int ?? 0
        let requested = job.details["variations_requested"] as? Int ?? 0
        presentGenerativeFillFailure(reason,
                                     title: "\(received) of \(requested) variations came back")
    }

    /// Edit ▸ Generative Expand…: the sheet, then the gate and the job on the active layer over
    /// the area the picture leaves transparent. No selection is needed.
    @objc func showGenerativeExpand(_ sender: Any?) {
        guard document != nil, validateGenerativeExpandItem() else {
            NSSound.beep()
            return
        }
        presentAsSheet(GenerativeFillSheetController(feature: .expand) { [weak self] settings in
            self?.runGenerativeExpand(settings)
        })
    }

    private func runGenerativeExpand(_ settings: GenerativeFillSettings) {
        guard let document else { NSSound.beep(); return }
        let layer = document.activeLayerIndex
        let title = "Generative Expand did not run"
        do {
            try GenerativeFillRun.gate(CloudProviders.provider(settings.provider), agent: false)
            let job = try GenerativeFillRun.start(
                settings, coverage: .transparency, document: document, layer: layer,
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

    /// A document on a layer plane, no Quick Mask, transform or text session, at 8 and 16 bits
    /// and on 8-bit CMYK. No selection is needed.
    func validateGenerativeExpandItem() -> Bool {
        guard let document, let doc = document.doc, !document.quickMaskActive,
              document.planeEditTarget == .layer, !isTransforming,
              !canvas.hasActiveTextSession else { return false }
        return GenerativeFillRun.refusal(doc, feature: .expand) == nil
    }

    /// A selection on a layer plane, no Quick Mask, transform or text session, at 8 and 16 bits.
    func validateGenerativeFillItem() -> Bool {
        guard let document, let doc = document.doc, document.canvasSelection != nil,
              !document.quickMaskActive, document.planeEditTarget == .layer, !isTransforming,
              !canvas.hasActiveTextSession else { return false }
        return GenerativeFillRun.refusal(doc) == nil
    }
}
