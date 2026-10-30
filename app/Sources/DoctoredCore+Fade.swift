import Foundation

// Edit ▸ Fade's core calls (v1; `core/src/ffi_fade.rs`): the byte document's `dr_doc_fade` and the
// typed document's plan and edit, 16-bit and 32-bit float. `self` is the faded step's RESULT and
// `before` the state before it; the layer index names the same layer in both.

extension RasterDocument {
    /// `dr_doc_fade`: layer `layer` of this document faded back toward the
    /// same layer of `before` at `opacity` (0...1) in `mode`. `coverage` is a
    /// region step's canvas-sized u8 weight (nil: 1 everywhere);
    /// `changedOnly` weighs a stroke by the pixels it changed instead. nil
    /// when nothing would change and for every refusal (Pass Through, another
    /// canvas size, a non-raster layer, a lock).
    func faded(from before: RasterDocument, layer: Int, mode: DrBlendMode, opacity: Double,
               coverage: [UInt8]?, changedOnly: Bool) -> RasterDocument? {
        guard layer >= 0, let beforePointer = before.ptr else { return nil }
        let result: OpaquePointer?
        if let coverage {
            result = coverage.withUnsafeBufferPointer {
                dr_doc_fade(ptr, beforePointer, layer, mode, Float(opacity), $0.baseAddress,
                            $0.count, changedOnly)
            }
        } else {
            result = dr_doc_fade(ptr, beforePointer, layer, mode, Float(opacity), nil, 0,
                                 changedOnly)
        }
        return wrap(result)
    }
}

extension RasterPrecisionDocument {
    /// `dr_precision_doc_fade_plan`: `retained_bytes`, `scratch_bytes` and
    /// `input_samples`, as `pixelEditPlan` answers.
    func fadePlan(layer: Int, before: RasterPrecisionDocument, coverage: Bool,
                  workingLimit: UInt64, otherRetained: UInt64) throws -> [String: Any] {
        guard layer >= 0 else { throw DoctoredCoreError(message: "Invalid high-depth layer index.") }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_fade_plan(
            ptr, layer, before.ptr, coverage, workingLimit, otherRetained, &error)
        return try precisionJSON(result, error)
    }

    /// `dr_precision_doc_fade`: nil for an exact no-op, as `editingPixels`;
    /// throws the core's sentence for a refusal (a mode a float document does
    /// not admit, among others).
    func fading(layer: Int, before: RasterPrecisionDocument, mode: DrBlendMode, opacity: Double,
                coverage: [UInt16]?, changedOnly: Bool, workingLimit: UInt64,
                otherRetained: UInt64, control: RasterJobControl? = nil)
        throws -> RasterPrecisionDocument? {
        guard layer >= 0 else { throw DoctoredCoreError(message: "Invalid high-depth layer index.") }
        var error: UnsafeMutablePointer<CChar>?
        let run = { (samples: UnsafePointer<UInt16>?, count: Int) in
            dr_precision_doc_fade(
                self.ptr, layer, before.ptr, mode, opacity, samples, count, changedOnly,
                workingLimit, otherRetained, control?.ptr, &error)
        }
        let result: OpaquePointer?
        if let coverage {
            result = coverage.withUnsafeBufferPointer { run($0.baseAddress, $0.count) }
        } else {
            result = run(nil, 0)
        }
        return try editResult(result, error)
    }
}
