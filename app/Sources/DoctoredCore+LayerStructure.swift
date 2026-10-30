import AppKit

// **Phase 12E-1 made the four pure STRUCTURE tools typed the same way as `set_layer_clipped`:**
// `RasterDocument.moveLayerTo` / `arrangeLayer` / `removeLayers` / `removingLayer` / `ungroupLayer`
// / `groupLayers` route to the core's own typed op on their FIRST line, so every existing caller —
// `LayerReorder.reorder` behind the panel drag and `reorder_layers`, `AgentServer+Groups`,
// `AgentServer+LayerProperties`, `AgentServer+LayerSets`, `EditorViewController+Arrange`, Delete
// Layer and the Action replays of all of them (`arrange_layer`'s only since it joined `typedTools`,
// `FloatDocument.tools` and both `families` maps as `layer_move` — until then MCP and a recorded
// Arrange were refused at depth while the menu items worked) — works at 16 bits and at 32-bit float
// with no typed branch of its own, and `PrecisionGrouping.swift`'s "every top-level layer exactly
// once" stopgap is gone. Those six facades (and `withLayerLabel`, `LayerReorder.reorder`) `throw`
// the core's sentence instead of swallowing it, and so do `addingLayer` (both forms),
// `mergingDown`, `mergeLayers`, `mergeVisible`, `stampVisible`, `flattening`, `withLayerLocks` /
// `withLockFlags`, `linkLayers`, `unlinkLayers`, `withLayerName`, `withLayerVisible`,
// `withLayerClipped` and the channel edits `removingChannel`, `renamingChannel`,
// `settingChannelOverlay`, `duplicatingChannel` and `invertingChannel`, and so do
// `withLayerOpacity`, `withLayerBlendMode`, `duplicatingLayer`, `duplicateLayers`, `moveLayers`,
// `transformLayers`, `rotated90/180/270`, `flippedH/V`, `resized`, `transformingLayer`,
// `perspectiveLayer`, `withLayerMaskEnabled`, `settingMaskProperties`, `transformingMask`,
// `addingChannel` and the three profile facades `assigningProfile`, `convertingToProfile` and
// `assigningProfileRasterizing`, and so do `withLayerOffset`, `removingLayerMask`, `cropped`,
// `canvasResized`, `settingMetadata` and `settingResolution` (the byte branches are unchanged;
// `hidingLayer` keeps its non-throwing signature through an internal `try?`, and so do the
// byte-only live ticks, the non-throwing byte openers and `AuxiliaryMattes`' skip-and-continue,
// each with a comment).
//
// **Phase 12E-1b did the same for merge, locks/links and PPI:** `mergingDown` / `mergeLayers` /
// `mergeVisible` / `withLayerLocks` / `linkLayers` / `unlinkLayers` route to the typed request on
// their first line, `layerLocks` / `layerLink` read the parsed `locks` / `link` metadata rows and
// `lockBlocking` routes to `dr_precision_doc_lock_block`, so `EditorViewController.mergeDown`
// (frozen), `EditorViewController+Locks`, `EditorViewController+Arrange`'s link items and the
// `AgentServer+Groups` handlers all work at depth unchanged; `set_resolution` joins `commonTools`
// and `FloatDocument.tools` and the Image Size sheet's three typed-source branches are gone. Two
// host pre-checks were ADDED, both reusing the existing wording rather than inventing any:
// `AgentServer+PrecisionPaint` and `AgentServer+PrecisionScalars` call `rejectLockedEdit` (the
// typed paint and mask paths never reach `AgentServer.performPixelEdit`, which is where the byte
// one lives), so a locked layer answers `LockFlags.refusal`'s sentence at both depths instead of
// the core's backstop. And `RasterDocument.moveLayers`' typed branch expands through `movingSet`
// before it transforms, because `dr_doc_move_layers` does that expansion INSIDE the byte core and
// the typed transform takes the set it is given — without it a linked partner stays behind at
// depth.

extension RasterDocument {
    // MARK: - Layer groups, locks, links and structure

