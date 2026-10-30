import AppKit

// MARK: - Embedded ordinary source assets
extension RasterPrecisionDocument {
    func retainedSourceInfo(_ index: Int) throws -> [String: Any]? {
        guard index >= 0 else { throw DoctoredCoreError(message: "Invalid source layer.") }
        _ = try RasterResources.shared.admit(scratch: dr_precision_doc_source_info_scratch_bytes(ptr, index))
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_source_info(ptr, index, &error)
        let info = try precisionJSON(result, error)
        return info["valid"] as? Bool == true ? info : nil
    }

    func retainedSourcePlan(asset: RasterPrecisionDocument? = nil, request: String) throws -> [String: Any] {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_source_plan(ptr, asset?.ptr, request, &error)
        return try precisionJSON(result, error)
    }

    func editingRetainedSource(asset: RasterPrecisionDocument? = nil, request: String,
                               workingLimit: UInt64, otherRetained: UInt64,
                               control: RasterJobControl? = nil) throws -> RasterPrecisionDocument? {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_source_edit(ptr, asset?.ptr, request, workingLimit,
                                                  otherRetained, control?.ptr, &error)
        return try editResult(result, error)
    }

    /// The existing numeric sheet and MCP affine command share this dispatch.
    /// A typed source's plan and computation must both refer to its master.
    func retainedAffineRequest(_ request: String) throws -> String? {
        guard var fields = try JSONSerialization.jsonObject(with: Data(request.utf8)) as? [String: Any],
              fields["op"] as? String == "affine", fields["target"] as? String == "pixels",
              let index = fields["layer"] as? Int, try retainedSourceInfo(index) != nil else { return nil }
        fields["op"] = "transform"
        fields.removeValue(forKey: "target")
        return try PrecisionPixelCommand(fields).request
    }
}

extension RasterDocument {
    func retainedSourceInfo(_ index: Int) throws -> [String: Any]? {
        guard isValidIndex(index) else { throw DoctoredCoreError(message: "Invalid source layer.") }
        if let precision { return try precision.retainedSourceInfo(index) }
        _ = try RasterResources.shared.admit(scratch: dr_doc_source_info_scratch_bytes(ptr, index))
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_doc_source_info(ptr, index, &error)
        let info = try precisionJSON(result, error)
        return info["valid"] as? Bool == true ? info : nil
    }

    func retainedSourcePlan(asset: RasterDocument? = nil, request: String) throws -> [String: Any] {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        let result: UnsafeMutablePointer<CChar>?
        if let precision {
            result = dr_precision_doc_mixed_source_plan(precision.ptr, asset?.ptr,
                                                        asset?.precision?.ptr, request, &error)
        } else {
            result = dr_doc_mixed_source_plan(ptr, asset?.ptr, asset?.precision?.ptr, request, &error)
        }
        return try precisionJSON(result, error)
    }

    func editingRetainedSource(asset: RasterDocument? = nil, request: String,
                               control: RasterJobControl? = nil) throws -> RasterDocument? {
        let plan = try retainedSourcePlan(asset: asset, request: request)
        let report = try RasterResources.shared.report()
        let retained = try PrecisionDocumentDescriptor.integer(report, "unique_retained_bytes")
        let transient = try PrecisionDocumentDescriptor.integer(report, "transient_estimate_bytes")
        let own = try PrecisionDocumentDescriptor.integer(plan, "retained_bytes")
        // Source plans inventory both handles as one live set. Shared masters
        // are counted once. The registry includes every other history/job owner.
        guard own <= retained else { throw DoctoredCoreError(message: "Embedded source is missing from resource accounting.") }
        let other = try PrecisionDocumentDescriptor.sum(retained - own, transient)
        let limit = RasterResources.shared.workingLimit
        var error: UnsafeMutablePointer<CChar>?
        if let precision {
            let result = dr_precision_doc_mixed_source_edit(precision.ptr, asset?.ptr,
                asset?.precision?.ptr, request, limit, other, control?.ptr, &error)
            if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Image source edit failed.")) }
            guard let result else { return nil }
            return try RasterDocument(precision: RasterPrecisionDocument(owning: result))
        }
        let result = dr_doc_mixed_source_edit(ptr, asset?.ptr, asset?.precision?.ptr,
                                              request, limit, other, control?.ptr, &error)
        if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Image source edit failed.")) }
        return result.map { RasterDocument(owning: $0) }
    }
}
