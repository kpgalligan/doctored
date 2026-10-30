import Foundation

/// Read only the bounded AVC sequence-header fields that establish sample
/// precision. An avc1 container tag alone cannot exclude High10/4:4:4 input.
enum HostAVCPrecision {
    static func requireByte420(_ data: Data) throws {
        _ = try configuration(data)
    }
    static func nalLengthBytes(_ data: Data) throws -> Int { try configuration(data).0 }
    static func configuredSPS(_ data: Data) throws -> [Data] { try configuration(data).1 }

    private static func configuration(_ data: Data) throws -> (Int, [Data]) {
        guard (7...65536).contains(data.count) else { throw refused() }
        let bytes = [UInt8](data)
        guard bytes[0] == 1, bytes[4] & 0xfc == 0xfc, bytes[5] & 0xe0 == 0xe0 else { throw refused() }
        var index = 6
        let count = Int(bytes[5] & 31)
        guard count > 0 else { throw refused() }
        let lengthBytes = Int(bytes[4] & 3) + 1
        guard lengthBytes != 3 else { throw refused() }
        var headers: [Data] = []
        for _ in 0..<count {
            guard index + 2 <= bytes.count else { throw refused() }
            let size = Int(bytes[index]) * 256 + Int(bytes[index + 1]); index += 2
            guard size > 4, size <= 4096, index + size <= bytes.count,
                  bytes[index] & 0x80 == 0, bytes[index] & 31 == 7 else { throw refused() }
            headers.append(Data(bytes[index..<(index + size)]))
            var rbsp: [UInt8] = [], zeros = 0
            for byte in bytes[(index + 1)..<(index + size)] {
                if zeros == 2 && byte == 3 { zeros = 0; continue }
                rbsp.append(byte); zeros = byte == 0 ? zeros + 1 : 0
            }
            var bits = Bits(bytes: rbsp)
            let profile = try bits.read(8)
            guard profile == Int(bytes[1]) else { throw refused() }
            _ = try bits.read(8); _ = try bits.read(8); _ = try bits.ue()
            if profile == 100 {
                let chroma = try bits.ue()
                guard chroma == 1, try bits.ue() == 0, try bits.ue() == 0 else { throw refused() }
            } else if ![66,77,88].contains(profile) { throw refused() }
            index += size
        }
        guard index < bytes.count else { throw refused() }
        let pictureCount = Int(bytes[index]); index += 1
        guard pictureCount > 0 else { throw refused() }
        for _ in 0..<pictureCount {
            guard index + 2 <= bytes.count else { throw refused() }
            let size = Int(bytes[index]) * 256 + Int(bytes[index + 1]); index += 2
            guard size >= 2, size <= 4096, index + size <= bytes.count,
                  bytes[index] & 0x80 == 0, bytes[index] & 31 == 8 else { throw refused() }
            index += size
        }
        if index < bytes.count {
            // High-profile avcC may carry the redundant precision fields.
            // Auxiliary SPS extensions are outside this fixed AVC policy.
            guard bytes[1] == 100, bytes.count - index == 4,
                  bytes[index] == 0xfd, bytes[index + 1] == 0xf8,
                  bytes[index + 2] == 0xf8, bytes[index + 3] == 0 else { throw refused() }
            index += 4
        }
        guard index == bytes.count else { throw refused() }
        return (lengthBytes, headers)
    }
    private static func refused() -> DoctoredCoreError {
        HostSourceError.failure("Only verified 8-bit 4:2:0 H.264 sequence headers are supported; no byte decode substitutes for a higher-depth source.")
    }
    private struct Bits {
        let bytes: [UInt8]
        var position = 0
        mutating func read(_ count: Int) throws -> Int {
            guard count >= 0, count <= 24, position + count <= bytes.count * 8 else { throw HostAVCPrecision.refused() }
            var value = 0
            for _ in 0..<count {
                value = (value << 1) | Int((bytes[position / 8] >> (7 - position % 8)) & 1)
                position += 1
            }
            return value
        }
        mutating func ue() throws -> Int {
            var zeros = 0
            while try read(1) == 0 { zeros += 1; guard zeros <= 16 else { throw HostAVCPrecision.refused() } }
            return (1 << zeros) - 1 + (try read(zeros))
        }
    }
}