    /// The half-open range of entry `idx`'s subtree: the entry alone for a
    /// raster layer, the whole group for a group. nil for an out-of-range
    /// index.
    func layerSubtree(_ idx: Int) -> (start: Int, end: Int)? {
        if let precisionMetadata { return precisionMetadata.layer(idx).map { ($0.subtreeStart, idx + 1) } }
        guard isValidIndex(idx) else { return nil }
        var start = 0
        var end = 0
        guard dr_doc_layer_subtree(ptr, idx, &start, &end) else { return nil }
        return (start, end)
    }

    /// Where a new entry inserted ABOVE `idx` lands — the ONE answer every
    /// "select what I just made" site asks for, because `idx + 1` is the
    /// wrong index the moment `idx` names a group. Falls back to `idx + 1`
    /// only for an index the core does not recognize, where nothing was
    /// inserted anyway.
    func insertionIndex(above idx: Int) -> Int {
        layerSubtree(idx)?.end ?? idx + 1
    }

    /// True when entry `idx` is a GROUP.
    func layerIsGroup(_ idx: Int) -> Bool {
        if let precisionMetadata { return precisionMetadata.layer(idx)?.info.isGroup ?? false }
        guard isValidIndex(idx) else { return false }
        return dr_doc_layer_is_group(ptr, idx)
    }

    /// Entry `idx`'s nesting depth; 0 at the top level.
    func layerDepth(_ idx: Int) -> Int {
        if let precisionMetadata { return precisionMetadata.layer(idx)?.info.depth ?? 0 }
        guard isValidIndex(idx) else { return 0 }
        return Int(dr_doc_layer_depth(ptr, idx))
    }

    /// Entry `idx`'s lock flags as the raw `DrLockFlags` bitmask.
    func layerLocks(_ idx: Int) -> UInt32 {
        if let precisionMetadata { return precisionMetadata.layer(idx)?.info.locks ?? 0 }
        guard isValidIndex(idx) else { return 0 }
        return dr_doc_layer_locks(ptr, idx)
    }

