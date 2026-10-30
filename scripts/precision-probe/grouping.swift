import Foundation

extension RasterPrecisionDocument {
    /// Every root wrapped in one group at `opacity` — the probes' grouping
    /// fixture — through the app's own typed `group_layers` edit and opacity
    /// edit. `passThrough` false makes an isolated Normal group.
    func groupingRoots(opacity: Float, passThrough: Bool = false) throws -> RasterPrecisionDocument? {
        let roots = try RasterDocument(precision: self).layerTree.topLevel()
        let request: [String: Any] = [
            "op": "group_layers", "layers": roots, "name": "Group", "pass_through": passThrough,
        ]
        let json = String(decoding: try JSONSerialization.data(withJSONObject: request), as: UTF8.self)
        let context = try PrecisionWorkContext(source: self)
        guard let grouped = try editingPixels(
            request: json, samples: [], workingLimit: context.limit,
            otherRetained: context.otherRetained)
        else { return nil }
        // The new group is the top root, so it is the last record.
        let top = try RasterDocument(precision: grouped).layerCount - 1
        // An unchanged opacity comes back nil.
        return try grouped.settingLayerOpacity(top, opacity: opacity) ?? grouped
    }
}
