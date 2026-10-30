// An independent ImageIO/CoreGraphics oracle for the opaque host-still
// fixtures. It imports no Doctored code: every answer comes from
// the system frameworks directly, so a drive can compare the app against it.
//
//   oracle layout <file>              JSON: container depth, gain-map presence,
//                                     and the decoded buffer's layout under the
//                                     HDR and the SDR decode requests
//   oracle redraw16 <file> <out>      HDR-request decode (HostStillDecoder's
//                                     exact options) drawn 1:1 into a 16-bpc
//                                     RGBX context in the image's own space;
//                                     raw little-endian u16 RGBX
//   oracle sdr8 <file> <out>          SDR-request decode drawn 1:1 into an
//                                     8-bpc RGBX context in its own space;
//                                     raw RGBX8
//   oracle raw16 <file> <out>         HDR-request decode's own provider
//                                     samples, no redraw: fails unless the
//                                     decode is 16 bpc, 64 bpp, 16Little,
//                                     packed, alpha last or noneSkipLast;
//                                     raw little-endian u16 RGBA, row padding
//                                     dropped, an X sample written as 65535
//   oracle gainmap <out.heic>         a synthetic 8-bit HEIC carrying an HDR
//                                     gain map, self-checked (nonzero exit on
//                                     any failed check)
//
// Build: xcrun swiftc -O -target arm64-apple-macos15.0 oracle.swift -o oracle
import CoreGraphics
import CoreImage
import Foundation
import ImageIO

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

func source(_ path: String) -> CGImageSource {
    guard let value = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL,
        [kCGImageSourceShouldCache: false] as CFDictionary) else { fail("cannot open \(path)") }
    return value
}

/// HostStillDecoder's decode options; only the request varies.
func decode(_ source: CGImageSource, _ request: CFString) -> CGImage? {
    CGImageSourceCreateImageAtIndex(source, 0,
        [kCGImageSourceShouldCacheImmediately: true, kCGImageSourceShouldAllowFloat: true,
         kCGImageSourceDecodeRequest: request,
         kCGImageSourceGenerateImageSpecificLumaScaling: false] as CFDictionary)
}

func gainMaps(_ source: CGImageSource) -> [String: Bool] {
    ["hdr_gain_map": CGImageSourceCopyAuxiliaryDataInfoAtIndex(source, 0,
        kCGImageAuxiliaryDataTypeHDRGainMap) != nil,
     "iso_gain_map": CGImageSourceCopyAuxiliaryDataInfoAtIndex(source, 0,
        kCGImageAuxiliaryDataTypeISOGainMap) != nil]
}

func alphaName(_ info: CGImageAlphaInfo) -> String {
    switch info {
    case .none: return "none"
    case .premultipliedLast: return "premultipliedLast"
    case .premultipliedFirst: return "premultipliedFirst"
    case .last: return "last"
    case .first: return "first"
    case .noneSkipLast: return "noneSkipLast"
    case .noneSkipFirst: return "noneSkipFirst"
    case .alphaOnly: return "alphaOnly"
    @unknown default: return "unknown(\(info.rawValue))"
    }
}

func orderName(_ info: CGBitmapInfo) -> String {
    switch info.rawValue & CGBitmapInfo.byteOrderMask.rawValue {
    case 0: return "default"
    case CGBitmapInfo.byteOrder16Little.rawValue: return "16Little"
    case CGBitmapInfo.byteOrder32Little.rawValue: return "32Little"
    case CGBitmapInfo.byteOrder16Big.rawValue: return "16Big"
    case CGBitmapInfo.byteOrder32Big.rawValue: return "32Big"
    default: return "other"
    }
}

func formatName(_ info: CGImagePixelFormatInfo) -> String {
    switch info {
    case .packed: return "packed"
    case .RGB555: return "RGB555"
    case .RGB565: return "RGB565"
    case .RGB101010: return "RGB101010"
    case .RGBCIF10: return "RGBCIF10"
    default: return "other(\(info.rawValue))"
    }
}

func isHDR(_ image: CGImage) -> Bool {
    image.bitmapInfo.contains(.floatComponents)
        || (image.colorSpace.map { $0.isHDR() || CGColorSpaceUsesExtendedRange($0) } ?? false)
}

