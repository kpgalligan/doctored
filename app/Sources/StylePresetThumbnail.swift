import AppKit

/// A style preset's swatch for the Styles panel — the preset's style
/// on a 36 px rounded square centred on a 64 × 64 light-grey canvas, rendered
/// by the core like any styled layer (`withLayerStyle`, then `flattened`), so
/// the swatch is exactly what applying the preset draws. Cached by the
/// style's JSON: two presets with one style share one image.
enum StylePresetThumbnail {
    static let canvas = 64
    static let shape = 36

    private static let cache: NSCache<NSString, NSImage> = {
        let cache = NSCache<NSString, NSImage>()
        cache.countLimit = 512
        return cache
    }()

    /// The swatch of `style`, or nil when the core could not render it.
    static func image(for style: String) -> NSImage? {
        if let cached = cache.object(forKey: style as NSString) { return cached }
        guard let rendered = render(style) else { return nil }
        cache.setObject(rendered, forKey: style as NSString)
        return rendered
    }

    private static func render(_ style: String) -> NSImage? {
        let inset = (canvas - shape) / 2
        guard let square = roundedSquare(),
              let blank = RasterDocument.blank(width: canvas, height: canvas, rgba: (236, 236, 236, 255)),
              let layered = blank.addingImageLayer(above: 0, square, name: "Swatch",
                                                   offset: (x: inset, y: inset))
        else { return nil }
        let styled = (try? layered.withLayerStyle(1, style)) ?? layered
        guard let flat = styled.flattened(), let cg = flat.makeCGImage(in: ColorProfile.sRGB)
        else { return nil }
        return NSImage(cgImage: cg, size: NSSize(width: canvas, height: canvas))
    }

    /// A mid-grey anti-aliased rounded square drawn by Core Graphics, as the
    /// STRAIGHT-alpha RGBA8 the core takes.
    private static func roundedSquare() -> RasterImage? {
        let side = shape
        var pixels = [UInt8](repeating: 0, count: side * side * 4)
        let drawn: Bool = pixels.withUnsafeMutableBytes { buffer in
            guard let context = CGContext(
                data: buffer.baseAddress, width: side, height: side, bitsPerComponent: 8,
                bytesPerRow: side * 4, space: ColorProfile.sRGB,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
            else { return false }
            context.setFillColor(CGColor(srgbRed: 0.62, green: 0.64, blue: 0.67, alpha: 1))
            let rect = CGRect(x: 0, y: 0, width: side, height: side)
            context.addPath(CGPath(roundedRect: rect, cornerWidth: 7, cornerHeight: 7, transform: nil))
            context.fillPath()
            return true
        }
        guard drawn else { return nil }
        // Un-premultiply the anti-aliased edge.
        for at in stride(from: 0, to: pixels.count, by: 4) {
            let alpha = Int(pixels[at + 3])
            guard alpha > 0, alpha < 255 else { continue }
            for channel in 0..<3 {
                pixels[at + channel] = UInt8(min(255, (Int(pixels[at + channel]) * 255 + alpha / 2) / alpha))
            }
        }
        return RasterImage.from(rgba: pixels, width: side, height: side)
    }
}
