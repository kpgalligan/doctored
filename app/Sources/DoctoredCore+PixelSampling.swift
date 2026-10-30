import AppKit

// MARK: - Composite pixel sampling (eyedropper)

extension RasterImage {
    /// The straight (non-premultiplied) RGBA of one pixel, read in place
    /// from the handle's pixel buffer (row 0 = top, matching the FFI
    /// convention); nil outside the image. The buffer behind a handle never
    /// changes, so this is safe and O(1) — used by the editor's eyedropper
    /// and the agent's sample_color, both against the flattened composite.
    func pixelRGBA(x: Int, y: Int) -> (r: UInt8, g: UInt8, b: UInt8, a: UInt8)? {
        let w = width
        guard x >= 0, y >= 0, x < w, y < height,
              let pixels = dr_image_pixels_rgba(ptr)
        else { return nil }
        let offset = (y * w + x) * 4
        return (pixels[offset], pixels[offset + 1], pixels[offset + 2], pixels[offset + 3])
    }

    /// Proves a pixel replacement is a no-op without copying the core buffer.
    func matchesRGBA(_ rgba: [UInt8], width: Int, height: Int) -> Bool {
        guard self.width == width, self.height == height, rgba.count == width * height * 4,
              let pixels = dr_image_pixels_rgba(ptr) else { return false }
        return rgba.withUnsafeBufferPointer {
            UnsafeBufferPointer(start: pixels, count: $0.count).elementsEqual($0)
        }
    }

    /// Canonical hex form of a sampled pixel: #RRGGBB, with the alpha byte
    /// appended (#RRGGBBAA) only when the pixel is not fully opaque —
    /// matching the color syntax the paint tools accept.
    static func hexString(_ rgba: (r: UInt8, g: UInt8, b: UInt8, a: UInt8)) -> String {
        let base = String(format: "#%02X%02X%02X", rgba.r, rgba.g, rgba.b)
        return rgba.a == 255 ? base : base + String(format: "%02X", rgba.a)
    }
}
