import AppKit

// ⌥+arrow (`moveDuplicateNudge`, one "Duplicate Layer" step per press) and ⌥-drag
// (`MoveDuplicateSession` in the frozen controller's `moveDuplicate`, begun in `moveDidBegin`,
// ticked from `moveDidUpdate`, published by `finishMoveDuplicate` from `moveDidFinish`; cleared
// there, in `moveDidCancel` and in `releaseDisplayForWindowClose`) duplicate the selected layers
// and move the COPIES, which become the layer selection only after the edit publishes.

/// The Move tool's ⌥ duplicate with no pixel selection: the
/// controller half. The model, the lock rule and the decisions live in
/// `MoveDuplicateLayers.swift`.
///
/// ⌥+arrow: one "Duplicate Layer" step per press — `duplicatingAndMoving` in
/// `applyEdit` at 8 bits, the typed gesture's `.duplicateMove` at 16.
///
/// ⌥-drag: the press latches a `MoveDuplicateSession` and opens the drag as a
/// plain Move does (a live edit at 8 bits, `precisionMoveGesture` at 16).
/// Nothing is copied until the first non-zero tick. At 8 bits that tick
/// duplicates on main into the live edit and every tick then moves the copies
/// by the step still owed (`moveAppliedDelta`, the plain drag's rule); at 16
/// bits every tick asks the gesture for the TOTAL `.duplicateMove` from the
/// captured source. The release publishes ONE "Duplicate Layer" step and only
/// then selects the copies.
extension EditorViewController {
    /// ⌥+arrow with no pixel selection: duplicate the selected layers and
    /// move the copies by (dx, dy).
    func moveDuplicateNudge(_ dx: Int, _ dy: Int) {
        guard let document, let doc = document.doc else { return }
        let indices = document.selectedLayerIndices
        let primary = document.activeLayerIndex
        guard !refuseLockedEdit(layers: MoveDuplicateLayers.subtrees(of: indices, in: doc),
                                kind: DR_EDIT_POSITION)
        else { return }
        let name = MoveDuplicateLayers.actionName
        let record = Self.moveRecord(doc, indices, dx: dx, dy: dy, duplicate: true)
        if doc.isHighDepth {
            do {
                let gesture = try PrecisionGeometryGesture(document)
                defer { gesture.cancel() }
                try gesture.finish(.duplicateMove(layers: indices, dx: dx, dy: dy),
                                   name: name, record: record)
            } catch {
                document.presentError(error)
                return
            }
        } else {
            document.applyEdit(name, record: record) {
                try $0.duplicatingAndMoving(indices, dx: dx, dy: dy)?.document
            }
        }
        guard document.doc !== doc else { return }
        setSelectedLayers(MoveDuplicateLayers.selection(
            copies: MoveDuplicateLayers.copies(of: indices, in: doc), primary: primary,
            originals: indices, in: doc))
    }

    /// The ⌥ press with no pixel selection: captures the originals. The
    /// caller has run the POSITION check and opens the live edit or gesture.
    func beginMoveDuplicate(_ document: ImageDocument) {
        guard let doc = document.doc else { return }
        moveDuplicate = MoveDuplicateSession(
            base: doc, originals: document.selectedLayerIndices,
            primary: document.activeLayerIndex)
    }

    /// One drag tick; `dx`, `dy` are the snapped TOTAL from the press.
    func updateMoveDuplicate(dx: Int, dy: Int) {
        guard var session = moveDuplicate, !session.refused, let document,
              let doc = document.doc else { return }
        if doc.isHighDepth {
            // Every tick: the release commits the last total asked for.
            moveAppliedDelta = (dx, dy)
            guard let gesture = precisionMoveGesture else { return }
            if session.copies == nil {
                guard dx != 0 || dy != 0 else { return }
                session.copies = MoveDuplicateLayers.copies(of: session.originals, in: session.base)
                moveDuplicate = session
            }
            gesture.request(.duplicateMove(layers: session.originals, dx: dx, dy: dy))
            return
        }
        guard let applied = moveAppliedDelta else { return }
        var current = doc
        if session.copies == nil {
            guard dx != 0 || dy != 0 else { return }
            let duplicated: RasterDocument?
            do { duplicated = try doc.duplicateLayers(session.originals) } catch { duplicated = nil }
            guard let duplicated else {
                session.refused = true
                moveDuplicate = session
                NSSound.beep()
                return
            }
            document.updateLiveEdit(duplicated)
            // A refused live update has already said why.
            guard document.doc === duplicated else {
                session.refused = true
                moveDuplicate = session
                return
            }
            session.copies = MoveDuplicateLayers.copies(of: session.originals, in: session.base)
            moveDuplicate = session
            current = duplicated
        }
        guard let copies = session.copies else { return }
        let stepX = dx - applied.x
        let stepY = dy - applied.y
        guard stepX != 0 || stepY != 0 else { return }
        // Byte-only here, so nothing is thrown; `try?` only satisfies the
        // throwing facade (the plain drag's rule). A tick does not re-render a
        // pattern fill; the release refits once (`refitLiveMove`).
        guard let moved = try? current.moveLayers(copies, dx: stepX, dy: stepY, refit: false)
        else { return }
        moveAppliedDelta = (dx, dy)
        document.updateLiveEdit(moved)
    }

    /// An 8-bit Move drag's release: the pattern fills its ticks moved
    /// re-rendered over the canvas (FillLayer.swift's PIN) as one more live
    /// update, so `endLiveEdit` makes one undo step. A refused re-render puts
    /// the pre-gesture document back and answers false.
    func refitLiveMove(_ document: ImageDocument) -> Bool {
        guard let current = document.doc else { return true }
        do {
            let refit = try current.refittingFillLayers()
            if refit !== current { document.updateLiveEdit(refit) }
            return true
        } catch {
            document.cancelLiveEdit()
            document.presentError(error)
            return false
        }
    }

    /// The release (or a leftover session at the next press): ONE "Duplicate
    /// Layer" step, then the copies selected. A click, a refusal or a failed
    /// duplicate publishes nothing.
    func finishMoveDuplicate() {
        guard let session = moveDuplicate else { return }
        moveDuplicate = nil
        let applied = moveAppliedDelta ?? (0, 0)
        moveAppliedDelta = nil
        moveDidEnd()
        let name = MoveDuplicateLayers.actionName
        let record = Self.moveRecord(session.base, session.originals,
                                     dx: applied.x, dy: applied.y, duplicate: true)
        var published = false
        if let gesture = precisionMoveGesture {
            precisionMoveGesture = nil
            canvas.precisionMoveActive = false
            defer {
                gesture.cancel()
                canvas.previewImage = nil
            }
            if session.copies != nil, !session.refused, let document {
                do {
                    published = try gesture.finish(
                        .duplicateMove(layers: session.originals, dx: applied.x, dy: applied.y),
                        name: name, record: record)
                } catch { document.presentError(error) }
            }
        } else if let document, document.doc?.isHighDepth != true {
            guard refitLiveMove(document) else { return }
            document.endLiveEdit(session.copies == nil ? "Move Layer" : name, record: record)
            published = session.copies != nil && document.doc !== session.base
        }
        guard published, let copies = session.copies else { return }
        setSelectedLayers(MoveDuplicateLayers.selection(
            copies: copies, primary: session.primary, originals: session.originals,
            in: session.base))
    }
}
