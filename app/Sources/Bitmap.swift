import AppKit
import ImageIO

/// The CoreGraphics ⇄ core-buffer seam: turning what macOS decoded or drew
/// into the STRAIGHT (non-premultiplied) RGBA8 the FFI takes, row 0 = top.
///
/// It exists because a few pixel sources are the platform's and not the
/// core's — glyph rasterization (TextLayer), shape paths (ShapeLayer),
/// vector pages (VectorImport) — and
/// CoreGraphics only ever renders into PREMULTIPLIED buffers. Every one of
/// those paths converts here, so the unpremultiply rounding is written
/// exactly once, and the described-layer renderers share ONE context
/// builder (`renderStraightRGBA`) so their placement rule is written once
/// too (DescribedLayer.swift).
///
/// It is also where the COLOUR rule for ingest is enforced: **the platform
/// decodes, the core converts.** Every context here is built in a caller-
/// supplied space with no default, so a caller must say which numbers it
/// wants out — the source image's own (`ingestSpace`, which converts
/// nothing) or the document's (a glyph, a shape fill, a re-rendered frame,
/// which CoreGraphics converts into once). The single conversion between
/// spaces then belongs to the core, where it is tested.
enum Bitmap {
    /// `image` rendered into a `width × height` straight-alpha RGBA8 buffer
    /// (row 0 = top, exactly `width * height * 4` bytes): scaled to FIT and
    /// centered, with transparent margins if the aspect ratios differ, so
    /// nothing is stretched or cropped. nil for a degenerate size, one past
    /// the core's pixel cap, or a context CoreGraphics refuses to make.
    ///
    /// `space` is the space the RESULTING BYTES are in: CoreGraphics colour-
    /// matches `image` into it. Pass `ingestSpace(of:).space` to convert
    /// nothing, or a document's space to land the document's own numbers.
    static func straightRGBA(
        from image: CGImage, fitting width: Int, height: Int, space: CGColorSpace
    ) -> [UInt8]? {
        guard width > 0, height > 0, width * height <= RasterImage.maxResizePixels,
              image.width > 0, image.height > 0
        else { return nil }
        let scale = min(
            CGFloat(width) / CGFloat(image.width), CGFloat(height) / CGFloat(image.height))
        let drawWidth = max(CGFloat(image.width) * scale, 1)
        let drawHeight = max(CGFloat(image.height) * scale, 1)
        let rect = CGRect(
            x: (CGFloat(width) - drawWidth) / 2, y: (CGFloat(height) - drawHeight) / 2,
            width: drawWidth, height: drawHeight)

        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        let drawn = pixels.withUnsafeMutableBufferPointer { buffer -> Bool in
            guard let context = CGContext(
                    data: buffer.baseAddress,
                    width: width,
                    height: height,
                    bitsPerComponent: 8,
                    bytesPerRow: width * 4,
                    space: space,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
            else { return false }
            // A bitmap context stores its first row at the TOP of what it
            // draws, which is the core's convention too — so, unlike the
            // flipped context the text renderer needs for glyph layout, this
            // one draws straight through.
            context.interpolationQuality = .high
            context.draw(image, in: rect)
            return true
        }
        guard drawn else { return nil }
        unpremultiply(&pixels)
        return pixels
    }

    /// Renders `draw` into a `width × height` straight-alpha RGBA8 buffer
    /// (row 0 = top): a premultiplied context in `space` flipped so y grows
    /// DOWN (CoreText and the shape paths draw in the canvas's own
    /// orientation), then the ONE unpremultiply. Every caller passes the
    /// document's DRAWING space (`RasterDocument.drawingSpace`), so an
    /// authored colour — a glyph's, a shape fill's — is converted into the
    /// document's numbers exactly once, here, by CoreGraphics. `draw`
    /// receives the context with the flip already applied and returns false
    /// to abort. `appKit: true`
    /// additionally pushes an NSGraphicsContext around `draw` — required by
    /// NSAttributedString drawing, and MAIN THREAD ONLY (AppKit drawing); the
    /// shape renderer leaves it false and stays pure
    /// CoreGraphics. nil for a degenerate size, one past the
    /// core's pixel cap, a context CoreGraphics refuses, or an aborted draw.
    static func renderStraightRGBA(
        width: Int, height: Int, appKit: Bool = false, space: CGColorSpace,
        _ draw: (CGContext) -> Bool
    ) -> [UInt8]? {
        guard width > 0, height > 0, width * height <= RasterImage.maxResizePixels else {
            return nil
        }
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        let drawn = pixels.withUnsafeMutableBufferPointer { buffer -> Bool in
            // CoreGraphics renders only into PREMULTIPLIED buffers; the
            // straight-alpha conversion happens below.
            guard let context = CGContext(
                    data: buffer.baseAddress,
                    width: width,
                    height: height,
                    bitsPerComponent: 8,
                    bytesPerRow: width * 4,
                    space: space,
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
            else { return false }
            return drawFlipped(context, height: height, appKit: appKit, draw)
        }
        guard drawn else { return nil }
        unpremultiply(&pixels)
        return pixels
    }

    /// Straight-alpha conversion of a premultiplied RGBA8 buffer in place:
    /// layer pixels cross the FFI with straight alpha (only the painting
    /// overlay is premultiplied). Fully transparent pixels carry no color at
    /// all.
    static func unpremultiply(_ pixels: inout [UInt8]) {
        for i in stride(from: 0, to: pixels.count, by: 4) {
            let alpha = Int(pixels[i + 3])
            if alpha == 255 { continue }
            if alpha == 0 {
                pixels[i] = 0
                pixels[i + 1] = 0
                pixels[i + 2] = 0
                continue
            }
            for channel in 0..<3 {
                let value = (Int(pixels[i + channel]) * 255 + alpha / 2) / alpha
                pixels[i + channel] = UInt8(min(value, 255))
            }
        }
    }

    /// Decodes a still image file through ImageIO — the platform decoders,
    /// which cover the formats the Rust core has none for (HEIC, HEIF) as
    /// well as the ones it does. Camera-tagged rotation is baked into the
    /// pixels, matching what the core does at open time, so callers never
    /// have to think about EXIF orientation.
    ///
    /// `maxSide` caps the longest side (0 = full size), which is what makes
    /// a preview cheap: ImageIO decodes to the requested size instead of
    /// producing a full-resolution image to throw away.
    static func decodeImage(_ url: URL, maxSide: Int = 0) -> CGImage? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              CGImageSourceGetCount(source) > 0
        else { return nil }
        // The transform-applying path is the thumbnail API even at full size;
        // "thumbnail" here only means "decoded to a bounding size".
        let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
        let pixelWidth = properties?[kCGImagePropertyPixelWidth] as? Int ?? 0
        let pixelHeight = properties?[kCGImagePropertyPixelHeight] as? Int ?? 0
        let fullSide = max(pixelWidth, pixelHeight, 1)
        let bound = maxSide > 0 ? min(maxSide, fullSide) : fullSide
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: bound,
        ]
        if let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) {
            return image
        }
        // A direct decode does not bake orientation. Only use it for an
        // already-upright source, so imageSize/imageDPI and normalized export
        // packets cannot describe a rotation the pixels never received.
        let orientation = properties?[kCGImagePropertyOrientation] as? Int ?? 1
        guard orientation == 1 else { return nil }
        return CGImageSourceCreateImageAtIndex(source, 0, nil)
    }

    /// The import-report sentence for the platform decode of the file at
    /// `url` when ImageIO reads it as Gray, CMYK or Lab — every such decode
    /// converts to sRGB as it reads. The core words it from
    /// the file's own bytes (`RasterDocument.platformConversionSentence`):
    /// the converted sentence when the core open would convert from the
    /// embedded profile itself, else a plain one. nil for an RGB file.
    static func platformConversion(at url: URL) -> String? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              let model = platformSource(of: source)
        else { return nil }
        // An unreadable file is worded as one with no profile, never silently.
        let bytes = (try? Data(contentsOf: url, options: .mappedIfSafe)) ?? Data()
        return RasterDocument.platformConversionSentence(encoded: bytes, source: model)
    }

    /// The same sentence for an image ImageIO decodes from `encoded` in
    /// memory — a pasteboard's own TIFF or PNG bytes.
    static func platformConversion(encoded: Data) -> String? {
        guard let source = CGImageSourceCreateWithData(encoded as CFData, nil),
              let model = platformSource(of: source)
        else { return nil }
        return RasterDocument.platformConversionSentence(encoded: encoded, source: model)
    }

    /// The sentence for a pasted image that came with none of its own
    /// encoded bytes (only a type AppKit synthesises): its decoded colour
    /// model with NO profile named, because the only bytes left to read one
    /// from would be AppKit's re-encode, which writes a generic profile the
    /// source never had. nil for an RGB image.
    static func platformConversion(unencoded image: CGImage) -> String? {
        let model: RasterPlatformSource
        switch image.colorSpace?.model {
        case .monochrome?: model = .gray
        case .cmyk?: model = .cmyk
        case .lab?: model = .lab
        default: return nil
        }
        return RasterDocument.platformConversionSentence(encoded: Data(), source: model)
    }

    /// True when ImageIO reads `encoded` in the same colour model as
    /// `image`, the image `NSImage(pasteboard:)` actually decoded — so the
    /// sentence `platformConversion(encoded:)` words from those bytes
    /// describes the conversion that happened. False when AppKit decoded
    /// another representation on the pasteboard (a file URL, a PDF) ahead
    /// of its TIFF/PNG.
    static func encodedModel(of encoded: Data, matches image: CGImage) -> Bool {
        guard let source = CGImageSourceCreateWithData(encoded as CFData, nil) else { return false }
        return sourceModel(of: source, matches: image)
    }

    /// `encodedModel(of:matches:)`'s twin for the file at `url` — a Finder
    /// file copy's URL on the pasteboard, which `NSImage(pasteboard:)`
    /// decodes when there is no TIFF/PNG ahead of it.
    static func fileModel(at url: URL, matches image: CGImage) -> Bool {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else { return false }
        return sourceModel(of: source, matches: image)
    }

    /// True when ImageIO reads `source` in the same colour model as `image`:
    /// the one comparison `encodedModel` and `fileModel` share.
    private static func sourceModel(of source: CGImageSource, matches image: CGImage) -> Bool {
        guard CGImageSourceGetCount(source) > 0,
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil)
                as? [CFString: Any],
              let model = properties[kCGImagePropertyColorModel] as? String
        else { return false }
        let models: [String: CGColorSpaceModel] = [
            kCGImagePropertyColorModelRGB as String: .rgb,
            kCGImagePropertyColorModelGray as String: .monochrome,
            kCGImagePropertyColorModelCMYK as String: .cmyk,
            kCGImagePropertyColorModelLab as String: .lab,
        ]
        return models[model] != nil && models[model] == image.colorSpace?.model
    }

    /// The colour model ImageIO reads `source` as, when it is one the
    /// platform decode converts to sRGB — asked of the SOURCE's properties,
    /// because `decodeImage`'s thumbnail decode has already rendered such a
    /// file into sRGB, so its `CGImage` says RGB. nil for RGB (and anything
    /// unreadable).
    fileprivate static func platformSource(of source: CGImageSource) -> RasterPlatformSource? {
        guard CGImageSourceGetCount(source) > 0,
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil)
                as? [CFString: Any],
              let model = properties[kCGImagePropertyColorModel] as? String
        else { return nil }
        let models: [String: RasterPlatformSource] = [
            kCGImagePropertyColorModelGray as String: .gray,
            kCGImagePropertyColorModelCMYK as String: .cmyk,
            kCGImagePropertyColorModelLab as String: .lab,
        ]
        return models[model]
    }

    /// The pixel size an image file DISPLAYS at, without decoding it: the
    /// stored dimensions, swapped when the EXIF orientation is one of the
    /// four quarter-turned ones (5-8) — which is the size `decodeImage`
    /// produces, since it bakes that rotation in. nil when ImageIO cannot
    /// read the file's properties.
    static func imageSize(_ url: URL) -> CGSize? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              CGImageSourceGetCount(source) > 0,
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil)
                as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int,
              width > 0, height > 0
        else { return nil }
        let turned = quarterTurned(properties)
        return CGSize(width: turned ? height : width, height: turned ? width : height)
    }

    /// True when the file's EXIF orientation is one of the four quarter
    /// turns (5-8), so the stored width is the DISPLAYED height. The ONE
    /// place that rule is written on this side: `decodeImage` bakes the
    /// rotation in, so both the size and the resolution below are read
    /// through it.
    private static func quarterTurned(_ properties: [CFString: Any]) -> Bool {
        (5...8).contains(properties[kCGImagePropertyOrientation] as? Int ?? 1)
    }

    /// The print resolution a still image file declares, in pixels per inch
    /// per axis, or nil when it declares none. ImageIO reports the same
    /// number for JFIF density, PNG `pHYs` and the EXIF resolution tags, so
    /// this is the platform decode path's answer to the core's own
    /// `metadata::scan`. Non-positive or non-finite values are treated as
    /// absent — a 0 dpi file states nothing.
    ///
    /// The pair is transposed for a quarter-turned file, exactly as
    /// `imageSize` transposes the dimensions and for the same reason: the
    /// file states its resolution for the picture as STORED, and
    /// `decodeImage` returns it upright. A 300 x 150 ppi frame stored
    /// sideways is a 150 x 300 ppi picture. The core's own open path applies
    /// the same rule (`dr_image::open_bytes`).
    /// `index` is the image read (a host still's primary image); the
    /// `declaredCaptureDPI` fallback walks only single-image JPEG/PNG/.rz.
    static func imageDPI(_ url: URL, index: Int = 0) -> (Double, Double)? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              CGImageSourceGetCount(source) > index,
              let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil)
                as? [CFString: Any],
              let x = properties[kCGImagePropertyDPIWidth] as? NSNumber,
              let y = properties[kCGImagePropertyDPIHeight] as? NSNumber
        else { return declaredCaptureDPI(url) }
        // NSNumber, not a Swift Double cast: ImageIO reports an integral
        // density as an integer CFNumber for most files.
        let (dx, dy) = (x.doubleValue, y.doubleValue)
        guard dx.isFinite, dy.isFinite, dx > 0, dy > 0 else { return declaredCaptureDPI(url) }
        return quarterTurned(properties) ? (dy, dx) : (dx, dy)
    }

    /// The frontmost pasteboard image as a CGImage, normalized through
    /// `NSBitmapImageRep` so the result has the source's real PIXEL
    /// dimensions (an NSImage's own size is in points) and carries the
    /// colour space its numbers belong to, plus `encoded`: the pasteboard's
    /// OWN `.tiff` bytes, or its `.png` when there is no TIFF, exactly as
    /// the source placed them — nil when it holds neither (a type AppKit
    /// synthesises). Never `tiffRepresentation`'s re-encode for that: AppKit
    /// writes a generic Gray/CMYK/Lab profile into it that the source never
    /// had. `file` is the pasteboard's first file URL — a Finder file
    /// copy — whose own bytes can word the sentence instead. nil when
    /// the pasteboard holds no image.
    ///
    /// This is the pasteboard's half of the ingest rule: it DECODES and
    /// converts nothing, leaving the caller to say which numbers it wants —
    /// `RasterImage.fromPasteboard` for a new document,
    /// `ExternalClipboardImage.read` for pixels joining an existing one.
    static func pasteboardImage(
        _ pasteboard: NSPasteboard
    ) -> (image: CGImage, encoded: Data?, file: URL?)? {
        guard let pasted = NSImage(pasteboard: pasteboard),
              let tiff = pasted.tiffRepresentation,
              let rep = NSBitmapImageRep(data: tiff),
              let image = rep.cgImage
        else { return nil }
        let encoded = pasteboard.data(forType: .tiff) ?? pasteboard.data(forType: .png)
        let file = (pasteboard.readObjects(
            forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL])?.first
        return (image, encoded, file)
    }

    /// The space to DECODE `image` into so that nothing is converted, plus
    /// the ICC bytes those numbers belong to — the pair a caller hands to
    /// `straightRGBA` and then assigns to the document, which is what makes
    /// "the platform converts nothing" true rather than hopeful.
    ///
    /// The image's own space qualifies only when it is RGB with three
    /// components, exports ICC bytes we could re-embed, and a 1×1 probe
    /// context can actually be built from it — an extended-range or
    /// otherwise unusual space is rejected by that probe rather than by
    /// guesswork, since a context CoreGraphics refuses would fail the whole
    /// decode. Otherwise the answer is sRGB with no profile: the decode then
    /// converts into sRGB, which is exactly what the numbers will be.
    ///
    /// A space that passes those tests must also carry ICC bytes a fresh
    /// document would STORE (`ingestRefusal`: a core-storable class and at
    /// most 16 MiB). One that does not is decoded into sRGB too, and
    /// `refusal` is the sentence saying so, for the caller's import report —
    /// labelling the refused profile's numbers sRGB would change the picture.
    static func ingestSpace(
        of image: CGImage
    ) -> (space: CGColorSpace, profile: Data?, refusal: String?) {
        ingestSpace(of: image.colorSpace)
    }

    /// The same question asked of a bare space, for a decode that has one
    /// without a `CGImage` to read it from — Core Image's RAW develop, which
    /// renders into `CIImage.colorSpace` (`RawImage.develop`).
    ///
    /// It is the ONE home for the rule, and the CGImage form above is this
    /// plus one property read. The two were written out twice, and the
    /// tightening that did land — refusing a space whose ICC bytes a fresh
    /// document will not store (`ingestRefusal`) — would otherwise have
    /// silently missed the RAW path, so the same photograph would have been
    /// labelled with a different profile depending on which decoder read it.
    static func ingestSpace(
        of space: CGColorSpace?
    ) -> (space: CGColorSpace, profile: Data?, refusal: String?) {
        guard let space = space, space.model == .rgb, space.numberOfComponents == 3,
              CGContext(
                data: nil, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) != nil,
              let icc = space.copyICCData() as Data?
        else { return (ColorProfile.sRGB, nil, nil) }
        if let refusal = ingestRefusal(icc) { return (ColorProfile.sRGB, nil, refusal) }
        return (space, icc, nil)
    }

    /// `image` converted to sRGB, or `image` itself when it already is one
    /// — the agent `render` exit's conversion, which lives here because
    /// this file is the documented CoreGraphics⇄core seam.
    ///
    /// The short-circuit is load-bearing, not an optimization: a render of
    /// an sRGB document must return the same bytes it always did, and a
    /// needless premultiply/unpremultiply round trip would perturb
    /// partially transparent pixels and zero the RGB under fully
    /// transparent ones. nil only when CoreGraphics refuses the conversion.
    static func sRGBCopy(of image: CGImage) -> CGImage? {
        if let space = image.colorSpace, CFEqual(space, ColorProfile.sRGB) { return image }
        let (w, h) = (image.width, image.height)
        guard let pixels = straightRGBA(
                from: image, fitting: w, height: h, space: ColorProfile.sRGB),
              let provider = CGDataProvider(data: Data(pixels) as CFData)
        else { return nil }
        return CGImage(
            width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
            space: ColorProfile.sRGB,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
            provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    }
}

