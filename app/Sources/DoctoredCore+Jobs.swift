import AppKit

extension RasterDocument {
    func contentAwareFillScratch(_ request: String) throws -> UInt64 {
        var err: UnsafeMutablePointer<CChar>?
        let bytes = dr_doc_content_aware_fill_scratch(ptr, request, &err)
        if let err { throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Could not plan content-aware fill.")) }
        return bytes
    }

    func contentAwareFillEdit(_ request: String, coverage: [UInt8], control: RasterJobControl) throws -> RasterDocument? {
        guard coverage.count == width * height else { throw DoctoredCoreError(message: "Fill coverage does not match its canvas.") }
        var err: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer {
            dr_doc_content_aware_fill_edit(ptr, request, $0.baseAddress, control.ptr, &err)
        }
        return try fallible(result, err, "The fill could not be applied.")
    }

    /// Spot healing: the core inpaints the overlay's footprint from a ring
    /// around it and blends the result in as `healJob` does. Only the
    /// overlay's ALPHA is read — there is no sampled source, so the overlay
    /// carries pure coverage. `ring` is the sampling-ring width in px (0 =
    /// automatic), `seed` makes the result reproducible and `type` is the
    /// Spot Healing Brush's Type. nil when nothing would change; throws the
    /// core's message on a cap or a starved sample region.
    func spotHealJob(_ index: Int, overlay: [UInt8], strength: Double, ring: Int, seed: UInt64,
                     type: SpotHealType, sample: RetouchSampling, ignoreAdjustments: Bool,
                     control: RasterJobControl) throws -> RasterDocument? {
        guard isValidIndex(index), overlay.count == width * height * 4 else {
            throw DoctoredCoreError(message: "The captured healing target or footprint is invalid.")
        }
        var err: UnsafeMutablePointer<CChar>?
        let handle = overlay.withUnsafeBufferPointer {
            dr_doc_spot_heal_layer_job(ptr, index, $0.baseAddress, UInt32(width), UInt32(height),
                Float(strength), UInt32(clamping: ring), seed, type.code, sample.code,
                ignoreAdjustments, control.ptr, &err)
        }
        return try fallible(handle, err, "The spot heal could not be applied.")
    }

    /// The Healing Brush (and the 8-bit Patch) on the worker: `dr_doc_heal_layer`
    /// under a job control, so the solve reserves its scratch against the
    /// token, polls cancellation and reports progress. `overlay` is the
    /// captured canvas-sized premultiplied overlay, copied off the canvas by
    /// the caller before the job starts.
    func healJob(_ index: Int, overlay: [UInt8], strength: Double,
                 control: RasterJobControl) throws -> RasterDocument? {
        guard isValidIndex(index), overlay.count == width * height * 4 else {
            throw DoctoredCoreError(message: "The captured healing target or footprint is invalid.")
        }
        var err: UnsafeMutablePointer<CChar>?
        let handle = overlay.withUnsafeBufferPointer {
            dr_doc_heal_layer(ptr, index, $0.baseAddress, UInt32(width), UInt32(height),
                Float(strength), control.ptr, &err)
        }
        return try fallible(handle, err, "The heal could not be applied.")
    }

    /// Healing Brush onto a NEW retouch layer, on the worker. `plate` is the
    /// captured current-and-below plate HANDLE the new layer will composite
    /// onto, read in place (`withPlateBytes`) for the call.
    func healToNewLayerJob(_ index: Int, overlay: [UInt8], strength: Double, plate: RasterImage,
                           name: String, control: RasterJobControl) throws -> RasterDocument? {
        guard isValidIndex(index), overlay.count == width * height * 4,
              plate.width == width, plate.height == height else {
            throw DoctoredCoreError(message: "The captured healing target or footprint is invalid.")
        }
        var err: UnsafeMutablePointer<CChar>?
        let result = overlay.withUnsafeBufferPointer { source in
            plate.withPlateBytes { backdrop in
                dr_doc_heal_to_new_layer(
                    ptr, index, source.baseAddress, UInt32(width), UInt32(height),
                    Float(strength), backdrop, name, control.ptr, &err)
            }
        }
        guard let handle = result else {
            throw DoctoredCoreError(message: "The captured healing target or footprint is invalid.")
        }
        return try fallible(handle, err, "The heal could not be applied to a new layer.")
    }

    /// Spot Healing onto a NEW retouch layer, on the worker. `plate` is the
    /// canvas-sized straight-RGBA8 current-and-below plate the new layer will
    /// composite onto, copied out of its handle by the caller before the job
    /// starts.
    func spotHealToNewLayerJob(_ index: Int, overlay: [UInt8], strength: Double, ring: Int,
                               seed: UInt64, type: SpotHealType, plate: [UInt8], name: String,
                               control: RasterJobControl) throws -> RasterDocument? {
        guard isValidIndex(index), overlay.count == width * height * 4,
              plate.count == width * height * 4 else {
            throw DoctoredCoreError(message: "The captured healing target or footprint is invalid.")
        }
        var err: UnsafeMutablePointer<CChar>?
        let handle = overlay.withUnsafeBufferPointer { source in
            plate.withUnsafeBufferPointer { backdrop in
                dr_doc_spot_heal_to_new_layer(
                    ptr, index, source.baseAddress, UInt32(width), UInt32(height),
                    Float(strength), UInt32(clamping: ring), seed, type.code,
                    backdrop.baseAddress, name, control.ptr, &err)
            }
        }
        return try fallible(handle, err, "The spot heal could not be applied to a new layer.")
    }

    /// One allocation union across both storage modes, including cross-depth
    /// masters shared by current documents, decoded inputs and history.
    static func mixedResourceReport(documents: [RasterDocument], images: [RasterImage],
                                    precisionDocuments: [RasterPrecisionDocument],
                                    precisionImages: [RasterPrecisionImage],
                                    rawLocalMasks: [RasterRawLocalMask] = [],
                                    scratch: UInt64, limit: UInt64) throws -> [String: Any] {
        var err: UnsafeMutablePointer<CChar>?
        let docs = documents.compactMap { $0.ptr }.map { Optional($0) }
        let rasters = images.map { Optional($0.ptr) }
        let typedDocs = precisionDocuments.map { Optional($0.ptr) }
        let typedImages = precisionImages.map { Optional($0.ptr) }
        let masks = rawLocalMasks.map { Optional($0.ptr) }
        let result = docs.withUnsafeBufferPointer { d in
            rasters.withUnsafeBufferPointer { i in
                typedDocs.withUnsafeBufferPointer { td in
                    typedImages.withUnsafeBufferPointer { ti in
                        masks.withUnsafeBufferPointer { m in
                            dr_mixed_resources_report_with_raw_masks(d.baseAddress, d.count, i.baseAddress, i.count,
                                td.baseAddress, td.count, ti.baseAddress, ti.count, m.baseAddress, m.count, scratch, limit, &err)
                        }
                    }
                }
            }
        }
        return try resourceJSON(result, err)
    }

    static func resourceReport(documents: [RasterDocument], images: [RasterImage],
                               scratch: UInt64, limit: UInt64) throws -> [String: Any] {
        var report = try mixedResourceReport(documents: documents, images: images,
            precisionDocuments: documents.compactMap(\.precision), precisionImages: [],
            scratch: scratch, limit: limit)
        try RasterResources.mergePrecisionPayloads(documents.compactMap { $0.precisionMetadata?.payload },
                                                    into: &report, limit: limit)
        let typedProjection = documents.compactMap {
            ($0.precisionInfo?["render_scratch_bytes"] as? NSNumber)?.uint64Value
        }.max() ?? 0
        let legacyProjection = (report["projection_scratch_estimate_bytes"] as? NSNumber)?.uint64Value ?? 0
        report["projection_scratch_estimate_bytes"] = max(legacyProjection, typedProjection)
        report["document_handle_count"] = documents.count
        return report
    }

    static func evictResourceCaches(_ documents: [RasterDocument],
                                    precisionDocuments: [RasterPrecisionDocument] = []) throws -> [String: Any] {
        let docs = documents.compactMap { $0.ptr }.map { Optional($0) }
        let typed = (precisionDocuments + documents.compactMap(\.precision)).map { Optional($0.ptr) }
        var error: UnsafeMutablePointer<CChar>?
        let result = docs.withUnsafeBufferPointer { docs in
            typed.withUnsafeBufferPointer { typed in
                dr_mixed_resources_evict_caches(docs.baseAddress, docs.count,
                                                typed.baseAddress, typed.count, &error)
            }
        }
        return try precisionJSON(result, error)
    }

    private static func resourceJSON(_ result: UnsafeMutablePointer<CChar>?,
                                     _ err: UnsafeMutablePointer<CChar>?) throws -> [String: Any] {
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(err, fallback: "Resource inspection failed."))
        }
        defer { dr_string_free(result) }
        guard let json = try JSONSerialization.jsonObject(with: Data(String(cString: result).utf8))
                as? [String: Any] else {
            throw DoctoredCoreError(message: "Invalid resource report.")
        }
        return json
    }
}

