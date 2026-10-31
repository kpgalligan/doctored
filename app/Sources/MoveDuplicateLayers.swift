import AppKit

// `RasterDocument.duplicatingAndMoving` is the one body (8 bits: `applyEdit`/`editStructure`; the
// drag duplicates on main into its live edit on the first non-zero tick, then moves the copies by
// the step owed), and `PrecisionGeometryRequest.duplicateMove` is the typed gesture's (planned as
// the move). The lock rule: duplicating never refuses; moving the copies is the originals' SUBTREE
// Position block (`refuseLockedEdit(layers: MoveDuplicateLayers.subtrees…)`, MCP
// `rejectLockedOffset` per index for a non-zero delta), never `movingSet` — the copies are
// unlinked, so a link partner's lock never refuses. `moveRecord(…, duplicate: true)` records
// `move_layers` with `"duplicate": true`, sized by the subtrees. MCP `move_layers duplicate` is the
// twin (a zero delta still duplicates; the reply adds `copies`). A duplicate snaps to its SOURCE:
// `MoveDuplicateLayers.snapExclusion` is `DragSnapping`'s exclusion for a duplicate drag
// and a duplicate ⌥⌘T box — the copies' subtrees and their ancestors while the 8-bit live edit
// holds them, otherwise only the originals' ancestors, never the originals. At 16/32 bits
// `layerBounds` is the typed content box (`dr_precision_doc_layer_bounds`), so the snap boxes
// are the same at every depth.

/// The Move tool's ⌥ duplicate with NO pixel selection: ⌥-drag and
/// ⌥-arrow duplicate the selected layers and move the
/// COPIES, leaving the originals where they were. `move_layers` with
/// `duplicate` is the MCP twin. The controller half is
/// `EditorViewController+MoveDuplicate.swift`.
///
/// Decisions stated here and in the feature reference:
/// - the copies are `duplicateLayers`' own — unlinked, each landing directly
///   above its source's subtree in the same group, keeping its locks — so the
///   gesture is ONE "Duplicate Layer" step (the Layer ▸ Duplicate Layer name);
/// - duplicating is never lock-refused (Layer ▸ Duplicate Layer is not), but
///   moving the copies is exactly the POSITION block the originals' SUBTREES
///   report (an original's own Position bit, an enclosing group's, or a group
///   original's descendant's), so that is the one check. Never `movingSet`:
///   the copies are unlinked, so a link partner's lock never refuses;
/// - a zero delta still duplicates, in place; the copies become the layer
///   selection after the edit publishes.
struct MoveDuplicateSession {
    /// The document at the press; `copies` and the selection derive from it.
    let base: RasterDocument
    /// The selected layers at the press, ascending.
    let originals: [Int]
    /// The active layer at the press; its copy becomes the primary.
    let primary: Int
    /// Where the copies landed, set on the first non-zero tick.
    var copies: [Int]? = nil
    /// The 8-bit duplicate failed on the first tick; later ticks do nothing.
    var refused = false
}

enum MoveDuplicateLayers {
    static let actionName = "Duplicate Layer"

    /// The union of the entries' subtrees, ascending: what a duplicate copies,
    /// and the set its POSITION check and its recorded step are sized by.
    static func subtrees(of indices: [Int], in doc: RasterDocument) -> [Int] {
        let tree = doc.layerTree
        var set = Set<Int>()
        for idx in indices where idx >= 0 && idx < doc.layerCount {
            set.formUnion(tree.subtree(of: idx))
        }
        return set.sorted()
    }

    /// Where `duplicateLayers(indices)`' copies land, read from the stack
    /// BEFORE the edit.
    static func copies(of indices: [Int], in doc: RasterDocument) -> [Int] {
        doc.layerTree.duplicateLandings(indices)
    }

    /// The layer selection after the duplicate: every copy, with the copy that
    /// carries `primary` as the active layer. `doc` is the stack before the edit.
    static func selection(copies: [Int], primary: Int, originals: [Int],
                          in doc: RasterDocument) -> LayerSelection {
        let main = doc.layerTree.duplicateLanding(of: primary, in: originals)
            ?? copies.last ?? primary
        return LayerSelection(primary: main, others: Set(copies))
    }

    /// The entries of `doc` a duplicate's drag or box must NOT snap to:
    /// a duplicate snaps to its SOURCE, so the originals themselves are never
    /// excluded.
    /// - `copies` set and `doc` not `base` (the 8-bit drag's live edit holds
    ///   the copies): the copies' subtrees plus their enclosing groups;
    /// - otherwise (the 16-bit drag, the 8-bit press before its first
    ///   duplicating tick, every ⌥⌘T session): only the originals' enclosing
    ///   groups, whose folded box contains the original.
    /// Out-of-range indices contribute nothing.
    static func snapExclusion(originals: [Int], copies: [Int]?, base: RasterDocument,
                              in doc: RasterDocument) -> Set<Int> {
        let tree = doc.layerTree
        let count = doc.layerCount
        var excluded = Set<Int>()
        if let copies, doc !== base {
            for copy in copies where copy >= 0 && copy < count {
                for index in tree.subtree(of: copy) {
                    excluded.insert(index)
                    excluded.formUnion(tree.ancestors(of: index))
                }
            }
        } else {
            for original in originals where original >= 0 && original < count {
                excluded.formUnion(tree.ancestors(of: original))
            }
        }
        return excluded
    }
}

extension RasterDocument {
    /// `duplicateLayers(indices)`, then the copies moved by (dx, dy) as one
    /// set (typed: `precisionMoveLayers(…, control:)`, safe on a worker). A
    /// zero delta or a move that answers nil keeps the duplicate; nil only
    /// when `duplicateLayers` answers nil.
    func duplicatingAndMoving(_ indices: [Int], dx: Int, dy: Int,
                              control: RasterJobControl? = nil)
        throws -> (document: RasterDocument, copies: [Int])?
    {
        let copies = MoveDuplicateLayers.copies(of: indices, in: self)
        guard let duplicated = try duplicateLayers(indices) else { return nil }
        guard dx != 0 || dy != 0 else { return (duplicated, copies) }
        let moved = isHighDepth
            ? try duplicated.precisionMoveLayers(copies, dx: dx, dy: dy, control: control)
            : try duplicated.moveLayers(copies, dx: dx, dy: dy)
        return (moved ?? duplicated, copies)
    }
}
