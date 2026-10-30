import AppKit
import UniformTypeIdentifiers

extension ImageDocument {
    /// File > Export as PSD…: writes the document as a PSD, a Large
    /// Document PSB or a TIFF with layers, at the DOCUMENT's own depth —
    /// 8-bit from a byte document, 16-bit from a typed one, 32-bit from a
    /// float one (its TIFF with layers too) — carrying either
    /// the editable stack or one flattened composite. The mirror of
    /// File > Export…, with one difference that matters — the preflight for
    /// EVERY container and mode is built before the panel opens, so the
    /// accessory can say what each would lose and disable a mode that cannot
    /// be written.
    ///
    /// The MCP twins are `inspect_psd_export` and `export_psd`
    /// (`AgentServer+PSDExport.swift`).
    @IBAction func exportPSD(_ sender: Any?) {
        guard commitPendingCanvasSessions() else { return }
        guard let source = doc, let window = windowForSheet else {
            NSSound.beep()
            return
        }
        var reports: [LayeredContainer: [PSDExportMode: PSDExportReport]] = [:]
        var captures: [LayeredContainer: [PSDExportMode: PSDExportCapture]] = [:]
        do {
            for container in LayeredContainer.allCases {
                for mode in PSDExportMode.allCases {
                    let report = try PSDExportReport(
                        source.psdExportReport(container: container, mode: mode.coreMode))
                    reports[container, default: [:]][mode] = report
                    if report.available {
                        captures[container, default: [:]][mode] =
                            try PSDExportCapture(self, container: container, mode: mode)
                    }
                }
            }
        } catch {
            presentError(error)
            return
        }
        guard !captures.isEmpty else {
            presentExportNotice(reports[.psd]?[.flattened]?.refusal
                ?? "This document cannot be written as a Photoshop document.")
            return
        }

        let accessory = PSDExportAccessoryController(reports: reports)
        let panel = NSSavePanel()
        panel.canCreateDirectories = true
        panel.allowedContentTypes = [accessory.selectedContainer.utType]
        panel.nameFieldStringValue =
            ((displayName ?? "Image") as NSString).deletingPathExtension + "-export"
        panel.accessoryView = accessory.view
        // The panel's type, and so the extension it appends, follows the
        // Format popup.
        accessory.onContainerChange = { [weak panel] container in
            panel?.allowedContentTypes = [container.utType]
        }

        panel.beginSheetModal(for: window) { [weak self] response in
            guard response == .OK, let self = self, let url = panel.url else { return }
            let container = accessory.selectedContainer
            guard let capture = captures[container]?[accessory.selectedMode] else {
                // The chosen container cannot hold this document at all; say
                // why rather than only beeping.
                self.presentExportNotice(
                    reports[container]?[accessory.selectedMode]?.refusal
                        ?? "This document cannot be written as \(container.title).")
                return
            }
            self.writePSD(capture, to: url)
        }
    }

    /// The half that runs after the panel closes: admit, write the capture
    /// taken before the panel opened, and then say once what the file could
    /// not carry.
    private func writePSD(_ capture: PSDExportCapture, to url: URL) {
        do {
            _ = try RasterResources.shared.admit(scratch: capture.scratchEstimate)
            let task = try WorkerTask(scratchEstimate: capture.scratchEstimate) {
                try capture.write(to: url)
            }
            try BackgroundWork.wait(task, title: "Exporting Photoshop Document")
            if let message = capture.droppedMessage {
                presentExportNotice(message)
            }
        } catch {
            presentError(error)
        }
    }
}
