import AppKit

// The CMYK document's paint and tone arguments over MCP (#1 phase 2): `ink` (`brush_stroke`,
// `fill`, `fill_selection`, `stroke_selection`, `fill_path`, `stroke_path`) and `start_ink` /
// `end_ink` (`gradient`), each C, M, Y, K in percent —
// the Color Picker's C/M/Y/K fields (`ColorPickerSheetController`) — and the tone keys each colour
// mode owns. Helpers only: the tools are the shared ones, whose handlers branch on a CMYK document
// (`AgentServer+PrecisionPaint.swift`, `PrecisionColorTools.swift`, `AgentServer+PrecisionFillStroke.swift`);
// the gate that narrows their arguments is `CmykDocument.refusal(tool:arguments:)`.
//
// The artwork batch (#1) adds the type, shape and fill layer tools' ink arguments — `ink`
// (`add/edit_text_layer`, `add/edit_fill_layer`), `color_runs[].ink`, `stops[].ink`, `fill_ink`
// and `stroke_ink` (`add/edit_shape_layer`) — which `cmykArtworkArguments` turns into the colour
// arguments the handlers already read (each ink's sRGB appearance) plus the matches that keep the
// ink exact (`CmykArtwork.swift`); the replies report each ink in percent (`inkPercent`).

extension AgentServer {
    /// The ink arguments, and the tools that take them.
    static let cmykInkKeys = ["ink", "start_ink", "end_ink", "fill_ink", "stroke_ink"]
    static let cmykInkTools: Set<String> = ["brush_stroke", "fill", "fill_selection", "gradient",
                                            "stroke_selection", "fill_path", "stroke_path",
                                            // The artwork batch: type, shape and fill layers.
                                            "add_text_layer", "edit_text_layer", "add_shape_layer",
                                            "edit_shape_layer", "add_fill_layer", "edit_fill_layer"]
    /// The artwork tools' nested rows that may carry an `ink`.
    static let cmykInkRows = ["color_runs", "stops"]

    /// The Levels and Curves keys a CMYK document reads (`core/src/cmyk_adjust.rs`) and the RGB
    /// ones it refuses; an RGB document refuses the CMYK ones.
    static let cmykToneKeys = ["c", "m", "y", "k", "cmyk", "cmyk_map", "c_map", "m_map", "y_map", "k_map"]
    static let rgbToneKeys = ["r", "g", "b", "rgb", "rgb_map", "r_map", "g_map", "b_map"]

    /// `key` (`ink`, `start_ink`, `end_ink`) as an ink: four numbers (or numeric strings), C, M,
    /// Y, K in percent 0…100. nil when the argument is absent.
    func cmykInkArgument(_ a: [String: Any], _ key: String) throws -> CmykInk? {
        guard let raw = a[key] else { return nil }
        let values = (raw as? [Any])?.compactMap { value -> Double? in
            if let number = NativeBrushStroke.jsonNumber(value) { return number }
            return (value as? String).flatMap(Double.init)
        }
        guard let values, values.count == 4, let ink = CmykInk(percent: values) else {
            throw ToolError(message: "\(key) must be four numbers, C, M, Y and K in percent 0…100.")
        }
        return ink
    }

    /// The straight CMYKA (0…1) a CMYK paint call takes: `key`'s ink when passed (its alpha the
    /// `colorKey` colour's when that is passed too, else 1), otherwise the `colorKey` colour
    /// (`fallback` when absent) converted through the document's profile (`CmykPaint.ink`).
    func cmykPaintInk(_ a: [String: Any], _ key: String, colorKey: String, fallback: NSColor,
                      precision: RasterPrecisionDocument) throws -> [Double] {
        let color = try parseColor(a, colorKey, fallback: fallback)
        do {
            if let ink = try cmykInkArgument(a, key) {
                let alpha = a[colorKey] == nil ? 1 : Double(color.usingColorSpace(.sRGB)?.alphaComponent ?? 1)
                return ink.unit + [min(max(alpha, 0), 1)]
            }
            return try CmykPaint.ink(color, stored: nil, in: precision)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Ink unit numbers (0…1, the first four) as the percents the replies report.
    static func inkPercent(_ numbers: [Double]) -> [Double] {
        numbers.prefix(4).map { ($0 * 100 * 1e6).rounded() / 1e6 }
    }

    /// `a` with each artwork ink argument turned into the colour argument its handler reads, and
    /// the matches that keep each ink exact (module comment). `pairs` name each top-level ink key
    /// with its numeric colour key and its legacy hex key; a `color_runs` row's `ink` becomes its
    /// `rgba`, a `stops` row's its `color` hex. An ink's alpha is its colour argument's when one is
    /// passed too, else 1. `a` as it is, with no matches, in an RGB document.
    func cmykArtworkArguments(_ a: [String: Any], doc: RasterDocument,
                              pairs: [(ink: String, numeric: String, legacy: String)])
        throws -> (arguments: [String: Any], matches: [CmykInkMatch]) {
        guard doc.isCmyk else { return (a, []) }
        var out = a
        var matches: [CmykInkMatch] = []
        let end = doc.artworkColorEnd
        func appearance(_ ink: CmykInk) throws -> [Double] {
            do {
                let rgb = try RasterCmykArtworkCore.colors([ink.unit], from: end, to: .srgb,
                                                           options: RasterDocument.artworkColorOptions)
                return rgb.first ?? [0, 0, 0]
            } catch { throw ToolError(message: error.localizedDescription) }
        }
        func alpha(_ row: [String: Any], numeric: String, legacy: String) throws -> Double {
            guard row[numeric] != nil || row[legacy] != nil,
                  let color = try artworkColor(row, numeric: numeric, legacy: legacy, fallback: nil, ink: .srgb)
            else { return 1 }
            return ArtworkColor.alpha(ArtworkColor.authored(color, in: .srgb))
        }
        for pair in pairs {
            guard let ink = try cmykInkArgument(a, pair.ink) else { continue }
            let rgb = try appearance(ink)
            out[pair.numeric] = rgb + [try alpha(a, numeric: pair.numeric, legacy: pair.legacy)]
            out[pair.legacy] = nil
            out[pair.ink] = nil
            matches.append(CmykInkMatch(srgb: rgb, ink: ink.unit))
        }
        if let rows = a["color_runs"] as? [[String: Any]] {
            out["color_runs"] = try rows.map { row -> [String: Any] in
                guard let ink = try cmykInkArgument(row, "ink") else { return row }
                var next = row
                let rgb = try appearance(ink)
                next["rgba"] = rgb + [try alpha(row, numeric: "rgba", legacy: "color")]
                next["color"] = nil
                next["ink"] = nil
                matches.append(CmykInkMatch(srgb: rgb, ink: ink.unit))
                return next
            }
        }
        if let rows = a["stops"] as? [[String: Any]] {
            out["stops"] = try rows.map { row -> [String: Any] in
                guard let ink = try cmykInkArgument(row, "ink") else { return row }
                var next = row
                let hex = CmykArtworkHex.six(try appearance(ink))
                next["color"] = hex
                next["ink"] = nil
                matches.append(CmykInkMatch(srgb: CmykArtworkHex.unit(hex), ink: ink.unit))
                return next
            }
        }
        return (out, matches)
    }
}
