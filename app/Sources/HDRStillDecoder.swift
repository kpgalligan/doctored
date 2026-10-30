import CoreGraphics
import CoreImage
import Foundation
import ImageIO

// The HDR photo decode (v1.x). The host owns stills (`app/CLAUDE.md`, "Described layers"):
// ImageIO says what a file is and Core Image applies a gain map; the samples cross into the core
// at native float (`DoctoredCore+HDR.swift`), where the one reference-white rule lives
// (`core/src/hdr_transfer.rs`): 1.0 is a gain map's SDR base white, and 203 cd/m² for PQ and HLG.
// `HostStillDecoder` is unchanged for every other caller — Place, Load Files into Stack (scoped to
// `.sdr`), linked images and the retained-image tools keep the SDR base / PQ refusal.

/// What kind of HDR photo a file is.
enum HDRStillKind: String {
    /// An SDR base with an ISO 21496-1 or Apple gain-map auxiliary image.
    case gainMap = "gain_map"
    /// BT.2100 PQ codes (HEIC, AVIF, PNG cICP, JPEG XL).
    case pq
    /// BT.2100 HLG codes.
    case hlg
}

enum HDRStillDecoder {
    /// The containers an HDR photo arrives in. Camera RAW is not one of them:
    /// `ImageDocument.openDocument(at:)` routes RAW before asking.
    static let containerTypes: Set<String> = [
        "public.heic", "public.heif", "public.avif", "public.png", "public.jpeg-xl",
    ]

    /// The BT.2100 spaces ImageIO decodes a PQ/HLG still into, by name: the
    /// transfer and the primaries the core is told. ImageIO names a BT.709
    /// cICP/nclx still `ITUR_709_PQ` / `_HLG` (measured on PNG, AVIF and JPEG
    /// XL), so those two rows stand beside the BT.2020 and Display P3 ones.
    static let spaces: [String: (kind: HDRStillKind, primaries: String)] = [
        "kCGColorSpaceITUR_2100_PQ": (.pq, "bt2020"),
        "kCGColorSpaceITUR_2020_PQ": (.pq, "bt2020"),
        "kCGColorSpaceDisplayP3_PQ": (.pq, "display_p3"),
        "kCGColorSpaceITUR_709_PQ": (.pq, "bt709"),
        "kCGColorSpaceITUR_2100_HLG": (.hlg, "bt2020"),
        "kCGColorSpaceITUR_2020_HLG": (.hlg, "bt2020"),
        "kCGColorSpaceDisplayP3_HLG": (.hlg, "display_p3"),
        "kCGColorSpaceITUR_709_HLG": (.hlg, "bt709"),
    ]

    /// The refusal for a gain-map photo whose expansion has no headroom.
    static let noHeadroom = "This photo carries a gain map that adds no headroom above SDR white, "
        + "so there is nothing brighter than white to open. Open it as SDR (turn off File ▸ Open "
        + "HDR Photos as HDR, or pass hdr_photo \"sdr\")."

