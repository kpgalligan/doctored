import AppKit

// The pixel-art tools are `put_pixels` (runs and/or one base64 RGBA8 buffer, replace semantics, one
// "Put Pixels" step through `performPixelEdit`, the selection ignored, a write past a trimmed layer
// grows it through `expandingRetouchDestination`, 8-bit only; `RasterDocument.puttingPixels` →
// `dr_doc_put_pixels`), `close_document` (File ▸ Close for external MCP clients: `document_id`
// required, an unsaved document refused unless `discard_changes`; excluded from the Assistant panel
// by `AssistantHelperSession.panelExcludedTools` and from Actions by `Action.notAStepTool`) and
// `render`'s integer `scale` (`renderScale` / `pixelScaled`, `DR_FILTER_NEAREST`, ≤ 4096 px).

/// MCP tools for exact, pixel-art work: `put_pixels` (many exact pixels onto
/// one raster layer, one call, one undo step), `close_document` (File ▸
/// Close and its save prompt, for external MCP clients only) and `render`'s
/// integer `scale` (an exact nearest-neighbour enlargement).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let pixelArtHandlers: [String: ToolHandler] = [
        "close_document": { $0.closeDocument },
        // Exact pixel writes, a scripted Pencil (AgentServer+PixelArt.swift)
        "put_pixels": { $0.putPixels },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let pixelArtToolFacts: [String: ToolFacts] = [
        // `close_document` ends a document — `open_document`'s reason, reversed.
        "close_document": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
    ]

    /// One parsed horizontal span of `put_pixels`' `runs`, its colour already
    /// converted to the document's straight RGBA8 bytes.
    private struct PixelRun {
        let x: Int
        let y: Int
        let length: Int
        let rgba: [UInt8]
    }

    /// `put_pixels`' parsed `buffer`: `width·height·4` raw straight RGBA8
    /// bytes as given, converted per distinct value when written.
    private struct PixelBuffer {
        let x: Int
        let y: Int
        let width: Int
        let height: Int
        let data: [UInt8]
    }

    /// An exact integer, or a refusal naming `name`: a fractional number, a
    /// Boolean and anything that is not a number (or a decimal integer string)
    /// is refused rather than truncated, which `intArg` would do.
    private static func exactInteger(_ value: Any?, _ name: String) throws -> Int {
        guard let value, !(value is NSNull) else {
            throw ToolError(message: "\(name) is required (an integer).")
        }
        if let number = value as? NSNumber, !isJSONBoolean(number),
           number.doubleValue.isFinite, number.doubleValue.rounded() == number.doubleValue,
           let exact = Int(exactly: number.doubleValue) {
            return exact
        }
        if let text = value as? String, let parsed = Int(text) { return parsed }
        throw ToolError(message: "\(name) must be an integer.")
    }

    // MARK: - put_pixels

    /// put_pixels: writes exact pixels onto one raster layer with REPLACE
    /// semantics — each written pixel takes its four bytes, alpha included,
    /// with no blending. The UI path it mirrors is a Pencil (hard 1 px brush)
    /// clicked pixel by pixel; here a whole drawing is one call and one
    /// "Put Pixels" undo step through `performPixelEdit`. The coordinates are
    /// the confinement, so the selection is ignored (as `transform_layer` and
    /// `move_layers` ignore it).
    func putPixels(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard !document.quickMaskActive else {
            throw ToolError(message: OperationScope.quickMaskRefusal)
        }
        let index = try paintLayerIndex(a, document)
        try rejectAdjustmentPixelEdit(document, index)
        guard let doc = document.doc else { throw ToolError(message: "Document has no image") }
        let canvasWidth = doc.width
        let canvasHeight = doc.height
        // Validated once up front, so a bad name refuses even with no runs.
        _ = try colorSpaceArgument(a)
        guard a["runs"] != nil || a["buffer"] != nil else {
            throw ToolError(
                message: "put_pixels needs runs, buffer or both: runs is an array of "
                    + "{x, y, length, color} spans, buffer one {x, y, width, height, data} rect "
                    + "of base64 RGBA8.")
        }

        // Each distinct colour converts ONCE, through `fill`'s path.
        var converted: [String: [UInt8]] = [:]
        func documentBytes(_ hex: String) throws -> [UInt8] {
            if let cached = converted[hex] { return cached }
            var arguments: [String: Any] = ["color": hex]
            if let space = a["color_space"] { arguments["color_space"] = space }
            let bytes = try colorRGBA(
                qualifiedColor(arguments, "color", in: document, fallback: .black), in: document)
            converted[hex] = bytes
            return bytes
        }

        let buffer = try parsePixelBuffer(a["buffer"], canvas: (canvasWidth, canvasHeight))
        let runs = try parsePixelRuns(a["runs"], canvas: (canvasWidth, canvasHeight)) {
            try documentBytes($0)
        }

        // The one bounding box both inputs are written into.
        var minX = Int.max, minY = Int.max, maxX = Int.min, maxY = Int.min
        if let buffer {
            minX = buffer.x; minY = buffer.y
            maxX = buffer.x + buffer.width; maxY = buffer.y + buffer.height
        }
        for run in runs {
            minX = min(minX, run.x); minY = min(minY, run.y)
            maxX = max(maxX, run.x + run.length); maxY = max(maxY, run.y + 1)
        }
        guard minX < maxX, minY < maxY else {
            throw ToolError(message: "put_pixels was given no pixels to write: runs is empty.")
        }
        let boxWidth = maxX - minX
        let boxHeight = maxY - minY
        var rgba = [UInt8](repeating: 0, count: boxWidth * boxHeight * 4)
        var mask = [UInt8](repeating: 0, count: boxWidth * boxHeight)

        // The buffer first, then the runs in array order: the last write wins.
        if let buffer {
            var byValue: [UInt32: [UInt8]] = [:]
            for row in 0..<buffer.height {
                for column in 0..<buffer.width {
                    let source = (row * buffer.width + column) * 4
                    let value = buffer.data[source..<source + 4]
                    let key = value.reduce(UInt32(0)) { $0 << 8 | UInt32($1) }
                    let bytes: [UInt8]
                    if let cached = byValue[key] {
                        bytes = cached
                    } else {
                        bytes = try documentBytes(String(format: "#%08X", key))
                        byValue[key] = bytes
                    }
                    let at = (buffer.y + row - minY) * boxWidth + (buffer.x + column - minX)
                    rgba.replaceSubrange(at * 4..<at * 4 + 4, with: bytes)
                    mask[at] = 255
                }
            }
        }
        for run in runs {
            let start = (run.y - minY) * boxWidth + (run.x - minX)
            for at in start..<start + run.length {
                rgba.replaceSubrange(at * 4..<at * 4 + 4, with: run.rgba)
                mask[at] = 255
            }
        }
        let written = mask.reduce(0) { $0 + ($1 == 0 ? 0 : 1) }

        // Growth: a written position outside the layer's pixel rect grows the
        // layer to its union with the canvas first, in the same undo step —
        // `SelectionFillStroke.destination`'s rule — unless its alpha is
        // frozen, which a grown transparent margin could never hold.
        var grows = false
        if let info = doc.layerInfo(index) {
            let left = info.offsetX, top = info.offsetY
            let right = info.offsetX + info.width, bottom = info.offsetY + info.height
            if minX < left || minY < top || maxX > right || maxY > bottom {
                scan: for row in 0..<boxHeight {
                    let y = minY + row
                    for column in 0..<boxWidth where mask[row * boxWidth + column] != 0 {
                        let x = minX + column
                        if x < left || x >= right || y < top || y >= bottom {
                            grows = true
                            break scan
                        }
                    }
                }
            }
            if grows, doc.transparencyLockOwner(index) != nil {
                throw ToolError(
                    message: "put_pixels writes outside layer \(index)'s pixel rect "
                        + "(\(info.offsetX),\(info.offsetY) \(info.width)×\(info.height)), which "
                        + "would grow the layer, but its transparency is locked. Write inside "
                        + "the layer's rect, or clear the lock with set_layer_lock first.")
            }
        }

        var growthError: Error?
        var coreRefused = false
        let rasterized: DroppedDescription?
        do {
            rasterized = try performPixelEdit(document, "Put Pixels", pixelLayer: index) { current in
                var destination = current
                if grows {
                    do {
                        destination = try current.expandingRetouchDestination(index)
                    } catch {
                        growthError = error
                        return nil
                    }
                }
                guard let result = destination.puttingPixels(
                    index, x: minX, y: minY, width: boxWidth, height: boxHeight,
                    rgba: rgba, mask: mask)
                else {
                    coreRefused = true
                    return nil
                }
                return result
            }
        } catch let error as ToolError {
            if let growthError {
                throw ToolError(
                    message: (growthError as? DoctoredCoreError)?.message
                        ?? growthError.localizedDescription)
            }
            if coreRefused, document.doc?.transparencyLockOwner(index) == nil {
                return try jsonResult(["ok": true, "layer": index, "unchanged": true])
            }
            throw error
        }
        return try pixelEditResult(
            [
                "ok": true, "layer": index, "pixels": written,
                "bounds": ["x": minX, "y": minY, "width": boxWidth, "height": boxHeight],
                "grew_layer": grows,
            ], layer: index, rasterized: rasterized)
    }

    /// `put_pixels`' optional `buffer`, refused by name when it is not an
    /// object, leaves the canvas, or its base64 does not decode to exactly
    /// `width·height·4` bytes.
    private func parsePixelBuffer(
        _ raw: Any?, canvas: (width: Int, height: Int)
    ) throws -> PixelBuffer? {
        guard let raw, !(raw is NSNull) else { return nil }
        guard let object = raw as? [String: Any] else {
            throw ToolError(message: "buffer must be an object {x, y, width, height, data}.")
        }
        let x = try Self.exactInteger(object["x"], "buffer.x")
        let y = try Self.exactInteger(object["y"], "buffer.y")
        let width = try Self.exactInteger(object["width"], "buffer.width")
        let height = try Self.exactInteger(object["height"], "buffer.height")
        guard width >= 1, height >= 1 else {
            throw ToolError(message: "buffer.width and buffer.height must be at least 1.")
        }
        guard x >= 0, y >= 0, x < canvas.width, y < canvas.height,
              width <= canvas.width - x, height <= canvas.height - y
        else {
            throw ToolError(
                message: "buffer (\(x),\(y) \(width)×\(height)) lies outside the "
                    + "\(canvas.width)×\(canvas.height) canvas; it must fit inside it.")
        }
        guard let text = object["data"] as? String,
              let decoded = Data(base64Encoded: text, options: .ignoreUnknownCharacters)
        else {
            throw ToolError(message: "buffer.data must be a base64 string of straight RGBA8 bytes.")
        }
        let expected = width * height * 4
        guard decoded.count == expected else {
            throw ToolError(
                message: "buffer.data decodes to \(decoded.count) bytes; a \(width)×\(height) "
                    + "buffer needs exactly \(expected) (width × height × 4, straight RGBA8, "
                    + "row 0 at the top).")
        }
        return PixelBuffer(x: x, y: y, width: width, height: height, data: [UInt8](decoded))
    }

    /// `put_pixels`' optional `runs`, each refused by its array index when it
    /// is malformed or leaves the canvas.
    private func parsePixelRuns(
        _ raw: Any?, canvas: (width: Int, height: Int), color: (String) throws -> [UInt8]
    ) throws -> [PixelRun] {
        guard let raw, !(raw is NSNull) else { return [] }
        guard let list = raw as? [Any] else {
            throw ToolError(message: "runs must be an array of {x, y, length, color} objects.")
        }
        return try list.enumerated().map { offset, item in
            let name = "runs[\(offset)]"
            guard let run = item as? [String: Any] else {
                throw ToolError(message: "\(name) must be an object {x, y, length, color}.")
            }
            let x = try Self.exactInteger(run["x"], "\(name).x")
            let y = try Self.exactInteger(run["y"], "\(name).y")
            var length = 1
            if let given = run["length"], !(given is NSNull) {
                length = try Self.exactInteger(given, "\(name).length")
            }
            guard length >= 1 else {
                throw ToolError(message: "\(name).length must be at least 1 (it is \(length)).")
            }
            guard x >= 0, y >= 0, x < canvas.width, y < canvas.height,
                  length <= canvas.width - x
            else {
                throw ToolError(
                    message: "\(name) (x \(x), y \(y), length \(length)) lies outside the "
                        + "\(canvas.width)×\(canvas.height) canvas; a run must fit inside it.")
            }
            guard let hex = run["color"] as? String else {
                throw ToolError(
                    message: "\(name).color is required: a hex colour #RGB, #RRGGBB or #RRGGBBAA.")
            }
            let bytes: [UInt8]
            do {
                bytes = try color(hex)
            } catch let error as ToolError {
                throw ToolError(message: "\(name): " + error.message)
            }
            return PixelRun(x: x, y: y, length: length, rgba: bytes)
        }
    }

    // MARK: - close_document

    /// close_document: File ▸ Close for an external MCP client. It mirrors
    /// `ImageDocument.canClose` (pending canvas sessions commit first) and
    /// its save prompt, answered by the caller: an unsaved document is
    /// refused unless `discard_changes` is true. `close()` then cancels the
    /// document's jobs, clears its recovery copy and closes its aux windows.
    /// Not offered to the in-app Assistant panel
    /// (`AssistantHelperSession.panelExcludedTools`) and never an Action step
    /// (`Action.notAStepTool`).
    func closeDocument(_ a: [String: Any]) throws -> String {
        guard a["document_id"] != nil, intArg(a, "document_id") != nil else {
            throw ToolError(
                message: "close_document requires document_id (list_documents reports each "
                    + "open document's id). There is no frontmost default: closing the wrong "
                    + "document cannot be taken back.")
        }
        let document = try target(a)
        let identifier = summary(document)["id"] ?? NSNull()
        if document.windowControllers.contains(where: { $0.window?.attachedSheet != nil }) {
            throw ToolError(
                message: "Document \(identifier) has a dialog open in its window. Finish or "
                    + "cancel it in the app before closing the document.")
        }
        try commitPendingCanvasSessions(document)
        let discard = boolArg(a, "discard_changes") ?? false
        let edited = document.isDocumentEdited
        if edited, !discard {
            throw ToolError(
                message: "Document \(identifier) (\(document.displayName ?? "Untitled")) has "
                    + "unsaved changes, so close_document did not close it. Pass "
                    + "discard_changes: true to close it anyway — the unsaved changes and their "
                    + "recovery copy are lost. To keep them, save_copy writes a copy first; it "
                    + "does not clear the unsaved-changes flag, so the close still needs "
                    + "discard_changes: true.")
        }
        let closed = summary(document)
        document.close()
        let remaining = NSDocumentController.shared.documents
            .compactMap { $0 as? ImageDocument }
            .map { summary($0)["id"] ?? NSNull() }
        return try jsonResult([
            "ok": true,
            "closed": [
                "id": closed["id"] ?? NSNull(), "title": closed["title"] ?? NSNull(),
                "path": closed["path"] ?? NSNull(),
            ],
            "discarded_changes": edited,
            "open_documents": remaining,
        ])
    }

    // MARK: - render scale

    /// `render`'s optional integer `scale` (nil when absent): refused by name
    /// when it is not an integer ≥ 1 or arrives together with `max_side`,
    /// whose shrink-to-fit it replaces.
    static func renderScale(_ a: [String: Any]) throws -> Int? {
        guard let raw = a["scale"], !(raw is NSNull) else { return nil }
        guard a["max_side"] == nil else {
            throw ToolError(
                message: "render takes scale or max_side, not both: scale enlarges by an exact "
                    + "integer factor, max_side shrinks to fit.")
        }
        let scale: Int
        do {
            scale = try exactInteger(raw, "scale")
        } catch {
            throw ToolError(message: "scale must be an integer >= 1 (an exact enlargement).")
        }
        guard scale >= 1 else {
            throw ToolError(message: "scale must be an integer >= 1 (it is \(scale)).")
        }
        return scale
    }

    /// `image` enlarged exactly `scale`× by nearest-neighbour replication
    /// (`dr_image_resize` with DR_FILTER_NEAREST, exact at integer factors),
    /// refused by name when the longest side would pass 4096 px.
    static func pixelScaled(_ image: RasterImage, by scale: Int) throws -> RasterImage {
        let longest = max(image.width, image.height)
        guard longest <= 4096 / scale else {
            let allowed = 4096 / max(longest, 1)
            throw ToolError(
                message: "scale \(scale) would render a \(longest) px side at more than 4096 px. "
                    + (allowed >= 1
                        ? "Pass a smaller scale (at most \(allowed) here) or render a region."
                        : "Render a smaller region of the canvas."))
        }
        guard scale > 1 else { return image }
        guard let scaled = image.resized(
            w: image.width * scale, h: image.height * scale, filter: DR_FILTER_NEAREST)
        else {
            throw ToolError(message: "Could not scale the render")
        }
        return scaled
    }
}
