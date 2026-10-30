import Foundation

// MARK: - Authored colour on a LUT-tagged document

/// The core's one sRGB ↔ document decision for an AUTHORED colour on a
/// document tagged with an RGB LUT profile (`dr_authored_color`): the
/// relative-colorimetric, BPC-off pipeline the core's layer styles, artwork
/// and high-depth `srgb` ink take, so a brush, Fill, Gradient or text colour
/// lands on the same numbers as a Color Overlay of the same hex.
enum RasterAuthoredColor {
    /// `rgb` (three components in 0…1, clamped by the core) converted from
    /// sRGB into `profile` (`toDocument` true) or out of it (false). nil when
    /// `profile` has a matrix/TRC model (ColorSync converts those), is not
    /// RGB, or cannot convert both ways.
    static func convert(_ rgb: [Float], profile: Data, toDocument: Bool) -> [Float]? {
        guard rgb.count == 3, !profile.isEmpty else { return nil }
        var out: [Float] = [0, 0, 0]
        let converted = profile.withUnsafeBytes { raw -> Bool in
            guard let bytes = raw.bindMemory(to: UInt8.self).baseAddress else { return false }
            return rgb.withUnsafeBufferPointer { input in
                out.withUnsafeMutableBufferPointer { output in
                    dr_authored_color(
                        bytes, raw.count, toDocument, input.baseAddress, output.baseAddress)
                }
            }
        }
        return converted ? out : nil
    }
}
