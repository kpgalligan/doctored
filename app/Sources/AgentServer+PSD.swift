import AppKit

extension AgentServer {
    /// This file's MCP tools; merged by AgentServer+Registry.swift.
    static let psdHandlers: [String: ToolHandler] = [
        "inspect_psd": inspectPSD,
    ]

    /// Facts for this file's tools (recording, selection, depth admission); merged in AgentServer+Registry.swift.
    static let psdToolFacts: [String: ToolFacts] = [
        "inspect_psd": [.nonRecordable, .documentFree],
    ]

    /// Mirrors the compatibility notice shown by File > Open, without
    /// creating a document or showing a modal dialog on the MCP callback.
    func inspectPSD(_ arguments: [String: Any]) throws -> String {
        let path = try requiredString(arguments, "path")
        let url = URL(fileURLWithPath: (path as NSString).expandingTildeInPath)
        return try jsonResult(RasterDocument.psdImportReport(at: url))
    }

    /// The PSD import mode `open_document` scopes its read with. A PSD or PSB
    /// requires an explicit choice. A TIFF carrying Photoshop's layer block is
    /// a flat TIFF first: without `psd_mode` it opens flat exactly as any
    /// other TIFF does (`.refuse` makes `openLayeredTIFF` answer nil), and
    /// with `psd_mode` it is treated as a PSD, fingerprint included.
    func psdOpenRequest(_ arguments: [String: Any], url: URL) throws -> PSDImportMode {
        let isPSD = ImageDocument.isPSD(at: url)
        let layeredTIFF = !isPSD && ImageDocument.isLayeredTIFF(at: url)
        guard isPSD || layeredTIFF else {
            if arguments["psd_mode"] != nil || arguments["psd_fingerprint"] != nil {
                throw ToolError(message: "psd_mode and psd_fingerprint apply only to Photoshop PSD "
                    + "and PSB files and to TIFFs with Photoshop layers.")
            }
            // If the file becomes a PSD between sniff and read, refuse;
            // never open an interactive compatibility dialog inside MCP.
            return .refuse
        }
        if layeredTIFF, arguments["psd_mode"] == nil {
            return .refuse
        }
        guard let mode = stringArg(arguments, "psd_mode"),
              let choice = PSDImportChoice(rawValue: mode) else {
            throw ToolError(message: "PSD requires compatibility preflight before editing. Call inspect_psd, "
                + "then open_document with psd_mode: layers or composite and its source_fingerprint as psd_fingerprint.")
        }
        // `open_document` returns an already-open document as it is, and a
        // TIFF opened flat has no `psdImport` for that reply to report, so
        // the layers request would be dropped in silence. Refuse it by name.
        if layeredTIFF, choice == .layers,
           let existing = NSDocumentController.shared.document(for: url) as? ImageDocument,
           existing.psdImport == nil {
            throw ToolError(message: "\(url.lastPathComponent) is already open flat, as an ordinary TIFF. "
                + "Close that document, then call open_document with psd_mode: layers again.")
        }
        guard let fingerprint = stringArg(arguments, "psd_fingerprint"), !fingerprint.isEmpty else {
            throw ToolError(message: "PSD import requires psd_fingerprint from inspect_psd; inspect the file first.")
        }
        return .explicit(choice, fingerprint: fingerprint)
    }
}
