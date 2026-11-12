import AppKit

extension AgentServer.DroppedDescription {
    /// The report's name for the kind a layer's meta claims — what
    /// `performPixelEdit` drops, decodable or not (DescribedLayer.swift,
    /// `LayerDescription.claimedKind`).
    init(_ kind: DescribedKind) {
        switch kind {
        case .text: self = .text
        case .shape: self = .shape
        case .fill: self = .fill
        }
    }
}

/// The agent's Free Transform commits, affine and perspective, on plain and
/// described layers: distort_layer (the mirror of ⌘-dragging a Free
/// Transform corner — the layer's rect mapped corner-for-corner onto an
/// explicit quad, committed through the same perspective op the interactive
/// session uses, so identical corners give identical pixels; a
/// PARALLELOGRAM quad is the exact affine it denotes and, on a described
/// layer, composes like transform_layer) and the described-layer compose
/// path transform_layer (AgentServer.swift) takes first — the mirror of
/// EditorViewController+DescribedTransform.swift's commit, the branch
/// commitTransformSession takes for a plain affine session on a text
/// or shape layer.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let distortHandlers: [String: ToolHandler] = [
        "distort_layer": { $0.distortLayer },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let distortToolFacts: [String: ToolFacts] = [
        // Phase 12F-1b: the EXACT canvas geometry. A zero-angle `crop` keeps
        // every raster and mask allocation; a quarter turn and a flip are a
        // Nearest PERMUTATION, so an extended-range component is only ever
        // copied. The FILTERED geometry is admitted too — a `crop`
        // with an angle (straighten), an arbitrary `rotate`, `image_size`,
        // the layer transforms and `transform_mask` — through the core's
        // premultiplied extended-range resampler, which clamps each RGB
        // component to the layer's own visible range (D3). `transform_again`
        // repeats a layer transform; with a selection it would transform the
        // selected PIXELS, which float does not admit, so
        // `DocumentPrecision.refusal` refuses that case by name.
        "distort_layer": [.typedTool, .floatTool],
    ]

    /// transform_layer on a DESCRIBED layer: composes `matrix` into the
    /// description and re-renders (RasterDocument.transformingDescribedLayer)
    /// as one undo step, reporting `rasterized: false` with the new bounds,
    /// transform and origin. FALL-THROUGH contract: returns nil whenever it
    /// did not compose (no description, a source that cannot render right
    /// now, a singular composed map, a raster past the core's cap); the
    /// caller then runs the rasterizing path, which reports as before. A
    /// refusal the compose THREW (a typed artwork or transform refusal, or a
    /// busy background worker) is rethrown as a `ToolError` carrying the
    /// core's sentence instead of falling through.
    func transformDescribedLayer(
        _ document: ImageDocument, layer index: Int, matrix: CGAffineTransform,
        sampler: (name: String, filter: DrResizeFilter), applied: [String: Any],
        name: String = "Transform Layer"
    ) throws -> String? {
        guard let doc = document.doc else { return nil }
        let prepared: RasterDocument?
        do {
            prepared = try doc.transformingDescribedLayer(index, matrix, sampler: sampler.filter)
        } catch {
            throw error as? ToolError ?? ToolError(message: error.localizedDescription)
        }
        guard let composed = prepared else { return nil }
        // Cannot throw for a refusal: `composed` is already the result.
        try performGroupedEdit(document, name) { _ in composed }
        document.lastFreeTransform = RepeatableTransform(matrix: matrix, sampler: sampler.filter)
        let after = document.doc
        let info = after?.layerInfo(index)
        var result: [String: Any] = [
            "ok": true,
            "layer": index,
            // Where the layer landed: the outward-rounded box of the source
            // quad under the composed map, so a render can be checked
            // against it.
            "bounds": [
                "x": info?.offsetX ?? 0, "y": info?.offsetY ?? 0,
                "width": info?.width ?? 0, "height": info?.height ?? 0,
            ],
            "applied": applied,
            "rasterized": false,
            "note": "The layer stays re-editable: its description absorbed the transform.",
        ]
        // The transform and origin read back from the committed document —
        // what get_document will report.
        if let description = after?.layerDescription(index) {
            result["transform"] = description.transform.array
        }
        if let origin = after?.describedAnchor(index) {
            result["origin"] = ["x": Double(origin.x), "y": Double(origin.y)]
        }
        return try jsonResult(result)
    }

    /// Adds the report for descriptions dropped from entries OTHER than the
    /// one the call named — a transformed group's children, a link partner.
    ///
    /// `pixelEditResult`'s `rasterized` covers the TARGET's own description;
    /// this covers the rest of the set, and exists for the same reason: a set
    /// transform resamples every member, so every described member loses its
    /// description in the same edit, and a model that cannot see what it lost
    /// cannot put it back. The sentence is appended to whatever note the
    /// target's own report already wrote, so a reply carries one note.
    static func appendSetRasterization(
        _ result: inout [String: Any], _ dropped: [(layer: Int, kind: DescribedKind)]
    ) {
        guard !dropped.isEmpty else { return }
        result["rasterized_layers"] = dropped.map {
            ["layer": $0.layer, "kind": $0.kind.rawValue] as [String: Any]
        }
        let list = dropped.map { "\($0.layer) (\($0.kind.rawValue))" }.joined(separator: ", ")
        let sentence =
            "This edit resampled every layer the set expands to, so the description on layer "
            + "\(list) was dropped as well — those pixels are no longer its rendering, and "
            + "edit_text_layer / edit_shape_layer / edit_fill_layer no longer work on them. The pixels are "
            + "intact; undo restores the descriptions."
        result["note"] = (result["note"] as? String).map { $0 + " " + sentence } ?? sentence
    }

    /// Why a transform on a described layer had to rasterize, when it was
    /// because the description could not render right now (a text family
    /// not installed here) or
    /// because the pixels are not its rendering any more (an earlier
    /// version's Image Size resampled them, `describedRasterIsStale`) —
    /// appended to the rasterization note, so the model learns the layer
    /// was not resampled on a whim. nil for a plain layer, a shape that
    /// could render, or a description that could render over its own
    /// raster — then the compose path was refused for a reason the note
    /// already words (a singular map, a raster past the cap) or the quad
    /// was a true perspective.
    static func unrenderableReason(
        _ document: ImageDocument, layer idx: Int, before doc: RasterDocument
    ) -> String? {
        guard let description = doc.layerDescription(idx) else { return nil }
        guard !description.isRenderable else {
            guard doc.describedRasterIsStale(idx) else { return nil }
            return "It rasterized because its pixels were no longer the rendering of its "
                + "description (an earlier version's Image Size resampled them while the "
                + "description kept its original size), so composing into the description "
                + "would have re-rendered the layer at a size nobody saw."
        }
        switch description {
        case let .text(payload):
            return "It rasterized because its font family “\(payload.font)” is not installed "
                + "here, so re-rendering it would have substituted a face. Undo, then "
                + "replace_missing_fonts replaces it."
        case .shape, .fill:
            return nil
        }
    }

    /// Maps layer's rect onto four canvas corners in ONE resample. Mirrors
    /// the UI's ⌘-corner Free Transform drag: same core op, same refusal
    /// surface (pre-named here so the model can self-correct), one undo
    /// step, and — like transform_layer — adjustment layers are allowed:
    /// moving their mask footprint is meaningful. A PARALLELOGRAM quad on a
    /// described layer is the exact affine it denotes and composes into the
    /// description like transform_layer (`rasterized: false`, the layer
    /// stays re-editable); a true perspective quad — or a description that
    /// cannot render right now — resamples the pixels and drops the
    /// description silently in the same edit, reported with the reason.
    ///
    /// A GROUP is a legitimate target — the core maps every descendant
    /// through the same quad — so this takes `structuralLayerIndex`, and the
    /// rect the corners are the destinations OF is then the group's CONTENT
    /// box, read from `layerBounds`. Not `layerInfo`: on a group those four
    /// keys answer the union of the descendants' pixel BUFFER rects, which is
    /// the whole canvas the moment a child is a paste, a text layer or a shape
    /// layer, and mapping the corners relative to that rect would apply an
    /// affine nobody asked for.
    ///
    /// Like every transform this is a POSITION edit, never a pixel one
    /// (`doc_lock.rs`): a transparency-locked layer still distorts, and a
    /// position lock — the group's own, any descendant's, or a link partner's
    /// — refuses the call BY NAME, over exactly the set the commit will write.
    func distortLayer(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let index = try structuralLayerIndex(a, document)
        guard let doc = document.doc, let info = doc.layerInfo(index) else {
            throw ToolError(message: "Layer \(index) could not be read")
        }
        let isGroup = doc.layerIsGroup(index)
        // LINKED partners transform with the layer whatever the selection
        // (§3F), so a linked raster entry and a group both commit through the
        // one set branch below — an affine or a true perspective alike,
        // `performTransformJob` with the `sourceRect` the corners map.
        let partners = doc.movingSet([index]).filter { $0 != index }
        let isLinked = !isGroup && !partners.isEmpty
        // A group commits through `transform_layers`, which fans out to the
        // whole subtree AND to every link group it touches; a lone plain
        // layer commits through the single-entry perspective (or compose)
        // path. The refusal is asked over the set that will really be
        // written, so it names the entry whose lock actually stopped it — a
        // group carrying no lock of its own used to be told to clear one,
        // which is a documented no-op, leaving no way out of the message, and
        // a position-locked link PARTNER used not to refuse the call at all.
        if isGroup || isLinked {
            try rejectLockedMove(document, [index])
        } else {
            try rejectLockedEdit(document, index, DR_EDIT_POSITION)
        }
        let rect: CGRect
        if isGroup {
            guard let box = doc.layerBounds(index) else {
                throw ToolError(
                    message: "Layer \(index) (\"\(info.name)\") is a group with nothing opaque "
                        + "inside it, so there is nothing to distort.")
            }
            rect = CGRect(
                x: CGFloat(box.x), y: CGFloat(box.y),
                width: CGFloat(box.width), height: CGFloat(box.height))
        } else {
            guard info.width > 0, info.height > 0 else {
                throw ToolError(
                    message: "Layer \(index) (\"\(info.name)\") has no pixels to distort.")
            }
            rect = CGRect(
                x: CGFloat(info.offsetX), y: CGFloat(info.offsetY),
                width: CGFloat(info.width), height: CGFloat(info.height))
        }
        let corners = try cornerPoints(a)

        let samplerName = (stringArg(a, "sampler") ?? "bicubic").lowercased()
        guard let sampler = Self.transformSamplers[samplerName] else {
            throw ToolError(
                message: "Unknown sampler \"\(samplerName)\". Use nearest, bilinear, "
                    + "bicubic, or lanczos.")
        }

        // The same live checks the interactive drag clamp enforces, named:
        // the core answers only NULL, so every refusal it would give is
        // diagnosed here first.
        let identity = zip(corners, LayerTransform.rectCorners(rect)).allSatisfy {
            abs($0.x - $1.x) <= LayerTransform.cornerEpsilon
                && abs($0.y - $1.y) <= LayerTransform.cornerEpsilon
        }
        guard !identity else {
            throw ToolError(
                message: "The corners match the layer's current rect — nothing to change.")
        }
        guard LayerTransform.isUsableQuad(corners) else {
            throw ToolError(
                message: "The corners must form a convex quad in the order top-left, "
                    + "top-right, bottom-right, bottom-left: a concave, self-crossing or "
                    + "collapsed arrangement folds the mapping and the core refuses it.")
        }
        let extent = LayerTransform.boundingExtent(of: corners)
        guard LayerTransform.extentIsCommittable(extent) else {
            throw ToolError(
                message: "The distorted layer would be \(Int(extent.width))×"
                    + "\(Int(extent.height)) px — collapsed, outside the coordinates the "
                    + "core can address, or past its 100 megapixel ceiling for one layer. "
                    + "Keep the corners nearer the canvas.")
        }

        let applied: [String: Any] = [
            "corners": corners.map {
                [Self.transformNumber(Double($0.x)), Self.transformNumber(Double($0.y))]
            },
            "sampler": sampler.name,
        ]
        if isGroup || isLinked || doc.isHighDepth || doc.hasRetainedSource(index) {
            let affine = LayerTransform.parallelogramAffine(of: rect, onto: corners)
            let truePerspective = affine == nil
            let described = doc.describedEntries(in: doc.movingSet([index])).map(\.layer)
            let authorize = boolArg(a, "rasterize_artwork") ?? false
            if truePerspective && !authorize && !described.isEmpty {
                throw ToolError(message: "True perspective rasterizes editable artwork; pass rasterize_artwork:true to authorize it.")
            }
            try performTransformJob(document, "Distort Layer", layers: [index],
                pixelLayer: index, matrix: affine ?? .identity,
                quad: truePerspective ? corners : nil, sampler: sampler.filter,
                sourceRect: rect, rasterizeArtwork: authorize)
            let bounds: [String: Any]
            if isGroup {
                // A group's rect is its CONTENT box, so its bounds are that
                // same box afterwards — never `layerInfo`'s union of buffers.
                let landed = document.doc?.layerBounds(index)
                bounds = ["x": landed?.x ?? 0, "y": landed?.y ?? 0,
                          "width": landed?.width ?? 0, "height": landed?.height ?? 0]
            } else {
                let after = document.doc?.layerInfo(index)
                bounds = ["x": after?.offsetX ?? 0, "y": after?.offsetY ?? 0,
                          "width": after?.width ?? 0, "height": after?.height ?? 0]
            }
            var reply: [String: Any] = [
                "ok": true, "layer": index, "applied": applied,
                "bounds": bounds,
                "rasterized": described.contains(index)
                    && document.doc?.layerMeta(index) == nil,
                "rasterized_layers": described.filter {
                    document.doc?.layerMeta($0) == nil
                },
                "document": summary(document),
            ]
            if isGroup {
                reply["note"] = "A group has no pixels of its own: the map those corners "
                    + "denote was applied to every layer inside it and to anything LINKED to "
                    + "one of them (linked masks follow). corners are the destinations of "
                    + "the group's CONTENT box (get_document's content_x/y/width/height), and "
                    + "bounds is that same content box afterwards."
            }
            if isLinked { reply["linked_layers"] = partners }
            return try jsonResult(reply)
        }
        // A parallelogram on a described layer composes as the exact affine
        // it is — the same compose-or-fall-through rule as transform_layer:
        // nil means it did not compose (a plain layer needs no Swift-side
        // help, since the core's perspective op already delegates a
        // parallelogram to its lossless affine path; a description that
        // cannot render, a singular map or a core refusal fall through) and
        // the rasterizing path below applies and reports as before.
        if Self.description(of: document, layer: index) != nil,
           let affine = LayerTransform.parallelogramAffine(of: rect, onto: corners),
           let composed = try transformDescribedLayer(
               document, layer: index, matrix: affine, sampler: sampler, applied: applied) {
            return composed
        }

        let rasterized: DroppedDescription?
        do {
            // POSITION, not PIXELS: a distortion resamples the whole buffer
            // including its alpha, so a transparency lock has nothing to
            // freeze and must not refuse (the same rule transform_layer
            // follows — see performPixelEdit's `lockKind`).
            rasterized = try performTransformJob(
                document, "Distort Layer", layers: [index], pixelLayer: index,
                matrix: .identity, quad: corners, sampler: sampler.filter,
                sourceRect: rect, rasterizeArtwork: boolArg(a, "rasterize_artwork") ?? false)
        } catch is ToolError {
            throw ToolError(
                message: "The core refused this distortion: the quad is too close to "
                    + "degenerate. Move the corners apart and keep the quad convex.")
        }

        let after = document.doc?.layerInfo(index)
        return try pixelEditResult(
            [
                "ok": true,
                "layer": index,
                // Where the layer landed: the outward-rounded bounding box
                // of the corners, so a render can be checked against it.
                "bounds": [
                    "x": after?.offsetX ?? 0, "y": after?.offsetY ?? 0,
                    "width": after?.width ?? 0, "height": after?.height ?? 0,
                ],
                "applied": applied,
            ], layer: index, rasterized: rasterized,
            reason: Self.unrenderableReason(document, layer: index, before: doc))
    }

    /// The `corners` argument: four [x, y] pairs (numbers, or the string
    /// forms every argument helper accepts), in the source rect's corner
    /// order TL, TR, BR, BL.
    func cornerPoints(_ a: [String: Any], subject: String = "the layer rect's") throws
        -> [CGPoint]
    {
        let usage =
            "corners must be four [x, y] canvas points — the destinations of \(subject) "
            + "top-left, top-right, bottom-right and bottom-left corners, e.g. "
            + "[[0, 0], [80, 10], [75, 60], [5, 50]]."
        guard let raw = a["corners"] as? [Any], raw.count == 4 else {
            throw ToolError(message: usage)
        }
        return try raw.map { entry in
            guard let pair = entry as? [Any], pair.count == 2,
                  let x = Self.finiteNumber(pair[0]), let y = Self.finiteNumber(pair[1])
            else { throw ToolError(message: usage) }
            return CGPoint(x: x, y: y)
        }
    }

    /// A finite Double out of a JSON number or its string form — the nested
    /// twin of doubleArg, which only reads top-level keys. Shared with the
    /// `transform` argument parser (AgentServer+Text.swift).
    static func finiteNumber(_ value: Any) -> Double? {
        let number: Double?
        if let d = value as? Double {
            number = d
        } else if let i = value as? Int {
            number = Double(i)
        } else if let s = value as? String {
            number = Double(s)
        } else {
            number = nil
        }
        guard let number, number.isFinite else { return nil }
        return number
    }
}