extension RasterImage {
    /// A still image file decoded by the PLATFORM (see `Bitmap.decodeImage`)
    /// and handed to the core as pixels — the open path for formats the core
    /// itself cannot decode. nil when ImageIO cannot read the file.
    ///
    /// The decode goes into the SOURCE image's own space (`ingestSpace`), so
    /// it converts nothing; `profile` is the ICC bytes those pixel numbers
    /// belong to and the caller assigns them to the document, leaving the
    /// core's `adopt_working_space` to do the one conversion. The document
    /// separately attaches capture fields through `ImageIOMetadata`.
    ///
    /// `refusal` is non-nil when the file's embedded profile is one a
    /// document cannot carry: the decode then went into sRGB (`profile` is
    /// nil) and the sentence belongs in the open's import report. A Gray,
    /// CMYK or Lab image — which ColorSync converts to sRGB as it decodes —
    /// reports that conversion (`Bitmap.platformConversion(at:)`):
    /// with the core's own converted sentence when the profile is one
    /// the core would convert from too, else a plain one.
    static func decoded(
        from url: URL
    ) -> (image: RasterImage, profile: Data?, dpi: (Double, Double)?, refusal: String?)? {
        guard let image = Bitmap.decodeImage(url) else { return nil }
        let ingest = Bitmap.ingestSpace(of: image)
        guard let pixels = Bitmap.straightRGBA(
                from: image, fitting: image.width, height: image.height, space: ingest.space),
              let raster = RasterImage.from(
                rgba: pixels, width: image.width, height: image.height)
        else { return nil }
        let refusal = ingest.refusal ?? Bitmap.platformConversion(at: url)
        return (raster, ingest.profile, Bitmap.imageDPI(url), refusal)
    }

