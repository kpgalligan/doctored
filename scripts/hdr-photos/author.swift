// The v1.x HDR photo fixture author and its read-only inspector. It imports no Doctored
// code: every file is written by ImageIO or Core Image directly and self-checked, so a drive never
// decodes its own output to decide what it wrote. `author.py` drives it (and writes the PNG,
// AVIF and JPEG XL fixtures itself, with the command-line encoders).
//
//   author gainmap-iso   <scene.json> <out.heic>   ImageIO kCGImageDestinationEncodeToISOGainmap
//   author gainmap-ci    <scene.json> <out.heic>   Core Image writeHEIFRepresentation(.hdrImage)
//   author gainmap-apple <scene.json> <gain.heic> <out.heic>
//                                                  Core Image kCIImageRepresentationHDRGainMapImage
//                                                  with <gain.heic>'s gain map; Apple aux only
//   author heif10        <scene.json> <pq|hlg> <out.heic>
//                                                  Core Image writeHEIF10Representation in
//                                                  itur_2100_PQ / itur_2100_HLG
//   author heic-codes    <codes16.png> <pq|hlg> <out.heic>
//                                                  16-bit BT.2100 codes author.py computed,
//                                                  labelled itur_2100_PQ / _HLG, through Core
//                                                  Image's HEIF10 writer in that same space
//   author inspect <path>                          read-only: prints one JSON object of numbers
//
// scene.json: {"width", "height", "space": "srgb" | "display_p3", "headroom",
// "patches": [[r, g, b], …]} — linear light, 1.0 = SDR white (gain maps) or 203 cd/m² (PQ/HLG),
// the patches tiled 4 across and len/4 down, each a flat rectangle. `inspect` reports, for an
// ISO-style gain map (either auxiliary type), the base image's linear values and the gain map's
// normalized codes at every patch centre of a `--patches COLS ROWS` grid, so the drive can apply
// ISO 21496-1 itself; for any file it reports the auxiliary types present, the Apple maker-note
// headroom tags (33, 48) and Apple's published headroom formula over them, the ISO metadata, and
// `CGImage.contentHeadroom` of ImageIO's HDR decode. It reads the file in place, writes nothing.
//
// Build: xcrun swiftc -O -target arm64-apple-macos15.0 author.swift -o author
import CoreGraphics
import CoreImage
import Foundation
import ImageIO

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

struct Scene {
    let width: Int
    let height: Int
    let space: String
    let headroom: Float
    let patches: [[Float]]

    init(path: String) {
        guard let data = FileManager.default.contents(atPath: path),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let width = object["width"] as? Int, let height = object["height"] as? Int,
              let space = object["space"] as? String,
              let headroom = (object["headroom"] as? NSNumber)?.floatValue,
              let patches = object["patches"] as? [[NSNumber]], patches.count % 4 == 0
        else { fail("unreadable scene \(path)") }
        self.width = width
        self.height = height
        self.space = space
        self.headroom = headroom
        self.patches = patches.map { $0.map(\.floatValue) }
    }

    var rows: Int { patches.count / 4 }

    func value(_ x: Int, _ y: Int) -> [Float] {
        let column = min(3, x * 4 / width), row = min(rows - 1, y * rows / height)
        return patches[row * 4 + column]
    }

    var linearSpace: CGColorSpace {
        guard let space = CGColorSpace(name: space == "display_p3"
            ? CGColorSpace.extendedLinearDisplayP3 : CGColorSpace.extendedLinearSRGB)
        else { fail("no linear space") }
        return space
    }

    var baseSpace: CGColorSpace {
        guard let space = CGColorSpace(name: space == "display_p3"
            ? CGColorSpace.displayP3 : CGColorSpace.sRGB) else { fail("no base space") }
        return space
    }

    /// The scene as a premultiplied RGBA float CGImage in the linear space,
    /// tagged with the scene's content headroom.
    var image: CGImage {
        var data = Data(count: width * height * 16)
        data.withUnsafeMutableBytes { raw in
            let p = raw.bindMemory(to: Float.self)
            for y in 0..<height {
                for x in 0..<width {
                    let v = value(x, y), at = (y * width + x) * 4
                    p[at] = v[0]; p[at + 1] = v[1]; p[at + 2] = v[2]; p[at + 3] = 1
                }
            }
        }
        guard let provider = CGDataProvider(data: data as CFData),
              let image = CGImage(width: width, height: height, bitsPerComponent: 32,
                  bitsPerPixel: 128, bytesPerRow: width * 16, space: linearSpace,
                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue
                      | CGBitmapInfo.floatComponents.rawValue
                      | CGBitmapInfo.byteOrder32Little.rawValue),
                  provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent),
              let tagged = CGImageCreateCopyWithContentHeadroom(headroom, image)
        else { fail("could not build the scene image") }
        return tagged
    }
}

