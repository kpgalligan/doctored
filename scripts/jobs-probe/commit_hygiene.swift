import AppKit

// A selected-pixels Free Transform commit whose publication `applyEdit`
// refuses keeps its session and leaves no undo step, at 8 and 16 bits, through
// the shared `ImageDocument.publishPrepared`. Also timed: the cost of
// `withUndoGroupDiscardingEmpty`'s rebuild when the body records nothing.

func verifyFloatingCommitRefusal() throws {
    let name = TransformSelectedPixels.actionName
    let finishFirst = "Finish the current gesture or edit first. Nothing changed."
    // 8 bits.
    do {
        let document = try fixture(width: 64, height: 64)
        defer { document.close() }
        let controller = try editor(document)
        _ = try select(document, side: 16)
        guard let base = document.doc else { throw ProbeFailure(description: "Floating fixture") }

        // (a) The UI commit publishes one step.
        controller.freeTransform(nil)
        try require(controller.transformSession?.floating != nil, "(a) floating session open")
        controller.transformSession?.transform.translation = CGVector(dx: 7, dy: 0)
        try require(controller.commitTransformSession(), "(a) the commit answered true")
        try require(controller.transformSession == nil, "(a) the session closed")
        try require(document.undoManager?.undoActionName == name,
                    "(a) one \"\(name)\" step: \(document.undoManager?.undoActionName ?? "-")")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "(a) one undo returns to the base")

        // (b) An MCP writer commits the same session as one step.
        controller.freeTransform(nil)
        try require(controller.transformSession?.floating != nil, "(b) floating session open")
        controller.transformSession?.transform.translation = CGVector(dx: 7, dy: 0)
        _ = try agentOK("save_copy", document,
                        ["path": directory.appendingPathComponent("floating.rz").path])
        try require(controller.transformSession == nil, "(b) save_copy closed the session")
        try require(document.undoManager?.undoActionName == name,
                    "(b) one \"\(name)\" step: \(document.undoManager?.undoActionName ?? "-")")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "(b) one undo returns to the base")

        // (c) A publication `applyEdit` refuses (a live edit is open) answers
        // `.refused`, clears the prepared edit and leaves no step.
        let renamed = try rename(document)
        guard let moved = try renamed.withLayerName(0, "Moved") else {
            throw ProbeFailure(description: "(c) moved fixture")
        }
        let edit = PreparedDocumentEdit(moved)
        document.preparedEdit = edit
        document.beginLiveEdit()
        var outcome: PublishOutcome?
        let refused = SessionCommitRefusal.capturing {
            outcome = document.publishPrepared(edit, name: name, record: .notACommand,
                                               resultingSelection: document.selectionHistory)
            return outcome == .published
        }
        document.abandonLiveEdit(ownedBy: renamed)
        try require(!refused.committed && outcome == .refused,
                    "(c) the refused publication answered .refused: \(String(describing: outcome))")
        try require(refused.sentence == finishFirst,
                    "(c) the refusal's sentence: \(refused.sentence ?? "-")")
        try require(document.doc === renamed, "(c) the document is unchanged")
        try require(document.preparedEdit == nil, "(c) the prepared edit was cleared")
        try require(document.undoManager?.undoActionName == "Rename",
                    "(c) the next undo is the rename: \(document.undoManager?.undoActionName ?? "-")")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "(c) one undo returns to the base")
    }
    // 16 bits.
    do {
        let document = try precisionFixture(width: 64, height: 48)
        defer { document.close() }
        let controller = try editor(document)
        _ = try select(document, side: 16)
        guard let base = document.doc else { throw ProbeFailure(description: "16-bit fixture") }

        // (d) A commit refused with a live edit open keeps the session, adds
        // no step and names its reason.
        controller.freeTransform(nil)
        try require(controller.transformSession?.floating != nil
                    && controller.transformSession?.precisionGesture != nil,
                    "(d) 16-bit floating session open")
        controller.transformSession?.transform.translation = CGVector(dx: 7, dy: 0)
        document.beginLiveEdit()
        let refused = SessionCommitRefusal.capturing { controller.commitTransformSession() }
        document.abandonLiveEdit(ownedBy: base)
        try require(!refused.committed, "(d) the refused commit answered false")
        try require(controller.transformSession?.floating != nil, "(d) the session stayed open")
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "(d) no undo step was added")
        try require(refused.sentence != nil, "(d) the refusal named its reason")
        // The kept session still commits once the live edit is gone.
        try require(controller.commitTransformSession(), "(d) the kept session commits")
        try require(controller.transformSession == nil
                    && document.undoManager?.undoActionName == name,
                    "(d) one \"\(name)\" step: \(document.undoManager?.undoActionName ?? "-")")
        document.undoManager?.undo()
        try require(document.doc === base && document.undoManager?.canUndo != true,
                    "(d) one undo returns to the base")

        // (e) The shared helper at 16 bits: `.refused`, no step, cleared edit.
        guard let moved = try base.withLayerName(0, "Moved") else {
            throw ProbeFailure(description: "(e) moved fixture")
        }
        let edit = PreparedDocumentEdit(moved)
        document.preparedEdit = edit
        document.beginLiveEdit()
        var outcome: PublishOutcome?
        let helper = SessionCommitRefusal.capturing {
            outcome = document.publishPrepared(edit, name: name, record: .notACommand,
                                               resultingSelection: document.selectionHistory)
            return outcome == .published
        }
        document.abandonLiveEdit(ownedBy: base)
        try require(!helper.committed && outcome == .refused,
                    "(e) the helper answered .refused: \(String(describing: outcome))")
        try require(helper.sentence == finishFirst,
                    "(e) the refusal's sentence: \(helper.sentence ?? "-")")
        try require(document.doc === base && document.preparedEdit == nil
                    && document.undoManager?.canUndo != true,
                    "(e) the document is unchanged, the edit cleared and no step added")
        try require(document.undoManager?.canRedo == true, "(e) the redo tail survived")
    }
    print("PASS: floating commit refusal — the selected-pixels Free Transform publishes one step "
        + "from the UI and an MCP writer, a refused publication answers .refused with applyEdit's "
        + "sentence, clears the prepared edit and leaves no step, and a 16-bit commit refused "
        + "with a live edit open keeps its session and commits later")
}

