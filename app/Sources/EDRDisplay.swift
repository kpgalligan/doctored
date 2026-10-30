import AppKit
import QuartzCore

// The EDR (extended dynamic range) facts the canvas needs (v1.x): how much headroom a screen
// can give, and the core's binary16 extended-range view (`DoctoredCore+HDR.swift`) as a CGImage. A
// screen's CURRENT headroom moves with brightness and ambient light; the engage rule reads the
// POTENTIAL headroom (the panel's capability), and CoreAnimation clips whatever the current
// headroom cannot show.

enum EDRDisplay {
    /// A test instance's stand-in for the screen's potential headroom: a
    /// positive number replaces `maximumPotentialExtendedDynamicRangeColorComponentValue`
    /// (the jobs-probe and the drive set 4, so an SDR test Mac still
    /// engages); unset, unparsable or not above 0 changes nothing.
    static let headroomOverrideVariable = "DR_EDR_HEADROOM"

    /// The override, else what `screen` reports; 1 when there is no screen.
    static func potentialHeadroom(of screen: NSScreen?) -> Double {
        if let text = ProcessInfo.processInfo.environment[headroomOverrideVariable],
           let value = Double(text), value.isFinite, value > 0 {
            return value
        }
        guard let screen else { return 1 }
        return Double(screen.maximumPotentialExtendedDynamicRangeColorComponentValue)
    }

    /// A backing layer's configuration for extended-range content: a
    /// 16-bit float backing store that keeps values above 1, EDR content
    /// on, and no tone mapping — values past the screen's current headroom
    /// clip, as the user asked to see the samples. Off restores RGBA8 and
    /// SDR. The editor canvas and the Develop canvas both use it.
    static func configure(_ layer: CALayer, extended: Bool) {
        if extended {
            layer.contentsFormat = .RGBA16Float
            layer.wantsExtendedDynamicRangeContent = true
            layer.toneMapMode = .never
            if #available(macOS 26, *) { layer.preferredDynamicRange = .high }
        } else {
            layer.contentsFormat = .RGBA8Uint
            layer.wantsExtendedDynamicRangeContent = false
            layer.toneMapMode = .automatic
            if #available(macOS 26, *) { layer.preferredDynamicRange = .standard }
        }
    }

    /// The core's extended-range view — RGBA binary16 little-endian,
    /// premultiplied, LINEAR light in the document primaries — as a 16-bit
    /// float CGImage in `documentSpace`'s extended-linearized twin, so
    /// CoreGraphics does the display encode and keeps values above 1. nil
    /// when the space has no linearized form or the buffer is the wrong size.
    static func image(halfFloat: Data, width: Int, height: Int,
                      documentSpace: CGColorSpace) -> CGImage? {
        guard width > 0, height > 0, halfFloat.count == width * height * 8,
              let space = CGColorSpaceCreateExtendedLinearized(documentSpace),
              let provider = CGDataProvider(data: halfFloat as CFData) else { return nil }
        let info = CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue
            | CGBitmapInfo.floatComponents.rawValue | CGBitmapInfo.byteOrder16Little.rawValue)
        return CGImage(width: width, height: height, bitsPerComponent: 16, bitsPerPixel: 64,
                       bytesPerRow: width * 8, space: space, bitmapInfo: info, provider: provider,
                       decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    }
}