    /// The source and its primary image, or nil.
    private static func primary(_ url: URL) -> (CGImageSource, Int)? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL,
                  [kCGImageSourceShouldCache: false] as CFDictionary),
              let type = CGImageSourceGetType(source) as String?, containerTypes.contains(type)
        else { return nil }
        let count = CGImageSourceGetCount(source), index = CGImageSourceGetPrimaryImageIndex(source)
        guard count > 0, index < count else { return nil }
        return (source, index)
    }

    /// An ISO or Apple gain-map auxiliary at the primary image → `.gainMap`;
    /// otherwise a LAZILY created HDR decode (no pixels decoded) whose colour
    /// space uses a BT.2100 transfer → `.pq` / `.hlg` by name (an unnamed or
    /// unknown 2100 space classifies by its transfer and `decode` refuses it
    /// by name); anything else is nil, an ordinary still.
    static func classify(_ url: URL) -> HDRStillKind? {
        guard let (source, index) = primary(url) else { return nil }
        if [kCGImageAuxiliaryDataTypeISOGainMap, kCGImageAuxiliaryDataTypeHDRGainMap].contains(where: {
            CGImageSourceCopyAuxiliaryDataInfoAtIndex(source, index, $0) != nil
        }) {
            return .gainMap
        }
        guard let image = CGImageSourceCreateImageAtIndex(source, index, [
                  kCGImageSourceShouldCache: false, kCGImageSourceShouldAllowFloat: true,
                  kCGImageSourceDecodeRequest: kCGImageSourceDecodeToHDR,
              ] as CFDictionary),
              let space = image.colorSpace, CGColorSpaceUsesITUR_2100TF(space) else { return nil }
        if let name = space.name as String?, let entry = spaces[name] { return entry.kind }
        return CGColorSpaceIsHLGBased(space) ? .hlg : .pq
    }

    /// The photo as a 32-bit float document, and what the decode reports.
    /// A gain map: Core Image's expansion at full headroom
    /// (`CIImage(contentsOf:options:[.expandToHDR: true])`) rendered as
    /// premultiplied `.RGBAf` in the SDR base space's extended-linearized
    /// twin, whose ICC becomes the document profile; refused by name when
    /// the expansion reports no headroom above 1. PQ/HLG: ImageIO's integer
    /// HDR decode through `HostStillDecoder`'s layout rules, the transfer and
    /// primaries from `spaces`. The EXIF orientation goes to the core.
    static func decode(_ url: URL, kind: HDRStillKind, maximumScratch: UInt64,
                       control: RasterJobControl?) throws -> (RasterDocument, [String: Any]) {
        guard control?.isCancelled != true else { throw JobCancelled() }
        guard let (source, index) = primary(url),
              let info = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any],
              let width = (info[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue,
              let height = (info[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue,
              width > 0, height > 0 else {
            throw HostSourceError.failure("macOS's image decoder did not report this HDR photo's dimensions.")
        }
        let orientation = (info[kCGImagePropertyOrientation] as? NSNumber)?.intValue ?? 1
        guard (1...8).contains(orientation) else {
            throw HostSourceError.failure("The image orientation is invalid.")
        }
        switch kind {
        case .gainMap:
            return try decodeGainMap(url, source: source, index: index, width: width, height: height,
                                     orientation: orientation, maximumScratch: maximumScratch,
                                     control: control)
        case .pq, .hlg:
            return try decodeSignal(source, index: index, kind: kind, width: width, height: height,
                                    orientation: orientation, maximumScratch: maximumScratch,
                                    control: control)
        }
    }

    private static func decodeGainMap(_ url: URL, source: CGImageSource, index: Int, width: Int,
                                      height: Int, orientation: Int, maximumScratch: UInt64,
                                      control: RasterJobControl?) throws -> (RasterDocument, [String: Any]) {
        guard let base = CGImageSourceCreateImageAtIndex(source, index,
                  [kCGImageSourceShouldCache: false] as CFDictionary),
              let baseSpace = base.colorSpace, baseSpace.model == .rgb,
              let profile = baseSpace.copyICCData() as Data?, !profile.isEmpty,
              let linear = CGColorSpaceCreateExtendedLinearized(baseSpace) else {
            throw HostSourceError.failure("This gain-map photo's SDR base has no RGB colour profile "
                + "the HDR rendition could be expressed in.")
        }
        guard let expanded = CIImage(contentsOf: url, options: [.expandToHDR: true]) else {
            throw HostSourceError.failure("Core Image could not apply this photo's gain map.")
        }
        let headroom = Double(expanded.contentHeadroom)
        guard headroom > 1 else { throw HostSourceError.failure(noHeadroom) }
        let extent = expanded.extent
        guard extent.origin == .zero, Int(extent.width) == width, Int(extent.height) == height else {
            throw HostSourceError.failure("The gain-map rendition is \(Int(extent.width))×"
                + "\(Int(extent.height)), not the \(width)×\(height) its file reports.")
        }
        let (count, overflow) = width.multipliedReportingOverflow(by: height)
        let (bytes, bytesOverflow) = count.multipliedReportingOverflow(by: 16)
        guard !overflow, !bytesOverflow, UInt64(bytes) <= maximumScratch / 2 else {
            throw HostSourceError.failure("The HDR rendition's buffer exceeds its admitted plan.")
        }
        guard control?.isCancelled != true else { throw JobCancelled() }
        var pixels = Data(count: bytes)
        let context = CIContext(options: [
            .workingColorSpace: linear, .cacheIntermediates: false, .useSoftwareRenderer: false,
        ])
        pixels.withUnsafeMutableBytes { out in
            guard let base = out.baseAddress else { return }
            context.render(expanded, toBitmap: base, rowBytes: width * 16, bounds: extent,
                           format: .RGBAf, colorSpace: linear)
        }
        let document = try RasterDocument.fromHostHDRPixels(bytes: pixels, width: width,
            height: height, rowBytes: width * 16, layout: "rgba32f_le_premultiplied",
            orientation: orientation, transfer: "linear", primaries: nil, profile: profile,
            control: control)
        return (document, ["decoder": "core-image", "content_headroom": headroom,
                           "profile_name": baseSpace.name as String? ?? "Decoded ICC"])
    }

    private static func decodeSignal(_ source: CGImageSource, index: Int, kind: HDRStillKind,
                                     width: Int, height: Int, orientation: Int,
                                     maximumScratch: UInt64,
                                     control: RasterJobControl?) throws -> (RasterDocument, [String: Any]) {
        guard let image = CGImageSourceCreateImageAtIndex(source, index, [
                  kCGImageSourceShouldCacheImmediately: true, kCGImageSourceShouldAllowFloat: true,
                  kCGImageSourceDecodeRequest: kCGImageSourceDecodeToHDR,
                  kCGImageSourceGenerateImageSpecificLumaScaling: false,
              ] as CFDictionary) else {
            throw HostSourceError.failure("macOS's image decoder could not decode this HDR photo.")
        }
        guard let space = image.colorSpace, CGColorSpaceUsesITUR_2100TF(space),
              let name = space.name as String?, let entry = spaces[name], entry.kind == kind else {
            let named = image.colorSpace?.name as String? ?? "an unnamed HDR space"
            throw HostSourceError.failure("This HDR photo decodes in \(named), which this build "
                + "does not open; PQ and HLG in BT.709, Display P3 and BT.2020 do.")
        }
        guard image.width == width, image.height == height else {
            throw HostSourceError.failure("The decoded image is \(image.width)×\(image.height), not "
                + "the \(width)×\(height) its file reports.")
        }
        let layout = try HostStillDecoder.layout(image)
        guard layout.depth == 16 else {
            throw HostSourceError.failure("macOS's image decoder returned \(image.bitsPerComponent)-bit "
                + "samples for this HDR photo; PQ and HLG open from 10- to 16-bit samples.")
        }
        let (bytes, overflow) = image.bytesPerRow.multipliedReportingOverflow(by: image.height)
        guard !overflow, bytes > 0, UInt64(bytes) <= maximumScratch / 2 else {
            throw HostSourceError.failure("The full image buffer exceeds its admitted layout.")
        }
        guard let captured = image.dataProvider?.data else {
            throw HostSourceError.failure(HostStillDecoder.noDecodedPixels)
        }
        guard CFDataGetLength(captured) == bytes else {
            throw HostSourceError.failure("The full image provider disagrees with its declared buffer size.")
        }
        let document = try RasterDocument.fromHostHDRPixels(bytes: captured as Data, width: width,
            height: height, rowBytes: image.bytesPerRow, layout: layout.token,
            orientation: orientation, transfer: kind.rawValue, primaries: entry.primaries,
            profile: nil, control: control)
        return (document, ["decoder": "apple-imageio", "layout": layout.token,
                           "color_space": name, "primaries": entry.primaries])
    }
}

extension HDRStillDecoder {
    /// The whole HDR photo OPEN, `F + a·w·h + 3·encoded + 64 MiB`
    /// (`openScratch`), admitted by `ImageDocument.openHDRStill` before the
    /// decode: Core Image's expansion or ImageIO's HDR decode, the f32 intake
    /// (the input, the sample vector and the tiles), the metadata pass and the
    /// first SDR projection (the open was measured with no EDR render after
    /// it; that has its own plan, `float_view::extended_display_scratch`).
    ///
    /// Measured on the Release app (`scripts/verify_v1x_hdr_photos.py
    /// --measure 3`): macOS 26.5.1, `DR_WORKING_LIMIT_BYTES` 8 GiB,
    /// `MallocLargeCache=0`, `DR_EDR_HEADROOM=1`, one MCP `open_document` per
    /// fresh instance, `proc_reset_footprint_interval`, E =
    /// `ri_interval_max_phys_footprint` − pre once the footprint has been
    /// still for 2 s; 3 runs, a baseline twin per run reading 0 / 0 / 16,384.
    /// Fixtures: `scripts/hdr-photos/author.py`'s admission HEICs (ImageIO's
    /// ISO gain map in Display P3; Core Image's 10-bit PQ). E per run
    /// (encoded bytes):
    ///   gain map 12 MP 4032×3024 (17,159) 631,488,992 / 631,603,680 / 632,586,696
    ///   gain map 48 MP 8064×6048 (35,961) 2,564,818,888 / 2,524,465,072 / 2,525,022,128
    ///   PQ 12 MP (15,940) 589,545,928 / 589,889,992 / 590,037,472
    ///   PQ 48 MP (27,726) 2,352,383,632 / 2,352,187,072 / 2,351,793,832
    /// (51.8–52.6 B/px for a gain map, 48.2–48.4 for PQ.) The rule, fixed
    /// before the run: `openScratch ≥ ⌈1.25 × max E⌉` for every case with
    /// `F` = 64 MiB; the least integer `a` is 63 (gain map 48 MP at 1.2503×),
    /// and `a` = 64 is chosen because run 1 of that case read 1.6 % above its
    /// other two. openScratch / max E: gain map 12 MP 914,606,357 / 1.4458; 48
    /// MP 3,255,674,219 / 1.2694; PQ 12 MP 914,602,700 / 1.5501; 48 MP
    /// 3,255,649,514 / 1.3840. A 48 MP HDR photo needs 3.26 GB, admitted from a
    /// 16 GB Mac (8 GiB) up.
    static let openFixed: UInt64 = 64 * 1024 * 1024
    static let openPerPixel: UInt64 = 64

    /// F + a·w·h + 3·encoded + 64 MiB, overflow-checked
    /// (`HostStillDecoder.openScratch`'s shape).
    static func openScratch(width: Int, height: Int, encoded: UInt64) throws -> UInt64 {
        let (pixels, overflow) = UInt64(width).multipliedReportingOverflow(by: UInt64(height))
        let (term, termOverflow) = pixels.multipliedReportingOverflow(by: openPerPixel)
        guard !overflow, !termOverflow else {
            throw DoctoredCoreError(message: "Working-memory calculation overflow.")
        }
        let files = try PrecisionDocumentDescriptor.sum(encoded,
            PrecisionDocumentDescriptor.sum(encoded, encoded))
        return try PrecisionDocumentDescriptor.sum(PrecisionDocumentDescriptor.sum(openFixed, term),
            PrecisionDocumentDescriptor.sum(files, 64 * 1024 * 1024))
    }
}
