import AppKit

// Unsupported byte methods receive NULL; the public capability gate must refuse them before work.
//
// The typed label runs through `RasterDocument.applyingPrecisionBookkeepingEdit` (synchronous on
// main, never `BackgroundWork`, refusing while it is occupied) and both call sites pass
// `renderNeutral: true`, so `applyEdit` carries the current projection (`carriedProjection`)
// instead of re-preparing the display behind a modal panel, and it stays one undo step. The typed
// layer RENAME takes the same route: `withLayerName`'s typed branch is
// `applyingPrecisionBookkeepingEdit`, and the Layers panel's inline rename passes `renderNeutral:
// true`. The typed LOCK, LINK and UNLINK (`precisionLayerLock` / `precisionLinkLayers` /
// `precisionUnlinkLayers`) and the channel NAME and OVERLAY (`renamingChannel` /
// `settingChannelOverlay`) take it too, every UI and MCP call site passes `renderNeutral: true`,
// and `set_layer_properties` at depth publishes render-neutral when only the name changed.

extension RasterDocument {
    /// Translate the expanded set as one captured operation. Keep artwork's
    /// description with its unchanged native raster even on a worker thread.
    func precisionMoveLayers(_ indices: [Int], dx: Int, dy: Int,
                             control: RasterJobControl? = nil) throws -> RasterDocument? {
        guard precision != nil else {
            throw DoctoredCoreError(message: "A high-depth document is required for Move.")
        }
        guard dx != 0 || dy != 0 else { return nil }
        let matrix = CGAffineTransform(translationX: CGFloat(dx), y: CGFloat(dy))
        return try geometrySet(.affine(layers: indices, matrix: matrix,
                                       sampler: DR_FILTER_NEAREST), rasterizeArtwork: true,
                               control: control)
    }

    /// Same captured command from a UI/MCP call or an already-owned final
    /// worker. The latter executes directly and never nests BackgroundWork.
    func performingPrecisionPixelEdit(_ command: PrecisionPixelCommand,
                                      input: PrecisionScalarBuffer? = nil) throws -> RasterDocument? {
        guard let precision else { throw DoctoredCoreError(message: "A high-depth document is required.") }
        let result: RasterPrecisionDocument?
        if Thread.isMainThread {
            result = try PrecisionPixelEdit(precision, command: command, input: input).run()
        } else {
            let budget = try PrecisionWorkContext(source: precision)
            let plan = try precision.pixelEditPlan(request: command.request, workingLimit: budget.limit,
                                                   otherRetained: budget.otherRetained)
            guard try PrecisionDocumentDescriptor.integer(plan, "input_samples") == UInt64(input?.samples.count ?? 0),
                  plan["output_samples"] == nil else {
                throw DoctoredCoreError(message: "High-depth input does not match this operation.")
            }
            if let input {
                guard try PrecisionDocumentDescriptor.integer(plan, "input_width") == UInt64(input.width),
                      try PrecisionDocumentDescriptor.integer(plan, "input_height") == UInt64(input.height),
                      try PrecisionDocumentDescriptor.integer(plan, "input_planes") == UInt64(input.planeCount) else {
                    throw DoctoredCoreError(message: "High-depth input layout does not match this operation.")
                }
            }
            let scratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
            let control = try RasterJobControl()
            guard scratch > 0, control.setScratchLimit(scratch) else {
                throw DoctoredCoreError(message: "Could not configure high-depth working memory.")
            }
            result = try precision.editingPixels(request: command.request, samples: input?.samples ?? [],
                workingLimit: budget.limit, otherRetained: budget.otherRetained, control: control)
        }
        return try result.map { try RasterDocument(precision: $0) }
    }

    /// Main thread only. O(records) typed edit (no input samples) run synchronously; never BackgroundWork.
    func applyingPrecisionBookkeepingEdit(_ command: PrecisionPixelCommand) throws -> RasterDocument? {
        guard let precision else { throw DoctoredCoreError(message: "A high-depth document is required.") }
        guard !BackgroundWork.occupied else {
            throw DoctoredCoreError(message: BackgroundWork.preparingBusyMessage)
        }
        let context = try PrecisionWorkContext(source: precision)
        let plan = try precision.pixelEditPlan(request: command.request, workingLimit: context.limit,
                                               otherRetained: context.otherRetained)
        guard try PrecisionDocumentDescriptor.integer(plan, "input_samples") == 0,
              plan["output_samples"] == nil else {
            throw DoctoredCoreError(message: "High-depth input does not match this operation.")
        }
        let scratch = try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes")
        _ = try RasterResources.shared.admit(scratch: scratch)
        let budget = try PrecisionWorkContext(source: precision)
        let control = try RasterJobControl()
        guard scratch > 0, control.setScratchLimit(scratch) else {
            throw DoctoredCoreError(message: "Could not configure high-depth working memory.")
        }
        let result = try precision.editingPixels(request: command.request, samples: [],
            workingLimit: budget.limit, otherRetained: budget.otherRetained, control: control)
        return try result.map { try RasterDocument(precision: $0) }
    }

