import AppKit
import ImageIO
import UniformTypeIdentifiers

// Single-use five-minute MCP tickets clear on stale acceptance, discard or owner close; synchronous
// requests refuse busy before replacing an existing ticket.

/// MCP mirrors Edit > Content-Aware Fill's source editor, final comparison and
/// exact Apply. Legacy direct calls retain their cancellable final-job contract.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let contentAwareFillHandlers: [String: ToolHandler] = [
        "content_aware_fill": { $0.contentAwareFill },
        "preview_content_aware_fill": { $0.previewContentAwareFill },
        "discard_content_aware_fill_preview": { $0.discardContentAwareFillPreview },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let contentAwareFillToolFacts: [String: ToolFacts] = [
        // CAF publishes its resolved source-area recipe at the central edit.
        // Session tickets and the raw acceptance call must never record twice.
        // The fill, its preview and the discard answer a float document
        // too (`FloatRetouch`).
        "content_aware_fill": [.nonRecordable, .selectionSensitive, .typedTool, .rasterTool, .floatTool, .cmykTool],
        "preview_content_aware_fill": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "discard_content_aware_fill_preview": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
    ]

    func contentAwareFill(_ a: [String: Any]) throws -> String {
        if let token = a["preview_id"] as? String {
            try fillKeys(a, allowed: ["preview_id", "document_id"])
            let review = try ContentAwareFillTickets.find(token)
            let document = try target(a)
            guard review.document === document else { throw ToolError(message: "This fill preview belongs to another document.") }
            let candidate = try review.requireCandidate()
            try ContentAwareFillPublication.apply(candidate, capture: review.capture, settings: review.settings,
                layer: review.layer, to: document, record: review.settings.record(layer: review.layer))
            var report = review.report; report["state"] = "completed"; report["ok"] = true
            ContentAwareFillTickets.discard()
            return try jsonResult(report)
        }
        let (document, layer, coverage, settings) = try fillInput(a)
        guard let source = document.doc else { throw ToolError(message: "Document has no image.") }
        let work = try ContentAwareFillWork(source: source, layer: layer, coverage: coverage, settings: settings, nativeCoverage: document.doc?.isHighDepth == true ? document.canvasSelection?.maskSamples16() : nil)
        // Registered payloads already cover coverage and the command graph.
        // Admit the native forecast once; its live scratch is reported by the
        // job token, not again as DocumentJobs' separate host allocation.
        _ = try RasterResources.shared.admit(scratch: work.scratch)
        let job = try DocumentJobs.start(document: document, layer: layer, title: "Content-Aware Fill",
            scratch: 0, record: settings.record(layer: layer), showProgress: false,
            rasterizesTarget: settings.output == .layer,
            compute: { _, control in try work.compute(control: control) })
        job.details = settings.arguments(layer: layer)
        job.details["selected_pixels"] = coverage.reduce(0) { $0 + ($1 > 0 ? 1 : 0) }
        return try finishJobCall(job, tool: "content_aware_fill", arguments: a,
            unchanged: { try self.contentAwareFillNoOpResult(layer: layer) })
    }

    /// A final fill that changed nothing: a no-op, not a failure.
    func contentAwareFillNoOpResult(layer: Int) throws -> String {
        try jsonResult([
            "ok": true, "changed": false, "layer": layer,
            "note": "Content-Aware Fill changed nothing: the selection never reached layer "
                + "\(layer)'s pixels, or the fill reproduced the pixels already there. "
                + "No undo step was added.",
        ])
    }

    func previewContentAwareFill(_ a: [String: Any]) throws -> String {
        let review: ContentAwareFillReview
        if let token = a["preview_id"] as? String {
            try fillKeys(a, allowed: ["preview_id", "document_id"])
            review = try ContentAwareFillTickets.find(token)
            guard review.document === (try target(a)) else { throw ToolError(message: "This fill preview belongs to another document.") }
            review.poll()
        } else {
            // Waiting requires an owned task. Refuse before replacing the
            // existing ticket when its cancelled predecessor would still own
            // the worker; asynchronous previews can queue and coalesce.
            guard boolArg(a, "background") == true || !BackgroundWork.occupied else {
                throw ToolError(message: "Another background operation is running. Wait for it to finish, or use background=true to queue this preview.")
            }
            let (document, layer, coverage, settings) = try fillInput(a)
            review = try ContentAwareFillReview(document: document, layer: layer, coverage: coverage)
            ContentAwareFillTickets.store(review)
            review.request(settings)
            if boolArg(a, "background") != true { try review.wait() }
        }
        guard let document = review.document else { throw ToolError(message: "The preview document closed.") }
        do { try review.capture.requireCurrent(document) }
        catch { review.cancel(); throw error }
        if review.state == "failed" || review.state == "cancelled" { throw ToolError(message: review.message) }
        guard review.state == "ready" else { return try jsonResult(review.report) }
        let candidate = try review.requireCandidate()
        guard let after = candidate.prepared.preview else { throw ToolError(message: "The comparison image is unavailable.") }
        let budget: UInt64 = 64 * 1024 * 1024
        let payload = try PrecisionEditPayload.admitted(bytes: budget)
        let images = try BackgroundWork.wait(try WorkerTask(scratchEstimate: budget) {
            try [candidate.before, after].map { try ContentAwareFillComparison.png($0) }
        }, title: "Encoding Content-Aware Fill Comparison")
        try review.capture.requireCurrent(document)
        defer { withExtendedLifetime(payload) {} }
        var report = review.report
        report["source_width"] = after.width; report["source_height"] = after.height
        report["preview_max_dimension"] = 1600
        let text = String(decoding: try JSONSerialization.data(withJSONObject: report, options: .sortedKeys), as: UTF8.self)
        return try callResult(content: images.map { ["type": "image", "data": $0.base64EncodedString(), "mimeType": "image/png"] }
            + [["type": "text", "text": text]], isError: false)
    }

    func discardContentAwareFillPreview(_ a: [String: Any]) throws -> String {
        try fillKeys(a, allowed: ["preview_id", "document_id"])
        guard let token = a["preview_id"] as? String else { throw ToolError(message: "preview_id is required.") }
        let review = try ContentAwareFillTickets.find(token)
        if a["document_id"] != nil, review.document !== (try target(a)) {
            throw ToolError(message: "This fill preview belongs to another document.")
        }
        ContentAwareFillTickets.discard()
        return try jsonResult(["preview_id": token, "state": "cancelled", "cancel_requested": true,
            "message": "Preview discarded. Any active worker is cancelled and retains its inputs only until its mailbox drains. No edit was applied."])
    }

    private func fillKeys(_ a: [String: Any], allowed: Set<String>) throws {
        if let key = a.keys.filter({ !allowed.contains($0) }).sorted().first {
            throw ToolError(message: "Unknown or conflicting content-aware fill argument: \(key).")
        }
    }
    private func fillInput(_ a: [String: Any]) throws -> (ImageDocument, Int, [UInt8], ContentAwareFillSettings) {
        try fillKeys(a, allowed: ["document_id", "layer", "index", "target", "ring", "seed", "sample", "sample_all_layers",
            "ignore_adjustments", "output", "source_mask", "background"])
        let document = try target(a)
        guard document.planeEditTarget == .layer, !document.quickMaskActive,
              a["target"] == nil || a["target"] as? String == "layer" else {
            throw ToolError(message: "Content-aware fill supports layer RGB only. Scalar planes, masks, channels and Quick Mask are unavailable.")
        }
        let layer = try paintLayerIndex(a, document)
        try rejectAdjustmentPixelEdit(document, layer)
        try rejectLockedEdit(document, layer, DR_EDIT_PIXELS)
        guard let doc = document.doc, let mask = editor(document)?.agentSelection?.maskBytes(),
              mask.count == doc.width * doc.height else {
            throw ToolError(message: "There is no selection to fill. Make a selection first; content_aware_fill changes only its coverage.")
        }
        var settings = ContentAwareFillSettings()
        if let value = a["ring"] {
            guard !Self.isJSONBoolean(value), let parsed = intArg(a, "ring"), (0...512).contains(parsed),
                  doubleArg(a, "ring") == Double(parsed) else { throw ToolError(message: "ring must be an integer from 0 to 512.") }
            settings.ring = parsed
        }
        if let value = a["seed"] {
            guard !Self.isJSONBoolean(value), let parsed = intArg(a, "seed"), parsed >= 0,
                  doubleArg(a, "seed") == Double(parsed) else { throw ToolError(message: "seed must be a non-negative integer.") }
            settings.seed = UInt64(parsed)
        }
        settings.choice = try retouchSampling(a, default: .current)
        settings.output = try retouchOutput(a)
        if let value = a["source_mask"] {
            guard let mask = value as? [String: Any] else { throw ToolError(message: "source_mask must be a versioned source-area recipe object.") }
            settings.sourceMask = mask
        }
        if a["background"] != nil, boolArg(a, "background") == nil { throw ToolError(message: "background must be a boolean.") }
        if let reason = RetainedImagePolicy.refusal(tool: "content_aware_fill", arguments: settings.arguments(layer: layer), document: document) {
            throw ToolError(message: reason)
        }
        return (document, layer, mask, settings)
    }
}

enum ContentAwareFillComparison {
    /// Display-only reduction; the candidate's native document and projection
    /// remain untouched and are the only values eligible for publication.
    static func png(_ image: CGImage) throws -> Data {
        let divisor = max(1, Double(max(image.width, image.height)) / 1600)
        let width = max(1, Int(Double(image.width) / divisor)), height = max(1, Int(Double(image.height) / divisor))
        guard let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
            bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
            throw DoctoredCoreError(message: "Could not allocate the comparison display.")
        }
        context.interpolationQuality = .high
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        guard let displayed = context.makeImage() else { throw DoctoredCoreError(message: "Could not render the comparison display.") }
        let bytes = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(bytes, UTType.png.identifier as CFString, 1, nil) else {
            throw DoctoredCoreError(message: "Could not encode the comparison display.")
        }
        CGImageDestinationAddImage(destination, displayed, nil)
        guard CGImageDestinationFinalize(destination) else { throw DoctoredCoreError(message: "Could not finish the comparison display.") }
        return bytes as Data
    }
}
