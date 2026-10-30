import AppKit

extension AgentServer {
    @discardableResult
    func performArtworkEdit(_ document: ImageDocument, _ title: String,
                            _ compute: (RasterDocument) throws -> RasterDocument?) throws -> Bool {
        var failure: Error?
        var unchanged = false
        do {
            try performGroupedEdit(document, title) { source in
                do {
                    let output = try compute(source)
                    unchanged = output == nil
                    return output
                } catch { failure = error; return nil }
            }
        } catch {
            if let failure { throw failure }
            if unchanged { return false }
            throw error
        }
        return true
    }

    /// Mirrors the Text tool's selected-run colors and Artwork Colors sheet.
    /// Numbers and hex digits are `ink`'s — the document's own, linear light
    /// at 32 bits.
    func artworkColor(_ a: [String: Any], numeric: String, legacy: String,
                      fallback: NSColor?, ink: ArtworkInk) throws -> NSColor? {
        guard let raw = a[numeric] else {
            if a[legacy] != nil {
                guard let text = stringArg(a, legacy) else { throw ToolError(message: "\(legacy) must be a hex color or empty string.") }
                if text.isEmpty { return nil }
                guard let color = TextLayer.color(fromHex: text) else { throw ToolError(message: "Invalid \(legacy) color.") }
                guard ink != .srgb else { return color }
                // The hex digits are the numbers ×255 in `ink`.
                guard let inked = ArtworkColor.color(ArtworkColor.authored(color), in: ink) else {
                    throw ToolError(message: "Invalid \(legacy) color.")
                }
                return inked
            }
            return fallback
        }
        guard a[legacy] == nil else { throw ToolError(message: "Pass \(numeric) or \(legacy), not both.") }
        if raw is NSNull { return nil }
        guard let values = raw as? [Any], values.count == 4,
              values.allSatisfy({ !Self.isJSONBoolean($0) }),
              let color = ArtworkColor.color(values.compactMap { ($0 as? NSNumber)?.doubleValue }, in: ink) else {
            throw ToolError(message: "\(numeric) requires four finite components: normalized at 8/16 bits; at 32 bits linear light with red, green and blue 0–65504 and alpha 0–1.")
        }
        return color
    }

    func artworkTextStyle(_ a: [String: Any], payload: TextLayerPayload,
                          current: ArtworkTextStyle? = nil) throws -> ArtworkTextStyle {
        let ink = payload.ink
        let rgba = ArtworkColor.authored(payload.nsColor, in: ink)
        var result = ArtworkTextStyle(rgba: rgba, runs: current?.runs ?? [], in: ink)
        if let raw = a["color_runs"] {
            guard let rows = raw as? [[String: Any]], rows.count <= 64 else {
                throw ToolError(message: "color_runs requires at most 64 sorted, nonoverlapping UTF-16 ranges with rgba.")
            }
            result.runs = try rows.map { row in
                guard Set(row.keys) == Set(["location", "length", "rgba"]),
                      let location = row["location"] as? NSNumber, let length = row["length"] as? NSNumber,
                      !Self.isJSONBoolean(location), !Self.isJSONBoolean(length),
                      location.doubleValue == Double(location.intValue), length.doubleValue == Double(length.intValue),
                      let color = try artworkColor(row, numeric: "rgba", legacy: "color", fallback: nil, ink: ink),
                      let rgba = ArtworkColor.rgba(color, in: ink) else { throw ToolError(message: "Each color run needs integer location/length and normalized rgba.") }
                return ArtworkColorRun(location: location.intValue, length: length.intValue, rgba: rgba)
            }
        }
        guard result.isValid(for: payload.string) else {
            throw ToolError(message: "Color runs must be sorted, nonoverlapping complete character ranges within the text. Supply color_runs:[] to remove existing runs when replacing text.")
        }
        return result
    }

    static func artworkRunFields(_ runs: [ArtworkColorRun]) -> [[String: Any]] {
        runs.map { ["location": $0.location, "length": $0.length, "rgba": $0.rgba] }
    }
}
