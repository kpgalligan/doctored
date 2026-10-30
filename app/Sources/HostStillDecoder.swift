import Foundation
import CoreGraphics
import ImageIO

// Never use a thumbnail/byte decode to replace an unsupported or degraded preserving source.
//
// Actual sample layout, alpha and ICC are authoritative; requested buffer depth or container depth
// alone is not evidence. The ONE sanctioned re-decode is the gain-map SDR base: an 8-bit container
// whose HDR decode is HDR only because of a gain map re-decodes with `kCGImageSourceDecodeToSDR`,
// records `decode_request: "sdr_base"` and discloses it; PQ/HLG and every other HDR decode still
// refuse.
//
// Still admission is the measured per-depth affine figure (`HostStillDecoder.scratchFixed8` +
// `scratchPerPixel8`·px and the `Wide` pair, through `HostSourceInput.scratch`), whose measurement
// record sits beside the constants; a single 8-bit still's whole OPEN, first projection included,
// is charged up front by `HostStillDecoder.openScratch` (`openFixed8` + `openPerPixel8`·px +
// 3·encoded + 64 MiB, admitted in `openHostSource` before the capture), its record beside those
// constants.

enum HostStillDecoder {
    /// The refusal when ImageIO hands back an image with no pixel provider at
    /// all — distinct from a provider whose size disagrees with the layout.
    static let noDecodedPixels = "macOS's image decoder returned no pixels for this image, so it cannot be opened on this system. A known case is a 12-bit 4:4:4 HEIC."

    /// The refusal for an AVIF whose primary item declares premultiplied
    /// alpha (an `iref` `prem` reference) and that cannot be decoded
    /// with it. macOS 26.5.1's ImageIO decodes every such file measured
    /// (8/10/12-bit, lossless matrix 0 and lossy matrix 6, `avifenc
    /// --premultiply`) with NO alpha (RGBX8 / RGB101010 / RGBX16), its RGB
    /// still premultiplied, while the straight twin decodes with its alpha
    /// exact against `avifdec`. So `decode` reads a copy whose `prem`
    /// reference no longer names the primary item
    /// (`HeifContainer.detachingPremultipliedReference`) and declares the
    /// samples premultiplied itself; this sentence is what is left for a file
    /// whose reference cannot be detached or whose alpha is still dropped.
    static let premultipliedRefusal = "This AVIF stores premultiplied alpha, which macOS's image decoder drops, so it cannot be opened with its transparency on this system."

