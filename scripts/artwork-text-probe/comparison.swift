import AppKit
import CoreText
import ImageIO

/// Independent reference: the ordinary NSLayoutManager superclass draws into
/// RGBA8. It neither extracts glyph outlines nor reads generated PDFs.
private final class TextCaptureReference: NSLayoutManager {
    let decorationsOnly: Bool
    init(decorationsOnly: Bool) { self.decorationsOnly = decorationsOnly; super.init() }
    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("TextCaptureReference does not support NSCoder") }
    override func showCGGlyphs(_ glyphs: UnsafePointer<CGGlyph>, positions: UnsafePointer<CGPoint>, count: Int,
        font: NSFont, textMatrix: CGAffineTransform, attributes: [NSAttributedString.Key: Any], in context: CGContext) {
        if !decorationsOnly {
            super.showCGGlyphs(glyphs, positions: positions, count: count, font: font,
                textMatrix: textMatrix, attributes: attributes, in: context)
        }
    }
}
private struct TextCaptureProbeError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}
private func textProbePixels(width: Int, height: Int, draw: (CGContext) throws -> Void) throws -> [UInt8] {
    var pixels = [UInt8](repeating: 0, count: width * height * 4)
    try pixels.withUnsafeMutableBytes { bytes in
        guard let space = CGColorSpace(name: CGColorSpace.sRGB), let context = CGContext(data: bytes.baseAddress,
            width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
            space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
            throw TextCaptureProbeError(message: "Reference context failed")
        }
        context.translateBy(x: 0, y: CGFloat(height)); context.scaleBy(x: 1, y: -1)
        context.setShouldAntialias(true); context.setAllowsFontSmoothing(false)
        NSGraphicsContext.saveGraphicsState(); defer { NSGraphicsContext.restoreGraphicsState() }
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
        try draw(context)
    }
    Bitmap.unpremultiply(&pixels)
    return pixels
}
func textCaptureReference(payload: TextLayerPayload, anchor: CGPoint, decorationsOnly: Bool = false) throws -> [UInt8] {
    let manager = TextCaptureReference(decorationsOnly: decorationsOnly)
    return try textCaptureWithLayoutManager(manager, payload: payload, anchor: anchor)
}
func textCaptureWithLayoutManager(_ manager: NSLayoutManager, payload: TextLayerPayload, anchor: CGPoint) throws -> [UInt8] {
    guard let plan = TextLayer.drawingPlan(payload, anchor: anchor, color: nil, layoutManager: manager) else {
        throw TextCaptureProbeError(message: "Reference text layout failed")
    }
    return try textProbePixels(width: plan.width, height: plan.height) { context in
        guard plan.draw(context) else { throw TextCaptureProbeError(message: "Reference TextKit draw failed") }
    }
}
func textCaptureRendered(_ drawing: NativeArtworkDrawing, pathsOnly: Bool = false) throws -> [UInt8] {
    try textProbePixels(width: drawing.width, height: drawing.height) { context in
        for part in drawing.parts {
            switch part {
            case .fill, .gradient, .pattern:
                // Fill-layer parts; a text drawing never has one.
                throw TextCaptureProbeError(message: "A text drawing holds a fill-layer part")
            case let .path(path, rgba):
                context.setFillColor(red: rgba[0], green: rgba[1], blue: rgba[2], alpha: rgba[3])
                context.addPath(path); context.fillPath()
            case let .intrinsic(pixels, opacity):
                if pathsOnly { continue }
                guard let space = CGColorSpace(name: CGColorSpace.sRGB),
                      let provider = CGDataProvider(data: Data(pixels) as CFData),
                      let image = CGImage(width: drawing.width, height: drawing.height, bitsPerComponent: 8,
                        bitsPerPixel: 32, bytesPerRow: drawing.width * 4, space: space,
                        bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                        provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent) else {
                    throw TextCaptureProbeError(message: "Intrinsic reference image failed")
                }
                context.saveGState(); context.setAlpha(opacity)
                context.translateBy(x: 0, y: CGFloat(drawing.height)); context.scaleBy(x: 1, y: -1)
                context.draw(image, in: CGRect(x: 0, y: 0, width: drawing.width, height: drawing.height))
                context.restoreGState()
            case let .coverage(pixels, rgba):
                var colored = [UInt8](repeating: 0, count: pixels.count * 4)
                for i in pixels.indices {
                    for c in 0..<3 { colored[i * 4 + c] = UInt8((rgba[c] * 255).rounded()) }
                    colored[i * 4 + 3] = pixels[i]
                }
                let scalar = NativeArtworkDrawing(width: drawing.width, height: drawing.height, offsetX: 0, offsetY: 0,
                    parts: [.intrinsic(pixels: colored, opacity: rgba[3])])
                let values = try textCaptureRendered(scalar)
                guard let space = CGColorSpace(name: CGColorSpace.sRGB),
                      let provider = CGDataProvider(data: Data(values) as CFData),
                      let image = CGImage(width: drawing.width, height: drawing.height, bitsPerComponent: 8,
                        bitsPerPixel: 32, bytesPerRow: drawing.width * 4, space: space,
                        bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue), provider: provider,
                        decode: nil, shouldInterpolate: false, intent: .defaultIntent) else {
                    throw TextCaptureProbeError(message: "Intrinsic coverage reference failed")
                }
                context.saveGState(); context.translateBy(x: 0, y: CGFloat(drawing.height)); context.scaleBy(x: 1, y: -1)
                context.draw(image, in: CGRect(x: 0, y: 0, width: drawing.width, height: drawing.height)); context.restoreGState()
            }
        }
    }
}
func textCaptureDifference(_ a: [UInt8], _ b: [UInt8], width: Int) -> [String: Any] {
    precondition(a.count == b.count)
    var maxComponent = 0, differingComponents = 0, alphaSum = 0, union = 0, maxAlpha = 0, maxPremultiplied = 0
    var beyondTwoPixels = 0
    var aBounds = [width, a.count / 4 / width, -1, -1], bBounds = aBounds
    var aArea = 0, bArea = 0
    for i in a.indices {
        let difference = abs(Int(a[i]) - Int(b[i])); maxComponent = max(maxComponent, difference)
        if difference > 0 { differingComponents += 1 }
    }
    for p in 0..<(a.count / 4) {
        let av = Int(a[p * 4 + 3]), bv = Int(b[p * 4 + 3])
        for c in 0..<3 {
            let ac = (Int(a[p * 4 + c]) * av + 127) / 255
            let bc = (Int(b[p * 4 + c]) * bv + 127) / 255
            maxPremultiplied = max(maxPremultiplied, abs(ac - bc))
        }
        if av > 0 || bv > 0 { union += 1; alphaSum += abs(av - bv) }
        maxAlpha = max(maxAlpha, abs(av - bv))
        if (av >= 8) != (bv >= 8) {
            let other = av >= 8 ? b : a
            let x = p % width, y = p / width, height = a.count / 4 / width
            var found = false
            for yy in max(0, y - 2)...min(height - 1, y + 2) {
                for xx in max(0, x - 2)...min(width - 1, x + 2) where other[(yy * width + xx) * 4 + 3] >= 8 { found = true }
            }
            if !found { beyondTwoPixels += 1 }
        }
        aArea += av; bArea += bv
        if av > 0 { aBounds = [min(aBounds[0], p % width), min(aBounds[1], p / width), max(aBounds[2], p % width), max(aBounds[3], p / width)] }
        if bv > 0 { bBounds = [min(bBounds[0], p % width), min(bBounds[1], p / width), max(bBounds[2], p % width), max(bBounds[3], p / width)] }
    }
    return ["max_component": maxComponent, "differing_components": differingComponents,
        "max_alpha": maxAlpha, "ink_pixels_outside_two_pixel_reference_neighborhood": beyondTwoPixels,
        "max_premultiplied_rgb": maxPremultiplied,
        "alpha_mean_over_ink_union": Double(alphaSum) / Double(max(1, union)),
        "reference_bounds": aBounds, "captured_bounds": bBounds,
        "reference_alpha_area": aArea, "captured_alpha_area": bArea]
}

func textCaptureSavePNG(_ pixels: [UInt8], width: Int, height: Int, url: URL) throws {
    guard let space = CGColorSpace(name: CGColorSpace.sRGB),
          let provider = CGDataProvider(data: Data(pixels) as CFData),
          let image = CGImage(width: width, height: height, bitsPerComponent: 8,
            bitsPerPixel: 32, bytesPerRow: width * 4, space: space,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue), provider: provider,
            decode: nil, shouldInterpolate: false, intent: .defaultIntent),
          let destination = CGImageDestinationCreateWithURL(url as CFURL, "public.png" as CFString, 1, nil) else {
        throw TextCaptureProbeError(message: "Could not prepare text probe image")
    }
    CGImageDestinationAddImage(destination, image, nil)
    guard CGImageDestinationFinalize(destination) else { throw TextCaptureProbeError(message: "Could not write text probe image") }
}
