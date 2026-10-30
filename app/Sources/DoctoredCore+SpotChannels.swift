import AppKit

// The spot channels' core wrappers (#1 phases 1a-2 and 1c-1, the header's "Spot channels" and
// "Spot channels 1c-1" sections): every `dr_*` call of the feature. The model, the sentences and the display helpers are
// `SpotChannel.swift`'s.

extension RasterDocument {
    /// Byte channel `i`'s ink, or nil for an alpha channel or an index out of range.
    func byteChannelSpot(_ i: Int) -> SpotInk? {
        guard let ptr, i >= 0 else { return nil }
        var lab = [Double](repeating: 0, count: 3)
        var solidity: UInt8 = 0
        let ok = lab.withUnsafeMutableBufferPointer { dr_doc_channel_spot(ptr, i, $0.baseAddress, &solidity) }
        return ok ? SpotInk(lab: lab, solidity: Int(solidity)) : nil
    }

    /// Channels ▸ New Spot Channel…: a spot channel named `name` with `ink`, its coverage the
    /// selection's (`maskBytes()` at 8 bits, `maskSamples16()` typed) or none. Throws the core's
    /// sentence for an ink out of range, a full channel budget, an Indexed or Bitmap document, or
    /// a busy worker at high depth.
    func addingSpotChannel(name: String, ink: SpotInk, selection: CanvasSelection?) throws -> RasterDocument? {
        let solidity = try ink.solidityByte()
        let lab = try ink.labTriple()
        let fits = selection.map { $0.canvasWidth == width && $0.canvasHeight == height } ?? true
        guard fits else { throw DoctoredCoreError(message: "The selection does not match the canvas.") }
        if let precision {
            // The add refuses on the main thread while a job owns the worker, as `addingChannel`'s
            // import does, so refuse before building the coverage.
            if Thread.isMainThread, BackgroundWork.occupied {
                throw DoctoredCoreError(message: BackgroundWork.preparingBusyMessage)
            }
            let coverage = selection?.maskSamples16() ?? [UInt16](repeating: 0, count: width * height)
            let context = try PrecisionWorkContext(source: precision)
            let result = try precision.addingSpotChannel(
                name: name, lab: lab, solidity: solidity, coverage: coverage,
                workingLimit: context.limit, otherRetained: context.otherRetained)
            return try RasterDocument(precision: result)
        }
        guard let ptr else { return nil }
        let coverage = selection?.maskBytes() ?? [UInt8](repeating: 0, count: width * height)
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer {
            dr_doc_add_spot_channel(ptr, name, $0.baseAddress, lab[0], lab[1], lab[2], solidity, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The spot channel was refused."))
        }
        return RasterDocument(owning: result)
    }

    /// Spot Channel Options…: channel `i`'s name (nil keeps it) and ink, one step. nil when nothing
    /// would change; throws for an alpha channel ("the channel is not a spot channel"), an ink out
    /// of range or, at high depth, an index out of range or a busy worker.
    func settingSpotChannel(_ i: Int, name: String?, ink: SpotInk) throws -> RasterDocument? {
        let solidity = try ink.solidityByte()
        let lab = try ink.labTriple()
        if let precision {
            if Thread.isMainThread, BackgroundWork.occupied {
                throw DoctoredCoreError(message: BackgroundWork.preparingBusyMessage)
            }
            let context = try PrecisionWorkContext(source: precision)
            return try precision.settingSpotChannel(
                i, name: name, lab: lab, solidity: solidity,
                workingLimit: context.limit, otherRetained: context.otherRetained)
                .map { try RasterDocument(precision: $0) }
        }
        guard let ptr else { return nil }
        guard byteChannelSpot(i) != nil else {
            throw DoctoredCoreError(message: SpotChannels.notSpotRefusal)
        }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_set_spot_channel(ptr, i, name, lab[0], lab[1], lab[2], solidity, &error)
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The spot channel was refused."))
        }
        return wrap(result)
    }

    /// `composite` (a canvas-sized byte rendering of this BYTE document) with every spot channel
    /// overprinted in list order, in the document's profile; nil when it has no spot channel or is
    /// typed. Throws the core's sentence for a composite that is not canvas-sized.
    func spotOverprint(_ composite: RasterImage) throws -> RasterImage? {
        guard let ptr else { return nil }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_spot_overprint(ptr, composite.ptr, &error)
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The spot overprint failed."))
        }
        return wrapImage(result)
    }

    /// `composite`, a region / preview_divisor render of this BYTE document whose output (i, j) is
    /// canvas (x + i·divisor, y + j·divisor), with every spot channel overprinted at those samples
    /// (`dr_doc_spot_overprint_region`); nil without a spot channel or for a typed document.
    func spotOverprintRegion(_ composite: RasterImage, x: Int, y: Int, divisor: Int) throws -> RasterImage? {
        guard let ptr else { return nil }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_spot_overprint_region(ptr, composite.ptr, UInt32(clamping: x),
                                                  UInt32(clamping: y), UInt32(clamping: divisor), &error)
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The spot overprint failed."))
        }
        return wrapImage(result)
    }

    /// Merge Spot Channel on byte channel `i` (`dr_doc_merge_spot_channel`): the flattened
    /// composite with that ink baked in and the channel deleted; nil for an alpha channel or out of
    /// range, or for a typed document.
    func byteMergingSpotChannel(_ i: Int) throws -> RasterDocument? {
        guard let ptr, i >= 0 else { return nil }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_merge_spot_channel(ptr, i, &error)
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Merge Spot Channel failed."))
        }
        return wrap(result)
    }

    /// Channel Options' "Color Indicates: Spot Color" and Spot Channel Options' "Alpha Channel":
    /// channel `i` made a spot channel with `spot`, or an alpha channel with nil, its plane inverted
    /// (`dr_doc_set_channel_kind` / `dr_precision_doc_set_channel_kind`). nil when it is already
    /// that kind; throws for an ink out of range and, typed, an index out of range, an Indexed or
    /// Bitmap document asked for a spot channel, admission or a busy worker.
    func settingChannelKind(_ i: Int, spot: SpotInk?) throws -> RasterDocument? {
        let solidity = try spot?.solidityByte() ?? 0
        let lab = try spot?.labTriple() ?? [0, 0, 0]
        if let precision {
            if Thread.isMainThread, BackgroundWork.occupied {
                throw DoctoredCoreError(message: BackgroundWork.preparingBusyMessage)
            }
            guard i >= 0 else { throw DoctoredCoreError(message: "typed channel index is out of range") }
            let context = try PrecisionWorkContext(source: precision)
            var error: UnsafeMutablePointer<CChar>?
            let result = dr_precision_doc_set_channel_kind(
                precision.ptr, i, spot != nil, lab[0], lab[1], lab[2], solidity,
                context.limit, context.otherRetained, &error)
            if error != nil {
                throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The channel kind was refused."))
            }
            return try result.map { try RasterDocument(precision: RasterPrecisionDocument(owning: $0)) }
        }
        guard let ptr, i >= 0 else { return nil }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_set_channel_kind(ptr, i, spot != nil, lab[0], lab[1], lab[2], solidity, &error)
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The channel kind was refused."))
        }
        return wrap(result)
    }
}

