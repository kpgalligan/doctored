import AppKit

// MCP `transform_again {duplicate, rasterize_artwork}` is the twin and refuses
// `TransformAgainMemory.refusal(on:)` (no memory, or one committed on a canvas of
// another size).

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let transformAgainHandlers: [String: ToolHandler] = [
        "transform_again": { $0.transformAgain },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let transformAgainToolFacts: [String: ToolFacts] = [
        // Transform Again repeats `transform_layers` or
        // `transform_selected_pixels`, both typed.
        // Phase 12F-1b: the EXACT canvas geometry. A zero-angle `crop` keeps
        // every raster and mask allocation; a quarter turn and a flip are a
        // Nearest PERMUTATION, so an extended-range component is only ever
        // copied. The FILTERED geometry is admitted too — a `crop`
        // with an angle (straighten), an arbitrary `rotate`, `image_size`,
        // the layer transforms and `transform_mask` — through the core's
        // premultiplied extended-range resampler, which clamps each RGB
        // component to the layer's own visible range (D3). `transform_again`
        // repeats a layer transform, and with a selection a Free Transform
        // of the selected pixels, admitted at float.
        "transform_again": [.selectionSensitive, .typedTool, .floatTool, .cmykTool],
    ]

    /// `transform_again` — mirrors Layer ▸ Transform Again (⇧⌘T) and, with
    /// `duplicate`, Layer ▸ Transform Again with Copy (⌥⇧⌘T)
    /// (EditorViewController+TransformAgain.swift): the application's last
    /// committed Free Transform, when this canvas is the size it was committed
    /// on (`ImageDocument.lastFreeTransform`, the decisions in
    /// TransformAgain.swift), applied verbatim to ⌘T's target as
    /// ONE step. With a selection: the active layer's selected pixels
    /// (`transform_selected_pixels`' commit); without one: the layer selection
    /// (`transform_layers`' commit, and its duplicate branch with `duplicate`).
    /// A remembered true perspective re-sends its rectangle and corners;
    /// over editable artwork it needs `rasterize_artwork`, `transform_layers`'
    /// rule and ⌘T's prompt. `target` "mask" or "channel:<name>" with
    /// `background` repeats onto that plane's selected area, as ⇧⌘T
    /// does with the plane as the edit target; the memory stores no target.
    /// "red", "green", "blue" or "alpha" repeats onto that colour
    /// plane of the active layer, rasterizing silently, the layer path's
    /// reply.
    func transformAgain(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "No image loaded.") }
        guard let again = document.lastFreeTransform else {
            throw ToolError(message: TransformAgainMemory.refusal(on: doc.canvasSize))
        }
        let planeTarget = try paintTarget(a, document)
        if planeTarget == .layer, a["background"] != nil {
            throw ToolError(message: Self.layerBackgroundRefusal)
        }
        let duplicate = boolArg(a, "duplicate") ?? false
        let name = RepeatableTransform.actionName(duplicate: duplicate)
        if let selection = document.canvasSelection {
            if planeTarget != .layer {
                return try transformAgainPlane(
                    a, document, doc, selection: selection, target: planeTarget, again: again,
                    duplicate: duplicate, name: name)
            }
            return try transformAgainSelectedPixels(
                document, doc, selection: selection, again: again, duplicate: duplicate,
                name: name)
        }
        guard planeTarget == .layer else {
            throw ToolError(message: "A mask or channel target needs a selection; "
                + "transform_mask transforms a whole mask.")
        }
        if duplicate {
            return try transformAgainCopy(
                document, doc, again: again, name: name,
                authorize: boolArg(a, "rasterize_artwork") ?? false)
        }
        return try transformAgainLayers(
            document, doc, again: again, name: name,
            authorize: boolArg(a, "rasterize_artwork") ?? false)
    }

    private func transformAgainSelectedPixels(
        _ document: ImageDocument, _ doc: RasterDocument, selection: CanvasSelection,
        again: RepeatableTransform, duplicate: Bool, name: String
    ) throws -> String {
        let layer = document.activeLayerIndex
        try rejectLockedEdit(document, layer, DR_EDIT_PIXELS)
        // Covers a multi-layer panel selection, as the UI's refusal does.
        if let reason = TransformSelectedPixels.refusal(document, layer: layer, target: .layer) {
            throw ToolError(message: reason)
        }
        guard let box = TransformSelectedPixels.box(doc, layer: layer, selection: selection) else {
            throw ToolError.selectedPixels(TransformSelectedPixels.emptyRefusal)
        }
        var fields: [String: Any] = [
            "target": "selected_pixels", "layer": layer,
            "sampler": EditorViewController.samplerAgentName(again.sampler),
        ]
        fields.merge(again.appliedFields) { _, new in new }
        if duplicate { fields["duplicate"] = true }
        let dropped = Self.description(of: document, layer: layer)
        guard try commitSelectedPixelsTransform(
            document, layer: layer, base: doc, history: document.selectionHistory,
            coverage: selection.maskSamples16(), bounds: selection.bounds,
            box: again.sourceRect(over: box), matrix: again.matrix, quad: again.quad,
            sampler: again.sampler, duplicate: duplicate, name: name)
        else { return try structureNoOp(fields, why: "the transform changed no pixel.") }
        document.lastFreeTransform = again
        var result = fields
        result["ok"] = true
        if let info = document.doc?.layerInfo(layer) {
            result["layer_bounds"] = ["x": info.offsetX, "y": info.offsetY,
                                      "width": info.width, "height": info.height]
        }
        if let bounds = document.canvasSelection?.bounds {
            result["selection_bounds"] = ["x": Int(bounds.minX), "y": Int(bounds.minY),
                                          "width": Int(bounds.width), "height": Int(bounds.height)]
        } else {
            result["selection_bounds"] = NSNull()
        }
        return try pixelEditResult(result, layer: layer, rasterized: dropped)
    }

    /// `transform_selected_pixels`' plane opening (lock, refusals, the
    /// selection's bounds, `background`), then its commit with the
    /// remembered map.
    private func transformAgainPlane(
        _ a: [String: Any], _ document: ImageDocument, _ doc: RasterDocument,
        selection: CanvasSelection, target: PaintTarget, again: RepeatableTransform,
        duplicate: Bool, name: String
    ) throws -> String {
        let layer = document.activeLayerIndex
        let opening = try planeTransformOpening(
            a, document, layer: layer, target: target, selection: selection,
            panelSelection: true)
        var fields: [String: Any] = [
            "target": target.agentName(in: doc), "background": opening.background.hex,
            "sampler": EditorViewController.samplerAgentName(again.sampler),
        ]
        if target == .plane(.alpha) { fields["background"] = nil }
        if target == .mask || target.isPlane { fields["layer"] = layer }
        fields.merge(again.appliedFields) { _, new in new }
        if duplicate { fields["duplicate"] = true }
        let dropped = target.isPlane ? Self.description(of: document, layer: layer) : nil
        guard try commitSelectedPixelsTransform(
            document, layer: layer, base: doc, history: document.selectionHistory,
            coverage: selection.maskSamples16(), bounds: selection.bounds,
            box: again.sourceRect(over: opening.box), matrix: again.matrix, quad: again.quad,
            sampler: again.sampler, duplicate: duplicate, name: name, target: target,
            background: opening.background)
        else { return try structureNoOp(fields, why: "the transform changed no pixel.") }
        document.lastFreeTransform = again
        var result = fields
        result["ok"] = true
        guard target.isPlane else {
            return try planeTransformResult(result, document, layer: layer, target: target)
        }
        if let info = document.doc?.layerInfo(layer) {
            result["layer_bounds"] = ["x": info.offsetX, "y": info.offsetY,
                                      "width": info.width, "height": info.height]
        }
        if let bounds = document.canvasSelection?.bounds {
            result["selection_bounds"] = ["x": Int(bounds.minX), "y": Int(bounds.minY),
                                          "width": Int(bounds.width), "height": Int(bounds.height)]
        } else {
            result["selection_bounds"] = NSNull()
        }
        return try pixelEditResult(result, layer: layer, rasterized: dropped)
    }

    private func transformAgainLayers(
        _ document: ImageDocument, _ doc: RasterDocument, again: RepeatableTransform,
        name: String, authorize: Bool
    ) throws -> String {
        let roots = document.selectedLayerIndices
        try rejectLockedMove(document, roots)
        let expanded = doc.movingSet(roots)
        let samplerName = EditorViewController.samplerAgentName(again.sampler)
        // One described layer at 8 bits composes an AFFINE memory into its
        // description, as ⌘T and transform_layer do.
        if !doc.isHighDepth, expanded.count == 1, again.perspective == nil,
           let composed = try transformDescribedLayer(
               document, layer: expanded[0], matrix: again.matrix,
               sampler: (samplerName, again.sampler),
               applied: ["matrix": again.matrixArray, "sampler": samplerName], name: name) {
            document.lastFreeTransform = again
            return composed
        }
        let described = doc.describedEntries(in: expanded).map(\.layer)
        // The horizon preflight comes first, as in the UI (before any prompt).
        if let reason = again.replayRefusal(source: doc, layers: roots) {
            throw ToolError(message: reason)
        }
        if again.perspective != nil, !authorize, !described.isEmpty {
            throw ToolError(message: Self.truePerspectiveRefusal)
        }
        let work = try TransformWork(
            source: doc, layers: roots, quad: again.quad, matrix: again.matrix,
            sampler: again.sampler, droppingDescriptions: described,
            sourceRect: again.perspective?.sourceRect, rasterizeArtwork: authorize)
        var fields: [String: Any] = [
            "target": "layers", "layers": expanded, "sampler": samplerName,
        ]
        fields.merge(again.appliedFields) { _, new in new }
        guard let prepared = try work.wait(in: document, title: name) else {
            return try structureNoOp(fields, why: "the transform changed no pixel.")
        }
        document.preparedEdit = prepared
        try performGroupedEdit(document, name) { _ in prepared.document }
        document.lastFreeTransform = again
        var result = fields
        result["ok"] = true
        result["changed"] = true
        result["rasterized_layers"] = described.filter { document.doc?.layerMeta($0) == nil }
        result["document"] = summary(document)
        return try jsonResult(result)
    }

    /// Batch 55's `transform_layers duplicate` branch with the
    /// remembered matrix and the active layer as the primary; the copies take
    /// its described rules — one 8-bit copy under an affine memory
    /// composes into its description, as ⌥⇧⌘T does, and a perspective memory
    /// over described copies needs `rasterize_artwork`.
    private func transformAgainCopy(
        _ document: ImageDocument, _ doc: RasterDocument, again: RepeatableTransform,
        name: String, authorize: Bool
    ) throws -> String {
        let roots = document.selectedLayerIndices
        // The copies are unlinked: the originals' SUBTREE Position block is
        // the rule, never `movingSet`.
        for root in roots { try rejectLockedOffset(document, root) }
        guard let copy = try DuplicateTransform(
            base: doc, originals: roots, primary: document.activeLayerIndex) else {
            throw ToolError(message: "The copy could not be made.")
        }
        let samplerName = EditorViewController.samplerAgentName(again.sampler)
        if let reason = again.replayRefusal(source: copy.source, layers: copy.copies) {
            throw ToolError(message: reason)
        }
        if again.perspective == nil,
           let composed = try composedCopy(copy, affine: again.matrix, sampler: again.sampler) {
            try publishComposedCopy(document, copy, composed, name: name)
            document.lastFreeTransform = again
            return try jsonResult([
                "ok": true, "changed": true, "target": "layers", "duplicate": true,
                "copies": copy.copies, "layers": copy.set, "sampler": samplerName,
                "rasterized_layers": [Int](), "document": summary(document),
            ].merging(again.appliedFields) { _, new in new })
        }
        let described = copy.source.describedEntries(in: copy.set).map(\.layer)
        if again.perspective != nil, !authorize, !described.isEmpty {
            throw ToolError(message: Self.truePerspectiveRefusal)
        }
        let work = try TransformWork(
            source: copy.source, layers: copy.copies, quad: again.quad, matrix: again.matrix,
            sampler: again.sampler, droppingDescriptions: described,
            sourceRect: again.perspective?.sourceRect, rasterizeArtwork: authorize)
        guard let prepared = try work.wait(in: document, title: name, base: doc) else {
            return try structureNoOp(
                ["target": "layers", "duplicate": true, "copies": [Int](), "layers": [Int](),
                 "sampler": samplerName].merging(again.appliedFields) { _, new in new },
                why: "the transform changed no pixel.")
        }
        document.preparedEdit = prepared
        try performGroupedEdit(document, name) { _ in prepared.document }
        document.lastFreeTransform = again
        retarget(document, copy.selection)
        let rasterized = described.filter { document.doc?.layerMeta($0) == nil }
        return try jsonResult([
            "ok": true, "changed": true, "target": "layers", "duplicate": true,
            "copies": copy.copies, "layers": copy.set,
            "sampler": samplerName, "rasterized_layers": rasterized, "document": summary(document),
        ].merging(again.appliedFields) { _, new in new })
    }
}
