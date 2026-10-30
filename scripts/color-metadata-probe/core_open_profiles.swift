import AppKit
import Accelerate
import ImageIO
import UniformTypeIdentifiers

// The core's flat open: a Gray-profiled grayscale file and a
// CMYK-profiled CMYK TIFF or JPEG (under an explicit rgba8 open since
// #547, which opens one as a CMYK document by default) are converted from that profile to
// sRGB as they are read, and reported. Every fixture is written by ImageIO
// and read back by the CORE — the preconditions prove no other path (ImageIO
// fallback, host source, typed import) took it. The ImageIO-written CMYK JPEG
// is the qualification of the core's Adobe APP14 inversion rule.

private let coreStripeWidth = 8
/// The four colour stripes, then the rich-dark one BPC is checked on.
private let coreCMYKStripes: [[UInt8]] = [
    [191, 0, 0, 0], [0, 191, 128, 0], [64, 64, 64, 64], [0, 0, 0, 0], [120, 120, 120, 240],
]
/// ColorSync's own relative-colorimetric draw against the core, per channel.
private let grayTolerance = 3
private let cmykTolerance = 8
/// The rich-dark stripe against ColorSync WITH black-point compensation.
/// Measured 1 for the TIFF and the JPEG alike (core 5,4,2 against 6,5,2,
/// since the lcms black-point rule; it was 2, core 4,4,2); ColorSync
/// without BPC sits 13 away, so this bound can tell the two intents apart.
private let darkBPCTolerance = 4

private func coreSentence(_ name: String, _ tail: String) -> String {
    "The embedded colour profile “\(name)” is \(tail)"
}

private func grayFixture() throws -> CGImage {
    let (width, height) = (32, 8)
    var values: [UInt8] = []
    for _ in 0..<height { for x in 0..<width { values.append(UInt8(x * 8)) } }
    // `CGColorSpace.genericGray` (Generic Gray Profile, γ1.8) is unavailable
    // to Swift by name; its Gamma 2.2 sibling carries an sRGB-shaped curve,
    // so converting it would be indistinguishable from not converting.
    guard let space = CGColorSpace(name: "kCGColorSpaceGenericGray" as CFString),
          let provider = CGDataProvider(data: Data(values) as CFData),
          let image = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 8,
            bytesPerRow: width, space: space, bitmapInfo: CGBitmapInfo(rawValue: 0),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    else { throw ProbeFailure(description: "gray fixture image") }
    return image
}

private func cmykSamples() -> [UInt8] {
    let width = coreStripeWidth * coreCMYKStripes.count
    var values: [UInt8] = []
    for _ in 0..<8 {
        for x in 0..<width { values.append(contentsOf: coreCMYKStripes[x / coreStripeWidth]) }
    }
    return values
}

private func cmykFixture() throws -> CGImage {
    let width = coreStripeWidth * coreCMYKStripes.count
    guard let space = CGColorSpace(name: CGColorSpace.genericCMYK),
          let provider = CGDataProvider(data: Data(cmykSamples()) as CFData),
          let image = CGImage(width: width, height: 8, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: width * 4, space: space, bitmapInfo: CGBitmapInfo(rawValue: 0),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent)
    else { throw ProbeFailure(description: "CMYK fixture image") }
    return image
}

private func writeFixture(_ image: CGImage, to url: URL, type: UTType) throws {
    guard let destination = CGImageDestinationCreateWithURL(url as CFURL,
        type.identifier as CFString, 1, nil) else {
        throw ProbeFailure(description: "ImageIO cannot write \(url.lastPathComponent)")
    }
    let options = [kCGImageDestinationLossyCompressionQuality as String: 1.0]
    CGImageDestinationAddImage(destination, image, options as CFDictionary)
    try require(CGImageDestinationFinalize(destination), "finalize \(url.lastPathComponent)")
}

private func decodedImage(_ url: URL) throws -> CGImage {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else { throw ProbeFailure(description: "ImageIO cannot decode \(url.lastPathComponent)") }
    return image
}

