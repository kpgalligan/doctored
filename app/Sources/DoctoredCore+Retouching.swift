import AppKit

extension RasterDocument {
    // MARK: - Retouching

    /// The two-tier result of a core op that reports through `err_out`: NULL
    /// WITH a message is an error, NULL WITHOUT one is a plain refusal
    /// (nothing would change), and a handle is the new document. The idiom
    /// `withLayerStyle` spells out inline, shared by the retouching wrappers
    /// below — a non-NULL handle never comes back with a message, so nothing
    /// is leaked by not reading `err` on that path.
    func fallible(
        _ handle: OpaquePointer?, _ err: UnsafeMutablePointer<CChar>?, _ fallback: String
    ) throws -> RasterDocument? {
        guard let handle = handle else {
            guard err != nil else { return nil }
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: fallback))
        }
        return RasterDocument(owning: handle)
    }

    /// The canvas-sized straight-RGBA8 SAMPLING PLATE a retouch gesture reads
    /// from, built by the core from this document's own layers. The one
    /// answer to "what is under the brush": no host path may sample a display
    /// image, and this is what replaces having to.
    ///
    /// nil for an unknown mode or, for `.current`, an index that names no
    /// raster layer; throws the core's message on a rendering, reservation or
    /// cancellation refusal.
    func retouchSource(
        _ idx: Int, sample: RetouchSampling, ignoreAdjustments: Bool,
        control: RasterJobControl? = nil
    ) throws -> RasterImage? {
        guard isValidIndex(idx) else { return nil }
        var err: UnsafeMutablePointer<CChar>? = nil
        let token = try control ?? RasterJobControl()
        if control == nil {
            let estimate = sample == .current ? UInt64(width * height) * 4 + 4096
                : try projectionScratchEstimate() + UInt64(width * height) * 4 + 4096
            let available = try RasterResources.shared.admit(scratch: estimate)
            guard token.setScratchLimit(available + estimate) else {
                throw DoctoredCoreError(message: "Could not admit the captured retouch source.")
            }
        }
        let handle = dr_doc_retouch_source(
            ptr, idx, sample.code, ignoreAdjustments, token.ptr, &err)
        guard let handle else {
            if let err { throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "")) }
            return nil
        }
        return RasterImage(owning: handle)
    }

    /// Clone Stamp onto a NEW retouch layer directly above `idx`, leaving
    /// `idx`'s pixels exactly as they were. One core call, so one Undo.
    func cloneToNewLayer(
        _ idx: Int, overlay data: UnsafePointer<UInt8>, w: Int, h: Int, alpha: Double,
        name: String
    ) throws -> RasterDocument? {
        guard isValidIndex(idx), w == width, h == height, alpha.isFinite else { return nil }
        var err: UnsafeMutablePointer<CChar>? = nil
        let handle = dr_doc_clone_to_new_layer(
            ptr, idx, data, UInt32(w), UInt32(h), Float(alpha), name, &err)
        return try fallible(handle, err, "The clone could not be applied to a new layer.")
    }

    /// Removes flash red inside a canvas rect on layer `idx`: red dominance
    /// gated by saturation and hue, corrected only where a scoring component
    /// fits within `pupilSize` (a fraction, 1.0 = the default) of the rect's
    /// SHORTER side and does not reach all four of its sides. `darken` is
    /// 0…1 (0.5 = the default). nil on an empty or off-canvas rect,
    /// on nothing red inside it, on no component the rect contains and
    /// admits under `pupilSize`, or when nothing would change.
    func redEyeLayer(
        _ idx: Int, rect: CGRect, pupilSize: Double, darken: Double
    ) -> RasterDocument? {
        guard isValidIndex(idx), !rect.isNull, !rect.isInfinite, !rect.isEmpty,
            rect.minX.isFinite, rect.minY.isFinite, rect.width.isFinite, rect.height.isFinite,
            pupilSize.isFinite, darken.isFinite
        else { return nil }
        // Clamp in DOUBLE space, before any integer conversion. `Int(_:)`
        // TRAPS — it does not saturate — on a finite Double outside Int64's
        // range, and the trap kills the process with every open document's
        // unsaved work; `red_eye {"x": 1e300, …}` and an `eyes` entry with a
        // huge radius both reached it. A canvas coordinate past ±2 × 10⁹
        // cannot name a pixel of any document this app can hold (`MAX_PIXELS`
        // is 100 million), so the clamp discards nothing real, and the agent
        // handlers wall the arguments far below it so a caller gets a
        // sentence rather than a silent clamp.
        let x = Int32(clamping: Int(Self.pixelCoordinate(rect.minX.rounded(.down))))
        let y = Int32(clamping: Int(Self.pixelCoordinate(rect.minY.rounded(.down))))
        let w = UInt32(clamping: Int(Self.pixelCoordinate(rect.width.rounded())))
        let h = UInt32(clamping: Int(Self.pixelCoordinate(rect.height.rounded())))
        guard w > 0, h > 0 else { return nil }
        return wrap(
            dr_doc_red_eye_layer(ptr, idx, x, y, w, h, Float(pupilSize), Float(darken)))
    }

    /// A finite canvas coordinate brought inside the range `Int(_:)` can
    /// convert without trapping. NaN is already excluded by the callers'
    /// `isFinite` guards; this handles the finite-but-astronomical case that
    /// a JSON number in scientific notation makes trivial to send.
    private static func pixelCoordinate(_ value: Double) -> Double {
        min(max(value, -2_000_000_000), 2_000_000_000)
    }
}

