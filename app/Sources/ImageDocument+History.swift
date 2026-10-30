import AppKit

// The history transition records the `renderNeutral` flag (`DocumentHistory.Entry.renderNeutral`,
// ANDed when edits merge into one transition), and `restoreHistoryTransition` hands
// `carriedProjection` to `preparedEdit` for such a transition, so Undo and Redo of a rename, label,
// lock, link or channel name/overlay carry the display too — and of a guide or ruler-origin edit:
// `ImageDocument.applyGuideEdit` re-points the retained projection at the guide-only document and
// records its transition `renderNeutral: true`.

extension ImageDocument {
    func isCurrentEditingState(_ state: EditingState?) -> Bool {
        guard let state, let current = captureEditingState() else { return false }
        return state.equivalent(to: current)
    }

    var historySavedDocument: RasterDocument? { savedDocument }

    var historySavedState: EditingState? {
        guard let savedDocument else { return nil }
        return EditingState(document: savedDocument, selection: SelectionHistoryState(),
            layerSelection: .single(0), paintTarget: .layer, jpegExportQuality: jpegExportQuality,
            embedColorProfile: embedColorProfile, stripMetadata: stripMetadata,
            exportBitDepth: exportBitDepth, exportDither: exportDither)
    }

    func captureEditingState() -> EditingState? {
        guard let doc else { return nil }
        return EditingState(document: doc, selection: selectionHistory,
            layerSelection: layerSelection, paintTarget: planeEditTarget,
            jpegExportQuality: jpegExportQuality, embedColorProfile: embedColorProfile,
            stripMetadata: stripMetadata, exportBitDepth: exportBitDepth, exportDither: exportDither,
            channelVisibility: channelVisibility)
    }

    /// Snapshot restoration is a new edit, unlike navigation through existing
    /// history. Source handle identity remains the saved/dirty authority.
    func restoreEditingState(_ state: EditingState, actionName: String) {
        guard !isLiveEditing, !isPublishingEditingState,
              let before = captureEditingState(), !before.equivalent(to: state) else { return }
        // A snapshot is an explicit saved state, so it restores with
        // the sampler list it was taken with — never the current one, which
        // a same-size flip or Rotate 180 since the snapshot has moved. Its
        // transition is marked `changesGeometry` (when the document changes;
        // a selection-only restore carries like any ordinary step) so undo
        // and redo across it likewise bring each side's own list.
        history.suspended += 1
        applyHistoryValues(state)
        history.suspended -= 1
        let documentChanged = before.document !== state.document
        if documentChanged { countEditChange(.changeDone) }
        registerHistoryChange(from: before, actionName: actionName,
                              changesGeometry: documentChanged)
    }

    /// Recovery/import initializes a complete state with an explicit saved
    /// marker and no fictional pre-recovery Undo target.
    func installEditingState(_ state: EditingState, markSaved: Bool) {
        history.suspended += 1
        applyHistoryValues(state)
        lastWrittenDocument = nil
        if markSaved {
            updateChangeCount(.changeCleared)
        } else {
            savedDocument = nil
            countEditChange(.changeDone)
        }
        history.suspended -= 1
        undoManager?.removeAllActions()
        history.reset(captureEditingState())
    }

    /// Called after the existing central mutation paths complete. One ID is
    /// registered per outer Undo group; entries themselves own all full states.
    func registerHistoryChange(from before: EditingState, actionName: String,
                               renderNeutral: Bool = false, changesGeometry: Bool = false) {
        guard let after = captureEditingState(), !before.equivalent(to: after),
              let manager = undoManager, manager.isUndoRegistrationEnabled,
              history.suspended == 0, !history.navigating, !history.rebuilding else { return }
        // Record first: a merge into a still-pending transition (nil) must not
        // open a deferred group, which would be left as an empty Undo step.
        let target = history.record(before: before, after: after, name: actionName,
                                    renderNeutral: renderNeutral, changesGeometry: changesGeometry)
        let historyManager = manager as? HistoryUndoManager
        if target == nil, historyManager?.defersOuterGroup == true, manager.groupingLevel == 0 {
            // The merge renamed a transition whose group already closed, and
            // `setActionName` raises with no group open: rebuild the
            // registrations from history's names, as the earlier helper did.
            rebuildHistoryUndo()
            return
        }
        if target != nil { historyManager?.openDeferredGroupIfNeeded() }
        withUndoGroup(manager) {
            if let target {
                manager.registerUndo(withTarget: target) { [weak self, target] _ in
                    self?.restoreHistoryTransition(target.id)
                }
            }
            manager.setActionName(actionName)
        }
    }