    func withLayerLocks(_ idx: Int, _ locks: UInt32) throws -> RasterDocument? {
        if isHighDepth { return try precisionLayerLock(idx, locks: locks) }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_with_layer_locks(ptr, idx, locks))
    }

    /// Which of entry `idx`'s lock bits would block an edit of `kind`
    /// (0 = allowed) — what a refusal alert reads to NAME the lock. The core
    /// ops refuse regardless, so a path that forgets to ask still cannot
    /// write.
    func lockBlocking(_ idx: Int, kind: DrEditKind) -> UInt32 {
        if let precision { return precision.lockBlocking(idx, kind: kind) }
        guard isValidIndex(idx) else { return 0 }
        return dr_doc_lock_block(ptr, idx, kind)
    }

    /// The entry whose OWN lock bits hold every bit of `locks` — `idx`
    /// itself, else its nearest enclosing group (a group's locks cover
    /// its contents). nil when none does. A refusal names this entry.
    func lockOwner(_ idx: Int, locks: UInt32) -> Int? {
        if let precision { return precision.lockOwner(idx, locks: locks) }
        guard isValidIndex(idx) else { return nil }
        var owner = 0
        return dr_doc_lock_owner(ptr, idx, locks, &owner) ? owner : nil
    }

    /// Entry `idx`'s link-group id; 0 when unlinked.
    func layerLink(_ idx: Int) -> UInt32 {
        if let precisionMetadata { return precisionMetadata.layer(idx)?.info.link ?? 0 }
        guard isValidIndex(idx) else { return 0 }
        return dr_doc_layer_link(ptr, idx)
    }

    /// Whether GROUP `idx` is shown expanded.
    func layerOpen(_ idx: Int) -> Bool {
        if let precisionMetadata { return precisionMetadata.layer(idx)?.info.open ?? false }
        guard isValidIndex(idx) else { return false }
        return dr_doc_layer_open(ptr, idx)
    }

    /// Expands or collapses GROUP `idx`; nil on a non-group, an out-of-range
    /// index or the state it already has (the core's purity rule). At 16/32
    /// bits the disclosure is the typed `layer_open` bookkeeping edit,
    /// which throws only while background work holds the document.
    func withLayerOpen(_ idx: Int, _ open: Bool) throws -> RasterDocument? {
        if isHighDepth {
            guard layerIsGroup(idx) else { return nil }
            return try applyingPrecisionBookkeepingEdit(.init(["op": "layer_open", "layer": idx, "open": open]))
        }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_with_layer_open(ptr, idx, open))
    }

    /// Entry `idx`'s CONTENT bounds — the canvas box of its opaque pixels,
    /// mask applied, and for a group the union over its descendants. This is
    /// what align and distribute act on, and it is NOT `LayerInfo`'s
    /// offset/width/height, which are a raster layer's pixel buffer rect.
    /// nil when nothing in the entry is opaque. At 16/32 bits the typed twin
    /// answers (`RasterPrecisionDocument.layerBounds`).
    func layerBounds(_ idx: Int) -> (x: Int, y: Int, width: Int, height: Int)? {
        guard isValidIndex(idx) else { return nil }
        if let precision { return precision.layerBounds(idx) }
        var xywh = [Int32](repeating: 0, count: 4)
        guard xywh.withUnsafeMutableBufferPointer({ dr_doc_layer_bounds(ptr, idx, $0.baseAddress) })
        else { return nil }
        return (Int(xywh[0]), Int(xywh[1]), Int(xywh[2]), Int(xywh[3]))
    }

    /// The entry a click at canvas `point` activates: the topmost one whose
    /// own coverage there is at least half, adjustment layers skipped. With
    /// `topLevel` the answer is that entry's top-level ancestor
    /// (Auto-Select: Group). nil when nothing is hit — the caller then leaves
    /// its selection alone rather than emptying it. At 16/32 bits the typed
    /// twin answers (`RasterPrecisionDocument.layerAt`).
    func layerAt(_ point: CGPoint, topLevel: Bool) -> Int? {
        guard point.x.isFinite, point.y.isFinite else { return nil }
        let x = Self.canvasCoordinate(point.x)
        let y = Self.canvasCoordinate(point.y)
        if let precision { return precision.layerAt(x: x, y: y, topLevel: topLevel) }
        var idx = 0
        guard dr_doc_layer_at(ptr, x, y, topLevel, &idx)
        else { return nil }
        return idx
    }

    /// A caller-supplied canvas coordinate narrowed to the `int32_t` the core
    /// takes. The clamp is done in DOUBLE arithmetic and never through `Int`:
    /// `Int(1e30)` traps ("Double value cannot be converted to Int because
    /// the result would be greater than Int.max") and kills the process, and
    /// this is reached from `auto_select_layer` with any finite number a
    /// model cares to send. Any coordinate that far out is off-canvas
    /// whichever extreme it lands on, so clamping answers the same "nothing
    /// there" the core would.
    private static func canvasCoordinate(_ v: CGFloat) -> Int32 {
        let floored = v.rounded(.down)
        if floored <= CGFloat(Int32.min) { return Int32.min }
        if floored >= CGFloat(Int32.max) { return Int32.max }
        return Int32(floored)
    }

    /// Every set op takes an explicit index list, validated here the way the
    /// core validates it: non-empty, in range and without repeats, since a
    /// repeat means the caller has miscounted and a silent dedupe would hide
    /// it. Ascending on the way through, which is the order the core wants.
    private func withIndices<T>(
        _ indices: [Int], _ body: (UnsafePointer<Int>?, Int) -> T?
    ) -> T? {
        let sorted = indices.sorted()
        guard !sorted.isEmpty, sorted.allSatisfy(isValidIndex),
              Set(sorted).count == sorted.count
        else { return nil }
        return sorted.withUnsafeBufferPointer { body($0.baseAddress, $0.count) }
    }

    /// Wraps `indices` — which must all share one parent — in a new group.
    /// Alongside the new document come the group's index and the two things
    /// the caller has to be able to report: the entries whose clipped flag
    /// was CLEARED (the bottom-most grouped entry loses a base that stayed
    /// outside) and the entries whose relative ORDER changed (a
    /// non-contiguous set gathers its subtrees into the topmost slot). Both
    /// are NEW indices.
    func groupLayers(
        _ indices: [Int], name: String
    ) throws -> (document: RasterDocument, group: Int, clearedClip: [Int], reordered: [Int])? {
        if isHighDepth { return try precisionGroupLayers(indices, name: name) }
        let cap = layerCount + 1
        var group = 0
        var cleared = [Int](repeating: 0, count: cap)
        var clearedLen = 0
        var reordered = [Int](repeating: 0, count: cap)
        var reorderedLen = 0
        let handle: OpaquePointer? = withIndices(indices) { buffer, count in
            cleared.withUnsafeMutableBufferPointer { clearedBuffer in
                reordered.withUnsafeMutableBufferPointer { reorderedBuffer in
                    dr_doc_group_layers(
                        ptr, buffer, count, name, &group,
                        clearedBuffer.baseAddress, &clearedLen,
                        reorderedBuffer.baseAddress, &reorderedLen, cap)
                }
            }
        }
        guard let document = wrap(handle) else { return nil }
        return (
            document, group,
            Array(cleared.prefix(min(clearedLen, cap))),
            Array(reordered.prefix(min(reorderedLen, cap)))
        )
    }

    /// Dissolves group `idx`; its children take its depth and its slot. The
    /// group's own mask, style, opacity, blend mode and clipped flag are
    /// discarded, so a caller reporting the loss must read them first.
    ///
    /// `clearedClip` is the mirror of `groupLayers`': the bottom-most child
    /// was baseless inside the group, so if it would gain a clip base out at
    /// the parent level its clipped flag is released instead — and the entry
    /// is named here (a NEW index) so the caller can report it.
    func ungroupLayer(
        _ idx: Int
    ) throws -> (document: RasterDocument, clearedClip: [Int])? {
        if isHighDepth { return try precisionUngroupLayer(idx) }
        guard isValidIndex(idx) else { return nil }
        let cap = layerCount
        var cleared = [Int](repeating: 0, count: max(cap, 1))
        var clearedLen = 0
        let handle: OpaquePointer? = cleared.withUnsafeMutableBufferPointer { buffer in
            dr_doc_ungroup_layer(ptr, idx, buffer.baseAddress, &clearedLen, cap)
        }
        guard let document = wrap(handle) else { return nil }
        return (document, Array(cleared.prefix(min(clearedLen, cap))))
    }

    /// The structural move: entry `from` and its subtree land at `to` at
    /// `depth` — how a layer moves into or out of a group.
    func moveLayerTo(from: Int, to: Int, depth: Int) throws -> RasterDocument? {
        if isHighDepth { return try precisionMoveLayerTo(from: from, to: to, depth: depth) }
        guard isValidIndex(from), isValidIndex(to), depth >= 0 else { return nil }
        return wrap(dr_doc_move_layer_to(ptr, from, to, UInt32(clamping: depth)))
    }

    /// Moves entry `idx` among its SIBLINGS only — never into or out of a
    /// group.
    func arrangeLayer(_ idx: Int, to how: DrArrange) throws -> RasterDocument? {
        if isHighDepth { return try precisionArrangeLayer(idx, to: how) }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_arrange_layer(ptr, idx, how))
    }

    /// Links the given entries under one id; unlink clears theirs.
    func linkLayers(_ indices: [Int]) throws -> RasterDocument? {
        if isHighDepth { return try precisionLinkLayers(indices) }
        return withIndices(indices) { buffer, count in wrap(dr_doc_link_layers(ptr, buffer, count)) }
    }

    func unlinkLayers(_ indices: [Int]) throws -> RasterDocument? {
        if isHighDepth { return try precisionUnlinkLayers(indices) }
        return withIndices(indices) { buffer, count in wrap(dr_doc_unlink_layers(ptr, buffer, count)) }
    }

    /// Translates every given entry by (dx, dy) — the ONE move-a-set op, so
    /// subtrees and link groups follow and a position lock refuses the whole
    /// call. At 8 bits a moved pattern fill is re-rendered over the canvas
    /// (`refittingFillLayers`, FillLayer.swift's PIN) unless `refit` is false —
    /// a drag tick, whose gesture refits once at its end; at 16 and 32 bits
    /// `geometrySet` re-renders it.
    func moveLayers(_ indices: [Int], dx: Int, dy: Int,
                    refit: Bool = true) throws -> RasterDocument? {
        if isHighDepth {
            return try precisionMoveLayers(indices, dx: dx, dy: dy)
        }
        let moved = withIndices(indices) { buffer, count in
            wrap(dr_doc_move_layers(
                ptr, buffer, count, Int32(clamping: dx), Int32(clamping: dy)))
        }
        guard refit, let moved else { return moved }
        return try moved.refittingFillLayers()
    }

    /// The same affine applied to every given entry, each deriving its own
    /// destination extent; all or nothing.
    func transformLayers(
        _ indices: [Int], _ transform: CGAffineTransform, sampler: DrResizeFilter
    ) throws -> RasterDocument? {
        try geometrySet(.affine(layers: indices, matrix: transform, sampler: sampler))
    }

    func geometryEditPlan(request: String) throws -> [String: Any] {
        if let precision {
            let context = try PrecisionWorkContext(source: precision)
            return try precision.pixelEditPlan(request: request, workingLimit: context.limit,
                                               otherRetained: context.otherRetained)
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required for geometry.") }
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        return try precisionJSON(dr_doc_geometry_edit_plan(ptr, request, &error), error)
    }

    func warpInfo(_ layer: Int) throws -> [String: Any]? {
        guard layer >= 0 && layer < layerCount else {
            throw DoctoredCoreError(message: "Invalid warp layer index.")
        }
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            return try precisionJSON(dr_precision_doc_warp_info(precision.ptr, layer, &error), error)
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required for warp.") }
        return try precisionJSON(dr_doc_warp_info(ptr, layer, &error), error)
    }

    func warpEditPlan(request: String) throws -> [String: Any] {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            return try precisionJSON(dr_precision_doc_warp_edit_plan(precision.ptr, request, &error), error)
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required for warp.") }
        return try precisionJSON(dr_doc_warp_edit_plan(ptr, request, &error), error)
    }

    func editingWarp(request: String, control: RasterJobControl? = nil) throws -> RasterDocument? {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            let budget = try PrecisionWorkContext(source: precision)
            let result = dr_precision_doc_warp_edit(precision.ptr, request, budget.limit,
                budget.otherRetained, control?.ptr, &error)
            if error != nil {
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Warp was refused."))
            }
            return try result.map { try RasterDocument(precision: RasterPrecisionDocument(owning: $0)) }
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required for warp.") }
        let budget = try legacyPrecisionWorkContext()
        let result = dr_doc_warp_edit(ptr, request, budget.limit, budget.other,
                                      control?.ptr, &error)
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Warp was refused."))
        }
        return wrap(result)
    }

    /// `budget` is the byte working-memory context; nil reads
    /// `legacyPrecisionWorkContext()`, which is main-only, so an off-main
    /// byte caller MUST pass it. Ignored for a typed document.
    func editingGeometry(request: String, budget: (limit: UInt64, other: UInt64)? = nil,
                         control: RasterJobControl? = nil) throws -> RasterDocument? {
        if let precision {
            let context = try PrecisionWorkContext(source: precision)
            return try precision.editingPixels(request: request, samples: [],
                workingLimit: context.limit, otherRetained: context.otherRetained,
                control: control).map { try RasterDocument(precision: $0) }
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required for geometry.") }
        try precisionCString(request, maximumBytes: 64 * 1024)
        let budget = try budget ?? legacyPrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_geometry_edit(ptr, request, budget.limit, budget.other, control?.ptr, &error)
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Geometry was refused."))
        }
        return wrap(result)
    }

    /// Move Selected Pixels: the byte plan, delta-independent
    /// `{retained_bytes, scratch_bytes, input_samples}`. Byte depth only; a
    /// typed document plans through `pixelEditPlan`.
    func selectedPixelsPlan(request: String) throws -> [String: Any] {
        guard precision == nil, let ptr else {
            throw DoctoredCoreError(message: "The selected-pixels plan is an 8-bit document API.")
        }
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        return try precisionJSON(dr_doc_selected_pixels_edit_plan(ptr, request, &error), error)
    }

    /// Move Selected Pixels: float the layer's pixels × the canvas u16
    /// `coverage` and move them by the request's delta, at either depth. nil
    /// is the identity. Off-main callers MUST pass `budget`
    /// (`legacyPrecisionWorkContext()` is main-only).
    func editingSelectedPixels(request: String, coverage: [UInt16],
                               budget: (limit: UInt64, other: UInt64)? = nil,
                               control: RasterJobControl? = nil) throws -> RasterDocument? {
        if let precision {
            let context = try PrecisionWorkContext(source: precision)
            return try precision.editingPixels(request: request, samples: coverage,
                workingLimit: context.limit, otherRetained: context.otherRetained,
                control: control).map { try RasterDocument(precision: $0) }
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required to move pixels.") }
        try precisionCString(request, maximumBytes: 64 * 1024)
        let budget = try budget ?? legacyPrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer { samples in
            dr_doc_selected_pixels_edit(ptr, request, samples.baseAddress, samples.count,
                                        budget.limit, budget.other, control?.ptr, &error)
        }
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error,
                fallback: "Moving the selected pixels was refused."))
        }
        return wrap(result)
    }

    /// The FLOAT's canvas rect for `layer` × `coverage` — the
    /// bbox of the pixels with alpha > 0 and coverage > 0 — or nil for an
    /// empty float, at either depth. Reads in place and needs no budget, so
    /// it is safe off main.
    func selectedPixelsBounds(layer: Int, coverage: [UInt16],
                              control: RasterJobControl? = nil) throws -> CGRect? {
        if let precision {
            return try precision.selectedPixelsBounds(layer: layer, coverage: coverage,
                                                      control: control)
        }
        guard let ptr else {
            throw DoctoredCoreError(message: "A document is required to find the selected pixels.")
        }
        var rect = [UInt32](repeating: 0, count: 4)
        var error: UnsafeMutablePointer<CChar>?
        let found = coverage.withUnsafeBufferPointer { samples in
            rect.withUnsafeMutableBufferPointer { out in
                dr_doc_selected_pixels_bounds(ptr, layer, samples.baseAddress, samples.count,
                                              control?.ptr, out.baseAddress, &error)
            }
        }
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error,
                fallback: "The selected pixels could not be measured."))
        }
        guard found else { return nil }
        return CGRect(x: Int(rect[0]), y: Int(rect[1]), width: Int(rect[2]), height: Int(rect[3]))
    }

    func duplicateLayers(_ indices: [Int]) throws -> RasterDocument? {
        if isHighDepth { return try precisionDuplicate(indices) }
        return withIndices(indices) { buffer, count in wrap(dr_doc_duplicate_layers(ptr, buffer, count)) }
    }

    /// Layer ▸ Duplicate to New Document: the given entries, with
    /// their subtrees, as a NEW document at this one's depth, canvas,
    /// profile and resolution. One core call at each depth; the receiver is
    /// unchanged.
    func duplicatingToNewDocument(_ indices: [Int]) throws -> RasterDocument? {
        if isHighDepth { return try precisionDuplicateToNewDocument(indices) }
        return withIndices(indices) { buffer, count in
            wrap(dr_doc_duplicate_to_new_document(ptr, buffer, count))
        }
    }

    func removeLayers(_ indices: [Int]) throws -> RasterDocument? {
        if isHighDepth { return try precisionRemoveLayers(indices) }
        return withIndices(indices) { buffer, count in wrap(dr_doc_remove_layers(ptr, buffer, count)) }
    }

    /// Merge Layers over a multi-selection: one raster entry at
    /// the lowest member's slot, through the projection's own kernel.
    func mergeLayers(_ indices: [Int]) throws -> RasterDocument? {
        if isHighDepth { return try precisionMergeLayers(indices) }
        return withIndices(indices) { buffer, count in wrap(dr_doc_merge_layers(ptr, buffer, count)) }
    }

    /// Aligns the given entries' CONTENT bounds to `edge` of the selection's
    /// union, or of the canvas with `toCanvas`.
    func alignLayers(_ indices: [Int], edge: DrAlign, toCanvas: Bool) -> RasterDocument? {
        withIndices(indices) { buffer, count in
            wrap(dr_doc_align_layers(ptr, buffer, count, edge, toCanvas))
        }
    }

    /// Spaces the given entries evenly: the outermost two keep their places
    /// and the gaps between adjacent content bounds are equalized.
    func distributeLayers(_ indices: [Int], vertical: Bool) -> RasterDocument? {
        withIndices(indices) { buffer, count in
            wrap(dr_doc_distribute_layers(ptr, buffer, count, vertical))
        }
    }

    /// Replaces every entry that CONTRIBUTES to the projection with one
    /// canvas-sized raster entry holding it. A visible layer inside a hidden
    /// group contributes nothing and therefore SURVIVES.
    func mergeVisible() throws -> RasterDocument? {
        if isHighDepth { return try precisionMergeVisible() }
        return wrap(dr_doc_merge_visible(ptr))
    }

    /// Adds the visible projection as a new entry above `above`'s subtree,
    /// leaving the rest of the stack alone.
    func stampVisible(above: Int, name: String) throws -> RasterDocument? {
        if isHighDepth { return try precisionStampVisible(above: above, name: name) }
        guard isValidIndex(above) else { return nil }
        return wrap(dr_doc_stamp_visible(ptr, above, name))
    }

    /// #547 phase 4c: Stamp Selected — one group, or two or more entries
    /// sharing one parent, composited into ONE new layer directly above the
    /// topmost, every original untouched. Throws the typed core's sentence;
    /// nil at 8 bits where the byte core refuses.
    func stampingSelected(_ indices: [Int], name: String) throws -> RasterDocument? {
        if isHighDepth { return try precisionStampSelected(indices, name: name) }
        return withIndices(indices) { buffer, count in
            wrap(dr_doc_stamp_selected(ptr, buffer, count, name))
        }
    }

    /// #547 phase 4c: Merge Group — the visible group `idx` replaced by ONE
    /// layer holding its contents and keeping its own appearance.
    func mergingGroup(_ idx: Int) throws -> RasterDocument? {
        if isHighDepth { return try precisionMergeGroup(idx) }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_merge_group(ptr, idx))
    }

    /// Layer Via Copy / Via Cut. This op RASTERIZES — it keeps neither the
    /// layer's metadata nor its style and refuses a group or an adjustment
    /// layer — so a caller whose target is a described layer wants
    /// `duplicatingLayer` instead. `mask` is a canvas-sized coverage buffer,
    /// or nil for the whole layer.
    func layerVia(_ idx: Int, mask: [UInt8]?, cut: Bool, name: String) -> RasterDocument? {
        guard isValidIndex(idx) else { return nil }
        guard let mask = mask else {
            return wrap(dr_doc_layer_via(
                ptr, idx, nil, UInt32(width), UInt32(height), cut, name))
        }
        guard mask.count == width * height else { return nil }
        return mask.withUnsafeBufferPointer { coverage in
            wrap(dr_doc_layer_via(
                ptr, idx, coverage.baseAddress, UInt32(width), UInt32(height), cut, name))
        }
    }
}