/// The probe's own ImageIO decode drawn into sRGB at relative colorimetric.
private func relativeReference(_ image: CGImage) throws -> [UInt8] {
    let (width, height) = (image.width, image.height)
    var bytes = [UInt8](repeating: 0, count: width * height * 4)
    let ok = bytes.withUnsafeMutableBytes { raw -> Bool in
        guard let context = CGContext(
            data: raw.baseAddress, width: width, height: height, bitsPerComponent: 8,
            bytesPerRow: width * 4, space: ColorProfile.sRGB,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
        context.setRenderingIntent(.relativeColorimetric)
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        return true
    }
    try require(ok, "reference context")
    return bytes
}

/// The same CMYK samples converted by ColorSync with black-point compensation
/// (`CGColorConversionInfo` through vImage), RGBX.
private func bpcReference(_ samples: [UInt8], width: Int, height: Int,
                          space: CGColorSpace) throws -> [UInt8] {
    let options = [CGColor.conversionBlackPointCompensation: true] as CFDictionary
    guard let info = CGColorConversionInfo(optionsSrc: space, dst: ColorProfile.sRGB,
                                           options: options),
          var source = vImage_CGImageFormat(bitsPerComponent: 8, bitsPerPixel: 32,
            colorSpace: space, bitmapInfo: CGBitmapInfo(rawValue: 0)),
          var destination = vImage_CGImageFormat(bitsPerComponent: 8, bitsPerPixel: 32,
            colorSpace: ColorProfile.sRGB,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue))
    else { throw ProbeFailure(description: "ColorSync BPC conversion setup") }
    var error = vImage_Error(kvImageNoError)
    guard let converter = vImageConverter_CreateWithCGColorConversionInfo(
        info, &source, &destination, nil, vImage_Flags(kvImageNoFlags), &error)?
        .takeRetainedValue(), error == kvImageNoError
    else { throw ProbeFailure(description: "ColorSync BPC converter (\(error))") }
    var input = samples
    var output = [UInt8](repeating: 0, count: width * height * 4)
    input.withUnsafeMutableBytes { raw in
        output.withUnsafeMutableBytes { out in
            var from = vImage_Buffer(data: raw.baseAddress, height: vImagePixelCount(height),
                                     width: vImagePixelCount(width), rowBytes: width * 4)
            var to = vImage_Buffer(data: out.baseAddress, height: vImagePixelCount(height),
                                   width: vImagePixelCount(width), rowBytes: width * 4)
            error = vImageConvert_AnyToAny(converter, &from, &to, nil,
                                           vImage_Flags(kvImageNoFlags))
        }
    }
    try require(error == kvImageNoError, "ColorSync BPC conversion (\(error))")
    return output
}

private func rgbDifference(_ a: [UInt8], _ b: [UInt8], at pixel: Int) -> Int {
    (0..<3).map { abs(Int(a[pixel * 4 + $0]) - Int(b[pixel * 4 + $0])) }.max() ?? Int.max
}

