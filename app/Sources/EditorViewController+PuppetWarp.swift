import AppKit

/// Edit ▸ Puppet Warp, the controller half: opening the session on the
/// active layer (or the selection's float), its options bar, the one commit
/// and the cancel. The session is `PuppetWarpSession` on the canvas; the
/// commit is `MeshWarpWork`; the MCP mirror is `puppet_warp`.
extension EditorViewController {
    func installPuppetWarp() {
        canvas.onPuppetWarpChange = { [weak self] in self?.puppetWarpDidChange() }
        canvas.onPuppetWarpCommit = { [weak self] in self?.commitPuppetWarpFromCanvas() }
        canvas.onPuppetWarpCancel = { [weak self] in self?.cancelPuppetWarp() }
    }

    /// Edit ▸ Puppet Warp: commits any open session, refuses a target that is
    /// not a layer's pixels and a locked layer (Pixels; Position too for the
    /// layer form), asks before dropping a text or shape layer's description,
    /// and opens the session on the layer's opaque pixels, or on the
    /// selection's float when a selection is active.
    @objc func showPuppetWarp(_ sender: Any?) {
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
            canvas.puppetWarpSession = try PuppetWarpSession(
                document: document, base: doc, source: source, layer: layer,
                plates: transformStackComposites(doc, around: layer))
            presentToolOptions()
        } catch {
            document.presentError(error)
        }
    }

    func puppetWarpDidChange() {
        optionsBar.refreshValues()
    }

    private func commitPuppetWarpFromCanvas() {
        commitPuppetWarp()
    }

    /// Closes the session with no edit.
    func cancelPuppetWarp() {
        guard canvas.puppetWarpSession != nil else { return }
        canvas.puppetWarpSession = nil
        presentToolOptions()
    }

    /// Commits the open session as ONE "Puppet Warp" undo step and closes it;
    /// no pin, or none moved, closes it with no step. True when the session
    /// is gone; a refusal (presented) or a cancel keeps it open.
    @discardableResult func commitPuppetWarp() -> Bool {
        guard let document, var session = canvas.puppetWarpSession else { return true }
        guard !session.isCommitting else { return false }
        if session.pins.isEmpty || session.isIdentity {
            cancelPuppetWarp()
            return true
        }
        // The preview's lease leaves before the commit admits, which would
        // otherwise count it.
        session.isCommitting = true
        session.reservation = nil
        canvas.puppetWarpSession = session
        let work = MeshWarpWork(
            kind: .puppet, document: document, base: session.base, source: session.source, layer: session.layer,
            fields: PuppetWarp.fields(layer: session.layer, options: session.options,
                                      pins: session.pins.map(\.json), float: session.isFloat,
                                      bounds: session.selectionBounds),
            coverage: session.coverage, selection: session.selectionHistory)
        let outcome: PublishOutcome
        do {
            outcome = try work.publish(
                name: PuppetWarp.actionName,
                record: PuppetWarp.step(options: session.options, pins: session.pins))
        } catch {
            resumePuppetWarp(session)
            if !(error is JobCancelled) { document.presentError(error) }
            return false
        }
        guard outcome != .refused else {
            resumePuppetWarp(session)
            return false
        }
        cancelPuppetWarp()
        return true
    }

    /// A refused or cancelled commit keeps the session, with its lease back.
    private func resumePuppetWarp(_ session: PuppetWarpSession) {
        var resumed = session
        resumed.isCommitting = false
        resumed.reservation = try? SelectionWorkBudget.reservePreview(
            bytes: PuppetWarpSession.workspaceBytes(
                canvas: (session.base.width, session.base.height),
                proxy: (session.proxy.width, session.proxy.height)),
            outputBytes: 0)
        canvas.puppetWarpSession = resumed
    }

    /// The session's commit for the paths that must not leave it open (a
    /// tool switch, retargeting, save, export): `commitPendingTransform`.
    @discardableResult func commitPendingPuppetWarp() -> Bool {
        guard canvas.puppetWarpSession != nil else { return true }
        return commitPuppetWarp()
    }

    /// The document moved underneath the session (an agent edit, undo): its
    /// mesh and pins describe the layer it opened on, so it closes without
    /// committing. The session's own publication is exempt.
    func puppetWarpDocumentDidChange() {
        guard let session = canvas.puppetWarpSession, !session.isCommitting,
              document?.doc !== session.base else { return }
        cancelPuppetWarp()
    }

    // MARK: - Options bar

    private func updatePuppetWarp(_ change: (inout PuppetWarpSession) throws -> Void) {
        guard var session = canvas.puppetWarpSession, !session.isCommitting else { return }
        do {
            try change(&session)
            canvas.puppetWarpSession = session
        } catch {
            NSSound.beep()
            document?.presentError(error)
        }
        optionsBar.refreshValues()
    }

    private var puppetWarpHasSelection: Bool {
        canvas.puppetWarpSession?.selected.isEmpty == false
    }

    private var puppetWarpSinglePin: PuppetPin? {
        guard let session = canvas.puppetWarpSession, let index = session.singleSelection else {
            return nil
        }
        return session.pins[index]
    }

    func puppetWarpClusters() -> [OptionCluster] {
        let options = canvas.puppetWarpSession?.options ?? PuppetWarpOptions()
        return [
            OptionCluster([
                OptionDescriptor(
                    id: "puppet.mode", microLabel: "Mode", overflowLabel: "Mode",
                    kind: .popup(
                        width: 90, items: PuppetMode.allCases.map(\.title),
                        get: { [weak self] in
                            PuppetMode.allCases.firstIndex(
                                of: self?.canvas.puppetWarpSession?.options.mode ?? .normal) ?? 1
                        },
                        set: { [weak self] index in
                            guard PuppetMode.allCases.indices.contains(index) else { return }
                            self?.updatePuppetWarp { try $0.setMode(PuppetMode.allCases[index]) }
                        })),
                OptionDescriptor(
                    id: "puppet.density", microLabel: "Density", overflowLabel: "Density",
                    kind: .popup(
                        width: 120, items: PuppetDensity.allCases.map(\.title),
                        get: { [weak self] in
                            PuppetDensity.allCases.firstIndex(
                                of: self?.canvas.puppetWarpSession?.options.density ?? .normal) ?? 1
                        },
                        set: { [weak self] index in
                            guard PuppetDensity.allCases.indices.contains(index) else { return }
                            self?.updatePuppetWarp {
                                try $0.setMesh(density: PuppetDensity.allCases[index],
                                               expansion: $0.options.expansion)
                            }
                        })),
                OptionDescriptor(
                    id: "puppet.expansion", microLabel: "Expansion", overflowLabel: "Expansion",
                    kind: .field(
                        width: 56, unit: " px", decimals: 0,
                        min: Double(PuppetWarp.expansionRange.lowerBound),
                        max: Double(PuppetWarp.expansionRange.upperBound),
                        quick: [-10, -4, 0, 2, 4, 10, 20],
                        get: { [weak self] in Double(self?.canvas.puppetWarpSession?.options.expansion ?? options.expansion) },
                        set: { [weak self] value in
                            self?.updatePuppetWarp {
                                try $0.setMesh(density: $0.options.density,
                                               expansion: Int(value.rounded()))
                            }
                        })),
                OptionDescriptor(
                    id: "puppet.mesh", overflowLabel: "Show Mesh",
                    kind: .checkbox(
                        label: "Show Mesh",
                        get: { [weak self] in self?.canvas.puppetWarpSession?.options.showMesh ?? true },
                        set: { [weak self] show in self?.updatePuppetWarp { $0.setShowMesh(show) } })),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "puppet.depth.forward", microLabel: "Pin Depth", overflowLabel: "Pin Forward",
                    kind: .button(title: "+", action: { [weak self] in
                        self?.updatePuppetWarp { try $0.shiftDepth(1) }
                    }),
                    isEnabled: { [weak self] in self?.puppetWarpHasSelection ?? false }),
                OptionDescriptor(
                    id: "puppet.depth.back", overflowLabel: "Pin Backward",
                    kind: .button(title: "−", action: { [weak self] in
                        self?.updatePuppetWarp { try $0.shiftDepth(-1) }
                    }),
                    isEnabled: { [weak self] in self?.puppetWarpHasSelection ?? false }),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "puppet.rotate", microLabel: "Rotate", overflowLabel: "Rotate",
                    kind: .popup(
                        width: 70, items: ["Auto", "Fixed"],
                        get: { [weak self] in self?.puppetWarpSinglePin?.rotate == nil ? 0 : 1 },
                        set: { [weak self] index in
                            let current = self?.puppetWarpSinglePin?.rotate ?? 0
                            self?.updatePuppetWarp { try $0.setRotate(index == 0 ? nil : current) }
                        }),
                    isEnabled: { [weak self] in self?.puppetWarpSinglePin != nil }),
                OptionDescriptor(
                    id: "puppet.angle", overflowLabel: "Angle",
                    kind: .field(
                        width: 56, unit: "°", decimals: 1, min: -3600, max: 3600,
                        quick: [-90, -45, 0, 45, 90, 180],
                        get: { [weak self] in self?.puppetWarpSinglePin?.rotate ?? 0 },
                        set: { [weak self] value in
                            self?.updatePuppetWarp { try $0.setRotate(value) }
                        }),
                    isEnabled: { [weak self] in self?.puppetWarpSinglePin != nil }),
            ]),
            OptionCluster([
                OptionDescriptor(
                    id: "puppet.removeAll", overflowLabel: "Remove All Pins",
                    kind: .button(title: "Remove All Pins", action: { [weak self] in
                        self?.updatePuppetWarp { try $0.removeAll() }
                    }),
                    isEnabled: { [weak self] in self?.canvas.puppetWarpSession?.pins.isEmpty == false }),
                OptionDescriptor(
                    id: "puppet.cancel", overflowLabel: "Cancel",
                    kind: .button(title: "Cancel", action: { [weak self] in self?.cancelPuppetWarp() })),
                OptionDescriptor(
                    id: "puppet.commit", overflowLabel: "Commit",
                    kind: .button(title: "Commit", action: { [weak self] in self?.commitPuppetWarp() })),
            ]),
        ]
    }
}
