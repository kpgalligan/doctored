import AppKit
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

// `PDFExport` runs the ORDINARY export chain at `format = .tiff` and the page's depth (8, or 16 at
// 16-bit and float) and draws that intermediate into a one-page PDF at the document's
// own print size, so no delivery setting is re-implemented; `PDFExportOptions` (depth, ZIP or 8-bit
// JPEG, quality) is what `PDFExportAccessoryController.swift`'s save-panel accessory and
// `export_pdf` both build, JPEG is the system ImageIO encoder with alpha as a separate soft mask,
// and the bits/filter/soft mask reported are read back from the written PDF.

/// How the page's one image is compressed. ZIP is lossless (Flate) at 8 or 16
/// bits; JPEG is the system's own (ImageIO) lossy encoder, 8 bits only.
enum PDFCompression: String {
    case zip, jpeg
}

/// What the page is written as: its image depth and compression. The UI's
/// save-panel accessory and `export_pdf` build the same value.
struct PDFExportOptions: Equatable {
    var compression: PDFCompression
    var bitDepth: Int
    var jpegQuality: Int

    /// ZIP at the document's remembered export depth, capped at 16: a byte
    /// document 8, a typed one its `exportBitDepth`, a float one 16 (its own
    /// 32-bit default has no PDF image depth).
    static func defaults(for document: ImageDocument) -> PDFExportOptions {
        PDFExportOptions(
            compression: .zip,
            bitDepth: min(ExportSettings.defaults(for: document).bitDepth, 16),
            jpegQuality: document.jpegExportQuality)
    }

    /// Refuses what no PDF page here can hold. A 16-bit page from a byte
    /// document is NOT refused here: the export chain's own sentence reaches
    /// the caller.
    func validate() throws {
        guard bitDepth == 8 || bitDepth == 16 else {
            throw DoctoredCoreError(
                message: "A PDF page holds an 8-bit or 16-bit image; bit_depth must be 8 or 16.")
        }
        if compression == .jpeg, bitDepth == 16 {
            throw DoctoredCoreError(message: "JPEG compression is 8-bit only; choose ZIP to keep 16 bits.")
        }
    }
}

/// What a PDF export produced, so the UI and MCP report the same numbers.
/// `bitsPerComponent`, `compression` and `hasSoftMask` are MEASURED from the
/// written file, never the request repeated back.
struct PDFExportReport {
    let pageSizePoints: CGSize
    let ppi: (x: Double, y: Double)
    let width: Int
    let height: Int
    let profileName: String
    let carried: UInt32
    let bytes: Int
    let bitsPerComponent: Int
    let compression: String
    let requestedCompression: PDFCompression
    let hasSoftMask: Bool
}

