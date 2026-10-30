import AppKit

extension Notification.Name {
    static let imageDocumentSelectionDidChange = Notification.Name("ImageDocumentSelectionDidChange")
}

/// Transient coverage belongs to the document, including when Quick Mask is visible.
/// Snapshots share Swift's copy-on-write mask storage; none of this is written to .rz.
struct SelectionHistoryState: Equatable {
    var selection: CanvasSelection?
    var previous: CanvasSelection?
    var quickMaskActive = false

    static func == (a: Self, b: Self) -> Bool {
        equivalent(a.selection, b.selection) && equivalent(a.previous, b.previous)
            && a.quickMaskActive == b.quickMaskActive
    }

    static func equivalent(_ a: CanvasSelection?, _ b: CanvasSelection?) -> Bool {
        switch (a, b) {
        case (nil, nil): return true
        case let (a?, b?):
            return a.canvasWidth == b.canvasWidth && a.canvasHeight == b.canvasHeight
                && a.shape == b.shape
        default: return false
        }
    }
}

extension ImageDocument {
    var canvasSelection: CanvasSelection? { selectionHistory.selection }
    var canReselect: Bool { selectionHistory.previous != nil }
    var quickMaskActive: Bool { selectionHistory.quickMaskActive }

    /// The selection command boundary shared by menus, canvas gestures and MCP.
    /// It deliberately never calls countEditChange: coverage is transient state.
    func setCanvasSelection(
        _ selection: CanvasSelection?, actionName: String = "Selection",
        record: [ActionStep] = .notACommand
    ) {
        guard let doc else { return }
        if let selection,
           selection.canvasWidth != doc.width || selection.canvasHeight != doc.height {
            NSSound.beep()
            return
        }
        var next = selectionHistory
        if !SelectionHistoryState.equivalent(next.selection, selection), let old = next.selection {
            next.previous = old
        }
        next.selection = selection
        next.quickMaskActive = false
        applySelectionState(next, actionName: actionName, record: record)
    }

    @discardableResult
    func reselect(record: [ActionStep] = .notACommand) -> Bool {
        guard let previous = selectionHistory.previous else { return false }
        setCanvasSelection(previous, actionName: "Reselect", record: record)
        return true
    }

    func setQuickMaskActive(_ active: Bool, record: [ActionStep] = .notACommand) {
        var next = selectionHistory
        next.quickMaskActive = active
        applySelectionState(next, actionName: active ? "Enter Quick Mask" : "Exit Quick Mask",
                            record: record)
    }

    func applyQuickMaskStroke(_ stroke: QuickMaskStroke, record: [ActionStep] = .notACommand) throws {
        guard let doc, stroke.canvasWidth == doc.width, stroke.canvasHeight == doc.height else {
            throw DoctoredCoreError(message: "Quick Mask stroke dimensions do not match the canvas.")
        }
        if doc.isHighDepth || canvasSelection?.hasNativeCoverage == true || stroke.native {
            let before = selectionHistory
            let owner = self.doc, capturedRevision = revision, target = planeEditTarget, layer = activeLayerIndex
            let capturedLayers = selectedLayerIndices, capturedView = channelVisibility
            _ = try RasterResources.shared.admit(scratch: UInt64(doc.width * doc.height) * 2 + 4096)
            let original = canvasSelection?.maskSamples16()
            let coverage = try stroke.applyingNative(to: original)
            guard self.doc === owner, revision == capturedRevision, planeEditTarget == target,
                  activeLayerIndex == layer, selectedLayerIndices == capturedLayers, channelVisibility == capturedView,
                  selectionHistory == before, !hasClosed else {
                throw DoctoredCoreError(message: "The image, target or selection changed during the Quick Mask stroke.")
            }
            guard original.map({ $0 != coverage }) ?? coverage.contains(where: { $0 != 0 }) else { return }
            var next = before
            next.selection = CanvasSelection(shape: .mask16(coverage), canvasWidth: doc.width, canvasHeight: doc.height)
            if let previous = before.selection { next.previous = previous }
            applySelectionState(next, actionName: "Quick Mask Stroke", record: record)
            return
        }
        let oldCoverage = canvasSelection?.maskBytes()
        let coverage = try stroke.applying(to: oldCoverage)
        // A canonical no-op must retain the geometric shape and Reselect state.
        // Empty coverage is nil selection, so avoid allocating its zero baseline.
        guard oldCoverage.map({ $0 != coverage }) ?? coverage.contains(where: { $0 != 0 }) else { return }
        var next = selectionHistory
        next.selection = CanvasSelection(shape: .mask(coverage), canvasWidth: doc.width,
                                         canvasHeight: doc.height)
        if !SelectionHistoryState.equivalent(next.selection, canvasSelection), let canvasSelection {
            next.previous = canvasSelection
        }
        applySelectionState(next, actionName: "Quick Mask Stroke", record: record)
    }

