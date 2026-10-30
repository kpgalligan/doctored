import AppKit

// The Move tool's persistent float. Pixels moved by the Move tool stay floating until
// something commits them, so consecutive drags and nudges of one selection move ONE lift and the
// pixels the float passes over come back. Not measured: the scripted take could not observe a
// float (`move_float_202_take.py`'s RESULT).
//
// The float is session state, never persisted and never undone itself. Every step lifts from
// `source` (the document before the first lift) and moves by the float's TOTAL delta through the
// same `move_selected` request, so a step is still one ordinary "Move" publication and Undo walks
// back one drag or nudge at a time. Commit is only ever dropping the float: each step's result
// already holds the composite. It is validated by handle identity (`FadeCandidate`'s pattern): a
// Move continues it only while the document and the selection are a state one of its steps left.
// Any other edit, a filter, an adjustment, Deselect or a selection edit replaces one of those, so
// they commit with no code here. A layer switch (`ImageDocument.layerSelection`) and Save
// (`updateChangeCount(.changeCleared)`) drop it explicitly. Undo and redo keep it
// (`history.navigating`), so redo back onto a step continues the float. Esc cancels only the drag.

/// One Move float: what every step lifts from and the states its steps left.
struct MoveFloat {
    /// A state one step left: the published document (weak; the history
    /// holds it) and the selection it published, at the float's total delta.
    struct Position {
        weak var document: RasterDocument?
        let selection: SelectionHistoryState
        let dx: Int
        let dy: Int
    }

    /// What every step lifts from: the document before the first lift (at 8
    /// bits with the layer's description dropped). The one strong handle the
    /// float pins, as `FadeCandidate` pins its pair.
    let source: RasterDocument
    let layer: Int
    let target: PaintTarget
    /// A plane target's vacated-area colour, latched at the lift.
    let background: PlaneBackground?
    /// The lift was an ⌥ duplicate: every later step moves the copy and
    /// leaves the original.
    let duplicate: Bool
    /// The lift selection's coverage and integral bounds.
    let coverage: [UInt16]
    let bounds: CGRect
    /// The selection history at the lift.
    let origin: SelectionHistoryState
    /// [0] is the state before the lift at (0, 0).
    private(set) var positions: [Position]

    init(source: RasterDocument, before: RasterDocument, selection: SelectionHistoryState,
         layer: Int, target: PaintTarget, background: PlaneBackground?, duplicate: Bool,
         coverage: [UInt16], bounds: CGRect) {
        self.source = source
        self.layer = layer
        self.target = target
        self.background = background
        self.duplicate = duplicate
        self.coverage = coverage
        self.bounds = bounds
        origin = selection
        positions = [Position(document: before, selection: selection, dx: 0, dy: 0)]
    }

    /// The core request for the float at the TOTAL delta (dx, dy) from the
    /// lift, run against `source` with `coverage`.
    func command(dx: Int, dy: Int) throws -> PrecisionPixelCommand {
        try MoveSelectedPixels.command(layer: layer, dx: dx, dy: dy, duplicate: duplicate,
                                       bounds: bounds, target: target, background: background)
    }

    /// The selection at the TOTAL delta: the lift selection translated, so a
    /// mask that went partly off the canvas comes back whole; the selection
    /// before this step kept for Reselect when it differs, Quick Mask off.
    func resultingSelection(from current: SelectionHistoryState, dx: Int,
                            dy: Int) -> SelectionHistoryState {
        var next = current
        let moved = origin.selection?.translated(dx: dx, dy: dy)
        if !SelectionHistoryState.equivalent(moved, current.selection), let old = current.selection {
            next.previous = old
        }
        next.selection = moved
        next.quickMaskActive = false
        return next
    }

    /// Whether `document` is a state one of the float's steps left (or the
    /// state before its lift).
    func holds(_ document: RasterDocument?) -> Bool {
        guard let document else { return false }
        return positions.contains { $0.document === document }
    }

    /// The float after a step from `positions[index]`: the later positions
    /// (a redo branch the step replaces) go, and the step's state is appended.
    func advanced(from index: Int, to document: RasterDocument, selection: SelectionHistoryState,
                  dx: Int, dy: Int) -> MoveFloat {
        var next = self
        next.positions = Array(positions.prefix(index + 1))
        next.positions.append(Position(document: document, selection: selection, dx: dx, dy: dy))
        return next
    }
}

extension ImageDocument {
    /// The float a Move of `layer`'s `target` continues, and the position it
    /// continues from: the document and the selection are a state one of its
    /// steps left (index ≥ 1; the latest match), `layer` is the active layer,
    /// `target` the paint target, no Quick Mask, and nothing is mid-gesture or
    /// publishing. nil otherwise: the Move lifts afresh.
    func liveMoveFloat(layer: Int, target: PaintTarget) -> (float: MoveFloat, at: Int)? {
        guard let float = moveFloat, let current = doc, float.layer == layer,
              activeLayerIndex == layer, planeEditTarget == target, float.target == target,
              !quickMaskActive, !isLiveEditing, !isPublishingEditingState else { return nil }
        guard let index = float.positions.indices.last(where: {
            $0 >= 1 && float.positions[$0].document === current
                && float.positions[$0].selection == selectionHistory
        }) else { return nil }
        return (float, index)
    }

    /// After a successful publication: the float advanced from `index` to the
    /// published document and selection at the TOTAL delta (dx, dy).
    func noteMoveFloat(_ float: MoveFloat, from index: Int, dx: Int, dy: Int) {
        guard let current = doc else { return }
        moveFloat = float.advanced(from: index, to: current, selection: selectionHistory,
                                   dx: dx, dy: dy)
    }

    /// The step after a Move publication: nothing when it published nothing
    /// (`doc` is still `base`); else the continued float advances, or a fresh
    /// lift (`lift`) starts one from its position 0.
    func noteMoveStep(base: RasterDocument, continuing: (float: MoveFloat, at: Int)?,
                      lift: () -> MoveFloat, dx: Int, dy: Int) {
        guard doc !== base else { return }
        if let continuing {
            noteMoveFloat(continuing.float, from: continuing.at, dx: dx, dy: dy)
        } else {
            noteMoveFloat(lift(), from: 0, dx: dx, dy: dy)
        }
    }
}
