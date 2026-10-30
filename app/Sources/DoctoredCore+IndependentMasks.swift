import AppKit

// MARK: - Independent masks and selection kernels

extension RasterPrecisionDocument {
    /// The plan of `estimatingForeground` for `source`: `retained_bytes`,
    /// `scratch_bytes` and `input_samples` (the canvas pixel count).
    func foregroundEstimatePlan(source: RasterPrecisionImage) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_estimate_foreground_plan(ptr, source.ptr, &error)
        return try precisionJSON(result, error)
    }

    /// Select and Mask's Decontaminate Colors at 16 bits
    /// (`dr_precision_doc_estimate_foreground`): foreground RGB estimated for
    /// the opaque canvas-sized composite `source` under `matte`. The result
    /// keeps source alpha and does not apply the matte. Nil means no code
    /// changed.
    func estimatingForeground(source: RasterPrecisionImage, matte: [UInt16],
                              parameters: [String: Any], workingLimit: UInt64,
                              otherRetained: UInt64,
                              control: RasterJobControl?) throws -> RasterPrecisionImage? {
        let json = String(decoding: try JSONSerialization.data(withJSONObject: parameters), as: UTF8.self)
        var error: UnsafeMutablePointer<CChar>?
        let result = matte.withUnsafeBufferPointer {
            dr_precision_doc_estimate_foreground(ptr, source.ptr, $0.baseAddress, matte.count, json,
                                                 workingLimit, otherRetained, control?.ptr, &error)
        }
        if let error {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Foreground estimation failed."))
        }
        return result.map { RasterPrecisionImage(owning: $0) }
    }

    /// Select and Mask's New Layer outputs at 16 bits. Source RGBA16 alpha and
    /// refinement coverage stay independent.
    func refinementCutoutPlan(foreground: RasterPrecisionImage) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_refinement_cutout_plan(ptr, foreground.ptr, &error)
        return try precisionJSON(result, error)
    }

    func addingRefinementCutout(foreground: RasterPrecisionImage, matte: [UInt16],
                                workingLimit: UInt64, otherRetained: UInt64,
                                control: RasterJobControl? = nil) throws -> RasterPrecisionDocument? {
        _ = try refinementCutoutPlan(foreground: foreground)
        guard matte.count == foreground.width * foreground.height else {
            throw DoctoredCoreError(message: "Native refinement coverage must match the canvas.")
        }
        var error: UnsafeMutablePointer<CChar>?
        let result = matte.withUnsafeBufferPointer { samples in
            dr_precision_doc_add_refinement_cutout(ptr, foreground.ptr, samples.baseAddress,
                workingLimit, otherRetained, control?.ptr, &error)
        }
        return try editResult(result, error)
    }
}

extension RasterDocument {
    func layerMaskProperties(_ index: Int) -> LayerMaskProperties? {
        if let precisionMetadata { return precisionMetadata.layer(index)?.mask }
        guard isValidIndex(index), let json = dr_doc_layer_mask_properties(ptr, index) else { return nil }
        defer { dr_string_free(json) }
        guard let value = try? JSONSerialization.jsonObject(with: Data(String(cString: json).utf8)) as? [String: Any],
              let linked = value["linked"] as? Bool,
              let density = value["density"] as? Double,
              let feather = value["feather"] as? Double,
              let outside = value["outside_coverage"] as? Int, [0, 255].contains(outside),
              let x = value["offset_x"] as? Int, let y = value["offset_y"] as? Int,
              let width = value["width"] as? Int, let height = value["height"] as? Int else { return nil }
        return LayerMaskProperties(linked: linked, density: density, feather: feather,
                                   outsideCoverage: UInt8(outside), offsetX: x, offsetY: y, width: width, height: height)
    }

    func hasSameLayerMask(as other: RasterDocument, index: Int) -> Bool {
        guard isValidIndex(index), other.isValidIndex(index) else { return false }
        return dr_doc_layer_masks_equal(ptr, other.ptr, index)
    }

    func settingMaskProperties(_ index: Int, linked: Bool, density: Double, feather: Double) throws -> RasterDocument? {
        if isHighDepth { return try precisionMaskProperties(index, linked: linked, density: density, feather: feather) }
        guard isValidIndex(index), density.isFinite, feather.isFinite,
              (0...1).contains(density), (0...1000).contains(feather) else { return nil }
        return wrap(dr_doc_set_mask_properties(ptr, index, linked, Float(density), Float(feather)))
    }

