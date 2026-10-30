import AppKit

// The Indexed Color document's core wrappers (#1 Indexed phase 1a): every `dr_*` call of the
// Indexed area, since phase 1c Image ▸ Mode ▸ Color Table's and the `.act` file's too, since
// phase 2d Paste and Paste Special's, and an Indexed Color PSD's open (#1). The gate
// that decides what an Indexed document admits is `IndexedDocument.swift`; Image ▸ Mode's job is
// `EditorViewController+CmykMode.swift`'s (`CmykModeConversion`'s indexed arms,
// `EditorViewController+IndexedMode.swift`).

/// `DR_INDEXED_SOURCE_*`: which container a file opens as an Indexed Color document from.
enum IndexedSource: Int32 {
    case none = 0, png = 1, gif = 2, tiff = 3
}

extension RasterPrecisionDocument {
    /// Which indexed container `url` is, header work only (`dr_path_indexed_source`).
    static func indexedSource(at url: URL) -> IndexedSource {
        IndexedSource(rawValue: dr_path_indexed_source(url.path)) ?? .none
    }

    private static func indexedJSON(_ object: [String: Any]) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8) else { return "{}" }
        return text
    }

    /// An indexed PNG, GIF or palette TIFF as an Indexed Color document, its palette, transparent
    /// index and every index kept (`dr_precision_doc_open_indexed`).
    static func openIndexed(url: URL, workingLimit: UInt64, otherRetained: UInt64,
                            control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_open_indexed(url.path, workingLimit, otherRetained,
                                                         control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not open the indexed file."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// An Indexed Color (colour mode 2) PSD or PSB's stored composite as an Indexed Color
    /// document (`import_precision` `"indexed"`, #1), still the file `fingerprint` names
    /// (`dr_precision_doc_open_indexed_psd`).
    static func openIndexedPSD(at url: URL, fingerprint: String, workingLimit: UInt64,
                               otherRetained: UInt64,
                               control: RasterJobControl? = nil) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let handle = dr_precision_doc_open_indexed_psd(url.path, fingerprint, workingLimit,
            otherRetained, control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not import PSD."))
        }
        return RasterPrecisionDocument(owning: handle)
    }

    /// Image ▸ Mode ▸ Indexed Color's plan: `working_memory_plan_bytes`, `refusal`,
    /// `flatten_required`, `flattens` (`dr_precision_doc_convert_to_indexed_plan`).
    func convertToIndexedPlan(options: [String: Any]) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.indexedJSON(options)
        return try precisionJSON(dr_precision_doc_convert_to_indexed_plan(ptr, text, &error), error)
    }

    /// Image ▸ Mode ▸ Indexed Color: a new Indexed Color document
    /// (`dr_precision_doc_convert_to_indexed`).
    func convertToIndexed(options: [String: Any], workingLimit: UInt64, otherRetained: UInt64,
                          control: RasterJobControl?) throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.indexedJSON(options)
        guard let result = dr_precision_doc_convert_to_indexed(ptr, text, workingLimit, otherRetained,
                                                               control?.ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to Indexed Color."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Image ▸ Mode ▸ RGB Color on an Indexed document: the RGB document its storage is
    /// (`dr_precision_doc_convert_indexed_to_rgb`).
    func convertIndexedToRGB() throws -> RasterPrecisionDocument {
        var error: UnsafeMutablePointer<CChar>?
        guard let result = dr_precision_doc_convert_indexed_to_rgb(ptr, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not convert to RGB Color."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// Image ▸ Mode ▸ Color Table's table (`dr_precision_doc_indexed_color_table`).
    func indexedColorTable() throws -> IndexedColorTable {
        var error: UnsafeMutablePointer<CChar>?
        return try IndexedColorTable(json: precisionJSON(dr_precision_doc_indexed_color_table(ptr, &error),
                                                         error))
    }

    /// The Color Table edit's plan: `working_memory_plan_bytes`, `refusal`, `changes`
    /// (`dr_precision_doc_set_color_table_plan`).
    func setColorTablePlan(_ table: IndexedColorTable) throws -> [String: Any] {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.indexedJSON(table.json)
        return try precisionJSON(dr_precision_doc_set_color_table_plan(ptr, text, &error), error)
    }

    /// Image ▸ Mode ▸ Color Table: a new Indexed document with `table`, every index kept; nil when
    /// it equals the document's (`dr_precision_doc_set_color_table`).
    func setColorTable(_ table: IndexedColorTable, workingLimit: UInt64, otherRetained: UInt64,
                       control: RasterJobControl?) throws -> RasterPrecisionDocument? {
        var error: UnsafeMutablePointer<CChar>?
        let text = Self.indexedJSON(table.json)
        guard let result = dr_precision_doc_set_color_table(ptr, text, workingLimit, otherRetained,
                                                            control?.ptr, &error) else {
            if error == nil { return nil }
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not set the color table."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// The table JSON the core reads (shared with `IndexedColorTable`'s file I/O).
    static func colorTableJSON(_ table: IndexedColorTable) -> String { indexedJSON(table.json) }

    /// Paste and Paste Special's plan on an Indexed document: `retained_bytes`, `scratch_bytes`
    /// (`dr_precision_doc_indexed_paste_plan`, phase 2d).
    func indexedPastePlan(_ payload: RasterClipboardPayload, request: String) throws -> [String: Any] {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        return try precisionJSON(dr_precision_doc_indexed_paste_plan(ptr, payload.ptr, request, &error),
                                 error)
    }

    /// Paste and Paste Special on an Indexed document: the payload merged into the one layer and
    /// re-indexed, masked by `coverage` (the canvas's samples, at `outsideCoverage`) unless it is
    /// empty (`dr_precision_doc_indexed_paste`, phase 2d).
    func indexedPasting(_ payload: RasterClipboardPayload, request: String, coverage: [UInt16],
                        outsideCoverage: UInt8, workingLimit: UInt64, otherRetained: UInt64,
                        control: RasterJobControl?) throws -> RasterPrecisionDocument {
        try precisionCString(request, maximumBytes: 64 * 1024)
        var error: UnsafeMutablePointer<CChar>?
        let result = coverage.withUnsafeBufferPointer { samples in
            dr_precision_doc_indexed_paste(ptr, payload.ptr, request,
                                           samples.isEmpty ? nil : samples.baseAddress, samples.count,
                                           outsideCoverage, workingLimit, otherRetained,
                                           control?.ptr, &error)
        }
        guard let result else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Paste failed."))
        }
        return RasterPrecisionDocument(owning: result)
    }

    /// The palette index at `(x, y)`; nil off the canvas or on any other document
    /// (`dr_precision_doc_indexed_index_at`).
    func indexedIndex(x: Int, y: Int) -> Int? {
        guard x >= 0, y >= 0 else { return nil }
        let index = dr_precision_doc_indexed_index_at(ptr, UInt32(clamping: x), UInt32(clamping: y))
        return index < 0 ? nil : Int(index)
    }
}

extension IndexedColorTable {
    /// The table in the `.act` file at `url` (768 or 772 bytes; `dr_indexed_read_act`).
    static func readAct(url: URL) throws -> IndexedColorTable {
        var error: UnsafeMutablePointer<CChar>?
        return try IndexedColorTable(json: precisionJSON(dr_indexed_read_act(url.path, &error), error))
    }

    /// Writes the table to `url` as a 772-byte `.act` (`dr_indexed_write_act`).
    func writeAct(to url: URL) throws {
        var error: UnsafeMutablePointer<CChar>?
        let text = RasterPrecisionDocument.colorTableJSON(self)
        guard dr_indexed_write_act(url.path, text, &error) else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not write the color table."))
        }
    }
}
