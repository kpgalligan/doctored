import Foundation

/// The helper's diagnostics: stderr lines prefixed `doctored-agent: ` (stdout is the
/// protocol only; the app passes the helper's stderr through to its own).
public enum Log {
    public static func warn(_ message: String) {
        let lines = message.split(separator: "\n", omittingEmptySubsequences: false)
        let text = lines.map { "doctored-agent: \($0)\n" }.joined()
        // The throwing write: the old `write(_:)` raises an Objective-C exception on an
        // error, so an EPIPE on a stderr whose reader went away (SIGPIPE is ignored)
        // would abort the helper. A diagnostic that cannot be written is dropped.
        try? FileHandle.standardError.write(contentsOf: Data(text.utf8))
    }
}
