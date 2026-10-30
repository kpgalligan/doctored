import AppKit

/// The 32-bit float document's tools (`FloatDocument.swift`):
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `getToneMap` | the Image ▸ Tone Map… card as it opens |
/// | `setToneMap` | Image ▸ Tone Map… ▸ Apply |
///
/// `sample_pixel` / `sample_color` on a float document go straight through
/// `scopedSample` (`PrecisionReadout.swift`), mirroring the Info panel's
/// readout, which answers the float schema with every source.
/// `histogram` likewise goes straight through `scopedHistogram`, mirroring the
/// Info panel's stop-binned histogram, `source: "scalar"` included: a mask or
/// channel is u16 coverage and measures as at 16 bits, while the core refuses
/// red/green/blue/alpha by name.
///
/// `get_tone_map` reads and records nothing. `set_tone_map` IS a step: it is
/// a per-document, undoable, `.rz`-persisted property, which is the test
/// `AgentServer+Guides.swift`'s header states for the view-preference
/// exemption, and this passes all four parts of it.
///
/// THE HANDLER FORWARDS `white` VERBATIM. `clip` has no white point, and the
/// CORE is where that is refused (`float_document::ToneMap::from_map`), so a
/// caller who sends `{"operator":"clip","white":2}` is told the field is
/// unknown rather than having it quietly dropped here.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let floatDocumentHandlers: [String: ToolHandler] = [
        "get_tone_map": { $0.getToneMap },
        "set_tone_map": { $0.setToneMap },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let floatDocumentToolFacts: [String: ToolFacts] = [
        // Phase 11F: `set_tone_map` IS a step for the same four reasons —
        // per-document, undoable, `.rz`-persisted, and it changes what every
        // deliverable looks like — so only the read is dropped.
        "get_tone_map": [.nonRecordable, .floatTool],
        "set_tone_map": [.floatTool],
    ]

    /// The document's map, the maps it could take, the operator table and the
    /// refusal, if any.
    func getToneMap(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard doc.isFloat, let map = doc.toneMap else {
            throw ToolError(
                message: "A tone map applies only to a 32-bit float document. "
                    + FloatDocument.limitation)
        }
        var result: [String: Any] = [
            "ok": true,
            "tone_map": map.json,
            "available": ToneMapOperator.allCases.map(\.rawValue),
            "float_refusal": doc.floatRefusal() ?? NSNull(),
        ]
        if let table = FloatOperators.table { result["operators"] = table.reply }
        return try jsonResult(result)
    }

    /// Image ▸ Tone Map… ▸ Apply. One undo step; a map the document already
    /// has is reported unchanged rather than failing.
    func setToneMap(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard doc.isFloat, let previous = doc.toneMap else {
            throw ToolError(
                message: "A tone map applies only to a 32-bit float document. "
                    + FloatDocument.limitation)
        }
        let name = try requiredString(a, "operator")
        // Deliberately NOT validated against the enum here beyond the name:
        // the core owns the vocabulary and every field rule.
        var wire: [String: Any] = ["operator": name]
        if let white = doubleArg(a, "white") { wire["white"] = white }
        guard let data = try? JSONSerialization.data(withJSONObject: wire),
              let text = String(data: data, encoding: .utf8) else {
            throw ToolError(message: "Invalid tone map arguments.")
        }

        var refusal: Error?
        var changed = false
        do {
            try performGroupedEdit(document, "Tone Map") { source in
                do {
                    guard let next = try source.settingToneMapText(text) else { return nil }
                    changed = true
                    return next
                } catch {
                    refusal = error
                    return nil
                }
            }
        } catch {
            if let refusal { throw ToolError(message: refusal.localizedDescription) }
            if !changed {
                return try jsonResult([
                    "ok": true, "tone_map": previous.json, "previous": previous.json,
                    "unchanged": true,
                ])
            }
            throw error
        }
        if let refusal { throw ToolError(message: refusal.localizedDescription) }
        guard changed, let applied = document.doc?.toneMap else {
            return try jsonResult([
                "ok": true, "tone_map": previous.json, "previous": previous.json,
                "unchanged": true,
            ])
        }
        return try jsonResult([
            "ok": true, "tone_map": applied.json, "previous": previous.json,
        ])
    }

    /// `render`'s `exposure`: a view exposure in stops for THIS render
    /// of a 32-bit float canvas or region, as the display view it asks for.
    ///
    /// It mirrors View ▸ 32-bit Exposure… without touching it: the render is
    /// a model's only view of the picture, and a tone-mapped PNG hides what is
    /// above white, so the model needs the exposure the user's window has. It
    /// is an ARGUMENT OF A READ — nothing is stored, no window changes and the
    /// window's own exposure (`set_view`) is never read — so none of the four
    /// counts of the view-preference exemption applies. Refused by name with
    /// `layer` or `channel`, outside ±20 stops, and when non-zero on a
    /// document that is not 32-bit float.
    func renderExposure(_ a: [String: Any], doc: RasterDocument) throws -> FloatDisplayView {
        guard a["exposure"] != nil else { return .document }
        guard let stops = doubleArg(a, "exposure"), stops.isFinite else {
            throw ToolError(message: "exposure must be a number of stops, -20 to 20.")
        }
        guard ViewExposure.range.contains(stops) else {
            throw ToolError(
                message: "exposure must be between \(Int(ViewExposure.range.lowerBound)) and "
                    + "\(Int(ViewExposure.range.upperBound)) stops; got \(stops).")
        }
        guard a["layer"] == nil, a["channel"] == nil else {
            throw ToolError(
                message: "exposure applies to the flattened canvas or a region only, not with "
                    + "layer or channel.")
        }
        guard stops == 0 || doc.isFloat else {
            throw ToolError(
                message: "exposure applies only to a 32-bit float document. "
                    + "This document's display has no range above white to look at.")
        }
        return FloatDisplayView(toneMap: nil, exposureStops: stops)
    }

    /// What the render's caption says about its exposure; empty at 0 stops.
    static func renderExposureNote(_ view: FloatDisplayView) -> String {
        view.exposureStops == 0 ? "" : " at exposure \(ViewExposure.text(view.exposureStops)) EV"
    }

    /// The `float` block `get_document` carries on a float document, beside
    /// `evaluation`.
    static func floatFields(_ doc: RasterDocument) -> [String: Any]? {
        guard doc.isFloat, let map = doc.toneMap else { return nil }
        return [
            "storage": "rgba32_float",
            "range": "extended",
            "tone_map": map.json,
            "projection_error": doc.floatRefusal() ?? NSNull(),
        ]
    }
}

extension RasterDocument {
    /// The wire form straight through, so the core refuses every field this
    /// build does not admit rather than the host silently dropping one.
    func settingToneMapText(_ text: String) throws -> RasterDocument? {
        guard let precision else {
            throw DoctoredCoreError(
                message: "A tone map applies only to a 32-bit float document.")
        }
        guard let result = try precision.settingToneMap(text) else { return nil }
        return try RasterDocument(precision: result)
    }
}
