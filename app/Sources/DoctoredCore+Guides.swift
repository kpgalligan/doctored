import AppKit

// **12E-1c** opens the five GUIDE tools and the two guide menu items at both typed depths, and
// there is no Swift-side depth code to it: the guide wrappers in this file branch on `precision`
// and `wrapTyped` boxes the typed handle, so `Guides.swift`, `EditorViewController+Guides.swift`,
// `AgentServer+Guides.swift`, `AgentCatalog.swift` and `ActionPlayer.swift` are untouched and
// `ImageDocument.applyGuideEdit` stays the ONE edit path. The gates that moved are
// `DocumentPrecision.commonTools` (the five tools), both whitelists in
// `EditorViewController+Precision.swift` (`newGuide:` / `clearGuides:` plus the `toggleGuides` /
// `toggleLockGuides` preference prefixes), `PrecisionModeConversion.losses` (guides and the ruler
// origin are PRESERVED now, not discarded) and the typed `get_document`
// (`AgentServer+DocumentReply.swift`), which reports the `guides` and `ruler` blocks through the
// byte path's own builders so the two depths answer that question the same way — and the
// `save_preservation` block through `preservationFields` at 16 bits.

extension RasterDocument {
    // MARK: - Guides and rulers
    //
    // Since 12E-1c every call below picks a family by DEPTH: a typed
    // document answers through `dr_precision_doc_guide_*`, whose contracts,
    // sentinels and refusals are the byte family's verbatim (`ffi_guide`).
    // Nothing above this line and nothing in `AgentServer+Guides.swift`,
    // `EditorViewController+Guides.swift` or `Guides.swift` knows which
    // depth it is talking to.

    /// Value snapshot of one guide (see the header's "Guides, rulers and
    /// snapping" section): a line across the whole canvas at `position`, in
    /// the canvas's continuous coordinate space — x for a vertical guide, y
    /// for a horizontal one, absolute and NOT measured from the ruler origin.
    struct GuideInfo {
        /// The core's stable per-guide identity: unique while the app runs,
        /// kept through a move, the geometry ops and undo/redo, and gone when
        /// the guide is. The list is re-SORTED by position on every edit, so
        /// an index cannot identify a guide across one — this is what a drag
        /// holds on to.
        let id: UInt64
        let orientation: GuideOrientation
        let position: Double
    }

    /// The largest guide list any document may carry (1024), asked of the
    /// core so the number has one home. Unlike `maxChannels` it takes no
    /// canvas: a guide is a line, not a plane. `addingGuide` answers one nil
    /// for four different refusals, so a caller that wants to SAY the list is
    /// full compares against this first.
    static var maxGuides: Int { dr_max_guides() }

    var guideCount: Int {
        precision.map { Int(dr_precision_doc_guide_count($0.ptr)) } ?? Int(dr_doc_guide_count(ptr))
    }

    /// The mutators' typed half: the core's new handle wrapped in a facade,
    /// nil for the same refusals the byte `wrap` reports (`ffi_guide`'s typed
    /// twins answer NULL for exactly what the byte family answers NULL for).
    /// The facade's own metadata capture throws its reason.
    func wrapTyped(_ result: OpaquePointer?) throws -> RasterDocument? {
        try result.map { try RasterDocument(precision: RasterPrecisionDocument(owning: $0)) }
    }

    private func isValidGuide(_ i: Int) -> Bool {
        i >= 0 && i < guideCount
    }

    /// One guide read WITHOUT the bounds check, for an index already known to
    /// be in range: the body `guideInfo` and `guides` share, so reading the
    /// whole list costs three FFI calls per guide rather than four.
    private func guideAt(_ i: Int) -> GuideInfo? {
        let raw = precision.map { dr_precision_doc_guide_orientation($0.ptr, i) }
            ?? dr_doc_guide_orientation(ptr, i)
        guard let orientation = GuideOrientation(rawValue: raw) else { return nil }
        return GuideInfo(
            id: precision.map { dr_precision_doc_guide_id($0.ptr, i) } ?? dr_doc_guide_id(ptr, i),
            orientation: orientation,
            position: precision.map { dr_precision_doc_guide_position($0.ptr, i) }
                ?? dr_doc_guide_position(ptr, i))
    }

    func guideInfo(_ i: Int) -> GuideInfo? {
        guard isValidGuide(i) else { return nil }
        return guideAt(i)
    }

    /// The whole list in the core's own order — sorted by orientation, then
    /// by position — read in one pass. The canvas caches this on every
    /// document change so a redraw crosses the boundary not at all.
    var guides: [GuideInfo] {
        (0..<guideCount).compactMap { guideAt($0) }
    }