/// Dedicated single-page PDF EXPORT (Phase 11D, plus 16 bits and JPEG):
/// the flattened document as one page, at its own print size, with its own
/// profile embedded.
///
/// # It is a raster page, deliberately
///
/// The page holds ONE image — the same pixels the 11A export chain produces
/// at the same settings — rather than vector content, because a Doctored
/// document IS pixels: its text and shape layers are rendered by the core at
/// the document's depth and there is nothing vector left to preserve by the
/// time an export runs. A vector-preserving export would be a different
/// feature over the described-layer descriptions.
///
/// # Why it runs the export chain first
///
/// Every setting a deliverable has — size, resampling filter, output profile,
/// intent, black point, sharpening, metadata policy, dither — belongs to
/// `ExportCapture`/`ExportSettings`, and re-implementing any of it here would
/// be a second copy that drifts. So the export runs the ordinary chain with
/// `format = .tiff` at the page's depth (8 or 16 bits; TIFF is the lossless
/// container the intermediate needs), reads that intermediate back through
/// ImageIO into an ICC-tagged `CGImage`, and draws it into a one-page PDF
/// context. A 16-bit page therefore carries the 16-bit PNG/TIFF export's
/// samples, and a float document's page is its tone-mapped projection, as
/// every other SDR deliverable from it is.
///
/// # ZIP and JPEG
///
/// ZIP is what Quartz writes for a drawn bitmap (Flate). JPEG hands Quartz a
/// JPEG-backed image, which it passes through as a DCTDecode stream: the
/// colour is the system (ImageIO) encoder's at `jpegQuality`, so it is not
/// byte-comparable with `export_image`'s own JPEG encoder at the same number.
/// Transparency travels as a separate 8-bit soft mask, never matted into the
/// colour. What the file actually holds — bits per component, filter, soft
/// mask — is read back from the written PDF (`measure`); a depth the writer
/// did not honour fails the export, a compression it did not honour is
/// reported.
///
/// # Where it runs, and its peak memory
///
/// `run` encodes on the `BackgroundWork` worker with progress and Cancel.
/// Its stated peak is `scratchEstimate`: the intermediate encode's own
/// estimate (which also covers the core's TIFF buffer the result keeps),
/// plus three output-sized RGBA planes at the page's depth that coexist
/// afterwards — the host's copy of the TIFF intermediate, the decoded
/// `CGImage` and the PDF data — and, for JPEG, a pixel copy, the alpha plane
/// and the JPEG bytes.
enum PDFExport {
    /// The TIFF intermediate the page is drawn from, at the page's depth.
    /// Dither is untouched: the caller owns it.
    private static func rasterSettings(_ settings: ExportSettings,
                                       options: PDFExportOptions) -> ExportSettings {
        var raster = settings
        raster.format = .tiff
        raster.bitDepth = options.bitDepth
        raster.jpegQuality = options.jpegQuality
        return raster
    }

    /// The worker's stated peak: the intermediate encode's estimate plus the
    /// TIFF copy, the decoded image and the PDF data at the page's depth, and
    /// JPEG's own pixel copy, alpha plane and JPEG bytes.
    static func scratchEstimate(_ capture: ExportCapture, settings: ExportSettings,
                                options: PDFExportOptions) throws -> UInt64 {
        let raster = rasterSettings(settings, options: options)
        let (width, height) = try capture.outputSize(raster)
        let pixels = UInt64(width) * UInt64(height)
        let jpeg: UInt64 = options.compression == .jpeg ? 9 * pixels : 0
        return try capture.scratchEstimate(raster)
            + 3 * pixels * (options.bitDepth == 16 ? 8 : 4) + jpeg
    }

    /// Encodes on the bounded worker with the progress panel and Cancel
    /// (Esc) up. Main thread only, like every other `BackgroundWork.wait`
    /// caller; the save panel and `write` stay with the caller on main.
    static func run(_ capture: ExportCapture, settings: ExportSettings,
                    options: PDFExportOptions,
                    title: String) throws -> (data: Data, report: PDFExportReport) {
        // #1: spot channels refuse a PDF by name until 1c writes them.
        if let refusal = SpotChannels.pdfRefusal(capture.source) { throw DoctoredCoreError(message: refusal) }
        let scratch = try scratchEstimate(capture, settings: settings, options: options)
        _ = try RasterResources.shared.admit(scratch: scratch)
        let parent = try RasterJobControl()
        try parent.beginCompoundOperation()
        return try BackgroundWork.wait(title: "Export as PDF", control: parent, makeTask: {
            try WorkerTask(scratchEstimate: scratch) {
                try encode(capture, settings: settings, options: options, title: title,
                           control: parent)
            }
        })
    }

