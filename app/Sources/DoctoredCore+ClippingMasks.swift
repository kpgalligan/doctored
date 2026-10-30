import AppKit

// `set_layer_clipped` / Layer > Create Clipping Mask are typed-capable: the facade's
// `withLayerClipped`/`layerClipped` branch to the core's `layer_clipped` op and the parsed
// `clipped` metadata, so the byte handlers need no depth check.

extension RasterDocument {
    // MARK: - Clipping masks

    /// Sets or clears layer `idx`'s clipped flag: a clipped layer is
    /// confined to the alpha footprint of the first UNCLIPPED layer beneath
    /// it. Grouping is positional and re-derived at every composite, so
    /// reordering or deleting layers needs no bookkeeping here; a clipped
    /// BOTTOM layer has no base and composites as if unclipped.
    func withLayerClipped(_ idx: Int, clipped: Bool) throws -> RasterDocument? {
        if isHighDepth { return try performingPrecisionPixelEdit(.init(["op": "layer_clipped", "layer": idx, "clipped": clipped])) }
        guard isValidIndex(idx) else { return nil }
        return wrap(dr_doc_with_layer_clipped(ptr, idx, clipped))
    }

    func layerClipped(_ idx: Int) -> Bool {
        if let precisionMetadata { return precisionMetadata.layer(idx)?.clipped ?? false }
        guard isValidIndex(idx) else { return false }
        return dr_doc_layer_clipped(ptr, idx)
    }
}