private func milliseconds(_ start: UInt64) -> Double {
    Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000
}

/// 21 empty discarding groups: (median, max) in ms.
private func timeEmptyDiscards(_ document: ImageDocument) -> (median: Double, max: Double) {
    var samples: [Double] = []
    for _ in 0..<21 {
        let start = DispatchTime.now().uptimeNanoseconds
        document.withUndoGroupDiscardingEmpty {}
        samples.append(milliseconds(start))
    }
    samples.sort()
    return (samples[10], samples[20])
}

private func renameStep(_ document: ImageDocument, _ label: String, action: String = "Rename") {
    withUndoGroup(document.undoManager) {
        document.applyEdit(action, record: .notACommand, renderNeutral: true) {
            try $0.withLayerName(0, label)
        }
    }
}

func verifyDiscardEmptyGroupCost() throws {
    for n in [50, 500, 2000] {
        guard let image = RasterImage.from(rgba: [UInt8](repeating: 200, count: 64 * 64 * 4),
                                           width: 64, height: 64),
              let document = ImageDocument.makeUntitled(with: image) else {
            throw ProbeFailure(description: "Discard cost fixture")
        }
        defer { document.close() }
        document.undoManager?.groupsByEvent = false
        document.updateChangeCount(.changeCleared)
        document.undoManager?.removeAllActions()
        guard let manager = document.undoManager else {
            throw ProbeFailure(description: "Discard cost undo manager")
        }

        var edits: [Double] = []
        for i in 0..<n {
            let start = DispatchTime.now().uptimeNanoseconds
            renameStep(document, "L\(i)")
            edits.append(milliseconds(start))
        }
        let editMS = edits.suffix(10).reduce(0, +) / 10
        try require(document.history.entries.count == n + 1 && document.history.cursor == n,
                    "[n=\(n)] \(n) renames are \(n) transitions: "
                    + "\(document.history.entries.count) entries, cursor \(document.history.cursor)")

        // Tail 0: no redo tail.
        let tail0 = timeEmptyDiscards(document)
        try require(document.history.entries.count == n + 1 && document.history.cursor == n,
                    "[n=\(n)] empty discards kept the history")
        try require(manager.canUndo && !manager.canRedo && manager.undoActionName == "Rename",
                    "[n=\(n)] empty discards kept Undo \"Rename\" and added no Redo")
        try require(document.stepHistory(redo: false) && document.history.cursor == n - 1,
                    "[n=\(n)] one undo steps to \(n - 1)")
        try require(document.stepHistory(redo: true) && document.history.cursor == n,
                    "[n=\(n)] one redo steps back to \(n)")

        // Tail n/2: half the history is redo.
        let half = n / 2
        for _ in 0..<half {
            try require(document.stepHistory(redo: false), "[n=\(n)] stepping back")
        }
        let tail = timeEmptyDiscards(document)
        try require(manager.canRedo && document.history.cursor == n - half,
                    "[n=\(n)] empty discards kept the redo tail")
        try require(document.stepHistory(redo: true) && document.history.cursor == n - half + 1,
                    "[n=\(n)] one redo moves the cursor by exactly one")

        print(String(format: "INFO: discard empty group n=%d edit_ms=%.3f tail0 median=%.3f "
            + "max=%.3f tail=%d median=%.3f max=%.3f (ms)",
            n, editMS, tail0.median, tail0.max, half, tail.median, tail.max))

        guard n == 50 else { continue }
        while document.history.cursor < document.history.entries.count - 1 {
            try require(document.stepHistory(redo: true), "[n=50] redo to the top")
        }
        // Two renames in one body are one entry, undone together.
        let count = document.history.entries.count
        let preBody = document.doc
        document.withUndoGroupDiscardingEmpty {
            renameStep(document, "Body A")
            renameStep(document, "Body B")
        }
        try require(document.history.entries.count == count + 1
                    && document.doc?.layerInfo(0)?.name == "Body B",
                    "[n=50] a body of two renames adds exactly one entry")
        try require(document.stepHistory(redo: false) && document.doc === preBody,
                    "[n=50] one undo returns to the pre-body document")
        try require(document.stepHistory(redo: true), "[n=50] redo the two-rename body")
        // A refused (nil) edit then a rename is one entry.
        let second = document.history.entries.count
        document.withUndoGroupDiscardingEmpty {
            withUndoGroup(document.undoManager) {
                document.applyEdit("Nil", record: .notACommand) { _ in nil }
            }
            renameStep(document, "Body C")
        }
        try require(document.history.entries.count == second + 1
                    && document.doc?.layerInfo(0)?.name == "Body C",
                    "[n=50] a nil edit and a rename add exactly one entry")
        // A deferred group closed mid-body is reopened at the next
        // registration (the deferred group asks `groupingLevel == 0` alone).
        // NSUndoManager's `undo()` raises inside an open group rather than
        // closing it, so the probe closes it with `endUndoGrouping` itself.
        let third = document.history.entries.count
        let preThird = document.doc
        document.withUndoGroupDiscardingEmpty {
            renameStep(document, "Body D")
            manager.endUndoGrouping()
            renameStep(document, "Body E")
        }
        try require(manager.groupingLevel == 0 && document.doc?.layerInfo(0)?.name == "Body E",
                    "[n=50] the rename after the mid-body close landed and its group closed")
        try require(document.history.entries.count == third + 2
                    && manager.undoActionName == "Rename",
                    "[n=50] each side of the mid-body close is one entry")
        try require(document.stepHistory(redo: false)
                    && document.doc?.layerInfo(0)?.name == "Body D",
                    "[n=50] one undo returns to the first rename")
        try require(document.stepHistory(redo: false) && document.doc === preThird,
                    "[n=50] a second undo returns to the document before that body")
        // A registration that merges into a still-pending transition
        // (`history.record` answers nil) opens no deferred group, so it
        // leaves no empty Undo step, and Undo carries the merged name. The
        // deferred group is closed with the history suspended, so
        // `closeGroup` keeps the first rename's transition pending at level 0.
        let preFourth = document.doc
        let fourth = document.history.cursor
        var pendingAtLevel0 = false
        document.withUndoGroupDiscardingEmpty {
            renameStep(document, "Body F", action: "Merge First")
            document.history.suspended += 1
            manager.endUndoGrouping()
            document.history.suspended -= 1
            pendingAtLevel0 = document.history.pendingID != nil && manager.groupingLevel == 0
            renameStep(document, "Body G", action: "Merge Second")
        }
        try require(pendingAtLevel0, "[n=50] the first rename's transition was pending at level 0")
        let top = document.history.entries.last?.name
        try require(manager.groupingLevel == 0 && document.history.cursor == fourth + 1
                    && document.history.entries.count == fourth + 2
                    && document.doc?.layerInfo(0)?.name == "Body G" && top == "Merge Second",
                    "[n=50] the merged rename is one entry: cursor \(document.history.cursor), "
                    + "top \(top ?? "-")")
        try require(manager.undoActionName == top,
                    "[n=50] Undo is named for the merged entry: \(manager.undoActionName)")
        try require(document.stepHistory(redo: false) && document.doc === preFourth,
                    "[n=50] one undo returns to the document before the merging body "
                    + "(no empty step on top)")
    }
    print("PASS: discard empty group cost — empty discarding groups keep the history, Undo and "
        + "the redo tail at 50, 500 and 2000 entries, a body's registrations are one entry, and a "
        + "group closed mid-body is reopened, and a registration merged into a pending transition "
        + "leaves no empty step (timings above are information)")
}

