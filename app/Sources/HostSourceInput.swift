import Foundation
import ImageIO

/// Inspection is bounded and advisory; only the private snapshot supplies
/// authoritative decoder inputs and encoded resources.
struct HostSourceInput {
    struct Entry {
        let role: String
        let url: URL
        let bytes: UInt64
        let width: Int
        let height: Int
        let depth: Int?
    }
    let entries: [Entry]
    let scratch: UInt64

    static func claims(_ url: URL) -> Bool {
        // Claimed families stay on the strict full-source route even if their
        // bytes or ImageIO subtype are unsupported. They must not fall through
        // to the legacy byte thumbnail decoder after a failed inspection.
        if LivePhoto.locate(url) != nil
            || ["mov", "m4v", "mp4", "jxl", "heic", "heif", "avif", "jp2", "j2k", "jpf", "jpx"]
            .contains(url.pathExtension.lowercased()) { return true }
        guard let source = CGImageSourceCreateWithURL(url as CFURL,
            [kCGImageSourceShouldCache: false] as CFDictionary),
            let type = CGImageSourceGetType(source) as String? else { return false }
        return ["public.jpeg-xl", "public.heic", "public.heif", "public.avif",
                "public.jpeg-2000"].contains(type)
    }

    init(url: URL) throws {
        let pair = LivePhoto.locate(url)
        let files: [(String, URL)]
        if let pair { files = pair.still.map { [("still", $0), ("video", pair.video)] } ?? [("video", pair.video)] }
        else if ["mov", "m4v", "mp4"].contains(url.pathExtension.lowercased()) { files = [("video", url)] }
        else { files = [("still", url)] }
        try self.init(files: files)
    }
    init(files: [(String, URL)]) throws {
        _ = try RasterResources.shared.admit(scratch: 32 * 1024 * 1024)
        guard [["still"], ["video"], ["still", "video"]].contains(files.map(\.0)) else {
            throw HostSourceError.failure("A media source needs one still/movie resource or an ordered still/movie pair.")
        }
        var entries: [Entry] = [], encoded: UInt64 = 0
        for (role, file) in files {
            let bytes = try HostSourceSnapshot.size(file)
            let width: Int, height: Int, depth: Int?
            if role == "still" {
                let probe = try HostStillDecoder.inspect(file)
                width = probe.width; height = probe.height; depth = probe.depth
            } else {
                let probe = try HostVideoDecoder.inspect(file)
                width = probe.width; height = probe.height; depth = nil
            }
            encoded = try PrecisionDocumentDescriptor.sum(encoded, bytes)
            entries.append(Entry(role: role, url: file.standardizedFileURL, bytes: bytes, width: width,
                                 height: height, depth: depth))
        }
        guard encoded <= 512 * 1024 * 1024 else { throw HostSourceError.failure("Encoded source resources exceed 512 MiB.") }
        self.entries = entries
        scratch = try Self.scratch(encoded: encoded, media: entries.map { ($0.role, $0.width, $0.height, $0.depth) })
    }

    /// encoded*3 + Σ(width*height*perPixel(role, depth)) + Σ(still fixed(depth)) + 64 MiB; overflow-checked via PrecisionDocumentDescriptor.sum.
    /// A still with no known depth is charged the larger of the two depth classes.
    static func scratch(encoded: UInt64, media: [(role: String, width: Int, height: Int, depth: Int?)]) throws -> UInt64 {
        var total = try PrecisionDocumentDescriptor.sum(encoded, PrecisionDocumentDescriptor.sum(encoded, encoded))
        for item in media {
            let pixels = UInt64(item.width * item.height)
            func charge(fixed: UInt64, perPixel: UInt64) throws -> UInt64 {
                let (term, overflow) = pixels.multipliedReportingOverflow(by: perPixel)
                guard !overflow else { throw DoctoredCoreError(message: "High-depth working-memory calculation overflow.") }
                return try PrecisionDocumentDescriptor.sum(term, fixed)
            }
            func still(_ depth: Int) throws -> UInt64 {
                try charge(fixed: HostStillDecoder.scratchFixed(depth: depth), perPixel: HostStillDecoder.scratchPerPixel(depth: depth))
            }
            let term = item.role != "still" ? try charge(fixed: 0, perPixel: HostVideoDecoder.scratchPerPixel)
                : try item.depth.map(still) ?? max(still(8), still(16))
            total = try PrecisionDocumentDescriptor.sum(total, term)
        }
        return try PrecisionDocumentDescriptor.sum(total, 64 * 1024 * 1024)
    }
}

final class HostSourceSnapshot {
    static let maximumResourceBytes: UInt64 = 256 * 1024 * 1024
    let urls: [URL]
    let bytes: [Data]
    let hashes: [String]
    let lease: PrecisionEditPayload
    private let directory: URL

    private init(directory: URL, urls: [URL], bytes: [Data], hashes: [String], lease: PrecisionEditPayload) {
        self.directory = directory; self.urls = urls; self.bytes = bytes; self.hashes = hashes
        self.lease = lease
    }
    deinit { try? FileManager.default.removeItem(at: directory) }

    static func size(_ url: URL) throws -> UInt64 {
        let info = try url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
        guard info.isRegularFile == true, let size = info.fileSize, size > 0,
              UInt64(size) <= maximumResourceBytes else {
            throw HostSourceError.failure("Choose a regular source file of at most 256 MiB.")
        }
        return UInt64(size)
    }
    static func capture(_ input: HostSourceInput) throws -> HostSourceSnapshot {
        // Reserve all captured originals before the first Data allocation; the
        // first member must stay accounted while the second file is copied.
        let lease = try PrecisionEditPayload.admitted(bytes: input.entries.reduce(UInt64(128 * 1024)) { $0 + $1.bytes })
        let directory = try folder()
        do {
            var urls: [URL] = [], bytes: [Data] = [], hashes: [String] = []
            for (index, entry) in input.entries.enumerated() {
                let url = directory.appendingPathComponent("\(index).\(entry.url.pathExtension)")
                let digest = try ImageSourceFile.fingerprint(entry.url, copyingTo: url, maximumBytes: entry.bytes)
                let captured = try Data(contentsOf: url, options: .uncached)
                guard captured.count == entry.bytes, HostSourceError.sha256(captured) == digest else {
                    throw HostSourceError.failure("The source changed during capture. Try again.")
                }
                urls.append(url); bytes.append(captured); hashes.append(digest)
            }
            return HostSourceSnapshot(directory: directory, urls: urls, bytes: bytes, hashes: hashes, lease: lease)
        } catch { try? FileManager.default.removeItem(at: directory); throw error }
    }
    private static func folder() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("doctored-host-source-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: false,
            attributes: [.posixPermissions: 0o700])
        return url
    }
}
