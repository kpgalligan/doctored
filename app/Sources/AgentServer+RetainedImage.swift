import AppKit

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let retainedImageHandlers: [String: ToolHandler] = [
        "place_image": placeImage,
        "get_image_source": getImageSource,
        "rasterize_image_source": rasterizeImageSource,
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let retainedImageToolFacts: [String: ToolFacts] = [
        "place_image": [.commonTool, .floatTool],
        "get_image_source": [.nonRecordable, .commonTool, .floatTool],
        "rasterize_image_source": [.commonTool, .floatTool],
    ]

    /// Mirrors Layer > Place Embedded… / Place Linked…. The new layer lands
    /// where the `position`/`layer` pair names (12E-2b; `above` the active
    /// entry by default). A camera file develops headless with `raw` over its
    /// sidecar, as the menu's Develop dialog does, and becomes a RAW
    /// layer; every other file lands as an ordinary raster layer. In a 32-bit
    /// float document the dialog's Development row is `raw.output_encoding`,
    /// defaulting as the dialog does
    /// (`RawSourcePlacement.defaultEncoding`).
    func placeImage(_ arguments: [String: Any]) throws -> String {
        let document = try target(arguments)
        let path = try requiredString(arguments, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        let name = stringArg(arguments, "name") ?? url.deletingPathExtension().lastPathComponent
        let raw = try rawOpenSettings(arguments)
        if raw != nil, !RawSourcePlacement.claims(url) {
            throw ToolError(message: "raw applies only to a camera RAW file.")
        }
        // `position` / `layer` (AgentServer+Adjustments.insertionArguments),
        // the same pair every layer-creating tool takes.
        let placement = try insertionArguments(arguments, document: document)
        let storage = try imageSourceStorage(arguments) ?? .embedded
        var disclosures: [String] = []
        if HostSourceInput.claims(url) {
            if arguments["name"] != nil, !(arguments["name"] is String) { throw ToolError(message: "name must be a string.") }
            try requireCurrentHostDecoder(arguments)
            // The place report: what the decode does not carry (the gain-map
            // SDR base), the same disclosure the open warning reports.
            disclosures = try HostSourceWork.place(in: document, url: url, name: name,
                x: hostInteger(arguments, "x", minimum: Int(Int32.min), maximum: Int(Int32.max)) ?? 0,
                y: hostInteger(arguments, "y", minimum: Int(Int32.min), maximum: Int(Int32.max)) ?? 0,
                storage: storage,
                selection: hostPlacementSelection(arguments), expected: hostFingerprints(arguments),
                insertion: placement)
        } else {
            guard arguments["host_frame"] == nil, arguments["source_fingerprints"] == nil, arguments["decoder_version"] == nil else {
                throw ToolError(message: "The recorded host source no longer has a supported host decoder.")
            }
            // The ordinary open's profile refusal, the same key.
            if let refusal = try RetainedImageWork.place(in: document, url: url, name: name,
                                   x: intArg(arguments, "x") ?? 0, y: intArg(arguments, "y") ?? 0,
                                   storage: storage,
                                   insertion: placement,
                                   raw: .headless(raw ?? RawDevelopSettings())) {
                disclosures = [refusal]
            }
        }
        let index = document.activeLayerIndex
        let info = try document.doc?.retainedSourceInfo(index)
        var result: [String: Any] = ["ok": true, "layer": index,
            "position": placement.position.rawValue,
            "source": info as Any? ?? NSNull(),
            "document": summary(document)]
        if !disclosures.isEmpty { result["disclosures"] = disclosures }
        return try jsonResult(result)
    }

    /// Mirrors Layer > RAW Source…'s report of the layer's retained source.
    func getImageSource(_ arguments: [String: Any]) throws -> String {
        let document = try target(arguments)
        let index = intArg(arguments, "layer") ?? document.activeLayerIndex
        guard let doc = document.doc else { throw ToolError(message: "Document has no image.") }
        let info = try LinkedImageWork.info(doc, layer: index)
        return try jsonResult(["layer": index, "source": info as Any? ?? NSNull()])
    }

    /// Mirrors Layer > Rasterize RAW Layer. The current pixels remain exact;
    /// one central Undo transition restores the original source binding.
    func rasterizeImageSource(_ arguments: [String: Any]) throws -> String {
        let document = try target(arguments)
        let index = intArg(arguments, "layer") ?? document.activeLayerIndex
        try rejectLockedEdit(document, index, DR_EDIT_PIXELS)
        try RetainedImageWork.rasterize(in: document, layer: index)
        return try jsonResult(["ok": true, "layer": index, "rasterized_image_source": true,
                              "document": summary(document)])
    }

    func imageSourceStorage(_ arguments: [String: Any]) throws -> ImageSourceStorage? {
        guard let value = arguments["storage"] else { return nil }
        guard let text = value as? String, let storage = ImageSourceStorage(rawValue: text) else {
            throw ToolError(message: "storage must be embedded or linked.")
        }
        return storage
    }

    /// The camera RAW sentence for place_image (`RawSourcePlacement`).
    static let cameraRawSourceSentence = "A camera RAW file (CR3, CR2, NEF, ARW, DNG and the "
        + "other formats open_document develops) becomes a retained RAW layer at any document "
        + "depth: its 16-bit development is the master in an 8- or 16-bit document, where "
        + "raw.output_encoding linear_float refuses. In a 32-bit float document the master is "
        + "the 32-bit float (linear) development (values above white kept) unless raw names "
        + "output_encoding sdr16 or grain, which give the 16-bit "
        + "master rendered into linear light. The camera file is embedded, so "
        + "redevelop_raw, RAW presets, synchronization and RAW locals work on it, and a linked "
        + "layer keeps its link through them. It develops with raw (open_document's develop "
        + "settings) over the exposure of an Adobe Camera Raw .xmp sidecar beside the file; the "
        + "sidecar's other settings are not applied and the reply's disclosures names them. "
        + "A place is sized by the file's resolution against the document's."

    static var imageSourceStorageProperty: [String: Any] {
        ["type": "string", "enum": ["embedded", "linked"],
         "description": "Only a camera RAW file reads it: linked keeps the file's absolute path and fingerprint beside the embedded camera file. Every other image is placed as plain pixels either way."]
    }

    @inline(never) func retainedImageCatalog() -> [[String: Any]] {
        let target: [String: Any] = ["document_id": Self.docIDProperty,
            "layer": ["type": "integer", "minimum": 0, "description": "Layer index; default active layer."]]
        return [
            tool("place_image", "Layer > Place Embedded… / Place Linked…: place an image file as a new layer. A PNG, JPEG or TIFF (8-bit, 16-bit or floating-point), a JPEG XL/HEIC/AVIF/JPEG 2000 still, or an H.264, SDR HEVC (hvc1, 8- or 10-bit) or ProRes 4444 movie frame lands as an ordinary raster layer (an HLG or PQ movie refuses by name: its frame opens only through open_document): its pixels are converted into the document's profile and depth, and nothing of the file is kept, so the reply's source is null. When the ordinary open converted from or ignored the file's embedded colour profile, the reply's disclosures names it. " + Self.cameraRawSourceSentence + " " + LayerInsertPosition.catalogSentence,
                 ["document_id": Self.docIDProperty,
                  "path": ["type": "string", "description": "Local image, movie or camera RAW file."],
                  "storage": Self.imageSourceStorageProperty,
                  "raw": Self.rawDevelopProperty,
                  "host_frame": ["type": "object", "description": "Optional movie or Live Photo selection {kind:still|video,time? or time_value/time_scale}."],
                  "source_fingerprints": ["type": "array", "items": ["type": "string"], "description": "Optional original-file SHA-256 guards for a still, movie or Live Photo pair."],
                  "decoder_version": ["type": "string", "description": "Optional exact system decoder version guard for those files."],
                  "name": ["type": "string", "description": "Layer name; default source filename."],
                  "x": ["type": "integer", "description": "Initial canvas X offset, default 0."],
                  "y": ["type": "integer", "description": "Initial canvas Y offset, default 0."]]
                    .merging(LayerInsertPosition.catalogProperties) { current, _ in current },
                 required: ["path"]),
            tool("rasterize_artwork", "Layer > Rasterize Artwork: remove a text/shape description, or a Generative Layer's generated tag (generative_fill), while preserving exact current pixels, mask and profile. Undo restores editability. Use before perspective distortion or deliberate raster editing.", target),
            tool("get_image_source", "Layer > RAW Source…: a RAW layer's retained source — identity/revision, dimensions, master and rendered depth, profile, affine, the raw development and current link_status (embedded/current/changed/missing/unreadable). Checks a linked file's fingerprint without changing the document. source=null for every other layer.", target),
            tool("rasterize_image_source", "Layer > Rasterize RAW Layer: detach the camera original and its development, preserving exact current layer pixels. Undo restores them. Required before destructive pixel edits of a RAW layer.", target)
        ]
    }
}