func verifyCoreOpenProfiles() throws {
    let gray = probeDirectory.appendingPathComponent("core-gray.jpg")
    let cmykTIFF = probeDirectory.appendingPathComponent("core-cmyk.tif")
    let cmykJPEG = probeDirectory.appendingPathComponent("core-cmyk.jpg")
    try writeFixture(try grayFixture(), to: gray, type: .jpeg)
    let cmyk = try cmykFixture()
    try writeFixture(cmyk, to: cmykTIFF, type: .tiff)
    try writeFixture(cmyk, to: cmykJPEG, type: .jpeg)

    // Preconditions: ImageIO wrote what we asked, and only the core reads it.
    var names: [URL: String] = [:]
    for (url, model) in [(gray, "Gray"), (cmykTIFF, "CMYK"), (cmykJPEG, "CMYK")] {
        let file = url.lastPathComponent
        let properties = try independentProperties(url)
        try require(properties[kCGImagePropertyColorModel as String] as? String == model,
                    "precondition: \(file) is \(model)")
        try require(properties[kCGImagePropertyProfileName as String] is String,
                    "precondition: \(file) carries a profile")
        guard let icc = try decodedImage(url).colorSpace?.copyICCData() as Data? else {
            throw ProbeFailure(description: "precondition: \(file) decodes with its profile")
        }
        names[url] = RasterProfile.inspect(icc).name
        try require((try? RasterDocument.open(url: url)) != nil,
                    "precondition: the core opens \(file)")
        try require(!HostSourceInput.claims(url), "precondition: no host source claims \(file)")
        try require(!PrecisionImportRequest.isHighDepthImage(url),
                    "precondition: \(file) is not a high-depth import")
    }
    let expected: [URL: String] = [
        gray: coreSentence(names[gray] ?? "", "a Gray profile, so the image was converted "
            + "from it to sRGB as it was read."),
        cmykTIFF: coreSentence(names[cmykTIFF] ?? "", "a CMYK profile, so the image was "
            + "converted from it to sRGB as it was read."),
        cmykJPEG: coreSentence(names[cmykJPEG] ?? "", "a CMYK profile, so the image was "
            + "converted from it to sRGB as it was read."),
    ]

    // The report and the open, per fixture.
    var opened: [URL: ImageDocument] = [:]
    for (url, sentence) in expected {
        let file = url.lastPathComponent
        let report = RasterDocument.metadataImportReport(at: url)
        try require(report?["profile_refusal"] as? String == sentence,
                    "\(file) report: \(String(describing: report?["profile_refusal"]))")
        try require((report?["warnings"] as? [String] ?? []).contains(sentence),
                    "\(file) sentence is a warning")
        // #547: a CMYK file opens as a CMYK DOCUMENT by default; the conversion to
        // sRGB this case measures is the explicit 8-bit open's.
        if url != gray {
            PrecisionImportRequest.mode = .ask
            let cmykDocument = try ImageDocument.openPrecisionDocumentIfRequested(at: url)
            try require(cmykDocument?.isCmyk == true, "\(file) opens as a CMYK document by default")
        }
        let previousMode = PrecisionImportRequest.mode
        PrecisionImportRequest.mode = .rgba8
        defer { PrecisionImportRequest.mode = previousMode }
        let document = ImageDocument()
        try document.read(from: url, ofType: file.hasSuffix(".tif") ? "public.tiff" : "public.jpeg")
        try require(document.openProfileRefusal == sentence,
                    "\(file) openProfileRefusal: "
                    + String(describing: document.openProfileRefusal))
        try require(document.doc.iccProfile == RasterProfile.builtin(.sRGB), "\(file) is sRGB")
        opened[url] = document
    }

    // Gray pixels: every pixel within ±3 of ColorSync; the raw code is not.
    let grayImage = try decodedImage(gray)
    let grayReference = try relativeReference(grayImage)
    guard let grayPixels = opened[gray]?.doc.layerPixels(0), grayPixels.count == grayReference.count
    else { throw ProbeFailure(description: "gray pixels") }
    let pixelCount = grayPixels.count / 4
    let grayMax = (0..<pixelCount).map { rgbDifference(grayPixels, grayReference, at: $0) }
        .max() ?? 0
    try require(grayMax <= grayTolerance, "gray converted within ±\(grayTolerance): \(grayMax)")
    let rawGray = (0..<pixelCount).map { abs((($0 % 32) * 8) - Int(grayReference[$0 * 4])) }
        .max() ?? 0
    try require(rawGray >= 8, "the raw gray code misses the reference: \(rawGray)")

    // CMYK TIFF pixels at each stripe centre.
    let cmykReference = try relativeReference(try decodedImage(cmykTIFF))
    guard let cmykPixels = opened[cmykTIFF]?.doc.layerPixels(0),
          cmykPixels.count == cmykReference.count
    else { throw ProbeFailure(description: "CMYK pixels") }
    let width = coreStripeWidth * coreCMYKStripes.count
    let centres = coreCMYKStripes.indices.map {
        4 * width + $0 * coreStripeWidth + coreStripeWidth / 2
    }
    let colourCentres = centres.prefix(4)
    let cmykMax = colourCentres.map { rgbDifference(cmykPixels, cmykReference, at: $0) }.max() ?? 0
    try require(cmykMax <= cmykTolerance, "CMYK converted within ±\(cmykTolerance): \(cmykMax)")

    // CMYK JPEG pixels at each stripe centre, against ColorSync's own draw of
    // the same (lossy) decode — the APP14 inversion rule's qualification.
    let jpegReference = try relativeReference(try decodedImage(cmykJPEG))
    guard let jpegPixels = opened[cmykJPEG]?.doc.layerPixels(0),
          jpegPixels.count == jpegReference.count
    else { throw ProbeFailure(description: "CMYK JPEG pixels") }
    let jpegMax = colourCentres.map { rgbDifference(jpegPixels, jpegReference, at: $0) }.max() ?? 0
    try require(jpegMax <= cmykTolerance,
                "CMYK JPEG converted within ±\(cmykTolerance): \(jpegMax)")
    var naive = cmykReference
    for (index, centre) in colourCentres.enumerated() {
        let ink = coreCMYKStripes[index].map(Double.init)
        for channel in 0..<3 {
            let value = (255 - ink[channel]) * (1 - ink[3] / 255)
            naive[centre * 4 + channel] = UInt8(value.rounded())
        }
    }
    let naiveMiss = colourCentres.map { rgbDifference(naive, cmykReference, at: $0) }.max() ?? 0
    try require(naiveMiss >= 16, "the naive conversion misses the reference: \(naiveMiss)")

    // The rich-dark stripe: black-point compensation, which ColorSync's own
    // relative draw does not apply, so the two references must differ first.
    guard let dark = centres.last, let space = CGColorSpace(name: CGColorSpace.genericCMYK)
    else { throw ProbeFailure(description: "dark stripe") }
    let bpc = try bpcReference(cmykSamples(), width: width, height: 8, space: space)
    let intentGap = rgbDifference(cmykReference, bpc, at: dark)
    try require(intentGap >= 2 * darkBPCTolerance,
                "ColorSync with and without BPC differ at the dark stripe: \(intentGap)")
    let darkMiss = rgbDifference(cmykPixels, bpc, at: dark)
    try require(darkMiss <= darkBPCTolerance,
                "dark stripe converted with BPC within ±\(darkBPCTolerance): \(darkMiss) "
                + "(core \(Array(cmykPixels[dark * 4..<dark * 4 + 3])), "
                + "BPC \(Array(bpc[dark * 4..<dark * 4 + 3])))")
    let jpegDarkMiss = rgbDifference(jpegPixels, bpc, at: dark)
    try require(jpegDarkMiss <= darkBPCTolerance,
                "JPEG dark stripe converted with BPC within ±\(darkBPCTolerance): "
                + "\(jpegDarkMiss) (core \(Array(jpegPixels[dark * 4..<dark * 4 + 3])), "
                + "BPC \(Array(bpc[dark * 4..<dark * 4 + 3])))")

    print("core open profiles: gray max \(grayMax) (raw \(rawGray)), CMYK max \(cmykMax) "
          + "(naive \(naiveMiss)), dark-stripe BPC \(darkMiss) (without BPC \(intentGap)), "
          + "CMYK JPEG max \(jpegMax), JPEG dark-stripe BPC \(jpegDarkMiss) "
          + "(core TIFF \(Array(cmykPixels[dark * 4..<dark * 4 + 3])), "
          + "JPEG \(Array(jpegPixels[dark * 4..<dark * 4 + 3])), "
          + "BPC \(Array(bpc[dark * 4..<dark * 4 + 3])), "
          + "relative \(Array(cmykReference[dark * 4..<dark * 4 + 3])))")
    print("PASS: the core open converts Gray, CMYK-TIFF and CMYK-JPEG profiles to sRGB and "
          + "reports every profile it does not keep")
}
