import AppKit

// **12F-1b** made a `canvas_size` that would change NOTHING the house no-op at every depth:
// `AgentServer.canvasSize` returns `structureNoOp` after the arguments are parsed and validated and
// BEFORE `requireChannelBudget`/`performGroupedEdit` (six lines in the frozen file), mirroring the
// sheet's own early return in `Sheets.swift` — which needed no change — so there is no history step
// and no dirtying at 8, 16 or float, and a recorded step or a default-policy batch no longer halts
// on a document that is already the target size.

/// Hosts the core's embedded MCP server (streamable HTTP over a Unix
/// socket, `AgentServer+Socket.swift`) and executes its tool calls
/// against the open documents. The Rust side owns the protocol; this class
/// owns the tool catalog and the dispatch, which runs on the main thread so
/// every edit flows through the same applyEdit path as the UI — undoable,
/// dirtying, and live in the window.
///
/// The socket is bound at every launch, in a `0700` folder, so only this
/// user's processes reach it; external MCP clients start the bundled
/// `Contents/Helpers/doctored-mcp` (Settings ▸ MCP copies the command),
/// which forwards to it. There is no TCP listener and no switch.
final class AgentServer {
    static let shared = AgentServer()

    /// The socket this instance bound at launch, nil when it bound none.
    var boundSocketPath: String?
    var isRunning: Bool { boundSocketPath != nil }

    private let documentIDs = AgentDocumentIDs()

    // Not private, like the helpers below it: the per-feature extension
    // files (AgentServer+…) build their handlers from the same pieces, and
    // handlers unreachable from the dispatch registry
    // (`AgentServer+Registry.swift`) would be dead.
    struct ToolError: Error {
        let message: String
        /// nil = an unavailable command (ActionFailurePrompt.swift).
        var kind: ToolErrorKind? = nil
    }

    /// The last `execute` ToolError's kind, for `ActionPlayer`.
    var lastToolErrorKind: ToolErrorKind?

    /// What a pixel edit dropped from a layer, for the report back: a layer
    /// whose pixels are the RENDERING of a description — a text layer's
    /// string, a shape layer's geometry — stops
    /// being that the moment something paints over them.
    enum DroppedDescription: String {
        case text
        case shape
        case fill
    }

    // MARK: - Dispatch (main thread)

    /// Executes one tool call and returns CallToolResult JSON. Errors are
    /// in-band (isError: true) so the model can read and correct them.
    func execute(tool: String, argumentsJSON: String, waitsForWorker: Bool = true) -> String {
        assert(Thread.isMainThread)
        // A replay owns the app for the length of its run: a call that
        // landed inside one would be folded into the action's single undo
        // entry and silently reverted with it (ActionPlayer.replayRefusal).
        if let refusal = ActionPlayer.replayRefusal(tool: tool) { return errorResult(refusal) }
        var arguments =
            (try? JSONSerialization.jsonObject(with: Data(argumentsJSON.utf8)))
            as? [String: Any] ?? [:]
        if waitsForWorker { waitForIdleWorker(tool: tool, arguments: arguments) }
        do {
            try AgentPathGuard.shared.check(tool: tool, arguments: arguments)
            // The canvas this call was WRITTEN against, sampled before it
            // runs: a crop, a rotate or a resize has replaced the handle by
            // the time the step is recorded, and the coordinates in it belong
            // to the size it started from (ActionRecorder.noteCanvas). nil
            // for a call that has no document yet — it creates its own — and
            // the recorder then reads the size the call produced.
            if BackgroundWork.isWaiting, ActionPlayer.isPlaying,
               !["job_status", "cancel_job", "resource_usage"].contains(tool) {
                throw ToolError(message: "An action step is running; wait for it to finish.")
            }
            let before = try? target(arguments)
            if before?.isPublishingEditingState == true,
               !["job_status", "cancel_job", "resource_usage"].contains(tool) {
                throw ToolError(message: "An editing state is being published. Wait for the current operation to finish.")
            }
            try refuseLiveGestureMutation(tool: tool, document: before)
            if let before, let reason = DocumentPrecision.refusal(tool: tool, arguments: arguments, document: before) { throw ToolError(message: reason) }
            arguments = try scopedToneArguments(tool: tool, arguments: arguments)
            arguments = try levelsAutoArguments(tool: tool, arguments: arguments, document: before)
            arguments = try calibratedArguments(tool: tool, arguments: arguments, document: before)
            arguments = try nativeStrokeArguments(tool: tool, arguments: arguments, document: before)
            if Self.documentJobTools.contains(tool),
               arguments["preview_id"] == nil,
               arguments["layer"] == nil, arguments["index"] == nil, let before {
                arguments["layer"] = before.activeLayerIndex
            }
            let canvas = before?.doc.map { (width: $0.width, height: $0.height) }
            before?.fadeHint = FadeStep.hint(tool: tool, arguments: arguments)
            defer { before?.fadeHint = nil }
            let result = try dispatch(tool, arguments)
            // The agent half of the recording seam: ONE line covering all
            // 107 tools, the HTTP MCP server and the assistant panel alike.
            // Only a call that SUCCEEDED is recorded, and a read-only tool
            // records nothing (ActionCatalogFacts.nonRecordable).
            // `target(arguments)` resolves the SAME document the handler
            // just edited — the id when one was passed, the front or only
            // document otherwise — so the recording's canvas is bound to
            // what the call touched rather than to whatever window happens
            // to be open (ActionRecorder.noteCanvas). The size above is
            // carried only when that is the same document it was read from,
            // so a call that made a new one cannot be filed under the old
            // one's canvas.
            let after = try? target(arguments)
            let deferred = Self.defersRecording(tool: tool, arguments: arguments, document: before)
            if !deferred && !(tool == "convert_precision" && boolArg(arguments, "dry_run") == true)
                && !Self.isUnchangedCanvasGeometryResult(tool: tool, result: result)
                && !(tool == "warp_layer" && Self.isUnchangedWarpResult(result)) {
                ActionRecorder.shared.recordAgentCall(
                    tool, tool == "warp_layer" ? PersistedWarpAction.exactArguments(arguments) : arguments,
                    on: after, canvas: after === before ? canvas : nil)
            }
            return result
        } catch let error as ToolError {
            lastToolErrorKind = error.kind
            return errorResult(error.message)
        } catch {
            lastToolErrorKind = nil
            return errorResult(error.localizedDescription)
        }
    }

    private func dispatch(_ tool: String, _ a: [String: Any]) throws -> String {
        if let result = try dispatchPrecision(tool, a) { return result }
        guard let handler = Self.handlers[tool] else {
            throw ToolError(message: "Unknown tool: \(tool)")
        }
        return try handler(self)(a)
    }

    /// Tool name → handler for the tools whose handler methods are private
    /// to this file. Every extension file that defines handlers declares its
    /// own table beside them; AgentServer+Registry.swift merges them all.
    static let coreHandlers: [String: ToolHandler] = [
        // Documents
        "list_documents": listDocuments,
        "open_document": openDocument,
        "get_document": getDocument,
        // Rendering
        "render": render,
        "sample_color": sampleColor,
        // Layer operations
        "set_active_layer": setActiveLayer,
        "new_layer": newLayer,
        "flatten_image": flattenImage,
        "transform_layer": transformLayer,
        // Adjustment layers and filters
        "add_adjustment_layer": addAdjustmentLayer,
        "edit_adjustment_layer": editAdjustmentLayer,
        "apply_filter": applyFilter,
        // Painting (brush, eraser, text)
        "brush_stroke": brushStroke,
        "eraser_stroke": eraserStroke,
        "add_text": addText,
        // Selection, fill, gradient
        "select_rect": selectRect,
        "select_ellipse": selectEllipse,
        "select_polygon": selectPolygon,
        "select_magic_wand": selectMagicWand,
        "select_all": selectAll,
        "deselect": deselect,
        "modify_selection": modifySelection,
        "fill": fill,
        "gradient": gradient,
        "clear_selection": clearSelection,
        // Whole-document geometry
        "image_size": imageSize,
        "canvas_size": canvasSize,
        // Undo, export
        "undo": undo,
        "redo": redo,
        "save_copy": saveCopy,
    ]

    // The dispatch switch's old inline one-liners, as named handlers the
    // table can reference.

    private func flattenImage(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard try editStructure(document, "Flatten Image", { try $0.flattening() }) else {
            throw ToolError(message: "Flatten Image failed — check the parameters")
        }
        return try jsonResult(["ok": true, "document": summary(document)])
    }

    private func brushStroke(_ a: [String: Any]) throws -> String {
        try paintStroke(a, erase: false)
    }

    private func eraserStroke(_ a: [String: Any]) throws -> String {
        try paintStroke(a, erase: true)
    }

    private func selectRect(_ a: [String: Any]) throws -> String {
        try selectShape(a) { rect in .rect(rect) }
    }

    private func selectEllipse(_ a: [String: Any]) throws -> String {
        try selectShape(a) { rect in .ellipse(rect) }
    }

    /// Mirrors Select > All (EditorViewController.selectAll). The same
    /// `applySelection` helper the marquee tools use, so feathering and the
    /// combine modes behave identically — and, being canvas-RELATIVE by
    /// construction, it is what lets a recorded Select All replay on a
    /// differently sized document.
    private func selectAll(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        return try applySelection(
            document,
            .rect(CGRect(x: 0, y: 0, width: doc.width, height: doc.height)),
            mode: selectionMode(a))
    }

    private func undo(_ a: [String: Any]) throws -> String { try undoRedo(a, redo: false) }

    private func redo(_ a: [String: Any]) throws -> String { try undoRedo(a, redo: true) }

    // MARK: - Documents

    private func openDocuments() -> [ImageDocument] {
        NSDocumentController.shared.documents.compactMap { $0 as? ImageDocument }
    }

    private func id(for document: ImageDocument) -> Int { documentIDs.id(for: document) }

    /// The stable tool-facing id of a document (the assistant panel pins
    /// its session to one document through this).
    func documentID(for document: ImageDocument) -> Int {
        id(for: document)
    }

    /// The document a call targets: document_id when given, else the
    /// current (main window's) document, else the only open one.
    func target(_ a: [String: Any]) throws -> ImageDocument {
        let documents = openDocuments()
        if let wanted = intArg(a, "document_id") {
            guard let match = documentIDs.document(withID: wanted, among: documents) else {
                throw ToolError(message: "No open document has id \(wanted). Call list_documents.")
            }
            return match
        }
        if let current = NSDocumentController.shared.currentDocument as? ImageDocument {
            return current
        }
        guard let first = documents.first else {
            throw ToolError(message: "No documents are open. Call open_document first.")
        }
        return first
    }

    // Internal, not private, like target and the helpers around it: the
    // +Feature handler files report documents with it.
    func summary(_ document: ImageDocument) -> [String: Any] {
        [
            "id": id(for: document),
            "title": document.displayName ?? "Untitled",
            "path": document.fileURL?.path ?? NSNull(),
            "width": document.doc?.width ?? 0,
            "height": document.doc?.height ?? 0,
            "layer_count": document.doc?.layerCount ?? 0,
            "active_layer": document.activeLayerIndex,
            "selected_layers": document.selectedLayerIndices,
            "unsaved_changes": document.isDocumentEdited,
            "projection_error": document.projectionError ?? NSNull(),
            "preview_is_current": document.doc?.isHighDepth == true ? document.precisionProjection != nil : document.projection != nil,
            "precision": document.doc?.precisionName ?? "rgba8",
            "color_mode": document.doc?.colorMode.rawValue ?? "rgb",
        ]
    }

    private func listDocuments(_: [String: Any]) throws -> String {
        let list = openDocuments().map(summary)
        return try jsonResult(["documents": list, "note": "layer index 0 is the bottom layer"])
    }