func describe(_ image: CGImage?) -> Any {
    guard let image else { return NSNull() }
    let space = image.colorSpace
    return ["bpc": image.bitsPerComponent, "bpp": image.bitsPerPixel,
            "alpha_info": alphaName(image.alphaInfo), "byte_order": orderName(image.bitmapInfo),
            "pixel_format": formatName(image.pixelFormatInfo),
            "color_space": (space?.name as String?) ?? NSNull(),
            "is_hdr": space?.isHDR() ?? false,
            "extended": space.map { CGColorSpaceUsesExtendedRange($0) } ?? false,
            "float": image.bitmapInfo.contains(.floatComponents),
            "width": image.width, "height": image.height] as [String: Any]
}

func layout(_ path: String) {
    let image = source(path)
    guard let info = CGImageSourceCopyPropertiesAtIndex(image, 0, nil) as? [CFString: Any] else {
        fail("no properties for \(path)")
    }
    var out: [String: Any] = gainMaps(image)
    out["type"] = (CGImageSourceGetType(image) as String?) ?? NSNull()
    out["depth"] = (info[kCGImagePropertyDepth] as? NSNumber)?.intValue ?? NSNull()
    out["hdr_request"] = describe(decode(image, kCGImageSourceDecodeToHDR))
    out["sdr_request"] = describe(decode(image, kCGImageSourceDecodeToSDR))
    guard let data = try? JSONSerialization.data(withJSONObject: out, options: [.sortedKeys]) else {
        fail("layout is not JSON")
    }
    FileHandle.standardOutput.write(data + Data("\n".utf8))
}

/// Draws `image` 1:1, in its own colour space, into an RGBX context of the
/// requested depth and writes the context's bytes.
func redraw(_ image: CGImage, bits: Int, to path: String) {
    guard let space = image.colorSpace else { fail("image has no colour space") }
    let order = bits == 16 ? CGBitmapInfo.byteOrder16Little.rawValue : CGBitmapInfo.byteOrder32Big.rawValue
    let rowBytes = image.width * 4 * bits / 8
    guard let context = CGContext(data: nil, width: image.width, height: image.height,
        bitsPerComponent: bits, bytesPerRow: rowBytes, space: space,
        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue | order) else {
        fail("cannot create a \(bits)-bpc RGBX context in \(space.name as String? ?? "?")")
    }
    context.interpolationQuality = .none
    context.setBlendMode(.copy)
    context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
    guard let data = context.data else { fail("context has no data") }
    let bytes = Data(bytes: data, count: rowBytes * image.height)
    do { try bytes.write(to: URL(fileURLWithPath: path)) } catch { fail("cannot write \(path)") }
}

func redraw16(_ path: String, _ out: String) {
    guard let image = decode(source(path), kCGImageSourceDecodeToHDR) else { fail("HDR decode failed") }
    guard !isHDR(image) else { fail("the HDR-request decode is HDR; redraw16 compares SDR integer decodes only") }
    // An image ImageIO describes but cannot decode (12-bit 4:4:4 HEIC) has no
    // provider bytes; drawing it would write zeros and pass silently.
    guard let decoded = image.dataProvider?.data, CFDataGetLength(decoded) >= image.bytesPerRow * image.height else {
        fail("the decode has no provider data covering \(image.bytesPerRow) x \(image.height) bytes")
    }
    redraw(image, bits: 16, to: out)
}

func sdr8(_ path: String, _ out: String) {
    guard let image = decode(source(path), kCGImageSourceDecodeToSDR) else { fail("SDR decode failed") }
    guard !isHDR(image) else { fail("the SDR-request decode is HDR") }
    redraw(image, bits: 8, to: out)
}

/// The decode's own 16-bit samples as ImageIO hands them over: what
/// the host reads before packing, so an exact comparison against it checks
/// the packing and nothing else.
func raw16(_ path: String, _ out: String) {
    guard let image = decode(source(path), kCGImageSourceDecodeToHDR) else { fail("HDR decode failed") }
    let order = image.bitmapInfo.rawValue & CGBitmapInfo.byteOrderMask.rawValue
    let alpha = image.alphaInfo
    guard image.bitsPerComponent == 16, image.bitsPerPixel == 64,
          order == CGBitmapInfo.byteOrder16Little.rawValue, image.pixelFormatInfo == .packed,
          !image.bitmapInfo.contains(.floatComponents),
          alpha == .last || alpha == .noneSkipLast else {
        fail("raw16 needs a packed 16Little RGBA16/RGBX16 decode; got \(image.bitsPerComponent) bpc, "
            + "\(image.bitsPerPixel) bpp, \(orderName(image.bitmapInfo)), \(formatName(image.pixelFormatInfo)), "
            + alphaName(alpha))
    }
    let row = image.width * 8
    guard image.bytesPerRow >= row, let decoded = image.dataProvider?.data,
          CFDataGetLength(decoded) >= image.bytesPerRow * image.height else {
        fail("the decode has no provider data covering \(image.bytesPerRow) x \(image.height) bytes")
    }
    let provider = decoded as Data
    var bytes = Data(capacity: row * image.height)
    for y in 0..<image.height {
        var line = provider.subdata(in: (y * image.bytesPerRow)..<(y * image.bytesPerRow + row))
        if alpha == .noneSkipLast {
            for x in 0..<image.width {
                line[x * 8 + 6] = 0xff
                line[x * 8 + 7] = 0xff
            }
        }
        bytes.append(line)
    }
    do { try bytes.write(to: URL(fileURLWithPath: out)) } catch { fail("cannot write \(out)") }
}

