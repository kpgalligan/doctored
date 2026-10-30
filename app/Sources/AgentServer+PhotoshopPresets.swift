import Foundation

// `read_photoshop_adjustment_preset` is the document-free, `nonRecordable` MCP twin; decoding is
// the core's (`RasterAdjustmentPresetFile`).

/// `read_photoshop_adjustment_preset`: the MCP twin of Layer ▸
/// New Adjustment Layer ▸ From Photoshop Preset…. It READS the preset and
/// answers the `{op, params}` the dialog would open prefilled with; a caller
/// then passes them to `add_adjustment_layer`, exactly as Apply records. It
/// resolves no document and changes nothing, so it answers at every depth
/// (`DocumentPrecision.documentFreeTools`) and is never an Action step.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let photoshopPresetsHandlers: [String: ToolHandler] = [
        "read_photoshop_adjustment_preset": { $0.readPhotoshopAdjustmentPreset },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let photoshopPresetsToolFacts: [String: ToolFacts] = [
        // Reading a Photoshop preset changes nothing; the dialog's
        // Apply records the resolved add_adjustment_layer instead.
        // #547 phase 5: color_mode "cmyk" reads a .alv/.acv's ink channels.
        "read_photoshop_adjustment_preset": [.nonRecordable, .documentFree, .cmykTool],
    ]

    func readPhotoshopAdjustmentPreset(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory),
              !isDirectory.boolValue else {
            throw ToolError(message: "No file at \(url.path)")
        }
        let mode = stringArg(a, "color_mode") ?? "rgb"
        guard ["rgb", "cmyk"].contains(mode) else {
            throw ToolError(message: "color_mode must be rgb or cmyk.")
        }
        do {
            let payload = try PhotoshopAdjustmentPreset.read(url, cmyk: mode == "cmyk")
            return try jsonResult([
                "ok": true,
                "op": payload.op,
                "params": payload.params,
                "file": url.path,
                "color_mode": mode,
            ])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// The one catalog entry, behind its own non-inlining boundary for the
    /// reason `AgentCatalog.swift`'s header gives.
    @inline(never) func photoshopPresetCatalog() -> [[String: Any]] {
        let description = [
            "Read a Photoshop adjustment PRESET file and answer the adjustment it holds as ",
            "{ok, op, params, file}: .acv Curves (RGB presets — the composite and the red, ",
            "green and blue curves; a CMYK or multichannel preset refuses), .alv Levels (the ",
            "composite black/white/gamma, its output levels and the red, green and blue ",
            "stages as r/g/b objects), .ahu ",
            "Hue/Saturation, .eap Exposure, .blw Black & White and .cha Channel Mixer (RGB ",
            "presets; a CMYK preset refuses). Files over 64 KiB refuse. color_mode \"cmyk\" ",
            "(#547) reads a .alv's or .acv's INK channels for a CMYK document's Levels or Curves: ",
            "the composite and c, m, y, k (Levels stages on L = 1 − ink, Curves points on the ink ",
            "axes); an RGB .acv and every other extension refuse there. ",
            "Pass op and params to add_adjustment_layer to create the layer — params carry no ",
            "`evaluation`, so an Exposure layer takes add_adjustment_layer's default. Changes ",
            "no document and records no Action step. The UI path is Layer ▸ New Adjustment ",
            "Layer ▸ From Photoshop Preset…, which opens the op's dialog prefilled with these ",
            "values.",
        ].joined()
        return [
            tool(
                "read_photoshop_adjustment_preset", description,
                [
                    "path": [
                        "type": "string",
                        "description": "Absolute path (or ~) to a .acv, .alv, .ahu, .eap, "
                            + ".blw or .cha file (a .cha must be an RGB Channel Mixer preset).",
                    ],
                    "color_mode": [
                        "type": "string", "enum": ["rgb", "cmyk"],
                        "description": "rgb (the default) or cmyk: read a .alv/.acv's ink channels.",
                    ],
                ],
                required: ["path"]),
        ]
    }
}
