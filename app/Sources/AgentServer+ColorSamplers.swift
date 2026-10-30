import AppKit

// MCP `get_color_samplers` / `set_color_samplers` (replace the list; `nonRecordable`) report
// `sample_pixel`'s own fields per sampler (`bytePixelFields` at 8 bits, `scopedSampleReport` at
// 16/32); View ▸ Show Color Samplers and the readout mode are view preferences with no tool.

/// The two Color Sampler tools:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `getColorSamplers` | the Info panel's Color Samplers section |
/// | `setColorSamplers` | the Color Sampler tool's click / drag / ⌥-click, the Eyedropper's ⇧-click, View ▸ Clear Color Samplers and the options bar's Clear All |
///
/// The list is saved DOCUMENT state: `.rz` and PSD (image resource
/// 1073) carry it. `set_color_samplers` commits through
/// `ImageDocument.commitColorSamplers` — the UI's own path — so it dirties
/// the document but adds no undo step and no Action step (both tools are
/// `nonRecordable`), and it is refused by name during a gesture, an edit or
/// a job on the document. A canvas-geometry edit moves the samplers with the
/// content and drops one that lands off the canvas; ordinary undo and redo
/// keep the current list, and undoing a geometry edit brings back the list
/// from before it. Each reading is `sample_pixel`'s own at the same point and size: the
/// byte fields at 8 bits, the scoped composite report at 16 and 32. It
/// reports COMMITTED values only — a sheet's preview never reaches MCP.
/// Show Color Samplers and the readout mode are view preferences and have
/// no tool (the ruler/grid exemption).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let colorSamplersHandlers: [String: ToolHandler] = [
        "get_color_samplers": { $0.getColorSamplers },
        "set_color_samplers": { $0.setColorSamplers },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let colorSamplersToolFacts: [String: ToolFacts] = [
        // The Color Samplers are view state — no undo step, no dirty
        // flag, nothing a `save_copy` carries.
        // The Color Samplers read the composite at every depth through
        // `sample_pixel`'s own per-depth path, and writing the list is view
        // state that touches no pixel storage.
        // The Color Samplers read linear float samples like sample_pixel.
        "get_color_samplers": [.nonRecordable, .commonTool, .floatTool],
        "set_color_samplers": [.nonRecordable, .commonTool, .floatTool],
    ]

    /// get_color_samplers: the Info panel's Color Samplers section.
    func getColorSamplers(_ a: [String: Any]) throws -> String {
        try jsonResult(colorSamplerReply(try target(a)))
    }

    /// set_color_samplers: REPLACES the list — add, move, delete and clear
    /// are one call; `[]` clears it. Every refusal leaves the list unchanged.
    /// Mirrors the Color Sampler tool's click, drag release and ⌥-click, the
    /// Eyedropper's ⇧-click, View ▸ Clear Color Samplers and the options
    /// bar's Clear All, which all commit through `commitColorSamplers`.
    func setColorSamplers(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let rows = a["samplers"] as? [Any] else {
            throw ToolError(
                message: "set_color_samplers requires samplers: an array of {x, y, sample_size} "
                    + "objects ([] clears the list).")
        }
        guard rows.count <= ColorSamplerSet.limit else {
            throw ToolError(
                message: "A document holds at most \(ColorSamplerSet.limit) color samplers "
                    + "(got \(rows.count)).")
        }
        var items: [ColorSampler] = []
        for (index, row) in rows.enumerated() {
            guard let object = row as? [String: Any],
                  let x = intArg(object, "x"), let y = intArg(object, "y") else {
                throw ToolError(message: "Sampler \(index) needs integer x and y.")
            }
            let size = intArg(object, "sample_size") ?? 1
            guard object["sample_size"] == nil || intArg(object, "sample_size") != nil,
                  [1, 3, 5].contains(size) else {
                throw ToolError(message: "Sampler \(index): sample_size must be 1, 3 or 5.")
            }
            guard (0..<doc.width).contains(x), (0..<doc.height).contains(y) else {
                throw ToolError(
                    message: "Sampler \(index) at (\(x), \(y)) is outside the canvas "
                        + "(\(doc.width)×\(doc.height), origin top-left).")
            }
            items.append(ColorSampler(x: x, y: y, sampleSize: size))
        }
        let changed: Bool
        do {
            changed = try document.commitColorSamplers(ColorSamplerSet(items: items))
        } catch let refusal as SilentEditRefusal {
            throw ToolError(message: refusal.message)
        }
        var reply = try colorSamplerReply(document)
        reply["changed"] = changed
        return try jsonResult(reply)
    }

    /// One row per sampler: `index` (0-based), `label` ("#n", the panel's),
    /// x, y, sample_size and `sample_pixel`'s reading.
    private func colorSamplerReply(_ document: ImageDocument) throws -> [String: Any] {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        var rows: [[String: Any]] = []
        for (index, sampler) in document.colorSamplers.items.enumerated() {
            var row: [String: Any]
            if doc.isHighDepth {
                row = try scopedSampleReport(
                    ["x": sampler.x, "y": sampler.y, "sample_size": sampler.sampleSize],
                    document: document)
                row.removeValue(forKey: "ok")
                // #1 Lab phase 2d: `sample_pixel`'s own Lab fields (`LabDocument.withReading`).
                if doc.isLab { row = LabDocument.withReading(row) }
                // #1: an 8/16-bit Grayscale document's K %, sample_color's rule.
                if doc.isGray, !doc.isFloat,
                   let normalized = row["normalized_rgba"] as? [Double], normalized.count == 4 {
                    row["gray_k_percent"] = GrayDocument.kPercent(normalized[0])
                }
            } else {
                guard let readout = try PixelReadout.at(
                    (x: sampler.x, y: sampler.y), in: document, reach: sampler.sampleSize / 2)
                else {
                    throw ToolError(message: "Sampler \(index) is outside the canvas.")
                }
                row = bytePixelFields(readout, size: sampler.sampleSize, doc: doc)
            }
            row["index"] = index
            row["label"] = "#\(index + 1)"
            row["x"] = sampler.x
            row["y"] = sampler.y
            row["sample_size"] = sampler.sampleSize
            rows.append(row)
        }
        return [
            "ok": true, "limit": ColorSamplerSet.limit, "count": rows.count, "samplers": rows,
        ]
    }

    // MARK: - Catalog

    @inline(never) func colorSamplerCatalog() -> [[String: Any]] {
        let reading = "Each sampler reports index (0-based), label (\"#n\", as the Info panel "
            + "numbers it), x, y, sample_size and sample_pixel's own reading at that point and "
            + "size — the 8-bit byte fields, or the 16-bit/32-bit float composite report. "
            + "Committed values only."
        return [
            tool("get_color_samplers",
                 "The document's Color Samplers (Info panel ▸ Color Samplers): up to "
                 + "\(ColorSamplerSet.limit) pinned points read live from the composite. "
                 + reading + " A read.",
                 ["document_id": Self.docIDProperty]),
            tool("set_color_samplers",
                 "Replace the document's Color Samplers — the Color Sampler tool's add, move "
                 + "and delete, and View > Clear Color Samplers: pass the whole new list, [] "
                 + "to clear. At most \(ColorSamplerSet.limit); each x/y must lie on the "
                 + "canvas and sample_size must be 1, 3 or 5 (default 1). A refusal leaves the "
                 + "list unchanged; it is refused by name while a gesture, an edit or a job is "
                 + "running on the document. The list is saved with the document (.doctored, and "
                 + "PSD image resource 1073, which stores no sample size): setting it dirties "
                 + "the document but adds no undo step and is not recorded as an Action step. "
                 + "A crop, canvas size, flip, rotate, straighten or resize moves the samplers "
                 + "with the content and drops any that land off the canvas; ordinary undo "
                 + "and redo keep the current list, and undoing a geometry edit restores the "
                 + "list from before it. The reply is get_color_samplers' plus changed. "
                 + reading,
                 ["document_id": Self.docIDProperty,
                  "samplers": [
                    "type": "array",
                    "description": "The new list, in order (#1 first).",
                    "items": [
                        "type": "object",
                        "properties": [
                            "x": ["type": "integer"], "y": ["type": "integer"],
                            "sample_size": ["type": "integer", "enum": [1, 3, 5],
                                            "description": "Side of the averaged square; default 1."],
                        ] as [String: Any],
                        "required": ["x", "y"],
                    ] as [String: Any],
                  ] as [String: Any]],
                 required: ["samplers"]),
        ]
    }
}
