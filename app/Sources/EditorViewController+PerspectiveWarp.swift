import AppKit

/// Edit ▸ Perspective Warp, the controller half: opening the session
/// on the active layer (or the selection's float), its options bar, the one
/// commit and the cancel. The session is `PerspectiveWarpSession` on the
/// canvas; the commit is `MeshWarpWork(kind: .perspective)`; the MCP mirror
/// is `perspective_warp`.
extension EditorViewController {
    func installPerspectiveWarp() {
        canvas.onPerspectiveWarpChange = { [weak self] in self?.perspectiveWarpDidChange() }
        canvas.onPerspectiveWarpCommit = { [weak self] in self?.commitPerspectiveWarpFromCanvas() }
        canvas.onPerspectiveWarpCancel = { [weak self] in self?.cancelPerspectiveWarp() }
    }

    /// Edit ▸ Perspective Warp: commits any open session, refuses a target
    /// that is not a layer's pixels and a locked layer (Pixels; Position too
    /// for the layer form), asks before dropping a text or shape layer's
    /// description, and opens the session in Layout on the layer, or on the
    /// selection's float when a selection is active.
    @objc func showPerspectiveWarp(_ sender: Any?) {
        guard let document, let doc = document.doc else { NSSound.beep(); return }
        guard commitPendingSessions() else { return }
        guard !refuseOperationScope(.pixelOnly) else { return }
        let layer = document.activeLayerIndex
        guard !refuseLockedEdit(layer: layer, kind: DR_EDIT_PIXELS) else { return }
        if document.canvasSelection == nil,
           refuseLockedEdit(layer: layer, kind: DR_EDIT_POSITION) { return }
        var source = doc
        if document.layerDescribesSource(layer) {
            guard document.confirmRasterize(layer: layer) else { return }
            guard let plain = doc.withLayerMeta(layer, nil) else {
                document.presentError(DoctoredCoreError(message: "The layer's description could not be dropped."))
                return
            }
            source = plain
        }
        do {
            canvas.perspectiveWarpSession = try PerspectiveWarpSession(
                document: document, base: doc, source: source, layer: layer,
                plates: transformStackComposites(doc, around: layer))
            presentToolOptions()
        } catch {
            document.presentError(error)
        }
    }

    func perspectiveWarpDidChange() {
        optionsBar.refreshValues()
    }

    private func commitPerspectiveWarpFromCanvas() {
        commitPerspectiveWarp()
    }

    /// Closes the session with no edit.
    func cancelPerspectiveWarp() {
        guard canvas.perspectiveWarpSession != nil else { return }
        canvas.perspectiveWarpSession = nil
        presentToolOptions()
    }

    /// Commits the open session as ONE "Perspective Warp" undo step and
    /// closes it; no quad, or an unwarped layout, closes it with no step.
    /// True when the session is gone; a refusal (presented) or a cancel keeps
    /// it open.
    @discardableResult func commitPerspectiveWarp() -> Bool {
        guard let document, var session = canvas.perspectiveWarpSession else { return true }
        guard !session.isCommitting else { return false }
        if session.isIdentity {
            cancelPerspectiveWarp()
            return true
        }
        // The preview's lease leaves before the commit admits, which would
        // otherwise count it.
        session.isCommitting = true
        session.reservation = nil
        canvas.perspectiveWarpSession = session
        let quads = session.layout.quadsJSON
        let work = MeshWarpWork(
            kind: .perspective, document: document, base: session.base, source: session.source,
            layer: session.layer,
            fields: PerspectiveWarp.fields(layer: session.layer, quads: quads,
                                           float: session.isFloat,
                                           bounds: session.workspace.selectionBounds),
            coverage: session.coverage, selection: session.workspace.selectionHistory)
        let outcome: PublishOutcome
        do {
            outcome = try work.publish(name: PerspectiveWarp.actionName,
                                       record: PerspectiveWarp.step(quads: quads))
        } catch {
            resumePerspectiveWarp(session)
            if !(error is JobCancelled) { document.presentError(error) }
            return false
        }
        guard outcome != .refused else {
            resumePerspectiveWarp(session)
            return false
        }
        cancelPerspectiveWarp()
        return true
    }

