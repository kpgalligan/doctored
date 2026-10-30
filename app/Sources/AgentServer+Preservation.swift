import AppKit

extension AgentServer {
    /// get_document mirrors File > Save/Save As format availability. Copies
    /// remain explicit exports and never mark the editing document saved.
    static func preservationFields(_ document: ImageDocument) throws -> [String: Any] {
        guard let doc = document.doc else { throw ToolError(message: "No image loaded") }
        var formats: [String: Any] = [ImageDocument.nativeFormatName: [String]()]
        for format in ExportFormat.allCases {
            // OpenEXR and Radiance are 32-bit float deliverables: only
            // a float document can write one, so only it lists them.
            if format.isFloatOnly && !doc.isFloat { continue }
            // #547 Bitmap phase 1c: the same answer Save As asks (`ImageDocument.saveLosses`).
            if doc.isBitmap {
                formats[format.fileExtension] = BitmapDocument.saveLosses(doc, format: format,
                    embedProfile: document.embedColorProfile, stripMetadata: document.stripMetadata)
                continue
            }
            // #547 Indexed phase 1b: the same answer Save As asks (`ImageDocument.saveLosses`).
            if doc.isIndexed {
                formats[format.fileExtension] = IndexedDocument.saveLosses(doc, format: format,
                    embedProfile: document.embedColorProfile, stripMetadata: document.stripMetadata)
                continue
            }
            if doc.precision != nil {
                // The "16 only for png/tiff" rule gained HEIC in Phase 11D
                // and AVIF (`doc_export.rs`'s `validate`); this list
                // and the export sheet's must agree with it.
                let bits = format.isFloatOnly ? 32
                    : format == .png || format == .tiff || format == .heic
                    || format == .avif ? 16 : 8
                formats[format.fileExtension] = PrecisionExportCapture.losses(format: format,
                    bitDepth: bits, isFloat: doc.isFloat, embedProfile: document.embedColorProfile,
                    stripMetadata: document.stripMetadata)
                continue
            }
            formats[format.fileExtension] = try doc.flatSaveLosses(
                format: format.drFormat, embedProfile: document.embedColorProfile,
                stripMetadata: document.stripMetadata)
        }
        return [
            "losses_by_format": formats,
            "save_types": document.writableTypes(for: .saveOperation),
            "save_as_types": document.writableTypes(for: .saveAsOperation),
        ]
    }
}