    func performingPrecisionAdjustment(_ operation: PrecisionCapturedEdit.Operation) throws -> RasterDocument? {
        guard let precision else { throw DoctoredCoreError(message: "A high-depth document is required.") }
        if Thread.isMainThread {
            return try PrecisionCapturedEdit(precision, operation: operation).run().map { try RasterDocument(precision: $0) }
        }
        let budget = try PrecisionWorkContext(source: precision)
        let next: RasterPrecisionDocument?
        switch operation {
        case .add(let name, let adjustment, let opacity, let anchor, let position):
            next = try precision.addingAdjustment(name: name, description: adjustment.description, opacity: opacity,
                anchor: anchor, position: position,
                workingLimit: budget.limit, otherRetained: budget.otherRetained)
        case .set(let index, let adjustment):
            next = try precision.settingAdjustment(index, description: adjustment.description,
                workingLimit: budget.limit, otherRetained: budget.otherRetained)
        case .apply(let index, let adjustment):
            next = try precision.applyingAdjustment(index, description: adjustment.description,
                workingLimit: budget.limit, otherRetained: budget.otherRetained)
        case .selected(let request, let coverage):
            next = try precision.selectedEdit(request: request, coverage: coverage?.samples ?? [],
                workingLimit: budget.limit, otherRetained: budget.otherRetained, control: nil)
        case .assignProfile(let profile):
            next = try precision.assigningProfile(profile, workingLimit: budget.limit, otherRetained: budget.otherRetained)
        case .convertProfile(let profile):
            next = try precision.convertingToProfile(profile, workingLimit: budget.limit, otherRetained: budget.otherRetained)
        case .convertProfileWith(let profile, let options):
            next = try precision.convertingToProfile(
                profile, options: options, workingLimit: budget.limit,
                otherRetained: budget.otherRetained, control: nil)
                .map { RasterPrecisionDocument(owning: $0.document) }
        case .assignProfileRasterizing(let profile):
            next = try precision.assigningProfileRasterizing(
                profile, workingLimit: budget.limit, otherRetained: budget.otherRetained,
                control: nil)
                .map { RasterPrecisionDocument(owning: $0) }
        }
        return try next.map { try RasterDocument(precision: $0) }
    }

    static func precisionFilter(_ filter: DrResizeFilter) throws -> PrecisionGeometryFilter {
        switch filter {
        case DR_FILTER_NEAREST: return .nearest
        case DR_FILTER_BILINEAR: return .bilinear
        case DR_FILTER_CATMULL_ROM: return .bicubic
        case DR_FILTER_LANCZOS3: return .lanczos3
        default: throw DoctoredCoreError(message: "This resampling filter is not supported for high-depth pixels.")
        }
    }

    func precisionTransform(_ indices: [Int], matrix: CGAffineTransform,
                            filter: DrResizeFilter) throws -> RasterDocument? {
        try geometrySet(.affine(layers: indices, matrix: matrix, sampler: filter))
    }

    /// The typed twin of byte `dr_doc_with_layer_offset` — an offset is a
    /// property write, not a move, so the integer set translation does NOT
    /// follow links (`follow_links: false`): a raster moves alone, a group its
    /// own subtree. `precisionMoveLayers` is the move that carries partners.
    func precisionLayerOffset(_ index: Int, dx: Int, dy: Int) throws -> RasterDocument? {
        guard dx != 0 || dy != 0 else { return nil }
        return try editingGeometry(request: PrecisionPixelCommand([
            "op": "affine_set", "layers": [index],
            "matrix": [1, 0, 0, 1, Double(dx), Double(dy)],
            "filter": try Self.precisionFilter(DR_FILTER_NEAREST).rawValue,
            "follow_links": false,
        ]).request)
    }

