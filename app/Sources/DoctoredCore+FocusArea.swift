import AppKit

extension RasterImage {
    /// Select ▸ Focus Area's coverage over this image (`dr_selection_focus_area`,
    /// `designs/focus-area.md`): `width * height` bytes and the range the core used,
    /// Auto's resolved one when `parameters` names none.
    func focusArea(parameters: [String: Any], control: RasterJobControl?) throws -> (mask: [UInt8], range: Double) {
        let json = String(decoding: try JSONSerialization.data(withJSONObject: parameters), as: UTF8.self)
        var mask = [UInt8](repeating: 0, count: width * height)
        var range = 0.0
        var error: UnsafeMutablePointer<CChar>?
        let success = mask.withUnsafeMutableBufferPointer { buffer in
            dr_selection_focus_area(ptr, json, buffer.baseAddress, &range, control?.ptr, &error)
        }
        guard success else {
            throw DoctoredCoreError(message: takeErrorMessage(error, fallback: "Focus Area could not be computed."))
        }
        return (mask, range)
    }
}
