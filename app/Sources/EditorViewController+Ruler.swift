import AppKit

/// The controller half of the Ruler tool (RulerTool.swift): the tool
/// action, the options bar's readouts (X and Y of the start point, W and H
/// the signed run to the end point, A the angle and L the length) with
/// Straighten Layer and Clear, and the Straighten Layer commit. MCP
/// `straighten_layers` is the twin of that commit.
extension EditorViewController {
    @objc func selectRulerTool(_ sender: Any?) { selectTool(.ruler) }

    /// Called once from `loadView`.
    func installRuler() {
        canvas.onRulerChange = { [weak self] in self?.rulerDidChange() }
        NotificationCenter.default.addObserver(
            self, selector: #selector(rulerDocumentDidChange(_:)),
            name: .imageDocumentImageDidChange, object: document)
    }

    /// The document changed: a line drawn on a canvas of another size is
    /// stale (the canvas already reads it as none), so it is dropped.
    @objc private func rulerDocumentDidChange(_ note: Notification) {
        // Against the DOCUMENT's size: the canvas may not have resized yet.
        guard note.userInfo?["isLive"] as? Bool != true, canvas.rulerSession.line != nil,
              let doc = document?.doc,
              canvas.rulerSession.canvasSize != CGSize(width: doc.width, height: doc.height)
        else { return }
        clearRuler()
    }

    /// The line changed (a drag tick, a clear, a canvas of another size):
    /// the bar's readouts and its two buttons follow.
    private func rulerDidChange() {
        guard currentTool == .ruler else { return }
        optionsBar.refreshValues()
    }

    /// Clear: the line goes away. Not an edit, so no undo step.
    func clearRuler() {
        canvas.clearRuler()
        optionsBar.refreshValues()
    }

    /// Straighten Layer: rotates the layer selection about the canvas centre
    /// by the smallest angle that lays the ruler line along an axis, through
    /// the sessionless set transform ⇧⌘T uses (`commitLayerSetTransform`),
    /// as ONE "Straighten Layer" step. The line is cleared once it commits:
    /// it described the picture as it was.
    func straightenLayerWithRuler() {
        // Never leave a text or shape session under the edit, as ⌘T does.
        guard canvas.commitTextSession(), commitShapeEditSession() else { return }
        guard transformSession == nil, let document, let doc = document.doc,
              let line = canvas.rulerMeasurement,
              let matrix = line.straightenMatrix(canvasWidth: doc.width, canvasHeight: doc.height)
        else {
            NSSound.beep()
            return
        }
        // #1 Bitmap phase 3: the straightened crop (EditorViewController+BitmapMode.swift).
        if doc.isBitmap {
            // The line stays when the straighten was refused or changed nothing.
            if straightenBitmapWithRuler(line) { clearRuler() }
            return
        }
        let sampler = transformSampler
        let record: [ActionStep] = .step(
            "straighten_layers",
            ["x0": Double(line.start.x), "y0": Double(line.start.y),
             "x1": Double(line.end.x), "y1": Double(line.end.y),
             "sampler": Self.samplerAgentName(sampler)],
            note: "Ruler tool: Straighten Layer")
        guard commitLayerSetTransform(
            document, matrix: matrix, quad: nil, sourceRect: nil, sampler: sampler,
            record: record, name: "Straighten Layer") else { return }
        document.lastFreeTransform = RepeatableTransform(matrix: matrix, sampler: sampler)
        clearRuler()
    }

    /// The Ruler's options bar.
    func rulerClusters() -> [OptionCluster] {
        let number: (Double?) -> String = { value in
            value.map { String(format: "%.1f", $0) } ?? "—"
        }
        let line: () -> RulerMeasurement? = { [weak self] in self?.canvas.rulerMeasurement }
        func readout(_ id: String, _ label: String, _ name: String,
                     _ value: @escaping (RulerMeasurement) -> Double) -> OptionDescriptor {
            OptionDescriptor(
                id: "ruler.\(id)", microLabel: label, overflowLabel: name,
                kind: .display(width: 56, get: { number(line().map(value)) }))
        }
        return [
            OptionCluster([
                readout("x", "X", "Start X") { Double($0.start.x) },
                readout("y", "Y", "Start Y") { Double($0.start.y) },
            ]),
            OptionCluster([
                readout("w", "W", "Width") { Double($0.end.x - $0.start.x) },
                readout("h", "H", "Height") { Double($0.end.y - $0.start.y) },
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "ruler.a", microLabel: "A", overflowLabel: "Angle",
                    kind: .display(width: 56, get: {
                        line().map { String(format: "%.1f°", $0.angleDegrees) } ?? "—"
                    })),
                readout("l", "L", "Length") { $0.length },
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "ruler.straighten", overflowLabel: "Straighten Layer",
                    kind: .button(
                        title: "Straighten Layer",
                        action: { [weak self] in self?.straightenLayerWithRuler() }),
                    isEnabled: { line()?.straightenRotation() != nil }),
                OptionDescriptor(
                    id: "ruler.clear", overflowLabel: "Clear",
                    kind: .button(title: "Clear", action: { [weak self] in self?.clearRuler() }),
                    isEnabled: { line() != nil }),
            ]),
        ]
    }
}