    /// Image ▸ Adjustments ▸ Invert on an 8-bit layer mask, over the whole
    /// canvas (`dr_doc_invert_mask`): with no selection the stored
    /// samples and the coverage outside the stored rectangle both invert;
    /// with one, the mask grows to hold the canvas and inverts by coverage.
    /// nil for a 16/32-bit document (its scalar edit follows the same rule in
    /// the core), a missing mask, Lock All, or a selection that changes nothing.
    func invertingLayerMask(_ index: Int, selection: [UInt8]?) -> RasterDocument? {
        guard !isHighDepth, isValidIndex(index) else { return nil }
        guard let selection else { return wrap(dr_doc_invert_mask(ptr, index, nil, 0)) }
        guard selection.count == width * height else { return nil }
        return selection.withUnsafeBufferPointer {
            wrap(dr_doc_invert_mask(ptr, index, $0.baseAddress, selection.count))
        }
    }

    func transformingMask(_ index: Int, matrix: CGAffineTransform,
                          filter: DrResizeFilter = DR_FILTER_BILINEAR) throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.affine(layer: index, target: .mask, matrix: matrix, filter: Self.precisionFilter(filter))) }
        guard isValidIndex(index) else { return nil }
        let values = [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty].map(Double.init)
        guard values.allSatisfy(\.isFinite) else { return nil }
        return values.withUnsafeBufferPointer {
            wrap(dr_doc_transform_mask(ptr, index, $0.baseAddress, Int32(filter.rawValue)))
        }
    }

    func colorRange(source: RasterImage, parameters: [String: Any],
                    control: RasterJobControl? = nil) throws -> [UInt8] {
        try selectionKernel(source: source, parameters: parameters, control: control, quick: false)
    }

    func quickSelection(source: RasterImage, parameters: [String: Any],
                        control: RasterJobControl? = nil) throws -> [UInt8] {
        try selectionKernel(source: source, parameters: parameters, control: control, quick: true)
    }

    private func selectionKernel(source: RasterImage, parameters: [String: Any],
                                 control: RasterJobControl?, quick: Bool) throws -> [UInt8] {
        guard source.width == width, source.height == height else {
            throw DoctoredCoreError(message: "Selection source dimensions do not match the document.")
        }
        let json = String(decoding: try JSONSerialization.data(withJSONObject: parameters), as: UTF8.self)
        var output = [UInt8](repeating: 0, count: width * height)
        var error: UnsafeMutablePointer<CChar>?
        let success = output.withUnsafeMutableBufferPointer { buffer in
            quick ? dr_selection_quick(ptr, source.ptr, json, buffer.baseAddress, control?.ptr, &error)
                : dr_selection_color_range(ptr, source.ptr, json, buffer.baseAddress, control?.ptr, &error)
        }
        guard success else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Selection could not be computed."))
        }
        return output
    }
}

/// One Magnetic Lasso trace (`dr_selection_trace_edge`): the path's vertices
/// at pixel centres (`from`'s pixel first), the vertex the host fastens an
/// anchor at, and the corridor's pixel count.
struct EdgeTraceResult {
    let points: [CGPoint]
    let fasten: Int?
    let corridorPixels: UInt64
}

extension RasterImage {
    /// Traces the Magnetic Lasso's live wire over this image — the core's
    /// strict JSON request (`from`, `trail`, `end`, `width`, `contrast`,
    /// `frequency`; see the header). The first call offers room for 65,536
    /// vertices; a short buffer retries once at the count the core needs.
    func traceEdge(parameters: [String: Any]) throws -> EdgeTraceResult {
        let json = String(decoding: try JSONSerialization.data(withJSONObject: parameters), as: UTF8.self)
        let fallback = "The edge could not be traced."
        var capacity = 65_536
        for attempt in 0..<2 {
            var points = [Float](repeating: 0, count: capacity * 2)
            var count = 0
            var fasten: Int32 = -1
            var corridor: UInt64 = 0
            var error: UnsafeMutablePointer<CChar>?
            let success = points.withUnsafeMutableBufferPointer {
                dr_selection_trace_edge(ptr, json, $0.baseAddress, capacity,
                                        &count, &fasten, &corridor, &error)
            }
            if success {
                let vertices = (0..<count).map {
                    CGPoint(x: CGFloat(points[2 * $0]), y: CGFloat(points[2 * $0 + 1]))
                }
                return EdgeTraceResult(points: vertices, fasten: fasten >= 0 ? Int(fasten) : nil,
                                       corridorPixels: corridor)
            }
            let message = takeErrorMessage(error, fallback: fallback)
            guard attempt == 0, count > capacity else { throw DoctoredCoreError(message: message) }
            capacity = count
        }
        throw DoctoredCoreError(message: fallback)
    }
}

