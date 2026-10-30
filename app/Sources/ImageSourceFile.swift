import Foundation
import CryptoKit
import ImageIO

// A plain PNG/JPEG/TIFF place admits the per-depth figure measured on JPEG and PNG (TIFF shares it
// unmeasured) (`ImageSourceFile.scratchPerPixel8` / `scratchPerPixel16`), whose measurement record
// sits beside the constants. A floating-point TIFF decodes at 32 bits (`scratchPerPixel32`) and is
// placed at any document depth: the core's place converts it (`convert_fallback`) and the layer is
// rasterized in the same edit.
//
// `ImageDocument.hasClosed` and `ImageSourceCapture` reject publication to closed owners.

enum ImageSourceStorage: String { case embedded, linked }
enum ImageSourceDepthConversion: String { case convertFallback = "convert_fallback" }

/// Container inspection selects the genuine decoder. All decodes use a private
/// file snapshot, so the fingerprint always describes the bytes we decoded.
struct ImageSourceFile {
    /// Intake reserves three encoded-file buffers plus codec preparation.
    /// Standalone link checks apply that file-size bound before any I/O.
    static var maximumEncodedBytes: UInt64 {
        let budget = RasterResources.shared.workingLimit
        return (budget - min(budget, 16 * 1024 * 1024)) / 3
    }

    let url: URL
    let depth: Int
    let byteCount: UInt64
    let scratchEstimate: UInt64

    init(url: URL) throws {
        self.url = url.standardizedFileURL
        let values = try url.resourceValues(forKeys: [.fileSizeKey, .isRegularFileKey])
        guard values.isRegularFile == true, let count = values.fileSize, count >= 0,
              UInt64(count) <= Self.maximumEncodedBytes,
              !ImageDocument.hasNativeSignature(at: url),
              let source = CGImageSourceCreateWithURL(url as CFURL,
                  [kCGImageSourceShouldCache: false] as CFDictionary),
              let type = CGImageSourceGetType(source) as String?,
              ["public.png", "public.jpeg", "public.tiff"].contains(type),
              CGImageSourceGetCount(source) == 1,
              let info = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = (info[kCGImagePropertyPixelWidth] as? NSNumber)?.uint64Value,
              let height = (info[kCGImagePropertyPixelHeight] as? NSNumber)?.uint64Value,
              let bits = (info[kCGImagePropertyDepth] as? NSNumber)?.intValue,
              bits > 0, bits <= 8 || bits == 16 || PrecisionImportRequest.isFloatImage(url),
              width > 0, height > 0, width <= UInt64(RasterImage.maxResizePixels) / height else {
            throw DoctoredCoreError(message: "Choose a single 8-bit PNG/JPEG/TIFF, a genuine 16-bit RGB PNG/TIFF or a floating-point TIFF image within the document size limit.")
        }
        // A floating-point TIFF (f16 or f32) decodes to an f32 float document,
        // so its master is 32-bit whatever its stored sample width.
        depth = PrecisionImportRequest.isFloatImage(url) ? 32 : (bits <= 8 ? 8 : 16)
        byteCount = UInt64(count)
        let filePeak = try PrecisionDocumentDescriptor.sum(byteCount, byteCount)
        let perPixel = depth == 8 ? Self.scratchPerPixel8
            : depth == 16 ? Self.scratchPerPixel16 : Self.scratchPerPixel32
        scratchEstimate = try PrecisionDocumentDescriptor.sum(
            PrecisionDocumentDescriptor.sum(filePeak, byteCount),
            width * height * perPixel + 16 * 1024 * 1024)
    }

    /// The staged snapshot's asset, its fingerprint and the open's profile
    /// refusal: the sentence naming an embedded profile the core open
    /// converted from or ignored, nil when it kept or found none.
    func decode() throws -> (asset: RasterDocument, sha256: String, profileRefusal: String?) {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("doctored-source-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false,
                                               attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: folder) }
        let staged = folder.appendingPathComponent("source.\(url.pathExtension)")
        let digest = try Self.fingerprint(url, copyingTo: staged, maximumBytes: byteCount)
        // Reinspect the captured bytes: a file may have changed after the open
        // panel or the first admission estimate. A larger capture refuses.
        let captured = try ImageSourceFile(url: staged)
        guard captured.depth == depth, captured.scratchEstimate <= scratchEstimate else {
            throw DoctoredCoreError(message: "The source changed while it was being read. Try the operation again.")
        }
        let asset: RasterDocument
        let refusal: String?
        if depth == 16 || depth == 32 {
            let context = try PrecisionWorkContext()
            asset = try RasterDocument(precision: RasterPrecisionDocument.openPreserving(
                url: staged, workingLimit: context.limit, otherRetained: context.otherRetained))
            refusal = asset.precisionProfileRefusal(openedFrom: staged)
        } else {
            asset = try RasterDocument.open(url: staged)
            refusal = RasterDocument.flatOpenProfileRefusal(at: staged)
        }
        return (asset, digest, refusal)
    }

    /// Bounded streaming I/O; no mapped file and no full-file Data allocation.
    static func fingerprint(_ url: URL, copyingTo destination: URL? = nil,
                            maximumBytes: UInt64? = nil) throws -> String {
        let values = try url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
        guard values.isRegularFile == true, let size = values.fileSize, size >= 0 else {
            throw DoctoredCoreError(message: "The linked source must be a regular file.")
        }
        guard UInt64(size) <= maximumEncodedBytes else {
            throw DoctoredCoreError(message: "The source file exceeds the supported encoded-file budget.")
        }
        let limit = maximumBytes ?? UInt64(size)
        guard UInt64(size) <= limit else { throw DoctoredCoreError(message: "The source grew while it was being read. Try again.") }
        let input = try FileHandle(forReadingFrom: url)
        defer { try? input.close() }
        var output: FileHandle?
        if let destination {
            guard FileManager.default.createFile(atPath: destination.path, contents: nil,
                                                  attributes: [.posixPermissions: 0o600]) else {
                throw DoctoredCoreError(message: "Could not create an image source snapshot.")
            }
            output = try FileHandle(forWritingTo: destination)
        }
        defer { try? output?.close() }
        var hash = SHA256()
        var total: UInt64 = 0
        while let data = try input.read(upToCount: 1024 * 1024), !data.isEmpty {
            total = try PrecisionDocumentDescriptor.sum(total, UInt64(data.count))
            guard total <= limit else { throw DoctoredCoreError(message: "The source grew while it was being read. Try again.") }
            hash.update(data: data)
            try output?.write(contentsOf: data)
        }
        guard total == UInt64(size) else { throw DoctoredCoreError(message: "The source changed while it was being read. Try again.") }
        return hash.finalize().map { String(format: "%02x", $0) }.joined()
    }
}