    /// Edit ▸ Fill… / ⌥⌫ / ⌘⌫ in Quick Mask and `fill_selection` there:
    /// the whole Quick Mask coverage filled as ONE file-clean "Fill" step.
    /// Native coverage (a 16-bit or 32-bit float document, or u16 samples on
    /// a byte one) fills exactly; otherwise the byte path. False when nothing
    /// changed.
    @discardableResult
    func applyQuickMaskFill(_ fill: QuickMaskFill, record: [ActionStep] = .notACommand) throws -> Bool {
        guard quickMaskActive else {
            throw DoctoredCoreError(message: "Quick Mask is not active; there is no Quick Mask to fill.")
        }
        let space = nsColorSpace
        return try applyQuickMaskCoverage({ doc in
            let width = doc.width, height = doc.height
            if doc.isHighDepth || self.canvasSelection?.hasNativeCoverage == true {
                _ = try RasterResources.shared.admit(scratch: UInt64(width * height) * 2 + 4096)
                let coverage = try fill.applyingNative(
                    to: self.canvasSelection?.maskSamples16(), width: width, height: height,
                    space: space)
                return CanvasSelection(
                    shape: .mask16(coverage), canvasWidth: width, canvasHeight: height)
            }
            let coverage = try fill.applying(
                to: self.canvasSelection?.maskBytes(), width: width, height: height, space: space)
            return CanvasSelection(shape: .mask(coverage), canvasWidth: width, canvasHeight: height)
        }, actionName: "Fill", record: record)
    }

    /// One edit of the whole Quick Mask coverage, committed as ONE file-clean
    /// step named `actionName` — the tail the Quick Mask fill, blur and
    /// gradient share. `compute` answers the coverage after the
    /// edit, nil for none; it may run a modal native job, so the document,
    /// its target and its selection are captured first and a change behind
    /// it refuses by name. False, with no step, when the coverage is sample
    /// for sample what it was (u16 samples when either side is native, bytes
    /// otherwise); with no selection before, all-zero coverage is that case
    /// and stays nil. The coverage before becomes Reselect's.
    @discardableResult
    func applyQuickMaskCoverage(
        _ compute: (RasterDocument) throws -> CanvasSelection?, actionName: String,
        record: [ActionStep]
    ) throws -> Bool {
        guard let doc else { throw DoctoredCoreError(message: "Document has no image") }
        let before = selectionHistory
        let owner = self.doc, capturedRevision = revision, target = planeEditTarget, layer = activeLayerIndex
        let capturedLayers = selectedLayerIndices, capturedView = channelVisibility
        let coverage = try compute(doc)
        guard self.doc === owner, revision == capturedRevision, planeEditTarget == target,
              activeLayerIndex == layer, selectedLayerIndices == capturedLayers, channelVisibility == capturedView,
              selectionHistory == before, !hasClosed else {
            throw DoctoredCoreError(message: "The image, target or selection changed during the Quick Mask edit.")
        }
        let changed: Bool
        switch (before.selection, coverage) {
        case (nil, nil): changed = false
        case (nil, let new?):
            // Empty coverage is nil selection, as the stroke path keeps it: an
            // edit that leaves an empty mask all zero changes nothing, so no
            // step registers and no all-zero mask is stored.
            changed = new.hasNativeCoverage
                ? new.maskSamples16().contains(where: { $0 != 0 })
                : new.maskBytes().contains(where: { $0 != 0 })
        case (_?, nil): changed = true
        case let (old?, new?):
            changed = old.hasNativeCoverage || new.hasNativeCoverage
                ? old.maskSamples16() != new.maskSamples16() : old.maskBytes() != new.maskBytes()
        }
        guard changed else { return false }
        var next = before
        next.selection = coverage
        if !SelectionHistoryState.equivalent(next.selection, before.selection), let previous = before.selection {
            next.previous = previous
        }
        applySelectionState(next, actionName: actionName, record: record)
        return true
    }

    private func applySelectionState(_ next: SelectionHistoryState, actionName: String,
                                     record: [ActionStep]) {
        guard next != selectionHistory else { return }
        let before = captureEditingState()
        selectionHistory = next
        ActionRecorder.shared.record(record, on: self)
        notifySelectionChanged()
        if let before { registerHistoryChange(from: before, actionName: actionName) }
    }

    func notifySelectionChanged() {
        NotificationCenter.default.post(name: .imageDocumentSelectionDidChange, object: self)
    }

    /// A geometry command invalidates coordinates as part of its own undo step.
    /// Restoring that document snapshot restores the exact corresponding coverage.
    func clearSelectionForGeometry() {
        selectionHistory = SelectionHistoryState()
    }
}
