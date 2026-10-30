import Foundation
import ImageIO

// `FlatOpenAdmission.charge(for:)` is an ImageIO header probe (no decode) that
// `ImageDocument.openDocument(at:)` admits before `RasterDocument.open(url:)` for a single-image ≤
// 8-bit JPEG/PNG; place keeps `ImageSourceFile`'s charge. Every other 8-bit open (TIFF, WebP, GIF,
// BMP, PSD, `.rz`, a 16-bit PNG at 8 bits, the ImageIO fallback) sets `Opened.projectionUncharged`,
// and `read(from:ofType:)` admits `FlatOpenAdmission.firstProjectionBytes` (`commitStageBytes` + 4n
// when it adopts; record beside it) after the decode and before installing anything, so a refused
// open or Revert changes nothing.

/// The core flat JPEG/PNG open's whole-open charge: decode, document, adoption, first projection.
/// `ImageDocument.openDocument(at:)` admits it before `RasterDocument.open(url:)` runs; place
/// (`ImageSourceFile.decode`) keeps its own charge.
/// TIFF, WebP, GIF, BMP, PSD, DRDC, a 16-bit PNG opened at 8 bits and the ImageIO
/// fallback decode are not charged here: their decode is not planned up front, and
/// `read(from:ofType:)` admits their first projection after the decode
/// (`firstProjectionBytes`).
///
/// Record (batch 165): the Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB,
/// `MallocLargeCache=0`, macOS 26.5.1, a fresh instance per case, MCP
/// `open_document`, E = `ri_interval_max_phys_footprint` − pre, 3 runs, retained
/// before the open 0, a baseline twin at E 0 ×3. Fixtures by
/// `scripts/host-still-admission/author.swift` (jpeg8, png8). E per run:
///   jpeg8-12 (4032×3024, 5,277,243 B) 340,640,008 / 340,541,704 / 340,967,688
///   jpeg8-48 (8064×6048, 21,065,959 B) 1,362,396,120 / 1,365,869,528 / 1,362,363,352
///   jpeg8-96 (12000×8000, 41,446,748 B) 2,689,157,040 / 2,688,763,824 / 2,688,845,744
///   png8-12 (25,791,018 B) 340,984,072 / 336,920,840 / 341,000,456
///   png8-48 (103,117,383 B) 1,366,180,824 / 1,366,295,512 / 1,366,328,280
///   png8-96 (202,946,848 B) 2,689,566,640 / 2,686,420,912 / 2,690,025,392
/// (28.0 B/px from 48 to 96 MP for both: the decoded image, the document and
/// the first projection, `dr_doc_flattened`'s 20 B/px plus the display payload.)
/// Rule: batch 140's (`HostStillDecoder.openScratch`'s record). Every case and
/// each kind's 48→96 MP requirement line continued to 100 MP must satisfy
/// `openScratch ≥ ⌈1.25 × max E⌉`; the smallest 48 MP charge wins (ties: the
/// larger `a`), F rounded up to a MiB. Result: F = 0 (exact need −4,176,231),
/// a = 34. openScratch / P/E: jpeg8-12 430,385,841 / 1.262 (binds, 4,176,231
/// to spare); -48 1,721,414,325 / 1.260; -96 3,388,340,244 / 1.260; png8-12
/// 491,927,166 / 1.443; -48 1,967,568,597 / 1.440; -96 3,872,840,544 / 1.440;
/// the 100 MP line points have 27,979,423 (jpeg8) and 531,537,365 (png8) spare.
/// 96 MP needs 3.39 GB (JPEG) / 3.87 GB (PNG), admitted from a 16 GB Mac
/// (8 GiB) up.
enum FlatOpenAdmission {
    static let openFixed: UInt64 = 0
    static let openPerPixel: UInt64 = 34

    /// F + a·w·h + 3·encoded, overflow-checked through PrecisionDocumentDescriptor.sum.
    static func openScratch(width: Int, height: Int, encoded: UInt64) throws -> UInt64 {
        let (pixels, overflow) = UInt64(width).multipliedReportingOverflow(by: UInt64(height))
        let (term, termOverflow) = pixels.multipliedReportingOverflow(by: openPerPixel)
        guard width >= 0, height >= 0, !overflow, !termOverflow else {
            throw DoctoredCoreError(message: "Working-memory calculation overflow.")
        }
        let files = try PrecisionDocumentDescriptor.sum(encoded, PrecisionDocumentDescriptor.sum(encoded, encoded))
        return try PrecisionDocumentDescriptor.sum(PrecisionDocumentDescriptor.sum(openFixed, term), files)
    }

