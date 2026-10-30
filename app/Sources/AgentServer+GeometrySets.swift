import AppKit

// MCP `transform_layers duplicate` is the twin of Layer ▸ Free Transform a Copy (⌥⌘T) with no pixel
// selection (`TransformWork.wait(…, base:)`, reply `copies`): at 8 bits one copy under an affine or
// parallelogram composes first (`composedCopy`), otherwise the non-duplicate branch's set rules
// (`rasterize_artwork`, `rasterized_layers` naming the copies).

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let geometrySetsHandlers: [String: ToolHandler] = [
        "transform_layers": { $0.transformLayersSet },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let geometrySetsToolFacts: [String: ToolFacts] = [
        "transform_layers": [.typedTool, .floatTool, .cmykTool],
    ]

    /// A true perspective over editable artwork without authorization:
    /// `transform_layers`' refusal, and `transform_again`'s for a remembered
    /// perspective.
    static let truePerspectiveRefusal =
        "True perspective rasterizes editable artwork; pass rasterize_artwork:true to authorize it."

    /// Mirrors the captured multi-layer Free Transform handles in
    /// EditorViewController+PrecisionGeometryGestures and TransformJobs.
    func transformLayersSet(_ arguments: [String: Any]) throws -> String {
        let document = try target(arguments)
        let roots = try layerIndices(arguments, document)
        guard let source = document.doc else { throw ToolError(message: "No image loaded.") }
        let expanded = source.movingSet(roots)
        let duplicate = boolArg(arguments, "duplicate") ?? false
        if duplicate {
            // The copies are unlinked: the originals' SUBTREE Position block
            // is the rule, never `movingSet`.
            for root in roots { try rejectLockedOffset(document, root) }
        } else {
            try rejectLockedMove(document, roots)
        }
        let hasMatrix = arguments["matrix"] != nil
        let hasRect = arguments["source_rect"] != nil
        let hasCorners = arguments["corners"] != nil
        guard hasMatrix != (hasRect || hasCorners), !hasMatrix || (!hasRect && !hasCorners),
              !hasRect || hasCorners else {
            throw ToolError(message:
                "transform_layers requires exactly matrix or source_rect with corners.")
        }
        // The recorded pivot only steers a relative Action replay's
        // remap; it never changes the pixels computed here.
        if let pivot = arguments["pivot"] {
            guard hasMatrix else {
                throw ToolError(message:
                    "pivot goes with matrix: it is the point the matrix was built around.")
            }
            guard let pair = pivot as? [Any], pair.count == 2 else {
                throw ToolError(message: "pivot needs exactly 2 numbers.")
            }
            guard pair.allSatisfy({ ActionJSON.number($0)?.doubleValue.isFinite == true }) else {
                throw ToolError(message: "pivot numbers must be finite.")
            }
        }
        let samplerName = stringArg(arguments, "sampler") ?? "bicubic"
        let sampler: DrResizeFilter
        switch samplerName {
        case "nearest": sampler = DR_FILTER_NEAREST
        case "bilinear": sampler = DR_FILTER_BILINEAR
        case "bicubic": sampler = DR_FILTER_CATMULL_ROM
        case "lanczos": sampler = DR_FILTER_LANCZOS3
        default: throw ToolError(message: "sampler must be nearest, bilinear, bicubic or lanczos.")
        }
        let authorize = boolArg(arguments, "rasterize_artwork") ?? false
        let request: GeometrySetRequest
        let rect: CGRect?
        if hasMatrix {
            let values = try numericArray(arguments["matrix"], count: 6, name: "matrix")
            request = .affine(layers: roots, matrix: CGAffineTransform(
                a: values[0], b: values[1], c: values[2], d: values[3],
                tx: values[4], ty: values[5]), sampler: sampler)
            rect = nil
        } else {
            let values = try numericArray(arguments["source_rect"], count: 4,
                                          name: "source_rect")
            guard values[2] > 0, values[3] > 0,
                  let rows = arguments["corners"] as? [Any], rows.count == 4 else {
                throw ToolError(message: "source_rect needs positive dimensions and four corners.")
            }
            let corners = try rows.map { row -> CGPoint in
                let pair = try numericArray(row, count: 2, name: "corner")
                return CGPoint(x: pair[0], y: pair[1])
            }
            let sourceRect = CGRect(x: values[0], y: values[1],
                                    width: values[2], height: values[3])
            request = .perspective(layers: roots, sourceRect: sourceRect,
                                   quad: corners, sampler: sampler)
            rect = sourceRect
        }
        let truePerspective: Bool
        if case let .perspective(_, sourceRect, corners, _) = request {
            truePerspective = LayerTransform.parallelogramAffine(
                of: sourceRect, onto: corners) == nil
        } else {
            truePerspective = false
        }
        if duplicate {
            return try transformLayersCopy(document, source: source, roots: roots,
                                           request: request, rect: rect, sampler: sampler,
                                           truePerspective: truePerspective, authorize: authorize)
        }
        let described = source.describedEntries(in: expanded).map(\.layer)
        if truePerspective && !authorize && !described.isEmpty {
            throw ToolError(message: Self.truePerspectiveRefusal)
        }
        // #1 Indexed phase 2d: an Indexed document's admitted plan, read before the edit
        // (`scripts/indexed-admission/measure.py`'s `transform-indexed`); since Bitmap phase
        // 2a-ii a Bitmap document's too (Bitmap phase 2b then refused
        // `transform_layers` on a Bitmap document, `BitmapDocument.disabledTools`).
        let indexedPlan = source.isIndexed || source.isBitmap
            ? IndexedDocument.geometryPlanBytes(source, try request.command()) : nil
        let work = try TransformWork(source: source, layers: roots,
            quad: { if case let .perspective(_, _, corners, _) = request { return corners }; return nil }(),
            matrix: { if case let .affine(_, matrix, _) = request { return matrix }; return .identity }(),
            sampler: sampler, droppingDescriptions: described,
            sourceRect: rect, rasterizeArtwork: authorize)
        guard let prepared = try work.wait(in: document, title: "Transform Layers") else {
            return try jsonResult(["ok": true, "changed": false, "layers": expanded,
                                   "rasterized_layers": [], "document": summary(document)])
        }
        document.preparedEdit = prepared
        try performGroupedEdit(document, "Transform Layers") { _ in prepared.document }
        document.lastFreeTransform = Self.repeatable(request)
        let rasterized = described.filter { document.doc?.layerMeta($0) == nil }
        var reply: [String: Any] = ["ok": true, "changed": true, "layers": expanded,
                                    "rasterized_layers": rasterized, "document": summary(document)]
        if let indexedPlan { reply["working_memory_plan_bytes"] = indexedPlan }
        return try jsonResult(reply)
    }

    /// Mirrors Layer ▸ Free Transform a Copy with no selection
    /// (EditorViewController+TransformCopy): duplicate `roots` and transform the
    /// COPIES as ONE "Transform Copy" step; the copies become the selection.
    /// An identity matrix, or corners that are the source rectangle's own,
    /// duplicates nothing — the UI's unchanged box. The active layer's copy
    /// stays the active layer when the active layer is one of `roots` (the
    /// UI's `primary` rule); otherwise the topmost root's copy is.
    ///
    /// The copies take the UI's described rules: at 8 bits one copy
    /// under an affine or a parallelogram composes into its own description
    /// (the ⌥⌘T gate's compose, `transformingDescribedLayer`), so a replayed
    /// ⌥⌘T keeps what the user kept; otherwise the non-duplicate branch's set
    /// rules — a true perspective over described copies needs
    /// `rasterize_artwork`, and `rasterized_layers` names the copies whose
    /// description was dropped.
    private func transformLayersCopy(
        _ document: ImageDocument, source: RasterDocument, roots: [Int],
        request: GeometrySetRequest, rect: CGRect?, sampler: DrResizeFilter,
        truePerspective: Bool, authorize: Bool
    ) throws -> String {
        let name = DuplicateTransform.actionName
        let unchanged: Bool
        switch request {
        case let .affine(_, matrix, _):
            unchanged = matrix.isIdentity
        case let .perspective(_, sourceRect, corners, _):
            unchanged = LayerTransform.parallelogramAffine(
                of: sourceRect, onto: corners)?.isIdentity == true
        }
        if unchanged {
            return try jsonResult(["ok": true, "changed": false, "duplicate": true,
                                   "copies": [Int](), "layers": [Int](),
                                   "rasterized_layers": [], "document": summary(document)])
        }
        guard let copy = try DuplicateTransform(
            base: source, originals: roots,
            primary: roots.contains(document.activeLayerIndex)
                ? document.activeLayerIndex : (roots.last ?? 0)) else {
            throw ToolError(message: "The copy could not be made.")
        }
        let affine: CGAffineTransform?
        switch request {
        case let .affine(_, matrix, _):
            affine = matrix
        case let .perspective(_, sourceRect, corners, _):
            affine = LayerTransform.parallelogramAffine(of: sourceRect, onto: corners)
        }
        if let affine, let composed = try composedCopy(copy, affine: affine, sampler: sampler) {
            try publishComposedCopy(document, copy, composed, name: name)
            document.lastFreeTransform = Self.repeatable(request)
            return try jsonResult(["ok": true, "changed": true, "duplicate": true,
                                   "copies": copy.copies, "layers": copy.set,
                                   "rasterized_layers": [Int](), "document": summary(document)])
        }
        let described = copy.source.describedEntries(in: copy.set).map(\.layer)
        if truePerspective && !authorize && !described.isEmpty {
            throw ToolError(message: Self.truePerspectiveRefusal)
        }
        let work = try TransformWork(source: copy.source, layers: copy.copies,
            quad: { if case let .perspective(_, _, corners, _) = request { return corners }; return nil }(),
            matrix: { if case let .affine(_, matrix, _) = request { return matrix }; return .identity }(),
            sampler: sampler, droppingDescriptions: described, sourceRect: rect,
            rasterizeArtwork: authorize)
        guard let prepared = try work.wait(in: document, title: name, base: source) else {
            return try jsonResult(["ok": true, "changed": false, "duplicate": true,
                                   "copies": [Int](), "layers": [Int](),
                                   "rasterized_layers": [], "document": summary(document)])
        }
        document.preparedEdit = prepared
        try performGroupedEdit(document, name) { _ in prepared.document }
        document.lastFreeTransform = Self.repeatable(request)
        retarget(document, copy.selection)
        let rasterized = described.filter { document.doc?.layerMeta($0) == nil }
        return try jsonResult(["ok": true, "changed": true, "duplicate": true,
                               "copies": copy.copies, "layers": copy.set,
                               "rasterized_layers": rasterized, "document": summary(document)])
    }

    /// The ⌥⌘T / ⌥⇧⌘T gate's compose for MCP: at 8 bits, ONE copy
    /// (no subtree) under `affine` composes into its own description
    /// (`transformingDescribedLayer` over the copy's handle). nil when it does
    /// not compose — a plain copy, a set, 16 bits, a description that cannot
    /// render — and the caller's `TransformWork` runs.
    func composedCopy(
        _ copy: DuplicateTransform, affine: CGAffineTransform, sampler: DrResizeFilter
    ) throws -> RasterDocument? {
        guard !copy.source.isHighDepth, copy.copies.count == 1, copy.set.count == 1 else {
            return nil
        }
        do {
            return try copy.source.transformingDescribedLayer(
                copy.copies[0], affine, sampler: sampler)
        } catch {
            throw error as? ToolError ?? ToolError(message: error.localizedDescription)
        }
    }

    /// Publishes `composedCopy`'s document as the one `name` step over the
    /// copy's base, then selects the copies (after publication, as the UI).
    func publishComposedCopy(
        _ document: ImageDocument, _ copy: DuplicateTransform, _ composed: RasterDocument,
        name: String
    ) throws {
        guard document.doc === copy.base else {
            throw ToolError(message: "The document changed before the transform started.")
        }
        try performGroupedEdit(document, name) { _ in composed }
        retarget(document, copy.selection)
    }

    /// Transform Again's memory: `.affine`'s matrix, or `.perspective`'s
    /// corners over its source rectangle — a parallelogram as its affine, a
    /// true perspective as its corners.
    private static func repeatable(_ request: GeometrySetRequest) -> RepeatableTransform? {
        switch request {
        case let .affine(_, matrix, sampler):
            return RepeatableTransform(matrix: matrix, sampler: sampler)
        case let .perspective(_, sourceRect, corners, sampler):
            return RepeatableTransform(matrix: .identity, quad: corners, sourceRect: sourceRect,
                                       sampler: sampler)
        }
    }

    private func numericArray(_ value: Any?, count: Int, name: String) throws -> [Double] {
        guard let values = value as? [Any], values.count == count else {
            throw ToolError(message: "\(name) needs exactly \(count) numbers.")
        }
        return try values.map { value in
            guard let number = value as? NSNumber, number.doubleValue.isFinite else {
                throw ToolError(message: "\(name) numbers must be finite.")
            }
            return number.doubleValue
        }
    }
}
