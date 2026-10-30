import Foundation

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let rawSourceHandlers: [String: ToolHandler] = [
        "get_raw_source": getRawSource,
        "redevelop_raw": redevelopRaw,
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let rawSourceToolFacts: [String: ToolFacts] = [
        "get_raw_source": [.nonRecordable, .commonTool, .floatTool],
        "redevelop_raw": [.commonTool, .floatTool],
    ]

    /// Mirrors Layer > Redevelop RAW, preserving the existing layer stack.
    /// `raw.output_encoding` mirrors the window's Development row in a 32-bit
    /// float document: `linear_float` keeps a 32-bit float (linear)
    /// master, `sdr16` the 16-bit SDR one.
    func redevelopRaw(_ arguments: [String: Any]) throws -> String {
        try rawWorkflowKeys(arguments, allowed: ["document_id", "layer", "raw"])
        let document = try target(arguments)
        let layer = try rawWorkflowLayer(arguments, document: document)
        let settings = try rawOpenSettings(arguments) ?? RawDevelopSettings()
        try RawSourceWork.redevelop(in: document, layer: layer, settings: settings)
        guard let source = document.doc else { throw ToolError(message: "Document has no image.") }
        return try jsonResult(["ok": true, "layer": layer, "source": RawSourceWork.info(source: source, layer: layer),
                               "document": summary(document)])
    }

    /// Mirrors Layer > RAW Source. Inspection never invokes a RAW decoder.
    func getRawSource(_ arguments: [String: Any]) throws -> String {
        try rawWorkflowKeys(arguments, allowed: ["document_id", "layer"])
        let document = try target(arguments)
        let layer = try rawWorkflowLayer(arguments, document: document)
        guard let source = document.doc else { throw ToolError(message: "Document has no image.") }
        return try jsonResult(["layer": layer, "source": RawSourceWork.info(source: source, layer: layer)])
    }

    @inline(never) func rawSourceCatalog() -> [[String: Any]] {
        let target: [String: Any] = ["document_id": Self.docIDProperty,
            "layer": ["type": "integer", "minimum": 0, "description": "Retained RAW layer; defaults to active layer."]]
        var edit = target
        edit["raw"] = Self.rawDevelopProperty
        var inspect = edit
        inspect["region"] = ["type": "object", "description": "Optional 100% source-pixel region {x,y,width,height}; within source, at most 1024 per side. Omit for Fit."]
        inspect["clipping"] = ["type": "string", "enum": ["none", "shadows", "highlights", "both"], "description": "Exact normalized output endpoint overlay, independent of display quantization."]
        var migrationPreview = inspect
        migrationPreview.removeValue(forKey: "raw")
        migrationPreview["geometry"] = ["type": "object", "description": "Optional reviewed EXIF/affine inverse map from proposed source coordinates into the saved source; required for unregistered decoder extent changes."]
        var migration = target
        migration["preview_id"] = ["type": "string", "description": "Unexpired review ticket from preview_raw_decoder_migration; any editing-state change invalidates it."]
        return [
            tool("redevelop_raw", "Layer > Redevelop RAW: patch the retained RAW's effective development recipe and regenerate its genuine 16-bit source below existing retouch layers. Omitted fields keep SAVED values, including white balance. Requires the captured decoder implementation/version; unavailable decoders leave the saved rendering intact. Original camera bytes are embedded; no source path is needed. Preserves accumulated affine, masks, layer properties and other layers. Empty/unchanged requests are no-ops; successful changes use one Undo. Explicit normalized SDR sRGB, Display P3, Adobe RGB (1998) or ProPhoto RGB source encoding; document profile stays unchanged and an 8-bit document quantizes only the fallback. In a 32-bit float document the 16-bit development is rendered into linear light, as a placed image is, unless raw.output_encoding is linear_float: the layer then keeps a 32-bit float (linear) master with values above white (tone_curve and shadows apply and keep values above white; grain applies on the display encoding; nonzero texture, clarity or dehaze refuse), and sdr16 switches it back; the layer's locals are kept either way. linear_float on a layer of an 8- or 16-bit document refuses. Unsupported controls refuse.", edit),
            tool("get_raw_source", "Layer > RAW Source: inspect the embedded original file identity, dense recipe, decoder version/implementation and availability diagnostic without decoding or modifying saved work. Saved rendering remains available when redevelopment is unavailable.", target),
            tool("inspect_raw_development", "Develop RAW > Fit / 100%, Before / After, Histogram and Clipping: inspect the complete original u16 developed source before document transforms, retouch, masks and selection. Returns Before then After PNG display images plus full-source original-code histograms and exact endpoint counts. Optional raw patch previews new settings without committing; omitted raw reads saved pixels without a decoder. region chooses native source pixels, not a scaled approximation. Normalized output clipping is not sensor saturation. A 32-bit float (linear) development's PNG images are its SDR-clipped view: its values are clamped to 0…1 and encoded by its profile, so a red marker there means at or above white (sample_pixel reads the layer's real values). Its histogram reads the float samples: storage rgba32_float, stop bins over -12…+6 EV, above_white_rgba, exact_one_rgba, below_zero_rgba and exact_zero_rgba (the histogram tool's float schema). When either master is 32-bit float the reply adds hdr_preview: before_peak, before_divisor, before_marked, after_peak, after_divisor and after_marked, the largest RGB value of that side's extended image (markers excluded), its divisor and its clipping-marked pixel count (all null for a 16-bit side). It follows the Develop window's HDR view on a screen with headroom: Fit, or with region the 100% region at divisor 1; clipping marks samples at or below 0.0 and at or above 1.0.", inspect),
            tool("preview_raw_decoder_migration", "Layer > Migrate RAW Decoder: develop a current-decoder candidate, deriving registered EXIF/crop/pad geometry or requiring a reviewed inverse map. Returns independently fit Before/After images, dimensions, histograms, exact mask fingerprints, resolved geometry and a five-minute preview_id. One app-wide ticket; unknown recipe semantics refuse editing.", migrationPreview),
            tool("migrate_raw_decoder", "Apply the exact reviewed RAW decoder candidate from preview_raw_decoder_migration. Requires its current preview_id; source, revision, target and selection must match the review. Preserves original camera data, layer geometry and other layers, with one Undo. Actions records the approved prior/new descriptors and refuses replay against a different source, recipe or current decoder.", migration, required: ["preview_id"])
        ]
    }
}