    func precisionDuplicate(_ indices: [Int]) throws -> RasterDocument? {
        guard !indices.isEmpty, Set(indices).count == indices.count,
              indices.allSatisfy({ (0..<layerCount).contains($0) }) else {
            throw DoctoredCoreError(message: "Invalid high-depth layer selection.")
        }
        // Reject ancestor/descendant duplication as ambiguous; the UI's
        // canonical root selection already removes these overlaps.
        for index in indices where layerIsGroup(index) {
            if let subtree = layerSubtree(index), indices.contains(where: { $0 != index && (subtree.start..<subtree.end).contains($0) }) {
                throw DoctoredCoreError(message: "Select each high-depth subtree only once.")
            }
        }
        var current = self
        var changed = false
        for index in indices.sorted(by: >) {
            guard let precision = current.precision else { throw DoctoredCoreError(message: "High-depth owner disappeared.") }
            if let next = try precision.duplicateLayer(index) {
                current = try RasterDocument(precision: next)
                changed = true
            }
        }
        return changed ? current : nil
    }

    // MARK: - Typed structure (12E-1)
    //
    // The five pure structure edits, each ONE core request through the
    // generic typed edit surface — never a byte surrogate. The two grouping
    // ops need the core's own REPORT (which entries lost a clipping mask,
    // which changed relative order, where the freed children landed), and
    // that report rides in the PLAN, so those two plan first and then apply
    // the very same request.

    /// The core's `"report"` object for a structural request, read out of the
    /// plan; nil when the request is an exact no-op (the core reports nothing
    /// for a document it would not change).
    private func precisionStructureReport(_ command: PrecisionPixelCommand) throws -> [String: Any]? {
        guard let precision else {
            throw DoctoredCoreError(message: "A high-depth document is required.")
        }
        let budget = try PrecisionWorkContext(source: precision)
        let plan = try precision.pixelEditPlan(request: command.request, workingLimit: budget.limit,
                                               otherRetained: budget.otherRetained)
        return plan["report"] as? [String: Any]
    }

    /// The core answers JSON numbers; a report's index lists come back as
    /// `NSNumber`s and are read through one place rather than five.
    private static func indices(_ value: Any?) -> [Int] {
        (value as? [Any])?.compactMap { ($0 as? NSNumber)?.intValue } ?? []
    }

