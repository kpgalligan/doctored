import AppKit

// MARK: - Developed source inspection

extension RasterDocument {
    /// Genuine u16 master before accumulated geometry, masks or retouch. The
    /// handle shares storage; the byte fallback never enters this path.
    func sourceMaster16(_ index: Int) throws -> RasterPrecisionImage {
        guard isValidIndex(index) else { throw DoctoredCoreError(message: "Invalid source layer.") }
        var error: UnsafeMutablePointer<CChar>?
        let result: OpaquePointer?
        if let precision {
            result = dr_precision_doc_source_master16(precision.ptr, index, &error)
        } else {
            result = dr_doc_source_master16(ptr, index, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "This layer has no retained 16-bit master."))
        }
        return RasterPrecisionImage(owning: result)
    }

    /// The retained developed master at its own depth: a 16-bit SDR
    /// development's u16 master or a 32-bit float (linear) development's f32
    /// one. Every reader of a master that may be f32 uses this; a byte
    /// document's master is always u16.
    func sourceMaster(_ index: Int) throws -> RasterPrecisionImage {
        guard let precision else { return try sourceMaster16(index) }
        guard isValidIndex(index) else { throw DoctoredCoreError(message: "Invalid source layer.") }
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_source_master(precision.ptr, index, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "This layer has no retained developed master."))
        }
        return RasterPrecisionImage(owning: result)
    }
}

extension RasterPrecisionImage {
    /// True for rgba32_float storage (a 32-bit float development's master),
    /// false for rgba16_unorm. Reads no pixel.
    var isFloatStorage: Bool { dr_precision_image_storage(ptr) == 32 }

    /// The image the develop inspection calls below read, which are defined
    /// on u16 codes. A u16 image answers itself. An f32 image (a 32-bit float
    /// development) answers its SDR-clipped view: RGB clamped to 0…1, encoded
    /// by its own profile curve, so a sample at or above white reads 65535
    /// and the highlight clipping marker shows it.
    func developInspection(control: RasterJobControl? = nil) throws -> RasterPrecisionImage {
        let context = try PrecisionWorkContext(image: self)
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_image_develop_inspection(ptr, context.limit,
            context.otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not prepare the developed source for inspection."))
        }
        return RasterPrecisionImage(owning: result)
    }

    /// Full-source bins and independent exact endpoint counts; never a sampled
    /// fit preview. Each inspection call consumes its own optional job token.
    /// An f32 image (a 32-bit float development, #4) answers the float
    /// histogram the Info and Levels readouts use (the scoped analysis's
    /// float schema: stop bins over −12…+6 EV, above-white and below-zero
    /// counts), admitted at that request's planned scratch.
    func developHistogram(control: RasterJobControl? = nil) throws -> [String: Any] {
        if isFloatStorage {
            let request = #"{"operation":"histogram","source":"composite","stride":1}"#
            let plan = try scopedAnalysisPlan(request)
            _ = try RasterResources.shared.admit(
                scratch: try PrecisionDocumentDescriptor.integer(plan, "scratch_bytes"))
            let context = try PrecisionWorkContext(image: self)
            return try scopedAnalysis(request, coverage: [], workingLimit: context.limit,
                                      otherRetained: context.otherRetained, control: control)
        }
        _ = try RasterResources.shared.admit(scratch: 1024 * 1024)
        let context = try PrecisionWorkContext(image: self)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_image_histogram_checked(ptr, 1, context.limit,
            context.otherRetained, control?.ptr, &error)
        return try precisionJSON(result, error)
    }

    private func developRegion(x: Int, y: Int, width: Int, height: Int) throws
        -> (x: UInt32, y: UInt32, width: UInt32, height: UInt32) {
        guard [x, y, width, height].allSatisfy({ (0...Int(UInt32.max)).contains($0) }),
              width > 0, height > 0 else {
            throw DoctoredCoreError(message: "Invalid developed-source inspection region.")
        }
        return (UInt32(x), UInt32(y), UInt32(width), UInt32(height))
    }