    /// The index a guide id currently sits at, or nil once it is gone. A
    /// linear scan on purpose: the list is a handful of entries, and a
    /// dictionary would have to be rebuilt on every edit that re-sorts it.
    func guideIndex(id: UInt64) -> Int? {
        guard id != 0 else { return nil }
        // The count is read ONCE and each id directly: a bounds-checked read
        // per candidate would re-read `guideCount` over the FFI each time.
        let n = guideCount
        return (0..<n).first { i in
            let candidate = precision.map { dr_precision_doc_guide_id($0.ptr, i) }
                ?? dr_doc_guide_id(ptr, i)
            return candidate == id
        }
    }

    /// Adds a guide; the list re-sorts, so the new guide is not necessarily
    /// last. nil when the position is outside the canvas, non-finite, already
    /// occupied by a same-orientation guide, or the list is full — the core
    /// answers one refusal for all four, so a host that must distinguish them
    /// checks first (see `AgentServer+Guides`).
    func addingGuide(
        _ orientation: GuideOrientation, at position: Double
    ) throws -> RasterDocument? {
        guard position.isFinite else { return nil }
        if let precision {
            return try wrapTyped(
                dr_precision_doc_add_guide(precision.ptr, orientation.rawValue, position))
        }
        return wrap(dr_doc_add_guide(ptr, orientation.rawValue, position))
    }

    /// Moves guide `i` along its own axis, keeping its identity and its
    /// orientation. nil for an out-of-range index, a non-finite or
    /// out-of-canvas position, the position it already has, or one another
    /// same-orientation guide occupies — again one refusal for all of them.
    func movingGuide(_ i: Int, to position: Double) throws -> RasterDocument? {
        guard isValidGuide(i), position.isFinite else { return nil }
        if let precision {
            return try wrapTyped(dr_precision_doc_move_guide(precision.ptr, i, position))
        }
        return wrap(dr_doc_move_guide(ptr, i, position))
    }

    func removingGuide(_ i: Int) throws -> RasterDocument? {
        guard isValidGuide(i) else { return nil }
        if let precision { return try wrapTyped(dr_precision_doc_remove_guide(precision.ptr, i)) }
        return wrap(dr_doc_remove_guide(ptr, i))
    }

    /// nil on an already empty list (clearing nothing is not an edit).
    func clearingGuides() throws -> RasterDocument? {
        if let precision { return try wrapTyped(dr_precision_doc_clear_guides(precision.ptr)) }
        return wrap(dr_doc_clear_guides(ptr))
    }

    /// The ruler's zero point in canvas coordinates, (0, 0) — the canvas's
    /// top-left — by default. It moves the rulers' LABELS and the New Guide
    /// sheet's typed number; it moves no grid, no snap target and no
    /// coordinate any other API reports.
    var rulerOrigin: (x: Double, y: Double) {
        var out = [Double](repeating: 0, count: 2)
        let ok = out.withUnsafeMutableBufferPointer { buffer in
            precision.map { dr_precision_doc_ruler_origin($0.ptr, buffer.baseAddress) }
                ?? dr_doc_ruler_origin(ptr, buffer.baseAddress)
        }
        return ok ? (out[0], out[1]) : (0, 0)
    }

    /// Moves the ruler zero point. nil on a non-finite component, a point
    /// outside the canvas, or the origin the document already has (after the
    /// core's four-decimal quantization, so a reported value echoed back
    /// registers no edit).
    func settingRulerOrigin(x: Double, y: Double) throws -> RasterDocument? {
        guard x.isFinite, y.isFinite else { return nil }
        if let precision {
            return try wrapTyped(dr_precision_doc_set_ruler_origin(precision.ptr, x, y))
        }
        return wrap(dr_doc_set_ruler_origin(ptr, x, y))
    }
}

/// Which way a guide runs — the core's `DrGuideOrientation`, whose two values
/// are `DR_GUIDE_HORIZONTAL` (0) and `DR_GUIDE_VERTICAL` (1). The name is the
/// LINE's direction, so a horizontal guide has a constant y and its position
/// is measured against the canvas HEIGHT. Wrapping it here is what keeps the
/// raw C int inside the `DoctoredCore*.swift` files.
enum GuideOrientation: Int32 {
    /// A line of constant y.
    case horizontal = 0
    /// A line of constant x.
    case vertical = 1

    /// The canvas extent this orientation's position is measured against:
    /// the height for a horizontal guide, the width for a vertical one. Both
    /// ends are legal — a guide may sit exactly on a canvas edge.
    func extent(inCanvas size: CGSize) -> Double {
        self == .horizontal ? Double(size.height) : Double(size.width)
    }
}
