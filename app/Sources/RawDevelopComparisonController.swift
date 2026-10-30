import AppKit

// The shared comparison panel performs no second decode.

/// Migration reviews the exact prepared source images that its caller will
/// publish. Changing detail, pan or clipping only inspects these same owners.
final class RawDevelopComparisonController: NSObject {
    static func run(before: RasterPrecisionImage, after: RasterPrecisionImage,
                    title: String, hint: String, applyTitle: String = "Migrate Decoder") -> Bool {
        let controller = RawDevelopComparisonController()
        return controller.run(before: before, after: after, title: title, hint: hint, applyTitle: applyTitle)
    }

    private var modal: ModalCardWindowController?
    private var ready = false
    private func run(before: RasterPrecisionImage, after: RasterPrecisionImage,
                     title: String, hint: String, applyTitle: String) -> Bool {
        do {
            let session = try RawDevelopPreviewSession(before: before, after: after)
            let available = NSScreen.main?.visibleFrame.height ?? 900
            let height = min(600, max(400, available - 260))
            let panel = RawDevelopPreviewPanel(width: 600, height: height)
            let cancel = NSButton(title: "Keep Saved Rendering", target: self, action: #selector(cancel(_:)))
            cancel.bezelStyle = .rounded
            let apply = StickerButton(title: applyTitle, style: .primary, target: self, action: #selector(apply(_:)))
            apply.isEnabled = false
            apply.setAccessibilityIdentifier("raw-migration-apply")
            cancel.setAccessibilityIdentifier("raw-migration-keep")
            session.onFrame = { [weak panel] frame in panel?.present(frame) }
            session.onStatus = { [weak self, weak panel, weak apply] status, ready in
                self?.ready = ready
                panel?.setStatus(status, ready: ready)
                apply?.isEnabled = ready
            }
            panel.onChange = { [weak session, weak panel] in
                guard let panel else { return }
                session?.request(viewport: panel.currentViewport())
            }
            let explanation = NSTextField(wrappingLabelWithString: hint)
            explanation.font = DS.sans(12)
            explanation.textColor = DS.textMuted
            explanation.preferredMaxLayoutWidth = 600
            explanation.widthAnchor.constraint(equalToConstant: 600).isActive = true
            let body = NSStackView(views: [explanation, panel])
            body.orientation = .vertical
            body.alignment = .leading
            body.spacing = 12
            body.widthAnchor.constraint(equalToConstant: 600).isActive = true
            body.setFrameSize(NSSize(width: 600, height: body.fittingSize.height))
            let scroll = NSScrollView()
            scroll.hasVerticalScroller = true
            scroll.autohidesScrollers = true
            scroll.drawsBackground = false
            scroll.documentView = body
            scroll.widthAnchor.constraint(equalToConstant: 618).isActive = true
            scroll.heightAnchor.constraint(equalToConstant: min(body.fittingSize.height, max(260, available - 160))).isActive = true
            let card = makeSheetView(title: title, content: scroll,
                buttonRow: makeButtonRow(cancel: cancel, apply: apply), width: 662)
            let modal = ModalCardWindowController(title: title, card: card)
            self.modal = modal
            session.request(viewport: panel.currentViewport())
            let response = modal.runModal()
            session.cancel()
            panel.clear()
            self.modal = nil
            return response == .OK && ready
        } catch {
            NSAlert(error: error).runModal()
            return false
        }
    }
    @objc private func apply(_ sender: Any?) { if ready { modal?.end(.OK) } }
    @objc private func cancel(_ sender: Any?) { modal?.end(.cancel) }
}
