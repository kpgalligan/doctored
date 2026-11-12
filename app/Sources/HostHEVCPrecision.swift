import Foundation

/// The bounded `hvcC` fields that establish an HEVC track's sample precision
/// (ISO/IEC 14496-15 §8.3.3.1): chroma_format_idc and the luma and chroma bit
/// depths are stated there for the configured sequence, so no SPS needs
/// parsing; `HostHEVCPreflight` then requires every in-band SPS to be one of
/// the configured ones, byte for byte. 4:2:0 at 8 or 10 bits opens; anything
/// else refuses by name.
enum HostHEVCPrecision {
    struct Configuration {
        /// 8 or 10, luma and chroma alike.
        let depth: Int
        /// The NAL length prefix in a sample: 1, 2 or 4 bytes.
        let lengthBytes: Int
        /// The configured SPS NAL units, each with its two-byte header.
        let sps: [Data]
    }

    static func configuration(_ data: Data) throws -> Configuration {
        let bytes = [UInt8](data)
        guard (23...65536).contains(bytes.count), bytes[0] == 1 else {
            throw refused("The HEVC sample description is malformed.")
        }
        let chroma = Int(bytes[16] & 3)
        let luma = Int(bytes[17] & 7) + 8, chromaDepth = Int(bytes[18] & 7) + 8
        guard chroma == 1, luma == chromaDepth, luma == 8 || luma == 10 else {
            let format = ["monochrome", "4:2:0", "4:2:2", "4:4:4"][chroma]
            throw refused("This HEVC track is \(format) at \(luma)-bit luma and \(chromaDepth)-bit "
                + "chroma; HEVC movie frames open from 8-bit or 10-bit 4:2:0 only.")
        }
        let lengthBytes = Int(bytes[21] & 3) + 1
        guard lengthBytes != 3 else { throw refused("The HEVC NAL length size is invalid.") }
        var index = 23, sps: [Data] = []
        for _ in 0..<Int(bytes[22]) {
            guard index + 3 <= bytes.count else { throw refused("The HEVC parameter sets are truncated.") }
            let type = Int(bytes[index] & 0x3f)
            let count = Int(bytes[index + 1]) << 8 | Int(bytes[index + 2])
            index += 3
            for _ in 0..<count {
                guard index + 2 <= bytes.count else { throw refused("The HEVC parameter sets are truncated.") }
                let size = Int(bytes[index]) << 8 | Int(bytes[index + 1])
                index += 2
                guard size >= 2, index + size <= bytes.count, type != 33 || size <= 4096 else {
                    throw refused("An HEVC parameter set is truncated or oversized.")
                }
                if type == 33 { sps.append(Data(bytes[index..<(index + size)])) }
                index += size
            }
        }
        guard index == bytes.count, !sps.isEmpty, sps.count <= 16 else {
            throw refused("The HEVC sample description lacks its sequence parameter set.")
        }
        return Configuration(depth: luma, lengthBytes: lengthBytes, sps: sps)
    }

    static func refused(_ reason: String) -> DoctoredCoreError {
        HostSourceError.failure(reason + " No byte decode was used to replace an unverified source.")
    }
}
