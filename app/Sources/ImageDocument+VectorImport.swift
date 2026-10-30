import AppKit

extension ImageDocument {
    /// Opens a PDF page or an SVG as a new raster document (Phase 11D).
    ///
    /// It is an OPEN, not an edit: nothing is recorded, nothing is dirty
    /// beyond a normal open, and the page and PPI that produced the pixels
    /// are written into the metadata report so the same import can be
    /// repeated exactly.
    static func openVector(_ url: URL) throws -> Opened {
        let kind = try VectorImport.inspect(url)
        let choice: VectorImportChoice
        switch VectorImportRequest.mode {
        case .headless(let requested):
            choice = requested
        case .ask:
            guard let chosen = VectorImportSheetController.runModal(url: url, kind: kind) else {
                throw CocoaError(.userCancelled)
            }
            choice = chosen
        }
        let (width, height) = try VectorImport.pixelSize(kind, choice: choice)
        let scratch = UInt64(width) * UInt64(height) * 8 + 64 * 1024 * 1024
        _ = try RasterResources.shared.admit(scratch: scratch)
        let image = try VectorImport.rasterize(url, choice: choice)
        guard var document = RasterDocument.from(image: image) else {
            throw DoctoredCoreError(message: "Could not build a document from \(url.lastPathComponent).")
        }
        // The renderer drew into sRGB, which is the profile the document
        // therefore carries; the PPI is the one the page was rasterized at,
        // so the print size matches the page's own.
        if let tagged = try document.assigningProfile(RasterProfile.builtin(.sRGB)) {
            document = tagged
        }
        if let sized = try document.settingResolution(x: choice.ppi, y: choice.ppi) {
            document = sized
        }
        let report: [String: Any] = [
            "container": "vector_raster",
            "renderer": kind.name == "pdf" ? "coregraphics" : "coresvg",
            "metadata_walked": false,
            "representation": "rasterized_page",
            "captured_packets": [String](),
            "page": choice.page,
            "pages": kind.pages,
            "ppi": choice.ppi,
            "pixels": [width, height],
            "warnings": VectorImport.warnings(kind),
        ]
        return Opened(doc: document, metadataNotCaptured: true, rawDevelop: nil,
                      metadataReport: report)
    }
}
