import AppKit

extension RasterDocument {
    // MARK: - Layer metadata

    /// Layer `idx`'s metadata string, or nil when it carries none. The core
    /// stores this blob without ever parsing it; the schema is the app's
    /// (see TextLayer.swift).
    func layerMeta(_ idx: Int) -> String? {
        if let precision {
            if let rows = precisionInfo?["layers"] as? [[String: Any]], rows.indices.contains(idx),
               let bytes = rows[idx]["description_bytes"] as? NSNumber {
                guard bytes.uint64Value <= UInt64(NativeArtwork.maximumDescriptionBytes) else { return nil }
                do { _ = try RasterResources.shared.admit(scratch: bytes.uint64Value * 16 + 1024 * 1024) }
                catch { return nil }
                guard let text = dr_precision_doc_artwork_description(precision.ptr, idx) else { return nil }
                defer { dr_string_free(text) }
                return String(cString: text)
            }
            guard let rows = precisionInfo?["layers"] as? [[String: Any]], rows.indices.contains(idx),
                  let adjustment = rows[idx]["adjustment"] as? [String: Any],
                  let bytes = adjustment["description_bytes"] as? NSNumber,
                  bytes.uint64Value <= 16 * 1024 * 1024 else { return nil }
            do {
                _ = try RasterResources.shared.admit(scratch: bytes.uint64Value * 8 + 1024 * 1024)
                return try precision.adjustmentDescription(idx)
            } catch { return nil }
        }
        guard isValidIndex(idx), let cMeta = dr_doc_layer_meta(ptr, idx) else { return nil }
        defer { dr_string_free(cMeta) }
        return String(cString: cMeta)
    }

    /// Attaches `meta` to layer `idx`, or CLEARS its metadata when `meta` is
    /// nil. nil comes back for an over-long payload (the core caps it at
    /// 16 MiB, the native format's own limit).
    func withLayerMeta(_ idx: Int, _ meta: String?) -> RasterDocument? {
        guard isValidIndex(idx) else { return nil }
        if let precision {
            do {
                let context = try PrecisionWorkContext(source: precision)
                var error: UnsafeMutablePointer<CChar>?
                let result: OpaquePointer?
                if let meta {
                    try precisionCString(meta, maximumBytes: NativeArtwork.maximumDescriptionBytes)
                    result = dr_precision_doc_artwork_metadata(precision.ptr, idx, meta, context.limit, context.otherRetained, &error)
                } else {
                    result = dr_precision_doc_artwork_metadata(precision.ptr, idx, nil, context.limit, context.otherRetained, &error)
                }
                if let error { throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Artwork metadata failed.")) }
                guard let result else { return nil }
                return try RasterDocument(precision: RasterPrecisionDocument(owning: result))
            } catch { return nil }
        }
        guard let meta = meta else { return wrap(dr_doc_with_layer_meta(ptr, idx, nil)) }
        return wrap(dr_doc_with_layer_meta(ptr, idx, meta))
    }

    /// Whether layer `idx`'s metadata parses as a color-adjustment
    /// description ({"type":"adjust", ...} — the ONE meta shape the core
    /// itself interprets): such a layer composites as an adjustment of the
    /// backdrop and its own pixels are ignored. Asked of the CORE, whose
    /// parse is the compositor's, so the answer can never drift from what
    /// actually renders; the Swift-side parse (AdjustmentLayer.swift) is
    /// only for reading the op and params back into a dialog.
    func layerIsAdjustment(_ idx: Int) -> Bool {
        if let precisionMetadata { return precisionMetadata.layer(idx)?.adjustment == true }
        guard isValidIndex(idx) else { return false }
        return dr_doc_layer_is_adjustment(ptr, idx)
    }
}
