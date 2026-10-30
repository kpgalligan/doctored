import AppKit

// MCP `detect_horizon` / `fit_crop` are read-only and `nonRecordable`, answering boxes that pass
// into `crop`.

/// The two straighten helpers for the agent, both read-only:
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `detectHorizon` | the Crop tool's Auto button (Vision's horizon → the Straighten angle) |
/// | `fitCrop` | the Crop tool's box fit inside the rotated image, set by the Straighten field or by the Straighten line |
///
/// Neither edits: each answers an angle and a box whose keys pass straight
/// into `crop`, which stays the one straighten-and-crop edit. No undo step,
/// no dirty flag, no Action step (both are `nonRecordable`).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let horizonHandlers: [String: ToolHandler] = [
        "detect_horizon": { $0.detectHorizon },
        "fit_crop": { $0.fitCrop },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let horizonToolFacts: [String: ToolFacts] = [
        // Both read the analysis composite and edit nothing.
        // The Crop tool (hence Auto) runs at float; both tools only read.
        "detect_horizon": [.nonRecordable, .commonTool, .floatTool],
        "fit_crop": [.nonRecordable, .commonTool, .floatTool],
    ]

    /// detect_horizon: the Auto button. "No horizon" is an answer, not an error.
    func detectHorizon(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let box = try straightenBox(a, doc: doc)
        guard let detected = try HorizonDetection.detect(document, title: "Detect Horizon") else {
            return try jsonResult([
                "ok": true, "found": false,
                "note": "No horizon was found. Vision looks for a clear horizon tilted by about "
                    + "1° to 10°; the image may already be level. fit_crop takes an angle or a "
                    + "line instead.",
            ])
        }
        // Vision's float noise (5.12500000032) is dropped, so the answer and
        // the box fitted at it agree with the angle `crop` will be handed.
        let angle = (detected * 1000).rounded() / 1000
        let canvas = CGSize(width: doc.width, height: doc.height)
        var crop = boxFields(CropFit.fitted(box, angle: angle, canvas: canvas))
        crop["angle"] = angle
        return try jsonResult(["ok": true, "found": true, "angle": angle, "suggested_crop": crop])
    }

    /// fit_crop: the box fit the Crop tool applies at an angle, or at the
    /// angle a Straighten line levels (the line's rule on the unrotated image).
    func fitCrop(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let box = try straightenBox(a, doc: doc)
        guard (a["angle"] != nil) != (a["line"] != nil) else {
            throw ToolError(message: "fit_crop takes exactly one of angle or line.")
        }
        let angle: Double
        if a["angle"] != nil {
            angle = DocumentPrecision.cropAngle(a)
            guard angle.isFinite, abs(angle) <= 45 else {
                throw ToolError(message: "angle must be between -45 and 45 degrees")
            }
        } else {
            let raw = a["line"] as? [Any] ?? []
            let values = raw.compactMap {
                ($0 as? NSNumber)?.doubleValue ?? ($0 as? String).flatMap(Double.init)
            }
            guard raw.count == 4, values.count == 4, values.allSatisfy(\.isFinite) else {
                throw ToolError(
                    message: "line must be four numbers [x1, y1, x2, y2] in canvas coordinates.")
            }
            guard let levelled = CropFit.lineAngle(
                from: CGPoint(x: values[0], y: values[1]),
                to: CGPoint(x: values[2], y: values[3]))
            else {
                throw ToolError(message: "line's two endpoints are the same point; give a line "
                    + "along something that should be level or vertical.")
            }
            angle = levelled
        }
        let canvas = CGSize(width: doc.width, height: doc.height)
        var reply = boxFields(CropFit.fitted(box, angle: angle, canvas: canvas))
        reply["ok"] = true
        reply["angle"] = angle
        return try jsonResult(reply)
    }

    /// The optional `x, y, width, height` box: all four or none (the whole
    /// canvas), whole numbers, inside the canvas, at least 1×1.
    private func straightenBox(_ a: [String: Any], doc: RasterDocument) throws -> CGRect {
        let keys = ["x", "y", "width", "height"]
        let named = keys.filter { a[$0] != nil }
        guard !named.isEmpty else {
            return CGRect(x: 0, y: 0, width: doc.width, height: doc.height)
        }
        guard named.count == keys.count else {
            throw ToolError(
                message: "Give all four of x, y, width and height, or none for the whole canvas.")
        }
        let values = keys.compactMap { key -> Int? in
            guard let value = doubleArg(a, key), value.isFinite, value == value.rounded(),
                  abs(value) <= 1_000_000 else { return nil }
            return Int(value)
        }
        guard values.count == keys.count else {
            throw ToolError(message: "x, y, width and height must be whole numbers.")
        }
        let (x, y, width, height) = (values[0], values[1], values[2], values[3])
        guard width >= 1, height >= 1, x >= 0, y >= 0,
              x + width <= doc.width, y + height <= doc.height else {
            throw ToolError(
                message: "The box must lie inside the \(doc.width) × \(doc.height) canvas "
                    + "(origin top-left) with width and height of at least 1.")
        }
        return CGRect(x: x, y: y, width: width, height: height)
    }

    private func boxFields(_ rect: CGRect) -> [String: Any] {
        ["x": Int(rect.minX), "y": Int(rect.minY),
         "width": Int(rect.width), "height": Int(rect.height)]
    }

    // MARK: - Catalog

    @inline(never) func horizonCatalog() -> [[String: Any]] {
        let integer: [String: Any] = ["type": "integer"]
        let box = "The optional box x, y, width, height (integers, canvas coordinates, origin "
            + "top-left; all four or none) is the crop box to fit and defaults to the whole "
            + "canvas; it must lie inside the canvas, at least 1×1. The fit is the Crop tool's: "
            + "the box keeps its aspect and, where it can, its centre and shrinks, to whole "
            + "pixels, until every corner of the straightened result lands on image pixels "
            + "(a 2-pixel margin from "
            + "the canvas edge), so the crop shows no transparent wedges. It fits against the "
            + "canvas and ignores layer pixels lying outside it."
        return [
            tool("detect_horizon",
                 "Find the horizon's tilt with Vision — the Crop tool's Auto button. Reads the "
                 + "flattened composite (a 32-bit float document's tone-mapped display) and "
                 + "edits nothing. Found: {ok, found: true, angle, suggested_crop: {x, y, width, "
                 + "height, angle}} — angle is the Straighten angle in degrees in crop's sign "
                 + "convention, and suggested_crop's keys pass straight into crop. None: {ok, "
                 + "found: false, note} (not an error). Vision answers only for a clear, "
                 + "textured horizon tilted by about 1° to 10°, in steps of about 0.125°; for "
                 + "anything else use fit_crop with an angle or a line. " + box,
                 ["document_id": Self.docIDProperty,
                  "x": integer, "y": integer, "width": integer, "height": integer]),
            tool("fit_crop",
                 "The crop box for a straighten — the Crop tool's box fit, set by its "
                 + "Straighten field or its Straighten line. Give exactly one of angle (degrees, "
                 + "-45 to 45, crop's sign: positive turns the picture counter-clockwise on "
                 + "screen) or line [x1, y1, x2, y2] (canvas coordinates of two points on "
                 + "something that should be level or vertical in the UNROTATED image; the "
                 + "angle levels it, and a line steeper than 45° is made vertical). Edits "
                 + "nothing; the reply {ok, angle, x, y, width, height} passes straight into "
                 + "crop. " + box,
                 ["document_id": Self.docIDProperty,
                  "angle": ["type": "number", "minimum": -45, "maximum": 45],
                  "line": [
                    "type": "array", "minItems": 4, "maxItems": 4,
                    "items": ["type": "number"],
                    "description": "Two canvas points [x1, y1, x2, y2]; they must differ.",
                  ] as [String: Any],
                  "x": integer, "y": integer, "width": integer, "height": integer]),
        ]
    }
}