    /// The frontmost pasteboard image for a NEW document (File > New from
    /// Clipboard): decoded into its OWN space, so nothing is converted, plus
    /// the ICC bytes those numbers belong to. The caller assigns that
    /// profile and then lets `adoptWorkingSpace` do the single conversion —
    /// the same three steps `decoded(from:)` exists for, because a
    /// screenshot on a modern Mac is Display P3 and labelling those numbers
    /// sRGB is how the same picture came to look different depending on
    /// whether it was opened or pasted. `refusal` is `decoded(from:)`'s: the
    /// pasted profile could not be carried, so the image was read into sRGB,
    /// or a Gray, CMYK or Lab paste was converted to sRGB, worded
    /// from the pasteboard's own bytes (`Bitmap.pasteboardImage`), else from
    /// a Finder file copy's own file, else from the decoded model.
    static func fromPasteboard(
        _ pasteboard: NSPasteboard = Clipboard.pasteboard
    ) -> (image: RasterImage, profile: Data?, refusal: String?)? {
        guard let pasted = Bitmap.pasteboardImage(pasteboard) else { return nil }
        let ingest = Bitmap.ingestSpace(of: pasted.image)
        guard let raster = Self.pasted(pasted.image, in: ingest.space) else { return nil }
        // The pasteboard's own bytes word the sentence only when they are
        // the image AppKit decoded; then a file URL's bytes under the same
        // test (a Finder file copy); otherwise the decoded model.
        let encoded = pasted.encoded.flatMap {
            Bitmap.encodedModel(of: $0, matches: pasted.image) ? $0 : nil
        }
        let file = pasted.file.flatMap {
            Bitmap.fileModel(at: $0, matches: pasted.image) ? $0 : nil
        }
        let platform = encoded.map { Bitmap.platformConversion(encoded: $0) }
            ?? file.map { Bitmap.platformConversion(at: $0) }
            ?? Bitmap.platformConversion(unencoded: pasted.image)
        return (raster, ingest.profile, ingest.refusal ?? platform)
    }

