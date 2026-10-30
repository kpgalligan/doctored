import AppKit

/// The two READ-ONLY tools behind the Info panel: the histogram it plots and
/// the readout that follows the cursor. Neither registers an undo step nor
/// dirties the document — they only look.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let infoHandlers: [String: ToolHandler] = [
        // Statistics and the pixel readout (AgentServer+Info.swift):
        // mirrors the Info panel's histogram and its cursor readout.
        "histogram": { $0.histogram },
        "sample_pixel": { $0.samplePixel },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let infoToolFacts: [String: ToolFacts] = [
        // #547 phase 5: a CMYK document's histogram per ink (`AgentServer+CmykColour.swift`).
        "histogram": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "sample_pixel": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
    ]

    /// histogram: the agent mirror of the Info panel's histogram (also the
    /// plot behind Levels and Curves). Always at stride 1 — only the app's
    /// live panel samples, because only it re-scans on every brush tick.
    func histogram(_ a: [String: Any]) throws -> String {
        try scopedHistogram(a, document: target(a))
    }

    /// sample_pixel: the agent mirror of the Info panel's readout —
    /// `sample_color` is the paint-oriented twin, and the two agree about
    /// the pixel's own numbers.
    func samplePixel(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        if a["source"] != nil || a["selection"] != nil || a["below"] != nil || a["layer"] != nil {
            return try scopedSample(a, document: document)
        }

        guard let x = intArg(a, "x"), let y = intArg(a, "y") else {
            throw ToolError(message: "sample_pixel requires x and y")
        }
        let size = intArg(a, "sample_size") ?? 1
        guard let reach = [1: 0, 3: 1, 5: 2][size] else {
            throw ToolError(message: "sample_size must be 1, 3 or 5 (got \(size))")
        }
        guard let readout = try PixelReadout.at((x: x, y: y), in: document, reach: reach) else {
            throw ToolError(
                message: "No part of the \(size)×\(size) sample at (\(x), \(y)) is inside "
                    + "the canvas (\(doc.width)×\(doc.height), origin top-left)")
        }
        var result = bytePixelFields(readout, size: size, doc: doc)
        // #547 phase 5: the Info panel's CMYK row through the working CMYK, "!" when out of gamut.
        if let cmyk = cmykReadout(rgb: [Int(readout.r), Int(readout.g), Int(readout.b)], document: doc) {
            result["cmyk"] = cmyk
        }
        result["ok"] = true
        return try jsonResult(result)
    }

    /// `sample_pixel`'s 8-bit reply without `ok` — `get_color_samplers` reports
    /// each sampler's reading in the same fields.
    func bytePixelFields(_ readout: PixelReadout, size: Int, doc: RasterDocument) -> [String: Any] {
        let hsb = readout.hsbRounded
        var result: [String: Any] = [
            "x": readout.x, "y": readout.y, "sample_size": size,
            "r": Int(readout.r), "g": Int(readout.g), "b": Int(readout.b),
            "a": Int(readout.a),
            "hex": readout.hex,
            "hsb": ["h": hsb.h, "s": hsb.s, "b": hsb.b],
            "space": doc.profileName,
            "lab_space": readout.labSpace,
            "paint_hex": readout.paintHex,
            "paint_hex_exact": readout.paintExact,
        ]
        var notes: [String] = []
        if let lab = readout.lab {
            result["lab"] = [
                "l": (lab.l * 1000).rounded() / 1000,
                "a": (lab.a * 1000).rounded() / 1000,
                "b": (lab.b * 1000).rounded() / 1000,
            ]
        } else {
            result["lab"] = NSNull()
            notes.append(
                "Lab is null because this build cannot model \(readout.labSpace) (a LUT "
                    + "profile); reporting an sRGB reading instead would be a made-up number.")
        }
        if !readout.paintExact {
            notes.append(
                "This pixel is outside the sRGB gamut, so no sRGB hex names it: painting "
                    + "paint_hex back gives the closest sRGB colour, which is duller than "
                    + "what was sampled.")
        }
        if !notes.isEmpty { result["note"] = notes.joined(separator: " ") }
        return result
    }
}
