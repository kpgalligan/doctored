import Foundation

/// Edit ▸ Perspective Warp's layout and preview calls
/// (`ffi_perspective_warp.rs`): neither needs a document handle.
enum RasterPerspectiveWarp {
    /// The core's resolution of `fields` (`{"quads","straighten"?,"anchor"?}`):
    /// joins, straightening and shared edges. Throws the core's sentence.
    static func layout(fields: [String: Any]) throws -> PerspectiveLayout {
        let text = try PerspectiveWarp.json(fields)
        var error: UnsafeMutablePointer<CChar>?
        let value = try precisionJSON(dr_perspective_warp_layout(text, &error), error)
        return try PerspectiveLayout(described: value)
    }

    /// The workspace preview: `proxy`, a picture of the layer's canvas rect
    /// `source` at any scale, through the warp of `fields`, optionally times
    /// `coverage` (proxy-sized bytes). Answers the picture and the canvas
    /// rect it covers.
    static func preview(fields: [String: Any], proxy: RasterImage, source: CGRect,
                        coverage: [UInt8]?) throws -> (image: RasterImage, rect: CGRect) {
        guard source.width >= 1, source.height >= 1, source.width <= CGFloat(UInt32.max),
              source.height <= CGFloat(UInt32.max) else {
            throw DoctoredCoreError(message: "The Perspective Warp preview needs a layer with pixels.")
        }
        let text = try PerspectiveWarp.json(fields)
        var rect = [Double](repeating: 0, count: 4)
        var error: UnsafeMutablePointer<CChar>?
        let result: OpaquePointer? = rect.withUnsafeMutableBufferPointer { buffer in
            let out = buffer.baseAddress
            let render = { (pointer: UnsafePointer<UInt8>?, count: Int) in
                dr_perspective_warp_preview(text, proxy.ptr, Int64(source.minX), Int64(source.minY),
                                            UInt32(source.width), UInt32(source.height), pointer,
                                            count, out, &error)
            }
            guard let coverage else { return render(nil, 0) }
            return coverage.withUnsafeBufferPointer { render($0.baseAddress, $0.count) }
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not render the Perspective Warp preview."))
        }
        return (RasterImage(owning: result), CGRect(x: rect[0], y: rect[1], width: rect[2], height: rect[3]))
    }
}

extension RasterDocument {
    /// `{"retained_bytes","scratch_bytes","rect","vertices"}` for a
    /// `perspective_warp` request; throws the core's refusal.
    func perspectiveWarpPlan(request: String) throws -> [String: Any] {
        try precisionCString(request, maximumBytes: 64 * 1024)
        let budget = try meshWarpBudget()
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            return try precisionJSON(dr_precision_doc_perspective_warp_plan(
                precision.ptr, request, budget.limit, budget.other, &error), error)
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required for Perspective Warp.") }
        return try precisionJSON(dr_doc_perspective_warp_plan(ptr, request, budget.limit,
                                                              budget.other, &error), error)
    }

    /// The `perspective_warp` edit under `budget` (read on main,
    /// `meshWarpBudget`), so it may run on the worker. nil for an identity
    /// layout; throws the core's refusal or `operation cancelled`.
    func editingPerspectiveWarp(request: String, budget: (limit: UInt64, other: UInt64),
                                control: RasterJobControl?) throws -> RasterDocument? {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            let result = dr_precision_doc_perspective_warp(precision.ptr, request, budget.limit,
                                                           budget.other, control?.ptr, &error)
            if error != nil {
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Perspective Warp was refused."))
            }
            return try result.map { try RasterDocument(precision: RasterPrecisionDocument(owning: $0)) }
        }
        guard let ptr else { throw DoctoredCoreError(message: "A document is required for Perspective Warp.") }
        let result = dr_doc_perspective_warp(ptr, request, budget.limit, budget.other, control?.ptr,
                                             &error)
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Perspective Warp was refused."))
        }
        return wrap(result)
    }
}