    private func restoreHistoryTransition(_ id: String) {
        guard let index = history.entryIndex(id: id), index > 0,
              let manager = undoManager else { return }
        let name = history.entries[index].name
        let target = history.entries[index].undoTarget
        manager.registerUndo(withTarget: target) { [weak self, target] _ in
            self?.restoreHistoryTransition(target.id)
        }
        manager.setActionName(name)
        guard !history.rebuilding else { return }
        history.refreshCurrentViewState()
        let destination = manager.isRedoing ? index : index - 1
        var restored = history.entries[destination].state
        // The samplers are saved document state that ordinary undo and
        // redo must not take back. A non-geometry transition
        // carries the CURRENT handle's list onto the destination entry —
        // that one entry only — and keeps the destination's own handle when
        // the lists already match (the core answers nil), so saved-handle
        // identity holds. A geometry transition restores the destination's
        // own list, which the core moved with the canvas.
        if !history.entries[index].changesGeometry, let current = doc,
           let carried = try? restored.document.withColorSamplers(current.colorSamplers) {
            history.replaceDocument(at: destination, with: carried)
            restored = history.entries[destination].state
        }
        let pixelsChanged = doc !== restored.document
        // Entry `index` describes the index − 1 ↔ index transition in
        // both directions, so a render-neutral one (a rename, label, lock,
        // link or channel name/overlay) carries the current display
        // projection across undo and redo instead of preparing a new one.
        if pixelsChanged, history.entries[index].renderNeutral, let current = doc,
           let carried = carriedProjection(from: current, to: restored.document) {
            preparedEdit = carried
        }
        history.navigating = true
        defer { history.navigating = false }
        applyHistoryValues(restored)
        history.move(to: destination)
        if pixelsChanged { countEditChange(manager.isRedoing ? .changeRedone : .changeUndone) }
    }

    private func applyHistoryValues(_ state: EditingState) {
        let wasPublishing = isPublishingEditingState
        isPublishingEditingState = true
        defer { isPublishingEditingState = wasPublishing }
        let pixelsChanged = doc !== state.document
        // Undo, redo, a History jump or a restore: nothing is fadeable after it.
        fadeCandidate = nil
        doc = state.document
        setLayerSelection(state.layerSelection)
        selectionHistory = state.selection
        jpegExportQuality = state.jpegExportQuality
        embedColorProfile = state.embedColorProfile
        stripMetadata = state.stripMetadata
        exportBitDepth = state.exportBitDepth
        exportDither = state.exportDither
        planeEditTarget = state.paintTarget
        channelVisibility = state.channelVisibility
        if pixelsChanged { docDidChange() }
        else {
            NotificationCenter.default.post(name: .imageDocumentImageDidChange,
                object: self, userInfo: ["isLive": false])
        }
        for controller in windowControllers {
            if let editor = controller.contentViewController as? EditorViewController {
                editor.setPaintTarget(state.paintTarget)
                editor.restoreChannelVisibility(state.channelVisibility)
            }
        }
        notifySelectionChanged()
    }

    /// After budget eviction, reconstruct the public manager from our own
    /// enumerable transitions. Synthetic Undo builds redo registrations without
    /// touching pixels, dirty state, notifications or the history cursor.
    func rebuildHistoryUndo() {
        guard let manager = undoManager, manager.groupingLevel == 0,
              !manager.isUndoing, !manager.isRedoing, !history.rebuilding else { return }
        history.rebuilding = true
        let groupedByEvent = manager.groupsByEvent
        defer { manager.groupsByEvent = groupedByEvent; history.rebuilding = false }
        manager.groupsByEvent = false
        manager.removeAllActions()
        for entry in history.entries.dropFirst() {
            manager.beginUndoGrouping()
            let target = entry.undoTarget
            manager.registerUndo(withTarget: target) { [weak self, target] _ in
                self?.restoreHistoryTransition(target.id)
            }
            manager.setActionName(entry.name)
            manager.endUndoGrouping()
        }
        for _ in 0..<max(0, history.entries.count - history.cursor - 1) { manager.undo() }
    }

    /// After budget eviction: removes only the evicted transitions'
    /// registrations, each on its own `HistoryUndoTarget`, under
    /// `rebuildHistoryUndo`'s guard; otherwise it does nothing, as the rebuild does.
    ///
    /// The residual O(n): `removeAllActions(withTarget:)` walks the
    /// manager's whole stack per target, and AppKit's only other removal is
    /// `removeAllActions()`. `levelsOfUndo` stays 0 — it cannot follow the
    /// byte budget or the redo-tail eviction — so the walk stays; at 2000
    /// entries it measured 0.115 ms per eviction (`cap_drop_ms`, median of
    /// three runs). With the history's own eviction O(1), it is
    /// most of a capped edit's growth from 50 to 2000 entries: 0.112 of
    /// 0.156 ms (jobs-probe "history bookkeeping cost", -Onone).
    func dropHistoryUndo(for targets: [HistoryUndoTarget]) {
        guard let manager = undoManager, manager.groupingLevel == 0,
              !manager.isUndoing, !manager.isRedoing, !history.rebuilding else { return }
        for target in targets { manager.removeAllActions(withTarget: target) }
    }