    /// `image` decoded into `space` as straight RGBA8 at its own size — the
    /// pasteboard's two ingests (`fromPasteboard` for a new document,
    /// `ExternalClipboardImage.pixels` for pixels joining one) share it.
    static func pasted(_ image: CGImage, in space: CGColorSpace) -> RasterImage? {
        guard let pixels = Bitmap.straightRGBA(
                from: image, fitting: image.width, height: image.height, space: space)
        else { return nil }
        return RasterImage.from(rgba: pixels, width: image.width, height: image.height)
    }
}

extension ExternalClipboardImage {
    /// The frontmost pasteboard image for pixels joining an EXISTING
    /// document (Paste), decoded by the platform and not yet converted. The
    /// UNTAGGED rule: when the pasteboard's own bytes carry no embedded
    /// profile (`Bitmap.carriesProfile`), or the decoded image's space
    /// exports no ICC, AppKit's decode has labelled the numbers with a space
    /// the source never named, so the image is `untagged` — and
    /// `pixels(into:profile:)` reads its numbers as the DESTINATION's
    /// (measured against Photoshop, `paste_depth.py` case f: an untagged TIFF
    /// pasted into a Display P3 document kept its numbers). A tagged image's
    /// `sourceProfile` is `Bitmap.ingestSpace`'s profile, or the built-in
    /// sRGB when the ingest decodes into sRGB. nil when the pasteboard holds
    /// no image.
    ///
    /// #547 phase 4c: `cmyk` is the pasteboard's own TIFF read as a CMYK
    /// payload when it is one (`cmykPayload`, an untagged one taking
    /// `cmykFallback`, else the working CMYK), and `platformModel` names the
    /// colour model CoreGraphics converted from while decoding a Gray, Lab or
    /// CMYK image ("Gray", "Lab", "CMYK"; nil for RGB) — read off the
    /// pasteboard's own bytes when they are the image decoded, else the
    /// decoded image's space, as `RasterImage.fromPasteboard` words it.
    static func read(_ pasteboard: NSPasteboard,
                     cmykFallback: Data? = nil) -> ExternalClipboardImage? {
        guard let pasted = Bitmap.pasteboardImage(pasteboard) else { return nil }
        let image = pasted.image
        let untaggedBytes = pasted.encoded.map { !Bitmap.carriesProfile($0) } ?? false
        let untagged = untaggedBytes || image.colorSpace?.copyICCData() == nil
        let profile = Bitmap.ingestSpace(of: image).profile ?? RasterProfile.builtin(.sRGB)
        let names: [RasterPlatformSource: String] = [.gray: "Gray", .cmyk: "CMYK", .lab: "Lab"]
        let encoded = pasted.encoded.flatMap {
            Bitmap.encodedModel(of: $0, matches: image) ? $0 : nil
        }
        let source: RasterPlatformSource?
        if let encoded, let decoder = CGImageSourceCreateWithData(encoded as CFData, nil) {
            source = Bitmap.platformSource(of: decoder)
        } else {
            switch image.colorSpace?.model {
            case .monochrome?: source = .gray
            case .cmyk?: source = .cmyk
            case .lab?: source = .lab
            default: source = nil
            }
        }
        return ExternalClipboardImage(image: image, sourceProfile: profile,
                                      sourceName: RasterProfile.inspect(profile).name,
                                      untagged: untagged,
                                      cmyk: cmykPayload(pasteboard, fallback: cmykFallback),
                                      platformModel: source.flatMap { names[$0] })
    }

