// A standalone fixture author for the host-still admission drive. It
// imports no Doctored code: every file is written by ImageIO or Core Image
// directly, then self-checked, so a drive never decodes its own output to
// decide what it wrote.
//
//   author <kind> <width> <height> <out> [<dx> <dy>]
//
//   heic8          8-bit HEIC, CGImageDestination, quality 0.9, sRGB
//   heic8-gainmap  8-bit HEIC carrying an HDR gain map (Core Image's HDR
//                  HEIF writer, Display P3 SDR base)
//   heic10         10-bit HEIC (CIContext.writeHEIF10Representation), sRGB
//   jpeg8          8-bit JPEG, CGImageDestination, quality 0.9, sRGB
//   png8 / png16   RGB PNG with no alpha, 8 or 16 bits per component, sRGB
//   heic8-pair     a two-image 8-bit HEIC (the shape of a light/dark dynamic
//                  wallpaper): index 0 is a (w/2)×(h/2) scene at 72 ppi,
//                  index 1 the w×h scene at 240 ppi, and the container's
//                  primary image is index 1
//
// The scene: R = 40+160·x/(w−1), G = 30+170·y/(h−1), B = 200−150·x/(w−1),
// plus xorshift32 noise of ±12 codes per channel from a fixed seed. Any
// failed self-check (dimensions, container depth, gain-map presence) exits
// nonzero. AVIF fixtures are `avifenc -s 8 -y 420 [-d 10]` over png8/png16.
//
// With the two trailing integers <dx> <dy> (jpeg8, png8 and png16 only; any
// other kind with them exits nonzero) the file holds the PLANE scene instead:
// grey value noise defined over the whole plane, pixel (x, y) sampled at
// (x + dx + 1/2, y + dy + 1/2), so two files written with different shifts
// show one picture moved by an exact whole-pixel translation — frames
// Auto-Align can register (the merge admission cases; the ramp has no
// features). It is `scripts/verify_v1_stack_align.py`'s `Picture`: the same
// integer lattice hash of (ix, iy, octave), five octaves weighted 1, 1/2, …
// 1/5, smoothstep interpolation, contrast 1.8 about mid-grey, with lattice
// cells of 256, 128, 64, 32 and 16 pixels. Without them the ramp scene is
// written exactly as before.
//
//   author <jpeg8|png16> <width> <height> <out> yaw <degrees> <focal>
//
// writes a YAWED view of the plane scene wrapped on a cylinder of radius
// <focal> (the Cylindrical and Spherical admission cases): a
// pinhole camera at the cylinder's axis, its principal point at the frame
// centre and its focal length <focal> pixels, turned <degrees> to the right.
// Pixel (x, y), with xc = x + 1/2 − width/2 and yc = y + 1/2 − height/2, sees
// θ = <degrees> + atan2(xc, f) and h = yc / hypot(xc, f), and takes the plane
// scene's value at (f·θ, f·h). Three such views a few degrees apart overlap
// as a panorama does, inside Auto-Align's registration reach. Any other kind
// with `yaw` exits nonzero.
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

struct XorShift32 {
    var state: UInt32 = 0x9E37_79B9
    mutating func next() -> UInt32 {
        state ^= state << 13
        state ^= state >> 17
        state ^= state << 5
        return state
    }
    /// An integer in −12…12.
    mutating func noise() -> Float { Float(Int(next() % 25) - 12) }
}

/// The scene as 8-bit-code floats (0…255, noise included, unclamped), one
/// RGB triple per pixel, row 0 = top. `body` sees (x, y, r, g, b).
func scene(_ width: Int, _ height: Int, _ body: (Int, Int, Float, Float, Float) -> Void) {
    var random = XorShift32()
    let wx = Float(max(width - 1, 1)), hy = Float(max(height - 1, 1))
    for y in 0..<height {
        let v = Float(y) / hy
        for x in 0..<width {
            let u = Float(x) / wx
            body(x, y, 40 + 160 * u + random.noise(), 30 + 170 * v + random.noise(),
                 200 - 150 * u + random.noise())
        }
    }
}

/// The plane scene's lattice: an integer hash of the cell, in 0…1
/// (`verify_v1_stack_align.py`'s `lattice`, in wrapping 32-bit arithmetic).
func lattice(_ ix: Int, _ iy: Int, _ octave: Int) -> Float {
    var h = UInt32(truncatingIfNeeded: ix) &* 374_761_393
        &+ UInt32(truncatingIfNeeded: iy) &* 668_265_263
        &+ UInt32(truncatingIfNeeded: octave + 1) &* 1_274_126_177
    h = (h ^ (h >> 13)) &* 1_274_126_177
    h ^= h >> 16
    return Float(h & 0xFFFF) / 65535
}

