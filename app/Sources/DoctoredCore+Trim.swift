import Foundation

/// The core's Image ▸ Trim scan (`core/src/trim_bounds.rs`): the
/// rectangle left after the checked edges move inward past every row and
/// column that is transparent, or that exactly equals a corner pixel. nil
/// when every pixel is trimmable. The model types are in CanvasTrim.swift.
extension RasterImage {
    func trimRect(_ basis: TrimBasis, _ edges: TrimEdges) -> CGRect? {
        var rect = [UInt32](repeating: 0, count: 4)
        let found = rect.withUnsafeMutableBufferPointer {
            dr_image_trim_rect(ptr, DrTrimBasis(rawValue: UInt32(basis.rawValue)),
                               UInt32(edges.rawValue), $0.baseAddress)
        }
        guard found else { return nil }
        return CGRect(x: Int(rect[0]), y: Int(rect[1]), width: Int(rect[2]), height: Int(rect[3]))
    }
}

extension RasterPrecisionImage {
    /// The same scan over a 16-bit or float image: exact codes at 16 bits,
    /// bit patterns at float.
    func trimRect(_ basis: TrimBasis, _ edges: TrimEdges) -> CGRect? {
        var rect = [UInt32](repeating: 0, count: 4)
        let found = rect.withUnsafeMutableBufferPointer {
            dr_precision_image_trim_rect(ptr, DrTrimBasis(rawValue: UInt32(basis.rawValue)),
                                         UInt32(edges.rawValue), $0.baseAddress)
        }
        guard found else { return nil }
        return CGRect(x: Int(rect[0]), y: Int(rect[1]), width: Int(rect[2]), height: Int(rect[3]))
    }
}

extension RasterPrecisionDocument {
    /// The scan over a CMYK document's composite ink (#547 phase 4b): a corner
    /// basis compares all five codes. The composite's scratch is admitted
    /// against `workingLimit` beside `otherRetained`; nil when every pixel is
    /// trimmable, a throw when refused.
    func cmykTrimRect(_ basis: TrimBasis, _ edges: TrimEdges, workingLimit: UInt64,
                      otherRetained: UInt64) throws -> CGRect? {
        var rect = [UInt32](repeating: 0, count: 4)
        var error: UnsafeMutablePointer<CChar>?
        let found = rect.withUnsafeMutableBufferPointer {
            dr_precision_doc_cmyk_trim_rect(ptr, DrTrimBasis(rawValue: UInt32(basis.rawValue)),
                                            UInt32(edges.rawValue), workingLimit, otherRetained,
                                            $0.baseAddress, &error)
        }
        if let error {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Could not trim the ink."))
        }
        guard found else { return nil }
        return CGRect(x: Int(rect[0]), y: Int(rect[1]), width: Int(rect[2]), height: Int(rect[3]))
    }
}
