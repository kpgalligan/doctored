import AppKit

// MARK: - Phase 9F selected native adjustments
extension RasterPrecisionDocument {
    func selectedEditPlan(request: String, workingLimit: UInt64, otherRetained: UInt64) throws -> [String: Any] {
        try precisionCString(request)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_selected_edit_plan(ptr, request, workingLimit, otherRetained, &error)
        return try precisionJSON(result, error)
    }

    func selectedEdit(request: String, coverage: [UInt16], workingLimit: UInt64, otherRetained: UInt64,
                      control: RasterJobControl?) throws -> RasterPrecisionDocument? {
        try precisionCString(request)
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer { mask in
            dr_precision_doc_selected_edit(ptr, request, mask.baseAddress, mask.count,
                workingLimit, otherRetained, control?.ptr, &error)
        }
        return try editResult(result, error)
    }

    func selectedAutoLevels(request: String, coverage: [UInt16], workingLimit: UInt64, otherRetained: UInt64,
                            control: RasterJobControl?) throws -> [String: Any] {
        try precisionCString(request)
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer { mask in
            dr_precision_doc_selected_auto_levels(ptr, request, mask.baseAddress, mask.count,
                workingLimit, otherRetained, control?.ptr, &error)
        }
        return try precisionJSON(result, error)
    }
}

// MARK: Phase 9F scoped native-code statistics
extension RasterDocument {
    func scopedAnalysisPlan(_ request: String) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result: UnsafeMutablePointer<CChar>?
        if let precision { result = dr_precision_doc_scoped_analysis_plan(precision.ptr, request, &error) }
        else { result = dr_doc_scoped_analysis_plan(ptr, request, &error) }
        return try precisionJSON(result, error)
    }

    func scopedAnalysis(_ request: String, coverage: [UInt16], workingLimit: UInt64,
                        otherRetained: UInt64, control: RasterJobControl?) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer { buffer in
            if let precision {
                return dr_precision_doc_scoped_analysis(precision.ptr, request, buffer.baseAddress,
                    buffer.count, workingLimit, otherRetained, control?.ptr, &error)
            }
            return dr_doc_scoped_analysis(ptr, request, buffer.baseAddress, buffer.count,
                workingLimit, otherRetained, control?.ptr, &error)
        }
        return try precisionJSON(result, error)
    }

    /// The 8-bit composite scope over this document's supplied (retained)
    /// projection: the same schema, with no second render.
    func projectionScopedAnalysisPlan(_ request: String) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_projection_scoped_analysis_plan(ptr, request, &error)
        return try precisionJSON(result, error)
    }

    func projectionScopedAnalysis(_ projection: RasterImage, _ request: String,
                                  coverage: [UInt16], workingLimit: UInt64,
                                  otherRetained: UInt64,
                                  control: RasterJobControl?) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer { buffer in
            withExtendedLifetime(projection) {
                dr_doc_projection_scoped_analysis(ptr, projection.ptr, request,
                    buffer.baseAddress, buffer.count, workingLimit, otherRetained,
                    control?.ptr, &error)
            }
        }
        return try precisionJSON(result, error)
    }
}

extension RasterPrecisionImage {
    /// The composite scope over a supplied (retained) projection: the
    /// same schema as the document, with no second render.
    func scopedAnalysisPlan(_ request: String) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_image_scoped_analysis_plan(ptr, request, &error)
        return try precisionJSON(result, error)
    }

    func scopedAnalysis(_ request: String, coverage: [UInt16], workingLimit: UInt64,
                        otherRetained: UInt64, control: RasterJobControl?) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer { buffer in
            dr_precision_image_scoped_analysis(ptr, request, buffer.baseAddress, buffer.count,
                workingLimit, otherRetained, control?.ptr, &error)
        }
        return try precisionJSON(result, error)
    }
}

extension RasterImage {
    /// Historical whole-target calls keep their exact byte algorithm; the
    /// explicit selection scope adds coverage-weighted endpoint measurement.
    func scopedAutoLevels(mask: [UInt8]?, mode: DrAutoMode, clip: Double)
        throws -> (black: [Double], white: [Double], gamma: [Double])? {
        guard let mask else { return autoLevels(mask: nil, mode: mode, clip: clip) }
        guard mask.count == width * height else { throw DoctoredCoreError(message: "Invalid Auto analysis coverage.") }
        var error: UnsafeMutablePointer<CChar>?
        let pointer = mask.withUnsafeBufferPointer { bytes in
            dr_image_selected_auto_levels(ptr, bytes.baseAddress, bytes.count, mode.rawValue, clip, &error)
        }
        let result = try precisionJSON(pointer, error)
        if result["identity"] as? Bool == true { return nil }
        guard let black = result["black"] as? [Double], let white = result["white"] as? [Double],
              let gamma = result["gamma"] as? [Double] else { throw DoctoredCoreError(message: "Invalid Auto analysis result.") }
        return (black, white, gamma)
    }
}
