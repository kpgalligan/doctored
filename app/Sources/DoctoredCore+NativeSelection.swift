import AppKit

// MARK: - Native selection coverage (additive u16 ABI)
extension RasterSelection {
    static func nativeBrushPlan(width: Int, height: Int, request: String, dabs: [Double],
                                workingLimit: UInt64, otherRetained: UInt64) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let pointer = dabs.withUnsafeBufferPointer {
            dr_brush_u16_plan(UInt32(width), UInt32(height), request, $0.baseAddress,
                dabs.count / 4, workingLimit, otherRetained, &error)
        }
        return try precisionJSON(pointer, error)
    }

    static func nativeBrush(width: Int, height: Int, request: String, dabs: [Double],
                            workingLimit: UInt64, otherRetained: UInt64,
                            control: RasterJobControl) throws -> [UInt16] {
        var error: UnsafeMutablePointer<CChar>?
        var output = [UInt16](repeating: 0, count: width * height)
        let ok = dabs.withUnsafeBufferPointer { input in
            output.withUnsafeMutableBufferPointer {
                dr_brush_u16(UInt32(width), UInt32(height), request, input.baseAddress,
                    dabs.count / 4, $0.baseAddress, $0.count, workingLimit, otherRetained, control.ptr, &error)
            }
        }
        guard ok else { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Native brush coverage failed.")) }
        return output
    }

    static func nativePlan(width: Int, height: Int, request: String,
                           workingLimit: UInt64, otherRetained: UInt64) throws -> [String: Any] {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw DoctoredCoreError(message: "Native selection dimensions exceed their limit.")
        }
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        return try precisionJSON(dr_selection_u16_plan(UInt32(width), UInt32(height), request,
            workingLimit, otherRetained, &error), error)
    }

    static func nativeTransform(_ samples: [UInt16], width: Int, height: Int, request: String,
                                workingLimit: UInt64, otherRetained: UInt64,
                                control: RasterJobControl? = nil) throws -> [UInt16] {
        let plan = try nativePlan(width: width, height: height, request: request,
            workingLimit: workingLimit, otherRetained: otherRetained)
        let outputCount = try PrecisionDocumentDescriptor.integer(plan, "output_samples")
        guard try PrecisionDocumentDescriptor.integer(plan, "input_samples") == UInt64(samples.count),
              outputCount > 0, outputCount <= UInt64(width * height) * 3 else {
            throw DoctoredCoreError(message: "Native selection samples do not match the request.")
        }
        var output = [UInt16](repeating: 0, count: Int(outputCount))
        var error: UnsafeMutablePointer<CChar>?
        let success = output.withUnsafeMutableBufferPointer { result in
            samples.withUnsafeBufferPointer { input in
                dr_selection_u16(UInt32(width), UInt32(height), request, input.baseAddress, input.count,
                    result.baseAddress, result.count, workingLimit, otherRetained, control?.ptr, &error)
            }
        }
        guard success else { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Native selection operation failed.")) }
        return output
    }
}

extension RasterPrecisionDocument {
    func selection(request: String, input: [UInt16] = [], workingLimit: UInt64,
                   otherRetained: UInt64, control: RasterJobControl? = nil) throws -> [UInt16] {
        let plan = try pixelEditPlan(request: request, workingLimit: workingLimit, otherRetained: otherRetained)
        let count = try PrecisionDocumentDescriptor.integer(plan, "output_samples")
        guard count <= UInt64(RasterImage.maxResizePixels),
              try PrecisionDocumentDescriptor.integer(plan, "input_samples") == UInt64(input.count) else {
            throw DoctoredCoreError(message: "Native source selection exceeds its allocation or input limit.")
        }
        var output = [UInt16](repeating: 0, count: Int(count))
        var error: UnsafeMutablePointer<CChar>?
        let success = output.withUnsafeMutableBufferPointer { result in
            input.withUnsafeBufferPointer { input in
                dr_precision_doc_select(ptr, request, input.baseAddress, input.count,
                    result.baseAddress, result.count, workingLimit, otherRetained, control?.ptr, &error)
            }
        }
        guard success else { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Native source selection failed.")) }
        return output
    }
}

extension RasterSelection {
    static func nativeInk(red: Double, green: Double, blue: Double) throws -> UInt16 {
        var result: UInt16 = 0
        var error: UnsafeMutablePointer<CChar>?
        guard dr_scalar_ink_u16(red, green, blue, &result, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Invalid native scalar ink."))
        }
        return result
    }

    /// The code Fill and Stroke write into one colour channel of an 8-bit
    /// layer: the swatch's 30/59/11 luma rounded to a code. nil only
    /// when the core refuses.
    static func planeInk8(red: UInt8, green: UInt8, blue: UInt8) -> UInt8? {
        var result: UInt8 = 0
        guard dr_plane_ink_u8(red, green, blue, &result, nil) else { return nil }
        return result
    }

    /// The sample Fill and Stroke write into one colour channel of a 16-bit
    /// layer: the encoded swatch's 30/59/11 luma, unrounded until its
    /// one 16-bit quantization.
    static func planeInk16(red: Double, green: Double, blue: Double) throws -> UInt16 {
        var result: UInt16 = 0
        var error: UnsafeMutablePointer<CChar>?
        guard dr_plane_ink_u16(red, green, blue, &result, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Invalid colour plane ink."))
        }
        return result
    }
}

extension RasterPrecisionDocument {
    func scalarAutoLevels(request: String, input: [UInt16], workingLimit: UInt64,
                          otherRetained: UInt64, control: RasterJobControl? = nil) throws -> [String: Any] {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        let result = input.withUnsafeBufferPointer { samples in
            dr_precision_doc_scalar_auto_levels(ptr, request, samples.baseAddress, samples.count,
                workingLimit, otherRetained, control?.ptr, &error)
        }
        return try precisionJSON(result, error)
    }

    func scalarStatistics(request: String, input: [UInt16], workingLimit: UInt64,
                          otherRetained: UInt64, control: RasterJobControl? = nil) throws -> [String: Any] {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        let result = input.withUnsafeBufferPointer { samples in
            dr_precision_doc_scalar_statistics(ptr, request, samples.baseAddress, samples.count,
                workingLimit, otherRetained, control?.ptr, &error)
        }
        return try precisionJSON(result, error)
    }
}

extension RasterPrecisionDocument {
    func requireChannelCapacity(additional: Int) throws {
        guard additional >= 0 else { throw DoctoredCoreError(message: "The added channel count cannot be negative.") }
        var error: UnsafeMutablePointer<CChar>?
        guard dr_precision_doc_channel_capacity(ptr, additional, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The complete channel set exceeds native document capacity."))
        }
    }
}