extension RasterImage {
    /// Select and Mask's Decontaminate Colors at 8 bits
    /// (`dr_image_estimate_foreground`). Output changes RGB, retains source
    /// alpha, and does not apply the matte. Nil means no byte change. The
    /// immutable source is never replaced here.
    func estimatingForeground(in document: RasterDocument, alpha: [UInt8],
                              parameters: [String: Any],
                              control: RasterJobControl? = nil) throws -> RasterImage? {
        guard width == document.width, height == document.height,
              alpha.count == width * height else {
            throw DoctoredCoreError(message: "Foreground coverage dimensions do not match the source.")
        }
        let json = String(decoding: try JSONSerialization.data(withJSONObject: parameters), as: UTF8.self)
        var error: UnsafeMutablePointer<CChar>?
        let result = alpha.withUnsafeBufferPointer {
            dr_image_estimate_foreground(document.ptr, ptr, $0.baseAddress, json, control?.ptr, &error)
        }
        if let error {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Foreground estimation failed."))
        }
        return result.map { RasterImage(owning: $0) }
    }

    /// Select and Mask's coverage refinement (`dr_selection_refine`). `brush`
    /// is the AREA plane (0 never refined, 128 automatic, 255 always refined;
    /// nil = all automatic). An edge stage reserves 64 bytes a pixel against
    /// `control` (`SelectionCapture.edgeRefineScratchBytes`).
    func refiningSelection(_ mask: [UInt8], brush: [UInt8]? = nil, parameters: [String: Any],
                           control: RasterJobControl? = nil) throws -> [UInt8] {
        guard mask.count == width * height, brush == nil || brush?.count == mask.count else {
            throw DoctoredCoreError(message: "Refinement coverage dimensions do not match the source.")
        }
        let json = String(decoding: try JSONSerialization.data(withJSONObject: parameters), as: UTF8.self)
        var output = [UInt8](repeating: 0, count: mask.count)
        var error: UnsafeMutablePointer<CChar>?
        let success = mask.withUnsafeBufferPointer { input in
            output.withUnsafeMutableBufferPointer { destination in
                func call(_ footprint: UnsafePointer<UInt8>?) -> Bool {
                    dr_selection_refine(ptr, input.baseAddress, footprint, json,
                                        destination.baseAddress, control?.ptr, &error)
                }
                if let brush { return brush.withUnsafeBufferPointer { call($0.baseAddress) } }
                return call(nil)
            }
        }
        guard success else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Selection refinement failed."))
        }
        return output
    }

    /// Coverage reduced or enlarged to `toWidth × toHeight` through the
    /// document resize's own plane kernel (`dr_selection_resize`), so a
    /// selection keeps fitting a resized copy of its document (a sheet's proxy preview).
    static func resizingSelection(_ mask: [UInt8], width: Int, height: Int, toWidth: Int, toHeight: Int,
                                  filter: DrResizeFilter = DR_FILTER_BILINEAR) throws -> [UInt8] {
        guard width > 0, height > 0, width <= maxResizePixels / height, mask.count == width * height,
              toWidth > 0, toHeight > 0, toWidth <= maxResizePixels / toHeight else {
            throw DoctoredCoreError(message: "Invalid selection resize dimensions.")
        }
        var output = [UInt8](repeating: 0, count: toWidth * toHeight)
        var error: UnsafeMutablePointer<CChar>?
        let success = mask.withUnsafeBufferPointer { input in
            output.withUnsafeMutableBufferPointer { destination in
                dr_selection_resize(input.baseAddress, UInt32(width), UInt32(height), UInt32(toWidth),
                                    UInt32(toHeight), Int32(filter.rawValue), destination.baseAddress, &error)
            }
        }
        guard success else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Selection resize failed."))
        }
        return output
    }

    static func transformingSelection(_ mask: [UInt8], width: Int, height: Int,
                                      matrix: CGAffineTransform,
                                      filter: DrResizeFilter = DR_FILTER_BILINEAR,
                                      control: RasterJobControl? = nil) throws -> [UInt8] {
        guard width > 0, height > 0, width <= maxResizePixels / height, mask.count == width * height else {
            throw DoctoredCoreError(message: "Invalid selection transform dimensions.")
        }
        let values = [matrix.a, matrix.b, matrix.c, matrix.d, matrix.tx, matrix.ty].map(Double.init)
        guard values.allSatisfy(\.isFinite) else { throw DoctoredCoreError(message: "Selection transform must be finite.") }
        var output = [UInt8](repeating: 0, count: mask.count)
        var error: UnsafeMutablePointer<CChar>?
        let success = mask.withUnsafeBufferPointer { input in
            output.withUnsafeMutableBufferPointer { destination in
                values.withUnsafeBufferPointer {
                    dr_selection_transform(input.baseAddress, UInt32(width), UInt32(height), $0.baseAddress,
                                           Int32(filter.rawValue), destination.baseAddress, control?.ptr, &error)
                }
            }
        }
        guard success else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Selection transform failed."))
        }
        return output
    }
}