    /// CGImageSource header probe (kCGImageSourceShouldCache false, no decode). For a single-image
    /// "public.jpeg" / "public.png" with kCGImagePropertyDepth ≤ 8, returns openScratch with
    /// encoded = file size. Anything else returns nil.
    static func charge(for url: URL) throws -> UInt64? {
        guard let source = CGImageSourceCreateWithURL(url as CFURL,
                  [kCGImageSourceShouldCache: false] as CFDictionary),
              let type = CGImageSourceGetType(source) as String?,
              ["public.jpeg", "public.png"].contains(type),
              CGImageSourceGetCount(source) == 1,
              let info = CGImageSourceCopyPropertiesAtIndex(source, 0,
                  [kCGImageSourceShouldCache: false] as CFDictionary) as? [CFString: Any],
              let depth = (info[kCGImagePropertyDepth] as? NSNumber)?.intValue, depth <= 8,
              let width = (info[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue,
              let height = (info[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue,
              width > 0, height > 0,
              let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize, size >= 0
        else { return nil }
        return try openScratch(width: width, height: height, encoded: UInt64(size))
    }
}

extension FlatOpenAdmission {
    /// An opened 8-bit document's first projection, for an open whose own charge omits it:
    /// `ByteFilterAdmission.commitStageBytes(pixels: w·h, in: document)` (8·w·h + the margined
    /// projection) plus 4·w·h when `adopts`. `adopts` is `read(from:ofType:)`'s condition for running
    /// the working-space adoption, which the gate cannot tell will convert (the core decides), so a
    /// flat open always carries the term and a `.rz` or PSD never does. Overflow throws.
    ///
    /// Record (batch 166): the Release app at `DR_WORKING_LIMIT_BYTES` 8 GiB, `MallocLargeCache=0`,
    /// a fresh instance per run, MCP `open_document`, E = `ri_interval_max_phys_footprint` − pre,
    /// 3 runs, a baseline twin at E 0 ×3. P = this charge + ΔR, ΔR = (R_post − R_pre) − 4·w·h,
    /// R = `unique_retained − cache`. Fixtures: `save_copy` of png8-48 (8064×6048) as TIFF / WebP /
    /// GIF / BMP, of png8-96 (12000×8000) as TIFF, of png8-48 assigned Display P3 as TIFF (the open
    /// converts it), a 16-bit PNG at `precision: rgba8`, the batch-165 R449 document (8256×5504, a
    /// placed 45 MP JPEG over white) as `.rz` and layered PSD, and a planar CMYK TIFF only the ImageIO
    /// fallback opens. E per run / P / P/E:
    ///   tiff48     1,365,017,560 / 1,365,033,944 / 1,364,886,488   P 2,389,805,865  1.751
    ///   tiff96     2,688,812,928 / 2,685,470,592 / 2,688,894,848   P 4,704,028,409  1.749
    ///   webp48     1,361,232,832 / 1,361,626,048 / 1,361,281,984   P 2,389,805,707  1.755
    ///   gif48      1,364,919,256 / 1,365,115,864 / 1,364,984,792   P 2,389,805,707  1.751
    ///   bmp48      1,365,066,712 / 1,364,902,872 / 1,364,689,880   P 2,389,805,707  1.751
    ///   png16 at 8 1,364,853,720 / 1,361,478,616 / 1,361,282,008   P 2,389,805,775  1.751
    ///   PSD layers 1,634,501,784 / 1,637,729,432 / 1,634,600,088   P 2,226,641,670  1.360
    ///   PSD comp.  1,271,612,328 / 1,271,481,256 / 1,270,924,200   P 2,044,868,107  1.608
    ///   .rz        1,272,890,376 / 1,272,824,840 / 1,272,759,304   P 2,226,651,399  1.749
    ///   fallback   1,364,886,800 / 1,368,474,872 / 1,368,540,408   P 2,389,806,682  1.746
    ///   P3 → sRGB  1,752,614,112 / 1,756,300,512 / 1,752,335,584   P 2,389,805,865  1.361
    /// Candidates: the margined projection alone (+ 4·w·h adopting) gave PSD layers 1.138 and the
    /// converting open 1.139; `commitStageBytes` alone gave the converting open 1.2496 (a conversion
    /// costs ≈ 8·w·h over the same open without one). Rule: every row ≥ ⌈1.25 × max E⌉.
    /// Since m8 rose to 7, every 8-bit P rises by 7/16 − 6/16 of the structural estimate (the
    /// 48 MP rows by 73,156,608, the 45 MP rows by 68,161,536), so P/E only rises.
    static func firstProjectionBytes(_ document: RasterDocument, adopts: Bool) throws -> UInt64 {
        let (pixels, overflow) = UInt64(document.width).multipliedReportingOverflow(
            by: UInt64(document.height))
        let (copy, copyOverflow) = pixels.multipliedReportingOverflow(by: 4)
        guard !overflow, !copyOverflow else {
            throw DoctoredCoreError(message: "Working-memory calculation overflow.")
        }
        let stage = try ByteFilterAdmission.commitStageBytes(pixels: pixels, in: document)
        return adopts ? try PrecisionDocumentDescriptor.sum(stage, copy) : stage
    }
}
