import AppKit

/// Image ▸ Trim… and Image ▸ Reveal All (CanvasTrim.swift): the menu
/// actions and their commits. Each is ONE undo step through an ordinary
/// canvas op — Trim a crop to the kept rectangle, Reveal All a canvas resize
/// — and a command with nothing to change beeps and leaves no step. MCP
/// twins: `trim` and `reveal_canvas` (AgentServer+CanvasFraming.swift).
extension EditorViewController {
    @objc func showTrim(_ sender: Any?) {
        guard document?.doc != nil else {
            NSSound.beep()
            return
        }
        presentAsSheet(TrimSheetController { [weak self] basis, edges in
            self?.commitTrim(basis: basis, edges: edges)
        })
    }

    /// The Trim sheet's OK: crop the canvas to the kept rectangle. Beeps,
    /// with no step, when no edge would move or nothing would be left.
    func commitTrim(basis: TrimBasis, edges: TrimEdges) {
        guard let document, let doc = document.doc else {
            NSSound.beep()
            return
        }
        let kept: CGRect?
        do {
            kept = try document.trimRect(basis: basis, edges: edges)
        } catch {
            document.presentError(error)
            return
        }
        guard let rect = kept, rect != CGRect(x: 0, y: 0, width: doc.width, height: doc.height)
        else {
            NSSound.beep()
            return
        }
        document.applyEdit(
            "Trim",
            record: .step("trim", CanvasFraming.trimArguments(basis: basis, edges: edges),
                          note: "Image ▸ Trim…"),
            changesCanvasGeometry: true,
            resultingSelection: document.selectionCarried(
                croppedToX: Int(rect.minX), y: Int(rect.minY),
                width: Int(rect.width), height: Int(rect.height))
        ) {
            try $0.cropped(
                x: Int(rect.minX), y: Int(rect.minY), w: Int(rect.width), h: Int(rect.height))
        }
    }

    /// Image ▸ Reveal All: grow the canvas until it holds every layer's
    /// pixels. Beeps, with no step, when nothing reaches outside it.
    @objc func revealAllCanvas(_ sender: Any?) {
        guard let document, let doc = document.doc, let rect = doc.revealAllRect() else {
            NSSound.beep()
            return
        }
        let width = Int(rect.width), height = Int(rect.height)
        // Growing the canvas grows every saved channel with it (ChannelBudget).
        if let reason = doc.channelBudgetRefusal(width: width, height: height) {
            document.presentError(DoctoredCoreError(message: reason))
            return
        }
        document.applyEdit(
            "Reveal All", record: .step("reveal_canvas", [:], note: "Image ▸ Reveal All"),
            changesCanvasGeometry: true,
            resultingSelection: document.selectionCarried(
                canvasWidth: width, height: height,
                originX: Int(-rect.minX), originY: Int(-rect.minY))
        ) {
            try $0.canvasResized(
                w: width, h: height, originX: Int(-rect.minX), originY: Int(-rect.minY))
        }
    }
}

/// The argument spelling `trim` shares between its recorded step and the tool.
enum CanvasFraming {
    static func trimArguments(basis: TrimBasis, edges: TrimEdges) -> [String: Any] {
        [
            "based_on": basis.agentName,
            "top": edges.contains(.top), "bottom": edges.contains(.bottom),
            "left": edges.contains(.left), "right": edges.contains(.right),
        ]
    }
}