/// The plane scene's grey, 0…255, at the plane point (sx, sy).
func planeValue(_ sx: Float, _ sy: Float) -> Float {
    let cells: [Float] = [256, 128, 64, 32, 16]
    let weights: [Float] = [1, 1.0 / 2, 1.0 / 3, 1.0 / 4, 1.0 / 5]
    let weightSum = weights.reduce(0, +)
    var total: Float = 0
    for octave in 0..<cells.count {
        let fx = sx / cells[octave], fy = sy / cells[octave]
        let ix = fx.rounded(.down), iy = fy.rounded(.down)
        let tx = fx - ix, ty = fy - iy
        let ux = tx * tx * (3 - 2 * tx), uy = ty * ty * (3 - 2 * ty)
        let cx = Int(ix), cy = Int(iy)
        let a = lattice(cx, cy, octave), b = lattice(cx + 1, cy, octave)
        let c = lattice(cx, cy + 1, octave), d = lattice(cx + 1, cy + 1, octave)
        let top = a + (b - a) * ux, bottom = c + (d - c) * ux
        total += weights[octave] * (top + (bottom - top) * uy)
    }
    return 255 * max(0, min(1, (total / weightSum - 0.5) * 1.8 + 0.5))
}

/// The plane scene as 8-bit-code floats (0…255), grey, row 0 = top: pixel
/// (x, y) is the value noise at (x + dx + 1/2, y + dy + 1/2). `body` sees
/// (x, y, r, g, b) with r = g = b.
func plane(_ width: Int, _ height: Int, dx: Int, dy: Int,
           _ body: (Int, Int, Float, Float, Float) -> Void) {
    for y in 0..<height {
        let sy = Float(y + dy) + 0.5
        for x in 0..<width {
            let grey = planeValue(Float(x + dx) + 0.5, sy)
            body(x, y, grey, grey, grey)
        }
    }
}

/// The yawed cylinder view of the plane scene (the header's `yaw` form):
/// pixel (x, y) takes the plane's value at (f·θ, f·h) of its centre's ray.
func cylinder(_ width: Int, _ height: Int, yaw: Double, focal f: Double,
              _ body: (Int, Int, Float, Float, Float) -> Void) {
    let turn = yaw * Double.pi / 180
    for y in 0..<height {
        let yc = Double(y) + 0.5 - Double(height) / 2
        for x in 0..<width {
            let xc = Double(x) + 0.5 - Double(width) / 2
            let theta = turn + atan2(xc, f)
            let h = yc / hypot(xc, f)
            let grey = planeValue(Float(f * theta), Float(f * h))
            body(x, y, grey, grey, grey)
        }
    }
}

/// The scene the writers below draw: the ramp, or the plane at a shift.
var draw: (Int, Int, (Int, Int, Float, Float, Float) -> Void) -> Void = scene

func clamp8(_ value: Float) -> UInt8 { UInt8(max(0, min(255, value.rounded()))) }
func clamp16(_ value: Float) -> UInt16 { UInt16(max(0, min(65535, (value * 257).rounded()))) }

let srgb = CGColorSpace(name: CGColorSpace.sRGB)!

func image(_ data: Data, _ width: Int, _ height: Int, bits: Int, pixel: Int,
           info: CGBitmapInfo, space: CGColorSpace = srgb) -> CGImage {
    guard let provider = CGDataProvider(data: data as CFData),
          let image = CGImage(width: width, height: height, bitsPerComponent: bits,
            bitsPerPixel: pixel, bytesPerRow: width * pixel / 8, space: space, bitmapInfo: info,
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent) else {
        fail("cannot build a \(bits)-bit scene image")
    }
    return image
}

/// Opaque RGBX8 (noneSkipLast), so no encoder stores an alpha plane.
func rgbx8(_ width: Int, _ height: Int) -> CGImage {
    var bytes = Data(count: width * height * 4)
    bytes.withUnsafeMutableBytes { raw in
        let p = raw.bindMemory(to: UInt8.self)
        draw(width, height) { x, y, r, g, b in
            let at = (y * width + x) * 4
            p[at] = clamp8(r); p[at + 1] = clamp8(g); p[at + 2] = clamp8(b); p[at + 3] = 255
        }
    }
    return image(bytes, width, height, bits: 8, pixel: 32,
                 info: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue))
}

func rgb8(_ width: Int, _ height: Int) -> CGImage {
    var bytes = Data(count: width * height * 3)
    bytes.withUnsafeMutableBytes { raw in
        let p = raw.bindMemory(to: UInt8.self)
        draw(width, height) { x, y, r, g, b in
            let at = (y * width + x) * 3
            p[at] = clamp8(r); p[at + 1] = clamp8(g); p[at + 2] = clamp8(b)
        }
    }
    return image(bytes, width, height, bits: 8, pixel: 24,
                 info: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue))
}

