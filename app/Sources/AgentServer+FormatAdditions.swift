import AppKit

// `export_pdf` and `inspect_vector` are `nonRecordable` and `notAStepTool` (a literal path, and a
// read).

/// The MCP half of Phase 11D's format additions: the twin of File ▸ Export
/// as PDF…, and the inspection a PDF/SVG open is driven from.
///
/// HEIC needs no tool of its own — it is a `format` the existing export tools
/// take, and the result's `bit_depth` and `carried` report what the host's
/// encoder actually wrote.
extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let formatAdditionsHandlers: [String: ToolHandler] = [
        // Format additions (AgentServer+FormatAdditions.swift): the twin of
        // File > Export as PDF… and the PDF/SVG inspection the open uses.
        "export_pdf": { $0.exportPDF },
        "inspect_vector": { $0.inspectVector },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let formatAdditionsToolFacts: [String: ToolFacts] = [
        // Phase 11D: a PDF export writes a literal path (export_image's
        // reason) and a vector inspection only reads.
        // File ▸ Export as PDF… writes an 8- or 16-bit page from
        // either depth's own projection (`PDFExport.swift`).
        // File ▸ Export as PDF… writes the tone-mapped projection at
        // 8 or 16 bits per component, exactly as `export_image` does.
        "export_pdf": [.nonRecordable, .commonTool, .floatTool],
        "inspect_vector": [.nonRecordable, .documentFree],
    ]

    /// Mirrors File ▸ Export as PDF…: one flattened page at the document's
    /// own print size, its one image at 8 or 16 bits (`bit_depth`) and ZIP or
    /// JPEG (`compression`, the accessory's two popups). Takes every
    /// `export_image` setting but `format`, which a PDF page fixes. JPEG is the
    /// system (ImageIO) encoder, so `jpeg_quality` is not byte-comparable with
    /// `export_image`'s own JPEG at the same number.
    func exportPDF(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try commitPendingCanvasSessions(document)
        guard document.doc != nil else { throw ToolError(message: "Document has no image") }
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)

        var object = (a["settings"] as? [String: Any]) ?? [:]
        if object["format"] != nil {
            throw ToolError(
                message: "export_pdf fixes format: a PDF page holds one image. Remove the format "
                    + "key; choose bit_depth and compression instead.")
        }
        let compressionName = stringArg(a, "compression") ?? PDFCompression.zip.rawValue
        guard let compression = PDFCompression(rawValue: compressionName) else {
            throw ToolError(message: "export_pdf compression must be zip or jpeg.")
        }
        // The PDF's own defaults, never the export workspace's: a float
        // document's workspace default is 32 bits, which no page holds.
        let defaults = PDFExportOptions.defaults(for: document)
        object["format"] = "tiff"
        if object["bit_depth"] == nil { object["bit_depth"] = defaults.bitDepth }
        if object["jpeg_quality"] == nil { object["jpeg_quality"] = defaults.jpegQuality }
        let settings: ExportSettings
        let options: PDFExportOptions
        do {
            var decoded = try ExportSettings.decode(object)
            if object["dither"] == nil {
                decoded.dither = decoded.bitDepth == 8
                    ? ExportSettings.defaults(for: document).dither : "none"
            }
            if let policy = stringArg(a, "metadata_policy") {
                decoded.metadata = try MetadataPolicyLibrary.named(policy).policy
            }
            settings = decoded
            options = PDFExportOptions(
                compression: compression, bitDepth: decoded.bitDepth,
                jpegQuality: decoded.jpegQuality)
            try options.validate()
        } catch { throw ToolError(message: error.localizedDescription) }

        let base = document.displayName ?? "Untitled"
        do {
            let capture = try ExportCapture(document)
            // Mirrors File ▸ Export as PDF…: the same worker, progress and Cancel.
            let (data, report) = try PDFExport.run(
                capture, settings: settings, options: options, title: base)
            try PDFExport.write(data, to: url, overwrite: boolArg(a, "overwrite") ?? false)
            // #547 Duotone phase 1b: a Duotone document's page is RGB through its inks.
            return try jsonResult(DuotoneDocument.noting([
                "ok": true,
                "path": url.path,
                "bytes": report.bytes,
                "width": report.width,
                "height": report.height,
                "page_size_points": [report.pageSizePoints.width, report.pageSizePoints.height],
                "ppi": [report.ppi.x, report.ppi.y],
                "profile_name": report.profileName,
                "carried": report.carried & DR_CARRIES_PROFILE != 0 ? ["profile"] : [String](),
                "bits_per_component": report.bitsPerComponent,
                "compression": report.compression,
                "has_soft_mask": report.hasSoftMask,
            ], for: capture.source))
        } catch let error as ToolError {
            throw error
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// What a PDF or SVG on disk is, before opening it: the query the page
    /// popup is built from, and the one a caller asks before choosing
    /// `open_document`'s `page` and `ppi`.
    func inspectVector(_ a: [String: Any]) throws -> String {
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw ToolError(message: "No file at \(url.path)")
        }
        guard VectorImport.claims(url) else {
            throw ToolError(message: "\(url.lastPathComponent) is not a PDF or an SVG.")
        }
        do {
            let kind = try VectorImport.inspect(url)
            return try jsonResult([
                "ok": true,
                "path": url.path,
                "kind": kind.name,
                "pages": kind.pages,
                "page_sizes_points": kind.pageSizes.map { [$0.width, $0.height] },
                "default_ppi": VectorImport.defaultPPI,
                "warnings": VectorImport.warnings(kind),
            ])
        } catch { throw ToolError(message: error.localizedDescription) }
    }

    /// The two catalog entries, kept beside their handlers.
    static func formatAdditionsCatalog() -> [[String: Any]] {
        [
            [
                "name": "export_pdf",
                "description": "Export the flattened document as a single-page PDF at its own "
                    + "print size, with its colour profile embedded (File ▸ Export as PDF…). "
                    + "Takes every export_image setting but format: the page holds one image at "
                    + "bit_depth 8 or 16 (default: the document's remembered export depth, capped "
                    + "at 16; an 8-bit document exports at 8), compressed as zip (lossless, the "
                    + "default) or jpeg (8-bit only, the system ImageIO encoder at jpeg_quality, "
                    + "so not byte-comparable with export_image's JPEG). Transparency travels as "
                    + "a soft mask. A 32-bit float document writes its tone-mapped projection. "
                    + "The result's bits_per_component, compression and has_soft_mask are read "
                    + "back from the written file.",
                "inputSchema": [
                    "type": "object",
                    "properties": [
                        "document_id": ["type": "integer"],
                        "path": ["type": "string", "description": "Where to write the PDF."],
                        "overwrite": ["type": "boolean", "default": false],
                        "compression": [
                            "type": "string", "enum": ["zip", "jpeg"], "default": "zip",
                            "description": "How the page's image is compressed; jpeg is 8-bit only.",
                        ],
                        "settings": [
                            "type": "object",
                            "description": "export_image settings without format; bit_depth 8 "
                                + "or 16.",
                        ],
                        "metadata_policy": [
                            "type": "string",
                            "description": "A saved metadata policy name, resolved at delivery.",
                        ],
                    ],
                    "required": ["path"],
                ],
            ],
            [
                "name": "inspect_vector",
                "description": "Report what a PDF or SVG file is — its page count and each page's "
                    + "size in points — without opening it. Use it before open_document's page "
                    + "and ppi.",
                "inputSchema": [
                    "type": "object",
                    "properties": ["path": ["type": "string"]],
                    "required": ["path"],
                ],
            ],
        ]
    }
}