extension RasterDocument {
    /// The display projection's charge: the core's structural estimate of the
    /// peak projection workspace (nested group/clip/style and adjustment
    /// buffers, supplied by the same core planner fill uses), plus
    /// `ProjectionMargin`'s fitted margin for its depth, so every
    /// caller is charged ≥ 1.25× the measured projection peak.
    /// 8-bit: the core estimate plus the n×4 display CGImage payload.
    /// High-depth: the typed render, then the display conversion, run one after the
    /// other (the max-of-sequential-stages rule), so the structural peak is
    /// max(render_scratch_bytes, render_output_bytes + display_scratch_bytes)
    /// plus display_overhead_bytes (page tables and edge-tile rounding).
    /// A missing render key throws render_refusal (as today); overflow throws.
    func projectionScratchEstimate() throws -> UInt64 {
        if let precision {
            let descriptor = try precision.descriptor()
            guard descriptor["render_scratch_bytes"] is NSNumber else {
                throw DoctoredCoreError(message: descriptor["render_refusal"] as? String
                    ?? "This high-depth document cannot render within the supported limits.")
            }
            let render = try PrecisionDocumentDescriptor.integer(descriptor, "render_scratch_bytes")
            let output = try PrecisionDocumentDescriptor.integer(descriptor, "render_output_bytes")
            let display = try PrecisionDocumentDescriptor.integer(descriptor, "display_scratch_bytes")
            let overhead = try PrecisionDocumentDescriptor.integer(descriptor, "display_overhead_bytes")
            return try ProjectionMargin.charged(PrecisionDocumentDescriptor.sum(
                max(render, try PrecisionDocumentDescriptor.sum(output, display)), overhead), highDepth: true)
        }
        let report = try Self.resourceReport(documents: [self], images: [], scratch: 0, limit: 0)
        guard let bytes = (report["projection_scratch_estimate_bytes"] as? NSNumber)?.uint64Value else {
            throw DoctoredCoreError(message: "Could not estimate projection working memory.")
        }
        // The display CGImage payload is the n×4 term.
        let (pixels, pixelOverflow) = UInt64(width).multipliedReportingOverflow(by: UInt64(height))
        let (payload, payloadOverflow) = pixels.multipliedReportingOverflow(by: 4)
        guard !pixelOverflow, !payloadOverflow else {
            throw DoctoredCoreError(message: "Working-memory calculation overflow.")
        }
        return try ProjectionMargin.charged(
            PrecisionDocumentDescriptor.sum(bytes, payload), highDepth: false)
    }
}

