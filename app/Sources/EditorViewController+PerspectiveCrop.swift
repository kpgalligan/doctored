import AppKit

/// The controller half of the Perspective Crop tool
/// (PerspectiveCrop.swift): the tool action, the options bar (W and H of the
/// result, 0 meaning the automatic size, and Reset) and the commit, which is
/// the Crop tool's prepared-edit pattern: one cancellable job that resamples
/// the document and prepares its display, published by `applyEdit` as ONE
/// "Perspective Crop" step recorded as `perspective_crop`.
extension EditorViewController {
    @objc func selectPerspectiveCropTool(_ sender: Any?) { selectTool(.perspectiveCrop) }

    /// Called once from `loadView`.
    func installPerspectiveCrop() {
        canvas.onPerspectiveCropCommit = { [weak self] in self?.commitPerspectiveCrop() }
        canvas.onPerspectiveCropCancel = { [weak self] in self?.resetPerspectiveCrop() }
        canvas.onPerspectiveCropChange = { [weak self] in self?.perspectiveCropDidChange() }
    }

    private func perspectiveCropDidChange() {
        guard currentTool == .perspectiveCrop else { return }
        optionsBar.refreshValues()
    }

    /// Escape and Reset: the quadrilateral goes away (a tool switch keeps
    /// it). The bar's W and H stay, like every other option.
    func resetPerspectiveCrop() {
        var session = PerspectiveCropSession()
        session.width = canvas.perspectiveCropSession.width
        session.height = canvas.perspectiveCropSession.height
        canvas.perspectiveCropSession = session
        optionsBar.refreshValues()
    }

    /// Return or a double-click. A quadrilateral that is not convex beeps
    /// (the canvas already draws it red); described layers are confirmed
    /// once, then rasterized inside the same edit.
    func commitPerspectiveCrop() {
        // Never leave a text or shape session under the edit, as ⌘T does —
        // and commit it BEFORE the document is read: a session commit can add
        // a layer, which the described-layer list below must include.
        guard canvas.commitTextSession(), commitShapeEditSession() else { return }
        let session = canvas.perspectiveCropSession
        guard let document, let doc = document.doc, let corners = session.corners,
              corners.count == 4, PerspectiveCropSession.isConvex(corners),
              let size = session.outputSize()
        else {
            NSSound.beep()
            return
        }
        let described = (0..<doc.layerCount).filter { document.layerDescribesSource($0) }
        if !described.isEmpty, !confirmPerspectiveCropRasterize(count: described.count) { return }
        let request = PerspectiveCropRequest(
            corners: corners, width: size.width, height: size.height, sampler: transformSampler)
        let before = document.doc
        let background = backgroundColor
        // The Crop tool's pattern: the job's prepared display is published by
        // `applyEdit`, which presents a core refusal's own sentence and
        // presents nothing for a cancel.
        defer { document.preparedEdit = nil }
        document.applyEdit(
            "Perspective Crop",
            record: .step(
                "perspective_crop", request.arguments(rasterizeArtwork: !described.isEmpty),
                note: "Perspective Crop tool"),
            changesCanvasGeometry: true
        ) { current in
            guard let prepared = try current.perspectiveCropped(
                request, described: described, indexedBackground: background)
            else { return nil }
            document.preparedEdit = prepared
            return prepared.document
        }
        guard document.doc !== before else { return }
        resetPerspectiveCrop()
    }

    private func confirmPerspectiveCropRasterize(count: Int) -> Bool {
        let alert = NSAlert()
        alert.messageText = "Rasterize described layers?"
        alert.informativeText =
            "Perspective Crop resamples every layer's pixels, so \(count) "
            + "layer\(count == 1 ? "" : "s") will no longer be editable as text or "
            + "shape. The pixels themselves are kept."
        alert.addButton(withTitle: "Rasterize")
        alert.addButton(withTitle: "Cancel")
        return alert.runModal() == .alertFirstButtonReturn
    }

    /// The Perspective Crop's options bar.
    func perspectiveCropClusters() -> [OptionCluster] {
        [
            OptionCluster([
                OptionDescriptor(
                    id: "pcrop.w", microLabel: "W", overflowLabel: "Width (0 = automatic)",
                    kind: .field(
                        width: 58, unit: " px", decimals: 0, min: 0, max: 100_000, quick: [],
                        get: { [weak self] in
                            Double(self?.canvas.perspectiveCropSession.width ?? 0)
                        },
                        set: { [weak self] in
                            self?.canvas.perspectiveCropSession.width = Int($0.rounded())
                        })),
                OptionDescriptor(
                    id: "pcrop.h", microLabel: "H", overflowLabel: "Height (0 = automatic)",
                    kind: .field(
                        width: 58, unit: " px", decimals: 0, min: 0, max: 100_000, quick: [],
                        get: { [weak self] in
                            Double(self?.canvas.perspectiveCropSession.height ?? 0)
                        },
                        set: { [weak self] in
                            self?.canvas.perspectiveCropSession.height = Int($0.rounded())
                        })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "pcrop.reset", overflowLabel: "Reset",
                    kind: .button(
                        title: "Reset", action: { [weak self] in self?.resetPerspectiveCrop() }),
                    isEnabled: { [weak self] in
                        self?.canvas.perspectiveCropSession.corners != nil
                    }),
            ]),
        ]
    }
}