    /// Exact source pixels/profile, capped at 1024 pixels per side.
    func developDetail(x: Int, y: Int, width: Int, height: Int,
                       control: RasterJobControl? = nil) throws -> RasterPrecisionImage {
        let region = try developRegion(x: x, y: y, width: width, height: height)
        let context = try PrecisionWorkContext(image: self)
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_image_develop_detail(ptr, region.x, region.y,
            region.width, region.height, context.limit, context.otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not inspect developed detail."))
        }
        return RasterPrecisionImage(owning: result)
    }

    /// Display only, at most 1600 pixels per side. Divisor1 preserves pixel
    /// positions; fit colors sample cell centers. Clipping bits1/2 mark exact
    /// RGB0/RGB65535 over every original sample per cell: blue/red/magenta.
    /// Carry this source's profileData when displaying the returned byte image.
    func developPreview(x: Int = 0, y: Int = 0, width: Int, height: Int,
                        divisor: Int = 1, clipping: UInt32 = 0,
                        control: RasterJobControl? = nil) throws -> RasterImage {
        let region = try developRegion(x: x, y: y, width: width, height: height)
        guard (1...Int(UInt32.max)).contains(divisor), clipping <= 3 else {
            throw DoctoredCoreError(message: "Invalid develop preview scale or clipping mode.")
        }
        let context = try PrecisionWorkContext(image: self)
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_image_develop_preview(ptr, region.x, region.y,
            region.width, region.height, UInt32(divisor), clipping, context.limit,
            context.otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not render developed inspection."))
        }
        return RasterImage(owning: result)
    }

    /// The extended-range preview of an f32 image (a 32-bit float
    /// development, #4) at Fit or 100 % (divisor 1): the cell centres
    /// `developPreview` samples, through rule X at gain 1, as RGBA binary16
    /// little-endian premultiplied pixels (ceil(region/divisor) per side, at
    /// most 1600) linear in this image's primaries. Clipping bits 1/2 mark a
    /// cell holding an RGB sample at or below 0.0 / at or above 1.0 as opaque
    /// blue/red/magenta. Answers the largest RGB value of the picture
    /// (markers excluded) and the marked pixel count. Refuses a u16 image.
    func developPreviewExtended(x: Int, y: Int, width: Int, height: Int, divisor: Int,
                                clipping: UInt32 = 0, control: RasterJobControl? = nil) throws
        -> (pixels: Data, peak: Float, marked: UInt64) {
        let region = try developRegion(x: x, y: y, width: width, height: height)
        guard (1...Int(UInt32.max)).contains(divisor), clipping <= 3 else {
            throw DoctoredCoreError(message: "Invalid develop preview scale or clipping mode.")
        }
        let outWidth = (width + divisor - 1) / divisor, outHeight = (height + divisor - 1) / divisor
        let (count, overflow) = outWidth.multipliedReportingOverflow(by: outHeight)
        let (length, lengthOverflow) = count.multipliedReportingOverflow(by: 8)
        guard !overflow, !lengthOverflow, length > 0 else {
            throw DoctoredCoreError(message: "The extended develop preview is too large.")
        }
        let context = try PrecisionWorkContext(image: self)
        var pixels = Data(count: length)
        var peak: Float = 0
        var marked: UInt64 = 0
        var error: UnsafeMutablePointer<CChar>?
        let ok = pixels.withUnsafeMutableBytes { out in
            dr_precision_image_develop_preview_extended(ptr, region.x, region.y, region.width,
                region.height, UInt32(divisor), clipping, out.bindMemory(to: UInt8.self).baseAddress,
                out.count, &peak, &marked, context.limit, context.otherRetained, control?.ptr, &error)
        }
        guard ok else {
            throw DoctoredCoreError(message: takeErrorMessage(
                error, fallback: "Could not render the extended develop preview."))
        }
        return (pixels, peak, marked)
    }
}