/// The running history bound is never below a full measure.
private func requireRetainedBound(_ document: ImageDocument, _ label: String) throws {
    let bound = document.history.retainedBytes
    document.history.measure()
    let measured = document.history.retainedBytes
    try require(measured <= bound,
                "\(label): the running bound \(bound) is below the full measure \(measured)")
}

/// History bookkeeping per edit, per enforcement, per undo and per
/// eviction at 50, 500 and 2000 entries. Timings are information; the case
/// asserts the history and Undo stay exact.
func verifyHistoryBookkeepingCost() throws {
    for n in [50, 500, 2000] {
        guard let image = RasterImage.from(rgba: [UInt8](repeating: 200, count: 64 * 64 * 4),
                                           width: 64, height: 64),
              let document = ImageDocument.makeUntitled(with: image) else {
            throw ProbeFailure(description: "Bookkeeping cost fixture")
        }
        defer { document.close() }
        document.undoManager?.groupsByEvent = false
        document.updateChangeCount(.changeCleared)
        document.history = DocumentHistory(document: document, maximumTransitions: n)
        document.undoManager?.removeAllActions()
        guard let manager = document.undoManager else {
            throw ProbeFailure(description: "Bookkeeping cost undo manager")
        }
        let history = document.history

        var edits: [Double] = []
        for i in 0..<n {
            let start = DispatchTime.now().uptimeNanoseconds
            renameStep(document, "L\(i)")
            edits.append(milliseconds(start))
        }
        let editMS = edits.suffix(10).reduce(0, +) / 10
        try require(history.entries.count == n + 1 && history.cursor == n,
                    "[n=\(n)] \(n) renames are \(n) transitions: "
                    + "\(history.entries.count) entries, cursor \(history.cursor)")
        try requireRetainedBound(document, "[n=\(n)] after \(n) renames")

        var enforcements: [Double] = []
        for _ in 0..<21 {
            let start = DispatchTime.now().uptimeNanoseconds
            history.enforceBudget()
            enforcements.append(milliseconds(start))
        }
        enforcements.sort()
        let enforceMedian = enforcements[10]
        try require(history.entries.count == n + 1 && history.cursor == n,
                    "[n=\(n)] enforcement under the budget kept every entry")

        var steps: [Double] = []
        for redo in [false, true] {
            for _ in 0..<10 {
                let before = history.cursor
                let start = DispatchTime.now().uptimeNanoseconds
                let moved = document.stepHistory(redo: redo)
                steps.append(milliseconds(start))
                try require(moved && history.cursor == before + (redo ? 1 : -1),
                            "[n=\(n)] one \(redo ? "redo" : "undo") moves the cursor by one: "
                            + "\(before) → \(history.cursor)")
            }
        }
        let undoMS = steps.reduce(0, +) / Double(steps.count)

        // `cap_drop_ms`, timed outside the rename's interval, is
        // `dropHistoryUndo`'s full-stack walk for a target that owns no
        // registration (it removes nothing).
        var capped: [Double] = []
        var drops: [Double] = []
        for i in 0..<10 {
            let start = DispatchTime.now().uptimeNanoseconds
            renameStep(document, "Cap\(i)")
            capped.append(milliseconds(start))
            let unregistered = [HistoryUndoTarget(id: "probe-unregistered-\(i)")]
            let dropStart = DispatchTime.now().uptimeNanoseconds
            document.dropHistoryUndo(for: unregistered)
            drops.append(milliseconds(dropStart))
        }
        let capEditMS = capped.reduce(0, +) / 10
        let capDropMS = drops.reduce(0, +) / 10
        try require(history.entries.count == n + 1 && history.cursor == n
                    && history.evictedCount == 10,
                    "[n=\(n)] each rename at the cap evicts one entry: \(history.entries.count) "
                    + "entries, cursor \(history.cursor), evicted \(history.evictedCount)")
        try requireRetainedBound(document, "[n=\(n)] at the cap")

        print(String(format: "INFO: history bookkeeping n=%d edit_ms=%.3f enforce_median=%.3f "
            + "undo_ms=%.3f cap_edit_ms=%.3f cap_drop_ms=%.3f (ms)", n, editMS,
            enforceMedian, undoMS, capEditMS, capDropMS))

        guard n == 50 else { continue }
        for _ in 0..<n {
            let before = history.cursor
            try require(document.stepHistory(redo: false) && history.cursor == before - 1,
                        "[n=50] each undo steps back one to \(before - 1): \(history.cursor)")
        }
        try require(history.cursor == 0 && !manager.canUndo,
                    "[n=50] \(n) undos reach the retained boundary with nothing left to undo")
        for _ in 0..<n {
            let before = history.cursor
            try require(document.stepHistory(redo: true) && history.cursor == before + 1,
                        "[n=50] each redo steps forward one to \(before + 1): \(history.cursor)")
        }
        for k in 0..<5 {
            var mask = [UInt8](repeating: 0, count: 64 * 64)
            let side = 8 + 4 * k
            for y in 0..<side {
                for x in 0..<side { mask[y * 64 + x] = 255 }
            }
            document.setCanvasSelection(CanvasSelection(shape: .mask(mask), canvasWidth: 64,
                                                        canvasHeight: 64),
                                        actionName: "Mask \(k)")
        }
        try require(history.cursor == n && history.entries.count == n + 1,
                    "[n=50] five mask selections are five transitions at the cap")
        try requireRetainedBound(document, "[n=50] after five mask selections")
        while history.cursor > 1 {
            try require(document.stepHistory(redo: false), "[n=50] stepping back to 1")
        }
        try document.setHistoryBudget(1)
        try require(history.entries.count == 2 && history.cursor == 1 && !manager.canRedo,
                    "[n=50] a 1-byte budget keeps two entries and no redo: "
                    + "\(history.entries.count) entries, cursor \(history.cursor), "
                    + "canRedo \(manager.canRedo)")
        try require(document.stepHistory(redo: false) && history.cursor == 0 && !manager.canUndo,
                    "[n=50] one undo reaches the boundary with nothing left to undo")
        try require(document.stepHistory(redo: true) && history.cursor == 1,
                    "[n=50] one redo returns to 1")
    }

    // The running bound grows by a saturating difference: renames after
    // `refreshProjection` has populated a windowed document's caches complete
    // without a trap, and a later full measure equals the exact figure.
    let cached = try fixture(width: 64, height: 64)
    defer { cached.close() }
    _ = try editor(cached)
    withUndoGroup(cached.undoManager) {
        cached.applyEdit("Cache", record: .notACommand) { try $0.withLayerName(0, "Cache") }
    }
    for i in 0..<5 { renameStep(cached, "Cached \(i)") }
    try require(cached.history.cursor == 6, "Cached renames: cursor \(cached.history.cursor)")
    guard let current = cached.captureEditingState() else {
        throw ProbeFailure(description: "Cached fixture state")
    }
    let exact = try EditingState.retainedBytes(
        in: cached.history.entries.map(\.state) + [current]
            + (cached.historySavedState.map { [$0] } ?? []))
    try requireRetainedBound(cached, "Cached renames")
    try require(cached.history.retainedBytes == exact,
                "Cached renames: the full measure \(cached.history.retainedBytes) equals the "
                + "exact figure \(exact)")
    print("PASS: history bookkeeping cost — at 50, 500 and 2000 entries each undo and redo moves "
        + "one step, each rename at the cap evicts one entry, the running byte bound is never "
        + "below a full measure, and a 1-byte budget leaves one reversible step (timings "
        + "above are information)")
}