/// A 64x48 HDR scene with real headroom (linear values up to 4.0), written
/// as an 8-bit SDR base plus a gain map by Core Image's HDR HEIF writer.
func gainmap(_ out: String) {
    let width = 64, height = 48
    var pixels = [Float](repeating: 0, count: width * height * 4)
    for y in 0..<height {
        for x in 0..<width {
            let u = Float(x) / Float(width - 1), v = Float(y) / Float(height - 1)
            let dx = Float(x) - 40, dy = Float(y) - 20
            let highlight = max(0, 1 - (dx * dx + dy * dy).squareRoot() / 14) * 3.2
            let at = (y * width + x) * 4
            pixels[at] = 0.05 + 0.7 * u + highlight
            pixels[at + 1] = 0.08 + 0.6 * v + highlight * 0.9
            pixels[at + 2] = 0.12 + 0.4 * (1 - u) + highlight * 0.8
            pixels[at + 3] = 1
        }
    }
    guard let linear = CGColorSpace(name: CGColorSpace.extendedLinearDisplayP3),
          let p3 = CGColorSpace(name: CGColorSpace.displayP3),
          let provider = CGDataProvider(data: Data(bytes: pixels, count: pixels.count * 4) as CFData),
          let float = CGImage(width: width, height: height, bitsPerComponent: 32, bitsPerPixel: 128,
            bytesPerRow: width * 16, space: linear,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue
                | CGBitmapInfo.floatComponents.rawValue | CGBitmapInfo.byteOrder32Little.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent) else {
        fail("cannot build the HDR scene")
    }
    // Marked opaque so the writer stores no alpha plane: the policy under
    // test is the OPAQUE still's.
    let extent = CGRect(x: 0, y: 0, width: width, height: height)
    let hdr = CIImage(cgImage: float).settingAlphaOne(in: extent)
    let sdr = hdr.applyingFilter("CIToneMapHeadroom",
        parameters: ["inputSourceHeadroom": 4.0, "inputTargetHeadroom": 1.0]).settingAlphaOne(in: extent)
    let context = CIContext(options: [.workingColorSpace: linear])
    do {
        try context.writeHEIFRepresentation(of: sdr, to: URL(fileURLWithPath: out), format: .RGBA8,
            colorSpace: p3, options: [.hdrImage: hdr])
    } catch { fail("Core Image HDR HEIF writer failed: \(error)") }

    // Self-check: the file is exactly the kind of still the policy targets.
    let written = source(out)
    guard let info = CGImageSourceCopyPropertiesAtIndex(written, 0, nil) as? [CFString: Any],
          (info[kCGImagePropertyDepth] as? NSNumber)?.intValue == 8 else { fail("container depth is not 8") }
    guard gainMaps(written).values.contains(true) else { fail("no gain map in the written file") }
    guard let hdrDecode = decode(written, kCGImageSourceDecodeToHDR), isHDR(hdrDecode) else {
        fail("the HDR-request decode is not HDR")
    }
    guard let sdrDecode = decode(written, kCGImageSourceDecodeToSDR), !isHDR(sdrDecode),
          sdrDecode.bitsPerComponent == 8 else { fail("the SDR-request decode is not 8-bpc SDR") }
}

let arguments = CommandLine.arguments
switch (arguments.count > 1 ? arguments[1] : "", arguments.count) {
case ("layout", 3): layout(arguments[2])
case ("redraw16", 4): redraw16(arguments[2], arguments[3])
case ("sdr8", 4): sdr8(arguments[2], arguments[3])
case ("raw16", 4): raw16(arguments[2], arguments[3])
case ("gainmap", 3): gainmap(arguments[2])
default: fail("usage: oracle layout <file> | redraw16 <file> <out> | sdr8 <file> <out> | raw16 <file> <out> | gainmap <out.heic>")
}