func rgb16(_ width: Int, _ height: Int) -> CGImage {
    var bytes = Data(count: width * height * 6)
    bytes.withUnsafeMutableBytes { raw in
        let p = raw.bindMemory(to: UInt16.self)
        draw(width, height) { x, y, r, g, b in
            let at = (y * width + x) * 3
            p[at] = clamp16(r).littleEndian; p[at + 1] = clamp16(g).littleEndian
            p[at + 2] = clamp16(b).littleEndian
        }
    }
    return image(bytes, width, height, bits: 16, pixel: 48,
                 info: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue
                    | CGBitmapInfo.byteOrder16Little.rawValue))
}

func write(_ image: CGImage, type: String, to out: String, quality: Double? = nil) {
    guard let destination = CGImageDestinationCreateWithURL(URL(fileURLWithPath: out) as CFURL,
        type as CFString, 1, nil) else { fail("cannot create a \(type) destination") }
    var options: [CFString: Any] = [:]
    if let quality { options[kCGImageDestinationLossyCompressionQuality] = quality }
    CGImageDestinationAddImage(destination, image, options as CFDictionary)
    guard CGImageDestinationFinalize(destination) else { fail("the \(type) writer failed") }
}

/// Two images in one HEIC, the primary one second, so opening index 0 instead
/// of the primary image gives the wrong size, and reading index 0's
/// properties instead of the primary image's gives the wrong resolution.
func pair(_ width: Int, _ height: Int, _ out: String) {
    guard let destination = CGImageDestinationCreateWithURL(URL(fileURLWithPath: out) as CFURL,
        "public.heic" as CFString, 2, nil) else { fail("cannot create a HEIC destination") }
    CGImageDestinationAddImage(destination, rgbx8(width / 2, height / 2),
        [kCGImageDestinationLossyCompressionQuality: 0.9,
         kCGImagePropertyDPIWidth: 72, kCGImagePropertyDPIHeight: 72] as CFDictionary)
    CGImageDestinationAddImage(destination, rgbx8(width, height),
        [kCGImageDestinationLossyCompressionQuality: 0.9, kCGImagePropertyPrimaryImage: true,
         kCGImagePropertyDPIWidth: 240, kCGImagePropertyDPIHeight: 240] as CFDictionary)
    guard CGImageDestinationFinalize(destination) else { fail("the HEIC pair writer failed") }
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: out) as CFURL, nil),
          CGImageSourceGetCount(source) == 2 else { fail("the HEIC pair does not hold two images") }
    guard CGImageSourceGetPrimaryImageIndex(source) == 1 else {
        fail("the HEIC pair's primary image is \(CGImageSourceGetPrimaryImageIndex(source)), expected 1")
    }
}

func heic10(_ width: Int, _ height: Int, _ out: String) {
    let context = CIContext(options: [.workingColorSpace: CGColorSpace(name: CGColorSpace.extendedLinearSRGB)!])
    do {
        try context.writeHEIF10Representation(of: CIImage(cgImage: rgb16(width, height)),
            to: URL(fileURLWithPath: out), colorSpace: srgb, options: [:])
    } catch { fail("Core Image HEIF10 writer failed: \(error)") }
}

/// The scene in extended linear Display P3 with a ×3.2 highlight disc (the
/// opaque-stills oracle's construction, scaled to the canvas), written as an
/// 8-bit SDR base plus a gain map by Core Image's HDR HEIF writer.
func gainmap(_ width: Int, _ height: Int, _ out: String) {
    func linear(_ code: Float) -> Float {
        let c = max(0, min(255, code)) / 255
        return c <= 0.04045 ? c / 12.92 : powf((c + 0.055) / 1.055, 2.4)
    }
    let cx = Float(width) * 0.625, cy = Float(height) * 0.42, radius = Float(height) * 0.29
    var floats = Data(count: width * height * 16)
    floats.withUnsafeMutableBytes { raw in
        let p = raw.bindMemory(to: Float.self)
        scene(width, height) { x, y, r, g, b in
            let dx = Float(x) - cx, dy = Float(y) - cy
            let highlight = max(0, 1 - (dx * dx + dy * dy).squareRoot() / radius) * 3.2
            let at = (y * width + x) * 4
            p[at] = linear(r) + highlight
            p[at + 1] = linear(g) + highlight * 0.9
            p[at + 2] = linear(b) + highlight * 0.8
            p[at + 3] = 1
        }
    }
    guard let space = CGColorSpace(name: CGColorSpace.extendedLinearDisplayP3),
          let p3 = CGColorSpace(name: CGColorSpace.displayP3) else { fail("no Display P3 spaces") }
    let float = image(floats, width, height, bits: 32, pixel: 128,
        info: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue
            | CGBitmapInfo.floatComponents.rawValue | CGBitmapInfo.byteOrder32Little.rawValue),
        space: space)
    let extent = CGRect(x: 0, y: 0, width: width, height: height)
    let hdr = CIImage(cgImage: float).settingAlphaOne(in: extent)
    let sdr = hdr.applyingFilter("CIToneMapHeadroom",
        parameters: ["inputSourceHeadroom": 4.0, "inputTargetHeadroom": 1.0]).settingAlphaOne(in: extent)
    let context = CIContext(options: [.workingColorSpace: space])
    do {
        try context.writeHEIFRepresentation(of: sdr, to: URL(fileURLWithPath: out), format: .RGBA8,
            colorSpace: p3, options: [.hdrImage: hdr])
    } catch { fail("Core Image HDR HEIF writer failed: \(error)") }
}