    private func openDocument(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw ToolError(message: "No file at \(url.path)")
        }
        let controller = NSDocumentController.shared
        // Parsed BEFORE the already-open short-circuit, so a bad value is
        // refused by name whether or not the file happens to be open — and so
        // a caller who asked for a different develop is told their settings
        // were dropped rather than handed the first develop in silence.
        // A camera RAW develops HEADLESSLY for an agent — never the modal
        // Develop dialog, which would hang this call (AgentServer+Raw.swift).
        let precision = try PrecisionImportRequest.requested(a["precision"])
        let hdrPhoto = try HDRPhotoRequest.requested(a["hdr_photo"])  // File ▸ Open HDR Photos as HDR
        let raw = try rawOpenSettings(a)
        let psd = try psdOpenRequest(a, url: url)
        // Phase 11D: a PDF/SVG open takes a page and a resolution, parsed
        // here for the same reason the develop settings above are — a bad
        // value is refused by name — and refused by name on a path this
        // route does not own.
        let vector = try VectorImportRequest.requested(page: a["page"], ppi: a["ppi"])
        if vector != nil, !VectorImport.claims(url) {
            throw ToolError(message: "page and ppi apply to a PDF or SVG open only.")
        }
        if let existing = controller.document(for: url) as? ImageDocument {
            var result: [String: Any] = ["already_open": true, "document": summary(existing)]
            if let imported = existing.psdImport {
                result["psd_import"] = imported.json
                result["psd_note"] = "The existing document is returned; its import choice is unchanged."
            }
            if let note = rawAlreadyOpenNote(requested: raw, document: existing) {
                result["note"] = note
            }
            return try jsonResult(result)
        }
        let type = try controller.typeForContents(of: url)
        let previousPrecision = PrecisionImportRequest.mode
        PrecisionImportRequest.mode = precision
        defer { PrecisionImportRequest.mode = previousPrecision }
        let previousHDR = HDRPhotoRequest.mode
        HDRPhotoRequest.mode = hdrPhoto
        defer { HDRPhotoRequest.mode = previousHDR }
        let previousPSD = PSDImportRequest.mode
        PSDImportRequest.mode = psd
        defer { PSDImportRequest.mode = previousPSD }
        RawImportRequest.mode = .headless(raw ?? RawDevelopSettings())
        defer { RawImportRequest.mode = .ask }
        let previousVector = VectorImportRequest.mode
        if VectorImport.claims(url) {
            VectorImportRequest.mode = .headless(vector ?? VectorImportChoice())
        }
        defer { VectorImportRequest.mode = previousVector }
        guard let document = try controller.makeDocument(withContentsOf: url, ofType: type)
            as? ImageDocument
        else {
            throw ToolError(message: "\(url.lastPathComponent) did not open as an image document")
        }
        controller.addDocument(document)
        document.makeWindowControllers()
        document.showWindows()
        controller.noteNewRecentDocumentURL(url)
        var result: [String: Any] = ["document": summary(document)]
        if let imported = document.psdImport { result["psd_import"] = imported.json }
        if let note = rawOpenNote(requested: raw, document: document) { result["note"] = note }
        result["lens_profile"] = document.metadataImportReport?["lens_profile"]  // RAW only
        result["hdr_open"] = document.metadataImportReport?["hdr_open"]  // HDR photos only
        return try jsonResult(result)
    }

    private func getDocument(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let hostLease = try PrecisionEditPayload.admitted(bytes:
            UInt64(doc.retainedSourceIndices.count) * 65_536 + 65_536)
        defer { withExtendedLifetime(hostLease) {} }
        let tree = doc.layerTree
        // Every row's content box in ONE bottom-up pass, never one descendant
        // sweep per row (AgentServer+Groups.contentBoxes).
        let boxes = Self.contentBoxes(doc, tree)
        let layers: [[String: Any]] = (0..<doc.layerCount).compactMap { index -> [String: Any]? in
            guard let info = doc.layerInfo(index) else { return nil }
            var layer: [String: Any] = [
                "index": index,
                "name": info.name,
                "width": info.width,
                "height": info.height,
                "offset_x": info.offsetX,
                "offset_y": info.offsetY,
                "opacity": (info.opacity * 100).rounded() / 100,
                "blend_mode": DrBlendMode.displayName(for: info.blendMode),
                "visible": info.visible,
                "has_mask": doc.layerHasMask(index),
                "mask_enabled": doc.layerMaskEnabled(index),
                "clipped": doc.layerClipped(index),
                // The colour label the panel filters on (LayerLabel.swift).
                "label": doc.layerLabel(index).name,
            ]
            // kind, depth, parent, children, open, locks,
            // inherited_locks/lock_owner, linked and the content box
            // (AgentServer+Groups.swift).
            layer.merge(
                Self.structureFields(
                    doc, index, tree, contentBox: index < boxes.count ? boxes[index] : nil)
            ) { _, new in new }
            // A TEXT LAYER also carries the description its pixels were
            // rendered from (typography, transform and origin included);
            // only those layers can be re-rendered with edit_text_layer, and
            // the key's absence says "plain raster".
            if let payload = doc.textPayload(index) {
                layer["text"] = Self.textFields(payload, anchor: doc.describedAnchor(index))
            }
            // An ADJUSTMENT layer recolors everything below it while
            // compositing and its own pixels are ignored (the core's parse
            // is authoritative); the description it composites from is what
            // edit_adjustment_layer replaces.
            let isAdjustment = doc.layerIsAdjustment(index)
            layer["is_adjustment"] = isAdjustment
            if isAdjustment, let payload = doc.adjustmentPayload(index) {
                // Every key survives except a Color Lookup's table, which
                // would swamp the reply (AgentServer+Adjustments.swift).
                layer["adjustment"] = [
                    "op": payload.op, "params": elidedAdjustmentParams(payload),
                ]
            }
            // A SHAPE layer carries the parametric description its pixels
            // were rendered from (position is its origin).
            if let payload = doc.shapePayload(index) {
                layer["shape"] = Self.shapeFields(payload, anchor: doc.describedAnchor(index))
            }
            // A FILL layer carries its color or gradient (edit_fill_layer).
            if let payload = doc.fillPayload(index) { layer["fill"] = Self.fillFields(payload) }
            // A STYLED layer reports its full style object (the canonical
            // JSON set_layer_style takes back).
            if let style = Self.layerStyleFields(doc, index) {
                layer["style"] = style
            }
            if let mask = doc.layerMaskProperties(index) { layer["mask_properties"] = mask.report }
            layer.merge(RetainedSourceInspection.fields(doc, layer: index)) { _, new in new }
            return layer
        }
        var result = summary(document)
        result["layers"] = layers
        result["global_light"] = Self.globalLightFields(doc)
        // Every other document-level key, shared with the typed reply
        // (AgentServer+DocumentReply.swift).
        result.merge(try documentFields(document, doc, tree)) { _, new in new }
        return try jsonResult(result)
    }

    // MARK: - Rendering

    private func render(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        var source: RasterImage?
        var what: String
        var spotNote = ""
        // The space the SOURCE pixels are in: a colour plane rendered as
        // grey is coverage and is sRGB by construction; everything else is
        // the document's.
        var space = doc.colorSpace
        // Phase 13A-1: a REGION or PREVIEW of the flattened canvas goes through
        // the core's regional compositor, which equals the full projection
        // sample for sample or refuses the stack by name (never a full-frame
        // fallback). It mirrors no menu item: the canvas display still renders
        // the whole projection.
        if a["region"] != nil || a["preview_divisor"] != nil, a["layer"] != nil || a["channel"] != nil {
            throw ToolError(message: "region and preview_divisor apply to the flattened canvas only")
        }
        let region = try Self.regionArgument(a, doc: doc)
        // A float render's view exposure (AgentServer+FloatDocument.swift).
        let view = try renderExposure(a, doc: doc)
        if let channel = stringArg(a, "channel"),
           !(doc.isIndexed && IndexedDocument.compositeChannels.contains(channel.lowercased())),
           !(doc.isBitmap && BitmapDocument.compositeChannels.contains(channel.lowercased())) {
            // ONE plane as grayscale instead of the colour image, described
            // by what the argument actually resolved to — an alpha channel
            // belongs to no layer (AgentServer+ChannelTargets.swift).
            let resolved = try Self.planeRenderImage(
                doc, channel: channel, layer: intArg(a, "layer"))
            source = resolved.image
            what = resolved.what
            space = ColorProfile.sRGB
        } else if let layer = intArg(a, "layer") {
            guard layer >= 0, layer < doc.layerCount else {
                throw ToolError(message: "Layer \(layer) is out of range (0..\(doc.layerCount - 1))")
            }
            // A GROUP has no pixels of its own, so what it "looks like" is
            // its own projection on a canvas-sized transparent buffer.
            source = doc.isHighDepth ? try nativeLayerDisplay(doc, layer: layer) : doc.layerIsGroup(layer) ? doc.layerCanvasImage(layer) : doc.layerImage(layer)
            what = "\(doc.layerIsGroup(layer) ? "group" : "layer") \(layer) "
                + "(\(doc.layerInfo(layer)?.name ?? ""))"
        } else if let region {
            if let precision = doc.precision {
                let context = try PrecisionWorkContext(source: precision)
                let image = try precision.renderRegion(region, workingLimit: context.limit,
                                                       otherRetained: context.otherRetained)
                source = try doc.display(of: image, context: try PrecisionWorkContext(image: image), view: view)
            } else {
                source = try doc.renderRegion(region, maxOutputBytes: RasterResources.shared.workingLimit)
            }
            // #1 1c-1: a region overprints the spot channels at its own samples (SpotChannel.swift).
            let spots = try source.map { try SpotChannels.regionComposite($0, doc: doc, region: region) }
            source = spots?.0 ?? source; spotNote = SpotChannels.renderNote(spots?.1 ?? 0)
            what = doc.isFloat ? "8-bit tone-mapped display of 32-bit float canvas" : doc.isHighDepth ? "8-bit display preview of 16-bit canvas" : "flattened canvas"
        } else {
            // #1: the full canvas shows every spot channel overprinted (SpotChannel.swift).
            let overprinted = try SpotChannels.renderComposite(try document.currentDisplayProjection(view: view), doc: doc)
            source = overprinted.0
            spotNote = SpotChannels.renderNote(overprinted.1)
            what = doc.isFloat ? "8-bit tone-mapped display of 32-bit float canvas" : doc.isHighDepth ? "8-bit display preview of 16-bit canvas" : "flattened canvas"
        }
        what += Self.renderExposureNote(view)
        guard var image = source else { throw ToolError(message: "Could not render") }

        let scale = try Self.renderScale(a)
        let maxSide = min(max(intArg(a, "max_side") ?? 1024, 64), 4096)
        let fullWidth = image.width
        let fullHeight = image.height
        if let scale { image = try Self.pixelScaled(image, by: scale) }
        if scale == nil, max(fullWidth, fullHeight) > maxSide {
            let scale = Double(maxSide) / Double(max(fullWidth, fullHeight))
            let w = max(Int((Double(fullWidth) * scale).rounded()), 1)
            let h = max(Int((Double(fullHeight) * scale).rounded()), 1)
            guard let scaled = image.resized(w: w, h: h, filter: DR_FILTER_BILINEAR) else {
                throw ToolError(message: "Could not scale the render")
            }
            image = scaled
        }
        // This render's reader is a vision model whose decode is naive sRGB,
        // so a wide-gamut document is CONVERTED rather than merely tagged
        // (Bitmap.sRGBCopy, which returns an already-sRGB image untouched so
        // the common case keeps its exact bytes).
        let converted = !CFEqual(space, ColorProfile.sRGB)
        guard let tagged = image.makeCGImage(in: space),
            let cgImage = Bitmap.sRGBCopy(of: tagged),
            let png = NSBitmapImageRep(cgImage: cgImage)
                .representation(using: .png, properties: [:])
        else {
            throw ToolError(message: "PNG encoding failed")
        }
        what += " of \(document.displayName ?? "Untitled")"
        var text =
            "\(what): full size \(fullWidth)×\(fullHeight) px, rendered at "
            + "\(image.width)×\(image.height) px"
        if let scale { text += ", scale \(scale)× nearest-neighbour" }
        if let region {
            text += ", region \(region.x),\(region.y) \(region.width)×\(region.height) "
                + "at divisor \(region.previewDivisor)"
            if region.previewDivisor > 1 { text += " (preview)" }
        }
        text += spotNote
        if converted { text += ", sRGB (document profile: \(doc.profileName))" }
        return try callResult(
            content: [
                ["type": "image", "data": png.base64EncodedString(), "mimeType": "image/png"],
                ["type": "text", "text": text],
            ], isError: false)
    }

    /// `render`'s optional `region` / `preview_divisor` (Phase 13A-1), or nil
    /// when neither is present. Either one selects the regional path: the
    /// region defaults to the whole canvas and the divisor to 1.
    static func regionArgument(_ a: [String: Any], doc: RasterDocument) throws -> RegionRenderRequest? {
        guard a["region"] != nil || a["preview_divisor"] != nil else { return nil }
        func integer(_ value: Any?, _ name: String) throws -> Int {
            if let number = value as? NSNumber, number.doubleValue == number.doubleValue.rounded(),
               let exact = Int(exactly: number.doubleValue) {
                return exact
            }
            if let text = value as? String, let parsed = Int(text) { return parsed }
            throw ToolError(message: "\(name) must be an integer")
        }
        var request = RegionRenderRequest(x: 0, y: 0, width: doc.width, height: doc.height)
        if let raw = a["region"] {
            guard let region = raw as? [String: Any] else {
                throw ToolError(message: "region must be an object {x, y, width, height}")
            }
            request.x = try integer(region["x"], "region.x")
            request.y = try integer(region["y"], "region.y")
            request.width = try integer(region["width"], "region.width")
            request.height = try integer(region["height"], "region.height")
        }
        if let raw = a["preview_divisor"] {
            request.previewDivisor = try integer(raw, "preview_divisor")
            guard request.previewDivisor >= 1 else {
                throw ToolError(message: "preview_divisor must be at least 1")
            }
        }
        return request
    }

    /// sample_color: the eyedropper. Reads one pixel of the flattened
    /// composite — the same projection render shows and the magic wand
    /// samples. Read-only: no undo step, no change counting, no document
    /// mutation. Runs on the main thread like every other tool (the
    /// trampoline hops there before dispatch).
    private func sampleColor(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let x = intArg(a, "x"), let y = intArg(a, "y") else {
            throw ToolError(message: "sample_color requires x and y")
        }
        guard x >= 0, y >= 0, x < doc.width, y < doc.height else {
            throw ToolError(
                message: "Point (\(x), \(y)) is outside the canvas "
                    + "(\(doc.width)×\(doc.height), origin top-left)")
        }
        let projection = try document.byteComposite(of: doc)
        guard let rgba = projection.pixelRGBA(x: x, y: y)
        else {
            throw ToolError(message: "Could not sample the composite")
        }
        // The numbers are the DOCUMENT's, in its own space. A hex string
        // carries no space and a colour ARGUMENT is authored sRGB
        // (`parseColor`) unless its tool takes `color_space` — brush_stroke,
        // fill, gradient, fill_selection, stroke_selection and add_text
        // (`DocumentPrecision.colorSpaceGate`), where `color_space:
        // "document"` passes `hex` back exactly. Everywhere else `hex` is a
        // reading: `paint_hex` is the CLOSEST sRGB spelling (ColorProfile
        // .paintBytes), identical to `hex` on an sRGB document. A pixel
        // outside the sRGB gamut has no sRGB spelling at all, and that is
        // reported rather than left silent: otherwise "sample this and
        // paint it over there" quietly paints a different colour.
        let paint = ColorProfile.paintBytes(rgba, in: doc.nsColorSpace)
        var result: [String: Any] = [
            "x": x, "y": y,
            "r": Int(rgba.r), "g": Int(rgba.g), "b": Int(rgba.b), "a": Int(rgba.a),
            "hex": RasterImage.hexString(rgba),
            "paint_hex": RasterImage.hexString(
                (r: paint.bytes[0], g: paint.bytes[1], b: paint.bytes[2], a: paint.bytes[3])),
            "paint_hex_exact": paint.exact,
            "space": doc.profileName,
        ]
        if !paint.exact {
            result["note"] =
                "This pixel is outside the sRGB gamut, so no sRGB hex names it: "
                + "painting paint_hex back gives the closest sRGB colour, which is "
                + "duller than what was sampled."
        }
        let components = [rgba.r, rgba.g, rgba.b, rgba.a].map { Double($0) / 255 }
        if let reading = SRGBReading(documentComponents: components, in: doc.nsColorSpace) {
            result.merge(reading.fields) { _, new in new }
            if !reading.inGamut {
                result["note"] = [result["note"] as? String, SRGBReading.outOfGamutNote]
                    .compactMap { $0 }.joined(separator: " ")
            }
        }
        return try jsonResult(result)
    }

    // MARK: - Layer operations

    /// Runs `transform` and registers it through applyEdit inside an
    /// explicit undo group, then force-closes any implicit event group
    /// NSUndoManager wrapped around it. Off the event path (these calls
    /// arrive via dispatch-to-main, not a real UI event) the implicit
    /// group would otherwise stay open across tool calls, merging every
    /// agent edit into one undo step. The transform and applyEdit's
    /// preflight run first, so a failed or refused edit never opens a group.
    func performGroupedEdit(
        _ document: ImageDocument, _ actionName: String, changesCanvasGeometry: Bool = false,
        allowWarpChange: Bool = false, renderNeutral: Bool = false,
        resultingSelection: SelectionHistoryState? = nil,
        _ transform: (RasterDocument) throws -> RasterDocument?
    ) throws {
        guard let current = document.doc else { throw ToolError(message: "No image loaded") }
        let revision = document.revision
        let state = document.captureEditingState()
        let result: RasterDocument?
        do { result = try transform(current) } catch {
            throw error as? ToolError ?? ToolError(message: error.localizedDescription)
        }
        guard let updated = result else {
            throw ToolError(message: "\(actionName) failed — check the parameters")
        }
        guard document.doc === current, document.revision == revision, !document.isLiveEditing,
              document.isCurrentEditingState(state) else {
            throw ToolError(message: "The document or edit target changed while \(actionName) was running. No result was applied.")
        }
        // applyEdit's refusals (linear-light, warp, depth, 16-bit display
        // preparation — ImageDocument+EditPreflight.swift), up front and BY
        // NAME, before the group opens: refused inside it they would leave an
        // empty undo step and the model would get no sentence.
        do {
            try document.preflightPublication(updated, record: [], allowWarpChange: allowWarpChange,
                                              renderNeutral: renderNeutral)
        } catch { throw ToolError(message: error.localizedDescription) }
        // The explicit group AND its flush, hoisted into withUndoGroup
        // (UndoGrouping.swift) so the drain is LEVEL-RELATIVE. Off the event
        // path — which is every call that arrives through a trampoline's
        // dispatch-to-main — the base is 0 and this is byte for byte the
        // absolute drain it replaces. Called from inside a real AppKit event,
        // as ActionPlayer does, it leaves the event's own implicit group for
        // AppKit to close, and nests cleanly inside the player's group so a
        // whole action collapses to one undo entry.
        withUndoGroup(document.undoManager) {
            document.applyEdit(actionName, record: .notACommand,
                               changesCanvasGeometry: changesCanvasGeometry,
                               allowWarpChange: allowWarpChange,
                               renderNeutral: renderNeutral,
                               resultingSelection: resultingSelection) { _ in updated }
        }
        guard document.doc === updated else {
            throw ToolError(message: "The document or edit target changed while \(actionName) was being prepared. No result was applied.")
        }
    }

    /// performGroupedEdit for an edit that REWRITES A LAYER'S PIXELS — a brush or
    /// eraser stroke on the layer itself, a fill, a gradient, add_text, a
    /// filter, an adjustment, or a transform that could not compose into the
    /// layer's description. Those pixels stop being the rendering of a
    /// described layer's description, so the description is DROPPED inside
    /// the same edit (one undo step, one handle), and the caller tells the
    /// model.
    ///
    /// The UI asks the user first (ImageDocument.applyRasterizingEdit), but
    /// the agent must never: a modal alert on this dispatched-to-main path
    /// would block the main thread — and with it the MCP connection — until
    /// somebody clicked it. Rasterizing silently and REPORTING it is the
    /// recovery-oriented equivalent: undo restores the text layer.
    ///
    /// `pixelLayer` is the layer whose own pixels the edit rewrites, or nil
    /// when it writes somewhere else (a layer MASK), which leaves a
    /// description valid. Returns the kind of description that was dropped.
    ///
    /// `lockKind` is which lock that rewrite answers to, and it is a
    /// parameter for exactly one reason: a TRANSFORM rewrites a layer's
    /// pixels — so it drops a description and belongs here — but it is a
    /// POSITION edit, never a Pixels one (`doc_lock.rs`: it resamples the
    /// whole buffer including its alpha, so a frozen alpha channel has no
    /// meaning). Defaulting it to `DR_EDIT_PIXELS` keeps every other caller
    /// exactly as it was; `transform_layer` and `distort_layer` pass
    /// `DR_EDIT_POSITION` so a PIXELS-locked layer is transformable over MCP
    /// the way it is in the app.
    @discardableResult
    func performPixelEdit(
        _ document: ImageDocument, _ actionName: String, pixelLayer: Int?,
        lockKind: DrEditKind = DR_EDIT_PIXELS, rasterizesTarget: Bool = true,
        _ transform: (RasterDocument) -> RasterDocument?
    ) throws -> DroppedDescription? {
        if rasterizesTarget, let layer = pixelLayer, lockKind == DR_EDIT_PIXELS, document.doc?.hasRetainedSource(layer) == true {
            throw ToolError(message: RetainedImagePolicy.rasterizeFirst)
        }
        // Locks, in the ONE place every pixel edit passes through:
        // `pixelLayer` is non-nil exactly when the edit rewrites THAT
        // layer's own pixels (its planes included) and nil when it writes a
        // mask or a channel — which is precisely what the Pixels lock is
        // about, and precisely what it must not block. Transparency never
        // blocks a pixel edit: the core restores the layer's alpha
        // afterwards instead (AgentServer+Groups.rejectLockedEdit).
        if let layer = pixelLayer { try rejectLockedEdit(document, layer, lockKind) }
        let dropped = rasterizesTarget ? pixelLayer.flatMap { Self.description(of: document, layer: $0) } : nil
        do {
            try performGroupedEdit(document, actionName) { doc in
                guard let updated = transform(doc) else { return nil }
                guard dropped != nil, let layer = pixelLayer else { return updated }
                return updated.withLayerMeta(layer, nil) ?? updated
            }
        } catch let error as ToolError {
            // The one refusal the generic message leaves unexplained: a
            // frozen alpha channel makes some edits no-ops BY DESIGN
            // (AgentServer+Groups.frozenAlphaRefusal).
            throw frozenAlphaRefusal(document, pixelLayer, lockKind, actionName) ?? error
        }
        return dropped
    }

    /// The kind of description layer `idx` carries — by the `type` its meta
    /// claims, whatever the payload version, so an undecodable description
    /// is dropped rather than left stale — or nil for plain pixels (the
    /// app-side mirror of ImageDocument.layerDescribesSource).
    static func description(
        of document: ImageDocument, layer idx: Int
    ) -> DroppedDescription? {
        guard let doc = document.doc else { return nil }
        return LayerDescription.claimedKind(of: doc.layerMeta(idx)).map(DroppedDescription.init)
    }

    /// A pixel-edit result, with the rasterization report appended when the
    /// edit dropped a layer's description; `reason` says WHY this edit had
    /// to rasterize when that is not obvious (a transform on a text layer
    /// whose family is not installed here).
    ///
    /// `alsoRasterized` names the OTHER entries of a set edit that lost a
    /// description — a transformed group's children, a link partner
    /// (AgentServer+Distort.appendSetRasterization).
    func pixelEditResult(
        _ fields: [String: Any], layer: Int, rasterized: DroppedDescription?,
        reason: String? = nil, alsoRasterized: [(layer: Int, kind: DescribedKind)] = []
    ) throws -> String {
        var result = fields
        var note: String?
        switch rasterized {
        case .text:
            result["rasterized_text"] = true
            note =
                "This edit painted over layer \(layer)'s pixels, so the layer is no longer "
                + "editable as text: the description it was rendered from (its text, "
                + "typography and transform) was dropped and edit_text_layer no longer works "
                + "on it. The pixels are intact; undo restores the text layer."
        case .shape:
            result["rasterized_shape"] = true
            note =
                "This edit painted over layer \(layer)'s pixels, so the layer is no longer "
                + "editable as a shape: the description it was rendered from (its kind, box, "
                + "styling and transform) was dropped. The pixels are intact; undo restores "
                + "the shape layer."
        case .fill:
            result["rasterized_fill"] = true
            note =
                "This edit painted over layer \(layer)'s pixels, so the layer is no longer "
                + "a fill layer: the description it was rendered from (its color, gradient "
                + "or pattern) was dropped and edit_fill_layer no longer works on it. The "
                + "pixels are intact; undo restores the fill layer."
        case nil:
            break
        }
        if let note = note {
            result["note"] = reason.map { note + " " + $0 } ?? note
        }
        Self.appendSetRasterization(&result, alsoRasterized)
        return try jsonResult(result)
    }

    private func docEdit(
        _ a: [String: Any], _ actionName: String, changesCanvasGeometry: Bool = false,
        resultingSelection: SelectionHistoryState? = nil,
        _ transform: @escaping (RasterDocument) throws -> RasterDocument?
    ) throws -> String {
        let document = try target(a)
        try performGroupedEdit(document, actionName, changesCanvasGeometry: changesCanvasGeometry,
                               resultingSelection: resultingSelection, transform)
        return try jsonResult(["ok": true, "document": summary(document)])
    }

    private func setActiveLayer(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let index = intArg(a, "index") else {
            throw ToolError(message: "set_active_layer requires index")
        }
        let count = document.doc?.layerCount ?? 0
        guard index >= 0, index < count else {
            throw ToolError(message: "Layer \(index) is out of range (0..\(count - 1))")
        }
        document.activeLayerIndex = index
        NotificationCenter.default.post(
            name: .imageDocumentImageDidChange, object: document, userInfo: ["isLive": false])
        // Read back rather than echoed: an entry inside a COLLAPSED group has
        // no row of its own, and the panel re-points the selection at the row
        // that stands for it (LayersPanelViewController.rebuildRows), so the
        // group is what every command will act on. Reporting the index asked
        // for would describe a state the document is not in.
        return try jsonResult(["ok": true, "active_layer": document.activeLayerIndex])
    }

    private func newLayer(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let name = stringArg(a, "name") ?? "Layer \(doc.layerCount + 1)"
        // `position` / `layer` (AgentServer+Adjustments.insertionArguments),
        // the same pair every layer-creating tool takes; the core owns the
        // slot rule and `landingIndex` is the "select what I just made" answer.
        let placement = try insertionArguments(a, document: document)
        let landing = placement.position.landingIndex(in: doc, anchor: placement.anchor)
        guard try editStructure(document, "New Layer", {
            try $0.addingLayer(anchor: placement.anchor, position: placement.position, name: name)
        }) else { throw ToolError(message: "New Layer failed — check the parameters") }
        document.activeLayerIndex = min(landing, (document.doc?.layerCount ?? 1) - 1)
        return try jsonResult([
            "ok": true, "new_layer_index": document.activeLayerIndex, "name": name,
            "layer": document.activeLayerIndex, "position": placement.position.rawValue,
        ])
    }

    // MARK: - Layer transform

    /// transform_layer's samplers, mapped onto the resize filters the Free
    /// Transform options bar offers ("bicubic" IS Catmull-Rom). image_size's
    /// spellings are accepted too, so one vocabulary covers both tools; the
    /// canonical name is what the result echoes back.
    static let transformSamplers: [String: (name: String, filter: DrResizeFilter)] = [
        "nearest": ("nearest", DR_FILTER_NEAREST),
        "bilinear": ("bilinear", DR_FILTER_BILINEAR),
        "bicubic": ("bicubic", DR_FILTER_CATMULL_ROM),
        "catmull-rom": ("bicubic", DR_FILTER_CATMULL_ROM),
        "lanczos": ("lanczos", DR_FILTER_LANCZOS3),
        "lanczos3": ("lanczos", DR_FILTER_LANCZOS3),
    ]

    /// The core's ceiling on one layer's pixels (doc.rs MAX_PIXELS), mirrored
    /// so an oversized request is refused with a message that names the cap
    /// instead of a bare NULL from the core. The number itself lives with
    /// the transform model, which shares it with the distort drag clamp.
    static let maxLayerPixels = LayerTransform.maxTransformPixels

    /// A finite number argument; nil when the caller omitted it.
    func finiteArg(_ a: [String: Any], _ key: String) throws -> Double? {
        guard let value = doubleArg(a, key) else { return nil }
        guard value.isFinite else {
            throw ToolError(message: "\(key) must be a finite number")
        }
        return value
    }

    /// A scale multiplier: finite, non-zero (zero is a singular matrix the
    /// core refuses) and clamped to the same magnitudes the Free Transform
    /// fields allow, so agent and UI accept exactly the same range.
    func scaleArg(_ a: [String: Any], _ key: String) throws -> CGFloat? {
        guard let value = try finiteArg(a, key) else { return nil }
        guard value != 0 else {
            throw ToolError(
                message: "\(key) must not be 0 — a zero scale collapses the layer to nothing "
                    + "(a singular matrix the core refuses). 1 keeps the size, 0.5 halves it, "
                    + "-1 mirrors it.")
        }
        return LayerTransform.clampScale(CGFloat(value))
    }

    /// Two decimals, for the parameters the result echoes back — except when
    /// that would round a nonzero value away to 0, which would contradict this
    /// same tool's "a scale of 0 is refused" and misreport a scale clamped to
    /// the 0.001 floor. Anything smaller than half a hundredth keeps two
    /// significant digits instead, so the echo stays tidy but never claims a
    /// value the call would have rejected.
    static func transformNumber(_ value: Double) -> Double {
        guard value.isFinite, value != 0 else { return 0 }
        let rounded = (value * 100).rounded() / 100
        if rounded != 0 { return rounded }
        let unit = pow(10.0, 1 - (log10(abs(value))).rounded(.down))
        let significant = (value * unit).rounded() / unit
        return significant.isFinite && significant != 0 ? significant : value
    }

    /// Rotates / scales / moves ONE layer in a single resample. The named
    /// parameters are compiled into a LayerTransform and its matrix is what
    /// goes to the core — the very composition the interactive Free Transform
    /// session commits, so identical parameters give identical pixels.
    private func transformLayer(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        // A group transforms every descendant with the same matrix.
        let index = try structuralLayerIndex(a, document)
        // A transform is a POSITION edit, never a pixel one (doc_lock.rs):
        // a transparency-locked layer still transforms. The SET version,
        // because the op below is transform_layerS: a locked DESCENDANT or a
        // locked LINK PARTNER refuses the call too, and must be the entry the
        // refusal names.
        try rejectLockedMove(document, [index])
        guard let doc = document.doc, let info = doc.layerInfo(index) else {
            throw ToolError(message: "Layer \(index) could not be read")
        }
        guard info.width > 0, info.height > 0 else {
            throw ToolError(
                message: "Layer \(index) (\"\(info.name)\") has no pixels to transform.")
        }
        // The layer's rect in CANVAS space — what the matrix maps.
        let rect = CGRect(
            x: CGFloat(info.offsetX), y: CGFloat(info.offsetY),
            width: CGFloat(info.width), height: CGFloat(info.height))

        let samplerName = (stringArg(a, "sampler") ?? "bicubic").lowercased()
        guard let sampler = Self.transformSamplers[samplerName] else {
            throw ToolError(
                message: "sampler must be nearest, bilinear, bicubic or lanczos "
                    + "(got \"\(samplerName)\")")
        }

        // The pivot: a named corner of the layer's CURRENT bounds, overridden
        // per axis by an explicit canvas coordinate. A described layer's
        // centre is its description's exact centre — the UI session's too
        // (EditorViewController+DescribedTransform.swift) — so a rotate and
        // a rotate-back share the pivot and return the anchor exactly.
        let anchor: CGPoint
        switch stringArg(a, "around") ?? "center" {
        case "center": anchor = doc.describedPivot(index) ?? CGPoint(x: rect.midX, y: rect.midY)
        case "top_left": anchor = CGPoint(x: rect.minX, y: rect.minY)
        case let other:
            throw ToolError(
                message: "around must be \"center\" or \"top_left\" (got \"\(other)\"); pass "
                    + "pivot_x / pivot_y for any other point.")
        }
        let pivot = CGPoint(
            x: CGFloat(try finiteArg(a, "pivot_x") ?? Double(anchor.x)),
            y: CGFloat(try finiteArg(a, "pivot_y") ?? Double(anchor.y)))

        let rotate = try finiteArg(a, "rotate")
        let uniformScale = try scaleArg(a, "scale")
        let scaleX = try scaleArg(a, "scale_x")
        let scaleY = try scaleArg(a, "scale_y")
        let translateX = try finiteArg(a, "translate_x")
        let translateY = try finiteArg(a, "translate_y")
        guard rotate != nil || uniformScale != nil || scaleX != nil || scaleY != nil
            || translateX != nil || translateY != nil
        else {
            throw ToolError(
                message: "transform_layer: nothing to change — pass at least one of rotate, "
                    + "scale, scale_x, scale_y, translate_x, translate_y.")
        }

        var transform = LayerTransform(pivot: pivot)
        transform.translation = CGVector(
            dx: CGFloat(translateX ?? 0), dy: CGFloat(translateY ?? 0))
        // Positive degrees turn CLOCKWISE on the flipped canvas, the same
        // sign the options bar's Angle field writes.
        if let rotate = rotate { transform.degrees = rotate }
        transform.scaleX = scaleX ?? uniformScale ?? 1
        transform.scaleY = scaleY ?? uniformScale ?? 1
        guard transform.isFinite else {
            throw ToolError(
                message: "Those parameters do not compose a usable transform — every value "
                    + "must be a finite number.")
        }
        guard !transform.isIdentity else {
            throw ToolError(
                message: "Those parameters leave the layer exactly where it is (a rotation "
                    + "that is a multiple of 360°, scales of 1, no translation), so there is "
                    + "nothing to transform.")
        }
        let matrix = transform.matrix

        // The core derives its destination extent exactly this way, so the
        // cases it would answer with NULL can be named precisely here.
        let extent = matrix.destinationExtent(of: rect)
        let int32Min = CGFloat(Int32.min)
        let int32Max = CGFloat(Int32.max)
        guard extent.width >= 1, extent.height >= 1,
            extent.minX >= int32Min, extent.minY >= int32Min,
            extent.maxX <= int32Max, extent.maxY <= int32Max
        else {
            throw ToolError(
                message: "The transformed layer would collapse to nothing or land outside the "
                    + "coordinates the core can address. Use a scale away from 0 and a "
                    + "translation that keeps the layer near the canvas.")
        }
        guard Double(extent.width) * Double(extent.height) <= Self.maxLayerPixels else {
            throw ToolError(
                message: "The transformed layer would be \(Int(extent.width))×"
                    + "\(Int(extent.height)) px, past the core's 100 megapixel ceiling for one "
                    + "layer (it is \(info.width)×\(info.height) px now). Scale down.")
        }
        guard abs(matrix.a * matrix.d - matrix.b * matrix.c) >= 1e-9 else {
            throw ToolError(
                message: "The composed matrix is degenerate (its determinant is ~0, so the "
                    + "layer collapses to a line): a scale of 0 or very near it. Use scales "
                    + "away from 0.")
        }

        let applied: [String: Any] = [
            "rotate": Self.transformNumber(transform.degrees),
            "scale_x": Self.transformNumber(Double(transform.scaleX)),
            "scale_y": Self.transformNumber(Double(transform.scaleY)),
            "translate_x": Self.transformNumber(translateX ?? 0),
            "translate_y": Self.transformNumber(translateY ?? 0),
            "pivot_x": Self.transformNumber(Double(pivot.x)),
            "pivot_y": Self.transformNumber(Double(pivot.y)),
            "sampler": sampler.name,
        ]
        // A described layer composes the matrix into its description and
        // re-renders (AgentServer+Distort.swift); nil means it did not (no
        // description, its source cannot render, or the composition was
        // refused) and the rasterizing path below applies and reports as
        // before.
        // …and only for a LONE entry. A matrix composes into ONE description,
        // so a layer that transforms as a SET — a group, or a layer with
        // LINKED partners — has to resample instead, exactly as the UI's ⌘T
        // does over a multi-entry session (MultiLayerEdit.swift). Without
        // this, a linked text layer would compose its own transform and its
        // partners would stay behind.
        if doc.movingSet([index]).count == 1,
           let composed = try transformDescribedLayer(
               document, layer: index, matrix: matrix, sampler: sampler, applied: applied) {
            return composed
        }
        // performPixelEdit is the pixel-rewrite chokepoint: a transform that
        // did not compose resamples the pixels a described layer was
        // rendered from, so the description is dropped in the SAME edit (one
        // undo step) and reported back — the agent must never raise the UI's
        // modal prompt.
        // Every OTHER entry the set transform resamples — a group's
        // descendants, a link partner — loses its description in the SAME
        // edit, exactly as the UI's TransformWork does: the
        // core preserves `meta` while it replaces the pixels, so without this
        // a text layer inside a transformed group would keep a description
        // that no longer matches its own picture.
        let partners = doc.movingSet([index]).filter { $0 != index }
        let describedPartners = doc.describedEntries(in: partners)
        let rasterized: DroppedDescription?
        rasterized = try performTransformJob(
            document, "Transform Layer", layers: [index], pixelLayer: index,
            matrix: matrix, sampler: sampler.filter)

        let after = document.doc?.layerInfo(index)
        let alsoRasterized = describedPartners.filter {
            document.doc?.layerMeta($0.layer) == nil
        }
        return try pixelEditResult(
            [
                "ok": true,
                "layer": index,
                // Where the layer landed: the outward-rounded bounding box of
                // the transformed corners, so a render can be checked against it.
                "bounds": [
                    "x": after?.offsetX ?? 0, "y": after?.offsetY ?? 0,
                    "width": after?.width ?? 0, "height": after?.height ?? 0,
                ],
                "applied": applied,
            ], layer: index, rasterized: rasterized,
            reason: Self.unrenderableReason(document, layer: index, before: doc),
            alsoRasterized: alsoRasterized)
    }

    // MARK: - Filters and geometry

    private func applyFilter(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let a = try developFilterMaskArguments(lensProfileArguments(a, document), document)
        try refuseUnknownFilterArguments(a)
        if let blurred = try quickMaskBlurFilter(a, document) { return blurred }
        // Structural, then the group guard once the target is known below —
        // the same shape the strokes use, and the reason apply_filter no
        // longer answers a group index with the generic "check the
        // parameters" (the core's layerImage is NULL for a group, so the
        // whole transform returned nil).
        let index = try structuralLayerIndex(a, document)
        let filter = try requiredString(a, "filter")
        // Validate an adjustment op's params ONCE, here, where we can still
        // throw: `filtered` is non-throwing and is also called from the
        // plane path's non-throwing closure, so a `try?` down there would
        // swallow the message and report a generic failure instead of
        // naming the key. nil for every legacy filter, whose flat arguments
        // are untouched — and which refuse a stray `params` in there rather
        // than dropping it (AgentServer+Adjustments.swift).
        let adjustParams = try adjustmentFilterParams(a, filter: filter, documentProfile: document.doc?.iccProfile)
        let editTarget = try paintTarget(a, document)
        try refuseChannelStagesOnScalarTarget(filter, adjustParams, target: editTarget)
        if document.doc.isHighDepth {
            let params = adjustParams ?? (Self.flatFilterArguments[filter] ?? []).reduce(into: [String: Any]()) { if let value = a[$1] { $0[$1] = value } }
            return try selectedPrecisionFilter(document, filter: filter, params: params, index: index, target: editTarget)
        }
        if let framed = try framedByteFilter(document, a, filter: filter, layer: index, target: editTarget) { return framed }
        if let inverted = try invertMaskFilter(document, filter: filter, layer: index, target: editTarget) { return inverted }
        return try applyScopedFilter(
            document, target: editTarget, layer: index, filter: filter
        ) { image in
            if let params = adjustParams {
                return document.doc?.applyingAdjustment(to: image, op: filter, params: params)
            }
            return try self.filtered(image, filter, a, adjustParams, in: document)
        }
    }

    private func filtered(
        _ image: RasterImage, _ filter: String, _ a: [String: Any],
        _ adjustParams: [String: Any]?, in document: ImageDocument
    ) throws -> RasterImage? {
        // The seven dr_image_filter_op filters, planned and admitted.
        if let request = byteFilterRequest(filter, a) { return try byteFiltered(image, request, in: document) }
        switch filter {
        case "curves", "exposure", "vibrance", "hue_saturation", "color_balance",
            "black_and_white", "photo_filter", "channel_mixer", "selective_color",
            "shadows_highlights", "white_balance", "gradient_map", "color_lookup":
            // The destructive twin of the adjustment layer of the same
            // name: the SAME core op (dr_image_adjust_op), parameters
            // already validated in applyFilter above.
            return adjustmentFilter(image, filter, adjustParams ?? [:])
        case "grayscale": return image.grayscaled()
        case "invert": return image.inverted()
        case "sepia": return image.sepia()
        case "edge_detect": return image.edgeDetected()
        case "emboss": return image.embossed()
        case "blur":
            // The core refuses a sigma above 1000 ("sigma must be finite in
            // (0,1000]", sampling_gaussian_fast::MAX_SIGMA) rather than run a
            // smaller one; name it here, with the value.
            let sigma = doubleArg(a, "sigma") ?? 4
            guard sigma > 0, sigma <= 1000 else {
                throw ToolError(message: "blur sigma must be above 0 and at most 1000 (got \(sigma))")
            }
            return try byteFiltered(image, ByteFilterRequest(op: "blur", params: ["sigma": sigma]), in: document)
        case "sharpen": return image.sharpened(amount: doubleArg(a, "amount") ?? 1)
        case "adjust":
            // `legacy: false` is the modern curve, through the one
            // adjustment twin; absent or true keeps the frozen export.
            if a["legacy"] as? Bool == false {
                return document.doc?.applyingAdjustment(to: image, op: "bcs", params: [
                    "brightness": doubleArg(a, "brightness") ?? 0,
                    "contrast": doubleArg(a, "contrast") ?? 0,
                    "saturation": doubleArg(a, "saturation") ?? 0, "legacy": false])
            }
            return image.adjusted(
                brightness: doubleArg(a, "brightness") ?? 0,
                contrast: doubleArg(a, "contrast") ?? 0,
                saturation: doubleArg(a, "saturation") ?? 0)
        case "hue_rotate": return image.hueRotated(degrees: doubleArg(a, "degrees") ?? 0)
        case "threshold": return image.thresholded(level: doubleArg(a, "level") ?? 0.5)
        case "posterize": return image.posterized(levels: intArg(a, "levels") ?? 4)
        case "pixelate": return image.pixelated(block: intArg(a, "block") ?? 8)
        case "add_noise":
            return image.noised(
                amount: doubleArg(a, "amount") ?? 0.2,
                seed: UInt64(bitPattern: Int64(intArg(a, "seed") ?? 1)))
        default: return nil
        }
    }

    // MARK: - Adjustment layers

    /// Refuses an edit aimed at an ADJUSTMENT layer's pixels — the compositor
    /// ignores them (the layer recolors its backdrop instead), so the edit
    /// would silently change nothing. The agent mirror of the UI's
    /// refuseAdjustmentPixelEdit alert, word for word. Mask, move, transform,
    /// property and stacking tools deliberately do NOT come through here.
    /// Internal, not private: the +Feature pixel-edit handlers refuse
    /// through it too.
    func rejectAdjustmentPixelEdit(_ document: ImageDocument, _ index: Int) throws {
        guard document.doc?.layerIsAdjustment(index) == true else { return }
        throw ToolError(
            message: "Adjustment layers have no pixels to edit. Paint on the layer's mask "
                + "instead, or target another layer.")
    }

    /// The `op` argument of the adjustment tools, as one of the twenty-one
    /// ops the core's schema (core/src/adjust.rs) interprets.
    private func adjustmentOp(_ name: String) throws -> AdjustmentLayerOp {
        guard let op = AdjustmentLayerOp(rawValue: name) else {
            let names = AdjustmentLayerOp.allCases.map { $0.rawValue }.joined(separator: ", ")
            throw ToolError(message: "Unknown adjustment op \"\(name)\". One of: \(names)")
        }
        return op
    }

    /// The `params` argument of the adjustment tools ([:] when omitted),
    /// validated against the core's meta schema for `op` — what passes here
    /// is guaranteed to parse in the core, so the app never stores meta the
    /// compositor would silently treat as plain raster. Deliberately STRICTER
    /// than the core, which clamps some out-of-range numbers and ignores
    /// unknown keys: silently bending a model's values (or dropping a typo'd
    /// key, leaving an accidental identity adjustment) hides mistakes, so
    /// every violation is refused naming the offending key and its range.
    func adjustmentParams(
        _ a: [String: Any], for op: AdjustmentLayerOp, documentProfile: Data? = nil
    ) throws -> [String: Any] {
        let params: [String: Any]
        if let raw = a["params"] {
            guard let object = raw as? [String: Any] else {
                throw ToolError(
                    message: "params must be a JSON object, e.g. {\"brightness\": 0.2}")
            }
            params = object
        } else {
            params = [:]
        }
        if let ink = try CmykAdjustFilter.validated(params, for: op, in: try? target(a)) { return ink }
        let allowed: [String]
        switch op {
        case .bcs: allowed = ["brightness", "contrast", "saturation", "legacy"]
        case .curves: allowed = CurvesMeta.keys
        case .levels: allowed = LevelsMeta.keys
        case .hueRotate: allowed = ["degrees"]
        case .threshold: allowed = ["level"]
        case .posterize: allowed = ["levels"]
        case .invert, .grayscale, .sepia: allowed = []
        case .exposure, .vibrance, .hueSaturation, .colorBalance, .blackAndWhite,
            .photoFilter, .channelMixer, .selectiveColor, .shadowsHighlights,
            .whiteBalance, .gradientMap, .colorLookup, .replaceColor:
            allowed = AdjustmentSchema.inputKeys(for: op)
        }
        let unknown = params.keys.filter { !allowed.contains($0) }.sorted()
        if let first = unknown.first {
            guard !allowed.isEmpty else {
                throw ToolError(
                    message: "\(op.rawValue) takes no params "
                        + "(got \(unknown.joined(separator: ", ")))")
            }
            throw ToolError(
                message: "Unknown \(op.rawValue) parameter \"\(first)\" — \(op.rawValue) "
                    + "takes \(allowed.joined(separator: ", ")).")
        }
        // A present value must be a JSON number (finite, inside `range`);
        // nil when the key was omitted, so callers can apply defaults.
        func number(_ key: String, range: ClosedRange<Double>? = nil) throws -> Double? {
            guard let value = params[key] else { return nil }
            guard let n = (value as? NSNumber)?.doubleValue, n.isFinite else {
                throw ToolError(message: "\(key) must be a number")
            }
            if let range = range, !range.contains(n) {
                throw ToolError(
                    message: "\(key) must be between \(range.lowerBound) and "
                        + "\(range.upperBound) (got \(n))")
            }
            return n
        }
        switch op {
        case .bcs:
            for key in ["brightness", "contrast", "saturation"] {
                _ = try number(key, range: -1...1)
            }
            // `legacy` false is the modern curve; absent is legacy.
            if let legacy = params["legacy"], !Self.isJSONBoolean(legacy) {
                throw ToolError(message: "legacy must be true or false")
            }
        case .levels:
            return try validatedLevelsParams(params) // AgentServer+Levels.swift
        case .hueRotate:
            _ = try number("degrees")
        case .threshold:
            _ = try number("level", range: 0...1)
        case .posterize:
            guard let levels = try number("levels") else {
                throw ToolError(
                    message: "posterize requires levels, an integer from 2 to 64")
            }
            guard levels == levels.rounded() else {
                throw ToolError(
                    message: "levels must be an integer from 2 to 64 (got \(levels))")
            }
            guard levels >= 2, levels <= 64 else {
                throw ToolError(
                    message: "levels must be between 2 and 64 (got \(Int(levels)))")
            }
        case .curves:
            return try validatedCurvesParams(params) // AgentServer+Curves.swift
        case .invert, .grayscale, .sepia:
            break // no params; the unknown-key check above already refused any
        case .exposure, .vibrance, .hueSaturation, .colorBalance, .blackAndWhite,
            .photoFilter, .channelMixer, .selectiveColor, .shadowsHighlights,
            .whiteBalance, .gradientMap, .colorLookup, .replaceColor:
            // The phase-5 ops share ONE table-driven validator
            // (AdjustmentSchema), which returns rather than falls through:
            // color_lookup's `file` expansion REPLACES the params object.
            return try AdjustmentSchema.validate(params, for: op, documentProfile: documentProfile)
        }
        return params
    }

    /// add_adjustment_layer: the agent mirror of Layer > New Adjustment
    /// Layer — the very chain the UI commits
    /// (RasterDocument.addingAdjustmentLayer): a canvas-sized transparent
    /// layer above the ACTIVE layer, the validated meta attached, and always
    /// a mask — from the live selection when one exists (marquee left up,
    /// exactly like Layer > Mask > From Selection) else reveal-all. One
    /// handle committed through performGroupedEdit = one undo step; the new layer
    /// becomes active, like the UI's post-commit selection.
    private func addAdjustmentLayer(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard document.doc != nil else { throw ToolError(message: "Document has no image") }
        let op = try adjustmentOp(requiredString(a, "op"))
        let params = try adjustmentParams(a, for: op, documentProfile: document.doc?.iccProfile)
        guard let meta = AdjustmentLayerPayload(op: op, params: params).json() else {
            throw ToolError(message: "Could not encode the adjustment parameters")
        }
        let name = stringArg(a, "name") ?? op.displayName
        // `position` / `layer` (AgentServer+Adjustments.insertionArguments),
        // passed to BOTH depths: the core owns the slot rule.
        let placement = try insertionArguments(a, document: document)
        if document.doc.isHighDepth {
            return try precisionAdjustment(document, op: op, params: params, index: nil,
                destructive: false, name: name, anchor: placement.anchor,
                position: placement.position)
        }
        let below = placement.anchor
        let selection = selectionMask(document)
        let landing = document.doc.map { placement.position.landingIndex(in: $0, anchor: below) }
            ?? below + 1
        try performGroupedEdit(document, "New \(op.displayName) Layer") {
            $0.addingAdjustmentLayer(anchor: below, position: placement.position, name: name,
                                     meta: meta, selection: selection)
        }
        let index = min(landing, (document.doc?.layerCount ?? 1) - 1)
        document.activeLayerIndex = index
        // The edit's own notification went out before the active layer
        // moved; this one lands the UI's paint target on the new mask.
        NotificationCenter.default.post(
            name: .imageDocumentImageDidChange, object: document, userInfo: ["isLive": false])
        return try jsonResult([
            "ok": true,
            "layer": index,
            "name": name,
            "op": op.rawValue,
            "position": placement.position.rawValue,
            "params": elidedAdjustmentParams(AdjustmentLayerPayload(op: op, params: params)),
            "mask": selection != nil ? "from_selection" : "reveal_all",
            "note": "Non-destructive: the layer recolors everything below it, gated by its "
                + "mask. Change its parameters with edit_adjustment_layer; brush/eraser "
                + "strokes on it paint the mask.",
        ])
    }

    /// edit_adjustment_layer: replaces ONLY an adjustment layer's meta — the
    /// agent mirror of the Adjustment Options sheet's Apply. params is a
    /// WHOLESALE replacement (validated like add_adjustment_layer's); op
    /// alone switches the op to its defaults (refused by validation when the
    /// op requires a parameter, i.e. posterize). Pixels, mask, properties
    /// and stacking are untouched; one undo step.
    private func editAdjustmentLayer(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let index = try paintLayerIndex(a, document)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard doc.layerIsAdjustment(index) else {
            let name = doc.layerInfo(index)?.name ?? ""
            throw ToolError(
                message: "Layer \(index) (\"\(name)\") is not an adjustment layer "
                    + "(get_document reports is_adjustment on the layers that are). Make one "
                    + "with add_adjustment_layer.")
        }
        let opArg = stringArg(a, "op")
        guard opArg != nil || a["params"] != nil else {
            throw ToolError(
                message: "edit_adjustment_layer: nothing to change — pass op and/or params.")
        }
        let op: AdjustmentLayerOp
        if let opArg = opArg {
            op = try adjustmentOp(opArg)
        } else {
            let current = doc.adjustmentPayload(index)
            guard let known = current?.knownOp else {
                throw ToolError(
                    message: "Layer \(index)'s adjustment op \"\(current?.op ?? "?")\" is not "
                        + "one this app can edit; pass op to replace it with a known one.")
            }
            op = known
        }
        // The one merge this tool does, and only for color_lookup, whose
        // table get_document has to elide (AgentServer+Adjustments.swift).
        let params = try adjustmentParams(
            mergingStoredLut(a, op: op, current: doc.adjustmentPayload(index)), for: op,
            documentProfile: document.doc?.iccProfile)
        guard let meta = AdjustmentLayerPayload(op: op, params: params).json() else {
            throw ToolError(message: "Could not encode the adjustment parameters")
        }
        if doc.isHighDepth { return try precisionAdjustment(document, op: op, params: params, index: index, destructive: false) }
        try performGroupedEdit(document, "Edit \(op.displayName) Layer") {
            $0.withLayerMeta(index, meta)
        }
        // Through the SAME elision get_document uses: a Color Lookup layer's
        // base64 table is up to ~575 KB, and echoing it back would put in the
        // reply exactly what `elidedAdjustmentParams` exists to keep out.
        return try jsonResult([
            "ok": true, "layer": index, "op": op.rawValue,
            "params": elidedAdjustmentParams(AdjustmentLayerPayload(op: op, params: params)),
        ])
    }

    // MARK: - Painting (brush, eraser, text)

    /// The layer a PIXEL call targets ("layer" arg, default active) — same
    /// convention as apply_filter.
    ///
    /// It refuses a GROUP, because a group has no pixels of its own: putting
    /// the guard in the default helper is fail-safe (a new pixel tool
    /// inherits it) and the handful of tools that legitimately accept a
    /// group take `structuralLayerIndex` instead. The tools whose TARGET
    /// decides — the strokes and apply_filter, where a mask or a channel is a
    /// legitimate target on a group — take `structuralLayerIndex` and call
    /// `rejectGroupPixelEdit` themselves once the target is known.
    func paintLayerIndex(_ a: [String: Any], _ document: ImageDocument) throws -> Int {
        let index = try structuralLayerIndex(a, document)
        try rejectGroupPixelEdit(document, index)
        return index
    }


    /// Builds a canvas-sized premultiplied RGBA8 overlay (row 0 = top,
    /// drawing coordinates top-left-origin — the same format the canvas
    /// view's stroke pipeline uses), lets `draw` fill it, and composites
    /// it onto `layer` through the regular performGroupedEdit path. With a
    /// COVERAGE `target` — the layer's mask, one of its colour planes or a
    /// document channel — the very same overlay is painted there instead
    /// (white reveals, black hides, the overlay's own alpha is the blend —
    /// `mode` and `alpha` do not apply).
    ///
    /// Returns the kind of description painting the layer's pixels dropped
    /// (see performPixelEdit); a MASK or CHANNEL stroke never drops one.
    /// Internal, not private: the byte canonical-dab replay rasterizes through
    /// this same overlay-and-clip path (AgentServer+NativeBrush.byteDabPaint).
    @discardableResult
    func paintOverlay(
        _ document: ImageDocument, layer: Int, actionName: String,
        mode: DrCompositeMode, alpha: Double, target: PaintTarget = .layer,
        blend: (mode: DrBlendMode, rgb: [UInt8])? = nil, onOpRefusal: (() -> Void)? = nil,
        draw: (CGContext) -> Void
    ) throws -> DroppedDescription? {
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        // A MASK stroke rewrites no layer pixel, so `pixelLayer` below is nil
        // and performPixelEdit's lock check never sees it — but Lock All
        // freezes the mask too (doc_lock.rs), and the refusal must say so.
        if target == .mask { try rejectLockedEdit(document, layer, DR_EDIT_MASK) }
        let width = doc.width
        let height = doc.height
        var data = [UInt8](repeating: 0, count: width * height * 4)
        return try performPixelEdit(
            document, actionName, pixelLayer: pixelLayer(for: target, layer: layer)
        ) { current in
            data.withUnsafeMutableBufferPointer { buffer -> RasterDocument? in
                guard let base = buffer.baseAddress,
                    let context = CGContext(
                        data: base, width: width, height: height, bitsPerComponent: 8,
                        bytesPerRow: width * 4, space: doc.drawingSpace,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
                else { return nil }
                context.translateBy(x: 0, y: CGFloat(height))
                context.scaleBy(x: 1, y: -1)
                // Strokes and text confine to the active selection, exactly
                // like the interactive tools.
                if let selection = editor(document)?.agentSelection {
                    selection.clip(context)
                }
                draw(context)
                if target.isCoverage {
                    return commitCoverage(
                        current, target: target, layer: layer, overlay: base, width: width,
                        height: height, onRefusal: onOpRefusal)
                }
                if let blend = blend, blend.mode != DR_BLEND_NORMAL {
                    // The Blend option: composite through the layer
                    // blend-mode set instead of plain source-over. Unlike
                    // the classic op, this one also refuses when the blend
                    // changes no pixel (Multiply by white) — the latch
                    // lets the caller report that as a no-op, not an error.
                    let out = current.paintingLayerBlendColor(
                        layer, overlay: base, w: width, h: height,
                        rgb: blend.rgb, mode: blend.mode, alpha: alpha)
                    if out == nil { onOpRefusal?() }
                    return out
                }
                return current.paintingLayer(
                    layer, overlay: base, w: width, h: height, mode: mode, alpha: alpha)
            }
        }
    }

    // Internal, not private: the +Feature stroke handlers parse the same
    // points argument.
    func parsePoints(_ a: [String: Any]) throws -> [CGPoint] {
        guard let raw = a["points"] as? [Any], !raw.isEmpty else {
            throw ToolError(message: "points must be a non-empty array of [x, y] pairs")
        }
        guard raw.count <= 10_000 else {
            throw ToolError(message: "Too many points (10,000 max)")
        }
        return try raw.map { entry in
            if let pair = entry as? [Any], pair.count == 2,
                let x = NativeBrushStroke.jsonNumber(pair[0]),
                let y = NativeBrushStroke.jsonNumber(pair[1])
            {
                return try strokePoint(x, y)
            }
            if let object = entry as? [String: Any],
                let x = NativeBrushStroke.jsonNumber(object["x"]),
                let y = NativeBrushStroke.jsonNumber(object["y"])
            {
                return try strokePoint(x, y)
            }
            throw ToolError(message: "Each point must be [x, y] or {\"x\": …, \"y\": …}")
        }
    }

    /// One stroke point, validated: finite and within ±100,000 canvas px —
    /// far past any real canvas, but a hard wall against coordinates that
    /// would make the soft-dab walk (one stamp per few px of arc length)
    /// loop effectively forever.
    private func strokePoint(_ x: Double, _ y: Double) throws -> CGPoint {
        guard x.isFinite, y.isFinite, abs(x) <= 100_000, abs(y) <= 100_000 else {
            throw ToolError(
                message: "Stroke points must be finite canvas coordinates within "
                    + "±100,000 px — got (\(x), \(y)). Keep strokes near the canvas.")
        }
        return CGPoint(x: x, y: y)
    }

    /// Hex color: #RGB, #RRGGBB, or #RRGGBBAA ('#' optional).
    func parseColor(_ a: [String: Any], _ key: String, fallback: NSColor) throws
        -> NSColor
    {
        guard var hex = stringArg(a, key)?.trimmingCharacters(in: .whitespaces) else {
            return fallback
        }
        if hex.hasPrefix("#") { hex.removeFirst() }
        if hex.count == 3 {
            hex = hex.map { "\($0)\($0)" }.joined()
        }
        guard hex.count == 6 || hex.count == 8, let value = UInt64(hex, radix: 16) else {
            throw ToolError(message: "\(key) must be a hex color like #RRGGBB or #RRGGBBAA")
        }
        let hasAlpha = hex.count == 8
        let rgb = hasAlpha ? value >> 8 : value
        let alpha = hasAlpha ? CGFloat(value & 0xFF) / 255 : 1
        return NSColor(
            srgbRed: CGFloat((rgb >> 16) & 0xFF) / 255,
            green: CGFloat((rgb >> 8) & 0xFF) / 255,
            blue: CGFloat(rgb & 0xFF) / 255,
            alpha: alpha)
    }

    private func paintStroke(_ a: [String: Any], erase: Bool) throws -> String {
        let document = try target(a)
        // Structural, not the pixel helper: whether a GROUP is a legal target
        // depends on the `target` argument, which is parsed below — a group's
        // canvas-sized mask and the document's channels are both paintable on
        // one (rejectGroupPixelEdit).
        let index = try structuralLayerIndex(a, document)
        let points = try byteStrokePoints(a)
        let size = CGFloat(min(max(doubleArg(a, "size") ?? 16, 1), EditorTool.brush.maximumBrushSize))
        let opacity = min(max(doubleArg(a, "opacity") ?? 1, 0), 1)
        let tip = try strokeTip(a)
        let blend = try blendModeArg(a)
        if blend != nil, erase {
            throw ToolError(
                message: "eraser_stroke has no blend_mode — erasing removes alpha; blend "
                    + "modes apply to brush_stroke and clone_stamp.")
        }
        // The whole routing decision — the widened target vocabulary, the
        // adjustment-layer forcing, the coverage colour, the blend_mode
        // refusals and the undo name (AgentServer+ChannelTargets.swift).
        let (target, layer, actionName, color, targetNote) = try resolvePaintTarget(
            a, document, requestedLayer: index, erase: erase, blend: blend)
        // The group guard, now that the target is known: only the two targets
        // that write the LAYER's own pixels refuse one.
        if target == .layer || target.isPlane { try rejectGroupPixelEdit(document, layer) }
        // A canonical dab stroke rasterizes its own dabs instead of this
        // polyline (AgentServer+NativeBrush.byteDabPaint).
        if let native = try capturedNativeStroke(a) {
            return try byteDabPaint(
                document, layer: layer, target: target, actionName: actionName, color: color,
                size: size, opacity: opacity, blend: blend, stroke: native, erase: erase)
        }
        // Latched when the PAINT OP answers nil for "nothing would change":
        // a blend mode's identity blend, or a coverage stroke on a plane or
        // a channel that painted what was already there. A no-op to report,
        // never a parameter error.
        var opRefused = false
        let stamped = SoftBrush.isStamped(tip: tip, size: size)
        let spacing = SoftBrush.spacing(for: size, percent: tip.spacingPercent)
        let centers = try stamped ? SoftBrush.stampCenters(along: points, spacing: spacing) : []
        let rasterized: DroppedDescription?
        // Only a non-Normal blend on a pixel target reads the ink colour.
        let needsInk = blend.map { $0 != DR_BLEND_NORMAL } == true && !target.isCoverage
        let rgb = needsInk ? ColorProfile.bytes(color, in: document.nsColorSpace) : []
        guard let rgb else { throw ToolError(message: "Could not convert the paint color.") }
        if stamped { try SoftBrush.admitDabImage(diameter: size) }
        do {
            rasterized = try paintOverlay(
                document, layer: layer, actionName: actionName,
                mode: erase ? DR_COMPOSITE_ERASE : DR_COMPOSITE_OVER, alpha: opacity,
                target: target, blend: blend.map { ($0, Array(rgb.prefix(3))) },
                onOpRefusal: { opRefused = true }
            ) { context in
                let ink = target.isCoverage
                    ? color : ColorProfile.drawingInk(color, in: document.nsColorSpace)
                context.setFillColor(ink.cgColor)
                context.setStrokeColor(ink.cgColor)
                // Any non-default tip stamps SoftBrush falloff dabs — the
                // interactive stamped pipeline — instead of a hard path. Dabs
                // deposit at the tip's FLOW, inside ONE transparency layer
                // capped at the color's own alpha (a mask stroke's opacity
                // rides there): per-dab opacity would compound where dabs
                // overlap, pushing a 50% stroke's core toward 100%, where the
                // hard path paints the whole polyline at one uniform alpha.
                if stamped,
                   let dab = SoftBrush.dab(
                       color: color.withAlphaComponent(tip.flow), diameter: size,
                       hardness: tip.hardness, space: document.drawingSpace) {
                    let alpha = (color.usingColorSpace(.sRGB) ?? color).alphaComponent
                    context.saveGState()
                    context.setAlpha(alpha)
                    context.beginTransparencyLayer(auxiliaryInfo: nil)
                    for center in centers {
                        SoftBrush.stamp(dab, in: context, at: center, diameter: size, tip: tip)
                    }
                    context.endTransparencyLayer()
                    context.restoreGState()
                    return
                }
                if points.count == 1 {
                    let p = points[0]
                    context.fillEllipse(
                        in: CGRect(
                            x: p.x - size / 2, y: p.y - size / 2, width: size, height: size))
                    return
                }
                context.setLineWidth(size)
                context.setLineCap(.round)
                context.setLineJoin(.round)
                context.move(to: points[0])
                for point in points.dropFirst() {
                    context.addLine(to: point)
                }
                context.strokePath()
            }
        } catch let error as ToolError {
            guard opRefused else { throw error }
            return try noOpStrokeResult(document, target: target, layer: layer, blend: blend)
        }
        var fields: [String: Any] = [
            "ok": true, "action": actionName, "layer": layer, "points": points.count,
            "target": target.agentName(in: document.doc),
        ]
        if let targetNote = targetNote { fields["note"] = targetNote }
        return try pixelEditResult(fields, layer: layer, rasterized: rasterized)
    }

    private func addText(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let layer = try paintLayerIndex(a, document)
        try rejectAdjustmentPixelEdit(document, layer)
        let text = try requiredString(a, "text")
        guard let x = doubleArg(a, "x"), let y = doubleArg(a, "y") else {
            throw ToolError(message: "add_text requires x and y (top-left of the text block)")
        }
        let fontSize = CGFloat(min(max(doubleArg(a, "size") ?? 48, 4), 1000))
        let font: NSFont
        if let name = stringArg(a, "font") {
            guard let named = NSFont(name: name, size: fontSize) else {
                throw ToolError(message: "Unknown font \"\(name)\" — use a family or PostScript name")
            }
            font = named
        } else {
            font = .systemFont(ofSize: fontSize)
        }
        let color = a["color_space"] != nil
            ? try qualifiedColor(a, "color", in: document, fallback: .black)
            : try parseColor(a, "color", fallback: .black)
        let attributed = NSAttributedString(
            string: text, attributes: [
                .font: font,
                .foregroundColor: ColorProfile.drawingInk(color, in: document.nsColorSpace),
            ])
        let canvasWidth = Double(document.doc?.width ?? 0)
        let box = CGRect(
            x: x, y: y, width: max(canvasWidth - x, 10), height: 100_000)
        let measured = attributed.boundingRect(
            with: NSSize(width: box.width, height: box.height),
            options: .usesLineFragmentOrigin)
        let rasterized = try paintOverlay(
            document, layer: layer, actionName: "Add Text",
            mode: DR_COMPOSITE_OVER, alpha: 1
        ) { context in
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
            attributed.draw(with: box, options: .usesLineFragmentOrigin)
            NSGraphicsContext.restoreGraphicsState()
        }
        return try pixelEditResult(
            [
                "ok": true, "layer": layer,
                "text_size": [
                    "width": Int(measured.width.rounded(.up)),
                    "height": Int(measured.height.rounded(.up)),
                ],
            ], layer: layer, rasterized: rasterized)
    }

    // MARK: - Selection, fill, gradient

    // Internal, not private: the +Feature stroke handlers reach the shared
    // selection through it.
    func editor(_ document: ImageDocument) -> EditorViewController? {
        document.windowControllers
            .compactMap { $0.contentViewController as? EditorViewController }
            .first
    }

    /// The document's live selection mask (nil when nothing is selected
    /// or the document has no editor window). Internal, like the arg
    /// helpers: the +Feature handlers cut with the same selection.
    func selectionMask(_ document: ImageDocument) -> [UInt8]? {
        document.canvasSelection?.maskBytes()
    }

    /// The select_* tools' optional "mode" argument (default replace).
    func selectionMode(_ a: [String: Any]) throws -> SelectionCombineMode {
        switch stringArg(a, "mode") ?? "replace" {
        case "replace": return .replace
        case "add": return .add
        case "subtract": return .subtract
        case "intersect": return .intersect
        case let other:
            throw ToolError(
                message: "mode must be replace, add, subtract, or intersect (got \"\(other)\")")
        }
    }

    /// `extra` is merged into the reported result, for tools that learned
    /// something while making the shape (select_subject's instance count).
    /// Internal, not private: the +Feature handler files apply selections.
    func applySelection(
        _ document: ImageDocument, _ shape: CanvasSelection.Shape,
        mode: SelectionCombineMode, feather: Double = 0, extra: [String: Any] = [:]
    ) throws -> String {
        guard let editorVC = editor(document) else {
            throw ToolError(message: "The document has no editor window to hold a selection.")
        }
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard feather.isFinite, feather >= 0, feather <= 250 else {
            throw ToolError(message: "feather must be between 0 and 250 pixels.")
        }
        guard
            var selection = CanvasSelection(
                shape: shape, canvasWidth: doc.width, canvasHeight: doc.height)
        else {
            throw ToolError(message: "The selection would be empty.")
        }
        if feather > 0, let softened = selection.feathered(by: feather) { selection = softened }
        return try setCombined(
            editorVC, CanvasSelection.combine(editorVC.agentSelection, with: selection, mode: mode),
            extra: extra)
    }

    /// Applies a combine/modify result: an empty (nil) result deselects,
    /// reported in-band rather than as an error. Internal, not private: the
    /// channel tools route an empty plane straight here rather than through
    /// applySelection, which throws on one.
    func setCombined(
        _ editorVC: EditorViewController, _ selection: CanvasSelection?,
        extra: [String: Any] = [:]
    ) throws -> String {
        // The tool's own keys never overwrite the shared ones, so a caller
        // cannot disguise a failure as an "ok".
        guard let selection = selection else {
            editorVC.agentSetSelection(nil)
            return try jsonResult(
                extra.merging([
                    "ok": true, "selection_empty": true,
                    "note": "The resulting selection is empty; the selection was cleared.",
                ]) { _, shared in shared })
        }
        editorVC.agentSetSelection(selection)
        let b = selection.bounds
        return try jsonResult(
            extra.merging([
                "ok": true,
                "bounds": [
                    "x": Int(b.minX), "y": Int(b.minY),
                    "width": Int(b.width), "height": Int(b.height),
                ],
            ]) { _, shared in shared })
    }

    private func selectShape(
        _ a: [String: Any], _ make: (CGRect) -> CanvasSelection.Shape
    ) throws -> String {
        let document = try target(a)
        guard let x = intArg(a, "x"), let y = intArg(a, "y"),
            let w = intArg(a, "width"), let h = intArg(a, "height"), w > 0, h > 0
        else {
            throw ToolError(message: "Requires x, y, width, height (width/height > 0)")
        }
        return try applySelection(
            document, antiAliased(a, make(CGRect(x: x, y: y, width: w, height: h)), document),
            mode: selectionMode(a), feather: doubleArg(a, "feather") ?? 0)
    }

    private func selectPolygon(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let points = try parsePoints(a)
        guard points.count >= 3 else {
            throw ToolError(message: "A polygon selection needs at least 3 points")
        }
        return try applySelection(document, antiAliased(a, .polygon(points), document),
                                  mode: selectionMode(a), feather: doubleArg(a, "feather") ?? 0)
    }

    private func selectMagicWand(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc, let x = intArg(a, "x"), let y = intArg(a, "y") else {
            throw ToolError(message: "select_magic_wand requires x and y")
        }
        // `sample_all_layers` false reads one layer alone (nil = the composite).
        let wandLayer = try wandSampleLayer(a, document)
        let sampleSize = try wandSampleSize(a)
        if doc.isHighDepth {
            let capture = try SelectionCapture(document: document, needsSource: false)
            let tolerance = doubleArg(a, "tolerance") ?? 32
            guard tolerance.isFinite, (0...255).contains(tolerance) else { throw ToolError(message: "Wand tolerance must be between 0 and 255.") }
            let samples = try capture.nativeSource(["kind": "magic_wand", "x": x, "y": y,
                "tolerance": Int((tolerance * 257).rounded()), "contiguous": boolArg(a, "contiguous") ?? true,
                "sample_size": sampleSize],
                in: document, title: "Magic Wand", scope: wandLayer.map { .layer($0) } ?? .composite)
            try capture.commit(.mask16(samples), in: document, name: "Magic Wand", mode: selectionMode(a),
                feather: doubleArg(a, "feather") ?? 0)
            return try jsonResult(selectionHistorySummary(document))
        }
        let tolerance = intArg(a, "tolerance") ?? 32
        let contiguous = boolArg(a, "contiguous") ?? true
        guard
            let mask = wandLayer.map({
                doc.magicWandLayer($0, x: x, y: y, tolerance: tolerance, contiguous: contiguous,
                                   sampleSize: sampleSize)
            }) ?? doc.magicWand(x: x, y: y, tolerance: tolerance, contiguous: contiguous,
                                sampleSize: sampleSize)
        else {
            throw ToolError(message: wandLayer == nil ? "The seed point is outside the canvas"
                : "The seed point is outside the canvas, or the layer has no pixels to sample")
        }
        return try applySelection(document, .mask(mask), mode: selectionMode(a),
                                  feather: doubleArg(a, "feather") ?? 0)
    }

    private func deselect(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        document.setCanvasSelection(nil, actionName: "Deselect")
        return try jsonResult(["ok": true])
    }

    private func modifySelection(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let editorVC = editor(document) else {
            throw ToolError(message: "The document has no editor window to hold a selection.")
        }
        guard let selection = editorVC.agentSelection else {
            // Select Inverse with nothing selected is no failure.
            if stringArg(a, "operation") == "invert" {
                return try structureNoOp(["operation": "invert", "selection_empty": true],
                                         why: "there is no selection to invert.")
            }
            throw ToolError(
                message: "There is no selection to modify. Make one with the select_* tools.")
        }
        // grow/shrink/smooth share feather's parameter name and validation
        // (radius, positive and finite, clamped to 250); border's band is a
        // width, so its parameter says so.
        func radius(_ op: String) throws -> Double {
            guard let radius = doubleArg(a, "radius"), radius.isFinite, radius > 0 else {
                throw ToolError(message: "\(op) requires a positive radius (px)")
            }
            return min(radius, 250)
        }
        if document.doc?.isHighDepth == true || selection.hasNativeCoverage {
            let capture = try SelectionCapture(document: document, needsSource: false)
            let operation = try requiredString(a, "operation")
            guard ["invert", "feather", "grow", "shrink", "smooth", "border"].contains(operation) else { throw ToolError(message: "Unknown selection operation.") }
            let value = operation == "invert" ? 0 : operation == "border" ? (doubleArg(a, "width") ?? .nan) : (doubleArg(a, "radius") ?? .nan)
            guard value.isFinite, (0...250).contains(value) else { throw ToolError(message: "Selection radius or width must be between 0 and 250 pixels.") }
            let result = try selection.nativeModified(operation, radius: value)
            try capture.requireCurrent(in: document)
            document.setCanvasSelection(result, actionName: "Modify Selection")
            return try jsonResult(selectionHistorySummary(document))
        }
        let modified: CanvasSelection?
        switch try requiredString(a, "operation") {
        case "invert":
            modified = selection.inverted()
        case "feather":
            modified = selection.feathered(by: try radius("feather"))
        case "grow":
            modified = selection.grown(by: try radius("grow"))
        case "shrink":
            modified = selection.shrunk(by: try radius("shrink"))
        case "smooth":
            modified = selection.smoothed(by: try radius("smooth"))
        case "border":
            guard let width = doubleArg(a, "width"), width.isFinite, width > 0 else {
                throw ToolError(message: "border requires a positive width (px)")
            }
            modified = selection.bordered(width: min(width, 250))
        case let other:
            throw ToolError(
                message: "operation must be \"invert\", \"feather\", \"grow\", \"shrink\", "
                    + "\"border\", or \"smooth\" (got \"\(other)\")")
        }
        return try setCombined(editorVC, modified)
    }

    private func fill(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let index = try structuralLayerIndex(a, document)
        // A colour plane or an alpha channel fills THAT plane instead
        // (AgentServer+ChannelTargets), mirroring the Fill tool's redirect in
        // EditorViewController+PlanePaint.
        let planeTarget = try paintTarget(a, document)
        let arguments = try fillArguments(a, document)
        // `blend_mode` / `sample_all_layers` (+BucketFill).
        let style = try bucketFillStyle(a, target: planeTarget)
        _ = try operationCapture(document, operation: .fill, target: planeTarget, layer: index)
        let mask = selectionMask(document)
        if planeTarget != .layer {
            return try fillPlane(
                document, target: planeTarget, layer: index, arguments, mask: mask)
        }
        if !style.isDefault {
            return try fillWithStyle(document, layer: index, arguments, style: style, mask: mask)
        }
        try rejectAdjustmentPixelEdit(document, index)
        let rasterized = try performPixelEdit(document, "Fill", pixelLayer: index) { doc in
            doc.bucketFilled(
                index, x: arguments.x, y: arguments.y, tolerance: arguments.tolerance,
                rgba: arguments.rgba, contiguous: arguments.contiguous, mask: mask)
        }
        return try pixelEditResult(
            ["ok": true, "layer": index], layer: index, rasterized: rasterized)
    }

    private func gradient(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        if let painted = try quickMaskGradient(a, document) { return painted }
        let index = try structuralLayerIndex(a, document)
        // As in fill: a plane or channel target lays the ramp down THAT plane
        // (AgentServer+ChannelTargets), mirroring the Gradient tool.
        let planeTarget = try paintTarget(a, document)
        let arguments = try gradientArguments(a, document)
        _ = try operationCapture(document, operation: .gradient, target: planeTarget, layer: index)
        let mask = selectionMask(document)
        if planeTarget != .layer {
            return try gradientPlane(
                document, target: planeTarget, layer: index, arguments, mask: mask)
        }
        try rejectAdjustmentPixelEdit(document, index)
        let rasterized = try performPixelEdit(document, "Gradient", pixelLayer: index) { doc in
            arguments.painting(doc, layer: index, mask: mask)
        }
        return try pixelEditResult(
            ["ok": true, "layer": index], layer: index, rasterized: rasterized)
    }

    /// clear_selection: erases the window's live selection out of a layer,
    /// proportionally to its coverage (a feathered selection leaves a soft
    /// edge). Nothing selected is a recoverable error, not a whole-layer wipe.
    private func clearSelection(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let index = try paintLayerIndex(a, document)
        try rejectAdjustmentPixelEdit(document, index)
        guard let mask = selectionMask(document) else {
            throw ToolError(
                message: "There is no selection to clear. Make one first with select_rect, "
                    + "select_ellipse, select_polygon or select_magic_wand.")
        }
        let rasterized = try performPixelEdit(document, "Clear", pixelLayer: index) { doc in
            doc.clearingSelection(index, mask: mask)
        }
        return try pixelEditResult(
            ["ok": true, "layer": index], layer: index, rasterized: rasterized)
    }

    /// The DOCUMENT's straight-alpha bytes of a parsed color — the agent's
    /// spelling of `EditorViewController.colorBytes`, sharing its one
    /// conversion (`ColorProfile.bytes`). `fill` and `gradient` hand these
    /// bytes straight to the core instead of drawing through a
    /// document-space context, so an AUTHORED `#RRGGBB` has to convert here
    /// to land the same colour `brush_stroke` and `add_shape_layer` paint.
    /// A hex over the wire is authored sRGB unless its tool takes
    /// `color_space` (`DocumentPrecision.colorSpaceGate`; `qualifiedColor`
    /// resolves it): a hex string carries no space, so on every other tool a
    /// colour read back out of the document travels as `sample_color`'s
    /// `paint_hex`, the sRGB spelling this converts back into the pixel it
    /// came from. Internal, not private: the plane fill/gradient mirrors in
    /// AgentServer+ChannelTargets reduce these same document bytes to
    /// coverage gray, exactly as the Fill tool's redirect does
    /// (EditorViewController+PlanePaint).
    func colorRGBA(_ color: NSColor, in document: ImageDocument) throws -> [UInt8] {
        guard let bytes = ColorProfile.bytes(color, in: document.nsColorSpace) else {
            throw ToolError(message: "Could not convert the color")
        }
        return bytes
    }

    private func imageSize(_ a: [String: Any]) throws -> String {
        guard let w = intArg(a, "width"), let h = intArg(a, "height") else {
            throw ToolError(message: "image_size requires width and height")
        }
        let filters: [String: DrResizeFilter] = [
            "nearest": DR_FILTER_NEAREST, "bilinear": DR_FILTER_BILINEAR,
            "catmull-rom": DR_FILTER_CATMULL_ROM, "lanczos3": DR_FILTER_LANCZOS3,
        ]
        let filter = try stringArg(a, "filter").map { name in
            guard let match = filters[name] else {
                throw ToolError(message: "filter must be one of \(filters.keys.sorted())")
            }
            return match
        } ?? DR_FILTER_LANCZOS3
        // The channel budget, asked as the Image Size sheet asks it: the core
        // refuses the resize outright when the channels would no longer fit,
        // and a generic failure would tell a model to fix the one thing that
        // is right (the size).
        let document = try target(a)
        try requireChannelBudget(document, width: w, height: h)
        let op = DocumentGeometry.resize(width: w, height: h, filter: filter)
        return try docEdit(a, op.actionName, changesCanvasGeometry: true,
                           resultingSelection: document.selectionCarried(through: op)) { try $0.applyingDocumentGeometry(op) }
    }

    private func canvasSize(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        guard let w = intArg(a, "width"), let h = intArg(a, "height") else {
            throw ToolError(message: "canvas_size requires width and height")
        }
        let anchors: [String: (col: Int, row: Int)] = [
            "top-left": (0, 0), "top": (1, 0), "top-right": (2, 0),
            "left": (0, 1), "center": (1, 1), "right": (2, 1),
            "bottom-left": (0, 2), "bottom": (1, 2), "bottom-right": (2, 2),
        ]
        let anchorName = stringArg(a, "anchor") ?? "center"
        guard let anchor = anchors[anchorName] else {
            throw ToolError(message: "anchor must be one of \(anchors.keys.sorted())")
        }
        // A call that would change NOTHING is the house no-op, never an error
        // (12F-1b). It mirrors the Canvas Size sheet's own early return
        // (`Sheets.swift`: dismiss, then `guard w != originalWidth || h !=
        // originalHeight`), which already makes no step and shows no alert at
        // every depth, and follows the precedents `move_layers` (dx = dy = 0),
        // `set_layer_properties`, the guide, channel, group and label tools
        // set. The anchor cannot matter: the origin is necessarily (0, 0).
        if w == doc.width && h == doc.height {
            return try structureNoOp(["document": summary(document)],
                why: "the canvas is already \(w) × \(h), so the anchor moves nothing.")
        }
        // The same channel-budget question the Canvas Size sheet asks first.
        try requireChannelBudget(document, width: w, height: h)
        // Matches the Canvas Size sheet: the anchor's column/row chooses how
        // much of the size delta lands left/above the old canvas origin.
        let (originX, originY) = RasterDocument.canvasSizeOrigin(oldWidth: doc.width,
            oldHeight: doc.height, newWidth: w, newHeight: h, anchor: anchor)
        try performGroupedEdit(document, "Canvas Size", resultingSelection: document.selectionCarried(
            canvasWidth: w, height: h, originX: originX, originY: originY)) {
            try $0.canvasResized(w: w, h: h, originX: originX, originY: originY)
        }
        return try jsonResult(["ok": true, "document": summary(document)])
    }

    // MARK: - Undo, export

    private func undoRedo(_ a: [String: Any], redo: Bool) throws -> String {
        let document = try target(a)
        if let refusal = DocumentJobs.historyStepRefusal(on: document, redo: redo) {
            throw ToolError(message: refusal)
        }
        guard let manager = document.undoManager else {
            throw ToolError(message: "No undo manager")
        }
        if redo {
            guard manager.canRedo else { throw ToolError(message: "Nothing to redo") }
        } else {
            guard manager.canUndo else { throw ToolError(message: "Nothing to undo") }
        }
        guard document.stepHistory(redo: redo) else {
            let verb = redo ? ("redone", "redo") : ("undone", "undo")
            throw ToolError(message: "Nothing was \(verb.0): that \(verb.1) step held no document change.")
        }
        var result: [String: Any] = ["ok": true, "document": summary(document)]
        if let warning = document.historyMemoryWarning { result["memory_warning"] = warning.detail }
        return try jsonResult(result)
    }

    private func saveCopy(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try commitPendingCanvasSessions(document)
        if document.doc?.precision != nil { return try savePrecisionCopy(a, document: document) }
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        let formatName = stringArg(a, "format") ?? url.pathExtension.lowercased()
        if ImageDocument.isNativeFormatName(formatName) {
            // Native layered format: the full document (layers, masks,
            // metadata, clipped flags), no flattening. dr_doc_save_native
            // writes atomically (temp file + rename), so a failure never
            // truncates an existing destination.
            guard let doc = document.doc else {
                throw ToolError(message: "No image loaded")
            }
            do {
                let captured = try CapturedSave(document, type: ImageDocument.nativeTypeIdentifier)
                _ = try RasterResources.shared.admit(scratch: captured.scratchEstimate,
                    own: captured.ownRetainedBytes, remedy: captured.refusalRemedy)
                try BackgroundWork.wait(try WorkerTask { try doc.saveNative(to: url) }, title: "Saving Copy")
            } catch {
                throw ToolError(message: error.localizedDescription)
            }
            return try jsonResult(
                ["ok": true, "path": url.path, "format": ImageDocument.nativeFormatName])
        }
        guard let doc = document.doc else {
            throw ToolError(message: "Document has no image")
        }
        // #1: a TIFF keeps the spot channels (1c-2a); any other flat file composites them.
        let image = try document.flatSaveProjection(format: ExportFormat.from(fileExtension: formatName))
        // `save_copy` is a FLAT save, so the host-encoded formats are not
        // among its choices (Phase 11D); `export_image` delivers those.
        guard let format = ExportFormat.from(fileExtension: formatName),
              ExportFormat.flatSaveCases.contains(format) else {
            let names = ([ImageDocument.nativeFormatName]
                + ExportFormat.flatSaveCases.map { $0.fileExtension })
                .joined(separator: ", ")
            throw ToolError(message: "Cannot infer the format; pass format as one of: \(names)")
        }
        let quality = intArg(a, "jpeg_quality") ?? document.jpegExportQuality
        let embed = boolArg(a, "embed_profile") ?? document.embedColorProfile
        let strip = boolArg(a, "strip_metadata") ?? document.stripMetadata
        let losses = try doc.flatSaveLosses(
            format: format.drFormat, embedProfile: embed, stripMetadata: strip)
        let report: RasterSaveReport
        do {
            // The document-level save, so the profile, the EXIF/XMP/IPTC
            // packets and the print resolution ride along; the warm
            // projection is the composite, so nothing re-flattens.
            _ = try RasterResources.shared.admit(scratch: SpotChannels.byteSaveScratch(doc, format: format))
            let task = try WorkerTask {
                try doc.saveImage(image, to: url, format: format.drFormat, jpegQuality: quality,
                                  embedProfile: embed, stripMetadata: strip)
            }
            report = try BackgroundWork.wait(task, title: "Exporting Copy")
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        var result: [String: Any] = ["ok": true, "path": url.path, "format": format.displayName]
        // What the chosen format could and could not carry, so a model never
        // has to guess whether the profile survived (AgentServer+Color.swift).
        result.merge(Self.savedFields(doc, document, report: report)) { _, new in new }
        result["flat_save_losses"] = losses
        result.merge(SpotChannels.keptPlan(doc, format: format)) { _, new in new }
        return try jsonResult(result)
    }

    // MARK: - Argument helpers

    func intArg(_ a: [String: Any], _ key: String) -> Int? {
        (a[key] as? NSNumber)?.intValue ?? (a[key] as? String).flatMap(Int.init)
    }

    func doubleArg(_ a: [String: Any], _ key: String) -> Double? {
        (a[key] as? NSNumber)?.doubleValue ?? (a[key] as? String).flatMap(Double.init)
    }

    func boolArg(_ a: [String: Any], _ key: String) -> Bool? {
        // Like intArg/doubleArg, a string spelling of the value is accepted.
        if let number = a[key] as? NSNumber { return number.boolValue }
        switch a[key] as? String {
        case "true", "1": return true
        case "false", "0": return false
        default: return nil
        }
    }

    func stringArg(_ a: [String: Any], _ key: String) -> String? {
        a[key] as? String
    }

    func requiredString(_ a: [String: Any], _ key: String) throws -> String {
        guard let value = stringArg(a, key), !value.isEmpty else {
            throw ToolError(message: "Missing required argument: \(key)")
        }
        return value
    }

    // MARK: - Result encoding

    func callResult(content: [[String: Any]], isError: Bool) throws -> String {
        let data = try JSONSerialization.data(
            withJSONObject: ["content": content, "isError": isError])
        return String(decoding: data, as: UTF8.self)
    }

    func jsonResult(_ object: [String: Any]) throws -> String {
        let data = try JSONSerialization.data(withJSONObject: object)
        return try callResult(
            content: [["type": "text", "text": String(decoding: data, as: UTF8.self)]],
            isError: false)
    }

    func errorResult(_ message: String) -> String {
        let content: [[String: Any]] = [["type": "text", "text": message]]
        let fallback = #"{"content":[{"type":"text","text":"tool failed"}],"isError":true}"#
        guard
            let data = try? JSONSerialization.data(
                withJSONObject: ["content": content, "isError": true])
        else { return fallback }
        return String(decoding: data, as: UTF8.self)
    }
}

/// C callback for DrAgentToolHandler: executes on main through
/// `AgentServeGate` hops (`AgentServer+ServeThread.swift`) and hands the
/// result back as a core-owned string.
func agentToolTrampoline(
    _ context: UnsafeMutableRawPointer?,
    _ toolName: UnsafePointer<CChar>?,
    _ argumentsJSON: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>? {
    guard let context = context, let toolName = toolName, let argumentsJSON = argumentsJSON
    else { return nil }
    let server = Unmanaged<AgentServer>.fromOpaque(context).takeUnretainedValue()
    let name = String(cString: toolName)
    let arguments = String(cString: argumentsJSON)
    // Held for the whole call: without it App Nap demotes the app after its
    // first multi-second tool and every later one runs ~5x slower for the
    // rest of the session (`AppActivity` carries the measurements).
    let result = AppActivity.userInitiated("running the \(name) tool") {
        Thread.isMainThread
            ? server.execute(tool: name, argumentsJSON: arguments)
            : server.executeFromServeThread(tool: name, argumentsJSON: arguments)
    }
    return RasterAgentServer.coreString(result)
}
