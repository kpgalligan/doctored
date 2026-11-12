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
            // #1 Bitmap phase 1c: the same answer Save As asks (`ImageDocument.saveLosses`).
            if doc.isBitmap {
                formats[format.fileExtension] = try BitmapDocument.saveLosses(doc, format: format,
                    embedProfile: document.embedColorProfile, stripMetadata: document.stripMetadata)
                continue
            }
            // #1 Indexed phase 1b: the same answer Save As asks (`ImageDocument.saveLosses`).
            if doc.isIndexed {
                formats[format.fileExtension] = try IndexedDocument.saveLosses(doc, format: format,
                    embedProfile: document.embedColorProfile, stripMetadata: document.stripMetadata)
                continue
            }
            // A Grayscale document's PNG, TIFF and JPEG, a CMYK document's TIFF and JPEG, a Lab
            // Color document's TIFF and a 16- or 32-bit RGB document's every flat-save format:
            // the same answer Save asks (`ImageDocument.saveLosses`), so a type is in
            // `save_types` exactly when its entry here is empty.
            if (doc.isGray && !doc.isDuotone && [.png, .tiff, .jpeg].contains(format))
                || (doc.isCmyk && [.tiff, .jpeg].contains(format))
                || (doc.isLab && format == .tiff)
                || (doc.isTypedRGB && ExportFormat.flatSaveCases.contains(format)) {
                formats[format.fileExtension] = try document.saveLosses(for: format.utType.identifier)
                continue
            }
            if doc.precision != nil {
                // The "16 only for png/tiff" rule gained HEIC in Phase 11D
                // and AVIF (`doc_export.rs`'s `validate`); this list
                // and the export sheet's must agree with it.
                let bits = format.isFloatOnly ? 32
                    : format == .png || format == .tiff || format == .heic
                    || format == .avif ? 16 : 8
                formats[format.fileExtension] = try PrecisionExportCapture.losses(doc, format: format,
                    bitDepth: bits, embedProfile: document.embedColorProfile,
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
