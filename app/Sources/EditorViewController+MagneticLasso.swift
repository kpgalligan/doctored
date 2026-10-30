import AppKit

// The commit is ONE "Magnetic Lasso" step recorded as the resolved `select_freehand`
// (`select_magnetic` is `nonRecordable`); a stale capture (an agent edit) cancels the outline with
// a beep (`magneticLassoDocumentDidChange`); `MagneticLassoSettings` persists
// Width/Contrast/Frequency; at 32-bit float the capture traces R1 at 8 bits
// (`SelectionCapture(edgeTrace:)`, `ImageDocument.edgeTraceComposite`, rule M).

/// The Magnetic Lasso tool: its tool action, the commit of a closed
/// outline, the `[` / `]` Width keys, the options bar's Width / Contrast /
/// Frequency, and the backstop that abandons an outline whose capture went
/// stale underneath it. The session is `MagneticLassoSession`; the canvas
/// half is `ImageCanvasView+MagneticLasso.swift`.
extension EditorViewController {
    @objc func selectMagneticLassoTool(_ sender: Any?) { selectTool(.magneticLasso) }

    /// Commits a closed outline as one "Magnetic Lasso" selection step,
    /// recorded as the resolved `select_freehand` polygon so a replay never
    /// depends on the image it was traced over.
    func finishMagneticLasso(_ session: MagneticLassoSession, outline: [CGPoint]) {
        guard let document else { return }
        do {
            try session.capture.commit(
                .polygon(outline), in: document, name: "Magnetic Lasso", mode: session.mode,
                feather: session.feather,
                record: .step("select_freehand",
                              ["points": ActionArgs.points(outline), "mode": session.mode.agentName,
                               "feather": session.feather],
                              note: "Magnetic Lasso tool"))
        } catch { NSApp.presentError(error) }
        syncSelectionHistory()
    }

    /// `[` / `]` on the canvas: the new Width, already clamped to 1…256.
    func magneticLassoWidthKeyChanged(_ width: Double) {
        var options = MagneticLassoSettings.shared.options
        options.width = width
        MagneticLassoSettings.shared.options = options
        canvas.magneticLasso?.options.width = width
        updateOptionsBar()
    }

    /// An open outline traces a capture of the document and selection at its
    /// first click; when either changed underneath it (an agent edit), the
    /// outline is abandoned with a beep rather than committed against
    /// pixels the user never traced.
    func magneticLassoDocumentDidChange() {
        guard let document, canvas.magneticLasso?.capture.isCurrent(in: document) == false else {
            return
        }
        canvas.cancelMagneticLasso(beep: true)
    }

    func magneticLassoClusters() -> [OptionCluster] {
        [OptionCluster([
            OptionDescriptor(id: "magnetic.width", microLabel: "Width",
                overflowLabel: "Edge detection width",
                kind: .field(width: 48, unit: " px", decimals: 0, min: 1, max: 256,
                    quick: [1, 3, 5, 10, 20, 40, 80, 256],
                    get: { MagneticLassoSettings.shared.options.width },
                    set: { [weak self] in self?.setMagneticLassoOption(\.width, $0) })),
            OptionDescriptor(id: "magnetic.contrast", microLabel: "Contrast",
                overflowLabel: "Edge contrast",
                kind: .field(width: 48, unit: " %", decimals: 0, min: 1, max: 100,
                    quick: [1, 5, 10, 20, 40, 60, 80, 100],
                    get: { MagneticLassoSettings.shared.options.contrast },
                    set: { [weak self] in self?.setMagneticLassoOption(\.contrast, $0) })),
            OptionDescriptor(id: "magnetic.frequency", microLabel: "Frequency",
                overflowLabel: "Anchor frequency",
                kind: .field(width: 48, unit: "", decimals: 0, min: 0, max: 100,
                    quick: [0, 20, 40, 57, 70, 85, 100],
                    get: { MagneticLassoSettings.shared.options.frequency },
                    set: { [weak self] in self?.setMagneticLassoOption(\.frequency, $0) })),
        ])]
    }

    /// One options-bar field: persisted, and taken by an open outline from
    /// its next trace on.
    private func setMagneticLassoOption(_ key: WritableKeyPath<MagneticLassoOptions, Double>,
                                        _ value: Double) {
        var options = MagneticLassoSettings.shared.options
        options[keyPath: key] = value
        MagneticLassoSettings.shared.options = options
        canvas.magneticLasso?.options = MagneticLassoSettings.shared.options
    }
}