    /// `withUndoGroup` for a body whose outcome is known only after it runs:
    /// NSUndoManager keeps an empty explicit group as an Undo step,
    /// and has no call that removes one, so an outermost group is never
    /// opened up front. The `HistoryUndoManager` defers it instead: the
    /// body's first history registration opens it (`registerHistoryChange`),
    /// and a body that records no transition opens none — no step, the redo
    /// tail untouched and no rebuild. Automatic event grouping is off
    /// for the body, as in `withUndoGroup`. A body nested in a deferral runs
    /// in it; nested, disabled, navigating or publishing callers get plain
    /// `withUndoGroup`.
    func withUndoGroupDiscardingEmpty<T>(_ body: () -> T) -> T {
        let historyManager = undoManager as? HistoryUndoManager
        if historyManager?.defersOuterGroup == true { return body() }
        let discardable: Bool
        if let manager = undoManager {
            discardable = manager.groupingLevel == 0 && manager.isUndoRegistrationEnabled
                && !manager.isUndoing && !manager.isRedoing && history.suspended == 0
                && !history.navigating && !history.rebuilding && !isPublishingEditingState
        } else {
            discardable = false
        }
        if discardable, let historyManager {
            let groupedByEvent = historyManager.groupsByEvent
            historyManager.groupsByEvent = false
            historyManager.defersOuterGroup = true
            defer {
                historyManager.closeDeferredGroup()
                historyManager.groupsByEvent = groupedByEvent
            }
            return body()
        }
        return withUndoGroup(undoManager, body)
    }

    func navigateHistory(toID id: String) throws {
        guard !isLiveEditing, !isPublishingEditingState, !ActionPlayer.isPlaying,
              (undoManager?.groupingLevel ?? 0) <= 1 else {
            throw DoctoredCoreError(message: "Finish the current operation before navigating history.")
        }
        guard let destination = history.entries.firstIndex(where: { $0.id == id }) else {
            throw DoctoredCoreError(message: "This history state is no longer retained.")
        }
        guard let manager = undoManager, history.cursor != destination else { return }
        // Every step is estimated live; the largest overage becomes one
        // warning for the entry the walk lands on. Navigation never refuses.
        let destinationName = history.entries[destination].name
        let historyManager = manager as? HistoryUndoManager
        historyManager?.navigationOverage = 0
        defer {
            let worst = historyManager?.navigationOverage ?? 0
            historyManager?.navigationOverage = nil
            publishNavigationOverage(worst, destinationName: destinationName)
        }
        while history.cursor > destination {
            let before = history.cursor
            manager.undo()
            guard history.cursor < before else { throw DoctoredCoreError(message: "History could not undo this state.") }
        }
        while history.cursor < destination {
            let before = history.cursor
            manager.redo()
            guard history.cursor > before else { throw DoctoredCoreError(message: "History could not redo this state.") }
        }
    }

    /// One `undo()` or `redo()` on the manager: true iff `history.cursor`
    /// moved. False for a step that held no document change, such as an
    /// empty explicit group, so MCP `undo`/`redo` never answer ok for
    /// a step that changed nothing.
    func stepHistory(redo: Bool) -> Bool {
        guard let manager = undoManager, manager.groupingLevel == 0 else { return false }
        let before = history.cursor
        if redo { manager.redo() } else { manager.undo() }
        return history.cursor != before
    }

    func setHistoryBudget(_ bytes: UInt64) throws {
        guard bytes > 0, !isLiveEditing, !isPublishingEditingState, !ActionPlayer.isPlaying,
              (undoManager?.groupingLevel ?? 0) == 0 else {
            throw DoctoredCoreError(message: "Choose a positive history budget after the current operation finishes.")
        }
        history.budgetBytes = bytes
        history.enforceBudget()
        history.notify()
    }

    func showHistoryPanel() {
        if history.entries.isEmpty { history.reset(captureEditingState()) }
        if historyWindowController == nil { historyWindowController = HistoryWindowController(document: self) }
        historyWindowController?.showWindow(nil)
        historyWindowController?.window?.makeKeyAndOrderFront(nil)
    }
}
