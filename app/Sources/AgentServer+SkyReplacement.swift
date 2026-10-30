import AppKit

/// The agent mirrors of Edit ▸ Sky Replacement… (M2a):
///
/// | Handler | UI path it mirrors |
/// |---|---|
/// | `skyReplacement` | Edit ▸ Sky Replacement… ▸ OK (`SkyReplacementSheetController`, `SkyReplacementRun.apply`) |
/// | `listSkies` | the sheet's sky popup: the library's skies by category and the user's own |
///
/// `sky_replacement` runs the sky model on the document, replaces the sky of one raster layer
/// and writes its output above it (the Sky Replacement Group by default, or one new layer) as one
/// undo step, recorded centrally with its arguments. `decontaminate` is off when absent,
/// so an Action step recorded before it existed replays its old result.
/// It takes a library sky id or a user sky's name (`sky`), or an image file (`sky_path`,
/// declared in `AgentPathGuard`). Adding or removing a user sky has no tool: it is the sheet's.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let skyReplacementHandlers: [String: ToolHandler] = [
        "sky_replacement": { $0.skyReplacement },
        "list_skies": { $0.listSkies },
    ]

    /// Facts for this file's tools; merged in AgentServer+Registry.swift. `sky_replacement` runs
    /// at 8 and 16 bits on a raster layer (32 bits and CMYK refuse, take 1);
    /// `list_skies` reads no document and records nothing.
    static let skyReplacementToolFacts: [String: ToolFacts] = [
        "sky_replacement": [.typedTool, .rasterTool],
        "list_skies": [.nonRecordable, .documentFree],
    ]

    /// sky_replacement — mirrors Edit ▸ Sky Replacement… and its OK.
    func skyReplacement(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let allowed: Set<String> = Set(SkyReplacementSettings.sliders.map(\.key)).union([
            "document_id", "layer", "sky", "sky_path", "offset_x", "offset_y", "lighting_mode", "flip",
            "output", "decontaminate",
        ])
        if let unexpected = a.keys.sorted().first(where: { !allowed.contains($0) }) {
            throw ToolError(message: "Unknown sky_replacement parameter: \(unexpected).")
        }
        try refuseNullOrBooleanNumbers(
            a, ["layer", "offset_x", "offset_y"] + SkyReplacementSettings.sliders.map(\.key),
            in: "sky_replacement")
        let settings: SkyReplacementSettings
        do { settings = try SkyReplacementSettings.parse(a) } catch {
            throw ToolError(message: error.localizedDescription)
        }
        let index = try structuralLayerIndex(a, document)
        guard !document.quickMaskActive else { throw ToolError(message: OperationScope.quickMaskRefusal) }
        guard document.planeEditTarget == .layer else {
            throw ToolError(message: "Sky Replacement writes a new layer from a layer's pixels; "
                + "target the layer, not a mask, colour plane or channel.")
        }
        if let reason = SkyReplacementRun.refusal(document: document, layer: index) {
            throw ToolError(message: reason)
        }
        guard ModelGate.ready("sky") != nil else { throw ToolError(message: ModelGate.notInstalled("sky")) }
        let name = a["sky"] as? String
        let path = a["sky_path"] as? String
        guard (name == nil) != (path == nil), a["sky"] == nil || name != nil,
              a["sky_path"] == nil || path != nil else {
            throw ToolError(
                message: "Give exactly one of sky (a library id or your sky's name) and sky_path.")
        }
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let reply: [String: Any]
        do {
            let choice: SkyChoice = try name.map { try SkyLibrary.resolve($0) }
                ?? .file(URL(fileURLWithPath: ((path ?? "") as NSString).expandingTildeInPath))
            let mask = try SkyReplacementRun.mask(document: document)
            let sky = try SkyLibrary.image(choice)
            let planned = RasterDocument.skyReplaceScratchBytes(
                width: doc.width, height: doc.height, skyWidth: sky.width, skyHeight: sky.height,
                bits: doc.isHighDepth ? 16 : 8, output: settings.output,
                decontaminate: settings.decontaminate)
            let covered = mask.coverage.reduce(0) { $0 + ($1 >= 32768 ? 1 : 0) }
            let written = try SkyReplacementRun.apply(
                document: document, layer: index, settings: settings, sky: sky,
                coverage: mask.coverage, record: .notACommand)
            reply = [
                "ok": true, "changed": written != nil, "layer": index,
                "output": settings.output.rawValue, "decontaminate": settings.decontaminate,
                "output_layer": written.map { $0 as Any } ?? NSNull(),
                "sky": SkyLibrary.identifier(choice),
                "model": ["id": mask.entry.id, "version": mask.entry.version],
                "sky_share": (Double(covered) / Double(max(1, mask.coverage.count)) * 10_000)
                    .rounded() / 10_000,
                "planned_bytes": planned,
            ]
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try jsonResult(reply)
    }

    /// list_skies — mirrors the sheet's sky popup: the library's state, its skies and the user's.
    func listSkies(_ a: [String: Any]) throws -> String {
        if let unexpected = a.keys.sorted().first(where: { $0 != "document_id" }) {
            throw ToolError(message: "Unknown list_skies parameter: \(unexpected).")
        }
        let entry = SkyLibrary.entry
        let library: [String: Any] = [
            "state": entry.map { ModelStore.shared.state($0.id).name } ?? "not_downloaded",
            "version": entry?.version ?? NSNull(),
        ]
        let skies: [[String: Any]] = SkyLibrary.presets().map {
            ["id": $0.id, "name": $0.name, "category": $0.category,
             "source_url": $0.sourceURL.absoluteString, "license": $0.license]
        }
        let users: [UserSky]
        do { users = try SkyLibrary.userSkies() } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try jsonResult([
            "library": library, "skies": skies,
            "user_skies": users.map {
                ["name": $0.name, "width": $0.width, "height": $0.height] as [String: Any]
            },
        ])
    }
}