extension RasterPrecisionDocument {
    func retouchSource(layer: Int, sample: RetouchSampling,
                       ignoreAdjustments: Bool) throws -> RasterPrecisionImage {
        let context = try PrecisionWorkContext(source: self)
        let control = try RasterJobControl()
        var error: UnsafeMutablePointer<CChar>?
        guard let handle = dr_precision_doc_retouch_source(ptr, layer, sample.code,
                ignoreAdjustments, context.limit, context.otherRetained, control.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not capture native retouch source."))
        }
        return RasterPrecisionImage(owning: handle)
    }

    /// `editingPixels` for one `retouch_clone` or `retouch_heal` request, with
    /// the sampling plate rendered from `plates` instead of from this document:
    /// Content-Aware Move heals the moved pixels onto the filled
    /// document while reading them from the picture before the fill. The
    /// request must write its target layer in place, and `plates` must have
    /// this document's canvas size and the sampled layer; `samples` is the
    /// request's planned input on THIS document. nil when nothing would
    /// change; throws the core's sentence on a refusal.
    func retouching(from plates: RasterPrecisionDocument, request: String, samples: [UInt16],
                    workingLimit: UInt64, otherRetained: UInt64, control: RasterJobControl?)
        throws -> RasterPrecisionDocument? {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        let result = samples.withUnsafeBufferPointer { input in
            dr_precision_doc_retouch_from(ptr, plates.ptr, request, input.baseAddress, input.count,
                                          workingLimit, otherRetained, control?.ptr, &error)
        }
        return try editResult(result, error)
    }
}

extension RasterImage {
    func retouchTransformed(matrix: [Double]) throws -> RasterImage {
        guard matrix.count == 6, matrix.allSatisfy(\.isFinite) else {
            throw DoctoredCoreError(message: "Retouch source transform requires six finite numbers.")
        }
        let scratch = UInt64(width * height) * 12 + 4096
        let available = try RasterResources.shared.admit(scratch: scratch)
        let control = try RasterJobControl()
        guard control.setScratchLimit(available + scratch) else {
            throw DoctoredCoreError(message: "Could not admit retouch source transformation.")
        }
        var error: UnsafeMutablePointer<CChar>?
        let result = matrix.withUnsafeBufferPointer {
            dr_image_transform_retouch_source(ptr, $0.baseAddress, control.ptr, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not transform retouch source."))
        }
        return RasterImage(owning: result)
    }
}

extension RasterDocument {
    func expandingRetouchDestination(_ layer: Int) throws -> RasterDocument {
        guard let info = layerInfo(layer), precision == nil else {
            throw DoctoredCoreError(message: "Retouch expansion requires a byte raster target.")
        }
        let width = max(self.width, info.offsetX + info.width) - min(0, info.offsetX)
        let height = max(self.height, info.offsetY + info.height) - min(0, info.offsetY)
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw DoctoredCoreError(message: "Retouch destination expansion exceeds supported capacity.")
        }
        let scratch = UInt64(width * height) * 4 + 4096
        let available = try RasterResources.shared.admit(scratch: scratch)
        let control = try RasterJobControl()
        guard control.setScratchLimit(available + scratch) else {
            throw DoctoredCoreError(message: "Could not admit retouch destination expansion.")
        }
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_doc_expand_retouch_destination(ptr, layer, control.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not expand retouch destination."))
        }
        return RasterDocument(owning: result)
    }

