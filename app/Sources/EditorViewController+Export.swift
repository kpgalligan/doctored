import AppKit

/// File ▸ Export… and File ▸ Quick Export. Both are pure deliveries: they
/// read the captured immutable document and its warm projection and write a
/// file, and neither touches undo, the dirty flag, the history or the
/// archive.
extension EditorViewController {
    /// File ▸ Export… — the export workspace sheet.
    @IBAction func showExportWorkspace(_ sender: Any?) {
        guard let document else {
            NSSound.beep()
            return
        }
        guard document.commitPendingCanvasSessions() else { return }
        do { presentAsSheet(try ExportWorkspaceSheetController(document: document)) }
        catch { NSApp.presentError(error) }
    }

    /// File ▸ Quick Export — the default preset, no dialog, and a notice
    /// naming the path it wrote. With no default set it uses the built-in
    /// preset: a PNG named after the document, beside the document's file
    /// or in ~/Downloads (`QuickExportDefaults.swift`). A name that is
    /// already a file is never overwritten; the export lands beside it as
    /// `-2`, `-3`, ….
    @IBAction func quickExport(_ sender: Any?) {
        guard let document else {
            NSSound.beep()
            return
        }
        guard document.commitPendingCanvasSessions() else { return }
        do {
            let preset = try ExportPresetLibrary.quickExportPreset(for: document)
            let outcome = try document.quickExport(preset: preset, collision: .suffix)
            var notice = "Wrote \(outcome.url.path)"
            if let message = ExportCapabilities.droppedMessage(
                outcome.source,
                report: try outcome.result.saveReport(), format: preset.settings.format,
                embedProfile: preset.settings.embedProfile,
                stripMetadata: preset.settings.stripMetadata,
                metadataNotCaptured: document.metadataNotCaptured,
                metadataImportReport: document.metadataImportReport) {
                notice += "\n\n" + message
            }
            document.presentExportNotice(notice)
        } catch {
            NSApp.presentError(error)
        }
    }

    /// File ▸ Export Layers… — the per-item delivery sheet (Phase 11B).
    @IBAction func exportLayers(_ sender: Any?) {
        guard let document else {
            NSSound.beep()
            return
        }
        guard document.commitPendingCanvasSessions() else { return }
        do { presentAsSheet(try LayerExportSheetController(document: document)) }
        catch { NSApp.presentError(error) }
    }

    /// File ▸ Export as PDF… — one flattened page at the document's own
    /// print size (Phase 11D). The save panel asks where, and its accessory
    /// the page's depth and compression; every other setting is
    /// the document's current export preference, exactly as Quick Export's
    /// is, and the notice names what the file holds and what did not travel.
    @IBAction func exportAsPDF(_ sender: Any?) {
        guard let document, document.doc != nil else {
            NSSound.beep()
            return
        }
        guard document.commitPendingCanvasSessions() else { return }
        do {
            let capture = try ExportCapture(document)
            let accessory = PDFExportAccessoryController(
                defaults: PDFExportOptions.defaults(for: document),
                offersDepth: document.doc?.precision != nil)
            let base = document.displayName ?? "Untitled"
            let panel = NSSavePanel()
            panel.allowedContentTypes = [.pdf]
            panel.nameFieldStringValue = (base as NSString).deletingPathExtension + ".pdf"
            panel.canCreateDirectories = true
            panel.accessoryView = accessory.view
            let response = document.windowForSheet.map { window -> NSApplication.ModalResponse in
                var result: NSApplication.ModalResponse = .cancel
                panel.beginSheetModal(for: window) { result = $0; NSApp.stopModal() }
                NSApp.runModal(for: panel)
                return result
            } ?? panel.runModal()
            guard response == .OK, let url = panel.url else { return }
            let options = accessory.options
            try options.validate()
            var settings = ExportSettings.defaults(for: document)
            // A 16-bit page reduces nothing, so there is nothing to dither.
            if options.bitDepth == 16 { settings.dither = "none" }
            let (data, report) = try PDFExport.run(
                capture, settings: settings, options: options, title: base)
            // The panel already asked about an existing file.
            try PDFExport.write(data, to: url, overwrite: true)
            var notice = "Wrote \(url.path)"
            notice += String(
                format: "\n\nOne page of %.2f × %.2f pt (%d × %d pixels at %g × %g ppi), "
                    + "carrying the %@ profile.",
                report.pageSizePoints.width, report.pageSizePoints.height,
                report.width, report.height, report.ppi.x, report.ppi.y, report.profileName)
            let written: String
            switch report.compression {
            case "zip": written = "ZIP"
            case "jpeg": written = "JPEG"
            default: written = report.compression
            }
            notice += "\n\nA PDF export writes the flattened picture as one "
                + "\(report.bitsPerComponent)-bit \(written) image and carries no capture packets."
            if report.requestedCompression == .jpeg, report.compression != "jpeg" {
                notice += " JPEG was requested; the system wrote \(report.compression.uppercased())."
            }
            document.presentExportNotice(notice)
        } catch {
            NSApp.presentError(error)
        }
    }

    /// File ▸ Contact Sheet… — the grid sheet (Phase 11B).
    @IBAction func showContactSheet(_ sender: Any?) {
        guard let document else {
            NSSound.beep()
            return
        }
        guard document.commitPendingCanvasSessions() else { return }
        do { presentAsSheet(try ContactSheetSheetController(document: document)) }
        catch { NSApp.presentError(error) }
    }
}
