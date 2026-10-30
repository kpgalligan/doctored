import Foundation

/// The File Info wrappers (`FileInfo.swift`): the core's getter and
/// setter at either depth, with the value crossing as JSON.
extension RasterDocument {
    /// The creator, copyright notice, description and keywords the document's
    /// metadata states. Empty fields when the core answers nothing.
    var fileInfo: FileInfo {
        let raw = precision.map { dr_precision_doc_file_info($0.ptr) } ?? dr_doc_file_info(ptr)
        guard let raw else { return FileInfo() }
        defer { dr_string_free(raw) }
        let object = (try? JSONSerialization.jsonObject(with: Data(String(cString: raw).utf8)))
            as? [String: Any]
        return object.flatMap { try? FileInfo.decode($0) } ?? FileInfo()
    }

    /// Sets the four fields. nil means no change: the metadata already says
    /// exactly this. A refusal (a value past the core's limits, an XMP packet
    /// it cannot edit) throws the core's sentence. Pixels never change.
    func settingFileInfo(_ info: FileInfo) throws -> RasterDocument? {
        let json = String(
            decoding: try JSONSerialization.data(withJSONObject: info.json), as: UTF8.self)
        var err: UnsafeMutablePointer<CChar>? = nil
        if let precision {
            let result = dr_precision_doc_set_file_info(precision.ptr, json, &err)
            if err != nil {
                throw DoctoredCoreError(
                    message: takeErrorMessage(err, fallback: "Could not set the file info."))
            }
            return try result.map {
                try RasterDocument(precision: RasterPrecisionDocument(owning: $0))
            }
        }
        return try fallible(
            dr_doc_set_file_info(ptr, json, &err), err, "Could not set the file info.")
    }
}
