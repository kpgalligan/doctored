import AppKit

/// One geometry/drawing plan for legacy colored rasters and typed scalar
/// coverage. A text plan retains TextKit objects and is main-thread-only; it
/// must be consumed before passing plain coverage values to a worker.
struct DescribedDrawingPlan {
    let width: Int
    let height: Int
    let offsetX: Int
    let offsetY: Int
    let appKit: Bool
    let draw: (CGContext) -> Bool

    func render(space: CGColorSpace) -> DescribedRaster? {
        guard let pixels = Bitmap.renderStraightRGBA(width: width, height: height,
            appKit: appKit, space: space, draw) else { return nil }
        return DescribedRaster(pixels: pixels, width: width, height: height,
                                offsetX: offsetX, offsetY: offsetY)
    }

    /// Call only with opaque drawing colors: this plane represents geometry,
    /// with authored color/opacity applied separately in the typed core.
    func coverage() -> [UInt8]? {
        Bitmap.renderCoverage(width: width, height: height, appKit: appKit, draw)
    }
}

extension Bitmap {
    /// An alpha-only bitmap is scalar coverage, never a colored pixel source.
    /// It uses the same flipped placement and AppKit context as the established
    /// described-layer renderer. Coverage has 256 possible values, even when later
    /// promoted exactly to u16; promotion does not create extra edge precision.
    static func renderCoverage(width: Int, height: Int, appKit: Bool = false,
                               _ draw: (CGContext) -> Bool) -> [UInt8]? {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              !appKit || Thread.isMainThread else { return nil }
        var coverage = [UInt8](repeating: 0, count: width * height)
        let drawn = coverage.withUnsafeMutableBufferPointer { buffer -> Bool in
            guard let context = CGContext(data: buffer.baseAddress, width: width, height: height,
                bitsPerComponent: 8, bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(),
                bitmapInfo: CGImageAlphaInfo.alphaOnly.rawValue) else { return false }
            return drawFlipped(context, height: height, appKit: appKit, draw)
        }
        return drawn ? coverage : nil
    }

    /// Shared orientation/AppKit bridge for both described-raster pixel formats.
    static func drawFlipped(_ context: CGContext, height: Int, appKit: Bool,
                            _ draw: (CGContext) -> Bool) -> Bool {
        context.translateBy(x: 0, y: CGFloat(height))
        context.scaleBy(x: 1, y: -1)
        guard appKit else { return draw(context) }
        NSGraphicsContext.saveGraphicsState()
        defer { NSGraphicsContext.restoreGraphicsState() }
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
        return draw(context)
    }
}
