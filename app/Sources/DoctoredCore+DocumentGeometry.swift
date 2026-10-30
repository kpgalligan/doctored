import AppKit

// **12F-1** makes CANVAS SIZE typed the same way as 12E-1's structure tools, and again there is
// almost no Swift-side depth code to it: `RasterDocument.canvasResized` routes typed on its FIRST
// line through `PrecisionPixelCommand.canvasSize` — with `cropped`'s artwork-description
// capture/restore VERBATIM, because a typed geometry edit drops a description whose pixels moved
// and the host puts it back — so `CanvasSizeSheetController.applyClicked`, `AgentServer.canvasSize`
// and `ActionSteps.canvasSize` are untouched and all three answer at 16 bits and at 32-bit float.
// `canvas_size` joins `typedTools`, BOTH `families` maps (the DEFAULT `["document"]` branch —
// Canvas Size targets the DOCUMENT, never a layer index) and `FloatDocument.tools`;
// `showCanvasSize:` joins both selector whitelists in `EditorViewController+Precision.swift`.

extension RasterDocument {
    // MARK: - Whole-document geometry

    func rotated90() throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.init(["op": "rotate90"])) }
        return wrap(dr_doc_rotate90(ptr))
    }
    func rotated180() throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.init(["op": "rotate180"])) }
        return wrap(dr_doc_rotate180(ptr))
    }
    func rotated270() throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.init(["op": "rotate270"])) }
        return wrap(dr_doc_rotate270(ptr))
    }
    func flippedH() throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.init(["op": "flip_horizontal"])) }
        return wrap(dr_doc_flip_horizontal(ptr))
    }
    func flippedV() throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.init(["op": "flip_vertical"])) }
        return wrap(dr_doc_flip_vertical(ptr))
    }

    /// The core's rotation PLAN for a `width` x `height` canvas turned by
    /// `degrees` — the ONE place an angle is classified and a rotated canvas
    /// sized (Phase 12F-1b3, `doc_rotate`). It takes no handle, so the sheet,
    /// the MCP handler and `ActionGeometry.canvasAfter` — which has only a
    /// recorded canvas SIZE — all read the same rule.
    ///
    /// nil when `degrees` is not finite. A plan whose `width` is 0 is a
    /// FILTERED angle whose expanded canvas the core refused.
    static func canvasRotationPlan(width: Int, height: Int, degrees: Double)
        -> CanvasRotationPlan?
    {
        guard width > 0, height > 0 else { return nil }
        var normalized = 0.0
        var rect = [Int32](repeating: 0, count: 4)
        let code = rect.withUnsafeMutableBufferPointer { buffer in
            dr_canvas_rotation_plan(
                UInt32(clamping: width), UInt32(clamping: height), degrees,
                &normalized, buffer.baseAddress)
        }
        let kind: CanvasRotationPlan.Kind
        switch code {
        case DR_ROTATION_IDENTITY: kind = .identity
        case DR_ROTATION_90: kind = .quarter(.rotate90)
        case DR_ROTATION_180: kind = .quarter(.rotate180)
        case DR_ROTATION_270: kind = .quarter(.rotate270)
        case DR_ROTATION_FILTERED: kind = .filtered
        default: return nil
        }
        return CanvasRotationPlan(
            kind: kind, degrees: normalized,
            left: Int(rect[0]), top: Int(rect[1]),
            width: Int(rect[2]), height: Int(rect[3]))
    }

    /// Every artwork layer's editable description, read before a typed
    /// geometry edit drops them so the result can put them back. A
    /// description that cannot be read refuses the whole edit by name.
    func artworkDescriptionsBeforeGeometry(_ operation: String) throws -> [(Int, String)] {
        var descriptions: [(Int, String)] = []
        for index in artworkIndices {
            guard let description = layerMeta(index) else {
                let name = layerInfo(index)?.name ?? "layer \(index)"
                throw DoctoredCoreError(message: "\(operation) needs the editable description of "
                    + "“\(name)”, which could not be read. Nothing was changed.")
            }
            descriptions.append((index, description))
        }
        return descriptions
    }

    /// Delete Cropped Pixels at 8 bits: every plain raster (no meta,
    /// no retained source) trimmed to the canvas. nil for a typed document
    /// (its crop carries the flag) and when no layer reaches outside.
    func deletingCroppedPixels() -> RasterDocument? {
        guard !isHighDepth else { return nil }
        return wrap(dr_doc_delete_cropped_pixels(ptr))
    }

    /// `deleteCropped` trims each plain raster to the new canvas; a
    /// typed crop then runs as the identity straighten over the window with
    /// the flag, which is the crop bit for bit plus the trim.
    func cropped(x: Int, y: Int, w: Int, h: Int, deleteCropped: Bool = false) throws -> RasterDocument? {
        // One sentence at every depth; compared by subtraction so a huge MCP
        // integer cannot overflow `x + w`.
        guard x >= 0, y >= 0, w > 0, h > 0, x < width, y < height,
              w <= width - x, h <= height - y
        else {
            throw DoctoredCoreError(message: "The crop rectangle must lie inside the "
                + "\(width) × \(height) canvas. Nothing was changed.")
        }
        if isHighDepth {
            let descriptions = try artworkDescriptionsBeforeGeometry("Crop")
            let command: PrecisionPixelCommand = deleteCropped
                ? try .straighten(matrix: .identity, filter: .nearest, x: x, y: y,
                                  width: w, height: h, deleteCropped: true)
                : try .crop(x: x, y: y, width: w, height: h)
            guard var result = try performingPrecisionPixelEdit(command) else { return nil }
            for (index, description) in descriptions {
                result = try result.restoringArtworkDescription(description, layer: index)
            }
            return try result.refittingFillLayers()
        }
        guard let result = wrap(dr_doc_crop(ptr, UInt32(x), UInt32(y), UInt32(w), UInt32(h)))
        else { return nil }
        let trimmed = deleteCropped ? result.deletingCroppedPixels() ?? result : result
        // A fill layer covers the new canvas, at both depths.
        return try trimmed.refittingFillLayers()
    }

    func resized(w: Int, h: Int, filter: DrResizeFilter) throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.resize(width: w, height: h, filter: Self.precisionFilter(filter))) }
        guard w > 0, h > 0, w * h <= RasterImage.maxResizePixels else { return nil }
        return wrap(dr_doc_resize(ptr, UInt32(w), UInt32(h), filter))
    }

    /// Canvas Size's 100 MP refusal, the one copy: `canvasResized`, the Canvas Size sheet and
    /// the crop past the canvas (`extendedCropRefusal`) all say it.
    static let canvasTooLargeRefusal =
        "The canvas cannot exceed 100 megapixels (width × height ≤ 100,000,000)."

    /// Where Canvas Size puts the old canvas's top-left, the one copy (the sheet,
    /// `canvas_size` and its Actions replay): floor(Δ · col / 2), floor(Δ · row / 2),
    /// floored toward −∞, never Swift's truncating `/` (take1 CS-C:
    /// +31 × +17 at the centre gives (15, 8); −31 × −17 gives (−16, −9)).
    static func canvasSizeOrigin(oldWidth: Int, oldHeight: Int, newWidth: Int, newHeight: Int,
                                 anchor: (col: Int, row: Int)) -> (x: Int, y: Int) {
        func floored(_ delta: Int, _ fraction: Int) -> Int {
            Int((Double(delta * fraction) / 2).rounded(.down))
        }
        return (floored(newWidth - oldWidth, anchor.col),
                floored(newHeight - oldHeight, anchor.row))
    }

    /// Changes the canvas size without scaling; (originX, originY) is where
    /// the old canvas's top-left lands in the new canvas. On an Indexed
    /// document the new area takes the entry nearest `indexedBackground` when
    /// there is no transparent index (#1 Indexed phase 3b); nil is white.
    func canvasResized(w: Int, h: Int, originX: Int, originY: Int,
                       indexedBackground: NSColor? = nil) throws -> RasterDocument? {
        guard w > 0, h > 0 else {
            throw DoctoredCoreError(message: "The canvas width and height must each be at least "
                + "1 pixel. Nothing was changed.")
        }
        // Division, never `w * h`: an MCP size can be any Int, and the
        // product would overflow before the comparison.
        guard w <= RasterImage.maxResizePixels / h else {
            throw DoctoredCoreError(message: Self.canvasTooLargeRefusal)
        }
        // Phase 12F-1: the typed op is `GeometryEdit::CanvasSize`, a
        // generalized Crop, so the artwork-description capture/restore is
        // `cropped`'s verbatim — a typed geometry edit drops a description
        // whose pixels moved and the host puts it back.
        if isHighDepth {
            let descriptions = try artworkDescriptionsBeforeGeometry("Canvas Size")
            let command = try IndexedDocument.backgrounded(
                .canvasSize(width: w, height: h, x: originX, y: originY), indexedBackground, in: self)
            guard var result = try performingPrecisionPixelEdit(command) else { return nil }
            for (index, description) in descriptions {
                result = try result.restoringArtworkDescription(description, layer: index)
            }
            return try result.refittingFillLayers()
        }
        // A fill layer covers the new canvas, at both depths.
        return try wrap(dr_doc_canvas_resize(ptr, UInt32(w), UInt32(h), Int32(originX), Int32(originY)))?
            .refittingFillLayers()
    }

    /// Free transform of ONE layer by an arbitrary affine matrix, resampled
    /// with `sampler`. The matrix works in CANVAS coordinates — it says where
    /// the layer's canvas rect lands — and its six elements cross the FFI in
    /// CGAffineTransform order, [a, b, c, d, tx, ty], so a CGAffineTransform
    /// passes straight through. The layer's new offset and size are the
    /// outward-rounded bounding box of the transformed corners; its mask,
    /// name, opacity, blend mode, visibility and metadata survive (dropping a
    /// text description is the caller's policy). nil for a singular or
    /// non-finite matrix, an out-of-range index, or an extent the core caps.
    func transformingLayer(
        _ idx: Int, _ transform: CGAffineTransform, sampler: DrResizeFilter
    ) throws -> RasterDocument? {
        if isHighDepth { return try precisionTransform([idx], matrix: transform, filter: sampler) }
        guard isValidIndex(idx), let affine = Self.affineElements(transform) else { return nil }
        return affine.withUnsafeBufferPointer { buffer in
            wrap(dr_doc_transform_layer(ptr, idx, buffer.baseAddress, sampler))
        }
    }

    /// The six elements in the order the FFI reads them, [a, b, c, d, tx,
    /// ty]; nil when any is not finite (a matrix the core would refuse).
    private static func affineElements(_ transform: CGAffineTransform) -> [Double]? {
        let affine: [Double] = [
            Double(transform.a), Double(transform.b), Double(transform.c),
            Double(transform.d), Double(transform.tx), Double(transform.ty),
        ]
        return affine.allSatisfy({ $0.isFinite }) ? affine : nil
    }

    /// Perspective transform of ONE layer: maps its canvas rect
    /// corner-for-corner onto `quad` — four canvas points in the source
    /// rect's corner order TL, TR, BR, BL. The homography is solved in the
    /// core; a parallelogram quad delegates to the affine path, exact fast
    /// paths included, and everything `transformingLayer` preserves
    /// survives here too. nil for a non-finite coordinate, a concave,
    /// self-intersecting or collapsed quad, an out-of-range index, or an
    /// extent the core caps.
    func perspectiveLayer(
        _ idx: Int, quad: [CGPoint], sampler: DrResizeFilter
    ) throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.perspective(layer: idx, target: .pixels, quad: quad, filter: Self.precisionFilter(sampler))) }
        guard isValidIndex(idx), quad.count == 4 else { return nil }
        let corners: [Double] = quad.flatMap { [Double($0.x), Double($0.y)] }
        guard corners.allSatisfy({ $0.isFinite }) else { return nil }
        return corners.withUnsafeBufferPointer { buffer in
            wrap(dr_doc_perspective_layer(ptr, idx, buffer.baseAddress, sampler))
        }
    }

    /// Writes the native DRDC format (all layers preserved).
    func saveNative(to url: URL) throws {
        if let precision {
            let context = try PrecisionWorkContext(source: precision)
            try precision.saveNative(to: url, workingLimit: context.limit, otherRetained: context.otherRetained)
            return
        }
        var err: UnsafeMutablePointer<CChar>? = nil
        guard dr_doc_save_native(ptr, url.path, &err) else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not save \(url.lastPathComponent)."))
        }
    }

    /// The 8-bit native save/snapshot scratch (core plan). Throws on 0, and for a typed document
    /// (which uses the precision descriptor). The one source for both Swift call sites.
    func nativeSaveScratchBytes() throws -> UInt64 {
        guard precision == nil else {
            throw DoctoredCoreError(message: "A high-depth document plans its native save through its descriptor.")
        }
        var err: UnsafeMutablePointer<CChar>? = nil
        let bytes = dr_doc_native_save_scratch_bytes(ptr, &err)
        guard bytes > 0 else {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Could not estimate native save memory."))
        }
        return bytes
    }

    /// The 8-bit native reopen plan (core `drdc_reopen_plan`): the bytes the reader retains and
    /// its decode peak. Throws for a typed document, as `nativeSaveScratchBytes` does.
    func nativeReopenPlan() throws -> (retainedBytes: UInt64, decodeBytes: UInt64) {
        guard precision == nil else {
            throw DoctoredCoreError(message: "A high-depth document plans its native reopen through its descriptor.")
        }
        var err: UnsafeMutablePointer<CChar>? = nil
        var retained: UInt64 = 0
        var decode: UInt64 = 0
        guard dr_doc_native_reopen_plan(ptr, &retained, &decode, &err) else {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Could not estimate native reopen memory."))
        }
        return (retained, decode)
    }
}
