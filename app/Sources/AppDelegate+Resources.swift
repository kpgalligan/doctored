import AppKit

extension AppDelegate {
    /// Help > Memory Usage; mirrored by MCP resource_usage.
    @objc func showResourceUsage(_ sender: Any?) {
        do {
            var report = try RasterResources.shared.report(
                scratch: (DocumentJobs.active?.scratch ?? 0) + (DocumentJobs.active?.control.scratchBytes ?? 0)
                    + (DocumentJobs.active?.renderScratch.bytes ?? 0) + BackgroundWork.scratchEstimate)
            report["editing_state_retention"] = EditingResourceReport.collect()
            let bytes = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
            let text = NSTextView(frame: NSRect(x: 0, y: 0, width: 560, height: 360))
            text.isEditable = false
            text.font = DS.mono(11)
            text.string = String(decoding: bytes, as: UTF8.self)
            let scroll = NSScrollView(frame: text.frame)
            scroll.hasVerticalScroller = true
            scroll.documentView = text
            let alert = NSAlert()
            alert.messageText = "Memory Usage"
            let limit = WorkingLimit.current
            let limitText = ByteCountFormatter.string(
                fromByteCount: Int64(clamping: limit.effectiveBytes), countStyle: .memory)
            alert.informativeText = "Shared image buffers are counted once across documents, undo and jobs. "
                + "Platform memory and temporary estimates are listed separately. "
                + "Working limit: \(limitText) (\(limit.source.label))."
            alert.accessoryView = scroll
            alert.addButton(withTitle: "Done")
            alert.addButton(withTitle: "Clear Caches")
            if alert.runModal() == .alertSecondButtonReturn { _ = try RasterResources.shared.report(evict: true) }
        } catch { NSApp.presentError(error) }
    }

    /// Doctored ▸ Working Memory…; MCP reads the same policy, read-only,
    /// through resource_usage's `working_limit` (an app-wide preference).
    @objc func showWorkingMemory(_ sender: Any?) {
        WorkingLimitSheetController.run()
    }
}