    /// Renders one page. `title` becomes the PDF's `/Title`. Worker-safe:
    /// no AppKit. A non-nil `control` is a compound parent
    /// (`beginCompoundOperation()` already called); the intermediate encode
    /// runs as its child over the first 80% and a cancel is honoured after
    /// the decode, after the JPEG encode and after the page closes.
    static func encode(_ capture: ExportCapture, settings: ExportSettings,
                       options: PDFExportOptions, title: String,
                       control: RasterJobControl?) throws -> (data: Data, report: PDFExportReport) {
        if let refusal = SpotChannels.pdfRefusal(capture.source) { throw DoctoredCoreError(message: refusal) }
        let raster = rasterSettings(settings, options: options)
        let result: RasterExportResult
        if let control {
            result = try control.withChildOperation(from: 0, to: 0.8) { child in
                try capture.encode(raster, control: child)
            }
        } else {
            result = try capture.encode(raster, control: nil)
        }
        let info = try result.info()
        let width = (info["width"] as? NSNumber)?.intValue ?? 0
        let height = (info["height"] as? NSNumber)?.intValue ?? 0
        guard width > 0, height > 0 else {
            throw DoctoredCoreError(message: "The export produced no pixels.")
        }
        guard let source = CGImageSourceCreateWithData(result.bytes as CFData, nil),
              CGImageSourceGetCount(source) > 0,
              let decoded = CGImageSourceCreateImageAtIndex(source, 0, [
                  kCGImageSourceShouldCache: false,
              ] as CFDictionary)
        else {
            throw DoctoredCoreError(message: "Could not read the export's intermediate image.")
        }
        if control?.isCancelled == true {
            throw JobCancelled()
        }
        let image: CGImage
        if options.compression == .jpeg {
            image = try jpegImage(from: decoded, quality: options.jpegQuality)
            if control?.isCancelled == true {
                throw JobCancelled()
            }
        } else {
            image = decoded
        }

        // The page is the document's PRINT size: the pixels divided by the
        // resolution, in the 72-per-inch units PDF measures in. A page sized
        // from the pixel count alone would print at the wrong size on every
        // document whose ppi is not 72.
        let resolution = capture.source.resolution
        let ppiX = resolution.x.isFinite && resolution.x > 0 ? resolution.x : 72
        let ppiY = resolution.y.isFinite && resolution.y > 0 ? resolution.y : 72
        let pageWidth = CGFloat(Double(width) * 72.0 / ppiX)
        let pageHeight = CGFloat(Double(height) * 72.0 / ppiY)
        guard pageWidth > 0, pageHeight > 0 else {
            throw DoctoredCoreError(message: "The page size this document implies is empty.")
        }
        var mediaBox = CGRect(x: 0, y: 0, width: pageWidth, height: pageHeight)

        let output = NSMutableData()
        guard let consumer = CGDataConsumer(data: output),
              let context = CGContext(consumer: consumer, mediaBox: &mediaBox, [
                  kCGPDFContextTitle: title,
                  kCGPDFContextCreator: "Doctored",
              ] as CFDictionary)
        else {
            throw DoctoredCoreError(message: "Could not open a PDF context.")
        }
        context.beginPDFPage(nil)
        context.interpolationQuality = .high
        context.draw(image, in: mediaBox)
        context.endPDFPage()
        context.closePDF()
        if control?.isCancelled == true {
            throw JobCancelled()
        }

        let data = output as Data
        guard !data.isEmpty else {
            throw DoctoredCoreError(message: "The PDF writer produced no bytes.")
        }
        let written = try measure(data)
        guard written.bitsPerComponent == options.bitDepth else {
            throw DoctoredCoreError(
                message: "The PDF writer stored \(written.bitsPerComponent) bits per component "
                    + "where \(options.bitDepth) were requested; nothing was written.")
        }
        let report = PDFExportReport(
            pageSizePoints: CGSize(width: pageWidth, height: pageHeight),
            ppi: (ppiX, ppiY), width: width, height: height,
            profileName: (info["profile_name"] as? String) ?? "",
            // The page carries the image's ICC profile as an ICCBased stream;
            // a PDF has nowhere this build writes the capture packets, so the
            // report states the profile alone rather than the TIFF's bits.
            carried: image.colorSpace?.copyICCData() != nil ? DR_CARRIES_PROFILE : 0,
            bytes: data.count,
            bitsPerComponent: written.bitsPerComponent,
            compression: written.compression,
            requestedCompression: options.compression,
            hasSoftMask: written.hasSoftMask)
        return (data, report)
    }

