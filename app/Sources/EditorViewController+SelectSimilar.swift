import AppKit

// `commitSimilar` adds the result through `commit(mode: .add)` and returns false with no history
// entry when nothing new would be covered. The menu items use the Magic Wand tolerance;
// `select_similar` is the MCP mirror. Float refuses (not in `FloatDocument.tools`).

/// Select ▸ Grow (contiguous) and Select ▸ Similar (global): colour-based
/// growth of the current selection, shared by the menu items and the
/// `select_similar` MCP tool. The core owns the predicate
/// (`selection_similar.rs`); this file captures, runs and commits it.
extension SelectionCapture {
    /// The admitted region at full coverage, at the document's own depth.
    /// Throws "Make a selection first." with nothing selected, and the core's
    /// refusal sentence when no pixel is at least half selected.
    func similarShape(tolerance: Double, contiguous: Bool, in document: ImageDocument) throws
        -> CanvasSelection.Shape
    {
        try requireCurrent(in: document)
        guard let current = selection.selection else {
            throw DoctoredCoreError(message: "Make a selection first.")
        }
        let title = contiguous ? "Grow" : "Similar"
        let shape: CanvasSelection.Shape
        if doc.isHighDepth {
            guard let source = doc.precision else {
                throw DoctoredCoreError(message: "Native Grow/Similar requires a typed document.")
            }
            let operation: [String: Any] = [
                "kind": "similar", "tolerance": Int((tolerance * 257).rounded()),
                "contiguous": contiguous,
            ]
            shape = .mask16(
                try NativeSelectionWork(
                    source: source, width: doc.width, height: doc.height, operation: operation,
                    input: current.maskSamples16()
                ).run(title: title))
        } else {
            // Synchronous, like the wand: the byte walk has no job control.
            shape = .mask(
                try doc.selectSimilar(
                    mask: current.maskBytes(), tolerance: Int(tolerance.rounded()),
                    contiguous: contiguous))
        }
        try requireCurrent(in: document)
        return shape
    }

    /// Adds the admitted region to the selection as one undo step named
    /// "Grow" or "Similar". False, with nothing committed, when every
    /// admitted sample is already covered at least as much.
    func commitSimilar(tolerance: Double, contiguous: Bool, in document: ImageDocument,
                       record: [ActionStep] = .notACommand) throws -> Bool
    {
        let shape = try similarShape(tolerance: tolerance, contiguous: contiguous, in: document)
        let current = selection.selection?.maskSamples16() ?? []
        let admitted: [UInt16]
        switch shape {
        case .mask16(let samples): admitted = samples
        case .mask(let bytes): admitted = bytes.map { UInt16($0) * 257 }
        case .rect, .ellipse, .polygon: admitted = []
        }
        if admitted.count == current.count, !zip(admitted, current).contains(where: { $0 > $1 }) {
            return false
        }
        try commit(shape, in: document, name: contiguous ? "Grow" : "Similar", mode: .add,
                   record: record)
        return true
    }
}

extension EditorViewController {
    /// Select ▸ Grow: the Magic Wand tolerance, flooded four-connected from
    /// the selected pixels. Not Select ▸ Modify ▸ Expand (`growSelection:`).
    @objc func selectGrow(_ sender: Any?) {
        runSelectSimilar(contiguous: true)
    }

    /// Select ▸ Similar: the same range, over the whole image.
    @objc func selectSimilar(_ sender: Any?) {
        runSelectSimilar(contiguous: false)
    }

    private func runSelectSimilar(contiguous: Bool) {
        guard let document, !isTransforming else {
            NSSound.beep()
            return
        }
        let tolerance = max(0, min(255, ToolOptionsStore.shared.select.tolerance))
        let record: [ActionStep] = .step(
            "select_similar", ["tolerance": tolerance, "contiguous": contiguous],
            note: contiguous ? "Select ▸ Grow" : "Select ▸ Similar")
        do {
            let capture = try SelectionCapture(document: document, needsSource: false)
            let changed = try capture.commitSimilar(
                tolerance: tolerance, contiguous: contiguous, in: document, record: record)
            if !changed { NSSound.beep() }
        } catch {
            document.presentError(error)
        }
    }
}
