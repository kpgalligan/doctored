import AppKit

func verifyByteCutoutProjectionCancellation() throws {
    let width = 4096, height = 4096, count = width * height
    var foregroundBytes = [UInt8](repeating: 255, count: count * 4)
    for i in 0..<count {
        foregroundBytes[i * 4] = UInt8(truncatingIfNeeded: i)
        foregroundBytes[i * 4 + 1] = 73
        foregroundBytes[i * 4 + 2] = 209
    }
    guard let foreground = RasterImage.from(rgba: foregroundBytes, width: width, height: height) else {
        throw ProbeFailure(description: "Missing byte cutout cancellation foreground")
    }
    foregroundBytes = []
    let alpha = [UInt8](repeating: 127, count: count)
    for closing in [false, true] {
        try autoreleasepool {
            let document = try fixture(width: width, height: height)
            defer { document.close() }
            document.undoManager?.removeAllActions()
            let source = document.doc, history = document.history.entries.count
            let selection = document.selectionHistory
            let session = try RefinementTrimapSession(width: width, height: height)
            let capture = try RefinementCutoutCapture(document: document, session: session)
            let token = try RasterJobControl()
            var progress: Double?, scratch: UInt64 = 0, invoked = false
            let timer = Timer(timeInterval: 0.001, repeats: true) { timer in
                let observed = token.progress
                guard progress == nil, token.stage == 6, observed > 0.35, observed < 0.90,
                      token.scratchBytes > 0,
                      let panel = NSApp.windows.first(where: { $0.isVisible && $0.title == "Preparing Cutout Display" }),
                      let content = panel.contentView else { return }
                if closing {
                    progress = observed; scratch = token.scratchBytes; timer.invalidate()
                    document.close(); invoked = true
                } else if let button = allViews(content).compactMap({ $0 as? NSButton })
                    .first(where: { $0.title == "Cancel" && !$0.isHidden }) {
                    progress = observed; scratch = token.scratchBytes; timer.invalidate()
                    invoked = button.sendAction(button.action, to: button.target)
                }
            }
            RunLoop.main.add(timer, forMode: .common); RunLoop.main.add(timer, forMode: .modalPanel)
            defer { timer.invalidate() }
            var failure: Error?
            do { _ = try capture.prepare(foreground: foreground, alpha: alpha,
                in: document, session: session, control: token) }
            catch { failure = error }
            try require(invoked && progress != nil && scratch > 0 && token.isCancelled
                && (failure as? JobCancelled)?.kernelMessage == "operation cancelled", "Byte render Cancel must reach an active core checkpoint: progress=\(progress ?? -1), scratch=\(scratch), invoked=\(invoked), cancelled=\(token.isCancelled), error=\(String(describing: failure))")
            try require(document.doc === source && document.history.entries.count == history
                && document.selectionHistory == selection && !document.isDocumentEdited
                && document.undoManager?.canUndo != true, "Byte cutout cancellation is atomic")
            try require(!BackgroundWork.occupied && !BackgroundWork.isWaiting
                && BackgroundWork.scratchEstimate == 0 && token.scratchBytes == 0,
                "Byte projection drains modal/worker/scratch ownership")
            print("PASS: \(closing ? "owner-close" : "Cancel action") during shared byte cutout projection: stage=6, progress=\(progress ?? -1), scratch=\(scratch)→0, core error=\((failure as? JobCancelled)?.kernelMessage ?? "none"); no edit; worker drained")
        }
    }
}
