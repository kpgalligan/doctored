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

    /// `dr_brush_u16`, or `dr_brush_u16_shaped` when `shapes` (one angle and roundness
    /// pair per dab, brush dynamics' tip shape) is given, or `dr_brush_u16_textured` (with
    /// or without `shapes`, the canvas at origin 0, 0) when `texture` is. With `dual` (the
    /// second tip's request and dabs), `dr_brush_u16_dual` with `shapes` and `texture` each
    /// optional.
    static func nativeBrush(width: Int, height: Int, request: String, dabs: [Double],
                            shapes: [Double]? = nil, texture: RasterBrushTexture? = nil,
                            dual: (request: String, dabs: [Double])? = nil,
                            workingLimit: UInt64, otherRetained: UInt64,
                            control: RasterJobControl) throws -> [UInt16] {
        var error: UnsafeMutablePointer<CChar>?
        var output = [UInt16](repeating: 0, count: width * height)
        let ok = dabs.withUnsafeBufferPointer { input in
            output.withUnsafeMutableBufferPointer { out in
                if let dual {
                    func call(_ pairs: UnsafePointer<Double>?) -> Bool {
                        dual.dabs.withUnsafeBufferPointer { second in
                            dr_brush_u16_dual(UInt32(width), UInt32(height), request,
                                input.baseAddress, dabs.count / 4, pairs, texture?.ptr, 0, 0,
                                dual.request, second.baseAddress, dual.dabs.count / 4,
                                out.baseAddress, out.count, workingLimit, otherRetained,
                                control.ptr, &error)
                        }
                    }
                    guard let shapes else { return call(nil) }
                    return shapes.withUnsafeBufferPointer { call($0.baseAddress) }
                }
                if let texture {
                    func call(_ pairs: UnsafePointer<Double>?) -> Bool {
                        dr_brush_u16_textured(UInt32(width), UInt32(height), request,
                            input.baseAddress, dabs.count / 4, pairs, texture.ptr, 0, 0,
                            out.baseAddress, out.count, workingLimit, otherRetained, control.ptr,
                            &error)
                    }
                    guard let shapes else { return call(nil) }
                    return shapes.withUnsafeBufferPointer { call($0.baseAddress) }
                }
                guard let shapes else {
                    return dr_brush_u16(UInt32(width), UInt32(height), request, input.baseAddress,
                        dabs.count / 4, out.baseAddress, out.count, workingLimit, otherRetained,
                        control.ptr, &error)
                }
                return shapes.withUnsafeBufferPointer { pairs in
                    dr_brush_u16_shaped(UInt32(width), UInt32(height), request, input.baseAddress,
                        dabs.count / 4, pairs.baseAddress, out.baseAddress, out.count, workingLimit,
                        otherRetained, control.ptr, &error)
                }
            }
        }
        guard ok else { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Native brush coverage failed.")) }
        return output
    }

    /// `dr_brush_overlay_rgba8`: the same brush as `nativeBrush`, written as the
    /// canvas-sized premultiplied RGBA8 overlay the byte paint ops take, in
    /// `rgb`'s straight ink and multiplied by `selection` (canvas-sized bytes). With
    /// `texture`, `dr_brush_overlay_rgba8_textured` with `shapes` and `origin`, the canvas
    /// coordinate of the output's pixel (0, 0); with `dual` (the second tip's request and dabs),
    /// `dr_brush_overlay_rgba8_dual` with `shapes`, `texture` and `origin`. `shapes` without a
    /// texture or a dual refuses.
    static func nativeBrushOverlay(width: Int, height: Int, request: String, dabs: [Double],
                                   shapes: [Double]? = nil, texture: RasterBrushTexture? = nil,
                                   dual: (request: String, dabs: [Double])? = nil,
                                   origin: (x: Int, y: Int) = (0, 0),
                                   rgb: [UInt8], selection: [UInt8]?,
                                   workingLimit: UInt64, otherRetained: UInt64) throws -> [UInt8] {
        guard rgb.count == 3 else { throw DoctoredCoreError(message: "Native brush overlay ink needs three bytes.") }
        guard shapes == nil || texture != nil || dual != nil else {
            throw DoctoredCoreError(message: "A shaped native brush overlay needs its texture.")
        }
        var error: UnsafeMutablePointer<CChar>?
        var output = [UInt8](repeating: 0, count: width * height * 4)
        let ok = dabs.withUnsafeBufferPointer { input in
            rgb.withUnsafeBufferPointer { ink in
                output.withUnsafeMutableBufferPointer { out in
                    func call(_ mask: UnsafePointer<UInt8>?, _ maskCount: Int) -> Bool {
                        if let dual {
                            func multiplied(_ pairs: UnsafePointer<Double>?) -> Bool {
                                dual.dabs.withUnsafeBufferPointer { second in
                                    dr_brush_overlay_rgba8_dual(UInt32(width), UInt32(height),
                                        request, input.baseAddress, dabs.count / 4, pairs,
                                        texture?.ptr, Int64(origin.x), Int64(origin.y),
                                        dual.request, second.baseAddress, dual.dabs.count / 4,
                                        ink.baseAddress, mask, maskCount, out.baseAddress,
                                        out.count, workingLimit, otherRetained, nil, &error)
                                }
                            }
                            guard let shapes else { return multiplied(nil) }
                            return shapes.withUnsafeBufferPointer { multiplied($0.baseAddress) }
                        }
                        guard let texture else {
                            return dr_brush_overlay_rgba8(UInt32(width), UInt32(height), request,
                                input.baseAddress, dabs.count / 4, ink.baseAddress, mask, maskCount,
                                out.baseAddress, out.count, workingLimit, otherRetained, nil, &error)
                        }
                        func textured(_ pairs: UnsafePointer<Double>?) -> Bool {
                            dr_brush_overlay_rgba8_textured(UInt32(width), UInt32(height), request,
                                input.baseAddress, dabs.count / 4, pairs, texture.ptr,
                                Int64(origin.x), Int64(origin.y), ink.baseAddress, mask, maskCount,
                                out.baseAddress, out.count, workingLimit, otherRetained, nil, &error)
                        }
                        guard let shapes else { return textured(nil) }
                        return shapes.withUnsafeBufferPointer { textured($0.baseAddress) }
                    }
                    guard let selection else { return call(nil, 0) }
                    return selection.withUnsafeBufferPointer { call($0.baseAddress, $0.count) }
                }
            }
        }
        guard ok else { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Native brush overlay failed.")) }
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