    /// The decoded 8-bit intermediate as a JPEG-backed image Quartz passes
    /// through as DCTDecode, with any transparency as a separate 8-bit alpha
    /// image (a soft mask). The colour is encoded as stored: nothing is
    /// matted, so a transparent pixel keeps its RGB under a zero mask.
    private static func jpegImage(from image: CGImage, quality: Int) throws -> CGImage {
        let alphaInfo = image.alphaInfo
        let order = image.bitmapInfo.intersection(.byteOrderMask)
        let straight = alphaInfo == .last && image.bitsPerPixel == 32
        let opaque = (alphaInfo == .none || alphaInfo == .noneSkipLast)
            && (image.bitsPerPixel == 24 || image.bitsPerPixel == 32)
        let rgbaOrder = order == [] || order == .byteOrder32Big
        guard image.bitsPerComponent == 8, (straight && rgbaOrder) || opaque,
              let space = image.colorSpace, let provider = image.dataProvider
        else {
            throw DoctoredCoreError(
                message: "The export's intermediate image has an unexpected layout for JPEG compression.")
        }
        // The RGB view: the same provider, with the fourth byte skipped.
        let rgb: CGImage
        if straight {
            guard let view = CGImage(
                width: image.width, height: image.height, bitsPerComponent: 8,
                bitsPerPixel: 32, bytesPerRow: image.bytesPerRow, space: space,
                bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue)
                    .union(order),
                provider: provider, decode: nil, shouldInterpolate: false,
                intent: .defaultIntent)
            else {
                throw DoctoredCoreError(message: "Could not prepare the image for JPEG compression.")
            }
            rgb = view
        } else {
            rgb = image
        }

