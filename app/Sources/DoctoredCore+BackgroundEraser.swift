import Foundation

// The Background Eraser's core calls (`core/src/ffi_background_eraser.rs`,
// `designs/background-eraser.md`): the byte layer's erase, and a typed document's coverage plane
// with its plan, which `erase_native` then erases (`BackgroundEraser.swift`).

extension RasterDocument {
    /// `dr_doc_background_eraser_layer`: the Background Eraser on layer `idx` along a stroke.
    /// `request` is `BackgroundEraserOptions.request` (its colours in the document's own
    /// space), `brushRequest` and `dabs` are `dr_brush_u16`'s (canvas coordinates), `selection`
    /// is canvas-sized u8 coverage or nil. nil when nothing would change, a lock keeps the
    /// layer, or `idx` is not a raster layer; throws the core's sentence for a bad request or
    /// stroke, the 256-million work bound or a selection of another size.
    func backgroundEraserLayer(
        _ idx: Int, request: String, brushRequest: String, dabs: [Double], selection: [UInt8]?
    ) throws -> RasterDocument? {
        guard isValidIndex(idx) else { return nil }
        try precisionCString(request, maximumBytes: 4096)
        try precisionCString(brushRequest, maximumBytes: 4096)
        var error: UnsafeMutablePointer<CChar>?
        let result = dabs.withUnsafeBufferPointer { input -> OpaquePointer? in
            let run = { (mask: UnsafePointer<UInt8>?, count: Int) in
                dr_doc_background_eraser_layer(
                    self.ptr, idx, request, brushRequest, input.baseAddress, dabs.count / 4,
                    mask, count, &error)
            }
            guard let selection else { return run(nil, 0) }
            return selection.withUnsafeBufferPointer { run($0.baseAddress, $0.count) }
        }
        if let error {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "The Background Eraser failed."))
        }
        return wrap(result)
    }
}

extension RasterPrecisionDocument {
    /// `dr_precision_doc_background_eraser_coverage_plan`: `retained_bytes`, `scratch_bytes`,
    /// `output_samples` and `work_units`; refuses what the run refuses (Bitmap by name).
    func backgroundEraserCoveragePlan(layer: Int, request: String, brushRequest: String,
                                      dabs: [Double], workingLimit: UInt64,
                                      otherRetained: UInt64) throws -> [String: Any] {
        guard layer >= 0 else { throw DoctoredCoreError(message: "Invalid high-depth layer index.") }
        try precisionCString(request, maximumBytes: 4096)
        try precisionCString(brushRequest, maximumBytes: 4096)
        var error: UnsafeMutablePointer<CChar>?
        let result = dabs.withUnsafeBufferPointer {
            dr_precision_doc_background_eraser_coverage_plan(
                ptr, layer, request, brushRequest, $0.baseAddress, dabs.count / 4, workingLimit,
                otherRetained, &error)
        }
        return try precisionJSON(result, error)
    }

    /// `dr_precision_doc_background_eraser_coverage`: the canvas-sized u16 coverage of the
    /// stroke on raster `layer`, 0 outside it; nothing is erased.
    func backgroundEraserCoverage(layer: Int, request: String, brushRequest: String,
                                  dabs: [Double], width: Int, height: Int, workingLimit: UInt64,
                                  otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> [UInt16] {
        guard layer >= 0 else { throw DoctoredCoreError(message: "Invalid high-depth layer index.") }
        try precisionCString(request, maximumBytes: 4096)
        try precisionCString(brushRequest, maximumBytes: 4096)
        var error: UnsafeMutablePointer<CChar>?
        var output = [UInt16](repeating: 0, count: width * height)
        let ok = dabs.withUnsafeBufferPointer { input in
            output.withUnsafeMutableBufferPointer {
                dr_precision_doc_background_eraser_coverage(
                    ptr, layer, request, brushRequest, input.baseAddress, dabs.count / 4,
                    $0.baseAddress, $0.count, workingLimit, otherRetained, control?.ptr, &error)
            }
        }
        guard ok else {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "The Background Eraser failed."))
        }
        return output
    }
}