    struct Probe: Equatable {
        let format: String
        let width: Int
        let height: Int
        let depth: Int
        let orientation: Int
        /// The container's primary image: a multi-image file (a light/dark
        /// dynamic wallpaper) opens its primary image.
        let index: Int
    }
    static func inspect(_ url: URL) throws -> Probe {
        guard let source = CGImageSourceCreateWithURL(url as CFURL,
            [kCGImageSourceShouldCache: false] as CFDictionary),
            let format = CGImageSourceGetType(source) as String? else {
            throw HostSourceError.failure("macOS's image decoder cannot read this file as an image.")
        }
        guard ["public.jpeg-xl", "public.heic", "public.heif", "public.avif", "public.jpeg-2000",
               "public.jpeg", "public.png", "public.tiff"].contains(format) else {
            throw HostSourceError.failure("This image format (\(format)) has no host decoder; JPEG XL, HEIC/HEIF, AVIF, JPEG 2000, JPEG, PNG and TIFF do.")
        }
        let count = CGImageSourceGetCount(source), index = CGImageSourceGetPrimaryImageIndex(source)
        guard count > 0, index < count else {
            throw HostSourceError.failure("This file contains no image to open.")
        }
        guard let info = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any],
            let width = (info[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue,
            let height = (info[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue,
            let depth = (info[kCGImagePropertyDepth] as? NSNumber)?.intValue else {
            throw HostSourceError.failure("macOS's image decoder did not report this image's dimensions and bit depth.")
        }
        guard width > 0, height > 0, width <= RasterImage.maxResizePixels / height else {
            throw HostSourceError.failure("This image is \(width)×\(height), outside the document size limit.")
        }
        guard (1...16).contains(depth) else {
            throw HostSourceError.failure("This image stores \(depth)-bit samples; a document keeps at most 16 bits per channel.")
        }
        // A still one image could not keep at its depth refuses here,
        // before any admission, capture or decode.
        try HostImageCapacity.require(width: width, height: height, depth: depth <= 8 ? 8 : 16)
        let orientation = (info[kCGImagePropertyOrientation] as? NSNumber)?.intValue ?? 1
        guard (1...8).contains(orientation) else { throw HostSourceError.failure("The image orientation is invalid.") }
        return Probe(format: format, width: width, height: height, depth: depth, orientation: orientation, index: index)
    }

    static func decode(_ url: URL, filename: String, maximumScratch: UInt64,
                       control: RasterJobControl? = nil) throws -> (RasterDocument, HostMediaContract) {
        guard control?.isCancelled != true else { throw JobCancelled() }
        let probe = try inspect(url)
        let need = scratchFixed(depth: probe.depth) + UInt64(probe.width * probe.height) * scratchPerPixel(depth: probe.depth)
            + 32 * 1024 * 1024
        guard need <= maximumScratch else {
            func memory(_ bytes: UInt64) -> String {
                ByteCountFormatter.string(fromByteCount: Int64(min(bytes, UInt64(Int64.max))), countStyle: .memory)
            }
            throw DoctoredCoreError(message: "Decoding this \(probe.width)×\(probe.height) image needs \(memory(need)) "
                + "of working memory, more than the \(memory(maximumScratch)) this job was admitted. "
                + RasterResources.smallerJobRemedy)
        }
        // ImageIO drops the alpha of a premultiplied AVIF, so such a
        // file is decoded from a copy whose `prem` reference is detached and
        // its samples are declared premultiplied below. A read failure or a
        // parse refusal keeps today's path.
        var premultiplied: HeifPrimaryColorFacts?
        var opened: CGImageSource?
        if probe.format == "public.avif",
           let data = try? Data(contentsOf: url, options: .alwaysMapped),
           let facts = try? HeifContainer.primaryColorFacts(data), facts.premultipliedAlpha {
            guard let detached = try? HeifContainer.detachingPremultipliedReference(data),
                  let after = try? HeifContainer.primaryColorFacts(detached), !after.premultipliedAlpha,
                  let patched = CGImageSourceCreateWithData(detached as CFData,
                      [kCGImageSourceShouldCache: false] as CFDictionary) else {
                throw HostSourceError.failure(premultipliedRefusal)
            }
            premultiplied = facts
            opened = patched
        } else {
            opened = CGImageSourceCreateWithURL(url as CFURL,
                [kCGImageSourceShouldCache: false] as CFDictionary)
        }
        guard let source = opened else {
            throw HostSourceError.failure(HostSourceError.unsupported)
        }
        var decoded = decodeImage(source, index: probe.index, request: kCGImageSourceDecodeToHDR)
        var decodeRequest: String?
        // An 8-bit container whose HDR decode comes back HDR only because of a
        // gain map (every iPhone 12+ photo) opens as its SDR base: the gain map
        // is not applied, the choice is recorded and disclosed, and the file is
        // retained. It depends only on the file bytes and the OS decoder, so a
        // re-decode reaches the same contract. Any other HDR decode (PQ/HLG, no
        // gain map, a deeper container) refuses below and is never tone-mapped.
        let hdr = decoded.map { image in
            image.bitmapInfo.contains(.floatComponents)
                || image.colorSpace.map { $0.isHDR() || CGColorSpaceUsesExtendedRange($0) } ?? false
        } ?? false
        if hdr, probe.depth <= 8, hasGainMap(source, index: probe.index) {
            decoded = nil
            decoded = decodeImage(source, index: probe.index, request: kCGImageSourceDecodeToSDR)
            decodeRequest = HostMediaContract.sdrBase
        }
        guard let image = decoded else {
            throw HostSourceError.failure("macOS's image decoder could not decode this image.")
        }
        guard image.width == probe.width, image.height == probe.height else {
            throw HostSourceError.failure("The decoded image is \(image.width)×\(image.height), not the \(probe.width)×\(probe.height) its file reports.")
        }
        // These HDR checks must stay ahead of `layout`: an HDR decode is
        // also packed RGB101010 and would otherwise match that arm.
        guard !image.bitmapInfo.contains(.floatComponents),
              !(image.colorSpace.map { $0.isHDR() || CGColorSpaceUsesExtendedRange($0) } ?? false) else {
            throw HostSourceError.failure("This image decodes as HDR, and there is no HDR policy to keep it "
                + "without tone-mapping yet (only an 8-bit file's gain-map SDR base opens).")
        }
        let bpc = image.bitsPerComponent
        guard bpc == (probe.depth <= 8 ? 8 : 16) || (bpc == 10 && (9...10).contains(probe.depth)) else {
            throw HostSourceError.failure("macOS's image decoder returned \(bpc)-bit samples for this \(probe.depth)-bit image, so its precision cannot be kept.")
        }
        guard let space = image.colorSpace, space.model == .rgb else {
            throw HostSourceError.failure("This image does not decode to an RGB colour space (it may be grayscale or CMYK), and there is no preserving decoder for its colour model yet.")
        }
        guard let profile = space.copyICCData() as Data?, !profile.isEmpty else {
            throw HostSourceError.failure("The decoded image carries no ICC profile, so its colours cannot be recorded.")
        }
        var layout = try layout(image)
        // ImageIO decodes a 12-bit AVIF whose primary item declares the
        // identity matrix (nclx matrix 0), without premultiplied alpha, with
        // its RGB left-aligned in 16 bits (white 65520). Only the container
        // says so, so such a file takes the `_left12` twin of its token and
        // the core widens RGB to full scale. Anything else, a read failure or
        // a parse refusal included, keeps today's token.
        let token = try premultiplied.map { try premultipliedToken(layout.token, facts: $0, probe: probe) }
            ?? leftAligned12(layout.token, url: url, probe: probe)
        if token != layout.token, let entry = HostDecodedContract.layouts[token] {
            layout = (token, entry.depth, entry.alpha)
        }
        let (bytes, overflow) = image.bytesPerRow.multipliedReportingOverflow(by: image.height)
        guard !overflow, image.bytesPerRow > 0, bytes > 0, UInt64(bytes) <= maximumScratch / 2,
              UInt64(bytes) * 2 + UInt64(probe.width * probe.height) * UInt64(layout.depth / 2) + 32 * 1024 * 1024 <= maximumScratch else {
            throw HostSourceError.failure("The full image buffer exceeds its admitted layout.")
        }
        let lease = try PrecisionEditPayload.admitted(bytes: UInt64(bytes) * 2 + UInt64(profile.count) + 65_536)
        defer { withExtendedLifetime(lease) {} }
        guard let captured = image.dataProvider?.data else {
            throw HostSourceError.failure(noDecodedPixels)
        }
        guard CFDataGetLength(captured) == bytes else {
            throw HostSourceError.failure("The full image provider disagrees with its declared buffer size.")
        }
        let document = try RasterDocument.fromHostPixels(bytes: captured as Data,
            width: image.width, height: image.height, rowBytes: image.bytesPerRow,
            layout: layout.token, profile: profile, orientation: probe.orientation, control: control)
        let contract = HostDecodedContract(width: document.width, height: document.height,
            storage_depth: layout.depth, layout: layout.token, alpha: layout.alpha,
            profile_sha256: HostSourceError.sha256(profile), profile_name: space.name as String? ?? "Decoded ICC")
        let signature = "\(probe.format):\(probe.width):\(probe.height):\(probe.depth):\(probe.orientation)"
        let media = HostMediaContract(resource_id: "still", filename: filename, format: probe.format,
            decoder: .current("apple-imageio"), encoded_depth: probe.depth,
            encoded_precision_note: "Encoded depth is an ImageIO container report; retained precision is the actual decoded integer layout.",
            width: probe.width, height: probe.height, orientation: probe.orientation,
            format_sha256: HostSourceError.sha256(Data(signature.utf8)), decoded: contract,
            decode_request: decodeRequest)
        return (document, media)
    }

    /// The full decode with today's options; only the decode request varies.
    private static func decodeImage(_ source: CGImageSource, index: Int, request: CFString) -> CGImage? {
        CGImageSourceCreateImageAtIndex(source, index,
            [kCGImageSourceShouldCacheImmediately: true, kCGImageSourceShouldAllowFloat: true,
             kCGImageSourceDecodeRequest: request,
             kCGImageSourceGenerateImageSpecificLumaScaling: false] as CFDictionary)
    }

    /// The `_left12` twin of `rgba16_le` / `rgba16_le_opaque` for a 12-bit
    /// AVIF whose container states nclx matrix 0 and no `prem` reference;
    /// `token` unchanged otherwise.
    private static func leftAligned12(_ token: String, url: URL, probe: Probe) -> String {
        guard probe.format == "public.avif", probe.depth == 12,
              ["rgba16_le", "rgba16_le_opaque"].contains(token),
              let data = try? Data(contentsOf: url, options: .alwaysMapped),
              let facts = try? HeifContainer.primaryColorFacts(data),
              facts.matrixCoefficients == 0, !facts.premultipliedAlpha else { return token }
        return token + "_left12"
    }

    /// The premultiplied twin of the straight token a detached-`prem` AVIF
    /// decodes to: the stored RGB is still premultiplied, so the core
    /// divides it. A 12-bit identity-matrix file comes back left-aligned like
    /// its straight twin and takes the `_left12` form. Any other token
    /// means the alpha was dropped after all, which refuses by name.
    private static func premultipliedToken(_ token: String, facts: HeifPrimaryColorFacts,
                                           probe: Probe) throws -> String {
        switch token {
        case "rgba8": return "rgba8_premultiplied"
        case "rgba16_le":
            return probe.depth == 12 && facts.matrixCoefficients == 0
                ? "rgba16_le_premultiplied_left12" : "rgba16_le_premultiplied"
        default: throw HostSourceError.failure(premultipliedRefusal)
        }
    }

    private static func hasGainMap(_ source: CGImageSource, index: Int) -> Bool {
        [kCGImageAuxiliaryDataTypeHDRGainMap, kCGImageAuxiliaryDataTypeISOGainMap].contains { type in
            CGImageSourceCopyAuxiliaryDataInfoAtIndex(source, index, type) != nil
        }
    }

    /// The declared layout of a decoded image; `HDRStillDecoder` reads a PQ/HLG decode through it.
    static func layout(_ image: CGImage) throws -> (token: String, depth: Int, alpha: String) {
        let order = image.bitmapInfo.rawValue & CGBitmapInfo.byteOrderMask.rawValue
        let packed = image.pixelFormatInfo == .packed
        var token: String?
        if packed, image.bitsPerComponent == 16, order == CGBitmapInfo.byteOrder16Little.rawValue {
            if image.bitsPerPixel == 64, image.alphaInfo == .last { token = "rgba16_le" }
            if image.bitsPerPixel == 64, image.alphaInfo == .premultipliedLast { token = "rgba16_le_premultiplied" }
            if image.bitsPerPixel == 64, image.alphaInfo == .noneSkipLast { token = "rgba16_le_opaque" }
            if image.bitsPerPixel == 48, image.alphaInfo == .none { token = "rgb16_le" }
        }
        if packed, image.bitsPerComponent == 8 {
            if image.bitsPerPixel == 24, image.alphaInfo == .none { token = "rgb8" }
            if image.bitsPerPixel == 32, order == 0 || order == CGBitmapInfo.byteOrder32Big.rawValue {
                if image.alphaInfo == .last { token = "rgba8" }
                if image.alphaInfo == .premultipliedLast { token = "rgba8_premultiplied" }
                if image.alphaInfo == .noneSkipLast { token = "rgba8_opaque" }
            }
            if image.bitsPerPixel == 32, order == CGBitmapInfo.byteOrder32Little.rawValue {
                if image.alphaInfo == .first { token = "bgra8" }
                if image.alphaInfo == .noneSkipFirst { token = "bgra8_opaque" }
            }
        }
        if image.bitsPerComponent == 10, image.bitsPerPixel == 32,
           order == CGBitmapInfo.byteOrder32Little.rawValue, image.alphaInfo == .noneSkipFirst,
           image.pixelFormatInfo == .RGB101010 {
            token = "rgb10_packed_le"
        }
        guard let token, let entry = HostDecodedContract.layouts[token] else {
            throw HostSourceError.failure("This full image decoder returned an unsupported sample layout; no byte conversion was substituted.")
        }
        return (token, entry.depth, entry.alpha)
    }
}

extension HostStillDecoder {
    /// Host-still admission, measured, not guessed. A still's scratch is
    /// affine per depth class: `F + a·w·h + 3·encoded + 64 MiB`
    /// (`HostSourceInput.scratch`). Every case of the class must satisfy
    /// `F + a·px + 3·enc + 64 MiB ≥ ⌈1.25 × max over runs of max(E_cap,
    /// E_init)⌉` at 12, 48 and 96 MP, and so must each 8-bit kind's 48→96 MP
    /// requirement line continued to the 100 MP document limit. The wide class
    /// is held to its whole open's peak instead (below). The guarantee
    /// is "never under-admits a measured case or its continued line": the
    /// 8-bit cases are synthetic gradient-plus-noise fixtures, the system iMac
    /// Blue and Sonoma HEICs (36 MP) and real system desktop pictures re-encoded to 8-bit
    /// HEIC at 36, 48 and 96 MP (one with a gain map); no camera original
    /// was measured. `F + a·px` must reach the layout
    /// gate's floor·px, and among such pairs the one with the smallest 48 MP
    /// charge is chosen (ties: the larger `a`); `F` is rounded up to a MiB.
    ///
    /// Method: as in `core/src/precision_png.rs`. A -O WMO build of the app
    /// sources with a harness `main`, release core; one case per process with
    /// `MallocLargeCache=0`, 3 runs plus a baseline twin; macOS 26.5.1. After
    /// setup (`HostSourceInput(url:)`; `inspect` for an open),
    /// `proc_reset_footprint_interval`, pre = `ri_phys_footprint`, the call, E =
    /// `ri_interval_max_phys_footprint` − pre (`RUSAGE_INFO_V4`). E_cap is
    /// `HostSourceWork.capture`; E_init is `ImageDocument.openHostSource`'s E
    /// (`.preserve`; wide also `.rgba8`) − (w·h·master B/px (4, wide 8) +
    /// encoded). Calibration, all four sessions: 1 GiB touched and dropped
    /// returns within 65,536 B; 256 MiB reads E 268,566,720, lifetime peak −
    /// pre 1,074,283,264. Every
    /// baseline twin reads E 0, and `/usr/bin/time -l` peak − pre equals E in
    /// every run. Fixtures: `scripts/host-still-admission/author.swift`
    /// (+ `avifenc -s 8 -y 420 [-d 10]`), the system iMac Blue HEIC, and
    /// re-encodes of `/System/Library/Desktop Pictures/iMac Blue.heic` (sRGB)
    /// and `Sonoma.heic` (Display P3): scaled to cover and centre-cropped in
    /// the source's space, RGBX8, `CGImageDestination` HEIC quality 0.9, the
    /// gain-map variant through author.swift's HDR HEIF construction. The
    /// Sonoma original (a two-image light/dark container, which opens its
    /// primary image) was measured in a fourth session, against a harness built
    /// from the primary-image tree.
    ///
    /// Per case (dimensions, encoded bytes): max E, its run's pre.
    ///   heic8-12 4032×3024, 6,859,319 B: E_cap 268,976,392 (pre 6,324,800),
    ///     E_open 312,262,944 (pre 6,242,880) → E_init 256,632,553
    ///   heic8-48 8064×6048, 27,396,271 B: E_cap 851,886,800 (pre 6,373,952),
    ///     E_open 1,021,739,896 (pre 6,242,856) → E_init 799,259,337
    ///   heic8-gainmap-12 4032×3024, 6,990,735 B: E_cap 372,015,512 (pre
    ///     6,472,256), E_open 328,450,408 (pre 6,308,416) → E_init 272,688,601
    ///   heic8-gainmap-48 8064×6048, 26,035,768 B: E_cap 861,651,688 (pre
    ///     6,423,080), E_open 1,025,999,688 (pre 6,308,416) → E_init 804,879,632
    ///   jpeg8-12 4032×3024, 5,277,243 B: E_cap 212,697,400 (pre 6,259,264),
    ///     E_open 257,605,920 (pre 6,046,272) → E_init 203,557,605
    ///   jpeg8-48 8064×6048, 21,065,959 B: E_cap 830,767,992 (pre 6,226,472),
    ///     E_open 1,006,633,848 (pre 6,095,424) → E_init 790,483,601
    ///   avif8-12 4032×3024, 412,557 B: E_cap 271,696,184 (pre 6,292,008),
    ///     E_open 339,968,312 (pre 6,128,192) → E_init 290,784,683
    ///   avif8-48 8064×6048, 1,615,685 B: E_cap 790,397,576 (pre 6,259,264),
    ///     E_open 985,744,152 (pre 6,177,344) → E_init 789,044,179
    ///   heic8-96 12000×8000, 53,925,311 B: E_cap 1,663,108,392 (pre 6,406,720)
    ///   heic8-gainmap-96 12000×8000, 51,908,313 B: E_cap 1,665,697,616 (pre
    ///     6,603,328)
    ///   jpeg8-96 12000×8000, 41,446,748 B: E_cap 1,628,767,792 (pre 6,160,960)
    ///   avif8-96 12000×8000, 3,156,195 B: E_cap 1,552,991,432 (pre 6,226,496)
    ///     (E_cap only: when this was measured, the core refused every 96 MP
    ///     8-bit open at the limit then in force (2 GiB), e.g. heic8-96
    ///     "precision operation needs 2362297063 bytes including retained
    ///     inputs". Now the whole open is charged up front by
    ///     `openScratch` below, which admits it at 8 GiB.)
    ///   iMac Blue 6016×6016, 26,284,797 B: E_cap 790,627,000 (pre 6,488,640),
    ///     E_open 910,836,480 (pre 6,341,184) → E_init 739,782,659
    ///   Sonoma 6016×6016, 20,362,274 B: E_cap 779,535,056 (pre 6,586,920),
    ///     E_open 905,610,032 (pre 6,472,256) → E_init 740,478,734
    ///   imacblue-36 (re-encoded) 6016×6016, 15,588,647 B: E_cap 768,787,056
    ///     (pre 6,308,416), E_open 899,547,856 (pre 6,177,344) → E_init
    ///     739,190,185
    ///   imacblue-48 8064×6048, 15,166,350 B: E_cap 827,785,912 (pre
    ///     6,373,952), E_open 1,009,435,416 (pre 6,226,496) → E_init 799,184,778
    ///   imacblue-96 12000×8000, 19,467,988 B: E_cap 1,594,770,632 (pre
    ///     6,406,720)
    ///   sonoma-48 8064×6048, 6,676,180 B: E_cap 811,041,464 (pre 6,373,952),
    ///     E_open 1,001,505,584 (pre 6,242,880) → E_init 799,745,116
    ///   sonoma-96 12000×8000, 11,320,484 B: E_cap 1,578,501,344 (pre
    ///     6,406,720)
    ///   sonoma-gainmap-48 8064×6048, 3,489,848 B: E_cap 816,317,112 (pre
    ///     6,521,408), E_open 1,004,012,456 (pre 6,292,008) → E_init 805,438,320
    ///   sonoma-gainmap-96 12000×8000, 7,576,355 B: E_cap 1,577,699,056 (pre
    ///     6,570,560)
    ///     (the re-encoded 96 MP opens are refused by the core too, e.g.
    ///     imacblue-96: "precision operation needs 2327839740 bytes …")
    ///   heic10-12 4032×3024, 9,278,261 B: E_cap 372,752,744 (pre 6,423,104),
    ///     E_open 372,834,664 → E_init 266,014,259; rgba8 E_init 266,128,923
    ///   heic10-48 8064×6048, 33,987,280 B: E_cap 1,259,389,936 (pre
    ///     6,341,184); open refused by the core at 16 bits (below)
    ///   avif10-12 4032×3024, 330,973 B: E_cap 350,650,680 (pre 6,324,800),
    ///     E_open 350,716,288 → E_init 252,843,171; rgba8 E_init 253,793,371
    ///   avif10-48 8064×6048, 1,282,869 B: E_cap 1,181,483,968 (pre
    ///     6,292,008); open refused by the core (below)
    /// Three-run E_cap at 48 MP: heic8 851,772,112 / 851,886,800 / 851,837,648;
    /// gain map 861,651,688 / 855,426,056 / 855,442,464; jpeg8 830,620,512 /
    /// 830,505,824 / 830,767,992; avif8 790,119,048 / 790,135,432 / 790,397,576;
    /// heic10 1,259,258,864 / 1,259,062,256 / 1,259,389,936; avif10
    /// 1,181,418,408 / 1,181,369,232 / 1,181,483,968. At 96 MP: heic8
    /// 1,662,551,336 / 1,663,108,392 / 1,663,059,240; gain map 1,665,582,928 /
    /// 1,665,697,616 / 1,662,469,968; jpeg8 1,628,767,792 / 1,628,603,952 /
    /// 1,628,522,056; avif8 1,552,778,440 / 1,552,925,872 / 1,552,991,432.
    /// Re-encoded: imacblue-36 768,475,808 / 768,787,056 / 768,606,856; -48
    /// 827,605,688 / 827,785,912 / 827,753,120; -96 1,594,770,632 /
    /// 1,594,655,944 / 1,594,508,512; sonoma-48 810,779,296 / 811,041,464 /
    /// 810,943,160; -96 1,578,501,344 / 1,578,337,480 / 1,578,501,320;
    /// sonoma-gainmap-48 810,484,744 / 816,317,112 / 810,697,760; -96
    /// 1,574,520,632 / 1,577,633,520 / 1,577,699,056. The Sonoma original:
    /// 779,535,056 / 779,485,856 / 779,420,320. The native-size
    /// re-encode of iMac Blue peaks 2.8 % below the original file.
    /// (heic10-96 and avif10-96 were refused here at 2 GiB; at 8 GiB the core
    /// refuses them after the capture, the Wide record below.)
    ///
    /// A pure per-pixel figure is the wrong shape: the 12 MP cases carry a
    /// fixed overhead (heic8-gainmap-12's third capture read 372,015,512
    /// against 285,720,936 and 285,589,864), so the per-pixel rule needed
    /// 31 B/px and over-charged 48 MP stills.
    ///
    /// 8-bit (heic8, gain map, jpeg8, avif8 at 12/48/96 MP, iMac Blue,
    /// Sonoma, and the re-encodes): F = 187 MiB (196,083,712; exact 195,648,407), a = 18;
    /// 48 MP charge 1,073,963,008 (22.0 B/px). The requirement's 48→96 MP
    /// slope is 19.6–20.1 B/px. Each 100 MP point is that kind's requirement
    /// line continued, so its `3·enc` credit assumes the encoded size continued
    /// along the same 48→96 MP line: heic8 1,929,142,009 (encoded 56,172,158),
    /// gain map 1,937,836,668 (54,099,559), jpeg8 1,923,814,349 (43,172,876),
    /// avif8 1,945,004,194 (3,286,667), imacblue 1,948,056,112 (19,832,310),
    /// sonoma 1,952,125,253 (11,713,828), sonoma gain map 1,961,853,051
    /// (7,922,457). A 100 MP file encoded much smaller than its kind's line
    /// gets less credit; the small-encoded re-encodes cover that case.
    /// Margins (have − need): heic8-12 167,019,867; -48 158,402,185; -96
    /// 74,083,019; gain map -12 38,615,215; -48 142,114,566; -96 64,795,495;
    /// jpeg8-12 232,622,379; -48 165,809,759; -96 79,573,080; avif8-12
    /// 120,419,217; -48 157,921,957; -96 59,421,871; iMac Blue 5,223,825;
    /// Sonoma 1,321,186 (charged 975,740,006, 1.2517× its capture peak);
    /// imacblue-36 435,305 (binding: less encoded credit than the original at
    /// the same peak); -48 151,838,532; -96 56,133,250; sonoma-48
    /// 147,298,582; -96 52,027,348; sonoma gain map -48 131,145,026; -96
    /// 41,797,821; 100 MP: heic8 66,941,703, gain map 58,247,044, jpeg8
    /// 72,269,363, avif8 51,079,518, imacblue 48,027,600, sonoma 43,958,459,
    /// sonoma gain map 34,230,661. Real content at 48 and 96 MP peaks below
    /// the synthetic HEIC fixtures at the same size (E_cap 16.4–17.0 B/px
    /// against 17.3–17.7).
    /// Wide (heic10, avif10), re-measured on the open that lands
    /// ONE raster layer (since 2026-10-10 no file or decoded master stays in the
    /// document; the earlier table was taken when a wide still kept a 16-bit master
    /// beside the canvas raster): the WHOLE open, measured on the Release app
    /// (`verify.py --measure 3`): `DR_WORKING_LIMIT_BYTES` 8 GiB,
    /// `MallocLargeCache=0`, macOS 26.5.1, a fresh instance per case, MCP
    /// `open_document` (`precision: preserve`), `proc_reset_footprint_interval`,
    /// E = `ri_interval_max_phys_footprint` − pre, read once the footprint has
    /// been still for 2 s; 3 runs, retained before the open 0, baseline twin E
    /// 16,384 / 0 / 16,384. Rule: `F + a·px + 3·enc + 64 MiB ≥ ⌈1.25 × max E⌉`
    /// for every case (the 100 MP fixture is exactly 100,000,000 px, the
    /// document limit); `a ≥ 24`, the rgba16 layout floor below; smallest 48 MP
    /// charge (ties: the larger `a`), `F` the least whole number of MiB. E per
    /// run (encoded bytes):
    ///   heic10-12 4032×3024 (9,278,261) 392,495,440 / 393,036,112 / 392,233,320
    ///   heic10-48 8064×6048 (33,987,280) 1,562,641,608 / 1,563,051,184 /
    ///     1,563,100,336
    ///   heic10-67 9456×7096 (46,761,977) 2,149,893,728 / 2,149,975,624 /
    ///     2,149,926,472
    ///   heic10-96 12000×8000 (66,895,279) 3,075,115,288 / 3,075,426,584 /
    ///     3,075,361,000
    ///   heic10-100 12500×8000 (69,695,517) 3,203,729,736 / 3,203,696,968 /
    ///     3,203,582,280
    ///   avif10-12 (330,973) 391,266,592 / 391,397,688 / 429,130,016
    ///   avif10-48 (1,282,869) 1,559,626,904 / 1,559,676,032 / 1,708,508,288
    ///   avif10-67 (1,754,936) 2,146,879,000 / 2,147,042,840 / 2,349,860,400
    ///   avif10-96 (2,496,883) 3,077,376,184 / 3,077,114,040 / 3,077,081,272
    ///   avif10-100 (2,627,702) 3,206,023,424 / 3,204,548,888 / 3,204,942,104
    /// (32.0–32.2 B/px, and 35.0–35.2 on avif10's third run at 12, 48 and
    /// 67 MP: the decoded image, the one 16-bit raster layer it lands as and
    /// the first projection. After the open the instance holds post − pre
    /// 2,024,457,800, 20.2 B/px, at heic10-100. Run for run the peaks are
    /// 0.05–2.2 % below the earlier table's (heic10-100 read 3,274,721,704 then); avif10's
    /// occasional high run, about 9.5 % above its others, fell on -67 and -96
    /// then.) One 16-bit
    /// image holds the document's 100 MP (`single_image_capacity`: a 1 GiB
    /// `MAX_DRDC_PNG_LEN`, no decoder cap on a 16-bit plane), so the 96 and
    /// 100 MP stills open. Result: F = 45 MiB (47,185,920; exact 46,761,236),
    /// a = 42; 48 MP charge 2,095,570,944 (43.0 B/px). Charge / P/E: heic10-12
    /// 654,225,823 / 1.665; -48 2,264,641,648 / 1.449; -67 3,072,771,307 /
    /// 1.429; -96 4,346,980,621 / 1.413; -100 4,523,381,335 / 1.412; avif10-12
    /// 627,383,959 / 1.462; -48 2,166,528,415 / 1.268; -67 2,937,750,184 /
    /// 1.250; -96 4,153,785,433 / 1.350; -100 4,322,177,890 / 1.348. Binding:
    /// avif10-67 (424,684 to spare), whose third run read 203 MB above its
    /// other two (avif10-48's third run 149 MB above, avif10-12's 38 MB); the
    /// rule charges the largest run. The pair before this refit, 10 MiB and
    /// 43, holds these rows too (1.263× of avif10-67) and
    /// charges 12,070,912 more at 48 MP; `a = 43` needs no `F` here and
    /// charges 1,585,152 more than the pair chosen. The measured build
    /// differs from this one in the wide pair and comments only.
    /// png16 (the core's own precision open, not this charge) is admitted by
    /// `precision_codec::decode_with_context`'s one up-front charge, `enc +
    /// P·px + 256 MiB` (`P` the per-pixel literal of its plan, shared with
    /// TIFF). Rule, fixed before the run: that sum ≥ ⌈1.25 × max E⌉ of the
    /// same whole open at every size. E per run (encoded bytes): 12 MP
    /// (51,909,722) 389,431,560 / 389,349,640 / 389,415,176; 48 MP
    /// (167,137,902) 1,561,756,752 / 1,561,461,840 / 1,561,216,080; 67 MP
    /// (228,943,901) 2,148,550,144 / 2,148,828,672 / 2,148,779,520; 96 MP
    /// (337,047,852) 3,074,820,232 / 3,074,918,536 / 3,074,754,720; 100 MP
    /// (343,726,658) 3,203,303,632 / 3,203,320,040 / 3,203,401,936. P = 32
    /// missed at 67, 96 and 100 MP (1.231× / 1.196× / 1.190×), so P is 34,
    /// the least integer that holds every row: margins (admitted − need)
    /// 248,109,840 / 141,593,866 / 92,735,901 / 25,835,138 / 7,909,694, the
    /// admitted sum 1.887× / 1.341× / 1.293× / 1.258× / 1.2525× of max E.
    /// The fixtures encode at about 3.4 B/px; a PNG that encodes smaller has
    /// less `enc` credit.
    /// Floors: the layout gate `2·bytes + (layout.depth/2)·w·h + 32 MiB ≤
    /// maximumScratch` needs `F + a·px + 3·enc + 32 MiB ≥ 12·px` (rgba8/bgra8)
    /// or `24·px` (rgba16); a = 18 ≥ 12 and a = 42 ≥ 24 hold at every size.
    ///
    /// Fit at 48 MP, `F + a·48,771,072 + 4·enc + 64 MiB + 128 KiB + raster`
    /// (the opened layer: 4 B/px, wide 8): heic8 1,445,872,316; gain map
    /// 1,440,430,304; avif8 1,342,749,972; jpeg8 1,420,551,068; heic10
    /// 2,688,928,576; avif10 2,558,110,932.
    /// Follow-up: at the 2 GiB test pin the core refuses avif10-48; at 8 GiB
    /// the wide 48, 67, 96 and 100 MP opens pass (`verify.py` A10, A11). A
    /// wide still is one 16-bit raster layer, 8 B/px of the typed format's
    /// decoded cap (8 GiB now; it was 1.6 GB when this was measured,
    /// which left 800 MB at 100 MP), so a canvas-sized mask (2 B/px) fits:
    /// gate A10 records that `add_layer_mask` succeeds on the 100 MP still,
    /// as it does on a 100 MP 16-bit PNG (A12). When this was first measured the
    /// still kept a 16-bit master beside the raster, which filled the 1.6 GB
    /// cap, and the mask was refused.
    static let scratchFixed8: UInt64 = 187 * 1024 * 1024
    static let scratchPerPixel8: UInt64 = 18
    static let scratchFixedWide: UInt64 = 45 * 1024 * 1024
    static let scratchPerPixelWide: UInt64 = 42
    static func scratchFixed(depth: Int) -> UInt64 { depth <= 8 ? scratchFixed8 : scratchFixedWide }
    static func scratchPerPixel(depth: Int) -> UInt64 { depth <= 8 ? scratchPerPixel8 : scratchPerPixelWide }
}

extension HostStillDecoder {
    /// The whole 8-bit still OPEN, measured, not guessed:
    /// `openFixed8 + openPerPixel8·w·h + 3·encoded + 64 MiB`, which
    /// `ImageDocument.openHostSource` admits for a single 8-bit still before
    /// `HostSourceWork.capture`. The capture figure above charges the capture
    /// alone; the open runs capture, the initial document, the
    /// metadata/PPI/mattes pass, the working-space adoption and the first
    /// projection (`render()` + `makeCGImage`) one after the other, and the
    /// projection binds: a -O stage harness (app sources minus `main.swift`,
    /// release core, 8 GiB, one run each) read peak − pre-open footprint at
    /// 96 MP heic8 / gain map / avif8 of capture 1,663,878,392 /
    /// 1,665,861,456 / 1,553,646,792, initial document 1,995,720,192 /
    /// 1,999,324,720 / 1,936,328,168, adoption 1,217,316,088 / 1,604,486,704
    /// (the gain map's Display P3 base converts) / 1,168,442,568 and render
    /// 3,138,275,920 / 3,141,257,856 / 3,089,418,808 — the rendered document
    /// (≈ 1.22 GB retained) plus `dr_doc_flattened`'s 1,920,959,832 (20 B/px).
    ///
    /// Method: MCP `open_document` on the Release app in a fresh instance at
    /// `DR_WORKING_LIMIT_BYTES` 8 GiB, `MallocLargeCache=0`, macOS 26.5.1;
    /// `proc_reset_footprint_interval`, pre = `ri_phys_footprint`, the call,
    /// E = `ri_interval_max_phys_footprint` − pre; 3 runs, retained before
    /// the open 0. Fixtures as above (`author.swift`, `avifenc -s 8 -y 420`),
    /// byte-identical to that record's. E per run:
    ///   heic8-12 397,508,944 / 393,937,232 / 393,740,624
    ///   heic8-48 1,590,461,760 / 1,590,396,224 / 1,590,068,544
    ///   heic8-96 3,130,542,648 / 3,130,231,352 / 3,130,591,824
    ///   heic8-gainmap-12 397,050,216 / 400,802,176 / 397,197,648
    ///   heic8-gainmap-48 1,589,642,584 / 1,593,394,520 / 1,593,230,656
    ///   heic8-gainmap-96 3,128,986,192 / 3,132,508,776 / 3,132,443,192
    ///   avif8-12 406,880,592 / 406,815,056 / 406,929,744
    ///   avif8-48 1,559,610,640 / 1,559,594,256 / 1,559,348,496
    ///   avif8-96 3,076,458,992 / 3,221,637,640 / 3,225,635,312
    ///   iMac Blue 1,182,630,896 / 1,186,350,088 / 1,186,464,728
    ///   Sonoma 1,180,156,888 / 1,180,140,576 / 1,180,058,584
    /// Rule: batch 140's (above). Every case and each kind's 48→96 MP
    /// requirement line continued to 100 MP (slopes heic8 40.76, gain map
    /// 40.74, avif8 44.09 B/px; encoded size continued on the same line) must
    /// satisfy `openScratch ≥ ⌈1.25 × max E⌉`; the smallest 48 MP charge wins
    /// (ties: the larger `a`), `F` rounded up to a MiB. Result: F = 30 MiB
    /// (31,457,280; exact 31,452,844), a = 41; 48 MP charge 2,031,071,232
    /// (41.6 B/px). openScratch / P/E: heic8-12 619,047,589 / 1.557; -48
    /// 2,180,368,909 / 1.371; -96 4,196,342,077 / 1.340; gain map -12
    /// 619,441,837 / 1.546; -48 2,176,287,400 / 1.366; -96 4,190,291,083 /
    /// 1.338; avif8-12 599,707,303 / 1.474; -48 2,103,027,151 / 1.348; -96
    /// 4,044,034,729 / 1.254; iMac Blue 1,661,303,031 / 1.400; Sonoma
    /// 1,643,535,462 / 1.393. Binding: the avif8 line at 100 MP (4,436 B to
    /// spare) and avif8-96 (11,990,589).
    ///
    /// Where it binds (need = openScratch + the retained 0 of a fresh
    /// instance): the 48 MP HEICs need 2,180,368,909 and 2,176,287,400 and
    /// 96 MP about 4.2 GB, all admitted from a 16 GB Mac (8 GiB) up. The
    /// core's flat JPEG/PNG open is admitted by `FlatOpenAdmission`,
    /// with its own record.
    /// Driven with this gate in place: at 8 GiB, 96 MP heic8 / gain map /
    /// avif8 max E 3,130,968,656 / 3,128,789,680 / 3,081,456,136 (1.340× /
    /// 1.339× / 1.312×).
    static let openFixed8: UInt64 = 30 * 1024 * 1024
    static let openPerPixel8: UInt64 = 41

    /// F + a·w·h + 3·encoded + 64 MiB, overflow-checked through
    /// `PrecisionDocumentDescriptor.sum` (`HostSourceInput.scratch`'s shape).
    static func openScratch(width: Int, height: Int, encoded: UInt64) throws -> UInt64 {
        let (pixels, overflow) = UInt64(width).multipliedReportingOverflow(by: UInt64(height))
        let (term, termOverflow) = pixels.multipliedReportingOverflow(by: openPerPixel8)
        guard !overflow, !termOverflow else {
            throw DoctoredCoreError(message: "Working-memory calculation overflow.")
        }
        let files = try PrecisionDocumentDescriptor.sum(encoded, PrecisionDocumentDescriptor.sum(encoded, encoded))
        return try PrecisionDocumentDescriptor.sum(PrecisionDocumentDescriptor.sum(openFixed8, term),
            PrecisionDocumentDescriptor.sum(files, 64 * 1024 * 1024))
    }
}
