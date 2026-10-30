import AppKit
import CryptoKit

/// The export workspace's MCP half — the twins of File ▸ Export… and File ▸
/// Quick Export, plus the preset library's four preference tools.
///
/// `export_preview` and `export_image` encode through the SAME
/// `ExportCapture` the sheet does, so the `encoded_sha256` a preview reports
/// is the sha256 of the file `export_image` writes. None of these tools
/// changes a document: no edit, no undo step, no dirty flag, and — unlike
/// the sheet's successful Save — none of the five per-document save
/// preferences (`save_copy`'s precedent: read as defaults only).
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let exportHandlers: [String: ToolHandler] = [
        // The export workspace (AgentServer+Export.swift): the twins of
        // File > Export… and File > Quick Export, plus the preset library.
        "export_preview": { $0.exportPreview },
        "export_image": { $0.exportImage },
        "quick_export": { $0.quickExport },
        "list_export_presets": { $0.listExportPresets },
        "save_export_preset": { $0.saveExportPreset },
        "delete_export_preset": { $0.deleteExportPreset },
        "set_default_export_preset": { $0.setDefaultExportPreset },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let exportToolFacts: [String: ToolFacts] = [
        // The export workspace writes files and edits a preference library.
        // `quick_export` IS a step, but the shared `ImageDocument.quickExport`
        // records the RESOLVED settings/destination/pattern itself, so the
        // generic agent-call recorder must not add a second, name-only copy.
        // The export workspace produces a deliverable from either depth's
        // own projection (ExportWorkspace.swift). The preset library reads no
        // document, so its four tools are in `documentFreeTools`.
        // The export-preset and metadata-policy libraries read no document,
        // so their tools are `DocumentPrecision.documentFreeTools`.
        // #547 phase 5: a CMYK document exports to sRGB (or keeps CMYK for
        // JPEG and TIFF with profile "document", `cmyk_export`).
        "export_preview": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "export_image": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "quick_export": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        // None of these handlers resolves a document.
        "list_export_presets": [.nonRecordable, .documentFree],
        "save_export_preset": [.nonRecordable, .documentFree],
        "delete_export_preset": [.nonRecordable, .documentFree],
        "set_default_export_preset": [.nonRecordable, .documentFree],
    ]

    private static let carriedNames: [(UInt32, String)] = [
        (DR_CARRIES_PROFILE, "profile"),
        (DR_CARRIES_EXIF, "exif"),
        (DR_CARRIES_XMP, "xmp"),
        (DR_CARRIES_IPTC, "iptc"),
        (DR_CARRIES_RESOLUTION, "resolution"),
    ]

    private func carried(_ report: RasterSaveReport) -> [String] {
        Self.carriedNames.filter { report.carries($0.0) }.map(\.1)
    }

    /// The settings object every export tool takes, with the document's own
    /// current defaults filled in for anything omitted.
    private func settings(_ a: [String: Any], document: ImageDocument) throws -> ExportSettings {
        guard let raw = a["settings"] else {
            throw ToolError(message: "Provide a settings object; see the tool schema.")
        }
        guard var object = raw as? [String: Any] else {
            throw ToolError(message: "settings must be an object.")
        }
        if object["format"] == nil {
            object["format"] = ExportSettings.defaults(for: document).format.requestName
        }
        let settings: ExportSettings
        do {
            var decoded = try ExportSettings.decode(object)
            // A named library policy is RESOLVED here, so the request the core
            // parses — and any step recorded from it — carries the policy
            // itself rather than a name that could later change.
            if let policy = stringArg(a, "metadata_policy") {
                decoded.metadata = try MetadataPolicyLibrary.named(policy).policy
            }
            settings = decoded
        } catch { throw ToolError(message: error.localizedDescription) }
        return settings
    }

    private func digest(_ data: Data) -> String {
        SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    private func encode(_ document: ImageDocument, _ settings: ExportSettings)
        throws -> (ExportCapture, RasterExportResult) {
        do {
            let capture = try ExportCapture(document)
            return (capture, try capture.run(settings, title: "Exporting"))
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    private func report(_ result: RasterExportResult) throws -> [String: Any] {
        let info = try result.info()
        let bytes = result.bytes
        var fields: [String: Any] = [
            "width": (info["width"] as? NSNumber)?.intValue ?? 0,
            "height": (info["height"] as? NSNumber)?.intValue ?? 0,
            "bytes": bytes.count,
            "encoded_sha256": digest(bytes),
            "format": info["format"] as? String ?? "",
            "profile": info["profile"] as? String ?? "",
            "bit_depth": (info["bit_depth"] as? NSNumber)?.intValue ?? 8,
            "carried": carried(try result.saveReport()),
            "metadata": info["metadata"] ?? NSNull(),
        ]
        // A 32-bit float deliverable's sample form: f32, f16 or rgbe.
        if let samples = info["float_samples"] as? String { fields["float_samples"] = samples }
        // A 32-bit HDR HEIC's encoding and content headroom (v1.x).
        if let hdr = info["hdr_encoding"] as? String { fields["hdr_encoding"] = hdr }
        if let headroom = info["content_headroom"] { fields["content_headroom"] = headroom }
        return fields
    }

    /// Mirrors the File ▸ Export… sheet's preview pane: the picture is
    /// decoded from the ACTUAL encoded bytes, and `encoded_sha256` is the
    /// sha256 the written file will have.
    func exportPreview(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        let settings = try settings(a, document: document)
        let (capture, result) = try encode(document, settings)
        // #547 Duotone phase 1b: a Duotone document's export is RGB through its inks; a byte
        // document's spot channels are composited into it (`SpotChannels.noting`).
        var fields = SpotChannels.noting(DuotoneDocument.noting(try report(result), for: capture.source),
                                         for: capture.source)
        let space = ColorProfile.space(for: settings.profile.profileData(for: capture.source))
        guard let decoded = try? result.preview(),
              let tagged = decoded.makeCGImage(in: space),
              let srgb = Bitmap.sRGBCopy(of: tagged),
              let png = NSBitmapImageRep(cgImage: srgb).representation(using: .png, properties: [:])
        else {
            throw ToolError(message: "Could not decode the encoded export for preview.")
        }
        fields["ok"] = true
        let data = try JSONSerialization.data(withJSONObject: fields)
        return try callResult(
            content: [
                ["type": "image", "data": png.base64EncodedString(), "mimeType": "image/png"],
                ["type": "text", "text": String(decoding: data, as: UTF8.self)],
            ], isError: false)
    }

    /// Mirrors the export sheet's Save: encode once and write those bytes.
    /// An existing path is REFUSED unless `overwrite` is true.
    func exportImage(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try commitPendingCanvasSessions(document)
        let settings = try settings(a, document: document)
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        let (capture, result) = try encode(document, settings)
        do { try result.write(to: url, overwrite: boolArg(a, "overwrite") ?? false) }
        catch { throw ToolError(message: error.localizedDescription) }
        var fields = SpotChannels.noting(DuotoneDocument.noting(try report(result), for: capture.source),
                                         for: capture.source)
        fields["ok"] = true
        fields["path"] = url.path
        // The encode's admitted plan (`ExportCapture.scratchEstimate`), the figure the
        // measured-admission drives hold (#547 phase 5's CMYK exports).
        fields["working_memory_plan_bytes"] = (try? capture.scratchEstimate(settings)) ?? NSNull()
        return try jsonResult(fields)
    }

    /// Mirrors File ▸ Quick Export. A recorded step carries the RESOLVED
    /// settings, destination and pattern, so replay never depends on a
    /// mutable library name; `preset` alone is the interactive spelling, and
    /// no arguments at all is the menu command itself: the library default,
    /// else the built-in PNG preset (`QuickExportDefaults.swift`).
    ///
    /// A call WITHOUT `settings` is the interactive one, so a taken name gets
    /// a `-2`, `-3`, … suffix as the menu's does. A call WITH `settings` is
    /// the resolved form a recorded step and a batch delivery make, and it
    /// keeps the refusal: replay and batch never write a second copy.
    func quickExport(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try commitPendingCanvasSessions(document)
        let collision: QuickExportCollision = a["settings"] == nil ? .suffix : .refuse
        let preset: ExportPreset
        do {
            if let name = stringArg(a, "preset"), a["settings"] == nil {
                preset = try ExportPresetLibrary.named(name)
            } else if ["preset", "settings", "destination", "pattern"].allSatisfy({ a[$0] == nil }) {
                preset = try ExportPresetLibrary.quickExportPreset(for: document)
            } else {
                let settings = try settings(a, document: document)
                preset = ExportPreset(
                    name: stringArg(a, "preset") ?? "quick_export",
                    settings: settings,
                    destination: try requiredString(a, "destination"),
                    pattern: try requiredString(a, "pattern"))
            }
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        try AgentPathGuard.shared.check(
            exportPreset: preset, argument: "export preset \"\(preset.name)\"")
        let index = intArg(a, "index") ?? 1
        let count = intArg(a, "count") ?? 1
        let policy: MetadataPolicy?
        do {
            policy = try stringArg(a, "metadata_policy")
                .map { try MetadataPolicyLibrary.named($0).policy }
        } catch { throw ToolError(message: error.localizedDescription) }
        let outcome: QuickExportOutcome
        do {
            outcome = try document.quickExport(preset: preset, policy: policy,
                                               ordinal: (index, count), collision: collision)
        } catch { throw ToolError(message: error.localizedDescription) }
        var fields = SpotChannels.noting(DuotoneDocument.noting(try report(outcome.result), for: document.doc),
                                         for: document.doc)
        fields["ok"] = true
        fields["path"] = outcome.url.path
        fields["preset"] = outcome.preset
        // Only when the pattern's own name was taken and a suffix was applied.
        if outcome.url != outcome.requested { fields["requested_path"] = outcome.requested.path }
        return try jsonResult(fields)
    }

    /// Mirrors File ▸ Export… ▸ Manage…: the library is a preference, so
    /// listing it changes no document.
    func listExportPresets(_ a: [String: Any]) throws -> String {
        do {
            let listing = try ExportPresetLibrary.listing()
            return try jsonResult([
                "presets": listing.entries.map { $0.item.json },
                "broken": listing.broken.map(\.json),
                "default_preset": try ExportPresetLibrary.defaultPresetName() ?? NSNull(),
                "maximum_presets": ExportPresetLibrary.maximumPresets,
                "maximum_file_bytes": ExportPresetLibrary.maximumFileBytes,
                "tokens": ExportNaming.tokenHelp,
                "collision": "quick_export never overwrites: by preset name or with no arguments "
                    + "a taken name gets -2, -3, …; settings + destination + pattern refuses an "
                    + "existing path",
            ])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Mirrors File ▸ Export… ▸ Save Preset….
    func saveExportPreset(_ a: [String: Any]) throws -> String {
        let name = try requiredString(a, "name")
        let destination = try requiredString(a, "destination")
        let pattern = try requiredString(a, "pattern")
        guard let object = a["settings"] as? [String: Any] else {
            throw ToolError(message: "save_export_preset needs a settings object.")
        }
        do {
            let preset = ExportPreset(name: name, settings: try ExportSettings.decode(object),
                                      destination: destination, pattern: pattern,
                                      metadataPolicy: stringArg(a, "metadata_policy"))
            let saved = try ExportPresetLibrary.save(preset, replace: boolArg(a, "replace") ?? false)
            return try jsonResult(["ok": true, "preset": saved.json])
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Mirrors File ▸ Export… ▸ Manage… ▸ Delete.
    func deleteExportPreset(_ a: [String: Any]) throws -> String {
        let hasName = stringArg(a, "name") != nil, hasFile = stringArg(a, "file") != nil
        guard hasName != hasFile else {
            throw ToolError(message: "delete_export_preset takes exactly one of name or file — a broken entry has no name.")
        }
        do {
            if hasFile {
                let file = try ExportPresetLibrary.validatedFileName(requiredString(a, "file"))
                try ExportPresetLibrary.delete(file: file)
                return try jsonResult(["ok": true, "file": file])
            }
            let preset = try ExportPresetLibrary.named(requiredString(a, "name"))
            let file = try ExportPresetLibrary.listing().entries.first { $0.item.name == preset.name }?.file
            try ExportPresetLibrary.delete(name: preset.name)
            var reply: [String: Any] = ["ok": true]
            if let file { reply["file"] = file }
            return try jsonResult(reply)
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Mirrors File ▸ Export… ▸ Manage… ▸ Set as Quick Export. A null name
    /// clears it, after which File ▸ Quick Export uses the built-in PNG
    /// preset again.
    func setDefaultExportPreset(_ a: [String: Any]) throws -> String {
        let name = a["name"] is NSNull ? nil : stringArg(a, "name")
        do { try ExportPresetLibrary.setDefault(name: name) }
        catch let error as ToolError { throw error }
        catch { throw ToolError(message: error.localizedDescription) }
        return try jsonResult(["ok": true, "default_preset": name ?? NSNull()])
    }

    @inline(never) func exportWorkspaceCatalog() -> [[String: Any]] {
        let settings: [String: Any] = [
            "type": "object",
            "description":
                "The export request: format (png|jpeg|tiff|bmp|gif|webp|heic|avif|jxl|exr|hdr), jpeg_quality "
                + "1–100 (it is also HEIC's and AVIF's quality dial), "
                + "bit_depth 8|16|32 (16 is png/tiff/heic/avif on a 16-bit document; HEIC and AVIF are "
                + "encoded by the host, so the RESULT's bit_depth is the depth that was "
                + "actually written, 8 or 10; jxl is lossless JPEG XL, 8-bit only, written as "
                + "sRGB (set profile srgb when the document is not sRGB), each side at least "
                + "2 px, carrying no profile or metadata; 32 is the float deliverable a 32-bit "
                + "float document writes and requires format tiff, exr or hdr (or heic/avif with "
                + "hdr_encoding gain_map|pq, the HDR photo deliverable), no sharpen, "
                + "profile document and dither none — exact at size mode none, resampled "
                + "through Image Size's extended-range resample otherwise; exr (OpenEXR: RGBA, "
                + "ZIP, associated alpha) and hdr (Radiance RGBE: opaque, each colour multiplied "
                + "by its alpha, values below zero written as zero) exist ONLY at bit_depth 32, "
                + "convert no pixel, declare the document profile's primaries in their header "
                + "and carry no ICC profile or metadata), float_samples f32|f16 (default f32; "
                + "f16 writes the nearest 16-bit half and requires bit_depth 32 and tiff or exr; "
                + "the result's float_samples reports f32, f16 or rgbe), dither none|ordered, "
                + "size {mode:none|width|height|long_edge|scale, value}, filter "
                + "nearest|bilinear|catmull-rom|lanczos3, profile "
                + "document|srgb|display-p3|file|installed (file takes profile_path, installed "
                + "takes profile_name — see list_installed_profiles), intent "
                + "perceptual|relative|saturation|absolute (default relative), "
                + "black_point_compensation (default false), "
                + "sharpen {radius 0–64, amount 0–5, threshold 0–1} (Unsharp Mask; method "
                + "\"unsharp_mask\" may be given) or {method:\"smart_sharpen\", and any of "
                + "apply_filter's smart_sharpen keys at their defaults: amount 1–500 % (200), "
                + "radius 0.1–64 px (1), noise 0–100 (10), remove gaussian|lens|motion (lens), "
                + "angle (0), shadow_/highlight_fade 0–100 (0), _tonal_width 0–100 (50), "
                + "_radius 1–100 px (1)} or null, embed_profile, "
                + "strip_metadata, and metadata {exif|xmp|iptc|density: default|keep|drop, "
                + "stamp:{author,copyright}|null}. Omitted fields keep today's export behaviour.",
        ]
        let metadataPolicy: [String: Any] = [
            "type": "string",
            "description":
                "Name of a saved metadata policy (list_metadata_policies). Resolved before the "
                + "encode, so a recorded step carries the policy itself. It overrides "
                + "settings.metadata.",
        ]
        let name: [String: Any] = [
            "type": "string", "description": "Export preset name, 1–120 characters.",
        ]
        let destination: [String: Any] = [
            "type": "string",
            "description": "An existing destination folder. It is never created.",
        ]
        let pattern: [String: Any] = [
            "type": "string",
            "description": "Naming pattern; {name} is required. Tokens: " + ExportNaming.tokenHelp,
        ]
        return [
            tool("export_preview",
                 "File > Export…: encode the document with these settings and return the picture "
                 + "DECODED FROM THE ENCODED BYTES plus the exact byte size and encoded_sha256. "
                 + "Writes nothing and changes no document state.",
                 ["document_id": Self.docIDProperty, "settings": settings,
                  "metadata_policy": metadataPolicy], required: ["settings"]),
            tool("export_image",
                 "File > Export… > Save: encode and write the file. The bytes are the bytes "
                 + "export_preview decoded, so encoded_sha256 matches. An existing path is "
                 + "refused unless overwrite is true. The document (mode, depth, profile, layers, "
                 + "selection, undo, dirty state) is unchanged.",
                 ["document_id": Self.docIDProperty, "path": ["type": "string"],
                  "settings": settings, "metadata_policy": metadataPolicy,
                  "overwrite": ["type": "boolean", "default": false]],
                 required: ["path", "settings"]),
            tool("quick_export",
                 "File > Quick Export: write the document into a preset's destination folder "
                 + "under its naming pattern, with no dialog. Give preset (the library name), "
                 + "or settings + destination + pattern, or none of the four: then it uses the "
                 + "Quick Export default (list_export_presets.default_preset), and with no "
                 + "default set a built-in preset — a PNG named {name} beside the document's "
                 + "own file, or in ~/Downloads for a document with no file. It NEVER "
                 + "overwrites: by preset name or with no arguments a taken name gets -2, -3, "
                 + "… and the result adds requested_path (the name that was taken) beside "
                 + "path; settings + destination + pattern refuses an existing path, which is "
                 + "the form a recorded step and a batch use. This is recordable as an Action "
                 + "step, because its name comes from a pattern that must contain {name}; the "
                 + "step carries the resolved settings, destination and pattern. index and "
                 + "count feed {index} (1-based, zero-padded to count's digit width), which is "
                 + "how a batch names its outputs.",
                 ["document_id": Self.docIDProperty, "preset": name, "settings": settings,
                  "destination": destination, "pattern": pattern,
                  "metadata_policy": metadataPolicy,
                  "index": ["type": "integer", "default": 1],
                  "count": ["type": "integer", "default": 1]]),
            tool("list_export_presets",
                 "File > Export… > Manage…: list the bounded versioned export-preset library and "
                 + "the Quick Export default. Presets live outside every document. A file that is "
                 + "not a readable preset is reported as a `broken` row — {file, code, reason, "
                 + "repairable} with code one of unreadable, oversize, not_json, unknown_version, "
                 + "invalid_schema, duplicate_name — instead of refusing the whole library; repair "
                 + "is RAW-only, so repairable is always false here.", [:]),
            tool("save_export_preset",
                 "File > Export… > Save Preset…: store settings, a destination folder and a "
                 + "naming pattern under a name. A library write is a preference: no Undo step, "
                 + "no Action step, and no document changes because a preset changed. A "
                 + "settings profile FILE the export cannot convert into is refused here with "
                 + "the export's own sentence.",
                 ["name": name, "settings": settings, "destination": destination,
                  "pattern": pattern, "metadata_policy": metadataPolicy,
                  "replace": ["type": "boolean", "default": false]],
                 required: ["name", "settings", "destination", "pattern"]),
            tool("delete_export_preset",
                 "File > Export… > Manage… > Delete: remove ONE library file. Pass exactly one of "
                 + "name (a listed preset) or file (any library file, including a broken one that "
                 + "has no name); passing both or neither is refused. Recorded Quick Export steps "
                 + "keep their resolved settings.",
                 ["name": name, "file": libraryFileProperty("list_export_presets")]),
            tool("set_default_export_preset",
                 "File > Export… > Manage… > Set as Quick Export: choose the preset File > Quick "
                 + "Export uses, or null to clear it (after which Quick Export uses its "
                 + "built-in PNG preset).",
                 ["name": ["type": ["string", "null"],
                           "description": "Preset name, or null to clear the default."]]),
        ]
    }
}
