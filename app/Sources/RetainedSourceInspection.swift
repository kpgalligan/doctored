import Foundation

/// Compact stack inspection shared by byte and typed owners. The caller keeps
/// its per-row response lease through JSON serialization; dense recipes and
/// native mask pixels are available only through their dedicated inspection.
enum RetainedSourceInspection {
    static func fields(_ document: RasterDocument, layer: Int) -> [String: Any] {
        guard document.hasRetainedSource(layer) else { return [:] }
        var fields: [String: Any] = ["has_image_source": true]
        if document.hasRetainedRawSource(layer) { fields["has_raw_source"] = true }
        do {
            let payload = try PrecisionEditPayload.admitted(bytes: 1024 * 1024)
            defer { withExtendedLifetime(payload) {} }
            guard var source = try document.retainedSourceInfo(layer) else {
                throw DoctoredCoreError(message: "The retained source summary is unavailable.")
            }
            if var raw = source["raw"] as? [String: Any] {
                raw.removeValue(forKey: "recipe")
                source["raw"] = raw
            }
            fields["image_source"] = source
        } catch {
            fields["source_inspection_error"] = String(describing: error)
        }
        return fields
    }
}
