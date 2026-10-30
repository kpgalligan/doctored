import AppKit
import UniformTypeIdentifiers

extension AppDelegate {
    /// File ▸ Merge to HDR…: bracketed exposures of one scene become
    /// ONE new 32-bit float document. It lives here, not on
    /// `EditorViewController`, for `loadFilesIntoStack`'s reason — it has to
    /// work at the Welcome window, before any document exists. The files are
    /// taken in file-name order; `MergeHDR.prepare` builds the stack and reads
    /// each frame's EV, the card shows them, and `MergeHDR.merge` is the one
    /// cancellable job. MCP twin `merge_to_hdr` (AgentServer+MergeHDR.swift),
    /// which keeps the caller's own order.
    @objc func mergeToHDR(_ sender: Any?) {
        let panel = NSOpenPanel()
        panel.title = MergeHDR.title
        panel.message = "Choose two or more exposures of one scene. They merge into one new "
            + "32-bit document."
        panel.prompt = "Choose"
        panel.allowsMultipleSelection = true
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = ImageDocument.readableTypes.compactMap(UTType.init)
        guard panel.runModal() == .OK else { return }
        mergeToHDR(urls: panel.urls)
    }

    /// The action past its open panel: the files in natural name order, the
    /// card, the merge, and every alert the action shows.
    func mergeToHDR(urls: [URL]) {
        // Natural file-name order ("frame 2" before "frame 10"), as Finder lists them.
        let urls = urls.sorted {
            $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending
        }
        do {
            let prepared = try MergeHDR.prepare(files: urls)
            guard let options = MergeHDRSheetController.run(
                frames: prepared.frames, title: MergeHDR.title, notice: prepared.notice)
            else { return }
            let outcome = try MergeHDR.merge(consume prepared, options: options)
            try ImageDocument.openMergedHDR(outcome, name: nil)
            MergeHDR.report(outcome)
        } catch is JobCancelled {
            return
        } catch let error as CocoaError where error.code == .userCancelled {
            return
        } catch {
            let alert = NSAlert()
            alert.messageText = "Could Not Merge to HDR"
            alert.informativeText = error.localizedDescription
            alert.runModal()
        }
    }
}

extension MergeHDR {
    /// What the user is told after a merge: the frames alignment left out,
    /// then the notice, in one informational alert; nothing when there is
    /// neither.
    static func report(_ outcome: Outcome) {
        var lines = outcome.leftOut.map { left -> String in
            // A core refusal arrives as a full sentence; the host's reasons do not.
            let reason = left.reason.hasSuffix(".") ? String(left.reason.dropLast()) : left.reason
            return "“\(left.name)” was left out: \(reason)."
        }
        if let notice = outcome.notice { lines.append(notice) }
        guard !lines.isEmpty else { return }
        let alert = NSAlert()
        alert.alertStyle = .informational
        alert.messageText = outcome.leftOut.isEmpty ? title
            : outcome.leftOut.count == 1 ? "One Frame Was Left Out"
            : "\(outcome.leftOut.count) Frames Were Left Out"
        alert.informativeText = lines.joined(separator: "\n")
        alert.runModal()
    }
}
