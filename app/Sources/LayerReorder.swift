import Foundation

// In `LayerReorder`'s drag chain, each move is expressed in the numbering the previous one
// produced, so a second implementation would disagree silently.

/// The multi-entry reorder arithmetic, as pure statics: what a panel drag
/// of several rows does to a document, and where each dragged entry lands.
///
/// It moved out of `LayersPanelViewController+Rows.swift` unchanged because
/// it now has a SECOND caller. `reorder_layers` — the agent twin the panel's
/// multi-row drag never had — has to compute exactly the same chain, and the
/// chain is not something either side can approximate: **each move is
/// expressed in the numbering the PREVIOUS move produced**, so every
/// remaining source, every landing already recorded and the destination group
/// are remapped after each step. A second implementation of that would agree
/// with the first for the easy cases and disagree for the ones people
/// actually hit (a group dragged past its own children, a non-contiguous
/// selection crossing a level boundary), and the disagreement would be
/// silent — the layers would simply be in the wrong order.
///
/// Nothing here touches AppKit, a view controller or a document: it takes a
/// `RasterDocument` and answers a new one, which is what lets the MCP handler
/// call it off the panel entirely.
enum LayerReorder {
    /// The whole drag as ONE new document, plus where each dragged entry
    /// landed (parallel to `sources`) and where the group it was dropped into
    /// ended up.
    ///
    /// `drop.at` is an insertion index in the PRE-move numbering — the line
    /// the panel drew, or the `at` a caller passed — and `drop.depth` the
    /// level everything lands at. nil when any move is refused, which is the
    /// core's answer for a (destination, depth) pair that would not leave a
    /// well-formed tree, and for a drop INSIDE one of the moving subtrees.
    static func reorder(
        _ doc: RasterDocument, sources: [Int], drop: (at: Int, depth: Int, into: Int?)
    ) throws -> (document: RasterDocument, landed: [Int], into: Int?)? {
        var updated = doc
        var pending = sources
        var landed: [Int] = []
        var into = drop.into
        var anchor = drop.at
        while !pending.isEmpty {
            let from = pending.removeFirst()
            guard let bounds = updated.layerSubtree(from) else { return nil }
            let subtree = bounds.start..<bounds.end
            guard anchor <= subtree.lowerBound || anchor >= subtree.upperBound else { return nil }
            // `to` indexes the stack with the moved subtree ALREADY taken
            // out — the core's own remove-then-insert convention.
            let to = anchor <= subtree.lowerBound ? anchor : anchor - subtree.count
            // The entry is the LAST record of its own subtree, so it lands at
            // the far end of the block just spliced in, and the next entry
            // goes immediately above it — which is what preserves the dragged
            // set's order.
            let landing = to + subtree.count - 1
            // The core refuses a move that moves NOTHING (an identical
            // document would mint a phantom undo step), and `to` equal to the
            // subtree's own start at its own depth is exactly that move. One
            // member of a multi-entry drag being already in place must not
            // refuse the whole drag, so it is skipped: every remap below is
            // the identity for it, and `landing` is where it already sits.
            if to == subtree.lowerBound, updated.layerDepth(from) == drop.depth {
                landed.append(landing)
                anchor = landing + 1
                continue
            }
            guard let next = try updated.moveLayerTo(from: from, to: to, depth: drop.depth)
            else { return nil }
            pending = pending.map { moved($0, subtree: subtree, to: to) }
            landed = landed.map { moved($0, subtree: subtree, to: to) }
            into = into.map { moved($0, subtree: subtree, to: to) }
            landed.append(landing)
            anchor = landing + 1
            updated = next
        }
        return (updated, landed, into)
    }

    /// Where index `i` ends up after a `moveLayerTo` that relocated the
    /// half-open range `subtree` to post-removal index `to` — the arithmetic
    /// the core itself performs (drain the block, splice it back in),
    /// mirrored here because each move renumbers the indices the NEXT move is
    /// expressed in.
    static func moved(_ i: Int, subtree: Range<Int>, to: Int) -> Int {
        if subtree.contains(i) { return to + (i - subtree.lowerBound) }
        let removed = i >= subtree.upperBound ? i - subtree.count : i
        return removed >= to ? removed + subtree.count : removed
    }
}
