import AppKit

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let psdExportHandlers: [String: ToolHandler] = [
        "inspect_psd_export": { $0.inspectPSDExport },
        "export_psd": { $0.exportPSD },
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let psdExportToolFacts: [String: ToolFacts] = [
        // The PSD export writes at the DOCUMENT's own depth, so both tools
        // answer in 16-bit mode too (`PSDExport.swift`).
        // A float document writes a 32-bit PSD or PSB, and a
        // 32-bit layered TIFF; both PSD tools reach the handler, and the
        // CORE's sentence answers what refuses at 32 bits (a fill, a style
        // that would bake, an adjustment with no Photoshop record).
        "inspect_psd_export": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
        "export_psd": [.nonRecordable, .commonTool, .floatTool, .cmykTool],
    ]

    /// Mirrors the report the File > Export as PSD… panel shows, without
    /// opening a panel or writing anything.
    func inspectPSDExport(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        guard let source = document.doc else {
            throw ToolError(message: "Document has no image")
        }
        let mode = try psdExportMode(a)
        let container = try psdExportContainer(a)
        do {
            return try jsonResult(
                source.psdExportReport(container: container, mode: mode.coreMode))
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
    }

    /// Mirrors File > Export as PSD… after the save panel: the same
    /// capture, the same admission, the same writer.
    func exportPSD(_ a: [String: Any]) throws -> String {
        let document = try target(a)
        try commitPendingCanvasSessions(document)
        let path = try requiredString(a, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        let mode = try psdExportMode(a)
        let container = try psdExportContainer(a)
        let capture: PSDExportCapture
        do {
            capture = try PSDExportCapture(document, container: container, mode: mode)
        } catch {
            // An unavailable mode answers with the core's own refusal, so a
            // model can choose the other mode or fix the document.
            throw ToolError(message: error.localizedDescription)
        }
        do {
            _ = try RasterResources.shared.admit(scratch: capture.scratchEstimate)
            try capture.run(to: url)
        } catch {
            throw ToolError(message: error.localizedDescription)
        }
        return try jsonResult([
            "ok": true, "path": url.path, "mode": mode.rawValue,
            "format": container.rawValue,
            "bit_depth": capture.bitDepth,
            "losses": capture.report.losses,
            "observations": capture.report.observations,
            "worst_case_bytes": NSNumber(value: capture.report.worstCaseBytes),
            // The admitted plan (`PSDExportCapture.scratchEstimate`), the figure the
            // measured-admission drives hold (#1 Duotone phase 1b's `export-duotone-psd16`).
            "working_memory_plan_bytes": NSNumber(value: capture.scratchEstimate),
        ])
    }

    private func psdExportMode(_ a: [String: Any]) throws -> PSDExportMode {
        guard let name = stringArg(a, "mode") else { return .layers }
        guard let mode = PSDExportMode(rawValue: name) else {
            throw ToolError(message: "Unknown PSD export mode “\(name)”; use layers or flattened.")
        }
        return mode
    }

    private func psdExportContainer(_ a: [String: Any]) throws -> LayeredContainer {
        guard let name = stringArg(a, "format") else { return .psd }
        guard let container = LayeredContainer(rawValue: name) else {
            throw ToolError(message: "Unknown layered export format “\(name)”; use psd, psb or tiff.")
        }
        return container
    }

    @inline(never)
    func psdExportCatalog() -> [[String: Any]] {
        let format: [String: Any] = [
            "type": "string", "enum": ["psd", "psb", "tiff"],
            "description": "psd (default) writes a classic PSD (30000 px per side, 2 GiB); psb a Large "
                + "Document PSB (300000 px per side); tiff a flat TIFF with Photoshop's layer block.",
        ]
        let mode: [String: Any] = [
            "type": "string", "enum": ["layers", "flattened"],
            "description": "layers (default) writes the editable stack; flattened writes one composited raster layer.",
        ]
        let documentID: [String: Any] = [
            "type": "integer", "description": "Document id from list_documents; omit for the active document.",
        ]
        return [
            tool(
                "inspect_psd_export",
                "Preflight for writing this document in Adobe Photoshop format (PSD, PSB or a TIFF "
                    + "with layers): a PSD, a Large Document PSB or a TIFF with "
                    + "layers (format) at the document's depth "
                    + "(8-, 16- or 32-bit RGB), the report "
                    + "File > Export as PSD… shows: available, refusal, per-document losses and "
                    + "observations, general export limitations, layer/group counts and the worst-case "
                    + "file size. Writes nothing and reads no pixels. A 32-bit float document reports "
                    + "depth 32: a PSD, PSB or TIFF with layers is available, and its "
                    + "layers mode refuses a gradient or pattern fill layer, a style that would be "
                    + "rasterized or an adjustment layer with no Photoshop record (flattened always "
                    + "writes); a Solid Color fill layer is written as Photoshop's SoCo. The reply's "
                    + "format and container name the file (PSD, PSB or TIFF), and bake_scratch_bytes "
                    + "is the working memory any baked adjustment layers need.",
                ["document_id": documentID, "mode": mode, "format": format]),
            tool(
                "export_psd",
                "Writes this document in Adobe Photoshop format (PSD, PSB or a TIFF with layers): "
                    + "a classic RGB PSD, a Large Document PSB or a TIFF with "
                    + "layers (format; the result echoes it) at the document's depth (8-, 16- or 32-bit RGB; "
                    + "the result reports bit_depth) (File > Export as PSD…): raster "
                    + "layers, ordinary masks, groups, clipping, opacity, blend keys, ICC and PPI, with a "
                    + "white-matted composite preview. Fails with inspect_psd_export's refusal when the "
                    + "mode is unavailable; returns the losses and observations it could not avoid. "
                    + "A 32-bit float document writes a 32-bit PSD, PSB or TIFF with layers "
                    + "(linear-light float samples, Raw channels, records in Lr32; the TIFF's flat "
                    + "image is float, Macintosh byte order; a Solid Color fill layer is Photoshop's "
                    + "SoCo, and style and fill colours are linear light x 255); in layers mode a "
                    + "gradient or pattern fill layer, a style that would be rasterized or an "
                    + "adjustment layer with no Photoshop record is refused with the core's sentence. "
                    + "Layer styles are written as Photoshop layer effects and blending options "
                    + "(lfx2, iOpa, blending ranges) and reported, and are baked into their layer "
                    + "only when Photoshop cannot hold them; knockout and Blend Interior Effects "
                    + "as Group ride that style as Photoshop's own knko and infx records at both "
                    + "depths, and a baked style writes neither; invert, threshold, "
                    + "posterize, levels, curves, color_balance, hue_saturation, photo_filter, "
                    + "exposure, vibrance, black_and_white, channel_mixer, selective_color and bcs "
                    + "(without saturation) adjustment layers "
                    + "are written as Photoshop adjustment records (parameters mapped, "
                    + "appearance in Photoshop not qualified) and every other adjustment layer is "
                    + "written as a pixel layer holding the picture up to that layer, reported as a "
                    + "loss; editable text, RAW layers' camera originals, alpha channels, "
                    + "guides and metadata packets are not written; layer comps are written in "
                    + "layers mode with their names, comments, capture flags and each layer's "
                    + "visibility, recorded offset, opacity, fill, blend mode and style (a style "
                    + "Photoshop's effects cannot hold is reported and written without it); the "
                    + "document's paths "
                    + "are written in every mode, and in layers mode a vector mask with no raster "
                    + "mask beside it is written as its path (every other vector mask as its "
                    + "rendered mask); in layers mode a bottom layer that is the Background "
                    + "(get_document's background, or an unmarked opaque Background a written "
                    + "knockout reaches) is written as Photoshop's Background layer; spot channels "
                    + "are written to PSD and PSB at 8 and 16 bits (not Lab), and to a layered "
                    + "TIFF from RGB and CMYK at 8 and 16 bits as its trailing extra samples (a "
                    + "Grayscale, Duotone, Lab or 32-bit TIFF refuses); layer locks are written as "
                    + "Photoshop's lspf.",
                ["path": ["type": "string", "description": "Absolute or ~ path to write. Overwrites atomically."],
                 "mode": mode, "format": format, "document_id": documentID],
                required: ["path"]),
        ]
    }
}
