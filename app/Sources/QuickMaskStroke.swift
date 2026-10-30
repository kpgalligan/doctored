import AppKit

/// Canonical source-over coverage, after pressure, smoothing and timed dabs.
/// RLE stores little-endian UInt32 run lengths followed by premultiplied gray
/// and alpha bytes, or two u16 codes in the additive native encoding.
/// Zero-alpha runs preserve the existing selection exactly.
struct QuickMaskStroke {
    let canvasWidth: Int
    let canvasHeight: Int
    let runs: Data
    let native: Bool

    var nativeBrush: NativeBrushStroke? = nil
    var byteBrush: NativeBrushStroke? = nil
    var byteSize: CGFloat = 1
    var brushOpacity: Double = 1
    var brushErases = false

    init(nativeBrush: NativeBrushStroke, width: Int, height: Int, opacity: Double, erase: Bool) throws {
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height,
              opacity.isFinite, (0...1).contains(opacity) else {
            throw DoctoredCoreError(message: "Invalid native Quick Mask dimensions or opacity.")
        }
        canvasWidth = width; canvasHeight = height; native = true; runs = Data()
        self.nativeBrush = nativeBrush; brushOpacity = opacity; brushErases = erase
    }

    init(byteBrush: NativeBrushStroke, size: CGFloat, width: Int, height: Int,
         opacity: Double, erase: Bool) throws {
        let pixels = try BytePaintRasterizer.pixels(stroke: byteBrush, size: size,
            ink: .color(erase ? .black : .white), width: width, height: height,
            space: ColorProfile.sRGB, selection: nil, coverageOpacity: opacity)
        self = try pixels.withUnsafeBufferPointer { buffer in
            guard let base = buffer.baseAddress else {
                throw DoctoredCoreError(message: "Quick Mask byte overlay is unavailable.")
            }
            return QuickMaskStroke(overlay: base, width: width, height: height, scale: 1)
        }
        self.byteBrush = byteBrush
        byteSize = size
        brushOpacity = opacity
        brushErases = erase
    }

    init(overlay: UnsafePointer<UInt8>, width: Int, height: Int, scale: CGFloat) {
        canvasWidth = width
        canvasHeight = height
        native = false
        let factor = max(0, min(255, Int((scale * 255).rounded())))
        var encoded = Data()
        var count: UInt32 = 0
        var gray: UInt8 = 0
        var alpha: UInt8 = 0
        func appendRun() {
            guard count > 0 else { return }
            encoded.append(contentsOf: [UInt8(count & 255), UInt8((count >> 8) & 255),
                                       UInt8((count >> 16) & 255), UInt8(count >> 24), gray, alpha])
        }
        for index in 0..<(width * height) {
            let a = UInt8(Int(overlay[index * 4 + 3]) * factor / 255)
            let g = a == 0 ? 0 : UInt8(Int(overlay[index * 4]) * factor / 255)
            if count > 0, (g != gray || a != alpha || count == UInt32.max) {
                appendRun()
                count = 0
            }
            gray = g
            alpha = a
            count += 1
        }
        appendRun()
        runs = encoded
    }

