import Foundation

/// The protocol's framing on the helper's stdin: lines end at LF (a CR before it is
/// dropped), and a last line without one counts at end of input. It splits on the LF
/// byte only, never on the Unicode line separators a JSON string may hold unescaped,
/// and scans each chunk once, so a multi-megabyte tool result costs one pass.
public struct LineSplitter: Sendable {
    private var pending = Data()

    public init() {}

    /// The lines that `chunk` completes, without their terminators.
    public mutating func append(_ chunk: Data) -> [Data] {
        var lines: [Data] = []
        var start = chunk.startIndex
        while let newline = chunk[start...].firstIndex(of: 0x0A) {
            var line = pending
            pending = Data()
            line.append(chunk[start..<newline])
            lines.append(Self.trimmed(line))
            start = chunk.index(after: newline)
        }
        pending.append(chunk[start...])
        return lines
    }

    /// The last line, when the input ended without a newline.
    public mutating func finish() -> Data? {
        defer { pending = Data() }
        return pending.isEmpty ? nil : Self.trimmed(pending)
    }

    private static func trimmed(_ line: Data) -> Data {
        if line.last == 0x0D { return Data(line.dropLast()) }
        return Data(line)
    }
}
