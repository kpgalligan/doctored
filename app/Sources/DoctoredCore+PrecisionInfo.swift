import Foundation

extension RasterPrecisionDocument {
    /// The inspection tree (`dr_precision_doc_info`) and the byte length of the
    /// JSON it was parsed from: the measured base of the typed facade's
    /// retained lease (`PrecisionFacadeMetadata.capture`).
    func infoMeasured() throws -> (info: [String: Any], jsonBytes: Int) {
        var error: UnsafeMutablePointer<CChar>?
        let result = dr_precision_doc_info(ptr, &error)
        let bytes = result.map { strlen($0) } ?? 0
        return (try precisionJSON(result, error), bytes)
    }
}