    /// Validate every run and the total before allocating a coverage buffer.
    init(width: Int, height: Int, base64: String, native: Bool = false) throws {
        let runSize = native ? 8 : 6
        guard width > 0, height > 0, width <= 100_000_000 / height,
              base64.utf8.count <= width * height * (native ? 12 : 8),
              let data = Data(base64Encoded: base64), !data.isEmpty, data.count % runSize == 0
        else { throw DoctoredCoreError(message: "Invalid Quick Mask coverage encoding or dimensions.") }
        var pixels = 0
        let valid = data.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) in
            for offset in stride(from: 0, to: bytes.count, by: runSize) {
                let count = Self.count(bytes, offset)
                guard count > 0, count <= width * height - pixels,
                      Self.components(bytes, offset, native: native).gray <= Self.components(bytes, offset, native: native).alpha
                else { return false }
                pixels += count
            }
            return pixels == width * height
        }
        guard valid else {
            throw DoctoredCoreError(message: "Quick Mask runs must cover the canvas exactly with gray ≤ alpha.")
        }
        canvasWidth = width
        canvasHeight = height
        self.native = native
        runs = data
    }

    private static func components(_ bytes: UnsafeRawBufferPointer, _ offset: Int, native: Bool) -> (gray: UInt16, alpha: UInt16) {
        if native {
            return (UInt16(bytes[offset + 4]) | UInt16(bytes[offset + 5]) << 8,
                    UInt16(bytes[offset + 6]) | UInt16(bytes[offset + 7]) << 8)
        }
        return (UInt16(bytes[offset + 4]) * 257, UInt16(bytes[offset + 5]) * 257)
    }

    private static func count(_ bytes: UnsafeRawBufferPointer, _ offset: Int) -> Int {
        Int(bytes[offset]) | (Int(bytes[offset + 1]) << 8)
            | (Int(bytes[offset + 2]) << 16) | (Int(bytes[offset + 3]) << 24)
    }

    func applying(to selection: [UInt8]?) throws -> [UInt8] {
        guard !native else { throw DoctoredCoreError(message: "Native Quick Mask uses the u16 selection transfer.") }
        var result = selection ?? [UInt8](repeating: 0, count: canvasWidth * canvasHeight)
        guard RasterSelection.paintQuickMask(&result, width: canvasWidth, height: canvasHeight, runs: runs) else {
            throw DoctoredCoreError(message: "Quick Mask coverage was refused; the selection is unchanged.")
        }
        return result
    }

    func applyingNative(to selection: [UInt16]?) throws -> [UInt16] {
        let count = canvasWidth * canvasHeight
        if let nativeBrush {
            _ = try RasterResources.shared.admit(scratch: UInt64(count) * 6 + 4096)
            let base = try PrecisionScalarBuffer(samples: selection ?? [UInt16](repeating: 0, count: count),
                width: canvasWidth, height: canvasHeight)
            let coverage = try nativeBrush.coverage(width: canvasWidth, height: canvasHeight,
                selection: nil, opacity: brushOpacity)
            let input = try PrecisionScalarBuffer.joining([base, coverage])
            return try NativeSelectionWork(width: canvasWidth, height: canvasHeight,
                operation: ["kind": "paint", "value": brushErases ? 0 : 65535], input: input.samples).run(title: "Quick Mask Stroke")
        }
        _ = try RasterResources.shared.admit(scratch: UInt64(count) * 24 + 65536)
        let original = selection ?? [UInt16](repeating: 0, count: count)
        guard original.count == count else { throw DoctoredCoreError(message: "Quick Mask selection extent changed.") }
        var gray = [UInt16](); gray.reserveCapacity(count)
        var alpha = [UInt16](); alpha.reserveCapacity(count)
        runs.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) in
            for offset in stride(from: 0, to: bytes.count, by: native ? 8 : 6) {
                let components = Self.components(bytes, offset, native: native)
                let length = Self.count(bytes, offset)
                gray.append(contentsOf: repeatElement(components.gray, count: length))
                alpha.append(contentsOf: repeatElement(components.alpha, count: length))
            }
        }
        return try NativeSelectionWork(width: canvasWidth, height: canvasHeight,
            operation: ["kind": "paint"], input: original + gray + alpha).run(title: "Quick Mask Stroke")
    }

    var actionSteps: [ActionStep] {
        get throws {
        if let nativeBrush {
            return try .step("quick_mask_stroke", ["width": canvasWidth, "height": canvasHeight,
                "native_stroke": nativeBrush.canonical, "opacity": brushOpacity, "erase": brushErases],
                note: "Select ▸ Quick Mask stroke")
        }
        if let byteBrush {
            return try .step("quick_mask_stroke", ["width": canvasWidth, "height": canvasHeight,
                "native_stroke": byteBrush.canonical, "size": Double(byteSize),
                "opacity": brushOpacity, "erase": brushErases],
                note: "Select ▸ Quick Mask stroke")
        }
        return .step("quick_mask_stroke", ["width": canvasWidth, "height": canvasHeight,
                                    (native ? "coverage_rle16" : "coverage_rle"): runs.base64EncodedString()],
              note: "Select ▸ Quick Mask stroke")
        }
    }
}
