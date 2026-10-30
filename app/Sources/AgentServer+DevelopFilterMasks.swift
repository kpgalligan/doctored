import Foundation

// `apply_filter develop_filter` with `masks`: mirrors Filters ▸ Develop Filter…'s Masks
// section (`DevelopFilterMaskPanel.swift`). `applyFilter` runs this before
// `refuseUnknownFilterArguments`, so both the 8-bit `byteFilterRequest` reduce and the 16-bit
// reduce carry the resolved masks and the document's `range_profile`
// (`flatFilterArguments["develop_filter"]`). The core refuses everything else by name
// (`develop_filter_masks.rs`).
extension AgentServer {
    /// `a` with each generated mask shape ({op: subject, people or sky}) made into the core's map
    /// shape and, when a mask has a range, the document's `range_profile`. Refuses a client
    /// `map` shape and a client `range_profile`, masks on a mask, plane or channel target, at
    /// 32 bits and in CMYK, a missing model and a model that found nothing. Any filter but
    /// `develop_filter` returns `a` unchanged; with no masks it only refuses a client `range_profile`.
    func developFilterMaskArguments(_ a: [String: Any], _ document: ImageDocument) throws -> [String: Any] {
        guard a["filter"] as? String == "develop_filter" else { return a }
        if a["range_profile"] != nil {
            throw ToolError(message: "range_profile is made by the app from the document's profile; omit it")
        }
        guard let masks = a["masks"] as? [Any], !masks.isEmpty else { return a }
        let clientMap = masks.contains { mask in
            ((mask as? [String: Any])?["shape"] as? [String: Any])?["op"] as? String == "map"
        }
        if clientMap {
            throw ToolError(message: "map shapes are made by the app; pass op subject, people or sky")
        }
        guard try paintTarget(a, document) == .layer else {
            throw ToolError(message: DevelopFilterMasks.scalarRefusal)
        }
        let layer = try structuralLayerIndex(a, document)
        let resolved: [String: Any]
        do {
            resolved = try DevelopFilterMasks.resolved(["masks": masks], document: document, layer: layer)
        } catch let error as JobCancelled {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        var result = a
        result["masks"] = resolved["masks"]
        if let profile = resolved["range_profile"] { result["range_profile"] = profile }
        return result
    }
}
