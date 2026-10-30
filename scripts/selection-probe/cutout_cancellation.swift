import AppKit

/// Runs the real captured host assembly/render/quantization pipeline. Cancel is
/// invoked through its button action while the core reports a live interior stage, never by a timed sleep
/// or a substituted renderer. The two disjoint parent ranges identify the
/// authoritative native render and its explicit RGBA8 derivative respectively.
func verifyCutoutPreparationCancellation() throws {
    let width = 4096, height = 4096, count = width * height
    var samples = [UInt16](repeating: 0, count: count * 4)
    for i in 0..<count {
        samples[i * 4] = UInt16(i % 65536)
        samples[i * 4 + 1] = 22003
        samples[i * 4 + 2] = 44007
        samples[i * 4 + 3] = 65535
    }
    let source = try RasterDocument.fromRawRGBA16(samples, width: width, height: height,
                                                 profile: RasterProfile.builtin(.sRGB))
    samples = []
    guard let foreground = source.precision?.layerSource(0) else {
        throw ProbeFailure(description: "Missing native cancellation foreground")
    }
    let matte = try PrecisionScalarBuffer(samples: [UInt16](repeating: 32769, count: count),
                                           width: width, height: height)
    for closing in [false, true] {
        for preview in [false, true] {
            try autoreleasepool {
                let document = try fixture(width: 8, height: 8)
                defer { document.close() }
                withUndoGroup(document.undoManager) {
                    document.applyEdit("Native cancellation fixture", record: .notACommand,
                                       intentionalDepthConversion: true) { _ in source }
                }
                document.setCanvasSelection(CanvasSelection(shape: .rect(CGRect(x: 1, y: 1, width: 3, height: 2)),
                    canvasWidth: width, canvasHeight: height))
                document.updateChangeCount(.changeCleared)
                document.undoManager?.removeAllActions()
                let history = document.history.entries.count
                let selection = document.selectionHistory
                let session = try RefinementTrimapSession(width: width, height: height)
                let capture = try RefinementCutoutCapture(document: document, session: session)
                let token = try RasterJobControl()
                var observedProgress: Double?
                var observedScratch: UInt64 = 0
                var observedStage: UInt32 = 0
                var clicked = false
                let lower = preview ? 0.89 : 0.30
                let upper = preview ? 0.98 : 0.85
                let timer = Timer(timeInterval: 0.001, repeats: true) { timer in
                    let progress = token.progress
                    guard observedProgress == nil, token.stage == 6, token.scratchBytes > 0,
                          progress > lower, progress < upper else { return }
                    guard let panel = NSApp.windows.first(where: { $0.title == "Preparing Cutout Display" && $0.isVisible }),
                          let content = panel.contentView else { return }
                    if closing {
                        observedProgress = progress; observedScratch = token.scratchBytes
                        observedStage = token.stage; timer.invalidate()
                        document.close()
                    } else if let button = allViews(content).compactMap({ $0 as? NSButton })
                        .first(where: { $0.title == "Cancel" && !$0.isHidden }) {
                        observedProgress = progress; observedScratch = token.scratchBytes
                        observedStage = token.stage; timer.invalidate()
                        clicked = button.sendAction(button.action, to: button.target)
                    }
                }
                RunLoop.main.add(timer, forMode: .common)
                RunLoop.main.add(timer, forMode: .modalPanel)
                defer { timer.invalidate() }
                var failure: Error?
                do {
                    _ = try capture.prepare(foreground: foreground, matte: matte,
                        in: document, session: session, control: token)
                } catch { failure = error }
                try require(observedProgress != nil && observedStage == 6 && observedScratch > 0,
                    "Cancellation must observe live interior native \(preview ? "preview" : "render") work")
                try require((failure as? JobCancelled)?.kernelMessage == "operation cancelled"
                    && token.isCancelled && (closing || clicked),
                    "Actual \(closing ? "owner close" : "Cancel action") must reach a running core checkpoint, not only discard late output; error=\(String(describing: failure))")
                try require(document.doc === source && document.selectionHistory == selection
                    && document.history.entries.count == history && !document.isDocumentEdited
                    && document.undoManager?.canUndo != true,
                    "Cancelled preparation preserves pixels, selection, history and clean identity")
                try require(!BackgroundWork.occupied && BackgroundWork.scratchEstimate == 0
                    && token.scratchBytes == 0 && !BackgroundWork.isWaiting,
                    "Cancelled/closed work drains its mailbox, modal session and all scratch")
                print("PASS: \(closing ? "owner-close" : "Cancel action") during native \(preview ? "preview" : "render"): stage=\(observedStage), progress=\(observedProgress ?? -1), scratch=\(observedScratch)→0; core error=\((failure as? JobCancelled)?.kernelMessage ?? "none"); no edit; worker drained")
            }
        }
    }
}
