import AppKit

extension RasterDocument {
    // MARK: - Layer comps

    /// The three `dr_*_layer_comps` calls, here because this file is the only
    /// home for `dr_*`. The model, the report types, the depth branch and the
    /// one shared writer live in `LayerComps.swift`.
    ///
    /// The REPORT form of every layer comp, at whichever depth this facade
    /// owns. Throws with the core's sentence.
    func layerCompsJSON() throws -> String {
        var err: UnsafeMutablePointer<CChar>? = nil
        let result = precision.map { dr_precision_doc_layer_comps_json($0.ptr, &err) }
            ?? dr_doc_layer_comps_json(ptr, &err)
        guard let result else {
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "Could not read the layer comps."))
        }
        defer { dr_string_free(result) }
        return String(cString: result)
    }

    /// One BYTE comp request. nil with no error is the core's exact no-op;
    /// a refusal throws with the core's own sentence, which the window and
    /// the MCP handler both show verbatim.
    func layerCompsEditHandle(_ request: String) throws -> RasterDocument? {
        var err: UnsafeMutablePointer<CChar>? = nil
        guard let handle = dr_doc_edit_layer_comps(ptr, request, &err) else {
            guard err != nil else { return nil }
            throw DoctoredCoreError(
                message: takeErrorMessage(err, fallback: "The layer comp could not be written."))
        }
        return RasterDocument(owning: handle)
    }
}
