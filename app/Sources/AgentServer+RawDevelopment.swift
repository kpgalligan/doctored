import AppKit
import ImageIO
import UniformTypeIdentifiers

// One expiring MCP review ticket retains the candidate; source/target/selection changes or document
// close invalidate it.
//
// Read-only native inspection uses the same histogram/detail/preview kernels as the UI.

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let rawDevelopmentHandlers: [String: ToolHandler] = [
        "inspect_raw_development": inspectRawDevelopment,
        "preview_raw_decoder_migration": previewRawDecoderMigration,
        "migrate_raw_decoder": migrateRawDecoder,
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let rawDevelopmentToolFacts: [String: ToolFacts] = [
        "inspect_raw_development": [.nonRecordable, .commonTool, .floatTool],
        "preview_raw_decoder_migration": [.nonRecordable, .commonTool, .floatTool],
        "migrate_raw_decoder": [.nonRecordable, .commonTool, .floatTool],
    ]

    /// Mirrors the Develop window's full-source histogram and before/after
    /// inspection. With no patch it reads the saved master without a decoder.
    /// `raw.output_encoding` mirrors the window's Development row; a
    /// 32-bit float (linear) master is inspected as the window shows it,
    /// clipped to SDR (`RasterPrecisionImage.developInspection`), and the
    /// reply's `hdr_preview` mirrors the window's HDR Fit preview.
    func inspectRawDevelopment(_ arguments: [String: Any]) throws -> String {
        try rawWorkflowKeys(arguments, allowed: ["document_id", "layer", "raw", "region", "clipping"])
        let document = try target(arguments)
        let capture = try ImageSourceCapture(document)
        let layer = try rawWorkflowLayer(arguments, document: document)
        _ = try RawSourceWork.info(source: capture.source, layer: layer)
        let payload = try PrecisionEditPayload.admitted(bytes: 2 * 1024 * 1024)
        defer { withExtendedLifetime(payload) {} }
        let before = try capture.source.sourceMaster(layer)
        var after = before
        var descriptor: RawSourceDescriptor?
        if let settings = try rawOpenSettings(arguments), settings != RawDevelopSettings(),
           let prepared = try RawSourceWork.prepare(source: capture.source, layer: layer, settings: settings) {
            after = try prepared.sourceMaster(layer)
            descriptor = try RawSourceDescriptor.read(RawSourceWork.info(source: prepared, layer: layer))
        }
        try capture.requireCurrent(document)
        let inspection = try rawInspection(before: before, after: after, arguments: arguments)
        try capture.requireCurrent(document)
        var report = inspection.report
        report["layer"] = layer
        report["document_revision"] = capture.revision
        report["committed"] = false
        report["proposed_descriptor"] = try descriptor.map { try JSONSerialization.jsonObject(with: Data($0.json().utf8)) }
        // The Develop window's HDR preview (#4): each f32 side's
        // extended image at the window's Fit, or at the 100 % `region`
        // (divisor 1), with `clipping`'s float markers (at or below 0.0, at
        // or above 1.0): its peak (markers excluded), divisor and marked
        // pixel count; null for a u16 side, and for a side whose extended
        // image is refused (the window falls back to SDR there, so the
        // mirror reports, never fails).
        if before.isFloatStorage || after.isFloatStorage {
            func extended(_ image: RasterPrecisionImage) -> RawDevelopExtendedImage? {
                let r = inspection.detail.map { ($0.x, $0.y, $0.width, $0.height, 1) }
                    ?? RawDevelopViewport().region(image)
                return try? RawDevelopExtendedImage.make(image, region: r, clipping: inspection.clipping)
            }
            let beforeImage = extended(before)
            let afterImage = after === before ? beforeImage : extended(after)
            func number(_ value: Any?) -> Any { value ?? NSNull() }
            report["hdr_preview"] = [
                "before_peak": number(beforeImage.map { Double($0.peak) }),
                "before_divisor": number(beforeImage?.divisor),
                "before_marked": number(beforeImage?.marked),
                "after_peak": number(afterImage.map { Double($0.peak) }),
                "after_divisor": number(afterImage?.divisor),
                "after_marked": number(afterImage?.marked)]
        }
        return try rawInspectionResult(inspection, report: report)
    }

    /// Mirrors Layer > Migrate RAW Decoder's review. One ticket lasts five
    /// minutes; a new preview replaces it and releases the prior candidate.
    func previewRawDecoderMigration(_ arguments: [String: Any]) throws -> String {
        try rawWorkflowKeys(arguments, allowed: ["document_id", "layer", "region", "clipping", "geometry"])
        let document = try target(arguments)
        let layer = try rawWorkflowLayer(arguments, document: document)
        RawMigrationReview.clear()
        let geometry: RawSourceMap?
        if let raw = arguments["geometry"] {
            guard let object = raw as? [String: Any] else {
                throw ToolError(message: "Migration geometry must be a strict EXIF or affine object.")
            }
            geometry = try JSONDecoder().decode(RawSourceMap.self,
                from: JSONSerialization.data(withJSONObject: object))
        } else { geometry = nil }
        let candidate = try RawDecoderMigration.prepare(in: document, layer: layer, geometry: geometry)
        let before = try candidate.capture.source.sourceMaster(layer)
        guard let after = candidate.asset.precision?.layerSource(0) else {
            throw ToolError(message: "Could not inspect the proposed RAW development.")
        }
        let inspection = try rawInspection(before: before, after: after, arguments: arguments)
        try candidate.capture.requireCurrent(document)
        let token = RawMigrationReview.retain(candidate, document: document)
        var report = inspection.report
        report["layer"] = layer
        report["preview_id"] = token
        report["expires_after_seconds"] = 300
        report["committed"] = false
        report["before_descriptor"] = try JSONSerialization.jsonObject(with: Data(candidate.previous.json().utf8))
        report["after_descriptor"] = try JSONSerialization.jsonObject(with: Data(candidate.descriptor.json().utf8))
        report["changes"] = candidate.changes
        report["geometry"] = try JSONSerialization.jsonObject(with: Data(candidate.geometry.json().utf8))
        report["before_width"] = before.width
        report["before_height"] = before.height
        report["after_width"] = after.width
        report["after_height"] = after.height
        report["before_masks"] = candidate.beforeMasks
        report["after_masks"] = candidate.afterMasks
        report["next_step"] = "Compare Before and After, then pass preview_id to migrate_raw_decoder to apply these exact pixels."
        return try rawInspectionResult(inspection, report: report)
    }

    func migrateRawDecoder(_ arguments: [String: Any]) throws -> String {
        try rawWorkflowKeys(arguments, allowed: ["document_id", "layer", "preview_id", "migration"])
        let document = try target(arguments)
        let candidate: RawDecoderMigration
        if let replay = arguments["migration"] {
            guard ActionPlayer.isPlaying, arguments["preview_id"] == nil,
                  let values = replay as? [String: Any],
                  Set(values.keys).isSubset(of: ["before", "after", "masks", "geometry", "after_masks"]),
                  let before = values["before"] as? String, let after = values["after"] as? String,
                  before.utf8.count <= 64 * 1024, after.utf8.count <= 64 * 1024 else {
                throw ToolError(message: "Recorded migration recipes are accepted only during Actions playback. Preview the migration first.")
            }
            let layer = try rawWorkflowLayer(arguments, document: document)
            guard let source = document.doc else { throw ToolError(message: "Document has no image.") }
            let saved = try RawSourceDescriptor.read(RawSourceWork.info(source: source, layer: layer))
            guard try saved.json() == before else {
                throw ToolError(message: "The recorded migration belongs to a different RAW source or saved recipe.")
            }
            let currentMasks = try RawLocalWork.maskFingerprints(source, layer: layer)
            if !currentMasks.isEmpty || values["masks"] != nil {
                guard let recordedMasks = values["masks"] as? [String], recordedMasks == currentMasks else {
                    throw ToolError(message: "The recorded migration's native local coverage changed. Review the new local rendering first.")
                }
            }
            let map: RawSourceMap?
            if let object = values["geometry"] as? [String: Any] {
                map = try JSONDecoder().decode(RawSourceMap.self,
                    from: JSONSerialization.data(withJSONObject: object))
            } else if values["geometry"] == nil { map = nil }
            else { throw ToolError(message: "Recorded migration geometry is invalid.") }
            candidate = try RawDecoderMigration.prepare(in: document, layer: layer, geometry: map)
            guard try candidate.descriptor.json() == after else {
                throw ToolError(message: "The recorded migration's decoder or effective recipe changed. Review a new migration before applying it.")
            }
            let replayMap = try candidate.geometry.affine(from: (candidate.capture.source.width,
                candidate.capture.source.height))
            if let afterMasks = values["after_masks"] as? [String] {
                guard afterMasks == candidate.afterMasks else {
                    throw ToolError(message: "Recorded migration output masks differ from the reviewed result.")
                }
            } else if candidate.asset.width != candidate.capture.source.width
                        || candidate.asset.height != candidate.capture.source.height
                        || replayMap != RawMigrationGeometry.identity {
                throw ToolError(message: "Legacy migration Actions only replay same-geometry sources.")
            }
        } else {
            guard let token = arguments["preview_id"] as? String else {
                throw ToolError(message: "Preview the current RAW decoder first, then provide its preview_id.")
            }
            candidate = try RawMigrationReview.take(token, document: document)
            if arguments["layer"] != nil,
               try rawWorkflowLayer(arguments, document: document) != candidate.layer {
                throw ToolError(message: "This preview belongs to a different RAW layer.")
            }
        }
        try candidate.apply(in: document)
        return try jsonResult(["ok": true, "layer": candidate.layer, "document": summary(document),
                               "note": "The reviewed decoder and rendering were applied in one Undo step."])
    }

    private struct RawInspection {
        let before: Data
        let after: Data
        let report: [String: Any]
        let payload: PrecisionEditPayload
        /// The 100 % region the request named; nil at Fit.
        let detail: (x: Int, y: Int, width: Int, height: Int)?
        let clipping: UInt32
    }

    private func rawInspection(before master: RasterPrecisionImage, after developed: RasterPrecisionImage,
                               arguments: [String: Any]) throws -> RawInspection {
        // A u16 master is read as it is; an f32 one (a 32-bit float
        // development) as its SDR-clipped u16 view.
        let before = try master.developInspection()
        let after = master === developed ? before : try developed.developInspection()
        var x = 0, y = 0, width = before.width, height = before.height
        var divisor = max(1, (max(width, height) + 1023) / 1024)
        var afterRegion = (0, 0, after.width, after.height,
            max(1, (max(after.width, after.height) + 1023) / 1024))
        if let raw = arguments["region"] {
            guard let region = raw as? [String: Any], Set(region.keys) == ["x", "y", "width", "height"] else {
                throw ToolError(message: "region requires integer x, y, width and height in source pixels.")
            }
            x = try rawWorkflowInteger(region["x"], name: "region.x")
            y = try rawWorkflowInteger(region["y"], name: "region.y")
            width = try rawWorkflowInteger(region["width"], name: "region.width")
            height = try rawWorkflowInteger(region["height"], name: "region.height")
            guard (1...1024).contains(width), (1...1024).contains(height),
                  x >= 0, y >= 0, x <= before.width - width, y <= before.height - height,
                  x <= after.width - width, y <= after.height - height else {
                throw ToolError(message: "The 100% region must fit both sources (before \(before.width)×\(before.height), after \(after.width)×\(after.height)) and be at most 1024×1024.")
            }
            divisor = 1
            afterRegion = (x, y, width, height, 1)
        }
        let modes: [String: UInt32] = ["none": 0, "shadows": 1, "highlights": 2, "both": 3]
        let mode = arguments["clipping"] as? String ?? "none"
        guard let clipping = modes[mode], arguments["clipping"] == nil || arguments["clipping"] is String else {
            throw ToolError(message: "clipping must be none, shadows, highlights or both.")
        }
        let region = (x, y, width, height, divisor)
        let budget: UInt64 = 96 * 1024 * 1024
        _ = try RasterResources.shared.admit(scratch: budget)
        let result = try BackgroundWork.wait(try WorkerTask(scratchEstimate: budget) {
            // #4: an f32 side's histogram reads its master's float samples.
            let beforeHistogram = try (master.isFloatStorage ? master : before).developHistogram()
            let afterHistogram = master === developed ? beforeHistogram
                : try (developed.isFloatStorage ? developed : after).developHistogram()
            func png(_ source: RasterPrecisionImage, crop: (Int, Int, Int, Int, Int)) throws -> Data {
                let image = try source.developPreview(x: crop.0, y: crop.1, width: crop.2,
                    height: crop.3, divisor: crop.4, clipping: clipping)
                guard let tagged = image.makeCGImage(in: ColorProfile.space(for: source.profileData)),
                      let rgb = Bitmap.sRGBCopy(of: tagged) else {
                    throw DoctoredCoreError(message: "Could not prepare the RAW inspection preview.")
                }
                let data = NSMutableData()
                guard let destination = CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil) else {
                    throw DoctoredCoreError(message: "Could not encode the RAW inspection preview.")
                }
                CGImageDestinationAddImage(destination, rgb, nil)
                guard CGImageDestinationFinalize(destination) else {
                    throw DoctoredCoreError(message: "Could not finish the RAW inspection preview.")
                }
                return data as Data
            }
            let beforePNG = try png(before, crop: region)
            return (beforePNG, before === after ? beforePNG : try png(after, crop: afterRegion),
                beforeHistogram, afterHistogram)
        }, title: "Reading RAW Detail and Histogram")
        let payload = try PrecisionEditPayload.admitted(bytes: budget)
        return RawInspection(before: result.0, after: result.1, report: [
            "before_histogram": result.2, "after_histogram": result.3,
            "source_width": before.width, "source_height": before.height,
            "before_width": before.width, "before_height": before.height,
            "after_width": after.width, "after_height": after.height,
            "region": ["x": x, "y": y, "width": width, "height": height], "preview_divisor": divisor,
            "after_preview_divisor": afterRegion.4,
            "clipping": mode,
            "scope": "Entire developed RAW source before document transforms, retouch, masks and selection; histogram visits every source pixel.",
            "preview_note": "Images are Before then After, each fit to its own source dimensions and converted to 8-bit sRGB. Normalized pan is not pixel correspondence. A 16-bit side's histogram bins and endpoint counts use original 16-bit source codes and each source's profile. Blue marks RGB zero, red marks RGB 65535, magenta both; Fit marks any endpoint in each sampled cell. Endpoint counts describe normalized output, not sensor saturation. A 32-bit float (linear) development's images are its SDR-clipped view (RGB clamped to 0…1 and encoded by its profile), so a red marker there means at or above white, not a lost highlight; its histogram reads the float samples (storage rgba32_float: stop bins over −12…+6 EV, above_white_rgba, exact_one_rgba, below_zero_rgba and exact_zero_rgba counts), and hdr_preview reports the Develop window's HDR view of it (Fit, or the 100% region at divisor 1) with clipping marking samples at or below 0.0 and at or above 1.0."],
            payload: payload,
            detail: arguments["region"] == nil ? nil : (x, y, width, height), clipping: clipping)
    }

    private func rawInspectionResult(_ inspection: RawInspection, report: [String: Any]) throws -> String {
        defer { withExtendedLifetime(inspection.payload) {} }
        let text = String(decoding: try JSONSerialization.data(withJSONObject: report, options: [.sortedKeys]), as: UTF8.self)
        return try callResult(content: [
            ["type": "image", "data": inspection.before.base64EncodedString(), "mimeType": "image/png"],
            ["type": "image", "data": inspection.after.base64EncodedString(), "mimeType": "image/png"],
            ["type": "text", "text": text]], isError: false)
    }

    func rawWorkflowKeys(_ arguments: [String: Any], allowed: Set<String>) throws {
        if let key = arguments.keys.filter({ !allowed.contains($0) }).sorted().first {
            throw ToolError(message: "Unknown RAW workflow argument: \(key).")
        }
    }
    func rawWorkflowLayer(_ arguments: [String: Any], document: ImageDocument) throws -> Int {
        if let value = arguments["layer"] { return try rawWorkflowInteger(value, name: "layer") }
        return document.activeLayerIndex
    }
    func rawWorkflowInteger(_ value: Any?, name: String) throws -> Int {
        guard let number = value as? NSNumber, !Self.isJSONBoolean(number),
              number.doubleValue.isFinite, number.doubleValue.rounded() == number.doubleValue,
              number.doubleValue >= 0, number.doubleValue <= Double(Int32.max) else {
            throw ToolError(message: "\(name) must be a nonnegative integer.")
        }
        return number.intValue
    }
}