    /// The image's straight RGBA8 for `document`: `.convert` decodes it into
    /// the document's drawing space (CoreGraphics converts once — a layer's
    /// bytes have to be the document's own numbers); `.preserve` (Don't
    /// Convert) decodes it into its OWN space (`Bitmap.ingestSpace`), so its
    /// numbers are kept and the document's profile relabels them. An
    /// untagged image is re-tagged with the document's drawing space first
    /// (`CGImage.copy(colorSpace:)`), so either way its numbers are kept.
    /// nil when CoreGraphics cannot decode it.
    func pixels(into document: RasterDocument, profile: PasteProfile) -> RasterImage? {
        if untagged {
            let space = document.drawingSpace
            return RasterImage.pasted(image.copy(colorSpace: space) ?? image, in: space)
        }
        let space = profile == .preserve
            ? Bitmap.ingestSpace(of: image).space : document.drawingSpace
        return RasterImage.pasted(image, in: space)
    }
}

extension Bitmap {
    /// True when ImageIO finds an embedded ICC profile in `encoded` (the
    /// pasteboard's own TIFF/PNG bytes): `kCGImagePropertyProfileName` is
    /// present on its first image. The question `ExternalClipboardImage`'s
    /// untagged rule asks.
    static func carriesProfile(_ encoded: Data) -> Bool {
        guard let source = CGImageSourceCreateWithData(encoded as CFData, nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil)
                as? [CFString: Any]
        else { return false }
        return properties[kCGImagePropertyProfileName] != nil
    }
}