extension RasterPrecisionDocument {
    /// The typed add (`dr_precision_doc_add_spot_channel`): `coverage` is canvas-sized u16.
    func addingSpotChannel(name: String, lab: [Double], solidity: UInt8, coverage: [UInt16],
                           workingLimit: UInt64, otherRetained: UInt64) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer {
            dr_precision_doc_add_spot_channel(ptr, name, lab[0], lab[1], lab[2], solidity,
                                              $0.baseAddress, $0.count, workingLimit, otherRetained,
                                              nil, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The spot channel was refused."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// The typed set (`dr_precision_doc_set_spot_channel`); nil when nothing changes.
    func settingSpotChannel(_ index: Int, name: String?, lab: [Double], solidity: UInt8,
                            workingLimit: UInt64, otherRetained: UInt64) throws -> RasterPrecisionDocument? {
        guard index >= 0 else { throw DoctoredCoreError(message: "typed channel index is out of range") }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_set_spot_channel(ptr, index, name, lab[0], lab[1], lab[2], solidity,
                                                       workingLimit, otherRetained, &error)
        if error != nil {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The spot channel was refused."))
        }
        return result.map(RasterPrecisionDocument.init(owning:))
    }

    /// Merge Spot Channel's typed plan (`dr_precision_doc_merge_spot_channel_plan`):
    /// `{"working_memory_plan_bytes", "refusal"}`.
    func mergeSpotChannelPlan(_ index: Int) throws -> [String: Any] {
        guard index >= 0 else { throw DoctoredCoreError(message: "typed channel index is out of range") }
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_merge_spot_channel_plan(ptr, index, &error)
        return try precisionJSON(result, error)
    }

    /// The typed Merge Spot Channel (`dr_precision_doc_merge_spot_channel`), admitted by the core
    /// against `workingLimit` beside `otherRetained`.
    func mergingSpotChannel(_ index: Int, workingLimit: UInt64, otherRetained: UInt64,
                            control: RasterJobControl?) throws -> RasterPrecisionDocument {
        guard index >= 0 else { throw DoctoredCoreError(message: "typed channel index is out of range") }
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_merge_spot_channel(ptr, index, workingLimit, otherRetained,
                                                               control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Merge Spot Channel failed."))
        }
        return RasterPrecisionDocument(owning: result)
    }
}

extension RasterImage {
    /// This image with one ink overprinted at `coverage` (exactly width × height bytes), the ink
    /// encoded in the profile `icc` (empty or unusable reads as sRGB) (`dr_image_spot_overprint`).
    func spotOverprinted(coverage: [UInt8], ink: SpotInk, icc: Data) throws -> RasterImage {
        let solidity = try ink.solidityByte()
        let lab = try ink.labTriple()
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer { cov in
            icc.withUnsafeBytes { profile in
                dr_image_spot_overprint(ptr, cov.baseAddress, cov.count, lab[0], lab[1], lab[2], solidity,
                                        profile.bindMemory(to: UInt8.self).baseAddress, profile.count,
                                        &error)
            }
        }
        guard let image = wrap(result) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The spot overprint failed."))
        }
        return image
    }

    /// This image, a region / preview_divisor render of a `canvasWidth` × `canvasHeight` canvas,
    /// with one ink overprinted at its samples: output (i, j) reads `coverage` (the CANVAS's plane)
    /// at (x + i·divisor, y + j·divisor) (`dr_image_spot_overprint_region`).
    func spotOverprintedRegion(coverage: [UInt8], canvasWidth: Int, canvasHeight: Int, x: Int, y: Int,
                               divisor: Int, ink: SpotInk, icc: Data) throws -> RasterImage {
        let solidity = try ink.solidityByte()
        let lab = try ink.labTriple()
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer { cov in
            icc.withUnsafeBytes { profile in
                dr_image_spot_overprint_region(
                    ptr, cov.baseAddress, cov.count, UInt32(clamping: canvasWidth),
                    UInt32(clamping: canvasHeight), UInt32(clamping: x), UInt32(clamping: y),
                    UInt32(clamping: divisor), lab[0], lab[1], lab[2], solidity,
                    profile.bindMemory(to: UInt8.self).baseAddress, profile.count, &error)
            }
        }
        guard let image = wrap(result) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "The spot overprint failed."))
        }
        return image
    }
}