func auxiliary(_ path: String) -> (iso: Bool, apple: Bool) {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil)
    else { fail("cannot read \(path)") }
    let index = CGImageSourceGetPrimaryImageIndex(source)
    return (CGImageSourceCopyAuxiliaryDataInfoAtIndex(source, index, kCGImageAuxiliaryDataTypeISOGainMap) != nil,
            CGImageSourceCopyAuxiliaryDataInfoAtIndex(source, index, kCGImageAuxiliaryDataTypeHDRGainMap) != nil)
}

/// The self-check every writer ends with: dimensions, and which auxiliary
/// gain maps the written file carries (or, for PQ/HLG, its HDR decode space).
func check(_ path: String, scene: Scene, iso: Bool, apple: Bool, hdrSpace: String? = nil) {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let info = CGImageSourceCopyPropertiesAtIndex(source, CGImageSourceGetPrimaryImageIndex(source), nil)
            as? [CFString: Any] else { fail("cannot inspect \(path)") }
    let w = (info[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue
    let h = (info[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue
    guard w == scene.width, h == scene.height else { fail("\(path): dimensions \(String(describing: w))x\(String(describing: h))") }
    let found = auxiliary(path)
    guard found.iso == iso, found.apple == apple else {
        fail("\(path): ISO gain map \(found.iso) (want \(iso)), Apple gain map \(found.apple) (want \(apple))")
    }
    if let hdrSpace {
        guard let image = CGImageSourceCreateImageAtIndex(source, CGImageSourceGetPrimaryImageIndex(source),
                  [kCGImageSourceDecodeRequest: kCGImageSourceDecodeToHDR] as CFDictionary),
              let name = image.colorSpace?.name as String?, name == hdrSpace else {
            fail("\(path): the HDR decode is not in \(hdrSpace)")
        }
    }
}

func quality() -> [CFString: Any] { [kCGImageDestinationLossyCompressionQuality: 1.0] }

func gainmapISO(_ scene: Scene, _ out: String) {
    guard let destination = CGImageDestinationCreateWithURL(URL(fileURLWithPath: out) as CFURL,
              "public.heic" as CFString, 1, nil) else { fail("no HEIC destination") }
    var options = quality()
    options[kCGImageDestinationEncodeRequest] = kCGImageDestinationEncodeToISOGainmap
    CGImageDestinationAddImage(destination, scene.image, options as CFDictionary)
    guard CGImageDestinationFinalize(destination) else { fail("ImageIO's ISO gain-map encode failed") }
    check(out, scene: scene, iso: true, apple: false)
}

func hdrAndSDR(_ scene: Scene) -> (CIImage, CIImage) {
    let hdr = CIImage(cgImage: scene.image)
    let sdr = hdr.applyingFilter("CIToneMapHeadroom", parameters: [
        "inputSourceHeadroom": scene.headroom, "inputTargetHeadroom": 1.0])
    return (hdr, sdr)
}

func gainmapCI(_ scene: Scene, _ out: String) {
    let (hdr, sdr) = hdrAndSDR(scene)
    let context = CIContext(options: [.workingColorSpace: scene.linearSpace])
    do {
        try context.writeHEIFRepresentation(of: sdr, to: URL(fileURLWithPath: out), format: .RGBA8,
            colorSpace: scene.baseSpace, options: [
                .hdrImage: hdr, .hdrGainMapAsRGB: true,
                CIImageRepresentationOption(rawValue: kCGImageDestinationLossyCompressionQuality as String): 1.0,
            ])
    } catch { fail("Core Image's HDR HEIF writer failed: \(error)") }
    check(out, scene: scene, iso: true, apple: false)
}

func gainmapApple(_ scene: Scene, _ gain: String, _ out: String) {
    let (_, sdr) = hdrAndSDR(scene)
    guard let map = CIImage(contentsOf: URL(fileURLWithPath: gain), options: [.auxiliaryHDRGainMap: true])
    else { fail("no gain map in \(gain)") }
    let context = CIContext(options: [.workingColorSpace: scene.linearSpace])
    do {
        try context.writeHEIFRepresentation(of: sdr, to: URL(fileURLWithPath: out), format: .RGBA8,
            colorSpace: scene.baseSpace, options: [
                .hdrGainMapImage: map,
                CIImageRepresentationOption(rawValue: kCGImageDestinationLossyCompressionQuality as String): 1.0,
            ])
    } catch { fail("Core Image's gain-map HEIF writer failed: \(error)") }
    check(out, scene: scene, iso: false, apple: true)
}

func heif10(_ scene: Scene, _ transfer: String, _ out: String) {
    let name = transfer == "hlg" ? CGColorSpace.itur_2100_HLG : CGColorSpace.itur_2100_PQ
    guard let target = CGColorSpace(name: name) else { fail("no \(transfer) space") }
    let context = CIContext(options: [.workingColorSpace: scene.linearSpace])
    do {
        try context.writeHEIF10Representation(of: CIImage(cgImage: scene.image),
            to: URL(fileURLWithPath: out), colorSpace: target, options: [
                CIImageRepresentationOption(rawValue: kCGImageDestinationLossyCompressionQuality as String): 1.0,
            ])
    } catch { fail("Core Image's HEIF10 writer failed: \(error)") }
    check(out, scene: scene, iso: false, apple: false, hdrSpace: name as String)
}

/// A 10-bit HEIC of the BT.2100 codes in `codes` (a 16-bit RGB PNG): the
/// decoded image relabelled in the 2100 space and written by Core Image's
/// HEIF10 writer INTO that same space, whose decode and encode are each
/// other's inverse, so the codes written are author.py's, not Core Image's
/// HLG model of a linear scene. (ImageIO's own HEIC writer moved a flat grey
/// by up to 5 ten-bit codes; this writer keeps every patch within 2.)
func heicCodes(_ codes: String, _ transfer: String, _ out: String) {
    let name = transfer == "hlg" ? CGColorSpace.itur_2100_HLG : CGColorSpace.itur_2100_PQ
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: codes) as CFURL, nil),
          let decoded = CGImageSourceCreateImageAtIndex(source, 0, nil), decoded.bitsPerComponent == 16,
          let space = CGColorSpace(name: name), let image = decoded.copy(colorSpace: space)
    else { fail("cannot read \(codes) as 16-bit codes") }
    let context = CIContext(options: [.workingColorSpace: CGColorSpace(name: CGColorSpace.extendedLinearITUR_2020) as Any])
    do {
        try context.writeHEIF10Representation(of: CIImage(cgImage: image), to: URL(fileURLWithPath: out),
            colorSpace: space, options: [
                CIImageRepresentationOption(rawValue: kCGImageDestinationLossyCompressionQuality as String): 1.0,
            ])
    } catch { fail("Core Image's HEIF10 writer failed: \(error)") }
    guard let check = CGImageSourceCreateWithURL(URL(fileURLWithPath: out) as CFURL, nil),
          let back = CGImageSourceCreateImageAtIndex(check, 0, [kCGImageSourceDecodeRequest: kCGImageSourceDecodeToHDR] as CFDictionary),
          back.colorSpace?.name as String? == name as String, back.width == decoded.width,
          back.height == decoded.height else { fail("\(out): the HDR decode is not in \(name)") }
    guard auxiliary(out) == (false, false) else { fail("\(out): an unexpected gain map") }
}

// MARK: - inspect

func number(_ value: Any?) -> Double? {
    if let number = value as? NSNumber { return number.doubleValue }
    if let text = value as? String { return Double(text) }
    return nil
}

/// The HDRToneMap metadata of a gain-map auxiliary as plain numbers.
func toneMapMetadata(_ metadata: CGImageMetadata) -> [String: Any] {
    var out: [String: Any] = [:]
    guard let tags = CGImageMetadataCopyTags(metadata) as? [CGImageMetadataTag] else { return out }
    for tag in tags {
        guard let name = CGImageMetadataTagCopyName(tag) as String? else { continue }
        let value = CGImageMetadataTagCopyValue(tag)
        if name == "ChannelMetadata", let channels = value as? [CGImageMetadataTag] {
            out["channels"] = channels.map { channel -> [String: Double] in
                var fields: [String: Double] = [:]
                if let structure = CGImageMetadataTagCopyValue(channel) as? [String: CGImageMetadataTag] {
                    for (key, field) in structure {
                        if let v = number(CGImageMetadataTagCopyValue(field)) { fields[key] = v }
                    }
                }
                return fields
            }
        } else if let v = number(value) {
            out[name] = v
        } else if let text = value as? String {
            out[name] = text
        }
    }
    return out
}

/// The gain map's codes (0…1) as Core Image reads them — its auxiliary
/// image rendered with NO colour management, so the numbers are the stored
/// codes (a 4:2:0 colour map converted to RGB as Core Image converts it, a
/// monochrome one replicated) — as an RGBA float bitmap and its size.
func gainMapCodes(_ url: URL) -> (pixels: [Float], width: Int, height: Int)? {
    guard let map = CIImage(contentsOf: url, options: [.auxiliaryHDRGainMap: true]) else { return nil }
    let extent = map.extent.integral
    let width = Int(extent.width), height = Int(extent.height)
    guard width > 0, height > 0 else { return nil }
    var pixels = [Float](repeating: 0, count: width * height * 4)
    let context = CIContext(options: [.workingColorSpace: NSNull(), .outputColorSpace: NSNull()])
    pixels.withUnsafeMutableBytes { raw in
        guard let base = raw.baseAddress else { return }
        context.render(map, toBitmap: base, rowBytes: width * 16, bounds: extent, format: .RGBAf,
                       colorSpace: nil)
    }
    return (pixels, width, height)
}

func inspect(_ path: String, columns: Int, rows: Int) {
    let url = URL(fileURLWithPath: path)
    guard let source = CGImageSourceCreateWithURL(url as CFURL, [kCGImageSourceShouldCache: false] as CFDictionary)
    else { fail("cannot read \(path)") }
    let index = CGImageSourceGetPrimaryImageIndex(source)
    var out: [String: Any] = ["type": CGImageSourceGetType(source) as String? ?? NSNull(), "index": index]
    let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any] ?? [:]
    let width = (properties[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue ?? 0
    let height = (properties[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue ?? 0
    out["width"] = width
    out["height"] = height
    // Apple's maker-note headroom tags and its published formula
    // ("Applying Apple HDR effect to your photos").
    if let maker = properties[kCGImagePropertyMakerAppleDictionary] as? [String: Any] {
        let m33 = number(maker["33"]), m48 = number(maker["48"])
        out["maker_apple_33"] = m33 ?? NSNull()
        out["maker_apple_48"] = m48 ?? NSNull()
        if let m33, let m48 {
            let stops: Double
            if m33 < 1 {
                stops = m48 <= 0.01 ? -20 * m48 + 1.8 : -0.101 * m48 + 1.601
            } else {
                stops = m48 <= 0.01 ? -70 * m48 + 3 : -0.303 * m48 + 2.303
            }
            out["apple_headroom_formula"] = pow(2, max(stops, 0))
        }
    }
    var auxiliaries: [String: Any] = [:]
    for (name, type) in [("iso", kCGImageAuxiliaryDataTypeISOGainMap), ("apple", kCGImageAuxiliaryDataTypeHDRGainMap)] {
        guard let info = CGImageSourceCopyAuxiliaryDataInfoAtIndex(source, index, type) as? [CFString: Any] else {
            auxiliaries[name] = NSNull()
            continue
        }
        var entry: [String: Any] = [:]
        if let description = info[kCGImageAuxiliaryDataInfoDataDescription] as? [String: Any] {
            entry["width"] = description["Width"] ?? NSNull()
            entry["height"] = description["Height"] ?? NSNull()
            entry["bytes_per_row"] = description["BytesPerRow"] ?? NSNull()
        }
        if let metadata = info[kCGImageAuxiliaryDataInfoMetadata] {
            // A CF type; checked by its type id rather than a forced cast.
            if CFGetTypeID(metadata as CFTypeRef) == CGImageMetadataGetTypeID() {
                let tagged = unsafeBitCast(metadata as CFTypeRef, to: CGImageMetadata.self)
                let fields = toneMapMetadata(tagged)
                entry["metadata"] = fields
                // Apple's own XMP headroom, where a camera writes one.
                for key in ["HDRGainMapHeadroom"] where fields[key] != nil {
                    entry["hdr_gain_map_headroom"] = fields[key]
                }
            }
        }
        if columns > 0, rows > 0, width > 0, height > 0, let map = gainMapCodes(url) {
            var samples: [[Double]] = []
            for r in 0..<rows {
                for c in 0..<columns {
                    let gx = min(map.width - 1, Int((Double(c) + 0.5) * Double(map.width) / Double(columns)))
                    let gy = min(map.height - 1, Int((Double(r) + 0.5) * Double(map.height) / Double(rows)))
                    let at = (gy * map.width + gx) * 4
                    samples.append([Double(map.pixels[at]), Double(map.pixels[at + 1]),
                                    Double(map.pixels[at + 2])])
                }
            }
            entry["codes"] = samples
            entry["codes_size"] = [map.width, map.height]
        }
        auxiliaries[name] = entry
    }
    out["auxiliary"] = auxiliaries
    // The base (SDR) image's linear values at the patch centres.
    if columns > 0, rows > 0, width > 0, height > 0,
       let base = CGImageSourceCreateImageAtIndex(source, index, [kCGImageSourceShouldCache: false] as CFDictionary),
       let baseSpace = base.colorSpace, let linear = CGColorSpaceCreateExtendedLinearized(baseSpace),
       let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 32,
           bytesPerRow: width * 16, space: linear,
           bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.floatComponents.rawValue
               | CGBitmapInfo.byteOrder32Little.rawValue),
       width * height <= 16_000_000 {
        context.draw(base, in: CGRect(x: 0, y: 0, width: width, height: height))
        if let data = context.data {
            let p = data.bindMemory(to: Float.self, capacity: width * height * 4)
            var values: [[Double]] = []
            for r in 0..<rows {
                for c in 0..<columns {
                    let x = Int((Double(c) + 0.5) * Double(width) / Double(columns))
                    let y = Int((Double(r) + 0.5) * Double(height) / Double(rows))
                    let at = (y * width + x) * 4
                    values.append([Double(p[at]), Double(p[at + 1]), Double(p[at + 2])])
                }
            }
            out["base_linear"] = values
        }
        out["base_space"] = baseSpace.name as String? ?? NSNull()
    }
    if let hdr = CGImageSourceCreateImageAtIndex(source, index, [
           kCGImageSourceDecodeRequest: kCGImageSourceDecodeToHDR, kCGImageSourceShouldCache: false,
       ] as CFDictionary) {
        out["hdr_decode_content_headroom"] = Double(hdr.contentHeadroom)
        out["hdr_decode_space"] = hdr.colorSpace?.name as String? ?? NSNull()
    } else {
        out["hdr_decode_content_headroom"] = NSNull()
    }
    guard let json = try? JSONSerialization.data(withJSONObject: out, options: [.sortedKeys]) else {
        fail("could not serialize the inspection")
    }
    FileHandle.standardOutput.write(json)
    FileHandle.standardOutput.write(Data("\n".utf8))
}

let arguments = CommandLine.arguments
guard arguments.count >= 3 else {
    fail("usage: author <gainmap-iso|gainmap-ci|gainmap-apple|heif10|inspect> …")
}
switch arguments[1] {
case "gainmap-iso" where arguments.count == 4:
    gainmapISO(Scene(path: arguments[2]), arguments[3])
case "gainmap-ci" where arguments.count == 4:
    gainmapCI(Scene(path: arguments[2]), arguments[3])
case "gainmap-apple" where arguments.count == 5:
    gainmapApple(Scene(path: arguments[2]), arguments[3], arguments[4])
case "heif10" where arguments.count == 5:
    heif10(Scene(path: arguments[2]), arguments[3], arguments[4])
case "heic-codes" where arguments.count == 5:
    heicCodes(arguments[2], arguments[3], arguments[4])
case "inspect":
    var columns = 0, rows = 0
    if arguments.count == 6, arguments[3] == "--patches", let c = Int(arguments[4]), let r = Int(arguments[5]) {
        columns = c
        rows = r
    } else if arguments.count != 3 {
        fail("usage: author inspect <path> [--patches COLS ROWS]")
    }
    inspect(arguments[2], columns: columns, rows: rows)
default:
    fail("unknown or malformed command \(arguments[1])")
}
