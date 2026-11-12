import AppKit

/// The agent's half of Layer > Place Live Photo…
/// (EditorViewController+LivePhoto.swift), and the host-media argument
/// readers `place_image` shares with it. The pair's still (or a chosen movie
/// frame) lands as an ordinary raster layer; nothing of the source files is
/// kept in the document.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let livePhotoHandlers: [String: ToolHandler] = [
        "add_live_photo_layer": addLivePhotoLayer,
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let livePhotoToolFacts: [String: ToolFacts] = [
        "add_live_photo_layer": [.nonRecordable, .commonTool, .floatTool],
    ]

    /// Mirrors Layer > Place Live Photo….
    func addLivePhotoLayer(_ arguments: [String: Any]) throws -> String {
        try hostKeys(arguments, allowed: ["document_id", "layer", "path", "name", "time", "kind",
            "time_value", "time_scale", "source_fingerprints", "decoder_version", "position"])
        let document = try target(arguments)
        let url = URL(fileURLWithPath: (try requiredString(arguments, "path") as NSString).expandingTildeInPath)
        let input = try HostSourceInput(url: url)
        guard input.entries.contains(where: { $0.role == "video" }) else {
            throw ToolError(message: "Choose a supported movie or a Live Photo pair sharing one basename.")
        }
        if arguments["name"] != nil, !(arguments["name"] is String) { throw ToolError(message: "name must be a string.") }
        // The `position`/`layer` pair every layer-creating tool takes (12E-2b).
        let placement = try insertionArguments(arguments, document: document)
        let selection = arguments["kind"] != nil || arguments["time"] != nil || arguments["time_value"] != nil || arguments["time_scale"] != nil
            ? try hostFrameSelection(arguments, defaultKind: "video") : nil
        try requireCurrentHostDecoder(arguments)
        let disclosures = try HostSourceWork.place(in: document, url: url,
            name: stringArg(arguments, "name") ?? url.deletingPathExtension().lastPathComponent,
            selection: selection, expected: hostFingerprints(arguments), insertion: placement)
        var result: [String: Any] = ["ok": true, "layer": document.activeLayerIndex,
            "position": placement.position.rawValue, "document": summary(document)]
        if !disclosures.isEmpty { result["disclosures"] = disclosures }
        return try jsonResult(result)
    }

    func hostFingerprints(_ arguments: [String: Any]) throws -> [String]? {
        guard let raw = arguments["source_fingerprints"] else { return nil }
        guard let values = raw as? [String], (1...2).contains(values.count), values.allSatisfy(HostSourceError.validHash) else {
            throw ToolError(message: "source_fingerprints must contain one or two lowercase SHA-256 strings.")
        }
        return values
    }
    func requireCurrentHostDecoder(_ arguments: [String: Any]) throws {
        if let value = arguments["decoder_version"] {
            guard let version = value as? String, version == ProcessInfo.processInfo.operatingSystemVersionString else {
                throw ToolError(message: "The recorded host decoder version differs from this system. No source was decoded.")
            }
        }
    }
    private func hostKeys(_ arguments: [String: Any], allowed: Set<String>) throws {
        if let key = Set(arguments.keys).subtracting(allowed).sorted().first { throw ToolError(message: "Unknown host-source field: \(key)") }
    }
    func hostFrameSelection(_ arguments: [String: Any], defaultKind: String? = nil) throws -> HostFrameSelection {
        if arguments["kind"] != nil, !(arguments["kind"] is String) { throw ToolError(message: "kind must be a string.") }
        guard let kind = stringArg(arguments, "kind") ?? defaultKind, ["still", "video"].contains(kind) else {
            throw ToolError(message: "kind must explicitly be still or video.")
        }
        if kind == "still" {
            guard arguments["time"] == nil, arguments["time_value"] == nil, arguments["time_scale"] == nil else {
                throw ToolError(message: "Still selection does not take a movie time.")
            }
            return .still
        }
        if arguments["time_value"] != nil || arguments["time_scale"] != nil {
            guard arguments["time"] == nil,
                  let value = try hostInteger(arguments, "time_value", minimum: 0, maximum: 9_007_199_254_740_991),
                  let scale = try hostInteger(arguments, "time_scale", minimum: 1, maximum: Int(Int32.max)) else {
                throw ToolError(message: "Use an exact integer time_value/time_scale pair, or time in seconds, but not both.")
            }
            return .video(HostMediaTime(Int64(value), Int32(scale)))
        }
        guard let raw = arguments["time"], !Self.isJSONBoolean(raw),
              let seconds = doubleArg(arguments, "time"), seconds.isFinite else { throw ToolError(message: "Video selection needs finite time or an exact time_value/time_scale pair.") }
        return .video(try .seconds(seconds))
    }
    func hostInteger(_ arguments: [String: Any], _ name: String, minimum: Int, maximum: Int) throws -> Int? {
        guard let raw = arguments[name] else { return nil }
        let value: Int?
        if let text = raw as? String { value = Int(text) }
        else if let number = raw as? NSNumber, !Self.isJSONBoolean(number), number.doubleValue.isFinite,
                number.doubleValue.rounded() == number.doubleValue {
            value = Int(exactly: number.doubleValue)
        } else { value = nil }
        guard let value, (minimum...maximum).contains(value) else {
            throw ToolError(message: "\(name) must be an integer in \(minimum)...\(maximum), without Boolean or fractional coercion.")
        }
        return value
    }
    func hostPlacementSelection(_ arguments: [String: Any]) throws -> HostFrameSelection? {
        guard let raw = arguments["host_frame"] else { return nil }
        guard let frame = raw as? [String: Any] else { throw ToolError(message: "host_frame must be a frame selection object.") }
        try hostKeys(frame, allowed: ["kind", "time", "time_value", "time_scale"])
        return try hostFrameSelection(frame)
    }

    @inline(never) func livePhotoCatalog() -> [[String: Any]] {
        var placement: [String: Any] = ["document_id": Self.docIDProperty]
        placement["kind"] = ["type": "string", "enum": ["still", "video"]]
        placement["time"] = ["type": "number", "description": "Video seconds; greatest presentation timestamp <= request, inside track start..<end."]
        placement["time_value"] = ["type": "integer", "description": "Exact rational movie-time numerator; pair with time_scale."]
        placement["time_scale"] = ["type": "integer", "minimum": 1]
        placement["source_fingerprints"] = ["type": "array", "items": ["type": "string"], "description": "Optional original-file SHA-256 guard."]
        placement["decoder_version"] = ["type": "string", "description": "Optional exact system decoder-version guard."]
        placement["path"] = ["type": "string", "description": "Supported movie, or either half of a same-basename JPEG/HEIC and movie pair."]
        placement["name"] = ["type": "string", "description": "Layer name; default source basename."]
        // THE one schema every layer-creating tool merges in (`LayerInsertion.swift`).
        placement.merge(LayerInsertPosition.catalogProperties) { _, shared in shared }
        return [tool("add_live_photo_layer", "Layer > Place Live Photo: place one frame of a Live Photo or movie as an ordinary raster layer at the document's depth, and select it. A same-basename JPEG/HEIC and movie pair defaults to its original still; a bare movie defaults to its first presentation sample. Pairing is by filename, not asset-ID verification. Optional kind/time selects a movie frame. The movie may be H.264, SDR HEVC (hvc1, 8- or 10-bit 4:2:0) or ProRes 4444; an HLG or PQ movie (or a pair holding one) refuses by name, as its frame opens only as a 32-bit float document through open_document. The source files are not kept in the document: the layer is plain pixels, converted into the document's profile. " + LayerInsertPosition.catalogSentence, placement, required: ["path"])]
    }
}