    /// Edit ▸ Stroke… with nothing selected: the working memory
    /// `strokingLayerEdge` reserves for layer `layer` at `widthPx`. 0 when
    /// there is nothing to outline — no alpha > 0 pixel — and for a bad
    /// index or width or a typed document; `UInt64.max` when the stroke's
    /// frame cannot be represented.
    func layerEdgeStrokeScratch(_ layer: Int, widthPx: Double) -> UInt64 {
        guard precision == nil, isValidIndex(layer) else { return 0 }
        return dr_doc_stroke_layer_edge_scratch(ptr, layer, Float(widthPx))
    }

    /// Edit ▸ Stroke… with nothing selected: layer `layer`'s own
    /// alpha edge stroked in LAYER space — beyond the canvas included — with
    /// straight `rgba` through `mode` at `opacity` (0…1). The layer grows to
    /// cover the band unless `preserveTransparency` or a transparency lock
    /// keeps its alpha. nil when nothing changed (or there is nothing to
    /// outline); throws the core's sentence on a refusal. A byte document
    /// only, as Stroke… is.
    func strokingLayerEdge(
        _ layer: Int, widthPx: Double, location: DrStrokeLocation, rgba: [UInt8],
        mode: DrBlendMode, opacity: Double, preserveTransparency: Bool
    ) throws -> RasterDocument? {
        guard precision == nil, isValidIndex(layer), rgba.count == 4 else {
            throw DoctoredCoreError(message: "Stroke… outlines a layer of an 8-bit document.")
        }
        let scratch = dr_doc_stroke_layer_edge_scratch(ptr, layer, Float(widthPx))
        if scratch == 0 { return nil }
        guard scratch != UInt64.max else {
            throw DoctoredCoreError(message: "The stroke exceeds supported capacity.")
        }
        let available = try RasterResources.shared.admit(scratch: scratch)
        let control = try RasterJobControl()
        guard control.setScratchLimit(available + scratch) else {
            throw DoctoredCoreError(message: "Could not admit the stroke.")
        }
        var error: UnsafeMutablePointer<CChar>?
        let result = rgba.withUnsafeBufferPointer { color in
            dr_doc_stroke_layer_edge(
                ptr, layer, Float(widthPx), location, color.baseAddress, mode, Float(opacity),
                preserveTransparency, control.ptr, &error)
        }
        guard let result else {
            guard error != nil else { return nil }
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "Could not stroke the layer's edge."))
        }
        return RasterDocument(owning: result)
    }
}


extension RasterPrecisionImage {
    func retouchTransformed(matrix: [Double]) throws -> RasterPrecisionImage {
        guard matrix.count == 6, matrix.allSatisfy(\.isFinite) else {
            throw DoctoredCoreError(message: "Retouch source transform requires six finite numbers.")
        }
        let context = try PrecisionWorkContext(image: self)
        let control = try RasterJobControl()
        var error: UnsafeMutablePointer<CChar>?
        let result = matrix.withUnsafeBufferPointer {
            dr_precision_image_transform_retouch_source(ptr, $0.baseAddress, context.limit,
                context.otherRetained, control.ptr, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not transform native retouch source."))
        }
        return RasterPrecisionImage(owning: result)
    }
}
