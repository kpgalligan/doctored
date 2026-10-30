import Foundation

/// Stack Modes: the core's `stack_mode` request at every depth. A
/// byte document goes through `dr_doc_stack_mode[_plan]`; a 16-bit or float
/// one through the typed request ABI (`editingGeometry`'s two-branch shape).
extension RasterDocument {
    /// The request's plan: `retained_bytes`, `scratch_bytes`, and
    /// `work_units` (byte) or `input_samples` (typed, always 0). Throws the
    /// core's sentence for every refusal the edit gives before it reads a
    /// pixel.
    func stackModePlan(request: String) throws -> [String: Any] {
        if let precision {
            let context = try PrecisionWorkContext(source: precision)
            return try precision.pixelEditPlan(request: request, workingLimit: context.limit,
                                               otherRetained: context.otherRetained)
        }
        guard let ptr else {
            throw DoctoredCoreError(message: "A document is required for a stack mode.")
        }
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        return try precisionJSON(dr_doc_stack_mode_plan(ptr, request, &error), error)
    }

    /// The edit: one new raster holding the statistic, every member hidden.
    /// `budget` is the byte working-memory context; nil reads
    /// `legacyPrecisionWorkContext()`, which is main-only, so an off-main
    /// byte caller MUST pass it. Ignored for a typed document. The core never
    /// answers "unchanged" for this request, so nil is not expected.
    func editingStackMode(request: String, budget: (limit: UInt64, other: UInt64)? = nil,
                          control: RasterJobControl? = nil) throws -> RasterDocument? {
        if let precision {
            let context = try PrecisionWorkContext(source: precision)
            return try precision.editingPixels(request: request, samples: [],
                workingLimit: context.limit, otherRetained: context.otherRetained,
                control: control).map { try RasterDocument(precision: $0) }
        }
        guard let ptr else {
            throw DoctoredCoreError(message: "A document is required for a stack mode.")
        }
        try precisionCString(request, maximumBytes: 64 * 1024)
        let budget = try budget ?? legacyPrecisionWorkContext()
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_stack_mode(ptr, request, budget.limit, budget.other, control?.ptr, &error)
        if error != nil {
            throw DoctoredCoreError(
                message: takeErrorMessage(error, fallback: "The stack mode was refused."))
        }
        return wrap(result)
    }
}
