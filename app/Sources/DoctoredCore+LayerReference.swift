import AppKit

extension RasterDocument {
    /// Layer `idx`'s reference point (Photoshop's `fxrp`, canvas px): where a
    /// linked pattern fill layer's tile origin follows (FillLayer.swift's
    /// PATTERN). The byte getters at 8 bits, the typed inspection row's
    /// `"reference"` at 16 and 32; (0, 0) when the layer or the row is unknown.
    func layerReference(_ idx: Int) -> (x: Int, y: Int) {
        if precision != nil {
            guard let rows = precisionInfo?["layers"] as? [[String: Any]], rows.indices.contains(idx),
                  let pair = rows[idx]["reference"] as? [NSNumber], pair.count == 2
            else { return (0, 0) }
            return (pair[0].intValue, pair[1].intValue)
        }
        guard isValidIndex(idx) else { return (0, 0) }
        return (Int(dr_doc_layer_reference_x(ptr, idx)), Int(dr_doc_layer_reference_y(ptr, idx)))
    }
}

extension RasterDocument {
    /// A LINKED pattern fill layer's tile after a canvas turn or flip by `map`
    /// (FillLayer.swift's TURN): the core's `dr_turn_pattern` over `map`'s CG elements, which
    /// turns it through the one implementation a Pattern Overlay's tile takes
    /// (`reference_point::turned_tile`) and answers a custom tile under the pattern's name.
    /// nil = keep the tile: `map` is not exactly a quarter turn or a flip, or the pattern did
    /// not encode or parse.
    static func turnedPattern(_ pattern: PatternRef, by map: LinearMap) -> PatternRef? {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        guard let data = try? encoder.encode(pattern),
              let json = String(data: data, encoding: .utf8)
        else { return nil }
        var err: UnsafeMutablePointer<CChar>?
        let raw = dr_turn_pattern(json, map.a, map.b, map.c, map.d, &err)
        if let err { dr_string_free(err) }
        guard let raw else { return nil }
        defer { dr_string_free(raw) }
        return try? JSONDecoder().decode(PatternRef.self, from: Data(String(cString: raw).utf8))
    }
}