    /// A refused or cancelled commit keeps the session, with its lease back.
    private func resumePerspectiveWarp(_ session: PerspectiveWarpSession) {
        var resumed = session
        resumed.isCommitting = false
        resumed.workspace.renewReservation()
        canvas.perspectiveWarpSession = resumed
    }

    /// The session's commit for the paths that must not leave it open (a
    /// tool switch, retargeting, save, export): `commitPendingTransform`.
    @discardableResult func commitPendingPerspectiveWarp() -> Bool {
        guard canvas.perspectiveWarpSession != nil else { return true }
        return commitPerspectiveWarp()
    }

    /// The document moved underneath the session (an agent edit, undo): its
    /// quads describe the layer it opened on, so it closes without
    /// committing. The session's own publication is exempt.
    func perspectiveWarpDocumentDidChange() {
        guard let session = canvas.perspectiveWarpSession, !session.isCommitting,
              document?.doc !== session.base else { return }
        cancelPerspectiveWarp()
    }

    // MARK: - Options bar

    private func updatePerspectiveWarp(_ change: (inout PerspectiveWarpSession) throws -> Void) {
        guard var session = canvas.perspectiveWarpSession, !session.isCommitting else { return }
        do {
            try change(&session)
            canvas.perspectiveWarpSession = session
        } catch {
            NSSound.beep()
            document?.presentError(error)
        }
        optionsBar.refreshValues()
    }

    private var perspectiveWarpInWarp: Bool { canvas.perspectiveWarpSession?.mode == .warp }

    func perspectiveWarpClusters() -> [OptionCluster] {
        [
            OptionCluster([
                OptionDescriptor(
                    id: "perspective.mode", overflowLabel: "Mode",
                    kind: .segmented(
                        segments: [
                            ("square.on.square.dashed", "L", "Layout: draw and join the planes"),
                            ("perspective", "W", "Warp: drag the pins"),
                        ],
                        get: { [weak self] in self?.canvas.perspectiveWarpSession?.mode.rawValue ?? 0 },
                        set: { [weak self] index in
                            guard let mode = PerspectiveWarpMode(rawValue: index) else { return }
                            self?.updatePerspectiveWarp { try $0.setMode(mode) }
                        },
                        segmentEnabled: { [weak self] index in
                            index == 0 || self?.canvas.perspectiveWarpSession?.layout.quads.isEmpty == false
                        })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "perspective.vertical", overflowLabel: "Straighten Vertical",
                    kind: .button(title: "Straighten Vertical", action: { [weak self] in
                        self?.updatePerspectiveWarp { try $0.straightenAll([.nearVertical]) }
                    }),
                    isEnabled: { [weak self] in self?.perspectiveWarpInWarp ?? false }),
                OptionDescriptor(
                    id: "perspective.horizontal", overflowLabel: "Level Horizontal",
                    kind: .button(title: "Level Horizontal", action: { [weak self] in
                        self?.updatePerspectiveWarp { try $0.straightenAll([.nearHorizontal]) }
                    }),
                    isEnabled: { [weak self] in self?.perspectiveWarpInWarp ?? false }),
                OptionDescriptor(
                    id: "perspective.both", overflowLabel: "Straighten Both",
                    kind: .button(title: "Straighten Both", action: { [weak self] in
                        self?.updatePerspectiveWarp {
                            try $0.straightenAll([.nearVertical, .nearHorizontal])
                        }
                    }),
                    isEnabled: { [weak self] in self?.perspectiveWarpInWarp ?? false }),
                OptionDescriptor(
                    id: "perspective.remove", overflowLabel: "Remove Warp",
                    kind: .button(title: "Remove Warp", action: { [weak self] in
                        self?.updatePerspectiveWarp { try $0.removeWarp() }
                    }),
                    isEnabled: { [weak self] in self?.perspectiveWarpInWarp ?? false }),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "perspective.cancel", overflowLabel: "Cancel",
                    kind: .button(title: "Cancel", action: { [weak self] in self?.cancelPerspectiveWarp() })),
                OptionDescriptor(
                    id: "perspective.commit", overflowLabel: "Commit",
                    kind: .button(title: "Commit", action: { [weak self] in self?.commitPerspectiveWarp() })),
            ]),
        ]
    }
}
