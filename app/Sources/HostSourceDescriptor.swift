import Foundation
import CoreMedia
import CryptoKit

// What one capture decoded: the media's sample contracts, the canvas the chosen frame is fitted to
// and the selection. It describes the decode for the place and open reports; nothing of it is
// stored in a document (2026-10-10: host media lands as an ordinary raster layer).

/// One capture's decoded media, canvas and selection.
struct HostSourceDescriptor: Codable, Equatable {
    var schema_version = 1
    var kind = "host_media"
    var state: HostSourceState
    var resources: [HostSourceResource]
}

struct HostSourceResource: Codable, Equatable {
    let id: String
    let media_type: String
    let sha256: String
    let byte_length: UInt64
}

struct HostSourceState: Codable, Equatable {
    static let currentPolicy = "integer-sdr-full-source-v1"
    var policy = currentPolicy
    var kind: String
    var pairing: String
    var canvas_width: Int
    var canvas_height: Int
    var media: [HostMediaContract]
    var selection: HostFrameSelection
}

struct HostMediaContract: Codable, Equatable {
    var resource_id: String
    var filename: String
    var format: String
    var decoder: HostDecoderIdentity
    var encoded_depth: Int?
    var encoded_precision_note: String
    var width: Int
    var height: Int
    var orientation: Int
    var track_id: Int32?
    var start: HostMediaTime?
    var duration: HostMediaTime?
    var format_sha256: String
    var decoded: HostDecodedContract
    /// The decode request a still needed beyond ImageIO's default HDR one:
    /// `sdr_base` for an 8-bit container whose gain map would otherwise make
    /// the decode HDR. Nil (omitted from JSON) otherwise, so descriptors saved
    /// before it existed still compare and round-trip unchanged. LAST stored
    /// property with a default, so the memberwise init call sites stay valid.
    var decode_request: String? = nil
}

extension HostMediaContract {
    static let sdrBase = "sdr_base"
    static let sdrBaseDisclosure = "The HDR gain map is not applied: the document holds the SDR base image. "
        + "The original file is not kept in the document."
}

extension HostSourceDescriptor {
    /// What the decode does not carry, stated once for every report
    /// that shows this source (open warnings, place replies).
    var disclosures: [String] {
        state.media.contains(where: { $0.decode_request == HostMediaContract.sdrBase })
            ? [HostMediaContract.sdrBaseDisclosure] : []
    }
}

struct HostDecoderIdentity: Codable, Equatable {
    let identifier: String
    let implementation: String
    let version: String

    static func current(_ identifier: String) -> Self {
        Self(identifier: identifier, implementation: "Apple system media frameworks",
             version: ProcessInfo.processInfo.operatingSystemVersionString)
    }
}

struct HostDecodedContract: Codable, Equatable {
    var width: Int
    var height: Int
    var storage_depth: Int
    var layout: String
    var alpha: String
    var profile_sha256: String
    var profile_name: String
    /// Every admitted host packing token with the storage depth and alpha
    /// policy it yields. `rgb10_packed_le` stores 10-bit codes but its OUTPUT
    /// depth is 16. `HostStillDecoder.layout` reads the same table.
    static let layouts: [String: (depth: Int, alpha: String)] = [
        "rgba8": (8, "straight"), "rgba8_premultiplied": (8, "premultiplied"),
        "rgb8": (8, "opaque"), "bgra8": (8, "straight"), "bgra8_opaque": (8, "opaque"),
        "rgba8_opaque": (8, "opaque"),
        "rgba16_le": (16, "straight"), "rgba16_le_premultiplied": (16, "premultiplied"),
        "rgb16_le": (16, "opaque"), "rgba16_le_opaque": (16, "opaque"),
        "rgb10_packed_le": (16, "opaque"),
        "argb16_be": (16, "straight"), "argb16_be_premultiplied": (16, "premultiplied"),
        "rgba16_le_left12": (16, "straight"), "rgba16_le_opaque_left12": (16, "opaque"),
        "rgba16_le_premultiplied_left12": (16, "premultiplied")]
    var valid: Bool {
        guard let expected = Self.layouts[layout] else { return false }
        return width > 0 && height > 0 && width <= RasterImage.maxResizePixels / height
            && expected.depth == storage_depth && expected.alpha == alpha && HostSourceError.validHash(profile_sha256)
            && !layout.isEmpty && layout.utf8.count < 64 && profile_name.utf8.count <= 1024
    }
}

struct HostMediaTime: Codable, Equatable {
    let value: Int64
    let timescale: Int32
    init(_ value: Int64, _ timescale: Int32) { self.value = value; self.timescale = timescale }
    init(_ time: CMTime) throws {
        guard time.isNumeric, !time.flags.contains(.hasBeenRounded), time.epoch == 0, time.timescale > 0 else {
            throw HostSourceError.failure("The movie has unsupported nonnumeric timing.")
        }
        value = time.value; timescale = time.timescale
    }
    var valid: Bool { timescale > 0 && abs(Double(value)) <= 9_007_199_254_740_991 }
    var seconds: Double { Double(value) / Double(timescale) }
    var cmTime: CMTime { CMTime(value: value, timescale: timescale) }
    static func == (lhs: Self, rhs: Self) -> Bool {
        guard lhs.valid, rhs.valid else { return lhs.value == rhs.value && lhs.timescale == rhs.timescale }
        return CMTimeCompare(lhs.cmTime, rhs.cmTime) == 0
    }
    static func seconds(_ seconds: Double) throws -> Self {
        guard seconds.isFinite, abs(seconds) <= 1_000_000 else { throw HostSourceError.failure("Frame time is invalid.") }
        // User seconds deliberately resolve to the nearest microsecond. Decoder
        // timestamps use the separate strict CMTime initializer and stay exact.
        return Self(Int64((seconds * 1_000_000).rounded()), 1_000_000)
    }
}

struct HostFrameSelection: Codable, Equatable {
    var kind: String
    var requested: HostMediaTime?
    var resolved: HostMediaTime?
    var sample_duration: HostMediaTime?
    var duration_origin: String?
    static let still = Self(kind: "still")
    static func video(_ requested: HostMediaTime) -> Self { Self(kind: "video", requested: requested) }
    var valid: Bool {
        if kind == "still" { return requested == nil && resolved == nil && sample_duration == nil && duration_origin == nil }
        guard kind == "video", let requested, let resolved, let sample_duration,
              requested.valid, resolved.valid, sample_duration.valid,
              ["decoder_sample", "next_pts", "track_end"].contains(duration_origin ?? "") else { return false }
        return sample_duration.seconds > 0 && CMTimeCompare(resolved.cmTime, requested.cmTime) <= 0
    }
}

enum HostSourceError {
    static func failure(_ message: String) -> DoctoredCoreError { DoctoredCoreError(message: message) }
    static func sha256(_ bytes: Data) -> String {
        SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
    }
    static func validHash(_ value: String) -> Bool {
        value.utf8.count == 64 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }
    static let unsupported = "This source needs a preserving decoder or HDR policy that is not supported yet. Its saved rendering remains intact."
}