    func precisionMoveLayerTo(from: Int, to: Int, depth: Int) throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "move_layer_to", "from": from, "to": to, "depth": depth])
        return try performingPrecisionPixelEdit(command)
    }

    func precisionArrangeLayer(_ idx: Int, to how: DrArrange) throws -> RasterDocument? {
        let name: String
        switch how {
        case DR_ARRANGE_FRONT: name = "front"
        case DR_ARRANGE_FORWARD: name = "forward"
        case DR_ARRANGE_BACKWARD: name = "backward"
        case DR_ARRANGE_BACK: name = "back"
        default: return nil
        }
        let command = try PrecisionPixelCommand(
            ["op": "arrange_layer", "layer": idx, "how": name])
        return try performingPrecisionPixelEdit(command)
    }

    func precisionRemoveLayers(_ indices: [Int]) throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "remove_layers", "layers": indices])
        return try performingPrecisionPixelEdit(command)
    }

    /// The typed Duplicate to New Document. The core answers a NEW
    /// document holding the given records; the receiver is unchanged.
    func precisionDuplicateToNewDocument(_ indices: [Int]) throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "duplicate_to_new_document", "layers": indices])
        return try performingPrecisionPixelEdit(command)
    }

    /// The 12E-2b typed ADD: one transparent canvas-sized leaf at the slot
    /// `position` names relative to `anchor`. Same shape as the five structure
    /// ops above — one request through the one typed edit surface — so a New
    /// Layer at depth never routes through a byte surrogate.
    func precisionAddLayer(anchor: Int, position: LayerInsertPosition, name: String)
        throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "add_layer", "name": name, "anchor": anchor,
             "position": position.rawValue])
        return try performingPrecisionPixelEdit(command)
    }

    // The 12E-1b typed MERGES and LOCK/LINK edits. Each is the same shape as
    // the five structure ops above — one request through the one typed edit
    // surface — so a typed edit never routes through a byte surrogate.

    func precisionMergeLayers(_ indices: [Int]) throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "merge_layers", "layers": indices])
        return try performingPrecisionPixelEdit(command)
    }

    func precisionMergeDown(_ idx: Int) throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "merge_down", "layer": idx])
        return try performingPrecisionPixelEdit(command)
    }

    func precisionMergeVisible() throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "merge_visible"])
        return try performingPrecisionPixelEdit(command)
    }

    // Phase 12E-2c: the typed STAMP VISIBLE and FLATTEN. Same shape again —
    // one request through the one typed edit surface.

    func precisionStampVisible(above: Int, name: String) throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "stamp_visible", "name": name, "anchor": above])
        return try performingPrecisionPixelEdit(command)
    }

    // #547 phase 4c: STAMP SELECTED and MERGE GROUP, one request each.

    func precisionStampSelected(_ indices: [Int], name: String) throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "stamp_selected", "layers": indices, "name": name])
        return try performingPrecisionPixelEdit(command)
    }

    func precisionMergeGroup(_ idx: Int) throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "merge_group", "layer": idx])
        return try performingPrecisionPixelEdit(command)
    }

    func precisionFlatten() throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "flatten"])
        return try performingPrecisionPixelEdit(command)
    }

    func precisionLayerLock(_ idx: Int, locks: UInt32) throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "layer_lock", "layer": idx, "locks": Int(locks)])
        return try applyingPrecisionBookkeepingEdit(command)
    }

    func precisionLinkLayers(_ indices: [Int]) throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "link_layers", "layers": indices])
        return try applyingPrecisionBookkeepingEdit(command)
    }

    func precisionUnlinkLayers(_ indices: [Int]) throws -> RasterDocument? {
        let command = try PrecisionPixelCommand(
            ["op": "unlink_layers", "layers": indices])
        return try applyingPrecisionBookkeepingEdit(command)
    }

    func precisionUngroupLayer(_ idx: Int) throws -> (document: RasterDocument, clearedClip: [Int])? {
        let command = try PrecisionPixelCommand(
            ["op": "ungroup_layer", "layer": idx])
        guard let report = try precisionStructureReport(command),
              let next = try performingPrecisionPixelEdit(command) else { return nil }
        return (next, Self.indices(report["cleared_clip"]))
    }

    func precisionGroupLayers(_ indices: [Int], name: String) throws
        -> (document: RasterDocument, group: Int, clearedClip: [Int], reordered: [Int])? {
        guard precision != nil else {
            throw DoctoredCoreError(message: "A high-depth document is required.")
        }
        // A new group is PASS THROUGH at every depth, as the byte op's is,
        // and a float document composites it (measured
        // against Photoshop 26.10.0, receipt `float-pass-through-473/take1`).
        let command = try PrecisionPixelCommand(
            ["op": "group_layers", "layers": indices, "name": name, "pass_through": true])
        guard let report = try precisionStructureReport(command),
              let group = (report["group"] as? NSNumber)?.intValue,
              let next = try performingPrecisionPixelEdit(command) else { return nil }
        return (next, group, Self.indices(report["cleared_clip"]),
                Self.indices(report["reordered"]))
    }

    func precisionMaskProperties(_ index: Int, linked: Bool? = nil, density: Double? = nil,
                                 feather: Double? = nil, enabled: Bool? = nil) throws -> RasterDocument? {
        // The mask's own properties — the raster mask's beside a vector mask
        // (`maskEditBaseline`), never the baked mask's.
        guard precisionMetadata?.layer(index) != nil, let (mask, wasEnabled) = maskEditBaseline(index)
        else {
            throw DoctoredCoreError(message: "Choose a high-depth mask.")
        }
        return try performingPrecisionPixelEdit(.maskProperties(layer: index, x: mask.offsetX, y: mask.offsetY,
            linked: linked ?? mask.linked, density: density ?? mask.density,
            feather: feather ?? mask.feather, enabled: enabled ?? wasEnabled))
    }
}

extension RasterDocument {
    /// The legacy source belongs to the same global live union as typed
    /// captures. Subtract only that source's deduplicated payload, once.
    func legacyPrecisionWorkContext() throws -> (limit: UInt64, other: UInt64) {
        guard !isHighDepth else { throw DoctoredCoreError(message: "Expected an eight-bit conversion source.") }
        let report = try RasterResources.shared.report(evict: true)
        let own = try Self.resourceReport(documents: [self], images: [], scratch: 0, limit: 0)
        let retained = try PrecisionDocumentDescriptor.integer(report, "unique_retained_bytes")
        let source = try PrecisionDocumentDescriptor.integer(own, "unique_retained_bytes")
        guard source <= retained else { throw DoctoredCoreError(message: "Conversion source is missing from resource accounting.") }
        let other = try PrecisionDocumentDescriptor.sum(retained - source,
            PrecisionDocumentDescriptor.integer(report, "transient_estimate_bytes"))
        return (RasterResources.shared.workingLimit, other)
    }
}