extension RasterDocument {
    /// Select and Mask's New Layer assembly budget (`dr_doc_add_refinement_cutout`).
    /// This excludes the caller's alpha buffer and the later display projection.
    var refinementCutoutScratchBytes: UInt64 { dr_doc_refinement_cutout_scratch_bytes(ptr) }

    func refinementCutoutRenderScratchBytes() throws -> UInt64 {
        var error: UnsafeMutablePointer<CChar>?
        let bytes = dr_doc_refinement_cutout_render_scratch_bytes(ptr, &error)
        guard bytes > 0 else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Invalid byte cutout projection."))
        }
        return bytes
    }

    func renderRefinementCutout(control: RasterJobControl) throws -> RasterImage {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_doc_render_refinement_cutout(ptr, control.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Byte cutout projection failed."))
        }
        return RasterImage(owning: result)
    }

    /// `foreground` contains RGB in this document's profile and keeps its own
    /// alpha on the new layer; `alpha` becomes a separate editable mask.
    /// Previous top-level entries are hidden. A result with nothing visible
    /// (alpha times the foreground's alpha is zero everywhere) is a no-op.
    /// Inputs survive every success/refusal unchanged.
    func addingRefinementCutout(foreground: RasterImage, alpha: [UInt8],
                                control: RasterJobControl? = nil) throws -> RasterDocument? {
        guard foreground.width == width, foreground.height == height,
              alpha.count == width * height else {
            throw DoctoredCoreError(message: "Refinement foreground and alpha must match the canvas.")
        }
        var error: UnsafeMutablePointer<CChar>?
        let result = alpha.withUnsafeBufferPointer { bytes in
            dr_doc_add_refinement_cutout(ptr, foreground.ptr, bytes.baseAddress, control?.ptr, &error)
        }
        if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Refinement output failed.")) }
        return result.map { RasterDocument(owning: $0) }
    }
}

extension DoctoredCoreProcess {
    /// The post-projection trim threshold for non-view cache bytes (the
    /// disposable caches an eviction drops): the working limit / 8. It follows
    /// `setWorkingLimit`; 0 on a core internal error.
    static func cacheTrimLimit() -> UInt64 {
        dr_process_cache_trim_limit()
    }
}