        let encoded = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(
            encoded, UTType.jpeg.identifier as CFString, 1, nil)
        else {
            throw DoctoredCoreError(message: "This system has no JPEG encoder.")
        }
        CGImageDestinationAddImage(destination, rgb, [
            kCGImageDestinationLossyCompressionQuality: max(0.0, min(1.0, Double(quality) / 100.0)),
        ] as CFDictionary)
        guard CGImageDestinationFinalize(destination),
              let jpegProvider = CGDataProvider(data: encoded as CFData),
              let decodedJPEG = CGImage(
                  jpegDataProviderSource: jpegProvider, decode: nil, shouldInterpolate: true,
                  intent: .defaultIntent)
        else {
            throw DoctoredCoreError(message: "The system JPEG encoder could not compress the page.")
        }
        // Keep the intermediate's own profile on the page's ICCBased stream.
        let jpeg = decodedJPEG.colorSpace?.copyICCData() == nil
            ? decodedJPEG.copy(colorSpace: space) ?? decodedJPEG : decodedJPEG
        guard straight, let alpha = try alphaPlane(of: image) else { return jpeg }
        guard let masked = jpeg.masking(alpha) else {
            throw DoctoredCoreError(message: "Could not attach the page's transparency.")
        }
        return masked
    }

    /// The straight alpha bytes of a 32-bit RGBA image as an 8-bit DeviceGray
    /// image, or nil when every pixel is opaque. Container marshalling only:
    /// the fourth byte of each pixel is copied out of the provider's bytes,
    /// no sample is computed.
    private static func alphaPlane(of image: CGImage) throws -> CGImage? {
        guard let bytes = image.dataProvider?.data else {
            throw DoctoredCoreError(message: "Could not read the page's transparency.")
        }
        let width = image.width, height = image.height, stride = image.bytesPerRow
        let data = bytes as Data
        guard data.count >= stride * (height - 1) + width * 4 else {
            throw DoctoredCoreError(message: "Could not read the page's transparency.")
        }
        var plane = [UInt8](repeating: 255, count: width * height)
        var opaque = true
        data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            for y in 0..<height {
                let row = y * stride
                for x in 0..<width {
                    let value = raw[row + x * 4 + 3]
                    plane[y * width + x] = value
                    if value != 255 { opaque = false }
                }
            }
        }
        if opaque { return nil }
        guard let provider = CGDataProvider(data: Data(plane) as CFData),
              let alpha = CGImage(
                  width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 8,
                  bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(),
                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
                  provider: provider, decode: nil, shouldInterpolate: false,
                  intent: .defaultIntent)
        else {
            throw DoctoredCoreError(message: "Could not prepare the page's transparency.")
        }
        return alpha
    }

    /// Reads back page 1's image XObject from the written PDF: its bits per
    /// component, its filter (`zip`, `jpeg`, `none` or the filter's own name
    /// lowercased) and whether it references a soft mask. The colour image is
    /// the one with a soft mask or a colour space other than DeviceGray.
    private static func measure(_ pdf: Data) throws -> (bitsPerComponent: Int,
                                                         compression: String,
                                                         hasSoftMask: Bool) {
        // The dictionaries below are borrowed from `document`, which must
        // outlive every read of them.
        guard let provider = CGDataProvider(data: pdf as CFData),
              let document = CGPDFDocument(provider)
        else {
            throw DoctoredCoreError(message: "The written PDF has no image to measure.")
        }
        return try withExtendedLifetime(document) {
            try measureImage(on: document.page(at: 1)?.dictionary)
        }
    }

    private static func measureImage(on page: CGPDFDictionaryRef?) throws
        -> (bitsPerComponent: Int, compression: String, hasSoftMask: Bool) {
        var images: [CGPDFDictionaryRef] = []
        if let page {
            var resources: CGPDFDictionaryRef?
            var objects: CGPDFDictionaryRef?
            if CGPDFDictionaryGetDictionary(page, "Resources", &resources), let resources,
               CGPDFDictionaryGetDictionary(resources, "XObject", &objects), let objects {
                CGPDFDictionaryApplyBlock(objects, { _, object, _ in
                    var stream: CGPDFStreamRef?
                    var subtype: UnsafePointer<CChar>?
                    if CGPDFObjectGetValue(object, .stream, &stream), let stream,
                       let dictionary = CGPDFStreamGetDictionary(stream),
                       CGPDFDictionaryGetName(dictionary, "Subtype", &subtype), let subtype,
                       String(cString: subtype) == "Image" {
                        images.append(dictionary)
                    }
                    return true
                }, nil)
            }
        }
        func hasSoftMask(_ dictionary: CGPDFDictionaryRef) -> Bool {
            var object: CGPDFObjectRef?
            return CGPDFDictionaryGetObject(dictionary, "SMask", &object)
        }
        func isGray(_ dictionary: CGPDFDictionaryRef) -> Bool {
            var name: UnsafePointer<CChar>?
            return CGPDFDictionaryGetName(dictionary, "ColorSpace", &name)
                && name.map { String(cString: $0) } == "DeviceGray"
        }
        guard let colour = images.first(where: { hasSoftMask($0) || !isGray($0) }) ?? images.first
        else {
            throw DoctoredCoreError(message: "The written PDF has no image to measure.")
        }
        var bits: CGPDFInteger = 0
        _ = CGPDFDictionaryGetInteger(colour, "BitsPerComponent", &bits)
        var filter: UnsafePointer<CChar>?
        var filters: CGPDFArrayRef?
        if !CGPDFDictionaryGetName(colour, "Filter", &filter),
           CGPDFDictionaryGetArray(colour, "Filter", &filters), let filters {
            _ = CGPDFArrayGetName(filters, 0, &filter)
        }
        let compression: String
        switch filter.map({ String(cString: $0) }) {
        case nil: compression = "none"
        case "FlateDecode": compression = "zip"
        case "DCTDecode": compression = "jpeg"
        case let other?: compression = other.lowercased()
        }
        return (Int(bits), compression, hasSoftMask(colour))
    }

    /// Writes `data` atomically, refusing an existing path unless
    /// `overwrite` — the `ExportResult::write` rule, restated here because
    /// this file's bytes never pass through the core.
    static func write(_ data: Data, to url: URL, overwrite: Bool) throws {
        if !overwrite, FileManager.default.fileExists(atPath: url.path) {
            throw DoctoredCoreError(
                message: "\(url.path) already exists. This export never overwrites a file.")
        }
        StagedFile.sweep(beside: url)
        let temporary = try DurableFile.writeTemporary(data, beside: url, prefix: StagedFile.prefix(.pdf))
        do {
            try DurableFile.publish(temporary, to: url)
        } catch {
            try? FileManager.default.removeItem(at: temporary)
            throw error
        }
    }
}