extension ImageSourceFile {
    /// Plain PNG/JPEG/TIFF placement per pixel, measured on JPEG and
    /// PNG (TIFF shares the figure unmeasured): the place
    /// estimate is `3·encoded + a·w·h + 16 MiB`, and `a` is the smallest
    /// integer with `a·px + 3·enc + 16 MiB ≥ ⌈1.25 × max E_place⌉` for every
    /// case of its depth. Method: `HostStillDecoder.scratchPerPixel8`'s record;
    /// after one setup `ImageSourceFile(url:)`, E_place measures
    /// `ImageSourceFile(url:)` + `decode()`. Every baseline twin reads E 0.
    ///
    /// Per case (dimensions, encoded bytes): max E, its run's pre; three runs.
    ///   jpeg8-12 4032×3024, 5,277,243 B: E 113,705,080 (pre 6,128,192);
    ///     113,705,080 / 113,672,312 / 113,672,312 → a 9
    ///   jpeg8-48 8064×6048, 21,065,959 B: E 453,968,232 (pre 6,111,808);
    ///     453,951,848 / 453,951,848 / 453,968,232 → a 10
    ///   png8-12 4032×3024, 25,791,018 B: E 149,553,296 (pre 6,177,344);
    ///     149,520,528 / 149,553,296 / 149,520,528 → a 8
    ///   png8-48 8064×6048, 103,117,383 B: E 597,950,920 (pre 6,177,344);
    ///     597,950,920 / 597,950,920 / 597,934,536 → a 9
    ///   png16-12 4032×3024, 51,909,722 B: E 299,712,752 (pre 6,210,112);
    ///     299,712,752 / 299,712,752 / 299,712,752 → a 17
    ///   png16-48 8064×6048, 167,137,902 B: E 1,117,340,464 (pre 6,160,960);
    ///     1,117,324,080 / 1,117,340,464 / 1,117,307,696 → a 19
    ///
    /// Fit: jpeg8-48 placed into an 8064×6048 8-bit document,
    /// `10·48,771,072 + 3·21,065,959 + 16 MiB + 195,084,288 + 128 KiB` =
    /// 762,901,173.
    static let scratchPerPixel8: UInt64 = 10
    static let scratchPerPixel16: UInt64 = 19
    /// A floating-point TIFF placed at every depth (an 8- or 16-bit document
    /// included), fitted the same way to the WHOLE MCP `place_image` (decode,
    /// conversion, render and publication, so it is conservative for the
    /// decode alone). Method and inputs: `retained_source_float`'s
    /// `# Measurement record (32-bit float master)`; 6000×4000 RGBA f32
    /// Deflate TIFFs into the 24 MP float target.
    ///   place32 (sRGB), 79,622,209 B: E 963,085,056 / 962,921,192 /
    ///     963,232,512 → a 40
    ///   place32p3 (Display P3), 79,619,601 B: E 1,348,682,760 /
    ///     1,348,797,448 / 1,348,797,448 → a 60
    /// Fit: place32p3, `60·24,000,000 + 3·79,619,601 + 16 MiB` =
    /// 1,695,636,019 ≥ ⌈1.25 × 1,348,797,448⌉ = 1,685,996,810 (59 gives
    /// 1,671,636,019).
    static let scratchPerPixel32: UInt64 = 60
}

/// A target/index remains meaningful only for the same immutable editor state.
struct ImageSourceCapture {
    let source: RasterDocument
    let revision: UInt64
    let state: EditingState

    init(_ document: ImageDocument) throws {
        guard !document.hasClosed, let source = document.doc, !document.isLiveEditing, !document.isPublishingEditingState,
              let state = document.captureEditingState() else {
            throw DoctoredCoreError(message: "Finish the current gesture before editing an image source.")
        }
        self.source = source
        revision = document.revision
        self.state = state
    }

    func requireCurrent(_ document: ImageDocument) throws {
        guard !document.hasClosed, document.doc === source, document.revision == revision, !document.isLiveEditing,
              !document.isPublishingEditingState,
              document.isCurrentEditingState(state) else {
            throw DoctoredCoreError(message: "The document, target or selection changed. No image source edit was applied.")
        }
    }
}