extension Bitmap {
    /// The largest ICC blob a document carries — the core's `profile_arg`
    /// cap (`MAX_DRDC_BLOB_LEN`), which `RasterProfile.inspect` does not ask.
    private static let maximumProfileBytes = 16 * 1024 * 1024

    /// nil when a fresh document would store `icc`: a core-storable class
    /// (`RasterProfileKind.isStorable`) no larger than 16 MiB — what
    /// `dr_doc_assign_profile` accepts on `RasterDocument.from(image:)`.
    /// Otherwise the sentence an import report gives for converting the
    /// decode from it to sRGB.
    static func ingestRefusal(_ icc: Data) -> String? {
        let info = RasterProfile.inspect(icc)
        if info.kind.isStorable && icc.count <= maximumProfileBytes { return nil }
        let named = info.name.isEmpty ? "" : " “\(info.name)”"
        let predicate = info.kind.refusalPredicate ?? ""
        let reason: String
        switch info.kind {
        case .notICC:
            return "The embedded colour profile " + predicate + " Doctored can read, "
                + "so the image was converted from it to sRGB as it was read."
        case .notRGB, .notImageProfile:
            reason = predicate
        case .rgbUnconvertible, .rgbMatrix, .rgbLut:
            reason = "is \(icc.count) bytes, over the 16 MiB a Doctored document can carry"
        }
        return "The embedded colour profile\(named) \(reason), "
            + "so the image was converted from it to sRGB as it was read."
    }
}