func check(_ out: String, width: Int, height: Int, depth: Int, gainMap: Bool, index: Int = 0,
           ppi: Int? = nil) {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: out) as CFURL,
              [kCGImageSourceShouldCache: false] as CFDictionary),
          let info = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any] else {
        fail("cannot inspect \(out)")
    }
    let w = (info[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue
    let h = (info[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue
    let d = (info[kCGImagePropertyDepth] as? NSNumber)?.intValue
    guard w == width, h == height else { fail("dimensions \(String(describing: w))x\(String(describing: h)), expected \(width)x\(height)") }
    guard d == depth else { fail("container depth \(String(describing: d)), expected \(depth)") }
    if let ppi {
        let x = (info[kCGImagePropertyDPIWidth] as? NSNumber)?.intValue
        let y = (info[kCGImagePropertyDPIHeight] as? NSNumber)?.intValue
        guard x == ppi, y == ppi else { fail("index \(index) resolution \(String(describing: x))x\(String(describing: y)), expected \(ppi)") }
    }
    let maps = [kCGImageAuxiliaryDataTypeHDRGainMap, kCGImageAuxiliaryDataTypeISOGainMap].contains {
        CGImageSourceCopyAuxiliaryDataInfoAtIndex(source, index, $0) != nil
    }
    guard maps == gainMap else { fail(gainMap ? "no gain map in the written file" : "an unexpected gain map") }
}

let arguments = CommandLine.arguments
guard arguments.count == 5 || arguments.count == 7 || arguments.count == 8,
      let width = Int(arguments[2]), let height = Int(arguments[3]), width > 1, height > 1 else {
    fail("usage: author <heic8|heic8-gainmap|heic10|jpeg8|png8|png16|heic8-pair> <width> <height> <out> [<dx> <dy> | yaw <degrees> <focal>]")
}
let kind = arguments[1], out = arguments[4]
if arguments.count == 8 {
    guard arguments[5] == "yaw", let degrees = Double(arguments[6]), let focal = Double(arguments[7]),
          degrees.isFinite, focal.isFinite, focal > 0 else {
        fail("the yawed scene is `yaw <degrees> <focal>`, with a finite angle and a focal above 0")
    }
    guard ["jpeg8", "png16"].contains(kind) else {
        fail("the yawed scene (yaw <degrees> <focal>) is written as jpeg8 or png16, not \(kind)")
    }
    draw = { width, height, body in cylinder(width, height, yaw: degrees, focal: focal, body) }
}
if arguments.count == 7 {
    guard let dx = Int(arguments[5]), let dy = Int(arguments[6]) else {
        fail("<dx> and <dy> must be integers")
    }
    guard ["jpeg8", "png8", "png16"].contains(kind) else {
        fail("the plane scene (<dx> <dy>) is written as jpeg8, png8 or png16, not \(kind)")
    }
    draw = { width, height, body in plane(width, height, dx: dx, dy: dy, body) }
}
switch kind {
case "heic8":
    write(rgbx8(width, height), type: "public.heic", to: out, quality: 0.9)
    check(out, width: width, height: height, depth: 8, gainMap: false)
case "heic8-gainmap":
    gainmap(width, height, out)
    check(out, width: width, height: height, depth: 8, gainMap: true)
case "heic10":
    heic10(width, height, out)
    check(out, width: width, height: height, depth: 10, gainMap: false)
case "jpeg8":
    write(rgbx8(width, height), type: "public.jpeg", to: out, quality: 0.9)
    check(out, width: width, height: height, depth: 8, gainMap: false)
case "png8":
    write(rgb8(width, height), type: "public.png", to: out)
    check(out, width: width, height: height, depth: 8, gainMap: false)
case "heic8-pair":
    pair(width, height, out)
    check(out, width: width / 2, height: height / 2, depth: 8, gainMap: false, index: 0, ppi: 72)
    check(out, width: width, height: height, depth: 8, gainMap: false, index: 1, ppi: 240)
case "png16":
    write(rgb16(width, height), type: "public.png", to: out)
    check(out, width: width, height: height, depth: 16, gainMap: false)
default:
    fail("unknown kind \(kind)")
}
